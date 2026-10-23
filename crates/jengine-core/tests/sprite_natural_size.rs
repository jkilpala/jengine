//! A sprite with no `size` draws at its texture's own size.
//!
//! Until Sep 2026 it drew at 64 x 64 whatever the texture was, while the
//! field's doc comment promised the texture's size - so a scene authored on
//! that promise with a 32 px texture drew at twice its intended size (the
//! arcade's brick-breaker, paddle game and score digits all did). The rule now matches
//! the promise; this pins it.

use hecs::World;
use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::SpriteComponent;
use jengine_core::ecs::systems::resolve_sprite_regions;
use glam::Vec2;

const FALLBACK: Vec2 = Vec2::new(64.0, 64.0);

fn assets_with(id: &str, width: u32, height: u32) -> AssetManager {
    let mut assets = AssetManager::new();
    assets.insert_texture_bytes(
        id.to_string(),
        RawTextureData { width, height, rgba: vec![255; (width * height * 4) as usize] },
    );
    assets
}

fn drawn(world: &World, e: hecs::Entity) -> Vec2 {
    world.get::<&SpriteComponent>(e).unwrap().drawn_size(FALLBACK)
}

#[test]
fn no_size_draws_at_the_texture_size() {
    let assets = assets_with("sprites/crate.png", 20, 12);
    let mut world = World::new();
    let e = world.spawn((SpriteComponent { texture_id: "sprites/crate.png".into(), ..Default::default() },));
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(drawn(&world, e), Vec2::new(20.0, 12.0));
}

#[test]
fn a_uv_rect_draws_at_the_size_of_the_cell_it_shows() {
    let assets = assets_with("sheet.png", 128, 64);
    let mut world = World::new();
    let e = world.spawn((SpriteComponent {
        texture_id: "sheet.png".into(),
        uv_rect: Some([0.25, 0.0, 0.5, 0.5]),
        ..Default::default()
    },));
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(drawn(&world, e), Vec2::new(32.0, 32.0));
}

#[test]
fn an_explicit_size_always_wins() {
    let assets = assets_with("sprites/crate.png", 20, 12);
    let mut world = World::new();
    let e = world.spawn((SpriteComponent {
        texture_id: "sprites/crate.png".into(),
        size: Some([100.0, 40.0]),
        ..Default::default()
    },));
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(drawn(&world, e), Vec2::new(100.0, 40.0));
}

#[test]
fn the_built_in_white_and_an_unloaded_texture_fall_back() {
    let mut assets = AssetManager::new();
    let mut world = World::new();
    let white = world.spawn((SpriteComponent { texture_id: "__white".into(), ..Default::default() },));
    let later = world.spawn((SpriteComponent { texture_id: "late.png".into(), ..Default::default() },));
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(drawn(&world, white), FALLBACK, "a coloured quad, not one pixel");
    assert_eq!(drawn(&world, later), FALLBACK, "not loaded yet");

    // Once the texture arrives, the sprite takes its size.
    assets.insert_texture_bytes("late.png".into(), RawTextureData { width: 8, height: 40, rgba: vec![0; 8 * 40 * 4] });
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(drawn(&world, later), Vec2::new(8.0, 40.0));
}

#[test]
fn a_texture_swap_is_measured_again() {
    let mut assets = assets_with("a.png", 16, 16);
    assets.insert_texture_bytes("b.png".into(), RawTextureData { width: 48, height: 24, rgba: vec![0; 48 * 24 * 4] });
    let mut world = World::new();
    let e = world.spawn((SpriteComponent { texture_id: "a.png".into(), ..Default::default() },));
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(drawn(&world, e), Vec2::new(16.0, 16.0));
    world.get::<&mut SpriteComponent>(e).unwrap().texture_id = "b.png".into();
    resolve_sprite_regions(&mut world, &assets);
    assert_eq!(drawn(&world, e), Vec2::new(48.0, 24.0));
}
