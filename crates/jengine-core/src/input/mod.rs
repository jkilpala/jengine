//! Input abstraction — keyboard, mouse, and action mapping.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// Gamepad state, binding names and dead-zone handling.
pub mod gamepad;
pub mod gestures;
pub mod haptics;
/// Hold, tap and double-tap on actions, and the view of input scripts query.
pub mod interactions;
/// Controller families and what their buttons are called, for prompts.
pub mod prompts;
pub mod replay;
/// What a text field copies, for the host to put on the system clipboard.
pub mod clipboard;
/// Which text field is being written in, for IME and soft keyboards.
pub mod text_entry;
/// Browser gamepad polling, shared by the editor viewport and web runtime.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod gamepad_web;
pub mod asset;
pub use asset::{INPUT_MAP_VERSION, InputMap};
/// On-screen sticks and buttons that press keys on a touch screen.
pub mod touch_controls;

/// Keyboard key codes — full set for a standard keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    // Letters
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    // Digits
    Digit0, Digit1, Digit2, Digit3, Digit4,
    Digit5, Digit6, Digit7, Digit8, Digit9,
    // Function keys
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    // Arrow keys
    Left, Right, Up, Down,
    // Modifiers
    ShiftLeft, ShiftRight,
    ControlLeft, ControlRight,
    AltLeft, AltRight,
    // Special
    Space, Enter, Escape, Backspace, Tab,
    Insert, Delete, Home, End, PageUp, PageDown,
    // Punctuation / misc
    Minus, Equal, BracketLeft, BracketRight, Backslash,
    Semicolon, Quote, Backquote, Comma, Period, Slash,
    CapsLock, NumLock, ScrollLock, PrintScreen, Pause,
    // Numpad
    Numpad0, Numpad1, Numpad2, Numpad3, Numpad4,
    Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
    NumpadAdd, NumpadSubtract, NumpadMultiply, NumpadDivide,
    NumpadDecimal, NumpadEnter,
}

/// All key codes — used by snapshot_input to iterate over every key.
pub const ALL_KEYS: &[KeyCode] = &[
    KeyCode::A, KeyCode::B, KeyCode::C, KeyCode::D, KeyCode::E,
    KeyCode::F, KeyCode::G, KeyCode::H, KeyCode::I, KeyCode::J,
    KeyCode::K, KeyCode::L, KeyCode::M, KeyCode::N, KeyCode::O,
    KeyCode::P, KeyCode::Q, KeyCode::R, KeyCode::S, KeyCode::T,
    KeyCode::U, KeyCode::V, KeyCode::W, KeyCode::X, KeyCode::Y,
    KeyCode::Z,
    KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3,
    KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7,
    KeyCode::Digit8, KeyCode::Digit9,
    KeyCode::F1, KeyCode::F2, KeyCode::F3, KeyCode::F4,
    KeyCode::F5, KeyCode::F6, KeyCode::F7, KeyCode::F8,
    KeyCode::F9, KeyCode::F10, KeyCode::F11, KeyCode::F12,
    KeyCode::Left, KeyCode::Right, KeyCode::Up, KeyCode::Down,
    KeyCode::ShiftLeft, KeyCode::ShiftRight,
    KeyCode::ControlLeft, KeyCode::ControlRight,
    KeyCode::AltLeft, KeyCode::AltRight,
    KeyCode::Space, KeyCode::Enter, KeyCode::Escape, KeyCode::Backspace, KeyCode::Tab,
    KeyCode::Insert, KeyCode::Delete, KeyCode::Home, KeyCode::End,
    KeyCode::PageUp, KeyCode::PageDown,
    KeyCode::Minus, KeyCode::Equal, KeyCode::BracketLeft, KeyCode::BracketRight,
    KeyCode::Backslash, KeyCode::Semicolon, KeyCode::Quote, KeyCode::Backquote,
    KeyCode::Comma, KeyCode::Period, KeyCode::Slash,
    KeyCode::CapsLock, KeyCode::NumLock, KeyCode::ScrollLock,
    KeyCode::PrintScreen, KeyCode::Pause,
    KeyCode::Numpad0, KeyCode::Numpad1, KeyCode::Numpad2, KeyCode::Numpad3,
    KeyCode::Numpad4, KeyCode::Numpad5, KeyCode::Numpad6, KeyCode::Numpad7,
    KeyCode::Numpad8, KeyCode::Numpad9,
    KeyCode::NumpadAdd, KeyCode::NumpadSubtract, KeyCode::NumpadMultiply,
    KeyCode::NumpadDivide, KeyCode::NumpadDecimal, KeyCode::NumpadEnter,
];

