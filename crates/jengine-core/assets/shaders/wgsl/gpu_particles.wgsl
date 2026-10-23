// GPU particles (particles::gpu): every particle a closed form of its index
// and the time, evaluated here per instance. No vertex buffer, no state - one
// uniform. `GpuParticles::sample` is this on the CPU; change both together.

struct U {
    view_projection: mat4x4<f32>,
    xf:     vec4<f32>,  // origin x, y, rotation, scale
    clock:  vec4<f32>,  // time, count, seed, y scale over x scale
    life:   vec4<f32>,  // life min, max, speed min, max
    aim:    vec4<f32>,  // angle min, max, gravity x, y
    motion: vec4<f32>,  // drag, swirl, attract, noise
    shape:  vec4<f32>,  // spawn radius, edge, noise speed, stretch
    size:   vec4<f32>,  // size at birth, at death, arms
    c0:     vec4<f32>,
    c1:     vec4<f32>,
    c2:     vec4<f32>,
    counts: vec4<f32>,                // attractors, colliders, pile
    att:    array<vec4<f32>, 4>,      // x, y, strength, swirl (emitter space)
    att2:   array<vec4<f32>, 4>,      // radius
    col:    array<vec4<f32>, 8>,      // centre x, y, half w, h (circle: radius)
    col2:   array<vec4<f32>, 8>,      // cos, sin of the turn, circle
};

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var u_texture: texture_2d<f32>;
@group(0) @binding(2) var u_sampler: sampler;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0)       uv:            vec2<f32>,
    @location(1)       color:         vec4<f32>,
};

fn hash_u(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn rand(a: u32, b: u32, c: u32) -> f32 {
    let h = hash_u(a ^ hash_u(b + hash_u(c)));
    return f32(h & 0x00ffffffu) / 16777216.0;
}

const TAU: f32 = 6.28318530718;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) i: u32) -> VsOut {
    let t = u.clock.x;
    let seed = u32(u.clock.z);
    let life = u.life.x + (u.life.y - u.life.x) * rand(i, seed, 1u);
    let phase = rand(i, seed, 2u) * life;
    let tt = t + phase;
    let cycle = floor(tt / life);
    let age = tt - cycle * life;
    let c = u32(i32(cycle));
    let r3 = rand(i, c ^ seed, 3u);
    var a = u.aim.x + (u.aim.y - u.aim.x) * r3;
    if (u.size.z >= 1.0) {
        let arms = u.size.z;
        a = u.aim.x + floor(r3 * arms) * TAU / arms + (rand(i, c ^ seed, 8u) - 0.5) * 0.5;
    }
    let speed = u.life.z + (u.life.w - u.life.z) * rand(i, c ^ seed, 4u);
    let ra = rand(i, c ^ seed, 5u) * TAU;
    var rr = u.shape.x;
    if (u.shape.y <= 0.5) {
        rr = u.shape.x * sqrt(rand(i, c ^ seed, 6u));
    }
    let spawn = vec2<f32>(cos(ra), sin(ra)) * rr;
    let v0 = vec2<f32>(cos(a), sin(a)) * speed;
    let g = u.aim.zw;
    let k = u.motion.x;
    var disp: vec2<f32>;
    var grav: vec2<f32>;
    var vel: vec2<f32>;
    if (k > 1e-4) {
        let decay = exp(-k * age);
        let f = (1.0 - decay) / k;
        disp = v0 * f;
        grav = g * ((age - f) / k);
        vel = v0 * decay + g * f;
    } else {
        disp = v0 * age;
        grav = g * (0.5 * age * age);
        vel = v0 + g * age;
    }
    var p = spawn + disp + grav;
    p = p / (1.0 + u.motion.z * age);
    let s = u.motion.y * age;
    p = vec2<f32>(p.x * cos(s) - p.y * sin(s), p.x * sin(s) + p.y * cos(s));
    let w = u.shape.z * age + rand(i, c ^ seed, 7u) * TAU;
    p = p + vec2<f32>(sin(w), cos(w * 1.3)) * (u.motion.w * (age / life));
    // Attractors: drawn in as the particle ages, and turned round.
    let na = min(u32(u.counts.x), 4u);
    for (var j = 0u; j < na; j = j + 1u) {
        let a = u.att[j];
        let radius = u.att2[j].x;
        let d = p - a.xy;
        var fall = 1.0;
        if (radius > 0.0) {
            fall = exp(-dot(d, d) / (radius * radius));
        }
        let pull = (1.0 - 1.0 / (1.0 + max(a.z, 0.0) * age)) * fall;
        let ang = a.w * age * fall;
        let dd = d * (1.0 - pull);
        p = a.xy + vec2<f32>(dd.x * cos(ang) - dd.y * sin(ang), dd.x * sin(ang) + dd.y * cos(ang));
    }
    // Colliders: a particle inside one is put out on its nearest side, up to
    // `pile` past it.
    let heap = u.counts.z * rand(i, c ^ seed, 9u);
    let nc = min(u32(u.counts.y), 8u);
    for (var j = 0u; j < nc; j = j + 1u) {
        let s = u.col[j];
        let r = u.col2[j];
        let d = p - s.xy;
        if (r.z > 0.5) {
            let l = length(d);
            if (l < s.z) {
                if (l > 1e-5) {
                    p = s.xy + d / l * (s.z + heap);
                } else {
                    p = s.xy + vec2<f32>(0.0, s.z + heap);
                }
            }
        } else {
            var q = vec2<f32>(d.x * r.x + d.y * r.y, -d.x * r.y + d.y * r.x);
            if (abs(q.x) < s.z && abs(q.y) < s.w) {
                if (s.z - abs(q.x) < s.w - abs(q.y)) {
                    q.x = select(s.z + heap, -(s.z + heap), q.x < 0.0);
                } else {
                    q.y = select(s.w + heap, -(s.w + heap), q.y < 0.0);
                }
                p = s.xy + vec2<f32>(q.x * r.x - q.y * r.y, q.x * r.y + q.y * r.x);
            }
        }
    }
    let f = age / life;
    let size = u.size.x + (u.size.y - u.size.x) * f;
    var color: vec4<f32>;
    if (f < 0.5) {
        color = mix(u.c0, u.c1, f * 2.0);
    } else {
        color = mix(u.c1, u.c2, f * 2.0 - 1.0);
    }

    // The quad: two triangles, stretched along the direction of travel
    // (turned with the swirl).
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, -0.5), vec2<f32>(0.5, 0.5),
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, 0.5), vec2<f32>(-0.5, 0.5),
    );
    let corner = corners[vi % 6u];
    var dir = vec2<f32>(1.0, 0.0);
    let vl = length(vel);
    if (vl > 1e-4) {
        let vd = vel / vl;
        dir = vec2<f32>(vd.x * cos(s) - vd.y * sin(s), vd.x * sin(s) + vd.y * cos(s));
    }
    let side = vec2<f32>(-dir.y, dir.x);
    let local = p + (dir * corner.x * (1.0 + u.shape.w) + side * corner.y) * size;

    // Into the world by the emitter's transform.
    // Scaled (x, and y by its ratio - a cloud squashed onto a floor), then
    // turned, then placed.
    let scaled = local * vec2<f32>(u.xf.w, u.xf.w * u.clock.w);
    let cr = cos(u.xf.z);
    let sr = sin(u.xf.z);
    let world = u.xf.xy + vec2<f32>(scaled.x * cr - scaled.y * sr, scaled.x * sr + scaled.y * cr);
    var out: VsOut;
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
