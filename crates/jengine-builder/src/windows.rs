//! Windows build — packages the native runtime + assets into a distributable folder.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Compile the project's native runtime and say where the executable is.
///
/// A project that ships its own Rust packages gets a *generated* crate under
/// `<project>/.jengine/runtime/` that depends on `jengine-runtime` as a
/// library plus those packages, and registers them through `run_with`.
/// Projects with no Rust packages build the stock runtime - the generated
/// crate is not created at all in that case. The executable runs a project
/// from its own folder (it reads `project.jengine` and `assets/` from the
/// working directory), which is what the editor's Build & Run does; a
/// Windows build copies it beside the staged game instead.
pub fn compile_native_runtime(engine_path: &Path, project_path: &Path, release: bool) -> anyhow::Result<PathBuf> {
    compile_native_runtime_for(engine_path, project_path, release, host_platform())
}

/// The desktop platform this builder runs on, by the name
/// `settings.graphics_backends` keys it: `windows`, `linux` or `macos`.
pub fn host_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

/// How a project's native runtime is built: where cargo runs, what the
/// binary is called, and its arguments - shared by a build on this machine
/// and a Linux build run inside WSL (`desktop::build_linux`).
pub struct RuntimeBuild {
    pub build_dir: PathBuf,
    pub binary_stem: String,
    pub args: Vec<String>,
    pub profile: &'static str,
}

/// The cargo arguments that strip a release runtime's symbol table, for the
/// platforms that keep it in the binary. An ELF or Mach-O executable carries
/// it unless told otherwise: a quarter of the empty project's Linux binary
/// (54.1 MB, 40.3 stripped - `docs/build-sizes.md`). A panic still names
/// itself; only a backtrace loses its function names, the trade the Android
/// build already makes. Windows keeps its symbols in the `.pdb` beside the
/// exe, which a build does not ship.
pub fn strip_args(release: bool, platform: &str) -> &'static [&'static str] {
    if release && matches!(platform, "linux" | "macos" | "ios") {
        &["--config", "profile.release.strip=true"]
    } else {
        &[]
    }
}

/// `jengine-runtime`'s default graphics backends, put back by hand when a
/// build must pass `--no-default-features` for its engine features alone.
/// Keep in step with `default` in `crates/jengine-runtime/Cargo.toml`.
const DEFAULT_BACKENDS: [&str; 3] = ["backend-dx12", "backend-vulkan", "backend-metal"];

/// Plan the runtime's build for `platform` (its graphics backends come from
/// `settings.graphics_backends.<platform>`), generating the project's
/// runtime crate when it has Rust packages.
pub fn plan_native_runtime(engine_path: &Path, project_path: &Path, release: bool, platform: &str) -> anyhow::Result<RuntimeBuild> {
    let profile = if release { "release" } else { "debug" };
    let resolved = match crate::packages::resolve_packages(project_path) {
        Ok(pkgs) => pkgs,
        Err(e) => {
            log::warn!("package resolution failed, building stock runtime: {e:#}");
            Vec::new()
        }
    };
    let project_runtime = crate::packages::generate_project_runtime(project_path, engine_path, &resolved)?;
    let (binary_stem, build_dir, generated) = match &project_runtime {
        Some(manifest) => {
            let dir = manifest.parent().expect("generated manifest has a parent").to_path_buf();
            let stem = crate::packages::project_runtime_binary_name(project_path);
            crate::progress::step(&format!(
                "Compiling {stem} with {} project package(s) ({profile})...",
                resolved
                    .iter()
                    .filter(|p| p.package_type == jengine_core::package::manifest::PackageType::Full)
                    .count()
            ));
            (stem, dir, true)
        }
        None => {
            crate::progress::step(&format!("Compiling jengine-runtime ({profile})..."));
            ("jengine-runtime".to_string(), engine_path.to_path_buf(), false)
        }
    };
    let mut args: Vec<String> = vec!["build".into()];
    if !generated {
        args.push("-p".into());
        args.push("jengine-runtime".into());
    }
    if release {
        args.push("--release".into());
    }
    args.extend(strip_args(release, platform).iter().map(|a| a.to_string()));
    // Graphics backends the project pinned for this platform; without a
    // pin the runtime's own default (every desktop API) applies.
    let backend_features = crate::graphics::backends_for_platform(project_path, platform)
        .map(|backends| {
            log::info!("{platform} graphics backends: {}", backends.join(", "));
            crate::graphics::features_for(&backends).join(",")
        })
        .filter(|f| !f.is_empty());
    // The engine features (`engine_features`). A generated crate names them
    // on its own `jengine-core` dependency; the stock runtime is told on the
    // command line, which `--no-default-features` would otherwise empty.
    let engine = crate::engine_features::for_project(project_path)?;
    crate::progress::step(&engine.summary());
    let mut features: Vec<String> = Vec::new();
    if let Some(backends) = &backend_features {
        features.push(backends.clone());
    }
    if !generated && (backend_features.is_some() || !engine.is_all()) {
        if backend_features.is_none() {
            // The stock runtime's own default backends.
            features.push(DEFAULT_BACKENDS.join(","));
        }
        features.extend(engine.host_features());
    }
    if !features.is_empty() {
        args.push("--no-default-features".into());
        args.push("--features".into());
        args.push(features.join(","));
    }
    Ok(RuntimeBuild { build_dir, binary_stem, args, profile })
}

