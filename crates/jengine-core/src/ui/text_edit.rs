//! A text field you can edit (Oct 2026; Godot's `LineEdit`, Unity's
//! `TMP_InputField`): a caret you can move, a selection, words, the
//! clipboard, Enter and Escape, a length limit, a password mask, and the
//! composition an IME shows before it commits. One model for both UI
//! systems - entity UI's `ui_text_input` and a `.gui` `TextInput` node each
//! keep a [`TextEdit`] and call [`TextEdit::update`] while focused.
//!
//! Until then a field appended what was typed and Backspace took the last
//! character: no caret, no selection, no paste, and text past the field's
//! right edge drew off it.
//!
//! The model counts **characters**, not bytes, so a caret never lands inside
//! an `ä`. Measuring is the renderer's: it alone has the font, so each frame
//! it lays a focused field out ([`layout`]) and leaves what it measured in a
//! per-field [`FieldView`] - where each character starts, and how far the
//! text has scrolled to keep the caret in sight - which the next frame's
//! click reads to put the caret where the pointer is.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::input::{InputState, KeyCode};

/// One field's editing state. Runtime only: never saved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextEdit {
    /// The caret, as a character index (0 = before the first).
    pub caret: usize,
    /// The other end of the selection, when there is one.
    pub anchor: Option<usize>,
    /// When the caret last moved (seconds, `profiling::unix_time`): it
    /// shows at once and blinks from then.
    pub moved_at: f64,
    /// The time of the update or click under way.
    now: f64,
    /// The pointer went down in the field and is still down: moving it
    /// selects.
    pub dragging: bool,
    /// Enter was pressed this frame (`submitted` to scripts).
    pub submitted: bool,
    /// The text changed this frame.
    pub changed: bool,
    /// When and where the last click landed, and how many in a row - a
    /// second selects a word, a third everything.
    last_click: Option<(f64, usize)>,
    clicks: u32,
}

/// What one update did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Edited {
    pub changed: bool,
    pub submitted: bool,
    /// Escape: the field gives focus up.
    pub cancelled: bool,
}

/// The longest text a field holds when it sets no limit of its own.
pub const MAX_CHARS: usize = 4096;

/// Seconds a caret blink lasts (on, then off).
pub const BLINK: f32 = 1.06;

/// How long two clicks may be apart and still be a double click.
const DOUBLE_CLICK: f64 = 0.4;

fn count(s: &str) -> usize {
    s.chars().count()
}

