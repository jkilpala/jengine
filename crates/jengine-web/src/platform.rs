//! A web portal's say over the running game (`docs/web-platforms.md`).
//!
//! The adapter a web build ships for its portal (the builder's
//! `assets/platform/`) keeps two flags on `window.__jengine_platform`:
//! `paused` - the portal paused the game (YouTube's `onPause`) or an ad is
//! up - and `muted` - its audio setting, or an ad. They are read once a frame
//! rather than pushed into the wasm: a portal can speak before the wasm has
//! started, and a flag read late is still right.

use wasm_bindgen::JsValue;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct HostState {
    pub paused: bool,
    pub muted: bool,
}

/// The adapter's flags now; both false on a page without one.
pub(crate) fn read() -> HostState {
    let Some(window) = web_sys::window() else {
        return HostState::default();
    };
    let adapter = js_sys::Reflect::get(&window, &JsValue::from_str("__jengine_platform"))
        .unwrap_or(JsValue::UNDEFINED);
    if adapter.is_undefined() || adapter.is_null() {
        return HostState::default();
    }
    let flag = |key: &str| {
        js_sys::Reflect::get(&adapter, &JsValue::from_str(key))
            .map(|v| v.is_truthy())
            .unwrap_or(false)
    };
    HostState {
        paused: flag("paused"),
        muted: flag("muted"),
    }
}
