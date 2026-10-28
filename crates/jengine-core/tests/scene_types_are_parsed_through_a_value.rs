//! Scene data is parsed from text by way of a `serde_json::Value`
//! (`scene::from_json_text`), never with `serde_json::from_str` straight into
//! an `EntityData`, `SceneData` or `ComponentBundle`.
//!
//! Serde's code is generated per reader. Every loader already reads a
//! `Value`, so one direct parse compiles a second deserializer for every
//! component the engine has - a `from_str::<Vec<EntityData>>` in the browser
//! hosts was 616 KB of the web module (238 KB gzipped), and nothing about it
//! is visible except the size. `docs/build-sizes.md`.

use std::path::{Path, PathBuf};

/// The v1 scene types: each holds a `ComponentBundle`, a typed field per
/// component. The v2 ones hold `Value`s and cost nothing to parse directly.
const TYPES: [&str; 3] = ["EntityData", "SceneData", "ComponentBundle"];

/// Whether `line` parses one of [`TYPES`] straight from text.
fn parses_directly(line: &str) -> bool {
    let code = line.trim_start();
    if code.starts_with("//") {
        return false;
    }
    let reads_text = ["serde_json::from_str", "serde_json::from_slice", "serde_json::from_reader"]
        .iter()
        .any(|f| code.contains(f));
    reads_text && TYPES.iter().any(|t| names_type(code, t))
}

/// `ty` appears as a whole identifier (so `EntityDataV2` is not `EntityData`).
fn names_type(code: &str, ty: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    code.match_indices(ty).any(|(i, _)| {
        let before = code[..i].chars().next_back().is_none_or(|c| !ident(c));
        let after = code[i + ty.len()..].chars().next().is_none_or(|c| !ident(c));
        before && after
    })
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_matcher_knows_a_direct_parse_when_it_sees_one() {
    assert!(parses_directly("    let kids: Vec<EntityData> = serde_json::from_str(json)?;"));
    assert!(parses_directly("match serde_json::from_str::<jengine_core::scene::format::EntityData>(json) {"));
    assert!(parses_directly("let s: SceneData = serde_json::from_slice(&bytes)?;"));
    assert!(!parses_directly("let s: SceneDataV2 = serde_json::from_str(&json)?;"));
    assert!(!parses_directly("let kids: Vec<EntityData> = crate::scene::from_json_text(json)?;"));
    assert!(!parses_directly("/// one `serde_json::from_str::<Vec<EntityData>>` cost 616 KB"));
}

#[test]
fn scene_types_are_parsed_through_a_value() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for parent in ["crates", "packages"] {
        let Ok(members) = std::fs::read_dir(repo.join(parent)) else { continue };
        for member in members.flatten() {
            rust_files(&member.path().join("src"), &mut files);
        }
    }
    assert!(files.len() > 100, "found only {} source files - wrong root?", files.len());
    let mut found = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        // Tests may parse however they like: they are not shipped.
        let shipped = text.split("#[cfg(test)]").next().unwrap_or("");
        for (n, line) in shipped.lines().enumerate() {
            if parses_directly(line) {
                let rel = file.strip_prefix(&repo).unwrap_or(file).display().to_string().replace('\\', "/");
                found.push(format!("{rel}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "parse these through `jengine_core::scene::from_json_text` instead - each direct parse \
         compiles a second deserializer for every component:\n{}",
        found.join("\n")
    );
}
