//! **Rich text** (Oct 2026): markup in world text - a word in a colour, a
//! name that shimmers, a shout that shakes, a pause mid-sentence, an icon
//! among the words - and text that appears a letter at a time.
//!
//! A `text` component with `"rich": true` reads its string as markup in
//! square brackets, Godot's BBCode spelling:
//!
//! | Tag | Does |
//! |---|---|
//! | `[color=#e05040]..[/color]` | colours the run (sRGB hex, or a name below) |
//! | `[b]..[/b]` | bold - each glyph drawn twice, a hair apart |
//! | `[wave]..[/wave]` | letters bob in a travelling wave |
//! | `[shake]..[/shake]` | letters jitter |
//! | `[rainbow]..[/rainbow]` | letters cycle through the hues |
//! | `[pause=0.5]` | the typewriter waits half a second here |
//! | `[icon=sprites/key.png]` | a square image a line tall, inline |
//!
//! `[[` is a literal `[`; a tag the parser does not know is kept as text.
//! Without `rich` a string is drawn as it is, brackets and all, so no
//! existing text changed meaning.
//!
//! [`parse`] turns the markup into the plain text that is laid out - so
//! wrapping, alignment and measuring are exactly plain text's - and a style
//! per character. The renderer draws each glyph with its character's style
//! (`text_layout::glyph_places` says which character a glyph is); the
//! effects move with the batch's clock, so they need no state. An icon
//! takes the room of a few spaces in the layout and is drawn over them as a
//! small sprite of its own texture, sorted with the text.
//!
//! **The typewriter** is `reveal_speed` (characters a second) on the
//! component: [`tick_reveals`], run by every host inside
//! `behaviors::tick_motion`, counts the time since the text last changed,
//! and [`visible_chars`] turns it into how many characters show, waiting at
//! each `[pause]`. A text whose reveal has never ticked shows whole - which
//! is what the editor shows while editing - and setting new text starts it
//! over.
//!
//! **The UI reads the same markup** (Oct 2026): an entity-UI `ui_text` with
//! `rich` (and its own typewriter, ticked here too), and a `.gui` Text node
//! with `rich`, parsed after its `{{variables}}` are filled in. Both draw
//! through the world text's own glyph loop, flat.

use crate::ecs::components::TextComponent;

/// How one character of rich text is drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CharStyle {
    /// A colour of its own (linear), or the component's.
    pub color: Option<[f32; 4]>,
    pub bold: bool,
    pub wave: bool,
    pub shake: bool,
    pub rainbow: bool,
}

/// An inline image: drawn over the placeholder spaces from `at` (a char
/// index into [`RichText::plain`]) for `len` characters. `id` is a byte
/// range of the markup, so the texture id borrows from the component.
#[derive(Debug, Clone, PartialEq)]
pub struct Icon {
    pub at: usize,
    pub len: usize,
    pub id: std::ops::Range<usize>,
}

/// Markup, parsed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RichText {
    /// What is laid out and drawn.
    pub plain: String,
    /// One per character of `plain`.
    pub styles: Vec<CharStyle>,
    /// `(char index, seconds)`: the typewriter waits before that character.
    pub pauses: Vec<(usize, f32)>,
    pub icons: Vec<Icon>,
}

/// A few colours by name; anything else is a `#rrggbb` (or `#rgb`) hex.
fn named(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "red" => [224, 72, 64],
        "orange" => [240, 150, 60],
        "gold" | "yellow" => [240, 200, 80],
        "green" => [110, 200, 110],
        "teal" => [70, 190, 180],
        "blue" => [90, 150, 240],
        "purple" => [170, 110, 230],
        "pink" => [240, 130, 190],
        "white" => [255, 255, 255],
        "grey" | "gray" => [150, 150, 160],
        "black" => [20, 20, 24],
        _ => return None,
    })
}

