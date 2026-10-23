//! `Audio.get_spectrum` / `get_level` / `synth` for the browser runtimes:
//! one synchronous hook, `state.audio_analysis(op, a)`, over the
//! process-wide ring and synth queue in [`crate::audio::analysis`] - the
//! same functions native's Luau globals call. The ring is fed by the
//! backend's `AnalyserNode` each frame, the queue drained by
//! `audio::music::drive`. Installed by [`super::nav::install_nav_callbacks`].

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
    use crate::audio::analysis as an;
    match name.as_str() {
        "spectrum" => {
            let out = js_sys::Array::new();
            for v in an::spectrum(a.as_f64().unwrap_or(16.0).max(1.0) as usize) {
                out.push(&JsValue::from_f64(v as f64));
            }
            out.into()
        }
        "level" => JsValue::from_f64(an::level() as f64),
        "synth" => {
            an::request(an::Voice::from_json(&opts(&a)));
            JsValue::NULL
        }
        other => {
            log::warn!("[audio] unknown analysis op '{other}'");
            JsValue::NULL
        }
    }
}

/// Install `audio_analysis` on `state`.
pub fn install_analysis_callbacks(state: &JsValue) -> Result<(), JsValue> {
    let f = Closure::wrap(Box::new(op) as Box<dyn Fn(String, JsValue) -> JsValue>);
    js_sys::Reflect::set(state, &JsValue::from_str("audio_analysis"), &f.into_js_value())?;
    Ok(())
}
