//! The network: the active symbol is the input neuron, the four pipeline agents
//! follow it in a chain, and the assistant floats above, faintly linked to them.
//!
//! Edges follow the real data flow: market data feeds research and charts; the
//! research brief reaches the chart and decision agents; the chart view reaches
//! the decision and execution agents; the decision reaches execution. The chain
//! (data → research → chart → decision → execution) is drawn stronger: it is the
//! path the hand-offs travel on.
//!
//! The layout is a 3D force simulation: every link is a spring with the same
//! rest length, every pair of nodes repels, and an attraction pulls everything
//! towards the centre. Linked nodes therefore sit at the same distance along
//! the three axes. The link length and the attraction are set in Settings →
//! Advanced. Nodes drift slowly, so the graph breathes like a live network; a
//! node can be dragged and the network rearranges around it. The simulation
//! runs in fixed sub-steps: the same motion at any frame rate.

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

/// Centre of the graph: the attraction pulls towards it, the overview looks at it.
pub const CENTER: Vec3 = Vec3::new(0.0, 0.8, 0.0);

/// The ideal layout, in link lengths, centred on the origin: every link has
/// exactly unit length. Research, chart and decision form a triangle; the
/// assistant and execution each sit on one side of it (two regular
/// tetrahedra sharing a face); the input node hangs off research and chart,
/// as far as possible from the others. Rotated so the flow reads left to
/// right (input → execution).
pub fn ideal() -> [Vec3; NODES] {
    static IDEAL: std::sync::OnceLock<[Vec3; NODES]> = std::sync::OnceLock::new();
    *IDEAL.get_or_init(compute_ideal)
}

fn compute_ideal() -> [Vec3; NODES] {
    let h = (2.0f32 / 3.0).sqrt();
    let r = Vec3::ZERO;
    let c = Vec3::X;
    let d = Vec3::new(0.5, 3f32.sqrt() / 2.0, 0.0);
    let a = Vec3::new(0.5, 3f32.sqrt() / 6.0, h);
    // Execution: research mirrored through the chart–decision–assistant face.
    let e = (c + d + a) * (2.0 / 3.0) - r;
    // Input: on the unit circle around the research–chart axis, in the middle
    // of the arc that stays at least one link away from decision and assistant.
    let rad = 3f32.sqrt() / 2.0;
    let ang = 215f32.to_radians();
    let i = Vec3::new(0.5, rad * ang.cos(), rad * ang.sin());
    let mut p = [r, c, d, e, a, i];
    let mid = p.iter().copied().sum::<Vec3>() / NODES as f32;
    let turn = Quat::from_rotation_arc((e - i).normalize(), Vec3::X);
    for q in &mut p {
        *q = turn * (*q - mid);
    }
    // Roll around the reading axis so the assistant floats on top.
    let up = p[4];
    let roll = Quat::from_rotation_x(-up.z.atan2(up.y));
    for q in &mut p {
        *q = roll * *q;
    }
    // Turn around the vertical (within ±40°, keeping the reading direction)
    // to the angle where the nodes are best apart on screen, seen from the
    // default camera (front, slightly above).
    let (sp, cp) = super::camera::DEFAULT_PITCH.sin_cos();
    let view = |q: Vec3| Vec2::new(q.x, q.y * cp - q.z * sp);
    let spread = |yaw: f32| {
        let t = Quat::from_rotation_y(yaw);
        let s: Vec<Vec2> = p.iter().map(|q| view(t * *q)).collect();
        let mut min = f32::MAX;
        for a in 0..NODES {
            for b in a + 1..NODES {
                min = min.min(s[a].distance(s[b]));
            }
        }
        min
    };
    let best = (-50..=50)
        .map(|d| (d as f32).to_radians())
        .max_by(|a, b| spread(*a).total_cmp(&spread(*b)))
        .unwrap_or(0.0);
    let t = Quat::from_rotation_y(best);
    p.map(|q| t * q)
}

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
/// Stiffness of a link (every link has the same rest length).
const K_LINK: f32 = 6.0;
/// Stiffness of the push between nodes closer than [`GAP`] link lengths,
/// relative to a link.
const K_REPEL: f32 = 1.0;
/// Closest two nodes may come, in link lengths.
const GAP: f32 = 1.0;
/// Strength of the attraction towards the centre, at attraction 1.
const K_CENTER: f32 = 0.5;
/// Weak pull that keeps the reading direction (x only).
const K_RANK: f32 = 0.1;
const DAMPING: f32 = 2.5;
/// Amplitude of the slow wander, as a force.
const DRIFT: f32 = 0.5;

