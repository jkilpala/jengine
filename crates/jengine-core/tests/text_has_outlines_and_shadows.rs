//! **World text has outlines and shadows** (Oct 2026). A caption over a busy
//! picture needed a dark strip behind it - Mapmaker's hint line had one -
//! because text could only be one colour. `outline` and `shadow` on a
//! `text` draw the glyphs again under themselves in one colour: on a
//! distance-field font (the built-in one) the same glyph with its edge moved
//! out and softened, through `params[0]` of the default sprite shader, so it
//! costs a quad a glyph and is round at the corners. Found by the showcase's
//! Neon Signs.

use jengine_core::ecs::components::{TextComponent, TextOutline, TextShadow};

#[test]
fn the_scene_format_keeps_them_and_plain_text_writes_neither() {
    let t = TextComponent {
        text: "Hi".into(),
        outline: Some(TextOutline { width: 3.0, color: [1.0, 0.0, 0.0, 1.0] }),
        shadow: Some(TextShadow { offset: [4.0, -4.0], color: [0.0, 0.0, 0.0, 0.5], softness: 2.0 }),
        ..Default::default()
    };
    let json = serde_json::to_value(&t).unwrap();
    assert_eq!(json["outline"]["width"], 3.0);
    assert_eq!(json["shadow"]["softness"], 2.0);
    let back: TextComponent = serde_json::from_value(json).unwrap();
    assert_eq!(back.outline, t.outline);
    assert_eq!(back.shadow, t.shadow);
    // Defaults: a black outline, a soft dark shadow down and right.
    let d: TextComponent = serde_json::from_str(r#"{ "text": "x", "outline": { "width": 2 }, "shadow": {} }"#).unwrap();
    assert_eq!(d.outline.unwrap().color, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(d.shadow.unwrap().offset, [2.0, -2.0]);
    let plain = serde_json::to_string(&TextComponent { text: "x".into(), ..Default::default() }).unwrap();
    assert!(!plain.contains("outline") && !plain.contains("shadow"), "{plain}");
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use glam::Vec2;
    use hecs::World;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::ecs::components::Transform;
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 240;
    const H: u32 = 120;

    /// Draws `texts`, each at its position, white on black.
    fn draw(gpu: &HeadlessGpu, texts: &[(Vec2, TextComponent)]) -> RawTextureData {
        let (rgba, aw, ah, font) = built_in_font_atlas();
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
        assets.mark_sdf("__font");
        assets.ensure_wgpu_texture("__font", &gpu.device, &gpu.queue).unwrap();
        let fonts = FrameFonts::builtin_only(&font, "__font");
        let mut world = World::new();
        for (p, t) in texts {
            world.spawn((Transform { position: *p, ..Default::default() }, t.clone()));
        }
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
        gpu.read_rgba()
    }

    fn px(img: &RawTextureData, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * img.width + x) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    }

    /// The box of pixels whose colour passes `is`, in image pixels.
    fn bounds(img: &RawTextureData, x0: u32, x1: u32, is: impl Fn([u8; 3]) -> bool) -> Option<(u32, u32, u32, u32)> {
        let mut b: Option<(u32, u32, u32, u32)> = None;
        for y in 0..img.height {
            for x in x0..x1 {
                if is(px(img, x, y)) {
                    b = Some(match b {
                        None => (x, y, x, y),
                        Some((a, c, d, e)) => (a.min(x), c.min(y), d.max(x), e.max(y)),
                    });
                }
            }
        }
        b
    }

    fn lit(c: [u8; 3]) -> bool {
        c.iter().any(|v| *v > 60)
    }

    fn text(s: &str) -> TextComponent {
        TextComponent { text: s.into(), font_scale: 4.0, ..Default::default() }
    }

    #[test]
    fn an_outline_grows_the_glyph_in_its_colour_and_leaves_its_neighbour_alone() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("text_has_outlines_and_shadows: no GPU adapter - skipped");
            return;
        };
        // Two H's: the left one outlined 4 units in red, the right one plain
        // and drawn after it in the same pass.
        let mut outlined = text("H");
        outlined.outline = Some(TextOutline { width: 4.0, color: [1.0, 0.0, 0.0, 1.0] });
        let left = Vec2::new(-90.0, 30.0);
        let right = Vec2::new(30.0, 30.0);
        let plain = draw(&gpu, &[(left, text("H")), (right, text("H"))]);
        let both = draw(&gpu, &[(left, outlined), (right, text("H"))]);
        let half = W / 2;
        let p = bounds(&plain, 0, half, lit).expect("the plain H draws");
        let o = bounds(&both, 0, half, lit).expect("the outlined H draws");
        let grew = (p.0 as f32 - o.0 as f32, o.2 as f32 - p.2 as f32);
        assert!(
            (2.5..=5.5).contains(&grew.0) && (2.5..=5.5).contains(&grew.1),
            "the outline reaches about 4 pixels past the glyph on each side: {grew:?} ({p:?} -> {o:?})"
        );
        // The rim is red; the middle of the H's stroke is still white.
        let rim = px(&both, o.0 + 1, (o.1 + o.3) / 2);
        assert!(rim[0] > 150 && rim[1] < 80, "the rim is the outline's colour: {rim:?}");
        let stroke = bounds(&plain, 0, half, |c| c[1] > 200).unwrap();
        let mid = px(&both, stroke.0 + 2, (stroke.1 + stroke.3) / 2);
        assert!(mid[1] > 200, "the glyph itself is drawn over its outline: {mid:?}");
        // The plain H to the right is exactly as it was: the edge did not
        // leak into the next text's draw.
        assert_eq!(bounds(&plain, half, W, lit), bounds(&both, half, W, lit), "the neighbour did not grow");
    }

    #[test]
    fn a_shadow_falls_where_it_is_offset_under_the_text() {
        let Some(gpu) = HeadlessGpu::new(W, H) else { return };
        let mut shadowed = text("H");
        shadowed.shadow = Some(TextShadow { offset: [10.0, -10.0], color: [0.0, 0.0, 1.0, 1.0], softness: 0.0 });
        let img = draw(&gpu, &[(Vec2::new(-40.0, 30.0), shadowed.clone())]);
        let white = bounds(&img, 0, W, |c| c[0] > 200 && c[1] > 200).expect("the text");
        let blue = bounds(&img, 0, W, |c| c[2] > 150 && c[0] < 80).expect("the shadow");
        assert!((blue.2 as i32 - white.2 as i32 - 10).abs() <= 1, "10 right: {white:?} vs {blue:?}");
        assert!((blue.3 as i32 - white.3 as i32 - 10).abs() <= 1, "10 down: {white:?} vs {blue:?}");
        // A soft shadow spreads further than a hard one.
        shadowed.shadow.as_mut().unwrap().softness = 3.0;
        let soft = draw(&gpu, &[(Vec2::new(-40.0, 30.0), shadowed)]);
        let any_blue = |c: [u8; 3]| c[2] > 20 && c[0] < c[2];
        let hard_area = bounds(&img, 0, W, any_blue).unwrap();
        let soft_area = bounds(&soft, 0, W, any_blue).unwrap();
        assert!(soft_area.2 > hard_area.2, "softened, it reaches further: {hard_area:?} vs {soft_area:?}");
    }

    /// Fading a text fades its outline and shadow with it (Oct 2026): a
    /// banner cross-fading greetings showed the faded ones' outlines.
    #[test]
    fn a_faded_text_takes_its_outline_and_shadow_with_it() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("text_has_outlines_and_shadows: no GPU adapter - skipped");
            return;
        };
        let mut gone = text("H");
        gone.color = [1.0, 1.0, 1.0, 0.0];
        gone.outline = Some(TextOutline { width: 4.0, color: [1.0, 0.0, 0.0, 1.0] });
        gone.shadow = Some(TextShadow { offset: [6.0, -6.0], color: [0.0, 1.0, 0.0, 1.0], softness: 1.0 });
        let img = draw(&gpu, &[(Vec2::new(-30.0, 30.0), gone)]);
        assert!(bounds(&img, 0, W, lit).is_none(), "a transparent text draws no outline or shadow either");
    }
}
