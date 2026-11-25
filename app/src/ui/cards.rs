//! Cards that render each agent's output.

use bevy_egui::egui::{self, Color32, RichText};
use chrono::Local;
use merces_agents::Lang;
use merces_core::{
    Action, AgentOutput, Bias, ChartView, Decision, LevelKind, MarketBrief, Side, Signal,
    SignalState, Stance, TradePlan,
};

use super::{meter, t};
use crate::theme::{self, Theme};

pub fn fmt_price(p: f64) -> String {
    if !p.is_finite() {
        "—".into()
    } else if p.abs() >= 1000.0 {
        format!("{p:.2}")
    } else if p.abs() >= 1.0 {
        format!("{p:.4}")
    } else {
        format!("{p:.6}")
    }
}

pub fn bias_chip(ui: &mut egui::Ui, th: &Theme, b: Bias, lang: Lang) {
    let (label, fg) = match b {
        Bias::Bullish => (t(lang, "Haussier", "Bullish"), th.pal.success),
        Bias::Bearish => (t(lang, "Baissier", "Bearish"), th.pal.error),
        Bias::Neutral => (t(lang, "Neutre", "Neutral"), th.pal.muted),
    };
    theme::chip(ui, label, fg, th.pal.fill);
}

fn kv(ui: &mut egui::Ui, th: &Theme, k: &str, v: impl Into<RichText>) {
    ui.label(RichText::new(k).color(th.pal.muted));
    ui.label(v.into());
    ui.end_row();
}

fn para(ui: &mut egui::Ui, th: &Theme, text: &str) {
    ui.label(RichText::new(text).color(th.pal.body));
}

pub fn output(ui: &mut egui::Ui, th: &Theme, o: &AgentOutput, lang: Lang, signal: Option<&Signal>) {
    match o {
        AgentOutput::Brief(b) => brief(ui, th, b, lang),
        AgentOutput::Chart(c) => chart(ui, th, c, lang),
        AgentOutput::Decision(d) => decision(ui, th, d, lang),
        AgentOutput::Plan(p) => plan(ui, th, p, lang, signal),
    }
}

/// One-line summary used in lists and the overview.
pub fn one_line(o: &AgentOutput, lang: Lang) -> String {
    let bias = |b: Bias| match b {
        Bias::Bullish => t(lang, "haussier", "bullish"),
        Bias::Bearish => t(lang, "baissier", "bearish"),
        Bias::Neutral => t(lang, "neutre", "neutral"),
    };
    match o {
        AgentOutput::Brief(b) => format!(
            "{} {} · {} {} · sentiment {:+}",
            t(lang, "Jour", "Day"),
            bias(b.bias_day),
            t(lang, "semaine", "week"),
            bias(b.bias_week),
            b.sentiment
        ),
        AgentOutput::Chart(c) => format!("{:?} · {} · {} %", c.stance, bias(c.bias), c.confidence),
        AgentOutput::Decision(d) => action_label(d.action, lang).to_owned(),
        AgentOutput::Plan(p) => format!(
            "{} @ {} · SL {} · TP {} · {} %",
            if p.side == Side::Long {
                "Long"
            } else {
                "Short"
            },
            fmt_price(p.entry),
            fmt_price(p.stop_loss),
            p.take_profits.first().map_or("—".into(), |v| fmt_price(*v)),
            p.confidence
        ),
    }
}

pub fn action_label(a: Action, lang: Lang) -> &'static str {
    match a {
        Action::Buy => t(lang, "Acheter", "Buy"),
        Action::Sell => t(lang, "Vendre", "Sell"),
        Action::NoTrade => t(lang, "Ne rien faire aujourd'hui", "No trade today"),
    }
}

