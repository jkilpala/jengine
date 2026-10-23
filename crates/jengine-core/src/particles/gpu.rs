//! **GPU particles** (Sep 2026): hundreds of thousands of particles from one
//! component, drawn in one instanced call with no per-particle state.
//!
//! A [`GpuParticles`] emitter is **stateless**: particle `i`'s position at
//! time `t` is a closed form of `i` and `t` - its own lifetime and phase from
//! a hash of `i`, its cycle (`floor((t + phase) / life)`) reseeding its
//! direction, speed and spawn point every time it is reborn, and its motion
//! the integral of launch velocity, gravity and drag, then pulled toward the
//! emitter (`attract`), turned about it (`swirl`) and wobbled (`noise`). The
//! vertex shader (`gpu_particles.wgsl`) evaluates that for every instance, so
//! the CPU never touches a particle and nothing has to be uploaded but one
//! uniform - which is why it runs on WebGL2 too, where compute shaders do not.
//!
//! What it cannot do follows from the same choice: a particle cannot bounce,
//! be pushed by a hit, or remember where the emitter *was* (it lives in the
//! emitter's space, so moving the emitter moves the cloud). That is what the
//! CPU [`super::ParticleEmitter`] is for. What it **can** do since Oct 2026 is
//! meet the world in closed form too: [`GpuAttractor`]s pull a particle in
//! and spin it round as it ages (a whirlwind), and with `collide` a particle
//! found inside a nearby collider is put out on its nearest side - which,
//! drawn every frame, is grains sliding along a rock and piling against it. [`GpuParticles::sample`] is the vertex
//! shader on the CPU, line for line, for the tests.
//!
//! With `simulate: "state"` (Oct 2026) an emitter keeps state after all,
//! where the device can: see [`super::gpu_state`] - a compute pass steps a
//! buffer of particles, so they bounce, are pushed and trail a moving
//! emitter. Where it cannot (WebGL2) the same emitter is drawn as here.

#[cfg(feature = "gpu-wgpu")]
use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// A cloud of stateless particles, drawn by the GPU.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuParticles {
    /// How many particles. Each loops on its own lifetime forever, so the
    /// cloud is always full (already "warm" at the first frame).
    pub count: u32,
    pub lifetime: [f32; 2],
    /// Launch speed, units per second.
    pub speed: [f32; 2],
    /// Launch direction, radians, in the emitter's space.
    pub angle: [f32; 2],
    /// Launch along this many evenly spaced directions (from `angle[0]`,
    /// each a little scattered) instead of across `angle`: with `swirl`,
    /// spiral arms. 0 launches across the whole range.
    pub arms: u32,
    /// Spawn on a disc of this radius - or its rim, with `edge`.
    pub radius: f32,
    pub edge: bool,
    /// Acceleration, units per second squared.
    pub gravity: [f32; 2],
    /// Velocity lost per second, `e^(-drag t)`.
    pub drag: f32,
    /// Radians per second the cloud turns about the emitter as it ages: a
    /// spiral arm for every launch direction.
    pub swirl: f32,
    /// Pull toward the emitter: distance shrinks as `1 / (1 + attract t)`.
    pub attract: f32,
    /// A wobble this many units across, growing with age.
    pub noise: f32,
    /// How fast the wobble turns, radians per second.
    pub noise_speed: f32,
    /// Size at birth and at death.
    pub size: [f32; 2],
    /// Stretch along the direction of travel: 0 square, 1 twice as long.
    pub stretch: f32,
    /// Colour at birth, half-life and death (linear RGBA).
    pub color_start: [f32; 4],
    pub color_mid: [f32; 4],
    pub color_end: [f32; 4],
    /// The particle texture; empty is a white square.
    pub texture_id: String,
    pub blend_mode: crate::renderer::material::BlendMode,
    pub z_order: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
    pub seed: u32,
    pub playing: bool,
    /// Seconds simulated, advanced by `particles::tick_particles` while
    /// `playing` (so a paused game pauses it).
    #[serde(skip)]
    pub time: f32,
    /// Points the cloud is drawn toward and spun round as it ages (Oct
    /// 2026): a whirlwind, a drain, a vortex. At most [`MAX_ATTRACTORS`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attractors: Vec<GpuAttractor>,
    /// Meet the scene's non-sensor colliders (Oct 2026): a particle inside
    /// one is put out on its nearest side. The [`MAX_COLLIDERS`] nearest the
    /// emitter take part; a box or a circle is itself, anything else its
    /// bounding box. Exact for an emitter not squashed by its scale.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub collide: bool,
    /// How deep the drift a collider gathers is: a particle put out of one
    /// lands up to this far past its surface (its own share, from a hash), so
    /// what piles against a rock is a soft heap rather than a hard line. 0
    /// lands each on the surface.
    #[serde(skip_serializing_if = "is_zero")]
    pub pile: f32,
    /// `closed` (the default): every particle a closed form of its index and
    /// the time, as above. `state` (Oct 2026): every particle a position and
    /// velocity a compute pass steps (`particles::gpu_state`) - born where
    /// the emitter is, bounced and pushed - where the device has compute;
    /// drawn closed where it has not (WebGL2).
    #[serde(skip_serializing_if = "GpuSimulation::is_closed")]
    pub simulate: GpuSimulation,
    /// `state` only: the share of the speed into a collider it comes out with.
    #[serde(skip_serializing_if = "is_default_bounce")]
    pub bounce: f32,
    /// `state` only: the share of the speed along a collider it loses there.
    #[serde(skip_serializing_if = "is_default_friction")]
    pub friction: f32,
    /// `state` only: the share of the emitter's own velocity a particle is
    /// born with - a torch swung throws its sparks.
    #[serde(skip_serializing_if = "is_zero")]
    pub inherit: f32,
    /// What `tick_particles` found this frame, in world space. Never saved.
    #[serde(skip)]
    pub world: GpuWorld,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

