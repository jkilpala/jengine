//! Help > Check Build Tools: what building a game needs on this machine,
//! and what is missing.
//!
//! Building a game compiles the engine from source, so an installed editor
//! is only as useful as the toolchain beside it - and until this existed the
//! first sign of a missing one was a raw cargo error in the Build panel.
//! Every check runs a tool with a timeout on a thread of its own, so a hung
//! tool costs a row, never the page.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::Emitter;

/// The oldest Rust the engine builds with when the engine's own
/// `Cargo.toml` cannot be read (`rust-version` in the workspace manifest).
const FALLBACK_MIN_RUST: &str = "1.97";

/// How long a `--version` may take. `wsl.exe` and `rustup` can be slow to
/// start cold; a tool that takes longer than this is reported as not
/// answering.
const QUICK: Duration = Duration::from_secs(12);
/// `vswhere` and `rustup target list` read more than a version string.
const SLOW: Duration = Duration::from_secs(20);

const RUSTUP_URL: &str = if cfg!(windows) { "https://win.rustup.rs/x86_64" } else { "https://rustup.rs" };
const RUSTUP_PAGE: &str = "https://rustup.rs";
const MSVC_URL: &str = "https://visualstudio.microsoft.com/visual-cpp-build-tools/";
const WASM_PACK_URL: &str = "https://crates.io/crates/wasm-pack";
const CARGO_NDK_URL: &str = "https://crates.io/crates/cargo-ndk";
const ANDROID_STUDIO_URL: &str = "https://developer.android.com/studio";
const WSL_URL: &str = "https://learn.microsoft.com/windows/wsl/install";
const ENGINE_REPO_URL: &str = "https://github.com/jkilpala/jengine";
const BINARYEN_URL: &str = "https://github.com/WebAssembly/binaryen/releases";

/// Install ids `install_build_tool` accepts.
pub const INSTALL_WASM32_TARGET: &str = "wasm32_target";
pub const INSTALL_WASM_PACK: &str = "wasm_pack";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Ok,
    Missing,
    Outdated,
    /// Missing, and only some builds need it.
    OptionalMissing,
}

/// Which builds a row stands in the way of - the summary line counts by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolGroup {
    /// Every game build.
    Core,
    /// Web builds and project viewports with Rust packages.
    Web,
    /// One platform, or nice to have.
    Optional,
}

