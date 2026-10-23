//! Android build — the native runtime as an installable APK.
//!
//! No Gradle. The app is a NativeActivity with no Java code at all, so an APK
//! is only a manifest, a few icon resources, `lib/<abi>/libmain.so` and the
//! game's files - and the SDK's build-tools make that directly:
//!
//! 1. `cargo ndk` builds the generated crate
//!    ([`crate::packages::generate_project_android`]) into `libmain.so` per ABI.
//! 2. The game is staged exactly as a Windows build lays it out
//!    ([`crate::stage::stage_game`]), plus a file list the runtime unpacks from
//!    (`jengine_runtime::android::MANIFEST`).
//! 3. `aapt2` compiles the icons and links the manifest into a base APK.
//! 4. The libraries and game files are added to it here, with the `zip`
//!    crate. Not with `aapt2 -A`: `aapt2` silently skips any directory whose
//!    name starts with `_`, and the asset pipeline writes `sprites/_atlases/`.
//! 5. `zipalign -P 16`, then `apksigner` with the debug key.
//!
//! With `settings.android.immersive` (the default) the APK also carries one
//! small Java class, `assets/android/JEngineActivity.java` - a NativeActivity
//! that hides the system bars, which only the UI thread may do - compiled with
//! `javac` and `d8` into `classes.dex`.
//!
//! The native libraries are **stored uncompressed and loaded from the APK in
//! place** (`extractNativeLibs="false"`, `zipalign -P 16` aligning them to
//! 16 KB), which is what Google recommends and also what works: unpacked to
//! a file of its own, `libmain.so` crashed the dynamic linker with a SIGBUS
//! on a 16 KB-page emulator, which zero-fills the rest of the page holding
//! the end of the writable segment - past the end of the file. Inside the
//! APK the file does not end there.
//!
//! The result installs on any phone or emulator with `adb install`, signed
//! with the debug key unless asked otherwise. For a store:
//!
//! - **`release_key`** signs with the project's own key
//!   (`settings.android.keystore` / `key_alias`); the passwords come from the
//!   caller, never the project file, and reach `apksigner` / `jarsigner` only
//!   through their own environment.
//! - **`bundle`** also writes the Android App Bundle Google Play takes: the
//!   same parts, linked by `aapt2 --proto-format` into the module layout
//!   `bundletool` wants (`manifest/`, `dex/`, `lib/`, `assets/`, `res/`), then
//!   `bundletool build-bundle` and `jarsigner`. `bundletool` is one jar Google
//!   publishes on GitHub; the builder looks for it in `BUNDLETOOL_JAR` or
//!   `<sdk>/bundletool/`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::app::{Orientation, ResolvedApp};

/// What to build and what to do with it.
#[derive(Debug, Clone)]
pub struct AndroidBuildOptions {
    /// Optimised Rust. The APK is debug-signed either way.
    pub release: bool,
    pub debug_overlay: bool,
    /// Replaces `settings.android.abis` for this build - `x86_64` for the
    /// emulator, say, without editing the project.
    pub abis: Option<Vec<String>>,
    /// `adb install` the APK onto the one connected device or emulator.
    pub install: bool,
    /// Start it after installing.
    pub launch: bool,
    /// Sign with the project's release key instead of the debug key.
    pub release_key: bool,
    /// Also write an Android App Bundle (`.aab`) - what Google Play takes.
    pub bundle: bool,
    /// The release keystore's password. Required with `release_key`.
    pub keystore_password: Option<String>,
    /// The key's own password, when it differs from the keystore's.
    pub key_password: Option<String>,
}

impl Default for AndroidBuildOptions {
    fn default() -> Self {
        Self {
            release: true,
            debug_overlay: true,
            abis: None,
            install: false,
            launch: false,
            release_key: false,
            bundle: false,
            keystore_password: None,
            key_password: None,
        }
    }
}

/// What a build produced.
#[derive(Debug, Clone)]
pub struct AndroidBuildOutput {
    pub apk: PathBuf,
    /// The App Bundle, when one was asked for.
    pub bundle: Option<PathBuf>,
    /// Signed with the release key (a store will take it) or the debug key.
    pub release_signed: bool,
}

/// The SDK pieces a build uses.
#[derive(Debug, Clone)]
pub struct AndroidSdk {
    pub root: PathBuf,
    pub ndk: PathBuf,
    pub build_tools: PathBuf,
}

/// The file the runtime unpacks from - `jengine_runtime::android::MANIFEST`,
/// which this crate cannot name without depending on the runtime.
const ASSET_MANIFEST: &str = "jengine-files.txt";

/// The immersive activity's source, and the class the manifest names.
const IMMERSIVE_ACTIVITY_SRC: &str = include_str!("../assets/android/JEngineActivity.java");
const IMMERSIVE_ACTIVITY: &str = "com.jengine.runtime.JEngineActivity";

/// The activity the manifest declares and `--run` starts.
pub fn activity_class(app: &ResolvedApp) -> &'static str {
    if app.immersive { IMMERSIVE_ACTIVITY } else { "android.app.NativeActivity" }
}