impl KeyCode {
    /// String name used by scripts (e.g. `is_key_pressed("W")`).
    pub fn name(&self) -> &'static str {
        match self {
            KeyCode::A => "A", KeyCode::B => "B", KeyCode::C => "C",
            KeyCode::D => "D", KeyCode::E => "E", KeyCode::F => "F",
            KeyCode::G => "G", KeyCode::H => "H", KeyCode::I => "I",
            KeyCode::J => "J", KeyCode::K => "K", KeyCode::L => "L",
            KeyCode::M => "M", KeyCode::N => "N", KeyCode::O => "O",
            KeyCode::P => "P", KeyCode::Q => "Q", KeyCode::R => "R",
            KeyCode::S => "S", KeyCode::T => "T", KeyCode::U => "U",
            KeyCode::V => "V", KeyCode::W => "W", KeyCode::X => "X",
            KeyCode::Y => "Y", KeyCode::Z => "Z",
            KeyCode::Digit0 => "0", KeyCode::Digit1 => "1", KeyCode::Digit2 => "2",
            KeyCode::Digit3 => "3", KeyCode::Digit4 => "4", KeyCode::Digit5 => "5",
            KeyCode::Digit6 => "6", KeyCode::Digit7 => "7", KeyCode::Digit8 => "8",
            KeyCode::Digit9 => "9",
            KeyCode::F1 => "F1", KeyCode::F2 => "F2", KeyCode::F3 => "F3",
            KeyCode::F4 => "F4", KeyCode::F5 => "F5", KeyCode::F6 => "F6",
            KeyCode::F7 => "F7", KeyCode::F8 => "F8", KeyCode::F9 => "F9",
            KeyCode::F10 => "F10", KeyCode::F11 => "F11", KeyCode::F12 => "F12",
            KeyCode::Left => "Left", KeyCode::Right => "Right",
            KeyCode::Up => "Up", KeyCode::Down => "Down",
            KeyCode::ShiftLeft => "ShiftLeft", KeyCode::ShiftRight => "ShiftRight",
            KeyCode::ControlLeft => "ControlLeft", KeyCode::ControlRight => "ControlRight",
            KeyCode::AltLeft => "AltLeft", KeyCode::AltRight => "AltRight",
            KeyCode::Space => "Space", KeyCode::Enter => "Enter",
            KeyCode::Escape => "Escape", KeyCode::Backspace => "Backspace",
            KeyCode::Tab => "Tab",
            KeyCode::Insert => "Insert", KeyCode::Delete => "Delete",
            KeyCode::Home => "Home", KeyCode::End => "End",
            KeyCode::PageUp => "PageUp", KeyCode::PageDown => "PageDown",
            KeyCode::Minus => "Minus", KeyCode::Equal => "Equal",
            KeyCode::BracketLeft => "BracketLeft", KeyCode::BracketRight => "BracketRight",
            KeyCode::Backslash => "Backslash", KeyCode::Semicolon => "Semicolon",
            KeyCode::Quote => "Quote", KeyCode::Backquote => "Backquote",
            KeyCode::Comma => "Comma", KeyCode::Period => "Period",
            KeyCode::Slash => "Slash",
            KeyCode::CapsLock => "CapsLock", KeyCode::NumLock => "NumLock",
            KeyCode::ScrollLock => "ScrollLock", KeyCode::PrintScreen => "PrintScreen",
            KeyCode::Pause => "Pause",
            KeyCode::Numpad0 => "Numpad0", KeyCode::Numpad1 => "Numpad1",
            KeyCode::Numpad2 => "Numpad2", KeyCode::Numpad3 => "Numpad3",
            KeyCode::Numpad4 => "Numpad4", KeyCode::Numpad5 => "Numpad5",
            KeyCode::Numpad6 => "Numpad6", KeyCode::Numpad7 => "Numpad7",
            KeyCode::Numpad8 => "Numpad8", KeyCode::Numpad9 => "Numpad9",
            KeyCode::NumpadAdd => "NumpadAdd", KeyCode::NumpadSubtract => "NumpadSubtract",
            KeyCode::NumpadMultiply => "NumpadMultiply", KeyCode::NumpadDivide => "NumpadDivide",
            KeyCode::NumpadDecimal => "NumpadDecimal", KeyCode::NumpadEnter => "NumpadEnter",
        }
    }

    /// Map a browser `KeyboardEvent.code` to a [`KeyCode`].
    ///
    /// The W3C code names are [`KeyCode::name`] for everything except letters,
    /// digits and arrows, which carry a `Key` / `Digit` / `Arrow` prefix - so
    /// stripping those three and deferring to [`KeyCode::from_name`] covers
    /// the whole enum, including the parts a hand-written table forgets.
    ///
    /// Both browser runtimes used to keep their own `match`, and both listed
    /// the same partial set: arrows, space, enter, escape, tab, modifiers,
    /// letters and digits. Everything else was silently dropped, so in the
    /// editor and on the web **Backspace never arrived** - which is the key
    /// GUI and entity-UI text fields delete with - and neither did PageUp /
    /// PageDown or the brackets, which is what the test lab tells you to press
    /// to walk its scenes.
    ///
    /// Since [`KeyCode::from_name`] accepts those spellings too, this is now
    /// the same function under the name the browser hosts call it by.
    pub fn from_browser_code(code: &str) -> Option<Self> {
        Self::from_name(code)
    }

    /// Parse a key name back to a KeyCode.
    ///
    /// Accepts the canonical [`KeyCode::name`] (`"Up"`, `"W"`, `"1"`) and the
    /// W3C `KeyboardEvent.code` spelling of the same key (`"ArrowUp"`,
    /// `"KeyW"`, `"Digit1"`). Input actions resolve their bindings through
    /// here, and until Sep 2026 only the canonical form was accepted - so
    /// `"ArrowUp"`, which three projects had written into `project.jengine`
    /// by hand, bound nothing and the arrow keys silently did nothing in
    /// every runtime. [`KeyCode::name`] still returns the canonical form.
    pub fn from_name(name: &str) -> Option<Self> {
        if let Some(key) = Self::from_canonical_name(name) {
            return Some(key);
        }
        // Each prefix names one kind of key, so `ArrowSpace` is not Space.
        let one = |rest: &str, kind: fn(char) -> bool| {
            (rest.len() == 1 && rest.chars().all(kind))
                .then(|| Self::from_canonical_name(rest))
                .flatten()
        };
        if let Some(rest) = name.strip_prefix("Key") {
            return one(rest, |c| c.is_ascii_uppercase());
        }
        if let Some(rest) = name.strip_prefix("Digit") {
            return one(rest, |c| c.is_ascii_digit());
        }
        name.strip_prefix("Arrow")
            .and_then(Self::from_canonical_name)
            .filter(|k| matches!(k, KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down))
    }

    fn from_canonical_name(name: &str) -> Option<Self> {
        match name {
            "A" => Some(KeyCode::A), "B" => Some(KeyCode::B), "C" => Some(KeyCode::C),
            "D" => Some(KeyCode::D), "E" => Some(KeyCode::E), "F" => Some(KeyCode::F),
            "G" => Some(KeyCode::G), "H" => Some(KeyCode::H), "I" => Some(KeyCode::I),
            "J" => Some(KeyCode::J), "K" => Some(KeyCode::K), "L" => Some(KeyCode::L),
            "M" => Some(KeyCode::M), "N" => Some(KeyCode::N), "O" => Some(KeyCode::O),
            "P" => Some(KeyCode::P), "Q" => Some(KeyCode::Q), "R" => Some(KeyCode::R),
            "S" => Some(KeyCode::S), "T" => Some(KeyCode::T), "U" => Some(KeyCode::U),
            "V" => Some(KeyCode::V), "W" => Some(KeyCode::W), "X" => Some(KeyCode::X),
            "Y" => Some(KeyCode::Y), "Z" => Some(KeyCode::Z),
            "0" => Some(KeyCode::Digit0), "1" => Some(KeyCode::Digit1),
            "2" => Some(KeyCode::Digit2), "3" => Some(KeyCode::Digit3),
            "4" => Some(KeyCode::Digit4), "5" => Some(KeyCode::Digit5),
            "6" => Some(KeyCode::Digit6), "7" => Some(KeyCode::Digit7),
            "8" => Some(KeyCode::Digit8), "9" => Some(KeyCode::Digit9),
            "F1" => Some(KeyCode::F1), "F2" => Some(KeyCode::F2), "F3" => Some(KeyCode::F3),
            "F4" => Some(KeyCode::F4), "F5" => Some(KeyCode::F5), "F6" => Some(KeyCode::F6),
            "F7" => Some(KeyCode::F7), "F8" => Some(KeyCode::F8), "F9" => Some(KeyCode::F9),
            "F10" => Some(KeyCode::F10), "F11" => Some(KeyCode::F11), "F12" => Some(KeyCode::F12),
            "Left" => Some(KeyCode::Left), "Right" => Some(KeyCode::Right),
            "Up" => Some(KeyCode::Up), "Down" => Some(KeyCode::Down),
            "ShiftLeft" => Some(KeyCode::ShiftLeft), "ShiftRight" => Some(KeyCode::ShiftRight),
            "ControlLeft" => Some(KeyCode::ControlLeft), "ControlRight" => Some(KeyCode::ControlRight),
            "AltLeft" => Some(KeyCode::AltLeft), "AltRight" => Some(KeyCode::AltRight),
            "Space" => Some(KeyCode::Space), "Enter" => Some(KeyCode::Enter),
            "Escape" => Some(KeyCode::Escape), "Backspace" => Some(KeyCode::Backspace),
            "Tab" => Some(KeyCode::Tab),
            "Insert" => Some(KeyCode::Insert), "Delete" => Some(KeyCode::Delete),
            "Home" => Some(KeyCode::Home), "End" => Some(KeyCode::End),
            "PageUp" => Some(KeyCode::PageUp), "PageDown" => Some(KeyCode::PageDown),
            "Minus" => Some(KeyCode::Minus), "Equal" => Some(KeyCode::Equal),
            "BracketLeft" => Some(KeyCode::BracketLeft), "BracketRight" => Some(KeyCode::BracketRight),
            "Backslash" => Some(KeyCode::Backslash), "Semicolon" => Some(KeyCode::Semicolon),
            "Quote" => Some(KeyCode::Quote), "Backquote" => Some(KeyCode::Backquote),
            "Comma" => Some(KeyCode::Comma), "Period" => Some(KeyCode::Period),
            "Slash" => Some(KeyCode::Slash),
            "CapsLock" => Some(KeyCode::CapsLock), "NumLock" => Some(KeyCode::NumLock),
            "ScrollLock" => Some(KeyCode::ScrollLock), "PrintScreen" => Some(KeyCode::PrintScreen),
            "Pause" => Some(KeyCode::Pause),
            "Numpad0" => Some(KeyCode::Numpad0), "Numpad1" => Some(KeyCode::Numpad1),
            "Numpad2" => Some(KeyCode::Numpad2), "Numpad3" => Some(KeyCode::Numpad3),
            "Numpad4" => Some(KeyCode::Numpad4), "Numpad5" => Some(KeyCode::Numpad5),
            "Numpad6" => Some(KeyCode::Numpad6), "Numpad7" => Some(KeyCode::Numpad7),
            "Numpad8" => Some(KeyCode::Numpad8), "Numpad9" => Some(KeyCode::Numpad9),
            "NumpadAdd" => Some(KeyCode::NumpadAdd), "NumpadSubtract" => Some(KeyCode::NumpadSubtract),
            "NumpadMultiply" => Some(KeyCode::NumpadMultiply), "NumpadDivide" => Some(KeyCode::NumpadDivide),
            "NumpadDecimal" => Some(KeyCode::NumpadDecimal), "NumpadEnter" => Some(KeyCode::NumpadEnter),
            _ => None,
        }
    }
}

