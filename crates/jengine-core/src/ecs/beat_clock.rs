//! Beat events (Oct 2026): the music's tempo turned into `on_beat` and
//! `on_bar` callbacks, timed by the music's own playhead.
//!
//! A `beat_clock` names a tempo (`bpm`), where the first downbeat falls in
//! the track (`offset`, seconds) and how many beats make a bar. Each frame
//! its clock runs on `dt` and is pulled toward the music's playhead
//! (`audio::music_position`, published by every host after its audio
//! update) - eased when close, snapped when far, as a rhythm game's song
//! clock is: the playhead is the truth but a coarse one, reported in buffer
//! steps a frame late, and a clock summed from `dt` slides off the song
//! within a minute. With no music playing, or `follow_music` off, it runs
//! on `dt` alone - a metronome.
//!
//! When the beat it is in changes, the entity's own script is owed
//! `on_beat(entity)`, and on a downbeat `on_bar(entity)` too
//! (`ecs::callbacks`, so no host was wired). What it reads it reads from the
//! custom `BeatClock` component, written every frame: `beat` (the beat of
//! the song, from 0), `bar`, `beat_in_bar`, `phase` (0 to 1 through the
//! beat - a pulse is `1 - phase`), `count` (beats fired since it started)
//! and `time`. A looping track's playhead jumps back to its start, and the
//! beat with it, so a loop authored in whole bars restarts on a downbeat.

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};

use crate::ecs::components::CustomComponents;

/// How far off the clock may be from a fresh playhead reading before it
/// jumps to it instead of easing.
pub const SNAP: f64 = 0.06;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BeatClock {
    /// Beats a minute.
    pub bpm: f32,
    /// Seconds into the track of the first downbeat.
    pub offset: f32,
    /// Beats in a bar (`on_bar` fires on the first of them).
    pub beats_per_bar: u32,
    /// Follow the music's playhead (false: run on `dt` alone).
    pub follow_music: bool,
    /// Counting (false: the clock holds and nothing fires).
    pub running: bool,
    /// Where the clock is, kept between frames; never saved.
    #[serde(skip)]
    pub state: BeatState,
}

