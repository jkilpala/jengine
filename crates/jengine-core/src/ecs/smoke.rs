//! Smoke (Oct 2026; Jos Stam's *Stable Fluids*): a grid of air that carries
//! smoke, curls and rises, and that what stands in it blocks and what moves
//! through it pushes.
//!
//! A [`Smoke`] is drawn by its entity's sprite, whose quad is the box the air
//! fills, as [`crate::ecs::pixel_sim`] is: `sync` keeps the entity's
//! `carvable` pointed at a texture of the smoke (with no collider), so the
//! sprite pass draws it - lit by the scene's lights like any lit sprite - and
//! no host was wired. Each step the air is
//!
//! - fed by its `sources` (smoke of a colour, heat and a push - a fan is a
//!   source with a push and no smoke),
//! - lifted by its heat (`buoyancy`) and weighed down by its smoke (`weight`),
//! - kept curling (`vorticity`, the confinement that gives back the small
//!   whirls a coarse grid loses),
//! - carried along itself (semi-Lagrangian advection - stable at any step),
//! - and made to flow without piling up or thinning out (a pressure solve,
//!   `iterations` Jacobi sweeps).
//!
//! Every non-sensor collider over the box is **solid** to it, rasterised a
//! step at a time; a collider that moves (by its body, its velocity or a
//! script - its speed is read from where it was last step, turning
//! included) drags the air in its cells along, so a swinging weight pushes
//! the smoke aside and leaves a wake. Steps are fixed (`steps_per_second`),
//! so smoke moves the same at any frame rate.

use std::sync::Arc;

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::asset::{AssetManager, RawTextureData};
use crate::ecs::components::{Collider2D, ColliderShape, SpriteComponent, Transform, WorldTransform};

/// A source of smoke, heat or wind.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SmokeSource {
    /// Where, in the entity's local units (the sprite's frame).
    pub at: [f32; 2],
    /// How wide, local units.
    pub radius: f32,
    /// Smoke added a second at its middle.
    pub density: f32,
    /// Heat added a second at its middle - what makes smoke rise.
    pub temperature: f32,
    /// The air in it is pushed toward this velocity (world units a second).
    pub velocity: [f32; 2],
    /// How hard: the share of the gap closed a second (0 leaves it alone).
    pub push: f32,
    /// The smoke's colour (linear).
    pub color: [f32; 4],
    pub active: bool,
}

impl Default for SmokeSource {
    fn default() -> Self {
        Self {
            at: [0.0, 0.0],
            radius: 20.0,
            density: 4.0,
            temperature: 4.0,
            velocity: [0.0, 0.0],
            push: 0.0,
            color: [0.5, 0.5, 0.5, 1.0],
            active: true,
        }
    }
}

/// `smoke`: a grid of air carrying smoke, drawn by the entity's sprite.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Smoke {
    /// Cells across and down; the sprite's size is what they cover.
    pub cells: [u32; 2],
    pub steps_per_second: f32,
    /// Jacobi sweeps of the pressure solve a step: more is less springy.
    pub iterations: u32,
    pub sources: Vec<SmokeSource>,
    /// Upward push per unit of heat (world units a second, a second).
    pub buoyancy: f32,
    /// Downward pull per unit of smoke.
    pub weight: f32,
    /// The share of heat lost a second.
    pub cooling: f32,
    /// The share of smoke lost a second.
    pub fade: f32,
    /// The share of motion lost a second.
    pub damping: f32,
    /// How hard small whirls are kept (vorticity confinement).
    pub vorticity: f32,
    /// How opaque a unit of smoke is: alpha is `1 - e^(-smoke * opacity)`.
    pub opacity: f32,
    /// Which edges are walls - left, right, top, bottom; an open edge lets
    /// the air (and its smoke) out.
    pub walls: [bool; 4],
    /// Colliders block it, and push it as they move.
    pub obstacles: bool,
    /// What `Smoke.puff` draws with when it names no colour (linear).
    pub color: [f32; 4],
    /// Runtime state - never saved.
    #[serde(skip)]
    pub state: SmokeState,
}

impl Default for Smoke {
    fn default() -> Self {
        Self {
            cells: [128, 72],
            steps_per_second: 60.0,
            iterations: 24,
            sources: Vec::new(),
            buoyancy: 300.0,
            weight: 4.0,
            cooling: 0.4,
            fade: 0.08,
            damping: 0.1,
            vorticity: 0.35,
            opacity: 1.2,
            walls: [true, true, false, true],
            obstacles: true,
            color: [0.5, 0.5, 0.5, 1.0],
            state: SmokeState::default(),
        }
    }
}

