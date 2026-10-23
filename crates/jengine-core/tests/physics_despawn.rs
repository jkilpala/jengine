//! A despawned entity's physics body must leave the simulation with it.
//!
//! Nothing that despawns an entity - script commands, `Lifetime`, hitboxes,
//! bounds, behavior graphs - tells physics, and for a long time nothing removed
//! the body at all: `PhysicsWorld::remove_body` had no caller anywhere. Bodies
//! are keyed by `entity.id()`, the index alone, and hecs hands a freed index to
//! the next spawn. So a despawned crate left an invisible collider behind, and
//! the next entity to reuse its index - physics or not - was moved by it.
//!
//! `ensure_physics_bodies` now reconciles against the world every frame, which
//! all three runtimes call right before stepping. These tests drive exactly that
//! frame: ensure, sync kinematics, step, sync back.

#![cfg(feature = "physics")]

use hecs::World;
use jengine_core::ecs::components::{
    BodyType, Collider2D, ColliderShape, Joint2D, Name, RigidBody2D, Transform,
};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;

const DT: f32 = 1.0 / 60.0;

fn physics() -> PhysicsWorld {
    PhysicsWorld::new(0.0, -9.81, 100.0)
}

fn at(x: f32, y: f32) -> Transform {
    let mut t = Transform::default();
    t.position.x = x;
    t.position.y = y;
    t
}

fn dynamic() -> RigidBody2D {
    RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() }
}

fn fixed() -> RigidBody2D {
    RigidBody2D { body_type: BodyType::Static, ..Default::default() }
}

fn square(size: f32) -> Collider2D {
    Collider2D {
        shape: ColliderShape::Box { width: size, height: size },
        ..Default::default()
    }
}

/// One runtime frame, in the order every host runs it.
fn frame(world: &mut World, physics: &mut PhysicsWorld) {
    ensure_physics_bodies(world, physics);
    physics.sync_kinematic_from_ecs(world);
    physics.step(DT);
    physics.sync_transforms_to_ecs(world);
}

fn y_of(world: &World, e: hecs::Entity) -> f32 {
    world.get::<&Transform>(e).unwrap().position.y
}

#[test]
fn an_entity_reusing_a_despawned_index_is_not_moved_by_the_old_body() {
    let mut world = World::new();
    let mut physics = physics();

    let crate_e = world.spawn((at(0.0, 0.0), dynamic(), square(10.0)));
    for _ in 0..30 {
        frame(&mut world, &mut physics);
    }
    assert!(y_of(&world, crate_e) < -10.0, "precondition: the crate falls");
    world.despawn(crate_e).unwrap();

    // A plain, non-physics entity. hecs gives it the crate's index.
    let label = world.spawn((at(0.0, 5000.0),));
    assert_eq!(label.id(), crate_e.id(), "precondition: the index is reused");

    frame(&mut world, &mut physics);
    assert_eq!(
        y_of(&world, label),
        5000.0,
        "an entity with no RigidBody2D was driven by a despawned entity's body"
    );
}

#[test]
fn a_physics_entity_reusing_the_index_gets_its_own_body() {
    let mut world = World::new();
    let mut physics = physics();

    let old = world.spawn((at(0.0, 0.0), dynamic(), square(10.0)));
    for _ in 0..30 {
        frame(&mut world, &mut physics);
    }
    world.despawn(old).unwrap();

    // Spawned far to the side and high up. Gravity only pulls in y, so if it
    // inherited the old body it would jump to x = 0.
    let new = world.spawn((at(700.0, 400.0), dynamic(), square(10.0)));
    assert_eq!(new.id(), old.id(), "precondition: the index is reused");

    frame(&mut world, &mut physics);
    let t = world.get::<&Transform>(new).unwrap();
    assert!((t.position.x - 700.0).abs() < 1e-3, "took the old body's x: {}", t.position.x);
    assert!(t.position.y > 390.0, "took the old body's y: {}", t.position.y);
}

#[test]
fn a_despawned_floor_stops_holding_things_up() {
    let mut world = World::new();
    let mut physics = physics();

    let floor = world.spawn((at(0.0, -100.0), fixed(), square(400.0)));
    let ball = world.spawn((at(0.0, 150.0), dynamic(), square(20.0)));
    for _ in 0..180 {
        frame(&mut world, &mut physics);
    }
    let resting = y_of(&world, ball);
    assert!(resting > 90.0, "precondition: the ball rests on the floor, y = {resting}");

    world.despawn(floor).unwrap();
    for _ in 0..60 {
        frame(&mut world, &mut physics);
    }
    assert!(
        y_of(&world, ball) < resting - 50.0,
        "the ball is still held up by the collider of a despawned floor"
    );
}

#[test]
fn removing_the_rigid_body_component_removes_the_body() {
    let mut world = World::new();
    let mut physics = physics();

    let e = world.spawn((at(0.0, 0.0), dynamic(), square(10.0)));
    frame(&mut world, &mut physics);
    assert!(physics.has_body(e.id() as u64));

    world.remove_one::<RigidBody2D>(e).unwrap();
    frame(&mut world, &mut physics);
    assert!(!physics.has_body(e.id() as u64));

    // And the entity is left where it is rather than falling.
    let y = y_of(&world, e);
    frame(&mut world, &mut physics);
    assert_eq!(y_of(&world, e), y);
}

#[test]
fn a_joint_is_rebuilt_when_its_target_is_respawned() {
    let mut world = World::new();
    let mut physics = physics();

    let anchor = world.spawn((
        at(0.0, 0.0),
        fixed(),
        square(10.0),
        Joint2D { target_entity: "Bob".into(), ..Default::default() },
    ));
    let bob = world.spawn((at(0.0, -50.0), dynamic(), square(10.0), Name("Bob".into())));
    frame(&mut world, &mut physics);
    assert!(physics.has_joint(anchor.id() as u64), "precondition: the joint exists");

    // Despawning the target removes the joint with its body.
    world.despawn(bob).unwrap();
    frame(&mut world, &mut physics);
    assert!(
        !physics.has_joint(anchor.id() as u64),
        "the anchor still claims a joint whose target body is gone"
    );

    // A new "Bob" must get attached again.
    world.spawn((at(0.0, -50.0), dynamic(), square(10.0), Name("Bob".into())));
    frame(&mut world, &mut physics);
    assert!(physics.has_joint(anchor.id() as u64), "the joint was not rebuilt");
}

#[test]
fn a_body_added_twice_under_one_id_does_not_leave_the_first_behind() {
    // `add_body` used to overwrite the maps and orphan the earlier body, which
    // kept simulating and colliding with nothing able to reach it.
    let mut physics = physics();
    let rb = fixed();
    physics.add_body(1, &rb, 0.0, 0.0, 0.0);
    physics.add_collider(1, &square(100.0));
    physics.add_body(1, &rb, 1000.0, 0.0, 0.0);
    physics.add_collider(1, &square(100.0));
    physics.step(DT);

    // A ray through where the first body was must hit nothing.
    let hit = physics.raycast(-300.0, 0.0, 1.0, 0.0, 600.0);
    assert!(hit.is_none(), "the first body is still in the simulation: {hit:?}");
    assert!(physics.raycast(700.0, 0.0, 1.0, 0.0, 600.0).is_some(), "the second body is missing");
}
