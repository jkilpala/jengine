//! Linux and macOS desktop builds (Oct 2026), beside Windows (`windows.rs`).
//!
//! The runtime is the same winit + wgpu program on every desktop, and it
//! finds its game files beside itself or in a macOS bundle's `Resources`
//! (`jengine_runtime::game_root`), so a build is the runtime plus the staged
//! game (`stage::stage_game`) laid out the platform's way:
//!
//! - **Linux** - a folder: the runtime named after the game, `project.jengine`
//!   and `assets/` beside it, and the app icon as `icon.png`. Built on a Linux
//!   machine, or **from Windows inside WSL** (`wsl.exe`): the same cargo
//!   plan (`windows::plan_native_runtime`), run in the distribution with its
//!   own target directory on the Linux filesystem, the binary copied back.
//! - **macOS** - an `.app` bundle: `Contents/MacOS/<game>`, the staged game in
//!   `Contents/Resources`, an `Info.plist` from `settings.app` (name, id,
//!   version, build) and the icon as `AppIcon.icns`. Only a Mac can build it -
//!   the linker and SDK are Apple's - so elsewhere it is refused by name.
//!
//! What a Linux machine needs to build: `pkg-config`, ALSA (`libasound2-dev`)
//! and udev (`libudev-dev`) headers; to run, Vulkan (Mesa or a vendor driver)
//! and X11 or Wayland with xkbcommon. [`LINUX_PACKAGES`] is the Debian /
//! Ubuntu line the build prints when cargo fails.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// What a Debian or Ubuntu machine installs to build (and run) the runtime.
pub const LINUX_PACKAGES: &str = "sudo apt install build-essential pkg-config libasound2-dev libudev-dev libxkbcommon-x11-0 libwayland-client0 libvulkan1 mesa-vulkan-drivers";

/// A file name from the game's name: letters, digits and `-`.
pub fn binary_name(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() { "game".into() } else { s }
}

/// A Windows path as WSL mounts it: `C:\dev\x` is `/mnt/c/dev/x`.
pub fn wsl_path(path: &Path) -> Result<String> {
    // A relative path (a build is often given one) is made absolute first.
    let abs = if path.is_relative() && !path.to_string_lossy().starts_with('/') {
        std::path::absolute(path).with_context(|| format!("resolving {}", path.display()))?
    } else {
        path.to_path_buf()
    };
    let s = abs.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s).to_string();
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(d), Some(':')) if d.is_ascii_alphabetic() => Ok(format!("/mnt/{}{}", d.to_ascii_lowercase(), &s[2..])),
        _ if s.starts_with('/') => Ok(s),
        _ => bail!("{} is not a path WSL can reach", path.display()),
    }
}

/// Single-quote a word for bash.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The bash script that builds the runtime inside WSL and copies the binary
/// to `dest` (a `/mnt/...` path). `~/.jengine/wsl-env.sh`, when present, is
/// sourced first - the place for a distribution's own `PKG_CONFIG_PATH` and
/// the like.
pub fn wsl_build_script(build_dir: &str, args: &[String], target_dir_name: &str, profile: &str, binary_stem: &str, dest: &str) -> String {
    let args: Vec<String> = args.iter().map(|a| sh_quote(a)).collect();
    format!(
        "set -e\n\
         [ -f \"$HOME/.cargo/env\" ] && . \"$HOME/.cargo/env\"\n\
         [ -f \"$HOME/.jengine/wsl-env.sh\" ] && . \"$HOME/.jengine/wsl-env.sh\"\n\
         export CARGO_TARGET_DIR=\"${{JENGINE_WSL_TARGET:-$HOME/.jengine/target}}/{target}\"\n\
         cd {dir}\n\
         cargo {args}\n\
         cp \"$CARGO_TARGET_DIR/{profile}/{stem}\" {dest}\n\
         chmod +x {dest}\n",
        target = target_dir_name,
        dir = sh_quote(build_dir),
        args = args.join(" "),
        profile = profile,
        stem = binary_stem,
        dest = sh_quote(dest),
    )
}

