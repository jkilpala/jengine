//! `.font` asset loading and per-entity font resolution.
//!
//! A bitmap font is two files: a `.font` JSON descriptor that deserializes
//! straight into [`FontData`], and its atlas image. The descriptor carries no
//! texture reference — the image is the **sibling file with the same stem**
//! (`fonts/ui.font` → `fonts/ui.png`), which is the convention the editor's
//! font tool and jSuite both write. [`font_texture_id`] is the single place
//! that pairing is spelled out; every runtime derives the image id with it so
//! the three of them cannot drift.
//!
//! Unlike `.atlas` — which needs a scene-level `atlas_refs` array because
//! nothing in the scene names an atlas — a font is named by the entities that
//! use it, in [`TextComponent::font_id`]. So discovery follows the `.gui`
//! model instead: [`collect_font_ids`] scans the world for referenced ids and
//! [`load_fonts`] loads exactly those. There is no new scene field to author
//! and no way for a refs list to fall out of sync with the entities.
//!
//! An empty `font_id` means the built-in font - and so does the literal
//! `"default"`, an older authoring convention that no code had ever read.
//! [`is_builtin_font_id`] is the one place that knows, which is why
//! [`collect_font_ids`] skips those ids and [`FrameFonts::resolve`] falls back.
//!
//! **Keep font atlases out of `assets/sprites/`.** Both builders copy the
//! whole asset tree, so a font ships wherever it sits - but the asset pipeline
//! runs atlas packing and image compression over `assets/sprites/` only. An
//! atlas repacked into a sheet, or re-encoded as JPEG, keeps its filename and
//! loses every glyph rect the `.font` describes. `assets/fonts/` is the
//! convention.
//!
//! [`TextComponent::font_id`]: crate::ecs::components::TextComponent::font_id

use std::collections::{HashMap, HashSet};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use crate::asset::AssetManager;
use crate::ecs::components::TextComponent;
use crate::renderer::text::FontData;
use std::sync::Arc;
use hecs::World;

/// The texture id holding `font_id`'s atlas image: the sibling file with the
/// same stem and a `.png` extension.
///
/// Pure and total so every runtime agrees on the pairing without needing the
/// filesystem. An id with no extension gets `.png` appended; backslashes are
/// normalized so a Windows-authored path keys the same as a POSIX one.
pub fn font_texture_id(font_id: &str) -> String {
    let normalized = font_id.replace('\\', "/");
    // A font file's atlas is made at runtime (`renderer::dynamic_font`), not
    // read from a sibling image: an id no file has, so nothing fetches it.
    if is_font_file(&normalized) {
        return format!("{normalized}#atlas");
    }
    // Only an extension on the *last* segment counts - "a.b/font" has no
    // extension, and treating the '.' in the directory as one would produce
    // a texture id pointing at a different directory entirely.
    match normalized.rsplit_once('/') {
        Some((dir, file)) => match file.rsplit_once('.') {
            Some((stem, _)) if !stem.is_empty() => format!("{dir}/{stem}.png"),
            _ => format!("{normalized}.png"),
        },
        None => match normalized.rsplit_once('.') {
            Some((stem, _)) if !stem.is_empty() => format!("{stem}.png"),
            _ => format!("{normalized}.png"),
        },
    }
}

/// Whether `font_id` names the built-in face rather than an asset to load.
///
/// Empty is the documented spelling. `"default"` is the other one: scenes were
/// authored with it long before anything read `font_id` at all, so it meant
/// nothing and did nothing. Once fonts actually loaded, those 43 authored ids
/// became requests for a file named `default` - a 404 on every scene load in a
/// web build, and a warning natively, for text that was rendering correctly.
///
/// Treated as a sentinel rather than rewritten in the scenes, because the
/// spelling is already out in projects this repo does not own. The cost is
/// that a font asset genuinely named `default.font` cannot be reached; name
/// it anything else.
pub fn is_builtin_font_id(font_id: &str) -> bool {
    font_id.is_empty() || font_id == "default"
}

