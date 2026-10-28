//! Project management Tauri commands.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

use jengine_core::package::manifest::PackageDependency;

/// Supported field types for custom component fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FieldType {
    Float,
    Int,
    String,
    Vec2,
    Bool,
    Color,
}

/// A single field in a custom component definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentFieldDef {
    pub name: String,
    pub field_type: FieldType,
    pub default_value: serde_json::Value,
}

/// A custom component definition (schema).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentDef {
    pub name: String,
    pub fields: Vec<ComponentFieldDef>,
}

/// Physics settings for the project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicsSettings {
    #[serde(default)]
    pub gravity_x: f32,
    #[serde(default = "default_gravity_y")]
    pub gravity_y: f32,
    /// Physics backend: "rapier2d" (default) or "none" to disable physics.
    #[serde(default = "default_physics_backend")]
    pub backend: String,
    /// Fixed timestep for physics simulation in seconds (default 1/60).
    #[serde(default = "default_fixed_timestep")]
    pub fixed_timestep: f32,
    /// How many world units equal 1 meter (default 1.0). Set higher for pixel-scale games.
    #[serde(default = "default_length_unit")]
    pub length_unit: f32,
    /// Named collision layers (16 max).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision_layer_names: Option<Vec<String>>,
    /// 16-element array of u16 bitmasks defining which layer pairs collide. Symmetric.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision_matrix: Option<Vec<u16>>,
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            gravity_x: 0.0,
            gravity_y: -9.8,
            backend: "rapier2d".to_string(),
            fixed_timestep: 1.0 / 60.0,
            length_unit: 1.0,
            collision_layer_names: None,
            collision_matrix: None,
        }
    }
}

fn default_physics_backend() -> String {
    "rapier2d".to_string()
}

fn default_gravity_y() -> f32 {
    -9.8
}

fn default_fixed_timestep() -> f32 {
    1.0 / 60.0
}

fn default_length_unit() -> f32 {
    1.0
}

fn default_width() -> u32 {
    1280
}
fn default_height() -> u32 {
    720
}
fn default_bg_color() -> [f32; 4] {
    [0.1, 0.1, 0.12, 1.0]
}
fn default_fps() -> u32 {
    60
}
fn default_title() -> String {
    "jEngine Game".to_string()
}

/// A named input action bound to one or more keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputAction {
    pub name: String,
    pub keys: Vec<String>,
}

