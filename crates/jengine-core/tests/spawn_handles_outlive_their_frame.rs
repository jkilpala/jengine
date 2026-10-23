//! A handle `spawn_entity` / `spawn_prefab` returned keeps naming its entity
//! in later frames, on native.
//!
//! The native command apply mapped the bridge's spawn id to the real entity
//! for the rest of one drain only, so a kept handle - the arcade's score
//! digits are 90 spawned cells, updated whenever the score changes - pointed
//! at nothing from the next frame on. `set_position` / `set_sprite_color`
//! were dropped and `get_position` answered nil with no error, while the
//! editor, whose `SCRIPT_ENTITY_MAP` persists, worked (BUGS #105). Found as
//! a space shooter's score that stayed 0 natively.
#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::Transform;
use jengine_core::ecs::systems::apply_script_commands;
use jengine_core::scripting::api::ScriptCommand;
use jengine_core::scripting::bridge::snapshot::snapshot_world_borrow;
use jengine_core::scripting::spawn_aliases::{self, FIRST_SPAWN_ID};

fn handle(n: u64) -> u64 {
    FIRST_SPAWN_ID + 700 + n
}

#[test]
fn a_spawn_handle_is_written_and_read_in_later_frames() {
    spawn_aliases::clear();
    let mut world = World::new();
    // Frame 1: spawn, and place it in the same drain (that always worked).
    apply_script_commands(
        &mut world,
        &[
            ScriptCommand::SpawnEntity { name: "Cell".into(), components: Box::new(None), spawn_id: Some(handle(1)), children: Vec::new() },
            ScriptCommand::SetPosition { entity_id: handle(1), x: 5.0, y: 6.0 },
        ],
        None,
        None,
    );
    // Frame 2: the script still holds the handle.
    apply_script_commands(&mut world, &[ScriptCommand::SetPosition { entity_id: handle(1), x: 40.0, y: -2.0 }], None, None);
    let p = world.query::<&Transform>().iter().next().map(|(_, t)| t.position).unwrap();
    assert_eq!((p.x, p.y), (40.0, -2.0), "the second frame's write reached the entity");

    // And reads resolve it.
    let snap = snapshot_world_borrow(&world);
    assert_eq!(snap.get_position(handle(1)), Some((40.0, -2.0)));
}

#[test]
fn a_handle_to_a_despawned_entity_names_nothing() {
    spawn_aliases::clear();
    let mut world = World::new();
    apply_script_commands(
        &mut world,
        &[ScriptCommand::SpawnEntity { name: "Gone".into(), components: Box::new(None), spawn_id: Some(handle(2)), children: Vec::new() }],
        None,
        None,
    );
    let e = world.iter().next().unwrap().entity();
    world.despawn(e).unwrap();
    // A new entity may take the old one's slot; the stale handle must not
    // reach it.
    let other = world.spawn((Transform::default(),));
    apply_script_commands(&mut world, &[ScriptCommand::SetPosition { entity_id: handle(2), x: 9.0, y: 9.0 }], None, None);
    assert_eq!(world.get::<&Transform>(other).unwrap().position.x, 0.0);
    assert_eq!(snapshot_world_borrow(&world).get_position(handle(2)), None);
}
