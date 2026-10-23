//! Moving focus and pressing things without a pointer: a D-pad, a stick or
//! the arrow keys walk a menu, and A (or Enter, or Space) presses what is
//! focused.
//!
//! A game on a controller has no pointer, and Apple Arcade and Google Play's
//! programmes want the whole game playable on one - menus included. `.gui`
//! documents could be walked with Tab and nothing else, and nothing pressed
//! the focused button; entity UI had no focus at all.
//!
//! This module is the part that knows nothing about either UI system: what
//! the player asked for this frame ([`read_intent`], with key repeat while a
//! direction is held), and which rect lies that way ([`pick_in_direction`]).
//! `ecs::systems::navigate_ui` applies it to both, and every host calls that
//! once a frame.
//!
//! Only what is marked `focusable` takes part - a `.gui` node's
//! `style.focusable`, an entity-UI element's `focusable` - so a gameplay HUD
//! with a button on it does not start taking the D-pad the player is moving
//! with.

use crate::input::{InputState, KeyCode};

/// Where focus moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavDirection {
    Up,
    Down,
    Left,
    Right,
}

/// What the player asked the UI for this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NavIntent {
    /// Move focus this way (once, or again as a held direction repeats).
    pub direction: Option<NavDirection>,
    /// Press the focused thing.
    pub activate: bool,
    /// Tab (`Some(false)`) or Shift-Tab (`Some(true)`): the document's own
    /// order rather than a direction.
    pub tab: Option<bool>,
}

impl NavIntent {
    pub fn is_empty(&self) -> bool {
        self.direction.is_none() && !self.activate && self.tab.is_none()
    }
}

/// A held direction repeats after this long...
pub const FIRST_REPEAT_SECONDS: f32 = 0.4;
/// ...and then this often, so a long list can be scrolled by holding down.
pub const REPEAT_SECONDS: f32 = 0.1;
/// How far a stick must be pushed to count as a direction. Well past the
/// binding dead zone: a stick drifting near it must not walk a menu.
pub const STICK_THRESHOLD: f32 = 0.5;

/// Which direction is held and for how long - lives on the `InputState`.
#[derive(Debug, Clone, Default)]
pub struct NavRepeat {
    held: Option<NavDirection>,
    until_repeat: f32,
}

/// The direction held right now, from any source: arrow keys, any pad's
/// D-pad, any pad's left stick. Stick Y is positive up (`gamepad::stick_y_up`).
pub fn held_direction(input: &InputState) -> Option<NavDirection> {
    held_direction_from(input, true)
}

/// [`held_direction`], with the arrow keys left out when `keys` is false.
fn held_direction_from(input: &InputState, keys: bool) -> Option<NavDirection> {
    use NavDirection::*;
    let key = [(KeyCode::Up, Up), (KeyCode::Down, Down), (KeyCode::Left, Left), (KeyCode::Right, Right)]
        .into_iter()
        .find(|(k, _)| keys && input.is_pressed(*k))
        .map(|(_, d)| d);
    if key.is_some() {
        return key;
    }
    for i in 0..input.pad_count() {
        let Some(pad) = input.pad_state(i).filter(|p| p.connected) else { continue };
        // D-pad buttons in the standard layout.
        for (button, dir) in [(12, Up), (13, Down), (14, Left), (15, Right)] {
            if pad.pressed.contains(&button) {
                return Some(dir);
            }
        }
        let (x, y) = (pad.axis(0), pad.axis(1));
        if x.abs().max(y.abs()) > STICK_THRESHOLD {
            return Some(if x.abs() > y.abs() {
                if x > 0.0 { Right } else { Left }
            } else if y > 0.0 {
                Up
            } else {
                Down
            });
        }
    }
    None
}

