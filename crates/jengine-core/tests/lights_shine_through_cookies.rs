//! **Light cookies** (Oct 2026): a point or spot light can shine through a
//! texture - `cookie` - spanning its reach and turning with it, as Unity's
//! and Godot's lights do. Before it a light was a round falloff and nothing
//! else, so light through a window's grille, leaf-dappled sun or a stained-
//! glass pattern was a lightmap painted by hand that could not move with the
//! light. Found by the showcase's Moonlit Study.
//!
//! The arithmetic is `renderer::lighting::cookie_factor`, the GPU's is
//! `light_cookie.wgsl`'s; this checks the CPU, the GPU against it, a light
//! whose cookie has not loaded, and that every host binds and loads cookies.

mod support;

use glam::Vec2;
use hecs::World;
use jengine_core::asset::RawTextureData;
use jengine_core::ecs::components::{AmbientLight, PointLight2D, SpotLight2D, Transform};
use jengine_core::renderer::lighting::{gather_lights, light_at, light_at_textured};

/// A 4 x 4 cookie: the left half white, the right half black.
fn half_and_half() -> RawTextureData {
    let mut rgba = Vec::new();
    for _ in 0..4 {
        for x in 0..4 {
            let v = if x < 2 { 255 } else { 0 };
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
    }
    RawTextureData { width: 4, height: 4, rgba }
}

fn at(x: f32, y: f32, rot: f32) -> Transform {
    Transform { position: Vec2::new(x, y), rotation: rot, scale: Vec2::ONE }
}

fn lit_world(rot: f32, scale: f32) -> World {
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    world.spawn((
        at(0.0, 0.0, rot),
        PointLight2D { radius: 100.0, falloff: 1.0, cookie: "c.png".into(), cookie_scale: scale, ..Default::default() },
    ));
    world
}

#[test]
fn the_light_is_cut_by_its_cookie_and_turns_with_it() {
    let cookie = half_and_half();
    let tex = |id: &str| (id == "c.png").then_some(&cookie);
    let lights = gather_lights(&lit_world(0.0, 1.0));
    let left = light_at_textured(Vec2::new(-30.0, 0.0), &lights, tex).x;
    let right = light_at_textured(Vec2::new(30.0, 0.0), &lights, tex).x;
    assert!((left - 0.7).abs() < 1e-3, "the white half passes the light (1 - 30/100): {left}");
    assert!(right < 1e-4, "the black half stops it: {right}");
    // Turned half round, the cookie turns with the light.
    let turned = gather_lights(&lit_world(std::f32::consts::PI, 1.0));
    assert!(light_at_textured(Vec2::new(-30.0, 0.0), &turned, tex).x < 1e-4);
    assert!(light_at_textured(Vec2::new(30.0, 0.0), &turned, tex).x > 0.6);
    // A cookie half the reach: past it, nothing - the cookie is the shape.
    let small = gather_lights(&lit_world(0.0, 0.5));
    assert!(light_at_textured(Vec2::new(-30.0, 0.0), &small, tex).x > 0.6);
    assert!(light_at_textured(Vec2::new(-60.0, 0.0), &small, tex).x < 1e-4, "outside the picture");
    // `light_at` has no pictures and sees the light uncut, as with lightmaps.
    assert!(light_at(Vec2::new(30.0, 0.0), &lights).x > 0.6);
}

#[test]
fn a_spot_lights_cookie_turns_with_its_aim() {
    let cookie = half_and_half();
    let tex = |id: &str| (id == "c.png").then_some(&cookie);
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    // Aimed straight up (turned a quarter): the cookie's +x - its black
    // half - now runs along the aim.
    world.spawn((
        at(0.0, 0.0, std::f32::consts::FRAC_PI_2),
        SpotLight2D { radius: 100.0, falloff: 1.0, angle: 1.4, cookie: "c.png".into(), ..Default::default() },
    ));
    let lights = gather_lights(&world);
    // Along the aim (+y) is the cookie's +x: its black half.
    assert!(light_at_textured(Vec2::new(0.0, 40.0), &lights, tex).x < 1e-4, "ahead along the aim is the black half");
    assert!(light_at(Vec2::new(0.0, 40.0), &lights).x > 0.5, "and the uncut light is there");
}

#[test]
fn the_scene_format_keeps_it_and_every_loader_fetches_it() {
    let world = lit_world(0.0, 0.75);
    let scene = jengine_core::scene::format::save_scene(&world);
    let pl = scene.entities.iter().find_map(|e| e.components.point_light.as_ref()).expect("saved");
    assert_eq!((pl.cookie.as_str(), pl.cookie_scale), ("c.png", 0.75));
    let json = serde_json::to_value(pl).unwrap();
    assert!(json.get("turn").is_none(), "the turn is the entity's, not authored");
    // Native and web preload what this names; the editor scans for it itself.
    assert!(jengine_core::scene::format::world_texture_ids(&world).contains("c.png"));
    let prefab = jengine_core::scene::format::prefab_texture_ids(&scene.entities);
    assert!(prefab.contains("c.png"), "a spawned lamp's cookie too");
    let editor = include_str!("../../../editor/src/lib/Viewport.svelte");
    assert!(editor.contains("point_light?.cookie") && editor.contains("spot_light?.cookie"), "the editor never loads a cookie");
}

#[test]
fn every_host_binds_the_cookies_before_the_lights() {
    for (name, src) in [
        ("native", support::with_frame_draw(include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs"))),
        ("editor", support::with_frame_draw(include_str!("../../jengine-editor-viewport/src/lib.rs"))),
        ("web", support::with_frame_draw(include_str!("../../jengine-web/src/lib.rs"))),
    ] {
        let prepare = src.find(".prepare_cookies(").unwrap_or_else(|| panic!("{name} never prepares cookies - a cookie light draws nothing there"));
        let render = src.find(".render_lights(").expect("renders lights");
        assert!(prepare < render, "{name} prepares the cookies after drawing the lights");
    }
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use jengine_core::asset::AssetManager;
    use jengine_core::renderer::backend::wgpu::WgpuLightingPass;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;

    const W: u32 = 256;
    const H: u32 = 64;

    fn srgb_to_linear(b: u8) -> f32 {
        let v = b as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    }

    /// The light at `x` on the GPU, the cookie loaded or not.
    fn gpu_light(gpu: &HeadlessGpu, world: &World, loaded: bool, xs: &[f32]) -> Vec<f32> {
        let mut assets = AssetManager::new();
        if loaded {
            assets.insert_texture_bytes("c.png".into(), half_and_half());
        }
        let lights = gather_lights(world);
        let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
        let camera = Camera2D::new(W as f32, H as f32);
        let mut encoder = gpu.encoder();
        drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
        lp.prepare_cookies(&gpu.device, &gpu.queue, &lights, &mut assets);
        lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
        lp.composite(&mut encoder, gpu.view());
        gpu.submit(encoder);
        let img = gpu.read_rgba();
        xs.iter()
            .map(|x| {
                let px = (x + W as f32 / 2.0) as u32;
                srgb_to_linear(img.rgba[(((H / 2) * W + px) * 4) as usize])
            })
            .collect()
    }

    #[test]
    fn the_gpu_cuts_the_light_where_the_cpu_does() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("lights_shine_through_cookies: no GPU adapter - skipped");
            return;
        };
        let cookie = half_and_half();
        let tex = |id: &str| (id == "c.png").then_some(&cookie);
        for rot in [0.0, std::f32::consts::PI] {
            let world = lit_world(rot, 1.0);
            let lights = gather_lights(&world);
            // Away from the cookie's middle seam, where the GPU filters.
            let xs = [-70.0, -30.0, 30.0, 70.0];
            let got = gpu_light(&gpu, &world, true, &xs);
            for (x, g) in xs.iter().zip(&got) {
                let cpu = light_at_textured(Vec2::new(*x, 0.0), &lights, tex).x;
                assert!((g - cpu).abs() < 0.03, "turned {rot}: at {x} the GPU has {g}, the CPU {cpu}");
            }
        }
        // Not loaded: the light waits rather than shining uncut.
        let dark = gpu_light(&gpu, &lit_world(0.0, 1.0), false, &[-30.0]);
        assert!(dark[0] < 0.01, "a light whose cookie has not arrived is not drawn: {dark:?}");
    }
}
