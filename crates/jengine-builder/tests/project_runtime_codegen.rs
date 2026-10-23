//! A project that ships Rust packages gets its own runtime crate.
//!
//! A native build is `cargo build -p jengine-runtime` and the shipped game *is*
//! that binary, so before this a third-party package had to be edited into the
//! engine workspace — and building project A then project B would clobber each
//! other's package list. `generate_project_runtime` writes a standalone crate
//! per project instead.

use std::path::{Path, PathBuf};

use jengine_builder::packages::{
    generate_project_android, generate_project_runtime, project_runtime_binary_name,
};
use jengine_core::package::manifest::{
    PackageManifest, PackageManifestInfo, PackageSource, PackageType,
};

fn resolved(
    name: &str,
    dir: &Path,
    kind: PackageType,
    entry: Option<&str>,
) -> jengine_builder::packages::ResolvedPackage {
    jengine_builder::packages::ResolvedPackage {
        name: name.to_string(),
        local_path: dir.to_path_buf(),
        source: PackageSource::Path { path: dir.display().to_string() },
        manifest: Some(PackageManifest {
            package: PackageManifestInfo {
                name: name.to_string(),
                version: "0.1.0".to_string(),
                description: String::new(),
                author: String::new(),
                engine_version: None,
                dependencies: vec![],
                package_type: kind,
                entry_point: entry.map(str::to_string),
            },
            scripts: None,
            assets: None,
        }),
        package_type: kind,
        installed_files: vec![],
    }
}

struct Fixture {
    _tmp: tempfile::TempDir,
    project: PathBuf,
    engine: PathBuf,
    pkg: PathBuf,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("projects").join("my-game");
    let engine = tmp.path().join("engine");
    let pkg = tmp.path().join("packages").join("cool-fx");
    for d in [&project, &engine, &pkg] {
        std::fs::create_dir_all(d).unwrap();
    }
    Fixture { _tmp: tmp, project, engine, pkg }
}

#[test]
fn a_project_with_no_rust_packages_generates_nothing() {
    // Then the caller builds the stock runtime, exactly as before — no crate
    // to keep in sync and no extra compile.
    let f = fixture();
    let asset_only = resolved("my-sounds", &f.pkg, PackageType::Asset, None);

    let out = generate_project_runtime(&f.project, &f.engine, &[asset_only]).unwrap();
    assert!(out.is_none(), "generated a crate for an asset-only package");
    assert!(!f.project.join(".jengine/runtime").exists());
}

#[test]
fn a_rust_package_produces_a_standalone_crate_that_registers_it() {
    let f = fixture();
    let pkg = resolved("cool-fx", &f.pkg, PackageType::Full, None);

    let manifest = generate_project_runtime(&f.project, &f.engine, &[pkg])
        .unwrap()
        .expect("a Full package must generate a crate");
    let toml = std::fs::read_to_string(&manifest).unwrap();
    let main = std::fs::read_to_string(manifest.parent().unwrap().join("src/main.rs")).unwrap();

    // Standalone: without an empty `[workspace]` the enclosing engine
    // workspace would try to claim it and fail on the unlisted member.
    assert!(toml.contains("[workspace]"), "not a standalone workspace:\n{toml}");
    assert!(toml.contains("jengine-runtime = { path ="), "missing runtime dep:\n{toml}");
    assert!(toml.contains("cool-fx = { path ="), "missing package dep:\n{toml}");
    // Backend features forward, so project graphics pinning keeps working.
    assert!(toml.contains("backend-dx12 = [\"jengine-runtime/backend-dx12\"]"), "{toml}");

    // The whole point: the package is constructed and registered.
    assert!(main.contains("run_with"), "{main}");
    assert!(
        main.contains("host.add_package(Box::new(cool_fx::CoolFxPackage));"),
        "entry point not registered:\n{main}"
    );
}

#[test]
fn an_explicit_entry_point_is_used_verbatim() {
    // The derived name is a guess. A package whose type is not
    // `<Name>Package` declares `entry_point` in its package.toml.
    let f = fixture();
    let pkg = resolved("cool-fx", &f.pkg, PackageType::Full, Some("cool_fx::Renderer"));

    let manifest = generate_project_runtime(&f.project, &f.engine, &[pkg]).unwrap().unwrap();
    let main = std::fs::read_to_string(manifest.parent().unwrap().join("src/main.rs")).unwrap();

    assert!(main.contains("Box::new(cool_fx::Renderer)"), "{main}");
    assert!(!main.contains("CoolFxPackage"), "used the guess over the declaration:\n{main}");
}

