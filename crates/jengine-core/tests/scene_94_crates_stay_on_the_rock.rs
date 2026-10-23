//! Test-lab scene 94, end to end: its own map, its own crates, real physics.
//!
//! The scene digs a 160x90 tilemap under five resting crates. What it
//! promises is that a crate rests on rock, falls when the rock under it is
//! dug, and lands on the rock below the hole - never through it.

#![cfg(feature = "physics")]

use std::path::PathBuf;

use jengine_core::ecs::components::{Name, RigidBody2D, Transform};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::physics::PhysicsWorld;
use jengine_core::scene::load_scene_auto;
use jengine_core::tilemap::Tilemap;

const TILE: f32 = 16.0;
const OX: f32 = -1280.0;
const OY: f32 = 720.0;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../projects/engine-test-lab")
}

fn load() -> (hecs::World, PhysicsWorld) {
    let text = std::fs::read_to_string(project_root().join("project.jengine")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    let p = &doc["settings"]["physics"];
    let (gx, gy, lu) = (
        p["gravity_x"].as_f64().unwrap_or(0.0) as f32,
        p["gravity_y"].as_f64().unwrap_or(-9.8) as f32,
        p["length_unit"].as_f64().unwrap_or(100.0) as f32,
    );
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let text =
        std::fs::read_to_string(project_root().join("assets/scenes/94-tilemap-chunks.scene")).unwrap();
    let mut world = hecs::World::new();
    load_scene_auto(&text, &mut world, &registry).expect("load scene 94");
    (world, PhysicsWorld::new(gx, gy, lu))
}

fn rock(world: &hecs::World) -> hecs::Entity {
    world.query::<&Name>().iter().find(|(_, n)| n.0 == "Rock").map(|(e, _)| e).expect("Rock")
}

fn crates(world: &hecs::World) -> Vec<hecs::Entity> {
    world.query::<(&Name, &RigidBody2D)>().iter().filter(|(_, (n, _))| n.0 == "Crate").map(|(e, _)| e).collect()
}

fn pos(world: &hecs::World, e: hecs::Entity) -> (f32, f32) {
    let t = world.get::<&Transform>(e).unwrap();
    (t.position.x, t.position.y)
}

/// The top of the highest solid tile under world x, as a world y.
fn surface_under(world: &hecs::World, x: f32, below_y: f32) -> f32 {
    let tm = world.get::<&Tilemap>(rock(world)).unwrap();
    let col = ((x - OX) / TILE).floor() as u32;
    let start = ((OY - below_y) / TILE).floor().max(0.0) as u32;
    let layer = &tm.layers[0];
    let row = (start..layer.height).find(|&r| layer.get_tile(col, r).is_some()).expect("rock below");
    OY - row as f32 * TILE
}

/// Frames at a display rate, in the order a runtime runs them.
fn run(world: &mut hecs::World, physics: &mut PhysicsWorld, seconds: f32, hz: f32) {
    let dt = 1.0 / hz;
    for _ in 0..(seconds * hz) as usize {
        jengine_core::ecs::systems::ensure_physics_bodies(world, physics);
        physics.step(dt);
        physics.sync_transforms_to_ecs(world);
        physics.blend_transforms_for_render(world);
    }
}

#[test]
fn scene_94_crates_rest_on_the_rock() {
    let (mut world, mut physics) = load();
    let all = crates(&world);
    assert_eq!(all.len(), 5);
    let start: Vec<_> = all.iter().map(|&e| pos(&world, e)).collect();
    run(&mut world, &mut physics, 3.0, 165.0);
    for (&e, &(x0, y0)) in all.iter().zip(&start) {
        let (x, y) = pos(&world, e);
        let floor = surface_under(&world, x0, y0);
        assert!(
            y > floor,
            "the crate at ({x0}, {y0}) should rest on the rock at y {floor}; it is at ({x}, {y})"
        );
    }
}

#[test]
fn scene_94_a_dug_crate_lands_on_the_rock_below() {
    let (mut world, mut physics) = load();
    run(&mut world, &mut physics, 1.0, 165.0);
    let e = crates(&world)[0];
    let (x, y) = pos(&world, e);
    let col = ((x - OX) / TILE).floor() as i32;
    let top = ((OY - y) / TILE).floor() as i32;
    // Dig a shaft 5 wide and 8 deep under it, the way the script does: one
    // `set_tile(nil)` per cell.
    {
        let r = rock(&world);
        let mut tm = world.get::<&mut Tilemap>(r).unwrap();
        for row in top..top + 10 {
            for c in col - 2..=col + 2 {
                tm.layers[0].set_tile(c as u32, row as u32, None);
            }
        }
    }
    let floor = OY - (top + 10) as f32 * TILE;
    run(&mut world, &mut physics, 3.0, 165.0);
    let (x2, y2) = pos(&world, e);
    assert!(y2 < y - 4.0 * TILE, "the crate did not fall into its shaft: ({x}, {y}) -> ({x2}, {y2})");
    assert!(
        y2 > floor,
        "the crate fell through the bottom of its shaft (y {floor}): ({x}, {y}) -> ({x2}, {y2})"
    );
}
