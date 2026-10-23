//! Music that crossfades, plays in layers and runs a playlist, and sounds a
//! script can still reach after it played them (Oct 2026).
//!
//! Music used to be one track at a time, swapped hard, in three copies (the
//! native `ScriptCommand::PlayMusic` arm and each browser host's `pm` arm,
//! each with its own idea of "the" music handle). Now every host hands its
//! backend to [`drive`] once a frame and one [`Director`] decides what plays:
//!
//! * **crossfade** - `Audio.crossfade_music(clip, seconds)` fades the old
//!   music out while the new one fades in; `Audio.play_music` is the same
//!   with no fade;
//! * **layers** (stems) - `Audio.play_music_layers({ drums, bass, lead },
//!   { 1, 0, 0 })` starts every stem in the same frame, and
//!   `Audio.set_music_layer(i, volume, seconds)` fades one in or out, which
//!   is adaptive music;
//! * **a playlist** - `Audio.set_playlist({ ... }, { crossfade, shuffle,
//!   loop })` plays its tracks through, crossfading into the next one before
//!   the current one ends; `Audio.next_track()` skips;
//! * **pitch and seek** - `Audio.set_music_pitch`, `Audio.seek_music`, and
//!   `Audio.set_sound_pitch(id, pitch)` on a one-shot.
//!
//! The fades run on the wall clock, as input does: music is the player's
//! time, and a paused game still finishes a crossfade into its pause music.
//!
//! **Sound ids.** `play_sound` returns an id at once, but the sound only
//! starts when the host applies the command, and the backend names it its own
//! way - so for as long as the engine existed that id reached nothing:
//! natively the script's counter and kira's never met, and the browsers
//! returned 0 and dropped `stop_sound` / `set_sound_volume` on the floor.
//! A host now records which backend sound each script id became
//! ([`alias`]) and every per-sound command looks it up ([`resolve`]).
//!
//! Requests are the process's (a `thread_local`, as the display and
//! streaming requests are): scripts write them directly - Luau globals
//! natively, one `state.music(op, args)` hook in the browsers - through the
//! one parser [`script_op`].

use std::cell::RefCell;
use std::collections::HashMap;

use serde_json::Value;

/// What the director needs of a backend: start, steer and stop sounds on the
/// music bus. Volumes are the sound's **own** - the implementation puts them
/// through the mixer's buses.
pub trait MusicOut {
    /// Start `clip` on the music bus; `None` when it cannot yet (a clip a
    /// browser is still decoding), and the director asks again next frame.
    fn start(&mut self, clip: &str, volume: f32, looping: bool, pitch: f32, offset: f64) -> Option<u64>;
    fn set_volume(&mut self, id: u64, volume: f32);
    fn set_pitch(&mut self, id: u64, pitch: f32);
    fn seek(&mut self, id: u64, seconds: f64);
    fn stop(&mut self, id: u64);
    fn pause(&mut self, id: u64);
    fn resume(&mut self, id: u64);
    fn position(&self, id: u64) -> Option<f64>;
    fn duration(&self, clip: &str) -> Option<f64>;
    fn is_playing(&self, id: u64) -> bool;
    /// Play a synthesised note (`audio::analysis::Voice`) on `bus`: its
    /// samples, loaded as the clip `name` the first time.
    fn play_voice(&mut self, _name: &str, _samples: &[f32], _rate: u32, _bus: &str) -> Option<u64> {
        None
    }
    /// Hand the spectrum the mix's latest samples, where the backend has to
    /// be asked for them (the browsers; natively a kira effect feeds it).
    fn feed_tap(&mut self) {}
}

/// One stem or track, playing or fading.
#[derive(Debug, Clone)]
struct Voice {
    id: Option<u64>,
    clip: String,
    /// Which layer of the current music it is (0 for a single track).
    layer: usize,
    gain: f32,
    target: f32,
    /// Gain per second toward `target`; infinite jumps there.
    rate: f32,
    /// Fading out for good: stopped when silent.
    leaving: bool,
    looping: bool,
    /// What `out` was last told, so an unchanged volume is not resent.
    sent: f32,
}

