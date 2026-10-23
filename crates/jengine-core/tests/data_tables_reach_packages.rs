//! A package can read the project's data tables from Rust.
//!
//! Scripts had `get_data`; Rust code had nothing - the native registry
//! lived inside the script system and the editor's tables only in the
//! browser. The deckbuilder's cards are data rows its package reads, so
//! both package-hosting runtimes now publish the tables
//! (`jengine_core::data::global`): native when it loads them, the editor
//! viewport when the editor seeds them.

use jengine_core::data::{self, DataRegistry};

const NATIVE_RUNNER: &str = include_str!("../../jengine-runtime/src/wgpu_scene_runner.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");

#[test]
fn the_editor_seed_shape_builds_a_registry() {
    let json = r#"{ "data/cards": { "schema": "Card", "rows": [
        { "id": "strike", "cost": 1, "name": "Strike" },
        { "id": "defend", "cost": 1, "name": "Defend" } ] } }"#;
    let reg = DataRegistry::from_tables_json(json).unwrap();
    assert_eq!(reg.row_ids("data/cards"), vec!["strike", "defend"], "rows in file order");
    assert_eq!(reg.get_string("data/cards.data", "defend", "name"), Some("Defend"));
    data::install_global(std::sync::Arc::new(reg));
    assert_eq!(data::global().unwrap().get_f64("data/cards", "strike", "cost"), Some(1.0));
}

#[test]
fn both_package_runtimes_publish_their_tables() {
    assert!(NATIVE_RUNNER.contains("jengine_core::data::install_global("), "native publishes what it loads");
    assert!(VIEWPORT.contains("jengine_core::data::install_global("), "the viewport publishes what the editor seeds");
}
