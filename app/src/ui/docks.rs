//! The two fixed side drawers.
//!
//! - Left, "Analyses": one fold per pipeline agent. Its header shows the
//!   agent, its state and its latest result in one line; unfolded, the live
//!   work, today's results and the history.
//! - Right, "Follow-up": signals, assistant, symbols and settings, each in
//!   its own fold.
//!
//! Each drawer rolls in to a thin strip on its edge of the window and rolls
//! out again. Clicking an agent in the scene, or a button of the top bar,
//! unfolds the matching section.

use bevy_egui::egui::{
    self, Align, Layout, RichText, ScrollArea, collapsing_header::CollapsingState,
};
use merces_core::AgentId;

use super::{UiState, agent, agent_name, assistant, journal, settings, symbols, t};
use crate::bridge::Bridge;
use crate::config::{AppConfig, Secrets};
use crate::scene::Selection;
use crate::state::Hub;
use crate::theme::{self, Theme};

/// A fold of a drawer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Agent(AgentId),
    Signals,
    Assistant,
    Symbols,
    Settings,
}

impl Section {
    fn right(self) -> bool {
        !matches!(self, Section::Agent(a) if a != AgentId::Assistant)
    }

    fn id(self) -> egui::Id {
        egui::Id::new(("dock-section", self))
    }
}

/// Width of a rolled-in drawer.
const STRIP: f32 = 40.0;

impl UiState {
    /// Unfold a section (and roll its drawer out) on the next frame.
    pub fn open_section(&mut self, s: Section) {
        let s = if s == Section::Agent(AgentId::Assistant) {
            Section::Assistant
        } else {
            s
        };
        if s.right() {
            self.right_open = true;
        } else {
            self.left_open = true;
        }
        self.request = Some(s);
    }
}

/// A drawer: a strip with a roll-out button when closed, a resizable panel
/// with a title and a roll-in button when open. Returns its width.
fn drawer(
    root: &mut egui::Ui,
    th: &Theme,
    left: bool,
    open: &mut bool,
    title: &str,
    content: impl FnOnce(&mut egui::Ui),
) -> f32 {
    let id = if left { "dock-left" } else { "dock-right" };
    let panel = if left {
        egui::Panel::left(id)
    } else {
        egui::Panel::right(id)
    };
    let frame = egui::Frame::new()
        .fill(th.pal.panel)
        .inner_margin(egui::Margin::symmetric(if *open { 16 } else { 4 }, 12));
    if !*open {
        return panel
            .exact_size(STRIP)
            .resizable(false)
            .frame(frame)
            .show_separator_line(false)
            .show(root, |ui| {
                ui.vertical_centered(|ui| {
                    if theme::chevron_button(ui, th, !left)
                        .on_hover_text(title)
                        .clicked()
                    {
                        *open = true;
                    }
                });
            })
            .response
            .rect
            .width();
    }
    panel
        .default_size(400.0)
        .size_range(320.0..=720.0)
        .resizable(true)
        .frame(frame)
        .show_separator_line(false)
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::title(th, title, 20.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::chevron_button(ui, th, left).clicked() {
                        *open = false;
                    }
                });
            });
            ui.add_space(4.0);
            ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    content(ui);
                });
        })
        .response
        .rect
        .width()
}

/// One fold, with a custom header. Unfolds (and scrolls into view) when
/// requested.
fn fold(
    ui: &mut egui::Ui,
    th: &Theme,
    section: Section,
    request: &mut Option<Section>,
    default_open: bool,
    header: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui),
) {
    let mut state = CollapsingState::load_with_default_open(ui.ctx(), section.id(), default_open);
    let wanted = *request == Some(section);
    if wanted {
        state.set_open(true);
    }
    let r = theme::card(th).show(ui, |ui| {
        ui.set_width(ui.available_width());
        state
            .show_header(ui, |ui| {
                ui.horizontal(header);
            })
            .body(|ui| {
                ui.add_space(4.0);
                body(ui);
            });
    });
    if wanted {
        r.response.scroll_to_me(Some(Align::TOP));
        *request = None;
    }
    ui.add_space(8.0);
}

