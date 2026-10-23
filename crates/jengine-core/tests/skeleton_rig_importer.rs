//! Integration tests for Slice 3.5.D — skeleton rig importer.
//!
//! End-to-end: parse a Spine-subset JSON document via the asset
//! manager, spawn the rig into a world, tick the slice-C player
//! against the imported clip, and confirm the slice-A resolver
//! produces the expected world-space bone pose.
#![cfg(feature = "native")]

use std::collections::HashMap;

use jengine_core::animation::rig::{parse_spine_json, spawn_rig};
use jengine_core::animation::skeletal::{update_skeletal_players, SkeletalPlayer};
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::{SkeletonRoot, Transform, WorldTransform};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use jengine_core::ecs::skeleton::find_bone;

const SPINE_DOC: &str = r#"{
    "skeleton": { "name": "hero" },
    "bones": [
        { "name": "root" },
        { "name": "arm", "parent": "root", "x": 0, "y": 0 }
    ],
    "animations": {
        "wave": {
            "bones": {
                "arm": {
                    "translate": [
                        { "time": 0, "x": 0, "y": 0 },
                        { "time": 1.0, "x": 10, "y": 0 }
                    ]
                }
            }
        }
    }
}"#;

#[test]
fn asset_manager_imports_spine_rig_and_clips() {
    let mut assets = AssetManager::new();
    let ids = assets
        .import_spine_skeleton("hero", "anim/", SPINE_DOC)
        .expect("import spine");
    assert_eq!(ids, vec!["anim/wave".to_string()]);
    assert!(assets.has_skeleton_rig("hero"));
    assert!(assets.has_skeletal_clip("anim/wave"));
}

#[test]
fn imported_rig_drives_world_transform_through_clip() {
    let mut assets = AssetManager::new();
    let ids = assets
        .import_spine_skeleton("hero", "", SPINE_DOC)
        .expect("import spine");
    assert_eq!(ids, vec!["wave".to_string()]);

    let rig = assets.get_skeleton_rig("hero").expect("rig").clone();
    let mut world = hecs::World::new();
    let bones = spawn_rig(&mut world, &rig);

    // Pin the root to a known world position and attach a player.
    let root = bones["root"];
    {
        let mut t = world.get::<&mut Transform>(root).unwrap();
        t.position.x = 100.0;
    }
    world
        .insert_one(
            root,
            SkeletalPlayer {
                clip: "wave".into(),
                time: 0.0,
                speed: 1.0,
                looping: false,
                playing: true,
                ..Default::default()
            },
        )
        .unwrap();

    // Sanity: the rig spawned a SkeletonRoot named after the rig.
    assert_eq!(world.get::<&SkeletonRoot>(root).unwrap().name, "hero");

    // Tick half the clip → arm local x ≈ 5.0 → world x ≈ 105.0.
    let clips: &HashMap<_, _> = assets.skeletal_clips();
    update_skeletal_players(&mut world, clips, 0.5);
    resolve_world_transforms(&mut world);

    let arm = find_bone(&world, root, "arm").expect("arm bone reachable");
    let arm_local = world.get::<&Transform>(arm).unwrap();
    assert!((arm_local.position.x - 5.0).abs() < 1e-4);
    let arm_world = world.get::<&WorldTransform>(arm).unwrap();
    assert!((arm_world.position.x - 105.0).abs() < 1e-4);
}

#[test]
fn parse_spine_json_directly_matches_asset_manager_path() {
    // Importing through the asset manager must produce the same data
    // as a direct call to parse_spine_json.
    let (rig_direct, clips_direct) = parse_spine_json(SPINE_DOC).expect("parse");
    let mut assets = AssetManager::new();
    assets
        .import_spine_skeleton("hero", "", SPINE_DOC)
        .expect("import");

    let rig_indirect = assets.get_skeleton_rig("hero").unwrap();
    assert_eq!(rig_direct.bones.len(), rig_indirect.bones.len());
    assert_eq!(rig_direct.name, rig_indirect.name);

    assert_eq!(clips_direct.len(), 1);
    let indirect = assets.get_skeletal_clip("wave").unwrap();
    assert_eq!(clips_direct[0].name, indirect.name);
    assert_eq!(clips_direct[0].duration, indirect.duration);
}
