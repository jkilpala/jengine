//! **Cloth and rope**: meshes shaped every frame by a simulation (Sep 2026).
//!
//! A `cloth` is a grid of points and a `rope` a chain of them, both moved by
//! position-based dynamics - gravity, a wind that gusts, a curl-noise
//! flutter - held together by distance constraints and pinned to entities:
//! a banner's top edge to its pole, a rope's ends to whatever it hangs
//! between. Each writes the result into its entity's [`Mesh2D`] - the
//! vertices in the entity's own space, the texture laid over the grid or
//! stretched along the rope - so it draws in the sprite pass, sorted, lit and
//! blended like anything else, and nothing about drawing it is new.
//!
//! Before this a banner was a sprite that could at most sway as a whole, and
//! anything bent had to be a script calling `Mesh.set` every frame, with the
//! simulation written in Lua. Both are **visual**: nothing here pushes a
//! body. A rope that must hold weight is a rapier joint (`Rope`, or a chain
//! of revolute joints for a bridge), and a `rope` component pinned along it
//! is how it is drawn.
//!
//! One step, [`tick_cloth`], called by every host right after the hierarchy
//! resolve (beside `tick_trails`), since the pins are read from world
//! transforms. Points collide with non-sensor colliders when asked
//! (`collide`), through the same CPU outlines particles use.

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::{Mesh2D, MeshVertex, Name, Transform, WorldTransform};
use crate::ecs::outlines::ColliderOutlines;
use crate::particles::Turbulence;

/// Which of a cloth's points are held to its entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ClothPin {
    /// The whole top row: a banner hanging from a beam.
    #[default]
    Top,
    /// The whole left column: a flag on a pole, flying right.
    Left,
    /// The whole right column: a flag flying left.
    Right,
    /// The two top corners only: a sheet on a line, sagging between them.
    TopCorners,
    /// Nothing: it falls (and drapes, with `collide`).
    None,
}

/// A rectangle of cloth: `cols` x `rows` points spanning `size`, pinned by
/// `pin` and blown about. Draws through the entity's `mesh` (added if
/// absent), with the mesh's texture laid over the grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Cloth {
    /// Its size at rest, in the entity's units.
    pub size: [f32; 2],
    /// Points across and down, at least 2 each. More is smoother and costs
    /// more: 16 x 10 is a flag.
    pub cols: u32,
    pub rows: u32,
    /// Which point of the rectangle sits on the entity's origin, as a
    /// sprite's anchor: `[0, 1]` (the default) is the top left - a flag's
    /// hoist on its pole - and `[0.5, 1]` the middle of the top edge.
    pub anchor: [f32; 2],
    pub pin: ClothPin,
    /// Acceleration, in world units per second squared. Projects run
    /// physics at about 981; cloth looks better lighter.
    pub gravity: [f32; 2],
    /// The wind, as an acceleration.
    pub wind: [f32; 2],
    /// How much the wind's strength wanders, 0..1: at 0.5 it blows between
    /// half and one and a half times `wind`, slowly.
    pub gust: f32,
    /// Curl-noise flutter, as an acceleration: small, uneven movement.
    pub flutter: f32,
    /// The size of one flutter eddy, in world units.
    pub flutter_scale: f32,
    /// A travelling wave, as an acceleration across the cloth: what makes a
    /// flag *fly* rather than stand out as a board. A 2D sheet the wind has
    /// pulled taut can only ripple sideways, and noise alone barely moves it,
    /// so the wave pushes across the cloth, growing from the pinned edge to
    /// the free one and running away from the pin. Scaled by the gusts.
    pub wave: f32,
    /// One wave's length along the cloth, in world units.
    pub wave_length: f32,
    /// Waves a second.
    pub wave_speed: f32,
    /// Velocity lost per second, `e^(-damping t)`.
    pub damping: f32,
    /// How firmly each constraint is met per iteration, 0..1.
    pub stiffness: f32,
    /// Constraint iterations per substep.
    pub iterations: u32,
    /// How much it resists folding, 0..1: 0 crumples like silk, 1 holds
    /// its shape like canvas.
    pub bend: f32,
    /// How much a fold - cloth bunched up across the grid - darkens it,
    /// 0..1, so a flat mesh reads as having depth.
    pub shade: f32,
    /// Points stop at non-sensor colliders.
    pub collide: bool,

    // ---- Written by the simulation ----
    /// The points, in world space (empty until the first step).
    #[serde(skip)]
    pub points: Vec<Vec2>,
    #[serde(skip)]
    pub velocity: Vec<Vec2>,
    /// Seconds simulated, for the wind's gusts and the flutter.
    #[serde(skip)]
    pub time: f32,
}

