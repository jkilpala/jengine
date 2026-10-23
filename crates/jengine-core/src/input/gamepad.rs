//! Gamepad state and binding names.
//!
//! The action layer resolves a binding by *name*: `is_action_pressed` walks
//! the action's key list and asks whether each name is currently down. That
//! makes gamepad support an addition to the name space rather than a second
//! input system — an action can bind `"Space"` and `"Pad0_A"` at once, and a
//! rebinding menu neither knows nor cares which is which.
//!
//! ## Binding names
//!
//! | Form | Meaning |
//! |---|---|
//! | `Pad0_A` | Button `A` on pad 0 |
//! | `Pad0_LeftStickX+` | Left stick X pushed **positive** (right) past the dead zone |
//! | `Pad0_LeftStickY-` | Left stick Y pushed **negative** (down) past the dead zone |
//! | `Pad0_LeftStickY+` | Left stick pushed **up** |
//! | `Pad0_LeftTrigger+` | Trigger past the dead zone |
//!
//! Pad index is zero-based. An axis binding must carry `+` or `-`: a stick
//! has no meaningful "pressed" without a direction, and silently picking one
//! would make `Pad0_LeftStickX` mean "right" for no stated reason.
//!
//! Button and axis names follow the W3C Standard Gamepad layout, which is
//! what browsers report and what `gilrs` maps to natively, so one name means
//! the same thing on both.
//!
//! ## Stick Y is positive **up**
//!
//! The one place the engine departs from the W3C layout. The world is y-up,
//! every game here was written expecting up to be `+` (`move_up` bound to
//! `LeftStickY+`), and `gilrs` reports it that way - but a browser and
//! Android report stick Y positive *down*. Until Sep 2026 the desktop backend
//! flipped gilrs to the browser's convention, so every binding and script
//! that assumed up was `+` - all of them - moved the wrong way on a real
//! controller, on every runtime. Nobody had plugged one in. A backend whose
//! source is positive-down converts through [`stick_y_up`]; no other place
//! flips a sign.
//!
//! ## Dead zone
//!
//! Axis bindings compare `|value|` against the action's dead zone (the
//! `dead_zone` field on `InputAction`, settable from scripts). Zero means
//! "unset", not "trigger on any drift" — a stick at rest reports small
//! non-zero values, so an unset dead zone falls back to
//! [`DEFAULT_AXIS_DEAD_ZONE`].

use std::collections::HashSet;

/// Used when an action does not set its own dead zone. Sticks rest at small
/// non-zero values, so 0.0 would leave an action permanently "pressed".
pub const DEFAULT_AXIS_DEAD_ZONE: f32 = 0.35;

/// Buttons in W3C Standard Gamepad order. The index *is* the browser's
/// `Gamepad.buttons` index, so the web backend needs no mapping table.
pub const BUTTON_NAMES: &[&str] = &[
    "A",          // 0  — south (Cross)
    "B",          // 1  — east  (Circle)
    "X",          // 2  — west  (Square)
    "Y",          // 3  — north (Triangle)
    "LeftBumper", // 4
    "RightBumper",// 5
    "LeftTrigger",// 6  — also reported as an axis-like button
    "RightTrigger",// 7
    "Back",       // 8  — Select / View
    "Start",      // 9  — Options / Menu
    "LeftStick",  // 10 — stick click
    "RightStick", // 11
    "DPadUp",     // 12
    "DPadDown",   // 13
    "DPadLeft",   // 14
    "DPadRight",  // 15
    "Guide",      // 16 — Home / PS button
];

/// Axes in W3C Standard Gamepad order; the index is the browser's
/// `Gamepad.axes` index.
pub const AXIS_NAMES: &[&str] = &["LeftStickX", "LeftStickY", "RightStickX", "RightStickY"];

/// The indices of the two stick-Y axes in [`AXIS_NAMES`].
pub const STICK_Y_AXES: [usize; 2] = [1, 3];

/// Turn axes from a positive-down source (a browser, Android) into the
/// engine's positive-up convention. See the module docs.
pub fn stick_y_up(axes: &mut [f32]) {
    for i in STICK_Y_AXES {
        if let Some(v) = axes.get_mut(i) {
            // `0.0 - v`, not `-v`: a centred stick stays 0.0 rather than -0.0,
            // which a script printing it shows as "-0.00".
            *v = 0.0 - *v;
        }
    }
}

/// One connected pad's state for the current frame.
#[derive(Debug, Clone, Default)]
pub struct GamepadState {
    /// Buttons held right now, by index.
    pub pressed: HashSet<usize>,
    /// Buttons that went down this frame.
    pub just_pressed: HashSet<usize>,
    /// Buttons that came up this frame.
    pub just_released: HashSet<usize>,
    /// Axis values in `-1.0..=1.0`, by index.
    pub axes: Vec<f32>,
    /// Previous frame's axis values. An axis has no press edge of its own, so
    /// "just pressed" for an axis binding means it crossed the dead zone
    /// between these two frames.
    pub prev_axes: Vec<f32>,
    /// Whether the pad is currently connected. A disconnected pad keeps its
    /// slot so indices stay stable across a reconnect.
    pub connected: bool,
    /// What the platform calls it (`"Xbox Wireless Controller"`), empty when
    /// the backend could not say.
    pub name: String,
    /// Its USB vendor id, when the backend knows it.
    pub vendor: Option<u16>,
}

impl GamepadState {
    /// Replace this frame's snapshot, deriving the edges from the previous
    /// one. Backends call this once per frame per pad.
    pub fn update(&mut self, pressed: HashSet<usize>, axes: Vec<f32>) {
        self.just_pressed = pressed.difference(&self.pressed).copied().collect();
        self.just_released = self.pressed.difference(&pressed).copied().collect();
        self.pressed = pressed;
        self.prev_axes = std::mem::replace(&mut self.axes, axes);
        self.connected = true;
    }

