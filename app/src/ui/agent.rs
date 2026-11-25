//! Side panel of a pipeline agent: live work, today's results, messages, history.

use bevy_egui::egui::{self, RichText, ScrollArea};
use chrono::Local;
use merces_core::{AgentId, AgentStatus};

use super::{AgentTab, UiState, agent_name, agent_role, cards, fmt_tokens, fmt_usd, t};
use crate::config::AppConfig;
use crate::scene::Selection;
use crate::state::Hub;
use crate::theme::{self, Theme};

pub fn status_chip(ui: &mut egui::Ui, th: &Theme, s: AgentStatus, lang: merces_agents::Lang) {
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

/// Returns the panel width in logical pixels.
pub fn panel(
    root: &mut egui::Ui,
    th: &Theme,
    cfg: &AppConfig,
    hub: &Hub,
    sel: &mut Selection,
    st: &mut UiState,
    agent: AgentId,
) -> f32 {
    let lang = cfg.lang();
    let view = hub.symbol(&cfg.active_symbol);
    let resp = egui::Panel::right("agent")
        .default_size(440.0)
        .size_range(360.0..=720.0)
        .frame(egui::Frame::new().fill(th.pal.panel).inner_margin(egui::Margin::same(20)))
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::eyebrow(th, format!("Agent {} · {}", agent.index() + 1, cfg.active_symbol)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::ghost_button(ui, RichText::new("✕").color(th.pal.muted)).on_hover_text("Échap").clicked() {
                        sel.agent = None;
                        sel.output = None;
                    }
                });
            });
            ui.label(theme::title(th, agent_name(agent, lang), 26.0));
            ui.label(RichText::new(agent_role(agent, lang)).color(th.pal.body));
            let av = view.map(|v| v.agent(agent).clone()).unwrap_or_default();
            ui.horizontal(|ui| {
                status_chip(ui, th, av.status, lang);
                let settings = cfg.engine.agent(agent);
                theme::chip(ui, settings.model.clone(), th.pal.muted, th.pal.fill);
                if let Some(u) = hub.usage.get(&agent) {
                    ui.label(
                        RichText::new(format!("{} tokens · {}", fmt_tokens(u.total_tokens()), fmt_usd(u.cost_usd)))
                            .size(12.0 * th.scale)
                            .color(th.pal.muted),
                    );
                }
            });
            ui.add_space(8.0);

            if sel.output.is_some() {
                st.agent_tab = AgentTab::Today;
            }
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (AgentTab::Live, t(lang, "En direct", "Live")),
                    (AgentTab::Today, t(lang, "Aujourd'hui", "Today")),
                    (AgentTab::Messages, t(lang, "Messages", "Messages")),
                    (AgentTab::History, t(lang, "Historique", "History")),
                ] {
                    if ui.selectable_label(st.agent_tab == tab, label).clicked() {
                        st.agent_tab = tab;
                        sel.output = None;
                    }
                }
            });
            ui.separator();

            ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(st.agent_tab == AgentTab::Live).show(ui, |ui| {
                match st.agent_tab {
                    AgentTab::Live => {
                        if av.activity.is_empty() && av.thinking.is_empty() && av.answer.is_empty() {
                            ui.label(RichText::new(t(lang, "Aucune activité pour l'instant. Lancez une analyse pour voir l'agent travailler.", "No activity yet. Run an analysis to watch the agent work.")).color(th.pal.muted));
                        }
                        for (when, text) in &av.activity {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(theme::mono(th, when.with_timezone(&Local).format("%H:%M:%S").to_string()).color(th.pal.muted));
                                ui.label(RichText::new(text).color(th.pal.body));
                            });
                        }
                        if !av.thinking.is_empty() {
                            ui.add_space(6.0);
                            ui.label(theme::eyebrow(th, t(lang, "Raisonnement", "Reasoning")));
                            theme::card(th).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(&av.thinking).color(th.pal.body));
                                if av.status == AgentStatus::Working {
                                    ui.spinner();
                                }
                            });
                        } else if av.status == AgentStatus::Working {
                            ui.spinner();
                        }
                        if !av.answer.is_empty() {
                            ui.collapsing(t(lang, "Réponse brute (JSON)", "Raw answer (JSON)"), |ui| {
                                ui.label(theme::mono(th, &av.answer).size(12.0 * th.scale));
                            });
                        }
                    }
                    AgentTab::Today => {
                        let outputs: Vec<_> = view.map(|v| v.today(agent).collect()).unwrap_or_default();
                        if outputs.is_empty() {
                            ui.label(RichText::new(t(lang, "Pas encore de résultat aujourd'hui pour ce symbole.", "No result yet today for this symbol.")).color(th.pal.muted));
                        }
                        for (i, o) in outputs.into_iter().enumerate() {
                            let highlight = sel.output == Some(i);
                            let frame = theme::card(th).fill(if highlight { th.pal.selected } else { th.pal.faint });
                            let r = frame.show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(theme::eyebrow(th, o.at.with_timezone(&Local).format("%H:%M").to_string()));
                                let signal = hub.signals.values().find(|s| s.run_id == o.run_id);
                                cards::output(ui, th, &o.output, lang, signal);
                            });
                            if highlight {
                                r.response.scroll_to_me(Some(egui::Align::TOP));
                            }
                            ui.add_space(8.0);
                        }
                    }
                    AgentTab::Messages => {
                        let msgs: Vec<_> = view
                            .map(|v| v.messages.iter().rev().filter(|m| m.from == agent || m.to == agent).collect())
                            .unwrap_or_default();
                        if msgs.is_empty() {
                            ui.label(RichText::new(t(lang, "Aucun message échangé.", "No messages exchanged.")).color(th.pal.muted));
                        }
                        for m in msgs {
                            theme::card(th).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                let incoming = m.to == agent;
                                ui.horizontal(|ui| {
                                    ui.label(theme::mono(th, m.at.with_timezone(&Local).format("%H:%M").to_string()).color(th.pal.muted));
                                    let arrow = if incoming { "←" } else { "→" };
                                    let other = if incoming { m.from } else { m.to };
                                    ui.label(RichText::new(format!("{arrow} {}", agent_name(other, lang))).color(th.pal.text));
                                });
                                ui.label(RichText::new(&m.summary).color(th.pal.body));
                            });
                            ui.add_space(6.0);
                        }
                    }
                    AgentTab::History => {
                        let mut all: Vec<(&String, &crate::state::OutputRecord)> = hub
                            .symbols
                            .iter()
                            .flat_map(|(s, v)| v.outputs.iter().filter(|o| o.agent == agent).map(move |o| (s, o)))
                            .collect();
                        all.sort_by_key(|o| std::cmp::Reverse(o.1.at));
                        if all.is_empty() {
                            ui.label(RichText::new(t(lang, "Aucun historique.", "No history.")).color(th.pal.muted));
                        }
                        egui::Grid::new("history").num_columns(3).striped(false).spacing([12.0, 6.0]).show(ui, |ui| {
                            for (s, o) in all.into_iter().take(100) {
                                ui.label(theme::mono(th, o.at.with_timezone(&Local).format("%d/%m %H:%M").to_string()).color(th.pal.muted));
                                ui.label(RichText::new(s).color(th.pal.text));
                                ui.label(RichText::new(cards::one_line(&o.output, lang)).color(th.pal.body));
                                ui.end_row();
                            }
                        });
                    }
                }
            });
        });
    resp.response.rect.width()
}
