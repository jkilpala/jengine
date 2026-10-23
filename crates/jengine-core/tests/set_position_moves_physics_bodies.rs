//! `set_position` on a dynamic physics body moves the body.
//!
//! A script's `set_position` writes the entity's `Transform`; the physics
//! step then wrote the rapier body's own pose back over it, every frame, so
//! on a dynamic body it did nothing in any runtime. Test-lab scene 84's
//! "drop them again" lifted three crates and they never left the floor
//! (BUGS #158). This drives the frame the way every runtime does:
//! `sync_kinematic_from_ecs`, `step`, `sync_transforms_to_ecs`.

#![cfg(feature = "physics")]

use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D, Transform};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;


/// One frame in the runtimes' order: physics, then scripts (`script`, which
/// is where `set_position` lands), then the render blend.
fn frame_with(
    physics: &mut PhysicsWorld,
    world: &mut hecs::World,
    dt: f32,
    script: impl FnOnce(&mut hecs::World),
) -> Vec<jengine_core::physics::ContactEvent> {
    ensure_physics_bodies(world, physics);
    physics.sync_kinematic_from_ecs(world);
    let events = physics.step(dt);
    physics.sync_transforms_to_ecs(world);
    script(world);
    // Every host blends after scripts and packages. It wrote the physics pose
    // over a script's set_position before the teleport could see it.
    physics.blend_transforms_for_render(world);
    events
}

fn frame(physics: &mut PhysicsWorld, world: &mut hecs::World, dt: f32) -> Vec<jengine_core::physics::ContactEvent> {
    frame_with(physics, world, dt, |_| {})
}

fn at(x: f32, y: f32) -> Transform {
    Transform { position: glam::Vec2::new(x, y), ..Default::default() }
}

fn boxed(w: f32, h: f32) -> Collider2D {
    Collider2D { shape: ColliderShape::Box { width: w, height: h }, ..Default::default() }
}

#[test]
fn a_moved_dynamic_body_is_teleported_and_falls_again() {
    // 60 Hz steps once a frame; at 144 / 165 most frames run no step, which
    // is the editor's case and the one the first fix missed.
    for hz in [60.0f32, 144.0, 165.0] {
        teleport_at(1.0 / hz);
    }
}

fn teleport_at(dt: f32) {
    let frames = |secs: f32| (secs / dt) as usize;
    let mut world = hecs::World::new();
    world.spawn((at(0.0, -240.0), RigidBody2D { body_type: BodyType::Static, ..Default::default() }, boxed(800.0, 32.0)));
    let crate_e = world.spawn((
        at(0.0, -100.0),
        RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, ..Default::default() },
        boxed(40.0, 40.0),
    ));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    let y = |w: &hecs::World| w.get::<&Transform>(crate_e).unwrap().position.y;

    for _ in 0..frames(3.0) {
        frame(&mut physics, &mut world, dt);
    }
    let resting = y(&world);
    assert!((resting - (-204.0)).abs() < 3.0, "the crate should rest on the floor, at {resting}");

    // What set_position does: write the transform, from the script phase.
    frame_with(&mut physics, &mut world, dt, |w| {
        w.get::<&mut Transform>(crate_e).unwrap().position.y = 200.0;
    });
    frame(&mut physics, &mut world, dt);
    assert!(y(&world) > 150.0, "the body snapped back to the floor instead of moving: {}", y(&world));

    // And it falls from there and lands hard.
    let mut landing = 0.0f32;
    for _ in 0..frames(3.0) {
        for ev in frame(&mut physics, &mut world, dt) {
            landing = landing.max(ev.impulse);
        }
    }
    assert!((y(&world) - resting).abs() < 3.0, "it should land on the floor again");
    assert!(landing > 100.0, "a 400-unit drop should report a real impact, got {landing}");
}

#[test]
fn a_body_nobody_moved_is_left_to_the_solver() {
    // A stack settling must not be "teleported" by its own sync.
    let mut world = hecs::World::new();
    world.spawn((at(0.0, -240.0), RigidBody2D { body_type: BodyType::Static, ..Default::default() }, boxed(800.0, 32.0)));
    let e = world.spawn((
        at(0.0, 0.0),
        RigidBody2D { body_type: BodyType::Dynamic, mass: 1.0, ..Default::default() },
        boxed(40.0, 40.0),
    ));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    let mut last = f32::MAX;
    // 30 frames: it reaches the floor near frame 38, where settling may nudge it up.
    for i in 0..30 {
        frame(&mut physics, &mut world, 1.0 / 60.0);
        let y = world.get::<&Transform>(e).unwrap().position.y;
        if i > 0 {
            assert!(y <= last + 1e-3, "free fall went up at frame {i}: {last} -> {y}");
        }
        last = y;
    }
}
