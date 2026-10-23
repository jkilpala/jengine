//! Package management Tauri commands.
//!
//! Exposes package listing, adding, removing, resolving, and editor schema
//! to the Svelte frontend.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use jengine_builder::packages::{self, ResolvedPackage};
use jengine_core::package::manifest::{PackageDependency, PackageSource};

/// Package info returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    /// Package name.
    pub name: String,
    /// Source: "path" or "git".
    pub source_type: String,
    /// Source path or git URL.
    pub source_location: String,
    /// Git tag, if any.
    pub tag: Option<String>,
    /// Whether the package is optional.
    pub optional: bool,
    /// Resolved local path (if resolved).
    pub local_path: Option<String>,
    /// Version from package.toml (if available).
    pub version: Option<String>,
    /// Description from package.toml (if available).
    pub description: Option<String>,
    /// Author from package.toml (if available).
    pub author: Option<String>,
}

impl PackageInfo {
    fn from_dependency(dep: &PackageDependency) -> Self {
        let (source_type, source_location, tag) = match &dep.source {
            PackageSource::Path { path } => ("path".to_string(), path.clone(), None),
            PackageSource::Git { url, tag, .. } => {
                ("git".to_string(), url.clone(), tag.clone())
            }
        };
        Self {
            name: dep.name.clone(),
            source_type,
            source_location,
            tag,
            optional: dep.optional,
            local_path: None,
            version: None,
            description: None,
            author: None,
        }
    }

    fn from_resolved(resolved: &ResolvedPackage) -> Self {
        let (source_type, source_location, tag) = match &resolved.source {
            PackageSource::Path { path } => ("path".to_string(), path.clone(), None),
            PackageSource::Git { url, tag, .. } => {
                ("git".to_string(), url.clone(), tag.clone())
            }
        };
        let (version, description, author) = match &resolved.manifest {
            Some(m) => (
                Some(m.package.version.clone()),
                Some(m.package.description.clone()),
                Some(m.package.author.clone()),
            ),
            None => (None, None, None),
        };
        Self {
            name: resolved.name.clone(),
            source_type,
            source_location,
            tag,
            optional: false,
            local_path: Some(resolved.local_path.to_string_lossy().to_string()),
            version,
            description,
            author,
        }
    }
}

/// Editor schema for a package's components (sent to frontend for generic rendering).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageEditorSchema {
    /// Component editor descriptors from the EditorRegistry.
    pub components: Vec<jengine_core::package::editor_registry::ComponentEditorInfo>,
    /// Menu items contributed by the package.
    pub menu_items: Vec<jengine_core::package::editor_registry::MenuItemDescriptor>,
    /// Toolbar actions contributed by the package.
    pub toolbar_actions: Vec<jengine_core::package::editor_registry::ToolbarAction>,
    /// Custom editor panels contributed by packages.
    pub panels: Vec<jengine_core::package::editor_registry::PanelDescriptor>,
}

/// List all package dependencies declared in the project.
#[tauri::command]
pub fn list_packages(project_path: String) -> Result<Vec<PackageInfo>, String> {
    let root = PathBuf::from(&project_path);
    let deps = packages::read_package_dependencies(&root)
        .map_err(|e| format!("Failed to read packages: {e}"))?;

    // Try to resolve for richer info; fall back to dependency-only info
    match packages::resolve_packages(&root) {
        Ok(resolved) => {
            let mut infos: Vec<PackageInfo> = resolved
                .iter()
                .map(PackageInfo::from_resolved)
                .collect();
            // Include optional packages that were skipped during resolution
            for dep in &deps {
                if dep.optional && !infos.iter().any(|i| i.name == dep.name) {
                    infos.push(PackageInfo::from_dependency(dep));
                }
            }
            Ok(infos)
        }
        Err(_) => Ok(deps.iter().map(PackageInfo::from_dependency).collect()),
    }
}

/// Add a package dependency to the project.
#[tauri::command]
pub fn add_package(
    project_path: String,
    name: String,
    source_type: String,
    source_location: String,
    tag: Option<String>,
    optional: bool,
) -> Result<(), String> {
    let root = PathBuf::from(&project_path);
    let source = match source_type.as_str() {
        "path" => PackageSource::Path {
            path: source_location,
        },
        "git" => PackageSource::Git {
            url: source_location,
            tag,
            rev: None,
        },
        other => return Err(format!("Unknown source type: {other}")),
    };
    let dep = PackageDependency {
        name,
        source,
        optional,
    };
    packages::add_package(&root, dep).map_err(|e| format!("Failed to add package: {e}"))
}

