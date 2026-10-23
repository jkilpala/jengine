//! Audio system — trait-based backend for sound effects and music.
//!
//! Native uses kira, WASM uses Web Audio API via JS wrappers.

#[cfg(feature = "audio")]
pub mod kira_backend;

#[cfg(not(target_arch = "wasm32"))]
pub mod audio_asset;

/// Web Audio backend, shared by the editor viewport and the web runtime so
/// the two wasm hosts route script audio commands through one implementation
/// rather than a copy each.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod web_audio;

pub mod mixer;
pub mod spatial;
pub mod zones;
pub use mixer::{AudioBus, AudioMixer, BusEffects, BusLayout, BusPlan, CompressorSettings, MixerLayout, MASTER_BUS, MUSIC_BUS, SFX_BUS};
pub mod variation;
/// Crossfades, stem layers, playlists, pitch and seek, and script sound ids
/// that reach their sound - one director every host drives.
pub mod music;
/// The mix's spectrum and level, and synthesised notes (`Audio.get_spectrum`,
/// `Audio.synth`).
pub mod analysis;
/// Clips a package renders to PCM at load (`jengine-tracker`'s `.song`).
pub mod transcode;

use anyhow::Result;

/// Opaque handle to a loaded audio clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClipHandle(pub u64);

/// Opaque handle to a currently-playing sound instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SoundHandle(pub u64);

/// Settings for playing a sound.
#[derive(Debug, Clone)]
pub struct PlaySettings {
    /// Volume multiplier (0.0 = silent, 1.0 = full).
    pub volume: f32,
    /// Playback speed multiplier (1.0 = normal).
    pub pitch: f32,
    /// Whether the sound loops.
    pub looping: bool,
    /// Stereo position: -1.0 hard left, 0.0 centre, 1.0 hard right. The
    /// convention both backends use natively, so no remapping anywhere.
    pub pan: f32,
    /// The mixer bus it plays through - its track's effects (`BusEffects`)
    /// are applied to it. Empty plays straight to the output.
    pub bus: String,
    /// Give it a low-pass of its own, for [`AudioBackend::set_lowpass`] (a
    /// spatial source with a `muffle`). Natively that is a track of its own,
    /// so only the sounds that need it ask.
    pub filter: bool,
}

impl Default for PlaySettings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            pitch: 1.0,
            looping: false,
            pan: 0.0,
            bus: String::new(),
            filter: false,
        }
    }
}

/// Trait for audio backends.
///
/// Implemented by `KiraAudioBackend` (native) and handled via JS wrappers (WASM).
pub trait AudioBackend {
    /// Load an audio clip from raw bytes. Returns a handle for later playback.
    fn load_clip(&mut self, id: &str, data: &[u8]) -> Result<ClipHandle>;

    /// Look up a clip handle by its string ID. Returns None if not loaded.
    fn get_clip_by_name(&self, name: &str) -> Option<ClipHandle>;

    /// Play a loaded clip with the given settings. Returns a handle to the playing instance.
    fn play(&mut self, clip: ClipHandle, settings: PlaySettings) -> Result<SoundHandle>;

    /// Stop a playing sound.
    fn stop(&mut self, handle: SoundHandle);

    /// Pause a playing sound.
    fn pause(&mut self, handle: SoundHandle);

    /// Resume a paused sound.
    fn resume(&mut self, handle: SoundHandle);

    /// Set volume of a playing sound (0.0–1.0).
    fn set_volume(&mut self, handle: SoundHandle, volume: f32);

    /// Set the global master volume (0.0–1.0).
    fn set_global_volume(&mut self, volume: f32);

    /// Move a playing sound across the stereo field (-1 left, 1 right) -
    /// what positional audio (`audio::spatial`) does every frame. Default
    /// no-op for a backend that cannot.
    fn set_pan(&mut self, _handle: SoundHandle, _pan: f32) {}

    /// A sound's own low-pass cutoff in hertz (0: open) - one played with
    /// `PlaySettings::filter`. The walls between a source and the listener
    /// (`audio::spatial`).
    fn set_lowpass(&mut self, _handle: SoundHandle, _hz: f32) {}

    /// Change a playing sound's playback rate (1 is as recorded) - what
    /// `Audio.set_sound_pitch` and the music's pitch do. Default no-op.
    fn set_pitch(&mut self, _handle: SoundHandle, _pitch: f32) {}

    /// Move a playing sound's playhead to `seconds`. Default no-op.
    fn seek(&mut self, _handle: SoundHandle, _seconds: f64) {}

    /// How long a loaded clip is, in seconds - what a playlist needs to
    /// crossfade before a track ends. `None` when the backend cannot say.
    fn clip_duration(&self, _clip: ClipHandle) -> Option<f64> {
        None
    }

