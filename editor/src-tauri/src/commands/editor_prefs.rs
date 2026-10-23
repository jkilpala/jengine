//! Preferences of the editor itself - not of a project, so they live in the
//! app data directory beside the recent-projects list, not in any
//! `editor-settings.json`.
//!
//! **Which GPU the editor draws on** is the first. The editor is a WebView2
//! page, and the browser engine - not wgpu's `HighPerformance` request -
//! decides which of a laptop's two GPUs it runs on. Chromium's
//! `--force_high_performance_gpu` makes it the discrete one, and it was
//! fixed in `tauri.conf.json`'s browser arguments, so there was no way to
//! leave the discrete GPU asleep on battery. The main window is now created
//! in `setup` (`create: false` in the config) so the argument can come from
//! here. Browser arguments are fixed when the WebView2 environment starts,
//! so a change applies from the next launch.

use serde::{Deserialize, Serialize};
use tauri::Manager;

const PREFS_FILE: &str = "editor-preferences.json";

/// The Chromium switch that puts the editor on the discrete GPU.
pub const HIGH_PERFORMANCE_GPU_ARG: &str = "--force_high_performance_gpu";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorGpu {
    /// The discrete GPU (`--force_high_performance_gpu`), as before.
    #[default]
    HighPerformance,
    /// Whatever Windows picks for the WebView2 process - usually the
    /// integrated GPU, or what Settings > Display > Graphics says.
    System,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorPrefs {
    pub gpu: EditorGpu,
    /// How hard the editor viewport's WebAssembly is optimised when it is
    /// built - `jengine_builder::wasm_opt`, which the CLI and `run-editor`
    /// read from this same file. `fast` builds in seconds what `optimized`
    /// takes minutes over.
    pub viewport_wasm_opt: jengine_builder::wasm_opt::WasmOpt,
}

/// The browser arguments for the editor's window: the config's own, plus
/// the GPU switch when the preference asks for it.
pub fn browser_args(base: Option<&str>, prefs: &EditorPrefs) -> Option<String> {
    let mut args: Vec<&str> = base
        .unwrap_or("")
        .split_whitespace()
        .filter(|a| *a != HIGH_PERFORMANCE_GPU_ARG)
        .collect();
    if prefs.gpu == EditorGpu::HighPerformance {
        args.push(HIGH_PERFORMANCE_GPU_ARG);
    }
    (!args.is_empty()).then(|| args.join(" "))
}

pub fn load(app: &tauri::AppHandle) -> EditorPrefs {
    app.path()
        .app_data_dir()
        .ok()
        .and_then(|dir| std::fs::read_to_string(dir.join(PREFS_FILE)).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(app: &tauri::AppHandle, prefs: &EditorPrefs) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create app data dir: {e}"))?;
    let json = serde_json::to_string_pretty(prefs).map_err(|e| format!("Failed to serialize preferences: {e}"))?;
    std::fs::write(dir.join(PREFS_FILE), json).map_err(|e| format!("Failed to write preferences: {e}"))
}

/// Create the main window from the config's first window, with the browser
/// arguments the preferences ask for.
pub fn create_main_window(app: &tauri::App) -> tauri::Result<()> {
    let Some(mut config) = app.config().app.windows.first().cloned() else {
        return Ok(());
    };
    config.create = true;
    // The editor draws its own title bar: the toolbar is it, with the
    // window's buttons at its right end (`WindowControls.svelte`) and its
    // empty space a drag region. On macOS the native traffic lights stay,
    // overlaid on the toolbar's left, which is where a Mac user looks for them.
    if cfg!(target_os = "macos") {
        config.title_bar_style = tauri::TitleBarStyle::Overlay;
        config.hidden_title = true;
    } else {
        config.decorations = false;
    }
    config.additional_browser_args = browser_args(config.additional_browser_args.as_deref(), &load(app.handle()));
    log::info!("editor window browser args: {:?}", config.additional_browser_args);
    tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?.build()?;
    Ok(())
}

#[tauri::command]
pub fn get_editor_prefs(app: tauri::AppHandle) -> EditorPrefs {
    load(&app)
}

#[tauri::command]
pub fn set_editor_prefs(app: tauri::AppHandle, prefs: EditorPrefs) -> Result<(), String> {
    save(&app, &prefs)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

    #[test]
    fn the_default_keeps_the_editor_on_the_discrete_gpu() {
        let args = browser_args(Some(BASE), &EditorPrefs::default()).unwrap();
        assert_eq!(args, format!("{BASE} {HIGH_PERFORMANCE_GPU_ARG}"));
    }

    #[test]
    fn letting_windows_choose_drops_only_the_gpu_switch() {
        let prefs = EditorPrefs { gpu: EditorGpu::System, ..Default::default() };
        let with_switch = format!("{BASE} {HIGH_PERFORMANCE_GPU_ARG}");
        assert_eq!(browser_args(Some(&with_switch), &prefs).as_deref(), Some(BASE));
        assert_eq!(browser_args(None, &prefs), None);
    }

    #[test]
    fn the_switch_is_never_doubled() {
        let with_switch = format!("{BASE} {HIGH_PERFORMANCE_GPU_ARG}");
        let args = browser_args(Some(&with_switch), &EditorPrefs::default()).unwrap();
        assert_eq!(args.matches(HIGH_PERFORMANCE_GPU_ARG).count(), 1);
    }

    #[test]
    fn the_config_window_is_created_in_setup() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let window = &conf["app"]["windows"][0];
        assert_eq!(window["create"], false, "Tauri must not create the window before the preferences are read");
        let args = window["additionalBrowserArgs"].as_str().unwrap_or("");
        assert!(!args.contains(HIGH_PERFORMANCE_GPU_ARG), "the GPU switch comes from the preferences, not the config");
    }
}
