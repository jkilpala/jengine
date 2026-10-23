//! **Lens rain** (Oct 2026; the `lens_rain` post effect, `output.wgsl`):
//! beads and running drops on the glass, each bending the frame behind it,
//! worked out from the clock with no state - and mist that steams the glass
//! except where drops are. Through the real post stack on a headless device
//! over a picture of stripes: with no drops and no mist the frame is
//! untouched; at one time the picture is the same twice; a moment later the
//! drops have moved; drops bend the stripes; and mist softens them. Found by
//! the showcase's Rainy Window. Without a GPU adapter it prints why and
//! passes.
#![cfg(feature = "gpu-wgpu")]

use std::collections::HashMap;

use glam::Vec2;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuPostProcessStack, WgpuSpriteBatch};
use jengine_core::renderer::material::UniformValue;
use jengine_core::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings};

const W: u32 = 128;
const H: u32 = 128;

/// Black and white stripes 4 pixels wide, through `lens_rain` with
/// `params` (none for no effect): each pixel's red byte.
fn frame(gpu: &HeadlessGpu, params: Option<&[(&str, f32)]>) -> Vec<u8> {
    let scene_format = wgpu::TextureFormat::Rgba16Float;
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, scene_format, gpu.color_format).expect("post stack");
    let effects = match params {
        Some(p) => {
            let p: HashMap<String, UniformValue> = p.iter().map(|(k, v)| (k.to_string(), UniformValue::Float(*v))).collect();
            vec![PostProcessEffectSettings { name: "lens_rain".into(), enabled: true, params: p }]
        }
        None => Vec::new(),
    };
    stack.set_settings(PostProcessSettings { effects, ..Default::default() });
    let mut rgba = Vec::with_capacity((W * H * 4) as usize);
    for _y in 0..H {
        for x in 0..W {
            let v = if (x / 4) % 2 == 0 { 255 } else { 0 };
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("stripes".into(), RawTextureData { width: W, height: H, rgba });
    assets.ensure_wgpu_texture("stripes", &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, scene_format).unwrap();
    let vp = glam::Mat4::orthographic_rh_gl(-(W as f32) / 2.0, W as f32 / 2.0, -(H as f32) / 2.0, H as f32 / 2.0, -1.0, 1.0);
    let mut encoder = gpu.encoder();
    {
        let mut pass = stack.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        batch.begin_frame();
        batch.begin();
        batch.draw_sprite_uv(Vec2::ZERO, Vec2::new(W as f32, H as f32), 0.0, glam::Vec4::ONE, [0.0, 0.0, 1.0, 1.0]);
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture("stripes").unwrap());
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    gpu.read_rgba().rgba.chunks(4).map(|p| p[0]).collect()
}

fn differing(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).filter(|(x, y)| (**x as i32 - **y as i32).abs() > 40).count()
}

/// How many pixels are neither near black nor near white: what softening
/// leaves of sharp stripes.
fn grey(img: &[u8]) -> usize {
    img.iter().filter(|&&v| v > 50 && v < 205).count()
}

#[test]
fn drops_bend_the_frame_and_move_with_time() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("lens_rain_bends_the_frame: no GPU adapter - skipped");
        return;
    };
    let plain = frame(&gpu, None);
    let none = frame(&gpu, Some(&[("amount", 0.0), ("time", 3.0)]));
    assert_eq!(differing(&plain, &none), 0, "no drops, no mist: the frame is untouched");
    let rain = [("amount", 1.0), ("size", 8.0), ("time", 3.0)];
    let a = frame(&gpu, Some(&rain));
    let again = frame(&gpu, Some(&rain));
    assert_eq!(a, again, "the same moment is the same picture");
    let bent = differing(&plain, &a);
    assert!(bent > 300, "drops bend the stripes: {bent} pixels moved");
    let later = frame(&gpu, Some(&[("amount", 1.0), ("size", 8.0), ("time", 3.4)]));
    assert!(differing(&a, &later) > 50, "a moment later the drops have run on");
    // No refraction: drops are there but bend nothing.
    let flat = frame(&gpu, Some(&[("amount", 1.0), ("size", 8.0), ("time", 3.0), ("refraction", 0.0)]));
    assert_eq!(differing(&plain, &flat), 0, "refraction 0 leaves the picture alone");
}

#[test]
fn mist_steams_the_glass_but_not_the_drops() {
    let Some(gpu) = HeadlessGpu::new(W, H) else { return };
    let plain = frame(&gpu, None);
    // Few drops, full mist: most of the glass is steamed.
    let misty = frame(&gpu, Some(&[("amount", 0.15), ("size", 8.0), ("time", 3.0), ("mist", 1.0), ("refraction", 0.0)]));
    assert!(grey(&misty) > grey(&plain) + (W * H / 3) as usize, "softened: {} grey against {}", grey(&misty), grey(&plain));
}
