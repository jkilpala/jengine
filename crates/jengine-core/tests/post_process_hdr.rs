//! HDR through the real post stack: a float scene, the bloom chain, and the
//! output pass's tonemap, run on the GPU and read back.
//!
//! What these pin down, each of which the renderer could not do before:
//!
//! - a scene value above 1.0 survives to the output pass, where an HDR scene
//!   tonemaps it and an LDR one clamps it exactly as an 8-bit target did;
//! - bloom reaches a long way: the old bloom was one 9-tap pass a few pixels
//!   wide, so a bright dot lit nothing 20 pixels off;
//! - an HDR scene with nothing brighter than white does not bloom at a
//!   threshold of 1.0 - bloom picks out *lights*, not white walls.
//!
//! Without a GPU adapter they print why and pass, like `render_golden`.
#![cfg(feature = "gpu-wgpu")]

use std::collections::HashMap;

use glam::{Mat4, Vec2, Vec4};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuPostProcessStack, WgpuSpriteBatch, WgpuTexture};
use jengine_core::renderer::material::UniformValue;
use jengine_core::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings, Tonemap};

const W: u32 = 256;
const H: u32 = 128;
const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

fn srgb_to_linear(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn bloom(threshold: f32, intensity: f32) -> PostProcessEffectSettings {
    let mut params = HashMap::new();
    params.insert("threshold".to_string(), UniformValue::Float(threshold));
    params.insert("intensity".to_string(), UniformValue::Float(intensity));
    PostProcessEffectSettings { name: "bloom".to_string(), enabled: true, params }
}

/// Render one frame: a background of `clear` (linear), plus a `dot` x `dot`
/// square of colour `dot_value` at the centre when `dot > 0`. Returns the
/// output as linear RGB per pixel, row-major.
fn frame(settings: PostProcessSettings, clear: f64, dot: f32, dot_value: f32) -> Option<Vec<[f64; 3]>> {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("post_process_hdr: no GPU adapter - skipped");
        return None;
    };
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, HDR, gpu.color_format).expect("post stack");
    stack.set_settings(settings);

    let white = WgpuTexture::from_rgba(&gpu.device, &gpu.queue, 1, 1, &[255, 255, 255, 255]).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, HDR).expect("batch");
    let vp = Mat4::orthographic_rh_gl(
        -(W as f32) / 2.0, W as f32 / 2.0, -(H as f32) / 2.0, H as f32 / 2.0, -1.0, 1.0,
    );

    let mut encoder = gpu.encoder();
    {
        let c = wgpu::Color { r: clear, g: clear, b: clear, a: 1.0 };
        let mut pass = stack.scene_target().begin_pass(&mut encoder, Some(c));
        if dot > 0.0 {
            batch.begin_frame();
            batch.begin();
            // A colour above 1.0 is how a scene authors something brighter
            // than white: vertex colours are floats, and so is the target.
            batch.draw_sprite(Vec2::ZERO, Vec2::splat(dot), 0.0, Vec4::new(dot_value, dot_value, dot_value, 1.0));
            batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, &white);
        }
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);

    let img = gpu.read_rgba();
    Some(
        img.rgba
            .chunks(4)
            .map(|p| {
                [
                    srgb_to_linear(p[0] as f64 / 255.0),
                    srgb_to_linear(p[1] as f64 / 255.0),
                    srgb_to_linear(p[2] as f64 / 255.0),
                ]
            })
            .collect(),
    )
}

fn at(img: &[[f64; 3]], x: u32, y: u32) -> f64 {
    img[(y * W + x) as usize][1]
}

fn hdr(tonemap: Tonemap, effects: Vec<PostProcessEffectSettings>) -> PostProcessSettings {
    PostProcessSettings { hdr: true, tonemap: Some(tonemap), effects, ..Default::default() }
}

#[test]
fn an_ldr_frame_clamps_as_an_8_bit_target_did() {
    // Not opted in: 4.0 comes out as exactly white, 0.25 as 0.25.
    let Some(img) = frame(PostProcessSettings::default(), 0.25, 40.0, 4.0) else { return };
    assert!(at(&img, W / 2, H / 2) > 0.99, "the dot should clip to white");
    let bg = at(&img, 8, 8);
    assert!((bg - 0.25).abs() < 0.006, "the background should pass through untouched, got {bg}");
}

#[test]
fn an_hdr_frame_tonemaps_rather_than_clips() {
    // ACES maps 4.0 to just under white and pulls 0.25 down a little - the
    // curve's toe - rather than clipping one and leaving the other.
    let Some(img) = frame(hdr(Tonemap::Aces, vec![]), 0.25, 40.0, 4.0) else { return };
    let dot = at(&img, W / 2, H / 2);
    let bg = at(&img, 8, 8);
    assert!(dot > 0.85 && dot < 1.0, "4.0 under ACES should be bright but below white, got {dot}");
    assert!(bg > 0.08 && bg < 0.25, "0.25 under ACES should darken a little, got {bg}");

    // Exposure is a stop: halving it darkens the background.
    let mut half = hdr(Tonemap::Aces, vec![]);
    half.exposure = Some(0.5);
    let Some(dim) = frame(half, 0.25, 0.0, 0.0) else { return };
    assert!(at(&dim, 8, 8) < bg * 0.75, "exposure 0.5 should darken the frame");
}

#[test]
fn bloom_reaches_far_from_a_bright_dot() {
    // A 4 x 4 dot at 8.0 on black. Measure how far its bloom reaches -
    // untonemapped, because ACES has a toe that maps anything under ~0.004
    // to black, which would measure the curve rather than the bloom.
    let Some(img) = frame(hdr(Tonemap::None, vec![bloom(1.0, 1.0)]), 0.0, 4.0, 8.0) else { return };
    let cx = W / 2;
    let cy = H / 2;
    let near = at(&img, cx + 10, cy);
    let far = at(&img, cx + 40, cy);
    assert!(near > 0.01, "10 px from the dot should glow, got {near}");
    assert!(far > 0.0005, "40 px from the dot should still glow - the old bloom reached a few pixels; got {far}");
    assert!(near > far, "and the glow should fall off with distance");

    // The glow is round, not a square or a cross.
    let diag = at(&img, cx + 21, cy + 21); // ~30 px out on the diagonal
    let straight = at(&img, cx + 30, cy);
    assert!(diag > straight * 0.3 && diag < straight * 3.0, "diag {diag} vs straight {straight}");
}

#[test]
fn an_hdr_frame_blooms_lights_not_white_walls() {
    // A white (1.0) square at threshold 1.0 is a wall, not a light.
    let Some(img) = frame(hdr(Tonemap::None, vec![bloom(1.0, 1.0)]), 0.0, 20.0, 1.0) else { return };
    let off = at(&img, W / 2 + 30, H / 2);
    assert!(off < 0.002, "a surface at exactly white should not bloom, got {off}");
}
