//! **Four more joints** (Oct 2026; Unity's `WheelJoint2D`,
//! `TargetJoint2D`, `RelativeJoint2D` and `FrictionJoint2D`). A wheel on a
//! suspension: revolute wheels pin the axle rigidly, so a car felt every
//! bump in its frame. A target joint - dragging a body with the mouse - had
//! to be faked by setting velocities toward the cursor, which ignores mass
//! and walks through walls. A motor joint holds a thing at an offset with
//! limited strength. Friction against another body is how a top-down game,
//! which has no floor to rub on, slows a puck. All four are rapier generic
//! joints with motors on their axes; the springs are a frequency and a
//! damping ratio, so they behave the same whatever they carry. Found by the
//! showcase's Crane Yard.

#![cfg(feature = "physics")]

use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, Joint2D, JointType, RigidBody2D};
use jengine_core::physics::PhysicsWorld;

const STEP: f32 = 1.0 / 60.0;

fn world(gravity: f32) -> PhysicsWorld {
    PhysicsWorld::new(0.0, gravity, 100.0)
}

fn body(w: &mut PhysicsWorld, id: u64, ty: BodyType, x: f32, y: f32, shape: ColliderShape) {
    w.add_body(id, &RigidBody2D { body_type: ty, mass: 1.0, ..Default::default() }, x, y, 0.0);
    w.add_collider(id, &Collider2D { shape, density: 1.0, ..Default::default() });
}

fn joint(joint_type: JointType) -> Joint2D {
    Joint2D { target_entity: "Other".into(), joint_type, ..Default::default() }
}

fn run(w: &mut PhysicsWorld, seconds: f32) {
    for _ in 0..(seconds / STEP).round() as usize {
        w.step(STEP);
    }
}

fn wheel(frequency: f32) -> JointType {
    serde_json::from_value(serde_json::json!({ "Wheel": { "anchor_b": [0.0, -30.0], "frequency": frequency } })).unwrap()
}

/// A wheel hung from a static chassis under gravity: where it settles.
fn hung_wheel(frequency: f32) -> (f32, f32) {
    let mut w = world(-9.81);
    body(&mut w, 1, BodyType::Static, 0.0, 0.0, ColliderShape::Box { width: 80.0, height: 20.0 });
    body(&mut w, 2, BodyType::Dynamic, 0.0, -30.0, ColliderShape::Circle { radius: 12.0 });
    w.add_joint(2, 1, &joint(wheel(frequency)));
    run(&mut w, 4.0);
    w.get_position(2).unwrap()
}

#[test]
fn a_wheel_rides_a_spring_along_its_axis() {
    // Gravity is 981 a second squared (length unit 100). A spring of f Hz
    // has stiffness (2 pi f)^2, so the wheel sags 981 / that below its mount.
    let (x, y) = hung_wheel(3.0);
    let sag = 981.0 / (std::f32::consts::TAU * 3.0).powi(2);
    assert!(x.abs() < 0.05, "the axle cannot leave its axis: x {x}");
    assert!((y - (-30.0 - sag)).abs() < 0.4, "3 Hz sags {sag:.2}: settled at {y:.2}");
    let (_, soft) = hung_wheel(1.0);
    assert!(soft < y - 15.0, "a softer spring sags further: 1 Hz at {soft:.1}, 3 Hz at {y:.1}");
}

#[test]
fn a_wheel_turns_free_and_its_motor_drives_it() {
    let mut w = world(0.0);
    body(&mut w, 1, BodyType::Static, 0.0, 0.0, ColliderShape::Box { width: 80.0, height: 20.0 });
    body(&mut w, 2, BodyType::Dynamic, 0.0, -30.0, ColliderShape::Circle { radius: 12.0 });
    w.add_joint(2, 1, &joint(wheel(3.0)));
    w.set_angular_velocity(2, 4.0);
    run(&mut w, 0.5);
    let spin = w.get_angular_velocity(2).unwrap();
    assert!((spin - 4.0).abs() < 0.1, "a free wheel keeps spinning: {spin}");
    // `set_motor_speed` reaches the wheel's rotation, not its suspension -
    // even though the joint was built without a motor.
    let mut driven: JointType = wheel(3.0);
    if let JointType::Wheel { motor_max_torque, .. } = &mut driven {
        *motor_max_torque = 1.0e7;
    }
    w.remove_joint(2);
    w.add_joint(2, 1, &joint(driven));
    // Positive is clockwise, as on a revolute wheel: rolling right.
    w.set_motor_speed(2, 6.0);
    run(&mut w, 0.5);
    let spin = w.get_angular_velocity(2).unwrap();
    let (x, y) = w.get_position(2).unwrap();
    assert!((spin + 6.0).abs() < 0.2, "a motor speed of 6 turns it clockwise (-6 rad/s): {spin}");
    assert!(x.abs() < 0.05 && (y + 30.0).abs() < 0.5, "and leaves the suspension alone: ({x}, {y})");
}

