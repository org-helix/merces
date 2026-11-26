//! The 3D network: the active symbol is the input neuron, the four pipeline
//! agents (characters) follow it in a chain linked by synapses, the assistant
//! floats above; today's results float beside their agent as badges,
//! hand-offs travel along the synapses, and an orbit camera with inertia flies
//! to an agent when it is clicked.

mod agents;
mod anim;
mod camera;
pub mod graph;
mod sky;
mod voxel;

use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::WindowFocused;
use bevy_framepace::{FramepaceSettings, Limiter};
use merces_core::AgentId;

pub use camera::OrbitCam;

use crate::config::AppConfig;
use crate::theme::{self, Theme};

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .init_resource::<Layout>()
            .add_plugins((sky::SkyPlugin, graph::GraphPlugin))
            .add_systems(Startup, setup)
            .configure_sets(Update, graph::GraphSet.after(crate::bridge::pump))
            .add_systems(
                Update,
                (
                    camera::camera_control,
                    anim::triggers,
                    anim::animate,
                    (
                        anim::handoffs,
                        anim::sparks,
                        agents::badges,
                        agents::body_colors,
                        agents::pedestals,
                    ),
                    render_settings,
                )
                    .chain()
                    .after(graph::GraphSet),
            );
    }
}

// ── Resources & components ────────────────────────────────────────────────

#[derive(Resource, Default)]
pub struct Selection {
    pub agent: Option<AgentId>,
    pub hovered: Option<AgentId>,
    /// The input neuron (active symbol) is under the pointer.
    pub input_hovered: bool,
    /// Result badge under the pointer: (agent, index in today's results).
    pub badge: Option<(AgentId, usize)>,
    /// Badge clicked: index into that agent's results of the day.
    pub output: Option<usize>,
    /// Right drawer width minus left drawer width, in logical pixels, set by
    /// the UI: the camera shifts so the focus stays centred in the free space.
    pub panel_px: f32,
    /// A node is being dragged.
    pub dragging: bool,
    /// How far (pixels) the current press has dragged a node: a click after a
    /// drag does not open the agent.
    pub drag_px: f32,
}

/// Where every node is this frame.
#[derive(Resource, Default)]
pub struct Layout {
    pub agents: [Vec3; 5],
    /// Bounding radius of each character in world units.
    pub sizes: [f32; 5],
    /// Rotation of each character around its own axis (radians).
    pub spins: [f32; 5],
    /// The input neuron (active symbol).
    pub input: Vec3,
    pub input_size: f32,
}

// ── Setup ──────────────────────────────────────────────────────────────────

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    cfg: Res<AppConfig>,
    theme: Res<Theme>,
) {
    agents::spawn_characters(&mut commands, &mut meshes, &mut mats, &theme);
    agents::setup_badges(&mut commands, &mut meshes, &mut mats);
    anim::setup_fx(&mut commands, &mut meshes, &mut mats);

    let mut cam = commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(theme::bevy_color(theme::DARK.canvas)),
            ..default()
        },
        Hdr,
        Tonemapping::TonyMcMapface,
        Transform::from_xyz(0.0, 6.0, 30.0).looking_at(graph::CENTER, Vec3::Y),
        OrbitCam::default(),
        if cfg.scene.quality.msaa() {
            Msaa::Sample4
        } else {
            Msaa::Off
        },
    ));
    if cfg.scene.quality.bloom() {
        cam.insert(Bloom::NATURAL);
    }
}

/// Apply the quality setting to the camera and the frame limit to the pacer.
fn render_settings(
    mut commands: Commands,
    cfg: Res<AppConfig>,
    mut focus: MessageReader<WindowFocused>,
    mut focused: Local<Option<bool>>,
    mut last_limit: Local<Option<(bool, u32)>>,
    mut pace: ResMut<FramepaceSettings>,
    mut cams: Query<(Entity, &mut Msaa, Option<&Bloom>), With<Camera3d>>,
) {
    for f in focus.read() {
        *focused = Some(f.focused);
    }
    let unfocused = *focused == Some(false) && cfg.scene.eco_unfocused;
    let limiter = if unfocused {
        Limiter::from_framerate(15.0)
    } else if cfg.scene.fps_cap == 0 {
        // Follow the display: vsync alone paces the frames. The pacer's own
        // "auto" mode added up to one missed vblank per frame (≈55 fps on a
        // 60 Hz screen, measured with MERCES_BENCH).
        Limiter::Off
    } else {
        Limiter::from_framerate(f64::from(cfg.scene.fps_cap.clamp(15, 360)))
    };
    let key = (unfocused, cfg.scene.fps_cap);
    if *last_limit != Some(key) {
        pace.limiter = limiter;
        *last_limit = Some(key);
    }
    if !cfg.is_changed() {
        return;
    }
    let q = cfg.scene.quality;
    for (e, mut msaa, bloom) in &mut cams {
        let want = if q.msaa() { Msaa::Sample4 } else { Msaa::Off };
        if *msaa != want {
            *msaa = want;
        }
        match (q.bloom(), bloom.is_some()) {
            (true, false) => {
                commands.entity(e).insert(Bloom::NATURAL);
            }
            (false, true) => {
                commands.entity(e).remove::<Bloom>();
            }
            _ => {}
        }
    }
}
