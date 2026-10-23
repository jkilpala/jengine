// Color grading — brightness / contrast / saturation + tint multiply.
//
// Uniforms (32 bytes):
//   brightness — multiplicative, 1.0 = identity.
//   contrast   — about middle grey, in log space; 1.0 = identity.
//                Values > 1 push extremes, values < 1 flatten toward grey.
//
// The shader sees **linear** colour: every target is an sRGB format (or an
// sRGB view of one), so a sample is decoded before it gets here. Middle
// grey in linear is 0.18, not 0.5 - linear 0.5 is a light grey, about 73%
// in sRGB. Contrast used to pivot at 0.5 in linear, which put every value
// below linear ~0.06 (sRGB ~27%) under zero at contrast 1.14: a dark scene
// went black outside its lights, and the channel that reached zero first
// was lost, so a dim warm floor turned red. Log-space contrast about 0.18
// keeps black at black, never goes negative, and is what Unity's and
// Godot's grading do.
//   saturation — 0 = greyscale, 1 = identity, > 1 = oversaturated.
//   _pad
//   tint(vec4) — multiplied (rgb) over the result. Alpha unused.

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

struct ColorGradeUniforms {
    brightness: f32,
    contrast:   f32,
    saturation: f32,
    _pad:       f32,
    tint:       vec4<f32>,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: ColorGradeUniforms;

const LUMA_WEIGHTS: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
const MID_GREY: f32 = 0.18;
// pow(0, c) is undefined in WGSL; a floor far below one 8-bit step keeps
// black at black without asking the GPU for it.
const TINY: f32 = 1e-6;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let base = textureSample(src_texture, src_sampler, in.tex_coord);
    var col  = base.rgb;

    // Brightness (multiplicative).
    col = col * u.brightness;

    // Contrast about middle grey, in log space.
    col = MID_GREY * pow(max(col, vec3<f32>(TINY)) / MID_GREY, vec3<f32>(u.contrast));

    // Saturation around luma.
    let lum = dot(col, LUMA_WEIGHTS);
    col = mix(vec3<f32>(lum), col, u.saturation);

    // Tint multiply (with alpha as a strength fade).
    let tinted = mix(col, col * u.tint.rgb, clamp(u.tint.a, 0.0, 1.0));

    // Saturation above 1 pushes a channel below its luma, and so below
    // zero for a strong colour; a negative channel is not a colour.
    return vec4<f32>(max(tinted, vec3<f32>(0.0)), base.a);
}
