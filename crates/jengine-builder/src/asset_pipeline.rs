//! Asset pipeline — orchestrates atlas packing and texture compression at build time.
//!
//! The pipeline scans a project's `assets/sprites/` directory, groups sprites into
//! atlases (one per subdirectory or via explicit config), packs them, optionally
//! compresses the output, and produces an `atlas_manifest.json` that maps original
//! sprite paths to atlas regions. Scene files can then be remapped to reference
//! atlas textures instead of individual sprites.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use image::GenericImageView;
use serde::{Deserialize, Serialize};

use crate::compression::{self, CompressionFormat, CompressionStats};
use crate::packer::{self, PackMode, PackerConfig};
use jengine_core::asset::atlas_def;

// Re-export manifest types from jengine-core so builder consumers can use them.
pub use jengine_core::renderer::atlas::{AtlasFileEntry, AtlasManifest, AtlasManifestEntry};

/// Asset pipeline configuration, typically stored in `project.jengine`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssetPipelineConfig {
    /// Atlas packing settings.
    pub atlas: AtlasPackingConfig,
    /// Image compression settings.
    pub compression: TextureCompressionConfig,
}



/// Atlas packing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtlasPackingConfig {
    /// Whether atlas packing is enabled.
    pub enabled: bool,
    /// Maximum atlas dimension in pixels.
    pub max_atlas_size: u32,
    /// Pixel padding between regions.
    pub padding: u32,
    /// Round atlas to power of two.
    pub power_of_two: bool,
    /// Glob patterns of sprite paths to exclude from packing.
    pub exclude_patterns: Vec<String>,
}

impl Default for AtlasPackingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_atlas_size: 4096,
            padding: 1,
            power_of_two: true,
            exclude_patterns: Vec::new(),
        }
    }
}

/// Texture compression configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextureCompressionConfig {
    /// Whether to compress textures during build.
    pub enabled: bool,
    /// Output format for compressed textures.
    pub format: CompressionFormat,
}

impl Default for TextureCompressionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            format: CompressionFormat::PngOptimized,
        }
    }
}

/// Results of running the asset pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPipelineResult {
    /// Atlas manifest (if packing was enabled).
    pub manifest: Option<AtlasManifest>,
    /// Number of atlases generated.
    pub atlases_created: usize,
    /// Total sprites packed.
    pub sprites_packed: usize,
    /// Compression statistics per file.
    pub compression_stats: Vec<CompressionStats>,
}

/// Run the full asset pipeline on a project's output directory.
///
/// This should be called AFTER assets have been copied to `output_path`.
/// It will:
/// 1. Scan `output_path/assets/sprites/` for images
/// 2. Group by subdirectory → one atlas per group
/// 3. Pack each group
/// 4. Optionally compress atlas images
/// 5. Write atlas images + metadata + manifest
/// 6. Keep the original sprite files (see the note where packing ends)
pub fn run_asset_pipeline(
    output_path: &Path,
    config: &AssetPipelineConfig,
) -> anyhow::Result<AssetPipelineResult> {
    let sprites_dir = output_path.join("assets").join("sprites");
    let mut result = AssetPipelineResult {
        manifest: None,
        atlases_created: 0,
        sprites_packed: 0,
        compression_stats: Vec::new(),
    };

    if !sprites_dir.exists() {
        log::info!("No sprites directory found, skipping asset pipeline");
        return Ok(result);
    }

    // -- Atlas definitions (.atlas) — process first --
    let assets_dir = output_path.join("assets");
    let def_manifest = process_atlas_definitions(&assets_dir, config)?;
    if !def_manifest.entries.is_empty() {
        result.atlases_created += def_manifest.atlases.len();
        result.sprites_packed += def_manifest.entries.len();
        log::info!(
            "Atlas definitions: {} atlases, {} sprites",
            def_manifest.atlases.len(),
            def_manifest.entries.len()
        );
    }

    // -- Atlas packing (auto-grouping by directory) --
    if config.atlas.enabled {
        let manifest = run_atlas_packing(&sprites_dir, config)?;
        result.atlases_created += manifest.atlases.len();
        result.sprites_packed += manifest.entries.len();

        // Merge into def_manifest or create new
        let mut combined = def_manifest;
        combined.entries.extend(manifest.entries);
        combined.atlases.extend(manifest.atlases);

        // Write manifest
        let manifest_path = output_path.join("assets").join("atlas_manifest.json");
        let json = serde_json::to_string_pretty(&combined)?;
        std::fs::write(&manifest_path, json)?;
        log::info!(
            "Atlas pipeline: {} atlases, {} sprites packed",
            result.atlases_created,
            result.sprites_packed
        );

        result.manifest = Some(combined);
    } else if !def_manifest.entries.is_empty() {
        // Only atlas definitions, no auto-packing
        let manifest_path = output_path.join("assets").join("atlas_manifest.json");
        let json = serde_json::to_string_pretty(&def_manifest)?;
        std::fs::write(&manifest_path, json)?;
        result.manifest = Some(def_manifest);
    } else {
        // No atlases — write an empty manifest so the web runtime doesn't 404
        let manifest_path = output_path.join("assets").join("atlas_manifest.json");
        let empty = AtlasManifest { entries: Default::default(), atlases: Vec::new() };
        let json = serde_json::to_string_pretty(&empty)?;
        std::fs::write(&manifest_path, json)?;
    }

    // -- Texture compression (on final output images) --
    if config.compression.enabled {
        result.compression_stats = run_compression(&sprites_dir, config)?;
        let total_saved: i64 = result
            .compression_stats
            .iter()
            .map(|s| s.original_bytes as i64 - s.compressed_bytes as i64)
            .sum();
        log::info!(
            "Compression: {} files processed, {:.1} KB saved",
            result.compression_stats.len(),
            total_saved as f64 / 1024.0
        );
    }

    Ok(result)
}

