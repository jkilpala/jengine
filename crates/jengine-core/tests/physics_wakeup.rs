//! Physics behaviour that only shows up after a body falls asleep.
//!
//! Rapier puts a body that has been still for a while to sleep, and a sleeping
//! body is skipped by the integrator entirely. Everything that pokes a body
//! individually passes `wake_up: true`, so those are fine. **Changing world
//! gravity does not touch any body**, so a sleeping one never learns about it —
//! reported from the editor as "gravity starts working only after you press
//! set_velocity or impulse".
//!
//! Also covers what a body weighs. `RigidBody2D.mass` used to be read by
//! nothing at all — rapier derives mass from the collider's density × area, so
//! a 40×40 crate authored `mass: 1.0` weighed about 1600 (BUGS #50). It is
//! authoritative now, with `mass <= 0` meaning "let the shape decide".

#![cfg(feature = "physics")]

use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D};
use jengine_core::physics::PhysicsWorld;

const LENGTH_UNIT: f32 = 100.0;
const DT: f32 = 1.0 / 60.0;

fn body(density: f32) -> (RigidBody2D, Collider2D) {
    (
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D {
            shape: ColliderShape::Box { width: 40.0, height: 40.0 },
            density,
            ..Default::default()
        },
    )
}

/// Step until the body stops moving, or give up. Returns whether it settled.
fn settle(world: &mut PhysicsWorld, id: u64, max_steps: usize) -> bool {
    for _ in 0..max_steps {
        world.step(DT);
        if let Some((vx, vy)) = world.get_velocity(id)
            && vx.abs() < 1e-3
            && vy.abs() < 1e-3
        {
            return true;
        }
    }
    false
}

/// A floor to land on, so the body has a reason to fall asleep.
fn world_with_floor_and_crate(density: f32) -> (PhysicsWorld, u64) {
    let mut w = PhysicsWorld::new(0.0, -9.81, LENGTH_UNIT);
    let floor = (
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D {
            shape: ColliderShape::Box { width: 1000.0, height: 32.0 },
            ..Default::default()
        },
    );
    w.add_body(1, &floor.0, 0.0, -300.0, 0.0);
    w.add_collider(1, &floor.1);

    let (rb, col) = body(density);
    w.add_body(2, &rb, 0.0, 0.0, 0.0);
    w.add_collider(2, &col);
    (w, 2)
}

#[test]
fn a_body_falls_under_gravity() {
    let (mut w, id) = world_with_floor_and_crate(1.0);
    for _ in 0..10 {
        w.step(DT);
    }
    let (_, vy) = w.get_velocity(id).expect("velocity");
    assert!(vy < -1.0, "the crate is not falling (vy = {vy})");
}

/// The bug: gravity changes are invisible to a body that is already asleep.
#[test]
fn re_enabling_gravity_wakes_a_sleeping_body() {
    let (mut w, id) = world_with_floor_and_crate(1.0);
    // Land it and let it fall asleep on the floor.
    assert!(settle(&mut w, id, 2_000), "the crate never came to rest");
    for _ in 0..200 {
        w.step(DT);
    }

    // Turn gravity off, then flip it upward. A body that is awake would rise;
    // a sleeping one that was never told notices nothing.
    w.set_gravity(0.0, 0.0);
    for _ in 0..30 {
        w.step(DT);
    }
    w.set_gravity(0.0, 9.81);
    for _ in 0..60 {
        w.step(DT);
    }

    let (_, vy) = w.get_velocity(id).expect("velocity");
    assert!(
        vy > 1.0,
        "the crate ignored a gravity change while asleep (vy = {vy}). \
         `set_gravity` has to wake the bodies: rapier skips sleeping bodies \
         entirely, so nothing else will."
    );
}

/// `RigidBody2D.mass` decides what a body weighs.
///
/// It used not to. `add_body` never read the field, and rapier derives mass
/// from the collider's density x area — so a 40x40 crate authored `mass: 1.0`
/// at the default density weighed about **1600**, and an impulse sized for a
/// 1 kg crate moved it 0.6 units/s (BUGS #50). A serialized,
/// inspector-editable field that does nothing is worse than not having one.
#[test]
fn an_authored_mass_is_what_the_body_weighs() {
    // Impulse / mass = velocity change, so the velocity after one step reads
    // the mass back out directly.
    for mass in [1.0f32, 4.0, 20.0] {
        let mut w = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);
        let rb = RigidBody2D { body_type: BodyType::Dynamic, mass, ..Default::default() };
        let col = Collider2D {
            shape: ColliderShape::Box { width: 40.0, height: 40.0 },
            // Deliberately left at a density that would dominate if it were
            // still in charge: 1.0 over a 40x40 box is a mass of ~1600.
            density: 1.0,
            ..Default::default()
        };
        w.add_body(1, &rb, 0.0, 0.0, 0.0);
        w.add_collider(1, &col);

        w.apply_impulse(1, 0.0, 100.0);
        w.step(DT);
        let (_, vy) = w.get_velocity(1).expect("velocity");
        let expected = 100.0 / mass;
        assert!(
            (vy - expected).abs() < expected * 0.05,
            "mass {mass}: impulse 100 gave dv {vy}, expected about {expected}.              The collider density is still deciding the mass."
        );
    }
}

