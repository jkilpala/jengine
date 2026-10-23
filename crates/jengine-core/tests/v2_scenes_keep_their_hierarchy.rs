//! A v2 scene keeps its hierarchy: in the file, and through a save.
//!
//! `EntityDataV2` had a name and a component map and nothing else, so a v2
//! scene could not say that one entity hangs from another, and
//! `save_scene_v2` dropped every `Parent` it was handed. Two things followed.
//! A package game - which must be v2, because a v1 scene deserializes into a
//! core-only bundle - could not author a skeleton, a turret on a hull, or
//! anything else built of parts. And the editor, which snapshots the world as
//! v2 before Play and restores that snapshot on Stop (and on a scene pop),
//! **flattened every hierarchy in the scene the first time Play was
//! pressed**: each child came back at its local position, in world space.
//!
//! Found by a walker game, whose strider is a rig of bones in a v2 scene.

use hecs::World;
use jengine_core::ecs::components::{Name, Parent};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::scene::format::{load_scene_v2, save_scene_v2, snapshot_scene_v2, SceneDataV2};
use jengine_core::scene::{load_scene_auto, SceneData};

fn registry() -> ComponentRegistry {
    let mut r = ComponentRegistry::new();
    register_core_components(&mut r);
    r
}

fn by_name(world: &World, name: &str) -> Vec<hecs::Entity> {
    world.query::<&Name>().iter().filter(|(_, n)| n.0 == name).map(|(e, _)| e).collect()
}

fn parent_name(world: &World, e: hecs::Entity) -> Option<String> {
    let p = world.get::<&Parent>(e).ok()?.0;
    world.get::<&Name>(p).ok().map(|n| n.0.clone())
}

const RIG: &str = r#"{
    "version": 2,
    "name": "rig",
    "entities": [
        { "name": "Hand", "parent": "Arm", "components": {
            "transform": { "position": [50.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] } } },
        { "name": "Body", "components": {
            "transform": { "position": [100.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] } } },
        { "name": "Arm", "parent": "Body", "components": {
            "transform": { "position": [10.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0] } } }
    ]
}"#;

#[test]
fn a_v2_scene_can_say_what_hangs_from_what() {
    let mut world = World::new();
    load_scene_auto(RIG, &mut world, &registry()).unwrap();
    let hand = by_name(&world, "Hand")[0];
    let arm = by_name(&world, "Arm")[0];
    // A forward reference resolves: `Hand` names `Arm` before `Arm` exists.
    assert_eq!(parent_name(&world, hand).as_deref(), Some("Arm"));
    assert_eq!(parent_name(&world, arm).as_deref(), Some("Body"));
    assert!(world.get::<&Parent>(by_name(&world, "Body")[0]).is_err());

    // And it is a hierarchy, not just a component: the hand is where its
    // parents put it.
    jengine_core::ecs::hierarchy::resolve_world_transforms(&mut world);
    let w = jengine_core::ecs::hierarchy::world_transform_of(&world, hand).unwrap();
    assert_eq!(w.position.x, 160.0);
}

#[test]
fn the_editors_v1_struct_carries_a_v2_parent_through() {
    // The editor loads every scene into `SceneData` and re-serializes it
    // into the viewport; the parent must survive that too.
    let loaded: SceneData = serde_json::from_str(RIG).unwrap();
    let json = serde_json::to_string(&loaded).unwrap();
    let mut world = World::new();
    load_scene_auto(&json, &mut world, &registry()).unwrap();
    assert_eq!(parent_name(&world, by_name(&world, "Hand")[0]).as_deref(), Some("Arm"));
}

#[test]
fn saving_and_loading_keeps_every_parent_even_between_namesakes() {
    // Two legs, each with a foot, and every leg and foot named alike - the
    // shape of any rig built from copies. A name cannot say which leg a
    // foot hangs from, so the save has to.
    let mut world = World::new();
    let body = world.spawn((Name("Body".into()),));
    let leg_a = world.spawn((Name("Leg".into()), Parent(body)));
    let leg_b = world.spawn((Name("Leg".into()), Parent(body)));
    let foot_a = world.spawn((Name("Foot".into()), Parent(leg_a), jengine_core::ecs::components::Transform::default()));
    let foot_b = world.spawn((Name("Foot".into()), Parent(leg_b)));
    let _ = (foot_a, foot_b);

    let registry = registry();
    for (label, data) in [("save", save_scene_v2(&world, &registry)), ("snapshot", snapshot_scene_v2(&world, &registry))] {
        let json = serde_json::to_string(&data).unwrap();
        let back: SceneDataV2 = serde_json::from_str(&json).unwrap();
        let mut again = World::new();
        load_scene_v2(&back, &mut again, &registry);

        let legs = by_name(&again, "Leg");
        let feet = by_name(&again, "Foot");
        assert_eq!((legs.len(), feet.len()), (2, 2), "{label}");
        for l in &legs {
            assert_eq!(parent_name(&again, *l).as_deref(), Some("Body"), "{label}: a leg lost its body");
        }
        // Each foot hangs from a *different* leg.
        let mut owners: Vec<hecs::Entity> = feet.iter().map(|f| again.get::<&Parent>(*f).expect("a foot lost its leg").0).collect();
        owners.sort_by_key(|e| e.id());
        owners.dedup();
        assert_eq!(owners.len(), 2, "{label}: both feet hang from one leg");
        // The foot that had a transform is still on the leg that had it.
        let with_tf = feet.iter().find(|f| again.get::<&jengine_core::ecs::components::Transform>(**f).is_ok()).unwrap();
        let without = feet.iter().find(|f| again.get::<&jengine_core::ecs::components::Transform>(**f).is_err()).unwrap();
        assert_ne!(again.get::<&Parent>(*with_tf).unwrap().0, again.get::<&Parent>(*without).unwrap().0);
    }
}

#[test]
fn a_v1_scene_with_a_hierarchy_survives_the_editors_play_snapshot() {
    // Test Lab scene 34's arm, as the editor would snapshot it before Play
    // and restore it on Stop.
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../projects/engine-test-lab/assets/scenes/34-skeleton-ik.scene"
    ))
    .unwrap();
    let registry = registry();
    let mut world = World::new();
    load_scene_auto(&text, &mut world, &registry).unwrap();
    let before = world.query::<&Parent>().iter().count();
    assert!(before >= 3, "scene 34 has a hierarchy to lose");

    let snap = serde_json::to_string(&snapshot_scene_v2(&world, &registry)).unwrap();
    let mut restored = World::new();
    load_scene_v2(&serde_json::from_str(&snap).unwrap(), &mut restored, &registry);
    assert_eq!(restored.query::<&Parent>().iter().count(), before, "Stop flattened the scene");
    let tip = by_name(&restored, "ArmTip")[0];
    assert_eq!(parent_name(&restored, tip).as_deref(), Some("ArmLower"));
}