#[test]
fn only_full_packages_become_dependencies() {
    // Script and asset packages install their files through `resolve` and have
    // no crate to link; listing them would break the build.
    let f = fixture();
    let pkgs = vec![
        resolved("cool-fx", &f.pkg, PackageType::Full, None),
        resolved("my-sounds", &f.pkg, PackageType::Asset, None),
        resolved("my-lib", &f.pkg, PackageType::Script, None),
    ];

    let manifest = generate_project_runtime(&f.project, &f.engine, &pkgs).unwrap().unwrap();
    let toml = std::fs::read_to_string(&manifest).unwrap();

    assert!(toml.contains("cool-fx = { path ="), "{toml}");
    assert!(!toml.contains("my-sounds"), "asset package became a dependency:\n{toml}");
    assert!(!toml.contains("my-lib"), "script package became a dependency:\n{toml}");
}

#[test]
fn regenerating_with_the_same_packages_does_not_touch_the_files() {
    // Rewriting identical content would bump mtimes and force Cargo to rebuild
    // the crate on every single build.
    let f = fixture();
    let pkg = resolved("cool-fx", &f.pkg, PackageType::Full, None);

    let manifest = generate_project_runtime(&f.project, &f.engine, std::slice::from_ref(&pkg))
        .unwrap()
        .unwrap();
    let first = std::fs::metadata(&manifest).unwrap().modified().unwrap();

    generate_project_runtime(&f.project, &f.engine, &[pkg]).unwrap();
    let second = std::fs::metadata(&manifest).unwrap().modified().unwrap();

    assert_eq!(first, second, "manifest rewritten despite identical content");
}

#[test]
fn two_projects_do_not_share_a_runtime_crate() {
    // The reason for this whole design: with the engine crate patched in
    // place, building one project would leave the other's packages linked.
    let f = fixture();
    let other = f.project.parent().unwrap().join("other-game");
    std::fs::create_dir_all(&other).unwrap();

    generate_project_runtime(
        &f.project,
        &f.engine,
        &[resolved("cool-fx", &f.pkg, PackageType::Full, None)],
    )
    .unwrap();
    generate_project_runtime(
        &other,
        &f.engine,
        &[resolved("other-fx", &f.pkg, PackageType::Full, None)],
    )
    .unwrap();

    let a = std::fs::read_to_string(f.project.join(".jengine/runtime/src/main.rs")).unwrap();
    let b = std::fs::read_to_string(other.join(".jengine/runtime/src/main.rs")).unwrap();

    assert!(a.contains("cool_fx") && !a.contains("other_fx"), "{a}");
    assert!(b.contains("other_fx") && !b.contains("cool_fx"), "{b}");
    assert_ne!(
        project_runtime_binary_name(&f.project),
        project_runtime_binary_name(&other),
        "both projects would produce the same binary name"
    );
}