/// Every `TextComponent.font_id` in `world` that names a real asset.
///
/// Shared by all three runtimes so "which fonts does this scene need" has one
/// answer. See [`is_builtin_font_id`] for the ids that need no asset.
pub fn collect_font_ids(world: &World) -> HashSet<String> {
    let mut wanted = HashSet::new();
    for (_, tc) in world.query::<&TextComponent>().iter() {
        if !is_builtin_font_id(&tc.font_id) {
            wanted.insert(tc.font_id.clone());
        }
    }
    // Entity-UI text names fonts too; it was never scanned, so a `ui_text`
    // with a font of its own fell back to the built-in face everywhere.
    for (_, t) in world.query::<&crate::ui::UIText>().iter() {
        if !is_builtin_font_id(&t.font_id) {
            wanted.insert(t.font_id.clone());
        }
    }
    wanted
}

/// Add every font id a GUI document's text-bearing widgets name.
///
/// `.gui` `Text`, `Button` and `TextInput` nodes each carry their own
/// `font_id`, so a document can name a font no `TextComponent` uses. Without
/// this the font would never be loaded and every widget would quietly fall
/// back to the built-in face - the failure mode being "it works in the scene
/// text but not in the HUD".
///
/// Takes `out` rather than returning, because callers union this across every
/// visible document before loading.
pub fn collect_gui_font_ids(doc: &crate::ui::gui_document::GuiDocument, out: &mut HashSet<String>) {
    use crate::ui::gui_document::{GuiNode, GuiNodeType};

    fn walk(node: &GuiNode, out: &mut HashSet<String>) {
        let font_id = match &node.node_type {
            GuiNodeType::Text { font_id, .. }
            | GuiNodeType::Button { font_id, .. }
            | GuiNodeType::TextInput { font_id, .. } => Some(font_id),
            _ => None,
        };
        if let Some(id) = font_id
            && !is_builtin_font_id(id)
        {
            out.insert(id.clone());
        }
        for child in &node.children {
            walk(child, out);
        }
    }

    walk(&doc.root, out);
}

/// Parse a `.font` descriptor and register it under `font_id`.
///
/// The counterpart for hosts that have no filesystem: the editor viewport and
/// the web runtime receive the two files over their own channels (a
/// `wasm_bindgen` push and an HTTP fetch) and call this with the JSON, having
/// uploaded the image under [`font_texture_id`] themselves. Native goes
/// through [`load_fonts`], which is this plus the disk reads.
pub fn insert_font_json(
    assets: &mut AssetManager,
    font_id: &str,
    json: &str,
) -> anyhow::Result<()> {
    let data: FontData = serde_json::from_str(json)
        .map_err(|e| anyhow::anyhow!("parsing .font '{font_id}': {e}"))?;
    assets.insert_font_data(font_id.to_string(), data);
    Ok(())
}

