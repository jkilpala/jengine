// The velocity pass (Oct 2026): each sprite's motion across the screen since
// the last frame, written where it covers - for the `motion_blur` post
// effect (`motion_blur.wgsl`). Drawn through the sprite batch
// (`WgpuSpriteBatch::velocity_pipeline`), so the bindings and the vertex are
// the default sprite shader's; the vertex colour carries the velocity, in
// screen UV per sixtieth of a second, in `.rg` (`render_velocity_wgpu`).
//
// The target is cleared to the camera's own motion, so whatever no sprite
// covers (tilemaps, particles, text) moves as the world under the camera.

struct Globals {
    view_projection: mat4x4<f32>,
    time:            vec4<f32>,
    params:          array<vec4<f32>, 4>,
    mask_xf:         array<vec4<f32>, 2>,
    mask_info:       vec4<f32>,
    mask_pts:        array<vec4<f32>, 8>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var u_texture: texture_2d<f32>;
@group(0) @binding(2) var u_sampler: sampler;

struct VsIn {
    @location(0) position: vec2<f32>,
    @location(1) uvq:      vec3<f32>,
    @location(2) color:    vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       uvq:           vec3<f32>,
    @location(1)       color:         vec4<f32>,
    @location(2)       world:         vec2<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip_position = globals.view_projection * vec4<f32>(in.position, 0.0, 1.0);
    out.uvq = in.uvq;
    out.color = in.color;
    out.world = in.position;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uvq.xy / in.uvq.z;
    let a = textureSample(u_texture, u_sampler, uv).a * in.color.a;
    if (a < 0.5) {
        discard;
    }
    // Blue is the sprite's z, for `depth_of_field`.
    return vec4<f32>(in.color.rg, in.color.b, 1.0);
}
