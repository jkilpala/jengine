// Phase 3.c-4 Step 6b — fullscreen-triangle blit / postprocess passthrough.
//
// Mirrors `crates/jengine-core/assets/shaders/fullscreen.vert` +
// `passthrough.frag` (the shader pair the gl `PostProcessStack` uses for
// its final blit-to-screen path). Generates an oversized triangle
// covering clip space [-1,1] from `vertex_index` so no VBO is needed.
//
// Texture is sampled with the same coordinate convention as the gl
// path (origin at bottom-left): we feed `tex_coord` directly to
// `textureSample`. Source render targets are written with
// `Mat4::orthographic_rh_gl(...)` so the gl convention is preserved.
//
// Used by `WgpuPostProcessStack::execute` for the final ping → swapchain
// blit. Future per-effect WGSL shaders (bloom / vignette / color_grade /
// chromatic) will share the same vertex stage and only swap the
// fragment.

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       tex_coord:     vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VsOut {
    // Same fullscreen-triangle math as the gl fullscreen.vert: vertex 0
    // = (-1,-1), vertex 1 = (3,-1), vertex 2 = (-1,3). Bit twiddling
    // keeps the GPU from needing a vertex buffer.
    let x = f32((i32(vertex_index) & 1) << 2) - 1.0;
    let y = f32((i32(vertex_index) & 2) << 1) - 1.0;
    var out: VsOut;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    out.tex_coord     = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(src_texture, src_sampler, in.tex_coord);
}
