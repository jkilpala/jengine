//! Spritesheet and texture compression Tauri commands.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Result of packing sprites in the editor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpritesheetPackResult {
    /// Base64-encoded atlas PNG image for preview.
    pub image_base64: String,
    /// Atlas metadata JSON.
    pub atlas_data: jengine_core::renderer::atlas::AtlasData,
    /// Atlas width.
    pub width: u32,
    /// Atlas height.
    pub height: u32,
    /// Packing efficiency as a percentage (0-100).
    pub efficiency: f32,
}

/// Config passed from the editor UI for packing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorPackConfig {
    pub max_size: u32,
    pub padding: u32,
    pub power_of_two: bool,
    /// "grid" or "rectpack"
    pub mode: String,
    /// For grid mode
    pub cell_width: Option<u32>,
    pub cell_height: Option<u32>,
    pub columns: Option<u32>,
}

/// Result of compressing a single texture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionResult {
    pub original_bytes: u64,
    pub compressed_bytes: u64,
    pub savings_percent: f64,
    pub format: String,
}

/// Atlas preview data returned to the editor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtlasPreviewData {
    pub atlas_data: jengine_core::renderer::atlas::AtlasData,
    /// Base64-encoded atlas image for canvas rendering.
    pub image_base64: String,
}

/// Per-file savings estimate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSavingsEstimate {
    /// Relative path from assets/.
    pub path: String,
    pub original_bytes: u64,
    pub compressed_bytes: u64,
    pub savings_percent: f64,
}

/// Aggregate savings estimate for an entire project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavingsEstimate {
    pub total_original_bytes: u64,
    pub total_compressed_bytes: u64,
    pub total_savings_percent: f64,
    pub file_count: usize,
    pub files: Vec<FileSavingsEstimate>,
}