/// The byte offset of character `ci` in `s` (its length past the end).
pub fn byte_at(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map_or(s.len(), |(b, _)| b)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where a ctrl+Left lands from `at`: the start of this word or the last.
fn word_left(chars: &[char], at: usize) -> usize {
    let mut i = at.min(chars.len());
    while i > 0 && !is_word(chars[i - 1]) {
        i -= 1;
    }
    while i > 0 && is_word(chars[i - 1]) {
        i -= 1;
    }
    i
}

/// Where a ctrl+Right lands from `at`: the end of this word or the next.
fn word_right(chars: &[char], at: usize) -> usize {
    let mut i = at.min(chars.len());
    while i < chars.len() && !is_word(chars[i]) {
        i += 1;
    }
    while i < chars.len() && is_word(chars[i]) {
        i += 1;
    }
    i
}

impl TextEdit {
    /// The selection as `(start, end)` characters, when it is not empty.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.caret).then(|| (a.min(self.caret), a.max(self.caret)))
    }

    /// The selected text.
    pub fn selected<'a>(&self, text: &'a str) -> &'a str {
        match self.selection() {
            Some((s, e)) => &text[byte_at(text, s)..byte_at(text, e)],
            None => "",
        }
    }

    /// A script set the caret (`caret`) or the selection (`selection`,
    /// `[start, end]` in characters): clamped to `text`.
    pub fn set_from_script(&mut self, text: &str, value: &serde_json::Value) -> bool {
        let n = count(text);
        let num = |v: &serde_json::Value| v.as_f64().map(|f| (f.max(0.0) as usize).min(n));
        if let Some(c) = num(value) {
            self.caret = c;
            self.anchor = None;
        } else if let Some([a, b]) = value.as_array().and_then(|v| <&[serde_json::Value; 2]>::try_from(v.as_slice()).ok())
            && let (Some(a), Some(b)) = (num(a), num(b))
        {
            self.anchor = Some(a);
            self.caret = b;
        } else {
            return false;
        }
        self.moved();
        true
    }

    /// Put the caret at the end, nothing selected: where focus lands.
    pub fn focus_end(&mut self, text: &str) {
        self.caret = count(text);
        self.anchor = None;
        self.moved_at = self.now;
        self.dragging = false;
    }

    fn moved(&mut self) {
        self.moved_at = self.now;
    }

    fn go(&mut self, to: usize, select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        self.caret = to;
        self.moved();
    }

    fn delete_selection(&mut self, text: &mut String) -> bool {
        let Some((s, e)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        text.replace_range(byte_at(text, s)..byte_at(text, e), "");
        self.caret = s;
        self.anchor = None;
        true
    }

    /// Put `s` where the caret is, over the selection, keeping to `max`
    /// characters and one line. Returns whether anything went in.
    pub fn insert(&mut self, text: &mut String, s: &str, max: usize) -> bool {
        let mut changed = self.delete_selection(text);
        let room = max.saturating_sub(count(text));
        let s: String = s.chars().filter(|c| !c.is_control()).take(room).collect();
        if !s.is_empty() {
            let at = byte_at(text, self.caret);
            text.insert_str(at, &s);
            self.caret += count(&s);
            changed = true;
        }
        self.moved();
        changed
    }

    /// One frame of a focused field: the keys, what was typed and pasted.
    /// Every key the field uses is consumed, and so is every key that typed
    /// a character, so a game's own bindings - Escape for back, H for the
    /// hub - do not fire while someone is writing. Copy and cut go to
    /// [`crate::input::clipboard`].
    pub fn update(&mut self, text: &mut String, input: &mut InputState, max: usize, now: f64) -> Edited {
        let max = if max == 0 { MAX_CHARS } else { max };
        let mut out = Edited::default();
        self.submitted = false;
        self.changed = false;
        self.now = now;
        let n = count(text);
        self.caret = self.caret.min(n);
        if self.anchor.is_some_and(|a| a > n) {
            self.anchor = None;
        }
        let ctrl = input.ctrl_held();
        let shift = input.is_pressed(KeyCode::ShiftLeft) || input.is_pressed(KeyCode::ShiftRight);
        let key = |k: KeyCode| input.is_typed(k);
        let chars: Vec<char> = text.chars().collect();

        if ctrl && key(KeyCode::A) {
            self.anchor = Some(0);
            self.caret = n;
            self.moved();
        }
        if ctrl && (key(KeyCode::C) || key(KeyCode::X)) && self.selection().is_some() {
            crate::input::clipboard::copy(self.selected(text));
            if key(KeyCode::X) {
                out.changed |= self.delete_selection(text);
            }
        }
        if key(KeyCode::Left) {
            let to = match (self.selection(), ctrl, shift) {
                (Some((s, _)), false, false) => s,
                (_, true, _) => word_left(&chars, self.caret),
                _ => self.caret.saturating_sub(1),
            };
            self.go(to, shift);
        }
        if key(KeyCode::Right) {
            let to = match (self.selection(), ctrl, shift) {
                (Some((_, e)), false, false) => e,
                (_, true, _) => word_right(&chars, self.caret),
                _ => (self.caret + 1).min(n),
            };
            self.go(to, shift);
        }
        if key(KeyCode::Home) || key(KeyCode::Up) {
            self.go(0, shift);
        }
        if key(KeyCode::End) || key(KeyCode::Down) {
            self.go(n, shift);
        }
        if key(KeyCode::Backspace) {
            if !self.delete_selection(text) && self.caret > 0 {
                let from = if ctrl { word_left(&chars, self.caret) } else { self.caret - 1 };
                text.replace_range(byte_at(text, from)..byte_at(text, self.caret), "");
                self.caret = from;
            }
            out.changed = true;
            self.moved();
        }
        if key(KeyCode::Delete) {
            if !self.delete_selection(text) && self.caret < n {
                let to = if ctrl { word_right(&chars, self.caret) } else { self.caret + 1 };
                text.replace_range(byte_at(text, self.caret)..byte_at(text, to), "");
            }
            out.changed = true;
            self.moved();
        }
        if !input.pasted().is_empty() {
            let pasted = input.pasted().replace(['\r', '\n', '\t'], " ");
            out.changed |= self.insert(text, &pasted, max);
        }
        if !input.typed_chars().is_empty() {
            let typed: String = input.typed_chars().iter().collect();
            out.changed |= self.insert(text, &typed, max);
        }
        if key(KeyCode::Enter) || key(KeyCode::NumpadEnter) {
            out.submitted = true;
        }
        if key(KeyCode::Escape) {
            out.cancelled = true;
        }
        // A field being written in owns the keyboard.
        input.consume_typing();
        self.submitted = out.submitted;
        self.changed = out.changed;
        out
    }

    /// A click at character `at`: the caret goes there (shift extends the
    /// selection); a second click on the same spot selects the word, a
    /// third the whole text. `now` is seconds on any steady clock.
    pub fn click(&mut self, text: &str, at: usize, shift: bool, now: f64) {
        self.now = now;
        let n = count(text);
        let at = at.min(n);
        let again = self.last_click.is_some_and(|(t, i)| now - t < DOUBLE_CLICK && i.abs_diff(at) <= 1);
        self.clicks = if again { self.clicks + 1 } else { 1 };
        self.last_click = Some((now, at));
        self.dragging = true;
        match self.clicks {
            2 => {
                let chars: Vec<char> = text.chars().collect();
                let s = if at < n && is_word(chars[at]) { word_left(&chars, at + 1) } else { at };
                self.anchor = Some(s);
                self.caret = word_right(&chars, s);
            }
            c if c >= 3 => {
                self.anchor = Some(0);
                self.caret = n;
            }
            _ => self.go(at, shift),
        }
        self.moved();
    }

    /// The pointer, still down, is at character `at`: select up to it.
    pub fn drag(&mut self, text: &str, at: usize) {
        if self.dragging && self.clicks <= 1 {
            let at = at.min(count(text));
            if at != self.caret {
                self.go(at, true);
            }
        }
    }

    /// Whether the caret shows at `now` (it blinks, and shows at once
    /// after it moves).
    pub fn caret_visible(&self, now: f64) -> bool {
        ((now - self.moved_at).max(0.0) as f32) % BLINK < BLINK * 0.5
    }
}

