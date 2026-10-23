//! The synchronous `raycast` callback the browser runtimes install.
//!
//! The sibling of [`super::nav`], and for exactly the same reason: Lua has
//! no await, so `raycast` has to *answer*, and the hosts own their world in
//! an `Rc<RefCell<…>>` with scripts running inside a `borrow_mut()` — so a
//! callback that reached back for the app would panic on a double borrow.
//! The callback owns its own data instead.
//!
//! That data is [`crate::physics::ray::RaySnapshot`], refreshed once a
//! frame before scripts run. It builds its shapes with the same function
//! `add_collider` uses and intersects with parry's own `RayCast`, so it
//! agrees with the simulation rather than approximating it — which is the
//! whole reason `raycast` sat unimplemented rather than being hacked in.
//!
//! The preamble already calls this: `raycast` in `browser.rs` reads
//! `__jengine_play.state.physics_raycast` and returns `null` when it is
//! absent. It has been absent since the function was specified. Installing
//! it here is what finally makes the call return something, in the editor
//! and in web builds, and it is the last of the four runtimes.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use crate::physics::ray::RaySnapshot;

/// Install `__jengine_play.state.physics_raycast`.
///
/// Call once when the play namespace is built, after seeding the snapshot,
/// so a `raycast` from a script's `init()` already has geometry to hit.
pub fn install_ray_callback(snapshot: Rc<RefCell<RaySnapshot>>) -> Result<(), JsValue> {
    let win = web_sys::window().ok_or(JsValue::NULL)?;
    let play_key = JsValue::from_str("__jengine_play");
    let play = js_sys::Reflect::get(&win, &play_key)?;
    let play: JsValue = if play.is_object() {
        play
    } else {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&win, &play_key, &obj)?;
        obj.into()
    };

    // The preamble reads every synchronous host hook off `state`.
    let state_key = JsValue::from_str("state");
    let state = js_sys::Reflect::get(&play, &state_key)?;
    let state: JsValue = if state.is_object() {
        state
    } else {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&play, &state_key, &obj)?;
        obj.into()
    };

    let snap = Rc::clone(&snapshot);
    let f = Closure::wrap(Box::new(
        move |ox: f64, oy: f64, dx: f64, dy: f64, max_dist: f64| -> JsValue {
            let snap = snap.borrow();
            let Some(hit) =
                snap.cast(ox as f32, oy as f32, dx as f32, dy as f32, max_dist as f32)
            else {
                // A miss is `null`, which is what the preamble has always
                // returned when the hook was missing - so a script written
                // against the old behaviour still reads correctly.
                return JsValue::NULL;
            };
            // The field names are `RayHit`'s own, so a script reads the same
            // table in every runtime. Native's Luau binding sets exactly
            // these six.
            let obj = js_sys::Object::new();
            let set = |k: &str, v: f64| {
                let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), &JsValue::from_f64(v));
            };
            set("entity_id", hit.entity_id as f64);
            set("x", hit.point_x as f64);
            set("y", hit.point_y as f64);
            set("normal_x", hit.normal_x as f64);
            set("normal_y", hit.normal_y as f64);
            set("distance", hit.distance as f64);
            obj.into()
        },
    ) as Box<dyn Fn(f64, f64, f64, f64, f64) -> JsValue>);
    js_sys::Reflect::set(
        &state,
        &JsValue::from_str("physics_raycast"),
        &f.into_js_value(),
    )?;

    // The shape queries (`physics::query`), from the same snapshot: an array
    // of entity ids for an overlap, a hit table (as `raycast`'s) or `null`
    // for a circle cast. `kind` is 0 circle (a = radius), 1 box (a, b =
    // width, height; c = turn), 2 point.
    {
        use crate::physics::query::QueryShape;
        let snap = Rc::clone(&snapshot);
        let f = Closure::wrap(Box::new(move |kind: f64, x: f64, y: f64, a: f64, b: f64, c: f64| -> JsValue {
            let (x, y, a, b, c) = (x as f32, y as f32, a as f32, b as f32, c as f32);
            let q = match kind as i32 {
                0 => QueryShape::Circle { x, y, radius: a },
                1 => QueryShape::Box { x, y, width: a, height: b, rotation: c },
                _ => QueryShape::Point { x, y },
            };
            let out = js_sys::Array::new();
            for id in snap.borrow().overlap(&q) {
                out.push(&JsValue::from_f64(id as f64));
            }
            out.into()
        }) as Box<dyn Fn(f64, f64, f64, f64, f64, f64) -> JsValue>);
        js_sys::Reflect::set(&state, &JsValue::from_str("physics_overlap"), &f.into_js_value())?;
    }
    {
        let snap = Rc::clone(&snapshot);
        let f = Closure::wrap(Box::new(move |ox: f64, oy: f64, dx: f64, dy: f64, r: f64, max_dist: f64| -> JsValue {
            let Some(hit) = snap.borrow().circle_cast(ox as f32, oy as f32, dx as f32, dy as f32, r as f32, max_dist as f32)
            else {
                return JsValue::NULL;
            };
            let obj = js_sys::Object::new();
            let set = |k: &str, v: f64| {
                let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), &JsValue::from_f64(v));
            };
            set("entity_id", hit.entity_id as f64);
            set("x", hit.point_x as f64);
            set("y", hit.point_y as f64);
            set("normal_x", hit.normal_x as f64);
            set("normal_y", hit.normal_y as f64);
            set("distance", hit.distance as f64);
            obj.into()
        }) as Box<dyn Fn(f64, f64, f64, f64, f64, f64) -> JsValue>);
        js_sys::Reflect::set(&state, &JsValue::from_str("physics_circle_cast"), &f.into_js_value())?;
    }

    Ok(())
}
