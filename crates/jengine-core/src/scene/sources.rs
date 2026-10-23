//! Inlining the asset files a scene points at.
//!
//! A scene can name three kinds of file instead of carrying their contents:
//! a sprite animator's `clip_source` (`.anim`), a particle emitter's
//! `particle_source` (`.particle`) and a tilemap's `source` (`.tilemap`).
//! Each is resolved by replacing the reference with the file's contents
//! before the scene is deserialized, and every loader of a scene from disk
//! has to do all three - one that forgets draws a scene with no ground, no
//! clips or empty emitters, and says nothing.
//!
//! That is what happened: the native runner resolved them for the scene it
//! started with and not for any scene a script loaded afterwards - which is
//! every game launched from a hub - and the web
//! runtime resolved `.anim` only. A platformer had no ground on Android. So
//! this is the one place the three are done together, and builds run it over
//! every scene they ship (`jengine_builder::stage`), so a shipped scene
//! carries its data whatever loads it.

use std::path::PathBuf;

use serde_json::Value;

/// Replace every `.anim`, `.particle` and `.tilemap` reference in `scene`
/// with the file's contents, searching `candidates` in order. The references
/// themselves are kept, so doing it twice is harmless. Returns what could
/// not be resolved; the rest of the scene is still inlined.
pub fn inline_sources(scene: &mut Value, candidates: &[PathBuf]) -> Vec<anyhow::Error> {
    let mut errors = crate::animation::asset::resolve_anim_sources(scene, candidates);
    errors.extend(crate::particles::asset::resolve_particle_sources(scene, candidates));
    errors.extend(crate::tilemap::asset::resolve_tilemap_sources(scene, candidates));
    errors
}

/// How many references of each kind `scene` carries: `(anim, particle,
/// tilemap)`. For a loader's log line.
pub fn count_sources(scene: &Value) -> (usize, usize, usize) {
    let count = |pointer: &str| {
        scene
            .get("entities")
            .and_then(Value::as_array)
            .map(|entities| {
                entities
                    .iter()
                    .filter(|e| e.pointer(pointer).and_then(Value::as_str).is_some())
                    .count()
            })
            .unwrap_or(0)
    };
    (
        count("/components/sprite_animator/clip_source"),
        count("/components/particle_emitter/particle_source"),
        count("/components/tilemap/source"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tilemap_source_is_inlined_and_kept() {
        let dir = std::env::temp_dir().join(format!("jengine_sources_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("tilemaps")).unwrap();
        std::fs::write(
            dir.join("tilemaps/level.tilemap"),
            r#"{ "tile_size": [32.0, 32.0],
                 "tilesets": [{ "texture_id": "atlases/tiles.png", "tile_width": 32,
                                "tile_height": 32, "columns": 2, "tile_count": 4,
                                "first_gid": 1 }],
                 "layers": [{ "name": "ground", "width": 2, "height": 1, "tiles": [1, 2] }] }"#,
        )
        .unwrap();
        let mut scene = serde_json::json!({
            "entities": [{ "name": "Level", "components": {
                "tilemap": { "source": "tilemaps/level.tilemap", "tile_size": [32.0, 32.0],
                             "tilesets": [], "layers": [] } } }]
        });
        let errors = inline_sources(&mut scene, std::slice::from_ref(&dir));
        std::fs::remove_dir_all(&dir).ok();
        assert!(errors.is_empty(), "{errors:?}");
        let tilemap = &scene["entities"][0]["components"]["tilemap"];
        assert_eq!(tilemap["tilesets"][0]["texture_id"], "atlases/tiles.png");
        assert_eq!(tilemap["layers"][0]["tiles"], serde_json::json!([1, 2]));
        assert_eq!(tilemap["source"], "tilemaps/level.tilemap", "the reference stays");
        assert_eq!(count_sources(&scene), (0, 0, 1));
    }
}
