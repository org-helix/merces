//! The agents as pixel-art characters, built from voxels: a 12×8 pixel body
//! with two side arms, two eyes and four legs (the pixel creature of the
//! reference sprite, given depth), and a pixel hat and accessory per job so
//! each one reads at a glance:
//!
//! | Agent        | Hat                       | Accessory              |
//! |--------------|---------------------------|------------------------|
//! | Research     | explorer helmet           | magnifying glass       |
//! | Chart        | trader's visor            | chart screen           |
//! | Decision     | top hat, accent band      | gavel                  |
//! | Execution    | pilot cap with a badge    | sight over one eye     |
//! | Assistant    | guide's hat               | speech bubble          |
//!
//! Each piece is one mesh (see `voxel.rs`). A pixel ring under each agent
//! shows its state. Animation lives in `anim.rs`.
//!
//! Today's results are pixel badges floating beside their agent: one per
//! result, coloured by its outcome, clickable to open the report.

use bevy::prelude::*;
use merces_core::{Action, AgentId, AgentOutput, AgentStatus, Bias};

use super::{Layout, Selection, voxel};
use crate::config::AppConfig;
use crate::state::Hub;
use crate::theme::{self, Theme};

/// A body part driven by the animation.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Body,
    ArmL,
    ArmR,
    Hat,
    EyeL,
    EyeR,
    Alert,
    Bubble,
}

/// Rest transform of an animated part.
#[derive(Component, Clone, Copy)]
pub struct Rest(pub Transform);

#[derive(Clone, Copy)]
pub struct Parts {
    pub body: Entity,
    pub arm_l: Entity,
    pub arm_r: Entity,
    pub hat: Entity,
    pub eye_l: Entity,
    pub eye_r: Entity,
    pub alert: Entity,
    pub bubble: Option<Entity>,
}

/// The root of a character; its transform is set by the graph layout.
#[derive(Component)]
pub struct Character {
    pub agent: AgentId,
    pub scale: f32,
    pub parts: Parts,
    pub body_material: Handle<StandardMaterial>,
}

#[derive(Component)]
pub struct ResultBadge {
    pub agent: AgentId,
    pub index: usize,
}

#[derive(Resource)]
pub struct CharacterAssets {
    badge_mesh: Handle<Mesh>,
    badge_mats: [Handle<StandardMaterial>; 5],
}

fn glow(mats: &mut Assets<StandardMaterial>, c: Color, k: f32) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: c,
        emissive: c.to_linear() * k,
        ..default()
    })
}

pub fn body_color(agent: AgentId, accent: bevy_egui::egui::Color32) -> Color {
    // The scene is always a night sky: the dark palette applies.
    let c = match agent {
        AgentId::Research => theme::BLUSH,
        AgentId::Chart => theme::PALE_ORANGE,
        AgentId::Decision => accent,
        AgentId::Execution => theme::AMBER,
        AgentId::Assistant => bevy_egui::egui::Color32::from_rgb(0xB8, 0x9E, 0x86),
    };
    theme::bevy_color(c)
}

/// Size of one pixel of the characters, in world units (before the character scale).
pub const PX: f32 = 0.075;

/// Body voxels sit around the origin: x in [-6, 6), y in [-4, 4) (+ legs
/// below), z in [-3, 3).
const BODY_ORIGIN: Vec3 = Vec3::new(0.0, 4.0, 0.0);

fn vox_mat(mats: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.85,
        reflectance: 0.2,
        ..default()
    })
}

/// The shared body: the 12×8 pixel block with its four legs, as in the
/// reference sprite, given depth. The colour comes from the material.
fn body_model() -> voxel::Model {
    let mut m = voxel::Model::new();
    let t = voxel::TINT;
    m.fill(IVec3::new(-6, 0, -3), IVec3::new(6, 8, 3), t);
    // A lighter top row and a slightly darker belly row: depth without outlines.
    m.fill(
        IVec3::new(-6, 7, -3),
        IVec3::new(6, 8, 3),
        [1.08, 1.08, 1.08],
    );
    m.fill(IVec3::new(-6, 0, -3), IVec3::new(6, 1, 3), [0.9, 0.9, 0.9]);
    for x in [-5, -3, 2, 4] {
        m.fill(
            IVec3::new(x, -2, -1),
            IVec3::new(x + 1, 0, 1),
            [0.92, 0.92, 0.92],
        );
    }
    m
}