/// Scan for `.atlas` files, pack each into an atlas with points + animation clips.
fn process_atlas_definitions(
    assets_dir: &Path,
    config: &AssetPipelineConfig,
) -> anyhow::Result<AtlasManifest> {
    use jengine_core::renderer::atlas::{
        AtlasAnimFrame, AtlasAnimationClip,
    };

    let mut manifest = AtlasManifest {
        entries: HashMap::new(),
        atlases: Vec::new(),
    };

    // Recursively find *.atlas files
    let def_files = find_atlas_defs(assets_dir)?;
    if def_files.is_empty() {
        return Ok(manifest);
    }

    let atlas_out_dir = assets_dir.join("sprites").join("_atlases");
    std::fs::create_dir_all(&atlas_out_dir)?;

    for def_path in &def_files {
        // `load_atlas_def` names the file in its own error - do not repeat it.
        let def = atlas_def::load_atlas_def(def_path)?;
        let atlas_name = def_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("atlas")
            .trim_end_matches(".atlas")
            .to_string();

        // Collect all unique image paths from images + animation frames
        let mut image_paths: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for img in &def.images {
            if seen.insert(img.path.clone()) {
                image_paths.push(img.path.clone());
            }
        }
        for anim in &def.animations {
            for frame_path in &anim.frames {
                if seen.insert(frame_path.clone()) {
                    image_paths.push(frame_path.clone());
                }
            }
        }

        if image_paths.is_empty() {
            continue;
        }

        // Load images and track their dimensions
        let mut inputs: Vec<(String, image::DynamicImage)> = Vec::new();
        let mut image_dims: HashMap<String, (u32, u32)> = HashMap::new();
        for path in &image_paths {
            let full_path = assets_dir.join(path);
            let img = image::open(&full_path).map_err(|e| {
                anyhow::anyhow!("Atlas '{}': failed to load image '{}': {}", atlas_name, path, e)
            })?;
            let (w, h) = img.dimensions();
            image_dims.insert(path.clone(), (w, h));
            // Use filename stem as region name
            let region_name = Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(path)
                .to_string();
            inputs.push((region_name, img));
        }

        // Pack the sprites
        let pack_config = PackerConfig {
            max_size: config.atlas.max_atlas_size,
            padding: config.atlas.padding,
            power_of_two: config.atlas.power_of_two,
            mode: PackMode::RectPack,
        };
        let mut pack_result = packer::pack_sprites(&pack_config, &inputs)
            .map_err(|e| anyhow::anyhow!("atlas '{atlas_name}' ({}): {e}", def_path.display()))?;

        // Enrich regions with anchors and resolved points from the definition
        for img_entry in &def.images {
            let region_name = Path::new(&img_entry.path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&img_entry.path);
            if let Some(region) = pack_result.atlas_data.regions.iter_mut().find(|r| r.name == region_name) {
                region.anchor = img_entry.anchor;
                let (w, h) = image_dims.get(&img_entry.path).copied().unwrap_or((0, 0));
                region.points = atlas_def::resolve_points(
                    &def.shared_points,
                    &img_entry.points,
                    None,
                    w as f32,
                    h as f32,
                );
            }
        }

        // Build animation clips with resolved points
        let mut animation_clips = Vec::new();
        for anim in &def.animations {
            let mut frames = Vec::new();
            for (fi, frame_path) in anim.frames.iter().enumerate() {
                let region_name = Path::new(frame_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(frame_path)
                    .to_string();
                let (w, h) = image_dims.get(frame_path).copied().unwrap_or((0, 0));
                let frame_pts = anim.frame_points.get(&fi).map(|v| v.as_slice());
                let resolved = atlas_def::resolve_points(
                    &def.shared_points,
                    &anim.points,
                    frame_pts,
                    w as f32,
                    h as f32,
                );
                frames.push(AtlasAnimFrame {
                    region_name,
                    points: resolved,
                });
            }
            animation_clips.push(AtlasAnimationClip {
                name: anim.name.clone(),
                fps: anim.fps,
                looping: anim.looping,
                frames,
            });
        }
        pack_result.atlas_data.animation_clips = animation_clips;

        // Save the packed atlas
        let atlas_image_name = format!("{atlas_name}.png");
        let atlas_meta_name = format!("{atlas_name}.atlas");
        let atlas_image_path = atlas_out_dir.join(&atlas_image_name);
        let atlas_meta_path = atlas_out_dir.join(&atlas_meta_name);
        packer::save_packed_atlas(&pack_result, &atlas_image_path, &atlas_meta_path)?;

        // Add manifest entries for each region
        let atlas_rel = format!("sprites/_atlases/{atlas_image_name}");
        for region in &pack_result.atlas_data.regions {
            let uv = region.uv_rect(pack_result.atlas_data.width, pack_result.atlas_data.height);
            // Map original image paths to atlas regions by matching file stem
            for img_path in &image_paths {
                let stem = Path::new(img_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if stem == region.name {
                    manifest.entries.insert(
                        img_path.clone(),
                        AtlasManifestEntry {
                            atlas_texture: atlas_rel.clone(),
                            region_name: region.name.clone(),
                            uv_rect: uv,
                        },
                    );
                }
            }
        }

        manifest.atlases.push(AtlasFileEntry {
            image_path: format!("sprites/_atlases/{atlas_image_name}"),
            meta_path: format!("sprites/_atlases/{atlas_meta_name}"),
            region_count: pack_result.atlas_data.regions.len(),
            width: pack_result.atlas_data.width,
            height: pack_result.atlas_data.height,
        });

        // Remove the .atlas definition file from build output
        let _ = std::fs::remove_file(def_path);

        log::info!(
            "Packed atlas definition '{}': {} regions, {} animation clips",
            atlas_name,
            pack_result.atlas_data.regions.len(),
            pack_result.atlas_data.animation_clips.len()
        );
    }

    Ok(manifest)
}

/// Recursively find the `.atlas` files that are packing *inputs*.
///
/// `.atlas` names two different JSON shapes, and both are legitimate:
///
/// - an **atlas definition** (`name` / `images` / `animations`) - a list of
///   source images for this pipeline to pack;
/// - **packed metadata** (`width` / `height` / `regions`) - an already-packed
///   sheet, which a scene loads by naming it in `atlas_refs` (see
///   `jengine_core::renderer::atlas_asset`) and which needs no build step.
///
/// The second kind is normally the packer's own output under `_atlases/`,
/// which this scan skips by directory - so nothing noticed that the check was
/// really about *shape*. A hand-made or tool-made sheet living anywhere else
/// was fed to the definition parser and failed the whole build with
/// `missing field \`name\`` pointing at its closing brace.
fn find_atlas_defs(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    if !dir.exists() {
        return Ok(results);
    }
    find_atlas_defs_recursive(dir, &mut results)?;
    results.sort();
    Ok(results)
}

fn find_atlas_defs_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            // Skip _atlases output directory
            if path.file_name().is_some_and(|n| n == "_atlases") {
                continue;
            }
            find_atlas_defs_recursive(&path, out)?;
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".atlas"))
        {
            if is_packed_atlas_metadata(&path) {
                log::info!(
                    "Atlas '{}' is already packed - passing it through",
                    path.display()
                );
                continue;
            }
            out.push(path);
        }
    }
    Ok(())
}

