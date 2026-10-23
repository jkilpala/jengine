//! **Prefab variants** (Oct 2026): a `.prefab` whose own components carry a
//! `prefab_instance` extends the prefab it names - the base, its own
//! components over it, then the instance's overrides - so a family of
//! creatures is one prefab and its differences. Scene instances with
//! overrides existed; a prefab could not build on another prefab, so every
//! variant was a full copy that drifted from the original. Found by the
//! showcase's Monster Zoo.

use std::collections::HashMap;

use jengine_core::scene::format::EntityData;
use jengine_core::scene::resolve_prefab_variants;

fn prefab(json: serde_json::Value) -> EntityData {
    serde_json::from_value(json).expect("a prefab")
}

fn zoo() -> HashMap<String, EntityData> {
    let mut m = HashMap::new();
    m.insert("Slime".into(), prefab(serde_json::json!({ "name": "Slime", "components": {
        "transform": { "position": [0, 0], "rotation": 0, "scale": [1, 1] },
        "sprite": { "texture_id": "slime.png", "color": [0.4, 1.0, 0.4, 1.0], "z_order": 3, "size": [64, 64] },
        "velocity": { "x": 20, "y": 0 } } })));
    m.insert("FireSlime".into(), prefab(serde_json::json!({ "name": "FireSlime", "components": {
        "prefab_instance": { "prefab_id": "Slime", "overrides": { "sprite.color": [1.0, 0.4, 0.2, 1.0] } },
        "point_light": { "radius": 120, "color": [1.0, 0.5, 0.2], "intensity": 1.5, "falloff": 2.0 } } })));
    m.insert("BigFireSlime".into(), prefab(serde_json::json!({ "name": "BigFireSlime", "components": {
        "prefab_instance": { "prefab_id": "FireSlime", "overrides": { "transform.scale": [2.0, 2.0] } } } })));
    m
}

#[test]
fn a_variant_is_its_base_with_its_own_components_and_overrides_over_it() {
    let r = resolve_prefab_variants(&zoo());
    let fire = &r["FireSlime"].components;
    let sprite = fire.sprite.as_ref().expect("the base's sprite");
    assert_eq!(sprite.texture_id, "slime.png");
    assert_eq!(sprite.color, [1.0, 0.4, 0.2, 1.0], "the override");
    assert!(fire.point_light.is_some(), "its own component");
    assert!(fire.velocity.is_some(), "the base's other components");
    assert_eq!(fire.prefab_instance.as_ref().map(|i| i.prefab_id.as_str()), Some("Slime"), "it names its base");
    // The base is untouched.
    assert_eq!(r["Slime"].components.sprite.as_ref().unwrap().color, [0.4, 1.0, 0.4, 1.0]);
    assert!(r["Slime"].components.point_light.is_none());
}

#[test]
fn a_variant_of_a_variant_inherits_both() {
    let r = resolve_prefab_variants(&zoo());
    let big = &r["BigFireSlime"].components;
    assert_eq!(big.transform.as_ref().unwrap().scale.to_array(), [2.0, 2.0]);
    assert_eq!(big.sprite.as_ref().unwrap().color, [1.0, 0.4, 0.2, 1.0], "the middle variant's override");
    assert!(big.point_light.is_some(), "the middle variant's component");
    assert_eq!(big.sprite.as_ref().unwrap().texture_id, "slime.png", "the root's");
}

#[test]
fn a_scene_instance_of_a_variant_gets_the_whole_chain() {
    use jengine_core::scene::format::SceneData;
    let prefabs = resolve_prefab_variants(&zoo());
    let mut scene: SceneData = serde_json::from_value(serde_json::json!({
        "name": "pen", "entities": [ { "name": "Pet", "components": {
            "prefab_instance": { "prefab_id": "FireSlime", "overrides": { "transform.position": [100, 50] } } } } ] }))
    .unwrap();
    let report = jengine_core::scene::expand_prefab_instances(&mut scene, &prefabs);
    assert_eq!(report.expanded, 1);
    let c = &scene.entities[0].components;
    assert_eq!(c.transform.as_ref().unwrap().position.to_array(), [100.0, 50.0]);
    assert_eq!(c.sprite.as_ref().unwrap().color, [1.0, 0.4, 0.2, 1.0]);
    assert!(c.point_light.is_some() && c.velocity.is_some());
}

#[test]
fn a_cycle_or_a_missing_base_is_left_as_authored() {
    let mut m = HashMap::new();
    m.insert("A".into(), prefab(serde_json::json!({ "name": "A", "components": {
        "prefab_instance": { "prefab_id": "B" }, "sprite": { "texture_id": "a.png", "color": [1, 1, 1, 1], "z_order": 0 } } })));
    m.insert("B".into(), prefab(serde_json::json!({ "name": "B", "components": {
        "prefab_instance": { "prefab_id": "A" }, "sprite": { "texture_id": "b.png", "color": [1, 1, 1, 1], "z_order": 0 } } })));
    m.insert("Orphan".into(), prefab(serde_json::json!({ "name": "Orphan", "components": {
        "prefab_instance": { "prefab_id": "Nobody" }, "sprite": { "texture_id": "o.png", "color": [1, 1, 1, 1], "z_order": 0 } } })));
    let r = resolve_prefab_variants(&m);
    assert_eq!(r.len(), 3, "nothing dropped");
    assert_eq!(r["Orphan"].components.sprite.as_ref().unwrap().texture_id, "o.png");
    for k in ["A", "B"] {
        assert!(r[k].components.sprite.is_some(), "{k} kept its own sprite");
    }
}

#[test]
fn every_runtime_resolves_its_prefabs() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for host in [
        "crates/jengine-runtime/src/wgpu_scene_runner.rs",
        "crates/jengine-web/src/lib.rs",
        "crates/jengine-editor-viewport/src/lib.rs",
    ] {
        let src = std::fs::read_to_string(root.join(host)).unwrap();
        assert!(src.contains("resolve_prefab_variants("), "{host} hands variants out unresolved");
    }
}