/// A side arm (2×2 pixels), pivoting at the body's edge.
fn arm_model(side: i32) -> voxel::Model {
    let mut m = voxel::Model::new();
    let (x0, x1) = if side > 0 { (0, 2) } else { (-2, 0) };
    m.fill(IVec3::new(x0, -1, -1), IVec3::new(x1, 1, 1), voxel::TINT);
    m
}

pub fn spawn_characters(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    theme: &Theme,
) {
    let body_mesh = meshes.add(body_model().mesh(PX, BODY_ORIGIN));
    let arm_l_mesh = meshes.add(arm_model(-1).mesh(PX, Vec3::ZERO));
    let arm_r_mesh = meshes.add(arm_model(1).mesh(PX, Vec3::ZERO));
    let eye_mesh = {
        let mut m = voxel::Model::new();
        m.fill(IVec3::ZERO, IVec3::new(1, 2, 1), voxel::rgb(0x0B0908));
        meshes.add(m.mesh(PX, Vec3::new(0.5, 1.0, 0.5)))
    };
    let alert_mesh = {
        let mut m = voxel::Model::new();
        let red = voxel::rgb(0xFF6B5E);
        m.fill(IVec3::new(0, 2, 0), IVec3::new(1, 6, 1), red);
        m.set(IVec3::ZERO, red);
        meshes.add(m.mesh(PX, Vec3::new(0.5, 0.0, 0.5)))
    };
    let pedestal_mesh = meshes.add(pedestal_model().mesh(PX, Vec3::new(0.5, 0.0, 0.5)));
    let vox = vox_mat(mats);
    let alert = glow(mats, Color::WHITE, 2.5);

    for agent in AgentId::ALL {
        let scale = if agent == AgentId::Assistant {
            1.0
        } else {
            1.3
        };
        let color = body_color(agent, theme.pal.accent);
        let body_material = mats.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.8,
            reflectance: 0.25,
            emissive: color.to_linear() * 0.04,
            ..default()
        });
        let root = commands
            .spawn((
                Transform::from_scale(Vec3::splat(scale)),
                Visibility::default(),
                Name::new(agent.key()),
            ))
            .id();

        // A pixel ring under the feet that shows the agent's state.
        let pedestal_material = glow(mats, theme::bevy_color(theme::AMBER), 0.15);
        commands.spawn((
            Mesh3d(pedestal_mesh.clone()),
            MeshMaterial3d(pedestal_material.clone()),
            Transform::from_xyz(0.0, -7.2 * PX, 0.0),
            Pedestal {
                agent,
                material: pedestal_material,
            },
            Pickable::IGNORE,
            ChildOf(root),
        ));

        let body = commands
            .spawn((
                Mesh3d(body_mesh.clone()),
                MeshMaterial3d(body_material.clone()),
                Transform::IDENTITY,
                Rest(Transform::IDENTITY),
                Part::Body,
                ChildOf(root),
            ))
            .id();

        // Eyes: one pixel wide, two tall, standing out of the front face.
        let eye_at = |x: f32| {
            Transform::from_translation(Vec3::new(x * PX, PX, 3.2 * PX))
                .with_scale(Vec3::new(1.0, 1.0, 0.5))
        };
        let mut eye = |x: f32, part: Part| {
            commands
                .spawn((
                    Mesh3d(eye_mesh.clone()),
                    MeshMaterial3d(vox.clone()),
                    eye_at(x),
                    Rest(eye_at(x)),
                    part,
                    ChildOf(body),
                ))
                .id()
        };
        let eye_l = eye(-3.5, Part::EyeL);
        let eye_r = eye(3.5, Part::EyeR);

        // Arms: the two side pixels, on a pivot at the body's edge.
        let mut arm = |side: f32, mesh: &Handle<Mesh>, part: Part| {
            let rest = Transform::from_translation(Vec3::new(side * 6.0 * PX, -PX, 0.0));
            let pivot = commands
                .spawn((rest, Rest(rest), Visibility::default(), part, ChildOf(body)))
                .id();
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(body_material.clone()),
                Transform::IDENTITY,
                ChildOf(pivot),
            ));
            pivot
        };
        let arm_l = arm(-1.0, &arm_l_mesh, Part::ArmL);
        let arm_r = arm(1.0, &arm_r_mesh, Part::ArmR);

        let hat_rest = Transform::from_translation(Vec3::new(0.0, 4.0 * PX, 0.0));
        let hat = commands
            .spawn((
                hat_rest,
                Rest(hat_rest),
                Visibility::default(),
                Part::Hat,
                ChildOf(body),
            ))
            .id();

        // Error signal: a pixel "!" above the hat, hidden unless in error.
        let alert_rest = Transform::from_xyz(0.0, 15.0 * PX, 0.0);
        let alert_e = commands
            .spawn((
                alert_rest,
                Rest(alert_rest),
                Visibility::Hidden,
                Part::Alert,
                ChildOf(body),
            ))
            .id();
        commands.spawn((
            Mesh3d(alert_mesh.clone()),
            MeshMaterial3d(alert.clone()),
            Transform::IDENTITY,
            ChildOf(alert_e),
            Pickable::IGNORE,
        ));

        let bubble = dress(commands, meshes, &vox, agent, body, hat, arm_r, theme);

        commands.entity(root).insert((
            super::anim::Anim::new(agent),
            Character {
                agent,
                scale,
                parts: Parts {
                    body,
                    arm_l,
                    arm_r,
                    hat,
                    eye_l,
                    eye_r,
                    alert: alert_e,
                    bubble,
                },
                body_material,
            },
        ));
        commands
            .entity(root)
            .observe(move |ev: On<PointerClick>, mut sel: ResMut<Selection>| {
                // A click that ends a drag only moved the node.
                if ev.button == PointerButton::Primary && sel.drag_px < 6.0 {
                    sel.agent = Some(agent);
                    sel.output = None;
                }
            })
            .observe(super::graph::drag_start(agent.index()))
            .observe(super::graph::drag_move(agent.index()))
            .observe(super::graph::drag_end)
            .observe(move |_: On<PointerOver>, mut sel: ResMut<Selection>| {
                sel.hovered = Some(agent)
            })
            .observe(move |_: On<PointerOut>, mut sel: ResMut<Selection>| {
                if sel.hovered == Some(agent) {
                    sel.hovered = None;
                }
            });
    }
}

