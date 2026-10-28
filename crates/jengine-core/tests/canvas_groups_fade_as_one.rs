//! `canvas_group` (`ecs::canvas_group`): a hierarchy faded as one picture.
//! Each group is a view through the main camera that draws only its members
//! into a transparent texture; the main pass skips the members and draws the
//! texture once, at the group's z and opacity.

mod support;

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::canvas_group::{CanvasGroup, canvas_group_views, clear_members, group_key, member_of, set_canvas_group_field};
use jengine_core::ecs::components::{Parent, SpriteComponent, Transform};
use jengine_core::renderer::camera::{Camera2D, CameraPass};

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), ..Default::default() }
}

#[test]
fn each_outermost_visible_group_is_a_view_through_the_main_camera() {
    let mut world = World::new();
    let ghost = world.spawn((at(0.0, 0.0), CanvasGroup { opacity: 0.5, ..Default::default() }));
    let arm = world.spawn((at(10.0, 0.0), Parent(ghost)));
    let inner = world.spawn((at(0.0, 0.0), Parent(ghost), CanvasGroup::default()));
    let hand = world.spawn((at(0.0, 0.0), Parent(inner)));
    let hidden = world.spawn((at(0.0, 0.0), CanvasGroup { opacity: 0.0, ..Default::default() }));
    let loose = world.spawn((at(0.0, 0.0),));
    let mut main = Camera2D::new(640.0, 360.0);
    main.position = Vec2::new(40.0, 5.0);
    let views = canvas_group_views(&world, &main, (640, 360));
    assert_eq!(views.len(), 1, "the ghost: the inner group is part of it, the hidden one draws nothing");
    let v = &views[0];
    assert!(v.transparent);
    assert_eq!(v.camera.position, main.position, "through the main camera");
    assert_eq!(v.camera.pass, CameraPass::CanvasGroup(group_key(ghost)));
    for e in [ghost, arm, inner, hand] {
        assert_eq!(member_of(e), group_key(ghost), "everything under the ghost is in it");
    }
    assert_eq!(member_of(hidden), group_key(hidden));
    assert_eq!(member_of(loose), 0);
    clear_members();
}

#[cfg(feature = "gpu-wgpu")]
#[test]
fn each_pass_draws_its_own_share() {
    use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
    let drawn = |world: &World, e: hecs::Entity, cam: &Camera2D| {
        set_frame_view(cam);
        let seen = jengine_core::ecs::wgpu_systems::in_view_layers(world, e);
        clear_frame_view();
        seen
    };
    let mut world = World::new();
    let ghost = world.spawn((at(0.0, 0.0), CanvasGroup::default()));
    let part = world.spawn((at(0.0, 0.0), Parent(ghost)));
    let loose = world.spawn((at(0.0, 0.0),));
    let main = Camera2D::new(640.0, 360.0);
    canvas_group_views(&world, &main, (640, 360));
    let mut group_cam = main;
    group_cam.pass = CameraPass::CanvasGroup(group_key(ghost));
    let mut view_cam = main;
    view_cam.pass = CameraPass::View;
    assert!(!drawn(&world, part, &main) && drawn(&world, loose, &main), "the main pass draws the picture, not the parts");
    assert!(drawn(&world, part, &group_cam) && !drawn(&world, loose, &group_cam), "the group's pass only its own");
    assert!(drawn(&world, part, &view_cam) && drawn(&world, loose, &view_cam), "another camera everything");
    clear_members();
}

#[test]
fn the_view_corners_are_what_the_camera_sees() {
    let mut cam = Camera2D::new(200.0, 100.0);
    cam.position = Vec2::new(10.0, 0.0);
    let c = cam.view_corners();
    let want = [Vec2::new(-90.0, 50.0), Vec2::new(110.0, 50.0), Vec2::new(110.0, -50.0), Vec2::new(-90.0, -50.0)];
    for (got, want) in c.iter().zip(want) {
        assert!(got.distance(want) < 1e-3, "{got} vs {want}");
    }
    cam.zoom = 2.0;
    assert!(cam.view_corners()[1].distance(Vec2::new(60.0, 25.0)) < 1e-3, "zoomed in, half as much");
}

#[test]
fn a_script_fades_it_and_the_scene_keeps_it() {
    let mut world = World::new();
    let g = world.spawn((at(0.0, 0.0), CanvasGroup::default()));
    set_canvas_group_field(&world, g, "opacity", &serde_json::json!(0.25)).unwrap();
    set_canvas_group_field(&world, g, "tint", &serde_json::json!([1, 0.5, 0.5, 1])).unwrap();
    assert!(set_canvas_group_field(&world, g, "alpha", &serde_json::json!(1)).is_err());
    assert_eq!(world.get::<&CanvasGroup>(g).unwrap().opacity, 0.25);
    let scene = jengine_core::scene::format::save_scene(&world);
    let saved = scene.entities.iter().find_map(|e| e.components.get::<jengine_core::ecs::canvas_group::CanvasGroup>("canvas_group")).expect("saved");
    assert_eq!(saved.opacity, 0.25);
    let _ = SpriteComponent::default();
}

