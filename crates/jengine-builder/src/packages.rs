//! Package resolver — reads project.jengine, fetches packages, syncs Cargo.toml.
//!
//! Packages are resolved at build-time:
//! - **Path** packages are used in-place (relative or absolute).
//! - **Git** packages are cloned to `.jengine/packages/<name>-<hash>/`.
//!
//! After resolution, `sync_cargo_toml()` patches the workspace `Cargo.toml`
//! to include resolved packages as path dependencies.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use jengine_core::package::manifest::{PackageDependency, PackageManifest, PackageSource, PackageType};

/// Directory under project root where git packages are cached.
const PACKAGES_CACHE_DIR: &str = ".jengine/packages";

/// Directory under project assets where package scripts/assets are installed.
const PACKAGES_ASSETS_DIR: &str = "assets/packages";

// ──────────────────────────── Resolved Package ────────────────────────────

/// A package dependency with a known local filesystem path.
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    /// Package name.
    pub name: String,
    /// Absolute path to the package on disk.
    pub local_path: PathBuf,
    /// Original source declaration.
    pub source: PackageSource,
    /// Parsed `package.toml` if it exists.
    pub manifest: Option<PackageManifest>,
    /// Detected package type (full, script, or asset).
    pub package_type: PackageType,
    /// Files installed into the project (relative to project root).
    pub installed_files: Vec<PathBuf>,
}

// ──────────────────────────── Read Dependencies ────────────────────────────

/// Read the `"packages"` array from a `project.jengine` file.
pub fn read_package_dependencies(project_root: &Path) -> Result<Vec<PackageDependency>> {
    let file = project_root.join("project.jengine");
    let contents = std::fs::read_to_string(&file)
        .with_context(|| format!("Failed to read {}", file.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&contents).context("Invalid JSON in project.jengine")?;

    match value.get("packages") {
        Some(arr) => {
            let deps: Vec<PackageDependency> =
                serde_json::from_value(arr.clone()).context("Invalid packages array")?;
            Ok(deps)
        }
        None => Ok(Vec::new()),
    }
}

// ──────────────────────────── Resolve ────────────────────────────

/// Resolve all package dependencies to local filesystem paths.
///
/// - Path dependencies are resolved relative to the project root.
/// - Git dependencies are cloned/updated in `.jengine/packages/`.
///
/// Returns an error if any package fails to resolve.
pub fn resolve_packages(project_root: &Path) -> Result<Vec<ResolvedPackage>> {
    let deps = read_package_dependencies(project_root)?;
    let mut resolved = Vec::with_capacity(deps.len());

    for dep in &deps {
        if dep.optional {
            log::info!("Skipping optional package: {}", dep.name);
            continue;
        }

        let local_path = match &dep.source {
            PackageSource::Path { path } => {
                let p = project_root.join(path);
                if !p.exists() {
                    anyhow::bail!(
                        "Package '{}' path does not exist: {}",
                        dep.name,
                        p.display()
                    );
                }
                log::info!("Resolved package '{}' from path: {}", dep.name, p.display());
                p
            }
            PackageSource::Git { url, tag, rev } => {
                let hash = source_hash(url, tag.as_deref(), rev.as_deref());
                let cache_dir = project_root
                    .join(PACKAGES_CACHE_DIR)
                    .join(format!("{}-{hash}", dep.name));

                if !cache_dir.exists() {
                    clone_git_package(url, tag.as_deref(), rev.as_deref(), &cache_dir)
                        .with_context(|| {
                            format!("Failed to clone package '{}' from {url}", dep.name)
                        })?;
                } else {
                    log::info!(
                        "Package '{}' already cached at {}",
                        dep.name,
                        cache_dir.display()
                    );
                }
                cache_dir
            }
        };

        // Try to load package.toml from the resolved path
        let manifest = load_package_manifest(&local_path);

        // Determine package type
        let package_type = manifest
            .as_ref()
            .map(|m| m.package.package_type)
            .unwrap_or(PackageType::Full);

        // Install script/asset files for non-full packages
        let installed_files = match package_type {
            PackageType::Script => install_script_package(project_root, &dep.name, &local_path, &manifest)?,
            PackageType::Asset => install_asset_package(project_root, &dep.name, &local_path, &manifest)?,
            PackageType::Full => Vec::new(),
        };

        if !installed_files.is_empty() {
            log::info!(
                "Installed {} file(s) from {} package '{}'",
                installed_files.len(),
                match package_type {
                    PackageType::Script => "script",
                    PackageType::Asset => "asset",
                    PackageType::Full => "full",
                },
                dep.name
            );
        }

        resolved.push(ResolvedPackage {
            name: dep.name.clone(),
            local_path,
            source: dep.source.clone(),
            manifest,
            package_type,
            installed_files,
        });
    }

    log::info!("Resolved {} package(s)", resolved.len());
    Ok(resolved)
}

// ──────────────────────────── Git Operations ────────────────────────────

/// Clone a git repository to a local directory.
fn clone_git_package(
    url: &str,
    tag: Option<&str>,
    rev: Option<&str>,
    target_dir: &Path,
) -> Result<()> {
    log::info!("Cloning package from {url} to {}", target_dir.display());

    // Create parent directory
    if let Some(parent) = target_dir.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create cache directory: {}", parent.display()))?;
    }

    // git clone
    let mut cmd = std::process::Command::new("git");
    cmd.arg("clone").arg(url).arg(target_dir);

    // If tag is specified, use --branch to clone that tag directly
    if let Some(t) = tag {
        cmd.arg("--branch").arg(t);
    }

    cmd.arg("--depth").arg("1"); // Shallow clone for speed

    let output = cmd
        .output()
        .context("Failed to run git. Is git installed and in PATH?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git clone failed: {stderr}");
    }

    // If rev is specified (and no tag), check out the specific commit
    if tag.is_none()
        && let Some(r) = rev
    {
        let output = std::process::Command::new("git")
            .args(["checkout", r])
            .current_dir(target_dir)
            .output()
            .context("Failed to run git checkout")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git checkout {r} failed: {stderr}");
        }
    }

    Ok(())
}

// ──────────────────────────── Cargo.toml Sync ────────────────────────────

/// Generate Cargo patch/dependency lines for resolved packages.
///
/// Returns a TOML string that can be appended to Cargo.toml:
/// ```toml
/// # jEngine packages (auto-generated)
/// [workspace.dependencies]
/// my-plugin = { path = ".jengine/packages/my-plugin-abc123" }
/// ```
pub fn generate_cargo_dependencies(
    project_root: &Path,
    packages: &[ResolvedPackage],
) -> Result<String> {
    if packages.is_empty() {
        return Ok(String::new());
    }

    let mut lines = Vec::new();
    lines.push("# --- jEngine packages (auto-generated, do not edit) ---".to_string());

    for pkg in packages {
        // Skip non-Rust packages — they don't need Cargo dependencies
        if pkg.package_type != PackageType::Full {
            continue;
        }
        // Make path relative to project root for portability
        let rel_path = pathdiff(project_root, &pkg.local_path);
        let path_str = rel_path.to_string_lossy().replace('\\', "/");
        lines.push(format!("{} = {{ path = \"{}\" }}", pkg.name, path_str));
    }

    lines.push("# --- end jEngine packages ---".to_string());
    Ok(lines.join("\n"))
}

// ──────────────────────── Per-project runtime crate ────────────────────────

/// Name of the generated crate directory under `<project>/.jengine/`.
const PROJECT_RUNTIME_DIR: &str = "runtime";

