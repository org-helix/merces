//! The network: the active symbol is the input neuron, the four pipeline agents
//! follow it in a chain, and the assistant floats above, faintly linked to them.
//!
//! Edges follow the real data flow: market data feeds research and charts; the
//! research brief reaches the chart and decision agents; the chart view reaches
//! the decision and execution agents; the decision reaches execution. The chain
//! (data → research → chart → decision → execution) is drawn stronger: it is the
//! path the hand-offs travel on.
//!
//! Nodes are held by springs (towards their rest place, along the edges, apart
//! from each other) and drift slowly, so the graph breathes like a live network.
//! A node can be dragged; released, it springs back with its momentum. The
//! simulation runs in fixed sub-steps: the same motion at any frame rate.

// The nodes live in parallel fixed arrays (positions, velocities, forces).
#![allow(clippy::needless_range_loop)]

use std::f32::consts::TAU;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use merces_core::{AgentId, AgentStatus};

use super::{Layout, Selection};
use crate::config::AppConfig;
use crate::state::Hub;
use crate::theme::{self, Theme};

/// Index of the input (symbol) node; 0..5 are the agents in `AgentId` order.
pub const INPUT: usize = 5;
const NODES: usize = 6;

/// Rest positions: the chain left to right with some depth, the assistant above.
const HOME: [Vec3; NODES] = [
    Vec3::new(-6.0, 1.8, 1.0),   // research
    Vec3::new(-1.0, -1.6, -0.6), // chart
    Vec3::new(4.5, 1.4, 0.6),    // decision
    Vec3::new(10.0, -0.4, -0.4), // execution
    Vec3::new(-1.0, 6.2, -4.0),  // assistant
    Vec3::new(-12.0, 0.0, 0.0),  // input
];

