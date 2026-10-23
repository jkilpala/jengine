//! `jengine android-smoke`: every scene of a project, started on a connected
//! device or emulator, looked at for the failures a desktop check cannot see.
//!
//! The Android code paths - startup, unpacking assets, the display cut-out
//! over JNI, suspend and resume - are compiled only for Android, and nothing
//! on a desktop runs them. So this starts the installed build once per scene
//! (a fresh process each time, with the scene named in the launch intent,
//! which a debug-key build honours), lets it run, and then reads three
//! things:
//!
//! - **the process** - gone means it crashed or quit;
//! - **logcat** - a Rust panic, a native crash, or an error the engine
//!   logged (a Lua error is one);
//! - **the screen** - a screenshot of one flat colour is a scene that did
//!   not draw.
//!
//! Then it backgrounds and resumes the app, and turns the phone to the other
//! landscape, which is where a notch changes sides.
//!
//! What it cannot tell is whether a scene looks *right*. The screenshots are
//! kept for that.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::app::ResolvedApp;

/// What to run.
#[derive(Debug, Clone)]
pub struct SmokeOptions {
    /// Only scenes whose path contains one of these. Empty is every scene.
    pub filters: Vec<String>,
    /// How long each scene runs before it is judged.
    pub dwell: Duration,
    /// Also background, resume and turn the app.
    pub lifecycle: bool,
    /// Run on this graphics backend (`gl`) instead of the one the device
    /// picks - how the GLES fallback is tested on a device with Vulkan.
    pub backend: Option<String>,
}

impl Default for SmokeOptions {
    fn default() -> Self {
        Self { filters: Vec::new(), dwell: Duration::from_secs(3), lifecycle: true, backend: None }
    }
}

/// How bad one finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Reported, does not fail the run.
    Warning,
    /// Fails the run.
    Error,
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
}

/// One scene, or one lifecycle step.
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub name: String,
    pub findings: Vec<Finding>,
    pub screenshot: Option<PathBuf>,
}

impl CheckResult {
    pub fn failed(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error)
    }
}

/// Every scene under `<project>/assets`, as the path the runtime loads
/// (`scenes/05-prefabs.scene`), in the order a person would number them.
pub fn project_scenes(project: &Path) -> Result<Vec<String>> {
    let assets = project.join("assets");
    let mut out = Vec::new();
    collect_scenes(&assets, &assets, &mut out)?;
    out.sort_by_key(|s| natural_key(s));
    Ok(out)
}

fn collect_scenes(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_scenes(root, &path, out)?;
        } else if path.extension().is_some_and(|e| e == "scene") {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// `scenes/9-x` before `scenes/10-x`: each run of text is followed by the
/// number after it, so runs of digits compare as numbers.
fn natural_key(s: &str) -> Vec<(String, u64)> {
    let mut key = Vec::new();
    let mut text = String::new();
    let mut digits = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            if !digits.is_empty() {
                key.push((std::mem::take(&mut text), digits.parse().unwrap_or(u64::MAX)));
                digits.clear();
            }
            text.push(c);
        }
    }
    key.push((text, digits.parse().unwrap_or(0)));
    key
}

/// Read a `logcat -v brief` dump for what went wrong.
///
/// A line is `P/tag( pid): message`. What counts:
/// - a native crash (`F/libc`, `F/DEBUG`) or a Java one (`E/AndroidRuntime`);
/// - anything the engine logged at error level, which covers a Rust panic
///   (the runtime's panic hook logs it) and a script error;
/// - the engine's warnings, as warnings.
pub fn logcat_findings(dump: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    for line in dump.lines() {
        let Some((head, message)) = line.split_once("): ") else { continue };
        let Some((priority, tag)) = head.split_once('/') else { continue };
        let tag = tag.split('(').next().unwrap_or("").trim();
        let message = message.trim();
        let severity = match (priority, tag) {
            ("F", "libc") | ("F", "DEBUG") | ("E", "AndroidRuntime") => Severity::Error,
            ("E" | "F", "jengine") => Severity::Error,
            ("W", "jengine") => Severity::Warning,
            _ => continue,
        };
        out.push(Finding { severity, message: format!("{tag}: {message}") });
    }
    out
}

