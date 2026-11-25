//! Settings: appearance (with an "Advanced" fold), agents, data & keys, risk,
//! about. The essentials fit on one screen.

use bevy_egui::egui::{self, RichText};
use merces_agents::Lang;
use merces_core::{AgentId, Effort};

use super::{SettingsTab, UiState, agent_name, t};
use crate::bridge::Bridge;
use crate::config::{self, AppConfig, FontChoice, Quality, Secrets, ThemeMode};
use crate::theme::{self, Theme};

const MODELS: [&str; 4] = [
    "claude-opus-5-5",
    "claude-sonnet-5-5",
    "claude-haiku-5-5",
    "claude-fable-5-2",
];

fn font_label(f: &FontChoice, lang: Lang) -> String {
    match f {
        FontChoice::Geist => "Geist".into(),
        FontChoice::Bricolage => "Bricolage Grotesque".into(),
        FontChoice::GeistMono => "Geist Mono".into(),
        FontChoice::Custom(p) => {
            let name = std::path::Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            format!("{} ({name})", t(lang, "Personnalisée", "Custom"))
        }
    }
}

fn font_picker(ui: &mut egui::Ui, id: &str, f: &mut FontChoice, lang: Lang) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(font_label(f, lang))
        .width(240.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(f, FontChoice::Geist, "Geist");
            ui.selectable_value(f, FontChoice::Bricolage, "Bricolage Grotesque");
            ui.selectable_value(f, FontChoice::GeistMono, "Geist Mono");
            if ui
                .selectable_label(
                    matches!(f, FontChoice::Custom(_)),
                    t(lang, "Fichier de police…", "Font file…"),
                )
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Fonts", &["ttf", "otf"])
                    .pick_file()
            {
                *f = FontChoice::Custom(path.display().to_string());
            }
        });
}

fn row(ui: &mut egui::Ui, th: &Theme, label: &str) {
    ui.label(RichText::new(label).color(th.pal.body));
}

pub fn window(
    ctx: &egui::Context,
    th: &Theme,
    cfg: &mut AppConfig,
    secrets: &mut Secrets,
    st: &mut UiState,
    bridge: &Bridge,
) {
    let lang = cfg.lang();
    let mut open = st.settings;
    egui::Window::new(t(lang, "Réglages", "Settings"))
        .open(&mut open)
        .default_width(600.0)
        .resizable(false)
        .default_pos(egui::pos2(300.0, 90.0))
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (tab, label) in [
                    (SettingsTab::Appearance, t(lang, "Apparence", "Appearance")),
                    (SettingsTab::Agents, t(lang, "Agents", "Agents")),
                    (SettingsTab::Data, t(lang, "Données", "Data")),
                    (SettingsTab::Risk, t(lang, "Risque", "Risk")),
                    (SettingsTab::About, t(lang, "À propos", "About")),
                ] {
                    ui.selectable_value(&mut st.settings_tab, tab, label);
                }
            });
            ui.separator();
            // The short tabs fit as they are; the long ones scroll on small screens.
            let room = (ctx.viewport_rect().height() - 220.0).max(240.0);
            match st.settings_tab {
                SettingsTab::Appearance => appearance(ui, th, cfg, lang),
                SettingsTab::Risk => risk(ui, th, cfg, lang),
                SettingsTab::About => about(ui, th, lang),
                SettingsTab::Agents => {
                    egui::ScrollArea::vertical()
                        .max_height(room)
                        .min_scrolled_height(room.min(420.0))
                        .show(ui, |ui| agents(ui, th, cfg, lang));
                }
                SettingsTab::Data => {
                    egui::ScrollArea::vertical()
                        .max_height(room)
                        .min_scrolled_height(room.min(420.0))
                        .show(ui, |ui| data(ui, th, cfg, secrets, st, bridge, lang));
                }
            }
        });
    st.settings = open;
}

