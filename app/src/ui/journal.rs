//! Journal: every trade plan followed on paper, with stats and CSV export.

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

pub fn window(ctx: &egui::Context, th: &Theme, cfg: &AppConfig, hub: &Hub, st: &mut UiState) {
    let lang = cfg.lang();
    let mut open = st.journal;
    egui::Window::new(t(lang, "Journal des signaux", "Signal journal"))
        .open(&mut open)
        .default_width(820.0)
        .default_pos(egui::pos2(80.0, 120.0))
        .show(ctx, |ui| {
            let mut signals: Vec<&Signal> = hub
                .signals
                .values()
                .filter(|s| st.journal_symbol.is_empty() || s.symbol == st.journal_symbol)
                .collect();
            signals.sort_by_key(|s| std::cmp::Reverse(s.created_at));

            let closed: Vec<&&Signal> = signals.iter().filter(|s| s.pnl_r.is_some()).collect();
            let wins = closed.iter().filter(|s| s.state == SignalState::TakeProfit).count();
            let total_r: f64 = closed.iter().filter_map(|s| s.pnl_r).sum();
            ui.horizontal(|ui| {
                for (label, value) in [
                    (t(lang, "Signaux", "Signals"), signals.len().to_string()),
                    (t(lang, "Clôturés", "Closed"), closed.len().to_string()),
                    (
                        t(lang, "Taux de réussite", "Win rate"),
                        if closed.is_empty() { "—".into() } else { format!("{:.0} %", 100.0 * wins as f64 / closed.len() as f64) },
                    ),
                    (t(lang, "Résultat", "Result"), format!("{total_r:+.2} R")),
                ] {
                    theme::card(th).show(ui, |ui| {
                        ui.label(theme::eyebrow(th, label));
                        ui.label(theme::title(th, value, 22.0));
                    });
                }
            });
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("journal-symbol")
                    .selected_text(if st.journal_symbol.is_empty() { t(lang, "Tous les symboles", "All symbols").to_owned() } else { st.journal_symbol.clone() })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut st.journal_symbol, String::new(), t(lang, "Tous les symboles", "All symbols"));
                        for s in &cfg.engine.symbols {
                            ui.selectable_value(&mut st.journal_symbol, s.symbol.clone(), &s.symbol);
                        }
                    });
                if ui.button(t(lang, "Exporter en CSV", "Export CSV")).clicked() {
                    let data = csv(&signals);
                    if let Some(path) = rfd::FileDialog::new().set_file_name("merces-signaux.csv").add_filter("CSV", &["csv"]).save_file() {
                        st.status_line = match std::fs::write(&path, data) {
                            Ok(()) => format!("{} {}", t(lang, "Exporté :", "Exported:"), path.display()),
                            Err(e) => e.to_string(),
                        };
                    }
                }
                ui.label(RichText::new(&st.status_line).color(th.pal.muted).size(12.0 * th.scale));
            });
            ui.separator();
            if signals.is_empty() {
                ui.label(RichText::new(t(lang, "Aucun signal pour l'instant : ils apparaissent quand l'agent d'exécution émet un plan.", "No signals yet: they appear when the execution agent issues a plan.")).color(th.pal.muted));
                return;
            }
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                egui::Grid::new("journal").num_columns(9).spacing([14.0, 6.0]).show(ui, |ui| {
                    for h in [
                        t(lang, "Date", "Date"),
                        t(lang, "Symbole", "Symbol"),
                        t(lang, "Sens", "Side"),
                        t(lang, "Entrée", "Entry"),
                        "SL",
                        "TP1",
                        t(lang, "Confiance", "Confidence"),
                        t(lang, "État", "State"),
                        t(lang, "Résultat", "Result"),
                    ] {
                        ui.label(theme::eyebrow(th, h));
                    }
                    ui.end_row();
                    for s in signals {
                        let p = &s.plan;
                        ui.label(theme::mono(th, s.created_at.with_timezone(&Local).format("%d/%m %H:%M").to_string()).color(th.pal.muted));
                        ui.label(RichText::new(&s.symbol).color(th.pal.text));
                        let (side, c) = if p.side == Side::Long { ("Long", th.pal.success) } else { ("Short", th.pal.error) };
                        ui.label(RichText::new(side).color(c));
                        ui.label(theme::mono(th, cards::fmt_price(p.entry)));
                        ui.label(theme::mono(th, cards::fmt_price(p.stop_loss)));
                        ui.label(theme::mono(th, p.take_profits.first().map_or("—".into(), |v| cards::fmt_price(*v))));
                        ui.label(theme::mono(th, format!("{} %", p.confidence)));
                        cards::state_chip(ui, th, s.state, lang);
                        match s.pnl_r {
                            Some(r) => ui.label(theme::mono(th, format!("{r:+.2} R")).color(if r >= 0.0 { th.pal.success } else { th.pal.error })),
                            None => ui.label(RichText::new("—").color(th.pal.muted)),
                        };
                        ui.end_row();
                    }
                });
            });
        });
    st.journal = open;
}
