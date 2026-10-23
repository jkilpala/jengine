//! The prefab-instance merge is written twice: in `scene::prefab`, which
//! every runtime runs when a scene loads, and in the editor's
//! `prefabInstance.ts`, which the Inspector shows an instance through and
//! turns edits back into overrides with. Both are checked against one
//! fixture, `fixtures/prefab_merge.json` - the editor's half in
//! `prefabInstance.test.ts` - so a change to either merge that the other did
//! not get fails one of the two.
//!
//! The engine fills each component it reads with the defaults of the fields
//! it omits, so both sides are compared after that same round trip.

use std::collections::HashMap;

use jengine_core::scene::format::{ComponentBundle, EntityData, SceneData};
use jengine_core::scene::expand_prefab_instances;
use serde_json::Value;

fn bundle(components: &Value) -> ComponentBundle {
    serde_json::from_value(components.clone()).unwrap_or_else(|e| panic!("{components}: {e}"))
}

/// Every component of a bundle as JSON, without the marker.
fn components(bundle: &ComponentBundle) -> Value {
    let mut v = serde_json::to_value(bundle).expect("serializes");
    let obj = v.as_object_mut().expect("an object");
    obj.retain(|k, v| k != "prefab_instance" && !v.is_null());
    v
}

#[test]
fn prefab_merge_matches_the_editor() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/prefab_merge.json"))
        .expect("the fixture");
    let fixture: Value = serde_json::from_str(&text).expect("JSON");
    let template = EntityData {
        name: "Hero".into(),
        parent: None,
        children: Vec::new(),
        components: bundle(&fixture["template"]),
    };
    let prefabs: HashMap<String, EntityData> = [("Hero".to_string(), template)].into();
    let cases = fixture["cases"].as_array().expect("cases");
    assert!(cases.len() >= 5, "the fixture's cases");
    for case in cases {
        let name = case["name"].as_str().unwrap_or("?");
        let mut scene = SceneData {
            entities: vec![EntityData {
                name: "it".into(),
                parent: None,
                children: Vec::new(),
                components: bundle(&case["entity"]),
            }],
            ..Default::default()
        };
        let report = expand_prefab_instances(&mut scene, &prefabs);
        assert_eq!(report.expanded, 1, "{name}: expanded");
        let got = components(&scene.entities[0].components);
        let want = components(&bundle(&case["expected"]));
        assert_eq!(got, want, "{name}");
    }
}
