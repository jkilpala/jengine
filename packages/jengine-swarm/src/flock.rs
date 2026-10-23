//! The flocking simulation: a uniform spatial grid plus the three classic
//! boid steering rules.
//!
//! Kept out of `lib.rs` because it is the part with actual algorithmic content
//! — the package plumbing around it is boilerplate.
//!
//! # Why a grid
//!
//! Naive flocking compares every boid to every other, which is `O(n²)`: at
//! 4,000 boids that is 16 million distance checks per frame and the frame
//! budget is gone. The engine's own `apply_separation` is written that way, and
//! it is fine for the ten enemies it was built for.
//!
//! A uniform grid whose cell size is the neighbour radius means a boid only
//! ever looks at the 3×3 cells around it. Cost becomes `O(n · k)` where `k` is
//! the average neighbours per cell — roughly linear as long as the flock does
//! not collapse into one cell.
//!
//! The grid itself now lives in the engine, as
//! `jengine_core::ecs::spatial::UniformGrid` — this package had a private copy
//! because nothing shared existed, and `apply_separation` was meanwhile still
//! comparing every body to every other. One implementation now serves all
//! three callers.

use glam::Vec2;
use jengine_core::ecs::spatial::UniformGrid;

/// Tuning for one flock. Mirrors the authored `SwarmSpawner` fields.
#[derive(Debug, Clone, Copy)]
pub struct FlockParams {
    pub half_width: f32,
    pub half_height: f32,
    pub max_speed: f32,
    pub neighbor_radius: f32,
    pub separation_radius: f32,
    pub separation: f32,
    pub alignment: f32,
    pub cohesion: f32,
    /// Most neighbours one boid will consider in a frame.
    ///
    /// Without this, cost is quadratic *in a fixed area* however good the
    /// grid is: the area does not grow with the flock, so doubling the boids
    /// doubles the density and therefore doubles the work per boid. Measured
    /// on this package before the cap: 0.31 us/boid at 1k rising to 2.59 at
    /// 16k.
    ///
    /// Steering off the ~24 nearest neighbours is visually indistinguishable
    /// from steering off four hundred — the far ones are averaged into
    /// insignificance anyway — so the cap costs nothing that can be seen.
    ///
    /// It only works as intended if the *nearest* neighbours are the ones
    /// kept. The grid walk used to start at the corner of the 3×3 block, so a
    /// capped boid steered off whichever neighbours happened to lie up and to
    /// the left of it — a systematic bias, and it had to wade through two
    /// sparse cells before filling the cap. `UniformGrid` now visits the
    /// centre cell first: per-boid cost went from 0.70 to 0.30 us at 16k, and
    /// growth from 1k to 16k density fell from 2.3x to 1.1x, which is the cap
    /// doing its job for the first time.
    pub max_neighbors: u32,
    /// A point the whole flock is drawn toward, and how hard (an
    /// acceleration, world units a second squared, at any distance).
    pub target: Option<(Vec2, f32)>,
    /// A point boids flee: its position, the radius it frightens, and how
    /// hard - strongest at its centre, nothing at the radius.
    pub threat: Option<(Vec2, f32, f32)>,
}

impl Default for FlockParams {
    fn default() -> Self {
        Self {
            half_width: 620.0,
            half_height: 340.0,
            max_speed: 140.0,
            neighbor_radius: 48.0,
            separation_radius: 18.0,
            separation: 1.6,
            alignment: 0.7,
            cohesion: 0.5,
            max_neighbors: 24,
            target: None,
            threat: None,
        }
    }
}

