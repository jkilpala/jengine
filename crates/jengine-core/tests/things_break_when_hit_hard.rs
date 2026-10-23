//! **A `breakable` body comes apart when it is hit hard enough** (Sep 2026):
//! a grid of pieces, each a body with a sub-rect of the sprite, carrying
//! the velocity the part had. Every host breaks things through
//! `ecs::fracture::break_on_impact` right after its physics step, and
//! throws the dust with `burst_debris` - one rule, no per-runtime copy.
//!
//! Found by the showcase's Collapse.

mod support;

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{BodyType, Collider2D, RigidBody2D, SpriteComponent, Transform, Velocity2D};
use jengine_core::ecs::fracture::{break_on_impact, Breakable};
use jengine_core::physics::PhysicsWorld;

static NATIVE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs")));
static VIEWPORT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs")));
static WEB: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs")));

#[test]
fn every_runtime_breaks_things_after_its_physics_step() {
    for (name, src) in [("native", NATIVE.as_str()), ("viewport", VIEWPORT.as_str()), ("web", WEB.as_str())] {
        // The frame's steps end at `end_frame()` (with or without
        // `fixed_update` scripts between them).
        let step = src.find("end_frame()").unwrap_or_else(|| panic!("{name}: no physics step"));
        let after = &src[step..step + 1200.min(src.len() - step)];
        assert!(after.contains("fracture::break_on_impact("), "{name} does not break things after its step");
        assert!(after.contains("fracture::burst_debris("), "{name} does not throw the dust");
    }
}

fn body(world: &mut World, pos: Vec2, w: f32, h: f32, kind: BodyType, v: f32, breakable: Option<Breakable>) -> hecs::Entity {
    let col: Collider2D =
        serde_json::from_value(serde_json::json!({ "shape": { "Box": { "width": w, "height": h } } })).unwrap();
    // CCD, or a fast block goes through the ground before it can hit it.
    let rb: RigidBody2D =
        serde_json::from_value(serde_json::json!({ "body_type": format!("{kind:?}"), "ccd": true })).unwrap();
    let e = world.spawn((
        Transform { position: pos, rotation: 0.0, scale: Vec2::ONE },
        col,
        rb,
        Velocity2D { x: 0.0, y: v, ..Default::default() },
        SpriteComponent { size: Some([w, h]), ..Default::default() },
    ));
    if let Some(b) = breakable {
        world.insert_one(e, b).unwrap();
    }
    e
}

/// Drop a block at `speed` from `from_y` onto the ground (its top at -80,
/// so a block rests at -60); how many bodies with sprites there are after a
/// second.
fn drop(speed: f32, threshold: f32, from_y: f32) -> usize {
    let mut world = World::new();
    // As a project sets it: -9.8 in metres, times the length unit.
    let mut physics = PhysicsWorld::new(0.0, -9.8, 100.0);
    body(&mut world, Vec2::new(0.0, -100.0), 800.0, 40.0, BodyType::Static, 0.0, None);
    body(
        &mut world,
        Vec2::new(0.0, from_y),
        60.0,
        40.0,
        BodyType::Dynamic,
        -speed,
        Some(Breakable { break_speed: threshold, cols: 3, rows: 2, ..Default::default() }),
    );
    for _ in 0..60 {
        jengine_core::ecs::systems::ensure_physics_bodies(&world, &mut physics);
        let contacts = physics.step(1.0 / 60.0);
        physics.sync_transforms_to_ecs(&mut world);
        break_on_impact(&mut world, &mut physics, &contacts, 1.0 / 60.0);
    }
    world.query::<&SpriteComponent>().iter().count() - 1
}

#[test]
fn a_block_dropped_hard_breaks_and_one_set_down_does_not() {
    assert_eq!(drop(900.0, 500.0, 0.0), 6, "a hard landing breaks the block into its 3 x 2 grid");
    assert_eq!(drop(0.0, 500.0, -59.0), 1, "set down, it stays whole: resting is no jolt");
    assert_eq!(drop(200.0, 500.0, -58.0), 1, "a soft landing is under its break speed");
}
