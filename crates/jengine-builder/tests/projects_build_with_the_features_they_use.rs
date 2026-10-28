//! Every shipped project builds with the engine features its assets use
//! (`jengine_builder::engine_features`, `docs/engine-features-plan.md`).
//!
//! A build chooses its features from the project: what a scene, prefab,
//! script or data folder uses is in, the rest is left out. So the risk is a
//! game that works in the editor (which always has everything) and quietly
//! loses a subsystem in its build. This runs the choice over every project
//! here - it must succeed, which also means no `exclude` leaves out what the
//! project uses - and pins what the test lab is known to use, so a scan that
//! stops seeing something fails here rather than in a player's build. The
//! game hubs' pins are `hub_projects_build_with_the_features_they_use.rs`,
//! which the public export leaves out with the hubs.

use std::path::{Path, PathBuf};

use jengine_builder::engine_features::for_project;

fn projects() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("the projects folder")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("project.jengine").is_file())
        .collect();
    out.sort();
    out
}

#[test]
fn every_project_gets_a_feature_set() {
    let all = projects();
    // The public repository has three projects, this one many more; the test
    // lab is in both.
    assert!(
        all.iter().any(|p| p.ends_with("engine-test-lab")),
        "the walk found no test lab among {} projects",
        all.len()
    );
    let mut failures = Vec::new();
    for project in &all {
        match for_project(project) {
            Ok(selection) => println!("{}: {}", project.file_name().unwrap().to_string_lossy(), selection.summary()),
            Err(e) => failures.push(format!("{}: {e:#}", project.display())),
        }
    }
    assert!(failures.is_empty(), "these projects cannot choose their engine features:\n{}", failures.join("\n"));
}

/// The editor saves `project.jengine` through a typed `ProjectSettings`,
/// which erases a key it has no field for - so `engine_features` written by
/// hand would be gone after the next save from Project Settings, and the
/// build would quietly go back to choosing on its own.
#[test]
fn the_editor_keeps_the_setting_when_it_saves() {
    let src = include_str!("../../../editor/src-tauri/src/commands/project.rs");
    assert!(
        src.contains("pub engine_features: Option<serde_json::Value>"),
        "editor ProjectSettings has no `engine_features` field - a save from Project Settings would erase it"
    );
}

#[test]
fn known_projects_get_what_they_use() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects");
    let names = |p: &str| for_project(&root.join(p)).unwrap().names;
    // The test lab has a scene for each feature.
    let every: Vec<&str> = jengine_core::features::ALL.iter().map(|f| f.name).collect();
    assert_eq!(names("engine-test-lab"), every);
}
