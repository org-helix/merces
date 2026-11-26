//! Symbols window: the watchlist, daily schedule, per-symbol risk, search and add.

use bevy_egui::egui::{self, RichText};
use chrono::NaiveTime;
use merces_agents::Command;
use merces_core::{AssetClass, Provider, RiskLimits, SymbolConfig};

use super::{UiState, t};
use crate::bridge::Bridge;
use crate::config::AppConfig;
use crate::state::Hub;
use crate::theme::{self, Theme};

fn class_label(c: AssetClass, lang: merces_agents::Lang) -> &'static str {
    match c {
        AssetClass::Crypto => "Crypto",
        AssetClass::Forex => "Forex",
        AssetClass::Index => t(lang, "Indice", "Index"),
        AssetClass::Commodity => t(lang, "Matière première", "Commodity"),
        AssetClass::Stock => t(lang, "Action", "Stock"),
    }
}

fn time_edit(ui: &mut egui::Ui, at: &mut Option<NaiveTime>, lang: merces_agents::Lang) {
    let mut on = at.is_some();
    if ui
        .checkbox(&mut on, t(lang, "Chaque jour à", "Daily at"))
        .changed()
    {
        *at = if on {
            NaiveTime::from_hms_opt(6, 30, 0)
        } else {
            None
        };
    }
    if let Some(time) = at.as_mut() {
        let (mut h, mut m) = (
            time.format("%H").to_string().parse::<u32>().unwrap_or(0),
            time.format("%M").to_string().parse::<u32>().unwrap_or(0),
        );
        let a = ui.add(egui::DragValue::new(&mut h).range(0..=23).suffix(" h"));
        let b = ui.add(
            egui::DragValue::new(&mut m)
                .range(0..=59)
                .speed(0.5)
                .suffix(" min"),
        );
        if (a.changed() || b.changed())
            && let Some(nt) = NaiveTime::from_hms_opt(h, m, 0)
        {
            *time = nt;
        }
        ui.label("UTC");
    }
}

