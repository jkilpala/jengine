//! How a texture is sampled: **nearest** - each texel a hard-edged square,
//! what pixel art wants and the engine's default - or **linear**: bilinear,
//! with mipmaps, so a texture drawn larger than it is stays smooth and one
//! drawn smaller does not shimmer. Unity sets it per texture (Filter Mode),
//! Godot per canvas item (`texture_filter`).
//!
//! Until Sep 2026 every texture was nearest and had no mipmaps, whatever it
//! held: the showcase's glows went blocky when stretched and its star fields
//! crawled when scaled down, and there was no way to ask otherwise.
//!
//! It is a project setting, `settings.texture_filter` in `project.jengine`:
//!
//! ```json
//! "texture_filter": { "default": "linear", "overrides": { "sprites/pixel/": "nearest" } }
//! ```
//!
//! An override is a texture-id **prefix**; the longest that matches wins.
//! Absent, everything is nearest - so no project written before the setting
//! looks any different. Applied where a texture is uploaded
//! (`AssetManager::ensure_wgpu_texture`), which every runtime shares; each
//! host hands the rules over with `AssetManager::set_texture_filters`.
//!
//! A packed sheet (an atlas, a font) filtered linearly bleeds a little of
//! each region's neighbours into its edge, and more at smaller mips - pad
//! the regions, or keep the sheet nearest with an override.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextureFilter {
    /// Each texel a hard-edged square, no mipmaps.
    #[default]
    Nearest,
    /// Bilinear within a level, blended between mipmap levels.
    Linear,
}

/// The project's rules: a default, and prefix overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextureFilterRules {
    #[serde(default)]
    pub default: TextureFilter,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, TextureFilter>,
}

impl TextureFilterRules {
    /// Whether this is what an absent setting means - everything nearest.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The rules in a project's `settings` object (its `texture_filter`
    /// key). Absent or malformed means the default, with a warning for the
    /// latter: a typo should not silently turn filtering off.
    pub fn from_settings(settings: Option<&serde_json::Value>) -> Self {
        let Some(v) = settings.and_then(|s| s.get("texture_filter")) else {
            return Self::default();
        };
        match serde_json::from_value(v.clone()) {
            Ok(rules) => rules,
            Err(e) => {
                log::warn!("settings.texture_filter is not valid ({e}); every texture stays nearest");
                Self::default()
            }
        }
    }

    /// How the texture `id` is sampled: the longest override prefix that
    /// matches it, else the default.
    pub fn filter_for(&self, id: &str) -> TextureFilter {
        let rule = self
            .overrides
            .iter()
            .filter(|(prefix, _)| id.starts_with(prefix.as_str()))
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(_, f)| *f);
        // A falling-sand grid is drawn a cell a pixel: nearest, unless the
        // project names it.
        #[cfg(feature = "pixel_sim")]
        if rule.is_none() && id.starts_with(crate::ecs::pixel_sim::TEXTURE_PREFIX) {
            return TextureFilter::Nearest;
        }
        // A tile light is a texel a cell, blended into a glow.
        #[cfg(feature = "tile_light")]
        if rule.is_none() && id.starts_with(crate::ecs::tile_light::TEXTURE_PREFIX) {
            return TextureFilter::Linear;
        }
        // Fog of war is a texel a cell, its edges soft.
        #[cfg(feature = "fog_of_war")]
        if rule.is_none() && id.starts_with(crate::ecs::fog_of_war::TEXTURE_PREFIX) {
            return TextureFilter::Linear;
        }
        // Smoke is a coarse grid of something soft: smooth, likewise.
        #[cfg(feature = "smoke")]
        if rule.is_none() && id.starts_with(crate::ecs::smoke::TEXTURE_PREFIX) {
            return TextureFilter::Linear;
        }
        rule.unwrap_or(self.default)
    }
}

