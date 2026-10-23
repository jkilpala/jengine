//! Wave function collapse (Oct 2026; Maxim Gumin's *WaveFunctionCollapse*,
//! the simple tiled model): a tilemap region filled with tiles that all fit
//! their neighbours, one cell at a time, so a level can be made rather than
//! drawn - and made **next to what is already there**, so it can go on for
//! ever.
//!
//! A [`WfcGenerator`] on a tilemap entity lists its `tiles`, each a gid and
//! the label of each of its four edges (up, right, down, left); two tiles may
//! sit side by side when the edges that touch carry the same label.
//! `Wfc.generate(e, x, y, w, h)` queues a region of its `layer`: every cell
//! starts able to be any tile, the tiles already in the layer just outside
//! the region narrow the cells beside them (so a new chunk joins the old one
//! seamlessly), and the edge of the layer narrows its cells to tiles whose
//! edge there is `outside`. Then, `cells_per_second` at a time (0: all at
//! once), the cell with fewest tiles left is settled - a tile picked by
//! `weight` - and what that rules out is carried to its neighbours, and on
//! out. The tile is written into the layer as it settles, so a region can be
//! watched being decided. A cell left with no tile is a contradiction: the
//! region is cleared and begun again from another seed, a few times before
//! it is given up with a log line. A finished region calls
//! `on_wfc_done(entity)` on the entity's script (`ecs::callbacks`).
//!
//! Each region's choices come from `seed`, where the region is and how many
//! regions the generator has begun, so a scene makes the same maps every time
//! it runs, and asking for a region again makes another. Ticked in `tick_motion`, so no host was
//! wired.

use std::collections::VecDeque;

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::tilemap::Tilemap;

/// One tile the generator may place.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WfcTile {
    pub gid: u32,
    /// The label of each edge: up, right, down, left.
    pub edges: [String; 4],
    /// How often it is picked against the others that fit.
    pub weight: f32,
}

impl Default for WfcTile {
    fn default() -> Self {
        Self { gid: 1, edges: Default::default(), weight: 1.0 }
    }
}

/// `wfc`: fills regions of a tilemap's layer with tiles that fit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WfcGenerator {
    /// The layer it writes.
    pub layer: String,
    /// At most 64.
    pub tiles: Vec<WfcTile>,
    /// How many cells are decided a second (0: a region at once).
    pub cells_per_second: f32,
    pub seed: u64,
    /// The label every edge facing off the layer must carry (empty: any).
    pub outside: String,
    /// Runtime state - never saved.
    #[serde(skip)]
    pub state: WfcState,
}

impl Default for WfcGenerator {
    fn default() -> Self {
        Self {
            layer: "ground".into(),
            tiles: Vec::new(),
            cells_per_second: 0.0,
            seed: 1,
            outside: String::new(),
            state: WfcState::default(),
        }
    }
}

/// Regions queued and the one being decided.
#[derive(Debug, Clone, Default)]
pub struct WfcState {
    pub queued: VecDeque<[u32; 4]>,
    job: Option<Job>,
    /// Regions begun, mixed into each one's choices: asking for the same
    /// region again makes another map, and the same run makes the same maps.
    started: u64,
    /// Regions finished, and given up, since the start - for tests.
    pub finished: u32,
    pub failed: u32,
}

#[derive(Debug, Clone)]
struct Job {
    region: [u32; 4],
    /// The tiles each cell may still be, as bits.
    wave: Vec<u64>,
    /// What the cells were narrowed to before anything was decided, to begin
    /// again from.
    start: Vec<u64>,
    settled: Vec<bool>,
    rng: u64,
    tries: u32,
    budget: f32,
}

const UP: usize = 0;
const RIGHT: usize = 1;
const DOWN: usize = 2;
const LEFT: usize = 3;
const STEP: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
const TRIES: u32 = 24;

fn opposite(d: usize) -> usize {
    (d + 2) % 4
}

