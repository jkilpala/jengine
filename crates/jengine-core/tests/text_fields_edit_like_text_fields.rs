//! **A text field you can edit** (Oct 2026): a caret, a selection, words,
//! the clipboard, Enter and Escape, a length limit, a password mask and an
//! IME's composition - one model (`ui::text_edit`) for entity UI's
//! `ui_text_input` and a `.gui` `TextInput`. Before it a field appended what
//! was typed and Backspace took the last character, and an entity-UI field
//! heard the keyboard only while the pointer happened to be over a widget.
//! Found by the showcase's Guest Book.
//!
//! The model itself is unit-tested in `ui::text_edit`; this drives both UI
//! systems through their real per-frame updates, and checks that every host
//! hands a focused field its IME, clipboard and (in a browser) the hidden
//! textarea.

use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::GuiComponent;
use jengine_core::ecs::systems::{apply_text_input, track_gui_pointer, update_ui_widgets};
use jengine_core::input::{InputState, KeyCode, text_entry};
use jengine_core::ui::gui_document::GuiNodeType;
use jengine_core::ui::scaling::UiScale;
use jengine_core::ui::text_edit::{self, FieldView};
use jengine_core::ui::{UIAnchor, UIElement, UITextInput, apply_ui_prop, read_ui_prop};

const W: f32 = 800.0;
const H: f32 = 600.0;

/// Ten pixels a character, from x = 306 (a field at 300..500 with its pad).
fn view(rect: [f32; 4], chars: usize) -> FieldView {
    FieldView { rect, scroll: 0.0, advances: (0..=chars).map(|i| i as f32 * 10.0).collect() }
}

fn frame(world: &mut World, input: &mut InputState, pointer: (f32, f32)) {
    let mut clicked = Vec::new();
    update_ui_widgets(world, input, pointer, W, H, &mut clicked);
}

#[test]
fn an_entity_ui_field_places_its_caret_edits_there_and_owns_the_keyboard() {
    let mut world = World::new();
    let e = world.spawn((
        UIElement { anchor: UIAnchor::Center, size: glam::Vec2::new(200.0, 40.0), ..Default::default() },
        UITextInput { text: "abcd".into(), ..Default::default() },
    ));
    text_edit::set_view(text_edit::entity_key(e), view([306.0, 280.0, 188.0, 40.0], 4));
    let mut input = InputState::new();
    // A click two characters in.
    input.begin_frame();
    input.set_mouse_button(true, true);
    frame(&mut world, &mut input, (327.0, 300.0));
    assert_eq!(read_ui_prop(&world, e, "focused"), Some(serde_json::json!(true)));
    assert_eq!(read_ui_prop(&world, e, "caret"), Some(serde_json::json!(2)));
    // Typing goes in there - and still does with the pointer far away.
    input.begin_frame();
    input.set_mouse_button(true, false);
    input.type_str("X");
    frame(&mut world, &mut input, (5.0, 5.0));
    input.begin_frame();
    input.key_down(KeyCode::Backspace);
    input.key_down(KeyCode::H);
    input.type_str("h");
    frame(&mut world, &mut input, (5.0, 5.0));
    assert_eq!(read_ui_prop(&world, e, "text"), Some(serde_json::json!("abhcd")));
    assert!(!input.is_just_pressed(KeyCode::H), "a letter typed into a field is not the game's");
    assert!(text_entry::take().is_some(), "the host learns a field is focused, and where its caret is");
    // Enter submits for one frame.
    input.key_up(KeyCode::Backspace);
    input.key_up(KeyCode::H);
    input.begin_frame();
    input.key_down(KeyCode::Enter);
    frame(&mut world, &mut input, (5.0, 5.0));
    assert_eq!(read_ui_prop(&world, e, "submitted"), Some(serde_json::json!(true)));
    input.key_up(KeyCode::Enter);
    input.begin_frame();
    frame(&mut world, &mut input, (5.0, 5.0));
    assert_eq!(read_ui_prop(&world, e, "submitted"), Some(serde_json::json!(false)));
    // Escape lets it go; a script can focus it again, caret at the end.
    input.begin_frame();
    input.key_down(KeyCode::Escape);
    frame(&mut world, &mut input, (5.0, 5.0));
    assert_eq!(read_ui_prop(&world, e, "focused"), Some(serde_json::json!(false)));
    assert!(!input.is_just_pressed(KeyCode::Escape), "the first Escape leaves the field, not the scene");
    assert!(apply_ui_prop(&mut world, e, "focused", &serde_json::json!(true)));
    assert_eq!(read_ui_prop(&world, e, "caret"), Some(serde_json::json!(5)));
}

