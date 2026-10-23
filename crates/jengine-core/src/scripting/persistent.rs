//! Persistent key/value store that survives scene transitions.
//!
//! `ScriptSystem::reset_initialized` rebuilds the Lua VM (and equivalently
//! the JS context) on every `load_scene` / `pop_scene`, so script globals
//! and module state evaporate between scenes. This module provides a tiny
//! process-global store that scripts can use as a "save slot" — score
//! counters, player names, last visited scene, etc.
//!
//! The store holds two flat namespaces (numbers + strings) keyed by
//! string. Both Lua and JS bridges expose the same API:
//!
//! ```text
//! persistent_set_number(key, value)
//! persistent_get_number(key)       -- returns nil if absent
//! persistent_set_string(key, value)
//! persistent_get_string(key)       -- returns nil if absent
//! persistent_has(key)              -- true if either namespace has key
//! persistent_clear()               -- wipe everything
//! ```
//!
//! **This store is the one every runtime uses.** Native scripts call it
//! directly; the browser hosts mirror it into `window.__jengine_persistent`
//! for the preamble's synchronous reads, send every write back here as a
//! `psn` / `pss` / `pcl` command, and publish it each frame as
//! `state.persistent_json`. Before that the browser preamble kept a store of
//! its own, so Rust code - a package - writing here was invisible to scripts
//! in the editor and on the web, and scripts' writes invisible to it
//! (BUGS #106).
//!
//! The store is a single global `Mutex<Inner>`. Scripting is single
//! threaded today so contention is irrelevant; the mutex just satisfies
//! `Sync` for the static.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Default)]
struct Inner {
    numbers: HashMap<String, f64>,
    strings: HashMap<String, String>,
}

fn store() -> &'static Mutex<Inner> {
    static STORE: OnceLock<Mutex<Inner>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Inner::default()))
}

fn lock() -> std::sync::MutexGuard<'static, Inner> {
    match store().lock() {
        Ok(g) => g,
        // Poisoning can only happen if a script panic'd while the guard
        // was held. Recover the inner state and keep going so the next
        // scene's scripts can still read/write.
        Err(p) => p.into_inner(),
    }
}

pub fn set_number(key: &str, value: f64) {
    lock().numbers.insert(key.to_string(), value);
}

pub fn get_number(key: &str) -> Option<f64> {
    lock().numbers.get(key).copied()
}

pub fn set_string(key: &str, value: &str) {
    lock().strings.insert(key.to_string(), value.to_string());
}

pub fn get_string(key: &str) -> Option<String> {
    lock().strings.get(key).cloned()
}

pub fn has(key: &str) -> bool {
    let g = lock();
    g.numbers.contains_key(key) || g.strings.contains_key(key)
}

/// Forget one key, whichever kind it holds.
pub fn remove(key: &str) {
    let mut g = lock();
    g.numbers.remove(key);
    g.strings.remove(key);
}

pub fn clear() {
    let mut g = lock();
    g.numbers.clear();
    g.strings.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_numbers_and_strings() {
        clear();
        set_number("score", 42.0);
        set_string("name", "alice");
        assert_eq!(get_number("score"), Some(42.0));
        assert_eq!(get_string("name").as_deref(), Some("alice"));
        assert!(has("score"));
        assert!(has("name"));
        assert!(!has("missing"));
        clear();
        assert!(get_number("score").is_none());
        assert!(get_string("name").is_none());
    }
}

/// The whole store as JSON, `{"numbers": {...}, "strings": {...}}`: what
/// the browser hosts publish for the preamble's mirror.
pub fn to_json() -> String {
    let g = lock();
    serde_json::json!({ "numbers": g.numbers, "strings": g.strings }).to_string()
}

#[cfg(test)]
mod json_tests {
    use super::*;

    #[test]
    fn the_store_serialises_both_namespaces() {
        set_number("json_test.n", 2.5);
        set_string("json_test.s", "hi");
        let v: serde_json::Value = serde_json::from_str(&to_json()).unwrap();
        assert_eq!(v["numbers"]["json_test.n"], 2.5);
        assert_eq!(v["strings"]["json_test.s"], "hi");
    }
}
