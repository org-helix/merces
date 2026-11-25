//! The assistant chat (side panel, opened from its planet or the top bar).

use bevy_egui::egui::{self, Key, RichText, ScrollArea};
use merces_agents::{ChatTurn, Command};
use merces_core::AgentId;

use super::{UiState, agent_role, md, t};
use crate::bridge::Bridge;
use crate::config::AppConfig;
use crate::scene::Selection;
use crate::state::Hub;
use crate::theme::{self, Theme};

pub fn panel(
    root: &mut egui::Ui,
    th: &Theme,
    cfg: &AppConfig,
    hub: &mut Hub,
    sel: &mut Selection,
    st: &mut UiState,
    bridge: &Bridge,
) -> f32 {
    let lang = cfg.lang();
    let resp = egui::Panel::right("assistant")
        .default_size(420.0)
        .size_range(340.0..=720.0)
        .frame(
            egui::Frame::new()
                .fill(th.pal.panel)
                .inner_margin(egui::Margin::same(20)),
        )
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::eyebrow(
                    th,
                    t(lang, "Hors pipeline", "Outside the pipeline"),
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::ghost_button(ui, RichText::new("✕").color(th.pal.muted)).clicked() {
                        sel.agent = None;
                    }
                    if !hub.chat.is_empty()
                        && theme::ghost_button(
                            ui,
                            RichText::new(t(lang, "Effacer", "Clear")).color(th.pal.muted),
                        )
                        .clicked()
                    {
                        hub.chat.clear();
                    }
                });
            });
            ui.label(theme::title(th, t(lang, "Assistant", "Assistant"), 26.0));
            ui.label(RichText::new(agent_role(AgentId::Assistant, lang)).color(th.pal.body));
            ui.add_space(8.0);

            let input_h = 96.0;
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .max_height(ui.available_height() - input_h)
                .show(ui, |ui| {
                    if hub.chat.is_empty() {
                        ui.label(
                            RichText::new(t(lang, "Exemples :", "Examples:")).color(th.pal.muted),
                        );
                        for ex in [
                            t(
                                lang,
                                "Comment ajouter l'or (XAU/USD) ?",
                                "How do I add gold (XAU/USD)?",
                            ),
                            t(
                                lang,
                                "Explique le résultat de l'agent de décision.",
                                "Explain the decision agent's result.",
                            ),
                            t(
                                lang,
                                "Qu'est-ce qu'un ratio risque/rendement ?",
                                "What is a reward/risk ratio?",
                            ),
                        ] {
                            if ui.link(ex).clicked() {
                                st.chat_input = ex.to_owned();
                            }
                        }
                    }
                    for turn in &hub.chat {
                        let frame = if turn.user {
                            egui::Frame::new()
                                .fill(th.pal.selected)
                                .corner_radius(egui::CornerRadius::same(12))
                                .inner_margin(egui::Margin::same(10))
                        } else {
                            theme::card(th)
                        };
                        ui.with_layout(
                            if turn.user {
                                egui::Layout::top_down(egui::Align::Max)
                            } else {
                                egui::Layout::top_down(egui::Align::Min)
                            },
                            |ui| {
                                frame.show(ui, |ui| {
                                    ui.set_max_width(ui.available_width() * 0.9);
                                    if turn.user {
                                        ui.label(RichText::new(&turn.text).color(th.pal.text));
                                    } else {
                                        md::show(ui, th, &turn.text);
                                    }
                                });
                            },
                        );
                        ui.add_space(6.0);
                    }
                    if hub.chat_busy {
                        ui.spinner();
                    }
                });

            ui.separator();
            let edit = egui::TextEdit::multiline(&mut st.chat_input)
                .hint_text(t(
                    lang,
                    "Posez une question… (Entrée pour envoyer, Maj+Entrée pour une nouvelle ligne)",
                    "Ask a question… (Enter to send, Shift+Enter for a new line)",
                ))
                .desired_rows(2)
                .desired_width(f32::INFINITY);
            let r = ui.add(edit);
            let enter =
                r.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);
            ui.horizontal(|ui| {
                let can = !hub.chat_busy && !st.chat_input.trim().is_empty();
                let clicked = ui
                    .add_enabled(can, egui::Button::new(t(lang, "Envoyer", "Send")))
                    .clicked();
                if (clicked || enter) && can {
                    let text = st.chat_input.trim().to_owned();
                    st.chat_input.clear();
                    hub.chat.push(ChatTurn { user: true, text });
                    hub.chat_busy = true;
                    bridge.send(Command::Ask(hub.chat.clone()));
                }
                if cfg.demo_mode {
                    ui.label(
                        RichText::new(t(
                            lang,
                            "Mode démo : réponses simulées",
                            "Demo mode: simulated answers",
                        ))
                        .size(12.0 * th.scale)
                        .color(th.pal.muted),
                    );
                }
            });
            if enter {
                // Remove the newline the Enter key typed.
                while st.chat_input.ends_with('\n') {
                    st.chat_input.pop();
                }
            }
        });
    resp.response.rect.width()
}
