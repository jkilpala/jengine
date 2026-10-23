//! Editing a `RigidBody2D` on a live body changes the body.
//!
//! `ensure_physics_bodies` built a body the first time it saw the component
//! and never looked at the component again, so the inspector switching a
//! crate from static to dynamic - or a script doing the same through the
//! component - changed the data and nothing in the simulation. The sync now
//! compares the body type the simulation holds with the component's and
//! applies the edit.

#![cfg(feature = "physics")]

use hecs::World;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D, Transform};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;

#[test]
fn switching_a_static_body_to_dynamic_makes_it_fall() {
    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 16.0, height: 16.0 }, ..Default::default() },
    ));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    ensure_physics_bodies(&world, &mut physics);
    for _ in 0..30 {
        physics.step(1.0 / 60.0);
    }
    physics.sync_transforms_to_ecs(&mut world);
    assert_eq!(world.get::<&Transform>(e).unwrap().position.y, 0.0, "a static body stays put");
    assert_eq!(physics.body_type_of(e.id() as u64), Some(BodyType::Static));

    world.get::<&mut RigidBody2D>(e).unwrap().body_type = BodyType::Dynamic;
    ensure_physics_bodies(&world, &mut physics);
    assert_eq!(physics.body_type_of(e.id() as u64), Some(BodyType::Dynamic), "the edit reached the body");
    for _ in 0..30 {
        physics.step(1.0 / 60.0);
    }
    physics.sync_transforms_to_ecs(&mut world);
    assert!(world.get::<&Transform>(e).unwrap().position.y < -1.0, "and it falls now");
}
