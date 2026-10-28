// The normal pass: sprites drawn into the normal buffer the deferred point
// and spot lights read (`normal_response` in their shaders). Driven by
// `ecs::wgpu_systems::render_normals_wgpu` through `WgpuLitSpritePipeline`.
//
// Until Sep 2026 this file was a **forward** shader (`fs_main`) that lit
// `lit_sprite` materials itself against a fixed array of sixteen lights and
// its own copy of the occluder set, drawn after every other sprite whatever
// its z - so a normal-mapped floor covered what stood on it. Normal-mapped
// sprites are ordinary sprites now; only their normals are drawn here. The
// pipeline layout still binds the lights and occluder buffers that shader
// read (group 0, bindings 1 and 2); nothing here declares them.
//
// Vertex layout matches `WgpuSpriteBatch`'s quads:
//   location 0: vec2 position  (world-space, pre-transformed)
//   location 1: vec3 uv     (u q, v q, q) - q is 1 but for a mesh in perspective
//   location 2: vec4 color

struct Globals {
    view_projection:   mat4x4<f32>,
    // Written by `upload_frame`; unused by the normal pass.
    ambient:           vec4<f32>,
    light_count:       vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

@group(1) @binding(0) var u_diffuse:         texture_2d<f32>;
@group(1) @binding(1) var u_diffuse_sampler: sampler;
@group(1) @binding(2) var u_normal:          texture_2d<f32>;
@group(1) @binding(3) var u_normal_sampler:  sampler;

struct VsIn {
    @location(0) position: vec2<f32>,
    @location(1) uv:       vec3<f32>,
    @location(2) color:    vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       world_pos:     vec2<f32>,
    @location(1)       uv:            vec3<f32>,
    @location(2)       color:         vec4<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip_position = globals.view_projection * vec4<f32>(in.position, 0.0, 1.0);
    out.world_pos     = in.position;
    out.uv            = in.uv;
    out.color         = in.color;
    return out;
}

// This sprite's normal into the normal buffer, covering what is under it by
// the diffuse's alpha - so a sprite standing on a bumpy floor hides the
// floor's bumps, and one with no normal map writes flat. The encoded value
// is written as it is stored (0..1, +y up); the buffer is `Rgba8Unorm`, so
// nothing is gamma-converted on the way in or out.
// The normal, and beside it the surface's specular (`light_specular.wgsl`):
// strength (the material's, from green, times the normal map's alpha - the
// map's mask of what shines) and shininess (blue).
// And the sprite's light layers (`LightLayers`), whole: red carries them
// with the caster bit, `layers * 2 + (1 if not a caster)`.
struct NormalOut {
    @location(0) normal:   vec4<f32>,
    @location(1) specular: vec4<f32>,
    @location(2) layers:   u32,
};

@fragment
fn fs_normal(in: VsOut) -> NormalOut {
    // Both samples before the discard: sampling after a non-uniform
    // `discard` is not uniform control flow.
    // Divided per pixel: a mesh in perspective carries its weight in `z`.
    let uv = in.uv.xy / in.uv.z;
    let coverage = textureSample(u_diffuse, u_diffuse_sampler, uv).a * in.color.a;
    let sampled = textureSample(u_normal, u_normal_sampler, uv);
    let enc = sampled.xyz;
    if (coverage < 0.01) {
        discard;
    }
    // A caster (its entity has an occluder; `render_normals_wgpu` says so
    // with a red of 0) writes z as `(z - 0.5) * 0.9`, 0..0.45: an encoded
    // normal's z is never under 0.5, so the lights can tell (under 0.475)
    // and decode it back. A plain shift by 0.5 put a flat normal's z (1.0)
    // at exactly 0.5, on the line.
    let code = u32(round(in.color.r));
    var out = enc;
    if ((code & 1u) == 0u) {
        out.z = (out.z - 0.5) * 0.9;
    }
    var result: NormalOut;
    result.normal = vec4<f32>(out, coverage);
    result.specular = vec4<f32>(in.color.g * sampled.a, in.color.b, 0.0, coverage);
    result.layers = (code >> 1u) & 255u;
    return result;
}
