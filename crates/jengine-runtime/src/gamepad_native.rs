//! Native gamepad polling, via `gilrs`.
//!
//! Feeds the same `InputState` the browser backend does
//! (`jengine_core::input::gamepad_web`), so an action bound to `Pad0_A`
//! behaves identically in the editor, a web build, and the native runtime.
//!
//! `gilrs` identifies pads by an opaque `GamepadId`, not by a small index, so
//! this keeps its own stable index map: the first pad to appear is `Pad0`, the
//! second `Pad1`, and a pad keeps its slot across a disconnect so bindings
//! survive a cable being kicked out.

use std::collections::{HashMap, HashSet};

use jengine_core::input::InputState;
use jengine_core::input::gamepad::{AXIS_NAMES, BUTTON_NAMES};

/// Wraps `gilrs` and the id → pad-index mapping.
pub struct GamepadBackend {
    gilrs: gilrs::Gilrs,
    /// `gilrs` id → our zero-based pad index.
    slots: HashMap<gilrs::GamepadId, usize>,
    /// Rumbles playing, by pad, with when they end. A gilrs effect stops
    /// when its handle is dropped, so each is kept until then; a new rumble
    /// on the same pad replaces the old one.
    rumbles: HashMap<usize, (gilrs::ff::Effect, std::time::Instant)>,
}

impl GamepadBackend {
    /// Returns `None` when no gamepad subsystem is available (a headless
    /// session, a locked-down container). Gamepad bindings then stay inert,
    /// exactly as they do before any pad is plugged in.
    pub fn new() -> Option<Self> {
        match gilrs::Gilrs::new() {
            Ok(gilrs) => Some(Self {
                gilrs,
                slots: HashMap::new(),
                rumbles: HashMap::new(),
            }),
            Err(e) => {
                // Expected on Android, where gilrs has no backend: said
                // once at info, so it does not read as a fault in a log
                // being scanned for them (`jengine android-smoke`).
                if cfg!(target_os = "android") {
                    log::info!("[input] no gamepads on Android yet: {e}");
                } else {
                    log::warn!("[input] gamepad support unavailable: {e}");
                }
                None
            }
        }
    }

    /// Drain events and push the current state of every pad into `input`.
    /// Call once per frame, before scripts read actions.
    pub fn poll(&mut self, input: &mut InputState) {
        // Draining is what advances gilrs's internal state; without it the
        // per-pad snapshots below never change.
        while self.gilrs.next_event().is_some() {}

        let mut present: Vec<usize> = Vec::new();
        let ids: Vec<gilrs::GamepadId> = self
            .gilrs
            .gamepads()
            .filter(|(_, pad)| pad.is_connected())
            .map(|(id, _)| id)
            .collect();

        for id in ids {
            let next_slot = self.slots.len();
            let index = *self.slots.entry(id).or_insert(next_slot);
            let pad = self.gilrs.gamepad(id);

            let mut pressed = HashSet::new();
            for (i, name) in BUTTON_NAMES.iter().enumerate() {
                if let Some(button) = button_for(name)
                    && pad.is_pressed(button)
                {
                    pressed.insert(i);
                }
            }

            // gilrs reports stick Y positive up, which is the engine's
            // convention (`gamepad::stick_y_up`), so nothing is flipped.
            let axes: Vec<f32> = AXIS_NAMES
                .iter()
                .map(|name| {
                    axis_for(name)
                        .and_then(|axis| pad.axis_data(axis).map(|d| d.value()))
                        .unwrap_or(0.0)
                })
                .collect();

            input.update_gamepad(index, pressed, axes);
            input.set_gamepad_info(index, pad.name(), pad.vendor_id());
            present.push(index);
        }

        // A pad that stopped reporting has been unplugged; release what it
        // held, or an action stays down forever.
        input.retain_gamepads(&present);

        self.play_haptics();
    }

