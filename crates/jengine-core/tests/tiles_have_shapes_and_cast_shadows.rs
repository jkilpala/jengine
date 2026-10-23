//! **Tiles that are not the whole cell, and tiles that cast shadows** (Oct
//! 2026; Godot's TileSet physics and occlusion layers, Unity's tile physics
//! shapes and TilemapShadowCaster).
//!
//! Every tile of a colliding layer used to be a whole box, and no tile cast
//! a shadow: a slope was a staircase, a ledge a full block, and a lit cave
//! had to be outlined again by hand in `light_occluder`s. A tileset names
//! shapes for the tiles that need them (`TilesetDef::shapes`), and a
//! `tilemap_occluder` names the layers whose tiles cast shadows - merged into
//! a few long outlines first. The merge is unit-tested in `tilemap::shapes`;
//! this runs the shapes through the physics sync, the browsers' ray snapshot,
//! `gather_lights` and the CPU light, which the GPU is pinned to by
//! `cpu_light_matches_the_shader`. Found by the showcase's Lamplit Mine.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{AmbientLight, PointLight2D, TilemapCollider, TilemapOccluder, Transform};
use jengine_core::renderer::lighting::{gather_lights, light_at};
use jengine_core::tilemap::{TileShape, Tilemap, TilemapLayer, TilesetDef};

const T: f32 = 32.0;

/// A map from rows of characters, the tilemap's top-left at the origin (so
/// cell `(c, r)` spans x `c*32..c*32+32` and y `-r*32-32..-r*32`).
/// `#` rock, `/` a slope rising to the right, `_` a half-height ledge,
/// `~` grass you walk through, `o` glass: solid, casting nothing.
fn map(rows: &[&str]) -> Tilemap {
    let (w, h) = (rows[0].len() as u32, rows.len() as u32);
    let mut layer = TilemapLayer { name: "rock".into(), width: w, height: h, visible: true, ..Default::default() };
    layer.tiles = rows
        .iter()
        .flat_map(|r| r.chars())
        .map(|ch| match ch {
            '#' => 1,
            '/' => 2,
            '_' => 3,
            '~' => 4,
            'o' => 5,
            _ => 0,
        })
        .collect();
    layer.touch_all();
    let px = |pts: &[(f32, f32)]| Some(pts.iter().map(|&(x, y)| [x, y]).collect::<Vec<_>>());
    Tilemap {
        tile_size: [T, T],
        layers: vec![layer],
        tilesets: vec![TilesetDef {
            texture_id: "rock.png".into(),
            tile_width: 32,
            tile_height: 32,
            columns: 5,
            tile_count: 5,
            first_gid: 1,
            image_width: 160,
            image_height: 32,
            shapes: vec![
                TileShape { tile: 1, collision: px(&[(0.0, 32.0), (32.0, 32.0), (32.0, 0.0)]), occluder: None },
                TileShape { tile: 2, collision: px(&[(0.0, 16.0), (32.0, 16.0), (32.0, 32.0), (0.0, 32.0)]), occluder: None },
                TileShape { tile: 3, collision: Some(vec![]), occluder: None },
                TileShape { tile: 4, collision: None, occluder: Some(vec![]) },
            ],
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[cfg(feature = "physics")]
mod collision {
    use super::*;
    use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D};
    use jengine_core::ecs::systems::ensure_physics_bodies;
    use jengine_core::physics::PhysicsWorld;
    use jengine_core::physics::ray::RaySnapshot;

    fn solid(tm: Tilemap) -> World {
        let mut world = World::new();
        let spec = TilemapCollider { solid: vec!["rock".into()], one_way: vec![] };
        world.spawn((Transform::default(), tm, spec));
        world
    }

    /// Where a ray straight down at `x` first meets the map, in the live
    /// physics world and in the browsers' snapshot - which must agree.
    fn ground_at(world: &World, physics: &PhysicsWorld, x: f32) -> Option<f32> {
        let live = physics.raycast(x, 200.0, 0.0, -1.0, 1000.0).map(|h| h.point_y);
        let mut snap = RaySnapshot::new();
        snap.refresh(world);
        let snapped = snap.cast(x, 200.0, 0.0, -1.0, 1000.0).map(|h| h.point_y);
        match (live, snapped) {
            (Some(a), Some(b)) => assert!((a - b).abs() < 0.01, "physics says {a}, the snapshot {b}, at x {x}"),
            (a, b) => assert_eq!(a.is_some(), b.is_some(), "physics and the snapshot disagree at x {x}"),
        }
        live
    }

    #[test]
    fn a_slope_a_ledge_and_grass_collide_as_their_shapes() {
        let world = solid(map(&["/_~#"]));
        let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
        ensure_physics_bodies(&world, &mut physics);
        physics.step(1.0 / 60.0);
        // The slope: three quarters across, three quarters of the way up.
        let y = ground_at(&world, &physics, 24.0).expect("the slope is solid");
        assert!((y - -8.0).abs() < 0.5, "the slope at x 24 is y -8 (a box would be 0), got {y}");
        // The ledge: half a tile tall, all the way across.
        let y = ground_at(&world, &physics, 48.0).expect("the ledge is solid");
        assert!((y - -16.0).abs() < 0.5, "the ledge's top is half way down, got {y}");
        // Grass: walked through.
        assert_eq!(ground_at(&world, &physics, 80.0), None, "grass is not solid");
        // And a whole tile is still a whole tile.
        let y = ground_at(&world, &physics, 112.0).expect("rock");
        assert!(y.abs() < 0.5, "rock's top is the cell's top, got {y}");
    }

    #[test]
    fn a_ball_rolls_down_a_slope() {
        let mut world = solid(map(&["......", "../###"]));
        let ball = world.spawn((
            Transform { position: Vec2::new(80.0, -24.0), ..Default::default() },
            RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
            Collider2D { shape: ColliderShape::Circle { radius: 6.0 }, ..Default::default() },
        ));
        let mut physics = PhysicsWorld::new(0.0, -981.0, 100.0);
        for _ in 0..90 {
            ensure_physics_bodies(&world, &mut physics);
            physics.step(1.0 / 60.0);
            physics.sync_transforms_to_ecs(&mut world);
        }
        let at = world.get::<&Transform>(ball).unwrap().position;
        assert!(at.x < 64.0, "it rolled off the slope to the left, at {at:?}");
        assert!(at.y < -60.0, "and down to the floor of the map's bottom row, at {at:?}");
    }
}

fn lit(rows: &[&str], light: Vec2) -> World {
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.0 },));
    world.spawn((
        Transform::default(),
        map(rows),
        TilemapOccluder { layers: vec!["rock".into()], ..Default::default() },
    ));
    world.spawn((
        Transform { position: light, ..Default::default() },
        PointLight2D { intensity: 1.0, radius: 1200.0, falloff: 0.0, ..Default::default() },
    ));
    world
}

