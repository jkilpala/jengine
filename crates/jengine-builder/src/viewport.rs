//! Building an editor viewport that carries a project's own Rust packages.
//!
//! The editor imports one WASM module from a fixed place, so a project's
//! package cannot be linked into it ahead of time. This generates the wrapper
//! crate (see [`crate::packages::generate_project_viewport`]), builds it into
//! the project's own `.jengine/viewport/pkg/`, and copies that into the
//! editor's module directory.
//!
//! **Each project keeps its build.** Opening a project whose cached module is
//! still current is a copy and a reload ([`switch`]), not a compile. "Current"
//! is decided by Cargo's own dep-info: every local source the module was
//! compiled from (core, the viewport, the packages, the shaders it includes),
//! and none of them newer than the build. Registry crates are not listed and
//! need not be - they do not change under a version.
//!
//! The whole thing lives here rather than in a shell script so the CLI, the
//! `build-project-viewport.ps1` wrapper and the editor's own "Rebuild viewport"
//! action all run the *same* code — a package that behaves differently
//! depending on how it was built is precisely the confusion this feature
//! exists to remove.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The stamp's file name, beside the module in the editor and in a cache.
const STAMP_FILE: &str = ".jengine-viewport.json";

/// The copy of Cargo's dep-info kept beside a cached module.
const SOURCES_FILE: &str = "sources.d";

/// Records which project's packages the built viewport module carries.
///
/// Written beside the module. The editor's module directory is shared, so
/// without this a module built for another project is indistinguishable from
/// a current one — and the symptom is a package that silently does nothing,
/// which is the exact failure mode this feature removes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ViewportStamp {
    /// Absolute path of the project whose packages are linked in.
    #[serde(default)]
    pub project: String,
    /// Names of the Rust packages linked in.
    #[serde(default)]
    pub packages: Vec<String>,
    /// True for a plain `build-viewport.ps1` build with no project packages.
    #[serde(default)]
    pub stock: bool,
    /// When the build *started*, in milliseconds since the epoch. A source
    /// changed after this - even during the build - makes the cache stale.
    /// Zero for a stamp that predates caching, which is never reused.
    #[serde(default)]
    pub built_at_ms: u64,
}

/// What the editor needs to decide whether to nag about a rebuild.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportStatus {
    /// The loaded module does not match this project's packages.
    pub needs_rebuild: bool,
    /// Human-readable reason, empty when nothing is wrong.
    pub reason: String,
    /// Packages the loaded module carries.
    pub linked: Vec<String>,
    /// Packages this project declares.
    pub wanted: Vec<String>,
    /// Project the loaded module was built for, empty for a stock build.
    pub built_for: String,
    /// This project has a current build of its own, so [`switch`] can load it
    /// without compiling. Only meaningful when `needs_rebuild` is set.
    #[serde(default)]
    pub cached: bool,
}

/// The two files a viewport module is.
pub const MODULE_FILES: [&str; 2] = ["jengine_editor_viewport.js", "jengine_editor_viewport_bg.wasm"];

/// Where an **installed** editor keeps its viewport modules.
///
/// An editor run from a checkout (`cargo tauri dev`) imports its module from
/// `editor/src/lib/viewport-wasm/` through Vite, so loading a project's build
/// is a copy there. An installed editor has no source tree: its frontend is
/// built with the module left out and imports it at run time from a URL the
/// editor serves - from `active` when a project's module is there, else from
/// `stock`, the module it shipped with. Set once at start by such an editor
/// ([`use_module_dirs`]); unset, everything is the checkout's.
#[derive(Debug, Clone)]
pub struct ModuleDirs {
    /// Writable: the project module loaded now, with its stamp.
    pub active: PathBuf,
    /// Read-only: the stock module and its stamp, shipped with the editor.
    pub stock: PathBuf,
}

static MODULE_DIRS: std::sync::OnceLock<ModuleDirs> = std::sync::OnceLock::new();

/// Serve viewport modules from `dirs` rather than the checkout's editor
/// source - what an installed editor calls once, before any build.
pub fn use_module_dirs(dirs: ModuleDirs) {
    let _ = MODULE_DIRS.set(dirs);
}

/// The directory a module file should be read from now: the active one when
/// it holds a whole module, else the stock one. `None` in a checkout's editor.
pub fn serving_dir() -> Option<PathBuf> {
    MODULE_DIRS.get().map(ModuleDirs::serving)
}

