//! Spritesheet packer — pack individual sprites into atlas textures.
//!
//! Supports two packing modes:
//! - **Grid**: uniform cell size, row-major layout (ideal for animation frames)
//! - **RectPack**: variable-sized sprites bin-packed via `rectangle-pack`

use std::collections::BTreeMap;
use std::path::Path;

use image::{DynamicImage, GenericImageView, RgbaImage};
use jengine_core::renderer::atlas::{AtlasData, AtlasRegion};
use rectangle_pack::{
    contains_smallest_box, pack_rects, volume_heuristic, GroupedRectsToPlace, PackedLocation,
    RectToInsert, TargetBin,
};
use serde::{Deserialize, Serialize};

/// Packing mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PackMode {
    /// All sprites placed on a uniform grid (same cell size).
    Grid(GridConfig),
    /// Variable-sized sprites bin-packed to minimize wasted space.
    RectPack,
}

/// Configuration for grid-mode packing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridConfig {
    /// Width of each cell in pixels.
    pub cell_width: u32,
    /// Height of each cell in pixels.
    pub cell_height: u32,
    /// Number of columns. `None` = auto-calculate from sprite count.
    pub columns: Option<u32>,
}

/// Spritesheet packer configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackerConfig {
    /// Maximum atlas dimension (width or height), default 4096.
    pub max_size: u32,
    /// Pixel padding between regions, default 1.
    pub padding: u32,
    /// Round atlas dimensions up to the next power of two.
    pub power_of_two: bool,
    /// Packing mode.
    pub mode: PackMode,
}

impl Default for PackerConfig {
    fn default() -> Self {
        Self {
            max_size: 4096,
            padding: 1,
            power_of_two: true,
            mode: PackMode::RectPack,
        }
    }
}

/// Result of packing sprites into an atlas.
pub struct PackResult {
    /// The packed atlas image (RGBA).
    pub image: DynamicImage,
    /// Atlas metadata with named regions.
    pub atlas_data: AtlasData,
}

/// Pack a set of named sprites into a single atlas.
///
/// `inputs` is a list of `(name, image)` pairs. Names become region identifiers.
pub fn pack_sprites(
    config: &PackerConfig,
    inputs: &[(String, DynamicImage)],
) -> anyhow::Result<PackResult> {
    if inputs.is_empty() {
        anyhow::bail!("No sprites to pack");
    }

    match &config.mode {
        PackMode::Grid(grid) => pack_grid(config, grid, inputs),
        PackMode::RectPack => pack_rectpack(config, inputs),
    }
}

/// Grid packing: arrange uniformly-sized frames in row-major order.
fn pack_grid(
    config: &PackerConfig,
    grid: &GridConfig,
    inputs: &[(String, DynamicImage)],
) -> anyhow::Result<PackResult> {
    let count = inputs.len() as u32;
    let cols = grid.columns.unwrap_or_else(|| {
        let sqrt = (count as f32).sqrt().ceil() as u32;
        sqrt.max(1)
    });
    let rows = count.div_ceil(cols);

    let cell_w = grid.cell_width + config.padding;
    let cell_h = grid.cell_height + config.padding;
    let mut atlas_w = cols * cell_w - config.padding;
    let mut atlas_h = rows * cell_h - config.padding;

    if config.power_of_two {
        atlas_w = atlas_w.next_power_of_two();
        atlas_h = atlas_h.next_power_of_two();
    }

    if atlas_w > config.max_size || atlas_h > config.max_size {
        anyhow::bail!(
            "Grid atlas would be {}x{} which exceeds max_size {}",
            atlas_w,
            atlas_h,
            config.max_size
        );
    }

    let mut atlas = RgbaImage::new(atlas_w, atlas_h);
    let mut regions = Vec::with_capacity(inputs.len());

    for (i, (name, img)) in inputs.iter().enumerate() {
        let col = (i as u32) % cols;
        let row = (i as u32) / cols;
        let x = col * cell_w;
        let y = row * cell_h;

        // Copy sprite into atlas, clamping to cell size
        let (sw, sh) = img.dimensions();
        let copy_w = sw.min(grid.cell_width);
        let copy_h = sh.min(grid.cell_height);
        let rgba = img.to_rgba8();

        for py in 0..copy_h {
            for px in 0..copy_w {
                atlas.put_pixel(x + px, y + py, *rgba.get_pixel(px, py));
            }
        }

        regions.push(AtlasRegion {
            name: name.clone(),
            x,
            y,
            width: grid.cell_width,
            height: grid.cell_height,
            anchor: [0.5, 0.5],
            points: Vec::new(),
        });
    }

    Ok(PackResult {
        image: DynamicImage::ImageRgba8(atlas),
        atlas_data: AtlasData {
            width: atlas_w,
            height: atlas_h,
            regions,
            animation_clips: Vec::new(),
        },
    })
}

