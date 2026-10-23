//! The API index in `docs/scripting-api.md` must match the registration sites.
//!
//! It is generated (`cargo run -p jengine-core --example script_api_index`)
//! and this is the guard that it was regenerated. Hand-maintaining it went
//! badly: the index sat 50 functions behind the engine, and because that file
//! feeds both the documentation website and `public/ai/api-spec.json`, the gap
//! reached the machine-readable description of the API as well.

use jengine_core::package::api_index::{IndexInputs, render_api_index, replace_api_index};
use jengine_core::package::core_script_api::register_core_script_api;
use jengine_core::package::script_api::ScriptApiRegistry;

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn api_index_matches_the_registration_sites() {
    let root = repo_root();
    let doc_path = root.join("docs/scripting-api.md");

    let mut registry = ScriptApiRegistry::new();
    register_core_script_api(&mut registry);

    let luau_rs = std::fs::read_to_string(
        root.join("crates/jengine-core/src/scripting/bridge/adapters/luau.rs"),
    )
    .expect("read luau.rs");
    let lua_runtime_ts = std::fs::read_to_string(root.join("editor/src/lib/lua-runtime.ts"))
        .expect("read lua-runtime.ts — this test needs the full workspace checkout");
    let doc = std::fs::read_to_string(&doc_path).expect("read docs/scripting-api.md");

    let table = render_api_index(IndexInputs {
        registry: &registry,
        luau_rs: &luau_rs,
        lua_runtime_ts: &lua_runtime_ts,
        existing_doc: &doc,
    });
    let expected = replace_api_index(&doc, &table).expect("locate '## API Index'");

    assert_eq!(
        expected, doc,
        "docs/scripting-api.md API index is out of date.\n\
         Run: cargo run -p jengine-core --example script_api_index\n\
         then `npm run sync` in website/ so the site and AI spec follow."
    );
}
