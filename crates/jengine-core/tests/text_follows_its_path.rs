//! **Text follows its path** (Oct 2026; `ecs::text_path`). A `text_path`
//! lays an entity's text along another entity's `vector_path` or `line`,
//! each letter on the curve and turned to it, and `scroll` runs it round like
//! a marquee. The bend itself is unit-tested in the module; this runs it
//! through the world - the path named, its transform, a `line` path, the
//! scroll clock in `tick_motion` and `TextPath.set` - and draws a text along
//! an arch on the GPU to see the letters land on the curve.
#![cfg(all(feature = "text_path", feature = "line2d"))]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{Name, Transform};
use jengine_core::ecs::text_path::{self, TextPath};
use jengine_core::renderer::line2d::Line2D;

#[test]
fn the_path_is_found_by_name_in_world_space_and_the_text_scrolls() {
    let mut world = World::new();
    world.spawn((
        Name("String".into()),
        Transform { position: Vec2::new(100.0, 20.0), ..Default::default() },
        Line2D { points: vec![[0.0, 0.0], [200.0, 0.0]], ..Default::default() },
    ));
    let label = world.spawn((
        Transform::default(),
        TextPath { path: "String".into(), scroll: 30.0, ..Default::default() },
    ));
    let path = text_path::source(&world, "String").expect("a line is a path");
    assert_eq!(path.points[0], Vec2::new(100.0, 20.0), "placed by its entity's transform");
    assert!((path.length() - 200.0).abs() < 1e-4);
    assert!(text_path::source(&world, "Nothing").is_none());

    jengine_core::ecs::behaviors::tick_motion(&mut world, 0.5, None, false);
    assert!((world.get::<&TextPath>(label).unwrap().offset - 15.0).abs() < 1e-4, "a marquee moves on");

    text_path::set_text_path_field(&world, label, "side", &serde_json::json!(12.0)).unwrap();
    assert_eq!(world.get::<&TextPath>(label).unwrap().side, 12.0);
    assert!(text_path::set_text_path_field(&world, label, "nope", &serde_json::json!(1)).is_err());
}

#[cfg(feature = "gpu-wgpu")]
#[test]
fn letters_land_on_an_arch() {
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::ecs::components::{PathCommand, TextComponent, VectorPath};
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::text::built_in_font_atlas;
    const W: u32 = 256;
    const H: u32 = 256;
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("letters_land_on_an_arch: no GPU adapter - skipped");
        return;
    };
    let mut assets = AssetManager::new();
    let (rgba, aw, ah, font) = built_in_font_atlas();
    assets.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    assets.mark_sdf("__font");
    assets.ensure_wgpu_texture("__font", &gpu.device, &gpu.queue).expect("upload");
    let fonts = FrameFonts::builtin_only(&font, "__font");

    let mut world = World::new();
    // An arch over the middle, left to right: half a circle of radius 80
    // centred 40 below the middle of the screen.
    let mut commands = vec![PathCommand::MoveTo { x: -80.0, y: -40.0 }];
    for k in 1..=32 {
        let a = std::f32::consts::PI * (1.0 - k as f32 / 32.0);
        commands.push(PathCommand::LineTo { x: a.cos() * 80.0, y: a.sin() * 80.0 - 40.0 });
    }
    world.spawn((Name("Arch".into()), Transform::default(), VectorPath { commands, ..Default::default() }));
    world.spawn((
        Transform::default(),
        TextComponent { text: "MMMMMMMMMMMMMMMMMMMM".into(), font_scale: 1.2, color: [1.0, 1.0, 1.0, 1.0], ..Default::default() },
        TextPath { path: "Arch".into(), ..Default::default() },
    ));
    jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);

    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        let assets = &assets;
        render_sprites_with_shadows_wgpu_materialized(
            &world,
            &SortLayers::default(),
            &mut batch,
            &fonts,
            Vec2::splat(64.0),
            WorldLayer::All,
            |b, tex, kind| {
                // Only the font is uploaded: the path's own stroke is not what is looked at.
                let Some(texture) = assets.get_wgpu_texture(tex) else { return };
                match kind {
                    SpriteShadowFlush::Shadow => b.flush_multiply(&gpu.device, &gpu.queue, &mut pass, &vp, texture),
                    SpriteShadowFlush::Sprite { .. } => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, texture),
                };
            },
        );
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let lit = |x: f32, y: f32| {
        let (px, py) = ((x + W as f32 / 2.0) as u32, (H as f32 / 2.0 - y) as u32);
        img.rgba[((py * img.width + px) * 4) as usize] > 120
    };
    let near = |cx: f32, cy: f32| (-8..=8).any(|dx| (-8..=8).any(|dy| lit(cx + dx as f32, cy + dy as f32)));
    // Ink near the top of the arch and down its sides, none in the middle
    // under it, where a straight line of text would have been.
    assert!(near(0.0, 40.0), "letters at the top of the arch");
    assert!(near(-72.0, -12.0), "and down its left side");
    assert!(!near(0.0, -40.0), "not along a straight baseline");
}