/// Build the runtime on this machine for `platform` (this machine's own)
/// and say where the executable is.
pub fn compile_native_runtime_for(
    engine_path: &Path,
    project_path: &Path,
    release: bool,
    platform: &str,
) -> anyhow::Result<PathBuf> {
    let plan = plan_native_runtime(engine_path, project_path, release, platform)?;
    // Streamed rather than inherited: compiling the runtime is the long
    // part of a build, and its output is what the editor's Build panel
    // shows.
    let mut cmd = Command::new("cargo");
    cmd.args(&plan.args).current_dir(&plan.build_dir);
    let status = crate::progress::run_streaming(&mut cmd, "cargo")?;
    if !status.success() {
        anyhow::bail!("cargo build failed with status: {status}");
    }
    // The generated crate is its own workspace, so its artifacts land in its
    // own `target/`, not the engine's - or in CARGO_TARGET_DIR.
    let source_binary = crate::cargo_target_dir(&plan.build_dir)
        .join(plan.profile)
        .join(format!("{}{}", plan.binary_stem, std::env::consts::EXE_SUFFIX));
    if !source_binary.exists() {
        anyhow::bail!("Built binary not found at: {}", source_binary.display());
    }
    Ok(source_binary)
}

/// Build a Windows distributable for the given project.
///
/// This will:
/// 1. Compile `jengine-runtime` in release mode
/// 2. Copy the runtime binary to `output_path/`
/// 3. Copy all assets (scenes, scripts, sprites) to `output_path/assets/`
pub fn build_windows(
    engine_path: &Path,
    project_path: &Path,
    output_path: &Path,
    release: bool,
    generate_report: bool,
    enable_profiling: bool,
    debug_overlay: bool,
) -> anyhow::Result<()> {
    let profile_name = if release { "release" } else { "debug" };
    log::info!(
        "Building Windows target ({}): engine={:?} project={:?} -> {:?}",
        profile_name,
        engine_path,
        project_path,
        output_path
    );

    // 1. Compile the runtime.
    let source_binary = compile_native_runtime(engine_path, project_path, release)?;

    // 2. Create output directory and copy the binary
    std::fs::create_dir_all(output_path)?;
    // Ship it under a stable name regardless of which crate produced it, so
    // the launcher script and anything else downstream do not have to care.
    let dest_binary = output_path.join("jengine-runtime.exe");

    std::fs::copy(&source_binary, &dest_binary)?;
    log::info!("Copied binary to {}", dest_binary.display());

    // 3. The game's files beside it: assets, project settings, packed atlases.
    crate::stage::stage_game(project_path, output_path, debug_overlay, &output_path.join("packs"))?;

    crate::progress::step(&format!(
        "Windows build complete: {}",
        output_path.display()
    ));

    // Emit a profiling launcher when requested. The standard `.exe` still
    // runs without profiling overhead; double-clicking `run-profiled.cmd`
    // sets the env vars that make `ScriptSystem` emit `[Script][PROF]`
    // log lines every 600 frames.
    if enable_profiling {
        let cmd_path = output_path.join("run-profiled.cmd");
        let script = "@echo off\r\n\
            REM Auto-generated by jEngine editor: launches the game with\r\n\
            REM script-system profiling enabled. Look for [Script][PROF]\r\n\
            REM lines in the console output.\r\n\
            setlocal\r\n\
            set JENGINE_SCRIPT_PROFILE=600\r\n\
            set RUST_LOG=info\r\n\
            \"%~dp0jengine-runtime.exe\" %*\r\n\
            endlocal\r\n";
        std::fs::write(&cmd_path, script)?;
        log::info!("Wrote profiling launcher to {}", cmd_path.display());
    }

    if generate_report {
        crate::report::generate_report(output_path, "windows", profile_name, None, crate::engine_features::for_project(project_path).ok().as_ref())?;
    }

    Ok(())
}
