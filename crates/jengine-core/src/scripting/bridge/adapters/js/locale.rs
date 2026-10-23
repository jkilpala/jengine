//! `Locale.*` for the browser runtimes: synchronous hooks on
//! `__jengine_play.state`, answered by the one implementation in
//! [`crate::locale`]. The language and the string tables are process-wide
//! (`data::global`), not part of the world, so these need no snapshot and
//! cannot meet the host's world borrow. Installed by
//! [`super::nav::install_nav_callbacks`], which both hosts already call.

use wasm_bindgen::prelude::*;

fn args_json(args: &JsValue) -> Option<serde_json::Value> {
    if args.is_undefined() || args.is_null() {
        return None;
    }
    let text = js_sys::JSON::stringify(args).ok()?.as_string()?;
    serde_json::from_str(&text).ok()
}

/// Install `locale_get` / `locale_set` / `locale_current` / `locale_list` /
/// `locale_name` on `state`.
pub fn install_locale_callbacks(state: &JsValue) -> Result<(), JsValue> {
    let f = Closure::wrap(Box::new(|key: JsValue, args: JsValue| {
        let key = key.as_string().unwrap_or_default();
        JsValue::from_str(&crate::locale::translate(&key, args_json(&args).as_ref()))
    }) as Box<dyn Fn(JsValue, JsValue) -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("locale_get"), &f.into_js_value())?;

    let f = Closure::wrap(Box::new(|code: JsValue| {
        crate::locale::set_locale(&code.as_string().unwrap_or_default());
    }) as Box<dyn Fn(JsValue)>);
    js_sys::Reflect::set(state, &JsValue::from_str("locale_set"), &f.into_js_value())?;

    let f = Closure::wrap(Box::new(|| JsValue::from_str(&crate::locale::current())) as Box<dyn Fn() -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("locale_current"), &f.into_js_value())?;

    let f = Closure::wrap(Box::new(|| {
        let out = js_sys::Array::new();
        for code in crate::locale::available() {
            out.push(&JsValue::from_str(&code));
        }
        JsValue::from(out)
    }) as Box<dyn Fn() -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("locale_list"), &f.into_js_value())?;

    let f = Closure::wrap(Box::new(|code: JsValue| {
        JsValue::from_str(&crate::locale::language_name(&code.as_string().unwrap_or_default()))
    }) as Box<dyn Fn(JsValue) -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("locale_name"), &f.into_js_value())?;
    Ok(())
}
