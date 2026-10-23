//! An occluder's inside takes light; what is beyond it does not - on the GPU,
//! and where the CPU says.
//!
//! Until Sep 2026 every occluder shadowed its own inside: the ray from a
//! light to a point inside the outline crosses the outline on the way out, so
//! a lit pillar, wall or crate that cast a shadow drew black. The advice was
//! to draw such sprites `unlit`, which also throws away their normal maps -
//! and the showcase's Crypt gave its columns zero-width occluders instead.
//! `LightOccluder::self_shadow` (off by default, as Unity's `selfShadows`)
//! decides it now; off, an occluder skips its own edges for a point it
//! contains, told by the parity of the edges the ray crosses.
//!
//! Each case renders the real lighting pass under zero ambient and reads the
//! pixels back beside `light_at`. Without a GPU adapter it prints why and
//! passes, like `render_golden`.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{AmbientLight, LightOccluder, PointLight2D, Transform};
use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::lighting::{self, gather_lights};

const W: u32 = 256;
const H: u32 = 128;

fn srgb_to_linear(b: u8) -> f32 {
    let v = b as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::ONE }
}

fn light(x: f32, softness: f32) -> (Transform, PointLight2D) {
    (at(x, 0.0), PointLight2D { color: [1.0; 3], intensity: 1.0, radius: 200.0, falloff: 1.0, shadow_softness: softness, height: jengine_core::ecs::components::DEFAULT_LIGHT_HEIGHT, casts_shadows: true, ..Default::default() })
}

/// Render `world` through the lighting pass and return a reader of linear
/// red at a world point, plus the world's `LightData`.
fn render(world: &World) -> Option<(impl Fn(Vec2) -> f32 + use<>, lighting::LightData)> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("occluders_do_not_shadow_themselves: no GPU adapter - skipped");
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
    let read = move |p: Vec2| {
        let px = (p.x + W as f32 / 2.0) as u32;
        let py = (H as f32 / 2.0 - p.y) as u32;
        srgb_to_linear(img.rgba[((py * W + px) * 4) as usize])
    };
    Some((read, lights))
}

/// A 40 x 40 box at x = 0 with a light at x = -80: the point in the box's
/// middle, and the point beyond it at x = 60.
fn box_and_light(self_shadow: bool, softness: f32) -> World {
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    world.spawn(light(-80.0, softness));
    world.spawn((at(0.0, 0.0), LightOccluder { size: [40.0, 40.0], self_shadow, ..Default::default() }));
    world
}

#[test]
fn an_occluders_inside_is_lit_and_what_is_beyond_it_is_not() {
    for softness in [0.0, 6.0] {
        let Some((screen, lights)) = render(&box_and_light(false, softness)) else { return };
        let (inside, beyond) = (Vec2::new(0.0, 0.0), Vec2::new(60.0, 0.0));
        let (gi, ci) = (screen(inside), lighting::light_at(inside, &lights).x);
        let (gb, cb) = (screen(beyond), lighting::light_at(beyond, &lights).x);
        // The light at 80 units of a 200 radius, falloff 1: 0.6.
        assert!(
            (ci - 0.6).abs() < 0.01 && (gi - ci).abs() < 0.03,
            "softness {softness}: the box's inside should take the light as if it were not there - GPU {gi}, CPU {ci}"
        );
        assert!(cb < 0.02 && gb < 0.03, "softness {softness}: beyond the box is still its shadow - GPU {gb}, CPU {cb}");
    }
}

#[test]
fn self_shadow_keeps_the_old_dark_inside() {
    let Some((screen, lights)) = render(&box_and_light(true, 0.0)) else { return };
    let inside = Vec2::new(5.0, 0.0);
    let (g, c) = (screen(inside), lighting::light_at(inside, &lights).x);
    assert!(c < 0.02 && g < 0.03, "self_shadow: true shadows the box's own inside - GPU {g}, CPU {c}");
}

#[test]
fn a_light_inside_an_occluder_is_still_boxed_in() {
    // The light sits in the box's middle: outside the box is dark, inside lit
    // - which is the same parity of edges crossed as "the point is inside",
    // so the light's own containment has to be told apart.
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    world.spawn(light(0.0, 0.0));
    world.spawn((at(0.0, 0.0), LightOccluder { size: [40.0, 40.0], ..Default::default() }));
    let Some((screen, lights)) = render(&world) else { return };
    let (inside, outside) = (Vec2::new(12.0, 0.0), Vec2::new(60.0, 0.0));
    let (gi, ci) = (screen(inside), lighting::light_at(inside, &lights).x);
    let (go, co) = (screen(outside), lighting::light_at(outside, &lights).x);
    assert!(ci > 0.9 && (gi - ci).abs() < 0.03, "inside, with the light: GPU {gi}, CPU {ci}");
    assert!(co < 0.02 && go < 0.03, "outside the box around the light: GPU {go}, CPU {co}");
}

#[test]
fn another_occluder_still_shadows_a_point_inside_this_one() {
    // Box A (no self shadow) contains the point; box B stands between it and
    // the light. A skips its own edges; B's still count.
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    world.spawn(light(-80.0, 0.0));
    world.spawn((at(0.0, 0.0), LightOccluder { size: [40.0, 40.0], ..Default::default() }));
    world.spawn((at(-40.0, 0.0), LightOccluder { size: [10.0, 30.0], ..Default::default() }));
    let Some((screen, lights)) = render(&world) else { return };
    let p = Vec2::new(0.0, 0.0);
    let (g, c) = (screen(p), lighting::light_at(p, &lights).x);
    assert!(c < 0.02 && g < 0.03, "box B shadows the point inside box A - GPU {g}, CPU {c}");
}

#[test]
fn a_ray_through_an_occluders_corners_still_crosses_it() {
    // A lamp and a point on a grid diagonal, a box centred between them: the
    // ray runs exactly through two of its corners, which is a light puzzle's
    // diagonal pane. Counting a corner as two edges - or, by a rounding, one
    // - makes the parity say "inside" for some of these, and the box stops
    // shadowing. Swept over sizes and places because only some roundings
    // bite. CPU only: the shader transcribes the same rule, pinned by
    // `cpu_light_matches_the_shader`.
    let mut wrong = Vec::new();
    for k in 0..400 {
        let cell = 20.0 + (k % 20) as f32 * 3.3;
        let c = Vec2::new(-150.0 + (k / 20) as f32 * 13.7, -40.0 + (k % 7) as f32 * 9.1);
        let half = cell * (0.3 + (k % 5) as f32 * 0.04);
        let mut w = World::new();
        w.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
        w.spawn((
            at(c.x - cell, c.y - cell),
            PointLight2D { color: [1.0; 3], intensity: 1.0, radius: 400.0, falloff: 1.0, shadow_softness: 0.0, height: jengine_core::ecs::components::DEFAULT_LIGHT_HEIGHT, casts_shadows: true, ..Default::default() },
        ));
        w.spawn((at(c.x, c.y), LightOccluder { size: [half * 2.0, half * 2.0], ..Default::default() }));
        let beyond = c + Vec2::splat(cell);
        let v = lighting::light_at(beyond, &gather_lights(&w)).x;
        if v > 1e-4 {
            wrong.push((c, cell, half, v));
        }
    }
    assert!(wrong.is_empty(), "{} of 400 boxes crossed exactly corner to corner cast no shadow, e.g. {:?}", wrong.len(), &wrong[..wrong.len().min(3)]);
}
