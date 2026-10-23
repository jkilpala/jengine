//! A project's `texture_filter` reaches the pixels, and every runtime hands
//! it over.
//!
//! Until Sep 2026 every texture was sampled nearest with no mipmaps - right
//! for pixel art and wrong for everything else: a stretched glow went
//! blocky, a star field drawn small crawled as it moved. Linear filtering
//! blends texels and samples a mip chain; nearest stays exactly what it was.
//! Without a GPU adapter the pixel tests print why and pass, like
//! `render_golden`.
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

const W: u32 = 64;
const H: u32 = 64;

/// Draw one sprite of `texture` (`tw` x `th`) at `size`, under `rules`, and
/// return the middle row's red channel as bytes.
fn middle_row(rules: TextureFilterRules, texture: Vec<u8>, tw: u32, th: u32, size: f32) -> Option<Vec<u8>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("texture_filters_reach_the_screen: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.set_texture_filters(rules);
    assets.insert_texture_bytes("art/t.png".into(), RawTextureData { width: tw, height: th, rgba: texture });
    assets.ensure_wgpu_texture("art/t.png", &gpu.device, &gpu.queue).unwrap();
    let (_, _, _, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut world = World::new();
    world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "art/t.png".into(), size: Some([size, size]), ..Default::default() },
    ));
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, _| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
            },
        );
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    Some((0..W).map(|x| img.rgba[(((H / 2) * W + x) * 4) as usize]).collect())
}

fn rules(default: &str) -> TextureFilterRules {
    TextureFilterRules::from_settings(Some(&json!({ "texture_filter": { "default": default } })))
}

/// Black on the left, white on the right: 2x1 texels.
fn two_texels() -> Vec<u8> {
    vec![0, 0, 0, 255, 255, 255, 255, 255]
}

#[test]
fn a_magnified_texture_is_blocky_nearest_and_smooth_linear() {
    // The two texels stretched over 64 pixels.
    let Some(nearest) = middle_row(TextureFilterRules::default(), two_texels(), 2, 1, 64.0) else { return };
    let grey = |row: &[u8]| row.iter().filter(|&&v| v > 20 && v < 235).count();
    assert_eq!(grey(&nearest), 0, "nearest: every pixel is black or white: {nearest:?}");
    let Some(linear) = middle_row(rules("linear"), two_texels(), 2, 1, 64.0) else { return };
    assert!(grey(&linear) > 16, "linear: a ramp between the two: {linear:?}");
}

#[test]
fn a_texture_drawn_small_averages_with_mipmaps() {
    // A 64x64 one-texel checkerboard drawn 8 pixels wide: nearest picks
    // whichever texel lands under each pixel (all black or all white here);
    // linear reads the mip level that averages them - grey.
    let mut board = Vec::with_capacity(64 * 64 * 4);
    for y in 0..64 {
        for x in 0..64 {
            let v = if (x + y) % 2 == 0 { 255 } else { 0 };
            board.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let Some(linear) = middle_row(rules("linear"), board.clone(), 64, 64, 8.0) else { return };
    let drawn: Vec<u8> = linear[29..35].to_vec();
    assert!(drawn.iter().all(|&v| (150..230).contains(&v)), "linear + mips: grey, got {drawn:?}");
    let Some(nearest) = middle_row(TextureFilterRules::default(), board, 64, 64, 8.0) else { return };
    let drawn: Vec<u8> = nearest[29..35].to_vec();
    assert!(drawn.iter().all(|&v| v < 20 || v > 235), "nearest: no averaging, got {drawn:?}");
}

#[test]
fn an_override_picks_nearest_for_its_prefix() {
    let r = TextureFilterRules::from_settings(Some(&json!({ "texture_filter": {
        "default": "linear", "overrides": { "art/": "nearest" } } })));
    let Some(row) = middle_row(r, two_texels(), 2, 1, 64.0) else { return };
    assert!(row.iter().all(|&v| v < 20 || v > 235), "the override kept it nearest: {row:?}");
}

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");
const EDITOR_VIEWPORT: &str = include_str!("../../../editor/src/lib/Viewport.svelte");
const EDITOR_SETTINGS: &str = include_str!("../../../editor/src-tauri/src/commands/project.rs");

/// Every runtime hands the project's rules to its `AssetManager`, and the
/// editor both pushes them into its viewport and keeps them when it saves.
#[test]
fn every_runtime_hands_over_the_texture_filters() {
    assert!(NATIVE.contains("assets.set_texture_filters(load_project_texture_filters(scene_path))"));
    assert!(WEB.contains("assets.set_texture_filters(texture_filters)") && WEB.contains("texture_filter: jengine_core"));
    assert!(VIEWPORT.contains("app.assets.set_texture_filters(rules)"));
    assert!(EDITOR_VIEWPORT.contains("viewport_set_texture_filters(JSON.stringify(proj.settings))"));
    assert!(
        EDITOR_SETTINGS.contains("pub texture_filter: jengine_core::renderer::texture_filter::TextureFilterRules"),
        "the editor's settings struct drops a field it lacks on the first save"
    );
}
