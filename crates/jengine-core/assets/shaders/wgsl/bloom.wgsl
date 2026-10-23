// Bloom - a mip chain, downsampled and then upsampled back up.
//
// The technique is the one Unity's and Godot's bloom use (Jimenez, "Next
// Generation Post Processing in Call of Duty: Advanced Warfare", 2014):
//
//   prefilter  full res  -> mip 0 (half): a 13-tap downsample, then the
//              threshold with a soft knee;
//   down       mip i-1   -> mip i:        the same 13-tap downsample;
//   up         mip i+1   -> mip i:        a 9-tap tent, added onto what the
//              downsample left there (the pipeline blends additively), times
//              `scatter`;
//   composite  source + mip 0 (tent) * intensity * norm -> destination.
//
// Every level halves the resolution, so seven levels reach a glow a fair
// fraction of the screen wide for the price of a few full-screen passes.
// The single-pass 9-tap blur this replaced reached a few pixels, and raising
// its radius only spread the taps into ghost copies.
//
// No Karis average on the first downsample (weighting each box by
// 1 / (1 + luma)): it stops a lone very bright pixel flickering, but it does
// so by taking most of the energy out of *every* small bright thing, and in
// 2D the small bright things - sparks, embers, bullets - are what should
// glow. Values are clamped instead (`MAX_VALUE`), which is what a NaN or a
// runaway additive stack needs.
//
// Texel sizes come from `textureDimensions`, so one uniform serves every
// pass. `norm` is 1 / (1 + scatter + scatter^2 + ...) over the levels,
// computed on the CPU, so `intensity` means the same thing whatever the
// chain's depth.

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

struct BloomUniforms {
    threshold: f32,
    knee:      f32,
    intensity: f32,
    scatter:   f32,
    norm:      f32,
    _pad0:     f32,
    _pad1:     f32,
    _pad2:     f32,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: BloomUniforms;
// The composite's second input, mip 0 of the chain. Only `fs_composite`
// reads it, and only its pipeline layout declares it.
@group(0) @binding(3) var bloom_texture: texture_2d<f32>;

// Half-float's largest finite value is 65504; anything near it, or a NaN
// from a bad blend, would smear across the whole chain.
const MAX_VALUE: f32 = 60000.0;

fn texel(t: texture_2d<f32>) -> vec2<f32> {
    return vec2<f32>(1.0) / vec2<f32>(textureDimensions(t));
}

fn fetch(uv: vec2<f32>) -> vec3<f32> {
    let c = textureSample(src_texture, src_sampler, uv).rgb;
    // `c == c` is false for NaN.
    return select(vec3<f32>(0.0), min(c, vec3<f32>(MAX_VALUE)), c == c);
}

// Unity's soft-knee threshold: below `threshold - knee` nothing, above
// `threshold + knee` everything over the threshold, a quadratic between.
fn prefilter(c: vec3<f32>) -> vec3<f32> {
    let brightness = max(c.r, max(c.g, c.b));
    let knee = max(u.knee, 1e-5);
    var soft = clamp(brightness - u.threshold + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee);
    let contribution = max(soft, brightness - u.threshold) / max(brightness, 1e-5);
    return c * contribution;
}

// The 13-tap downsample: a 4x4 box around the centre plus four 2x2 boxes
// at the corners, weighted 0.5 / 0.125 x 4, which filters far better than
// one bilinear tap per output pixel.
fn downsample13(uv: vec2<f32>) -> vec3<f32> {
    let t = texel(src_texture);
    let a = fetch(uv + t * vec2<f32>(-2.0,  2.0));
    let b = fetch(uv + t * vec2<f32>( 0.0,  2.0));
    let c = fetch(uv + t * vec2<f32>( 2.0,  2.0));
    let d = fetch(uv + t * vec2<f32>(-2.0,  0.0));
    let e = fetch(uv);
    let f = fetch(uv + t * vec2<f32>( 2.0,  0.0));
    let g = fetch(uv + t * vec2<f32>(-2.0, -2.0));
    let h = fetch(uv + t * vec2<f32>( 0.0, -2.0));
    let i = fetch(uv + t * vec2<f32>( 2.0, -2.0));
    let j = fetch(uv + t * vec2<f32>(-1.0,  1.0));
    let k = fetch(uv + t * vec2<f32>( 1.0,  1.0));
    let l = fetch(uv + t * vec2<f32>(-1.0, -1.0));
    let m = fetch(uv + t * vec2<f32>( 1.0, -1.0));

    let box_centre = (j + k + l + m) * 0.25;
    let box_tl = (a + b + d + e) * 0.25;
    let box_tr = (b + c + e + f) * 0.25;
    let box_bl = (d + e + g + h) * 0.25;
    let box_br = (e + f + h + i) * 0.25;

    return box_centre * 0.5 + (box_tl + box_tr + box_bl + box_br) * 0.125;
}

// The 9-tap tent upsample, one source texel wide.
fn tent9(t_src: texture_2d<f32>, uv: vec2<f32>) -> vec3<f32> {
    let t = texel(t_src);
    var s = textureSample(t_src, src_sampler, uv).rgb * 4.0;
    s += textureSample(t_src, src_sampler, uv + vec2<f32>(-t.x, 0.0)).rgb * 2.0;
    s += textureSample(t_src, src_sampler, uv + vec2<f32>( t.x, 0.0)).rgb * 2.0;
    s += textureSample(t_src, src_sampler, uv + vec2<f32>(0.0, -t.y)).rgb * 2.0;
    s += textureSample(t_src, src_sampler, uv + vec2<f32>(0.0,  t.y)).rgb * 2.0;
    s += textureSample(t_src, src_sampler, uv + vec2<f32>(-t.x, -t.y)).rgb;
    s += textureSample(t_src, src_sampler, uv + vec2<f32>( t.x, -t.y)).rgb;
    s += textureSample(t_src, src_sampler, uv + vec2<f32>(-t.x,  t.y)).rgb;
    s += textureSample(t_src, src_sampler, uv + vec2<f32>( t.x,  t.y)).rgb;
    return s / 16.0;
}

@fragment
fn fs_prefilter(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(prefilter(downsample13(in.tex_coord)), 1.0);
}

@fragment
fn fs_down(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(downsample13(in.tex_coord), 1.0);
}

@fragment
fn fs_up(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(tent9(src_texture, in.tex_coord) * u.scatter, 1.0);
}

@fragment
fn fs_composite(in: VsOut) -> @location(0) vec4<f32> {
    let base = textureSample(src_texture, src_sampler, in.tex_coord);
    let bloom = tent9(bloom_texture, in.tex_coord);
    return vec4<f32>(base.rgb + bloom * u.intensity * u.norm, base.a);
}