/// A direction pressed this frame, held or not: arrow-key and D-pad edges.
fn tapped_direction(input: &InputState, keys: bool) -> Option<NavDirection> {
    use NavDirection::*;
    for (k, d) in [(KeyCode::Up, Up), (KeyCode::Down, Down), (KeyCode::Left, Left), (KeyCode::Right, Right)] {
        if keys && input.is_just_pressed(k) {
            return Some(d);
        }
    }
    for i in 0..input.pad_count() {
        for (button, d) in [(12, Up), (13, Down), (14, Left), (15, Right)] {
            if input.is_pad_button_just_pressed(i, button) {
                return Some(d);
            }
        }
    }
    None
}

/// Read this frame's intent, advancing the repeat of a held direction by `dt`.
/// Call once a frame: calling it twice would repeat twice as fast.
pub fn read_intent(input: &mut InputState, dt: f32) -> NavIntent {
    read_intent_with(input, dt, true)
}

/// [`read_intent`], reading the keyboard's arrows, Enter and Space only when
/// `keys` is true. A focused text field owns those - its caret moves on the
/// arrows and Enter submits it - so navigation, which runs before the field
/// is updated, must not take them first (it moved focus off the field on
/// every arrow, Oct 2026). A pad still navigates, so it can leave the field.
pub fn read_intent_with(input: &mut InputState, dt: f32, keys: bool) -> NavIntent {
    let held = held_direction_from(input, keys);
    // A press that also ended within the frame is not held by the time this
    // looks, but it was a press: take the edge. Quick taps do this, and so
    // does `adb shell input keyevent`, which is how the emulator tests it.
    let tapped = tapped_direction(input, keys);
    let repeat = &mut input.ui_nav;
    let direction = if held.is_none() && tapped.is_some() {
        repeat.held = None;
        tapped
    } else if held != repeat.held {
        repeat.held = held;
        repeat.until_repeat = FIRST_REPEAT_SECONDS;
        held
    } else if held.is_some() {
        repeat.until_repeat -= dt;
        if repeat.until_repeat <= 0.0 {
            repeat.until_repeat = (repeat.until_repeat + REPEAT_SECONDS).max(0.0);
            held
        } else {
            None
        }
    } else {
        None
    };
    let pad_a = (0..input.pad_count()).any(|i| input.is_pad_button_just_pressed(i, 0));
    let activate =
        pad_a || (keys && (input.is_just_pressed(KeyCode::Enter) || input.is_just_pressed(KeyCode::Space)));
    let tab = input.is_just_pressed(KeyCode::Tab).then(|| {
        input.is_pressed(KeyCode::ShiftLeft) || input.is_pressed(KeyCode::ShiftRight)
    });
    NavIntent { direction, activate, tab }
}