impl Default for Cloth {
    fn default() -> Self {
        Self {
            size: [160.0, 100.0],
            cols: 16,
            rows: 10,
            anchor: [0.0, 1.0],
            pin: ClothPin::Top,
            gravity: [0.0, -500.0],
            wind: [0.0, 0.0],
            gust: 0.4,
            flutter: 300.0,
            flutter_scale: 70.0,
            wave: 0.0,
            wave_length: 140.0,
            wave_speed: 1.4,
            damping: 1.0,
            stiffness: 1.0,
            iterations: 6,
            bend: 0.5,
            shade: 0.45,
            collide: false,
            points: Vec::new(),
            velocity: Vec::new(),
            time: 0.0,
        }
    }
}

/// Where a rope is held part way along: `at` 0..1 of its length, to a
/// named entity (at `offset` in that entity's space).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RopePin {
    pub at: f32,
    pub entity: String,
    pub offset: [f32; 2],
}

/// A rope: a chain of points from its entity (the start, at `start_offset`)
/// to `end` (an entity name, at `end_offset`), or free at the end when
/// `end` is empty. Draws through the entity's `mesh` as a strip `width`
/// wide, the texture stretched from start to end.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Rope {
    /// Its length at rest. Longer than the distance between its ends sags.
    pub length: f32,
    pub segments: u32,
    pub width: f32,
    pub start_offset: [f32; 2],
    pub end: String,
    pub end_offset: [f32; 2],
    /// More places it is held, by fraction of its length.
    pub pins: Vec<RopePin>,
    pub gravity: [f32; 2],
    pub wind: [f32; 2],
    pub gust: f32,
    pub flutter: f32,
    pub flutter_scale: f32,
    pub damping: f32,
    pub stiffness: f32,
    pub iterations: u32,
    pub collide: bool,

    // ---- Written by the simulation ----
    /// The points, in world space (empty until the first step).
    #[serde(skip)]
    pub points: Vec<Vec2>,
    #[serde(skip)]
    pub velocity: Vec<Vec2>,
    /// Seconds simulated, for the wind's gusts and the flutter.
    #[serde(skip)]
    pub time: f32,
}

impl Default for Rope {
    fn default() -> Self {
        Self {
            length: 200.0,
            segments: 16,
            width: 4.0,
            start_offset: [0.0, 0.0],
            end: String::new(),
            end_offset: [0.0, 0.0],
            pins: Vec::new(),
            gravity: [0.0, -700.0],
            wind: [0.0, 0.0],
            gust: 0.4,
            flutter: 0.0,
            flutter_scale: 80.0,
            damping: 1.5,
            stiffness: 1.0,
            iterations: 12,
            collide: false,
            points: Vec::new(),
            velocity: Vec::new(),
            time: 0.0,
        }
    }
}

/// The step every point takes: at most this long, so a slow frame is several.
const SUBSTEP: f32 = 1.0 / 120.0;
/// A frame longer than this is simulated as this long (a hitch, a load).
const MAX_FRAME: f32 = 0.1;

/// The inverse of [`WorldTransform::apply`]: a world point into the
/// entity's own space.
fn to_local(w: &WorldTransform, p: Vec2) -> Vec2 {
    let d = p - w.position;
    let (sin, cos) = w.rotation.sin_cos();
    let k = Vec2::new(d.x * cos + d.y * sin, -d.x * sin + d.y * cos);
    let s = Vec2::new(k.x - w.shear * k.y, k.y);
    let sc = Vec2::new(
        if w.scale.x.abs() > 1e-6 { w.scale.x } else { 1e-6 },
        if w.scale.y.abs() > 1e-6 { w.scale.y } else { 1e-6 },
    );
    s / sc
}

