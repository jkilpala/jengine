//! Atlas CRUD Tauri commands — create, load, save atlas definitions + preview.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use jengine_core::asset::atlas_def::AtlasDefinition;

/// Image dimensions returned to the editor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDimensions {
    pub width: u32,
    pub height: u32,
}

/// Atlas preview data for the editor canvas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtlasImagePreview {
    /// Base64-encoded PNG data for the image.
    pub image_base64: String,
    pub width: u32,
    pub height: u32,
}

/// Create a new empty atlas definition file.
/// Returns the relative path to the created file.
#[tauri::command]
pub fn create_atlas(project_path: String, name: String) -> Result<String, String> {
    let project = PathBuf::from(&project_path);
    let assets_dir = project.join("assets").join("sprites");

    // Sanitize name
    let safe_name = name.trim().replace(' ', "_");
    if safe_name.is_empty() {
        return Err("Atlas name cannot be empty".to_string());
    }

    let file_name = format!("{safe_name}.atlas");
    let full_path = assets_dir.join(&file_name);

    if full_path.exists() {
        return Err(format!("Atlas '{file_name}' already exists"));
    }

    let def = AtlasDefinition::new(safe_name);
    let json = serde_json::to_string_pretty(&def)
        .map_err(|e| format!("Failed to serialize atlas: {e}"))?;

    std::fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create directory: {e}"))?;
    std::fs::write(&full_path, json)
        .map_err(|e| format!("Failed to write atlas file: {e}"))?;

    // Return path relative to project root
    let rel = format!("assets/sprites/{file_name}");
    Ok(rel)
}

/// Load an atlas definition from disk.
/// `atlas_path` is relative to the project root (e.g. "assets/sprites/my.atlas").
#[tauri::command]
pub fn load_atlas(project_path: String, atlas_path: String) -> Result<AtlasDefinition, String> {
    let project = PathBuf::from(&project_path);
    let full_path = project.join(&atlas_path);

    if !full_path.exists() {
        return Err(format!("Atlas file not found: {atlas_path}"));
    }

    let json = std::fs::read_to_string(&full_path)
        .map_err(|e| format!("Failed to read atlas: {e}"))?;
    let def: AtlasDefinition = serde_json::from_str(&json)
        .map_err(|e| format!("Failed to parse atlas: {e}"))?;

    Ok(def)
}

/// Save an atlas definition to disk.
/// `atlas_path` is relative to the project root (e.g. "assets/sprites/my.atlas").
#[tauri::command]
pub fn save_atlas(
    project_path: String,
    atlas_path: String,
    definition: AtlasDefinition,
) -> Result<(), String> {
    let project = PathBuf::from(&project_path);
    let full_path = project.join(&atlas_path);

    let json = serde_json::to_string_pretty(&definition)
        .map_err(|e| format!("Failed to serialize atlas: {e}"))?;

    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {e}"))?;
    }

    std::fs::write(&full_path, json)
        .map_err(|e| format!("Failed to write atlas: {e}"))?;

    Ok(())
}

/// Get the pixel dimensions of an image file.
/// `image_path` is relative to the assets directory (e.g. "sprites/player.png").
#[tauri::command]
pub fn get_image_dimensions(
    project_path: String,
    image_path: String,
) -> Result<ImageDimensions, String> {
    let project = PathBuf::from(&project_path);
    let full_path = project.join("assets").join(&image_path);

    if !full_path.exists() {
        return Err(format!("Image not found: {image_path}"));
    }

    let img = image::open(&full_path)
        .map_err(|e| format!("Failed to open image: {e}"))?;

    Ok(ImageDimensions {
        width: img.width(),
        height: img.height(),
    })
}

