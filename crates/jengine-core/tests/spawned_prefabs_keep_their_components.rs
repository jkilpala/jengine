//! A spawned prefab keeps every component the scene loader would give it.
//!
//! The native `ScriptCommand::SpawnEntity` arm used to be a hand-copied
//! subset of the v1 `ComponentBundle` while the editor viewport spawned
//! generically through the registry - so a prefab carrying one of the missing
//! fields worked in the editor and silently lost it on native. Measured Sep
//! 2026: **45 of the 77 fields were missing**, `material` among them, which is
//! part of why no game had ever used a graph material. Then a shared
//! `add_to_builder` was the one list, and this test checked it against the
//! struct.
//!
//! Since Oct 2026 there is no list: a bundle is JSON by scene key, and every
//! path - the scene loader, a script's spawn of a loaded prefab (decoded once,
//! `PreparedPrefab`), the command arm with a bundle in hand, the browsers -
//! decodes through the component registry. So this spawns **every registered
//! component** each native way and checks each one arrived.
#![cfg(feature = "native")]

use std::sync::Arc;

use hecs::World;
use jengine_core::ecs::components::Name;
use jengine_core::ecs::systems::apply_script_commands;
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::scene::format::{ComponentBundle, EntityData, SceneData};
use jengine_core::scene::prefab::PreparedPrefab;
use jengine_core::scripting::api::{ScriptCommand, ScriptServices};

fn registry() -> ComponentRegistry {
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    registry
}

/// One entity holding every registered component that has a default, at its
/// default - and the keys, to look for.
fn everything(registry: &ComponentRegistry) -> (EntityData, Vec<String>) {
    let mut bundle = ComponentBundle::new();
    let mut keys = Vec::new();
    for desc in registry.iter() {
        let Some(default) = desc.default_fn else { continue };
        bundle.insert_value(desc.name.clone(), (desc.serialize_fn)(default().as_ref()));
        keys.push(desc.name.clone());
    }
    keys.sort();
    let data = EntityData { name: "Everything".into(), parent: None, components: bundle, children: Vec::new() };
    (data, keys)
}

/// The keys of `keys` the entity named "Everything" in `world` does not have.
fn missing(world: &World, registry: &ComponentRegistry, keys: &[String]) -> Vec<String> {
    let e = world
        .query::<&Name>()
        .iter()
        .find(|(_, n)| n.0 == "Everything")
        .map(|(e, _)| e)
        .expect("the entity spawned");
    keys.iter().filter(|k| (registry.get(k).unwrap().query_fn)(world, e).is_none()).cloned().collect()
}

#[test]
fn every_registered_component_survives_each_native_spawn() {
    let registry = registry();
    let (data, keys) = everything(&registry);
    assert!(keys.len() > 150, "only {} components with a default - did the registry move?", keys.len());

    // A loaded prefab, as a script's `spawn_prefab` spawns it.
    let mut world = World::new();
    PreparedPrefab::new(&data, &registry).spawn(&mut world, "Everything");
    assert_eq!(missing(&world, &registry, &keys), Vec::<String>::new(), "lost by a prepared prefab");

    // The command arm with a bundle in hand.
    let mut world = World::new();
    apply_script_commands(
        &mut world,
        &[ScriptCommand::SpawnEntity {
            name: "Everything".into(),
            components: Box::new(Some(data.components.clone())),
            spawn_id: None,
            children: Vec::new(),
            prepared: None,
        }],
        None,
        None,
    );
    assert_eq!(missing(&world, &registry, &keys), Vec::<String>::new(), "lost by the command arm");

    // The v1 scene loader, and back out through the saver.
    let scene = SceneData { entities: vec![data], ..Default::default() };
    let mut world = World::new();
    jengine_core::scene::format::load_scene_with_registry(&scene, &mut world, &registry);
    assert_eq!(missing(&world, &registry, &keys), Vec::<String>::new(), "lost by the v1 loader");
    let saved = jengine_core::scene::format::save_scene_with(&world, &registry);
    let entity = saved.entities.iter().find(|e| e.name == "Everything").unwrap();
    let dropped: Vec<&String> = keys.iter().filter(|k| !entity.components.contains(k) && **k != "tags").collect();
    assert!(dropped.is_empty(), "lost by the v1 saver: {dropped:?}");
}

#[test]
fn a_prepared_prefab_is_made_once_and_forgotten_when_the_map_changes() {
    let mut services = ScriptServices::default();
    let bullet: EntityData = serde_json::from_value(serde_json::json!({
        "name": "Bullet", "components": { "transform": { "position": [1, 2], "rotation": 0, "scale": [1, 1] } }
    }))
    .unwrap();
    services.prefabs_mut().insert("Bullet".into(), bullet.clone());

    let first = services.prepared_prefab("Bullet").expect("a loaded prefab");
    let again = services.prepared_prefab("Bullet").unwrap();
    assert!(Arc::ptr_eq(&first, &again), "decoded once, then reused");
    assert!(services.prepared_prefab("Nothing").is_none());

    // A pack replacing the prefab: the next spawn must see the new one.
    let mut moved = bullet;
    moved.components.value_mut("transform").unwrap()["position"] = serde_json::json!([9, 9]);
    services.prefabs_mut().insert("Bullet".into(), moved);
    let after = services.prepared_prefab("Bullet").unwrap();
    assert!(!Arc::ptr_eq(&first, &after), "a changed map is decoded again");
    let mut world = World::new();
    let e = after.spawn(&mut world, "Bullet");
    assert_eq!(world.get::<&jengine_core::ecs::components::Transform>(e).unwrap().position.x, 9.0);
}
