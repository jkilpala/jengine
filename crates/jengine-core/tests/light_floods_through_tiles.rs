//! **Light flooded through a tile grid** (Oct 2026; `ecs::tile_light`,
//! Terraria's and Minecraft's light). A `tile_light` on a tilemap spreads
//! the light of its sources - `tile_light_source` entities and the map's own
//! glowing tiles - cell by cell, round corners, into the faces of the rock
//! and no further, and draws it as a lightmap over the map. The flood itself
//! is unit-tested in the module; this runs it through `sync` (the texture),
//! `gather_lights` and `light_at_textured`, which is what the screen shows,
//! and through `TileLight.set`. Found by the showcase's Torchlit Depths.

use glam::Vec2;
use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::{AmbientLight, Transform};
use jengine_core::ecs::tile_light::{self, TileEmitter, TileLight, TileLightSource};
use jengine_core::renderer::lighting::{gather_lights, light_at_textured};
use jengine_core::tilemap::{Tilemap, TilemapLayer};

const T: f32 = 16.0;

/// `#` rock (the `rock` layer, collision), `L` lava (the `glow` layer, which
/// gives off light), `.` air. The map's top-left corner at the origin, so
/// cell `(c, r)` is centred on `(c*16 + 8, -r*16 - 8)`.
fn map(rows: &[&str]) -> Tilemap {
    let (w, h) = (rows[0].len() as u32, rows.len() as u32);
    let mut rock = TilemapLayer { name: "rock".into(), width: w, height: h, visible: true, collision: true, ..Default::default() };
    let mut glow = TilemapLayer { name: "glow".into(), width: w, height: h, visible: true, ..Default::default() };
    for r in rows {
        for ch in r.chars() {
            rock.tiles.push(if ch == '#' { 1 } else { 0 });
            glow.tiles.push(if ch == 'L' { 2 } else { 0 });
        }
    }
    rock.touch_all();
    glow.touch_all();
    Tilemap { tile_size: [T, T], layers: vec![glow, rock], ..Default::default() }
}

fn cell(c: u32, r: u32) -> Vec2 {
    Vec2::new(c as f32 * T + 8.0, -(r as f32) * T - 8.0)
}

const CAVE: &[&str] = &[
    "################",
    "#......#.......#",
    "#......#.......#",
    "#......#####...#",
    "#..............#",
    "################",
    "################",
    "################",
];

fn world_with_torch(at: Vec2) -> (World, hecs::Entity, hecs::Entity) {
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    let m = world.spawn((Transform::default(), map(CAVE), TileLight { decay: 0.85, solid_decay: 0.4, ..Default::default() }));
    let torch = world.spawn((
        Transform { position: at, ..Default::default() },
        TileLightSource { color: [1.0, 0.6, 0.2], intensity: 1.0, on: true },
    ));
    (world, m, torch)
}

fn light(world: &World, assets: &AssetManager, p: Vec2) -> glam::Vec3 {
    light_at_textured(p, &gather_lights(world), |id| assets.get_texture_bytes(id))
}

#[test]
fn a_torch_lights_round_corners_and_not_through_rock() {
    let (mut world, _, _) = world_with_torch(cell(2, 1));
    let mut assets = AssetManager::new();
    tile_light::sync(&mut world, &mut assets);

    let near = light(&world, &assets, cell(3, 2));
    assert!(near.x > 0.4, "beside the torch: {near}");
    assert!(near.x > near.z * 2.0, "in the torch's own colour: {near}");
    // Round the wall: the right-hand room is reached only by the passage
    // along the bottom, so it is lit, and dimmer than the torch's own room.
    let round = light(&world, &assets, cell(9, 1));
    assert!(round.x > 0.01, "round the corner: {round}");
    assert!(round.x < light(&world, &assets, cell(5, 1)).x, "and dimmer for the way round");
    // The wall's face takes light, and it dies three cells into the floor.
    let face = light(&world, &assets, cell(7, 1));
    assert!(face.x > 0.1, "the wall's face: {face}");
    let deep = light(&world, &assets, cell(5, 7));
    assert!(deep.x < face.x * 0.5, "deeper in the rock: {deep} vs {face}");
}

#[test]
fn glowing_tiles_light_and_moving_a_torch_relights() {
    let (mut world, m, torch) = world_with_torch(cell(2, 1));
    world.get::<&mut TileLight>(m).unwrap().emitters =
        vec![TileEmitter { tile: 2, color: [0.0, 0.3, 1.0], intensity: 1.0 }];
    // A pool of glowing tiles in the right room.
    world.get::<&mut Tilemap>(m).unwrap().layers[0].set_tile(13, 4, Some(2));
    let mut assets = AssetManager::new();
    tile_light::sync(&mut world, &mut assets);
    let pool = light(&world, &assets, cell(13, 3));
    assert!(pool.z > 0.5 && pool.z > pool.x, "blue over the pool: {pool}");

    // Carried into the right room, the torch lights it - once the rebuild
    // gap has passed.
    let before = light(&world, &assets, cell(12, 1)).x;
    world.get::<&mut Transform>(torch).unwrap().position = cell(12, 2);
    world.get::<&mut TileLight>(m).unwrap().built_ms = f64::NEG_INFINITY;
    tile_light::sync(&mut world, &mut assets);
    let after = light(&world, &assets, cell(12, 1)).x;
    assert!(after > before + 0.3, "the torch moved: {before} -> {after}");

    // Through the script setter: off, and the room goes dark but the pool.
    tile_light::set_tile_light_field(&world, torch, "on", &serde_json::json!(false)).unwrap();
    world.get::<&mut TileLight>(m).unwrap().built_ms = f64::NEG_INFINITY;
    tile_light::sync(&mut world, &mut assets);
    assert!(light(&world, &assets, cell(12, 1)).x < 0.05);
    tile_light::set_tile_light_field(&world, m, "intensity", &serde_json::json!(2.0)).unwrap();
    assert_eq!(world.get::<&TileLight>(m).unwrap().intensity, 2.0);
    assert!(tile_light::set_tile_light_field(&world, m, "nonsense", &serde_json::json!(1)).is_err());
}

#[test]
fn a_hundred_torches_cost_one_pass() {
    // Max per channel, not a sum: any number of sources is one flood.
    let (w, h) = (200u32, 120u32);
    let solid: Vec<bool> = (0..w * h).map(|i| (i % w) % 17 == 0 && (i / w) % 9 != 0).collect();
    let seeds: Vec<tile_light::Seed> = (0..400).map(|k| ((k * 37) % w, (k * 53) % h, [1.0, 0.7, 0.4])).collect();
    let light = tile_light::flood(w, h, &solid, &seeds, 0.85, 0.4);
    assert_eq!(light.len(), (w * h) as usize);
    for (x, y, _) in &seeds {
        assert!((light[(y * w + x) as usize][0] - 1.0).abs() < 1e-6);
    }
}
