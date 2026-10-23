//! **A scene using a package component must belong to a project that links it.**
//!
//! `project.jengine` has two package lists and they do different jobs:
//!
//! * top-level `"packages"` — read by `jengine_builder::packages`, which turns
//!   each entry into a Cargo path dependency in the generated runtime and
//!   viewport crates. **This is the one that makes a package exist.**
//! * `settings.enabled_packages` — read by the runtime to gate the built-in
//!   four it was compiled with (physics, audio, particles, parallax). Naming a
//!   project's own package here does nothing at all.
//!
//! Putting a package in the second list and not the first is silent in every
//! way that matters: the project loads, the scene loads, the component key is
//! simply not registered so it lands in `CustomComponents` instead, and the
//! package's script commands are never registered - so the first sign is
//! `attempt to call a nil value (global 'dr_start')` at the moment a player
//! presses Play. That is exactly how one game shipped broken (Sep 2026).
//!
//! So: extract every component key each package registers, find which scenes
//! use those keys, and require the owning project to list the package.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `component key -> package name`, read from each package's
/// `registry.register::<T>("key", …)` calls.
fn component_owners() -> BTreeMap<String, String> {
    // Keys core already owns. `jengine-physics` re-registers `rigid_body`,
    // `collider` and `character_controller`, so without this every scene with
    // a physics body looks like it is missing a package it does not need.
    let core = std::fs::read_to_string(
        repo().join("crates/jengine-core/src/package/core_registration.rs"),
    )
    .expect("core_registration.rs")
    .replace("register_defaulted::<", "register::<");
    let mut core_keys = BTreeSet::new();
    let mut rest = core.as_str();
    while let Some(i) = rest.find("registry.register::<") {
        rest = &rest[i..];
        let Some(q) = rest.find('"') else { break };
        let after = &rest[q + 1..];
        let Some(end) = after.find('"') else { break };
        core_keys.insert(after[..end].to_string());
        rest = &after[end..];
    }
    assert!(core_keys.len() > 20, "only {} core components - the parse broke", core_keys.len());

    let mut out = BTreeMap::new();
    let dir = repo().join("packages");
    for entry in std::fs::read_dir(&dir).expect("packages/ exists").flatten() {
        let pkg = entry.file_name().to_string_lossy().to_string();
        let lib = entry.path().join("src/lib.rs");
        let Ok(src) = std::fs::read_to_string(&lib) else { continue };
        // `register_defaulted` registers the same way, with a `Default` for the editor.
        let src = src.replace("register_defaulted::<", "register::<");
        let mut rest = src.as_str();
        while let Some(i) = rest.find("registry.register::<") {
            rest = &rest[i..];
            let Some(q) = rest.find('"') else { break };
            let after = &rest[q + 1..];
            let Some(end) = after.find('"') else { break };
            let key = after[..end].to_string();
            if !core_keys.contains(&key) {
                out.entry(key).or_insert(pkg.clone());
            }
            rest = &after[end..];
        }
    }
    out
}

fn scenes(project: &Path) -> Vec<PathBuf> {
    let dir = project.join("assets/scenes");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "scene"))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Package names in the top-level `"packages"` array - the list that links.
fn linked(project: &Path) -> BTreeSet<String> {
    let src = std::fs::read_to_string(project.join("project.jengine")).unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&src).unwrap_or(serde_json::Value::Null);
    v["packages"]
        .as_array()
        .map(|a| {
            a.iter().filter_map(|p| p["name"].as_str().map(str::to_string)).collect()
        })
        .unwrap_or_default()
}

/// The packages the runtime is **compiled with** and gates by name, read from
/// its own `is_on("…")` calls. These need no entry in `"packages"` - they are
/// already linked, which is what makes them built-in.
fn built_in() -> BTreeSet<String> {
    let src = std::fs::read_to_string(
        repo().join("crates/jengine-runtime/src/wgpu_scene_runner.rs"),
    )
    .expect("the runtime source");
    let mut out = BTreeSet::new();
    let mut rest = src.as_str();
    while let Some(i) = rest.find("is_on(\"") {
        rest = &rest[i + "is_on(\"".len()..];
        let Some(end) = rest.find('"') else { break };
        out.insert(rest[..end].to_string());
        rest = &rest[end..];
    }
    assert!(!out.is_empty(), "found no `is_on(..)` calls - the extraction broke");
    out
}