/// Generate a standalone runtime crate for `project_root` that links the
/// project's Full (Rust) packages, returning its manifest path.
///
/// Returns `Ok(None)` when the project has no Rust packages — then there is
/// nothing to generate and the caller builds the stock `jengine-runtime`.
///
/// The generated crate carries an empty `[workspace]` table so Cargo treats it
/// as its own workspace root. Without that it would be swallowed by whichever
/// workspace encloses it — projects live under the engine repo's `projects/`,
/// so the engine workspace would try to claim it and fail on the unlisted
/// member.
///
/// Backend features are forwarded rather than re-declared, so
/// `--features backend-dx12` means the same thing here as on the stock
/// runtime and the graphics pinning in project settings keeps working.
pub fn generate_project_runtime(
    project_root: &Path,
    engine_root: &Path,
    packages: &[ResolvedPackage],
) -> Result<Option<PathBuf>> {
    let full: Vec<&ResolvedPackage> = packages
        .iter()
        .filter(|p| p.package_type == PackageType::Full)
        .collect();
    if full.is_empty() {
        return Ok(None);
    }

    let crate_dir = project_root.join(".jengine").join(PROJECT_RUNTIME_DIR);
    std::fs::create_dir_all(crate_dir.join("src"))
        .with_context(|| format!("creating {}", crate_dir.display()))?;

    let project_name = project_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("game");
    let crate_name = format!("{}-runtime", slug(project_name));

    // Paths are relative to the generated crate so the project tree stays
    // movable, as long as it keeps the same relation to the engine.
    let runtime_path = rel_path(&crate_dir, &engine_root.join("crates").join("jengine-runtime"));
    let winit_path = rel_path(&crate_dir, &engine_root.join("vendor").join("winit"));

    let mut deps = String::new();
    for pkg in &full {
        deps.push_str(&format!(
            "{} = {{ path = \"{}\" }}\n",
            pkg.name,
            rel_path(&crate_dir, &pkg.local_path)
        ));
    }

    let cargo_toml = format!(
        "# Generated by jengine-builder. Do not edit — rebuilt on every build.\n\
         #\n\
         # This crate exists so `{project_name}` can ship its own Rust packages\n\
         # without editing the engine workspace. See docs/writing-packages.md.\n\
         \n\
         [package]\n\
         name = \"{crate_name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\
         \n\
         # Standalone: not a member of whatever workspace encloses the project.\n\
         [workspace]\n\
         \n\
         [dependencies]\n\
         jengine-runtime = {{ path = \"{runtime_path}\", default-features = false }}\n\
         anyhow = \"1\"\n\
         {deps}\n\
         # Forwarded so `--features backend-*` behaves exactly as it does on the\n\
         # stock runtime, and project graphics pinning keeps working.\n\
         [features]\n\
         default = [\"backend-dx12\", \"backend-vulkan\", \"backend-metal\"]\n\
         backend-dx12 = [\"jengine-runtime/backend-dx12\"]\n\
         backend-vulkan = [\"jengine-runtime/backend-vulkan\"]\n\
         backend-metal = [\"jengine-runtime/backend-metal\"]\n\
         backend-webgl = [\"jengine-runtime/backend-webgl\"]\n\
         \n\
         # The engine's winit (vendor/winit/JENGINE-PATCH.md): on iOS it is what\n\
         # puts the window in a UIWindowScene, without which iOS 27 shows only\n\
         # the launch screen. A standalone workspace does not inherit the\n\
         # engine's own [patch], so it is repeated here.\n\
         [patch.crates-io]\n\
         winit = {{ path = \"{winit_path}\" }}\n"
    );
    write_if_changed(&crate_dir.join("Cargo.toml"), &cargo_toml)?;
    seed_engine_lock(&crate_dir, engine_root)?;

    let adds = package_add_lines(&full);

    let main_rs = format!(
        "//! Generated by jengine-builder for `{project_name}`. Do not edit.\n\
         //!\n\
         //! The engine runs from `jengine_runtime` as a library; this crate\n\
         //! exists only to link this project's packages and register them.\n\
         \n\
         fn main() -> anyhow::Result<()> {{\n\
         \x20   jengine_runtime::run_with(&|host| {{\n\
         {adds}\x20   }})\n\
         }}\n"
    );
    write_if_changed(&crate_dir.join("src").join("main.rs"), &main_rs)?;

    log::info!(
        "generated project runtime for {} with {} package(s) at {}",
        project_name,
        full.len(),
        crate_dir.display()
    );
    Ok(Some(crate_dir.join("Cargo.toml")))
}

/// Directory under `<project>/.jengine/` for the generated Android crate.
pub const PROJECT_ANDROID_DIR: &str = "android";

/// Generate the crate an Android build compiles, returning its manifest path.
///
/// Unlike [`generate_project_runtime`] this always generates, packages or
/// not: Android does not run a binary, it loads `libmain.so` and calls its
/// `android_main`, so there is no stock artifact to fall back to. The crate
/// is a `cdylib` named `main` (the name NativeActivity looks for) whose one
/// function registers the project's packages and hands over to
/// `jengine_runtime::android`.
///
/// `backend_features` are `jengine-runtime` features (`backend-vulkan`).
/// On Android any native backend also compiles wgpu's GLES path - wgpu
/// turns it on by target - so a phone without usable Vulkan still gets an
/// adapter.
pub fn generate_project_android(
    project_root: &Path,
    engine_root: &Path,
    packages: &[ResolvedPackage],
    backend_features: &[String],
) -> Result<PathBuf> {
    let full: Vec<&ResolvedPackage> = packages
        .iter()
        .filter(|p| p.package_type == PackageType::Full)
        .collect();

    let crate_dir = project_root.join(".jengine").join(PROJECT_ANDROID_DIR);
    std::fs::create_dir_all(crate_dir.join("src"))
        .with_context(|| format!("creating {}", crate_dir.display()))?;

    let project_name = project_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("game");
    let crate_name = format!("{}-android", slug(project_name));
    let winit_path = rel_path(&crate_dir, &engine_root.join("vendor").join("winit"));
    let runtime_path = rel_path(&crate_dir, &engine_root.join("crates").join("jengine-runtime"));

    let mut deps = String::new();
    for pkg in &full {
        deps.push_str(&format!(
            "{} = {{ path = \"{}\" }}\n",
            pkg.name,
            rel_path(&crate_dir, &pkg.local_path)
        ));
    }
    let features = backend_features
        .iter()
        .map(|f| format!("\"{f}\""))
        .collect::<Vec<_>>()
        .join(", ");

    let cargo_toml = format!(
        "# Generated by jengine-builder. Do not edit — rebuilt on every build.\n\
         #\n\
         # The Android build of `{project_name}`: the runtime as `libmain.so`.\n\
         # See docs/mobile-platforms-proposal.md.\n\
         \n\
         [package]\n\
         name = \"{crate_name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\
         \n\
         # Standalone: not a member of whatever workspace encloses the project.\n\
         [workspace]\n\
         \n\
         [lib]\n\
         # NativeActivity loads `lib<name>.so` for the `android.app.lib_name`\n\
         # the manifest gives it, and the builder writes `main` there.\n\
         name = \"main\"\n\
         crate-type = [\"cdylib\"]\n\
         path = \"src/lib.rs\"\n\
         \n\
         [dependencies]\n\
         jengine-runtime = {{ path = \"{runtime_path}\", default-features = false, features = [{features}] }}\n\
         {deps}\n\
         [profile.release]\n\
         # Symbols stripped: a phone has no use for them and they are most of\n\
         # the library. `adb logcat` still names the panic.\n\
         strip = true\n\
         lto = \"thin\"\n\
         \n\
         # winit with the Android input hook game controllers arrive through\n\
         # (vendor/winit/JENGINE-PATCH.md). A standalone workspace does not\n\
         # inherit the engine's own [patch], so it is repeated here.\n\
         [patch.crates-io]\n\
         winit = {{ path = \"{winit_path}\" }}\n"
    );
    write_if_changed(&crate_dir.join("Cargo.toml"), &cargo_toml)?;
    seed_engine_lock(&crate_dir, engine_root)?;

    let adds = package_add_lines(&full);
    let lib_rs = format!(
        "//! Generated by jengine-builder for `{project_name}`. Do not edit.\n\
         //!\n\
         //! NativeActivity calls `android_main` on its own thread once the app\n\
         //! starts; everything after that is `jengine_runtime`.\n\
         \n\
         #[unsafe(no_mangle)]\n\
         fn android_main(app: jengine_runtime::android::AndroidApp) {{\n\
         \x20   jengine_runtime::android::run_with(app, &|host| {{\n\
         {adds}\x20   }});\n\
         }}\n"
    );
    // The closure is empty for a project with no packages, which names `host`
    // without using it.
    let lib_rs = if full.is_empty() {
        lib_rs.replace("&|host| {", "&|_host| {")
    } else {
        lib_rs
    };
    write_if_changed(&crate_dir.join("src").join("lib.rs"), &lib_rs)?;

    log::info!(
        "generated Android crate for {} with {} package(s) at {}",
        project_name,
        full.len(),
        crate_dir.display()
    );
    Ok(crate_dir.join("Cargo.toml"))
}

/// Directory under `<project>/.jengine/` for the generated viewport wrapper.
pub(crate) const PROJECT_VIEWPORT_DIR: &str = "viewport";

