//! Lightweight thread-local string interner for `BridgeCommand` field
//! and component names.
//!
//! At high entity counts (10k+ scripted entities) the
//! [`crate::scripting::bridge::command::BridgeCommand::SetCustomField`] /
//! [`crate::scripting::bridge::command::BridgeCommand::SetCustomFields`]
//! variants dominate the per-frame command queue. Each command carries a
//! `component_name` and one or more `field_name` strings, and scripts
//! re-emit the same string literals every frame (e.g. `"MoverData"`,
//! `"velocity_x"`). Without interning that's ~10⁵ String allocations and
//! frees per frame just for these labels.
//!
//! `intern(s) -> Arc<str>` returns a shared reference to a single
//! allocation per unique string. Field names come from a bounded set of
//! literals in script source, so the interner saturates quickly and
//! steady-state cost is one hash lookup + one `Arc::clone`.
//!
//! ## Threading
//!
//! Interning is per-thread (`thread_local!`). Bridge commands are
//! produced + consumed on the same thread today (the script runtimes are
//! single-threaded). If we ever fan out script execution across threads,
//! switch to `OnceLock<RwLock<...>>` — but pay the lock cost only then.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

thread_local! {
    /// Storing the `Arc<str>` directly (not the underlying `Box<str>`) lets
    /// `HashSet::get(&str)` find existing entries without allocating: the
    /// `Borrow<str>` impl on `Arc<str>` walks the inner string slice.
    static INTERNER: RefCell<HashSet<Arc<str>>> = RefCell::new(HashSet::with_capacity(64));
}

/// Return an interned `Arc<str>` view of `s`. Allocates exactly once per
/// unique string for the lifetime of the thread. After the first call for
/// a given string, subsequent calls are a hash lookup + refcount bump.
pub fn intern(s: &str) -> Arc<str> {
    INTERNER.with(|i| {
        let mut set = i.borrow_mut();
        if let Some(existing) = set.get(s) {
            return Arc::clone(existing);
        }
        let arc: Arc<str> = Arc::from(s);
        set.insert(Arc::clone(&arc));
        arc
    })
}

#[cfg(test)]
mod tests {
    use super::intern;

    #[test]
    fn same_input_returns_same_arc() {
        let a = intern("MoverData");
        let b = intern("MoverData");
        assert!(std::sync::Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn different_inputs_are_distinct() {
        let a = intern("velocity_x");
        let b = intern("velocity_y");
        assert!(!std::sync::Arc::ptr_eq(&a, &b));
        assert_ne!(&*a, &*b);
    }

    #[test]
    fn round_trips_content() {
        let a = intern("hello");
        assert_eq!(&*a, "hello");
    }
}
