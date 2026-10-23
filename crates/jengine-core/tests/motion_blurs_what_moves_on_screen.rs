//! **Motion blur** (Oct 2026): the `motion_blur` post effect smears each
//! pixel along its motion across the screen since the last frame, read from a
//! velocity pass (`render_velocity_wgpu`) - each sprite's own motion
//! combined with the camera's. So when the camera pans with a car, the car
//! stays sharp and the street it passes streaks; a reprojection of the
//! camera alone would blur the car as much as the street. Found by the
//! showcase's Night Pursuit.

mod support;

use glam::Vec2;
use jengine_core::ecs::wgpu_systems::view_uv;

#[test]
fn a_point_on_the_view_is_its_screen_uv() {
    let cam = jengine_core::renderer::camera::Camera2D::new(200.0, 100.0);
    let c = cam.view_corners();
    assert!(view_uv(&c, Vec2::ZERO).distance(Vec2::splat(0.5)) < 1e-5, "the middle is the middle");
    assert!(view_uv(&c, Vec2::new(-100.0, 50.0)).distance(Vec2::ZERO) < 1e-5, "top left is 0, 0 (y down)");
    assert!(view_uv(&c, Vec2::new(100.0, -50.0)).distance(Vec2::ONE) < 1e-5, "bottom right is 1, 1");
    // A turned camera: its own corners are still 0 and 1.
    let mut turned = cam;
    turned.rotation = 0.7;
    let t = turned.view_corners();
    assert!(view_uv(&t, t[1]).distance(Vec2::new(1.0, 0.0)) < 1e-4);
    assert!(view_uv(&t, t[3]).distance(Vec2::new(0.0, 1.0)) < 1e-4);
}

#[test]
fn every_host_draws_the_velocity_pass() {
    let src = support::with_frame_draw(include_str!("../src/ecs/frame_draw.rs"));
    assert!(src.contains("post.motion_blur_on()") && src.contains("wgpu_systems::render_velocity_wgpu("));
}

/// The camera pans 12 units a frame. One white square rides with it (the
/// followed car), one stands still (the street). After a frame of history,
/// with motion blur on, the still one's edges are smeared across many pixels
/// and the followed one's are not.
#[cfg(feature = "gpu-wgpu")]
#[test]
fn what_the_camera_follows_stays_sharp_and_the_rest_streaks() {
    use std::collections::HashMap;

    use hecs::World;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::ecs::components::{SpriteComponent, Transform};
    use jengine_core::ecs::wgpu_systems::{begin_velocity, render_velocity_wgpu};
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::backend::wgpu::{VELOCITY_FORMAT, WgpuPostProcessStack, WgpuSpriteBatch};
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::material::UniformValue;
    use jengine_core::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings};

    const W: u32 = 128;
    const H: u32 = 64;
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("motion_blurs_what_moves_on_screen: no GPU adapter - skipped");
        return;
    };
    let scene_format = wgpu::TextureFormat::Rgba16Float;
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, scene_format, gpu.color_format).expect("post stack");
    let mut params = HashMap::new();
    params.insert("shutter".to_string(), UniformValue::Float(1.0));
    params.insert("max_length".to_string(), UniformValue::Float(0.15));
    stack.set_settings(PostProcessSettings {
        effects: vec![PostProcessEffectSettings { name: "motion_blur".into(), enabled: true, params }],
        ..Default::default()
    });
    assert!(stack.motion_blur_on());
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("white", &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, scene_format).unwrap();
    let mut world = World::new();
    let square = SpriteComponent { texture_id: "white".into(), size: Some([16.0, 16.0]), ..Default::default() };
    let followed = world.spawn((Transform::default(), square.clone()));
    let still = world.spawn((Transform::default(), square));

    let mut row = Vec::new();
    for frame in 0..2 {
        let cam_x = frame as f32 * 12.0;
        let mut camera = Camera2D::new(W as f32, H as f32);
        camera.position = Vec2::new(cam_x, 0.0);
        world.get::<&mut Transform>(followed).unwrap().position = Vec2::new(cam_x - 30.0, 0.0);
        world.get::<&mut Transform>(still).unwrap().position = Vec2::new(30.0, 0.0);
        jengine_core::renderer::frame_view::set_frame_view(&camera);
        let vp = camera.view_projection();
        let mut encoder = gpu.encoder();
        batch.begin_frame();
        batch.set_target_format(&gpu.device, scene_format);
        {
            let mut pass = stack.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
            batch.begin();
            for e in [followed, still] {
                let p = world.get::<&Transform>(e).unwrap().position;
                batch.draw_sprite_uv(p, Vec2::splat(16.0), 0.0, glam::Vec4::ONE, [0.0, 0.0, 1.0, 1.0]);
            }
            batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture("white").unwrap());
        }
        // `begin_velocity` scales the camera's motion by the wall time since
        // the last frame; take that back out, as the sprites below are taken
        // at a fixed sixtieth. A software renderer (CI's WARP) is a tenth of
        // a second a frame, which shrank the camera's streak to nothing.
        let (camera_motion, scale) = begin_velocity(&camera);
        let camera_motion = [camera_motion[0] / scale, camera_motion[1] / scale];
        let pipeline = batch.velocity_pipeline(&gpu.device);
        batch.set_target_format(&gpu.device, VELOCITY_FORMAT);
        {
            let clear = wgpu::Color { r: camera_motion[0] as f64, g: camera_motion[1] as f64, b: 0.0, a: 0.0 };
            let mut vpass = stack.velocity_target().begin_pass(&mut encoder, Some(clear));
            // A fixed sixtieth of a second, whatever the test's own pace.
            render_velocity_wgpu(&world, &SortLayers::default(), &mut batch, Vec2::splat(16.0), &camera, 1.0, |b, tex| {
                b.flush_with_pipeline(&gpu.device, &gpu.queue, &mut vpass, &vp, assets.get_wgpu_texture(tex).unwrap(), &pipeline);
            });
        }
        stack.execute(&gpu.queue, &mut encoder, gpu.view());
        gpu.submit(encoder);
        let img = gpu.read_rgba();
        row = (0..W).map(|x| img.rgba[(((H / 2) * W + x) * 4) as usize]).collect();
        jengine_core::renderer::frame_view::clear_frame_view();
    }
    // Screen x of the followed square is 64 - 30 = 34, the still one's 94;
    // count the part-lit pixels round each.
    let partial = |lo: usize, hi: usize| row[lo..hi].iter().filter(|&&r| r > 20 && r < 235).count();
    let (sharp, smeared) = (partial(16, 54), partial(70, 124));
    assert!(sharp <= 3, "the followed square is sharp: {sharp} part-lit pixels in {:?}", &row[16..54]);
    assert!(smeared >= 8, "the still square streaks as the camera passes it: {smeared} part-lit in {:?}", &row[70..124]);
}