/// Generate a WASM wrapper crate that links this project's packages into the
/// editor viewport, returning its manifest path.
///
/// Returns `Ok(None)` when the project has no Rust packages — then the editor
/// uses the stock viewport and there is nothing to build.
///
/// # Why a wrapper works
///
/// The viewport is one prebuilt WASM module shared by every project, so a
/// project package cannot be linked into it ahead of time. `wasm-bindgen`
/// exports defined in an **rlib dependency** do survive into a dependent
/// `cdylib` (verified: a wrapper reproduces all 80 of the viewport's exports,
/// adding only its own), so a wrapper crate that re-exports
/// `jengine-editor-viewport` produces a drop-in replacement module with the
/// identical JS API — the editor's imports do not change.
///
/// The wrapper registers the project's packages from `#[wasm_bindgen(start)]`,
/// which runs during module init, before the editor calls `viewport_init` and
/// therefore before the `PackageHost` is built.
pub fn generate_project_viewport(
    project_root: &Path,
    engine_root: &Path,
    packages: &[ResolvedPackage],
) -> Result<Option<PathBuf>> {
    let full: Vec<&ResolvedPackage> = packages
        .iter()
        .filter(|p| p.package_type == PackageType::Full)
        .collect();
    if full.is_empty() {
        return Ok(None);
    }

    let crate_dir = project_root.join(".jengine").join(PROJECT_VIEWPORT_DIR);
    std::fs::create_dir_all(crate_dir.join("src"))
        .with_context(|| format!("creating {}", crate_dir.display()))?;

    let project_name = project_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("game");
    let crate_name = format!("{}-viewport", slug(project_name));
    let viewport_path = rel_path(
        &crate_dir,
        &engine_root.join("crates").join("jengine-editor-viewport"),
    );

    let mut deps = String::new();
    for pkg in &full {
        deps.push_str(&format!(
            "{} = {{ path = \"{}\" }}\n",
            pkg.name,
            rel_path(&crate_dir, &pkg.local_path)
        ));
    }

    // The crate name must be UNIQUE, not the viewport's: Cargo refuses two
    // packages with the same name and version in one lockfile ("package
    // collision in the lockfile"), and this crate depends on the viewport.
    //
    // The emitted module still has to be named `jengine_editor_viewport`,
    // because the editor imports that fixed path — `wasm-pack --out-name`
    // pins the filenames independently of the crate name, which is what
    // `build-project-viewport.ps1` passes.
    let cargo_toml = format!(
        "# Generated by jengine-builder. Do not edit — rebuilt on every build.\n\
         #\n\
         # Links `{project_name}`'s Rust packages into the editor viewport.\n\
         # Built with `wasm-pack --out-name jengine_editor_viewport` so the\n\
         # emitted module is a drop-in replacement for the stock viewport.\n\
         \n\
         [package]\n\
         name = \"{crate_name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\
         \n\
         # Standalone: not a member of whatever workspace encloses the project.\n\
         [workspace]\n\
         \n\
         [lib]\n\
         crate-type = [\"cdylib\", \"rlib\"]\n\
         \n\
         [dependencies]\n\
         jengine-editor-viewport-lib = {{ package = \"jengine-editor-viewport\", path = \"{viewport_path}\" }}\n\
         wasm-bindgen = \"0.2\"\n\
         {deps}"
    );
    write_if_changed(&crate_dir.join("Cargo.toml"), &cargo_toml)?;
    seed_engine_lock(&crate_dir, engine_root)?;

    let adds = package_add_lines(&full);

    let lib_rs = format!(
        "//! Generated by jengine-builder for `{project_name}`. Do not edit.\n\
         //!\n\
         //! Re-exports the editor viewport so this module has the identical JS\n\
         //! API, and registers this project's packages at module init.\n\
         \n\
         pub use jengine_editor_viewport_lib::*;\n\
         \n\
         /// Runs during WASM module init — before the editor calls\n\
         /// `viewport_init`, which is when the `PackageHost` is built.\n\
         #[wasm_bindgen::prelude::wasm_bindgen(start)]\n\
         pub fn __register_project_packages() {{\n\
         \x20   jengine_editor_viewport_lib::register_project_package(|host| {{\n\
         {adds}\x20   }});\n\
         }}\n"
    );
    write_if_changed(&crate_dir.join("src").join("lib.rs"), &lib_rs)?;

    log::info!(
        "generated project viewport for {} with {} package(s) at {}",
        project_name,
        full.len(),
        crate_dir.display()
    );
    Ok(Some(crate_dir.join("Cargo.toml")))
}

/// Directory under `<project>/.jengine/` for the generated web crate.
const PROJECT_WEB_DIR: &str = "web";

/// Packages that cannot be compiled into a web build.
///
/// `jengine-audio` turns on jengine-core's kira-backed `audio` feature, which
/// is native-only; browser runtimes play sound through `web_audio` instead,
/// which is why the editor viewport leaves it out too. Listed by name so a
/// web build refuses it with that name, rather than failing deep in a wasm
/// compile - or, worse, quietly building without it, which is the `nil`
/// global at Play this generator exists to remove.
pub const NATIVE_ONLY_PACKAGES: &[&str] = &["jengine-audio"];

/// Generate a WASM crate that links this project's packages into the web
/// runtime, returning its manifest path.
///
/// Returns `Ok(None)` when the project has no Rust packages - then a web build
/// uses the stock `jengine-web` exactly as before.
///
/// The shape is the viewport wrapper's (see [`generate_project_viewport`]),
/// with one difference: `jengine-web` has a `#[wasm_bindgen(start)]` of its
/// own, and wasm-bindgen allows one per module. So this crate depends on
/// `jengine-web` with its `standalone` feature **off** - which removes that
/// start - and its own start registers the packages and then calls
/// `jengine_web::run()`. A stock build keeps `standalone` and is unchanged.
///
/// Features are forwarded rather than re-declared, so `--features
/// physics,backend-webgpu` means the same here as on the stock crate. A crate
/// that does not forward `physics` would compile the web host's physics code
/// out without a word, because `jengine-web`'s `#[cfg(feature = "physics")]`
/// reads jengine-web's own feature.
pub fn generate_project_web(
    project_root: &Path,
    engine_root: &Path,
    packages: &[ResolvedPackage],
) -> Result<Option<PathBuf>> {
    let full: Vec<&ResolvedPackage> = packages
        .iter()
        .filter(|p| p.package_type == PackageType::Full)
        .collect();
    let native_only: Vec<&str> = full
        .iter()
        .map(|p| p.name.as_str())
        .filter(|n| NATIVE_ONLY_PACKAGES.contains(n))
        .collect();
    if !native_only.is_empty() {
        anyhow::bail!(
            "this project links package(s) that cannot run in a web build: {}. \
             Remove them from the project's \"packages\" list, or build for \
             Windows instead.",
            native_only.join(", ")
        );
    }
    if full.is_empty() {
        return Ok(None);
    }

    let crate_dir = project_root.join(".jengine").join(PROJECT_WEB_DIR);
    std::fs::create_dir_all(crate_dir.join("src"))
        .with_context(|| format!("creating {}", crate_dir.display()))?;

    let project_name = project_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("game");
    // Unique for the same reason as the viewport wrapper's: Cargo refuses a
    // crate and its dependency sharing a name. `wasm-pack --out-name
    // jengine_web` pins the emitted files, so `index.html` is unchanged.
    let crate_name = format!("{}-web", slug(project_name));
    let web_path = rel_path(&crate_dir, &engine_root.join("crates").join("jengine-web"));

    let mut deps = String::new();
    for pkg in &full {
        deps.push_str(&format!(
            "{} = {{ path = \"{}\" }}\n",
            pkg.name,
            rel_path(&crate_dir, &pkg.local_path)
        ));
    }

    let cargo_toml = format!(
        "# Generated by jengine-builder. Do not edit — rebuilt on every build.\n\
         #\n\
         # Links `{project_name}`'s Rust packages into the web runtime.\n\
         # Built with `wasm-pack --out-name jengine_web` so the emitted module\n\
         # is a drop-in replacement for the stock one.\n\
         \n\
         [package]\n\
         name = \"{crate_name}\"\n\
         version = \"0.1.0\"\n\
         edition = \"2024\"\n\
         \n\
         # Standalone: not a member of whatever workspace encloses the project.\n\
         [workspace]\n\
         \n\
         [lib]\n\
         crate-type = [\"cdylib\", \"rlib\"]\n\
         \n\
         [dependencies]\n\
         # `standalone` off: this crate supplies the module's start function.\n\
         jengine-web-lib = {{ package = \"jengine-web\", path = \"{web_path}\", default-features = false }}\n\
         wasm-bindgen = \"0.2\"\n\
         {deps}\n\
         # Forwarded so `--features physics,backend-*` behaves exactly as it\n\
         # does on the stock web crate.\n\
         [features]\n\
         default = [\"physics\", \"backend-webgpu\"]\n\
         physics = [\"jengine-web-lib/physics\"]\n\
         backend-webgpu = [\"jengine-web-lib/backend-webgpu\"]\n\
         backend-webgl = [\"jengine-web-lib/backend-webgl\"]\n"
    );
    write_if_changed(&crate_dir.join("Cargo.toml"), &cargo_toml)?;
    seed_engine_lock(&crate_dir, engine_root)?;

    let lib_rs = format!(
        "//! Generated by jengine-builder for `{project_name}`. Do not edit.\n\
         //!\n\
         //! Registers this project's packages, then starts the web runtime.\n\
         \n\
         #![cfg(target_arch = \"wasm32\")]\n\
         \n\
         /// The module's one start function: `jengine-web`'s own is compiled\n\
         /// out with its `standalone` feature.\n\
         #[wasm_bindgen::prelude::wasm_bindgen(start)]\n\
         pub fn __start_with_project_packages() {{\n\
         \x20   jengine_web_lib::register_project_package(|host| {{\n\
         {adds}\x20   }});\n\
         \x20   jengine_web_lib::run();\n\
         }}\n",
        adds = package_add_lines(&full),
    );
    write_if_changed(&crate_dir.join("src").join("lib.rs"), &lib_rs)?;

    log::info!(
        "generated project web crate for {} with {} package(s) at {}",
        project_name,
        full.len(),
        crate_dir.display()
    );
    Ok(Some(crate_dir.join("Cargo.toml")))
}

