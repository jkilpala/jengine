//! Save states (Oct 2026): the running world written down under a name and
//! put back later exactly as it was - every entity and component, and every
//! body's pose, velocity and spin - the quicksave of an emulator, for a
//! puzzle's undo, a checkpoint or a replay's start.
//!
//! `State.save(slot)` / `State.load(slot)` only queue a request: the world and
//! the physics are both needed, and only the physics frame holds both, so
//! [`sync`] runs at the end of `physics_frame_end`, which every host calls - no
//! host was wired. A load puts each saved entity back **under its own handle**
//! (`hecs::World::spawn_at`), so a script's handles stay good; whatever was
//! spawned since is gone and whatever was destroyed since is back. What a
//! script keeps in its own variables is its own and is not rewound - keep
//! what must be in a custom component, which is. Runtime state a component
//! never saves (a trail's points, a particle's age) starts again. States live
//! in memory for the session.

use std::collections::HashMap;
use std::sync::Mutex;

use hecs::{Entity, World};

use crate::ecs::components::{CustomComponents, Name, Parent};


pub use crate::physics::BodyState;

#[derive(Debug, Clone, PartialEq)]
struct SavedEntity {
    handle: u64,
    name: Option<String>,
    parent: Option<u64>,
    components: HashMap<String, serde_json::Value>,
    custom: Option<HashMap<String, serde_json::Value>>,
}

/// A whole world, written down.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldState {
    entities: Vec<SavedEntity>,
    /// By physics id (`Entity::id`).
    bodies: HashMap<u64, BodyState>,
}

