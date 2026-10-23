//! 2D blend spaces (Oct 2026): clips placed on two axes and mixed by where a
//! position is, all at one phase - a walk and a run in eight directions about
//! an idle, steered by one `Animation.set_blend_position` a frame.

use std::collections::HashMap;

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::animation::skeletal::{
    BlendPoint, BlendSpace2D, BoneTrack, ScalarKey, SkeletalClip, SkeletalPlayer, blend_weights,
    update_skeletal_players,
};
use jengine_core::ecs::components::{Bone, Parent, SkeletonRoot, Transform};

/// A clip that holds bone `b` at `angle` for `duration` seconds.
fn hold(name: &str, angle: f32, duration: f32) -> SkeletalClip {
    SkeletalClip {
        name: name.into(),
        duration,
        tracks: vec![BoneTrack {
            bone_name: "b".into(),
            rotation: vec![ScalarKey { time: 0.0, value: angle }, ScalarKey { time: duration, value: angle }],
            ..Default::default()
        }],
    }
}

fn clips() -> HashMap<String, SkeletalClip> {
    [hold("idle", 0.0, 1.0), hold("east", 1.0, 1.0), hold("north", 0.5, 2.0), hold("west", -1.0, 1.0)]
        .into_iter()
        .map(|c| (c.name.clone(), c))
        .collect()
}

fn space(position: [f32; 2]) -> BlendSpace2D {
    let at = |clip: &str, x: f32, y: f32| BlendPoint { clip: clip.into(), at: [x, y] };
    BlendSpace2D {
        points: vec![at("idle", 0.0, 0.0), at("east", 1.0, 0.0), at("north", 0.0, 1.0), at("west", -1.0, 0.0)],
        position,
    }
}

fn rig(player: SkeletalPlayer) -> (World, Entity, Entity) {
    let mut world = World::new();
    let root = world.spawn((Transform::default(), SkeletonRoot { name: "r".into() }, player));
    let bone = world.spawn((Transform::default(), Bone { name: "b".into(), length: 10.0 }, Parent(root)));
    (world, root, bone)
}

fn angle(world: &World, bone: Entity) -> f32 {
    world.get::<&Transform>(bone).unwrap().rotation
}

#[test]
fn weights_are_whole_at_a_point_and_sum_to_one_everywhere() {
    let points = [Vec2::ZERO, Vec2::X, Vec2::Y, -Vec2::X, -Vec2::Y, Vec2::new(0.7, 0.7)];
    for (i, p) in points.iter().enumerate() {
        let w = blend_weights(&points, *p);
        assert!((w[i] - 1.0).abs() < 1e-5, "point {i} is all its own clip: {w:?}");
    }
    let mut last: Option<Vec<f32>> = None;
    for k in 0..=200 {
        // A walk across the space and out past its edge.
        let p = Vec2::new(-1.6 + k as f32 * 0.016, 0.3);
        let w = blend_weights(&points, p);
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-5, "sum at {p}: {w:?}");
        assert!(w.iter().all(|x| *x >= 0.0));
        if let Some(prev) = &last {
            let jump = w.iter().zip(prev).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            assert!(jump < 0.1, "the mix jumps at {p}: {jump}");
        }
        last = Some(w);
    }
    // Halfway from the idle to the east, the two share it.
    let w = blend_weights(&points[..4], Vec2::new(0.5, 0.0));
    assert!((w[0] - 0.5).abs() < 1e-5 && (w[1] - 0.5).abs() < 1e-5, "{w:?}");
}

#[test]
fn the_pose_follows_the_position() {
    let player = SkeletalPlayer { blend_space: Some(space([1.0, 0.0])), ..Default::default() };
    let (mut world, root, bone) = rig(player);
    let clips = clips();
    update_skeletal_players(&mut world, &clips, 0.01);
    assert!((angle(&world, bone) - 1.0).abs() < 1e-4, "on the east point: the east clip");
    world.get::<&mut SkeletalPlayer>(root).unwrap().set_blend_position(0.5, 0.0);
    update_skeletal_players(&mut world, &clips, 0.01);
    assert!((angle(&world, bone) - 0.5).abs() < 1e-4, "halfway to idle: {}", angle(&world, bone));
    world.get::<&mut SkeletalPlayer>(root).unwrap().set_blend_position(-1.0, 0.0);
    update_skeletal_players(&mut world, &clips, 0.01);
    assert!((angle(&world, bone) + 1.0).abs() < 1e-4);
}

