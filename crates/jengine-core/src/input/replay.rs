//! **Input replays** (Oct 2026): a run recorded and played back exactly as
//! it happened - a pinball game, a speedrun, a bug report a tester can send.
//!
//! What makes a run is its starting world, its inputs and its frames' time,
//! so a replay keeps the last two and starts again from the first:
//! `Replay.record()` and `Replay.play()` each **arm** the replay, and the
//! next scene load (`scene::streaming::scene_replaced`, which every host
//! calls) starts it - so recording and playback both begin on a freshly
//! built world, physics and scripts. Then once a frame:
//!
//! - every host's frame time goes through [`frame_dt`] (its one line,
//!   `replay::frame_dt(raw) * time_scale`): recording, the time is kept;
//!   playing, the kept time is handed back instead of the clock's;
//! - [`apply`], at the end of `InputState::update_interactions` (which every
//!   host calls once a frame, after its input has arrived and before scripts
//!   run): recording, a snapshot of the frame's input is kept ([`Frame`] -
//!   keys, mouse, wheel, typed text, pads, fingers, gestures, action timing);
//!   playing, the input is replaced by the kept frame's, wall-clock holds
//!   and gestures included.
//!
//! The physics steps at a fixed rate on the accumulated frame time, so the
//! same times and the same input step it the same way. What a replay cannot
//! reach: anything timed on the wall clock rather than the frame
//! (`profiling::now_ms` - toasts, speech bubbles, wet paint), and a script's
//! `math.random` unless it seeds it. When the frames run out, playback ends
//! and the live input takes over where it left off. The two halves keep
//! their own place, so it does not matter which a host calls first.
//!
//! While a replay plays, everything reads the replayed input - including a
//! script's own "skip the replay" key, which it therefore cannot see. So the
//! player's own keys are kept too, every frame, and [`live_key`] answers from
//! them: a game reads the keys that *control* the replay through it, and they
//! are never part of what is replayed.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::{InputState, KeyCode, Touch, gamepad, gestures, interactions};

/// Where a replay is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Idle,
    /// Recording from the next scene load.
    ArmedRecord,
    Recording,
    /// Playing back from the next scene load.
    ArmedPlay,
    Playing,
    /// Played to its end; live input again.
    Finished,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Idle => "idle",
            Mode::ArmedRecord | Mode::ArmedPlay => "armed",
            Mode::Recording => "recording",
            Mode::Playing => "playing",
            Mode::Finished => "finished",
        }
    }
}

/// One frame's input, as scripts and systems read it.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    pressed: HashSet<KeyCode>,
    just_pressed: HashSet<KeyCode>,
    just_released: HashSet<KeyCode>,
    repeated: HashSet<KeyCode>,
    mouse_position: (f32, f32),
    screen_size: (f32, f32),
    mouse: [bool; 9],
    mouse_wheel_delta: f32,
    mouse_delta: (f32, f32),
    typed_chars: Vec<char>,
    pads: Vec<gamepad::GamepadState>,
    touches: Vec<Touch>,
    gestures: Vec<gestures::Gesture>,
    interactions: HashMap<String, interactions::ActionTiming>,
    last_device: interactions::Device,
    last_pad: Option<usize>,
}

impl Frame {
    /// This frame's input.
    pub fn of(i: &InputState) -> Self {
        Self {
            pressed: i.pressed.clone(),
            just_pressed: i.just_pressed.clone(),
            just_released: i.just_released.clone(),
            repeated: i.repeated.clone(),
            mouse_position: i.mouse_position,
            screen_size: i.screen_size,
            mouse: [
                i.mouse_left,
                i.mouse_left_just_pressed,
                i.mouse_left_just_released,
                i.mouse_right,
                i.mouse_right_just_pressed,
                i.mouse_right_just_released,
                i.mouse_middle,
                i.mouse_middle_just_pressed,
                i.mouse_middle_just_released,
            ],
            mouse_wheel_delta: i.mouse_wheel_delta,
            mouse_delta: i.mouse_delta,
            typed_chars: i.typed_chars.clone(),
            pads: i.pads.clone(),
            touches: i.touches.clone(),
            gestures: i.gestures.clone(),
            interactions: i.interactions.clone(),
            last_device: i.last_device,
            last_pad: i.last_pad,
        }
    }

