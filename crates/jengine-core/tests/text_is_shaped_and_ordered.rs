//! **Shaped and bidirectional text** (Oct 2026; `renderer::dynamic_font`,
//! `rustybuzz` and `unicode-bidi`). A font file drew every letter on its own
//! and left to right: Arabic unjoined, undotted and backwards, Hebrew
//! backwards, Devanagari's vowel signs after the consonant they belong
//! before. A line needing it is now shaped into glyphs in visual order -
//! private-use characters standing for each glyph with its offset and
//! advance, which the atlas bakes by glyph index - so layout and drawing are
//! unchanged. Found by the showcase's Bazaar Signs.

use jengine_core::asset::AssetManager;
use jengine_core::renderer::dynamic_font::{self, GLYPH_BASE, Line, ShapedGlyph};

const ARABIC: &[u8] = include_bytes!("../../../projects/engine-test-lab/assets/fonts/noto/NotoSansArabic-Regular-subset.ttf");
const HEBREW: &[u8] = include_bytes!("../../../projects/engine-test-lab/assets/fonts/noto/NotoSansHebrew-Regular-subset.ttf");
const DEVANAGARI: &[u8] =
    include_bytes!("../../../projects/engine-test-lab/assets/fonts/noto/NotoSansDevanagari-Regular-subset.ttf");

/// The glyph a font maps `c` to on its own.
fn nominal(font: &[u8], c: char) -> u16 {
    rustybuzz::Face::from_slice(font, 0).unwrap().glyph_index(c).unwrap().0
}

/// The one line of `text` shaped, as glyphs (a space as `None`).
fn shaped(font: &[u8], text: &str) -> Vec<Option<ShapedGlyph>> {
    match dynamic_font::shape(font, text).remove(0) {
        Line::Shaped(g) => g,
        Line::Plain(t) => panic!("'{t}' was not shaped"),
    }
}

fn ids(g: &[Option<ShapedGlyph>]) -> Vec<Option<u16>> {
    g.iter().map(|g| g.map(|g| g.glyph)).collect()
}

#[test]
fn latin_is_left_alone() {
    assert!(matches!(dynamic_font::display_text("fonts/none.ttf", "Hello, world"), std::borrow::Cow::Borrowed(_)));
    assert_eq!(dynamic_font::shape(ARABIC, "plain ASCII"), vec![Line::Plain("plain ASCII".into())]);
}

#[test]
fn hebrew_runs_right_to_left() {
    // "shalom": shin, lamed, vav, final mem - drawn mem first, at the left.
    let g = ids(&shaped(HEBREW, "שלום"));
    assert_eq!(g.len(), 4);
    assert_eq!(g[0], Some(nominal(HEBREW, 'ם')), "the last letter is the leftmost glyph");
    assert_eq!(g[3], Some(nominal(HEBREW, 'ש')), "and the first the rightmost");
}

#[test]
fn arabic_joins_and_places_its_dots() {
    // "shai" (tea): sheen, alef, yeh. Noto draws the dots as marks, placed
    // over (or under) their letter by an offset and advancing nothing.
    let g = shaped(ARABIC, "شاي");
    let bases: Vec<u16> = g.iter().flatten().filter(|g| g.advance > 0).map(|g| g.glyph).collect();
    let marks: Vec<ShapedGlyph> = g.iter().flatten().filter(|g| g.advance == 0).copied().collect();
    assert_eq!(bases.len(), 3, "three letters: {g:?}");
    assert!(!marks.is_empty() && marks.iter().all(|m| m.dx != 0 || m.dy != 0), "the dots are placed by offsets: {marks:?}");
    // Visual order: sheen, the first letter, is the rightmost base - in its
    // initial form, not its isolated one.
    assert_ne!(*bases.last().unwrap(), nominal(ARABIC, 'ش'), "sheen takes its joined form");
}

#[test]
fn a_mixed_line_keeps_latin_left_to_right() {
    // A left-to-right paragraph with an Arabic word in it: the Latin first,
    // in order, then the Arabic run reversed.
    let g = ids(&shaped(ARABIC, "Price 35 درهم"));
    assert_eq!(g[0], Some(nominal(ARABIC, 'P')));
    assert_eq!(g[1], Some(nominal(ARABIC, 'r')));
    assert_eq!(g[5], None, "a space stays a space, so wrapping still breaks there");
}

#[test]
fn devanagari_reorders_its_vowel_sign() {
    // "ki": ka then the i-sign, which is written before the consonant.
    let g = ids(&shaped(DEVANAGARI, "कि"));
    assert_eq!(g.len(), 2);
    assert_ne!(g[0], Some(nominal(DEVANAGARI, 'क')), "the vowel sign comes first: {g:?}");
    assert_eq!(g[1], Some(nominal(DEVANAGARI, 'क')));
}

#[test]
fn a_font_file_bakes_and_lays_out_shaped_text() {
    use hecs::World;
    use jengine_core::ecs::components::{TextComponent, Transform};
    use jengine_core::renderer::text_layout;
    let id = "fonts/noto/NotoSansArabic-Regular-subset.ttf";
    let mut assets = AssetManager::new();
    dynamic_font::insert_font_file(&mut assets, id, ARABIC).unwrap();
    let mut world = World::new();
    world.spawn((Transform::default(), TextComponent { text: "قهوة".into(), font_id: id.into(), ..Default::default() }));
    dynamic_font::sync(&world, &mut assets);
    let shown = dynamic_font::display_text(id, "قهوة").into_owned();
    assert!(shown.chars().all(|c| c as u32 >= GLYPH_BASE), "glyph characters: {shown:?}");
    let baked = dynamic_font::baked(&assets, id);
    for c in shown.chars() {
        assert!(baked.contains(&c), "glyph {:x} is baked", c as u32);
    }
    let font = assets.get_font_data(id).unwrap();
    assert_eq!(font.shaping.as_deref(), Some(id));
    let lines = text_layout::layout_text(&font, "قهوة", 1.0, None);
    assert_eq!(lines[0].text, shown, "layout draws the display string");
    assert!(lines[0].width > 10.0, "measured by the shaped advances: {}", lines[0].width);
    // A mark advances nothing.
    let marks = font.glyphs.iter().filter(|g| g.ch as u32 >= GLYPH_BASE && g.advance == 0.0).count();
    assert!(marks > 0, "the dots are zero-advance glyphs");
    assert_eq!(dynamic_font::display_text(id, &shown), shown, "shaping a display string again leaves it");
}