/// The wind's strength multiplier at time `t`: a slow wander between
/// `1 - gust` and `1 + gust`, from two incommensurate sines.
fn gust_factor(gust: f32, t: f32, seed: f32) -> f32 {
    let g = gust.clamp(0.0, 1.0);
    let w = 0.6 * (t * 0.73 + seed).sin() + 0.4 * (t * 1.91 + seed * 2.3).sin();
    1.0 + g * w
}

/// Shared by cloth and rope: one position-based step of `points`. `pinned`
/// says where a point is held (its target at the end of the frame, and at
/// its start for interpolation); `constraints` are `(a, b, rest length)`.
#[allow(clippy::too_many_arguments)]
fn simulate(
    points: &mut [Vec2],
    velocity: &mut [Vec2],
    pinned: &[Option<(Vec2, Vec2)>],
    constraints: &[(usize, usize, f32, f32)],
    accel: impl Fn(usize, Vec2, f32) -> Vec2,
    damping: f32,
    stiffness: f32,
    iterations: u32,
    outlines: Option<&ColliderOutlines>,
    time: f32,
    dt: f32,
) {
    let dt = dt.clamp(0.0, MAX_FRAME);
    if dt <= 0.0 {
        return;
    }
    let steps = (dt / SUBSTEP).ceil().max(1.0) as u32;
    let h = dt / steps as f32;
    let keep = (-damping.max(0.0) * h).exp();
    let k = stiffness.clamp(0.0, 1.0);
    let mut prev: Vec<Vec2> = points.to_vec();
    for s in 0..steps {
        let f = (s + 1) as f32 / steps as f32;
        let t = time + h * (s + 1) as f32;
        for i in 0..points.len() {
            prev[i] = points[i];
            if let Some((from, to)) = pinned[i] {
                points[i] = from.lerp(to, f);
                continue;
            }
            velocity[i] = (velocity[i] + accel(i, points[i], t) * h) * keep;
            points[i] += velocity[i] * h;
        }
        for _ in 0..iterations.max(1) {
            for &(a, b, rest, weight) in constraints {
                let d = points[b] - points[a];
                let len = d.length();
                if len < 1e-6 {
                    continue;
                }
                let (wa, wb) = (pinned[a].is_none() as u8 as f32, pinned[b].is_none() as u8 as f32);
                let w = wa + wb;
                if w == 0.0 {
                    continue;
                }
                let corr = d * ((len - rest) / len) * k * weight / w;
                points[a] += corr * wa;
                points[b] -= corr * wb;
            }
        }
        if let Some(o) = outlines {
            for i in 0..points.len() {
                if pinned[i].is_some() {
                    continue;
                }
                if let Some(hit) = o.first_hit(prev[i], points[i], u16::MAX) {
                    points[i] = hit.point + hit.normal * 0.5;
                }
            }
        }
        for i in 0..points.len() {
            if pinned[i].is_none() {
                velocity[i] = (points[i] - prev[i]) / h;
            }
        }
    }
}

impl Cloth {
    fn dims(&self) -> (usize, usize) {
        (self.cols.max(2) as usize, self.rows.max(2) as usize)
    }

    /// Point `(c, r)`'s place at rest, in the entity's space (row 0 on top).
    fn rest(&self, c: usize, r: usize) -> Vec2 {
        let (cols, rows) = self.dims();
        let u = c as f32 / (cols - 1) as f32;
        let v = 1.0 - r as f32 / (rows - 1) as f32;
        Vec2::new((u - self.anchor[0]) * self.size[0], (v - self.anchor[1]) * self.size[1])
    }

    fn is_pinned(&self, c: usize, r: usize) -> bool {
        let (cols, _) = self.dims();
        match self.pin {
            ClothPin::Top => r == 0,
            ClothPin::Left => c == 0,
            ClothPin::Right => c == cols - 1,
            ClothPin::TopCorners => r == 0 && (c == 0 || c == cols - 1),
            ClothPin::None => false,
        }
    }

    /// The points, in world space, after the last step.
    pub fn points(&self) -> &[Vec2] {
        &self.points
    }

