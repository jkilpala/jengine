//! **A script can play a skeletal clip** (Sep 2026). `play_animation`,
//! `stop_animation` and `set_animation_speed` reached only the sprite
//! animator in every runtime - native bound skeletal versions and then bound
//! the sprite ones over them, and the browser hosts had only the sprite
//! ones - so a `SkeletalPlayer` played its authored clip and nothing a
//! script asked for. They now drive whichever the entity has, and a player
//! can crossfade and blend (`crossfade_animation`, `set_animation_blend`),
//! and a two-bone IK solve has a weight (`set_ik_weight`).
//!
//! Found by the showcase's The Creature, whose walk turns into a run.

use hecs::World;
use jengine_core::animation::skeletal::SkeletalPlayer;
use jengine_core::animation::SpriteAnimator;
use jengine_core::ecs::components::IkTwoBone;
use jengine_core::ecs::systems::apply_script_commands;
use jengine_core::scripting::api::ScriptCommand;

const LUAU: &str = include_str!("../src/scripting/bridge/adapters/luau.rs");
const QUICKJS: &str = include_str!("../src/scripting/bridge/adapters/js/host.rs");

fn world_with_both() -> (World, u64) {
    let mut world = World::new();
    let e = world.spawn((
        SkeletalPlayer { clip: "idle".into(), ..Default::default() },
        SpriteAnimator::default(),
        IkTwoBone::default(),
    ));
    (world, e.id() as u64)
}

#[test]
fn native_animation_commands_reach_a_skeletal_player() {
    let (mut world, id) = world_with_both();
    let e = world.iter().next().unwrap().entity();
    apply_script_commands(
        &mut world,
        &[
            ScriptCommand::PlayAnimation { entity_id: id, clip_name: "walk".into() },
            ScriptCommand::SetAnimationSpeed { entity_id: id, speed: 1.5 },
            ScriptCommand::SetAnimationBlend { entity_id: id, clip: "run".into(), weight: 0.25 },
            ScriptCommand::SetIkWeight { entity_id: id, weight: 0.5 },
        ],
        None,
        None,
    );
    {
        let p = world.get::<&SkeletalPlayer>(e).unwrap();
        assert_eq!((p.clip.as_str(), p.speed, p.playing), ("walk", 1.5, true));
        assert_eq!((p.blend_clip.as_str(), p.blend), ("run", 0.25));
        assert_eq!(world.get::<&SpriteAnimator>(e).unwrap().speed, 1.5, "the sprite animator still hears it");
        assert_eq!(world.get::<&IkTwoBone>(e).unwrap().weight, 0.5);
    }
    apply_script_commands(
        &mut world,
        &[ScriptCommand::CrossfadeAnimation { entity_id: id, clip: "run".into(), seconds: 0.4 }],
        None,
        None,
    );
    {
        let p = world.get::<&SkeletalPlayer>(e).unwrap();
        assert_eq!(p.clip, "run");
        assert_eq!(p.fade.as_ref().map(|f| (f.clip.as_str(), f.duration)), Some(("walk", 0.4)));
    }
    apply_script_commands(&mut world, &[ScriptCommand::StopAnimation { entity_id: id }], None, None);
    assert!(!world.get::<&SkeletalPlayer>(e).unwrap().playing);
}

/// The bug was two bindings of one name, the second silently winning.
#[test]
fn each_animation_function_is_bound_once_natively() {
    for name in ["play_animation", "stop_animation", "set_animation_speed", "set_animation_time"] {
        let luau = LUAU.matches(&format!("globals.set(\"{name}\"")).count();
        assert_eq!(luau, 1, "Luau binds `{name}` {luau} times - the last one wins, in silence");
        let js = QUICKJS.matches(&format!("\"{name}\",")).count();
        assert_eq!(js, 1, "QuickJS binds `{name}` {js} times");
    }
}
