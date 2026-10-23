//! Which controller is this, and what is the button called on it (Oct 2026).
//!
//! A button prompt - "press [A] to jump" - has to name the button the player
//! is holding: A on an Xbox pad, Cross on a PlayStation one, and **B** on a
//! Switch, whose south button is labelled B. The engine reads every pad in
//! the W3C Standard Gamepad layout, which is positional (button 0 is the
//! south one), so a binding like `Pad0_A` means "the south button" on every
//! pad, and only the label changes. This module says which family a pad is
//! ([`pad_kind`], from its name and USB vendor id) and what a binding is
//! called in each family ([`label`]), and picks the binding an action should
//! show for the device in use ([`prompt`]).
//!
//! A prompt carries a **glyph** id as well as a label - `"xbox/a"`,
//! `"playstation/cross"`, `"keyboard/space"` - so a game can keep one icon
//! per glyph (an atlas region by that name) and never map labels itself.

use super::gamepad::{self, PadBinding};

/// The family of a controller, which decides what its buttons are called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PadKind {
    Xbox,
    PlayStation,
    Switch,
    /// Anything else: labelled as an Xbox pad, which is what most third-party
    /// pads copy, under its own glyph family so a game can draw them neutral.
    #[default]
    Generic,
}

impl PadKind {
    /// `"xbox"`, `"playstation"`, `"switch"`, `"generic"` - what scripts read
    /// and the glyph family.
    pub fn name(self) -> &'static str {
        match self {
            PadKind::Xbox => "xbox",
            PadKind::PlayStation => "playstation",
            PadKind::Switch => "switch",
            PadKind::Generic => "generic",
        }
    }
}

const VENDOR_MICROSOFT: u16 = 0x045e;
const VENDOR_SONY: u16 = 0x054c;
const VENDOR_NINTENDO: u16 = 0x057e;

/// Which family a pad is, from what its platform calls it and its USB vendor
/// id. The vendor decides when it is known; otherwise the name is searched
/// for the words each family's pads report (`"Xbox 360 Controller"`,
/// `"DualSense Wireless Controller"`, `"Pro Controller"`).
pub fn pad_kind(name: &str, vendor: Option<u16>) -> PadKind {
    match vendor {
        Some(VENDOR_MICROSOFT) => return PadKind::Xbox,
        Some(VENDOR_SONY) => return PadKind::PlayStation,
        Some(VENDOR_NINTENDO) => return PadKind::Switch,
        _ => {}
    }
    let n = name.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["xbox", "xinput", "x-box"]) {
        PadKind::Xbox
    } else if has(&["dualsense", "dualshock", "playstation", "ps4", "ps5", "ps3", "sony"]) {
        PadKind::PlayStation
    } else if has(&["pro controller", "joy-con", "joycon", "nintendo", "switch"]) {
        PadKind::Switch
    } else {
        PadKind::Generic
    }
}

/// The USB vendor id in a browser's `Gamepad.id`, which has no field of its
/// own: Chromium writes `"... (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)"`,
/// Firefox `"054c-0ce6-DualSense Wireless Controller"`.
pub fn vendor_in_id(id: &str) -> Option<u16> {
    let hex4 = |s: &str| {
        let h: String = s.chars().take(4).collect();
        (h.len() == 4).then(|| u16::from_str_radix(&h, 16).ok()).flatten()
    };
    if let Some(at) = id.find("Vendor: ") {
        return hex4(&id[at + 8..]);
    }
    let b = id.as_bytes();
    if b.len() > 9 && b[4] == b'-' && b[9] == b'-' {
        return hex4(id);
    }
    None
}

/// How a prompt is drawn: a keyboard, or a family of pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Keyboard,
    Pad(PadKind),
}

impl Style {
    /// `"keyboard"` or the pad family's name.
    pub fn name(self) -> &'static str {
        match self {
            Style::Keyboard => "keyboard",
            Style::Pad(k) => k.name(),
        }
    }

    /// The style a script names: `"keyboard"` (also `"mouse"`, `"touch"`),
    /// `"xbox"`, `"playstation"`, `"switch"`, `"generic"`.
    pub fn from_name(name: &str) -> Option<Style> {
        Some(match name {
            "keyboard" | "mouse" | "touch" => Style::Keyboard,
            "xbox" => Style::Pad(PadKind::Xbox),
            "playstation" => Style::Pad(PadKind::PlayStation),
            "switch" => Style::Pad(PadKind::Switch),
            "generic" | "gamepad" => Style::Pad(PadKind::Generic),
            _ => return None,
        })
    }
}