/// What a password field shows: one `*` a character.
pub fn masked(text: &str) -> String {
    "*".repeat(count(text))
}

/// The text a field shows while an IME composes: the composition in at the
/// caret, and the character range it takes.
pub fn with_preedit(text: &str, caret: usize, preedit: &str) -> (String, Option<(usize, usize)>) {
    if preedit.is_empty() {
        return (text.to_string(), None);
    }
    let at = byte_at(text, caret);
    let mut out = String::with_capacity(text.len() + preedit.len());
    out.push_str(&text[..at]);
    out.push_str(preedit);
    out.push_str(&text[at..]);
    (out, Some((caret, caret + count(preedit))))
}

/// The character boundary nearest `x` (measured from the text's start),
/// given where each one is: `advances[i]` is where character `i` starts,
/// and the last entry is where the text ends.
pub fn caret_index_at(advances: &[f32], x: f32) -> usize {
    if advances.is_empty() {
        return 0;
    }
    let mut best = 0;
    for (i, a) in advances.iter().enumerate() {
        if (a - x).abs() < (advances[best] - x).abs() {
            best = i;
        }
    }
    best
}

/// How a focused field draws this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldLayout {
    /// How far the text is scrolled left, in pixels.
    pub scroll: f32,
    /// The characters that fit: `first..last`.
    pub first: usize,
    pub last: usize,
    /// Where character `first` starts on screen.
    pub text_x: f32,
    /// The caret's x on screen, when it shows.
    pub caret_x: Option<f32>,
    /// The selection's span on screen.
    pub selection: Option<(f32, f32)>,
}

