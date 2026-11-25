//! Things drawn over the scene: planet labels, the overview card, toasts and
//! the assistant's symbol proposals.

use bevy::prelude::{Camera, GlobalTransform, Vec3};
use bevy_egui::egui::{self, Align2, FontId, Pos2, RichText};
use merces_agents::Command;
use merces_core::{AgentId, AgentOutput, AgentStatus, RunOutcome};

use super::{agent_name, cards, t};
use crate::bridge::Bridge;
use crate::config::AppConfig;
use crate::scene::{Layout, Selection};
use crate::state::{Hub, ToastKind};
use crate::theme::{self, Theme};

#[allow(clippy::too_many_arguments)]
pub fn labels(
    ctx: &egui::Context,
    th: &Theme,
    cfg: &AppConfig,
    hub: &Hub,
    sel: &Selection,
    lay: &Layout,
    cam: &Camera,
    gt: &GlobalTransform,
) {
    let lang = cfg.lang();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("labels"),
    ));
    let scale = ctx.pixels_per_point();
    let view = hub.symbol(&cfg.active_symbol);
    let project = |p: Vec3| {
        cam.world_to_viewport(gt, p)
            .ok()
            .map(|v| Pos2::new(v.x / scale, v.y / scale))
    };
    let font = FontId::new(13.0 * th.scale, theme::medium());
    for a in AgentId::ALL {
        let pos = lay.agents[a.index()];
        let Some(at) = project(pos - Vec3::Y * (lay.sizes[a.index()] + 0.35)) else {
            continue;
        };
        let status = if a == AgentId::Assistant {
            if hub.chat_busy {
                AgentStatus::Working
            } else {
                AgentStatus::Idle
            }
        } else {
            view.map(|v| v.agent(a).status).unwrap_or_default()
        };
        let dot = match status {
            AgentStatus::Idle => th.pal.muted,
            AgentStatus::Working => th.pal.accent,
            AgentStatus::Done => th.pal.success,
            AgentStatus::NoTrade => th.pal.warning,
            AgentStatus::Error => th.pal.error,
        };
        let strong = sel.agent == Some(a) || sel.hovered == Some(a);
        let color = if strong { th.pal.text } else { th.pal.body };
        let rect = painter.text(
            at + egui::vec2(6.0, 0.0),
            Align2::CENTER_TOP,
            agent_name(a, lang),
            font.clone(),
            color,
        );
        painter.circle_filled(Pos2::new(rect.left() - 8.0, rect.center().y), 3.5, dot);
    }
    if let Some(at) = project(lay.sun - Vec3::Y * (lay.sun_size + 0.5)) {
        let price = hub
            .prices
            .get(&cfg.active_symbol)
            .map(|p| cards::fmt_price(*p))
            .unwrap_or_default();
        painter.text(
            at,
            Align2::CENTER_TOP,
            &cfg.active_symbol,
            FontId::new(16.0 * th.scale, theme::display()),
            th.pal.text,
        );
        painter.text(
            at + egui::vec2(0.0, 20.0 * th.scale),
            Align2::CENTER_TOP,
            price,
            FontId::new(12.0 * th.scale, egui::FontFamily::Monospace),
            th.pal.muted,
        );
    }
}

