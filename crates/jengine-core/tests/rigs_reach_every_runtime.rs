//! **`Skeleton.spawn_rig` spawns a rig, in every runtime** (Oct 2026).
//!
//! It bound in all four runtimes' worth of registration and spawned nothing
//! in any of them: native had the command arm and the browsers had nothing,
//! but no runtime loaded a `.rig` file, so native warned "rig not found" on
//! every call. Rigs are delivered the way skeletal clips are now - native
//! walks `assets/`, the web build reads them from the builder's
//! `skelanim_manifest.json`, the editor pushes each one - and named by their
//! path there (`rigs/arm.rig`). The browser half only runs in a browser, so
//! it is checked by source; native is run.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::{
    Bone, CustomComponents, Name, Parent, ScriptComponent, ScriptLanguage, SkeletonRoot, Transform,
};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;

fn read(rel: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
}

const ARM: &str = r#"{
    "name": "spawned_arm",
    "bones": [
        { "name": "upper", "position": [10.0, 20.0], "length": 40.0 },
        { "name": "lower", "parent": "upper", "position": [40.0, 0.0], "length": 30.0 },
        { "name": "hand", "parent": "lower", "position": [30.0, 0.0] }
    ]
}"#;

#[test]
fn native_loads_rigs_and_a_script_spawns_one() {
    let dir = std::env::temp_dir().join(format!("jengine_rigs_{}", std::process::id()));
    let assets_dir = dir.join("assets");
    std::fs::create_dir_all(assets_dir.join("rigs/deep")).unwrap();
    std::fs::write(assets_dir.join("rigs/deep/arm.rig"), ARM).unwrap();
    std::fs::write(assets_dir.join("rigs/broken.rig"), "not json").unwrap();
    let mut assets = AssetManager::new();
    assert_eq!(assets.load_skeleton_rigs_from_dir(&assets_dir), 1, "the broken rig is skipped, the good one loads");
    assert!(assets.has_skeleton_rig("rigs/deep/arm.rig"), "named by its path under assets/");

    let script = dir.join("spawner.lua");
    std::fs::write(
        &script,
        r#"
local spawned = nil
function update(entity, dt)
    if spawned == nil then
        spawned = Skeleton.spawn_rig("rigs/deep/arm.rig")
        Entity.set_custom(entity, "Probe", "handle", spawned)
    end
end
"#,
    )
    .unwrap();
    let mut world = World::new();
    world.spawn((
        Name("Spawner".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: script.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..2 {
        system.run(&mut world, 1.0 / 60.0, &input, None, Some(&mut assets));
    }

    let roots: Vec<_> = world.query::<&SkeletonRoot>().iter().map(|(e, r)| (e, r.name.clone())).collect();
    assert_eq!(roots.len(), 1, "one rig spawned: {roots:?}");
    assert_eq!(roots[0].1, "spawned_arm");
    let bones: Vec<String> = world.query::<&Bone>().iter().map(|(_, b)| b.name.clone()).collect();
    assert_eq!(bones.len(), 3, "every bone an entity: {bones:?}");
    let parented = world.query::<(&Bone, &Parent)>().iter().count();
    assert_eq!(parented, 2, "lower and hand hang from their parents");
    let upper = world.query::<(&Bone, &Transform)>().iter().find(|(_, (b, _))| b.name == "upper").map(|(_, (_, t))| t.position);
    assert_eq!(upper, Some(glam::Vec2::new(10.0, 20.0)), "in its bind pose");
}

#[test]
fn the_web_build_ships_rigs_and_loads_them() {
    let builder = read("crates/jengine-builder/src/web.rs");
    assert!(builder.contains("\"rigs\": inline_json(assets, \"rig\")"), "the manifest carries no rigs");
    let web = read("crates/jengine-web/src/lib.rs");
    assert!(web.contains("manifest.rigs") && web.contains("insert_skeleton_rig"), "the web runtime reads no rigs");
}

#[test]
fn the_editor_pushes_every_rig_into_the_viewport() {
    let svelte = read("editor/src/lib/Viewport.svelte");
    assert!(svelte.contains("viewport_load_skeleton_rig") && svelte.contains("f.endsWith(\".rig\")"));
    assert!(read("crates/jengine-editor-viewport/src/lib.rs").contains("insert_skeleton_rig"));
}

#[test]
fn native_loads_every_rig_under_assets() {
    assert!(read("crates/jengine-runtime/src/wgpu_scene_runner.rs").contains("load_skeleton_rigs_from_dir"));
}

#[test]
fn the_browsers_spawn_through_one_arm() {
    let preamble = read("crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs");
    assert!(preamble.contains("function spawn_rig(rig_id)") && preamble.contains("t: 'srig'"));
    let dispatch = read("crates/jengine-core/src/scripting/bridge/adapters/js/host_dispatch.rs");
    let arm = dispatch.find("\"srig\" =>").expect("the shared browser arm");
    let body: String = dispatch[arm..].chars().take(900).collect();
    assert!(body.contains("spawn_rig_root(") && body.contains("note_spawn"), "the arm spawns and records the handle");
}