/// One `host.add_package(..)` line per package, indented for the body of the
/// registration closure every generated crate has.
fn package_add_lines(full: &[&ResolvedPackage]) -> String {
    let mut adds = String::new();
    for pkg in full {
        let entry = pkg
            .manifest
            .as_ref()
            .map(|m| m.package.resolved_entry_point())
            .unwrap_or_else(|| derived_entry_point(&pkg.name));
        adds.push_str(&format!("        host.add_package(Box::new({entry}));\n"));
    }
    adds
}

/// Name of the binary the generated project runtime produces.
///
/// Derived from the project directory the same way `generate_project_runtime`
/// derives the crate name, so the build step can find the artifact without
/// re-reading the generated manifest.
pub fn project_runtime_binary_name(project_root: &Path) -> String {
    let project_name = project_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("game");
    format!("{}-runtime", slug(project_name))
}

/// Write only when the content differs, so an unchanged package set does not
/// bump mtimes and force Cargo to rebuild the whole crate every time.
/// Give a generated crate the engine's `Cargo.lock`, so a project builds
/// against the dependency versions the engine is tested with.
///
/// A generated crate is a workspace of its own and would otherwise resolve
/// every dependency afresh - which is how a project's viewport came to link
/// `wasm-bindgen` 0.2.129 while the engine pinned 0.2.127. With one
/// `CARGO_TARGET_DIR` for both, the two builds also write the same uplifted
/// files, and wasm-pack then ran one version's CLI over the other's module
/// and refused it. Cargo keeps the versions it finds and adds only what the
/// project's packages bring, so seeding the lock changes nothing else.
fn seed_engine_lock(crate_dir: &Path, engine_root: &Path) -> Result<()> {
    let Ok(lock) = std::fs::read_to_string(engine_root.join("Cargo.lock")) else {
        return Ok(()); // an engine checkout without a lock: nothing to seed
    };
    let target = crate_dir.join("Cargo.lock");
    if let Ok(existing) = std::fs::read_to_string(&target)
        && lock_versions_agree(&existing, &lock)
    {
        return Ok(()); // already resolved alongside the engine's versions
    }
    std::fs::write(&target, lock).with_context(|| format!("writing {}", target.display()))
}

/// Whether every package two lock files both name is at the same versions.
fn lock_versions_agree(a: &str, b: &str) -> bool {
    fn versions(lock: &str) -> std::collections::BTreeMap<&str, std::collections::BTreeSet<&str>> {
        let mut out: std::collections::BTreeMap<&str, std::collections::BTreeSet<&str>> = Default::default();
        for block in lock.split("[[package]]") {
            let field = |key: &str| {
                block.lines().find_map(|l| l.strip_prefix(key).map(|v| v.trim().trim_matches('"')))
            };
            if let (Some(name), Some(version)) = (field("name = "), field("version = ")) {
                out.entry(name).or_default().insert(version);
            }
        }
        out
    }
    let (a, b) = (versions(a), versions(b));
    a.iter().all(|(name, va)| b.get(name).is_none_or(|vb| va == vb))
}

fn write_if_changed(path: &Path, contents: &str) -> Result<()> {
    if let Ok(existing) = std::fs::read_to_string(path)
        && existing == contents
    {
        return Ok(());
    }
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Cargo-safe crate name.
pub(crate) fn slug(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_lowercase()
}

/// Entry point guess for a package with no `package.toml`.
///
/// Same rule as `PackageManifestInfo::derived_entry_point`, duplicated only
/// for the manifest-less case.
fn derived_entry_point(name: &str) -> String {
    let module = name.replace('-', "_");
    let stem = name.strip_prefix("jengine-").unwrap_or(name);
    let mut camel = String::new();
    for part in stem.split(['-', '_']).filter(|p| !p.is_empty()) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            camel.extend(first.to_uppercase());
            camel.push_str(chars.as_str());
        }
    }
    format!("{module}::{camel}Package")
}

/// Path of `target` relative to `base`, forward-slashed for Cargo, with any
/// Windows verbatim prefix stripped.
///
/// A generated crate sits at `<project>/.jengine/<kind>/` and depends on things
/// *above* it, so this has to emit `../..` segments. The older `pathdiff` here
/// only strips a common prefix — it cannot walk up — and silently returned the
/// target unchanged, which produced a dependency path relative to the current
/// working directory instead of the manifest. That resolves to nothing and the
/// generated crate fails to build.
///
/// Both sides are canonicalized first so `.`, `..` and relative inputs compare
/// correctly; when either cannot be canonicalized (the path does not exist
/// yet), or the two live on different roots, an absolute path is emitted
/// instead. Absolute is portable enough — it is correct on this machine, which
/// is what a generated build artifact needs to be.
fn rel_path(base: &Path, target: &Path) -> String {
    let abs = |p: &Path| -> PathBuf {
        p.canonicalize().unwrap_or_else(|_| {
            std::env::current_dir()
                .map(|cwd| cwd.join(p))
                .unwrap_or_else(|_| p.to_path_buf())
        })
    };
    let (base, target) = (abs(base), abs(target));

    let rel = relative_from(&base, &target).unwrap_or_else(|| target.clone());
    let out = strip_verbatim(&rel).replace('\\', "/");
    if out.is_empty() { ".".to_string() } else { out }
}

/// `target` expressed relative to `base`, walking up with `..` as needed.
///
/// `None` when the two share no root — different Windows drives, say — since
/// no relative path exists between them.
fn relative_from(base: &Path, target: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let mut b = base.components().peekable();
    let mut t = target.components().peekable();

    // Drop the shared prefix.
    while let (Some(x), Some(y)) = (b.peek(), t.peek()) {
        if x == y {
            b.next();
            t.next();
        } else {
            break;
        }
    }
    // Nothing in common at all (different prefix/root) — no relative path.
    if base.components().next() != target.components().next()
        && matches!(base.components().next(), Some(Component::Prefix(_)))
    {
        return None;
    }

    let mut out = PathBuf::new();
    for c in b {
        if matches!(c, Component::Normal(_)) {
            out.push("..");
        }
    }
    for c in t {
        out.push(c.as_os_str());
    }
    Some(out)
}

/// A path as a human should see it, with any Windows verbatim prefix removed.
pub fn display_path(path: &Path) -> String {
    strip_verbatim(path)
}

