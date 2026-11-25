//! Helix look (palette of the org-helix projects, structure of Dusk DS):
//! Bricolage Grotesque titles, Geist text, 4 px grid, radii 8/12/16, no drop
//! shadows, depth from translucent fills. Adapted from netlix's `theme.rs`.
//!
//! Dark is the default ("dark Helix sky"); light uses the Pale Amber canvas.
//! The same palette also colours the 3D scene.

use std::sync::Arc;

use bevy::prelude::*;
use bevy_egui::egui::{
    self, Button, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin,
    Response, RichText, Shadow, Stroke, TextStyle, Ui, vec2,
};

use crate::config::{Appearance, FontChoice, ThemeMode};

// ── Helix brand palette ────────────────────────────────────────────────────
pub const RED: Color32 = Color32::from_rgb(0xB2, 0x4E, 0x30); // the only action accent
pub const RUST: Color32 = Color32::from_rgb(0x86, 0x3B, 0x24); // Red under the pointer
pub const INK: Color32 = Color32::from_rgb(0x32, 0x29, 0x21);
pub const AMBER: Color32 = Color32::from_rgb(0xFA, 0xF6, 0xED);
pub const PAPER: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const DUSTY: Color32 = Color32::from_rgb(0x63, 0x52, 0x42);
pub const PALE_ORANGE: Color32 = Color32::from_rgb(0xEC, 0xD3, 0xCB);
pub const BLUSH: Color32 = Color32::from_rgb(0xDC, 0xAF, 0xA2);

const fn ink(alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(0x32, 0x29, 0x21, alpha)
}
const fn amber(alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(0xFA, 0xF6, 0xED, alpha)
}

/// Semantic colours. UI code only reads these.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub canvas: Color32,
    pub panel: Color32,
    pub tile: Color32,
    pub field: Color32,
    pub text: Color32,
    pub body: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub fill: Color32,
    pub fill_hover: Color32,
    pub fill_active: Color32,
    pub hairline: Color32,
    pub wire: Color32,
    pub selected: Color32,
    pub ring: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub on_accent: Color32,
    pub link: Color32,
    pub error: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub on_status: Color32,
    pub inverse: Color32,
    pub on_inverse: Color32,
}

pub const LIGHT: Palette = Palette {
    dark: false,
    canvas: AMBER,
    panel: PAPER,
    tile: PAPER,
    field: AMBER,
    text: INK,
    body: ink(217),
    muted: DUSTY,
    faint: ink(10),
    fill: ink(15),
    fill_hover: ink(26),
    fill_active: ink(31),
    hairline: ink(31),
    wire: ink(115),
    selected: PALE_ORANGE,
    ring: INK,
    accent: RED,
    accent_hover: RUST,
    on_accent: PAPER,
    link: RED,
    error: RUST,
    // Helix: extrapolated — the palette has no status colours. AA on Paper and Amber.
    success: Color32::from_rgb(0x3E, 0x6B, 0x48),
    warning: Color32::from_rgb(0x7A, 0x5A, 0x12),
    on_status: PAPER,
    inverse: INK,
    on_inverse: AMBER,
};

pub const DARK: Palette = Palette {
    dark: true,
    canvas: Color32::from_rgb(0x1E, 0x19, 0x14), // ink, deepened (top of the Helix sky)
    panel: Color32::from_rgb(0x28, 0x20, 0x1A),  // floating plate, opaque
    tile: Color32::from_rgb(0x3C, 0x33, 0x2B),   // ink lifted toward Pale Amber
    field: Color32::from_rgb(0x1E, 0x19, 0x14),
    text: AMBER,
    body: amber(204),
    muted: amber(166),
    faint: amber(10),
    fill: amber(20),
    fill_hover: amber(36),
    fill_active: amber(31),
    hairline: amber(20),
    wire: amber(90),
    selected: Color32::from_rgb(0x5C, 0x32, 0x23), // ink/rust step of the Helix sky
    ring: BLUSH,
    accent: RED,
    accent_hover: RUST,
    on_accent: PAPER,
    link: BLUSH,
    // Helix: extrapolated — AA on the dark canvas.
    error: Color32::from_rgb(0xFF, 0x8A, 0x8A),
    success: Color32::from_rgb(0x4A, 0xDE, 0x80),
    warning: Color32::from_rgb(0xF2, 0xC4, 0x6B),
    on_status: INK,
    inverse: AMBER,
    on_inverse: INK,
};

/// Theme in use, rebuilt when the appearance settings change.
#[derive(Resource, Clone)]
pub struct Theme {
    pub pal: Palette,
    pub scale: f32,
    /// The OS prefers dark (for `ThemeMode::System`).
    pub os_dark: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            pal: DARK,
            scale: 1.0,
            os_dark: true,
        }
    }
}

