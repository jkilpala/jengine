//! **A shape can clip other drawing** (`sprite_mask` + `masked`, Sep 2026):
//! a portal showing another place, a window onto a layer behind, a frame a
//! picture stays inside. The shape rides in the sprite batch's globals and
//! every sprite shader keeps a fragment by it, so the pixels here come from
//! the real collector and the real pipelines. There was no way to clip one
//! sprite by another before. Found by the showcase's Through the Window.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{Masked, Name, SpriteComponent, SpriteMask, Transform};
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::mask::MaskShape;
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 64;
const H: u32 = 64;

/// A white 60 x 60 square masked by a 30-wide ellipse at the centre (or
/// `outside` it, or not at all); the red channel at the centre and at a
/// corner of the square.
fn render(masked: Option<bool>) -> Option<(u8, u8)> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("sprites_can_be_masked: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
    let (rgba, aw, ah, font) = built_in_font_atlas();
    let mut fa = AssetManager::new();
    fa.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    fa.mark_sdf("__font");
    let fonts = FrameFonts::builtin_only(&font, "__font");

    let mut world = World::new();
    world.spawn((
        Transform::default(),
        Name("Portal".into()),
        SpriteMask { shape: MaskShape::Ellipse { size: [30.0, 30.0] }, feather: 0.0 },
    ));
    let square = SpriteComponent { color: [1.0; 4], size: Some([60.0, 60.0]), ..Default::default() };
    match masked {
        Some(outside) => world.spawn((Transform::default(), square, Masked { by: "Portal".into(), outside })),
        None => world.spawn((Transform::default(), square)),
    };

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, kind| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                match kind {
                    SpriteShadowFlush::Shadow => b.flush_multiply(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                    SpriteShadowFlush::Sprite { .. } => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                };
            },
        );
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let px = |x: u32, y: u32| img.rgba[((y * W + x) * 4) as usize];
    Some((px(32, 32), px(8, 8)))
}

#[test]
fn a_masked_square_keeps_only_its_masks_inside() {
    let Some((centre, corner)) = render(None) else { return };
    assert!(centre > 200 && corner > 200, "unmasked, the square fills: {centre} {corner}");
    let Some((centre, corner)) = render(Some(false)) else { return };
    assert!(centre > 200, "inside the ellipse it is kept: {centre}");
    assert!(corner < 20, "its corner, outside, is clipped: {corner}");
    let Some((centre, corner)) = render(Some(true)) else { return };
    assert!(centre < 20 && corner > 200, "an outside mask cuts a hole instead: {centre} {corner}");
}
