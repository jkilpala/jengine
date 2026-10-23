//! **Mirrors** (Oct 2026; `ecs::mirror`): a `mirror` shows what is in front
//! of its line, reflected, over the region as deep behind it - a view of its
//! own draws the region in front into a texture, and the sprite pass draws
//! that texture behind the line, flipped across it. Found by the showcase's
//! Mirror Maze, where every wall that was a mirror had been a camera, a
//! target and a hand-placed, hand-flipped sprite.

mod support;

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::Transform;
use jengine_core::ecs::mirror::{Mirror, MirrorPlace, clear_active, is_active, mirror_key, mirror_views, target_id};
use jengine_core::renderer::camera::{Camera2D, CameraPass};

fn at(x: f32, y: f32, rot: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: rot, ..Default::default() }
}

/// Where a world point lands in a camera's picture, as texture UV.
fn uv_in(camera: &Camera2D, p: Vec2) -> Vec2 {
    let ndc = camera.view_projection() * glam::Vec4::new(p.x, p.y, 0.0, 1.0);
    Vec2::new((ndc.x + 1.0) * 0.5, (1.0 - ndc.y) * 0.5)
}

/// The UV the reflection quad gives a point inside it (bilinear over its
/// corners, which is what the GPU does for a parallelogram).
fn uv_on_quad(place: &MirrorPlace, p: Vec2) -> Vec2 {
    let ([a, b, _, d], uv) = place.reflection();
    let (s, t) = ((p - a).dot(b - a) / (b - a).length_squared(), (p - a).dot(d - a) / (d - a).length_squared());
    let lerp = |i: usize, j: usize, k: f32| Vec2::new(uv[i][0], uv[i][1]).lerp(Vec2::new(uv[j][0], uv[j][1]), k);
    lerp(0, 1, s).lerp(lerp(3, 2, s), t)
}

#[test]
fn a_point_behind_the_mirror_shows_its_reflection_in_front() {
    for rot in [0.0f32, 0.7, -2.1] {
        let mirror = Mirror { width: 120.0, depth: 60.0, ..Default::default() };
        let place = MirrorPlace::of(&mirror, Vec2::new(30.0, -10.0), rot);
        let camera = place.camera(1);
        assert_eq!(camera.pass, CameraPass::Mirror(1));
        for (s, d) in [(0.0f32, 10.0f32), (-40.0, 30.0), (50.0, 55.0)] {
            let front = place.centre + place.along * s + place.front * d;
            let behind = place.centre + place.along * s - place.front * d;
            let seen = uv_in(&camera, front);
            let shown = uv_on_quad(&place, behind);
            assert!(seen.distance(shown) < 1e-3, "rot {rot}: what is {d} in front shows {d} behind: {seen} vs {shown}");
        }
    }
}

#[test]
fn only_mirrors_near_the_view_draw_and_their_textures_are_sized_by_resolution() {
    let mut world = World::new();
    let near = world.spawn((at(0.0, 0.0, 0.0), Mirror { width: 200.0, depth: 100.0, resolution: 0.5, ..Default::default() }));
    let far = world.spawn((at(9000.0, 0.0, 0.0), Mirror::default()));
    let off = world.spawn((at(0.0, 0.0, 0.0), Mirror { enabled: false, ..Default::default() }));
    let main = Camera2D::new(640.0, 360.0);
    let views = mirror_views(&world, &main);
    assert_eq!(views.len(), 1, "only the mirror near the view draws");
    assert_eq!(views[0].target, target_id(near));
    assert_eq!(views[0].size, (100, 50), "half a texel a world unit");
    assert!(views[0].transparent, "a reflection has no post chain of its own");
    assert_eq!(views[0].camera.pass, CameraPass::Mirror(mirror_key(near)));
    assert!(is_active(near) && !is_active(far) && !is_active(off));
    clear_active();
    assert!(!is_active(near));
}

