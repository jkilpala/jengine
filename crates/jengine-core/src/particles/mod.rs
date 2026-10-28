//! Particle system — configurable emitters with pooled simulation and rendering.

pub mod asset;

pub use crate::ecs::outlines::{ColliderOutlines, SurfaceHit};

pub mod gpu;
pub mod gpu_state;

use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::renderer::material::BlendMode;
use std::collections::HashMap;

/// A single particle (internal, not serialized).
#[derive(Debug, Clone)]
struct Particle {
    position: Vec2,
    velocity: Vec2,
    lifetime: f32,
    age: f32,
    color_start: [f32; 4],
    color_end: [f32; 4],
    size_start: f32,
    size_end: f32,
    rotation: f32,
    angular_velocity: f32,
    /// Where it has been, for its emitter's `trail`.
    trail: TrailPoints,
    /// Its own draw in 0..1, picked at birth - which frame a `flipbook`
    /// starts on, or holds.
    seed: f32,
}

/// The most points a particle trail keeps ([`ParticleTrail::length`]).
pub const MAX_TRAIL_POINTS: usize = 16;

/// A particle's recent positions: a ring of up to [`MAX_TRAIL_POINTS`],
/// recorded every [`ParticleTrail::interval`]. Inline in the particle, so the
/// pool's swap-on-death carries it along.
#[derive(Debug, Clone, Copy, Default)]
struct TrailPoints {
    points: [Vec2; MAX_TRAIL_POINTS],
    /// Where the next point goes.
    head: u8,
    /// How many are recorded.
    len: u8,
    /// Seconds since the last point.
    since: f32,
}

impl TrailPoints {
    fn push(&mut self, p: Vec2) {
        self.points[self.head as usize] = p;
        self.head = ((self.head as usize + 1) % MAX_TRAIL_POINTS) as u8;
        self.len = (self.len as usize + 1).min(MAX_TRAIL_POINTS) as u8;
        self.since = 0.0;
    }

    /// The last `keep` points, oldest first.
    fn oldest_first(&self, keep: usize) -> impl Iterator<Item = Vec2> + '_ {
        let n = (self.len as usize).min(keep);
        (0..n).map(move |k| {
            let back = n - k;
            self.points[(self.head as usize + MAX_TRAIL_POINTS - back) % MAX_TRAIL_POINTS]
        })
    }
}

/// How a [`ParticleFlipbook`] moves through its frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FlipbookMode {
    /// Every frame over the particle's life, `cycles` times - a puff that
    /// blooms and fades however long it lives.
    #[default]
    Life,
    /// `fps` frames a second, looping - a flame that flickers at its own
    /// pace whatever its lifetime.
    Fps,
    /// One frame, picked at birth and held - a handful of different debris
    /// shapes from one sheet.
    Random,
}

/// A texture sheet animated on each particle (Oct 2026; Unity's Texture
/// Sheet Animation, Godot's particle `anim_speed` / `anim_offset`): the
/// emitter's texture cut into `columns` x `rows` frames, read left to right
/// and top down, `frames` of them used (0 for all). `random_start` begins
/// each particle on a frame of its own, so a cloud of them does not flicker
/// in step.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParticleFlipbook {
    pub columns: u32,
    pub rows: u32,
    pub frames: u32,
    pub mode: FlipbookMode,
    /// Frames a second, for `fps`.
    pub fps: f32,
    /// Times through the frames over a life, for `life`.
    pub cycles: f32,
    pub random_start: bool,
}

impl Default for ParticleFlipbook {
    fn default() -> Self {
        Self { columns: 1, rows: 1, frames: 0, mode: FlipbookMode::Life, fps: 12.0, cycles: 1.0, random_start: false }
    }
}

impl ParticleFlipbook {
    /// How many frames it plays.
    pub fn frame_count(&self) -> u32 {
        let all = self.columns.max(1) * self.rows.max(1);
        if self.frames == 0 { all } else { self.frames.min(all) }
    }

    /// The frame a particle `age` seconds into a life of `lifetime` shows,
    /// given its own `seed` (0..1).
    pub fn frame(&self, age: f32, lifetime: f32, seed: f32) -> u32 {
        let n = self.frame_count();
        let start = if self.random_start || self.mode == FlipbookMode::Random { (seed * n as f32) as u32 } else { 0 };
        let step = match self.mode {
            FlipbookMode::Life => ((age / lifetime.max(1e-4)).clamp(0.0, 0.9999) * n as f32 * self.cycles.max(0.0)) as u32,
            FlipbookMode::Fps => (age.max(0.0) * self.fps.max(0.0)) as u32,
            FlipbookMode::Random => 0,
        };
        (start + step) % n
    }

    /// The texture rectangle of frame `f`: `[u_min, v_min, u_max, v_max]`,
    /// `v` from the top.
    pub fn uv(&self, f: u32) -> [f32; 4] {
        let (c, r) = (self.columns.max(1), self.rows.max(1));
        let (x, y) = ((f % c) as f32, ((f / c) % r) as f32);
        [x / c as f32, y / r as f32, (x + 1.0) / c as f32, (y + 1.0) / r as f32]
    }
}

/// A ribbon behind each particle through where it has been (Oct 2026;
/// Godot's particle trails, Unity's Trails module): streaks, fireworks,
/// comets, sparks that draw lines. The trail is recorded per particle - a
/// point every `interval`, the last `length` kept - so it follows the path
/// the particle really took, bounces, wind and turbulence included.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParticleTrail {
    /// Points kept, 2 to 16: with `interval`, how long a trail lasts.
    pub length: u32,
    /// Seconds between two recorded points.
    pub interval: f32,
    /// The ribbon's width at the head, as a share of the particle's size.
    pub width: f32,
    /// Narrow to nothing at the tail.
    pub taper: bool,
    /// Fade to nothing at the tail.
    pub fade: bool,
}

impl Default for ParticleTrail {
    fn default() -> Self {
        Self { length: 8, interval: 0.03, width: 0.8, taper: true, fade: true }
    }
}

/// `light` on an emitter: its particles give off light (Oct 2026; Unity's
/// Lights module) - sparks that light the ground they fall past, fireflies,
/// embers. Each is a point light that casts no shadow, placed and coloured
/// as the particle is drawn, gathered with the scene's other lights by
/// `renderer::lighting::gather_lights` - so the GPU and `light_at` both see
/// it. GPU particles carry none.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParticleLights {
    /// One particle in `every` carries a light, chosen by its own seed - so a
    /// particle has one, or not, for its whole life.
    pub every: u32,
    /// At most this many an emitter, the oldest particles first.
    pub max: u32,
    /// The light's radius - times the particle's size over its size at birth
    /// with `scale_with_size`, so a shrinking ember's light shrinks.
    pub radius: f32,
    pub intensity: f32,
    pub falloff: f32,
    /// The particle's colour, or `color`.
    pub use_particle_color: bool,
    pub color: [f32; 3],
    pub scale_with_size: bool,
    /// Dimmed by the particle's alpha, so a fading particle's light fades.
    pub fade: bool,
}

impl Default for ParticleLights {
    fn default() -> Self {
        Self {
            every: 1,
            max: 64,
            radius: 80.0,
            intensity: 1.0,
            falloff: 2.0,
            use_particle_color: true,
            color: [1.0, 1.0, 1.0],
            scale_with_size: true,
            fade: true,
        }
    }
}

/// One particle's light this frame (`ParticleEmitter::lit`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleLight {
    pub position: Vec2,
    pub color: [f32; 3],
    pub intensity: f32,
    pub radius: f32,
    pub falloff: f32,
}

/// Particle emitter component — configures how particles spawn and behave.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticleEmitter {
    /// Optional reference to a `.particle` asset that supplies the emitter
    /// settings. When set, the asset's fields are inlined into this component
    /// by `particles::asset::resolve_particle_sources` before the scene is
    /// handed to the ECS loader — at which point all of the fields below
    /// reflect the file's contents (unless the entity overrode them in
    /// the scene). Kept on the component for traceability; ignored at
    /// runtime once resolution has run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub particle_source: Option<String>,
    /// Texture to use for particles. None = white quad.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture_id: Option<String>,
    /// Particles spawned per second (continuous mode).
    #[serde(default = "default_emission_rate")]
    pub emission_rate: f32,
    /// Maximum number of alive particles in the pool.
    #[serde(default = "default_max_particles")]
    pub max_particles: u32,
    /// Particle lifetime range [min, max] in seconds.
    #[serde(default = "default_lifetime")]
    pub lifetime: [f32; 2],
    /// Initial speed range [min, max].
    #[serde(default = "default_speed")]
    pub speed: [f32; 2],
    /// Emission angle range [min, max] in radians. 0 = right, PI/2 = up.
    #[serde(default = "default_angle")]
    pub angle: [f32; 2],
    /// Start color RGBA.
    #[serde(default = "default_white")]
    pub color_start: [f32; 4],
    /// End color RGBA (interpolated over lifetime).
    #[serde(default = "default_white_transparent")]
    pub color_end: [f32; 4],
    /// Colours a particle is born in, one picked at random each: the start
    /// is the pick at `color_start`'s alpha, the end the pick at
    /// `color_end`'s - confetti from one emitter, where it took one emitter
    /// a colour. Empty: `color_start` /
    /// `color_end` for every particle, as before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub color_choices: Vec<[f32; 4]>,
    /// Start size range [min, max].
    #[serde(default = "default_size_start")]
    pub size_start: [f32; 2],
    /// End size range [min, max].
    #[serde(default = "default_size_end")]
    pub size_end: [f32; 2],
    /// Gravity multiplier (0 = no gravity, 1 = full gravity).
    #[serde(default)]
    pub gravity_modifier: f32,
    /// Angular velocity range [min, max] in radians/sec.
    #[serde(default)]
    pub angular_velocity: [f32; 2],
    /// If > 0, this emitter bursts instead of emitting continuously: it
    /// spawns this many particles at once when it becomes active - on spawn,
    /// or each time `active` goes from false to true - and nothing in between.
    #[serde(default)]
    pub burst_count: u32,
    /// Whether the emitter is actively spawning particles.
    #[serde(default = "default_true")]
    pub active: bool,
    /// If true, particles simulate in world space (unaffected by emitter movement).
    #[serde(default = "default_true")]
    pub world_space: bool,
    /// Draw order for particle layer.
    #[serde(default)]
    pub z_order: i32,
    /// Optional sort-layer name (see `SortLayers`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_layer: Option<String>,
    /// Where on the emitter a particle starts. A point (the default, and
    /// the only choice before Sep 2026), a disc, a rectangle or a line - so
    /// rain across a sky is one emitter, not the row of nine a street
    /// scene needed before it existed.
    #[serde(default, skip_serializing_if = "EmitShape::is_point")]
    pub emit_shape: EmitShape,
    /// How particles blend with what is under them. `Additive` is what fire,
    /// sparks and magic want: overlapping particles add up to a bright core
    /// instead of the frontmost hiding the rest. Particles were always alpha
    /// until Sep 2026 - the renderer said so in a comment and nothing else.
    #[serde(default, skip_serializing_if = "is_alpha")]
    pub blend_mode: BlendMode,
    /// Colour over a particle's life, as `[t, r, g, b, a]` keys with `t` in
    /// 0..1, interpolated linearly. When present it replaces the
    /// `color_start` -> `color_end` fade, which is the only way to say "fade
    /// in, then out" - a dust mote that pops in at full alpha reads as a
    /// glitch. Colours may exceed 1.0 in an HDR scene.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub color_curve: Vec<[f32; 5]>,
    /// Size over a particle's life, as `[t, multiplier]` keys: the
    /// particle's start size times the curve. Replaces the
    /// `size_start` -> `size_end` fade when present.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub size_curve: Vec<[f32; 2]>,
    /// Stretch each particle along its velocity: its length is its size
    /// plus the distance it covers in `stretch` seconds, and it is turned to
    /// face where it is going. `0` (the default) draws square, spinning
    /// particles as before. Sparks, rain and tracers want 0.02 - 0.08.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub stretch: f32,
    /// Air resistance, per second: a particle's velocity falls by `e^(-drag
    /// * t)`, whatever the frame rate. `0` (the default) never slows. A
    /// firework burst wants 1.5 - 3: out fast, then hanging and falling; a
    /// puff of dust wants more.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub drag: f32,
    /// A swirling flow the particles drift along - smoke that curls, sparks
    /// that eddy, magic that stirs. See [`Turbulence`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turbulence: Option<Turbulence>,
    /// What the particles do when they meet a `collider` - rain on a roof,
    /// sparks off a wall. `None` (the default) passes through everything,
    /// as particles always did. See [`ParticleCollision`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision: Option<ParticleCollision>,
    /// Other emitters, named by entity, that burst where these particles
    /// die or hit something - a splash where a raindrop lands, embers where
    /// a firework shell bursts. See [`SubEmitter`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub_emitters: Vec<SubEmitter>,
    /// A ribbon behind each particle through where it has been
    /// ([`ParticleTrail`]). None for none. GPU particles have none - they
    /// keep no history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trail: Option<ParticleTrail>,
    /// A texture sheet each particle plays through ([`ParticleFlipbook`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flipbook: Option<ParticleFlipbook>,
    /// Light from the particles ([`ParticleLights`]). None for none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<ParticleLights>,
    /// This frame's particle lights, left by [`tick_particles`] for
    /// `gather_lights`. Never saved.
    #[serde(skip)]
    pub lit: Vec<ParticleLight>,
    /// A `sprite` emit shape's pixels, read from the texture by
    /// [`collect_emit_pixels`]. Never saved.
    #[serde(skip)]
    pub emit_pixels: Option<std::sync::Arc<SpritePixels>>,
}

