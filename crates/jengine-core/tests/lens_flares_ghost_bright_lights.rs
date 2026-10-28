//! **Lens flares** (Oct 2026): the `lens_flare` post effect throws the
//! reflections a bright light leaves in a lens across the frame - ghosts on
//! the line through the centre, a halo, colour fringes, an anamorphic streak.
//! Screen space, per pixel: each pixel looks back through the centre at what
//! is bright there, so only what is past `threshold` flares. Found by the
//! showcase's Orbital Station, whose sun and explosions had bloom and nothing
//! of the lens.
#![cfg(feature = "lens_flare")]

/// A bright square and a dim one on black. With `lens_flare` on, the bright
/// one throws a soft ghost to the point mirrored through the centre and the
/// dim one throws nothing.
#[cfg(feature = "gpu-wgpu")]
#[test]
fn a_bright_light_throws_ghosts_through_the_centre() {
    use std::collections::HashMap;

    use glam::{Vec2, Vec4};
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::backend::wgpu::{WgpuPostProcessStack, WgpuSpriteBatch};
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::material::UniformValue;
    use jengine_core::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings};

    const W: u32 = 512;
    const H: u32 = 256;
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("lens_flares_ghost_bright_lights: no GPU adapter - skipped");
        return;
    };
    let scene_format = wgpu::TextureFormat::Rgba16Float;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("white", &gpu.device, &gpu.queue).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, scene_format).unwrap();
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();

    // Screen pixel (160, 96) is world (-96, 32); its mirror through the
    // centre is pixel (352, 160). The dim square at pixel (160, 176) mirrors
    // to (352, 80) - far enough from the bright ghost that its blur does not
    // reach.
    let mut render = |on: bool| {
        let mut stack = WgpuPostProcessStack::new(&gpu.device, W, H, scene_format, gpu.color_format).expect("post stack");
        let mut params = HashMap::new();
        params.insert("threshold".to_string(), UniformValue::Float(1.0));
        params.insert("intensity".to_string(), UniformValue::Float(1.0));
        params.insert("ghosts".to_string(), UniformValue::Float(1.0));
        params.insert("halo".to_string(), UniformValue::Float(0.0));
        params.insert("dispersion".to_string(), UniformValue::Float(0.0));
        stack.set_settings(PostProcessSettings {
            effects: vec![PostProcessEffectSettings { name: "lens_flare".into(), enabled: on, params }],
            ..Default::default()
        });
        let mut encoder = gpu.encoder();
        batch.begin_frame();
        batch.set_target_format(&gpu.device, scene_format);
        {
            let mut pass = stack.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
            batch.begin();
            batch.draw_sprite_uv(Vec2::new(-96.0, 32.0), Vec2::splat(24.0), 0.0, Vec4::new(8.0, 8.0, 8.0, 1.0), [0.0, 0.0, 1.0, 1.0]);
            batch.draw_sprite_uv(Vec2::new(-96.0, -48.0), Vec2::splat(24.0), 0.0, Vec4::new(0.6, 0.6, 0.6, 1.0), [0.0, 0.0, 1.0, 1.0]);
            batch.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture("white").unwrap());
        }
        stack.execute(&gpu.queue, &mut encoder, gpu.view());
        gpu.submit(encoder);
        gpu.read_rgba().rgba
    };
    let red = |img: &[u8], x: u32, y: u32| img[((y * W + x) * 4) as usize];
    let off = render(false);
    let on = render(true);
    assert!(red(&off, 352, 160) < 3, "nothing at the mirror without the effect: {}", red(&off, 352, 160));
    assert!(red(&on, 352, 160) > 40, "the bright square's ghost lands at its mirror: {}", red(&on, 352, 160));
    // Soft: the 24-pixel square's ghost is a blurred disc, lit well past
    // where the square's own edge would be.
    assert!(red(&on, 372, 160) > 10, "the ghost is soft, not a sharp copy: {}", red(&on, 372, 160));
    assert!(red(&on, 352, 80) < 3, "the dim square, under the threshold, throws nothing: {}", red(&on, 352, 80));
    assert!(red(&on, 256, 20) < 3, "and the rest of the frame stays dark: {}", red(&on, 256, 20));
}
