//! `Unlit` must survive every way a scene reaches the world.
//!
//! A marker component with no data is the easiest kind to drop silently: a
//! v1 `ComponentBundle` without the field, a save path that forgets to
//! extract it, or a v2 registry that does not know the key all leave the
//! entity loading fine - just lit again, which in scene 28 means a caption
//! too dark to read, the bug the component exists to fix.

use jengine_core::ecs::components::{Name, Unlit};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::scene::format::save_scene_v2;
use jengine_core::scene::load_scene_auto;

fn scene_28() -> String {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../projects/engine-test-lab/assets/scenes/28-spotlights.scene");
    std::fs::read_to_string(p).unwrap()
}

fn unlit_names(world: &hecs::World) -> Vec<String> {
    let mut v: Vec<String> = world
        .query::<(&Name, &Unlit)>()
        .iter()
        .map(|(_, (n, _))| n.0.clone())
        .collect();
    v.sort();
    v
}

#[test]
fn unlit_survives_a_v1_load_and_a_v2_round_trip() {
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);

    let mut world = hecs::World::new();
    load_scene_auto(&scene_28(), &mut world, &registry).unwrap();
    let loaded = unlit_names(&world);
    assert!(loaded.contains(&"Header".to_string()), "scene 28's title lost `unlit` on load: {loaded:?}");

    let saved = serde_json::to_string(&save_scene_v2(&world, &registry)).unwrap();
    let mut again = hecs::World::new();
    load_scene_auto(&saved, &mut again, &registry).unwrap();
    assert_eq!(unlit_names(&again), loaded, "`unlit` did not survive a v2 save and load");
}
