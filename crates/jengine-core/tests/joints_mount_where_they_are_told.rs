//! A joint has two ends, and until now only one of them could be aimed.
//!
//! `JointType::Fixed`, `Rope` and `Spring` all carry `anchor_a` **and**
//! `anchor_b`. `Revolute` and `Prismatic` carried a single `anchor`,
//! used as `local_anchor1`, with `local_anchor2` hardcoded to the second
//! body's origin - so a revolute joint could only ever pin something
//! *through the middle of* whatever it was attached to.
//!
//! For a pendulum on a peg that is fine, which is why it survived: the
//! only revolute joint in the repo before this was a test pendulum and a
//! debug-draw fixture, and both hang from the origin.
//!
//! It stops being fine the moment two things are jointed to one body at
//! **different places** - which is to say, the moment anything has
//! wheels. A vehicle wants its rear wheel at the back of the chassis and
//! its front wheel at the front; with one anchor there is no way to say
//! so. Putting the offset in `anchor` instead moves the *wheel's* local
//! attachment point, which then orbits as the wheel turns - a wheel
//! whose axle goes round in circles.
//!
//! You cannot work around it from the other side either. Anchoring from
//! the chassis means a `Joint2D` per wheel on the chassis entity, and an
//! entity holds one component of a type.

#![cfg(feature = "physics")]

use jengine_core::ecs::components::{
    BodyType, Collider2D, ColliderShape, Joint2D, JointType, RigidBody2D,
};
use jengine_core::physics::PhysicsWorld;

const LENGTH_UNIT: f32 = 100.0;

/// A chassis at the origin with a wheel jointed to it at `mount`, after
/// `steps` of settling. Returns where the wheel ended up.
fn wheel_after(mount: [f32; 2], steps: usize) -> (f32, f32) {
    let mut w = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);
    // The chassis is static so nothing moves but the wheel - this is a
    // question about where the joint puts things, not about dynamics.
    w.add_body(1, &RigidBody2D { body_type: BodyType::Static, ..Default::default() }, 0.0, 0.0, 0.0);
    w.add_collider(1, &Collider2D { shape: ColliderShape::Box { width: 64.0, height: 24.0 }, ..Default::default() });

    // The wheel starts well away from where it belongs, so settling *to*
    // the mount is the thing being measured.
    w.add_body(2, &RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, ..Default::default() }, 0.0, 0.0, 0.0);
    w.add_collider(2, &Collider2D { shape: ColliderShape::Circle { radius: 14.0 }, density: 1.0, ..Default::default() });

    w.add_joint(
        2,
        1,
        &Joint2D {
            target_entity: "Chassis".into(),
            joint_type: JointType::Revolute {
                anchor: [0.0, 0.0],
                anchor_b: mount,
                limits: None,
                motor_speed: 0.0,
                motor_max_torque: 0.0,
            },
            collide_connected: false,
            ..Default::default()
        },
    );
    for _ in 0..steps {
        w.step(1.0 / 60.0);
    }
    w.get_position(2).unwrap_or((f32::NAN, f32::NAN))
}

#[test]
fn a_wheel_hangs_where_the_chassis_says_and_not_at_its_middle() {
    // The whole bug in one assertion: mounted at the back of the
    // chassis, the wheel is at the back of the chassis.
    let (x, y) = wheel_after([-24.0, -14.0], 240);
    assert!(
        (x - -24.0).abs() < 2.0 && (y - -14.0).abs() < 2.0,
        "a wheel mounted at (-24, -14) settled at ({x:.1}, {y:.1}) - before `anchor_b` \
         existed every joint pinned through the other body's origin, so it settled at (0, 0)"
    );
}

#[test]
fn two_wheels_on_one_chassis_land_in_different_places() {
    // The reason one anchor is not enough. This is not a harder version
    // of the test above; it is the case that cannot be expressed at all
    // with a single anchor, because both wheels would pin to the same
    // point and a vehicle would be a unicycle with a spare.
    let rear = wheel_after([-24.0, -14.0], 240);
    let front = wheel_after([24.0, -14.0], 240);
    assert!(
        front.0 - rear.0 > 40.0,
        "the wheels should be a wheelbase apart: rear at {rear:?}, front at {front:?}"
    );
}

#[test]
fn an_unset_anchor_b_still_means_the_other_bodys_origin() {
    // Backwards compatibility, stated as a test: `anchor_b` defaults to
    // zero and every joint authored before it existed behaves exactly as
    // it did. The pendulum in `physics_joints` is the one that would
    // notice.
    let (x, y) = wheel_after([0.0, 0.0], 240);
    assert!(
        x.hypot(y) < 2.0,
        "with no anchor_b the wheel should sit on the chassis origin, not at ({x:.1}, {y:.1})"
    );
}

#[test]
fn the_anchor_on_the_jointed_body_still_works_too() {
    // `anchor` is unchanged: it is the point *on the wheel* that meets
    // the mount. Offsetting it holds the wheel off its own centre, which
    // is what a crank is.
    let mut w = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);
    w.add_body(1, &RigidBody2D { body_type: BodyType::Static, ..Default::default() }, 0.0, 0.0, 0.0);
    w.add_collider(1, &Collider2D { shape: ColliderShape::Box { width: 64.0, height: 24.0 }, ..Default::default() });
    w.add_body(2, &RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, ..Default::default() }, 0.0, 0.0, 0.0);
    w.add_collider(2, &Collider2D { shape: ColliderShape::Circle { radius: 14.0 }, density: 1.0, ..Default::default() });
    w.add_joint(
        2,
        1,
        &Joint2D {
            target_entity: "Chassis".into(),
            joint_type: JointType::Revolute {
                anchor: [10.0, 0.0],
                anchor_b: [30.0, 0.0],
                limits: None,
                motor_speed: 0.0,
                motor_max_torque: 0.0,
            },
            collide_connected: false,
            ..Default::default()
        },
    );
    for _ in 0..240 {
        w.step(1.0 / 60.0);
    }
    let (x, y) = w.get_position(2).unwrap();
    // The wheel's own (10, 0) sits on the chassis's (30, 0), so the
    // wheel's centre is 10 units back from the mount - whichever way it
    // has turned to get there.
    let d = (x - 30.0).hypot(y);
    assert!(
        (d - 10.0).abs() < 2.0,
        "the wheel centre should be 10 from the mount, it is {d:.1} (at {x:.1}, {y:.1})"
    );
}