/// Game settings stored in project.jengine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default = "default_bg_color")]
    pub background_color: [f32; 4],
    #[serde(default = "default_fps")]
    pub target_fps: u32,
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_scene: Option<String>,
    #[serde(default)]
    pub physics: PhysicsSettings,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_actions: Vec<InputAction>,
    /// Preferred external code editor command (e.g. "code", "cursor"). Empty means auto-detect.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub code_editor: String,
    /// Asset pipeline (atlas packing & compression) settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_pipeline: Option<jengine_builder::asset_pipeline::AssetPipelineConfig>,
    /// Which built-in packages are enabled for this project.
    /// Defaults to all available packages for backward compatibility.
    #[serde(default = "default_enabled_packages")]
    pub enabled_packages: Vec<String>,
    /// Graphics backends compiled into each platform's build.
    #[serde(default)]
    pub graphics_backends: GraphicsBackends,
    /// What the game is called once installed on a phone: id, name,
    /// version, icon, orientation. Kept here as well as read by the builder
    /// because this struct is what a save writes - a field it lacked would be
    /// erased by the first save from Project Settings.
    #[serde(default, skip_serializing_if = "jengine_builder::app::AppSettings::is_unset")]
    pub app: jengine_builder::app::AppSettings,
    /// The most frames a second each platform's build draws
    /// (`jengine_core::time::FrameRateCaps`).
    #[serde(default, skip_serializing_if = "is_default_caps")]
    pub frame_rate_cap: jengine_core::time::FrameRateCaps,
    /// Android-only numbers: SDK levels and ABIs.
    #[serde(default, skip_serializing_if = "jengine_builder::app::AndroidSettings::is_unset")]
    pub android: jengine_builder::app::AndroidSettings,
    /// What the UI does when the window is not `width` x `height`.
    ///
    /// `"ScaleWithScreen"` (the default) scales both UI systems uniformly so a
    /// UI authored once fits any window; `"ScaleAndCenter"` scales the same
    /// but lays the UI out at the authored size, centred, for a UI placed in
    /// absolute positions; `"ConstantPixelSize"` is the old
    /// behaviour, one UI pixel per screen pixel, for projects whose scripts
    /// already fit their own UI. Absent means scale - see
    /// `jengine_core::ui::scaling`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_scale_mode: Option<String>,
    /// How textures are sampled - nearest or filtered with mipmaps - by
    /// default and per texture-id prefix
    /// (`jengine_core::renderer::texture_filter`). Kept here because this
    /// struct is what a save writes: a field it lacked would be erased.
    #[serde(default, skip_serializing_if = "jengine_core::renderer::texture_filter::TextureFilterRules::is_default")]
    pub texture_filter: jengine_core::renderer::texture_filter::TextureFilterRules,
    /// Which of two GPUs a built game asks for
    /// (`jengine_core::renderer::gpu_preference`).
    #[serde(default, skip_serializing_if = "jengine_core::renderer::gpu_preference::GpuPreference::is_default")]
    pub gpu_preference: jengine_core::renderer::gpu_preference::GpuPreference,
    /// The project's mixer: buses and their effects before any script
    /// touches them (`jengine_core::audio::MixerLayout`, Project Settings ->
    /// Audio). Kept here for the same reason as `texture_filter`.
    #[serde(default, skip_serializing_if = "jengine_core::audio::MixerLayout::is_empty")]
    pub audio_mixer: jengine_core::audio::MixerLayout,
    /// Asset packs a build moves out of the game, downloaded when a script
    /// asks (`jengine_core::asset::packs`, Project Settings -> Asset Packs).
    /// Kept here for the same reason as `texture_filter`.
    #[serde(default, skip_serializing_if = "jengine_core::asset::packs::PackSettings::is_empty")]
    pub asset_packs: jengine_core::asset::packs::PackSettings,
    /// Which engine features a build carries beyond what the project's
    /// assets use: `{ "include": [..], "exclude": [..], "all": true }`
    /// (`jengine_builder::engine_features`). Read by the builder only; kept
    /// here for the same reason as `texture_filter`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_features: Option<serde_json::Value>,
    /// Live ops: the service, the channel, its public key and the values
    /// the game ships with (`jengine_core::liveops`, Project Settings -> Live
    /// Ops). Kept here for the same reason as `texture_filter`.
    #[serde(default, skip_serializing_if = "is_default_liveops")]
    pub liveops: jengine_core::liveops::LiveOpsSettings,
    /// The web portal a web build is made for by default - `none`,
    /// `jcabinet`, `poki`, `crazygames` or `youtube` - and when it hears the
    /// game is ready to play (`first_frame` or `script`); Project Settings ->
    /// Web, read by `jengine_builder::web_platform`. Kept here for the same
    /// reason as `texture_filter`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub web_platform: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub web_game_ready: String,
    /// Extra `<script>` tags a web build's page loads (`web.rs`
    /// `read_web_scripts`). Missing from this struct until Oct 2026, so the
    /// first save from Project Settings erased it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub web_scripts: Vec<String>,
}

fn is_default_liveops(s: &jengine_core::liveops::LiveOpsSettings) -> bool {
    *s == jengine_core::liveops::LiveOpsSettings::default()
}

/// Which graphics APIs each platform's build carries.
///
/// wgpu compiles a separate code path per API and picks one at runtime, so
/// a build should only include what its target can actually use. Listing
/// more than one is valid — wgpu falls back in order — it just costs size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphicsBackends {
    /// Browser builds. `webgpu` alone is the smallest; add `webgl` for a
    /// fallback on browsers without WebGPU (Firefox stable, Safari < 18).
    #[serde(default = "default_web_backends")]
    pub web: Vec<String>,
    /// Windows native. `dx12` is the platform default; `vulkan` is the
    /// alternative for drivers where DX12 misbehaves.
    #[serde(default = "default_windows_backends")]
    pub windows: Vec<String>,
    /// Reserved for the platforms the builder does not target yet — a
    /// macOS/iOS build wants `metal`, Android wants `vulkan`. Stored now so
    /// projects can express it before the build paths land.
    #[serde(default = "default_macos_backends")]
    pub macos: Vec<String>,
    #[serde(default = "default_ios_backends")]
    pub ios: Vec<String>,
    #[serde(default = "default_android_backends")]
    pub android: Vec<String>,
}

fn is_default_caps(caps: &jengine_core::time::FrameRateCaps) -> bool {
    *caps == jengine_core::time::FrameRateCaps::default()
}

fn default_web_backends() -> Vec<String> {
    vec!["webgpu".to_string()]
}
fn default_windows_backends() -> Vec<String> {
    vec!["dx12".to_string()]
}
fn default_macos_backends() -> Vec<String> {
    vec!["metal".to_string()]
}
fn default_ios_backends() -> Vec<String> {
    vec!["metal".to_string()]
}
fn default_android_backends() -> Vec<String> {
    vec!["vulkan".to_string()]
}

impl Default for GraphicsBackends {
    fn default() -> Self {
        Self {
            web: default_web_backends(),
            windows: default_windows_backends(),
            macos: default_macos_backends(),
            ios: default_ios_backends(),
            android: default_android_backends(),
        }
    }
}