/// Build an APK for `project_path` into `output_path`, returning its path.
pub fn build_android(
    engine_path: &Path,
    project_path: &Path,
    output_path: &Path,
    opts: &AndroidBuildOptions,
) -> Result<AndroidBuildOutput> {
    let mut app = crate::app::read(project_path)?;
    if let Some(abis) = &opts.abis {
        app.abis = abis.clone();
    }
    if app.id_defaulted {
        log::warn!(
            "settings.app.id is not set; using '{}'. Set one before publishing - it cannot change after.",
            app.id
        );
    }
    // Refused before anything is compiled: a release build that fails at the
    // signing step has wasted the whole build.
    let signer = if opts.release_key {
        Signer::release(project_path, &app, opts)?
    } else {
        Signer::debug()?
    };
    let sdk = find_sdk()?;
    let bundletool = if opts.bundle { Some(find_bundletool(&sdk)?) } else { None };
    let platform_jar = sdk.root.join("platforms").join(format!("android-{}", app.target_sdk)).join("android.jar");
    if !platform_jar.exists() {
        bail!(
            "Android platform {} is not installed ({} is missing). Install it with the SDK manager, \
             or set settings.android.target_sdk to one that is.",
            app.target_sdk,
            platform_jar.display()
        );
    }
    log::info!(
        "Building Android {} ({}) v{} ({}) for {} - NDK {}, build-tools {}",
        app.name,
        app.id,
        app.version,
        app.build,
        app.abis.join(", "),
        file_name(&sdk.ndk),
        file_name(&sdk.build_tools),
    );

    let work = output_path.join("intermediates");
    std::fs::create_dir_all(&work)?;

    // 1. Native library.
    let libs = work.join("lib");
    compile_native(engine_path, project_path, &sdk, &app, opts.release, &libs)?;

    // 2. Game files.
    let game = work.join("game");
    if game.exists() {
        std::fs::remove_dir_all(&game).with_context(|| format!("clearing {}", game.display()))?;
    }
    let packs = crate::asset_packs::read_settings(project_path);
    let packs_out = crate::asset_packs::packs_beside_bundle(output_path, "Android", &packs);
    crate::stage::stage_game(project_path, &game, opts.debug_overlay, &packs_out)?;
    let files = write_asset_manifest(&game, &app)?;

    // 3. Icons and manifest.
    let res = work.join("res");
    if res.exists() {
        std::fs::remove_dir_all(&res)?;
    }
    let has_icon = match &app.icon {
        Some(icon) => {
            write_icons(&project_path.join(icon), app.icon_background, &res)?;
            true
        }
        None => {
            log::info!("settings.app.icon is not set; the APK uses Android's default icon");
            false
        }
    };
    let internet = crate::packages::resolve_packages(project_path)
        .unwrap_or_default()
        .iter()
        .any(|p| p.name == "jengine-net");
    let manifest_path = work.join("AndroidManifest.xml");
    std::fs::write(&manifest_path, android_manifest(&app, has_icon, internet, !signer.release))?;

    crate::progress::step("Linking the APK...");
    let compiled_res = if has_icon {
        let compiled = work.join("res.zip");
        let mut compile = Command::new(sdk.build_tools.join(exe("aapt2")));
        compile.arg("compile").arg("--dir").arg(&res).arg("-o").arg(&compiled);
        run(&mut compile, "aapt2 compile")?;
        Some(compiled)
    } else {
        None
    };
    let link = |out: &Path, proto: bool| {
        aapt2_link(&sdk, &platform_jar, &manifest_path, &app, compiled_res.as_deref(), out, proto)
    };
    let base_apk = work.join("base.apk");
    link(&base_apk, false)?;

    // The one class an immersive build carries.
    let dex = if app.immersive {
        Some(compile_activity(&sdk, &platform_jar, app.min_sdk, &work)?)
    } else {
        None
    };

    // 4. Libraries, code and game files.
    crate::progress::step(&format!("Packing {} game file(s) into the APK...", files.len()));
    let unaligned = work.join("unaligned.apk");
    pack_apk(&base_apk, &libs, dex.as_deref(), &game, &files, &unaligned)?;

    // 5. Align and sign.
    let aligned = work.join("aligned.apk");
    run(
        Command::new(sdk.build_tools.join(exe("zipalign")))
            // -P 16: uncompressed libraries on 16 KB page boundaries.
            .args(["-P", "16", "-f", "4"])
            .arg(&unaligned)
            .arg(&aligned),
        "zipalign",
    )?;
    std::fs::create_dir_all(output_path)?;
    let apk = output_path.join(format!("{}-{}.apk", app.id, app.version));
    signer.sign_apk(&sdk, &aligned, &apk)?;
    let signed_with = if signer.release { "release-signed" } else { "debug-signed" };
    let size = std::fs::metadata(&apk).map(|m| m.len()).unwrap_or(0);
    crate::progress::step(&format!(
        "Android build complete: {} ({:.1} MB, {signed_with})",
        crate::packages::display_path(&apk),
        size as f64 / 1_048_576.0
    ));

    // 6. The App Bundle, from the same parts.
    let bundle = match &bundletool {
        Some(bundletool) => {
            crate::progress::step("Building the App Bundle...");
            let proto = work.join("base-proto.zip");
            link(&proto, true)?;
            let module = work.join("base-module.zip");
            pack_bundle_module(&proto, &libs, dex.as_deref(), &game, &files, &module)?;
            let aab = output_path.join(format!("{}-{}.aab", app.id, app.version));
            if aab.exists() {
                std::fs::remove_file(&aab)?;
            }
            run(
                Command::new("java")
                    .arg("-jar")
                    .arg(bundletool)
                    .arg("build-bundle")
                    .arg(format!("--modules={}", module.display()))
                    .arg(format!("--output={}", aab.display())),
                "bundletool build-bundle",
            )?;
            signer.sign_bundle(&aab)?;
            let size = std::fs::metadata(&aab).map(|m| m.len()).unwrap_or(0);
            crate::progress::step(&format!(
                "App Bundle: {} ({:.1} MB, {signed_with}){}",
                crate::packages::display_path(&aab),
                size as f64 / 1_048_576.0,
                if signer.release { "" } else { " - Google Play wants the release key" }
            ));
            Some(aab)
        }
        None => None,
    };

    if opts.install || opts.launch {
        let adb = sdk.root.join("platform-tools").join(exe("adb"));
        crate::progress::step("Installing with adb...");
        run(Command::new(&adb).arg("install").arg("-r").arg(&apk), "adb install")?;
        if opts.launch {
            run(
                Command::new(&adb).args([
                    "shell",
                    "am",
                    "start",
                    "-n",
                    &format!("{}/{}", app.id, activity_class(&app)),
                ]),
                "adb shell am start",
            )?;
            log::info!("Started. Its log: adb logcat -s jengine");
        }
    }
    Ok(AndroidBuildOutput { apk, bundle, release_signed: signer.release })
}

