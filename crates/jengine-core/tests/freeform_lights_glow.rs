//! Freeform lights (`renderer::freeform_light`): a polygon of light, full
//! inside and fading over its falloff outside, drawn into the light buffer
//! beside the lightmaps - and `light_at` answering exactly what the screen
//! shows. The GPU half needs an adapter; without one it prints why and
//! passes.

use glam::{Vec2, Vec3};
use hecs::World;
use jengine_core::ecs::components::{AmbientLight, Transform};
use jengine_core::renderer::freeform_light::{FreeformLight, mesh, place};
use jengine_core::renderer::lighting::{gather_lights, light_at};

/// An L, so the outline has a concave corner: the light must not double up
/// in it, and its inside must be whole.
fn l_shape() -> FreeformLight {
    FreeformLight {
        points: vec![[-120.0, -80.0], [120.0, -80.0], [120.0, -10.0], [-40.0, -10.0], [-40.0, 80.0], [-120.0, 80.0]],
        color: [1.0, 0.6, 0.3],
        intensity: 0.8,
        falloff: 36.0,
    }
}

#[test]
fn light_at_sees_a_freeform_light() {
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    world.spawn((Transform { position: Vec2::new(10.0, 0.0), ..Default::default() }, l_shape()));
    let lights = gather_lights(&world);
    let full = Vec3::new(0.8, 0.48, 0.24);
    assert!((light_at(Vec2::new(-70.0, 0.0), &lights) - full).abs().max_element() < 1e-5, "inside the upright");
    assert!((light_at(Vec2::new(80.0, -40.0), &lights) - full).abs().max_element() < 1e-5, "inside the foot");
    // Out past the falloff: nothing. In the notch of the L, 18 from both
    // arms: the same as 18 off one edge - not twice it.
    assert_eq!(light_at(Vec2::new(200.0, 70.0), &lights), Vec3::ZERO);
    let notch = light_at(Vec2::new(-12.0, 8.0), &lights);
    let edge = light_at(Vec2::new(130.0 + 18.0, -40.0), &lights);
    assert!((notch - edge).abs().max_element() < 1e-4, "{notch} vs {edge}");
    // Turned and scaled with its entity.
    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.0 },));
    world.spawn((
        Transform { position: Vec2::ZERO, rotation: std::f32::consts::FRAC_PI_2, scale: Vec2::splat(0.5) },
        FreeformLight { points: vec![[0.0, 0.0], [200.0, 0.0], [200.0, 20.0], [0.0, 20.0]], falloff: 0.0, ..Default::default() },
    ));
    let lights = gather_lights(&world);
    assert_eq!(light_at(Vec2::new(-5.0, 90.0), &lights), Vec3::ONE, "a long bar turned upright and halved");
    assert_eq!(light_at(Vec2::new(90.0, 5.0), &lights), Vec3::ZERO);
}

#[test]
fn the_mesh_covers_the_outline_and_the_falloff() {
    let placed = place(&l_shape(), Vec2::ZERO, 0.0, Vec2::ONE).unwrap();
    let (pos, weights, indices) = mesh(&placed);
    assert_eq!(indices.len() % 3, 0);
    assert!(indices.iter().all(|i| (*i as usize) < pos.len()));
    // The inside's triangles and the rings' quads add up to the area the
    // light reaches: the L plus a band round it.
    let area: f32 = indices
        .chunks(3)
        .map(|t| {
            let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
            ((b - a).perp_dot(c - a)).abs() * 0.5
        })
        .sum();
    // The L, a band of the falloff's width along its 800 units of outline,
    // a quarter disc at each of its five convex corners, less the square
    // the two strips at its concave corner do not both cover.
    let f = 36.0f32;
    let want = 240.0 * 70.0 + 80.0 * 90.0 + 800.0 * f + std::f32::consts::PI * f * f - f * f;
    assert!((area - want).abs() < want * 0.03, "{area} against {want}");
    assert!(weights.iter().all(|w| (0.0..=1.0).contains(w)));
}