fn appearance(ui: &mut egui::Ui, th: &Theme, cfg: &mut AppConfig, lang: Lang) {
    egui::Grid::new("appearance")
        .num_columns(2)
        .spacing([24.0, 12.0])
        .show(ui, |ui| {
            let a = &mut cfg.appearance;
            row(ui, th, t(lang, "Thème", "Theme"));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut a.theme, ThemeMode::Dark, t(lang, "Sombre", "Dark"));
                ui.selectable_value(&mut a.theme, ThemeMode::Light, t(lang, "Clair", "Light"));
                ui.selectable_value(
                    &mut a.theme,
                    ThemeMode::System,
                    t(lang, "Système", "System"),
                );
            });
            ui.end_row();

            row(ui, th, t(lang, "Couleur d'accent", "Accent colour"));
            ui.horizontal(|ui| {
                ui.color_edit_button_srgb(&mut a.accent);
                if a.accent != [0xB2, 0x4E, 0x30]
                    && ui.link(t(lang, "Rouge Helix", "Helix red")).clicked()
                {
                    a.accent = [0xB2, 0x4E, 0x30];
                }
            });
            ui.end_row();

            row(ui, th, t(lang, "Police", "Font"));
            font_picker(ui, "ui-font", &mut a.ui_font, lang);
            ui.end_row();

            row(ui, th, t(lang, "Taille du texte", "Text size"));
            let mut pct = (a.font_scale * 100.0).round();
            if ui
                .add(
                    egui::Slider::new(&mut pct, 80.0..=150.0)
                        .step_by(5.0)
                        .suffix(" %"),
                )
                .changed()
            {
                a.font_scale = pct / 100.0;
            }
            ui.end_row();

            row(ui, th, t(lang, "Qualité 3D", "3D quality"));
            ui.horizontal(|ui| {
                let q = &mut cfg.scene.quality;
                ui.selectable_value(q, Quality::Low, t(lang, "Basse", "Low"))
                    .on_hover_text(t(
                        lang,
                        "Sans anticrénelage ni lueur, moins d'étoiles : pour les GPU intégrés sur batterie.",
                        "No anti-aliasing or glow, fewer stars: for integrated GPUs on battery.",
                    ));
                ui.selectable_value(q, Quality::Medium, t(lang, "Moyenne", "Medium"));
                ui.selectable_value(q, Quality::High, t(lang, "Haute", "High"));
            });
            ui.end_row();

            row(ui, th, t(lang, "Langue", "Language"));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut cfg.engine.lang, Lang::Fr, "Français");
                ui.selectable_value(&mut cfg.engine.lang, Lang::En, "English");
            });
            ui.end_row();
        });
    ui.add_space(8.0);
    egui::CollapsingHeader::new(t(lang, "Avancé", "Advanced"))
        .id_salt("advanced")
        .default_open(false)
        .show(ui, |ui| advanced(ui, th, cfg, lang));
}

fn advanced(ui: &mut egui::Ui, th: &Theme, cfg: &mut AppConfig, lang: Lang) {
    egui::Grid::new("advanced-grid")
        .num_columns(2)
        .spacing([24.0, 12.0])
        .show(ui, |ui| {
            row(ui, th, t(lang, "Police des titres", "Title font"));
            font_picker(ui, "title-font", &mut cfg.appearance.title_font, lang);
            ui.end_row();

            let s = &mut cfg.scene;
            row(
                ui,
                th,
                t(lang, "Images par seconde max.", "Max frames per second"),
            );
            ui.horizontal(|ui| {
                for (v, label) in [
                    (0, t(lang, "Écran", "Display")),
                    (30, "30"),
                    (60, "60"),
                    (120, "120"),
                ] {
                    ui.selectable_value(&mut s.fps_cap, v, label);
                }
            });
            ui.end_row();

            row(ui, th, t(lang, "Rythme des orbites", "Orbit pace"));
            ui.add(
                egui::Slider::new(&mut s.time_scale, 0.25..=3.0)
                    .step_by(0.25)
                    .suffix(" ×"),
            );
            ui.end_row();

            row(ui, th, t(lang, "Mouvement réduit", "Reduced motion"));
            ui.checkbox(&mut s.reduced_motion, "").on_hover_text(t(
                lang,
                "Fige les orbites et ne garde que les animations essentielles.",
                "Freezes the orbits and keeps only the essential animations.",
            ));
            ui.end_row();

            row(
                ui,
                th,
                t(
                    lang,
                    "Économie hors focus (15 i/s)",
                    "Eco when unfocused (15 fps)",
                ),
            );
            ui.checkbox(&mut s.eco_unfocused, "");
            ui.end_row();
        });
}