/// A flat ring of pixels (radius about 7).
fn pedestal_model() -> voxel::Model {
    let mut m = voxel::Model::new();
    for x in -8..8 {
        for z in -8..8 {
            let d = ((x as f32 + 0.5).powi(2) + (z as f32 + 0.5).powi(2)).sqrt();
            if (6.2..7.6).contains(&d) {
                m.set(IVec3::new(x, 0, z), voxel::TINT);
            }
        }
    }
    m
}

/// A voxel piece attached to `parent`.
fn piece(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    vox: &Handle<StandardMaterial>,
    model: &voxel::Model,
    parent: Entity,
    at: Vec3,
) -> Entity {
    commands
        .spawn((
            Mesh3d(meshes.add(model.mesh(PX, Vec3::ZERO))),
            MeshMaterial3d(vox.clone()),
            Transform::from_translation(at),
            ChildOf(parent),
        ))
        .id()
}

/// Pixel hat and accessory of one agent. Returns the speech bubble (assistant only).
#[allow(clippy::too_many_arguments)]
fn dress(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    vox: &Handle<StandardMaterial>,
    agent: AgentId,
    body: Entity,
    hat: Entity,
    arm_r: Entity,
    theme: &Theme,
) -> Option<Entity> {
    let v = IVec3::new;
    let rgb = voxel::rgb;
    let accent = theme.pal.accent;
    let accent = [
        f32::from(accent.r()) / 255.0,
        f32::from(accent.g()) / 255.0,
        f32::from(accent.b()) / 255.0,
    ];
    let ink = rgb(0x2A221C);
    let mut h = voxel::Model::new();
    let mut tool = voxel::Model::new();
    let mut on_body = voxel::Model::new();
    match agent {
        AgentId::Research => {
            // Explorer helmet: wide brim, dark band, khaki dome.
            let (khaki, light) = (rgb(0xB59A73), rgb(0xCDB48A));
            h.fill(v(-7, 0, -4), v(7, 1, 4), khaki)
                .fill(v(-5, 1, -3), v(5, 2, 3), ink)
                .fill(v(-5, 2, -3), v(5, 4, 3), khaki)
                .fill(v(-4, 4, -2), v(4, 5, 2), light)
                .fill(v(-2, 5, -1), v(2, 6, 1), light);
            // Magnifying glass: brass frame, pale lens, wooden handle.
            let (brass, lens, wood) = (rgb(0xC99A4E), rgb(0xBFD8E8), rgb(0x6B4A2E));
            tool.fill(v(1, -3, 0), v(2, -1, 1), wood)
                .fill(v(0, -8, 0), v(4, -3, 1), brass)
                .fill(v(1, -7, 0), v(3, -4, 1), lens);
        }
        AgentId::Chart => {
            // Trader's visor: a light band and a green bill.
            let (band, green, tip) = (rgb(0xEDE6DA), rgb(0x3E8E5A), rgb(0x5DB57B));
            h.fill(v(-6, 0, -3), v(6, 1, 3), band)
                .clear(v(-5, 0, -2), v(5, 1, 2))
                .fill(v(-5, 0, 3), v(5, 1, 5), green)
                .fill(v(-4, 0, 5), v(4, 1, 6), tip);
            // A small chart screen with candles.
            let (screen, up, down) = (rgb(0x1E1914), rgb(0x4ADE80), rgb(0xFF8A8A));
            tool.fill(v(1, -8, 0), v(9, -2, 1), ink)
                .fill(v(2, -7, 0), v(8, -3, 1), screen);
            for (x, y0, y1, c) in [
                (2, -7, -5, up),
                (3, -6, -4, down),
                (4, -6, -3, up),
                (5, -5, -4, down),
                (6, -5, -3, up),
                (7, -4, -3, up),
            ] {
                tool.fill(v(x, y0, 1), v(x + 1, y1, 2), c);
            }
        }
        AgentId::Decision => {
            // Top hat with a band in the accent colour.
            let (black, top) = (rgb(0x231D19), rgb(0x3C332B));
            h.fill(v(-6, 0, -4), v(6, 1, 4), black)
                .fill(v(-4, 1, -3), v(4, 2, 3), accent)
                .fill(v(-4, 2, -3), v(4, 7, 3), black)
                .fill(v(-4, 7, -3), v(4, 8, 3), top);
            // Gavel.
            let (wood, dark) = (rgb(0x8A5A3A), rgb(0x5C3A24));
            tool.fill(v(1, -5, 0), v(2, -1, 1), wood)
                .fill(v(-1, -7, -1), v(4, -5, 2), dark)
                .fill(v(0, -7, -1), v(1, -5, 2), wood);
        }
        AgentId::Execution => {
            // Pilot cap: navy crown, black visor, gold badge.
            let (navy, light, gold) = (rgb(0x2E3A55), rgb(0x3E4C6E), rgb(0xE0B040));
            h.fill(v(-5, 0, -3), v(5, 3, 3), navy)
                .fill(v(-4, 3, -2), v(4, 4, 2), light)
                .fill(v(-5, 0, 3), v(5, 1, 6), ink)
                .fill(v(-1, 1, 3), v(1, 3, 4), gold);
            // Sight over the right eye: a red pixel frame.
            let red = rgb(0xD9483B);
            on_body
                .fill(v(2, 3, 3), v(6, 7, 4), red)
                .clear(v(3, 4, 3), v(5, 6, 4));
        }
        AgentId::Assistant => {
            // Guide's hat: stepped point with a gold band.
            let (brown, gold) = (rgb(0x7A5A3A), rgb(0xD9B25A));
            h.fill(v(-6, 0, -4), v(6, 1, 4), brown)
                .fill(v(-4, 1, -3), v(4, 2, 3), gold)
                .fill(v(-4, 2, -3), v(4, 3, 3), brown)
                .fill(v(-3, 3, -2), v(3, 5, 2), brown)
                .fill(v(-2, 5, -1), v(2, 7, 1), brown)
                .fill(v(-1, 7, -1), v(0, 9, 0), brown);
        }
    }
    piece(commands, meshes, vox, &h, hat, Vec3::ZERO);
    if tool.len() > 0 {
        piece(
            commands,
            meshes,
            vox,
            &tool,
            arm_r,
            Vec3::new(2.0 * PX, 0.0, 1.0 * PX),
        );
    }
    if on_body.len() > 0 {
        piece(commands, meshes, vox, &on_body, body, -BODY_ORIGIN * PX);
    }
    if agent != AgentId::Assistant {
        return None;
    }
    // Speech bubble beside the head, with three dots.
    let (paper, dots) = (rgb(0xFAF6ED), rgb(0x322921));
    let mut b = voxel::Model::new();
    b.fill(v(0, 1, 0), v(9, 6, 1), paper)
        .clear(v(0, 1, 0), v(1, 2, 1))
        .clear(v(8, 1, 0), v(9, 2, 1))
        .clear(v(0, 5, 0), v(1, 6, 1))
        .clear(v(8, 5, 0), v(9, 6, 1))
        .set(v(1, 0, 0), paper);
    for x in [2, 4, 6] {
        b.set(v(x, 3, 1), dots);
    }
    let rest = Transform::from_translation(Vec3::new(5.0 * PX, 8.0 * PX, 0.0));
    let bubble = commands
        .spawn((
            rest,
            Rest(rest),
            Visibility::default(),
            Part::Bubble,
            ChildOf(body),
        ))
        .id();
    piece(commands, meshes, vox, &b, bubble, Vec3::ZERO);
    Some(bubble)
}

