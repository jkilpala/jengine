//! **A skeleton plays layers, each on part of it** (Oct 2026). A
//! `skeletal_player`'s `layers` lay clips over the base pose in order, each
//! within its `mask` (a bone and everything below it), overriding by its
//! weight or adding its motion; `Animation.play_layer` / `stop_layer` /
//! `set_layer_weight` fade them, and a one-shot layer fades itself out when
//! its clip ends and calls the root's `on_layer_end`. Before this a skeleton
//! played one pose: an archer could walk or draw, not both. Found by the
//! showcase's Archer's Range.
//!
//! `animation::skeletal`'s own tests cover masks, fades and additive layers;
//! this drives the script surface natively, the shared browser arm, a layer
//! loaded from a scene, and checks every runtime wires the three functions.

use hecs::{Entity, World};
use jengine_core::animation::skeletal::{
    AnimLayer, BoneTrack, LayerMode, ScalarKey, SkeletalClip, SkeletalPlayer, update_skeletal_players,
};
use jengine_core::ecs::components::{Bone, Parent, SkeletonRoot, Transform};
use std::collections::HashMap;

fn read(p: &str) -> String {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(p)).unwrap().replace("\r\n", "\n")
}

/// Hips with a leg, and a spine with an arm on it.
fn archer(player: SkeletalPlayer) -> (World, Entity) {
    let mut w = World::new();
    let root = w.spawn((Transform::default(), SkeletonRoot { name: "archer".into() }, player));
    let hips = w.spawn((Transform::default(), Parent(root), Bone { name: "hips".into(), length: 1.0 }));
    w.spawn((Transform::default(), Parent(hips), Bone { name: "leg".into(), length: 1.0 }));
    let spine = w.spawn((Transform::default(), Parent(hips), Bone { name: "spine".into(), length: 1.0 }));
    w.spawn((Transform::default(), Parent(spine), Bone { name: "arm".into(), length: 1.0 }));
    (w, root)
}

fn pose(name: &str, duration: f32, angle: f32) -> SkeletalClip {
    SkeletalClip {
        name: name.into(),
        duration,
        tracks: ["hips", "leg", "spine", "arm"]
            .iter()
            .map(|b| BoneTrack {
                bone_name: b.to_string(),
                position: vec![],
                rotation: vec![ScalarKey { time: 0.0, value: angle }],
                scale: vec![],
            })
            .collect(),
    }
}

fn clips() -> HashMap<String, SkeletalClip> {
    let mut m = HashMap::new();
    m.insert("walk".into(), pose("walk", 1.0, 0.2));
    m.insert("draw".into(), pose("draw", 0.5, 1.5));
    m
}

fn angle(world: &World, root: Entity, bone: &str) -> f32 {
    let e = jengine_core::ecs::skeleton::find_bone(world, root, bone).unwrap();
    world.get::<&Transform>(e).unwrap().rotation
}

fn upper() -> SkeletalPlayer {
    SkeletalPlayer {
        clip: "walk".into(),
        layers: vec![AnimLayer { name: "upper".into(), mask: vec!["spine".into()], weight: 0.0, ..Default::default() }],
        ..Default::default()
    }
}

