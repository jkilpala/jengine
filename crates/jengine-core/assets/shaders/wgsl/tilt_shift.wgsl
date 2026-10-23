// Tilt-shift and blur - a disc blur whose radius varies down the screen.
//
// A tilt-shift photograph is sharp in one band and soft above and below it,
// which the eye reads as a lens focused very close: the town becomes a
// model of a town. Each pixel's radius runs from `min_px` inside the band
// to `max_px` past it, eased over `falloff`; with `min_px == max_px` it is a
// plain blur of that radius (the `blur` effect - a menu over a softened
// world). The kernel is a golden-angle spiral of 32 taps, even over the
// disc, which reads as a lens's bokeh rather than a box. With `mode` 1 the
// sharp region is a rectangle instead of a band (`blur` with a focus
// rectangle: the room you are in, the world outside it soft).
//
// Uniforms (48 bytes):
//   focus_v  - the band's centre, in screen v (0 top, 1 bottom).
//   band     - half-height of the sharp band, in v.
//   falloff  - how far past the band (in v) or the rectangle (in pixels)
//              the blur takes to reach `max_px`.
//   max_px   - the blur radius far from the band, pixels.
//   min_px   - the radius inside the band (0 for tilt-shift).
//   mode     - 0 the band, 1 the rectangle.
//   texel    - one pixel in uv.
//   rect     - the rectangle's centre (xy) and half size (zw), in uv.

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       tex_coord:     vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    let x = f32((i32(vertex_index) & 1) << 2) - 1.0;
    let y = f32((i32(vertex_index) & 2) << 1) - 1.0;
    var out: VsOut;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.tex_coord     = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

struct TiltShiftUniforms {
    focus_v: f32,
    band:    f32,
    falloff: f32,
    max_px:  f32,
    min_px:  f32,
    mode:    f32,
    texel:   vec2<f32>,
    rect:    vec4<f32>,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: TiltShiftUniforms;

const TAPS: i32 = 32;
const GOLDEN: f32 = 2.39996323;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let base = textureSample(src_texture, src_sampler, in.tex_coord);
    var t: f32;
    if (u.mode > 0.5) {
        // How far outside the rectangle, in pixels.
        let px = (abs(in.tex_coord - u.rect.xy) - u.rect.zw) / u.texel;
        t = smoothstep(0.0, max(u.falloff, 1e-4), length(max(px, vec2<f32>(0.0))));
    } else {
        let d = abs(in.tex_coord.y - u.focus_v);
        t = smoothstep(u.band, u.band + max(u.falloff, 1e-4), d);
    }
    let radius = mix(u.min_px, u.max_px, t);
    if (radius < 0.5) {
        return base;
    }
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < TAPS; i = i + 1) {
        let f = (f32(i) + 0.5) / f32(TAPS);
        let r = sqrt(f) * radius;
        let a = f32(i) * GOLDEN;
        let o = vec2<f32>(cos(a), sin(a)) * r * u.texel;
        sum = sum + textureSampleLevel(src_texture, src_sampler, in.tex_coord + o, 0.0).rgb;
    }
    return vec4<f32>(sum / f32(TAPS), base.a);
}