/// What the row's button does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolAction {
    /// Open a page in the browser.
    Download { url: String, label: String },
    /// `install_build_tool(id)`.
    Install { id: String, label: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolRow {
    pub id: String,
    pub name: String,
    pub status: ToolStatus,
    pub group: ToolGroup,
    /// The version found, as the tool prints it (`1.97.1`).
    pub version: Option<String>,
    /// "Every game build", "Android only".
    pub needed_for: String,
    /// A path, the reason it is missing, or what to pick in an installer.
    pub detail: Option<String>,
    pub action: Option<ToolAction>,
    /// A second way to get it, when `action` is an install.
    pub help_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BuildToolsReport {
    /// `windows`, `macos`, `linux`.
    pub os: String,
    pub rows: Vec<ToolRow>,
}

impl ToolRow {
    fn new(id: &str, name: &str, group: ToolGroup, needed_for: &str) -> ToolRow {
        ToolRow {
            id: id.into(),
            name: name.into(),
            status: ToolStatus::Ok,
            group,
            version: None,
            needed_for: needed_for.into(),
            detail: None,
            action: None,
            help_url: None,
        }
    }

    /// Missing - or optional-missing for an optional row.
    fn missing(mut self, detail: impl Into<String>) -> ToolRow {
        self.status = if self.group == ToolGroup::Optional { ToolStatus::OptionalMissing } else { ToolStatus::Missing };
        self.detail = Some(detail.into());
        self
    }

    fn download(mut self, url: &str, label: &str) -> ToolRow {
        self.action = Some(ToolAction::Download { url: url.into(), label: label.into() });
        self
    }

    fn install(mut self, id: &str, label: &str) -> ToolRow {
        self.action = Some(ToolAction::Install { id: id.into(), label: label.into() });
        self
    }
}

// ─────────────────────────────── versions ────────────────────────────────

/// A dotted version, missing parts zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// `1.97`, `1.97.1`, `1.97.1-nightly` - the leading numbers only.
pub fn parse_dotted(s: &str) -> Option<Version> {
    let s = s.trim().trim_start_matches('v');
    let mut parts = s.split(|c: char| c == '.' || c == '-' || c == '+');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|p| p.parse().ok());
    let patch = parts.next().and_then(|p| p.parse().ok());
    // A bare number (`116`, wasm-opt's) is a version too, but a word is not.
    Some(Version(major, minor.unwrap_or(0), patch.unwrap_or(0)))
}

/// The first word of a `--version` line that looks like `x.y` or `x.y.z`:
/// `rustc 1.97.1 (8bab26f4f 2026-07-14)` is `1.97.1`.
pub fn parse_version(output: &str) -> Option<Version> {
    output
        .split_whitespace()
        .find(|w| {
            let w = w.trim_start_matches('v');
            w.contains('.') && w.chars().next().is_some_and(|c| c.is_ascii_digit())
        })
        .and_then(parse_dotted)
}

/// Whether `found` is older than the engine's `rust-version`.
pub fn is_outdated(found: Version, min: Version) -> bool {
    found < min
}

/// `rust-version = "1.97"` from a workspace manifest.
pub fn rust_version_in(manifest: &str) -> Option<Version> {
    manifest.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("rust-version")?.trim_start().strip_prefix('=')?;
        let value = rest.trim().trim_matches('"');
        // `rust-version.workspace = true` is a member's, not a number.
        parse_dotted(value).filter(|_| rest.trim().starts_with('"'))
    })
}

fn min_rust(engine: Option<&Path>) -> Version {
    engine
        .and_then(|e| std::fs::read_to_string(e.join("Cargo.toml")).ok())
        .and_then(|m| rust_version_in(&m))
        .unwrap_or_else(|| parse_dotted(FALLBACK_MIN_RUST).expect("a version"))
}

// ─────────────────────────────── running ─────────────────────────────────

#[derive(Debug)]
enum Probe {
    /// It ran; exit status, and stdout + stderr.
    Ran { success: bool, out: String },
    /// No such program.
    NotFound,
    /// It did not finish in time (and was killed).
    TimedOut,
    Failed(String),
}

/// Text a tool printed. `wsl.exe` writes UTF-16LE.
fn decode(bytes: &[u8]) -> String {
    let nuls = bytes.iter().filter(|b| **b == 0).count();
    if bytes.len() >= 2 && nuls * 3 >= bytes.len() {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units).replace('\u{feff}', "")
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// No console window flashing up behind the editor for each check.
fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Run `program args`, killing it after `timeout`.
fn probe(program: &Path, args: &[&str], timeout: Duration) -> Probe {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match quiet(&mut cmd).spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Probe::NotFound,
        Err(e) => return Probe::Failed(e.to_string()),
    };
    // Drain both pipes on their own threads: a tool that fills one would
    // otherwise block before it exits.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Probe::TimedOut;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Probe::Failed(e.to_string()),
        }
    };
    let mut text = decode(&out.join().unwrap_or_default());
    let err = decode(&err.join().unwrap_or_default());
    if !err.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&err);
    }
    Probe::Ran { success: status.success(), out: text }
}

fn first_line(s: &str) -> String {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string()
}

/// `~/.cargo` (or `CARGO_HOME`) - where rustup puts cargo, rustc and every
/// `cargo install`ed tool.
fn cargo_home() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(|h| PathBuf::from(h).join(".cargo")))
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// A tool that is in `~/.cargo/bin` but not on this process's PATH: just
/// installed, and the editor was started before the installer changed PATH.
/// Builds call tools by name, so it is still missing to them.
fn off_path(name: &str) -> Option<PathBuf> {
    cargo_home().map(|h| h.join("bin").join(exe(name))).filter(|p| p.is_file())
}

