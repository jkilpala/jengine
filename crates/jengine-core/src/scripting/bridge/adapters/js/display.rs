//! `Display.*` / `Cursor.*` for the browser runtimes: one synchronous hook,
//! `state.display(op, a, b, c)`, over the process-wide settings in
//! [`crate::display`] - the setters write what is wanted, the getters read
//! what the host last published (`display::web::WebDisplay::sync`). No
//! world, so no snapshot. Installed by [`super::nav::install_nav_callbacks`],
//! which both hosts call.

use wasm_bindgen::prelude::*;

fn num(v: &JsValue, or: f64) -> f64 {
    v.as_f64().unwrap_or(or)
}

fn truthy(v: &JsValue) -> bool {
    v.is_undefined() || v.is_null() || v.as_bool().unwrap_or(true)
}

fn op(name: String, a: JsValue, b: JsValue, c: JsValue) -> JsValue {
    use crate::display as d;
    match name.as_str() {
        "set_mode" => d::set_mode(&a.as_string().unwrap_or_default()),
        "get_mode" => return JsValue::from_str(d::info().mode.name()),
        "set_size" => d::set_size(num(&a, 0.0).max(0.0) as u32, num(&b, 0.0).max(0.0) as u32),
        "get_size" => {
            let i = d::info();
            let out = js_sys::Array::new();
            out.push(&JsValue::from(i.width));
            out.push(&JsValue::from(i.height));
            return out.into();
        }
        "get_resolutions" => {
            let out = js_sys::Array::new();
            for (w, h, hz) in d::info().modes {
                let m = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&m, &"w".into(), &JsValue::from(w));
                let _ = js_sys::Reflect::set(&m, &"h".into(), &JsValue::from(h));
                let _ = js_sys::Reflect::set(&m, &"hz".into(), &JsValue::from(hz));
                out.push(&m);
            }
            return out.into();
        }
        "set_vsync" => d::set_vsync(truthy(&a)),
        "get_vsync" => return JsValue::from(d::info().vsync),
        "set_max_fps" => d::set_max_fps(if a.is_undefined() || a.is_null() { -1 } else { num(&a, -1.0).round() as i64 }),
        "get_max_fps" => return JsValue::from(d::info().max_fps),
        "supports" => return JsValue::from(d::info().supports.get(&a.as_string().unwrap_or_default())),
        "cursor_visible" => d::set_cursor_visible(truthy(&a)),
        "cursor_confined" => d::set_cursor_confined(truthy(&a)),
        "cursor_locked" => d::set_cursor_locked(truthy(&a)),
        "cursor_icon" => d::set_cursor_icon(&a.as_string().unwrap_or_else(|| "default".into())),
        "cursor_image" => d::set_cursor_image(&a.as_string().unwrap_or_default(), num(&b, 0.0) as f32, num(&c, 0.0) as f32),
        other => log::warn!("[display] unknown op '{other}'"),
    }
    JsValue::NULL
}

/// Install `display` on `state`.
pub fn install_display_callbacks(state: &JsValue) -> Result<(), JsValue> {
    let f = Closure::wrap(Box::new(op) as Box<dyn Fn(String, JsValue, JsValue, JsValue) -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("display"), &f.into_js_value())?;
    Ok(())
}
