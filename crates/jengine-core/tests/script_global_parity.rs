//! Every script global must exist in every runtime — or be listed here.
//!
//! There is no single place that registers a script global. Native installs
//! them on the Luau VM (`adapters/luau.rs`, plus everything declared with
//! `reg!` in `package/core_script_api.rs`, which the adapter installs
//! dynamically); the editor and deployed web builds share one Fengari VM whose
//! surface is the `BRIDGE_FUNCTIONS` allowlist in
//! `editor/src/lib/lua-runtime.ts`. Nothing checks that the two agree, and
//! compiling proves nothing either way.
//!
//! The failure this prevents: a function wired everywhere in Rust but missing
//! from `BRIDGE_FUNCTIONS` is simply `nil` in the editor — the runtime you
//! actually develop in — and in every web build. A game that saves progress or
//! parents entities then runs correctly on native and silently does nothing in
//! the editor. An audit in Sep 2026 found 41 functions in that state, split
//! across whole feature groups (saves, hierarchy, pathfinding, input
//! remapping, audio busses, skeletal rigs).
//!
//! This is a **ratchet**, not a gate: the known gaps are listed below and the
//! test fails on anything *new*. It also fails when a listed gap is closed
//! without the list being updated, so the list cannot rot into fiction.
//!
//! Like `preamble_command_tags`, it reads source text rather than calling into
//! the registries. `BRIDGE_FUNCTIONS` is TypeScript, and the Luau adapter is
//! behind `feature = "native"` — a test that needed both to be loadable would
//! not run in a plain `cargo test`, which is how these gaps accumulated.

use std::collections::BTreeSet;

const LUAU_RS: &str = include_str!("../src/scripting/bridge/adapters/luau.rs");

/// Registered on native, missing from the editor/web Fengari runtime.
///
/// **This list should only ever shrink.** Closing a gap means wiring the
/// function through `lua-runtime.ts` + the viewport and web
/// `drain_play_commands_into`, then deleting the name from here.
const KNOWN_NATIVE_ONLY: &[&str] = &[
    // Input remapping (the `.input` asset workflow).
    // Audio mixer busses.
    // Skeletal rigs + IK: `spawn_rig` was the last, closed Oct 2026 by
    // delivering `.rig` assets to every runtime.
    // Misc.
];

/// Present in the editor/web runtime, missing on native.
///
/// Most of these are the `reg!`-metadata trap: `core_script_api.rs` describes
/// them (so the editor shows a tooltip and the browser preamble implements
/// them) but nothing installs them on the Luau VM, because
/// `install_script_api` skips handler-less descriptors. A script using one
/// runs in the editor and errors on native — the mirror image of the more
/// familiar failure, and it went unnoticed for exactly that reason.
///
/// Closing one means either giving its descriptor a handler or setting it
/// directly in `adapters/luau.rs`.
const KNOWN_EDITOR_ONLY: &[&str] = &[
    // Not registered anywhere on native.
    // Registered as metadata in `core_script_api.rs`, never installed.
    // `raycast` was here, and it was the last one. It is bound on native
    // since Sep 2026: the snapshot *does* carry a borrowed physics world
    // now (`BridgeReadSnapshot::physics_ptr`), which is the same trick
    // `world_ptr` uses for `get_custom` and needs no collider copying and
    // no reimplementation of ray-vs-shape. What made it look impossible is
    // that `ScriptState` already had a `physics_world_ptr` and a working
    // `raycast` on it, and nothing had ever called the method that sets
    // the pointer - so the route existed, was dead, and read as tried.
    // `crates/jengine-core/tests/raycast_reaches_the_real_world.rs`.
    // Entity lookup.
    // Math helpers. `math.clamp` / `math.lerp` exist in both runtimes; it is
    // the bare globals that are browser-only.
];

/// Globals the Fengari runtime provides in Lua itself rather than as a bridge
/// function, so they are absent from `BRIDGE_FUNCTIONS` by design.
const IMPLEMENTED_LUA_SIDE: &[&str] = &[
    // `require` resolves against the seeded prefab_scripts cache inside
    // lua-runtime.ts; there is no command to send.
    "require",
    // `print` joins its arguments and goes to the log, as native's does
    // (Sep 2026); lua-runtime.ts installs it with `lua_setglobal`.
    "print",
];

/// Read the Fengari allowlist out of the editor's TypeScript.
///
/// Not `include_str!`: the editor lives outside this crate, and a missing
/// checkout should fail with a sentence rather than a macro error.
fn editor_bridge_functions() -> BTreeSet<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../editor/src/lib/lua-runtime.ts");
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {} ({e}). This test compares the native script API \
             against the editor's Fengari allowlist and needs the full \
             workspace checkout.",
            path.display()
        )
    });
    let start = src
        .find("BRIDGE_FUNCTIONS")
        .expect("BRIDGE_FUNCTIONS not found in lua-runtime.ts — was it renamed?");
    // Skip past the type annotation: the declaration reads
    // `const BRIDGE_FUNCTIONS: string[] = [`, and the first `[` belongs to
    // `string[]`, not to the array.
    let eq = src[start..]
        .find('=')
        .expect("BRIDGE_FUNCTIONS has no initializer");
    let open = eq + src[start + eq..]
        .find('[')
        .expect("BRIDGE_FUNCTIONS is not an array literal");
    let end = src[start + open..]
        .find(']')
        .expect("unterminated BRIDGE_FUNCTIONS array");
    let body = &src[start + open..start + open + end];
    // Line comments out first: an apostrophe in one ("the mix's") opened a
    // quote that swallowed every name after it - twice, before this.
    let code: String = body.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("
");
    quoted_names(&code)
}