/// Remove a package dependency from the project.
#[tauri::command]
pub fn remove_package(project_path: String, name: String) -> Result<bool, String> {
    let root = PathBuf::from(&project_path);
    packages::remove_package(&root, &name).map_err(|e| format!("Failed to remove package: {e}"))
}

/// Resolve all packages (clone git repos, validate paths) and return their info.
#[tauri::command]
pub fn resolve_packages(project_path: String) -> Result<Vec<PackageInfo>, String> {
    let root = PathBuf::from(&project_path);
    let resolved = packages::resolve_packages(&root)
        .map_err(|e| format!("Failed to resolve packages: {e}"))?;
    Ok(resolved.iter().map(PackageInfo::from_resolved).collect())
}

/// Get the editor schema for enabled packages only.
///
/// Accepts a list of enabled package names and only registers those,
/// so the editor only shows components/menus/panels for active packages.
///
/// Also reads `editor.json` from resolved external packages and merges
/// their schema into the result.
#[tauri::command]
pub fn get_package_editor_schema(
    enabled_packages: Vec<String>,
    project_path: Option<String>,
) -> Result<PackageEditorSchema, String> {
    use jengine_core::package::editor_registry::EditorRegistry;
    use jengine_core::package::JEnginePackage;

    let mut editor = EditorRegistry::new();

    // All built-in packages with their implementations
    let builtins: Vec<Box<dyn JEnginePackage>> = vec![
        Box::new(jengine_physics::PhysicsPackage),
        Box::new(jengine_audio::AudioPackage),
        Box::new(jengine_particles::ParticlesPackage),
        Box::new(jengine_parallax::ParallaxPackage),
        Box::new(jengine_swarm::SwarmPackage),
    ];

    // Every core component, and every component of an enabled built-in
    // package, for the schemas derived below.
    let mut registry = jengine_core::package::component_registry::ComponentRegistry::new();
    jengine_core::package::core_registration::register_core_components(&mut registry);
    for pkg in &builtins {
        let meta = pkg.metadata();
        if enabled_packages.iter().any(|n| n == &meta.name) {
            pkg.register_editor(&mut editor);
            pkg.register_components(&mut registry);
        }
    }
    // A component with no hand-written metadata gets metadata derived from
    // itself, so it can be added and edited - see
    // `editor_registry::derived_component_info`. The Inspector's own
    // sections still win for the components in `componentDefs.ts`.
    for desc in registry.iter() {
        if editor.components.iter().any(|c| c.component_name == desc.name) {
            continue;
        }
        if let Some(info) = jengine_core::package::editor_registry::derived_component_info(desc) {
            editor.components.push(info);
        }
    }

    // Load editor.json from resolved external packages
    if let Some(ref path) = project_path {
        let root = PathBuf::from(path);
        if let Ok(resolved) = jengine_builder::packages::resolve_packages(&root) {
            for pkg in &resolved {
                let editor_json_path = pkg.local_path.join("editor.json");
                if editor_json_path.exists() {
                    match std::fs::read_to_string(&editor_json_path) {
                        Ok(contents) => {
                            match serde_json::from_str::<EditorRegistry>(&contents) {
                                Ok(ext_schema) => {
                                    log::info!(
                                        "Loaded editor.json for package '{}': {} component(s)",
                                        pkg.name,
                                        ext_schema.components.len()
                                    );
                                    editor.components.extend(ext_schema.components);
                                    editor.menu_items.extend(ext_schema.menu_items);
                                    editor.toolbar_actions.extend(ext_schema.toolbar_actions);
                                    editor.panels.extend(ext_schema.panels);
                                }
                                Err(e) => {
                                    log::warn!(
                                        "Invalid editor.json in package '{}': {e}",
                                        pkg.name
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!(
                                "Failed to read editor.json for '{}': {e}",
                                pkg.name
                            );
                        }
                    }
                }
            }
        }
    }

    Ok(PackageEditorSchema {
        components: editor.components,
        menu_items: editor.menu_items,
        toolbar_actions: editor.toolbar_actions,
        panels: editor.panels,
    })
}

/// Info about an available built-in package (for the package manager UI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailablePackage {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
}

/// List all available built-in packages with their metadata.
#[tauri::command]
pub fn get_available_packages() -> Vec<AvailablePackage> {
    use jengine_core::package::JEnginePackage;

    let builtins: Vec<Box<dyn JEnginePackage>> = vec![
        Box::new(jengine_physics::PhysicsPackage),
        Box::new(jengine_audio::AudioPackage),
        Box::new(jengine_particles::ParticlesPackage),
        Box::new(jengine_parallax::ParallaxPackage),
    ];

    builtins
        .iter()
        .map(|pkg| {
            let m = pkg.metadata();
            AvailablePackage {
                name: m.name,
                version: m.version,
                description: m.description,
                author: m.author,
            }
        })
        .collect()
}

/// Read a package panel's web bundle HTML file and return it as a string.
///
/// The `package_path` is the resolved local path of the package, and
/// `web_bundle_path` is the relative path to the HTML entry within it.
#[tauri::command]
pub fn read_package_panel_html(
    package_path: String,
    web_bundle_path: String,
) -> Result<String, String> {
    let base = PathBuf::from(&package_path);
    let entry = base.join(&web_bundle_path);

    // Security: ensure the resolved path is within the package directory
    let canonical_base = base
        .canonicalize()
        .map_err(|e| format!("Invalid package path: {e}"))?;
    let canonical_entry = entry
        .canonicalize()
        .map_err(|e| format!("Panel HTML not found: {e}"))?;
    if !canonical_entry.starts_with(&canonical_base) {
        return Err("Panel path escapes package directory".to_string());
    }

    std::fs::read_to_string(&canonical_entry)
        .map_err(|e| format!("Failed to read panel HTML: {e}"))
}

/// Detect package info from a local path by reading package.toml or Cargo.toml.
///
/// Returns the package name, version, and description so the "Add Package"
/// form can auto-fill them.
#[tauri::command]
pub fn detect_package_info(source_path: String) -> Result<DetectedPackageInfo, String> {
    let dir = PathBuf::from(&source_path);
    if !dir.is_dir() {
        return Err(format!("Not a directory: {source_path}"));
    }

    // Try package.toml first
    let manifest_path = dir.join("package.toml");
    if manifest_path.exists() {
        let contents = std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("Failed to read package.toml: {e}"))?;
        let manifest: jengine_core::package::manifest::PackageManifest =
            toml::from_str(&contents)
                .map_err(|e| format!("Invalid package.toml: {e}"))?;
        return Ok(DetectedPackageInfo {
            name: manifest.package.name,
            version: Some(manifest.package.version),
            description: if manifest.package.description.is_empty() {
                None
            } else {
                Some(manifest.package.description)
            },
        });
    }

    // Fall back to Cargo.toml
    let cargo_path = dir.join("Cargo.toml");
    if cargo_path.exists() {
        let contents = std::fs::read_to_string(&cargo_path)
            .map_err(|e| format!("Failed to read Cargo.toml: {e}"))?;
        let cargo: toml::Value =
            toml::from_str(&contents)
                .map_err(|e| format!("Invalid Cargo.toml: {e}"))?;
        if let Some(pkg) = cargo.get("package") {
            let name = pkg.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let version = pkg.get("version")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let description = pkg.get("description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            return Ok(DetectedPackageInfo {
                name,
                version,
                description,
            });
        }
    }

    Err("No package.toml or Cargo.toml found".to_string())
}