fn not_on_path_note(name: &str) -> String {
    match off_path(name) {
        Some(p) => format!(
            "Installed at {}, but not on the editor's PATH. Restart the editor so it sees the new PATH.",
            p.display()
        ),
        None => format!("`{name}` was not found on PATH."),
    }
}

/// Run `name --version` and turn it into a row: ok with its version, or
/// missing with why.
fn version_row(mut row: ToolRow, name: &str, args: &[&str]) -> (ToolRow, Option<Version>) {
    match probe(Path::new(name), args, QUICK) {
        Probe::Ran { success: true, out } => {
            let version = parse_version(&out);
            row.version = Some(version.map(|v| v.to_string()).unwrap_or_else(|| first_line(&out)));
            (row, version)
        }
        Probe::Ran { success: false, out } => (row.missing(format!("`{name}` failed: {}", first_line(&out))), None),
        Probe::NotFound => (row.missing(not_on_path_note(name)), None),
        Probe::TimedOut => (row.missing(format!("`{name}` did not answer within {} s.", QUICK.as_secs())), None),
        Probe::Failed(e) => (row.missing(format!("Could not run `{name}`: {e}")), None),
    }
}

// ─────────────────────────────── the checks ──────────────────────────────

fn check_rust(id: &str, name: &str, tool: &str, min: Version) -> ToolRow {
    let row = ToolRow::new(id, name, ToolGroup::Core, "Every game build");
    let (row, version) = version_row(row, tool, &["--version"]);
    match version {
        Some(v) if is_outdated(v, min) => ToolRow {
            status: ToolStatus::Outdated,
            detail: Some(format!("The engine needs Rust {min} or later. Run `rustup update` to update.")),
            ..row
        },
        Some(_) => row,
        None => row.download(RUSTUP_URL, "Get rustup"),
    }
}

fn check_rustup() -> (ToolRow, bool) {
    let row = ToolRow::new("rustup", "rustup", ToolGroup::Core, "Installing and updating Rust and its targets");
    let (row, _) = version_row(row, "rustup", &["--version"]);
    let found = row.status == ToolStatus::Ok;
    (if found { row } else { row.download(RUSTUP_URL, "Get rustup") }, found)
}

/// The C++ tools rustc links with on Windows, found the way rustc's `cc`
/// finds them: Visual Studio's own locator.
#[cfg(windows)]
fn check_msvc() -> ToolRow {
    let row = ToolRow::new("msvc", "MSVC build tools (link.exe)", ToolGroup::Core, "Every game build on Windows")
        .download(MSVC_URL, "Get Build Tools");
    let pick = "In the installer, pick the \"Desktop development with C++\" workload.";
    let vswhere = std::env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"))
        .join(r"Microsoft Visual Studio\Installer\vswhere.exe");
    if !vswhere.is_file() {
        return row.missing(format!("No Visual Studio or Build Tools is installed. {pick}"));
    }
    let args = [
        "-latest",
        "-products",
        "*",
        "-requires",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
        "-format",
        "json",
        "-utf8",
    ];
    let out = match probe(&vswhere, &args, SLOW) {
        Probe::Ran { success: true, out } => out,
        Probe::TimedOut => return row.missing(format!("vswhere did not answer within {} s.", SLOW.as_secs())),
        Probe::Ran { out, .. } => return row.missing(format!("vswhere failed: {}", first_line(&out))),
        Probe::NotFound | Probe::Failed(_) => return row.missing(format!("Could not run vswhere. {pick}")),
    };
    let Some(found) = parse_vswhere(&out) else {
        return row.missing(format!("Visual Studio is installed, but without the C++ build tools. {pick}"));
    };
    if !windows_sdk_present() {
        return ToolRow {
            version: found.version,
            ..row.missing(
                "The C++ tools are installed, but no Windows 10/11 SDK. In the Visual Studio Installer, \
                 modify the installation and add a Windows SDK.",
            )
        };
    }
    ToolRow { version: found.version, detail: Some(found.path), action: None, ..row }
}