/// Labels and glyph names per button, in [`gamepad::BUTTON_NAMES`] order.
const XBOX: [(&str, &str); 17] = [
    ("A", "a"), ("B", "b"), ("X", "x"), ("Y", "y"), ("LB", "lb"), ("RB", "rb"), ("LT", "lt"), ("RT", "rt"),
    ("View", "view"), ("Menu", "menu"), ("LS", "ls"), ("RS", "rs"), ("D-Pad Up", "dpad_up"),
    ("D-Pad Down", "dpad_down"), ("D-Pad Left", "dpad_left"), ("D-Pad Right", "dpad_right"), ("Xbox", "guide"),
];
const PLAYSTATION: [(&str, &str); 17] = [
    ("Cross", "cross"), ("Circle", "circle"), ("Square", "square"), ("Triangle", "triangle"), ("L1", "l1"),
    ("R1", "r1"), ("L2", "l2"), ("R2", "r2"), ("Create", "create"), ("Options", "options"), ("L3", "l3"),
    ("R3", "r3"), ("D-Pad Up", "dpad_up"), ("D-Pad Down", "dpad_down"), ("D-Pad Left", "dpad_left"),
    ("D-Pad Right", "dpad_right"), ("PS", "guide"),
];
/// Positional, as the engine reads every pad: the south button is button 0,
/// and on a Switch pad the south button says **B**.
const SWITCH: [(&str, &str); 17] = [
    ("B", "b"), ("A", "a"), ("Y", "y"), ("X", "x"), ("L", "l"), ("R", "r"), ("ZL", "zl"), ("ZR", "zr"),
    ("-", "minus"), ("+", "plus"), ("L Stick", "ls"), ("R Stick", "rs"), ("D-Pad Up", "dpad_up"),
    ("D-Pad Down", "dpad_down"), ("D-Pad Left", "dpad_left"), ("D-Pad Right", "dpad_right"), ("Home", "guide"),
];

fn button_table(kind: PadKind) -> &'static [(&'static str, &'static str); 17] {
    match kind {
        PadKind::Xbox | PadKind::Generic => &XBOX,
        PadKind::PlayStation => &PLAYSTATION,
        PadKind::Switch => &SWITCH,
    }
}

fn key_label(name: &str) -> &str {
    match name {
        "ShiftLeft" | "ShiftRight" => "Shift",
        "ControlLeft" | "ControlRight" => "Ctrl",
        "AltLeft" | "AltRight" => "Alt",
        "Escape" => "Esc",
        "PageUp" => "Page Up",
        "PageDown" => "Page Down",
        "Minus" => "-",
        "Equal" => "=",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backslash" => "\\",
        "Semicolon" => ";",
        "Quote" => "'",
        "Backquote" => "`",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        other => other,
    }
}

fn snake(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c == ' ' {
            if !out.ends_with('_') {
                out.push('_');
            }
            continue;
        }
        if c.is_ascii_uppercase() && !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// What `binding` is called, and its glyph id, in `style`. A pad binding
/// shown in the keyboard style keeps the generic family's names, and a key
/// in a pad style is still a key: the binding decides the device, the style
/// only the family.
pub fn label(binding: &str, style: Style) -> (String, String) {
    match gamepad::parse_binding(binding) {
        Some(PadBinding::Button { index, .. }) => {
            let kind = match style {
                Style::Pad(k) => k,
                Style::Keyboard => PadKind::Generic,
            };
            match button_table(kind).get(index) {
                Some((l, g)) => (l.to_string(), format!("{}/{g}", kind.name())),
                None => (binding.to_string(), format!("{}/unknown", kind.name())),
            }
        }
        Some(PadBinding::Axis { index, positive, .. }) => {
            let kind = match style {
                Style::Pad(k) => k,
                Style::Keyboard => PadKind::Generic,
            };
            let stick = if index < 2 { "Left Stick" } else { "Right Stick" };
            let way = match (index % 2 == 0, positive) {
                (true, true) => "Right",
                (true, false) => "Left",
                (false, true) => "Up",
                (false, false) => "Down",
            };
            (format!("{stick} {way}"), format!("{}/{}_{}", kind.name(), snake(stick), way.to_ascii_lowercase()))
        }
        None => (key_label(binding).to_string(), format!("keyboard/{}", snake(binding))),
    }
}

/// An action's prompt: what to show for it, and which binding it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    /// `"A"`, `"Cross"`, `"Space"` - empty for an unbound action.
    pub label: String,
    /// `"xbox/a"`, `"playstation/cross"`, `"keyboard/space"`.
    pub glyph: String,
    /// The binding shown, as the action holds it (`"Pad0_A"`, `"Space"`).
    pub binding: String,
    /// The style it is drawn in (`"keyboard"`, `"xbox"`, ...).
    pub style: &'static str,
}