/// A puff a script asked for: world point, radius, smoke, push.
#[derive(Debug, Clone, Copy)]
pub struct Puff {
    pub at: Vec2,
    pub radius: f32,
    pub amount: f32,
    pub velocity: Vec2,
}

/// The air's fields, a cell each, row by row from the top. Velocity is in
/// world units a second, `y` up.
#[derive(Debug, Clone, Default)]
pub struct SmokeState {
    pub width: u32,
    pub height: u32,
    pub u: Vec<f32>,
    pub v: Vec<f32>,
    pub density: Vec<f32>,
    pub temperature: Vec<f32>,
    /// Smoke's colour, carried as colour times smoke.
    pub dye: [Vec<f32>; 3],
    /// Solid this step, and the solid's velocity there.
    pub solid: Vec<bool>,
    pub solid_u: Vec<f32>,
    pub solid_v: Vec<f32>,
    pressure: Vec<f32>,
    scratch: Vec<f32>,
    pending: f32,
    /// Where each collider was last step: entity bits, position, turn.
    last: Vec<(u64, Vec2, f32)>,
    pub puffs: Vec<Puff>,
    pub dirty: bool,
}

impl SmokeState {
    pub fn new(w: u32, h: u32) -> Self {
        let n = (w * h) as usize;
        Self {
            width: w,
            height: h,
            u: vec![0.0; n],
            v: vec![0.0; n],
            density: vec![0.0; n],
            temperature: vec![0.0; n],
            dye: [vec![0.0; n], vec![0.0; n], vec![0.0; n]],
            solid: vec![false; n],
            solid_u: vec![0.0; n],
            solid_v: vec![0.0; n],
            pressure: vec![0.0; n],
            scratch: vec![0.0; n],
            dirty: true,
            ..Default::default()
        }
    }

    fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.width as i32 + x) as usize
    }

    /// The smoke in a cell (0 outside).
    pub fn density_at(&self, x: i32, y: i32) -> f32 {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return 0.0;
        }
        self.density[self.idx(x, y)]
    }

    /// Bilinear sample of a field at a fractional cell (centres at +0.5);
    /// outside, the nearest edge cell for a wall, else nothing.
    fn sample(&self, f: &[f32], p: Vec2, walls: [bool; 4]) -> f32 {
        let (w, h) = (self.width as i32, self.height as i32);
        let x = p.x - 0.5;
        let y = p.y - 0.5;
        if (x < -0.5 && !walls[0]) || (x > w as f32 - 0.5 && !walls[1]) || (y < -0.5 && !walls[2]) || (y > h as f32 - 0.5 && !walls[3]) {
            return 0.0;
        }
        let x = x.clamp(0.0, (w - 1) as f32);
        let y = y.clamp(0.0, (h - 1) as f32);
        let (x0, y0) = (x.floor() as i32, y.floor() as i32);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let a = f[self.idx(x0, y0)] * (1.0 - fx) + f[self.idx(x1, y0)] * fx;
        let b = f[self.idx(x0, y1)] * (1.0 - fx) + f[self.idx(x1, y1)] * fx;
        a * (1.0 - fy) + b * fy
    }

    /// Add to the fields in a disc about cell `c` (fractional), `r` cells
    /// wide, falling off to its edge.
    fn splat(&mut self, c: Vec2, r: f32, mut f: impl FnMut(&mut Self, usize, f32)) {
        let r = r.max(0.75);
        let (w, h) = (self.width as i32, self.height as i32);
        for y in ((c.y - r).floor() as i32).max(0)..=((c.y + r).ceil() as i32).min(h - 1) {
            for x in ((c.x - r).floor() as i32).max(0)..=((c.x + r).ceil() as i32).min(w - 1) {
                let d = (Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - c).length() / r;
                if d < 1.0 {
                    let i = self.idx(x, y);
                    if !self.solid[i] {
                        let k = 1.0 - d * d;
                        f(self, i, k * k);
                    }
                }
            }
        }
    }

    /// One step of `dt` seconds, cells `cell` world units wide and high.
    fn step(&mut self, sim: &SmokeParams, cell: Vec2, dt: f32) {
        let (w, h) = (self.width as i32, self.height as i32);
        let n = self.u.len();
        // Heat lifts, smoke weighs.
        for i in 0..n {
            if !self.solid[i] {
                self.v[i] += (sim.buoyancy * self.temperature[i] - sim.weight * self.density[i]) * dt;
            }
        }
        // Small whirls kept.
        if sim.vorticity > 0.0 {
            let mut curl = std::mem::take(&mut self.scratch);
            for y in 0..h {
                for x in 0..w {
                    let i = self.idx(x, y);
                    let vx = |s: &Self, xx: i32| s.v[s.idx(xx.clamp(0, w - 1), y)];
                    let uy = |s: &Self, yy: i32| s.u[s.idx(x, yy.clamp(0, h - 1))];
                    // Row y - 1 is up.
                    let dvdx = (vx(self, x + 1) - vx(self, x - 1)) / (2.0 * cell.x);
                    let dudy = (uy(self, y - 1) - uy(self, y + 1)) / (2.0 * cell.y);
                    curl[i] = dvdx - dudy;
                }
            }
            let eps = sim.vorticity * cell.x.max(cell.y);
            for y in 1..h - 1 {
                for x in 1..w - 1 {
                    let i = self.idx(x, y);
                    if self.solid[i] {
                        continue;
                    }
                    let nx = (curl[self.idx(x + 1, y)].abs() - curl[self.idx(x - 1, y)].abs()) / (2.0 * cell.x);
                    let ny = (curl[self.idx(x, y - 1)].abs() - curl[self.idx(x, y + 1)].abs()) / (2.0 * cell.y);
                    let len = (nx * nx + ny * ny).sqrt() + 1e-5;
                    let (nx, ny) = (nx / len, ny / len);
                    self.u[i] += eps * ny * curl[i] * dt;
                    self.v[i] -= eps * nx * curl[i] * dt;
                }
            }
            self.scratch = curl;
        }
        self.solids_move();
        self.project(sim, cell);
        // Carry everything along the flow.
        let back = |s: &Self, x: i32, y: i32| {
            let i = s.idx(x, y);
            Vec2::new(x as f32 + 0.5 - s.u[i] * dt / cell.x, y as f32 + 0.5 + s.v[i] * dt / cell.y)
        };
        let walls = sim.walls;
        let mut fields: Vec<Vec<f32>> = Vec::with_capacity(7);
        for f in [&self.u, &self.v, &self.density, &self.temperature, &self.dye[0], &self.dye[1], &self.dye[2]] {
            let mut out = vec![0.0; n];
            for y in 0..h {
                for x in 0..w {
                    let i = self.idx(x, y);
                    if !self.solid[i] {
                        out[i] = self.sample(f, back(self, x, y), walls);
                    }
                }
            }
            fields.push(out);
        }
        let mut it = fields.into_iter();
        let fade_v = (-sim.damping * dt).exp();
        let fade_d = (-sim.fade * dt).exp();
        let fade_t = (-sim.cooling * dt).exp();
        self.u = it.next().unwrap();
        self.v = it.next().unwrap();
        self.density = it.next().unwrap();
        self.temperature = it.next().unwrap();
        for k in 0..3 {
            self.dye[k] = it.next().unwrap();
        }
        for i in 0..n {
            self.u[i] *= fade_v;
            self.v[i] *= fade_v;
            self.density[i] *= fade_d;
            self.temperature[i] *= fade_t;
            for k in 0..3 {
                self.dye[k][i] *= fade_d;
            }
        }
        self.solids_move();
        self.project(sim, cell);
        self.dirty = true;
    }

    /// A solid's cells take its velocity, and hold no smoke.
    fn solids_move(&mut self) {
        for i in 0..self.u.len() {
            if self.solid[i] {
                self.u[i] = self.solid_u[i];
                self.v[i] = self.solid_v[i];
                self.density[i] = 0.0;
                self.temperature[i] = 0.0;
                for k in 0..3 {
                    self.dye[k][i] = 0.0;
                }
            }
        }
    }

    /// Take the divergence out of the flow: solve for the pressure that
    /// would and subtract its gradient. A solid neighbour is a wall moving
    /// with the solid (its velocity in the divergence, no pressure jump
    /// across it); an open edge holds pressure 0.
    fn project(&mut self, sim: &SmokeParams, cell: Vec2) {
        let (w, h) = (self.width as i32, self.height as i32);
        let walls = sim.walls;
        let n = self.u.len();
        // Velocity of a neighbour, or at a grid edge: a wall's is zero
        // across it, an open edge's is the cell's own.
        let edge = |x: i32, y: i32| -> Option<usize> {
            if x < 0 || y < 0 || x >= w || y >= h { None } else { Some((y * w + x) as usize) }
        };
        let mut div = std::mem::take(&mut self.scratch);
        div.resize(n, 0.0);
        for y in 0..h {
            for x in 0..w {
                let i = self.idx(x, y);
                if self.solid[i] {
                    div[i] = 0.0;
                    continue;
                }
                let ux = |xx: i32, wall: bool| match edge(xx, y) {
                    Some(j) => self.u[j],
                    None if wall => 0.0,
                    None => self.u[i],
                };
                let vy = |yy: i32, wall: bool| match edge(x, yy) {
                    Some(j) => self.v[j],
                    None if wall => 0.0,
                    None => self.v[i],
                };
                div[i] = (ux(x + 1, walls[1]) - ux(x - 1, walls[0])) / (2.0 * cell.x)
                    + (vy(y - 1, walls[2]) - vy(y + 1, walls[3])) / (2.0 * cell.y);
            }
        }
        let (ix, iy) = (1.0 / (cell.x * cell.x), 1.0 / (cell.y * cell.y));
        let p = &mut self.pressure;
        // Red-black Gauss-Seidel, over-relaxed: in place, the same in any
        // order within a colour, and many times quicker to settle than
        // Jacobi. A solid neighbour or a wall is no pressure jump, so it
        // drops out; an open edge holds 0.
        const OMEGA: f32 = 1.8;
        for _ in 0..sim.iterations.max(1) {
            for colour in 0..2 {
                for y in 0..h {
                    for x in ((y + colour) & 1..w).step_by(2) {
                        let i = (y * w + x) as usize;
                        if self.solid[i] {
                            p[i] = 0.0;
                            continue;
                        }
                        let (mut sum, mut diag) = (0.0, 0.0);
                        for (xx, yy, wall, k) in
                            [(x - 1, y, walls[0], ix), (x + 1, y, walls[1], ix), (x, y - 1, walls[2], iy), (x, y + 1, walls[3], iy)]
                        {
                            match edge(xx, yy) {
                                Some(j) if !self.solid[j] => {
                                    sum += p[j] * k;
                                    diag += k;
                                }
                                Some(_) => {}
                                None if wall => {}
                                None => diag += k,
                            }
                        }
                        if diag > 0.0 {
                            let new = (sum - div[i]) / diag;
                            p[i] += (new - p[i]) * OMEGA;
                        }
                    }
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if self.solid[i] {
                    continue;
                }
                let pn = |xx: i32, yy: i32, wall: bool| match edge(xx, yy) {
                    Some(j) if !self.solid[j] => p[j],
                    Some(_) => p[i],
                    None if wall => p[i],
                    None => 0.0,
                };
                self.u[i] -= (pn(x + 1, y, walls[1]) - pn(x - 1, y, walls[0])) / (2.0 * cell.x);
                self.v[i] -= (pn(x, y - 1, walls[2]) - pn(x, y + 1, walls[3])) / (2.0 * cell.y);
            }
        }
        self.scratch = div;
    }

    /// The smoke as a picture (sRGB, y down): its colour, and its
    /// opacity from how much there is.
    pub fn picture(&self, opacity: f32) -> Vec<u8> {
        let n = self.density.len();
        let mut rgba = vec![0u8; n * 4];
        for i in 0..n {
            let d = self.density[i];
            if d <= 1e-4 {
                continue;
            }
            let a = 1.0 - (-d * opacity).exp();
            let c = [0, 1, 2].map(|k| to_srgb_byte(self.dye[k][i] / d));
            rgba[i * 4..i * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], (a * 255.0).round().clamp(0.0, 255.0) as u8]);
        }
        rgba
    }
}

