//! Fengari (real Lua 5.3 VM in JS) compile path — `wasm32` only.
//!
//! When the editor frontend has loaded `fengari-web` and installed
//! `globalThis.__jengine_lua_compile`, Lua sources can be executed by an
//! actual Lua VM instead of going through the brittle string-based
//! Lua→JS [`super::transpiler`]. This is essential for editor↔native parity.
//!
//! The JS shim returns the same `{init, update, on_event, on_gui_click, lookup}`
//! shape as [`super::browser::compile`], so [`BrowserScriptFns`] is unchanged.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use super::super::super::diagnostics::{BackendKind, ScriptError, ScriptErrorKind};
use super::browser::{self, BrowserScriptFns};

/// Returns the JS function `globalThis.__jengine_lua_compile` if installed.
fn fengari_compile_fn() -> Option<js_sys::Function> {
    let win = web_sys::window()?;
    let v = js_sys::Reflect::get(&win, &JsValue::from_str("__jengine_lua_compile")).ok()?;
    if v.is_function() {
        Some(v.unchecked_into())
    } else {
        None
    }
}

/// Idempotently expose the bridge `lookup` JS function (which can resolve any
/// API function by name from the wrapper IIFE scope) on
/// `window.__jengine_play.__api_lookup` so the Fengari shim can wire up Lua
/// globals without duplicating the 130-function bridge surface.
fn ensure_api_lookup_installed() -> Result<(), JsValue> {
    let win = web_sys::window().ok_or(JsValue::NULL)?;
    let key = JsValue::from_str("__jengine_play");
    let play_val = js_sys::Reflect::get(&win, &key)?;
    // Auto-create the namespace if the host hasn't seeded it yet. Lua scripts
    // compile *before* the viewport's `sync_world_to_play_namespace` runs, so
    // requiring the namespace up-front would deadlock the very first compile
    // of every play session. A bare empty object is sufficient — the viewport
    // will populate fields onto it lazily.
    let play: JsValue = if play_val.is_object() {
        play_val
    } else {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&win, &key, &obj)?;
        obj.into()
    };
    let existing = js_sys::Reflect::get(&play, &JsValue::from_str("__api_lookup"))?;
    if existing.is_function() {
        return Ok(());
    }
    // Compile a stub wrapper with empty user source — its `ns_lookup` closure
    // captures every API value declared in the IIFE scope (functions AND namespace
    // objects like `event`, `animation`, etc.) so the Fengari bridge can populate
    // both bare globals and Lua namespace tables.
    let stub = browser::compile("__api_stub", "")
        .map_err(|e| JsValue::from_str(&format!("api_stub compile: {}", e.message)))?;
    js_sys::Reflect::set(&play, &JsValue::from_str("__api_lookup"), &stub.ns_lookup)?;
    Ok(())
}

/// Try to compile `lua_source` via the Fengari shim. Returns:
/// - `None` — Fengari shim not installed; caller should fall back to transpiler.
/// - `Some(Ok(_))` — compiled successfully.
/// - `Some(Err(_))` — Fengari is installed but the script failed to compile.
pub(super) fn try_compile(
    path: &str,
    lua_source: &str,
) -> Option<Result<BrowserScriptFns, ScriptError>> {
    let func = fengari_compile_fn()?;
    if let Err(e) = ensure_api_lookup_installed() {
        return Some(Err(make_error(
            path,
            &format!("ensure_api_lookup_installed: {:?}", e),
        )));
    }
    let label = JsValue::from_str(path);
    let src = JsValue::from_str(lua_source);
    let result = match func.call2(&JsValue::NULL, &label, &src) {
        Ok(v) => v,
        Err(e) => return Some(Err(js_error(path, &e))),
    };
    let obj: js_sys::Object = match result.dyn_into() {
        Ok(o) => o,
        Err(_) => {
            return Some(Err(make_error(
                path,
                "__jengine_lua_compile did not return an object",
            )))
        }
    };
    let init = match js_sys::Reflect::get(&obj, &JsValue::from_str("init")) {
        Ok(v) => v,
        Err(e) => return Some(Err(js_error(path, &e))),
    };
    let update = match js_sys::Reflect::get(&obj, &JsValue::from_str("update")) {
        Ok(v) => v,
        Err(e) => return Some(Err(js_error(path, &e))),
    };
    let on_event = match js_sys::Reflect::get(&obj, &JsValue::from_str("on_event")) {
        Ok(v) => v,
        Err(e) => return Some(Err(js_error(path, &e))),
    };
    let on_gui_click = match js_sys::Reflect::get(&obj, &JsValue::from_str("on_gui_click")) {
        Ok(v) => v,
        Err(e) => return Some(Err(js_error(path, &e))),
    };
    let lookup = match js_sys::Reflect::get(&obj, &JsValue::from_str("lookup")) {
        Ok(v) => v,
        Err(e) => return Some(Err(js_error(path, &e))),
    };
    Some(Ok(BrowserScriptFns {
        init,
        update,
        on_event,
        on_gui_click,
        lookup,
        // Fengari user scripts don't need ns_lookup (only the API stub does).
        ns_lookup: JsValue::NULL,
        // Fengari runs the user source unmodified — no wrapper prefix.
        prefix_lines: 0,
    }))
}

fn make_error(path: &str, message: &str) -> ScriptError {
    ScriptError {
        backend: BackendKind::JavaScript,
        script_path: path.to_string(),
        location: None,
        kind: ScriptErrorKind::Runtime,
        message: message.to_string(),
        bridge_call: None,
    }
}

fn js_error(path: &str, e: &JsValue) -> ScriptError {
    let msg = e
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(e, &JsValue::from_str("message"))
                .ok()
                .and_then(|v| v.as_string())
        })
        .unwrap_or_else(|| format!("{:?}", e));
    ScriptError {
        backend: BackendKind::JavaScript,
        script_path: path.to_string(),
        location: None,
        kind: ScriptErrorKind::Runtime,
        message: msg,
        bridge_call: None,
    }
}
