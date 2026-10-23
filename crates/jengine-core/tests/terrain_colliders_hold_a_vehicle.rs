//! The collider shapes nothing has ever used.
//!
//! `ColliderShape` declares nine shapes. Four of them - `HeightField`,
//! `Polyline`, `ConvexPolygon` and `ConvexDecomposition` - are built by
//! the physics backend and are referenced by **no project and no
//! package**. Box and Circle carry every game in the arcade.
//!
//! This session has twice found that "wired but never exercised" is how
//! a bug hides here: the `CCDSolver` had run on an empty set since
//! physics was added (BUGS #143), and all eight physics script commands
//! were dropped on native (BUGS #144). A vehicle game wants a long
//! rolling ground, which is exactly what a heightfield or a polyline is
//! for - so before building one on top of them, find out whether they
//! hold anything up.

#![cfg(feature = "physics")]

use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D};
use jengine_core::physics::PhysicsWorld;

const LENGTH_UNIT: f32 = 100.0;
const DT: f32 = 1.0 / 60.0;

/// Drop a ball onto `ground` from `(x, y)` and report where it settles.
fn drop_onto(ground: ColliderShape, x: f32, y: f32, steps: usize) -> (f32, f32) {
    let mut w = PhysicsWorld::new(0.0, -9.81, LENGTH_UNIT);
    w.add_body(1, &RigidBody2D { body_type: BodyType::Static, ..Default::default() }, 0.0, 0.0, 0.0);
    w.add_collider(1, &Collider2D { shape: ground, friction: 0.9, ..Default::default() });

    let ball = RigidBody2D { body_type: BodyType::Dynamic, ccd: true, ..Default::default() };
    w.add_body(2, &ball, x, y, 0.0);
    w.add_collider(
        2,
        &Collider2D { shape: ColliderShape::Circle { radius: 8.0 }, friction: 0.9, ..Default::default() },
    );
    for _ in 0..steps {
        w.step(DT);
    }
    w.get_position(2).unwrap_or((f32::NAN, f32::NAN))
}

/// A flat heightfield 800 wide at height 0.
fn flat_field() -> ColliderShape {
    ColliderShape::HeightField { heights: vec![0.0; 33], scale: [800.0, 1.0] }
}

#[test]
fn a_heightfield_holds_a_body_up() {
    // The bar this has to clear: something dropped on it must stop, at
    // roughly the surface, rather than falling through for ever.
    let (_, y) = drop_onto(flat_field(), 0.0, 200.0, 400);
    assert!(y.is_finite(), "the ball left the world entirely: {y}");
    assert!(
        (y - 8.0).abs() < 6.0,
        "a ball of radius 8 should settle near y = 8 on a flat field at 0; it is at {y}"
    );
}

#[test]
fn a_heightfield_slope_sends_a_body_downhill() {
    // The thing a vehicle game actually needs: ground that is not level,
    // and a body that knows which way is down on it.
    let heights: Vec<f32> = (0..33).map(|i| 120.0 - i as f32 * 7.5).collect();
    let slope = ColliderShape::HeightField { heights, scale: [800.0, 1.0] };
    let (x_left, _) = drop_onto(slope.clone(), -200.0, 260.0, 60);
    let (x_late, _) = drop_onto(slope, -200.0, 260.0, 400);
    assert!(
        x_late > x_left + 20.0,
        "the ball should roll downhill: {x_left:.0} -> {x_late:.0}"
    );
}

#[test]
fn a_polyline_holds_a_body_up_too() {
    // The other way to spell a long rolling ground, and the one a game
    // reaches for when the surface is not a function of x.
    let verts: Vec<[f32; 2]> = (0..17).map(|i| [-400.0 + i as f32 * 50.0, 0.0]).collect();
    let (_, y) = drop_onto(ColliderShape::Polyline { vertices: verts }, 0.0, 200.0, 400);
    assert!(y.is_finite(), "the ball left the world entirely: {y}");
    assert!((y - 8.0).abs() < 6.0, "should settle near y = 8, is at {y}");
}

#[test]
fn a_convex_polygon_is_solid() {
    // A wedge, for the ramps a course is built from.
    let shape = ColliderShape::ConvexPolygon {
        vertices: vec![[-100.0, -20.0], [100.0, -20.0], [100.0, 20.0], [-100.0, 20.0]],
    };
    let (_, y) = drop_onto(shape, 0.0, 200.0, 400);
    assert!(y.is_finite());
    assert!(y > 20.0, "it should rest on top of a box that reaches y = 20, not at {y}");
}

#[test]
fn ccd_does_not_save_a_fast_body_from_a_heightfield() {
    // **A limitation, recorded rather than fixed.**
    //
    // `ccd: true` reliably stops a body against a box - `fast_bodies_do_
    // not_tunnel` holds that at twelve thousand units a second. Against
    // a heightfield it does not: a heightfield is a surface with no
    // thickness, and a body crossing it in one step goes through and
    // keeps going.
    //
    // Whether that is rapier's CCD or how this engine drives it was not
    // chased down; what matters for anything building on it is that
    // **thin ground is not safe ground at speed**, and the mitigation is
    // the next test.
    let mut w = PhysicsWorld::new(0.0, -9.81, LENGTH_UNIT);
    w.add_body(1, &RigidBody2D { body_type: BodyType::Static, ..Default::default() }, 0.0, 0.0, 0.0);
    w.add_collider(1, &Collider2D { shape: flat_field(), ..Default::default() });

    let fast = RigidBody2D { body_type: BodyType::Dynamic, ccd: true, gravity_scale: 0.0, ..Default::default() };
    w.add_body(2, &fast, 0.0, 120.0, 0.0);
    w.add_collider(2, &Collider2D { shape: ColliderShape::Circle { radius: 8.0 }, ..Default::default() });
    w.set_velocity(2, 0.0, -6000.0);
    for _ in 0..20 {
        w.step(DT);
    }
    let (_, y) = w.get_position(2).unwrap();
    assert!(
        y < -40.0,
        "this records that a heightfield does NOT stop a fast body (it reached {y}).          If this ever starts failing, CCD has learned to sweep thin ground and the          mitigation below can be dropped."
    );
}

#[test]
fn ground_with_thickness_does_stop_a_fast_body() {
    // The mitigation, and the reason a hill-driving package builds its terrain
    // from thick convex quads rather than one heightfield: give the
    // ground real depth and CCD works exactly as it does against any
    // other box.
    let mut w = PhysicsWorld::new(0.0, -9.81, LENGTH_UNIT);
    w.add_body(1, &RigidBody2D { body_type: BodyType::Static, ..Default::default() }, 0.0, 0.0, 0.0);
    // One segment of a course: a quad from the surface down to a floor.
    w.add_collider(
        1,
        &Collider2D {
            shape: ColliderShape::ConvexPolygon {
                vertices: vec![[-400.0, -300.0], [400.0, -300.0], [400.0, 0.0], [-400.0, 0.0]],
            },
            ..Default::default()
        },
    );
    let fast = RigidBody2D { body_type: BodyType::Dynamic, ccd: true, gravity_scale: 0.0, ..Default::default() };
    w.add_body(2, &fast, 0.0, 120.0, 0.0);
    w.add_collider(2, &Collider2D { shape: ColliderShape::Circle { radius: 8.0 }, ..Default::default() });
    w.set_velocity(2, 0.0, -6000.0);
    for _ in 0..20 {
        w.step(DT);
    }
    let (_, y) = w.get_position(2).unwrap();
    assert!(y > -40.0, "thick ground should have stopped it; it reached {y}");
}