/// A named input action bound to one or more keys (e.g. "jump" → [Space, W]).
///
/// A binding is a key, a pad button (`Pad0_A`) or a stick or trigger with a
/// direction (`Pad0_LeftStickX+`) - see `gamepad`. `dead_zone` is how far an
/// axis binding must be pushed to count (0: `gamepad::DEFAULT_AXIS_DEAD_ZONE`);
/// keys ignore it. Defaults to `0.0` so existing `project.jengine` files and
/// `.input` assets without the field deserialize cleanly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputAction {
    pub name: String,
    pub keys: Vec<String>,
    #[serde(default)]
    pub dead_zone: f32,
}

/// Where a finger is in its life, this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchStage {
    /// It landed this frame.
    Began,
    /// It moved this frame.
    Moved,
    /// It is down and did not move.
    Held,
    /// It lifted this frame - reported once, then it is gone. A tap shorter
    /// than a frame is only ever seen here.
    Ended,
}

impl TouchStage {
    /// `"began"`, `"moved"`, `"held"`, `"ended"` - what scripts read.
    pub fn name(self) -> &'static str {
        match self {
            TouchStage::Began => "began",
            TouchStage::Moved => "moved",
            TouchStage::Held => "held",
            TouchStage::Ended => "ended",
        }
    }
}

/// One finger on a touch screen.
///
/// The first finger that lands on nothing is also the left mouse button, as
/// it always has been; this is every finger, so a game can pinch, play a
/// chord or tap two things at once. Kept by
/// [`touch_controls::TouchControlsState::tick`], which every host calls, so
/// there is one place that decides what a finger is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Touch {
    /// Stable for the finger's life; reused by the platform afterwards.
    pub id: u64,
    /// Where it is, in the units and orientation of `mouse_position`
    /// (window pixels, y down).
    pub position: (f32, f32),
    /// Where it landed.
    pub start: (f32, f32),
    pub stage: TouchStage,
    /// It landed on an on-screen control, which is pressing keys with it.
    pub on_control: bool,
}

/// Tracks the current frame's input state, including action mapping.
#[derive(Debug, Default)]
pub struct InputState {
    pressed: HashSet<KeyCode>,
    just_pressed: HashSet<KeyCode>,
    just_released: HashSet<KeyCode>,
    pub mouse_position: (f32, f32),
    /// Render surface size in window coordinates. The runtime updates
    /// this each frame from the swap-chain / canvas size so scripts can
    /// convert mouse coords to world coords without DPI guesswork.
    pub screen_size: (f32, f32),
    pub mouse_left: bool,
    pub mouse_right: bool,
    pub mouse_left_just_pressed: bool,
    pub mouse_right_just_pressed: bool,
    /// Whether the right mouse button was just released this frame.
    pub mouse_right_just_released: bool,
    /// The middle button (the wheel pressed), the same three ways (Oct
    /// 2026) - a pan, a "place another", a 3D tool's orbit.
    pub mouse_middle: bool,
    pub mouse_middle_just_pressed: bool,
    pub mouse_middle_just_released: bool,
    /// Falling edge of the left button — true for the single frame the button
    /// goes down→up. Drag interactions need to know where a drag *ends*, not
    /// just where it starts, and the press edge alone cannot express that.
    pub mouse_left_just_released: bool,
    /// Whether a GUI scroll gesture owns the pointer this frame.
    ///
    /// Set once a frame by the runtime from its
    /// [`crate::ui::scroll::GuiScrollState`]. Everything that would otherwise
    /// treat the release as a click reads it, so dragging a list of buttons
    /// scrolls without activating the one the drag started on.
    ///
    /// It lives here because the click dispatch is buried inside
    /// `ScriptSystem::update`, which sees an `InputState` and nothing else.
    pub gui_scroll_dragging: bool,
    pub mouse_wheel_delta: f32,
    /// How far the mouse moved this frame, in pixels (x right, y down), from
    /// the device's raw motion where the host has it - so it keeps counting
    /// at the window edge and while the pointer is locked
    /// (`Cursor.set_locked`). A spinner or a trackball reads this, not the
    /// difference of two `mouse_position`s, which stops at the edge and jumps
    /// when the pointer enters the window.
    pub mouse_delta: (f32, f32),
    /// Characters typed this frame, in order.
    ///
    /// Separate from the key state on purpose: `KeyCode` is a physical key,
    /// while text entry needs what the *layout* produced — shift, dead keys,
    /// AltGr and IME all mean "which key" and "which character" are different
    /// questions. Runtimes push here from their platform's text event
    /// (`KeyEvent::text` on winit, `keydown`'s single-char `key` in a
    /// browser). Control characters are filtered out by the pushers; editing
    /// keys travel as `KeyCode` like any other key.
    ///
    /// Cleared every `begin_frame`, like `just_pressed`.
    pub typed_chars: Vec<char>,
    /// Action map: action name → list of key names.
    actions: HashMap<String, Vec<String>>,
    /// 2.5 — default bindings captured at load time. `reset_bindings()`
    /// restores `actions` from this map. Cleared & rebuilt on every
    /// `load_actions`.
    defaults: HashMap<String, Vec<String>>,
    /// 2.5 — per-action dead-zone (gamepad axes). Keyboard ignores it;
    /// scripts may read it via `input_get_dead_zone(name)` and the
    /// gamepad layer (future) will threshold axis magnitude against it.
    dead_zones: HashMap<String, f32>,
    /// Connected gamepads, indexed by pad number. Filled by whichever
    /// backend the host runs (`gilrs` natively, the Gamepad API in a
    /// browser); empty when nothing polls, which is why every gamepad
    /// binding simply never matches rather than erroring.
    pads: Vec<gamepad::GamepadState>,
    /// Which menu direction is held and when it next repeats
    /// (`ui::navigation::read_intent`).
    pub ui_nav: crate::ui::navigation::NavRepeat,
    /// Every finger on the screen this frame, in the order they landed -
    /// see [`Touch`]. Empty without a touch screen.
    pub touches: Vec<Touch>,
    /// This frame's gestures (`input::gestures`), recognised from the
    /// fingers in `update_interactions`.
    pub gestures: Vec<gestures::Gesture>,
    gesture_state: gestures::GestureRecognizer,
    /// Keys the platform repeated this frame (held long enough), which a
    /// text field treats as pressed again - Backspace held deletes on.
    repeated: HashSet<KeyCode>,
    /// Text pasted this frame (the host read the clipboard on ctrl+V).
    pasted: String,
    /// Each action's hold / tap / double-tap timing, brought up to date by
    /// [`Self::update_interactions`].
    interactions: HashMap<String, interactions::ActionTiming>,
    /// The device the player used last - a key, a mouse button, a pad, a
    /// finger - which button prompts follow. Movement does not count: a
    /// mouse nudged under a pad player should not swap every prompt.
    pub last_device: interactions::Device,
    /// The pad used last, when one has been.
    pub last_pad: Option<usize>,
}

