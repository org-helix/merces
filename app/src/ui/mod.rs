//! egui interface over the 3D scene: top bar, agent panel, overview card,
//! symbols, journal, settings and assistant.

mod agent;
mod assistant;
mod cards;
mod journal;
mod md;
mod overlay;
mod settings;
mod symbols;
mod topbar;

use bevy::prelude::*;
use bevy::window::{WindowTheme, WindowThemeChanged};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use crossbeam_channel::Receiver;
use merces_agents::Lang;
use merces_core::{AgentId, SymbolInfo};

use crate::bridge::Bridge;
use crate::config::{self, AppConfig, Appearance, Secrets};
use crate::scene::{Layout, Selection};
use crate::state::Hub;
use crate::theme::{self, Theme};

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiState>()
            .add_systems(Update, (os_theme, update_theme).chain())
            .add_systems(EguiPrimaryContextPass, (apply_egui_theme, ui_main).chain());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Appearance,
    Scene,
    Agents,
    Data,
    Risk,
    General,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentTab {
    #[default]
    Live,
    Today,
    Messages,
    History,
}

#[derive(Resource, Default)]
pub struct UiState {
    pub settings: bool,
    pub settings_tab: SettingsTab,
    pub symbols: bool,
    pub journal: bool,
    pub agent_tab: AgentTab,
    pub chat_input: String,
    pub search: String,
    pub search_rx: Option<Receiver<Vec<SymbolInfo>>>,
    pub search_results: Vec<SymbolInfo>,
    pub manual: merces_core::SymbolConfig,
    pub draft_secrets: Option<Secrets>,
    pub show_keys: bool,
    pub journal_symbol: String,
    pub status_line: String,
    saved: Option<AppConfig>,
    /// Fonts set with `set_fonts` only apply from the next frame.
    fonts_ready: bool,
}

/// French / English text.
pub fn t(lang: Lang, fr: &'static str, en: &'static str) -> &'static str {
    match lang {
        Lang::Fr => fr,
        Lang::En => en,
    }
}

pub fn agent_name(a: AgentId, lang: Lang) -> &'static str {
    match a {
        AgentId::Research => t(lang, "Recherche", "Research"),
        AgentId::Chart => t(lang, "Analyse du cours", "Chart analysis"),
        AgentId::Decision => t(lang, "Décision", "Decision"),
        AgentId::Execution => t(lang, "Exécution", "Execution"),
        AgentId::Assistant => t(lang, "Assistant", "Assistant"),
    }
}

pub fn agent_role(a: AgentId, lang: Lang) -> &'static str {
    match a {
        AgentId::Research => t(
            lang,
            "Analyse les marchés, les news et le calendrier ; donne le biais du jour et de la semaine.",
            "Reads markets, news and the calendar; gives the day and week bias.",
        ),
        AgentId::Chart => t(
            lang,
            "Lit les graphiques multi-unités de temps pour confirmer ou contredire la recherche.",
            "Reads multi-timeframe charts to confirm or challenge the research.",
        ),
        AgentId::Decision => t(
            lang,
            "Décide d'acheter, de vendre ou de ne rien faire si le risque est trop élevé.",
            "Decides to buy, sell, or do nothing when the risk is too high.",
        ),
        AgentId::Execution => t(
            lang,
            "Fixe l'entrée, l'heure, le TP/SL et son pourcentage de confiance.",
            "Sets the entry, time, TP/SL and its confidence.",
        ),
        AgentId::Assistant => t(
            lang,
            "Vous aide sur l'application et répond à vos questions de trading.",
            "Helps with the app and answers trading questions.",
        ),
    }
}

/// Follow the OS light/dark preference (used by the "System" theme).
fn os_theme(mut changes: MessageReader<WindowThemeChanged>, mut theme: ResMut<Theme>) {
    for c in changes.read() {
        let dark = c.theme == WindowTheme::Dark;
        if theme.os_dark != dark {
            theme.os_dark = dark;
        }
    }
}

fn update_theme(cfg: Res<AppConfig>, mut theme: ResMut<Theme>) {
    if !cfg.is_changed() && !theme.is_changed() {
        return;
    }
    let pal = theme::palette(&cfg.appearance, theme.os_dark);
    let scale = cfg.appearance.font_scale.clamp(0.8, 1.5);
    if theme.pal != pal || theme.scale != scale {
        theme.pal = pal;
        theme.scale = scale;
    }
}

