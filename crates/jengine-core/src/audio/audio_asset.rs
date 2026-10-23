//! Scene-time `audio_refs` resolution.
//!
//! Scenes opt into runtime audio loading by adding a top-level
//! `audio_refs` array next to `entities`:
//!
//! ```json
//! {
//!   "name": "demo",
//!   "audio_refs": ["sounds/blip.wav", "sounds/hum.wav"],
//!   "entities": [...]
//! }
//! ```
//!
//! Each referenced clip is read from disk (against the same candidate
//! roots used for textures / atlases) and its raw bytes are inserted
//! into the [`AssetManager`] audio cache under the normalized
//! forward-slash path as the clip id. The backend decodes the bytes
//! on the first `play_sound("path/to/clip.wav", ...)` call via
//! `apply_audio_commands`.
//!
//! Mirrors the `.atlas` resolver in shape: errors are collected, never
//! short-circuit, and the resolver is a no-op on scenes without an
//! `audio_refs` array.

use std::path::PathBuf;

use serde_json::Value;

use crate::asset::AssetManager;

/// Resolve every `audio_refs[*]` entry in `scene_json` against `candidates`.
///
/// Returns the list of errors encountered (one per failed reference); the
/// resolver does **not** short-circuit so one missing clip doesn't take
/// down an otherwise-valid scene.
pub fn resolve_audio_refs(
    scene_json: &Value,
    candidates: &[PathBuf],
    assets: &mut AssetManager,
) -> Vec<anyhow::Error> {
    let mut errors = Vec::new();

    let Some(refs) = scene_json.get("audio_refs").and_then(|v| v.as_array()) else {
        return errors;
    };

    for entry in refs {
        let Some(rel) = entry.as_str() else {
            errors.push(anyhow::anyhow!(
                "audio_refs entry is not a string: {entry}"
            ));
            continue;
        };

        let Some(resolved_path) = crate::asset::find_in_roots(rel, candidates) else {
            errors.push(anyhow::anyhow!(
                "audio ref '{rel}' not found in any asset root"
            ));
            continue;
        };

        let bytes = match std::fs::read(&resolved_path) {
            Ok(b) => b,
            Err(e) => {
                errors.push(anyhow::anyhow!(
                    "reading audio file {}: {e}",
                    resolved_path.display()
                ));
                continue;
            }
        };

        let clip_id = rel.replace('\\', "/");
        // A clip a package renders (a `.song`) is rendered now, while the
        // scene loads: natively a clip is loaded on its first play, and a
        // song rendered then would stall the frame its music starts on.
        if let Err(e) = super::transcode::prepare(&clip_id, &bytes) {
            errors.push(anyhow::anyhow!("audio ref {e}"));
        }
        assets.insert_audio(clip_id, bytes);
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn make_tmp_dir(label: &str) -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "jengine-audio-asset-test-{}-{}-{}",
            label,
            std::process::id(),
            id
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolves_audio_ref_and_inserts_bytes() {
        let dir = make_tmp_dir("ok");
        let clip_rel = "sounds/blip.wav";
        let clip_path = dir.join(clip_rel);
        std::fs::create_dir_all(clip_path.parent().unwrap()).unwrap();
        std::fs::write(&clip_path, b"RIFFxxxxWAVE").unwrap();

        let scene = json!({ "audio_refs": [clip_rel], "entities": [] });
        let mut assets = AssetManager::new();
        let errs = resolve_audio_refs(&scene, std::slice::from_ref(&dir), &mut assets);
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
        let cached = assets.get_audio_data(clip_rel).expect("audio bytes missing");
        assert_eq!(cached, b"RIFFxxxxWAVE");
    }

    #[test]
    fn missing_clip_yields_error_but_no_panic() {
        let dir = make_tmp_dir("missing");
        let scene = json!({ "audio_refs": ["sounds/nope.wav"], "entities": [] });
        let mut assets = AssetManager::new();
        let errs = resolve_audio_refs(&scene, std::slice::from_ref(&dir), &mut assets);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].to_string().contains("not found"));
    }

    #[test]
    fn no_audio_refs_is_noop() {
        let dir = make_tmp_dir("none");
        let scene = json!({ "entities": [] });
        let mut assets = AssetManager::new();
        let errs = resolve_audio_refs(&scene, std::slice::from_ref(&dir), &mut assets);
        assert!(errs.is_empty());
    }
}
