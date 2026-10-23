//! Every native global that returns several values must be unpacked into
//! several values by the editor's Lua runtime too.
//!
//! The browser preamble returns a JS array for a multi-value function, and
//! `lua-runtime.ts` spreads that array into separate Lua values **only** for a
//! name in its hand-kept `MULTI_RETURN` set. Anything else reaches the script
//! as one table. So `local hp, max = get_health(e)` gave `hp` a table and
//! `max` nil in the editor and on the web - and the first arithmetic on `hp`
//! killed the script's whole `update`, navigation included (test-lab scene
//! 36). `get_contact` had the same gap. Native was right all along, which is
//! why nothing noticed: the name binds everywhere, so every name-level parity
//! check passes.
//!
//! The native surface is read from its declared return types, so a new
//! tuple-returning global has to be listed or this fails.

const LUAU: &str = include_str!("../src/scripting/bridge/adapters/luau.rs");
const RUNTIME: &str = include_str!("../../../editor/src/lib/lua-runtime.ts");

/// `(name, return type)` for each native global declared `-> LuaResult<(a, b, ..)>`.
fn native_tuple_returners() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (at, _) in LUAU.match_indices("globals.set(\"") {
        let name: String = LUAU[at + 13..].chars().take_while(|c| *c != '"').collect();
        // The closure that was registered: the nearest `create_function`
        // before this `globals.set`, and nothing past it.
        let Some(start) = LUAU[..at].rfind("create_function") else { continue };
        let block = &LUAU[start..at];
        let Some(ret) = block.find("-> LuaResult<(") else { continue };
        // Only the closure's own signature: the return type comes before its body.
        let Some(body) = block.find('{') else { continue };
        if ret > body {
            continue;
        }
        let ty: String = block[ret + 13..].chars().take_while(|c| *c != '{').collect();
        if ty.contains(',') {
            out.push((name, ty.trim().to_string()));
        }
    }
    out
}

fn multi_return_set() -> Vec<String> {
    let start = RUNTIME.find("const MULTI_RETURN").expect("MULTI_RETURN not found");
    let end = start + RUNTIME[start..].find("]);").expect("MULTI_RETURN not closed");
    RUNTIME[start..end]
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

#[test]
fn the_scan_finds_the_known_tuple_returners() {
    let found: Vec<String> = native_tuple_returners().into_iter().map(|(n, _)| n).collect();
    for name in ["get_position", "get_velocity", "get_health", "get_contact"] {
        assert!(found.iter().any(|f| f == name), "scan lost `{name}`: found {found:?}");
    }
}

#[test]
fn every_native_multi_return_is_unpacked_in_the_editor() {
    let multi = multi_return_set();
    let missing: Vec<String> = native_tuple_returners()
        .into_iter()
        .filter(|(n, _)| !multi.contains(n))
        .map(|(n, ty)| format!("{n} -> {ty}"))
        .collect();
    assert!(
        missing.is_empty(),
        "these return several values natively but one table in the editor and on the \
         web - add them to MULTI_RETURN in editor/src/lib/lua-runtime.ts:\n  {}",
        missing.join("\n  ")
    );
}

/// The other way round (Oct 2026): a name the editor unpacks must be one
/// native returns as several values. `input_get_bindings` and
/// `input_get_actions` were listed while native returns ONE table, so
/// `local keys = Input.get_bindings(a)` held the whole list natively and only
/// the first key in the editor and on the web - test-lab scene 54 showed
/// "0 actions" there, its `type(...) == "table"` guard hiding the cause.
#[test]
fn every_editor_unpack_is_a_native_tuple() {
    let native: Vec<String> = native_tuple_returners().into_iter().map(|(n, _)| n).collect();
    let extra: Vec<String> = multi_return_set().into_iter().filter(|m| !native.contains(m)).collect();
    assert!(
        extra.is_empty(),
        "MULTI_RETURN in editor/src/lib/lua-runtime.ts unpacks these into several values,          but native returns one value (a table) - remove them: {extra:?}"
    );
}

/// `get_contact` is 1-based natively (`for i = 1, contact_count()`); the
/// preamble indexed the contact array from 0, so the first contact was never
/// returned and the last index answered nil.
#[test]
fn the_browser_get_contact_is_one_based() {
    const PREAMBLE: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
    let at = PREAMBLE.find("function get_contact(i)").expect("get_contact not in the preamble");
    let body = &PREAMBLE[at..at + 400];
    assert!(
        body.contains("s.contacts[i - 1]") && !body.contains("s.contacts[i]"),
        "the preamble's get_contact must index `contacts[i - 1]`: {body}"
    );
}