impl WorldState {
    /// How many entities it holds.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

enum Request {
    Save(String),
    // Read only where the physics answers the queue.
    #[cfg_attr(not(feature = "physics"), allow(dead_code))]
    Load(String),
    Forget(String),
}

static REQUESTS: Mutex<Vec<Request>> = Mutex::new(Vec::new());
static SLOTS: Mutex<Option<HashMap<String, WorldState>>> = Mutex::new(None);

fn push(r: Request) {
    if let Ok(mut q) = REQUESTS.lock() {
        q.push(r);
    }
}

/// Save the world under `slot` at the end of this frame's physics.
pub fn request_save(slot: &str) {
    push(Request::Save(slot.to_string()));
}

/// Put the world back as `slot` holds it, at the end of this frame's physics.
pub fn request_load(slot: &str) {
    push(Request::Load(slot.to_string()));
}

/// Drop a saved state.
pub fn request_forget(slot: &str) {
    push(Request::Forget(slot.to_string()));
}

/// Whether `slot` holds a state - saves still queued count, so a script can
/// ask straight after saving.
pub fn has(slot: &str) -> bool {
    let queued = REQUESTS.lock().is_ok_and(|q| {
        q.iter().rev().find_map(|r| match r {
            Request::Save(s) if s == slot => Some(true),
            Request::Forget(s) if s == slot => Some(false),
            _ => None,
        }) == Some(true)
    });
    queued || SLOTS.lock().is_ok_and(|s| s.as_ref().is_some_and(|m| m.contains_key(slot)))
}

/// Forget every state (a test, or a host replacing the world wholesale).
pub fn clear() {
    if let Ok(mut q) = REQUESTS.lock() {
        q.clear();
    }
    if let Ok(mut s) = SLOTS.lock() {
        *s = None;
    }
}

/// Write the world's entities down (no physics: see [`sync`]).
pub fn capture(world: &World) -> WorldState {
    crate::package::component_registry::with_registry(|registry| {
        let mut entities = Vec::new();
        for e in world.iter().map(|e| e.entity()) {
            entities.push(SavedEntity {
                handle: e.to_bits().get(),
                name: world.get::<&Name>(e).ok().map(|n| n.0.clone()),
                parent: world.get::<&Parent>(e).ok().map(|p| p.0.to_bits().get()),
                components: registry.serialize_entity(world, e),
                custom: world.get::<&CustomComponents>(e).ok().map(|c| c.0.clone()),
            });
        }
        WorldState { entities, bodies: HashMap::new() }
    })
}

/// Put `state`'s entities back under their own handles; despawn the rest.
/// Returns the physics ids of entities that are gone.
pub fn restore(world: &mut World, state: &WorldState) -> Vec<u64> {
    let keep: std::collections::HashSet<u64> = state.entities.iter().map(|s| s.handle).collect();
    let alive: Vec<Entity> = world.iter().map(|e| e.entity()).collect();
    let mut gone = Vec::new();
    for e in alive {
        if !keep.contains(&e.to_bits().get()) {
            gone.push(e.id() as u64);
            let _ = world.despawn(e);
        }
    }
    crate::package::component_registry::with_registry(|registry| {
        for s in &state.entities {
            let Some(handle) = Entity::from_bits(s.handle) else { continue };
            let mut b = hecs::EntityBuilder::new();
            let unknown = registry.deserialize_into_builder(&mut b, &s.components);
            if let Some(n) = &s.name {
                b.add(Name(n.clone()));
            }
            if let Some(p) = s.parent.and_then(Entity::from_bits) {
                b.add(Parent(p));
            }
            let mut custom = s.custom.clone().unwrap_or_default();
            custom.extend(unknown);
            if !custom.is_empty() {
                b.add(CustomComponents(custom));
            }
            if world.contains(handle) {
                let _ = world.despawn(handle);
            }
            world.spawn_at(handle, b.build());
        }
    });
    gone
}

/// Answer this frame's requests: save the world and its bodies, or put
/// them back. Called at the end of `physics_frame_end`.
#[cfg(feature = "physics")]
pub fn sync(world: &mut World, physics: &mut crate::physics::PhysicsWorld) {
    let requests = match REQUESTS.lock() {
        Ok(mut q) if !q.is_empty() => std::mem::take(&mut *q),
        _ => return,
    };
    for r in requests {
        match r {
            Request::Save(slot) => {
                let mut state = capture(world);
                for e in world.iter().map(|e| e.entity()) {
                    let id = e.id() as u64;
                    if let Some(b) = physics.body_state(id) {
                        state.bodies.insert(id, b);
                    }
                }
                log::info!("[state] saved '{slot}': {} entities, {} bodies", state.len(), state.bodies.len());
                if let Ok(mut s) = SLOTS.lock() {
                    s.get_or_insert_with(HashMap::new).insert(slot, state);
                }
            }
            Request::Load(slot) => {
                let state = SLOTS.lock().ok().and_then(|s| s.as_ref().and_then(|m| m.get(&slot).cloned()));
                let Some(state) = state else {
                    log::warn!("[state] nothing saved as '{slot}'");
                    continue;
                };
                for id in restore(world, &state) {
                    physics.remove_body(id);
                }
                // Bodies for what came back, then every body as it was.
                crate::ecs::systems::ensure_physics_bodies(world, physics);
                for (id, b) in &state.bodies {
                    physics.restore_body(*id, *b);
                }
                log::info!("[state] loaded '{slot}'");
            }
            Request::Forget(slot) => {
                if let Ok(mut s) = SLOTS.lock()
                    && let Some(m) = s.as_mut()
                {
                    m.remove(&slot);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::Transform;

    #[test]
    fn a_world_comes_back_under_its_own_handles() {
        let mut world = World::new();
        let a = world.spawn((Name("A".into()), Transform::default()));
        let b = world.spawn((Name("B".into()), Transform::default(), Parent(a)));
        let state = capture(&world);
        // Things change: B moves and is renamed, A goes, C arrives.
        world.get::<&mut Transform>(b).unwrap().position.x = 50.0;
        world.get::<&mut Name>(b).unwrap().0 = "B2".into();
        world.despawn(a).unwrap();
        let c = world.spawn((Name("C".into()),));
        let gone = restore(&mut world, &state);
        assert_eq!(gone, vec![c.id() as u64]);
        assert!(!world.contains(c));
        assert!(world.contains(a), "A is back under its own handle");
        assert_eq!(world.get::<&Name>(a).unwrap().0, "A");
        assert_eq!(world.get::<&Name>(b).unwrap().0, "B");
        assert_eq!(world.get::<&Transform>(b).unwrap().position.x, 0.0);
        assert_eq!(world.get::<&Parent>(b).unwrap().0, a);
    }
}
