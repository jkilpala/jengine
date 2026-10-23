//! A large, mostly opaque black sprite darkens what is under it in a lit,
//! bloomed HDR frame - lit or `Unlit`, over lit floor or an unlit glow.
//!
//! A game in the RPG Classics batch reported that a black veil at z 60 with
//! alpha 0.82 did not visibly darken a scene lit by an ambient and point
//! lights with HDR bloom. This draws that frame through the passes
//! `ecs::frame_draw::draw_frame` runs for the screen camera - the scene layer,
//! the lights, the emission layer, the composite with `set_hdr`, then the post
//! chain with bloom and the tonemap - and measures the pixels under the veil
//! against the same frame without it.
//!
//! It is not an engine bug (Oct 2026): the veil leaves 0.18 of the light,
//! lit or unlit, over lit floor and over unlit glow, HDR or not. What makes
//! one *look* ineffective is (a) blending in linear light - 0.18 linear is
//! sRGB 0.46, so an "82%" veil reads as about half as bright; (b) in an HDR
//! frame the veil scales radiance **before** the tonemap, and over a
//! highlight well past 1 the tonemap's curve flattens toward white, so the
//! screen loses far less than the radiance did; (c) bloom from bright things
//! outside a veil is added after it, over it; and (d) UI - entity UI and
//! `.gui` - draws after the whole world and its post chain, so no world
//! sprite at any z covers it. A fade over everything belongs in the UI.
//!
//! Without a GPU adapter it prints why and passes, like `render_golden`.
#![cfg(feature = "gpu-wgpu")]

use std::collections::HashMap;

use glam::Vec2;
use hecs::World;
use jengine_core::asset::sort_layers::SortLayers;
use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
use jengine_core::ecs::components::{AmbientLight, PointLight2D, SpriteComponent, Transform, Unlit};
use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_world_items};
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::backend::wgpu::{WgpuLightingPass, WgpuPostProcessStack, WgpuSpriteBatch};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::font_asset::FrameFonts;
use jengine_core::renderer::lighting::gather_lights;
use jengine_core::renderer::material::UniformValue;
use jengine_core::renderer::postprocess::{PostProcessEffectSettings, PostProcessSettings};
use jengine_core::renderer::text::built_in_font_atlas;

const W: u32 = 160;
const H: u32 = 96;
const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

fn quad(x: f32, y: f32, w: f32, h: f32, color: [f32; 4], z: i32) -> (Transform, SpriteComponent) {
    (
        Transform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::ONE },
        SpriteComponent { texture_id: Default::default(), color, size: Some([w, h]), z_order: z, ..Default::default() },
    )
}

/// The scene: a lit grey floor over the whole view, an ambient and two point
/// lights bright enough to bloom, and an unlit glowing panel on the right
/// half's top - the two kinds of thing a veil may lie over.
fn scene() -> World {
    let mut world = World::new();
    world.spawn((AmbientLight { color: [0.8, 0.85, 1.0], intensity: 0.45 },));
    for x in [-40.0, 40.0] {
        world.spawn((
            Transform { position: Vec2::new(x, 0.0), rotation: 0.0, scale: Vec2::ONE },
            PointLight2D { color: [1.0, 0.9, 0.7], intensity: 1.6, radius: 160.0, falloff: 1.0, ..Default::default() },
        ));
    }
    world.spawn(quad(0.0, 0.0, W as f32, H as f32, [0.7, 0.7, 0.7, 1.0], 0));
    let (t, s) = quad(40.0, 24.0, 60.0, 30.0, [1.0, 0.95, 0.6, 1.0], 10);
    world.spawn((t, s, Unlit {}));
    world
}

fn post_settings(hdr: bool) -> PostProcessSettings {
    let mut params = HashMap::new();
    params.insert("threshold".to_string(), UniformValue::Float(1.0));
    params.insert("intensity".to_string(), UniformValue::Float(0.8));
    PostProcessSettings {
        hdr,
        exposure: Some(1.0),
        effects: vec![PostProcessEffectSettings { name: "bloom".into(), enabled: true, params }],
        ..Default::default()
    }
}

