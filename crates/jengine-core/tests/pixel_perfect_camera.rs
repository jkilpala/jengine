//! **A pixel-perfect camera draws the world at the art's resolution**
//! (`Camera2D::pixel_size`, Sep 2026). `pixelate` got the look by
//! point-sampling a full-resolution frame - full cost, and a slow pan
//! shimmered - and nothing could draw the world small. Now a camera with a
//! `pixel_size` draws it into a target of one texel per art pixel, on the
//! art's grid, and the output pass scales it up nearest, shifted by what the
//! grid snap took off so a pan stays smooth. Found by the showcase's Pixel
//! Perfect II.
#![cfg(feature = "gpu-wgpu")]

mod support;

use glam::{Vec2, Vec4};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuPostProcessStack, WgpuSpriteBatch, WgpuTexture};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::pixel_perfect;

const W: u32 = 256;
const H: u32 = 128;
fn hosts() -> [(&'static str, String); 3] {
    [
    ("native", support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs"))),
    ("viewport", support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs"))),
    ("web", support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs"))),
]
}

#[test]
fn every_runtime_draws_the_world_through_the_pixel_plan() {
    for (name, src) in hosts() {
        let at = src.find("apply_pixel_plan(").unwrap_or_else(|| panic!("{name} never sizes its world for the camera"));
        // The world pass is a loop over the views and then the screen
        // (`ecs::views`); each pass plans its own camera, and the world's
        // view-projection is that camera's.
        let looped = src[at..].find("for which in (0..views.len())").unwrap_or_else(|| panic!("{name}: no world loop"));
        let body = &src[at + looped..];
        let view_plan = body.find("apply_pixel_plan(").unwrap_or(usize::MAX);
        let vp = body.find("let view_projection = camera.view_projection();").unwrap_or(usize::MAX);
        assert!(view_plan < vp && vp < 2000, "{name}: each pass draws through its planned camera");
    }
}

/// A turned white box at (10, 5) on black, seen by `camera`; red channel.
fn render(camera: Camera2D) -> Option<Vec<u8>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("pixel_perfect_camera: no GPU adapter - skipped");
        None
    })?;
    let plan = pixel_perfect::plan(&camera);
    let (tw, th) = plan.map_or((W, H), |p| p.size);
    let mut stack = WgpuPostProcessStack::new(&gpu.device, tw, th, gpu.color_format, gpu.color_format).expect("stack");
    stack.set_source(plan.map(|p| p.source));
    let draw = plan.map_or(camera, |p| p.camera);
    let white = WgpuTexture::from_rgba(&gpu.device, &gpu.queue, 1, 1, &[255, 255, 255, 255]).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("batch");
    let mut encoder = gpu.encoder();
    {
        let mut pass = stack.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        batch.begin_frame();
        batch.begin();
        batch.draw_sprite(Vec2::new(10.0, 5.0), Vec2::new(40.0, 30.0), 0.5, Vec4::ONE);
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &draw.view_projection(), &white);
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    Some(img.rgba.chunks(4).map(|p| p[0]).collect())
}

fn camera(x: f32, pixel_size: f32) -> Camera2D {
    let mut c = Camera2D::new(W as f32, H as f32);
    c.position = Vec2::new(x, 0.0);
    c.pixel_size = pixel_size;
    c
}

/// How many 4 x 4 blocks, starting `ox` pixels in, are not one colour -
/// give or take the output pass's dither, which is per display pixel.
fn mixed_blocks(img: &[u8], ox: u32) -> usize {
    let mut mixed = 0;
    let mut bx = ox;
    while bx + 4 <= W {
        for by in (0..H).step_by(4) {
            let first = img[(by * W + bx) as usize];
            let uniform = (0..4).all(|dy| (0..4).all(|dx| img[((by + dy) * W + bx + dx) as usize].abs_diff(first) <= 3));
            if !uniform {
                mixed += 1;
            }
        }
        bx += 4;
    }
    mixed
}

fn lit(img: &[u8]) -> usize {
    img.iter().filter(|&&v| v > 127).count()
}

#[test]
fn the_world_is_drawn_in_whole_art_pixels_where_the_plain_camera_draws_it() {
    let Some(plain) = render(camera(0.0, 0.0)) else { return };
    let Some(pixels) = render(camera(0.0, 4.0)) else { return };
    assert!(mixed_blocks(&plain, 0) > 10, "a turned box drawn at the screen's resolution has fine edges");
    assert_eq!(mixed_blocks(&pixels, 0), 0, "at 4 units a pixel every 4 x 4 block is one art pixel");
    let (a, b) = (lit(&plain) as f32, lit(&pixels) as f32);
    assert!((a - b).abs() / a < 0.12, "and the box is where it was, the same size: {a} against {b} pixels");
}

#[test]
fn half_a_pixel_of_pan_moves_the_picture_half_a_pixel() {
    // Two world units right is two screen pixels left: the art pixels stay
    // whole, and their grid moves by exactly that.
    let Some(img) = render(camera(2.0, 4.0)) else { return };
    assert_eq!(mixed_blocks(&img, 2), 0, "the blocks sit two pixels over");
    assert!(mixed_blocks(&img, 0) > 0, "not where they sat before");
}