const DEFAULT_BOUNCE: f32 = 0.3;
const DEFAULT_FRICTION: f32 = 0.1;

fn is_default_bounce(v: &f32) -> bool {
    *v == DEFAULT_BOUNCE
}

fn is_default_friction(v: &f32) -> bool {
    *v == DEFAULT_FRICTION
}

/// How a GPU cloud moves ([`GpuParticles::simulate`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuSimulation {
    /// A closed form of each particle's index and the time: no state.
    #[default]
    Closed,
    /// Positions and velocities a compute pass steps (`gpu_state`).
    State,
}

impl GpuSimulation {
    fn is_closed(&self) -> bool {
        *self == GpuSimulation::Closed
    }
}

/// The most attractors one cloud feels.
pub const MAX_ATTRACTORS: usize = 4;
/// The most colliders one cloud meets: the nearest to the emitter.
pub const MAX_COLLIDERS: usize = 8;

/// A point a GPU cloud is drawn toward ([`GpuParticles::attractors`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuAttractor {
    /// Where it is, in world units - or, with `follow`, where that entity is.
    pub x: f32,
    pub y: f32,
    /// An entity by name to be at, each frame (a whirlwind that wanders).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub follow: String,
    /// How hard it pulls: the share of the way in a particle has come is
    /// `1 - 1 / (1 + strength * age)`.
    pub strength: f32,
    /// Radians per second a particle is turned round it.
    pub swirl: f32,
    /// How far it reaches: its pull falls off as `exp(-(d / radius)^2)`; 0
    /// reaches everywhere.
    pub radius: f32,
}

/// A GPU cloud's world this frame ([`GpuParticles::world`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GpuWorld {
    /// Each attractor: x, y, strength, swirl; and its radius.
    pub attractors: Vec<([f32; 4], f32)>,
    pub colliders: Vec<GpuCollider>,
}

