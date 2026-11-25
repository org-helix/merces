//! The agents as characters: one shared body (a small rounded pebble with two
//! eyes, stubby arms and feet), and a different hat and accessory per job so
//! each one reads at a glance:
//!
//! | Agent        | Hat                       | Accessory              |
//! |--------------|---------------------------|------------------------|
//! | Research     | explorer helmet           | magnifying glass       |
//! | Chart        | trader's visor            | small chart screen     |
//! | Decision     | top hat with a red band   | gavel                  |
//! | Execution    | pilot cap with a badge    | sight over one eye     |
//! | Assistant    | guide's hat               | speech bubble          |
//!
//! Everything is built in code from primitives: no external asset, original
//! design (see `app/assets/ASSETS.md`). Animation lives in `anim.rs`.
//!
//! Today's results (the former moons) are badges floating beside their agent:
//! one per result, coloured by its outcome, clickable to open the report.

use bevy::prelude::*;
use merces_core::{Action, AgentId, AgentOutput, Bias};

use super::{Layout, Selection};
use crate::config::AppConfig;
use crate::state::Hub;
use crate::theme::{self, Theme};

/// Rest-pose anchors (character space, body centre at the origin).
pub const SHOULDER: Vec3 = Vec3::new(0.40, -0.02, 0.0);
pub const HAND_DROP: f32 = 0.40;
pub const HAT_AT: Vec3 = Vec3::new(0.0, 0.47, 0.0);
pub const EYE: Vec3 = Vec3::new(0.15, 0.16, 0.385);

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

/// The root of a character; its transform is set by the orbit layout.
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

struct Kit {
    sphere: Handle<Mesh>,
    body: Handle<Mesh>,
    arm: Handle<Mesh>,
    cylinder: Handle<Mesh>,
    cone: Handle<Mesh>,
    cube: Handle<Mesh>,
    torus: Handle<Mesh>,
}

