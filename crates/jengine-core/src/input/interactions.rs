//! Hold, tap and double-tap on input actions, and the one view of input that
//! scripts query (Oct 2026).
//!
//! An action used to have three answers - pressed, just pressed, just
//! released - so "hold to charge", "tap to talk, hold to pick up" and
//! "double-tap to dash" were each a timer a script kept beside the action,
//! counted in `dt` (which a paused game stops) and reset by hand. Now
//! [`InputState::update_interactions`] keeps, per action, when its press
//! began, how long the last one lasted and when the recent ones started, on
//! the **wall clock** - input is the player's time, not the game's - and the
//! questions are asked of that:
//!
//! * **held** for `t` seconds: true on the one frame the press reaches `t`,
//!   so a charge attack can ask about 0.5, 1.0 and 2.0 of one press;
//! * **tapped**: released before `t` - so tap-or-hold on one button never
//!   fires both, for the same `t`;
//! * **double-tapped**: a press that starts within `t` of the previous one,
//!   counted in pairs (a fourth quick press is a second double tap, not a
//!   third).
//!
//! Every threshold is an argument rather than a setting on the action, so
//! nothing has to be configured and one press can be asked about several.
//!
//! [`InputView`] is what scripts read: actions' bindings and timing, the
//! pads' names and families, the device used last. Natively it rides in the
//! script snapshot; the browser hosts keep the frame's copy behind one hook
//! (`js/input_query.rs`), so every runtime answers from this file.

use std::collections::{HashMap, VecDeque};

use super::prompts::{self, PadKind, Prompt, Style};
use super::InputState;

/// The hold a script asks about when it names none.
pub const DEFAULT_HOLD: f32 = 0.5;
/// A tap is a press released before this, when the script names nothing.
pub const DEFAULT_TAP: f32 = 0.25;
/// The longest gap between two presses of a double tap, by default.
pub const DEFAULT_DOUBLE_TAP: f32 = 0.3;
/// Recent press starts kept per action - enough to count a burst of taps.
const PRESS_HISTORY: usize = 8;

/// The device the player used last, for prompts that follow them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Device {
    #[default]
    None,
    Keyboard,
    Mouse,
    Gamepad,
    Touch,
}

impl Device {
    pub fn name(self) -> &'static str {
        match self {
            Device::None => "none",
            Device::Keyboard => "keyboard",
            Device::Mouse => "mouse",
            Device::Gamepad => "gamepad",
            Device::Touch => "touch",
        }
    }
}

/// One action's press timing, as of this frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActionTiming {
    /// When the current press began, on the clock `update_interactions` was
    /// given; `None` while the action is up.
    pub down_since: Option<f64>,
    /// Seconds the current press has lasted (0 while up).
    pub held: f32,
    /// What `held` was last frame - a hold of `t` is reached when it crosses.
    pub prev_held: f32,
    /// A press began this frame.
    pub just_pressed: bool,
    /// A press ended this frame, after `released_after` seconds.
    pub just_released: bool,
    pub released_after: f32,
    /// Start times of the recent presses, oldest first.
    pub presses: VecDeque<f64>,
}

impl ActionTiming {
    /// Bring this action up to `now`, given whether it is down and whether a
    /// press began and ended inside the frame (a tap shorter than a frame).
    fn step(&mut self, now: f64, down: bool, blipped: bool) {
        self.just_pressed = false;
        self.just_released = false;
        let was_down = self.down_since.is_some();
        if down && !was_down {
            self.down_since = Some(now);
            self.just_pressed = true;
            self.press_at(now);
        } else if !down && was_down {
            let since = self.down_since.take().unwrap_or(now);
            self.just_released = true;
            self.released_after = (now - since).max(0.0) as f32;
        } else if !down && blipped {
            self.just_pressed = true;
            self.just_released = true;
            self.released_after = 0.0;
            self.press_at(now);
        }
        self.prev_held = self.held;
        self.held = self.down_since.map(|s| (now - s).max(0.0) as f32).unwrap_or(0.0);
    }

    fn press_at(&mut self, now: f64) {
        self.presses.push_back(now);
        while self.presses.len() > PRESS_HISTORY {
            self.presses.pop_front();
        }
    }

    /// The press reached `seconds` this frame (zero or less: it began).
    pub fn held_for(&self, seconds: f32) -> bool {
        if seconds <= 0.0 {
            return self.just_pressed;
        }
        self.down_since.is_some() && self.held >= seconds && self.prev_held < seconds
    }

    /// A press shorter than `seconds` ended this frame.
    pub fn tapped(&self, seconds: f32) -> bool {
        self.just_released && self.released_after < seconds
    }