#[test]
fn the_cycle_runs_at_the_pace_of_the_mix() {
    // North's clip is twice as long: on its point the phase moves half as fast.
    let player = SkeletalPlayer { blend_space: Some(space([0.0, 1.0])), ..Default::default() };
    let (mut world, root, _) = rig(player);
    let clips = clips();
    update_skeletal_players(&mut world, &clips, 0.25);
    assert!((world.get::<&SkeletalPlayer>(root).unwrap().time - 0.125).abs() < 1e-5, "phase = 0.25 s / 2 s");
    let player = SkeletalPlayer { blend_space: Some(space([0.0, 0.5])), ..Default::default() };
    let (mut world, root, _) = rig(player);
    update_skeletal_players(&mut world, &clips, 0.25);
    let t = world.get::<&SkeletalPlayer>(root).unwrap().time;
    assert!((t - 0.25 * 0.75).abs() < 1e-5, "between the idle's 1/s and north's 1/2 per s: {t}");
}

#[test]
fn a_clip_crossfades_out_of_the_space_and_back() {
    let player = SkeletalPlayer { blend_space: Some(space([1.0, 0.0])), ..Default::default() };
    let (mut world, root, bone) = rig(player);
    let clips = clips();
    update_skeletal_players(&mut world, &clips, 0.01);
    world.get::<&mut SkeletalPlayer>(root).unwrap().crossfade("west", 0.5);
    update_skeletal_players(&mut world, &clips, 0.25);
    let a = angle(&world, bone);
    assert!(a > -1.0 + 1e-3 && a < 1.0 - 1e-3, "halfway through the fade, between east and west: {a}");
    update_skeletal_players(&mut world, &clips, 0.5);
    assert!((angle(&world, bone) + 1.0).abs() < 1e-4, "the clip alone once the fade ends");
    // A crossfade to "" goes back to the space.
    world.get::<&mut SkeletalPlayer>(root).unwrap().crossfade("", 0.2);
    update_skeletal_players(&mut world, &clips, 0.3);
    assert!((angle(&world, bone) - 1.0).abs() < 1e-4, "back on the space's east point");
}

#[test]
fn set_blend_position_reaches_a_player_from_every_runtime() {
    use jengine_core::ecs::systems::apply_script_commands;
    use jengine_core::scripting::api::ScriptCommand;
    use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
    let (mut world, root, _) = rig(SkeletalPlayer { blend_space: Some(space([0.0, 0.0])), ..Default::default() });
    let id = root.id() as u64;
    apply_script_commands(&mut world, &[ScriptCommand::SetAnimationBlendPosition { entity_id: id, x: 0.25, y: -0.5 }], None, None);
    assert_eq!(world.get::<&SkeletalPlayer>(root).unwrap().blend_space.as_ref().unwrap().position, [0.25, -0.5]);
    struct Host(World);
    impl ScriptWorldHost for Host {
        fn world(&mut self) -> &mut World {
            &mut self.0
        }
        fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
            self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
        }
    }
    let mut host = Host(world);
    assert!(handle_world_command(&mut host, "sabp", &JsonArgs::new(serde_json::json!({ "e": id, "x": 1.0, "y": 2.0 }))));
    assert_eq!(host.0.get::<&SkeletalPlayer>(root).unwrap().blend_space.as_ref().unwrap().position, [1.0, 2.0]);
}

#[test]
fn a_blend_space_loads_from_a_scene() {
    let json = r#"{ "clip": "", "blend_space": { "points": [ { "clip": "idle", "at": [0, 0] }, { "clip": "east", "at": [1, 0] } ], "position": [0.5, 0] } }"#;
    let p: SkeletalPlayer = serde_json::from_str(json).unwrap();
    assert!(p.plays_space());
    let back = serde_json::to_value(&p).unwrap();
    assert_eq!(back["blend_space"]["points"][1]["clip"], "east");
}