/// Marker of the status ring under an agent.
#[derive(Component)]
pub struct Pedestal {
    agent: AgentId,
    material: Handle<StandardMaterial>,
}

/// The ring under each agent glows in its state's colour; it breathes while
/// the agent works. Materials are written only when the look changes.
pub fn pedestals(
    time: Res<Time>,
    cfg: Res<AppConfig>,
    theme: Res<Theme>,
    sel: Res<Selection>,
    anims: Query<(&Character, &super::anim::Anim)>,
    rings: Query<&Pedestal>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut last: Local<[Option<(u8, bool, u32)>; 5]>,
) {
    let t = time.elapsed_secs();
    let calm = cfg.scene.reduced_motion;
    let mut status = [AgentStatus::Idle; 5];
    for (c, a) in &anims {
        status[c.agent.index()] = a.status;
    }
    for r in &rings {
        let i = r.agent.index();
        let s = status[i];
        let lit = sel.agent == Some(r.agent) || sel.hovered == Some(r.agent);
        let working = s == AgentStatus::Working && !calm;
        // Quantised breathing so the material is not rewritten every frame for nothing.
        let phase = if working {
            ((t * 2.4).sin() * 8.0).round() as i32 + 8
        } else {
            0
        } as u32;
        let key = (s as u8, lit, phase);
        if last[i] == Some(key) {
            continue;
        }
        last[i] = Some(key);
        let p = theme.pal;
        let (c, k) = match s {
            AgentStatus::Idle => (theme::AMBER, 0.12),
            AgentStatus::Working => (p.accent, 1.2 + 0.12 * phase as f32),
            AgentStatus::Done => (theme::DARK.success, 0.9),
            AgentStatus::NoTrade => (theme::DARK.warning, 0.9),
            AgentStatus::Error => (theme::DARK.error, 1.4),
        };
        let k = if lit { k + 0.5 } else { k };
        if let Some(mut m) = mats.get_mut(&r.material) {
            let col = theme::bevy_color(c);
            m.base_color = col;
            m.emissive = col.to_linear() * k;
        }
    }
}

