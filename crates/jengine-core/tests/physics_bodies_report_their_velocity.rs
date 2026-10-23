//! **A physics body's `Velocity2D` is its velocity** (Oct 2026), and the
//! motion step leaves a dynamic body to the physics.
//!
//! `Velocity2D` on a rigid body used to be a seed: read once when the body was
//! built, never written again (BUGS #132), so a package reading it saw the
//! speed the body was spawned with, forever. Scripts were spared - they ask
//! rapier - but a package has no physics world to ask.
//!
//! Writing it back exposed a second bug. The motion step (`velocity_system`)
//! integrates **every** `Velocity2D`, physics bodies included, and the physics
//! frame reads a dynamic body's transform moving under it as a script's
//! `set_position` and teleports the body there. So a body authored with a
//! velocity was carried by it twice - once by rapier, once by the motion step,
//! for as long as it lived - and a velocity written back every frame would
//! have done the same to every moving body. A dynamic body is the physics'
//! now; a kinematic one is still moved by its `Velocity2D`, which is how a
//! moving platform is authored.
//!
//! And a write goes the other way, as a moved transform does: a package that
//! sets a dynamic body's `Velocity2D` sets the body's velocity. A physics
//! launcher's mid-flight tricks do exactly that, and before this only "worked" through
//! the double carry above.

#![cfg(all(feature = "native", feature = "physics"))]

use hecs::World;
use jengine_core::ecs::behaviors::tick_motion;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D, Transform, Velocity2D};
use jengine_core::ecs::systems::{physics_frame_begin, physics_frame_end};
use jengine_core::particles::ParticleSystem;
use jengine_core::physics::PhysicsWorld;

const DT: f32 = 1.0 / 60.0;

/// A frame as every host runs one: motion, then the physics frame.
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

fn body(world: &mut World, kind: BodyType, vx: f32) -> hecs::Entity {
    world.spawn((
        Transform::default(),
        Velocity2D { x: vx, y: 0.0, gravity: 0.0 },
        RigidBody2D { body_type: kind, gravity_scale: 0.0, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 5.0 }, ..Default::default() },
    ))
}

#[test]
fn a_dynamic_body_travels_at_its_own_speed_once() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let mut particles = ParticleSystem::new();
    let e = body(&mut world, BodyType::Dynamic, 120.0);
    for _ in 0..60 {
        frame(&mut world, &mut physics, &mut particles);
    }
    let x = world.get::<&Transform>(e).unwrap().position.x;
    // One second at 120 a second. Carried twice, it was ~240.
    assert!((x - 120.0).abs() < 6.0, "a second at 120/s moves it about 120, not twice that: {x}");
}

#[test]
fn a_dynamic_body_reports_the_velocity_rapier_gives_it() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let mut particles = ParticleSystem::new();
    let e = body(&mut world, BodyType::Dynamic, 0.0);
    frame(&mut world, &mut physics, &mut particles);
    physics.set_velocity(e.id() as u64, 30.0, -50.0);
    for _ in 0..3 {
        frame(&mut world, &mut physics, &mut particles);
    }
    let v = world.get::<&Velocity2D>(e).unwrap().clone();
    assert!((v.x - 30.0).abs() < 0.5 && (v.y + 50.0).abs() < 0.5, "the component reads the body's speed: {v:?}");
}

#[test]
fn a_kinematic_body_is_still_moved_by_its_velocity() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let mut particles = ParticleSystem::new();
    let e = body(&mut world, BodyType::Kinematic, 60.0);
    for _ in 0..60 {
        frame(&mut world, &mut physics, &mut particles);
    }
    let x = world.get::<&Transform>(e).unwrap().position.x;
    assert!((x - 60.0).abs() < 3.0, "a moving platform authored by its velocity: {x}");
    assert_eq!(world.get::<&Velocity2D>(e).unwrap().x, 60.0, "and keeps the velocity it was given");
}

#[test]
fn a_package_that_writes_the_velocity_steers_the_body() {
    let mut world = World::new();
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    let mut particles = ParticleSystem::new();
    let e = body(&mut world, BodyType::Dynamic, 100.0);
    for _ in 0..10 {
        frame(&mut world, &mut physics, &mut particles);
    }
    // A package doubles it, as a launcher game's boost does.
    {
        let mut v = world.get::<&mut Velocity2D>(e).unwrap();
        v.x *= 2.0;
    }
    let before = world.get::<&Transform>(e).unwrap().position.x;
    for _ in 0..60 {
        frame(&mut world, &mut physics, &mut particles);
    }
    let after = world.get::<&Transform>(e).unwrap().position.x;
    let v = world.get::<&Velocity2D>(e).unwrap().x;
    assert!((v - 200.0).abs() < 1.0, "the body took the written speed: {v}");
    assert!(((after - before) - 200.0).abs() < 8.0, "and covers it in a second: {}", after - before);
}
