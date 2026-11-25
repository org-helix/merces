// Corona and halo of the star: a camera-facing quad centred on it, drawn
// additively. Streamers turn slowly; small prominences rise and fade at the
// limb. `noise.wesl` is appended to this source.
//
// Mesh: a quad with POSITION in [-1, 1]² (z = 0) and UV_0 = the same corner.
// The entity scale is the quad half-size in world units.

import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, decompress_vertex},
    mesh_view_bindings::{view, globals},
};

struct CoronaParams {
    // rgb: inner colour (linear), a: intensity.
    inner: vec4<f32>,
    // rgb: outer colour (linear), a: activity (0..1).
    outer: vec4<f32>,
    // x: motion (0 or 1), y: quad half-size in star radii, zw: unused.
    misc: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> corona: CoronaParams;

struct CoronaOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) corner: vec2<f32>,
};

@vertex
fn vertex(vertex_in: Vertex) -> CoronaOut {
    let v = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let centre = world_from_local * vec4<f32>(0.0, 0.0, 0.0, 1.0);
    let half_size = length(world_from_local[0].xyz);
    let view_pos = view.view_from_world * centre + vec4<f32>(v.uv * half_size, 0.0, 0.0);
    var out: CoronaOut;
    out.clip = view.clip_from_view * view_pos;
    out.corner = v.uv;
    return out;
}

@fragment
fn fragment(in: CoronaOut) -> @location(0) vec4<f32> {
    let t = globals.time * corona.misc.x;
    let q = length(in.corner);
    let r = q * corona.misc.y; // distance from the centre, in star radii
    if (r < 0.98) {
        return vec4<f32>(0.0);
    }
    let a = atan2(in.corner.y, in.corner.x);
    let dir = vec3<f32>(cos(a), sin(a), 0.0);
    let h = r - 1.0;

    // Two-scale halo, then slowly turning streamers.
    let halo = exp(-h * 3.2) * 0.8 + exp(-h * 1.3) * 0.1;
    let streamers = fbm3(dir * 2.2 + vec3<f32>(0.0, 0.0, t * 0.03) + vec3<f32>(h * 0.35));
    let shaped = halo * (0.55 + 0.9 * streamers);

    // Prominences: a few short-lived arcs at the limb.
    let prom_noise = noise3(dir * 5.0 + vec3<f32>(0.0, 0.0, t * 0.07));
    let prom = smoothstep(0.72, 0.86, prom_noise) * exp(-h * 7.0);

    let edge = 1.0 - smoothstep(0.75, 1.0, q);
    let rim = smoothstep(0.98, 1.04, r);
    let col = mix(corona.inner.rgb, corona.outer.rgb, clamp(h / 1.6, 0.0, 1.0));
    let intensity = corona.inner.a * (1.0 + 0.45 * corona.outer.a);
    let rgb = col * (shaped + prom * 1.6) * intensity * edge * rim;
    return vec4<f32>(rgb, 0.0);
}