/// Left drawer: the analyses, one fold per pipeline agent.
fn analyses(
    ui: &mut egui::Ui,
    th: &Theme,
    cfg: &AppConfig,
    hub: &Hub,
    sel: &mut Selection,
    st: &mut UiState,
) {
    let lang = cfg.lang();
    let view = hub.symbol(&cfg.active_symbol);
    ui.label(theme::eyebrow(
        th,
        format!(
            "{} · {}",
            t(lang, "Aujourd'hui", "Today"),
            cfg.active_symbol
        ),
    ));
    ui.add_space(4.0);
    for a in AgentId::PIPELINE {
        let status = view.map(|v| v.agent(a).status).unwrap_or_default();
        let latest = view
            .and_then(|v| v.today(a).next())
            .map(|o| super::cards::one_line(&o.output, lang));
        let mut request = st.request;
        fold(
            ui,
            th,
            Section::Agent(a),
            &mut request,
            false,
            |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(theme::title(th, agent_name(a, lang), 16.0));
                        agent::status_chip(ui, th, status, lang);
                    });
                    if let Some(line) = &latest {
                        ui.add(
                            egui::Label::new(
                                RichText::new(line).size(12.0 * th.scale).color(th.pal.body),
                            )
                            .truncate(),
                        );
                    }
                });
            },
            |ui| agent::body(ui, th, cfg, hub, sel, st, a),
        );
        st.request = request;
    }
}

/// Right drawer: signals, assistant, symbols, settings.
#[allow(clippy::too_many_arguments)]
fn follow_up(
    ui: &mut egui::Ui,
    th: &Theme,
    cfg: &mut AppConfig,
    hub: &mut Hub,
    secrets: &mut Secrets,
    st: &mut UiState,
    bridge: &Bridge,
) {
    let lang = cfg.lang();
    let mut request = st.request;
    let open_signals = hub
        .signals
        .values()
        .filter(|s| !s.state.is_closed())
        .count();
    fold(
        ui,
        th,
        Section::Signals,
        &mut request,
        true,
        |ui| {
            ui.label(theme::title(th, t(lang, "Signaux", "Signals"), 16.0));
            if open_signals > 0 {
                theme::chip(
                    ui,
                    format!("{open_signals}"),
                    th.pal.on_accent,
                    th.pal.accent,
                );
            }
        },
        |ui| journal::body(ui, th, cfg, hub, st),
    );
    let chat_busy = hub.chat_busy;
    let symbol_count = cfg.engine.symbols.len();
    fold(
        ui,
        th,
        Section::Assistant,
        &mut request,
        false,
        |ui| {
            ui.label(theme::title(th, t(lang, "Assistant", "Assistant"), 16.0));
            if chat_busy {
                ui.spinner();
            }
        },
        |ui| assistant::body(ui, th, cfg, hub, st, bridge),
    );
    fold(
        ui,
        th,
        Section::Symbols,
        &mut request,
        false,
        |ui| {
            ui.label(theme::title(th, t(lang, "Symboles", "Symbols"), 16.0));
            ui.label(
                RichText::new(symbol_count.to_string())
                    .size(12.0 * th.scale)
                    .color(th.pal.muted),
            );
        },
        |ui| symbols::body(ui, th, cfg, hub, st, bridge),
    );
    fold(
        ui,
        th,
        Section::Settings,
        &mut request,
        false,
        |ui| {
            ui.label(theme::title(th, t(lang, "Réglages", "Settings"), 16.0));
        },
        |ui| settings::body(ui, th, cfg, secrets, st, bridge),
    );
    st.request = request;
}

/// Show both drawers. Returns (left width, right width) in logical pixels.
#[allow(clippy::too_many_arguments)]
pub fn show(
    root: &mut egui::Ui,
    th: &Theme,
    cfg: &mut AppConfig,
    hub: &mut Hub,
    sel: &mut Selection,
    secrets: &mut Secrets,
    st: &mut UiState,
    bridge: &Bridge,
) -> (f32, f32) {
    // An agent clicked in the scene unfolds its section.
    if sel.agent != st.last_selected {
        st.last_selected = sel.agent;
        if let Some(a) = sel.agent {
            st.open_section(Section::Agent(a));
        }
    }
    let lang = cfg.lang();
    let mut left_open = st.left_open;
    let left = drawer(
        root,
        th,
        true,
        &mut left_open,
        t(lang, "Analyses", "Analyses"),
        |ui| analyses(ui, th, cfg, hub, sel, st),
    );
    st.left_open = left_open;
    let mut right_open = st.right_open;
    let right = drawer(
        root,
        th,
        false,
        &mut right_open,
        t(lang, "Suivi", "Follow-up"),
        |ui| follow_up(ui, th, cfg, hub, secrets, st, bridge),
    );
    st.right_open = right_open;
    (left, right)
}
