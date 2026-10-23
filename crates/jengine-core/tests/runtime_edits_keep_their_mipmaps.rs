//! A mipmapped texture changed at runtime is patched where it changed - its
//! base level **and** every mip level below - not rebuilt.
//!
//! Before this, `ensure_wgpu_texture` met a dirty rectangle on a texture
//! with mipmaps by throwing the GPU texture away and uploading the whole
//! image again, its mip chain averaged texel by texel in linear light. Rust
//! Bowl paints skid marks into its 884-texel floor nearly every frame, and
//! with the project's linear filtering that rebuild was most of a 190 ms
//! frame in the editor. The CPU half (`update_mip_chain` equals a rebuilt
//! chain byte for byte) is a unit test in `texture_filter`; this is the GPU
//! half: drawn small enough to sample the low mips, an edited texture looks
//! exactly like the same picture uploaded fresh, and is the same texture.
//! Without a GPU adapter it prints why and passes, like `render_golden`.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::{SpriteComponent, Transform};
use jengine_core::ecs::wgpu_systems::{WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::text::built_in_font_atlas;
use jengine_core::renderer::texture_filter::TextureFilterRules;
use serde_json::json;

const W: u32 = 32;
const SIDE: u32 = 128;
const ID: &str = "art/floor.png";

/// A checker of red and green, 8 texels a square.
fn picture() -> Vec<u8> {
    let mut out = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let red = ((x / 8) + (y / 8)) % 2 == 0;
            out.extend_from_slice(if red { &[220, 30, 30, 255] } else { &[30, 200, 40, 255] });
        }
    }
    out
}

/// Paint a blue block over the left half's upper part, as a game would.
fn paint(rgba: &mut [u8]) -> (u32, u32, u32, u32) {
    let (x0, y0, w, h) = (8, 16, 56, 40);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let i = ((y * SIDE + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&[20, 40, 230, 255]);
        }
    }
    (x0, y0, w, h)
}

/// The texture drawn at `size` pixels in a 32 x 32 frame.
fn draw(gpu: &HeadlessGpu, assets: &mut AssetManager, size: f32) -> Vec<u8> {
    if assets.get_wgpu_texture(ID).is_none() {
        assets.ensure_wgpu_texture(ID, &gpu.device, &gpu.queue).unwrap();
    }
    let (_, _, _, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut world = World::new();
    world.spawn((Transform::default(), SpriteComponent { texture_id: ID.into(), size: Some([size, size]), ..Default::default() }));
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let vp = Camera2D::new(W as f32, W as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(W as f32), WorldLayer::All,
            |b, tex, _| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
            },
        );
    }
    gpu.submit(encoder);
    gpu.read_rgba().rgba
}

fn linear() -> TextureFilterRules {
    TextureFilterRules::from_settings(Some(&json!({ "texture_filter": { "default": "linear" } })))
}

#[test]
fn an_edited_texture_patches_every_mip_level_in_place() {
    let Some(gpu) = HeadlessGpu::new(W, W) else {
        eprintln!("runtime_edits_keep_their_mipmaps: no GPU adapter - skipped");
        return;
    };
    let mut assets = AssetManager::new();
    assets.set_texture_filters(linear());
    assets.insert_texture_bytes(ID.into(), RawTextureData { width: SIDE, height: SIDE, rgba: picture() });
    assets.ensure_wgpu_texture(ID, &gpu.device, &gpu.queue).unwrap();
    let before = assets.get_wgpu_texture(ID).unwrap().id;

    // Edit twice - the first edit makes the kept chain, the second patches
    // it - drawing small in between so the old mips are on the GPU.
    let mut edited = picture();
    let small_before = draw(&gpu, &mut assets, 16.0);
    let r = paint(&mut edited);
    assets.texture_bytes_mut(ID).unwrap().rgba.copy_from_slice(&edited);
    assets.mark_texture_dirty(ID, r.0, r.1, r.2, r.3);
    let _ = draw(&gpu, &mut assets, 16.0);
    for y in 90..100u32 {
        for x in 70..120u32 {
            let i = ((y * SIDE + x) * 4) as usize;
            edited[i..i + 4].copy_from_slice(&[240, 240, 240, 255]);
        }
    }
    assets.texture_bytes_mut(ID).unwrap().rgba.copy_from_slice(&edited);
    assets.mark_texture_dirty(ID, 70, 90, 50, 10);
    // At 16 pixels for 128 texels the sampler reads mip 3; at 4, mip 5.
    let small = draw(&gpu, &mut assets, 16.0);
    let tiny = draw(&gpu, &mut assets, 4.0);
    assert_eq!(assets.get_wgpu_texture(ID).unwrap().id, before, "the texture was uploaded again instead of patched");
    assert_ne!(small, small_before, "the low mips still show the picture before the edit");

    // The same picture, uploaded whole.
    let mut fresh = AssetManager::new();
    fresh.set_texture_filters(linear());
    fresh.insert_texture_bytes(ID.into(), RawTextureData { width: SIDE, height: SIDE, rgba: edited });
    assert_eq!(small, draw(&gpu, &mut fresh, 16.0), "patched mips differ from rebuilt ones at 16 px");
    assert_eq!(tiny, draw(&gpu, &mut fresh, 4.0), "patched mips differ from rebuilt ones at 4 px");
}
