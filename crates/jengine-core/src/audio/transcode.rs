//! Clips that are not audio files: a format a package renders to PCM when
//! the clip is loaded - `jengine-tracker`'s `.song`, chip music written as
//! notes, is the one there is.
//!
//! Every runtime funnels a clip through one call - natively
//! `KiraAudioBackend::load_clip`, in the browsers
//! `WebAudioBackend::load_clip` - and both ask [`transcode`] first. So once
//! a package has registered its extension (`JEnginePackage::register_audio`),
//! a `.song` is just a clip: `Audio.play_music`, crossfades, layers,
//! playlists, pitch, seek, buses and `music_position` all work on it
//! unchanged. A transcoded clip can say where its loop starts, which each
//! backend keeps per clip and loops from.
//!
//! The registry is the process's, like the music director: the backends do
//! not know the package host, and a package registers before any scene
//! loads.
//!
//! Rendering costs CPU at load in exchange for the bytes it saves. Natively
//! a clip is loaded on its first play, so `audio_refs` preloading calls
//! [`prepare`] to render it then, and the first play takes the result.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// A clip rendered to stereo PCM.
#[derive(Debug, Clone, Default)]
pub struct Transcoded {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub rate: u32,
    /// The frame a looping play goes back to, or `None` to loop the whole clip.
    pub loop_start: Option<u64>,
}

impl Transcoded {
    pub fn frames(&self) -> usize {
        self.left.len().min(self.right.len())
    }

    pub fn duration(&self) -> f64 {
        self.frames() as f64 / f64::from(self.rate.max(1))
    }

    /// Where the loop starts in seconds, when it is not the top of the clip.
    pub fn loop_start_seconds(&self) -> Option<f64> {
        self.loop_start.filter(|&f| f > 0 && (f as usize) < self.frames()).map(|f| f as f64 / f64::from(self.rate.max(1)))
    }
}

/// Renders a clip's file bytes to PCM.
pub type Transcoder = fn(&[u8]) -> Result<Transcoded, String>;

/// Extensions a package of the engine's own plays, so a project that uses
/// one without listing the package is told which to add rather than shown
/// a decode error.
const KNOWN: &[(&str, &str)] = &[("song", "jengine-tracker")];

static TRANSCODERS: RwLock<Vec<(String, Transcoder)>> = RwLock::new(Vec::new());

/// What [`prepare`] rendered, by clip id, with a fingerprint of the bytes.
static PREPARED: Mutex<Option<HashMap<String, (u64, Arc<Transcoded>)>>> = Mutex::new(None);

/// What a package is handed in `JEnginePackage::register_audio`.
pub struct AudioRegistrar {
    _private: (),
}

impl AudioRegistrar {
    pub(crate) fn new() -> Self {
        Self { _private: () }
    }

    /// Play clips whose id ends in `.<extension>` through `transcoder`.
    pub fn clip_transcoder(&mut self, extension: &str, transcoder: Transcoder) {
        register_clip_transcoder(extension, transcoder);
    }
}

/// Register a transcoder for an extension (without the dot, any case). A
/// second registration of one extension replaces the first.
pub fn register_clip_transcoder(extension: &str, transcoder: Transcoder) {
    let ext = extension.trim_start_matches('.').to_ascii_lowercase();
    let mut list = TRANSCODERS.write().unwrap_or_else(|e| e.into_inner());
    list.retain(|(e, _)| *e != ext);
    list.push((ext, transcoder));
}

fn extension_of(id: &str) -> Option<String> {
    let name = id.rsplit(['/', '\\']).next().unwrap_or(id);
    name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase())
}

/// Whether `id` is a clip some transcoder - registered or known - is for.
pub fn is_transcoded(id: &str) -> bool {
    let Some(ext) = extension_of(id) else { return false };
    KNOWN.iter().any(|(e, _)| *e == ext) || TRANSCODERS.read().unwrap_or_else(|e| e.into_inner()).iter().any(|(e, _)| *e == ext)
}

/// The package that plays `id`'s extension, for one of the engine's own.
pub fn package_for(id: &str) -> Option<&'static str> {
    let ext = extension_of(id)?;
    KNOWN.iter().find(|(e, _)| *e == ext).map(|(_, p)| *p)
}

fn fingerprint(bytes: &[u8]) -> u64 {
    // FNV-1a: a `.song` is a few kilobytes, and this only tells an edited
    // file from the one that was prepared.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h ^ bytes.len() as u64
}

