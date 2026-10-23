//! Every component a v1 scene can hold is registered, so a v2 scene can hold
//! it too - and so the editor, which snapshots the world as v2 before Play
//! and restores the snapshot on Stop, gives it back.
//!
//! The v1 `ComponentBundle` and the `ComponentRegistry` are two lists of the
//! same components, and they drifted: the whole skeleton set (`bone`,
//! `skeleton_root`, both IK constraints, `skinned_mesh`, `skeletal_player`),
//! `anim_state_machine` and `light_occluder` were only ever in the bundle. So
//! a package game could not author a rig, and scene 34's arm lost its bones
//! the first time the editor's Stop button was pressed. BUGS #151.

use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;

/// `custom` is the bag of unknown keys, not a component.
const NOT_COMPONENTS: &[&str] = &["custom"];

fn bundle_fields() -> Vec<String> {
    // LF, whatever git checked out: CRLF on Windows found no "\n}\n".
    let src = include_str!("../src/scene/format.rs").replace("\r\n", "\n");
    let start = src.find("pub struct ComponentBundle").expect("ComponentBundle");
    let end = start + src[start..].find("\n}\n").expect("end of ComponentBundle");
    src[start..end]
        .lines()
        .filter_map(|l| l.trim().strip_prefix("pub "))
        .filter_map(|l| l.split(':').next())
        .filter(|name| !name.contains(' ') && !name.contains('('))
        .map(str::to_string)
        .collect()
}

#[test]
fn every_v1_component_is_in_the_registry() {
    let fields = bundle_fields();
    assert!(fields.len() > 60, "found {} fields - has ComponentBundle moved?", fields.len());
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let missing: Vec<&String> = fields
        .iter()
        .filter(|f| !NOT_COMPONENTS.contains(&f.as_str()) && registry.get(f).is_none())
        .collect();
    assert!(missing.is_empty(), "in the v1 bundle but not registered - a v2 scene drops them: {missing:?}");
}

/// The other direction. A component registered but missing from the bundle
/// is dropped by the v1 loader, which every runtime uses - silently, since
/// serde ignores an unknown key. `tilemap_collider` was one: test-lab scene
/// 94's map loaded with no collider and its crates fell out of the world
/// (BUGS #164), and `character_controller` and `bulk_entity` were the same.
#[test]
fn every_registered_component_is_in_the_v1_bundle() {
    let fields = bundle_fields();
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let missing: Vec<String> = registry
        .iter()
        .map(|d| d.name.clone())
        .filter(|n| !fields.contains(n))
        .collect();
    assert!(missing.is_empty(), "registered but not in the v1 bundle - a v1 scene drops them: {missing:?}");
}

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
