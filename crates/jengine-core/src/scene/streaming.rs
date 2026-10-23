//! Scenes that load in the background, and scenes laid into the running one
//! (Oct 2026; Unity's `LoadSceneAsync` and `LoadSceneMode.Additive`, Godot's
//! threaded resource loading and instancing a scene into the tree).
//!
//! `Scene.load_additive(path, x, y)` adds a scene to the one running: its
//! entities come in offset by `(x, y)` and tagged with the **chunk** they came
//! in with ([`ChunkMember`]) once everything they draw has loaded, so a long
//! level can stream the next room in ahead of the player and
//! `Scene.unload(chunk)` drop the one behind. `Scene.preload(path)` loads a
//! scene's assets in the background without showing it, so a `Scene.load`
//! that follows is a swap rather than a hitch. `Scene.status` and
//! `Scene.progress` say how far along either is.
//!
//! Before this a scene came in only whole and only at once: `Scene.load`
//! read and decoded every texture on the frame it was asked for, and a
//! level bigger than one scene was one enormous scene.
//!
//! Like the display settings, the requests are the process's: a script asks
//! here, each host drains them once a frame ([`take_requests`]), loads in its
//! own way - native decodes textures on worker threads, the browsers fetch -
//! and reports how far it has got ([`set_status`], [`set_preload`]). Laying a
//! loaded chunk into the world is shared, [`merge`], and so is taking it out,
//! [`unload_chunk`].

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use glam::Vec2;
use hecs::{Entity, World};

use crate::ecs::components::{Parent, Transform};

/// Which additive chunk an entity came in with. Runtime only: never saved,
/// never registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkMember(pub u32);

/// What a script asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Add the scene at `path` to the running one, offset by `offset`, as
    /// chunk `handle`.
    Additive { handle: u32, path: String, offset: [f32; 2] },
    /// Take chunk `handle` out again.
    Unload { handle: u32 },
    /// Load the scene's assets without showing it.
    Preload { path: String },
}

/// How far a chunk or a preload has got.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    /// Loading, this far (0..1).
    Loading(f32),
    Loaded,
    Failed,
    /// Never asked for, taken out again, or gone with the scene it was in.
    Unloaded,
}

impl Status {
    /// `Scene.status`.
    pub fn name(&self) -> &'static str {
        match self {
            Status::Loading(_) => "loading",
            Status::Loaded => "loaded",
            Status::Failed => "failed",
            Status::Unloaded => "unloaded",
        }
    }

    /// `Scene.progress`: 1 once loaded, 0 for what is not coming.
    pub fn progress(&self) -> f32 {
        match self {
            Status::Loading(p) => p.clamp(0.0, 0.99),
            Status::Loaded => 1.0,
            Status::Failed | Status::Unloaded => 0.0,
        }
    }
}

#[derive(Default)]
struct State {
    next: u32,
    requests: Vec<Request>,
    chunks: HashMap<u32, Status>,
    preloads: HashMap<String, Status>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut s)
}

/// `Scene.load_additive(path, x, y)`: a handle at once, the chunk when its
/// assets are in.
pub fn load_additive(path: &str, x: f32, y: f32) -> u32 {
    with(|s| {
        s.next += 1;
        let handle = s.next;
        s.chunks.insert(handle, Status::Loading(0.0));
        s.requests.push(Request::Additive { handle, path: path.to_string(), offset: [x, y] });
        handle
    })
}

/// `Scene.unload(handle)`: a chunk still loading is simply not laid in.
pub fn unload(handle: u32) {
    with(|s| {
        if s.chunks.contains_key(&handle) {
            s.chunks.insert(handle, Status::Unloaded);
            s.requests.push(Request::Unload { handle });
        }
    });
}

/// `Scene.preload(path)`. Asking again for a scene already preloaded, or on
/// its way, does nothing.
pub fn preload(path: &str) {
    with(|s| {
        if matches!(s.preloads.get(path), Some(Status::Loading(_) | Status::Loaded)) {
            return;
        }
        s.preloads.insert(path.to_string(), Status::Loading(0.0));
        s.requests.push(Request::Preload { path: path.to_string() });
    });
}

/// A chunk's status.
pub fn status(handle: u32) -> Status {
    with(|s| s.chunks.get(&handle).copied().unwrap_or(Status::Unloaded))
}

/// A preload's status.
pub fn preload_status(path: &str) -> Status {
    with(|s| s.preloads.get(path).copied().unwrap_or(Status::Unloaded))
}

/// Whether chunk `handle` is still wanted - asked for and not unloaded since
/// - which a host checks before laying in what it has loaded.
pub fn is_wanted(handle: u32) -> bool {
    matches!(status(handle), Status::Loading(_))
}

/// Everything asked for since the last call. A host calls this once a frame.
pub fn take_requests() -> Vec<Request> {
    with(|s| std::mem::take(&mut s.requests))
}

/// The host says how far a chunk has got. Ignored for a chunk no longer
/// wanted, so a late "loaded" cannot bring an unloaded one back.
pub fn set_status(handle: u32, status: Status) {
    with(|s| {
        if let Some(cur) = s.chunks.get_mut(&handle)
            && (matches!(cur, Status::Loading(_)) || matches!(status, Status::Unloaded))
        {
            *cur = status;
        }
    });
}