fn to_srgb_byte(linear: f32) -> u8 {
    let l = linear.clamp(0.0, 1.0);
    let s = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// What a step reads from the component.
struct SmokeParams {
    buoyancy: f32,
    weight: f32,
    cooling: f32,
    fade: f32,
    damping: f32,
    vorticity: f32,
    iterations: u32,
    walls: [bool; 4],
}

/// Its texture's id.
pub fn texture_id(e: Entity) -> String {
    format!("{TEXTURE_PREFIX}{}", e.id())
}

/// What every smoke texture's id starts with.
pub const TEXTURE_PREFIX: &str = "__smoke/";

fn frame_of(world: &World, e: Entity) -> Option<(WorldTransform, Vec2, [f32; 2])> {
    let s = world.get::<&SpriteComponent>(e).ok()?;
    let wt = match (world.get::<&WorldTransform>(e), world.get::<&Transform>(e)) {
        (Ok(w), _) => (*w).clone(),
        (_, Ok(t)) => WorldTransform::from(&*t),
        _ => return None,
    };
    Some((wt, s.drawn_size(Vec2::splat(64.0)), s.anchor))
}

/// A local point (the sprite's frame) as a cell, fractional.
fn cell_of(local: Vec2, size: Vec2, anchor: [f32; 2], cells: [u32; 2]) -> Vec2 {
    let u = local.x / size.x + anchor[0];
    let v = 1.0 - (local.y / size.y + anchor[1]);
    Vec2::new(u * cells[0] as f32, v * cells[1] as f32)
}

fn world_to_local(wt: &WorldTransform, p: Vec2) -> Vec2 {
    let d = Vec2::from_angle(-wt.rotation).rotate(p - wt.position);
    d / Vec2::new(wt.scale.x.max(1e-6), wt.scale.y.max(1e-6))
}

/// A collider as something a point can be inside.
enum Solid {
    Circle(Vec2, f32),
    /// A convex loop, anticlockwise.
    Convex(Vec<Vec2>),
    /// Anything else: its box.
    Box(Vec2, Vec2),
}

impl Solid {
    fn of(t: &Transform, col: &Collider2D) -> Option<(Self, Vec2, Vec2)> {
        let rot = glam::Mat2::from_angle(t.rotation);
        let off = Vec2::from(col.offset);
        let place = |p: Vec2| t.position + rot * (p + off);
        let convex = |pts: Vec<Vec2>| {
            let area: f32 = (0..pts.len()).map(|i| pts[i].perp_dot(pts[(i + 1) % pts.len()])).sum();
            if area < 0.0 { pts.into_iter().rev().collect() } else { pts }
        };
        let s = match &col.shape {
            ColliderShape::Circle { radius } => Solid::Circle(place(Vec2::ZERO), *radius),
            ColliderShape::Box { width, height } => {
                let (hw, hh) = (width / 2.0, height / 2.0);
                Solid::Convex(convex(
                    [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)].map(|(x, y)| place(Vec2::new(x, y))).to_vec(),
                ))
            }
            ColliderShape::Capsule { half_height, radius } => {
                let mut pts = Vec::with_capacity(18);
                for (cy, from) in [(-*half_height, std::f32::consts::PI), (*half_height, 0.0)] {
                    for k in 0..=8 {
                        let a = from + std::f32::consts::PI * k as f32 / 8.0;
                        pts.push(place(Vec2::new(a.cos() * radius, cy + a.sin() * radius)));
                    }
                }
                Solid::Convex(convex(pts))
            }
            ColliderShape::Triangle { a, b, c } => {
                Solid::Convex(convex([a, b, c].map(|p| place(Vec2::from(*p))).to_vec()))
            }
            ColliderShape::ConvexPolygon { vertices } if vertices.len() >= 3 => {
                Solid::Convex(convex(vertices.iter().map(|p| place(Vec2::from(*p))).collect()))
            }
            ColliderShape::ConvexPolygon { vertices }
            | ColliderShape::ConvexDecomposition { vertices, .. }
            | ColliderShape::Polyline { vertices } => {
                if vertices.is_empty() {
                    return None;
                }
                let pts: Vec<Vec2> = vertices.iter().map(|p| place(Vec2::from(*p))).collect();
                let lo = pts.iter().fold(Vec2::splat(f32::MAX), |a, p| a.min(*p));
                let hi = pts.iter().fold(Vec2::splat(f32::MIN), |a, p| a.max(*p));
                Solid::Box(lo, hi)
            }
            _ => return None,
        };
        let (lo, hi) = match &s {
            Solid::Circle(c, r) => (*c - Vec2::splat(*r), *c + Vec2::splat(*r)),
            Solid::Convex(p) => (
                p.iter().fold(Vec2::splat(f32::MAX), |a, q| a.min(*q)),
                p.iter().fold(Vec2::splat(f32::MIN), |a, q| a.max(*q)),
            ),
            Solid::Box(lo, hi) => (*lo, *hi),
        };
        Some((s, lo, hi))
    }

    fn contains(&self, p: Vec2) -> bool {
        match self {
            Solid::Circle(c, r) => (p - *c).length_squared() <= r * r,
            Solid::Convex(pts) => (0..pts.len()).all(|i| (pts[(i + 1) % pts.len()] - pts[i]).perp_dot(p - pts[i]) >= 0.0),
            Solid::Box(lo, hi) => p.cmpge(*lo).all() && p.cmple(*hi).all(),
        }
    }
}