fn brief(ui: &mut egui::Ui, th: &Theme, b: &MarketBrief, lang: Lang) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(t(lang, "Jour", "Day")).color(th.pal.muted));
        bias_chip(ui, th, b.bias_day, lang);
        ui.label(RichText::new(t(lang, "Semaine", "Week")).color(th.pal.muted));
        bias_chip(ui, th, b.bias_week, lang);
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Sentiment").color(th.pal.muted));
        let v = (b.sentiment as f32 + 100.0) / 200.0;
        let c = if b.sentiment > 15 {
            th.pal.success
        } else if b.sentiment < -15 {
            th.pal.error
        } else {
            th.pal.muted
        };
        meter(ui, th, v, c, 140.0);
        ui.label(theme::mono(th, format!("{:+}", b.sentiment)));
    });
    para(ui, th, &b.summary);
    if !b.catalysts.is_empty() {
        ui.label(theme::eyebrow(th, t(lang, "Catalyseurs", "Catalysts")));
        for c in &b.catalysts {
            ui.horizontal_wrapped(|ui| {
                bias_chip(ui, th, c.impact, lang);
                ui.label(RichText::new(&c.title).color(th.pal.text));
                ui.label(
                    RichText::new(&c.when)
                        .color(th.pal.muted)
                        .size(12.0 * th.scale),
                );
            });
        }
    }
    if !b.risks.is_empty() {
        ui.label(theme::eyebrow(th, t(lang, "Risques", "Risks")));
        for r in &b.risks {
            ui.label(RichText::new(format!("• {r}")).color(th.pal.body));
        }
    }
    if !b.sources.is_empty() {
        ui.collapsing(t(lang, "Sources", "Sources"), |ui| {
            for s in &b.sources {
                if s.starts_with("http") {
                    ui.hyperlink(s);
                } else {
                    ui.label(RichText::new(s).color(th.pal.muted));
                }
            }
        });
    }
}

fn chart(ui: &mut egui::Ui, th: &Theme, c: &ChartView, lang: Lang) {
    ui.horizontal(|ui| {
        let (label, fg) = match c.stance {
            Stance::Confirms => (
                t(lang, "Confirme la recherche", "Confirms research"),
                th.pal.success,
            ),
            Stance::Contradicts => (
                t(lang, "Contredit la recherche", "Contradicts research"),
                th.pal.error,
            ),
            Stance::Mixed => (t(lang, "Avis mitigé", "Mixed view"), th.pal.warning),
        };
        theme::chip(ui, label, fg, th.pal.fill);
        bias_chip(ui, th, c.bias, lang);
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new(t(lang, "Confiance", "Confidence")).color(th.pal.muted));
        meter(ui, th, c.confidence as f32 / 100.0, th.pal.accent, 140.0);
        ui.label(theme::mono(th, format!("{} %", c.confidence)));
    });
    for s in &c.structure {
        ui.label(RichText::new(format!("• {s}")).color(th.pal.body));
    }
    if !c.levels.is_empty() {
        ui.label(theme::eyebrow(th, t(lang, "Niveaux clés", "Key levels")));
        egui::Grid::new(ui.next_auto_id())
            .num_columns(3)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                let mut levels = c.levels.clone();
                levels.sort_by(|a, b| b.price.total_cmp(&a.price));
                for l in &levels {
                    let (k, col) = match l.kind {
                        LevelKind::Support => (t(lang, "Support", "Support"), th.pal.success),
                        LevelKind::Resistance => {
                            (t(lang, "Résistance", "Resistance"), th.pal.error)
                        }
                        LevelKind::Pivot => ("Pivot", th.pal.muted),
                    };
                    ui.label(RichText::new(k).color(col));
                    ui.label(theme::mono(th, fmt_price(l.price)));
                    ui.label(
                        RichText::new(&l.note)
                            .color(th.pal.muted)
                            .size(12.0 * th.scale),
                    );
                    ui.end_row();
                }
            });
    }
    ui.label(RichText::new(format!("ATR : {}", fmt_price(c.atr))).color(th.pal.muted));
    para(ui, th, &c.summary);
}

