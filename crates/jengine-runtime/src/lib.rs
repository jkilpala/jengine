//! jEngine Native Runtime — opens a window and runs the game loop on wgpu.
//!
//! This is a **library** with a thin `main.rs` beside it. The split exists so a
//! project can ship its own Rust packages: `jengine-builder` generates a small
//! per-project crate that depends on this one and calls [`run_with`], adding
//! the project's packages in the hook. Before the split, a shipped game *was*
//! this crate, so a third-party package meant editing the engine workspace and
//! two projects could not be built without clobbering each other.
//!
//! Phase 8.1 (2026-05-06): the legacy glow/glutin path was removed. The
//! runtime is now wgpu-only — there is a single `RuntimeMode` (wgpu) and
//! `--gpu-backend` / `--wgpu-smoke` / `--wgpu-ecs` / `--wgpu-scene` /
//! `--bench` flags no longer exist. Use the standalone
//! `jengine-benchmark` crate for perf work.

/// Native gamepad polling (gilrs), feeding the same InputState the
/// browser backend does.
mod gamepad_native;
/// Game controllers on Android, which gilrs cannot read (a winit input hook).
mod gamepad_android;
mod hot_reload;
mod instance_tile;
/// `Display.*` / `Cursor.*` applied to the window (`jengine_core::display`).
mod display_native;
/// Text entry: the IME, the soft keyboard and the clipboard (`jengine_core::input::text_entry`).
mod text_native;
/// The UI handed to the platform's screen reader (`jengine_core::ui::accessibility`).
#[cfg(feature = "accessibility")]
mod a11y_native;
/// Scenes loaded in the background and laid into the running one (`jengine_core::scene::streaming`).
mod streaming_native;
mod shot_env;
mod wgpu_scene_runner;
#[cfg(all(windows, feature = "video"))]
mod video_mf;

/// The Android entry: what a generated project crate's `android_main` calls.
#[cfg(target_os = "android")]
pub mod android;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

use jengine_core::input::{InputAction, KeyCode};
use jengine_core::scripting::bridge::{BudgetConfig, BudgetPolicy};

/// Runtime project settings loaded from project.jengine.
#[derive(Debug, Clone, serde::Deserialize)]
struct RuntimeSettings {
    #[allow(dead_code)] // Reserved for future window sizing override.
    #[serde(default = "default_width")]
    width: u32,
    #[allow(dead_code)]
    #[serde(default = "default_height")]
    height: u32,
    #[serde(default = "default_bg_color")]
    background_color: [f32; 4],
    #[allow(dead_code)]
    #[serde(default = "default_title")]
    title: String,
    #[serde(default)]
    pub entry_scene: Option<String>,
    #[allow(dead_code)] // wgpu_scene_runner loads input_actions itself.
    #[serde(default)]
    input_actions: Vec<InputAction>,
    /// Step 3.3 — per-frame script execution budget. `None` (or
    /// missing block) leaves the bridge at its compile-time default
    /// (unlimited + Warn). Hosts opt in by adding a `script_budget`
    /// section to `project.jengine`'s `settings` table:
    ///
    /// ```jsonc
    /// "script_budget": {
    ///     "max_script_ms_per_frame": 4.0,
    ///     "max_invocations_per_frame": 2000,
    ///     "overflow_policy": "drop", // "warn" | "drop" | "defer"
    ///     "deferred_cap": 256
    /// }
    /// ```
    #[serde(default)]
    script_budget: Option<ScriptBudgetSettings>,
}

/// Project-file shape for the script budget. All fields default to
/// the runner's own defaults (zero ⇒ "unlimited" for the cap fields,
/// `Warn` for the policy). Resolved into a
/// [`jengine_core::scripting::bridge::BudgetConfig`] by
/// [`resolve_script_budget`].
#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
struct ScriptBudgetSettings {
    #[serde(default)]
    max_script_ms_per_frame: f32,
    #[serde(default)]
    max_invocations_per_frame: u32,
    #[serde(default)]
    overflow_policy: BudgetPolicySetting,
    #[serde(default)]
    deferred_cap: u32,
}