impl ModuleDirs {
    /// The active directory when it holds a whole module, else the stock one.
    pub fn serving(&self) -> PathBuf {
        if has_module(&self.active) { self.active.clone() } else { self.stock.clone() }
    }
}

fn has_module(dir: &Path) -> bool {
    MODULE_FILES.iter().all(|f| dir.join(f).is_file())
}

/// The editor's module directory, which its source imports by a fixed path -
/// or an installed editor's active directory.
fn editor_dir(engine_root: &Path) -> PathBuf {
    match MODULE_DIRS.get() {
        Some(dirs) => dirs.active.clone(),
        None => engine_root.join("editor").join("src").join("lib").join("viewport-wasm"),
    }
}

/// Path of the stamp beside the editor's viewport module.
fn stamp_path(engine_root: &Path) -> PathBuf {
    match MODULE_DIRS.get() {
        Some(_) => serving_dir().expect("module dirs are set").join(STAMP_FILE),
        None => editor_dir(engine_root).join(STAMP_FILE),
    }
}

/// Go back to the stock module: in an installed editor that is taking the
/// project's module out of the active directory, so the stock one is served
/// (a stock *stamp* beside a project's module would vouch for the wrong
/// thing); in a checkout it is only the stamp, as it always was.
fn mark_stock(engine_root: &Path) -> Result<()> {
    if let Some(dirs) = MODULE_DIRS.get() {
        for f in MODULE_FILES.iter().copied().chain([STAMP_FILE]) {
            let path = dirs.active.join(f);
            if path.exists() {
                std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            }
        }
        return Ok(());
    }
    write_stamp(&stamp_path(engine_root), &ViewportStamp { stock: true, ..Default::default() })
}

/// Where a project's own built viewport is kept.
pub fn cache_dir(project_root: &Path) -> PathBuf {
    project_root
        .join(".jengine")
        .join(crate::packages::PROJECT_VIEWPORT_DIR)
        .join("pkg")
}

/// The Rust package names a project declares, sorted for comparison.
fn wanted_packages(project_root: &Path) -> Vec<String> {
    let mut names: Vec<String> = crate::packages::resolve_packages(project_root)
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.package_type == jengine_core::package::manifest::PackageType::Full)
        .map(|p| p.name)
        .collect();
    names.sort();
    names
}

fn read_stamp_at(path: &Path) -> Option<ViewportStamp> {
    let text = std::fs::read_to_string(path).ok()?;
    // Tolerate a BOM: an earlier build wrote the stamp with PowerShell's
    // `Set-Content -Encoding utf8`, which emits one, and serde_json rejects it.
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// Read the stamp of the editor's module, if it has one.
pub fn read_stamp(engine_root: &Path) -> Option<ViewportStamp> {
    // An installed editor serving the module it shipped with: that one is
    // stock by definition. Its stamp is a dotfile, which Tauri's macOS bundler
    // leaves out of the app's resources.
    if let Some(dirs) = MODULE_DIRS.get()
        && dirs.serving() == dirs.stock
    {
        return Some(read_stamp_at(&dirs.stock.join(STAMP_FILE)).unwrap_or(ViewportStamp { stock: true, ..Default::default() }));
    }
    read_stamp_at(&stamp_path(engine_root))
}

/// Whether the editor's current viewport module matches `project_root`.
///
/// A project with no Rust packages is satisfied by any viewport — the stock
/// module already carries every built-in package — so it never asks for a
/// rebuild, and switching from a package-having project to one without does
/// not nag.
pub fn status(engine_root: &Path, project_root: &Path) -> ViewportStatus {
    let wanted = wanted_packages(project_root);
    let stamp = read_stamp(engine_root);
    let linked = stamp.as_ref().map(|s| {
        let mut v = s.packages.clone();
        v.sort();
        v
    });
    let built_for = stamp.as_ref().map(|s| s.project.clone()).unwrap_or_default();

    if wanted.is_empty() {
        return ViewportStatus {
            needs_rebuild: false,
            reason: String::new(),
            linked: linked.unwrap_or_default(),
            wanted,
            built_for,
            cached: false,
        };
    }

    let (needs_rebuild, reason) = match &linked {
        // No stamp at all: the module predates stamping, so it cannot be
        // assumed to carry anything.
        None => (
            true,
            format!(
                "The editor viewport has no package stamp, so it does not carry \
                 this project's package(s): {}.",
                wanted.join(", ")
            ),
        ),
        Some(linked) if linked != &wanted => {
            let built = if built_for.is_empty() {
                "the stock viewport".to_string()
            } else {
                format!(
                    "'{}'",
                    Path::new(&built_for)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| built_for.clone())
                )
            };
            (
                true,
                format!(
                    "The editor viewport was built for {built} and carries [{}], \
                     but this project needs [{}].",
                    linked.join(", "),
                    wanted.join(", ")
                ),
            )
        }
        Some(_) => (false, String::new()),
    };

    let cached = needs_rebuild && cache_problem(engine_root, project_root, &wanted).is_none();
    ViewportStatus {
        needs_rebuild,
        reason,
        linked: linked.unwrap_or_default(),
        wanted,
        built_for,
        cached,
    }
}