/// Link the manifest and icon resources with `aapt2`: an APK to add code,
/// libraries and assets to, or with `proto` the protobuf form an App Bundle
/// module is made from.
fn aapt2_link(
    sdk: &AndroidSdk,
    platform_jar: &Path,
    manifest: &Path,
    app: &ResolvedApp,
    compiled_res: Option<&Path>,
    out: &Path,
    proto: bool,
) -> Result<()> {
    let mut link = Command::new(sdk.build_tools.join(exe("aapt2")));
    link.arg("link")
        .arg("-o")
        .arg(out)
        .arg("-I")
        .arg(platform_jar)
        .arg("--manifest")
        .arg(manifest)
        .args(["--min-sdk-version", &app.min_sdk.to_string()])
        .args(["--target-sdk-version", &app.target_sdk.to_string()])
        .args(["--version-code", &app.build.to_string()])
        .args(["--version-name", &app.version]);
    if proto {
        link.arg("--proto-format");
    }
    if let Some(res) = compiled_res {
        link.arg(res);
    }
    run(&mut link, if proto { "aapt2 link --proto-format" } else { "aapt2 link" })
}

// ─────────────────────────────── Signing ─────────────────────────────────────

/// The key a build is signed with.
///
/// Passwords are handed to `apksigner` and `jarsigner` in **their** process
/// environment (`--ks-pass env:...`, `-storepass:env ...`), so they appear on
/// no command line a process listing would show, and never in this one's
/// environment either.
struct Signer {
    keystore: PathBuf,
    alias: String,
    store_pass: String,
    key_pass: String,
    release: bool,
}

const STORE_PASS_VAR: &str = "JENGINE_SIGN_STORE_PASS";
const KEY_PASS_VAR: &str = "JENGINE_SIGN_KEY_PASS";

impl Signer {
    fn debug() -> Result<Self> {
        Ok(Self {
            keystore: debug_keystore()?,
            alias: "androiddebugkey".into(),
            store_pass: "android".into(),
            key_pass: "android".into(),
            release: false,
        })
    }

    /// The project's release key - refused, with the reason, rather than
    /// quietly falling back to the debug key, which a store would reject
    /// only after the upload.
    fn release(project: &Path, app: &ResolvedApp, opts: &AndroidBuildOptions) -> Result<Self> {
        if app.id_defaulted {
            bail!(
                "a release build needs its own app id - '{}' was made up from the project name. \
                 Set settings.app.id (Project Settings -> Mobile App); it cannot change after \
                 the first upload.",
                app.id
            );
        }
        let Some(keystore) = &app.keystore else {
            bail!(
                "no release keystore: set settings.android.keystore and key_alias \
                 (Project Settings -> Mobile App). docs/mobile-platforms-proposal.md says how \
                 to make one."
            );
        };
        if app.key_alias.is_empty() {
            bail!("settings.android.key_alias is empty - which key in {keystore}?");
        }
        let path = project.join(keystore);
        if !path.is_file() {
            bail!("the release keystore {} does not exist", path.display());
        }
        let store_pass = opts
            .keystore_password
            .clone()
            .filter(|p| !p.is_empty())
            .context("a release build needs the keystore password")?;
        let key_pass = opts.key_password.clone().filter(|p| !p.is_empty()).unwrap_or_else(|| store_pass.clone());
        Ok(Self { keystore: path, alias: app.key_alias.clone(), store_pass, key_pass, release: true })
    }

    fn sign_apk(&self, sdk: &AndroidSdk, input: &Path, out: &Path) -> Result<()> {
        run(
            Command::new(sdk.build_tools.join(script("apksigner")))
                .arg("sign")
                .arg("--ks")
                .arg(&self.keystore)
                .args(["--ks-key-alias", &self.alias])
                .args(["--ks-pass", &format!("env:{STORE_PASS_VAR}")])
                .args(["--key-pass", &format!("env:{KEY_PASS_VAR}")])
                .arg("--out")
                .arg(out)
                .arg(input)
                .env(STORE_PASS_VAR, &self.store_pass)
                .env(KEY_PASS_VAR, &self.key_pass),
            "apksigner",
        )
    }