/// Load each id in `wanted` from disk, resolving against `roots` in order.
///
/// For each font: parse the `.font` into [`FontData`], then load the sibling
/// image into the texture cache under [`font_texture_id`]. Both land or
/// neither does — a font whose image is missing would otherwise render every
/// glyph from whatever texture happened to be bound.
///
/// Returns one error per font that failed and never short-circuits, so one bad
/// font does not cost a scene the rest of its text. Fonts already loaded are
/// skipped, which makes this safe to call on every scene load.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_fonts(
    wanted: &HashSet<String>,
    roots: &[PathBuf],
    assets: &mut AssetManager,
) -> Vec<anyhow::Error> {
    let mut errors = Vec::new();

    for font_id in wanted {
        if assets.has_font_data(font_id) {
            continue;
        }
        let Some(path) = crate::asset::find_in_roots(font_id, roots) else {
            errors.push(anyhow::anyhow!(
                "font '{font_id}' not found in any asset root"
            ));
            continue;
        };

        // A TTF / OTF is the font itself; its atlas is made as text asks.
        if is_font_file(font_id) {
            #[cfg(not(feature = "dynamic_font"))]
            crate::features::missing("dynamic_font", "a TTF / OTF font");
            #[cfg(feature = "dynamic_font")]
            match std::fs::read(&path) {
                Ok(bytes) => {
                    if let Err(e) = crate::renderer::dynamic_font::insert_font_file(assets, font_id, &bytes) {
                        errors.push(anyhow::anyhow!("parsing font {}: {e}", path.display()));
                    }
                }
                Err(e) => errors.push(anyhow::anyhow!("reading font {}: {e}", path.display())),
            }
            continue;
        }
        let json = match std::fs::read_to_string(&path) {
            Ok(j) => j,
            Err(e) => {
                errors.push(anyhow::anyhow!("reading .font {}: {e}", path.display()));
                continue;
            }
        };
        let data: FontData = match serde_json::from_str(&json) {
            Ok(d) => d,
            Err(e) => {
                errors.push(anyhow::anyhow!("parsing .font {}: {e}", path.display()));
                continue;
            }
        };

        // The image is the sibling by stem. Resolve it from the *found* path
        // rather than re-searching the roots: a font and its atlas that live
        // in different roots would be a packaging accident, not a feature.
        let image_path = path.with_extension("png");
        if !image_path.is_file() {
            errors.push(anyhow::anyhow!(
                "font '{font_id}' is missing its sibling atlas image at {}",
                image_path.display()
            ));
            continue;
        }
        let texture_id = font_texture_id(font_id);
        if let Err(e) = assets.load_texture_bytes(&texture_id, &image_path.to_string_lossy()) {
            errors.push(anyhow::anyhow!(
                "loading font atlas {}: {e}",
                image_path.display()
            ));
            continue;
        }

        assets.insert_font_data(font_id.clone(), data);
        log::info!(
            "[font] loaded '{font_id}' from {} (atlas {})",
            path.display(),
            image_path.display()
        );
    }

    errors
}

/// The fonts a frame can draw with, lifted out of the [`AssetManager`] so the
/// render path can hold them while the draw closures hold `&mut` of it.
///
/// This is the same "resolve before building the render closures" move the GUI
/// path makes with its documents, for the same borrow reason - but a font is
/// shared by `Arc`, so gathering costs a refcount bump per font instead of
/// copying a few hundred glyph records every frame.
///
/// The built-in font is borrowed rather than shared: every runtime keeps it in
/// a field of its own state struct, which does not alias the asset manager.
pub struct FrameFonts<'a> {
    builtin: &'a FontData,
    builtin_texture_id: &'a str,
    custom: HashMap<String, (Arc<FontData>, String)>,
}

impl<'a> FrameFonts<'a> {
    /// Take a shared handle to every loaded font.
    ///
    /// Call this *before* the flush closures capture `&mut assets`. Scenes
    /// load only the fonts their entities name, so this map is typically
    /// empty or a single entry.
    pub fn gather(
        assets: &AssetManager,
        builtin: &'a FontData,
        builtin_texture_id: &'a str,
    ) -> Self {
        let custom = assets
            .font_ids()
            .filter_map(|id| {
                let data = assets.get_font_shared(id)?;
                Some((id.clone(), (data, font_texture_id(id))))
            })
            .collect();
        Self { builtin, builtin_texture_id, custom }
    }

    /// A [`FrameFonts`] holding only the built-in font.
    ///
    /// For hosts and preview paths that never load a custom font, so they do
    /// not need an [`AssetManager`] in hand to call the render systems.
    pub fn builtin_only(builtin: &'a FontData, builtin_texture_id: &'a str) -> Self {
        Self { builtin, builtin_texture_id, custom: HashMap::new() }
    }

    /// The font `font_id` should draw with, and the texture holding its
    /// glyphs.
    ///
    /// Falls back to the built-in font for an empty id (the documented "use
    /// the default" case) and for an id that failed to load, so a missing font
    /// costs the styling rather than the text - text that silently vanishes is
    /// far harder to diagnose than text in the wrong face. Returning the pair
    /// together is what keeps the glyph rects and the bound atlas from ever
    /// disagreeing.
    pub fn resolve(&self, font_id: &str) -> (&FontData, &str) {
        if is_builtin_font_id(font_id) {
            return (self.builtin, self.builtin_texture_id);
        }
        match self.custom.get(font_id) {
            Some((data, texture_id)) => (data.as_ref(), texture_id.as_str()),
            None => (self.builtin, self.builtin_texture_id),
        }
    }

