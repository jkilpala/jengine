//! Scene-time `.atlas` reference resolution.
//!
//! Scenes opt into runtime atlas loading by adding a top-level
//! `atlas_refs` array next to `entities`:
//!
//! ```json
//! {
//!   "name": "demo",
//!   "atlas_refs": ["sprites/_atlases/demo.atlas"],
//!   "entities": [...]
//! }
//! ```
//!
//! Each referenced `.atlas` is parsed as an [`AtlasData`], its sibling PNG
//! is pre-loaded into the [`AssetManager`] raw-bytes cache, and the
//! `AtlasData` itself is registered under the sibling PNG path (which
//! doubles as the runtime atlas id). Per-region atlas mappings are also
//! registered, so the script bridge can call
//! `set_sprite_region(entity, "sprites/_atlases/demo.png", "tile_red")`
//! and have the engine swap the entity's `SpriteComponent.texture_id` /
//! `uv_rect` to the region.
//!
//! Mirrors the `.tilemap` / `.anim` / `.particle` resolvers in shape:
//! errors are collected, never short-circuit, and the resolver is a
//! no-op on scenes that don't reference atlases.

use std::path::PathBuf;

use serde_json::Value;

use crate::asset::AssetManager;
use crate::renderer::atlas::AtlasData;

/// Resolve every `atlas_refs[*]` entry in `scene_json` against `candidates`.
///
/// For each referenced `.atlas` file:
/// 1. Parse it into [`AtlasData`].
/// 2. Determine the sibling PNG path (same stem, `.png` extension).
/// 3. Load the PNG bytes via [`AssetManager::load_texture_bytes`] under
///    the sibling path as the id (so the wgpu sprite pipeline can find
///    it on first render).
/// 4. Insert the [`AtlasData`] under the same id via
///    [`AssetManager::insert_atlas_data`].
/// 5. Register `register_atlas_mapping(<png>, <png>, region_name)` for
///    every region so the existing `atlas_lookup_entry` / `get_sprite_region`
///    paths work for scripts that want to query points.
///
/// Returns the list of errors encountered (one per failed reference); the
/// resolver does **not** short-circuit so one missing atlas doesn't take
/// down an otherwise-valid scene.
pub fn resolve_atlas_refs(
    scene_json: &Value,
    candidates: &[PathBuf],
    assets: &mut AssetManager,
) -> Vec<anyhow::Error> {
    let mut errors = Vec::new();

    for rel in crate::renderer::atlas::scene_atlas_refs(scene_json) {
        let rel = rel.as_str();

        let Some(resolved_path) = crate::asset::find_in_roots(rel, candidates) else {
            errors.push(anyhow::anyhow!(
                "atlas ref '{rel}' not found in any asset root"
            ));
            continue;
        };

        let bytes = match std::fs::read(&resolved_path) {
            Ok(b) => b,
            Err(e) => {
                errors.push(anyhow::anyhow!(
                    "reading .atlas file {}: {e}",
                    resolved_path.display()
                ));
                continue;
            }
        };
        let atlas: AtlasData = match serde_json::from_slice(&bytes) {
            Ok(a) => a,
            Err(e) => {
                errors.push(anyhow::anyhow!(
                    "parsing .atlas file {}: {e}",
                    resolved_path.display()
                ));
                continue;
            }
        };

        // Sibling PNG path: same parent, same stem, .png extension.
        let png_path = resolved_path.with_extension("png");
        if !png_path.is_file() {
            errors.push(anyhow::anyhow!(
                "atlas ref '{rel}' is missing sibling image at {}",
                png_path.display()
            ));
            continue;
        }

        // The runtime atlas id = the sibling PNG path expressed as a
        // forward-slash relative string (so it matches what scenes write
        // in `SpriteComponent.texture_id` and what wgpu_scene_runner's
        // `load_world_textures` keys on).
        let atlas_id = derive_atlas_id(rel);

        if let Err(e) = assets.load_texture_bytes(&atlas_id, &png_path.to_string_lossy()) {
            errors.push(anyhow::anyhow!(
                "loading atlas image {}: {e}",
                png_path.display()
            ));
            continue;
        }

        assets.register_atlas(atlas_id, atlas);
    }

    errors
}

use crate::renderer::atlas::derive_atlas_id;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn make_tmp_dir(label: &str) -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "jengine-atlas-asset-test-{}-{}-{}",
            label,
            std::process::id(),
            id
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Build a 2x2 RGBA PNG so `load_texture_bytes` succeeds.
    fn write_dummy_png(path: &Path) {
        let rgba: [u8; 16] = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let img = image::RgbaImage::from_raw(2, 2, rgba.to_vec()).unwrap();
        img.save(path).unwrap();
    }

    #[test]
    fn resolves_atlas_ref_and_inserts_data_and_mapping() {
        let dir = make_tmp_dir("ok");
        let atlas_path = dir.join("demo.atlas");
        let png_path = dir.join("demo.png");
        write_dummy_png(&png_path);
        let atlas_json = json!({
            "width": 2,
            "height": 2,
            "regions": [
                { "name": "r1", "x": 0, "y": 0, "width": 1, "height": 1 },
                { "name": "r2", "x": 1, "y": 1, "width": 1, "height": 1 }
            ]
        });
        std::fs::write(&atlas_path, atlas_json.to_string()).unwrap();

        let scene = json!({ "atlas_refs": ["demo.atlas"], "entities": [] });
        let mut assets = AssetManager::new();
        let errs = resolve_atlas_refs(&scene, std::slice::from_ref(&dir), &mut assets);
        assert!(errs.is_empty(), "errors: {errs:?}");

        assert!(assets.has_atlas_data("demo.png"));
        assert!(assets.has_texture_bytes("demo.png"));
        let mapped = assets.atlas_lookup_entry("r1").unwrap();
        assert_eq!(mapped, ("demo.png".to_string(), "r1".to_string()));

        let atlas = assets.get_atlas_data("demo.png").unwrap();
        let uv = atlas.get_uv_rect("r2").unwrap();
        assert_eq!(uv, [0.5, 0.5, 1.0, 1.0]);
    }

    #[test]
    fn missing_atlas_file_yields_error_without_aborting() {
        let dir = make_tmp_dir("missing");
        let scene = json!({ "atlas_refs": ["nope.atlas"], "entities": [] });
        let mut assets = AssetManager::new();
        let errs = resolve_atlas_refs(&scene, std::slice::from_ref(&dir), &mut assets);
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn no_atlas_refs_is_noop() {
        let scene = json!({ "entities": [] });
        let mut assets = AssetManager::new();
        let errs = resolve_atlas_refs(&scene, &[], &mut assets);
        assert!(errs.is_empty());
    }
}