#[test]
fn an_entity_ui_field_keeps_to_its_limit_and_masks_a_password() {
    let mut world = World::new();
    let e = world.spawn((
        UIElement { anchor: UIAnchor::Center, size: glam::Vec2::new(200.0, 40.0), ..Default::default() },
        UITextInput { max_length: 3, password: true, ..Default::default() },
    ));
    assert!(apply_ui_prop(&mut world, e, "focused", &serde_json::json!(true)));
    let mut input = InputState::new();
    input.begin_frame();
    input.type_str("secret");
    frame(&mut world, &mut input, (5.0, 5.0));
    assert_eq!(read_ui_prop(&world, e, "text"), Some(serde_json::json!("sec")));
    assert_eq!(text_edit::masked("sec"), "***");
}

const DOC: &str = r#"{
  "name": "T",
  "anchor": "TopLeft",
  "root": { "id": "root", "node_type": { "type": "Panel" }, "style": {}, "children": [
    { "id": "field", "node_type": { "type": "TextInput", "text": "abcd", "placeholder": "", "font_id": "",
        "font_size": 16.0, "color": [1,1,1,1], "bg_color": [0,0,0,1] },
      "style": { "width": { "type": "Fixed", "value": 200 }, "height": { "type": "Fixed", "value": 40 } }, "children": [] },
    { "id": "button", "node_type": { "type": "Button", "label": "OK" },
      "style": { "width": { "type": "Fixed", "value": 80 }, "height": { "type": "Fixed", "value": 40 } }, "children": [] } ] }
}"#;

fn gui_text(assets: &AssetManager) -> String {
    match &assets.get_gui("ui/t.gui").unwrap().root.find_node("field").unwrap().node_type {
        GuiNodeType::TextInput { text, .. } => text.clone(),
        _ => unreachable!(),
    }
}

#[test]
fn a_gui_field_is_focused_by_a_click_with_its_caret_where_the_click_was() {
    let doc = jengine_core::ui::gui_document::load_gui(DOC).unwrap();
    let layout = jengine_core::ui::layout::compute_layout(&doc, W, H);
    let r = layout.iter().find(|e| e.id == "field").unwrap().rect;
    let b = layout.iter().find(|e| e.id == "button").unwrap().rect;
    let mut assets = AssetManager::new();
    assets.insert_gui("ui/t.gui".into(), doc);
    let mut world = World::new();
    let g = world.spawn((GuiComponent { gui_id: "ui/t.gui".into(), ..Default::default() },));
    text_edit::set_view(text_edit::gui_key("ui/t.gui", "field"), view([r.x + 6.0, r.y, r.width - 12.0, r.height], 4));
    let scale = UiScale::constant();
    let mut input = InputState::new();
    input.screen_size = (W, H);
    // A click one character in.
    input.begin_frame();
    input.mouse_position = (r.x + 6.0 + 9.0, r.y + r.height * 0.5);
    input.set_mouse_button(true, true);
    track_gui_pointer(&mut world, &assets, &input, &scale, 0.016);
    assert_eq!(world.get::<&GuiComponent>(g).unwrap().focused_node_id.as_deref(), Some("field"));
    apply_text_input(&mut world, &mut assets, &mut input);
    input.begin_frame();
    input.set_mouse_button(true, false);
    input.type_str("Z");
    track_gui_pointer(&mut world, &assets, &input, &scale, 0.016);
    apply_text_input(&mut world, &mut assets, &mut input);
    assert_eq!(gui_text(&assets), "aZbcd");
    // Selected with shift+End, then typed over.
    input.begin_frame();
    input.key_down(KeyCode::ShiftLeft);
    input.key_down(KeyCode::End);
    apply_text_input(&mut world, &mut assets, &mut input);
    input.release_all();
    input.begin_frame();
    input.type_str("!");
    apply_text_input(&mut world, &mut assets, &mut input);
    assert_eq!(gui_text(&assets), "aZ!");
    // A click on the button lets the field go.
    input.begin_frame();
    input.mouse_position = (b.x + 5.0, b.y + 5.0);
    input.set_mouse_button(true, true);
    track_gui_pointer(&mut world, &assets, &input, &scale, 0.016);
    assert_eq!(world.get::<&GuiComponent>(g).unwrap().focused_node_id, None);
}

