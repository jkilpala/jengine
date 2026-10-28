//! How hard a WebAssembly module is optimised after it is compiled.
//!
//! wasm-pack runs binaryen's `wasm-opt -O` over every release module and
//! takes no level on its command line (only from a crate's `Cargo.toml`, and
//! the engine's crates are checked in). On the editor viewport - 16 MB of code
//! by Oct 2026 - that pass took fifteen minutes; `-O1` took twenty seconds for
//! a module 5% larger. So the level is a choice, and the builder runs
//! `wasm-opt` itself: wasm-pack is told `--no-opt`, and [`optimize`] runs the
//! chosen level afterwards.
//!
//! Two choices, kept apart on purpose:
//!
//! * **The editor viewport** - an editor preference (Project Settings ->
//!   Graphics -> Editor), machine-wide, stored in the editor's
//!   `editor-preferences.json`. [`viewport_level`] reads it, so the editor's
//!   *Rebuild viewport*, `jengine viewport-build` and `run-editor` agree.
//! * **A game's web build** - per project, beside the other WASM size options
//!   in Build Settings, passed to `web::build_web`.
//!
//! The `wasm-opt` used is the newest one at hand: `JENGINE_WASM_OPT`, else one
//! on `PATH` (a binaryen installed by hand is usually newer and faster than
//! the one wasm-pack pins), else the copy wasm-pack downloaded into its cache.
//! With none anywhere, wasm-pack is left to optimise at its own `-O` - which
//! downloads binaryen - so the chosen level applies from the next build.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The levels a build can ask for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WasmOpt {
    /// No `wasm-opt` pass: the fastest build, the largest module.
    Off,
    /// `-O1`: seconds rather than minutes, a module ~5% larger raw - and
    /// the smallest gzipped or brotli'd, so the smallest download
    /// ([`WasmOpt::WEB_BUILD_DEFAULT`]).
    Fast,
    /// `-O`: what wasm-pack has always done, and the editor viewport's
    /// default.
    #[default]
    Optimized,
    /// `-Oz`: the smallest module, about as slow as `Optimized`.
    Size,
    /// `-O3`: the most speed-oriented passes, the slowest build.
    Max,
}

impl WasmOpt {
    pub const ALL: [WasmOpt; 5] = [WasmOpt::Off, WasmOpt::Fast, WasmOpt::Optimized, WasmOpt::Size, WasmOpt::Max];

    /// The level a game's web build gets when its project names none:
    /// `Fast`. Measured on the empty project (`docs/build-sizes.md`), `-O1`
    /// gives the smallest download at every Cargo profile - gzip 2-7% and
    /// brotli ~8% under `-O`, `-Oz` and `-O3`, which only shrink the raw
    /// module - in seconds instead of minutes, and runs the CPU benchmark
    /// (`jengine-benchmark-cpu`) within noise of `-O` and ~3% behind `-O3`.
    /// The editor viewport is not downloaded and keeps [`WasmOpt::default`].
    pub const WEB_BUILD_DEFAULT: WasmOpt = WasmOpt::Fast;

    /// The `wasm-opt` flag, or `None` for no pass.
    pub fn flag(self) -> Option<&'static str> {
        match self {
            WasmOpt::Off => None,
            WasmOpt::Fast => Some("-O1"),
            WasmOpt::Optimized => Some("-O"),
            WasmOpt::Size => Some("-Oz"),
            WasmOpt::Max => Some("-O3"),
        }
    }

    /// The name used in files and on the command line.
    pub fn id(self) -> &'static str {
        match self {
            WasmOpt::Off => "off",
            WasmOpt::Fast => "fast",
            WasmOpt::Optimized => "optimized",
            WasmOpt::Size => "size",
            WasmOpt::Max => "max",
        }
    }

    pub fn parse(s: &str) -> Option<WasmOpt> {
        WasmOpt::ALL.into_iter().find(|l| l.id() == s.trim().to_ascii_lowercase())
    }
}

/// The editor's identifier (`tauri.conf.json`), which names its data folder.
const EDITOR_IDENTIFIER: &str = "com.jengine.editor";
/// The editor's machine-wide preferences file (`commands/editor_prefs.rs`).
const EDITOR_PREFS_FILE: &str = "editor-preferences.json";

/// Where the editor keeps its preferences: Tauri's `app_data_dir`, which is
/// the platform's data folder joined with the identifier.
pub fn editor_data_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    };
    base.map(|b| b.join(EDITOR_IDENTIFIER))
}