/// Which cells the colliders fill, and how fast each is moving there.
fn rasterise(world: &World, me: Entity, state: &mut SmokeState, wt: &WorldTransform, size: Vec2, anchor: [f32; 2], step: f32) {
    let (w, h) = (state.width, state.height);
    state.solid.iter_mut().for_each(|s| *s = false);
    state.solid_u.iter_mut().for_each(|s| *s = 0.0);
    state.solid_v.iter_mut().for_each(|s| *s = 0.0);
    let cells = [w, h];
    let mut now = Vec::new();
    for (e, (t, col)) in world.query::<(&Transform, &Collider2D)>().iter() {
        if e == me || col.is_sensor {
            continue;
        }
        let Some((solid, lo, hi)) = Solid::of(t, col) else { continue };
        let bits = e.to_bits().get();
        let (vel, spin) = match state.last.iter().find(|(b, _, _)| *b == bits) {
            Some(&(_, p, r)) => {
                let mut turn = t.rotation - r;
                turn = (turn + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                ((t.position - p) / step, turn / step)
            }
            None => (Vec2::ZERO, 0.0),
        };
        now.push((bits, t.position, t.rotation));
        // The box's corners as cells, then every cell centre in that range.
        let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)]
            .map(|p| cell_of(world_to_local(wt, p), size, anchor, cells));
        let clo = corners.iter().fold(Vec2::splat(f32::MAX), |a, p| a.min(*p));
        let chi = corners.iter().fold(Vec2::splat(f32::MIN), |a, p| a.max(*p));
        let x0 = (clo.x.floor() as i32).max(0);
        let x1 = (chi.x.ceil() as i32).min(w as i32 - 1);
        let y0 = (clo.y.floor() as i32).max(0);
        let y1 = (chi.y.ceil() as i32).min(h as i32 - 1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let local = Vec2::new(
                    ((x as f32 + 0.5) / w as f32 - anchor[0]) * size.x,
                    ((1.0 - (y as f32 + 0.5) / h as f32) - anchor[1]) * size.y,
                );
                let p = wt.apply(local);
                if solid.contains(p) {
                    let i = (y as u32 * w + x as u32) as usize;
                    state.solid[i] = true;
                    let r = p - t.position;
                    let v = vel + Vec2::new(-r.y, r.x) * spin;
                    state.solid_u[i] = v.x;
                    state.solid_v[i] = v.y;
                }
            }
        }
    }
    state.last = now;
}

