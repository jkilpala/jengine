//! Rumble and vibration (Oct 2026; Godot's `Input.start_joy_vibration` /
//! `vibrate_handheld`, Unity's gamepad haptics).
//!
//! Two requests, kept apart because they are different hardware:
//!
//! * [`rumble`] shakes a **gamepad**: its strong (low-frequency) and weak
//!   (high-frequency) motors, each 0..1, for some seconds. Natively through
//!   gilrs' force feedback, in a browser through the Gamepad API's
//!   `dual-rumble` effect, on Android through the controller's vibrator.
//! * [`vibrate`] buzzes the **device** - a phone. In a browser
//!   `navigator.vibrate` (Android browsers; iOS has none), in an Android
//!   build the system vibrator. A desktop has nothing to buzz and ignores it.
//!
//! Like the language (`crate::locale`), this is the process's, not the
//! world's: a script's request goes into one queue here, and whichever
//! gamepad poll a platform already runs every frame drains it
//! (`take`) - the native runtime's gilrs backend, `gamepad_web::poll_gamepads`
//! in both browser hosts, the Android controller glue. So no host is wired
//! for it, and a request made before any pad is connected is simply dropped
//! by the poll that finds no pad.

use std::sync::Mutex;

/// One request, as the backends receive it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Haptic {
    /// Gamepad `pad` (the `Pad0` of the bindings is 0): motor strengths
    /// 0..1 for `seconds`. Zero strength or zero seconds stops it.
    Rumble { pad: usize, strong: f32, weak: f32, seconds: f32 },
    /// The device itself, at `strength` 0..1, for `seconds`.
    Vibrate { seconds: f32, strength: f32 },
}

/// The longest a request may run: a game that forgets to stop a rumble
/// should not buzz a pad for a minute.
pub const MAX_SECONDS: f32 = 5.0;

static QUEUE: Mutex<Vec<Haptic>> = Mutex::new(Vec::new());

fn push(h: Haptic) {
    if let Ok(mut q) = QUEUE.lock() {
        // A runaway script asking every frame keeps only the latest few.
        if q.len() >= 64 {
            q.remove(0);
        }
        q.push(h);
    }
}

fn unit(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
}

fn secs(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, MAX_SECONDS) } else { 0.0 }
}

/// `Input.rumble(pad, strong, weak, seconds)`.
pub fn rumble(pad: usize, strong: f32, weak: f32, seconds: f32) {
    push(Haptic::Rumble { pad, strong: unit(strong), weak: unit(weak), seconds: secs(seconds) });
}

/// `Input.vibrate(seconds, strength)`.
pub fn vibrate(seconds: f32, strength: f32) {
    push(Haptic::Vibrate { seconds: secs(seconds), strength: unit(strength) });
}

/// Everything asked for since the last call. The platform's gamepad poll
/// calls this once a frame and plays what it can.
pub fn take() -> Vec<Haptic> {
    QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

/// The strongest of a rumble's two motors, for hardware with one.
pub fn single_motor(strong: f32, weak: f32) -> f32 {
    strong.max(weak * 0.7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_clamped_and_drained_once() {
        let _ = take();
        rumble(1, 2.0, -1.0, 99.0);
        vibrate(0.2, f32::NAN);
        let got = take();
        assert_eq!(got[0], Haptic::Rumble { pad: 1, strong: 1.0, weak: 0.0, seconds: MAX_SECONDS });
        assert_eq!(got[1], Haptic::Vibrate { seconds: 0.2, strength: 0.0 });
        assert!(take().is_empty());
    }
}
