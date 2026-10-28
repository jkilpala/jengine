//! Build pipeline Tauri commands.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::Emitter;

/// Event name the Build panel listens on. One payload per output line.
const BUILD_OUTPUT_EVENT: &str = "build-output";

/// One line of build output. `replaces_previous` is set for a progress-bar
/// redraw (a bare carriage return), so the panel updates the last line in
/// place instead of scrolling hundreds of near-identical ones.
#[derive(Clone, serde::Serialize)]
struct BuildLine<'a> {
    text: &'a str,
    replaces_previous: bool,
}

/// Forward build output to the frontend for as long as this is alive.
///
/// The builder's sink is process-wide, so it has to be cleared when the
/// build ends - including when it ends by returning an error, which is what
/// the `Drop` is for.
struct BuildStream;

impl Drop for BuildStream {
    fn drop(&mut self) {
        jengine_builder::progress::set_sink(None);
    }
}

/// Start streaming `cargo` / `wasm-pack` output into the editor's Build
/// panel. Keep the returned guard alive for the duration of the build.
fn stream_output(app: &tauri::AppHandle) -> BuildStream {
    let handle = app.clone();
    jengine_builder::progress::set_sink(Some(Arc::new(
        move |line: &str, replaces_previous: bool| {
            // A dropped event only costs a line in the panel - never fail a
            // build because the window went away mid-compile.
            let _ = handle.emit(
                BUILD_OUTPUT_EVENT,
                BuildLine {
                    text: line,
                    replaces_previous,
                },
            );
        },
    )));
    BuildStream
}

/// The engine checkout builds compile against - `JENGINE_HOME`, else the
/// nearest checkout above the working directory or the editor itself
/// (`jengine_builder::engine_root`, shared with the `jengine` CLI). It used to
/// walk up from the executable only, so an installed editor could not build.
pub(crate) fn engine_root() -> Result<PathBuf, String> {
    jengine_builder::engine_root::locate()
}

/// `async` is load-bearing: Tauri runs a **synchronous** command on the main
/// thread, and on Windows that is the thread pumping WebView2 messages. A
/// build takes minutes, so a sync command froze the whole editor for its
/// duration and delivered no `build-output` event until it finished — the
/// panel showed the command line and then appeared to hang.
#[tauri::command(async)]
pub fn build_windows(app: tauri::AppHandle, project_path: String, output_path: String, release: bool, generate_report: bool, enable_profiling: bool, debug_overlay: bool) -> Result<String, String> {
    let _stream = stream_output(&app);
    let engine = engine_root()?;
    let project = PathBuf::from(&project_path);
    let output = PathBuf::from(&output_path);

    jengine_builder::windows::build_windows(&engine, &project, &output, release, generate_report, enable_profiling, debug_overlay)
        .map_err(|e| {
            let msg = format!("Windows build failed: {e}");
            jengine_builder::progress::emit(&msg);
            msg
        })?;

    Ok(output.display().to_string())
}

/// `async` for the same reason as `build_windows`: a synchronous command
/// blocks the WebView2 message pump for the length of the build.
/// A Linux folder (built inside WSL from Windows) or a macOS `.app` (on a
/// Mac only) - `jengine_builder::desktop`. `platform` is `linux` or `macos`.
#[tauri::command(async)]
pub fn build_desktop(
    app: tauri::AppHandle,
    platform: String,
    project_path: String,
    output_path: String,
    release: bool,
    generate_report: bool,
    debug_overlay: bool,
) -> Result<String, String> {
    let _stream = stream_output(&app);
    let engine = engine_root()?;
    let project = PathBuf::from(&project_path);
    let output = PathBuf::from(&output_path);
    let built = match platform.as_str() {
        "linux" => jengine_builder::desktop::build_linux(&engine, &project, &output, release, generate_report, debug_overlay),
        "macos" => jengine_builder::desktop::build_macos(&engine, &project, &output, release, generate_report, debug_overlay),
        other => return Err(format!("no desktop platform '{other}'")),
    };
    built.map(|p| p.display().to_string()).map_err(|e| {
        let msg = format!("{platform} build failed: {e:#}");
        jengine_builder::progress::emit(&msg);
        msg
    })
}

/// An iOS `.app` (on a Mac only - `jengine_builder::ios`): signed ad hoc
/// for the simulator, or with a development identity and profile for a
/// device; `install` / `launch` put it on the booted simulator or the first
/// connected device.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub fn build_ios(
    app: tauri::AppHandle,
    project_path: String,
    output_path: String,
    release: bool,
    debug_overlay: bool,
    device: bool,
    install: bool,
    launch: bool,
    identity: Option<String>,
    profile: Option<String>,
) -> Result<String, String> {
    use jengine_builder::ios::{IosOptions, IosTarget};
    let _stream = stream_output(&app);
    let engine = engine_root()?;
    let opts = IosOptions {
        target: if device { IosTarget::Device } else { IosTarget::Simulator },
        release,
        debug_overlay,
        install,
        launch,
        device: None,
        identity: identity.filter(|s| !s.trim().is_empty()),
        profile: profile.filter(|s| !s.trim().is_empty()).map(PathBuf::from),
    };
    jengine_builder::ios::build_ios(&engine, &PathBuf::from(&project_path), &PathBuf::from(&output_path), &opts)
        .map(|p| p.display().to_string())
        .map_err(|e| {
            let msg = format!("iOS build failed: {e:#}");
            jengine_builder::progress::emit(&msg);
            msg
        })
}

