//! **A camera draws only the layers it is told to** (`cull_mask` on the
//! camera, `render_layers` on an entity, Sep 2026). Every camera saw
//! everything, so a minimap could not show a marker the street views do not,
//! or leave the roofs off to show the roads - Unity's culling mask and
//! Godot's cull mask. The mask rides in the frame view each pass sets from
//! its own camera, and every world collector asks `in_view_layers`, so no
//! host was touched. Found by the showcase's Two Views.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{Camera2DComponent, RenderLayers, SpriteComponent, Transform};
use jengine_core::ecs::views::render_target_cameras;
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, in_view_layers, render_sprites_with_shadows_wgpu_materialized};
use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 64;
const H: u32 = 64;

fn camera(mask: u32) -> Camera2D {
    let mut c = Camera2D::new(W as f32, H as f32);
    c.cull_mask = mask;
    c
}

#[test]
fn an_entity_is_drawn_by_a_camera_that_shares_a_layer() {
    let mut world = World::new();
    let plain = world.spawn((Transform::default(),));
    let marker = world.spawn((Transform::default(), RenderLayers { layers: 0b10 }));
    set_frame_view(&camera(u32::MAX));
    assert!(in_view_layers(&world, plain) && in_view_layers(&world, marker), "a camera sees every layer by default");
    set_frame_view(&camera(0b01));
    assert!(in_view_layers(&world, plain), "no layers is layer 1");
    assert!(!in_view_layers(&world, marker), "a street camera on layer 1 does not see the marker");
    set_frame_view(&camera(0b10));
    assert!(!in_view_layers(&world, plain) && in_view_layers(&world, marker), "a map camera on layer 2 sees only it");
    clear_frame_view();
}

#[test]
fn a_view_and_the_main_camera_carry_their_masks() {
    let mut world = World::new();
    world.spawn((
        Camera2DComponent { target: "views/map".into(), cull_mask: 0b11, ..Default::default() },
        Transform::default(),
    ));
    world.spawn((Camera2DComponent { cull_mask: 0b01, ..Default::default() }, Transform::default()));
    assert_eq!(render_target_cameras(&world, (64, 64))[0].camera.cull_mask, 0b11);
    let mut main = Camera2D::new(64.0, 64.0);
    jengine_core::ecs::systems::update_camera(&world, &mut main, 0.016);
    assert_eq!(main.cull_mask, 0b01, "update_camera copies the main camera's mask");
}

/// A red sprite on layer 1 at the left, a green one on layer 2 at the right,
/// seen through a camera with `mask`; (red at left, green at right).
fn render(mask: u32) -> Option<(u8, u8)> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("cameras_cull_by_layer: no GPU adapter - skipped");
        None
    })?;
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
    let (rgba, aw, ah, font) = built_in_font_atlas();
    let mut fa = AssetManager::new();
    fa.insert_texture_bytes("__font".into(), RawTextureData { width: aw, height: ah, rgba });
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut world = World::new();
    let quad = |c: [f32; 4]| SpriteComponent { color: c, size: Some([20.0, 20.0]), ..Default::default() };
    world.spawn((Transform { position: Vec2::new(-16.0, 0.0), ..Default::default() }, quad([1.0, 0.0, 0.0, 1.0])));
    world.spawn((
        Transform { position: Vec2::new(16.0, 0.0), ..Default::default() },
        quad([0.0, 1.0, 0.0, 1.0]),
        RenderLayers { layers: 0b10 },
    ));
    let cam = camera(mask);
    set_frame_view(&cam);
    let vp = cam.view_projection();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        batch.begin_frame();
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, kind| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                if let SpriteShadowFlush::Sprite { .. } = kind {
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
                }
            },
        );
    }
    gpu.submit(encoder);
    clear_frame_view();
    let img = gpu.read_rgba();
    let px = |x: u32, c: usize| img.rgba[((32 * W + x) * 4) as usize + c];
    Some((px(16, 0), px(48, 1)))
}

#[test]
fn a_camera_draws_only_its_layers() {
    let Some((red, green)) = render(u32::MAX) else { return };
    assert!(red > 200 && green > 200, "every layer: {red} {green}");
    let Some((red, green)) = render(0b01) else { return };
    assert!(red > 200 && green < 20, "layer 1 only: {red} {green}");
    let Some((red, green)) = render(0b10) else { return };
    assert!(red < 20 && green > 200, "layer 2 only: {red} {green}");
}