fn default_enabled_packages() -> Vec<String> {
    vec![
        "jengine-physics".to_string(),
        "jengine-audio".to_string(),
        "jengine-particles".to_string(),
        "jengine-parallax".to_string(),
        // Every built-in the runtimes link (`get_package_editor_schema`'s list).
        // The list is an allow-list once written, and swarm was missing from it,
        // so no project created in the editor could use a swarm.
        "jengine-swarm".to_string(),
    ]
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            width: default_width(),
            height: default_height(),
            background_color: default_bg_color(),
            target_fps: default_fps(),
            title: default_title(),
            entry_scene: None,
            physics: PhysicsSettings::default(),
            input_actions: Vec::new(),
            code_editor: String::new(),
            asset_pipeline: None,
            enabled_packages: default_enabled_packages(),
            graphics_backends: GraphicsBackends::default(),
            // Absent means scale, so a project written before the setting
            // existed gets the fitting behaviour rather than the old spill.
            ui_scale_mode: None,
            texture_filter: Default::default(),
            gpu_preference: Default::default(),
            audio_mixer: Default::default(),
            asset_packs: Default::default(),
            engine_features: None,
            liveops: Default::default(),
            app: Default::default(),
            android: Default::default(),
            frame_rate_cap: Default::default(),
            web_platform: String::new(),
            web_game_ready: String::new(),
            web_scripts: Vec::new(),
        }
    }
}

/// On-disk project file format (project.jengine).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFile {
    pub name: String,
    /// The engine that last saved the project (`jengine_core::version`).
    /// A save writes the newer of it and this engine's, never an older one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_scene: Option<String>,
    #[serde(default)]
    pub settings: ProjectSettings,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub component_definitions: Vec<ComponentDef>,
    /// Package dependencies for this project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<PackageDependency>,
    /// The project's test server (`jengine_builder::servers`), carried
    /// through a save untouched - the editor has no form for it, and a save
    /// that dropped it would take the Servers panel's settings with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<serde_json::Value>,
}

/// Runtime project info returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    /// Absolute path to the project root directory.
    pub path: String,
    pub default_scene: Option<String>,
    pub settings: ProjectSettings,
    pub component_definitions: Vec<ComponentDef>,
    /// Package dependencies declared in project.jengine.
    pub packages: Vec<PackageDependency>,
    pub scene_files: Vec<String>,
    pub script_files: Vec<String>,
    pub sprite_files: Vec<String>,
    pub prefab_files: Vec<String>,
    pub gui_files: Vec<String>,
    pub anim_files: Vec<String>,
    /// `.behavior` graph asset files (Step 7 / Slice 7.7).
    pub behavior_files: Vec<String>,
}

/// A node in the asset tree (file or directory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetNode {
    /// Display name (filename or folder name).
    pub name: String,
    /// Path relative to `assets/` using forward slashes.
    pub path: String,
    /// True if this node is a directory.
    pub is_dir: bool,
    /// Children (only populated for directories).
    pub children: Vec<AssetNode>,
}

/// Create a new project from a template - `starter` (a scene, a player
/// script, input actions; the default) or `empty` (settings only) - then open
/// it as `open_project` does. `jengine project new` writes the same files.
#[tauri::command]
pub fn create_project(
    app: tauri::AppHandle,
    path: String,
    name: String,
    template: Option<String>,
) -> Result<ProjectInfo, String> {
    use jengine_builder::new_project::{ProjectTemplate, create_project};
    let template = match template.as_deref() {
        None => ProjectTemplate::default(),
        Some(t) => ProjectTemplate::from_name(t).ok_or_else(|| format!("Unknown project template: {t}"))?,
    };
    create_project(&PathBuf::from(&path), &name, template)
        .map_err(|e| format!("Failed to create project: {e:#}"))?;
    open_project(app, path)
}