/// Rectangle bin-packing for variable-sized sprites.
fn pack_rectpack(
    config: &PackerConfig,
    inputs: &[(String, DynamicImage)],
) -> anyhow::Result<PackResult> {
    // Build the set of rectangles to place
    let mut rects_to_place: GroupedRectsToPlace<usize, ()> = GroupedRectsToPlace::new();
    for (i, (_, img)) in inputs.iter().enumerate() {
        let (w, h) = img.dimensions();
        rects_to_place.push_rect(
            i,
            None,
            RectToInsert::new(w + config.padding * 2, h + config.padding * 2, 1),
        );
    }

    // Create the target bin
    let max = config.max_size;
    let mut target_bins = BTreeMap::new();
    target_bins.insert(0, TargetBin::new(max, max, 1));

    // Pack
    let placements = pack_rects(
        &rects_to_place,
        &mut target_bins,
        &volume_heuristic,
        &contains_smallest_box,
    )
    .map_err(|_| anyhow::anyhow!("Failed to pack sprites — atlas too small (max {}x{})", max, max))?;

    // Determine actual used bounds
    let mut max_x: u32 = 0;
    let mut max_y: u32 = 0;
    let mut locations: Vec<(usize, PackedLocation)> = Vec::with_capacity(inputs.len());

    for (i, _) in inputs.iter().enumerate() {
        let (_, loc) = placements.packed_locations().get(&i).ok_or_else(|| {
            anyhow::anyhow!("Missing placement for sprite {i}")
        })?;

        let right = loc.x() + loc.width();
        let bottom = loc.y() + loc.height();
        max_x = max_x.max(right);
        max_y = max_y.max(bottom);
        locations.push((i, *loc));
    }

    let mut atlas_w = max_x;
    let mut atlas_h = max_y;
    if config.power_of_two {
        atlas_w = atlas_w.next_power_of_two();
        atlas_h = atlas_h.next_power_of_two();
    }

    // Blit sprites into atlas
    let mut atlas = RgbaImage::new(atlas_w, atlas_h);
    let mut regions = Vec::with_capacity(inputs.len());

    for (i, loc) in &locations {
        let (name, img) = &inputs[*i];
        let (w, h) = img.dimensions();
        let x = loc.x() + config.padding;
        let y = loc.y() + config.padding;
        let rgba = img.to_rgba8();

        for py in 0..h {
            for px in 0..w {
                atlas.put_pixel(x + px, y + py, *rgba.get_pixel(px, py));
            }
        }

        regions.push(AtlasRegion {
            name: name.clone(),
            x,
            y,
            width: w,
            height: h,
            anchor: [0.5, 0.5],
            points: Vec::new(),
        });
    }

    // Sort regions by name for deterministic output
    regions.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(PackResult {
        image: DynamicImage::ImageRgba8(atlas),
        atlas_data: AtlasData {
            width: atlas_w,
            height: atlas_h,
            regions,
            animation_clips: Vec::new(),
        },
    })
}

/// Save a packed atlas to disk as PNG image + JSON metadata.
pub fn save_packed_atlas(
    result: &PackResult,
    image_path: &Path,
    meta_path: &Path,
) -> anyhow::Result<()> {
    result.image.save(image_path)?;
    let json = serde_json::to_string_pretty(&result.atlas_data)?;
    std::fs::write(meta_path, json)?;
    log::info!(
        "Saved atlas: {}x{} with {} regions -> {:?}",
        result.atlas_data.width,
        result.atlas_data.height,
        result.atlas_data.regions.len(),
        image_path
    );
    Ok(())
}

/// Recursively collect image files from a directory.
///
/// Returns `(relative_name, loaded_image)` pairs. Names use forward slashes
/// and have extensions stripped (e.g. `"player/walk_0"`).
pub fn collect_sprites(
    dir: &Path,
    extensions: &[&str],
) -> anyhow::Result<Vec<(String, DynamicImage)>> {
    let mut results = Vec::new();
    collect_sprites_recursive(dir, dir, extensions, &mut results)?;
    results.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(results)
}