    /// A press began this frame within `seconds` of the one before, and it
    /// is the second of a pair: the run of presses each within `seconds` of
    /// the next, ending at this one, is even.
    pub fn double_tapped(&self, seconds: f32) -> bool {
        if !self.just_pressed {
            return false;
        }
        let mut run = 1;
        let times: Vec<f64> = self.presses.iter().copied().collect();
        for w in times.windows(2).rev() {
            if w[1] - w[0] <= seconds as f64 {
                run += 1;
            } else {
                break;
            }
        }
        run >= 2 && run % 2 == 0
    }
}

impl InputState {
    /// Bring every action's hold / tap / double-tap timing up to now. Every
    /// host calls this once a frame, after the frame's input has arrived and
    /// pads are polled, before scripts run.
    pub fn update_interactions(&mut self) {
        self.update_interactions_at(crate::profiling::unix_time());
    }

    /// [`Self::update_interactions`] on a given clock, in seconds.
    pub fn update_interactions_at(&mut self, now: f64) {
        let names: Vec<String> = self.get_actions().keys().cloned().collect();
        let mut timing = std::mem::take(&mut self.interactions);
        timing.retain(|k, _| names.contains(k));
        for name in names {
            let down = self.is_action_pressed(&name);
            let blipped = self.is_action_just_pressed(&name) && self.is_action_just_released(&name);
            timing.entry(name).or_default().step(now, down, blipped);
        }
        self.interactions = timing;
        // Gestures from the fingers - and the mouse, a finger while none is
        // down - on the same clock.
        let mouse = Some((self.mouse_position, self.mouse_left));
        self.gestures = self.gesture_state.update(&self.touches, mouse, self.mouse_wheel_delta, now);
        // An input replay (`input::replay`) keeps this frame's input, or puts
        // back the kept frame's - after the timing above, so holds and
        // gestures play back as they were.
        super::replay::apply(self);
    }

    /// One action's timing, if it has any.
    pub fn action_timing(&self, action: &str) -> Option<&ActionTiming> {
        self.interactions.get(action)
    }
}

/// A pad as scripts see it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PadInfo {
    pub connected: bool,
    pub name: String,
    pub kind: PadKind,
}

/// Everything about input a script asks that is more than a key or a button:
/// interactions, pad identity and prompts. Built once a frame from the
/// `InputState` (`InputView::of`); natively it is the snapshot's, in the
/// browsers the hook's.
#[derive(Debug, Clone, Default)]
pub struct InputView {
    pub bindings: HashMap<String, Vec<String>>,
    pub timing: HashMap<String, ActionTiming>,
    pub pads: Vec<PadInfo>,
    pub last_device: Device,
    pub last_pad: Option<usize>,
}

impl InputView {
    pub fn of(input: &InputState) -> InputView {
        InputView {
            bindings: input.get_actions().clone(),
            timing: input.interactions.clone(),
            pads: (0..input.pad_count())
                .map(|i| {
                    let p = input.pad_state(i);
                    PadInfo {
                        connected: input.is_pad_connected(i),
                        name: p.map(|p| p.name.clone()).unwrap_or_default(),
                        kind: p.map(|p| p.kind()).unwrap_or_default(),
                    }
                })
                .collect(),
            last_device: input.last_device,
            last_pad: input.last_pad,
        }
    }

    fn timing(&self, action: &str) -> Option<&ActionTiming> {
        self.timing.get(action)
    }

    /// Seconds the action has been held, 0 while up.
    pub fn hold_time(&self, action: &str) -> f32 {
        self.timing(action).map(|t| t.held).unwrap_or(0.0)
    }

    pub fn held(&self, action: &str, seconds: Option<f32>) -> bool {
        self.timing(action).is_some_and(|t| t.held_for(seconds.unwrap_or(DEFAULT_HOLD)))
    }

    pub fn tapped(&self, action: &str, seconds: Option<f32>) -> bool {
        self.timing(action).is_some_and(|t| t.tapped(seconds.unwrap_or(DEFAULT_TAP)))
    }

    pub fn double_tapped(&self, action: &str, seconds: Option<f32>) -> bool {
        self.timing(action).is_some_and(|t| t.double_tapped(seconds.unwrap_or(DEFAULT_DOUBLE_TAP)))
    }

    /// What the platform calls pad `index`; empty when there is none.
    pub fn pad_name(&self, index: usize) -> &str {
        self.pads.get(index).filter(|p| p.connected).map(|p| p.name.as_str()).unwrap_or("")
    }

