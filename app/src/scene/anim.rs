//! Procedural animation of the characters.
//!
//! Each status (`AgentStatus`) has a pose: a small set of numbers (lift,
//! squash, lean, arm angles, gaze…) computed from the time. The current pose
//! is a weighted blend of the status poses; weights move towards the current
//! status with an exponential fade, so every change is a cross-fade. Short
//! reactions (wave, throw, catch, cheer, slump, nod, head shake) are added on
//! top with a smooth envelope.
//!
//! Hand-offs (`EngineEvent::Message`): the sender throws a small envelope that
//! travels between the two moving bodies and the receiver catches it.
//!
//! Reduced motion freezes the oscillations, skips the reactions and the work
//! sparks, and keeps only the static pose of each status and the hand-off.
//! Nothing here allocates per frame: fixed arrays and queries only.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;
use merces_core::{AgentId, AgentStatus, RunOutcome};

use super::agents::{Character, Part, Rest};
use super::{Layout, Selection};
use crate::config::AppConfig;
use crate::state::Hub;
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reaction {
    /// Greets before the camera flies in (click).
    Wave,
    /// Sends its work to the next agent.
    Throw,
    /// Receives work.
    Catch,
    /// A trade plan was emitted.
    Cheer,
    /// The plan was rejected by the guard-rails.
    Slump,
    /// Short satisfaction when done.
    Nod,
    /// "Nothing today."
    Shake,
}

impl Reaction {
    fn duration(self) -> f32 {
        match self {
            Reaction::Wave => 1.0,
            Reaction::Throw => 0.6,
            Reaction::Catch => 0.7,
            Reaction::Cheer => 1.6,
            Reaction::Slump => 2.4,
            Reaction::Nod => 1.0,
            Reaction::Shake => 1.3,
        }
    }
}

/// Animation state of one character.
#[derive(Component)]
pub struct Anim {
    /// Blend weights of the status poses, indexed like `status_slot`.
    weights: [f32; 5],
    status: AgentStatus,
    reaction: Option<(Reaction, f32)>,
    /// 0..1: how much the character turns towards the camera.
    face: f32,
    phase: f32,
}

impl Anim {
    pub fn new(agent: AgentId) -> Self {
        let mut weights = [0.0; 5];
        weights[0] = 1.0;
        Self {
            weights,
            status: AgentStatus::Idle,
            reaction: None,
            face: 0.0,
            phase: agent.index() as f32 * 1.37,
        }
    }

    pub fn react(&mut self, r: Reaction, now: f32) {
        self.reaction = Some((r, now));
    }
}

fn status_slot(s: AgentStatus) -> usize {
    match s {
        AgentStatus::Idle => 0,
        AgentStatus::Working => 1,
        AgentStatus::Done => 2,
        AgentStatus::NoTrade => 3,
        AgentStatus::Error => 4,
    }
}

/// Every animated quantity. Arms: x = forward swing, y = outward swing (radians).
#[derive(Default, Clone, Copy)]
struct Pose {
    lift: f32,
    squash: f32,
    lean: f32,
    roll: f32,
    turn: f32,
    arm_l: Vec2,
    arm_r: Vec2,
    look: Vec2,
    blink: f32,
    hat_tilt: f32,
    hat_lift: f32,
    bubble: f32,
}

impl Pose {
    fn add(&mut self, o: &Pose, w: f32) {
        self.lift += o.lift * w;
        self.squash += o.squash * w;
        self.lean += o.lean * w;
        self.roll += o.roll * w;
        self.turn += o.turn * w;
        self.arm_l += o.arm_l * w;
        self.arm_r += o.arm_r * w;
        self.look += o.look * w;
        self.blink += o.blink * w;
        self.hat_tilt += o.hat_tilt * w;
        self.hat_lift += o.hat_lift * w;
        self.bubble += o.bubble * w;
    }
}

/// Smooth bump: 0 outside [a, b], 1 in the middle.
fn bump(u: f32, a: f32, b: f32) -> f32 {
    if u <= a || u >= b {
        return 0.0;
    }
    (PI * (u - a) / (b - a)).sin()
}

