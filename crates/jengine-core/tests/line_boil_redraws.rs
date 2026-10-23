//! **Line boil** (Oct 2026; the `boil` post effect, `output.wgsl`): the
//! frame read through a smooth noise offset of up to `amount` pixels, a new
//! pattern for each drawing, so lines wobble as if redrawn. Through the real
//! post stack on a headless device: a straight line comes out wavy, never
//! more than `amount` from where it was; the same drawing twice is the same
//! picture (it steps, it does not shimmer), and the next drawing differs.
//! Found by the showcase's Sketchbook. Without a GPU adapter it prints why
//! and passes.
#![cfg(feature = "gpu-wgpu")]

use std::collections::HashMap;

use glam::Vec2;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuPostProcessStack, WgpuSpriteBatch};
use jengine_core::renderer::material::UniformValue;
use jengine_core::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings};

const W: u32 = 96;
const H: u32 = 96;

/// A white vertical bar 4 pixels wide at the middle, boiled with `seed`
/// (none for no boil): which pixels are lit.
fn frame(gpu: &HeadlessGpu, seed: Option<f32>) -> Vec<bool> {
    let scene_format = wgpu::TextureFormat::Rgba16Float;
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, scene_format, gpu.color_format).expect("post stack");
    let effects = match seed {
        Some(s) => {
            let p: HashMap<String, UniformValue> = [("amount", 3.0), ("scale", 12.0), ("seed", s)]
                .into_iter()
                .map(|(k, v)| (k.to_string(), UniformValue::Float(v)))
                .collect();
            vec![PostProcessEffectSettings { name: "boil".into(), enabled: true, params: p }]
        }
        None => Vec::new(),
    };
    stack.set_settings(PostProcessSettings { effects, ..Default::default() });
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("white", &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, scene_format).unwrap();
    let vp = glam::Mat4::orthographic_rh_gl(-(W as f32) / 2.0, W as f32 / 2.0, -(H as f32) / 2.0, H as f32 / 2.0, -1.0, 1.0);
    let mut encoder = gpu.encoder();
    {
        let mut pass = stack.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        batch.begin_frame();
        batch.begin();
        batch.draw_sprite_uv(Vec2::ZERO, Vec2::new(4.0, H as f32), 0.0, glam::Vec4::ONE, [0.0, 0.0, 1.0, 1.0]);
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture("white").unwrap());
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    gpu.read_rgba().rgba.chunks(4).map(|p| p[0] > 128).collect()
}

/// The lit columns of each row.
fn spans(img: &[bool]) -> Vec<Vec<u32>> {
    (0..H).map(|y| (0..W).filter(|&x| img[(y * W + x) as usize]).collect()).collect()
}

#[test]
fn a_boiled_line_wobbles_by_drawing_and_stays_near_itself() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("line_boil_redraws: no GPU adapter - skipped");
        return;
    };
    let straight = frame(&gpu, None);
    let a = frame(&gpu, Some(3.0));
    let again = frame(&gpu, Some(3.0));
    let b = frame(&gpu, Some(4.0));
    assert_eq!(a, again, "the same drawing twice is the same picture");
    assert_ne!(a, b, "the next drawing is a new one");
    assert_ne!(a, straight, "and boiled, the line is not straight");
    // It wanders, but never further than the amount.
    let rows = spans(&a);
    let lefts: Vec<u32> = rows.iter().filter_map(|r| r.first().copied()).collect();
    assert_eq!(lefts.len(), H as usize, "a lit pixel on every row");
    let (lo, hi) = (*lefts.iter().min().unwrap(), *lefts.iter().max().unwrap());
    assert!(hi > lo, "its left edge moves from row to row");
    let base = spans(&straight)[H as usize / 2][0];
    assert!(lo + 4 >= base && hi <= base + 4, "within the amount of where it was: {lo}..{hi} about {base}");
}
