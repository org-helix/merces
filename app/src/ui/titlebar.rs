//! Window controls drawn by the app (the system frame is gone, see `window.rs`).
//!
//! The top bar is the title bar: its empty parts move the window, a
//! double-click maximises or restores it. Windows and Linux get minimise,
//! maximise/restore and close buttons on the right, and thin edges that resize
//! the window; macOS keeps its native traffic lights on the left.

use bevy::math::CompassOctant;
use bevy_egui::egui::{
    self, CornerRadius, CursorIcon, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use merces_agents::Lang;

use super::t;
use crate::theme::Theme;
use crate::window::{MACOS, WindowCtl, WindowRequest};

pub const BUTTON_WIDTH: f32 = 46.0;
/// Room left for the macOS traffic lights.
pub const MACOS_LIGHTS: f32 = 72.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Button {
    Minimize,
    Maximize,
    Close,
}

/// Background of the bar: drag to move, double-click to maximise. Call before
/// the bar's widgets so they take their clicks first.
pub fn drag_zone(ui: &mut Ui, bar: Rect, win: &mut WindowCtl) {
    let r = ui.interact(bar, Id::new("titlebar-drag"), Sense::click_and_drag());
    if r.drag_started_by(egui::PointerButton::Primary) {
        win.request(WindowRequest::Drag);
    }
    if r.double_clicked() {
        win.request(WindowRequest::ToggleMaximize);
    }
}

/// Width taken by the window buttons on the right (0 on macOS).
pub fn buttons_width() -> f32 {
    if MACOS { 0.0 } else { 3.0 * BUTTON_WIDTH }
}

/// Minimise, maximise/restore, close, at the right end of `bar` (Windows, Linux).
pub fn buttons(ui: &mut Ui, th: &Theme, bar: Rect, win: &mut WindowCtl, lang: Lang) {
    if MACOS {
        return;
    }
    let mut right = bar.right();
    for b in [Button::Close, Button::Maximize, Button::Minimize] {
        right -= BUTTON_WIDTH;
        let rect = Rect::from_min_max(
            pos2(right, bar.top()),
            pos2(right + BUTTON_WIDTH, bar.bottom()),
        );
        if button(ui, th, rect, b, win.maximized, lang).clicked() {
            win.request(match b {
                Button::Minimize => WindowRequest::Minimize,
                Button::Maximize => WindowRequest::ToggleMaximize,
                Button::Close => WindowRequest::Close,
            });
        }
    }
}

fn button(
    ui: &mut Ui,
    th: &Theme,
    rect: Rect,
    b: Button,
    maximized: bool,
    lang: Lang,
) -> egui::Response {
    let c = th.pal;
    let resp = ui.interact(rect, Id::new(("window-button", b as u8)), Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    let close = b == Button::Close;
    if hot {
        ui.painter().rect_filled(
            rect,
            CornerRadius::ZERO,
            if close { c.accent } else { c.fill_hover },
        );
    }
    let color = if hot && close { c.on_accent } else { c.text };
    let p = ui.painter();
    let st = Stroke::new(1.2, color);
    let m = rect.center();
    match b {
        Button::Minimize => {
            p.line_segment([m + vec2(-5.0, 0.0), m + vec2(5.0, 0.0)], st);
        }
        Button::Maximize if maximized => {
            // Restore: two overlapping windows.
            let back = Rect::from_center_size(m + vec2(1.5, -1.5), Vec2::splat(8.0));
            p.line_segment([back.left_top(), back.right_top()], st);
            p.line_segment([back.right_top(), back.right_bottom()], st);
            let front = Rect::from_center_size(m + vec2(-1.0, 1.0), Vec2::splat(8.0));
            p.rect_stroke(front, CornerRadius::same(1), st, StrokeKind::Middle);
        }
        Button::Maximize => {
            let r = Rect::from_center_size(m, Vec2::splat(10.0));
            p.rect_stroke(r, CornerRadius::same(1), st, StrokeKind::Middle);
        }
        Button::Close => {
            p.line_segment([m + vec2(-5.0, -5.0), m + vec2(5.0, 5.0)], st);
            p.line_segment([m + vec2(-5.0, 5.0), m + vec2(5.0, -5.0)], st);
        }
    }
    let tip = match b {
        Button::Minimize => t(lang, "Réduire", "Minimise"),
        Button::Maximize if maximized => t(lang, "Restaurer", "Restore"),
        Button::Maximize => t(lang, "Agrandir", "Maximise"),
        Button::Close => t(lang, "Fermer", "Close"),
    };
    resp.on_hover_text(tip)
}

/// Thin areas along the window edges that resize it, and the hairline
/// border. Call last so the edges take precedence over the panels.
pub fn frame(ctx: &egui::Context, th: &Theme, win: &mut WindowCtl) {
    let r = ctx.viewport_rect();
    if !win.maximized {
        // Hairline around the window (Windows 11 draws its own, in the same colour).
        if !cfg!(target_os = "windows") {
            let radius = if MACOS { 10 } else { 0 };
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                Id::new("frame"),
            ))
            .rect_stroke(
                r,
                CornerRadius::same(radius),
                Stroke::new(1.0, th.pal.hairline),
                StrokeKind::Inside,
            );
        }
    }
    if MACOS || win.maximized {
        return;
    }
    let (edge, corner) = (5.0, 12.0);
    let at = |x0: f32, y0: f32, x1: f32, y1: f32| {
        Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1))
    };
    let zones = [
        (
            at(r.left(), r.top(), r.left() + corner, r.top() + corner),
            CompassOctant::NorthWest,
            CursorIcon::ResizeNorthWest,
        ),
        (
            at(r.right() - corner, r.top(), r.right(), r.top() + corner),
            CompassOctant::NorthEast,
            CursorIcon::ResizeNorthEast,
        ),
        (
            at(r.left(), r.bottom() - corner, r.left() + corner, r.bottom()),
            CompassOctant::SouthWest,
            CursorIcon::ResizeSouthWest,
        ),
        (
            at(
                r.right() - corner,
                r.bottom() - corner,
                r.right(),
                r.bottom(),
            ),
            CompassOctant::SouthEast,
            CursorIcon::ResizeSouthEast,
        ),
        (
            at(
                r.left() + corner,
                r.top(),
                r.right() - corner,
                r.top() + edge,
            ),
            CompassOctant::North,
            CursorIcon::ResizeNorth,
        ),
        (
            at(
                r.left() + corner,
                r.bottom() - edge,
                r.right() - corner,
                r.bottom(),
            ),
            CompassOctant::South,
            CursorIcon::ResizeSouth,
        ),
        (
            at(
                r.left(),
                r.top() + corner,
                r.left() + edge,
                r.bottom() - corner,
            ),
            CompassOctant::West,
            CursorIcon::ResizeWest,
        ),
        (
            at(
                r.right() - edge,
                r.top() + corner,
                r.right(),
                r.bottom() - corner,
            ),
            CompassOctant::East,
            CursorIcon::ResizeEast,
        ),
    ];
    egui::Area::new(Id::new("resize-edges"))
        .order(egui::Order::Foreground)
        .fixed_pos(r.min)
        .interactable(true)
        .show(ctx, |ui| {
            for (i, (rect, dir, cursor)) in zones.into_iter().enumerate() {
                let resp = ui.interact(rect, Id::new(("resize", i)), Sense::drag());
                if resp.hovered() || resp.dragged() {
                    ctx.set_cursor_icon(cursor);
                }
                if resp.drag_started() {
                    win.request(WindowRequest::Resize(dir));
                }
            }
        });
}