/// The layout forces the user can tune (Settings → Advanced).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Forces {
    /// Pull of every node towards the centre (0 = none, 1 = default).
    pub attraction: f32,
    /// Rest length of every link: the spacing between linked nodes, the same
    /// along the three axes.
    pub distance: f32,
}

impl Default for Forces {
    fn default() -> Self {
        Self {
            attraction: 1.0,
            distance: 5.5,
        }
    }
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
        let l = Forces::default().distance;
        let pos = ideal().map(|p| CENTER + p * l);
        Self {
            pos,
            vel: [Vec3::ZERO; NODES],
            grab: None,
            t: 0.0,
            acc: 0.0,
        }
    }
}

impl Net {
    /// Slow, smooth wander (sum of incommensurate sines), as a small force.
    fn drift(i: usize, t: f32) -> Vec3 {
        let p = i as f32 * 1.7;
        Vec3::new(
            (t * 0.21 + p).sin() + 0.5 * (t * 0.37 + p * 2.1).sin(),
            (t * 0.17 + p * 1.3).sin() + 0.4 * (t * 0.29 + p).sin(),
            (t * 0.13 + p * 0.7).sin() + 0.5 * (t * 0.31 + p * 1.1).sin(),
        ) * (DRIFT / 1.5)
    }

    /// One fixed simulation step of the force layout.
    fn step(&mut self, dt: f32, calm: bool, f: Forces) {
        if !calm {
            self.t += dt;
        }
        let l = f.distance.max(1.0);
        let rest = ideal();
        let mut force = [Vec3::ZERO; NODES];
        // Links: springs, all with the same rest length.
        for (a, b, kind) in EDGES {
            let k = if kind == EdgeKind::Assistant {
                K_LINK * 0.8
            } else {
                K_LINK
            };
            let d = self.pos[b] - self.pos[a];
            let len = d.length().max(1e-4);
            let pull = d / len * (len - l) * k;
            force[a] += pull;
            force[b] -= pull;
        }
        // Every pair closer than a link length is pushed apart: no node comes
        // nearer than the link distance, whether linked or not.
        for a in 0..NODES {
            for b in a + 1..NODES {
                let d = self.pos[b] - self.pos[a];
                let len = d.length().max(1e-3);
                let gap = l * GAP - len;
                if gap > 0.0 {
                    let push = d / len * gap * K_REPEL * K_LINK;
                    force[a] -= push;
                    force[b] += push;
                }
            }
        }
        for i in 0..NODES {
            // Attraction towards the centre.
            force[i] += (CENTER - self.pos[i]) * K_CENTER * f.attraction.max(0.0);
            // Keep the reading direction (x only).
            force[i].x += (CENTER.x + rest[i].x * l - self.pos[i].x) * K_RANK;
            if !calm {
                force[i] += Self::drift(i, self.t);
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
            Transform::from_translation(Net::default().pos[INPUT]),
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
    let forces = Forces {
        attraction: cfg.scene.attraction.clamp(0.0, 3.0),
        distance: cfg.scene.link_distance.clamp(3.0, 12.0),
    };
    net.acc += time.delta_secs().min(0.1);
    while net.acc >= SUBSTEP {
        net.acc -= SUBSTEP;
        net.step(SUBSTEP, calm, forces);
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

    fn settle(n: &mut Net, f: Forces, seconds: f32, calm: bool) {
        for _ in 0..(seconds / SUBSTEP) as usize {
            n.step(SUBSTEP, calm, f);
        }
    }

    fn edge_lengths(n: &Net) -> Vec<f32> {
        EDGES
            .iter()
            .map(|(a, b, _)| n.pos[*a].distance(n.pos[*b]))
            .collect()
    }

    fn mean_to_center(n: &Net) -> f32 {
        n.pos.iter().map(|p| p.distance(CENTER)).sum::<f32>() / NODES as f32
    }

    #[test]
    fn linked_nodes_sit_at_the_link_distance() {
        let f = Forces::default();
        let mut n = Net::default();
        settle(&mut n, f, 30.0, true);
        let l = f.distance;
        for (len, (a, b, _)) in edge_lengths(&n).iter().zip(EDGES) {
            assert!(
                (len / l - 1.0).abs() < 0.3,
                "link {a}-{b} is {len:.2}, expected about {l}"
            );
        }
        for a in 0..NODES {
            for b in a + 1..NODES {
                let d = n.pos[a].distance(n.pos[b]);
                assert!(d > 0.9 * l, "nodes {a} and {b} too close: {d:.2}");
            }
        }
    }

    #[test]
    fn spreads_evenly_in_three_dimensions() {
        let f = Forces::default();
        let mut n = Net::default();
        settle(&mut n, f, 30.0, true);
        let min = n.pos.iter().fold(Vec3::INFINITY, |m, p| m.min(*p));
        let max = n.pos.iter().fold(Vec3::NEG_INFINITY, |m, p| m.max(*p));
        let extent = max - min;
        for (axis, e) in ["x", "y", "z"].iter().zip(extent.to_array()) {
            assert!(e > 0.7 * f.distance, "flat along {axis}: {extent:?}");
        }
        assert!(
            extent.max_element() / extent.min_element() < 2.5,
            "uneven spread: {extent:?}"
        );
    }

    #[test]
    fn attraction_and_distance_are_tunable() {
        let mut loose = Net::default();
        settle(
            &mut loose,
            Forces {
                attraction: 0.0,
                ..Default::default()
            },
            30.0,
            true,
        );
        let mut tight = Net::default();
        settle(
            &mut tight,
            Forces {
                attraction: 3.0,
                ..Default::default()
            },
            30.0,
            true,
        );
        assert!(mean_to_center(&tight) < mean_to_center(&loose) * 0.95);

        let f = Forces {
            distance: 9.0,
            ..Default::default()
        };
        let mut wide = Net::default();
        settle(&mut wide, f, 40.0, true);
        let lens = edge_lengths(&wide);
        let mean = lens.iter().sum::<f32>() / lens.len() as f32;
        assert!((mean / 9.0 - 1.0).abs() < 0.1, "mean link {mean:.2}");
    }

    #[test]
    fn released_node_rejoins_the_network() {
        let f = Forces::default();
        let mut n = Net::default();
        settle(&mut n, f, 20.0, true);
        let far = n.pos[2] + Vec3::new(0.0, 12.0, 0.0);
        n.grab = Some((2, far));
        settle(&mut n, f, 0.5, true);
        assert!(
            n.pos[2].distance(far) < 1e-3,
            "the grabbed node follows the pointer"
        );
        n.grab = None;
        settle(&mut n, f, 20.0, true);
        for len in edge_lengths(&n) {
            assert!((len / f.distance - 1.0).abs() < 0.3, "{len:.2}");
        }
    }

    #[test]
    fn calm_graph_is_still() {
        let f = Forces::default();
        let mut n = Net::default();
        settle(&mut n, f, 90.0, true);
        let before = n.pos;
        n.step(SUBSTEP, true, f);
        for i in 0..NODES {
            assert!(n.pos[i].distance(before[i]) < 1e-3);
        }
    }

    #[test]
    fn assistant_floats_on_top_and_flow_reads_left_to_right() {
        let p = ideal();
        let top = p.iter().map(|q| q.y).fold(f32::MIN, f32::max);
        assert_eq!(p[4].y, top, "the assistant is the highest node");
        // Seen from the default camera, no two nodes overlap.
        for a in 0..NODES {
            for b in a + 1..NODES {
                let (sp, cp) = super::super::camera::DEFAULT_PITCH.sin_cos();
                let at = |q: Vec3| Vec2::new(q.x, q.y * cp - q.z * sp);
                let d = at(p[a]).distance(at(p[b]));
                assert!(d > 0.45, "nodes {a} and {b} overlap on screen: {d:.2}");
            }
        }
        assert!(
            p[INPUT].x < p[0].x && p[0].x < p[3].x,
            "input → research → execution"
        );
        for (a, b, _) in EDGES {
            assert!((p[a].distance(p[b]) - 1.0).abs() < 1e-4, "link {a}-{b}");
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