#[cfg(feature = "native")]
#[test]
fn a_script_draws_the_bow_over_the_walk() {
    use jengine_core::ecs::components::ScriptComponent;
    use jengine_core::ecs::components::ScriptLanguage;
    use jengine_core::ecs::systems::{ScriptSystem, apply_script_commands};
    const SRC: &str = r#"
local frame = 0
function update(entity, dt)
    frame = frame + 1
    if frame == 1 then Animation.play_layer(entity, "upper", "draw", 0) end
    if frame == 3 then Animation.stop_layer(entity, "upper", 0) end
    if frame == 4 then Animation.set_layer_weight(entity, "upper", 0.5) end
end
"#;
    let dir = std::env::temp_dir().join(format!("jengine_layers_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("archer.lua");
    std::fs::write(&path, SRC).unwrap();
    let (mut world, root) = archer(upper());
    world
        .insert_one(root, ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None })
        .unwrap();
    let mut system = ScriptSystem::new().expect("script system");
    let input = jengine_core::input::InputState::new();
    let clips = clips();
    let mut frame = |world: &mut World| {
        system.run(world, 0.1, &input, None, None);
        let cmds = std::mem::take(&mut system.api.commands);
        apply_script_commands(world, &cmds, None, None);
        update_skeletal_players(world, &clips, 0.1);
    };
    frame(&mut world);
    assert_eq!(angle(&world, root, "arm"), 1.5, "the arm draws");
    assert_eq!(angle(&world, root, "spine"), 1.5, "from the spine up");
    assert_eq!(angle(&world, root, "leg"), 0.2, "while the legs walk");
    frame(&mut world);
    frame(&mut world);
    assert_eq!(angle(&world, root, "arm"), 0.2, "stop_layer gave the arm back to the walk");
    frame(&mut world);
    assert!((angle(&world, root, "arm") - 0.85).abs() < 1e-4, "half of each: {}", angle(&world, root, "arm"));
}

#[test]
fn the_browsers_shared_arm_plays_a_layer() {
    use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
    struct Host(World);
    impl ScriptWorldHost for Host {
        fn world(&mut self) -> &mut World {
            &mut self.0
        }
        fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
            self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
        }
    }
    let (world, root) = archer(upper());
    let mut host = Host(world);
    let args = serde_json::json!({ "e": root.id(), "op": "play", "l": "upper", "c": "draw", "v": 1, "s": 0.4, "lp": true });
    assert!(handle_world_command(&mut host, "anly", &JsonArgs::new(args)));
    update_skeletal_players(&mut host.0, &clips(), 0.2);
    let p = host.0.get::<&SkeletalPlayer>(root).unwrap();
    assert_eq!((p.layers[0].clip.as_str(), p.layers[0].looping), ("draw", true));
    assert!((p.layers[0].weight - 0.5).abs() < 1e-4, "half way through a 0.4 s fade");
}

#[test]
fn layers_load_from_a_scene() {
    let json = r#"{ "clip": "walk", "layers": [
        { "name": "upper", "clip": "aim", "mask": ["spine"] },
        { "name": "breath", "clip": "breathe", "mode": "additive", "weight": 0.4 } ] }"#;
    let p: SkeletalPlayer = serde_json::from_str(json).unwrap();
    assert_eq!(p.layers[0].mask, vec!["spine".to_string()]);
    assert_eq!((p.layers[0].weight, p.layers[0].looping), (1.0, true), "a layer authored in a scene is on and loops");
    assert_eq!(p.layers[1].mode, LayerMode::Additive);
    let back = serde_json::to_value(&p).unwrap();
    assert_eq!(back["layers"][1]["mode"], "additive");
}

#[test]
fn every_runtime_wires_the_layer_functions() {
    let preamble = read("crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs");
    let luau = read("crates/jengine-core/src/scripting/bridge/adapters/luau.rs");
    let shared = read("crates/jengine-core/src/scripting/host_commands.rs");
    for (flat, op) in [("animation_play_layer", "play"), ("animation_stop_layer", "stop"), ("animation_set_layer_weight", "weight")] {
        let at = preamble.find(&format!("function {flat}(")).unwrap_or_else(|| panic!("no preamble {flat}"));
        let body = &preamble[at..at + preamble[at..].find("}}\n").unwrap()];
        assert!(body.contains("t: 'anly'") && body.contains(&format!("op: '{op}'")), "{flat} does not send anly/{op}");
        assert!(luau.contains(&format!("globals.set(\"{flat}\", f)")), "{flat} is not bound natively");
    }
    assert!(shared.contains("\"anly\" =>") && shared.contains("\"anly\","), "the shared browser arm is missing");
    assert!(read("crates/jengine-core/src/ecs/systems.rs").contains("player.layer_op("), "native does not apply layers");
}