    /// An App Bundle is signed as a jar, which is what Play checks.
    fn sign_bundle(&self, aab: &Path) -> Result<()> {
        run(
            Command::new("jarsigner")
                .args(["-keystore"])
                .arg(&self.keystore)
                .args(["-storepass:env", STORE_PASS_VAR, "-keypass:env", KEY_PASS_VAR])
                .args(["-sigalg", "SHA256withRSA", "-digestalg", "SHA-256"])
                .arg(aab)
                .arg(&self.alias)
                .env(STORE_PASS_VAR, &self.store_pass)
                .env(KEY_PASS_VAR, &self.key_pass),
            "jarsigner (from a JDK)",
        )
    }
}

// ─────────────────────────────── App Bundle ──────────────────────────────────

/// Find `bundletool`: `BUNDLETOOL_JAR`, else the newest jar in
/// `<sdk>/bundletool/`.
fn find_bundletool(sdk: &AndroidSdk) -> Result<PathBuf> {
    if let Some(jar) = std::env::var_os("BUNDLETOOL_JAR").map(PathBuf::from).filter(|p| p.is_file()) {
        return Ok(jar);
    }
    let dir = sdk.root.join("bundletool");
    std::fs::read_dir(&dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jar"))
        .max_by_key(|p| version_key(file_name(p).trim_start_matches(|c: char| !c.is_ascii_digit())))
        .with_context(|| {
            format!(
                "an App Bundle needs bundletool, which the Android SDK does not include: put \
                 bundletool-all-<version>.jar from https://github.com/google/bundletool/releases \
                 in {}, or point BUNDLETOOL_JAR at it",
                dir.display()
            )
        })
}

/// Lay out the base module `bundletool build-bundle` takes: the proto-linked
/// manifest under `manifest/`, its resources as they are, and the code,
/// libraries and game files in their module directories.
fn pack_bundle_module(
    proto: &Path,
    libs: &Path,
    dex: Option<&Path>,
    game: &Path,
    files: &[String],
    out: &Path,
) -> Result<()> {
    use zip::write::SimpleFileOptions;

    let mut archive = zip::ZipArchive::new(std::fs::File::open(proto)?)?;
    let mut writer = zip::ZipWriter::new(std::fs::File::create(out)?);
    let deflate = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        let target = if name == "AndroidManifest.xml" { "manifest/AndroidManifest.xml".to_string() } else { name };
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        writer.start_file(target, deflate)?;
        writer.write_all(&bytes)?;
    }
    if let Some(dex) = dex {
        writer.start_file("dex/classes.dex", deflate)?;
        writer.write_all(&std::fs::read(dex)?)?;
    }
    for entry in std::fs::read_dir(libs)? {
        let abi_dir = entry?.path();
        if !abi_dir.is_dir() {
            continue;
        }
        let abi = file_name(&abi_dir);
        for lib in std::fs::read_dir(&abi_dir)? {
            let lib = lib?.path();
            writer.start_file(format!("lib/{abi}/{}", file_name(&lib)), deflate)?;
            writer.write_all(&std::fs::read(&lib)?)?;
        }
    }
    for rel in files {
        writer.start_file(format!("assets/{rel}"), deflate)?;
        writer.write_all(&std::fs::read(game.join(rel))?)?;
    }
    writer.finish()?;
    Ok(())
}

// ─────────────────────────────── SDK discovery ───────────────────────────────

/// Find the SDK, an NDK new enough for current Rust, and the newest
/// build-tools. The environment wins (`ANDROID_HOME`, `ANDROID_NDK_HOME`),
/// then the place Android Studio installs to.
pub fn find_sdk() -> Result<AndroidSdk> {
    let root = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .chain(default_sdk_location())
        .find(|p| p.is_dir())
        .context(
            "no Android SDK found. Install Android Studio (or the command-line tools) and set \
             ANDROID_HOME if it is not in the default place.",
        )?;

    let ndk = match ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .find(|p| p.is_dir())
    {
        Some(p) => p,
        None => {
            let newest = newest_version_dir(&root.join("ndk"))
                .with_context(|| format!("no NDK under {}", root.join("ndk").display()))?;
            let major = version_key(&file_name(&newest)).first().copied().unwrap_or(0);
            // Rust's Android targets link against libunwind, which the NDK
            // has shipped instead of libgcc since r23; r25 is the oldest
            // cargo-ndk supports.
            if major < 25 {
                bail!(
                    "the newest NDK under {} is {}, and Rust needs r25 or later (NDK 25.x). \
                     Install a newer one, or point ANDROID_NDK_HOME at it.",
                    root.join("ndk").display(),
                    file_name(&newest)
                );
            }
            newest
        }
    };

    let build_tools = newest_version_dir(&root.join("build-tools"))
        .filter(|p| p.join(exe("aapt2")).exists())
        .with_context(|| format!("no build-tools with aapt2 under {}", root.join("build-tools").display()))?;

    Ok(AndroidSdk { root, ndk, build_tools })
}

fn default_sdk_location() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Android").join("Sdk"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|d| PathBuf::from(d).join("Library/Android/sdk"))
    } else {
        std::env::var_os("HOME").map(|d| PathBuf::from(d).join("Android/Sdk"))
    }
}

/// The subdirectory with the highest dotted version number.
fn newest_version_dir(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .max_by_key(|p| version_key(&file_name(p)))
}

fn version_key(name: &str) -> Vec<u32> {
    name.split(['.', '-']).map(|s| s.parse().unwrap_or(0)).collect()
}