impl InputState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load action mappings from a list of InputAction definitions. The
    /// provided list is captured as `defaults` so `reset_bindings()` can
    /// restore it later.
    pub fn load_actions(&mut self, actions: &[InputAction]) {
        self.actions.clear();
        self.defaults.clear();
        self.dead_zones.clear();
        for action in actions {
            self.actions.insert(action.name.clone(), action.keys.clone());
            self.defaults.insert(action.name.clone(), action.keys.clone());
            if action.dead_zone != 0.0 {
                self.dead_zones.insert(action.name.clone(), action.dead_zone);
            }
        }
    }

    /// 2.5 — replace every binding for `action`. Unknown actions are
    /// created on the fly (lets scripts introduce new actions at runtime
    /// without a config file). `defaults` is left untouched so
    /// `reset_bindings` still restores the asset-loaded list.
    pub fn set_bindings(&mut self, action: &str, keys: Vec<String>) {
        self.actions.insert(action.to_string(), keys);
    }

    /// 2.5 — append `key` to `action`'s binding list (no-op if already bound).
    pub fn add_binding(&mut self, action: &str, key: &str) {
        let entry = self.actions.entry(action.to_string()).or_default();
        if !entry.iter().any(|k| k == key) {
            entry.push(key.to_string());
        }
    }

    /// 2.5 — remove `key` from `action`'s binding list (no-op if missing).
    pub fn remove_binding(&mut self, action: &str, key: &str) {
        if let Some(list) = self.actions.get_mut(action) {
            list.retain(|k| k != key);
        }
    }

    /// 2.5 — drop all keys for `action` (action still exists, just unbound).
    pub fn clear_bindings(&mut self, action: &str) {
        if let Some(list) = self.actions.get_mut(action) {
            list.clear();
        }
    }

    /// 2.5 — restore every action to the bindings supplied to the most
    /// recent `load_actions`. Actions introduced at runtime via
    /// `set_bindings` / `add_binding` for names not in the defaults are
    /// dropped, matching Unity/Godot reset semantics.
    pub fn reset_bindings(&mut self) {
        self.actions.clear();
        for (name, keys) in &self.defaults {
            self.actions.insert(name.clone(), keys.clone());
        }
    }

    /// 2.5 — read the current binding list for `action`.
    pub fn get_bindings(&self, action: &str) -> Option<&Vec<String>> {
        self.actions.get(action)
    }

    /// 2.5 — set the dead-zone for `action` (clamped to `0.0..=1.0`).
    pub fn set_dead_zone(&mut self, action: &str, value: f32) {
        let v = value.clamp(0.0, 1.0);
        if v == 0.0 {
            self.dead_zones.remove(action);
        } else {
            self.dead_zones.insert(action.to_string(), v);
        }
    }

    /// 2.5 — read the dead-zone for `action` (0.0 when unset).
    pub fn get_dead_zone(&self, action: &str) -> f32 {
        self.dead_zones.get(action).copied().unwrap_or(0.0)
    }

    /// Call at the start of each frame to reset per-frame state.
    pub fn begin_frame(&mut self) {
        self.just_pressed.clear();
        self.repeated.clear();
        self.pasted.clear();
        self.just_released.clear();
        self.mouse_left_just_pressed = false;
        self.mouse_left_just_released = false;
        self.mouse_right_just_pressed = false;
        self.mouse_right_just_released = false;
        self.mouse_middle_just_pressed = false;
        self.mouse_middle_just_released = false;
        self.mouse_wheel_delta = 0.0;
        self.mouse_delta = (0.0, 0.0);
        self.typed_chars.clear();
    }

    /// Record a middle mouse button state change.
    pub fn set_middle_mouse_button(&mut self, pressed: bool) {
        if pressed && !self.mouse_middle {
            self.mouse_middle_just_pressed = true;
            self.last_device = interactions::Device::Mouse;
        }
        if !pressed && self.mouse_middle {
            self.mouse_middle_just_released = true;
        }
        self.mouse_middle = pressed;
    }

    /// Record a mouse button state change.
    pub fn set_mouse_button(&mut self, left: bool, pressed: bool) {
        if pressed {
            self.last_device = interactions::Device::Mouse;
        }
        if left {
            if pressed && !self.mouse_left {
                self.mouse_left_just_pressed = true;
            }
            if !pressed && self.mouse_left {
                self.mouse_left_just_released = true;
            }
            self.mouse_left = pressed;
        } else {
            if pressed && !self.mouse_right {
                self.mouse_right_just_pressed = true;
            }
            if !pressed && self.mouse_right {
                self.mouse_right_just_released = true;
            }
            self.mouse_right = pressed;
        }
    }

    /// Record a key press event.
    pub fn key_down(&mut self, key: KeyCode) {
        if self.pressed.insert(key) {
            self.just_pressed.insert(key);
            self.last_device = interactions::Device::Keyboard;
        } else {
            // Already down: the platform's key repeat.
            self.repeated.insert(key);
        }
    }

    /// Pressed this frame, or repeated by the platform because it is held -
    /// how a text field reads Backspace and the arrows.
    pub fn is_typed(&self, key: KeyCode) -> bool {
        self.just_pressed.contains(&key) || self.repeated.contains(&key)
    }

    /// Either Ctrl key is held.
    pub fn ctrl_held(&self) -> bool {
        self.pressed.contains(&KeyCode::ControlLeft) || self.pressed.contains(&KeyCode::ControlRight)
    }

    /// Text the host read off the clipboard for a paste this frame.
    pub fn paste(&mut self, text: &str) {
        self.pasted.push_str(text);
    }

    /// What was pasted this frame.
    pub fn pasted(&self) -> &str {
        &self.pasted
    }

    /// A text field has the keyboard: every key pressed or repeated this
    /// frame stops being a game's - except Tab and the function keys, which
    /// move focus and toggle the overlays - and what was typed is spent.
    pub fn consume_typing(&mut self) {
        let keep = |k: &KeyCode| {
            matches!(
                k,
                KeyCode::Tab
                    | KeyCode::F1 | KeyCode::F2 | KeyCode::F3 | KeyCode::F4 | KeyCode::F5 | KeyCode::F6
                    | KeyCode::F7 | KeyCode::F8 | KeyCode::F9 | KeyCode::F10 | KeyCode::F11 | KeyCode::F12
            )
        };
        self.just_pressed.retain(keep);
        self.repeated.retain(keep);
        self.typed_chars.clear();
        self.pasted.clear();
    }

    /// Record a key release event.
    pub fn key_up(&mut self, key: KeyCode) {
        if self.pressed.remove(&key) {
            self.just_released.insert(key);
        }
    }

    /// Release every held key and mouse button, as if each had come up.
    ///
    /// Call when the window or page loses focus. The platform stops sending
    /// key-up events to an unfocused window, so a key held during alt-tab (or
    /// a tab switch, or a click into another editor panel's text field) used
    /// to stay pressed until that same key was pressed and released again - a
    /// player walked into a wall indefinitely. Each release reports its
    /// `just_released` edge, because as far as the game can know the key did
    /// come up: a charge-on-hold attack should fire, not hang.
    ///
    /// Gamepads are left alone. Their backends poll whole-device snapshots,
    /// which stay correct without focus.
    pub fn release_all(&mut self) {
        for key in std::mem::take(&mut self.pressed) {
            self.just_released.insert(key);
        }
        self.just_pressed.clear();
        self.set_mouse_button(true, false);
        self.set_mouse_button(false, false);
        self.set_middle_mouse_button(false);
        for t in &mut self.touches {
            t.stage = TouchStage::Ended;
        }
    }

    /// Consume a key's press edge for the rest of the frame.
    ///
    /// Used by focused text entry: Backspace in a text field means "delete a
    /// character", and a game that also binds Backspace to something (the
    /// Engine Test Lab binds it to "back to hub") should not get both. Whoever
    /// handles the keystroke first calls this, and later readers — including
    /// `is_action_just_pressed` — no longer see it.
    ///
    /// Only the press edge is cleared; `is_key_pressed` still reports the key
    /// as held, because it is.
    pub fn consume_key(&mut self, key: KeyCode) {
        self.just_pressed.remove(&key);
    }

    /// Record a character typed this frame.
    ///
    /// Control characters (including the ones a platform reports for Enter,
    /// Tab and Backspace) are dropped: those are editing commands and reach
    /// scripts and widgets as `KeyCode`s, not as text. Without this filter a
    /// text field would happily insert a `\u{8}` when you pressed Backspace.
    pub fn type_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        self.typed_chars.push(ch);
    }

    /// Record every character in `text` (a platform's per-event string, which
    /// may hold more than one for IME commit or a paste).
    pub fn type_str(&mut self, text: &str) {
        for ch in text.chars() {
            self.type_char(ch);
        }
    }

    /// Characters typed this frame, in order.
    pub fn typed_chars(&self) -> &[char] {
        &self.typed_chars
    }

    /// Record mouse motion: adds to this frame's [`InputState::mouse_delta`].
    /// Hosts feed the device's raw motion here (winit's
    /// `DeviceEvent::MouseMotion`, a browser's `movementX` / `movementY`).
    /// Movement does not change `last_device` - only a press does.
    pub fn mouse_motion(&mut self, dx: f32, dy: f32) {
        self.mouse_delta.0 += dx;
        self.mouse_delta.1 += dy;
    }

    /// Record a mouse wheel event.
    pub fn mouse_wheel(&mut self, delta: f32) {
        self.mouse_wheel_delta += delta;
        if delta != 0.0 {
            self.last_device = interactions::Device::Mouse;
        }
    }

    /// Is the key currently held down?
    pub fn is_pressed(&self, key: KeyCode) -> bool {
        self.pressed.contains(&key)
    }

    /// Was the key pressed this frame?
    pub fn is_just_pressed(&self, key: KeyCode) -> bool {
        self.just_pressed.contains(&key)
    }

    /// Was the key released this frame?
    pub fn is_just_released(&self, key: KeyCode) -> bool {
        self.just_released.contains(&key)
    }

    /// Every input that went down this frame, as the binding name an action
    /// would hold it by (Oct 2026): keys by `KeyCode::name`, pad buttons as
    /// `Pad0_A`, and a stick pushed past the dead zone this frame as
    /// `Pad0_LeftStickX+` / `-`. Sorted, so two keys pressed in one frame
    /// come back the same way every time. What a "press a key for Jump"
    /// screen captures: `Input.get_just_pressed()`.
    pub fn just_pressed_bindings(&self) -> Vec<String> {
        let mut out: Vec<String> = self.just_pressed.iter().map(|k| k.name().to_string()).collect();
        out.sort();
        let dz = gamepad::DEFAULT_AXIS_DEAD_ZONE;
        for (p, pad) in self.pads.iter().enumerate().filter(|(_, p)| p.connected) {
            let mut buttons: Vec<usize> = pad.just_pressed.iter().copied().collect();
            buttons.sort();
            for b in buttons {
                if let Some(name) = gamepad::BUTTON_NAMES.get(b) {
                    out.push(format!("Pad{p}_{name}"));
                }
            }
            for (i, name) in gamepad::AXIS_NAMES.iter().enumerate() {
                let (now, was) = (pad.axis(i), pad.prev_axis(i));
                if now >= dz && was < dz {
                    out.push(format!("Pad{p}_{name}+"));
                }
                if now <= -dz && was > -dz {
                    out.push(format!("Pad{p}_{name}-"));
                }
            }
        }
        out
    }

    /// Is an action currently pressed? (any bound key held)
    pub fn is_action_pressed(&self, action: &str) -> bool {
        let Some(keys) = self.actions.get(action) else {
            return false;
        };
        let dead_zone = self.effective_dead_zone(action);
        keys.iter().any(|k| {
            if let Some(binding) = gamepad::parse_binding(k) {
                return self.pad_binding_held(binding, dead_zone);
            }
            KeyCode::from_name(k).is_some_and(|kc| self.pressed.contains(&kc))
        })
    }

    /// The dead zone to threshold this action's axis bindings against.
    ///
    /// An unset (zero) dead zone means "not configured", not "fire on any
    /// drift": a resting stick reports small non-zero values, so zero would
    /// leave the action permanently pressed.
    fn effective_dead_zone(&self, action: &str) -> f32 {
        match self.dead_zones.get(action) {
            Some(v) if *v > 0.0 => *v,
            _ => gamepad::DEFAULT_AXIS_DEAD_ZONE,
        }
    }

    fn pad(&self, index: usize) -> Option<&gamepad::GamepadState> {
        self.pads.get(index).filter(|p| p.connected)
    }

    /// The full state of pad `index`, connected or not.
    ///
    /// For a host that needs to publish everything about a pad at once; a
    /// script should use the narrow accessors below.
    pub fn pad_state(&self, index: usize) -> Option<&gamepad::GamepadState> {
        self.pads.get(index)
    }

    /// Whether pad `index` is connected right now.
    ///
    /// Scripts need this before reading an axis: a stick that reads 0.0
    /// because nothing is plugged in is indistinguishable from one held at
    /// centre, and a game that wants to say "connect a controller" cannot tell
    /// the difference otherwise.
    pub fn is_pad_connected(&self, index: usize) -> bool {
        self.pad(index).is_some()
    }

    /// How many pad slots exist, connected or not.
    ///
    /// Slots are kept across a disconnect so indices stay stable, so this is
    /// "how many have ever been seen", not "how many are plugged in". Pair it
    /// with [`Self::is_pad_connected`].
    pub fn pad_count(&self) -> usize {
        self.pads.len()
    }

    /// Raw axis value in `-1.0..=1.0`, **not** dead-zoned.
    ///
    /// Zero for a disconnected pad or an axis it does not have. Dead zones are
    /// a property of an *action* binding, so a script reading a stick directly
    /// gets the raw value and decides its own threshold — see
    /// [`Self::action_axis_value`] for the bound, dead-zoned path, which is the
    /// one a rebindable game should use.
    pub fn pad_axis(&self, index: usize, axis: usize) -> f32 {
        self.pad(index).map(|p| p.axis(axis)).unwrap_or(0.0)
    }

    /// Whether button `button` on pad `index` is held.
    pub fn is_pad_button_pressed(&self, index: usize, button: usize) -> bool {
        self.pad(index).is_some_and(|p| p.pressed.contains(&button))
    }

    /// Whether button `button` on pad `index` went down this frame.
    pub fn is_pad_button_just_pressed(&self, index: usize, button: usize) -> bool {
        self.pad(index).is_some_and(|p| p.just_pressed.contains(&button))
    }

    fn pad_binding_held(&self, binding: gamepad::PadBinding, dead_zone: f32) -> bool {
        match binding {
            gamepad::PadBinding::Button { pad, index } => {
                self.pad(pad).is_some_and(|p| p.pressed.contains(&index))
            }
            gamepad::PadBinding::Axis {
                pad,
                index,
                positive,
            } => self.pad(pad).is_some_and(|p| {
                let v = p.axis(index);
                if positive {
                    v >= dead_zone
                } else {
                    v <= -dead_zone
                }
            }),
        }
    }

    /// How far an action is pushed, `0.0..=1.0`: the largest of its bindings,
    /// where a key or button reads 1.0 while held and an axis binding reads
    /// its dead-zoned deflection in the bound direction.
    ///
    /// Every axis binding names a direction (`LeftStickX+` / `LeftStickX-`),
    /// so an action is one-sided and its value is a magnitude. A signed axis
    /// is two actions: `right - left`.
    ///
    /// A `-` binding used to return the raw negative value while a key on the
    /// same action returned +1.0, so `right - left` sent the stick and the
    /// D-pad in opposite directions (scene 44 did exactly that).
    ///
    /// Returns 0.0 for an unbound or unmoved action.
    pub fn action_axis_value(&self, action: &str) -> f32 {
        let Some(keys) = self.actions.get(action) else {
            return 0.0;
        };
        let dead_zone = self.effective_dead_zone(action);
        let mut best = 0.0f32;
        for key in keys {
            let value = match gamepad::parse_binding(key) {
                Some(gamepad::PadBinding::Axis {
                    pad,
                    index,
                    positive,
                }) => {
                    let v = self.pad(pad).map(|p| p.axis(index)).unwrap_or(0.0);
                    // Deflection in the bound direction; the other direction
                    // and the dead zone both read as zero.
                    let along = if positive { v } else { -v };
                    if along >= dead_zone { along.min(1.0) } else { 0.0 }
                }
                Some(binding) => {
                    if self.pad_binding_held(binding, dead_zone) {
                        1.0
                    } else {
                        0.0
                    }
                }
                None => {
                    if KeyCode::from_name(key).is_some_and(|kc| self.pressed.contains(&kc)) {
                        1.0
                    } else {
                        0.0
                    }
                }
            };
            best = best.max(value);
        }
        best
    }

    /// Replace a pad's state for this frame. Backends call this once per
    /// connected pad; unknown indices grow the list.
    pub fn update_gamepad(
        &mut self,
        index: usize,
        pressed: std::collections::HashSet<usize>,
        axes: Vec<f32>,
    ) {
        if self.pads.len() <= index {
            self.pads.resize_with(index + 1, gamepad::GamepadState::default);
        }
        self.pads[index].update(pressed, axes);
        // A press, or a stick pushed out of its dead zone, makes this the pad
        // in use; a stick drifting at rest does not.
        let pad = &self.pads[index];
        let dz = gamepad::DEFAULT_AXIS_DEAD_ZONE;
        let pushed = (0..pad.axes.len()).any(|a| pad.axis(a).abs() >= dz && pad.prev_axis(a).abs() < dz);
        if !pad.just_pressed.is_empty() || pushed {
            self.last_device = interactions::Device::Gamepad;
            self.last_pad = Some(index);
        }
    }

    /// What the platform calls pad `index` and its USB vendor id, which
    /// decide its family ([`prompts::pad_kind`]). Backends call it whenever
    /// they poll; it costs a comparison when nothing changed.
    pub fn set_gamepad_info(&mut self, index: usize, name: &str, vendor: Option<u16>) {
        if self.pads.len() <= index {
            self.pads.resize_with(index + 1, gamepad::GamepadState::default);
        }
        let pad = &mut self.pads[index];
        if pad.name != name || pad.vendor != vendor {
            pad.name = name.to_string();
            pad.vendor = vendor;
        }
    }

    /// Disconnect every pad not in `present`. Polling backends report the
    /// set of live pads each frame; anything missing has gone away, and a
    /// silent disappearance would otherwise leave its buttons held.
    pub fn retain_gamepads(&mut self, present: &[usize]) {
        for (index, pad) in self.pads.iter_mut().enumerate() {
            if pad.connected && !present.contains(&index) {
                pad.disconnect();
            }
        }
    }

    /// Mark a pad disconnected, releasing whatever it held — otherwise an
    /// action stays pressed forever when a controller dies mid-input.
    pub fn disconnect_gamepad(&mut self, index: usize) {
        if let Some(pad) = self.pads.get_mut(index) {
            pad.disconnect();
        }
    }

    /// True when at least one pad has reported in.
    pub fn has_gamepad(&self) -> bool {
        self.pads.iter().any(|p| p.connected)
    }

    /// Whether a controller is being used right now: a button held, or a
    /// stick pushed past [`gamepad::DEFAULT_AXIS_DEAD_ZONE`]. State, not an
    /// edge, so it is seen whatever order a host polls pads and reads it in.
    /// The on-screen touch controls step aside on it.
    pub fn gamepad_in_use(&self) -> bool {
        self.pads.iter().filter(|p| p.connected).any(|p| {
            !p.pressed.is_empty() || p.axes.iter().any(|a| a.abs() > gamepad::DEFAULT_AXIS_DEAD_ZONE)
        })
    }

    /// Was an action just pressed this frame? (any bound key just pressed)
    pub fn is_action_just_pressed(&self, action: &str) -> bool {
        let Some(keys) = self.actions.get(action) else {
            return false;
        };
        let dead_zone = self.effective_dead_zone(action);
        keys.iter().any(|k| match gamepad::parse_binding(k) {
            Some(gamepad::PadBinding::Button { pad, index }) => self
                .pad(pad)
                .is_some_and(|p| p.just_pressed.contains(&index)),
            // An axis has no press edge of its own; crossing the dead zone is
            // the closest honest equivalent, and it is what the digital
            // helpers below compare against.
            Some(binding @ gamepad::PadBinding::Axis { .. }) => {
                self.pad_binding_held(binding, dead_zone)
                    && !self.pad_binding_was_held(binding, dead_zone)
            }
            None => KeyCode::from_name(k).is_some_and(|kc| self.just_pressed.contains(&kc)),
        })
    }

    /// Whether an axis binding was past the dead zone on the *previous*
    /// frame, for edge detection. Backends keep the previous axis values in
    /// `GamepadState::prev_axes`.
    fn pad_binding_was_held(&self, binding: gamepad::PadBinding, dead_zone: f32) -> bool {
        match binding {
            gamepad::PadBinding::Button { pad, index } => self
                .pad(pad)
                .is_some_and(|p| p.pressed.contains(&index) && !p.just_pressed.contains(&index)),
            gamepad::PadBinding::Axis {
                pad,
                index,
                positive,
            } => self.pad(pad).is_some_and(|p| {
                let v = p.prev_axis(index);
                if positive {
                    v >= dead_zone
                } else {
                    v <= -dead_zone
                }
            }),
        }
    }

    /// Was an action just released this frame? (any bound key, button or
    /// axis just released)
    ///
    /// Used to test keyboard bindings only, so an action bound to `Pad0_A`
    /// reported a press and never a release - a hold-to-charge attack on a
    /// controller never fired.
    pub fn is_action_just_released(&self, action: &str) -> bool {
        let Some(keys) = self.actions.get(action) else {
            return false;
        };
        let dead_zone = self.effective_dead_zone(action);
        keys.iter().any(|k| match gamepad::parse_binding(k) {
            // Read the slot directly rather than through `pad()`: a pad that
            // disconnected this frame releases everything it held, and that
            // release is exactly what the action needs to see.
            Some(gamepad::PadBinding::Button { pad, index }) => self
                .pads
                .get(pad)
                .is_some_and(|p| p.just_released.contains(&index)),
            // The mirror of the press edge: back inside the dead zone.
            Some(binding @ gamepad::PadBinding::Axis { .. }) => {
                self.pad_binding_was_held(binding, dead_zone)
                    && !self.pad_binding_held(binding, dead_zone)
            }
            None => KeyCode::from_name(k).is_some_and(|kc| self.just_released.contains(&kc)),
        })
    }

    /// Get the current mouse wheel delta for this frame.
    pub fn get_mouse_wheel(&self) -> f32 {
        self.mouse_wheel_delta
    }

    /// Get a list of all action names and their bindings.
    pub fn get_actions(&self) -> &HashMap<String, Vec<String>> {
        &self.actions
    }

    /// Iterator over keys that became pressed this frame.
    /// Used by event-synthesis paths (script `key_pressed:*` events,
    /// behavior-graph `OnKey { edge: "pressed" }` dispatch) to avoid
    /// scanning the full `ALL_KEYS` list every frame.
    pub fn just_pressed_iter(&self) -> impl Iterator<Item = KeyCode> + '_ {
        self.just_pressed.iter().copied()
    }

    /// Iterator over keys that became released this frame.
    pub fn just_released_iter(&self) -> impl Iterator<Item = KeyCode> + '_ {
        self.just_released.iter().copied()
    }

    /// Iterator over keys currently held (every frame the key is down).
    /// Used by behavior-graph `OnKey { edge: "held" }` dispatch.
    pub fn pressed_iter(&self) -> impl Iterator<Item = KeyCode> + '_ {
        self.pressed.iter().copied()
    }
}