/// How an emitter's particles meet the scene's colliders
/// ([`ColliderOutlines`]): every non-sensor `collider` whose
/// `collision_membership` shares a bit with `mask`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParticleCollision {
    /// What a particle does when it hits.
    pub response: CollisionResponse,
    /// On a bounce, the share of its speed into the surface a particle keeps
    /// coming off it: `0` stops dead, `1` bounces as hard as it hit.
    pub bounce: f32,
    /// On a bounce, the share of its speed along the surface it loses.
    pub friction: f32,
    /// The collision layers it hits.
    pub mask: u16,
}

impl Default for ParticleCollision {
    fn default() -> Self {
        Self { response: CollisionResponse::Kill, bounce: 0.4, friction: 0.2, mask: u16::MAX }
    }
}

/// What a particle does on meeting a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CollisionResponse {
    /// It dies there - rain, which a sub-emitter turns into a splash.
    #[default]
    Kill,
    /// It bounces off - sparks, shell casings, hail.
    Bounce,
}

/// Another emitter that bursts where this one's particles die or hit. The
/// target is an ordinary emitter found by its entity's name - usually one
/// with `emission_rate: 0` that does nothing else - and its particles are
/// its own: its texture, colours, speeds and angles, spawned from its shape
/// about the point where the event happened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubEmitter {
    /// When it bursts.
    #[serde(default)]
    pub on: SubEmitterEvent,
    /// The entity name of the emitter to burst.
    pub emitter: String,
    /// How many particles each time.
    #[serde(default = "default_sub_count")]
    pub count: u32,
    /// The chance, 0..1, that an event bursts at all - so not every
    /// raindrop splashes.
    #[serde(default = "default_sub_chance")]
    pub chance: f32,
}

fn default_sub_count() -> u32 {
    4
}

fn default_sub_chance() -> f32 {
    1.0
}

/// What sets a [`SubEmitter`] off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SubEmitterEvent {
    /// Every death: its lifetime ending, or a collision that kills it.
    #[default]
    Death,
    /// Every time it hits a surface, bounce or kill.
    Collision,
}

/// A flow field particles are carried along, on top of their own velocity:
/// the **curl** of smooth noise, so it swirls and never piles particles up
/// or pulls them apart - smoke curls into eddies instead of clumping.
/// Anchored to the world, so neighbouring particles move together, and two
/// emitters with the same settings share one flow.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Turbulence {
    /// The flow's speed, in world units per second.
    pub strength: f32,
    /// The size of one swirl, in world units.
    pub scale: f32,
    /// How fast the flow changes, in swirls per second: `0` is a still
    /// river of eddies, `1` churns.
    pub speed: f32,
}

impl Default for Turbulence {
    fn default() -> Self {
        Self { strength: 60.0, scale: 120.0, speed: 0.3 }
    }
}

impl Turbulence {
    /// The flow at `p` at time `t`: the curl of a noise potential, so it is
    /// divergence-free. The size of the result is about `strength`.
    pub fn flow(&self, p: Vec2, t: f32) -> Vec2 {
        let scale = self.scale.max(1e-3);
        let q = p / scale;
        let e = 0.01;
        let psi = |x: f32, y: f32| {
            // Two octaves, each drifting its own way through time.
            let a = value_noise(x + t * self.speed, y - t * self.speed * 0.7);
            let b = value_noise(x * 2.03 - t * self.speed * 1.3 + 17.1, y * 2.03 + t * self.speed * 0.9 + 3.7);
            a + 0.5 * b
        };
        let dx = (psi(q.x + e, q.y) - psi(q.x - e, q.y)) / (2.0 * e);
        let dy = (psi(q.x, q.y + e) - psi(q.x, q.y - e)) / (2.0 * e);
        // (dpsi/dy, -dpsi/dx): the curl. Value noise's slope runs to about
        // 1.5 a cell; halving it puts the flow's size near `strength`.
        Vec2::new(dy, -dx) * (self.strength * 0.5)
    }
}

/// Smooth value noise on the integer lattice, 0..1, quintic-eased so its
/// slope is continuous - the flow is built from the slope.
fn value_noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let ease = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (ux, uy) = (ease(fx), ease(fy));
    let h = |cx: f32, cy: f32| {
        let mut n = (cx as i32 as u32).wrapping_mul(0x27d4_eb2d) ^ (cy as i32 as u32).wrapping_mul(0x1656_67b1);
        n ^= n >> 15;
        n = n.wrapping_mul(0x85eb_ca6b);
        n ^= n >> 13;
        (n & 0x00ff_ffff) as f32 / 16_777_215.0
    };
    let a = h(ix, iy);
    let b = h(ix + 1.0, iy);
    let c = h(ix, iy + 1.0);
    let d = h(ix + 1.0, iy + 1.0);
    let top = a + (b - a) * ux;
    let bottom = c + (d - c) * ux;
    top + (bottom - top) * uy
}

fn is_alpha(b: &BlendMode) -> bool {
    *b == BlendMode::Alpha
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

/// Linear interpolation through `[t, ...]` keys, clamped at both ends.
/// Keys need not be sorted in the file; they are read in order of `t`.
fn sample_curve<const N: usize>(keys: &[[f32; N]], t: f32) -> [f32; N] {
    let mut sorted: Vec<&[f32; N]> = keys.iter().collect();
    sorted.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let first = sorted[0];
    if t <= first[0] {
        return *first;
    }
    for w in sorted.windows(2) {
        let (a, b) = (w[0], w[1]);
        if t <= b[0] {
            let span = (b[0] - a[0]).max(1e-6);
            let f = (t - a[0]) / span;
            let mut out = *a;
            for i in 1..N {
                out[i] = a[i] + (b[i] - a[i]) * f;
            }
            out[0] = t;
            return out;
        }
    }
    **sorted.last().expect("non-empty")
}

/// Where a [`ParticleEmitter`] starts its particles, about the emitter's
/// position. Offsets are uniform over the shape.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum EmitShape {
    #[default]
    Point,
    /// A disc of `radius`; with `edge`, only its rim - a ring.
    Circle {
        radius: f32,
        #[serde(default)]
        edge: bool,
    },
    /// A rectangle `size` wide and tall, centred on the emitter.
    Rect { size: [f32; 2] },
    /// A horizontal line `length` long, centred on the emitter - a sky.
    Line { length: f32 },
    /// Along the `vector_path` on the emitter's own entity, evenly by length
    /// (Oct 2026) - a fuse burning down a line, sparks off a neon sign, spray
    /// along a coast. Where the path is drawn, turn and scale included.
    Path,
    /// Over the opaque pixels of the sprite on the emitter's own entity -
    /// alpha above `cutoff` - or with `edge` only those on its outline (Oct
    /// 2026): a dissolve, a burning silhouette, a ghost giving off mist. The
    /// pixels are read from what the sprite draws (`collect_emit_pixels`) -
    /// a painted or carved copy before its texture - read again whenever
    /// those pixels change, and follow the sprite's turn and scale. With
    /// nothing opaque it emits nothing.
    Sprite {
        #[serde(default)]
        edge: bool,
        #[serde(default = "default_emit_cutoff")]
        cutoff: f32,
    },
}

fn default_emit_cutoff() -> f32 {
    0.5
}

/// Where a `path` or `sprite` emit shape starts particles this frame, as
/// offsets from the emitter's world position - built by [`tick_particles`]
/// from the entity's path or its sprite's pixels and its world transform.
#[derive(Debug, Clone)]
pub enum EmitGeometry {
    /// A polyline and the distance along it at each point.
    Polyline { points: Vec<Vec2>, along: Vec<f32> },
    /// Points in the sprite's local space, mapped by `xf`'s linear part, each
    /// jittered within one texel (`jitter`, local).
    Pixels { points: std::sync::Arc<Vec<Vec2>>, xf: crate::ecs::components::WorldTransform, jitter: Vec2 },
}

impl EmitGeometry {
    /// An offset drawn from the geometry - evenly along a polyline, any
    /// pixel of a sprite.
    fn sample(&self, rng: &mut SimpleRng) -> Vec2 {
        match self {
            EmitGeometry::Polyline { points, along } => {
                let total = along.last().copied().unwrap_or(0.0);
                if points.len() < 2 || total <= 0.0 {
                    return points.first().copied().unwrap_or(Vec2::ZERO);
                }
                let d = rng.range(0.0, total);
                let i = along.partition_point(|&a| a <= d).clamp(1, points.len() - 1);
                let (a, b) = (points[i - 1], points[i]);
                let seg = (along[i] - along[i - 1]).max(1e-6);
                a + (b - a) * ((d - along[i - 1]) / seg)
            }
            EmitGeometry::Pixels { points, xf, jitter } => {
                if points.is_empty() {
                    return Vec2::ZERO;
                }
                let k = ((rng.range(0.0, 1.0) * points.len() as f32) as usize).min(points.len() - 1);
                let local = points[k] + Vec2::new(rng.range(-0.5, 0.5) * jitter.x, rng.range(-0.5, 0.5) * jitter.y);
                xf.apply_linear(local)
            }
        }
    }

