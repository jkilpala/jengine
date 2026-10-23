//! **Soft shadows and ambient occlusion skip what cannot matter, and nothing
//! else** (Oct 2026). Both were the cost of the showcase's Clay in the editor
//! (14 fps): a soft light tested every edge in its reach on each of its eight
//! rays, twice, and ambient occlusion measured every pixel against every edge
//! near the view. Now a light tests each edge once per pixel against the wedge
//! its rays fill and the rays visit only what passed, and ambient occlusion
//! skips an occluder whose box is farther than its radius - 97 ms a frame on
//! Clay's lighting at 1600 x 900 became 15.
//!
//! Both culls are meant to be exact, which is what this checks: the GPU
//! against `light_at`, which tests every edge, over a soft lamp and a soft
//! spot, bounded shadows, an occluder that shadows itself, a tinted one and
//! enough edges to need three words of the edge mask. Nothing on the GPU
//! compared a soft shadow with the CPU before - the soft path was the one
//! `render_golden` left out. Without a GPU adapter the GPU half passes.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{
    AmbientLight, AmbientOcclusion, DEFAULT_LIGHT_HEIGHT, LightOccluder, PointLight2D, SpotLight2D, Transform,
};
use jengine_core::renderer::lighting::{self, ao_occluder_bounds, gather_lights};

const W: u32 = 384;
const H: u32 = 256;

fn at(x: f32, y: f32, rotation: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation, scale: Vec2::ONE }
}

fn circle(r: f32, n: usize) -> Vec<[f32; 2]> {
    (0..n)
        .map(|k| {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            [r * a.cos(), r * a.sin()]
        })
        .collect()
}

fn world() -> World {
    let mut w = World::new();
    w.spawn((AmbientLight { color: [1.0; 3], intensity: 0.3 },));
    w.spawn((AmbientOcclusion { radius: 18.0, strength: 0.6 },));
    w.spawn((
        at(-170.0, 110.0, 0.0),
        PointLight2D {
            color: [1.0; 3],
            intensity: 0.4,
            radius: 700.0,
            falloff: 1.0,
            shadow_softness: 24.0,
            height: DEFAULT_LIGHT_HEIGHT,
            casts_shadows: true,
            ..Default::default()
        },
    ));
    w.spawn((
        at(170.0, -120.0, 0.0),
        SpotLight2D {
            color: [1.0; 3],
            intensity: 0.25,
            radius: 600.0,
            angle: 0.9,
            direction: 2.4,
            falloff: 1.0,
            shadow_softness: 16.0,
            height: DEFAULT_LIGHT_HEIGHT,
            casts_shadows: true,
            ..Default::default()
        },
    ));
    // Twelve boxes in a loose grid, turned, with shadows of three lengths.
    for k in 0..12 {
        let (col, row) = ((k % 4) as f32, (k / 4) as f32);
        w.spawn((
            at(-120.0 + col * 70.0 + row * 9.0, 60.0 - row * 55.0, 0.3 * k as f32),
            LightOccluder {
                size: [22.0 + (k % 3) as f32 * 8.0, 18.0],
                shadow_length: [0.0, 40.0, 90.0][k % 3],
                self_shadow: k == 5,
                ..Default::default()
            },
        ));
    }
    w.spawn((at(150.0, 60.0, 0.0), LightOccluder { polygon: circle(26.0, 14), shadow_length: 60.0, ..Default::default() }));
    w.spawn((
        at(-40.0, -100.0, 0.0),
        LightOccluder { polygon: circle(20.0, 12), shadow_tint: [1.0, 0.4, 0.2], shadow_opacity: 0.7, ..Default::default() },
    ));
    w
}

#[test]
fn the_scene_needs_more_than_one_mask_word() {
    assert!(gather_lights(&world()).occluders.len() > 64, "the edge mask's third word is exercised");
}

/// The box and end the ambient occlusion pass reads for each occluder.
#[test]
fn every_edge_carries_its_occluders_box_and_end() {
    let segs = [[0.0, 0.0, 10.0, 0.0], [10.0, 0.0, 10.0, 5.0], [20.0, -3.0, 30.0, 4.0]];
    let (boxes, ends) = ao_occluder_bounds(&segs, &[7, 7, 9]);
    assert_eq!(ends, vec![2, 2, 3]);
    assert_eq!(boxes[0], [0.0, 0.0, 10.0, 5.0]);
    assert_eq!(boxes[1], boxes[0]);
    assert_eq!(boxes[2], [20.0, -3.0, 30.0, 4.0]);
}

/// The edge mask walks its bits with a de Bruijn table, not
/// `firstTrailingBit`: WGSL's bit functions become GLSL functions WebGL2's
/// GLSL ES 3.00 does not have (`findLSB`, `bitCount`, ...), and naga writes
/// them anyway - so a shader using one compiles everywhere but the WebGL
/// fallback, where the lights go dark. No engine shader uses them.
#[test]
fn engine_shaders_use_no_bit_functions_webgl2_lacks() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/shaders/wgsl");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "wgsl") {
            let src = std::fs::read_to_string(&path).unwrap();
            for f in ["firstTrailingBit", "firstLeadingBit", "countOneBits", "countTrailingZeros", "countLeadingZeros", "reverseBits", "extractBits", "insertBits"] {
                assert!(!src.contains(&format!("{f}(")), "{} calls {f}, which WebGL2 cannot compile", path.display());
            }
        }
    }
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
    fn the_gpu_shadows_as_the_cpu_says() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("soft_shadows_match_the_cpu: no GPU adapter - skipped");
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

        let (mut checked, mut off, mut worst, mut levels) = (0, 0, 0.0f32, Vec::new());
        for py in (2..H - 2).step_by(5) {
            for px in (2..W - 2).step_by(5) {
                let p = Vec2::new(px as f32 - W as f32 / 2.0 + 0.5, H as f32 / 2.0 - py as f32 - 0.5);
                let at = ((py * W + px) * 4) as usize;
                let cpu = lighting::light_at(p, &lights);
                for c in 0..3 {
                    let d = (srgb_to_linear(img.rgba[at + c]) - cpu[c]).abs();
                    worst = worst.max(d);
                    if d > 0.03 {
                        off += 1;
                    }
                }
                levels.push(cpu.x);
                checked += 3;
            }
        }
        // A pixel centre that grazes a corner can fall either side of a
        // ray test between f32 on two devices; a skipped edge would be a
        // whole region off.
        assert!(off * 200 <= checked, "{off} of {checked} channels differ from the CPU by more than 0.03 (worst {worst})");
        assert!(worst < 0.2, "a channel differs from the CPU by {worst}");
        // The scene really has shadow, penumbra and light.
        let lo = levels.iter().cloned().fold(f32::MAX, f32::min);
        let hi = levels.iter().cloned().fold(f32::MIN, f32::max);
        let between = levels.iter().filter(|&&v| v > lo + 0.05 && v < hi - 0.05).count();
        assert!(hi - lo > 0.3 && between > 20, "shadows from {lo} to {hi}, {between} points in between");
    }
}