/// Step every smoke: obstacles, sources, puffs, the air. Inside
/// `tick_motion`, so every host runs it.
pub fn tick_smoke(world: &mut World, dt: f32) {
    let _span = crate::profiling::trace::span("smoke");
    let ids: Vec<Entity> = world.query::<&Smoke>().iter().map(|(e, _)| e).collect();
    for e in ids {
        let Some((wt, size, anchor)) = frame_of(world, e) else { continue };
        let Ok(mut smoke) = world.get::<&mut Smoke>(e) else { continue };
        let [cw, ch] = [smoke.cells[0].max(2), smoke.cells[1].max(2)];
        if smoke.state.width != cw || smoke.state.height != ch {
            let puffs = std::mem::take(&mut smoke.state.puffs);
            smoke.state = SmokeState::new(cw, ch);
            smoke.state.puffs = puffs;
        }
        let step = 1.0 / smoke.steps_per_second.clamp(1.0, 240.0);
        let steps = {
            let s = &mut smoke.state;
            s.pending += dt.clamp(0.0, 0.1);
            let n = (s.pending / step + 1e-3).floor();
            s.pending = (s.pending - n * step).max(0.0);
            n as usize
        };
        let cells = [cw, ch];
        // A cell's size in world units (the sprite's, scaled).
        let cell = Vec2::new(
            size.x * wt.scale.x.abs() / cw as f32,
            size.y * wt.scale.y.abs() / ch as f32,
        )
        .max(Vec2::splat(1e-3));
        let per_cell = cw as f32 / size.x.max(1e-3);
        // Scripts' puffs, at once.
        let puffs = std::mem::take(&mut smoke.state.puffs);
        let tint = smoke.color;
        for p in puffs {
            let c = cell_of(world_to_local(&wt, p.at), size, anchor, cells);
            let r = p.radius / wt.scale.x.abs().max(1e-3) * per_cell;
            smoke.state.splat(c, r, |s, i, k| {
                s.density[i] += p.amount * k;
                for ch in 0..3 {
                    s.dye[ch][i] += tint[ch] * p.amount * k;
                }
                s.u[i] += p.velocity.x * k;
                s.v[i] += p.velocity.y * k;
            });
            smoke.state.dirty = true;
        }
        if steps == 0 {
            continue;
        }
        let params = SmokeParams {
            buoyancy: smoke.buoyancy,
            weight: smoke.weight,
            cooling: smoke.cooling,
            fade: smoke.fade,
            damping: smoke.damping,
            vorticity: smoke.vorticity,
            iterations: smoke.iterations,
            walls: smoke.walls,
        };
        let obstacles = smoke.obstacles;
        let sources = smoke.sources.clone();
        let mut state = std::mem::take(&mut smoke.state);
        drop(smoke);
        // A slow frame runs every step it owes (a tenth of a second at
        // most), or the smoke falls behind the frame rate.
        for _ in 0..steps.min(12) {
            if obstacles {
                rasterise(world, e, &mut state, &wt, size, anchor, step);
            }
            for src in sources.iter().filter(|s| s.active) {
                let c = cell_of(Vec2::from(src.at), size, anchor, cells);
                let r = src.radius * per_cell;
                let target = Vec2::from(src.velocity);
                let pull = 1.0 - (-src.push.max(0.0) * step).exp();
                state.splat(c, r, |s, i, k| {
                    let d = src.density * step * k;
                    s.density[i] += d;
                    for ch in 0..3 {
                        s.dye[ch][i] += src.color[ch] * d;
                    }
                    s.temperature[i] += src.temperature * step * k;
                    if pull > 0.0 {
                        s.u[i] += (target.x - s.u[i]) * pull * k;
                        s.v[i] += (target.y - s.v[i]) * pull * k;
                    }
                });
            }
            state.step(&params, cell, step);
        }
        if let Ok(mut smoke) = world.get::<&mut Smoke>(e) {
            smoke.state = state;
        }
    }
}