    /// A polyline from world points, as offsets from `origin`.
    pub fn polyline(world_points: &[Vec2], origin: Vec2) -> Self {
        let points: Vec<Vec2> = world_points.iter().map(|p| *p - origin).collect();
        let mut along = Vec::with_capacity(points.len());
        let mut d = 0.0;
        for (i, p) in points.iter().enumerate() {
            if i > 0 {
                d += p.distance(points[i - 1]);
            }
            along.push(d);
        }
        EmitGeometry::Polyline { points, along }
    }
}

/// A sprite's opaque pixels for a `sprite` emit shape: texel centres in the
/// sprite's local space (before its transform), and what they were read
/// from. Kept in the emitter ([`ParticleEmitter::emit_pixels`]), never saved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpritePixels {
    pub key: u64,
    pub points: Vec<Vec2>,
    /// One texel's size in local units, the jitter a particle takes.
    pub texel: Vec2,
}

/// The most pixels a `sprite` emit shape keeps; a bigger sprite keeps an
/// even spread of them.
pub const MAX_EMIT_PIXELS: usize = 4096;

/// Read (or keep) the opaque pixels of every `sprite`-shaped emitter's
/// sprite whose texture is loaded. Called by `carve::apply_carves`, the one
/// per-frame step every host makes with its assets, so no host is wired.
pub fn collect_emit_pixels(world: &mut hecs::World, assets: &crate::asset::AssetManager) {
    use crate::ecs::carve::{Carvable, TexelFrame};
    use crate::ecs::components::SpriteComponent;
    use crate::ecs::carve::{PaintableC, painted_texture};
    for (_, (em, sprite, carved, painted)) in
        world.query_mut::<(&mut ParticleEmitter, &SpriteComponent, Option<&Carvable>, Option<&PaintableC>)>()
    {
        let EmitShape::Sprite { edge, cutoff } = em.emit_shape else { continue };
        // What the sprite draws - a carved or painted copy before its own
        // texture - and that copy as it is now: a fire painted into a canvas
        // moves its flames with it (`texture_revision`).
        let source = carved
            .and_then(|c| c.state.texture.as_deref())
            .or_else(|| painted_texture(painted))
            .unwrap_or_else(|| crate::asset::resolve_texture_id(&sprite.texture_id));
        let Some(tex) = assets.get_texture_bytes(source) else { continue };
        let frame = TexelFrame::of(sprite, tex.width, tex.height);
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            source.hash(&mut h);
            for v in [frame.size.x, frame.size.y, frame.anchor.x, frame.anchor.y, cutoff] {
                v.to_bits().hash(&mut h);
            }
            for v in frame.texels {
                v.to_bits().hash(&mut h);
            }
            (edge, tex.width, tex.height, assets.texture_revision(source)).hash(&mut h);
            h.finish()
        };
        if em.emit_pixels.as_ref().is_some_and(|p| p.key == key) {
            continue;
        }
        let cut = (cutoff.clamp(0.0, 1.0) * 255.0) as u8;
        let [x0, y0, x1, y1] = frame.texels;
        let (lo_x, hi_x) = (x0.min(x1).floor().max(0.0) as i64, x0.max(x1).ceil().min(tex.width as f32) as i64);
        let (lo_y, hi_y) = (y0.min(y1).floor().max(0.0) as i64, y0.max(y1).ceil().min(tex.height as f32) as i64);
        let solid = |x: i64, y: i64| {
            x >= lo_x
                && x < hi_x
                && y >= lo_y
                && y < hi_y
                && tex.rgba.get(((y * tex.width as i64 + x) * 4 + 3) as usize).is_some_and(|&a| a > cut)
        };
        let mut texels = Vec::new();
        for y in lo_y..hi_y {
            for x in lo_x..hi_x {
                if solid(x, y) && (!edge || !(solid(x - 1, y) && solid(x + 1, y) && solid(x, y - 1) && solid(x, y + 1))) {
                    texels.push(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
                }
            }
        }
        // An even spread of a big set, not its first rows.
        let step = texels.len().div_ceil(MAX_EMIT_PIXELS).max(1);
        let points: Vec<Vec2> = texels.iter().step_by(step).map(|t| frame.local_of(*t)).collect();
        let span = Vec2::new((x1 - x0).abs().max(1.0), (y1 - y0).abs().max(1.0));
        em.emit_pixels = Some(std::sync::Arc::new(SpritePixels { key, points, texel: frame.size / span }));
    }
}

impl EmitShape {
    pub fn is_point(&self) -> bool {
        matches!(self, EmitShape::Point)
    }

    /// An offset from the emitter, drawn from the shape.
    fn sample(&self, rng: &mut SimpleRng) -> Vec2 {
        match *self {
            EmitShape::Point => Vec2::ZERO,
            EmitShape::Circle { radius, edge } => {
                let a = rng.range(0.0, std::f32::consts::TAU);
                // Uniform over the disc's area, not bunched at its centre.
                let r = if edge { radius } else { radius * rng.range(0.0, 1.0).sqrt() };
                Vec2::new(a.cos(), a.sin()) * r
            }
            EmitShape::Rect { size } => Vec2::new(
                rng.range(-0.5, 0.5) * size[0],
                rng.range(-0.5, 0.5) * size[1],
            ),
            EmitShape::Line { length } => Vec2::new(rng.range(-0.5, 0.5) * length, 0.0),
            // Their geometry is the entity's ([`EmitGeometry`]); without it
            // - no path, pixels not yet read - the emitter's own point.
            EmitShape::Path | EmitShape::Sprite { .. } => Vec2::ZERO,
        }
    }
}

fn default_emission_rate() -> f32 {
    20.0
}
fn default_max_particles() -> u32 {
    256
}
fn default_lifetime() -> [f32; 2] {
    [0.5, 1.5]
}
fn default_speed() -> [f32; 2] {
    [50.0, 100.0]
}
fn default_angle() -> [f32; 2] {
    [0.0, std::f32::consts::TAU]
}
fn default_white() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}
fn default_white_transparent() -> [f32; 4] {
    [1.0, 1.0, 1.0, 0.0]
}
fn default_size_start() -> [f32; 2] {
    [8.0, 12.0]
}
fn default_size_end() -> [f32; 2] {
    [2.0, 4.0]
}
fn default_true() -> bool {
    true
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            particle_source: None,
            texture_id: None,
            emission_rate: default_emission_rate(),
            max_particles: default_max_particles(),
            lifetime: default_lifetime(),
            speed: default_speed(),
            angle: default_angle(),
            color_start: default_white(),
            color_end: default_white_transparent(),
            color_choices: Vec::new(),
            size_start: default_size_start(),
            size_end: default_size_end(),
            gravity_modifier: 0.0,
            angular_velocity: [0.0, 0.0],
            burst_count: 0,
            active: true,
            world_space: true,
            z_order: 0,
            sort_layer: None,
            emit_shape: EmitShape::Point,
            blend_mode: BlendMode::Alpha,
            color_curve: Vec::new(),
            size_curve: Vec::new(),
            stretch: 0.0,
            drag: 0.0,
            turbulence: None,
            collision: None,
            sub_emitters: Vec::new(),
            trail: None,
            flipbook: None,
            light: None,
            lit: Vec::new(),
            emit_pixels: None,
        }
    }
}

/// Per-emitter particle pool.
struct ParticlePool {
    particles: Vec<Particle>,
    alive_count: usize,
    /// Fractional emission accumulator for sub-frame spawning.
    emit_accumulator: f32,
    /// Whether a `burst_count` emitter still owes its burst. Set while the
    /// emitter is inactive, cleared when the burst fires.
    burst_armed: bool,
    /// Whether particles in this pool are simulated in world space (true) or
    /// local to the emitter (false). When `false`, particle positions are
    /// stored relative to the emitter and the emitter's current world
    /// position is added at draw time, so particles "follow" the emitter.
    /// Captured at pool creation time (and updated on each frame from the
    /// emitter component) so that toggling the field on a live emitter takes
    /// effect for newly spawned particles.
    world_space: bool,
}

impl ParticlePool {
    fn new(max: u32, world_space: bool) -> Self {
        Self {
            particles: Vec::with_capacity(max as usize),
            alive_count: 0,
            emit_accumulator: 0.0,
            burst_armed: true,
            world_space,
        }
    }

    /// Spawn a single particle. Returns false if pool is full.
    fn spawn(&mut self, max: u32, p: Particle) -> bool {
        if self.alive_count < max as usize {
            if self.alive_count < self.particles.len() {
                self.particles[self.alive_count] = p;
            } else {
                self.particles.push(p);
            }
            self.alive_count += 1;
            true
        } else {
            false
        }
    }

    /// Kill particle at index by swapping with the last alive particle.
    fn kill(&mut self, idx: usize) {
        if idx < self.alive_count && self.alive_count > 0 {
            self.alive_count -= 1;
            self.particles.swap(idx, self.alive_count);
        }
    }
}

/// Simple deterministic RNG (xorshift32) for particle randomness.
struct SimpleRng {
    state: u32,
}

impl SimpleRng {
    fn new(seed: u32) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    fn next_u32(&mut self) -> u32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        self.state
    }

    /// Random f32 in [0, 1).
    fn next_f32(&mut self) -> f32 {
        (self.next_u32() & 0x00FF_FFFF) as f32 / 16_777_216.0
    }

    /// Random f32 in [min, max].
    fn range(&mut self, min: f32, max: f32) -> f32 {
        min + self.next_f32() * (max - min)
    }
}

/// Centralized particle system — manages pools for all emitter entities.
pub struct ParticleSystem {
    pools: HashMap<u64, ParticlePool>,
    rng: SimpleRng,
    gravity: Vec2,
    /// Total alive particle count (updated after each tick).
    total_alive: u32,
    /// Seconds simulated, for the turbulence field's drift.
    time: f32,
    /// The area effects whose `force` blows particles about
    /// (`ecs::area_effect::wind_at`), set by [`tick_particles`].
    pub winds: Vec<crate::ecs::area_effect::PlacedArea>,
    /// Each `path` / `sprite` shaped emitter's [`EmitGeometry`] this frame,
    /// by entity, set by [`tick_particles`].
    pub emit_geometry: HashMap<u64, EmitGeometry>,
    /// The floating origin this system last followed
    /// (`ecs::floating_origin::follow`).
    origin_seen: Option<(f64, f64)>,
    /// Each emitter's own pace (`ecs::time_scale::factor`) this frame, by
    /// entity, for the ones that keep one - set by [`tick_particles`].
    pub clocks: HashMap<u64, f32>,
}

impl ParticleSystem {
    pub fn new() -> Self {
        Self {
            pools: HashMap::new(),
            rng: SimpleRng::new(42),
            gravity: Vec2::new(0.0, -200.0),
            // Nothing is alive before the first tick. This was `u32::MAX` as
            // a "not computed yet" sentinel, which anything displaying the
            // count showed as four billion particles.
            total_alive: 0,
            time: 0.0,
            winds: Vec::new(),
            emit_geometry: HashMap::new(),
            origin_seen: None,
            clocks: HashMap::new(),
        }
    }

