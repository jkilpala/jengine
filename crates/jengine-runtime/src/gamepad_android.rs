//! Game controllers on Android.
//!
//! `gilrs` has no Android backend, and winit has no gamepad support - worse,
//! it read a thumbstick's joystick `MotionEvent` as a *touch* at the stick's
//! own `-1..1` coordinates, so pushing a stick pressed the top-left corner of
//! the screen. A patched winit (`vendor/winit/JENGINE-PATCH.md`) lets this
//! module see every input event first; it claims the ones that come from a
//! controller and turns them into the same pad state `gamepad_native` feeds
//! on a desktop - W3C Standard Gamepad buttons and axes, stick Y positive
//! up (`gamepad::stick_y_up`) - so a `Pad0_A` binding means the same button,
//! and `LeftStickY+` the same direction, everywhere.
//!
//! Android reports controllers as events, not state: a key event per button
//! and a joystick motion event carrying every axis. [`PadTracker`] keeps the
//! latest of both per device and derives a snapshot each frame. The tracker
//! is plain code, tested on any platform; only the hook that feeds it and
//! the check for unplugged devices are Android's.

// On a desktop only the tests reach the tracker; the hook that feeds it is
// Android's.
#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::collections::{HashMap, HashSet};

use jengine_core::input::InputState;

/// Android SDK `KeyEvent.KEYCODE_*` values a controller sends.
mod keycode {
    pub const DPAD_UP: u32 = 19;
    pub const DPAD_DOWN: u32 = 20;
    pub const DPAD_LEFT: u32 = 21;
    pub const DPAD_RIGHT: u32 = 22;
    pub const DPAD_CENTER: u32 = 23;
    pub const BUTTON_A: u32 = 96;
    pub const BUTTON_B: u32 = 97;
    pub const BUTTON_C: u32 = 98;
    pub const BUTTON_X: u32 = 99;
    pub const BUTTON_Y: u32 = 100;
    pub const BUTTON_Z: u32 = 101;
    pub const BUTTON_L1: u32 = 102;
    pub const BUTTON_R1: u32 = 103;
    pub const BUTTON_L2: u32 = 104;
    pub const BUTTON_R2: u32 = 105;
    pub const BUTTON_THUMBL: u32 = 106;
    pub const BUTTON_THUMBR: u32 = 107;
    pub const BUTTON_START: u32 = 108;
    pub const BUTTON_SELECT: u32 = 109;
    pub const BUTTON_MODE: u32 = 110;
}

/// Android SDK `MotionEvent.AXIS_*` values a controller reports.
pub mod axis {
    pub const X: u32 = 0;
    pub const Y: u32 = 1;
    pub const Z: u32 = 11;
    pub const RX: u32 = 12;
    pub const RY: u32 = 13;
    pub const RZ: u32 = 14;
    pub const HAT_X: u32 = 15;
    pub const HAT_Y: u32 = 16;
    pub const LTRIGGER: u32 = 17;
    pub const RTRIGGER: u32 = 18;
    pub const GAS: u32 = 22;
    pub const BRAKE: u32 = 23;
    /// Every axis the tracker reads, for the hook to ask for.
    pub const ALL: [u32; 12] = [X, Y, Z, RX, RY, RZ, HAT_X, HAT_Y, LTRIGGER, RTRIGGER, GAS, BRAKE];
}

/// `InputDevice.SOURCE_GAMEPAD` without its class bits: set on a
/// controller's buttons and D-pad, never on a keyboard's arrow keys, which
/// share the D-pad's key codes.
const SOURCE_GAMEPAD_BIT: u32 = 0x0000_0400;
/// `InputDevice.SOURCE_JOYSTICK`: a controller's sticks, triggers and hat.
const SOURCE_JOYSTICK: u32 = 0x0100_0010;

/// A button or trigger past this is held. Triggers are analog on most pads.
const TRIGGER_PRESSED: f32 = 0.5;