/// A collider as a GPU cloud meets it: a turned box, or a circle.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GpuCollider {
    pub centre: Vec2,
    /// Half its width and height; a circle's radius in `x`.
    pub half: Vec2,
    pub rotation: f32,
    pub circle: bool,
    /// How it moves, from its `Velocity2D`: a stateful cloud is pushed by it.
    pub velocity: Vec2,
}

impl Default for GpuParticles {
    fn default() -> Self {
        Self {
            count: 10_000,
            lifetime: [1.5, 3.0],
            speed: [40.0, 120.0],
            angle: [0.0, std::f32::consts::TAU],
            arms: 0,
            radius: 0.0,
            edge: false,
            gravity: [0.0, 0.0],
            drag: 0.0,
            swirl: 0.0,
            attract: 0.0,
            noise: 0.0,
            noise_speed: 2.0,
            size: [4.0, 1.0],
            stretch: 0.0,
            color_start: [1.0, 1.0, 1.0, 0.0],
            color_mid: [1.0, 1.0, 1.0, 1.0],
            color_end: [1.0, 1.0, 1.0, 0.0],
            texture_id: Default::default(),
            blend_mode: crate::renderer::material::BlendMode::Additive,
            z_order: 0,
            sort_layer: None,
            seed: 0,
            playing: true,
            time: 0.0,
            attractors: Vec::new(),
            collide: false,
            pile: 0.0,
            simulate: GpuSimulation::Closed,
            bounce: DEFAULT_BOUNCE,
            friction: DEFAULT_FRICTION,
            inherit: 0.0,
            world: GpuWorld::default(),
        }
    }
}

/// What the vertex shader reads (`gpu_particles.wgsl`), one per draw.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "gpu-wgpu", derive(Pod, Zeroable))]
pub struct GpuParticleUniform {
    pub view_projection: [[f32; 4]; 4],
    /// Emitter origin x, y, rotation, scale.
    pub xf: [f32; 4],
    /// Time, count, seed, and the emitter's y scale over its x scale (a
    /// cloud squashed onto a floor in perspective).
    pub clock: [f32; 4],
    /// Lifetime min, max, speed min, max.
    pub life: [f32; 4],
    /// Angle min, max, gravity x, y.
    pub aim: [f32; 4],
    /// Drag, swirl, attract, noise.
    pub motion: [f32; 4],
    /// Spawn radius, edge, noise speed, stretch.
    pub shape: [f32; 4],
    /// Size at birth, at death, the number of arms.
    pub size: [f32; 4],
    pub c0: [f32; 4],
    pub c1: [f32; 4],
    pub c2: [f32; 4],
    /// How many attractors and colliders follow.
    pub counts: [f32; 4],
    /// Attractors in the emitter's space: x, y, strength, swirl; and radius.
    pub att: [[f32; 4]; MAX_ATTRACTORS],
    pub att2: [[f32; 4]; MAX_ATTRACTORS],
    /// Colliders in the emitter's space: centre x, y, half width, height (a
    /// circle's radius in z); and the cosine and sine of the turn, circle.
    pub col: [[f32; 4]; MAX_COLLIDERS],
    pub col2: [[f32; 4]; MAX_COLLIDERS],
}

impl GpuParticles {
    /// Whether it keeps state (`simulate: "state"`).
    pub fn stateful(&self) -> bool {
        self.simulate == GpuSimulation::State
    }

