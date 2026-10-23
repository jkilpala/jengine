//! A sun (`DirectionalLight2D`): even light, parallel shadows, and shadows
//! that lengthen as it sets - on the GPU, and where the CPU says.
//!
//! A sun is drawn as a point light placed far out along its direction
//! (`lighting::sun_as_point`), so the point pipeline, the occluder packing and
//! `light_at` all handle it unchanged. Two things had to change for that to
//! hold: the near epsilon of the shadow ray was a thousandth of the ray, which
//! on a 40,000-unit ray ignored every edge within forty units of a point; and
//! a sun's `height` stretches bounded shadows (`shadow_length / height`),
//! carried to the shader as the light's shadow scale.
//!
//! Renders the real lighting pass under zero ambient and reads pixels back
//! beside `light_at`. Without a GPU adapter it prints why and passes.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{AmbientLight, DirectionalLight2D, LightOccluder, Transform};
use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::lighting::{self, gather_lights};

const W: u32 = 384;
const H: u32 = 192;

fn srgb_to_linear(b: u8) -> f32 {
    let v = b as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::ONE }
}

/// Two 20x20 boxes at (-100, 50) and (100, 50), each shadow 40 long under a
/// sun at height 1, and a sun travelling `direction` at `height`.
fn world(direction: f32, height: f32) -> World {
    let mut w = World::new();
    w.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    w.spawn((at(0.0, 0.0), DirectionalLight2D { direction, height, ..Default::default() }));
    for x in [-100.0, 100.0] {
        w.spawn((at(x, 50.0), LightOccluder { size: [20.0, 20.0], shadow_length: 40.0, ..Default::default() }));
    }
    w
}

/// (GPU, CPU) light at each point.
fn sample(world: &World, points: &[Vec2]) -> Option<Vec<(f32, f32)>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("suns_cast_parallel_shadows: no GPU adapter - skipped");
        None
    })?;
    let lights = gather_lights(world);
    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let camera = Camera2D::new(W as f32, H as f32);
    let mut encoder = gpu.encoder();
    drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    Some(
        points
            .iter()
            .map(|p| {
                let px = (p.x + W as f32 / 2.0) as u32;
                let py = (H as f32 / 2.0 - p.y) as u32;
                let g = srgb_to_linear(img.rgba[((py * W + px) * 4) as usize]);
                (g, lighting::light_at(*p, &lights).x)
            })
            .collect(),
    )
}

const DOWN: f32 = -std::f32::consts::FRAC_PI_2;

#[test]
fn a_sun_lights_evenly_and_shadows_in_parallel() {
    // Straight down. 20 below each box's bottom face (y = 40) is shadowed,
    // for both boxes alike; to the side of either is full light.
    let w = world(DOWN, 1.0);
    let pts = [Vec2::new(-100.0, 20.0), Vec2::new(100.0, 20.0), Vec2::new(-150.0, 20.0), Vec2::new(0.0, -60.0)];
    let Some(v) = sample(&w, &pts) else { return };
    for (i, s) in v[..2].iter().enumerate() {
        assert!(s.1 < 1e-3 && s.0 < 0.02, "under box {i}: {s:?}");
    }
    for s in &v[2..] {
        assert!((s.1 - 1.0).abs() < 1e-3 && (s.0 - 1.0).abs() < 0.02, "in the open, the sun's full intensity: {s:?}");
    }
}

#[test]
fn a_low_sun_throws_longer_shadows() {
    // 60 below the bottom face is past a 40-long shadow under a high sun, and
    // inside it under one at height 0.4 (40 / 0.4 = 100).
    let p = [Vec2::new(-100.0, -20.0)];
    let Some(high) = sample(&world(DOWN, 1.0), &p) else { return };
    let Some(low) = sample(&world(DOWN, 0.4), &p) else { return };
    assert!(high[0].1 > 0.99 && high[0].0 > 0.97, "high sun, short shadow: {:?}", high[0]);
    assert!(low[0].1 < 1e-3 && low[0].0 < 0.02, "low sun, long shadow: {:?}", low[0]);
}

#[test]
fn a_slanted_sun_shadows_the_same_way_everywhere() {
    // Travelling down and to the right at 30 degrees off vertical: the
    // shadow's middle 25 units along that line from each box's centre + 10.
    let dir = DOWN + 30f32.to_radians();
    let travel = Vec2::new(dir.cos(), dir.sin());
    let w = world(dir, 1.0);
    let pts: Vec<Vec2> = [-100.0f32, 100.0].iter().map(|&x| Vec2::new(x, 50.0) + travel * 30.0).collect();
    let Some(v) = sample(&w, &pts) else { return };
    for s in &v {
        assert!(s.1 < 1e-3 && s.0 < 0.02, "the same shadow under each box: {s:?}");
    }
    // Straight below the box, off the slanted shadow, is lit.
    let Some(below) = sample(&w, &[Vec2::new(-100.0 - 12.0, 5.0)]) else { return };
    assert!(below[0].1 > 0.99, "left of a shadow thrown to the right: {:?}", below[0]);
}

#[test]
fn an_edge_close_to_the_point_still_shadows_it() {
    // A point 5 units under a box's bottom face, on a 40,000-unit ray: a
    // thousandth of the ray (forty units) used to skip the edge.
    let Some(v) = sample(&world(DOWN, 1.0), &[Vec2::new(-100.0, 35.0)]) else { return };
    assert!(v[0].1 < 1e-3 && v[0].0 < 0.02, "right under the face: {:?}", v[0]);
}

/// A light that gives no light is not drawn, packed or asked about: a town's
/// moon by day, and its lamps. Each was a full shadow test per pixel it
/// covered - with a day's moon over the whole screen, Day & Night ran at 16
/// frames a second.
#[test]
fn a_dark_light_is_left_out() {
    use jengine_core::ecs::components::PointLight2D;
    let mut w = world(DOWN, 1.0);
    w.spawn((at(0.0, 0.0), DirectionalLight2D { intensity: 0.0, ..Default::default() }));
    w.spawn((at(50.0, 0.0), PointLight2D { intensity: 0.0, ..Default::default() }));
    w.spawn((at(60.0, 0.0), PointLight2D { color: [0.0; 3], ..Default::default() }));
    let lights = gather_lights(&w);
    assert_eq!(lighting::point_lights_and_suns(&lights).len(), 1, "only the lit sun");
    assert_eq!(lighting::pack_light_occluders(&lights).point.len(), 1);
}