/// 2.5 — apply a queued `BridgeCommand::Input*` variant to the live
/// `InputState`. The host runtime calls this in a loop on
/// `ScriptServices::pending_input_remaps` after `ScriptSystem::run`
/// completes. Non-Input variants are ignored so the caller can pass the
/// raw drain without filtering.
pub fn apply_input_remap(
    input: &mut InputState,
    cmd: &crate::scripting::bridge::BridgeCommand,
) {
    use crate::scripting::bridge::BridgeCommand;
    match cmd {
        BridgeCommand::InputSetBindings { action, keys } => {
            input.set_bindings(action, keys.clone());
        }
        BridgeCommand::InputAddBinding { action, key } => {
            input.add_binding(action, key);
        }
        BridgeCommand::InputRemoveBinding { action, key } => {
            input.remove_binding(action, key);
        }
        BridgeCommand::InputClearBindings { action } => {
            input.clear_bindings(action);
        }
        BridgeCommand::InputResetBindings => {
            input.reset_bindings();
        }
        BridgeCommand::InputSetDeadZone { action, value } => {
            input.set_dead_zone(action, *value);
        }
        _ => {}
    }
}

/// The modifier state of a browser `keydown`, for [`browser_keeps_default`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BrowserKeyMods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

/// Whether a web build lets the browser act on this `keydown` as well.
///
/// `code` is the W3C `KeyboardEvent.code`. The page is the game, so the
/// answer is **no** unless the key is browser chrome the player expects to
/// keep. Until Sep 2026 the web runtime never called `preventDefault`, so
/// every key did two things: F3 toggled the stats overlay *and* opened the
/// browser's find bar, Space and the arrows scrolled the page under the
/// game, Tab walked focus off the canvas, Ctrl+S saved the page, and
/// Alt+Left navigated away from the game.
///
/// Kept: reload (F5, Ctrl+R), fullscreen (F11), zoom (Ctrl with + / - / 0),
/// the developer tools (Ctrl+Shift+I / J / C, and F12 when the build has no
/// debug overlay to give it to), the tab and window keys (Ctrl+W / T / N /
/// Tab / PageUp / PageDown - most of which a page cannot block anyway), and
/// anything with Meta held, which is where macOS puts all of the above.
pub fn browser_keeps_default(code: &str, m: BrowserKeyMods, debug_overlay: bool) -> bool {
    if m.meta {
        return true;
    }
    match code {
        "F5" | "F11" => return true,
        "F12" => return !debug_overlay,
        _ => {}
    }
    if m.ctrl && !m.alt {
        if m.shift {
            return matches!(code, "KeyI" | "KeyJ" | "KeyC" | "KeyR" | "KeyT" | "KeyN" | "Tab");
        }
        return matches!(
            code,
            "KeyR"
                | "KeyW"
                | "KeyT"
                | "KeyN"
                | "Tab"
                | "PageUp"
                | "PageDown"
                | "Equal"
                | "Minus"
                | "Digit0"
                | "NumpadAdd"
                | "NumpadSubtract"
                | "Numpad0"
        );
    }
    false
}