    /// Play what scripts asked for (`jengine_core::input::haptics`): each
    /// rumble as a force-feedback effect of the two motors on its pad. A
    /// desktop has no device to vibrate, so `vibrate` is dropped here.
    fn play_haptics(&mut self) {
        use gilrs::ff::{BaseEffect, BaseEffectType, EffectBuilder, Replay, Ticks};
        use jengine_core::input::haptics::{self, Haptic};
        let now = std::time::Instant::now();
        self.rumbles.retain(|_, (_, end)| *end > now);
        for h in haptics::take() {
            let Haptic::Rumble { pad, strong, weak, seconds } = h else { continue };
            let Some(id) = self.slots.iter().find(|(_, i)| **i == pad).map(|(id, _)| *id) else { continue };
            // Asking for nothing stops what is playing.
            self.rumbles.remove(&pad);
            if seconds <= 0.0 || (strong <= 0.0 && weak <= 0.0) {
                continue;
            }
            if !self.gilrs.gamepad(id).is_ff_supported() {
                log::debug!("[input] pad {pad} cannot rumble");
                continue;
            }
            let ms = (seconds * 1000.0) as u32;
            let scheduling = Replay { after: Ticks::from_ms(0), play_for: Ticks::from_ms(ms), with_delay: Ticks::from_ms(0) };
            let magnitude = |v: f32| (v.clamp(0.0, 1.0) * u16::MAX as f32) as u16;
            let effect = EffectBuilder::new()
                .add_effect(BaseEffect {
                    kind: BaseEffectType::Strong { magnitude: magnitude(strong) },
                    scheduling,
                    envelope: Default::default(),
                })
                .add_effect(BaseEffect {
                    kind: BaseEffectType::Weak { magnitude: magnitude(weak) },
                    scheduling,
                    envelope: Default::default(),
                })
                .gamepads(&[id])
                .finish(&mut self.gilrs);
            match effect {
                Ok(effect) => {
                    if let Err(e) = effect.play() {
                        log::warn!("[input] rumble on pad {pad} failed: {e}");
                        continue;
                    }
                    self.rumbles.insert(pad, (effect, now + std::time::Duration::from_millis(ms as u64 + 50)));
                }
                Err(e) => log::warn!("[input] could not make a rumble for pad {pad}: {e}"),
            }
        }
    }
}

/// Map a W3C Standard Gamepad button name onto gilrs's button enum.
///
/// The names are the engine's binding vocabulary and must mean the same thing
/// on every backend, so this is an explicit table rather than an ordering
/// assumption.
fn button_for(name: &str) -> Option<gilrs::Button> {
    use gilrs::Button;
    Some(match name {
        "A" => Button::South,
        "B" => Button::East,
        "X" => Button::West,
        "Y" => Button::North,
        "LeftBumper" => Button::LeftTrigger,
        "RightBumper" => Button::RightTrigger,
        "LeftTrigger" => Button::LeftTrigger2,
        "RightTrigger" => Button::RightTrigger2,
        "Back" => Button::Select,
        "Start" => Button::Start,
        "LeftStick" => Button::LeftThumb,
        "RightStick" => Button::RightThumb,
        "DPadUp" => Button::DPadUp,
        "DPadDown" => Button::DPadDown,
        "DPadLeft" => Button::DPadLeft,
        "DPadRight" => Button::DPadRight,
        "Guide" => Button::Mode,
        _ => return None,
    })
}

/// Map a W3C axis name onto gilrs's axis enum.
fn axis_for(name: &str) -> Option<gilrs::Axis> {
    use gilrs::Axis;
    Some(match name {
        "LeftStickX" => Axis::LeftStickX,
        "LeftStickY" => Axis::LeftStickY,
        "RightStickX" => Axis::RightStickX,
        "RightStickY" => Axis::RightStickY,
        _ => return None,
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A rumble for a pad that is not there is drained, not kept: the queue
    /// is the process's, and a request left in it would play the moment a
    /// pad appeared.
    #[test]
    fn a_rumble_for_a_missing_pad_is_dropped() {
        let Some(mut backend) = GamepadBackend::new() else { return };
        let _ = jengine_core::input::haptics::take();
        jengine_core::input::haptics::rumble(7, 1.0, 1.0, 0.5);
        backend.poll(&mut jengine_core::input::InputState::new());
        assert!(jengine_core::input::haptics::take().is_empty());
        assert!(backend.rumbles.is_empty());
    }

    #[test]
    fn every_binding_name_maps_to_a_gilrs_button() {
        // A name in the engine's vocabulary that this backend cannot map
        // would be a binding that works in the browser and silently does
        // nothing natively — the exact class of bug this session has been
        // clearing out.
        for name in BUTTON_NAMES {
            assert!(
                button_for(name).is_some(),
                "no gilrs button for binding name '{name}'"
            );
        }
        for name in AXIS_NAMES {
            assert!(
                axis_for(name).is_some(),
                "no gilrs axis for binding name '{name}'"
            );
        }
    }
}