/// A rect in y-down screen space (`.gui` layout's; entity UI converts).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl NavRect {
    fn center(&self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

/// The candidate lying `dir` of `from` that a player would expect: the
/// nearest along the direction, plus a price for being off to the side -
/// half the distance across when it overlaps `from` across the direction (it
/// is in line, give or take), double when it does not. `None` when nothing
/// lies that way: focus stays put rather than wrapping, as a menu's edge does.
pub fn pick_in_direction(from: NavRect, candidates: &[NavRect], dir: NavDirection) -> Option<usize> {
    let (fx, fy) = from.center();
    let mut best: Option<(f32, usize)> = None;
    for (i, c) in candidates.iter().enumerate() {
        let (cx, cy) = c.center();
        let (along, across, overlap) = match dir {
            NavDirection::Down => (cy - fy, (cx - fx).abs(), c.x < from.x + from.w && from.x < c.x + c.w),
            NavDirection::Up => (fy - cy, (cx - fx).abs(), c.x < from.x + from.w && from.x < c.x + c.w),
            NavDirection::Right => (cx - fx, (cy - fy).abs(), c.y < from.y + from.h && from.y < c.y + c.h),
            NavDirection::Left => (fx - cx, (cy - fy).abs(), c.y < from.y + from.h && from.y < c.y + c.h),
        };
        // Strictly that way: a candidate beside `from` is not below it.
        if along <= 1.0 {
            continue;
        }
        let score = along + across * if overlap { 0.5 } else { 2.0 };
        if best.is_none_or(|(s, _)| score < s) {
            best = Some((score, i));
        }
    }
    best.map(|(_, i)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32) -> NavRect {
        NavRect { x, y, w: 100.0, h: 40.0 }
    }

    #[test]
    fn a_column_is_walked_in_order_and_stops_at_its_ends() {
        let items = [r(0.0, 0.0), r(0.0, 50.0), r(0.0, 100.0)];
        assert_eq!(pick_in_direction(items[0], &items, NavDirection::Down), Some(1));
        assert_eq!(pick_in_direction(items[1], &items, NavDirection::Down), Some(2));
        assert_eq!(pick_in_direction(items[2], &items, NavDirection::Down), None);
        assert_eq!(pick_in_direction(items[2], &items, NavDirection::Up), Some(1));
        assert_eq!(pick_in_direction(items[1], &items, NavDirection::Left), None);
    }

    #[test]
    fn a_grid_prefers_what_is_in_line_over_what_is_nearer_diagonally() {
        // 0 1
        // 2 3, with 3 nudged left so its centre is nearer 0 than 2's is.
        let items = [r(0.0, 0.0), r(120.0, 0.0), r(0.0, 60.0), r(60.0, 55.0)];
        assert_eq!(pick_in_direction(items[0], &items, NavDirection::Down), Some(2));
        assert_eq!(pick_in_direction(items[0], &items, NavDirection::Right), Some(1));
        assert_eq!(pick_in_direction(items[1], &items, NavDirection::Down), Some(3));
        assert_eq!(pick_in_direction(items[3], &items, NavDirection::Up), Some(0));
    }

    fn frame(input: &mut InputState, dt: f32) -> NavIntent {
        let intent = read_intent(input, dt);
        input.begin_frame();
        intent
    }

    #[test]
    fn a_held_direction_repeats_after_a_pause() {
        let mut input = InputState::new();
        input.key_down(KeyCode::Down);
        assert_eq!(frame(&mut input, 0.016).direction, Some(NavDirection::Down), "fires on press");
        let mut fired = 0;
        let mut t = 0.0;
        while t < FIRST_REPEAT_SECONDS - 0.02 {
            fired += frame(&mut input, 0.016).direction.is_some() as u32;
            t += 0.016;
        }
        assert_eq!(fired, 0, "nothing until the first repeat");
        let mut fired = 0;
        for _ in 0..40 {
            fired += frame(&mut input, 0.016).direction.is_some() as u32;
        }
        // 0.64 s at one per 0.1 s after the first.
        assert!((6..=7).contains(&fired), "repeats {fired} times");
        input.key_up(KeyCode::Down);
        assert_eq!(frame(&mut input, 0.016).direction, None);
    }

    #[test]
    fn a_stick_and_a_dpad_are_directions_and_a_is_activate() {
        let mut input = InputState::new();
        input.update_gamepad(0, Default::default(), vec![0.1, 0.9, 0.0, 0.0]);
        assert_eq!(frame(&mut input, 0.016).direction, Some(NavDirection::Up), "stick up is +");
        input.update_gamepad(0, [14].into_iter().collect(), vec![0.0; 4]);
        assert_eq!(frame(&mut input, 0.016).direction, Some(NavDirection::Left));
        input.update_gamepad(0, [0].into_iter().collect(), vec![0.0; 4]);
        let i = frame(&mut input, 0.016);
        assert!(i.activate && i.direction.is_none());
        // A stick drifting under the threshold is nothing.
        input.update_gamepad(0, Default::default(), vec![0.3, -0.4, 0.0, 0.0]);
        assert!(frame(&mut input, 0.016).is_empty());
    }

    #[test]
    fn a_tap_shorter_than_a_frame_still_moves() {
        let mut input = InputState::new();
        input.key_down(KeyCode::Right);
        input.key_up(KeyCode::Right);
        assert_eq!(frame(&mut input, 0.016).direction, Some(NavDirection::Right));
        assert_eq!(frame(&mut input, 0.016).direction, None, "once");
    }
}