/// Open a project by selecting its project.jengine file or directory containing one.
#[tauri::command]
pub fn open_project(app: tauri::AppHandle, path: String) -> Result<ProjectInfo, String> {
    let input_path = PathBuf::from(&path);

    // Accept either the .jengine file or the directory containing it
    let (project_dir, project_file_path) = if input_path.is_file() {
        let dir = input_path
            .parent()
            .ok_or("Invalid project file path")?
            .to_path_buf();
        (dir, input_path)
    } else {
        let file = input_path.join("project.jengine");
        (input_path, file)
    };

    if !project_file_path.exists() {
        return Err(format!(
            "No project.jengine found at: {}",
            project_file_path.display()
        ));
    }

    let contents = std::fs::read_to_string(&project_file_path)
        .map_err(|e| format!("Failed to read project file: {e}"))?;
    let project: ProjectFile =
        serde_json::from_str(&contents).map_err(|e| format!("Invalid project file: {e}"))?;
    if let Some(warning) = jengine_core::version::newer_project_warning(project.engine_version.as_deref()) {
        log::warn!("{warning}");
    }

    let project_dir_str = project_dir
        .to_str()
        .ok_or("Project path is not valid UTF-8")?
        .to_string();

    let assets_dir = project_dir.join("assets");
    let scene_files = list_files_with_ext(&assets_dir, &assets_dir, "scene");
    let script_files = {
        let mut files = list_files_with_ext(&assets_dir, &assets_dir, "lua");
        files.extend(list_files_with_ext(&assets_dir, &assets_dir, "js"));
        files
    };
    let sprite_files = {
        let mut files = list_files_with_ext(&assets_dir, &assets_dir, "png");
        files.extend(list_files_with_ext(&assets_dir, &assets_dir, "jpg"));
        files
    };
    let prefab_files = list_files_with_suffix(&assets_dir, &assets_dir, ".prefab");
    let gui_files = list_files_with_ext(&assets_dir, &assets_dir, "gui");
    let anim_files = list_files_with_ext(&assets_dir, &assets_dir, "anim");
    let behavior_files = list_files_with_ext(&assets_dir, &assets_dir, "behavior");

    add_to_recent(&app, &project.name, &project_dir_str);

    Ok(ProjectInfo {
        name: project.name,
        path: project_dir_str,
        default_scene: project.default_scene,
        settings: project.settings,
        component_definitions: project.component_definitions,
        packages: project.packages,
        scene_files,
        script_files,
        sprite_files,
        prefab_files,
        gui_files,
        anim_files,
        behavior_files,
    })
}

/// Save/update the project file.
#[tauri::command]
pub fn save_project(
    path: String,
    name: String,
    default_scene: Option<String>,
    settings: ProjectSettings,
    component_definitions: Vec<ComponentDef>,
    #[allow(unused_variables)]
    packages: Option<Vec<PackageDependency>>,
) -> Result<(), String> {
    let project_file_path = PathBuf::from(&path).join("project.jengine");

    // Load existing project to preserve packages if not explicitly provided
    let existing_packages = if packages.is_none() {
        std::fs::read_to_string(&project_file_path)
            .ok()
            .and_then(|s| serde_json::from_str::<ProjectFile>(&s).ok())
            .map(|p| p.packages)
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let existing = std::fs::read_to_string(&project_file_path)
        .ok()
        .and_then(|s| serde_json::from_str::<ProjectFile>(&s).ok());
    let engine = jengine_core::version::ENGINE_VERSION;
    let engine_version = existing
        .as_ref()
        .and_then(|p| p.engine_version.as_deref())
        .map_or(engine, |v| jengine_core::version::newest(v, engine))
        .to_string();
    let server = existing.and_then(|p| p.server);
    let project = ProjectFile {
        name,
        engine_version: Some(engine_version),
        default_scene,
        settings,
        component_definitions,
        packages: packages.unwrap_or(existing_packages),
        server,
    };
    let json = serde_json::to_string_pretty(&project)
        .map_err(|e| format!("Failed to serialize project: {e}"))?;
    std::fs::write(&project_file_path, json)
        .map_err(|e| format!("Failed to write project file: {e}"))?;
    Ok(())
}

/// Rescan asset files and return updated lists for the project store.
#[derive(serde::Serialize)]
pub struct ProjectFileLists {
    pub scene_files: Vec<String>,
    pub script_files: Vec<String>,
    pub sprite_files: Vec<String>,
    pub prefab_files: Vec<String>,
    pub gui_files: Vec<String>,
    pub anim_files: Vec<String>,
    /// `.behavior` graph asset files (Step 7 / Slice 7.7).
    pub behavior_files: Vec<String>,
}

#[tauri::command]
pub fn refresh_project_files(project_path: String) -> Result<ProjectFileLists, String> {
    let assets_dir = PathBuf::from(&project_path).join("assets");
    let scene_files = list_files_with_ext(&assets_dir, &assets_dir, "scene");
    let script_files = {
        let mut files = list_files_with_ext(&assets_dir, &assets_dir, "lua");
        files.extend(list_files_with_ext(&assets_dir, &assets_dir, "js"));
        files
    };
    let sprite_files = {
        let mut files = list_files_with_ext(&assets_dir, &assets_dir, "png");
        files.extend(list_files_with_ext(&assets_dir, &assets_dir, "jpg"));
        files
    };
    let prefab_files = list_files_with_suffix(&assets_dir, &assets_dir, ".prefab");
    let gui_files = list_files_with_ext(&assets_dir, &assets_dir, "gui");
    let anim_files = list_files_with_ext(&assets_dir, &assets_dir, "anim");
    let behavior_files = list_files_with_ext(&assets_dir, &assets_dir, "behavior");
    Ok(ProjectFileLists { scene_files, script_files, sprite_files, prefab_files, gui_files, anim_files, behavior_files })
}

#[tauri::command]
pub fn list_assets(project_path: String) -> Result<Vec<String>, String> {
    let assets_dir = PathBuf::from(&project_path).join("assets");
    if !assets_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_files(&assets_dir, &assets_dir, &mut files);
    Ok(files)
}

fn list_files_with_suffix(base: &std::path::Path, dir: &std::path::Path, suffix: &str) -> Vec<String> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && name.ends_with(suffix)
                    && let Some(rel) = path.strip_prefix(base).ok().and_then(|r| r.to_str())
                {
                    result.push(rel.replace('\\', "/"));
                }
            } else if path.is_dir() {
                result.extend(list_files_with_suffix(base, &path, suffix));
            }
        }
    }
    result
}

