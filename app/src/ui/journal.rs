//! Signals: every trade plan followed on paper, with stats and CSV export,
//! in its fold of the right drawer.

use bevy_egui::egui::{self, RichText};
use chrono::Local;
use merces_core::{Side, Signal, SignalState};

use super::{UiState, cards, t};
use crate::config::AppConfig;
use crate::state::Hub;
use crate::theme::{self, Theme};

fn csv(signals: &[&Signal]) -> String {
    let mut out = String::from(
        "created_at,symbol,side,entry,stop_loss,tp1,confidence,state,exit_price,pnl_r,flags\n",
    );
    for s in signals {
        let p = &s.plan;
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{:?},{},{},\"{}\"\n",
            s.created_at.to_rfc3339(),
            s.symbol,
            if p.side == Side::Long {
                "long"
            } else {
                "short"
            },
            p.entry,
            p.stop_loss,
            p.take_profits
                .first()
                .map_or(String::new(), |v| v.to_string()),
            p.confidence,
            s.state,
            s.exit_price.map_or(String::new(), |v| v.to_string()),
            s.pnl_r.map_or(String::new(), |v| format!("{v:.3}")),
            s.flagged.join("; ").replace('"', "'"),
        ));
    }
    out
}

/// The signals in the right drawer: totals, a symbol filter, CSV export, then
/// one compact card per signal (newest first).
pub fn body(ui: &mut egui::Ui, th: &Theme, cfg: &AppConfig, hub: &Hub, st: &mut UiState) {
    let lang = cfg.lang();
    let mut signals: Vec<&Signal> = hub
        .signals
        .values()
        .filter(|s| st.journal_symbol.is_empty() || s.symbol == st.journal_symbol)
        .collect();
    signals.sort_by_key(|s| std::cmp::Reverse(s.created_at));

    let closed: Vec<&&Signal> = signals.iter().filter(|s| s.pnl_r.is_some()).collect();
    let wins = closed
        .iter()
        .filter(|s| s.state == SignalState::TakeProfit)
        .count();
    let total_r: f64 = closed.iter().filter_map(|s| s.pnl_r).sum();
    egui::Grid::new("signal-stats")
        .num_columns(4)
        .spacing([12.0, 2.0])
        .show(ui, |ui| {
            for label in [
                t(lang, "Signaux", "Signals"),
                t(lang, "Clôturés", "Closed"),
                t(lang, "Réussite", "Win rate"),
                t(lang, "Résultat", "Result"),
            ] {
                ui.label(theme::eyebrow(th, label));
            }
            ui.end_row();
            let rate = if closed.is_empty() {
                "—".to_owned()
            } else {
                format!("{:.0} %", 100.0 * wins as f64 / closed.len() as f64)
            };
            for value in [
                signals.len().to_string(),
                closed.len().to_string(),
                rate,
                format!("{total_r:+.2} R"),
            ] {
                ui.label(theme::title(th, value, 17.0));
            }
            ui.end_row();
        });
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("journal-symbol")
            .selected_text(if st.journal_symbol.is_empty() {
                t(lang, "Tous les symboles", "All symbols").to_owned()
            } else {
                st.journal_symbol.clone()
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut st.journal_symbol,
                    String::new(),
                    t(lang, "Tous les symboles", "All symbols"),
                );
                for s in &cfg.engine.symbols {
                    ui.selectable_value(&mut st.journal_symbol, s.symbol.clone(), &s.symbol);
                }
            });
        if ui
            .button(t(lang, "Exporter en CSV", "Export CSV"))
            .clicked()
        {
            let data = csv(&signals);
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("merces-signaux.csv")
                .add_filter("CSV", &["csv"])
                .save_file()
            {
                st.status_line = match std::fs::write(&path, data) {
                    Ok(()) => format!("{} {}", t(lang, "Exporté :", "Exported:"), path.display()),
                    Err(e) => e.to_string(),
                };
            }
        }
    });
    if !st.status_line.is_empty() {
        ui.label(
            RichText::new(&st.status_line)
                .color(th.pal.muted)
                .size(12.0 * th.scale),
        );
    }
    if signals.is_empty() {
        ui.label(
            RichText::new(t(
                lang,
                "Aucun signal pour l'instant : ils apparaissent quand l'agent d'exécution émet un plan.",
                "No signals yet: they appear when the execution agent issues a plan.",
            ))
            .color(th.pal.muted),
        );
        return;
    }
    for s in signals.into_iter().take(100) {
        let p = &s.plan;
        egui::Frame::new()
            .fill(th.pal.fill)
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&s.symbol).color(th.pal.text));
                    let (side, c) = if p.side == Side::Long {
                        ("Long", th.pal.success)
                    } else {
                        ("Short", th.pal.error)
                    };
                    ui.label(RichText::new(side).color(c));
                    cards::state_chip(ui, th, s.state, lang);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match s.pnl_r {
                            Some(r) => {
                                ui.label(theme::mono(th, format!("{r:+.2} R")).color(if r >= 0.0 {
                                    th.pal.success
                                } else {
                                    th.pal.error
                                }))
                            }
                            None => ui.label(
                                theme::mono(
                                    th,
                                    s.created_at
                                        .with_timezone(&Local)
                                        .format("%d/%m %H:%M")
                                        .to_string(),
                                )
                                .color(th.pal.muted),
                            ),
                        };
                    });
                });
                ui.label(
                    theme::mono(
                        th,
                        format!(
                            "{} {} · SL {} · TP {} · {} %",
                            t(lang, "Entrée", "Entry"),
                            cards::fmt_price(p.entry),
                            cards::fmt_price(p.stop_loss),
                            p.take_profits
                                .first()
                                .map_or("—".into(), |v| cards::fmt_price(*v)),
                            p.confidence
                        ),
                    )
                    .size(12.0 * th.scale)
                    .color(th.pal.body),
                );
            });
        ui.add_space(4.0);
    }
}
