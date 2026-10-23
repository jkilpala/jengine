//! A tilemap under a parent is where its parent carries it - drawn, solid
//! and picked there (Oct 2026).
//!
//! Every reader of a tilemap's place read its local `Transform.position`: the
//! tile pass, the colliders, the ray snapshot, navigation and the scripts'
//! `world_to_tile`. So a minimap tilemap parented to the camera drew at its
//! local offset about the origin, and a map moved by a parent stayed behind
//! while the things on it went. Four Strategy Classics games met it (Rust
//! Dunes' minimap, Six Banners' and Iron Rails' scrolling maps, Iron Front's
//! cell sizes). They all go through `tilemap::world_origin` now.

#![cfg(feature = "native")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{
    CustomComponents, Parent, ScriptComponent, ScriptLanguage, Transform,
};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use jengine_core::tilemap::{Tilemap, TilemapLayer, TilesetDef};

const TILE: f32 = 32.0;
/// The parent's place, and the map's offset from it.
const PARENT: Vec2 = Vec2::new(300.0, 200.0);
const LOCAL: Vec2 = Vec2::new(-40.0, 20.0);

fn at(p: Vec2) -> Transform {
    Transform { position: p, ..Default::default() }
}

/// A 4 x 2 map, every cell gid 1.
fn map() -> Tilemap {
    Tilemap {
        tile_size: [TILE, TILE],
        layers: vec![TilemapLayer {
            name: "ground".into(),
            tiles: vec![1; 8],
            width: 4,
            height: 2,
            visible: true,
            collision: false,
            ..Default::default()
        }],
        tilesets: vec![TilesetDef {
            texture_id: "ground".into(),
            tile_width: 32,
            tile_height: 32,
            columns: 1,
            tile_count: 1,
            first_gid: 1,
            image_width: 32,
            image_height: 32,
            ..Default::default()
        }],
        ..Default::default()
    }
}

/// A parent at `PARENT` and the map under it at `LOCAL`, hierarchy resolved.
fn parented(world: &mut World, extra: impl hecs::DynamicBundle) -> hecs::Entity {
    let parent = world.spawn((at(PARENT),));
    let e = world.spawn(extra);
    world.insert(e, (at(LOCAL), map(), Parent(parent))).unwrap();
    resolve_world_transforms(world);
    e
}

#[test]
fn the_origin_is_the_world_position() {
    let mut world = World::new();
    let e = parented(&mut world, ());
    let t: Transform = (*world.get::<&Transform>(e).unwrap()).clone();
    assert_eq!(jengine_core::tilemap::world_origin(&world, e, &t), PARENT + LOCAL);
    // An unparented map is exactly where it always was.
    let alone = world.spawn((at(LOCAL), map()));
    assert_eq!(jengine_core::tilemap::world_origin(&world, alone, &at(LOCAL)), LOCAL);
}