/// The W3C Standard Gamepad button a controller key is, if any. The indices
/// are `gamepad::BUTTON_NAMES`'.
fn button_index(code: u32) -> Option<usize> {
    use keycode::*;
    Some(match code {
        BUTTON_A => 0,
        BUTTON_B => 1,
        BUTTON_X => 2,
        BUTTON_Y => 3,
        BUTTON_L1 => 4,
        BUTTON_R1 => 5,
        BUTTON_L2 => 6,
        BUTTON_R2 => 7,
        BUTTON_SELECT => 8,
        BUTTON_START => 9,
        BUTTON_THUMBL => 10,
        BUTTON_THUMBR => 11,
        DPAD_UP => 12,
        DPAD_DOWN => 13,
        DPAD_LEFT => 14,
        DPAD_RIGHT => 15,
        BUTTON_MODE => 16,
        // A D-pad's centre press is its "A" on a TV remote-style pad.
        DPAD_CENTER => 0,
        // Six-button pads: C and Z have no standard slot.
        BUTTON_C | BUTTON_Z => return None,
        _ => return None,
    })
}

/// Whether a key event is a controller's. A `BUTTON_*` code is only ever
/// one; a D-pad code is also a keyboard's arrow key, so that is decided by
/// the device the event came from.
pub fn is_controller_key(source: u32, code: u32) -> bool {
    if (keycode::BUTTON_A..=keycode::BUTTON_MODE).contains(&code) {
        return button_index(code).is_some();
    }
    button_index(code).is_some() && source & SOURCE_GAMEPAD_BIT != 0
}

/// Whether a motion event is a controller's sticks rather than a finger.
pub fn is_controller_motion(source: u32) -> bool {
    source & SOURCE_JOYSTICK == SOURCE_JOYSTICK
}

/// One controller event, as the hook saw it.
#[derive(Debug, Clone, PartialEq)]
pub enum PadEvent {
    Key { device: i32, code: u32, down: bool },
    Motion { device: i32, axes: Vec<(u32, f32)> },
}

#[derive(Debug, Default)]
struct Device {
    keys: HashSet<usize>,
    /// Buttons that went down since the last publish. A press released
    /// before the frame read it still shows for one frame: Android delivers
    /// events, and a tap can start and end between two frames.
    tapped: HashSet<usize>,
    axes: HashMap<u32, f32>,
}

impl Device {
    fn axis(&self, a: u32) -> f32 {
        self.axes.get(&a).copied().unwrap_or(0.0)
    }

    /// The right stick is `Z` / `RZ` on most pads and `RX` / `RY` on some:
    /// whichever pair the device has moved is the stick.
    fn right_stick(&self) -> (f32, f32) {
        let zx = self.axis(axis::Z);
        let zy = self.axis(axis::RZ);
        let rx = self.axis(axis::RX);
        let ry = self.axis(axis::RY);
        if zx.abs() + zy.abs() >= rx.abs() + ry.abs() { (zx, zy) } else { (rx, ry) }
    }

    /// Buttons held and the four standard axes.
    fn snapshot(&self) -> (HashSet<usize>, Vec<f32>) {
        let mut pressed: HashSet<usize> = self.keys.union(&self.tapped).copied().collect();
        // Triggers arrive as axes (`LTRIGGER`, or `BRAKE` on some pads).
        if self.axis(axis::LTRIGGER).max(self.axis(axis::BRAKE)) > TRIGGER_PRESSED {
            pressed.insert(6);
        }
        if self.axis(axis::RTRIGGER).max(self.axis(axis::GAS)) > TRIGGER_PRESSED {
            pressed.insert(7);
        }
        // Many pads report the D-pad as a hat axis rather than as keys.
        let (hx, hy) = (self.axis(axis::HAT_X), self.axis(axis::HAT_Y));
        if hy < -TRIGGER_PRESSED {
            pressed.insert(12);
        }
        if hy > TRIGGER_PRESSED {
            pressed.insert(13);
        }
        if hx < -TRIGGER_PRESSED {
            pressed.insert(14);
        }
        if hx > TRIGGER_PRESSED {
            pressed.insert(15);
        }
        let (rx, ry) = self.right_stick();
        let mut axes = vec![self.axis(axis::X), self.axis(axis::Y), rx, ry];
        // Android's stick Y is positive down; the engine's is up.
        jengine_core::input::gamepad::stick_y_up(&mut axes);
        (pressed, axes)
    }
}