/// Whether a screenshot is one flat colour - a scene that drew nothing, or
/// a surface that never came back.
///
/// Samples a grid rather than every pixel; a real scene differs from its
/// background somewhere on a 16-pixel grid.
pub fn looks_blank(image: &image::RgbaImage) -> bool {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return true;
    }
    let (mut lo, mut hi) = ([255u8; 3], [0u8; 3]);
    for y in (0..h).step_by(16) {
        for x in (0..w).step_by(16) {
            let p = image.get_pixel(x, y);
            for c in 0..3 {
                lo[c] = lo[c].min(p[c]);
                hi[c] = hi[c].max(p[c]);
            }
        }
    }
    (0..3).all(|c| hi[c] - lo[c] <= 4)
}

/// The device the run talks to, through `adb`.
struct Device {
    adb: PathBuf,
}

impl Device {
    fn adb(&self) -> Command {
        Command::new(&self.adb)
    }

    fn output(&self, args: &[&str]) -> Result<Vec<u8>> {
        let out = self.adb().args(args).output().with_context(|| format!("running adb {}", args.join(" ")))?;
        if !out.status.success() {
            // `am` reports on stdout (`cmd: Can't find service: activity`
            // is the system process being down), adb itself on stderr.
            let mut why = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let said = String::from_utf8_lossy(&out.stdout);
            if !said.trim().is_empty() {
                if !why.is_empty() {
                    why.push_str(" / ");
                }
                why.push_str(said.trim());
            }
            bail!("adb {} failed: {why}", args.join(" "));
        }
        Ok(out.stdout)
    }

    fn shell(&self, command: &str) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.output(&["shell", command])?).trim().to_string())
    }

    fn pid(&self, id: &str) -> Option<String> {
        self.shell(&format!("pidof {id}")).ok().filter(|p| !p.is_empty())
    }

    fn clear_log(&self) -> Result<()> {
        self.output(&["logcat", "-c"]).map(|_| ())
    }

    fn log(&self) -> Result<String> {
        let out = self.output(&[
            "logcat", "-d", "-v", "brief", "jengine:V", "AndroidRuntime:E", "DEBUG:V", "libc:F", "*:S",
        ])?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    fn screenshot(&self) -> Result<image::RgbaImage> {
        let png = self.output(&["exec-out", "screencap", "-p"])?;
        Ok(image::load_from_memory(&png).context("decoding the screenshot")?.to_rgba8())
    }
}

/// The ABI of the connected device, which is the one to build for.
pub fn device_abi() -> Result<String> {
    let sdk = crate::android::find_sdk()?;
    let device = Device { adb: adb_path(&sdk) };
    device.output(&["get-state"]).context("no device or emulator is connected (adb get-state)")?;
    let abi = device.shell("getprop ro.product.cpu.abi")?;
    if abi.is_empty() {
        bail!("the device did not report its ABI");
    }
    Ok(abi)
}

fn adb_path(sdk: &crate::android::AndroidSdk) -> PathBuf {
    let name = if cfg!(windows) { "adb.exe" } else { "adb" };
    sdk.root.join("platform-tools").join(name)
}

