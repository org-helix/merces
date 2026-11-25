// The star's surface: granulation cells, slow differential rotation, a few
// spots, limb darkening. Emissive only; the point light at its centre is what
// lights the agents. `noise.wesl` is appended to this source.

import bevy_pbr::render::{
    forward_io::VertexOutput,
    mesh_view_bindings::{view, globals},
};

struct SunParams {
    // rgb: hot core colour (linear), a: intensity.
    hot: vec4<f32>,
    // rgb: cooler limb / spot colour (linear), a: activity (0..1).
    cool: vec4<f32>,
    // x: motion (0 = frozen, 1 = animated), yzw: unused.
    misc: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> sun: SunParams;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let v = normalize(view.world_position - in.world_position.xyz);
    let mu = clamp(dot(n, v), 0.0, 1.0);
    let t = globals.time * sun.misc.x;

    // Differential rotation: the equator turns faster than the poles.
    let lat = n.y;
    let ang = t * (0.045 + 0.02 * (1.0 - lat * lat));
    let c = cos(ang);
    let s = sin(ang);
    let p = vec3<f32>(c * n.x - s * n.z, n.y, s * n.x + c * n.z);

    // Granulation: bright cells, darker lanes, slowly boiling.
    let cell = cells3(p * 13.0 + vec3<f32>(0.0, t * 0.12, t * 0.05));
    let lanes = smoothstep(0.2, 0.95, cell);
    let fine = fbm3(p * 34.0 + vec3<f32>(t * 0.25));

    // Spots, kept to the low latitudes.
    let spot_noise = fbm3(p * 2.4 + vec3<f32>(5.0, 1.0, 3.0));
    let spot = smoothstep(0.64, 0.73, spot_noise) * (1.0 - smoothstep(0.25, 0.6, abs(lat)));

    let limb = 0.32 + 0.68 * pow(mu, 0.5);
    var col = mix(sun.hot.rgb, sun.cool.rgb, 0.3 * lanes + 0.35 * (1.0 - limb));
    col *= (1.0 - 0.25 * lanes) * (0.92 + 0.16 * fine);
    col *= limb;
    col = mix(col, sun.cool.rgb * 0.18, spot * 0.85);

    let intensity = sun.hot.a * (1.0 + 0.3 * sun.cool.a);
    return vec4<f32>(col * intensity, 1.0);
}
