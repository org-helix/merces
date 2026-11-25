// The faint glow behind the stars: a Milky Way band with dust lanes and a very
// subtle warm nebula, on a large inside-out sphere. Added on top of the ink sky.
// `noise.wesl` is appended to this source when the shader is built.

import bevy_pbr::render::forward_io::VertexOutput;

struct SkyParams {
    // rgb: band tint (linear), a: intensity.
    band: vec4<f32>,
    // rgb: nebula tint (linear), a: intensity.
    nebula: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> sky: SkyParams;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.world_position.xyz);
    // The galactic plane, tilted against the orbits.
    let n = normalize(vec3<f32>(0.32, 1.0, 0.48));
    let lat = dot(dir, n);
    let band = exp(-lat * lat / 0.028);
    let clumps = fbm3(dir * 5.0);
    let lanes = smoothstep(0.42, 0.62, fbm3(dir * 9.0 + vec3<f32>(7.1, 2.3, 4.8)));
    let core = band * (0.45 + 0.9 * clumps) * (1.0 - 0.75 * lanes * exp(-lat * lat / 0.006));

    let neb = smoothstep(0.52, 0.85, fbm3(dir * 1.6 + vec3<f32>(3.0, 9.0, 1.0)));

    let rgb = sky.band.rgb * core * sky.band.a + sky.nebula.rgb * neb * sky.nebula.a;
    return vec4<f32>(rgb, 0.0);
}