fn mat(mats: &mut Assets<StandardMaterial>, c: Color, roughness: f32) -> Handle<StandardMaterial> {
    mats.add(StandardMaterial {
        base_color: c,
        perceptual_roughness: roughness,
        reflectance: 0.3,
        ..default()
    })
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

pub fn spawn_characters(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mats: &mut Assets<StandardMaterial>,
    theme: &Theme,
) {
    let kit = Kit {
        sphere: meshes.add(Sphere::new(1.0).mesh().ico(3).expect("ico sphere")),
        body: meshes.add(Capsule3d::new(0.42, 0.32).mesh().rings(8).latitudes(20)),
        arm: meshes.add(Capsule3d::new(0.085, 0.24)),
        cylinder: meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(40)),
        cone: meshes.add(Cone::new(1.0, 1.0).mesh().resolution(40)),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        torus: meshes.add(
            Torus::new(0.8, 1.0)
                .mesh()
                .minor_resolution(12)
                .major_resolution(40),
        ),
    };
    let ink = mat(mats, theme::bevy_color(theme::INK), 0.35);
    let eye = mats.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.06, 0.05),
        perceptual_roughness: 0.15,
        reflectance: 0.6,
        ..default()
    });
    let alert = glow(mats, theme::bevy_color(theme::DARK.error), 4.0);

    for agent in AgentId::ALL {
        let scale = if agent == AgentId::Assistant {
            0.8
        } else {
            1.0
        };
        let body_material = mats.add(StandardMaterial {
            base_color: body_color(agent, theme.pal.accent),
            perceptual_roughness: 0.55,
            reflectance: 0.35,
            emissive: body_color(agent, theme.pal.accent).to_linear() * 0.03,
            ..default()
        });
        let root = commands
            .spawn((
                Transform::from_scale(Vec3::splat(scale)),
                Visibility::default(),
                Name::new(agent.key()),
            ))
            .id();

        let body = commands
            .spawn((
                Mesh3d(kit.body.clone()),
                MeshMaterial3d(body_material.clone()),
                Transform::IDENTITY,
                Rest(Transform::IDENTITY),
                Part::Body,
                ChildOf(root),
            ))
            .id();

        // Eyes: dark, glossy, slightly oval.
        let eye_at = |x: f32| {
            Transform::from_translation(Vec3::new(x, EYE.y, EYE.z))
                .with_scale(Vec3::new(0.056, 0.074, 0.04))
        };
        let eye_l = commands
            .spawn((
                Mesh3d(kit.sphere.clone()),
                MeshMaterial3d(eye.clone()),
                eye_at(-EYE.x),
                Rest(eye_at(-EYE.x)),
                Part::EyeL,
                ChildOf(body),
            ))
            .id();
        let eye_r = commands
            .spawn((
                Mesh3d(kit.sphere.clone()),
                MeshMaterial3d(eye.clone()),
                eye_at(EYE.x),
                Rest(eye_at(EYE.x)),
                Part::EyeR,
                ChildOf(body),
            ))
            .id();

        // Feet.
        for x in [-0.17, 0.17] {
            commands.spawn((
                Mesh3d(kit.sphere.clone()),
                MeshMaterial3d(body_material.clone()),
                Transform::from_xyz(x, -0.57, 0.06).with_scale(Vec3::new(0.15, 0.08, 0.19)),
                ChildOf(body),
            ));
        }

        // Arms: a pivot at the shoulder, the arm hanging below it.
        let arm = |commands: &mut Commands, side: f32, part: Part| {
            let rest = Transform::from_translation(SHOULDER * Vec3::new(side, 1.0, 1.0))
                .with_rotation(Quat::from_rotation_z(side * 0.2));
            let pivot = commands
                .spawn((rest, Rest(rest), Visibility::default(), part, ChildOf(body)))
                .id();
            commands.spawn((
                Mesh3d(kit.arm.clone()),
                MeshMaterial3d(body_material.clone()),
                Transform::from_xyz(0.0, -HAND_DROP * 0.5, 0.0),
                ChildOf(pivot),
            ));
            pivot
        };
        let arm_l = arm(&mut *commands, -1.0, Part::ArmL);
        let arm_r = arm(&mut *commands, 1.0, Part::ArmR);

        let hat_rest = Transform::from_translation(HAT_AT);
        let hat = commands
            .spawn((
                hat_rest,
                Rest(hat_rest),
                Visibility::default(),
                Part::Hat,
                ChildOf(body),
            ))
            .id();

        // Error signal: a small "!" above the hat, hidden unless in error.
        let alert_rest = Transform::from_xyz(0.0, 1.12, 0.0);
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
            Mesh3d(kit.arm.clone()),
            MeshMaterial3d(alert.clone()),
            Transform::from_xyz(0.0, 0.14, 0.0).with_scale(Vec3::new(0.5, 0.45, 0.5)),
            ChildOf(alert_e),
            Pickable::IGNORE,
        ));
        commands.spawn((
            Mesh3d(kit.sphere.clone()),
            MeshMaterial3d(alert.clone()),
            Transform::from_scale(Vec3::splat(0.05)),
            ChildOf(alert_e),
            Pickable::IGNORE,
        ));

        let bubble = dress(
            &mut *commands,
            mats,
            &kit,
            agent,
            body,
            hat,
            arm_l,
            arm_r,
            &ink,
            theme,
        );

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
}

/// A piece of a hat or accessory.
fn piece(
    commands: &mut Commands,
    mesh: &Handle<Mesh>,
    material: &Handle<StandardMaterial>,
    parent: Entity,
    tf: Transform,
) -> Entity {
    commands
        .spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            tf,
            ChildOf(parent),
        ))
        .id()
}

fn cyl(r: f32, h: f32, y: f32) -> Transform {
    Transform::from_xyz(0.0, y, 0.0).with_scale(Vec3::new(r, h, r))
}

