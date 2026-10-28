//! Fonts straight from TTF / OTF files (Oct 2026; Godot's DynamicFont,
//! TextMeshPro's dynamic atlas): any script a font file covers - Cyrillic,
//! Greek, Japanese, Chinese, Korean - with no atlas baked beforehand.
//!
//! A `font_id` naming a `.ttf` or `.otf` is loaded by each host's font loader
//! as bytes and handed to [`insert_font_file`], which parses it (`fontdue`),
//! bakes ASCII and makes an empty distance-field atlas. Every other glyph is
//! baked the first time a text asks for it: [`sync`] - which rides on
//! `carve::apply_carves`, the per-frame step every host makes with its assets
//! - collects the characters the world's texts use that the atlas has not
//! got, rasterizes each as a distance field with the built-in face's recipe
//! (48 px, laid out at 16, [`SDF_SPREAD`]), shelf-packs it and marks just
//! that rectangle dirty, so the runtime-texture path uploads it with no host
//! wiring. The atlas grows downward when a shelf no longer fits, up to
//! [`MAX_HEIGHT`].
//!
//! A text drawn in the frame a glyph is first asked for may show the font's
//! `?` for that one frame.
//!
//! # Shaping and direction (Oct 2026)
//!
//! A line with letters that change by their neighbours or run right to left
//! - Arabic, Hebrew, the Indic scripts, Thai - is **shaped**
//! ([`display_text`]): split into runs of one direction by the Unicode
//! bidirectional algorithm (`unicode-bidi`), each run shaped by `rustybuzz`
//! (HarfBuzz's rules: joining forms, conjuncts, reordered vowel signs, and
//! GPOS placing each mark on its base), and the glyphs written back as
//! **private-use characters** in visual order: each distinct [`ShapedGlyph`]
//! - glyph, advance and offset together - is given the next character from
//! U+F0000, which this module bakes by glyph index with the offset folded
//! into its bearings and the shaped advance as its own. So the layout and
//! the renderer, which place characters left to right, draw shaped text
//! unchanged: `FontData::shaping` names the font file, and `text_layout`
//! swaps in the display string. Lines without such letters are left alone,
//! at no cost. What is lost: wrapping inside a shaped line breaks only at a
//! space, rich-text styles land on glyphs rather than letters, and a text
//! field's caret in shaped text is approximate. Arabic needs the offsets:
//! Noto draws its dots as separate marks.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use crate::asset::{AssetManager, RawTextureData};
use crate::renderer::text::{FontData, GlyphInfo, SDF_SPREAD, coverage_to_sdf};

/// The size every layout metric is in, as the built-in face's.
const LAYOUT_PX: f32 = 16.0;
/// The size the field is baked at.
const SDF_PX: f32 = 48.0;
const ATLAS_SCALE: f32 = SDF_PX / LAYOUT_PX;
pub const ATLAS_WIDTH: u32 = 1024;
const START_HEIGHT: u32 = 256;
pub const MAX_HEIGHT: u32 = 4096;
const PAD: u32 = 1;
/// Where shaped glyphs live: the Supplementary Private Use Area-A, which
/// no font's own characters use. Each distinct shaped glyph - its index,
/// advance and offsets together - is given the next character from here.
pub const GLYPH_BASE: u32 = 0xF0000;
const GLYPH_END: u32 = 0xFFFFD;

/// Whether `c` is a letter that needs shaping or runs right to left:
/// Hebrew, Arabic (and its supplements and presentation forms), Syriac,
/// Thaana, N'Ko, the Indic scripts, Thai, Lao and Myanmar.
pub fn needs_shaping(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0x0900..=0x0DFF | 0x0E00..=0x0EFF | 0x1000..=0x109F | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

pub use crate::renderer::font_asset::is_font_file;

/// A loaded font file and where its atlas has room.
struct Live {
    font: fontdue::Font,
    /// The file, for shaping (`rustybuzz` reads it as it shapes).
    bytes: Arc<Vec<u8>>,
    ascent: f32,
    /// The next free spot on the current shelf, and the shelf's height.
    x: u32,
    y: u32,
    shelf: u32,
    /// Every character tried, found or not, so a missing one is asked once.
    tried: HashSet<char>,
    /// Shaped glyphs by their display character, and back.
    variants: HashMap<char, ShapedGlyph>,
    variant_of: HashMap<ShapedGlyph, char>,
    /// The font's units per em, for turning shaping's positions into pixels.
    upem: f32,
}

/// One glyph as shaping placed it: its index, how far it moves the pen, and
/// how far it is pushed from where the pen is (a mark above its base) - in
/// font units, x right and y up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapedGlyph {
    pub glyph: u16,
    pub advance: i32,
    pub dx: i32,
    pub dy: i32,
}