/// Centre of the graph, looked at in the overview.
pub const CENTER: Vec3 = Vec3::new(-0.8, 1.2, 0.0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeKind {
    /// The hand-off chain.
    Chain,
    /// Other data the agent receives.
    Data,
    /// The assistant reads the agents' work.
    Assistant,
}

pub const EDGES: [(usize, usize, EdgeKind); 11] = [
    (INPUT, 0, EdgeKind::Chain),
    (0, 1, EdgeKind::Chain),
    (1, 2, EdgeKind::Chain),
    (2, 3, EdgeKind::Chain),
    (INPUT, 1, EdgeKind::Data),
    (0, 2, EdgeKind::Data),
    (1, 3, EdgeKind::Data),
    (4, 0, EdgeKind::Assistant),
    (4, 1, EdgeKind::Assistant),
    (4, 2, EdgeKind::Assistant),
    (4, 3, EdgeKind::Assistant),
];

const SUBSTEP: f32 = 1.0 / 120.0;
const K_HOME: f32 = 5.0;
const K_EDGE: f32 = 3.0;
const K_REPEL: f32 = 18.0;
const MIN_GAP: f32 = 2.6;
const DAMPING: f32 = 4.5;
const DRIFT: f32 = 0.35;
const INPUT_RADIUS: f32 = 0.55;
/// Pixel size of the input node.
const INPUT_PX: f32 = 0.1;

/// A ball of pixels (radius 5.5 pixels), centred on the origin.
fn pixel_ball() -> super::voxel::Model {
    let mut m = super::voxel::Model::new();
    for x in -6..6 {
        for y in -6..6 {
            for z in -6..6 {
                let c = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                if c.length() < 5.6 {
                    // Lighter towards the top: a pixel sphere with volume.
                    let k = 0.82 + 0.24 * (c.y / 5.6).max(-1.0);
                    m.set(IVec3::new(x, y, z), [k, k, k]);
                }
            }
        }
    }
    m
}

/// A flat ring of pixels around the ball (radius about 8.5 pixels).
fn pixel_ring() -> super::voxel::Model {
    let mut m = super::voxel::Model::new();
    for x in -10..10 {
        for z in -10..10 {
            let d = Vec2::new(x as f32 + 0.5, z as f32 + 0.5).length();
            if (7.8..9.0).contains(&d) {
                m.set(IVec3::new(x, 0, z), super::voxel::TINT);
            }
        }
    }
    m
}

/// Positions and velocities of the nodes, and the node being dragged.
#[derive(Resource)]
pub struct Net {
    pub pos: [Vec3; NODES],
    vel: [Vec3; NODES],
    /// Dragged node and where the pointer holds it.
    grab: Option<(usize, Vec3)>,
    t: f32,
    acc: f32,
}

impl Default for Net {
    fn default() -> Self {
        Self {
            pos: HOME,
            vel: [Vec3::ZERO; NODES],
            grab: None,
            t: 0.0,
            acc: 0.0,
        }
    }
}

impl Net {
    /// Slow, smooth wander around the rest place (sum of incommensurate sines).
    fn drift(i: usize, t: f32) -> Vec3 {
        let p = i as f32 * 1.7;
        Vec3::new(
            (t * 0.21 + p).sin() + 0.5 * (t * 0.37 + p * 2.1).sin(),
            (t * 0.17 + p * 1.3).sin() + 0.4 * (t * 0.29 + p).sin(),
            (t * 0.13 + p * 0.7).sin(),
        ) * (DRIFT / 1.5)
    }

    /// One fixed simulation step.
    fn step(&mut self, dt: f32, calm: bool) {
        if !calm {
            self.t += dt;
        }
        let mut force = [Vec3::ZERO; NODES];
        for i in 0..NODES {
            let rest = if calm {
                HOME[i]
            } else {
                HOME[i] + Self::drift(i, self.t)
            };
            force[i] += (rest - self.pos[i]) * K_HOME;
        }
        for (a, b, kind) in EDGES {
            let k = match kind {
                EdgeKind::Chain => K_EDGE,
                EdgeKind::Data => K_EDGE * 0.5,
                EdgeKind::Assistant => K_EDGE * 0.15,
            };
            let d = self.pos[b] - self.pos[a];
            let len = d.length().max(1e-4);
            let rest = HOME[a].distance(HOME[b]);
            let f = d / len * (len - rest) * k;
            force[a] += f;
            force[b] -= f;
        }
        for a in 0..NODES {
            for b in a + 1..NODES {
                let d = self.pos[b] - self.pos[a];
                let len = d.length().max(1e-4);
                if len < MIN_GAP {
                    let f = d / len * (MIN_GAP - len) * K_REPEL;
                    force[a] -= f;
                    force[b] += f;
                }
            }
        }
        let decay = (-DAMPING * dt).exp();
        for i in 0..NODES {
            if let Some((g, at)) = self.grab
                && g == i
            {
                // Follow the pointer; the velocity is kept for the release.
                let v = (at - self.pos[i]) / dt;
                self.vel[i] = v.clamp_length_max(40.0);
                self.pos[i] = at;
                continue;
            }
            self.vel[i] = (self.vel[i] + force[i] * dt) * decay;
            self.pos[i] += self.vel[i] * dt;
        }
    }
}

/// Point of the edge from `a` to `b`: a straight line, shared by the drawn
/// edges, the pulses and the hand-offs so they all travel on the same path.
pub fn edge_point(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a.lerp(b, t)
}

pub struct GraphPlugin;

impl Plugin for GraphPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Net>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (simulate, (input_look, edges, pulses))
                    .chain()
                    .in_set(GraphSet),
            );
    }
}

/// Runs before the camera and the animations (they read [`Layout`]).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphSet;

#[derive(Component)]
struct InputNode(Handle<StandardMaterial>);

#[derive(Component)]
struct InputRing;