/// Drop a Windows verbatim path prefix, which Cargo cannot parse in a
/// dependency path.
///
/// `Path::canonicalize` hands back a verbatim path on Windows (`\\?\C:\...`)
/// and Cargo rejects it with "invalid path url". Caught by actually compiling
/// a generated crate rather than by reading the generated text.
fn strip_verbatim(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

// ──────────────────────────── Package Manifest ────────────────────────────

/// Load and parse `package.toml` from a package directory.
fn load_package_manifest(package_dir: &Path) -> Option<PackageManifest> {
    let manifest_path = package_dir.join("package.toml");
    let contents = std::fs::read_to_string(&manifest_path).ok()?;
    match toml::from_str::<PackageManifest>(&contents) {
        Ok(m) => Some(m),
        Err(e) => {
            log::warn!(
                "Failed to parse package.toml at {}: {e}",
                manifest_path.display()
            );
            None
        }
    }
}

// ──────────────────────────── Helpers ────────────────────────────

/// Compute a short hash for a git source (url + tag/rev) for cache directory naming.
fn source_hash(url: &str, tag: Option<&str>, rev: Option<&str>) -> String {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    tag.hash(&mut hasher);
    rev.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Simple relative-path computation: make `target` relative to `base`.
fn pathdiff(base: &Path, target: &Path) -> PathBuf {
    // Try to strip the base prefix; fall back to the absolute path
    match target.strip_prefix(base) {
        Ok(rel) => rel.to_path_buf(),
        Err(_) => target.to_path_buf(),
    }
}

// ──────────────────────────── Script / Asset Package Install ────────────────────────────

/// Install script files from a script package into `assets/packages/<name>/`.
///
/// Script files matching the patterns in `[scripts].files` are copied to the
/// project assets directory. If no manifest exists, defaults to `**/*.lua` and `**/*.js`.
fn install_script_package(
    project_root: &Path,
    name: &str,
    package_dir: &Path,
    manifest: &Option<PackageManifest>,
) -> Result<Vec<PathBuf>> {
    let patterns = manifest
        .as_ref()
        .and_then(|m| m.scripts.as_ref())
        .map(|s| s.files.clone())
        .unwrap_or_else(|| vec!["**/*.lua".to_string(), "**/*.js".to_string()]);

    install_files_by_glob(project_root, name, package_dir, &patterns)
}

/// Install asset files from an asset package into `assets/packages/<name>/`.
///
/// Asset files matching the patterns in `[assets].files` are copied to the
/// project assets directory. If no manifest exists, defaults to common asset types.
fn install_asset_package(
    project_root: &Path,
    name: &str,
    package_dir: &Path,
    manifest: &Option<PackageManifest>,
) -> Result<Vec<PathBuf>> {
    let patterns = manifest
        .as_ref()
        .and_then(|m| m.assets.as_ref())
        .map(|a| a.files.clone())
        .unwrap_or_else(|| {
            vec![
                "**/*.png".to_string(),
                "**/*.jpg".to_string(),
                "**/*.jpeg".to_string(),
                "**/*.ogg".to_string(),
                "**/*.wav".to_string(),
                "**/*.mp3".to_string(),
                // Tracker music (`jengine-tracker`).
                "**/*.song".to_string(),
                "**/*.scene".to_string(),
                "**/*.prefab".to_string(),
                "**/*.gui".to_string(),
            ]
        });

    install_files_by_glob(project_root, name, package_dir, &patterns)
}

/// Copy files matching glob patterns from `package_dir` into `assets/packages/<name>/`.
///
/// Preserves relative directory structure within the package.
/// Returns list of installed file paths (relative to project root).
fn install_files_by_glob(
    project_root: &Path,
    name: &str,
    package_dir: &Path,
    patterns: &[String],
) -> Result<Vec<PathBuf>> {
    let target_dir = project_root.join(PACKAGES_ASSETS_DIR).join(name);
    let mut installed = Vec::new();

    for pattern in patterns {
        let full_pattern = package_dir.join(pattern);
        let full_pattern_str = full_pattern.to_string_lossy();

        let entries = glob::glob(&full_pattern_str)
            .with_context(|| format!("Invalid glob pattern: {pattern}"))?;

        for entry in entries {
            let src = entry.with_context(|| format!("Glob iteration error for pattern: {pattern}"))?;
            if !src.is_file() {
                continue;
            }

            // Compute relative path within the package
            let rel = src
                .strip_prefix(package_dir)
                .unwrap_or(&src);
            let dest = target_dir.join(rel);

            // Create parent directories
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
            }

            std::fs::copy(&src, &dest).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    src.display(),
                    dest.display()
                )
            })?;

            // Store relative path from project root
            let rel_to_project = dest
                .strip_prefix(project_root)
                .unwrap_or(&dest)
                .to_path_buf();
            installed.push(rel_to_project);
        }
    }

    if installed.is_empty() {
        log::warn!("No files matched patterns for package '{name}'");
    }

    Ok(installed)
}

/// Remove installed files for a script/asset package.
///
/// Deletes `assets/packages/<name>/` directory if it exists.
pub fn uninstall_package_assets(project_root: &Path, name: &str) -> Result<()> {
    let target_dir = project_root.join(PACKAGES_ASSETS_DIR).join(name);
    if target_dir.exists() {
        std::fs::remove_dir_all(&target_dir).with_context(|| {
            format!(
                "Failed to remove package assets at {}",
                target_dir.display()
            )
        })?;
        log::info!("Removed installed assets for package '{name}'");
    }
    Ok(())
}

// ──────────────────────────── Add / Remove helpers ────────────────────────────

/// Add a package dependency to `project.jengine`.
pub fn add_package(project_root: &Path, dep: PackageDependency) -> Result<()> {
    let file = project_root.join("project.jengine");
    let contents = std::fs::read_to_string(&file)
        .with_context(|| format!("Failed to read {}", file.display()))?;
    let mut value: serde_json::Value =
        serde_json::from_str(&contents).context("Invalid JSON in project.jengine")?;

    let packages = value
        .as_object_mut()
        .context("project.jengine root must be a JSON object")?
        .entry("packages")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));

    let arr = packages
        .as_array_mut()
        .context("packages must be a JSON array")?;

    // Check for duplicates
    if arr.iter().any(|v| v.get("name").and_then(|n| n.as_str()) == Some(&dep.name)) {
        anyhow::bail!("Package '{}' is already in project.jengine", dep.name);
    }

    arr.push(serde_json::to_value(&dep)?);

    let json = serde_json::to_string_pretty(&value)?;
    std::fs::write(&file, json)
        .with_context(|| format!("Failed to write {}", file.display()))?;

    log::info!("Added package '{}' to project.jengine", dep.name);
    Ok(())
}

/// Remove a package dependency from `project.jengine` by name.
pub fn remove_package(project_root: &Path, name: &str) -> Result<bool> {
    let file = project_root.join("project.jengine");
    let contents = std::fs::read_to_string(&file)
        .with_context(|| format!("Failed to read {}", file.display()))?;
    let mut value: serde_json::Value =
        serde_json::from_str(&contents).context("Invalid JSON in project.jengine")?;

    let removed = if let Some(packages) = value.get_mut("packages").and_then(|v| v.as_array_mut())
    {
        let before = packages.len();
        packages.retain(|v| v.get("name").and_then(|n| n.as_str()) != Some(name));
        packages.len() < before
    } else {
        false
    };

    if removed {
        let json = serde_json::to_string_pretty(&value)?;
        std::fs::write(&file, json)
            .with_context(|| format!("Failed to write {}", file.display()))?;
        log::info!("Removed package '{name}' from project.jengine");
    }

    Ok(removed)
}

/// List packages declared in `project.jengine`.
pub fn list_packages(project_root: &Path) -> Result<Vec<PackageDependency>> {
    read_package_dependencies(project_root)
}

// ──────────────────────────── Package Template ────────────────────────────

/// Scaffold a new package from the built-in template.
///
/// Creates `<output_dir>/<name>/` with `Cargo.toml`, `src/lib.rs`,
/// `package.toml`, and `README.md`. The crate is left for a workspace to
/// claim, as the engine's own `packages/` are; a package made inside a
/// project is [`create_project_package`].
pub fn create_package_from_template(output_dir: &Path, name: &str) -> Result<PathBuf> {
    let link = format!("../../packages/{name}");
    scaffold_rust_package(output_dir, name, false, &link)
}