fn file_name(p: &Path) -> String {
    p.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_string()
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// SDK tools that are shell scripts wrapping a jar (`apksigner`).
fn script(name: &str) -> String {
    if cfg!(windows) { format!("{name}.bat") } else { name.to_string() }
}

fn run(cmd: &mut Command, what: &str) -> Result<()> {
    let status = crate::progress::run_streaming(cmd, what)
        .with_context(|| format!("running {what} - is it installed?"))?;
    if !status.success() {
        bail!("{what} failed with {status}");
    }
    Ok(())
}

// ─────────────────────────────── Native library ──────────────────────────────

fn compile_native(
    engine_path: &Path,
    project_path: &Path,
    sdk: &AndroidSdk,
    app: &ResolvedApp,
    release: bool,
    libs_out: &Path,
) -> Result<()> {
    let packages = match crate::packages::resolve_packages(project_path) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("package resolution failed, building with no project packages: {e:#}");
            Vec::new()
        }
    };
    let backends = crate::graphics::backends_for_platform(project_path, "android")
        .unwrap_or_else(|| vec!["vulkan".to_string()]);
    let mut features = crate::graphics::features_for(&backends);
    features.retain(|f| {
        let ok = f == "backend-vulkan";
        if !ok {
            log::warn!("graphics backend '{f}' does not exist on Android - ignored");
        }
        ok
    });
    if features.is_empty() {
        features.push("backend-vulkan".to_string());
    }
    let manifest = crate::packages::generate_project_android(project_path, engine_path, &packages, &features)?;
    let crate_dir = manifest.parent().expect("generated manifest has a parent");

    if libs_out.exists() {
        std::fs::remove_dir_all(libs_out)?;
    }
    crate::progress::step(&format!(
        "Compiling libmain.so for {} ({})...",
        app.abis.join(", "),
        if release { "release" } else { "debug" }
    ));
    let mut cmd = Command::new("cargo");
    cmd.arg("ndk");
    for abi in &app.abis {
        cmd.args(["-t", abi]);
    }
    cmd.args(["-P", &app.min_sdk.to_string()])
        .arg("-o")
        .arg(libs_out)
        .arg("build");
    if release {
        cmd.arg("--release");
    }
    cmd.current_dir(crate_dir)
        .env("ANDROID_HOME", &sdk.root)
        .env("ANDROID_NDK_HOME", &sdk.ndk)
        // Android 15 devices may use 16 KB memory pages, and a library laid
        // out for 4 KB ones does not load there. NDK r28 makes this the
        // default; r27 needs asking.
        .env("RUSTFLAGS", "-C link-arg=-Wl,-z,max-page-size=16384");
    let status = crate::progress::run_streaming(&mut cmd, "cargo ndk")
        .context("running cargo ndk - install it with `cargo install cargo-ndk`")?;
    if !status.success() {
        bail!("cargo ndk build failed with {status}");
    }
    for abi in &app.abis {
        let so = libs_out.join(abi).join("libmain.so");
        if !so.exists() {
            bail!("cargo ndk finished but {} is missing", so.display());
        }
        bundle_libcxx(sdk, abi, &so)?;
    }
    Ok(())
}

/// Ship the NDK's C++ runtime beside `libmain.so` when it links against it.
///
/// Luau is C++, and the `cc` crate links C++ against the *shared* runtime on
/// Android. That library is not part of Android, so an APK without it
/// installs fine and then dies at launch with the linker failing to find
/// `libc++_shared.so` - and cargo-ndk does not copy it. The name is in the
/// library's dynamic string table whenever it is needed, which is a cheaper
/// test than parsing the ELF.
fn bundle_libcxx(sdk: &AndroidSdk, abi: &str, libmain: &Path) -> Result<()> {
    const LIBCXX: &str = "libc++_shared.so";
    let dest = libmain.with_file_name(LIBCXX);
    if dest.exists() {
        return Ok(());
    }
    let bytes = std::fs::read(libmain)?;
    if !bytes.windows(LIBCXX.len()).any(|w| w == LIBCXX.as_bytes()) {
        return Ok(());
    }
    let triple = match abi {
        "arm64-v8a" => "aarch64-linux-android",
        "x86_64" => "x86_64-linux-android",
        "armeabi-v7a" => "arm-linux-androideabi",
        "x86" => "i686-linux-android",
        other => bail!("no NDK triple for ABI {other}"),
    };
    // `prebuilt/` holds one directory, named for the host (windows-x86_64,
    // linux-x86_64, darwin-x86_64).
    let prebuilt = sdk.ndk.join("toolchains").join("llvm").join("prebuilt");
    let host = std::fs::read_dir(&prebuilt)
        .with_context(|| format!("reading {}", prebuilt.display()))?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .with_context(|| format!("no host toolchain under {}", prebuilt.display()))?;
    let source = host.join("sysroot").join("usr").join("lib").join(triple).join(LIBCXX);
    std::fs::copy(&source, &dest)
        .with_context(|| format!("copying {} (libmain.so needs it)", source.display()))?;
    log::info!("bundled {LIBCXX} for {abi}");
    Ok(())
}

// ─────────────────────────────── Game files ──────────────────────────────────