/// Steer and integrate one frame of flocking, in place.
///
/// Separation, alignment and cohesion are accumulated in a single neighbour
/// pass — the neighbour walk dominates the cost, so doing it once and deriving
/// all three from it is most of the speed.
pub fn step(positions: &mut [Vec2], velocities: &mut [Vec2], params: &FlockParams, dt: f32) {
    let n = positions.len();
    if n == 0 {
        return;
    }
    // The engine's shared broadphase, not a private copy. This package had
    // its own for a while because neither `apply_separation` nor the trigger
    // system offered one usable from outside the engine; both now go through
    // the same grid.
    let grid = UniformGrid::build(positions, params.neighbor_radius.max(8.0));
    let neighbor_sq = params.neighbor_radius * params.neighbor_radius;
    let sep_sq = params.separation_radius * params.separation_radius;

    // Snapshot the flock in the grid's own cell order.
    //
    // Two things at once. First, steering must be computed against a snapshot:
    // velocities are read for alignment while being written, so without one a
    // boid would align to neighbours that had already moved this frame and the
    // result would depend on iteration order.
    //
    // Second, and this is where the time actually went: reading `prev_pos[j]`
    // for a neighbour `j` is a random access into a 128 KB array at 16k boids,
    // and there are two dozen of them per boid per frame. Permuting into cell
    // order first means the neighbour walk reads each cell as a contiguous run
    // instead — the same arithmetic against memory the prefetcher can follow.
    let order = grid.order();
    let snap_pos: Vec<Vec2> = order.iter().map(|&i| positions[i as usize]).collect();
    let snap_vel: Vec<Vec2> = order.iter().map(|&i| velocities[i as usize]).collect();

    // Iteration is by *slot* (position in cell order), not by boid index, so
    // the outer walk is sequential over the same arrays the inner one reads.
    for slot in 0..n {
        let p = snap_pos[slot];
        let mut sep = Vec2::ZERO;
        let mut align = Vec2::ZERO;
        let mut center = Vec2::ZERO;
        let mut neighbors = 0.0f32;

        grid.for_each_near_run(p, |start, end| {
            for k in start..end {
                if k == slot {
                    continue;
                }
                let d = snap_pos[k] - p;
                let d2 = d.length_squared();
                if d2 > neighbor_sq || d2 <= 1e-6 {
                    continue;
                }
                neighbors += 1.0;
                align += snap_vel[k];
                center += snap_pos[k];
                if d2 < sep_sq {
                    // Push away, weighted by closeness: the nearer the
                    // neighbour, the harder the shove. `d2` not `sqrt(d2)` on
                    // purpose - it is a steering weight, not a physical force.
                    sep -= d / d2.max(1e-4);
                }
                // Stop once we have enough to steer by; see `max_neighbors`.
                // Returning from inside the run abandons the whole walk, which
                // is the point - in a dense cell the walk *is* the cost.
                if neighbors as u32 >= params.max_neighbors {
                    return false;
                }
            }
            true
        });

        let mut steer = Vec2::ZERO;
        if neighbors > 0.0 {
            steer += sep * params.separation * 100.0;
            steer += (align / neighbors - snap_vel[slot]) * params.alignment;
            steer += (center / neighbors - p) * params.cohesion;
        }

        if let Some((target, pull)) = params.target {
            steer += (target - p).normalize_or_zero() * pull;
        }
        if let Some((threat, radius, strength)) = params.threat {
            let away = p - threat;
            let d = away.length();
            if d < radius && d > 1e-3 {
                // A frightened boid may break the speed limit for a moment -
                // the burst of speed is what makes a murmuration tear open.
                steer += away / d * strength * (1.0 - d / radius);
            }
        }

        // Turn away from the boundary before reaching it, so the flock stays
        // in view without the teleport a wrap would cause or the pile-up a
        // hard clamp would.
        let margin = 60.0;
        if p.x < -params.half_width + margin {
            steer.x += params.max_speed;
        } else if p.x > params.half_width - margin {
            steer.x -= params.max_speed;
        }
        if p.y < -params.half_height + margin {
            steer.y += params.max_speed;
        } else if p.y > params.half_height - margin {
            steer.y -= params.max_speed;
        }

        let mut v = snap_vel[slot] + steer * dt;
        let speed = v.length();
        if speed > params.max_speed {
            v = v / speed * params.max_speed;
        } else if speed < params.max_speed * 0.35 {
            // Keep them moving: a boid that stalls in the middle of a flock
            // never recovers, because every rule is relative to neighbours.
            if speed > 1e-4 {
                v = v / speed * (params.max_speed * 0.35);
            } else {
                v = Vec2::new(params.max_speed * 0.35, 0.0);
            }
        }

        // Results go back to the caller's own indexing. This is a scattered
        // write, but there is one per boid rather than one per neighbour.
        let dst = order[slot] as usize;
        velocities[dst] = v;
        // Hard clamp as a backstop: steering keeps them in, but a boid shoved
        // out by a dense cluster must not escape to infinity.
        let np = p + v * dt;
        positions[dst] = Vec2::new(
            np.x.clamp(-params.half_width, params.half_width),
            np.y.clamp(-params.half_height, params.half_height),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flock(n: usize) -> (Vec<Vec2>, Vec<Vec2>) {
        let p = (0..n).map(|i| Vec2::new((hash01(i as u32 * 3) - 0.5) * 400.0, (hash01(i as u32 * 5 + 1) - 0.5) * 300.0)).collect();
        let v = (0..n).map(|i| Vec2::from_angle(hash01(i as u32 * 7 + 2) * 6.28) * 60.0).collect();
        (p, v)
    }

    fn mean(v: &[Vec2]) -> Vec2 {
        v.iter().copied().sum::<Vec2>() / v.len() as f32
    }

    #[test]
    fn a_target_draws_the_flock_toward_it() {
        let (mut p, mut v) = flock(300);
        let target = Vec2::new(350.0, 200.0);
        let params = FlockParams { target: Some((target, 400.0)), ..FlockParams::default() };
        let before = (mean(&p) - target).length();
        for _ in 0..180 {
            step(&mut p, &mut v, &params, 1.0 / 60.0);
        }
        let after = (mean(&p) - target).length();
        assert!(after < before * 0.5, "the flock closed on its target: {before} -> {after}");
    }

    #[test]
    fn a_threat_clears_a_hole_in_the_flock() {
        let (mut p, mut v) = flock(600);
        let params = FlockParams { threat: Some((Vec2::ZERO, 90.0, 1500.0)), ..FlockParams::default() };
        let near = |p: &[Vec2]| p.iter().filter(|q| q.length() < 45.0).count();
        let before = near(&p);
        for _ in 0..90 {
            step(&mut p, &mut v, &params, 1.0 / 60.0);
        }
        assert!(before > 5, "the test flock starts with boids at the middle: {before}");
        assert!(near(&p) * 4 < before, "they fled it: {before} -> {}", near(&p));
    }
}

/// Deterministic pseudo-random in `0.0..1.0` from an integer seed.
///
/// A hash rather than a real PRNG so spawning is reproducible across runs and
/// runtimes without pulling in `rand` — the same scene must look the same
/// natively and in the editor when comparing frame rates.
pub fn hash01(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    x = ((x >> ((x >> 28).wrapping_add(4))) ^ x).wrapping_mul(277_803_737);
    x = (x >> 22) ^ x;
    (x & 0x00FF_FFFF) as f32 / 16_777_216.0
}
