//! GPU particles meet the world (Oct 2026): the stateless cloud is drawn
//! toward `attractors` and spun round them as it ages, and with `collide` a
//! particle found inside one of the nearest colliders is put out on its
//! nearest side. `particles::tick_particles` gathers what each cloud meets
//! (`GpuParticles::world`) and the uniform carries it in the emitter's
//! space; `GpuParticles::sample` is the vertex shader on the CPU.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{Collider2D, ColliderShape, Name, Transform};
use jengine_core::particles::gpu::{GpuAttractor, GpuCollider, GpuParticles, GpuWorld};
use jengine_core::particles::{tick_particles, ParticleSystem};

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::ONE }
}

fn stream() -> GpuParticles {
    GpuParticles {
        count: 2000,
        lifetime: [4.0, 4.0],
        speed: [200.0, 200.0],
        angle: [-0.3, 0.3],
        radius: 40.0,
        ..Default::default()
    }
}

#[test]
fn no_particle_is_left_inside_a_rock() {
    let mut g = GpuParticles { collide: true, ..stream() };
    g.world = GpuWorld {
        attractors: vec![],
        colliders: vec![
            GpuCollider { centre: Vec2::new(300.0, 0.0), half: Vec2::splat(60.0), rotation: 0.0, circle: true, velocity: Vec2::ZERO },
            GpuCollider { centre: Vec2::new(500.0, 10.0), half: Vec2::new(40.0, 90.0), rotation: 0.6, circle: false, velocity: Vec2::ZERO },
        ],
    };
    let free = stream();
    let mut inside_before = 0;
    for i in 0..2000 {
        for t in [1.0, 1.7, 2.6, 3.3] {
            let (p, _, _) = g.sample(i, t);
            assert!(p.distance(Vec2::new(300.0, 0.0)) >= 59.9, "{i} at {p} is inside the rock");
            let q = p - Vec2::new(500.0, 10.0);
            let (s, c) = (-0.6f32).sin_cos();
            let l = Vec2::new(q.x * c - q.y * s, q.x * s + q.y * c);
            assert!(!(l.x.abs() < 39.9 && l.y.abs() < 89.9), "{i} at {p} is inside the slab");
            if free.sample(i, t).0.distance(Vec2::new(300.0, 0.0)) < 60.0 {
                inside_before += 1;
            }
        }
    }
    assert!(inside_before > 100, "the stream really runs through the rock without it: {inside_before}");
}

#[test]
fn an_attractor_draws_the_cloud_in_and_turns_it() {
    let free = stream();
    let mut pulled = stream();
    pulled.world.attractors = vec![([400.0, 0.0, 3.0, 0.0], 0.0)];
    let mut spun = stream();
    spun.world.attractors = vec![([400.0, 0.0, 0.0, 2.0], 0.0)];
    let to = Vec2::new(400.0, 0.0);
    let mean = |g: &GpuParticles| (0..800).map(|i| g.sample(i, 3.0).0.distance(to)).sum::<f32>() / 800.0;
    assert!(mean(&pulled) < mean(&free) * 0.5, "{} against {}", mean(&pulled), mean(&free));
    // A swirl alone keeps each particle's distance and turns it round.
    let (a, b) = (free.sample(7, 2.0).0, spun.sample(7, 2.0).0);
    assert!((a.distance(to) - b.distance(to)).abs() < 0.01, "the same distance");
    assert!(a.distance(b) > 10.0, "turned round: {a} {b}");
}

#[test]
fn a_reaching_radius_spares_the_far_side() {
    let mut g = stream();
    g.world.attractors = vec![([2000.0, 0.0, 5.0, 0.0], 100.0)];
    let free = stream();
    for i in 0..200 {
        assert!(g.sample(i, 1.5).0.distance(free.sample(i, 1.5).0) < 0.01, "nothing reaches 1.6 km");
    }
}

#[test]
fn the_tick_finds_what_each_cloud_meets_and_the_uniform_brings_it_home() {
    let mut w = World::new();
    w.spawn((at(300.0, 50.0), Name("Twister".into())));
    w.spawn((at(150.0, 0.0), Collider2D { shape: ColliderShape::Circle { radius: 30.0 }, ..Default::default() }));
    w.spawn((at(900.0, 0.0), Collider2D { shape: ColliderShape::Box { width: 40.0, height: 20.0 }, ..Default::default() }));
    w.spawn((at(120.0, 0.0), Collider2D { shape: ColliderShape::Circle { radius: 9.0 }, is_sensor: true, ..Default::default() }));
    let storm = w.spawn((
        at(100.0, 0.0),
        GpuParticles {
            collide: true,
            attractors: vec![GpuAttractor { follow: "Twister".into(), strength: 1.0, swirl: 2.0, radius: 50.0, ..Default::default() }],
            ..stream()
        },
    ));
    let mut sys = ParticleSystem::new();
    tick_particles(&w, &mut sys, 0.016);
    let g = w.get::<&GpuParticles>(storm).unwrap();
    assert_eq!(g.world.attractors, vec![([300.0, 50.0, 1.0, 2.0], 50.0)], "at the twister");
    assert_eq!(g.world.colliders.len(), 2, "the sensor is not met");
    assert!(g.world.colliders[0].circle && g.world.colliders[0].centre == Vec2::new(150.0, 0.0), "nearest first");
    // In the emitter's space: the emitter at (100, 0), turned a quarter and
    // doubled, sees the circle 50 ahead of it as 25 to its right.
    let u = g.uniform(Vec2::new(100.0, 0.0), std::f32::consts::FRAC_PI_2, Vec2::splat(2.0), [[0.0; 4]; 4]);
    assert_eq!(u.counts[1], 2.0);
    assert!((u.col[0][0] - 0.0).abs() < 1e-4 && (u.col[0][1] + 25.0).abs() < 1e-4, "{:?}", u.col[0]);
    assert!((u.col[0][2] - 15.0).abs() < 1e-4, "its radius halved too");
    assert!((u.att2[0][0] - 25.0).abs() < 1e-4, "and the attractor's reach");
}

#[test]
fn a_pile_is_a_soft_heap_not_a_line() {
    let rock = GpuCollider { centre: Vec2::new(300.0, 0.0), half: Vec2::splat(60.0), rotation: 0.0, circle: true, velocity: Vec2::ZERO };
    let free = stream();
    let mut g = GpuParticles { collide: true, pile: 20.0, ..stream() };
    g.world.colliders = vec![rock];
    let mut dists = Vec::new();
    for i in 0..2000 {
        if free.sample(i, 1.7).0.distance(rock.centre) < 60.0 {
            dists.push(g.sample(i, 1.7).0.distance(rock.centre));
        }
    }
    assert!(dists.len() > 30);
    assert!(dists.iter().all(|d| (59.9..=80.1).contains(d)), "within the heap: {dists:?}");
    let lo = dists.iter().cloned().fold(f32::MAX, f32::min);
    let hi = dists.iter().cloned().fold(f32::MIN, f32::max);
    assert!(lo < 64.0 && hi > 76.0, "spread through it: {lo}..{hi}");
}