/// Two red squares overlapping, at half opacity. Loose, each is half red, so
/// where they overlap is redder than where they do not. As a canvas group
/// at half opacity the picture is drawn once, so the overlap is the same red
/// as the rest - the parts do not show through each other.
#[cfg(feature = "gpu-wgpu")]
#[test]
fn a_group_fades_as_one_picture() {
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::wgpu_systems::{
        SpriteShadowFlush, VIEW_TARGET_FORMAT, WorldLayer, render_sprites_with_shadows_wgpu_materialized,
    };
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 64;
    let Some(gpu) = HeadlessGpu::new(W, W) else {
        eprintln!("canvas_groups_fade_as_one: no GPU adapter - skipped");
        return;
    };
    let (_r, _w, _h, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let render = |grouped: bool| -> (u8, u8) {
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
        assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
        let mut world = World::new();
        let alpha = if grouped { 1.0 } else { 0.5 };
        let square = |x: f32| (at(x, 0.0), SpriteComponent { color: [1.0, 0.0, 0.0, alpha], size: Some([24.0, 24.0]), ..Default::default() });
        let root = world.spawn(square(-8.0));
        let other = world.spawn(square(8.0));
        if grouped {
            world.insert_one(root, CanvasGroup { opacity: 0.5, ..Default::default() }).unwrap();
            world.insert_one(other, Parent(root)).unwrap();
            world.get::<&mut Transform>(other).unwrap().position = Vec2::new(16.0, 0.0);
        }
        jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);
        let main = Camera2D::new(W as f32, W as f32);
        let views = canvas_group_views(&world, &main, (W, W));
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        // Once a frame, as a host does: `begin_frame` rewinds the vertex
        // arena, and every pass's vertices are written before the encoder
        // runs - calling it per pass let the main pass overwrite the group
        // pass's quads, and the group drew the main pass's picture.
        batch.begin_frame();
        let mut encoder = gpu.encoder();
        // The group's own pass, as a host draws a view: into its texture,
        // cleared to transparent.
        for v in &views {
            assets.ensure_render_texture(&v.target, W, W, VIEW_TARGET_FORMAT, &gpu.device);
            batch.set_target_format(&gpu.device, VIEW_TARGET_FORMAT);
            set_frame_view(&v.camera);
            let vp = v.camera.view_projection();
            let target_view = assets.get_wgpu_texture(&v.target).unwrap().view.clone();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("group"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let assets = &assets;
            render_sprites_with_shadows_wgpu_materialized(
                &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                |b, tex, kind| {
                    if let SpriteShadowFlush::Sprite { .. } = kind {
                        b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture(tex).unwrap());
                    }
                },
            );
        }
        // The main pass: the loose squares, or the group's picture.
        batch.set_target_format(&gpu.device, gpu.color_format);
        set_frame_view(&main);
        let vp = main.view_projection();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            let assets = &assets;
            render_sprites_with_shadows_wgpu_materialized(
                &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                |b, tex, kind| {
                    if let SpriteShadowFlush::Sprite { material_id } = kind {
                        let t = assets.get_wgpu_texture(tex).unwrap();
                        let _ = material_id;
                        b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
                    }
                },
            );
        }
        gpu.submit(encoder);
        clear_frame_view();
        clear_members();
        let img = gpu.read_rgba();
        let px = |x: u32| img.rgba[((32 * W + x) * 4) as usize];
        // World -16 (one square only) and 0 (both).
        (px(16), px(32))
    };
    let (single, overlap) = render(false);
    assert!(overlap > single + 20, "loose parts show through each other: {single} vs {overlap}");
    let (single, overlap) = render(true);
    assert!(single > 100, "the group is drawn: {single}");
    assert!((overlap as i32 - single as i32).abs() <= 2, "one picture, faded once: {single} vs {overlap}");
}

#[test]
fn every_host_draws_the_groups_as_views_cleared_to_transparent() {
    for (name, src) in [
        ("native", support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs"))),
        ("editor", support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs"))),
        ("web", support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs"))),
    ] {
        assert!(src.contains("canvas_group::canvas_group_views("), "{name} never draws canvas groups");
        assert!(
            src.contains("views[v].transparent") && src.contains("begin_pass(&mut frame.encoder, Some(clear))"),
            "{name} clears a group's picture to the scene's background, so it covers the world"
        );
    }
}

/// A batch is begun once a frame, never per view: `begin_frame` rewinds its
/// vertex arena, and every pass's vertices are written before the encoder
/// runs, so a batch begun again for the next view overwrote the earlier
/// views' quads. The lit batch (the normal pass) was begun per view in every
/// host, so a view - or a canvas group - with normal maps drew the main
/// camera's normals.
#[test]
fn every_host_begins_its_batches_once_a_frame() {
    for (name, src) in [
        ("native", support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs"))),
        ("editor", support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs"))),
        ("web", support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs"))),
    ] {
        let loop_at = src.find("for which in (0..views.len())").expect("the view loop");
        let after = &src[loop_at..];
        assert!(!after.contains("lit_batch.begin_frame()"), "{name} begins the lit batch inside the view loop");
        assert!(!after.contains("sprite_batch.begin_frame()"), "{name} begins the sprite batch inside the view loop");
        assert!(src[..loop_at].contains("lit_sprite_batch.begin_frame()"), "{name} never begins the lit batch");
    }
}
