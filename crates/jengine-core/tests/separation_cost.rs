//! `apply_separation` at crowd scale.
//!
//! It compared every separating body to every other — `O(n²)`, documented as
//! such and correct for the ten enemies it was written for. A game with a few
//! hundred is not unusual, and that is 100x the work.
//!
//! `#[ignore]`d: timing. Run with
//! `cargo test -p jengine-core --release --test separation_cost -- --ignored --nocapture`

use std::time::Instant;

use hecs::World;
use jengine_core::ecs::components::{Separation, Transform};
use jengine_core::ecs::systems::apply_separation;

fn crowd(n: usize, spread: f32) -> World {
    let mut world = World::new();
    for i in 0..n {
        // A deliberately dense scatter: overlapping is the case that costs.
        let a = i as f32 * 2.399_963;
        let r = ((i % 97) as f32 / 97.0) * spread;
        let mut t = Transform::default();
        t.position.x = a.cos() * r;
        t.position.y = a.sin() * r;
        world.spawn((t, Separation { radius: 16.0 }));
    }
    world
}

#[test]
#[ignore = "timing-sensitive; run with --release"]
fn report_separation_cost_by_crowd_size() {
    println!();
    println!("{:>8}  {:>12}  {:>14}", "bodies", "ms/frame", "us/body");
    println!("{:->8}  {:->12}  {:->14}", "", "", "");
    for n in [100usize, 500, 2_000, 8_000] {
        let mut world = crowd(n, 600.0);
        // Warm: the first pass moves things, later ones are the steady state.
        apply_separation(&mut world);
        let t0 = Instant::now();
        let iters = 20;
        for _ in 0..iters {
            apply_separation(&mut world);
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0 / iters as f64;
        println!("{n:>8}  {ms:>12.3}  {:>14.3}", ms * 1000.0 / n as f64);
    }
}

// ── the broadphase must not change the result ──────────────────────────────

/// The original all-pairs implementation, kept as the reference.
///
/// A faster separation that pushes bodies differently is not a fix, it is a
/// regression nobody would notice until a crowd behaved oddly.
fn brute_force_push(bodies: &[(Vec2, f32)]) -> Vec<Vec2> {
    let n = bodies.len();
    let mut push = vec![Vec2::ZERO; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let d = bodies[i].0 - bodies[j].0;
            let min_d = bodies[i].1 + bodies[j].1;
            let len = d.length();
            if len > 1e-4 && len < min_d {
                let shove = d / len * ((min_d - len) * 0.25);
                push[i] += shove;
                push[j] -= shove;
            } else if len <= 1e-4 {
                let a = i as f32 * 2.399_963;
                let nudge = Vec2::new(a.cos(), a.sin()) * (min_d * 0.25);
                push[i] += nudge;
                push[j] -= nudge;
            }
        }
    }
    push
}

use glam::Vec2;

fn positions_of(world: &World) -> Vec<Vec2> {
    let mut v: Vec<(u32, Vec2)> = world
        .query::<(&Transform, &Separation)>()
        .iter()
        .map(|(e, (t, _))| (e.id(), t.position))
        .collect();
    v.sort_by_key(|(id, _)| *id);
    v.into_iter().map(|(_, p)| p).collect()
}

#[test]
fn the_grid_gives_the_same_push_as_all_pairs() {
    for n in [2usize, 17, 200, 900] {
        let mut world = crowd(n, 300.0);

        let before: Vec<(Vec2, f32)> = {
            let mut v: Vec<(u32, Vec2, f32)> = world
                .query::<(&Transform, &Separation)>()
                .iter()
                .map(|(e, (t, s))| (e.id(), t.position, s.radius))
                .collect();
            v.sort_by_key(|(id, _, _)| *id);
            v.into_iter().map(|(_, p, r)| (p, r)).collect()
        };
        let expected: Vec<Vec2> = before
            .iter()
            .zip(brute_force_push(&before))
            .map(|((p, _), push)| *p + push)
            .collect();

        apply_separation(&mut world);
        let got = positions_of(&world);

        assert_eq!(got.len(), expected.len());
        for (i, (g, e)) in got.iter().zip(expected.iter()).enumerate() {
            assert!(
                (*g - *e).length() < 1e-3,
                "n={n}: body {i} moved to {g:?}, all-pairs says {e:?}"
            );
        }
    }
}

#[test]
fn coincident_bodies_still_push_apart_rather_than_together() {
    // The subtle half of the rewrite. The all-pairs version seeded the escape
    // direction from the lower index of each pair; seeding it from the current
    // body instead would send both the same way and they would stay stacked
    // forever.
    let mut world = World::new();
    let a = world.spawn((Transform::default(), Separation { radius: 10.0 }));
    let b = world.spawn((Transform::default(), Separation { radius: 10.0 }));

    apply_separation(&mut world);

    let pa = world.get::<&Transform>(a).unwrap().position;
    let pb = world.get::<&Transform>(b).unwrap().position;
    assert!(
        (pa - pb).length() > 1e-3,
        "two bodies at the same spot did not separate: {pa:?} vs {pb:?}"
    );
}
