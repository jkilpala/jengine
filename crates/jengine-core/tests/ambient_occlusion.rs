//! Ambient occlusion from occluders - on the GPU, and where the CPU says.
//!
//! The engine had no ambient occlusion: the ambient light was one flat
//! colour, so a white room of white boxes (the showcase's Clay) read as
//! shapes pasted on paper, with nothing darkening where a box meets the floor
//! or two boxes stand close. An `ambient_occlusion` component now darkens the
//! ambient term near `LightOccluder` outlines: per occluder by its nearest
//! edge, multiplied, and not at all inside the occluder itself.
//!
//! Renders the real lighting pass under a white ambient with no lights and
//! reads pixels back beside `light_at`. Without a GPU adapter the GPU half
//! prints why and passes.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{AmbientLight, AmbientOcclusion, LightOccluder, Transform};
use jengine_core::renderer::lighting::{self, gather_lights};

const W: u32 = 256;
const H: u32 = 128;

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::ONE }
}

/// Two 40 x 40 boxes 12 apart, a third on its own; white ambient, AO
/// radius 30 at strength 0.8.
fn world() -> World {
    let mut w = World::new();
    w.spawn((AmbientLight { color: [1.0; 3], intensity: 1.0 },));
    w.spawn((AmbientOcclusion { radius: 30.0, strength: 0.8 },));
    w.spawn((at(-70.0, 0.0), LightOccluder { size: [40.0, 40.0], ..Default::default() }));
    w.spawn((at(-18.0, 0.0), LightOccluder { size: [40.0, 40.0], ..Default::default() }));
    w.spawn((at(70.0, 0.0), LightOccluder { size: [40.0, 40.0], ..Default::default() }));
    w
}

fn cpu(p: Vec2) -> f32 {
    lighting::light_at(p, &gather_lights(&world())).x
}

#[test]
fn far_from_everything_the_ambient_is_whole() {
    assert!((cpu(Vec2::new(70.0, 55.0)) - 1.0).abs() < 1e-4, "{}", cpu(Vec2::new(70.0, 55.0)));
}

#[test]
fn near_an_outline_it_darkens_by_the_rule() {
    // 10 from the lone box's right face: 1 - 0.8 * (1 - 10/30)^2.
    let want = 1.0 - 0.8 * (2.0f32 / 3.0).powi(2);
    assert!((cpu(Vec2::new(100.0, 0.0)) - want).abs() < 1e-4, "{} vs {want}", cpu(Vec2::new(100.0, 0.0)));
}

#[test]
fn inside_an_occluder_its_own_outline_does_not_darken_it() {
    assert!((cpu(Vec2::new(70.0, 0.0)) - 1.0).abs() < 1e-4, "{}", cpu(Vec2::new(70.0, 0.0)));
    assert!((cpu(Vec2::new(85.0, 15.0)) - 1.0).abs() < 1e-4, "near its corner, still inside: {}", cpu(Vec2::new(85.0, 15.0)));
}

#[test]
fn a_narrow_gap_is_darker_than_one_wall() {
    // The gap between the pair is x in (-50, -38); its middle is 6 from both.
    let gap = cpu(Vec2::new(-44.0, 0.0));
    let one_wall = cpu(Vec2::new(96.0, 0.0)); // 6 from the lone box only
    assert!(gap < one_wall - 0.1, "two walls should multiply: gap {gap}, one wall {one_wall}");
}

#[test]
fn without_the_component_nothing_changes() {
    let mut w = world();
    let ao: Vec<_> = w.query::<&AmbientOcclusion>().iter().map(|(e, _)| e).collect();
    for e in ao {
        w.despawn(e).unwrap();
    }
    assert!((lighting::light_at(Vec2::new(100.0, 0.0), &gather_lights(&w)).x - 1.0).abs() < 1e-4);
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;

    fn srgb_to_linear(b: u8) -> f32 {
        let v = b as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    }

    #[test]
    fn the_gpu_occludes_as_the_cpu_says() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("ambient_occlusion: no GPU adapter - skipped");
            return;
        };
        let lights = gather_lights(&world());
        let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
        let camera = Camera2D::new(W as f32, H as f32);
        let mut encoder = gpu.encoder();
        drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
        lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
        lp.composite(&mut encoder, gpu.view());
        gpu.submit(encoder);
        let img = gpu.read_rgba();
        for (x, y) in [(70.0, 55.0), (100.0, 0.0), (96.0, 0.0), (-44.0, 0.0), (70.0, 0.0), (-18.0, 30.0), (0.0, -25.0)] {
            let px = (x + W as f32 / 2.0) as u32;
            let py = (H as f32 / 2.0 - y) as u32;
            let g = srgb_to_linear(img.rgba[((py * W + px) * 4) as usize]);
            let c = lighting::light_at(Vec2::new(x + 0.5, y - 0.5), &lights).x;
            assert!((g - c).abs() < 0.03, "at ({x}, {y}) the GPU gives {g}, the CPU {c}");
        }
    }
}
