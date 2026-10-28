//! `LiveOps.*` for the browser runtimes: one synchronous hook,
//! `state.liveops(op, a, b, c)` (each argument text), over the process-wide
//! live ops in [`crate::liveops`] - the same `script_call` the Luau globals
//! call, so the two answer alike. No world, so no snapshot. Installed by
//! [`super::nav::install_nav_callbacks`], which both hosts call.

use wasm_bindgen::prelude::*;

#[cfg(feature = "liveops")]
use crate::liveops::{ScriptAnswer, script_call};

fn op(name: String, a: JsValue, b: JsValue, c: JsValue) -> JsValue {
    #[cfg(not(feature = "liveops"))]
    {
        let _ = (name, a, b, c);
        crate::features::missing("liveops", "LiveOps.*");
        return JsValue::NULL;
    }
    #[cfg(feature = "liveops")]
    let args = [a, b, c].map(|v| v.as_string().unwrap_or_default());
    #[cfg(feature = "liveops")]
    return match script_call(&name, &[args[0].as_str(), args[1].as_str(), args[2].as_str()]) {
        ScriptAnswer::Nothing => JsValue::NULL,
        ScriptAnswer::Text(s) => JsValue::from_str(&s),
        ScriptAnswer::Number(n) => JsValue::from(n),
        ScriptAnswer::Bool(b) => JsValue::from_bool(b),
        ScriptAnswer::List(names) => names.into_iter().map(JsValue::from).collect::<js_sys::Array>().into(),
        // A value as the script would have written it: a number, a string,
        // a table.
        ScriptAnswer::Json(v) => js_sys::JSON::parse(&v.to_string()).unwrap_or(JsValue::NULL),
    };
}

/// Install `liveops` on `state`.
pub fn install_liveops_callbacks(state: &JsValue) -> Result<(), JsValue> {
    let f = Closure::wrap(Box::new(op) as Box<dyn Fn(String, JsValue, JsValue, JsValue) -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("liveops"), &f.into_js_value())?;
    Ok(())
}