#[derive(Debug, Clone, Default)]
struct Playlist {
    clips: Vec<String>,
    /// Play order: indices into `clips`, reshuffled each round if asked.
    order: Vec<usize>,
    at: usize,
    crossfade: f32,
    shuffle: bool,
    looping: bool,
    volume: f32,
    seed: u64,
}

impl Playlist {
    fn reorder(&mut self, avoid_first: Option<usize>) {
        self.order = (0..self.clips.len()).collect();
        if self.shuffle {
            for i in (1..self.order.len()).rev() {
                self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let j = ((self.seed >> 33) as usize) % (i + 1);
                self.order.swap(i, j);
            }
            // A new round does not open with the track that closed the last.
            if self.order.len() > 1 && avoid_first == self.order.first().copied() {
                self.order.swap(0, 1);
            }
        }
        self.at = 0;
    }

    fn current(&self) -> Option<&str> {
        self.order.get(self.at).and_then(|&i| self.clips.get(i)).map(String::as_str)
    }
}

/// A request a script made.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    /// Replace the music with these layers (`(clip, volume)`), fading over
    /// `fade` seconds; an empty list fades to silence.
    Play { layers: Vec<(String, f32)>, looping: bool, fade: f32 },
    Layer { index: usize, volume: f32, fade: f32 },
    Playlist { clips: Vec<String>, crossfade: f32, shuffle: bool, looping: bool, volume: f32 },
    Next,
    Pause,
    Resume,
    Volume(f32),
    Pitch(f32),
    Seek(f64),
    SoundPitch { id: u64, pitch: f32 },
}

/// The music, as the scripts asked for it.
#[derive(Debug, Default)]
pub struct Director {
    pending: Vec<Request>,
    voices: Vec<Voice>,
    playlist: Option<Playlist>,
    volume: f32,
    pitch: f32,
    paused: bool,
    last: Option<f64>,
    clip: String,
    position: Option<f64>,
    layers: Vec<f32>,
    /// The world's time pitch the voices were last told
    /// (`time::audio_time_pitch`).
    time_pitch: f32,
}

impl Director {
    pub fn new() -> Self {
        Director { volume: 1.0, pitch: 1.0, time_pitch: 1.0, ..Default::default() }
    }

    pub fn request(&mut self, r: Request) {
        self.pending.push(r);
    }

    /// The clip of the music playing now (the first layer's), empty for none.
    pub fn clip(&self) -> &str {
        &self.clip
    }

    /// Seconds into the music playing now.
    pub fn position(&self) -> Option<f64> {
        self.position
    }

    /// The backend ids of the music playing now (not what is fading out).
    pub fn playing_ids(&self) -> Vec<u64> {
        self.voices.iter().filter(|v| !v.leaving).filter_map(|v| v.id).collect()
    }

    /// Where layer `index` is heading.
    pub fn layer_volume(&self, index: usize) -> f32 {
        self.layers.get(index).copied().unwrap_or(0.0)
    }

    fn replace(&mut self, layers: Vec<(String, f32)>, looping: bool, fade: f32, out: &mut dyn MusicOut) {
        for v in &mut self.voices {
            if !v.leaving {
                v.leaving = true;
                v.target = 0.0;
                v.rate = if fade > 0.0 { v.gain.max(0.001) / fade } else { f32::INFINITY };
            }
        }
        self.layers = layers.iter().map(|(_, vol)| *vol).collect();
        for (layer, (clip, vol)) in layers.into_iter().enumerate() {
            if clip.is_empty() {
                continue;
            }
            let gain = if fade > 0.0 { 0.0 } else { vol };
            self.voices.push(Voice {
                id: None,
                clip,
                layer,
                gain,
                target: vol,
                rate: if fade > 0.0 { vol.max(0.001) / fade } else { f32::INFINITY },
                leaving: false,
                looping,
                sent: -1.0,
            });
        }
        // Started in this frame together, so the stems stay in time.
        self.start_waiting(out);
    }