/// Write the list the runtime unpacks from, returning the files in it.
///
/// The first line identifies the build, so a phone that already unpacked this
/// exact build skips the copy and one holding an older one replaces it.
fn write_asset_manifest(game: &Path, app: &ResolvedApp) -> Result<Vec<String>> {
    let mut files = Vec::new();
    collect_files(game, game, &mut files)?;
    files.retain(|f| f != ASSET_MANIFEST);
    files.sort();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let mut text = format!("build {}+{} {stamp}\n", app.version, app.build);
    for f in &files {
        text.push_str(f);
        text.push('\n');
    }
    std::fs::write(game.join(ASSET_MANIFEST), text)?;
    files.push(ASSET_MANIFEST.to_string());
    Ok(files)
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else {
            let rel = path.strip_prefix(root).expect("walked from root");
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// Compile the immersive activity into a `classes.dex`, returning its path.
///
/// `javac --release 8` against the target platform's `android.jar` (the
/// newest bytecode `d8` reads without complaint from every JDK here), then
/// `d8` for the device. Both come from tools a build already needs: the JDK
/// for `apksigner`, the build-tools for `aapt2`.
fn compile_activity(sdk: &AndroidSdk, platform_jar: &Path, min_sdk: u32, work: &Path) -> Result<PathBuf> {
    let dir = work.join("java");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    let src = dir.join("src").join("com").join("jengine").join("runtime");
    std::fs::create_dir_all(&src)?;
    let java = src.join("JEngineActivity.java");
    std::fs::write(&java, IMMERSIVE_ACTIVITY_SRC)?;
    let classes = dir.join("classes");
    std::fs::create_dir_all(&classes)?;
    run(
        Command::new("javac")
            .args(["--release", "8", "-nowarn", "-Xlint:-options", "-classpath"])
            .arg(platform_jar)
            .arg("-d")
            .arg(&classes)
            .arg(&java),
        "javac (from a JDK)",
    )?;
    let mut class_files = Vec::new();
    collect_files(&classes, &classes, &mut class_files)?;
    let dex_dir = dir.join("dex");
    std::fs::create_dir_all(&dex_dir)?;
    let mut d8 = Command::new(sdk.build_tools.join(script("d8")));
    d8.arg("--release")
        .arg("--lib")
        .arg(platform_jar)
        .args(["--min-api", &min_sdk.to_string()])
        .arg("--output")
        .arg(&dex_dir);
    for f in &class_files {
        d8.arg(classes.join(f));
    }
    run(&mut d8, "d8")?;
    let dex = dex_dir.join("classes.dex");
    if !dex.exists() {
        bail!("d8 finished but {} is missing", dex.display());
    }
    Ok(dex)
}

/// Copy the base APK's entries as they are, then add the native libraries,
/// the code (if any) and the game files under `assets/`.
fn pack_apk(
    base: &Path,
    libs: &Path,
    dex: Option<&Path>,
    game: &Path,
    files: &[String],
    out: &Path,
) -> Result<()> {
    use zip::write::SimpleFileOptions;

    let mut archive = zip::ZipArchive::new(std::fs::File::open(base)?)?;
    let mut writer = zip::ZipWriter::new(std::fs::File::create(out)?);
    for i in 0..archive.len() {
        // Raw, so `resources.arsc` stays stored uncompressed as aapt2 wrote it.
        writer.raw_copy_file(archive.by_index_raw(i)?)?;
    }
    let deflate = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    // Stored, so Android can map them straight out of the APK once zipalign
    // has put them on 16 KB boundaries.
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    for entry in std::fs::read_dir(libs)? {
        let abi_dir = entry?.path();
        if !abi_dir.is_dir() {
            continue;
        }
        let abi = file_name(&abi_dir);
        for lib in std::fs::read_dir(&abi_dir)? {
            let lib = lib?.path();
            writer.start_file(format!("lib/{abi}/{}", file_name(&lib)), stored)?;
            writer.write_all(&std::fs::read(&lib)?)?;
        }
    }
    if let Some(dex) = dex {
        writer.start_file("classes.dex", deflate)?;
        writer.write_all(&std::fs::read(dex)?)?;
    }
    for rel in files {
        writer.start_file(format!("assets/{rel}"), deflate)?;
        let mut bytes = Vec::new();
        std::fs::File::open(game.join(rel))?.read_to_end(&mut bytes)?;
        writer.write_all(&bytes)?;
    }
    writer.finish()?;
    Ok(())
}

// ─────────────────────────────── Manifest ────────────────────────────────────

/// The `AndroidManifest.xml` for a NativeActivity app.
///
/// `debuggable` goes on a debug-key build, as Gradle does: it is what lets
/// `adb shell run-as` read the app's files, and what the runtime checks
/// before honouring a launch's `scene` extra (`jengine android-smoke`). A
/// store refuses a debuggable upload, so a release-key build never is.
pub fn android_manifest(app: &ResolvedApp, has_icon: bool, internet: bool, debuggable: bool) -> String {
    let orientation = match app.orientation {
        Orientation::Landscape => "sensorLandscape",
        Orientation::Portrait => "sensorPortrait",
        Orientation::Any => "fullUser",
    };
    let icon = if has_icon { "\n        android:icon=\"@mipmap/ic_launcher\"" } else { "" };
    let internet = if internet {
        "    <uses-permission android:name=\"android.permission.INTERNET\" />\n"
    } else {
        ""
    };
    // `Input.vibrate` buzzes the phone. A normal permission - granted at
    // install, no prompt - so every build carries it.
    let vibrate = "    <uses-permission android:name=\"android.permission.VIBRATE\" />\n";
    let activity = activity_class(app);
    let has_code = app.immersive;
    let debuggable = if debuggable { "\n        android:debuggable=\"true\"" } else { "" };
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!-- Generated by jengine-builder from settings.app. Do not edit. -->
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="{id}">
{internet}{vibrate}    <!-- hasCode: the only Java is the immersive activity, a NativeActivity
         that hides the system bars; the game is libmain.so.
         extractNativeLibs="false": the libraries are stored uncompressed
         and page-aligned, and load from the APK. -->
    <application
        android:label="{label}"{icon}
        android:hasCode="{has_code}"
        android:extractNativeLibs="false"
        android:appCategory="game"
        android:isGame="true"{debuggable}>
        <!-- configChanges: turning the phone is a resize the engine handles,
             not a reason for Android to destroy and restart the game. -->
        <activity
            android:name="{activity}"
            android:exported="true"
            android:launchMode="singleTask"
            android:screenOrientation="{orientation}"
            android:theme="@android:style/Theme.Black.NoTitleBar.Fullscreen"
            android:configChanges="orientation|screenSize|screenLayout|smallestScreenSize|keyboard|keyboardHidden|navigation|uiMode|density">
            <meta-data android:name="android.app.lib_name" android:value="main" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"#,
        id = app.id,
        label = xml_escape(&app.name),
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "\\'")
}

// ─────────────────────────────── Icons ───────────────────────────────────────

/// Densities Android asks for, as multiples of the 160 dpi baseline.
const DENSITIES: [(&str, f32); 5] =
    [("mdpi", 1.0), ("hdpi", 1.5), ("xhdpi", 2.0), ("xxhdpi", 3.0), ("xxxhdpi", 4.0)];

/// Write launcher icons for every density from one square image.
///
/// Two kinds: the legacy 48 dp square, and the adaptive icon every launcher
/// since Android 8 uses - a 108 dp foreground over a solid background, which
/// the launcher crops to its own shape and may move about. The picture sits
/// in the middle 72 dp of the foreground so the crop does not eat it.
pub fn write_icons(source: &Path, background: [u8; 3], res: &Path) -> Result<()> {
    let img = image::open(source)
        .with_context(|| format!("reading settings.app.icon {}", source.display()))?
        .to_rgba8();
    let (w, h) = img.dimensions();
    if w != h {
        log::warn!("app icon {} is {w}x{h}, not square; it will be stretched", source.display());
    }
    if w.min(h) < 432 {
        log::warn!(
            "app icon {} is {w}x{h}; 512x512 or more keeps it sharp on high-density screens",
            source.display()
        );
    }
    let filter = image::imageops::FilterType::Lanczos3;
    for (name, scale) in DENSITIES {
        let dir = res.join(format!("mipmap-{name}"));
        std::fs::create_dir_all(&dir)?;

        let legacy = (48.0 * scale).round() as u32;
        image::imageops::resize(&img, legacy, legacy, filter)
            .save(dir.join("ic_launcher.png"))?;

        let canvas = (108.0 * scale).round() as u32;
        let inner = (72.0 * scale).round() as u32;
        let mut fg = image::RgbaImage::new(canvas, canvas);
        let scaled = image::imageops::resize(&img, inner, inner, filter);
        let offset = ((canvas - inner) / 2) as i64;
        image::imageops::overlay(&mut fg, &scaled, offset, offset);
        fg.save(dir.join("ic_launcher_foreground.png"))?;
    }

    let adaptive = res.join("mipmap-anydpi-v26");
    std::fs::create_dir_all(&adaptive)?;
    std::fs::write(
        adaptive.join("ic_launcher.xml"),
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <adaptive-icon xmlns:android=\"http://schemas.android.com/apk/res/android\">\n\
         \x20   <background android:drawable=\"@color/ic_launcher_background\" />\n\
         \x20   <foreground android:drawable=\"@mipmap/ic_launcher_foreground\" />\n\
         </adaptive-icon>\n",
    )?;
    let values = res.join("values");
    std::fs::create_dir_all(&values)?;
    let [r, g, b] = background;
    std::fs::write(
        values.join("ic_launcher_background.xml"),
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <resources>\n\
             \x20   <color name=\"ic_launcher_background\">#{r:02X}{g:02X}{b:02X}</color>\n\
             </resources>\n"
        ),
    )?;
    Ok(())
}

