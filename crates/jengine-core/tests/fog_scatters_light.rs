//! **Fog catches light** (Oct 2026; `ecs::fog`): in the lighting composite,
//! a fog veils what is behind it by its density and adds the light the air
//! catches - the light buffer gathered over `scatter` pixels, so a lamp
//! glows in the fog past its own reach. Rendered on the GPU through the real
//! composite; without a GPU adapter it prints why and passes. The density
//! itself is tested in the module.
#![cfg(all(feature = "gpu-wgpu", feature = "fog"))]

use hecs::World;
use jengine_core::ecs::components::{AmbientLight, PointLight2D, Transform};
use jengine_core::ecs::fog::Fog;
use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::lighting::gather_lights;

const W: u32 = 256;
const H: u32 = 64;

fn srgb_to_linear(b: u8) -> f32 {
    let v = b as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// The composite over a black (or white) floor, a light of radius 40 in the
/// middle and the ambient given; the red channel along y = 0.
fn render(fog: Option<Fog>, white: bool, ambient: f32) -> Option<Vec<f32>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("fog_scatters_light: no GPU adapter - skipped");
        None
    })?;
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: ambient },));
    world.spawn((Transform::default(), PointLight2D { radius: 40.0, falloff: 1.0, ..Default::default() }));
    if let Some(f) = fog {
        world.spawn((Transform::default(), f));
    }
    let lights = gather_lights(&world);
    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let camera = Camera2D::new(W as f32, H as f32);
    let mut encoder = gpu.encoder();
    let floor = if white { wgpu::Color::WHITE } else { wgpu::Color::BLACK };
    drop(lp.scene_target().begin_pass(&mut encoder, Some(floor)));
    lp.set_hdr(&gpu.queue, false);
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba().rgba;
    Some((0..W).map(|px| srgb_to_linear(img[(((H / 2) * W + px) * 4) as usize])).collect())
}

fn at(row: &[f32], x: f32) -> f32 {
    row[(x + W as f32 / 2.0) as usize]
}

fn even(density: f32, scatter: f32, color: [f32; 3]) -> Fog {
    Fog { density, scatter, color, noise_scale: 0.0, ..Default::default() }
}

#[test]
fn fog_glows_where_the_light_is_and_past_its_reach() {
    let Some(clear) = render(None, false, 0.0) else { return };
    assert!(at(&clear, 0.0) < 0.01, "a black floor shows nothing without fog, lit or not");
    let Some(fog) = render(Some(even(0.5, 0.0, [1.0; 3])), false, 0.0) else { return };
    let mid = at(&fog, 0.0);
    assert!(mid > 0.3, "the air catches the light over a black floor: {mid}");
    assert!(at(&fog, 100.0) < 0.01, "and nothing where no light reaches");
    // Scattered over 30 pixels, it glows past the light's own 40.
    let Some(spread) = render(Some(even(0.5, 30.0, [1.0; 3])), false, 0.0) else { return };
    let past = at(&spread, 52.0);
    assert!(past > 0.008 && at(&fog, 52.0) < 0.002, "a halo past its reach: {past} against {}", at(&fog, 52.0));
    assert!(at(&spread, 0.0) < mid, "and softer at its heart");
}

#[test]
fn fog_veils_what_is_behind_it() {
    // A white floor fully lit, under black fog of half density: half as bright.
    let Some(lit) = render(None, true, 1.0) else { return };
    let Some(veiled) = render(Some(even(0.5, 0.0, [0.0; 3])), true, 1.0) else { return };
    let (a, b) = (at(&lit, -100.0), at(&veiled, -100.0));
    assert!((a - 1.0).abs() < 0.02, "lit white: {a}");
    assert!((b - 0.5).abs() < 0.03, "behind black fog of density 0.5: half: {b}");
}
