//! **A camera can draw into a texture** (`camera.target`, Sep 2026): every
//! active camera with a target is a view, drawn each frame before the main
//! camera through the same world pass - into its own lighting and post
//! buffers, the post chain writing its texture - so any sprite, mesh or UI
//! image naming that texture shows what it sees. There was one camera a
//! frame before, so a monitor, a minimap and split-screen were all
//! impossible. Found by the showcase's Two Views.
#![cfg(feature = "gpu-wgpu")]

mod support;

use std::collections::HashMap;

use glam::{Vec2, Vec4};
use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::{Camera2DComponent, Transform};
use jengine_core::ecs::views::render_target_cameras;
use jengine_core::ecs::wgpu_systems::{apply_pixel_plan, sync_view_targets};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuSpriteBatch, WgpuTexture};
use jengine_core::renderer::postprocess::PostProcessSettings;

fn hosts() -> [(&'static str, String); 3] {
    [
    ("native", support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs"))),
    ("viewport", support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs"))),
    ("web", support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs"))),
]
}

#[test]
fn every_runtime_draws_its_views_before_the_screen() {
    for (name, src) in hosts() {
        let views = src.find("render_target_cameras(").unwrap_or_else(|| panic!("{name} draws no views"));
        let sync = src.find("sync_view_targets(").unwrap_or_else(|| panic!("{name} never makes the views' buffers"));
        let looped = src.find("for which in (0..views.len()).map(Some).chain([None])").unwrap_or_else(|| {
            panic!("{name} does not draw each view through its world pass, then the screen")
        });
        assert!(views < looped && sync < looped, "{name}: the views are found and synced before the world pass");
        let after = &src[looped..];
        assert!(
            after.contains("post.execute(queue, &mut frame.encoder, &tex.view)"),
            "{name}: a view's post chain writes its texture"
        );
    }
}

/// A red box seen by a view camera at (500, 0); then, on the main target, a
/// sprite showing the view's texture. The main target's centre pixel.
#[test]
fn a_sprite_shows_what_a_view_camera_sees() {
    let Some(gpu) = HeadlessGpu::new(64, 64) else {
        eprintln!("cameras_draw_into_textures: no GPU adapter - skipped");
        return;
    };
    let mut world = World::new();
    world.spawn((
        Camera2DComponent {
            is_active: true,
            viewport_width: 64.0,
            viewport_height: 64.0,
            target: "views/far".into(),
            target_size: [32, 32],
            ..Default::default()
        },
        Transform { position: Vec2::new(500.0, 0.0), ..Default::default() },
    ));
    let views = render_target_cameras(&world, (64, 64));
    assert_eq!(views.len(), 1);
    let mut assets = AssetManager::new();
    let mut targets = HashMap::new();
    sync_view_targets(&mut targets, &views, &gpu.device, &mut assets, gpu.color_format, &PostProcessSettings::default());
    let view = &views[0];
    let t = targets.get_mut(&view.target).expect("the view's buffers");
    let cam = apply_pixel_plan(view.camera, view.size, &gpu.device, &mut t.post, &mut t.light);

    let white = WgpuTexture::from_rgba(&gpu.device, &gpu.queue, 1, 1, &[255, 255, 255, 255]).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("batch");
    let mut encoder = gpu.encoder();
    // The view: a red box where it looks, nothing where the screen looks.
    {
        let mut pass = t.post.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        batch.begin_frame();
        batch.begin();
        batch.draw_sprite(Vec2::new(500.0, 0.0), Vec2::new(40.0, 40.0), 0.0, Vec4::new(1.0, 0.0, 0.0, 1.0));
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &cam.view_projection(), &white);
    }
    let tex = assets.get_wgpu_texture("views/far").expect("the view's texture is an asset");
    t.post.execute(&gpu.queue, &mut encoder, &tex.view);
    // The screen: one sprite, textured with the view.
    {
        let screen = jengine_core::renderer::camera::Camera2D::new(64.0, 64.0);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("screen"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: gpu.view(),
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        batch.begin();
        batch.draw_sprite(Vec2::ZERO, Vec2::new(64.0, 64.0), 0.0, Vec4::ONE);
        batch.flush(&gpu.device, &gpu.queue, &mut pass, &screen.view_projection(), tex);
    }
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let px = &img.rgba[((32 * 64 + 32) * 4) as usize..][..4];
    assert!(px[0] > 200 && px[1] < 40 && px[2] < 40, "the screen shows the view's red box: {px:?}");
    let corner = &img.rgba[((2 * 64 + 2) * 4) as usize..][..4];
    assert!(corner[0] < 40, "and the black around it: {corner:?}");
}
