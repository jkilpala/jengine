//! A project can choose how many fixed physics steps run per second.
//!
//! The step was a hardcoded 60 Hz. `physics.fixed_timestep` in `project.jengine`
//! is the field every project carried and the editor's Project Settings edited,
//! which nothing read. It now sets the step through
//! `PhysicsWorld::set_fixed_timestep`, and every host reads it: the native
//! runner from the project settings, the web runtime from its
//! `WebPhysicsSettings`, and the editor viewport from the play call. The rate
//! changes how finely a second is integrated, not how far a body travels in
//! it.

#![cfg(feature = "physics")]

use hecs::World;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D, Transform};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;

fn fall_for_one_second(hz: f32) -> f32 {
    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 16.0, height: 16.0 }, ..Default::default() },
    ));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    physics.set_step_hz(hz);
    ensure_physics_bodies(&world, &mut physics);
    // Feed the accumulator in frames of a rate unrelated to the step rate.
    for _ in 0..100 {
        physics.step(0.01);
    }
    physics.sync_transforms_to_ecs(&mut world);
    world.get::<&Transform>(e).unwrap().position.y
}

#[test]
fn the_rate_is_reported_back() {
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    assert_eq!(physics.step_hz(), 60.0, "the default is the rate every game was built on");
    physics.set_step_hz(120.0);
    assert_eq!(physics.step_hz(), 120.0);
    physics.set_fixed_timestep(1.0 / 240.0);
    assert_eq!(physics.step_hz(), 240.0, "the project spells it in seconds");
    physics.set_fixed_timestep(0.0);
    assert_eq!(physics.step_hz(), 60.0, "a zero step is the default, not a division by it");
}

#[test]
fn a_body_falls_the_same_distance_per_second_at_any_rate() {
    let at_60 = fall_for_one_second(60.0);
    let at_120 = fall_for_one_second(120.0);
    let at_240 = fall_for_one_second(240.0);
    assert!(at_60 < -100.0, "a second of falling at 981 units/s² covers ground: {at_60}");
    // Semi-implicit Euler over-shoots by half a step of velocity per step, so the
    // finer rate lands a little short of the coarser one; a few percent, not a
    // different simulation.
    for (name, y) in [("120 Hz", at_120), ("240 Hz", at_240)] {
        let ratio = y / at_60;
        assert!((0.95..=1.05).contains(&ratio), "{name} fell {y} against {at_60} at 60 Hz (ratio {ratio})");
    }
}

#[test]
fn an_unreasonable_rate_is_clamped_rather_than_trusted() {
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    physics.set_step_hz(0.0);
    assert_eq!(physics.step_hz(), 10.0, "zero would divide the frame into nothing");
    physics.set_step_hz(f32::NAN);
    assert_eq!(physics.step_hz(), 60.0, "NaN falls back to the default");
    physics.set_step_hz(1_000_000.0);
    assert_eq!(physics.step_hz(), 1000.0, "a million steps a second is a stalled frame");
}

/// Every host applies the project's rate to the world it builds.
#[test]
fn every_host_applies_the_projects_step_rate() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    for (host, path) in [
        ("native runtime", "crates/jengine-runtime/src/wgpu_scene_runner.rs"),
        ("web runtime", "crates/jengine-web/src/lib.rs"),
        ("editor viewport", "crates/jengine-editor-viewport/src/lib.rs"),
    ] {
        let src = std::fs::read_to_string(format!("{root}/{path}")).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(src.contains("set_fixed_timestep("), "{host} ({path}) never applies the project's physics step rate");
    }
    let toolbar = std::fs::read_to_string(format!("{root}/editor/src/lib/Toolbar.svelte")).unwrap();
    assert!(
        toolbar.contains("physics?.fixed_timestep"),
        "the editor's play call does not hand the viewport the project's step rate"
    );
}