#[test]
fn every_host_gives_a_focused_field_its_ime_clipboard_and_keyboard() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |p: &str| std::fs::read_to_string(root.join(p)).unwrap();
    let runner = read("crates/jengine-runtime/src/wgpu_scene_runner.rs");
    assert!(runner.contains("state.text.sync("), "native never follows the focused field");
    assert!(runner.contains("WindowEvent::Ime(ime)"), "native drops IME events");
    assert!(runner.contains("paste_if_asked("), "native never pastes");
    // A field owns the keys typed into it only if it sees them before the
    // scripts do: the UI updates first in every host (natively it used to
    // run after the scripts, so a game saw H typed into a field).
    let script_run = runner.find("state.script_system.run(").unwrap();
    for call in ["update_ui_widgets(", "apply_text_input("] {
        assert!(runner.find(call).unwrap() < script_run, "native runs {call} after the scripts");
    }
    for host in ["crates/jengine-web/src/lib.rs", "crates/jengine-editor-viewport/src/lib.rs"] {
        let src = read(host);
        let scripts = src.find(if host.contains("web") { "run_scripts_frame(" } else { "invoke each entity's `update(entity_id, dt)`" }).unwrap();
        for call in ["update_ui_widgets(", "apply_text_input("] {
            assert!(src.find(call).unwrap() < scripts, "{host} runs {call} after the scripts");
        }
    }
    let native = read("crates/jengine-runtime/src/text_native.rs");
    for call in ["set_ime_allowed(", "set_ime_cursor_area(", "arboard::Clipboard", "show_keyboard("] {
        assert!(native.contains(call), "text_native never calls {call}");
    }
    for host in ["crates/jengine-web/src/lib.rs", "crates/jengine-editor-viewport/src/lib.rs"] {
        let src = read(host);
        assert!(src.contains("text_proxy.sync("), "{host} never focuses the hidden textarea");
        assert!(src.contains("is_proxy_event(&e)"), "{host} types the textarea's letters twice");
        // What the textarea got must be in the frame's input before a field
        // reads it (and before a replay keeps the frame): synced beside the
        // scripts it was cleared by `begin_frame` unread, and nothing typed
        // in a browser reached any field once the textarea had focus.
        let sync = src.find("text_proxy.sync(").unwrap();
        let reader = if host.contains("web") { "tick_world(app, dt)" } else { "apply_text_input(" };
        for later in ["update_interactions()", reader] {
            assert!(
                sync < src.find(later).unwrap(),
                "{host} hands the textarea's typing over after {later}, which throws it away"
            );
        }
        assert_eq!(src.matches("text_proxy.sync(").count(), 1, "{host} syncs the textarea twice a frame");
    }
    assert!(read("crates/jengine-editor-viewport/src/lib.rs").contains("text_proxy.release()"));
}

/// A form: a field over a button, both focusable.
const FORM: &str = r#"{
  "name": "F",
  "anchor": "TopLeft",
  "root": { "id": "root", "node_type": { "type": "Panel" },
    "style": { "flex": { "direction": "Column", "gap": 10.0, "align_items": "Start", "justify_content": "Start" } },
    "children": [
    { "id": "code", "node_type": { "type": "TextInput", "text": "ab", "placeholder": "", "font_id": "",
        "font_size": 16.0, "color": [1,1,1,1], "bg_color": [0,0,0,1] },
      "style": { "focusable": true, "width": { "type": "Fixed", "value": 200 }, "height": { "type": "Fixed", "value": 40 } }, "children": [] },
    { "id": "join", "node_type": { "type": "Button", "label": "JOIN" },
      "style": { "focusable": true, "width": { "type": "Fixed", "value": 80 }, "height": { "type": "Fixed", "value": 40 } }, "children": [] } ] }
}"#;

