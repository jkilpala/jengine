//! `Accessibility.announce` / `speech` for the browser runtimes: two
//! synchronous hooks, `state.accessibility_announce(text)` and
//! `state.accessibility_speech()`, over [`crate::ui::accessibility`] - the
//! functions native's Luau globals call. The tree and its live region
//! are kept by `ui::accessibility::web`. Installed by
//! [`super::nav::install_nav_callbacks`].

use wasm_bindgen::prelude::*;

/// Install `accessibility_announce` and `accessibility_speech` on `state`.
pub fn install_accessibility_callbacks(state: &JsValue) -> Result<(), JsValue> {
    let f = Closure::wrap(Box::new(|text: JsValue| {
        let text = text.as_string().unwrap_or_default();
        let _ = &text;
        crate::feature!("accessibility", "Accessibility.announce", { crate::ui::accessibility::announce(&text) });
    }) as Box<dyn Fn(JsValue)>);
    js_sys::Reflect::set(state, &JsValue::from_str("accessibility_announce"), &f.into_js_value())?;
    let f = Closure::wrap(Box::new(|| {
        #[cfg(feature = "accessibility")]
        return JsValue::from_str(&crate::ui::accessibility::focused_speech());
        #[cfg(not(feature = "accessibility"))]
        {
            crate::features::missing("accessibility", "Accessibility.speech");
            JsValue::from_str("")
        }
    }) as Box<dyn Fn() -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("accessibility_speech"), &f.into_js_value())?;
    Ok(())
}
