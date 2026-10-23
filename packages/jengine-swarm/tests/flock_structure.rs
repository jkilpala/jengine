//! Does the flock actually flock?
//!
//! Every other test here is about cost. But a flocking system that has been
//! optimised into a uniform gas would pass all of them — faster, even, since a
//! dispersed flock has fewer neighbours to consider. The perf tests would call
//! that an improvement.
//!
//! This measures the emergent structure instead, with the **index of
//! dispersion**: the variance of per-cell occupancy divided by its mean. A
//! uniform random scatter sits at ~1.0 by definition (cell counts are Poisson);
//! a clumped one sits far above it. It is a coarse statistic, deliberately —
//! the point is to catch a system that stopped clumping at all, not to police
//! the exact character of the clumping, which is a matter of taste and tuning.
//!
//! Written after a change to the grid's visit order (centre cell first, so a
//! capped boid steers off its *nearest* neighbours rather than whichever lay up
//! and to the left) made the flock look different in a screenshot — a dense
//! blob before, an even spread after. Measured, the structure is largely
//! intact: 21.4 against the previous 27.3 at 16,000 boids, and 4.7 against 4.5
//! at 2,000, where the neighbour cap does not bind at all. So roughly 20% less
//! clumping at high density, not the collapse to a uniform gas it looked like.
//! One frame of a flock that swirls is not evidence; this is.

use glam::Vec2;
use jengine_core::ecs::components::Transform;
use jengine_swarm::{Boid, SwarmSpawner, update_swarm};

/// Half-extents of the area `SwarmSpawner::default` keeps the flock inside.
const HALF_W: f32 = 620.0;
const HALF_H: f32 = 340.0;

/// Variance-to-mean ratio of per-cell occupancy over the play area.
///
/// 1.0 is a uniform random scatter. Above it means clumped; below, more evenly
/// spread than chance (a crystal, which flocking does not produce).
fn index_of_dispersion(world: &hecs::World, cell: f32) -> f64 {
    let points: Vec<Vec2> = world
        .query::<(&Transform, &Boid)>()
        .iter()
        .map(|(_, (t, _))| Vec2::new(t.position.x, t.position.y))
        .collect();
    assert!(!points.is_empty(), "no boids to measure");

    let cols = ((2.0 * HALF_W) / cell).ceil() as i32;
    let rows = ((2.0 * HALF_H) / cell).ceil() as i32;
    let mut counts = vec![0u32; (cols * rows) as usize];
    for p in &points {
        let gx = (((p.x + HALF_W) / cell) as i32).clamp(0, cols - 1);
        let gy = (((p.y + HALF_H) / cell) as i32).clamp(0, rows - 1);
        counts[(gy * cols + gx) as usize] += 1;
    }

    let n_cells = counts.len() as f64;
    let mean = points.len() as f64 / n_cells;
    let var = counts
        .iter()
        .map(|&c| {
            let d = c as f64 - mean;
            d * d
        })
        .sum::<f64>()
        / n_cells;
    var / mean.max(1e-9)
}

/// Settle a flock of `n` for a few seconds of simulated time.
fn settled(n: usize, dt: f32, seconds: f32) -> hecs::World {
    let d = SwarmSpawner::default();
    let mut world = hecs::World::new();
    world.spawn((Transform::default(), SwarmSpawner { count: n, ..d }));
    for _ in 0..(seconds / dt) as usize {
        update_swarm(&mut world, dt);
    }
    assert_eq!(world.query::<&Boid>().iter().count(), n);
    world
}

#[test]
fn the_flock_clumps_rather_than_spreading_evenly() {
    // Generous threshold. Measured 4.7 at 2k and 21.4 at 16k, against 1.0 for
    // a uniform scatter, so this fires only if the structure is essentially
    // gone — which is the failure worth catching.
    for n in [2_000usize, 16_000] {
        let world = settled(n, 1.0 / 60.0, 3.33);
        let d = index_of_dispersion(&world, 48.0);
        println!("{n:>6} boids: index of dispersion {d:.2}");
        assert!(
            d > 3.0,
            "{n} boids scattered almost uniformly (index of dispersion {d:.2}, \
             uniform is 1.0) - the flocking rules have stopped producing structure"
        );
    }
}

/// Emergent structure must not depend on the frame rate.
///
/// Worth stating because it is an easy thing to get wrong — steering is applied
/// per step, so a system that is not properly `dt`-scaled clumps differently at
/// 30 fps than at 120, and the difference shows up as "the editor looks unlike
/// the native runtime" rather than as anything obviously a bug.
#[test]
fn structure_does_not_depend_on_the_step_size() {
    let mut seen = Vec::new();
    for fps in [30.0f32, 60.0, 120.0] {
        // The same simulated duration in each case, so only the step differs.
        let world = settled(8_000, 1.0 / fps, 3.33);
        let d = index_of_dispersion(&world, 48.0);
        println!("8000 boids at {fps:>5} fps: index of dispersion {d:.2}");
        seen.push(d);
    }
    let (lo, hi) = seen.iter().fold((f64::MAX, 0.0f64), |(l, h), &d| (l.min(d), h.max(d)));
    assert!(
        hi / lo < 1.6,
        "clumping varied {:.2}x across step sizes ({seen:?}) - the steering is \
         not properly dt-scaled",
        hi / lo
    );
}
