//! jEngine patch: hardware keyboard keys.
//!
//! winit 0.30 on iOS reports only text typed into the soft keyboard
//! (`insertText:`) and Backspace, so a keyboard attached to an iPad or a
//! phone - or the Mac keyboard reaching a simulator - pressed nothing.
//! UIKit hands key presses to `pressesBegan:withEvent:` and friends up the
//! responder chain; [`WinitUIWindow`](super::window::WinitUIWindow), at the
//! end of that chain, turns each into a `KeyboardInput` here.
//!
//! Each event carries the physical key (from the HID usage UIKit reports)
//! and **no text**: text still comes only from `insertText:` while the view
//! is first responder, so a focused text field does not get every character
//! twice.

use objc2_foundation::{MainThreadMarker, NSSet};
use objc2_ui_kit::{UIKey, UIPress};

use super::app_state::{self, EventWrapper};
use crate::event::{ElementState, Event, KeyEvent, WindowEvent};
use crate::keyboard::{Key, KeyCode, KeyLocation, NamedKey, NativeKeyCode, PhysicalKey};
use crate::platform_impl::platform::DEVICE_ID;
use crate::platform_impl::KeyEventExtra;
use crate::window::WindowId as RootWindowId;

/// Turn the presses in `presses` into key events for `window_id`. Returns
/// whether any carried a key (the rest are left to the next responder).
pub(crate) fn handle_presses(
    mtm: MainThreadMarker,
    window_id: RootWindowId,
    presses: &NSSet<UIPress>,
    state: ElementState,
) -> bool {
    let events: Vec<EventWrapper> = presses
        .iter()
        .filter_map(|press| unsafe { press.key(mtm) })
        .map(|key| EventWrapper::StaticEvent(Event::WindowEvent {
            window_id,
            event: WindowEvent::KeyboardInput {
                device_id: DEVICE_ID,
                event: key_event(&key, state),
                is_synthetic: false,
            },
        }))
        .collect();
    let any = !events.is_empty();
    if any {
        app_state::handle_nonuser_events(mtm, events);
    }
    any
}

fn key_event(key: &UIKey, state: ElementState) -> KeyEvent {
    let usage = unsafe { key.keyCode() }.0;
    let physical_key = match key_code(usage) {
        Some(code) => PhysicalKey::Code(code),
        None => PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
    };
    let logical_key = match physical_key {
        PhysicalKey::Code(code) => named(code),
        _ => None,
    }
    .map(Key::Named)
    .unwrap_or_else(|| {
        let chars = unsafe { key.charactersIgnoringModifiers() }.to_string();
        if chars.is_empty() {
            Key::Unidentified(crate::keyboard::NativeKey::Unidentified)
        } else {
            Key::Character(chars.into())
        }
    });
    let location = match physical_key {
        PhysicalKey::Code(
            KeyCode::ShiftLeft | KeyCode::ControlLeft | KeyCode::AltLeft | KeyCode::SuperLeft,
        ) => KeyLocation::Left,
        PhysicalKey::Code(
            KeyCode::ShiftRight | KeyCode::ControlRight | KeyCode::AltRight | KeyCode::SuperRight,
        ) => KeyLocation::Right,
        PhysicalKey::Code(code) if format!("{code:?}").starts_with("Numpad") => {
            KeyLocation::Numpad
        },
        _ => KeyLocation::Standard,
    };
    // UIKit sends no new press for a held key, so there is no repeat.
    KeyEvent {
        physical_key,
        logical_key,
        text: None,
        location,
        state,
        repeat: false,
        platform_specific: KeyEventExtra {},
    }
}