#[cfg(test)]
mod browser_default_tests {
    use super::*;

    const NONE: BrowserKeyMods = BrowserKeyMods { ctrl: false, shift: false, alt: false, meta: false };
    const CTRL: BrowserKeyMods = BrowserKeyMods { ctrl: true, ..NONE };

    #[test]
    fn game_keys_do_not_reach_the_browser() {
        // F3 opened the find bar; the rest scroll, move focus or navigate.
        for code in ["F3", "F4", "F1", "Space", "ArrowUp", "ArrowDown", "PageDown", "Tab", "Backspace", "Slash", "Quote", "KeyW", "Escape", "AltLeft"] {
            assert!(!browser_keeps_default(code, NONE, true), "{code}");
        }
        for code in ["KeyS", "KeyF", "KeyP", "KeyD", "KeyG"] {
            assert!(!browser_keeps_default(code, CTRL, true), "Ctrl+{code}");
        }
        let alt = BrowserKeyMods { alt: true, ..NONE };
        assert!(!browser_keeps_default("ArrowLeft", alt, true), "Alt+Left navigates away");
    }

    #[test]
    fn browser_chrome_keys_stay_the_browsers() {
        for code in ["F5", "F11"] {
            assert!(browser_keeps_default(code, NONE, true), "{code}");
        }
        for code in ["KeyR", "KeyW", "KeyT", "Tab", "Equal", "Minus", "Digit0"] {
            assert!(browser_keeps_default(code, CTRL, true), "Ctrl+{code}");
        }
        let ctrl_shift = BrowserKeyMods { shift: true, ..CTRL };
        assert!(browser_keeps_default("KeyI", ctrl_shift, true), "devtools");
        let meta = BrowserKeyMods { meta: true, ..NONE };
        assert!(browser_keeps_default("KeyQ", meta, true), "Cmd+Q");
    }