fn agents(ui: &mut egui::Ui, th: &Theme, cfg: &mut AppConfig, lang: Lang) {
    let e = &mut cfg.engine;
    ui.horizontal(|ui| {
        row(ui, th, t(lang, "Budget quotidien", "Daily budget"));
        ui.add(
            egui::DragValue::new(&mut e.daily_budget_usd)
                .range(0.0..=500.0)
                .speed(0.1)
                .prefix("$ "),
        );
        ui.label(RichText::new(t(lang, "(0 = sans limite)", "(0 = no limit)")).color(th.pal.muted));
    });
    ui.horizontal(|ui| {
        row(
            ui,
            th,
            t(lang, "Recherches web par analyse", "Web searches per run"),
        );
        ui.add(egui::DragValue::new(&mut e.research_searches).range(1..=20));
    });
    ui.add_space(6.0);
    egui::Grid::new("agents-grid")
        .num_columns(3)
        .spacing([16.0, 8.0])
        .show(ui, |ui| {
            for agent in AgentId::ALL {
                let mut s = e.agent(agent);
                ui.label(RichText::new(agent_name(agent, lang)).color(th.pal.text));
                egui::ComboBox::from_id_salt(("model", agent))
                    .selected_text(&s.model)
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for m in MODELS {
                            ui.selectable_value(&mut s.model, m.to_owned(), m);
                        }
                    });
                egui::ComboBox::from_id_salt(("effort", agent))
                    .selected_text(s.effort.as_str())
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        for ef in Effort::ALL {
                            ui.selectable_value(&mut s.effort, ef, ef.as_str());
                        }
                    });
                ui.end_row();
                e.agents.insert(agent, s);
            }
        });
    ui.add_space(6.0);
    egui::CollapsingHeader::new(t(lang, "Consignes supplémentaires", "Extra instructions"))
        .id_salt("instructions")
        .show(ui, |ui| {
            for agent in AgentId::ALL {
                let mut s = e.agent(agent);
                ui.label(RichText::new(agent_name(agent, lang)).color(th.pal.muted));
                ui.add(
                    egui::TextEdit::multiline(&mut s.extra_instructions)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .hint_text(t(
                            lang,
                            "Ex. : privilégier les setups de cassure en séance US.",
                            "E.g. favour breakout setups during the US session.",
                        )),
                );
                e.agents.insert(agent, s);
            }
        });
}

fn data(
    ui: &mut egui::Ui,
    th: &Theme,
    cfg: &mut AppConfig,
    secrets: &mut Secrets,
    st: &mut UiState,
    bridge: &Bridge,
    lang: Lang,
) {
    ui.checkbox(
        &mut cfg.demo_mode,
        t(
            lang,
            "Mode démo (Claude et marché simulés, sans clé ni coût)",
            "Demo mode (simulated Claude and market, no keys, no cost)",
        ),
    );
    ui.add_space(8.0);
    let draft = st.draft_secrets.get_or_insert_with(|| secrets.clone());
    egui::Grid::new("keys")
        .num_columns(2)
        .spacing([24.0, 10.0])
        .show(ui, |ui| {
            for (label, value, hint) in [
                ("Anthropic (Claude)", &mut draft.anthropic, "sk-ant-…"),
                (
                    "Twelve Data",
                    &mut draft.twelve_data,
                    t(lang, "forex, indices, or", "forex, indices, gold"),
                ),
                (
                    "Alpaca key id",
                    &mut draft.alpaca_key_id,
                    t(lang, "actions US", "US stocks"),
                ),
                ("Alpaca secret", &mut draft.alpaca_secret, ""),
                (
                    "Finnhub",
                    &mut draft.finnhub,
                    t(lang, "news (optionnel)", "news (optional)"),
                ),
            ] {
                row(ui, th, label);
                ui.add(
                    egui::TextEdit::singleline(value)
                        .password(!st.show_keys)
                        .hint_text(hint)
                        .desired_width(320.0),
                );
                ui.end_row();
            }
        });
    ui.horizontal(|ui| {
        ui.checkbox(&mut st.show_keys, t(lang, "Afficher", "Show"));
        let dirty = st.draft_secrets.as_ref() != Some(secrets);
        if ui
            .add_enabled(
                dirty,
                egui::Button::new(t(lang, "Enregistrer les clés", "Save keys")),
            )
            .clicked()
            && let Some(d) = st.draft_secrets.clone()
        {
            match config::save_secrets(&d) {
                Ok(backend) => {
                    *secrets = Secrets {
                        backend: backend.clone(),
                        ..d
                    };
                    st.status_line = if backend == "keyring" {
                        t(
                            lang,
                            "Clés enregistrées dans le trousseau du système.",
                            "Keys saved in the system keyring.",
                        )
                        .to_owned()
                    } else {
                        format!(
                            "{} {backend}",
                            t(
                                lang,
                                "Trousseau indisponible ; clés enregistrées dans",
                                "No keyring; keys saved in"
                            )
                        )
                    };
                    bridge.sync(cfg, secrets);
                }
                Err(e) => st.status_line = e,
            }
        }
    });
    ui.label(
        RichText::new(&st.status_line)
            .size(12.0 * th.scale)
            .color(th.pal.muted),
    );
    ui.add_space(6.0);
    ui.label(
        RichText::new(t(
            lang,
            "Les clés restent sur cet ordinateur (trousseau Windows, Keychain macOS, Secret Service Linux). Binance ne demande pas de clé. Merces n'utilise que des API de lecture : aucun ordre n'est passé.",
            "Keys stay on this computer (Windows credential store, macOS Keychain, Linux Secret Service). Binance needs no key. Merces only uses read-only APIs: no order is placed.",
        ))
        .size(12.0 * th.scale)
        .color(th.pal.muted),
    );
}