fn darken(c: Color32, k: f32) -> Color32 {
    let f = |v: u8| (f32::from(v) * k).round() as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

pub fn palette(a: &Appearance, os_dark: bool) -> Palette {
    let dark = match a.theme {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => os_dark,
    };
    let mut p = if dark { DARK } else { LIGHT };
    let accent = Color32::from_rgb(a.accent[0], a.accent[1], a.accent[2]);
    if accent != RED {
        p.accent = accent;
        p.accent_hover = darken(accent, 0.75);
    }
    p
}

/// Egui colour → Bevy colour (sRGB).
pub fn bevy_color(c: Color32) -> Color {
    Color::srgba_u8(c.r(), c.g(), c.b(), c.a())
}

// ── Type ───────────────────────────────────────────────────────────────────
pub fn display() -> FontFamily {
    FontFamily::Name("display".into())
}
pub fn medium() -> FontFamily {
    FontFamily::Name("medium".into())
}
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

const BRICOLAGE: &[u8] = include_bytes!("../assets/fonts/BricolageGrotesqueDisplay-SemiBold.ttf");
const GEIST: &[u8] = include_bytes!("../assets/fonts/Geist-Regular.ttf");
const GEIST_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Geist-Medium.ttf");
const GEIST_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Geist-SemiBold.ttf");
const GEIST_MONO: &[u8] = include_bytes!("../assets/fonts/GeistMono-Regular.ttf");

/// Font name for a choice, loading custom files into `fonts`. Falls back to Geist.
fn font_for(choice: &FontChoice, fonts: &mut FontDefinitions, default: &str) -> String {
    match choice {
        FontChoice::Geist => "geist".into(),
        FontChoice::Bricolage => "bricolage".into(),
        FontChoice::GeistMono => "geist-mono".into(),
        FontChoice::Custom(path) => match std::fs::read(path) {
            Ok(bytes) => {
                let key = format!("custom:{path}");
                fonts
                    .font_data
                    .insert(key.clone(), Arc::new(FontData::from_owned(bytes)));
                key
            }
            Err(e) => {
                tracing::warn!("font {path}: {e}");
                default.into()
            }
        },
    }
}

pub fn install_fonts(ctx: &egui::Context, a: &Appearance) {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        ("bricolage", BRICOLAGE),
        ("geist", GEIST),
        ("geist-medium", GEIST_MEDIUM),
        ("geist-semibold", GEIST_SEMIBOLD),
        ("geist-mono", GEIST_MONO),
    ] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    // egui's bundled fonts stay as fallbacks for symbols Geist doesn't have (✓ ✕ …).
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let with_fallback = |first: String| {
        let mut list = vec![first];
        list.extend(fallback.iter().cloned());
        list
    };
    let ui = font_for(&a.ui_font, &mut fonts, "geist");
    let title = font_for(&a.title_font, &mut fonts, "bricolage");
    // Medium and semibold follow the UI font when it is a custom one.
    let (med, semi) = match a.ui_font {
        FontChoice::Geist => ("geist-medium".to_owned(), "geist-semibold".to_owned()),
        _ => (ui.clone(), ui.clone()),
    };
    fonts
        .families
        .insert(FontFamily::Proportional, with_fallback(ui));
    fonts.families.insert(display(), with_fallback(title));
    fonts.families.insert(medium(), with_fallback(med));
    fonts.families.insert(semibold(), with_fallback(semi));
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono.insert(0, "geist-mono".into());
    }
    ctx.set_fonts(fonts);
}

pub fn apply_style(ctx: &egui::Context, theme: &Theme, a: &Appearance) {
    let c = theme.pal;
    ctx.set_theme(if c.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let k = a.font_scale.clamp(0.8, 1.5);
    let d = 1.0;
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(12.0 * k, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(14.0 * k, FontFamily::Proportional),
            ),
            (TextStyle::Button, FontId::new(14.0 * k, medium())),
            (TextStyle::Heading, FontId::new(20.0 * k, display())),
            (
                TextStyle::Monospace,
                FontId::new(13.0 * k, FontFamily::Monospace),
            ),
        ]
        .into();

        let s = &mut style.spacing;
        s.item_spacing = vec2(8.0, 8.0 * d);
        s.button_padding = vec2(12.0, 6.0 * d);
        s.interact_size = vec2(32.0, 32.0 * d);
        s.window_margin = Margin::same(16);
        s.menu_margin = Margin::same(8);
        s.combo_width = 160.0;
        s.text_edit_width = 200.0;

        let v = &mut style.visuals;
        *v = if c.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.panel_fill = c.panel;
        v.window_fill = c.panel;
        v.window_stroke = Stroke::new(1.0, c.hairline);
        v.window_shadow = Shadow::NONE;
        v.popup_shadow = Shadow::NONE;
        v.window_corner_radius = CornerRadius::same(16);
        v.menu_corner_radius = CornerRadius::same(12);
        v.extreme_bg_color = c.field;
        v.text_edit_bg_color = Some(c.field);
        v.faint_bg_color = c.faint;
        v.code_bg_color = c.fill;
        v.hyperlink_color = c.link;
        v.warn_fg_color = c.warning;
        v.error_fg_color = c.error;
        v.selection.bg_fill = c.selected;
        v.selection.stroke = Stroke::new(2.0, c.ring);
        v.text_cursor.stroke = Stroke::new(2.0, c.ring);

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = c.panel;
        w.noninteractive.weak_bg_fill = c.panel;
        w.noninteractive.bg_stroke = Stroke::new(1.0, c.hairline);
        w.noninteractive.fg_stroke = Stroke::new(1.0, c.body);
        w.noninteractive.corner_radius = CornerRadius::same(8);
        for (wv, fill) in [
            (&mut w.inactive, c.fill),
            (&mut w.hovered, c.fill_hover),
            (&mut w.active, c.fill_active),
            (&mut w.open, c.fill_hover),
        ] {
            wv.bg_fill = fill;
            wv.weak_bg_fill = fill;
            wv.bg_stroke = Stroke::NONE;
            wv.fg_stroke = Stroke::new(1.5, c.text);
            wv.corner_radius = CornerRadius::same(8);
            wv.expansion = 0.0;
        }
    });
}