    /// The uniform for drawing at `origin` (turned by `rotation`, scaled by
    /// `scale`) through `view_projection`.
    pub fn uniform(&self, origin: Vec2, rotation: f32, scale: Vec2, view_projection: [[f32; 4]; 4]) -> GpuParticleUniform {
        let ratio = if scale.x.abs() > 1e-6 { scale.y / scale.x } else { 1.0 };
        // The world into the emitter's space: placed, turned and scaled back.
        let sx = if scale.x.abs() > 1e-6 { scale.x } else { 1.0 };
        let sy = sx * if ratio.abs() > 1e-6 { ratio } else { 1.0 };
        let (sn, cs) = (-rotation).sin_cos();
        let local = |w: Vec2| {
            let d = w - origin;
            Vec2::new((d.x * cs - d.y * sn) / sx, (d.x * sn + d.y * cs) / sy)
        };
        let mut att = [[0.0; 4]; MAX_ATTRACTORS];
        let mut att2 = [[0.0; 4]; MAX_ATTRACTORS];
        let na = self.world.attractors.len().min(MAX_ATTRACTORS);
        for (j, (a, radius)) in self.world.attractors.iter().take(na).enumerate() {
            let p = local(Vec2::new(a[0], a[1]));
            att[j] = [p.x, p.y, a[2], a[3]];
            att2[j] = [radius / sx.abs(), 0.0, 0.0, 0.0];
        }
        let mut col = [[0.0; 4]; MAX_COLLIDERS];
        let mut col2 = [[0.0; 4]; MAX_COLLIDERS];
        let nc = self.world.colliders.len().min(MAX_COLLIDERS);
        for (j, c) in self.world.colliders.iter().take(nc).enumerate() {
            let p = local(c.centre);
            let turn = c.rotation - rotation;
            col[j] = [p.x, p.y, c.half.x / sx.abs(), c.half.y / sy.abs()];
            col2[j] = [turn.cos(), turn.sin(), if c.circle { 1.0 } else { 0.0 }, 0.0];
        }
        GpuParticleUniform {
            view_projection,
            xf: [origin.x, origin.y, rotation, scale.x],
            clock: [self.time, self.count as f32, (self.seed % 65536) as f32, ratio],
            life: [self.lifetime[0].max(0.01), self.lifetime[1].max(self.lifetime[0]).max(0.01), self.speed[0], self.speed[1]],
            aim: [self.angle[0], self.angle[1], self.gravity[0], self.gravity[1]],
            motion: [self.drag.max(0.0), self.swirl, self.attract.max(0.0), self.noise],
            shape: [self.radius.max(0.0), if self.edge { 1.0 } else { 0.0 }, self.noise_speed, self.stretch.max(0.0)],
            size: [self.size[0], self.size[1], self.arms as f32, 0.0],
            c0: self.color_start,
            c1: self.color_mid,
            c2: self.color_end,
            counts: [na as f32, nc as f32, self.pile.max(0.0) / sx.abs(), 0.0],
            att,
            att2,
            col,
            col2,
        }
    }

    /// Particle `i` at time `t`, in the emitter's space: position, size and
    /// colour - exactly what `gpu_particles.wgsl`'s `particle` computes.
    pub fn sample(&self, i: u32, t: f32) -> (Vec2, f32, [f32; 4]) {
        let u = self.uniform(Vec2::ZERO, 0.0, Vec2::ONE, [[0.0; 4]; 4]);
        let u = GpuParticleUniform { clock: [t, u.clock[1], u.clock[2], 0.0], ..u };
        let p = particle(&u, i);
        (p.0, p.1, p.2)
    }
}

/// `Particles.set(e, field, value)` on a GPU emitter: any field by name,
/// through the component's serde, the clock kept. A name it has not got is
/// an error rather than a silent no-op.
pub fn set_field(g: &mut GpuParticles, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut whole = serde_json::to_value(&*g).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not a GPU particle field"));
    }
    // Scripts have only floats: a whole number over a count stays whole.
    let v = match (map.get(field), value) {
        (Some(serde_json::Value::Number(o)), serde_json::Value::Number(n)) if !o.is_f64() => n
            .as_f64()
            .filter(|f| f.fract() == 0.0 && *f >= 0.0)
            .map_or(value.clone(), |f| (f as u64).into()),
        // An empty Lua table arrives as `{}`; for a list (`attractors`) it
        // means none.
        (Some(serde_json::Value::Array(_)), serde_json::Value::Object(o)) if o.is_empty() => serde_json::Value::Array(Vec::new()),
        _ => value.clone(),
    };
    map.insert(field.to_string(), v);
    let time = g.time;
    *g = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    g.time = time;
    Ok(())
}