#[test]
fn a_script_picks_cells_where_the_parent_carries_the_map() {
    const SRC: &str = r#"
function update(entity, dt)
    local wx, wy = Tilemap.tile_to_world(entity, 1, 0)
    Entity.set_custom(entity, "Read", "wx", wx)
    Entity.set_custom(entity, "Read", "wy", wy)
    local tx, ty = Tilemap.world_to_tile(entity, wx, wy)
    Entity.set_custom(entity, "Read", "tx", tx)
    Entity.set_custom(entity, "Read", "ty", ty)
end
"#;
    let dir = std::env::temp_dir().join(format!("jengine_parented_tilemap_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("reader.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut world = World::new();
    let e = parented(
        &mut world,
        (
            CustomComponents::default(),
            ScriptComponent {
                language: ScriptLanguage::Lua,
                source_path: path.to_string_lossy().into(),
                cull_distance: None,
            },
        ),
    );
    let mut system = jengine_core::ecs::systems::ScriptSystem::new().expect("script system");
    let input = jengine_core::input::InputState::new();
    system.run(&mut world, 1.0 / 60.0, &input, None, None);

    let custom = world.get::<&CustomComponents>(e).unwrap();
    let read = custom.0.get("Read").expect("the script wrote its readings");
    let n = |k: &str| read.get(k).and_then(|v| v.as_f64()).unwrap_or(f64::NAN) as f32;
    // Cell (1, 0)'s centre: one and a half tiles right of the world origin,
    // half a tile down.
    let want = PARENT + LOCAL + Vec2::new(1.5 * TILE, -0.5 * TILE);
    assert!((n("wx") - want.x).abs() < 1e-3 && (n("wy") - want.y).abs() < 1e-3, "tile_to_world: {read}");
    assert_eq!((n("tx"), n("ty")), (1.0, 0.0), "world_to_tile round-trips: {read}");
}

#[cfg(feature = "physics")]
#[test]
fn the_colliders_are_where_the_parent_carries_the_map() {
    use jengine_core::ecs::components::TilemapCollider;
    use jengine_core::ecs::systems::ensure_physics_bodies;
    use jengine_core::physics::PhysicsWorld;
    let mut world = World::new();
    parented(&mut world, (TilemapCollider { solid: vec!["ground".into()], one_way: vec![] },));
    let mut physics = PhysicsWorld::new(0.0, 0.0, 100.0);
    ensure_physics_bodies(&world, &mut physics);
    physics.step(1.0 / 60.0);
    // Straight down through the middle of the map's world place: met at its top.
    let origin = PARENT + LOCAL;
    let x = origin.x + 2.0 * TILE;
    let hit = physics.raycast(x, origin.y + 500.0, 0.0, -1.0, 1000.0).expect("the map is solid where it is drawn");
    assert!((hit.point_y - origin.y).abs() < 0.5, "met at the map's top edge, got {}", hit.point_y);
    // Where the map's local offset alone would have put it, nothing.
    assert!(physics.raycast(LOCAL.x + 2.0 * TILE, LOCAL.y + 500.0, 0.0, -1.0, 600.0).is_none());
}

#[cfg(feature = "gpu-wgpu")]
#[test]
fn the_tiles_are_drawn_where_the_parent_carries_the_map() {
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::ecs::wgpu_systems::render_tilemaps_wgpu;
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    const W: u32 = 160;
    const H: u32 = 120;
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("tilemaps_follow_their_parent: no GPU adapter - skipped");
        return;
    };
    let mut assets = AssetManager::new();
    let rgba = [40u8, 160, 40, 255].repeat(32 * 32);
    assets.insert_texture_bytes("ground".into(), RawTextureData { width: 32, height: 32, rgba });
    assets.ensure_wgpu_texture("ground", &gpu.device, &gpu.queue).unwrap();

    // The parent puts the map's corner at (-20, 30); at its local offset
    // alone it would hang at (-40, 20), so x = -30 is map there and not here.
    let mut world = World::new();
    let parent = world.spawn((at(Vec2::new(20.0, 10.0)),));
    world.spawn((at(Vec2::new(-40.0, 20.0)), map(), Parent(parent)));
    resolve_world_transforms(&mut world);

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_tilemaps_wgpu(&world, &SortLayers::default(), &mut batch, &camera, |b, tex| {
            b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture(tex).unwrap());
        });
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let pixel = |x: f32, y: f32| {
        let px = (x + W as f32 / 2.0) as u32;
        let py = (H as f32 / 2.0 - y) as u32;
        let i = ((py * img.width + px) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    // Inside the map at its world place: green.
    let inside = pixel(-20.0 + 70.0, 30.0 - 20.0);
    assert!(inside[1] > 120 && inside[0] < 80, "the map is drawn at its world place: {inside:?}");
    // Left of it, where the local offset alone would have drawn: black.
    let outside = pixel(-30.0, 0.0);
    assert!(outside.iter().all(|c| *c < 16), "nothing is drawn at the local offset: {outside:?}");
}