fn list_files_with_ext(base: &std::path::Path, dir: &std::path::Path, ext: &str) -> Vec<String> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == ext) {
                if let Some(rel) = path.strip_prefix(base).ok().and_then(|r| r.to_str()) {
                    result.push(rel.replace('\\', "/"));
                }
            } else if path.is_dir() {
                result.extend(list_files_with_ext(base, &path, ext));
            }
        }
    }
    result
}

/// List all image files (png/jpg/jpeg/webp) under the project root,
/// skipping internal dirs and atlas output folders.
#[tauri::command]
pub fn list_project_images(project_path: String) -> Result<Vec<String>, String> {
    let root = PathBuf::from(&project_path);
    let assets_dir = root.join("assets");
    if !assets_dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    collect_images(&assets_dir, &assets_dir, &mut files);
    Ok(files)
}

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];
const SKIP_DIRS: &[&str] = &["target", "_atlases", ".git", "node_modules"];

fn collect_images(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let is_image = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
                    .unwrap_or(false);
                if is_image
                    && let Some(s) = path.strip_prefix(base).ok().and_then(|rel| rel.to_str())
                {
                    out.push(s.replace('\\', "/"));
                }
            } else if path.is_dir() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if !SKIP_DIRS.contains(&name_str.as_ref()) {
                    collect_images(base, &path, out);
                }
            }
        }
    }
}

fn collect_files(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(s) = path.strip_prefix(base).ok().and_then(|rel| rel.to_str()) {
                    out.push(s.replace('\\', "/"));
                }
            } else if path.is_dir() {
                collect_files(base, &path, out);
            }
        }
    }
}

/// Build a recursive tree of a project directory, skipping internal files.
fn build_asset_tree(base: &std::path::Path, dir: &std::path::Path) -> Vec<AssetNode> {
    /// Files/dirs at the project root that should be hidden from the asset browser.
    const HIDDEN: &[&str] = &["project.jengine", "editor-settings.json", "target"];

    let mut nodes = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return nodes;
    };

    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());

    let is_root = base == dir;

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden entries at the project root
        if is_root && HIDDEN.iter().any(|h| *h == name) {
            continue;
        }

        let rel = path
            .strip_prefix(base)
            .ok()
            .and_then(|r| r.to_str())
            .map(|s| s.replace('\\', "/"))
            .unwrap_or_default();

        if path.is_dir() {
            let children = build_asset_tree(base, &path);
            nodes.push(AssetNode {
                name,
                path: rel,
                is_dir: true,
                children,
            });
        } else {
            nodes.push(AssetNode {
                name,
                path: rel,
                is_dir: false,
                children: Vec::new(),
            });
        }
    }

    nodes
}

/// Return the full asset tree for the project.
///
/// Scans the `assets/` directory, but paths are relative to the project root
/// (e.g. "assets/scenes/main.scene") so they work directly with other commands.
/// The project's own Rust packages follow as one more top-level folder,
/// `packages` - see [`package_tree`].
#[tauri::command]
pub fn list_asset_tree(project_path: String) -> Result<Vec<AssetNode>, String> {
    let project_dir = PathBuf::from(&project_path);
    let assets_dir = project_dir.join("assets");
    let mut tree = if assets_dir.exists() { build_asset_tree(&project_dir, &assets_dir) } else { Vec::new() };
    if let Some(packages) = package_tree(&project_dir) {
        tree.push(packages);
    }
    Ok(tree)
}