/// Run every chosen scene of `project` on the connected device, which must
/// have the project's debug-key build installed. Screenshots go to
/// `out/shots/`.
pub fn run_smoke(project: &Path, out: &Path, opts: &SmokeOptions) -> Result<Vec<CheckResult>> {
    let app = crate::app::read(project)?;
    let sdk = crate::android::find_sdk()?;
    let device = Device { adb: adb_path(&sdk) };
    device.output(&["get-state"]).context("no device or emulator is connected (adb get-state)")?;
    if device.shell(&format!("pm path {}", app.id))?.is_empty() {
        bail!("{} is not installed on the device - build it with `jengine android-build --install`", app.id);
    }

    // Android's one-time "Viewing full screen" notice, shown over an
    // immersive app after a device reset. It takes focus, so the lifecycle
    // check's HOME press dismissed it instead of backgrounding the game.
    // Marked as seen, as a player's tap would.
    let _ = device.shell("settings put secure immersive_mode_confirmations confirmed");

    let mut scenes = project_scenes(project)?;
    if !opts.filters.is_empty() {
        scenes.retain(|s| opts.filters.iter().any(|f| s.contains(f.as_str())));
    }
    if scenes.is_empty() {
        bail!("no scene matches");
    }
    let shots = out.join("shots");
    std::fs::create_dir_all(&shots)?;

    let mut results = Vec::new();
    for (i, scene) in scenes.iter().enumerate() {
        crate::progress::step(&format!("[{}/{}] {scene}", i + 1, scenes.len()));
        // A step that cannot be carried out is a finding, not the end of
        // the run: a device that fails at scene 112 still has 111 results.
        let result = run_scene(&device, &app, scene, opts, &shots).unwrap_or_else(|e| CheckResult {
            name: scene.clone(),
            findings: vec![Finding { severity: Severity::Error, message: format!("{e:#}") }],
            screenshot: None,
        });
        for f in &result.findings {
            log::info!("  {:?}: {}", f.severity, f.message);
        }
        results.push(result);
    }
    if opts.lifecycle {
        let entry = scenes[0].clone();
        crate::progress::step(&format!("Lifecycle: background, resume and turn, on {entry}"));
        let rotation: Vec<(&str, String)> = ["accelerometer_rotation", "user_rotation"]
            .into_iter()
            .map(|k| (k, device.shell(&format!("settings get system {k}")).unwrap_or_default()))
            .collect();
        match run_lifecycle(&device, &app, &entry, opts, &shots) {
            Ok(r) => results.extend(r),
            Err(e) => results.push(CheckResult {
                name: "background, resume and turn".into(),
                findings: vec![Finding { severity: Severity::Error, message: format!("{e:#}") }],
                screenshot: None,
            }),
        }
        // Rotation is a device setting; put it back even when the step failed.
        for (k, v) in rotation {
            if !v.is_empty() && v != "null" {
                let _ = device.shell(&format!("settings put system {k} {v}"));
            }
        }
        crate::progress::step(&format!("Controller: a button and a stick, on {entry}"));
        match run_controller(&device, &app, &entry, opts, &shots) {
            Ok(r) => results.push(r),
            Err(e) => results.push(CheckResult {
                name: "a controller".into(),
                findings: vec![Finding { severity: Severity::Error, message: format!("{e:#}") }],
                screenshot: None,
            }),
        }
        if let Some(menu) = entry_scene(project) {
            crate::progress::step(&format!("Menu: D-pad and A on the entry scene, {menu}"));
            match run_menu(&device, &app, &menu, opts, &shots) {
                Ok(r) => results.push(r),
                Err(e) => results.push(CheckResult {
                    name: "the entry menu from a controller".into(),
                    findings: vec![Finding { severity: Severity::Error, message: format!("{e:#}") }],
                    screenshot: None,
                }),
            }
        }
    }
    // Leave the device as it was found: nothing of ours running.
    let _ = device.shell(&format!("am force-stop {}", app.id));
    Ok(results)
}

fn start(device: &Device, app: &ResolvedApp, scene: &str, opts: &SmokeOptions) -> Result<()> {
    let component = format!("{}/{}", app.id, crate::android::activity_class(app));
    // -S: stop the app first, so every scene starts in a fresh process.
    // -W: return once the activity is up.
    let mut args = vec!["shell", "am", "start", "-S", "-W", "-n", &component, "--es", "scene", scene];
    if let Some(backend) = &opts.backend {
        args.extend(["--es", "backend", backend.as_str()]);
    }
    device.output(&args)?;
    Ok(())
}

/// A finding if the log shows the run on another backend than was asked -
/// which would make a GLES pass mean nothing.
fn check_backend(log: &str, opts: &SmokeOptions, findings: &mut Vec<Finding>) {
    let Some(want) = &opts.backend else { return };
    let Some(line) = log.lines().find(|l| l.contains("wgpu adapter:")) else { return };
    let got = line.rsplit("backend=").next().unwrap_or("").trim_end_matches(')').to_ascii_lowercase();
    if !got.contains(&want.to_ascii_lowercase()) {
        findings.push(Finding {
            severity: Severity::Error,
            message: format!("asked for the {want} backend and ran on {got}"),
        });
    }
}

fn shot_name(scene: &str) -> String {
    scene.trim_end_matches(".scene").replace(['/', '\\'], "_")
}

/// Screenshot into `shots`, and a finding if it is blank or cannot be taken.
fn look(device: &Device, shots: &Path, name: &str, findings: &mut Vec<Finding>) -> Option<PathBuf> {
    match device.screenshot() {
        Ok(img) => {
            if looks_blank(&img) {
                findings.push(Finding {
                    severity: Severity::Error,
                    message: "the screen is one flat colour - nothing drew".into(),
                });
            }
            let path = shots.join(format!("{name}.png"));
            match img.save(&path) {
                Ok(()) => Some(path),
                Err(e) => {
                    log::warn!("could not save {}: {e}", path.display());
                    None
                }
            }
        }
        Err(e) => {
            findings.push(Finding { severity: Severity::Warning, message: format!("no screenshot: {e:#}") });
            None
        }
    }
}

