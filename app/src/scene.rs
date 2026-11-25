//! The 3D universe: the active symbol as a sun, the four pipeline agents as
//! planets, the assistant as a detached station, today's results as moons,
//! glowing particles for messages, and an orbit camera that flies to a planet
//! when it is clicked. A second layout (graph, Obsidian-like) blends in smoothly.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::WindowFocused;
use bevy_egui::egui::Color32;
use bevy_egui::input::EguiWantsInput;
use bevy_framepace::{FramepaceSettings, Limiter};
use merces_agents::demo::Rng;
use merces_core::{AgentId, AgentStatus};

use crate::config::{AppConfig, ViewMode};
use crate::state::Hub;
use crate::theme::{self, Theme};

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>()
            .init_resource::<Layout>()
            .init_resource::<Pulses>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    layout,
                    (
                        planet_looks,
                        sun_looks,
                        moons,
                        spawn_flows,
                        move_particles,
                        links_and_orbits,
                        stars,
                    ),
                    camera_control,
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
    /// Moon clicked: index into that agent's results of the day.
    pub output: Option<usize>,
    /// Right panel width in logical pixels (0 when closed), set by the UI.
    pub panel_px: f32,
}

/// Where every body is this frame.
#[derive(Resource, Default)]
pub struct Layout {
    pub agents: [Vec3; 5],
    pub sizes: [f32; 5],
    pub sun: Vec3,
    pub sun_size: f32,
    /// 0 = solar system, 1 = graph.
    pub blend: f32,
}

/// Planets that just received a message pulse briefly.
#[derive(Resource, Default)]
struct Pulses([f64; 5]);

