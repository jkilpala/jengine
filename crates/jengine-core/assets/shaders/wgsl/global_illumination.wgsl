// 2D global illumination by radiance cascades (renderer::backend::wgpu::gi).
//
// Light that bounces: the lit surfaces of the occluders - the scene's colour
// times the direct light on it - shine again into the room, and every pixel
// gathers what reaches it round the occluders in between. Five kinds of pass,
// all fragment shaders (so WebGL2 runs them), at a fraction of the frame's
// resolution:
//   fs_occluder  the occluders' outlines, thick, into the seed texture;
//   fs_source    each outline texel's light: scene x (light - ambient);
//   fs_jfa       jump flooding: each texel learns its nearest outline texel,
//                which is a distance field to sphere-trace by;
//   fs_cascade   one cascade: rays from probes over its interval, merged
//                with the cascade above (4x the directions, 2x the spacing);
//   fs_apply     cascade 0's probes, averaged and interpolated, added to the
//                light buffer - which the composite multiplies by the scene,
//                so a bounce tints what it lands on.

struct G {
    vp:      mat4x4<f32>,  // world to clip, for the occluders
    size:    vec4<f32>,    // gi width, height; frame width, height
    p:       vec4<f32>,    // cascade, cascade count, jump, strength
    ambient: vec4<f32>,    // the ambient light taken off the source; w: frame pixels per gi pixel
};

@group(0) @binding(0) var<uniform> g: G;
@group(1) @binding(0) var t0: texture_2d<f32>;
@group(1) @binding(1) var t1: texture_2d<f32>;
@group(1) @binding(2) var t2: texture_2d<f32>;

const TAU: f32 = 6.28318530718;

struct FullOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> FullOut {
    let x = f32((i32(vi) & 1) << 2) - 1.0;
    let y = f32((i32(vi) & 2) << 1) - 1.0;
    var out: FullOut;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    return out;
}

// ── the occluders' outlines ──────────────────────────────────────────────

@vertex
fn vs_occluder(@location(0) world: vec2<f32>) -> FullOut {
    var out: FullOut;
    out.pos = g.vp * vec4<f32>(world, 0.0, 1.0);
    return out;
}

@fragment
fn fs_occluder(in: FullOut) -> @location(0) vec4<f32> {
    // A seed: this texel's own centre, and "occupied".
    return vec4<f32>(floor(in.pos.xy) + 0.5, 0.0, 1.0);
}

fn gi_size() -> vec2<i32> {
    return vec2<i32>(i32(g.size.x), i32(g.size.y));
}

fn inside(t: vec2<i32>) -> bool {
    let s = gi_size();
    return t.x >= 0 && t.y >= 0 && t.x < s.x && t.y < s.y;
}

// ── the source ───────────────────────────────────────────────────────────

@fragment
fn fs_source(in: FullOut) -> @location(0) vec4<f32> {
    let t = vec2<i32>(floor(in.pos.xy));
    let occupied = textureLoad(t2, t, 0).a;
    if (occupied < 0.5) {
        return vec4<f32>(0.0);
    }
    let frame = vec2<i32>(i32(g.size.z), i32(g.size.w));
    let f = clamp(vec2<i32>((vec2<f32>(t) + 0.5) * g.ambient.w), vec2<i32>(0), frame - 1);
    let scene = textureLoad(t0, f, 0);
    let light = textureLoad(t1, f, 0);
    return vec4<f32>(scene.rgb * max(light.rgb - g.ambient.rgb, vec3<f32>(0.0)), 1.0);
}

// ── jump flooding ────────────────────────────────────────────────────────

@fragment
fn fs_jfa(in: FullOut) -> @location(0) vec4<f32> {
    let t = vec2<i32>(floor(in.pos.xy));
    let p = vec2<f32>(t) + 0.5;
    let k = i32(g.p.z);
    var best = textureLoad(t0, t, 0);
    var bd = 1e9;
    if (best.a > 0.5) {
        bd = distance(p, best.xy);
    }
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            let q = t + vec2<i32>(dx, dy) * k;
            if ((dx != 0 || dy != 0) && inside(q)) {
                let s = textureLoad(t0, q, 0);
                if (s.a > 0.5) {
                    let d = distance(p, s.xy);
                    if (d < bd) {
                        bd = d;
                        best = s;
                    }
                }
            }
        }
    }
    return best;
}

// ── a cascade ────────────────────────────────────────────────────────────

// The nearest outline texel to `p` (the jump-flooded seed), or none.
fn seed_at(p: vec2<f32>) -> vec4<f32> {
    let t = vec2<i32>(floor(p));
    if (!inside(t)) {
        return vec4<f32>(0.0);
    }
    return textureLoad(t1, t, 0);
}

