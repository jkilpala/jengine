//! Editor settings — persistent per-project editor preferences (editor-settings.json).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Editor-specific settings stored in `editor-settings.json` next to `project.jengine`.
/// These are UI/workflow preferences, not game-runtime data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorSettings {
    /// Shared build output base directory.
    /// Actual output is placed at `<build_output_dir>/<platform>/<profile>/`.
    #[serde(default)]
    pub build_output_dir: String,

    // Legacy per-platform fields — kept for deserialization compat, not written back.
    #[serde(default, skip_serializing)]
    #[allow(dead_code)]
    pub build_output_dir_windows: String,
    #[serde(default, skip_serializing)]
    #[allow(dead_code)]
    pub build_output_dir_web: String,

    /// Last selected build profile ("debug" or "release").
    #[serde(default = "default_profile")]
    pub build_profile: String,

    /// Last selected build target ("windows" or "web").
    #[serde(default = "default_target")]
    pub build_target: String,

    /// Whether to generate an HTML build report after builds.
    #[serde(default = "default_true")]
    pub build_generate_report: bool,

    /// Preferred code editor command override (empty = use project setting).
    #[serde(default)]
    pub code_editor: String,

    /// Whether to show collider overlays in the viewport.
    #[serde(default)]
    pub show_colliders: bool,

    /// Whether to show the profiler overlay in the viewport.
    #[serde(default)]
    pub show_profiler: bool,

    // The three WASM size options default OFF (Oct 2026; they were on). They
    // were turned on for size alone - a mid-sized game 1.56 -> 1.16 MB
    // gzipped - and `opt-level = 'z'` makes the engine's code ~3.7x slower
    // on the web (the frame of `jengine-benchmark-cpu`, 1.9 -> 7.2 ms);
    // `'s'` 1.4x. Fat LTO saved nothing measurable on its own. So a game
    // that wants the smaller module asks for it, knowing the price
    // (docs/build-sizes.md). A settings file that spells out `true` keeps it.
    /// WASM: enable link-time optimization (lto = true).
    #[serde(default)]
    pub wasm_lto: bool,

    /// WASM: optimize for size (opt-level = 'z') instead of speed.
    #[serde(default)]
    pub wasm_opt_size: bool,

    /// WASM: use single codegen unit (codegen-units = 1) for better optimization.
    #[serde(default)]
    pub wasm_single_codegen: bool,

    /// WASM: the `wasm-opt` pass after a release web build
    /// (`jengine_builder::wasm_opt`) - this project's own, apart from the
    /// editor viewport's preference. `fast` unless set: the smallest download
    /// (`WasmOpt::WEB_BUILD_DEFAULT`).
    #[serde(default = "default_web_wasm_opt")]
    pub wasm_opt_level: jengine_builder::wasm_opt::WasmOpt,

    /// Windows debug builds only: emit a `run-profiled.cmd` launcher next to
    /// the produced `.exe` that sets `JENGINE_SCRIPT_PROFILE` + `RUST_LOG=info`
    /// before running the game. Used to capture per-phase script-system
    /// timings (`[Script][PROF]` log lines) without rebuilding the runtime.
    #[serde(default)]
    pub build_enable_profiling: bool,

    /// Ship the F3 stats HUD / F12 inspector in the built game. On by
    /// default - they cost nothing until a key is pressed - but a public
    /// build can turn them off so players cannot open the dev overlays.
    #[serde(default = "default_true")]
    pub build_debug_overlay: bool,

    /// Web builds: whether the game writes its info lines - the engine's and
    /// the scripts' `log` / `print` - to the browser console. On by default,
    /// as every build before the option did; Build Settings warns before a
    /// release build that keeps them.
    #[serde(default = "default_true")]
    pub build_logging: bool,
}

fn default_profile() -> String {
    "release".to_string()
}
fn default_target() -> String {
    "windows".to_string()
}
fn default_true() -> bool {
    true
}
fn default_web_wasm_opt() -> jengine_builder::wasm_opt::WasmOpt {
    jengine_builder::wasm_opt::WasmOpt::WEB_BUILD_DEFAULT
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            build_output_dir: String::new(),
            build_output_dir_windows: String::new(),
            build_output_dir_web: String::new(),
            build_profile: default_profile(),
            build_target: default_target(),
            build_generate_report: true,
            code_editor: String::new(),
            show_colliders: false,
            show_profiler: false,
            wasm_lto: false,
            wasm_opt_size: false,
            wasm_single_codegen: false,
            wasm_opt_level: default_web_wasm_opt(),
            build_enable_profiling: false,
            build_debug_overlay: true,
            build_logging: true,
        }
    }
}

fn settings_path(project_path: &str) -> PathBuf {
    PathBuf::from(project_path).join("editor-settings.json")
}

#[tauri::command]
pub fn load_editor_settings(project_path: String) -> Result<EditorSettings, String> {
    let path = settings_path(&project_path);
    if !path.exists() {
        return Ok(EditorSettings::default());
    }
    let contents =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read editor settings: {e}"))?;
    let settings: EditorSettings =
        serde_json::from_str(&contents).map_err(|e| format!("Invalid editor settings: {e}"))?;
    Ok(settings)
}

#[tauri::command]
pub fn save_editor_settings(project_path: String, settings: EditorSettings) -> Result<(), String> {
    let path = settings_path(&project_path);
    let json = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize editor settings: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write editor settings: {e}"))?;
    Ok(())
}
