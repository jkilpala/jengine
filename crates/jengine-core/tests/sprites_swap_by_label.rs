//! Sprite swapping (Oct 2026; Unity's Sprite Library and Resolver): a
//! `sprite_library` holds parts by category and label, a `sprite_resolver`
//! on a sprite shows one of them, and `Sprite.set_label` changes it - on a
//! part, or on the library's entity for every part at once. Resolved by the
//! pass every host already runs before drawing (`resolve_sprite_regions`),
//! and every part is preloaded with the scene, since nothing names a part
//! until it is worn.

use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::SpriteComponent;
use jengine_core::ecs::sprite_library::set_label;
use jengine_core::ecs::systems::resolve_sprite_regions;
use jengine_core::scene::format::{prefab_texture_ids, world_texture_ids, EntityData};
use jengine_core::scene::{load_scene, save_scene, SceneData};

const SCENE: &str = r#"{
  "name": "doll",
  "entities": [
    { "name": "Doll", "components": {
        "transform": { "position": [0, 0], "rotation": 0, "scale": [1, 1] },
        "sprite_library": { "categories": {
          "hat":  { "summer": { "texture_id": "dolls/hat_summer.png" },
                    "winter": { "texture_id": "dolls/hat_winter.png", "size": [80, 50] } },
          "boot": { "summer": { "texture_id": "dolls/boot_summer.png" },
                    "winter": { "texture_id": "dolls/boot_winter.png" } } } } } },
    { "name": "Head", "parent": "Doll", "components": {
        "transform": { "position": [0, 100], "rotation": 0, "scale": [1, 1] } } },
    { "name": "Hat", "parent": "Head", "components": {
        "transform": { "position": [0, 40], "rotation": 0, "scale": [1, 1] },
        "sprite": { "texture_id": "", "color": [1, 1, 1, 1], "z_order": 3, "size": [60, 40] },
        "sprite_resolver": { "category": "hat", "label": "summer" } } },
    { "name": "BootL", "parent": "Doll", "components": {
        "transform": { "position": [-20, 0], "rotation": 0, "scale": [1, 1] },
        "sprite": { "texture_id": "", "color": [1, 1, 1, 1], "z_order": 3, "size": [30, 30] },
        "sprite_resolver": { "category": "boot", "label": "summer" } } },
    { "name": "BootR", "parent": "Doll", "components": {
        "transform": { "position": [20, 0], "rotation": 0, "scale": [1, 1] },
        "sprite": { "texture_id": "", "color": [1, 1, 1, 1], "z_order": 3, "size": [30, 30] },
        "sprite_resolver": { "category": "boot", "label": "summer" } } }
  ]
}"#;

fn load() -> World {
    let data: SceneData = serde_json::from_str(SCENE).expect("the scene parses");
    let mut world = World::new();
    load_scene(&data, &mut world);
    world
}

fn named(w: &World, name: &str) -> hecs::Entity {
    w.query::<&jengine_core::ecs::components::Name>()
        .iter()
        .find(|(_, n)| n.0 == name)
        .map(|(e, _)| e)
        .unwrap_or_else(|| panic!("no {name}"))
}

fn tex(w: &World, name: &str) -> String {
    w.get::<&SpriteComponent>(named(w, name)).unwrap().texture_id.to_string()
}

#[test]
fn a_loaded_doll_wears_its_labels_and_changes_outfit_in_one_call() {
    let mut w = load();
    let assets = AssetManager::new();
    resolve_sprite_regions(&mut w, &assets);
    assert_eq!(tex(&w, "Hat"), "dolls/hat_summer.png", "found two parents up");
    assert_eq!(tex(&w, "BootL"), "dolls/boot_summer.png");

    let doll = named(&w, "Doll");
    assert_eq!(set_label(&w, doll, "winter", None), Ok(3));
    resolve_sprite_regions(&mut w, &assets);
    assert_eq!((tex(&w, "Hat"), tex(&w, "BootL"), tex(&w, "BootR")),
               ("dolls/hat_winter.png".into(), "dolls/boot_winter.png".into(), "dolls/boot_winter.png".into()));
    assert_eq!(w.get::<&SpriteComponent>(named(&w, "Hat")).unwrap().size, Some([80.0, 50.0]), "the entry's size");

    assert_eq!(set_label(&w, named(&w, "BootL"), "summer", None), Ok(1), "one part alone");
    resolve_sprite_regions(&mut w, &assets);
    assert_eq!((tex(&w, "BootL"), tex(&w, "BootR")), ("dolls/boot_summer.png".into(), "dolls/boot_winter.png".into()));
}

#[test]
fn every_part_is_preloaded_with_the_scene_and_with_a_prefab() {
    let w = load();
    let ids = world_texture_ids(&w);
    for id in ["dolls/hat_summer.png", "dolls/hat_winter.png", "dolls/boot_summer.png", "dolls/boot_winter.png"] {
        assert!(ids.contains(id), "{id} is preloaded: {ids:?}");
    }
    let data: SceneData = serde_json::from_str(SCENE).unwrap();
    let doll: Vec<EntityData> = data.entities.into_iter().filter(|e| e.name == "Doll").collect();
    let ids = prefab_texture_ids(doll.iter());
    assert!(ids.contains("dolls/hat_winter.png"), "a prefab carrying the library: {ids:?}");
}

#[test]
fn the_library_and_resolver_round_trip_through_a_save() {
    let w = load();
    let saved = save_scene(&w);
    let text = serde_json::to_string(&saved).unwrap();
    assert!(text.contains("\"sprite_library\"") && text.contains("\"sprite_resolver\""));
    assert!(!text.contains("\"applied\""), "the resolver's bookkeeping is never saved");
}
