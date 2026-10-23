//! Falling sand (Oct 2026; Noita's world, the Powder Toy): a grid of cells,
//! each a material, that a simulation moves a few times a frame - sand
//! falls and piles, water flows and levels, oil floats on water and burns,
//! fire rises and spreads to what burns, smoke and steam drift up and thin,
//! lava runs slowly and turns water to steam and itself to stone.
//!
//! A [`PixelSim`] is drawn by its entity's sprite (its quad and place) and
//! is solid where its cells are: it keeps the entity's `carvable`
//! (`ecs::carve`) pointed at a texture of its own, so the sprite pass draws
//! the simulation and the physics builds its collider from the solid cells -
//! stone, wood and sand at rest - rebuilt at most every `collider_interval`
//! seconds, so a crate sits on a sand dune and falls when it is washed away.
//! Fire and lava also glow: a second texture of only them, drawn additively
//! over the sprite (`glow`), which bloom picks up.
//!
//! Cells step in a fixed `steps_per_second` (they are rules, not physics -
//! the same at any frame rate), bottom row up, each row's direction
//! alternating so nothing drifts one way, each cell moving at most once a
//! step. Start it from a picture (`seed_texture`: every opaque pixel the
//! material whose key colour is nearest, see [`MATERIALS`]), pour from
//! `sources`, and paint from a script with `PixelSim.paint`.

use std::sync::Arc;

use glam::Vec2;
use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::asset::{AssetManager, RawTextureData};
use crate::ecs::components::{SpriteComponent, Transform, WorldTransform};

/// What a cell is. 0 is empty.
pub type Material = u8;
pub const EMPTY: Material = 0;
pub const STONE: Material = 1;
pub const SAND: Material = 2;
pub const WATER: Material = 3;
pub const OIL: Material = 4;
pub const FIRE: Material = 5;
pub const SMOKE: Material = 6;
pub const LAVA: Material = 7;
pub const STEAM: Material = 8;
pub const WOOD: Material = 9;

/// Every material by name, with the key colour a seed picture uses for it
/// (sRGB).
pub const MATERIALS: [(&str, Material, [u8; 3]); 10] = [
    ("empty", EMPTY, [0, 0, 0]),
    ("stone", STONE, [128, 128, 128]),
    ("sand", SAND, [230, 200, 110]),
    ("water", WATER, [40, 110, 230]),
    ("oil", OIL, [70, 40, 20]),
    ("fire", FIRE, [255, 140, 0]),
    ("smoke", SMOKE, [60, 60, 60]),
    ("lava", LAVA, [255, 60, 0]),
    ("steam", STEAM, [220, 230, 240]),
    ("wood", WOOD, [130, 80, 40]),
];

/// A material by its name (`"sand"`), as scripts and sources name them.
pub fn material_named(name: &str) -> Option<Material> {
    MATERIALS.iter().find(|(n, _, _)| *n == name).map(|(_, m, _)| *m)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Empty,
    Static,
    Powder,
    Liquid,
    Gas,
}

fn kind(m: Material) -> Kind {
    match m {
        EMPTY => Kind::Empty,
        STONE | WOOD => Kind::Static,
        SAND => Kind::Powder,
        WATER | OIL | LAVA => Kind::Liquid,
        _ => Kind::Gas,
    }
}

/// How heavy, against what is in the way: a heavier cell sinks through a
/// lighter one that can move.
fn density(m: Material) -> u8 {
    match m {
        EMPTY => 0,
        FIRE | SMOKE | STEAM => 1,
        OIL => 3,
        WATER => 4,
        LAVA => 6,
        SAND => 8,
        _ => 255,
    }
}

fn flammable(m: Material) -> bool {
    m == OIL || m == WOOD
}

/// A spout pouring a material.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PixelSource {
    /// Where, in the entity's local units (the sprite's frame).
    pub at: [f32; 2],
    pub material: String,
    /// Cells a second.
    pub rate: f32,
    /// How wide it pours, local units.
    pub radius: f32,
    pub active: bool,
}

impl Default for PixelSource {
    fn default() -> Self {
        Self { at: [0.0, 0.0], material: "sand".into(), rate: 200.0, radius: 6.0, active: true }
    }
}