    /// Whether `handle` still refers to a sound the backend can address -
    /// playing or paused, not finished or stopped.
    ///
    /// Required rather than defaulted: the mixer prunes its track list with
    /// it, and a default of `true` would quietly bring back the leak that
    /// pruning exists to fix.
    fn is_playing(&self, handle: SoundHandle) -> bool;

    /// Bring the backend's bus tracks in line with the mixer: create the
    /// ones it lacks under their parents, drop the ones gone (when
    /// `plan.tree`), and set each listed bus's effects. A backend without
    /// effects ignores it, and its sounds play unprocessed.
    fn apply_buses(&mut self, _plan: &BusPlan) {}

    /// Stop every currently playing sound (including music). Default no-op
    /// so legacy backends compile; overridden by backends that track handles.
    fn stop_all(&mut self) {}

    /// Stop every playing instance of one clip, returning how many were
    /// stopped.
    ///
    /// The addressable form of "stop this sound": `play_sound` is a deferred
    /// command, so it has already returned by the time the sound starts and
    /// there is no handle to hand back to the script. Default no-op for
    /// backends that do not retain sources.
    fn stop_by_clip(&mut self, _clip: ClipHandle) -> usize {
        0
    }

    /// Pause every currently playing sound (including music). Default no-op.
    fn pause_all(&mut self) {}

    /// Resume every currently paused sound (including music). Default no-op.
    fn resume_all(&mut self) {}

    /// The app left the foreground (a phone backgrounding it): pause what is
    /// playing and remember it. Not `pause_all`, because the matching
    /// [`Self::unsuspend`] must not resume a sound the game had paused
    /// itself. Default no-op.
    fn suspend(&mut self) {}

    /// Back in the foreground: resume exactly what [`Self::suspend`] paused.
    fn unsuspend(&mut self) {}

    /// Seconds into the sound `handle` is playing (or paused at), or `None`
    /// when the backend does not know it. What `get_music_position` reads.
    fn position(&self, _handle: SoundHandle) -> Option<f64> {
        None
    }

    /// Per-frame update (kira needs this for internal garbage collection).
    fn update(&mut self);
}

/// The music's playhead as the runtime last published it: `f64` bits, NaN
/// for no music.
static MUSIC_POSITION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(f64::NAN.to_bits());

/// Publish where the music is, once a frame. Every runtime calls this after
/// its audio update: native from its backend's handle, the editor viewport
/// and web from the Web Audio clock. `None` when no music is playing.
pub fn publish_music_position(position: Option<f64>) {
    let bits = position.filter(|p| p.is_finite()).unwrap_or(f64::NAN).to_bits();
    MUSIC_POSITION.store(bits, std::sync::atomic::Ordering::Relaxed);
}

/// Seconds into the current music track, as last published, or `None` with
/// no music playing. Scripts read it as `get_music_position()`; a package
/// reads it here, to lock a clock to the music (a rhythm game's song clock does).
pub fn music_position() -> Option<f64> {
    let p = f64::from_bits(MUSIC_POSITION.load(std::sync::atomic::Ordering::Relaxed));
    (!p.is_nan()).then_some(p)
}

/// A no-op audio backend for when audio is not available.
pub struct NullAudioBackend;

impl AudioBackend for NullAudioBackend {
    fn load_clip(&mut self, _id: &str, _data: &[u8]) -> Result<ClipHandle> {
        Ok(ClipHandle(0))
    }
    fn get_clip_by_name(&self, _name: &str) -> Option<ClipHandle> {
        None
    }
    fn play(&mut self, _clip: ClipHandle, _settings: PlaySettings) -> Result<SoundHandle> {
        Ok(SoundHandle(0))
    }
    fn stop(&mut self, _handle: SoundHandle) {}
    fn pause(&mut self, _handle: SoundHandle) {}
    fn resume(&mut self, _handle: SoundHandle) {}
    fn set_volume(&mut self, _handle: SoundHandle, _volume: f32) {}
    fn set_global_volume(&mut self, _volume: f32) {}
    fn is_playing(&self, _handle: SoundHandle) -> bool {
        false
    }
    fn update(&mut self) {}
}

/// Once per frame, after [`AudioBackend::update`]: forget sounds that have
/// finished, then push any bus volume / mute / solo change to the sounds still
/// playing.
///
/// This used to happen only inside `apply_audio_commands`, which runs on frames
/// that issue an audio command - so a script moving a bus slider without also
/// playing a sound changed nothing until something else made a noise, and
/// finished sounds were never forgotten at all.
pub fn sync_mixer(audio: &mut dyn AudioBackend, mixer: &mut AudioMixer) {
    mixer.retain_tracks(|h| audio.is_playing(SoundHandle(h)));
    for (handle, volume) in mixer.take_volume_updates() {
        audio.set_volume(SoundHandle(handle), volume);
    }
    // The bus tracks and their effects (`BusEffects`): created on the first
    // frame, then sent only what changed.
    if let Some(plan) = mixer.take_bus_plan() {
        audio.apply_buses(&plan);
    }
}
