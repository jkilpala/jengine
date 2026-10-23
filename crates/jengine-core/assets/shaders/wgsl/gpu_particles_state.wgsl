// The draw of GPU particles that keep state (particles::gpu_state): each
// instance reads its particle from the buffer `gpu_particles_step.wgsl`
// steps. Built only on a device with compute and storage buffers in the
// vertex stage - WebGL2 has neither, and draws the emitter in closed form
// (`gpu_particles.wgsl`) instead.

struct Particle {
    pos:   vec2<f32>,
    vel:   vec2<f32>,
    age:   f32,
    life:  f32,
    cycle: f32,
    pad:   f32,
};

// ─── the draw ───────────────────────────────────────────────────────────

// The closed form's uniform (`gpu_particles.wgsl`), for the look: colours,
// sizes, stretch, the emitter's scale and the view.
struct U {
    view_projection: mat4x4<f32>,
    xf:     vec4<f32>,
    clock:  vec4<f32>,
    life:   vec4<f32>,
    aim:    vec4<f32>,
    motion: vec4<f32>,
    shape:  vec4<f32>,
    size:   vec4<f32>,
    c0:     vec4<f32>,
    c1:     vec4<f32>,
    c2:     vec4<f32>,
    counts: vec4<f32>,
    att:    array<vec4<f32>, 4>,
    att2:   array<vec4<f32>, 4>,
    col:    array<vec4<f32>, 8>,
    col2:   array<vec4<f32>, 8>,
};

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var u_texture: texture_2d<f32>;
@group(0) @binding(2) var u_sampler: sampler;
@group(0) @binding(3) var<storage, read> drawn: array<Particle>;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       uv:            vec2<f32>,
    @location(1)       color:         vec4<f32>,
};

@vertex
fn vs_state(@builtin(vertex_index) vi: u32, @builtin(instance_index) i: u32) -> VsOut {
    let p = drawn[i];
    var out: VsOut;
    if (p.age < 0.0 || p.age >= p.life) {
        // Not born, or between lives: a point outside the view.
        out.clip_position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        out.uv = vec2<f32>(0.0, 0.0);
        out.color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
        return out;
    }
    let f = p.age / p.life;
    let size = (u.size.x + (u.size.y - u.size.x) * f) * abs(u.xf.w);
    var color: vec4<f32>;
    if (f < 0.5) {
        color = mix(u.c0, u.c1, f * 2.0);
    } else {
        color = mix(u.c1, u.c2, f * 2.0 - 1.0);
    }
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, -0.5), vec2<f32>(0.5, 0.5),
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, 0.5), vec2<f32>(-0.5, 0.5),
    );
    let corner = corners[vi % 6u];
    var dir = vec2<f32>(1.0, 0.0);
    let vl = length(p.vel);
    if (vl > 1e-4) {
        dir = p.vel / vl;
    }
    let side = vec2<f32>(-dir.y, dir.x);
    let world = p.pos + (dir * corner.x * (1.0 + u.shape.w) + side * corner.y) * size;
    out.clip_position = u.view_projection * vec4<f32>(world, 0.0, 1.0);
    out.uv = corner + vec2<f32>(0.5, 0.5);
    out.uv.y = 1.0 - out.uv.y;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(u_texture, u_sampler, in.uv) * in.color;
}
