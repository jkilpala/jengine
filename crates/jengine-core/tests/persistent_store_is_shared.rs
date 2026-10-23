//! `persistent_*` is one store in every runtime: the engine's
//! `scripting::persistent`.
//!
//! The browser preamble used to keep its own store in
//! `window.__jengine_persistent`, so on native a package (Rust) and a script
//! shared the store and in the editor and web builds they did not: neither
//! saw the other's writes, silently (BUGS #106). Found building the
//! arcade's monster RPG, whose package hands the party to the battle scene
//! through the store. Now browser writes are commands applied to the one
//! store, and the hosts publish it back every frame.

use hecs::{Entity, World};
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
use jengine_core::scripting::persistent;
use serde_json::json;

const PREAMBLE: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/script_host.rs");

struct Host(World);
impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, _id: u32) -> Option<Entity> {
        None
    }
}


/// The per-frame publish both browser hosts share (`adapters::js::mirror`).
const MIRROR: &str = include_str!("../src/scripting/bridge/adapters/js/mirror.rs");

/// A host's source as far as what it publishes goes: its own, plus the shared
/// mirror - but only if it calls it, so a host that dropped the call fails.
fn with_mirror(src: &str) -> String {
    if src.contains("mirror::sync_world_to_play_namespace(") {
        format!("{src}
{MIRROR}")
    } else {
        src.to_string()
    }
}

#[test]
fn a_browser_write_lands_in_the_engine_store() {
    let mut host = Host(World::new());
    assert!(handle_world_command(&mut host, "psn", &JsonArgs::new(json!({ "k": "shared.n", "n": 7 }))));
    assert!(handle_world_command(&mut host, "pss", &JsonArgs::new(json!({ "k": "shared.s", "s": "party" }))));
    assert_eq!(persistent::get_number("shared.n"), Some(7.0));
    assert_eq!(persistent::get_string("shared.s").as_deref(), Some("party"));
    // And what the hosts publish carries it back to the preamble.
    let v: serde_json::Value = serde_json::from_str(&persistent::to_json()).unwrap();
    assert_eq!(v["strings"]["shared.s"], "party");
}

#[test]
fn the_preamble_writes_through_and_reads_what_the_hosts_publish() {
    for (tag, func) in [("psn", "persistent_set_number"), ("pss", "persistent_set_string"), ("pcl", "persistent_clear")] {
        let body = &PREAMBLE[PREAMBLE.find(&format!("function {func}(")).expect(func)..];
        let body = &body[..body.find("\n  }}").unwrap()];
        assert!(body.contains(&format!("t: '{tag}'")), "{func} does not send `{tag}` to the engine store");
    }
    assert!(PREAMBLE.contains("st.persistent_json"), "the preamble never refreshes its mirror");
    for (name, src) in [("viewport", with_mirror(VIEWPORT)), ("web", with_mirror(WEB))] {
        assert!(
            src.contains("\"persistent_json\"") && src.contains("persistent::to_json()"),
            "the {name} host does not publish the engine store"
        );
    }
}
