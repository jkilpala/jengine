//! A `RigidBody2D` changed after its body was built reaches the body.
//!
//! Its gravity scale, dampings, fixed rotation and CCD were read once, when
//! `ensure_physics_bodies` built the body, so a package writing
//! `angular_damping` to steady a vehicle in the air (a hill-driving
//! game's first idea) changed a number nothing read again. `sync_body_settings`, inside
//! `ensure_physics_bodies`, gives the body what the component says now.

#![cfg(feature = "physics")]

use hecs::World;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D, Transform};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;

fn spinning(world: &mut World, physics: &mut PhysicsWorld, damping: f32) -> f32 {
    let e = world.spawn((
        Transform::default(),
        RigidBody2D { body_type: BodyType::Dynamic, gravity_scale: 0.0, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 16.0, height: 16.0 }, ..Default::default() },
    ));
    ensure_physics_bodies(world, physics);
    physics.set_angular_velocity(e.id() as u64, 10.0);
    world.get::<&mut RigidBody2D>(e).unwrap().angular_damping = damping;
    for _ in 0..60 {
        ensure_physics_bodies(world, physics);
        physics.step(1.0 / 60.0);
        physics.sync_transforms_to_ecs(world);
    }
    physics.get_angular_velocity(e.id() as u64).expect("a body")
}

#[test]
fn a_damping_changed_after_the_body_was_built_slows_it() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    let free = spinning(&mut world, &mut physics, 0.0);
    let damped = spinning(&mut world, &mut physics, 5.0);
    assert!(free > 9.0, "an undamped body keeps spinning: {free}");
    assert!(damped < 1.0, "a damping written after the build slows the body: {damped}");
}

#[test]
fn a_gravity_scale_changed_after_the_body_was_built_takes() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    let e = world.spawn((
        Transform::default(),
        RigidBody2D { body_type: BodyType::Dynamic, gravity_scale: 0.0, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 16.0, height: 16.0 }, ..Default::default() },
    ));
    ensure_physics_bodies(&world, &mut physics);
    world.get::<&mut RigidBody2D>(e).unwrap().gravity_scale = 1.0;
    for _ in 0..30 {
        ensure_physics_bodies(&world, &mut physics);
        physics.step(1.0 / 60.0);
        physics.sync_transforms_to_ecs(&mut world);
    }
    assert!(world.get::<&Transform>(e).unwrap().position.y < -10.0, "it falls once gravity is turned on");
}
