//! **Animated tiles** (Oct 2026): a tileset's `animations` make a tile draw
//! as its frames in turn - water, torches, a waterfall - on the map's own
//! clock, which `tick_motion` (every host's shared step) advances. Before it
//! a tile was one picture, and moving water was sprites laid over the map.
//! Found by the showcase's Waterfall Grotto.

use hecs::World;
use jengine_core::ecs::behaviors::tick_motion;
use jengine_core::tilemap::{TileAnimation, TileFrame, Tilemap, TilemapLayer, TilesetDef};

fn water(scatter: bool) -> TilesetDef {
    TilesetDef {
        texture_id: "t.png".into(),
        tile_width: 16,
        tile_height: 16,
        columns: 4,
        tile_count: 4,
        first_gid: 1,
        image_width: 64,
        image_height: 16,
        animations: vec![TileAnimation {
            tile: 0,
            frames: vec![
                TileFrame { tile: 1, duration: 0.25 },
                TileFrame { tile: 2, duration: 0.25 },
                TileFrame { tile: 3, duration: 0.5 },
            ],
            scatter,
        }],
        ..Default::default()
    }
}

#[test]
fn a_tile_shows_its_frames_in_turn_and_loops() {
    let ts = water(false);
    let at = |t: f64| ts.frame_at(0, t, 0, 0);
    assert_eq!((at(0.0), at(0.24), at(0.26), at(0.6), at(1.01)), (1, 1, 2, 3, 1));
    assert_eq!(ts.frame_at(2, 0.3, 0, 0), 2, "a tile with no animation is itself");
    // Without scatter every cell shows the same frame.
    assert!((0..20).all(|c| ts.frame_at(0, 0.3, c, c * 3) == 2));
    // With it they spread across the loop, and each keeps its own pace.
    let sc = water(true);
    let frames: std::collections::BTreeSet<u32> = (0..40).map(|c| sc.frame_at(0, 0.0, c, 7)).collect();
    assert_eq!(frames.len(), 3, "scattered cells start on every frame: {frames:?}");
    assert_eq!(sc.frame_at(0, 0.1, 5, 5), sc.frame_at(0, 1.1, 5, 5), "still a one-second loop per cell");
}

#[test]
fn the_maps_clock_runs_in_the_shared_step() {
    let mut world = World::new();
    let tm = Tilemap {
        tile_size: [16.0, 16.0],
        layers: vec![TilemapLayer { name: "g".into(), tiles: vec![1, 1], width: 2, height: 1, ..Default::default() }],
        tilesets: vec![water(false)],
        ..Default::default()
    };
    let e = world.spawn((tm,));
    for _ in 0..30 {
        tick_motion(&mut world, 1.0 / 60.0, None, false);
    }
    let t = world.get::<&Tilemap>(e).unwrap().anim_time;
    assert!((t - 0.5).abs() < 1e-4, "half a second on its clock: {t}");
    // The cells still name the tile they were given.
    assert_eq!(world.get::<&Tilemap>(e).unwrap().layers[0].get_tile(0, 0), Some(1));
    // And the clock is not part of the scene.
    let json = serde_json::to_value(&*world.get::<&Tilemap>(e).unwrap()).unwrap();
    assert!(json.get("anim_time").is_none());
    assert_eq!(json["tilesets"][0]["animations"][0]["frames"][2]["duration"], 0.5);
}

#[test]
fn tileds_animations_import() {
    let tmj = r#"{ "width": 1, "height": 1, "tilewidth": 16, "tileheight": 16, "layers": [],
        "tilesets": [{ "firstgid": 1, "tilecount": 4, "tilewidth": 16, "tileheight": 16, "columns": 4,
          "image": "t.png", "imagewidth": 64, "imageheight": 16,
          "tiles": [{ "id": 0, "animation": [{ "tileid": 1, "duration": 250 }, { "tileid": 3, "duration": 500 }] }] }] }"#;
    let tm = jengine_core::tilemap::tiled::parse_tiled_json(tmj).unwrap();
    assert_eq!(tm.tilesets[0].animations[0].frames, vec![TileFrame { tile: 1, duration: 0.25 }, TileFrame { tile: 3, duration: 0.5 }]);
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use glam::Vec2;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::components::Transform;
    use jengine_core::ecs::wgpu_systems::render_tilemaps_wgpu;
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;

    /// A tileset of four flat colours: red, green, blue, white.
    fn sheet() -> RawTextureData {
        let colours = [[255u8, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];
        let mut rgba = Vec::new();
        for _ in 0..16 {
            for c in colours {
                for _ in 0..16 {
                    rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
                }
            }
        }
        RawTextureData { width: 64, height: 16, rgba }
    }

    #[test]
    fn the_screen_shows_the_frame_of_the_moment() {
        let Some(gpu) = HeadlessGpu::new(64, 64) else {
            eprintln!("tiles_animate: no GPU adapter - skipped");
            return;
        };
        let mut assets = AssetManager::new();
        for (id, tex) in [(WHITE_TEXTURE_ID, RawTextureData { width: 1, height: 1, rgba: vec![255; 4] }), ("t.png", sheet())] {
            assets.insert_texture_bytes(id.into(), tex);
            assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
        }
        let draw = |time: f64| {
            let mut world = World::new();
            let tm = Tilemap {
                tile_size: [64.0, 64.0],
                layers: vec![TilemapLayer { name: "g".into(), tiles: vec![1], width: 1, height: 1, ..Default::default() }],
                tilesets: vec![water(false)],
                anim_time: time,
                ..Default::default()
            };
            world.spawn((Transform { position: Vec2::new(-32.0, 32.0), ..Default::default() }, tm));
            let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
            let camera = Camera2D::new(64.0, 64.0);
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
            let i = ((32 * 64 + 32) * 4) as usize;
            [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
        };
        assert_eq!(draw(0.1), [0, 255, 0], "the first frame (tile 1, green)");
        assert_eq!(draw(0.3), [0, 0, 255], "the second (tile 2, blue)");
        assert_eq!(draw(0.8), [255, 255, 255], "the third (tile 3, white)");
    }
}
