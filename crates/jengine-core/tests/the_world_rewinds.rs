//! **The world rewinds** (Oct 2026; `ecs::rewind`): a `rewind` clock and
//! `rewindable` entities that keep their history - a falling body put back
//! where it was, held there against gravity, and let go again from the past
//! with the speed it had then. Found by the showcase's Rewind Ruins. The
//! recording itself is tested in the module.

#![cfg(all(feature = "native", feature = "physics"))]

use hecs::World;
use jengine_core::ecs::behaviors::tick_motion;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D, Transform, Velocity2D};
use jengine_core::ecs::rewind::{Rewind, Rewindable, set_rewind_field};
use jengine_core::ecs::systems::{physics_frame_begin, physics_frame_end};
use jengine_core::particles::ParticleSystem;
use jengine_core::physics::PhysicsWorld;

const DT: f32 = 1.0 / 60.0;

/// A frame as every host runs one: motion (the rewind is in it), then the
/// physics frame.
fn frame(world: &mut World, physics: &mut PhysicsWorld, particles: &mut ParticleSystem) {
    tick_motion(world, DT, None, false);
    physics_frame_begin(world, physics, DT);
    let steps = physics.begin_frame(DT);
    for _ in 0..steps {
        physics.step_once();
    }
    let contacts = physics.end_frame();
    physics_frame_end(world, physics, &contacts, particles, DT);
}

fn y(world: &World, e: hecs::Entity) -> f32 {
    world.get::<&Transform>(e).unwrap().position.y
}

#[test]
fn a_falling_body_goes_back_up_holds_and_falls_again_from_the_past() {
    let mut world = World::new();
    // Gravity is given in metres and scaled by the length unit: 981 a second
    // squared.
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    let mut particles = ParticleSystem::new();
    let clock = world.spawn((Rewind::default(),));
    let ball = world.spawn((
        Transform::default(),
        Velocity2D::default(),
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 5.0 }, ..Default::default() },
        Rewindable::default(),
    ));
    for _ in 0..60 {
        frame(&mut world, &mut physics, &mut particles);
    }
    let fallen = y(&world, ball);
    assert!(fallen < -400.0, "a second of falling: {fallen}");

    // Back half a second: about a quarter of the way down.
    set_rewind_field(&world, clock, "mode", &serde_json::json!("rewind")).unwrap();
    for _ in 0..30 {
        frame(&mut world, &mut physics, &mut particles);
    }
    let back = y(&world, ball);
    assert!(back > -150.0 && back < -90.0, "half a second back is about a quarter of the fall: {back}");

    // Held, gravity does not take it.
    set_rewind_field(&world, clock, "mode", &serde_json::json!("hold")).unwrap();
    for _ in 0..30 {
        frame(&mut world, &mut physics, &mut particles);
    }
    let held = y(&world, ball);
    assert!((held - back).abs() < 2.0, "held where it was: {back} -> {held}");

    // Let go, it falls on at the speed it had then, not from rest.
    set_rewind_field(&world, clock, "mode", &serde_json::json!("record")).unwrap();
    for _ in 0..6 {
        frame(&mut world, &mut physics, &mut particles);
    }
    let after = y(&world, ball);
    // From rest, a tenth of a second drops ~5 units; at ~490 a second, ~54.
    assert!(held - after > 35.0, "it keeps the speed it had: fell {} in a tenth of a second", held - after);
}
