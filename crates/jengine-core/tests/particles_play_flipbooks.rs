//! **A particle plays a texture sheet** (`flipbook` on a `particle_emitter`,
//! Oct 2026; Unity's Texture Sheet Animation, Godot's particle animation):
//! the emitter's texture cut into frames, each particle showing the one its
//! age (or a frame rate, or a draw at birth) picks. Before it every particle
//! drew the whole texture, so an explosion was a stretched dot and smoke
//! could not billow.
//!
//! A 2 x 1 sheet - red on the left, blue on the right - on a particle that
//! lives a second: it must draw red in its first half and blue in its second,
//! on the GPU. The frame arithmetic is unit-tested here too, and
//! `Particles.set` must reach the new field.
#![cfg(feature = "gpu-wgpu")]

use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::Transform;
use jengine_core::ecs::wgpu_systems::render_particles_wgpu;
use jengine_core::particles::{FlipbookMode, ParticleEmitter, ParticleFlipbook, ParticleSystem, tick_particles};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;

const W: u32 = 32;
const H: u32 = 32;

fn sheet() -> ParticleFlipbook {
    ParticleFlipbook { columns: 2, rows: 1, ..Default::default() }
}

/// The centre pixel of a frame showing one particle, `age` seconds into a
/// one-second life.
fn draw(age: f32) -> Option<[u8; 4]> {
    let gpu = HeadlessGpu::new(W, H)?;
    let mut world = World::new();
    world.spawn((
        Transform::default(),
        ParticleEmitter {
            texture_id: Some("sheet.png".into()),
            emission_rate: 0.0,
            burst_count: 1,
            max_particles: 1,
            lifetime: [1.0, 1.0],
            speed: [0.0, 0.0],
            size_start: [24.0, 24.0],
            size_end: [24.0, 24.0],
            color_end: [1.0, 1.0, 1.0, 1.0],
            flipbook: Some(sheet()),
            ..Default::default()
        },
    ));
    let mut system = ParticleSystem::new();
    tick_particles(&world, &mut system, 0.0);
    let steps = (age / 0.01).round() as usize;
    for _ in 0..steps {
        tick_particles(&world, &mut system, 0.01);
    }
    let mut assets = AssetManager::new();
    // Two texels: red, then blue.
    assets.insert_texture_bytes(
        "sheet.png".into(),
        RawTextureData { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 255, 255] },
    );
    assets.ensure_wgpu_texture("sheet.png", &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_particles_wgpu(&world, &SortLayers::default(), &system, &mut batch, |b, tex| {
            let t = assets.get_wgpu_texture(tex.unwrap_or("sheet.png")).expect("uploaded");
            b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
        });
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba().rgba;
    let i = ((H / 2 * W + W / 2) * 4) as usize;
    Some([img[i], img[i + 1], img[i + 2], img[i + 3]])
}

#[test]
fn a_particle_shows_the_frame_its_age_picks() {
    let Some(young) = draw(0.2) else {
        eprintln!("particles_play_flipbooks: no GPU adapter - skipped");
        return;
    };
    assert!(young[0] > 200 && young[2] < 50, "the first frame (red) early in its life: {young:?}");
    let old = draw(0.8).expect("a GPU a moment ago");
    assert!(old[2] > 200 && old[0] < 50, "the second frame (blue) late in its life: {old:?}");
}

#[test]
fn frames_follow_the_life_the_clock_or_a_draw() {
    let life = ParticleFlipbook { columns: 4, rows: 2, ..Default::default() };
    assert_eq!(life.frame_count(), 8);
    assert_eq!((life.frame(0.0, 2.0, 0.0), life.frame(1.0, 2.0, 0.0), life.frame(1.99, 2.0, 0.0)), (0, 4, 7));
    // Frame 5 is the second column of the second row.
    assert_eq!(life.uv(5), [0.25, 0.5, 0.5, 1.0]);
    let twice = ParticleFlipbook { cycles: 2.0, ..life };
    assert_eq!(twice.frame(1.0, 2.0, 0.0), 0, "half way through two cycles is the first frame again");
    let fps = ParticleFlipbook { mode: FlipbookMode::Fps, fps: 10.0, frames: 6, ..life };
    assert_eq!((fps.frame(0.35, 9.0, 0.0), fps.frame(0.65, 9.0, 0.0)), (3, 0), "ten a second, looping over six");
    let random = ParticleFlipbook { mode: FlipbookMode::Random, ..life };
    assert_eq!((random.frame(0.0, 1.0, 0.5), random.frame(0.9, 1.0, 0.5)), (4, 4), "a drawn frame is held");
    let staggered = ParticleFlipbook { random_start: true, ..life };
    assert_eq!(staggered.frame(1.0, 2.0, 0.75), (6 + 4) % 8, "starts on its own frame and moves on");
}

#[test]
fn particles_set_reaches_the_flipbook() {
    let mut e = ParticleEmitter::default();
    jengine_core::particles::set_emitter_field(&mut e, "flipbook", &serde_json::json!({ "columns": 4, "rows": 4, "mode": "fps", "fps": 24 }))
        .expect("flipbook is an emitter field");
    let fb = e.flipbook.expect("set");
    assert_eq!((fb.columns, fb.rows, fb.mode, fb.fps), (4, 4, FlipbookMode::Fps, 24.0));
    jengine_core::particles::set_emitter_field(&mut e, "flipbook.fps", &serde_json::json!(8)).unwrap();
    assert_eq!(e.flipbook.unwrap().fps, 8.0);
}
