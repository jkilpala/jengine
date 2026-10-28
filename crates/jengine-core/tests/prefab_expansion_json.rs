//! BUGS #12 — `prefab_instance` expansion is one implementation, used by all
//! three runtimes.
//!
//! The JSON wrapper around `expand_prefab_instances` was copied into
//! `jengine-runtime` and `jengine-web` and **missing entirely** from the editor
//! viewport, so an entity whose content came from a `PrefabInstance` marker was
//! empty on the surface most authoring happens on. The two copies had also
//! drifted: only the web one preserved top-level scene keys that `SceneData`
//! does not model.

use jengine_core::scene::{expand_prefab_instances_in_json, format::EntityData};
use std::collections::HashMap;

fn prefabs() -> HashMap<String, EntityData> {
    let template: EntityData = serde_json::from_str(
        r#"{
            "name": "Coin",
            "components": {
                "transform": { "position": [0.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] },
                "sprite": { "texture_id": "coin.png", "color": [1.0, 1.0, 1.0, 1.0], "z_order": 0 }
            }
        }"#,
    )
    .expect("prefab parses");
    HashMap::from([("Coin".to_string(), template)])
}

const SCENE: &str = r#"{
    "name": "test",
    "audio_refs": ["sounds/hum.wav"],
    "entities": [
        {
            "name": "Coin1",
            "components": {
                "transform": { "position": [10.0, 20.0], "rotation": 0.0, "scale": [1.0, 1.0] },
                "prefab_instance": { "prefab_id": "Coin", "overrides": {} }
            }
        }
    ]
}"#;

#[test]
fn expands_a_marker_into_the_template_components() {
    let out = expand_prefab_instances_in_json(SCENE, &prefabs(), "test");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let comps = &v["entities"][0]["components"];

    // The whole point: the sprite comes from the template, not the scene.
    assert_eq!(comps["sprite"]["texture_id"], "coin.png");
    // The instance's own transform wins over the template's.
    assert_eq!(comps["transform"]["position"][0], 10.0);
}

#[test]
fn preserves_top_level_keys_scene_data_does_not_model() {
    // The native copy round-tripped through `SceneData` and dropped
    // `audio_refs` — silently, surfacing much later as a sound that never
    // plays. Any scene combining prefab instances with audio hit it.
    let out = expand_prefab_instances_in_json(SCENE, &prefabs(), "test");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["audio_refs"][0], "sounds/hum.wav");
}

#[test]
fn passes_through_what_it_cannot_confidently_rewrite() {
    // Malformed JSON and v2 scenes go through untouched so the real loader
    // reports the real diagnostic instead of this function masking it.
    assert_eq!(expand_prefab_instances_in_json("not json", &prefabs(), "test"), "not json");

    let v2 = r#"{"version": 2, "entities": []}"#;
    assert_eq!(expand_prefab_instances_in_json(v2, &prefabs(), "test"), v2);
}

#[test]
fn a_missing_prefab_leaves_the_entity_alone_rather_than_dropping_it() {
    let scene = SCENE.replace("\"prefab_id\": \"Coin\"", "\"prefab_id\": \"Nope\"");
    let out = expand_prefab_instances_in_json(&scene, &prefabs(), "test");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();

    // The marker stays and the entity survives; the caller logs the warning.
    // Dropping it would make a typo'd prefab id look like a scene that never
    // had the entity.
    assert_eq!(v["entities"][0]["name"], "Coin1");
    assert!(v["entities"][0]["components"]["prefab_instance"].is_object());
}

/// Runtimes parse a scene once and expand it in place
/// (`expand_prefab_instances_in_value`); the text version is a wrapper over
/// it, so the two must agree to the byte.
#[test]
fn expanding_in_place_is_the_text_version_without_the_text() {
    use jengine_core::scene::expand_prefab_instances_in_value;
    let mut value: serde_json::Value = serde_json::from_str(SCENE).unwrap();
    assert!(expand_prefab_instances_in_value(&mut value, &prefabs(), "test"));
    let text: serde_json::Value =
        serde_json::from_str(&expand_prefab_instances_in_json(SCENE, &prefabs(), "test")).unwrap();
    assert_eq!(value, text);
    // What it cannot rewrite it leaves exactly as it was.
    let mut odd = serde_json::json!({ "entities": "not a list" });
    assert!(!expand_prefab_instances_in_value(&mut odd, &prefabs(), "test"));
    assert_eq!(odd, serde_json::json!({ "entities": "not a list" }));
}

/// The scene-wide fields a runtime keeps once the scene itself has gone to
/// the loader: everything but the entities.
#[test]
fn a_scene_header_is_everything_but_the_entities() {
    use jengine_core::scene::format::{scene_header, scene_metadata};
    let scene: serde_json::Value = serde_json::from_str(
        r#"{ "name": "n", "audio_refs": ["a.wav"], "sort_layers": "x.sortlayers",
             "post_process": { "effects": [] }, "entities": [{ "name": "E" }] }"#,
    )
    .unwrap();
    let header = scene_header(&scene);
    assert!(header.get("entities").is_none());
    assert_eq!(header["audio_refs"][0], "a.wav");
    assert_eq!(jengine_core::scene::name_in(&header).as_deref(), Some("n"));
    assert_eq!(scene_metadata(&header).sort_layers.as_deref(), Some("x.sortlayers"));
    assert_eq!(scene_metadata(&header), scene_metadata(&scene));
}

/// A scene a script enters natively (`Scene.load`, `push_scene`) or streams
/// in goes through `prepare_scene`, which expands its prefab instances. Until
/// Oct 2026 `reload_scene` did not, so a scene placing prefabs, entered from a
/// hub, loaded with its placements unexpanded on native alone.
#[test]
fn every_native_scene_load_expands_its_prefabs() {
    let src = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
    let body = |name: &str| {
        let at = src.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("no fn {name}"));
        let tail = &src[at..];
        &tail[..tail.find("\n}\n").unwrap_or(tail.len())]
    };
    assert!(body("prepare_scene").contains("expand_prefab_instances_in_value("));
    assert!(body("reload_scene").contains("prepare_scene("), "Scene.load must expand prefab instances");
    assert!(body("build_state").contains("expand_prefab_instances_in_value("));
}