/// Upload each smoke's picture and point its entity's `carvable` at it (no
/// collider), so the sprite pass draws it. In `carve::apply_carves`.
pub fn sync(world: &mut World, assets: &mut AssetManager) {
    let ids: Vec<Entity> = world.query::<&Smoke>().iter().map(|(e, _)| e).collect();
    let mut add_carvable = Vec::new();
    for e in ids {
        let Ok(mut smoke) = world.get::<&mut Smoke>(e) else { continue };
        if smoke.state.width == 0 {
            continue;
        }
        let (w, h) = (smoke.state.width, smoke.state.height);
        let id = texture_id(e);
        if smoke.state.dirty || assets.get_texture_bytes(&id).is_none() {
            let rgba = smoke.state.picture(smoke.opacity);
            match assets.texture_bytes_mut(&id) {
                Some(t) if t.width == w && t.height == h => {
                    t.rgba.copy_from_slice(&rgba);
                    assets.mark_texture_dirty(&id, 0, 0, w, h);
                }
                _ => assets.insert_texture_bytes(id.clone(), RawTextureData { width: w, height: h, rgba }),
            }
            smoke.state.dirty = false;
        }
        drop(smoke);
        let source = world
            .get::<&SpriteComponent>(e)
            .map(|s| crate::asset::resolve_texture_id(&s.texture_id).to_string())
            .unwrap_or_default();
        match world.get::<&mut crate::ecs::carve::Carvable>(e) {
            Ok(mut c) => {
                if c.state.texture.as_deref() != Some(id.as_str()) {
                    c.state.texture = Some(id.clone());
                    c.state.source = source;
                    c.state.width = w;
                    c.state.height = h;
                    c.state.mask = Arc::new(vec![0; (w * h) as usize]);
                }
                c.collider = false;
            }
            Err(_) => add_carvable.push(e),
        }
    }
    for e in add_carvable {
        let _ = world.insert_one(
            e,
            crate::ecs::carve::Carvable { scorch: 0.0, collider: false, ..Default::default() },
        );
    }
}

/// `Smoke.puff(e, x, y, radius, amount, vx, vy)`: smoke and a push in a
/// disc at a world point, at the next step.
pub fn queue_puff(world: &World, entity: Entity, puff: Puff) -> Result<(), String> {
    let mut smoke = world.get::<&mut Smoke>(entity).map_err(|_| "no smoke on that entity".to_string())?;
    smoke.state.puffs.push(puff);
    Ok(())
}

