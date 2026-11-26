//! The top bar, which is also the window's title bar: wordmark, active symbol
//! and its price, mode, the day's cost, the run button (the zone's single
//! primary action), the three windows, and the window controls.

use bevy_egui::egui::{self, Align, Layout, Margin, RichText, UiBuilder};
use chrono::{Local, Utc};
use merces_agents::Command;

use super::{UiState, fmt_usd, t, titlebar};
use crate::bridge::Bridge;
use crate::config::AppConfig;
use crate::state::Hub;
use crate::theme::{self, Theme};
use crate::window::{MACOS, WindowCtl};

pub const HEIGHT: f32 = 48.0;

pub fn top(
    root: &mut egui::Ui,
    th: &Theme,
    cfg: &mut AppConfig,
    hub: &mut Hub,
    st: &mut UiState,
    bridge: &Bridge,
    win: &mut WindowCtl,
) {
    let lang = cfg.lang();
    egui::Panel::top("top")
        .exact_size(HEIGHT)
        .frame(
            egui::Frame::new()
                .fill(th.pal.panel)
                .inner_margin(Margin::ZERO),
        )
        .show_separator_line(false)
        .show(root, |ui| {
            let bar = ui.max_rect();
            titlebar::drag_zone(ui, bar, win);
            titlebar::buttons(ui, th, bar, win, lang);
            let content = egui::Rect::from_min_max(
                bar.min,
                egui::pos2(bar.right() - titlebar::buttons_width(), bar.bottom()),
            )
            .shrink2(egui::vec2(16.0, 6.0));
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(content)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| bar_content(ui, th, cfg, hub, st, bridge),
            );
        });
}

fn bar_content(
    ui: &mut egui::Ui,
    th: &Theme,
    cfg: &mut AppConfig,
    hub: &mut Hub,
    st: &mut UiState,
    bridge: &Bridge,
) {
    let lang = cfg.lang();
    if MACOS {
        ui.add_space(titlebar::MACOS_LIGHTS - 16.0);
    }
    ui.add(egui::Label::new(theme::title(th, "Merces", 21.0)).selectable(false));
    ui.add_space(14.0);

    // Active symbol and its price (the only place the price is shown).
    let current = cfg.active_symbol.clone();
    let label = cfg
        .engine
        .symbol(&current)
        .map(|s| format!("{} · {}", s.symbol, s.name))
        .unwrap_or_else(|| t(lang, "Aucun symbole", "No symbol").to_owned());
    egui::ComboBox::from_id_salt("symbol")
        .selected_text(label)
        .width(200.0)
        .show_ui(ui, |ui| {
            for s in &cfg.engine.symbols {
                ui.selectable_value(
                    &mut cfg.active_symbol,
                    s.symbol.clone(),
                    format!("{} · {}", s.symbol, s.name),
                );
            }
        });
    if let Some(p) = hub.prices.get(&current) {
        ui.label(theme::mono(th, super::cards::fmt_price(*p)).size(15.0 * th.scale));
    }
    // Other symbols being analysed.
    for s in hub.running() {
        if s != current
            && theme::chip(ui, format!("⟳ {s}"), th.pal.on_accent, th.pal.accent)
                .interact(egui::Sense::click())
                .clicked()
        {
            cfg.active_symbol = s;
        }
    }
    if cfg.demo_mode {
        theme::chip(
            ui,
            t(lang, "Démo", "Demo"),
            th.pal.on_inverse,
            th.pal.inverse,
        )
        .on_hover_text(t(
            lang,
            "Claude et le marché sont simulés : sans clé, sans réseau, sans coût.",
            "Claude and the market are simulated: no keys, no network, no cost.",
        ));
    } else if !bridge.has_claude_key() {
        theme::chip(
            ui,
            t(lang, "Clé Anthropic manquante", "Anthropic key missing"),
            th.pal.on_status,
            th.pal.error,
        );
    }

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        for (section, label) in [
            (super::docks::Section::Settings, t(lang, "Réglages", "Settings")),
            (super::docks::Section::Signals, t(lang, "Signaux", "Signals")),
            (super::docks::Section::Symbols, t(lang, "Symboles", "Symbols")),
        ] {
            if theme::ghost_button(ui, RichText::new(label).color(th.pal.body)).clicked() {
                st.open_section(section);
            }
        }
        ui.add_space(8.0);

        let running = hub.symbol(&current).is_some_and(|v| v.running.is_some());
        if running {
            if ui.button(t(lang, "Annuler", "Cancel")).clicked() {
                bridge.send(Command::Cancel(Some(current.clone())));
            }
            ui.spinner();
        } else {
            let r = theme::primary_button(ui, th, t(lang, "Lancer l'analyse", "Run analysis"));
            let r = match next_run(cfg, hub) {
                Some(next) => r.on_hover_text(next),
                None => r,
            };
            if r.clicked() {
                if !cfg.demo_mode && !bridge.has_claude_key() {
                    hub.toast(
                        crate::state::ToastKind::Error,
                        t(
                            lang,
                            "Ajoutez votre clé Anthropic (Réglages → Données) ou activez le mode démo.",
                            "Add your Anthropic key (Settings → Data) or turn on demo mode.",
                        ),
                    );
                    st.settings_tab = super::SettingsTab::Data;
                    st.open_section(super::docks::Section::Settings);
                } else if !current.is_empty() {
                    bridge.send(Command::Run(current.clone()));
                }
            }
        }

        // The day's Claude cost (nothing to show in demo mode).
        if !cfg.demo_mode {
            let budget = cfg.engine.daily_budget_usd;
            let spent = hub.spent_today();
            let cost = if budget > 0.0 {
                format!("{} / {}", fmt_usd(spent), fmt_usd(budget))
            } else {
                fmt_usd(spent)
            };
            let over = budget > 0.0 && spent >= budget;
            ui.label(theme::mono(th, cost).color(if over { th.pal.error } else { th.pal.muted }))
                .on_hover_text(t(lang, "Coût Claude du jour / budget", "Today's Claude cost / budget"));
        }
    });
}

/// "Next analysis: 26/11 07:30" for the active symbol, when it is scheduled.
fn next_run(cfg: &AppConfig, hub: &Hub) -> Option<String> {
    let lang = cfg.lang();
    let s = cfg.engine.symbol(&cfg.active_symbol)?;
    let at = s.schedule_utc.filter(|_| s.enabled)?;
    let now = Utc::now();
    let mut due = now.date_naive().and_time(at).and_utc();
    let ran_today = hub
        .symbol(&s.symbol)
        .and_then(|v| v.last_outcome.as_ref())
        .is_some_and(|(when, _)| *when >= due);
    if now >= due && ran_today {
        due += chrono::Duration::days(1);
    }
    Some(format!(
        "{} {}",
        t(lang, "Prochaine analyse :", "Next analysis:"),
        due.with_timezone(&Local).format("%d/%m %H:%M")
    ))
}
