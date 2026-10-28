//! `Noise.*` for the browser runtimes: one synchronous hook,
//! `state.noise(op, a, b, c, d, e, f)`, answered by [`crate::noise`] - the
//! functions the native Luau globals call, so a seed draws the same island
//! in every runtime. Pure, so no snapshot and no world borrow. Installed by
//! [`super::nav::install_nav_callbacks`], which both hosts call.

use wasm_bindgen::prelude::*;

#[cfg(feature = "noise")]
fn num(v: &JsValue, or: f64) -> f64 {
    v.as_f64().unwrap_or(or)
}

#[cfg(feature = "noise")]
fn seed(v: &JsValue) -> u32 {
    v.as_f64().map(crate::noise::seed_from).unwrap_or(0)
}

#[cfg(feature = "noise")]
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

#[cfg(feature = "noise")]
fn numbers(values: impl IntoIterator<Item = f64>) -> JsValue {
    let out = js_sys::Array::new();
    for v in values {
        out.push(&JsValue::from_f64(v));
    }
    out.into()
}

#[cfg(feature = "noise")]
#[allow(clippy::too_many_arguments)]
fn op(name: String, a: JsValue, b: JsValue, c: JsValue, d: JsValue, e: JsValue, f: JsValue) -> JsValue {
    use crate::noise as n;
    let (x, y) = (num(&a, 0.0), num(&b, 0.0));
    match name.as_str() {
        "perlin" => JsValue::from_f64(n::perlin(x, y, seed(&c))),
        "perlin3" => JsValue::from_f64(n::perlin3(x, y, num(&c, 0.0), seed(&d))),
        "simplex" => JsValue::from_f64(n::simplex(x, y, seed(&c))),
        "cellular" => {
            let cell = n::cellular(x, y, seed(&c));
            numbers([cell.f1, cell.f2, cell.value])
        }
        "fractal" => JsValue::from_f64(n::Fractal::from_json(&opts(&c)).sample(x, y)),
        "grid" => numbers(n::script_grid(x, y, num(&c, 0.0), num(&d, 0.0), num(&e, 1.0), &opts(&f))),
        other => {
            log::warn!("[noise] unknown op '{other}'");
            JsValue::NULL
        }
    }
}

/// `Noise.*` in a build without noise: logs once, answers null.
#[cfg(not(feature = "noise"))]
fn op(_name: String, _a: JsValue, _b: JsValue, _c: JsValue, _d: JsValue, _e: JsValue, _f: JsValue) -> JsValue {
    crate::features::missing("noise", "Noise.*");
    JsValue::NULL
}

/// Install `noise` on `state`.
pub fn install_noise_callbacks(state: &JsValue) -> Result<(), JsValue> {
    let f = Closure::wrap(
        Box::new(op) as Box<dyn Fn(String, JsValue, JsValue, JsValue, JsValue, JsValue, JsValue) -> JsValue>
    );
    js_sys::Reflect::set(state, &JsValue::from_str("noise"), &f.into_js_value())?;
    Ok(())
}
