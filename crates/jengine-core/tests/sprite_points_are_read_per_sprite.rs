//! `compute_sprite_points` - the per-frame table `Sprite.get_point` answers
//! from - gives every sprite on a pointed region its points, and none to a
//! sprite whose texture has none.
//!
//! It runs over every sprite every frame, so since Oct 2026 it reads the
//! animator from the same query and remembers the last texture's answer
//! rather than hashing its id again; this pins that the shortcut changes
//! nothing - including for two textures alternating, which is where a cache
//! that remembered the wrong thing would show.

use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::{SpriteComponent, Transform};
use jengine_core::renderer::atlas::AtlasData;
use jengine_core::scripting::api::compute_sprite_points;

fn sprite(world: &mut World, texture: &str) -> hecs::Entity {
    world.spawn((Transform::default(), SpriteComponent { texture_id: texture.into(), ..Default::default() }))
}

#[test]
fn every_sprite_on_a_pointed_region_gets_its_points() {
    let atlas: AtlasData = serde_json::from_str(
        r#"{ "width": 64, "height": 64, "regions": [
            { "name": "hero", "x": 0, "y": 0, "width": 32, "height": 16,
              "points": [{ "name": "hand", "x": 16.0, "y": 4.0 }] },
            { "name": "rock", "x": 32, "y": 0, "width": 16, "height": 16 }
        ] }"#,
    )
    .unwrap();
    let mut assets = AssetManager::new();
    assets.register_atlas("sheet".into(), atlas);
    assets.register_atlas_mapping("sprites/hero.png".into(), "sheet".into(), "hero".into());
    assets.register_atlas_mapping("sprites/rock.png".into(), "sheet".into(), "rock".into());

    let mut world = World::new();
    let plain = sprite(&mut world, "sprites/plain.png");
    let hero_a = sprite(&mut world, "sprites/hero.png");
    let rock = sprite(&mut world, "sprites/rock.png");
    let hero_b = sprite(&mut world, "sprites/hero.png");
    let hero_c = sprite(&mut world, "sprites/hero.png");

    let points = compute_sprite_points(&world, &assets);
    for hero in [hero_a, hero_b, hero_c] {
        let p = points.get(&(hero.id() as u64)).and_then(|m| m.get("hand")).copied();
        assert_eq!(p, Some((0.5, 0.25)), "each hero sprite has its hand, normalised");
    }
    assert!(!points.contains_key(&(plain.id() as u64)), "a texture on no atlas has no points");
    assert!(!points.contains_key(&(rock.id() as u64)), "a region with no points adds none");
}
