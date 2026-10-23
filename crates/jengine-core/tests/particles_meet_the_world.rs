//! **Particles meet the world** (Sep 2026): an emitter with `collision`
//! hits the scene's colliders - dies on them or bounces off - and a
//! `sub_emitters` entry bursts another emitter where that happens, which is
//! rain that splashes on a roof. And the tick every host runs is one
//! function, `particles::tick_particles`, which places each emitter where it
//! is drawn: the hosts used its *local* position, so an emitter on a child of
//! a moving thing emitted from somewhere else. Native also kept a second
//! particle system that `Particles.emit_burst` went into and nothing drew.
//!
//! Found by the showcase's Storm.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{Collider2D, Name, Parent, Transform};
use jengine_core::particles::{
    tick_particles, CollisionResponse, EmitShape, ParticleCollision, ParticleEmitter, ParticleSystem, SubEmitter,
    SubEmitterEvent,
};

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");
const SYSTEMS: &str = include_str!("../src/ecs/systems.rs");

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

fn roof(world: &mut World, y: f32) {
    let col: Collider2D =
        serde_json::from_value(serde_json::json!({ "shape": { "Box": { "width": 400.0, "height": 20.0 } } })).unwrap();
    world.spawn((at(0.0, y), col));
}

/// Rain straight down from `y`, fast, many at once.
fn rain(response: CollisionResponse) -> ParticleEmitter {
    ParticleEmitter {
        emission_rate: 0.0,
        burst_count: 50,
        max_particles: 50,
        lifetime: [5.0, 5.0],
        speed: [600.0, 600.0],
        angle: [-std::f32::consts::FRAC_PI_2, -std::f32::consts::FRAC_PI_2],
        emit_shape: EmitShape::Line { length: 200.0 },
        collision: Some(ParticleCollision { response, bounce: 0.5, friction: 0.0, ..Default::default() }),
        ..Default::default()
    }
}

fn run(world: &World, system: &mut ParticleSystem, frames: usize) {
    for _ in 0..frames {
        tick_particles(world, system, 1.0 / 60.0);
    }
}

fn lowest(system: &ParticleSystem, world: &World) -> f32 {
    positions(system, world).iter().map(|p| p.y).fold(f32::MAX, f32::min)
}

fn positions(system: &ParticleSystem, world: &World) -> Vec<Vec2> {
    let mut q = world.query::<(&Transform, &ParticleEmitter)>();
    let emitters: Vec<(u64, &ParticleEmitter, Vec2)> =
        q.iter().map(|(e, (t, em))| (e.id() as u64, em, t.position)).collect();
    let layers = jengine_core::asset::sort_layers::SortLayers::default();
    system.draw_data(&emitters, &layers).iter().flat_map(|g| g.particles.iter().map(|p| p.position)).collect()
}

#[test]
fn rain_dies_on_the_roof_and_nothing_passes_it() {
    let mut world = World::new();
    roof(&mut world, 0.0);
    world.spawn((at(0.0, 200.0), rain(CollisionResponse::Kill)));
    let mut system = ParticleSystem::new();
    run(&world, &mut system, 1);
    assert_eq!(system.alive_count(), 50);
    run(&world, &mut system, 60);
    assert_eq!(system.alive_count(), 0, "every drop hit the roof and died there");

    // Without `collision` the same rain falls straight through.
    let mut world = World::new();
    roof(&mut world, 0.0);
    world.spawn((at(0.0, 200.0), ParticleEmitter { collision: None, ..rain(CollisionResponse::Kill) }));
    let mut system = ParticleSystem::new();
    run(&world, &mut system, 30);
    assert!(lowest(&system, &world) < -100.0, "no collision, no roof");
}

#[test]
fn a_bouncing_particle_comes_back_up_and_settles_on_top() {
    let mut world = World::new();
    roof(&mut world, 0.0);
    let em = ParticleEmitter { gravity_modifier: 3.0, ..rain(CollisionResponse::Bounce) };
    world.spawn((at(0.0, 200.0), em));
    let mut system = ParticleSystem::new();
    let mut lowest_seen = f32::MAX;
    for _ in 0..240 {
        run(&world, &mut system, 1);
        lowest_seen = lowest_seen.min(lowest(&system, &world));
    }
    assert_eq!(system.alive_count(), 50, "bouncing kills nothing");
    assert!(lowest_seen >= 9.9, "none went below the roof's top (y 10): {lowest_seen}");
    let resting = positions(&system, &world);
    assert!(resting.iter().all(|p| (p.y - 10.0).abs() < 2.0), "all came to rest on the roof: {resting:?}");
}

