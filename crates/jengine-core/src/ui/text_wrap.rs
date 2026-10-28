//! Text wrapped to a width with the built-in font's metrics - how a speech
//! bubble or a toast sizes its card before anything is drawn. Kept apart
//! from both so each can be left out of a build alone
//! (`docs/engine-features-plan.md`).

use std::cell::RefCell;

use glam::Vec2;

use crate::renderer::text::FontData;

thread_local! {
    static FONT: RefCell<Option<FontData>> = const { RefCell::new(None) };
}

fn with_font<R>(f: impl FnOnce(&FontData) -> R) -> R {
    FONT.with(|cell| {
        let mut cell = cell.borrow_mut();
        let font = cell.get_or_insert_with(|| crate::renderer::text::built_in_font_atlas().3);
        f(font)
    })
}

/// `text` wrapped to `max_width` at `font_size`: its lines and its size,
/// UI pixels.
pub fn wrap(text: &str, font_size: f32, max_width: f32) -> (String, Vec2) {
    with_font(|font| {
        let scale = font_size / 16.0;
        let lines = crate::renderer::text_layout::layout_text(font, text, scale, Some(max_width.max(20.0)));
        let w = lines.iter().fold(0.0f32, |a, l| a.max(l.width));
        let h = lines.len() as f32 * font.line_height * scale;
        (lines.iter().map(|l| l.text.trim_end()).collect::<Vec<_>>().join("\n"), Vec2::new(w, h))
    })
}
