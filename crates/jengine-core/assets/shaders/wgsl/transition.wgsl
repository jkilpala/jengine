// Scene transitions (`renderer::transition`, `Scene.transition`): the last
// frame of the old scene, held in a texture, drawn over the new scene's
// frame - as much of it as the style says is left at `progress` (0 all old,
// 1 all new). Drawn by `WgpuFrame::present` after everything else, into the
// display view, alpha blended.
//
//   0 fade      the old frame fades out;
//   1 iris      a circle opens from the middle onto the new scene;
//   2 diamond   diamonds open across the screen, left to right;
//   3 slide     the old frame slides off to the left;
//   4 wipe      a soft edge sweeps from left to right;
//   5 dissolve  the old frame burns away where the mask is darkest first
//               (a texture's red, stretched over the screen, or noise).

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       uv:            vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    let x = f32((i32(vertex_index) & 1) << 2) - 1.0;
    let y = f32((i32(vertex_index) & 2) << 1) - 1.0;
    var out: VsOut;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

struct TransitionU {
    // x style, y progress, z aspect (width / height), w 1 when the mask is a
    // texture.
    p: vec4<f32>,
}

@group(0) @binding(0) var old_frame: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var mask_tex: texture_2d<f32>;
@group(0) @binding(3) var<uniform> u: TransitionU;

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Smooth value noise, two octaves: the dissolve's mask without a texture.
fn noise(p: vec2<f32>) -> f32 {
    var v = 0.0;
    var a = 0.65;
    var q = p;
    for (var o = 0; o < 2; o = o + 1) {
        let i = floor(q);
        let f = fract(q);
        let s = f * f * (3.0 - 2.0 * f);
        let n = mix(mix(hash12(i), hash12(i + vec2<f32>(1.0, 0.0)), s.x),
                    mix(hash12(i + vec2<f32>(0.0, 1.0)), hash12(i + vec2<f32>(1.0, 1.0)), s.x), s.y);
        v = v + n * a;
        q = q * 2.7;
        a = a * 0.35;
    }
    return v / 0.8775;
}

// How much of the old frame is left at `uv` (0..1), and where to read it.
fn keep(uv: vec2<f32>) -> vec3<f32> {
    let style = i32(u.p.x + 0.5);
    let k = clamp(u.p.y, 0.0, 1.0);
    let aspect = u.p.z;
    if (style == 1) {
        let d = length((uv - vec2<f32>(0.5)) * vec2<f32>(aspect, 1.0));
        let r = k * length(vec2<f32>(aspect, 1.0)) * 0.51;
        return vec3<f32>(smoothstep(r - 0.004, r + 0.004, d), uv);
    }
    if (style == 2) {
        let cells = 9.0;
        let c = uv * vec2<f32>(cells * aspect, cells);
        let f = fract(c) - vec2<f32>(0.5);
        let d = abs(f.x) + abs(f.y);
        // Each column opens a little after the one on its left.
        let local = clamp(k * 1.6 - (floor(c.x) / (cells * aspect)) * 0.6, 0.0, 1.0);
        let r = local * 1.1;
        return vec3<f32>(smoothstep(r - 0.03, r, d), uv);
    }
    if (style == 3) {
        let at = uv + vec2<f32>(k, 0.0);
        return vec3<f32>(select(0.0, 1.0, at.x <= 1.0), at);
    }
    if (style == 4) {
        let e = k * 1.1 - 0.05;
        return vec3<f32>(smoothstep(e - 0.05, e + 0.05, uv.x), uv);
    }
    if (style == 5) {
        var m = noise(uv * vec2<f32>(aspect, 1.0) * 6.0);
        if (u.p.w > 0.5) {
            m = textureSampleLevel(mask_tex, samp, uv, 0.0).r;
        }
        let e = k * 1.1;
        return vec3<f32>(smoothstep(e - 0.1, e, m), uv);
    }
    return vec3<f32>(1.0 - k, uv);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let kp = keep(in.uv);
    let c = textureSampleLevel(old_frame, samp, kp.yz, 0.0).rgb;
    return vec4<f32>(c, kp.x);
}