    /// One frame: move the points, with the pins at `from` -> `to`.
    pub fn step(&mut self, from: &WorldTransform, to: &WorldTransform, outlines: Option<&ColliderOutlines>, dt: f32) {
        let (cols, rows) = self.dims();
        let n = cols * rows;
        if self.points.len() != n {
            self.points = (0..n).map(|i| to.apply(self.rest(i % cols, i / cols))).collect();
            self.velocity = vec![Vec2::ZERO; n];
        }
        let pinned: Vec<Option<(Vec2, Vec2)>> = (0..n)
            .map(|i| {
                let (c, r) = (i % cols, i / cols);
                self.is_pinned(c, r).then(|| (from.apply(self.rest(c, r)), to.apply(self.rest(c, r))))
            })
            .collect();
        let sx = self.size[0].abs() / (cols - 1) as f32;
        let sy = self.size[1].abs() / (rows - 1) as f32;
        let scale = to.scale.abs();
        let (dx, dy) = (sx * scale.x, sy * scale.y);
        let diag = (dx * dx + dy * dy).sqrt();
        let mut cons = Vec::with_capacity(n * 6);
        let bend = self.bend.clamp(0.0, 1.0);
        for r in 0..rows {
            for c in 0..cols {
                let i = r * cols + c;
                if c + 1 < cols {
                    cons.push((i, i + 1, dx, 1.0));
                }
                if r + 1 < rows {
                    cons.push((i, i + cols, dy, 1.0));
                }
                if c + 1 < cols && r + 1 < rows {
                    cons.push((i, i + cols + 1, diag, 1.0));
                    cons.push((i + 1, i + cols, diag, 1.0));
                }
                // Two apart: resists folding. A 2D sheet has nowhere to go
                // out of its plane, so without these a slack flag crumples
                // onto itself in zigzags.
                if bend > 0.0 && c + 2 < cols {
                    cons.push((i, i + 2, dx * 2.0, bend));
                }
                if bend > 0.0 && r + 2 < rows {
                    cons.push((i, i + 2 * cols, dy * 2.0, bend));
                }
            }
        }
        let (g, wind) = (Vec2::from(self.gravity), Vec2::from(self.wind));
        let (gust, flutter) = (self.gust, self.flutter);
        let turb = Turbulence { strength: flutter, scale: self.flutter_scale, speed: 1.2 };
        // A ripple travels with the wind, so the noise is read upwind.
        let drift = wind.normalize_or_zero() * self.flutter_scale * 2.0;
        // The wave: how far each point is from the pinned edge at rest (0..1
        // of the cloth), and the direction across it, turned with the entity.
        let (phase, grow, along) = {
            let (w, h) = (self.size[0].abs(), self.size[1].abs());
            // (distance from the pin at rest, the same as 0..1 of the cloth)
            let dist = |i: usize| {
                let u = (i % cols) as f32 / (cols - 1) as f32;
                let v = (i / cols) as f32 / (rows - 1) as f32;
                match self.pin {
                    ClothPin::Left | ClothPin::None => (u * w, u),
                    ClothPin::Right => ((1.0 - u) * w, 1.0 - u),
                    ClothPin::Top | ClothPin::TopCorners => (v * h, v),
                }
            };
            let free = match self.pin {
                ClothPin::Left | ClothPin::None => Vec2::X,
                ClothPin::Right => -Vec2::X,
                ClothPin::Top | ClothPin::TopCorners => -Vec2::Y,
            };
            let length = self.wave_length.max(1.0);
            let phase: Vec<f32> = (0..n).map(|i| dist(i).0 / length).collect();
            let grow: Vec<f32> = (0..n).map(|i| dist(i).1).collect();
            (phase, grow, to.apply_linear(Vec2::new(-free.y, free.x)).normalize_or_zero())
        };
        let (wave, wave_speed) = (self.wave, self.wave_speed);
        let accel = |i: usize, p: Vec2, t: f32| {
            let g_now = gust_factor(gust, t, 0.0);
            let mut a = g + wind * g_now;
            if flutter > 0.0 {
                a += turb.flow(p - drift * t, t);
            }
            if wave != 0.0 {
                let s = (std::f32::consts::TAU * (phase[i] - wave_speed * t)).sin();
                a += along * (wave * g_now * grow[i] * s);
            }
            a
        };
        simulate(
            &mut self.points,
            &mut self.velocity,
            &pinned,
            &cons,
            accel,
            self.damping,
            self.stiffness,
            self.iterations,
            if self.collide { outlines } else { None },
            self.time,
            dt,
        );
        self.time += dt.clamp(0.0, MAX_FRAME);
    }