#[cfg(feature = "gpu-wgpu")]
#[test]
fn the_screen_shows_what_light_at_says() {
    use jengine_core::asset::{AssetManager, RawTextureData, WHITE_TEXTURE_ID};
    use jengine_core::ecs::wgpu_systems::render_lightmaps_wgpu;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::backend::wgpu::{WgpuLightingPass, WgpuSpriteBatch};
    use jengine_core::renderer::camera::Camera2D;
    const W: u32 = 320;
    const H: u32 = 240;
    let Some(gpu) = HeadlessGpu::new(W, H) else {
        eprintln!("freeform_lights_glow: no GPU adapter - skipped");
        return;
    };
    let lp = WgpuLightingPass::new(&gpu.device, W, H, gpu.color_format).expect("lighting pass");
    let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).expect("sprite batch");
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(WHITE_TEXTURE_ID.into(), RawTextureData { width: 1, height: 1, rgba: vec![255; 4] });
    assets.ensure_wgpu_texture(WHITE_TEXTURE_ID, &gpu.device, &gpu.queue).unwrap();

    let mut world = World::new();
    world.spawn((AmbientLight { color: [1.0; 3], intensity: 0.05 },));
    world.spawn((Transform { position: Vec2::new(10.0, 0.0), ..Default::default() }, l_shape()));
    let lights = gather_lights(&world);
    let camera = Camera2D::new(W as f32, H as f32);
    let vp = camera.view_projection();
    let mut encoder = gpu.encoder();
    drop(lp.scene_target().begin_pass(&mut encoder, Some(wgpu::Color::WHITE)));
    lp.render_lights(&gpu.device, &gpu.queue, &mut encoder, &camera, &lights);
    let mut flushed = Vec::new();
    {
        let mut pass = lp.light_buffer_pass(&mut encoder);
        let drawn = render_lightmaps_wgpu(&world, &mut batch, |b, tex_id, _| {
            flushed.push(tex_id.to_string());
            let tex = assets.get_wgpu_texture(tex_id).expect("the white texture");
            b.flush_additive(&gpu.device, &gpu.queue, &mut pass, &vp, tex);
        });
        assert_eq!(drawn, 1, "one freeform light drawn");
    }
    assert_eq!(flushed, vec![WHITE_TEXTURE_ID.to_string()]);
    lp.composite(&mut encoder, gpu.view());
    gpu.submit(encoder);
    let img = gpu.read_rgba();

    let to_srgb = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let e = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (e * 255.0).round() as i32
    };
    let (mut compared, mut inside, mut fading) = (0, 0, 0);
    for py in (1..H).step_by(2) {
        for px in (1..W).step_by(2) {
            let p = Vec2::new(px as f32 + 0.5 - W as f32 / 2.0, H as f32 / 2.0 - (py as f32 + 0.5));
            let want = light_at(p, &lights);
            // The outline itself falls between pixels; skip where a step
            // either way changes the answer much.
            let steady = [(-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5)]
                .iter()
                .all(|(dx, dy)| (light_at(p + Vec2::new(*dx, *dy), &lights) - want).abs().max_element() < 0.06);
            if !steady {
                continue;
            }
            let i = ((py * W + px) * 4) as usize;
            for ch in 0..3 {
                let got = img.rgba[i + ch] as i32;
                let exp = to_srgb(want[ch]);
                assert!(
                    (got - exp).abs() <= 5,
                    "pixel ({px}, {py}) channel {ch}: the screen shows {got}, light_at says {exp}"
                );
            }
            compared += 1;
            if want.x > 0.8 {
                inside += 1;
            } else if want.x > 0.1 {
                fading += 1;
            }
        }
    }
    assert!(compared > 15_000, "{compared}");
    assert!(inside > 1000 && fading > 400, "inside {inside}, fading {fading}");
}

#[test]
fn the_light_setters_reach_a_freeform_light() {
    use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
    struct Host(World);
    impl ScriptWorldHost for Host {
        fn world(&mut self) -> &mut World {
            &mut self.0
        }
        fn resolve_entity(&mut self, id: u32) -> Option<hecs::Entity> {
            self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
        }
    }
    let mut world = World::new();
    let e = world.spawn((Transform::default(), l_shape()));
    let mut host = Host(world);
    assert!(handle_world_command(&mut host, "sli", &JsonArgs(serde_json::json!({ "e": e.id(), "i": 2.5 }))));
    assert!(handle_world_command(&mut host, "slc", &JsonArgs(serde_json::json!({ "e": e.id(), "r": 0.1, "g": 0.2, "b": 0.9 }))));
    let l = host.0.get::<&FreeformLight>(e).unwrap();
    assert_eq!((l.intensity, l.color), (2.5, [0.1, 0.2, 0.9]));
}
