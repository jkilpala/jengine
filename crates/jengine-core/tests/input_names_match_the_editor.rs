//! The editor's input-action editor offers the binding names in
//! `editor/src/lib/inputNames.json`, and its "press to bind" turns a key or
//! a pad press into one of them. Checked here against the engine's own lists
//! - `KeyCode::name` over `ALL_KEYS`, and `gamepad::BUTTON_NAMES` /
//! `AXIS_NAMES` - so a key or button added to the engine reaches the editor,
//! and a name the editor writes is one an action resolves.

use jengine_core::input::gamepad::{AXIS_NAMES, BUTTON_NAMES};
use jengine_core::input::{ALL_KEYS, KeyCode};
use serde_json::Value;

fn list(v: &Value, key: &str) -> Vec<String> {
    v[key].as_array().unwrap_or_else(|| panic!("{key}")).iter().map(|s| s.as_str().expect("a name").to_string()).collect()
}

#[test]
fn input_names_match_the_editor() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../editor/src/lib/inputNames.json"))
        .expect("the editor's list");
    let names: Value = serde_json::from_str(&text).expect("JSON");
    let keys: Vec<String> = ALL_KEYS.iter().map(|k| k.name().to_string()).collect();
    assert_eq!(list(&names, "keys"), keys, "the keys, in ALL_KEYS order");
    for k in &keys {
        assert!(KeyCode::from_name(k).is_some(), "{k} resolves");
    }
    assert_eq!(list(&names, "pad_buttons"), BUTTON_NAMES.to_vec());
    assert_eq!(list(&names, "pad_axes"), AXIS_NAMES.to_vec());
}