thread_local! {
    static LIVE: RefCell<HashMap<String, Live>> = RefCell::new(HashMap::new());
    /// Shaped display strings by (font, text): texts are drawn every frame.
    static SHAPED: RefCell<HashMap<(String, String), Arc<str>>> = RefCell::new(HashMap::new());
}

/// What `text` draws as in the font file `font_id`: itself when no line
/// needs shaping, else each such line shaped and in visual order, each glyph
/// a display character standing for its [`ShapedGlyph`] (see the module
/// docs). Cached.
pub fn display_text<'a>(font_id: &str, text: &'a str) -> Cow<'a, str> {
    if !text.chars().any(needs_shaping) {
        return Cow::Borrowed(text);
    }
    let key = (font_id.to_string(), text.to_string());
    if let Some(s) = SHAPED.with(|c| c.borrow().get(&key).cloned()) {
        return Cow::Owned(s.to_string());
    }
    let Some(bytes) = LIVE.with(|l| l.borrow().get(font_id).map(|live| Arc::clone(&live.bytes))) else {
        return Cow::Borrowed(text);
    };
    let lines = shape(&bytes, text);
    let shown: Arc<str> = LIVE.with(|l| {
        let mut l = l.borrow_mut();
        let Some(live) = l.get_mut(font_id) else { return text.into() };
        let mut out = String::with_capacity(text.len() * 2);
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            match line {
                Line::Plain(t) => out.push_str(t),
                Line::Shaped(glyphs) => {
                    for g in glyphs {
                        match g {
                            None => out.push(' '),
                            Some(g) => out.push(variant(live, *g)),
                        }
                    }
                }
            }
        }
        out.into()
    });
    SHAPED.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= 1024 {
            c.clear();
        }
        c.insert(key, Arc::clone(&shown));
    });
    Cow::Owned(shown.to_string())
}

/// The display character for `g`, given one the first time it is seen.
fn variant(live: &mut Live, g: ShapedGlyph) -> char {
    if let Some(c) = live.variant_of.get(&g) {
        return *c;
    }
    let n = GLYPH_BASE + live.variants.len() as u32;
    let Some(c) = char::from_u32(n).filter(|_| n <= GLYPH_END) else { return '?' };
    live.variants.insert(c, g);
    live.variant_of.insert(g, c);
    c
}

/// A line of text as [`shape`] leaves it.
#[derive(Debug, Clone, PartialEq)]
pub enum Line {
    /// Nothing in it needs shaping: drawn as it is.
    Plain(String),
    /// Its glyphs in visual order, a space as `None` (so wrapping still
    /// breaks there).
    Shaped(Vec<Option<ShapedGlyph>>),
}

/// `text` shaped line by line with `rustybuzz`, runs in visual order by
/// `unicode-bidi`.
pub fn shape(bytes: &[u8], text: &str) -> Vec<Line> {
    let Some(face) = rustybuzz::Face::from_slice(bytes, 0) else {
        return text.split('\n').map(|l| Line::Plain(l.to_string())).collect();
    };
    text.split('\n')
        .map(|line| {
            if !line.chars().any(needs_shaping) {
                return Line::Plain(line.to_string());
            }
            let mut out = Vec::new();
            let bidi = unicode_bidi::BidiInfo::new(line, None);
            for para in &bidi.paragraphs {
                let (levels, runs) = bidi.visual_runs(para, para.range.clone());
                for run in runs {
                    let rtl = levels[run.start].is_rtl();
                    let piece = &line[run.clone()];
                    let mut buf = rustybuzz::UnicodeBuffer::new();
                    buf.push_str(piece);
                    buf.set_direction(if rtl { rustybuzz::Direction::RightToLeft } else { rustybuzz::Direction::LeftToRight });
                    buf.guess_segment_properties();
                    let shaped = rustybuzz::shape(&face, &[], buf);
                    // HarfBuzz hands glyphs over in visual order, a right to
                    // left run already reversed.
                    for (info, pos) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
                        if piece[info.cluster as usize..].starts_with(' ') {
                            out.push(None);
                        } else {
                            out.push(Some(ShapedGlyph {
                                glyph: info.glyph_id as u16,
                                advance: pos.x_advance,
                                dx: pos.x_offset,
                                dy: pos.y_offset,
                            }));
                        }
                    }
                }
            }
            Line::Shaped(out)
        })
        .collect()
}

