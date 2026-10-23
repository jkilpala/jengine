//! Fonts straight from TTF / OTF files (Oct 2026): a `font_id` naming a font
//! file draws in it - any script the file covers - its glyphs baked into a
//! distance-field atlas the first time a text asks for them
//! (`renderer::dynamic_font`).

use std::path::PathBuf;

use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::TextComponent;
use jengine_core::renderer::dynamic_font::{self, baked, insert_font_file, is_font_file};
use jengine_core::renderer::font_asset::{collect_font_ids, font_texture_id};
use jengine_core::renderer::text_layout::measure_line;

const JP: &str = "fonts/noto/NotoSansJP-Regular-subset.ttf";
const LATIN: &str = "fonts/noto/NotoSans-Regular-subset.ttf";

fn bytes(id: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../projects/engine-test-lab/assets").join(id);
    std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn text(font_id: &str, s: &str) -> TextComponent {
    TextComponent { text: s.into(), font_id: font_id.into(), ..Default::default() }
}

#[test]
fn a_font_file_loads_with_ascii_and_bakes_the_rest_on_demand() {
    dynamic_font::clear();
    let mut assets = AssetManager::new();
    assert!(is_font_file(JP) && is_font_file("a/B.OTF") && !is_font_file("fonts/ui.font"));
    insert_font_file(&mut assets, JP, &bytes(JP)).unwrap();
    let atlas = font_texture_id(JP);
    assert_eq!(atlas, format!("{JP}#atlas"), "an id no file has, so nothing fetches it");
    let ascii = baked(&assets, JP);
    assert!(ascii.contains(&'A') && ascii.contains(&'?') && !ascii.contains(&'こ'));
    let font = assets.get_font_data(JP).unwrap();
    assert!(font.sdf && font.line_height > 10.0 && font.pixel_size == 16.0);

    // A world text in Japanese: its glyphs are baked by the frame's sync.
    let mut world = World::new();
    world.spawn((text(JP, "こんにちは世界"),));
    world.spawn((text(JP, "Ω"),)); // not in this font
    assert!(collect_font_ids(&world).contains(JP));
    dynamic_font::sync(&world, &mut assets);
    let now = baked(&assets, JP);
    for ch in "こんにちは世界".chars() {
        assert!(now.contains(&ch), "'{ch}' was not baked");
    }
    assert!(!now.contains(&'Ω'), "a character the font has not got is left to its '?'");

    // Each glyph has ink in the atlas, and the atlas says where it changed.
    let data = assets.get_font_data(JP).unwrap().clone();
    let raw = assets.get_texture_bytes(&atlas).unwrap();
    assert_eq!((raw.width, raw.height), (data.atlas_width, data.atlas_height));
    let g = data.glyphs.iter().find(|g| g.ch == '世').unwrap();
    let inked = (0..g.height).any(|row| {
        (0..g.width).any(|col| raw.rgba[(((g.y + row) * raw.width + g.x + col) * 4 + 3) as usize] > 128)
    });
    assert!(inked, "the glyph's field has an inside");
    assert!(g.advance > 10.0, "a full-width glyph advances most of a layout em: {}", g.advance);
    // Laid out, the line uses the real glyphs, not the '?' fallback.
    let wide = measure_line(&data, "世界", 1.0);
    let q = data.glyphs.iter().find(|g| g.ch == '?').unwrap().advance * 2.0;
    assert!(wide > q * 1.3, "{wide} vs two '?' at {q}");

    // A second sync with nothing new does nothing.
    dynamic_font::sync(&world, &mut assets);
    assert_eq!(assets.get_font_data(JP).unwrap().glyphs.len(), data.glyphs.len());
}

#[test]
fn entity_ui_text_reaches_its_font_too() {
    dynamic_font::clear();
    let mut assets = AssetManager::new();
    insert_font_file(&mut assets, LATIN, &bytes(LATIN)).unwrap();
    let mut world = World::new();
    world.spawn((jengine_core::ui::UIText { text: "Привет".into(), font_id: LATIN.into(), ..Default::default() },));
    assert!(collect_font_ids(&world).contains(LATIN), "entity UI's fonts are collected for loading");
    dynamic_font::sync(&world, &mut assets);
    assert!("Привет".chars().all(|c| baked(&assets, LATIN).contains(&c)));
}

#[test]
fn the_atlas_grows_when_its_shelves_fill() {
    dynamic_font::clear();
    let mut assets = AssetManager::new();
    insert_font_file(&mut assets, LATIN, &bytes(LATIN)).unwrap();
    let before = assets.get_font_data(LATIN).unwrap().atlas_height;
    let mut world = World::new();
    // Every character the subset has beyond ASCII.
    world.spawn((text(LATIN, "Привет, мир — Γειά σου κόσμε — Ünïcödé Чайхана Базар Самовар Ταβέρνα Αγορά Шёлк Καφές"),));
    dynamic_font::sync(&world, &mut assets);
    let data = assets.get_font_data(LATIN).unwrap();
    let raw = assets.get_texture_bytes(&font_texture_id(LATIN)).unwrap();
    assert!(data.atlas_height > before, "{} glyphs should not fit {before} rows", data.glyphs.len());
    assert_eq!(raw.height, data.atlas_height, "the image grew with the metadata");
    assert!(data.glyphs.iter().all(|g| g.y + g.height <= data.atlas_height));
}

#[test]
fn gui_widgets_reach_their_font_too() {
    use jengine_core::ecs::components::GuiComponent;
    use jengine_core::ui::gui_document::GuiDocument;
    dynamic_font::clear();
    let mut assets = AssetManager::new();
    insert_font_file(&mut assets, JP, &bytes(JP)).unwrap();
    let doc: GuiDocument = serde_json::from_value(serde_json::json!({
        "name": "t", "anchor": "Center",
        "root": { "id": "root", "node_type": { "type": "Panel" }, "children": [
            { "id": "b", "node_type": { "type": "Button", "label": "寿司", "font_id": JP } }
        ] }
    }))
    .unwrap();
    assets.insert_gui("ui/t.gui".into(), doc);
    let mut world = World::new();
    world.spawn((serde_json::from_value::<GuiComponent>(serde_json::json!({ "gui_id": "ui/t.gui" })).unwrap(),));
    dynamic_font::sync(&world, &mut assets);
    assert!("寿司".chars().all(|c| baked(&assets, JP).contains(&c)), "a button label in a font file is baked");
}