// ── Result badges ──────────────────────────────────────────────────────────

const BADGES_MAX: usize = 6;

pub fn setup_badges(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
) {
    let p = theme::DARK;
    let m = |mats: &mut Assets<StandardMaterial>, c| glow(mats, theme::bevy_color(c), 1.2);
    let badge_mats = [
        m(mats, p.success),
        m(mats, p.error),
        m(mats, p.warning),
        m(mats, theme::RED),
        m(mats, theme::PALE_ORANGE),
    ];
    commands.insert_resource(CharacterAssets {
        badge_mesh: meshes.add(Cuboid::new(0.16, 0.035, 0.16)),
        badge_mats,
    });
}

/// Colour slot of a result: 0 up/buy, 1 down/sell, 2 nothing to do, 3 plan, 4 neutral.
pub fn badge_kind(o: &AgentOutput) -> usize {
    let bias = |b: Bias| match b {
        Bias::Bullish => 0,
        Bias::Bearish => 1,
        Bias::Neutral => 4,
    };
    match o {
        AgentOutput::Brief(b) => bias(b.bias_day),
        AgentOutput::Chart(c) => bias(c.bias),
        AgentOutput::Decision(d) => match d.action {
            Action::Buy => 0,
            Action::Sell => 1,
            Action::NoTrade => 2,
        },
        AgentOutput::Plan(_) => 3,
    }
}

