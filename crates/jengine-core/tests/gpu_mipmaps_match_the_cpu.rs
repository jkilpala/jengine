//! A linear texture's mip levels are drawn on the GPU when it is uploaded
//! (`renderer::backend::wgpu::mipmap`, Oct 2026) - building them on the CPU
//! was 32 ms of a 50 ms first frame for a showcase scene in a browser. They
//! must be the levels the CPU makes (`texture_filter::mip_chain`): averaged
//! in linear light and weighted by alpha for colour, plainly for data,
//! clamped at an odd edge. Read back level by level, each texel within one
//! step a channel of the CPU's - the GPU's sRGB encode may round a step the
//! other way. Without a GPU adapter it prints why and passes.
#![cfg(feature = "gpu-wgpu")]

use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::mipmap::mip_level_count;
use jengine_core::renderer::backend::wgpu::{MipGenerator, WgpuTexture};
use jengine_core::renderer::texture_filter::{TextureFilter, mip_chain};

/// Every byte of a picture from a cheap hash of its place: noise, with some
/// texels fully clear and some half clear, so the alpha weighting and an
/// edge's clear texels are both exercised.
fn picture(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let n = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)).wrapping_mul(2_654_435_761);
            let a = match n % 5 {
                0 => 0,
                1 => 128,
                _ => 255,
            };
            out.extend_from_slice(&[(n >> 8) as u8, (n >> 16) as u8, (n >> 24) as u8, a]);
        }
    }
    out
}

/// One mip level of `texture`, read back tightly packed.
fn read_level(gpu: &HeadlessGpu, texture: &wgpu::Texture, level: u32) -> Vec<u8> {
    let (w, h) = ((texture.width() >> level).max(1), (texture.height() >> level).max(1));
    let row = (w * 4).div_ceil(256) * 256;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mip readback"),
        size: (row * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.encoder();
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture { texture, mip_level: level, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    gpu.submit(encoder);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    gpu.device.poll(wgpu::Maintain::Wait);
    let data = slice.get_mapped_range();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let start = (y * row) as usize;
        out.extend_from_slice(&data[start..start + (w * 4) as usize]);
    }
    out
}

fn check(gpu: &HeadlessGpu, mips: &MipGenerator, w: u32, h: u32, srgb: bool) {
    let rgba = picture(w, h);
    let tex = WgpuTexture::from_rgba_filtered_gpu_mips(&gpu.device, &gpu.queue, mips, w, h, &rgba, TextureFilter::Linear, srgb)
        .expect("upload");
    let cpu = mip_chain(w, h, &rgba, srgb);
    assert_eq!(tex.texture.mip_level_count(), 1 + cpu.len() as u32, "{w}x{h}: a level count the CPU chain does not have");
    assert_eq!(tex.texture.mip_level_count(), mip_level_count(w, h));
    for (k, (lw, lh, want)) in cpu.iter().enumerate() {
        let got = read_level(gpu, &tex.texture, k as u32 + 1);
        assert_eq!(got.len(), want.len(), "{w}x{h} level {}: {lw}x{lh} read as the wrong size", k + 1);
        let (at, worst) = got.iter().zip(want).map(|(a, b)| a.abs_diff(*b)).enumerate().max_by_key(|&(_, d)| d).unwrap();
        assert!(
            worst <= 1,
            "{w}x{h} {} level {} ({lw}x{lh}): texel {} channel {} is {} on the GPU and {} on the CPU",
            if srgb { "colour" } else { "data" },
            k + 1,
            at / 4,
            at % 4,
            got[at],
            want[at],
        );
    }
}

#[test]
fn gpu_mip_levels_are_the_cpus() {
    let Some(gpu) = HeadlessGpu::new(4, 4) else {
        eprintln!("gpu_mipmaps_match_the_cpu: no GPU adapter - skipped");
        return;
    };
    let mips = MipGenerator::new(&gpu.device);
    // Square and power of two; odd on both sides (an edge texel clamps); a
    // strip one texel tall; a column one texel wide.
    for (w, h) in [(64, 64), (37, 20), (300, 1), (1, 9)] {
        check(&gpu, &mips, w, h, true);
        check(&gpu, &mips, w, h, false);
    }
}
