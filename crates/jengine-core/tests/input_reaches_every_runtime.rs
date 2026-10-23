//! Input must reach scripts the same way in every runtime.
//!
//! # The bug
//!
//! The browser preamble answered `is_action_pressed` by looking the action's
//! bindings up in `action_bindings` and testing each against `pressed_keys`.
//! That map is built from `KeyCode`s, so it holds keyboard keys and nothing
//! else — a gamepad binding like `Pad0_A` was never found. **Gamepad actions
//! worked natively and were silently dead in the editor viewport and in web
//! builds** (BUGS #9), which is to say dead in the runtime you develop in and
//! the one you ship.
//!
//! Native never had the problem because it does not re-derive anything: it
//! resolves actions through `InputState` and hands scripts the answers. The
//! browser hosts do that now too, via one shared publisher.
//!
//! Every existing guard passed throughout. `script_global_parity` saw
//! `is_action_pressed` bound in all runtimes; it was. It just could not see
//! half the input devices.

use jengine_core::input::{InputAction, InputState};
use std::collections::HashSet;

/// One action with the given bindings, the way a project file would load it.
fn state_with(action: &str, keys: &[&str]) -> InputState {
    let mut input = InputState::new();
    input.load_actions(&[InputAction {
        name: action.to_string(),
        keys: keys.iter().map(|k| k.to_string()).collect(),
        dead_zone: 0.0,
    }]);
    input
}

const BROWSER: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/script_host.rs");

/// The preamble must read resolved action state, not re-derive it.
///
/// Re-deriving is what made gamepad bindings invisible, and it would be
/// invisible again: the JS would still compile, the function would still be
/// bound, and only pad users would notice.

/// The per-frame publish both browser hosts share (`adapters::js::mirror`).
const MIRROR: &str = include_str!("../src/scripting/bridge/adapters/js/mirror.rs");

