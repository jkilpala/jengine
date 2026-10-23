//! `Audio.crossfade_music` / `play_music_layers` / `set_music_layer` /
//! `set_playlist` / `next_track` / `set_music_pitch` / `seek_music` /
//! `set_sound_pitch` / `get_music_clip` / `get_music_layer` for the browser
//! runtimes: one synchronous hook, `state.music(op, args)`, answered by the
//! same [`crate::audio::music::script_op`] the Luau globals call. The music
//! director is the process's, so no snapshot. Installed by
//! [`super::nav::install_nav_callbacks`], which both hosts call.

use wasm_bindgen::prelude::*;

fn op(name: String, args: JsValue) -> JsValue {
    let args: Vec<serde_json::Value> = js_sys::JSON::stringify(&args)
        .ok()
        .and_then(|t| t.as_string())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    match crate::audio::music::script_op(&name, &args) {
        serde_json::Value::String(s) => JsValue::from_str(&s),
        serde_json::Value::Number(n) => JsValue::from_f64(n.as_f64().unwrap_or(0.0)),
        _ => JsValue::NULL,
    }
}

/// Install `music` on `state`.
pub fn install_music_callbacks(state: &JsValue) -> Result<(), JsValue> {
    let f = Closure::wrap(Box::new(op) as Box<dyn Fn(String, JsValue) -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("music"), &f.into_js_value())?;
    Ok(())
}