fn srgb_to_linear(c: u8) -> f32 {
    let v = c as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// A colour as markup writes it - sRGB, as a picker shows it - in the
/// engine's linear space.
pub fn parse_color(value: &str) -> Option<[f32; 4]> {
    let v = value.trim();
    let rgb = if let Some(hex) = v.strip_prefix('#') {
        let n = |s: &str| u8::from_str_radix(s, 16).ok();
        match hex.len() {
            6 => [n(&hex[0..2])?, n(&hex[2..4])?, n(&hex[4..6])?],
            3 => {
                let d = |i: usize| n(&hex[i..i + 1]).map(|x| x * 17);
                [d(0)?, d(1)?, d(2)?]
            }
            _ => return None,
        }
    } else {
        named(&v.to_ascii_lowercase())?
    };
    Some([srgb_to_linear(rgb[0]), srgb_to_linear(rgb[1]), srgb_to_linear(rgb[2]), 1.0])
}

/// Parse `markup`. An icon becomes `icon_spaces` spaces of the plain text,
/// which the caller sizes to about a line's height in its font
/// ([`icon_spaces`]).
/// Text as it is, no markup read - what a plain (not `rich`) text draws
/// through when its typewriter runs, so `[brackets]` stay brackets.
pub fn literal(text: &str) -> RichText {
    RichText {
        plain: text.to_string(),
        styles: vec![CharStyle::default(); text.chars().count()],
        pauses: Vec::new(),
        icons: Vec::new(),
    }
}

/// Whether a text's typewriter is running and has to be drawn through the
/// rich path: `reveal_speed` counts for plain text too. It used to be drawn
/// only for `rich` text, while `dialogue::typing` counted it either way - so
/// a plain line showed whole and the player's first press only finished a
/// line already on screen.
pub fn reveal_running(rich: bool, speed: f32, elapsed: Option<f32>) -> bool {
    !rich && speed > 0.0 && elapsed.is_some()
}

pub fn parse(markup: &str, icon_spaces: usize) -> RichText {
    let mut out = RichText::default();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let (mut bold, mut wave, mut shake, mut rainbow) = (0u32, 0u32, 0u32, 0u32);
    let mut count = 0usize;
    let push = |out: &mut RichText, count: &mut usize, ch: char, style: CharStyle| {
        out.plain.push(ch);
        out.styles.push(style);
        *count += 1;
    };
    let mut i = 0usize;
    while i < markup.len() {
        let rest = &markup[i..];
        let ch = rest.chars().next().unwrap();
        if ch == '[' {
            if rest.starts_with("[[") {
                let style = current(&colors, bold, wave, shake, rainbow);
                push(&mut out, &mut count, '[', style);
                i += 2;
                continue;
            }
            if let Some(close) = rest.find(']') {
                let tag = &rest[1..close];
                let (name, value) = match tag.split_once('=') {
                    Some((n, v)) => (n.trim(), Some(v)),
                    None => (tag.trim(), None),
                };
                let value_start = i + 1 + tag.find('=').map_or(tag.len(), |p| p + 1);
                let known = match (name, value) {
                    ("color", Some(v)) => parse_color(v).map(|c| colors.push(c)).is_some(),
                    ("/color", None) => {
                        colors.pop();
                        true
                    }
                    ("b", None) => {
                        bold += 1;
                        true
                    }
                    ("/b", None) => {
                        bold = bold.saturating_sub(1);
                        true
                    }
                    ("wave", None) => {
                        wave += 1;
                        true
                    }
                    ("/wave", None) => {
                        wave = wave.saturating_sub(1);
                        true
                    }
                    ("shake", None) => {
                        shake += 1;
                        true
                    }
                    ("/shake", None) => {
                        shake = shake.saturating_sub(1);
                        true
                    }
                    ("rainbow", None) => {
                        rainbow += 1;
                        true
                    }
                    ("/rainbow", None) => {
                        rainbow = rainbow.saturating_sub(1);
                        true
                    }
                    ("pause", Some(v)) => match v.trim().parse::<f32>() {
                        Ok(s) if s >= 0.0 => {
                            out.pauses.push((count, s));
                            true
                        }
                        _ => false,
                    },
                    ("icon", Some(v)) if !v.trim().is_empty() => {
                        let lead = v.len() - v.trim_start().len();
                        let id = value_start + lead..value_start + lead + v.trim().len();
                        let n = icon_spaces.max(1);
                        out.icons.push(Icon { at: count, len: n, id });
                        let style = current(&colors, bold, wave, shake, rainbow);
                        for _ in 0..n {
                            push(&mut out, &mut count, ' ', style);
                        }
                        true
                    }
                    _ => false,
                };
                if known {
                    i += close + 1;
                    continue;
                }
            }
        }
        let style = current(&colors, bold, wave, shake, rainbow);
        push(&mut out, &mut count, ch, style);
        i += ch.len_utf8();
    }
    out
}

fn current(colors: &[[f32; 4]], bold: u32, wave: u32, shake: u32, rainbow: u32) -> CharStyle {
    CharStyle { color: colors.last().copied(), bold: bold > 0, wave: wave > 0, shake: shake > 0, rainbow: rainbow > 0 }
}

/// How many spaces an icon takes in `font`: about a line's height of them.
pub fn icon_spaces(font: &crate::renderer::text::FontData) -> usize {
    let space = font.glyphs.iter().find(|g| g.ch == ' ').map_or(font.line_height * 0.3, |g| g.advance);
    ((font.line_height * 0.95) / space.max(1e-3)).ceil().max(1.0) as usize
}

/// How many characters of `rich` the typewriter shows `elapsed` seconds
/// after the text was set, at `speed` characters a second, waiting at each
/// pause; and how far into the next one it is (0..1), for a fade. Every
/// character with no reveal running (`speed` 0, or `elapsed` `None`).
pub fn visible_chars(rich: &RichText, speed: f32, elapsed: Option<f32>) -> (usize, f32) {
    let total = rich.styles.len();
    let Some(elapsed) = elapsed else { return (total, 0.0) };
    if !(speed > 0.0) {
        return (total, 0.0);
    }
    let step = 1.0 / speed;
    let mut t = 0.0f32;
    let mut pauses = rich.pauses.iter().peekable();
    for i in 0..total {
        while let Some(&&(at, s)) = pauses.peek() {
            if at > i {
                break;
            }
            t += s;
            pauses.next();
        }
        if elapsed < t + step {
            return (i, ((elapsed - t) / step).clamp(0.0, 1.0));
        }
        t += step;
    }
    (total, 0.0)
}

/// Advance every rich text's typewriter by `dt` - start it over when its
/// string changed. Run by every host inside `behaviors::tick_motion`.
pub fn tick_reveals(world: &mut hecs::World, dt: f32) {
    for (_, t) in world.query_mut::<&mut TextComponent>() {
        if t.reveal_speed > 0.0 {
            tick_reveal(&mut t.reveal, &t.text, dt);
        }
    }
    // Entity-UI text has the same typewriter (Oct 2026).
    for (_, t) in world.query_mut::<&mut crate::ui::UIText>() {
        if t.reveal_speed > 0.0 {
            tick_reveal(&mut t.reveal, &t.text, dt);
        }
    }
}

fn tick_reveal(reveal: &mut Option<crate::ecs::components::TextReveal>, text: &str, dt: f32) {
    let key = text_key(text);
    match reveal {
        Some(r) if r.key == key => r.elapsed += dt,
        other => *other = Some(crate::ecs::components::TextReveal { key, elapsed: 0.0 }),
    }
}

pub(crate) fn text_key(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// What an effect does to the glyph of character `index` at `time`
/// seconds: an offset in line heights (x, y), and a colour in place of the
/// glyph's own when it is a rainbow. A rainbow keeps the brightness of
/// `base` (the colour the glyph would have had): white text cycles through
/// bright hues, dark ink on paper through deep ones - one bright rainbow
/// reads on a dark scene and vanishes in yellow on a page.
pub fn effect(style: &CharStyle, index: usize, time: f32, base: [f32; 4]) -> (glam::Vec2, Option<[f32; 4]>) {
    let mut offset = glam::Vec2::ZERO;
    let i = index as f32;
    if style.wave {
        offset.y += (time * 6.0 - i * 0.55).sin() * 0.09;
    }
    if style.shake {
        // A new place a few times a second, the same for every viewer of a
        // frame: a hash of the character and the tick, not a random draw.
        let tick = (time * 22.0).floor();
        // `rem_euclid`, not `fract`: a negative sine has a negative fract,
        // which made the jitter -0.12..0.04 of a line instead of +-0.04 -
        // three times as wide, and mostly left, onto the glyph before.
        let h = |k: f32| ((i * 12.9898 + tick * 78.233 + k).sin() * 43758.547).rem_euclid(1.0);
        offset += glam::Vec2::new(h(1.7) - 0.5, h(9.1) - 0.5) * 0.08;
    }
    let color = style.rainbow.then(|| {
        let hue = (time * 0.35 + i * 0.07).fract();
        // sRGB value: 0.45 for black ink, 1 for white.
        let bright = base[0].max(base[1]).max(base[2]).clamp(0.0, 1.0).powf(1.0 / 2.2);
        let rgb = hsv(hue, 0.55 + 0.35 * (1.0 - bright), 0.45 + 0.55 * bright);
        [rgb[0], rgb[1], rgb[2], 1.0]
    });
    (offset, color)
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let k = |n: f32| {
        let k = (n + h * 6.0) % 6.0;
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    // Linear, as every colour here is.
    [k(5.0).powf(2.2), k(3.0).powf(2.2), k(1.0).powf(2.2)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_what_is_left_of_the_markup() {
        let r = parse("a [color=red]big[/color] [b]bold[/b] word", 3);
        assert_eq!(r.plain, "a big bold word");
        assert_eq!(r.styles.len(), r.plain.chars().count());
        assert!(r.styles[2].color.is_some() && r.styles[4].color.is_some());
        assert!(r.styles[1].color.is_none() && r.styles[5].color.is_none());
        assert!(r.styles[6].bold && !r.styles[11].bold);
    }

    #[test]
    fn unknown_tags_and_double_brackets_stay_text() {
        assert_eq!(parse("[[not a tag] and [nope]", 3).plain, "[not a tag] and [nope]");
        assert_eq!(parse("[color=notacolour]x[/color]", 3).plain, "[color=notacolour]x");
    }

    #[test]
    fn colours_are_srgb_in_the_markup_and_linear_after() {
        let c = parse_color("#808080").unwrap();
        assert!((c[0] - 0.2158).abs() < 1e-3, "mid grey is 0.216 linear: {c:?}");
        assert_eq!(parse_color("#fff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert!(parse_color("gold").is_some());
    }

    #[test]
    fn nested_colours_come_back() {
        let r = parse("[color=red]a[color=blue]b[/color]c[/color]", 3);
        assert_eq!(r.styles[0].color, r.styles[2].color);
        assert_ne!(r.styles[0].color, r.styles[1].color);
    }

    #[test]
    fn an_icon_takes_spaces_and_remembers_its_texture() {
        let m = "a key [icon=sprites/key.png] here";
        let r = parse(m, 4);
        assert_eq!(r.plain, "a key      here");
        assert_eq!(r.icons.len(), 1);
        let icon = &r.icons[0];
        assert_eq!((icon.at, icon.len), (6, 4));
        assert_eq!(&m[icon.id.clone()], "sprites/key.png");
    }

    #[test]
    fn the_typewriter_waits_at_pauses() {
        let r = parse("ab[pause=1]cd", 3);
        assert_eq!(r.pauses, vec![(2, 1.0)]);
        // Ten a second: a and b by 0.2 s, then a second's wait.
        assert_eq!(visible_chars(&r, 10.0, Some(0.05)).0, 0);
        assert_eq!(visible_chars(&r, 10.0, Some(0.15)).0, 1);
        assert_eq!(visible_chars(&r, 10.0, Some(0.25)).0, 2);
        assert_eq!(visible_chars(&r, 10.0, Some(1.15)).0, 2, "still waiting");
        assert_eq!(visible_chars(&r, 10.0, Some(1.35)).0, 3);
        assert_eq!(visible_chars(&r, 10.0, Some(9.0)).0, 4);
        assert_eq!(visible_chars(&r, 10.0, None).0, 4, "never ticked: all of it");
        assert_eq!(visible_chars(&r, 0.0, Some(0.0)).0, 4, "no speed: all of it");
    }

    #[test]
    fn new_text_starts_the_reveal_over() {
        let mut world = hecs::World::new();
        let e = world.spawn((TextComponent { text: "hello".into(), rich: true, reveal_speed: 10.0, ..Default::default() },));
        tick_reveals(&mut world, 0.1);
        tick_reveals(&mut world, 0.3);
        assert!((world.get::<&TextComponent>(e).unwrap().reveal.as_ref().unwrap().elapsed - 0.3).abs() < 1e-6);
        world.get::<&mut TextComponent>(e).unwrap().text = "goodbye".into();
        tick_reveals(&mut world, 0.1);
        assert_eq!(world.get::<&TextComponent>(e).unwrap().reveal.as_ref().unwrap().elapsed, 0.0);
    }

    #[test]
    fn effects_move_only_what_asks() {
        let plain = CharStyle::default();
        assert_eq!(effect(&plain, 3, 1.7, [1.0; 4]), (glam::Vec2::ZERO, None));
        let wave = CharStyle { wave: true, ..Default::default() };
        let (a, _) = effect(&wave, 0, 0.3, [1.0; 4]);
        let (b, _) = effect(&wave, 5, 0.3, [1.0; 4]);
        assert!(a.y != b.y && a.x == 0.0, "a wave travels along the letters");
        let rainbow = CharStyle { rainbow: true, ..Default::default() };
        let on_white = effect(&rainbow, 0, 0.0, [1.0; 4]).1.unwrap();
        let on_ink = effect(&rainbow, 0, 0.0, [0.02, 0.01, 0.0, 1.0]).1.unwrap();
        let peak = |c: [f32; 4]| c[0].max(c[1]).max(c[2]);
        assert!(peak(on_ink) < peak(on_white) * 0.5, "dark ink makes a deep rainbow, white a bright one");
    }

    /// A shake jitters each glyph within 0.04 of a line either way, centred
    /// - it reached 0.12 to the left, onto the glyph before, and a colon
    /// after a word vanished into it now and then (The Quest Journal).
    #[test]
    fn a_shake_stays_small_and_centred() {
        let shake = CharStyle { shake: true, ..Default::default() };
        let (mut lo, mut hi, mut sum) = (glam::Vec2::splat(1.0), glam::Vec2::splat(-1.0), glam::Vec2::ZERO);
        let mut n = 0.0;
        for index in 0..40 {
            for tick in 0..200 {
                let o = effect(&shake, index, tick as f32 / 22.0 + 0.01, [1.0; 4]).0;
                lo = lo.min(o);
                hi = hi.max(o);
                sum += o;
                n += 1.0;
            }
        }
        assert!(lo.min_element() >= -0.04 - 1e-6 && hi.max_element() <= 0.04 + 1e-6, "{lo:?} .. {hi:?}");
        assert!((sum / n).abs().max_element() < 0.005, "centred: mean {:?}", sum / n);
    }
}