/// Package info detected from a local path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedPackageInfo {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
}

/// What *New Rust Package* would make of `name`, asked as the name is typed:
/// whether the builder accepts it, and the namespace scripts will call its
/// commands by. Both come from the engine's own rules
/// (`packages::validate_package_name`, `namespaces::package_namespace`), so
/// the dialog cannot promise a name the scaffold then refuses.
#[derive(Debug, Clone, Serialize)]
pub struct RustPackagePreview {
    /// Why the name cannot be used, or `None` when it can.
    pub error: Option<String>,
    /// `my-rules` is `MyRules`: `MyRules.nudge(e, 10)` from Lua.
    pub namespace: String,
}

#[tauri::command]
pub fn preview_rust_package(project_path: String, name: String) -> RustPackagePreview {
    let taken = packages::read_package_dependencies(&PathBuf::from(&project_path))
        .map(|deps| deps.iter().any(|d| d.name == name))
        .unwrap_or(false);
    let error = match packages::validate_package_name(&name) {
        Err(e) => Some(format!("{e:#}")),
        Ok(()) if taken => Some(format!("The project already lists a package called '{name}'")),
        Ok(()) => None,
    };
    RustPackagePreview { error, namespace: jengine_core::scripting::namespaces::package_namespace(&name) }
}

/// Scaffold a Rust package inside the project (`packages/<name>/`) and list
/// it in `project.jengine` - the editor's *New Rust Package*. Returns the
/// package's `src/lib.rs`, relative to the project, for the code editor.
#[tauri::command]
pub fn create_rust_package(project_path: String, name: String) -> Result<String, String> {
    packages::create_project_package(&PathBuf::from(&project_path), &name).map_err(|e| format!("{e:#}"))?;
    Ok(format!("packages/{name}/src/lib.rs"))
}
