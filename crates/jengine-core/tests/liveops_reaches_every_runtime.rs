//! Live ops (`jengine_core::liveops`) are wired into every host, and every
//! `LiveOps.*` function reaches the one op they all share.
//!
//! The wiring is read from the hosts' source, as the other `*_reach_*`
//! guards do. Each gap would be silent: a host that never syncs is a game
//! that ships its defaults forever; one that sets live ops up after its
//! packs mounts them before the cached release is laid over them, and every
//! changed pack looks remote at start; a function missing from one list is
//! `nil` in one runtime.
#![cfg(feature = "liveops")]

const NATIVE: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const WEB: &str = include_str!("../../jengine-web/src/lib.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const VIEWPORT_SVELTE: &str = include_str!("../../../editor/src/lib/Viewport.svelte");
const EDITOR_SETTINGS: &str = include_str!("../../../editor/src-tauri/src/commands/project.rs");
const LUAU: &str = include_str!("../src/scripting/bridge/adapters/luau.rs");
const PREAMBLE: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
const NAV: &str = include_str!("../src/scripting/bridge/adapters/js/nav.rs");
const LUA_RUNTIME: &str = include_str!("../../../editor/src/lib/lua-runtime.ts");
const NAMESPACES: &str = include_str!("../src/scripting/namespaces.rs");
const DOCS: &str = include_str!("../../../docs/scripting-api.md");

fn body<'a>(src: &'a str, name: &str) -> &'a str {
    let start = src.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("no fn {name}"));
    let rest = &src[start..];
    let end = rest[3..].find("\nfn ").or_else(|| rest[3..].find("\nasync fn ")).or_else(|| rest[3..].find("\npub fn ")).map_or(rest.len(), |e| e + 3);
    &rest[..end]
}

fn before(src: &str, first: &str, then: &str) -> bool {
    match (src.find(first), src.find(then)) {
        (Some(a), Some(b)) => a < b,
        _ => false,
    }
}

#[test]
fn the_native_runner_believes_the_cache_before_the_packs_mount() {
    let init = body(NATIVE, "init_asset_packs");
    assert!(init.contains("liveops::native::init(&settings)"), "native never sets live ops up");
    assert!(before(init, "liveops::native::init(", "native::init(&assets_root"), "the cached release must be known before the packs mount");
    assert!(init.contains("live.as_ref()"), "the packs never hear of the cached release");
    let sync = body(NATIVE, "sync_asset_packs");
    assert!(before(sync, "liveops::native::sync()", "packs::native::sync()"), "native never asks the service (or asks after the packs sync)");
}

#[test]
fn the_web_runtime_does_too() {
    let init = body(WEB, "init_and_run");
    assert!(before(init, "liveops::web::init(", "packs::web::init("), "the web sets live ops up after its packs mount, or not at all");
    assert!(init.contains("live.as_ref()).await"));
    assert!(body(WEB, "poll_asset_packs").contains("liveops::web::sync()"), "the web never asks the service");
    for field in ["    liveops: serde_json::Value,", "    app: serde_json::Value,"] {
        assert!(WEB.contains(field), "the web's project settings drop `{}`", field.trim());
    }
}

#[test]
fn the_editor_plays_with_its_own_channel_and_keeps_the_settings() {
    let set = body(VIEWPORT, "viewport_set_liveops");
    assert!(set.contains("liveops::web::init(&settings, true)"), "the editor must ask as itself: its channel, no content");
    assert!(VIEWPORT.contains("jengine_core::liveops::web::sync();"), "the editor's Play never asks the service");
    assert!(VIEWPORT_SVELTE.contains("viewport_set_liveops(JSON.stringify(settings))"), "the editor never hands the viewport its live ops");
    assert!(EDITOR_SETTINGS.contains("pub liveops: jengine_core::liveops::LiveOpsSettings"), "a save from Project Settings would erase settings.liveops");
}

#[test]
fn every_liveops_function_is_bound_in_every_runtime() {
    assert!(NAV.contains("super::liveops::install_liveops_callbacks(&state)?;"), "the browsers never install the liveops hook");
    for (global, op) in jengine_core::liveops::SCRIPT_GLOBALS {
        assert!(LUAU.contains(&format!("globals.set(\"{global}\"")), "{global} is not bound natively");
        if op != "get" {
            assert!(LUAU.contains(&format!("liveops_fn(\"{op}\")")), "native's {global} does not ask for '{op}'");
        }
        assert!(PREAMBLE.contains(&format!("function {global}(")), "{global} is not in the browser preamble");
        assert!(PREAMBLE.contains(&format!("__liveops('{op}'")), "{global} does not ask the hook for '{op}'");
        assert!(LUA_RUNTIME.contains(&format!("'{global}'")), "{global} is not in BRIDGE_FUNCTIONS - nil in the editor and web");
        let short = global.trim_start_matches("liveops_");
        assert!(NAMESPACES.contains(&format!("(\"LiveOps\", \"{short}\", \"{global}\")")), "{global} has no LiveOps namespace entry");
        assert!(DOCS.contains(&format!("LiveOps.{short}(")), "LiveOps.{short} is not documented");
    }
}

#[test]
fn every_op_answers() {
    for (global, op) in jengine_core::liveops::SCRIPT_GLOBALS {
        if matches!(op, "get" | "fetch" | "set_player" | "set_attribute") {
            continue;
        }
        assert_ne!(jengine_core::liveops::script_op(op, "x"), jengine_core::liveops::ScriptAnswer::Nothing, "{global} answers nothing");
    }
}

const CORE_NATIVE: &str = include_str!("../src/liveops/native.rs");
const CORE_WEB: &str = include_str!("../src/liveops/web.rs");
const RUNTIME_LIB: &str = include_str!("../../jengine-runtime/src/lib.rs");

/// L13: an ask says the player's language (read again when it changes),
/// and `LiveOps.expose` reaches the service - both from the per-frame sync
/// every host already calls, so a sync that skips either is a runtime whose
/// rules on language never match, or whose results stay empty.
#[test]
fn every_sync_says_the_language_and_sends_exposures() {
    for (name, src) in [("native", CORE_NATIVE), ("web", CORE_WEB)] {
        let sync = body(src, "sync");
        assert!(sync.contains("l.follow_locale()"), "{name}'s sync never reads the language");
        assert!(sync.contains("l.take_exposures()"), "{name}'s sync never sends exposures");
    }
    assert!(body(CORE_WEB, "post_json").contains("\"POST\""), "the web posts exposures some other way");
    assert!(RUNTIME_LIB.contains("packs::native::set_http_post("), "the native runtime has no POST, so no exposure is ever sent");
}