    /// Move every world-space particle (and its trail) back by `by` - the
    /// world was moved back under it (`ecs::floating_origin`). Local pools
    /// ride their emitter.
    pub fn shift(&mut self, by: Vec2) {
        for pool in self.pools.values_mut().filter(|p| p.world_space) {
            for p in &mut pool.particles {
                p.position -= by;
                for q in &mut p.trail.points {
                    *q -= by;
                }
            }
        }
    }

    /// Set the gravity vector used for particles with gravity_modifier > 0.
    pub fn set_gravity(&mut self, gravity: Vec2) {
        self.gravity = gravity;
    }

    /// Get total alive particle count (from last tick).
    pub fn alive_count(&self) -> u32 {
        self.total_alive
    }

    /// Update all particle pools: spawn new particles, simulate existing ones, cull dead.
    ///
    /// `emitters` should contain (entity_id, emitter, emitter_world_position) tuples.
    pub fn update(&mut self, emitters: &[(u64, &ParticleEmitter, Vec2)], dt: f32) {
        self.step(emitters, &[], &ColliderOutlines::default(), dt);
    }

    /// [`Self::update`], with each emitter's entity name (`names[i]` for
    /// `emitters[i]`, which is how a [`SubEmitter`] finds its target) and the
    /// surfaces a [`ParticleCollision`] emitter's particles hit. Hosts call
    /// [`tick_particles`], which gathers both from the world.
    pub fn step(
        &mut self,
        emitters: &[(u64, &ParticleEmitter, Vec2)],
        names: &[Option<&str>],
        surfaces: &ColliderOutlines,
        dt: f32,
    ) {
        // Where sub-emitters burst this frame: (target index, point, count).
        let mut bursts: Vec<(usize, Vec2, u32)> = Vec::new();
        let by_name: HashMap<&str, usize> = if emitters.iter().any(|(_, em, _)| !em.sub_emitters.is_empty()) {
            names.iter().enumerate().filter_map(|(i, n)| n.map(|n| (n, i))).collect()
        } else {
            HashMap::new()
        };
        // Track which entity IDs are still alive
        let active_ids: std::collections::HashSet<u64> = emitters.iter().map(|(id, _, _)| *id).collect();

        // Remove pools for despawned emitter entities
        self.pools.retain(|id, _| active_ids.contains(id));
        self.time += dt;
        let time = self.time;

        for &(entity_id, emitter, world_pos) in emitters {
            // Its own pace (`time_scale`), if it keeps one.
            let dt = dt * self.clocks.get(&entity_id).copied().unwrap_or(1.0);
            let pool = self
                .pools
                .entry(entity_id)
                .or_insert_with(|| ParticlePool::new(emitter.max_particles, emitter.world_space));
            // Sync world_space in case the emitter component was edited
            // since the pool was created. Existing particles keep whatever
            // mode they were spawned with (their stored positions are either
            // absolute or local depending on the pool's mode at spawn time);
            // toggling mid-flight effectively only affects future particles.
            pool.world_space = emitter.world_space;

            // A `sprite` shape with nothing opaque in it (an empty canvas, a
            // fire burnt out) has nowhere to start a particle: none start.
            let barren = matches!(
                self.emit_geometry.get(&entity_id),
                Some(EmitGeometry::Pixels { points, .. }) if points.is_empty()
            );
            // Spawn new particles
            if emitter.burst_count > 0 {
                // `burst_count` was documented and authorable - the editor's
                // Explosion preset uses it - and read by nothing, so a burst
                // emitter emitted nothing at all.
                if !emitter.active {
                    pool.burst_armed = true;
                } else if pool.burst_armed {
                    pool.burst_armed = false;
                    for _ in 0..if barren { 0 } else { emitter.burst_count } {
                        let p = Self::make_particle(&mut self.rng, emitter, world_pos, self.emit_geometry.get(&entity_id));
                        if !pool.spawn(emitter.max_particles, p) {
                            break;
                        }
                    }
                }
            } else if emitter.active {
                // Continuous emission
                pool.emit_accumulator += emitter.emission_rate * dt;
                let to_spawn = pool.emit_accumulator as u32;
                pool.emit_accumulator -= to_spawn as f32;
                for _ in 0..if barren { 0 } else { to_spawn } {
                    let p = Self::make_particle(&mut self.rng, emitter, world_pos, self.emit_geometry.get(&entity_id));
                    if !pool.spawn(emitter.max_particles, p) {
                        break;
                    }
                }
            }

            // Simulate alive particles
            let gravity = self.gravity * emitter.gravity_modifier;
            let keep = if emitter.drag > 0.0 { (-emitter.drag * dt).exp() } else { 1.0 };
            // A local-space pool's positions are offsets; the flow is the
            // world's, so it is read where the particle really is.
            let origin = if pool.world_space { Vec2::ZERO } else { world_pos };
            // The sub-emitters this emitter sets off, resolved once a frame.
            let subs: Vec<(SubEmitterEvent, usize, u32, f32)> = emitter
                .sub_emitters
                .iter()
                .filter_map(|s| by_name.get(s.emitter.as_str()).map(|&t| (s.on, t, s.count, s.chance)))
                .collect();
            let collision = emitter.collision.filter(|_| !surfaces.is_empty());
            let mut i = 0;
            while i < pool.alive_count {
                let p = &mut pool.particles[i];
                p.age += dt;
                if p.age >= p.lifetime {
                    let at = p.position + origin;
                    for &(on, target, count, chance) in &subs {
                        if on == SubEmitterEvent::Death && (chance >= 1.0 || self.rng.next_f32() < chance) {
                            bursts.push((target, at, count));
                        }
                    }
                    pool.kill(i);
                    // Don't increment i — swapped particle needs to be checked
                    continue;
                }
                let from = p.position + origin;
                let wind = if self.winds.is_empty() {
                    Vec2::ZERO
                } else {
                    crate::ecs::area_effect::wind_at(&self.winds, from)
                };
                p.velocity = (p.velocity + (gravity + wind) * dt) * keep;
                p.position += p.velocity * dt;
                if let Some(turb) = &emitter.turbulence {
                    p.position += turb.flow(p.position + origin, time) * dt;
                }
                p.rotation += p.angular_velocity * dt;
                if let Some(c) = collision
                    && let Some(hit) = surfaces.first_hit(from, p.position + origin, c.mask)
                {
                    let killed = c.response == CollisionResponse::Kill;
                    for &(on, target, count, chance) in &subs {
                        let fires = on == SubEmitterEvent::Collision || (killed && on == SubEmitterEvent::Death);
                        if fires && (chance >= 1.0 || self.rng.next_f32() < chance) {
                            bursts.push((target, hit.point, count));
                        }
                    }
                    if killed {
                        pool.kill(i);
                        continue;
                    }
                    // Off the surface: the speed into it turned round and cut
                    // by `bounce`, the speed along it cut by `friction`. Left
                    // a hair above it, so resting on it is a bounce of nothing
                    // every frame rather than falling through.
                    let into = p.velocity.dot(hit.normal) * hit.normal;
                    p.velocity = (p.velocity - into) * (1.0 - c.friction).max(0.0) - into * c.bounce;
                    p.position = hit.point + hit.normal * 0.05 - origin;
                }
                // Where it has been, every `interval` - the first point at once.
                if let Some(tr) = &emitter.trail {
                    p.trail.since += dt;
                    if p.trail.len == 0 || p.trail.since >= tr.interval.max(1e-3) {
                        p.trail.push(p.position);
                    }
                }
                i += 1;
            }
        }

        for (target, at, count) in bursts {
            let (id, emitter, pos) = emitters[target];
            let Some(pool) = self.pools.get_mut(&id) else { continue };
            // A local-space pool keeps offsets from its emitter.
            let shift = if pool.world_space { Vec2::ZERO } else { at - pos };
            for _ in 0..count {
                let mut p = Self::make_particle(&mut self.rng, emitter, at, None);
                p.position += shift;
                if !pool.spawn(emitter.max_particles, p) {
                    break;
                }
            }
        }

        self.total_alive = self.pools.values().map(|p| p.alive_count as u32).sum();
    }

    /// Immediately burst N particles from a specific emitter.
    pub fn burst(&mut self, entity_id: u64, emitter: &ParticleEmitter, world_pos: Vec2, count: u32) {
        let pool = self
            .pools
            .entry(entity_id)
            .or_insert_with(|| ParticlePool::new(emitter.max_particles, emitter.world_space));
        pool.world_space = emitter.world_space;

        for _ in 0..count {
            let p = Self::make_particle(&mut self.rng, emitter, world_pos, self.emit_geometry.get(&entity_id));
            if !pool.spawn(emitter.max_particles, p) {
                break;
            }
        }
    }

    fn make_particle(rng: &mut SimpleRng, emitter: &ParticleEmitter, world_pos: Vec2, geometry: Option<&EmitGeometry>) -> Particle {
        let angle = rng.range(emitter.angle[0], emitter.angle[1]);
        let speed = rng.range(emitter.speed[0], emitter.speed[1]);
        let lifetime = rng.range(emitter.lifetime[0], emitter.lifetime[1]).max(0.01);
        let size_start = rng.range(emitter.size_start[0], emitter.size_start[1]);
        let size_end = rng.range(emitter.size_end[0], emitter.size_end[1]);
        let angular_vel = rng.range(emitter.angular_velocity[0], emitter.angular_velocity[1]);

        // World-space particles are stored at the absolute spawn position so
        // they don't follow a moving emitter. Local-space particles are
        // stored at (0,0) relative to the emitter; the emitter's current
        // world position is added back in `draw_data` at render time.
        let offset = match geometry {
            Some(g) => g.sample(rng),
            None => emitter.emit_shape.sample(rng),
        };
        let position = if emitter.world_space { world_pos + offset } else { offset };
        // A colour of its own only when there are choices, so an emitter
        // without them draws the same random sequence it always did.
        let (color_start, color_end) = match emitter.color_choices.len() {
            0 => (emitter.color_start, emitter.color_end),
            n => {
                let c = emitter.color_choices[((rng.range(0.0, 1.0) * n as f32) as usize).min(n - 1)];
                ([c[0], c[1], c[2], emitter.color_start[3]], [c[0], c[1], c[2], emitter.color_end[3]])
            }
        };

        Particle {
            position,
            velocity: Vec2::new(angle.cos() * speed, angle.sin() * speed),
            lifetime,
            age: 0.0,
            color_start,
            color_end,
            size_start,
            size_end,
            rotation: 0.0,
            angular_velocity: angular_vel,
            trail: TrailPoints::default(),
            seed: rng.range(0.0, 1.0),
        }
    }

    /// Collect draw data for rendering. Returns groups by texture key, sorted by z_order.
    ///
    /// Each entry: `(texture_id, Vec<ParticleDrawData>)` with absolute world
    /// positions ready to draw. For local-space pools (`world_space = false`)
    /// the emitter's current world position is added to each particle's
    /// stored local offset.
    ///
    /// Each emitter tuple: `(entity_id, &ParticleEmitter, emitter_world_position)`.
    /// Pass the same emitter list (with the same positions) used in [`Self::update`]
    /// for consistent results.
    pub fn draw_data(
        &self,
        emitters: &[(u64, &ParticleEmitter, Vec2)],
        sort_layers: &crate::asset::sort_layers::SortLayers,
    ) -> Vec<ParticleGroup> {
        self.draw_data_masked(emitters, sort_layers, &Default::default())
    }

