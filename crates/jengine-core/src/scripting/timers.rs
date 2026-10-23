//! Script timers for the browser hosts - `set_timer` / `set_interval`.
//!
//! In the editor viewport and web builds, `set_timer` is a preamble function:
//! it has to hand back an id synchronously, so it pushes
//! `{ id, entity, delay, elapsed, repeat, cb }` onto
//! `window.__jengine_play.timers` and returns. Until Sep 2026 **nothing ever
//! read that list back**: every timer and interval in those runtimes was
//! silently dead, while native ticked its own list in `ScriptServices`. The
//! arcade found it - a snake game and a falling-block puzzle never reached
//! their game-over screens, which wait on a timer.
//!
//! [`advance`] is the rule, written once and shared by both hosts so they
//! cannot drift from each other or from native: add the frame's (scaled) dt,
//! fire a timer at most once per frame when it is due, keep an interval's
//! overshoot, drop a one-shot. The hosts do the JS glue around it.

/// One pending timer, as the preamble stores it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptTimer {
    pub id: u32,
    /// Script-side id of the entity whose script owns the callback.
    pub entity: u32,
    pub delay: f32,
    pub elapsed: f32,
    pub repeat: bool,
    /// Name of the script's global function to call with `(entity)`.
    pub callback: String,
}

/// Advance every timer by `dt` and return the `(entity, callback)` pairs that
/// fire this frame, in list order. Fired one-shots are removed; intervals
/// keep their overshoot, the way native's `ScriptServices::tick_timers` does.
pub fn advance(timers: &mut Vec<ScriptTimer>, dt: f32) -> Vec<(u32, String)> {
    let mut fired = Vec::new();
    timers.retain_mut(|t| {
        t.elapsed += dt.max(0.0);
        if t.elapsed < t.delay {
            return true;
        }
        fired.push((t.entity, t.callback.clone()));
        if t.repeat && t.delay > 0.0 {
            t.elapsed -= t.delay;
            true
        } else {
            false
        }
    });
    fired
}

