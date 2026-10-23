//! The editor previews a scene in another language by writing each
//! `text_key`'s text itself (`editor/src/lib/localeEdit.ts`), so its lookup
//! - the language chosen, its fallbacks, the first table with the key - is
//! checked against the engine's (`locale::current_in`, `translate_with`) on
//! one fixture, `fixtures/locale_lookup.json`; the editor's half is
//! `localeEdit.test.ts`.

use jengine_core::data::DataRegistry;
use jengine_core::locale;
use serde_json::{Value, json};

#[test]
fn locale_lookup_matches_the_editor() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/locale_lookup.json"))
        .expect("the fixture");
    let fixture: Value = serde_json::from_str(&text).expect("JSON");
    let mut registry = DataRegistry::new();
    let fields: Vec<Value> = fixture["languages"]
        .as_array()
        .expect("languages")
        .iter()
        .map(|c| json!({ "name": c, "type": { "kind": "string" }, "default": "" }))
        .collect();
    registry
        .load_schema_json(&json!({ "name": "Strings", "version": 1, "fields": fields }).to_string())
        .expect("schema");
    for (id, rows) in fixture["tables"].as_object().expect("tables") {
        registry
            .load_table_json(id, &json!({ "schema": "Strings", "version": 1, "rows": rows }).to_string())
            .unwrap_or_else(|e| panic!("{id}: {e:?}"));
    }
    for case in fixture["cases"].as_array().expect("cases") {
        let wanted = case["wanted"].as_str().expect("wanted");
        let key = case["key"].as_str().expect("key");
        locale::set_locale(wanted);
        let lang = locale::current_in(&registry);
        assert_eq!(lang, case["lang"].as_str().expect("lang"), "{wanted}: the language");
        let got = locale::translate_with(&registry, &lang, key, None);
        assert_eq!(got.as_deref(), case["text"].as_str(), "{wanted} {key}");
    }
    locale::set_locale("");
}
