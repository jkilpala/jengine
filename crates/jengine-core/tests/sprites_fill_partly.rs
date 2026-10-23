//! **Partly drawn sprites** (Oct 2026; `ecs::sprite_fill`): a `bar` fill
//! draws the first `amount` of its sprite from one side, a `radial` fill the
//! first `amount` of a turn about its middle - a health bar, a cooldown, the
//! warning that fills before a boss strikes. Found by the showcase's Arena
//! Boss, whose telegraphs had nothing to fill.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{FillFrom, SpriteComponent, SpriteFill, Transform};

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

#[test]
fn a_script_sets_how_much_is_drawn() {
    let mut s = SpriteComponent { fill: Some(SpriteFill::Radial { amount: 0.0, start: 0.0, clockwise: true }), ..Default::default() };
    assert!(s.set_fill_amount(0.4));
    assert!(matches!(s.fill, Some(SpriteFill::Radial { amount, .. }) if (amount - 0.4).abs() < 1e-6));
    assert!(s.set_fill_amount(3.0));
    assert!(matches!(s.fill, Some(SpriteFill::Radial { amount, .. }) if amount == 1.0), "clamped to a whole one");
    let mut plain = SpriteComponent::default();
    assert!(!plain.set_fill_amount(0.5), "a plain sprite has nothing to fill");
}

#[test]
fn fills_read_from_a_scene() {
    let bar: SpriteFill = serde_json::from_value(serde_json::json!({ "mode": "bar", "amount": 0.3, "from": "bottom" })).unwrap();
    assert_eq!(bar, SpriteFill::Bar { amount: 0.3, from: FillFrom::Bottom });
    let radial: SpriteFill = serde_json::from_value(serde_json::json!({ "mode": "radial", "amount": 0.5 })).unwrap();
    assert_eq!(
        radial,
        SpriteFill::Radial { amount: 0.5, start: std::f32::consts::FRAC_PI_2, clockwise: true },
        "from the top, clockwise, by default"
    );
}

/// Two white squares on black: a radial fill of a quarter from the top,
/// clockwise, lights only the top-right quarter; a bar fill of a half from
/// the left lights only the left half.
#[cfg(feature = "gpu-wgpu")]
#[test]
fn only_the_filled_part_is_drawn() {
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 160;
    const H: u32 = 80;
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("sprites_fill_partly: no GPU adapter - skipped");
        return;
    };
    let (_r, _w, _h, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
    let mut world = World::new();
    let square = |fill| SpriteComponent { size: Some([60.0, 60.0]), fill: Some(fill), ..Default::default() };
    // Pixel (40, 40) is world (-40, 0); pixel (120, 40) is world (40, 0).
    world.spawn((at(-40.0, 0.0), square(SpriteFill::Radial { amount: 0.25, start: std::f32::consts::FRAC_PI_2, clockwise: true })));
    world.spawn((at(40.0, 0.0), square(SpriteFill::Bar { amount: 0.5, from: FillFrom::Left })));
    jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);
    let camera = Camera2D::new(W as f32, H as f32);
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    batch.begin_frame();
    let mut encoder = gpu.encoder();
    set_frame_view(&camera);
    let vp = camera.view_projection();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        let assets = &assets;
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
    let img = gpu.read_rgba();
    let lit = |x: u32, y: u32| img.rgba[((y * W + x) * 4) as usize] > 128;
    // The radial square spans pixels 10..70, 10..70 (y down).
    assert!(lit(55, 25), "top right of the radial square is drawn");
    assert!(!lit(25, 25), "top left is not");
    assert!(!lit(55, 55), "bottom right is not");
    assert!(!lit(25, 55), "bottom left is not");
    // The bar square spans pixels 90..150.
    assert!(lit(100, 40) && lit(115, 40), "the left half of the bar is drawn");
    assert!(!lit(125, 40) && !lit(140, 40), "the right half is not");
}