#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum BudgetPolicySetting {
    #[default]
    Warn,
    Drop,
    Defer,
}

fn default_width() -> u32 { 1280 }
fn default_height() -> u32 { 720 }
fn default_bg_color() -> [f32; 4] { [0.1, 0.1, 0.12, 1.0] }
fn default_title() -> String { "jEngine Game".to_string() }

/// Minimal project file reader — extracts settings and top-level default_scene.
#[derive(serde::Deserialize)]
struct ProjectFilePartial {
    #[serde(default)]
    engine_version: Option<String>,
    #[serde(default)]
    default_scene: Option<String>,
    #[serde(default)]
    settings: RuntimeSettings,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            width: default_width(),
            height: default_height(),
            background_color: default_bg_color(),
            title: default_title(),
            entry_scene: None,
            input_actions: Vec::new(),
            script_budget: None,
        }
    }
}

/// Resolve the active script-execution budget. Priority (highest first):
///
/// 1. Environment overrides — useful for ad-hoc experiments without
///    editing `project.jengine`:
///      * `JENGINE_SCRIPT_MAX_MS` — float, ms per frame; `0` = unlimited.
///      * `JENGINE_SCRIPT_MAX_INVOCATIONS` — u32; `0` = unlimited.
///      * `JENGINE_SCRIPT_OVERFLOW_POLICY` — `warn` | `drop` | `defer`.
///      * `JENGINE_SCRIPT_DEFERRED_CAP` — u32, queue depth for `defer`.
/// 2. The `settings.script_budget` block in `project.jengine`.
/// 3. The runner's compile-time defaults (unlimited + Warn).
///
/// Returns `Some` when *any* override / project setting was supplied
/// and `None` when nothing customises the budget.
fn resolve_script_budget(settings: Option<ScriptBudgetSettings>) -> Option<BudgetConfig> {
    let mut from_project = settings.is_some();
    let project = settings.unwrap_or_default();

    let env_ms = std::env::var("JENGINE_SCRIPT_MAX_MS")
        .ok()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .filter(|v| *v >= 0.0);
    let env_inv = std::env::var("JENGINE_SCRIPT_MAX_INVOCATIONS")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    let env_policy = std::env::var("JENGINE_SCRIPT_OVERFLOW_POLICY")
        .ok()
        .and_then(|s| match s.trim().to_ascii_lowercase().as_str() {
            "warn" => Some(BudgetPolicy::Warn),
            "drop" => Some(BudgetPolicy::Drop),
            "defer" => Some(BudgetPolicy::Defer),
            _ => {
                log::warn!(
                    "[Script] JENGINE_SCRIPT_OVERFLOW_POLICY='{}' is not warn|drop|defer; ignoring",
                    s
                );
                None
            }
        });
    let env_deferred = std::env::var("JENGINE_SCRIPT_DEFERRED_CAP")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());

    let any_env =
        env_ms.is_some() || env_inv.is_some() || env_policy.is_some() || env_deferred.is_some();
    if any_env {
        from_project = true;
    }

    if !from_project {
        return None;
    }

    let policy = env_policy.unwrap_or(match project.overflow_policy {
        BudgetPolicySetting::Warn => BudgetPolicy::Warn,
        BudgetPolicySetting::Drop => BudgetPolicy::Drop,
        BudgetPolicySetting::Defer => BudgetPolicy::Defer,
    });
    Some(BudgetConfig {
        max_script_ms_per_frame: env_ms.unwrap_or(project.max_script_ms_per_frame),
        max_invocations_per_frame: env_inv.unwrap_or(project.max_invocations_per_frame),
        overflow_policy: policy,
        deferred_cap: env_deferred.unwrap_or(project.deferred_cap),
    })
}

