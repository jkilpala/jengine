//! **Smoke** (Oct 2026; `ecs::smoke`): a grid of air carrying smoke - fed
//! by sources, lifted by heat, kept curling and made to flow without piling
//! up - drawn by its entity's sprite through a texture of its own, blocked by
//! colliders and pushed by the ones that move. Found by the showcase's Smoke
//! Chamber. The solver itself is tested in the module.
#![cfg(feature = "smoke")]

use glam::Vec2;
use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::carve::Carvable;
use jengine_core::ecs::components::{Collider2D, ColliderShape, SpriteComponent, Transform};
use jengine_core::ecs::smoke::{Puff, Smoke, SmokeSource, queue_puff, sync, texture_id, tick_smoke};

/// 64 x 32 cells over 256 x 128 world units, centred on (0, 0): a cell is 4
/// units, and cell (x, y) has its centre at (4x - 126, 62 - 4y).
fn air(sources: Vec<SmokeSource>) -> (World, hecs::Entity) {
    let mut world = World::new();
    let e = world.spawn((
        Transform::default(),
        SpriteComponent { size: Some([256.0, 128.0]), ..Default::default() },
        Smoke { cells: [64, 32], sources, fade: 0.0, ..Default::default() },
    ));
    (world, e)
}

fn density_at(world: &World, e: hecs::Entity, p: Vec2) -> f32 {
    let s = world.get::<&Smoke>(e).unwrap();
    let x = ((p.x + 128.0) / 4.0).floor() as i32;
    let y = ((64.0 - p.y) / 4.0).floor() as i32;
    s.state.density_at(x, y)
}

#[test]
fn smoke_is_drawn_through_its_carvable_and_rises_from_its_source() {
    let (mut world, e) = air(vec![SmokeSource {
        at: [0.0, -40.0],
        radius: 10.0,
        density: 6.0,
        temperature: 6.0,
        ..Default::default()
    }]);
    let mut assets = AssetManager::new();
    for _ in 0..90 {
        tick_smoke(&mut world, 1.0 / 60.0);
    }
    sync(&mut world, &mut assets);
    sync(&mut world, &mut assets);
    let id = texture_id(e);
    let tex = assets.get_texture_bytes(&id).expect("its texture");
    assert_eq!((tex.width, tex.height), (64, 32));
    {
        let c = world.get::<&Carvable>(e).expect("a carvable was added");
        assert_eq!(c.state.texture.as_deref(), Some(id.as_str()), "the sprite pass draws the smoke");
        assert!(!c.collider, "smoke is not solid");
    }
    let above = density_at(&world, e, Vec2::new(0.0, 10.0));
    let beside = density_at(&world, e, Vec2::new(100.0, -40.0));
    assert!(above > 0.05, "smoke has risen above its source: {above}");
    assert!(above > beside * 4.0, "more above it than off to the side: {above} / {beside}");
}

#[test]
fn a_collider_keeps_the_smoke_out_and_a_moving_one_pushes_it() {
    let (mut world, e) = air(vec![]);
    // A pillar across the middle, and smoke puffed on both sides of it.
    let pillar = world.spawn((
        Transform { position: Vec2::new(0.0, 0.0), ..Default::default() },
        Collider2D { shape: ColliderShape::Box { width: 24.0, height: 80.0 }, ..Default::default() },
    ));
    for x in [-60.0, 0.0, 60.0] {
        queue_puff(&world, e, Puff { at: Vec2::new(x, 0.0), radius: 20.0, amount: 2.0, velocity: Vec2::ZERO }).unwrap();
    }
    tick_smoke(&mut world, 1.0 / 60.0);
    assert_eq!(density_at(&world, e, Vec2::ZERO), 0.0, "none inside the pillar");
    assert!(density_at(&world, e, Vec2::new(-60.0, 0.0)) > 0.5, "the puffs beside it");
    // Now it sweeps right: the air ahead of it moves with it.
    for k in 1..=10 {
        world.get::<&mut Transform>(pillar).unwrap().position.x = k as f32 * 4.0;
        tick_smoke(&mut world, 1.0 / 60.0);
    }
    let s = world.get::<&Smoke>(e).unwrap();
    // Cell 50 is x = 74, ahead of the pillar (now at 40, 12 either side).
    let ahead = s.state.u[(16 * 64 + 50) as usize];
    assert!(ahead > 10.0, "the air ahead is pushed right: {ahead}");
}

#[test]
fn its_texture_is_sampled_smoothly_whatever_the_projects_default() {
    use jengine_core::renderer::texture_filter::{TextureFilter, TextureFilterRules};
    let rules: TextureFilterRules = serde_json::from_value(serde_json::json!({ "default": "nearest" })).unwrap();
    assert_eq!(rules.filter_for("__smoke/7"), TextureFilter::Linear);
    assert_eq!(rules.filter_for("sprites/a.png"), TextureFilter::Nearest);
}
