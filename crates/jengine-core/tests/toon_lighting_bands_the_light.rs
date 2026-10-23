//! Toon lighting (`toon_lighting`) steps the light into bands - on the GPU,
//! and where the CPU says.
//!
//! One point light with a linear falloff over a white floor, zero ambient:
//! smooth, every pixel along the ray is a different brightness. With three
//! bands and a hard edge, every pixel is 0, 1/3, 2/3 or 1 of it, and the
//! screen and `light_at` agree on which. Dithered, each band edge is an
//! ordered pattern of the two bands on a grid anchored to the world, and the
//! two agree on every pixel of it. Without a GPU adapter it prints why and
//! passes.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{AmbientLight, PointLight2D, ToonLighting, Transform};
use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::lighting::{self, gather_lights};

const W: u32 = 256;
const H: u32 = 32;

fn srgb_to_linear(b: u8) -> f32 {
    let v = b as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// The composite's image and the lights it drew, a light at the left edge
/// reaching the right.
fn render(toon: Option<ToonLighting>) -> Option<(Vec<u8>, lighting::LightData)> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("toon_lighting_bands_the_light: no GPU adapter - skipped");
        None
    })?;
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    world.spawn((
        Transform { position: Vec2::new(-128.0, 0.0), ..Default::default() },
        PointLight2D { radius: 256.0, falloff: 1.0, ..Default::default() },
    ));
    if let Some(t) = toon {
        world.spawn((t,));
    }
    let lights = gather_lights(&world);
    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let camera = Camera2D::new(W as f32, H as f32);
    let mut encoder = gpu.encoder();
    drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
    lp.set_hdr(&gpu.queue, false);
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    Some((gpu.read_rgba().rgba, lights))
}

/// (GPU, CPU) light along y = 0.
fn along(toon: Option<ToonLighting>, xs: &[f32]) -> Option<Vec<(f32, f32)>> {
    let (img, lights) = render(toon)?;
    Some(
        xs.iter()
            .map(|&x| {
                let px = (x + W as f32 / 2.0) as u32;
                let g = srgb_to_linear(img[(((H / 2) * W + px) * 4) as usize]);
                (g, lighting::light_at(Vec2::new(x, 0.0), &lights).x)
            })
            .collect(),
    )
}

#[test]
fn three_hard_bands() {
    // Linear falloff from x = -128: 1 - (x + 128) / 256.
    // Clear of the band edges (the edge between two bands is where the smooth
    // light is an odd sixth, e.g. 0.5 at x = 0), where a rounding decides.
    let xs = [-100.0, -40.0, 12.0, 40.0, 100.0];
    let Some(smooth) = along(None, &xs) else { return };
    let Some(toon) = along(Some(ToonLighting { bands: 3, softness: 0.0, dither: 0.0 }), &xs) else { return };
    for ((g, c), (sg, sc)) in toon.iter().zip(&smooth) {
        let nearest = (sc * 3.0).round() / 3.0;
        assert!((c - nearest).abs() < 1e-4, "the CPU steps {sc} to the nearest third: {c}");
        assert!((g - c).abs() < 0.02, "and the GPU agrees: {g} vs {c}");
        assert!((sg - sc).abs() < 0.02, "the smooth light matches too: {sg} vs {sc}");
    }
    // Four distinct points, three distinct levels: it really is banded.
    let mut levels: Vec<i32> = toon.iter().map(|(_, c)| (c * 3.0).round() as i32).collect();
    levels.dedup();
    assert!(levels.len() >= 3, "{levels:?}");
}

#[test]
fn colour_is_kept() {
    let t = ToonLighting { bands: 4, softness: 0.0, dither: 0.0 };
    let l = lighting::toon_step(glam::Vec3::new(0.6, 0.3, 0.15), &t, glam::Vec2::ZERO);
    assert!((l.x - 0.5).abs() < 1e-5, "the brightest channel goes to its band: {l:?}");
    assert!((l.y / l.x - 0.5).abs() < 1e-5 && (l.z / l.x - 0.25).abs() < 1e-5, "and the hue stays: {l:?}");
}

/// Dithered: across a whole band (softness 1) the light is a pattern of the
/// two bands either side, the share of the upper one rising with the light -
/// and the GPU draws the pattern the CPU computes, pixel for pixel, because
/// both anchor it to the world.
#[test]
fn dithered_bands_match_pixel_for_pixel() {
    let toon = ToonLighting { bands: 3, softness: 1.0, dither: 2.0 };
    let Some((img, lights)) = render(Some(toon)) else { return };
    let smooth = lighting::LightData { toon: None, ..lights.clone() };
    let mut worst: f32 = 0.0;
    let mut levels = std::collections::BTreeSet::new();
    for py in 4..H - 4 {
        for px in 0..W {
            // A pixel's centre, in the world the 1:1 camera shows.
            let p = Vec2::new(px as f32 + 0.5 - W as f32 / 2.0, H as f32 / 2.0 - (py as f32 + 0.5));
            let g = srgb_to_linear(img[((py * W + px) * 4) as usize]);
            let c = lighting::light_at(p, &lights).x;
            assert!(((c * 3.0).round() - c * 3.0).abs() < 1e-4, "every pixel is a band: {c} at {p}");
            levels.insert((c * 3.0).round() as i32);
            if (g - c).abs() >= 0.02 {
                // The two disagree only where the light is a hair from this
                // cell's threshold - the GPU's light is not the CPU's to the
                // last bit, and a pixel there can go either way. Anywhere
                // else a disagreement is the pattern itself being wrong.
                let scaled = lighting::light_at(p, &smooth).x * 3.0;
                let off = (scaled.fract() - lighting::bayer_threshold(p, toon.dither)).abs();
                worst = worst.max(off);
                assert!(off < 0.06, "the GPU draws the CPU's pattern: {g} vs {c} at {p}, {off} from the threshold");
            }
        }
    }
    eprintln!("the widest disagreement sat {worst} of a band from its threshold");
    assert_eq!(levels.len(), 4, "every band, 0 to 1: {levels:?}");
}

#[test]
fn dither_mixes_the_two_bands_by_the_light() {
    let t = ToonLighting { bands: 2, softness: 1.0, dither: 1.0 };
    // Over one 4x4 tile of cells, the share lit to the upper band is how far
    // the light is between the two - a quarter of the way, a quarter lit.
    let share = |m: f32| {
        let mut up = 0;
        for y in 0..4 {
            for x in 0..4 {
                let p = glam::Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let l = lighting::toon_step(glam::Vec3::splat(m), &t, p);
                if l.x > m {
                    up += 1;
                }
            }
        }
        up
    };
    assert_eq!(share(0.5 + 0.125), 4, "a quarter of the way up a band");
    assert_eq!(share(0.5 + 0.375), 12, "three quarters");
    let hard = ToonLighting { dither: 0.0, softness: 0.0, ..t };
    assert_eq!(lighting::toon_step(glam::Vec3::splat(0.6), &hard, glam::Vec2::ZERO).x, 0.5, "and no dither is the old step");
}