    /// Write the grid into `mesh`, in `at`'s space: the texture over the
    /// whole grid, each quad two triangles, and folds darkened by `shade`.
    pub fn write_mesh(&self, mesh: &mut Mesh2D, at: &WorldTransform) {
        let (cols, rows) = self.dims();
        if self.points.len() != cols * rows {
            return;
        }
        let dx = (self.size[0].abs() / (cols - 1) as f32 * at.scale.x.abs()).max(1e-3);
        let p = |c: usize, r: usize| self.points[r * cols + c];
        mesh.vertices.clear();
        for r in 0..rows {
            for c in 0..cols {
                // How bunched the cloth is across here: 1 flat, toward 0 in a fold.
                let (l, rr) = (c.saturating_sub(1), (c + 1).min(cols - 1));
                let span = p(rr, r).distance(p(l, r)) / (dx * (rr - l) as f32);
                let fold = (1.0 - span.clamp(0.0, 1.0)).sqrt();
                let b = 1.0 - self.shade.clamp(0.0, 1.0) * fold;
                mesh.vertices.push(MeshVertex {
                    position: to_local(at, p(c, r)).into(),
                    uv: [c as f32 / (cols - 1) as f32, r as f32 / (rows - 1) as f32],
                    color: [b, b, b, 1.0],
                    q: 1.0,
                });
            }
        }
        mesh.indices.clear();
        for r in 0..rows - 1 {
            for c in 0..cols - 1 {
                let a = (r * cols + c) as u32;
                let (b, cc, d) = (a + 1, a + 1 + cols as u32, a + cols as u32);
                mesh.indices.extend_from_slice(&[a, b, cc, a, cc, d]);
            }
        }
    }
}

impl Rope {
    fn count(&self) -> usize {
        self.segments.max(1) as usize + 1
    }

    /// The points, in world space, after the last step.
    pub fn points(&self) -> &[Vec2] {
        &self.points
    }

    /// One frame. `start` is where the rope begins (its entity), `end` where
    /// it is held at the far end if anywhere, and `pins` its other holds as
    /// (point index, from, to).
    pub fn step(
        &mut self,
        start: (Vec2, Vec2),
        end: Option<(Vec2, Vec2)>,
        pins: &[(usize, Vec2, Vec2)],
        outlines: Option<&ColliderOutlines>,
        dt: f32,
    ) {
        let n = self.count();
        if self.points.len() != n {
            let to = end.map_or(start.1 + Vec2::new(0.0, -self.length), |e| e.1);
            self.points = (0..n).map(|i| start.1.lerp(to, i as f32 / (n - 1) as f32)).collect();
            self.velocity = vec![Vec2::ZERO; n];
        }
        let mut pinned: Vec<Option<(Vec2, Vec2)>> = vec![None; n];
        pinned[0] = Some(start);
        if let Some(e) = end {
            pinned[n - 1] = Some(e);
        }
        for &(i, from, to) in pins {
            if i < n {
                pinned[i] = Some((from, to));
            }
        }
        let seg = self.length.max(0.0) / (n - 1) as f32;
        let cons: Vec<(usize, usize, f32, f32)> = (0..n - 1).map(|i| (i, i + 1, seg, 1.0)).collect();
        let (g, wind) = (Vec2::from(self.gravity), Vec2::from(self.wind));
        let (gust, flutter) = (self.gust, self.flutter);
        let turb = Turbulence { strength: flutter, scale: self.flutter_scale, speed: 0.8 };
        let accel = |_: usize, p: Vec2, t: f32| {
            let mut a = g + wind * gust_factor(gust, t, 1.7);
            if flutter > 0.0 {
                a += turb.flow(p, t);
            }
            a
        };
        simulate(
            &mut self.points,
            &mut self.velocity,
            &pinned,
            &cons,
            accel,
            self.damping,
            self.stiffness,
            self.iterations,
            if self.collide { outlines } else { None },
            self.time,
            dt,
        );
        self.time += dt.clamp(0.0, MAX_FRAME);
    }

