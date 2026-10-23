//! Stick Y is positive **up** on every gamepad backend.
//!
//! Every game binds `move_up` to `LeftStickY+` and every script that reads a
//! stick treats up as `+`, but until Sep 2026 all three backends delivered up
//! as `-`: the browser's convention, which the desktop copied by flipping
//! gilrs. So a real controller moved every game upside down, on every
//! runtime, and nothing noticed because nothing here had been played with
//! one. The convention now lives in `gamepad::stick_y_up`, and this checks
//! each backend still goes through it (or, for gilrs, does not flip at all).

use jengine_core::input::gamepad::{AXIS_NAMES, STICK_Y_AXES, stick_y_up};

const WEB: &str = include_str!("../src/input/gamepad_web.rs");
const ANDROID: &str = include_str!("../../jengine-runtime/src/gamepad_android.rs");
const DESKTOP: &str = include_str!("../../jengine-runtime/src/gamepad_native.rs");

#[test]
fn positive_down_sources_convert_through_the_one_helper() {
    assert!(WEB.contains("stick_y_up(&mut axes)"), "the browser reports Y down-positive and must convert");
    assert!(ANDROID.contains("stick_y_up(&mut axes)"), "Android reports Y down-positive and must convert");
}

#[test]
fn gilrs_is_already_up_positive_and_is_not_flipped() {
    assert!(!DESKTOP.contains("stick_y_up("), "gilrs reports up as + already; converting it inverts the stick");
    assert!(!DESKTOP.contains("* -1.0") && !DESKTOP.contains("axis_sign"), "the desktop backend flips a sign again");
}

#[test]
fn the_helper_flips_the_two_y_axes_and_nothing_else() {
    for i in STICK_Y_AXES {
        assert!(AXIS_NAMES[i].ends_with('Y'), "{} is not a Y axis", AXIS_NAMES[i]);
    }
    let mut axes = vec![0.5, 0.25, -0.75, 1.0];
    stick_y_up(&mut axes);
    assert_eq!(axes, [0.5, -0.25, -0.75, -1.0]);
    // A pad reporting fewer axes is not an index panic.
    let mut short = vec![0.5];
    stick_y_up(&mut short);
    assert_eq!(short, [0.5]);
}
