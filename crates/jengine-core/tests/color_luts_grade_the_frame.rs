//! **Colour look-up tables and film grain** (Oct 2026; Unity's Color Lookup
//! and Film Grain): a `color_lut` names a strip the frame is graded through,
//! in the output pass after the tonemap, and a `film_grain` effect adds a
//! new pattern of grain each frame. Before them the only grade was the
//! `color_grade` effect's few sliders, and a look painted in an image
//! editor could not be used at all.
//!
//! These run the real post stack on a headless device: fill the scene with
//! one colour, grade it, read it back. The strip's addressing is
//! unit-tested in `renderer::lut`. Without a GPU adapter they print why and
//! pass.
#![cfg(feature = "gpu-wgpu")]

use std::collections::HashMap;

use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::ColorLut;
use jengine_core::renderer::backend::wgpu::WgpuPostProcessStack;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::lut;
use jengine_core::renderer::material::UniformValue;
use jengine_core::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings};

const W: u32 = 32;
const H: u32 = 32;

fn to_byte(linear: f64) -> u8 {
    let s = if linear <= 0.0031308 { linear * 12.92 } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

/// The frame for a scene filled with `linear`, graded through `strip` at
/// `strength`, with `grain` grain - every pixel's rgb bytes.
fn run(gpu: &HeadlessGpu, linear: [f64; 3], strip: Option<RawTextureData>, strength: f32, grain: f32) -> Vec<[u8; 3]> {
    let mut world = World::new();
    let mut assets = AssetManager::new();
    if let Some(strip) = strip {
        assets.insert_texture_bytes("lut.png".into(), strip);
        world.spawn((ColorLut { texture_id: "lut.png".into(), strength },));
    }
    lut::sync(&world, &assets);
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, wgpu::TextureFormat::Rgba16Float, gpu.color_format)
        .expect("post stack");
    let mut effects = Vec::new();
    if grain > 0.0 {
        let mut params = HashMap::new();
        params.insert("intensity".to_string(), UniformValue::Float(grain));
        params.insert("size".to_string(), UniformValue::Float(1.0));
        effects.push(PostProcessEffectSettings { name: "film_grain".into(), enabled: true, params });
    }
    stack.set_settings(PostProcessSettings { effects, ..Default::default() });
    stack.sync_lut(&gpu.device);
    let mut encoder = gpu.encoder();
    {
        let clear = wgpu::Color { r: linear[0], g: linear[1], b: linear[2], a: 1.0 };
        let _pass = stack.scene_target().begin_pass(&mut encoder, Some(clear));
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    gpu.read_rgba().rgba.chunks(4).map(|p| [p[0], p[1], p[2]]).collect()
}

fn inverted(n: u32) -> RawTextureData {
    let mut t = lut::identity(n);
    for px in t.rgba.chunks_mut(4) {
        for c in &mut px[..3] {
            *c = 255 - *c;
        }
    }
    t
}

fn near(a: [u8; 3], b: [u8; 3], tol: i32) -> bool {
    a.iter().zip(&b).all(|(x, y)| (*x as i32 - *y as i32).abs() <= tol)
}

#[test]
fn a_lut_grades_the_frame() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("color_luts_grade_the_frame: no GPU adapter - skipped");
        return;
    };
    let colour = [0.3, 0.6, 0.15];
    let want = colour.map(to_byte);
    let plain = run(&gpu, colour, None, 1.0, 0.0)[0];
    assert!(near(plain, want, 1), "no LUT: {plain:?} for {want:?}");
    let same = run(&gpu, colour, Some(lut::identity(32)), 1.0, 0.0)[0];
    assert!(near(same, want, 2), "the neutral strip changes nothing: {same:?} for {want:?}");
    // Under the same name and size as the neutral strip: a regrade of one
    // image (as the editor's reload makes) must reach the frame.
    let inv = run(&gpu, colour, Some(inverted(32)), 1.0, 0.0)[0];
    let want_inv = want.map(|b| 255 - b);
    assert!(near(inv, want_inv, 3), "an inverting strip inverts: {inv:?} for {want_inv:?}");
    let half = run(&gpu, colour, Some(inverted(32)), 0.5, 0.0)[0];
    for k in 0..3 {
        let (lo, hi) = (want[k].min(want_inv[k]), want[k].max(want_inv[k]));
        assert!(half[k] > lo && half[k] < hi, "half strength lies between: {half:?}");
    }
    let wrong = run(&gpu, colour, Some(RawTextureData { width: 10, height: 10, rgba: vec![0; 400] }), 1.0, 0.0)[0];
    assert!(near(wrong, want, 1), "a texture that is not a strip is ignored: {wrong:?}");
}

#[test]
fn grain_is_noise_about_the_colour() {
    let Some(gpu) = HeadlessGpu::new(W, H) else { return };
    let flat = run(&gpu, [0.18, 0.18, 0.18], None, 1.0, 0.0);
    // The output pass's dither moves a pixel by at most a step either way,
    // so two pixels may be two apart.
    assert!(flat.iter().all(|p| near(*p, flat[0], 2)), "no grain, one colour");
    let grainy = run(&gpu, [0.18, 0.18, 0.18], None, 1.0, 0.2);
    let reds: Vec<f64> = grainy.iter().map(|p| p[0] as f64).collect();
    let mean = reds.iter().sum::<f64>() / reds.len() as f64;
    let var = reds.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / reds.len() as f64;
    assert!(var.sqrt() > 5.0, "grain varies pixel to pixel: sd {}", var.sqrt());
    assert!((mean - flat[0][0] as f64).abs() < 6.0, "about the same colour: mean {mean}, flat {}", flat[0][0]);
}