fn run_scene(
    device: &Device,
    app: &ResolvedApp,
    scene: &str,
    opts: &SmokeOptions,
    shots: &Path,
) -> Result<CheckResult> {
    // Stop the last scene's process before clearing the log: stopped by
    // `am start -S` instead, it went on logging after the clear, and its
    // errors were reported against the next scene.
    device.shell(&format!("am force-stop {}", app.id))?;
    device.clear_log()?;
    start(device, app, scene, opts)?;
    std::thread::sleep(opts.dwell);
    let alive = device.pid(&app.id).is_some();
    let log = device.log()?;
    let mut findings = logcat_findings(&log);
    check_backend(&log, opts, &mut findings);
    if !alive {
        findings.push(Finding { severity: Severity::Error, message: "the process is gone - it crashed or quit".into() });
    }
    let started_there = log.contains(&format!("building runtime for scene: assets/{scene}"));
    if !started_there && alive {
        findings.push(Finding {
            severity: Severity::Error,
            message: "did not start at this scene - is the installed build debug-signed (debuggable)?".into(),
        });
    }
    let screenshot = if alive { look(device, shots, &shot_name(scene), &mut findings) } else { None };
    Ok(CheckResult { name: scene.to_string(), findings, screenshot })
}

/// Background and resume, then turn to the other landscape and back.
fn run_lifecycle(
    device: &Device,
    app: &ResolvedApp,
    scene: &str,
    opts: &SmokeOptions,
    shots: &Path,
) -> Result<Vec<CheckResult>> {
    let dwell = opts.dwell;
    let mut results = Vec::new();

    // Background and resume: the surface is dropped and rebuilt, audio
    // paused and resumed, in the same process.
    device.clear_log()?;
    start(device, app, scene, opts)?;
    std::thread::sleep(dwell);
    let before = device.pid(&app.id);
    device.shell("input keyevent KEYCODE_HOME")?;
    std::thread::sleep(Duration::from_secs(2));
    let component = format!("{}/{}", app.id, crate::android::activity_class(app));
    device.output(&["shell", "am", "start", "-W", "-n", &component])?;
    std::thread::sleep(dwell);
    let log = device.log()?;
    let mut findings = logcat_findings(&log);
    let after = device.pid(&app.id);
    if after.is_none() {
        findings.push(Finding { severity: Severity::Error, message: "the process is gone after resuming".into() });
    } else if after != before {
        findings.push(Finding {
            severity: Severity::Error,
            message: format!("resumed in a new process ({before:?} -> {after:?}) - the game restarted"),
        });
    }
    for (needle, what) in [("[wgpu-scene] suspended", "suspended"), ("[wgpu-scene] resumed", "resumed")] {
        if !log.contains(needle) {
            findings.push(Finding { severity: Severity::Error, message: format!("never logged that it {what}") });
        }
    }
    let screenshot = if after.is_some() { look(device, shots, "lifecycle_resumed", &mut findings) } else { None };
    results.push(CheckResult { name: "background and resume".into(), findings, screenshot });

    // Turn to the other landscape (rotation 3), where a notch is on the
    // other side, and back. The activity handles the turn as a resize, so
    // it must stay the same process.
    device.clear_log()?;
    // `run_smoke` puts both settings back afterwards, whatever happens here.
    let before = device.pid(&app.id);
    device.shell("settings put system accelerometer_rotation 0")?;
    device.shell("settings put system user_rotation 3")?;
    std::thread::sleep(dwell);
    let mut findings = Vec::new();
    let turned = look(device, shots, "lifecycle_turned", &mut findings);
    device.shell("settings put system user_rotation 1")?;
    std::thread::sleep(dwell);
    findings.extend(logcat_findings(&device.log()?));
    let after = device.pid(&app.id);
    if after.is_none() {
        findings.push(Finding { severity: Severity::Error, message: "the process is gone after turning".into() });
    } else if after != before {
        findings.push(Finding { severity: Severity::Error, message: "turning the phone restarted the game".into() });
    }
    results.push(CheckResult { name: "turn to the other landscape".into(), findings, screenshot: turned });
    Ok(results)
}