/// Where a game's files are: the first of the working directory, the
/// executable's folder, and - inside a macOS `.app` - its `Resources`
/// folder (`Contents/MacOS/../Resources`) that holds a `project.jengine`.
/// A game started from a file manager (Linux) or the Finder (macOS) does not
/// start in its own folder, as one double-clicked on Windows does; `None`
/// when none of them is a game (a bare `--scene` run from a source tree).
pub fn game_root(cwd: &std::path::Path, exe: Option<&std::path::Path>) -> Option<PathBuf> {
    let mut candidates = vec![cwd.to_path_buf()];
    if let Some(dir) = exe.and_then(|e| e.parent()) {
        candidates.push(dir.to_path_buf());
        candidates.push(dir.join("..").join("Resources"));
    }
    candidates.into_iter().find(|c| c.join("project.jengine").is_file())
}

/// Where a game inside an app bundle saves: an app's bundle is signed (and
/// on iOS read-only), so `./saves` - inside it - is no place to write.
/// `<home>/Library/Application Support/<bundle id>/saves`, the id read from
/// the bundle's `Info.plist` (else the `.app`'s own name); `None` for a
/// game that is not in a `.app`, which keeps saving beside itself.
pub fn bundle_save_dir(root: &std::path::Path, home: &std::path::Path) -> Option<PathBuf> {
    let app = root.ancestors().find(|a| a.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")))?;
    // macOS: <X>.app/Contents/Info.plist; iOS: <X>.app/Info.plist.
    let id = [app.join("Contents").join("Info.plist"), app.join("Info.plist")]
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .and_then(|plist| plist_string(&plist, "CFBundleIdentifier"))
        .or_else(|| app.file_stem().map(|s| s.to_string_lossy().into_owned()))?;
    Some(home.join("Library").join("Application Support").join(id).join("saves"))
}

/// A string value from an XML property list: the `<string>` after `<key>key</key>`.
fn plist_string(plist: &str, key: &str) -> Option<String> {
    let after = &plist[plist.find(&format!("<key>{key}</key>"))? ..];
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")? + start;
    let value = after[start..end].trim();
    (!value.is_empty()).then(|| value.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&"))
}

/// Try to load settings from a project.jengine file in the current directory.
fn load_project_settings() -> RuntimeSettings {
    let path = std::path::Path::new("project.jengine");
    if let Ok(contents) = std::fs::read_to_string(path)
        && let Ok(project) = serde_json::from_str::<ProjectFilePartial>(&contents)
    {
        log::info!("Loaded project settings from project.jengine");
        if let Some(warning) = jengine_core::version::newer_project_warning(project.engine_version.as_deref()) {
            log::warn!("{warning}");
        }
        let mut settings = project.settings;
        if settings.entry_scene.is_none() {
            settings.entry_scene = project.default_scene;
        }
        return settings;
    }
    log::info!("No project.jengine found, using default settings");
    RuntimeSettings::default()
}

/// CLI for the wgpu-only runtime. Phase 8.1 dropped `--bench`,
/// `--wgpu-smoke`, `--wgpu-ecs`, `--wgpu-scene`, and `--gpu-backend`.
#[derive(Debug, Parser)]
#[command(name = "jengine-runtime", about = "jEngine native runtime (wgpu)")]
struct Cli {
    /// Scene file to load. When omitted, the runtime falls back to
    /// `project.jengine`'s `entry_scene` (or top-level `default_scene`)
    /// resolved relative to `assets/`.
    #[arg(long = "scene", value_name = "PATH")]
    scene: Option<PathBuf>,
}

/// Count heap allocations so the F3 overlay and `engine.frame_stats()` can
/// report memory. Costs two relaxed atomic ops per allocation; see
/// `jengine_core::profiling::memory`.
#[global_allocator]
static ALLOC: jengine_core::profiling::memory::TrackingAllocator<std::alloc::System> =
    jengine_core::profiling::memory::TrackingAllocator::new(std::alloc::System);

/// Hook a generated project runtime uses to register its own packages.
///
/// Called once, before [`PackageHost::initialize`], so a package's components
/// land in the registry the scene loader consults. Built-in packages are added
/// by the runtime first, honouring `settings.enabled_packages`.
pub type PackageHook = dyn Fn(&mut jengine_core::package::host::PackageHost);

/// Run the engine with no extra packages — what the stock `jengine-runtime`
/// binary does.
pub fn run() -> Result<()> {
    run_with(&|_host| {})
}

/// Run the engine, letting the caller register additional packages.
///
/// ```ignore
/// fn main() -> anyhow::Result<()> {
///     jengine_runtime::run_with(&|host| {
///         host.add_package(Box::new(my_package::MyPackage));
///     })
/// }
/// ```
pub fn run_with(add_packages: &PackageHook) -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("jEngine Runtime starting (wgpu)...");

    let cli = Cli::parse();
    // Start where the game's files are (`game_root`), so `project.jengine`
    // and `assets/` resolve however the game was launched.
    if let Ok(cwd) = std::env::current_dir()
        && let Some(root) = game_root(&cwd, std::env::current_exe().ok().as_deref())
        && root != cwd
    {
        match std::env::set_current_dir(&root) {
            Ok(()) => log::info!("game files at {}", root.display()),
            Err(e) => log::warn!("could not move to the game's folder {}: {e}", root.display()),
        }
    }
    if std::env::var_os("JENGINE_SAVE_DIR").is_none()
        && let (Ok(root), Some(home)) = (std::env::current_dir(), std::env::var_os("HOME"))
        && let Some(saves) = bundle_save_dir(&root, std::path::Path::new(&home))
    {
        log::info!("saves at {}", saves.display());
        // SAFETY: still single-threaded - nothing has been spawned yet.
        unsafe { std::env::set_var("JENGINE_SAVE_DIR", &saves) };
    }
    let settings = load_project_settings();

    // Resolve scene path: CLI > project entry_scene (resolved against assets/).
    let scene_path = match cli.scene {
        Some(p) => p,
        None => match settings.entry_scene.as_ref() {
            Some(entry) => {
                let resolved = std::path::Path::new("assets").join(entry);
                log::info!(
                    "no --scene supplied; using project.jengine entry_scene: {}",
                    resolved.display()
                );
                resolved
            }
            None => {
                log::error!(
                    "no --scene supplied and no project.jengine entry_scene found"
                );
                std::process::exit(2);
            }
        },
    };

    run_event_loop(EventLoop::new()?, settings, scene_path, add_packages)
}

/// Where every platform's entry ends up: the event loop is built the way the
/// platform needs (Android hands it the activity) and run the same way.
fn run_event_loop(
    event_loop: EventLoop<()>,
    settings: RuntimeSettings,
    scene_path: PathBuf,
    add_packages: &PackageHook,
) -> Result<()> {
    #[cfg(feature = "packs")]
    install_pack_http();
    let mut app = App::new(settings, scene_path, add_packages);
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// The HTTP client asset packs download through
/// (`jengine_core::asset::packs::native`, which links none of its own).
/// A 4xx or 5xx answer is an error, which the pack reports as its failure.
#[cfg(feature = "packs")]
fn install_pack_http() {
    jengine_core::asset::packs::native::set_http(Box::new(|url: &str| {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(20))
            .timeout_read(std::time::Duration::from_secs(60))
            .build();
        // ureq's error names the URL already.
        let resp = agent.get(url).call().map_err(|e| anyhow::anyhow!("{e}"))?;
        let len = resp.header("Content-Length").and_then(|v| v.parse::<u64>().ok());
        Ok((Box::new(resp.into_reader()) as Box<dyn std::io::Read + Send>, len))
    }));
    // Live ops' exposures (`LiveOps.expose`).
    jengine_core::asset::packs::native::set_http_post(Box::new(|url: &str, content_type: &str, body: &[u8]| {
        let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(20)).build();
        let resp = agent.post(url).set("Content-Type", content_type).send_bytes(body).map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut resp.into_reader(), &mut out)?;
        Ok(out)
    }));
}

