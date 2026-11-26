//! What a pipeline agent did, shown in its fold of the left drawer. Two tabs:
//! - "Live" while an analysis runs (timeline, streamed reasoning), which
//!   becomes "Today" once it is over (today's results, then the last run's
//!   timeline folded away);
//! - "History" (every result of this agent, all symbols).

use bevy_egui::egui::{self, RichText, ScrollArea};
use chrono::{DateTime, Local, Utc};
use merces_agents::Lang;
use merces_core::{AgentId, AgentStatus};

use super::{AgentTab, UiState, agent_name, agent_role, cards, t};
use crate::config::AppConfig;
use crate::scene::Selection;
use crate::state::{AgentView, Hub, SymbolView};
use crate::theme::{self, Theme};

pub fn status_chip(ui: &mut egui::Ui, th: &Theme, s: AgentStatus, lang: Lang) {
    let (label, fg) = match s {
        AgentStatus::Idle => (t(lang, "En veille", "Idle"), th.pal.muted),
        AgentStatus::Working => (t(lang, "Au travail", "Working"), th.pal.accent),
        AgentStatus::Done => (t(lang, "Terminé", "Done"), th.pal.success),
        AgentStatus::NoTrade => (
            t(lang, "Rien aujourd'hui", "No trade today"),
            th.pal.warning,
        ),
        AgentStatus::Error => (t(lang, "Erreur", "Error"), th.pal.error),
    };
    theme::chip(ui, label, fg, th.pal.fill);
}

/// Content of an agent's fold in the left drawer: its role, then "Live"
/// while an analysis runs (timeline, streamed reasoning), which becomes
/// "Today" once it is over, and "History" (every result, all symbols).
pub fn body(
    ui: &mut egui::Ui,
    th: &Theme,
    cfg: &AppConfig,
    hub: &Hub,
    sel: &mut Selection,
    st: &mut UiState,
    agent: AgentId,
) {
    let lang = cfg.lang();
    let view = hub.symbol(&cfg.active_symbol);
    let running = view.is_some_and(|v| v.running.is_some());
    let av = view.map(|v| v.agent(agent).clone()).unwrap_or_default();
    ui.label(
        RichText::new(agent_role(agent, lang))
            .size(12.0 * th.scale)
            .color(th.pal.muted),
    );
    let slot = agent.index().min(3);
    if sel.output.is_some() && sel.agent == Some(agent) {
        st.agent_tabs[slot] = AgentTab::Now;
    }
    ui.horizontal(|ui| {
        let now = if running {
            t(lang, "En direct", "Live")
        } else {
            t(lang, "Aujourd'hui", "Today")
        };
        for (tab, label) in [
            (AgentTab::Now, now),
            (AgentTab::History, t(lang, "Historique", "History")),
        ] {
            if ui
                .selectable_label(st.agent_tabs[slot] == tab, label)
                .clicked()
            {
                st.agent_tabs[slot] = tab;
                sel.output = None;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            theme::chip(
                ui,
                cfg.engine.agent(agent).model.clone(),
                th.pal.muted,
                th.pal.fill,
            );
        });
    });
    ui.separator();
    match st.agent_tabs[slot] {
        AgentTab::Now if running => {
            if let Some(v) = view {
                live(ui, th, v, &av, agent, lang);
            }
        }
        AgentTab::Now => {
            today(ui, th, hub, view, sel, agent, lang);
            if let Some(v) = view
                && !(av.activity.is_empty() && av.thinking.is_empty())
            {
                ui.add_space(4.0);
                egui::CollapsingHeader::new(t(
                    lang,
                    "Déroulé de la dernière analyse",
                    "How the last analysis went",
                ))
                .id_salt(("last-run", agent))
                .show(ui, |ui| live(ui, th, v, &av, agent, lang));
            }
        }
        AgentTab::History => history(ui, th, hub, agent, lang),
    }
}

/// Today's results of the agent, newest first.
fn today(
    ui: &mut egui::Ui,
    th: &Theme,
    hub: &Hub,
    view: Option<&SymbolView>,
    sel: &Selection,
    agent: AgentId,
    lang: Lang,
) {
    let mut any = false;
    for (i, o) in view.into_iter().flat_map(|v| v.today(agent)).enumerate() {
        any = true;
        let highlight = sel.output == Some(i);
        let frame = theme::card(th).fill(if highlight {
            th.pal.selected
        } else {
            th.pal.faint
        });
        // One id scope per card: their folds ("Sources"…) share labels.
        let r = ui
            .push_id(("result", i), |ui| {
                frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(theme::eyebrow(
                        th,
                        o.at.with_timezone(&Local).format("%H:%M").to_string(),
                    ));
                    let signal = hub.signals.values().find(|s| s.run_id == o.run_id);
                    cards::output(ui, th, &o.output, lang, signal);
                })
            })
            .inner;
        if highlight {
            r.response.scroll_to_me(Some(egui::Align::TOP));
        }
        ui.add_space(8.0);
    }
    if !any {
        ui.label(
            RichText::new(t(
                lang,
                "Pas encore de résultat aujourd'hui pour ce symbole. Lancez une analyse pour voir l'agent travailler.",
                "No result yet today for this symbol. Run an analysis to watch the agent work.",
            ))
            .color(th.pal.muted),
        );
    }
}