/// Every `'name'` / `"name"` in a chunk of source, restricted to the shape a
/// script global takes (lower snake case).
fn quoted_names(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let quote = bytes[i];
        if (quote == b'\'' || quote == b'"')
            && let Some(close) = text[i + 1..].find(quote as char) {
                let name = &text[i + 1..i + 1 + close];
                if is_global_name(name) {
                    out.insert(name.to_string());
                }
                i += close + 2;
                continue;
            }
        i += 1;
    }
    out
}

fn is_global_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() > 1
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Globals the Luau adapter sets — the real native surface.
///
/// Deliberately *not* including `core_script_api.rs`. Its `reg!` entries are
/// metadata for editor tooltips; `BridgeRunner::install_script_api` installs
/// only descriptors that carry a handler ("Functions without a `handler` are
/// skipped silently"), and none of the 139 core entries does. Counting them
/// as native is what hid 21 functions that work in the browser runtimes and
/// are `nil` on native.
fn native_globals() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in LUAU_RS.lines() {
        // `globals.set("name", …)` and `lua.globals().set("name", …)`.
        for marker in ["globals.set(", "globals().set("] {
            if let Some(idx) = line.find(marker) {
                let rest = &line[idx + marker.len()..];
                if let Some(name) = first_quoted(rest) {
                    out.insert(name);
                }
            }
        }
    }
    out
}

fn first_quoted(text: &str) -> Option<String> {
    let start = text.find('"')?;
    let rest = &text[start + 1..];
    let end = rest.find('"')?;
    let name = &rest[..end];
    is_global_name(name).then(|| name.to_string())
}

#[test]
fn script_globals_reach_every_runtime() {
    let native = native_globals();
    let editor = editor_bridge_functions();

    // Parsing guard. Both sides are scraped from source, so a refactor that
    // breaks a pattern would otherwise turn this test into a no-op that
    // passes forever. These floors are far below the real counts (191 and
    // 161 in Sep 2026) and only catch wholesale parse failure.
    assert!(
        native.len() > 120,
        "only parsed {} native globals — the scrape in native_globals() has \
         stopped matching, so this test is no longer checking anything",
        native.len()
    );
    assert!(
        editor.len() > 120,
        "only parsed {} entries from BRIDGE_FUNCTIONS — the scrape in \
         editor_bridge_functions() has stopped matching",
        editor.len()
    );

    let known_native_only: BTreeSet<&str> = KNOWN_NATIVE_ONLY.iter().copied().collect();
    let known_editor_only: BTreeSet<&str> = KNOWN_EDITOR_ONLY.iter().copied().collect();
    let lua_side: BTreeSet<&str> = IMPLEMENTED_LUA_SIDE.iter().copied().collect();

    let native_only: BTreeSet<&str> = native
        .iter()
        .map(String::as_str)
        .filter(|n| !editor.contains(*n))
        .collect();
    let editor_only: BTreeSet<&str> = editor
        .iter()
        .map(String::as_str)
        .filter(|n| !native.contains(*n))
        .collect();

    let new_native_only: Vec<&&str> = native_only
        .iter()
        .filter(|n| !known_native_only.contains(*n) && !lua_side.contains(*n))
        .collect();
    let new_editor_only: Vec<&&str> = editor_only
        .iter()
        .filter(|n| !known_editor_only.contains(*n))
        .collect();

    assert!(
        new_native_only.is_empty(),
        "new script global(s) registered on native but missing from \
         BRIDGE_FUNCTIONS in editor/src/lib/lua-runtime.ts: {new_native_only:?}\n\
         They will be `nil` in the editor AND in every web build. Wire them \
         through lua-runtime.ts plus the viewport and web \
         `drain_play_commands_into`, or add them to KNOWN_NATIVE_ONLY with a \
         reason."
    );
    assert!(
        new_editor_only.is_empty(),
        "new script global(s) in BRIDGE_FUNCTIONS with no native \
         registration: {new_editor_only:?}\n\
         They will be `nil` in the native runtime. Register them in \
         adapters/luau.rs or package/core_script_api.rs, or add them to \
         KNOWN_EDITOR_ONLY."
    );

    // The other direction: a gap that has been closed must leave the list, or
    // the list slowly becomes a lie and stops meaning anything.
    let stale_native: Vec<&&str> = known_native_only
        .iter()
        .filter(|n| !native_only.contains(*n))
        .collect();
    let stale_editor: Vec<&&str> = known_editor_only
        .iter()
        .filter(|n| !editor_only.contains(*n))
        .collect();
    assert!(
        stale_native.is_empty(),
        "KNOWN_NATIVE_ONLY lists {stale_native:?}, which now reach the editor \
         runtime (or are no longer registered natively). Delete them from the \
         list — that is what closing a parity gap looks like."
    );
    assert!(
        stale_editor.is_empty(),
        "KNOWN_EDITOR_ONLY lists {stale_editor:?}, which are no longer \
         editor-only. Delete them from the list."
    );
}
