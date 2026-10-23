//! The post chain's colour grade, run on the GPU and read back.
//!
//! Every target is an sRGB format, so the post shaders see **linear** colour,
//! where middle grey is 0.18. `color_grade` pivoted its contrast at 0.5
//! instead - a light grey in linear - so at `contrast: 1.14` everything below
//! linear ~0.06 went under zero and clamped to black. Nothing noticed until a
//! dark scene used it: the visual showcase's Firelight hall went black outside
//! the brazier, and the dim floor turned red because its green and blue
//! reached zero before its red did.
//!
//! These drive the real `WgpuPostProcessStack` through a headless device: fill
//! its scene target with one colour, run the chain into the headless target,
//! read a pixel. Without a GPU adapter they print why and pass, like
//! `render_golden`.
#![cfg(feature = "gpu-wgpu")]

use std::collections::HashMap;

use jengine_core::renderer::backend::wgpu::WgpuPostProcessStack;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::material::UniformValue;
use jengine_core::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings};

const W: u32 = 16;
const H: u32 = 16;

fn srgb_to_linear(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn byte_to_linear(b: u8) -> f64 {
    srgb_to_linear(b as f64 / 255.0)
}

fn grade(contrast: f32, saturation: f32) -> PostProcessSettings {
    let mut params = HashMap::new();
    params.insert("contrast".to_string(), UniformValue::Float(contrast));
    params.insert("saturation".to_string(), UniformValue::Float(saturation));
    PostProcessSettings {
        effects: vec![PostProcessEffectSettings {
            name: "color_grade".to_string(),
            enabled: true,
            params,
        }],
        ..Default::default()
    }
}

/// The colour the chain writes for a scene filled with `linear` (rgb), as
/// linear floats read back from the centre pixel. `None` without a GPU.
fn run(settings: PostProcessSettings, linear: [f64; 3]) -> Option<[f64; 3]> {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("post_process_grades_in_linear: no GPU adapter - skipped");
        return None;
    };
    // The scene in a float target, as every host draws it where the adapter
    // allows; the grade runs there and the output pass writes the display.
    let mut stack =
        WgpuPostProcessStack::new(&gpu.device, W, H, wgpu::TextureFormat::Rgba16Float, gpu.color_format)
            .expect("post stack");
    stack.set_settings(settings);

    let mut encoder = gpu.encoder();
    {
        // A clear colour is linear, and the sRGB target encodes it on store,
        // exactly as a sprite's colour would be.
        let clear = wgpu::Color { r: linear[0], g: linear[1], b: linear[2], a: 1.0 };
        let _pass = stack.scene_target().begin_pass(&mut encoder, Some(clear));
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);

    let image = gpu.read_rgba();
    let i = (((H / 2) * W + W / 2) * 4) as usize;
    let px = &image.rgba[i..i + 4];
    Some([byte_to_linear(px[0]), byte_to_linear(px[1]), byte_to_linear(px[2])])
}

/// One 8-bit sRGB step near black is ~0.0003 linear; near 0.18 it is ~0.003.
fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn contrast_one_changes_nothing() {
    for v in [0.02, 0.18, 0.6] {
        let Some(out) = run(grade(1.0, 1.0), [v, v, v]) else { return };
        for c in out {
            assert!(close(c, v, v * 0.03 + 0.001), "contrast 1.0 moved {v} to {c}");
        }
    }
}

#[test]
fn middle_grey_is_the_pivot() {
    let Some(out) = run(grade(1.4, 1.0), [0.18, 0.18, 0.18]) else { return };
    for c in out {
        assert!(close(c, 0.18, 0.006), "middle grey moved to {c} under contrast 1.4");
    }
}

#[test]
fn more_contrast_darkens_a_dark_value_without_crushing_it() {
    // Linear 0.03 is sRGB ~19% - a dim floor in a dark room.
    let Some(out) = run(grade(1.14, 1.0), [0.03, 0.03, 0.03]) else { return };
    for c in out {
        assert!(c < 0.03, "contrast > 1 should darken a value below middle grey, got {c}");
        assert!(c > 0.015, "contrast 1.14 crushed linear 0.03 to {c} - the old 0.5 pivot's bug");
    }
}

#[test]
fn more_contrast_keeps_a_dim_colour_its_hue() {
    // A dim warm floor under firelight. Under the old pivot its green and
    // blue went to zero and it came out pure red.
    let input = [0.06, 0.03, 0.012];
    let Some(out) = run(grade(1.14, 1.0), input) else { return };
    assert!(out[1] > 0.005 && out[2] > 0.001, "a channel was crushed to black: {out:?}");
    let ratio_in = input[0] / input[1];
    let ratio_out = out[0] / out[1];
    // Log-space contrast raises each channel to the same power, so the
    // red:green ratio grows a little (that is what contrast is) - but it must
    // stay the same colour, not triple.
    assert!(ratio_out < ratio_in * 1.3, "hue shifted from r/g {ratio_in:.2} to {ratio_out:.2}");
}
