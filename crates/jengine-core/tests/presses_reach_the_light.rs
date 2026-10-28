//! `Sprite.press` on the GPU: a dent pressed into a paintable floor's normal
//! map reaches the lights - the wall of a pit turned to the light is lit
//! brighter than the flat floor, the wall turned away darker - and a second
//! press, into a normal map already on the GPU, is patched in where it landed
//! (`AssetManager::patch_dirty`) rather than lost or uploaded whole.
#![cfg(all(feature = "gpu-wgpu", feature = "paint"))]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::carve::apply_carves;
use jengine_core::ecs::components::{AmbientLight, MaterialComponent, PointLight2D, SpriteComponent, Transform};
use jengine_core::ecs::paint::{Paintable, Stamp, StampShape};
use jengine_core::ecs::wgpu_systems::{
    SpriteShadowFlush, WorldLayer, lit_sprite_textures, needs_normal_pass, render_normals_wgpu,
    render_sprites_with_shadows_wgpu_materialized,
};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuLightingPass, WgpuLitSpritePipeline, WgpuSpriteBatch};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::lighting::gather_lights;
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 128;
const H: u32 = 128;

/// A white floor filling the frame with a flat 32 x 32 normal map, and a low
/// light off to its left.
fn scene() -> (World, AssetManager, hecs::Entity) {
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(
        "floor.png".into(),
        RawTextureData { width: 4, height: 4, rgba: [200u8, 200, 200, 255].repeat(16) },
    );
    assets.insert_texture_bytes(
        "floor_n.png".into(),
        RawTextureData { width: 32, height: 32, rgba: [128u8, 128, 255, 0].repeat(32 * 32) },
    );
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.1 },));
    world.spawn((
        Transform { position: Vec2::new(-90.0, 0.0), ..Default::default() },
        PointLight2D { color: [1.0; 3], intensity: 0.45, radius: 400.0, falloff: 1.0, height: 0.15, ..Default::default() },
    ));
    let floor = world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "floor.png".into(), size: Some([128.0, 128.0]), ..Default::default() },
        MaterialComponent { normal_map_id: Some("floor_n.png".to_string()), ..Default::default() },
        Paintable::default(),
    ));
    (world, assets, floor)
}

/// One lit frame; the red channel.
fn frame(gpu: &HeadlessGpu, world: &mut World, assets: &mut AssetManager) -> Vec<u8> {
    apply_carves(world, assets);
    assets.ensure_wgpu_texture("floor.png", &gpu.device, &gpu.queue).unwrap();
    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let mut lit = WgpuLitSpritePipeline::new(&gpu.device, gpu.color_format).unwrap();
    let flat = WgpuLitSpritePipeline::flat_normal_texture(&gpu.device, &gpu.queue);
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let atlas = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&atlas.3, "__font");
    let mut encoder = gpu.encoder();
    {
        // The painted copy draws in place of the floor; make sure it is up.
        let copy = world.query::<&Paintable>().iter().find_map(|(_, p)| p.state.texture.clone());
        if let Some(id) = &copy {
            assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
        }
        let mut pass = lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        batch.begin_frame();
        let assets = &*assets;
        render_sprites_with_shadows_wgpu_materialized(
            world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, kind| {
                if let SpriteShadowFlush::Sprite { .. } = kind {
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture(tex).unwrap());
                }
            },
        );
    }
    assert!(needs_normal_pass(world));
    {
        let mut np = lp.normal_pass(&mut encoder);
        lit.begin_frame();
        render_normals_wgpu(world, &SortLayers::default(), &mut lit, Vec2::splat(64.0), |pipe, d, n| {
            let (diffuse, normal, missing) = lit_sprite_textures(assets, &gpu.device, &gpu.queue, d, n, &flat).unwrap();
            assert!(missing.is_none(), "the normal map {n:?} reached the GPU");
            pipe.flush_normals(&gpu.device, &gpu.queue, &mut np, &vp, diffuse, normal);
        });
    }
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &gather_lights(world));
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    gpu.read_rgba().rgba.chunks(4).map(|p| p[0]).collect()
}

/// The mean red over a 3 x 3 patch at world (x, y) - y up, the frame's
/// middle at the origin.
fn at(img: &[u8], x: i32, y: i32) -> f32 {
    let mut sum = 0.0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let px = (W as i32 / 2 + x + dx) as u32;
            let py = (H as i32 / 2 - y + dy) as u32;
            sum += img[(py * W + px) as usize] as f32;
        }
    }
    sum / 9.0
}

fn press(world: &mut World, floor: hecs::Entity, x: f32, y: f32) {
    let stamp = Stamp { x, y, size: 44.0, color: [1.0; 4], shape: StampShape::Disc, rotation: 0.0, depth: 2.0, blend: None };
    world.get::<&mut Paintable>(floor).unwrap().push(stamp);
}

#[test]
fn a_pressed_pit_is_lit_like_a_pit_and_a_second_press_is_patched_in() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("presses_reach_the_light: no GPU adapter - skipped");
        return;
    };
    let (mut world, mut assets, floor) = scene();
    let before = frame(&gpu, &mut world, &mut assets);

    press(&mut world, floor, 0.0, 30.0);
    let once = frame(&gpu, &mut world, &mut assets);
    // The light is to the left: the pit's right wall faces it, its left
    // wall faces away.
    let (near, far) = (at(&once, 21, 30), at(&once, -21, 30));
    assert!(near > at(&before, 21, 30) + 8.0, "the wall facing the light is brighter: {near} vs {}", at(&before, 21, 30));
    assert!(far < at(&before, -21, 30) - 8.0, "the wall facing away is darker: {far} vs {}", at(&before, -21, 30));
    assert!((at(&once, 0, -40) - at(&before, 0, -40)).abs() < 3.0, "the floor away from it is as it was");

    // The pressed map is on the GPU now: a second press must be patched in.
    press(&mut world, floor, 0.0, -30.0);
    let twice = frame(&gpu, &mut world, &mut assets);
    assert!(at(&twice, 21, -30) > at(&once, 21, -30) + 8.0, "the second pit shows: {} vs {}", at(&twice, 21, -30), at(&once, 21, -30));
    assert!((at(&twice, 21, 30) - near).abs() < 3.0, "and the first is still there");
}