/// A Rust package for one project: scaffolded into `<project>/packages/<name>`
/// from the built-in template and listed in the project's `project.jengine`,
/// so the next viewport or runtime build links it. The editor's
/// *New Rust Package* comes here.
///
/// The crate is **standalone** (an empty `[workspace]` table): a project
/// inside the engine repo would otherwise be claimed by the engine's
/// workspace, which does not list it, and Cargo refuses to build it.
///
/// Refuses a name Cargo would not accept, one the project already lists, and
/// a folder that already exists - before writing anything.
pub fn create_project_package(project_root: &Path, name: &str) -> Result<PathBuf> {
    validate_package_name(name)?;
    if read_package_dependencies(project_root)?.iter().any(|d| d.name == name) {
        anyhow::bail!("The project already lists a package called '{name}'");
    }
    let link = format!("packages/{name}");
    let pkg_dir = scaffold_rust_package(&project_root.join("packages"), name, true, &link)?;
    add_package(
        project_root,
        PackageDependency { name: name.to_string(), source: PackageSource::Path { path: link }, optional: false },
    )?;
    Ok(pkg_dir)
}

/// Whether `name` can name a package: a crate name Cargo accepts, kept to
/// lowercase letters, digits and single hyphens, since it also becomes a
/// Rust module (`my-pkg` is `my_pkg`) and the scripts' namespace (`MyPkg`).
pub fn validate_package_name(name: &str) -> Result<()> {
    let ok = name.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.ends_with('-')
        && !name.contains("--")
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok {
        anyhow::bail!(
            "'{name}' is not a package name: use lowercase letters, digits and hyphens, starting with a letter (e.g. my-game-rules)"
        );
    }
    if derived_entry_point(name).starts_with("jengine_core::") {
        anyhow::bail!("'{name}' is the engine's own crate");
    }
    // Scripts reach a package's commands through a namespace named from it;
    // one the engine already has (`sprite` is `Sprite`) would be shadowed.
    let namespace = jengine_core::scripting::namespaces::package_namespace(name);
    if jengine_core::scripting::namespaces::core_namespaces().contains(namespace.as_str()) {
        anyhow::bail!("'{name}' would give scripts the namespace {namespace}, which the engine already has");
    }
    Ok(())
}

fn scaffold_rust_package(output_dir: &Path, name: &str, standalone: bool, link_path: &str) -> Result<PathBuf> {
    let pkg_dir = output_dir.join(name);
    if pkg_dir.exists() {
        anyhow::bail!("Directory already exists: {}", pkg_dir.display());
    }

    // Name the type the way the builder will look for it, so a fresh package
    // links without an `entry_point` (it used to be `MyCoolPlugin`, while the
    // builder linked `my_cool_plugin::MyCoolPluginPackage`).
    let entry_point = derived_entry_point(name);
    let struct_name = entry_point.rsplit("::").next().unwrap_or(&entry_point).to_string();
    let namespace = struct_name.trim_end_matches("Package").to_string();
    let component_struct = format!("{namespace}Mover");
    // One token: `package_entries` drops a prefix up to the first `_`, so
    // `coolfx_nudge` is `CoolFx.nudge` where `cool_fx_nudge` would be `CoolFx.fx_nudge`.
    let command_prefix = namespace.to_lowercase();
    let component_key = format!("{command_prefix}_mover");

    // The engine's core crate, relative to where the package actually lands:
    // `packages/<name>` in the engine repo is `../../crates/jengine-core`, but
    // `projects/<game>/packages/<name>` is four levels up, and a fixed path
    // scaffolded a crate there that could not find the engine.
    std::fs::create_dir_all(pkg_dir.join("src"))?;
    let engine = crate::engine_root::locate().map_err(anyhow::Error::msg)?;
    let core_path = rel_path(&pkg_dir, &engine.join("crates/jengine-core"));

    let workspace = if standalone {
        "\n# Standalone: it lives inside a project, not the engine workspace.\n[workspace]\n"
    } else {
        ""
    };
    let fill = |text: &str| {
        text.replace("{{CORE_PATH}}", &core_path)
            .replace("{{WORKSPACE}}", workspace)
            .replace("{{LINK_PATH}}", link_path)
            .replace("{{PACKAGE_NAME}}", name)
            .replace("{{PACKAGE_STRUCT}}", &struct_name)
            .replace("{{ENTRY_POINT}}", &entry_point)
            .replace("{{NAMESPACE}}", &namespace)
            .replace("{{COMPONENT_STRUCT}}", &component_struct)
            .replace("{{COMPONENT_KEY}}", &component_key)
            .replace("{{COMMAND_PREFIX}}", &command_prefix)
    };
    let cargo_toml = fill(include_str!("../../../templates/package/Cargo.toml"));
    let lib_rs = fill(include_str!("../../../templates/package/src/lib.rs"));
    let package_toml = fill(include_str!("../../../templates/package/package.toml"));
    let readme = fill(include_str!("../../../templates/package/README.md"));

    std::fs::create_dir_all(pkg_dir.join("src"))?;
    std::fs::write(pkg_dir.join("Cargo.toml"), cargo_toml)?;
    std::fs::write(pkg_dir.join("src/lib.rs"), lib_rs)?;
    std::fs::write(pkg_dir.join("package.toml"), package_toml)?;
    std::fs::write(pkg_dir.join("README.md"), readme)?;

    log::info!("Created package '{}' at {}", name, pkg_dir.display());
    Ok(pkg_dir)
}

/// Scaffold a new script-only package from the built-in template.
///
/// Creates `<output_dir>/<name>/` with `package.toml`, `scripts/example.lua`,
/// and `README.md`. No Cargo.toml or Rust code.
pub fn create_script_package_from_template(output_dir: &Path, name: &str) -> Result<PathBuf> {
    let pkg_dir = output_dir.join(name);
    if pkg_dir.exists() {
        anyhow::bail!("Directory already exists: {}", pkg_dir.display());
    }

    let package_toml = include_str!("../../../templates/script-package/package.toml")
        .replace("{{PACKAGE_NAME}}", name);
    let example_lua = include_str!("../../../templates/script-package/scripts/example.lua")
        .replace("{{PACKAGE_NAME}}", name);
    let readme = include_str!("../../../templates/script-package/README.md")
        .replace("{{PACKAGE_NAME}}", name);

    std::fs::create_dir_all(pkg_dir.join("scripts"))?;
    std::fs::write(pkg_dir.join("package.toml"), package_toml)?;
    std::fs::write(pkg_dir.join("scripts/example.lua"), example_lua)?;
    std::fs::write(pkg_dir.join("README.md"), readme)?;

    log::info!("Created script package '{}' at {}", name, pkg_dir.display());
    Ok(pkg_dir)
}

/// Scaffold a new asset-only package from the built-in template.
///
/// Creates `<output_dir>/<name>/` with `package.toml`, asset directories,
/// and `README.md`. No Cargo.toml or Rust code.
pub fn create_asset_package_from_template(output_dir: &Path, name: &str) -> Result<PathBuf> {
    let pkg_dir = output_dir.join(name);
    if pkg_dir.exists() {
        anyhow::bail!("Directory already exists: {}", pkg_dir.display());
    }

    let package_toml = include_str!("../../../templates/asset-package/package.toml")
        .replace("{{PACKAGE_NAME}}", name);
    let readme = include_str!("../../../templates/asset-package/README.md")
        .replace("{{PACKAGE_NAME}}", name);

    std::fs::create_dir_all(pkg_dir.join("sprites"))?;
    std::fs::create_dir_all(pkg_dir.join("sounds"))?;
    std::fs::create_dir_all(pkg_dir.join("prefabs"))?;
    std::fs::write(pkg_dir.join("package.toml"), package_toml)?;
    std::fs::write(pkg_dir.join("README.md"), readme)?;

    log::info!("Created asset package '{}' at {}", name, pkg_dir.display());
    Ok(pkg_dir)
}

/// Convert a kebab-case name to PascalCase (e.g. "my-plugin" → "MyPlugin").
#[cfg(test)]
mod tests {
    use super::*;
    use jengine_core::package::manifest::{PackageManifest, PackageManifestInfo, PackageSource, PackageType};

    const ENGINE_LOCK: &str = "version = 4\n\n[[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.127\"\n\n[[package]]\nname = \"glam\"\nversion = \"0.29.3\"\n";

