//! A new project from one of the project templates.
//!
//! The editor's *Create New Project* and `jengine project new` both come here,
//! so a project made either way starts the same. The **starter** template
//! (`templates/project/`) is a `project.jengine` with input actions and every
//! built-in package enabled, and a v2 scene with a camera, a title and a
//! player that moves - something that runs on Play. The **empty** one
//! (`templates/project-empty/`) is the same settings with no scene, script or
//! input action: nothing to delete before starting your own.
//!
//! Until Oct 2026 the editor wrote a bare `project.jengine` and an empty
//! `assets/`, whose `enabled_packages` allow-list left out `jengine-swarm`,
//! and the CLI could not make a project at all - so an AI agent, which cannot
//! click the editor, had to write one by hand from the documentation.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Which files a new project starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectTemplate {
    /// A scene with a camera, a title and a moving player, its script and
    /// input actions.
    #[default]
    Starter,
    /// Project settings only.
    Empty,
}

impl ProjectTemplate {
    /// Every template, in the order a picker lists them.
    pub const ALL: [ProjectTemplate; 2] = [ProjectTemplate::Starter, ProjectTemplate::Empty];

    /// The name the CLI's `--template` and the editor pass.
    pub fn name(self) -> &'static str {
        match self {
            ProjectTemplate::Starter => "starter",
            ProjectTemplate::Empty => "empty",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.name() == name)
    }

    /// `(path under the project, template text)`.
    fn files(self) -> &'static [(&'static str, &'static str)] {
        match self {
            ProjectTemplate::Starter => &[
                ("project.jengine", include_str!("../../../templates/project/project.jengine")),
                ("assets/scenes/main.scene", include_str!("../../../templates/project/assets/scenes/main.scene")),
                ("assets/scripts/player.lua", include_str!("../../../templates/project/assets/scripts/player.lua")),
            ],
            ProjectTemplate::Empty => &[(
                "project.jengine",
                include_str!("../../../templates/project-empty/project.jengine"),
            )],
        }
    }
}

/// Folders a project's assets usually go in, created empty.
const DIRS: &[&str] = &[
    "assets/scenes",
    "assets/scripts",
    "assets/sprites",
    "assets/sounds",
    "assets/prefabs",
    "assets/ui",
];

/// Write a new project into `dir` (created if missing) named `name`, from
/// `template`.
///
/// Refuses a directory that already holds a `project.jengine`. Returns the
/// path of the new `project.jengine`.
pub fn create_project(dir: &Path, name: &str, template: ProjectTemplate) -> Result<PathBuf> {
    let manifest = dir.join("project.jengine");
    if manifest.exists() {
        bail!("{} already exists", manifest.display());
    }
    // The name lands inside JSON strings: escape it as JSON does, without the
    // surrounding quotes, so a name with a quote or backslash stays valid.
    let json_name = serde_json::to_string(name)?;
    let json_name = &json_name[1..json_name.len() - 1];

    for (rel, text) in template.files() {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let filled = if rel.ends_with(".lua") {
            text.replace("{{PROJECT_NAME}}", name)
        } else {
            text.replace("{{PROJECT_NAME}}", json_name)
        };
        let filled = filled.replace("{{ENGINE_VERSION}}", jengine_core::version::ENGINE_VERSION);
        std::fs::write(&path, filled).with_context(|| format!("writing {}", path.display()))?;
    }
    for rel in DIRS {
        std::fs::create_dir_all(dir.join(rel))?;
    }
    log::info!("Created project '{name}' at {} ({} template)", dir.display(), template.name());
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_project_is_complete_and_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = create_project(tmp.path(), "Quote \"Me\" Game", ProjectTemplate::Starter).unwrap();

        let project: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).expect("project.jengine is JSON");
        assert_eq!(project["name"], "Quote \"Me\" Game");
        assert_eq!(project["default_scene"], "scenes/main.scene");
        let enabled: Vec<&str> = project["settings"]["enabled_packages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        for builtin in ["jengine-physics", "jengine-audio", "jengine-particles", "jengine-parallax", "jengine-swarm"] {
            assert!(enabled.contains(&builtin), "{builtin} is not enabled");
        }

        // The scene loads through the engine's own loader, with every entity.
        let scene = std::fs::read_to_string(tmp.path().join("assets/scenes/main.scene")).unwrap();
        let mut registry = jengine_core::package::component_registry::ComponentRegistry::new();
        jengine_core::package::core_registration::register_core_components(&mut registry);
        let mut world = jengine_core::ecs::World::new();
        jengine_core::scene::load_scene_auto(&scene, &mut world, &registry).expect("main.scene loads");
        assert_eq!(world.len(), 4);

        // The script it names exists, and calls the engine by namespace.
        let script = std::fs::read_to_string(tmp.path().join("assets/scripts/player.lua")).unwrap();
        assert!(script.contains("Transform.get_position"));
        assert!(tmp.path().join("assets/sprites").is_dir());
    }

    #[test]
    fn an_existing_project_is_not_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        create_project(tmp.path(), "First", ProjectTemplate::Starter).unwrap();
        assert!(create_project(tmp.path(), "Second", ProjectTemplate::Empty).is_err());
    }

    #[test]
    fn an_empty_project_has_settings_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = create_project(tmp.path(), "Blank", ProjectTemplate::Empty).unwrap();

        let project: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).expect("project.jengine is JSON");
        assert_eq!(project["name"], "Blank");
        assert_eq!(project["settings"]["title"], "Blank");
        // No entry scene, so the editor opens an untitled one.
        assert!(project.get("default_scene").is_none());
        assert!(project["settings"].get("entry_scene").is_none());
        assert!(project["settings"].get("input_actions").is_none());
        // The same settings as the starter, apart from what it leaves out.
        let starter: serde_json::Value = serde_json::from_str(ProjectTemplate::Starter.files()[0].1).unwrap();
        for key in ["width", "height", "physics", "enabled_packages", "ui_scale_mode"] {
            assert_eq!(project["settings"][key], starter["settings"][key], "{key} differs from the starter's");
        }

        // Folders, but not a file in them.
        let files = walk(&tmp.path().join("assets"));
        assert!(files.is_empty(), "an empty project has files: {files:?}");
        assert!(tmp.path().join("assets/scenes").is_dir());
    }

    #[test]
    fn templates_are_named_both_ways() {
        for t in ProjectTemplate::ALL {
            assert_eq!(ProjectTemplate::from_name(t.name()), Some(t));
        }
        assert_eq!(ProjectTemplate::from_name("nope"), None);
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
        out
    }
}