/// Bottom-left card: today's result for the active symbol.
pub fn overview(ctx: &egui::Context, th: &Theme, cfg: &AppConfig, hub: &Hub, sel: &mut Selection) {
    let lang = cfg.lang();
    egui::Area::new(egui::Id::new("overview"))
        .anchor(Align2::LEFT_BOTTOM, egui::vec2(16.0, -60.0))
        .show(ctx, |ui| {
            theme::plate(th).show(ui, |ui| {
                ui.set_max_width(360.0);
                ui.label(theme::eyebrow(th, format!("{} · {}", t(lang, "Aujourd'hui", "Today"), cfg.active_symbol)));
                let view = hub.symbol(&cfg.active_symbol);
                match view {
                    Some(v) if v.running.is_some() => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            let working = AgentId::PIPELINE.into_iter().find(|a| v.agent(*a).status == AgentStatus::Working);
                            let text = match working {
                                Some(a) => format!("{} {}", agent_name(a, lang), t(lang, "travaille…", "is working…")),
                                None => t(lang, "Analyse en cours…", "Analysis running…").to_owned(),
                            };
                            ui.label(theme::title(th, text, 18.0));
                        });
                    }
                    Some(v) => {
                        let decision = v.today(AgentId::Decision).next();
                        let plan = v.today(AgentId::Execution).next();
                        match (&v.last_outcome, decision) {
                            (_, Some(d)) => {
                                if let AgentOutput::Decision(dec) = &d.output {
                                    ui.label(theme::title(th, cards::action_label(dec.action, lang), 20.0));
                                }
                                if let Some(p) = plan.filter(|p| p.run_id == d.run_id) {
                                    ui.label(theme::mono(th, cards::one_line(&p.output, lang)));
                                }
                            }
                            (Some((_, RunOutcome::Failed { error })), None) => {
                                ui.label(RichText::new(error).color(th.pal.error));
                            }
                            _ => {
                                ui.label(theme::title(th, t(lang, "Pas encore d'analyse", "No analysis yet"), 18.0));
                            }
                        }
                        if let Some(b) = v.today(AgentId::Research).next() {
                            ui.label(RichText::new(cards::one_line(&b.output, lang)).color(th.pal.body));
                        }
                    }
                    None => {
                        ui.label(theme::title(th, t(lang, "Pas encore d'analyse", "No analysis yet"), 18.0));
                    }
                }
                ui.add_space(4.0);
                ui.label(
                    RichText::new(t(
                        lang,
                        "Cliquez une planète pour voir le travail de l'agent. Glisser : tourner · molette : zoom.",
                        "Click a planet to see the agent's work. Drag: rotate · wheel: zoom.",
                    ))
                    .size(12.0 * th.scale)
                    .color(th.pal.muted),
                );
                if ui.link(t(lang, "Ouvrir l'assistant", "Open the assistant")).clicked() {
                    sel.agent = Some(AgentId::Assistant);
                }
            });
        });
}

pub fn toasts(ctx: &egui::Context, th: &Theme, hub: &Hub) {
    if hub.toasts.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::new("toasts"))
        .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 72.0))
        .interactable(false)
        .show(ctx, |ui| {
            for toast in &hub.toasts {
                let (fill, fg) = match toast.kind {
                    ToastKind::Error => (th.pal.error, th.pal.on_status),
                    ToastKind::Success => (th.pal.success, th.pal.on_status),
                    ToastKind::Info => (th.pal.inverse, th.pal.on_inverse),
                };
                egui::Frame::new()
                    .fill(fill)
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.set_max_width(520.0);
                        ui.label(RichText::new(&toast.text).color(fg));
                    });
                ui.add_space(6.0);
            }
        });
}

pub fn proposals(
    ctx: &egui::Context,
    th: &Theme,
    cfg: &mut AppConfig,
    hub: &mut Hub,
    bridge: &Bridge,
) {
    let Some(p) = hub.proposals.first().cloned() else {
        return;
    };
    let lang = cfg.lang();
    egui::Window::new(t(
        lang,
        "Proposition de l'assistant",
        "Assistant's proposal",
    ))
    .collapsible(false)
    .resizable(false)
    .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
    .show(ctx, |ui| {
        ui.label(theme::title(th, format!("{} · {}", p.symbol, p.name), 18.0));
        ui.label(
            RichText::new(format!("{:?} · {}", p.class, p.provider.label())).color(th.pal.muted),
        );
        let exists = cfg.engine.symbol(&p.symbol).is_some();
        if exists {
            ui.label(
                RichText::new(t(
                    lang,
                    "Ce symbole est déjà suivi.",
                    "This symbol is already tracked.",
                ))
                .color(th.pal.warning),
            );
        }
        ui.horizontal(|ui| {
            if !exists && theme::primary_button(ui, th, t(lang, "Ajouter", "Add")).clicked() {
                cfg.engine.symbols.push(p.clone());
                hub.proposals.remove(0);
                bridge.send(Command::Configure(cfg.engine.clone()));
            } else if ui.button(t(lang, "Ignorer", "Dismiss")).clicked() {
                hub.proposals.remove(0);
            }
        });
    });
}
