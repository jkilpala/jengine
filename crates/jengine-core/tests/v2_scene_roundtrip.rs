//! A v2 scene survives the editor's load → re-serialize → load path.
//!
//! The editor does not hand the viewport the file on disk. It loads every
//! scene into the **v1** `SceneData` struct, re-serializes that, and pushes the
//! result. `SceneData` had no `version` field, so a v2 scene arrived at the
//! viewport looking like v1 — and `load_scene_auto` then took the
//! `ComponentBundle` path, where a package component is an unknown key that
//! lands in `CustomComponents` instead of becoming a real component.
//!
//! The symptom was a scene that looked completely healthy: scripts running,
//! no warnings, and a package system that found nothing to do. Scene 61
//! reported its requested boid count while the world contained none.

use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::scene::{load_scene_auto, SceneData};
use serde::{Deserialize, Serialize};

/// Stands in for a package component the engine core knows nothing about.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Widget {
    power: f32,
}

const V2_SCENE: &str = r#"{
    "version": 2,
    "name": "v2 test",
    "entities": [
        {
            "name": "Thing",
            "components": {
                "transform": { "position": [3.0, 4.0], "rotation": 0.0, "scale": [1.0, 1.0] },
                "widget": { "power": 9.5 }
            }
        }
    ]
}"#;

fn registry_with_widget() -> ComponentRegistry {
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    registry.register::<Widget>("widget", "Widget", "Test");
    registry
}

#[test]
fn the_version_survives_the_editors_round_trip() {
    // Exactly what the editor does: `load_scene` into SceneData, then
    // `JSON.stringify` it into the viewport.
    let loaded: SceneData = serde_json::from_str(V2_SCENE).expect("parses as SceneData");
    assert_eq!(loaded.version, 2, "version dropped on load");

    let re_emitted = serde_json::to_string(&loaded).expect("re-serializes");
    let v: serde_json::Value = serde_json::from_str(&re_emitted).unwrap();
    assert_eq!(v["version"], 2, "version dropped on re-serialize: {re_emitted}");
}

#[test]
fn a_package_component_is_still_a_real_component_after_the_round_trip() {
    // The assertion that actually matters. Landing in `CustomComponents`
    // instead would leave the owning package's system with nothing to do,
    // which is what happened and why nothing rendered.
    let loaded: SceneData = serde_json::from_str(V2_SCENE).unwrap();
    let re_emitted = serde_json::to_string(&loaded).unwrap();

    let mut world = hecs::World::new();
    load_scene_auto(&re_emitted, &mut world, &registry_with_widget()).expect("loads");

    let widgets: Vec<Widget> = world.query::<&Widget>().iter().map(|(_, w)| w.clone()).collect();
    assert_eq!(
        widgets,
        vec![Widget { power: 9.5 }],
        "the package component did not survive as a real component"
    );
}

#[test]
fn a_v1_scene_is_not_stamped_with_a_version() {
    // v1 files must stay byte-identical: adding `"version": 1` to every scene
    // in the repo would be a huge diff for no behaviour.
    let v1 = r#"{"name":"old","entities":[]}"#;
    let loaded: SceneData = serde_json::from_str(v1).unwrap();
    assert_eq!(loaded.version, 1, "a versionless scene should read as v1");

    let re_emitted = serde_json::to_string(&loaded).unwrap();
    assert!(
        !re_emitted.contains("version"),
        "v1 scene gained a version field: {re_emitted}"
    );
}

#[test]
fn a_v1_scene_still_loads_its_typed_components() {
    // The round-trip must not break the format 60 of the test-lab's scenes use.
    let v1 = r#"{
        "name": "old",
        "entities": [{
            "name": "E",
            "components": {
                "transform": { "position": [1.0, 2.0], "rotation": 0.0, "scale": [1.0, 1.0] }
            }
        }]
    }"#;
    let loaded: SceneData = serde_json::from_str(v1).unwrap();
    let re_emitted = serde_json::to_string(&loaded).unwrap();

    let mut world = hecs::World::new();
    load_scene_auto(&re_emitted, &mut world, &registry_with_widget()).expect("loads");
    let n = world
        .query::<&jengine_core::ecs::components::Transform>()
        .iter()
        .count();
    assert_eq!(n, 1);
}