    /// `"xbox"`, `"playstation"`, `"switch"`, `"generic"`, or `""` for no pad.
    pub fn pad_type(&self, index: usize) -> &'static str {
        self.pads.get(index).filter(|p| p.connected).map(|p| p.kind.name()).unwrap_or("")
    }

    /// The style prompts follow: the family of the pad used last when the
    /// player is on a pad, else the keyboard.
    pub fn current_style(&self) -> Style {
        match self.last_device {
            Device::Gamepad => {
                Style::Pad(self.last_pad.and_then(|i| self.pads.get(i)).map(|p| p.kind).unwrap_or_default())
            }
            _ => Style::Keyboard,
        }
    }

    /// An action's prompt in `style` (a [`Style::from_name`] name), or in
    /// the style of the device used last.
    pub fn prompt(&self, action: &str, style: Option<&str>) -> Prompt {
        let style = style.and_then(Style::from_name).unwrap_or_else(|| self.current_style());
        let none = Vec::new();
        prompts::prompt(self.bindings.get(action).unwrap_or(&none), style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{InputAction, KeyCode};

    fn input() -> InputState {
        let mut i = InputState::new();
        i.load_actions(&[InputAction { name: "jump".into(), keys: vec!["Space".into()], dead_zone: 0.0 }]);
        i
    }

    /// Run one frame at `t`: events, then the update, as a host does.
    fn frame(i: &mut InputState, t: f64, events: impl FnOnce(&mut InputState)) -> ActionTiming {
        i.begin_frame();
        events(i);
        i.update_interactions_at(t);
        i.action_timing("jump").cloned().unwrap()
    }

    #[test]
    fn a_hold_is_reached_once_per_threshold() {
        let mut i = input();
        frame(&mut i, 0.0, |i| i.key_down(KeyCode::Space));
        let mut reached = Vec::new();
        for k in 1..=30 {
            let t = frame(&mut i, k as f64 / 10.0, |_| {});
            for s in [0.5f32, 1.0, 2.0] {
                if t.held_for(s) {
                    reached.push((k, s));
                }
            }
        }
        assert_eq!(reached, vec![(5, 0.5), (10, 1.0), (20, 2.0)]);
    }

    #[test]
    fn tap_or_hold_on_one_button_never_fires_both() {
        let mut i = input();
        frame(&mut i, 0.0, |i| i.key_down(KeyCode::Space));
        let up = frame(&mut i, 0.1, |i| i.key_up(KeyCode::Space));
        assert!(up.tapped(0.25) && up.just_released);
        frame(&mut i, 1.0, |i| i.key_down(KeyCode::Space));
        let held = frame(&mut i, 1.3, |_| {});
        assert!(held.held_for(0.25));
        let up = frame(&mut i, 1.4, |i| i.key_up(KeyCode::Space));
        assert!(!up.tapped(0.25), "a held press is not also a tap");
    }

    #[test]
    fn a_tap_inside_one_frame_is_still_a_tap() {
        let mut i = input();
        let t = frame(&mut i, 0.0, |i| {
            i.key_down(KeyCode::Space);
            i.key_up(KeyCode::Space);
        });
        assert!(t.just_pressed && t.tapped(0.25));
    }

    #[test]
    fn double_taps_come_in_pairs() {
        let mut i = input();
        let mut doubles = Vec::new();
        // Four quick presses, a pause, one more.
        for (n, t0) in [0.0, 0.2, 0.4, 0.6, 2.0].into_iter().enumerate() {
            let down = frame(&mut i, t0, |i| i.key_down(KeyCode::Space));
            if down.double_tapped(0.3) {
                doubles.push(n);
            }
            frame(&mut i, t0 + 0.05, |i| i.key_up(KeyCode::Space));
        }
        assert_eq!(doubles, vec![1, 3]);
    }

    #[test]
    fn prompts_follow_the_device_used_last() {
        let mut i = InputState::new();
        i.load_actions(&[InputAction { name: "jump".into(), keys: vec!["Space".into(), "Pad0_A".into()], dead_zone: 0.0 }]);
        i.update_gamepad(0, Default::default(), vec![0.0; 4]);
        i.set_gamepad_info(0, "DualSense Wireless Controller", Some(0x054c));
        i.key_down(KeyCode::Space);
        assert_eq!(InputView::of(&i).prompt("jump", None).label, "Space");
        i.update_gamepad(0, [0usize].into_iter().collect(), vec![0.0; 4]);
        let v = InputView::of(&i);
        assert_eq!(v.last_device, Device::Gamepad);
        assert_eq!(v.prompt("jump", None).label, "Cross");
        assert_eq!(v.prompt("jump", Some("switch")).label, "B");
        assert_eq!(v.pad_type(0), "playstation");
        assert_eq!(v.pad_name(0), "DualSense Wireless Controller");
        assert_eq!(v.pad_type(1), "");
    }
}