#[derive(Debug, PartialEq)]
pub struct VsInstall {
    pub path: String,
    pub version: Option<String>,
}

/// The first install in `vswhere -format json`, with its display version.
pub fn parse_vswhere(json: &str) -> Option<VsInstall> {
    let list: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    let first = list.as_array()?.first()?;
    let path = first.get("installationPath")?.as_str()?.to_string();
    let version = first
        .pointer("/catalog/productDisplayVersion")
        .or_else(|| first.get("installationVersion"))
        .and_then(|v| v.as_str())
        .map(|v| {
            let name = first.get("displayName").and_then(|n| n.as_str());
            match name {
                Some(n) => format!("{n} {v}"),
                None => v.to_string(),
            }
        });
    Some(VsInstall { path, version })
}

/// `kernel32.lib` under some Windows 10/11 SDK: without one the linker is
/// found and every link fails.
#[cfg(windows)]
fn windows_sdk_present() -> bool {
    let kits = std::env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"))
        .join(r"Windows Kits\10\Lib");
    std::fs::read_dir(kits)
        .map(|dirs| dirs.flatten().any(|d| d.path().join(r"um\x64\kernel32.lib").is_file()))
        .unwrap_or(false)
}

fn check_wasm_target(rustup: bool) -> ToolRow {
    let row = ToolRow::new(
        "wasm32_target",
        "wasm32-unknown-unknown target",
        ToolGroup::Web,
        "Web builds, and project viewports with Rust packages",
    );
    if !rustup {
        return row.missing("Needs rustup to install.").download(RUSTUP_URL, "Get rustup");
    }
    match probe(Path::new("rustup"), &["target", "list", "--installed"], SLOW) {
        Probe::Ran { success: true, out } if has_target(&out, "wasm32-unknown-unknown") => {
            ToolRow { version: Some("installed".into()), ..row }
        }
        Probe::Ran { success: true, .. } => {
            row.missing("Not installed for the default toolchain.").install(INSTALL_WASM32_TARGET, "Install")
        }
        Probe::TimedOut => row
            .missing(format!("rustup did not answer within {} s.", SLOW.as_secs()))
            .install(INSTALL_WASM32_TARGET, "Install"),
        Probe::Ran { out, .. } => row.missing(format!("rustup failed: {}", first_line(&out))).install(INSTALL_WASM32_TARGET, "Install"),
        Probe::NotFound | Probe::Failed(_) => row.missing("Could not run rustup.").download(RUSTUP_URL, "Get rustup"),
    }
}

/// Whether `rustup target list --installed` lists `target`.
pub fn has_target(listing: &str, target: &str) -> bool {
    listing.lines().any(|l| l.trim() == target)
}

fn check_wasm_pack(cargo: bool) -> ToolRow {
    let row = ToolRow::new("wasm_pack", "wasm-pack", ToolGroup::Web, "Web builds, and project viewports with Rust packages");
    let (row, _) = version_row(row, "wasm-pack", &["--version"]);
    if row.status == ToolStatus::Ok {
        return row;
    }
    if cargo {
        ToolRow { help_url: Some(WASM_PACK_URL.into()), ..row.install(INSTALL_WASM_PACK, "Install") }
    } else {
        row.download(WASM_PACK_URL, "Download")
    }
}

fn check_engine(located: &Result<PathBuf, String>) -> ToolRow {
    let row = ToolRow::new("engine_source", "Engine source", ToolGroup::Core, "Every game build");
    match located {
        Ok(path) => ToolRow { detail: Some(path.display().to_string()), ..row },
        Err(e) => row.missing(e.clone()).download(ENGINE_REPO_URL, "Get the source"),
    }
}

