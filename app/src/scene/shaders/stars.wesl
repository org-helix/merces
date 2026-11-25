// The star field: every star is a camera-facing quad of one shared mesh, drawn
// in a single pass. Size is in pixels, so distant stars stay crisp; the quads
// still sit at different depths, which gives parallax when the camera moves.
//
// Mesh attributes: POSITION = star centre (same for the 4 corners),
// UV_0 = corner in [-1, 1]², COLOR = linear colour × brightness, alpha = size (px).

import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, decompress_vertex},
    mesh_view_bindings::{view, globals},
};

struct StarParams {
    // x: twinkle amount (0 = still), y: overall brightness, z: pixel ratio, w: unused.
    params: vec4<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> star: StarParams;

struct StarOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) corner: vec2<f32>,
    @location(1) color: vec4<f32>,
};

fn hash13(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q += dot(q, q.zyx + 31.32);
    return fract((q.x + q.y) * q.z);
}

@vertex
fn vertex(vertex_in: Vertex) -> StarOut {
    let v = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let centre = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    var clip = view.clip_from_world * centre;

    // Twinkle: two incommensurate sines per star, phase from its position.
    let h = hash13(v.position);
    let t = globals.time;
    let tw = 0.5 * sin(t * (1.3 + 2.1 * h) + h * 40.0) + 0.5 * sin(t * (2.9 + 1.7 * h) + h * 91.0);
    let twinkle = 1.0 + star.params.x * 0.35 * tw;

    let size_px = v.color.a * star.params.z * (0.85 + 0.15 * twinkle);
    let ndc_offset = v.uv * size_px * 2.0 / view.viewport.zw;
    clip = vec4<f32>(clip.xy + ndc_offset * clip.w, clip.zw);

    var out: StarOut;
    out.clip = clip;
    out.corner = v.uv;
    out.color = vec4<f32>(v.color.rgb * twinkle * star.params.y, 1.0);
    return out;
}

@fragment
fn fragment(in: StarOut) -> @location(0) vec4<f32> {
    let d2 = dot(in.corner, in.corner);
    // Soft core plus a faint wide skirt; zero at the quad edge.
    let core = exp(-d2 * 9.0);
    let skirt = exp(-d2 * 2.5) * 0.18;
    let a = (core + skirt) * clamp(1.0 - d2, 0.0, 1.0);
    // Premultiplied blending with alpha 0 = pure addition.
    return vec4<f32>(in.color.rgb * a, 0.0);
}
