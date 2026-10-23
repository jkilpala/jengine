//! Typing into a focused GUI `TextInput`.
//!
//! `TextInput` shipped as a renderable node with a `text` field, `gui_focus`
//! set focus, and nothing anywhere consumed a keystroke — `InputState` had no
//! notion of typed characters at all, so the widget was display-only in every
//! runtime and the only way to change it was `gui_set(..., "text", ...)`.

use hecs::World;
use jengine_core::asset::AssetManager;
use jengine_core::ecs::components::GuiComponent;
use jengine_core::ecs::systems::apply_text_input;
use jengine_core::input::{InputState, KeyCode};
use jengine_core::ui::gui_document::GuiNodeType;

const DOC: &str = r#"{
  "name": "T",
  "anchor": "TopLeft",
  "root": {
    "id": "root",
    "node_type": { "type": "Panel" },
    "style": {},
    "children": [
      {
        "id": "field",
        "node_type": { "type": "TextInput", "text": "", "placeholder": "type",
                       "font_id": "", "font_size": 16.0,
                       "color": [1,1,1,1], "bg_color": [0,0,0,1] },
        "style": { "focusable": true },
        "children": []
      },
      {
        "id": "other",
        "node_type": { "type": "TextInput", "text": "", "placeholder": "",
                       "font_id": "", "font_size": 16.0,
                       "color": [1,1,1,1], "bg_color": [0,0,0,1] },
        "style": {},
        "children": []
      }
    ]
  }
}"#;

fn setup(focus: Option<&str>) -> (World, AssetManager) {
    let mut assets = AssetManager::new();
    assets.insert_gui("ui/t.gui".to_string(), jengine_core::ui::gui_document::load_gui(DOC).unwrap());
    let mut world = World::new();
    world.spawn((GuiComponent {
        gui_id: "ui/t.gui".to_string(),
        visible: true,
        variables: Default::default(),
        focused_node_id: focus.map(str::to_string),
        ..Default::default()
    },));
    (world, assets)
}

fn field_text(assets: &AssetManager, node_id: &str) -> String {
    let doc = assets.get_gui("ui/t.gui").unwrap();
    match &doc.root.find_node(node_id).unwrap().node_type {
        GuiNodeType::TextInput { text, .. } => text.clone(),
        _ => panic!("{node_id} is not a TextInput"),
    }
}

#[test]
fn typed_characters_reach_the_focused_field() {
    let (mut world, mut assets) = setup(Some("field"));
    let mut input = InputState::new();
    input.type_str("hi");
    input.type_char('!');

    let changed = apply_text_input(&mut world, &mut assets, &mut input);

    assert_eq!(changed, 1);
    assert_eq!(field_text(&assets, "field"), "hi!");
    // Only the focused node is edited.
    assert_eq!(field_text(&assets, "other"), "");
}

#[test]
fn backspace_deletes_and_control_characters_are_not_inserted() {
    let (mut world, mut assets) = setup(Some("field"));

    let mut input = InputState::new();
    input.type_str("abc");
    apply_text_input(&mut world, &mut assets, &mut input);
    assert_eq!(field_text(&assets, "field"), "abc");

    // Backspace is an editing command, and arrives as a KeyCode. The control
    // character a platform may also report for it must not be inserted.
    let mut input = InputState::new();
    input.key_down(KeyCode::Backspace);
    input.type_char('\u{8}');
    input.type_char('\n');
    input.type_char('\t');
    apply_text_input(&mut world, &mut assets, &mut input);
    assert_eq!(field_text(&assets, "field"), "ab");
}

#[test]
fn nothing_happens_without_focus_or_input() {
    let (mut world, mut assets) = setup(None);
    let mut input = InputState::new();
    input.type_str("ignored");
    assert_eq!(apply_text_input(&mut world, &mut assets, &mut input), 0);
    assert_eq!(field_text(&assets, "field"), "");

    // Focused, but nothing typed this frame.
    let (mut world, mut assets) = setup(Some("field"));
    let mut input = InputState::new();
    assert_eq!(apply_text_input(&mut world, &mut assets, &mut input), 0);
}