/// Keep one badge per result of the day (max 6 per agent) and place them in a
/// small column beside their agent, facing the camera.
pub fn badges(
    mut commands: Commands,
    hub: Res<Hub>,
    cfg: Res<AppConfig>,
    lay: Res<Layout>,
    sel: Res<Selection>,
    time: Res<Time>,
    assets: Res<CharacterAssets>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    mut existing: Query<(Entity, &ResultBadge, &mut Transform)>,
    mut built: Local<[(usize, Option<chrono::DateTime<chrono::Utc>>); 5]>,
) {
    let view = hub.symbol(&cfg.active_symbol);
    for agent in AgentId::PIPELINE {
        let i = agent.index();
        // Rebuilt only when the day's results change (count or newest).
        let key = (
            view.map_or(0, |v| v.today(agent).take(BADGES_MAX).count()),
            view.and_then(|v| v.today(agent).next()).map(|o| o.at),
        );
        if built[i] == key {
            continue;
        }
        for (e, b, _) in &existing {
            if b.agent == agent {
                commands.entity(e).despawn();
            }
        }
        let outs = view
            .into_iter()
            .flat_map(|v| v.today(agent))
            .take(BADGES_MAX)
            .map(|o| &o.output);
        for (index, o) in outs.enumerate() {
            commands
                .spawn((
                    Mesh3d(assets.badge_mesh.clone()),
                    MeshMaterial3d(assets.badge_mats[badge_kind(o)].clone()),
                    Transform::from_translation(lay.agents[i]),
                    ResultBadge { agent, index },
                ))
                .observe(move |ev: On<PointerClick>, mut sel: ResMut<Selection>| {
                    if ev.button == PointerButton::Primary {
                        sel.agent = Some(agent);
                        sel.output = Some(index);
                    }
                })
                .observe(move |_: On<PointerOver>, mut sel: ResMut<Selection>| {
                    sel.badge = Some((agent, index));
                })
                .observe(move |_: On<PointerOut>, mut sel: ResMut<Selection>| {
                    if sel.badge == Some((agent, index)) {
                        sel.badge = None;
                    }
                });
        }
        built[i] = key;
    }

    let Ok(cam) = cams.single() else {
        return;
    };
    let (right, up) = (cam.right().as_vec3(), cam.up().as_vec3());
    let facing = Quat::from_mat3(&Mat3::from_cols(right, up, right.cross(up)));
    let t = time.elapsed_secs();
    let calm = cfg.scene.reduced_motion;
    for (_, b, mut tf) in &mut existing {
        let i = b.agent.index();
        let r = lay.sizes[i];
        let bob = if calm {
            0.0
        } else {
            (t * 1.3 + b.index as f32 * 0.8).sin() * 0.03
        };
        let pos = lay.agents[i] - right * (r * 0.95 + 0.25)
            + up * (r * 0.55 - b.index as f32 * 0.24 + bob);
        let hot = sel.badge == Some((b.agent, b.index))
            || sel.output == Some(b.index) && sel.agent == Some(b.agent);
        tf.translation = pos;
        tf.rotation = facing * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        tf.scale = Vec3::splat(if hot { 1.35 } else { 1.0 });
    }
}

/// Follow accent changes on the Decision agent's body.
pub fn body_colors(
    theme: Res<Theme>,
    chars: Query<&Character>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    if !theme.is_changed() {
        return;
    }
    for c in &chars {
        let color = body_color(c.agent, theme.pal.accent);
        if let Some(mut m) = mats.get_mut(&c.body_material)
            && m.base_color != color
        {
            m.base_color = color;
            m.emissive = color.to_linear() * 0.03;
        }
    }
}