/// `Smoke.set(e, field, value)`: any field by name, or a source's as
/// `"sources.<n>.<field>"` (`n` from 1). The air keeps moving.
pub fn set_smoke_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut smoke = world.get::<&mut Smoke>(entity).map_err(|_| "no smoke on that entity".to_string())?;
    let mut whole = serde_json::to_value(&*smoke).map_err(|e| e.to_string())?;
    let mut at = &mut whole;
    let parts: Vec<&str> = field.split('.').collect();
    for (k, part) in parts.iter().enumerate() {
        let last = k + 1 == parts.len();
        let next = match at {
            serde_json::Value::Object(map) => {
                if !map.contains_key(*part) {
                    return Err(format!("'{field}' is not a smoke field"));
                }
                map.get_mut(*part).unwrap()
            }
            serde_json::Value::Array(list) => {
                let n: usize = part.parse().map_err(|_| format!("'{field}': '{part}' is not an index"))?;
                let len = list.len();
                list.get_mut(n.wrapping_sub(1)).ok_or_else(|| format!("'{field}': no source {n} of {len}"))?
            }
            _ => return Err(format!("'{field}' is not a smoke field")),
        };
        if last {
            // Scripts have only floats: a whole number over a count stays whole.
            *next = match (&*next, value) {
                (serde_json::Value::Number(o), serde_json::Value::Number(n)) if !o.is_f64() => n
                    .as_f64()
                    .filter(|f| f.fract() == 0.0 && *f >= 0.0)
                    .map_or(value.clone(), |f| (f as u64).into()),
                _ => value.clone(),
            };
            break;
        }
        at = next;
    }
    let mut new: Smoke = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    new.state = std::mem::take(&mut smoke.state);
    *smoke = new;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> SmokeParams {
        let d = Smoke::default();
        SmokeParams {
            buoyancy: d.buoyancy,
            weight: d.weight,
            cooling: d.cooling,
            fade: 0.0,
            damping: d.damping,
            vorticity: d.vorticity,
            iterations: 40,
            walls: [true; 4],
        }
    }

    fn divergence(s: &SmokeState, cell: Vec2) -> f32 {
        let (w, h) = (s.width as i32, s.height as i32);
        let mut worst = 0.0f32;
        for y in 2..h - 2 {
            for x in 2..w - 2 {
                let i = s.idx(x, y);
                if s.solid[i] {
                    continue;
                }
                let d = (s.u[s.idx(x + 1, y)] - s.u[s.idx(x - 1, y)]) / (2.0 * cell.x)
                    + (s.v[s.idx(x, y - 1)] - s.v[s.idx(x, y + 1)]) / (2.0 * cell.y);
                worst = worst.max(d.abs());
            }
        }
        worst
    }

    #[test]
    fn the_solve_takes_out_the_divergence() {
        let mut s = SmokeState::new(32, 32);
        let cell = Vec2::splat(4.0);
        // A source of flow in the middle: the air in a disc pushed outward.
        for y in 0..32 {
            for x in 0..32 {
                let i = s.idx(x, y);
                let d = Vec2::new(x as f32 - 15.5, 15.5 - y as f32);
                let k = (1.0 - d.length() / 6.0).max(0.0);
                s.u[i] = d.x * 3.0 * k;
                s.v[i] = d.y * 3.0 * k;
            }
        }
        let before = divergence(&s, cell);
        let mut p = params();
        p.iterations = 60;
        s.project(&p, cell);
        let after = divergence(&s, cell);
        assert!(after < before * 0.2, "divergence {before} -> {after}");
    }

    #[test]
    fn hot_smoke_rises() {
        let mut s = SmokeState::new(24, 48);
        let cell = Vec2::splat(4.0);
        let p = params();
        s.splat(Vec2::new(12.0, 40.0), 3.0, |s, i, k| {
            s.density[i] += k;
            s.temperature[i] += 2.0 * k;
        });
        let centre_y = |s: &SmokeState| {
            let (mut m, mut t) = (0.0, 0.0);
            for y in 0..48 {
                for x in 0..24 {
                    let d = s.density_at(x, y);
                    m += d * y as f32;
                    t += d;
                }
            }
            m / t
        };
        let start = centre_y(&s);
        // A blob has to push the air above it aside and draw it in below,
        // so it starts slowly - two seconds.
        for _ in 0..120 {
            s.step(&p, cell, 1.0 / 60.0);
        }
        assert!(centre_y(&s) < start - 4.0, "the smoke rose: row {start} -> {}", centre_y(&s));
    }

    #[test]
    fn a_solid_keeps_smoke_out_and_a_moving_one_pushes_it() {
        let mut s = SmokeState::new(32, 16);
        let cell = Vec2::splat(4.0);
        let mut p = params();
        p.buoyancy = 0.0;
        p.weight = 0.0;
        // A wall moving right through the middle, as a swinging weight does.
        for y in 4..12 {
            for x in 8..10 {
                let i = s.idx(x, y);
                s.solid[i] = true;
                s.solid_u[i] = 200.0;
            }
        }
        for _ in 0..10 {
            s.step(&p, cell, 1.0 / 60.0);
        }
        let i = s.idx(9, 8);
        assert_eq!(s.density[i], 0.0, "no smoke inside it");
        let ahead = s.u[s.idx(11, 8)];
        assert!(ahead > 20.0, "the air ahead of it is pushed along: {ahead}");
    }

    #[test]
    fn a_field_and_a_sources_field_are_set_by_name() {
        let mut world = World::new();
        let e = world.spawn((Smoke { sources: vec![SmokeSource::default()], ..Default::default() },));
        set_smoke_field(&world, e, "fade", &serde_json::json!(0.5)).unwrap();
        set_smoke_field(&world, e, "sources.1.active", &serde_json::json!(false)).unwrap();
        set_smoke_field(&world, e, "iterations", &serde_json::json!(12.0)).unwrap();
        assert!(set_smoke_field(&world, e, "sources.2.active", &serde_json::json!(false)).is_err());
        assert!(set_smoke_field(&world, e, "custard", &serde_json::json!(1)).is_err());
        let s = world.get::<&Smoke>(e).unwrap();
        assert_eq!(s.fade, 0.5);
        assert_eq!(s.iterations, 12);
        assert!(!s.sources[0].active);
    }
}