/// `pixel_sim`: a grid of falling-sand cells drawn by the entity's sprite.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PixelSim {
    /// Cells across and down; the sprite's size is what they cover.
    pub cells: [u32; 2],
    pub steps_per_second: f32,
    /// A picture to start from: each opaque pixel becomes the material whose
    /// key colour is nearest (scaled to `cells`). Empty starts empty.
    pub seed_texture: String,
    pub sources: Vec<PixelSource>,
    /// Build a solid collider from stone, wood and resting sand.
    pub collider: bool,
    pub collider_interval: f32,
    pub collider_step: u32,
    /// Fire and lava drawn again additively at this strength (0: no glow).
    pub glow: f32,
    pub seed: u32,
    /// Runtime state - never saved.
    #[serde(skip)]
    pub state: PixelState,
}

impl Default for PixelSim {
    fn default() -> Self {
        Self {
            cells: [320, 180],
            steps_per_second: 60.0,
            seed_texture: String::new(),
            sources: Vec::new(),
            collider: true,
            collider_interval: 0.3,
            collider_step: 2,
            glow: 3.0,
            seed: 0x5eed_1234,
            state: PixelState::default(),
        }
    }
}

/// A simulation's cells and bookkeeping.
#[derive(Debug, Clone, Default)]
pub struct PixelState {
    pub width: u32,
    pub height: u32,
    pub mat: Vec<Material>,
    /// Gas, fire: steps left. Everything else: a shade for its colour.
    pub life: Vec<u8>,
    clock: Vec<u8>,
    tick: u8,
    pub seeded: bool,
    pending: f32,
    pour: Vec<f32>,
    collider_age: f32,
    dice: u32,
    /// The picture changed since it was last uploaded.
    pub dirty: bool,
    /// Paint queued by scripts: world point, radius, material.
    pub paints: Vec<(Vec2, f32, Material)>,
}

/// Its texture's id, and its glow's.
pub fn texture_id(e: Entity) -> String {
    format!("__pixel_sim/{}", e.id())
}
pub fn glow_id(e: Entity) -> String {
    format!("__pixel_sim/{}#glow", e.id())
}

/// The prefix of every pixel simulation's texture: sampled nearest whatever
/// the project's default (`texture_filter::filter_for`), since a cell is a
/// pixel.
pub const TEXTURE_PREFIX: &str = "__pixel_sim/";

impl PixelState {
    fn new(w: u32, h: u32, seed: u32) -> Self {
        let n = (w * h) as usize;
        Self {
            width: w,
            height: h,
            mat: vec![EMPTY; n],
            life: vec![0; n],
            clock: vec![0; n],
            tick: 1,
            dice: seed.max(1),
            dirty: true,
            ..Default::default()
        }
    }

    fn roll(&mut self) -> u32 {
        let mut x = self.dice;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.dice = x;
        x
    }

    fn chance(&mut self, p: f32) -> bool {
        (self.roll() % 10_000) as f32 / 10_000.0 < p
    }

