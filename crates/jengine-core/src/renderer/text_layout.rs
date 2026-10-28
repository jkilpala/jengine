//! Line breaking and measurement for bitmap text.
//!
//! Text rendering was a single advance loop that dropped `\n` outright, so
//! multi-line output meant stacking one entity per line by hand — which does
//! not survive a translation, a font change, or a string whose length you do
//! not control. This is the layout pass that was missing: it splits on
//! explicit newlines, optionally wraps to a width, and measures, so every
//! text call site gets the same answer.
//!
//! Measurement uses the same advances the renderer draws with, so a string
//! that measures 100 units wide is drawn 100 units wide. Anything that
//! measures text with a different rule will disagree with the screen sooner
//! or later.

use crate::renderer::text::FontData;

/// One laid-out line: the slice of the source it covers, and its width in
/// the same units the caller's `scale` produces.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidOutLine {
    pub text: String,
    pub width: f32,
}

/// Advance width of `text` in scaled units, using the renderer's own
/// per-glyph advances and its `?` fallback for missing glyphs.
pub fn measure_line(font: &FontData, text: &str, scale: f32) -> f32 {
    let shaped = display_for(font, text);
    shaped
        .chars()
        .filter(|c| *c != '\n')
        .filter_map(|ch| {
            font.glyphs
                .iter()
                .find(|g| g.ch == ch)
                .or_else(|| font.glyphs.iter().find(|g| g.ch == '?'))
        })
        .map(|g| g.advance * scale)
        .sum()
}

/// What `text` draws as in `font`: the text itself, or - for a font file
/// with letters that need it - shaped and in visual order
/// (`dynamic_font::display_text`). Shaping a display string again leaves it.
pub fn display_for<'a>(font: &FontData, text: &'a str) -> std::borrow::Cow<'a, str> {
    match &font.shaping {
        #[cfg(feature = "dynamic_font")]
        Some(id) => crate::renderer::dynamic_font::display_text(id, text),
        _ => std::borrow::Cow::Borrowed(text),
    }
}

/// Break `text` into lines.
///
/// Explicit `\n` always breaks. When `max_width` is `Some` and positive,
/// lines are additionally wrapped on whitespace; a single word longer than
/// the limit is broken mid-word rather than overflowing silently, because a
/// URL or a German compound would otherwise run off the screen with no
/// indication anything went wrong.
pub fn layout_text(
    font: &FontData,
    text: &str,
    scale: f32,
    max_width: Option<f32>,
) -> Vec<LaidOutLine> {
    let shaped = display_for(font, text);
    let text: &str = &shaped;
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        match max_width {
            Some(limit) if limit > 0.0 => wrap_paragraph(font, paragraph, scale, limit, &mut out),
            _ => out.push(LaidOutLine {
                width: measure_line(font, paragraph, scale),
                text: paragraph.to_string(),
            }),
        }
    }
    // `"a\n"` is two lines, the second empty — dropping it would move
    // anything positioned below the text.
    if out.is_empty() {
        out.push(LaidOutLine {
            text: String::new(),
            width: 0.0,
        });
    }
    out
}

fn wrap_paragraph(
    font: &FontData,
    paragraph: &str,
    scale: f32,
    limit: f32,
    out: &mut Vec<LaidOutLine>,
) {
    let mut line = String::new();
    let mut line_width = 0.0f32;

    for word in paragraph.split_inclusive(' ') {
        let word_width = measure_line(font, word, scale);

        // Trailing spaces do not justify a break: a line ending in a space
        // that only overflows because of that space should stay put.
        let trimmed_width = measure_line(font, word.trim_end(), scale);

        if !line.is_empty() && line_width + trimmed_width > limit {
            out.push(LaidOutLine {
                text: line.trim_end().to_string(),
                width: measure_line(font, line.trim_end(), scale),
            });
            line = String::new();
            line_width = 0.0;
        }

        // A single word wider than the whole line: break it rather than
        // overflow. Splitting per character is crude but predictable.
        if line.is_empty() && trimmed_width > limit {
            let mut chunk = String::new();
            let mut chunk_width = 0.0f32;
            for ch in word.chars() {
                let ch_width = measure_line(font, &ch.to_string(), scale);
                if !chunk.is_empty() && chunk_width + ch_width > limit {
                    out.push(LaidOutLine {
                        width: chunk_width,
                        text: std::mem::take(&mut chunk),
                    });
                    chunk_width = 0.0;
                }
                chunk.push(ch);
                chunk_width += ch_width;
            }
            line = chunk;
            line_width = chunk_width;
            continue;
        }

        line.push_str(word);
        line_width += word_width;
    }

    out.push(LaidOutLine {
        text: line.trim_end().to_string(),
        width: measure_line(font, line.trim_end(), scale),
    });
}