fn projects() -> Vec<PathBuf> {
    std::fs::read_dir(repo().join("projects"))
        .expect("projects/ exists")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("project.jengine").exists())
        .collect()
}

#[test]
fn every_scene_component_a_package_owns_has_that_package_linked() {
    let owners = component_owners();
    // Components of the engine's own packages, which every checkout has.
    for (key, pkg) in [("online", "jengine-online"), ("swarm_spawner", "jengine-swarm")] {
        assert_eq!(
            owners.get(key).map(String::as_str),
            Some(pkg),
            "{key} is not {pkg}'s among {} package components - the extraction broke",
            owners.len()
        );
    }

    let built_in = built_in();
    let mut problems = Vec::new();
    for project in projects() {
        let listed = linked(&project);
        for scene in scenes(&project) {
            let Ok(src) = std::fs::read_to_string(&scene) else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&src) else { continue };
            let Some(entities) = v["entities"].as_array() else { continue };
            for e in entities {
                let Some(comps) = e["components"].as_object() else { continue };
                for key in comps.keys() {
                    let Some(pkg) = owners.get(key) else { continue };
                    if built_in.contains(pkg) {
                        continue;
                    }
                    if !listed.contains(pkg) {
                        problems.push(format!(
                            "{}: entity `{}` has `{key}`, owned by `{pkg}`, which \
                             {} does not list under top-level \"packages\"",
                            scene.file_name().unwrap().to_string_lossy(),
                            e["name"].as_str().unwrap_or("?"),
                            project.file_name().unwrap().to_string_lossy(),
                        ));
                    }
                }
            }
        }
    }
    problems.sort();
    problems.dedup();
    assert!(
        problems.is_empty(),
        "a scene uses a package component the project does not link. The component will \
         land in `CustomComponents` and the package's script commands will be nil at \
         runtime:\n  {}\n\n\
         Note `settings.enabled_packages` is NOT this list - it only gates the built-in \
         four the runtime is compiled with.",
        problems.join("\n  ")
    );
}

/// Every file under `dir` with extension `ext`, recursively.
fn files_with(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            files_with(&p, ext, out);
        } else if p.extension().is_some_and(|e| e == ext) {
            out.push(p);
        }
    }
}

/// A clip a package renders (`jengine_core::audio::transcode`) needs that
/// package linked, or it fails to load - "no player for .song" in the log,
/// and silence where the music was. Scenes name clips in `audio_refs`;
/// scripts name them to `Audio.play_music`.
#[test]
fn every_song_a_project_plays_has_its_player_linked() {
    let mut problems = Vec::new();
    for project in projects() {
        let listed = linked(&project);
        let mut named: Vec<(String, String)> = Vec::new();
        for scene in scenes(&project) {
            let Ok(src) = std::fs::read_to_string(&scene) else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&src) else { continue };
            for clip in v["audio_refs"].as_array().into_iter().flatten().filter_map(|c| c.as_str()) {
                named.push((scene.file_name().unwrap().to_string_lossy().into_owned(), clip.to_string()));
            }
        }
        let mut scripts = Vec::new();
        files_with(&project.join("assets"), "lua", &mut scripts);
        for script in scripts {
            let Ok(src) = std::fs::read_to_string(&script) else { continue };
            for (i, _) in src.match_indices(".song\"") {
                let start = src[..i].rfind('"').map_or(i, |q| q + 1);
                named.push((script.file_name().unwrap().to_string_lossy().into_owned(), src[start..i + 5].to_string()));
            }
        }
        for (file, clip) in named {
            let Some(pkg) = jengine_core::audio::transcode::package_for(&clip) else { continue };
            if !listed.contains(pkg) {
                problems.push(format!(
                    "{}/{file}: plays `{clip}`, which needs `{pkg}` under top-level \"packages\"",
                    project.file_name().unwrap().to_string_lossy()
                ));
            }
        }
    }
    problems.sort();
    problems.dedup();
    assert!(problems.is_empty(), "a project plays a clip it has no player for:\n  {}", problems.join("\n  "));
}

// There is deliberately no test on `settings.enabled_packages`. It looks like
// the place to declare a package and is not - it gates only the built-ins the
// runtime was compiled with - but the arcade already lists eight of its own
// packages there, harmlessly and inconsistently (the first twelve games were
// added, later ones were not). Failing the build on that would be imposing a
// rule nobody adopted, on entries that do nothing either way. The list above
// is the one that decides whether a package exists, so that is the one
// guarded.
