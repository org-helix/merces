//! Things drawn over the scene, only when they are useful: the name of the
//! agent under the pointer (or selected), the summary of a result badge under
//! the pointer, toasts, the assistant's symbol proposals and the first-launch
//! notice.

use bevy::prelude::{Camera, GlobalTransform, Vec3};
use bevy_egui::egui::{self, Align2, FontId, Pos2, RichText};
use merces_agents::Command;

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
    let project = |p: Vec3| {
        cam.world_to_viewport(gt, p)
            .ok()
            .map(|v| Pos2::new(v.x / scale, v.y / scale))
    };
    // The name of the agent under the pointer, unless its panel is open.
    if let Some(a) = sel.hovered.filter(|a| sel.agent != Some(*a)) {
        let i = a.index();
        if let Some(at) = project(lay.agents[i] - Vec3::Y * (lay.sizes[i] + 0.2)) {
            painter.text(
                at,
                Align2::CENTER_TOP,
                agent_name(a, lang),
                FontId::new(13.0 * th.scale, theme::medium()),
                th.pal.text,
            );
        }
    }
    // The input neuron under the pointer: the symbol it feeds the agents.
    if sel.input_hovered
        && let Some(at) = project(lay.input - Vec3::Y * (lay.input_size * 1.6 + 0.2))
    {
        let name = cfg
            .engine
            .symbol(&cfg.active_symbol)
            .map_or(cfg.active_symbol.clone(), |s| {
                format!("{} · {}", s.symbol, s.name)
            });
        painter.text(
            at,
            Align2::CENTER_TOP,
            format!("{name} — {}", t(lang, "données de marché", "market data")),
            FontId::new(13.0 * th.scale, theme::medium()),
            th.pal.text,
        );
    }
    // A result badge under the pointer: its one-line summary.
    if let Some((a, index)) = sel.badge
        && let Some(o) = hub
            .symbol(&cfg.active_symbol)
            .and_then(|v| v.today(a).nth(index))
    {
        let text = format!(
            "{} · {}",
            o.at.with_timezone(&chrono::Local).format("%H:%M"),
            cards::one_line(&o.output, lang)
        );
        if let Some(pos) = ctx.pointer_hover_pos() {
            egui::Area::new(egui::Id::new("badge-tip"))
                .fixed_pos(pos + egui::vec2(14.0, 10.0))
                .order(egui::Order::Tooltip)
                .interactable(false)
                .show(ctx, |ui| {
                    theme::plate(th)
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_max_width(320.0);
                            ui.label(RichText::new(text).color(th.pal.text));
                        });
                });
        }
    }
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

/// The notice shown in full here and in Settings → About.
pub fn disclaimer(lang: merces_agents::Lang) -> &'static str {
    t(
        lang,
        "Merces est une aide à la décision, pas un conseil financier. Aucun ordre n'est passé : les plans sont suivis sur papier.",
        "Merces is decision support, not financial advice. No order is placed: plans are tracked on paper.",
    )
}

/// First launch: the notice and how to move around, once.
pub fn welcome(ctx: &egui::Context, th: &Theme, cfg: &mut AppConfig) {
    if cfg.disclaimer_seen {
        return;
    }
    let lang = cfg.lang();
    egui::Window::new(t(lang, "Bienvenue", "Welcome"))
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.label(theme::title(th, t(lang, "Bienvenue dans Merces", "Welcome to Merces"), 22.0));
            ui.add_space(4.0);
            ui.label(RichText::new(disclaimer(lang)).color(th.pal.text));
            ui.add_space(6.0);
            ui.label(
                RichText::new(t(
                    lang,
                    "Quatre agents travaillent à la chaîne autour du symbole actif. Cliquez un agent pour voir son travail ; l'assistant, avec son chapeau de guide, répond à vos questions. Glisser : tourner · molette : zoomer · Échap : vue d'ensemble.",
                    "Four agents work in a chain around the active symbol. Click an agent to see its work; the assistant, in the guide's hat, answers your questions. Drag: rotate · wheel: zoom · Esc: overview.",
                ))
                .color(th.pal.body),
            );
            ui.add_space(10.0);
            if theme::primary_button(ui, th, t(lang, "J'ai compris", "Got it")).clicked() {
                cfg.disclaimer_seen = true;
            }
        });
}
