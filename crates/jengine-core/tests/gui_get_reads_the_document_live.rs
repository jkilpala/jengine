//! `gui_get` answers from the loaded document itself.
//!
//! It used to answer from `gui_node_props`, a map the script system rebuilt
//! every frame with three `String`s per readable property per node of every
//! visible document - to serve a getter most frames never call. The snapshot
//! now lends the asset manager under the same contract as the world, and
//! `BridgeReadSnapshot::gui_prop` walks the document for the one node asked
//! about. The mapping from node to property is one function,
//! `snapshot::gui_node_prop`, shared with the eager fill a hand-built test
//! snapshot still uses.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::{CustomComponents, GuiComponent, Name, ScriptComponent, ScriptLanguage};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;
use jengine_core::ui::gui_document::load_gui;

const DOC: &str = r#"{
  "name": "Hud", "anchor": "TopLeft",
  "root": {
    "id": "root", "node_type": { "type": "Panel" },
    "style": { "width": { "type": "Fixed", "value": 200.0 }, "height": { "type": "Fixed", "value": 100.0 } },
    "children": [
      { "id": "title", "node_type": { "type": "Text", "text": "Score: 12", "font_id": "", "font_size": 12.0, "color": [1,1,1,1], "alignment": "Left" },
        "style": { "width": { "type": "Fixed", "value": 100.0 }, "height": { "type": "Fixed", "value": 20.0 } }, "children": [] },
      { "id": "go", "node_type": { "type": "Button", "label": "Start", "font_id": "", "font_size": 12.0 },
        "style": { "width": { "type": "Fixed", "value": 60.0 }, "height": { "type": "Fixed", "value": 20.0 } }, "children": [] },
      { "id": "bar", "node_type": { "type": "ProgressBar", "value": 0.25, "min": 0.0, "max": 1.0 },
        "style": { "width": { "type": "Fixed", "value": 60.0 }, "height": { "type": "Fixed", "value": 10.0 }, "visible": false }, "children": [] }
    ]
  }
}"#;

const SRC: &str = r#"
function update(entity, dt)
    set_custom(entity, "R", "title", gui_get("hud.gui", "title", "text"))
    set_custom(entity, "R", "label", gui_get("hud.gui", "go", "label"))
    set_custom(entity, "R", "text_of_button", gui_get("hud.gui", "go", "text"))
    set_custom(entity, "R", "value", gui_get("hud.gui", "bar", "value"))
    set_custom(entity, "R", "hidden", gui_get("hud.gui", "bar", "visible"))
    set_custom(entity, "R", "missing", gui_get("hud.gui", "nobody", "text"))
end
"#;

#[test]
fn a_script_reads_widget_properties_from_the_loaded_document() {
    let dir = std::env::temp_dir().join(format!(
        "jengine_gui_get_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("reader.lua");
    std::fs::write(&path, SRC).unwrap();

    let mut assets = AssetManager::new();
    assets.insert_gui("hud.gui".into(), load_gui(DOC).expect("the document parses"));
    let mut world = World::new();
    let e = world.spawn((
        Name("Hud".into()),
        GuiComponent { gui_id: "hud.gui".into(), ..Default::default() },
        CustomComponents::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    system.run(&mut world, 1.0 / 60.0, &input, None, Some(&mut assets));

    let custom = world.get::<&CustomComponents>(e).unwrap();
    let r = custom.0.get("R").expect("the script wrote its readings");
    let s = |k: &str| r.get(k).and_then(|v| v.as_str()).unwrap_or("<none>").to_string();
    assert_eq!(s("title"), "Score: 12");
    assert_eq!(s("label"), "Start");
    assert_eq!(s("text_of_button"), "Start", "a button's label answers to `text` too");
    assert_eq!(s("value"), "0.25");
    assert_eq!(s("hidden"), "false", "a style property, read live");
    assert_eq!(s("missing"), "", "an unknown node reads as the empty string, as before");
}