    /// [`Self::draw_data`], with each emitter's mask index (`renderer::mask`)
    /// by entity id; an emitter not in `masks` is unmasked.
    pub fn draw_data_masked(
        &self,
        emitters: &[(u64, &ParticleEmitter, Vec2)],
        sort_layers: &crate::asset::sort_layers::SortLayers,
        masks: &std::collections::HashMap<u64, u32>,
    ) -> Vec<ParticleGroup> {
        // Sort emitters by z_order
        let mut sorted: Vec<_> = emitters.to_vec();
        sorted.sort_by_key(|(_, e, _)| sort_layers.resolve(e.sort_layer.as_deref(), e.z_order));

        // Group by texture and blend.
        let mut groups: Vec<ParticleGroup> = Vec::new();

        for (entity_id, emitter, emitter_pos) in &sorted {
            let pool = match self.pools.get(entity_id) {
                Some(p) => p,
                None => continue,
            };
            if pool.alive_count == 0 {
                continue;
            }

            // Local-space particles are stored relative to the emitter; add
            // the current emitter position to resolve them to world space.
            let offset = if pool.world_space { Vec2::ZERO } else { *emitter_pos };

            // Extend the previous group only when it has the same texture
            // and blend. This used to merge into *any* group with the
            // texture, so emitters A (tex1, z0), B (tex2, z1), C (tex1, z2)
            // drew as [A C] then [B], putting C under B despite its higher
            // z. Merging only neighbours keeps the sorted order and still
            // batches the common case.
            let mask = masks.get(entity_id).copied().unwrap_or(0);
            let fits = groups
                .last()
                .is_some_and(|g| g.texture == emitter.texture_id && g.blend == emitter.blend_mode && g.mask == mask);
            if !fits {
                groups.push(ParticleGroup {
                    texture: emitter.texture_id.clone(),
                    blend: emitter.blend_mode,
                    mask,
                    particles: Vec::new(),
                    trails: Vec::new(),
                });
            }
            let last = groups.last_mut().expect("just ensured");
            let (group, trails) = (&mut last.particles, &mut last.trails);

            for i in 0..pool.alive_count {
                let p = &pool.particles[i];
                let (color, size) = appearance(emitter, p);
                let (length, rotation) = if emitter.stretch > 0.0 && p.velocity.length_squared() > 1e-6 {
                    (size + p.velocity.length() * emitter.stretch, p.velocity.y.atan2(p.velocity.x))
                } else {
                    (size, p.rotation)
                };
                if let Some(tr) = &emitter.trail
                    && p.trail.len > 0
                {
                    // Oldest first, ending where the particle is now.
                    let keep = (tr.length as usize).clamp(2, MAX_TRAIL_POINTS) - 1;
                    let points: Vec<Vec2> = p
                        .trail
                        .oldest_first(keep)
                        .chain(std::iter::once(p.position))
                        .map(|q| q + offset)
                        .collect();
                    trails.push(TrailDrawData { points, width: size * tr.width, color, taper: tr.taper, fade: tr.fade });
                }
                let uv = match &emitter.flipbook {
                    Some(fb) => fb.uv(fb.frame(p.age, p.lifetime, p.seed)),
                    None => [0.0, 0.0, 1.0, 1.0],
                };
                group.push(ParticleDrawData {
                    position: p.position + offset,
                    size,
                    length,
                    rotation,
                    color,
                    uv,
                });
            }
        }

        groups
    }
}

/// Particles that draw together: one texture, one blend, in z order.
#[derive(Debug, Clone)]
pub struct ParticleGroup {
    pub texture: Option<String>,
    pub blend: BlendMode,
    /// The emitter's mask in the frame's `MaskTable` (`renderer::mask`), 0
    /// for none: emitters with different masks never share a group.
    pub mask: u32,
    pub particles: Vec<ParticleDrawData>,
    /// The group's particle trails ([`ParticleTrail`]), drawn under its
    /// particles.
    pub trails: Vec<TrailDrawData>,
}

/// One particle's trail, ready to draw: its points oldest first, ending
/// where the particle is.
#[derive(Debug, Clone)]
pub struct TrailDrawData {
    pub points: Vec<Vec2>,
    /// Width at the head.
    pub width: f32,
    /// The particle's colour.
    pub color: [f32; 4],
    pub taper: bool,
    pub fade: bool,
}

impl TrailDrawData {
    /// The ribbon it draws as - two vertices per point, across the path,
    /// `width` wide at the head and narrowing (`taper`) and fading (`fade`)
    /// to the tail - as positions, texture coordinates (u along, v across,
    /// so a soft round texture gives the ribbon soft edges), colours and
    /// triangles for `WgpuSpriteBatch::draw_mesh`. Each segment is a quad
    /// `a b c, a c d`, one sprite slot.
    pub fn ribbon(&self) -> (Vec<Vec2>, Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<u32>) {
        let n = self.points.len();
        let mut pos = Vec::with_capacity(n * 2);
        let mut uvq = Vec::with_capacity(n * 2);
        let mut col = Vec::with_capacity(n * 2);
        let mut idx = Vec::with_capacity(n.saturating_sub(1) * 6);
        if n < 2 {
            return (pos, uvq, col, idx);
        }
        for i in 0..n {
            let s = i as f32 / (n - 1) as f32;
            let prev = self.points[i.saturating_sub(1)];
            let next = self.points[(i + 1).min(n - 1)];
            let dir = (next - prev).normalize_or_zero();
            let across = Vec2::new(-dir.y, dir.x);
            let half = self.width * 0.5 * if self.taper { s } else { 1.0 };
            let alpha = self.color[3] * if self.fade { s } else { 1.0 };
            let c = [self.color[0], self.color[1], self.color[2], alpha];
            pos.push(self.points[i] + across * half);
            pos.push(self.points[i] - across * half);
            uvq.push([s, 0.0, 1.0]);
            uvq.push([s, 1.0, 1.0]);
            col.push(c);
            col.push(c);
        }
        for i in 0..n as u32 - 1 {
            let (a, b, c, d) = (2 * i, 2 * i + 1, 2 * i + 3, 2 * i + 2);
            idx.extend_from_slice(&[a, b, c, a, c, d]);
        }
        (pos, uvq, col, idx)
    }
}

impl Default for ParticleSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Data needed to draw a single particle.
#[derive(Debug, Clone)]
pub struct ParticleDrawData {
    pub position: Vec2,
    /// Height - and width, unless stretched.
    pub size: f32,
    /// Width along `rotation`: `size`, plus the stretch along velocity.
    pub length: f32,
    pub rotation: f32,
    pub color: [f32; 4],
    /// The texture rectangle it shows, `[u_min, v_min, u_max, v_max]` -
    /// the whole texture, or its `flipbook` frame.
    pub uv: [f32; 4],
}