fn srgb_to_linear_exact(b: u8) -> f32 {
    let v = b as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb_exact(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

// The two conversions by table, byte for byte the formulas above. A mip
// chain called them fifteen times a texel, each a `powf`: a 1280 x 720 image
// took ~65 ms natively and ~700 ms in wasm, on the frame its texture was first
// drawn - which is what the showcase's Long Corridor hitched on as each new
// hall came into view.

/// Every byte's linear value.
static SRGB_TO_LINEAR: std::sync::LazyLock<[f32; 256]> =
    std::sync::LazyLock::new(|| std::array::from_fn(|b| srgb_to_linear_exact(b as u8)));

/// `SRGB_FROM[b]` is the smallest linear value the formula encodes as `b` or
/// more (`b` from 1), found by bisecting over the bit patterns of positive
/// floats, which order as the floats do.
static SRGB_FROM: std::sync::LazyLock<[f32; 255]> = std::sync::LazyLock::new(|| {
    std::array::from_fn(|i| {
        let want = i as u8 + 1;
        let (mut lo, mut hi) = (0u32, 1.0f32.to_bits());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if linear_to_srgb_exact(f32::from_bits(mid)) >= want { hi = mid } else { lo = mid + 1 }
        }
        f32::from_bits(lo)
    })
});

fn srgb_to_linear(b: u8) -> f32 {
    SRGB_TO_LINEAR[b as usize]
}

fn linear_to_srgb(v: f32) -> u8 {
    // NaN and anything not above zero encode as 0, as the formula's do.
    if !(v > 0.0) {
        return 0;
    }
    SRGB_FROM.partition_point(|&t| t <= v) as u8
}

/// The mipmap levels below an RGBA8 image, each half the one above
/// (rounded down, never below 1) - a 2x2 box average.
///
/// For colour (`srgb`) the average is taken in linear light and weighted by
/// alpha: averaging sRGB bytes darkens every blend, and averaging a
/// transparent texel's colour in would put a dark fringe round every sprite
/// edge (the texel's colour is whatever the painter left there). A normal
/// map (`!srgb`) is averaged as the plain data it is.
pub fn mip_chain(width: u32, height: u32, rgba: &[u8], srgb: bool) -> Vec<(u32, u32, Vec<u8>)> {
    let mut out: Vec<(u32, u32, Vec<u8>)> = Vec::new();
    let (mut w, mut h) = (width, height);
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut dst = vec![0u8; (nw * nh * 4) as usize];
        let src = out.last().map_or(rgba, |l| l.2.as_slice());
        downsample(src, w, h, &mut dst, nw, (0, 0, nw, nh), srgb);
        out.push((nw, nh, dst));
        w = nw;
        h = nh;
    }
    out
}

/// One level from the level above: the texels of `dst` (width `nw`) inside
/// `rect` (`x0, y0, x1, y1`, end exclusive), each the 2x2 box under it in
/// `src` (`w` x `h`), clamped at the edge. The one filter [`mip_chain`] and
/// [`update_mip_chain`] share, so a level patched in place is byte for byte
/// the level a full rebuild makes.
fn downsample(src: &[u8], w: u32, h: u32, dst: &mut [u8], nw: u32, rect: (u32, u32, u32, u32), srgb: bool) {
    let (x0, y0, x1, y1) = rect;
    for y in y0..y1 {
        for x in x0..x1 {
            let mut acc = [0.0f32; 3];
            let mut alpha = 0.0f32;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (x * 2 + dx).min(w - 1);
                let sy = (y * 2 + dy).min(h - 1);
                let i = ((sy * w + sx) * 4) as usize;
                let a = src[i + 3] as f32 / 255.0;
                for c in 0..3 {
                    let v = if srgb { srgb_to_linear(src[i + c]) } else { src[i + c] as f32 / 255.0 };
                    acc[c] += if srgb { v * a } else { v };
                }
                alpha += a;
            }
            let o = ((y * nw + x) * 4) as usize;
            for c in 0..3 {
                dst[o + c] = if srgb {
                    if alpha > 0.0 { linear_to_srgb(acc[c] / alpha) } else { 0 }
                } else {
                    (acc[c] / 4.0 * 255.0 + 0.5) as u8
                };
            }
            dst[o + 3] = (alpha / 4.0 * 255.0 + 0.5) as u8;
        }
    }
}