#[test]
fn a_cave_is_lit_inside_and_its_rock_shadows_the_world_beyond() {
    // A hollow of two cells in a block of rock, a lamp in the middle of it.
    let world = lit(&["####", "#..#", "####"], Vec2::new(64.0, -48.0));
    let lights = gather_lights(&world);
    assert_eq!(lights.occluders.len(), 8, "the block's four edges and the hollow's four, not 40: {:?}", lights.occluders);
    let at = |x: f32, y: f32| light_at(Vec2::new(x, y), &lights).x;
    assert!(at(40.0, -50.0) > 0.5, "the cave is lit");
    // The rock face the lamp sees: inside the rock, but on the lamp's side
    // of it - lit, as a `light_occluder` without self_shadow is.
    assert!(at(64.0, -28.0) > 0.5, "the cave's ceiling is lit: {}", at(64.0, -28.0));
    // Beyond the rock: in its shadow.
    assert!(at(64.0, 60.0) < 0.05, "above the map is dark: {}", at(64.0, 60.0));
    assert!(at(200.0, -48.0) < 0.05, "beyond the right wall is dark");
}

#[test]
fn glass_lets_the_light_through_and_an_edit_reaches_the_shadow() {
    // The lamp is under a roof; part of the roof is glass.
    let mut world = lit(&["#o#", "..."], Vec2::new(48.0, -48.0));
    let at = |w: &World, x: f32, y: f32| light_at(Vec2::new(x, y), &gather_lights(w)).x;
    assert!(at(&world, 48.0, 60.0) > 0.5, "straight up through the glass is lit");
    assert!(at(&world, 16.0, 30.0) < 0.05, "up through the rock is dark");
    // Break the left rock: the outline is merged again and the light gets out.
    for (_, tm) in world.query_mut::<&mut Tilemap>() {
        tm.layers[0].set_tile(0, 0, None);
    }
    assert!(at(&world, 16.0, 30.0) > 0.5, "the broken rock casts nothing");
}

#[test]
fn a_tiled_maps_tile_collision_objects_become_shapes() {
    let json = r#"{
        "width": 2, "height": 1, "tilewidth": 32, "tileheight": 32, "orientation": "orthogonal",
        "layers": [{ "type": "tilelayer", "name": "ground", "width": 2, "height": 1, "data": [1, 2] }],
        "tilesets": [{
            "firstgid": 1, "tilecount": 3, "tilewidth": 32, "tileheight": 32, "columns": 3,
            "image": "tiles.png", "imagewidth": 96, "imageheight": 32,
            "tiles": [
                { "id": 1, "objectgroup": { "objects": [{ "x": 0, "y": 16, "width": 32, "height": 16 }] } },
                { "id": 2, "objectgroup": { "objects": [{ "x": 0, "y": 32,
                    "polygon": [{ "x": 0, "y": 0 }, { "x": 32, "y": 0 }, { "x": 32, "y": -32 }] }] } }
            ]
        }]
    }"#;
    let tm = jengine_core::tilemap::tiled::parse_tiled_json(json).expect("parses");
    let shapes = &tm.tilesets[0].shapes;
    assert_eq!(shapes.len(), 2, "{shapes:?}");
    assert_eq!(shapes[0].collision, Some(vec![[0.0, 16.0], [32.0, 16.0], [32.0, 32.0], [0.0, 32.0]]));
    assert_eq!(shapes[1].collision, Some(vec![[0.0, 32.0], [32.0, 32.0], [32.0, 0.0]]), "a polygon is relative to its object");
}

#[test]
fn the_scene_keeps_the_shapes_and_the_occluder() {
    let world = lit(&["/#"], Vec2::ZERO);
    let scene = jengine_core::scene::format::save_scene(&world);
    let json = serde_json::to_string(&scene).unwrap();
    let back: jengine_core::scene::format::SceneData = serde_json::from_str(&json).unwrap();
    let mut loaded = World::new();
    jengine_core::scene::format::load_scene(&back, &mut loaded);
    let mut q = loaded.query::<(&Tilemap, &TilemapOccluder)>();
    let (_, (tm, occ)) = q.iter().next().expect("both survive a save");
    assert_eq!(tm.tilesets[0].shapes.len(), 4);
    assert_eq!(occ.layers, vec!["rock".to_string()]);
}
