//! **Light layers** (`light_mask` on a light, `light_layers` on a sprite;
//! Oct 2026; Godot's `light_mask` / `range_item_cull_mask`, Unity's target
//! sorting layers): a light lights a sprite only when they share a layer.
//! Before it every light lit everything, so a torch could not light the hero
//! without lighting the backdrop behind him, nor a backdrop ignore the
//! player's lamp.
//!
//! Each sprite writes its layers whole into a buffer beside the normals
//! (`fs_normal`'s third output, never blended) and the lights read it
//! (`on_light_layers`); a pixel no sprite drew is on every layer. The CPU's
//! `light_at_on` gives the same answer.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::{
    AmbientLight, LightLayers, PointLight2D, SpotLight2D, SpriteComponent, Transform,
};
use jengine_core::ecs::wgpu_systems::{
    SpriteShadowFlush, WorldLayer, lit_sprite_textures, needs_normal_pass, render_normals_wgpu,
    render_sprites_with_shadows_wgpu_materialized, uses_light_layers,
};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuLightingPass, WgpuLitSpritePipeline, WgpuSpriteBatch};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::lighting::{gather_lights, light_at, light_at_on};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 128;
const H: u32 = 64;

/// Two white squares, the left on layer 1 (no component), the right on
/// layer `right`, under a light whose mask is `mask`. Returns the frame's
/// red channel.
fn world(mask: u32, right: u32) -> World {
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    world.spawn((
        Transform::default(),
        PointLight2D { color: [1.0; 3], intensity: 0.8, radius: 2000.0, falloff: 1.0, light_mask: mask, ..Default::default() },
    ));
    world.spawn((
        Transform { position: Vec2::new(-32.0, 0.0), ..Default::default() },
        SpriteComponent { texture_id: "white.png".into(), size: Some([60.0, 60.0]), ..Default::default() },
    ));
    world.spawn((
        Transform { position: Vec2::new(32.0, 0.0), ..Default::default() },
        SpriteComponent { texture_id: "white.png".into(), size: Some([60.0, 60.0]), ..Default::default() },
        LightLayers { layers: right },
    ));
    world
}

fn render(gpu: &HeadlessGpu, world: &World) -> Vec<u8> {
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes("white.png".into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture("white.png", &gpu.device, &gpu.queue).unwrap();
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
        let mut pass = lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE));
        batch.begin_frame();
        let assets = &assets;
        render_sprites_with_shadows_wgpu_materialized(
            world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, kind| {
                if let SpriteShadowFlush::Sprite { .. } = kind {
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture(tex).unwrap());
                }
            },
        );
    }
    if needs_normal_pass(world) {
        let mut np = lp.normal_pass(&mut encoder);
        lit.begin_frame();
        let assets = &mut assets;
        render_normals_wgpu(world, &SortLayers::default(), &mut lit, Vec2::splat(64.0), |pipe, d, n| {
            let (diffuse, normal, _) = lit_sprite_textures(assets, &gpu.device, &gpu.queue, d, n, &flat).unwrap();
            pipe.flush_normals(&gpu.device, &gpu.queue, &mut np, &vp, diffuse, normal);
        });
    }
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &gather_lights(world));
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    gpu.read_rgba().rgba.chunks(4).map(|p| p[0]).collect()
}

/// The red at `(x, y)` pixels from the frame's centre, y up.
fn at(img: &[u8], x: i32, y: i32) -> u8 {
    img[((H as i32 / 2 - y) as u32 * W + (W as i32 / 2 + x) as u32) as usize]
}

#[test]
fn a_light_lights_only_the_layers_its_mask_names() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("lights_light_their_layers: no GPU adapter - skipped");
        return;
    };
    // Layer 1 only: the left square is lit, the right one (layer 2) is not -
    // and the backdrop, which no sprite drew, is on every layer.
    let w = world(1, 2);
    assert!(needs_normal_pass(&w) && uses_light_layers(&w));
    let img = render(&gpu, &w);
    assert!(at(&img, -32, 0) > 150, "layer 1 is lit: {}", at(&img, -32, 0));
    assert!(at(&img, 32, 0) < 10, "layer 2 is not: {}", at(&img, 32, 0));
    assert!(at(&img, 0, 30) > 150 || at(&img, 63, 31) > 150, "the backdrop is lit");
    // A sprite on both layers is lit by it; a light of every layer lights all.
    let img = render(&gpu, &world(1, 3));
    assert!(at(&img, 32, 0) > 150, "layers 1 and 2 share layer 1 with it");
    let all = world(u32::MAX, 2);
    assert!(!uses_light_layers(&all), "every layer is the default and costs nothing");
    let img = render(&gpu, &all);
    assert!(at(&img, -32, 0) > 150 && at(&img, 32, 0) > 150);
    // Layer 2 only: the reverse.
    let img = render(&gpu, &world(2, 2));
    assert!(at(&img, -32, 0) < 10 && at(&img, 32, 0) > 150);
}

#[test]
fn the_cpu_asks_by_layer_too() {
    let w = world(1, 2);
    let lights = gather_lights(&w);
    let p = Vec2::new(32.0, 0.0);
    assert!(light_at(p, &lights).x > 0.5, "light_at asks for every layer");
    assert!(light_at_on(p, &lights, 1).x > 0.5);
    assert_eq!(light_at_on(p, &lights, 2).x, 0.0, "a sprite on layer 2 is unlit");
    let mut spot = World::new();
    spot.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    spot.spawn((
        Transform::default(),
        SpotLight2D { radius: 500.0, angle: 3.0, light_mask: 4, ..Default::default() },
    ));
    let lights = gather_lights(&spot);
    assert!(light_at_on(Vec2::new(50.0, 0.0), &lights, 4).x > 0.0);
    assert_eq!(light_at_on(Vec2::new(50.0, 0.0), &lights, 1).x, 0.0, "spot lights keep their layers");
}

#[test]
fn the_scene_keeps_both() {
    let w = world(6, 4);
    let scene = jengine_core::scene::format::save_scene(&w);
    let json = serde_json::to_string(&scene).unwrap();
    assert!(json.contains("\"light_mask\":6") && json.contains("\"light_layers\":{\"layers\":4}"), "{json}");
    let plain = jengine_core::scene::format::save_scene(&world(u32::MAX, 1));
    assert!(!serde_json::to_string(&plain).unwrap().contains("light_mask"), "every layer is not written");
}