/// Total size of a laid-out block: widest line by `line_height * count`.
pub fn measure_block(lines: &[LaidOutLine], line_height: f32) -> (f32, f32) {
    let width = lines.iter().map(|l| l.width).fold(0.0f32, f32::max);
    (width, line_height * lines.len() as f32)
}

/// World-space bounding box of a piece of rendered text.
///
/// Centre plus size, because that is what hit-testing and gizmo code want; the
/// renderer itself works from the top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextBounds {
    pub center_x: f32,
    pub center_y: f32,
    pub width: f32,
    pub height: f32,
}

/// Measure where a `TextComponent` actually lands in the world.
///
/// **`position` is the top-left of the first line, not the centre.** Glyphs
/// advance rightward and successive lines sit one `line_height` *lower*, so a
/// caller that assumes a centred box is wrong by half the width and half the
/// height — which is why clicking a long label in the editor viewport used to
/// select nothing. Text **can** be turned since Sep 2026
/// ([`glyph_quads`]), but this box is still the unrotated one: a hit test on
/// turned text is wrong by the turn, which nothing needs yet.
///
/// `scale` is the same multiplier `render_text_wgpu` computes
/// (`font_scale * transform.scale.x`), and `wrap_width` is
/// `TextComponent.wrap_width` with `0` meaning no wrapping.
pub fn text_bounds(
    font: &FontData,
    text: &str,
    position: (f32, f32),
    scale: f32,
    wrap_width: Option<f32>,
) -> TextBounds {
    let lines = layout_text(font, text, scale, wrap_width);
    let (width, height) = measure_block(&lines, font.line_height * scale);
    TextBounds {
        center_x: position.0 + width * 0.5,
        center_y: position.1 - height * 0.5,
        width,
        height,
    }
}


/// One glyph, placed: where to draw its quad and which part of the atlas to
/// draw. The renderer turns these straight into sprites.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphQuad {
    /// Centre of the quad, in world units.
    pub center: glam::Vec2,
    pub size: glam::Vec2,
    /// How far the quad itself is turned, in radians - the same angle the
    /// whole block was turned by.
    pub rotation: f32,
    /// `[u_min, v_min, u_max, v_max]` into the font atlas.
    pub uv: [f32; 4],
}

/// Place every glyph of `text`.
///
/// This is the advance loop the wgpu renderer used to carry inline, pulled
/// out for two reasons: text can now be **turned** (world text ignored its
/// entity's `Transform.rotation` entirely, which nobody noticed until the
/// camera learned to rotate and every label in the world lay on its side),
/// and placement can be checked without a GPU.
///
/// `position` is the top-left of the first line and the pivot the whole
/// block turns about. Lines run *down* from it, because world Y is up.
/// Missing glyphs fall back to `?`.
#[allow(clippy::too_many_arguments)]
pub fn glyph_quads(
    font: &FontData,
    text: &str,
    position: glam::Vec2,
    scale: f32,
    rotation: f32,
    wrap_width: Option<f32>,
    align: crate::ecs::components::TextAlign,
) -> Vec<GlyphQuad> {
    glyph_places(font, text, position, scale, rotation, wrap_width, align).into_iter().map(|p| p.quad).collect()
}