/// The browser hosts' whole timer step: read `__jengine_play.timers`,
/// advance it by `dt`, write the survivors back, and return what fired for
/// the host to call. The list is written back *before* the caller runs any
/// callback, so a callback that sets a new timer appends to the new list.
#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub fn advance_play_timers(play: &js_sys::Object, dt: f32) -> Vec<(u32, String)> {
    use wasm_bindgen::{JsCast, JsValue};
    let key = JsValue::from_str("timers");
    let Ok(list) = js_sys::Reflect::get(play, &key) else { return Vec::new() };
    let Some(list) = list.dyn_ref::<js_sys::Array>() else { return Vec::new() };
    if list.length() == 0 {
        return Vec::new();
    }
    let num = |o: &JsValue, k: &str| {
        js_sys::Reflect::get(o, &JsValue::from_str(k)).ok().and_then(|v| v.as_f64()).unwrap_or(0.0)
    };
    let mut timers: Vec<ScriptTimer> = list
        .iter()
        .map(|o| ScriptTimer {
            id: num(&o, "id") as u32,
            entity: num(&o, "entity") as u32,
            delay: num(&o, "delay") as f32,
            elapsed: num(&o, "elapsed") as f32,
            repeat: js_sys::Reflect::get(&o, &JsValue::from_str("repeat"))
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            callback: js_sys::Reflect::get(&o, &JsValue::from_str("cb"))
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default(),
        })
        .collect();
    let fired = advance(&mut timers, dt);
    let out = js_sys::Array::new();
    for t in &timers {
        let o = js_sys::Object::new();
        let set = |k: &str, v: JsValue| {
            let _ = js_sys::Reflect::set(&o, &JsValue::from_str(k), &v);
        };
        set("id", JsValue::from_f64(t.id as f64));
        set("entity", JsValue::from_f64(t.entity as f64));
        set("delay", JsValue::from_f64(t.delay as f64));
        set("elapsed", JsValue::from_f64(t.elapsed as f64));
        set("repeat", JsValue::from_bool(t.repeat));
        set("cb", JsValue::from_str(&t.callback));
        out.push(&o);
    }
    let _ = js_sys::Reflect::set(play, &key, &out);
    fired
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timer(id: u32, delay: f32, repeat: bool) -> ScriptTimer {
        ScriptTimer { id, entity: 7, delay, elapsed: 0.0, repeat, callback: format!("cb{id}") }
    }

    #[test]
    fn a_one_shot_fires_once_when_due_and_is_removed() {
        let mut t = vec![timer(1, 0.5, false)];
        assert!(advance(&mut t, 0.3).is_empty());
        assert_eq!(advance(&mut t, 0.3), vec![(7, "cb1".to_string())]);
        assert!(t.is_empty());
        assert!(advance(&mut t, 1.0).is_empty());
    }

    #[test]
    fn an_interval_repeats_and_keeps_its_overshoot() {
        let mut t = vec![timer(1, 0.5, true)];
        let mut fires = 0;
        for _ in 0..10 {
            fires += advance(&mut t, 0.3).len();
        }
        // 3.0 s at 0.5 s per fire, at most one per frame.
        assert_eq!(fires, 6);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn a_paused_frame_does_not_advance_anything() {
        let mut t = vec![timer(1, 0.5, false), timer(2, 0.1, true)];
        for _ in 0..100 {
            assert!(advance(&mut t, 0.0).is_empty());
        }
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn a_zero_delay_interval_cannot_fire_forever() {
        let mut t = vec![timer(1, 0.0, true)];
        assert_eq!(advance(&mut t, 0.016).len(), 1);
        assert!(t.is_empty(), "a zero-period interval is treated as a one-shot");
    }
}

/// Drop the play-state a scene owns, when that scene is replaced.
///
/// The browser half of [`crate::scripting::api::ScriptServices::clear_scene_scoped`],
/// and it exists for the same reason: a `Timer` and an event listener each
/// carry a raw entity id, and **the next scene reuses those ids**. A survivor
/// does not linger harmlessly - it fires its callback on whatever unrelated
/// entity inherits the number.
///
/// Native was fixed in BUGS #149; the browser hosts kept theirs on
/// `window.__jengine_play`, whose accessor reuses whatever object is already
/// there and was reset by nothing. Test-lab scene 96 measured it: after
/// twenty-five reloads, one copy heard **twenty-six** pings - one for every
/// dead registration - and twenty-four timers fired out of scenes that no
/// longer existed.
///
/// Written here, once, rather than in each host. Both hosts calling one
/// function is the lesson of the `evt` arm, which carried a private copy of
/// the event dispatch and so did not benefit when the shared one was fixed.
///
/// # What is deliberately left alone
///
/// - `save_data`, which is the *only* thing that carries state across a
///   scene on purpose - a microgame compilation keeps its run there.
/// - `data_tables` and `prefabs`, shared and expensive to rebuild.
/// - `next_timer_id`, `next_id`, `next_entity_id`, `next_tween_id`: rewinding
///   a monotonic counter lets a stale id from a dead scene match a fresh one,
///   which is this bug again by another route.
/// - `__module_cache__`, the `require` cache. Clearing it would make the
///   browsers disagree with native, where a module's state outliving a scene
///   is documented behaviour (see CLAUDE.md) rather than a bug.
/// - Everything the host republishes every frame - positions, inputs, and the
///   rest of the read mirrors.
#[cfg(all(feature = "web", target_arch = "wasm32"))]
pub fn clear_scene_scoped_play_state(play: &js_sys::Object) {
    use wasm_bindgen::JsValue;
    // Entity-keyed, and the reason this exists.
    let _ = js_sys::Reflect::set(play, &JsValue::from_str("timers"), &js_sys::Array::new());
    let _ = js_sys::Reflect::set(
        play,
        &JsValue::from_str("event_listeners"),
        &js_sys::Object::new(),
    );
    // Any command queued by the outgoing scene and not yet drained names its
    // entities too.
    let _ = js_sys::Reflect::set(play, &JsValue::from_str("commands"), &js_sys::Array::new());
}