#[test]
fn a_target_joint_pulls_to_its_point_then_to_what_it_follows() {
    let mut w = world(0.0);
    body(&mut w, 1, BodyType::Dynamic, 0.0, 0.0, ColliderShape::Box { width: 20.0, height: 20.0 });
    let target: JointType = serde_json::from_value(serde_json::json!({ "Target": { "target": [100.0, 50.0] } })).unwrap();
    w.add_target_joint(1, &joint(target.clone()), None, [100.0, 50.0]);
    for _ in 0..180 {
        w.sync_joint_targets(|_| None);
        w.step(STEP);
    }
    let (x, y) = w.get_position(1).unwrap();
    assert!((x - 100.0).abs() < 1.0 && (y - 50.0).abs() < 1.0, "pulled to its point: ({x}, {y})");
    // Named another way: entity 9, which has no body, is the mouse.
    w.remove_joint(1);
    w.add_joint(1, 9, &joint(target));
    for _ in 0..180 {
        w.sync_joint_targets(|id| (id == 9).then_some([-80.0, 20.0]));
        w.step(STEP);
    }
    let (x, y) = w.get_position(1).unwrap();
    assert!((x + 80.0).abs() < 1.0 && (y - 20.0).abs() < 1.0, "pulled to what it follows: ({x}, {y})");
    // Let go: the body coasts, no longer pulled anywhere.
    w.remove_joint(1);
    w.set_velocity(1, 60.0, 0.0);
    run(&mut w, 1.0);
    let (x, _) = w.get_position(1).unwrap();
    assert!(x > -30.0, "released, it coasts on: x {x}");
}

#[test]
fn a_weak_target_joint_lags_a_strong_one() {
    let at = |max_force: f32| {
        let mut w = world(0.0);
        body(&mut w, 1, BodyType::Dynamic, 0.0, 0.0, ColliderShape::Box { width: 20.0, height: 20.0 });
        let target: JointType =
            serde_json::from_value(serde_json::json!({ "Target": { "target": [200.0, 0.0], "max_force": max_force } })).unwrap();
        w.add_target_joint(1, &joint(target), None, [200.0, 0.0]);
        for _ in 0..20 {
            w.sync_joint_targets(|_| None);
            w.step(STEP);
        }
        w.get_position(1).unwrap().0
    };
    let (weak, strong) = (at(50.0), at(0.0));
    assert!(weak < strong * 0.5, "a weak pull ({weak:.1}) gets there far slower than an unlimited one ({strong:.1})");
}

#[test]
fn a_motor_joint_holds_its_offset() {
    let mut w = world(0.0);
    body(&mut w, 1, BodyType::Static, 0.0, 0.0, ColliderShape::Box { width: 20.0, height: 20.0 });
    body(&mut w, 2, BodyType::Dynamic, 0.0, 0.0, ColliderShape::Box { width: 10.0, height: 10.0 });
    let motor: JointType =
        serde_json::from_value(serde_json::json!({ "Motor": { "linear_offset": [60.0, 10.0], "angular_offset": 0.5 } })).unwrap();
    w.add_joint(2, 1, &joint(motor));
    run(&mut w, 3.0);
    let (x, y) = w.get_position(2).unwrap();
    let a = w.get_rotation(2).unwrap();
    assert!((x - 60.0).abs() < 1.0 && (y - 10.0).abs() < 1.0, "held at its offset: ({x}, {y})");
    assert!((a - 0.5).abs() < 0.02, "and its angle: {a}");
}

#[test]
fn friction_slows_a_slide_by_its_force() {
    let slide = |friction: Option<f32>| {
        let mut w = world(0.0);
        body(&mut w, 1, BodyType::Static, 0.0, -200.0, ColliderShape::Box { width: 20.0, height: 20.0 });
        body(&mut w, 2, BodyType::Dynamic, 0.0, 0.0, ColliderShape::Box { width: 10.0, height: 10.0 });
        if let Some(f) = friction {
            let j: JointType = serde_json::from_value(serde_json::json!({ "Friction": { "max_force": f, "max_torque": f } })).unwrap();
            w.add_joint(2, 1, &joint(j));
        }
        w.set_velocity(2, 300.0, 0.0);
        run(&mut w, 0.5);
        w.get_velocity(2).unwrap().0
    };
    assert!((slide(None) - 300.0).abs() < 1.0, "nothing slows it alone");
    // Mass 1, force 200: it loses 200 a second, 100 in half a second.
    let v = slide(Some(200.0));
    assert!((v - 200.0).abs() < 8.0, "200 of friction on a mass of 1 takes 100 off in half a second: {v}");
    assert!(slide(Some(5000.0)).abs() < 1.0, "enough friction stops it");
}

#[test]
fn the_new_joints_read_from_a_scene_with_their_defaults() {
    let j: Joint2D = serde_json::from_value(serde_json::json!({
        "target_entity": "Chassis", "joint_type": { "Wheel": {} }
    }))
    .unwrap();
    let JointType::Wheel { axis, frequency, damping_ratio, limits, .. } = j.joint_type else { panic!() };
    assert_eq!((axis, frequency, damping_ratio, limits), ([0.0, 1.0], 3.0, 0.7, None));
    for v in [
        serde_json::json!({ "Motor": {} }),
        serde_json::json!({ "Friction": {} }),
        serde_json::json!({ "Target": {} }),
    ] {
        serde_json::from_value::<JointType>(v.clone()).unwrap_or_else(|e| panic!("{v}: {e}"));
    }
}
