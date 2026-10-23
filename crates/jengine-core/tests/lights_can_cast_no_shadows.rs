//! A light can opt out of shadows - on the GPU, and where the CPU says.
//!
//! Every light was shadowed by every occluder in its reach. That is wrong for
//! a glow, a fill, and above all for a light that sits *on* the occluder: the
//! showcase's Spellcraft put a crystal light atop each stone post, and in flat
//! land the post then threw a wedge of dark straight down the floor beneath
//! its own crystal. Godot's lights have `shadow_enabled`, Unity's a shadow
//! strength; `casts_shadows: false` here hands the light no occluder edges,
//! which is the one packing both the shader and `light_at` read.
//!
//! Without a GPU adapter it prints why and passes.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{AmbientLight, LightOccluder, PointLight2D, SpotLight2D, Transform};
use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::lighting::{self, gather_lights};

const W: u32 = 256;
const H: u32 = 64;

fn srgb_to_linear(b: u8) -> f32 {
    let v = b as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::ONE }
}

/// A light at x = -100 and a wall at x = -50; (GPU, CPU) light at x = 40,
/// behind the wall.
fn behind_the_wall(casts: bool, spot: bool) -> Option<(f32, f32)> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("lights_can_cast_no_shadows: no GPU adapter - skipped");
        None
    })?;
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    if spot {
        world.spawn((
            at(-100.0, 0.0),
            SpotLight2D { radius: 400.0, falloff: 1.0, angle: 0.6, casts_shadows: casts, ..Default::default() },
        ));
    } else {
        world.spawn((at(-100.0, 0.0), PointLight2D { radius: 400.0, falloff: 1.0, casts_shadows: casts, ..Default::default() }));
    }
    world.spawn((at(-50.0, 0.0), LightOccluder { size: [10.0, 40.0], ..Default::default() }));
    let lights = gather_lights(&world);

    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let camera = Camera2D::new(W as f32, H as f32);
    let mut encoder = gpu.encoder();
    drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let (x, px) = (40.0, (40.0 + W as f32 / 2.0) as u32);
    let g = srgb_to_linear(img.rgba[(((H / 2) * W + px) * 4) as usize]);
    Some((g, lighting::light_at(Vec2::new(x, 0.0), &lights).x))
}

#[test]
fn a_light_that_casts_no_shadows_shines_past_an_occluder() {
    for spot in [false, true] {
        let Some(shadowed) = behind_the_wall(true, spot) else { return };
        let Some(open) = behind_the_wall(false, spot) else { return };
        let kind = if spot { "spot" } else { "point" };
        assert!(shadowed.0 < 0.02 && shadowed.1 < 1e-3, "{kind}: a shadowing light leaves the far side dark: {shadowed:?}");
        // 140 units of a 400 radius, falloff 1: 1 - 140/400.
        assert!(open.1 > 0.5, "{kind}: with casts_shadows off the CPU sees the light: {open:?}");
        assert!((open.0 - open.1).abs() < 0.03, "{kind}: and the GPU agrees with it: {open:?}");
    }
}
