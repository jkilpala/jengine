//! **GPU particles that keep state** (Oct 2026): `simulate: "state"` on a
//! [`GpuParticles`] emitter. Each particle is a position, a velocity, an age
//! and a life in a GPU buffer, and a compute pass (`gpu_particles_state.wgsl`)
//! steps them every frame - so they are born where the emitter **is**, and a
//! moving emitter leaves a wake; they **bounce** off colliders (`bounce`,
//! `friction`) instead of being put out on the nearest side; a collider that
//! moves **pushes** them (its velocity is in the step); a moving emitter can
//! throw its speed into them (`inherit`); and attractors are real forces.
//!
//! The step runs once a frame in `ecs::frame_draw::draw_frame`, before the
//! views, and the sprite batch keeps each emitter's buffer, keyed by its
//! entity - so no host was wired. A device with no compute (WebGL2) never
//! steps one, and the batch then draws the emitter in closed form exactly as
//! `simulate: "closed"` does: the same cloud, minus what only state can do.
//! [`step_cpu`] is the shader on the CPU, line for line - change both
//! together; `stateful_particles_step_on_the_gpu` compares them.

#[cfg(feature = "gpu-wgpu")]
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use super::gpu::{GpuParticles, MAX_ATTRACTORS, MAX_COLLIDERS};

/// The longest step a frame takes (a slow frame is run as several).
pub const MAX_STEP: f32 = 1.0 / 120.0;
/// The most steps one frame runs; past that the frame's time is dropped.
pub const MAX_SUBSTEPS: u32 = 8;
/// Threads a compute workgroup runs (`@workgroup_size` in the shader).
pub const WORKGROUP: u32 = 64;

/// One particle as the buffer holds it.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "gpu-wgpu", derive(Pod, Zeroable))]
pub struct StateParticle {
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    /// Seconds since it was born; below 0, not born yet.
    pub age: f32,
    pub life: f32,
    /// How many times it has been born (reseeds its launch).
    pub cycle: f32,
    pub _pad: f32,
}

/// What the compute step reads.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "gpu-wgpu", derive(Pod, Zeroable))]
pub struct StepUniform {
    /// Step, steps, time, count.
    pub clock: [f32; 4],
    /// Emitter origin x, y, rotation, scale x.
    pub xf: [f32; 4],
    /// Scale y, emitter velocity x, y, seed.
    pub xf2: [f32; 4],
    /// Lifetime min, max, speed min, max.
    pub life: [f32; 4],
    /// Angle min, max, gravity x, y.
    pub aim: [f32; 4],
    /// Drag, noise, noise speed, inherit.
    pub motion: [f32; 4],
    /// Spawn radius, edge, arms, bounce.
    pub shape: [f32; 4],
    /// Friction, attractors, colliders.
    pub extra: [f32; 4],
    /// Attractors in world space: x, y, strength, swirl; and radius.
    pub att: [[f32; 4]; MAX_ATTRACTORS],
    pub att2: [[f32; 4]; MAX_ATTRACTORS],
    /// Colliders in world space: centre x, y, half width, height (a circle's
    /// radius in z); the cosine and sine of the turn, circle; velocity x, y.
    pub col: [[f32; 4]; MAX_COLLIDERS],
    pub col2: [[f32; 4]; MAX_COLLIDERS],
    pub col3: [[f32; 4]; MAX_COLLIDERS],
}

/// What a stateful emitter remembers between frames, to step the next.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StepClock {
    pub time: Option<f32>,
    pub origin: Vec2,
}

/// One stateful emitter this frame: its key (the entity), its particle count
/// and where it is.
#[derive(Debug, Clone)]
pub struct StateEmitter {
    pub key: u64,
    pub emitter: GpuParticles,
    pub origin: Vec2,
    pub rotation: f32,
    pub scale: Vec2,
}

/// Every emitter with `simulate: "state"` in the world.
pub fn gather(world: &hecs::World) -> Vec<StateEmitter> {
    use crate::ecs::components::{Transform, WorldTransform};
    let mut out = Vec::new();
    for (e, (g, t, w)) in world.query::<(&GpuParticles, &Transform, Option<&WorldTransform>)>().iter() {
        if !g.stateful() || g.count == 0 {
            continue;
        }
        let (origin, rotation, scale) = w.map_or((t.position, t.rotation, t.scale), |w| (w.position, w.rotation, w.scale));
        out.push(StateEmitter { key: e.to_bits().get(), emitter: g.clone(), origin, rotation, scale });
    }
    out
}