    #[test]
    fn a_generated_crate_builds_against_the_engines_lock() {
        let engine = tempfile::tempdir().unwrap();
        let crate_dir = tempfile::tempdir().unwrap();
        std::fs::write(engine.path().join("Cargo.lock"), ENGINE_LOCK).unwrap();

        // No lock yet: the engine's is copied in.
        seed_engine_lock(crate_dir.path(), engine.path()).unwrap();
        assert_eq!(std::fs::read_to_string(crate_dir.path().join("Cargo.lock")).unwrap(), ENGINE_LOCK);

        // A lock that resolved the engine's versions plus a package of the
        // project's own is kept - cargo added that one alongside.
        let extended = format!("{ENGINE_LOCK}\n[[package]]\nname = \"rotator\"\nversion = \"0.1.0\"\n");
        std::fs::write(crate_dir.path().join("Cargo.lock"), &extended).unwrap();
        seed_engine_lock(crate_dir.path(), engine.path()).unwrap();
        assert_eq!(std::fs::read_to_string(crate_dir.path().join("Cargo.lock")).unwrap(), extended);

        // One that drifted from the engine's versions is replaced.
        let drifted = ENGINE_LOCK.replace("0.2.127", "0.2.129");
        std::fs::write(crate_dir.path().join("Cargo.lock"), drifted).unwrap();
        seed_engine_lock(crate_dir.path(), engine.path()).unwrap();
        assert_eq!(std::fs::read_to_string(crate_dir.path().join("Cargo.lock")).unwrap(), ENGINE_LOCK);
    }

    #[test]
    fn source_hash_deterministic() {
        let h1 = source_hash("https://github.com/a/b.git", Some("v1.0"), None);
        let h2 = source_hash("https://github.com/a/b.git", Some("v1.0"), None);
        assert_eq!(h1, h2);
    }

    #[test]
    fn source_hash_differs_by_tag() {
        let h1 = source_hash("https://github.com/a/b.git", Some("v1.0"), None);
        let h2 = source_hash("https://github.com/a/b.git", Some("v2.0"), None);
        assert_ne!(h1, h2);
    }

    #[test]
    fn generate_cargo_deps_empty() {
        let result =
            generate_cargo_dependencies(Path::new("/project"), &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn generate_cargo_deps_format() {
        let packages = vec![ResolvedPackage {
            name: "my-plugin".to_string(),
            local_path: PathBuf::from("/project/packages/my-plugin"),
            source: PackageSource::Path {
                path: "packages/my-plugin".to_string(),
            },
            manifest: None,
            package_type: PackageType::Full,
            installed_files: Vec::new(),
        }];
        let result =
            generate_cargo_dependencies(Path::new("/project"), &packages).unwrap();
        assert!(result.contains("my-plugin = { path = \"packages/my-plugin\" }"));
    }

    #[test]
    fn generate_cargo_deps_skips_script_packages() {
        let packages = vec![
            ResolvedPackage {
                name: "my-rust-pkg".to_string(),
                local_path: PathBuf::from("/project/packages/my-rust-pkg"),
                source: PackageSource::Path {
                    path: "packages/my-rust-pkg".to_string(),
                },
                manifest: None,
                package_type: PackageType::Full,
                installed_files: Vec::new(),
            },
            ResolvedPackage {
                name: "my-scripts".to_string(),
                local_path: PathBuf::from("/project/packages/my-scripts"),
                source: PackageSource::Path {
                    path: "packages/my-scripts".to_string(),
                },
                manifest: None,
                package_type: PackageType::Script,
                installed_files: Vec::new(),
            },
            ResolvedPackage {
                name: "my-assets".to_string(),
                local_path: PathBuf::from("/project/packages/my-assets"),
                source: PackageSource::Path {
                    path: "packages/my-assets".to_string(),
                },
                manifest: None,
                package_type: PackageType::Asset,
                installed_files: Vec::new(),
            },
        ];
        let result =
            generate_cargo_dependencies(Path::new("/project"), &packages).unwrap();
        assert!(result.contains("my-rust-pkg"));
        assert!(!result.contains("my-scripts"));
        assert!(!result.contains("my-assets"));
    }

    fn fixture_package(root: &Path, name: &str, package_type: PackageType) -> ResolvedPackage {
        ResolvedPackage {
            name: name.to_string(),
            local_path: root.join("packages").join(name),
            source: PackageSource::Path { path: format!("packages/{name}") },
            manifest: None,
            package_type,
            installed_files: Vec::new(),
        }
    }

    #[test]
    fn project_web_crate_links_every_rust_package_behind_one_start() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = tmp.path().join("engine");
        let project = tmp.path().join("projects").join("My Game");
        std::fs::create_dir_all(&project).unwrap();
        let packages = vec![
            fixture_package(tmp.path(), "jengine-alpha", PackageType::Full),
            fixture_package(tmp.path(), "jengine-beta-gamma", PackageType::Full),
            fixture_package(tmp.path(), "some-scripts", PackageType::Script),
        ];

        let manifest = generate_project_web(&project, &engine, &packages)
            .unwrap()
            .expect("a project with Rust packages gets a crate");
        assert_eq!(manifest, project.join(".jengine").join("web").join("Cargo.toml"));
        let cargo = std::fs::read_to_string(&manifest).unwrap();
        let lib = std::fs::read_to_string(manifest.parent().unwrap().join("src/lib.rs")).unwrap();

        // A unique crate name: Cargo refuses one shared with a dependency.
        assert!(cargo.contains("name = \"my-game-web\""), "{cargo}");
        // `standalone` off, so jengine-web's own start function is compiled out.
        assert!(
            cargo.contains("package = \"jengine-web\"") && cargo.contains("default-features = false"),
            "{cargo}"
        );
        // Every feature `build_web` passes is forwarded, or it silently means nothing.
        for feature in ["physics", "backend-webgpu", "backend-webgl"] {
            assert!(
                cargo.contains(&format!("{feature} = [\"jengine-web-lib/{feature}\"]")),
                "{feature} not forwarded:\n{cargo}"
            );
        }
        assert!(cargo.contains("jengine-alpha = { path = "), "{cargo}");
        assert!(cargo.contains("jengine-beta-gamma = { path = "), "{cargo}");
        assert!(!cargo.contains("some-scripts"), "a script package is not a crate:\n{cargo}");

        assert_eq!(lib.matches("wasm_bindgen(start)").count(), 1, "{lib}");
        assert!(lib.contains("host.add_package(Box::new(jengine_alpha::AlphaPackage))"), "{lib}");
        assert!(
            lib.contains("host.add_package(Box::new(jengine_beta_gamma::BetaGammaPackage))"),
            "{lib}"
        );
        assert_eq!(lib.matches("add_package").count(), 2, "{lib}");
        // Packages first, then the runtime: `run` reads them to build the host.
        let registers = lib.find("register_project_package").unwrap();
        let runs = lib.find("jengine_web_lib::run()").expect("the start runs the runtime");
        assert!(registers < runs, "{lib}");
    }

    #[test]
    fn project_web_crate_is_not_generated_without_rust_packages() {
        let tmp = tempfile::tempdir().unwrap();
        let packages = vec![fixture_package(tmp.path(), "some-scripts", PackageType::Script)];
        let generated = generate_project_web(tmp.path(), tmp.path(), &packages).unwrap();
        assert!(generated.is_none());
        assert!(!tmp.path().join(".jengine").join("web").exists());
    }

    #[test]
    fn project_web_crate_refuses_a_native_only_package_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let packages = vec![
            fixture_package(tmp.path(), "jengine-alpha", PackageType::Full),
            fixture_package(tmp.path(), "jengine-audio", PackageType::Full),
        ];
        let err = generate_project_web(tmp.path(), tmp.path(), &packages).unwrap_err();
        assert!(format!("{err:#}").contains("jengine-audio"), "{err:#}");
    }

    #[test]
    fn create_package_template() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg_dir = create_package_from_template(tmp.path(), "my-cool-plugin").unwrap();

        assert!(pkg_dir.join("Cargo.toml").exists());
        assert!(pkg_dir.join("src/lib.rs").exists());
        assert!(pkg_dir.join("package.toml").exists());
        assert!(pkg_dir.join("README.md").exists());

        let lib = std::fs::read_to_string(pkg_dir.join("src/lib.rs")).unwrap();
        // The type the builder will link, with no `entry_point` declared.
        assert_eq!(derived_entry_point("my-cool-plugin"), "my_cool_plugin::MyCoolPluginPackage");
        assert!(lib.contains("pub struct MyCoolPluginPackage;"));
        assert!(lib.contains("my-cool-plugin"));
        assert!(lib.contains("MyCoolPlugin.nudge"), "the namespace scripts call it by");
        assert!(lib.contains("\"mycoolplugin_nudge\""));
        assert!(!lib.contains("{{"), "an unfilled placeholder");
        for file in ["Cargo.toml", "package.toml", "README.md"] {
            let text = std::fs::read_to_string(pkg_dir.join(file)).unwrap();
            assert!(!text.contains("{{"), "an unfilled placeholder in {file}");
        }