/// Build a Linux folder for `project_path` into `output_path`; returns the
/// game's executable there.
pub fn build_linux(
    engine_path: &Path,
    project_path: &Path,
    output_path: &Path,
    release: bool,
    generate_report: bool,
    debug_overlay: bool,
) -> Result<PathBuf> {
    let app = crate::app::read(project_path)?;
    let name = binary_name(&app.name);
    std::fs::create_dir_all(output_path)?;
    let dest = output_path.join(&name);
    let plan = crate::windows::plan_native_runtime(engine_path, project_path, release, "linux")?;

    if cfg!(target_os = "linux") {
        let built = crate::windows::compile_native_runtime_for(engine_path, project_path, release, "linux")?;
        std::fs::copy(&built, &dest).with_context(|| format!("copying the runtime to {}", dest.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
        }
    } else if cfg!(windows) {
        crate::progress::step("Building for Linux inside WSL...");
        let script = wsl_build_script(
            &wsl_path(&plan.build_dir)?,
            &plan.args,
            &format!("{}-linux", binary_name(&app.name)),
            plan.profile,
            &plan.binary_stem,
            &wsl_path(&dest)?,
        );
        let mut cmd = Command::new("wsl.exe");
        cmd.args(["-e", "bash", "-c", &script]);
        let status = crate::progress::run_streaming(&mut cmd, "wsl")
            .context("running wsl.exe - is WSL installed (`wsl --install -d Ubuntu`)?")?;
        if !status.success() {
            bail!(
                "the Linux build failed inside WSL ({status}). If cargo could not find a library, the distribution \
                 needs: {LINUX_PACKAGES} (and Rust: https://rustup.rs)"
            );
        }
    } else {
        bail!("a Linux build is made on Linux, or on Windows inside WSL");
    }

    crate::stage::stage_game(project_path, output_path, debug_overlay, &output_path.join("packs"))?;
    if let Some(icon) = &app.icon {
        let _ = std::fs::copy(project_path.join(icon), output_path.join("icon.png"));
    }
    crate::progress::step(&format!("Linux build complete: {}", output_path.display()));
    if generate_report {
        crate::report::generate_report(output_path, "linux", plan.profile, None)?;
    }
    Ok(dest)
}

/// An `Info.plist` for the game, from `settings.app`.
pub fn info_plist(app: &crate::app::ResolvedApp, executable: &str, has_icon: bool) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let icon = if has_icon { "\t<key>CFBundleIconFile</key>\n\t<string>AppIcon</string>\n" } else { "" };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <!-- Generated by jengine-builder from settings.app. Do not edit. -->\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>CFBundleName</key>\n\t<string>{name}</string>\n\
         \t<key>CFBundleDisplayName</key>\n\t<string>{name}</string>\n\
         \t<key>CFBundleIdentifier</key>\n\t<string>{id}</string>\n\
         \t<key>CFBundleExecutable</key>\n\t<string>{exe}</string>\n\
         \t<key>CFBundlePackageType</key>\n\t<string>APPL</string>\n\
         \t<key>CFBundleShortVersionString</key>\n\t<string>{version}</string>\n\
         \t<key>CFBundleVersion</key>\n\t<string>{build}</string>\n\
         {icon}\
         \t<key>LSMinimumSystemVersion</key>\n\t<string>11.0</string>\n\
         \t<key>NSHighResolutionCapable</key>\n\t<true/>\n\
         \t<key>LSApplicationCategoryType</key>\n\t<string>public.app-category.games</string>\n\
         </dict>\n</plist>\n",
        name = esc(&app.name),
        id = esc(&app.id),
        exe = esc(executable),
        version = esc(&app.version),
        build = app.build,
        icon = icon,
    )
}