    fn start_waiting(&mut self, out: &mut dyn MusicOut) {
        let (volume, pitch) = (self.volume, self.pitch * self.time_pitch);
        let mut started = Vec::new();
        for v in self.voices.iter_mut().filter(|v| v.id.is_none() && !v.leaving) {
            v.id = out.start(&v.clip, v.gain * volume, v.looping, pitch, 0.0);
            v.sent = v.gain * volume;
            if let Some(id) = v.id {
                started.push(id);
            }
        }
        if self.paused {
            for id in started {
                out.pause(id);
            }
        }
    }

    fn play_track(&mut self, out: &mut dyn MusicOut) {
        let Some(list) = self.playlist.as_ref() else { return };
        let Some(clip) = list.current().map(str::to_string) else { return };
        let (fade, vol) = (list.crossfade, list.volume);
        // Its tracks end, so the list can move on: never looped.
        self.replace(vec![(clip, vol)], false, fade, out);
    }

    fn advance(&mut self, out: &mut dyn MusicOut) {
        let Some(list) = self.playlist.as_mut() else { return };
        list.at += 1;
        if list.at >= list.order.len() {
            if !list.looping {
                let fade = list.crossfade;
                self.playlist = None;
                self.replace(Vec::new(), false, fade, out);
                return;
            }
            let last = list.order.last().copied();
            list.reorder(last);
        }
        self.play_track(out);
    }

    fn apply(&mut self, r: Request, out: &mut dyn MusicOut) {
        match r {
            Request::Play { layers, looping, fade } => {
                self.playlist = None;
                self.replace(layers, looping, fade, out);
            }
            Request::Layer { index, volume, fade } => {
                if let Some(l) = self.layers.get_mut(index) {
                    *l = volume;
                }
                for v in self.voices.iter_mut().filter(|v| !v.leaving && v.layer == index) {
                    v.target = volume;
                    v.rate = if fade > 0.0 { (volume - v.gain).abs().max(0.001) / fade } else { f32::INFINITY };
                }
            }
            Request::Playlist { clips, crossfade, shuffle, looping, volume } => {
                let mut list = Playlist {
                    clips,
                    crossfade,
                    shuffle,
                    looping,
                    volume,
                    seed: (crate::profiling::unix_time() * 1000.0) as u64 | 1,
                    ..Default::default()
                };
                list.reorder(None);
                self.playlist = Some(list);
                self.play_track(out);
            }
            Request::Next => self.advance(out),
            Request::Pause => {
                self.paused = true;
                for id in self.voices.iter().filter_map(|v| v.id) {
                    out.pause(id);
                }
            }
            Request::Resume => {
                self.paused = false;
                for id in self.voices.iter().filter_map(|v| v.id) {
                    out.resume(id);
                }
            }
            Request::Volume(v) => self.volume = v.max(0.0),
            Request::Pitch(p) => {
                self.pitch = p.max(0.01);
                for id in self.voices.iter().filter_map(|v| v.id) {
                    out.set_pitch(id, self.pitch * self.time_pitch);
                }
            }
            Request::Seek(s) => {
                for id in self.voices.iter().filter(|v| !v.leaving).filter_map(|v| v.id) {
                    out.seek(id, s.max(0.0));
                }
            }
            Request::SoundPitch { id, pitch } => out.set_pitch(resolve(id), pitch.max(0.01)),
        }
    }