/// The generated crate actually compiles.
///
/// `#[ignore]`d because it builds the whole engine from scratch in a temp
/// directory — minutes, not milliseconds. Run it deliberately after touching
/// the codegen:
///
/// ```text
/// cargo test -p jengine-builder --test project_runtime_codegen -- --ignored --nocapture
/// ```
///
/// Everything above checks the *text* the generator emits; only this checks
/// that the text is a valid crate. Uses `jengine-parallax` as the stand-in
/// third-party package because it is a real `JEnginePackage` with a real
/// entry point.
#[test]
#[ignore = "compiles the engine; run explicitly"]
fn the_generated_crate_compiles() {
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("engine root");
    let parallax = engine.join("packages/jengine-parallax");

    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("my-game");
    std::fs::create_dir_all(&project).unwrap();

    let manifest = generate_project_runtime(
        &project,
        &engine,
        &[resolved("jengine-parallax", &parallax, PackageType::Full, None)],
    )
    .unwrap()
    .expect("generated");

    let out = std::process::Command::new("cargo")
        .args(["build", "--manifest-path"])
        .arg(&manifest)
        .output()
        .expect("cargo runs");

    assert!(
        out.status.success(),
        "generated crate failed to compile:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// ── editor viewport wrapper ────────────────────────────────────────────────

#[test]
fn the_viewport_wrapper_is_a_drop_in_replacement() {
    use jengine_builder::packages::generate_project_viewport;
    let f = fixture();
    let pkg = resolved("cool-fx", &f.pkg, PackageType::Full, None);

    let manifest = generate_project_viewport(&f.project, &f.engine, &[pkg])
        .unwrap()
        .expect("a Full package must generate a viewport wrapper");
    let toml = std::fs::read_to_string(&manifest).unwrap();
    let lib = std::fs::read_to_string(manifest.parent().unwrap().join("src/lib.rs")).unwrap();

    // The crate name must NOT be the viewport's: Cargo refuses two packages
    // with the same name and version in one lockfile ("package collision in
    // the lockfile") and this crate depends on the viewport. The emitted
    // module still gets the stock filename, from `wasm-pack --out-name`.
    assert!(
        toml.contains("name = \"my-game-viewport\""),
        "wrapper crate name collides with its own dependency:\n{toml}"
    );
    // Depending on a crate with your own name requires renaming it.
    assert!(
        toml.contains("jengine-editor-viewport-lib = { package = \"jengine-editor-viewport\""),
        "{toml}"
    );
    assert!(toml.contains("crate-type = [\"cdylib\", \"rlib\"]"), "{toml}");
    assert!(toml.contains("cool-fx = { path ="), "{toml}");

    // Re-export gives the wrapper the viewport's whole JS API.
    assert!(lib.contains("pub use jengine_editor_viewport_lib::*;"), "{lib}");
    // Registration must happen at module init, before `viewport_init` builds
    // the host — anything later is too late.
    assert!(lib.contains("wasm_bindgen(start)"), "{lib}");
    assert!(
        lib.contains("host.add_package(Box::new(cool_fx::CoolFxPackage));"),
        "{lib}"
    );
}

#[test]
fn no_rust_packages_means_no_viewport_wrapper() {
    use jengine_builder::packages::generate_project_viewport;
    let f = fixture();
    let asset_only = resolved("my-sounds", &f.pkg, PackageType::Asset, None);
    assert!(
        generate_project_viewport(&f.project, &f.engine, &[asset_only])
            .unwrap()
            .is_none(),
        "generated a wrapper for a project with no Rust packages"
    );
}

/// The generated viewport wrapper actually compiles for wasm.
///
/// `#[ignore]`d — it builds the viewport and its whole dependency tree for
/// `wasm32-unknown-unknown`, which takes minutes. Run it after touching the
/// codegen:
///
/// ```text
/// cargo test -p jengine-builder --test project_runtime_codegen -- --ignored
/// ```
///
/// The text assertions above passed on a *native* generated crate that did not
/// compile at all (Cargo rejects the verbatim path Windows `canonicalize`
/// returns), so checking the emitted text is not evidence.
#[test]
#[ignore = "compiles the viewport for wasm; run explicitly"]
fn the_generated_viewport_wrapper_compiles() {
    use jengine_builder::packages::generate_project_viewport;
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("engine root");
    let parallax = engine.join("packages/jengine-parallax");

    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("my-game");
    std::fs::create_dir_all(&project).unwrap();

    let manifest = generate_project_viewport(
        &project,
        &engine,
        &[resolved("jengine-parallax", &parallax, PackageType::Full, None)],
    )
    .unwrap()
    .expect("generated");

    let out = std::process::Command::new("cargo")
        .args(["build", "--target", "wasm32-unknown-unknown", "--manifest-path"])
        .arg(&manifest)
        .output()
        .expect("cargo runs");

    assert!(
        out.status.success(),
        "generated viewport wrapper failed to compile:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Android has no stock binary to fall back to - the activity loads
/// `libmain.so` - so the crate is generated whether or not there are packages.
#[test]
fn an_android_crate_is_generated_even_without_packages() {
    let f = fixture();
    let features = vec!["backend-vulkan".to_string()];
    let manifest = generate_project_android(&f.project, &f.engine, &[], &features).unwrap();
    let toml = std::fs::read_to_string(&manifest).unwrap();
    let lib = std::fs::read_to_string(manifest.parent().unwrap().join("src/lib.rs")).unwrap();

    assert!(toml.contains("[workspace]"), "not a standalone workspace:
{toml}");
    // NativeActivity looks for `lib<android.app.lib_name>.so`, and the
    // builder writes `main` into the manifest.
    assert!(toml.contains("name = \"main\""), "{toml}");
    assert!(toml.contains("crate-type = [\"cdylib\"]"), "{toml}");
    assert!(
        toml.contains("default-features = false, features = [\"backend-vulkan\"]"),
        "backends not passed through:
{toml}"
    );
    // The symbol android-activity calls, unmangled.
    assert!(lib.contains("#[unsafe(no_mangle)]
fn android_main("), "{lib}");
    assert!(lib.contains("jengine_runtime::android::run_with(app, &|_host| {"), "{lib}");
}

#[test]
fn an_android_crate_registers_the_project_packages() {
    let f = fixture();
    let pkg = resolved("cool-fx", &f.pkg, PackageType::Full, None);
    let manifest = generate_project_android(&f.project, &f.engine, &[pkg], &[]).unwrap();
    let toml = std::fs::read_to_string(&manifest).unwrap();
    let lib = std::fs::read_to_string(manifest.parent().unwrap().join("src/lib.rs")).unwrap();

    assert!(toml.contains("cool-fx = { path ="), "missing package dep:
{toml}");
    assert!(
        lib.contains("host.add_package(Box::new(cool_fx::CoolFxPackage));"),
        "entry point not registered:
{lib}"
    );
    // Beside the native runtime crate, not on top of it.
    assert!(manifest.ends_with(Path::new(".jengine").join("android").join("Cargo.toml")));
}