/// A short gesture every `period` seconds, lasting `len`.
fn every(t: f32, period: f32, len: f32) -> f32 {
    bump(t.rem_euclid(period), 0.0, len)
}

fn status_pose(s: AgentStatus, agent: AgentId, t: f32, ph: f32) -> Pose {
    let mut p = Pose {
        squash: 1.0,
        ..default()
    };
    match s {
        AgentStatus::Idle => {
            p.squash = 1.0 + 0.025 * (1.5 * t + ph).sin();
            p.look = Vec2::new(
                0.7 * (0.31 * t + ph).sin() + 0.3 * (0.83 * t).sin(),
                0.3 * (0.47 * t + 2.0 * ph).sin(),
            );
            p.roll = 0.03 * (0.9 * t + ph).sin();
            // Now and then: adjusts its hat.
            let g = every(t + ph * 3.0, 9.0, 1.4);
            p.arm_r = Vec2::new(2.4 * g, 0.3 * g);
            p.hat_tilt = 0.12 * g;
        }
        AgentStatus::Working => {
            p.lean = 0.16;
            p.squash = 1.0 + 0.015 * (3.0 * t).sin();
            p.look = Vec2::new(0.15 * (1.7 * t).sin(), -0.55);
            let type_l = 0.16 * (14.0 * t).sin();
            let type_r = 0.16 * (14.0 * t + PI).sin();
            match agent {
                AgentId::Research => {
                    // Searches around with the magnifying glass.
                    p.arm_r =
                        Vec2::new(1.25 + 0.15 * (1.3 * t).sin(), 0.15 + 0.25 * (0.9 * t).sin());
                    p.arm_l = Vec2::new(0.35, 0.1);
                    p.turn = 0.35 * (0.7 * t).sin();
                    p.look = Vec2::new(0.6 * (0.7 * t).sin(), -0.2);
                }
                AgentId::Chart => {
                    // Taps on its chart screen.
                    p.arm_l = Vec2::new(1.2, -0.05);
                    p.arm_r = Vec2::new(1.05 + type_r, -0.3);
                    p.look = Vec2::new(-0.3, -0.5);
                }
                AgentId::Decision => {
                    // Reads, ponders, taps the gavel.
                    let tap = (2.2 * t).sin().max(0.0).powi(8);
                    p.arm_r = Vec2::new(0.7 + 0.5 * tap, 0.05);
                    p.arm_l = Vec2::new(1.3, -0.35);
                    p.lean = 0.1 + 0.04 * (1.1 * t).sin();
                    p.look = Vec2::new(0.25 * (0.5 * t).sin(), -0.3);
                }
                AgentId::Execution => {
                    // Aims, steady, tiny adjustments.
                    p.arm_r = Vec2::new(1.5 + 0.03 * (7.0 * t).sin(), -0.1);
                    p.arm_l = Vec2::new(1.4, 0.12);
                    p.lean = 0.08;
                    p.look = Vec2::new(0.05 * (2.3 * t).sin(), 0.0);
                    p.blink = 0.35;
                }
                AgentId::Assistant => {
                    // Writes its answer.
                    p.arm_l = Vec2::new(1.1 + type_l, -0.15);
                    p.arm_r = Vec2::new(1.1 + type_r, -0.15);
                    p.bubble = 1.0;
                }
            }
        }
        AgentStatus::Done => {
            p.squash = 1.0 + 0.02 * (1.2 * t + ph).sin();
            p.arm_l = Vec2::new(0.1, 0.25);
            p.arm_r = Vec2::new(0.1, 0.25);
            p.look = Vec2::new(0.0, 0.15);
        }
        AgentStatus::NoTrade => {
            // Arms crossed, looks aside.
            p.arm_l = Vec2::new(1.35, -0.8);
            p.arm_r = Vec2::new(1.2, -0.85);
            p.lean = -0.05;
            p.roll = 0.05;
            p.look = Vec2::new(0.45, -0.1);
            p.squash = 1.0 + 0.015 * (1.1 * t + ph).sin();
        }
        AgentStatus::Error => {
            // Scratches its head.
            let scratch = 0.12 * (16.0 * t).sin();
            p.arm_r = Vec2::new(2.75 + scratch, -0.35);
            p.hat_tilt = 0.12 + 0.4 * scratch;
            p.look = Vec2::new(-0.3, 0.4);
            p.roll = -0.06;
        }
    }
    p
}