    /// Write the rope into `mesh` as a strip `width` wide, in `at`'s space,
    /// the texture's u running start to end.
    pub fn write_mesh(&self, mesh: &mut Mesh2D, at: &WorldTransform) {
        let n = self.points.len();
        if n < 2 {
            return;
        }
        let total: f32 = self.points.windows(2).map(|w| w[0].distance(w[1])).sum::<f32>().max(1e-3);
        let half = self.width * 0.5;
        mesh.vertices.clear();
        let mut run = 0.0;
        for i in 0..n {
            let a = self.points[i.saturating_sub(1)];
            let b = self.points[(i + 1).min(n - 1)];
            let d = (b - a).normalize_or(Vec2::X);
            let side = Vec2::new(-d.y, d.x) * half;
            if i > 0 {
                run += self.points[i].distance(self.points[i - 1]);
            }
            let u = run / total;
            for (s, v) in [(1.0, 0.0), (-1.0, 1.0)] {
                mesh.vertices.push(MeshVertex {
                    position: to_local(at, self.points[i] + side * s).into(),
                    uv: [u, v],
                    color: [1.0, 1.0, 1.0, 1.0],
                    q: 1.0,
                });
            }
        }
        mesh.indices.clear();
        for i in 0..n - 1 {
            let a = (i * 2) as u32;
            // a (top), a+1 (bottom), a+3, a+2: one quad per segment.
            mesh.indices.extend_from_slice(&[a, a + 2, a + 3, a, a + 3, a + 1]);
        }
    }
}

/// `Cloth.set(e, field, value)`: write any field of the entity's `cloth` or
/// `rope` by name - `"wind"`, `"gust"`, `"flutter"`, `"length"`, `"end"` -
/// through the component's own serde, so a field takes what a scene file
/// would give it. The simulation's state is kept: the wind changes, the
/// cloth does not jump back to rest. A name the component has not got is
/// an error rather than a silent no-op.
pub fn set_cloth_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    fn write<T: Serialize + serde::de::DeserializeOwned>(
        c: &T,
        field: &str,
        value: &serde_json::Value,
        kind: &str,
    ) -> Result<T, String> {
        let mut whole = serde_json::to_value(c).map_err(|e| e.to_string())?;
        let map = whole.as_object_mut().ok_or("not an object")?;
        if !map.contains_key(field) {
            return Err(format!("'{field}' is not a {kind} field"));
        }
        // Scripts have only floats: a whole number over a count stays whole.
        let v = match (map.get(field), value) {
            (Some(serde_json::Value::Number(o)), serde_json::Value::Number(n)) if !o.is_f64() => n
                .as_f64()
                .filter(|f| f.fract() == 0.0 && *f >= 0.0)
                .map_or(value.clone(), |f| (f as u64).into()),
            _ => value.clone(),
        };
        map.insert(field.to_string(), v);
        serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))
    }
    if let Ok(mut c) = world.get::<&mut Cloth>(entity) {
        let mut new = write(&*c, field, value, "cloth")?;
        new.points = std::mem::take(&mut c.points);
        new.velocity = std::mem::take(&mut c.velocity);
        new.time = c.time;
        *c = new;
        return Ok(());
    }
    if let Ok(mut r) = world.get::<&mut Rope>(entity) {
        let mut new = write(&*r, field, value, "rope")?;
        new.points = std::mem::take(&mut r.points);
        new.velocity = std::mem::take(&mut r.velocity);
        new.time = r.time;
        *r = new;
        return Ok(());
    }
    Err("the entity has no cloth or rope".into())
}

fn world_of(world: &World, e: Entity) -> Option<WorldTransform> {
    if let Ok(w) = world.get::<&WorldTransform>(e) {
        return Some((*w).clone());
    }
    world.get::<&Transform>(e).ok().map(|t| WorldTransform::from(&*t))
}