#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub fn build_web(app: tauri::AppHandle, project_path: String, output_path: String, release: bool, generate_report: bool, wasm_lto: bool, wasm_opt_size: bool, wasm_single_codegen: bool, wasm_opt: Option<String>, debug_overlay: bool, logging: Option<bool>, platform: Option<String>) -> Result<String, String> {
    let _stream = stream_output(&app);
    let engine = engine_root()?;
    let project = PathBuf::from(&project_path);
    let output = PathBuf::from(&output_path);
    // The web portal for this build; none given (or "project") keeps the
    // project's `settings.web_platform`.
    let platform = match platform.as_deref().map(str::trim) {
        None | Some("") | Some("project") => None,
        Some(id) => Some(jengine_builder::web_platform::WebPlatform::parse(id).ok_or_else(|| {
            format!("Unknown web portal \"{id}\" (expected one of: {})", jengine_builder::web_platform::known_ids())
        })?),
    };

    // The project's wasm-opt level (Build Settings); none given is a web
    // build's default (`fast`), not the viewport's.
    let wasm_opt = match wasm_opt.as_deref().map(str::trim) {
        None | Some("") => jengine_builder::wasm_opt::WasmOpt::WEB_BUILD_DEFAULT,
        Some(id) => jengine_builder::wasm_opt::WasmOpt::parse(id)
            .ok_or_else(|| format!("Unknown wasm-opt level \"{id}\" (off, fast, optimized, size, max)"))?,
    };

    jengine_builder::web::build_web(&engine, &project, &output, release, generate_report, wasm_lto, wasm_opt_size, wasm_single_codegen, wasm_opt, debug_overlay, logging.unwrap_or(true), platform)
        .map_err(|e| {
            let msg = format!("Web build failed: {e}");
            jengine_builder::progress::emit(&msg);
            msg
        })?;

    Ok(output.display().to_string())
}

/// An Android APK - and with `bundle` the `.aab` Google Play takes - signed
/// with the debug key, or with `release_key` the project's own (its password
/// passed in, used for this build and not kept). Optionally installed onto the
/// connected device or emulator with `adb` and started. Returns the bundle's
/// path when one was made, else the APK's.
///
/// `async` for the same reason as `build_windows`.
#[tauri::command(async)]
pub fn build_android(
    app: tauri::AppHandle,
    project_path: String,
    output_path: String,
    release: bool,
    debug_overlay: bool,
    install: bool,
    launch: bool,
    bundle: bool,
    release_key: bool,
    keystore_password: Option<String>,
) -> Result<String, String> {
    let _stream = stream_output(&app);
    let engine = engine_root()?;
    let project = PathBuf::from(&project_path);
    let output = PathBuf::from(&output_path);
    let opts = jengine_builder::android::AndroidBuildOptions {
        release,
        debug_overlay,
        abis: None,
        install,
        launch,
        bundle,
        release_key,
        // Held for this build only - the editor never stores it.
        keystore_password,
        key_password: None,
    };
    let out = jengine_builder::android::build_android(&engine, &project, &output, &opts)
        .map_err(|e| {
            let msg = format!("Android build failed: {e:#}");
            jengine_builder::progress::emit(&msg);
            msg
        })?;
    Ok(jengine_builder::packages::display_path(out.bundle.as_ref().unwrap_or(&out.apk)))
}

