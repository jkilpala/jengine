//! The behaviour-graph op reference in `docs/scripting-api.md` is the
//! catalog's, not a hand-kept copy.
//!
//! The hand-kept one listed 23 of 68 ops and showed an example graph that
//! did not compile - in the file that feeds the website and the AI spec.
#![cfg(feature = "behavior")]

use jengine_core::behavior::catalog::{render_op_reference, replace_op_reference};

#[test]
fn behavior_op_reference_is_current() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/scripting-api.md");
    // Line endings are git's business (a Windows checkout has CRLF, the
    // generator writes LF), not a sign the reference is stale.
    let doc = std::fs::read_to_string(&path)
        .expect("read docs/scripting-api.md")
        .replace("\r\n", "\n");
    let expected = replace_op_reference(&doc, &render_op_reference())
        .expect("docs/scripting-api.md has lost its generated behaviour op block");
    assert!(
        expected == doc,
        "the behaviour op reference in docs/scripting-api.md is out of date.\n\
         Run: cargo run -p jengine-core --example behavior_op_reference\n\
         then `npm run sync` in website/."
    );
}
