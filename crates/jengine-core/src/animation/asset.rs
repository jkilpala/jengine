//! `.anim` file format and scene-time clip-source resolution.
//!
//! A `.anim` file is a small JSON document that ships one or more
//! [`AnimationClip`]s plus an optional shared `texture_id`. It lets scenes and
//! prefabs reference reusable animation data instead of inlining frame arrays
//! into every entity that needs them.
//!
//! Two on-disk layouts are accepted, both with the `.anim` extension:
//!
//! * **Multi-clip** (preferred, matches the editor's `MultiClipAnimData`):
//!   ```json
//!   { "texture_id": "sprites/foo.png", "clips": { "walk": { ... }, "idle": { ... } } }
//!   ```
//! * **Single-clip** (legacy single-clip files produced before the multi-clip
//!   format existed):
//!   ```json
//!   { "name": "walk", "frames": [...], "frame_duration": 0.1, "looping": true }
//!   ```
//!
//! A frame can name an atlas region instead of carrying coordinates:
//! `{ "region": "grunt_walk_2" }`. The region is looked up in the atlas
//! registered under the frame's `texture_id`, or the sprite's texture in
//! spritesheet mode - so the atlas has to be in the scene's `atlas_refs`.
//! See [`super::SpriteAnimator::resolve_frame_regions`].
//!
//! Resolution happens **before** the scene is fed to the ECS loader:
//! [`resolve_anim_sources`] walks the raw scene JSON, looks for every
//! `entities[*].components.sprite_animator.clip_source` string, loads the
//! referenced file from disk, and rewrites the JSON so the resulting
//! `SpriteAnimator` deserialises with the file's clips inlined. The runtime
//! component itself never touches the filesystem.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::AnimationClip;

/// On-disk shape of a `.anim` file (multi-clip variant).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimAssetFile {
    /// Optional shared texture for all clips. When present and the entity's
    /// sprite component has no `texture_id`, the resolver assigns this value
    /// so authors only have to declare the texture in one place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture_id: Option<String>,
    /// Named clips keyed by clip name.
    pub clips: HashMap<String, AnimationClip>,
}

/// Read a `.anim` file from disk and normalise it to multi-clip form.
///
/// Accepts both the multi-clip and single-clip JSON shapes described in the
/// module docs.
pub fn read_anim_file(path: &Path) -> anyhow::Result<AnimAssetFile> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("reading .anim file {}: {}", path.display(), e))?;
    parse_anim_asset(&bytes, &path.display().to_string())
}

/// Parse `.anim` bytes and normalise to multi-clip form.
///
/// Split out of [`read_anim_file`] so hosts without a filesystem (the web
/// runtime fetches the file over HTTP) normalise byte-identically to the
/// native loader instead of reimplementing the two accepted layouts.
/// `label` only names the source in error messages.
pub fn parse_anim_asset(bytes: &[u8], label: &str) -> anyhow::Result<AnimAssetFile> {
    let raw: Value = serde_json::from_slice(bytes)
        .map_err(|e| anyhow::anyhow!("parsing .anim file {label}: {e}"))?;

    // Multi-clip has a "clips" object and no top-level "name".
    let is_multi = raw
        .get("clips")
        .is_some_and(|v| v.is_object())
        && !raw.get("name").is_some_and(|v| v.is_string());

    if is_multi {
        let parsed: AnimAssetFile = serde_json::from_value(raw)
            .map_err(|e| anyhow::anyhow!("parsing .anim file {label}: {e}"))?;
        Ok(parsed)
    } else {
        // Legacy single-clip: wrap into multi-clip with the clip's name as the key.
        let clip: AnimationClip = serde_json::from_value(raw)
            .map_err(|e| anyhow::anyhow!("parsing .anim file {label}: {e}"))?;
        let mut clips = HashMap::new();
        clips.insert(clip.name.clone(), clip);
        Ok(AnimAssetFile { texture_id: None, clips })
    }
}

