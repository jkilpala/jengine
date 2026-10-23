//! `Scene.load_additive` / `unload` / `preload` / `status` / `progress` for
//! the browser runtimes: one synchronous hook, `state.streaming(op, a, b,
//! c)`, over the process-wide requests in [`crate::scene::streaming`]; the
//! hosts load what is asked. No world, so no snapshot. Installed by
//! [`super::nav::install_nav_callbacks`], which both hosts call.

use wasm_bindgen::prelude::*;

use crate::scene::streaming as st;

fn status_of(v: &JsValue) -> st::Status {
    match (v.as_string(), v.as_f64()) {
        (Some(path), _) => st::preload_status(&path),
        (None, Some(n)) => st::status(n.max(0.0) as u32),
        _ => st::Status::Unloaded,
    }
}

fn op(name: String, a: JsValue, b: JsValue, c: JsValue) -> JsValue {
    match name.as_str() {
        "load_additive" => {
            let path = a.as_string().unwrap_or_default();
            let h = st::load_additive(&path, b.as_f64().unwrap_or(0.0) as f32, c.as_f64().unwrap_or(0.0) as f32);
            return JsValue::from(h);
        }
        "unload" => st::unload(a.as_f64().unwrap_or(0.0).max(0.0) as u32),
        "preload" => st::preload(&a.as_string().unwrap_or_default()),
        "status" => return JsValue::from_str(status_of(&a).name()),
        "progress" => return JsValue::from(status_of(&a).progress()),
        other => log::warn!("[stream] unknown op '{other}'"),
    }
    JsValue::NULL
}

/// Install `streaming` on `state`.
pub fn install_streaming_callbacks(state: &JsValue) -> Result<(), JsValue> {
    let f = Closure::wrap(Box::new(op) as Box<dyn Fn(String, JsValue, JsValue, JsValue) -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("streaming"), &f.into_js_value())?;
    // `Scene.transition`'s first half (`renderer::transition::arm`); the
    // preamble's `load_scene` is the second.
    let f = Closure::wrap(Box::new(|style: String, seconds: f64, texture: String| {
        crate::renderer::transition::arm(&style, seconds as f32, &texture);
    }) as Box<dyn Fn(String, f64, String)>);
    js_sys::Reflect::set(state, &JsValue::from_str("scene_transition_arm"), &f.into_js_value())?;
    Ok(())
}
