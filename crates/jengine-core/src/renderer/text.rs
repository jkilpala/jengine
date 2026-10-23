//! Font data — backend-agnostic font atlas + glyph metadata.
//!
//! The wgpu renderer uploads `built_in_font_atlas()` bytes via
//! `AssetManager::ensure_wgpu_texture` and queries glyph rects through
//! [`FontData`]. No GPU types are touched here.
//!
//! The built-in default font is **Noto Sans Regular** (SIL OFL 1.1), baked
//! via `fontdue` as a **signed distance field** at 48 px (Sep 2026): each
//! texel holds how far it is from the glyph's edge, and the sprite shader
//! draws the edge where that crosses one half, as sharp as the screen's
//! pixels at any size and turn. It was a 16 px coverage bitmap, so a title
//! at four times that was sixteen-pixel glyphs blown up. Its layout metrics
//! are still 16 px ones ([`FontData::atlas_scale`]), so every `font_scale`
//! and `font_size` draws the size it did. See
//! `assets/fonts/LICENSE-NotoSans.txt`.

use serde::{Deserialize, Serialize};

/// Metadata for a single glyph in the font atlas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlyphInfo {
    /// Character this glyph represents.
    pub ch: char,
    /// X pixel offset in the atlas.
    pub x: u32,
    /// Y pixel offset in the atlas.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Horizontal advance (spacing to next glyph) in pixels.
    #[serde(default)]
    pub advance: f32,
    /// Horizontal bearing — how far right of the pen position the glyph
    /// bitmap should be drawn. Defaults to 0 (legacy bitmap-font behavior).
    #[serde(default)]
    pub x_offset: i32,
    /// Vertical offset from the line-top down to the glyph bitmap top, in
    /// pixels. Defaults to 0 (legacy bitmap-font behavior — all glyphs flush
    /// to the line top). Required for TTF-derived fonts so descenders sit
    /// correctly below the baseline.
    #[serde(default)]
    pub y_offset: i32,
}

fn default_pixel_size_one() -> f32 { 1.0 }

/// Font atlas metadata (JSON companion file).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontData {
    /// Atlas image width.
    pub atlas_width: u32,
    /// Atlas image height.
    pub atlas_height: u32,
    /// Line height in pixels (distance between baselines).
    pub line_height: f32,
    /// Native pixel size the atlas was rasterized at. GUI/UI code divides the
    /// caller-specified `font_size` by this to convert pixel-height units
    /// into the multiplicative `scale` consumed by `queue_text_glyphs`.
    /// Defaults to 1.0 to preserve legacy bitmap-font semantics (where the
    /// `scale` parameter was a raw multiplier on glyph dimensions).
    #[serde(default = "default_pixel_size_one")]
    pub pixel_size: f32,
    /// Atlas pixels per layout pixel. Glyph rects and offsets are in atlas
    /// pixels; advances and the line height are in layout pixels - so a font
    /// baked larger than it is laid out (the built-in distance field, 3)
    /// draws the size its metrics say. 1 for a plain bitmap font.
    #[serde(default = "default_atlas_scale")]
    pub atlas_scale: f32,
    /// The atlas is a signed distance field (alpha 0.5 on the edge), drawn
    /// by the sprite shader's distance branch (`AssetManager::mark_sdf`,
    /// which `AssetManager::insert_font_data` calls for a font's sibling
    /// image when this is set).
    #[serde(default)]
    pub sdf: bool,
    /// How far, in atlas pixels, a distance field's glyph rects reach past
    /// the ink on every side ([`SDF_SPREAD`] for every field this engine
    /// bakes). The renderer does not need it - it draws the whole rect - but
    /// a tool measuring where a glyph's ink sits does.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sdf_spread: f32,
    /// Per-glyph information.
    pub glyphs: Vec<GlyphInfo>,
    /// The font file this was baked from, when it is one
    /// (`renderer::dynamic_font`): layout shapes its text through it -
    /// joined Arabic, Devanagari conjuncts, right-to-left order. Never saved.
    #[serde(skip)]
    pub shaping: Option<String>,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn default_atlas_scale() -> f32 {
    1.0
}

/// Pixels of distance either side of a glyph's edge the built-in atlas
/// encodes, in atlas pixels: alpha `0.5 - d / (2 * SDF_SPREAD)`, clamped.
pub const SDF_SPREAD: f32 = 6.0;