/// A controller's button and stick, injected with `adb shell input`: the
/// engine must say it saw a controller, and nothing may fail. Before the
/// runtime claimed controller events, winit turned a stick into touches in
/// the screen's top-left corner - a press nobody made.
fn run_controller(
    device: &Device,
    app: &ResolvedApp,
    scene: &str,
    opts: &SmokeOptions,
    shots: &Path,
) -> Result<CheckResult> {
    device.shell(&format!("am force-stop {}", app.id))?;
    device.clear_log()?;
    start(device, app, scene, opts)?;
    std::thread::sleep(opts.dwell);
    device.shell("input gamepad keyevent BUTTON_A")?;
    // Up and to the right, then back to centre.
    device.shell("input joystick motionevent DOWN 0.8 -0.8")?;
    device.shell("input joystick motionevent MOVE 0.8 -0.8")?;
    device.shell("input joystick motionevent UP 0 0")?;
    std::thread::sleep(Duration::from_secs(1));
    let log = device.log()?;
    let mut findings = logcat_findings(&log);
    if device.pid(&app.id).is_none() {
        findings.push(Finding { severity: Severity::Error, message: "the process is gone after controller input".into() });
    }
    if !log.contains("controller connected as Pad") {
        findings.push(Finding {
            severity: Severity::Error,
            message: "the engine never saw a controller - is the winit input hook installed?".into(),
        });
    }
    let screenshot = look(device, shots, "controller", &mut findings);
    Ok(CheckResult { name: "a controller".into(), findings, screenshot })
}

