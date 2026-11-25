//! The backdrop of the network: a star field drawn in one pass (one mesh, one
//! draw) and a faint Milky Way band. All the animation runs on the GPU from the
//! global time: no material is touched per frame.
//!
//! The 3D scene keeps a dark sky in both themes: the panels follow the light
//! theme, the night sky stays night.

use std::f32::consts::TAU;

use bevy::asset::{RenderAssetUsages, uuid_handle};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Face, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::{Shader, ShaderRef};
use bevy::window::PrimaryWindow;
use merces_agents::demo::Rng;

use crate::config::{AppConfig, Quality};
use crate::theme;

const STARS_SHADER: Handle<Shader> = uuid_handle!("6f1c7a52-3b0e-4d39-9a51-0c6a2f0d8e11");
const SKY_SHADER: Handle<Shader> = uuid_handle!("6f1c7a52-3b0e-4d39-9a51-0c6a2f0d8e12");

const NOISE: &str = include_str!("shaders/noise.wesl");

const SKY_RADIUS: f32 = 240.0;

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        let mut shaders = app.world_mut().resource_mut::<Assets<Shader>>();
        let with_noise = |src: &str| format!("{src}\n{NOISE}");
        for (handle, source, path) in [
            (
                STARS_SHADER,
                include_str!("shaders/stars.wesl").to_owned(),
                "merces/stars.wesl",
            ),
            (
                SKY_SHADER,
                with_noise(include_str!("shaders/sky.wesl")),
                "merces/sky.wesl",
            ),
        ] {
            let _ = shaders.insert(handle.id(), Shader::from_wesl(source, path));
        }
        app.add_plugins((
            MaterialPlugin::<StarMaterial>::default(),
            MaterialPlugin::<SkyMaterial>::default(),
        ))
        .add_systems(Startup, spawn)
        .add_systems(Update, (star_field, star_settings));
    }
}

// ── Materials ──────────────────────────────────────────────────────────────

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct StarMaterial {
    /// x: twinkle amount, y: brightness, z: pixels per logical pixel.
    #[uniform(0)]
    params: Vec4,
}

impl Material for StarMaterial {
    fn vertex_shader() -> ShaderRef {
        STARS_SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        STARS_SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SkyMaterial {
    #[uniform(0)]
    band: LinearRgba,
    #[uniform(0)]
    nebula: LinearRgba,
}

impl Material for SkyMaterial {
    fn fragment_shader() -> ShaderRef {
        SKY_SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Seen from inside.
        descriptor.primitive.cull_mode = Some(Face::Front);
        Ok(())
    }
}

// ── Entities ───────────────────────────────────────────────────────────────

#[derive(Component)]
struct StarField;

#[derive(Resource)]
struct SkyHandles {
    stars: Handle<StarMaterial>,
}

fn lin(c: bevy_egui::egui::Color32) -> LinearRgba {
    theme::bevy_color(c).to_linear()
}

fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut stars: ResMut<Assets<StarMaterial>>,
    mut skies: ResMut<Assets<SkyMaterial>>,
    cfg: Res<AppConfig>,
) {
    let motion = if cfg.scene.reduced_motion { 0.0 } else { 1.0 };

    // Milky Way band and nebula, far behind everything.
    let band = lin(theme::BLUSH).mix(&lin(theme::DUSTY), 0.5);
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(SKY_RADIUS).mesh().uv(64, 32))),
        MeshMaterial3d(skies.add(SkyMaterial {
            band: band.with_alpha(0.055),
            nebula: lin(theme::RUST).with_alpha(0.03),
        })),
        Pickable::IGNORE,
        NoFrustumCulling,
        Name::new("sky"),
    ));

    let star_mat = stars.add(StarMaterial {
        params: Vec4::new(motion, 1.0, 1.0, 0.0),
    });

    commands.insert_resource(SkyHandles { stars: star_mat });
}

pub fn star_count(q: Quality) -> u32 {
    match q {
        Quality::Low => 2_500,
        Quality::Medium => 5_000,
        Quality::High => 9_000,
    }
}