/// Lay a field out: scroll so the caret stays in `width` (starting from
/// last frame's `scroll`), and say which characters fit, where the caret
/// and the selection are. `left` is where the text area starts on screen.
pub fn layout(
    advances: &[f32],
    caret: usize,
    selection: Option<(usize, usize)>,
    caret_shows: bool,
    left: f32,
    width: f32,
    scroll: f32,
) -> FieldLayout {
    let at = |i: usize| advances.get(i).copied().unwrap_or_else(|| advances.last().copied().unwrap_or(0.0));
    let total = at(usize::MAX);
    let room = (width - 2.0).max(1.0);
    let caret_px = at(caret);
    let mut scroll = scroll.clamp(0.0, (total - room).max(0.0));
    if caret_px - scroll > room {
        scroll = caret_px - room;
    }
    if caret_px < scroll {
        scroll = caret_px;
    }
    let n = advances.len().saturating_sub(1);
    let first = (0..=n).find(|&i| at(i) >= scroll - 0.01).unwrap_or(n);
    let last = (first..=n).rev().find(|&i| at(i) <= scroll + room + 0.01).unwrap_or(first).max(first);
    FieldLayout {
        scroll,
        first,
        last,
        text_x: left + at(first) - scroll,
        caret_x: caret_shows.then(|| left + caret_px - scroll),
        selection: selection.map(|(s, e)| {
            (left + at(s).max(scroll) - scroll, left + at(e).min(scroll + room) - scroll)
        }),
    }
}

/// What the renderer measured of one field last frame, for the next
/// frame's clicks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FieldView {
    /// The text area on screen (UI pixels): left, top, width, height.
    pub rect: [f32; 4],
    pub scroll: f32,
    /// Where each character starts, relative to the text's start, and where
    /// it ends (one more entry than characters).
    pub advances: Vec<f32>,
}

impl FieldView {
    /// The character nearest screen x `x`.
    pub fn index_at(&self, x: f32) -> usize {
        caret_index_at(&self.advances, x - self.rect[0] + self.scroll)
    }

    /// The caret's rectangle on screen, for an IME's candidate window.
    pub fn caret_rect(&self, caret: usize) -> [f32; 4] {
        let x = self.advances.get(caret).copied().unwrap_or(0.0);
        [self.rect[0] + x - self.scroll, self.rect[1], 2.0, self.rect[3]]
    }
}

thread_local! {
    static VIEWS: RefCell<HashMap<u64, FieldView>> = RefCell::new(HashMap::new());
}

/// What the renderer measured of field `key` (see [`entity_key`],
/// [`gui_key`]).
pub fn view(key: u64) -> Option<FieldView> {
    VIEWS.with(|v| v.borrow().get(&key).cloned())
}

/// The renderer leaves what it measured.
pub fn set_view(key: u64, view: FieldView) {
    VIEWS.with(|v| {
        let mut v = v.borrow_mut();
        if v.len() > 256 {
            v.clear();
        }
        v.insert(key, view);
    });
}

/// The view key of an entity-UI field.
pub fn entity_key(entity: hecs::Entity) -> u64 {
    entity.to_bits().get()
}