/// The prompt for an action bound to `bindings`, in `style`: its first
/// binding of that device - a key for the keyboard, a pad binding for a pad -
/// or, when it has none, its first binding of any kind, drawn as what it is.
pub fn prompt(bindings: &[String], style: Style) -> Prompt {
    let is_pad = |b: &String| gamepad::parse_binding(b).is_some();
    let wanted = match style {
        Style::Keyboard => bindings.iter().find(|b| !is_pad(b)),
        Style::Pad(_) => bindings.iter().find(|b| is_pad(b)),
    };
    let Some(binding) = wanted.or(bindings.first()) else {
        return Prompt { label: String::new(), glyph: String::new(), binding: String::new(), style: style.name() };
    };
    // A fallback across devices is drawn as its own device.
    let style = match (style, is_pad(binding)) {
        (Style::Keyboard, true) => Style::Pad(PadKind::Generic),
        (Style::Pad(_), false) => Style::Keyboard,
        (s, _) => s,
    };
    let (label, glyph) = label(binding, style);
    Prompt { label, glyph, binding: binding.clone(), style: style.name() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pad_is_known_by_its_vendor_or_its_name() {
        assert_eq!(pad_kind("whatever", Some(0x054c)), PadKind::PlayStation);
        assert_eq!(pad_kind("Xbox Wireless Controller", None), PadKind::Xbox);
        assert_eq!(pad_kind("DualSense Wireless Controller", None), PadKind::PlayStation);
        assert_eq!(pad_kind("Nintendo Switch Pro Controller", None), PadKind::Switch);
        assert_eq!(pad_kind("8BitDo SN30", None), PadKind::Generic);
    }

    #[test]
    fn a_browser_id_carries_the_vendor() {
        assert_eq!(vendor_in_id("DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)"), Some(0x054c));
        assert_eq!(vendor_in_id("057e-2009-Pro Controller"), Some(0x057e));
        assert_eq!(vendor_in_id("Xbox 360 Controller (XInput STANDARD GAMEPAD)"), None);
    }

    #[test]
    fn the_south_button_is_named_by_its_family() {
        let south = |k| label("Pad0_A", Style::Pad(k));
        assert_eq!(south(PadKind::Xbox), ("A".into(), "xbox/a".into()));
        assert_eq!(south(PadKind::PlayStation), ("Cross".into(), "playstation/cross".into()));
        assert_eq!(south(PadKind::Switch), ("B".into(), "switch/b".into()));
        assert_eq!(label("Pad0_LeftStickY+", Style::Pad(PadKind::Xbox)).0, "Left Stick Up");
        assert_eq!(label("Pad1_RightStickX-", Style::Pad(PadKind::Switch)).1, "switch/right_stick_left");
        assert_eq!(label("ShiftLeft", Style::Keyboard), ("Shift".into(), "keyboard/shift_left".into()));
    }

    #[test]
    fn a_prompt_shows_the_binding_of_the_device_in_use() {
        let b: Vec<String> = vec!["Space".into(), "Pad0_A".into()];
        assert_eq!(prompt(&b, Style::Keyboard).label, "Space");
        let p = prompt(&b, Style::Pad(PadKind::PlayStation));
        assert_eq!((p.label.as_str(), p.binding.as_str(), p.style), ("Cross", "Pad0_A", "playstation"));
        // Only a key: a pad player is still shown the key, drawn as a key.
        let p = prompt(&["E".to_string()], Style::Pad(PadKind::Xbox));
        assert_eq!((p.label.as_str(), p.style), ("E", "keyboard"));
        assert_eq!(prompt(&[], Style::Keyboard).label, "");
    }
}
