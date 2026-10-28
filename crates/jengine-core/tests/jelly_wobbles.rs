//! **Jelly** (Oct 2026; `ecs::jelly`): a sprite drawn as a grid of points
//! on springs. Through the real sprite pass on a headless device: a jelly
//! at rest draws exactly as its sprite does, and squashed - its top row
//! pushed down, its middle pushed out - its top edge is lower and its sides
//! wider, the picture stretched over the grid. The springs themselves are
//! unit-tested in `ecs::jelly`. Found by the showcase's Jelly Bounce.
//! Without a GPU adapter it prints why and passes.
#![cfg(all(feature = "gpu-wgpu", feature = "jelly"))]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{SpriteComponent, Transform};
use jengine_core::ecs::jelly::Jelly;
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 64;

/// A 32 x 32 white square at the middle, plain or a jelly with `offsets`;
/// whether each pixel is lit, rows from the top.
fn render(gpu: &HeadlessGpu, jelly: Option<Jelly>) -> Vec<bool> {
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
    let (_r, _w, _h, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut world = World::new();
    let e = world.spawn((Transform::default(), SpriteComponent { size: Some([32.0, 32.0]), ..Default::default() }));
    if let Some(j) = jelly {
        world.insert_one(e, j).unwrap();
    }
    let cam = Camera2D::new(W as f32, W as f32);
    set_frame_view(&cam);
    let vp = cam.view_projection();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, kind| {
                if let SpriteShadowFlush::Sprite { .. } = kind {
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture(tex).unwrap());
                }
            },
        );
    }
    gpu.submit(encoder);
    clear_frame_view();
    gpu.read_rgba().rgba.chunks(4).map(|p| p[0] > 128).collect()
}

fn lit_rows(img: &[bool], x: u32) -> Vec<u32> {
    (0..W).filter(|&y| img[(y * W + x) as usize]).collect()
}

fn lit_cols(img: &[bool], y: u32) -> Vec<u32> {
    (0..W).filter(|&x| img[(y * W + x) as usize]).collect()
}

#[test]
fn a_jelly_draws_as_its_sprite_until_it_is_squashed() {
    let Some(gpu) = HeadlessGpu::new(W, W) else {
        eprintln!("jelly_wobbles: no GPU adapter - skipped");
        return;
    };
    let plain = render(&gpu, None);
    let mut rest = Jelly { cols: 4, rows: 4, ..Default::default() };
    rest.offsets = vec![Vec2::ZERO; 25];
    rest.speeds = vec![Vec2::ZERO; 25];
    assert_eq!(render(&gpu, Some(rest.clone())), plain, "at rest, the sprite exactly");
    // Squashed: the top row 8 down, the middle row's ends 6 out.
    let mut squashed = rest;
    for i in 0..5 {
        squashed.offsets[4 * 5 + i] = Vec2::new(0.0, -8.0);
    }
    squashed.offsets[2 * 5] = Vec2::new(-6.0, 0.0);
    squashed.offsets[2 * 5 + 4] = Vec2::new(6.0, 0.0);
    let img = render(&gpu, Some(squashed));
    let (before, after) = (lit_rows(&plain, 32), lit_rows(&img, 32));
    assert_eq!(after.last(), before.last(), "the bottom stays");
    assert!(after.len() + 6 <= before.len(), "the top came down: {} rows to {}", before.len(), after.len());
    let (wide_before, wide_after) = (lit_cols(&plain, 32), lit_cols(&img, 32));
    assert!(wide_after.len() >= wide_before.len() + 8, "and the middle spread: {} to {}", wide_before.len(), wide_after.len());
}