impl Default for BeatClock {
    fn default() -> Self {
        Self { bpm: 120.0, offset: 0.0, beats_per_bar: 4, follow_music: true, running: true, state: BeatState::default() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BeatState {
    /// Seconds into the music (or since starting, free-running).
    pub time: f64,
    /// The beat the clock was in last frame.
    pub beat: Option<i64>,
    /// Beats fired.
    pub count: u64,
    last_reading: Option<f64>,
}

impl BeatState {
    /// Run the clock `dt` on and pull it toward `audio`, the playhead as last
    /// published (`None` with no music).
    pub fn advance(&mut self, dt: f64, audio: Option<f64>) {
        self.time += dt;
        let Some(a) = audio else {
            self.last_reading = None;
            return;
        };
        // A reading that has not moved says nothing new - the device has not
        // rendered another buffer - and would pull the clock back.
        let fresh = self.last_reading.is_none_or(|last| (a - last).abs() > 1e-9);
        let first = self.last_reading.is_none();
        self.last_reading = Some(a);
        if !fresh {
            return;
        }
        let err = a - self.time;
        if first || err.abs() > SNAP {
            // The music started, looped or was sought: be where it is.
            self.time = a;
        } else {
            self.time += err * (dt * 8.0).min(1.0);
        }
    }
}

/// The beat (from 0) a clock at `time` seconds is in, and how far through it.
pub fn beat_at(time: f64, bpm: f32, offset: f32) -> (i64, f64) {
    let spb = 60.0 / (bpm.max(1.0) as f64);
    let b = (time - offset as f64) / spb;
    let whole = b.floor();
    (whole as i64, b - whole)
}

/// One frame of every beat clock: advance it, fire `on_beat` / `on_bar` when
/// its beat changes, and write what a script reads. Called from
/// `tick_motion`.
pub fn tick_beat_clocks(world: &mut World, dt: f32) {
    let audio = crate::audio::music_position();
    let mut out: Vec<(Entity, serde_json::Value)> = Vec::new();
    for (e, c) in world.query_mut::<&mut BeatClock>() {
        if !c.running {
            continue;
        }
        let reading = if c.follow_music { audio } else { None };
        // `dt` is already the world's scaled time (`Time.set_scale`).
        c.state.advance(dt.max(0.0) as f64, reading);
        let (beat, phase) = beat_at(c.state.time, c.bpm, c.offset);
        let bpb = c.beats_per_bar.max(1) as i64;
        if c.state.beat.is_some_and(|last| last != beat) {
            c.state.count += 1;
            crate::ecs::callbacks::push(e, "on_beat");
            if beat.rem_euclid(bpb) == 0 {
                crate::ecs::callbacks::push(e, "on_bar");
            }
        }
        c.state.beat = Some(beat);
        out.push((
            e,
            serde_json::json!({
                "beat": beat,
                "bar": beat.div_euclid(bpb),
                "beat_in_bar": beat.rem_euclid(bpb),
                "phase": phase,
                "count": c.state.count,
                "time": c.state.time,
            }),
        ));
    }
    for (e, v) in out {
        let missing = match world.get::<&mut CustomComponents>(e) {
            Ok(mut c) => {
                c.0.insert("BeatClock".into(), v);
                None
            }
            Err(_) => Some(v),
        };
        if let Some(v) = missing {
            let mut map = std::collections::HashMap::new();
            map.insert("BeatClock".into(), v);
            let _ = world.insert_one(e, CustomComponents(map));
        }
    }
}

/// `BeatClock.set(e, field, value)`: any field by name. Setting `bpm` or
/// `offset` keeps the clock's time, so the beat moves with the tempo.
pub fn set_beat_clock_field(world: &World, entity: Entity, field: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut c = world.get::<&mut BeatClock>(entity).map_err(|_| "no beat_clock on that entity".to_string())?;
    let state = c.state.clone();
    let mut whole = serde_json::to_value(&*c).map_err(|e| e.to_string())?;
    let map = whole.as_object_mut().ok_or("not an object")?;
    if !map.contains_key(field) {
        return Err(format!("'{field}' is not a beat_clock field"));
    }
    map.insert(field.to_string(), value.clone());
    *c = serde_json::from_value(whole).map_err(|e| format!("'{field}': {e}"))?;
    c.state = state;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_beat_is_sixty_over_the_tempo() {
        assert_eq!(beat_at(0.0, 120.0, 0.0), (0, 0.0));
        let (b, p) = beat_at(1.25, 120.0, 0.0);
        assert_eq!(b, 2);
        assert!((p - 0.5).abs() < 1e-9);
        // Before the first downbeat is beat -1.
        assert_eq!(beat_at(0.1, 120.0, 0.2).0, -1);
    }

    #[test]
    fn the_clock_eases_toward_the_playhead_and_snaps_when_far() {
        let mut s = BeatState::default();
        s.advance(0.016, Some(1.0));
        assert_eq!(s.time, 1.0, "the first reading is where the music is");
        s.advance(0.016, Some(1.026));
        assert!((s.time - 1.026).abs() < 0.01, "close: eased toward it ({})", s.time);
        // A stale reading (no new buffer) does not pull it back.
        let before = s.time;
        s.advance(0.016, Some(1.026));
        assert!((s.time - (before + 0.016)).abs() < 1e-9);
        // The track looped: the playhead jumped back, so does the clock.
        s.advance(0.016, Some(0.01));
        assert_eq!(s.time, 0.01);
        // No music: a metronome on dt.
        s.advance(0.5, None);
        assert!((s.time - 0.51).abs() < 1e-9);
    }
}
