//! The editor lists a placed prefab's parts in the scene tree and edits one
//! by writing a scene entity laid over it (`editor/src/lib/prefabParts.ts`),
//! so it names parts and merges overlays itself. Both are checked against
//! the engine's expansion (`expand_prefab_instances`, `flatten_children`)
//! on one fixture, `fixtures/prefab_parts.json` - the editor's half in
//! `prefabParts.test.ts`. A change to either that the other did not get
//! fails one of the two, and a part edited in the editor is not the part
//! that plays.

use std::collections::HashMap;

use jengine_core::scene::format::{EntityData, SceneData};
use jengine_core::scene::{expand_prefab_instances, resolve_prefab_variants};
use serde_json::Value;

#[test]
fn prefab_parts_match_the_editor() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/prefab_parts.json"))
        .expect("the fixture");
    let fixture: Value = serde_json::from_str(&text).expect("JSON");
    let mut prefabs = HashMap::new();
    for p in fixture["prefabs"].as_array().unwrap() {
        let d: EntityData = serde_json::from_value(p.clone()).expect("a prefab");
        prefabs.insert(d.name.clone(), d);
    }
    let resolved = resolve_prefab_variants(&prefabs);
    let mut scene: SceneData = serde_json::from_value(fixture["scene"].clone()).expect("the scene");
    expand_prefab_instances(&mut scene, &resolved);

    let mut got: Vec<(String, Option<String>, [f32; 2], Option<String>)> = scene
        .entities
        .iter()
        .map(|e| {
            let c = &e.components;
            let pos = c.get::<jengine_core::ecs::components::Transform>("transform").map(|t| [t.position.x, t.position.y]).unwrap_or([0.0, 0.0]);
            let tex = c.get::<jengine_core::ecs::components::SpriteComponent>("sprite").map(|s| s.texture_id.as_str().to_string());
            (e.name.clone(), e.parent.clone(), pos, tex)
        })
        .collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    let want: Vec<(String, Option<String>, [f32; 2], Option<String>)> = fixture["expected"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["name"].as_str().unwrap().to_string(),
                e["parent"].as_str().map(str::to_string),
                [e["position"][0].as_f64().unwrap() as f32, e["position"][1].as_f64().unwrap() as f32],
                e["texture"].as_str().map(str::to_string),
            )
        })
        .collect();
    assert_eq!(got, want);
}
