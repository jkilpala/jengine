//! **Painterly post effects** (Oct 2026): `paint` (a Kuwahara filter) and
//! `halftone` (dots on a turned screen), both in the output pass. These run
//! the real post stack on a headless device: a noisy picture with an edge
//! down its middle is painted flat on each side with the edge kept sharp,
//! and a flat grey is printed as dots of ink and paper whose share follows
//! the grey. Found by the showcase's Oil and Ink. Without a GPU adapter they
//! print why and pass.
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

/// A picture through the post chain with `effect` on (none for `""`):
/// every pixel's rgb. The picture is `rgba`, W x H, filling the frame.
fn frame(gpu: &HeadlessGpu, rgba: Vec<u8>, effect: &str, params: HashMap<String, UniformValue>) -> Vec<[u8; 3]> {
    let scene_format = wgpu::TextureFormat::Rgba16Float;
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, scene_format, gpu.color_format).expect("post stack");
    let effects = if effect.is_empty() {
        Vec::new()
    } else {
        vec![PostProcessEffectSettings { name: effect.into(), enabled: true, params }]
    };
    stack.set_settings(PostProcessSettings { effects, ..Default::default() });
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("picture".into(), RawTextureData { width: W, height: H, rgba });
    assets.ensure_wgpu_texture("picture", &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, scene_format).unwrap();
    let vp = glam::Mat4::orthographic_rh_gl(-(W as f32) / 2.0, W as f32 / 2.0, -(H as f32) / 2.0, H as f32 / 2.0, -1.0, 1.0);
    let mut encoder = gpu.encoder();
    {
        let mut pass = stack.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        batch.begin_frame();
        batch.begin();
        batch.draw_sprite_uv(Vec2::ZERO, Vec2::new(W as f32, H as f32), 0.0, glam::Vec4::ONE, [0.0, 0.0, 1.0, 1.0]);
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture("picture").unwrap());
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    gpu.read_rgba().rgba.chunks(4).map(|p| [p[0], p[1], p[2]]).collect()
}

/// Dark on the left, light on the right, both noisy (a hash, so the
/// picture is the same every run).
fn noisy_edge() -> Vec<u8> {
    let mut out = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            let h = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)).wrapping_mul(2_654_435_761) >> 24;
            let n = h as i32 - 128;
            let base = if x < W / 2 { 64 } else { 192 };
            let v = (base + n * 40 / 128).clamp(0, 255) as u8;
            out.extend_from_slice(&[v, v, v, 255]);
        }
    }
    out
}

fn flat(v: [u8; 3]) -> Vec<u8> {
    [v[0], v[1], v[2], 255].repeat((W * H) as usize)
}

fn params(pairs: &[(&str, f32)]) -> HashMap<String, UniformValue> {
    pairs.iter().map(|(k, v)| (k.to_string(), UniformValue::Float(*v))).collect()
}

fn at(img: &[[u8; 3]], x: u32, y: u32) -> u8 {
    img[(y * W + x) as usize][0]
}

/// The mean absolute difference between horizontal neighbours over a
/// region: how noisy it is.
fn roughness(img: &[[u8; 3]], xs: std::ops::Range<u32>) -> f64 {
    let (mut sum, mut n) = (0.0, 0.0);
    for y in 8..H - 8 {
        for x in xs.clone() {
            sum += (at(img, x + 1, y) as f64 - at(img, x, y) as f64).abs();
            n += 1.0;
        }
    }
    sum / n
}

#[test]
fn paint_flattens_noise_and_keeps_edges() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("painterly_post_effects: no GPU adapter - skipped");
        return;
    };
    let raw = frame(&gpu, noisy_edge(), "", HashMap::new());
    let painted = frame(&gpu, noisy_edge(), "paint", params(&[("radius", 4.0)]));
    for xs in [6..40, 56..90] {
        let (before, after) = (roughness(&raw, xs.clone()), roughness(&painted, xs.clone()));
        assert!(after < before * 0.4, "flat areas become strokes ({xs:?}): {before:.1} -> {after:.1}");
    }
    // The edge stays where it was, and stays an edge: two texels either
    // side of it are still dark and light on nearly every row.
    let rows = (0..H).filter(|&y| at(&painted, 45, y) < 110 && at(&painted, 50, y) > 150).count();
    assert!(rows as u32 > H * 9 / 10, "the edge is kept sharp on {rows} of {H} rows");
}

#[test]
fn halftone_prints_grey_as_dots_of_ink() {
    let Some(gpu) = HeadlessGpu::new(W, H) else { return };
    let mut p = params(&[("cell", 8.0), ("angle", 45.0)]);
    p.insert("ink".into(), UniformValue::Vec4([0.0, 0.0, 0.0, 1.0]));
    p.insert("paper".into(), UniformValue::Vec4([1.0, 1.0, 1.0, 1.0]));
    let share = |grey: u8| {
        let img = frame(&gpu, flat([grey; 3]), "halftone", p.clone());
        let n = img.len() as f64;
        let ink = img.iter().filter(|c| c[0] < 40).count() as f64 / n;
        let paper = img.iter().filter(|c| c[0] > 215).count() as f64 / n;
        (ink, paper)
    };
    let (mid_ink, mid_paper) = share(128);
    assert!(mid_ink + mid_paper > 0.8, "dots, not a grey: {mid_ink:.2} ink, {mid_paper:.2} paper");
    assert!((0.3..0.7).contains(&mid_ink), "half grey is about half ink: {mid_ink:.2}");
    let (dark_ink, _) = share(64);
    let (light_ink, _) = share(200);
    assert!(dark_ink > mid_ink + 0.1 && light_ink < mid_ink - 0.1, "darker is more ink: {dark_ink:.2} / {mid_ink:.2} / {light_ink:.2}");
}

#[test]
fn cmyk_prints_red_with_magenta_and_yellow() {
    let Some(gpu) = HeadlessGpu::new(W, H) else { return };
    let mut p = params(&[("mode", 1.0), ("cell", 6.0)]);
    p.insert("paper".into(), UniformValue::Vec4([1.0, 1.0, 1.0, 1.0]));
    let img = frame(&gpu, flat([255, 0, 0]), "halftone", p);
    let n = img.len() as f64;
    let mean = |k: usize| img.iter().map(|c| c[k] as f64).sum::<f64>() / n;
    assert!(mean(0) > 200.0 && mean(1) < 70.0 && mean(2) < 70.0, "red: {:.0} {:.0} {:.0}", mean(0), mean(1), mean(2));
}