fn decision(ui: &mut egui::Ui, th: &Theme, d: &Decision, lang: Lang) {
    let color = match d.action {
        Action::Buy => th.pal.success,
        Action::Sell => th.pal.error,
        Action::NoTrade => th.pal.warning,
    };
    ui.label(theme::title(th, action_label(d.action, lang), 22.0).color(color));
    ui.horizontal(|ui| {
        ui.label(RichText::new(t(lang, "Risque", "Risk")).color(th.pal.muted));
        let c = if d.risk_score >= 70 {
            th.pal.error
        } else if d.risk_score >= 40 {
            th.pal.warning
        } else {
            th.pal.success
        };
        meter(ui, th, d.risk_score as f32 / 100.0, c, 140.0);
        ui.label(theme::mono(th, format!("{}/100", d.risk_score)));
    });
    para(ui, th, &d.rationale);
    egui::Grid::new(ui.next_auto_id())
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            kv(
                ui,
                th,
                t(lang, "Risques", "Risks"),
                RichText::new(&d.risk_assessment).color(th.pal.body),
            );
            kv(
                ui,
                th,
                t(lang, "Horizon", "Horizon"),
                RichText::new(&d.horizon).color(th.pal.body),
            );
            kv(
                ui,
                th,
                t(lang, "Prévision", "Prediction"),
                RichText::new(&d.prediction).color(th.pal.body),
            );
        });
}

pub fn state_chip(ui: &mut egui::Ui, th: &Theme, s: SignalState, lang: Lang) {
    let (label, fg): (&str, Color32) = match s {
        SignalState::Pending => (t(lang, "En attente", "Pending"), th.pal.muted),
        SignalState::Triggered => (t(lang, "Déclenché", "Triggered"), th.pal.warning),
        SignalState::TakeProfit => ("Take-profit", th.pal.success),
        SignalState::StopLoss => ("Stop-loss", th.pal.error),
        SignalState::Expired => (t(lang, "Expiré", "Expired"), th.pal.muted),
    };
    theme::chip(ui, label, fg, th.pal.fill);
}

fn plan(ui: &mut egui::Ui, th: &Theme, p: &TradePlan, lang: Lang, signal: Option<&Signal>) {
    ui.horizontal(|ui| {
        let (label, fg) = match p.side {
            Side::Long => ("LONG", th.pal.success),
            Side::Short => ("SHORT", th.pal.error),
        };
        ui.label(theme::title(th, label, 22.0).color(fg));
        if let Some(s) = signal {
            state_chip(ui, th, s.state, lang);
            if let Some(r) = s.pnl_r {
                ui.label(theme::mono(th, format!("{r:+.2} R")).color(if r >= 0.0 {
                    th.pal.success
                } else {
                    th.pal.error
                }));
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new(t(lang, "Confiance", "Confidence")).color(th.pal.muted));
        meter(ui, th, p.confidence as f32 / 100.0, th.pal.accent, 160.0);
        ui.label(theme::mono(th, format!("{} %", p.confidence)));
    });
    egui::Grid::new(ui.next_auto_id())
        .num_columns(2)
        .spacing([16.0, 6.0])
        .show(ui, |ui| {
            kv(
                ui,
                th,
                t(lang, "Entrée", "Entry"),
                theme::mono(th, fmt_price(p.entry)),
            );
            kv(
                ui,
                th,
                "Stop-loss",
                theme::mono(th, fmt_price(p.stop_loss)).color(th.pal.error),
            );
            for (i, tp) in p.take_profits.iter().enumerate() {
                ui.label(RichText::new(format!("TP{}", i + 1)).color(th.pal.muted));
                ui.label(theme::mono(th, fmt_price(*tp)).color(th.pal.success));
                ui.end_row();
            }
            kv(
                ui,
                th,
                "R:R",
                theme::mono(th, format!("{:.2}", p.computed_rr())),
            );
            kv(
                ui,
                th,
                t(lang, "Risque", "Risk"),
                theme::mono(th, format!("{:.2} %", p.risk_pct)),
            );
            kv(
                ui,
                th,
                t(lang, "Fenêtre", "Window"),
                theme::mono(
                    th,
                    format!(
                        "{} → {}",
                        p.entry_window_start
                            .with_timezone(&Local)
                            .format("%d/%m %H:%M"),
                        p.entry_window_end
                            .with_timezone(&Local)
                            .format("%d/%m %H:%M")
                    ),
                ),
            );
        });
    para(ui, th, &p.notes);
    if let Some(s) = signal {
        for f in &s.flagged {
            ui.label(RichText::new(format!("⚠ {f}")).color(th.pal.warning));
        }
    }
}
