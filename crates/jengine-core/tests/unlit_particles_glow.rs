//! `unlit` on a particle emitter's entity is honoured in a lit frame
//! (`render_particles_in_layer`): its particles draw at full brightness
//! whatever the light, while a lit emitter's are lit - and a lit particle in
//! front of an unlit sprite hides it, as a lit sprite does.
//!
//! Particles used to draw only in the scene layer, so `unlit` on an emitter
//! did nothing at all: the showcase's Snowfield had its snow dimmed by the
//! night, and Firefight's "unlit" smoke was lit. Only additive emitters looked
//! right, because a glow adds itself whichever way it is drawn.
//!
//! Three columns under **zero** ambient, through the passes in the order
//! every runtime runs them: a lit red particle (black), an unlit red one
//! (full red), and an unlit green sprite under a lit white particle (hidden).
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{AmbientLight, SpriteComponent, Transform, Unlit};
use jengine_core::ecs::wgpu_systems::{
    SpriteShadowFlush, WorldLayer, render_particles_in_layer, render_sprites_with_shadows_wgpu_materialized,
};
use jengine_core::particles::{ParticleEmitter, ParticleSystem, tick_particles};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuLightingPass, WgpuSpriteBatch};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::lighting::gather_lights;
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 96;
const H: u32 = 32;

/// One still particle, 14 across, of `color`, at x.
fn emitter(x: f32, color: [f32; 4]) -> (Transform, ParticleEmitter) {
    (
        Transform { position: Vec2::new(x, 0.0), ..Default::default() },
        ParticleEmitter {
            texture_id: Some(WHITE_TEXTURE_ID.into()),
            emission_rate: 0.0,
            burst_count: 1,
            max_particles: 1,
            lifetime: [10.0, 10.0],
            speed: [0.0, 0.0],
            size_start: [14.0, 14.0],
            size_end: [14.0, 14.0],
            color_start: color,
            color_end: color,
            z_order: 5,
            ..Default::default()
        },
    )
}

#[test]
fn unlit_particles_glow_and_lit_ones_cut_glows_out() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("unlit_particles_glow: no GPU adapter - skipped");
        return;
    };
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    world.spawn(emitter(-30.0, [1.0, 0.0, 0.0, 1.0]));
    let (t, e) = emitter(0.0, [1.0, 0.0, 0.0, 1.0]);
    world.spawn((t, e, Unlit {}));
    world.spawn((
        Transform { position: Vec2::new(30.0, 0.0), ..Default::default() },
        SpriteComponent { color: [0.0, 1.0, 0.0, 1.0], size: Some([20.0, 20.0]), ..Default::default() },
        Unlit {},
    ));
    world.spawn(emitter(30.0, [1.0, 1.0, 1.0, 1.0]));
    let mut system = ParticleSystem::new();
    for _ in 0..3 {
        tick_particles(&world, &mut system, 1.0 / 60.0);
    }

    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).unwrap();
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
    let (_, _, _, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    batch.begin_frame();
    for (layer, clear) in [(WorldLayer::Scene, true), (WorldLayer::Emission, false)] {
        let mut pass = if clear {
            lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK))
        } else {
            lp.emission_pass(&mut encoder)
        };
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), layer,
            |b, tex, kind| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                if let SpriteShadowFlush::Sprite { .. } = kind {
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
                }
            },
        );
        render_particles_in_layer(&world, &SortLayers::default(), &system, &mut batch, layer, |b, tex| {
            let t = assets.get_wgpu_texture(tex.unwrap_or(WHITE_TEXTURE_ID)).expect("uploaded");
            b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t);
        });
    }
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &gather_lights(&world));
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    let px = |x: f32| {
        let i = (((H / 2) * W + (x + W as f32 / 2.0) as u32) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    };
    let (lit, unlit, covered) = (px(-30.0), px(0.0), px(30.0));
    assert!(lit.iter().all(|&c| c < 8), "a lit particle under no light is dark: {lit:?}");
    assert!(unlit[0] > 245 && unlit[1] < 8, "an unlit particle is full red under no light: {unlit:?}");
    assert!(covered.iter().all(|&c| c < 8), "a lit particle hides the unlit glow behind it: {covered:?}");
}