fn reaction_pose(r: Reaction, u: f32) -> Pose {
    let env = (u / 0.18).clamp(0.0, 1.0) * (1.0 - ((u - 0.75) / 0.25).clamp(0.0, 1.0));
    let mut p = Pose::default();
    match r {
        Reaction::Wave => {
            p.arm_r = Vec2::new(0.2, 2.3 + 0.35 * (u * TAU * 3.0).sin()) * env;
            p.look.y = 0.3 * env;
            p.lift = 0.05 * env;
        }
        Reaction::Throw => {
            p.arm_r.x = -0.9 * bump(u, 0.0, 0.45) + 1.8 * bump(u, 0.35, 1.0);
            p.lean = 0.12 * bump(u, 0.3, 1.0);
        }
        Reaction::Catch => {
            p.arm_l = Vec2::new(1.4, 0.2) * env;
            p.arm_r = Vec2::new(1.4, 0.2) * env;
            p.squash = -0.08 * bump(u, 0.2, 0.6);
        }
        Reaction::Cheer => {
            p.lift = 0.35 * (u * TAU).sin().abs() * env;
            p.arm_l = Vec2::new(2.8, 0.5) * env;
            p.arm_r = Vec2::new(2.8, 0.5) * env;
            p.squash = 0.06 * (u * TAU * 2.0).sin() * env;
            p.look.y = 0.5 * env;
        }
        Reaction::Slump => {
            p.lean = 0.35 * env;
            p.lift = -0.08 * env;
            p.arm_l.y = -0.15 * env;
            p.arm_r.y = -0.15 * env;
            p.look.y = -0.7 * env;
            p.hat_tilt = 0.1 * env;
            p.hat_lift = -0.04 * env;
            p.squash = -0.06 * env;
        }
        Reaction::Nod => {
            p.lift = 0.12 * bump(u, 0.1, 0.5);
            p.arm_l.y = 0.5 * env;
            p.arm_r.y = 0.5 * env;
            p.lean = -0.12 * bump(u, 0.45, 0.9);
        }
        Reaction::Shake => {
            p.turn = 0.4 * (u * TAU * 3.0).sin() * env;
        }
    }
    p
}

fn status_of(hub: &Hub, cfg: &AppConfig, a: AgentId) -> AgentStatus {
    if a == AgentId::Assistant {
        return if hub.chat_busy {
            AgentStatus::Working
        } else {
            AgentStatus::Idle
        };
    }
    hub.symbol(&cfg.active_symbol)
        .map(|v| v.agent(a).status)
        .unwrap_or_default()
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let d = (b - a + PI).rem_euclid(TAU) - PI;
    a + d * t
}

/// Status changes, outcome of the run and clicks become reactions.
pub fn triggers(
    time: Res<Time>,
    hub: Res<Hub>,
    cfg: Res<AppConfig>,
    sel: Res<Selection>,
    mut chars: Query<(&Character, &mut Anim)>,
    mut last_sel: Local<Option<AgentId>>,
    mut last_outcome: Local<Option<Option<chrono::DateTime<chrono::Utc>>>>,
) {
    let now = time.elapsed_secs();
    let calm = cfg.scene.reduced_motion;
    let outcome = hub
        .symbol(&cfg.active_symbol)
        .and_then(|v| v.last_outcome.as_ref());
    let outcome_at = outcome.map(|(at, _)| *at);
    // The first outcome seen (loaded from the database) is not news.
    let fresh = last_outcome.is_some_and(|seen| seen != outcome_at);
    *last_outcome = Some(outcome_at);
    let clicked = (sel.agent != *last_sel).then_some(sel.agent).flatten();
    *last_sel = sel.agent;

    for (c, mut anim) in &mut chars {
        let st = status_of(&hub, &cfg, c.agent);
        if st != anim.status {
            let previous = anim.status;
            anim.status = st;
            if !calm {
                match st {
                    AgentStatus::Done if previous == AgentStatus::Working => {
                        anim.react(Reaction::Nod, now)
                    }
                    AgentStatus::NoTrade => anim.react(Reaction::Shake, now),
                    _ => {}
                }
            }
        }
        if calm {
            anim.reaction = None;
            continue;
        }
        if clicked == Some(c.agent) {
            anim.react(Reaction::Wave, now);
        }
        if fresh && let Some((_, o)) = outcome {
            match (o, c.agent) {
                (RunOutcome::Trade { .. }, AgentId::Decision | AgentId::Execution) => {
                    anim.react(Reaction::Cheer, now)
                }
                (RunOutcome::Trade { .. }, AgentId::Research | AgentId::Chart) => {
                    anim.react(Reaction::Nod, now)
                }
                (RunOutcome::Invalid { .. }, AgentId::Decision | AgentId::Execution) => {
                    anim.react(Reaction::Slump, now)
                }
                _ => {}
            }
        }
    }
}

