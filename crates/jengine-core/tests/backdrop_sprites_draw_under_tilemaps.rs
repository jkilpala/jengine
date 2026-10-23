//! A sprite sorted below every tilemap draws beneath the tilemaps.
//!
//! Tilemaps are their own pass, drawn before the sprite pass, so every sprite
//! used to cover every tilemap whatever the z orders said: a parallax sky
//! behind a scrolling tile terrain hid the terrain entirely, and the only
//! advice was "never put a full-screen sprite over a tilemap". Found building
//! the arcade's horizontal shooter, whose terrain is a tilemap with a
//! parallax sky behind it. Rendered through the real pipeline, and read back
//! pixel by pixel. Needs a GPU; without one it prints why and passes.

#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::{SpriteComponent, Transform};
use jengine_core::ecs::wgpu_systems::{backdrop_ceiling, render_sprites_wgpu_multi, render_tilemaps_wgpu};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::tilemap::{Tilemap, TilemapLayer, TilesetDef};

const W: u32 = 256;
const H: u32 = 128;

fn solid(rgba: [u8; 4]) -> RawTextureData {
    RawTextureData { width: 32, height: 32, rgba: rgba.repeat(32 * 32) }
}

fn sprite(texture: &str, z: i32, size: f32) -> SpriteComponent {
    SpriteComponent {
        texture_id: texture.into(),
        z_order: z,
        size: Some([size, size]),
        ..SpriteComponent::default()
    }
}

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Transform::default() }
}

/// One red 32 px tile with its top-left at (-48, 16): it covers x -48..-16.
fn one_tile_map(z: i32) -> Tilemap {
    Tilemap {
        tile_size: [32.0, 32.0],
        layers: vec![TilemapLayer {
            name: "terrain".into(),
            tiles: vec![1],
            width: 1,
            height: 1,
            visible: true,
            collision: false,
            ..Default::default()
        }],
        tilesets: vec![TilesetDef {
            texture_id: "red".into(),
            tile_width: 32,
            tile_height: 32,
            columns: 1,
            tile_count: 1,
            first_gid: 1,
            image_width: 32,
            image_height: 32,
            ..Default::default()
        }],
        source: None,
        z_order: z,
        sort_layer: None,
        ..Default::default()
    }
}

fn pixel(image: &RawTextureData, world_x: f32, world_y: f32) -> [u8; 3] {
    let x = (world_x + W as f32 / 2.0) as u32;
    let y = (H as f32 / 2.0 - world_y) as u32;
    let i = ((y * image.width + x) * 4) as usize;
    [image.rgba[i], image.rgba[i + 1], image.rgba[i + 2]]
}

fn is(c: [u8; 3], want: [u8; 3]) -> bool {
    c.iter().zip(want.iter()).all(|(a, b)| (*a as i16 - *b as i16).abs() <= 8)
}

#[test]
fn a_backdrop_sprite_draws_under_the_tilemap_and_a_foreground_one_over_it() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let (red, green, blue) = ([220, 40, 40], [40, 200, 60], [50, 80, 230]);
    let mut assets = AssetManager::new();
    for (id, c) in [("red", red), ("green", green), ("blue", blue)] {
        assets.insert_texture_bytes(id.into(), solid([c[0], c[1], c[2], 255]));
        assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).expect("upload");
    }

    let mut world = World::new();
    // A sky over the whole view, sorted below the terrain.
    world.spawn((at(0.0, 0.0), sprite("green", -5, 400.0)));
    world.spawn((at(-48.0, 16.0), one_tile_map(0)));
    // A ship over the tile's right half.
    world.spawn((at(-16.0, 0.0), sprite("blue", 5, 16.0)));
    assert_eq!(backdrop_ceiling(&world, &SortLayers::default()), Some(0));

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        let mut flush = |b: &mut WgpuSpriteBatch, tex: &str| {
            let texture = assets.get_wgpu_texture(tex).expect("texture uploaded");
            b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture);
        };
        batch.begin_frame();
        // The order every runtime draws in: tilemaps, then sprites.
        render_tilemaps_wgpu(&world, &SortLayers::default(), &mut batch, &camera, &mut flush);
        render_sprites_wgpu_multi(&world, &SortLayers::default(), &mut batch, Vec2::splat(64.0), &mut flush);
    }
    gpu.submit(encoder);
    let image = gpu.read_rgba();

    assert!(is(pixel(&image, -40.0, 0.0), red), "the tile shows over the sky: {:?}", pixel(&image, -40.0, 0.0));
    assert!(is(pixel(&image, -16.0, 0.0), blue), "the ship shows over the tile: {:?}", pixel(&image, -16.0, 0.0));
    assert!(is(pixel(&image, 60.0, 0.0), green), "the sky shows elsewhere: {:?}", pixel(&image, 60.0, 0.0));
}

#[test]
fn without_a_tilemap_nothing_is_a_backdrop() {
    let mut world = World::new();
    world.spawn((at(0.0, 0.0), sprite("green", -5, 400.0)));
    assert_eq!(backdrop_ceiling(&world, &SortLayers::default()), None);
}
