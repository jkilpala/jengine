//! **Spline shapes** (Oct 2026): terrain drawn from a curve (Unity's
//! SpriteShape) - a fill tiled with one texture, a strip along the edge tiled
//! with another, and a solid collider of the same shape. Before it, terrain
//! was tiles, a hand-built mesh or a carved image. Found by the showcase's
//! Rolling Hills.
//!
//! The geometry is unit-tested in `renderer::spline_shape`; this checks what
//! reads it: the scene format, the collider (and the browsers' ray snapshot,
//! which must agree with it), the preload list and, on the GPU, the drawing.

use glam::Vec2;
use hecs::World;
use jengine_core::ecs::components::Transform;
use jengine_core::renderer::spline_shape::SplineShape;

/// A gentle hill from x -300 to 300, its top at y 0 in the middle, filled
/// down to -200, placed at `at`.
fn hill() -> SplineShape {
    SplineShape {
        points: vec![[-300.0, -60.0], [-150.0, -20.0], [0.0, 0.0], [150.0, -20.0], [300.0, -60.0]],
        fill_bottom: -200.0,
        fill_texture: "dirt".into(),
        edge_texture: "grass".into(),
        edge_width: 20.0,
        edge_offset: 0.5,
        ..Default::default()
    }
}

/// The curve's height at x, from the built mesh.
fn surface_at(shape: &SplineShape, x: f32) -> f32 {
    let curve = &shape.mesh().curve;
    for w in curve.windows(2) {
        if (w[0].x..=w[1].x).contains(&x) {
            let t = (x - w[0].x) / (w[1].x - w[0].x);
            return w[0].y + (w[1].y - w[0].y) * t;
        }
    }
    panic!("x {x} is off the curve");
}

#[test]
fn the_scene_format_keeps_the_shape_and_not_its_mesh() {
    let mut world = World::new();
    let s = hill();
    let _ = s.mesh();
    world.spawn((Transform::default(), s));
    let scene = jengine_core::scene::format::save_scene(&world);
    let saved = scene.entities[0].components.spline_shape.as_ref().expect("saved");
    assert_eq!(saved.points.len(), 5);
    let json = serde_json::to_string(saved).unwrap();
    assert!(!json.contains("cache"), "{json}");
    let ids = jengine_core::scene::format::world_texture_ids(&world);
    assert!(ids.contains("dirt") && ids.contains("grass"), "both textures are preloaded: {ids:?}");
}

#[cfg(feature = "physics")]
#[test]
fn the_collider_is_the_shape_drawn_and_the_browser_rays_agree() {
    use jengine_core::ecs::components::{BodyType, Collider2D, ColliderShape, RigidBody2D};
    use jengine_core::ecs::systems::ensure_physics_bodies;
    use jengine_core::physics::PhysicsWorld;
    use jengine_core::physics::ray::RaySnapshot;

    let mut world = World::new();
    let at = Vec2::new(40.0, -30.0);
    let shape = world.spawn((Transform { position: at, ..Default::default() }, hill()));
    let mut physics = PhysicsWorld::new(0.0, -9.81, 100.0);
    ensure_physics_bodies(&world, &mut physics);
    physics.step(1.0 / 60.0);
    let shape_ref = (*world.get::<&SplineShape>(shape).unwrap()).clone();
    for x in [-250.0f32, -120.0, 0.0, 90.0, 260.0] {
        let hit = physics.raycast(at.x + x, 400.0, 0.0, -1.0, 1000.0).expect("the hill is solid");
        let want = at.y + surface_at(&shape_ref, x);
        assert!((hit.point_y - want).abs() < 1.0, "at x {x}: met at {} where the curve is {want}", hit.point_y);
        assert_eq!(hit.entity_id, shape.id() as u64);
    }
    // The browsers answer from a snapshot built by the same function.
    let mut snap = RaySnapshot::new();
    snap.refresh(&world);
    for x in (-280..280).step_by(23) {
        let (ox, dx) = (at.x + x as f32, 0.3);
        let a = snap.cast(ox, 400.0, dx, -1.0, 1000.0).map(|h| (h.entity_id, h.distance));
        let b = physics.raycast(ox, 400.0, dx, -1.0, 1000.0).map(|h| (h.entity_id, h.distance));
        match (a, b) {
            (Some(a), Some(b)) => assert!(a.0 == b.0 && (a.1 - b.1).abs() < 0.05, "{a:?} vs {b:?}"),
            (a, b) => assert_eq!(a.is_some(), b.is_some(), "from x {x}: {a:?} vs {b:?}"),
        }
    }
    // A ball dropped on the top comes to rest on it, not in it.
    let ball = world.spawn((
        Transform { position: at + Vec2::new(0.0, 80.0), ..Default::default() },
        RigidBody2D { body_type: BodyType::Dynamic, ..Default::default() },
        Collider2D { shape: ColliderShape::Circle { radius: 10.0 }, ..Default::default() },
    ));
    for _ in 0..120 {
        ensure_physics_bodies(&world, &mut physics);
        physics.step(1.0 / 60.0);
        physics.sync_transforms_to_ecs(&mut world);
    }
    let p = world.get::<&Transform>(ball).unwrap().position;
    let top = at.y + surface_at(&shape_ref, p.x - at.x);
    assert!((p.y - (top + 10.0)).abs() < 3.0, "the ball sits on the surface: {p:?}, surface {top}");
}