#[test]
fn every_host_draws_the_mirrors_views() {
    for (name, src) in [
        ("native", support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs"))),
        ("editor", support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs"))),
        ("web", support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs"))),
    ] {
        assert!(src.contains("mirror::mirror_views("), "{name} never draws mirrors");
    }
}

/// A red square in front of a mirror on black. After the mirror's view and
/// the main pass, the square's reflection is red behind the line, where
/// nothing else is - and with the mirror off, that place is black.
#[cfg(feature = "gpu-wgpu")]
#[test]
fn what_stands_in_front_of_a_mirror_shows_behind_it() {
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::components::SpriteComponent;
    use jengine_core::ecs::wgpu_systems::{
        SpriteShadowFlush, VIEW_TARGET_FORMAT, WorldLayer, render_sprites_with_shadows_wgpu_materialized,
    };
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 128;
    let Some(gpu) = HeadlessGpu::new(W, W) else {
        eprintln!("mirrors_reflect: no GPU adapter - skipped");
        return;
    };
    let (_r, _w, _h, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let render = |enabled: bool| -> (u8, u8) {
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
        assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
        let mut world = World::new();
        // The mirror along y = 0, facing up; the square 30 above it.
        world.spawn((at(0.0, 0.0, 0.0), Mirror { width: 100.0, depth: 50.0, enabled, ..Default::default() }));
        world.spawn((at(0.0, 30.0, 0.0), SpriteComponent { color: [1.0, 0.0, 0.0, 1.0], size: Some([16.0, 16.0]), ..Default::default() }));
        jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);
        let main = Camera2D::new(W as f32, W as f32);
        let views = mirror_views(&world, &main);
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        batch.begin_frame();
        let mut encoder = gpu.encoder();
        macro_rules! draw {
            ($pass:expr, $vp:expr) => {{
                let assets = &assets;
                render_sprites_with_shadows_wgpu_materialized(
                    &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                    |b, tex, kind| {
                        if let SpriteShadowFlush::Sprite { .. } = kind {
                            b.flush(&gpu.device, &gpu.queue, $pass, $vp, assets.get_wgpu_texture(tex).unwrap());
                        }
                    },
                );
            }};
        }
        for v in &views {
            assets.ensure_render_texture(&v.target, v.size.0, v.size.1, VIEW_TARGET_FORMAT, &gpu.device);
            batch.set_target_format(&gpu.device, VIEW_TARGET_FORMAT);
            set_frame_view(&v.camera);
            let vp = v.camera.view_projection();
            let target_view = assets.get_wgpu_texture(&v.target).unwrap().view.clone();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mirror"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            draw!(&mut pass, &vp);
        }
        batch.set_target_format(&gpu.device, gpu.color_format);
        set_frame_view(&main);
        let vp = main.view_projection();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            draw!(&mut pass, &vp);
        }
        gpu.submit(encoder);
        clear_frame_view();
        clear_active();
        let img = gpu.read_rgba();
        // World (0, 30) is pixel (64, 34); (0, -30) is pixel (64, 94).
        let red = |y: u32| img.rgba[((y * W + 64) * 4) as usize];
        (red(34), red(94))
    };
    let (square, reflection) = render(true);
    assert!(square > 200, "the square itself: {square}");
    assert!(reflection > 200, "its reflection, as far behind the line: {reflection}");
    let (_, nothing) = render(false);
    assert!(nothing < 5, "no mirror, no reflection: {nothing}");
}

#[test]
fn a_script_turns_a_mirror_off_by_name() {
    use jengine_core::ecs::mirror::set_mirror_field;
    let mut world = World::new();
    let m = world.spawn((at(0.0, 0.0, 0.0), Mirror::default()));
    set_mirror_field(&world, m, "enabled", &serde_json::json!(false)).unwrap();
    set_mirror_field(&world, m, "z_order", &serde_json::json!(4.0)).unwrap();
    {
        let mirror = world.get::<&Mirror>(m).unwrap();
        assert!(!mirror.enabled);
        assert_eq!(mirror.z_order, 4, "a Lua number reaches native as a float");
    }
    assert!(set_mirror_field(&world, m, "glare", &serde_json::json!(1)).is_err(), "an unknown field is refused");
    assert!(mirror_views(&world, &Camera2D::new(640.0, 360.0)).is_empty(), "an off mirror draws nothing");
}
