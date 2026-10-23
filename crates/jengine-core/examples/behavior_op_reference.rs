//! Regenerate the behaviour-graph op reference in `docs/scripting-api.md`.
//!
//!     cargo run -p jengine-core --example behavior_op_reference
//!
//! Then `npm run sync` in `website/` so the site and the AI spec follow.

use jengine_core::behavior::catalog::{render_op_reference, replace_op_reference};

fn main() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/scripting-api.md");
    let doc = std::fs::read_to_string(&path).expect("read docs/scripting-api.md");
    let Some(updated) = replace_op_reference(&doc, &render_op_reference()) else {
        eprintln!("docs/scripting-api.md has no generated behaviour op block - add the markers");
        std::process::exit(1);
    };
    if updated == doc {
        println!("op reference already current");
    } else {
        std::fs::write(&path, updated).expect("write docs/scripting-api.md");
        println!("op reference updated");
    }
}
