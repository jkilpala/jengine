// Light shafts - light scattered through the air, streaming from a source.
//
// Screen-space light scattering (GPU Gems 3, ch. 13): every pixel marches
// toward the source's place on the screen and gathers the bright parts of
// the frame it crosses - sky between leaves, a window, a lamp - each step
// weaker than the last. Added back over the frame, the gaps in whatever
// stands between the viewer and the light become shafts, and the shapes that
// block it cut them. Coloured things lit from behind cast coloured shafts.
//
// Uniforms (48 bytes):
//   source    - the light's place on the screen, UV (may be off it).
//   length    - how far the shafts reach, in screen heights: a pixel this
//               far from the source gathers at half strength.
//   intensity - how strongly the gathered light is added.
//   color     - its tint (rgb); a.
//   decay     - each step's weight after the last (0.9 - 0.99).
//   threshold - brightness below which the frame adds nothing: only what
//               glows casts a shaft, by how far past this it is.

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

struct LightShaftsUniforms {
    source:    vec2<f32>,
    length:    f32,
    intensity: f32,
    color:     vec4<f32>,
    decay:     f32,
    threshold: f32,
    aspect:    f32,
    _pad:      f32,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: LightShaftsUniforms;

const SAMPLES: i32 = 64;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let base = textureSample(src_texture, src_sampler, in.tex_coord);
    // Every pixel marches all the way to the source - stopping short draws
    // an arc where the ray first fails to reach a bright sun - and what it
    // gathers fades with its distance from the source, measured in screen
    // heights (x scaled by the aspect).
    let step = (u.source - in.tex_coord) / f32(SAMPLES);
    let d = vec2<f32>((u.source.x - in.tex_coord.x) * u.aspect, u.source.y - in.tex_coord.y);
    let r = length(d) / max(u.length, 1e-3);
    let fade = 1.0 / (1.0 + r * r);
    // A far pixel takes long steps; start each at its own fraction of one
    // (interleaved gradient noise) so they band as grain rather than stripes.
    let jitter = fract(52.9829189 * fract(dot(in.clip_position.xy, vec2<f32>(0.06711056, 0.00583715))));

    var coord = in.tex_coord + step * (jitter - 1.0);
    var weight = 1.0;
    var gathered = vec3<f32>(0.0);
    for (var i = 0; i < SAMPLES; i = i + 1) {
        coord = coord + step;
        let s = textureSampleLevel(src_texture, src_sampler, coord, 0.0).rgb;
        // The part of the colour above the threshold, as bloom's prefilter
        // takes it: only what is brighter than `threshold` casts a shaft,
        // and an HDR sky casts in proportion to how far past it it is.
        let bright = max(max(s.r, s.g), s.b);
        gathered = gathered + s * (max(bright - u.threshold, 0.0) / max(bright, 1e-4)) * weight;
        weight = weight * u.decay;
    }
    let shafts = gathered / f32(SAMPLES) * fade * u.intensity * u.color.rgb;
    return vec4<f32>(base.rgb + shafts, base.a);
}
