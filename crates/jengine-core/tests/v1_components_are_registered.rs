//! Every component a v1 scene can hold is registered, so a v2 scene can hold
//! it too - and so the editor, which snapshots the world as v2 before Play
//! and restores the snapshot on Stop, gives it back.
//!
//! The v1 `ComponentBundle` and the `ComponentRegistry` used to be two lists
//! of the same components, and they drifted both ways: the whole skeleton set
//! (`bone`, `skeleton_root`, both IK constraints, `skinned_mesh`,
//! `skeletal_player`), `anim_state_machine` and `light_occluder` were only
//! ever in the bundle - so a package game could not author a rig, and scene
//! 34's arm lost its bones the first time the editor's Stop button was pressed
//! (BUGS #151) - while `tilemap_collider`, `character_controller` and
//! `bulk_entity` were only in the registry, and a v1 scene dropped them
//! (BUGS #164). Two tests compared the lists.
//!
//! Since Oct 2026 there is one list. A v1 bundle is JSON by scene key, read
//! through the registry like a v2 one, so the two cannot drift;
//! `spawned_prefabs_keep_their_components` loads and saves every registered
//! component through the v1 path to keep it that way. What is left here is
//! the round trip the drift broke.

use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;

#[test]
fn a_rig_survives_the_editors_play_snapshot() {
    use jengine_core::ecs::components::{Bone, IkTwoBone, SkeletonRoot};
    use jengine_core::scene::format::{load_scene_v2, snapshot_scene_v2};
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../projects/engine-test-lab/assets/scenes/34-skeleton-ik.scene"
    ))
    .unwrap();
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let mut world = hecs::World::new();
    jengine_core::scene::load_scene_auto(&text, &mut world, &registry).unwrap();
    let count = |w: &hecs::World| {
        (
            w.query::<&Bone>().iter().count(),
            w.query::<&SkeletonRoot>().iter().count(),
            w.query::<&IkTwoBone>().iter().count(),
            w.query::<&jengine_core::animation::skinned_mesh::SkinnedMesh>().iter().count(),
        )
    };
    let before = count(&world);
    assert!(before.0 >= 3 && before.1 >= 1 && before.2 >= 1 && before.3 >= 1, "{before:?}");
    let snap = serde_json::to_string(&snapshot_scene_v2(&world, &registry)).unwrap();
    let mut restored = hecs::World::new();
    load_scene_v2(&serde_json::from_str(&snap).unwrap(), &mut restored, &registry);
    assert_eq!(count(&restored), before, "Stop stripped the rig");
}
