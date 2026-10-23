//! `apply_force` pushes for the frame it is called in, and no longer.
//!
//! It used to hand the force to rapier's `add_force`, which keeps a force on
//! a body until `reset_forces` - and nothing reset it. Every frame's call
//! stacked on the last and none went away: holding a key ramped the push
//! without bound and letting go changed nothing (test-lab scene 56's ball
//! accelerated until it tunnelled through a wall, BUGS #156).
//!
//! A force held for one second on a unit mass must end at `F` units/s, at
//! any frame rate, and stay there once released.

#![cfg(feature = "physics")]

use jengine_core::ecs::components::{BodyType, Collider2D, RigidBody2D, Transform};
use jengine_core::physics::PhysicsWorld;

fn ball(world: &mut hecs::World) -> u64 {
    let collider: Collider2D =
        serde_json::from_value(serde_json::json!({ "shape": { "Circle": { "radius": 10.0 } } })).unwrap();
    let rb = RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, gravity_scale: 0.0, ..Default::default() };
    world.spawn((Transform::default(), rb, collider)).id() as u64
}

fn held_for_one_second(frame_dt: f32) -> (f32, f32) {
    let mut world = hecs::World::new();
    let id = ball(&mut world);
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    jengine_core::ecs::systems::ensure_physics_bodies(&world, &mut physics);

    const F: f32 = 300.0;
    let frames = (1.0 / frame_dt).round() as usize;
    for _ in 0..frames {
        physics.apply_force(id, F, 0.0);
        physics.step(frame_dt);
    }
    let held = physics.get_velocity(id).unwrap().0;
    for _ in 0..frames {
        physics.step(frame_dt);
    }
    let released = physics.get_velocity(id).unwrap().0;
    (held, released)
}

#[test]
fn a_held_force_accelerates_by_f_over_m_at_any_frame_rate() {
    for hz in [60.0f32, 144.0, 165.0, 30.0] {
        let (held, released) = held_for_one_second(1.0 / hz);
        assert!(
            (held - 300.0).abs() < 300.0 * 0.03,
            "at {hz} Hz a 300 N push on 1 kg for 1 s gave {held} units/s, not 300 - \
             forces are accumulating across frames again"
        );
        assert!(
            (released - held).abs() < 1.0,
            "at {hz} Hz the body kept accelerating after the force stopped ({held} -> {released})"
        );
    }
}
