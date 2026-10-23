//! **Every component a shipped scene or prefab names must exist.**
//!
//! A scene's `components` map keeps any key it does not know - it becomes a
//! custom component (`ComponentBundle.custom`), which is how a game's own
//! `Stats` bag works. So a misspelt built-in is not an error anywhere: it
//! loads, it saves, it survives the editor, and it does nothing. The
//! showcase's Ten Thousand Lights shipped its ten thousand lanterns with
//! `"velocity2d"` (the component is `velocity`), so not one of them ever
//! rose, and nothing said so until the Trails scene's meteors hung at the
//! top of the sky with the same key.
//!
//! Custom components are named in CapitalCase by convention (`Stats`,
//! `Inventory`); every built-in and package key is snake_case. So a
//! lowercase key that nothing registers is a typo, and this fails on it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every key in a `registry.register::<T>("key", ...)` call under `dir`.
fn registered_in(dir: &Path, out: &mut BTreeSet<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !path.ends_with("target") && !path.ends_with(".jengine") && !path.ends_with("node_modules") {
                registered_in(&path, out);
            }
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&path) else { continue };
        // `register_defaulted` registers the same way, with a `Default` for the editor.
        let src = src.replace("register_defaulted::<", "register::<");
        let mut rest = src.as_str();
        while let Some(i) = rest.find("register::<") {
            rest = &rest[i..];
            let Some(open) = rest.find('(') else { break };
            let after = rest[open + 1..].trim_start();
            if let Some(stripped) = after.strip_prefix('"')
                && let Some(end) = stripped.find('"')
            {
                out.insert(stripped[..end].to_string());
            }
            rest = &rest[open..];
        }
    }
}

fn authored(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !path.ends_with(".jengine") && !path.ends_with("node_modules") && !path.ends_with("target") {
                authored(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "scene" || e == "prefab") {
            out.push(path);
        }
    }
}

fn is_snake(key: &str) -> bool {
    !key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[test]
fn every_lowercase_component_key_is_registered() {
    let mut keys = BTreeSet::new();
    registered_in(&repo().join("crates/jengine-core/src"), &mut keys);
    registered_in(&repo().join("packages"), &mut keys);
    registered_in(&repo().join("projects"), &mut keys);
    for k in ["transform", "sprite", "velocity", "trail", "rigid_body"] {
        assert!(keys.contains(k), "the key extraction broke: no {k:?} in {} keys", keys.len());
    }

    let mut files = Vec::new();
    authored(&repo().join("projects"), &mut files);
    assert!(files.len() > 100, "only {} scenes and prefabs - the walk broke", files.len());

    let mut problems = BTreeSet::new();
    for file in &files {
        let Ok(src) = std::fs::read_to_string(file) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&src) else { continue };
        let entities: Vec<&serde_json::Value> = match v.get("entities").and_then(|e| e.as_array()) {
            Some(list) => list.iter().collect(),
            None => vec![&v],
        };
        for e in entities {
            let Some(components) = e.get("components").and_then(|c| c.as_object()) else { continue };
            for key in components.keys() {
                if is_snake(key) && !keys.contains(key) {
                    let rel = file.strip_prefix(repo()).unwrap_or(file).display().to_string();
                    problems.insert(format!("{key:?} in {rel}"));
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "components nothing registers - misspelt built-ins that load as dead custom data:\n  {}",
        problems.into_iter().collect::<Vec<_>>().join("\n  ")
    );
}
