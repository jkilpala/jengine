//! **Lightning** (Oct 2026; `ecs::lightning`): a branching bolt made afresh
//! at every strike that lights the world while it flashes. Two halves: the
//! light, read by `gather_lights` - so `light_at` and the GPU agree - lights
//! the ground by the strike and leaves it dark once the bolt has faded; and
//! the bolt itself, rendered through the real sprite pass on a headless
//! device, is a bright line from the cloud to the ground with the sky either
//! side of it left dark. Found by the showcase's Thunderhead. The pixel half
//! passes with a note without a GPU adapter.
#![cfg(feature = "lightning")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{AmbientLight, Transform};
use jengine_core::ecs::lightning::{Lightning, strike, tick_lightning};
use jengine_core::renderer::lighting::{gather_lights, has_lights, light_at};

fn storm() -> (World, hecs::Entity) {
    let mut world = World::new();
    world.spawn((AmbientLight { color: [0.2, 0.25, 0.4], intensity: 0.15 },));
    let cloud = world.spawn((
        Transform { position: Vec2::new(0.0, 300.0), ..Default::default() },
        Lightning { branches: 2, duration: 0.5, light_intensity: 2.0, light_radius: 400.0, flash: 0.4, ..Default::default() },
    ));
    (world, cloud)
}

#[test]
fn a_strike_lights_the_ground_and_the_dark_comes_back() {
    let (mut world, cloud) = storm();
    assert!(has_lights(&world));
    let ground = Vec2::new(60.0, -200.0);
    let before = light_at(ground, &gather_lights(&world));
    strike(&mut world, cloud, ground.x, ground.y).unwrap();
    tick_lightning(&mut world, 0.0);
    let during = light_at(ground, &gather_lights(&world));
    assert!(during.x > before.x + 1.0, "lit by the strike: {before} -> {during}");
    // The flash lifts the ambient light too, far from the bolt.
    let far = Vec2::new(-2000.0, 0.0);
    assert!(light_at(far, &gather_lights(&world)).x > light_at(far, &{
        let mut quiet = gather_lights(&world);
        quiet.point_lights.clear();
        quiet.ambient.intensity = 0.15;
        quiet
    }).x, "the whole sky flashes");
    for _ in 0..40 {
        tick_lightning(&mut world, 0.016);
    }
    let after = light_at(ground, &gather_lights(&world));
    assert!((after - before).length() < 1e-5, "dark again once it has faded: {before} vs {after}");
}

#[cfg(feature = "gpu-wgpu")]
mod pixels {
    use super::*;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 64;
    const H: u32 = 64;

    #[test]
    fn the_bolt_is_a_bright_line_with_dark_either_side() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("lightning_strikes_and_lights: no GPU adapter - skipped");
            return;
        };
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
        assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
        let (rgba, aw, ah, font) = built_in_font_atlas();
        let mut fa = AssetManager::new();
        fa.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
        let fonts = FrameFonts::builtin_only(&font, "__font");
        // A straight bolt down the middle, no forks: from the top of the
        // view to the bottom.
        let mut world = World::new();
        let cloud = world.spawn((
            Transform { position: Vec2::new(0.0, 32.0), ..Default::default() },
            Lightning { branches: 0, jag: 0.0, width: 2.0, glow: 3.0, ..Default::default() },
        ));
        strike(&mut world, cloud, 0.0, -32.0).unwrap();
        tick_lightning(&mut world, 0.0);
        let cam = Camera2D::new(W as f32, H as f32);
        set_frame_view(&cam);
        let vp = cam.view_projection();
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        let mut encoder = gpu.encoder();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            batch.begin_frame();
            render_sprites_with_shadows_wgpu_materialized(
                &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                |b, tex, kind| {
                    let t = assets.get_wgpu_texture(tex).expect("uploaded");
                    if let SpriteShadowFlush::Sprite { .. } = kind {
                        b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
                    }
                },
            );
        }
        gpu.submit(encoder);
        clear_frame_view();
        let img = gpu.read_rgba();
        let at = |x: u32, y: u32| img.rgba[((y * W + x) * 4) as usize] as i32;
        // Near the top, where the bolt is widest: the core white, the glow
        // dimmer beside it, the sky dark.
        let y = 8;
        assert!(at(32, y) > 200, "the core: {}", at(32, y));
        assert!(at(34, y) > 20 && at(34, y) < at(32, y), "the glow beside it: {}", at(34, y));
        assert!(at(8, y) < 10 && at(56, y) < 10, "the sky either side: {} {}", at(8, y), at(56, y));
    }
}