#[derive(Component)]
struct Planet {
    agent: AgentId,
    orbit: f32,
    size: f32,
    phase: f32,
    speed: f32,
    tilt: f32,
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct Halo(AgentId, Handle<StandardMaterial>);

#[derive(Component)]
struct Ring(AgentId);

#[derive(Component)]
struct Sun(Handle<StandardMaterial>);

#[derive(Component)]
struct Moon {
    agent: AgentId,
    index: usize,
}

#[derive(Component)]
struct Star;

#[derive(Component)]
struct Particle {
    from: AgentId,
    to: AgentId,
    t: f32,
    delay: f32,
}

#[derive(Component)]
pub struct OrbitCam {
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub target: Vec3,
    goal_yaw: f32,
    goal_pitch: f32,
    goal_dist: f32,
    drag: f32,
}

#[derive(Resource)]
struct Assets3d {
    sphere: Handle<Mesh>,
    moon_mesh: Handle<Mesh>,
    moon_mat: Handle<StandardMaterial>,
    particle_mat: Handle<StandardMaterial>,
    star_mat: Handle<StandardMaterial>,
}

pub fn agent_color(agent: AgentId, pal: &theme::Palette) -> Color32 {
    match agent {
        AgentId::Research => theme::BLUSH,
        AgentId::Chart => theme::PALE_ORANGE,
        AgentId::Decision => pal.accent,
        AgentId::Execution => {
            if pal.dark {
                theme::AMBER
            } else {
                theme::DUSTY
            }
        }
        AgentId::Assistant => theme::DUSTY,
    }
}

fn linear(c: Color32) -> LinearRgba {
    theme::bevy_color(c).to_linear()
}

const ORBITS: [(f32, f32, f32); 5] = [
    // (orbit radius, size, angular speed)
    (5.0, 0.62, 0.16),
    (7.4, 0.72, 0.12),
    (9.8, 0.80, 0.095),
    (12.2, 0.70, 0.075),
    (16.0, 0.38, 0.05),
];

/// Graph layout (Obsidian-like): the pipeline as a gentle arc, assistant apart.
const GRAPH: [Vec3; 5] = [
    Vec3::new(-8.5, 1.2, 1.5),
    Vec3::new(-3.0, -1.0, -1.0),
    Vec3::new(3.0, 1.0, 1.0),
    Vec3::new(8.5, -1.2, -1.5),
    Vec3::new(0.0, 6.0, -7.0),
];

// ── Setup ──────────────────────────────────────────────────────────────────

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    cfg: Res<AppConfig>,
    theme: Res<Theme>,
) {
    let pal = theme.pal;
    let sphere = meshes.add(Sphere::new(1.0).mesh().ico(5).expect("ico sphere"));
    let low_sphere = meshes.add(Sphere::new(1.0).mesh().ico(1).expect("ico sphere"));
    let ring_mesh = meshes.add(Torus::new(1.25, 1.35));
    let moon_mesh = meshes.add(Sphere::new(1.0).mesh().ico(2).expect("ico sphere"));

    let star_mat = mats.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: linear(theme::AMBER) * 1.6,
        unlit: true,
        ..default()
    });
    let particle_mat = mats.add(StandardMaterial {
        base_color: theme::bevy_color(pal.accent),
        emissive: linear(theme::PALE_ORANGE) * 14.0,
        unlit: true,
        ..default()
    });
    let moon_mat = mats.add(StandardMaterial {
        base_color: theme::bevy_color(theme::BLUSH),
        emissive: linear(theme::BLUSH) * 0.8,
        perceptual_roughness: 0.8,
        ..default()
    });

    // Sun: the active symbol.
    let sun_mat = mats.add(StandardMaterial {
        base_color: theme::bevy_color(pal.accent),
        emissive: linear(pal.accent) * 6.0,
        ..default()
    });
    commands
        .spawn((
            Mesh3d(sphere.clone()),
            MeshMaterial3d(sun_mat.clone()),
            Transform::from_scale(Vec3::splat(1.7)),
            Sun(sun_mat),
            Name::new("sun"),
        ))
        .observe(|_: On<PointerClick>, mut sel: ResMut<Selection>| {
            sel.agent = None;
            sel.output = None;
        });
    commands.spawn((
        PointLight {
            intensity: 6_000_000.0,
            range: 200.0,
            shadow_maps_enabled: false,
            color: theme::bevy_color(theme::PALE_ORANGE),
            ..default()
        },
        Transform::default(),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: theme::bevy_color(theme::AMBER),
        brightness: 120.0,
        ..default()
    });

    // Planets.
    for (i, agent) in AgentId::ALL.into_iter().enumerate() {
        let (orbit, size, speed) = ORBITS[i];
        let color = agent_color(agent, &pal);
        let material = mats.add(StandardMaterial {
            base_color: theme::bevy_color(color),
            emissive: linear(color) * 0.05,
            perceptual_roughness: 0.65,
            ..default()
        });
        let halo_mat = mats.add(StandardMaterial {
            base_color: theme::bevy_color(color).with_alpha(0.12),
            emissive: linear(color) * 0.6,
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });
        let mesh = if agent == AgentId::Assistant {
            low_sphere.clone()
        } else {
            sphere.clone()
        };
        commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                Transform::from_scale(Vec3::splat(size)),
                Planet {
                    agent,
                    orbit,
                    size,
                    phase: i as f32 * 1.9 + 0.4,
                    speed,
                    tilt: if agent == AgentId::Assistant {
                        0.35
                    } else {
                        0.04 * i as f32
                    },
                    material,
                },
                Name::new(agent.key()),
            ))
            .with_children(|p| {
                p.spawn((
                    Mesh3d(sphere.clone()),
                    MeshMaterial3d(halo_mat.clone()),
                    Transform::from_scale(Vec3::splat(1.55)),
                    Visibility::Hidden,
                    Halo(agent, halo_mat),
                    Pickable::IGNORE,
                ));
                p.spawn((
                    Mesh3d(ring_mesh.clone()),
                    MeshMaterial3d(particle_mat.clone()),
                    Transform::from_rotation(Quat::from_rotation_x(0.35)),
                    Visibility::Hidden,
                    Ring(agent),
                    Pickable::IGNORE,
                ));
            })
            .observe(move |ev: On<PointerClick>, mut sel: ResMut<Selection>| {
                if ev.button == PointerButton::Primary {
                    sel.agent = Some(agent);
                    sel.output = None;
                }
            })
            .observe(move |_: On<PointerOver>, mut sel: ResMut<Selection>| {
                sel.hovered = Some(agent)
            })
            .observe(move |_: On<PointerOut>, mut sel: ResMut<Selection>| {
                if sel.hovered == Some(agent) {
                    sel.hovered = None;
                }
            });
    }

    // Camera.
    let mut cam = commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(theme::bevy_color(pal.canvas)),
            ..default()
        },
        Hdr,
        Tonemapping::TonyMcMapface,
        Transform::from_xyz(0.0, 14.0, 26.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCam {
            yaw: 0.0,
            pitch: 0.5,
            dist: 30.0,
            target: Vec3::ZERO,
            goal_yaw: 0.0,
            goal_pitch: 0.5,
            goal_dist: 30.0,
            drag: 0.0,
        },
        if cfg.scene.msaa {
            Msaa::Sample4
        } else {
            Msaa::Off
        },
    ));
    if cfg.scene.bloom {
        cam.insert(Bloom::NATURAL);
    }

    commands.insert_resource(Assets3d {
        sphere,
        moon_mesh,
        moon_mat,
        particle_mat,
        star_mat,
    });
}

