//! The assistant chat, in its fold of the right drawer (also unfolded by
//! clicking the assistant in the scene).

use bevy_egui::egui::{self, Key, RichText, ScrollArea};
use merces_agents::{ChatTurn, Command};
use merces_core::AgentId;

use super::{UiState, agent_role, md, t};
use crate::bridge::Bridge;
use crate::config::AppConfig;
use crate::state::Hub;
use crate::theme::{self, Theme};

pub fn body(
    ui: &mut egui::Ui,
    th: &Theme,
    cfg: &AppConfig,
    hub: &mut Hub,
    st: &mut UiState,
    bridge: &Bridge,
) {
    let lang = cfg.lang();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(agent_role(AgentId::Assistant, lang))
                .size(12.0 * th.scale)
                .color(th.pal.muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
    ScrollArea::vertical()
        .id_salt("chat")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .max_height(360.0)
        .show(ui, |ui| {
            if hub.chat.is_empty() {
                ui.label(RichText::new(t(lang, "Exemples :", "Examples:")).color(th.pal.muted));
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
    let enter = r.has_focus() && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift);
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
}
