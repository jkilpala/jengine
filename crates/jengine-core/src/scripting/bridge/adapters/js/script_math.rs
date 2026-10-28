//! A script's maths for the browser runtimes: [`crate::scripting::script_math`]
//! published as `globalThis.__jengine_script_math`, so Fengari computes the
//! same bits the native Luau does.
//!
//! The editor's Lua runtime (`editor/src/lib/lua-runtime.ts`) puts these in
//! each script's `math` table, and the vendored Fengari's `^` and `%`
//! (`editor/vendor/fengari`) call `pow` and `mod` - a global rather than a
//! field of `__jengine_play.state`, because the operators are reached from
//! inside Fengari with nothing but the numbers. Both read it when they run,
//! not when a script compiles, so the order hosts install things in does
//! not matter; with nothing installed (Fengari under a unit test) they fall
//! back to JavaScript's `Math`. Installed by [`super::nav::install_nav_callbacks`].

use wasm_bindgen::prelude::*;

use crate::scripting::script_math as m;

fn set1(obj: &js_sys::Object, name: &str, f: fn(f64) -> f64) -> Result<(), JsValue> {
    let c = Closure::wrap(Box::new(f) as Box<dyn Fn(f64) -> f64>);
    js_sys::Reflect::set(obj, &JsValue::from_str(name), &c.into_js_value()).map(|_| ())
}

fn set2(obj: &js_sys::Object, name: &str, f: fn(f64, f64) -> f64) -> Result<(), JsValue> {
    let c = Closure::wrap(Box::new(f) as Box<dyn Fn(f64, f64) -> f64>);
    js_sys::Reflect::set(obj, &JsValue::from_str(name), &c.into_js_value()).map(|_| ())
}

/// Install `globalThis.__jengine_script_math`. Once per page is enough;
/// installing again replaces it with the same functions.
pub fn install_script_math() -> Result<(), JsValue> {
    let obj = js_sys::Object::new();
    for (name, f) in [
        ("sin", m::sin as fn(f64) -> f64),
        ("cos", m::cos),
        ("tan", m::tan),
        ("asin", m::asin),
        ("acos", m::acos),
        ("atan", m::atan),
        ("exp", m::exp),
        ("log10", m::log10),
        ("sinh", m::sinh),
        ("cosh", m::cosh),
        ("tanh", m::tanh),
    ] {
        set1(&obj, name, f)?;
    }
    set2(&obj, "atan2", m::atan2)?;
    set2(&obj, "pow", m::pow)?;
    set2(&obj, "mod", m::modulo)?;
    // `math.log(x [, base])`: a missing base arrives as `undefined`.
    let log = Closure::wrap(Box::new(|x: f64, base: Option<f64>| m::log(x, base)) as Box<dyn Fn(f64, Option<f64>) -> f64>);
    js_sys::Reflect::set(&obj, &JsValue::from_str("log"), &log.into_js_value())?;
    // `math.random([m [, n]])`; NaN for an empty interval, which the Lua
    // runtime raises as the error both VMs raise.
    let random = Closure::wrap(Box::new(|a: Option<f64>, b: Option<f64>| m::random(a, b).unwrap_or(f64::NAN))
        as Box<dyn Fn(Option<f64>, Option<f64>) -> f64>);
    js_sys::Reflect::set(&obj, &JsValue::from_str("random"), &random.into_js_value())?;
    let seed = Closure::wrap(Box::new(m::seed) as Box<dyn Fn(f64)>);
    js_sys::Reflect::set(&obj, &JsValue::from_str("randomseed"), &seed.into_js_value())?;
    js_sys::Reflect::set(&js_sys::global(), &JsValue::from_str("__jengine_script_math"), &obj)?;
    Ok(())
}