/// A host's source as far as what it publishes goes: its own, plus the shared
/// mirror - but only if it calls it, so a host that dropped the call fails.
fn with_mirror(src: &str) -> String {
    if src.contains("mirror::sync_world_to_play_namespace(") {
        format!("{src}
{MIRROR}")
    } else {
        src.to_string()
    }
}

#[test]
fn the_preamble_does_not_resolve_actions_for_itself() {
    for (name, published) in [
        ("is_action_pressed", "actions_pressed"),
        ("is_action_just_pressed", "actions_just_pressed"),
        ("is_action_just_released", "actions_just_released"),
    ] {
        let start = BROWSER
            .find(&format!("function {name}("))
            .unwrap_or_else(|| panic!("{name} is not in the preamble"));
        let end = BROWSER[start..]
            .find("\n  }}")
            .map(|n| start + n)
            .unwrap_or(BROWSER.len());
        let body = &BROWSER[start..end];
        assert!(
            body.contains(published),
            "`{name}` does not read `{published}`. Resolving an action from its \
             bindings in JavaScript only ever sees `pressed_keys`, which is \
             keyboard-only — that is how gamepad actions came to be dead in \
             both browser runtimes (BUGS #9)."
        );
        assert!(
            !body.contains("action_bindings"),
            "`{name}` walks `action_bindings` again. Actions are resolved once, \
             in Rust, by the same code the native runtime uses."
        );
    }
}

/// Both browser hosts must publish that state, or the preamble reads nothing.
#[test]
fn both_browser_hosts_publish_the_input_mirror() {
    for (name, src) in [("viewport", with_mirror(VIEWPORT)), ("web", with_mirror(WEB))] {
        assert!(
            src.contains("input_mirror::publish"),
            "{name} does not publish the input mirror, so every action and every \
             gamepad read in that runtime answers `false` / 0.0 while compiling \
             and running perfectly (BUGS #9)"
        );
    }
}

/// The analog surface must exist in the preamble, where the Fengari runtime
/// finds it.
///
/// `lua-runtime.ts` binds a bridge function by name through the wrapper's
/// `ns_lookup`, which evaluates the name in the wrapper's scope - so defining
/// it there is what makes it reachable. This used to also require a
/// `name: name,` entry, which was membership of the preamble's hand-written
/// lowercase `input` table; that table is gone (Sep 2026) and `Input.*` is
/// built from `scripting::namespaces` in every runtime, checked by
/// `script_namespaces_reach_every_runtime`.
#[test]
fn the_analog_input_functions_are_defined_where_the_runtime_looks() {
    assert!(
        BROWSER.contains("ns_lookup: function(name)"),
        "the preamble no longer exposes ns_lookup, which is how the Fengari runtime binds its functions"
    );
    for name in [
        "get_action_axis",
        "get_gamepad_count",
        "is_gamepad_connected",
        "get_gamepad_axis",
        "is_gamepad_button_pressed",
    ] {
        assert!(
            BROWSER.contains(&format!("function {name}(")),
            "the preamble does not define `{name}`"
        );
    }
}

/// A pad binding drives an action, in the engine's own resolution.
///
/// This is the behaviour the browser hosts were missing; testing it here pins
/// the thing they now delegate to.
#[test]
fn a_gamepad_binding_resolves_an_action() {
    let mut input = state_with("fire", &["Pad0_A"]);
    assert!(!input.is_action_pressed("fire"), "nothing is connected yet");
    input.update_gamepad(0, HashSet::from([0]), vec![0.0; 4]);

    assert!(
        input.is_action_pressed("fire"),
        "a pad button did not resolve its action — check the binding name \
         parser in `input::gamepad`"
    );
}

/// An axis binding reads as a signed analog value, and a key binding still
/// reads as full deflection so both can drive one action.
#[test]
fn an_action_axis_is_analog_from_a_stick_and_digital_from_a_key() {
    let mut input = state_with("move", &["Pad0_LeftStickX+"]);
    input.update_gamepad(0, HashSet::new(), vec![0.62, 0.0, 0.0, 0.0]);
    let v = input.action_axis_value("move");
    assert!(
        (v - 0.62).abs() < 1e-5,
        "expected the raw stick value, got {v} — the action axis is being \
         thresholded to 1.0, which is the behaviour BUGS #9 was about"
    );

    let mut keyed = state_with("move", &["D"]);
    keyed.key_down(jengine_core::input::KeyCode::from_name("D").expect("D"));
    assert_eq!(
        keyed.action_axis_value("move"),
        1.0,
        "a key binding must read as full deflection so a keyboard and a stick \
         can drive the same action"
    );
}

/// Raw pad reads are not dead-zoned, and a disconnected pad is distinguishable
/// from a centred stick.
#[test]
fn raw_pad_reads_are_undeadzoned_and_report_connection() {
    let mut input = InputState::new();
    assert!(!input.is_pad_connected(0));
    assert_eq!(input.pad_axis(0, 0), 0.0, "a missing pad reads zero, not garbage");

    // 0.03 is well below any sensible dead zone: the raw read must still show it.
    input.update_gamepad(0, HashSet::from([2]), vec![0.03, -0.5, 0.0, 0.0]);

    assert!(input.is_pad_connected(0));
    assert!(
        (input.pad_axis(0, 0) - 0.03).abs() < 1e-6,
        "a raw axis was dead-zoned; a dead zone belongs to a binding, not a device"
    );
    assert!(input.is_pad_button_pressed(0, 2));
    assert!(!input.is_pad_button_pressed(0, 3));
    assert_eq!(input.pad_axis(0, 9), 0.0, "an axis the pad lacks reads zero");
}

// ── The browser key table ────────────────────────────────────────────────────

// `WEB` above is the script host; the key listeners live in the crate root.
const WEB_ROOT: &str = include_str!("../../jengine-web/src/lib.rs");

/// Neither browser runtime may keep its own `KeyboardEvent.code` table.
///
/// Both did, and both listed the same partial set - arrows, space, enter,
/// escape, tab, modifiers, letters, digits - dropping everything else on the
/// floor. An unmapped key is not an error, it is a key that does nothing, so
/// the gap was invisible: **Backspace never arrived**, which is the key every
/// text field deletes with, and neither did PageUp / PageDown or the brackets,
/// which is what the test lab tells you to press to walk its scenes. Native
/// had them all along through winit, so this was two runtimes out of three.
///
/// `KeyCode::from_browser_code` derives the whole enum from the key names and
/// is round-tripped over `ALL_KEYS` in `input`'s own tests, so the only thing
/// left to check is that nobody grows a second table beside it.
#[test]
fn no_browser_runtime_keeps_its_own_key_table() {
    for (name, src) in [("viewport", VIEWPORT), ("web", WEB_ROOT)] {
        assert!(
            src.contains("KeyCode::from_browser_code"),
            "{name} does not map browser keys through `KeyCode::from_browser_code`"
        );
        assert!(
            !src.contains("\"KeyA\" => A"),
            "{name} has grown its own browser key table again - the last two              listed the same partial set and silently dropped Backspace,              PageUp/PageDown and the brackets"
        );
    }
}
