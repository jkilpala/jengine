//! World text parented to another entity draws where its parent puts it.
//!
//! Sprites were placed by their resolved `WorldTransform`; text was placed by
//! its local `Transform`, as if it had no parent. A card built of a frame and
//! texts (the arcade's deckbuilder) drew its art on the card and its name,
//! cost and description in a heap at the world's origin. Rendered through
//! the real pipeline; needs a GPU, and without one prints why and passes.

#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::{Parent, TextComponent, Transform};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use jengine_core::ecs::wgpu_systems::render_text_wgpu;
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 256;
const H: u32 = 128;

/// Brightest pixel in a column band, as a quick "is there ink here" test.
fn ink(img: &RawTextureData, x0: u32, x1: u32) -> u8 {
    let mut best = 0;
    for y in 0..img.height {
        for x in x0..x1 {
            let i = ((y * img.width + x) * 4) as usize;
            best = best.max(img.rgba[i]);
        }
    }
    best
}

#[test]
fn a_child_text_draws_at_its_parent() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("skipped: no GPU adapter for the headless renderer");
        return;
    };
    let (rgba, aw, ah, font) = built_in_font_atlas();
    const FONT: &str = "__font/builtin";
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(FONT.into(), RawTextureData { width: aw, height: ah, rgba });
    assets.ensure_wgpu_texture(FONT, &gpu.device, &gpu.queue).expect("upload font");
    let fonts = FrameFonts::builtin_only(&font, FONT);

    let mut world = World::new();
    // A parent far to the right; its text sits at the parent's origin.
    let card = world.spawn((Transform { position: Vec2::new(80.0, 0.0), ..Default::default() },));
    world.spawn((
        Transform::default(),
        TextComponent { text: "HIT".into(), font_scale: 1.5, color: [1.0, 1.0, 1.0, 1.0], ..Default::default() },
        Parent(card),
    ));
    resolve_world_transforms(&mut world);

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    batch.begin_frame();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        let assets = &assets;
        render_text_wgpu(&world, &SortLayers::default(), &mut batch, &fonts, |b: &mut WgpuSpriteBatch, tex: &str| {
            b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture(tex).expect("font"));
        });
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    // World x 80 is pixel 208; the origin is pixel 128.
    assert!(ink(&img, 200, 250) > 150, "the text is at its parent (x 80)");
    assert!(ink(&img, 110, 150) < 40, "and not at its own local origin");
}
