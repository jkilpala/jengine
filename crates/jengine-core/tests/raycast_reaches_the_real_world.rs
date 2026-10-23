//! `raycast`, the last script function that was missing on native.
//!
//! Everything else in the docs' list of absentees turned out to be stale -
//! wired all along, and the list was wrong. This one really was not there,
//! and it is instructive about *why*: it is the only script read that
//! cannot be answered from a snapshot.
//!
//! Every other read is a lookup. A position, a velocity, a contact list, a
//! tile: the host can copy or borrow it once a frame and the script asks
//! for one of a fixed set of things. A ray is not a lookup. Its origin and
//! direction come from the script, so there is nothing to precompute, and
//! the two obvious ways out are both worse than borrowing: copying the
//! collider set every frame is not cheap, and reimplementing ray-vs-shape
//! would drift away from the rapier everything else collides with.
//!
//! What made it look harder than it is: `ScriptState` has carried a
//! `physics_world_ptr` field and a working `raycast` method for a long
//! time, and **nothing ever called the method that sets the pointer**. The
//! route existed, was dead, and read as having been tried and abandoned.
//! The live native path is `BridgeReadSnapshot`, and that is where the
//! pointer needed to be.
//!
//! These tests go through the snapshot, which is the object both the Luau
//! adapter and the browser adapters read. What they cannot check is the
//! `globals.set("raycast", ...)` line itself - that is
//! `script_global_parity`'s job, and it is the ratchet that was listing
//! this as a known gap.

#![cfg(feature = "physics")]

use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D};
use jengine_core::physics::PhysicsWorld;
use jengine_core::scripting::bridge::BridgeReadSnapshot;

const LENGTH_UNIT: f32 = 100.0;

/// A world with a 40x400 wall at `x`, owned by entity `id`.
fn wall_at(w: &mut PhysicsWorld, id: u64, x: f32) {
    w.add_body(id, &RigidBody2D { body_type: BodyType::Static, ..Default::default() }, x, 0.0, 0.0);
    w.add_collider(
        id,
        &Collider2D {
            shape: ColliderShape::Box { width: 40.0, height: 400.0 },
            ..Default::default()
        },
    );
}

/// A snapshot with the physics world lent to it, the way the runtime lends
/// it for the duration of one script run.
fn snap_of(physics: &PhysicsWorld) -> BridgeReadSnapshot {
    BridgeReadSnapshot { physics_ptr: Some(physics as *const _), ..Default::default() }
}

#[test]
fn a_ray_finds_the_wall_in_front_of_it() {
    let mut w = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);
    wall_at(&mut w, 7, 300.0);
    w.step(1.0 / 60.0);

    let hit = snap_of(&w).raycast(0.0, 0.0, 1.0, 0.0, 1000.0).expect("the wall is right there");
    assert_eq!(hit.entity_id, 7, "it should report which entity was hit");
    // The wall's near face is at 300 - 20.
    assert!((hit.distance - 280.0).abs() < 2.0, "distance {}", hit.distance);
    assert!((hit.point_x - 280.0).abs() < 2.0, "the hit point should be on the face");
    assert!(hit.normal_x < -0.9, "the normal should point back along the ray: {}", hit.normal_x);
}

#[test]
fn a_ray_with_nothing_in_front_of_it_finds_nothing() {
    let mut w = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);
    wall_at(&mut w, 7, 300.0);
    w.step(1.0 / 60.0);
    assert!(snap_of(&w).raycast(0.0, 0.0, -1.0, 0.0, 1000.0).is_none(), "it fired away from it");
    assert!(snap_of(&w).raycast(0.0, 900.0, 1.0, 0.0, 1000.0).is_none(), "it fired over it");
}