/// A USB HID keyboard usage (page 0x07) - what `UIKey.keyCode` reports - as
/// the key in that position on a US layout.
pub(crate) fn key_code(usage: i64) -> Option<KeyCode> {
    use KeyCode::*;
    const LETTERS: [KeyCode; 26] = [
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO,
        KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    ];
    const DIGITS: [KeyCode; 10] =
        [Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0];
    const FUNCTION: [KeyCode; 12] = [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12];
    const NUMPAD: [KeyCode; 10] = [
        Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9, Numpad0,
    ];
    Some(match usage {
        0x04..=0x1D => LETTERS[(usage - 0x04) as usize],
        0x1E..=0x27 => DIGITS[(usage - 0x1E) as usize],
        0x28 => Enter,
        0x29 => Escape,
        0x2A => Backspace,
        0x2B => Tab,
        0x2C => Space,
        0x2D => Minus,
        0x2E => Equal,
        0x2F => BracketLeft,
        0x30 => BracketRight,
        0x31 => Backslash,
        0x33 => Semicolon,
        0x34 => Quote,
        0x35 => Backquote,
        0x36 => Comma,
        0x37 => Period,
        0x38 => Slash,
        0x39 => CapsLock,
        0x3A..=0x45 => FUNCTION[(usage - 0x3A) as usize],
        0x46 => PrintScreen,
        0x47 => ScrollLock,
        0x48 => Pause,
        0x49 => Insert,
        0x4A => Home,
        0x4B => PageUp,
        0x4C => Delete,
        0x4D => End,
        0x4E => PageDown,
        0x4F => ArrowRight,
        0x50 => ArrowLeft,
        0x51 => ArrowDown,
        0x52 => ArrowUp,
        0x53 => NumLock,
        0x54 => NumpadDivide,
        0x55 => NumpadMultiply,
        0x56 => NumpadSubtract,
        0x57 => NumpadAdd,
        0x58 => NumpadEnter,
        0x59..=0x62 => NUMPAD[(usage - 0x59) as usize],
        0x63 => NumpadDecimal,
        0x64 => IntlBackslash,
        0x65 => ContextMenu,
        0x67 => NumpadEqual,
        0xE0 => ControlLeft,
        0xE1 => ShiftLeft,
        0xE2 => AltLeft,
        0xE3 => SuperLeft,
        0xE4 => ControlRight,
        0xE5 => ShiftRight,
        0xE6 => AltRight,
        0xE7 => SuperRight,
        _ => return None,
    })
}

/// The named logical key for a key that types no character.
fn named(code: KeyCode) -> Option<NamedKey> {
    use KeyCode::*;
    Some(match code {
        Enter | NumpadEnter => NamedKey::Enter,
        Escape => NamedKey::Escape,
        Backspace => NamedKey::Backspace,
        Tab => NamedKey::Tab,
        Space => NamedKey::Space,
        CapsLock => NamedKey::CapsLock,
        F1 => NamedKey::F1,
        F2 => NamedKey::F2,
        F3 => NamedKey::F3,
        F4 => NamedKey::F4,
        F5 => NamedKey::F5,
        F6 => NamedKey::F6,
        F7 => NamedKey::F7,
        F8 => NamedKey::F8,
        F9 => NamedKey::F9,
        F10 => NamedKey::F10,
        F11 => NamedKey::F11,
        F12 => NamedKey::F12,
        PrintScreen => NamedKey::PrintScreen,
        ScrollLock => NamedKey::ScrollLock,
        Pause => NamedKey::Pause,
        Insert => NamedKey::Insert,
        Home => NamedKey::Home,
        PageUp => NamedKey::PageUp,
        Delete => NamedKey::Delete,
        End => NamedKey::End,
        PageDown => NamedKey::PageDown,
        ArrowRight => NamedKey::ArrowRight,
        ArrowLeft => NamedKey::ArrowLeft,
        ArrowDown => NamedKey::ArrowDown,
        ArrowUp => NamedKey::ArrowUp,
        NumLock => NamedKey::NumLock,
        ContextMenu => NamedKey::ContextMenu,
        ControlLeft | ControlRight => NamedKey::Control,
        ShiftLeft | ShiftRight => NamedKey::Shift,
        AltLeft | AltRight => NamedKey::Alt,
        SuperLeft | SuperRight => NamedKey::Super,
        _ => return None,
    })
}
