//! **Text is sharp at any size** (Sep 2026): the built-in font is a signed
//! distance field baked at 48 px, and the sprite shader draws its edge one
//! screen pixel wide whatever the scale. It was a 16 px coverage bitmap, so
//! a title at ten times that was sixteen-pixel glyphs stretched - an edge
//! a dozen pixels of blur (or of blocks, filtered nearest). Found by the
//! showcase's Atlas of the Deep.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::{TextComponent, Transform};
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::text::built_in_font_atlas;

const HOSTS: [(&str, &str); 3] = [
    ("native", include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs")),
    ("viewport", include_str!("../../jengine-editor-viewport/src/lib.rs")),
    ("web", include_str!("../../jengine-web/src/lib.rs")),
];

#[test]
fn every_runtime_draws_the_builtin_font_as_a_distance_field() {
    for (name, src) in HOSTS {
        assert!(
            src.contains("assets.mark_sdf(BUILTIN_FONT_ID)"),
            "{name} registers the built-in font without marking it a distance field: its text draws as a blurred halo"
        );
    }
}

#[test]
fn the_builtin_font_is_a_distance_field_with_16_px_metrics() {
    let (_, _, _, font) = built_in_font_atlas();
    assert!(font.sdf);
    assert_eq!(font.pixel_size, 16.0, "a UI font_size of 16 is the native size, as before");
    assert!((font.atlas_scale - 3.0).abs() < 1e-6);
    let a = font.glyphs.iter().find(|g| g.ch == 'A').unwrap();
    assert!(a.advance > 8.0 && a.advance < 13.0, "advances are 16 px ones: {}", a.advance);
}

/// A capital I at font scale 12 (a 190 px glyph); how many pixels across its
/// left edge are neither background nor ink.
#[test]
fn a_glyph_ten_times_its_size_has_a_one_pixel_edge() {
    let (w, h) = (256u32, 256u32);
    let Some(gpu) = HeadlessGpu::new(w, h) else {
        eprintln!("text_stays_sharp_scaled: no GPU adapter - skipped");
        return;
    };
    let (rgba, aw, ah, font) = built_in_font_atlas();
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("font".into(), RawTextureData { width: aw, height: ah, rgba });
    assets.mark_sdf("font");
    assets.ensure_wgpu_texture("font", &gpu.device, &gpu.queue).unwrap();
    assert!(assets.get_wgpu_texture("font").unwrap().sdf, "a marked texture uploads as a distance field");
    let fonts = FrameFonts::builtin_only(&font, "font");
    let mut world = World::new();
    world.spawn((
        Transform { position: Vec2::new(0.0, 100.0), ..Default::default() },
        TextComponent {
            text: "I".into(),
            font_scale: 12.0,
            color: [1.0; 4],
            align: jengine_core::ui::TextAlign::Center,
            ..Default::default()
        },
    ));

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let vp = Camera2D::new(w as f32, h as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, id, kind| {
                let t = assets.get_wgpu_texture(id).expect("uploaded");
                if let SpriteShadowFlush::Sprite { .. } = kind {
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
                }
            },
        );
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let row: Vec<u8> = (0..w).map(|x| img.rgba[((h / 2 * w + x) * 4) as usize]).collect();
    let ink = row.iter().filter(|&&v| v > 225).count();
    let edge = row.iter().filter(|&&v| v > 30 && v <= 225).count();
    assert!(ink > 15, "the stem is there: {ink} px of ink");
    assert!(edge <= 4, "both edges together span at most a few pixels: {edge} ({row:?})");
}