/// Controllers by Android device id, each with a stable pad slot: the first
/// controller seen is `Pad0`, and a controller keeps its slot across a
/// reconnect, as on a desktop.
#[derive(Debug, Default)]
pub struct PadTracker {
    devices: HashMap<i32, Device>,
    slots: HashMap<i32, usize>,
    /// Each controller's name and USB vendor, asked of Android once.
    identity: HashMap<i32, (String, Option<u16>)>,
}

impl PadTracker {
    pub fn apply(&mut self, event: PadEvent) {
        let device = match &event {
            PadEvent::Key { device, .. } | PadEvent::Motion { device, .. } => *device,
        };
        if !self.slots.contains_key(&device) {
            let slot = self.slots.len();
            self.slots.insert(device, slot);
            log::info!("[input] controller connected as Pad{slot} (Android device {device})");
        }
        let d = self.devices.entry(device).or_default();
        match event {
            PadEvent::Key { code, down, .. } => {
                if let Some(i) = button_index(code) {
                    if down {
                        d.keys.insert(i);
                        d.tapped.insert(i);
                    } else {
                        d.keys.remove(&i);
                    }
                }
            }
            PadEvent::Motion { axes, .. } => {
                for (a, v) in axes {
                    d.axes.insert(a, v);
                }
            }
        }
    }

    /// Forget a controller that is gone; its slot stays reserved for it.
    pub fn remove(&mut self, device: i32) {
        if self.devices.remove(&device).is_some() {
            log::info!("[input] controller disconnected (Android device {device})");
        }
    }

    /// The Android device a pad slot belongs to - what `Input.rumble(pad, ...)`
    /// has to find.
    pub fn device_for_slot(&self, slot: usize) -> Option<i32> {
        self.slots.iter().find(|(_, s)| **s == slot).map(|(d, _)| *d)
    }

    /// The Android device ids currently tracked.
    pub fn devices(&self) -> Vec<i32> {
        self.devices.keys().copied().collect()
    }

    /// Controllers whose name has not been asked for yet.
    pub fn unnamed(&self) -> Vec<i32> {
        self.devices.keys().filter(|d| !self.identity.contains_key(d)).copied().collect()
    }

    /// What Android calls a controller, and its USB vendor id.
    pub fn set_identity(&mut self, device: i32, name: String, vendor: Option<u16>) {
        self.identity.insert(device, (name, vendor));
    }

    /// Push every controller's state into `input`, and disconnect the pads
    /// whose device is gone. Once a frame: a tap is shown by one publish.
    pub fn publish(&mut self, input: &mut InputState) {
        let mut present = Vec::new();
        for (id, d) in &mut self.devices {
            let slot = self.slots[id];
            let (pressed, axes) = d.snapshot();
            d.tapped.clear();
            input.update_gamepad(slot, pressed, axes);
            if let Some((name, vendor)) = self.identity.get(id) {
                input.set_gamepad_info(slot, name, *vendor);
            }
            present.push(slot);
        }
        input.retain_gamepads(&present);
    }
}

#[cfg(target_os = "android")]
mod glue {
    //! The hook winit calls, and the check for unplugged controllers.

    use std::sync::Mutex;

    use winit::platform::android::activity::input::{Axis, InputEvent, KeyAction};

    use super::{PadEvent, PadTracker, is_controller_key, is_controller_motion};

    static TRACKER: Mutex<Option<PadTracker>> = Mutex::new(None);

    fn with_tracker<R>(f: impl FnOnce(&mut PadTracker) -> R) -> R {
        let mut guard = TRACKER.lock().unwrap_or_else(|e| e.into_inner());
        f(guard.get_or_insert_with(PadTracker::default))
    }

    fn axis_of(a: u32) -> Option<Axis> {
        use super::axis::*;
        Some(match a {
            X => Axis::X,
            Y => Axis::Y,
            Z => Axis::Z,
            RX => Axis::Rx,
            RY => Axis::Ry,
            RZ => Axis::Rz,
            HAT_X => Axis::HatX,
            HAT_Y => Axis::HatY,
            LTRIGGER => Axis::Ltrigger,
            RTRIGGER => Axis::Rtrigger,
            GAS => Axis::Gas,
            BRAKE => Axis::Brake,
            _ => return None,
        })
    }

