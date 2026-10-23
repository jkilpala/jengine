//! A dynamic body moves every frame, whatever the display's rate.
//!
//! The simulation steps at 60 Hz. Drawn at 165 Hz, a frame runs either zero
//! or one step, so drawing the *last stepped pose* held a falling body still
//! on most frames and jumped it on the rest - 60 Hz motion under a 165 Hz
//! frame counter, which reads as judder that no frame-rate number explains.
//! `PhysicsWorld::blend_transforms_for_render` writes, just before drawing,
//! a pose blended between the one before the latest step and the one after
//! it, by how far the accumulator has run since, so every frame moves.
//!
//! It is a separate call from `sync_transforms_to_ecs`, which keeps writing
//! the stepped pose, because the blend lags the simulation by up to a step
//! and gameplay reads the transform: with the blend in the sync, a hill-driving
//! game's vehicle decided "wheel on the ground" a step late and could not
//! climb its own default course (that package's own test drives it).

#![cfg(feature = "physics")]

mod support;

use hecs::World;
use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D, Transform};
use jengine_core::ecs::systems::ensure_physics_bodies;
use jengine_core::physics::PhysicsWorld;

fn falling_body() -> (World, PhysicsWorld, hecs::Entity) {
    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 16.0, height: 16.0 }, ..Default::default() },
    ));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    ensure_physics_bodies(&world, &mut physics);
    (world, physics, e)
}

fn y_of(world: &World, e: hecs::Entity) -> f32 {
    world.get::<&Transform>(e).unwrap().position.y
}

#[test]
fn a_falling_body_moves_on_every_165hz_frame() {
    let (mut world, mut physics, e) = falling_body();
    // Warm up past the first step so there is a previous pose to blend from,
    // reading the drawn pose the way a host does every frame.
    for _ in 0..3 {
        physics.step(1.0 / 60.0);
        physics.sync_transforms_to_ecs(&mut world);
        physics.blend_transforms_for_render(&mut world);
    }
    let mut last = y_of(&world, e);
    let mut frames_that_moved = 0;
    for _ in 0..60 {
        physics.step(1.0 / 165.0);
        physics.sync_transforms_to_ecs(&mut world);
        physics.blend_transforms_for_render(&mut world);
        let y = y_of(&world, e);
        assert!(y <= last, "a falling body never goes up: {y} after {last}");
        if y < last {
            frames_that_moved += 1;
        }
        last = y;
    }
    // Before the fix roughly two frames in three were a repeat of the last.
    assert_eq!(frames_that_moved, 60, "every frame moved the body");
}

#[test]
fn the_blend_never_runs_ahead_of_the_simulation() {
    let (mut world, mut physics, e) = falling_body();
    for _ in 0..40 {
        physics.step(1.0 / 165.0);
        physics.sync_transforms_to_ecs(&mut world);
        physics.blend_transforms_for_render(&mut world);
        let drawn = y_of(&world, e);
        let (_, simulated) = physics.get_position(e.id() as u64).expect("the body exists");
        // Drawn between the last two stepped poses: never below the newest.
        assert!(drawn >= simulated - 1e-4, "drawn {drawn} is ahead of the simulation {simulated}");
    }
}

/// What scripts and packages read is the stepped pose, blend or no blend:
/// the sync after a step writes exactly what the simulation holds, even
/// when the previous frame's blend left the transform elsewhere.
#[test]
fn the_sync_writes_the_stepped_pose_not_the_blend() {
    let (mut world, mut physics, e) = falling_body();
    for _ in 0..20 {
        physics.step(1.0 / 165.0);
        physics.sync_transforms_to_ecs(&mut world);
        let (_, stepped) = physics.get_position(e.id() as u64).expect("the body exists");
        assert!((y_of(&world, e) - stepped).abs() < 1e-5, "the sync is the stepped pose");
        physics.blend_transforms_for_render(&mut world);
    }
}

