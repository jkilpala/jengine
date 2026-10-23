//! Publish the input snapshot that browser-runtime scripts read.
//!
//! # Why this exists
//!
//! The browser preamble used to answer `is_action_pressed` by looking the
//! action's *bindings* up in `action_bindings` and testing each against
//! `pressed_keys`. That map is built from `KeyCode`s, so it only ever contains
//! keyboard keys — and a gamepad binding like `Pad0_A` was therefore never
//! found. **Gamepad actions worked natively and were silently dead in the
//! editor and in web builds**, which is to say dead in the runtime you develop
//! in and the one you ship (BUGS #9).
//!
//! Native never had the problem because it does not re-derive anything: it
//! resolves actions in Rust, through `InputState`, and hands the *answers* to
//! scripts. This does the same for the browser hosts. The preamble reads a
//! resolved set instead of re-implementing binding resolution in JavaScript,
//! which is one implementation instead of two — and the second one could not
//! see half the input devices.
//!
//! Both browser hosts call [`publish`], so there is no second copy to drift.

use crate::input::InputState;
use wasm_bindgen::JsValue;

/// Publish resolved action state, action axes, and raw pad state onto the
/// `__jengine_play` namespace.
///
/// Called once per frame by each browser host, next to the key maps it already
/// publishes. Cheap: an action set is a handful of names, and pads are at most
/// a few — this is not the per-entity mirror.
pub fn publish(play: &js_sys::Object, input: &InputState) {
    let set = |key: &str, obj: &js_sys::Object| {
        let _ = js_sys::Reflect::set(play, &JsValue::from_str(key), obj);
    };

    let pressed = js_sys::Object::new();
    let just_pressed = js_sys::Object::new();
    let just_released = js_sys::Object::new();
    let axes = js_sys::Object::new();
    for (name, _) in input.get_actions() {
        let key = JsValue::from_str(name);
        if input.is_action_pressed(name) {
            let _ = js_sys::Reflect::set(&pressed, &key, &JsValue::TRUE);
        }
        if input.is_action_just_pressed(name) {
            let _ = js_sys::Reflect::set(&just_pressed, &key, &JsValue::TRUE);
        }
        if input.is_action_just_released(name) {
            let _ = js_sys::Reflect::set(&just_released, &key, &JsValue::TRUE);
        }
        // Published for every action, not just non-zero ones: a script reading
        // an axis wants 0.0 back rather than `undefined`, and an action that
        // has been released this frame must stop reporting its last value.
        let _ = js_sys::Reflect::set(
            &axes,
            &key,
            &JsValue::from_f64(input.action_axis_value(name) as f64),
        );
    }
    set("actions_pressed", &pressed);
    set("actions_just_pressed", &just_pressed);
    set("actions_just_released", &just_released);
    set("action_axes", &axes);

    // Raw pads, for a game that wants a stick rather than a bound action.
    // Disconnected slots are published too, with `connected: false`, because
    // the indices have to stay stable across a reconnect and a script asking
    // about pad 1 should get an answer rather than a hole.
    let pads = js_sys::Array::new();
    for index in 0..input.pad_count() {
        let entry = js_sys::Object::new();
        let connected = input.is_pad_connected(index);
        let _ = js_sys::Reflect::set(
            &entry,
            &JsValue::from_str("connected"),
            &JsValue::from_bool(connected),
        );
        let axis_arr = js_sys::Array::new();
        let button_obj = js_sys::Object::new();
        if let Some(state) = input.pad_state(index) {
            for value in &state.axes {
                axis_arr.push(&JsValue::from_f64(*value as f64));
            }
            for button in &state.pressed {
                let _ = js_sys::Reflect::set(
                    &button_obj,
                    &JsValue::from_f64(*button as f64),
                    &JsValue::TRUE,
                );
            }
        }
        let _ = js_sys::Reflect::set(&entry, &JsValue::from_str("axes"), &axis_arr);
        let _ = js_sys::Reflect::set(&entry, &JsValue::from_str("buttons"), &button_obj);
        pads.push(&entry);
    }
    let _ = js_sys::Reflect::set(play, &JsValue::from_str("pads"), &pads);

    // Every finger, for `get_touches`. The preamble turns each into a world
    // point itself, from the camera the host publishes beside this.
    let touches = js_sys::Array::new();
    for t in &input.touches {
        let entry = js_sys::Object::new();
        let put = |k: &str, v: JsValue| {
            let _ = js_sys::Reflect::set(&entry, &JsValue::from_str(k), &v);
        };
        put("id", JsValue::from_f64(t.id as f64));
        put("x", JsValue::from_f64(t.position.0 as f64));
        put("y", JsValue::from_f64(t.position.1 as f64));
        put("sx", JsValue::from_f64(t.start.0 as f64));
        put("sy", JsValue::from_f64(t.start.1 as f64));
        put("phase", JsValue::from_str(t.stage.name()));
        put("c", JsValue::from_bool(t.on_control));
        touches.push(&entry);
    }
    let _ = js_sys::Reflect::set(play, &JsValue::from_str("touches"), &touches);

    // This frame's gestures, for `get_gestures`; world points likewise.
    let gestures = js_sys::Array::new();
    for g in &input.gestures {
        let entry = js_sys::Object::new();
        let put = |k: &str, v: JsValue| {
            let _ = js_sys::Reflect::set(&entry, &JsValue::from_str(k), &v);
        };
        put("kind", JsValue::from_str(g.kind.name()));
        put("x", JsValue::from_f64(g.x as f64));
        put("y", JsValue::from_f64(g.y as f64));
        put("dx", JsValue::from_f64(g.dx as f64));
        put("dy", JsValue::from_f64(g.dy as f64));
        put("scale", JsValue::from_f64(g.scale as f64));
        put("angle", JsValue::from_f64(g.angle as f64));
        put("speed", JsValue::from_f64(g.speed as f64));
        put("phase", JsValue::from_str(g.phase));
        gestures.push(&entry);
    }
    let _ = js_sys::Reflect::set(play, &JsValue::from_str("gestures"), &gestures);

    // Holds, taps, pads' names, prompts: answered by the hook from this.
    super::input_query::set_view(crate::input::interactions::InputView::of(input));
}