fn check_cargo_ndk(cargo: bool) -> ToolRow {
    let row = ToolRow::new("cargo_ndk", "cargo-ndk", ToolGroup::Optional, "Android builds only");
    // `cargo ndk --version`: a cargo subcommand, so cargo itself must run.
    if !cargo {
        return row.missing("Needs cargo.").download(CARGO_NDK_URL, "Download");
    }
    let (row, _) = version_row(row, "cargo", &["ndk", "--version"]);
    if row.status == ToolStatus::Ok {
        return row;
    }
    ToolRow {
        detail: Some("Not installed. Install it with `cargo install cargo-ndk`.".into()),
        ..row.download(CARGO_NDK_URL, "Download")
    }
}

fn check_android_sdk() -> ToolRow {
    let row = ToolRow::new("android_sdk", "Android SDK and NDK", ToolGroup::Optional, "Android builds only");
    match jengine_builder::android::find_sdk() {
        Ok(sdk) => {
            let ndk = sdk.ndk.file_name().map(|n| n.to_string_lossy().into_owned());
            ToolRow { version: ndk.map(|n| format!("NDK {n}")), detail: Some(sdk.root.display().to_string()), ..row }
        }
        Err(e) => row.missing(format!("{e:#}")).download(ANDROID_STUDIO_URL, "Get Android Studio"),
    }
}

#[cfg(windows)]
fn check_wsl() -> ToolRow {
    let row = ToolRow::new("wsl", "WSL", ToolGroup::Optional, "Linux builds from Windows only");
    let wsl = Path::new("wsl.exe");
    // `--version` is the Store WSL's; an inbox WSL only knows `--status`.
    match probe(wsl, &["--version"], QUICK) {
        Probe::Ran { success: true, out } => {
            ToolRow { version: parse_version(&out).map(|v| v.to_string()).or_else(|| Some(first_line(&out))), ..row }
        }
        Probe::NotFound => row.missing("Not installed.").download(WSL_URL, "How to install"),
        Probe::TimedOut => row.missing(format!("wsl.exe did not answer within {} s.", QUICK.as_secs())).download(WSL_URL, "How to install"),
        _ => match probe(wsl, &["--status"], QUICK) {
            Probe::Ran { success: true, .. } => ToolRow { version: Some("installed".into()), ..row },
            _ => row
                .missing("Not installed. Run `wsl --install -d Ubuntu` in an administrator terminal.")
                .download(WSL_URL, "How to install"),
        },
    }
}

fn check_wasm_opt() -> ToolRow {
    let row = ToolRow::new("wasm_opt", "wasm-opt", ToolGroup::Optional, "Optimising web builds (informational)");
    match jengine_builder::wasm_opt::find_binary() {
        Some(path) => {
            let version = match probe(&path, &["--version"], QUICK) {
                // `wasm-opt version 117 (version_117)`: binaryen numbers releases.
                Probe::Ran { success: true, out } => out
                    .split_whitespace()
                    .find(|w| w.chars().all(|c| c.is_ascii_digit()))
                    .map(|n| format!("version {n}"))
                    .or_else(|| Some(first_line(&out))),
                _ => None,
            };
            ToolRow { version, detail: Some(path.display().to_string()), ..row }
        }
        None => row
            .missing("Not found; web builds use wasm-pack's own copy.")
            .download(BINARYEN_URL, "Download"),
    }
}