/// The project's `packages/` folder as a tree node, or `None` without one.
///
/// A package made by *New Rust Package* lives there, outside `assets/`, and
/// the asset browser showed only `assets/` - so the code you had just made
/// could be reached only through the file system. Cargo's own output
/// (`target/`, `Cargo.lock`) and version control are left out at every depth:
/// a package's `target/` holds thousands of files.
fn package_tree(project_dir: &std::path::Path) -> Option<AssetNode> {
    const SKIPPED: &[&str] = &["target", "Cargo.lock", ".git"];

    fn walk(base: &std::path::Path, dir: &std::path::Path) -> Vec<AssetNode> {
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        entries
            .into_iter()
            .filter(|e| !SKIPPED.contains(&e.file_name().to_string_lossy().as_ref()))
            .map(|e| {
                let path = e.path();
                let rel = path
                    .strip_prefix(base)
                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                let is_dir = path.is_dir();
                AssetNode {
                    name: e.file_name().to_string_lossy().into_owned(),
                    path: rel,
                    is_dir,
                    children: if is_dir { walk(base, &path) } else { Vec::new() },
                }
            })
            .collect()
    }

    let dir = project_dir.join("packages");
    dir.is_dir().then(|| AssetNode {
        name: "packages".into(),
        path: "packages".into(),
        is_dir: true,
        children: walk(project_dir, &dir),
    })
}

/// A prefab entry with its filename and parsed entity data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefabEntry {
    /// File name without `.prefab` suffix (e.g. "Player").
    pub name: String,
    /// Relative path from assets/ (e.g. "prefabs/Player.prefab").
    pub file: String,
    /// The parsed entity data.
    pub data: serde_json::Value,
}

/// List all prefabs in the project, returning their names and parsed data.
#[tauri::command]
pub fn list_prefabs(project_path: String) -> Result<Vec<PrefabEntry>, String> {
    let assets_dir = PathBuf::from(&project_path).join("assets");
    let files = list_files_with_suffix(&assets_dir, &assets_dir, ".prefab");
    let mut entries = Vec::new();
    for file in files {
        let abs_path = assets_dir.join(file.replace('/', std::path::MAIN_SEPARATOR_STR));
        let contents = std::fs::read_to_string(&abs_path)
            .map_err(|e| format!("Failed to read prefab {file}: {e}"))?;
        let data: serde_json::Value = serde_json::from_str(&contents)
            .map_err(|e| format!("Failed to parse prefab {file}: {e}"))?;
        let name = file
            .rsplit('/')
            .next()
            .unwrap_or(&file)
            .strip_suffix(".prefab")
            .unwrap_or(&file)
            .to_string();
        entries.push(PrefabEntry { name, file, data });
    }
    Ok(entries)
}

/// Create a new folder inside the project directory.
#[tauri::command]
pub fn create_asset_folder(project_path: String, relative_path: String) -> Result<(), String> {
    let target = PathBuf::from(&project_path)
        .join(&relative_path);
    // Prevent path traversal
    if relative_path.contains("..") {
        return Err("Invalid path".to_string());
    }
    std::fs::create_dir_all(&target)
        .map_err(|e| format!("Failed to create folder: {e}"))?;
    Ok(())
}

/// Delete an asset file or folder.
#[tauri::command]
pub fn delete_asset(project_path: String, relative_path: String) -> Result<(), String> {
    if relative_path.contains("..") || relative_path.is_empty() {
        return Err("Invalid path".to_string());
    }
    let target = PathBuf::from(&project_path)
        .join(&relative_path);
    if !target.exists() {
        return Err(format!("Path does not exist: {relative_path}"));
    }
    if target.is_dir() {
        std::fs::remove_dir_all(&target)
            .map_err(|e| format!("Failed to delete folder: {e}"))?;
    } else {
        std::fs::remove_file(&target)
            .map_err(|e| format!("Failed to delete file: {e}"))?;
    }
    Ok(())
}

/// Rename/move an asset file or folder.
#[tauri::command]
pub fn rename_asset(
    project_path: String,
    old_relative_path: String,
    new_relative_path: String,
) -> Result<(), String> {
    if old_relative_path.contains("..") || new_relative_path.contains("..") {
        return Err("Invalid path".to_string());
    }
    let project_dir = PathBuf::from(&project_path);
    let old_path = project_dir.join(&old_relative_path);
    let new_path = project_dir.join(&new_relative_path);
    if !old_path.exists() {
        return Err(format!("Source does not exist: {old_relative_path}"));
    }
    if new_path.exists() {
        return Err(format!("Destination already exists: {new_relative_path}"));
    }
    // Ensure parent directory exists
    if let Some(parent) = new_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create parent directory: {e}"))?;
    }
    std::fs::rename(&old_path, &new_path)
        .map_err(|e| format!("Failed to rename: {e}"))?;
    Ok(())
}

/// Create a new file in the project directory with the given content.
#[tauri::command]
pub fn create_asset_file(
    project_path: String,
    relative_path: String,
    content: String,
) -> Result<(), String> {
    if relative_path.contains("..") {
        return Err("Invalid path".to_string());
    }
    let dest = PathBuf::from(&project_path)
        .join(&relative_path);
    if dest.exists() {
        return Err(format!("File already exists: {relative_path}"));
    }
    // Ensure parent directory exists
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {e}"))?;
    }
    std::fs::write(&dest, content)
        .map_err(|e| format!("Failed to create file: {e}"))?;
    Ok(())
}

