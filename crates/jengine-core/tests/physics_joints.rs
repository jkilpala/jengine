//! Joints, motors and raycasts against a real rapier world.
//!
//! * A second joint on the same entity overwrote the first in the joint map,
//!   so the first kept simulating with nothing able to remove it.
//! * Motors passed the authored max torque as rapier's *damping factor*, so
//!   the torque cap was never set: a motor authored with zero torque still
//!   drove its joint with unlimited force.
//! * `raycast` measured distance in multiples of the direction's length.

#![cfg(feature = "physics")]

use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, Joint2D, JointType, RigidBody2D};
use jengine_core::physics::PhysicsWorld;

const DT: f32 = 1.0 / 60.0;

fn world() -> PhysicsWorld {
    // No gravity: only the motor moves anything.
    PhysicsWorld::new(0.0, 0.0, 100.0)
}

fn body(w: &mut PhysicsWorld, id: u64, kind: BodyType, x: f32, y: f32) {
    let rb = RigidBody2D { body_type: kind, ..Default::default() };
    w.add_body(id, &rb, x, y, 0.0);
    w.add_collider(
        id,
        &Collider2D { shape: ColliderShape::Box { width: 10.0, height: 10.0 }, ..Default::default() },
    );
}

fn revolute(speed: f32, torque: f32) -> Joint2D {
    Joint2D {
        target_entity: String::new(),
        joint_type: JointType::Revolute {
            anchor: [0.0, 0.0],
            anchor_b: [0.0, 0.0],
            limits: None,
            motor_speed: speed,
            motor_max_torque: torque,
        },
        collide_connected: false,
        ..Default::default()
    }
}

fn run(w: &mut PhysicsWorld, seconds: f32) {
    for _ in 0..(seconds / DT) as usize {
        w.step(DT);
    }
}

#[test]
fn every_joint_on_an_entity_is_tracked_and_removed() {
    let mut w = world();
    body(&mut w, 1, BodyType::Static, 0.0, 0.0);
    body(&mut w, 2, BodyType::Dynamic, 50.0, 0.0);
    body(&mut w, 3, BodyType::Dynamic, -50.0, 0.0);
    w.add_joint(1, 2, &revolute(0.0, 0.0));
    w.add_joint(1, 3, &revolute(0.0, 0.0));
    assert_eq!(w.joint_count(1), 2, "the second joint replaced the first");

    w.remove_joint(1);
    assert_eq!(w.joint_count(1), 0);
    assert!(!w.has_joint(1));
}

#[test]
fn an_authored_motor_spins_at_its_speed() {
    let mut w = world();
    body(&mut w, 1, BodyType::Static, 0.0, 0.0);
    body(&mut w, 2, BodyType::Dynamic, 0.0, 0.0);
    w.add_joint(1, 2, &revolute(3.0, 1.0e6));
    run(&mut w, 1.0);
    let omega = w.get_angular_velocity(2).unwrap();
    assert!((omega.abs() - 3.0).abs() < 0.1, "expected |omega| = 3, got {omega}");
}

#[test]
fn a_motor_with_zero_torque_cannot_turn_anything() {
    // With the torque passed as a damping factor this was a 0-damping motor
    // with *unlimited* force. A zero cap must mean no drive at all.
    let mut w = world();
    body(&mut w, 1, BodyType::Static, 0.0, 0.0);
    body(&mut w, 2, BodyType::Dynamic, 0.0, 0.0);
    w.add_joint(1, 2, &revolute(3.0, 0.0));
    run(&mut w, 1.0);
    let omega = w.get_angular_velocity(2).unwrap();
    assert!(omega.abs() < 1e-3, "a zero-torque motor turned the body: {omega}");
}

#[test]
fn set_motor_speed_drives_a_joint_that_had_no_motor() {
    let mut w = world();
    body(&mut w, 1, BodyType::Static, 0.0, 0.0);
    body(&mut w, 2, BodyType::Dynamic, 0.0, 0.0);
    w.add_joint(1, 2, &revolute(0.0, 0.0));
    w.set_motor_speed(1, -2.0);
    run(&mut w, 1.0);
    let omega = w.get_angular_velocity(2).unwrap();
    assert!((omega.abs() - 2.0).abs() < 0.1, "expected |omega| = 2, got {omega}");
}

#[test]
fn set_motor_speed_leaves_a_fixed_joint_alone() {
    let mut w = world();
    body(&mut w, 1, BodyType::Static, 0.0, 0.0);
    body(&mut w, 2, BodyType::Dynamic, 20.0, 0.0);
    let fixed = Joint2D {
        target_entity: String::new(),
        joint_type: JointType::Fixed { anchor_a: [20.0, 0.0], anchor_b: [0.0, 0.0] },
        collide_connected: false,
        ..Default::default()
    };
    w.add_joint(1, 2, &fixed);
    w.set_motor_speed(1, 5.0);
    run(&mut w, 0.5);
    let omega = w.get_angular_velocity(2).unwrap();
    assert!(omega.abs() < 1e-3, "a fixed joint was motorised: {omega}");
}

#[test]
fn raycast_distance_is_in_world_units_whatever_the_direction_length() {
    let mut w = world();
    body(&mut w, 1, BodyType::Static, 100.0, 0.0);
    w.step(DT); // build the query pipeline

    // Box edge at x = 95.
    for len in [1.0f32, 10.0, 0.25] {
        let hit = w.raycast(0.0, 0.0, len, 0.0, 200.0).unwrap_or_else(|| panic!("no hit at len {len}"));
        assert!((hit.distance - 95.0).abs() < 1e-3, "len {len}: distance {}", hit.distance);
        assert!((hit.point_x - 95.0).abs() < 1e-3);
    }
    // `max_dist` is world units too: 90 falls short of the box.
    assert!(w.raycast(0.0, 0.0, 10.0, 0.0, 90.0).is_none(), "max_dist was scaled by the direction");
    // A zero direction is no ray.
    assert!(w.raycast(0.0, 0.0, 0.0, 0.0, 200.0).is_none());
}
