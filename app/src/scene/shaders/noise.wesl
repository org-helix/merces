// Small value-noise helpers shared by the sky and the star. Appended to the
// sources that use them (no import: WESL imports go first, functions can come last).

fn hash3(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q += dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn noise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash3(i);
    let b = hash3(i + vec3<f32>(1.0, 0.0, 0.0));
    let c = hash3(i + vec3<f32>(0.0, 1.0, 0.0));
    let d = hash3(i + vec3<f32>(1.0, 1.0, 0.0));
    let e = hash3(i + vec3<f32>(0.0, 0.0, 1.0));
    let f1 = hash3(i + vec3<f32>(1.0, 0.0, 1.0));
    let g = hash3(i + vec3<f32>(0.0, 1.0, 1.0));
    let h = hash3(i + vec3<f32>(1.0, 1.0, 1.0));
    return mix(
        mix(mix(a, b, u.x), mix(c, d, u.x), u.y),
        mix(mix(e, f1, u.x), mix(g, h, u.x), u.y),
        u.z,
    );
}

fn fbm3(p: vec3<f32>) -> f32 {
    var sum = 0.0;
    var amp = 0.5;
    var q = p;
    for (var i = 0; i < 4; i++) {
        sum += amp * noise3(q);
        q = q * 2.03 + vec3<f32>(1.7, 9.2, 3.1);
        amp *= 0.5;
    }
    return sum;
}

// Cellular (Worley) distance to the nearest feature point: granulation cells.
fn cells3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    var best = 8.0;
    for (var x = -1; x <= 1; x++) {
        for (var y = -1; y <= 1; y++) {
            for (var z = -1; z <= 1; z++) {
                let o = vec3<f32>(f32(x), f32(y), f32(z));
                let h = vec3<f32>(hash3(i + o), hash3(i + o + 17.0), hash3(i + o + 41.0));
                let d = o + h - f;
                best = min(best, dot(d, d));
            }
        }
    }
    return sqrt(best);
}