    /// Clear edges without clearing held buttons. Called at the end of a
    /// frame, mirroring `InputState::begin_frame`.
    pub fn clear_edges(&mut self) {
        self.just_pressed.clear();
        self.just_released.clear();
    }

    /// Mark the pad gone: nothing is held, nothing is deflected.
    pub fn disconnect(&mut self) {
        self.just_released = std::mem::take(&mut self.pressed);
        self.just_pressed.clear();
        self.prev_axes = std::mem::take(&mut self.axes);
        self.connected = false;
    }

    /// Which family it is, for prompts: Xbox, PlayStation, Switch or generic.
    pub fn kind(&self) -> super::prompts::PadKind {
        super::prompts::pad_kind(&self.name, self.vendor)
    }

    pub fn axis(&self, index: usize) -> f32 {
        self.axes.get(index).copied().unwrap_or(0.0)
    }

    /// Previous frame's value for `index`, for axis edge detection.
    pub fn prev_axis(&self, index: usize) -> f32 {
        self.prev_axes.get(index).copied().unwrap_or(0.0)
    }
}

/// What a `Pad…` binding name refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadBinding {
    Button { pad: usize, index: usize },
    /// `positive` is the direction the axis must be pushed for the binding to
    /// count as pressed.
    Axis {
        pad: usize,
        index: usize,
        positive: bool,
    },
}

/// Parse a binding name, or `None` when it is not a gamepad binding at all
/// (the overwhelmingly common case — every keyboard key takes this path).
pub fn parse_binding(name: &str) -> Option<PadBinding> {
    let rest = name.strip_prefix("Pad")?;
    let (pad_str, rest) = rest.split_once('_')?;
    let pad: usize = pad_str.parse().ok()?;

    // Axis bindings end in an explicit direction.
    if let Some(axis_name) = rest.strip_suffix('+').or_else(|| rest.strip_suffix('-')) {
        let positive = rest.ends_with('+');
        if let Some(index) = AXIS_NAMES.iter().position(|a| *a == axis_name) {
            return Some(PadBinding::Axis {
                pad,
                index,
                positive,
            });
        }
        // Triggers are reported as buttons but are natural to bind with a
        // direction, so `Pad0_LeftTrigger+` resolves to the button.
        if let Some(index) = BUTTON_NAMES.iter().position(|b| *b == axis_name) {
            return Some(PadBinding::Button { pad, index });
        }
        return None;
    }

    BUTTON_NAMES
        .iter()
        .position(|b| *b == rest)
        .map(|index| PadBinding::Button { pad, index })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_button_and_axis_names() {
        assert_eq!(
            parse_binding("Pad0_A"),
            Some(PadBinding::Button { pad: 0, index: 0 })
        );
        assert_eq!(
            parse_binding("Pad1_DPadLeft"),
            Some(PadBinding::Button { pad: 1, index: 14 })
        );
        assert_eq!(
            parse_binding("Pad0_LeftStickX+"),
            Some(PadBinding::Axis {
                pad: 0,
                index: 0,
                positive: true
            })
        );
        assert_eq!(
            parse_binding("Pad0_RightStickY-"),
            Some(PadBinding::Axis {
                pad: 0,
                index: 3,
                positive: false
            })
        );
    }

    #[test]
    fn a_trigger_with_a_direction_resolves_to_its_button() {
        // Triggers arrive as buttons, but binding one reads naturally with a
        // direction, so both spellings work.
        assert_eq!(
            parse_binding("Pad0_LeftTrigger+"),
            Some(PadBinding::Button { pad: 0, index: 6 })
        );
        assert_eq!(
            parse_binding("Pad0_LeftTrigger"),
            Some(PadBinding::Button { pad: 0, index: 6 })
        );
    }

    #[test]
    fn keyboard_names_are_not_gamepad_bindings() {
        // Every keyboard binding goes through this function first, so it has
        // to reject cheaply and without surprises.
        for name in ["Space", "W", "1", "Padlock", "Pad", "PadX_A", "Pad0_Nope"] {
            assert_eq!(parse_binding(name), None, "{name} parsed as a pad binding");
        }
    }

    #[test]
    fn an_axis_without_a_direction_is_rejected() {
        // "Pushed which way?" has no good default, so it is not guessed.
        assert_eq!(parse_binding("Pad0_LeftStickX"), None);
    }

    #[test]
    fn update_derives_edges_from_the_previous_frame() {
        let mut pad = GamepadState::default();
        pad.update(HashSet::from([0, 1]), vec![0.0, 0.0, 0.0, 0.0]);
        assert!(pad.just_pressed.contains(&0));
        assert!(pad.pressed.contains(&1));

        pad.clear_edges();
        pad.update(HashSet::from([1]), vec![0.0, 0.0, 0.0, 0.0]);
        assert!(pad.just_released.contains(&0), "0 was released");
        assert!(!pad.just_pressed.contains(&1), "1 was already held");
    }

    #[test]
    fn disconnect_releases_everything_held() {
        // Otherwise an action stays pressed forever when a controller's
        // battery dies mid-input.
        let mut pad = GamepadState::default();
        pad.update(HashSet::from([3]), vec![1.0, 0.0, 0.0, 0.0]);
        pad.clear_edges();
        pad.disconnect();
        assert!(pad.pressed.is_empty());
        assert!(pad.just_released.contains(&3));
        assert_eq!(pad.axis(0), 0.0);
        assert!(!pad.connected);
    }
}
