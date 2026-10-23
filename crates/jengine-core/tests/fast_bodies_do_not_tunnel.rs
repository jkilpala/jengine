//! A body moving faster than it is thick must not pass through a wall.
//!
//! Rapier's solver moves a body by integrating its velocity and *then*
//! looking for overlaps. A ball 12 units across travelling 3000 units a
//! second covers 50 units in a 60 Hz step, so at the end of the step it
//! is already on the far side of a 20-unit wall, touching nothing. No
//! contact is generated and nothing looks broken - the ball simply
//! leaves.
//!
//! The answer is continuous collision detection, which sweeps the body's
//! shape along its path instead of testing where it landed. jEngine has
//! always **constructed** a `CCDSolver` and handed it to every step - but
//! rapier only sweeps bodies that ask to be swept, and until now nothing
//! could ask. The solver ran on an empty set for the life of the engine.
//!
//! Nothing noticed because no game here had gone fast enough. A physics
//! launcher game tested stacking, not speed. A pinball table is the case where the
//! ball is small, the flippers hit hard, and the outer wall is the only
//! thing between the ball and infinity.

#![cfg(feature = "physics")]

use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D};
use jengine_core::physics::PhysicsWorld;

const LENGTH_UNIT: f32 = 100.0;
const DT: f32 = 1.0 / 60.0;

/// Fire a small fast ball at a thin wall and report where it ended up.
///
/// The wall is 20 units thick, centred on the origin, so its near face is
/// at x = -10. Anything that ends up past x = +10 went through it.
fn fire_at_wall(ccd: bool, speed: f32) -> f32 {
    let mut w = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);

    w.add_body(1, &RigidBody2D { body_type: BodyType::Static, ..Default::default() }, 0.0, 0.0, 0.0);
    w.add_collider(
        1,
        &Collider2D {
            shape: ColliderShape::Box { width: 20.0, height: 400.0 },
            ..Default::default()
        },
    );

    let ball = RigidBody2D {
        body_type: BodyType::Dynamic,
        gravity_scale: 0.0,
        ccd,
        ..Default::default()
    };
    w.add_body(2, &ball, -200.0, 0.0, 0.0);
    w.add_collider(2, &Collider2D { shape: ColliderShape::Circle { radius: 6.0 }, ..Default::default() });
    w.set_velocity(2, speed, 0.0);

    // Every speed gets the same *travel budget* rather than the same
    // number of steps: enough time to reach x = +400 if nothing stops
    // it. A fixed step count made the first version of this test wrong -
    // at 3000 units a second the ball arrived exactly at the wall on the
    // last step, which reads as "it was stopped" whether or not anything
    // stopped it.
    let per_step = speed / 60.0;
    let steps = ((600.0 / per_step).ceil() as usize).clamp(1, 4000);
    for _ in 0..steps {
        w.step(DT);
    }
    w.get_position(2).map(|(x, _)| x).unwrap_or(f32::NAN)
}

#[test]
fn without_ccd_a_fast_ball_goes_straight_through() {
    // Recorded rather than fixed: this is what rapier does by default and
    // what every jEngine body did until `ccd` existed. Keeping the test
    // means the next person wondering why their bullet misses has the
    // answer in one place.
    let x = fire_at_wall(false, 3000.0);
    assert!(x > 10.0, "expected it to tunnel through; it stopped at {x}");
}

#[test]
fn with_ccd_the_same_ball_is_stopped() {
    let x = fire_at_wall(true, 3000.0);
    assert!(
        x < -3.0,
        "the ball should be stopped at the wall rather than past it; it reached {x}"
    );
}

#[test]
fn ccd_holds_at_the_speeds_a_flipper_actually_produces() {
    // A flipper launches a ball at a few thousand units a second, and a
    // table's outer wall is the only thing between the ball and infinity.
    for speed in [800.0, 1500.0, 3000.0, 6000.0, 12000.0] {
        let x = fire_at_wall(true, speed);
        assert!(x < 10.0, "at {speed} units a second the ball reached {x}");
    }
}

#[test]
fn ccd_is_off_unless_asked_for() {
    // The sweep is not free, so it stays opt-in and a scene that does not
    // mention it gets exactly the old behaviour.
    assert!(!RigidBody2D::default().ccd, "the default must not change under anybody");
}

#[test]
fn a_slow_body_behaves_the_same_either_way() {
    // The sanity check on the whole feature: turning CCD on must not move
    // anything that was not tunnelling in the first place. This one is
    // given the same travel budget, so it does reach the wall - it just
    // never crosses more than its own radius in a step.
    let without = fire_at_wall(false, 300.0);
    let with = fire_at_wall(true, 300.0);
    assert!(without < 0.0 && with < 0.0, "without {without}, with {with}");
    assert!(
        (without - with).abs() < 1.0,
        "CCD moved a slow body that was never tunnelling: {without} vs {with}"
    );
}