fn render(id: &str, bytes: &[u8]) -> Option<Result<Transcoded, String>> {
    let ext = extension_of(id)?;
    let found = TRANSCODERS.read().unwrap_or_else(|e| e.into_inner()).iter().find(|(e, _)| *e == ext).map(|(_, t)| *t);
    match found {
        Some(t) => Some(t(bytes).map_err(|e| format!("'{id}': {e}"))),
        None => package_for(id)
            .map(|p| Err(format!("no player for .{ext} - add {p} to the project's packages to play '{id}'"))),
    }
}

/// The clip as PCM, if `id` is a transcoded format: `None` means "decode
/// it as an audio file". Takes what [`prepare`] rendered for these bytes.
pub fn transcode(id: &str, bytes: &[u8]) -> Option<Result<Arc<Transcoded>, String>> {
    if !is_transcoded(id) {
        return None;
    }
    let key = fingerprint(bytes);
    let mut prepared = PREPARED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(map) = prepared.as_mut()
        && let Some((fp, t)) = map.remove(id)
        && fp == key
    {
        return Some(Ok(t));
    }
    drop(prepared);
    let started = crate::profiling::now_ms();
    let out = render(id, bytes)?.map(Arc::new);
    if let Ok(t) = &out {
        log::info!("rendered '{id}': {:.1} s of audio in {:.0} ms", t.duration(), crate::profiling::now_ms() - started);
    }
    Some(out)
}

/// Render `id` now so its first play does not: what `audio_refs`
/// preloading calls natively. Does nothing for an ordinary audio file; an
/// error is returned for the caller to log, and the first play reports it
/// again.
pub fn prepare(id: &str, bytes: &[u8]) -> Result<(), String> {
    if !is_transcoded(id) {
        return Ok(());
    }
    let key = fingerprint(bytes);
    if let Some(map) = PREPARED.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
        && map.get(id).is_some_and(|(fp, _)| *fp == key)
    {
        return Ok(());
    }
    let t = transcode(id, bytes).unwrap_or_else(|| Err("not transcoded".into()))?;
    PREPARED.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(id.to_string(), (key, t));
    Ok(())
}

/// Where a playhead `pos` seconds into a clip lands, looping from
/// `loop_start` back from `duration` - the arithmetic a backend that keeps
/// its own playhead (Web Audio) needs once a loop does not start at 0.
pub fn loop_position(pos: f64, loop_start: f64, duration: f64) -> f64 {
    if duration <= 0.0 || pos < duration {
        return pos.max(0.0);
    }
    let start = loop_start.clamp(0.0, duration);
    let span = duration - start;
    if span <= 0.0 {
        return duration;
    }
    start + (pos - start) % span
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(_: &[u8]) -> Result<Transcoded, String> {
        Ok(Transcoded { left: vec![0.5; 100], right: vec![-0.5; 100], rate: 100, loop_start: Some(25) })
    }

    #[test]
    fn a_registered_extension_is_rendered_and_others_are_left_alone() {
        register_clip_transcoder(".TestTone", tone);
        assert!(transcode("sounds/hit.wav", b"RIFF").is_none());
        let t = transcode("music/a.testtone", b"x").unwrap().unwrap();
        assert_eq!(t.frames(), 100);
        assert_eq!(t.loop_start_seconds(), Some(0.25));
    }

    #[test]
    fn a_known_extension_with_no_package_names_the_package() {
        // `.song` may be registered by another test binary's package, never
        // this one's.
        if TRANSCODERS.read().unwrap().iter().any(|(e, _)| e == "song") {
            return;
        }
        let e = transcode("music/theme.song", b"{}").unwrap().unwrap_err();
        assert!(e.contains("jengine-tracker"), "{e}");
    }

    #[test]
    fn a_prepared_clip_is_taken_once_and_only_for_its_own_bytes() {
        register_clip_transcoder("prepped", tone);
        prepare("a.prepped", b"one").unwrap();
        assert!(PREPARED.lock().unwrap().as_ref().unwrap().contains_key("a.prepped"));
        // Other bytes under the same id render afresh and drop the stale one.
        transcode("a.prepped", b"two").unwrap().unwrap();
        assert!(!PREPARED.lock().unwrap().as_ref().unwrap().contains_key("a.prepped"));
    }

    #[test]
    fn a_playhead_wraps_into_the_loop() {
        assert_eq!(loop_position(3.0, 2.0, 10.0), 3.0);
        assert_eq!(loop_position(12.0, 2.0, 10.0), 4.0);
        assert_eq!(loop_position(19.0, 2.0, 10.0), 3.0);
        assert_eq!(loop_position(12.0, 0.0, 10.0), 2.0);
    }
}