    /// Bring the music up to `now` (seconds, any clock): apply what scripts
    /// asked for, start what waits, step every fade, let go of what has gone
    /// silent or ended, and move a playlist on.
    pub fn tick(&mut self, now: f64, out: &mut dyn MusicOut) {
        let dt = self.last.map(|l| (now - l).clamp(0.0, 0.25) as f32).unwrap_or(0.0);
        self.last = Some(now);
        // Audio following time: the voices bend with it.
        let tp = crate::time::audio_time_pitch();
        if (tp - self.time_pitch).abs() > 1e-4 {
            self.time_pitch = tp;
            for id in self.voices.iter().filter_map(|v| v.id) {
                out.set_pitch(id, self.pitch * tp);
            }
        }
        for r in std::mem::take(&mut self.pending) {
            self.apply(r, out);
        }
        self.start_waiting(out);
        let volume = self.volume;
        let paused = self.paused;
        for v in &mut self.voices {
            if !paused {
                v.gain = if v.rate.is_infinite() {
                    v.target
                } else if v.gain < v.target {
                    (v.gain + v.rate * dt).min(v.target)
                } else {
                    (v.gain - v.rate * dt).max(v.target)
                };
            }
            let out_gain = v.gain * volume;
            if let Some(id) = v.id
                && (out_gain - v.sent).abs() > 1e-4
            {
                out.set_volume(id, out_gain);
                v.sent = out_gain;
            }
        }
        // Gone silent on the way out, or finished by itself.
        self.voices.retain(|v| {
            let silent = v.leaving && v.gain <= 0.0;
            let ended = v.id.is_some_and(|id| !out.is_playing(id)) && !paused;
            if silent && let Some(id) = v.id {
                out.stop(id);
            }
            !(silent || ended)
        });
        // The playlist moves on a crossfade before the track ends, or when it
        // has ended.
        if !paused && let Some(list) = self.playlist.as_ref() {
            let fade = list.crossfade as f64;
            let current = self.voices.iter().find(|v| !v.leaving);
            let due = match current {
                None => list.current().is_some() && self.voices.iter().all(|v| v.id.is_some()),
                Some(v) => match (v.id.and_then(|id| out.position(id)), out.duration(&v.clip)) {
                    (Some(p), Some(d)) => d > 0.0 && p >= d - fade.min(d * 0.5),
                    _ => false,
                },
            };
            if due {
                self.advance(out);
            }
        }
        let main = self.voices.iter().find(|v| !v.leaving && v.id.is_some());
        self.clip = main.map(|v| v.clip.clone()).unwrap_or_default();
        self.position = main.and_then(|v| v.id).and_then(|id| out.position(id));
        // Aliases of sounds that are gone go too.
        ALIASES.with(|a| a.borrow_mut().retain(|_, b| out.is_playing(*b)));
    }
}

thread_local! {
    static DIRECTOR: RefCell<Director> = RefCell::new(Director::new());
    static ALIASES: RefCell<HashMap<u64, u64>> = RefCell::new(HashMap::new());
}

/// Ask for something of the music.
pub fn request(r: Request) {
    DIRECTOR.with(|d| d.borrow_mut().request(r));
}

/// Fade the music out over `seconds` (0: at once) and drop any playlist.
pub fn stop(seconds: f32) {
    request(Request::Play { layers: Vec::new(), looping: false, fade: seconds.max(0.0) });
}

/// Read the director.
pub fn with<R>(f: impl FnOnce(&Director) -> R) -> R {
    DIRECTOR.with(|d| f(&d.borrow()))
}

/// The host's step, once a frame after its audio update: drive the music on
/// `out` and publish where it is (`get_music_position`).
pub fn drive(out: &mut dyn MusicOut) {
    drive_at(crate::profiling::unix_time(), out);
}

/// [`drive`] on a given clock.
pub fn drive_at(now: f64, out: &mut dyn MusicOut) {
    let position = DIRECTOR.with(|d| {
        let mut d = d.borrow_mut();
        d.tick(now, out);
        d.position()
    });
    super::publish_music_position(position);
    // The spectrum's samples and the synth's notes ride on the same step.
    out.feed_tap();
    for v in super::analysis::take() {
        let samples = v.render(super::analysis::SYNTH_RATE);
        out.play_voice(&v.clip_name(), &samples, super::analysis::SYNTH_RATE, &v.bus);
    }
}

