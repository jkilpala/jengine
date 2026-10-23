//! **Particles drawn by the GPU** (`gpu_particles`, Sep 2026): a stateless
//! cloud - each particle a closed form of its index and the time - drawn in
//! one instanced call at its z in the sprite pass. The CPU particles cost per
//! particle every frame; these cost one uniform, so two hundred thousand is
//! a draw call. `GpuParticles::sample` is the vertex shader on the CPU, and
//! here the GPU's pixels are checked against it. Found by the showcase's
//! Spell Storm.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::Transform;
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::particles::gpu::GpuParticles;
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::material::BlendMode;
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 128;
const H: u32 = 128;
const HOSTS: [(&str, &str); 3] = [
    ("native", include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs")),
    ("viewport", include_str!("../../jengine-editor-viewport/src/lib.rs")),
    ("web", include_str!("../../jengine-web/src/lib.rs")),
];

#[test]
fn every_runtime_feeds_the_sprite_batch_its_clock() {
    // What a graph's `TimeInput` reads. The web never set it, so every
    // animated graph stood still in a web build.
    for (name, src) in HOSTS {
        assert!(src.contains("sprite_batch.set_time("), "{name} never tells the sprite batch the time");
    }
}

/// Draw `g` at the origin; the red channel of every pixel.
fn render(g: GpuParticles) -> Option<Vec<u8>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("gpu_particles_draw: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
    let (rgba, aw, ah, font) = built_in_font_atlas();
    let mut fa = AssetManager::new();
    fa.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut world = World::new();
    world.spawn((Transform::default(), g));
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
                if let SpriteShadowFlush::Sprite { .. } = kind {
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
                }
            },
        );
    }
    gpu.submit(encoder);
    Some(gpu.read_rgba().rgba.chunks(4).map(|p| p[0]).collect())
}

#[test]
fn each_particle_is_drawn_where_the_cpu_says_it_is() {
    let g = GpuParticles {
        count: 3,
        lifetime: [5.0, 5.0],
        speed: [10.0, 20.0],
        swirl: 0.3,
        size: [6.0, 6.0],
        color_start: [1.0; 4],
        color_mid: [1.0; 4],
        color_end: [1.0; 4],
        blend_mode: BlendMode::Alpha,
        time: 1.7,
        ..Default::default()
    };
    let expect: Vec<Vec2> = (0..3).map(|i| g.sample(i, 1.7).0).collect();
    let Some(img) = render(g) else { return };
    for p in expect {
        let (x, y) = ((p.x + W as f32 / 2.0).round() as u32, (H as f32 / 2.0 - p.y).round() as u32);
        assert!(img[(y * W + x) as usize] > 200, "a particle is drawn at {p}");
    }
    let lit = img.iter().filter(|&&v| v > 200).count();
    assert!(lit <= 3 * 64, "and only there: {lit} pixels");
}

#[test]
fn two_hundred_thousand_particles_are_one_draw() {
    let g = GpuParticles {
        count: 200_000,
        lifetime: [2.0, 3.0],
        speed: [10.0, 40.0],
        size: [1.0, 1.0],
        color_start: [1.0; 4],
        color_mid: [1.0; 4],
        color_end: [1.0; 4],
        time: 2.0,
        ..Default::default()
    };
    let Some(img) = render(g) else { return };
    let lit = img.iter().filter(|&&v| v > 100).count();
    assert!(lit > 3000, "a whole cloud: {lit} pixels lit");
}