thread_local! {
    /// Each simulated entity's transform last frame, so a pin that moved is
    /// swept across the frame's substeps rather than jumping on the first.
    static LAST: std::cell::RefCell<std::collections::HashMap<Entity, WorldTransform>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Step every `cloth` and `rope` and write their meshes. Every host calls
/// it right after the hierarchy resolve, as it does `tick_trails`.
pub fn tick_cloth(world: &mut World, dt: f32) {
    let _span = crate::profiling::trace::span("cloth");
    let cloths: Vec<Entity> = world.query::<&Cloth>().iter().map(|(e, _)| e).collect();
    let ropes: Vec<Entity> = world.query::<&Rope>().iter().map(|(e, _)| e).collect();
    if cloths.is_empty() && ropes.is_empty() {
        return;
    }
    let wants_outlines = world.query::<&Cloth>().iter().any(|(_, c)| c.collide)
        || world.query::<&Rope>().iter().any(|(_, r)| r.collide);
    let outlines = wants_outlines.then(|| ColliderOutlines::gather(world));
    // Ropes name what they hang from; resolved once a frame, only if asked.
    let names: std::collections::HashMap<String, Entity> = if ropes.is_empty() {
        Default::default()
    } else {
        world.query::<&Name>().iter().map(|(e, n)| (n.0.clone(), e)).collect()
    };
    let last = LAST.with(|l| std::mem::take(&mut *l.borrow_mut()));
    let mut next: std::collections::HashMap<Entity, WorldTransform> = Default::default();
    let mut seen = |e: Entity, now: WorldTransform| {
        next.insert(e, now.clone());
        last.get(&e).cloned().unwrap_or(now)
    };

    for e in cloths {
        let Some(now) = world_of(world, e) else { continue };
        let before = seen(e, now.clone());
        if world.get::<&Mesh2D>(e).is_err() {
            let _ = world.insert_one(e, Mesh2D::default());
        }
        let (Ok(mut cloth), Ok(mut mesh)) = (world.get::<&mut Cloth>(e), world.get::<&mut Mesh2D>(e)) else { continue };
        cloth.step(&before, &now, outlines.as_ref(), dt);
        cloth.write_mesh(&mut mesh, &now);
    }

    for e in ropes {
        let Some(now) = world_of(world, e) else { continue };
        let before = seen(e, now.clone());
        let (start_offset, end_name, end_offset, pins, count) = {
            let Ok(r) = world.get::<&Rope>(e) else { continue };
            (Vec2::from(r.start_offset), r.end.clone(), Vec2::from(r.end_offset), r.pins.clone(), r.count())
        };
        let start = (before.apply(start_offset), now.apply(start_offset));
        let mut held = |name: &str, offset: Vec2| -> Option<(Vec2, Vec2)> {
            let &target = names.get(name)?;
            let now = world_of(world, target)?;
            let before = seen(target, now.clone());
            Some((before.apply(offset), now.apply(offset)))
        };
        let end = if end_name.is_empty() { None } else { held(&end_name, end_offset) };
        let pin_points: Vec<(usize, Vec2, Vec2)> = pins
            .iter()
            .filter_map(|p| {
                let (from, to) = held(&p.entity, Vec2::from(p.offset))?;
                let i = (p.at.clamp(0.0, 1.0) * (count - 1) as f32).round() as usize;
                Some((i, from, to))
            })
            .collect();
        if world.get::<&Mesh2D>(e).is_err() {
            let _ = world.insert_one(e, Mesh2D::default());
        }
        let (Ok(mut rope), Ok(mut mesh)) = (world.get::<&mut Rope>(e), world.get::<&mut Mesh2D>(e)) else { continue };
        rope.step(start, end, &pin_points, outlines.as_ref(), dt);
        rope.write_mesh(&mut mesh, &now);
    }
    LAST.with(|l| *l.borrow_mut() = next);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> WorldTransform {
        WorldTransform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::ONE, shear: 0.0 }
    }

    #[test]
    fn to_local_undoes_apply() {
        let w = WorldTransform { position: Vec2::new(10.0, -4.0), rotation: 0.7, scale: Vec2::new(2.0, 0.5), shear: 0.3 };
        let p = Vec2::new(3.0, 5.0);
        assert!(to_local(&w, w.apply(p)).distance(p) < 1e-4);
    }

    #[test]
    fn a_banner_hangs_and_keeps_its_size() {
        let mut c = Cloth { size: [100.0, 60.0], cols: 6, rows: 4, anchor: [0.5, 1.0], flutter: 0.0, gust: 0.0, ..Default::default() };
        let w = at(0.0, 0.0);
        for _ in 0..240 {
            c.step(&w, &w, None, 1.0 / 60.0);
        }
        let p = c.points();
        // The top row stays where it was pinned.
        assert!(p[0].distance(Vec2::new(-50.0, 0.0)) < 1e-3 && p[5].distance(Vec2::new(50.0, 0.0)) < 1e-3);
        // The bottom hangs about its height below, not stretched far past it.
        let bottom = p[18].y;
        assert!(bottom < -55.0 && bottom > -66.0, "hangs its height: {bottom}");
    }

    #[test]
    fn a_flag_flies_in_the_wind() {
        let mut c = Cloth {
            size: [100.0, 60.0], cols: 8, rows: 5, pin: ClothPin::Left, flutter: 0.0, gust: 0.0,
            wind: [1500.0, 0.0], ..Default::default()
        };
        let w = at(0.0, 0.0);
        for _ in 0..300 {
            c.step(&w, &w, None, 1.0 / 60.0);
        }
        // The fly end is out to the right of the pole, not hanging down it.
        let fly = c.points()[7];
        assert!(fly.x > 70.0, "the wind holds it out: {fly}");
    }

    #[test]
    fn a_rope_between_two_points_sags() {
        let mut r = Rope { length: 240.0, segments: 12, ..Default::default() };
        let (a, b) = (Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0));
        for _ in 0..300 {
            r.step((a, a), Some((b, b)), &[], None, 1.0 / 60.0);
        }
        let p = r.points();
        assert!(p[0] == a && p[12] == b, "held at both ends");
        let low = p[6].y;
        assert!(low < -40.0, "longer than the gap, it sags: {low}");
        let len: f32 = p.windows(2).map(|w| w[0].distance(w[1])).sum();
        assert!((len - 240.0).abs() < 8.0, "and keeps its length: {len}");
    }

    #[test]
    fn a_rope_blown_into_a_wall_stays_out_of_it() {
        let mut world = World::new();
        let col: crate::ecs::components::Collider2D =
            serde_json::from_value(serde_json::json!({ "shape": { "Box": { "width": 200.0, "height": 400.0 } } })).unwrap();
        // A wall whose left face is at x = 60, beside a rope hanging at x = 0.
        world.spawn((Transform { position: Vec2::new(160.0, -100.0), rotation: 0.0, scale: Vec2::ONE }, col));
        let outlines = ColliderOutlines::gather(&world);
        let mut r = Rope { length: 200.0, segments: 10, collide: true, wind: [3000.0, 0.0], gust: 0.0, ..Default::default() };
        let a = Vec2::new(0.0, 0.0);
        for _ in 0..300 {
            r.step((a, a), None, &[], Some(&outlines), 1.0 / 60.0);
        }
        let right = r.points().iter().map(|p| p.x).fold(f32::MIN, f32::max);
        assert!(right > 55.0, "the wind carries it to the wall: {right}");
        assert!(right < 61.0, "and the wall stops it at its face, x 60: {right}");
    }

    #[test]
    fn the_mesh_is_in_the_entitys_space() {
        let mut c = Cloth { size: [100.0, 60.0], cols: 3, rows: 2, flutter: 0.0, ..Default::default() };
        let w = at(500.0, 200.0);
        c.step(&w, &w, None, 0.0);
        let mut m = Mesh2D::default();
        c.write_mesh(&mut m, &w);
        assert_eq!(m.vertices.len(), 6);
        assert_eq!(m.indices.len(), 12);
        assert_eq!(m.vertices[0].position, [0.0, 0.0], "the top left sits on the origin");
        assert_eq!(m.vertices[5].uv, [1.0, 1.0]);
    }
}
