//! Top bar (symbol, price, run button, view, windows) and bottom status bar.

use bevy_egui::egui::{self, Align, Layout, Margin, RichText};
use chrono::{Local, Utc};
use merces_agents::Command;
use merces_core::AgentId;

use super::{UiState, fmt_usd, t};
use crate::bridge::Bridge;
use crate::config::{AppConfig, ViewMode};
use crate::scene::Selection;
use crate::state::Hub;
use crate::theme::{self, Theme};

pub fn top(
    root: &mut egui::Ui,
    th: &Theme,
    cfg: &mut AppConfig,
    hub: &mut Hub,
    sel: &mut Selection,
    st: &mut UiState,
    bridge: &Bridge,
) {
    let lang = cfg.lang();
    egui::Panel::top("top")
        .frame(egui::Frame::new().fill(th.pal.panel).inner_margin(Margin::symmetric(16, 10)))
        .show_separator_line(false)
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::title(th, "Merces", 22.0));
                ui.add_space(4.0);
                ui.label(theme::eyebrow(th, t(lang, "Agents de trading", "Trading agents")));
                ui.add_space(16.0);

                // Active symbol.
                let current = cfg.active_symbol.clone();
                let label = cfg
                    .engine
                    .symbol(&current)
                    .map(|s| format!("{} · {}", s.symbol, s.name))
                    .unwrap_or_else(|| t(lang, "Aucun symbole", "No symbol").to_owned());
                egui::ComboBox::from_id_salt("symbol").selected_text(label).width(200.0).show_ui(ui, |ui| {
                    for s in &cfg.engine.symbols {
                        ui.selectable_value(&mut cfg.active_symbol, s.symbol.clone(), format!("{} · {}", s.symbol, s.name));
                    }
                });
                if let Some(p) = hub.prices.get(&current) {
                    ui.label(theme::mono(th, merces_agents_fmt(*p)).size(15.0 * th.scale));
                }

                // Other symbols being analysed.
                for s in hub.running() {
                    let fg = th.pal.on_accent;
                    if theme::chip(ui, format!("⟳ {s}"), fg, th.pal.accent).interact(egui::Sense::click()).clicked() {
                        cfg.active_symbol = s;
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    for (open, label) in [
                        (&mut st.settings, t(lang, "Réglages", "Settings")),
                        (&mut st.journal, t(lang, "Journal", "Journal")),
                        (&mut st.symbols, t(lang, "Symboles", "Symbols")),
                    ] {
                        let text = if *open { RichText::new(label).color(th.pal.text).underline() } else { RichText::new(label).color(th.pal.body) };
                        if theme::ghost_button(ui, text).clicked() {
                            *open = !*open;
                        }
                    }
                    let chat_open = sel.agent == Some(AgentId::Assistant);
                    let text = RichText::new(t(lang, "Assistant", "Assistant")).color(if chat_open { th.pal.text } else { th.pal.body });
                    if theme::ghost_button(ui, text).clicked() {
                        sel.agent = if chat_open { None } else { Some(AgentId::Assistant) };
                    }
                    ui.separator();

                    // View switch (segmented).
                    let mut view = cfg.scene.view;
                    ui.selectable_value(&mut view, ViewMode::Graph, t(lang, "Graphe", "Graph"));
                    ui.selectable_value(&mut view, ViewMode::Solar, t(lang, "Système", "System"));
                    cfg.scene.view = view;
                    ui.separator();

                    let budget = cfg.engine.daily_budget_usd;
                    let spent = hub.spent_today();
                    let cost = if budget > 0.0 { format!("{} / {}", fmt_usd(spent), fmt_usd(budget)) } else { fmt_usd(spent) };
                    ui.label(theme::mono(th, cost).color(if budget > 0.0 && spent >= budget { th.pal.error } else { th.pal.muted }))
                        .on_hover_text(t(lang, "Coût Claude du jour / budget", "Today's Claude cost / budget"));

                    let running = hub.symbol(&current).is_some_and(|v| v.running.is_some());
                    if running {
                        if ui.button(t(lang, "Annuler", "Cancel")).clicked() {
                            bridge.send(Command::Cancel(Some(current.clone())));
                        }
                        ui.spinner();
                    } else if theme::primary_button(ui, th, t(lang, "Lancer l'analyse", "Run analysis")).clicked() {
                        if !cfg.demo_mode && !bridge.has_claude_key() {
                            hub.toast(
                                crate::state::ToastKind::Error,
                                t(lang, "Ajoutez votre clé Anthropic (Réglages → Données) ou activez le mode démo.", "Add your Anthropic key (Settings → Data) or turn on demo mode."),
                            );
                            st.settings = true;
                            st.settings_tab = super::SettingsTab::Data;
                        } else if !current.is_empty() {
                            bridge.send(Command::Run(current.clone()));
                        }
                    }
                });
            });
        });
}

fn merces_agents_fmt(p: f64) -> String {
    if p >= 1000.0 {
        format!("{p:.2}")
    } else if p >= 1.0 {
        format!("{p:.4}")
    } else {
        format!("{p:.6}")
    }
}

pub fn bottom(root: &mut egui::Ui, th: &Theme, cfg: &AppConfig, hub: &Hub, bridge: &Bridge) {
    let lang = cfg.lang();
    egui::Panel::bottom("bottom")
        .frame(egui::Frame::new().fill(th.pal.panel).inner_margin(Margin::symmetric(16, 6)))
        .show_separator_line(false)
        .show(root, |ui| {
            ui.horizontal(|ui| {
                if cfg.demo_mode {
                    theme::chip(ui, t(lang, "Mode démo", "Demo mode"), th.pal.on_inverse, th.pal.inverse);
                } else if !bridge.has_claude_key() {
                    theme::chip(ui, t(lang, "Clé Anthropic manquante", "Anthropic key missing"), th.pal.on_status, th.pal.error);
                }
                ui.label(
                    RichText::new(t(
                        lang,
                        "Aide à la décision, pas un conseil financier. Aucun ordre n'est passé : les signaux sont suivis sur papier.",
                        "Decision support, not financial advice. No order is placed: signals are tracked on paper.",
                    ))
                    .size(12.0 * th.scale)
                    .color(th.pal.muted),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(s) = cfg.engine.symbol(&cfg.active_symbol) {
                        let next = match s.schedule_utc {
                            Some(at) if s.enabled => {
                                let now = Utc::now();
                                let mut due = now.date_naive().and_time(at).and_utc();
                                let ran_today = hub
                                    .symbol(&s.symbol)
                                    .and_then(|v| v.last_outcome.as_ref())
                                    .is_some_and(|(when, _)| *when >= due);
                                if now >= due && ran_today {
                                    due += chrono::Duration::days(1);
                                }
                                format!(
                                    "{} {}",
                                    t(lang, "Prochaine analyse :", "Next analysis:"),
                                    due.with_timezone(&Local).format("%d/%m %H:%M")
                                )
                            }
                            _ => t(lang, "Analyse manuelle", "Manual analysis").to_owned(),
                        };
                        ui.label(RichText::new(next).size(12.0 * th.scale).color(th.pal.muted));
                    }
                });
            });
        });
}