/// Why this project's cached module cannot be loaded as it is, or `None` when
/// it can: it exists, carries `wanted`, and nothing it was compiled from has
/// changed since.
fn cache_problem(engine_root: &Path, project_root: &Path, wanted: &[String]) -> Option<String> {
    let cache = cache_dir(project_root);
    let Some(stamp) = read_stamp_at(&cache.join(STAMP_FILE)) else {
        return Some("no cached build".into());
    };
    let mut packages = stamp.packages.clone();
    packages.sort();
    if packages != wanted {
        return Some(format!(
            "the cached build carries [{}], the project now needs [{}]",
            packages.join(", "),
            wanted.join(", ")
        ));
    }
    if stamp.built_at_ms == 0 {
        return Some("the cached build has no build time".into());
    }
    for file in MODULE_FILES {
        if !cache.join(file).is_file() {
            return Some(format!("the cached build is missing {file}"));
        }
    }
    let Ok(dep_info) = std::fs::read_to_string(cache.join(SOURCES_FILE)) else {
        return Some("the cached build has no source list".into());
    };
    let mut sources = dep_info_sources(&dep_info);
    if sources.is_empty() {
        return Some("the cached build's source list is empty".into());
    }
    // A lockfile change can move a registry crate, which dep-info never lists.
    let lock = engine_root.join("Cargo.lock");
    if lock.is_file() {
        sources.push(lock);
    }

    let built_at = UNIX_EPOCH + Duration::from_millis(stamp.built_at_ms);
    for source in &sources {
        match std::fs::metadata(source).and_then(|m| m.modified()) {
            Ok(modified) if modified <= built_at => {}
            Ok(_) => return Some(format!("{} changed since it was built", source.display())),
            // Gone: deleted or renamed, which changes the build too.
            Err(_) => return Some(format!("{} is gone", source.display())),
        }
    }
    None
}