    /// Claim controller events before winit sees them. Everything else -
    /// fingers, keyboards, the back button - goes on to winit as before.
    pub fn install() {
        winit::platform::android::set_input_hook(Some(Box::new(|event: &InputEvent<'_>| {
            match event {
                InputEvent::KeyEvent(key) => {
                    let source: u32 = key.source().into();
                    let code: u32 = key.key_code().into();
                    if !is_controller_key(source, code) {
                        return false;
                    }
                    let down = match key.action() {
                        KeyAction::Down => true,
                        KeyAction::Up => false,
                        // A repeat-count "multiple" changes nothing held.
                        _ => return true,
                    };
                    with_tracker(|t| t.apply(PadEvent::Key { device: key.device_id(), code, down }));
                    true
                }
                InputEvent::MotionEvent(motion) => {
                    let source: u32 = motion.source().into();
                    if !is_controller_motion(source) || motion.pointer_count() == 0 {
                        return false;
                    }
                    let pointer = motion.pointer_at_index(0);
                    let axes = super::axis::ALL
                        .iter()
                        .filter_map(|&a| axis_of(a).map(|ax| (a, pointer.axis_value(ax))))
                        .collect();
                    with_tracker(|t| t.apply(PadEvent::Motion { device: motion.device_id(), axes }));
                    true
                }
                _ => false,
            }
        })));
    }

    /// Frames between asking Android which controllers still exist.
    const PRESENCE_CHECK_FRAMES: u32 = 60;
    static FRAMES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    /// Push controller state into `input`. Once a second, drop controllers
    /// Android no longer knows: a pad switched off sends no event to say so,
    /// and its buttons would stay held.
    pub fn poll(input: &mut jengine_core::input::InputState) {
        let frame = FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if frame % PRESENCE_CHECK_FRAMES == 0 {
            let known = with_tracker(|t| t.devices());
            for id in known {
                if !crate::android::input_device_exists(id) {
                    with_tracker(|t| t.remove(id));
                }
            }
        }
        for id in with_tracker(|t| t.unnamed()) {
            let (name, vendor) = crate::android::input_device_identity(id).unwrap_or_default();
            with_tracker(|t| t.set_identity(id, name, vendor));
        }
        with_tracker(|t| t.publish(input));
        play_haptics();
    }

    /// Play what scripts asked for (`jengine_core::input::haptics`): a
    /// rumble on its controller's vibrator, a vibrate on the phone's.
    fn play_haptics() {
        use jengine_core::input::haptics::{self, Haptic, single_motor};
        for h in haptics::take() {
            match h {
                Haptic::Rumble { pad, strong, weak, seconds } => {
                    if let Some(device) = with_tracker(|t| t.device_for_slot(pad)) {
                        let amp = (single_motor(strong, weak) * 255.0) as i32;
                        crate::android::vibrate_input_device(device, (seconds * 1000.0) as i64, amp);
                    }
                }
                Haptic::Vibrate { seconds, strength } => {
                    crate::android::vibrate_phone((seconds * 1000.0) as i64, (strength * 255.0) as i32);
                }
            }
        }
    }
}

#[cfg(target_os = "android")]
pub use glue::{install, poll};

#[cfg(test)]
mod tests {
    use super::*;

    const PAD: u32 = 0x0000_0501; // KEYBOARD | GAMEPAD, as most pads' buttons report
    const KEYBOARD: u32 = 0x0000_0101;

    #[test]
    fn a_rumble_finds_the_controller_in_its_slot() {
        let mut t = PadTracker::default();
        t.apply(PadEvent::Key { device: 12, code: 96, down: true });
        t.apply(PadEvent::Key { device: 30, code: 96, down: true });
        assert_eq!(t.device_for_slot(0), Some(12));
        assert_eq!(t.device_for_slot(1), Some(30));
        assert_eq!(t.device_for_slot(2), None);
    }

