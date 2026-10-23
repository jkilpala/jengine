//! `Input.is_action_held` / `is_action_tapped` / `is_action_double_tapped` /
//! `get_hold_time`, the pads' names and families, the device used last and
//! button prompts, for the browser runtimes: one synchronous hook,
//! `state.input_query(op, a, b)`, answered by the same
//! [`crate::input::interactions::InputView`] native reads from its snapshot.
//! [`super::input_mirror::publish`] leaves the frame's view here; installed
//! by [`super::nav::install_nav_callbacks`], which both hosts call.

use std::cell::RefCell;

use wasm_bindgen::prelude::*;

use crate::input::interactions::InputView;

thread_local! {
    static VIEW: RefCell<InputView> = RefCell::new(InputView::default());
}

/// This frame's view, for the hook to answer from.
pub fn set_view(view: InputView) {
    VIEW.with(|v| *v.borrow_mut() = view);
}

fn secs(v: &JsValue) -> Option<f32> {
    v.as_f64().map(|s| s as f32)
}

fn pad(v: &JsValue) -> usize {
    v.as_f64().unwrap_or(0.0).max(0.0) as usize
}

fn op(name: String, a: JsValue, b: JsValue) -> JsValue {
    VIEW.with(|v| {
        let v = v.borrow();
        let action = a.as_string().unwrap_or_default();
        match name.as_str() {
            "hold_time" => JsValue::from_f64(v.hold_time(&action) as f64),
            "held" => JsValue::from_bool(v.held(&action, secs(&b))),
            "tapped" => JsValue::from_bool(v.tapped(&action, secs(&b))),
            "double_tapped" => JsValue::from_bool(v.double_tapped(&action, secs(&b))),
            "pad_name" => JsValue::from_str(v.pad_name(pad(&a))),
            "pad_type" => JsValue::from_str(v.pad_type(pad(&a))),
            "last_device" => JsValue::from_str(v.last_device.name()),
            "last_pad" => JsValue::from_f64(v.last_pad.map(|p| p as f64).unwrap_or(-1.0)),
            "prompt" => {
                let p = v.prompt(&action, b.as_string().as_deref());
                let o = js_sys::Object::new();
                for (k, val) in [("label", p.label.as_str()), ("glyph", &p.glyph), ("binding", &p.binding), ("style", p.style)] {
                    let _ = js_sys::Reflect::set(&o, &JsValue::from_str(k), &JsValue::from_str(val));
                }
                o.into()
            }
            other => {
                log::warn!("[input] unknown query '{other}'");
                JsValue::NULL
            }
        }
    })
}

/// Install `input_query` on `state`.
pub fn install_input_query_callbacks(state: &JsValue) -> Result<(), JsValue> {
    let f = Closure::wrap(Box::new(op) as Box<dyn Fn(String, JsValue, JsValue) -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("input_query"), &f.into_js_value())?;
    Ok(())
}
