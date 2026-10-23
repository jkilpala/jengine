//! **Palettes** (Oct 2026; `renderer::palette`, the `palette` post
//! effect): the frame quantised to a handful of colours with ordered
//! dithering, last in the output pass. These run the real post stack on a
//! headless device - fill the scene with one colour, quantise it, read it
//! back - and compare with `palette::quantize`, the shader on the CPU.
//! Without a GPU adapter they print why and pass.
#![cfg(feature = "gpu-wgpu")]

use std::collections::{HashMap, HashSet};

use jengine_core::renderer::backend::wgpu::WgpuPostProcessStack;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::material::UniformValue;
use jengine_core::renderer::palette::{Palette, quantize};
use jengine_core::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings};

const W: u32 = 16;
const H: u32 = 16;

fn to_byte(linear: f32) -> u8 {
    let s = if linear <= 0.003_130_8 { linear * 12.92 } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

fn run(gpu: &HeadlessGpu, linear: [f64; 3], params: &HashMap<String, UniformValue>) -> Vec<[u8; 3]> {
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, wgpu::TextureFormat::Rgba16Float, gpu.color_format)
        .expect("post stack");
    let effects = vec![PostProcessEffectSettings { name: "palette".into(), enabled: true, params: params.clone() }];
    stack.set_settings(PostProcessSettings { effects, ..Default::default() });
    let mut encoder = gpu.encoder();
    {
        let clear = wgpu::Color { r: linear[0], g: linear[1], b: linear[2], a: 1.0 };
        let _pass = stack.scene_target().begin_pass(&mut encoder, Some(clear));
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    gpu.read_rgba().rgba.chunks(4).map(|p| [p[0], p[1], p[2]]).collect()
}

#[test]
fn the_frame_is_only_palette_colours_dithered_as_the_cpu_says() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("no GPU adapter - skipping");
        return;
    };
    // PICO-8, dithered in cells of one pixel.
    let mut params = HashMap::new();
    params.insert("preset".to_string(), UniformValue::Int(1));
    params.insert("dither".to_string(), UniformValue::Float(1.0));
    params.insert("cell".to_string(), UniformValue::Float(1.0));
    let pal = Palette::from_params(&params);
    let palette: HashSet<[u8; 3]> = pal.colors[..pal.count]
        .iter()
        .map(|c| [(c[0] * 255.0).round() as u8, (c[1] * 255.0).round() as u8, (c[2] * 255.0).round() as u8])
        .collect();
    let colour = [0.20f64, 0.35, 0.08];
    let px = run(&gpu, colour, &params);
    let mut seen = HashSet::new();
    for (i, p) in px.iter().enumerate() {
        assert!(palette.iter().any(|q| (0..3).all(|j| (q[j] as i32 - p[j] as i32).abs() <= 1)), "pixel {i} {p:?} is not PICO-8");
        // The CPU twin picks the same colour at the same pixel (centre).
        let (x, y) = ((i as u32 % W) as f32 + 0.5, (i as u32 / W) as f32 + 0.5);
        let cpu = quantize(&pal, [colour[0] as f32, colour[1] as f32, colour[2] as f32], [x, y], 0.0);
        let cpu = [to_byte(cpu[0]), to_byte(cpu[1]), to_byte(cpu[2])];
        assert!((0..3).all(|j| (cpu[j] as i32 - p[j] as i32).abs() <= 1), "pixel {i}: gpu {p:?}, cpu {cpu:?}");
        seen.insert(*p);
    }
    assert!(seen.len() >= 2, "a colour between two of the palette's is dithered from both: {seen:?}");
}