// What one probe sees one way over [start, end): the light of the outline it
// hits (and nothing past it), or nothing and a clear view on.
fn march(origin: vec2<f32>, dir: vec2<f32>, start: f32, end: f32) -> vec4<f32> {
    var t = start;
    for (var n = 0; n < 48; n = n + 1) {
        if (t >= end) {
            break;
        }
        let p = origin + dir * t;
        if (p.x < 0.0 || p.y < 0.0 || p.x >= g.size.x || p.y >= g.size.y) {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0);
        }
        let s = seed_at(p);
        var d = 1e9;
        if (s.a > 0.5) {
            d = distance(p, s.xy);
        }
        if (d < 0.75) {
            let light = textureLoad(t0, vec2<i32>(floor(s.xy)), 0).rgb;
            return vec4<f32>(light, 0.0);
        }
        t = t + max(d - 0.5, 0.5);
    }
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

// The cascade above, averaged over the four directions that split
// `dir_index`, at probe `probe` of that cascade.
fn upper_probe(probe: vec2<i32>, dir_index: i32, block: i32) -> vec4<f32> {
    let s = gi_size();
    let spacing = i32(1u << u32(i32(g.p.x) + 1));
    let probes = (s + spacing - 1) / spacing;
    let pr = clamp(probe, vec2<i32>(0), probes - 1);
    var sum = vec4<f32>(0.0);
    for (var k = 0; k < 4; k = k + 1) {
        let d = dir_index * 4 + k;
        let texel = pr * block + vec2<i32>(d % block, d / block);
        sum = sum + textureLoad(t2, texel, 0);
    }
    return sum * 0.25;
}

@fragment
fn fs_cascade(in: FullOut) -> @location(0) vec4<f32> {
    let c = i32(g.p.x);
    let count = i32(g.p.y);
    let block = i32(2u << u32(c));                  // 2^(c+1) texels a probe, each way
    let t = vec2<i32>(floor(in.pos.xy));
    let probe = t / block;
    let local = t - probe * block;
    let dir_index = local.x + local.y * block;
    let dirs = f32(block * block);
    let spacing = f32(1u << u32(c));
    let origin = (vec2<f32>(probe) + 0.5) * spacing;
    let a = (f32(dir_index) + 0.5) * TAU / dirs;
    let dir = vec2<f32>(cos(a), sin(a));
    // Intervals grow four times a cascade: [ (4^c - 1) / 3, (4^(c+1) - 1) / 3 ).
    let start = (pow(4.0, f32(c)) - 1.0) / 3.0;
    let end = (pow(4.0, f32(c + 1)) - 1.0) / 3.0;
    var r = march(origin, dir, start, end);
    if (r.a > 0.0 && c + 1 < count) {
        // Seen clear: what lies beyond is the cascade above's, between its
        // four nearest probes.
        let ub = block * 2;
        let us = spacing * 2.0;
        let q = origin / us - 0.5;
        let b = vec2<i32>(floor(q));
        let f = q - floor(q);
        let p00 = upper_probe(b, dir_index, ub);
        let p10 = upper_probe(b + vec2<i32>(1, 0), dir_index, ub);
        let p01 = upper_probe(b + vec2<i32>(0, 1), dir_index, ub);
        let p11 = upper_probe(b + vec2<i32>(1, 1), dir_index, ub);
        let above = mix(mix(p00, p10, f.x), mix(p01, p11, f.x), f.y);
        r = vec4<f32>(r.rgb + above.rgb * r.a, r.a * above.a);
    }
    return r;
}

// ── apply ────────────────────────────────────────────────────────────────

fn probe0(probe: vec2<i32>) -> vec3<f32> {
    let pr = clamp(probe, vec2<i32>(0), gi_size() - 1);
    let base = pr * 2;
    let sum = textureLoad(t0, base, 0).rgb + textureLoad(t0, base + vec2<i32>(1, 0), 0).rgb
        + textureLoad(t0, base + vec2<i32>(0, 1), 0).rgb + textureLoad(t0, base + vec2<i32>(1, 1), 0).rgb;
    return sum * 0.25;
}

@fragment
fn fs_apply(in: FullOut) -> @location(0) vec4<f32> {
    let q = in.pos.xy / g.ambient.w - 0.5;
    let b = vec2<i32>(floor(q));
    let f = q - floor(q);
    let i00 = probe0(b);
    let i10 = probe0(b + vec2<i32>(1, 0));
    let i01 = probe0(b + vec2<i32>(0, 1));
    let i11 = probe0(b + vec2<i32>(1, 1));
    let irradiance = mix(mix(i00, i10, f.x), mix(i01, i11, f.x), f.y);
    return vec4<f32>(irradiance * g.p.w, 0.0);
}
