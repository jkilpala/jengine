//! Script handles for entities a script spawned, kept for as long as the
//! entity lives - the native runtime's half of what the browser hosts keep
//! in `SCRIPT_ENTITY_MAP`.
//!
//! `spawn_entity` / `spawn_prefab` return an id the bridge allocates on the
//! spot (from [`FIRST_SPAWN_ID`] up), because the entity does not exist
//! until the frame's commands are applied. The native apply mapped that id
//! to the real entity only for the rest of that one drain, so a handle kept
//! in a script - a HUD built from spawned cells, a pool of bullets - pointed
//! at nothing from the next frame on: every `set_*` on it was dropped and
//! every `get_*` answered nil, silently, while the browser runtimes
//! honoured it (BUGS #105). Both the command apply and the read snapshot
//! consult this table now.
//!
//! One table per thread, like the rest of native script state. It is
//! cleared when a scene is replaced, and entries whose entity has gone are
//! dropped whenever it is read.
//!
//! It is read the other way too ([`handle_of`]): a raycast hit or a contact
//! names a spawned entity by the handle the script holds, so a script can
//! tell its own bomb from the ground it landed on. They used to report the
//! real id, which a script never sees for a spawned entity - found by the
//! showcase's Crater Isle, whose bombs exploded on themselves.

use hecs::{Entity, World};
use std::cell::RefCell;
use std::collections::HashMap;

/// The first id the bridge hands out for a spawn.
pub const FIRST_SPAWN_ID: u64 = 1_000_000;

thread_local! {
    static ALIASES: RefCell<HashMap<u64, Entity>> = RefCell::new(HashMap::new());
    /// The same table, entity id to handle.
    static HANDLES: RefCell<HashMap<u64, u64>> = RefCell::new(HashMap::new());
}

/// Remember that the script's `spawn_id` is `entity`.
pub fn record(spawn_id: u64, entity: Entity) {
    ALIASES.with(|a| a.borrow_mut().insert(spawn_id, entity));
    HANDLES.with(|h| h.borrow_mut().insert(entity.id() as u64, spawn_id));
}

/// Forget every handle - the scene they belonged to is gone.
pub fn clear() {
    ALIASES.with(|a| a.borrow_mut().clear());
    HANDLES.with(|h| h.borrow_mut().clear());
}

/// The handle a script holds for entity id `id`, if a script spawned it.
pub fn handle_of(id: u64) -> Option<u64> {
    HANDLES.with(|h| h.borrow().get(&id).copied())
}

/// The id a script knows entity `id` by: its spawn handle, or the id itself.
pub fn script_id(id: u64) -> u64 {
    handle_of(id).unwrap_or(id)
}

/// The entity behind one handle, if the handle is known.
pub fn resolve(spawn_id: u64) -> Option<Entity> {
    ALIASES.with(|a| a.borrow().get(&spawn_id).copied())
}

/// Every handle whose entity is still in `world`, dropping the rest.
pub fn live(world: &World) -> Vec<(u64, Entity)> {
    ALIASES.with(|a| {
        let mut a = a.borrow_mut();
        if a.is_empty() {
            return Vec::new();
        }
        a.retain(|_, e| world.contains(*e));
        HANDLES.with(|h| {
            let mut h = h.borrow_mut();
            h.clear();
            h.extend(a.iter().map(|(id, e)| (e.id() as u64, *id)));
        });
        a.iter().map(|(id, e)| (*id, *e)).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_lives_as_long_as_its_entity() {
        clear();
        let mut world = World::new();
        let a = world.spawn((1u8,));
        let b = world.spawn((2u8,));
        record(FIRST_SPAWN_ID, a);
        record(FIRST_SPAWN_ID + 1, b);
        world.despawn(a).unwrap();
        assert_eq!(live(&world), vec![(FIRST_SPAWN_ID + 1, b)]);
        clear();
        assert!(live(&world).is_empty());
    }
}