fn next(rng: &mut u64) -> u64 {
    // xorshift64*
    let mut x = *rng;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *rng = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

fn unit(rng: &mut u64) -> f32 {
    (next(rng) >> 40) as f32 / (1u64 << 24) as f32
}

/// Which tiles may sit on side `d` of each tile, as bits.
fn compat(tiles: &[WfcTile]) -> [Vec<u64>; 4] {
    let mut out: [Vec<u64>; 4] = Default::default();
    for (d, list) in out.iter_mut().enumerate() {
        *list = tiles
            .iter()
            .map(|t| {
                tiles.iter().enumerate().fold(0u64, |m, (k, s)| {
                    if s.edges[opposite(d)] == t.edges[d] { m | (1 << k) } else { m }
                })
            })
            .collect();
    }
    out
}

/// What may be on side `d` of a cell that may be any of `mask`.
fn allowed(compat: &[Vec<u64>; 4], d: usize, mask: u64) -> u64 {
    let mut out = 0;
    let mut m = mask;
    while m != 0 {
        let k = m.trailing_zeros() as usize;
        out |= compat[d][k];
        m &= m - 1;
    }
    out
}

/// `Wfc.generate(e, x, y, w, h)`: queue a region of the generator's layer.
pub fn queue_region(world: &World, entity: Entity, x: i32, y: i32, w: i32, h: i32) -> Result<(), String> {
    let mut g = world.get::<&mut WfcGenerator>(entity).map_err(|_| "no wfc on that entity".to_string())?;
    if w <= 0 || h <= 0 {
        return Err(format!("an empty region ({w} x {h})"));
    }
    g.state.queued.push_back([x.max(0) as u32, y.max(0) as u32, w as u32, h as u32]);
    Ok(())
}

/// Decide cells of every generator's region. Inside `tick_motion`.
pub fn tick_wfc(world: &mut World, dt: f32) {
    let ids: Vec<Entity> = world.query::<&WfcGenerator>().iter().map(|(e, _)| e).collect();
    for e in ids {
        let Ok(mut q) = world.query_one::<(&mut WfcGenerator, &mut Tilemap)>(e) else { continue };
        let Some((g, map)) = q.get() else { continue };
        if g.tiles.is_empty() || g.tiles.len() > 64 {
            continue;
        }
        let Some(layer_w) = map.find_layer(&g.layer).map(|l| (l.width, l.height)) else { continue };
        let seed = g.seed;
        let g_outside = g.outside.clone();
        let compat = compat(&g.tiles);
        let WfcGenerator { tiles, layer, state, cells_per_second, .. } = &mut *g;
        let rate = *cells_per_second;
        let carry = state.job.as_ref().map_or(0.0, |j| j.budget);
        let mut budget = if rate > 0.0 { rate * dt.max(0.0) + carry } else { f32::INFINITY };
        let mut done = false;
        while budget >= 1.0 {
            if state.job.is_none() {
                let Some(region) = state.queued.pop_front() else { break };
                let r = clip(region, layer_w);
                if r[2] == 0 || r[3] == 0 {
                    continue;
                }
                let start = narrow_from_outside(tiles, layer, &g_outside, map, &compat, r, layer_w);
                let place = ((r[0] as u64) << 32 | r[1] as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                let rng = (seed ^ place ^ state.started.wrapping_mul(0xD1B5_4A32_D192_ED03)).max(1);
                state.started += 1;
                state.job = Some(Job {
                    region: r,
                    wave: start.clone(),
                    start,
                    settled: vec![false; (r[2] * r[3]) as usize],
                    rng,
                    tries: 0,
                    budget: 0.0,
                });
                clear(map, layer, r);
            }
            let job = state.job.as_mut().unwrap();
            match settle_one(job, tiles, &compat) {
                Step::Settled(i, k) => {
                    let (x, y) = (job.region[0] + i as u32 % job.region[2], job.region[1] + i as u32 / job.region[2]);
                    if let Some(l) = map.find_layer_mut(layer) {
                        l.set_tile(x, y, Some(tiles[k].gid));
                    }
                    budget -= 1.0;
                }
                Step::Done => {
                    state.job = None;
                    state.finished += 1;
                    done = true;
                }
                Step::Contradiction => {
                    let r = job.region;
                    job.tries += 1;
                    if job.tries > TRIES {
                        log::warn!("wfc: gave up on the region {r:?} after {TRIES} tries - the tiles cannot fill it");
                        state.job = None;
                        state.failed += 1;
                    } else {
                        job.wave = job.start.clone();
                        job.settled.iter_mut().for_each(|s| *s = false);
                        next(&mut job.rng);
                        clear(map, layer, r);
                    }
                    budget -= 1.0;
                }
            }
        }
        if let Some(job) = state.job.as_mut() {
            // Fractions of a cell are kept for the next frame.
            job.budget = if budget.is_finite() { budget.max(0.0) } else { 0.0 };
        }
        drop(q);
        if done {
            crate::ecs::callbacks::push(e, "on_wfc_done");
        }
    }
}

fn clip(r: [u32; 4], (w, h): (u32, u32)) -> [u32; 4] {
    let x = r[0].min(w);
    let y = r[1].min(h);
    [x, y, r[2].min(w - x), r[3].min(h - y)]
}

fn clear(map: &mut Tilemap, layer: &str, r: [u32; 4]) {
    if let Some(l) = map.find_layer_mut(layer) {
        for y in r[1]..r[1] + r[3] {
            for x in r[0]..r[0] + r[2] {
                l.set_tile(x, y, None);
            }
        }
    }
}

/// Every cell of the region narrowed by what is beside it outside the
/// region: a tile already placed there, or the edge of the layer.
fn narrow_from_outside(
    tiles: &[WfcTile],
    layer_name: &str,
    outside: &str,
    map: &Tilemap,
    compat: &[Vec<u64>; 4],
    r: [u32; 4],
    (lw, lh): (u32, u32),
) -> Vec<u64> {
    let all = if tiles.len() == 64 { u64::MAX } else { (1u64 << tiles.len()) - 1 };
    let layer = map.find_layer(layer_name);
    let mut wave = vec![all; (r[2] * r[3]) as usize];
    for cy in 0..r[3] {
        for cx in 0..r[2] {
            let (x, y) = ((r[0] + cx) as i32, (r[1] + cy) as i32);
            let i = (cy * r[2] + cx) as usize;
            for (d, (dx, dy)) in STEP.iter().enumerate() {
                let (nx, ny) = (x + dx, y + dy);
                let inside_region = nx >= r[0] as i32
                    && ny >= r[1] as i32
                    && nx < (r[0] + r[2]) as i32
                    && ny < (r[1] + r[3]) as i32;
                if inside_region {
                    continue;
                }
                if nx < 0 || ny < 0 || nx >= lw as i32 || ny >= lh as i32 {
                    if !outside.is_empty() {
                        wave[i] &= tiles.iter().enumerate().fold(0u64, |m, (k, t)| {
                            if t.edges[d] == outside { m | (1 << k) } else { m }
                        });
                    }
                    continue;
                }
                let gid = layer.and_then(|l| l.get_tile(nx as u32, ny as u32));
                if let Some(gid) = gid
                    && let Some(k) = tiles.iter().position(|t| t.gid == gid)
                {
                    wave[i] &= compat[opposite(d)][k];
                }
            }
        }
    }
    wave
}

enum Step {
    Settled(usize, usize),
    Done,
    Contradiction,
}

/// Settle the most decided cell left and carry what it rules out.
fn settle_one(job: &mut Job, tiles: &[WfcTile], compat: &[Vec<u64>; 4]) -> Step {
    let (w, h) = (job.region[2] as i32, job.region[3] as i32);
    // The open cell with fewest tiles left; ties broken at random.
    let mut best: Option<(u32, u64, usize)> = None;
    for (i, &m) in job.wave.iter().enumerate() {
        if job.settled[i] {
            continue;
        }
        let n = m.count_ones();
        if n == 0 {
            return Step::Contradiction;
        }
        let tie = (i as u64 ^ job.rng).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32;
        if best.is_none_or(|(bn, bt, _)| n < bn || (n == bn && tie < bt)) {
            best = Some((n, tie, i));
        }
    }
    let Some((_, _, i)) = best else { return Step::Done };
    // A tile by weight.
    let m = job.wave[i];
    let total: f32 = (0..tiles.len()).filter(|k| m & (1 << k) != 0).map(|k| tiles[k].weight.max(0.0)).sum();
    let mut pick = unit(&mut job.rng) * total;
    let mut chosen = m.trailing_zeros() as usize;
    for k in 0..tiles.len() {
        if m & (1 << k) == 0 {
            continue;
        }
        pick -= tiles[k].weight.max(0.0);
        if pick <= 0.0 {
            chosen = k;
            break;
        }
    }
    job.wave[i] = 1 << chosen;
    job.settled[i] = true;
    // Carry it outward.
    let mut stack = vec![i];
    while let Some(c) = stack.pop() {
        let (cx, cy) = ((c as i32) % w, (c as i32) / w);
        for (d, (dx, dy)) in STEP.iter().enumerate() {
            let (nx, ny) = (cx + dx, cy + dy);
            if nx < 0 || ny < 0 || nx >= w || ny >= h {
                continue;
            }
            let n = (ny * w + nx) as usize;
            let narrowed = job.wave[n] & allowed(compat, d, job.wave[c]);
            if narrowed != job.wave[n] {
                job.wave[n] = narrowed;
                if narrowed == 0 {
                    return Step::Contradiction;
                }
                stack.push(n);
            }
        }
    }
    let _ = (UP, RIGHT, DOWN, LEFT);
    Step::Settled(i, chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tilemap::TilemapLayer;

    /// Pipes: a tile per set of open sides (bit 1 up, 2 right, 4 down, 8
    /// left), gid 1 + the bits, edges "o" open or "x" shut.
    pub fn pipes() -> Vec<WfcTile> {
        (0..16)
            .map(|b: u32| WfcTile {
                gid: 1 + b,
                edges: std::array::from_fn(|d| if b & (1 << d) != 0 { "o".into() } else { "x".into() }),
                weight: if b == 0 { 0.2 } else { 1.0 },
            })
            .collect()
    }

    pub fn map(w: u32, h: u32) -> Tilemap {
        Tilemap {
            layers: vec![TilemapLayer { name: "ground".into(), width: w, height: h, tiles: vec![0; (w * h) as usize], ..Default::default() }],
            ..Default::default()
        }
    }

    pub fn fits(g: &WfcGenerator, m: &Tilemap) -> Result<(), String> {
        let l = m.find_layer("ground").unwrap();
        let tile = |x: u32, y: u32| l.get_tile(x, y).and_then(|gid| g.tiles.iter().find(|t| t.gid == gid));
        for y in 0..l.height {
            for x in 0..l.width {
                let Some(t) = tile(x, y) else { continue };
                if x + 1 < l.width && let Some(r) = tile(x + 1, y) && r.edges[LEFT] != t.edges[RIGHT] {
                    return Err(format!("({x}, {y}) and its right do not fit"));
                }
                if y + 1 < l.height && let Some(b) = tile(x, y + 1) && b.edges[UP] != t.edges[DOWN] {
                    return Err(format!("({x}, {y}) and below it do not fit"));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn a_region_is_filled_with_tiles_that_fit() {
        let mut world = World::new();
        let e = world.spawn((WfcGenerator { tiles: pipes(), outside: "x".into(), ..Default::default() }, map(12, 10)));
        queue_region(&world, e, 0, 0, 12, 10).unwrap();
        tick_wfc(&mut world, 0.016);
        let g = world.get::<&WfcGenerator>(e).unwrap();
        let m = world.get::<&Tilemap>(e).unwrap();
        assert_eq!(g.state.finished, 1);
        let l = m.find_layer("ground").unwrap();
        assert!((0..10).all(|y| (0..12).all(|x| l.get_tile(x, y).is_some())), "every cell filled");
        fits(&g, &m).unwrap();
        // Nothing opens off the edge of the layer.
        for x in 0..12 {
            let top = l.get_tile(x, 0).unwrap() - 1;
            assert_eq!(top & 1, 0, "the top row opens upward at {x}");
        }
    }

    #[test]
    fn a_region_beside_another_joins_it_and_the_same_seed_makes_the_same_map() {
        let make = || {
            let mut world = World::new();
            let e = world.spawn((WfcGenerator { tiles: pipes(), seed: 7, ..Default::default() }, map(20, 8)));
            queue_region(&world, e, 0, 0, 10, 8).unwrap();
            queue_region(&world, e, 10, 0, 10, 8).unwrap();
            tick_wfc(&mut world, 0.016);
            let m = (*world.get::<&Tilemap>(e).unwrap()).clone();
            let g = (*world.get::<&WfcGenerator>(e).unwrap()).clone();
            (g, m)
        };
        let (g, m) = make();
        assert_eq!(g.state.finished, 2);
        fits(&g, &m).expect("the seam between the two regions fits");
        let (_, again) = make();
        assert_eq!(m.find_layer("ground").unwrap().tiles, again.find_layer("ground").unwrap().tiles);
    }

    #[test]
    fn a_region_is_decided_at_its_rate_and_says_when_it_is_done() {
        let mut world = World::new();
        let e = world.spawn((
            WfcGenerator { tiles: pipes(), cells_per_second: 60.0, ..Default::default() },
            map(8, 8),
        ));
        queue_region(&world, e, 0, 0, 8, 8).unwrap();
        tick_wfc(&mut world, 0.25);
        let placed = |w: &World| {
            let m = w.get::<&Tilemap>(e).unwrap();
            m.find_layer("ground").unwrap().tiles.iter().filter(|t| **t != 0).count()
        };
        let quarter = placed(&world);
        assert!((13..=16).contains(&quarter), "a quarter second at 60 cells a second: {quarter}");
        for _ in 0..10 {
            tick_wfc(&mut world, 0.25);
        }
        assert_eq!(placed(&world), 64);
        assert_eq!(world.get::<&WfcGenerator>(e).unwrap().state.finished, 1);
        let calls = crate::ecs::callbacks::take();
        assert!(calls.iter().any(|c| c.entity == e && c.handler == "on_wfc_done"), "{calls:?}");
    }
}
