//! The editor's sheet slicer writes an `.atlas` of packed metadata beside an
//! image (`editor/src/lib/sheetSlicing.ts`, `atlasJson`). The fixture here is
//! what it writes for a two-cell sheet - `sheetSlicing.test.ts` checks the
//! editor produces exactly it - and this test checks the engine reads it as
//! the `AtlasData` a scene's `atlas_refs` load: regions by name with the
//! right UV rect, and a clip of them.

use jengine_core::renderer::atlas::AtlasData;

#[test]
fn sliced_sheets_load_as_atlases() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sliced_sheet.atlas"))
        .expect("the fixture");
    let atlas: AtlasData = serde_json::from_str(&text).expect("packed metadata the engine reads");
    assert_eq!((atlas.width, atlas.height), (64, 32));
    assert_eq!(atlas.get_uv_rect("hero_1"), Some([0.5, 0.0, 1.0, 1.0]));
    let region = atlas.get_region("hero_0").expect("a region by name");
    assert_eq!(region.anchor, [0.5, 0.5], "the default pivot");
    let clip = &atlas.animation_clips[0];
    assert_eq!(clip.name, "walk");
    assert_eq!(clip.frames.iter().map(|f| f.region_name.as_str()).collect::<Vec<_>>(), ["hero_0", "hero_1"]);
}
