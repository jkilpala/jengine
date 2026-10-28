//! `Packs.download` / `remove` / `set_url` / `status` / `progress` / `size` /
//! `error` / `of` / `list` for the browser runtimes: one synchronous hook,
//! `state.packs(op, arg)`, over the process-wide packs in
//! [`crate::asset::packs`] - the same `script_op` the Luau globals call, so
//! the two answer alike. No world, so no snapshot. Installed by
//! [`super::nav::install_nav_callbacks`], which both hosts call.

use wasm_bindgen::prelude::*;

#[cfg(feature = "packs")]
use crate::asset::packs::{ScriptAnswer, script_op};

fn op(name: String, arg: JsValue) -> JsValue {
    #[cfg(not(feature = "packs"))]
    {
        let _ = (name, arg);
        crate::features::missing("packs", "Packs.*");
        return JsValue::NULL;
    }
    #[cfg(feature = "packs")]
    return match script_op(&name, &arg.as_string().unwrap_or_default()) {
        ScriptAnswer::Nothing => JsValue::NULL,
        ScriptAnswer::Text(s) => JsValue::from_str(&s),
        ScriptAnswer::Number(n) => JsValue::from(n),
        ScriptAnswer::List(names) => names.into_iter().map(JsValue::from).collect::<js_sys::Array>().into(),
    };
}

/// Install `packs` on `state`.
pub fn install_pack_callbacks(state: &JsValue) -> Result<(), JsValue> {
    let f = Closure::wrap(Box::new(op) as Box<dyn Fn(String, JsValue) -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("packs"), &f.into_js_value())?;
    Ok(())
}