/// Load a TTF / OTF from its bytes under `font_id`: its atlas (at
/// `font_texture_id(font_id)`), its `FontData`, and ASCII baked at once.
pub fn insert_font_file(assets: &mut AssetManager, font_id: &str, bytes: &[u8]) -> Result<(), String> {
    let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()).map_err(|e| e.to_string())?;
    let metrics = font.horizontal_line_metrics(SDF_PX).ok_or("the font has no horizontal metrics")?;
    let texture = crate::renderer::font_asset::font_texture_id(font_id);
    assets.insert_texture_bytes(
        texture,
        RawTextureData { width: ATLAS_WIDTH, height: START_HEIGHT, rgba: blank(ATLAS_WIDTH, START_HEIGHT) },
    );
    assets.insert_font_data(
        font_id.to_string(),
        FontData {
            atlas_width: ATLAS_WIDTH,
            atlas_height: START_HEIGHT,
            line_height: metrics.new_line_size / ATLAS_SCALE,
            pixel_size: LAYOUT_PX,
            atlas_scale: ATLAS_SCALE,
            sdf: true,
            sdf_spread: SDF_SPREAD,
            glyphs: Vec::new(),
            shaping: Some(font_id.to_string()),
        },
    );
    LIVE.with(|l| {
        l.borrow_mut().insert(
            font_id.to_string(),
            Live {
                font,
                bytes: Arc::new(bytes.to_vec()),
                ascent: metrics.ascent,
                x: PAD,
                y: PAD,
                shelf: 0,
                tried: HashSet::new(),
                variants: HashMap::new(),
                variant_of: HashMap::new(),
                upem: rustybuzz::Face::from_slice(bytes, 0).map_or(1000.0, |f| f.units_per_em() as f32),
            },
        )
    });
    SHAPED.with(|c| c.borrow_mut().retain(|(f, _), _| f != font_id));
    add_glyphs(assets, font_id, (' '..='~').collect());
    Ok(())
}

/// Forget every loaded font file - a host replacing its `AssetManager`.
pub fn clear() {
    LIVE.with(|l| l.borrow_mut().clear());
    SHAPED.with(|c| c.borrow_mut().clear());
}

fn blank(w: u32, h: u32) -> Vec<u8> {
    // White with no alpha, so a bilinear sample at a glyph's edge never
    // drags in black.
    [255u8, 255, 255, 0].repeat((w * h) as usize)
}

/// Bake every character the world's texts ask a loaded font file for and it
/// has not got yet. See the module docs.
pub fn sync(world: &hecs::World, assets: &mut AssetManager) {
    if LIVE.with(|l| l.borrow().is_empty()) {
        return;
    }
    let mut wanted: HashMap<String, BTreeSet<char>> = HashMap::new();
    let mut want = |font_id: &str, text: &str| {
        if !is_font_file(font_id) {
            return;
        }
        // What is drawn: the shaped glyphs, for a text that needs them.
        let shown = display_text(font_id, text);
        let text: &str = &shown;
        let fresh: Vec<char> = LIVE.with(|l| {
            l.borrow()
                .get(font_id)
                .map(|live| text.chars().filter(|c| !c.is_control() && !live.tried.contains(c)).collect())
                .unwrap_or_default()
        });
        if !fresh.is_empty() {
            wanted.entry(font_id.to_string()).or_default().extend(fresh);
        }
    };
    for (_, t) in world.query::<&crate::ecs::components::TextComponent>().iter() {
        want(&t.font_id, &t.text);
    }
    for (_, t) in world.query::<&crate::ui::UIText>().iter() {
        want(&t.font_id, &t.text);
    }
    // `.gui` widgets: the documents in the cache are the live ones -
    // `gui_set` writes into them - so their text is this frame's.
    let mut gui_text: Vec<(String, String)> = Vec::new();
    for (_, g) in world.query::<&crate::ecs::components::GuiComponent>().iter() {
        if let Some(doc) = assets.get_gui(&g.gui_id) {
            collect_gui_text(&doc.root, &mut gui_text);
        }
    }
    for (font_id, text) in &gui_text {
        want(font_id, text);
    }
    for (font_id, chars) in wanted {
        add_glyphs(assets, &font_id, chars.into_iter().collect());
    }
}

/// Every `(font_id, text)` a `.gui` document's text-bearing widgets draw:
/// a `Text`'s text, a `Button`'s label, a `TextInput`'s text and placeholder.
fn collect_gui_text(node: &crate::ui::gui_document::GuiNode, out: &mut Vec<(String, String)>) {
    use crate::ui::gui_document::GuiNodeType;
    match &node.node_type {
        GuiNodeType::Text { font_id, text, .. } => out.push((font_id.clone(), text.clone())),
        GuiNodeType::Button { font_id, label, .. } => out.push((font_id.clone(), label.clone())),
        GuiNodeType::TextInput { font_id, text, placeholder, .. } => {
            out.push((font_id.clone(), format!("{text}{placeholder}")));
        }
        _ => {}
    }
    for child in &node.children {
        collect_gui_text(child, out);
    }
}