        let cargo = std::fs::read_to_string(pkg_dir.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"my-cool-plugin\""));
        // The core path is relative to where the package landed and reaches the engine.
        let core = cargo
            .lines()
            .find_map(|l| l.split("path = \"").nth(1).map(|r| r.split('"').next().unwrap().to_string()))
            .expect("a path dependency on jengine-core");
        assert!(pkg_dir.join(&core).join("Cargo.toml").exists(), "{core} does not reach jengine-core");
    }

    #[test]
    fn a_project_package_is_scaffolded_standalone_and_listed() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("project.jengine"), r#"{ "name": "game", "settings": {} }"#).unwrap();

        let pkg_dir = create_project_package(tmp.path(), "game-rules").unwrap();
        assert_eq!(pkg_dir, tmp.path().join("packages/game-rules"));
        let cargo = std::fs::read_to_string(pkg_dir.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("\n[workspace]\n"), "not standalone:\n{cargo}");
        // Still parses as a manifest, with the table where Cargo wants it.
        let parsed: toml::Value = toml::from_str(&cargo).unwrap();
        assert!(parsed.get("workspace").is_some() && parsed["package"]["name"].as_str() == Some("game-rules"));
        let readme = std::fs::read_to_string(pkg_dir.join("README.md")).unwrap();
        assert!(readme.contains(r#""path": "packages/game-rules""#), "README links the wrong path");

        let deps = read_package_dependencies(tmp.path()).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "game-rules");
        assert!(matches!(&deps[0].source, PackageSource::Path { path } if path == "packages/game-rules"));
        // And it resolves to the folder just written.
        let resolved = resolve_packages(tmp.path()).unwrap();
        assert_eq!(resolved[0].local_path.canonicalize().unwrap(), pkg_dir.canonicalize().unwrap());

        // A second of the same name is refused, and nothing is listed twice.
        assert!(create_project_package(tmp.path(), "game-rules").is_err());
        assert_eq!(read_package_dependencies(tmp.path()).unwrap().len(), 1);
    }

    #[test]
    fn package_names_are_checked_before_anything_is_written() {
        for bad in ["", "Game", "1st", "my_pkg", "my--pkg", "pkg-", "my pkg", "jengine-core", "sprite", "jengine-audio"] {
            assert!(validate_package_name(bad).is_err(), "{bad:?} was accepted");
        }
        for good in ["rules", "my-game-rules", "jengine-tides", "pkg2"] {
            validate_package_name(good).unwrap();
        }
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("project.jengine"), r#"{ "name": "game", "settings": {} }"#).unwrap();
        assert!(create_project_package(tmp.path(), "Bad Name").is_err());
        assert!(!tmp.path().join("packages").exists());
    }

    #[test]
    fn add_and_remove_package() {
        let tmp = tempfile::tempdir().unwrap();
        let project_file = tmp.path().join("project.jengine");
        std::fs::write(
            &project_file,
            r#"{ "name": "test", "settings": {} }"#,
        )
        .unwrap();

        // Add
        let dep = PackageDependency {
            name: "test-pkg".to_string(),
            source: PackageSource::Path {
                path: "packages/test-pkg".to_string(),
            },
            optional: false,
        };
        add_package(tmp.path(), dep).unwrap();

        let deps = list_packages(tmp.path()).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "test-pkg");

        // Duplicate check
        let dup = PackageDependency {
            name: "test-pkg".to_string(),
            source: PackageSource::Path {
                path: "packages/test-pkg".to_string(),
            },
            optional: false,
        };
        assert!(add_package(tmp.path(), dup).is_err());

        // Remove
        let removed = remove_package(tmp.path(), "test-pkg").unwrap();
        assert!(removed);
        let deps = list_packages(tmp.path()).unwrap();
        assert!(deps.is_empty());

        // Remove non-existent
        let removed = remove_package(tmp.path(), "no-such-pkg").unwrap();
        assert!(!removed);
    }

    #[test]
    fn create_script_package_template_works() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg_dir =
            create_script_package_from_template(tmp.path(), "my-script-pack").unwrap();

        assert!(pkg_dir.join("package.toml").exists());
        assert!(pkg_dir.join("scripts/example.lua").exists());
        assert!(pkg_dir.join("README.md").exists());
        // No Cargo.toml — script packages are not Rust crates
        assert!(!pkg_dir.join("Cargo.toml").exists());

        let manifest_str = std::fs::read_to_string(pkg_dir.join("package.toml")).unwrap();
        assert!(manifest_str.contains("my-script-pack"));
        assert!(manifest_str.contains("script"));

        let lua = std::fs::read_to_string(pkg_dir.join("scripts/example.lua")).unwrap();
        assert!(lua.contains("my-script-pack"));

        // Duplicate should fail
        assert!(create_script_package_from_template(tmp.path(), "my-script-pack").is_err());
    }

    #[test]
    fn create_asset_package_template_works() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg_dir =
            create_asset_package_from_template(tmp.path(), "sprite-pack").unwrap();

        assert!(pkg_dir.join("package.toml").exists());
        assert!(pkg_dir.join("README.md").exists());
        assert!(pkg_dir.join("sprites").is_dir());
        assert!(pkg_dir.join("sounds").is_dir());
        assert!(pkg_dir.join("prefabs").is_dir());
        // No Cargo.toml
        assert!(!pkg_dir.join("Cargo.toml").exists());

        let manifest_str = std::fs::read_to_string(pkg_dir.join("package.toml")).unwrap();
        assert!(manifest_str.contains("sprite-pack"));
        assert!(manifest_str.contains("asset"));
    }

    #[test]
    fn install_files_by_glob_copies_matching_files() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path().join("project");
        let package_dir = tmp.path().join("pkg");

        // Set up a fake package with some files
        std::fs::create_dir_all(package_dir.join("scripts")).unwrap();
        std::fs::write(package_dir.join("scripts/init.lua"), "-- init").unwrap();
        std::fs::write(package_dir.join("scripts/helper.lua"), "-- helper").unwrap();
        std::fs::write(package_dir.join("scripts/data.txt"), "not a script").unwrap();
        std::fs::create_dir_all(&project_root).unwrap();

        let patterns = vec!["**/*.lua".to_string()];
        let installed =
            install_files_by_glob(&project_root, "test-pkg", &package_dir, &patterns).unwrap();

        assert_eq!(installed.len(), 2);
        // Files should exist under assets/packages/test-pkg/
        let dest_dir = project_root.join("assets/packages/test-pkg");
        assert!(dest_dir.join("scripts/init.lua").exists());
        assert!(dest_dir.join("scripts/helper.lua").exists());
        // .txt should NOT be copied
        assert!(!dest_dir.join("scripts/data.txt").exists());
    }

    #[test]
    fn install_script_package_uses_manifest_patterns() {
        let tmp = tempfile::tempdir().unwrap();
        let project_root = tmp.path().join("project");
        let package_dir = tmp.path().join("pkg");

        std::fs::create_dir_all(package_dir.join("src")).unwrap();
        std::fs::write(package_dir.join("src/main.lua"), "-- main").unwrap();
        std::fs::write(package_dir.join("src/util.js"), "// util").unwrap();
        std::fs::create_dir_all(&project_root).unwrap();

        let manifest = Some(PackageManifest {
            package: PackageManifestInfo {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
                description: String::new(),
                author: String::new(),
                engine_version: None,
                dependencies: vec![],
                package_type: PackageType::Script,
                entry_point: None,
            },
            scripts: Some(jengine_core::package::manifest::ScriptSection {
                files: vec!["src/**/*.lua".to_string()], // Only Lua, not JS
            }),
            assets: None,
        });

        let installed =
            install_script_package(&project_root, "test-pkg", &package_dir, &manifest)
                .unwrap();

        assert_eq!(installed.len(), 1);
        let dest_dir = project_root.join("assets/packages/test-pkg");
        assert!(dest_dir.join("src/main.lua").exists());
        assert!(!dest_dir.join("src/util.js").exists());
    }

    #[test]
    fn uninstall_package_assets_removes_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg_assets = tmp.path().join("assets/packages/some-pkg");
        std::fs::create_dir_all(pkg_assets.join("scripts")).unwrap();
        std::fs::write(pkg_assets.join("scripts/a.lua"), "x").unwrap();

        assert!(pkg_assets.exists());
        uninstall_package_assets(tmp.path(), "some-pkg").unwrap();
        assert!(!pkg_assets.exists());

        // Removing non-existent is not an error
        uninstall_package_assets(tmp.path(), "some-pkg").unwrap();
    }
}
