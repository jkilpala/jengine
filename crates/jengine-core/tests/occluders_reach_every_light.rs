//! Past sixty-four occluder edges, shadows still fall - on the GPU, and where
//! the CPU says they do.
//!
//! Until Sep 2026 the lighting pass bound the first sixty-four segments it was
//! handed for every light and dropped the rest in silence: sixteen boxes for
//! the whole screen. Box seventeen cast nothing, wherever it stood, and three
//! lighting games were designed around choosing
//! which sixteen. Each light now gets its own list from a shared pool of
//! `OCCLUDER_POOL` edges, holding only what is in its reach.
//!
//! Twenty-four boxes in a row - ninety-six edges - under two lights. The last
//! box's shadow is read back from the screen and compared with `light_at`.
//! Without a GPU adapter this prints why and passes, like `render_golden`.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{AmbientLight, LightOccluder, PointLight2D, Transform};
use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::lighting::{self, gather_lights};

const W: u32 = 512;
const H: u32 = 128;

fn srgb_to_linear(b: u8) -> f32 {
    let v = b as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

#[test]
fn the_twenty_fourth_box_still_casts_a_shadow() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("occluders_reach_every_light: no GPU adapter - skipped");
        return;
    };
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.0 },));
    // Two lights, left and right, each seeing only its half of the row.
    for x in [-128.0, 128.0] {
        world.spawn((
            Transform { position: Vec2::new(x, 0.0), rotation: 0.0, scale: Vec2::ONE },
            PointLight2D { color: [1.0, 1.0, 1.0], intensity: 1.0, radius: 120.0, falloff: 1.0, shadow_softness: 0.0, height: jengine_core::ecs::components::DEFAULT_LIGHT_HEIGHT, casts_shadows: true, ..Default::default() },
        ));
    }
    // Twenty-four small boxes along y = 30, spawned left to right: the last
    // ones are far past the sixty-four edges the old pass kept.
    for i in 0..24 {
        let x = -240.0 + i as f32 * 20.9;
        world.spawn((
            Transform { position: Vec2::new(x, 30.0), rotation: 0.0, scale: Vec2::ONE },
            LightOccluder { size: [10.0, 10.0], ..Default::default() },
        ));
    }
    let lights = gather_lights(&world);
    assert_eq!(lights.occluders.len(), 96);

    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let camera = Camera2D::new(W as f32, H as f32);
    let mut encoder = gpu.encoder();
    drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();

    // World (x, y) -> pixel. y runs up in the world, down in the image.
    let screen = |p: Vec2| -> f32 {
        let px = (p.x + W as f32 / 2.0) as u32;
        let py = (H as f32 / 2.0 - p.y) as u32;
        srgb_to_linear(img.rgba[((py * W + px) * 4) as usize])
    };

    // Box 21 (i = 20, edges 80-83 - past the sixty-four the old pass kept)
    // sits at x ~ 178, up and right of the right-hand light. The point
    // straight behind it, well inside the light's radius, is in shadow; the
    // same distance off to one side, nothing is in the way.
    let the_box = Vec2::new(-240.0 + 20.0 * 20.9, 30.0);
    let light = Vec2::new(128.0, 0.0);
    let behind = light + (the_box - light) * 1.6;
    assert!(behind.distance(light) < 100.0 && behind.x < W as f32 / 2.0, "inside the light and the frame");
    // Turned 35 degrees clockwise from the box: below the row, clear.
    let beside = light + Vec2::from_angle(-35f32.to_radians()).rotate(behind - light);
    let gpu_behind = screen(behind);
    let cpu_behind = lighting::light_at(behind, &lights).x;
    let gpu_beside = screen(beside);
    assert!(cpu_behind < 0.02, "the CPU says box 21 shadows {behind:?}: {cpu_behind}");
    assert!(gpu_beside > 0.1, "the control point beside it is lit on screen: {gpu_beside}");
    assert!(
        gpu_behind < 0.02,
        "box 21 of 24 casts no shadow on screen at {behind:?} ({gpu_behind}, while the          point beside it is {gpu_beside}) - the old sixty-four-edge budget is back"
    );

    // And a lit point with nothing between it and a light agrees too.
    let open = Vec2::new(128.0, -40.0);
    let (g, c) = (screen(open), lighting::light_at(open, &lights).x);
    assert!(c > 0.3 && (g - c).abs() < 0.05, "open ground: GPU {g}, CPU {c}");
}