#[tauri::command]
pub fn open_build_report(report_path: String) -> Result<(), String> {
    let path = PathBuf::from(&report_path);
    if !path.exists() {
        return Err(format!("Report not found: {report_path}"));
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &report_path])
            .spawn()
            .map_err(|e| format!("Failed to open report: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&report_path)
            .spawn()
            .map_err(|e| format!("Failed to open report: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&report_path)
            .spawn()
            .map_err(|e| format!("Failed to open report: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
pub fn open_build_folder(folder_path: String) -> Result<(), String> {
    let path = std::path::PathBuf::from(&folder_path);
    if !path.exists() {
        return Err(format!("Folder not found: {folder_path}"));
    }
    #[cfg(target_os = "windows")]
    {
        // explorer.exe requires backslashes — forward slashes from Tauri's dialog
        // cause it to silently open Documents instead of the target folder.
        let native_path = folder_path.replace('/', "\\");
        std::process::Command::new("explorer")
            .arg(&native_path)
            .spawn()
            .map_err(|e| format!("Failed to open Explorer: {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open Finder: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open file manager: {e}"))?;
    }
    Ok(())
}

// ── Editor viewport packages ───────────────────────────────────────────────

/// Whether the loaded viewport module carries this project's Rust packages.
///
/// The viewport is one prebuilt WASM module shared by every project, so it
/// carries whichever project it was last built for. Without this check a
/// package simply does nothing in the editor and there is no signal saying
/// why — which makes package development guesswork.
#[tauri::command]
pub fn viewport_package_status(
    project_path: String,
) -> Result<jengine_builder::viewport::ViewportStatus, String> {
    let engine = engine_root()?;
    Ok(jengine_builder::viewport::status(
        &engine,
        &PathBuf::from(&project_path),
    ))
}

/// Load this project's own cached viewport build, if nothing it was compiled
/// from has changed since. `false` means there is none to load and a rebuild
/// is needed; on `true` the editor must reload to pick the module up.
///
/// A copy of a few megabytes, but `async` so it never runs on the WebView2
/// message-pump thread.
#[tauri::command(async)]
pub fn switch_project_viewport(project_path: String) -> Result<bool, String> {
    let engine = engine_root()?;
    jengine_builder::viewport::switch(&engine, &PathBuf::from(&project_path))
        .map_err(|e| format!("Loading the cached viewport failed: {e:#}"))
}

/// Rebuild the editor viewport with this project's Rust packages linked in.
///
/// Streams the compile into the Build panel like any other build. The editor
/// must be reloaded afterwards: the old module is already instantiated in the
/// page.
///
/// `async` for the same reason as `build_windows`: a wasm-pack compile takes
/// minutes, and a synchronous command runs on the WebView2 message-pump
/// thread.
#[tauri::command(async)]
pub fn rebuild_project_viewport(
    app: tauri::AppHandle,
    project_path: String,
) -> Result<Vec<String>, String> {
    let _stream = stream_output(&app);
    let engine = engine_root()?;
    // The editor preference, read through the editor's own app-data path.
    let level = super::editor_prefs::load(&app).viewport_wasm_opt;
    jengine_builder::viewport::build_with(&engine, &PathBuf::from(&project_path), level).map_err(|e| {
        let msg = format!("Viewport rebuild failed: {e:#}");
        jengine_builder::progress::emit(&msg);
        msg
    })
}

// ── Build & Run ─────────────────────────────────────────────────────────────

/// The copies of the game the last Build & Run started.
static INSTANCES: std::sync::LazyLock<std::sync::Mutex<Vec<jengine_builder::native_run::Instance>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Vec::new()));

fn stop_instances() -> usize {
    let mut list = INSTANCES.lock().unwrap();
    let mut stopped = 0;
    for inst in list.iter_mut() {
        if inst.running() {
            let _ = inst.child.kill();
            let _ = inst.child.wait();
            stopped += 1;
        }
    }
    list.clear();
    stopped
}

/// Compile the project's native runtime (release, its output in the Build
/// panel) and start `instances` copies of it from the project folder - each
/// with its own saves, side by side on the screen
/// (`jengine_builder::native_run`). Copies a previous run left going are
/// closed first, as Defold's relaunch does. `scene` is a path under
/// `assets/`; without one the project's start scene runs.
///
/// `async` for the reason `build_windows` gives.
#[tauri::command(async)]
pub fn build_and_run_native(app: tauri::AppHandle, project_path: String, scene: Option<String>, instances: u32) -> Result<String, String> {
    let _stream = stream_output(&app);
    let engine = engine_root()?;
    let project = PathBuf::from(&project_path);
    let closed = stop_instances();
    if closed > 0 {
        jengine_builder::progress::emit(&format!("Closed {closed} running instance(s)"));
    }
    let started = jengine_builder::native_run::build_and_run(&engine, &project, scene.as_deref(), instances)
        .map_err(|e| {
            let msg = format!("Build & Run failed: {e:#}");
            jengine_builder::progress::emit(&msg);
            msg
        })?;
    for inst in &started {
        jengine_builder::progress::emit(&format!("Instance {} running - its log: {}", inst.n, inst.log.display()));
    }
    let n = started.len();
    *INSTANCES.lock().unwrap() = started;
    Ok(format!("{n} instance(s) running"))
}

/// Close every copy the last Build & Run started.
#[tauri::command]
pub fn stop_native_instances() -> usize {
    stop_instances()
}

/// How many of them are still running.
#[tauri::command]
pub fn native_instances_running() -> usize {
    INSTANCES.lock().unwrap().iter_mut().map(|i| i.running()).filter(|r| *r).count()
}

/// Called as the editor closes: the copies it started do not outlive it.
pub fn shutdown() {
    stop_instances();
}
