// Shape (vector path) shader — wgpu/WGSL canonical.
// Mirrors the GLSL shape vertex/fragment pair used by the glow `ShapeBatch`.

struct Globals {
    view_projection: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> u_globals: Globals;

struct VsIn {
    @location(0) position: vec2<f32>,
    @location(1) color:    vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       color:         vec4<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.clip_position = u_globals.view_projection * vec4<f32>(in.position, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
