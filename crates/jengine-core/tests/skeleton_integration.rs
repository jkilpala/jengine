//! Integration tests for Slice 3.5.B — Bone + SkeletonRoot foundation.
//!
//! Covers:
//!  - Scene format round-trips `Bone` and `SkeletonRoot` components.
//!  - After load, `find_bone` resolves bone names within a parented
//!    skeleton hierarchy.
//!  - Bone `length` is preserved verbatim.
#![cfg(feature = "native")]

use jengine_core::ecs::components::{Bone, Parent, SkeletonRoot, Transform};
use jengine_core::ecs::skeleton::{collect_bones, find_bone, find_skeleton};
use jengine_core::scene::format::{
    load_scene_with_mapping, save_scene, ComponentBundle, EntityData, SceneData,
};

fn skeleton_scene() -> SceneData {
    // hero (root) -> pelvis -> spine -> head
    //                              \--> arm
    let root = EntityData {
        children: Vec::new(),
        name: "HeroRoot".to_string(),
        parent: None,
        components: ComponentBundle::new().with("transform", &Transform::default()).with("skeleton_root", &SkeletonRoot { name: "hero".into() }),
    };
    let pelvis = EntityData {
        children: Vec::new(),
        name: "Pelvis".to_string(),
        parent: Some("HeroRoot".to_string()),
        components: ComponentBundle::new().with("transform", &Transform::default()).with("bone", &Bone { name: "pelvis".into(), length: 1.0 }),
    };
    let spine = EntityData {
        children: Vec::new(),
        name: "Spine".to_string(),
        parent: Some("Pelvis".to_string()),
        components: ComponentBundle::new().with("transform", &Transform::default()).with("bone", &Bone { name: "spine".into(), length: 1.5 }),
    };
    let head = EntityData {
        children: Vec::new(),
        name: "Head".to_string(),
        parent: Some("Spine".to_string()),
        components: ComponentBundle::new().with("transform", &Transform::default()).with("bone", &Bone { name: "head".into(), length: 0.5 }),
    };
    let arm = EntityData {
        children: Vec::new(),
        name: "Arm".to_string(),
        parent: Some("Spine".to_string()),
        components: ComponentBundle::new().with("transform", &Transform::default()).with("bone", &Bone { name: "arm".into(), length: 2.0 }),
    };
    SceneData {
        name: "skeleton_test".to_string(),
        entities: vec![root, pelvis, spine, head, arm],
        ..Default::default()
    }
}

#[test]
fn scene_round_trip_preserves_bones_and_root() {
    let scene = skeleton_scene();
    let mut world = hecs::World::new();
    let mapping = load_scene_with_mapping(&scene, &mut world);
    assert_eq!(mapping.len(), 5);

    // Root carries SkeletonRoot.
    let root_marker = world
        .get::<&SkeletonRoot>(mapping[0])
        .expect("HeroRoot should have SkeletonRoot after load");
    assert_eq!(root_marker.name, "hero");

    // Bones round-trip with their length values.
    let spine_bone = world
        .get::<&Bone>(mapping[2])
        .expect("Spine should have Bone after load");
    assert_eq!(spine_bone.name, "spine");
    assert_eq!(spine_bone.length, 1.5);

    // Save back and confirm fields survive.
    let saved = save_scene(&world);
    let saved_head = saved
        .entities
        .iter()
        .find(|e| e.name == "Head")
        .expect("Head should be saved");
    let head_bone = saved_head
        .components.get::<jengine_core::ecs::components::Bone>("bone")
        .expect("Head should have bone in saved scene");
    assert_eq!(head_bone.name, "head");
    assert_eq!(head_bone.length, 0.5);
    let saved_root = saved
        .entities
        .iter()
        .find(|e| e.name == "HeroRoot")
        .expect("HeroRoot should be saved");
    let root_save = saved_root
        .components.get::<jengine_core::ecs::components::SkeletonRoot>("skeleton_root")
        .expect("HeroRoot should have skeleton_root in saved scene");
    assert_eq!(root_save.name, "hero");
}

#[test]
fn find_bone_after_scene_load_walks_parent_chain() {
    let scene = skeleton_scene();
    let mut world = hecs::World::new();
    let mapping = load_scene_with_mapping(&scene, &mut world);
    let root = mapping[0];

    // `head` is a grandchild via pelvis->spine->head; find_bone must
    // walk the Parent chain restored by the scene loader.
    let head = find_bone(&world, root, "head").expect("find_bone should locate 'head'");
    assert_eq!(head, mapping[3]);

    // Verify the chain itself: head's parent is spine, spine's parent
    // is pelvis, pelvis's parent is the root.
    let head_parent = world.get::<&Parent>(head).unwrap().0;
    assert_eq!(head_parent, mapping[2]);
    let spine_parent = world.get::<&Parent>(mapping[2]).unwrap().0;
    assert_eq!(spine_parent, mapping[1]);
    let pelvis_parent = world.get::<&Parent>(mapping[1]).unwrap().0;
    assert_eq!(pelvis_parent, root);

    assert_eq!(find_bone(&world, root, "missing"), None);
}

#[test]
fn find_skeleton_returns_named_root_after_load() {
    let scene = skeleton_scene();
    let mut world = hecs::World::new();
    let mapping = load_scene_with_mapping(&scene, &mut world);
    assert_eq!(find_skeleton(&world, "hero"), Some(mapping[0]));
    assert_eq!(find_skeleton(&world, "ghost"), None);
}

#[test]
fn collect_bones_after_load_returns_full_rig() {
    let scene = skeleton_scene();
    let mut world = hecs::World::new();
    let mapping = load_scene_with_mapping(&scene, &mut world);
    let mut names: Vec<String> = collect_bones(&world, mapping[0])
        .into_iter()
        .map(|(_, n)| n)
        .collect();
    names.sort();
    assert_eq!(names, vec!["arm", "head", "pelvis", "spine"]);
}