#[tauri::command]
pub fn pack_spritesheet(
    project_path: String,
    sprite_paths: Vec<String>,
    config: EditorPackConfig,
) -> Result<SpritesheetPackResult, String> {
    use jengine_builder::packer::{GridConfig, PackMode, PackerConfig};

    let project = PathBuf::from(&project_path);

    // Load sprites
    let mut inputs = Vec::new();
    for rel_path in &sprite_paths {
        let full_path = project.join(rel_path);
        let img = image::open(&full_path).map_err(|e| format!("Failed to load {rel_path}: {e}"))?;
        // Use filename without extension as region name
        let name = std::path::Path::new(rel_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(rel_path)
            .to_string();
        inputs.push((name, img));
    }

    let mode = if config.mode == "grid" {
        PackMode::Grid(GridConfig {
            cell_width: config.cell_width.unwrap_or(32),
            cell_height: config.cell_height.unwrap_or(32),
            columns: config.columns,
        })
    } else {
        PackMode::RectPack
    };

    let pack_config = PackerConfig {
        max_size: config.max_size,
        padding: config.padding,
        power_of_two: config.power_of_two,
        mode,
    };

    let result =
        jengine_builder::packer::pack_sprites(&pack_config, &inputs).map_err(|e| e.to_string())?;

    // Calculate packing efficiency
    let used_pixels: u64 = result
        .atlas_data
        .regions
        .iter()
        .map(|r| r.width as u64 * r.height as u64)
        .sum();
    let total_pixels = result.atlas_data.width as u64 * result.atlas_data.height as u64;
    let efficiency = if total_pixels > 0 {
        (used_pixels as f32 / total_pixels as f32) * 100.0
    } else {
        0.0
    };

    // Encode image to PNG bytes then base64
    let mut png_bytes = std::io::Cursor::new(Vec::new());
    result
        .image
        .write_to(&mut png_bytes, image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode atlas: {e}"))?;

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes.into_inner());

    Ok(SpritesheetPackResult {
        image_base64: b64,
        atlas_data: result.atlas_data.clone(),
        width: result.atlas_data.width,
        height: result.atlas_data.height,
        efficiency,
    })
}

#[tauri::command]
pub fn save_spritesheet(
    project_path: String,
    output_name: String,
    image_base64: String,
    atlas_data_json: String,
) -> Result<(), String> {
    let project = PathBuf::from(&project_path);
    let atlas_dir = project.join("assets").join("sprites").join("_atlases");
    std::fs::create_dir_all(&atlas_dir).map_err(|e| e.to_string())?;

    // Decode base64 image
    use base64::Engine;
    let png_bytes = base64::engine::general_purpose::STANDARD
        .decode(&image_base64)
        .map_err(|e| format!("Invalid base64: {e}"))?;

    let image_path = atlas_dir.join(format!("{output_name}.png"));
    let meta_path = atlas_dir.join(format!("{output_name}.atlas"));

    std::fs::write(&image_path, &png_bytes).map_err(|e| e.to_string())?;
    std::fs::write(&meta_path, &atlas_data_json).map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn compress_texture(
    project_path: String,
    relative_path: String,
    format: String,
    quality: Option<u8>,
) -> Result<CompressionResult, String> {
    use jengine_builder::compression::{self, CompressionFormat};

    let project = PathBuf::from(&project_path);
    let input = project.join("assets").join(&relative_path);

    let comp_format = match format.as_str() {
        "png_optimized" => CompressionFormat::PngOptimized,
        "webp_lossless" => CompressionFormat::WebPLossless,
        "webp_lossy" => CompressionFormat::WebPLossy(quality.unwrap_or(80)),
        "jpeg" => CompressionFormat::Jpeg(quality.unwrap_or(85)),
        "basis_universal" => CompressionFormat::BasisUniversal(quality.unwrap_or(2)),
        _ => CompressionFormat::Png,
    };

    // Determine output extension
    let output_ext = match &comp_format {
        CompressionFormat::WebPLossless | CompressionFormat::WebPLossy(_) => "webp",
        CompressionFormat::Jpeg(_) => "jpg",
        CompressionFormat::BasisUniversal(_) => "basis",
        _ => "png",
    };

    let output = input.with_extension(output_ext);
    let stats =
        compression::compress_file(&input, &output, &comp_format).map_err(|e| e.to_string())?;

    // Remove original if format changed
    if output != input {
        let _ = std::fs::remove_file(&input);
    }

    Ok(CompressionResult {
        original_bytes: stats.original_bytes,
        compressed_bytes: stats.compressed_bytes,
        savings_percent: stats.savings_percent(),
        format: comp_format.to_string(),
    })
}

#[tauri::command]
pub fn preview_atlas(
    project_path: String,
    atlas_path: String,
) -> Result<AtlasPreviewData, String> {
    let project = PathBuf::from(&project_path);
    let meta_path = project.join(&atlas_path);

    // Load atlas metadata
    let meta_json =
        std::fs::read_to_string(&meta_path).map_err(|e| format!("Cannot read atlas: {e}"))?;
    let atlas_data: jengine_core::renderer::atlas::AtlasData =
        serde_json::from_str(&meta_json).map_err(|e| format!("Invalid atlas JSON: {e}"))?;

    // Load corresponding image (same name, .png extension)
    let image_path = meta_path.with_extension("png");
    let image_bytes =
        std::fs::read(&image_path).map_err(|e| format!("Cannot read atlas image: {e}"))?;

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&image_bytes);

    Ok(AtlasPreviewData {
        atlas_data,
        image_base64: b64,
    })
}

#[tauri::command]
pub fn estimate_savings(
    project_path: String,
    format: String,
    quality: Option<u8>,
) -> Result<SavingsEstimate, String> {
    use jengine_builder::compression::CompressionFormat;

    let project = PathBuf::from(&project_path);
    let sprites_dir = project.join("assets").join("sprites");

    if !sprites_dir.exists() {
        return Ok(SavingsEstimate {
            total_original_bytes: 0,
            total_compressed_bytes: 0,
            total_savings_percent: 0.0,
            file_count: 0,
            files: Vec::new(),
        });
    }

    let comp_format = match format.as_str() {
        "png_optimized" => CompressionFormat::PngOptimized,
        "webp_lossless" => CompressionFormat::WebPLossless,
        "webp_lossy" => CompressionFormat::WebPLossy(quality.unwrap_or(80)),
        "jpeg" => CompressionFormat::Jpeg(quality.unwrap_or(85)),
        "basis_universal" => CompressionFormat::BasisUniversal(quality.unwrap_or(2)),
        _ => CompressionFormat::Png,
    };

    let mut files = Vec::new();
    collect_image_files(&sprites_dir, &sprites_dir, &comp_format, &mut files)
        .map_err(|e| format!("Failed to scan sprites: {e}"))?;

    let total_original: u64 = files.iter().map(|f| f.original_bytes).sum();
    let total_compressed: u64 = files.iter().map(|f| f.compressed_bytes).sum();
    let total_savings = if total_original > 0 {
        (1.0 - total_compressed as f64 / total_original as f64) * 100.0
    } else {
        0.0
    };

    Ok(SavingsEstimate {
        total_original_bytes: total_original,
        total_compressed_bytes: total_compressed,
        total_savings_percent: total_savings,
        file_count: files.len(),
        files,
    })
}

/// Recursively collect image files and estimate compression savings in memory.
fn collect_image_files(
    base: &Path,
    dir: &Path,
    format: &jengine_builder::compression::CompressionFormat,
    results: &mut Vec<FileSavingsEstimate>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_image_files(base, &path, format, results)?;
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if !["png", "jpg", "jpeg"].iter().any(|&e| e.eq_ignore_ascii_case(ext)) {
                continue;
            }
            let original_bytes = std::fs::metadata(&path)
                .map(|m| m.len())
                .unwrap_or(0);
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            // Load image and compress in memory (don't write to disk)
            match image::open(&path) {
                Ok(img) => {
                    match jengine_builder::compression::compress_image(&img, format) {
                        Ok(compressed) => {
                            let compressed_bytes = compressed.len() as u64;
                            let savings = if original_bytes > 0 {
                                (1.0 - compressed_bytes as f64 / original_bytes as f64) * 100.0
                            } else {
                                0.0
                            };
                            results.push(FileSavingsEstimate {
                                path: rel,
                                original_bytes,
                                compressed_bytes,
                                savings_percent: savings,
                            });
                        }
                        Err(e) => {
                            log::warn!("Failed to compress {rel}: {e}");
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to load image {rel}: {e}");
                }
            }
        }
    }
    Ok(())
}

/// An individual atlas region entry for the sprite picker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtlasPickerEntry {
    /// Display label (region name).
    pub name: String,
    /// Path to the atlas PNG image (project-root-relative).
    pub atlas_image: String,
    /// UV rect [u_min, v_min, u_max, v_max].
    pub uv_rect: [f32; 4],
    /// Region pixel size [width, height].
    pub size: [f32; 2],
    /// Anchor [x, y] fraction.
    pub anchor: [f32; 2],
}

