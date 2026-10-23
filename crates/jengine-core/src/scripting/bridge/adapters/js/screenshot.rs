//! `Screenshot.take` / `ready` for the browser runtimes: one synchronous
//! hook, `state.screenshot(op, a)`, over the request queue in
//! [`crate::renderer::screenshot`] - the functions native's Luau globals
//! call. The copy itself is made as the frame is presented
//! (`WgpuFrame::present`). Installed by [`super::nav::install_nav_callbacks`].

use wasm_bindgen::prelude::*;

fn opts(v: &JsValue) -> serde_json::Value {
    if v.is_undefined() || v.is_null() {
        return serde_json::Value::Null;
    }
    js_sys::JSON::stringify(v)
        .ok()
        .and_then(|s| s.as_string())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn op(name: String, a: JsValue) -> JsValue {
    use crate::renderer::screenshot as shot;
    match name.as_str() {
        "take" => JsValue::from_f64(shot::request(&opts(&a)) as f64),
        "ready" => JsValue::from_bool(shot::is_done(a.as_f64().unwrap_or(0.0) as u64)),
        other => {
            log::warn!("[screenshot] unknown op '{other}'");
            JsValue::NULL
        }
    }
}

/// Install `screenshot` on `state`.
pub fn install_screenshot_callbacks(state: &JsValue) -> Result<(), JsValue> {
    let f = Closure::wrap(Box::new(op) as Box<dyn Fn(String, JsValue) -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("screenshot"), &f.into_js_value())?;
    Ok(())
}
