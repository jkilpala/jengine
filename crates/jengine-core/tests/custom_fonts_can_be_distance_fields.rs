//! **A game's own font can be a distance field** (Oct 2026). The built-in
//! face has been one since Sep 2026; a custom `.font` stayed a bitmap, so the
//! arcade's tabletop font - drawn at 64 px for phones - blurred or went
//! blocky wherever a game drew it larger. `"sdf": true` in a `.font` always
//! parsed; nothing could bake the atlas to go with it, and no host would
//! have drawn it as one.
//!
//! `renderer::text::font_to_sdf` (`jengine font-sdf`) turns a bitmap font
//! into a distance field without moving any of its text, and
//! `AssetManager::insert_font_data` marks such a font's atlas so every host
//! draws it through the sprite shader's distance branch. Checked here on a
//! font made in the test - one bar for 'I', its edges half a pixel - so
//! where the ink should be is known exactly.

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::{TextComponent, Transform};
use jengine_core::renderer::text::{FontData, GlyphInfo, SDF_SPREAD, font_to_sdf};

const FONT_ID: &str = "fonts/test.font";
const TEXTURE_ID: &str = "fonts/test.png";

/// A 64 x 64 atlas holding one glyph, 'I': a bar 12 x 48 whose outer
/// columns are half covered, so its edges sit half a pixel in.
fn bitmap_font() -> (FontData, Vec<u8>) {
    let (w, h) = (64u32, 64u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 4..52 {
        for x in 8..20 {
            let i = ((y * w + x) * 4) as usize;
            rgba[i..i + 3].fill(255);
            rgba[i + 3] = if x == 8 || x == 19 { 128 } else { 255 };
        }
    }
    let glyph = GlyphInfo { ch: 'I', x: 8, y: 4, width: 12, height: 48, advance: 16.0, x_offset: 2, y_offset: 8 };
    let space = GlyphInfo { ch: ' ', x: 0, y: 0, width: 0, height: 0, advance: 14.0, x_offset: 0, y_offset: 0 };
    let data = FontData {
        atlas_width: w,
        atlas_height: h,
        line_height: 64.0,
        pixel_size: 64.0,
        atlas_scale: 1.0,
        sdf: false,
        sdf_spread: 0.0,
        glyphs: vec![space, glyph],
        shaping: None,
    };
    (data, rgba)
}

#[test]
fn the_layout_does_not_move() {
    let (font, rgba) = bitmap_font();
    let (sdf, _, _, _) = font_to_sdf(&font, &rgba, 64, 64, 1).unwrap();
    assert!(sdf.sdf);
    assert_eq!(sdf.sdf_spread, SDF_SPREAD);
    assert_eq!((sdf.line_height, sdf.pixel_size, sdf.atlas_scale), (64.0, 64.0, 1.0));
    let pad = SDF_SPREAD as i32;
    for (a, b) in font.glyphs.iter().zip(&sdf.glyphs) {
        assert_eq!(a.ch, b.ch);
        assert_eq!(a.advance, b.advance, "{:?}: an advance moves every glyph after it", a.ch);
        if a.width == 0 {
            assert_eq!((b.width, b.height), (0, 0), "a space stays empty");
            continue;
        }
        assert_eq!(b.width as i32, a.width as i32 + 2 * pad, "the rect grows by the spread on each side");
        assert_eq!((b.x_offset + pad, b.y_offset + pad), (a.x_offset, a.y_offset), "so the ink lands where it did");
    }
}

/// A small font is baked finer than its pixels, so a counter a pixel wide
/// survives - and its rects and offsets are in those finer texels, which the
/// atlas scale says, so the ink still lands where it did.
#[test]
fn a_small_font_is_baked_finer_than_its_pixels() {
    use jengine_core::renderer::text::sdf_upscale_for;
    let (font, rgba) = bitmap_font();
    assert_eq!(sdf_upscale_for(&font), 1, "64 px needs no upscaling");
    let small = FontData { pixel_size: 16.0, ..font.clone() };
    assert_eq!(sdf_upscale_for(&small), 3, "16 px is baked at 48, as the built-in face is");
    let legacy = FontData { pixel_size: 1.0, line_height: 12.0, ..font.clone() };
    assert_eq!(sdf_upscale_for(&legacy), 4, "a legacy font is sized by its line, and never past four");

    let (sdf, _, _, _) = font_to_sdf(&font, &rgba, 64, 64, 3).unwrap();
    assert_eq!(sdf.atlas_scale, 3.0);
    assert_eq!(sdf.line_height, font.line_height, "layout pixels are not scaled");
    let pad = SDF_SPREAD as i32;
    let (a, b) = (&font.glyphs[1], &sdf.glyphs[1]);
    assert_eq!(a.advance, b.advance);
    assert_eq!(b.width as i32, a.width as i32 * 3 + 2 * pad);
    // In layout pixels the ink starts where it did.
    assert_eq!((b.x_offset + pad) as f32 / sdf.atlas_scale, a.x_offset as f32);
    assert_eq!((b.y_offset + pad) as f32 / sdf.atlas_scale, a.y_offset as f32);
}

#[test]
fn a_distance_field_is_not_converted_twice() {
    let (font, rgba) = bitmap_font();
    let (sdf, field, w, h) = font_to_sdf(&font, &rgba, 64, 64, 1).unwrap();
    assert!(font_to_sdf(&sdf, &field, w, h, 1).is_err());
}

#[test]
fn the_field_is_half_on_the_edge() {
    let (font, rgba) = bitmap_font();
    let (sdf, field, w, _) = font_to_sdf(&font, &rgba, 64, 64, 1).unwrap();
    let g = &sdf.glyphs[1];
    let pad = SDF_SPREAD as u32;
    let alpha = |x: u32, y: u32| field[(((g.y + y) * w + g.x + x) * 4 + 3) as usize];
    let mid = g.height / 2;
    // The half-covered column is the edge; a column further in is inside,
    // a spread outside is nothing.
    assert!((alpha(pad, mid) as i32 - 128).abs() <= 8, "edge column {}", alpha(pad, mid));
    assert!(alpha(pad + 3, mid) > 160, "inside {}", alpha(pad + 3, mid));
    assert!(alpha(0, mid) < 40, "a spread out {}", alpha(0, mid));
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 256;
    const H: u32 = 256;

    /// The 'I' drawn at four times its size; the middle row's red channel.
    fn draw(gpu: &HeadlessGpu, font: FontData, rgba: Vec<u8>, w: u32, h: u32) -> (Vec<u8>, bool) {
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes(TEXTURE_ID.into(), RawTextureData { width: w, height: h, rgba });
        assets.insert_font_data(FONT_ID.into(), font);
        let marked = assets.ensure_wgpu_texture(TEXTURE_ID, &gpu.device, &gpu.queue).unwrap().sdf;
        let (brgba, bw, bh, builtin) = built_in_font_atlas();
        assets.insert_texture_bytes("__font".into(), RawTextureData { width: bw, height: bh, rgba: brgba });
        let fonts = FrameFonts::gather(&assets, &builtin, "__font");
        let mut world = World::new();
        world.spawn((
            Transform { position: Vec2::new(-40.0, 100.0), ..Default::default() },
            TextComponent { text: "I".into(), font_id: FONT_ID.into(), font_scale: 4.0, color: [1.0; 4], ..Default::default() },
        ));
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        let vp = Camera2D::new(W as f32, H as f32).view_projection();
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
        let row = (0..W).map(|x| img.rgba[((H / 2 * W + x) * 4) as usize]).collect();
        (row, marked)
    }

    /// (the ink's middle, pixels that are neither ink nor background).
    fn measure(row: &[u8]) -> (f32, usize) {
        let ink: Vec<usize> = (0..row.len()).filter(|&x| row[x] > 128).collect();
        assert!(ink.len() > 20, "the bar is drawn: {row:?}");
        let mid = (ink[0] + ink[ink.len() - 1]) as f32 / 2.0;
        (mid, row.iter().filter(|&&v| v > 30 && v <= 225).count())
    }

    #[test]
    fn a_converted_font_draws_sharp_where_it_drew_soft() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("custom_fonts_can_be_distance_fields: no GPU adapter - skipped");
            return;
        };
        let (font, rgba) = bitmap_font();
        let (bitmap_row, bitmap_marked) = draw(&gpu, font.clone(), rgba.clone(), 64, 64);
        let (sdf, field, w, h) = font_to_sdf(&font, &rgba, 64, 64, 1).unwrap();
        let (sdf_row, sdf_marked) = draw(&gpu, sdf, field, w, h);

        assert!(!bitmap_marked, "a bitmap font's atlas is not a distance field");
        assert!(sdf_marked, "insert_font_data marks a distance-field font's atlas, so no host has to");
        let (bitmap_mid, bitmap_edge) = measure(&bitmap_row);
        let (sdf_mid, sdf_edge) = measure(&sdf_row);
        assert!((bitmap_mid - sdf_mid).abs() <= 1.0, "the ink sits where it did: {bitmap_mid} against {sdf_mid}");
        assert!(sdf_edge <= 4, "both edges together span a few pixels: {sdf_edge} ({sdf_row:?})");
        assert!(bitmap_edge > sdf_edge + 2, "the bitmap's half-covered columns stretch four times: {bitmap_edge}");
        let (fine, field, w, h) = font_to_sdf(&font, &rgba, 64, 64, 3).unwrap();
        let (fine_row, _) = draw(&gpu, fine, field, w, h);
        let (fine_mid, fine_edge) = measure(&fine_row);
        assert!((bitmap_mid - fine_mid).abs() <= 1.0, "baked finer, the ink still sits where it did: {bitmap_mid} against {fine_mid}");
        assert!(fine_edge <= 4, "and stays sharp: {fine_edge}");
    }
}
