//! **Autotiling** (Oct 2026): a tilemap layer painted in terrains draws its
//! own edges and corners on a dual grid (`tilemap::autotile`). Every tile was
//! placed by hand - a coast, a cliff or a path meant choosing each corner
//! and edge piece, and a script that dug or flooded a cell had to redo its
//! neighbours. Found by the showcase's Mapmaker.
//!
//! The rule is unit-tested in `tilemap::autotile`; this draws a map through
//! the real tilemap pass with a tileset whose sixteen tiles are sixteen
//! colours, so the pixel at each corner says which corner tile was chosen.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::Transform;
use jengine_core::tilemap::autotile::{Autotile, AutotileLayer};
use jengine_core::tilemap::{Tilemap, TilemapLayer, TilesetDef};

/// The colour of tile `k` of the test tileset.
fn colour(k: u32) -> [u8; 3] {
    [(k * 16) as u8, (255 - k * 16) as u8, 128]
}

/// 3 x 3 cells of 32 units, the middle one painted terrain 1, the map's
/// top-left corner at (-48, 48) so it is centred on the origin.
fn map() -> Tilemap {
    let mut tiles = vec![0u32; 9];
    tiles[4] = 1;
    Tilemap {
        tile_size: [32.0, 32.0],
        layers: vec![TilemapLayer {
            name: "ground".into(),
            tiles,
            width: 3,
            height: 3,
            visible: true,
            collision: false,
            ..Default::default()
        }],
        tilesets: vec![TilesetDef {
            texture_id: "tiles".into(),
            tile_width: 16,
            tile_height: 16,
            columns: 4,
            tile_count: 16,
            first_gid: 1,
            image_width: 64,
            image_height: 64,
            ..Default::default()
        }],
        source: None,
        z_order: 0,
        sort_layer: None,
        ..Default::default()
    }
}

#[test]
fn the_component_round_trips_through_a_scene_bundle() {
    let mut world = World::new();
    let rule = Autotile { layers: vec![AutotileLayer { layer: "ground".into(), terrains: vec![1] }] };
    world.spawn((Transform::default(), map(), rule.clone()));
    let scene = jengine_core::scene::format::save_scene(&world);
    assert_eq!(scene.entities[0].components.get::<Autotile>("autotile"), Some(rule.clone()), "saved with the scene");
    // And back.
    let mut loaded = World::new();
    jengine_core::scene::format::load_scene(&scene, &mut loaded);
    let mut q = loaded.query::<&Autotile>();
    assert_eq!(q.iter().next().map(|(_, a)| a.clone()), Some(rule), "and loaded with it");
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::ecs::wgpu_systems::render_tilemaps_wgpu;
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;

    const W: u32 = 128;
    const H: u32 = 128;

    fn draw(gpu: &HeadlessGpu, autotile: bool) -> RawTextureData {
        // Sixteen 16 px tiles, four to a row, tile k all colour(k).
        let mut rgba = vec![0u8; 64 * 64 * 4];
        for y in 0..64u32 {
            for x in 0..64u32 {
                let k = (y / 16) * 4 + x / 16;
                let i = ((y * 64 + x) * 4) as usize;
                rgba[i..i + 3].copy_from_slice(&colour(k));
                rgba[i + 3] = 255;
            }
        }
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes("tiles".into(), RawTextureData { width: 64, height: 64, rgba });
        assets.ensure_wgpu_texture("tiles", &gpu.device, &gpu.queue).unwrap();
        let mut world = World::new();
        let at = Transform { position: Vec2::new(-48.0, 48.0), ..Default::default() };
        if autotile {
            let rule = Autotile { layers: vec![AutotileLayer { layer: "ground".into(), terrains: vec![1] }] };
            world.spawn((at, map(), rule));
        } else {
            world.spawn((at, map()));
        }
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
        gpu.read_rgba()
    }

    fn pixel(image: &RawTextureData, world_x: f32, world_y: f32) -> [u8; 3] {
        let x = (world_x + W as f32 / 2.0) as u32;
        let y = (H as f32 / 2.0 - world_y) as u32;
        let i = ((y * image.width + x) * 4) as usize;
        [image.rgba[i], image.rgba[i + 1], image.rgba[i + 2]]
    }

    fn near(a: [u8; 3], b: [u8; 3]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (*x as i16 - *y as i16).abs() <= 6)
    }

    #[test]
    fn a_painted_cell_draws_its_four_corners() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("tilemaps_autotile_their_terrains: no GPU adapter - skipped");
            return;
        };
        let img = draw(&gpu, true);
        // The painted cell spans (-16..16, -16..16); its corners sit at
        // (+-16, +-16), each the centre of a dual tile 32 wide. Sample each
        // dual tile on the cell's side of its corner.
        for (x, y, mask) in [(-10.0, 10.0, 8), (10.0, 10.0, 4), (-10.0, -10.0, 2), (10.0, -10.0, 1)] {
            let got = pixel(&img, x, y);
            assert!(near(got, colour(mask)), "at ({x}, {y}) the corner tile {mask}: {got:?} not {:?}", colour(mask));
        }
        assert_eq!(pixel(&img, -40.0, 40.0), [0, 0, 0], "no terrain reaches the map's far corner");
    }

    #[test]
    fn without_the_component_the_cell_is_its_own_tile() {
        let Some(gpu) = HeadlessGpu::new(W, H) else { return };
        let img = draw(&gpu, false);
        // Gid 1 is local tile 0, drawn over the whole cell.
        assert!(near(pixel(&img, 0.0, 0.0), colour(0)));
        assert!(near(pixel(&img, -10.0, 10.0), colour(0)));
    }
}
