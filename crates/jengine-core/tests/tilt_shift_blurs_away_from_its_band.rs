//! **`tilt_shift` blurs the frame away from a band, and `blur` all of it**
//! (Sep 2026) - there was no blur of any kind in the post stack, so a
//! miniature town could not look like one and a menu could not soften the
//! world behind it. The band is a **world** height, put on the screen
//! through the frame's view.
//!
//! And a patroller with `face_velocity` turns to the way it walks - patrol
//! writes no velocity, so it used to slide round every corner facing
//! wherever it was authored. Both found by the showcase's Tilt-Shift
//! Diorama.
#![cfg(all(feature = "gpu-wgpu", feature = "tilt_shift"))]

use std::collections::HashMap;

use glam::{Mat4, Vec2, Vec4};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuPostProcessStack, WgpuSpriteBatch, WgpuTexture};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::material::UniformValue;
use jengine_core::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings};

const W: u32 = 256;
const H: u32 = 128;

fn effect(name: &str, params: &[(&str, f32)]) -> PostProcessEffectSettings {
    let params: HashMap<String, UniformValue> =
        params.iter().map(|(k, v)| (k.to_string(), UniformValue::Float(*v))).collect();
    PostProcessEffectSettings { name: name.to_string(), enabled: true, params }
}

/// Vertical stripes, two pixels white and two black, over the whole frame;
/// the contrast (max - min of the red channel) along row `y`.
fn contrast(effects: Vec<PostProcessEffectSettings>, rows: &[u32]) -> Option<Vec<i32>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("tilt_shift_blurs_away_from_its_band: no GPU adapter - skipped");
        None
    })?;
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, gpu.color_format, gpu.color_format).expect("stack");
    stack.set_settings(PostProcessSettings { effects, ..Default::default() });
    jengine_core::renderer::frame_view::set_frame_view(&Camera2D::new(W as f32, H as f32));
    let white = WgpuTexture::from_rgba(&gpu.device, &gpu.queue, 1, 1, &[255, 255, 255, 255]).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("batch");
    let vp = Mat4::orthographic_rh_gl(-(W as f32) / 2.0, W as f32 / 2.0, -(H as f32) / 2.0, H as f32 / 2.0, -1.0, 1.0);
    let mut encoder = gpu.encoder();
    {
        let mut pass = stack.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        batch.begin_frame();
        batch.begin();
        let mut x = -(W as f32) / 2.0 + 1.0;
        while x < W as f32 / 2.0 {
            batch.draw_sprite(Vec2::new(x, 0.0), Vec2::new(2.0, H as f32), 0.0, Vec4::ONE);
            x += 4.0;
        }
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, &white);
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    jengine_core::renderer::frame_view::clear_frame_view();
    let img = gpu.read_rgba();
    Some(
        rows.iter()
            .map(|&y| {
                let row: Vec<i32> = (40..W - 40).map(|x| img.rgba[((y * W + x) * 4) as usize] as i32).collect();
                row.iter().max().unwrap() - row.iter().min().unwrap()
            })
            .collect(),
    )
}

#[test]
fn tilt_shift_is_sharp_in_its_band_and_soft_away_from_it() {
    let (middle, top) = (H / 2, 6);
    let Some(off) = contrast(vec![], &[middle, top]) else { return };
    assert!(off[0] > 200 && off[1] > 200, "stripes are crisp without the effect: {off:?}");

    // The band at world y = 0 - the middle row - ten units either side.
    let tilt = effect("tilt_shift", &[("focus_y", 0.0), ("band", 10.0), ("falloff", 20.0), ("max_blur", 8.0)]);
    let Some(on) = contrast(vec![tilt], &[middle, top]) else { return };
    assert!(on[0] > 200, "the band stays sharp: {}", on[0]);
    assert!(on[1] < 60, "far from it the stripes blur to grey: {}", on[1]);

    // Moved to the top of the frame (world y = 58), the band follows.
    let tilt = effect("tilt_shift", &[("focus_y", 58.0), ("band", 10.0), ("falloff", 20.0), ("max_blur", 8.0)]);
    let Some(moved) = contrast(vec![tilt], &[middle, top]) else { return };
    assert!(moved[1] > 200 && moved[0] < 60, "the band is a world height: {moved:?}");

    let blur = effect("blur", &[("radius", 6.0)]);
    let Some(all) = contrast(vec![blur], &[middle, top]) else { return };
    assert!(all[0] < 60 && all[1] < 60, "blur softens everything: {all:?}");
}

