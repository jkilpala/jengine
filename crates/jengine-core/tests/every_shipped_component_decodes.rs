//! **Every component in every shipped scene and prefab decodes.**
//!
//! A scene entity's components are JSON under their scene keys
//! (`scene::format::ComponentBundle`), decoded through the component registry
//! when the entity spawns, and one that does not decode is left out with a
//! log line - the entity spawns without its sprite or its collider. Until Oct
//! 2026 a v1 scene parsed into a struct with a typed field per component, so
//! a malformed one failed the parse and any test that merely parsed the
//! scene caught it. Parsing looks inside nothing now, so this decodes every
//! component the core registry knows, in v1 and v2 files alike, parts
//! included. A package's components are its own tests' to check.

use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::scene::format::ComponentBundle;
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
        } else if p.extension().is_some_and(|x| x == "scene" || x == "prefab") {
            out.push(p);
        }
    }
}

/// Check one entity's components, and its parts', into `broken`.
fn check_entity(entity: &serde_json::Value, registry: &ComponentRegistry, at: &str, broken: &mut Vec<String>) {
    let name = entity.get("name").and_then(|n| n.as_str()).unwrap_or("?");
    if let Some(components) = entity.get("components") {
        match serde_json::from_value::<ComponentBundle>(components.clone()) {
            Ok(bundle) => {
                for (key, why) in bundle.check(registry) {
                    broken.push(format!("{at} '{name}' {key}: {why}"));
                }
            }
            Err(e) => broken.push(format!("{at} '{name}': components are not an object: {e}")),
        }
    }
    for part in entity.get("children").and_then(|c| c.as_array()).into_iter().flatten() {
        check_entity(part, registry, at, broken);
    }
}

#[test]
fn every_shipped_component_decodes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for top in ["projects", "templates", "crates"] {
        collect(&root.join(top), &mut files);
    }
    files.sort();
    assert!(files.len() >= 500, "found only {} scenes and prefabs - the walk broke", files.len());

    let mut registry = ComponentRegistry::new();
    jengine_core::package::core_registration::register_core_components(&mut registry);

    let mut broken = Vec::new();
    for f in &files {
        let rel = f.strip_prefix(&root).unwrap_or(f).display().to_string().replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                broken.push(format!("{rel}: not JSON: {e}"));
                continue;
            }
        };
        match value.get("entities").and_then(|e| e.as_array()) {
            // A scene: its entities.
            Some(entities) => {
                for entity in entities {
                    check_entity(entity, &registry, &rel, &mut broken);
                }
            }
            // A prefab: one entity.
            None => check_entity(&value, &registry, &rel, &mut broken),
        }
    }
    assert!(
        broken.is_empty(),
        "{} shipped components do not decode - each is an entity spawned without it:\n  {}",
        broken.len(),
        broken.join("\n  ")
    );
}

#[test]
fn a_malformed_component_is_found() {
    let mut registry = ComponentRegistry::new();
    jengine_core::package::core_registration::register_core_components(&mut registry);
    let entity = serde_json::json!({ "name": "Box", "components": {
        "transform": { "position": [0, 0], "rotation": 0, "scale": [1, 1] },
        "sprite": { "size": "big" },
        "Stats": { "hp": 3 }
    }});
    let mut broken = Vec::new();
    check_entity(&entity, &registry, "test", &mut broken);
    assert_eq!(broken.len(), 1, "the sprite and only the sprite - a custom component is not checked: {broken:?}");
    assert!(broken[0].contains("sprite"), "{broken:?}");
}