/// The particles a stateful emitter starts with: none born yet, each due
/// after a share of the longest life, so the cloud fills over one lifetime
/// rather than bursting.
pub fn initial(g: &GpuParticles) -> Vec<StateParticle> {
    let lmax = g.lifetime[1].max(g.lifetime[0]).max(0.01);
    (0..g.count)
        .map(|i| StateParticle { age: -rand(i, g.seed % 65536, 2) * lmax, life: 1.0, ..Default::default() })
        .collect()
}

/// The step uniform for this frame, and the clock to keep for the next -
/// `None` when nothing is due (no time passed, or the clock was rewound, which
/// starts it over).
pub fn step_uniform(e: &StateEmitter, clock: &StepClock) -> (Option<StepUniform>, StepClock) {
    let g = &e.emitter;
    let next = StepClock { time: Some(g.time), origin: e.origin };
    let Some(last) = clock.time else { return (None, next) };
    let dt = (g.time - last).min(MAX_STEP * MAX_SUBSTEPS as f32);
    if dt <= 0.0 {
        return (None, next);
    }
    let steps = ((dt / MAX_STEP).ceil() as u32).clamp(1, MAX_SUBSTEPS);
    let vel = (e.origin - clock.origin) / dt;
    let mut att = [[0.0; 4]; MAX_ATTRACTORS];
    let mut att2 = [[0.0; 4]; MAX_ATTRACTORS];
    let na = g.world.attractors.len().min(MAX_ATTRACTORS);
    for (j, (a, radius)) in g.world.attractors.iter().take(na).enumerate() {
        att[j] = *a;
        att2[j] = [*radius, 0.0, 0.0, 0.0];
    }
    let mut col = [[0.0; 4]; MAX_COLLIDERS];
    let mut col2 = [[0.0; 4]; MAX_COLLIDERS];
    let mut col3 = [[0.0; 4]; MAX_COLLIDERS];
    let nc = g.world.colliders.len().min(MAX_COLLIDERS);
    for (j, c) in g.world.colliders.iter().take(nc).enumerate() {
        col[j] = [c.centre.x, c.centre.y, c.half.x, c.half.y];
        col2[j] = [c.rotation.cos(), c.rotation.sin(), if c.circle { 1.0 } else { 0.0 }, 0.0];
        col3[j] = [c.velocity.x, c.velocity.y, 0.0, 0.0];
    }
    let u = StepUniform {
        clock: [dt / steps as f32, steps as f32, g.time, g.count as f32],
        xf: [e.origin.x, e.origin.y, e.rotation, e.scale.x],
        xf2: [e.scale.y, vel.x, vel.y, (g.seed % 65536) as f32],
        life: [g.lifetime[0].max(0.01), g.lifetime[1].max(g.lifetime[0]).max(0.01), g.speed[0], g.speed[1]],
        aim: [g.angle[0], g.angle[1], g.gravity[0], g.gravity[1]],
        motion: [g.drag.max(0.0), g.noise, g.noise_speed, g.inherit],
        shape: [g.radius.max(0.0), if g.edge { 1.0 } else { 0.0 }, g.arms as f32, g.bounce.clamp(0.0, 1.0)],
        extra: [g.friction.clamp(0.0, 1.0), na as f32, nc as f32, 0.0],
        att,
        att2,
        col,
        col2,
        col3,
    };
    (Some(u), next)
}

