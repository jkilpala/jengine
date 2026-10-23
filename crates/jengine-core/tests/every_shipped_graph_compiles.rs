//! **Every `.behavior` file in the repo compiles.**
//!
//! A graph that fails to compile does not fail loudly at runtime: the
//! entity is logged once, keeps no instance, and simply does nothing - an
//! enemy that stands still. And the compiler changes: in Sep 2026 an `"out"`
//! naming no output became an error where it used to be silently ignored,
//! and three advertised-but-unwirable outputs (`self`, `OnTick.dt`) were
//! removed from the catalog. Tests loaded a handful of graphs each; nothing
//! loaded all of them, so a rule change could break a shipped game's enemies
//! with every test green.

use jengine_core::behavior::{BehaviorAsset, compile};
use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        // Build output and the generated per-project crates are not sources.
        if name == "target" || name == ".jengine" || name == "node_modules" {
            continue;
        }
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "behavior") {
            out.push(p);
        }
    }
}

#[test]
fn every_shipped_graph_compiles() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for top in ["projects", "crates", "templates"] {
        collect(&root.join(top), &mut files);
    }
    files.sort();
    assert!(files.len() >= 20, "found only {} graphs - the walk broke", files.len());

    let mut broken = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap_or(f).display().to_string();
        let text = std::fs::read_to_string(f).unwrap();
        match BehaviorAsset::from_json(&text) {
            Err(e) => broken.push(format!("{rel}: {e}")),
            Ok(asset) => {
                if let Err(e) = compile(&asset) {
                    broken.push(format!("{rel}: {e}"));
                }
            }
        }
    }
    assert!(
        broken.is_empty(),
        "{} of {} shipped graphs do not compile - each is an entity that silently does \
         nothing:\n  {}",
        broken.len(),
        files.len(),
        broken.join("\n  ")
    );
}