/// Whether this `.atlas` holds an already-packed sheet's metadata.
///
/// Shape, not location: `regions` with no `images` is `AtlasData`. An
/// unreadable or unparseable file is *not* treated as packed, so it still
/// reaches the definition parser and reports its own error rather than being
/// silently skipped.
fn is_packed_atlas_metadata(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    value
        .as_object()
        .is_some_and(|o| o.contains_key("regions") && !o.contains_key("images"))
}

/// Group sprites by subdirectory and pack each group into an atlas.
fn run_atlas_packing(
    sprites_dir: &Path,
    config: &AssetPipelineConfig,
) -> anyhow::Result<AtlasManifest> {
    let mut manifest = AtlasManifest {
        entries: HashMap::new(),
        atlases: Vec::new(),
    };

    // Create output directory for atlases
    let atlas_out_dir = sprites_dir.join("_atlases");
    std::fs::create_dir_all(&atlas_out_dir)?;

    // Group sprites by immediate subdirectory
    let groups = group_sprites_by_dir(sprites_dir, &config.atlas.exclude_patterns)?;

    for (group_name, sprite_files) in &groups {
        if sprite_files.is_empty() {
            continue;
        }

        // Load all images in this group
        let mut inputs: Vec<(String, image::DynamicImage)> = Vec::new();
        for file_path in sprite_files {
            let rel = file_path
                .strip_prefix(sprites_dir)
                .unwrap_or(file_path)
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            let img = image::open(file_path)?;
            inputs.push((rel, img));
        }

        // Determine packing mode (auto-detect grid if all same size)
        let mode = if let Some(grid) = packer::auto_detect_grid(&inputs) {
            PackMode::Grid(grid)
        } else {
            PackMode::RectPack
        };

        let pack_config = PackerConfig {
            max_size: config.atlas.max_atlas_size,
            padding: config.atlas.padding,
            power_of_two: config.atlas.power_of_two,
            mode,
        };

        let pack_result = packer::pack_sprites(&pack_config, &inputs)?;

        // Save atlas
        let atlas_image_name = format!("{group_name}.png");
        let atlas_meta_name = format!("{group_name}.atlas");
        let atlas_image_path = atlas_out_dir.join(&atlas_image_name);
        let atlas_meta_path = atlas_out_dir.join(&atlas_meta_name);
        packer::save_packed_atlas(&pack_result, &atlas_image_path, &atlas_meta_path)?;

        // Optionally compress the atlas image
        if config.compression.enabled {
            let compressed =
                compression::compress_image(&pack_result.image, &config.compression.format)?;
            let output_ext = match &config.compression.format {
                CompressionFormat::WebPLossless | CompressionFormat::WebPLossy(_) => "webp",
                CompressionFormat::Jpeg(_) => "jpg",
                _ => "png",
            };
            let compressed_name = format!("{group_name}.{output_ext}");
            let compressed_path = atlas_out_dir.join(&compressed_name);
            std::fs::write(&compressed_path, &compressed)?;

            // If format changed, remove the original PNG
            if output_ext != "png" {
                let _ = std::fs::remove_file(&atlas_image_path);
            }
        }

        // Build manifest entries
        let atlas_rel = format!("sprites/_atlases/{atlas_image_name}");
        for region in &pack_result.atlas_data.regions {
            let uv = region.uv_rect(pack_result.atlas_data.width, pack_result.atlas_data.height);
            // Map original sprite path -> atlas region
            let original_key = format!("sprites/{}.png", region.name);
            manifest.entries.insert(
                original_key,
                AtlasManifestEntry {
                    atlas_texture: atlas_rel.clone(),
                    region_name: region.name.clone(),
                    uv_rect: uv,
                },
            );
        }

        manifest.atlases.push(AtlasFileEntry {
            image_path: format!("sprites/_atlases/{atlas_image_name}"),
            meta_path: format!("sprites/_atlases/{atlas_meta_name}"),
            region_count: pack_result.atlas_data.regions.len(),
            width: pack_result.atlas_data.width,
            height: pack_result.atlas_data.height,
        });

        // The original sprite files are kept. `remap_all_references` rewrites
        // every reference it can see - scenes, prefabs, `.anim`, `.gui` - but
        // not texture ids a script builds at runtime (`set_sprite`, a spawn
        // with a computed path). Deleting the originals turned each of those
        // into a missing texture in the shipped build. Keeping them costs
        // download size for sprites that end up unused; a missing sprite
        // costs the game.
        let _ = &sprite_files;
    }

    Ok(manifest)
}

