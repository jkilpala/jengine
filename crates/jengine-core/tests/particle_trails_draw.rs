//! **Particle trails** (`trail` on a `particle_emitter`, Oct 2026; Godot's
//! particle trails, Unity's Trails module): each particle draws a ribbon
//! through where it has been, under itself. Before it a particle could only
//! be stretched along its velocity (`stretch`), which is a straight streak
//! and no record of a curve.
//!
//! One particle flies right across a 128 x 32 view, one world unit a pixel;
//! its trail must cover the pixels it has passed and not those ahead of it.
//! The recording and the ribbon's shape are unit-tested in `particles`.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::Transform;
use jengine_core::ecs::wgpu_systems::render_particles_wgpu;
use jengine_core::particles::{ParticleEmitter, ParticleSystem, ParticleTrail, tick_particles};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;

const W: u32 = 128;
const H: u32 = 32;

fn draw(trail: Option<ParticleTrail>) -> Option<Vec<u8>> {
    let gpu = HeadlessGpu::new(W, H)?;
    let mut world = World::new();
    world.spawn((
        Transform { position: Vec2::new(-56.0, 0.0), ..Default::default() },
        ParticleEmitter {
            texture_id: Some("white.png".into()),
            emission_rate: 0.0,
            burst_count: 1,
            max_particles: 1,
            lifetime: [10.0, 10.0],
            speed: [80.0, 80.0],
            angle: [0.0, 0.0],
            size_start: [6.0, 6.0],
            size_end: [6.0, 6.0],
            trail,
            ..Default::default()
        },
    ));
    let mut system = ParticleSystem::new();
    for _ in 0..60 {
        tick_particles(&world, &mut system, 1.0 / 60.0);
    }
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white.png".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("white.png", &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let vp = Camera2D::new(W as f32, H as f32).view_projection();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_particles_wgpu(&world, &SortLayers::default(), &system, &mut batch, |b, tex| {
            let t = assets.get_wgpu_texture(tex.unwrap_or("white.png")).expect("uploaded");
            b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
        });
    }
    gpu.submit(encoder);
    Some(gpu.read_rgba().rgba.chunks(4).map(|p| p[0]).collect())
}

/// The brightest red along the middle row between world x `from` and `to`.
fn row_max(img: &[u8], from: i32, to: i32) -> u8 {
    let y = H / 2;
    (from..to).map(|x| img[(y * W + (x + W as i32 / 2) as u32) as usize]).max().unwrap_or(0)
}

#[test]
fn a_trail_covers_where_the_particle_has_been() {
    let Some(plain) = draw(None) else {
        eprintln!("particle_trails_draw: no GPU adapter - skipped");
        return;
    };
    // After a second at 80 units a second the particle is at x = 24.
    assert!(row_max(&plain, 20, 28) > 200, "the particle is where it flew to");
    assert!(row_max(&plain, -10, 10) < 10, "and without a trail nothing is behind it");
    let trail = draw(Some(ParticleTrail { length: 10, interval: 0.05, width: 1.0, taper: false, fade: false })).unwrap();
    assert!(row_max(&trail, -10, 10) > 200, "the trail lies behind it");
    assert!(row_max(&trail, 32, 60) < 10, "and not ahead of it");
}