#[test]
fn a_sub_emitter_splashes_where_the_rain_lands() {
    let mut world = World::new();
    roof(&mut world, 0.0);
    let mut drops = rain(CollisionResponse::Kill);
    drops.sub_emitters =
        vec![SubEmitter { on: SubEmitterEvent::Collision, emitter: "Splash".into(), count: 3, chance: 1.0 }];
    world.spawn((at(0.0, 200.0), drops));
    // The splash: emits nothing itself; slow, short-lived, upward.
    let splash = ParticleEmitter {
        emission_rate: 0.0,
        max_particles: 500,
        lifetime: [10.0, 10.0],
        speed: [0.0, 0.0],
        world_space: false,
        ..Default::default()
    };
    // Far from the roof, and local-space: its particles still appear at the
    // splash point, not about the emitter.
    world.spawn((at(900.0, 900.0), splash, Name("Splash".into())));
    let mut system = ParticleSystem::new();
    run(&world, &mut system, 60);
    assert_eq!(system.alive_count(), 150, "three splash particles per drop");
    let pos = positions(&system, &world);
    assert!(pos.iter().all(|p| (p.y - 10.0).abs() < 1.0 && p.x.abs() <= 100.5), "splashes on the roof: {pos:?}");

    // `chance` thins them out; a name nothing has bursts nothing.
    let mut world = World::new();
    roof(&mut world, 0.0);
    let mut drops = rain(CollisionResponse::Kill);
    drops.sub_emitters = vec![
        SubEmitter { on: SubEmitterEvent::Death, emitter: "Splash".into(), count: 2, chance: 0.5 },
        SubEmitter { on: SubEmitterEvent::Death, emitter: "Nobody".into(), count: 2, chance: 1.0 },
    ];
    world.spawn((at(0.0, 200.0), drops));
    world.spawn((at(0.0, 0.0), ParticleEmitter { emission_rate: 0.0, max_particles: 500, lifetime: [10.0, 10.0], ..Default::default() }, Name("Splash".into())));
    let mut system = ParticleSystem::new();
    run(&world, &mut system, 60);
    let n = system.alive_count();
    assert!(n > 20 && n < 80 && n % 2 == 0, "about half of fifty drops splashed two each: {n}");
}

#[test]
fn a_child_emitter_emits_where_it_is_drawn() {
    let mut world = World::new();
    let parent = world.spawn((at(500.0, 0.0),));
    let em = ParticleEmitter {
        emission_rate: 0.0,
        burst_count: 5,
        max_particles: 5,
        speed: [0.0, 0.0],
        lifetime: [5.0, 5.0],
        ..Default::default()
    };
    world.spawn((at(10.0, 0.0), em, Parent(parent)));
    jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);
    let mut system = ParticleSystem::new();
    run(&world, &mut system, 1);
    let mut q = world.query::<(&Transform, &ParticleEmitter)>();
    let e: Vec<_> = q.iter().map(|(e, (_, em))| (e.id() as u64, em, Vec2::new(510.0, 0.0))).collect();
    let layers = jengine_core::asset::sort_layers::SortLayers::default();
    let pos: Vec<Vec2> =
        system.draw_data(&e, &layers).iter().flat_map(|g| g.particles.iter().map(|p| p.position)).collect();
    assert_eq!(pos.len(), 5);
    assert!(pos.iter().all(|p| (p.x - 510.0).abs() < 1e-3), "at the parent's 500 plus its own 10: {pos:?}");
}

/// Every host ticks through `tick_particles`, and there is one system.
#[test]
fn every_runtime_ticks_particles_once_through_the_one_function() {
    for (name, src) in [("native", NATIVE), ("viewport", VIEWPORT), ("web", WEB)] {
        assert!(src.contains("particles::tick_particles("), "{name} does not call `tick_particles`");
        assert!(
            !src.contains("particle_system.update("),
            "{name} ticks particles itself - with its own idea of where an emitter is"
        );
    }
    assert!(
        !NATIVE.contains("particle_system: ParticleSystem"),
        "native keeps a second particle system: the script system's is the one bursts land in"
    );
    assert!(
        !SYSTEMS.contains("particle_system.update("),
        "the script system ticks particles too - every emitter simulated twice"
    );
    assert!(SYSTEMS.contains("snapshot.particle_count ="), "`Particles.get_count` reads a count nothing fills");
}