    #[test]
    fn a_keyboards_arrows_are_not_a_dpad() {
        assert!(is_controller_key(PAD, keycode::DPAD_UP));
        assert!(!is_controller_key(KEYBOARD, keycode::DPAD_UP));
        // A BUTTON_* code is a controller's whatever the source says.
        assert!(is_controller_key(KEYBOARD, keycode::BUTTON_A));
        assert!(!is_controller_key(PAD, 62)); // KEYCODE_SPACE
    }

    #[test]
    fn sticks_are_claimed_and_fingers_are_not() {
        assert!(is_controller_motion(SOURCE_JOYSTICK));
        assert!(!is_controller_motion(0x0000_1002)); // touchscreen
        assert!(!is_controller_motion(0x0000_2002)); // mouse
    }

    fn snapshot(t: &mut PadTracker) -> (Vec<usize>, Vec<f32>) {
        let mut input = InputState::new();
        t.publish(&mut input);
        let pad = &input.pad_state(0).unwrap();
        let mut pressed: Vec<usize> = pad.pressed.iter().copied().collect();
        pressed.sort();
        (pressed, pad.axes.clone())
    }

    #[test]
    fn buttons_triggers_hat_and_sticks_map_to_the_standard_layout() {
        let mut t = PadTracker::default();
        t.apply(PadEvent::Key { device: 7, code: keycode::BUTTON_A, down: true });
        t.apply(PadEvent::Key { device: 7, code: keycode::BUTTON_START, down: true });
        t.apply(PadEvent::Motion {
            device: 7,
            axes: vec![
                (axis::X, 0.5),
                (axis::Y, -1.0),
                (axis::Z, 0.25),
                (axis::RZ, 0.75),
                (axis::HAT_X, 1.0),
                (axis::RTRIGGER, 0.9),
            ],
        });
        let (pressed, axes) = snapshot(&mut t);
        // A, Start, right trigger, D-pad right.
        assert_eq!(pressed, [0, 7, 9, 15]);
        // Up is -1 on Android and +1 in the engine.
        assert_eq!(axes, [0.5, 1.0, 0.25, -0.75]);

        t.apply(PadEvent::Key { device: 7, code: keycode::BUTTON_A, down: false });
        assert_eq!(snapshot(&mut t).0, [7, 9, 15]);
    }

    #[test]
    fn a_tap_between_two_frames_is_seen_once() {
        let mut t = PadTracker::default();
        t.apply(PadEvent::Key { device: 2, code: keycode::BUTTON_A, down: true });
        t.apply(PadEvent::Key { device: 2, code: keycode::BUTTON_A, down: false });
        assert_eq!(snapshot(&mut t).0, [0], "held for the frame it happened in");
        assert!(snapshot(&mut t).0.is_empty(), "and only that one");
    }

    #[test]
    fn a_right_stick_on_rx_ry_is_found_too() {
        let mut t = PadTracker::default();
        t.apply(PadEvent::Motion { device: 1, axes: vec![(axis::RX, -0.6), (axis::RY, 0.4)] });
        assert_eq!(snapshot(&mut t).1, [0.0, 0.0, -0.6, -0.4]);
    }

    #[test]
    fn controllers_keep_their_slots_and_a_gone_one_lets_go() {
        let mut t = PadTracker::default();
        t.apply(PadEvent::Key { device: 30, code: keycode::BUTTON_B, down: true });
        t.apply(PadEvent::Key { device: 12, code: keycode::BUTTON_X, down: true });
        let mut input = InputState::new();
        t.publish(&mut input);
        // First seen is Pad0, whatever its device id.
        assert!(input.pad_state(0).unwrap().pressed.contains(&1));
        assert!(input.pad_state(1).unwrap().pressed.contains(&2));

        t.remove(30);
        t.publish(&mut input);
        assert!(!input.pad_state(0).unwrap().connected);
        assert!(input.pad_state(0).unwrap().pressed.is_empty(), "an unplugged pad holds nothing");
        assert!(input.pad_state(1).unwrap().connected);

        // Back again: the same slot.
        t.apply(PadEvent::Key { device: 30, code: keycode::BUTTON_Y, down: true });
        t.publish(&mut input);
        assert!(input.pad_state(0).unwrap().connected && input.pad_state(0).unwrap().pressed.contains(&3));
    }
}