    /// Make `i` this frame's input again.
    pub fn restore(&self, i: &mut InputState) {
        i.pressed = self.pressed.clone();
        i.just_pressed = self.just_pressed.clone();
        i.just_released = self.just_released.clone();
        i.repeated = self.repeated.clone();
        i.mouse_position = self.mouse_position;
        i.screen_size = self.screen_size;
        let m = self.mouse;
        (i.mouse_left, i.mouse_left_just_pressed, i.mouse_left_just_released) = (m[0], m[1], m[2]);
        (i.mouse_right, i.mouse_right_just_pressed, i.mouse_right_just_released) = (m[3], m[4], m[5]);
        (i.mouse_middle, i.mouse_middle_just_pressed, i.mouse_middle_just_released) = (m[6], m[7], m[8]);
        i.mouse_wheel_delta = self.mouse_wheel_delta;
        i.mouse_delta = self.mouse_delta;
        i.typed_chars = self.typed_chars.clone();
        i.pads = self.pads.clone();
        i.touches = self.touches.clone();
        i.gestures = self.gestures.clone();
        i.interactions = self.interactions.clone();
        i.last_device = self.last_device;
        i.last_pad = self.last_pad;
    }
}

/// The process's replay.
#[derive(Debug, Default)]
struct Replay {
    mode: Mode,
    dts: Vec<f32>,
    inputs: Vec<Frame>,
    /// Where playback is in each.
    dt_at: usize,
    input_at: usize,
    /// The keys the player pressed this frame, whatever is being replayed.
    live: HashSet<KeyCode>,
}

thread_local! {
    static REPLAY: RefCell<Replay> = RefCell::new(Replay::default());
}

/// Arm a recording: it starts with the next scene load (the frames kept
/// so far are dropped then).
pub fn record() {
    REPLAY.with(|r| r.borrow_mut().mode = Mode::ArmedRecord);
}

/// Arm playback of what was recorded: it starts with the next scene load.
/// `false` (and nothing armed) when nothing was recorded.
pub fn play() -> bool {
    REPLAY.with(|r| {
        let mut r = r.borrow_mut();
        if r.dts.is_empty() && r.inputs.is_empty() {
            return false;
        }
        r.mode = Mode::ArmedPlay;
        true
    })
}

/// Stop recording or playing (the frames recorded are kept).
pub fn stop() {
    REPLAY.with(|r| r.borrow_mut().mode = Mode::Idle);
}

/// Forget what was recorded.
pub fn clear() {
    REPLAY.with(|r| {
        let mut r = r.borrow_mut();
        let live = std::mem::take(&mut r.live);
        *r = Replay { live, ..Replay::default() };
    });
}

/// Whether the player pressed `key` this frame - the live keyboard, even
/// while a replay is feeding everything else its recorded input.
pub fn live_key(key: &str) -> bool {
    KeyCode::from_name(key).is_some_and(|k| REPLAY.with(|r| r.borrow().live.contains(&k)))
}

pub fn mode() -> Mode {
    REPLAY.with(|r| r.borrow().mode)
}

/// How many frames were recorded.
pub fn frames() -> usize {
    REPLAY.with(|r| r.borrow().dts.len())
}

/// How far playback has got, 0 to 1 (1 once finished, 0 otherwise).
pub fn progress() -> f32 {
    REPLAY.with(|r| {
        let r = r.borrow();
        match r.mode {
            Mode::Playing if !r.dts.is_empty() => r.dt_at as f32 / r.dts.len() as f32,
            Mode::Finished => 1.0,
            _ => 0.0,
        }
    })
}

/// A scene has been replaced: an armed replay starts; one under way ends
/// (a recording keeps what it has).
pub fn on_scene_replaced() {
    REPLAY.with(|r| {
        let mut r = r.borrow_mut();
        match r.mode {
            Mode::ArmedRecord => {
                r.dts.clear();
                r.inputs.clear();
                r.mode = Mode::Recording;
            }
            Mode::ArmedPlay => {
                r.dt_at = 0;
                r.input_at = 0;
                r.mode = Mode::Playing;
            }
            Mode::Recording => r.mode = Mode::Idle,
            Mode::Playing => r.mode = Mode::Finished,
            _ => {}
        }
    });
}

