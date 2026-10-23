//! An occluder's shadow can end - on the GPU, and where the CPU says.
//!
//! Lighting is flat land, so an occluder is infinitely tall and its shadow
//! runs to the edge of the light. Under a far sun in a top-down view that is
//! wrong for anything short: the showcase's Stained Glass put its sun beyond
//! a nave and the first row of pews shadowed every pew behind it, to the
//! altar. `LightOccluder::shadow_length` bounds it, fading over the last
//! quarter; `0` keeps it unbounded.
//!
//! Renders the real lighting pass under zero ambient and reads pixels back
//! beside `light_at`. Without a GPU adapter it prints why and passes.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{AmbientLight, LightOccluder, PointLight2D, Transform};
use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::lighting::{self, gather_lights};

const W: u32 = 384;
const H: u32 = 96;

fn srgb_to_linear(b: u8) -> f32 {
    let v = b as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::ONE }
}

/// A light at x = -160, a 10 x 40 wall at x = -100 whose shadow is
/// `length` long; returns (GPU, CPU) light at each x along y = 0.
fn along(length: f32, xs: &[f32]) -> Option<Vec<(f32, f32)>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("shadows_have_a_length: no GPU adapter - skipped");
        None
    })?;
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    world.spawn((
        at(-160.0, 0.0),
        PointLight2D { color: [1.0; 3], intensity: 1.0, radius: 600.0, falloff: 1.0, shadow_softness: 0.0, height: jengine_core::ecs::components::DEFAULT_LIGHT_HEIGHT, casts_shadows: true, ..Default::default() },
    ));
    world.spawn((at(-100.0, 0.0), LightOccluder { size: [10.0, 40.0], shadow_length: length, ..Default::default() }));
    let lights = gather_lights(&world);

    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let camera = Camera2D::new(W as f32, H as f32);
    let mut encoder = gpu.encoder();
    drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    Some(
        xs.iter()
            .map(|&x| {
                let px = (x + W as f32 / 2.0) as u32;
                let py = H / 2;
                let g = srgb_to_linear(img.rgba[((py * W + px) * 4) as usize]);
                (g, lighting::light_at(Vec2::new(x, 0.0), &lights).x)
            })
            .collect(),
    )
}

#[test]
fn a_shadow_ends_where_its_length_says() {
    // The wall's far face is at x = -95: 50 past it is in shadow, 85 is in
    // the fade (75..100 - and 95 past the near face, near its end), 200 is
    // past the end.
    let Some(v) = along(100.0, &[-45.0, -10.0, 105.0]) else { return };
    let (near, fade, far) = (v[0], v[1], v[2]);
    assert!(near.1 < 1e-3 && near.0 < 0.02, "50 past the wall is in its shadow: {near:?}");
    assert!(fade.1 > 0.05 && fade.1 < 0.5, "85 past it the shadow is fading: {fade:?}");
    assert!((fade.0 - fade.1).abs() < 0.03, "and the GPU fades as the CPU does: {fade:?}");
    // At 265 units of a 600 radius, falloff 1: 1 - 265/600.
    assert!((far.1 - (1.0 - 265.0 / 600.0)).abs() < 0.01 && (far.0 - far.1).abs() < 0.03, "past the end it is lit: {far:?}");
}

#[test]
fn zero_is_unbounded() {
    let Some(v) = along(0.0, &[105.0]) else { return };
    assert!(v[0].1 < 1e-3 && v[0].0 < 0.02, "an unbounded shadow reaches the edge of the light: {:?}", v[0]);
}

#[test]
fn the_owner_word_carries_the_length_in_steps_of_four() {
    use lighting::{owner_shadow_length, shadow_length_bits};
    assert_eq!(owner_shadow_length(shadow_length_bits(0.0)), 0.0);
    assert_eq!(owner_shadow_length(shadow_length_bits(100.0)), 100.0);
    assert_eq!(owner_shadow_length(shadow_length_bits(1.0)), 4.0, "a set length is at least one step");
    assert_eq!(owner_shadow_length(shadow_length_bits(1e9)), 0x7FF as f32 * 4.0, "clamped, not wrapped into other bits");
    // And it leaves the id and the flag alone.
    let o = 12345 | lighting::NO_SELF_SHADOW | shadow_length_bits(8000.0);
    assert_eq!(o & lighting::OWNER_ID_MASK, 12345);
    assert!(o & lighting::NO_SELF_SHADOW != 0);
}

#[test]
fn a_thick_occluder_is_measured_from_the_face_the_point_sees() {
    // A wall 60 thick along the ray, shadow_length 100. 80 past its far face
    // the shadow is barely into its fade: nearly dark. Measured per edge,
    // the near face - 140 away - had already faded out and let most of the
    // light through, so every bounded shadow ended in a step a wall's
    // thickness long. CPU only; the shader's copy is pinned by
    // `cpu_light_matches_the_shader` and compared by the test above.
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    world.spawn((
        at(-300.0, 0.0),
        PointLight2D { color: [1.0; 3], intensity: 1.0, radius: 800.0, falloff: 1.0, shadow_softness: 0.0, height: jengine_core::ecs::components::DEFAULT_LIGHT_HEIGHT, casts_shadows: true, ..Default::default() },
    ));
    world.spawn((at(-100.0, 0.0), LightOccluder { size: [60.0, 40.0], shadow_length: 100.0, ..Default::default() }));
    let lights = gather_lights(&world);
    let open = 1.0 - (-70.0f32 + 300.0 + 80.0) / 800.0;
    let v = lighting::light_at(Vec2::new(-70.0 + 80.0, 0.0), &lights).x;
    assert!(v < open * 0.03, "80 past a thick wall's far face: {v} of {open} - the shadow is fading from its near face");
}
