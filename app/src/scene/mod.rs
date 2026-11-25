//! The 3D universe: the active symbol is the central star, the four pipeline
//! agents and the assistant are characters on Keplerian orbits, today's
//! results float beside their agent as badges, hand-offs fly between them, and
//! an orbit camera with inertia flies to an agent when it is clicked.

mod agents;
mod anim;
mod camera;
mod orbits;
mod sky;

use bevy::asset::RenderAssetUsages;
use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::mesh::PrimitiveTopology;
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
            .init_resource::<SimClock>()
            .add_plugins(sky::SkyPlugin)
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    layout,
                    camera::camera_control,
                    anim::triggers,
                    anim::animate,
                    (
                        anim::handoffs,
                        anim::sparks,
                        agents::badges,
                        agents::body_colors,
                        orbit_highlight,
                    ),
                    render_settings,
                )
                    .chain()
                    .after(crate::bridge::pump),
            );
    }
}

// ── Resources & components ────────────────────────────────────────────────

#[derive(Resource, Default)]
pub struct Selection {
    pub agent: Option<AgentId>,
    pub hovered: Option<AgentId>,
    /// Result badge under the pointer: (agent, index in today's results).
    pub badge: Option<(AgentId, usize)>,
    /// Badge clicked: index into that agent's results of the day.
    pub output: Option<usize>,
    /// Right panel width in logical pixels (0 when closed), set by the UI.
    pub panel_px: f32,
}

/// Where every body is this frame.
#[derive(Resource, Default)]
pub struct Layout {
    pub agents: [Vec3; 5],
    /// Bounding radius of each character in world units.
    pub sizes: [f32; 5],
    /// Rotation of each character around its own axis (radians).
    pub spins: [f32; 5],
}

/// Simulation time of the solar system: advances with the time scale and
/// stops (without jumping) under reduced motion.
#[derive(Resource, Default)]
pub struct SimClock {
    pub t: f64,
}

#[derive(Component)]
struct OrbitLine {
    agent: AgentId,
    material: Handle<StandardMaterial>,
}

#[derive(Resource)]
struct Orbits([orbits::Orbit; 5]);

const ORBIT_ALPHA: f32 = 0.07;
const ORBIT_ALPHA_FOCUS: f32 = 0.2;

// ── Setup ──────────────────────────────────────────────────────────────────

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    cfg: Res<AppConfig>,
    theme: Res<Theme>,
) {
    let orbits = orbits::agent_orbits();
    agents::spawn_characters(&mut commands, &mut meshes, &mut mats, &theme);
    agents::setup_badges(&mut commands, &mut meshes, &mut mats);
    anim::setup_fx(&mut commands, &mut meshes, &mut mats);

    // Orbits: static line meshes, built once (the ellipses never change).
    for (agent, o) in AgentId::ALL.into_iter().zip(orbits) {
        let points: Vec<[f32; 3]> = (0..=256)
            .map(|k| {
                o.at_eccentric_anomaly(k as f32 / 256.0 * std::f32::consts::TAU)
                    .to_array()
            })
            .collect();
        let mesh = Mesh::new(
            PrimitiveTopology::LineStrip,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, points);
        let material = mats.add(StandardMaterial {
            base_color: theme::bevy_color(theme::AMBER).with_alpha(ORBIT_ALPHA),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            OrbitLine { agent, material },
            Pickable::IGNORE,
        ));
    }
    commands.insert_resource(Orbits(orbits));

    let mut cam = commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(theme::bevy_color(theme::DARK.canvas)),
            ..default()
        },
        Hdr,
        Tonemapping::TonyMcMapface,
        Transform::from_xyz(0.0, 20.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
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

// ── Per-frame systems ──────────────────────────────────────────────────────

/// Advance the clock and put every character on its orbit.
fn layout(
    time: Res<Time>,
    cfg: Res<AppConfig>,
    orbits: Res<Orbits>,
    mut clock: ResMut<SimClock>,
    mut lay: ResMut<Layout>,
    mut chars: Query<(&agents::Character, &mut Transform)>,
) {
    if !cfg.scene.reduced_motion {
        clock.t += f64::from(time.delta_secs() * cfg.scene.time_scale.clamp(0.0, 4.0));
    }
    for (c, mut tf) in &mut chars {
        let i = c.agent.index();
        let o = &orbits.0[i];
        let pos = o.position(clock.t);
        tf.translation = pos;
        if tf.scale.x != c.scale {
            tf.scale = Vec3::splat(c.scale);
        }
        lay.agents[i] = pos;
        lay.sizes[i] = o.body_radius;
        lay.spins[i] = o.spin(clock.t);
    }
}

/// The selected agent's orbit is drawn a little brighter (only on change).
fn orbit_highlight(
    sel: Res<Selection>,
    lines: Query<&OrbitLine>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut last: Local<Option<Option<AgentId>>>,
) {
    let focus = sel.agent.or(sel.hovered);
    if *last == Some(focus) {
        return;
    }
    *last = Some(focus);
    for l in &lines {
        let a = if focus == Some(l.agent) {
            ORBIT_ALPHA_FOCUS
        } else {
            ORBIT_ALPHA
        };
        if let Some(mut m) = mats.get_mut(&l.material) {
            m.base_color = m.base_color.with_alpha(a);
        }
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
        Limiter::Auto
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