/// What the agent did during the run: tool use and hand-offs in time order,
/// then its streamed reasoning.
fn live(ui: &mut egui::Ui, th: &Theme, v: &SymbolView, av: &AgentView, agent: AgentId, lang: Lang) {
    // Messages of the last run only (they are kept for the whole session).
    let since: DateTime<Utc> = v.run_started.unwrap_or_default();
    let mut acts = av.activity.iter().peekable();
    let mut msgs = v
        .messages
        .iter()
        .filter(|m| (m.to == agent || m.from == agent) && m.at >= since)
        .peekable();
    if acts.peek().is_none() && msgs.peek().is_none() && av.thinking.is_empty() {
        let text = if av.status == AgentStatus::Idle {
            t(lang, "En attente de son tour.", "Waiting for its turn.")
        } else {
            t(lang, "Aucune activité pour l'instant.", "No activity yet.")
        };
        ui.label(RichText::new(text).color(th.pal.muted));
    }
    loop {
        let take_act = match (acts.peek(), msgs.peek()) {
            (Some(a), Some(m)) => a.0 <= m.at,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        ui.horizontal_wrapped(|ui| {
            if take_act {
                let (when, text) = acts.next().expect("peeked");
                ui.label(
                    theme::mono(
                        th,
                        when.with_timezone(&Local).format("%H:%M:%S").to_string(),
                    )
                    .color(th.pal.muted),
                );
                ui.label(RichText::new(text).color(th.pal.body));
            } else {
                let m = msgs.next().expect("peeked");
                ui.label(
                    theme::mono(
                        th,
                        m.at.with_timezone(&Local).format("%H:%M:%S").to_string(),
                    )
                    .color(th.pal.muted),
                );
                let incoming = m.to == agent;
                let (arrow, other) = if incoming {
                    ("←", m.from)
                } else {
                    ("→", m.to)
                };
                ui.label(
                    RichText::new(format!("{arrow} {}", agent_name(other, lang)))
                        .color(th.pal.text),
                );
                ui.label(RichText::new(&m.summary).color(th.pal.body));
            }
        });
    }
    if !av.thinking.is_empty() {
        ui.add_space(6.0);
        ui.label(theme::eyebrow(th, t(lang, "Raisonnement", "Reasoning")));
        theme::card(th).show(ui, |ui| {
            ui.set_width(ui.available_width());
            // One label per paragraph: while the text streams in, only the
            // last paragraph changes and needs a new layout.
            for para in av.thinking.split("\n\n") {
                if !para.trim().is_empty() {
                    ui.label(RichText::new(para).color(th.pal.body));
                }
            }
            if av.status == AgentStatus::Working {
                ui.spinner();
            }
        });
    } else if av.status == AgentStatus::Working {
        ui.spinner();
    }
    if !av.answer.is_empty() {
        egui::CollapsingHeader::new(t(lang, "Réponse brute (JSON)", "Raw answer (JSON)"))
            .id_salt(("raw", agent))
            .show(ui, |ui| {
                ui.label(theme::mono(th, &av.answer).size(12.0 * th.scale));
            });
    }
}

/// Every result of the agent, all symbols, newest first. Only the visible rows
/// are laid out.
fn history(ui: &mut egui::Ui, th: &Theme, hub: &Hub, agent: AgentId, lang: Lang) {
    let mut all: Vec<(&String, &crate::state::OutputRecord)> = hub
        .symbols
        .iter()
        .flat_map(|(s, v)| {
            v.outputs
                .iter()
                .filter(|o| o.agent == agent)
                .map(move |o| (s, o))
        })
        .collect();
    all.sort_by_key(|o| std::cmp::Reverse(o.1.at));
    if all.is_empty() {
        ui.label(RichText::new(t(lang, "Aucun historique.", "No history.")).color(th.pal.muted));
        return;
    }
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y;
    ScrollArea::vertical()
        .id_salt(("history", agent))
        .max_height(320.0)
        .auto_shrink([false, true])
        .show_rows(ui, row_h, all.len(), |ui, range| {
            for (s, o) in &all[range] {
                ui.horizontal(|ui| {
                    ui.label(
                        theme::mono(
                            th,
                            o.at.with_timezone(&Local).format("%d/%m %H:%M").to_string(),
                        )
                        .color(th.pal.muted),
                    );
                    ui.label(RichText::new(*s).color(th.pal.text));
                    ui.add(
                        egui::Label::new(
                            RichText::new(cards::one_line(&o.output, lang)).color(th.pal.body),
                        )
                        .truncate(),
                    );
                });
            }
        });
}