/// The world was replaced and the backend's sounds are gone: forget them.
/// Requests made since stay.
pub fn reset() {
    DIRECTOR.with(|d| {
        let mut d = d.borrow_mut();
        let pending = std::mem::take(&mut d.pending);
        *d = Director::new();
        d.pending = pending;
    });
    ALIASES.with(|a| a.borrow_mut().clear());
}

/// Record that the script's sound `script_id` is the backend's `backend_id`.
pub fn alias(script_id: u64, backend_id: u64) {
    if script_id != 0 {
        ALIASES.with(|a| a.borrow_mut().insert(script_id, backend_id));
    }
}

/// The backend's id for a script's sound id (itself, if it was never aliased).
pub fn resolve(script_id: u64) -> u64 {
    ALIASES.with(|a| a.borrow().get(&script_id).copied().unwrap_or(script_id))
}

fn num(v: Option<&Value>, default: f64) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(default)
}

fn clips(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a.iter().filter_map(|c| c.as_str().map(str::to_string)).collect(),
        // A Lua table with keys 1..n can arrive as an object.
        Some(Value::Object(o)) => {
            let mut keyed: Vec<(usize, String)> = o
                .iter()
                .filter_map(|(k, c)| Some((k.parse().ok()?, c.as_str()?.to_string())))
                .collect();
            keyed.sort();
            keyed.into_iter().map(|(_, c)| c).collect()
        }
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn numbers(v: Option<&Value>) -> Vec<f32> {
    match v {
        Some(Value::Array(a)) => a.iter().map(|n| n.as_f64().unwrap_or(0.0) as f32).collect(),
        Some(Value::Object(o)) => {
            let mut keyed: Vec<(usize, f32)> =
                o.iter().filter_map(|(k, n)| Some((k.parse().ok()?, n.as_f64()? as f32))).collect();
            keyed.sort_by_key(|(k, _)| *k);
            keyed.into_iter().map(|(_, n)| n).collect()
        }
        _ => Vec::new(),
    }
}

/// Every script function of this module, by name, its arguments as JSON -
/// the one parser both the Luau globals and the browsers' hook call. Returns
/// what a getter answers (`Null` for the rest).
pub fn script_op(op: &str, args: &[Value]) -> Value {
    let a = |i: usize| args.get(i).filter(|v| !v.is_null());
    match op {
        "crossfade" => {
            let clip = a(0).and_then(Value::as_str).unwrap_or_default().to_string();
            let fade = num(a(1), 1.0) as f32;
            let vol = num(a(2), 1.0) as f32;
            let looping = a(3).and_then(Value::as_bool).unwrap_or(true);
            let layers = if clip.is_empty() { Vec::new() } else { vec![(clip, vol)] };
            request(Request::Play { layers, looping, fade: fade.max(0.0) });
        }
        "layers" => {
            let names = clips(a(0));
            let vols = numbers(a(1));
            let fade = num(a(2), 0.0) as f32;
            let layers = names.into_iter().enumerate().map(|(i, c)| (c, vols.get(i).copied().unwrap_or(1.0))).collect();
            request(Request::Play { layers, looping: true, fade: fade.max(0.0) });
        }
        "layer" => {
            // Lua counts layers from 1.
            let index = (num(a(0), 1.0) as i64 - 1).max(0) as usize;
            request(Request::Layer { index, volume: num(a(1), 1.0) as f32, fade: num(a(2), 0.5).max(0.0) as f32 });
        }
        "playlist" => {
            let opts = a(1);
            let get = |k: &str| opts.and_then(|o| o.get(k));
            request(Request::Playlist {
                clips: clips(a(0)),
                crossfade: num(get("crossfade"), 2.0).max(0.0) as f32,
                shuffle: get("shuffle").and_then(Value::as_bool).unwrap_or(false),
                looping: get("loop").and_then(Value::as_bool).unwrap_or(true),
                volume: num(get("volume"), 1.0) as f32,
            });
        }
        "next" => request(Request::Next),
        "pitch" => request(Request::Pitch(num(a(0), 1.0) as f32)),
        "seek" => request(Request::Seek(num(a(0), 0.0))),
        "sound_pitch" => request(Request::SoundPitch { id: num(a(0), 0.0).max(0.0) as u64, pitch: num(a(1), 1.0) as f32 }),
        "clip" => return Value::String(with(|d| d.clip().to_string())),
        "layer_volume" => {
            let index = (num(a(0), 1.0) as i64 - 1).max(0) as usize;
            return serde_json::json!(with(|d| d.layer_volume(index)));
        }
        other => log::warn!("[music] unknown op '{other}'"),
    }
    Value::Null
}

/// The native backend as a [`MusicOut`]: kira through the mixer's music bus,
/// decoding a clip from the assets the first time it plays.
pub struct NativeOut<'a> {
    pub audio: &'a mut dyn super::AudioBackend,
    pub assets: &'a crate::asset::AssetManager,
    pub mixer: &'a mut super::AudioMixer,
}

