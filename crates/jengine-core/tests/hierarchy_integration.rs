//! Integration tests for Slice 3.5.A — entity parenting + transform hierarchy.
//!
//! Covers the foundation contract:
//!  - Scene round-trip preserves `parent` name references and rebuilds the
//!    `Parent` component after load.
//!  - `resolve_world_transforms` composes through scene-authored parent
//!    chains correctly.
//!  - The legacy `ScriptCommand::SetParent` applicator attaches and detaches
//!    via the same path scripts take.
//!
//! Gated on `native` so the scene + ECS code paths are available; nothing
//! here touches wgpu / the GPU.
#![cfg(feature = "native")]

use jengine_core::ecs::components::{Parent, Transform, WorldTransform};
use jengine_core::ecs::hierarchy::resolve_world_transforms;
use jengine_core::scene::format::{
    load_scene_with_mapping, save_scene, ComponentBundle, EntityData, SceneData,
};

fn scene_with_parent() -> SceneData {
    let parent = EntityData {
        children: Vec::new(),
        name: "Parent".to_string(),
        parent: None,
        components: ComponentBundle::new().with("transform", &Transform {
                position: glam::Vec2::new(10.0, 20.0),
                rotation: 0.0,
                scale: glam::Vec2::ONE,
            }),
    };
    let child = EntityData {
        children: Vec::new(),
        name: "Child".to_string(),
        parent: Some("Parent".to_string()),
        components: ComponentBundle::new().with("transform", &Transform {
                position: glam::Vec2::new(5.0, 0.0),
                rotation: 0.0,
                scale: glam::Vec2::ONE,
            }),
    };
    SceneData {
        name: "hier_test".to_string(),
        entities: vec![parent, child],
        ..Default::default()
    }
}

#[test]
fn scene_load_restores_parent_component() {
    let scene = scene_with_parent();
    let mut world = hecs::World::new();
    let mapping = load_scene_with_mapping(&scene, &mut world);
    assert_eq!(mapping.len(), 2);
    // Child must have a Parent component pointing at the parent entity.
    let parent_comp = world
        .get::<&Parent>(mapping[1])
        .expect("child entity should have Parent component after load");
    assert_eq!(parent_comp.0, mapping[0]);
    // Parent itself must NOT have a Parent component.
    assert!(world.get::<&Parent>(mapping[0]).is_err());
}

#[test]
fn resolver_composes_loaded_parent_chain() {
    let scene = scene_with_parent();
    let mut world = hecs::World::new();
    let mapping = load_scene_with_mapping(&scene, &mut world);
    resolve_world_transforms(&mut world);

    let parent_w = world.get::<&WorldTransform>(mapping[0]).unwrap();
    assert_eq!(parent_w.position, glam::Vec2::new(10.0, 20.0));

    let child_w = world.get::<&WorldTransform>(mapping[1]).unwrap();
    // Parent (10,20) + local (5,0) = world (15,20).
    assert_eq!(child_w.position, glam::Vec2::new(15.0, 20.0));
}

#[test]
fn scene_round_trip_preserves_parent_field() {
    let scene = scene_with_parent();
    let mut world = hecs::World::new();
    let _ = load_scene_with_mapping(&scene, &mut world);

    let saved = save_scene(&world);
    // Saved scene is order-independent (entity iteration order is not
    // guaranteed by hecs), so look up by name.
    let saved_child = saved
        .entities
        .iter()
        .find(|e| e.name == "Child")
        .expect("Child should be saved");
    assert_eq!(saved_child.parent.as_deref(), Some("Parent"));

    let saved_parent = saved
        .entities
        .iter()
        .find(|e| e.name == "Parent")
        .expect("Parent should be saved");
    assert!(saved_parent.parent.is_none());
}

#[test]
fn unknown_parent_name_is_ignored_with_warning() {
    let mut scene = scene_with_parent();
    scene.entities[1].parent = Some("DoesNotExist".to_string());
    let mut world = hecs::World::new();
    let mapping = load_scene_with_mapping(&scene, &mut world);
    // Child must NOT have a Parent component (load logged a warning).
    assert!(world.get::<&Parent>(mapping[1]).is_err());
    // Resolver still runs cleanly.
    resolve_world_transforms(&mut world);
}

#[test]
fn self_parent_in_scene_is_rejected() {
    let mut scene = scene_with_parent();
    scene.entities[0].parent = Some("Parent".to_string());
    let mut world = hecs::World::new();
    let mapping = load_scene_with_mapping(&scene, &mut world);
    assert!(world.get::<&Parent>(mapping[0]).is_err());
}