pub fn body(
    ui: &mut egui::Ui,
    th: &Theme,
    cfg: &mut AppConfig,
    hub: &Hub,
    st: &mut UiState,
    bridge: &Bridge,
) {
    let lang = cfg.lang();
    ui.label(theme::eyebrow(
        th,
        t(lang, "Suivis par les agents", "Tracked by the agents"),
    ));
    let mut remove = None;
    egui::ScrollArea::vertical()
        .max_height(340.0)
        .show(ui, |ui| {
            for (i, s) in cfg.engine.symbols.iter_mut().enumerate() {
                theme::card(th).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut s.enabled, "");
                        ui.label(theme::title(th, &s.symbol, 16.0));
                        ui.add(egui::TextEdit::singleline(&mut s.name).desired_width(140.0));
                        ui.label(
                            RichText::new(format!(
                                "{} · {}",
                                class_label(s.class, lang),
                                s.provider.label()
                            ))
                            .color(th.pal.muted),
                        );
                        if let Some(p) = hub.prices.get(&s.symbol) {
                            ui.label(theme::mono(th, super::cards::fmt_price(*p)));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::ghost_button(
                                ui,
                                RichText::new(t(lang, "Retirer", "Remove")).color(th.pal.error),
                            )
                            .clicked()
                            {
                                remove = Some(i);
                            }
                            if ui.button(t(lang, "Analyser", "Analyse")).clicked() {
                                bridge.send(Command::Run(s.symbol.clone()));
                            }
                        });
                    });
                    ui.horizontal(|ui| time_edit(ui, &mut s.schedule_utc, lang));
                    ui.horizontal(|ui| {
                        let mut custom = s.risk.is_some();
                        if ui
                            .checkbox(
                                &mut custom,
                                t(lang, "Limites de risque propres", "Own risk limits"),
                            )
                            .changed()
                        {
                            s.risk = custom.then(RiskLimits::default);
                        }
                        if let Some(r) = s.risk.as_mut() {
                            ui.label("R:R ≥");
                            ui.add(
                                egui::DragValue::new(&mut r.min_rr)
                                    .range(0.5..=10.0)
                                    .speed(0.05),
                            );
                            ui.label(t(lang, "risque ≤", "risk ≤"));
                            ui.add(
                                egui::DragValue::new(&mut r.max_risk_pct)
                                    .range(0.05..=10.0)
                                    .speed(0.05)
                                    .suffix(" %"),
                            );
                        }
                    });
                });
                ui.add_space(6.0);
            }
        });
    if let Some(i) = remove {
        let gone = cfg.engine.symbols.remove(i);
        if cfg.active_symbol == gone.symbol {
            cfg.active_symbol = cfg
                .engine
                .symbols
                .first()
                .map(|s| s.symbol.clone())
                .unwrap_or_default();
        }
    }

    ui.separator();
    ui.label(theme::eyebrow(
        th,
        t(lang, "Ajouter un symbole", "Add a symbol"),
    ));
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut st.search)
                .hint_text("BTC, EUR/USD, XAU, AAPL…")
                .desired_width(220.0),
        );
        let go = ui.button(t(lang, "Rechercher", "Search")).clicked()
            || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        if go && !st.search.trim().is_empty() {
            st.search_rx = Some(bridge.search(st.search.clone()));
            st.search_results.clear();
        }
        if st.search_rx.is_some() {
            ui.spinner();
        }
    });
    if let Some(rx) = &st.search_rx
        && let Ok(results) = rx.try_recv()
    {
        st.search_results = results;
        st.search_rx = None;
    }
    if !st.search_results.is_empty() {
        egui::ScrollArea::vertical()
            .id_salt("results")
            .max_height(180.0)
            .show(ui, |ui| {
                for r in st.search_results.clone() {
                    ui.horizontal(|ui| {
                        ui.label(theme::mono(th, &r.symbol));
                        ui.label(RichText::new(&r.name).color(th.pal.body));
                        ui.label(
                            RichText::new(format!(
                                "{} · {} · {}",
                                class_label(r.class, lang),
                                r.provider.label(),
                                r.exchange
                            ))
                            .color(th.pal.muted)
                            .size(12.0 * th.scale),
                        );
                        let exists = cfg.engine.symbol(&r.symbol).is_some();
                        if ui
                            .add_enabled(!exists, egui::Button::new(t(lang, "Ajouter", "Add")))
                            .clicked()
                        {
                            cfg.engine.symbols.push(SymbolConfig {
                                symbol: r.symbol.clone(),
                                name: r.name.clone(),
                                class: r.class,
                                provider: r.provider,
                                ..SymbolConfig::default()
                            });
                        }
                    });
                }
            });
    } else {
        ui.label(
                    RichText::new(t(
                        lang,
                        "La recherche couvre Binance (sans clé), Twelve Data et Alpaca (avec leurs clés). Ou ajoutez à la main :",
                        "Search covers Binance (no key), Twelve Data and Alpaca (with their keys). Or add by hand:",
                    ))
                    .size(12.0 * th.scale)
                    .color(th.pal.muted),
                );
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut st.manual.symbol)
                .hint_text(t(lang, "Symbole", "Symbol"))
                .desired_width(110.0),
        );
        ui.add(
            egui::TextEdit::singleline(&mut st.manual.name)
                .hint_text(t(lang, "Nom", "Name"))
                .desired_width(110.0),
        );
        egui::ComboBox::from_id_salt("class")
            .selected_text(class_label(st.manual.class, lang))
            .show_ui(ui, |ui| {
                for c in AssetClass::ALL {
                    if ui
                        .selectable_value(&mut st.manual.class, c, class_label(c, lang))
                        .clicked()
                    {
                        st.manual.provider = c.default_provider();
                    }
                }
            });
        egui::ComboBox::from_id_salt("provider")
            .selected_text(st.manual.provider.label())
            .show_ui(ui, |ui| {
                for p in Provider::ALL {
                    ui.selectable_value(&mut st.manual.provider, p, p.label());
                }
            });
        let sym = st.manual.symbol.trim().to_uppercase();
        let ok = !sym.is_empty() && cfg.engine.symbol(&sym).is_none();
        if ui
            .add_enabled(ok, egui::Button::new(t(lang, "Ajouter", "Add")))
            .clicked()
        {
            let mut s = st.manual.clone();
            s.symbol = sym.clone();
            if s.name.trim().is_empty() {
                s.name = sym;
            }
            cfg.engine.symbols.push(s);
            st.manual = SymbolConfig {
                symbol: String::new(),
                name: String::new(),
                ..SymbolConfig::default()
            };
        }
    });
}
