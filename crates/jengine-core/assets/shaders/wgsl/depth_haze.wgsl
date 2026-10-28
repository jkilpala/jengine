// Depth haze (Oct 2026): the air between the eye and what is far away -
// each pixel mixed toward the haze colour by how deep it is. Its depth is
// what the velocity pass (`velocity.wgsl`) wrote into its blue: a 3D stage's
// depth for whatever stands on one (`ecs::stage`, per vertex for a floor),
// a sprite's z otherwise. HD-2D's far roofs and hills go pale and blue this
// way, which is half of what makes a diorama read as deep.
//
// Uniforms:
//   color - the haze's colour (rgb, linear) and the most it covers (a).
//   near  - the z at which the haze begins (nearer is higher z).
//   far   - the z at which it is at its most.
//   sky   - how much covers a pixel with no depth (a backdrop, the sky).
//   curve - how it thickens between them (1 even, 2 slow then fast).

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

struct HazeUniforms {
    color: vec4<f32>,
    near:  f32,
    far:   f32,
    sky:   f32,
    curve: f32,
}

@group(0) @binding(0) var src_texture: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;
@group(0) @binding(2) var<uniform> u: HazeUniforms;
@group(0) @binding(3) var velocity: texture_2d<f32>;

// What the velocity pass is cleared to (`depth_clear`): nothing drew here.
const UNKNOWN: f32 = -60000.0;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(src_texture));
    let here = clamp(vec2<i32>(in.tex_coord * vec2<f32>(size)), vec2<i32>(0), size - vec2<i32>(1));
    let colour = textureLoad(src_texture, here, 0);
    let vsize = vec2<i32>(textureDimensions(velocity));
    let vhere = clamp(vec2<i32>(in.tex_coord * vec2<f32>(vsize)), vec2<i32>(0), vsize - vec2<i32>(1));
    let depth = textureLoad(velocity, vhere, 0).b;
    var amount = u.sky;
    if (depth > UNKNOWN) {
        let span = max(u.near - u.far, 1e-3);
        amount = pow(clamp((u.near - depth) / span, 0.0, 1.0), max(u.curve, 0.01));
    }
    amount = amount * u.color.a;
    return vec4<f32>(mix(colour.rgb, u.color.rgb, amount), colour.a);
}
