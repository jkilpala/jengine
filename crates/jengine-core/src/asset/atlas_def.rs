//! Atlas definition — the source-of-truth file authored in the editor.
//!
//! An `.atlas` file references source images and defines animations + named points.
//! Packing into a GPU atlas texture happens at build time (not stored here).

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::renderer::atlas::SpritePoint;

/// Top-level atlas definition stored as `.atlas`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtlasDefinition {
    /// Display name for this atlas (e.g. "PlayerShip").
    pub name: String,
    /// Individual images in this atlas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageEntry>,
    /// Sprite animations (sequences of images).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animations: Vec<AnimationEntry>,
    /// Points shared across ALL images and animations in this atlas.
    /// Individual images/animations/frames can override these by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared_points: Vec<SpritePoint>,
}

/// A single image within an atlas definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageEntry {
    /// Logical name for this image (e.g. "ship_idle").
    pub name: String,
    /// Path to the source image, relative to `assets/` (e.g. "sprites/ship_idle.png").
    pub path: String,
    /// Anchor/pivot as a fraction of width/height. [0.5, 0.5] = center.
    #[serde(default = "default_anchor")]
    pub anchor: [f32; 2],
    /// Named points specific to this image (override shared_points by name).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<SpritePoint>,
}

/// A sprite animation — an ordered sequence of source images with timing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnimationEntry {
    /// Name of this animation clip (e.g. "fly", "explode").
    pub name: String,
    /// Frames per second for playback.
    #[serde(default = "default_fps")]
    pub fps: f32,
    /// Whether the animation loops.
    #[serde(default = "default_true")]
    pub looping: bool,
    /// Ordered list of source image paths (relative to `assets/`).
    pub frames: Vec<String>,
    /// Points shared across all frames of this animation (override shared_points by name).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<SpritePoint>,
    /// Per-frame point overrides. Key = frame index, value = points for that frame.
    /// These override both shared_points and animation-level points by name.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub frame_points: HashMap<usize, Vec<SpritePoint>>,
}

fn default_anchor() -> [f32; 2] {
    [0.5, 0.5]
}

fn default_fps() -> f32 {
    10.0
}

fn default_true() -> bool {
    true
}

impl AtlasDefinition {
    /// Create a new empty atlas definition.
    pub fn new(name: String) -> Self {
        Self {
            name,
            images: Vec::new(),
            animations: Vec::new(),
            shared_points: Vec::new(),
        }
    }
}

/// Load an atlas definition from a JSON file.
pub fn load_atlas_def(path: &Path) -> anyhow::Result<AtlasDefinition> {
    let json = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("reading atlas definition {}: {e}", path.display()))?;
    match serde_json::from_str(&json) {
        Ok(def) => Ok(def),
        Err(e) => Err(anyhow::anyhow!(
            "parsing atlas definition {}: {e}{}",
            path.display(),
            packed_metadata_hint(&json)
        )),
    }
}

/// Extra guidance when an `.atlas` holds *packed* atlas metadata.
///
/// Two different JSON shapes are called "atlas", and both are legitimate:
/// this file, the authored definition (`name` / `images` / `animations`), and
/// `AtlasData`, the `width` / `height` / `regions` metadata describing an
/// already-packed sheet. They are loaded by different code - a definition by
/// the build pipeline, packed metadata by `renderer::atlas_asset` when a scene
/// names it in `atlas_refs`, which needs no build step at all.
///
/// Parsing one as the other fails with a "missing field" error at the end of the
/// file, which says nothing about what is wrong - and means the *caller*
/// reached for the wrong parser, not that the file is bad.
fn packed_metadata_hint(json: &str) -> &'static str {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return "";
    };
    let Some(obj) = value.as_object() else {
        return "";
    };
    if obj.contains_key("regions") && !obj.contains_key("images") {
        return concat!(
            "\n  This is packed atlas metadata (width/height/regions), not an atlas",
            " definition (name/images/animations). Both are valid `.atlas` files: a",
            " packed sheet is loaded by naming it in a scene's `atlas_refs` and needs",
            " no build step. If a build reached here the caller chose the wrong",
            " parser - the file is not necessarily wrong."
        );
    }
    ""
}

/// Save an atlas definition to a JSON file.
pub fn save_atlas_def(path: &Path, def: &AtlasDefinition) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(def)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, json)?;
    Ok(())
}

