//! The skin editor previews a mesh bending with its bones by skinning it
//! itself, and writes each influence's offset itself
//! (`editor/src/lib/skinEdit.ts`), so both are checked against the engine's
//! `compute_skinned_positions` on one fixture, `fixtures/skin_sample.json` -
//! the editor's half in `skinEdit.test.ts`. A change to either that the
//! other did not get fails one of the two, and a rig that bends one way in
//! the editor bends another in Play.

use glam::Vec2;
use jengine_core::animation::skinned_mesh::{SkinnedMesh, compute_skinned_positions};
use jengine_core::ecs::components::{Bone, Parent, SkeletonRoot, Transform};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use serde_json::Value;

fn v2(v: &Value) -> Vec2 {
    Vec2::new(v[0].as_f64().unwrap() as f32, v[1].as_f64().unwrap() as f32)
}

#[test]
fn skin_sample_matches_the_editor() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/skin_sample.json"))
        .expect("the fixture");
    let fixture: Value = serde_json::from_str(&text).expect("JSON");
    let mut world = hecs::World::new();
    let root = world.spawn((
        Transform {
            position: v2(&fixture["root"]["position"]),
            rotation: fixture["root"]["rotation"].as_f64().unwrap() as f32,
            ..Default::default()
        },
        SkeletonRoot { name: "skin".into() },
    ));
    let mut by_name = std::collections::HashMap::new();
    for bone in fixture["bones"].as_array().unwrap() {
        let name = bone["name"].as_str().unwrap();
        let parent = bone["parent"].as_str().map(|p| by_name[p]).unwrap_or(root);
        let rotation = fixture["pose"][name].as_f64().unwrap_or(bone["rotation"].as_f64().unwrap()) as f32;
        let e = world.spawn((
            Transform { position: v2(&bone["position"]), rotation, ..Default::default() },
            Parent(parent),
            Bone { name: name.into(), length: bone["length"].as_f64().unwrap() as f32 },
        ));
        by_name.insert(name.to_string(), e);
    }
    resolve_world_transforms(&mut world);
    let mesh: SkinnedMesh = serde_json::from_value(fixture["mesh"].clone()).expect("the mesh");
    let got = compute_skinned_positions(&mesh, &world, root);
    let want = fixture["expected"].as_array().unwrap();
    assert_eq!(got.len(), want.len());
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        let w = v2(w);
        assert!((*g - w).length() < 1e-3, "vertex {i}: got {g}, want {w}");
    }
}