// ── Per-frame systems ──────────────────────────────────────────────────────

fn layout(
    time: Res<Time>,
    cfg: Res<AppConfig>,
    mut lay: ResMut<Layout>,
    pulses: Res<Pulses>,
    mut planets: Query<(&Planet, &mut Transform), Without<Sun>>,
    mut sun: Query<&mut Transform, With<Sun>>,
) {
    let dt = time.delta_secs();
    let goal = if cfg.scene.view == ViewMode::Graph {
        1.0
    } else {
        0.0
    };
    let k = if cfg.scene.reduced_motion {
        1.0
    } else {
        1.0 - (-3.0 * dt).exp()
    };
    lay.blend += (goal - lay.blend) * k;
    let b = lay.blend * lay.blend * (3.0 - 2.0 * lay.blend);
    let t = time.elapsed_secs();
    let speed = if cfg.scene.reduced_motion {
        0.0
    } else {
        cfg.scene.orbit_speed
    };
    let now = time.elapsed_secs_f64();
    for (p, mut tf) in &mut planets {
        let i = p.agent.index();
        let a = p.phase + t * p.speed * speed;
        let solar = Vec3::new(
            a.cos() * p.orbit,
            (a * 0.7).sin() * p.orbit * p.tilt,
            a.sin() * p.orbit,
        );
        let bob = Vec3::Y * (t * 0.6 + i as f32).sin() * 0.25 * speed.min(1.0);
        let pos = solar.lerp(GRAPH[i] + bob, b);
        let since = (now - pulses.0[i]) as f32;
        let pulse = if since < 0.6 {
            1.0 + 0.25 * (1.0 - since / 0.6)
        } else {
            1.0
        };
        let size = p.size * pulse * (1.0 + 0.25 * b);
        tf.translation = pos;
        tf.scale = Vec3::splat(size);
        tf.rotation = Quat::from_rotation_y(t * 0.2 * speed + i as f32);
        lay.agents[i] = pos;
        lay.sizes[i] = size;
    }
    let sun_size = 1.7 * (1.0 - 0.55 * b);
    if let Ok(mut tf) = sun.single_mut() {
        tf.translation = Vec3::new(0.0, -2.5 * b, 0.0);
        tf.scale = Vec3::splat(sun_size);
        lay.sun = tf.translation;
    }
    lay.sun_size = sun_size;
}

