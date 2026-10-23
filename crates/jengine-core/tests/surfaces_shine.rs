//! Specular highlights (`MaterialComponent::specular` / `shininess`,
//! `light_specular.wgsl`): a shiny surface glints where a point light's
//! reflection would meet the eye, tighter the shinier, masked by the normal
//! map's alpha - and a matt one is lit exactly as before.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::{AmbientLight, MaterialComponent, PointLight2D, SpriteComponent, Transform};
use jengine_core::ecs::wgpu_systems::{
    SpriteShadowFlush, WorldLayer, lit_sprite_textures, needs_normal_pass, render_normals_wgpu,
    render_sprites_with_shadows_wgpu_materialized, specular_encoding,
};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuLightingPass, WgpuLitSpritePipeline, WgpuSpriteBatch};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::lighting::gather_lights;
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 128;
const H: u32 = 128;

fn solid(w: u32, h: u32, rgba: [u8; 4]) -> RawTextureData {
    RawTextureData { width: w, height: h, rgba: rgba.repeat((w * h) as usize) }
}

/// A flat normal map whose alpha is 0 on its left half and 1 on its right:
/// a sprite that is cloth on the left and metal on the right.
fn half_mask() -> RawTextureData {
    let n = 16u32;
    let mut rgba = Vec::new();
    for _y in 0..n {
        for x in 0..n {
            rgba.extend_from_slice(&[128, 128, 255, if x < n / 2 { 0 } else { 255 }]);
        }
    }
    RawTextureData { width: n, height: n, rgba }
}

/// A white square filling the frame, a point light above its middle, the
/// material shiny or not; returns the red channel of the frame.
fn render(gpu: &HeadlessGpu, specular: f32, shininess: f32, normal: Option<RawTextureData>) -> Vec<u8> {
    render_on(gpu, [255, 255, 255, 255], specular, shininess, normal)
}

fn render_on(gpu: &HeadlessGpu, surface: [u8; 4], specular: f32, shininess: f32, normal: Option<RawTextureData>) -> Vec<u8> {
    let mut assets = AssetManager::new();
    // White, under a dim light: an LDR frame clamps light at 1, so the
    // highlight needs room above the diffuse to show.
    assets.insert_texture_bytes("grey.png".into(), solid(4, 4, surface));
    assets.ensure_wgpu_texture("grey.png", &gpu.device, &gpu.queue).unwrap();
    let has_normal = normal.is_some();
    if let Some(n) = normal {
        assets.insert_texture_bytes("normal.png".into(), n);
    }
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.1 },));
    world.spawn((
        Transform::default(),
        // Low (`height` 0.1 of a radius of 400: 40 units above the plane), so
        // the highlight is a spot rather than a sheen over the whole frame.
        PointLight2D { color: [1.0; 3], intensity: 0.3, radius: 400.0, falloff: 1.0, height: 0.1, ..Default::default() },
    ));
    world.spawn((
        Transform::default(),
        SpriteComponent { texture_id: "grey.png".into(), size: Some([128.0, 128.0]), ..Default::default() },
        MaterialComponent {
            normal_map_id: has_normal.then(|| "normal.png".to_string()),
            specular,
            shininess,
            ..Default::default()
        },
    ));
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
        let mut pass = lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
        batch.begin_frame();
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
    if needs_normal_pass(&world) {
        let mut np = lp.normal_pass(&mut encoder);
        lit.begin_frame();
        let assets = &mut assets;
        render_normals_wgpu(&world, &SortLayers::default(), &mut lit, Vec2::splat(64.0), |pipe, d, n| {
            let (diffuse, normal, _) = lit_sprite_textures(assets, &gpu.device, &gpu.queue, d, n, &flat).unwrap();
            pipe.flush_normals(&gpu.device, &gpu.queue, &mut np, &vp, diffuse, normal);
        });
    }
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &gather_lights(&world));
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    gpu.read_rgba().rgba.chunks(4).map(|p| p[0]).collect()
}

fn at(img: &[u8], x: i32, y: i32) -> u8 {
    img[((H as i32 / 2 + y) as u32 * W + (W as i32 / 2 + x) as u32) as usize]
}

#[test]
fn a_shiny_surface_glints_under_the_light_and_a_matt_one_does_not() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("surfaces_shine: no GPU adapter - skipped");
        return;
    };
    let matt = render(&gpu, 0.0, 24.0, None);
    let shiny = render(&gpu, 1.0, 24.0, None);
    // Under the light the glint; far from it, the diffuse alone.
    assert!(at(&shiny, 0, 0) as i32 > at(&matt, 0, 0) as i32 + 30, "a glint: {} vs {}", at(&shiny, 0, 0), at(&matt, 0, 0));
    assert!((at(&shiny, 60, 0) as i32 - at(&matt, 60, 0) as i32).abs() <= 6, "next to none far off: {} vs {}", at(&shiny, 60, 0), at(&matt, 60, 0));
    // Tighter the shinier: a little way out, the broad sheen is still bright.
    let sheen = render(&gpu, 1.0, 4.0, None);
    let pin = render(&gpu, 1.0, 120.0, None);
    assert!(at(&sheen, 18, 0) > at(&pin, 18, 0) + 10, "broad {} vs tight {}", at(&sheen, 18, 0), at(&pin, 18, 0));
}

#[test]
fn the_normal_maps_alpha_masks_the_shine() {
    let Some(gpu) = HeadlessGpu::new(W, H) else { return };
    let matt = render(&gpu, 0.0, 8.0, Some(half_mask()));
    let masked = render(&gpu, 1.0, 8.0, Some(half_mask()));
    // The light is over the middle: look just either side of it.
    assert!((at(&masked, -6, 0) as i32 - at(&matt, -6, 0) as i32).abs() <= 3, "cloth does not shine");
    assert!(at(&masked, 6, 0) as i32 > at(&matt, 6, 0) as i32 + 20, "metal does: {} vs {}", at(&masked, 6, 0), at(&matt, 6, 0));
}

#[test]
fn shininess_rides_the_normal_pass_in_the_vertex_colour() {
    assert_eq!(specular_encoding(0.0, 24.0), [0.0, 23.0 / 127.0]);
    assert_eq!(specular_encoding(2.0, 500.0), [1.0, 1.0], "clamped");
    let mut world = World::new();
    world.spawn((MaterialComponent::default(),));
    assert!(!needs_normal_pass(&world), "a matt material needs no normal pass");
    world.spawn((MaterialComponent { specular: 0.5, ..Default::default() },));
    assert!(needs_normal_pass(&world), "a shiny one does");
}

/// A highlight sits on top of the surface: a near-black shiny thing still
/// glints in the light's colour. It was summed into the light the scene is
/// multiplied by at first, so a dark surface's highlight was as dark as it.
#[test]
fn a_dark_surface_still_glints() {
    let Some(gpu) = HeadlessGpu::new(W, H) else { return };
    let matt = render_on(&gpu, [20, 20, 20, 255], 0.0, 24.0, None);
    let shiny = render_on(&gpu, [20, 20, 20, 255], 1.0, 24.0, None);
    assert!(at(&matt, 0, 0) < 40, "dark: {}", at(&matt, 0, 0));
    assert!(at(&shiny, 0, 0) > 120, "and still a bright glint: {}", at(&shiny, 0, 0));
}