/// Group sprite files by their immediate parent subdirectory.
///
/// Sprites in the root of `sprites/` are grouped under `"_root"`.
/// Each subdirectory becomes its own atlas group.
fn group_sprites_by_dir(
    sprites_dir: &Path,
    exclude_patterns: &[String],
) -> anyhow::Result<Vec<(String, Vec<PathBuf>)>> {
    let mut groups: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let image_exts = ["png", "jpg", "jpeg"];

    collect_sprite_files(sprites_dir, sprites_dir, &image_exts, exclude_patterns, &mut groups)?;

    // Sort for deterministic output
    let mut sorted: Vec<(String, Vec<PathBuf>)> = groups.into_iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, files) in &mut sorted {
        files.sort();
    }
    Ok(sorted)
}

fn collect_sprite_files(
    base: &Path,
    dir: &Path,
    exts: &[&str],
    exclude_patterns: &[String],
    groups: &mut HashMap<String, Vec<PathBuf>>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Skip the _atlases output directory
        if path.file_name().is_some_and(|n| n == "_atlases") {
            continue;
        }

        if path.is_dir() {
            collect_sprite_files(base, &path, exts, exclude_patterns, groups)?;
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if !exts.iter().any(|&e| e.eq_ignore_ascii_case(ext)) {
                continue;
            }

            // A sheet with its own packed `.atlas` beside it (the editor's
            // sheet slicer writes one) is already an atlas: packing it again
            // would move its pixels and leave its regions pointing into the
            // wrong picture. It is left whole.
            if is_packed_atlas_metadata(&path.with_extension("atlas")) {
                log::info!("Sprite '{}' has its own packed .atlas - not repacked", path.display());
                continue;
            }

            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            // Check exclude patterns (simple substring match)
            if exclude_patterns.iter().any(|p| rel.contains(p.as_str())) {
                continue;
            }

            // Group by immediate parent directory name
            let group = path
                .parent()
                .and_then(|p| p.strip_prefix(base).ok())
                .and_then(|p| {
                    let s = p.to_string_lossy().replace('\\', "/");
                    if s.is_empty() { None } else { Some(s) }
                })
                .unwrap_or_else(|| "_root".to_string());

            groups.entry(group).or_default().push(path);
        }
    }
    Ok(())
}

/// Run compression on all remaining image files in the sprites directory.
fn run_compression(
    sprites_dir: &Path,
    config: &AssetPipelineConfig,
) -> anyhow::Result<Vec<CompressionStats>> {
    let mut stats = Vec::new();
    compress_dir_recursive(sprites_dir, &config.compression.format, &mut stats)?;
    Ok(stats)
}

fn compress_dir_recursive(
    dir: &Path,
    format: &CompressionFormat,
    stats: &mut Vec<CompressionStats>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            compress_dir_recursive(&path, format, stats)?;
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && ["png", "jpg", "jpeg"].iter().any(|&e| e.eq_ignore_ascii_case(ext))
        {
            match compression::compress_file(&path, &path, format) {
                Ok(s) => stats.push(s),
                Err(e) => log::warn!("Failed to compress {}: {e}", path.display()),
            }
        }
    }
    Ok(())
}

/// Place a sub-rect of an original image inside that image's packed region.
///
/// `local` is `[u_min, v_min, u_max, v_max]` in the original image; the result
/// is the same rect in atlas coordinates.
fn map_into_region(region: [f32; 4], local: [f32; 4]) -> [f32; 4] {
    let w = region[2] - region[0];
    let h = region[3] - region[1];
    [
        region[0] + local[0] * w,
        region[1] + local[1] * h,
        region[0] + local[2] * w,
        region[1] + local[3] * h,
    ]
}

fn uv_of(value: &serde_json::Value) -> [f32; 4] {
    value
        .get("uv_rect")
        .and_then(|u| serde_json::from_value(u.clone()).ok())
        .unwrap_or([0.0, 0.0, 1.0, 1.0])
}

/// Point one `{ texture_id, uv_rect }` object at its packed region, keeping
/// any sub-rect it drew. Returns the entry used, if the texture was packed.
///
/// The sub-rect used to be overwritten with the whole region, so a sprite
/// drawing one frame of a sheet drew the entire sheet once packed.
fn remap_textured<'m>(
    obj: &mut serde_json::Value,
    manifest: &'m AtlasManifest,
) -> Option<&'m AtlasManifestEntry> {
    let texture_id = obj.get("texture_id").and_then(|t| t.as_str())?;
    let entry = manifest.entries.get(texture_id)?;
    let mapped = map_into_region(entry.uv_rect, uv_of(obj));
    obj["texture_id"] = serde_json::Value::String(entry.atlas_texture.clone());
    obj["uv_rect"] = serde_json::json!(mapped);
    Some(entry)
}