/// An integer hash, the same in WGSL (`hash_u`): PCG's output permutation.
pub fn hash_u(v: u32) -> u32 {
    let state = v.wrapping_mul(747796405).wrapping_add(2891336453);
    let word = ((state >> ((state >> 28).wrapping_add(4))) ^ state).wrapping_mul(277803737);
    (word >> 22) ^ word
}

fn rand(a: u32, b: u32, c: u32) -> f32 {
    let h = hash_u(a ^ hash_u(b.wrapping_add(hash_u(c))));
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

/// The vertex shader's particle on the CPU: emitter-space position, size,
/// colour and velocity direction.
fn particle(u: &GpuParticleUniform, i: u32) -> (Vec2, f32, [f32; 4], Vec2) {
    let t = u.clock[0];
    let seed = u.clock[2] as u32;
    let life = u.life[0] + (u.life[1] - u.life[0]) * rand(i, seed, 1);
    let phase = rand(i, seed, 2) * life;
    let tt = t + phase;
    let cycle = (tt / life).floor();
    let age = tt - cycle * life;
    let c = cycle as i32 as u32;
    let r3 = rand(i, c ^ seed, 3);
    let a = if u.size[2] >= 1.0 {
        let arms = u.size[2];
        u.aim[0] + (r3 * arms).floor() * std::f32::consts::TAU / arms + (rand(i, c ^ seed, 8) - 0.5) * 0.5
    } else {
        u.aim[0] + (u.aim[1] - u.aim[0]) * r3
    };
    let speed = u.life[2] + (u.life[3] - u.life[2]) * rand(i, c ^ seed, 4);
    let ra = rand(i, c ^ seed, 5) * std::f32::consts::TAU;
    let rr = if u.shape[1] > 0.5 { u.shape[0] } else { u.shape[0] * rand(i, c ^ seed, 6).sqrt() };
    let spawn = Vec2::new(ra.cos(), ra.sin()) * rr;
    let v0 = Vec2::new(a.cos(), a.sin()) * speed;
    let g = Vec2::new(u.aim[2], u.aim[3]);
    let k = u.motion[0];
    let (disp, grav, vel) = if k > 1e-4 {
        let decay = (-k * age).exp();
        let f = (1.0 - decay) / k;
        (v0 * f, g * ((age - f) / k), v0 * decay + g * f)
    } else {
        (v0 * age, g * (0.5 * age * age), v0 + g * age)
    };
    let mut p = spawn + disp + grav;
    p /= 1.0 + u.motion[2] * age;
    let s = u.motion[1] * age;
    let (sn, cs) = s.sin_cos();
    p = Vec2::new(p.x * cs - p.y * sn, p.x * sn + p.y * cs);
    let w = u.shape[2] * age + rand(i, c ^ seed, 7) * std::f32::consts::TAU;
    p += Vec2::new(w.sin(), (w * 1.3).cos()) * (u.motion[3] * (age / life));
    // Attractors: drawn in as the particle ages, and turned round.
    for j in 0..(u.counts[0] as usize).min(MAX_ATTRACTORS) {
        let a = u.att[j];
        let radius = u.att2[j][0];
        let centre = Vec2::new(a[0], a[1]);
        let d = p - centre;
        let fall = if radius > 0.0 { (-d.length_squared() / (radius * radius)).exp() } else { 1.0 };
        let pull = (1.0 - 1.0 / (1.0 + a[2].max(0.0) * age)) * fall;
        let (sn, cs) = (a[3] * age * fall).sin_cos();
        let dd = d * (1.0 - pull);
        p = centre + Vec2::new(dd.x * cs - dd.y * sn, dd.x * sn + dd.y * cs);
    }
    // Colliders: a particle inside one is put out on its nearest side, up to
    // `pile` past it.
    let heap = u.counts[2] * rand(i, c ^ seed, 9);
    for j in 0..(u.counts[1] as usize).min(MAX_COLLIDERS) {
        let s = u.col[j];
        let r = u.col2[j];
        let centre = Vec2::new(s[0], s[1]);
        let d = p - centre;
        if r[2] > 0.5 {
            let l = d.length();
            if l < s[2] {
                p = centre + if l > 1e-5 { d / l * (s[2] + heap) } else { Vec2::new(0.0, s[2] + heap) };
            }
        } else {
            let q = Vec2::new(d.x * r[0] + d.y * r[1], -d.x * r[1] + d.y * r[0]);
            if q.x.abs() < s[2] && q.y.abs() < s[3] {
                let side = |v: f32, h: f32| if v < 0.0 { -h } else { h };
                let q = if s[2] - q.x.abs() < s[3] - q.y.abs() {
                    Vec2::new(side(q.x, s[2] + heap), q.y)
                } else {
                    Vec2::new(q.x, side(q.y, s[3] + heap))
                };
                p = centre + Vec2::new(q.x * r[0] - q.y * r[1], q.x * r[1] + q.y * r[0]);
            }
        }
    }
    let f = age / life;
    let size = u.size[0] + (u.size[1] - u.size[0]) * f;
    let col = |a: [f32; 4], b: [f32; 4], x: f32| [0, 1, 2, 3].map(|j| a[j] + (b[j] - a[j]) * x);
    let color = if f < 0.5 { col(u.c0, u.c1, f * 2.0) } else { col(u.c1, u.c2, f * 2.0 - 1.0) };
    (p, size, color, vel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_particle_is_the_same_every_time() {
        let g = GpuParticles { count: 100, seed: 7, ..Default::default() };
        assert_eq!(g.sample(42, 3.25), g.sample(42, 3.25));
        assert_ne!(g.sample(42, 3.25).0, g.sample(43, 3.25).0, "two particles are two");
    }

    #[test]
    fn with_no_speed_a_particle_stays_on_its_spawn_disc() {
        let g = GpuParticles { speed: [0.0, 0.0], radius: 50.0, ..Default::default() };
        for i in 0..200 {
            let (p, _, _) = g.sample(i, 1.7);
            assert!(p.length() <= 50.001, "{i} at {p}");
        }
        let ring = GpuParticles { edge: true, ..g };
        let (p, _, _) = ring.sample(3, 0.4);
        assert!((p.length() - 50.0).abs() < 0.01, "on the rim: {}", p.length());
    }

    #[test]
    fn attraction_draws_the_cloud_in() {
        let free = GpuParticles { lifetime: [4.0, 4.0], speed: [100.0, 100.0], ..Default::default() };
        let pulled = GpuParticles { attract: 2.0, ..free.clone() };
        let mean = |g: &GpuParticles| (0..500).map(|i| g.sample(i, 10.0).0.length()).sum::<f32>() / 500.0;
        assert!(mean(&pulled) < mean(&free) * 0.6, "{} against {}", mean(&pulled), mean(&free));
    }

    #[test]
    fn arms_launch_along_a_few_directions() {
        let g = GpuParticles { arms: 3, speed: [100.0, 100.0], lifetime: [9.0, 9.0], ..Default::default() };
        for i in 0..300 {
            let (p, _, _) = g.sample(i, 0.5);
            let a = p.y.atan2(p.x).rem_euclid(std::f32::consts::TAU / 3.0);
            let off = a.min(std::f32::consts::TAU / 3.0 - a);
            assert!(off < 0.3, "{i} launched {off} off an arm");
        }
    }

    #[test]
    fn a_particle_is_reborn_with_a_new_direction() {
        let g = GpuParticles { lifetime: [1.0, 1.0], speed: [100.0, 100.0], ..Default::default() };
        // A whole lifetime apart, the age is the same and the cycle is not.
        let (a, _, _) = g.sample(5, 0.3);
        let (b, _, _) = g.sample(5, 1.3);
        assert!((a.length() - b.length()).abs() < 0.01, "same age, same distance");
        assert!(a.distance(b) > 1.0, "but another direction: {a} {b}");
    }
}