/// An `.icns` icon from a picture: PNG entries at 128, 256, 512 and 1024
/// pixels, which every macOS since 10.7 reads.
pub fn icns_from_image(img: &image::DynamicImage) -> Result<Vec<u8>> {
    let mut entries: Vec<u8> = Vec::new();
    for (kind, size) in [(b"ic07", 128u32), (b"ic08", 256), (b"ic09", 512), (b"ic10", 1024)] {
        let scaled = img.resize_exact(size, size, image::imageops::FilterType::Lanczos3);
        let mut png = Vec::new();
        scaled.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
        entries.extend_from_slice(kind);
        entries.extend_from_slice(&((png.len() + 8) as u32).to_be_bytes());
        entries.extend_from_slice(&png);
    }
    let mut out = Vec::with_capacity(entries.len() + 8);
    out.extend_from_slice(b"icns");
    out.extend_from_slice(&((entries.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(&entries);
    Ok(out)
}

/// Lay out `<Name>.app` in `output_path` around an already-built runtime:
/// the executable, `Info.plist`, the icon and the staged game. Returns the
/// bundle's path. (Separate from compiling, so it is testable anywhere.)
pub fn write_app_bundle(
    project_path: &Path,
    output_path: &Path,
    runtime: &Path,
    debug_overlay: bool,
) -> Result<PathBuf> {
    let app = crate::app::read(project_path)?;
    let exe = binary_name(&app.name);
    let bundle = output_path.join(format!("{}.app", app.name.trim()));
    let contents = bundle.join("Contents");
    let _ = std::fs::remove_dir_all(&bundle);
    std::fs::create_dir_all(contents.join("MacOS"))?;
    std::fs::create_dir_all(contents.join("Resources"))?;
    let dest = contents.join("MacOS").join(&exe);
    std::fs::copy(runtime, &dest).with_context(|| format!("copying the runtime to {}", dest.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
    }
    let mut has_icon = false;
    if let Some(icon) = &app.icon {
        match image::open(project_path.join(icon)) {
            Ok(img) => {
                std::fs::write(contents.join("Resources").join("AppIcon.icns"), icns_from_image(&img)?)?;
                has_icon = true;
            }
            Err(e) => log::warn!("settings.app.icon {icon} could not be read ({e}); the app has the default icon"),
        }
    }
    std::fs::write(contents.join("Info.plist"), info_plist(&app, &exe, has_icon))?;
    let packs = crate::asset_packs::read_settings(project_path);
    let packs_out = crate::asset_packs::packs_beside_bundle(output_path, "macOS", &packs);
    crate::stage::stage_game(project_path, &contents.join("Resources"), debug_overlay, &packs_out)?;
    Ok(bundle)
}

/// Build a macOS `.app` for `project_path` into `output_path`. Only on a Mac.
pub fn build_macos(
    engine_path: &Path,
    project_path: &Path,
    output_path: &Path,
    release: bool,
    generate_report: bool,
    debug_overlay: bool,
) -> Result<PathBuf> {
    if !cfg!(target_os = "macos") {
        bail!(
            "a macOS build is made on a Mac: the linker and the system frameworks are Apple's. \
             Run `jengine macos-build <project>` there (Rust from https://rustup.rs and Xcode's command-line tools)."
        );
    }
    let runtime = crate::windows::compile_native_runtime_for(engine_path, project_path, release, "macos")?;
    std::fs::create_dir_all(output_path)?;
    let bundle = write_app_bundle(project_path, output_path, &runtime, debug_overlay)?;
    // Apple Silicon runs no unsigned code, and the bundle was assembled after
    // the linker signed the binary: sign the whole thing ad hoc ("-"), which
    // is what runs on this Mac. Another Mac wants a Developer ID and
    // notarization on top.
    let status = crate::progress::run_streaming(
        Command::new("codesign").args(["--force", "--deep", "--sign", "-"]).arg(&bundle),
        "codesign",
    )
    .context("running codesign (Xcode's command-line tools: `xcode-select --install`)")?;
    if !status.success() {
        bail!("codesign could not sign {} ({status})", bundle.display());
    }
    crate::progress::step(&format!("macOS build complete: {}", bundle.display()));
    if generate_report {
        crate::report::generate_report(output_path, "macos", if release { "release" } else { "debug" }, None)?;
    }
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_paths() {
        assert_eq!(binary_name("Tom & Jerry's Game!"), "tom-jerry-s-game");
        assert_eq!(binary_name("  "), "game");
        // `wsl_path` runs on Windows only (WSL is how Windows builds for
        // Linux); elsewhere `C:\...` is a relative path and means nothing.
        if cfg!(windows) {
            assert_eq!(wsl_path(Path::new(r"C:\dev\jengine\projects\x")).unwrap(), "/mnt/c/dev/jengine/projects/x");
            assert_eq!(wsl_path(Path::new(r"\\?\D:\games")).unwrap(), "/mnt/d/games");
            // A relative path is resolved against the working directory first.
            let rel = wsl_path(Path::new("relative/path")).unwrap();
            assert!(rel.starts_with("/mnt/") && rel.ends_with("/relative/path"), "{rel}");
        }
    }

    #[test]
    fn the_wsl_script_builds_in_its_own_target_and_copies_back() {
        let s = wsl_build_script(
            "/mnt/c/e",
            &["build".into(), "-p".into(), "jengine-runtime".into(), "--release".into()],
            "my-game-linux",
            "release",
            "jengine-runtime",
            "/mnt/c/out/my game",
        );
        assert!(s.contains("cd '/mnt/c/e'"));
        assert!(s.contains("cargo 'build' '-p' 'jengine-runtime' '--release'"));
        assert!(s.contains("/my-game-linux"), "a target directory of its own: {s}");
        assert!(s.contains("cp \"$CARGO_TARGET_DIR/release/jengine-runtime\" '/mnt/c/out/my game'"));
        assert!(s.contains(". \"$HOME/.jengine/wsl-env.sh\""));
    }

    #[test]
    fn an_icns_holds_four_png_sizes() {
        let img = image::DynamicImage::new_rgba8(64, 64);
        let icns = icns_from_image(&img).unwrap();
        assert_eq!(&icns[..4], b"icns");
        assert_eq!(u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize, icns.len());
        // The first entry: ic07, then a PNG signature.
        assert_eq!(&icns[8..12], b"ic07");
        assert_eq!(&icns[16..20], b"\x89PNG");
    }

    #[test]
    fn a_bundle_has_the_layout_macos_expects() {
        let dir = std::env::temp_dir().join(format!("jengine-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("proj");
        std::fs::create_dir_all(project.join("assets/scenes")).unwrap();
        std::fs::write(project.join("assets/scenes/main.scene"), r#"{"name":"main","entities":[]}"#).unwrap();
        std::fs::write(
            project.join("project.jengine"),
            r#"{"name":"Lab","default_scene":"scenes/main.scene","settings":{"entry_scene":"scenes/main.scene","app":{"id":"com.example.lab","name":"Lab Game","version":"1.2.0","build":7}}}"#,
        )
        .unwrap();
        let runtime = dir.join("fake-runtime");
        std::fs::write(&runtime, b"#!").unwrap();
        let bundle = write_app_bundle(&project, &dir.join("out"), &runtime, false).unwrap();
        assert!(bundle.ends_with("Lab Game.app"));
        let c = bundle.join("Contents");
        assert!(c.join("MacOS/lab-game").is_file());
        assert!(c.join("Resources/project.jengine").is_file(), "the game where game_root looks for it");
        assert!(c.join("Resources/assets/scenes/main.scene").is_file());
        let plist = std::fs::read_to_string(c.join("Info.plist")).unwrap();
        assert!(plist.contains("<string>com.example.lab</string>"));
        assert!(plist.contains("<string>lab-game</string>"));
        assert!(plist.contains("<string>1.2.0</string>") && plist.contains("<string>7</string>"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn macos_is_refused_off_a_mac() {
        if cfg!(target_os = "macos") {
            return;
        }
        let err = build_macos(Path::new("."), Path::new("."), Path::new("."), true, false, false).unwrap_err();
        assert!(err.to_string().contains("made on a Mac"), "{err}");
    }
}
