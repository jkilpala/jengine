// Chromatic aberration — radial RGB channel split.
//
// Each colour channel samples from a slightly different uv offset along
// the radial direction from screen center. The strength uniform controls
// the maximum offset (in UV space) at the corners.
//
// Uniforms (16 bytes):
//   strength  — peak offset in UV units (0.0..0.05 typical).
//   _pad0/1/2 — alignment.

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

struct ChromaticUniforms {
    strength: f32,
    _pad0:    f32,
    _pad1:    f32,
    _pad2:    f32,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: ChromaticUniforms;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let center = vec2<f32>(0.5, 0.5);
    let dir    = in.tex_coord - center;

    // Scale offset by radial distance so corners get max split.
    let scale  = u.strength;
    let r_uv   = in.tex_coord + dir * scale;
    let g_uv   = in.tex_coord;
    let b_uv   = in.tex_coord - dir * scale;

    let r = textureSample(src_texture, src_sampler, r_uv).r;
    let g = textureSample(src_texture, src_sampler, g_uv).g;
    let b = textureSample(src_texture, src_sampler, b_uv).b;
    let a = textureSample(src_texture, src_sampler, g_uv).a;

    return vec4<f32>(r, g, b, a);
}
