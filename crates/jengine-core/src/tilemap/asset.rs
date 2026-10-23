//! `.tilemap` file format and scene-time tilemap-source resolution.
//!
//! A `.tilemap` file is a JSON document carrying a [`Tilemap`]'s tile size,
//! tilesets, and layers — the same shape the editor's tilemap editor reads
//! and writes. Scenes reference a `.tilemap` via
//! `components.tilemap.source`, letting multiple scenes share the same
//! authored map without duplicating the layer data.
//!
//! On-disk layout matches [`Tilemap`]'s serde shape (minus `source`):
//! ```json
//! {
//!   "tile_size": [48.0, 48.0],
//!   "tilesets": [ { "texture_id": "...", "tile_width": 16, ... } ],
//!   "layers":   [ { "name": "ground", "tiles": [...], ... } ]
//! }
//! ```
//!
//! Resolution happens **before** the scene is fed to the ECS loader:
//! [`resolve_tilemap_sources`] walks the raw scene JSON, looks for every
//! `entities[*].components.tilemap.source` string, loads the referenced
//! file from disk, and **replaces** the entity's `tile_size`, `tilesets`,
//! and `layers` with the file's values - and its `orientation`, when the
//! file names one (the grid's shape belongs to the map). The `.tilemap` file is the
//! authoritative source of truth — re-editing the file propagates to every
//! scene that references it. The `source` field itself is left in place
//! for traceability (and so the editor can re-link).
//!
//! The runtime [`Tilemap`] component itself never touches the filesystem.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::Tilemap;

/// Read a `.tilemap` file from disk into a `serde_json::Value`. The file
/// must parse as a JSON object that round-trips through [`Tilemap`] (so
/// schema errors are surfaced at load time, not later in the renderer).
pub fn read_tilemap_file(path: &Path) -> anyhow::Result<Value> {
    let bytes = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("reading .tilemap file {}: {}", path.display(), e))?;
    let raw: Value = serde_json::from_slice(&bytes)
        .map_err(|e| anyhow::anyhow!("parsing .tilemap file {}: {}", path.display(), e))?;
    if !raw.is_object() {
        anyhow::bail!(
            "expected .tilemap file {} to contain a JSON object, got {}",
            path.display(),
            raw
        );
    }
    let _: Tilemap = serde_json::from_value(raw.clone())
        .map_err(|e| anyhow::anyhow!("validating .tilemap file {}: {}", path.display(), e))?;
    Ok(raw)
}

/// Walk a parsed scene JSON value and replace every
/// `tilemap.source = "path"` reference with that file's `tile_size`,
/// `tilesets`, and `layers` inlined (overwriting any stale copies the
/// editor may have cached in the scene).
///
/// Behaviour per entity:
/// * No `tilemap` or no `source` → untouched.
/// * `source` set → file loaded, `tile_size`/`tilesets`/`layers` replaced
///   on the component. `source` itself is left in place for traceability.
///
/// Errors are collected per entity and returned as a `Vec`; the resolver
/// does not short-circuit so one broken reference doesn't take down the
/// whole scene. Callers should log the errors. The scene JSON is mutated
/// in place.
pub fn resolve_tilemap_sources(
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

        let source = components
            .get("tilemap")
            .and_then(|v| v.get("source"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let Some(src) = source else { continue };

        let Some(resolved_path) = crate::asset::find_in_roots(&src, candidates) else {
            errors.push(anyhow::anyhow!(
                "tilemap source '{src}' not found in any asset root"
            ));
            continue;
        };

        let file_value = match read_tilemap_file(&resolved_path) {
            Ok(v) => v,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };

        let Some(file_obj) = file_value.as_object() else {
            continue;
        };

        if let Some(tm) = components
            .get_mut("tilemap")
            .and_then(|v| v.as_object_mut())
        {
            for key in ["tile_size", "orientation", "tilesets", "layers"] {
                if let Some(val) = file_obj.get(key) {
                    tm.insert(key.to_string(), val.clone());
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
            "jengine-tilemap-asset-test-{}-{}-{}",
            label,
            std::process::id(),
            id
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_tilemap_json() -> &'static str {
        r#"{
            "tile_size": [32.0, 32.0],
            "tilesets": [{
                "texture_id": "sprites/tiles.png",
                "tile_width": 16, "tile_height": 16,
                "columns": 4, "tile_count": 4, "first_gid": 1,
                "image_width": 64, "image_height": 16
            }],
            "layers": [{
                "name": "ground", "width": 2, "height": 2,
                "visible": true, "collision": false,
                "tiles": [1, 2, 3, null]
            }]
        }"#
    }

    #[test]
    fn read_tilemap_file_round_trips() {
        let dir = make_tmp_dir("read");
        let path = dir.join("a.tilemap");
        std::fs::write(&path, sample_tilemap_json()).unwrap();
        let v = read_tilemap_file(&path).unwrap();
        assert_eq!(v["tile_size"][0], 32.0);
        assert_eq!(v["tilesets"][0]["texture_id"], "sprites/tiles.png");
        assert_eq!(v["layers"][0]["name"], "ground");
    }

    #[test]
    fn read_tilemap_file_rejects_invalid_json() {
        let dir = make_tmp_dir("bad");
        let path = dir.join("bad.tilemap");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(read_tilemap_file(&path).is_err());
    }

    #[test]
    fn resolve_replaces_inlined_tile_size_layers_tilesets() {
        let dir = make_tmp_dir("resolve");
        let path = dir.join("pond.tilemap");
        std::fs::write(&path, sample_tilemap_json()).unwrap();

        // Scene has stale inlined data — resolver should overwrite it.
        let mut scene = json!({
            "entities": [{
                "name": "Map",
                "components": {
                    "tilemap": {
                        "source": "pond.tilemap",
                        "tile_size": [9999.0, 9999.0],
                        "tilesets": [],
                        "layers": []
                    }
                }
            }]
        });

        let errs = resolve_tilemap_sources(&mut scene, std::slice::from_ref(&dir));
        assert!(errs.is_empty(), "errors: {errs:?}");

        let tm = &scene["entities"][0]["components"]["tilemap"];
        assert_eq!(tm["source"], "pond.tilemap", "source field preserved");
        assert_eq!(tm["tile_size"][0], 32.0, "tile_size replaced from file");
        assert_eq!(tm["tilesets"][0]["texture_id"], "sprites/tiles.png");
        assert_eq!(tm["layers"][0]["name"], "ground");
        assert_eq!(tm["layers"][0]["tiles"][3], Value::Null);
    }

    #[test]
    fn resolve_skips_entities_without_source() {
        let dir = make_tmp_dir("nosrc");
        let mut scene = json!({
            "entities": [{
                "name": "InlineMap",
                "components": {
                    "tilemap": {
                        "tile_size": [16.0, 16.0],
                        "tilesets": [],
                        "layers": []
                    }
                }
            }]
        });
        let errs = resolve_tilemap_sources(&mut scene, &[dir]);
        assert!(errs.is_empty());
        // Tile size untouched.
        assert_eq!(
            scene["entities"][0]["components"]["tilemap"]["tile_size"][0],
            16.0
        );
    }

    #[test]
    fn resolve_reports_missing_file() {
        let mut scene = json!({
            "entities": [{
                "name": "Map",
                "components": {
                    "tilemap": { "source": "does-not-exist.tilemap" }
                }
            }]
        });
        let errs = resolve_tilemap_sources(&mut scene, &[PathBuf::from(".")]);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].to_string().contains("does-not-exist.tilemap"));
    }
}