impl MusicOut for NativeOut<'_> {
    fn play_voice(&mut self, name: &str, samples: &[f32], rate: u32, bus: &str) -> Option<u64> {
        if self.audio.get_clip_by_name(name).is_none()
            && let Err(e) = self.audio.load_clip(name, &super::analysis::wav_bytes(samples, rate))
        {
            log::warn!("[synth] could not load a note: {e}");
            return None;
        }
        let handle = self.audio.get_clip_by_name(name)?;
        let settings = super::PlaySettings {
            volume: self.mixer.output_volume(bus, 1.0),
            pitch: 1.0,
            looping: false,
            pan: 0.0,
            bus: bus.to_string(),
            filter: false,
        };
        self.audio.play(handle, settings).ok().map(|h| h.0)
    }
    fn start(&mut self, clip: &str, volume: f32, looping: bool, pitch: f32, offset: f64) -> Option<u64> {
        if self.audio.get_clip_by_name(clip).is_none() {
            let data = self.assets.get_audio_data(clip)?;
            if let Err(e) = self.audio.load_clip(clip, data) {
                log::warn!("[music] could not load '{clip}': {e}");
                return None;
            }
        }
        let handle = self.audio.get_clip_by_name(clip)?;
        let settings = super::PlaySettings {
            volume: self.mixer.output_volume(super::MUSIC_BUS, volume),
            pitch,
            looping,
            pan: 0.0,
            bus: super::MUSIC_BUS.to_string(),
            filter: false,
        };
        let id = self.audio.play(handle, settings).ok()?.0;
        self.mixer.register_track(id, super::MUSIC_BUS, volume);
        if offset > 0.0 {
            self.audio.seek(super::SoundHandle(id), offset);
        }
        Some(id)
    }
    fn set_volume(&mut self, id: u64, volume: f32) {
        if let Some(out) = self.mixer.set_track_volume(id, volume) {
            self.audio.set_volume(super::SoundHandle(id), out);
        }
    }
    fn set_pitch(&mut self, id: u64, pitch: f32) {
        self.audio.set_pitch(super::SoundHandle(id), pitch);
    }
    fn seek(&mut self, id: u64, seconds: f64) {
        self.audio.seek(super::SoundHandle(id), seconds);
    }
    fn stop(&mut self, id: u64) {
        self.audio.stop(super::SoundHandle(id));
        self.mixer.unregister_track(id);
    }
    fn pause(&mut self, id: u64) {
        self.audio.pause(super::SoundHandle(id));
    }
    fn resume(&mut self, id: u64) {
        self.audio.resume(super::SoundHandle(id));
    }
    fn position(&self, id: u64) -> Option<f64> {
        self.audio.position(super::SoundHandle(id))
    }
    fn duration(&self, clip: &str) -> Option<f64> {
        self.audio.clip_duration(self.audio.get_clip_by_name(clip)?)
    }
    fn is_playing(&self, id: u64) -> bool {
        self.audio.is_playing(super::SoundHandle(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend on paper: what started, at what volume, where each playhead
    /// is (advanced by hand), and clips `len` seconds long.
    #[derive(Default)]
    struct Paper {
        next: u64,
        sounds: HashMap<u64, (String, f32, f64, bool, f32)>, // clip, volume, position, paused, pitch
        len: f64,
        log: Vec<String>,
    }

    impl Paper {
        fn playing(&self) -> Vec<(String, f32)> {
            let mut v: Vec<_> = self.sounds.values().map(|s| (s.0.clone(), (s.1 * 100.0).round() / 100.0)).collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        }
        fn advance(&mut self, s: f64) {
            for v in self.sounds.values_mut().filter(|v| !v.3) {
                v.2 += s;
            }
            let len = self.len;
            self.sounds.retain(|_, v| v.2 < len);
        }
    }

    impl MusicOut for Paper {
        fn start(&mut self, clip: &str, volume: f32, _l: bool, pitch: f32, offset: f64) -> Option<u64> {
            self.next += 1;
            self.sounds.insert(self.next, (clip.into(), volume, offset, false, pitch));
            self.log.push(format!("start {clip}"));
            Some(self.next)
        }
        fn set_volume(&mut self, id: u64, volume: f32) {
            if let Some(s) = self.sounds.get_mut(&id) {
                s.1 = volume;
            }
        }
        fn set_pitch(&mut self, id: u64, pitch: f32) {
            if let Some(s) = self.sounds.get_mut(&id) {
                s.4 = pitch;
            }
        }
        fn seek(&mut self, id: u64, seconds: f64) {
            if let Some(s) = self.sounds.get_mut(&id) {
                s.2 = seconds;
            }
        }
        fn stop(&mut self, id: u64) {
            if let Some(s) = self.sounds.remove(&id) {
                self.log.push(format!("stop {}", s.0));
            }
        }
        fn pause(&mut self, id: u64) {
            if let Some(s) = self.sounds.get_mut(&id) {
                s.3 = true;
            }
        }
        fn resume(&mut self, id: u64) {
            if let Some(s) = self.sounds.get_mut(&id) {
                s.3 = false;
            }
        }
        fn position(&self, id: u64) -> Option<f64> {
            self.sounds.get(&id).map(|s| s.2)
        }
        fn duration(&self, _clip: &str) -> Option<f64> {
            Some(self.len)
        }
        fn is_playing(&self, id: u64) -> bool {
            self.sounds.contains_key(&id)
        }
    }

    fn play(d: &mut Director, clip: &str, fade: f32) {
        d.request(Request::Play { layers: vec![(clip.into(), 1.0)], looping: true, fade });
    }

    #[test]
    fn a_crossfade_trades_one_track_for_another() {
        let (mut d, mut out) = (Director::new(), Paper { len: 100.0, ..Default::default() });
        play(&mut d, "a", 0.0);
        d.tick(0.0, &mut out);
        assert_eq!(out.playing(), vec![("a".into(), 1.0)]);
        play(&mut d, "b", 2.0);
        for k in 0..=4 {
            d.tick(k as f64 * 0.25, &mut out);
        }
        assert_eq!(out.playing(), vec![("a".into(), 0.5), ("b".into(), 0.5)], "halfway through");
        for k in 5..=8 {
            d.tick(k as f64 * 0.25, &mut out);
        }
        assert_eq!(out.playing(), vec![("b".into(), 1.0)]);
        assert_eq!(d.clip(), "b");
        assert!(out.log.contains(&"stop a".to_string()), "the old track is let go once silent");
    }

    #[test]
    fn layers_start_together_and_fade_one_by_one() {
        let (mut d, mut out) = (Director::new(), Paper { len: 100.0, ..Default::default() });
        script_op_on(&mut d, "layers", &[serde_json::json!(["drums", "bass", "lead"]), serde_json::json!([1, 0, 0])]);
        d.tick(0.0, &mut out);
        assert_eq!(out.log, vec!["start drums", "start bass", "start lead"], "every stem starts in one frame");
        d.request(Request::Layer { index: 1, volume: 0.8, fade: 0.5 });
        d.tick(0.1, &mut out);
        d.tick(0.35, &mut out);
        d.tick(0.6, &mut out);
        assert_eq!(out.playing(), vec![("bass".into(), 0.8), ("drums".into(), 1.0), ("lead".into(), 0.0)]);
        assert_eq!(d.layer_volume(1), 0.8);
    }

    #[test]
    fn a_playlist_crossfades_into_its_next_track_and_goes_round() {
        let (mut d, mut out) = (Director::new(), Paper { len: 10.0, ..Default::default() });
        d.request(Request::Playlist { clips: vec!["one".into(), "two".into()], crossfade: 2.0, shuffle: false, looping: true, volume: 1.0 });
        d.tick(0.0, &mut out);
        assert_eq!(d.clip(), "one");
        let mut t = 0.0;
        for _ in 0..70 {
            t += 0.25;
            out.advance(0.25);
            d.tick(t, &mut out);
        }
        // 17.5 s: "one" for 8, crossfading into "two" by 10, and "two"
        // handing back to "one" two seconds before its own end, at 16.
        assert_eq!(out.log.iter().filter(|l| l.starts_with("start")).cloned().collect::<Vec<_>>(),
                   vec!["start one", "start two", "start one"]);
    }

    #[test]
    fn a_playlist_that_does_not_loop_fades_out_at_its_end() {
        let (mut d, mut out) = (Director::new(), Paper { len: 4.0, ..Default::default() });
        d.request(Request::Playlist { clips: vec!["only".into()], crossfade: 1.0, shuffle: false, looping: false, volume: 1.0 });
        let mut t = 0.0;
        d.tick(t, &mut out);
        for _ in 0..30 {
            t += 0.25;
            out.advance(0.25);
            d.tick(t, &mut out);
        }
        assert!(out.sounds.is_empty());
        assert_eq!(d.clip(), "");
    }

    #[test]
    fn a_paused_crossfade_holds_and_resumes() {
        let (mut d, mut out) = (Director::new(), Paper { len: 100.0, ..Default::default() });
        play(&mut d, "a", 0.0);
        d.tick(0.0, &mut out);
        play(&mut d, "b", 2.0);
        d.request(Request::Pause);
        d.tick(0.0, &mut out);
        d.tick(0.25, &mut out);
        assert_eq!(out.playing(), vec![("a".into(), 1.0), ("b".into(), 0.0)], "a paused fade does not move");
        assert!(out.sounds.values().all(|s| s.3), "and everything it plays is paused");
        d.request(Request::Resume);
        for k in 1..=5 {
            d.tick(k as f64 * 0.25, &mut out);
        }
        assert_eq!(out.playing(), vec![("a".into(), 0.5), ("b".into(), 0.5)]);
    }

    #[test]
    fn a_script_sound_id_reaches_the_backends_sound() {
        alias(1_000_001, 7);
        assert_eq!(resolve(1_000_001), 7);
        assert_eq!(resolve(42), 42, "an id never aliased is itself");
    }

    fn script_op_on(d: &mut Director, op: &str, args: &[Value]) {
        // `script_op` writes the thread's director; route it to `d`.
        script_op(op, args);
        DIRECTOR.with(|g| {
            for r in std::mem::take(&mut g.borrow_mut().pending) {
                d.request(r);
            }
        });
    }
}