/// The frame time the world is given: `raw` live, kept while recording, the
/// kept time while playing. Every host's dt goes through this, once a frame.
pub fn frame_dt(raw: f32) -> f32 {
    REPLAY.with(|r| {
        let mut r = r.borrow_mut();
        match r.mode {
            Mode::Recording => {
                r.dts.push(raw);
                raw
            }
            Mode::Playing => match r.dts.get(r.dt_at).copied() {
                Some(dt) => {
                    r.dt_at += 1;
                    dt
                }
                None => {
                    r.mode = Mode::Finished;
                    raw
                }
            },
            _ => raw,
        }
    })
}

/// The frame's input: kept while recording, replaced by the kept frame's
/// while playing. Called at the end of `InputState::update_interactions`.
pub fn apply(input: &mut InputState) {
    REPLAY.with(|r| {
        let mut r = r.borrow_mut();
        r.live.clone_from(&input.just_pressed);
        match r.mode {
            Mode::Recording => r.inputs.push(Frame::of(input)),
            Mode::Playing => match r.inputs.get(r.input_at) {
                Some(f) => {
                    f.restore(input);
                    r.input_at += 1;
                }
                None => r.mode = Mode::Finished,
            },
            _ => {}
        }
    });
}

/// `Replay.*` for scripts: one entry every runtime calls (Luau globals
/// natively, one `state.replay` hook in the browsers). Returns what a getter
/// answers; `arg` is a key name for `live_key`.
pub fn script_op(op: &str, arg: &str) -> serde_json::Value {
    use serde_json::json;
    match op {
        "record" => {
            record();
            json!(null)
        }
        "play" => json!(play()),
        "stop" => {
            stop();
            json!(null)
        }
        "clear" => {
            clear();
            json!(null)
        }
        "status" => json!(mode().name()),
        "frames" => json!(frames()),
        "progress" => json!(progress()),
        "live_key" => json!(live_key(arg)),
        _ => json!(null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replay_arms_starts_on_a_scene_load_and_finishes() {
        clear();
        assert!(!play(), "nothing to play yet");
        record();
        assert_eq!(mode(), Mode::ArmedRecord);
        assert_eq!(frame_dt(0.5), 0.5, "armed is not recording");
        on_scene_replaced();
        assert_eq!(mode(), Mode::Recording);
        let mut input = InputState::new();
        for (k, dt) in [0.016f32, 0.020, 0.013].into_iter().enumerate() {
            frame_dt(dt);
            input.mouse_position = (k as f32, 0.0);
            apply(&mut input);
        }
        assert_eq!(frames(), 3);
        assert!(play());
        on_scene_replaced();
        assert_eq!(mode(), Mode::Playing);
        let mut live = InputState::new();
        let mut got = Vec::new();
        for _ in 0..3 {
            got.push(frame_dt(0.1));
            live.mouse_position = (99.0, 99.0);
            // The player's own key: under the replay, `live_key` still sees it.
            live.just_pressed = HashSet::from([KeyCode::R]);
            apply(&mut live);
            assert_eq!(live.mouse_position.1, 0.0, "the kept input, not the live");
            assert!(live_key("R"), "the player's key reaches live_key");
            assert!(!live.just_pressed.contains(&KeyCode::R), "everything else reads the replay");
        }
        assert_eq!(got, vec![0.016, 0.020, 0.013], "the kept times");
        assert!((progress() - 1.0).abs() < 1e-6);
        assert_eq!(frame_dt(0.1), 0.1, "past the end: live time");
        assert_eq!(mode(), Mode::Finished);
        clear();
    }

    #[test]
    fn a_scene_load_mid_recording_ends_it_keeping_what_it_has() {
        clear();
        record();
        on_scene_replaced();
        frame_dt(0.016);
        on_scene_replaced();
        assert_eq!(mode(), Mode::Idle);
        assert_eq!(frames(), 1);
        clear();
    }
}