    /// The built-in font, for callers that never had a `font_id` to begin with.
    pub fn builtin(&self) -> &FontData {
        self.builtin
    }

    /// The texture id of the built-in font atlas.
    pub fn builtin_texture_id(&self) -> &str {
        self.builtin_texture_id
    }
}


/// Whether `font_id` names a font file this module loads.
pub fn is_font_file(font_id: &str) -> bool {
    let lower = font_id.to_ascii_lowercase();
    lower.ends_with(".ttf") || lower.ends_with(".otf")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::Transform;

    #[test]
    fn texture_id_is_the_sibling_png() {
        assert_eq!(font_texture_id("fonts/ui.font"), "fonts/ui.png");
        assert_eq!(font_texture_id("ui.font"), "ui.png");
        // Backslashes normalize, so a Windows-authored scene keys the same.
        assert_eq!(font_texture_id("fonts\\ui.font"), "fonts/ui.png");
    }

    #[test]
    fn a_dot_in_a_directory_is_not_an_extension() {
        // Naive `rsplit_once('.')` would produce "v1/ui.png" here, pointing at
        // a directory that does not exist.
        assert_eq!(font_texture_id("fonts.v1/ui.font"), "fonts.v1/ui.png");
        assert_eq!(font_texture_id("fonts/ui"), "fonts/ui.png");
    }

    #[test]
    fn collect_skips_the_builtin_and_deduplicates() {
        let mut world = World::new();
        let text = |id: &str| TextComponent {
            text: "hi".to_string(),
            font_id: id.to_string(),
            ..Default::default()
        };
        world.spawn((Transform::default(), text("fonts/a.font")));
        world.spawn((Transform::default(), text("fonts/a.font")));
        world.spawn((Transform::default(), text("")));
        // The other spelling of "built-in". Scenes were authored with it long
        // before anything read `font_id`, and once fonts loaded it became a
        // request for a file named `default` - a 404 per scene load on web.
        world.spawn((Transform::default(), text("default")));

        let ids = collect_font_ids(&world);
        assert_eq!(ids.len(), 1, "{ids:?}");
        assert!(ids.contains("fonts/a.font"));
    }

    #[test]
    fn the_default_sentinel_names_no_asset() {
        assert!(is_builtin_font_id(""));
        assert!(is_builtin_font_id("default"));
        // Only the bare word. A real file keeps its extension and is loaded.
        assert!(!is_builtin_font_id("default.font"));
        assert!(!is_builtin_font_id("fonts/ui.font"));
    }

    #[test]
    fn gui_widget_fonts_are_discovered_too() {
        // A `.gui` can name a font no `TextComponent` uses. Missing this is
        // the "works in scene text, built-in face in the HUD" bug.
        let doc = crate::ui::gui_document::load_gui(
            r#"{
                "name": "t",
                "root": {
                    "id": "root",
                    "node_type": { "type": "Panel" },
                    "children": [
                        { "id": "a", "node_type": { "type": "Text",
                          "text": "hi", "font_id": "fonts/a.font" },
                          "children": [] },
                        { "id": "b", "node_type": { "type": "Button",
                          "label": "go", "font_id": "fonts/b.font" },
                          "children": [
                            { "id": "c", "node_type": { "type": "TextInput",
                              "font_id": "fonts/c.font" }, "children": [] }
                          ] },
                        { "id": "d", "node_type": { "type": "Text",
                          "text": "plain", "font_id": "" }, "children": [] }
                    ]
                }
            }"#,
        )
        .expect("gui parses");

        let mut ids = HashSet::new();
        collect_gui_font_ids(&doc, &mut ids);
        let mut got: Vec<_> = ids.into_iter().collect();
        got.sort();
        // Nested widgets counted, the empty id skipped.
        assert_eq!(got, vec!["fonts/a.font", "fonts/b.font", "fonts/c.font"]);
    }

    #[test]
    fn an_empty_font_id_resolves_to_the_builtin() {
        let assets = AssetManager::new();
        let builtin = tiny_font();
        let fonts = FrameFonts::gather(&assets, &builtin, "__font/builtin");
        let (data, tex) = fonts.resolve("");
        assert_eq!(tex, "__font/builtin");
        assert_eq!(data.line_height, builtin.line_height);
    }

    #[test]
    fn an_unloaded_font_falls_back_rather_than_dropping_the_text() {
        // A missing font must cost the styling, not the string - text that
        // vanishes is far harder to diagnose than text in the wrong face.
        let assets = AssetManager::new();
        let builtin = tiny_font();
        let fonts = FrameFonts::gather(&assets, &builtin, "__font/builtin");
        assert_eq!(fonts.resolve("fonts/nope.font").1, "__font/builtin");
    }

    #[test]
    fn a_loaded_font_wins_and_brings_its_own_atlas() {
        let mut assets = AssetManager::new();
        let builtin = tiny_font();
        let mut custom = tiny_font();
        custom.line_height = 99.0;
        assets.insert_font_data("fonts/ui.font".to_string(), custom);

        let fonts = FrameFonts::gather(&assets, &builtin, "__font/builtin");
        let (data, tex) = fonts.resolve("fonts/ui.font");
        assert_eq!(data.line_height, 99.0);
        assert_eq!(tex, "fonts/ui.png");
    }

    #[test]
    fn gathered_fonts_outlive_the_asset_manager_borrow() {
        // The whole point of the `Arc`: the render path holds these while the
        // flush closures hold `&mut` of the manager they came from.
        let mut assets = AssetManager::new();
        let builtin = tiny_font();
        assets.insert_font_data("fonts/ui.font".to_string(), tiny_font());
        let fonts = FrameFonts::gather(&assets, &builtin, "__font/builtin");

        // Mutating the manager afterwards must not disturb the gathered set.
        assets.insert_font_data("fonts/other.font".to_string(), tiny_font());
        assert_eq!(fonts.resolve("fonts/ui.font").1, "fonts/ui.png");
        assert_eq!(fonts.resolve("fonts/other.font").1, "__font/builtin");
    }

    #[test]
    fn pixel_size_defaults_to_one_for_legacy_fonts() {
        // Legacy hand-authored fonts have no `pixel_size`; changing that
        // default would resize every one of them.
        let json = r#"{"atlas_width":8,"atlas_height":8,"line_height":8,"glyphs":[]}"#;
        let mut assets = AssetManager::new();
        insert_font_json(&mut assets, "legacy.font", json).unwrap();
        assert_eq!(assets.get_font_data("legacy.font").unwrap().pixel_size, 1.0);
    }

    #[test]
    fn insert_font_json_reports_a_parse_error() {
        let mut assets = AssetManager::new();
        assert!(insert_font_json(&mut assets, "bad.font", "{ not json").is_err());
        assert!(!assets.has_font_data("bad.font"));
    }

    fn tiny_font() -> FontData {
        FontData {
            atlas_width: 8,
            atlas_height: 8,
            line_height: 8.0,
            pixel_size: 1.0,
            atlas_scale: 1.0,
            sdf: false,
            sdf_spread: 0.0,
            glyphs: Vec::new(),
            shaping: None,
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod disk_tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn make_tmp_dir(label: &str) -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "jengine-font-asset-test-{}-{}-{}",
            label,
            std::process::id(),
            id
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_dummy_png(path: &Path) {
        let rgba: [u8; 16] = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        image::RgbaImage::from_raw(2, 2, rgba.to_vec())
            .unwrap()
            .save(path)
            .unwrap();
    }

    const FONT_JSON: &str = r#"{
        "atlas_width": 2, "atlas_height": 2, "line_height": 10, "pixel_size": 16,
        "glyphs": [{"ch":"A","x":0,"y":0,"width":1,"height":1,"advance":2.0,
                    "x_offset":0,"y_offset":1}]
    }"#;

    #[test]
    fn loads_a_font_and_its_sibling_atlas() {
        let dir = make_tmp_dir("ok");
        std::fs::create_dir_all(dir.join("fonts")).unwrap();
        std::fs::write(dir.join("fonts/ui.font"), FONT_JSON).unwrap();
        write_dummy_png(&dir.join("fonts/ui.png"));

        let mut assets = AssetManager::new();
        let wanted = HashSet::from(["fonts/ui.font".to_string()]);
        let errs = load_fonts(&wanted, std::slice::from_ref(&dir), &mut assets);

        assert!(errs.is_empty(), "{errs:?}");
        assert!(assets.has_font_data("fonts/ui.font"));
        // The image must land under the derived id, or the render pass binds
        // nothing and every glyph samples the wrong texture.
        assert!(assets.has_texture_bytes("fonts/ui.png"));
        let data = assets.get_font_data("fonts/ui.font").unwrap();
        assert_eq!(data.pixel_size, 16.0);
        assert_eq!(data.glyphs.len(), 1);
    }

    #[test]
    fn a_font_without_its_image_is_rejected_whole() {
        // Registering the metadata alone would draw every glyph from whatever
        // texture the batch had bound - worse than not loading the font.
        let dir = make_tmp_dir("noimg");
        std::fs::write(dir.join("ui.font"), FONT_JSON).unwrap();

        let mut assets = AssetManager::new();
        let wanted = HashSet::from(["ui.font".to_string()]);
        let errs = load_fonts(&wanted, std::slice::from_ref(&dir), &mut assets);

        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].to_string().contains("sibling atlas image"));
        assert!(!assets.has_font_data("ui.font"));
    }

    #[test]
    fn one_bad_font_does_not_stop_the_others() {
        let dir = make_tmp_dir("mixed");
        std::fs::write(dir.join("good.font"), FONT_JSON).unwrap();
        write_dummy_png(&dir.join("good.png"));

        let mut assets = AssetManager::new();
        let wanted = HashSet::from(["good.font".to_string(), "missing.font".to_string()]);
        let errs = load_fonts(&wanted, std::slice::from_ref(&dir), &mut assets);

        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(assets.has_font_data("good.font"));
    }

    #[test]
    fn an_already_loaded_font_is_not_re_read() {
        // Scene loads call this every time; re-reading every font on every
        // transition would put disk I/O on the load path for nothing.
        let dir = make_tmp_dir("cached");
        std::fs::write(dir.join("ui.font"), FONT_JSON).unwrap();
        write_dummy_png(&dir.join("ui.png"));

        let mut assets = AssetManager::new();
        let wanted = HashSet::from(["ui.font".to_string()]);
        assert!(load_fonts(&wanted, std::slice::from_ref(&dir), &mut assets).is_empty());

        std::fs::remove_file(dir.join("ui.font")).unwrap();
        let errs = load_fonts(&wanted, std::slice::from_ref(&dir), &mut assets);
        assert!(errs.is_empty(), "cached font should not be re-read: {errs:?}");
    }

    #[test]
    fn the_repo_test_font_parses() {
        // A genuine third-party file (jSuite-generated, 319 glyphs) rather
        // than one this repo produced - the format claim is only worth
        // anything if it holds for a file we did not write.
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../projects/engine-test-lab/assets/fonts/bebasneue-regular.font");
        if !path.is_file() {
            return; // Asset not present in this checkout; nothing to prove.
        }
        let json = std::fs::read_to_string(&path).unwrap();
        let data: FontData = serde_json::from_str(&json).unwrap();
        assert_eq!(data.glyphs.len(), 319);
        assert_eq!(data.pixel_size, 16.0);
        assert!(
            data.glyphs.iter().any(|g| g.ch == '?'),
            "the '?' fallback glyph must exist or missing chars draw nothing"
        );
    }
}
