//! A script command names an entity by id, and the engine resolves that id
//! through the frame's `id -> Entity` index - never by scanning the world.
//!
//! `apply_script_commands_impl` built that index and then, in 29 of its
//! arms, ignored it: `for (entity, c) in world.query_mut::<&mut T>() { if
//! entity.id() as u64 == real_id { ... } }`, one full query per command.
//! `tick_tweens` did the same per tween, and a colour tween walked the whole
//! world. None of it was wrong, which is why it lasted: it is `O(commands x
//! entities)` a frame, invisible at ten entities and the whole frame at ten
//! thousand.
//!
//! This scans the systems module for the comparison that shape needs. A new
//! arm written that way fails here with its line number.

use std::path::Path;

#[test]
fn no_arm_scans_the_world_for_an_entity_id() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ecs/systems.rs");
    let src = std::fs::read_to_string(&path).expect("read ecs/systems.rs");
    let offenders: Vec<String> = src
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let t = line.trim_start();
            !t.starts_with("//") && line.contains("id() as u64 == ")
        })
        .map(|(i, line)| format!("  {}:{}: {}", path.display(), i + 1, line.trim()))
        .collect();
    assert!(
        offenders.is_empty(),
        "entity ids are resolved through the frame's index (`entity_index.get(&id)` in the \
         command applier, `snapshot.entity_handles` in `ScriptSystem::run`), not by comparing \
         `entity.id()` against every entity in a query:\n{}",
        offenders.join("\n")
    );
}

/// The same for the two bridge accessors that used to copy: nothing in
/// `ScriptSystem::run` may fill the legacy `ScriptState` snapshot maps or
/// clone a tilemap into the bridge snapshot again.
#[test]
fn the_frame_does_not_deep_copy_the_world_into_a_snapshot() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ecs/systems.rs");
    let src = std::fs::read_to_string(&path).expect("read ecs/systems.rs");
    let banned = [
        "self.api.snapshot.snapshot_tilemaps(",
        "self.api.snapshot.snapshot_particles(",
        "self.api.snapshot.snapshot_animations(",
        "self.api.snapshot.snapshot_sprite_points(",
        "bridge::snapshot_tilemaps(",
        "self.api.snapshot.sprite_points.clone()",
    ];
    let offenders: Vec<String> = src
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let t = line.trim_start();
            !t.starts_with("//") && banned.iter().any(|b| line.contains(b))
        })
        .map(|(i, line)| format!("  {}:{}: {}", path.display(), i + 1, line.trim()))
        .collect();
    assert!(
        offenders.is_empty(),
        "the per-frame script tick reads tilemaps, emitters and animators live from the world \
         (through `world_ptr` / `entity_handles`); these calls deep-copied them every frame:\n{}",
        offenders.join("\n")
    );
}

/// The lent world pointer is dereferenced in **one** place per snapshot -
/// `world()` (and its `physics()` twin) - so the contract has one site to
/// audit. Fifty accessors each carried their own `unsafe` block and their own
/// copy of the SAFETY sentence; a new one written that way fails here.
#[test]
fn the_world_pointer_is_dereferenced_in_one_place() {
    for (rel, allowed) in [
        ("src/scripting/bridge/context.rs", 3usize),
        ("src/scripting/api.rs", 2usize),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
        let src = std::fs::read_to_string(&path).expect("read");
        let sites: Vec<usize> = src
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim_start().starts_with("//") && l.contains("unsafe {"))
            .map(|(i, _)| i + 1)
            .collect();
        assert!(
            sites.len() <= allowed,
            "{rel} has {} `unsafe` blocks at lines {sites:?}; the raw pointers are dereferenced only in \
             `world()` and `physics()` - read through those, do not deref `world_ptr` again",
            sites.len()
        );
    }
}
