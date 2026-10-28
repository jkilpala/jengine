//! **Depth of field by layer** (Oct 2026): the `depth_of_field` post effect
//! blurs each pixel by how far its layer's z is from `focus`, reading the z
//! the velocity pass (`render_velocity_wgpu`) writes into its blue channel.
//! Two rules make it look like a lens rather than a blur: a sharp layer does
//! not leak into the soft ones round it, and a soft layer behind does not
//! bleed over a sharp one in front. Found by the showcase's Rack Focus.
#![cfg(feature = "depth_of_field")]

mod support;

#[test]
fn every_host_draws_the_velocity_pass_for_depth_of_field() {
    let src = support::with_frame_draw(include_str!("../src/ecs/frame_draw.rs"));
    assert!(src.contains("post.depth_of_field_on()"), "the velocity pass runs for depth of field too");
    assert!(src.contains("post.depth_clear()"), "where no sprite covers, the depth is unknown");
    let shader = include_str!("../assets/shaders/wgsl/velocity.wgsl");
    assert!(shader.contains("in.color.b"), "the velocity pass writes the sprite's z");
}

/// One row of the frame through the `depth_of_field` effect: `squares` are
/// `(x, half width, z, grey)`, drawn in z order over black.
#[cfg(feature = "gpu-wgpu")]
fn row_through_depth_of_field(squares: &[(f32, f32, i32, f32)], focus: f32) -> Option<Vec<u8>> {
    use std::collections::HashMap;

    use glam::Vec2;
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

    const W: u32 = 160;
    const H: u32 = 48;
    let gpu = HeadlessGpu::new(W, H)?;
    let scene_format = wgpu::TextureFormat::Rgba16Float;
    let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, scene_format, gpu.color_format).expect("post stack");
    let mut params = HashMap::new();
    params.insert("focus".to_string(), UniformValue::Float(focus));
    params.insert("range".to_string(), UniformValue::Float(0.0));
    params.insert("strength".to_string(), UniformValue::Float(0.5));
    params.insert("max_radius".to_string(), UniformValue::Float(10.0));
    stack.set_settings(PostProcessSettings {
        effects: vec![PostProcessEffectSettings { name: "depth_of_field".into(), enabled: true, params }],
        ..Default::default()
    });
    assert!(stack.depth_of_field_on() && !stack.motion_blur_on());
    assert!(stack.depth_clear() < -60000.0, "empty space is cleared to an unknown depth");
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("white", &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, scene_format).unwrap();
    let mut world = World::new();
    let mut sorted = squares.to_vec();
    sorted.sort_by_key(|s| s.2);
    for &(x, half, z, grey) in &sorted {
        world.spawn((
            Transform { position: Vec2::new(x, 0.0), ..Default::default() },
            SpriteComponent {
                texture_id: "white".into(),
                size: Some([half * 2.0, 40.0]),
                color: [grey, grey, grey, 1.0],
                z_order: z,
                ..Default::default()
            },
        ));
    }
    let camera = Camera2D::new(W as f32, H as f32);
    jengine_core::renderer::frame_view::set_frame_view(&camera);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    batch.begin_frame();
    batch.set_target_format(&gpu.device, scene_format);
    {
        let mut pass = stack.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        batch.begin();
        for &(x, half, _, grey) in &sorted {
            batch.draw_sprite_uv(Vec2::new(x, 0.0), Vec2::new(half * 2.0, 40.0), 0.0, glam::Vec4::new(grey, grey, grey, 1.0), [0.0, 0.0, 1.0, 1.0]);
        }
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture("white").unwrap());
    }
    let (camera_motion, _) = begin_velocity(&camera);
    let pipeline = batch.velocity_pipeline(&gpu.device);
    batch.set_target_format(&gpu.device, VELOCITY_FORMAT);
    {
        let clear = wgpu::Color {
            r: camera_motion[0] as f64,
            g: camera_motion[1] as f64,
            b: stack.depth_clear() as f64,
            a: 0.0,
        };
        let mut vpass = stack.velocity_target().begin_pass(&mut encoder, Some(clear));
        render_velocity_wgpu(&world, &SortLayers::default(), &mut batch, Vec2::splat(16.0), &camera, 1.0, |b, tex| {
            b.flush_with_pipeline(&gpu.device, &gpu.queue, &mut vpass, &vp, assets.get_wgpu_texture(tex).unwrap(), &pipeline);
        });
    }
    stack.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    jengine_core::renderer::frame_view::clear_frame_view();
    Some((0..W).map(|x| img.rgba[(((H / 2) * W + x) * 4) as usize]).collect())
}

/// Part-lit pixels in `row[lo..hi]`: an edge in focus has none or one.
#[cfg(feature = "gpu-wgpu")]
fn soft(row: &[u8], lo: usize, hi: usize) -> usize {
    row[lo..hi].iter().filter(|&&r| r > 20 && r < 235).count()
}

/// Three white squares: one at the focus, one in front of it, one behind.
/// The one in focus keeps hard edges; both others go soft.
#[cfg(feature = "gpu-wgpu")]
#[test]
fn the_layer_in_focus_is_sharp_and_the_others_blur() {
    // Screen x = 80 + world x; each square 20 wide.
    let Some(row) = row_through_depth_of_field(&[(-50.0, 10.0, 0, 1.0), (0.0, 10.0, 20, 1.0), (50.0, 10.0, -20, 1.0)], 0.0)
    else {
        eprintln!("depth_of_field_blurs_by_layer: no GPU adapter - skipped");
        return;
    };
    let (focused, near, far) = (soft(&row, 10, 50), soft(&row, 55, 105), soft(&row, 105, 155));
    assert!(focused <= 2, "the layer in focus is sharp: {focused} part-lit in {:?}", &row[10..50]);
    assert!(near >= 8, "a layer in front is soft: {near} part-lit in {:?}", &row[55..105]);
    assert!(far >= 8, "a layer behind is soft: {far} part-lit in {:?}", &row[105..155]);
    // Rack the focus to the near square: now it is the sharp one.
    let row = row_through_depth_of_field(&[(-50.0, 10.0, 0, 1.0), (0.0, 10.0, 20, 1.0), (50.0, 10.0, -20, 1.0)], 20.0).unwrap();
    assert!(soft(&row, 55, 105) <= 2, "focus pulled to the near layer: {:?}", &row[55..105]);
    assert!(soft(&row, 10, 50) >= 8, "and the old one goes soft: {:?}", &row[10..50]);
}

/// A sharp white square in front of a wide grey band far behind it: the
/// band is soft, but it does not bleed over the square's edges - and the
/// square, sharp, does not leak out into the band.
#[cfg(feature = "gpu-wgpu")]
#[test]
fn a_soft_layer_behind_does_not_bleed_over_a_sharp_one() {
    let Some(row) = row_through_depth_of_field(&[(0.0, 70.0, -30, 0.4), (0.0, 12.0, 0, 1.0)], 0.0) else {
        eprintln!("depth_of_field_blurs_by_layer: no GPU adapter - skipped");
        return;
    };
    // The square spans screen x 68..92; the band 10..150, grey ~170 in sRGB.
    let white = |r: u8| r >= 245;
    assert!(row[69..91].iter().all(|&r| white(r)), "the square is white to its edges: {:?}", &row[60..100]);
    let band = |r: u8| (140..=200).contains(&r);
    assert!(band(row[64]) && band(row[96]), "right outside it is the band, not a white halo: {:?}", &row[60..100]);
}