/// Remap the frames of a `clips` object (`{ name: { frames: [...] } }`).
///
/// A frame with its own `texture_id` is remapped against that texture. A
/// spritesheet frame - no `texture_id` - draws from `sheet`, the texture of
/// the sprite or `.anim` it belongs to: its `uv_rect` is a sub-rect of that
/// sheet, so once the sheet is packed it has to move with it. Only
/// `texture_id` frames used to be remapped, so every spritesheet animation
/// sampled the wrong part of the atlas page.
///
/// A frame naming a `region` is left alone: it resolves against an atlas at
/// runtime, not against the packed original.
fn remap_clips(clips: &mut serde_json::Value, sheet: Option<&AtlasManifestEntry>, manifest: &AtlasManifest) {
    let Some(clips) = clips.as_object_mut() else {
        return;
    };
    for clip in clips.values_mut() {
        remap_frames(clip, sheet, manifest);
    }
}

fn remap_frames(clip: &mut serde_json::Value, sheet: Option<&AtlasManifestEntry>, manifest: &AtlasManifest) {
    let Some(frames) = clip.get_mut("frames").and_then(|f| f.as_array_mut()) else {
        return;
    };
    for frame in frames {
        if frame.get("region").is_some_and(|r| !r.is_null()) {
            continue;
        }
        if frame.get("texture_id").is_some_and(|t| t.is_string()) {
            remap_textured(frame, manifest);
        } else if let Some(entry) = sheet {
            let mapped = map_into_region(entry.uv_rect, uv_of(frame));
            frame["uv_rect"] = serde_json::json!(mapped);
        }
    }
}

/// Remap one entity's `components` object: its sprite and animator.
fn remap_components(components: &mut serde_json::Value, manifest: &AtlasManifest) {
    let sheet = components
        .get_mut("sprite")
        .and_then(|sprite| remap_textured(sprite, manifest))
        .cloned();
    if let Some(clips) = components
        .get_mut("sprite_animator")
        .and_then(|a| a.get_mut("clips"))
    {
        remap_clips(clips, sheet.as_ref(), manifest);
    }
}

/// Remap sprite references in a scene to point to atlas textures.
///
/// For each entity's `SpriteComponent`, if its `texture_id` appears in the
/// manifest, point it at the atlas texture with its `uv_rect` placed inside the
/// region; its animator's frames move with it.
pub fn remap_scene_references(scene_json: &mut serde_json::Value, manifest: &AtlasManifest) {
    let Some(entities) = scene_json.get_mut("entities").and_then(|e| e.as_array_mut()) else {
        return;
    };
    for entity in entities {
        if let Some(components) = entity.get_mut("components") {
            remap_components(components, manifest);
        }
    }
}

/// Remap a `.prefab` file: one entity's `components` at the top level.
///
/// Prefabs were never remapped, so every spawned sprite whose texture was
/// packed pointed at a file the build had deleted.
pub fn remap_prefab_references(prefab_json: &mut serde_json::Value, manifest: &AtlasManifest) {
    if let Some(components) = prefab_json.get_mut("components") {
        remap_components(components, manifest);
    }
}

/// Remap a `.anim` file. Its top-level `texture_id` is the sheet its
/// spritesheet frames are cut from; both the multi-clip and the legacy
/// single-clip layouts are handled.
///
/// A clip file with no `texture_id` borrows the texture of whichever sprite
/// uses it, which this pass cannot see - its spritesheet frames are left in
/// original-image coordinates. Give such an `.anim` a `texture_id` if its
/// sheet is packed.
pub fn remap_anim_references(anim_json: &mut serde_json::Value, manifest: &AtlasManifest) {
    let sheet = anim_json
        .get("texture_id")
        .and_then(|t| t.as_str())
        .and_then(|t| manifest.entries.get(t))
        .cloned();
    if let Some(entry) = &sheet {
        anim_json["texture_id"] = serde_json::Value::String(entry.atlas_texture.clone());
    }
    if let Some(clips) = anim_json.get_mut("clips") {
        remap_clips(clips, sheet.as_ref(), manifest);
    } else {
        remap_frames(anim_json, sheet.as_ref(), manifest);
    }
}

/// Remap a `.gui` document: every `Image` node, at any depth.
pub fn remap_gui_references(gui_json: &mut serde_json::Value, manifest: &AtlasManifest) {
    fn walk(node: &mut serde_json::Value, manifest: &AtlasManifest) {
        if let Some(node_type) = node.get_mut("node_type")
            && node_type.get("type").and_then(|t| t.as_str()) == Some("Image")
        {
            remap_textured(node_type, manifest);
        }
        if let Some(children) = node.get_mut("children").and_then(|c| c.as_array_mut()) {
            for child in children {
                walk(child, manifest);
            }
        }
    }
    if let Some(root) = gui_json.get_mut("root") {
        walk(root, manifest);
    }
}

/// Read the asset pipeline config from a project's `project.jengine` file.
///
/// Returns defaults if the file doesn't exist or doesn't contain pipeline settings.
pub fn read_pipeline_config(project_path: &Path) -> AssetPipelineConfig {
    let project_file = project_path.join("project.jengine");
    if let Ok(contents) = std::fs::read_to_string(&project_file)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&contents)
        && let Some(pipeline) = val.get("settings").and_then(|s| s.get("asset_pipeline"))
        && let Ok(config) = serde_json::from_value::<AssetPipelineConfig>(pipeline.clone())
    {
        return config;
    }
    AssetPipelineConfig::default()
}