fn collect_sprites_recursive(
    base: &Path,
    dir: &Path,
    extensions: &[&str],
    out: &mut Vec<(String, DynamicImage)>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_sprites_recursive(base, &path, extensions, out)?;
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && extensions.iter().any(|&e| e.eq_ignore_ascii_case(ext))
        {
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            let img = image::open(&path)?;
            out.push((rel, img));
        }
    }
    Ok(())
}

/// Detect if all images have the same dimensions, suggesting grid mode.
pub fn auto_detect_grid(images: &[(String, DynamicImage)]) -> Option<GridConfig> {
    if images.is_empty() {
        return None;
    }
    let (w, h) = images[0].1.dimensions();
    if images.iter().all(|(_, img)| img.dimensions() == (w, h)) {
        Some(GridConfig {
            cell_width: w,
            cell_height: h,
            columns: None,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn make_solid(w: u32, h: u32, color: Rgba<u8>) -> DynamicImage {
        let mut img = RgbaImage::new(w, h);
        for pixel in img.pixels_mut() {
            *pixel = color;
        }
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn grid_packing_basic() {
        let inputs: Vec<(String, DynamicImage)> = (0..4)
            .map(|i| {
                (
                    format!("frame_{i}"),
                    make_solid(16, 16, Rgba([255, 0, 0, 255])),
                )
            })
            .collect();

        let config = PackerConfig {
            max_size: 256,
            padding: 0,
            power_of_two: false,
            mode: PackMode::Grid(GridConfig {
                cell_width: 16,
                cell_height: 16,
                columns: Some(2),
            }),
        };

        let result = pack_sprites(&config, &inputs).unwrap();
        assert_eq!(result.atlas_data.regions.len(), 4);
        assert_eq!(result.atlas_data.width, 32);
        assert_eq!(result.atlas_data.height, 32);

        // Check no overlaps
        for (i, a) in result.atlas_data.regions.iter().enumerate() {
            for b in result.atlas_data.regions.iter().skip(i + 1) {
                assert!(
                    a.x + a.width <= b.x || b.x + b.width <= a.x
                        || a.y + a.height <= b.y || b.y + b.height <= a.y,
                    "Regions {:?} and {:?} overlap",
                    a.name,
                    b.name,
                );
            }
        }
    }

    #[test]
    fn rectpack_variable_sizes() {
        let inputs = vec![
            ("small".into(), make_solid(8, 8, Rgba([255, 0, 0, 255]))),
            ("medium".into(), make_solid(32, 32, Rgba([0, 255, 0, 255]))),
            ("tall".into(), make_solid(16, 48, Rgba([0, 0, 255, 255]))),
            ("wide".into(), make_solid(64, 16, Rgba([255, 255, 0, 255]))),
        ];

        let config = PackerConfig {
            max_size: 256,
            padding: 1,
            power_of_two: true,
            mode: PackMode::RectPack,
        };

        let result = pack_sprites(&config, &inputs).unwrap();
        assert_eq!(result.atlas_data.regions.len(), 4);

        // All regions within atlas bounds
        for r in &result.atlas_data.regions {
            assert!(r.x + r.width <= result.atlas_data.width);
            assert!(r.y + r.height <= result.atlas_data.height);
        }

        // Atlas is power of two
        assert!(result.atlas_data.width.is_power_of_two());
        assert!(result.atlas_data.height.is_power_of_two());
    }

    #[test]
    fn auto_detect_grid_uniform() {
        let images: Vec<(String, DynamicImage)> = (0..3)
            .map(|i| (format!("f{i}"), make_solid(32, 32, Rgba([0, 0, 0, 255]))))
            .collect();
        let grid = auto_detect_grid(&images).unwrap();
        assert_eq!(grid.cell_width, 32);
        assert_eq!(grid.cell_height, 32);
    }

    #[test]
    fn auto_detect_grid_mixed_returns_none() {
        let images = vec![
            ("a".into(), make_solid(32, 32, Rgba([0, 0, 0, 255]))),
            ("b".into(), make_solid(16, 16, Rgba([0, 0, 0, 255]))),
        ];
        assert!(auto_detect_grid(&images).is_none());
    }
}
