//! `Input.rumble` / `Input.vibrate` for the browser runtimes: synchronous
//! hooks on `__jengine_play.state` that queue the request in
//! [`crate::input::haptics`], where `gamepad_web::poll_gamepads` plays it.
//! `Audio.set_variation` rides along: it writes the process-wide
//! [`crate::audio::variation`] table the same way.
//! The queue is the process's, not the world's, so these need no snapshot.
//! Installed by [`super::nav::install_nav_callbacks`], which both hosts call.

use wasm_bindgen::prelude::*;

/// Install `input_rumble` and `input_vibrate` on `state`.
pub fn install_haptics_callbacks(state: &JsValue) -> Result<(), JsValue> {
    let f = Closure::wrap(Box::new(|pad: f64, strong: f64, weak: f64, seconds: f64| {
        crate::input::haptics::rumble(pad.max(0.0) as usize, strong as f32, weak as f32, seconds as f32);
    }) as Box<dyn Fn(f64, f64, f64, f64)>);
    js_sys::Reflect::set(state, &JsValue::from_str("input_rumble"), &f.into_js_value())?;
    let f = Closure::wrap(Box::new(|seconds: f64, strength: f64| {
        crate::input::haptics::vibrate(seconds as f32, strength as f32);
    }) as Box<dyn Fn(f64, f64)>);
    js_sys::Reflect::set(state, &JsValue::from_str("input_vibrate"), &f.into_js_value())?;
    // `Audio.set_variation`: the process-wide per-clip table, likewise.
    let f = Closure::wrap(Box::new(|clip: String, pitch: f64, volume: f64| {
        crate::audio::variation::set(&clip, pitch as f32, volume as f32);
    }) as Box<dyn Fn(String, f64, f64)>);
    js_sys::Reflect::set(state, &JsValue::from_str("audio_set_variation"), &f.into_js_value())?;
    // `Audio.follow_time` and `Time.get_scale`: the process-wide time scale.
    let f = Closure::wrap(Box::new(|on: bool| {
        crate::time::set_audio_follows_time(on);
    }) as Box<dyn Fn(bool)>);
    js_sys::Reflect::set(state, &JsValue::from_str("audio_follow_time"), &f.into_js_value())?;
    let f = Closure::wrap(Box::new(|| crate::time::world_scale() as f64) as Box<dyn Fn() -> f64>);
    js_sys::Reflect::set(state, &JsValue::from_str("time_get_scale"), &f.into_js_value())?;
    // `State.*`: save states are requests the physics frame answers
    // (`crate::ecs::save_state`), so these only queue them.
    let f = Closure::wrap(Box::new(|slot: String| {
        let _ = &slot;
        crate::feature!("save_state", "State.save", { crate::ecs::save_state::request_save(&slot) })
    }) as Box<dyn Fn(String)>);
    js_sys::Reflect::set(state, &JsValue::from_str("state_save"), &f.into_js_value())?;
    let f = Closure::wrap(Box::new(|slot: String| {
        let _ = &slot;
        crate::feature!("save_state", "State.load", { crate::ecs::save_state::request_load(&slot) })
    }) as Box<dyn Fn(String)>);
    js_sys::Reflect::set(state, &JsValue::from_str("state_load"), &f.into_js_value())?;
    let f = Closure::wrap(Box::new(|slot: String| {
        #[cfg(feature = "save_state")]
        return crate::ecs::save_state::has(&slot);
        #[cfg(not(feature = "save_state"))]
        {
            let _ = slot;
            crate::features::missing("save_state", "State.has");
            false
        }
    }) as Box<dyn Fn(String) -> bool>);
    js_sys::Reflect::set(state, &JsValue::from_str("state_has"), &f.into_js_value())?;
    let f = Closure::wrap(Box::new(|slot: String| {
        let _ = &slot;
        crate::feature!("save_state", "State.forget", { crate::ecs::save_state::request_forget(&slot) })
    }) as Box<dyn Fn(String)>);
    js_sys::Reflect::set(state, &JsValue::from_str("state_forget"), &f.into_js_value())?;
    // `Replay.*`: the process-wide input replay (`crate::input::replay`),
    // one hook taking the op's name and a key name.
    let f = Closure::wrap(Box::new(|op: String, arg: String| match crate::input::replay::script_op(&op, &arg) {
        serde_json::Value::Bool(b) => JsValue::from(b),
        serde_json::Value::Number(n) => JsValue::from(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => JsValue::from_str(&s),
        _ => JsValue::NULL,
    }) as Box<dyn Fn(String, String) -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("replay"), &f.into_js_value())?;
    Ok(())
}
