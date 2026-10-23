// Pixelate - the frame point-sampled in cells, one per art pixel.
//
// A pixel-art game draws at a low resolution and scales it up. This does
// the same to a full-resolution frame: every screen pixel takes the colour
// at the centre of its cell, so a rotated or scaled sprite lands on the
// same grid as everything else instead of breaking into finer-than-art
// "mixels", and light and glow are drawn at the art's resolution too.
//
// The grid is anchored to the world (`offset` is where world (0, 0) falls,
// modulo a cell), so a camera moving by less than a cell moves the grid
// with the world: the art scrolls smoothly and nothing shimmers.
//
// Uniforms (16 bytes):
//   cell    - a cell's size in screen pixels (the frame's height / lines).
//   _pad    - alignment.
//   offset  - the grid's origin in screen pixels, modulo `cell`.

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

struct PixelateUniforms {
    cell:   f32,
    _pad:   f32,
    offset: vec2<f32>,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: PixelateUniforms;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(src_texture));
    let cell = max(u.cell, 1.0);
    let index = floor((in.clip_position.xy - u.offset) / cell);
    let centre = (index + vec2<f32>(0.5)) * cell + u.offset;
    let texel = clamp(vec2<i32>(floor(centre)), vec2<i32>(0), dims - vec2<i32>(1));
    return textureLoad(src_texture, texel, 0);
}