/// The level the editor viewport is built at: the editor preference
/// `viewport_wasm_opt`, else the default.
pub fn viewport_level() -> WasmOpt {
    editor_data_dir()
        .map(|d| d.join(EDITOR_PREFS_FILE))
        .and_then(|p| viewport_level_in(&p))
        .unwrap_or_default()
}

/// [`viewport_level`] from a given preferences file.
pub fn viewport_level_in(prefs_file: &Path) -> Option<WasmOpt> {
    let text = std::fs::read_to_string(prefs_file).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
    serde_json::from_value(doc.get("viewport_wasm_opt")?.clone()).ok()
}

/// The `wasm-opt` to run, newest first - see the module docs.
pub fn find_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("JENGINE_WASM_OPT").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    find_binary_in(&path_dirs, &wasm_pack_caches())
}

/// [`find_binary`] over given folders: each `PATH` folder, then each
/// wasm-pack cache (its `wasm-opt-*/bin/`, the most recently written first).
pub fn find_binary_in(path_dirs: &[PathBuf], caches: &[PathBuf]) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "wasm-opt.exe" } else { "wasm-opt" };
    if let Some(p) = path_dirs.iter().map(|d| d.join(exe)).find(|p| p.is_file()) {
        return Some(p);
    }
    let mut cached: Vec<(std::time::SystemTime, PathBuf)> = caches
        .iter()
        .filter_map(|c| std::fs::read_dir(c).ok())
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("wasm-opt-"))
        .map(|e| e.path().join("bin").join(exe))
        .filter(|p| p.is_file())
        .map(|p| (std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
        .collect();
    cached.sort();
    cached.pop().map(|(_, p)| p)
}

/// Where wasm-pack keeps the tools it downloads.
fn wasm_pack_caches() -> Vec<PathBuf> {
    if let Some(c) = std::env::var_os("WASM_PACK_CACHE") {
        return vec![PathBuf::from(c)];
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    };
    base.map(|b| vec![b.join(".wasm-pack")]).unwrap_or_default()
}

/// What a build does about optimisation, decided before wasm-pack runs.
#[derive(Debug, Clone)]
pub struct Plan {
    pub level: WasmOpt,
    /// The `wasm-opt` the builder runs afterwards; `None` when the level is
    /// `Off`, or when no binary exists yet and wasm-pack optimises instead.
    pub binary: Option<PathBuf>,
    /// Whether wasm-pack is told `--no-opt`.
    pub no_opt: bool,
}

impl Plan {
    pub fn new(level: WasmOpt) -> Plan {
        Plan::with_binary(level, if level == WasmOpt::Off { None } else { find_binary() })
    }

    pub fn with_binary(level: WasmOpt, binary: Option<PathBuf>) -> Plan {
        match (level, binary) {
            (WasmOpt::Off, _) => Plan { level, binary: None, no_opt: true },
            (_, Some(b)) => Plan { level, binary: Some(b), no_opt: true },
            // No wasm-opt anywhere: wasm-pack's own `-O` downloads one.
            (_, None) => Plan { level, binary: None, no_opt: false },
        }
    }

    /// The arguments wasm-pack gets for this plan.
    pub fn wasm_pack_args(&self) -> &'static [&'static str] {
        if self.no_opt { &["--no-opt"] } else { &[] }
    }

    /// Optimise `wasm` in place, as the plan says. Logs the time and the
    /// size before and after through [`crate::progress`].
    pub fn optimize(&self, wasm: &Path) -> Result<()> {
        let Some(flag) = self.level.flag() else {
            crate::progress::step("WebAssembly optimisation: off (the module is not optimised)");
            return Ok(());
        };
        let Some(binary) = &self.binary else {
            log::warn!(
                "no wasm-opt found, so wasm-pack optimised at its own -O this time; the '{}' level applies from the next build",
                self.level.id()
            );
            return Ok(());
        };
        let before = std::fs::metadata(wasm).with_context(|| format!("reading {}", wasm.display()))?.len();
        crate::progress::step(&format!(
            "Optimising {} with wasm-opt {flag} ({:.1} MB)...",
            wasm.file_name().unwrap_or_default().to_string_lossy(),
            before as f64 / 1e6
        ));
        let out = wasm.with_extension("wasm-opt.tmp");
        let started = Instant::now();
        let mut cmd = std::process::Command::new(binary);
        cmd.arg(wasm).arg("-o").arg(&out).arg(flag);
        let status = crate::progress::run_streaming(&mut cmd, "wasm-opt")
            .with_context(|| format!("running {}", binary.display()))?;
        if !status.success() {
            let _ = std::fs::remove_file(&out);
            anyhow::bail!("wasm-opt {flag} failed with status {status}");
        }
        std::fs::rename(&out, wasm).with_context(|| format!("replacing {}", wasm.display()))?;
        let after = std::fs::metadata(wasm)?.len();
        crate::progress::step(&format!(
            "wasm-opt {flag}: {:.1} MB -> {:.1} MB in {:.0} s",
            before as f64 / 1e6,
            after as f64 / 1e6,
            started.elapsed().as_secs_f64()
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_have_flags_and_round_trip_by_name() {
        assert_eq!(WasmOpt::default(), WasmOpt::Optimized, "the viewport's default");
        assert_eq!(WasmOpt::WEB_BUILD_DEFAULT, WasmOpt::Fast, "a game's web build: the smallest download");
        assert_eq!(WasmOpt::Optimized.flag(), Some("-O"));
        assert_eq!(WasmOpt::Fast.flag(), Some("-O1"));
        assert_eq!(WasmOpt::Off.flag(), None);
        for level in WasmOpt::ALL {
            assert_eq!(WasmOpt::parse(level.id()), Some(level));
            let json = serde_json::to_value(level).unwrap();
            assert_eq!(json, serde_json::json!(level.id()));
        }
        assert_eq!(WasmOpt::parse(" FAST "), Some(WasmOpt::Fast));
        assert_eq!(WasmOpt::parse("ludicrous"), None);
    }

    #[test]
    fn the_viewport_level_comes_from_the_editor_preference() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(EDITOR_PREFS_FILE);
        assert_eq!(viewport_level_in(&file), None, "no file");
        std::fs::write(&file, r#"{ "gpu": "system" }"#).unwrap();
        assert_eq!(viewport_level_in(&file), None, "an older file without the key");
        std::fs::write(&file, r#"{ "gpu": "system", "viewport_wasm_opt": "fast" }"#).unwrap();
        assert_eq!(viewport_level_in(&file), Some(WasmOpt::Fast));
        std::fs::write(&file, r#"{ "viewport_wasm_opt": "nonsense" }"#).unwrap();
        assert_eq!(viewport_level_in(&file), None, "an unknown level falls back to the default");
    }

    #[test]
    fn a_wasm_opt_on_path_beats_wasm_packs_copy() {
        let exe = if cfg!(windows) { "wasm-opt.exe" } else { "wasm-opt" };
        let path_dir = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let cached = cache.path().join("wasm-opt-abc123").join("bin");
        std::fs::create_dir_all(&cached).unwrap();
        std::fs::write(cached.join(exe), b"").unwrap();
        let found = find_binary_in(&[path_dir.path().into()], &[cache.path().into()]).unwrap();
        assert_eq!(found, cached.join(exe), "only the cache has one");
        std::fs::write(path_dir.path().join(exe), b"").unwrap();
        let found = find_binary_in(&[path_dir.path().into()], &[cache.path().into()]).unwrap();
        assert_eq!(found, path_dir.path().join(exe), "PATH first");
        assert_eq!(find_binary_in(&[], &[]), None);
    }

    #[test]
    fn with_no_wasm_opt_anywhere_wasm_pack_optimises() {
        let plan = Plan::with_binary(WasmOpt::Fast, None);
        assert!(!plan.no_opt && plan.wasm_pack_args().is_empty());
        let plan = Plan::with_binary(WasmOpt::Fast, Some("wasm-opt".into()));
        assert_eq!(plan.wasm_pack_args(), ["--no-opt"]);
        let plan = Plan::with_binary(WasmOpt::Off, Some("wasm-opt".into()));
        assert!(plan.no_opt && plan.binary.is_none(), "off runs nothing");
    }

    /// The real thing, when a wasm-opt is installed: a minimal module comes
    /// back optimised and valid.
    #[test]
    fn a_module_is_optimised_in_place() {
        let Some(binary) = find_binary() else {
            eprintln!("skipped: no wasm-opt on this machine");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let wasm = dir.path().join("m.wasm");
        // An empty module: the magic number and version 1.
        std::fs::write(&wasm, b"\0asm\x01\0\0\0").unwrap();
        Plan::with_binary(WasmOpt::Fast, Some(binary)).optimize(&wasm).unwrap();
        let bytes = std::fs::read(&wasm).unwrap();
        assert!(bytes.starts_with(b"\0asm"), "not a module any more");
        assert!(!dir.path().join("m.wasm-opt.tmp").exists(), "the temporary file was left behind");
    }
}
