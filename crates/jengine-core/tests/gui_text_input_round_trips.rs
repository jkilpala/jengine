//! What a script writes into a text field is what it reads back out.
//!
//! `gui_get(gui, node, "text")` has always returned a `TextInput`'s
//! **text** - what the player typed. `gui_set(gui, node, "text", ...)`
//! wrote its **placeholder**, the grey prompt behind it. One property
//! name, two different fields, in opposite directions.
//!
//! It hid because an empty field draws its placeholder: setting "text" on
//! an empty box looks exactly like filling the box, and keeps looking like
//! it until somebody types. So "clear the field" silently did nothing, and
//! a form that pre-filled a value silently offered a suggestion instead
//! (BUGS #138).
//!
//! Found by writing a game whose whole interface is a keyword typed at a
//! person: after each word the field has to be cleared, and it never was.

use jengine_core::ui::gui_document::{GuiDocument, GuiNode, GuiNodeType, NodeStyle, apply_gui_node_prop};

fn field(text: &str, placeholder: &str) -> GuiNode {
    GuiNode {
        id: "word".into(),
        node_type: GuiNodeType::TextInput {
            text: text.into(),
            placeholder: placeholder.into(),
            font_id: String::new(),
            font_size: 16.0,
            color: [1.0; 4],
            bg_color: [0.1, 0.1, 0.1, 1.0],
            max_length: 0,
            password: false,
            edit: Default::default(),
        },
        style: NodeStyle::default(),
        children: vec![],
    }
}

fn text_of(node: &GuiNode) -> (String, String) {
    match &node.node_type {
        GuiNodeType::TextInput { text, placeholder, .. } => (text.clone(), placeholder.clone()),
        _ => panic!("not a text input"),
    }
}

#[test]
fn writing_text_fills_the_field_not_the_prompt() {
    let mut node = field("", "ask about...");
    assert!(apply_gui_node_prop(&mut node, "text", &serde_json::json!("crown")));
    let (text, placeholder) = text_of(&node);
    assert_eq!(text, "crown", "the field was not filled");
    assert_eq!(placeholder, "ask about...", "the prompt was overwritten instead");
}

#[test]
fn a_field_can_be_cleared_which_is_the_whole_point() {
    let mut node = field("crown", "ask about...");
    assert!(apply_gui_node_prop(&mut node, "text", &serde_json::json!("")));
    assert_eq!(text_of(&node).0, "", "the field still holds the last word typed");
}

#[test]
fn the_prompt_has_its_own_property() {
    let mut node = field("crown", "ask about...");
    assert!(apply_gui_node_prop(&mut node, "placeholder", &serde_json::json!("say a word")));
    let (text, placeholder) = text_of(&node);
    assert_eq!(placeholder, "say a word");
    assert_eq!(text, "crown", "setting the prompt wiped what was typed");
}

#[test]
fn what_is_written_is_what_the_engine_reads_back() {
    // The round trip that was broken: `gui_set` then `gui_get` gave back
    // whatever was typed last, for ever. `gui_get` reads a TextInput's
    // `text`, so a write that landed on `placeholder` was invisible to it.
    let mut doc = GuiDocument::default();
    doc.root.children = vec![field("crown", "ask about...")];

    // Type a word, then have the script clear it, as a conversation does.
    let node = doc.root.find_node_mut("word").unwrap();
    apply_gui_node_prop(node, "text", &serde_json::json!(""));
    assert_eq!(text_of(doc.root.find_node("word").unwrap()).0, "");

    // And the document still round-trips through its own format.
    let json = jengine_core::ui::gui_document::save_gui(&doc);
    let back = jengine_core::ui::gui_document::load_gui(&json).unwrap();
    let (text, placeholder) = text_of(back.root.find_node("word").unwrap());
    assert_eq!(text, "");
    assert_eq!(placeholder, "ask about...");
}

#[test]
fn a_number_typed_into_the_field_still_arrives_as_text() {
    // Scripts stringify, and a number-shaped value arrives as a JSON
    // number rather than a string - which is why the text arm parses both.
    let mut node = field("", "");
    assert!(apply_gui_node_prop(&mut node, "text", &serde_json::json!(42)));
    assert_eq!(text_of(&node).0, "42");
}
