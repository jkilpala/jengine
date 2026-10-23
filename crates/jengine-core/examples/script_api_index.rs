//! Regenerate the API Index in `docs/scripting-api.md`.
//!
//! ```text
//! cargo run -p jengine-core --example script_api_index            # write
//! cargo run -p jengine-core --example script_api_index -- --check # verify
//! ```
//!
//! That document is upstream of the documentation website and of the
//! machine-readable AI spec, so the index being wrong is not only a
//! documentation problem — it is what tools are told the engine can do.
//! `tests/script_api_index_is_current.rs` runs the check.

use std::path::{Path, PathBuf};

use jengine_core::package::api_index::{IndexInputs, render_api_index, replace_api_index};
use jengine_core::package::core_script_api::register_core_script_api;
use jengine_core::package::script_api::ScriptApiRegistry;

fn repo_root() -> PathBuf {
    // <repo>/crates/jengine-core
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("resolve repo root")
}

fn main() {
    let check_only = std::env::args().any(|a| a == "--check");
    let root = repo_root();
    let doc_path = root.join("docs/scripting-api.md");

    let mut registry = ScriptApiRegistry::new();
    register_core_script_api(&mut registry);

    let luau_rs = std::fs::read_to_string(
        root.join("crates/jengine-core/src/scripting/bridge/adapters/luau.rs"),
    )
    .expect("read luau.rs");
    let lua_runtime_ts =
        std::fs::read_to_string(root.join("editor/src/lib/lua-runtime.ts")).expect(
            "read editor/src/lib/lua-runtime.ts — this tool needs the full workspace checkout",
        );
    let doc = std::fs::read_to_string(&doc_path).expect("read docs/scripting-api.md");

    let table = render_api_index(IndexInputs {
        registry: &registry,
        luau_rs: &luau_rs,
        lua_runtime_ts: &lua_runtime_ts,
        existing_doc: &doc,
    });
    let updated = replace_api_index(&doc, &table).expect("locate the '## API Index' section");

    if check_only {
        if updated == doc {
            println!("API index is current.");
        } else {
            eprintln!(
                "docs/scripting-api.md API index is stale.\n\
                 Run: cargo run -p jengine-core --example script_api_index"
            );
            std::process::exit(1);
        }
        return;
    }

    if updated == doc {
        println!("API index already current — nothing written.");
    } else {
        std::fs::write(&doc_path, updated).expect("write docs/scripting-api.md");
        println!("Wrote {}", doc_path.display());
        println!("Run `npm run sync` in website/ to push it to the site and AI spec.");
    }
}