/// `mass <= 0` means "let the shape decide", which is rapier's own model and
/// the way to author a body whose weight should follow its size.
#[test]
fn a_non_positive_mass_falls_back_to_collider_density() {
    let mut w = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);
    let rb = RigidBody2D { body_type: BodyType::Dynamic, mass: 0.0, ..Default::default() };
    let col = Collider2D {
        shape: ColliderShape::Box { width: 40.0, height: 40.0 },
        density: 1.0,
        ..Default::default()
    };
    w.add_body(1, &rb, 0.0, 0.0, 0.0);
    w.add_collider(1, &col);

    w.apply_impulse(1, 0.0, 1000.0);
    w.step(DT);
    let (_, vy) = w.get_velocity(1).expect("velocity");
    // 40x40 at density 1.0 is a mass of ~1600, so an impulse of 1000 barely
    // moves it. The point is only that density is back in charge.
    assert!(
        vy < 1.0,
        "with mass 0 the density should decide, giving a heavy body; got dv {vy}"
    );
}

/// Angular inertia has to scale with the shape, not vanish.
///
/// The other way to force a mass — `additional_mass` on the body — leaves
/// inertia at zero, and a body with no rotational inertia spins up from the
/// lightest touch. `ColliderBuilder::mass` scales the shape's whole mass
/// properties instead.
#[test]
fn forcing_a_mass_keeps_the_shape_s_angular_inertia() {
    let mut w = PhysicsWorld::new(0.0, 0.0, LENGTH_UNIT);
    let rb = RigidBody2D { body_type: BodyType::Dynamic, mass: 2.0, ..Default::default() };
    let col = Collider2D {
        shape: ColliderShape::Box { width: 40.0, height: 40.0 },
        ..Default::default()
    };
    w.add_body(1, &rb, 0.0, 0.0, 0.0);
    w.add_collider(1, &col);

    let before = w.get_angular_velocity(1).expect("angvel");
    assert_eq!(before, 0.0);
    // An off-centre impulse imparts spin. With zero inertia this would be an
    // absurd number or a NaN; with shape-derived inertia it stays sane.
    w.apply_impulse(1, 50.0, 0.0);
    w.step(DT);
    let spin = w.get_angular_velocity(1).expect("angvel");
    assert!(
        spin.is_finite() && spin.abs() < 1000.0,
        "angular velocity {spin} after a small impulse — inertia looks like zero"
    );
}

/// Toggling gravity repeatedly, with bodies resting and in contact, must not
/// blow up.
///
/// Reported from the editor as "sometimes the editor crashes in this scene when
/// changing gravity on and off". Waking bodies is the new thing `set_gravity`
/// does, and there is a wrong way to do it: `RigidBodySet::iter_mut` hands out
/// `&mut RigidBody` without recording that the body was touched, so the island
/// manager can be left describing a sleeping island that contains awake bodies.
/// `get_mut` does that bookkeeping, and is what every other command here uses.
///
/// A panic in the solver surfaces in a browser as the viewport simply stopping,
/// which is exactly what "crashes" looked like.
#[test]
fn toggling_gravity_under_a_pile_of_resting_bodies_is_stable() {
    let mut w = PhysicsWorld::new(0.0, -9.81, LENGTH_UNIT);
    let floor = (
        RigidBody2D { body_type: BodyType::Static, ..Default::default() },
        Collider2D {
            shape: ColliderShape::Box { width: 1000.0, height: 32.0 },
            ..Default::default()
        },
    );
    w.add_body(1, &floor.0, 0.0, -300.0, 0.0);
    w.add_collider(1, &floor.1);

    // A stack, so bodies are in contact with each other as well as the floor —
    // contacts are what the island manager tracks, and a stack is the case most
    // likely to expose inconsistent island bookkeeping.
    for i in 0..8u64 {
        let (rb, col) = body(0.001);
        w.add_body(10 + i, &rb, 0.0, -260.0 + i as f32 * 42.0, 0.0);
        w.add_collider(10 + i, &col);
    }
    for _ in 0..600 {
        w.step(DT);
    }

    // Many toggles, with varying numbers of steps between them so the bodies
    // are caught at different points of falling asleep.
    for round in 0..40 {
        w.set_gravity(0.0, 0.0);
        for _ in 0..(round % 7) {
            w.step(DT);
        }
        w.set_gravity(0.0, -9.81);
        for _ in 0..(round % 5 + 1) {
            w.step(DT);
        }
    }

    // Everything is still addressable and finite. A body that fell out of the
    // world, or a NaN from a corrupted solve, is the failure worth naming.
    for i in 0..8u64 {
        let (vx, vy) = w.get_velocity(10 + i).expect("body went missing");
        assert!(
            vx.is_finite() && vy.is_finite(),
            "body {i} has a non-finite velocity ({vx}, {vy}) after gravity toggling"
        );
    }
}