fn planet_looks(
    time: Res<Time>,
    hub: Res<Hub>,
    cfg: Res<AppConfig>,
    theme: Res<Theme>,
    sel: Res<Selection>,
    planets: Query<&Planet>,
    mut halos: Query<(&Halo, &mut Visibility, &mut Transform), Without<Ring>>,
    mut rings: Query<(&Ring, &mut Visibility, &mut Transform), Without<Halo>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let t = time.elapsed_secs();
    let pal = theme.pal;
    let view = hub.symbol(&cfg.active_symbol);
    let status = |a: AgentId| -> AgentStatus {
        if a == AgentId::Assistant {
            return if hub.chat_busy {
                AgentStatus::Working
            } else {
                AgentStatus::Idle
            };
        }
        view.map(|v| v.agent(a).status).unwrap_or_default()
    };
    let calm = cfg.scene.reduced_motion;
    for p in &planets {
        let st = status(p.agent);
        let base = agent_color(p.agent, &pal);
        let pulse = if calm {
            0.5
        } else {
            0.5 + 0.5 * (t * 3.2).sin()
        };
        let (color, glow) = match st {
            AgentStatus::Idle => (base, 0.04),
            AgentStatus::Working => (base, 0.6 + 1.8 * pulse),
            AgentStatus::Done => (base, 0.9),
            AgentStatus::NoTrade => (theme::RUST, 0.7),
            AgentStatus::Error => (pal.error, 0.4 + 1.2 * pulse),
        };
        let lift = if sel.agent == Some(p.agent) || sel.hovered == Some(p.agent) {
            0.5
        } else {
            0.0
        };
        if let Some(mut m) = mats.get_mut(&p.material) {
            m.base_color = theme::bevy_color(color);
            m.emissive = linear(color) * (glow + lift);
        }
        let working = st == AgentStatus::Working;
        for (h, mut vis, mut tf) in &mut halos {
            if h.0 == p.agent {
                *vis = if working || lift > 0.0 {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                tf.scale = Vec3::splat(1.45 + 0.15 * pulse);
                if let Some(mut m) = mats.get_mut(&h.1) {
                    m.emissive = linear(color) * (0.25 + 0.6 * pulse);
                }
            }
        }
        for (r, mut vis, mut tf) in &mut rings {
            if r.0 == p.agent {
                *vis = if working {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
                tf.rotation = Quat::from_rotation_x(0.35) * Quat::from_rotation_y(t * 1.6);
            }
        }
    }
}

fn sun_looks(
    time: Res<Time>,
    hub: Res<Hub>,
    cfg: Res<AppConfig>,
    theme: Res<Theme>,
    sun: Query<&Sun>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let busy = hub
        .symbol(&cfg.active_symbol)
        .is_some_and(|v| v.running.is_some());
    let t = time.elapsed_secs();
    let k = if busy && !cfg.scene.reduced_motion {
        5.0 + 2.0 * (t * 2.0).sin()
    } else {
        4.0
    };
    for s in &sun {
        if let Some(mut m) = mats.get_mut(&s.0) {
            m.base_color = theme::bevy_color(theme.pal.accent);
            m.emissive = linear(theme.pal.accent) * k;
        }
    }
}

/// Moons = today's results of each agent for the active symbol (max 6 each).
fn moons(
    mut commands: Commands,
    time: Res<Time>,
    hub: Res<Hub>,
    cfg: Res<AppConfig>,
    lay: Res<Layout>,
    assets: Res<Assets3d>,
    mut existing: Query<(Entity, &Moon, &mut Transform)>,
) {
    let view = hub.symbol(&cfg.active_symbol);
    let wanted = |a: AgentId| view.map_or(0, |v| v.today(a).count().min(6));
    // Despawn moons that are no longer wanted.
    let mut have = [0usize; 5];
    for (e, m, _) in &existing {
        if m.index >= wanted(m.agent) {
            commands.entity(e).despawn();
        } else {
            have[m.agent.index()] += 1;
        }
    }
    for agent in AgentId::PIPELINE {
        for index in have[agent.index()]..wanted(agent) {
            commands
                .spawn((
                    Mesh3d(assets.moon_mesh.clone()),
                    MeshMaterial3d(assets.moon_mat.clone()),
                    Transform::from_scale(Vec3::splat(0.14)),
                    Moon { agent, index },
                ))
                .observe(move |ev: On<PointerClick>, mut sel: ResMut<Selection>| {
                    if ev.button == PointerButton::Primary {
                        sel.agent = Some(agent);
                        sel.output = Some(index);
                    }
                });
        }
    }
    let speed = if cfg.scene.reduced_motion {
        0.0
    } else {
        cfg.scene.orbit_speed
    };
    let t = time.elapsed_secs();
    for (_, m, mut tf) in &mut existing {
        let i = m.agent.index();
        let r = lay.sizes[i] * 1.9 + 0.25 * m.index as f32;
        let a = t * 0.9 * speed + m.index as f32 * TAU / 6.0;
        tf.translation =
            lay.agents[i] + Vec3::new(a.cos() * r, (a * 1.3).sin() * 0.25, a.sin() * r);
    }
}

fn spawn_flows(mut commands: Commands, mut hub: ResMut<Hub>, assets: Res<Assets3d>) {
    for f in std::mem::take(&mut hub.flows) {
        for k in 0..4 {
            commands.spawn((
                Mesh3d(assets.sphere.clone()),
                MeshMaterial3d(assets.particle_mat.clone()),
                Transform::from_scale(Vec3::splat(0.0)),
                Particle {
                    from: f.from,
                    to: f.to,
                    t: 0.0,
                    delay: k as f32 * 0.12,
                },
                Pickable::IGNORE,
            ));
        }
    }
}

fn bezier(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    let mid = (a + b) * 0.5 + Vec3::Y * a.distance(b) * 0.35;
    a.lerp(mid, t).lerp(mid.lerp(b, t), t)
}

fn move_particles(
    mut commands: Commands,
    time: Res<Time>,
    lay: Res<Layout>,
    mut pulses: ResMut<Pulses>,
    mut q: Query<(Entity, &mut Particle, &mut Transform)>,
) {
    let dt = time.delta_secs();
    const DURATION: f32 = 1.5;
    for (e, mut p, mut tf) in &mut q {
        if p.delay > 0.0 {
            p.delay -= dt;
            continue;
        }
        p.t += dt / DURATION;
        if p.t >= 1.0 {
            pulses.0[p.to.index()] = time.elapsed_secs_f64();
            commands.entity(e).despawn();
            continue;
        }
        let (a, b) = (lay.agents[p.from.index()], lay.agents[p.to.index()]);
        tf.translation = bezier(a, b, p.t);
        tf.scale = Vec3::splat(0.12 * (1.0 - (2.0 * p.t - 1.0).powi(4)) + 0.02);
    }
}

fn links_and_orbits(
    mut gizmos: Gizmos,
    lay: Res<Layout>,
    theme: Res<Theme>,
    particles: Query<&Particle>,
    planets: Query<&Planet>,
) {
    let pal = theme.pal;
    let wire = theme::bevy_color(pal.wire);
    let solar = 1.0 - lay.blend;
    if solar > 0.02 {
        let c = theme::bevy_color(pal.hairline).with_alpha(0.35 * solar);
        for p in &planets {
            let iso = Isometry3d::new(Vec3::ZERO, Quat::from_rotation_x(FRAC_PI_2));
            gizmos.circle(iso, p.orbit, c).resolution(128);
        }
    }
    // Pipeline links, brighter while a message travels on them.
    for pair in AgentId::PIPELINE.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let active = particles.iter().any(|p| p.from == a && p.to == b);
        let color = if active {
            theme::bevy_color(pal.accent).with_alpha(0.9)
        } else {
            wire.with_alpha(0.25 + 0.35 * lay.blend)
        };
        let (pa, pb) = (lay.agents[a.index()], lay.agents[b.index()]);
        gizmos.linestrip((0..=24).map(|i| bezier(pa, pb, i as f32 / 24.0)), color);
    }
    // Graph view: every agent hangs off the symbol.
    if lay.blend > 0.02 {
        let c = wire.with_alpha(0.18 * lay.blend);
        for i in 0..5 {
            gizmos.line(lay.sun, lay.agents[i], c);
        }
    }
}

fn stars(
    mut commands: Commands,
    cfg: Res<AppConfig>,
    assets: Res<Assets3d>,
    existing: Query<Entity, With<Star>>,
    mut count: Local<Option<u32>>,
) {
    let want = cfg.scene.stars.min(5_000);
    if *count == Some(want) {
        return;
    }
    *count = Some(want);
    for e in &existing {
        commands.entity(e).despawn();
    }
    let mut rng = Rng::new(42);
    for _ in 0..want {
        // Uniform on a thick shell, kept away from the system.
        let z = rng.f64() as f32 * 2.0 - 1.0;
        let a = rng.f64() as f32 * TAU;
        let r = 70.0 + rng.f64() as f32 * 90.0;
        let s = (1.0 - z * z).sqrt();
        let pos = Vec3::new(s * a.cos(), z, s * a.sin()) * r;
        commands.spawn((
            Mesh3d(assets.sphere.clone()),
            MeshMaterial3d(assets.star_mat.clone()),
            Transform::from_translation(pos)
                .with_scale(Vec3::splat(0.08 + rng.f64() as f32 * 0.16)),
            Star,
            Pickable::IGNORE,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn camera_control(
    time: Res<Time>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    egui: Res<EguiWantsInput>,
    lay: Res<Layout>,
    cfg: Res<AppConfig>,
    windows: Query<&Window>,
    mut sel: ResMut<Selection>,
    mut cams: Query<(&mut Transform, &mut OrbitCam, &Projection)>,
    mut last: Local<Option<AgentId>>,
) {
    let Ok((mut tf, mut cam, proj)) = cams.single_mut() else {
        return;
    };
    let dt = time.delta_secs();
    let over_ui = egui.wants_any_pointer_input();

    if keys.just_pressed(KeyCode::Escape) && !egui.wants_any_keyboard_input() {
        sel.agent = None;
        sel.output = None;
    }
    let press = buttons.just_pressed(MouseButton::Left) || buttons.just_pressed(MouseButton::Right);
    if press {
        cam.drag = 0.0;
    }
    if !over_ui {
        // The motion reported on the press frame happened before the press: ignore it.
        if !press && (buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Right)) {
            cam.goal_yaw -= motion.delta.x * 0.005;
            cam.goal_pitch = (cam.goal_pitch + motion.delta.y * 0.004).clamp(-1.35, 1.35);
            cam.drag += motion.delta.length();
        }
        let lines = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        if lines != 0.0 {
            cam.goal_dist = (cam.goal_dist * (1.0 - lines * 0.1)).clamp(2.0, 80.0);
        }
        // A short click on empty space returns to the overview.
        if buttons.just_released(MouseButton::Left)
            && cam.drag < 4.0
            && sel.hovered.is_none()
            && sel.agent.is_some()
        {
            sel.agent = None;
            sel.output = None;
        }
    }

    // Goals: focus a planet, or frame the whole system.
    let (goal_target, min_dist) = match sel.agent {
        Some(a) => (lay.agents[a.index()], lay.sizes[a.index()] * 6.0),
        None => (Vec3::new(0.0, -0.5 * lay.blend, 0.0), 0.0),
    };
    if *last != sel.agent {
        *last = sel.agent;
        cam.goal_dist = if sel.agent.is_some() {
            min_dist.max(3.5)
        } else {
            30.0
        };
    }

    let k = if cfg.scene.reduced_motion {
        1.0
    } else {
        1.0 - (-5.0 * dt).exp()
    };
    cam.yaw += (cam.goal_yaw - cam.yaw) * k;
    cam.pitch += (cam.goal_pitch - cam.pitch) * k;
    cam.dist += (cam.goal_dist - cam.dist) * k;

    // Keep a focused planet left of the side panel.
    let offset = match (sel.agent, proj, windows.single()) {
        (Some(_), Projection::Perspective(p), Ok(w)) if sel.panel_px > 0.0 => {
            let half_w = (p.fov * 0.5).tan() * cam.dist * w.width() / w.height().max(1.0);
            half_w * (sel.panel_px / w.width().max(1.0))
        }
        _ => 0.0,
    };
    let rot = Quat::from_rotation_y(cam.yaw) * Quat::from_rotation_x(-cam.pitch);
    let right = rot * Vec3::X;
    let goal = goal_target + right * offset;
    let kt = if cfg.scene.reduced_motion {
        1.0
    } else {
        1.0 - (-4.0 * dt).exp()
    };
    cam.target = cam.target.lerp(goal, kt);
    tf.translation = cam.target + rot * Vec3::new(0.0, 0.0, cam.dist);
    tf.look_at(cam.target, Vec3::Y);
}

/// Apply theme colours and render settings to the camera and frame limiter.
fn render_settings(
    mut commands: Commands,
    cfg: Res<AppConfig>,
    theme: Res<Theme>,
    mut focus: MessageReader<WindowFocused>,
    mut focused: Local<Option<bool>>,
    mut last_limit: Local<Option<String>>,
    mut pace: ResMut<FramepaceSettings>,
    mut cams: Query<(Entity, &mut Camera, &mut Msaa, Option<&Bloom>)>,
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
    let key = format!("{limiter:?}");
    if last_limit.as_deref() != Some(key.as_str()) {
        pace.limiter = limiter;
        *last_limit = Some(key);
    }
    if !cfg.is_changed() && !theme.is_changed() {
        return;
    }
    for (e, mut cam, mut msaa, bloom) in &mut cams {
        cam.clear_color = ClearColorConfig::Custom(theme::bevy_color(theme.pal.canvas));
        let want = if cfg.scene.msaa {
            Msaa::Sample4
        } else {
            Msaa::Off
        };
        if *msaa != want {
            *msaa = want;
        }
        match (cfg.scene.bloom, bloom.is_some()) {
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