/// Bake `chars` into `font_id`'s atlas, growing it if a shelf will not fit.
fn add_glyphs(assets: &mut AssetManager, font_id: &str, chars: Vec<char>) {
    let texture = crate::renderer::font_asset::font_texture_id(font_id);
    let Some(shared) = assets.get_font_shared(font_id) else { return };
    let mut data = (*shared).clone();
    let mut dirty: Option<(u32, u32, u32, u32)> = None;
    let mut grown = false;
    LIVE.with(|l| {
        let mut l = l.borrow_mut();
        let Some(live) = l.get_mut(font_id) else { return };
        for ch in chars {
            if !live.tried.insert(ch) {
                continue;
            }
            // A shaped glyph is baked by its index, moved by its offsets and
            // advancing as shaping says (`ShapedGlyph`).
            let shaped = live.variants.get(&ch).copied();
            if let Some(g) = shaped {
                if g.glyph >= live.font.glyph_count() {
                    continue;
                }
            } else if ch != ' ' && live.font.lookup_glyph_index(ch) == 0 {
                continue; // not in the font: layout draws its `?`
            }
            let (mut m, coverage) = match shaped {
                Some(g) => live.font.rasterize_indexed(g.glyph, SDF_PX),
                None => live.font.rasterize(ch, SDF_PX),
            };
            // Font units to the bake's pixels.
            if let Some(g) = shaped {
                let unit = SDF_PX / live.upem.max(1.0);
                m.advance_width = g.advance as f32 * unit;
                m.xmin += (g.dx as f32 * unit).round() as i32;
                m.ymin += (g.dy as f32 * unit).round() as i32;
            }
            let (bitmap, w, h) = if m.width > 0 && m.height > 0 {
                let (b, w, h) = coverage_to_sdf(&coverage, m.width, m.height);
                (b, w as u32, h as u32)
            } else {
                (Vec::new(), 0, 0)
            };
            let pad = SDF_SPREAD as i32;
            let (xmin, ymin) = (m.xmin - if w > 0 { pad } else { 0 }, m.ymin - if w > 0 { pad } else { 0 });
            // Shelf-pack: next shelf when the row is full, a taller atlas
            // when the shelves are.
            let (bw, bh) = (w.max(1), h.max(1));
            if live.x + bw + PAD > ATLAS_WIDTH {
                live.y += live.shelf + PAD;
                live.x = PAD;
                live.shelf = 0;
            }
            while live.y + bh + PAD > data.atlas_height {
                if data.atlas_height >= MAX_HEIGHT {
                    log::warn!("font '{font_id}': its atlas is full at {MAX_HEIGHT} rows; '{ch}' is not drawn");
                    return;
                }
                let new_h = (data.atlas_height * 2).min(MAX_HEIGHT);
                if let Some(raw) = assets.texture_bytes_mut(&texture) {
                    raw.rgba.extend(blank(ATLAS_WIDTH, new_h - raw.height));
                    raw.height = new_h;
                }
                data.atlas_height = new_h;
                grown = true;
            }
            let (px, py) = (live.x, live.y);
            if w > 0
                && let Some(raw) = assets.texture_bytes_mut(&texture)
            {
                for row in 0..h {
                    for col in 0..w {
                        let dst = (((py + row) * raw.width + px + col) * 4) as usize;
                        raw.rgba[dst..dst + 4].copy_from_slice(&[255, 255, 255, bitmap[(row * w + col) as usize]]);
                    }
                }
                let r = dirty.get_or_insert((px, py, px + w, py + h));
                *r = (r.0.min(px), r.1.min(py), r.2.max(px + w), r.3.max(py + h));
            }
            live.x += bw + PAD;
            live.shelf = live.shelf.max(bh);
            data.glyphs.push(GlyphInfo {
                ch,
                x: px,
                y: py,
                width: w,
                height: h,
                advance: m.advance_width / ATLAS_SCALE,
                x_offset: xmin,
                // Line top down to the bitmap's top, as the built-in face.
                y_offset: (live.ascent - (ymin as f32 + h as f32)).round() as i32,
            });
        }
    });
    if let Some((x0, y0, x1, y1)) = dirty {
        assets.mark_texture_dirty(&texture, x0, y0, x1 - x0, y1 - y0);
    }
    if grown {
        // A taller texture is a new upload; `ensure_wgpu_texture` sees the
        // size change and makes one.
        assets.mark_texture_dirty(&texture, 0, 0, ATLAS_WIDTH, data.atlas_height);
    }
    assets.insert_font_data(font_id.to_string(), data);
}

/// Characters baked into a loaded font file so far. For tests.
pub fn baked(assets: &AssetManager, font_id: &str) -> BTreeSet<char> {
    assets.get_font_data(font_id).map(|d| d.glyphs.iter().map(|g| g.ch).collect()).unwrap_or_default()
}