/// Every source file a Cargo dep-info (Makefile syntax) lists, deduplicated.
///
/// A rule is `target: dep dep ...`, a space in a path escaped as `\ `; Cargo
/// also writes an empty rule per dep, which adds nothing. The separator is a
/// colon *followed by a space*, so a Windows drive letter is not one.
pub fn dep_info_sources(text: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for line in text.lines() {
        let Some(at) = find_rule_colon(line) else { continue };
        let mut current = String::new();
        let mut chars = line[at + 1..].chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' if chars.peek() == Some(&' ') => {
                    current.push(' ');
                    chars.next();
                }
                c if c.is_whitespace() => {
                    if !current.is_empty() {
                        out.push(PathBuf::from(std::mem::take(&mut current)));
                    }
                }
                c => current.push(c),
            }
        }
        if !current.is_empty() {
            out.push(PathBuf::from(current));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Byte index of the colon ending a rule's target: one followed by a space or
/// the end of the line, and not preceded by an escaping backslash.
fn find_rule_colon(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    (0..bytes.len()).find(|&i| {
        bytes[i] == b':'
            && (i + 1 == bytes.len() || bytes[i + 1] == b' ')
            && (i == 0 || bytes[i - 1] != b'\\')
    })
}

/// Load this project's cached module into the editor if it is current.
///
/// Returns `false` - and touches nothing - when there is no current build, so
/// the caller falls back to [`build`]. On `true` the editor must reload: the
/// old module is already instantiated in the page.
pub fn switch(engine_root: &Path, project_root: &Path) -> Result<bool> {
    let wanted = wanted_packages(project_root);
    if wanted.is_empty() {
        return Ok(false);
    }
    if let Some(why) = cache_problem(engine_root, project_root, &wanted) {
        log::info!("Cached viewport for {} not used: {why}", project_root.display());
        return Ok(false);
    }
    activate(engine_root, &cache_dir(project_root))?;
    Ok(true)
}

/// Load the project's cached module if it is current, else build one.
/// What `run-editor.ps1` wants on launch: no compile when nothing changed.
pub fn ensure(engine_root: &Path, project_root: &Path) -> Result<Vec<String>> {
    if switch(engine_root, project_root)? {
        let names = wanted_packages(project_root);
        crate::progress::step(&format!(
            "Editor viewport loaded from this project's cached build ({}).",
            names.join(", ")
        ));
        return Ok(names);
    }
    build(engine_root, project_root)
}

/// Copy a built module (with its stamp) into the editor's module directory.
fn activate(engine_root: &Path, from: &Path) -> Result<()> {
    let to = editor_dir(engine_root);
    std::fs::create_dir_all(&to).with_context(|| format!("creating {}", to.display()))?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(from)
        .with_context(|| format!("reading {}", from.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // The source list is the cache's own; the rest are wasm-pack
            // extras that confuse Vite.
            !matches!(name, SOURCES_FILE | "package.json" | ".gitignore" | "README.md")
        })
        .collect();
    // The stamp last, so a copy cut short leaves the old stamp - which no
    // longer matches - rather than a new one vouching for a half-copied module.
    files.sort_by_key(|p| p.file_name().is_some_and(|n| n == STAMP_FILE));
    for file in files {
        let dest = to.join(file.file_name().expect("a file has a name"));
        std::fs::copy(&file, &dest)
            .with_context(|| format!("copying {} to {}", file.display(), dest.display()))?;
    }
    Ok(())
}

/// Generate the wrapper crate, build it into the project's cache and load it
/// into the editor.
///
/// Returns the package names linked in. Output streams through
/// [`crate::progress`], so the editor's Build panel shows the compile the same
/// way it shows a game build.
pub fn build(engine_root: &Path, project_root: &Path) -> Result<Vec<String>> {
    build_with(engine_root, project_root, crate::wasm_opt::viewport_level())
}

/// [`build`] at a given `wasm-opt` level rather than the editor preference.
pub fn build_with(engine_root: &Path, project_root: &Path, level: crate::wasm_opt::WasmOpt) -> Result<Vec<String>> {
    // Absolute before anything is derived from them: wasm-pack resolves
    // `--out-dir` against the crate it builds, so a relative project path
    // (`jengine viewport-build projects/x`) sent the module to
    // `<crate>/projects/x/...` - and the copy below then took the *previous*
    // build from the real cache and stamped it as fresh.
    let engine_root = &std::path::absolute(engine_root).context("making the engine path absolute")?;
    let project_root = &std::path::absolute(project_root).context("making the project path absolute")?;
    let packages = crate::packages::resolve_packages(project_root)
        .context("resolving the project's packages")?;

    let manifest = crate::packages::generate_project_viewport(project_root, engine_root, &packages)?;
    let Some(manifest) = manifest else {
        // No Rust packages: the stock viewport already has every built-in, so
        // there is nothing project-specific to build.
        crate::progress::step("No Rust packages in this project - the stock viewport is enough.");
        mark_stock(engine_root)?;
        return Ok(Vec::new());
    };

    let names: Vec<String> = packages
        .iter()
        .filter(|p| p.package_type == jengine_core::package::manifest::PackageType::Full)
        .map(|p| p.name.clone())
        .collect();

    crate::progress::step(&format!(
        "Building editor viewport with {} project package(s): {}...",
        names.len(),
        names.join(", ")
    ));

    let crate_dir = manifest
        .parent()
        .expect("generated manifest has a parent")
        .to_path_buf();
    let cache = cache_dir(project_root);
    // Taken before the compile, so a source saved while it runs counts as
    // newer than the build.
    let built_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // `--out-name` pins the emitted filenames to the stock module's, so the
    // editor's fixed import path keeps working while the wrapper crate has a
    // unique name (Cargo refuses a crate and its dependency sharing a name).
    let plan = crate::wasm_opt::Plan::new(level);
    let mut cmd = Command::new("wasm-pack");
    cmd.arg("build")
        .arg(&crate_dir)
        .args(["--target", "web", "--release", "--out-dir"])
        .arg(&cache)
        .args(["--out-name", "jengine_editor_viewport"])
        .args(plan.wasm_pack_args());
    let status = crate::progress::run_streaming(&mut cmd, "wasm-pack")
        .context("running wasm-pack. Is it installed and on PATH?")?;
    if !status.success() {
        anyhow::bail!("wasm-pack failed with status: {status}");
    }
    plan.optimize(&cache.join("jengine_editor_viewport_bg.wasm"))?;

    // Keep Cargo's list of what the module was compiled from, which is how a
    // later open decides whether this build is still current. Without it the
    // cache is simply never reused.
    let project_name = project_root.file_name().and_then(|s| s.to_str()).unwrap_or("game");
    let dep_info = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate_dir.join("target"))
        .join("wasm32-unknown-unknown")
        .join("release")
        .join(format!(
            "{}.d",
            format!("{}-viewport", crate::packages::slug(project_name)).replace('-', "_")
        ));
    if let Err(e) = std::fs::copy(&dep_info, cache.join(SOURCES_FILE)) {
        log::warn!(
            "No dep-info at {} ({e}) - this viewport build will not be reused.",
            dep_info.display()
        );
        let _ = std::fs::remove_file(cache.join(SOURCES_FILE));
    }

    write_stamp(
        &cache.join(STAMP_FILE),
        &ViewportStamp {
            // Verbatim-stripped: `canonicalize` returns a `\?\C:\...` path on
            // Windows, which is correct but unreadable in a banner that names
            // the project back to the user.
            project: crate::packages::display_path(
                &project_root
                    .canonicalize()
                    .unwrap_or_else(|_| project_root.to_path_buf()),
            ),
            packages: names.clone(),
            stock: false,
            built_at_ms,
        },
    )?;
    activate(engine_root, &cache)?;

    crate::progress::step(&format!(
        "Editor viewport rebuilt with: {}. Reload the editor to load it.",
        names.join(", ")
    ));
    Ok(names)
}

