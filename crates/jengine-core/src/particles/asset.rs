//! `.particle` file format and scene-time particle-source resolution.
//!
//! A `.particle` file is a small JSON document that carries a single
//! emitter's serialized [`ParticleEmitter`] settings, letting scenes and
//! prefabs reference reusable emitter presets instead of inlining all
//! ~15 fields into every entity that needs them.
//!
//! On-disk layout matches `ParticleEmitter`'s serde shape exactly:
//! ```json
//! {
//!   "texture_id": "sprites/flame.png",
//!   "emission_rate": 80.0,
//!   "lifetime": [0.6, 1.1],
//!   ...
//! }
//! ```
//!
//! Resolution happens **before** the scene is fed to the ECS loader:
//! [`resolve_particle_sources`] walks the raw scene JSON, looks for every
//! `entities[*].components.particle_emitter.particle_source` string, loads
//! the referenced file from disk, and merges the file's fields into the
//! emitter object. Per-field overrides set in the scene win — i.e. a scene
//! that says `{ "particle_source": "fire.particle", "z_order": 10 }`
//! inherits everything from `fire.particle` except `z_order`.
//!
//! The runtime component itself never touches the filesystem.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::ParticleEmitter;

/// Read a `.particle` file from disk into a `serde_json::Value` (kept as a
/// generic map so callers can merge fields). The file must parse as a
/// JSON object whose fields are a strict subset of `ParticleEmitter`'s.
pub fn read_particle_file(path: &Path) -> anyhow::Result<Value> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("reading .particle file {}: {}", path.display(), e))?;
    let raw: Value = serde_json::from_slice(&bytes)
        .map_err(|e| anyhow::anyhow!("parsing .particle file {}: {}", path.display(), e))?;
    if !raw.is_object() {
        anyhow::bail!(
            "expected .particle file {} to contain a JSON object, got {}",
            path.display(),
            raw
        );
    }
    // Round-trip through the strong type so we surface schema errors here
    // rather than silently swallowing unknown fields downstream.
    let _: ParticleEmitter = serde_json::from_value(raw.clone())
        .map_err(|e| anyhow::anyhow!("validating .particle file {}: {}", path.display(), e))?;
    Ok(raw)
}

/// Walk a parsed scene JSON value and replace every
/// `particle_emitter.particle_source = "path"` reference with the file's
/// fields inlined.
///
/// Behaviour per entity:
/// * No `particle_emitter` or no `particle_source` → untouched.
/// * `particle_source` set → file loaded, every field present in the file
///   is inserted into the emitter object **only when the entity hasn't
///   already set it** (scene overrides win). The `particle_source` field
///   itself is left in place for traceability.
///
/// Errors are collected per entity and returned as a `Vec`; the resolver
/// does not short-circuit so one broken reference doesn't take down the
/// whole scene. Callers should log the errors. The scene JSON is mutated
/// in place.
pub fn resolve_particle_sources(
    scene_json: &mut Value,
    candidates: &[PathBuf],
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

        let particle_source = components
            .get("particle_emitter")
            .and_then(|v| v.get("particle_source"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let Some(src) = particle_source else { continue };

        let Some(resolved_path) = crate::asset::find_in_roots(&src, candidates) else {
            errors.push(anyhow::anyhow!(
                "particle_source '{src}' not found in any asset root"
            ));
            continue;
        };

        let file_value = match read_particle_file(&resolved_path) {
            Ok(v) => v,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };

        let Some(file_obj) = file_value.as_object() else {
            continue;
        };

        if let Some(emitter) = components
            .get_mut("particle_emitter")
            .and_then(|v| v.as_object_mut())
        {
            for (key, val) in file_obj {
                // Don't overwrite particle_source itself; scene overrides
                // for any other field win.
                if key == "particle_source" {
                    continue;
                }
                if !emitter.contains_key(key) {
                    emitter.insert(key.clone(), val.clone());
                }
            }
        }
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
            "jengine-particle-asset-test-{}-{}-{}",
            label,
            std::process::id(),
            id
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn read_particle_file_round_trips() {
        let dir = make_tmp_dir("read");
        let path = dir.join("fire.particle");
        std::fs::write(
            &path,
            r#"{
                "texture_id": "sprites/flame.png",
                "emission_rate": 80.0,
                "max_particles": 400,
                "lifetime": [0.6, 1.1],
                "speed": [70.0, 140.0],
                "angle": [1.35, 1.79],
                "color_start": [1.0, 0.85, 0.25, 1.0],
                "color_end":   [0.8, 0.1,  0.0,  0.0],
                "size_start":  [18.0, 26.0],
                "size_end":    [4.0,  8.0],
                "gravity_modifier": -0.25,
                "active": true,
                "world_space": true,
                "z_order": 5
            }"#,
        )
        .unwrap();

        let value = read_particle_file(&path).unwrap();
        assert_eq!(value["emission_rate"], 80.0);
        assert_eq!(value["texture_id"], "sprites/flame.png");
    }

    #[test]
    fn resolve_inlines_fields_into_emitter() {
        let dir = make_tmp_dir("resolve");
        let pp = dir.join("blue.particle");
        std::fs::write(
            &pp,
            r#"{
                "texture_id": "sprites/spark.png",
                "emission_rate": 60.0,
                "lifetime": [1.0, 1.6],
                "color_start": [0.0, 0.5, 1.0, 1.0],
                "z_order": 3
            }"#,
        )
        .unwrap();

        let mut scene = json!({
            "entities": [{
                "name": "FX",
                "components": {
                    "particle_emitter": { "particle_source": "blue.particle" }
                }
            }]
        });

        let errors = resolve_particle_sources(&mut scene, std::slice::from_ref(&dir));
        assert!(errors.is_empty(), "{errors:?}");

        let emitter = &scene["entities"][0]["components"]["particle_emitter"];
        assert_eq!(emitter["texture_id"], "sprites/spark.png");
        assert_eq!(emitter["emission_rate"], 60.0);
        assert_eq!(emitter["z_order"], 3);
        // particle_source preserved for traceability
        assert_eq!(emitter["particle_source"], "blue.particle");
    }

    #[test]
    fn scene_field_overrides_win_over_asset() {
        let dir = make_tmp_dir("override");
        let pp = dir.join("base.particle");
        std::fs::write(
            &pp,
            r#"{
                "emission_rate": 20.0,
                "z_order": 0
            }"#,
        )
        .unwrap();

        let mut scene = json!({
            "entities": [{
                "name": "FX",
                "components": {
                    "particle_emitter": {
                        "particle_source": "base.particle",
                        "z_order": 99
                    }
                }
            }]
        });

        let errors = resolve_particle_sources(&mut scene, std::slice::from_ref(&dir));
        assert!(errors.is_empty(), "{errors:?}");

        let emitter = &scene["entities"][0]["components"]["particle_emitter"];
        // Asset filled in:
        assert_eq!(emitter["emission_rate"], 20.0);
        // Scene override preserved:
        assert_eq!(emitter["z_order"], 99);
    }

    #[test]
    fn missing_particle_source_returns_error_but_keeps_going() {
        let mut scene = json!({
            "entities": [{
                "name": "FX",
                "components": { "particle_emitter": { "particle_source": "nope.particle" } }
            }]
        });
        let errors = resolve_particle_sources(&mut scene, &[PathBuf::from(".")]);
        assert_eq!(errors.len(), 1);
    }
}