/// Copy a file from anywhere on disk into the project's assets directory.
#[tauri::command]
pub fn import_asset(
    project_path: String,
    source_path: String,
    dest_relative_path: String,
) -> Result<(), String> {
    if dest_relative_path.contains("..") {
        return Err("Invalid destination path".to_string());
    }
    let source = PathBuf::from(&source_path);
    if !source.is_file() {
        return Err(format!("Source is not a file: {source_path}"));
    }
    let dest = PathBuf::from(&project_path)
        .join(&dest_relative_path);
    // Ensure parent directory exists
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {e}"))?;
    }
    std::fs::copy(&source, &dest)
        .map_err(|e| format!("Failed to import file: {e}"))?;
    Ok(())
}

/// Duplicate a file already inside the project (binary-safe copy).
#[tauri::command]
pub fn duplicate_asset(
    project_path: String,
    relative_path: String,
    new_relative_path: String,
) -> Result<(), String> {
    if relative_path.contains("..") || new_relative_path.contains("..") {
        return Err("Invalid path".to_string());
    }
    let project_dir = PathBuf::from(&project_path);
    let source = project_dir.join(&relative_path);
    if !source.is_file() {
        return Err(format!("Source is not a file: {relative_path}"));
    }
    let dest = project_dir.join(&new_relative_path);
    if dest.exists() {
        return Err(format!("Destination already exists: {new_relative_path}"));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {e}"))?;
    }
    std::fs::copy(&source, &dest)
        .map_err(|e| format!("Failed to duplicate file: {e}"))?;
    Ok(())
}

const MAX_RECENT: usize = 10;
const RECENT_FILE: &str = "recent-projects.json";

/// A single entry in the recent projects list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentProject {
    /// Project display name.
    pub name: String,
    /// Absolute path to the project directory.
    pub path: String,
}

/// Read the recent-projects.json from the app data directory.
fn read_recent(app: &tauri::AppHandle) -> Vec<RecentProject> {
    let Ok(data_dir) = app.path().app_data_dir() else {
        return Vec::new();
    };
    let file = data_dir.join(RECENT_FILE);
    if !file.exists() {
        return Vec::new();
    }
    let Ok(contents) = std::fs::read_to_string(&file) else {
        return Vec::new();
    };
    serde_json::from_str(&contents).unwrap_or_default()
}

/// Write the recent-projects.json to the app data directory.
fn write_recent(app: &tauri::AppHandle, list: &[RecentProject]) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {e}"))?;
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("Failed to create app data dir: {e}"))?;
    let json = serde_json::to_string_pretty(list)
        .map_err(|e| format!("Failed to serialize recent projects: {e}"))?;
    std::fs::write(data_dir.join(RECENT_FILE), json)
        .map_err(|e| format!("Failed to write recent projects: {e}"))?;
    Ok(())
}

/// Add a project to the recent list (moves to front if already present).
pub fn add_to_recent(app: &tauri::AppHandle, name: &str, path: &str) {
    let mut list = read_recent(app);
    // Remove existing entry with same path (case-insensitive on Windows)
    list.retain(|e| !paths_equal(&e.path, path));
    // Insert at front
    list.insert(
        0,
        RecentProject {
            name: name.to_string(),
            path: path.to_string(),
        },
    );
    // Trim to max
    list.truncate(MAX_RECENT);
    let _ = write_recent(app, &list);
}

