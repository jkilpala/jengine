// Depth of field (Oct 2026): each pixel blurred by how far its layer is
// from the one in focus. A layer's depth is its z - the velocity pass
// (`velocity.wgsl`) writes each covering sprite's z into its blue channel,
// and where no sprite covers it is cleared to UNKNOWN: sharp itself, behind
// everything, spreading onto nothing. So a forest drawn
// in layers - far hills, trees, the deer, near ferns - goes soft by layer,
// and pulling `focus` from one z to another racks the focus.
//
// Uniforms:
//   focus      - the z that is sharp.
//   range      - how far either side of it stays sharp, in z.
//   strength   - pixels of blur per unit of z past the range.
//   max_radius - the widest blur, in pixels.
//
// It gathers a disc round each pixel, inside out, and lets a tap in only if
// its own blur reaches this pixel ("scatter as gather") - so a sharp thing
// does not leak into the blur around it - and a tap *behind* this pixel's
// layer reaches no further than this pixel's own blur, so a soft background never bleeds
// over a sharp thing in front of it. A soft thing in front does spread over
// what is behind it, as a near fern does over the deer.

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

struct DofUniforms {
    focus:      f32,
    range:      f32,
    strength:   f32,
    max_radius: f32,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: DofUniforms;
@group(0) @binding(3) var velocity: texture_2d<f32>;

const TAPS: i32 = 48;
// What the velocity pass is cleared to (`depth_clear`): no sprite here.
const UNKNOWN: f32 = -60000.0;
const GOLDEN: f32 = 2.39996323;

// A pixel's layer - read, not filtered: blended across an edge, a depth
// between two layers belongs to neither.
fn depth_at(px: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(velocity));
    return textureLoad(velocity, clamp(px, vec2<i32>(0), size - vec2<i32>(1)), 0).b;
}

fn coc(depth: f32) -> f32 {
    if (depth <= UNKNOWN) {
        return 0.0;
    }
    return clamp((abs(depth - u.focus) - u.range) * u.strength, 0.0, u.max_radius);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(src_texture));
    let vsize = vec2<f32>(textureDimensions(velocity));
    let here = clamp(vec2<i32>(in.tex_coord * vec2<f32>(size)), vec2<i32>(0), size - vec2<i32>(1));
    // The velocity pass is the scene's size; a pixel-perfect source may not be.
    let vhere = vec2<i32>(in.tex_coord * vsize);
    let d0 = depth_at(vhere);
    let c0 = coc(d0);
    var sum = textureLoad(src_texture, here, 0);
    var count = 1.0;
    if (u.max_radius < 0.5) {
        return sum;
    }
    // The spiral turned by a hash of the pixel: one fixed spiral draws a
    // hard edge as a few dozen shifted copies of it, in bands; turned per
    // pixel, the copies average into a fine grain.
    var h = u32(here.x) * 1973u + u32(here.y) * 9277u + 26699u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    h = h ^ (h >> 16u);
    let spin = f32(h & 65535u) / 65536.0 * 6.28318531;
    for (var i = 0; i < TAPS; i = i + 1) {
        // A golden-angle spiral fills the disc evenly at any count.
        let r = u.max_radius * sqrt((f32(i) + 0.5) / f32(TAPS));
        let a = f32(i) * GOLDEN + spin;
        let off = vec2<f32>(cos(a), sin(a)) * r;
        let dt = depth_at(vhere + vec2<i32>(round(off * vsize / vec2<f32>(size))));
        var reach = coc(dt);
        if (dt < d0) {
            reach = min(reach, c0);
        }
        // A tap that does not reach adds what the pixel has so far instead
        // of being left out (Gustafsson's single-pass bokeh): every pixel
        // averages the same count, so a soft thing's edge fades by how much
        // of the disc lands on it. Left out, the taps that reach were
        // normalised up and a blur came out as a hard-edged dilation.
        let m = clamp(reach - r + 0.5, 0.0, 1.0);
        let px = clamp(here + vec2<i32>(round(off)), vec2<i32>(0), size - vec2<i32>(1));
        sum = sum + mix(sum / count, textureLoad(src_texture, px, 0), m);
        count = count + 1.0;
    }
    return sum / count;
}