/// Load an image and return it as base64-encoded PNG for the editor canvas.
/// `image_path` is relative to the assets directory (e.g. "sprites/player.png").
#[tauri::command]
pub fn load_atlas_image(
    project_path: String,
    image_path: String,
) -> Result<AtlasImagePreview, String> {
    let project = PathBuf::from(&project_path);
    let full_path = project.join("assets").join(&image_path);

    if !full_path.exists() {
        return Err(format!("Image not found: {image_path}"));
    }

    let img = image::open(&full_path)
        .map_err(|e| format!("Failed to open image: {e}"))?;

    let width = img.width();
    let height = img.height();

    // Encode to PNG bytes, then base64
    let mut png_bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png_bytes);
    img.write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode PNG: {e}"))?;

    let b64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &png_bytes,
    );

    Ok(AtlasImagePreview {
        image_base64: b64,
        width,
        height,
    })
}

// ── Atlas definition browsing (for Inspector pickers) ────────────────

/// An image entry from an atlas definition, for the sprite picker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtlasDefImageEntry {
    /// Atlas display name (e.g. "PlayerShip").
    pub atlas_name: String,
    /// Relative path to the `.atlas` file (from assets/).
    pub atlas_path: String,
    /// Logical image name within the atlas.
    pub image_name: String,
    /// Source image path relative to assets/ (e.g. "sprites/ship_idle.png").
    pub image_path: String,
    /// Anchor from the atlas definition.
    pub anchor: [f32; 2],
}

/// An animation entry from an atlas definition, for the animator importer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtlasDefAnimationEntry {
    /// Atlas display name.
    pub atlas_name: String,
    /// Relative path to the `.atlas` file (from assets/).
    pub atlas_path: String,
    /// Animation clip name.
    pub animation_name: String,
    /// Frames per second.
    pub fps: f32,
    /// Whether the animation loops.
    pub looping: bool,
    /// Number of frames.
    pub frame_count: usize,
    /// Source image paths for each frame (relative to assets/).
    pub frame_paths: Vec<String>,
}

/// List all images across all `.atlas` definition files for the sprite picker.
#[tauri::command]
pub fn list_atlas_def_images(project_path: String) -> Result<Vec<AtlasDefImageEntry>, String> {
    let root = PathBuf::from(&project_path);
    let assets_dir = root.join("assets");
    let mut entries = Vec::new();
    collect_atlas_defs(&assets_dir, &assets_dir, &mut |atlas_path, def| {
        for img in &def.images {
            entries.push(AtlasDefImageEntry {
                atlas_name: def.name.clone(),
                atlas_path: atlas_path.to_string(),
                image_name: img.name.clone(),
                image_path: img.path.clone(),
                anchor: img.anchor,
            });
        }
    });
    Ok(entries)
}

/// List all animations across all `.atlas` definition files.
#[tauri::command]
pub fn list_atlas_def_animations(
    project_path: String,
) -> Result<Vec<AtlasDefAnimationEntry>, String> {
    let root = PathBuf::from(&project_path);
    let assets_dir = root.join("assets");
    let mut entries = Vec::new();
    collect_atlas_defs(&assets_dir, &assets_dir, &mut |atlas_path, def| {
        for anim in &def.animations {
            entries.push(AtlasDefAnimationEntry {
                atlas_name: def.name.clone(),
                atlas_path: atlas_path.to_string(),
                animation_name: anim.name.clone(),
                fps: anim.fps,
                looping: anim.looping,
                frame_count: anim.frames.len(),
                frame_paths: anim.frames.clone(),
            });
        }
    });
    Ok(entries)
}

/// Recursively scan for `.atlas` files and invoke `cb` with the relative path and parsed definition.
fn collect_atlas_defs(
    base: &Path,
    dir: &Path,
    cb: &mut dyn FnMut(&str, &AtlasDefinition),
) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_atlas_defs(base, &path, cb);
        } else if path.extension().and_then(|e| e.to_str()) == Some("atlas") {
            let Ok(json) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(def) = serde_json::from_str::<AtlasDefinition>(&json) else {
                continue;
            };
            let rel = path
                .strip_prefix(base)
                .ok()
                .and_then(|r| r.to_str())
                .map(|s| s.replace('\\', "/"))
                .unwrap_or_default();
            cb(&rel, &def);
        }
    }
}
