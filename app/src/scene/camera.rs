//! Orbit camera with inertia.
//!
//! - Dragging turns the view; on release the rotation keeps its speed and
//!   slows down smoothly.
//! - The wheel sets a goal distance; the distance eases towards it.
//! - Focusing an agent follows it as it drifts: the camera targets the live
//!   position plus an offset that decays to zero, so the flight starts where
//!   the camera was and ends locked on the moving node, without jumps.
//! - Pressing on a node drags the node, not the view.
//! - Every smoothing uses `1 - exp(-k·dt)`: the same motion at any frame rate.
//! - Reduced motion: no inertia, instant moves.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy_egui::input::EguiWantsInput;
use merces_core::AgentId;

use super::graph::CENTER;
use super::{Layout, Selection};
use crate::config::AppConfig;

/// Distance of the overview (whole network in view).
pub const OVERVIEW_DIST: f32 = 23.0;
/// Pitch limits (radians above the horizontal): the network reads from the
/// front, slightly from above or below.
const PITCH_MIN: f32 = -0.6;
const PITCH_MAX: f32 = 1.0;
/// Pause between the agent's greeting and the flight towards it.
const GREETING: f32 = 0.3;

#[derive(Component)]
pub struct OrbitCam {
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub target: Vec3,
    goal_dist: f32,
    yaw_vel: f32,
    pitch_vel: f32,
    /// What the camera looks at, and since when (for the greeting pause).
    focus: Option<AgentId>,
    pending: Option<(Option<AgentId>, f32)>,
    /// Decaying offset between the target and the focused point.
    offset: Vec3,
    /// Sideways shift that keeps a focused agent left of the side panel.
    side: f32,
    drag: f32,
    /// The current press started on a node (it drags the node, not the view).
    on_node: bool,
}

impl Default for OrbitCam {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.18,
            dist: OVERVIEW_DIST,
            target: CENTER,
            goal_dist: OVERVIEW_DIST,
            yaw_vel: 0.0,
            pitch_vel: 0.0,
            focus: None,
            pending: None,
            offset: Vec3::ZERO,
            side: 0.0,
            drag: 0.0,
            on_node: false,
        }
    }
}

impl OrbitCam {
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(-self.pitch)
    }
}

fn ease(rate: f32, dt: f32) -> f32 {
    1.0 - (-rate * dt).exp()
}

pub fn camera_control(
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
) {
    let Ok((mut tf, mut cam, proj)) = cams.single_mut() else {
        return;
    };
    let dt = time.delta_secs().min(0.1);
    let now = time.elapsed_secs();
    let calm = cfg.scene.reduced_motion;
    let over_ui = egui.wants_any_pointer_input();

    if keys.just_pressed(KeyCode::Escape) && !egui.wants_any_keyboard_input() {
        sel.agent = None;
        sel.output = None;
    }
    let press = buttons.just_pressed(MouseButton::Left) || buttons.just_pressed(MouseButton::Right);
    if press {
        cam.on_node = sel.hovered.is_some() || sel.input_hovered || sel.badge.is_some();
        sel.drag_px = 0.0;
        cam.drag = 0.0;
        cam.yaw_vel = 0.0;
        cam.pitch_vel = 0.0;
    }
    let held = buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Right);
    if !over_ui {
        // The motion reported on the press frame happened before the press: ignore it.
        if !press && held && !cam.on_node && !sel.dragging {
            let (dy, dp) = (-motion.delta.x * 0.005, motion.delta.y * 0.004);
            cam.yaw += dy;
            cam.pitch += dp;
            cam.drag += motion.delta.length();
            // Velocity for the inertia, smoothed over the last few frames.
            if dt > 0.0 {
                let k = ease(25.0, dt);
                cam.yaw_vel += (dy / dt - cam.yaw_vel) * k;
                cam.pitch_vel += (dp / dt - cam.pitch_vel) * k;
            }
        }
        let lines = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        if lines != 0.0 {
            cam.goal_dist = (cam.goal_dist * (1.0 - lines * 0.1)).clamp(2.5, 90.0);
        }
        // A short click on empty space returns to the overview.
        if buttons.just_released(MouseButton::Left)
            && cam.drag < 4.0
            && sel.hovered.is_none()
            && !sel.input_hovered
            && sel.badge.is_none()
            && sel.agent.is_some()
        {
            sel.agent = None;
            sel.output = None;
        }
    }
    // Inertia after release.
    if !held || over_ui {
        if calm {
            cam.yaw_vel = 0.0;
            cam.pitch_vel = 0.0;
        }
        let (yv, pv) = (cam.yaw_vel, cam.pitch_vel);
        cam.yaw += yv * dt;
        cam.pitch += pv * dt;
        let decay = (-3.5 * dt).exp();
        cam.yaw_vel *= decay;
        cam.pitch_vel *= decay;
    }
    if cam.pitch < PITCH_MIN || cam.pitch > PITCH_MAX {
        cam.pitch = cam.pitch.clamp(PITCH_MIN, PITCH_MAX);
        cam.pitch_vel = 0.0;
    }

    // Focus changes: the agent greets first, then the camera flies.
    if sel.agent != cam.focus && cam.pending.map(|p| p.0) != Some(sel.agent) {
        let delay = if sel.agent.is_some() && !calm {
            GREETING
        } else {
            0.0
        };
        cam.pending = Some((sel.agent, now + delay));
    }
    if let Some((next, at)) = cam.pending
        && now >= at
    {
        cam.pending = None;
        let new_point = next.map_or(CENTER, |a| lay.agents[a.index()]);
        // Continuity: start from where the camera looks now.
        cam.offset = cam.target - new_point;
        cam.focus = next;
        cam.goal_dist = match next {
            Some(a) => (lay.sizes[a.index()] * 7.5).max(4.5),
            None => OVERVIEW_DIST,
        };
    }

    let k = if calm { 1.0 } else { ease(7.0, dt) };
    cam.dist += (cam.goal_dist - cam.dist) * k;
    let ko = if calm { 1.0 } else { ease(3.2, dt) };
    cam.offset *= 1.0 - ko;

    // Keep a focused agent left of the side panel.
    let side_goal = match (cam.focus, proj, windows.single()) {
        (Some(_), Projection::Perspective(p), Ok(w)) if sel.panel_px > 0.0 => {
            let half_w = (p.fov * 0.5).tan() * cam.dist * w.width() / w.height().max(1.0);
            half_w * (sel.panel_px / w.width().max(1.0))
        }
        _ => 0.0,
    };
    cam.side += (side_goal - cam.side) * ko;

    let rot = cam.rotation();
    let focus_point = cam.focus.map_or(CENTER, |a| lay.agents[a.index()]);
    cam.target = focus_point + cam.offset + rot * Vec3::X * cam.side;
    tf.translation = cam.target + rot * Vec3::new(0.0, 0.0, cam.dist);
    tf.look_at(cam.target, Vec3::Y);
}