    #[test]
    fn f12_goes_to_the_overlay_only_when_the_build_has_one() {
        assert!(!browser_keeps_default("F12", NONE, true));
        assert!(browser_keeps_default("F12", NONE, false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every key the engine names must survive the browser round trip.
    ///
    /// The two browser runtimes each kept a hand-written table and each listed
    /// the same partial set, so the keys nobody happened to need were missing
    /// from both - silently, because an unmapped key is not an error, it is a
    /// key that does nothing.
    #[test]
    fn every_key_code_maps_from_its_browser_code() {
        // The W3C `code` for a key, derived the same way a browser derives it.
        fn browser_code(key: KeyCode) -> String {
            let name = key.name();
            // A single letter is `KeyX`, a single digit `DigitN`, and the four
            // arrows carry `Arrow`. Everything else is its own name.
            if name.len() == 1 && name.chars().all(|c| c.is_ascii_alphabetic()) {
                format!("Key{name}")
            } else if name.len() == 1 && name.chars().all(|c| c.is_ascii_digit()) {
                format!("Digit{name}")
            } else if matches!(key, KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down) {
                format!("Arrow{name}")
            } else {
                name.to_string()
            }
        }
        for key in ALL_KEYS {
            let code = browser_code(*key);
            assert_eq!(
                KeyCode::from_browser_code(&code),
                Some(*key),
                "browser code {code:?} does not reach {key:?}"
            );
        }
    }

    /// The keys that were missing, named outright.
    ///
    /// `Backspace` is the one with teeth: it is how every text field deletes,
    /// so without it typing in the editor or on the web could only ever add.
    #[test]
    fn the_keys_the_hand_written_tables_forgot() {
        for (code, expected) in [
            ("Backspace", KeyCode::Backspace),
            ("PageUp", KeyCode::PageUp),
            ("PageDown", KeyCode::PageDown),
            ("BracketLeft", KeyCode::BracketLeft),
            ("BracketRight", KeyCode::BracketRight),
            ("Home", KeyCode::Home),
            ("End", KeyCode::End),
            ("Delete", KeyCode::Delete),
            ("Minus", KeyCode::Minus),
            ("Numpad5", KeyCode::Numpad5),
            ("NumpadEnter", KeyCode::NumpadEnter),
        ] {
            assert_eq!(KeyCode::from_browser_code(code), Some(expected));
        }
    }

    /// `project.jengine` files were written with the browser spelling, and an
    /// action bound to `"ArrowUp"` has to answer to the Up key.
    #[test]
    fn bindings_accept_the_browser_spelling_of_a_key() {
        assert_eq!(KeyCode::from_name("ArrowUp"), Some(KeyCode::Up));
        assert_eq!(KeyCode::from_name("KeyW"), Some(KeyCode::W));
        assert_eq!(KeyCode::from_name("Digit3"), Some(KeyCode::Digit3));
        // The canonical name is still what the engine reports.
        assert_eq!(KeyCode::Up.name(), "Up");
        // Each prefix only onto its own kind of key.
        for bogus in ["ArrowSpace", "Arrow", "KeyEnter", "Keya", "Digit10", "DigitA", "KeyF1"] {
            assert_eq!(KeyCode::from_name(bogus), None, "{bogus:?}");
        }

        let mut input = InputState::new();
        input.set_bindings("up", vec!["ArrowUp".into()]);
        input.key_down(KeyCode::Up);
        assert!(input.is_action_pressed("up"));
    }

    #[test]
    fn a_code_the_engine_has_no_key_for_is_none() {
        // Real `code` values with no `KeyCode`: refusing them is right, and is
        // what keeps the mapping from guessing.
        for code in ["MetaLeft", "IntlBackslash", "ContextMenu", "", "Key"] {
            assert_eq!(KeyCode::from_browser_code(code), None, "{code:?}");
        }
    }

    fn act(name: &str, keys: &[&str]) -> InputAction {
        InputAction {
            name: name.to_string(),
            keys: keys.iter().map(|k| k.to_string()).collect(),
            dead_zone: 0.0,
        }
    }

    #[test]
    fn load_actions_captures_defaults() {
        let mut s = InputState::new();
        s.load_actions(&[act("jump", &["Space", "W"])]);
        s.set_bindings("jump", vec!["Enter".into()]);
        assert_eq!(s.get_bindings("jump").unwrap(), &vec!["Enter".to_string()]);
        s.reset_bindings();
        assert_eq!(
            s.get_bindings("jump").unwrap(),
            &vec!["Space".to_string(), "W".to_string()]
        );
    }

    #[test]
    fn add_and_remove_binding_are_idempotent() {
        let mut s = InputState::new();
        s.load_actions(&[act("fire", &["Space"])]);
        s.add_binding("fire", "Enter");
        s.add_binding("fire", "Enter"); // dup ignored
        assert_eq!(s.get_bindings("fire").unwrap().len(), 2);
        s.remove_binding("fire", "Space");
        s.remove_binding("fire", "Space"); // no-op
        assert_eq!(s.get_bindings("fire").unwrap(), &vec!["Enter".to_string()]);
    }

    #[test]
    fn dead_zone_clamps_and_clears_at_zero() {
        let mut s = InputState::new();
        s.load_actions(&[act("strafe", &["A"])]);
        s.set_dead_zone("strafe", 1.5); // clamped to 1.0
        assert_eq!(s.get_dead_zone("strafe"), 1.0);
        s.set_dead_zone("strafe", 0.0); // removed
        assert_eq!(s.get_dead_zone("strafe"), 0.0);
    }

    #[test]
    fn reset_drops_runtime_only_actions() {
        let mut s = InputState::new();
        s.load_actions(&[act("jump", &["Space"])]);
        s.set_bindings("crouch", vec!["C".into()]); // not in defaults
        s.reset_bindings();
        assert!(s.get_bindings("crouch").is_none());
        assert_eq!(s.get_bindings("jump").unwrap(), &vec!["Space".to_string()]);
    }

    #[test]
    fn mouse_left_edges_fire_once_and_track_held_state() {
        let mut s = InputState::new();
        assert!(!s.mouse_left && !s.mouse_left_just_pressed && !s.mouse_left_just_released);

        // Press: both the edge and the held state go true.
        s.begin_frame();
        s.set_mouse_button(true, true);
        assert!(s.mouse_left_just_pressed);
        assert!(s.mouse_left);
        assert!(!s.mouse_left_just_released);

        // Held across a frame boundary: the edge clears, the hold persists.
        s.begin_frame();
        assert!(!s.mouse_left_just_pressed);
        assert!(s.mouse_left);

        // A repeat "pressed" report while already down is not a new edge.
        s.set_mouse_button(true, true);
        assert!(!s.mouse_left_just_pressed);

        // Release: the falling edge fires exactly once and the hold drops.
        s.begin_frame();
        s.set_mouse_button(true, false);
        assert!(s.mouse_left_just_released);
        assert!(!s.mouse_left);

        s.begin_frame();
        assert!(!s.mouse_left_just_released);
    }

    #[test]
    fn losing_focus_releases_everything_with_release_edges() {
        let mut s = InputState::new();
        s.load_actions(&[act("walk", &["D"])]);
        s.key_down(KeyCode::D);
        s.key_down(KeyCode::ShiftLeft);
        s.set_mouse_button(true, true);
        s.begin_frame();

        s.release_all();
        assert!(!s.is_pressed(KeyCode::D) && !s.is_action_pressed("walk"), "key stuck after focus loss");
        assert!(s.is_just_released(KeyCode::D) && s.is_action_just_released("walk"));
        assert!(s.is_just_released(KeyCode::ShiftLeft));
        assert!(!s.mouse_left && s.mouse_left_just_released);

        // Pressing the key again after refocusing is a fresh press.
        s.begin_frame();
        s.key_down(KeyCode::D);
        assert!(s.is_just_pressed(KeyCode::D));
    }

    #[test]
    fn pressed_action_uses_current_bindings() {
        let mut s = InputState::new();
        s.load_actions(&[act("jump", &["Space"])]);
        s.key_down(KeyCode::Space);
        assert!(s.is_action_pressed("jump"));
        // Rebind to Enter — Space release means jump no longer pressed.
        s.set_bindings("jump", vec!["Enter".into()]);
        assert!(!s.is_action_pressed("jump"));
        s.key_down(KeyCode::Enter);
        assert!(s.is_action_pressed("jump"));
    }
}

#[cfg(test)]
mod gamepad_action_tests {
    use super::*;
    use std::collections::HashSet;

    fn state_with(action: &str, keys: &[&str], dead_zone: f32) -> InputState {
        let mut input = InputState::new();
        input.load_actions(&[InputAction {
            name: action.to_string(),
            keys: keys.iter().map(|k| k.to_string()).collect(),
            dead_zone,
        }]);
        input
    }

    #[test]
    fn a_pad_button_presses_the_action() {
        let mut input = state_with("jump", &["Space", "Pad0_A"], 0.0);
        assert!(!input.is_action_pressed("jump"));
        input.update_gamepad(0, HashSet::from([0]), vec![0.0; 4]);
        assert!(input.is_action_pressed("jump"));
        assert!(input.is_action_just_pressed("jump"));
    }

    #[test]
    fn an_unpolled_gamepad_binding_simply_never_matches() {
        // Nothing polls pads in a headless build; a binding that references
        // one must be inert rather than an error.
        let input = state_with("jump", &["Pad0_A"], 0.0);
        assert!(!input.is_action_pressed("jump"));
        assert!(!input.has_gamepad());
    }

    #[test]
    fn an_axis_needs_to_pass_the_dead_zone() {
        let mut input = state_with("right", &["Pad0_LeftStickX+"], 0.5);
        input.update_gamepad(0, HashSet::new(), vec![0.4, 0.0, 0.0, 0.0]);
        assert!(!input.is_action_pressed("right"), "0.4 < dead zone 0.5");
        input.update_gamepad(0, HashSet::new(), vec![0.9, 0.0, 0.0, 0.0]);
        assert!(input.is_action_pressed("right"));
        // The opposite direction must not fire on the same binding.
        input.update_gamepad(0, HashSet::new(), vec![-0.9, 0.0, 0.0, 0.0]);
        assert!(!input.is_action_pressed("right"));
    }

    #[test]
    fn a_zero_dead_zone_falls_back_rather_than_firing_on_drift() {
        // A resting stick reports small non-zero values. Treating an unset
        // dead zone as 0.0 would leave the action permanently pressed.
        let mut input = state_with("right", &["Pad0_LeftStickX+"], 0.0);
        input.update_gamepad(0, HashSet::new(), vec![0.08, 0.0, 0.0, 0.0]);
        assert!(!input.is_action_pressed("right"));
    }

    #[test]
    fn analog_value_reads_through_for_sticks_and_snaps_for_keys() {
        let mut input = state_with("move", &["Pad0_LeftStickX+", "D"], 0.3);
        input.update_gamepad(0, HashSet::new(), vec![0.75, 0.0, 0.0, 0.0]);
        assert!((input.action_axis_value("move") - 0.75).abs() < 1e-6);

        // A key on the same action still reads as fully deflected, so one
        // action can serve keyboard and stick.
        let mut keyed = state_with("move", &["D"], 0.3);
        keyed.key_down(KeyCode::D);
        assert!((keyed.action_axis_value("move") - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_negative_axis_binding_reads_as_a_magnitude_like_a_key() {
        // Scene 44: `right - left`. A stick pushed left must give the same
        // sign as the D-pad pushed left.
        let mut input = state_with("left", &["Pad0_LeftStickX-", "Pad0_DPadLeft"], 0.2);
        input.update_gamepad(0, HashSet::new(), vec![-0.8, 0.0, 0.0, 0.0]);
        assert!((input.action_axis_value("left") - 0.8).abs() < 1e-6);
        // Pushed the other way, a `-` binding reads nothing.
        input.update_gamepad(0, HashSet::new(), vec![0.8, 0.0, 0.0, 0.0]);
        assert_eq!(input.action_axis_value("left"), 0.0);
        // The D-pad on the same action agrees in sign.
        input.update_gamepad(0, HashSet::from([14]), vec![0.0; 4]);
        assert_eq!(input.action_axis_value("left"), 1.0);
    }

    #[test]
    fn a_pad_button_release_is_an_action_release() {
        let mut input = state_with("charge", &["Space", "Pad0_A"], 0.0);
        input.update_gamepad(0, HashSet::from([0]), vec![0.0; 4]);
        assert!(!input.is_action_just_released("charge"));
        input.update_gamepad(0, HashSet::new(), vec![0.0; 4]);
        assert!(input.is_action_just_released("charge"), "the pad release was not seen");
    }

    #[test]
    fn an_axis_returning_to_centre_is_an_action_release() {
        let mut input = state_with("right", &["Pad0_LeftStickX+"], 0.5);
        input.update_gamepad(0, HashSet::new(), vec![0.9, 0.0, 0.0, 0.0]);
        assert!(!input.is_action_just_released("right"));
        input.update_gamepad(0, HashSet::new(), vec![0.1, 0.0, 0.0, 0.0]);
        assert!(input.is_action_just_released("right"));
    }

    #[test]
    fn disconnecting_releases_a_held_action() {
        let mut input = state_with("jump", &["Pad0_A"], 0.0);
        input.update_gamepad(0, HashSet::from([0]), vec![0.0; 4]);
        assert!(input.is_action_pressed("jump"));
        input.disconnect_gamepad(0);
        assert!(!input.is_action_pressed("jump"), "a dead controller must not hold an action down");
    }
}