/// The screen camera's lit frame, as `draw_frame` draws it (no views, no
/// scene readers, no normal maps): every pixel in sRGB.
fn render(gpu: &HeadlessGpu, world: &World, hdr: bool) -> RawTextureData {
    let light = WgpuLightingPass::new(&gpu.device, W, H, SCENE_FORMAT).expect("lighting pass");
    let mut post = WgpuPostProcessStack::new(&gpu.device, W, H, SCENE_FORMAT, gpu.color_format).expect("post stack");
    post.set_settings(post_settings(hdr));
    let mut batch = WgpuSpriteBatch::new(&gpu.device, SCENE_FORMAT).expect("sprite batch");
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).expect("upload");
    let (_, _, _, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let sort_layers = SortLayers::default();

    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    jengine_core::renderer::frame_view::set_frame_view(&camera);
    let mut encoder = gpu.encoder();
    batch.begin_frame();
    render_world_items(world, &sort_layers, &fonts, Vec2::splat(64.0), |collected| {
        {
            let mut pass = light.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::BLACK));
            collected.draw(WorldLayer::Scene, &mut batch, |b, tex, kind| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                match kind {
                    SpriteShadowFlush::Shadow => b.flush_multiply(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                    SpriteShadowFlush::Sprite { .. } => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                };
            });
        }
        let lights = gather_lights(world);
        light.prepare_cookies(&gpu.device, &gpu.queue, &lights, &mut assets);
        light.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
        if collected.draws_emission() {
            let mut ep = light.emission_pass(&mut encoder);
            collected.draw(WorldLayer::Emission, &mut batch, |b, tex, _| {
                let t = assets.get_wgpu_texture(tex).expect("uploaded");
                b.flush(&gpu.device, &gpu.queue, &mut ep, &vp, t);
            });
        }
        light.set_hdr(&gpu.queue, post.hdr_active());
        light.composite(&mut encoder, &post.scene_target().view);
    });
    post.execute(&gpu.queue, &mut encoder, gpu.view());
    gpu.submit(encoder);
    gpu.read_rgba()
}

/// Mean sRGB brightness of a box of pixels about world point (x, y).
fn mean(img: &RawTextureData, x: f32, y: f32) -> f32 {
    let (cx, cy) = ((x + W as f32 / 2.0) as i32, (H as f32 / 2.0 - y) as i32);
    let mut sum = 0.0;
    let mut n = 0.0;
    for py in cy - 3..=cy + 3 {
        for px in cx - 3..=cx + 3 {
            let i = ((py as u32 * W + px as u32) * 4) as usize;
            sum += (img.rgba[i] as f32 + img.rgba[i + 1] as f32 + img.rgba[i + 2] as f32) / 3.0;
            n += 1.0;
        }
    }
    sum / n
}

/// Linear light from an sRGB byte level.
fn linear(v: f32) -> f32 {
    let s = v / 255.0;
    if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
}

#[test]
fn a_dark_veil_darkens_a_lit_bloomed_scene() {
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("a_dark_veil_darkens_a_lit_scene: no GPU adapter - skipped");
        return;
    };
    // The floor under a light, and the unlit panel.
    let probes = [("lit floor", -40.0, -24.0), ("unlit glow", 40.0, 24.0)];
    for hdr in [true, false] {
        let bare = render(&gpu, &scene(), hdr);
        for unlit_veil in [false, true] {
            let mut world = scene();
            let (t, s) = quad(0.0, 0.0, W as f32 + 20.0, H as f32 + 20.0, [0.0, 0.0, 0.0, 0.82], 60);
            if unlit_veil {
                world.spawn((t, s, Unlit {}));
            } else {
                world.spawn((t, s));
            }
            let veiled = render(&gpu, &world, hdr);
            for (what, x, y) in probes {
                let (before, after) = (mean(&bare, x, y), mean(&veiled, x, y));
                let ratio = linear(after) / linear(before).max(1e-4);
                eprintln!(
                    "hdr {hdr} {} veil over {what}: sRGB {before:.0} -> {after:.0} (linear x{ratio:.2})",
                    if unlit_veil { "unlit" } else { "lit" }
                );
                assert!(before > 60.0, "{what} is bright to start with ({before:.0})");
                // An 0.82 black veil leaves 0.18 of the light; tonemapping
                // and bloom bend that, but it must read as clearly darker.
                assert!(
                    ratio < 0.4,
                    "hdr {hdr}, {} veil: {what} barely darkened, sRGB {before:.0} -> {after:.0} (linear x{ratio:.2})",
                    if unlit_veil { "unlit" } else { "lit" }
                );
            }
        }
    }
}
