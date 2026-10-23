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