#[test]
fn a_ray_stops_at_max_dist() {
    // The whole point of the function in a stealth game: a guard can only
    // see so far, and the check is the same one that decides whether the
    // wall is in the way.
    let mut w = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);
    wall_at(&mut w, 7, 300.0);
    w.step(1.0 / 60.0);
    let s = snap_of(&w);
    assert!(s.raycast(0.0, 0.0, 1.0, 0.0, 200.0).is_none(), "200 units should not reach 280");
    assert!(s.raycast(0.0, 0.0, 1.0, 0.0, 400.0).is_some(), "400 units should");
}

#[test]
fn the_direction_does_not_have_to_be_normalised() {
    // And this is not a nicety: rapier measures time of impact in
    // multiples of the direction's length, so an unnormalised direction
    // used to make `max_dist` mean something else entirely and report a
    // hit at 280 units as a distance of 28.
    let mut w = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);
    wall_at(&mut w, 7, 300.0);
    w.step(1.0 / 60.0);
    let s = snap_of(&w);
    let unit = s.raycast(0.0, 0.0, 1.0, 0.0, 1000.0).unwrap();
    let long = s.raycast(0.0, 0.0, 10.0, 0.0, 1000.0).unwrap();
    assert!(
        (unit.distance - long.distance).abs() < 0.5,
        "a direction ten times as long should report the same distance: {} vs {}",
        unit.distance,
        long.distance
    );
}

#[test]
fn the_nearest_wall_is_the_one_reported() {
    // A vision check that returned the far wall would see through the near
    // one, which is the bug this function exists to prevent.
    let mut w = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);
    wall_at(&mut w, 1, 600.0);
    wall_at(&mut w, 2, 300.0);
    w.step(1.0 / 60.0);
    let hit = snap_of(&w).raycast(0.0, 0.0, 1.0, 0.0, 2000.0).unwrap();
    assert_eq!(hit.entity_id, 2, "the near wall is at 300 and the far one at 600");
}

#[test]
fn a_ray_sees_where_bodies_are_now_rather_than_where_they_were() {
    // The reason this borrows the live world instead of a snapshot. A
    // guard's line of sight has to be blocked by the door that just
    // closed, not by where the door was at the start of the frame.
    let mut w = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);
    // A dynamic wall, drifting across the ray.
    w.add_body(
        3,
        &RigidBody2D { body_type: BodyType::Dynamic, gravity_scale: 0.0, ..Default::default() },
        900.0,
        0.0,
        0.0,
    );
    w.add_collider(
        3,
        &Collider2D {
            shape: ColliderShape::Box { width: 40.0, height: 400.0 },
            ..Default::default()
        },
    );
    w.set_velocity(3, -600.0, 0.0);
    let mut distances = Vec::new();
    for _ in 0..40 {
        w.step(1.0 / 60.0);
        if let Some(hit) = snap_of(&w).raycast(0.0, 0.0, 1.0, 0.0, 2000.0) {
            distances.push(hit.distance);
        }
    }
    assert!(distances.len() > 20, "it should have been in range most of the time");
    assert!(
        distances[0] - distances[distances.len() - 1] > 200.0,
        "the reported distance should follow the wall in: {:.0} then {:.0}",
        distances[0],
        distances[distances.len() - 1]
    );
}

#[test]
fn with_no_physics_world_lent_a_ray_finds_nothing_rather_than_panicking() {
    // Which is also what a miss looks like. A script cannot tell them
    // apart, and that is the contract the browser hosts have always
    // advertised - `raycast` there reads a hook no host installs and
    // returns null.
    let snap = BridgeReadSnapshot::default();
    assert!(snap.raycast(0.0, 0.0, 1.0, 0.0, 1000.0).is_none());
}

#[test]
fn a_zero_direction_is_not_a_ray() {
    let mut w = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);
    wall_at(&mut w, 7, 300.0);
    w.step(1.0 / 60.0);
    let s = snap_of(&w);
    assert!(s.raycast(0.0, 0.0, 0.0, 0.0, 1000.0).is_none());
    assert!(s.raycast(0.0, 0.0, f32::NAN, 0.0, 1000.0).is_none());
}
