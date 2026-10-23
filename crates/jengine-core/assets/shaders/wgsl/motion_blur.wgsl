// Motion blur (Oct 2026): each pixel smeared back along how it moved across
// the screen since the last frame - read from the velocity pass
// (`velocity.wgsl`: each sprite's own motion, and the camera's where no
// sprite is). A car the camera follows has no motion on screen and stays
// sharp while the street streams past it; a car overtaking it streaks.
//
// Uniforms:
//   shutter - how much of a sixtieth of a second's motion is smeared (1 is
//             all of it - a half-open film shutter at 30 fps).
//   samples - taps along the smear.
//   max_len - the longest smear, in screen UV (keeps a teleport from
//             streaking across the frame).

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

struct MotionBlurUniforms {
    shutter: f32,
    samples: f32,
    max_len: f32,
    // One pixel, in screen UV (across).
    texel:   f32,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: MotionBlurUniforms;
@group(0) @binding(3) var velocity: texture_2d<f32>;

// A pixel's own motion - read, not filtered: blended across an edge, a
// tap beside a still car would half-reach and streak it.
fn motion_at(uv: vec2<f32>) -> vec2<f32> {
    let size = vec2<i32>(textureDimensions(velocity));
    let px = clamp(vec2<i32>(uv * vec2<f32>(size)), vec2<i32>(0), size - vec2<i32>(1));
    var v = textureLoad(velocity, px, 0).rg * u.shutter;
    let len = length(v);
    if (len > u.max_len) {
        v = v * (u.max_len / len);
    }
    return v;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let v = motion_at(in.tex_coord);
    let n = max(u.samples, 1.0);
    var sum = textureSampleLevel(src_texture, src_sampler, in.tex_coord, 0.0);
    var weight = 1.0;
    // Centred on the pixel, so a smear sits about where the thing is now.
    // A tap counts only if what is there moves far enough to reach this
    // pixel ("scatter as gather"): the street streaks past a car the camera
    // follows, and the car - not moving on screen - does not bleed into it.
    for (var i = 0; i < 16; i = i + 1) {
        if (f32(i) >= n) {
            break;
        }
        let t = (f32(i) + 0.5) / n - 0.5;
        let at = in.tex_coord - v * t;
        let reach = length(motion_at(at)) * 0.5;
        let dist = length(v * t);
        let w = clamp((reach - dist) / max(u.texel, 1e-5) + 1.0, 0.0, 1.0);
        // The tap's own pixel, as its motion was read: filtered, it would
        // bring in the neighbour its weight was not judged on.
        let size = vec2<i32>(textureDimensions(src_texture));
        let px = clamp(vec2<i32>(at * vec2<f32>(size)), vec2<i32>(0), size - vec2<i32>(1));
        sum = sum + textureLoad(src_texture, px, 0) * w;
        weight = weight + w;
    }
    return sum / weight;
}
