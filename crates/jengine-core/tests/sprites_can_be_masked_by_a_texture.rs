//! **A mask can be a texture's alpha** (Oct 2026; Unity's `SpriteMask`). A
//! `sprite_mask` was a rectangle, an ellipse or a polygon of sixteen points,
//! so a keyhole, a splash of ink or a torn edge could not clip anything - a
//! painted silhouette needs the texture bound in every masked draw, and the
//! sprite flush bound one texture.
//!
//! `MaskShape::Texture` reads a texture's alpha over a quad. Every sprite
//! flush now binds a second texture (a white texel when there is no texture
//! mask), and the collector fetches the mask's through the host's own flush
//! callback - `WgpuSpriteBatch::request_mask_texture` - so no host changed.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{Masked, Name, SpriteComponent, SpriteMask, Transform, WorldTransform};
use jengine_core::renderer::mask::{MaskShape, coverage_sampled, pack};

/// The left half of the texture is opaque, the right transparent.
fn left_half(uv: Vec2) -> f32 {
    if uv.x < 0.5 { 1.0 } else { 0.0 }
}

fn shape(texture: &str, cutoff: f32) -> MaskShape {
    MaskShape::Texture { texture_id: texture.into(), size: [40.0, 40.0], uv_rect: [0.0, 0.0, 1.0, 1.0], alpha_cutoff: cutoff }
}

#[test]
fn a_texture_mask_keeps_where_its_alpha_is_opaque() {
    let at = WorldTransform { position: Vec2::new(10.0, 0.0), ..Default::default() };
    let m = pack(&shape("masks/half.png", 0.0), &at, false, 0.0);
    assert_eq!(coverage_sampled(&m, Vec2::new(0.0, 0.0), left_half), 1.0, "left of the middle: opaque");
    assert_eq!(coverage_sampled(&m, Vec2::new(20.0, 0.0), left_half), 0.0, "right of it: clear");
    assert_eq!(coverage_sampled(&m, Vec2::new(-20.0, 0.0), left_half), 0.0, "off the quad is clipped");
    let out = pack(&shape("masks/half.png", 0.0), &at, true, 0.0);
    assert_eq!(coverage_sampled(&out, Vec2::new(0.0, 0.0), left_half), 0.0, "an outside mask cuts the silhouette out");
    assert_eq!(coverage_sampled(&out, Vec2::new(20.0, 0.0), left_half), 1.0);
}

#[test]
fn a_cutoff_makes_a_soft_alpha_hard() {
    let at = WorldTransform::default();
    let soft = |_: Vec2| 0.4;
    let m = pack(&shape("masks/soft.png", 0.0), &at, false, 0.0);
    assert!((coverage_sampled(&m, Vec2::ZERO, soft) - 0.4).abs() < 1e-6, "no cutoff: the alpha is the coverage");
    let hard = pack(&shape("masks/soft.png", 0.5), &at, false, 0.0);
    assert_eq!(coverage_sampled(&hard, Vec2::ZERO, soft), 0.0, "under the cutoff: gone");
    let low = pack(&shape("masks/soft.png", 0.3), &at, false, 0.0);
    assert_eq!(coverage_sampled(&low, Vec2::ZERO, soft), 1.0, "over it: whole");
}

#[test]
fn a_mask_texture_is_preloaded_with_the_scene() {
    let mut world = World::new();
    world.spawn((Transform::default(), SpriteMask { shape: shape("masks/keyhole.png", 0.0), feather: 0.0 }));
    assert!(jengine_core::scene::format::world_texture_ids(&world).contains("masks/keyhole.png"));
}

#[test]
fn a_mask_texture_round_trips_through_json() {
    let json = r#"{ "shape": { "Texture": { "texture_id": "masks/ink.png", "size": [64, 32] } } }"#;
    let m: SpriteMask = serde_json::from_str(json).unwrap();
    assert_eq!(m.shape.texture_id(), Some("masks/ink.png"));
    let MaskShape::Texture { uv_rect, alpha_cutoff, .. } = m.shape else { panic!() };
    assert_eq!((uv_rect, alpha_cutoff), ([0.0, 0.0, 1.0, 1.0], 0.0), "the whole texture, soft, unless said");
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 64;
    const H: u32 = 64;

    /// A white 60 x 60 square masked by `mask` (a 40 x 40 texture mask at the
    /// centre, whose texture is `texture` - left half opaque - unless it is
    /// not loaded); the red channel left and right of the centre, and at a
    /// corner.
    fn render(gpu: &HeadlessGpu, texture: &str, outside: bool) -> (u8, u8, u8) {
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
        let mut half = vec![255u8; 8 * 8 * 4];
        for y in 0..8 {
            for x in 4..8 {
                half[(y * 8 + x) * 4 + 3] = 0;
            }
        }
        assets.insert_texture_bytes("masks/half.png".into(), RawTextureData { width: 8, height: 8, rgba: half });
        for id in [WHITE_TEXTURE_ID, "masks/half.png"] {
            assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
        }
        let (rgba, aw, ah, font) = built_in_font_atlas();
        let mut fa = AssetManager::new();
        fa.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
        fa.mark_sdf("__font");
        let fonts = FrameFonts::builtin_only(&font, "__font");

        let mut world = World::new();
        world.spawn((Transform::default(), Name("Ink".into()), SpriteMask { shape: shape(texture, 0.0), feather: 0.0 }));
        world.spawn((
            Transform::default(),
            SpriteComponent { color: [1.0; 4], size: Some([60.0, 60.0]), ..Default::default() },
            Masked { by: "Ink".into(), outside },
        ));

        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        let vp = Camera2D::new(W as f32, H as f32).view_projection();
        let mut encoder = gpu.encoder();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            batch.begin_frame();
            render_sprites_with_shadows_wgpu_materialized(
                &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                |b, tex, kind| {
                    // As every host does: a texture not loaded drops the flush.
                    let Some(t) = assets.get_wgpu_texture(tex) else {
                        b.discard_pending();
                        return;
                    };
                    if let SpriteShadowFlush::Sprite { .. } = kind {
                        b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
                    }
                },
            );
        }
        gpu.submit(encoder);
        let img = gpu.read_rgba();
        let px = |x: u32, y: u32| img.rgba[((y * W + x) * 4) as usize];
        (px(24, 32), px(40, 32), px(4, 4))
    }

    #[test]
    fn the_gpu_clips_to_the_textures_alpha() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("sprites_can_be_masked_by_a_texture: no GPU adapter - skipped");
            return;
        };
        let (left, right, corner) = render(&gpu, "masks/half.png", false);
        assert!(left > 200, "the opaque half keeps the square: {left}");
        assert!(right < 20, "the clear half clips it: {right}");
        assert!(corner < 20, "and so does everything off the mask's quad: {corner}");
        let (left, right, corner) = render(&gpu, "masks/half.png", true);
        assert!(left < 20 && right > 200 && corner > 200, "outside, the silhouette is the hole: {left} {right} {corner}");
    }

    #[test]
    fn a_mask_texture_not_loaded_yet_does_not_swallow_the_draw() {
        let Some(gpu) = HeadlessGpu::new(W, H) else { return };
        // The host drops the request's flush; the square's own flush must
        // still draw, clipped to the mask's quad by a blank (white) texel.
        let (left, right, corner) = render(&gpu, "masks/missing.png", false);
        assert!(left > 200 && right > 200, "the square is drawn: {left} {right}");
        assert!(corner < 20, "within the mask's quad: {corner}");
    }
}
