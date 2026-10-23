//! Browser gamepad polling.
//!
//! The Gamepad API is poll-only by design: `navigator.getGamepads()` returns
//! a fresh snapshot each call, and there is no event for "button went down".
//! That suits [`super::gamepad::GamepadState`], which derives its edges from
//! the previous frame's snapshot anyway.
//!
//! Shared by the editor viewport and the web runtime so both read pads the
//! same way — the split between those two hosts is where several script
//! functions quietly diverged before.
//!
//! ## Browser quirks this handles
//!
//! * **Nothing appears until the user presses a button.** Browsers hide pads
//!   from a page that has never received gamepad input (a fingerprinting
//!   guard), so an empty list is normal, not an error.
//! * **Slots are sparse.** `getGamepads()` returns a fixed-length array with
//!   `null` holes; a pad's `index` is its identity, so a pad unplugged and
//!   replugged keeps its bindings.
//! * **Standard mapping is not guaranteed.** A pad reporting a non-standard
//!   mapping still gives buttons and axes, just not in W3C order. It is used
//!   as-is rather than dropped: wrong-but-rebindable beats absent.

use wasm_bindgen::JsCast;

use super::InputState;

/// Poll every connected pad into `input`. Call once per frame, before scripts
/// read actions.
pub fn poll_gamepads(input: &mut InputState) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let navigator = window.navigator();
    let Ok(pads) = navigator.get_gamepads() else {
        // Not supported, or blocked by permissions policy. Bindings stay
        // inert rather than erroring every frame - but a phone's vibration
        // needs no pad.
        play_haptics(&navigator, &js_sys::Array::new());
        return;
    };

    let mut seen: Vec<usize> = Vec::new();
    for entry in pads.iter() {
        let Ok(pad) = entry.dyn_into::<web_sys::Gamepad>() else {
            continue; // an empty slot
        };
        if !pad.connected() {
            continue;
        }
        let index = pad.index() as usize;

        let buttons = pad.buttons();
        let mut pressed = std::collections::HashSet::new();
        for (i, b) in buttons.iter().enumerate() {
            if let Ok(button) = b.dyn_into::<web_sys::GamepadButton>()
                && button.pressed()
            {
                pressed.insert(i);
            }
        }

        let mut axes: Vec<f32> = pad
            .axes()
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();
        // The Gamepad API reports stick Y positive down; the engine's is up.
        super::gamepad::stick_y_up(&mut axes);

        input.update_gamepad(index, pressed, axes);
        // `id` is the pad's name, with its USB vendor inside it.
        let id = pad.id();
        input.set_gamepad_info(index, &id, super::prompts::vendor_in_id(&id));
        seen.push(index);
    }

    // A pad that stopped reporting has gone away; release whatever it held.
    input.retain_gamepads(&seen);

    play_haptics(&navigator, &pads);
}

/// Play what scripts asked for (`super::haptics`): a rumble as the Gamepad
/// API's `dual-rumble` effect on its pad's `vibrationActuator`, a vibrate as
/// `navigator.vibrate`. Both are looked up rather than bound, because
/// neither is in every browser (Safari has no `vibrate`, and some pads no
/// actuator) - and an absent one is a silent no-op, as on hardware that
/// cannot buzz.
fn play_haptics(navigator: &web_sys::Navigator, pads: &js_sys::Array) {
    use super::haptics::{self, Haptic};
    use wasm_bindgen::JsValue;
    let get = |o: &JsValue, k: &str| js_sys::Reflect::get(o, &JsValue::from_str(k)).unwrap_or(JsValue::UNDEFINED);
    for h in haptics::take() {
        match h {
            Haptic::Rumble { pad, strong, weak, seconds } => {
                let Some(gp) = pads.iter().find(|p| {
                    p.dyn_ref::<web_sys::Gamepad>().is_some_and(|g| g.connected() && g.index() as usize == pad)
                }) else {
                    continue;
                };
                let actuator = get(&gp, "vibrationActuator");
                let Some(play) = get(&actuator, "playEffect").dyn_ref::<js_sys::Function>().cloned() else { continue };
                let params = js_sys::Object::new();
                let set = |k: &str, v: f64| {
                    let _ = js_sys::Reflect::set(&params, &JsValue::from_str(k), &JsValue::from_f64(v));
                };
                set("startDelay", 0.0);
                set("duration", (seconds * 1000.0) as f64);
                set("strongMagnitude", strong as f64);
                set("weakMagnitude", weak as f64);
                let _ = play.call2(&actuator, &JsValue::from_str("dual-rumble"), &params);
            }
            Haptic::Vibrate { seconds, strength } => {
                let Some(vibrate) = get(navigator, "vibrate").dyn_ref::<js_sys::Function>().cloned() else { continue };
                // The Vibration API has no strength: a weak buzz is a short one.
                let ms = if strength <= 0.0 { 0.0 } else { (seconds * 1000.0 * (0.4 + 0.6 * strength)) as f64 };
                let _ = vibrate.call1(navigator, &JsValue::from_f64(ms));
            }
        }
    }
}
