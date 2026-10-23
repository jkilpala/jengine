//! Sounds that never play the same twice (Oct 2026; Godot's
//! `AudioStreamRandomizer`, FMOD's pitch and volume randomisation): a clip
//! given a variation plays each one-shot at a pitch up to `pitch` semitones
//! either way and a volume up to `volume` (a fraction) either way of what was
//! asked - footsteps, impacts, gunfire. Set per clip by
//! `Audio.set_variation(clip, pitch, volume)` and kept for the process, like
//! the bus tree, so it reaches every way a sound starts - a script, an
//! `audio_source`, an animation frame's `PlaySound` - because both backends
//! apply it where a one-shot is played (`kira_backend`'s `play`, the
//! browsers' `play_sound_routed`). Music and looping sounds are left alone.

use std::collections::HashMap;
use std::sync::Mutex;

/// How far a clip's one-shots wander: semitones of pitch and a fraction of
/// volume, either way.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Variation {
    pub pitch: f32,
    pub volume: f32,
}

struct State {
    clips: HashMap<String, Variation>,
    seed: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let state = guard.get_or_insert_with(|| State { clips: HashMap::new(), seed: 0x2545_F491_4F6C_DD1D });
    f(state)
}

/// `Audio.set_variation(clip, pitch, volume)`: `pitch` in semitones (0 to
/// 12), `volume` a fraction (0 to 1). Both 0 removes the clip's variation.
pub fn set(clip: &str, pitch: f32, volume: f32) {
    let v = Variation {
        pitch: if pitch.is_finite() { pitch.clamp(0.0, 12.0) } else { 0.0 },
        volume: if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 0.0 },
    };
    with(|s| {
        if v == Variation::default() {
            s.clips.remove(clip);
        } else {
            s.clips.insert(clip.to_string(), v);
        }
    });
}

/// A clip's variation, if it has one.
pub fn get(clip: &str) -> Option<Variation> {
    with(|s| s.clips.get(clip).copied())
}

/// Every variation, for a test or a host starting afresh.
pub fn clear() {
    with(|s| s.clips.clear());
}

/// Seed the dice, so a test can expect a sequence.
pub fn seed(seed: u64) {
    with(|s| s.seed = seed.max(1));
}

/// A number in -1..1 (xorshift; nothing here needs better).
fn roll(s: &mut State) -> f32 {
    let mut x = s.seed;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    s.seed = x;
    ((x >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
}

/// The pitch (a playback rate) and volume a one-shot of `clip` plays at,
/// from the ones it was asked for: unchanged for a clip with no variation.
pub fn apply(clip: &str, pitch: f32, volume: f32) -> (f32, f32) {
    let (pitch, volume) = with(|s| {
        let Some(v) = s.clips.get(clip).copied() else {
            return (pitch, volume);
        };
        let p = roll(s);
        let g = roll(s);
        (pitch * 2f32.powf(p * v.pitch / 12.0), (volume * (1.0 + g * v.volume)).max(0.0))
    });
    // Audio following the world's time (`Audio.follow_time`): a shot fired
    // in slow motion sounds slowed. Every backend's one-shot comes through
    // here, which is why it is here.
    (pitch * crate::time::audio_time_pitch(), volume)
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test, since the table is the process's and tests run in threads.
    #[test]
    fn a_varied_clip_never_plays_the_same_twice() {
        clear();
        seed(7);
        assert_eq!(apply("steps/a.wav", 1.0, 0.8), (1.0, 0.8), "no variation, no change");
        set("steps/a.wav", 2.0, 0.25);
        let plays: Vec<(f32, f32)> = (0..400).map(|_| apply("steps/a.wav", 1.0, 0.8)).collect();
        let (lo, hi) = (2f32.powf(-2.0 / 12.0), 2f32.powf(2.0 / 12.0));
        assert!(plays.iter().all(|(p, v)| *p >= lo - 1e-4 && *p <= hi + 1e-4 && *v >= 0.6 - 1e-4 && *v <= 1.0 + 1e-4));
        let min_p = plays.iter().map(|p| p.0).fold(f32::MAX, f32::min);
        let max_p = plays.iter().map(|p| p.0).fold(f32::MIN, f32::max);
        assert!(min_p < 0.92 && max_p > 1.08, "the whole range is used: {min_p}..{max_p}");
        assert!(plays.windows(2).all(|w| w[0] != w[1]), "never the same twice in a row");
        assert_eq!(apply("other.wav", 1.5, 0.5), (1.5, 0.5), "only the clip it was set on");
        set("steps/a.wav", 0.0, 0.0);
        assert_eq!(get("steps/a.wav"), None, "both 0 removes it");
        clear();
    }
}
