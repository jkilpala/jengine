//! Integration tests for Slice 3.5.C — skeletal animation playback.
//!
//! Covers:
//!  - Scene format round-trips `SkeletalPlayer` with its bookkeeping
//!    fields preserved.
//!  - End-to-end pipeline: load scene → register clip → tick →
//!    `resolve_world_transforms` produces the expected world-space
//!    pose for an animated bone.
#![cfg(feature = "native")]

use std::collections::HashMap;

use jengine_core::animation::skeletal::{
    update_skeletal_players, BoneTrack, SkeletalClip, SkeletalPlayer, Vec2Key,
};
use jengine_core::ecs::components::{Bone, SkeletonRoot, Transform, WorldTransform};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use jengine_core::scene::format::{
    load_scene_with_mapping, save_scene, ComponentBundle, EntityData, SceneData,
};

fn rig_scene_with_player() -> SceneData {
    let root = EntityData {
        children: Vec::new(),
        name: "RigRoot".to_string(),
        parent: None,
        components: ComponentBundle::new().with("transform", &Transform {
                position: glam::Vec2::new(100.0, 0.0),
                rotation: 0.0,
                scale: glam::Vec2::ONE,
            }).with("skeleton_root", &SkeletonRoot { name: "rig".into() }).with("skeletal_player", &SkeletalPlayer {
                clip: "wave".into(),
                time: 0.0,
                speed: 2.0,
                looping: false,
                playing: true,
                ..Default::default()
            }),
    };
    let arm = EntityData {
        children: Vec::new(),
        name: "Arm".to_string(),
        parent: Some("RigRoot".to_string()),
        components: ComponentBundle::new().with("transform", &Transform::default()).with("bone", &Bone { name: "arm".into(), length: 1.0 }),
    };
    SceneData {
        name: "skelanim_test".to_string(),
        entities: vec![root, arm],
        ..Default::default()
    }
}

#[test]
fn scene_round_trip_preserves_skeletal_player_fields() {
    let scene = rig_scene_with_player();
    let mut world = hecs::World::new();
    let mapping = load_scene_with_mapping(&scene, &mut world);

    let player = world
        .get::<&SkeletalPlayer>(mapping[0])
        .expect("RigRoot should have SkeletalPlayer after load");
    assert_eq!(player.clip, "wave");
    assert_eq!(player.speed, 2.0);
    assert!(!player.looping);
    assert!(player.playing);

    // Save → expect the same shape back.
    let saved = save_scene(&world);
    let saved_root = saved
        .entities
        .iter()
        .find(|e| e.name == "RigRoot")
        .expect("RigRoot should be saved");
    let saved_player = saved_root
        .components.get::<jengine_core::animation::skeletal::SkeletalPlayer>("skeletal_player")
        .expect("SkeletalPlayer should be saved");
    assert_eq!(saved_player.clip, "wave");
    assert_eq!(saved_player.speed, 2.0);
    assert!(!saved_player.looping);
}

#[test]
fn end_to_end_animation_drives_world_transform() {
    let scene = rig_scene_with_player();
    let mut world = hecs::World::new();
    let mapping = load_scene_with_mapping(&scene, &mut world);

    let mut clips: HashMap<String, SkeletalClip> = HashMap::new();
    clips.insert(
        "wave".to_string(),
        SkeletalClip {
            name: "wave".into(),
            duration: 1.0,
            tracks: vec![BoneTrack {
                bone_name: "arm".into(),
                position: vec![
                    Vec2Key { time: 0.0, value: [0.0, 0.0] },
                    Vec2Key { time: 1.0, value: [10.0, 0.0] },
                ],
                rotation: vec![],
                scale: vec![],
            }],
        },
    );

    // Advance 0.25s at speed 2.0 → effective time 0.5 → position lerp = 5.0.
    update_skeletal_players(&mut world, &clips, 0.25);
    resolve_world_transforms(&mut world);

    let arm = mapping[1];
    let arm_local = world.get::<&Transform>(arm).unwrap();
    assert!((arm_local.position.x - 5.0).abs() < 1e-4);

    // World-space pose includes the root translation (100, 0).
    let arm_world = world.get::<&WorldTransform>(arm).unwrap();
    assert!((arm_world.position.x - 105.0).abs() < 1e-4);
    assert_eq!(arm_world.position.y, 0.0);

    // Non-looping player advanced past midway but not duration: still playing.
    let player = world.get::<&SkeletalPlayer>(mapping[0]).unwrap();
    assert!(player.playing);
    assert!((player.time - 0.5).abs() < 1e-4);
}
