//! Swarm performance, measured without a window.
//!
//! Frame rate in the editor or the runtime mixes simulation, rendering and the
//! rest of the tick. This times *only* the flocking, so a change to the
//! algorithm can be judged on its own.
//!
//! Both tests are `#[ignore]`d: one is a benchmark, and the scaling check
//! measures wall-clock time, which is exactly the kind of assertion that goes
//! flaky on a loaded CI box. Run them deliberately, in **release** — a debug
//! build is roughly ten times slower and says nothing useful:
//!
//! ```text
//! cargo test -p jengine-swarm --release --test perf -- --ignored --nocapture
//! ```

use std::time::Instant;

use jengine_core::ecs::components::Transform;
use jengine_swarm::{Boid, SwarmSpawner, update_swarm};

/// Build a world with `n` boids already spawned and settled, in the fixed
/// on-screen area a scene actually uses.
fn world_with(n: usize) -> hecs::World {
    world_with_area(n, 1.0)
}

/// As [`world_with`], with the area scaled by `area_scale` on each axis.
///
/// Scaling the area matters for measuring the *grid*: with a fixed area,
/// doubling the boids doubles the density, so per-boid cost rises however good
/// the spatial structure is. Holding density constant (area proportional to
/// `n`) is the only way to see whether the grid itself is linear.
fn world_with_area(n: usize, area_scale: f32) -> hecs::World {
    let d = SwarmSpawner::default();
    let mut world = hecs::World::new();
    world.spawn((
        Transform::default(),
        SwarmSpawner {
            count: n,
            half_width: d.half_width * area_scale,
            half_height: d.half_height * area_scale,
            ..Default::default()
        },
    ));
    // First tick spawns; a few more let the flock leave its scattered start,
    // so the timing reflects a real, clustered flock rather than a uniform
    // one — clustering is the case the grid has to handle well.
    for _ in 0..30 {
        update_swarm(&mut world, 1.0 / 60.0);
    }
    assert_eq!(world.query::<&Boid>().iter().count(), n);
    world
}

/// Median milliseconds per `update_swarm` call over `frames`.
fn median_ms(world: &mut hecs::World, frames: usize) -> f64 {
    let mut samples = Vec::with_capacity(frames);
    for _ in 0..frames {
        let t0 = Instant::now();
        update_swarm(world, 1.0 / 60.0);
        samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    // Median, not mean: one GC-like hitch or a scheduler preemption should not
    // define the number.
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}

#[test]
#[ignore = "benchmark; run with --release --nocapture"]
fn report_swarm_cost_by_population() {
    println!();
    println!("{:>8}  {:>10}  {:>12}", "boids", "ms/frame", "us/boid");
    println!("{:->8}  {:->10}  {:->12}", "", "", "");
    for n in [500usize, 1_000, 2_000, 4_000, 8_000, 16_000] {
        let mut world = world_with(n);
        let ms = median_ms(&mut world, 60);
        println!("{n:>8}  {ms:>10.3}  {:>12.3}", ms * 1000.0 / n as f64);
    }
    println!();
    println!("A 60 Hz frame is 16.67 ms in total, so the simulation wants to");
    println!("stay well under that - rendering and the rest of the tick still");
    println!("have to fit alongside it.");
}

#[test]
#[ignore = "timing-sensitive; run with --release"]
fn cost_grows_roughly_linearly_at_constant_density() {
    // The point of the spatial grid: at constant density, 8x the boids should
    // cost ~8x, not the ~64x brute force would.
    //
    // The area is scaled with sqrt(n) so density is held constant. Comparing
    // at a *fixed* area measures density, not the grid — the first version of
    // this test did exactly that, reported 37x, and blamed the grid. The area
    // does not grow with the flock, so at a fixed size doubling the boids
    // doubles the work per boid no matter how good the spatial structure is.
    let mut small = world_with_area(1_000, 1.0);
    let mut large = world_with_area(8_000, (8.0f32).sqrt());

    let small_ms = median_ms(&mut small, 40);
    let large_ms = median_ms(&mut large, 40);
    let ratio = large_ms / small_ms.max(1e-6);

    println!("1k: {small_ms:.3} ms, 8k: {large_ms:.3} ms, ratio {ratio:.1}x for 8x the boids");
    // Loose on purpose: this catches a regression to quadratic, it does not
    // police constant factors on a machine that might be busy.
    assert!(
        ratio < 20.0,
        "8x the boids at the same density cost {ratio:.1}x the time - that \
         looks quadratic, so the spatial grid is not doing its job"
    );
}

#[test]
#[ignore = "timing-sensitive; run with --release"]
fn the_neighbor_cap_bounds_cost_in_a_dense_flock() {
    // A scene has a fixed screen, so density really does grow with the flock,
    // and the neighbour cap is what keeps that affordable. Measured on this
    // package before the cap existed: 0.31 us/boid at 1k rising to 2.59 at
    // 16k — an 8x rise purely from density. With the cap it is ~2x.
    let mut small = world_with(1_000);
    let mut large = world_with(16_000);

    let small_per_boid = median_ms(&mut small, 40) / 1_000.0;
    let large_per_boid = median_ms(&mut large, 40) / 16_000.0;
    let growth = large_per_boid / small_per_boid.max(1e-9);

    println!(
        "fixed area: {:.4} us/boid at 1k, {:.4} us/boid at 16k ({growth:.1}x)",
        small_per_boid * 1000.0,
        large_per_boid * 1000.0
    );
    assert!(
        growth < 4.0,
        "per-boid cost grew {growth:.1}x with density - the neighbour cap is \
         not bounding the work"
    );
}
