//! Merces: a 3D dashboard of AI trading agents.
//!
//! Four agents (research → chart → decision → execution) orbit the active
//! symbol, a star; they hand their work to each other; click one to see its
//! work and today's results. A separate assistant helps with the app and
//! trading questions.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Bevy systems take their dependencies as parameters.
#![allow(clippy::too_many_arguments)]

mod bridge;
mod config;
mod perf;
mod scene;
mod state;
mod theme;
mod ui;
mod window;

use bevy::prelude::*;
use bevy_egui::EguiPlugin;

fn main() {
    let started = perf::Started(std::time::Instant::now());
    let cfg = config::load();
    let secrets = config::load_secrets();
    let bridge = bridge::Bridge::start(&cfg, &secrets);
    let theme = theme::Theme {
        pal: theme::palette(&cfg.appearance, true),
        scale: cfg.appearance.font_scale.clamp(0.8, 1.5),
        os_dark: true,
    };

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(window::primary()),
                    ..default()
                })
                .set(bevy::log::LogPlugin {
                    filter: "wgpu=error,naga=warn,bevy_render=warn,merces=info".into(),
                    ..default()
                }),
        )
        .add_plugins((
            MeshPickingPlugin,
            EguiPlugin::default(),
            bevy_framepace::FramepacePlugin,
        ))
        .insert_resource(cfg)
        .insert_resource(secrets)
        .insert_resource(bridge)
        .insert_resource(theme)
        .insert_resource(started)
        .init_resource::<state::Hub>()
        .add_systems(Update, bridge::pump)
        .add_plugins((
            scene::ScenePlugin,
            ui::UiPlugin,
            window::FramelessPlugin,
            perf::PerfPlugin,
        ))
        .run();
}