/// Bring a [`mip_chain`] up to date after the base image changed inside
/// `rect` (`x, y, w, h`), recomputing only the texels of each level that
/// the change reaches. Answers each touched level's rectangle (`level`
/// counting the base as 0, then `x, y, w, h`), for the upload.
///
/// This is what lets a texture edited at runtime keep its mipmaps cheaply:
/// rebuilding the whole chain for a few changed texels costs a linear-light
/// average of every texel in the image, every frame something is painted.
pub fn update_mip_chain(
    base: &[u8],
    width: u32,
    height: u32,
    chain: &mut [(u32, u32, Vec<u8>)],
    rect: (u32, u32, u32, u32),
    srgb: bool,
) -> Vec<(u32, (u32, u32, u32, u32))> {
    let mut out = Vec::new();
    let (x, y, w, h) = rect;
    if w == 0 || h == 0 {
        return out;
    }
    // End-exclusive, in the level above's texels.
    let (mut x0, mut y0, mut x1, mut y1) = (x, y, (x + w).min(width), (y + h).min(height));
    let (mut sw, mut sh) = (width, height);
    for k in 0..chain.len() {
        let (nw, nh) = (chain[k].0, chain[k].1);
        // A source texel s is read by the texel s / 2 below it (and by none
        // past the last one, on an odd edge).
        let (nx0, ny0) = (x0 / 2, y0 / 2);
        let (nx1, ny1) = (((x1 - 1) / 2 + 1).min(nw), ((y1 - 1) / 2 + 1).min(nh));
        if nx0 >= nx1 || ny0 >= ny1 {
            break;
        }
        let (above, below) = chain.split_at_mut(k);
        let src = above.last().map_or(base, |l| l.2.as_slice());
        downsample(src, sw, sh, &mut below[0].2, nw, (nx0, ny0, nx1, ny1), srgb);
        out.push((k as u32 + 1, (nx0, ny0, nx1 - nx0, ny1 - ny0)));
        (x0, y0, x1, y1) = (nx0, ny0, nx1, ny1);
        (sw, sh) = (nw, nh);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_tables_encode_exactly_as_the_formulas() {
        for b in 0..=255u8 {
            assert_eq!(srgb_to_linear(b).to_bits(), srgb_to_linear_exact(b).to_bits(), "byte {b}");
        }
        // Every boundary and its neighbours, then a sweep through every
        // float in [0, 1] at a stride, and the edges.
        let mut probes: Vec<f32> = vec![-1.0, -0.0, 0.0, 1.0, 1.5, f32::NAN, f32::INFINITY, f32::MIN_POSITIVE];
        for t in SRGB_FROM.iter() {
            for d in -3i64..=3 {
                probes.push(f32::from_bits((t.to_bits() as i64 + d).max(0) as u32));
            }
        }
        probes.extend((0..1.0f32.to_bits()).step_by(997).map(f32::from_bits));
        for v in probes {
            assert_eq!(linear_to_srgb(v), linear_to_srgb_exact(v), "linear {v:e} (bits {:#x})", v.to_bits());
        }
    }

    fn noise_image(w: u32, h: u32, seed: u32) -> Vec<u8> {
        let mut s = seed | 1;
        (0..w * h * 4)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                (s >> 8) as u8
            })
            .collect()
    }

    /// Patching a chain in place must give exactly what rebuilding it does -
    /// on odd sizes too, where the last row and column of a level are read
    /// by nothing below it - and must touch only the texels the change
    /// reaches.
    #[test]
    fn a_patched_mip_chain_is_the_rebuilt_one() {
        for (w, h, rect) in [(64, 64, (10, 20, 5, 3)), (37, 23, (30, 0, 7, 23)), (884, 884, (400, 300, 9, 2)), (5, 3, (4, 2, 1, 1)), (16, 1, (0, 0, 3, 1))] {
            for srgb in [true, false] {
                let mut img = noise_image(w, h, w * 31 + h);
                let mut chain = mip_chain(w, h, &img, srgb);
                let (x, y, rw, rh) = rect;
                for yy in y..y + rh {
                    for xx in x..x + rw {
                        let i = ((yy * w + xx) * 4) as usize;
                        img[i] = img[i].wrapping_add(97);
                        img[i + 3] = 255 - img[i + 3];
                    }
                }
                let touched = update_mip_chain(&img, w, h, &mut chain, rect, srgb);
                // (The stale chain differs from the rebuilt one wherever the
                // change reaches a level, so equality is the proof it was
                // patched; a texel on an odd edge reaches none.)
                assert_eq!(chain, mip_chain(w, h, &img, srgb), "{w}x{h} {rect:?} srgb={srgb}");
                // Each level's rectangle is within the level, and small.
                for (level, (lx, ly, lw, lh)) in &touched {
                    let (cw, ch, _) = &chain[*level as usize - 1];
                    assert!(lx + lw <= *cw && ly + lh <= *ch);
                    assert!(*lw <= rw / (1 << level) + 2 && *lh <= rh / (1 << level) + 2, "level {level} patched {lw}x{lh}");
                }
            }
        }
    }

    #[test]
    fn nothing_set_means_nearest_everywhere() {
        let rules = TextureFilterRules::from_settings(Some(&json!({ "width": 1280 })));
        assert!(rules.is_default());
        assert_eq!(rules.filter_for("sprites/hero.png"), TextureFilter::Nearest);
    }

    #[test]
    fn the_longest_prefix_wins() {
        let rules = TextureFilterRules::from_settings(Some(&json!({ "texture_filter": {
            "default": "linear",
            "overrides": { "sprites/": "nearest", "sprites/fx/": "linear" }
        }})));
        assert_eq!(rules.filter_for("backgrounds/sky.png"), TextureFilter::Linear);
        assert_eq!(rules.filter_for("sprites/hero.png"), TextureFilter::Nearest);
        assert_eq!(rules.filter_for("sprites/fx/glow.png"), TextureFilter::Linear);
    }

    #[test]
    fn a_malformed_setting_falls_back_to_nearest() {
        let rules = TextureFilterRules::from_settings(Some(&json!({ "texture_filter": { "default": "blurry" } })));
        assert!(rules.is_default());
    }

    #[test]
    fn mips_halve_down_to_one_texel() {
        let chain = mip_chain(8, 2, &[255; 8 * 2 * 4], true);
        let sizes: Vec<(u32, u32)> = chain.iter().map(|(w, h, _)| (*w, *h)).collect();
        assert_eq!(sizes, vec![(4, 1), (2, 1), (1, 1)]);
    }

    #[test]
    fn a_transparent_texel_does_not_darken_the_average() {
        // One opaque red texel beside three transparent black ones: the level
        // below is red at a quarter alpha, not a quarter-red smudge.
        let mut rgba = vec![0u8; 2 * 2 * 4];
        rgba[..4].copy_from_slice(&[255, 0, 0, 255]);
        let chain = mip_chain(2, 2, &rgba, true);
        let texel = &chain[0].2;
        assert_eq!(texel[0], 255, "{texel:?}");
        assert_eq!(texel[3], 64, "{texel:?}");
    }

    #[test]
    fn colour_is_averaged_in_linear_light() {
        // Black and white average to linear 0.5 - sRGB 188, not 128.
        let rgba = [0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255];
        let chain = mip_chain(2, 2, &rgba, true);
        assert!((chain[0].2[0] as i32 - 188).abs() <= 1, "{}", chain[0].2[0]);
        let data = mip_chain(2, 2, &rgba, false);
        assert!((data[0].2[0] as i32 - 128).abs() <= 1, "data stays data: {}", data[0].2[0]);
    }
}
