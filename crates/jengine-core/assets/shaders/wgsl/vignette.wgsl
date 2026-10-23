// Vignette — radial darkening (or tint) toward screen edges.
//
// Uniforms (32 bytes):
//   intensity   — how aggressive the falloff is (0 = none, 1 = strong).
//   smoothness  — width of the falloff band (0 = hard ring, 1 = soft).
//   _pad0/_pad1 — alignment.
//   color (vec4) — colour to fade toward at the edges (rgb used).

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

struct VignetteUniforms {
    intensity:  f32,
    smoothness: f32,
    _pad0:      f32,
    _pad1:      f32,
    color:      vec4<f32>,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: VignetteUniforms;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let base = textureSample(src_texture, src_sampler, in.tex_coord);

    // Distance from screen center (0..~0.707).
    let d = distance(in.tex_coord, vec2<f32>(0.5, 0.5));

    // Inner radius shrinks as intensity rises; outer radius extends by
    // smoothness so a higher smoothness softens the falloff edge.
    let inner = clamp(0.5 - u.intensity * 0.5, 0.0, 0.5);
    let outer = inner + max(u.smoothness, 0.001);

    // smoothstep(inner, outer, d) → 0 at center, 1 at edge.
    let v = smoothstep(inner, outer, d);

    let tinted = mix(base.rgb, u.color.rgb, v);
    return vec4<f32>(tinted, base.a);
}