/// Squared Euclidean distance transform (Felzenszwalb and Huttenlocher) of
/// `f` in place, `w` x `h`: each cell becomes its squared distance to the
/// nearest cell that was 0.
fn edt(f: &mut [f32], w: usize, h: usize) {
    fn pass(f: &mut [f32], n: usize, stride: usize, start: usize, d: &mut [f32], v: &mut [usize], z: &mut [f32]) {
        let g: Vec<f32> = (0..n).map(|i| f[start + i * stride]).collect();
        let mut k = 0usize;
        v[0] = 0;
        z[0] = f32::NEG_INFINITY;
        z[1] = f32::INFINITY;
        for q in 1..n {
            loop {
                let p = v[k];
                let s = ((g[q] + (q * q) as f32) - (g[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32);
                if s <= z[k] {
                    if k == 0 {
                        v[0] = q;
                        z[0] = f32::NEG_INFINITY;
                        z[1] = f32::INFINITY;
                        break;
                    }
                    k -= 1;
                    continue;
                }
                k += 1;
                v[k] = q;
                z[k] = s;
                z[k + 1] = f32::INFINITY;
                break;
            }
        }
        k = 0;
        for (q, dq) in d.iter_mut().enumerate().take(n) {
            while z[k + 1] < q as f32 {
                k += 1;
            }
            let p = v[k];
            *dq = (q as f32 - p as f32).powi(2) + g[p];
        }
        for i in 0..n {
            f[start + i * stride] = d[i];
        }
    }
    let m = w.max(h);
    let (mut d, mut v, mut z) = (vec![0.0f32; m], vec![0usize; m], vec![0.0f32; m + 1]);
    for x in 0..w {
        pass(f, h, w, x, &mut d, &mut v, &mut z);
    }
    for y in 0..h {
        pass(f, w, 1, y * w, &mut d, &mut v, &mut z);
    }
}

/// A coverage bitmap (`w` x `h`, 0..255) as a signed distance field padded
/// by [`SDF_SPREAD`] on every side: alpha 255 deep inside, 128 on the edge,
/// 0 a spread away outside.
pub fn coverage_to_sdf(coverage: &[u8], w: usize, h: usize) -> (Vec<u8>, usize, usize) {
    let pad = SDF_SPREAD as usize;
    let (pw, ph) = (w + 2 * pad, h + 2 * pad);
    let inside = |x: usize, y: usize| -> f32 {
        if x < pad || y < pad || x >= w + pad || y >= h + pad {
            0.0
        } else {
            coverage[(y - pad) * w + (x - pad)] as f32 / 255.0
        }
    };
    const FAR: f32 = 1e20;
    // Distance to the glyph (from outside) and to the background (from inside).
    let mut to_in = vec![FAR; pw * ph];
    let mut to_out = vec![FAR; pw * ph];
    for y in 0..ph {
        for x in 0..pw {
            if inside(x, y) >= 0.5 {
                to_in[y * pw + x] = 0.0;
            } else {
                to_out[y * pw + x] = 0.0;
            }
        }
    }
    edt(&mut to_in, pw, ph);
    edt(&mut to_out, pw, ph);
    let mut out = vec![0u8; pw * ph];
    for y in 0..ph {
        for x in 0..pw {
            let i = y * pw + x;
            let c = inside(x, y);
            // Negative inside. A partly covered pixel is on the edge: its
            // coverage says how far across it, finer than whole pixels.
            let mut d = to_in[i].sqrt() - to_out[i].sqrt();
            if c > 0.0 && c < 1.0 && d.abs() <= 1.5 {
                d = 0.5 - c;
            }
            out[i] = ((0.5 - d / (2.0 * SDF_SPREAD)).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    (out, pw, ph)
}

/// A bitmap font as a distance field (Oct 2026): what `jengine font-sdf`
/// does to a `.font` and its atlas. Each glyph's coverage - the atlas alpha
/// inside its rect - goes through [`coverage_to_sdf`], the glyphs are packed
/// again, and each rect grows by [`SDF_SPREAD`] on every side with its
/// offsets moved out by as much, so the ink lands exactly where it did.
/// Advances, the line height, the pixel size and the atlas scale are
/// untouched: nothing that lays text out moves, and only the drawing
/// changes - sharp at any size or turn, where the bitmap blurred scaled up.
///
/// Until this a custom `.font` could not be a distance field: `"sdf": true`
/// parsed, and nothing could make an atlas to go with it. `rgba` is the
/// atlas, `width` x `height`, as `data` describes it.
///
/// `upscale` bakes the field that many texels per source pixel, each
/// glyph's coverage resampled bilinearly first, and multiplies the atlas
/// scale by it (rects and offsets are atlas pixels; advances and the line
/// height layout ones, so the text still lays out the same). A field one
/// texel per pixel cannot hold a counter a pixel or two wide - a 16 px
/// face's 'A' and 'R' fill in - which is why the built-in face is baked at
/// three times its size; [`sdf_upscale_for`] picks the same for any font.
pub fn font_to_sdf(data: &FontData, rgba: &[u8], width: u32, height: u32, upscale: u32) -> anyhow::Result<(FontData, Vec<u8>, u32, u32)> {
    let k = upscale.clamp(1, 8);
    if data.sdf {
        anyhow::bail!("the font is already a distance field");
    }
    if rgba.len() != (width * height * 4) as usize {
        anyhow::bail!("the atlas is {} bytes, not {width} x {height} RGBA", rgba.len());
    }
    let pad = SDF_SPREAD as u32;
    // Each glyph's field, from its coverage.
    let mut fields: Vec<(Vec<u8>, u32, u32)> = Vec::with_capacity(data.glyphs.len());
    for g in &data.glyphs {
        if g.width == 0 || g.height == 0 {
            fields.push((Vec::new(), 0, 0));
            continue;
        }
        if g.x + g.width > width || g.y + g.height > height {
            anyhow::bail!("glyph {:?} at {},{} {}x{} is outside the {width} x {height} atlas", g.ch, g.x, g.y, g.width, g.height);
        }
        let src = |col: i32, row: i32| -> f32 {
            let (c, r) = (col.clamp(0, g.width as i32 - 1) as u32, row.clamp(0, g.height as i32 - 1) as u32);
            rgba[(((g.y + r) * width + g.x + c) * 4 + 3) as usize] as f32
        };
        let (cw, ch) = (g.width * k, g.height * k);
        let mut coverage = Vec::with_capacity((cw * ch) as usize);
        for row in 0..ch {
            for col in 0..cw {
                // Bilinear, at this texel's centre in source pixels.
                let u = (col as f32 + 0.5) / k as f32 - 0.5;
                let v = (row as f32 + 0.5) / k as f32 - 0.5;
                let (u0, v0) = (u.floor(), v.floor());
                let (fu, fv) = (u - u0, v - v0);
                let (u0, v0) = (u0 as i32, v0 as i32);
                let top = src(u0, v0) * (1.0 - fu) + src(u0 + 1, v0) * fu;
                let bottom = src(u0, v0 + 1) * (1.0 - fu) + src(u0 + 1, v0 + 1) * fu;
                coverage.push((top * (1.0 - fv) + bottom * fv).round() as u8);
            }
        }
        let (field, fw, fh) = coverage_to_sdf(&coverage, cw as usize, ch as usize);
        fields.push((field, fw as u32, fh as u32));
    }

    // Shelf-packed, tallest first, a texel apart: a field's edge reads its
    // neighbours when filtered.
    const GAP: u32 = 1;
    // As wide as the source scaled, within what every GPU this runs on can
    // hold (2048, WebGL2's floor), unless one glyph needs more.
    let atlas_w = (width * k).clamp(256, 2048).max(fields.iter().map(|f| f.1 + 2 * GAP).max().unwrap_or(0));
    let mut order: Vec<usize> = (0..fields.len()).filter(|&i| fields[i].1 > 0).collect();
    order.sort_by(|&a, &b| fields[b].2.cmp(&fields[a].2));
    let mut at = vec![(0u32, 0u32); fields.len()];
    let (mut x, mut y, mut shelf) = (GAP, GAP, 0u32);
    for &i in &order {
        let (_, w, h) = fields[i];
        if x + w + GAP > atlas_w {
            x = GAP;
            y += shelf + GAP;
            shelf = 0;
        }
        at[i] = (x, y);
        x += w + GAP;
        shelf = shelf.max(h);
    }
    let atlas_h = (y + shelf + GAP).div_ceil(4).max(4) * 4;

    let mut pixels = vec![0u8; (atlas_w * atlas_h * 4) as usize];
    let mut glyphs = Vec::with_capacity(data.glyphs.len());
    for (i, g) in data.glyphs.iter().enumerate() {
        let (field, w, h) = &fields[i];
        let (px, py) = at[i];
        for row in 0..*h {
            for col in 0..*w {
                let d = (((py + row) * atlas_w + px + col) * 4) as usize;
                pixels[d..d + 3].fill(0xFF);
                pixels[d + 3] = field[(row * w + col) as usize];
            }
        }
        let grown = *w > 0;
        glyphs.push(GlyphInfo {
            ch: g.ch,
            x: if grown { px } else { 0 },
            y: if grown { py } else { 0 },
            width: *w,
            height: *h,
            advance: g.advance,
            x_offset: g.x_offset * k as i32 - if grown { pad as i32 } else { 0 },
            y_offset: g.y_offset * k as i32 - if grown { pad as i32 } else { 0 },
        });
    }
    let out = FontData {
        atlas_width: atlas_w,
        atlas_height: atlas_h,
        line_height: data.line_height,
        pixel_size: data.pixel_size,
        atlas_scale: data.atlas_scale * k as f32,
        sdf: true,
        sdf_spread: SDF_SPREAD,
        glyphs,
        shaping: None,
    };
    Ok((out, pixels, atlas_w, atlas_h))
}

/// How many field texels per source pixel [`font_to_sdf`] should bake
/// `data` at: enough for a 48 px field, as the built-in face is baked, and
/// never more than four. A font's size is its `pixel_size`, or its line
/// height for a legacy font that left `pixel_size` at 1.
pub fn sdf_upscale_for(data: &FontData) -> u32 {
    let size = if data.pixel_size > 1.0 { data.pixel_size } else { data.line_height } * data.atlas_scale;
    ((48.0 / size.max(1.0)).ceil() as u32).clamp(1, 4)
}

/// Backend-agnostic raw atlas + metadata for the built-in default UI font.
///
/// Rasterizes **Noto Sans Regular** (SIL OFL 1.1, embedded via
/// `include_bytes!`) into a single greyscale-as-alpha atlas via `fontdue`,
/// then shelf-packs glyphs U+0020..=U+007E into a 512×N RGBA8 texture.
///
/// Returns `(rgba, atlas_width, atlas_height, font_data)`. Pure CPU work —
/// no GPU types touched. Called once at startup by the wgpu runtime path
/// which uploads the bytes via `AssetManager::ensure_wgpu_texture`.
pub fn built_in_font_atlas() -> (Vec<u8>, u32, u32, FontData) {
    use fontdue::{Font, FontSettings};

    // ASCII subset (U+0020..U+007E) of Noto Sans Regular - exactly the range
    // rasterized below. The full face is 607 KB and shipped in every binary
    // and in the wasm, where it was a quarter of the compressed payload; the
    // subset is 8 KB. Regenerate with
    // `assets/fonts/subset_builtin_font.py` if the range below changes.
    const TTF_BYTES: &[u8] =
        include_bytes!("../../assets/fonts/NotoSans-Regular-ascii.ttf");
    /// The size every layout metric is in: what `font_scale` 1 and a UI
    /// `font_size` of 16 draw.
    const PX_SIZE: f32 = 16.0;
    /// The size the distance field is baked at - three texels per layout
    /// pixel, so a glyph's corners stay sharp scaled up.
    const SDF_PX: f32 = 48.0;
    const ATLAS_SCALE: f32 = SDF_PX / PX_SIZE;
    const ATLAS_W: u32 = 1024;
    const PADDING: u32 = 1;

    let font = Font::from_bytes(TTF_BYTES, FontSettings::default())
        .expect("built-in font: NotoSans-Regular.ttf failed to parse");

    let line_metrics = font
        .horizontal_line_metrics(SDF_PX)
        .expect("built-in font: missing horizontal line metrics");
    let ascent = line_metrics.ascent;
    // Layout pixels, as the advances are.
    let line_height = line_metrics.new_line_size / ATLAS_SCALE;
    let pad = SDF_SPREAD as i32;

    struct Baked {
        ch: char,
        bitmap: Vec<u8>,
        w: u32,
        h: u32,
        xmin: i32,
        ymin: i32,
        advance: f32,
    }

    // Must match RANGES in `assets/fonts/subset_builtin_font.py`: a glyph
    // rasterized here but absent from the subset comes out blank, and one in
    // the subset but not rasterized is dead weight in every binary.
    const RANGES: &[(u32, u32)] = &[
        (0x20, 0x7E),   // ASCII
        (0xA0, 0xFF),   // Latin-1 Supplement - accents, guillemets, currency
        (0x100, 0x17F), // Latin Extended-A - Polish, Czech, Hungarian, Turkish
    ];
    let codepoints: Vec<char> = RANGES
        .iter()
        .flat_map(|(first, last)| *first..=*last)
        .filter_map(char::from_u32)
        .collect();

    let mut baked: Vec<Baked> = Vec::with_capacity(codepoints.len());
    for ch in codepoints {
        let (m, coverage) = font.rasterize(ch, SDF_PX);
        // A space has no bitmap and needs no field.
        let (bitmap, w, h) = if m.width > 0 && m.height > 0 {
            let (b, w, h) = coverage_to_sdf(&coverage, m.width, m.height);
            (b, w as u32, h as u32)
        } else {
            (Vec::new(), 0, 0)
        };
        baked.push(Baked {
            ch,
            bitmap,
            w,
            h,
            // The padded field starts a spread left of the bitmap and
            // reaches a spread below it.
            xmin: m.xmin - if w > 0 { pad } else { 0 },
            ymin: m.ymin - if w > 0 { pad } else { 0 },
            advance: m.advance_width / ATLAS_SCALE,
        });
    }

    // Shelf packing — sort tallest first to minimize wasted vertical space.
    let mut order: Vec<usize> = (0..baked.len()).collect();
    order.sort_by(|&a, &b| baked[b].h.cmp(&baked[a].h));

    let mut positions: Vec<(u32, u32)> = vec![(0, 0); baked.len()];
    let mut shelf_x: u32 = PADDING;
    let mut shelf_y: u32 = PADDING;
    let mut shelf_h: u32 = 0;

    for &idx in &order {
        let bw = baked[idx].w.max(1);
        let bh = baked[idx].h.max(1);
        if shelf_x + bw + PADDING > ATLAS_W {
            shelf_y += shelf_h + PADDING;
            shelf_x = PADDING;
            shelf_h = 0;
        }
        positions[idx] = (shelf_x, shelf_y);
        shelf_x += bw + PADDING;
        if bh > shelf_h {
            shelf_h = bh;
        }
    }
    let used_h = shelf_y + shelf_h + PADDING;
    // Rounded to four rows, not to a power of two: no GPU this runs on
    // needs one, and the distance field's atlas would be twice its size.
    let atlas_h = used_h.div_ceil(4).max(16) * 4;

    let mut pixels = vec![0u8; (ATLAS_W * atlas_h * 4) as usize];

    let mut glyphs: Vec<GlyphInfo> = Vec::with_capacity(baked.len());
    for (i, b) in baked.iter().enumerate() {
        let (px, py) = positions[i];
        if b.w > 0 && b.h > 0 {
            for row in 0..b.h {
                for col in 0..b.w {
                    let src = (row * b.w + col) as usize;
                    let alpha = b.bitmap[src];
                    let dx = px + col;
                    let dy = py + row;
                    let dst = ((dy * ATLAS_W + dx) * 4) as usize;
                    pixels[dst] = 0xFF;
                    pixels[dst + 1] = 0xFF;
                    pixels[dst + 2] = 0xFF;
                    pixels[dst + 3] = alpha;
                }
            }
        }
        // y_offset = distance from line-top down to glyph bitmap top.
        //   glyph_top_from_baseline = ymin + h   (positive = above baseline)
        //   line_top_from_baseline  = ascent    (positive = above baseline)
        //   y_offset (line_top → glyph_top, downward positive)
        //     = ascent - (ymin + h)
        let y_offset = (ascent - (b.ymin as f32 + b.h as f32)).round() as i32;
        glyphs.push(GlyphInfo {
            ch: b.ch,
            x: px,
            y: py,
            width: b.w,
            height: b.h,
            advance: b.advance,
            x_offset: b.xmin,
            y_offset,
        });
    }

    let data = FontData {
        atlas_width: ATLAS_W,
        atlas_height: atlas_h,
        line_height,
        pixel_size: PX_SIZE,
        atlas_scale: ATLAS_SCALE,
        sdf: true,
        sdf_spread: SDF_SPREAD,
        glyphs,
        shaping: None,
    };

    (pixels, ATLAS_W, atlas_h, data)
}
