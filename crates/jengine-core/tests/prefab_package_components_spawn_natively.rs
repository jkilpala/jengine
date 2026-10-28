//! A prefab's package components become the package's types when a script
//! spawns it on native.
//!
//! The native command apply builds entities from the core `ComponentBundle`,
//! which catches every key it does not know in `custom` - so a prefab with a
//! `shooter_body` spawned as a `CustomComponents` bag the package's systems
//! never matched, while the editor, which spawns through the registry, got a
//! real one (BUGS #109). Found building the bullet hell, whose enemies are
//! prefabs with package components.
#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::CustomComponents;
use jengine_core::ecs::systems::apply_script_commands;
use jengine_core::package::component_registry::{self, ComponentRegistry};
use jengine_core::scene::format::EntityData;
use jengine_core::scripting::api::ScriptCommand;
use jengine_core::scripting::spawn_aliases::{self, FIRST_SPAWN_ID};
use serde::{Deserialize, Serialize};

const NATIVE_RUNNER: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct Spin {
    speed: f32,
}

fn prefab() -> EntityData {
    serde_json::from_value(serde_json::json!({
        "name": "Fairy",
        "components": {
            "transform": { "position": [1.0, 2.0], "rotation": 0.0, "scale": [1.0, 1.0] },
            "spin": { "speed": 3.5 },
            "Notes": { "kind": "script data" }
        }
    }))
    .unwrap()
}

#[test]
fn a_spawned_prefab_gets_its_registered_components() {
    spawn_aliases::clear();
    let mut registry = ComponentRegistry::new();
    registry.register::<Spin>("spin", "Spin", "Test");
    component_registry::install_global(registry);
    let mut world = World::new();
    let data = prefab();
    apply_script_commands(
        &mut world,
        &[ScriptCommand::SpawnEntity {
            name: data.name.clone(),
            components: Box::new(Some(data.components)),
            spawn_id: Some(FIRST_SPAWN_ID + 900),
            children: Vec::new(),
            prepared: None,
        }],
        None,
        None,
    );

    // Typed by the spawn itself, so a package command in the same drain finds it.
    let (e, spin) = world.query::<&Spin>().iter().next().map(|(e, s)| (e, s.clone())).expect("a typed Spin");
    assert_eq!(spin, Spin { speed: 3.5 });
    let custom = world.get::<&CustomComponents>(e).expect("the unknown key stays custom");
    assert!(custom.0.contains_key("Notes"));
    assert!(!custom.0.contains_key("spin"), "adopted keys leave the custom bag");
}

#[test]
fn the_native_runner_publishes_its_registry() {
    assert!(
        NATIVE_RUNNER.contains("component_registry::install_global(registry.clone())"),
        "wgpu_scene_runner must publish the registry its packages registered into"
    );
}