/// Approximate black-body colour (sRGB, 0..1) for a temperature in kelvin.
fn blackbody(k: f32) -> [f32; 3] {
    let t = k / 100.0;
    let r = if t <= 66.0 {
        1.0
    } else {
        (1.292_936 * (t - 60.0).powf(-0.133_204_76)).clamp(0.0, 1.0)
    };
    let g = if t <= 66.0 {
        (0.390_081_58 * t.ln() - 0.631_841_4).clamp(0.0, 1.0)
    } else {
        (1.129_890_9 * (t - 60.0).powf(-0.075_514_85)).clamp(0.0, 1.0)
    };
    let b = if t >= 66.0 {
        1.0
    } else if t <= 19.0 {
        0.0
    } else {
        (0.543_206_8 * (t - 10.0).ln() - 1.196_254_1).clamp(0.0, 1.0)
    };
    [r, g, b]
}

/// One mesh for the whole star field: 4 vertices per star (see `stars.wesl`).
pub fn star_mesh(count: u32) -> Mesh {
    let mut rng = Rng::new(0x5eed_57a5);
    let mut f = || rng.f64() as f32;
    let n = count as usize;
    let mut pos = Vec::with_capacity(n * 4);
    let mut uv = Vec::with_capacity(n * 4);
    let mut col = Vec::with_capacity(n * 4);
    let mut idx = Vec::with_capacity(n * 6);
    let galactic = Vec3::new(0.32, 1.0, 0.48).normalize();
    let gu = galactic.any_orthonormal_vector();
    let gw = galactic.cross(gu);
    for i in 0..n {
        // 40 % of the stars crowd into the Milky Way band.
        let dir = if f() < 0.4 {
            let a = f() * TAU;
            let lat = (f() + f() + f() - 1.5) * 0.12;
            (gu * a.cos() + gw * a.sin() + galactic * lat).normalize()
        } else {
            let z = f() * 2.0 - 1.0;
            let a = f() * TAU;
            let s = (1.0 - z * z).sqrt();
            Vec3::new(s * a.cos(), z, s * a.sin())
        };
        let r = 60.0 + 110.0 * f();
        let centre = dir * r;
        // Magnitudes: most stars faint, a few bright.
        let m = f();
        let brightness = 0.05 + 2.6 * m.powi(8);
        let size = 2.2 + 4.5 * m.powi(6);
        // Temperature: mostly orange to white, some blue-white.
        let pick = f();
        let kelvin = if pick < 0.7 {
            3_600.0 + 2_600.0 * f()
        } else if pick < 0.92 {
            6_200.0 + 2_800.0 * f()
        } else {
            9_000.0 + 7_000.0 * f()
        };
        let [cr, cg, cb] = blackbody(kelvin);
        // Sober: pull 35 % toward white, then linear.
        let c = LinearRgba::from(Color::srgb(
            cr * 0.65 + 0.35,
            cg * 0.65 + 0.35,
            cb * 0.65 + 0.35,
        ));
        let color = [
            c.red * brightness,
            c.green * brightness,
            c.blue * brightness,
            size,
        ];
        let base = (i * 4) as u32;
        for corner in [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
            pos.push(centre.to_array());
            uv.push(corner);
            col.push(color);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
    .with_inserted_indices(Indices::U32(idx))
}

/// (Re)build the star field when the quality changes.
fn star_field(
    mut commands: Commands,
    cfg: Res<AppConfig>,
    handles: Res<SkyHandles>,
    mut meshes: ResMut<Assets<Mesh>>,
    existing: Query<Entity, With<StarField>>,
    mut built: Local<Option<Quality>>,
) {
    let q = cfg.scene.quality;
    if *built == Some(q) {
        return;
    }
    *built = Some(q);
    for e in &existing {
        commands.entity(e).despawn();
    }
    commands.spawn((
        Mesh3d(meshes.add(star_mesh(star_count(q)))),
        MeshMaterial3d(handles.stars.clone()),
        Pickable::IGNORE,
        NoFrustumCulling,
        StarField,
        Name::new("stars"),
    ));
}

/// Twinkle, star size on HiDPI screens and the star's motion follow the settings.
fn star_settings(
    cfg: Res<AppConfig>,
    windows: Query<&Window, With<PrimaryWindow>>,
    handles: Res<SkyHandles>,
    mut stars: ResMut<Assets<StarMaterial>>,
    mut last: Local<Option<(bool, f32)>>,
) {
    let scale = windows.single().map(|w| w.scale_factor()).unwrap_or(1.0);
    let key = (cfg.scene.reduced_motion, scale);
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    let motion = if cfg.scene.reduced_motion { 0.0 } else { 1.0 };
    if let Some(mut m) = stars.get_mut(&handles.stars) {
        m.params = Vec4::new(motion, 1.0, scale, 0.0);
    }
}