#[derive(Component)]
struct Pulse {
    edge: usize,
    seed: f32,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut gizmo_cfg: ResMut<GizmoConfigStore>,
    theme: Res<Theme>,
) {
    let (g, _) = gizmo_cfg.config_mut::<DefaultGizmoConfigGroup>();
    g.line.width = 1.6;

    // The input neuron: the active symbol's market data.
    let accent = theme::bevy_color(theme.pal.accent);
    let input_mat = mats.add(StandardMaterial {
        base_color: accent,
        emissive: accent.to_linear() * 2.5,
        ..default()
    });
    let ring_mat = mats.add(StandardMaterial {
        base_color: theme::bevy_color(theme::PALE_ORANGE),
        emissive: theme::bevy_color(theme::PALE_ORANGE).to_linear() * 1.5,
        unlit: true,
        ..default()
    });
    commands
        .spawn((
            Mesh3d(meshes.add(pixel_ball().mesh(INPUT_PX, Vec3::splat(0.0)))),
            MeshMaterial3d(input_mat.clone()),
            Transform::from_translation(HOME[INPUT]),
            InputNode(input_mat),
            Name::new("input"),
        ))
        .with_children(|p| {
            p.spawn((
                Mesh3d(meshes.add(pixel_ring().mesh(INPUT_PX, Vec3::new(0.0, 0.5, 0.0)))),
                MeshMaterial3d(ring_mat),
                Transform::from_rotation(Quat::from_rotation_x(0.45)),
                InputRing,
                Pickable::IGNORE,
            ));
        })
        .observe(|_: On<PointerOver>, mut sel: ResMut<Selection>| sel.input_hovered = true)
        .observe(|_: On<PointerOut>, mut sel: ResMut<Selection>| sel.input_hovered = false)
        .observe(drag_start(INPUT))
        .observe(drag_move(INPUT))
        .observe(drag_end);

    // Signals travelling on the data edges (not the assistant's).
    let pulse_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let pulse_mat = mats.add(StandardMaterial {
        base_color: theme::bevy_color(theme::PALE_ORANGE),
        emissive: theme::bevy_color(theme::PALE_ORANGE).to_linear() * 5.0,
        unlit: true,
        ..default()
    });
    for (edge, (_, _, kind)) in EDGES.iter().enumerate() {
        if *kind == EdgeKind::Assistant {
            continue;
        }
        for k in 0..2 {
            commands.spawn((
                Mesh3d(pulse_mesh.clone()),
                MeshMaterial3d(pulse_mat.clone()),
                Transform::from_scale(Vec3::ZERO),
                Pulse {
                    edge,
                    seed: k as f32 * 0.5 + edge as f32 * 0.13,
                },
                Pickable::IGNORE,
            ));
        }
    }

    // Light: a warm key light from the front and above, a low ambient fill.
    commands.spawn((
        DirectionalLight {
            illuminance: 4_500.0,
            color: Color::srgb(1.0, 0.92, 0.82),
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(4.0, 9.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: theme::bevy_color(theme::AMBER),
        brightness: 220.0,
        ..default()
    });
}

/// Advance the springs and publish the node positions.
fn simulate(
    time: Res<Time>,
    cfg: Res<AppConfig>,
    mut net: ResMut<Net>,
    mut lay: ResMut<Layout>,
    mut chars: Query<(&super::agents::Character, &mut Transform), Without<InputNode>>,
    mut input: Query<&mut Transform, With<InputNode>>,
) {
    let calm = cfg.scene.reduced_motion;
    net.acc += time.delta_secs().min(0.1);
    while net.acc >= SUBSTEP {
        net.acc -= SUBSTEP;
        net.step(SUBSTEP, calm);
    }
    let t = net.t;
    for (c, mut tf) in &mut chars {
        let i = c.agent.index();
        tf.translation = net.pos[i];
        if tf.scale.x != c.scale {
            tf.scale = Vec3::splat(c.scale);
        }
        lay.agents[i] = net.pos[i];
        lay.sizes[i] = 0.8 * c.scale;
        // Characters face the viewer, with a slow, slight sway.
        lay.spins[i] = if calm {
            0.0
        } else {
            0.35 * (t * 0.23 + i as f32 * 1.9).sin()
        };
    }
    lay.input = net.pos[INPUT];
    lay.input_size = INPUT_RADIUS;
    if let Ok(mut tf) = input.single_mut() {
        tf.translation = net.pos[INPUT];
    }
}

/// The input neuron glows brighter while its symbol is analysed. The material
/// is only written while the level ramps.
fn input_look(
    time: Res<Time>,
    hub: Res<Hub>,
    cfg: Res<AppConfig>,
    theme: Res<Theme>,
    node: Query<&InputNode>,
    mut ring: Query<&mut Transform, With<InputRing>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut level: Local<Option<(f32, [u8; 3])>>,
) {
    if !cfg.scene.reduced_motion
        && let Ok(mut tf) = ring.single_mut()
    {
        tf.rotation =
            Quat::from_rotation_x(0.45) * Quat::from_rotation_y(time.elapsed_secs() * 0.4);
    }
    let busy = hub
        .symbol(&cfg.active_symbol)
        .is_some_and(|v| v.running.is_some());
    let goal = if busy { 1.0 } else { 0.0 };
    let (mut lv, accent) = level.unwrap_or((-1.0, [0; 3]));
    if (lv - goal).abs() < 1e-3 && accent == cfg.appearance.accent {
        return;
    }
    lv = if lv < 0.0 {
        goal
    } else {
        lv + (goal - lv) * (1.0 - (-2.0 * time.delta_secs()).exp())
    };
    if (lv - goal).abs() < 2e-3 {
        lv = goal;
    }
    *level = Some((lv, cfg.appearance.accent));
    let Ok(n) = node.single() else { return };
    if let Some(mut m) = mats.get_mut(&n.0) {
        let c = theme::bevy_color(theme.pal.accent);
        m.base_color = c;
        m.emissive = c.to_linear() * (2.5 + 4.0 * lv);
    }
}

fn node_pos(lay: &Layout, i: usize) -> Vec3 {
    if i == INPUT { lay.input } else { lay.agents[i] }
}

/// Is the source of this edge producing right now?
fn edge_active(hub: &Hub, cfg: &AppConfig, from: usize) -> bool {
    let Some(v) = hub.symbol(&cfg.active_symbol) else {
        return false;
    };
    if from == INPUT {
        return v.running.is_some();
    }
    AgentId::PIPELINE
        .get(from)
        .is_some_and(|a| v.agent(*a).status == AgentStatus::Working)
}

/// The synapses. Few, short curves: drawn as gizmos every frame.
fn edges(
    mut gizmos: Gizmos,
    lay: Res<Layout>,
    hub: Res<Hub>,
    cfg: Res<AppConfig>,
    sel: Res<Selection>,
) {
    let wire = theme::bevy_color(theme::AMBER);
    let hot = theme::bevy_color(theme::PALE_ORANGE);
    let focus = sel.agent.or(sel.hovered).map(AgentId::index);
    for (a, b, kind) in EDGES {
        let (pa, pb) = (node_pos(&lay, a), node_pos(&lay, b));
        let lit = focus.is_some_and(|f| f == a || f == b);
        let alpha = match kind {
            EdgeKind::Chain => 0.30,
            EdgeKind::Data => 0.14,
            EdgeKind::Assistant => 0.10,
        } * if lit { 2.0 } else { 1.0 };
        let color = if kind != EdgeKind::Assistant && edge_active(&hub, &cfg, a) {
            hot.with_alpha((alpha * 1.8_f32).min(0.8))
        } else {
            wire.with_alpha(alpha)
        };
        const SEGMENTS: usize = 28;
        if kind == EdgeKind::Assistant {
            // Dotted: the assistant only reads.
            for k in (0..SEGMENTS).step_by(2) {
                let (u, v) = (k as f32 / SEGMENTS as f32, (k + 1) as f32 / SEGMENTS as f32);
                gizmos.line(edge_point(pa, pb, u), edge_point(pa, pb, v), color);
            }
        } else {
            gizmos.line(pa, pb, color);
        }
    }
}

/// Signals flowing along the data edges: slow at rest, fast and brighter while
/// the source works. Hidden under reduced motion.
fn pulses(
    time: Res<Time>,
    lay: Res<Layout>,
    hub: Res<Hub>,
    cfg: Res<AppConfig>,
    mut q: Query<(&Pulse, &mut Transform, &mut Visibility)>,
    mut phase: Local<[f32; EDGES.len()]>,
) {
    let calm = cfg.scene.reduced_motion;
    let dt = time.delta_secs().min(0.1);
    let mut active = [false; EDGES.len()];
    for (e, (a, _, _)) in EDGES.iter().enumerate() {
        active[e] = edge_active(&hub, &cfg, *a);
        phase[e] = (phase[e] + dt * if active[e] { 0.55 } else { 0.1 }).fract();
    }
    for (p, mut tf, mut vis) in &mut q {
        let want = if calm {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *vis != want {
            *vis = want;
        }
        if calm {
            continue;
        }
        let (a, b, kind) = EDGES[p.edge];
        let u = (phase[p.edge] + p.seed).fract();
        tf.translation = edge_point(node_pos(&lay, a), node_pos(&lay, b), u);
        let size = match (active[p.edge], kind) {
            (true, _) => 0.09,
            (false, EdgeKind::Chain) => 0.05,
            _ => 0.035,
        };
        // Fade in and out at the ends.
        tf.scale = Vec3::splat(size * (u * TAU * 0.5).sin().max(0.0).sqrt());
    }
}

// ── Dragging nodes ──────────────────────────────────────────────────────────

/// Start dragging `node` (agents and the input neuron).
pub fn drag_start(node: usize) -> impl Fn(On<PointerDragStart>, ResMut<Net>, ResMut<Selection>) {
    move |ev, mut net, mut sel| {
        if ev.button == PointerButton::Primary {
            net.grab = Some((node, net.pos[node]));
            sel.dragging = true;
        }
    }
}

/// Move the dragged node in the screen plane, under the pointer.
type CameraQuery<'w, 's> =
    Query<'w, 's, (&'static GlobalTransform, &'static Projection), With<Camera3d>>;
type WindowQuery<'w, 's> = Query<'w, 's, &'static Window, With<PrimaryWindow>>;

pub fn drag_move(
    node: usize,
) -> impl Fn(On<PointerDrag>, ResMut<Net>, ResMut<Selection>, CameraQuery, WindowQuery) {
    move |ev, mut net, mut sel, cams, windows| {
        if ev.button != PointerButton::Primary {
            return;
        }
        sel.drag_px = sel.drag_px.max(ev.distance.length());
        let (Ok((cam, proj)), Ok(win)) = (cams.single(), windows.single()) else {
            return;
        };
        let Projection::Perspective(p) = proj else {
            return;
        };
        let Some((g, at)) = net.grab else { return };
        if g != node {
            return;
        }
        let depth = (at - cam.translation())
            .dot(cam.forward().as_vec3())
            .max(0.5);
        let per_px = 2.0 * depth * (p.fov * 0.5).tan() / win.height().max(1.0);
        let at =
            at + (cam.right().as_vec3() * ev.delta.x - cam.up().as_vec3() * ev.delta.y) * per_px;
        net.grab = Some((node, at));
    }
}

pub fn drag_end(_: On<PointerDragEnd>, mut net: ResMut<Net>, mut sel: ResMut<Selection>) {
    net.grab = None;
    sel.dragging = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settles_near_home_and_apart() {
        let mut n = Net::default();
        for _ in 0..(120 * 20) {
            n.step(SUBSTEP, false);
        }
        for i in 0..NODES {
            assert!(
                n.pos[i].distance(HOME[i]) < 1.5,
                "node {i} drifted to {:?}",
                n.pos[i]
            );
            for j in i + 1..NODES {
                assert!(
                    n.pos[i].distance(n.pos[j]) > 2.0,
                    "nodes {i} and {j} too close"
                );
            }
        }
    }

    #[test]
    fn released_node_springs_back() {
        let mut n = Net {
            grab: Some((2, HOME[2] + Vec3::new(0.0, 6.0, 0.0))),
            ..Default::default()
        };
        for _ in 0..30 {
            n.step(SUBSTEP, true);
        }
        assert!(n.pos[2].distance(HOME[2]) > 5.0);
        n.grab = None;
        for _ in 0..(120 * 6) {
            n.step(SUBSTEP, true);
        }
        assert!(n.pos[2].distance(HOME[2]) < 0.2, "{:?}", n.pos[2]);
    }

    #[test]
    fn calm_graph_is_still() {
        let mut n = Net::default();
        for _ in 0..240 {
            n.step(SUBSTEP, true);
        }
        let before = n.pos;
        n.step(SUBSTEP, true);
        for i in 0..NODES {
            assert!(n.pos[i].distance(before[i]) < 1e-4);
        }
    }

    #[test]
    fn every_agent_is_connected() {
        for i in 0..NODES {
            assert!(
                EDGES.iter().any(|(a, b, _)| *a == i || *b == i),
                "node {i} is isolated"
            );
        }
    }

    /// The drag observers move the grabbed node under the pointer, in the
    /// screen plane, and let it go on release.
    #[test]
    fn dragging_moves_the_node_with_the_pointer() {
        use bevy::camera::NormalizedRenderTarget;
        use bevy::picking::backend::HitData;
        use bevy::picking::events::Pointer;
        use bevy::picking::pointer::{Location, PointerId};
        use bevy::window::WindowRef;

        let mut app = App::new();
        app.init_resource::<Net>().init_resource::<Selection>();
        let world = app.world_mut();
        let win = world.spawn((Window::default(), PrimaryWindow)).id();
        let cam = world
            .spawn((
                Camera3d::default(),
                Projection::Perspective(PerspectiveProjection::default()),
                GlobalTransform::from(
                    Transform::from_xyz(0.0, 0.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
                ),
            ))
            .id();
        let node = world
            .spawn_empty()
            .observe(drag_start(2))
            .observe(drag_move(2))
            .observe(drag_end)
            .id();
        let window = WindowRef::Primary.normalize(Some(win)).expect("window");
        let pointer = || {
            Pointer::new(
                PointerId::Mouse,
                Location {
                    target: NormalizedRenderTarget::Window(window),
                    position: Vec2::ZERO,
                },
            )
        };
        let hit = HitData::new(cam, 0.0, None, None);
        let start = world.resource::<Net>().pos[2];
        world.trigger(PointerDragStart {
            entity: node,
            pointer: pointer(),
            button: PointerButton::Primary,
            hit,
        });
        assert!(world.resource::<Selection>().dragging);
        world.trigger(PointerDrag {
            entity: node,
            pointer: pointer(),
            button: PointerButton::Primary,
            distance: Vec2::new(0.0, -100.0),
            delta: Vec2::new(0.0, -100.0),
        });
        let grab = world.resource::<Net>().grab.expect("grabbed").1;
        assert!(
            grab.y > start.y + 1.0,
            "dragging up moves the node up: {grab:?}"
        );
        assert!((grab.x - start.x).abs() < 1e-3);
        assert!(world.resource::<Selection>().drag_px >= 100.0);
        world.trigger(PointerDragEnd {
            entity: node,
            pointer: pointer(),
            button: PointerButton::Primary,
            distance: Vec2::new(0.0, -100.0),
        });
        assert!(world.resource::<Net>().grab.is_none());
        assert!(!world.resource::<Selection>().dragging);
    }
}