/// `blur` with a focus rectangle: sharp inside it, soft outside - the room
/// you are in, the world beyond it out of focus (Oct 2026, the showcase's
/// The Lit House). The rectangle is in world units, through the view.
#[test]
fn blur_keeps_its_focus_rectangle_sharp() {
    let (middle, top) = (H / 2, 6);
    // A rectangle 80 wide and 40 high about the world origin - the middle of
    // the frame - with a short falloff.
    let focus = effect(
        "blur",
        &[("radius", 6.0), ("focus_x", 0.0), ("focus_y", 0.0), ("focus_w", 80.0), ("focus_h", 40.0), ("falloff", 4.0)],
    );
    let Some(img) = frame(vec![focus]) else { return };
    let span = |y: u32, x0: u32, x1: u32| {
        let row: Vec<i32> = (x0..x1).map(|x| img[((y * W + x) * 4) as usize] as i32).collect();
        row.iter().max().unwrap() - row.iter().min().unwrap()
    };
    let (cx, half) = (W / 2, 30);
    let inside = span(middle, cx - half, cx + half);
    assert!(inside > 200, "inside the rectangle stays sharp: {inside}");
    let beside = span(middle, 4, 30);
    assert!(beside < 60, "beside it, blurred: {beside}");
    let above = span(top, cx - half, cx + half);
    assert!(above < 60, "above it too: {above}");
}

/// The stripes through the post stack, as RGBA.
fn frame(effects: Vec<PostProcessEffectSettings>) -> Option<Vec<u8>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("blur_keeps_its_focus_rectangle_sharp: no GPU adapter - skipped");
        None
    })?;
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, gpu.color_format, gpu.color_format).expect("stack");
    stack.set_settings(PostProcessSettings { effects, ..Default::default() });
    jengine_core::renderer::frame_view::set_frame_view(&Camera2D::new(W as f32, H as f32));
    let white = WgpuTexture::from_rgba(&gpu.device, &gpu.queue, 1, 1, &[255, 255, 255, 255]).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("batch");
    let vp = Mat4::orthographic_rh_gl(-(W as f32) / 2.0, W as f32 / 2.0, -(H as f32) / 2.0, H as f32 / 2.0, -1.0, 1.0);
    let mut encoder = gpu.encoder();
    {
        let mut pass = stack.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        batch.begin_frame();
        batch.begin();
        let mut x = -(W as f32) / 2.0 + 1.0;
        while x < W as f32 / 2.0 {
            batch.draw_sprite(Vec2::new(x, 0.0), Vec2::new(2.0, H as f32), 0.0, Vec4::ONE);
            x += 4.0;
        }
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, &white);
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    jengine_core::renderer::frame_view::clear_frame_view();
    Some(gpu.read_rgba().rgba)
}

#[test]
fn a_patroller_with_face_velocity_turns_to_the_way_it_walks() {
    use hecs::World;
    use jengine_core::ecs::behaviors::update_patrol_paths;
    use jengine_core::ecs::components::{FaceVelocity, PatrolPathRef, Transform};
    use jengine_core::ecs::path::{Path2D, Path2DLibrary};

    let mut lib = Path2DLibrary::new();
    lib.insert("corner", Path2D::new([Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)]));
    let mut world = World::new();
    let patrol = PatrolPathRef { path_id: "corner".into(), speed: 100.0, index: 1, ..Default::default() };
    let snap = world.spawn((Transform::default(), patrol.clone(), FaceVelocity::default()));
    let smooth = world.spawn((Transform::default(), patrol, FaceVelocity { turn_rate: 2.0, ..Default::default() }));
    update_patrol_paths(&mut world, Some(&lib), 0.5);
    assert!(world.get::<&Transform>(snap).unwrap().rotation.abs() < 1e-4, "east along the first leg");
    // Past the corner: heading north.
    for _ in 0..6 {
        update_patrol_paths(&mut world, Some(&lib), 0.1);
    }
    let r = world.get::<&Transform>(snap).unwrap().rotation;
    assert!((r - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "turned north at once: {r}");
    let r = world.get::<&Transform>(smooth).unwrap().rotation;
    assert!(r > 0.1 && r < std::f32::consts::FRAC_PI_2 - 0.1, "turning at 2 rad/s, part way round: {r}");
}