/// Blend the poses and write the part transforms.
pub fn animate(
    time: Res<Time>,
    cfg: Res<AppConfig>,
    sel: Res<Selection>,
    lay: Res<Layout>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    mut chars: Query<(&Character, &mut Anim, &mut Transform)>,
    mut parts: Query<(&Part, &Rest, &mut Transform, &mut Visibility), Without<Character>>,
) {
    let dt = time.delta_secs();
    let calm = cfg.scene.reduced_motion;
    let t = if calm { 0.0 } else { time.elapsed_secs() };
    let now = time.elapsed_secs();
    let fade = if calm { 1.0 } else { 1.0 - (-7.0 * dt).exp() };
    let cam = cams
        .single()
        .map(|g| g.translation())
        .unwrap_or(Vec3::Z * 40.0);

    for (c, mut anim, mut root) in &mut chars {
        let i = c.agent.index();
        // Cross-fade the status weights.
        let slot = status_slot(anim.status);
        let mut sum = 0.0;
        for (k, w) in anim.weights.iter_mut().enumerate() {
            let goal = if k == slot { 1.0 } else { 0.0 };
            *w += (goal - *w) * fade;
            sum += *w;
        }
        let mut pose = Pose::default();
        for (k, s) in [
            AgentStatus::Idle,
            AgentStatus::Working,
            AgentStatus::Done,
            AgentStatus::NoTrade,
            AgentStatus::Error,
        ]
        .into_iter()
        .enumerate()
        {
            let w = anim.weights[k] / sum.max(1e-4);
            if w > 1e-3 {
                pose.add(&status_pose(s, c.agent, t, anim.phase), w);
            }
        }
        let mut wants_face = sel.hovered == Some(c.agent) || sel.agent == Some(c.agent);
        if let Some((r, start)) = anim.reaction {
            let u = (now - start) / r.duration();
            if u >= 1.0 {
                anim.reaction = None;
            } else {
                pose.add(&reaction_pose(r, u), 1.0);
                wants_face |= matches!(r, Reaction::Wave | Reaction::Cheer);
            }
        }
        // Blink every few seconds.
        if !calm {
            pose.blink = pose.blink.max(every(t + anim.phase * 2.1, 3.7, 0.14));
        }

        // Spin on itself, or turn towards the camera when looked at.
        let goal_face = if wants_face { 1.0 } else { 0.0 };
        let kf = if calm { 1.0 } else { 1.0 - (-5.0 * dt).exp() };
        anim.face += (goal_face - anim.face) * kf;
        let pos = lay.agents[i];
        let to_cam = cam - pos;
        let cam_yaw = to_cam.x.atan2(to_cam.z);
        let yaw = lerp_angle(lay.spins[i], cam_yaw, anim.face);
        // Seen from above, a character leans back a little, away from the
        // camera, so its face reads better than the top of its hat.
        let flat = Vec2::new(to_cam.x, to_cam.z);
        let elevation = to_cam.y.atan2(flat.length().max(1e-3));
        let away = -Vec3::new(flat.x, 0.0, flat.y).normalize_or_zero();
        let lean = 0.5 * elevation.max(0.0);
        let up = (Vec3::Y * lean.cos() + away * lean.sin()).normalize();
        root.rotation =
            Quat::from_rotation_arc(Vec3::Y, up) * Quat::from_rotation_y(yaw + pose.turn);

        let p = &c.parts;
        let side = |e: Entity| if e == p.arm_l { -1.0 } else { 1.0 };
        for e in [p.body, p.arm_l, p.arm_r, p.hat, p.eye_l, p.eye_r, p.alert]
            .into_iter()
            .chain(p.bubble)
        {
            let Ok((part, rest, mut tf, mut vis)) = parts.get_mut(e) else {
                continue;
            };
            let r = rest.0;
            match part {
                Part::Body => {
                    let sq = pose.squash.max(0.5);
                    tf.translation = r.translation + Vec3::Y * pose.lift;
                    tf.rotation =
                        Quat::from_rotation_x(pose.lean) * Quat::from_rotation_z(pose.roll);
                    tf.scale = Vec3::new(1.0 / sq.sqrt(), sq, 1.0 / sq.sqrt());
                }
                Part::ArmL | Part::ArmR => {
                    let s = side(e);
                    let a = if *part == Part::ArmL {
                        pose.arm_l
                    } else {
                        pose.arm_r
                    };
                    tf.rotation =
                        r.rotation * Quat::from_rotation_z(s * a.y) * Quat::from_rotation_x(-a.x);
                }
                Part::EyeL | Part::EyeR => {
                    let look = pose.look.clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
                    tf.translation = r.translation + Vec3::new(look.x * 0.045, look.y * 0.035, 0.0);
                    tf.scale =
                        r.scale * Vec3::new(1.0, 1.0 - 0.9 * pose.blink.clamp(0.0, 1.0), 1.0);
                }
                Part::Hat => {
                    tf.translation = r.translation + Vec3::Y * pose.hat_lift;
                    tf.rotation = Quat::from_rotation_z(-pose.hat_tilt)
                        * Quat::from_rotation_x(pose.hat_tilt * 0.5);
                }
                Part::Alert => {
                    let on = anim.status == AgentStatus::Error;
                    let want = if on {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                    if *vis != want {
                        *vis = want;
                    }
                    tf.translation = r.translation + Vec3::Y * 0.06 * (3.0 * t).sin();
                }
                Part::Bubble => {
                    let k = pose.bubble.clamp(0.0, 1.0);
                    let s = 1.0 + 0.08 * k * (6.0 * t).sin();
                    tf.scale = Vec3::splat(s);
                    tf.translation = r.translation + Vec3::Y * 0.05 * (1.3 * t).sin();
                }
            }
        }
    }
}

// ── Hand-offs and work sparks ─────────────────────────────────────────────

#[derive(Component)]
pub struct Parcel {
    from: AgentId,
    to: AgentId,
    /// Progress 0..1; negative while the sender winds up.
    t: f32,
}

#[derive(Component)]
pub struct Spark {
    agent: AgentId,
    seed: f32,
}

#[derive(Resource)]
pub struct FxAssets {
    parcel_mesh: Handle<Mesh>,
    parcel_mat: Handle<StandardMaterial>,
}

pub fn setup_fx(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
) {
    let spark_mesh = meshes.add(Sphere::new(1.0).mesh().ico(1).expect("ico sphere"));
    let spark_mat = mats.add(StandardMaterial {
        base_color: theme::bevy_color(theme::PALE_ORANGE),
        emissive: theme::bevy_color(theme::PALE_ORANGE).to_linear() * 6.0,
        unlit: true,
        ..default()
    });
    for agent in AgentId::ALL {
        for k in 0..6 {
            commands.spawn((
                Mesh3d(spark_mesh.clone()),
                MeshMaterial3d(spark_mat.clone()),
                Transform::from_scale(Vec3::ZERO),
                Visibility::Hidden,
                Spark {
                    agent,
                    seed: k as f32 / 6.0,
                },
                Pickable::IGNORE,
            ));
        }
    }
    commands.insert_resource(FxAssets {
        parcel_mesh: meshes.add(Cuboid::new(0.24, 0.16, 0.03)),
        parcel_mat: mats.add(StandardMaterial {
            base_color: theme::bevy_color(theme::AMBER),
            emissive: theme::bevy_color(theme::PALE_ORANGE).to_linear() * 3.0,
            ..default()
        }),
    });
}

/// Arc between two moving bodies, lifted above the orbital plane.
pub fn arc(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    let mid = (a + b) * 0.5 + Vec3::Y * (1.0 + a.distance(b) * 0.3);
    a.lerp(mid, t).lerp(mid.lerp(b, t), t)
}

const HAND_LIFT: Vec3 = Vec3::new(0.0, 0.2, 0.0);
const FLIGHT: f32 = 1.4;

pub fn handoffs(
    mut commands: Commands,
    time: Res<Time>,
    mut hub: ResMut<Hub>,
    cfg: Res<AppConfig>,
    lay: Res<Layout>,
    fx: Res<FxAssets>,
    mut gizmos: Gizmos,
    mut chars: Query<(&Character, &mut Anim)>,
    mut parcels: Query<(Entity, &mut Parcel, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let calm = cfg.scene.reduced_motion;
    let flows = std::mem::take(&mut hub.flows);
    for f in flows {
        if !calm {
            for (c, mut a) in &mut chars {
                if c.agent == f.from {
                    a.react(Reaction::Throw, now);
                }
            }
        }
        commands.spawn((
            Mesh3d(fx.parcel_mesh.clone()),
            MeshMaterial3d(fx.parcel_mat.clone()),
            Transform::from_translation(lay.agents[f.from.index()]).with_scale(Vec3::ZERO),
            Parcel {
                from: f.from,
                to: f.to,
                t: if calm { 0.0 } else { -0.25 },
            },
            Pickable::IGNORE,
        ));
    }
    let wire = theme::bevy_color(theme::PALE_ORANGE);
    for (e, mut p, mut tf) in &mut parcels {
        p.t += dt / FLIGHT;
        let (a, b) = (
            lay.agents[p.from.index()] + HAND_LIFT,
            lay.agents[p.to.index()] + HAND_LIFT,
        );
        if p.t >= 1.0 {
            if !calm {
                for (c, mut anim) in &mut chars {
                    if c.agent == p.to {
                        anim.react(Reaction::Catch, now);
                    }
                }
            }
            commands.entity(e).despawn();
            continue;
        }
        let u = p.t.max(0.0);
        // The path lights up while the envelope travels on it.
        let fade = (u * 4.0).min(1.0) * ((1.0 - u) * 4.0).min(1.0);
        gizmos.linestrip(
            (0..=24).map(|k| arc(a, b, k as f32 / 24.0)),
            wire.with_alpha(0.35 * fade),
        );
        tf.translation = arc(a, b, u);
        tf.scale = Vec3::splat(if p.t < 0.0 { 0.0 } else { 1.0 });
        tf.rotation = if calm {
            Quat::IDENTITY
        } else {
            Quat::from_rotation_y(u * TAU * 1.5) * Quat::from_rotation_z(0.3 * (u * TAU).sin())
        };
    }
}

/// A few sparks rise from an agent at work (not in reduced motion).
pub fn sparks(
    time: Res<Time>,
    cfg: Res<AppConfig>,
    lay: Res<Layout>,
    chars: Query<(&Character, &Anim)>,
    mut q: Query<(&Spark, &mut Transform, &mut Visibility)>,
) {
    let t = time.elapsed_secs();
    let mut working = [false; 5];
    for (c, a) in &chars {
        working[c.agent.index()] = a.status == AgentStatus::Working && !cfg.scene.reduced_motion;
    }
    for (s, mut tf, mut vis) in &mut q {
        let i = s.agent.index();
        let want = if working[i] {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
        if !working[i] {
            continue;
        }
        let k = (t * 0.6 + s.seed).fract();
        let a = s.seed * TAU + t * 1.3;
        let r = lay.sizes[i] * (0.45 + 0.3 * k);
        tf.translation =
            lay.agents[i] + Vec3::new(a.cos() * r, lay.sizes[i] * (0.2 + 1.1 * k), a.sin() * r);
        tf.scale = Vec3::splat(0.035 * (1.0 - k) + 0.005);
    }
}