/// Build the **stock** viewport - the engine's own, with no project's packages
/// - into the editor at the editor preference's `wasm-opt` level, and stamp it
/// as stock. `build-viewport.ps1` / `.sh` and `run-editor` call this (through
/// `jengine viewport-build --stock`), so the level applies to every way the
/// viewport is built.
pub fn build_stock(engine_root: &Path) -> Result<()> {
    build_stock_with(engine_root, crate::wasm_opt::viewport_level())
}

/// [`build_stock`] at a given `wasm-opt` level.
pub fn build_stock_with(engine_root: &Path, level: crate::wasm_opt::WasmOpt) -> Result<()> {
    let engine_root = &std::path::absolute(engine_root).context("making the engine path absolute")?;
    if MODULE_DIRS.get().is_some() {
        anyhow::bail!("an installed editor's stock viewport is the one it shipped with, and is not rebuilt");
    }
    let out = editor_dir(engine_root);
    crate::progress::step(&format!("Building the stock editor viewport (wasm-opt: {})...", level.id()));
    let plan = crate::wasm_opt::Plan::new(level);
    let mut cmd = Command::new("wasm-pack");
    cmd.arg("build")
        .arg(engine_root.join("crates").join("jengine-editor-viewport"))
        .args(["--target", "web", "--release", "--out-dir"])
        .arg(&out)
        .args(plan.wasm_pack_args());
    let status = crate::progress::run_streaming(&mut cmd, "wasm-pack")
        .context("running wasm-pack. Is it installed and on PATH?")?;
    if !status.success() {
        anyhow::bail!("wasm-pack failed with status: {status}");
    }
    plan.optimize(&out.join("jengine_editor_viewport_bg.wasm"))?;
    // wasm-pack's extras confuse Vite.
    let _ = std::fs::remove_file(out.join("package.json"));
    let _ = std::fs::remove_file(out.join(".gitignore"));
    // The directory is shared with project builds, and the editor trusts the
    // stamp to say which packages the module carries - so a stock build must
    // always rewrite it.
    write_stamp(&stamp_path(engine_root), &ViewportStamp { stock: true, ..Default::default() })?;
    crate::progress::step("Stock editor viewport built.");
    Ok(())
}

/// Write a stamp as UTF-8 without a BOM, which `JSON.parse` requires.
fn write_stamp(path: &Path, stamp: &ViewportStamp) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let json = serde_json::to_string_pretty(stamp)?;
    std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_installed_editor_serves_the_stock_module_until_a_whole_project_module_is_active() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = ModuleDirs { active: tmp.path().join("active"), stock: tmp.path().join("stock") };
        assert_eq!(dirs.serving(), dirs.stock, "nothing active yet");
        std::fs::create_dir_all(&dirs.active).unwrap();
        std::fs::write(dirs.active.join(MODULE_FILES[0]), "glue").unwrap();
        assert_eq!(dirs.serving(), dirs.stock, "glue without its wasm is not a module");
        std::fs::write(dirs.active.join(MODULE_FILES[1]), "wasm").unwrap();
        assert_eq!(dirs.serving(), dirs.active);
    }
}