fn lerp_color(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line emitter spreads its particles along the line; a point one
    /// starts them all in one place.
    #[test]
    fn an_emit_shape_spreads_where_particles_start() {
        let mut sys = ParticleSystem::new();
        let line = ParticleEmitter {
            emission_rate: 0.0,
            speed: [0.0, 0.0],
            lifetime: [5.0, 5.0],
            emit_shape: EmitShape::Line { length: 400.0 },
            ..Default::default()
        };
        sys.burst(1, &line, Vec2::new(100.0, 50.0), 200);
        let xs: Vec<f32> = sys.pools[&1].particles[..sys.pools[&1].alive_count].iter().map(|p| p.position.x).collect();
        let ys: Vec<f32> = sys.pools[&1].particles[..sys.pools[&1].alive_count].iter().map(|p| p.position.y).collect();
        let (min, max) = xs.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        assert!(min >= -100.0 && max <= 300.0 && max - min > 300.0, "spread along the line: {min}..{max}");
        assert!(ys.iter().all(|y| (*y - 50.0).abs() < 1e-4), "and only along it");

        let ring = ParticleEmitter { emit_shape: EmitShape::Circle { radius: 30.0, edge: true }, ..line.clone() };
        sys.burst(2, &ring, Vec2::ZERO, 50);
        assert!(sys.pools[&2].particles[..sys.pools[&2].alive_count].iter().all(|p| (p.position.length() - 30.0).abs() < 0.01));

        let point = ParticleEmitter { emit_shape: EmitShape::Point, ..line };
        sys.burst(3, &point, Vec2::new(7.0, 7.0), 20);
        assert!(sys.pools[&3].particles[..sys.pools[&3].alive_count].iter().all(|p| p.position == Vec2::new(7.0, 7.0)));
    }

    #[test]
    fn an_emit_shape_reads_from_a_scene() {
        let e: ParticleEmitter = serde_json::from_str(r#"{"emit_shape": {"kind": "rect", "size": [100, 20]}}"#).unwrap();
        assert_eq!(e.emit_shape, EmitShape::Rect { size: [100.0, 20.0] });
        let e: ParticleEmitter = serde_json::from_str("{}").unwrap();
        assert!(e.emit_shape.is_point(), "absent is a point, as every existing scene means");
        assert!(!serde_json::to_string(&e).unwrap().contains("emit_shape"), "and saves as it loaded");
    }

    #[test]
    fn a_blend_splits_a_group_and_saves_only_when_set() {
        let mut sys = ParticleSystem::new();
        let a = ParticleEmitter { active: false, ..make_test_emitter() };
        let b = ParticleEmitter { blend_mode: BlendMode::Additive, ..a.clone() };
        sys.burst(1, &a, Vec2::ZERO, 1);
        sys.burst(2, &b, Vec2::ZERO, 1);
        let groups = sys.draw_data(&[(1u64, &a, Vec2::ZERO), (2, &b, Vec2::ZERO)], &Default::default());
        assert_eq!(groups.len(), 2, "one texture, two blends: two draws");
        assert_eq!(groups[1].blend, BlendMode::Additive);

        let e: ParticleEmitter = serde_json::from_str(r#"{"blend_mode": "Additive"}"#).unwrap();
        assert_eq!(e.blend_mode, BlendMode::Additive);
        let plain = serde_json::to_string(&ParticleEmitter::default()).unwrap();
        for key in ["blend_mode", "color_curve", "size_curve", "stretch"] {
            assert!(!plain.contains(key), "{key} saved on an emitter that never set it: {plain}");
        }
    }

    #[test]
    fn a_colour_curve_fades_in_and_out() {
        let mut sys = ParticleSystem::new();
        let e = ParticleEmitter {
            active: false,
            speed: [0.0, 0.0],
            lifetime: [1.0, 1.0],
            color_curve: vec![[0.0, 1.0, 1.0, 1.0, 0.0], [0.5, 1.0, 0.5, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0, 0.0]],
            ..make_test_emitter()
        };
        sys.burst(1, &e, Vec2::ZERO, 1);
        let alpha_at = |sys: &ParticleSystem| sys.draw_data(&[(1u64, &e, Vec2::ZERO)], &Default::default())[0].particles[0].color;
        assert_eq!(alpha_at(&sys)[3], 0.0, "born invisible");
        sys.update(&[(1u64, &e, Vec2::ZERO)], 0.25);
        let c = alpha_at(&sys);
        assert!((c[3] - 0.5).abs() < 1e-4 && (c[1] - 0.75).abs() < 1e-4, "half way up the first key pair: {c:?}");
        sys.update(&[(1u64, &e, Vec2::ZERO)], 0.25);
        assert!((alpha_at(&sys)[3] - 1.0).abs() < 1e-4, "full at the middle key");
    }

    #[test]
    fn a_size_curve_scales_the_start_size() {
        let mut sys = ParticleSystem::new();
        let e = ParticleEmitter {
            active: false,
            speed: [0.0, 0.0],
            size_curve: vec![[0.0, 0.0], [1.0, 2.0]],
            ..make_test_emitter()
        };
        sys.burst(1, &e, Vec2::ZERO, 1);
        sys.update(&[(1u64, &e, Vec2::ZERO)], 0.5);
        let d = &sys.draw_data(&[(1u64, &e, Vec2::ZERO)], &Default::default())[0].particles[0];
        assert!((d.size - 10.0).abs() < 1e-3, "start size 10 x curve 1.0 at t=0.5, got {}", d.size);
    }

    #[test]
    fn stretch_turns_a_particle_along_its_flight() {
        let mut sys = ParticleSystem::new();
        let e = ParticleEmitter {
            active: false,
            speed: [100.0, 100.0],
            angle: [std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2],
            size_start: [4.0, 4.0],
            size_end: [4.0, 4.0],
            stretch: 0.1,
            ..make_test_emitter()
        };
        sys.burst(1, &e, Vec2::ZERO, 1);
        let d = &sys.draw_data(&[(1u64, &e, Vec2::ZERO)], &Default::default())[0].particles[0];
        assert!((d.length - 14.0).abs() < 1e-3, "4 + 100 * 0.1, got {}", d.length);
        assert!((d.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-4, "facing up, got {}", d.rotation);
        assert!((d.size - 4.0).abs() < 1e-4, "and no thicker");
    }

    #[test]
    fn drag_slows_a_particle_the_same_at_any_frame_rate() {
        let travel = |steps: u32| {
            let mut sys = ParticleSystem::new();
            let e = ParticleEmitter { active: false, speed: [100.0, 100.0], drag: 2.0, lifetime: [5.0, 5.0], ..make_test_emitter() };
            sys.burst(1, &e, Vec2::ZERO, 1);
            for _ in 0..steps {
                sys.update(&[(1u64, &e, Vec2::ZERO)], 1.0 / steps as f32);
            }
            sys.draw_data(&[(1u64, &e, Vec2::ZERO)], &Default::default())[0].particles[0].position.x
        };
        // x = 100 / 2 * (1 - e^-2) = 43.2 over a second, at any step.
        let (slow, fast) = (travel(30), travel(240));
        assert!((slow - fast).abs() < 1.5, "30 fps {slow} vs 240 fps {fast}");
        assert!((fast - 43.2).abs() < 1.0, "and it slowed as drag says: {fast}");
    }

    #[test]
    fn turbulence_swirls_without_piling_particles_up() {
        let t = Turbulence { strength: 50.0, scale: 100.0, speed: 0.0 };
        // It moves things, by about its strength ...
        let mut total = 0.0;
        let mut n = 0.0;
        for i in 0..40 {
            for j in 0..40 {
                total += t.flow(Vec2::new(i as f32 * 13.0, j as f32 * 17.0), 0.0).length();
                n += 1.0;
            }
        }
        let mean = total / n;
        assert!(mean > 10.0 && mean < 80.0, "mean speed {mean}");
        // ... and it is a curl: what flows into a small box flows out of it.
        let h = 1.0;
        for (x, y) in [(10.0, 20.0), (230.0, -75.0), (-310.0, 145.0)] {
            let div = (t.flow(Vec2::new(x + h, y), 0.0).x - t.flow(Vec2::new(x - h, y), 0.0).x
                + t.flow(Vec2::new(x, y + h), 0.0).y - t.flow(Vec2::new(x, y - h), 0.0).y)
                / (2.0 * h);
            assert!(div.abs() < 0.05 * mean, "divergence {div} at ({x}, {y})");
        }
    }

    fn make_test_emitter() -> ParticleEmitter {
        ParticleEmitter {
            emission_rate: 100.0,
            max_particles: 10,
            lifetime: [1.0, 1.0],
            speed: [50.0, 50.0],
            angle: [0.0, 0.0], // all go right
            color_start: [1.0, 0.0, 0.0, 1.0],
            color_end: [1.0, 0.0, 0.0, 0.0],
            size_start: [10.0, 10.0],
            size_end: [2.0, 2.0],
            gravity_modifier: 0.0,
            angular_velocity: [0.0, 0.0],
            burst_count: 0,
            active: true,
            world_space: true,
            texture_id: None,
            particle_source: None,
            z_order: 0,
            sort_layer: None,
            emit_shape: EmitShape::Point,
            ..Default::default()
        }
    }

    /// `color_choices` gives each particle one of its colours, keeping the
    /// start and end alphas; without choices the sequence is untouched.
    #[test]
    fn particles_pick_a_colour_each() {
        let choices = vec![[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]];
        let e = ParticleEmitter { color_choices: choices.clone(), ..make_test_emitter() };
        let mut rng = SimpleRng::new(7);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..60 {
            let p = ParticleSystem::make_particle(&mut rng, &e, Vec2::ZERO, None);
            assert!(choices.iter().any(|c| c[..3] == p.color_start[..3]), "{:?}", p.color_start);
            assert_eq!(p.color_start[..3], p.color_end[..3]);
            assert_eq!((p.color_start[3], p.color_end[3]), (1.0, 0.0));
            seen.insert((p.color_start[0] > 0.5, p.color_start[1] > 0.5));
        }
        assert_eq!(seen.len(), 3, "all three colours turn up");
        let plain = make_test_emitter();
        let (mut a, mut b) = (SimpleRng::new(9), SimpleRng::new(9));
        let first = ParticleSystem::make_particle(&mut a, &plain, Vec2::ZERO, None);
        let again = ParticleSystem::make_particle(&mut b, &plain, Vec2::ZERO, None);
        assert_eq!(first.seed, again.seed);
        assert_eq!(first.color_start, plain.color_start);
    }

    #[test]
    fn continuous_emission() {
        let mut sys = ParticleSystem::new();
        let emitter = make_test_emitter();
        let emitters = vec![(1u64, &emitter, Vec2::ZERO)];

        // 100 particles/sec * 0.1s = 10 particles
        sys.update(&emitters, 0.1);
        assert_eq!(sys.alive_count(), 10);
    }

    #[test]
    fn particles_die_after_lifetime() {
        let mut sys = ParticleSystem::new();
        let emitter = make_test_emitter();
        let emitters = vec![(1u64, &emitter, Vec2::ZERO)];

        sys.update(&emitters, 0.05); // spawn 5
        assert_eq!(sys.alive_count(), 5);

        // Advance past lifetime (1.0s)
        for _ in 0..20 {
            sys.update(&emitters, 0.1);
        }
        // All original 5 should be dead, but new ones spawned
        // Total alive should be capped at max_particles = 10
        assert!(sys.alive_count() <= 10);
    }

    #[test]
    fn burst_spawns_fixed_count() {
        let mut sys = ParticleSystem::new();
        let mut emitter = make_test_emitter();
        emitter.active = false; // no continuous emission

        sys.burst(1, &emitter, Vec2::ZERO, 5);
        // Need to call update to get alive_count computed
        let emitters = vec![(1u64, &emitter, Vec2::ZERO)];
        sys.update(&emitters, 0.0);
        assert_eq!(sys.alive_count(), 5);
    }

    #[test]
    fn pool_capped_at_max() {
        let mut sys = ParticleSystem::new();
        let emitter = make_test_emitter(); // max 10
        sys.burst(1, &emitter, Vec2::ZERO, 20); // try to spawn 20
        let emitters = vec![(1u64, &emitter, Vec2::ZERO)];
        sys.update(&emitters, 0.0);
        assert_eq!(sys.alive_count(), 10); // capped
    }

    #[test]
    fn particles_move_with_velocity() {
        let mut sys = ParticleSystem::new();
        let mut emitter = make_test_emitter(); // angle=0 → velocity is (50, 0)
        emitter.active = false; // no continuous emission

        sys.burst(1, &emitter, Vec2::ZERO, 1);
        let emitters = vec![(1u64, &emitter, Vec2::ZERO)];
        sys.update(&emitters, 0.5); // half second, still alive (lifetime=1.0)

        let draw = sys.draw_data(&[(1, &emitter, Vec2::ZERO)], &Default::default());
        assert_eq!(draw.len(), 1);
        assert_eq!(draw[0].particles.len(), 1);
        // velocity = (50, 0), dt = 0.5 → x ≈ 25
        assert!((draw[0].particles[0].position.x - 25.0).abs() < 1.0);
    }

    #[test]
    fn draw_data_interpolates_color() {
        let mut sys = ParticleSystem::new();
        let mut emitter = make_test_emitter();
        emitter.active = false; // no continuous emission
        sys.burst(1, &emitter, Vec2::ZERO, 1);

        // Advance halfway through lifetime
        let emitters = vec![(1u64, &emitter, Vec2::ZERO)];
        sys.update(&emitters, 0.5);

        let draw = sys.draw_data(&[(1, &emitter, Vec2::ZERO)], &Default::default());
        assert_eq!(draw.len(), 1);
        assert_eq!(draw[0].particles.len(), 1);
        let color = draw[0].particles[0].color;
        // Start: [1,0,0,1], End: [1,0,0,0], at t=0.5 → alpha ≈ 0.5
        assert!((color[3] - 0.5).abs() < 0.1);
    }

    #[test]
    fn inactive_emitter_no_spawn() {
        let mut sys = ParticleSystem::new();
        let mut emitter = make_test_emitter();
        emitter.active = false;
        let emitters = vec![(1u64, &emitter, Vec2::ZERO)];
        sys.update(&emitters, 1.0);
        assert_eq!(sys.alive_count(), 0);
    }

    #[test]
    fn a_burst_emitter_bursts_once_then_again_on_reactivation() {
        let mut sys = ParticleSystem::new();
        let mut emitter = make_test_emitter(); // max 10, lifetime 1 s
        emitter.burst_count = 6;
        emitter.max_particles = 64;

        sys.update(&[(1u64, &emitter, Vec2::ZERO)], 0.0);
        assert_eq!(sys.alive_count(), 6, "the burst did not fire");
        // No continuous emission in between.
        sys.update(&[(1u64, &emitter, Vec2::ZERO)], 0.1);
        assert_eq!(sys.alive_count(), 6);

        // Off, then on again: a second burst.
        emitter.active = false;
        sys.update(&[(1u64, &emitter, Vec2::ZERO)], 0.0);
        emitter.active = true;
        sys.update(&[(1u64, &emitter, Vec2::ZERO)], 0.0);
        assert_eq!(sys.alive_count(), 12);
    }

    #[test]
    fn nothing_is_alive_before_the_first_tick() {
        assert_eq!(ParticleSystem::new().alive_count(), 0);
    }

    #[test]
    fn draw_groups_keep_z_order_across_textures() {
        let mut sys = ParticleSystem::new();
        let mut a = make_test_emitter();
        a.active = false;
        a.texture_id = Some("one".into());
        a.z_order = 0;
        let mut b = a.clone();
        b.texture_id = Some("two".into());
        b.z_order = 1;
        let mut c = a.clone();
        c.z_order = 2;
        for (id, em) in [(1u64, &a), (2, &b), (3, &c)] {
            sys.burst(id, em, Vec2::ZERO, 1);
        }
        let emitters = [(1u64, &a, Vec2::ZERO), (2, &b, Vec2::ZERO), (3, &c, Vec2::ZERO)];
        let groups = sys.draw_data(&emitters, &Default::default());
        let order: Vec<Option<&str>> = groups.iter().map(|g| g.texture.as_deref()).collect();
        assert_eq!(order, vec![Some("one"), Some("two"), Some("one")], "C was drawn under B");
    }

    #[test]
    fn neighbouring_emitters_with_one_texture_still_batch() {
        let mut sys = ParticleSystem::new();
        let mut a = make_test_emitter();
        a.active = false;
        let b = a.clone();
        sys.burst(1, &a, Vec2::ZERO, 1);
        sys.burst(2, &b, Vec2::ZERO, 1);
        let groups = sys.draw_data(&[(1u64, &a, Vec2::ZERO), (2, &b, Vec2::ZERO)], &Default::default());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].particles.len(), 2);
    }

    #[test]
    fn despawned_entity_pool_removed() {
        let mut sys = ParticleSystem::new();
        let mut emitter = make_test_emitter();
        emitter.active = false; // no continuous emission
        sys.burst(1, &emitter, Vec2::ZERO, 5);

        // Update with entity 1 present
        let emitters = vec![(1u64, &emitter, Vec2::ZERO)];
        sys.update(&emitters, 0.01);
        assert_eq!(sys.alive_count(), 5);

        // Update without entity 1
        sys.update(&[], 0.01);
        assert_eq!(sys.alive_count(), 0);
    }

    #[test]
    fn local_space_particles_follow_emitter() {
        // World-space particle: spawn at emitter pos, emitter then moves,
        // particle stays where it was (modulo its own velocity).
        // Local-space particle: spawn at offset (0,0), emitter then moves,
        // particle's draw position should follow the emitter.
        let mut sys = ParticleSystem::new();
        let mut emitter = make_test_emitter();
        emitter.active = false;
        emitter.world_space = false;
        emitter.speed = [0.0, 0.0]; // no velocity → particle stays at local (0,0)
        emitter.gravity_modifier = 0.0;

        // Spawn one particle at emitter world pos (10, 10)
        sys.burst(1, &emitter, Vec2::new(10.0, 10.0), 1);

        // First frame: emitter still at (10, 10) → draw position should be (10, 10).
        let frame1 = sys.draw_data(&[(1u64, &emitter, Vec2::new(10.0, 10.0))], &Default::default());
        assert_eq!(frame1[0].particles[0].position, Vec2::new(10.0, 10.0));

        // Emitter moves to (50, 80) → draw position should track the emitter
        // because the particle is local-space with zero velocity.
        sys.update(&[(1u64, &emitter, Vec2::new(50.0, 80.0))], 0.0);
        let frame2 = sys.draw_data(&[(1u64, &emitter, Vec2::new(50.0, 80.0))], &Default::default());
        assert_eq!(frame2[0].particles[0].position, Vec2::new(50.0, 80.0));

        // Sanity: world-space particle does NOT follow the emitter.
        let mut sys_ws = ParticleSystem::new();
        let mut emitter_ws = emitter.clone();
        emitter_ws.world_space = true;
        sys_ws.burst(2, &emitter_ws, Vec2::new(10.0, 10.0), 1);
        sys_ws.update(&[(2u64, &emitter_ws, Vec2::new(50.0, 80.0))], 0.0);
        let frame_ws = sys_ws.draw_data(&[(2u64, &emitter_ws, Vec2::new(50.0, 80.0))], &Default::default());
        assert_eq!(frame_ws[0].particles[0].position, Vec2::new(10.0, 10.0));
    }
}

/// `Particles.set(e, field, value)`: write any field of an emitter by name -
/// `"angle"`, `"speed"`, `"gravity_modifier"`, `"turbulence.strength"`
/// (a dot reaches into a group, creating it with its defaults if the
/// emitter had none). The value goes through the component's own serde, so
/// a field takes exactly what a scene file would give it, and a whole
/// number written to a count is taken as one. `particle_source` is refused:
/// it is resolved when a scene loads, not at run time.
pub fn set_emitter_field(emitter: &mut ParticleEmitter, field: &str, value: &serde_json::Value) -> Result<(), String> {
    if field.is_empty() || field.starts_with("particle_source") {
        return Err(format!("'{field}' cannot be set at run time"));
    }
    // Serde ignores a key it does not know, so a misspelt field would
    // change nothing in silence: check the name against every field there is
    // (an emitter with each optional one filled, so none is skipped).
    let top = field.split('.').next().unwrap_or_default();
    let full = ParticleEmitter {
        particle_source: Some(String::new()),
        texture_id: Some(String::new()),
        sort_layer: Some(String::new()),
        blend_mode: BlendMode::Additive,
        color_curve: vec![[0.0; 5]],
        size_curve: vec![[0.0; 2]],
        stretch: 1.0,
        drag: 1.0,
        turbulence: Some(Turbulence::default()),
        collision: Some(ParticleCollision::default()),
        sub_emitters: vec![SubEmitter { on: SubEmitterEvent::Death, emitter: String::new(), count: 1, chance: 1.0 }],
        trail: Some(ParticleTrail::default()),
        flipbook: Some(ParticleFlipbook::default()),
        light: Some(ParticleLights::default()),
        color_choices: vec![[1.0; 4]],
        ..Default::default()
    };
    if serde_json::to_value(&full).ok().and_then(|v| v.get(top).cloned()).is_none() {
        return Err(format!("'{top}' is not a particle emitter field"));
    }
    let mut whole = serde_json::to_value(&*emitter).map_err(|e| e.to_string())?;
    let parts: Vec<&str> = field.split('.').collect();
    set_json_path(&mut whole, &parts, value).map_err(|at| format!("'{field}': '{at}' is not a group"))?;
    let updated: ParticleEmitter = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    *emitter = updated;
    Ok(())
}

/// Write `value` at `path` in `v`, creating a group missing on the way (an
/// `Option` field left out of the JSON). A whole number written over a
/// count stays whole: scripts only have floats. `Err` names the part that
/// was not a group.
fn set_json_path(v: &mut serde_json::Value, path: &[&str], value: &serde_json::Value) -> Result<(), String> {
    use serde_json::Value as J;
    let J::Object(map) = v else {
        return Err(String::new());
    };
    let (head, rest) = (path[0], &path[1..]);
    if rest.is_empty() {
        let new = match (map.get(head), value) {
            (Some(J::Number(o)), J::Number(n)) if !o.is_f64() => match n.as_f64() {
                Some(f) if f.fract() == 0.0 && f >= 0.0 => J::from(f as u64),
                _ => value.clone(),
            },
            _ => value.clone(),
        };
        map.insert(head.to_string(), new);
        return Ok(());
    }
    let next = map.entry(head.to_string()).or_insert(J::Null);
    if next.is_null() {
        *next = J::Object(Default::default());
    }
    set_json_path(next, rest, value).map_err(|e| if e.is_empty() { head.to_string() } else { e })
}

/// What each GPU cloud meets this frame ([`gpu::GpuWorld`]): its
/// attractors where they are (a `follow` at its entity's world position),
/// and, for a cloud that collides, the colliders nearest it as boxes and
/// circles. Inside [`tick_particles`], so no host was wired.
fn gpu_world(world: &hecs::World) {
    use crate::ecs::components::{Collider2D, ColliderShape, Name, Transform, WorldTransform};
    let mut wants_names = false;
    let mut wants_colliders = false;
    for (_, g) in world.query::<&gpu::GpuParticles>().iter() {
        wants_names |= g.attractors.iter().any(|a| !a.follow.is_empty());
        wants_colliders |= g.collide;
    }
    let at = |t: &Transform, w: Option<&WorldTransform>| w.map_or((t.position, t.rotation), |w| (w.position, w.rotation));
    let mut named: std::collections::HashMap<String, glam::Vec2> = std::collections::HashMap::new();
    if wants_names {
        for (_, (n, t, w)) in world.query::<(&Name, &Transform, Option<&WorldTransform>)>().iter() {
            named.entry(n.0.clone()).or_insert(at(t, w).0);
        }
    }
    let mut colliders: Vec<gpu::GpuCollider> = Vec::new();
    if wants_colliders {
        for (_, (c, t, w, v)) in world
            .query::<(&Collider2D, &Transform, Option<&WorldTransform>, Option<&crate::ecs::components::Velocity2D>)>()
            .iter()
        {
            if c.is_sensor {
                continue;
            }
            // A moving collider pushes stateful particles (`gpu_state`).
            let velocity = v.map_or(glam::Vec2::ZERO, |v| glam::Vec2::new(v.x, v.y));
            let (pos, rot) = at(t, w);
            let s = w.map_or(t.scale, |w| w.scale).abs();
            let (sn, cs) = rot.sin_cos();
            let off = glam::Vec2::new(c.offset[0] * s.x, c.offset[1] * s.y);
            let centre = pos + glam::Vec2::new(off.x * cs - off.y * sn, off.x * sn + off.y * cs);
            let points: Vec<[f32; 2]> = match &c.shape {
                ColliderShape::Box { width, height } => {
                    colliders.push(gpu::GpuCollider {
                        centre,
                        half: glam::Vec2::new(width * 0.5 * s.x, height * 0.5 * s.y),
                        rotation: rot,
                        circle: false,
                        velocity,
                    });
                    continue;
                }
                ColliderShape::Circle { radius } => {
                    colliders.push(gpu::GpuCollider {
                        centre,
                        half: glam::Vec2::splat(radius * s.x.max(s.y)),
                        rotation: 0.0,
                        circle: true,
                        velocity,
                    });
                    continue;
                }
                ColliderShape::Capsule { half_height, radius } => {
                    vec![[-radius, -half_height - radius], [*radius, half_height + radius]]
                }
                ColliderShape::Segment { a, b } => vec![*a, *b],
                ColliderShape::Triangle { a, b, c } => vec![*a, *b, *c],
                ColliderShape::ConvexPolygon { vertices }
                | ColliderShape::Polyline { vertices }
                | ColliderShape::ConvexDecomposition { vertices, .. } => vertices.clone(),
                ColliderShape::HeightField { .. } => continue,
            };
            // Anything else: its bounding box, turned with it.
            if points.is_empty() {
                continue;
            }
            let (mut lo, mut hi) = (glam::Vec2::splat(f32::MAX), glam::Vec2::splat(f32::MIN));
            for p in &points {
                let v = glam::Vec2::new(p[0] * s.x, p[1] * s.y);
                lo = lo.min(v);
                hi = hi.max(v);
            }
            let mid = (lo + hi) * 0.5;
            colliders.push(gpu::GpuCollider {
                centre: centre + glam::Vec2::new(mid.x * cs - mid.y * sn, mid.x * sn + mid.y * cs),
                half: (hi - lo) * 0.5,
                rotation: rot,
                circle: false,
                velocity,
            });
        }
    }
    for (_, (g, t, w)) in world.query::<(&mut gpu::GpuParticles, &Transform, Option<&WorldTransform>)>().iter() {
        let origin = at(t, w).0;
        g.world.attractors = g
            .attractors
            .iter()
            .take(gpu::MAX_ATTRACTORS)
            .map(|a| {
                let p = if a.follow.is_empty() {
                    glam::Vec2::new(a.x, a.y)
                } else {
                    named.get(&a.follow).copied().unwrap_or(glam::Vec2::new(a.x, a.y))
                };
                ([p.x, p.y, a.strength, a.swirl], a.radius.max(0.0))
            })
            .collect();
        g.world.colliders.clear();
        if g.collide {
            let mut near: Vec<(f32, gpu::GpuCollider)> =
                colliders.iter().map(|c| ((c.centre - origin).length() - c.half.length(), *c)).collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            g.world.colliders = near.into_iter().take(gpu::MAX_COLLIDERS).map(|(_, c)| c).collect();
        }
    }
}

/// The one particle tick, called by every host once a frame: each emitter at
/// its **world** position (a child of a moving thing emits from where it is
/// drawn - the hosts used its local position until Sep 2026), with its
/// entity's name for [`SubEmitter`]s to find it, and the scene's colliders
/// gathered as [`ColliderOutlines`] when any emitter collides.
pub fn tick_particles(world: &hecs::World, system: &mut ParticleSystem, dt: f32) {
    let _span = crate::profiling::trace::span("particles");
    let by = crate::ecs::floating_origin::follow(&mut system.origin_seen);
    if by != Vec2::ZERO {
        system.shift(by);
    }
    // GPU particles keep only a clock (`gpu`): advanced here, with the
    // game's `dt`, so a paused game pauses them.
    for (_, g) in world.query::<&mut gpu::GpuParticles>().iter() {
        if g.playing {
            g.time += dt;
        }
    }
    gpu_world(world);
    use crate::ecs::components::{Name, Transform, WorldTransform};
    let mut q = world.query::<(&Transform, &ParticleEmitter, Option<&WorldTransform>, Option<&Name>)>();
    let mut emitters = Vec::new();
    let mut names = Vec::new();
    system.emit_geometry.clear();
    system.clocks.clear();
    for (e, (t, em, wt, name)) in q.iter() {
        let at = wt.map_or(t.position, |w| w.position);
        if let Ok(own) = world.get::<&crate::ecs::time_scale::TimeScale>(e) {
            system.clocks.insert(e.id() as u64, crate::ecs::time_scale::factor(Some(&*own)));
        }
        emitters.push((e.id() as u64, em, at));
        names.push(name.map(|n| n.0.as_str()));
        // A `path` or `sprite` shape: where this frame's particles start.
        let xf = wt.cloned().unwrap_or_else(|| WorldTransform::from(t));
        let geometry = match em.emit_shape {
            EmitShape::Path => world.get::<&crate::ecs::components::VectorPath>(e).ok().map(|path| {
                let (local, closed) = crate::ecs::path_follow::flatten(&path);
                let mut points: Vec<Vec2> = local.iter().map(|p| xf.apply(*p)).collect();
                // A closed path's last side, back to its start.
                if closed && points.len() > 2 {
                    points.push(points[0]);
                }
                EmitGeometry::polyline(&points, at)
            }),
            EmitShape::Sprite { .. } => em.emit_pixels.as_ref().map(|px| EmitGeometry::Pixels {
                points: std::sync::Arc::new(px.points.clone()),
                xf: xf.clone(),
                jitter: px.texel,
            }),
            _ => None,
        };
        if let Some(g) = geometry {
            system.emit_geometry.insert(e.id() as u64, g);
        }
    }
    // Wind from the area effects: only boxes with a force, and none read at
    // all when there are none.
    system.winds = crate::ecs::area_effect::placed_areas(world);
    system.winds.retain(|a| a.effect.force != [0.0, 0.0]);
    let surfaces = if emitters.iter().any(|(_, em, _)| em.collision.is_some()) {
        ColliderOutlines::gather(world)
    } else {
        ColliderOutlines::default()
    };
    system.step(&emitters, &names, &surfaces, dt);
    drop(emitters);
    drop(names);
    drop(q);
    // Particle lights (`light`): left on the emitter for `gather_lights`.
    for (e, (t, em, wt)) in world.query::<(&Transform, &mut ParticleEmitter, Option<&WorldTransform>)>().iter() {
        if em.light.is_some() {
            let at = wt.map_or(t.position, |w| w.position);
            em.lit = system.lights_for(e.id() as u64, em, at);
        } else if !em.lit.is_empty() {
            em.lit.clear();
        }
    }
}

/// A particle's colour and size now, as it is drawn - and lit.
fn appearance(emitter: &ParticleEmitter, p: &Particle) -> ([f32; 4], f32) {
    let t = (p.age / p.lifetime).clamp(0.0, 1.0);
    let color = if emitter.color_curve.is_empty() {
        lerp_color(p.color_start, p.color_end, t)
    } else {
        let k = sample_curve(&emitter.color_curve, t);
        [k[1], k[2], k[3], k[4]]
    };
    let size = if emitter.size_curve.is_empty() {
        p.size_start + (p.size_end - p.size_start) * t
    } else {
        p.size_start * sample_curve(&emitter.size_curve, t)[1]
    };
    (color, size)
}

impl ParticleSystem {
    /// The lights an emitter's particles give off this frame
    /// ([`ParticleLights`]), oldest particle first, at most `max`.
    pub fn lights_for(&self, entity_id: u64, emitter: &ParticleEmitter, emitter_pos: Vec2) -> Vec<ParticleLight> {
        let (Some(cfg), Some(pool)) = (emitter.light.as_ref(), self.pools.get(&entity_id)) else {
            return Vec::new();
        };
        let offset = if pool.world_space { Vec2::ZERO } else { emitter_pos };
        let every = cfg.every.max(1);
        let mut out: Vec<(f32, ParticleLight)> = Vec::new();
        for p in &pool.particles[..pool.alive_count] {
            // The seed is drawn once at birth, so the choice holds all life.
            if every > 1 && ((p.seed * 65_536.0) as u32) % every != 0 {
                continue;
            }
            let (color, size) = appearance(emitter, p);
            let rgb = if cfg.use_particle_color { [color[0], color[1], color[2]] } else { cfg.color };
            let alpha = if cfg.fade { color[3].clamp(0.0, 1.0) } else { 1.0 };
            let grow = if cfg.scale_with_size { size / p.size_start.max(1e-3) } else { 1.0 };
            let intensity = cfg.intensity * alpha;
            let radius = cfg.radius * grow.max(0.0);
            if intensity <= 0.0 || radius <= 0.5 {
                continue;
            }
            out.push((p.age, ParticleLight { position: p.position + offset, color: rgb, intensity, radius, falloff: cfg.falloff }));
        }
        if out.len() > cfg.max as usize {
            out.sort_by(|a, b| b.0.total_cmp(&a.0));
            out.truncate(cfg.max as usize);
        }
        out.into_iter().map(|(_, l)| l).collect()
    }
}

/// Where an emitter entity's particles are drawn from: its world position,
/// as [`tick_particles`] simulates it.
pub fn emitter_position(world: &hecs::World, entity: hecs::Entity) -> Option<Vec2> {
    use crate::ecs::components::{Transform, WorldTransform};
    if let Ok(w) = world.get::<&WorldTransform>(entity) {
        return Some(w.position);
    }
    world.get::<&Transform>(entity).ok().map(|t| t.position)
}

#[cfg(test)]
mod trail_tests {
    use super::*;

    /// A trail records a point every `interval`, keeps the last `length`,
    /// and ends where the particle is.
    #[test]
    fn a_particle_trail_follows_where_it_has_been() {
        let mut sys = ParticleSystem::new();
        let em = ParticleEmitter {
            emission_rate: 0.0,
            burst_count: 1,
            max_particles: 1,
            lifetime: [10.0, 10.0],
            speed: [100.0, 100.0],
            angle: [0.0, 0.0],
            size_start: [10.0, 10.0],
            size_end: [10.0, 10.0],
            trail: Some(ParticleTrail { length: 4, interval: 0.1, ..Default::default() }),
            ..Default::default()
        };
        let emitters = [(1u64, &em, Vec2::ZERO)];
        for _ in 0..20 {
            sys.update(&emitters, 0.05);
        }
        let groups = sys.draw_data_masked(&emitters, &Default::default(), &Default::default());
        let trail = &groups[0].trails[0];
        assert_eq!(trail.points.len(), 4, "three recorded and the head: {:?}", trail.points);
        let head = *trail.points.last().unwrap();
        assert!((head.x - 100.0).abs() < 1e-3 && head.y.abs() < 1e-3, "it ends at the particle: {head:?}");
        for w in trail.points.windows(2) {
            assert!(w[1].x > w[0].x, "oldest first: {:?}", trail.points);
        }
        assert!(trail.points[0].x >= 65.0, "only the last few are kept: {:?}", trail.points);
        assert_eq!(trail.width, 8.0, "width is a share of the size");
    }

    #[test]
    fn a_script_sets_a_trail_whole_by_field_or_not_at_all() {
        let mut em = ParticleEmitter::default();
        set_emitter_field(&mut em, "trail", &serde_json::json!({ "length": 12 })).unwrap();
        assert_eq!(em.trail.map(|t| t.length), Some(12), "missing fields take their defaults");
        set_emitter_field(&mut em, "trail.width", &serde_json::json!(2.0)).unwrap();
        assert_eq!(em.trail.map(|t| t.width), Some(2.0));
        set_emitter_field(&mut em, "trail", &serde_json::Value::Null).unwrap();
        assert!(em.trail.is_none(), "nil removes it");
    }

    #[test]
    fn a_trail_ribbon_tapers_and_fades_to_its_tail() {
        let t = TrailDrawData {
            points: vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)],
            width: 4.0,
            color: [1.0, 0.5, 0.25, 1.0],
            taper: true,
            fade: true,
        };
        let (pos, uv, col, idx) = t.ribbon();
        assert_eq!((pos.len(), uv.len(), col.len(), idx.len()), (6, 6, 6, 12));
        assert_eq!((pos[0], pos[1]), (Vec2::ZERO, Vec2::ZERO), "the tail is a point");
        assert_eq!((pos[4], pos[5]), (Vec2::new(20.0, 2.0), Vec2::new(20.0, -2.0)), "the head is the width");
        assert_eq!((col[0][3], col[4][3]), (0.0, 1.0));
        assert_eq!(&idx[..6], &[0, 1, 3, 0, 3, 2], "a quad a b c, a c d");
        let wide = TrailDrawData { taper: false, fade: false, ..t };
        let (pos, _, col, _) = wide.ribbon();
        assert_eq!((pos[0], col[0][3]), (Vec2::new(0.0, 2.0), 1.0));
    }
}