/// Compare two paths for equality (case-insensitive on Windows, case-sensitive elsewhere).
fn paths_equal(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// Get the list of recent projects.
#[tauri::command]
pub fn get_recent_projects(app: tauri::AppHandle) -> Vec<RecentProject> {
    let mut list = read_recent(&app);
    // Filter out projects whose directory no longer exists
    list.retain(|e| PathBuf::from(&e.path).join("project.jengine").exists());
    list
}

/// Remove a project from the recent list by path.
#[tauri::command]
pub fn remove_recent_project(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let mut list = read_recent(&app);
    list.retain(|e| !paths_equal(&e.path, &path));
    write_recent(&app, &list)
}

/// Read a binary asset file and return its raw bytes.
/// `relative_path` is relative to the project's `assets/` directory (e.g. "sprites/player.png").
#[tauri::command]
pub fn read_asset_binary(project_path: String, relative_path: String) -> Result<Vec<u8>, String> {
    let file_path = PathBuf::from(&project_path)
        .join("assets")
        .join(&relative_path);

    // Check existence before canonicalize to give a clear error
    if !file_path.exists() {
        return Err(format!(
            "Asset not found: {relative_path} (looked at: {})",
            file_path.display()
        ));
    }

    // Prevent path traversal
    let canonical = file_path
        .canonicalize()
        .map_err(|e| format!("Asset not found: {e}"))?;
    let assets_dir = PathBuf::from(&project_path)
        .join("assets")
        .canonicalize()
        .map_err(|e| format!("Assets directory not found: {e}"))?;
    if !canonical.starts_with(&assets_dir) {
        return Err("Path traversal not allowed".to_string());
    }

    std::fs::read(&canonical).map_err(|e| format!("Failed to read asset: {e}"))
}

/// Open a file in the user's preferred code editor.
///
/// If `editor_cmd` is empty, tries common editors in order: code (VS Code), cursor, codium.
/// Opens the project folder and navigates to the specific file.
/// `relative_path` is relative to the project root (e.g. "assets/scripts/player.lua").
#[tauri::command]
pub fn open_in_code_editor(
    project_path: String,
    relative_path: String,
    editor_cmd: String,
) -> Result<(), String> {
    if relative_path.contains("..") {
        return Err("Invalid path".to_string());
    }

    let file_path = PathBuf::from(&project_path)
        .join(&relative_path);

    if !file_path.exists() {
        return Err(format!("File not found: {relative_path}"));
    }

    let candidates: Vec<&str> = if editor_cmd.is_empty() {
        vec!["code", "cursor", "codium"]
    } else {
        vec![editor_cmd.as_str()]
    };

    for cmd in &candidates {
        let Some(program) = find_program(cmd) else { continue };
        let mut command = std::process::Command::new(&program);
        command.arg(&project_path).arg("--goto").arg(&file_path);
        // `code.cmd` is a batch script: without this a console window flashes up.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        if command.spawn().is_ok() {
            return Ok(());
        }
    }

    Err(format!(
        "Could not launch code editor. Tried: {}. Install VS Code or set a custom editor in Project Settings.",
        candidates.join(", ")
    ))
}

/// The program `cmd` names, found as a shell would find it.
///
/// `Command::new("code")` on Windows looks only for `code.exe`, and VS Code,
/// Cursor and VSCodium put a `code.cmd` / `cursor.cmd` script on the PATH - so
/// "Open in code editor" failed for every one of them and reported that none
/// was installed. This walks the PATH with each `PATHEXT` extension, then the
/// places those editors install to when the PATH lacks them (the editor may
/// have been started before the installer added it). A path is taken as given.
fn find_program(cmd: &str) -> Option<PathBuf> {
    let given = PathBuf::from(cmd);
    if given.components().count() > 1 || given.is_absolute() {
        return given.is_file().then_some(given);
    }
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
        let known: &[(&Option<PathBuf>, &str)] = &[
            (&local, "Programs/Microsoft VS Code/bin"),
            (&program_files, "Microsoft VS Code/bin"),
            (&local, "Programs/cursor/resources/app/bin"),
            (&local, "Programs/VSCodium/bin"),
            (&program_files, "VSCodium/bin"),
        ];
        dirs.extend(known.iter().filter_map(|(root, rel)| root.as_ref().map(|r| r.join(rel))));
    }
    for dir in dirs {
        for ext in &exts {
            let candidate = dir.join(format!("{cmd}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Reveal a file or folder in the system file explorer.
///
/// For files, opens the parent folder with the file selected.
/// For folders, opens the folder directly.
#[tauri::command]
pub fn show_in_file_manager(project_path: String, relative_path: String) -> Result<(), String> {
    if relative_path.contains("..") {
        return Err("Invalid path".to_string());
    }
    let target = PathBuf::from(&project_path).join(&relative_path);
    if !target.exists() {
        return Err(format!("Path does not exist: {relative_path}"));
    }

    #[cfg(target_os = "windows")]
    {
        // Canonicalize to get an absolute path with backslashes — explorer requires this.
        let canonical = target
            .canonicalize()
            .map_err(|e| format!("Failed to resolve path: {e}"))?;
        if canonical.is_file() {
            // explorer.exe /select, requires the whole thing as a single raw command
            std::process::Command::new("cmd")
                .args(["/C", &format!("explorer /select,\"{}\"", canonical.display())])
                .spawn()
                .map_err(|e| format!("Failed to open Explorer: {e}"))?;
        } else {
            std::process::Command::new("explorer")
                .arg(&canonical)
                .spawn()
                .map_err(|e| format!("Failed to open Explorer: {e}"))?;
        }
    }

    #[cfg(target_os = "macos")]
    {
        if target.is_file() {
            std::process::Command::new("open")
                .arg("-R")
                .arg(&target)
                .spawn()
                .map_err(|e| format!("Failed to open Finder: {e}"))?;
        } else {
            std::process::Command::new("open")
                .arg(&target)
                .spawn()
                .map_err(|e| format!("Failed to open Finder: {e}"))?;
        }
    }

    #[cfg(target_os = "linux")]
    {
        let dir = if target.is_file() {
            target.parent().unwrap_or(&target).to_path_buf()
        } else {
            target
        };
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("Failed to open file manager: {e}"))?;
    }

    Ok(())
}
