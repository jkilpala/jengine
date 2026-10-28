//! **Snow cover** (Oct 2026; `ecs::snow`): snow lying on the upward edges
//! of a sprite's own picture, deepening and melting. Rendered through the
//! real sprite pass on a headless device, twice with the same GPU: a brown
//! block shows a thin white cap over its top and nothing beside it; deepened,
//! the cap reaches higher - which only shows if the remade picture reaches
//! the GPU, since it is remade under the same texture id at the same size.
//! The picture is made through `carve::apply_carves`, the step every host
//! runs with its assets.
//! Found by the showcase's First Snow. Without a GPU adapter it prints why
//! and passes.
#![cfg(all(feature = "gpu-wgpu", feature = "snow"))]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{SpriteComponent, Transform};
use jengine_core::ecs::snow::SnowCover;
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 64;
const H: u32 = 64;

/// The column through the middle, rgb, top to bottom.
fn draw(gpu: &HeadlessGpu, world: &World, assets: &mut AssetManager) -> Vec<[u8; 3]> {
    let (rgba, aw, ah, font) = built_in_font_atlas();
    let mut fa = AssetManager::new();
    fa.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let cam = Camera2D::new(W as f32, H as f32);
    set_frame_view(&cam);
    let vp = cam.view_projection();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, kind| {
                // As a host does: ask, and upload (or patch) what is missing.
                if assets.get_wgpu_texture(tex).is_none() {
                    assets.ensure_wgpu_texture(tex, &gpu.device, &gpu.queue).unwrap();
                }
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                if let SpriteShadowFlush::Sprite { .. } = kind {
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
                }
            },
        );
    }
    gpu.submit(encoder);
    clear_frame_view();
    let img = gpu.read_rgba();
    (0..H).map(|y| {
        let i = ((y * W + W / 2) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    }).collect()
}

fn white(p: [u8; 3]) -> bool {
    p.iter().all(|&c| c > 200)
}

#[test]
fn snow_caps_the_block_and_deepens() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("snow_settles_on_what_faces_up: no GPU adapter - skipped");
        return;
    };
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    // A 64 x 64 picture solid from row 32 down, between columns 8 and 56:
    // drawn 64 x 64, a texel a pixel.
    let mut rgba = Vec::new();
    for y in 0..64u32 {
        for x in 0..64u32 {
            let solid = y >= 32 && (8..56).contains(&x);
            rgba.extend_from_slice(&[110, 70, 40, if solid { 255 } else { 0 }]);
        }
    }
    assets.insert_texture_bytes("block.png".into(), RawTextureData { width: 64, height: 64, rgba });
    let mut world = World::new();
    let block = world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "block.png".into(), size: Some([64.0, 64.0]), ..Default::default() },
        SnowCover { depth: 3.0, bumps: 0.0, step: 1.0, ..Default::default() },
    ));
    jengine_core::ecs::carve::apply_carves(&mut world, &mut assets);
    let thin = draw(&gpu, &world, &mut assets);
    // The block's top is row 32 on screen; snow lies in the rows above it.
    assert!(white(thin[30]), "a cap just over the top: {:?}", thin[30]);
    assert!(!white(thin[24]), "but thin: {:?}", thin[24]);
    assert!(thin[40][0] > 60 && thin[40][2] < 60, "the block below it, brown: {:?}", thin[40]);
    // Deepened: remade under the same id and size.
    world.get::<&mut SnowCover>(block).unwrap().depth = 10.0;
    jengine_core::ecs::carve::apply_carves(&mut world, &mut assets);
    let deep = draw(&gpu, &world, &mut assets);
    assert!(white(deep[24]), "the deeper snow reaches higher - the remade picture reached the GPU: {:?}", deep[24]);
}