// ── Small building blocks ──────────────────────────────────────────────────

/// Section title: Bricolage 600, negative tracking.
pub fn title(theme: &Theme, text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text)
        .family(display())
        .size(size * theme.scale)
        .color(theme.pal.text)
        .extra_letter_spacing(-0.02 * size * theme.scale)
}

/// Small label above a group (sentence case, never forced capitals).
pub fn eyebrow(theme: &Theme, text: impl Into<String>) -> RichText {
    RichText::new(text)
        .family(medium())
        .size(12.0 * theme.scale)
        .color(theme.pal.muted)
}

pub fn mono(theme: &Theme, text: impl Into<String>) -> RichText {
    RichText::new(text)
        .family(FontFamily::Monospace)
        .color(theme.pal.text)
}

/// The single primary action of a zone: accent fill, Paper text, darker on hover.
pub fn primary_button(ui: &mut Ui, theme: &Theme, text: &str) -> Response {
    ui.scope(|ui| {
        let c = theme.pal;
        let w = &mut ui.visuals_mut().widgets;
        for (wv, fill) in [
            (&mut w.inactive, c.accent),
            (&mut w.hovered, c.accent_hover),
            (&mut w.active, c.accent_hover),
        ] {
            wv.bg_fill = fill;
            wv.weak_bg_fill = fill;
            wv.fg_stroke.color = c.on_accent;
        }
        ui.add(Button::new(RichText::new(text).family(medium())).min_size(vec2(0.0, 36.0)))
    })
    .inner
}

/// Text-only action (Dusk "ghost").
pub fn ghost_button(ui: &mut Ui, text: impl Into<RichText>) -> Response {
    ui.add(
        Button::new(text.into())
            .frame(false)
            .min_size(vec2(0.0, 32.0)),
    )
}

/// Close button of a panel: a drawn ✕ (the text fonts have no such glyph).
pub fn close_button(ui: &mut Ui, theme: &Theme) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), theme.pal.fill_hover);
    }
    let c = if resp.hovered() {
        theme.pal.text
    } else {
        theme.pal.muted
    };
    let m = rect.center();
    let st = Stroke::new(1.3, c);
    ui.painter()
        .line_segment([m + vec2(-5.0, -5.0), m + vec2(5.0, 5.0)], st);
    ui.painter()
        .line_segment([m + vec2(-5.0, 5.0), m + vec2(5.0, -5.0)], st);
    resp
}

/// Translucent block for grouped content (no border, no shadow).
pub fn card(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.pal.faint)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(12))
}

/// Opaque floating plate (panels over the 3D scene).
pub fn plate(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.pal.panel)
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(16))
}

/// Small pill: tag / status chip.
pub fn chip(ui: &mut Ui, text: impl Into<String>, fg: Color32, bg: Color32) -> Response {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).family(medium()).size(12.0).color(fg))
        })
        .response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color32) -> f64 {
        let ch = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_pairs_meet_wcag_aa() {
        for p in [&LIGHT, &DARK] {
            let name = if p.dark { "dark" } else { "light" };
            for (fg, bg, what) in [
                (p.text, p.canvas, "text on canvas"),
                (p.text, p.panel, "text on panel"),
                (p.text, p.tile, "text on tile"),
                (p.text, p.selected, "text on selection"),
                (p.on_accent, p.accent, "primary button"),
                (p.on_accent, p.accent_hover, "hovered primary button"),
                (p.error, p.panel, "error text"),
                (p.success, p.panel, "success text"),
                (p.warning, p.panel, "warning text"),
                (p.on_inverse, p.inverse, "inverse pill"),
                (p.ring, p.selected, "selected list item"),
            ] {
                let ratio = contrast(fg, bg);
                assert!(ratio >= 4.5, "{name}: {what} is {ratio:.2}:1");
            }
        }
    }
}