    #[inline]
    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height)
            .then(|| (y as u32 * self.width + x as u32) as usize)
    }

    pub fn at(&self, x: i32, y: i32) -> Material {
        self.idx(x, y).map_or(STONE, |i| self.mat[i])
    }

    /// Put `m` in cell `(x, y)`, a fresh shade or life.
    pub fn set(&mut self, x: i32, y: i32, m: Material) {
        let Some(i) = self.idx(x, y) else { return };
        let r = self.roll();
        self.mat[i] = m;
        self.life[i] = match m {
            FIRE => 30 + (r % 40) as u8,
            SMOKE => 90 + (r % 80) as u8,
            STEAM => 70 + (r % 70) as u8,
            _ => (r % 256) as u8,
        };
        self.clock[i] = self.tick;
        self.dirty = true;
    }

    fn swap(&mut self, a: usize, b: usize) {
        self.mat.swap(a, b);
        self.life.swap(a, b);
        self.clock[a] = self.tick;
        self.clock[b] = self.tick;
        self.dirty = true;
    }

    /// Move the cell at `i` into `(x, y)` if it can go there: empty, or a
    /// lighter thing that moves.
    fn try_move(&mut self, i: usize, x: i32, y: i32) -> bool {
        let Some(j) = self.idx(x, y) else { return false };
        let (m, t) = (self.mat[i], self.mat[j]);
        // Something that already moved this step stays where it went - else
        // one gas cell is handed along a whole row of liquid in one step.
        if t != EMPTY && self.clock[j] == self.tick {
            return false;
        }
        let k = kind(t);
        let can = match kind(m) {
            Kind::Gas => t == EMPTY,
            _ => t == EMPTY || ((k == Kind::Liquid || k == Kind::Gas) && density(t) < density(m)),
        };
        if can {
            self.swap(i, j);
        }
        can
    }

    /// One step of every rule.
    pub fn step(&mut self) {
        self.tick = self.tick.wrapping_add(1).max(1);
        let (w, h) = (self.width as i32, self.height as i32);
        for y in (0..h).rev() {
            let left_first = (y as u32 + self.tick as u32) % 2 == 0;
            for k in 0..w {
                let x = if left_first { k } else { w - 1 - k };
                let i = (y * w + x) as usize;
                if self.clock[i] == self.tick {
                    continue;
                }
                let m = self.mat[i];
                match kind(m) {
                    Kind::Empty | Kind::Static => {}
                    Kind::Powder => self.powder(i, x, y),
                    Kind::Liquid => self.liquid(i, x, y),
                    Kind::Gas => self.gas(i, x, y),
                }
            }
        }
    }

    fn side(&mut self) -> i32 {
        if self.roll() & 1 == 0 { -1 } else { 1 }
    }

    fn powder(&mut self, i: usize, x: i32, y: i32) {
        if self.try_move(i, x, y + 1) {
            return;
        }
        let d = self.side();
        if !self.try_move(i, x + d, y + 1) {
            self.try_move(i, x - d, y + 1);
        }
    }

    fn liquid(&mut self, i: usize, x: i32, y: i32) {
        let m = self.mat[i];
        // What it touches: lava turns water to steam and itself to stone,
        // and lights what burns.
        if m == LAVA {
            for (dx, dy) in [(0, -1), (1, 0), (-1, 0), (0, 1)] {
                let Some(j) = self.idx(x + dx, y + dy) else { continue };
                let t = self.mat[j];
                match t {
                    WATER => {
                        self.set(x + dx, y + dy, STEAM);
                        if self.chance(0.3) {
                            self.set(x, y, STONE);
                            return;
                        }
                    }
                    _ if flammable(t) && self.chance(0.08) => self.set(x + dx, y + dy, FIRE),
                    _ => {}
                }
            }
            // Thick: it moves one step in three.
            if self.roll() % 3 != 0 {
                return;
            }
        }
        if self.try_move(i, x, y + 1) {
            return;
        }
        let d = self.side();
        if self.try_move(i, x + d, y + 1) || self.try_move(i, x - d, y + 1) {
            return;
        }
        // Flow sideways as far as it runs free.
        let reach = match m {
            WATER => 5,
            OIL => 4,
            _ => 1,
        };
        // A heavier liquid beside a lighter one now and then slips under it
        // a cell - which, with sinking, is what lays oil on top of water.
        if let Some(j) = self.idx(x + d, y) {
            let t = self.mat[j];
            if kind(t) == Kind::Liquid && density(t) < density(m) && self.clock[j] != self.tick && self.chance(0.3) {
                self.swap(i, j);
                return;
            }
        }
        for dir in [d, -d] {
            let mut cx = x;
            let mut moved = false;
            let mut ci = i;
            for _ in 0..reach {
                let Some(j) = self.idx(cx + dir, y) else { break };
                let t = self.mat[j];
                // It runs into air, smoke and steam - not into fire, which
                // would be shuffled along and never get its turn.
                if t != EMPTY && (t == FIRE || kind(t) != Kind::Gas || self.clock[j] == self.tick) {
                    break;
                }
                self.swap(ci, j);
                ci = j;
                cx += dir;
                moved = true;
                // Fall again as soon as it can.
                if self.idx(cx, y + 1).is_some_and(|b| self.mat[b] == EMPTY) {
                    break;
                }
            }
            if moved {
                return;
            }
        }
    }

    fn gas(&mut self, i: usize, x: i32, y: i32) {
        let m = self.mat[i];
        // Gases thin away; fire burns down to smoke and lights what burns.
        if self.life[i] == 0 {
            let next = match m {
                FIRE if self.chance(0.4) => SMOKE,
                STEAM if self.chance(0.08) => WATER,
                _ => EMPTY,
            };
            self.set(x, y, next);
            return;
        }
        self.life[i] -= 1;
        if m == FIRE {
            let mut fuel = false;
            for (dx, dy) in [(0, -1), (1, 0), (-1, 0), (0, 1)] {
                let Some(j) = self.idx(x + dx, y + dy) else { continue };
                let t = self.mat[j];
                match t {
                    WATER => {
                        self.set(x, y, STEAM);
                        return;
                    }
                    _ if flammable(t) => {
                        fuel = true;
                        let p = if t == OIL { 0.25 } else { 0.05 };
                        if self.chance(p) {
                            self.set(x + dx, y + dy, FIRE);
                            if t == WOOD {
                                // Wood burns longer.
                                if let Some(j) = self.idx(x + dx, y + dy) {
                                    self.life[j] = self.life[j].saturating_add(60);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Fire on fuel stays put and burns it; free fire flickers up.
            if fuel || self.roll() % 2 == 0 {
                return;
            }
        }
        if self.try_move(i, x, y - 1) {
            return;
        }
        let d = self.side();
        if self.try_move(i, x + d, y - 1) || self.try_move(i, x - d, y - 1) {
            return;
        }
        if !self.try_move(i, x + d, y) {
            self.try_move(i, x - d, y);
        }
    }

    /// Paint a disc of `m` (or empty) about cell `(cx, cy)`, radius in cells.
    pub fn paint(&mut self, cx: f32, cy: f32, r: f32, m: Material) {
        let r = r.max(0.5);
        let (x0, x1) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
        let (y0, y1) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = Vec2::new(x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if d.length_squared() <= r * r && self.idx(x, y).is_some() {
                    // Paint does not overwrite stone with loose things.
                    if m == EMPTY || self.at(x, y) != STONE || kind(m) == Kind::Static {
                        self.set(x, y, m);
                    }
                }
            }
        }
    }

    /// The cells as a picture (sRGB, y down) and the glow of fire and lava.
    pub fn picture(&self) -> (Vec<u8>, Vec<u8>) {
        let n = self.mat.len();
        let mut rgba = vec![0u8; n * 4];
        let mut glow = vec![0u8; n * 4];
        for i in 0..n {
            let s = self.life[i] as f32 / 255.0;
            let v = |base: [f32; 3], spread: f32| base.map(|c| (c * (1.0 - spread * 0.5 + spread * s)).clamp(0.0, 255.0) as u8);
            let (c, a, g): ([u8; 3], u8, bool) = match self.mat[i] {
                EMPTY => continue,
                STONE => (v([112.0, 104.0, 110.0], 0.35), 255, false),
                WOOD => (v([116.0, 74.0, 42.0], 0.3), 255, false),
                SAND => (v([216.0, 184.0, 112.0], 0.25), 255, false),
                WATER => (v([38.0, 96.0, 196.0], 0.15), 210, false),
                OIL => (v([124.0, 86.0, 38.0], 0.25), 245, false),
                LAVA => (v([255.0, 108.0, 24.0], 0.4), 255, true),
                FIRE => {
                    let t = (self.life[i] as f32 / 70.0).clamp(0.0, 1.0);
                    ([255, (70.0 + 170.0 * t) as u8, (10.0 + 80.0 * t * t) as u8], 255, true)
                }
                SMOKE => {
                    let t = (self.life[i] as f32 / 170.0).clamp(0.0, 1.0);
                    ([64, 62, 66], (40.0 + 140.0 * t) as u8, false)
                }
                STEAM => {
                    let t = (self.life[i] as f32 / 140.0).clamp(0.0, 1.0);
                    ([214, 224, 236], (30.0 + 120.0 * t) as u8, false)
                }
                _ => continue,
            };
            rgba[i * 4..i * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], a]);
            if g {
                glow[i * 4..i * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        (rgba, glow)
    }

    /// Solid to bodies: stone, wood, and sand with sand or something solid
    /// under it.
    pub fn solid_mask(&self) -> Vec<u8> {
        let (w, h) = (self.width as i32, self.height as i32);
        let mut out = vec![0u8; self.mat.len()];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                out[i] = match self.mat[i] {
                    STONE | WOOD => 1,
                    SAND => {
                        let below = self.at(x, y + 1);
                        u8::from(below == SAND || kind(below) == Kind::Static)
                    }
                    _ => 0,
                };
            }
        }
        out
    }
}

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

/// Step every simulation: pour, apply the scripts' paint, run the rules.
/// Inside `tick_motion`, so every host runs it.
pub fn tick_pixel_sims(world: &mut World, dt: f32) {
    let _span = crate::profiling::trace::span("pixel_sims");
    let ids: Vec<Entity> = world.query::<&PixelSim>().iter().map(|(e, _)| e).collect();
    for e in ids {
        let Some((wt, size, anchor)) = frame_of(world, e) else { continue };
        let Ok(mut sim) = world.get::<&mut PixelSim>(e) else { continue };
        let [cw, ch] = [sim.cells[0].max(1), sim.cells[1].max(1)];
        if sim.state.width != cw || sim.state.height != ch {
            // Paints a script queued before the first step (in its `init`)
            // carry over.
            let paints = std::mem::take(&mut sim.state.paints);
            sim.state = PixelState::new(cw, ch, sim.seed);
            sim.state.seeded = sim.seed_texture.is_empty();
            sim.state.paints = paints;
        }
        if !sim.state.seeded {
            continue;
        }
        let cells = sim.cells;
        let to_cell = |p: Vec2| cell_of(world_to_local(&wt, p), size, anchor, cells);
        let per_cell = cells[0] as f32 / size.x.max(1e-3) / wt.scale.x.abs().max(1e-3);
        let paints = std::mem::take(&mut sim.state.paints);
        for (p, r, m) in paints {
            let c = to_cell(p);
            sim.state.paint(c.x, c.y, r * per_cell, m);
        }
        let steps = {
            let step = 1.0 / sim.steps_per_second.clamp(1.0, 240.0);
            let s = &mut sim.state;
            s.pending += dt.clamp(0.0, 0.1);
            let n = (s.pending / step + 1e-3).floor();
            s.pending = (s.pending - n * step).max(0.0);
            n as usize
        };
        // Pour: each source's cells for the time stepped.
        let n_sources = sim.sources.len();
        if sim.state.pour.len() != n_sources {
            sim.state.pour = vec![0.0; n_sources];
        }
        let step = 1.0 / sim.steps_per_second.clamp(1.0, 240.0);
        for k in 0..n_sources {
            let src = sim.sources[k].clone();
            let Some(m) = material_named(&src.material) else { continue };
            if !src.active {
                continue;
            }
            sim.state.pour[k] += src.rate.max(0.0) * steps as f32 * step;
            let c = cell_of(Vec2::from(src.at), size, anchor, cells);
            let r = (src.radius * cells[0] as f32 / size.x.max(1e-3)).max(0.5);
            while sim.state.pour[k] >= 1.0 {
                sim.state.pour[k] -= 1.0;
                let a = (sim.state.roll() % 6283) as f32 / 1000.0;
                let d = (sim.state.roll() % 1000) as f32 / 1000.0 * r;
                let (x, y) = ((c.x + a.cos() * d) as i32, (c.y + a.sin() * d) as i32);
                if sim.state.at(x, y) == EMPTY {
                    sim.state.set(x, y, m);
                }
            }
        }
        for _ in 0..steps {
            sim.state.step();
        }
        sim.state.collider_age += dt;
    }
}

/// Upload what changed, seed from a picture once it is loaded, and keep the
/// entity's `carvable` pointed at the simulation - its texture for the
/// sprite pass, its solid cells for the physics. Rides on
/// `carve::apply_carves` (pixels), so no host was wired.
pub fn sync(world: &mut World, assets: &mut AssetManager) {
    let ids: Vec<Entity> = world.query::<&PixelSim>().iter().map(|(e, _)| e).collect();
    let mut add_carvable = Vec::new();
    for e in ids {
        let Ok(mut sim) = world.get::<&mut PixelSim>(e) else { continue };
        if sim.state.width == 0 {
            continue;
        }
        if !sim.state.seeded {
            let Some(src) = assets.get_texture_bytes(&sim.seed_texture) else { continue };
            let src = src.clone();
            let (w, h) = (sim.state.width, sim.state.height);
            for y in 0..h {
                for x in 0..w {
                    let sx = (x * src.width / w).min(src.width - 1);
                    let sy = (y * src.height / h).min(src.height - 1);
                    let p = &src.rgba[((sy * src.width + sx) * 4) as usize..][..4];
                    if p[3] < 128 {
                        continue;
                    }
                    let best = MATERIALS
                        .iter()
                        .skip(1)
                        .min_by_key(|(_, _, c)| {
                            (0..3).map(|k| (c[k] as i32 - p[k] as i32).pow(2)).sum::<i32>()
                        })
                        .map(|(_, m, _)| *m)
                        .unwrap_or(EMPTY);
                    sim.state.set(x as i32, y as i32, best);
                }
            }
            sim.state.seeded = true;
        }
        let (w, h) = (sim.state.width, sim.state.height);
        let id = texture_id(e);
        if sim.state.dirty || assets.get_texture_bytes(&id).is_none() {
            let (rgba, glow) = sim.state.picture();
            // Written in place once it exists, so the GPU copy is patched,
            // not rebuilt.
            let mut put = |id: String, bytes: Vec<u8>| match assets.texture_bytes_mut(&id) {
                Some(t) if t.width == w && t.height == h => {
                    t.rgba.copy_from_slice(&bytes);
                    assets.mark_texture_dirty(&id, 0, 0, w, h);
                }
                _ => assets.insert_texture_bytes(id, RawTextureData { width: w, height: h, rgba: bytes }),
            };
            put(id.clone(), rgba);
            if sim.glow > 0.0 {
                put(glow_id(e), glow);
            }
            sim.state.dirty = false;
        }
        let collider = sim.collider;
        let due = sim.state.collider_age >= sim.collider_interval.max(0.0);
        let step = sim.collider_step.max(1);
        let mask = (collider && due).then(|| sim.state.solid_mask());
        if mask.is_some() {
            sim.state.collider_age = 0.0;
        }
        drop(sim);
        let source = world
            .get::<&SpriteComponent>(e)
            .map(|s| crate::asset::resolve_texture_id(&s.texture_id).to_string())
            .unwrap_or_default();
        match world.get::<&mut crate::ecs::carve::Carvable>(e) {
            Ok(mut c) => {
                let fresh = c.state.texture.as_deref() != Some(id.as_str());
                if fresh {
                    c.state.texture = Some(id.clone());
                    c.state.source = source;
                    c.state.width = w;
                    c.state.height = h;
                    c.state.mask = Arc::new(vec![0; (w * h) as usize]);
                }
                c.collider = collider;
                c.collider_step = step;
                if let Some(mask) = mask
                    && *c.state.mask != mask
                {
                    c.state.mask = Arc::new(mask);
                    c.state.rev += 1;
                }
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

/// `PixelSim.paint(e, x, y, radius, material)`: a disc of a material (or
/// `"empty"`) at a world point, applied at the next step.
pub fn queue_paint(world: &World, entity: Entity, x: f32, y: f32, radius: f32, material: &str) -> Result<(), String> {
    let m = material_named(material).ok_or_else(|| format!("'{material}' is not a material"))?;
    let mut sim = world.get::<&mut PixelSim>(entity).map_err(|_| "no pixel_sim on that entity".to_string())?;
    sim.state.paints.push((Vec2::new(x, y), radius.max(0.0), m));
    Ok(())
}

/// The glow quad's corners in world space and its texture, for the sprite
/// pass (`render_world_items`), and its strength.
pub fn glow_quads(world: &World) -> Vec<(Entity, [Vec2; 4], f32, i32)> {
    world
        .query::<(&PixelSim, &SpriteComponent, &Transform, Option<&WorldTransform>)>()
        .iter()
        .filter(|(_, (p, _, _, _))| p.glow > 0.0 && p.state.width > 0)
        .map(|(e, (p, s, t, w))| {
            let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
            let size = s.drawn_size(Vec2::splat(64.0));
            let (x0, x1) = (-s.anchor[0] * size.x, (1.0 - s.anchor[0]) * size.x);
            let (y0, y1) = (-s.anchor[1] * size.y, (1.0 - s.anchor[1]) * size.y);
            let c = [Vec2::new(x0, y1), Vec2::new(x1, y1), Vec2::new(x1, y0), Vec2::new(x0, y0)].map(|p| wt.apply(p));
            (e, c, p.glow, s.z_order)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(w: u32, h: u32) -> PixelState {
        PixelState::new(w, h, 7)
    }

    fn count(s: &PixelState, m: Material) -> usize {
        s.mat.iter().filter(|c| **c == m).count()
    }

    #[test]
    fn sand_falls_and_piles() {
        let mut s = grid(21, 20);
        for y in 0..5 {
            s.set(10, y, SAND);
        }
        for _ in 0..80 {
            s.step();
        }
        assert_eq!(count(&s, SAND), 5, "nothing is lost");
        assert_eq!(s.at(10, 19), SAND, "it reached the floor");
        let height = (0..20).rev().take_while(|y| s.at(10, *y) == SAND).count();
        assert!(height < 5, "and slumped into a pile rather than a column: {height}");
    }

    #[test]
    fn water_levels_and_oil_floats_on_it() {
        let mut s = grid(30, 20);
        for x in 0..10 {
            for y in 0..6 {
                s.set(x, y, WATER);
            }
        }
        for x in 20..30 {
            for y in 0..3 {
                s.set(x, y, OIL);
            }
        }
        for _ in 0..2000 {
            s.step();
        }
        // The bottom rows are water all across; oil only above it.
        for x in 0..30 {
            assert_eq!(s.at(x, 19), WATER, "the floor is water at {x}");
        }
        let oil_below_water = (0..30).any(|x| (1..20).any(|y| s.at(x, y) == OIL && s.at(x, y - 1) == WATER));
        assert!(!oil_below_water, "no water sits on oil");
        assert_eq!(count(&s, WATER), 60);
    }

    #[test]
    fn fire_burns_oil_to_smoke_and_lava_meets_water_as_stone() {
        let mut s = grid(20, 20);
        for x in 0..20 {
            s.set(x, 19, OIL);
        }
        s.set(0, 18, FIRE);
        for _ in 0..600 {
            s.step();
        }
        assert_eq!(count(&s, OIL), 0, "the fire ran along the oil and burned it all");
        let mut l = grid(10, 10);
        for x in 0..10 {
            l.set(x, 9, WATER);
            l.set(x, 8, WATER);
        }
        l.set(5, 0, LAVA);
        for _ in 0..200 {
            l.step();
        }
        assert!(count(&l, STONE) >= 1, "lava cooled to stone on the water");
        assert!(count(&l, WATER) < 20, "and boiled some of it away");
    }

    #[test]
    fn the_same_seed_steps_the_same() {
        let run = || {
            let mut s = grid(16, 16);
            s.paint(8.0, 2.0, 3.0, SAND);
            s.paint(4.0, 4.0, 2.0, WATER);
            for _ in 0..50 {
                s.step();
            }
            s.mat.clone()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn resting_sand_is_solid_and_falling_sand_is_not() {
        let mut s = grid(5, 5);
        s.set(2, 4, SAND);
        s.set(2, 3, SAND);
        s.set(0, 0, SAND);
        let m = s.solid_mask();
        assert_eq!(m[(4 * 5 + 2) as usize], 1);
        assert_eq!(m[(3 * 5 + 2) as usize], 1);
        assert_eq!(m[0], 0, "a grain in the air is not ground");
    }
}