/// List all atlas regions across all .atlas files for use in the sprite picker.
#[tauri::command]
pub fn list_atlas_entries(project_path: String) -> Result<Vec<AtlasPickerEntry>, String> {
    let root = PathBuf::from(&project_path);
    let mut entries = Vec::new();
    collect_atlas_entries(&root, &root, &mut entries);
    Ok(entries)
}

fn collect_atlas_entries(base: &Path, dir: &Path, out: &mut Vec<AtlasPickerEntry>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_atlas_entries(base, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("atlas") {
            let Ok(json) = std::fs::read_to_string(&path) else { continue };
            let Ok(data) = serde_json::from_str::<jengine_core::renderer::atlas::AtlasData>(&json) else { continue };
            // Derive the atlas image path (same name but .png)
            // Paths are relative to assets/ dir since that's what the viewport texture loader expects
            let image_path = path.with_extension("png");
            let assets_dir = base.join("assets");
            let rel_image = image_path
                .strip_prefix(&assets_dir)
                .or_else(|_| image_path.strip_prefix(base))
                .ok()
                .and_then(|r| r.to_str())
                .map(|s| s.replace('\\', "/"))
                .unwrap_or_default();
            for region in &data.regions {
                let uv = region.uv_rect(data.width, data.height);
                out.push(AtlasPickerEntry {
                    name: region.name.clone(),
                    atlas_image: rel_image.clone(),
                    uv_rect: uv,
                    size: [region.width as f32, region.height as f32],
                    anchor: region.anchor,
                });
            }
        }
    }
}