/// Rewrite every asset that names a sprite texture - `.scene`, `.prefab`,
/// `.anim` and `.gui` files under `assets_dir` - to point at the packed atlas.
///
/// This used to be scenes only, so prefabs, shared animation clips and GUI
/// images kept naming the original files.
pub fn remap_all_references(assets_dir: &Path, manifest: &AtlasManifest) -> anyhow::Result<()> {
    if !assets_dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(assets_dir)? {
        let path = entry?.path();
        if path.is_dir() {
            remap_all_references(&path, manifest)?;
            continue;
        }
        let remap: fn(&mut serde_json::Value, &AtlasManifest) =
            match path.extension().and_then(|e| e.to_str()) {
                Some("scene") => remap_scene_references,
                Some("prefab") => remap_prefab_references,
                Some("anim") => remap_anim_references,
                Some("gui") => remap_gui_references,
                _ => continue,
            };
        let contents = std::fs::read_to_string(&path)?;
        let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&contents) else {
            log::warn!("Skipping unparseable asset during atlas remap: {}", path.display());
            continue;
        };
        let before = json.clone();
        remap(&mut json, manifest);
        if json != before {
            std::fs::write(&path, serde_json::to_string_pretty(&json)?)?;
            log::info!("Remapped atlas references: {}", path.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_already_packed_sheet_is_not_a_packing_input() {
        // The build failed outright on a hand-made sheet: every `.atlas` was
        // fed to the definition parser, which reported a missing `name` at the
        // closing brace. Only the packer's own output escaped, and only
        // because it lives in `_atlases/`, which the scan skips by name.
        let dir = std::env::temp_dir().join("jengine_pipeline_packed_atlas");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sprites")).unwrap();

        let packed = dir.join("sprites").join("creatures.atlas");
        std::fs::write(
            &packed,
            r#"{ "width": 128, "height": 192,
                 "regions": [ { "name": "a", "x": 0, "y": 0, "width": 32, "height": 32 } ] }"#,
        )
        .unwrap();

        let definition = dir.join("sprites").join("ship.atlas");
        std::fs::write(
            &definition,
            r#"{ "name": "Ship", "images": [ { "name": "idle", "path": "sprites/idle.png" } ] }"#,
        )
        .unwrap();

        let found = find_atlas_defs(&dir).unwrap();
        assert_eq!(found, vec![definition], "{found:?}");
    }

    #[test]
    fn a_sliced_sheet_is_not_repacked() {
        // A sheet the editor sliced carries its regions in a packed `.atlas`
        // beside it; packed again, its pixels move and every region points
        // into the wrong picture.
        let dir = std::env::temp_dir().join("jengine_pipeline_sliced_sheet");
        let _ = std::fs::remove_dir_all(&dir);
        let sprites = dir.join("sprites").join("chars");
        std::fs::create_dir_all(&sprites).unwrap();
        write_test_png(&sprites.join("hero.png"), 4, 2);
        write_test_png(&sprites.join("coin.png"), 2, 2);
        std::fs::write(
            sprites.join("hero.atlas"),
            r#"{ "width": 4, "height": 2, "regions": [ { "name": "hero_0", "x": 0, "y": 0, "width": 2, "height": 2 } ] }"#,
        )
        .unwrap();
        let groups = group_sprites_by_dir(&dir.join("sprites"), &[]).unwrap();
        let files: Vec<String> = groups
            .iter()
            .flat_map(|(_, f)| f.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()))
            .collect();
        assert_eq!(files, vec!["coin.png".to_string()], "{groups:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_broken_atlas_still_reaches_the_parser() {
        // Skipping on shape must not become skipping on failure - a malformed
        // file has to report its own error, not vanish from the build.
        let dir = std::env::temp_dir().join("jengine_pipeline_broken_atlas");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let broken = dir.join("bad.atlas");
        std::fs::write(&broken, "{ not json").unwrap();

        assert_eq!(find_atlas_defs(&dir).unwrap(), vec![broken]);
    }

    /// Creates a small test PNG image and writes it to `path`.
    fn write_test_png(path: &Path, width: u32, height: u32) {
        let img = image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x * 60) as u8, (y * 60) as u8, 128, 255])
        });
        img.save(path).expect("Failed to write test PNG");
    }

    /// Create a minimal project directory with sprites.
    fn setup_mock_project(dir: &Path, sprite_files: &[&str]) {
        let sprites_dir = dir.join("assets").join("sprites");
        std::fs::create_dir_all(&sprites_dir).unwrap();
        for file in sprite_files {
            let sprite_path = sprites_dir.join(file);
            if let Some(parent) = sprite_path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            write_test_png(&sprite_path, 16, 16);
        }
    }

    #[test]
    fn test_remap_scene_references() {
        let mut scene = serde_json::json!({
            "name": "test",
            "entities": [
                {
                    "name": "Player",
                    "components": {
                        "sprite": {
                            "texture_id": "sprites/player/idle.png",
                            "color": [1,1,1,1],
                            "z_order": 0
                        }
                    }
                },
                {
                    "name": "BG",
                    "components": {
                        "sprite": {
                            "texture_id": "sprites/bg.png",
                            "color": [1,1,1,1],
                            "z_order": -1
                        }
                    }
                }
            ]
        });

        let mut manifest = AtlasManifest {
            entries: HashMap::new(),
            atlases: Vec::new(),
        };
        manifest.entries.insert(
            "sprites/player/idle.png".to_string(),
            AtlasManifestEntry {
                atlas_texture: "sprites/_atlases/player.png".to_string(),
                region_name: "player/idle".to_string(),
                uv_rect: [0.0, 0.0, 0.5, 1.0],
            },
        );

        remap_scene_references(&mut scene, &manifest);

        // Player sprite should be remapped
        let player_sprite = &scene["entities"][0]["components"]["sprite"];
        assert_eq!(
            player_sprite["texture_id"].as_str().unwrap(),
            "sprites/_atlases/player.png"
        );
        assert_eq!(player_sprite["uv_rect"].as_array().unwrap().len(), 4);

        // BG sprite should be unchanged (not in manifest)
        let bg_sprite = &scene["entities"][1]["components"]["sprite"];
        assert_eq!(bg_sprite["texture_id"].as_str().unwrap(), "sprites/bg.png");
        assert!(bg_sprite.get("uv_rect").is_none());
    }

    fn manifest_with(texture: &str, region: [f32; 4]) -> AtlasManifest {
        let mut manifest = AtlasManifest { entries: HashMap::new(), atlases: Vec::new() };
        manifest.entries.insert(
            texture.to_string(),
            AtlasManifestEntry {
                atlas_texture: "sprites/_atlases/page.png".to_string(),
                region_name: "sheet".to_string(),
                uv_rect: region,
            },
        );
        manifest
    }

    fn uv(v: &serde_json::Value) -> [f32; 4] {
        serde_json::from_value(v.clone()).unwrap()
    }

    #[test]
    fn a_sprite_sub_rect_is_placed_inside_its_region() {
        // The sheet sits in the right half of the page; the sprite draws the
        // sheet's bottom-left quarter.
        let manifest = manifest_with("sprites/sheet.png", [0.5, 0.0, 1.0, 1.0]);
        let mut scene = serde_json::json!({ "entities": [ { "components": {
            "sprite": { "texture_id": "sprites/sheet.png", "uv_rect": [0.0, 0.5, 0.5, 1.0] }
        } } ] });
        remap_scene_references(&mut scene, &manifest);
        let sprite = &scene["entities"][0]["components"]["sprite"];
        assert_eq!(sprite["texture_id"], "sprites/_atlases/page.png");
        assert_eq!(uv(&sprite["uv_rect"]), [0.5, 0.5, 0.75, 1.0], "the sub-rect was replaced by the whole region");
    }

    #[test]
    fn spritesheet_frames_move_with_their_sprite() {
        let manifest = manifest_with("sprites/sheet.png", [0.5, 0.0, 1.0, 1.0]);
        let mut scene = serde_json::json!({ "entities": [ { "components": {
            "sprite": { "texture_id": "sprites/sheet.png" },
            "sprite_animator": { "clips": { "walk": { "frames": [
                { "uv_rect": [0.0, 0.0, 0.5, 1.0] },
                { "uv_rect": [0.5, 0.0, 1.0, 1.0] },
                { "region": "named" }
            ] } } }
        } } ] });
        remap_scene_references(&mut scene, &manifest);
        let frames = &scene["entities"][0]["components"]["sprite_animator"]["clips"]["walk"]["frames"];
        assert_eq!(uv(&frames[0]["uv_rect"]), [0.5, 0.0, 0.75, 1.0], "spritesheet frame not remapped");
        assert_eq!(uv(&frames[1]["uv_rect"]), [0.75, 0.0, 1.0, 1.0]);
        assert!(frames[2].get("uv_rect").is_none(), "a region frame resolves at runtime and must be left alone");
    }

    #[test]
    fn prefabs_anim_files_and_gui_images_are_remapped() {
        let manifest = manifest_with("sprites/sheet.png", [0.0, 0.0, 0.5, 0.5]);

        let mut prefab = serde_json::json!({ "name": "Enemy", "components": {
            "sprite": { "texture_id": "sprites/sheet.png" }
        } });
        remap_prefab_references(&mut prefab, &manifest);
        assert_eq!(prefab["components"]["sprite"]["texture_id"], "sprites/_atlases/page.png");

        let mut anim = serde_json::json!({ "texture_id": "sprites/sheet.png", "clips": { "walk": {
            "frames": [ { "uv_rect": [0.5, 0.5, 1.0, 1.0] } ]
        } } });
        remap_anim_references(&mut anim, &manifest);
        assert_eq!(anim["texture_id"], "sprites/_atlases/page.png");
        assert_eq!(uv(&anim["clips"]["walk"]["frames"][0]["uv_rect"]), [0.25, 0.25, 0.5, 0.5]);

        let mut gui = serde_json::json!({ "root": { "node_type": { "type": "Panel" }, "children": [
            { "node_type": { "type": "Button" }, "children": [
                { "node_type": { "type": "Image", "texture_id": "sprites/sheet.png",
                                 "uv_rect": [0.0, 0.0, 0.5, 0.5] }, "children": [] }
            ] }
        ] } });
        remap_gui_references(&mut gui, &manifest);
        let image = &gui["root"]["children"][0]["children"][0]["node_type"];
        assert_eq!(image["texture_id"], "sprites/_atlases/page.png");
        assert_eq!(uv(&image["uv_rect"]), [0.0, 0.0, 0.25, 0.25]);
    }

    #[test]
    fn packing_keeps_the_original_sprite_files() {
        // Scripts can name a texture at runtime, which no remap can rewrite.
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        setup_mock_project(tmp.path(), &["player/idle.png", "player/run.png"]);
        let mut config = AssetPipelineConfig::default();
        config.atlas.enabled = true;
        let result = run_asset_pipeline(tmp.path(), &config).unwrap();
        assert_eq!(result.sprites_packed, 2);
        assert!(tmp.path().join("assets/sprites/player/idle.png").exists());
    }

    #[test]
    fn test_group_sprites_by_dir() {
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        let sprites = tmp.path();
        // Root sprite
        write_test_png(&sprites.join("bg.png"), 8, 8);
        // Subdir sprites
        std::fs::create_dir_all(sprites.join("player")).unwrap();
        write_test_png(&sprites.join("player").join("idle.png"), 16, 16);
        write_test_png(&sprites.join("player").join("run.png"), 16, 16);
        std::fs::create_dir_all(sprites.join("enemies")).unwrap();
        write_test_png(&sprites.join("enemies").join("slime.png"), 16, 16);

        let groups = group_sprites_by_dir(sprites, &[]).unwrap();
        let group_names: Vec<&str> = groups.iter().map(|(n, _)| n.as_str()).collect();
        assert!(group_names.contains(&"_root"));
        assert!(group_names.contains(&"player"));
        assert!(group_names.contains(&"enemies"));

        // Player group should have 2 files
        let player_group = groups.iter().find(|(n, _)| n == "player").unwrap();
        assert_eq!(player_group.1.len(), 2);
    }

    #[test]
    fn test_group_sprites_excludes_patterns() {
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        let sprites = tmp.path();
        std::fs::create_dir_all(sprites.join("ui")).unwrap();
        write_test_png(&sprites.join("ui").join("button.png"), 8, 8);
        std::fs::create_dir_all(sprites.join("player")).unwrap();
        write_test_png(&sprites.join("player").join("idle.png"), 16, 16);

        let groups = group_sprites_by_dir(sprites, &["ui/".to_string()]).unwrap();
        let group_names: Vec<&str> = groups.iter().map(|(n, _)| n.as_str()).collect();
        assert!(group_names.contains(&"player"));
        assert!(!group_names.contains(&"ui"), "ui/ should be excluded");
    }

    #[test]
    fn test_run_pipeline_no_sprites_dir() {
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        let config = AssetPipelineConfig {
            atlas: AtlasPackingConfig {
                enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let result = run_asset_pipeline(tmp.path(), &config).unwrap();
        assert_eq!(result.atlases_created, 0);
        assert_eq!(result.sprites_packed, 0);
    }

    #[test]
    fn test_run_pipeline_atlas_packing() {
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        setup_mock_project(tmp.path(), &["player/idle.png", "player/run.png"]);

        let config = AssetPipelineConfig {
            atlas: AtlasPackingConfig {
                enabled: true,
                max_atlas_size: 256,
                padding: 1,
                power_of_two: true,
                exclude_patterns: Vec::new(),
            },
            compression: TextureCompressionConfig {
                enabled: false,
                ..Default::default()
            },
        };

        let result = run_asset_pipeline(tmp.path(), &config).unwrap();
        assert_eq!(result.atlases_created, 1);
        assert_eq!(result.sprites_packed, 2);
        assert!(result.manifest.is_some());

        // Manifest should map both sprites
        let manifest = result.manifest.unwrap();
        assert!(manifest.entries.contains_key("sprites/player/idle.png"));
        assert!(manifest.entries.contains_key("sprites/player/run.png"));

        // Atlas file should exist
        let atlas_dir = tmp.path().join("assets").join("sprites").join("_atlases");
        assert!(atlas_dir.exists());
        assert!(atlas_dir.join("player.png").exists());
        assert!(atlas_dir.join("player.atlas").exists());

        // Manifest file should be written
        assert!(
            tmp.path()
                .join("assets")
                .join("atlas_manifest.json")
                .exists()
        );
    }

    #[test]
    fn test_run_pipeline_disabled() {
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        setup_mock_project(tmp.path(), &["player/idle.png"]);

        let config = AssetPipelineConfig::default(); // both disabled
        let result = run_asset_pipeline(tmp.path(), &config).unwrap();
        assert_eq!(result.atlases_created, 0);
        assert!(result.manifest.is_none());

        // Original sprite should still exist
        assert!(tmp.path().join("assets/sprites/player/idle.png").exists());
    }

    #[test]
    fn test_read_pipeline_config_missing_file() {
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        let config = read_pipeline_config(tmp.path());
        assert!(!config.atlas.enabled);
        assert!(!config.compression.enabled);
    }

    #[test]
    fn test_read_pipeline_config_from_project_file() {
        let tmp = tempfile::tempdir().expect("Failed to create temp dir");
        let project_file = tmp.path().join("project.jengine");
        std::fs::write(
            &project_file,
            serde_json::json!({
                "name": "test",
                "settings": {
                    "asset_pipeline": {
                        "atlas": {
                            "enabled": true,
                            "max_atlas_size": 2048,
                            "padding": 2,
                            "power_of_two": false,
                            "exclude_patterns": ["ui/"]
                        },
                        "compression": {
                            "enabled": true,
                            "format": "WebPLossless"
                        }
                    }
                }
            })
            .to_string(),
        )
        .unwrap();

        let config = read_pipeline_config(tmp.path());
        assert!(config.atlas.enabled);
        assert_eq!(config.atlas.max_atlas_size, 2048);
        assert_eq!(config.atlas.padding, 2);
        assert!(!config.atlas.power_of_two);
        assert_eq!(config.atlas.exclude_patterns, vec!["ui/"]);
        assert!(config.compression.enabled);
    }
}
