//! **What `jengine-server new` writes is a server that works.**
//!
//! Two layers. The `match` template's room is compiled into this test binary
//! and its own tests run here, on every `cargo test` - so the template cannot
//! rot between releases. And, slower and so only on request, each template is
//! generated whole and built and tested as the stranger's crate it will be:
//!
//! ```text
//! cargo test -p jengine-server --test a_generated_server -- --ignored
//! ```

#[allow(dead_code)]
#[path = "../templates/server/match_rooms.rs"]
mod match_rooms;

use std::path::PathBuf;
use std::process::Command;

use jengine_server::scaffold::{Options, Template, generate};

/// Generate each template beside the engine and run its tests with cargo.
/// The build directory is shared between them and kept, so a second run is
/// quick; the first builds the engine's server crate once more.
#[test]
#[ignore = "builds three crates with cargo: minutes the first time"]
fn every_template_builds_and_passes_its_own_tests() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/generated-servers");
    std::fs::create_dir_all(&root).unwrap();
    for template in [Template::Relay, Template::Match, Template::World] {
        let dir = root.join(format!("my-{}-server", template.name()));
        let _ = std::fs::remove_dir_all(&dir);
        generate(&Options { dir: dir.clone(), name: None, template, engine: None }).unwrap();

        let output = Command::new(env!("CARGO"))
            .args(["test", "--quiet"])
            .current_dir(&dir)
            .env("CARGO_TARGET_DIR", root.join("target"))
            .output()
            .expect("cargo runs");
        let (out, err) = (String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert!(output.status.success(), "the {} template does not pass:\n{out}\n{err}", template.name());
        // A room template that has no tests would pass by saying nothing.
        if template != Template::Relay {
            let ran = out.lines().any(|l| l.starts_with("test result: ok.") && !l.contains(" 0 passed"));
            assert!(ran, "{}: no tests ran:\n{out}", template.name());
        }
        assert!(!err.contains("warning: unused"), "the {} template has dead code:\n{err}", template.name());
    }
}
