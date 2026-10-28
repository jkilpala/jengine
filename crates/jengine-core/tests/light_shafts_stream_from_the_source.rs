//! `light_shafts` streams light from a source through the frame: a pixel
//! below a bright thing, on the line to the source, gathers its light; a
//! pixel whose line to the source crosses nothing bright gathers none. The
//! source is a **world** point, put on the screen through the frame's view.
//!
//! The showcase's Forest God Rays is built on it. Without a GPU adapter it
//! prints why and passes.
#![cfg(all(feature = "gpu-wgpu", feature = "light_shafts"))]

use std::collections::HashMap;

use glam::{Mat4, Vec2, Vec4};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuPostProcessStack, WgpuSpriteBatch, WgpuTexture};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::material::UniformValue;
use jengine_core::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings};

const W: u32 = 256;
const H: u32 = 128;

fn shafts(source: (f32, f32)) -> PostProcessEffectSettings {
    let mut params = HashMap::new();
    for (k, v) in [
        ("source_x", source.0),
        ("source_y", source.1),
        ("length", 3.0),
        ("intensity", 8.0),
        ("decay", 0.98),
        ("threshold", 0.5),
    ] {
        params.insert(k.to_string(), UniformValue::Float(v));
    }
    PostProcessEffectSettings { name: "light_shafts".to_string(), enabled: true, params }
}

/// A white 16 x 16 square at world (0, 40) on black; the output's red
/// channel (0..255) at each of `points`.
fn frame(effects: Vec<PostProcessEffectSettings>, points: &[(f32, f32)]) -> Option<Vec<u8>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("light_shafts_stream_from_the_source: no GPU adapter - skipped");
        None
    })?;
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, gpu.color_format, gpu.color_format).expect("stack");
    stack.set_settings(PostProcessSettings { effects, ..Default::default() });
    // The frame's view, as a host sets it: one world unit a pixel, centred.
    jengine_core::renderer::frame_view::set_frame_view(&Camera2D::new(W as f32, H as f32));
    let white = WgpuTexture::from_rgba(&gpu.device, &gpu.queue, 1, 1, &[255, 255, 255, 255]).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("batch");
    let vp = Mat4::orthographic_rh_gl(-(W as f32) / 2.0, W as f32 / 2.0, -(H as f32) / 2.0, H as f32 / 2.0, -1.0, 1.0);
    let mut encoder = gpu.encoder();
    {
        let mut pass = stack.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        batch.begin_frame();
        batch.begin();
        batch.draw_sprite(Vec2::new(0.0, 40.0), Vec2::splat(16.0), 0.0, Vec4::ONE);
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, &white);
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    jengine_core::renderer::frame_view::clear_frame_view();
    let img = gpu.read_rgba();
    Some(
        points
            .iter()
            .map(|&(x, y)| {
                let px = (x + W as f32 / 2.0) as u32;
                let py = (H as f32 / 2.0 - y) as u32;
                img.rgba[((py * W + px) * 4) as usize]
            })
            .collect(),
    )
}

#[test]
fn light_streams_from_the_source_through_what_is_bright() {
    let below = (0.0, -30.0);
    let aside = (110.0, -30.0);
    let Some(off) = frame(vec![], &[below, aside]) else { return };
    assert_eq!(off, vec![0, 0], "without the effect, black is black");

    // A sun above the frame, straight over the square.
    let Some(on) = frame(vec![shafts((0.0, 200.0))], &[below, aside]) else { return };
    assert!(on[0] > 20, "below the bright square, on its line to the source, light streams: {}", on[0]);
    assert!(on[1] < 4, "aside, the line to the source crosses nothing bright: {}", on[1]);

    // The source is a world point: moved far to the right, the square is no
    // longer on the line from the pixel below it.
    let Some(moved) = frame(vec![shafts((2000.0, 200.0))], &[below]) else { return };
    assert!(moved[0] < on[0] / 3, "the rays follow the source: {} against {}", moved[0], on[0]);
}