/// The view key of a `.gui` field.
pub fn gui_key(gui_id: &str, node_id: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (gui_id, node_id).hash(&mut h);
    h.finish() | (1 << 63)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(input: &mut InputState, keys: &[KeyCode]) {
        input.begin_frame();
        for k in keys {
            input.key_down(*k);
        }
    }

    fn release_all(input: &mut InputState) {
        input.release_all();
        input.begin_frame();
    }

    #[test]
    fn typing_goes_in_at_the_caret_not_at_the_end() {
        let mut input = InputState::new();
        let mut text = "helo".to_string();
        let mut e = TextEdit::default();
        e.focus_end(&text);
        press(&mut input, &[KeyCode::Left]);
        e.update(&mut text, &mut input, 0, 1.0);
        release_all(&mut input);
        input.type_str("l");
        let out = e.update(&mut text, &mut input, 0, 1.0);
        assert_eq!(text, "hello");
        assert!(out.changed);
        assert_eq!(e.caret, 4);
    }

    #[test]
    fn words_selection_and_the_clipboard() {
        let _ = crate::input::clipboard::take_copy();
        let mut input = InputState::new();
        let mut text = "pale moon rising".to_string();
        let mut e = TextEdit::default();
        e.focus_end(&text);
        // ctrl+shift+Left selects "rising"; ctrl+X cuts it.
        press(&mut input, &[KeyCode::ControlLeft, KeyCode::ShiftLeft, KeyCode::Left]);
        e.update(&mut text, &mut input, 0, 1.0);
        assert_eq!(e.selected(&text), "rising");
        release_all(&mut input);
        press(&mut input, &[KeyCode::ControlLeft, KeyCode::X]);
        e.update(&mut text, &mut input, 0, 1.0);
        assert_eq!(text, "pale moon ");
        assert_eq!(crate::input::clipboard::take_copy().as_deref(), Some("rising"));
        release_all(&mut input);
        // Home, then paste.
        press(&mut input, &[KeyCode::Home]);
        e.update(&mut text, &mut input, 0, 1.0);
        release_all(&mut input);
        input.paste("a ");
        e.update(&mut text, &mut input, 0, 1.0);
        assert_eq!(text, "a pale moon ");
        release_all(&mut input);
        // ctrl+Backspace takes the word before the caret.
        press(&mut input, &[KeyCode::End]);
        e.update(&mut text, &mut input, 0, 1.0);
        release_all(&mut input);
        press(&mut input, &[KeyCode::ControlLeft, KeyCode::Backspace]);
        e.update(&mut text, &mut input, 0, 1.0);
        assert_eq!(text, "a pale ");
    }

    #[test]
    fn a_held_key_repeats_and_the_field_owns_the_keyboard() {
        let mut input = InputState::new();
        let mut text = "abc".to_string();
        let mut e = TextEdit::default();
        e.focus_end(&text);
        press(&mut input, &[KeyCode::Backspace]);
        e.update(&mut text, &mut input, 0, 1.0);
        assert!(!input.is_just_pressed(KeyCode::Backspace), "consumed");
        // Held: the platform repeats the key-down.
        input.begin_frame();
        input.key_down(KeyCode::Backspace);
        e.update(&mut text, &mut input, 0, 1.0);
        assert_eq!(text, "a");
        // A letter key typed into the field does not reach the game's H.
        release_all(&mut input);
        input.key_down(KeyCode::H);
        input.type_str("h");
        e.update(&mut text, &mut input, 0, 1.0);
        assert_eq!(text, "ah");
        assert!(!input.is_just_pressed(KeyCode::H));
    }

    #[test]
    fn a_limit_a_submit_and_wide_characters() {
        let mut input = InputState::new();
        let mut text = "äö".to_string();
        let mut e = TextEdit::default();
        e.focus_end(&text);
        input.type_str("üxyz");
        e.update(&mut text, &mut input, 4, 1.0);
        assert_eq!(text, "äöüx", "four characters, not bytes");
        press(&mut input, &[KeyCode::Left, KeyCode::Enter]);
        let out = e.update(&mut text, &mut input, 4, 1.0);
        assert!(out.submitted);
        assert_eq!(e.caret, 3, "between ü and x");
    }

    #[test]
    fn clicks_place_select_a_word_and_everything() {
        let text = "one two";
        let mut e = TextEdit::default();
        e.click(text, 5, false, 10.0);
        assert_eq!((e.caret, e.selection()), (5, None));
        e.click(text, 5, false, 10.2);
        assert_eq!(e.selected(text), "two");
        e.click(text, 5, false, 10.3);
        assert_eq!(e.selected(text), "one two");
        e.click(text, 1, false, 20.0);
        e.drag(text, 6);
        assert_eq!(e.selected(text), "ne tw");
    }

    #[test]
    fn a_long_line_scrolls_to_keep_the_caret_in_sight() {
        // Ten characters ten pixels each in a field fifty wide.
        let adv: Vec<f32> = (0..=10).map(|i| i as f32 * 10.0).collect();
        let l = layout(&adv, 10, None, true, 100.0, 50.0, 0.0);
        assert!((l.scroll - 52.0).abs() < 0.01, "{l:?}");
        assert_eq!((l.first, l.last), (6, 10));
        assert!((l.caret_x.unwrap() - 148.0).abs() < 0.01);
        // Back to the start: the scroll follows.
        let l = layout(&adv, 1, Some((1, 3)), false, 100.0, 50.0, l.scroll);
        assert!((l.scroll - 10.0).abs() < 0.01);
        assert_eq!(l.first, 1);
        assert_eq!(l.selection, Some((100.0, 120.0)));
        assert_eq!(caret_index_at(&adv, 34.0), 3);
        assert_eq!(with_preedit("ab", 1, "日本").0, "a日本b");
    }
}