#[cfg(feature = "gpu-wgpu")]
mod gpu {
    use super::*;
    use jengine_core::asset::sort_layers::SortLayers;
    use jengine_core::asset::{AssetManager, RawTextureData};
    use jengine_core::ecs::wgpu_systems::{SpriteShadowFlush, WorldLayer, render_sprites_with_shadows_wgpu_materialized};
    use jengine_core::renderer::backend::wgpu::WgpuSpriteBatch;
    use jengine_core::renderer::backend::wgpu::headless::HeadlessGpu;
    use jengine_core::renderer::camera::Camera2D;
    use jengine_core::renderer::font_asset::FrameFonts;
    use jengine_core::renderer::text::built_in_font_atlas;

    const W: u32 = 640;
    const H: u32 = 400;

    fn solid(c: [u8; 3]) -> RawTextureData {
        RawTextureData { width: 8, height: 8, rgba: [c[0], c[1], c[2], 255].repeat(64) }
    }

    #[test]
    fn the_fill_is_under_the_curve_and_the_edge_along_it() {
        let Some(gpu) = HeadlessGpu::new(W, H) else {
            eprintln!("terrain_follows_spline_shapes: no GPU adapter - skipped");
            return;
        };
        let mut assets = AssetManager::new();
        for (id, c) in [("dirt", [200u8, 40, 30]), ("grass", [40u8, 200, 40])] {
            assets.insert_texture_bytes(id.into(), solid(c));
            assets.ensure_wgpu_texture(id, &gpu.device, &gpu.queue).unwrap();
        }
        let (_, _, _, font) = built_in_font_atlas();
        let fonts = FrameFonts::builtin_only(&font, "__font");
        let mut world = World::new();
        let shape = hill();
        world.spawn((Transform::default(), shape.clone()));
        let mut batch = WgpuSpriteBatch::new(&gpu.device, gpu.color_format).unwrap();
        let vp = Camera2D::new(W as f32, H as f32).view_projection();
        let mut encoder = gpu.encoder();
        {
            let mut pass = gpu.color_pass(&mut encoder, wgpu::Color::BLACK);
            batch.begin_frame();
            render_sprites_with_shadows_wgpu_materialized(
                &world, &SortLayers::default(), &mut batch, &fonts, Vec2::splat(64.0), WorldLayer::All,
                |b, tex, kind| {
                    let t = assets.get_wgpu_texture(tex).unwrap_or_else(|| panic!("{tex}"));
                    match kind {
                        SpriteShadowFlush::Shadow => b.flush_multiply(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                        SpriteShadowFlush::Sprite { .. } => b.flush(&gpu.device, &gpu.queue, &mut pass, &vp, t),
                    };
                },
            );
        }
        gpu.submit(encoder);
        let img = gpu.read_rgba();
        let px = |x: f32, y: f32| {
            let (px, py) = ((x + W as f32 / 2.0) as u32, (H as f32 / 2.0 - y) as u32);
            let i = ((py * W + px) * 4) as usize;
            [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
        };
        for x in [-200.0f32, -60.0, 0.0, 120.0] {
            let top = surface_at(&shape, x);
            let fill = px(x, top - 40.0);
            assert!(fill[0] > 150 && fill[1] < 80, "the fill under the curve at x {x}: {fill:?}");
            let edge = px(x, top + 2.0);
            assert!(edge[1] > 150 && edge[0] < 80, "the edge over it, straddling the curve: {edge:?}");
            let sky = px(x, top + 20.0);
            assert!(sky.iter().all(|c| *c < 20), "nothing above the edge: {sky:?}");
        }
    }
}