// ─────────────────────────────── Signing ─────────────────────────────────────

/// The debug keystore Android Studio uses, made the same way if it is not
/// there yet, so an APK from here and one from Studio install over each
/// other.
fn debug_keystore() -> Result<PathBuf> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .context("no home directory for the debug keystore")?;
    let dir = PathBuf::from(home).join(".android");
    let ks = dir.join("debug.keystore");
    if ks.exists() {
        return Ok(ks);
    }
    std::fs::create_dir_all(&dir)?;
    log::info!("creating the debug keystore at {}", ks.display());
    run(
        Command::new("keytool")
            .args(["-genkeypair", "-keystore"])
            .arg(&ks)
            .args(["-storepass", "android", "-alias", "androiddebugkey", "-keypass", "android"])
            .args(["-keyalg", "RSA", "-keysize", "2048", "-validity", "10000"])
            .args(["-dname", "CN=Android Debug,O=Android,C=US"]),
        "keytool (from a JDK)",
    )?;
    Ok(ks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{AndroidSettings, AppSettings};

    fn app(orientation: &str) -> ResolvedApp {
        AppSettings {
            id: "com.example.game".into(),
            name: "Tom & Jerry's <Game>".into(),
            orientation: orientation.into(),
            ..Default::default()
        }
        .resolve(&AndroidSettings::default(), "game", "", (1280, 720))
        .unwrap()
    }

    #[test]
    fn the_manifest_carries_the_app_settings() {
        let m = android_manifest(&app("portrait"), true, false, false);
        assert!(m.contains(r#"package="com.example.game""#));
        assert!(m.contains(r#"android:label="Tom &amp; Jerry\'s &lt;Game&gt;""#), "{m}");
        assert!(m.contains(r#"android:screenOrientation="sensorPortrait""#));
        assert!(m.contains(r#"android:icon="@mipmap/ic_launcher""#));
        assert!(m.contains(r#"android:value="main""#));
        assert!(!m.contains("INTERNET"));
    }

    #[test]
    fn immersive_builds_run_the_java_activity_and_others_none() {
        let mut a = app("");
        assert!(a.immersive);
        let m = android_manifest(&a, false, false, false);
        assert!(m.contains(r#"android:name="com.jengine.runtime.JEngineActivity""#), "{m}");
        assert!(m.contains(r#"android:hasCode="true""#));
        a.immersive = false;
        let m = android_manifest(&a, false, false, false);
        assert!(m.contains(r#"android:name="android.app.NativeActivity""#));
        assert!(m.contains(r#"android:hasCode="false""#));
    }

    #[test]
    fn the_java_activity_is_the_class_the_manifest_names() {
        // The source declares the package and class the manifest points at.
        assert!(IMMERSIVE_ACTIVITY_SRC.contains("package com.jengine.runtime;"));
        assert!(IMMERSIVE_ACTIVITY_SRC.contains("public class JEngineActivity extends NativeActivity"));
        assert_eq!(IMMERSIVE_ACTIVITY, "com.jengine.runtime.JEngineActivity");
    }

    #[test]
    fn no_icon_and_network_are_reflected() {
        let m = android_manifest(&app(""), false, true, false);
        assert!(!m.contains("android:icon"));
        assert!(m.contains("android.permission.INTERNET"));
        assert!(m.contains("sensorLandscape"));
    }

    #[test]
    fn only_a_debug_build_is_debuggable() {
        assert!(android_manifest(&app(""), false, false, true).contains(r#"android:debuggable="true""#));
        assert!(!android_manifest(&app(""), false, false, false).contains("debuggable"));
    }

    #[test]
    fn icons_come_out_at_every_density() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("icon.png");
        image::RgbaImage::from_pixel(512, 512, image::Rgba([200, 30, 30, 255]))
            .save(&src)
            .unwrap();
        let res = dir.path().join("res");
        write_icons(&src, [1, 2, 3], &res).unwrap();
        for (name, scale) in DENSITIES {
            let legacy = image::open(res.join(format!("mipmap-{name}/ic_launcher.png"))).unwrap();
            assert_eq!(legacy.width(), (48.0 * scale).round() as u32);
            let fg = image::open(res.join(format!("mipmap-{name}/ic_launcher_foreground.png")))
                .unwrap()
                .to_rgba8();
            let size = (108.0 * scale).round() as u32;
            assert_eq!(fg.width(), size);
            // Transparent margin, picture in the middle.
            assert_eq!(fg.get_pixel(0, 0)[3], 0);
            assert_eq!(fg.get_pixel(size / 2, size / 2)[3], 255);
        }
        let colour = std::fs::read_to_string(res.join("values/ic_launcher_background.xml")).unwrap();
        assert!(colour.contains("#010203"));
    }

    fn release_opts(password: Option<&str>) -> AndroidBuildOptions {
        AndroidBuildOptions {
            release_key: true,
            keystore_password: password.map(str::to_string),
            ..Default::default()
        }
    }

    /// Each refusal happens before anything is compiled, and says which
    /// setting is missing.
    #[test]
    fn a_release_key_is_refused_without_what_it_needs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("release.jks"), b"not really a keystore").unwrap();
        let resolve = |id: &str, keystore: &str, alias: &str| {
            AppSettings { id: id.into(), ..Default::default() }
                .resolve(
                    &AndroidSettings { keystore: keystore.into(), key_alias: alias.into(), ..Default::default() },
                    "game",
                    "",
                    (1280, 720),
                )
                .unwrap()
        };
        let err = |app: ResolvedApp, pw: Option<&str>| {
            Signer::release(dir.path(), &app, &release_opts(pw)).err().expect("refused").to_string()
        };
        assert!(err(resolve("", "release.jks", "upload"), Some("pw")).contains("its own app id"));
        assert!(err(resolve("com.example.game", "", ""), Some("pw")).contains("no release keystore"));
        assert!(err(resolve("com.example.game", "release.jks", ""), Some("pw")).contains("key_alias"));
        assert!(err(resolve("com.example.game", "missing.jks", "upload"), Some("pw")).contains("does not exist"));
        assert!(err(resolve("com.example.game", "release.jks", "upload"), None).contains("password"));
        assert!(err(resolve("com.example.game", "release.jks", "upload"), Some("")).contains("password"));

        let ok = Signer::release(dir.path(), &resolve("com.example.game", "release.jks", "upload"), &release_opts(Some("pw")))
            .unwrap();
        assert!(ok.release);
        assert_eq!(ok.key_pass, "pw", "the key's password defaults to the keystore's");
    }

    #[test]
    fn versions_sort_numerically() {
        assert!(version_key("27.2.12479018") > version_key("22.1.7171670"));
        assert!(version_key("36.0.0") > version_key("9.0.0"));
    }
}