/// Hat and accessory of one agent. Returns the speech bubble (assistant only).
fn dress(
    commands: &mut Commands,
    mats: &mut Assets<StandardMaterial>,
    kit: &Kit,
    agent: AgentId,
    body: Entity,
    hat: Entity,
    arm_l: Entity,
    arm_r: Entity,
    ink: &Handle<StandardMaterial>,
    theme: &Theme,
) -> Option<Entity> {
    let hand = Vec3::new(0.0, -HAND_DROP, 0.02);
    let red = mat(mats, theme::bevy_color(theme.pal.accent), 0.5);
    match agent {
        AgentId::Research => {
            // Explorer helmet: domed crown, wide flat brim, dark band.
            let khaki = mat(mats, Color::srgb_u8(0xB5, 0x9A, 0x73), 0.75);
            piece(
                commands,
                &kit.sphere,
                &khaki,
                hat,
                Transform::from_xyz(0.0, 0.07, 0.0).with_scale(Vec3::new(0.35, 0.27, 0.37)),
            );
            piece(commands, &kit.cylinder, &khaki, hat, cyl(0.54, 0.025, 0.0));
            piece(commands, &kit.cylinder, ink, hat, cyl(0.355, 0.06, 0.04));
            // Magnifying glass in the right hand.
            let brass = mat(mats, Color::srgb_u8(0xC9, 0x9A, 0x4E), 0.3);
            let glass = mats.add(StandardMaterial {
                base_color: Color::srgba(0.85, 0.9, 1.0, 0.35),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.05,
                reflectance: 0.8,
                ..default()
            });
            let lens = Transform::from_translation(hand + Vec3::new(0.0, -0.2, 0.08))
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
            let holder = commands
                .spawn((lens, Visibility::default(), ChildOf(arm_r)))
                .id();
            piece(
                commands,
                &kit.torus,
                &brass,
                holder,
                Transform::from_scale(Vec3::splat(0.14)),
            );
            piece(
                commands,
                &kit.cylinder,
                &glass,
                holder,
                cyl(0.12, 0.01, 0.0),
            );
            piece(
                commands,
                &kit.cylinder,
                ink,
                holder,
                Transform::from_xyz(0.0, 0.0, -0.21)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
                    .with_scale(Vec3::new(0.025, 0.14, 0.025)),
            );
        }
        AgentId::Chart => {
            // Trader's visor: a band round the head and a tinted bill.
            piece(
                commands,
                &kit.torus,
                &red,
                hat,
                Transform::from_xyz(0.0, -0.04, 0.0).with_scale(Vec3::new(0.43, 0.3, 0.43)),
            );
            let visor = mats.add(StandardMaterial {
                base_color: Color::srgba_u8(0x3A, 0x5A, 0x4C, 210),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.3,
                ..default()
            });
            piece(
                commands,
                &kit.cylinder,
                &visor,
                hat,
                Transform::from_xyz(0.0, -0.02, 0.3)
                    .with_rotation(Quat::from_rotation_x(0.28))
                    .with_scale(Vec3::new(0.3, 0.02, 0.2)),
            );
            // A small chart screen in the left hand.
            let screen = commands
                .spawn((
                    Transform::from_translation(hand + Vec3::new(0.05, -0.06, 0.14))
                        .with_rotation(Quat::from_rotation_x(-0.5)),
                    Visibility::default(),
                    ChildOf(arm_l),
                ))
                .id();
            piece(
                commands,
                &kit.cube,
                ink,
                screen,
                Transform::from_scale(Vec3::new(0.34, 0.24, 0.03)),
            );
            let bars = glow(mats, theme::bevy_color(theme::PALE_ORANGE), 1.6);
            let up = glow(mats, theme::bevy_color(theme::DARK.success), 1.4);
            for (i, h) in [0.06, 0.1, 0.08, 0.15].into_iter().enumerate() {
                let m = if i == 3 { &up } else { &bars };
                piece(
                    commands,
                    &kit.cube,
                    m,
                    screen,
                    Transform::from_xyz(-0.105 + i as f32 * 0.07, -0.08 + h * 0.5, 0.017)
                        .with_scale(Vec3::new(0.04, h, 0.006)),
                );
            }
        }
        AgentId::Decision => {
            // Top hat with a red band.
            piece(commands, &kit.cylinder, ink, hat, cyl(0.25, 0.44, 0.25));
            piece(commands, &kit.cylinder, ink, hat, cyl(0.4, 0.03, 0.03));
            piece(commands, &kit.cylinder, &red, hat, cyl(0.255, 0.07, 0.08));
            // Gavel in the right hand.
            let wood = mat(mats, Color::srgb_u8(0x7A, 0x4E, 0x30), 0.6);
            let gavel = commands
                .spawn((
                    Transform::from_translation(hand + Vec3::new(0.0, -0.02, 0.05))
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                    Visibility::default(),
                    ChildOf(arm_r),
                ))
                .id();
            piece(commands, &kit.cylinder, &wood, gavel, cyl(0.022, 0.3, 0.12));
            piece(
                commands,
                &kit.cylinder,
                &wood,
                gavel,
                Transform::from_xyz(0.0, 0.28, 0.0)
                    .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
                    .with_scale(Vec3::new(0.065, 0.2, 0.065)),
            );
        }
        AgentId::Execution => {
            // Pilot cap: flat wide crown, dark band, bill, gold badge.
            let navy = mat(mats, Color::srgb_u8(0x2A, 0x2B, 0x36), 0.5);
            piece(commands, &kit.cylinder, &navy, hat, cyl(0.37, 0.12, 0.1));
            piece(commands, &kit.cylinder, ink, hat, cyl(0.345, 0.09, 0.0));
            piece(
                commands,
                &kit.cylinder,
                ink,
                hat,
                Transform::from_xyz(0.0, -0.03, 0.33)
                    .with_rotation(Quat::from_rotation_x(0.3))
                    .with_scale(Vec3::new(0.24, 0.02, 0.16)),
            );
            let gold = glow(mats, Color::srgb_u8(0xE8, 0xB8, 0x5A), 0.6);
            piece(
                commands,
                &kit.sphere,
                &gold,
                hat,
                Transform::from_xyz(0.0, 0.1, 0.36).with_scale(Vec3::new(0.05, 0.05, 0.02)),
            );
            // A sight over the right eye.
            let sight = commands
                .spawn((
                    Transform::from_xyz(EYE.x, EYE.y, EYE.z + 0.03)
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                    Visibility::default(),
                    ChildOf(body),
                ))
                .id();
            piece(
                commands,
                &kit.torus,
                &gold,
                sight,
                Transform::from_scale(Vec3::splat(0.1)),
            );
            let lens = glow(mats, theme::bevy_color(theme.pal.accent), 0.8);
            piece(
                commands,
                &kit.cube,
                &lens,
                sight,
                Transform::from_scale(Vec3::new(0.15, 0.004, 0.012)),
            );
            piece(
                commands,
                &kit.cube,
                &lens,
                sight,
                Transform::from_scale(Vec3::new(0.012, 0.004, 0.15)),
            );
        }
        AgentId::Assistant => {
            // Guide's hat: wide brim, pointed crown, rust band.
            let felt = mat(mats, Color::srgb_u8(0x6E, 0x58, 0x45), 0.85);
            piece(commands, &kit.cylinder, &felt, hat, cyl(0.56, 0.025, 0.0));
            piece(
                commands,
                &kit.cone,
                &felt,
                hat,
                Transform::from_xyz(0.0, 0.19, 0.0).with_scale(Vec3::new(0.3, 0.38, 0.3)),
            );
            let band = mat(mats, theme::bevy_color(theme::RUST), 0.6);
            piece(commands, &kit.cylinder, &band, hat, cyl(0.285, 0.05, 0.035));
            // Speech bubble beside the head.
            let rest = Transform::from_xyz(0.62, 0.78, 0.0);
            let bubble = commands
                .spawn((
                    rest,
                    Rest(rest),
                    Visibility::default(),
                    Part::Bubble,
                    ChildOf(body),
                ))
                .id();
            let paper = glow(mats, theme::bevy_color(theme::AMBER), 0.25);
            piece(
                commands,
                &kit.sphere,
                &paper,
                bubble,
                Transform::from_scale(Vec3::new(0.27, 0.19, 0.08)),
            );
            piece(
                commands,
                &kit.cone,
                &paper,
                bubble,
                Transform::from_xyz(-0.14, -0.17, 0.0)
                    .with_rotation(Quat::from_rotation_z(2.5))
                    .with_scale(Vec3::new(0.06, 0.14, 0.04)),
            );
            for x in [-0.09, 0.0, 0.09] {
                piece(
                    commands,
                    &kit.sphere,
                    ink,
                    bubble,
                    Transform::from_xyz(x, 0.0, 0.07).with_scale(Vec3::splat(0.028)),
                );
            }
            return Some(bubble);
        }
    }
    None
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
        badge_mesh: meshes.add(Cylinder::new(0.095, 0.03).mesh().resolution(28)),
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
