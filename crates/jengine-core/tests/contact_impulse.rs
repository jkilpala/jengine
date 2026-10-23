//! A collision says how hard it was, and where.
//!
//! `ContactEvent` carried only *who* touched, so a script could not tell a
//! nudge from a crash: "break this crate if something lands on it hard
//! enough" had to be guessed from velocities read a frame late, on entities
//! the script had to know about in advance. Rapier has had the solver's
//! per-contact impulses all along and the engine discarded them.
//!
//! The impulse also arrives in `on_collision_enter`'s payload, which is
//! where a physics game actually wants it.
//!
//! A contact is reported one step after rapier first sees the pair, because
//! the solver writes the impulse during the step that resolves it: read on
//! the step the pair appears, every impact in the world is zero.
//!
//! Gravity here is around -10, not -300: the engine's `length_unit` is 100,
//! so -300 is an enormous acceleration and a falling crate tunnels straight
//! through the floor.

#![cfg(feature = "physics")]

use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D};
use jengine_core::physics::{ContactType, PhysicsWorld};

const DT: f32 = 1.0 / 60.0;
const FLOOR: u64 = 1;
const CRATE: u64 = 2;

fn box_collider(width: f32, height: f32) -> Collider2D {
    Collider2D { shape: ColliderShape::Box { width, height }, ..Default::default() }
}

/// A floor, and one crate dropped onto it from `height`. Returns the first
/// contact between them.
fn drop_crate(height: f32, gravity: f32) -> Option<jengine_core::physics::ContactEvent> {
    let mut w = PhysicsWorld::new(0.0, gravity, 100.0);
    w.add_body(FLOOR, &RigidBody2D { body_type: BodyType::Static, ..Default::default() }, 0.0, -50.0, 0.0);
    w.add_collider(FLOOR, &box_collider(400.0, 20.0));
    w.add_body(CRATE, &RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, ..Default::default() }, 0.0, height, 0.0);
    w.add_collider(CRATE, &box_collider(40.0, 40.0));

    for _ in 0..900 {
        for ev in w.step(DT) {
            if ev.event_type == ContactType::Started {
                return Some(ev);
            }
        }
    }
    None
}

#[test]
fn a_harder_landing_reports_a_bigger_impulse() {
    let soft = drop_crate(-10.0, -9.81).expect("landed").impulse;
    let hard = drop_crate(300.0, -9.81).expect("landed").impulse;
    assert!(soft >= 0.0, "an impulse is never negative: {soft}");
    assert!(hard > soft * 2.0, "dropping further should hit harder: {hard} vs {soft}");

    // And harder again under more gravity, for the same reason.
    let heavier = drop_crate(300.0, -30.0).expect("landed").impulse;
    assert!(heavier > hard, "more gravity, harder landing: {heavier} vs {hard}");
}

#[test]
fn the_contact_says_where_it_happened() {
    let mut w = PhysicsWorld::new(0.0, -9.81, 100.0);
    w.add_body(FLOOR, &RigidBody2D { body_type: BodyType::Static, ..Default::default() }, 0.0, -50.0, 0.0);
    w.add_collider(FLOOR, &box_collider(400.0, 20.0));
    // Dropped off to one side: the contact must be reported over there, not
    // at the origin.
    w.add_body(CRATE, &RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, ..Default::default() }, 120.0, 100.0, 0.0);
    w.add_collider(CRATE, &box_collider(40.0, 40.0));

    for _ in 0..900 {
        for ev in w.step(DT) {
            if ev.event_type == ContactType::Started {
                assert!((ev.point.0 - 120.0).abs() < 40.0, "contact x was {}", ev.point.0);
                assert!((ev.point.1 + 40.0).abs() < 40.0, "contact y was {}", ev.point.1);
                return;
            }
        }
    }
    panic!("no contact");
}

#[test]
fn a_resting_stack_is_not_an_impact() {
    // A crate already sitting on the floor reports its first contact, but
    // the load of simply resting is far below what landing from a height
    // costs - which is the difference a game needs to tell.
    let resting = drop_crate(-29.0, -9.81).expect("touching already").impulse;
    let falling = drop_crate(300.0, -9.81).expect("landed").impulse;
    assert!(falling > resting * 3.0, "resting {resting} vs landing {falling}");
}

#[test]
fn letting_go_reports_no_impulse() {
    let mut w = PhysicsWorld::new(0.0, -9.81, 100.0);
    w.add_body(FLOOR, &RigidBody2D { body_type: BodyType::Static, ..Default::default() }, 0.0, -50.0, 0.0);
    w.add_collider(FLOOR, &box_collider(200.0, 20.0));
    // A crate resting on the floor, then flicked upward: when it stops
    // touching, the `Stopped` event has nothing to report.
    w.add_body(CRATE, &RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, ..Default::default() }, 0.0, -18.0, 0.0);
    w.add_collider(CRATE, &box_collider(40.0, 40.0));
    for _ in 0..30 {
        w.step(DT);
    }
    w.apply_impulse(CRATE, 0.0, 40.0);

    for _ in 0..600 {
        for ev in w.step(DT) {
            if ev.event_type == ContactType::Stopped {
                assert_eq!(ev.impulse, 0.0);
                return;
            }
        }
    }
    panic!("the crate never left the floor");
}