/// Every check, the slow ones side by side.
pub fn run_checks() -> BuildToolsReport {
    let engine = jengine_builder::engine_root::locate();
    let min = min_rust(engine.as_ref().ok().map(PathBuf::as_path));

    let (rustc, cargo, (rustup, rustup_ok), msvc, wsl, android, wasm_opt) = std::thread::scope(|s| {
        let rustc = s.spawn(|| check_rust("rustc", "Rust compiler (rustc)", "rustc", min));
        let cargo = s.spawn(|| check_rust("cargo", "Cargo", "cargo", min));
        let rustup = s.spawn(check_rustup);
        #[cfg(windows)]
        let msvc = Some(s.spawn(check_msvc));
        #[cfg(not(windows))]
        let msvc: Option<std::thread::ScopedJoinHandle<'_, ToolRow>> = None;
        #[cfg(windows)]
        let wsl = Some(s.spawn(check_wsl));
        #[cfg(not(windows))]
        let wsl: Option<std::thread::ScopedJoinHandle<'_, ToolRow>> = None;
        let android = s.spawn(check_android_sdk);
        let wasm_opt = s.spawn(check_wasm_opt);
        let join = |h: std::thread::ScopedJoinHandle<'_, ToolRow>, id: &str| {
            h.join().unwrap_or_else(|_| ToolRow::new(id, id, ToolGroup::Optional, "").missing("The check panicked."))
        };
        (
            join(rustc, "rustc"),
            join(cargo, "cargo"),
            rustup.join().unwrap_or_else(|_| (ToolRow::new("rustup", "rustup", ToolGroup::Core, "").missing("The check panicked."), false)),
            msvc.map(|h| join(h, "msvc")),
            wsl.map(|h| join(h, "wsl")),
            join(android, "android_sdk"),
            join(wasm_opt, "wasm_opt"),
        )
    });
    let cargo_ok = cargo.status != ToolStatus::Missing;

    // These need to know whether rustup and cargo run, so they go second.
    let (wasm_target, wasm_pack, cargo_ndk) = std::thread::scope(|s| {
        let t = s.spawn(|| check_wasm_target(rustup_ok));
        let p = s.spawn(|| check_wasm_pack(cargo_ok));
        let n = s.spawn(|| check_cargo_ndk(cargo_ok));
        (
            t.join().unwrap_or_else(|_| ToolRow::new("wasm32_target", "wasm32", ToolGroup::Web, "").missing("The check panicked.")),
            p.join().unwrap_or_else(|_| ToolRow::new("wasm_pack", "wasm-pack", ToolGroup::Web, "").missing("The check panicked.")),
            n.join().unwrap_or_else(|_| ToolRow::new("cargo_ndk", "cargo-ndk", ToolGroup::Optional, "").missing("The check panicked.")),
        )
    });

    let mut rows = vec![rustc, cargo, rustup];
    rows.extend(msvc);
    rows.push(check_engine(&engine));
    rows.push(wasm_target);
    rows.push(wasm_pack);
    rows.push(android);
    rows.push(cargo_ndk);
    rows.extend(wsl);
    rows.push(wasm_opt);
    // Every "get rustup" button goes to the same page; the platform's
    // installer is the download, the page explains it.
    for row in &mut rows {
        if matches!(&row.action, Some(ToolAction::Download { url, .. }) if url == RUSTUP_URL) && row.help_url.is_none() {
            row.help_url = Some(RUSTUP_PAGE.into());
        }
    }
    BuildToolsReport { os: std::env::consts::OS.to_string(), rows }
}

/// What a Help > Check Build Tools page shows.
///
/// `async` for the reason the build commands are: a synchronous command
/// runs on the thread pumping WebView2's messages, and these checks start a
/// dozen processes.
#[tauri::command(async)]
pub fn check_build_tools() -> BuildToolsReport {
    run_checks()
}

// ─────────────────────────────── installing ──────────────────────────────

/// The Build panel's event, as `commands/build.rs` sends it.
const BUILD_OUTPUT_EVENT: &str = "build-output";

#[derive(Clone, Serialize)]
struct BuildLine<'a> {
    text: &'a str,
    replaces_previous: bool,
}

/// Clears the builder's process-wide sink when the install ends, however it
/// ends.
struct InstallStream;

impl Drop for InstallStream {
    fn drop(&mut self) {
        jengine_builder::progress::set_sink(None);
    }
}