fn risk(ui: &mut egui::Ui, th: &Theme, cfg: &mut AppConfig, lang: Lang) {
    let r = &mut cfg.engine.risk;
    egui::Grid::new("risk")
        .num_columns(2)
        .spacing([24.0, 12.0])
        .show(ui, |ui| {
            row(
                ui,
                th,
                t(
                    lang,
                    "Ratio rendement/risque minimum (TP1)",
                    "Minimum reward/risk (TP1)",
                ),
            );
            ui.add(
                egui::DragValue::new(&mut r.min_rr)
                    .range(0.5..=10.0)
                    .speed(0.05),
            );
            ui.end_row();
            row(
                ui,
                th,
                t(lang, "Risque max. par trade", "Max risk per trade"),
            );
            ui.add(
                egui::DragValue::new(&mut r.max_risk_pct)
                    .range(0.05..=10.0)
                    .speed(0.05)
                    .suffix(" %"),
            );
            ui.end_row();
            row(
                ui,
                th,
                t(
                    lang,
                    "Confiance minimale (signalée en dessous)",
                    "Minimum confidence (flagged below)",
                ),
            );
            ui.add(
                egui::DragValue::new(&mut r.min_confidence)
                    .range(0..=100)
                    .suffix(" %"),
            );
            ui.end_row();
            row(
                ui,
                th,
                t(lang, "Distance max. de l'entrée", "Max entry distance"),
            );
            ui.add(
                egui::DragValue::new(&mut r.max_entry_atr)
                    .range(0.1..=10.0)
                    .speed(0.05)
                    .suffix(" ATR"),
            );
            ui.end_row();
        });
    ui.add_space(6.0);
    ui.label(
        RichText::new(t(
            lang,
            "Ces limites sont données aux agents 3 et 4 puis vérifiées par le logiciel : un plan qui les enfreint est renvoyé une fois à l'agent, puis rejeté.",
            "These limits are given to agents 3 and 4, then checked by the software: a plan that breaks them goes back to the agent once, then is rejected.",
        ))
        .size(12.0 * th.scale)
        .color(th.pal.muted),
    );
}

fn about(ui: &mut egui::Ui, th: &Theme, lang: Lang) {
    ui.label(RichText::new(super::overlay::disclaimer(lang)).color(th.pal.text));
    ui.add_space(8.0);
    egui::Grid::new("about")
        .num_columns(2)
        .spacing([24.0, 12.0])
        .show(ui, |ui| {
            row(ui, th, "Version");
            ui.label(theme::mono(th, env!("CARGO_PKG_VERSION")));
            ui.end_row();
            row(ui, th, t(lang, "Données", "Data"));
            ui.label(
                theme::mono(th, config::data_dir().display().to_string()).size(12.0 * th.scale),
            );
            ui.end_row();
            row(ui, th, t(lang, "Configuration", "Configuration"));
            ui.label(
                theme::mono(
                    th,
                    config::config_path()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                )
                .size(12.0 * th.scale),
            );
            ui.end_row();
        });
    ui.add_space(8.0);
    ui.label(
        RichText::new(t(
            lang,
            "Thème Helix : palette des projets org-helix (encre, ambre pâle, rouge), titres Bricolage Grotesque, texte Geist (SIL OFL). Personnages et ciel dessinés par le code, originaux (GPL-3.0).",
            "Helix theme: the org-helix palette (ink, pale amber, red), Bricolage Grotesque titles, Geist text (SIL OFL). Characters and sky drawn in code, original (GPL-3.0).",
        ))
        .size(12.0 * th.scale)
        .color(th.pal.muted),
    );
}