fn code_text(assets: &AssetManager) -> (String, bool) {
    match &assets.get_gui("ui/f.gui").unwrap().root.find_node("code").unwrap().node_type {
        GuiNodeType::TextInput { text, edit, .. } => (text.clone(), edit.submitted),
        _ => unreachable!(),
    }
}

/// What a host does with a frame's input, in its order: navigation, then the
/// fields, then (scripts would run here).
fn host_frame(world: &mut World, assets: &mut AssetManager, input: &mut InputState) -> usize {
    let out = jengine_core::ecs::systems::navigate_ui(world, assets, input, &UiScale::constant(), 0.016);
    apply_text_input(world, assets, input);
    out.gui_activations.len()
}

/// **A focused field keeps its arrows and its Enter** (Oct 2026). Menu
/// navigation runs before the fields are updated, and it read the arrows,
/// Enter and Space whatever had focus - so in a room-code box an arrow moved
/// focus to the next button instead of the caret, and Enter "pressed" the
/// field. While a field has focus the keyboard is the field's; a pad still
/// navigates, so a pad player can leave it.
#[test]
fn a_focused_field_keeps_its_arrows_and_enter_from_navigation() {
    let doc = jengine_core::ui::gui_document::load_gui(FORM).unwrap();
    let layout = jengine_core::ui::layout::compute_layout(&doc, W, H);
    let r = layout.iter().find(|e| e.id == "code").unwrap().rect;
    let mut assets = AssetManager::new();
    assets.insert_gui("ui/f.gui".into(), doc);
    let mut world = World::new();
    let g = world.spawn((GuiComponent { gui_id: "ui/f.gui".into(), ..Default::default() },));
    text_edit::set_view(text_edit::gui_key("ui/f.gui", "code"), view([r.x + 6.0, r.y, r.width - 12.0, r.height], 2));
    let focus = |w: &World| w.get::<&GuiComponent>(g).unwrap().focused_node_id.clone();
    let mut input = InputState::new();
    input.screen_size = (W, H);

    // Clicked into, at its end.
    input.begin_frame();
    input.mouse_position = (r.x + r.width - 8.0, r.y + r.height * 0.5);
    input.set_mouse_button(true, true);
    host_frame(&mut world, &mut assets, &mut input);
    input.begin_frame();
    input.set_mouse_button(true, false);
    host_frame(&mut world, &mut assets, &mut input);
    assert_eq!(focus(&world).as_deref(), Some("code"));

    // Left, then a letter: the caret moved inside the field, focus stayed.
    input.begin_frame();
    input.key_down(KeyCode::Left);
    host_frame(&mut world, &mut assets, &mut input);
    input.release_all();
    input.begin_frame();
    input.type_str("Z");
    host_frame(&mut world, &mut assets, &mut input);
    assert_eq!(focus(&world).as_deref(), Some("code"), "an arrow took focus off the field");
    assert_eq!(code_text(&assets).0, "aZb", "the arrow did not move the caret");

    // Down is not navigation either.
    input.begin_frame();
    input.key_down(KeyCode::Down);
    host_frame(&mut world, &mut assets, &mut input);
    input.release_all();
    assert_eq!(focus(&world).as_deref(), Some("code"), "Down moved focus to the button");

    // Enter submits the field and presses nothing.
    input.begin_frame();
    input.key_down(KeyCode::Enter);
    let pressed = host_frame(&mut world, &mut assets, &mut input);
    input.release_all();
    assert_eq!(pressed, 0, "Enter in a field pressed a node");
    assert!(code_text(&assets).1, "Enter did not submit the field");

    // A pad's D-pad still leaves the field for the button.
    input.begin_frame();
    input.update_gamepad(0, [13usize].into_iter().collect(), vec![0.0, 0.0]);
    host_frame(&mut world, &mut assets, &mut input);
    input.begin_frame();
    input.update_gamepad(0, Default::default(), vec![0.0, 0.0]);
    host_frame(&mut world, &mut assets, &mut input);
    assert_eq!(focus(&world).as_deref(), Some("join"), "a pad could not leave the field");
}