#[test]
fn focus_on_a_non_text_node_is_not_text_entry() {
    // Focus can land on any focusable node — a button reached by Tab. Typing
    // at one must not panic or invent a text field.
    let (mut world, mut assets) = setup(Some("root"));
    let mut input = InputState::new();
    input.type_str("xyz");
    assert_eq!(apply_text_input(&mut world, &mut assets, &mut input), 0);
}

#[test]
fn typed_chars_clear_each_frame() {
    // Otherwise a single keystroke would be re-inserted every frame it stayed
    // in the buffer.
    let mut input = InputState::new();
    input.type_str("a");
    assert_eq!(input.typed_chars().len(), 1);
    input.begin_frame();
    assert!(input.typed_chars().is_empty());
}

#[test]
fn a_field_that_took_backspace_consumes_it() {
    // Backspace is commonly bound to something else — the Engine Test Lab
    // binds it to "back to hub". A focused field must not delete a character
    // *and* let that fire, or you get thrown out of the scene mid-word.
    let (mut world, mut assets) = setup(Some("field"));
    let mut input = InputState::new();
    input.type_str("ab");
    apply_text_input(&mut world, &mut assets, &mut input);

    let mut input = InputState::new();
    input.key_down(KeyCode::Backspace);
    assert!(input.is_just_pressed(KeyCode::Backspace));
    apply_text_input(&mut world, &mut assets, &mut input);
    assert_eq!(field_text(&assets, "field"), "a");
    assert!(
        !input.is_just_pressed(KeyCode::Backspace),
        "the field edited text, so it owns the keystroke"
    );

    // With nothing focused it stays available to whoever else wants it.
    let (mut world, mut assets) = setup(None);
    let mut input = InputState::new();
    input.key_down(KeyCode::Backspace);
    apply_text_input(&mut world, &mut assets, &mut input);
    assert!(input.is_just_pressed(KeyCode::Backspace));
}

fn submitted(assets: &AssetManager, node_id: &str) -> String {
    let doc = assets.get_gui("ui/t.gui").unwrap();
    jengine_core::scripting::bridge::snapshot::gui_node_prop(doc.root.find_node(node_id).unwrap(), "submitted").unwrap()
}

#[test]
fn a_field_let_go_after_sending_is_no_longer_submitted() {
    // `lib/chat.lua` with `let_go`: Enter sends the line and gives the
    // keyboard back. `submitted` was cleared only by the focused field's own
    // update, so it read "true" for good once the field was let go - and the
    // next T focused the field and, in the same `update`, took the stale
    // submit as a line sent and let go again. T never opened the chat twice.
    let (mut world, mut assets) = setup(Some("field"));
    let mut input = InputState::new();
    input.type_str("hello");
    input.key_down(KeyCode::Enter);
    apply_text_input(&mut world, &mut assets, &mut input);
    assert_eq!(submitted(&assets, "field"), "true");

    // The script lets go, and the next frame runs with nothing pressed.
    for (_, g) in world.query_mut::<&mut GuiComponent>() {
        g.focused_node_id = None;
    }
    input.key_up(KeyCode::Enter);
    input.begin_frame();
    apply_text_input(&mut world, &mut assets, &mut input);
    assert_eq!(submitted(&assets, "field"), "false", "an unfocused field stays submitted");

    // Focused again, it is not submitted until Enter is pressed again.
    for (_, g) in world.query_mut::<&mut GuiComponent>() {
        g.focused_node_id = Some("field".to_string());
    }
    input.begin_frame();
    input.type_str("x");
    apply_text_input(&mut world, &mut assets, &mut input);
    assert_eq!(submitted(&assets, "field"), "false");
}