/// Resolve the effective points for a given context by merging the 3-tier hierarchy:
/// 1. `shared_points` (atlas-wide base)
/// 2. `layer_points` (animation-level or image-level override by name)
/// 3. `frame_points` (per-frame override by name, if animating)
///
/// Auto-inserts Origin (0, 0) and Center (w/2, h/2) if not present at any level.
pub fn resolve_points(
    shared_points: &[SpritePoint],
    layer_points: &[SpritePoint],
    frame_points: Option<&[SpritePoint]>,
    image_width: f32,
    image_height: f32,
) -> Vec<SpritePoint> {
    let mut points: HashMap<String, SpritePoint> = HashMap::new();

    // Layer 1: shared (base)
    for p in shared_points {
        points.insert(p.name.clone(), p.clone());
    }

    // Layer 2: animation/image level (overrides shared by name)
    for p in layer_points {
        points.insert(p.name.clone(), p.clone());
    }

    // Layer 3: per-frame overrides (highest precedence)
    if let Some(fps) = frame_points {
        for p in fps {
            points.insert(p.name.clone(), p.clone());
        }
    }

    // Auto-insert Origin if missing
    if !points.contains_key("Origin") {
        points.insert(
            "Origin".to_string(),
            SpritePoint {
                name: "Origin".to_string(),
                x: 0.0,
                y: 0.0,
            },
        );
    }

    // Auto-insert Center if missing
    if !points.contains_key("Center") {
        points.insert(
            "Center".to_string(),
            SpritePoint {
                name: "Center".to_string(),
                x: image_width / 2.0,
                y: image_height / 2.0,
            },
        );
    }

    // Return sorted for deterministic output
    let mut result: Vec<SpritePoint> = points.into_values().collect();
    result.sort_by(|a, b| {
        // Origin first, Center second, then alphabetical
        let rank = |name: &str| match name {
            "Origin" => 0,
            "Center" => 1,
            _ => 2,
        };
        rank(&a.name).cmp(&rank(&b.name)).then(a.name.cmp(&b.name))
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_auto_origin_center() {
        let result = resolve_points(&[], &[], None, 64.0, 32.0);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "Origin");
        assert_eq!(result[0].x, 0.0);
        assert_eq!(result[0].y, 0.0);
        assert_eq!(result[1].name, "Center");
        assert_eq!(result[1].x, 32.0);
        assert_eq!(result[1].y, 16.0);
    }

    #[test]
    fn shared_points_override_defaults() {
        let shared = vec![SpritePoint {
            name: "Origin".to_string(),
            x: 10.0,
            y: 20.0,
        }];
        let result = resolve_points(&shared, &[], None, 64.0, 32.0);
        let origin = result.iter().find(|p| p.name == "Origin").unwrap();
        assert_eq!(origin.x, 10.0);
        assert_eq!(origin.y, 20.0);
    }

    #[test]
    fn layer_overrides_shared() {
        let shared = vec![SpritePoint {
            name: "LeftCannon".to_string(),
            x: 10.0,
            y: 10.0,
        }];
        let layer = vec![SpritePoint {
            name: "LeftCannon".to_string(),
            x: 47.0,
            y: 17.0,
        }];
        let result = resolve_points(&shared, &layer, None, 64.0, 32.0);
        let cannon = result.iter().find(|p| p.name == "LeftCannon").unwrap();
        assert_eq!(cannon.x, 47.0);
        assert_eq!(cannon.y, 17.0);
    }

    #[test]
    fn frame_overrides_all() {
        let shared = vec![SpritePoint {
            name: "LeftCannon".to_string(),
            x: 10.0,
            y: 10.0,
        }];
        let layer = vec![SpritePoint {
            name: "LeftCannon".to_string(),
            x: 47.0,
            y: 17.0,
        }];
        let frame = vec![SpritePoint {
            name: "LeftCannon".to_string(),
            x: 50.0,
            y: 25.0,
        }];
        let result = resolve_points(&shared, &layer, Some(&frame), 64.0, 32.0);
        let cannon = result.iter().find(|p| p.name == "LeftCannon").unwrap();
        assert_eq!(cannon.x, 50.0);
        assert_eq!(cannon.y, 25.0);
    }

    #[test]
    fn roundtrip_json() {
        let def = AtlasDefinition {
            name: "PlayerShip".to_string(),
            images: vec![ImageEntry {
                name: "idle".to_string(),
                path: "sprites/ship_idle.png".to_string(),
                anchor: [0.5, 0.5],
                points: vec![SpritePoint {
                    name: "LeftCannon".to_string(),
                    x: 47.0,
                    y: 17.0,
                }],
            }],
            animations: vec![AnimationEntry {
                name: "fly".to_string(),
                fps: 12.0,
                looping: true,
                frames: vec![
                    "sprites/ship_fly_01.png".to_string(),
                    "sprites/ship_fly_02.png".to_string(),
                ],
                points: vec![],
                frame_points: HashMap::new(),
            }],
            shared_points: vec![SpritePoint {
                name: "Origin".to_string(),
                x: 0.0,
                y: 32.0,
            }],
        };

        let json = serde_json::to_string_pretty(&def).unwrap();
        let loaded: AtlasDefinition = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.name, "PlayerShip");
        assert_eq!(loaded.images.len(), 1);
        assert_eq!(loaded.images[0].name, "idle");
        assert_eq!(loaded.images[0].points[0].name, "LeftCannon");
        assert_eq!(loaded.animations.len(), 1);
        assert_eq!(loaded.animations[0].name, "fly");
        assert_eq!(loaded.animations[0].frames.len(), 2);
        assert_eq!(loaded.shared_points[0].name, "Origin");
    }

    #[test]
    fn a_parse_failure_names_the_file() {
        // The whole complaint: `missing field \`name\` at line 174 column 1`
        // with no way to tell which of a project's assets said it.
        let dir = std::env::temp_dir().join("jengine_atlas_def_err");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("creatures.atlas");
        std::fs::write(&path, "{ \"width\": 128 }").unwrap();

        let err = load_atlas_def(&path).unwrap_err().to_string();
        assert!(err.contains("creatures.atlas"), "{err}");
    }

    #[test]
    fn packed_metadata_in_an_atlas_file_says_so() {
        // Both shapes are called "atlas". A tool emitting the packer's output
        // into a `.atlas` fails at the closing brace with `missing field
        // \`name\``, which points at the end of the file and explains nothing.
        let dir = std::env::temp_dir().join("jengine_atlas_def_shape");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("creatures.atlas");
        std::fs::write(
            &path,
            r#"{ "width": 128, "height": 192, "regions": [ { "name": "a" } ] }"#,
        )
        .unwrap();

        let err = load_atlas_def(&path).unwrap_err().to_string();
        assert!(err.contains("packed atlas metadata"), "{err}");

        // An ordinary broken definition must not get the hint - it would be
        // advice pointing away from the real mistake.
        let other = dir.join("broken.atlas");
        std::fs::write(&other, r#"{ "images": [] }"#).unwrap();
        let err = load_atlas_def(&other).unwrap_err().to_string();
        assert!(!err.contains("packed atlas metadata"), "{err}");
    }
}