/// The host says how far a preload has got.
pub fn set_preload(path: &str, status: Status) {
    with(|s| {
        s.preloads.insert(path.to_string(), status);
    });
}

/// The running scene was replaced: its chunks went with it. Preloads stay -
/// they are for scenes still to come.
pub fn scene_replaced() {
    // A scene transition armed for this change starts playing.
    crate::renderer::transition::on_scene_replaced();
    // An armed input replay starts with the fresh world (`input::replay`).
    crate::input::replay::on_scene_replaced();
    with(|s| {
        for st in s.chunks.values_mut() {
            *st = Status::Unloaded;
        }
        s.requests.retain(|r| matches!(r, Request::Preload { .. }));
    });
}

/// Lay `chunk` into `world` as chunk `handle`: every entity moved across,
/// its `Parent` re-pointed at its moved parent, the roots offset by
/// `offset`, each tagged [`ChunkMember`]. Returns `(old, new)` for every
/// entity in the chunk's own order (by id), which is the scene file's order
/// for a freshly loaded chunk - how a host matches its scripts to them.
pub fn merge(world: &mut World, mut chunk: World, handle: u32, offset: Vec2) -> Vec<(Entity, Entity)> {
    let mut olds: Vec<Entity> = chunk.iter().map(|e| e.entity()).collect();
    olds.sort_by_key(|e| e.id());
    let mut pairs = Vec::with_capacity(olds.len());
    for old in olds {
        let Ok(taken) = chunk.take(old) else { continue };
        let new = world.spawn(taken);
        pairs.push((old, new));
    }
    let map: HashMap<Entity, Entity> = pairs.iter().copied().collect();
    for &(_, new) in &pairs {
        let parent = world.get::<&Parent>(new).ok().map(|p| p.0);
        match parent.map(|p| map.get(&p).copied()) {
            Some(Some(moved)) => {
                let _ = world.insert_one(new, Parent(moved));
            }
            // A parent outside the chunk is not this world's to point at.
            Some(None) => {
                let _ = world.remove_one::<Parent>(new);
            }
            None => {
                if let Ok(mut t) = world.get::<&mut Transform>(new) {
                    t.position += offset;
                }
            }
        }
        let _ = world.insert_one(new, ChunkMember(handle));
    }
    pairs
}

/// Take chunk `handle` out of `world`. Returns the entities that went, for
/// a host to drop what it keeps per entity (a browser's script instances).
pub fn unload_chunk(world: &mut World, handle: u32) -> Vec<Entity> {
    let members: Vec<Entity> = world
        .query::<&ChunkMember>()
        .iter()
        .filter(|(_, m)| m.0 == handle)
        .map(|(e, _)| e)
        .collect();
    for &e in &members {
        let _ = world.despawn(e);
    }
    members
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chunk_comes_in_offset_with_its_hierarchy_and_goes_again() {
        let mut world = World::new();
        let keep = world.spawn((Transform::default(),));
        let mut chunk = World::new();
        let root = chunk.spawn((Transform { position: Vec2::new(10.0, 0.0), ..Default::default() },));
        let child = chunk.spawn((Transform { position: Vec2::new(1.0, 1.0), ..Default::default() }, Parent(root)));
        let pairs = merge(&mut world, chunk, 7, Vec2::new(100.0, 50.0));
        assert_eq!(pairs.len(), 2);
        let (new_root, new_child) = (pairs[0].1, pairs[1].1);
        assert_eq!(world.get::<&Transform>(new_root).unwrap().position, Vec2::new(110.0, 50.0), "the root moved");
        assert_eq!(world.get::<&Transform>(new_child).unwrap().position, Vec2::new(1.0, 1.0), "the child kept its local place");
        assert_eq!(world.get::<&Parent>(new_child).unwrap().0, new_root, "and its parent, in this world");
        let _ = child;
        assert_eq!(unload_chunk(&mut world, 7).len(), 2);
        assert!(world.contains(keep) && !world.contains(new_root));
    }

    #[test]
    fn requests_handles_and_statuses() {
        let a = load_additive("scenes/room_a.scene", 0.0, 0.0);
        let b = load_additive("scenes/room_b.scene", 800.0, 0.0);
        assert_ne!(a, b);
        preload("scenes/boss.scene");
        preload("scenes/boss.scene");
        let reqs: Vec<Request> = take_requests()
            .into_iter()
            .filter(|r| match r {
                Request::Additive { handle, .. } => *handle == a || *handle == b,
                Request::Preload { path } => path == "scenes/boss.scene",
                _ => false,
            })
            .collect();
        assert_eq!(reqs.len(), 3, "one preload, however often asked");
        assert_eq!(status(a), Status::Loading(0.0));
        set_status(a, Status::Loading(0.5));
        assert_eq!(status(a).progress(), 0.5);
        set_status(a, Status::Loaded);
        unload(b);
        set_status(b, Status::Loaded);
        assert_eq!(status(b), Status::Unloaded, "a late load does not bring an unloaded chunk back");
        assert!(!is_wanted(b));
        scene_replaced();
        assert_eq!(status(a), Status::Unloaded);
        set_preload("scenes/boss.scene", Status::Loaded);
        assert_eq!(preload_status("scenes/boss.scene").name(), "loaded");
    }
}