/// Every distinct `sprite_animator.clip_source` in a scene JSON.
///
/// Lets a host gather the set of `.anim` files a scene needs before it has
/// any of them — the web runtime fetches these, then hands the results to
/// [`inline_anim_sources`].
pub fn collect_clip_sources(scene_json: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(entities) = scene_json.get("entities").and_then(|v| v.as_array()) else {
        return out;
    };
    for ent in entities {
        let Some(src) = ent
            .get("components")
            .and_then(|c| c.get("sprite_animator"))
            .and_then(|a| a.get("clip_source"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        if !src.is_empty() && !out.iter().any(|s| s == src) {
            out.push(src.to_string());
        }
    }
    out
}

/// Rewrite a scene JSON so every `clip_source` entity carries the referenced
/// clips inline, given assets already loaded by the caller (keyed by the
/// `clip_source` string).
///
/// This is the half of [`resolve_anim_sources`] that does not touch the
/// filesystem, so the fetch-based hosts inline by exactly the same rules.
/// A source missing from `loaded` is reported and left alone.
pub fn inline_anim_sources(
    scene_json: &mut Value,
    loaded: &HashMap<String, AnimAssetFile>,
) -> Vec<anyhow::Error> {
    let mut errors = Vec::new();
    let Some(entities) = scene_json
        .get_mut("entities")
        .and_then(|v| v.as_array_mut())
    else {
        return errors;
    };

    for ent in entities.iter_mut() {
        let Some(components) = ent.get_mut("components").and_then(|v| v.as_object_mut()) else {
            continue;
        };
        let clip_source = components
            .get("sprite_animator")
            .and_then(|v| v.get("clip_source"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let Some(src) = clip_source else { continue };
        let Some(asset) = loaded.get(&src) else {
            errors.push(anyhow::anyhow!("clip_source '{src}' was not loaded"));
            continue;
        };

        let clips_value = match serde_json::to_value(&asset.clips) {
            Ok(v) => v,
            Err(e) => {
                errors.push(anyhow::anyhow!("serialising clips from {src}: {e}"));
                continue;
            }
        };
        if let Some(animator) = components
            .get_mut("sprite_animator")
            .and_then(|v| v.as_object_mut())
        {
            animator.insert("clips".to_string(), clips_value);
        }

        // Backfill the sprite's texture_id when the .anim provides one and
        // the entity hasn't set its own.
        if let Some(tex) = asset.texture_id.clone()
            && let Some(sprite) = components.get_mut("sprite").and_then(|v| v.as_object_mut())
        {
            let needs_fill = sprite
                .get("texture_id")
                .and_then(|v| v.as_str())
                .is_none_or(|s| s.is_empty());
            if needs_fill {
                sprite.insert("texture_id".to_string(), Value::String(tex));
            }
        }
    }

    errors
}

/// Walk a parsed scene JSON value and replace every
/// `sprite_animator.clip_source = "path"` with that file's clips inlined.
///
/// Behaviour per entity:
/// * No `sprite_animator` or no `clip_source` → untouched.
/// * `clip_source` set → loaded, `clips` object replaced, `clip_source` left
///   in place for traceability.
/// * If the `.anim` declares a `texture_id` and the sibling `sprite.texture_id`
///   is missing or empty, the sprite's texture is filled in.
///
/// Errors are collected per entity and returned as a `Vec`; the resolver does
/// not short-circuit so one broken reference doesn't take down the whole
/// scene. Callers should log the errors. The scene JSON is mutated in place.
pub fn resolve_anim_sources(
    scene_json: &mut Value,
    candidates: &[PathBuf],
) -> Vec<anyhow::Error> {
    let mut errors = Vec::new();
    let mut loaded: HashMap<String, AnimAssetFile> = HashMap::new();

    for src in collect_clip_sources(scene_json) {
        let Some(resolved_path) = crate::asset::find_in_roots(&src, candidates) else {
            errors.push(anyhow::anyhow!(
                "clip_source '{src}' not found in any asset root"
            ));
            continue;
        };
        match read_anim_file(&resolved_path) {
            Ok(asset) => {
                loaded.insert(src, asset);
            }
            Err(e) => errors.push(e),
        }
    }

    // Sources that failed to load are already reported above; the shared
    // inliner would report them a second time, so drop those entries first.
    errors.extend(
        inline_anim_sources(scene_json, &loaded)
            .into_iter()
            .filter(|e| !e.to_string().ends_with("was not loaded")),
    );
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
            "jengine-anim-asset-test-{}-{}-{}",
            label,
            std::process::id(),
            id
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn read_multi_clip_file() {
        let dir = make_tmp_dir("multi");
        let path = dir.join("test.anim");
        std::fs::write(
            &path,
            r#"{
                "texture_id": "sprites/x.png",
                "clips": {
                    "walk": {
                        "name": "walk",
                        "frames": [{ "uv_rect": [0.0, 0.0, 1.0, 0.25] }],
                        "frame_duration": 0.1,
                        "looping": true
                    }
                }
            }"#,
        )
        .unwrap();

        let asset = read_anim_file(&path).unwrap();
        assert_eq!(asset.texture_id.as_deref(), Some("sprites/x.png"));
        assert_eq!(asset.clips.len(), 1);
        assert!(asset.clips.contains_key("walk"));
    }

    #[test]
    fn read_single_clip_legacy_file() {
        let dir = make_tmp_dir("legacy");
        let path = dir.join("legacy.anim");
        std::fs::write(
            &path,
            r#"{
                "name": "idle",
                "frames": [{ "uv_rect": [0.0, 0.0, 1.0, 1.0] }],
                "frame_duration": 0.2,
                "looping": false
            }"#,
        )
        .unwrap();

        let asset = read_anim_file(&path).unwrap();
        assert!(asset.texture_id.is_none());
        assert_eq!(asset.clips.len(), 1);
        assert!(asset.clips.contains_key("idle"));
    }

    #[test]
    fn resolve_inlines_clips_and_backfills_texture() {
        let dir = make_tmp_dir("resolve");
        let anim_path = dir.join("walk.anim");
        std::fs::write(
            &anim_path,
            r#"{
                "texture_id": "sprites/hero.png",
                "clips": {
                    "walk": {
                        "name": "walk",
                        "frames": [{ "uv_rect": [0.0, 0.0, 0.5, 1.0] }],
                        "frame_duration": 0.15,
                        "looping": true
                    }
                }
            }"#,
        )
        .unwrap();

        let mut scene = json!({
            "name": "test",
            "entities": [{
                "name": "Hero",
                "components": {
                    "sprite": { "texture_id": "", "color": [1,1,1,1] },
                    "sprite_animator": { "clip_source": "walk.anim", "clips": {} }
                }
            }]
        });

        let errors = resolve_anim_sources(&mut scene, std::slice::from_ref(&dir));
        assert!(errors.is_empty(), "{errors:?}");

        let animator = &scene["entities"][0]["components"]["sprite_animator"];
        assert!(animator["clips"]["walk"].is_object());
        assert_eq!(
            scene["entities"][0]["components"]["sprite"]["texture_id"],
            "sprites/hero.png"
        );
    }

    #[test]
    fn missing_clip_source_returns_error_but_keeps_going() {
        let mut scene = json!({
            "entities": [{
                "name": "A",
                "components": { "sprite_animator": { "clip_source": "nope.anim", "clips": {} } }
            }]
        });
        let errors = resolve_anim_sources(&mut scene, &[PathBuf::from(".")]);
        assert_eq!(errors.len(), 1);
    }
}