fn stream_to_build_panel(app: &tauri::AppHandle) -> InstallStream {
    let handle = app.clone();
    jengine_builder::progress::set_sink(Some(Arc::new(move |line: &str, replaces_previous: bool| {
        let _ = handle.emit(BUILD_OUTPUT_EVENT, BuildLine { text: line, replaces_previous });
    })));
    InstallStream
}

/// A tool by name if it runs from PATH, else its `~/.cargo/bin` copy (one
/// installed since the editor started).
fn tool_path(name: &str) -> PathBuf {
    match probe(Path::new(name), &["--version"], QUICK) {
        Probe::NotFound => off_path(name).unwrap_or_else(|| PathBuf::from(name)),
        _ => PathBuf::from(name),
    }
}

/// The command an install id runs.
pub fn install_command(id: &str) -> Result<(PathBuf, Vec<&'static str>), String> {
    match id {
        INSTALL_WASM32_TARGET => Ok((tool_path("rustup"), vec!["target", "add", "wasm32-unknown-unknown"])),
        INSTALL_WASM_PACK => Ok((tool_path("cargo"), vec!["install", "wasm-pack", "--locked"])),
        other => Err(format!("'{other}' is not something the editor can install")),
    }
}

/// Install one tool, its output streamed into the Build panel.
#[tauri::command(async)]
pub fn install_build_tool(app: tauri::AppHandle, id: String) -> Result<String, String> {
    let (program, args) = install_command(&id)?;
    let _stream = stream_to_build_panel(&app);
    let mut cmd = Command::new(&program);
    cmd.args(&args);
    let what = program.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| id.clone());
    let status = jengine_builder::progress::run_streaming(quiet(&mut cmd), &what).map_err(|e| {
        let msg = format!("Install failed: {e:#}");
        jengine_builder::progress::emit(&msg);
        msg
    })?;
    if status.success() {
        Ok(format!("Installed {id}"))
    } else {
        let msg = format!("Install failed: {what} exited with {status}");
        jengine_builder::progress::emit(&msg);
        Err(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_read_from_version_lines() {
        assert_eq!(parse_version("rustc 1.97.1 (8bab26f4f 2026-07-14)"), Some(Version(1, 97, 1)));
        assert_eq!(parse_version("cargo 1.97.0 (a1b2c3d4e 2026-06-30)"), Some(Version(1, 97, 0)));
        assert_eq!(parse_version("wasm-pack 0.13.1"), Some(Version(0, 13, 1)));
        assert_eq!(parse_version("rustc 1.99.0-nightly (abc 2026-09-01)"), Some(Version(1, 99, 0)));
        assert_eq!(parse_version("WSL version: 2.4.13.0\nKernel version: 5.15"), Some(Version(2, 4, 13)));
        assert_eq!(parse_version("no numbers here"), None);
        assert_eq!(parse_dotted("1.97"), Some(Version(1, 97, 0)));
    }

    #[test]
    fn an_older_rust_is_outdated() {
        let min = parse_dotted("1.97").unwrap();
        assert!(is_outdated(Version(1, 96, 9), min));
        assert!(is_outdated(Version(0, 99, 0), min));
        assert!(!is_outdated(Version(1, 97, 0), min));
        assert!(!is_outdated(Version(1, 97, 1), min));
        assert!(!is_outdated(Version(2, 0, 0), min));
    }

    #[test]
    fn the_minimum_comes_from_the_workspace_manifest() {
        let manifest = "[workspace.package]\nedition = \"2024\"\nrust-version = \"1.97\"\n";
        assert_eq!(rust_version_in(manifest), Some(Version(1, 97, 0)));
        assert_eq!(rust_version_in("rust-version.workspace = true\n"), None);
        // The real one, so the page and the engine cannot disagree.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let real = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(rust_version_in(&real).is_some(), "the root Cargo.toml has a rust-version");
    }

    #[test]
    fn targets_are_matched_whole() {
        let listing = "x86_64-pc-windows-msvc\nwasm32-unknown-unknown\n";
        assert!(has_target(listing, "wasm32-unknown-unknown"));
        assert!(!has_target("wasm32-unknown-emscripten\n", "wasm32-unknown-unknown"));
    }

    #[test]
    fn vswhere_json_gives_a_path_and_version() {
        let json = r#"[{"installationPath":"C:\\BuildTools","installationVersion":"17.11.1","displayName":"Visual Studio Build Tools 2022","catalog":{"productDisplayVersion":"17.11.1"}}]"#;
        let found = parse_vswhere(json).unwrap();
        assert_eq!(found.path, r"C:\BuildTools");
        assert_eq!(found.version.as_deref(), Some("Visual Studio Build Tools 2022 17.11.1"));
        assert_eq!(parse_vswhere("[]"), None);
    }

    #[test]
    fn utf16_output_is_decoded() {
        let bytes: Vec<u8> = "WSL version: 2.4.13.0".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode(&bytes), "WSL version: 2.4.13.0");
        assert_eq!(decode(b"plain"), "plain");
    }

    #[test]
    fn a_missing_optional_row_is_optional_missing() {
        let core = ToolRow::new("a", "A", ToolGroup::Core, "x").missing("gone");
        assert_eq!(core.status, ToolStatus::Missing);
        let opt = ToolRow::new("b", "B", ToolGroup::Optional, "x").missing("gone");
        assert_eq!(opt.status, ToolStatus::OptionalMissing);
    }

    #[test]
    fn only_known_ids_install() {
        assert!(install_command(INSTALL_WASM32_TARGET).is_ok());
        assert!(install_command(INSTALL_WASM_PACK).unwrap().1.contains(&"--locked"));
        assert!(install_command("rm -rf /").is_err());
    }

    #[test]
    fn a_timed_out_tool_is_killed() {
        // A program that does not exit: a shell that waits.
        #[cfg(windows)]
        let (prog, args) = ("powershell.exe", vec!["-NoProfile", "-Command", "Start-Sleep 30"]);
        #[cfg(not(windows))]
        let (prog, args) = ("sleep", vec!["30"]);
        let start = Instant::now();
        let r = probe(Path::new(prog), &args, Duration::from_millis(300));
        assert!(matches!(r, Probe::TimedOut), "{r:?}");
        assert!(start.elapsed() < Duration::from_secs(10));
        assert!(matches!(probe(Path::new("no-such-tool-jengine"), &[], QUICK), Probe::NotFound));
    }

    #[test]
    fn every_row_has_the_shape_the_page_reads() {
        let report = run_checks();
        let ids: Vec<&str> = report.rows.iter().map(|r| r.id.as_str()).collect();
        for id in ["rustc", "cargo", "rustup", "engine_source", "wasm32_target", "wasm_pack", "android_sdk", "cargo_ndk", "wasm_opt"] {
            assert!(ids.contains(&id), "{id} missing from {ids:?}");
        }
        #[cfg(windows)]
        assert!(ids.contains(&"msvc") && ids.contains(&"wsl"));
        for row in &report.rows {
            assert!(!row.name.is_empty() && !row.needed_for.is_empty(), "{row:?}");
            if row.status != ToolStatus::Ok {
                assert!(row.detail.is_some(), "a row that is not ok says why: {row:?}");
            }
            if row.group == ToolGroup::Optional {
                assert!(row.status != ToolStatus::Missing, "optional rows are optional_missing: {row:?}");
            }
            if let Some(ToolAction::Install { id, .. }) = &row.action {
                assert!(install_command(id).is_ok());
            }
        }
        let json = serde_json::to_value(&report).unwrap();
        let first = &json["rows"][0];
        for key in ["id", "name", "status", "group", "version", "needed_for", "detail", "action", "help_url"] {
            assert!(first.get(key).is_some(), "{key} in {first}");
        }
    }
}