/// Re-style egui when the theme or appearance changes; fonts only when they change.
fn apply_egui_theme(
    mut contexts: EguiContexts,
    cfg: Res<AppConfig>,
    theme: Res<Theme>,
    mut last: Local<Option<(Appearance, theme::Palette)>>,
    mut fonts: Local<Option<(config::FontChoice, config::FontChoice)>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let key = (cfg.appearance.clone(), theme.pal);
    if last.as_ref() == Some(&key) {
        return Ok(());
    }
    let font_key = (
        cfg.appearance.ui_font.clone(),
        cfg.appearance.title_font.clone(),
    );
    if fonts.as_ref() != Some(&font_key) {
        theme::install_fonts(ctx, &cfg.appearance);
        *fonts = Some(font_key);
    }
    theme::apply_style(ctx, &theme, &cfg.appearance);
    *last = Some(key);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn ui_main(
    mut contexts: EguiContexts,
    mut cfg: ResMut<AppConfig>,
    mut hub: ResMut<Hub>,
    theme: Res<Theme>,
    mut sel: ResMut<Selection>,
    mut ui_state: ResMut<UiState>,
    bridge: Res<Bridge>,
    mut secrets: ResMut<Secrets>,
    lay: Res<Layout>,
    cams: Query<(&Camera, &GlobalTransform)>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    let st = &mut *ui_state;
    if !st.fonts_ready {
        st.fonts_ready = true;
        return Ok(());
    }
    if st.saved.is_none() {
        st.saved = Some(cfg.clone());
    }
    // Edit a copy so change detection only fires on real changes.
    let mut draft = cfg.clone();
    let th = &*theme;
    let mut root = egui::Ui::new(
        ctx.clone(),
        "root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let root = &mut root;

    topbar::top(root, th, &mut draft, &mut hub, &mut sel, st, &bridge);
    topbar::bottom(root, th, &draft, &hub, &bridge);

    match sel.agent {
        Some(AgentId::Assistant) => {
            sel.panel_px = assistant::panel(root, th, &draft, &mut hub, &mut sel, st, &bridge);
        }
        Some(agent) => {
            sel.panel_px = agent::panel(root, th, &draft, &hub, &mut sel, st, agent);
        }
        None => {
            sel.panel_px = 0.0;
            overlay::overview(&ctx, th, &draft, &hub, &mut sel);
        }
    }
    if let Ok((cam, gt)) = cams.single()
        && draft.scene.labels
    {
        overlay::labels(&ctx, th, &draft, &hub, &sel, &lay, cam, gt);
    }
    overlay::toasts(&ctx, th, &hub);
    overlay::proposals(&ctx, th, &mut draft, &mut hub, &bridge);

    if st.symbols {
        symbols::window(&ctx, th, &mut draft, &hub, st, &bridge);
    }
    if st.journal {
        journal::window(&ctx, th, &draft, &hub, st);
    }
    if st.settings {
        settings::window(&ctx, th, &mut draft, &mut secrets, st, &bridge);
    }

    if draft != *cfg {
        if draft.active_symbol != cfg.active_symbol {
            bridge.send(merces_agents::Command::Focus(draft.active_symbol.clone()));
        }
        *cfg = draft;
    }
    // Persist and push to the engine when the configuration really changed.
    if st.saved.as_ref() != Some(&*cfg) {
        if let Err(e) = config::save(&cfg) {
            st.status_line = format!("config : {e}");
        }
        bridge.sync(&cfg, &secrets);
        st.saved = Some(cfg.clone());
    }
    Ok(())
}

/// Horizontal meter (0..=1), track + value, radius 2.
pub fn meter(ui: &mut egui::Ui, th: &Theme, value: f32, color: egui::Color32, width: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, th.pal.fill);
    let mut v = rect;
    v.set_width(rect.width() * value.clamp(0.0, 1.0));
    p.rect_filled(v, 3.0, color);
}

pub fn fmt_usd(v: f64) -> String {
    format!("{v:.2} $")
}

pub fn fmt_tokens(n: u64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1} M", n as f64 / 1e6),
        n if n >= 1_000 => format!("{:.1} k", n as f64 / 1e3),
        n => n.to_string(),
    }
}
