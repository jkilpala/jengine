//! **Darkness, and an effect on a whole group** (Oct 2026). A point or spot
//! light with `subtract` takes its light away (Godot's Light2D
//! `BLEND_MODE_SUB`) - drawn after the lights that add, with a
//! reverse-subtract blend, and never below black: the composite clamps, and
//! `light_at` subtracts and clamps the same way. And a `canvas_group` with a
//! `material` draws its whole picture through it - an outline round the
//! group rather than round each part. Found by the showcase's Ghost Lantern.
//!
//! Without a GPU adapter the pixel tests print why and pass.
#![cfg(feature = "gpu-wgpu")]

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::{AmbientLight, PointLight2D, SpotLight2D, Transform};
use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::lighting::{self, gather_lights};

const W: u32 = 256;
const H: u32 = 64;

fn srgb_to_linear(b: u8) -> f32 {
    let v = b as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn at(x: f32, y: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: 0.0, scale: Vec2::ONE }
}

/// Ambient `ambient`, a light adding at x = -100 and one taking away at
/// x = 60: (GPU, CPU) light at each of `xs`.
fn lit(ambient: f32, dark: f32, spot: bool, xs: &[f32]) -> Option<Vec<(f32, f32)>> {
    let gpu = HeadlessGpu::new(W, H).or_else(|| {
        eprintln!("lights_subtract_and_groups_wear_materials: no GPU adapter - skipped");
        None
    })?;
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: ambient },));
    world.spawn((at(-100.0, 0.0), PointLight2D { radius: 400.0, falloff: 1.0, ..Default::default() }));
    if spot {
        world.spawn((
            at(20.0, 0.0),
            SpotLight2D { radius: 120.0, falloff: 1.0, angle: 0.9, intensity: dark, subtract: true, ..Default::default() },
        ));
    } else {
        world.spawn((at(60.0, 0.0), PointLight2D { radius: 100.0, falloff: 1.0, intensity: dark, subtract: true, ..Default::default() }));
    }
    let lights = gather_lights(&world);
    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let camera = Camera2D::new(W as f32, H as f32);
    let mut encoder = gpu.encoder();
    drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();
    Some(
        xs.iter()
            .map(|&x| {
                let px = (x + W as f32 / 2.0) as u32;
                let g = srgb_to_linear(img.rgba[(((H / 2) * W + px) * 4) as usize]);
                (g, lighting::light_at(Vec2::new(x, 0.0), &lights).x)
            })
            .collect(),
    )
}

#[test]
fn a_subtractive_light_takes_light_away_on_the_gpu_and_the_cpu() {
    for spot in [false, true] {
        let kind = if spot { "spot" } else { "point" };
        let Some(v) = lit(0.3, 0.4, spot, &[60.0, -90.0]) else { return };
        let (centre, far) = (v[0], v[1]);
        // At its centre: ambient 0.3 + the far light (1 - 160/400 = 0.6),
        // less 0.4 at full strength (a spot at 40 of its 120).
        let expect = if spot { 0.3 + 0.6 - 0.4 * (1.0 - 40.0 / 120.0) } else { 0.3 + 0.6 - 0.4 };
        assert!((centre.1 - expect).abs() < 0.03, "{kind}: the CPU takes the light away: {centre:?}, expected {expect}");
        assert!((centre.0 - centre.1).abs() < 0.03, "{kind}: and the GPU agrees: {centre:?}");
        // Out of its reach nothing is taken.
        // (An 8-bit target holds 1 at most; the CPU's 1.275 reads as 1 there.)
        assert!((far.0 - far.1.min(1.0)).abs() < 0.03 && far.1 > 1.2, "{kind}: out of reach it is untouched: {far:?}");
    }
}

#[test]
fn darkness_stops_at_black() {
    let Some(v) = lit(0.0, 3.0, false, &[60.0]) else { return };
    let (gpu, cpu) = v[0];
    assert!(cpu == 0.0, "the CPU never goes below black: {cpu}");
    assert!(gpu < 0.01, "nor does the screen: {gpu}");
}

#[test]
fn subtract_is_off_unless_a_scene_says_so() {
    let plain = serde_json::to_value(PointLight2D::default()).unwrap();
    assert!(!serde_json::from_value::<PointLight2D>(plain).unwrap().subtract);
    let mut dark = serde_json::to_value(SpotLight2D::default()).unwrap();
    dark["subtract"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SpotLight2D>(dark).unwrap().subtract);
    assert!(!serde_json::to_string(&PointLight2D::default()).unwrap().contains("subtract"), "the default is not written");
}

/// The group's picture reaches the host's flush with the group's material,
/// which is how the host picks the graph pipeline (`material_pipeline`).
#[test]
fn a_canvas_group_draws_its_picture_through_its_material() {
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::canvas_group::{CanvasGroup, canvas_group_views, clear_members};
    use jengine_core::ecs::components::{MaterialComponent, SpriteComponent};
    use jengine_core::ecs::wgpu_systems::{
        SpriteShadowFlush, VIEW_TARGET_FORMAT, WorldLayer, render_sprites_with_shadows_wgpu_materialized,
    };
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::frame_view::{clear_frame_view, set_frame_view};
    use jengine_core::renderer::text::built_in_font_atlas;

    const S: u32 = 64;
    let Some(gpu) = HeadlessGpu::new(S, S) else {
        eprintln!("lights_subtract_and_groups_wear_materials: no GPU adapter - skipped");
        return;
    };
    let (_r, _w, _h, font) = built_in_font_atlas();
    let fonts = FrameFonts::builtin_only(&font, "__font");
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();
    let mut world = World::new();
    world.spawn((
        at(0.0, 0.0),
        SpriteComponent { size: Some([24.0, 24.0]), ..Default::default() },
        CanvasGroup::default(),
        MaterialComponent { material_id: "ghost_outline".into(), ..Default::default() },
    ));
    jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);
    let main = Camera2D::new(S as f32, S as f32);
    let views = canvas_group_views(&world, &main, (S, S));
    for v in &views {
        assets.ensure_render_texture(&v.target, S, S, VIEW_TARGET_FORMAT, &gpu.device);
    }
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
    batch.begin_frame();
    set_frame_view(&main);
    let vp = main.view_projection();
    let mut seen = Vec::new();
    let mut encoder = gpu.encoder();
    {
        let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
        let assets = &assets;
        render_sprites_with_shadows_wgpu_materialized(
            &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
            |b, tex, kind| {
                if let SpriteShadowFlush::Sprite { material_id } = kind {
                    seen.push((tex.to_string(), material_id.map(str::to_string)));
                    b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, assets.get_wgpu_texture(tex).unwrap());
                }
            },
        );
    }
    gpu.submit(encoder);
    clear_frame_view();
    clear_members();
    let picture = seen.iter().find(|(t, _)| t.starts_with("__canvas_group/")).expect("the group's picture is drawn");
    assert_eq!(picture.1.as_deref(), Some("ghost_outline"), "through the group's material: {seen:?}");
}
