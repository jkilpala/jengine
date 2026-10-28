//! `jengine` CLI — package management and build commands.
//!
//! Usage:
//!   jengine package list              — List declared packages
//!   jengine package add <name> --path <path>
//!   jengine package add <name> --git <url> [--tag <tag>]
//!   jengine package remove <name>     — Remove a package
//!   jengine package new <name>        — Scaffold a new package from template
//!   jengine package resolve           — Resolve all packages to local paths

use std::path::PathBuf;
use std::process::ExitCode;

use jengine_core::package::manifest::{PackageDependency, PackageSource};

fn main() -> ExitCode {
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        return ExitCode::FAILURE;
    }

    match args[1].as_str() {
        "package" | "pkg" => {
            if args.len() < 3 {
                eprintln!("Usage: jengine package <list|add|remove|new|resolve>");
                return ExitCode::FAILURE;
            }
            match args[2].as_str() {
                "list" | "ls" => cmd_package_list(),
                "add" => cmd_package_add(&args[3..]),
                "remove" | "rm" => cmd_package_remove(&args[3..]),
                "new" => cmd_package_new(&args[3..]),
                "resolve" => cmd_package_resolve(),
                other => {
                    eprintln!("Unknown package subcommand: {other}");
                    ExitCode::FAILURE
                }
            }
        }
        "project" => match args.get(2).map(String::as_str) {
            Some("new") => cmd_project_new(&args[3..]),
            _ => {
                eprintln!("Usage: jengine project new <dir> [--name <name>] [--template starter|empty]");
                ExitCode::FAILURE
            }
        },
        "viewport-crate" => cmd_viewport_crate(&args[2..]),
        "runtime-crate" => cmd_runtime_crate(&args[2..]),
        "viewport-build" => cmd_viewport_build(&args[2..]),
        "viewport-use" => cmd_viewport_use(&args[2..]),
        "engine-bundle" => cmd_engine_bundle(&args[2..]),
        "web-crate" => cmd_web_crate(&args[2..]),
        "web-build" => cmd_web_build(&args[2..]),
        "linux-build" => cmd_desktop_build(&args[2..], "linux"),
        "macos-build" => cmd_desktop_build(&args[2..], "macos"),
        "ios-build" => cmd_ios_build(&args[2..]),
        "run" => cmd_run(&args[2..]),
        "android-build" => cmd_android_build(&args[2..]),
        "android-smoke" => cmd_android_smoke(&args[2..]),
        "font-sdf" => cmd_font_sdf(&args[2..]),
        "psd-import" => cmd_psd_import(&args[2..]),
        "liveops" => match args.get(2).map(String::as_str) {
            Some("publish") => cmd_liveops_publish(&args[3..]),
            _ => {
                eprintln!("usage: jengine liveops publish <build> --server <url> --channel <name> [--token <t>] [--notes <text>]");
                ExitCode::FAILURE
            }
        },
        "--version" | "-V" | "version" => {
            println!("jengine {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        "help" | "--help" | "-h" => {
            print_usage();
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("Unknown command: {other}");
            print_usage();
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!(
        "\
jEngine CLI

USAGE:
    jengine <COMMAND>
    jengine --version

Builds compile the engine from source. The engine checkout is JENGINE_HOME
when set, else the nearest one above the current folder or this program.

COMMANDS:
    project new <dir> [--name <name>] [--template starter|empty]
                                            Create a project: `starter` (the
                                            default) has a scene, a player script
                                            and input actions; `empty` only the
                                            settings. The name defaults to the
                                            folder's
    package list                            List declared packages
    package add <name> --path <path>        Add a local path package
    package add <name> --git <url> [--tag <tag>] [--rev <rev>]
                                            Add a git package
    package remove <name>                   Remove a package
    package new <name> [--type full|script|asset] [--project <dir>]
                                            Scaffold a new package from template.
                                            With --project, a Rust package inside
                                            that project, listed in its
                                            project.jengine (the editor's New
                                            Rust Package)
    package resolve                         Resolve all packages to local paths
    viewport-crate [project]                Generate the editor-viewport wrapper
                                            that links a project's Rust packages
    runtime-crate [project]                 Generate the native runtime crate
                                            that links a project's Rust packages
    viewport-build [project] [--stock] [--wasm-opt <level>]
                                            Build the editor viewport with a
                                            project's Rust packages linked in
                                            (--stock: the engine's own). The
                                            wasm-opt level (off|fast|optimized|
                                            size|max) defaults to the editor's
                                            preference
    viewport-use [project]                  Load the project's cached viewport
                                            build, building only if it is stale
    engine-bundle --out <zip> [--from <engine>]
                                            Zip the engine source an installed
                                            editor ships and builds from (the
                                            engine checkout by default)
    web-crate [project]                     Generate the web crate that links a
                                            project's Rust packages
    run [project] [--instances <1-4>] [--scene <path under assets/>]
                                            Build the native game (release) and start
                                            it from the project folder - several copies
                                            side by side, each with its own saves - as
                                            the editor's Build & Run does. Waits for
                                            them to close
    liveops publish <build> --server <url> --channel <name> [--token <t>] [--notes <text>]
                                            Publish a build's asset packs to a live
                                            ops service as the channel's next
                                            content release: archives it lacks are
                                            uploaded, then the manifest published.
                                            The token defaults to JENGINE_ADMIN_TOKEN
    web-build [project] [--out <dir>] [--dev] [--lto] [--opt-size] [--no-report] [--no-overlay]
              [--no-logs] [--platform <none|jcabinet|poki|crazygames|youtube>] [--wasm-opt <level>]
                                            Build a project for the web, as the
                                            editor's Build panel does (--no-overlay
                                            ships F3 / F12 inert; --no-logs keeps
                                            info lines out of the browser console,
                                            warnings and errors still shown; --platform picks
                                            the web portal over the project's
                                            web_platform). --wasm-opt defaults to
                                            fast, the smallest download;
                                            --opt-size is opt-level 'z', about a
                                            third smaller and ~3.7x slower code
                                            (docs/build-sizes.md). Default
                                            output: target/web-build/<project>/
    android-build [project] [--out <dir>] [--dev] [--abi <abi>]... [--install] [--run]
                  [--bundle] [--release-key]
                                            Build an APK. --abi replaces
                                            settings.android.abis (x86_64 for the
                                            emulator); --install adb-installs it,
                                            --run also starts it. --bundle also
                                            writes the .aab Google Play takes;
                                            --release-key signs with
                                            settings.android.keystore, its
                                            password in JENGINE_KEYSTORE_PASSWORD
                                            (and JENGINE_KEY_PASSWORD if the key's
                                            differs). Default output:
                                            target/android-build/<project>/
    android-smoke [project] [--no-build] [--scene <filter>]... [--dwell <secs>]
                  [--no-lifecycle] [--backend gl] [--out <dir>]
                                            Build a debug APK for the connected
                                            device or emulator, install it, start
                                            every scene in its own process, and
                                            fail on a crash, an engine error in
                                            logcat, or a blank screen; then
                                            background, resume and turn it,
                                            and press a controller's button and
                                            stick at it.
                                            --backend gl runs on the GLES
                                            fallback instead of Vulkan.
                                            Screenshots and report.md go to
                                            target/android-smoke/<project>/
    font-sdf <in.font> [out.font] [--scale <k>]
                                            Turn a bitmap font into a distance
                                            field, sharp at any size: the .font
                                            and its sibling .png are rewritten
                                            (in place without out.font). Its
                                            layout - advances, line height,
                                            where each glyph's ink sits - does
                                            not change. The field is baked k
                                            texels per pixel - by default
                                            enough for a 48 px field, so a
                                            small font keeps its counters.
    psd-import <file.psd> [--origin centre|bottom] [--hidden]
                                            A layered Photoshop document (under
                                            a project's assets/) as a sprite per
                                            layer (sprites/<doc>/) and a prefab
                                            (prefabs/<doc>.prefab) whose parts
                                            are its layers and groups, placed
                                            and ordered as the document draws
                                            them. --origin bottom puts the root
                                            at the canvas's bottom middle (a
                                            character's feet); --hidden brings
                                            hidden layers in too.
    linux-build [project] [--out <dir>] [--debug] [--no-report]
                                            A Linux folder: the runtime named
                                            after the game, project.jengine and
                                            assets/ beside it. On Windows it is
                                            built inside WSL (sourcing
                                            ~/.jengine/wsl-env.sh there, if any).
                                            Default output:
                                            target/linux-build/<project>/
    macos-build [project] [--out <dir>] [--debug] [--no-report]
                                            A macOS .app (Info.plist and icon
                                            from settings.app). Only on a Mac.
                                            Default output:
                                            target/macos-build/<project>/
    ios-build [project] [--out <dir>] [--debug] [--target sim|device]
              [--install] [--run] [--udid <id>] [--identity <name>]
              [--profile <x.mobileprovision>]
                                            An iOS .app, signed: ad hoc for the
                                            simulator (the default), with a
                                            development identity and profile for
                                            a device. --install puts it on the
                                            booted simulator (booting one) or a
                                            connected device; --run starts it.
                                            Only on a Mac. Default output:
                                            target/ios-build/<project>/iphone-simulator/
                                            (or iphone/ for a device) - not a Mac
                                            app; it does not open from Finder
    help                                    Show this help
"
    );
}

/// `jengine font-sdf`: `renderer::text::font_to_sdf` over a `.font` and its
/// sibling `.png` (`font_asset::font_texture_id`), written back beside the
/// output `.font`.
fn cmd_font_sdf(args: &[String]) -> ExitCode {
    let mut paths = Vec::new();
    let mut scale: Option<u32> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--scale" {
            match it.next().and_then(|v| v.parse().ok()).filter(|k| (1..=8).contains(k)) {
                Some(k) => scale = Some(k),
                None => {
                    eprintln!("--scale takes a whole number from 1 to 8");
                    return ExitCode::FAILURE;
                }
            }
        } else {
            paths.push(PathBuf::from(a));
        }
    }
    let Some(input) = paths.first().cloned() else {
        eprintln!("Usage: jengine font-sdf <in.font> [out.font] [--scale <k>]");
        return ExitCode::FAILURE;
    };
    let output = paths.get(1).cloned().unwrap_or_else(|| input.clone());
    let sibling = |p: &std::path::Path| p.with_extension("png");
    let run = || -> anyhow::Result<(usize, u32, u32)> {
        use anyhow::Context;
        let json = std::fs::read_to_string(&input).with_context(|| format!("reading {}", input.display()))?;
        let data: jengine_core::renderer::text::FontData =
            serde_json::from_str(&json).with_context(|| format!("parsing {}", input.display()))?;
        let image = image::open(sibling(&input))
            .with_context(|| format!("reading {}", sibling(&input).display()))?
            .to_rgba8();
        let (w, h) = image.dimensions();
        let k = scale.unwrap_or_else(|| jengine_core::renderer::text::sdf_upscale_for(&data));
        let (out, rgba, ow, oh) = jengine_core::renderer::text::font_to_sdf(&data, image.as_raw(), w, h, k)?;
        image::RgbaImage::from_raw(ow, oh, rgba)
            .context("building the atlas")?
            .save(sibling(&output))
            .with_context(|| format!("writing {}", sibling(&output).display()))?;
        std::fs::write(&output, serde_json::to_string_pretty(&out)? + "\n")
            .with_context(|| format!("writing {}", output.display()))?;
        Ok((out.glyphs.len(), ow, oh))
    };
    match run() {
        Ok((n, w, h)) => {
            println!("{}: {n} glyphs, a {w} x {h} distance field", output.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("font-sdf: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// `jengine psd-import`: `psd_import::import_psd`.
fn cmd_psd_import(args: &[String]) -> ExitCode {
    use jengine_builder::psd_import::{ImportOptions, Origin, import_psd};
    let mut options = ImportOptions::default();
    let mut file = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--hidden" => options.include_hidden = true,
            "--origin" => match it.next().map(String::as_str) {
                Some("centre" | "center") => options.origin = Origin::Centre,
                Some("bottom") => options.origin = Origin::Bottom,
                _ => {
                    eprintln!("--origin takes centre or bottom");
                    return ExitCode::FAILURE;
                }
            },
            _ => file = Some(PathBuf::from(a)),
        }
    }
    let Some(file) = file else {
        eprintln!("Usage: jengine psd-import <file.psd> [--origin centre|bottom] [--hidden]");
        return ExitCode::FAILURE;
    };
    match import_psd(&file, &options) {
        Ok(report) => {
            println!("{}: {} layer(s) as sprites", report.prefab, report.sprites.len());
            for s in &report.skipped {
                println!("  left out: {s}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("psd-import: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn project_root() -> PathBuf {
    std::env::current_dir().expect("Failed to get current directory")
}

/// Generate `<project>/.jengine/runtime/`, the native runtime crate that links
/// a project's Rust packages.
///
/// A build does this itself; the command exists so a developer can build and
/// run their project's real runtime without going through a full package step.
fn cmd_runtime_crate(args: &[String]) -> ExitCode {
    let (project, engine) = match crate_paths(args) {
        Some(p) => p,
        None => return ExitCode::FAILURE,
    };
    let packages = match jengine_builder::packages::resolve_packages(&project) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error resolving packages: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    match jengine_builder::packages::generate_project_runtime(&project, &engine, &packages) {
        Ok(Some(manifest)) => {
            println!("{}", manifest.display());
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("No Rust packages in this project - the stock runtime is enough.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Build the editor viewport with a project's Rust packages linked in.
///
/// The whole operation lives in `jengine_builder::viewport` so this command,
/// `build-project-viewport.ps1` and the editor's "Rebuild viewport" button all
/// run the same code.
fn cmd_viewport_build(args: &[String]) -> ExitCode {
    // `--wasm-opt <level>` overrides the editor preference; `--stock` builds
    // the engine's own viewport (what build-viewport.ps1 and run-editor do).
    let mut level = jengine_builder::wasm_opt::viewport_level();
    let mut stock = false;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--stock" => stock = true,
            "--wasm-opt" => {
                i += 1;
                match args.get(i).and_then(|l| jengine_builder::wasm_opt::WasmOpt::parse(l)) {
                    Some(l) => level = l,
                    None => {
                        eprintln!("--wasm-opt needs one of: off, fast, optimized, size, max");
                        return ExitCode::FAILURE;
                    }
                }
            }
            other => rest.push(other.to_string()),
        }
        i += 1;
    }
    let (project, engine) = match crate_paths(&rest) {
        Some(p) => p,
        None => return ExitCode::FAILURE,
    };
    install_progress_sink();
    if stock {
        return match jengine_builder::viewport::build_stock_with(&engine, level) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Error: {e:#}");
                ExitCode::FAILURE
            }
        };
    }
    match jengine_builder::viewport::build_with(&engine, &project, level) {
        Ok(names) => {
            if names.is_empty() {
                eprintln!("No Rust packages - the stock viewport is enough.");
            } else {
                eprintln!("Viewport rebuilt with: {}", names.join(", "));
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Load a project's own cached viewport build into the editor, compiling only
/// when something it was built from has changed - what `run-editor.ps1` runs
/// on every launch.
/// Zip the engine source for an installer - see `engine_root::pack`.
fn cmd_engine_bundle(args: &[String]) -> ExitCode {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let Some(out) = flag("--out") else {
        eprintln!("usage: jengine engine-bundle --out <zip> [--from <engine>]");
        return ExitCode::FAILURE;
    };
    let from = match flag("--from").map(Ok).unwrap_or_else(jengine_builder::engine_root::locate) {
        Ok(from) => from,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };
    match jengine_builder::engine_root::pack(&from, &out) {
        Ok(count) => {
            let size = std::fs::metadata(&out).map_or(0, |m| m.len());
            eprintln!("{count} files from {} into {} ({:.1} MB)", from.display(), out.display(), size as f64 / 1e6);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_viewport_use(args: &[String]) -> ExitCode {
    let (project, engine) = match crate_paths(args) {
        Some(p) => p,
        None => return ExitCode::FAILURE,
    };
    install_progress_sink();
    match jengine_builder::viewport::ensure(&engine, &project) {
        Ok(names) => {
            if names.is_empty() {
                eprintln!("No Rust packages - the stock viewport is enough.");
            } else {
                eprintln!("Viewport carries: {}", names.join(", "));
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Generate `<project>/.jengine/web/`, the crate that links a project's Rust
/// packages into the web runtime. A web build does this itself; the command
/// exists so the generated crate can be inspected or `cargo check`ed alone.
fn cmd_web_crate(args: &[String]) -> ExitCode {
    let (project, engine) = match crate_paths(args) {
        Some(p) => p,
        None => return ExitCode::FAILURE,
    };
    let packages = match jengine_builder::packages::resolve_packages(&project) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error resolving packages: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    match jengine_builder::packages::generate_project_web(&project, &engine, &packages) {
        Ok(Some(manifest)) => {
            println!("{}", manifest.display());
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("No Rust packages in this project - the stock web runtime is enough.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// Build a project for the web - the same `jengine_builder::web::build_web`
/// the editor's Build panel runs, so a session with no editor window can
/// build, serve and check its own work.
fn cmd_run(args: &[String]) -> ExitCode {
    let mut project: Option<PathBuf> = None;
    let mut instances = 1u32;
    let mut scene: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--instances" => {
                i += 1;
                match args.get(i).and_then(|n| n.parse().ok()) {
                    Some(n) if (1..=jengine_builder::native_run::MAX_INSTANCES).contains(&n) => instances = n,
                    _ => {
                        eprintln!("--instances needs a number from 1 to {}", jengine_builder::native_run::MAX_INSTANCES);
                        return ExitCode::FAILURE;
                    }
                }
            }
            "--scene" => {
                i += 1;
                scene = args.get(i).cloned();
            }
            flag if flag.starts_with("--") => {
                eprintln!("Unknown flag: {flag}");
                return ExitCode::FAILURE;
            }
            path => project = Some(PathBuf::from(path)),
        }
        i += 1;
    }
    let project = project.unwrap_or_else(project_root);
    let (project, engine) = match crate_paths(&[project.to_string_lossy().to_string()]) {
        Some(p) => p,
        None => return ExitCode::FAILURE,
    };
    match jengine_builder::native_run::build_and_run(&engine, &project, scene.as_deref(), instances) {
        Ok(mut started) => {
            for inst in &started {
                eprintln!("instance {} running - its log: {}", inst.n, inst.log.display());
            }
            for inst in &mut started {
                let _ = inst.child.wait();
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("run failed: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// `jengine linux-build` / `macos-build`: `desktop::build_linux` /
/// `build_macos`.
fn cmd_desktop_build(args: &[String], platform: &str) -> ExitCode {
    let mut project: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut release = true;
    let mut report = true;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                match args.get(i) {
                    Some(dir) => out = Some(PathBuf::from(dir)),
                    None => {
                        eprintln!("--out needs a directory");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "--debug" => release = false,
            "--no-report" => report = false,
            flag if flag.starts_with("--") => {
                eprintln!("Unknown flag: {flag}");
                return ExitCode::FAILURE;
            }
            path => project = Some(PathBuf::from(path)),
        }
        i += 1;
    }
    let project = project.unwrap_or_else(project_root);
    let (project, engine) = match crate_paths(&[project.to_string_lossy().to_string()]) {
        Some(p) => p,
        None => return ExitCode::FAILURE,
    };
    let out = out.unwrap_or_else(|| {
        let name = project
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "game".to_string());
        engine.join("target").join(format!("{platform}-build")).join(name)
    });
    install_progress_sink();
    let built = if platform == "macos" {
        jengine_builder::desktop::build_macos(&engine, &project, &out, release, report, true)
    } else {
        jengine_builder::desktop::build_linux(&engine, &project, &out, release, report, true)
    };
    match built {
        Ok(path) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{platform}-build: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// `jengine ios-build`: `ios::build_ios`.
fn cmd_ios_build(args: &[String]) -> ExitCode {
    use jengine_builder::ios::{IosOptions, IosTarget};
    let mut project: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut opts = IosOptions {
        target: IosTarget::Simulator,
        release: true,
        debug_overlay: true,
        install: false,
        launch: false,
        device: None,
        identity: None,
        profile: None,
    };
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        let mut value = |name: &str| -> Option<String> {
            i += 1;
            let v = args.get(i).cloned();
            if v.is_none() {
                eprintln!("{name} needs a value");
            }
            v
        };
        match flag {
            "--out" => match value("--out") {
                Some(v) => out = Some(PathBuf::from(v)),
                None => return ExitCode::FAILURE,
            },
            "--target" => match value("--target").as_deref() {
                Some("sim") | Some("simulator") => opts.target = IosTarget::Simulator,
                Some("device") => opts.target = IosTarget::Device,
                Some(other) => {
                    eprintln!("--target is sim or device, not {other}");
                    return ExitCode::FAILURE;
                }
                None => return ExitCode::FAILURE,
            },
            "--udid" => match value("--udid") {
                Some(v) => opts.device = Some(v),
                None => return ExitCode::FAILURE,
            },
            "--identity" => match value("--identity") {
                Some(v) => opts.identity = Some(v),
                None => return ExitCode::FAILURE,
            },
            "--profile" => match value("--profile") {
                Some(v) => opts.profile = Some(PathBuf::from(v)),
                None => return ExitCode::FAILURE,
            },
            "--debug" => opts.release = false,
            "--install" => opts.install = true,
            "--run" => {
                opts.install = true;
                opts.launch = true;
            }
            f if f.starts_with("--") => {
                eprintln!("Unknown flag: {f}");
                return ExitCode::FAILURE;
            }
            path => project = Some(PathBuf::from(path)),
        }
        i += 1;
    }
    let project = project.unwrap_or_else(project_root);
    let (project, engine) = match crate_paths(&[project.to_string_lossy().to_string()]) {
        Some(p) => p,
        None => return ExitCode::FAILURE,
    };
    let out = out.unwrap_or_else(|| {
        let name = project
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "game".to_string());
        engine.join("target").join("ios-build").join(name)
    });
    install_progress_sink();
    match jengine_builder::ios::build_ios(&engine, &project, &out, &opts) {
        Ok(path) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ios-build: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_liveops_publish(args: &[String]) -> ExitCode {
    let mut build: Option<PathBuf> = None;
    let (mut server, mut channel, mut notes) = (None, None, String::new());
    let mut token = std::env::var("JENGINE_ADMIN_TOKEN").unwrap_or_default();
    let mut i = 0;
    while i < args.len() {
        let next = |i: &mut usize| {
            *i += 1;
            args.get(*i).cloned()
        };
        match args[i].as_str() {
            "--server" => server = next(&mut i),
            "--channel" => channel = next(&mut i),
            "--token" => token = next(&mut i).unwrap_or_default(),
            "--notes" => notes = next(&mut i).unwrap_or_default(),
            flag if flag.starts_with("--") => {
                eprintln!("Unknown flag: {flag}");
                return ExitCode::FAILURE;
            }
            path => build = Some(PathBuf::from(path)),
        }
        i += 1;
    }
    let (Some(build), Some(server), Some(channel)) = (build, server, channel) else {
        eprintln!("usage: jengine liveops publish <build> --server <url> --channel <name> [--token <t>] [--notes <text>]");
        return ExitCode::FAILURE;
    };
    if token.is_empty() {
        eprintln!("jengine liveops publish: no admin token (--token, or JENGINE_ADMIN_TOKEN)");
        return ExitCode::FAILURE;
    }
    match jengine_builder::liveops_publish::publish(&build, &server, &channel, &token, &notes) {
        Ok(p) => {
            println!(
                "Published release {} on {channel}: {} archive(s) uploaded, {} already there",
                p.release,
                p.uploaded.len(),
                p.already_there.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("jengine liveops publish: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_web_build(args: &[String]) -> ExitCode {
    let mut project: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut release = true;
    let mut lto = false;
    let mut opt_size = false;
    let mut report = true;
    let mut overlay = true;
    let mut logs = true;
    let mut platform = None;
    let mut wasm_opt = jengine_builder::wasm_opt::WasmOpt::WEB_BUILD_DEFAULT;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--wasm-opt" => {
                i += 1;
                match args.get(i).and_then(|l| jengine_builder::wasm_opt::WasmOpt::parse(l)) {
                    Some(l) => wasm_opt = l,
                    None => {
                        eprintln!("--wasm-opt needs one of: off, fast, optimized, size, max");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "--out" => {
                i += 1;
                match args.get(i) {
                    Some(dir) => out = Some(PathBuf::from(dir)),
                    None => {
                        eprintln!("--out needs a directory");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "--dev" => release = false,
            "--lto" => lto = true,
            "--opt-size" => opt_size = true,
            "--no-report" => report = false,
            "--no-overlay" => overlay = false,
            "--no-logs" => logs = false,
            "--platform" => {
                i += 1;
                match args.get(i).and_then(|id| jengine_builder::web_platform::WebPlatform::parse(id)) {
                    Some(p) => platform = Some(p),
                    None => {
                        eprintln!(
                            "--platform needs one of: {}",
                            jengine_builder::web_platform::known_ids()
                        );
                        return ExitCode::FAILURE;
                    }
                }
            }
            flag if flag.starts_with("--") => {
                eprintln!("Unknown flag: {flag}");
                return ExitCode::FAILURE;
            }
            path => project = Some(PathBuf::from(path)),
        }
        i += 1;
    }
    let project = project.unwrap_or_else(project_root);
    let (project, engine) = match crate_paths(&[project.to_string_lossy().to_string()]) {
        Some(p) => p,
        None => return ExitCode::FAILURE,
    };
    let profile = if release { "release" } else { "debug" };
    let out = out.unwrap_or_else(|| {
        let name = project
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "game".to_string());
        engine.join("target").join("web-build").join(name).join(profile)
    });
    install_progress_sink();
    match jengine_builder::web::build_web(
        &engine, &project, &out, release, report, lto, opt_size, false, wasm_opt, overlay, logs, platform,
    ) {
        Ok(()) => {
            eprintln!(
                "Web build ({profile}) written to {}",
                jengine_builder::packages::display_path(&out)
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_android_build(args: &[String]) -> ExitCode {
    let mut project: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut opts = jengine_builder::android::AndroidBuildOptions::default();
    let mut abis: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" | "--abi" => {
                let flag = args[i].clone();
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("{flag} needs a value");
                    return ExitCode::FAILURE;
                };
                if flag == "--out" {
                    out = Some(PathBuf::from(value));
                } else {
                    abis.push(value.clone());
                }
            }
            "--dev" => opts.release = false,
            "--install" => opts.install = true,
            "--run" => opts.launch = true,
            "--bundle" => opts.bundle = true,
            "--release-key" => opts.release_key = true,
            flag if flag.starts_with("--") => {
                eprintln!("Unknown flag: {flag}");
                return ExitCode::FAILURE;
            }
            path => project = Some(PathBuf::from(path)),
        }
        i += 1;
    }
    if !abis.is_empty() {
        opts.abis = Some(abis);
    }
    // Passwords from the environment only - a command-line argument would
    // sit in the shell's history and in every process listing.
    opts.keystore_password = std::env::var("JENGINE_KEYSTORE_PASSWORD").ok();
    opts.key_password = std::env::var("JENGINE_KEY_PASSWORD").ok();
    let project = project.unwrap_or_else(project_root);
    let (project, engine) = match crate_paths(&[project.to_string_lossy().to_string()]) {
        Some(p) => p,
        None => return ExitCode::FAILURE,
    };
    let profile = if opts.release { "release" } else { "debug" };
    let out = out.unwrap_or_else(|| {
        let name = project
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "game".to_string());
        engine.join("target").join("android-build").join(name).join(profile)
    });
    install_progress_sink();
    match jengine_builder::android::build_android(&engine, &project, &out, &opts) {
        Ok(out) => {
            eprintln!("APK written to {}", jengine_builder::packages::display_path(&out.apk));
            if let Some(aab) = &out.bundle {
                eprintln!("App Bundle written to {}", jengine_builder::packages::display_path(aab));
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_android_smoke(args: &[String]) -> ExitCode {
    use jengine_builder::android_smoke::{self, SmokeOptions};
    let mut project: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut build = true;
    let mut opts = SmokeOptions::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" | "--scene" | "--dwell" | "--backend" => {
                let flag = args[i].clone();
                i += 1;
                let Some(value) = args.get(i) else {
                    eprintln!("{flag} needs a value");
                    return ExitCode::FAILURE;
                };
                match flag.as_str() {
                    "--out" => out = Some(PathBuf::from(value)),
                    "--scene" => opts.filters.push(value.clone()),
                    "--backend" => opts.backend = Some(value.clone()),
                    _ => match value.parse::<f64>() {
                        Ok(s) if s > 0.0 => opts.dwell = std::time::Duration::from_secs_f64(s),
                        _ => {
                            eprintln!("--dwell takes seconds, got '{value}'");
                            return ExitCode::FAILURE;
                        }
                    },
                }
            }
            "--no-build" => build = false,
            "--no-lifecycle" => opts.lifecycle = false,
            flag if flag.starts_with("--") => {
                eprintln!("Unknown flag: {flag}");
                return ExitCode::FAILURE;
            }
            path => project = Some(PathBuf::from(path)),
        }
        i += 1;
    }
    let project = project.unwrap_or_else(project_root);
    let (project, engine) = match crate_paths(&[project.to_string_lossy().to_string()]) {
        Some(p) => p,
        None => return ExitCode::FAILURE,
    };
    let name = project
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "game".to_string());
    let out = out.unwrap_or_else(|| engine.join("target").join("android-smoke").join(&name));
    install_progress_sink();

    if build {
        // The device's own ABI, debug-signed (so it honours a launch's
        // scene), optimised (so a slow frame is the game's, not the build's).
        let abi = match android_smoke::device_abi() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("Error: {e:#}");
                return ExitCode::FAILURE;
            }
        };
        let build_opts = jengine_builder::android::AndroidBuildOptions {
            abis: Some(vec![abi]),
            install: true,
            ..Default::default()
        };
        let apk_out = engine.join("target").join("android-build").join(&name).join("release");
        if let Err(e) = jengine_builder::android::build_android(&engine, &project, &apk_out, &build_opts) {
            eprintln!("Error: {e:#}");
            return ExitCode::FAILURE;
        }
    }

    let results = match android_smoke::run_smoke(&project, &out, &opts) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    let report = android_smoke::report_markdown(&project, &opts, &results);
    let report_path = out.join(match &opts.backend {
        Some(b) => format!("report-{b}.md"),
        None => "report.md".to_string(),
    });
    if let Err(e) = std::fs::write(&report_path, &report) {
        eprintln!("could not write {}: {e}", report_path.display());
    }
    let failed: Vec<_> = results.iter().filter(|r| r.failed()).collect();
    for r in &failed {
        eprintln!("FAILED {}", r.name);
        for f in r.findings.iter().filter(|f| f.severity == android_smoke::Severity::Error).take(3) {
            eprintln!("    {}", f.message);
        }
    }
    eprintln!(
        "{} checked, {} failed. Report: {}",
        results.len(),
        failed.len(),
        jengine_builder::packages::display_path(&report_path)
    );
    if failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// Print build progress as it happens, the way the editor's Build panel
/// shows it. Without a sink installed, `progress` lines go to `log`.
fn install_progress_sink() {
    jengine_builder::progress::set_sink(Some(std::sync::Arc::new(
        |line: &str, replaces_previous: bool| {
            // A terminal already knows what a carriage return means, so hand
            // a progress redraw straight back to it: the bar counts up in
            // place instead of scrolling past.
            if replaces_previous {
                print!("{line}\r");
                let _ = std::io::Write::flush(&mut std::io::stdout());
            } else {
                println!("{line}");
            }
        },
    )));
}

/// Resolve the (project, engine) pair both codegen commands need.
fn crate_paths(args: &[String]) -> Option<(PathBuf, PathBuf)> {
    let project = args.first().map(PathBuf::from).unwrap_or_else(project_root);
    match jengine_builder::engine_root::locate() {
        Ok(engine) => Some((project, engine)),
        Err(e) => {
            eprintln!("Error: {e}");
            None
        }
    }
}

/// Generate `<project>/.jengine/viewport/`, the wrapper crate that links a
/// project's Rust packages into the editor viewport.
///
/// Prints one package name per line on stdout so `build-project-viewport.ps1`
/// can record which packages the built module carries — the editor needs that
/// to tell a stale viewport from a current one.
fn cmd_viewport_crate(args: &[String]) -> ExitCode {
    let project = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(project_root);
    let engine = match jengine_builder::engine_root::locate() {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let packages = match jengine_builder::packages::resolve_packages(&project) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error resolving packages: {e:#}");
            return ExitCode::FAILURE;
        }
    };

    match jengine_builder::packages::generate_project_viewport(&project, &engine, &packages) {
        Ok(Some(manifest)) => {
            eprintln!("Generated {}", manifest.display());
            for pkg in packages
                .iter()
                .filter(|p| p.package_type == jengine_core::package::manifest::PackageType::Full)
            {
                println!("{}", pkg.name);
            }
            ExitCode::SUCCESS
        }
        Ok(None) => {
            eprintln!("No Rust packages in this project — the stock viewport is enough.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_package_list() -> ExitCode {
    let root = project_root();
    match jengine_builder::packages::list_packages(&root) {
        Ok(deps) => {
            if deps.is_empty() {
                println!("No packages declared in project.jengine");
            } else {
                println!("{} package(s):", deps.len());
                for dep in &deps {
                    let source_str = match &dep.source {
                        PackageSource::Path { path } => format!("path: {path}"),
                        PackageSource::Git { url, tag, rev } => {
                            let mut s = format!("git: {url}");
                            if let Some(t) = tag {
                                s.push_str(&format!(" (tag: {t})"));
                            }
                            if let Some(r) = rev {
                                s.push_str(&format!(" (rev: {r})"));
                            }
                            s
                        }
                    };
                    let optional = if dep.optional { " [optional]" } else { "" };
                    println!("  {} — {}{}", dep.name, source_str, optional);
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_package_add(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("Usage: jengine package add <name> --path <path> | --git <url> [--tag <tag>]");
        return ExitCode::FAILURE;
    }

    let name = &args[0];
    let mut path: Option<String> = None;
    let mut git_url: Option<String> = None;
    let mut tag: Option<String> = None;
    let mut rev: Option<String> = None;
    let mut optional = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--path" => {
                i += 1;
                path = args.get(i).cloned();
            }
            "--git" => {
                i += 1;
                git_url = args.get(i).cloned();
            }
            "--tag" => {
                i += 1;
                tag = args.get(i).cloned();
            }
            "--rev" => {
                i += 1;
                rev = args.get(i).cloned();
            }
            "--optional" => {
                optional = true;
            }
            other => {
                eprintln!("Unknown flag: {other}");
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }

    let source = if let Some(p) = path {
        PackageSource::Path { path: p }
    } else if let Some(url) = git_url {
        PackageSource::Git { url, tag, rev }
    } else {
        eprintln!("Must specify --path or --git");
        return ExitCode::FAILURE;
    };

    let dep = PackageDependency {
        name: name.clone(),
        source,
        optional,
    };

    let root = project_root();
    match jengine_builder::packages::add_package(&root, dep) {
        Ok(()) => {
            println!("Added package '{name}'");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_package_remove(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("Usage: jengine package remove <name>");
        return ExitCode::FAILURE;
    }

    let name = &args[0];
    let root = project_root();
    match jengine_builder::packages::remove_package(&root, name) {
        Ok(true) => {
            println!("Removed package '{name}'");
            ExitCode::SUCCESS
        }
        Ok(false) => {
            eprintln!("Package '{name}' not found in project.jengine");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_project_new(args: &[String]) -> ExitCode {
    let Some(dir) = args.first().map(PathBuf::from) else {
        eprintln!("Usage: jengine project new <dir> [--name <name>] [--template starter|empty]");
        return ExitCode::FAILURE;
    };
    let mut name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "New Game".to_string());
    let mut template = jengine_builder::new_project::ProjectTemplate::default();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--name" if i + 1 < args.len() => {
                name = args[i + 1].clone();
                i += 2;
            }
            "--template" if i + 1 < args.len() => {
                let Some(t) = jengine_builder::new_project::ProjectTemplate::from_name(&args[i + 1]) else {
                    eprintln!("Unknown template: {} (starter or empty)", args[i + 1]);
                    return ExitCode::FAILURE;
                };
                template = t;
                i += 2;
            }
            other => {
                eprintln!("Unknown flag: {other}");
                return ExitCode::FAILURE;
            }
        }
    }
    match jengine_builder::new_project::create_project(&dir, &name, template) {
        Ok(path) => {
            println!("Created project '{name}' at {}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_package_new(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("Usage: jengine package new <name> [--type full|script|asset] [--project <dir>]");
        return ExitCode::FAILURE;
    }

    let name = &args[0];
    let mut pkg_type = "full";
    let mut project: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--type" | "-t" => {
                i += 1;
                if let Some(t) = args.get(i) {
                    pkg_type = match t.as_str() {
                        "full" | "script" | "asset" => t.as_str(),
                        other => {
                            eprintln!("Unknown package type: {other} (expected: full, script, asset)");
                            return ExitCode::FAILURE;
                        }
                    };
                } else {
                    eprintln!("--type requires a value (full, script, asset)");
                    return ExitCode::FAILURE;
                }
            }
            "--project" => {
                i += 1;
                let Some(dir) = args.get(i) else {
                    eprintln!("--project requires a project folder");
                    return ExitCode::FAILURE;
                };
                project = Some(PathBuf::from(dir));
            }
            other => {
                eprintln!("Unknown flag: {other}");
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }

    if let Some(project) = project {
        if pkg_type != "full" {
            eprintln!("--project makes a Rust package; leave out --type {pkg_type}");
            return ExitCode::FAILURE;
        }
        return match jengine_builder::packages::create_project_package(&project, name) {
            Ok(path) => {
                println!("Created Rust package '{name}' at {} and listed it in project.jengine", path.display());
                println!("Rebuild the viewport to use it in the editor: jengine viewport-build {}", project.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Error: {e:#}");
                ExitCode::FAILURE
            }
        };
    }

    let output_dir = project_root().join("packages");

    let result = match pkg_type {
        "script" => jengine_builder::packages::create_script_package_from_template(&output_dir, name),
        "asset" => jengine_builder::packages::create_asset_package_from_template(&output_dir, name),
        _ => jengine_builder::packages::create_package_from_template(&output_dir, name),
    };

    match result {
        Ok(path) => {
            println!("Created {pkg_type} package '{name}' at {}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_package_resolve() -> ExitCode {
    let root = project_root();
    match jengine_builder::packages::resolve_packages(&root) {
        Ok(packages) => {
            if packages.is_empty() {
                println!("No packages to resolve");
            } else {
                println!("Resolved {} package(s):", packages.len());
                for pkg in &packages {
                    let type_label = match pkg.package_type {
                        jengine_core::package::manifest::PackageType::Full => "full",
                        jengine_core::package::manifest::PackageType::Script => "script",
                        jengine_core::package::manifest::PackageType::Asset => "asset",
                    };
                    let files_info = if pkg.installed_files.is_empty() {
                        String::new()
                    } else {
                        format!(" ({} files installed)", pkg.installed_files.len())
                    };
                    println!("  {} [{}] → {}{}", pkg.name, type_label, pkg.local_path.display(), files_info);
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}