fn rand(a: u32, b: u32, c: u32) -> f32 {
    let h = super::gpu::hash_u(a ^ super::gpu::hash_u(b.wrapping_add(super::gpu::hash_u(c))));
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

/// Born (again): its launch from a hash of its index and how many times it
/// has been, placed at the emitter and turned and scaled with it.
fn spawn(p: &mut StateParticle, i: u32, u: &StepUniform, over: f32) {
    let seed = u.xf2[3] as u32;
    p.cycle += 1.0;
    let c = p.cycle as u32;
    let tau = std::f32::consts::TAU;
    p.life = u.life[0] + (u.life[1] - u.life[0]) * rand(i, c ^ seed, 1);
    let r3 = rand(i, c ^ seed, 3);
    let a = if u.shape[2] >= 1.0 {
        let arms = u.shape[2];
        u.aim[0] + (r3 * arms).floor() * tau / arms + (rand(i, c ^ seed, 8) - 0.5) * 0.5
    } else {
        u.aim[0] + (u.aim[1] - u.aim[0]) * r3
    };
    let speed = u.life[2] + (u.life[3] - u.life[2]) * rand(i, c ^ seed, 4);
    let ra = rand(i, c ^ seed, 5) * tau;
    let rr = if u.shape[1] > 0.5 { u.shape[0] } else { u.shape[0] * rand(i, c ^ seed, 6).sqrt() };
    let (sn, cs) = u.xf[2].sin_cos();
    let turn = |v: Vec2| Vec2::new(v.x * cs - v.y * sn, v.x * sn + v.y * cs);
    let scale = Vec2::new(u.xf[3], u.xf2[0]);
    let local = Vec2::new(ra.cos(), ra.sin()) * rr * scale;
    let v0 = turn(Vec2::new(a.cos(), a.sin()) * speed * u.xf[3]) + Vec2::new(u.xf2[1], u.xf2[2]) * u.motion[3];
    let pos = Vec2::new(u.xf[0], u.xf[1]) + turn(local);
    p.pos = pos.into();
    p.vel = v0.into();
    p.age = over;
}

/// One frame's steps of every particle - `cs_step` in
/// `gpu_particles_state.wgsl`, line for line.
pub fn step_cpu(ps: &mut [StateParticle], u: &StepUniform) {
    for (i, p) in ps.iter_mut().enumerate() {
        step_one(p, i as u32, u);
    }
}

fn step_one(p: &mut StateParticle, i: u32, u: &StepUniform) {
    let h = u.clock[0];
    let steps = u.clock[1] as u32;
    for s in 0..steps {
        let t = u.clock[2] - h * (steps - 1 - s) as f32;
        if p.age < 0.0 {
            p.age += h;
            if p.age < 0.0 {
                continue;
            }
            spawn(p, i, u, p.age);
        } else {
            p.age += h;
            if p.age >= p.life {
                let over = p.age - p.life;
                spawn(p, i, u, over);
            }
        }
        let mut pos = Vec2::from(p.pos);
        let mut vel = Vec2::from(p.vel);
        let mut acc = Vec2::new(u.aim[2], u.aim[3]);
        // Turbulence: a slowly turning field anchored to the world.
        let w = t * u.motion[2];
        acc += Vec2::new((pos.y * 0.013 + w).sin(), (pos.x * 0.011 - w * 1.3).cos()) * u.motion[1];
        // Attractors: a spring toward each, and a push round it.
        for j in 0..(u.extra[1] as usize).min(MAX_ATTRACTORS) {
            let a = u.att[j];
            let d = Vec2::new(a[0], a[1]) - pos;
            let radius = u.att2[j][0];
            let fall = if radius > 0.0 { (-d.length_squared() / (radius * radius)).exp() } else { 1.0 };
            acc += d * (a[2] * fall) + Vec2::new(-d.y, d.x) * (a[3] * fall);
        }
        vel += acc * h;
        vel *= (-u.motion[0] * h).exp();
        pos += vel * h;
        // Colliders: out onto the surface, and the velocity into it bounced
        // and rubbed - relative to the collider, so a moving one pushes.
        for j in 0..(u.extra[2] as usize).min(MAX_COLLIDERS) {
            let s = u.col[j];
            let r = u.col2[j];
            let cv = Vec2::new(u.col3[j][0], u.col3[j][1]);
            let centre = Vec2::new(s[0], s[1]);
            let d = pos - centre;
            let mut hit = false;
            let mut n = Vec2::ZERO;
            if r[2] > 0.5 {
                let l = d.length();
                if l < s[2] {
                    n = if l > 1e-5 { d / l } else { Vec2::new(0.0, 1.0) };
                    pos = centre + n * s[2];
                    hit = true;
                }
            } else {
                let q = Vec2::new(d.x * r[0] + d.y * r[1], -d.x * r[1] + d.y * r[0]);
                if q.x.abs() < s[2] && q.y.abs() < s[3] {
                    let (q, nl) = if s[2] - q.x.abs() < s[3] - q.y.abs() {
                        let sx = if q.x < 0.0 { -1.0 } else { 1.0 };
                        (Vec2::new(sx * s[2], q.y), Vec2::new(sx, 0.0))
                    } else {
                        let sy = if q.y < 0.0 { -1.0 } else { 1.0 };
                        (Vec2::new(q.x, sy * s[3]), Vec2::new(0.0, sy))
                    };
                    pos = centre + Vec2::new(q.x * r[0] - q.y * r[1], q.x * r[1] + q.y * r[0]);
                    n = Vec2::new(nl.x * r[0] - nl.y * r[1], nl.x * r[1] + nl.y * r[0]);
                    hit = true;
                }
            }
            if hit {
                let mut rv = vel - cv;
                let vn = rv.dot(n);
                if vn < 0.0 {
                    rv -= n * (vn * (1.0 + u.shape[3]));
                    let vt = rv - n * rv.dot(n);
                    rv -= vt * u.extra[0];
                }
                vel = rv + cv;
            }
        }
        p.pos = pos.into();
        p.vel = vel.into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::particles::gpu::{GpuCollider, GpuSimulation};

    fn emitter(g: GpuParticles, origin: Vec2) -> StateEmitter {
        StateEmitter { key: 1, emitter: g, origin, rotation: 0.0, scale: Vec2::ONE }
    }

    /// Step `frames` frames of `dt`, moving the emitter by `walk` a frame.
    fn run(g: &mut GpuParticles, frames: usize, dt: f32, walk: Vec2) -> Vec<StateParticle> {
        let mut ps = initial(g);
        let mut clock = StepClock::default();
        let mut origin = Vec2::ZERO;
        for _ in 0..frames {
            g.time += dt;
            origin += walk;
            let e = emitter(g.clone(), origin);
            let (u, next) = step_uniform(&e, &clock);
            clock = next;
            if let Some(u) = u {
                step_cpu(&mut ps, &u);
            }
        }
        ps
    }

    fn base() -> GpuParticles {
        GpuParticles {
            count: 400,
            lifetime: [2.0, 2.0],
            speed: [100.0, 100.0],
            simulate: GpuSimulation::State,
            ..Default::default()
        }
    }

    #[test]
    fn particles_are_born_over_one_lifetime_not_at_once() {
        let mut g = base();
        let ps = run(&mut g, 30, 1.0 / 60.0, Vec2::ZERO);
        let born = ps.iter().filter(|p| p.age >= 0.0).count();
        assert!(born > 40 && born < 160, "a quarter of a lifetime in, about a quarter born: {born}");
    }

    #[test]
    fn a_moving_emitter_leaves_a_wake() {
        // Slow particles from an emitter crossing 300 units: the cloud is
        // strung out behind it, where the closed form would carry it along.
        let mut g = GpuParticles { speed: [5.0, 5.0], lifetime: [3.0, 3.0], ..base() };
        let ps = run(&mut g, 180, 1.0 / 60.0, Vec2::new(300.0 / 180.0, 0.0));
        let xs: Vec<f32> = ps.iter().filter(|p| p.age >= 0.0).map(|p| p.pos[0]).collect();
        let (lo, hi) = xs.iter().fold((f32::MAX, f32::MIN), |(l, h), &x| (l.min(x), h.max(x)));
        assert!(hi - lo > 200.0, "spread along the path: {lo}..{hi}");
    }

    #[test]
    fn particles_bounce_off_a_floor_and_stay_above_it() {
        let mut g = GpuParticles {
            speed: [0.0, 0.0],
            gravity: [0.0, -800.0],
            bounce: 0.5,
            lifetime: [3.0, 3.0],
            ..base()
        };
        g.world.colliders = vec![GpuCollider {
            centre: Vec2::new(0.0, -150.0),
            half: Vec2::new(400.0, 50.0),
            rotation: 0.0,
            circle: false,
            velocity: Vec2::ZERO,
        }];
        let ps = run(&mut g, 120, 1.0 / 60.0, Vec2::ZERO);
        let born: Vec<&StateParticle> = ps.iter().filter(|p| p.age > 0.5).collect();
        assert!(!born.is_empty());
        assert!(born.iter().all(|p| p.pos[1] >= -100.01), "none fell through the floor");
        assert!(born.iter().any(|p| p.vel[1] > 10.0), "some are on the way back up - they bounced");
    }

    #[test]
    fn a_moving_collider_pushes_what_it_meets() {
        let mut g = GpuParticles { speed: [0.0, 0.0], lifetime: [10.0, 10.0], ..base() };
        g.world.colliders = vec![GpuCollider {
            centre: Vec2::new(-5.0, 0.0),
            half: Vec2::splat(20.0),
            rotation: 0.0,
            circle: true,
            velocity: Vec2::new(300.0, 0.0),
        }];
        let ps = run(&mut g, 90, 1.0 / 60.0, Vec2::ZERO);
        assert!(ps.iter().filter(|p| p.age > 0.0).any(|p| p.vel[0] > 50.0), "struck and sent on");
    }
}
