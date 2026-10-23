//! Tracker music for jEngine: a `.song` - chip music written as notes, a
//! few kilobytes of JSON from jSuite's Music Tracker - rendered to PCM when
//! it loads, so a game plays it like any other clip.
//!
//! - [`song`]: the format - parsing and checking a `.song` document.
//! - [`synth`]: the player, a port of jSuite's `synth.ts`: one pass of a
//!   song as stereo samples, and the frame it loops from.
//! - [`digest`]: the fingerprint jSuite's fixtures store, which is how the
//!   two players are held to each other.
//!
//! The package registers `.song` with the engine's clip transcoders
//! (`jengine_core::audio::transcode`), which both audio backends ask before
//! decoding a file. So `Audio.play_music("music/theme.song")`, crossfades,
//! stem layers, playlists, pitch, seek, buses and `music_position` all work
//! unchanged, in every runtime - and a song with an intro loops from its
//! loop point rather than from the top.

pub mod digest;
pub mod song;
pub mod synth;

use jengine_core::audio::transcode::{AudioRegistrar, Transcoded};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::script_api::ScriptApiRegistry;
use jengine_core::package::system_schedule::SystemSchedule;
use jengine_core::package::{JEnginePackage, PackageMetadata};

pub use song::{Song, SongError, parse_song, parse_song_text};
pub use synth::{Rendered, render};

pub const PACKAGE: &str = "jengine-tracker";

/// The rate every song is rendered at - the one jSuite's fixtures pin. Each
/// backend resamples it to its device, as it does a 44.1 kHz OGG.
pub const RATE: u32 = 44100;

/// A `.song` file's bytes as PCM: what the engine's clip loaders call.
/// Warnings (an effect this player ignores) are logged, errors returned.
pub fn transcode_song(bytes: &[u8]) -> Result<Transcoded, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "a .song is JSON text, and this is not UTF-8".to_string())?;
    let (song, issues) = parse_song_text(text).map_err(|e| e.to_string())?;
    for issue in issues {
        log::warn!("[tracker] '{}': {}", song.name, issue.message);
    }
    let r = render(&song, RATE);
    Ok(Transcoded { left: r.left, right: r.right, rate: r.rate, loop_start: r.loop_start })
}

pub struct TrackerPackage;

impl JEnginePackage for TrackerPackage {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: PACKAGE.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Tracker music: .song files from jSuite's Music Tracker, rendered to PCM at load".into(),
            author: String::new(),
            engine_version: Some("0.1.0".into()),
            dependencies: vec![],
        }
    }

    fn register_components(&self, _registry: &mut ComponentRegistry) {}

    fn register_systems(&self, _schedule: &mut SystemSchedule) {}

    fn register_script_api(&self, _api: &mut ScriptApiRegistry) {}

    fn register_audio(&self, audio: &mut AudioRegistrar) {
        audio.clip_transcoder("song", transcode_song);
    }
}

/// The package, for a runtime to add to its `PackageHost`.
pub fn package() -> Box<dyn JEnginePackage> {
    Box::new(TrackerPackage)
}