struct App<'a> {
    settings: RuntimeSettings,
    scene_path: PathBuf,
    state: Option<wgpu_scene_runner::WgpuSceneState>,
    /// Borrowed rather than boxed: it lives for the whole `run_with` call and
    /// is only read when the window resumes.
    add_packages: &'a PackageHook,
}

impl<'a> App<'a> {
    fn new(
        settings: RuntimeSettings,
        scene_path: PathBuf,
        add_packages: &'a PackageHook,
    ) -> Self {
        Self { settings, scene_path, state: None, add_packages }
    }
}

impl ApplicationHandler for App<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Desktop resumes once, at startup. A phone resumes every time the
        // app comes back to the foreground, with a new native window under
        // the same winit one - so a second resume rebuilds the surface and
        // keeps everything else.
        if let Some(state) = self.state.as_mut() {
            wgpu_scene_runner::resume(state);
            return;
        }
        log::info!("building runtime for scene: {}", self.scene_path.display());
        match wgpu_scene_runner::build_state(event_loop, &self.scene_path, self.add_packages) {
            Ok(mut s) => {
                s.clear_color = self.settings.background_color;
                wgpu_scene_runner::refresh_safe_insets(&mut s);
                if let Some(cfg) = resolve_script_budget(self.settings.script_budget) {
                    log::info!(
                        "[Script] budget installed: max_ms={}, max_inv={}, policy={:?}, deferred_cap={}",
                        cfg.max_script_ms_per_frame, cfg.max_invocations_per_frame,
                        cfg.overflow_policy, cfg.deferred_cap,
                    );
                    s.script_system.set_script_budget(cfg);
                }
                self.state = Some(s);
            }
            Err(e) => {
                log::error!("failed to initialise wgpu runtime: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.as_mut() {
            wgpu_scene_runner::about_to_wait(state, event_loop);
        }
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.as_mut() {
            wgpu_scene_runner::suspend(state);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if let Some(state) = self.state.as_mut() {
            wgpu_scene_runner::handle_window_event(state, event_loop, event);
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        if let Some(state) = self.state.as_mut() {
            wgpu_scene_runner::handle_device_event(state, event);
        }
    }
}

/// Map winit physical key codes to engine KeyCodes. Used by
/// `wgpu_scene_runner::handle_window_event`.
pub(crate) fn map_key(key: winit::keyboard::KeyCode) -> Option<KeyCode> {
    use winit::keyboard::KeyCode as WK;
    match key {
        // Letters
        WK::KeyA => Some(KeyCode::A), WK::KeyB => Some(KeyCode::B),
        WK::KeyC => Some(KeyCode::C), WK::KeyD => Some(KeyCode::D),
        WK::KeyE => Some(KeyCode::E), WK::KeyF => Some(KeyCode::F),
        WK::KeyG => Some(KeyCode::G), WK::KeyH => Some(KeyCode::H),
        WK::KeyI => Some(KeyCode::I), WK::KeyJ => Some(KeyCode::J),
        WK::KeyK => Some(KeyCode::K), WK::KeyL => Some(KeyCode::L),
        WK::KeyM => Some(KeyCode::M), WK::KeyN => Some(KeyCode::N),
        WK::KeyO => Some(KeyCode::O), WK::KeyP => Some(KeyCode::P),
        WK::KeyQ => Some(KeyCode::Q), WK::KeyR => Some(KeyCode::R),
        WK::KeyS => Some(KeyCode::S), WK::KeyT => Some(KeyCode::T),
        WK::KeyU => Some(KeyCode::U), WK::KeyV => Some(KeyCode::V),
        WK::KeyW => Some(KeyCode::W), WK::KeyX => Some(KeyCode::X),
        WK::KeyY => Some(KeyCode::Y), WK::KeyZ => Some(KeyCode::Z),
        // Digits
        WK::Digit0 => Some(KeyCode::Digit0), WK::Digit1 => Some(KeyCode::Digit1),
        WK::Digit2 => Some(KeyCode::Digit2), WK::Digit3 => Some(KeyCode::Digit3),
        WK::Digit4 => Some(KeyCode::Digit4), WK::Digit5 => Some(KeyCode::Digit5),
        WK::Digit6 => Some(KeyCode::Digit6), WK::Digit7 => Some(KeyCode::Digit7),
        WK::Digit8 => Some(KeyCode::Digit8), WK::Digit9 => Some(KeyCode::Digit9),
        // F-keys
        WK::F1 => Some(KeyCode::F1), WK::F2 => Some(KeyCode::F2),
        WK::F3 => Some(KeyCode::F3), WK::F4 => Some(KeyCode::F4),
        WK::F5 => Some(KeyCode::F5), WK::F6 => Some(KeyCode::F6),
        WK::F7 => Some(KeyCode::F7), WK::F8 => Some(KeyCode::F8),
        WK::F9 => Some(KeyCode::F9), WK::F10 => Some(KeyCode::F10),
        WK::F11 => Some(KeyCode::F11), WK::F12 => Some(KeyCode::F12),
        // Arrows
        WK::ArrowLeft => Some(KeyCode::Left), WK::ArrowRight => Some(KeyCode::Right),
        WK::ArrowUp => Some(KeyCode::Up), WK::ArrowDown => Some(KeyCode::Down),
        // Modifiers
        WK::ShiftLeft => Some(KeyCode::ShiftLeft), WK::ShiftRight => Some(KeyCode::ShiftRight),
        WK::ControlLeft => Some(KeyCode::ControlLeft), WK::ControlRight => Some(KeyCode::ControlRight),
        WK::AltLeft => Some(KeyCode::AltLeft), WK::AltRight => Some(KeyCode::AltRight),
        // Special keys
        WK::Space => Some(KeyCode::Space), WK::Enter => Some(KeyCode::Enter),
        WK::Escape => Some(KeyCode::Escape), WK::Backspace => Some(KeyCode::Backspace),
        WK::Tab => Some(KeyCode::Tab),
        WK::Insert => Some(KeyCode::Insert), WK::Delete => Some(KeyCode::Delete),
        WK::Home => Some(KeyCode::Home), WK::End => Some(KeyCode::End),
        WK::PageUp => Some(KeyCode::PageUp), WK::PageDown => Some(KeyCode::PageDown),
        // Punctuation
        WK::Minus => Some(KeyCode::Minus), WK::Equal => Some(KeyCode::Equal),
        WK::BracketLeft => Some(KeyCode::BracketLeft), WK::BracketRight => Some(KeyCode::BracketRight),
        WK::Backslash => Some(KeyCode::Backslash), WK::Semicolon => Some(KeyCode::Semicolon),
        WK::Quote => Some(KeyCode::Quote), WK::Backquote => Some(KeyCode::Backquote),
        WK::Comma => Some(KeyCode::Comma), WK::Period => Some(KeyCode::Period),
        WK::Slash => Some(KeyCode::Slash),
        WK::CapsLock => Some(KeyCode::CapsLock), WK::NumLock => Some(KeyCode::NumLock),
        WK::ScrollLock => Some(KeyCode::ScrollLock), WK::PrintScreen => Some(KeyCode::PrintScreen),
        WK::Pause => Some(KeyCode::Pause),
        // Numpad
        WK::Numpad0 => Some(KeyCode::Numpad0), WK::Numpad1 => Some(KeyCode::Numpad1),
        WK::Numpad2 => Some(KeyCode::Numpad2), WK::Numpad3 => Some(KeyCode::Numpad3),
        WK::Numpad4 => Some(KeyCode::Numpad4), WK::Numpad5 => Some(KeyCode::Numpad5),
        WK::Numpad6 => Some(KeyCode::Numpad6), WK::Numpad7 => Some(KeyCode::Numpad7),
        WK::Numpad8 => Some(KeyCode::Numpad8), WK::Numpad9 => Some(KeyCode::Numpad9),
        WK::NumpadAdd => Some(KeyCode::NumpadAdd), WK::NumpadSubtract => Some(KeyCode::NumpadSubtract),
        WK::NumpadMultiply => Some(KeyCode::NumpadMultiply), WK::NumpadDivide => Some(KeyCode::NumpadDivide),
        WK::NumpadDecimal => Some(KeyCode::NumpadDecimal), WK::NumpadEnter => Some(KeyCode::NumpadEnter),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear_budget_env() {
        for k in [
            "JENGINE_SCRIPT_MAX_MS",
            "JENGINE_SCRIPT_MAX_INVOCATIONS",
            "JENGINE_SCRIPT_OVERFLOW_POLICY",
            "JENGINE_SCRIPT_DEFERRED_CAP",
        ] {
            // SAFETY: this test is the sole reader/writer of these
            // env vars; no concurrent thread is observing them.
            unsafe { std::env::remove_var(k); }
        }
    }

    #[test]
    fn resolve_script_budget_priority_chain() {
        clear_budget_env();
        assert!(resolve_script_budget(None).is_none());

        let project = ScriptBudgetSettings {
            max_script_ms_per_frame: 4.0,
            max_invocations_per_frame: 2000,
            overflow_policy: BudgetPolicySetting::Drop,
            deferred_cap: 256,
        };
        let cfg = resolve_script_budget(Some(project)).expect("project block must yield Some");
        assert_eq!(cfg.max_script_ms_per_frame, 4.0);
        assert_eq!(cfg.max_invocations_per_frame, 2000);
        assert!(matches!(cfg.overflow_policy, BudgetPolicy::Drop));
        assert_eq!(cfg.deferred_cap, 256);

        // SAFETY: see clear_budget_env.
        unsafe {
            std::env::set_var("JENGINE_SCRIPT_MAX_MS", "8.5");
            std::env::set_var("JENGINE_SCRIPT_OVERFLOW_POLICY", "Defer");
        }
        let cfg = resolve_script_budget(Some(project)).expect("env+project must yield Some");
        assert_eq!(cfg.max_script_ms_per_frame, 8.5);
        assert_eq!(cfg.max_invocations_per_frame, 2000);
        assert!(matches!(cfg.overflow_policy, BudgetPolicy::Defer));
        assert_eq!(cfg.deferred_cap, 256);

        clear_budget_env();
        // SAFETY: see clear_budget_env.
        unsafe {
            std::env::set_var("JENGINE_SCRIPT_MAX_INVOCATIONS", "500");
        }
        let cfg = resolve_script_budget(None).expect("env alone must yield Some");
        assert_eq!(cfg.max_invocations_per_frame, 500);
        assert_eq!(cfg.max_script_ms_per_frame, 0.0);
        assert!(matches!(cfg.overflow_policy, BudgetPolicy::Warn));
        clear_budget_env();
    }
}

#[cfg(test)]
mod game_root_tests {
    use super::game_root;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("jengine-root-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_working_directory_first() {
        let d = dir("cwd");
        std::fs::write(d.join("project.jengine"), "{}").unwrap();
        assert_eq!(game_root(&d, Some(&d.join("elsewhere/game"))), Some(d.clone()));
    }

    #[test]
    fn beside_the_executable_when_launched_from_elsewhere() {
        let game = dir("exe");
        std::fs::write(game.join("project.jengine"), "{}").unwrap();
        let home = dir("home");
        assert_eq!(game_root(&home, Some(&game.join("my-game"))), Some(game.clone()));
    }

    #[test]
    fn inside_a_macos_bundle() {
        let app = dir("app").join("Game.app").join("Contents");
        std::fs::create_dir_all(app.join("MacOS")).unwrap();
        std::fs::create_dir_all(app.join("Resources")).unwrap();
        std::fs::write(app.join("Resources").join("project.jengine"), "{}").unwrap();
        let root = game_root(&dir("finder"), Some(&app.join("MacOS").join("Game"))).unwrap();
        assert!(root.join("project.jengine").is_file());
        assert!(root.ends_with("Resources"));
    }

    #[test]
    fn none_from_a_source_tree() {
        let d = dir("bare");
        assert_eq!(game_root(&d, Some(&d.join("jengine-runtime"))), None);
    }

    #[test]
    fn a_bundled_game_saves_outside_its_bundle() {
        use super::bundle_save_dir;
        let home = std::path::Path::new("/Users/someone");
        let support = home.join("Library").join("Application Support");

        // macOS: the id from Contents/Info.plist.
        let app = dir("saves-mac").join("Game.app");
        let resources = app.join("Contents").join("Resources");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(
            app.join("Contents").join("Info.plist"),
            "<plist><dict>\n\t<key>CFBundleName</key>\n\t<string>Game</string>\n\
             \t<key>CFBundleIdentifier</key>\n\t<string>com.example.game</string>\n</dict></plist>",
        )
        .unwrap();
        assert_eq!(bundle_save_dir(&resources, home), Some(support.join("com.example.game").join("saves")));

        // iOS: the plist at the bundle's root; with none, the bundle's name.
        let ios = dir("saves-ios").join("Pocket.app");
        std::fs::create_dir_all(&ios).unwrap();
        assert_eq!(bundle_save_dir(&ios, home), Some(support.join("Pocket").join("saves")));

        // Not in a bundle: saves stay beside the game.
        assert_eq!(bundle_save_dir(&dir("saves-folder"), home), None);
    }
}

#[cfg(test)]
#[cfg(feature = "packs")]
mod pack_http_tests {
    use jengine_core::asset::packs::{self, PackEntry, archive, native};
    use std::io::{Read, Write};

    /// Serve `body` at `/pack.jpak` (anything else is a 404) for `requests`
    /// requests on a port of its own.
    fn serve(body: Vec<u8>, requests: usize) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(requests) {
                let mut s = stream.unwrap();
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                if head.starts_with("GET /pack.jpak ") {
                    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = s.write_all(&body);
                } else {
                    let _ = write!(s, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                }
            }
        });
        port
    }

    #[test]
    fn a_pack_downloads_over_http_and_a_missing_one_says_why() {
        let bytes = archive::encode(&[("music/theme.ogg".to_string(), vec![5; 4000])]);
        let entry = PackEntry {
            name: "music".into(),
            file: "pack.jpak".into(),
            size: bytes.len() as u64,
            sha256: packs::sha256_hex(&bytes),
            files: vec!["music/theme.ogg".into()],
        };
        let port = serve(bytes, 2);
        super::install_pack_http();
        let cache = std::env::temp_dir().join(format!("jengine-pack-http-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cache);
        let mut last = 0.0;
        let dir = native::install(&entry, &format!("http://127.0.0.1:{port}/pack.jpak"), &cache, &cache, &mut |f| last = f).unwrap();
        assert_eq!(std::fs::read(dir.join("music/theme.ogg")).unwrap(), vec![5; 4000]);
        assert!(last > 0.9);

        let mut gone = entry.clone();
        gone.name = "gone".into();
        let err = native::install(&gone, &format!("http://127.0.0.1:{port}/nothing.jpak"), &cache, &cache, &mut |_| {}).unwrap_err();
        assert!(format!("{err:#}").contains("404"), "{err:#}");
        let _ = std::fs::remove_dir_all(&cache);
    }
}
