//! Callbacks the engine owes an entity's own script (Oct 2026):
//! `on_screen_entered`, `on_path_end` and whatever comes next.
//!
//! A system that wants to tell an entity's script something - "you came on
//! screen", "you reached the end of your path" - [`push`]es it here, and every
//! host drains the queue once a frame with [`take`] and calls the handler on
//! that entity's script: natively through
//! `ScriptSystem::queue_entity_callbacks`, in the browsers through their
//! `call_lifecycle_handler`. So a new callback is a `push` in core and no host
//! wiring - the hosts used to grow a dispatch site per callback, and each was
//! a place one runtime could forget.
//!
//! The queue is per thread (the world is), and bounded: a world ticked with
//! no host draining it - a headless test, a tool - keeps the newest
//! [`MAX_QUEUED`] and drops the rest.

use std::cell::RefCell;

use hecs::Entity;

/// One handler to call on one entity's script, with no payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityCallback {
    pub entity: Entity,
    /// The handler's name, `on_...`.
    pub handler: &'static str,
}

impl EntityCallback {
    /// The event name a host dispatching `on_<name>` uses.
    pub fn event(&self) -> &'static str {
        self.handler.strip_prefix("on_").unwrap_or(self.handler)
    }
}

/// The most callbacks kept between two drains.
pub const MAX_QUEUED: usize = 4096;

thread_local! {
    static QUEUE: RefCell<Vec<EntityCallback>> = const { RefCell::new(Vec::new()) };
}

/// Owe `entity`'s script a call to `handler`.
pub fn push(entity: Entity, handler: &'static str) {
    QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        if q.len() >= MAX_QUEUED {
            q.remove(0);
        }
        q.push(EntityCallback { entity, handler });
    });
}

/// Everything owed since the last call, oldest first.
pub fn take() -> Vec<EntityCallback> {
    QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut()))
}


/// `on_<name>`, interned once a name: callbacks carry `&'static str`
/// handler names, and the names a timeline, a dialogue or a behaviour
/// tree raises are a small, fixed set.
#[cfg(any(feature = "timeline", feature = "dialogue", feature = "behavior_tree"))]
pub(crate) fn handler_for(name: &str) -> &'static str {
    use std::sync::{Mutex, OnceLock};
    static NAMES: OnceLock<Mutex<std::collections::HashSet<&'static str>>> = OnceLock::new();
    let full = format!("on_{name}");
    let mut set = NAMES.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = set.get(full.as_str()) {
        return s;
    }
    let s: &'static str = Box::leak(full.into_boxed_str());
    set.insert(s);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callbacks_queue_in_order_and_are_taken_once() {
        let mut world = hecs::World::new();
        let a = world.spawn(());
        let b = world.spawn(());
        take();
        push(a, "on_screen_entered");
        push(b, "on_path_end");
        let got = take();
        assert_eq!(got, vec![
            EntityCallback { entity: a, handler: "on_screen_entered" },
            EntityCallback { entity: b, handler: "on_path_end" },
        ]);
        assert_eq!(got[1].event(), "path_end");
        assert!(take().is_empty());
    }

    #[test]
    fn an_undrained_queue_keeps_the_newest() {
        let mut world = hecs::World::new();
        let e = world.spawn(());
        take();
        for _ in 0..MAX_QUEUED + 10 {
            push(e, "on_path_end");
        }
        assert_eq!(take().len(), MAX_QUEUED);
    }
}
