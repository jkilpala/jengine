//! **Anti-aliasing** (Oct 2026): an `fxaa` post effect smooths the stairs
//! on a turned edge - a rotated sprite, a thin line, a mesh - in the output
//! pass, after the tonemap. Before it every turned edge stair-stepped, which
//! a wire figure turning slowly makes crawl. Found by the showcase's
//! Spirograph.
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

/// A white square turned by `angle` on black, through the post chain, with
/// FXAA on or off: every pixel's red byte.
fn frame(gpu: &HeadlessGpu, angle: f32, fxaa: bool) -> Vec<u8> {
    let scene_format = wgpu::TextureFormat::Rgba16Float;
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, scene_format, gpu.color_format).expect("post stack");
    let mut effects = Vec::new();
    if fxaa {
        let mut params = HashMap::new();
        params.insert("span".to_string(), UniformValue::Float(8.0));
        effects.push(PostProcessEffectSettings { name: "fxaa".into(), enabled: true, params });
    }
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
        batch.draw_sprite_uv(Vec2::ZERO, Vec2::splat(52.0), angle, glam::Vec4::ONE, [0.0, 0.0, 1.0, 1.0]);
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture("white").unwrap());
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    gpu.read_rgba().rgba.chunks(4).map(|p| p[0]).collect()
}

/// Pixels neither black nor white: what a smoothed edge is made of.
fn partial(img: &[u8]) -> usize {
    img.iter().filter(|&&r| r > 24 && r < 230).count()
}

#[test]
fn fxaa_smooths_a_turned_edge_and_leaves_the_rest() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("fxaa_smooths_edges: no GPU adapter - skipped");
        return;
    };
    let hard = frame(&gpu, 0.3, false);
    let soft = frame(&gpu, 0.3, true);
    // Without it, a hard edge: next to no part-lit pixels. With it, the
    // stairs along four edges of a 52-texel square are blended.
    assert!(partial(&hard) < 8, "no anti-aliasing: {} part-lit", partial(&hard));
    assert!(partial(&soft) > 60, "FXAA: {} part-lit", partial(&soft));
    // The square's middle and the empty corner are untouched.
    let at = |img: &[u8], x: u32, y: u32| img[(y * W + x) as usize];
    assert_eq!((at(&soft, 48, 48), at(&soft, 2, 2)), (at(&hard, 48, 48), at(&hard, 2, 2)));
    assert!(at(&soft, 48, 48) > 240 && at(&soft, 2, 2) < 8);
    // The light is where it was: FXAA blurs along edges, it does not move or
    // dim the shape.
    let sum = |img: &[u8]| img.iter().map(|&r| r as f64).sum::<f64>();
    assert!((sum(&soft) / sum(&hard) - 1.0).abs() < 0.05, "{} vs {}", sum(&soft), sum(&hard));
}

#[test]
fn an_upright_edge_has_no_stairs_to_smooth() {
    let Some(gpu) = HeadlessGpu::new(W, H) else { return };
    // Axis-aligned, on whole pixels: the straight runs are left alone, and
    // only the four corners - where two edges meet - are softened.
    let hard = frame(&gpu, 0.0, false);
    let soft = frame(&gpu, 0.0, true);
    // The square spans texels 22..74 on both axes.
    let corners = [(22i32, 22i32), (74, 22), (22, 74), (74, 74)];
    let mut changed = Vec::new();
    for (i, (a, b)) in hard.iter().zip(&soft).enumerate() {
        if (*a as i32 - *b as i32).abs() > 40 {
            changed.push(((i as u32 % W) as i32, (i as u32 / W) as i32));
        }
    }
    let off_corner: Vec<_> = changed
        .iter()
        .filter(|(x, y)| !corners.iter().any(|(cx, cy)| (x - cx).abs() <= 3 && (y - cy).abs() <= 3))
        .collect();
    assert!(off_corner.is_empty(), "a straight edge changed at {off_corner:?} (all changes: {changed:?})");
}
