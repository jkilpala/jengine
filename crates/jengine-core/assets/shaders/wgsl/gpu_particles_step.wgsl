// The step of GPU particles that keep state (particles::gpu_state): once a
// frame, every particle aged, reborn at the emitter when its life is up,
// pushed by gravity, turbulence and attractors, and bounced off colliders.
// `gpu_state::step_cpu` is `cs_step` on the CPU, line for line; change both
// together. `gpu_particles_state.wgsl` draws what it leaves.

struct Particle {
    pos:   vec2<f32>,
    vel:   vec2<f32>,
    age:   f32,
    life:  f32,
    cycle: f32,
    pad:   f32,
};

struct StepU {
    clock:  vec4<f32>,  // step, steps, time, count
    xf:     vec4<f32>,  // origin x, y, rotation, scale x
    xf2:    vec4<f32>,  // scale y, emitter velocity x, y, seed
    life:   vec4<f32>,  // life min, max, speed min, max
    aim:    vec4<f32>,  // angle min, max, gravity x, y
    motion: vec4<f32>,  // drag, noise, noise speed, inherit
    shape:  vec4<f32>,  // spawn radius, edge, arms, bounce
    extra:  vec4<f32>,  // friction, attractors, colliders
    att:    array<vec4<f32>, 4>,
    att2:   array<vec4<f32>, 4>,
    col:    array<vec4<f32>, 8>,
    col2:   array<vec4<f32>, 8>,
    col3:   array<vec4<f32>, 8>,
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

// ─── the step ───────────────────────────────────────────────────────────

@group(0) @binding(0) var<uniform> su: StepU;
@group(0) @binding(1) var<storage, read_write> particles: array<Particle>;

fn spawn(p: ptr<function, Particle>, i: u32, over: f32) {
    let seed = u32(su.xf2.w);
    (*p).cycle = (*p).cycle + 1.0;
    let c = u32((*p).cycle);
    (*p).life = su.life.x + (su.life.y - su.life.x) * rand(i, c ^ seed, 1u);
    let r3 = rand(i, c ^ seed, 3u);
    var a = su.aim.x + (su.aim.y - su.aim.x) * r3;
    if (su.shape.z >= 1.0) {
        let arms = su.shape.z;
        a = su.aim.x + floor(r3 * arms) * TAU / arms + (rand(i, c ^ seed, 8u) - 0.5) * 0.5;
    }
    let speed = su.life.z + (su.life.w - su.life.z) * rand(i, c ^ seed, 4u);
    let ra = rand(i, c ^ seed, 5u) * TAU;
    var rr = su.shape.x;
    if (su.shape.y <= 0.5) {
        rr = su.shape.x * sqrt(rand(i, c ^ seed, 6u));
    }
    let sn = sin(su.xf.z);
    let cs = cos(su.xf.z);
    let scale = vec2<f32>(su.xf.w, su.xf2.x);
    let local = vec2<f32>(cos(ra), sin(ra)) * rr * scale;
    let v = vec2<f32>(cos(a), sin(a)) * speed * su.xf.w;
    let v0 = vec2<f32>(v.x * cs - v.y * sn, v.x * sn + v.y * cs) + su.xf2.yz * su.motion.w;
    (*p).pos = su.xf.xy + vec2<f32>(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
    (*p).vel = v0;
    (*p).age = over;
}

@compute @workgroup_size(64)
fn cs_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= u32(su.clock.w)) {
        return;
    }
    var p = particles[i];
    let h = su.clock.x;
    let steps = u32(su.clock.y);
    for (var s = 0u; s < steps; s = s + 1u) {
        let t = su.clock.z - h * f32(steps - 1u - s);
        if (p.age < 0.0) {
            p.age = p.age + h;
            if (p.age < 0.0) {
                continue;
            }
            spawn(&p, i, p.age);
        } else {
            p.age = p.age + h;
            if (p.age >= p.life) {
                let over = p.age - p.life;
                spawn(&p, i, over);
            }
        }
        var pos = p.pos;
        var vel = p.vel;
        var acc = su.aim.zw;
        // Turbulence: a slowly turning field anchored to the world.
        let w = t * su.motion.z;
        acc = acc + vec2<f32>(sin(pos.y * 0.013 + w), cos(pos.x * 0.011 - w * 1.3)) * su.motion.y;
        // Attractors: a spring toward each, and a push round it.
        let na = min(u32(su.extra.y), 4u);
        for (var j = 0u; j < na; j = j + 1u) {
            let a = su.att[j];
            let d = a.xy - pos;
            let radius = su.att2[j].x;
            var fall = 1.0;
            if (radius > 0.0) {
                fall = exp(-dot(d, d) / (radius * radius));
            }
            acc = acc + d * (a.z * fall) + vec2<f32>(-d.y, d.x) * (a.w * fall);
        }
        vel = vel + acc * h;
        vel = vel * exp(-su.motion.x * h);
        pos = pos + vel * h;
        // Colliders: out onto the surface, and the velocity into it bounced
        // and rubbed - relative to the collider, so a moving one pushes.
        let nc = min(u32(su.extra.z), 8u);
        for (var j = 0u; j < nc; j = j + 1u) {
            let sc = su.col[j];
            let r = su.col2[j];
            let cv = su.col3[j].xy;
            let d = pos - sc.xy;
            var hit = false;
            var n = vec2<f32>(0.0, 0.0);
            if (r.z > 0.5) {
                let l = length(d);
                if (l < sc.z) {
                    n = vec2<f32>(0.0, 1.0);
                    if (l > 1e-5) {
                        n = d / l;
                    }
                    pos = sc.xy + n * sc.z;
                    hit = true;
                }
            } else {
                var q = vec2<f32>(d.x * r.x + d.y * r.y, -d.x * r.y + d.y * r.x);
                if (abs(q.x) < sc.z && abs(q.y) < sc.w) {
                    var nl: vec2<f32>;
                    if (sc.z - abs(q.x) < sc.w - abs(q.y)) {
                        let sx = select(1.0, -1.0, q.x < 0.0);
                        q = vec2<f32>(sx * sc.z, q.y);
                        nl = vec2<f32>(sx, 0.0);
                    } else {
                        let sy = select(1.0, -1.0, q.y < 0.0);
                        q = vec2<f32>(q.x, sy * sc.w);
                        nl = vec2<f32>(0.0, sy);
                    }
                    pos = sc.xy + vec2<f32>(q.x * r.x - q.y * r.y, q.x * r.y + q.y * r.x);
                    n = vec2<f32>(nl.x * r.x - nl.y * r.y, nl.x * r.y + nl.y * r.x);
                    hit = true;
                }
            }
            if (hit) {
                var rv = vel - cv;
                let vn = dot(rv, n);
                if (vn < 0.0) {
                    rv = rv - n * (vn * (1.0 + su.shape.w));
                    let vt = rv - n * dot(rv, n);
                    rv = rv - vt * su.extra.x;
                }
                vel = rv + cv;
            }
        }
        p.pos = pos;
        p.vel = vel;
    }
    particles[i] = p;
}