/// The scene a project starts at (`settings.entry_scene`, or the top-level
/// `default_scene`), as the path the runtime loads.
fn entry_scene(project: &Path) -> Option<String> {
    let text = std::fs::read_to_string(project.join("project.jengine")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("settings")
        .and_then(|s| s.get("entry_scene"))
        .or_else(|| v.get("default_scene"))
        .and_then(|s| s.as_str())
        .map(str::to_string)
}

/// The entry scene's menu from a controller: D-pad down twice and A - the
/// second press moves off the first item, so a menu whose first row does
/// nothing still gets a row that does. A menu is the one screen a
/// controller player cannot skip, so a press that changes no scene is
/// reported - as a warning, since an entry scene need not be a menu.
fn run_menu(
    device: &Device,
    app: &ResolvedApp,
    scene: &str,
    opts: &SmokeOptions,
    shots: &Path,
) -> Result<CheckResult> {
    device.shell(&format!("am force-stop {}", app.id))?;
    device.clear_log()?;
    start(device, app, scene, opts)?;
    std::thread::sleep(opts.dwell);
    for key in ["DPAD_DOWN", "DPAD_DOWN", "BUTTON_A"] {
        device.shell(&format!("input gamepad keyevent {key}"))?;
        std::thread::sleep(Duration::from_millis(300));
    }
    std::thread::sleep(opts.dwell);
    let log = device.log()?;
    let mut findings = logcat_findings(&log);
    if device.pid(&app.id).is_none() {
        findings.push(Finding { severity: Severity::Error, message: "the process is gone after the menu press".into() });
    } else if !log.contains("requested Load(") && !log.contains("requested Push(") {
        findings.push(Finding {
            severity: Severity::Warning,
            message: format!(
                "D-pad down twice and A on {scene} changed no scene - if it is a menu, can a controller use it? \
                 (its .gui needs `focusable` buttons and no `\"navigable\": false`)"
            ),
        });
    }
    let screenshot = look(device, shots, "menu", &mut findings);
    Ok(CheckResult { name: "the entry menu from a controller".into(), findings, screenshot })
}

/// The run as Markdown: failures first, then warnings, then what passed.
pub fn report_markdown(project: &Path, opts: &SmokeOptions, results: &[CheckResult]) -> String {
    let failed: Vec<&CheckResult> = results.iter().filter(|r| r.failed()).collect();
    let mut s = format!(
        "# Android smoke: {}{}\n\n{} checked, **{} failed**.\n\n",
        project.display(),
        opts.backend.as_ref().map(|b| format!(" ({b} backend)")).unwrap_or_default(),
        results.len(),
        failed.len(),
    );
    if !failed.is_empty() {
        s.push_str("## Failed\n\n");
        for r in &failed {
            s.push_str(&format!("### {}\n\n", r.name));
            if let Some(shot) = &r.screenshot {
                s.push_str(&format!("Screenshot: `{}`\n\n", shot.display()));
            }
            // A scene that errors once a frame says so hundreds of times.
            for (message, n) in tally(r.findings.iter().filter(|f| f.severity == Severity::Error).map(|f| &f.message)) {
                let times = if n > 1 { format!(" (x{n})") } else { String::new() };
                s.push_str(&format!("- {message}{times}\n"));
            }
            s.push('\n');
        }
    }
    // Warnings once each, with where they were seen: the same startup
    // warning in every scene is one line, not a hundred sections.
    let mut warnings: Vec<(&String, Vec<&str>)> = Vec::new();
    for r in results {
        for f in r.findings.iter().filter(|f| f.severity == Severity::Warning) {
            match warnings.iter_mut().find(|(m, _)| *m == &f.message) {
                Some((_, seen)) => {
                    if !seen.contains(&r.name.as_str()) {
                        seen.push(&r.name);
                    }
                }
                None => warnings.push((&f.message, vec![&r.name])),
            }
        }
    }
    if !warnings.is_empty() {
        s.push_str("## Warnings\n\n");
        for (message, seen) in warnings {
            let wher = if seen.len() == results.len() {
                "every check".to_string()
            } else if seen.len() > 4 {
                format!("{} checks", seen.len())
            } else {
                seen.join(", ")
            };
            s.push_str(&format!("- {message} - {wher}\n"));
        }
        s.push('\n');
    }
    let passed: Vec<_> = results.iter().filter(|r| !r.failed()).map(|r| r.name.as_str()).collect();
    if !passed.is_empty() {
        s.push_str(&format!("## Passed\n\n{}\n", passed.join(", ")));
    }
    s
}

/// Each distinct message once, in first-seen order, with its count.
fn tally<'a>(messages: impl Iterator<Item = &'a String>) -> Vec<(&'a String, usize)> {
    let mut out: Vec<(&String, usize)> = Vec::new();
    for m in messages {
        match out.iter_mut().find(|(g, _)| *g == m) {
            Some((_, n)) => *n += 1,
            None => out.push((m, 1)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenes_sort_as_numbered() {
        let mut v = vec!["scenes/10-gui.scene", "scenes/9-tilemap.scene", "scenes/100-x.scene", "scenes/hub.scene"];
        v.sort_by_key(|s| natural_key(s));
        assert_eq!(v, ["scenes/9-tilemap.scene", "scenes/10-gui.scene", "scenes/100-x.scene", "scenes/hub.scene"]);
    }

    #[test]
    fn logcat_errors_warnings_and_crashes_are_told_apart() {
        let dump = "\
I/jengine( 4242): building runtime for scene: assets/scenes/hub.scene
W/jengine( 4242): gilrs has no Android backend
E/jengine( 4242): Bridge (Luau) error: attempt to call a nil value
E/jengine( 4242): panic: panicked at src/lib.rs:1:1
F/libc    ( 4242): Fatal signal 11 (SIGSEGV)
E/SomethingElse( 99): not ours
--------- beginning of crash
";
        let f = logcat_findings(dump);
        let errors: Vec<_> = f.iter().filter(|f| f.severity == Severity::Error).map(|f| f.message.as_str()).collect();
        assert_eq!(
            errors,
            [
                "jengine: Bridge (Luau) error: attempt to call a nil value",
                "jengine: panic: panicked at src/lib.rs:1:1",
                "libc: Fatal signal 11 (SIGSEGV)",
            ]
        );
        assert_eq!(f.iter().filter(|f| f.severity == Severity::Warning).count(), 1);
    }

    #[test]
    fn a_flat_screen_is_blank_and_a_drawn_one_is_not() {
        let mut img = image::RgbaImage::from_pixel(320, 160, image::Rgba([10, 10, 12, 255]));
        assert!(looks_blank(&img));
        for x in 100..140 {
            for y in 60..100 {
                img.put_pixel(x, y, image::Rgba([200, 80, 40, 255]));
            }
        }
        assert!(!looks_blank(&img));
    }

    #[test]
    fn the_report_puts_failures_first_and_counts_repeats() {
        let results = vec![
            CheckResult { name: "scenes/a.scene".into(), findings: vec![], screenshot: None },
            CheckResult {
                name: "scenes/b.scene".into(),
                findings: vec![
                    Finding { severity: Severity::Error, message: "boom".into() },
                    Finding { severity: Severity::Error, message: "boom".into() },
                ],
                screenshot: None,
            },
        ];
        let md = report_markdown(Path::new("p"), &SmokeOptions::default(), &results);
        assert!(md.contains("**1 failed**"));
        assert!(md.contains("- boom (x2)"));
        assert!(md.find("## Failed").unwrap() < md.find("## Passed").unwrap());
    }
}