/// One glyph of [`glyph_places`]: its quad, which character of the source
/// it is, and the centre of that character's cell (its advance wide, a line
/// tall) - where something drawn *in place of* a glyph goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphPlace {
    /// Index into `text.chars()`.
    pub index: usize,
    pub quad: GlyphQuad,
    pub cell_center: glam::Vec2,
}

/// [`glyph_quads`], each glyph tagged with the character of `text` it draws
/// (Oct 2026) - what rich text styles a glyph by (`renderer::rich_text`).
/// Wrapping drops the spaces at each break and every newline, so a line's
/// characters are matched back to the source in order, skipping those.
#[allow(clippy::too_many_arguments)]
pub fn glyph_places(
    font: &FontData,
    text: &str,
    position: glam::Vec2,
    scale: f32,
    rotation: f32,
    wrap_width: Option<f32>,
    align: crate::ecs::components::TextAlign,
) -> Vec<GlyphPlace> {
    // A shaped text's glyphs are the display string's, so indices are into
    // it: rich-text styles on shaped text land on glyphs, not letters.
    let shaped = display_for(font, text);
    let text: &str = &shaped;
    let source: Vec<char> = text.chars().collect();
    let mut next = 0usize;
    let (aw, ah) = (font.atlas_width as f32, font.atlas_height as f32);
    let line_height = font.line_height * scale;
    let (sin, cos) = rotation.sin_cos();
    // Turning about `position` means placing each glyph as if the block were
    // upright, then rotating that offset. Every line turns about the same
    // pivot, so a paragraph stays a paragraph.
    let turn = |local: glam::Vec2| {
        if rotation == 0.0 {
            position + local
        } else {
            position + glam::Vec2::new(local.x * cos - local.y * sin, local.x * sin + local.y * cos)
        }
    };

    let mut out = Vec::new();
    for (i, line) in layout_text(font, text, scale, wrap_width).iter().enumerate() {
        let mut cursor_x = -align.offset(line.width);
        let cursor_y = -line_height * i as f32;
        for ch in line.text.chars() {
            if ch == '\n' {
                continue;
            }
            // This character's place in the source: the next one equal to
            // it, past any space or newline the wrap dropped.
            while next < source.len() && source[next] != ch && (source[next] == ' ' || source[next] == '\n') {
                next += 1;
            }
            let index = next.min(source.len().saturating_sub(1));
            next += 1;
            let Some(glyph) = font
                .glyphs
                .iter()
                .find(|g| g.ch == ch)
                .or_else(|| font.glyphs.iter().find(|g| g.ch == '?'))
            else {
                continue;
            };
            // Rects and offsets are atlas pixels (`FontData::atlas_scale`).
            let s = scale / font.atlas_scale.max(1e-6);
            let (gw, gh) = (glyph.width as f32 * s, glyph.height as f32 * s);
            // x_offset is the horizontal bearing; y_offset is how far below
            // the line top the bitmap starts, and world Y is up.
            let local = glam::Vec2::new(
                cursor_x + glyph.x_offset as f32 * s + gw * 0.5,
                cursor_y - glyph.y_offset as f32 * s - gh * 0.5,
            );
            let quad = GlyphQuad {
                center: turn(local),
                size: glam::Vec2::new(gw, gh),
                rotation,
                uv: [
                    glyph.x as f32 / aw,
                    glyph.y as f32 / ah,
                    (glyph.x + glyph.width) as f32 / aw,
                    (glyph.y + glyph.height) as f32 / ah,
                ],
            };
            let cell = glam::Vec2::new(cursor_x + glyph.advance * scale * 0.5, cursor_y - line_height * 0.5);
            out.push(GlyphPlace { index, quad, cell_center: turn(cell) });
            cursor_x += glyph.advance * scale;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::text::GlyphInfo;

    /// A font where every glyph advances 10 units, so widths are countable by
    /// eye and a failure says which character count was wrong.
    #[test]
    fn text_bounds_extend_right_and_down_from_the_position() {
        // The bug this exists to prevent: assuming the position is the centre.
        // "abc" at 10 units/glyph is 30 wide, one line of 12 tall, so the box
        // sits half a width to the right and half a height *below* the origin.
        let f = uniform_font();
        let b = text_bounds(&f, "abc", (100.0, 200.0), 1.0, None);
        assert_eq!(b.width, 30.0);
        assert_eq!(b.height, 12.0);
        assert_eq!(b.center_x, 115.0);
        assert_eq!(b.center_y, 194.0);
    }

    #[test]
    fn text_bounds_grow_with_line_count() {
        let f = uniform_font();
        let one = text_bounds(&f, "ab", (0.0, 0.0), 1.0, None);
        let two = text_bounds(&f, "ab
cd", (0.0, 0.0), 1.0, None);
        assert_eq!(two.height, one.height * 2.0);
        // Width is the widest line, not the sum.
        assert_eq!(two.width, one.width);
    }

    #[test]
    fn text_bounds_scale() {
        // A title at font_scale 4 is four times the box; picking it with an
        // unscaled 64x64 default is why long labels were unclickable.
        let f = uniform_font();
        let a = text_bounds(&f, "abcd", (0.0, 0.0), 1.0, None);
        let b = text_bounds(&f, "abcd", (0.0, 0.0), 4.0, None);
        assert_eq!(b.width, a.width * 4.0);
        assert_eq!(b.height, a.height * 4.0);
    }

    #[test]
    fn text_bounds_respect_wrapping() {
        let f = uniform_font();
        let wrapped = text_bounds(&f, "aa bb cc", (0.0, 0.0), 1.0, Some(25.0));
        assert!(wrapped.width <= 25.0, "{wrapped:?}");
        assert!(wrapped.height > 12.0, "wrapping must add lines: {wrapped:?}");
    }

    #[test]
    fn empty_text_has_no_area() {
        let f = uniform_font();
        let b = text_bounds(&f, "", (5.0, 5.0), 1.0, None);
        assert_eq!(b.width, 0.0);
    }

    fn uniform_font() -> FontData {
        let glyphs = (32u8..127)
            .map(|b| GlyphInfo {
                ch: b as char,
                x: 0,
                y: 0,
                width: 8,
                height: 8,
                x_offset: 0,
                y_offset: 0,
                advance: 10.0,
            })
            .collect();
        FontData {
            atlas_scale: 1.0,
            sdf: false,
            sdf_spread: 0.0,
            glyphs,
            shaping: None,
            atlas_width: 128,
            atlas_height: 128,
            line_height: 12.0,
            pixel_size: 8.0,
        }
    }

    #[test]
    fn newline_splits_into_lines() {
        // The whole point: `\n` used to be dropped, so this was one line.
        let font = uniform_font();
        let lines = layout_text(&font, "ab\ncd", 1.0, None);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "ab");
        assert_eq!(lines[1].text, "cd");
    }

    #[test]
    fn a_trailing_newline_keeps_its_empty_line() {
        // Otherwise anything positioned under the block moves up a line.
        let font = uniform_font();
        let lines = layout_text(&font, "ab\n", 1.0, None);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].text, "");
    }

    #[test]
    fn measure_matches_the_renderers_advances() {
        let font = uniform_font();
        assert_eq!(measure_line(&font, "abc", 1.0), 30.0);
        assert_eq!(measure_line(&font, "abc", 2.0), 60.0);
    }

    #[test]
    fn wraps_on_whitespace_at_the_limit() {
        let font = uniform_font();
        // "aa bb cc" — each word 20 wide, space 10. Limit 50 fits "aa bb".
        let lines = layout_text(&font, "aa bb cc", 1.0, Some(50.0));
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0].text, "aa bb");
        assert_eq!(lines[1].text, "cc");
    }

    #[test]
    fn a_word_longer_than_the_line_is_broken_not_overflowed() {
        // A URL or a long compound would otherwise run off the screen with
        // nothing to indicate it had.
        let font = uniform_font();
        let lines = layout_text(&font, "abcdefgh", 1.0, Some(30.0));
        assert!(lines.len() > 1, "{lines:?}");
        assert!(lines.iter().all(|l| l.width <= 30.0), "{lines:?}");
    }

    #[test]
    fn wrapping_still_honours_explicit_newlines() {
        let font = uniform_font();
        let lines = layout_text(&font, "aa bb\ncc", 1.0, Some(100.0));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "aa bb");
        assert_eq!(lines[1].text, "cc");
    }

    #[test]
    fn no_wrap_width_leaves_long_lines_alone() {
        let font = uniform_font();
        let lines = layout_text(&font, "aa bb cc", 1.0, None);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].width, 80.0);
    }

    use crate::ecs::components::TextAlign;
    use glam::Vec2;

    fn quads(text: &str, rotation: f32) -> Vec<GlyphQuad> {
        glyph_quads(&uniform_font(), text, Vec2::new(100.0, 50.0), 1.0, rotation, None, TextAlign::Left)
    }

    #[test]
    fn unturned_text_advances_to_the_right_and_down() {
        let q = quads("ab
cd", 0.0);
        assert_eq!(q.len(), 4);
        // Glyphs are 8 wide with a 10 advance, so the first centre sits 4
        // right of the pen and the second a whole advance further.
        assert_eq!(q[0].center, Vec2::new(104.0, 46.0));
        assert_eq!(q[1].center, Vec2::new(114.0, 46.0));
        // The second line is one line height lower; world Y is up.
        assert_eq!(q[2].center, Vec2::new(104.0, 34.0));
        assert!(q.iter().all(|g| g.rotation == 0.0 && g.size == Vec2::new(8.0, 8.0)));
    }

    #[test]
    fn a_quarter_turn_runs_the_line_upwards() {
        let q = quads("ab", std::f32::consts::FRAC_PI_2);
        // Turned a quarter anticlockwise about the text's own position, the
        // line that ran right now runs up, and each quad is turned with it.
        assert!((q[0].center - Vec2::new(104.0, 54.0)).length() < 0.001, "{:?}", q[0].center);
        assert!((q[1].center - Vec2::new(104.0, 64.0)).length() < 0.001, "{:?}", q[1].center);
        assert!(q.iter().all(|g| (g.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-6));
    }

    #[test]
    fn turning_keeps_every_glyph_the_same_distance_from_the_pivot() {
        let pivot = Vec2::new(100.0, 50.0);
        let straight = quads("hello world", 0.0);
        for angle in [0.4f32, 1.9, -2.7] {
            let turned = quads("hello world", angle);
            assert_eq!(turned.len(), straight.len());
            for (a, b) in straight.iter().zip(turned.iter()) {
                let (da, db) = ((a.center - pivot).length(), (b.center - pivot).length());
                assert!((da - db).abs() < 0.001, "{da} vs {db} at {angle} rad");
            }
        }
    }

    #[test]
    fn alignment_and_wrapping_survive_the_turn() {
        let font = uniform_font();
        let at = Vec2::new(0.0, 0.0);
        // Centred, a 3-glyph line (30 wide) starts 15 to the left.
        let centred = glyph_quads(&font, "abc", at, 1.0, 0.0, None, TextAlign::Center);
        assert_eq!(centred[0].center.x, -11.0);
        // The same string turned a half turn ends up mirrored about the
        // pivot, not re-laid-out.
        let turned = glyph_quads(&font, "abc", at, 1.0, std::f32::consts::PI, None, TextAlign::Center);
        assert!((turned[0].center.x - 11.0).abs() < 0.001, "{:?}", turned[0].center);
        // Wrapping still breaks in the same place.
        let wrapped = glyph_quads(&font, "aa bb", at, 1.0, 0.7, Some(25.0), TextAlign::Left);
        assert_eq!(wrapped.len(), 4);
    }

    #[test]
    fn block_measures_widest_line_by_height() {
        let font = uniform_font();
        let lines = layout_text(&font, "a\nabc", 1.0, None);
        let (w, h) = measure_block(&lines, 12.0);
        assert_eq!(w, 30.0);
        assert_eq!(h, 24.0);
    }
}