/// A frame that did not step draws what the sync wrote: no stale blend.
#[test]
fn a_frame_without_a_step_blends_nothing() {
    let (mut world, mut physics, e) = falling_body();
    physics.step(1.0 / 60.0);
    physics.sync_transforms_to_ecs(&mut world);
    physics.blend_transforms_for_render(&mut world);
    // Now a frame with no step at all (the editor out of play): the sync
    // rewrites the stepped pose and the blend must leave it alone.
    physics.sync_transforms_to_ecs(&mut world);
    let stepped = y_of(&world, e);
    physics.blend_transforms_for_render(&mut world);
    assert_eq!(y_of(&world, e), stepped);
}

/// Every host blends before its hierarchy resolve, after scripts and
/// packages - the order is the whole point of the split.
#[test]
fn every_host_blends_for_drawing_after_the_frame_has_read_the_stepped_pose() {
    for (host, rel) in [
        ("native runtime", "jengine-runtime/src/wgpu_scene_runner.rs"),
        ("editor viewport", "jengine-editor-viewport/src/lib.rs"),
        ("web runtime", "jengine-web/src/lib.rs"),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel);
        let src = support::with_frame_draw(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}")));
        let blend = src.find("blend_transforms_for_render(").unwrap_or_else(|| panic!("{host} never blends physics poses for drawing"));
        let resolve = src[blend..].find("resolve_world_transforms(").map(|i| i + blend);
        assert!(resolve.is_some_and(|r| r - blend < 800), "{host}: the blend is not right before the hierarchy resolve");
        // The frame's order (sync, scripts, blend, resolve, draw) crosses
        // functions in every host, so it cannot be read off the file; the
        // blend's neighbour is what can be.
        assert!(src.contains("sync_transforms_to_ecs("), "{host} no longer syncs physics poses");
    }
}

/// The next frame starts from the stepped pose, not the picture: whatever
/// reads the world before the step - a package's `ScriptExec` system - must
/// not see the blend. On a 165 Hz frame with no step the blend was never
/// overwritten, so a driving game's pitch control read a jeep that jumped
/// back and forth between the blend and the step and flipped it.
#[test]
fn the_next_frame_reads_the_stepped_pose_again() {
    let (mut world, mut physics, e) = falling_body();
    for _ in 0..3 {
        physics.step(1.0 / 60.0);
        physics.sync_transforms_to_ecs(&mut world);
    }
    physics.step(1.0 / 165.0);
    physics.sync_transforms_to_ecs(&mut world);
    let stepped = y_of(&world, e);
    physics.blend_transforms_for_render(&mut world);
    assert_ne!(y_of(&world, e), stepped, "the frame is drawn blended");
    physics.restore_stepped_poses(&mut world);
    assert_eq!(y_of(&world, e), stepped, "the next frame starts from the stepped pose");

    // A transform a script moved since the blend is the script's.
    physics.blend_transforms_for_render(&mut world);
    physics.step(1.0 / 60.0);
    physics.sync_transforms_to_ecs(&mut world);
    physics.blend_transforms_for_render(&mut world);
    world.get::<&mut Transform>(e).unwrap().position.y = 500.0;
    physics.restore_stepped_poses(&mut world);
    assert_eq!(y_of(&world, e), 500.0, "a script's set_position survives");
}

/// Each host puts the stepped pose back at the start of its frame - before
/// behaviour graphs, motion and every package phase read the world.
#[test]
fn every_host_starts_its_frame_from_the_stepped_pose() {
    for (host, rel) in [
        ("native runtime", "jengine-runtime/src/wgpu_scene_runner.rs"),
        ("editor viewport", "jengine-editor-viewport/src/lib.rs"),
        ("web runtime", "jengine-web/src/lib.rs"),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel);
        let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let restore = src
            .find("restore_stepped_poses(")
            .unwrap_or_else(|| panic!("{host} never puts the stepped physics pose back"));
        for later in ["tick_pre_simulation(", "tick_motion(", "SystemPhase::PreUpdate", "SystemPhase::ScriptExec"] {
            let at = src.find(later).unwrap_or_else(|| panic!("{host}: no {later}"));
            assert!(restore < at, "{host}: {later} runs before the stepped pose is put back");
        }
    }
}
