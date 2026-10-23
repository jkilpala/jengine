//! `Sprite.set_color` and the rest have to exist in every runtime, and every
//! script function has to have one.
//!
//! The namespaces are one list (`scripting::namespaces`). Native builds its
//! tables from it directly - `every_core_namespaced_name_resolves_natively`
//! in the Luau adapter checks each one. The editor and web builds build
//! theirs in `lua-runtime.ts` from the copy the hosts publish, wrapping the
//! flat global each row names. So what can go wrong is at the edges, and
//! that is what this checks:
//!
//! - a row naming a flat function the Fengari runtime does not bind (it would
//!   be skipped there, and the namespaced name would be nil in the editor);
//! - a script function added natively with no namespace at all;
//! - a browser host that stops publishing the list, or a runtime that stops
//!   reading it.

use std::collections::BTreeSet;
use std::path::PathBuf;

use jengine_core::scripting::namespaces::{CORE, DUPLICATES, GLOBALS};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The quoted names inside `const BRIDGE_FUNCTIONS: string[] = [ ... ];`,
/// plus the globals `lua-runtime.ts` sets itself with `lua_setglobal`.
fn fengari_globals() -> BTreeSet<String> {
    let ts = read("editor/src/lib/lua-runtime.ts");
    let start = ts.find("const BRIDGE_FUNCTIONS").expect("BRIDGE_FUNCTIONS");
    let open = start + ts[start..].find("= [").expect("array") + 3;
    let close = open + ts[open..].find("];").expect("end of array");
    let mut out = BTreeSet::new();
    for line in ts[open..close].lines() {
        let code = line.split("//").next().unwrap_or("");
        for part in code.split(',') {
            let name = part.trim().trim_matches(['\'', '"']);
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                out.insert(name.to_string());
            }
        }
    }
    let mut rest = ts.as_str();
    while let Some(i) = rest.find("lua_setglobal(L, to_luastring('") {
        rest = &rest[i + "lua_setglobal(L, to_luastring('".len()..];
        if let Some(end) = rest.find('\'') {
            out.insert(rest[..end].to_string());
        }
    }
    out
}

/// Every `globals.set("name", ...)` in the Luau adapter.
fn native_globals() -> BTreeSet<String> {
    let src = read("crates/jengine-core/src/scripting/bridge/adapters/luau.rs");
    let mut out = BTreeSet::new();
    let mut rest = src.as_str();
    while let Some(i) = rest.find("globals.set(\"") {
        rest = &rest[i + "globals.set(\"".len()..];
        if let Some(end) = rest.find('"') {
            out.insert(rest[..end].to_string());
        }
    }
    out
}

/// Native-only by known design, as in `script_global_parity`. May only
/// shrink - and is empty since `spawn_rig` reached the browsers (Oct 2026).
const KNOWN_NATIVE_ONLY: &[&str] = &[];

#[test]
fn a_known_native_only_function_that_reaches_the_editor_is_struck_off() {
    let fengari = fengari_globals();
    let closed: Vec<&&str> = KNOWN_NATIVE_ONLY.iter().filter(|n| fengari.contains(**n)).collect();
    assert!(closed.is_empty(), "{closed:?} reach the editor now - delete them from KNOWN_NATIVE_ONLY");
}

#[test]
fn every_namespaced_name_points_at_a_function_the_editor_binds() {
    let fengari = fengari_globals();
    let missing: Vec<String> = CORE
        .iter()
        .filter(|(_, _, flat)| !fengari.contains(*flat) && !KNOWN_NATIVE_ONLY.contains(flat))
        .map(|(ns, short, flat)| format!("{ns}.{short} ({flat})"))
        .collect();
    assert!(
        missing.is_empty(),
        "these namespaced names point at a flat function lua-runtime.ts does not bind, so \
         they would be nil in the editor and on the web: {missing:?}"
    );
}

#[test]
fn every_native_script_function_has_a_namespace() {
    let namespaced: BTreeSet<&str> = CORE.iter().map(|(_, _, flat)| *flat).collect();
    let accounted = |name: &str| {
        namespaced.contains(name)
            || GLOBALS.contains(&name)
            || DUPLICATES.iter().any(|(flat, _)| *flat == name)
    };
    let orphans: Vec<String> = native_globals().into_iter().filter(|n| !accounted(n)).collect();
    assert!(
        orphans.is_empty(),
        "script functions with no namespace - add each to `scripting::namespaces::CORE` \
         (or GLOBALS / DUPLICATES if it really should not have one): {orphans:?}"
    );
}

#[test]
fn both_browser_hosts_publish_the_list_and_the_runtime_reads_it() {
    for host in ["crates/jengine-editor-viewport/src/lib.rs", "crates/jengine-web/src/script_host.rs"] {
        assert!(
            read(host).contains("scripting::namespaces::publish("),
            "{host} does not publish the script namespaces; every `Sprite.set_color` \
             would be nil there"
        );
    }
    assert!(
        read("editor/src/lib/lua-runtime.ts").contains("__script_namespaces"),
        "lua-runtime.ts no longer reads the published namespaces"
    );
}
