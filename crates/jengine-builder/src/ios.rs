//! iOS builds: a flat `<Name>.app` - the runtime, `Info.plist`, the game's
//! files beside them and the icons - signed, and installed and started on the
//! simulator or a device.
//!
//! Like the Android build, there is no Xcode project: cargo compiles the
//! project's generated runtime crate for `aarch64-apple-ios` (a device) or
//! `aarch64-apple-ios-sim` (the simulator on an Apple Silicon Mac), and this
//! module lays the bundle out around it. Every step past laying it out needs
//! a Mac (Apple's linker, `codesign`, `xcrun`), so [`build_ios`] refuses
//! anywhere else; [`write_ios_bundle`] and the plist are plain file work and
//! tested on any host.
//!
//! The runtime finds its game in the bundle (the executable's folder, via
//! `jengine_runtime::game_root`), saves under the app's own
//! `Library/Application Support` (`bundle_save_dir` - the bundle is
//! read-only), and keeps its UI out of the notch (winit's safe area).
//!
//! Signing: the simulator takes an ad-hoc signature. A device needs a
//! development identity from the keychain and a provisioning profile naming
//! the app's id and the device - `--identity` and `--profile`, or
//! `JENGINE_IOS_IDENTITY` / `JENGINE_IOS_PROFILE`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::app::{Orientation, ResolvedApp};

/// The oldest iOS a build runs on. `UILaunchScreen` (an empty one means
/// "full screen, the background colour") needs 14; 15 is what wgpu's Metal
/// backend is tested against.
pub const MIN_IOS: &str = "15.0";

/// Where an iOS build runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IosTarget {
    /// The simulator, on an Apple Silicon Mac.
    Simulator,
    /// An iPhone or iPad.
    Device,
}

impl IosTarget {
    pub fn triple(self) -> &'static str {
        match self {
            IosTarget::Simulator => "aarch64-apple-ios-sim",
            IosTarget::Device => "aarch64-apple-ios",
        }
    }

    fn platform(self) -> &'static str {
        match self {
            IosTarget::Simulator => "iPhoneSimulator",
            IosTarget::Device => "iPhoneOS",
        }
    }

    /// The folder under the output path a bundle for this target goes in.
    /// Named for what it runs on, because a simulator `.app` looks like a
    /// Mac app in Finder, and double-clicked it dies before any code runs
    /// ("DYLD_ROOT_PATH not set for simulator program").
    pub fn folder(self) -> &'static str {
        match self {
            IosTarget::Simulator => "iphone-simulator",
            IosTarget::Device => "iphone",
        }
    }
}

/// What `build_ios` was asked for.
#[derive(Debug, Clone)]
pub struct IosOptions {
    pub target: IosTarget,
    pub release: bool,
    pub debug_overlay: bool,
    /// Install the app (the booted simulator, or `device`).
    pub install: bool,
    /// Start it after installing.
    pub launch: bool,
    /// A device's identifier (`xcrun devicectl list devices`); the first one
    /// connected when `None`.
    pub device: Option<String>,
    /// The signing identity for a device build ("Apple Development: ...").
    pub identity: Option<String>,
    /// The `.mobileprovision` for a device build.
    pub profile: Option<PathBuf>,
}

/// The icons an app carries without an asset catalog: `CFBundleIconFiles`
/// names the base, and iOS picks `@2x` / `@3x` and the `~ipad` ones by
/// itself. (A store upload wants an asset catalog, which is `actool`'s.)
pub const IOS_ICONS: &[(&str, u32)] = &[
    ("AppIcon60x60@2x.png", 120),
    ("AppIcon60x60@3x.png", 180),
    ("AppIcon76x76@2x~ipad.png", 152),
    ("AppIcon83.5x83.5@2x~ipad.png", 167),
];

/// `settings.app.id` as an iOS bundle identifier: letters, digits, hyphens
/// and dots only. The id is shared with Android, whose package names take
/// underscores and refuse hyphens - so the project-derived default
/// (`com.jengine.engine_test_lab`) is valid there and refused here, by Xcode,
/// a provisioning profile and the App Store alike. Any other character
/// becomes a hyphen.
pub fn bundle_id(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '-' })
        .collect()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// The orientations `settings.app.orientation` allows, as `UIInterfaceOrientation`s.
pub fn orientations(o: Orientation) -> &'static [&'static str] {
    match o {
        Orientation::Landscape => &["UIInterfaceOrientationLandscapeLeft", "UIInterfaceOrientationLandscapeRight"],
        Orientation::Portrait => &["UIInterfaceOrientationPortrait"],
        Orientation::Any => &[
            "UIInterfaceOrientationPortrait",
            "UIInterfaceOrientationLandscapeLeft",
            "UIInterfaceOrientationLandscapeRight",
        ],
    }
}

/// An iOS `Info.plist` for the game, from `settings.app`.
pub fn ios_info_plist(app: &ResolvedApp, executable: &str, target: IosTarget, has_icon: bool) -> String {
    let list = |items: &[&str]| items.iter().map(|i| format!("\t\t<string>{i}</string>\n")).collect::<String>();
    let orient = list(orientations(app.orientation));
    let icons = if has_icon {
        "\t<key>CFBundleIcons</key>\n\t<dict>\n\t\t<key>CFBundlePrimaryIcon</key>\n\t\t<dict>\n\
         \t\t\t<key>CFBundleIconFiles</key>\n\t\t\t<array><string>AppIcon60x60</string></array>\n\t\t</dict>\n\t</dict>\n\
         \t<key>CFBundleIcons~ipad</key>\n\t<dict>\n\t\t<key>CFBundlePrimaryIcon</key>\n\t\t<dict>\n\
         \t\t\t<key>CFBundleIconFiles</key>\n\t\t\t<array><string>AppIcon60x60</string><string>AppIcon76x76</string><string>AppIcon83.5x83.5</string></array>\n\t\t</dict>\n\t</dict>\n"
    } else {
        ""
    };
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
         \t<key>CFBundleSupportedPlatforms</key>\n\t<array>\n\t\t<string>{platform}</string>\n\t</array>\n\
         \t<key>MinimumOSVersion</key>\n\t<string>{MIN_IOS}</string>\n\
         \t<key>UIDeviceFamily</key>\n\t<array>\n\t\t<integer>1</integer>\n\t\t<integer>2</integer>\n\t</array>\n\
         \t<key>UIRequiredDeviceCapabilities</key>\n\t<array>\n\t\t<string>arm64</string>\n\t\t<string>metal</string>\n\t</array>\n\
         \t<key>UILaunchScreen</key>\n\t<dict/>\n\
         \t<key>UIApplicationSceneManifest</key>\n\t<dict>\n\
         \t\t<key>UIApplicationSupportsMultipleScenes</key>\n\t\t<false/>\n\
         \t\t<key>UISceneConfigurations</key>\n\t\t<dict>\n\
         \t\t\t<key>UIWindowSceneSessionRoleApplication</key>\n\t\t\t<array>\n\
         \t\t\t\t<dict>\n\t\t\t\t\t<key>UISceneConfigurationName</key>\n\t\t\t\t\t<string>Default Configuration</string>\n\t\t\t\t</dict>\n\
         \t\t\t</array>\n\t\t</dict>\n\t</dict>\n\
         \t<key>UIRequiresFullScreen</key>\n\t<true/>\n\
         \t<key>CADisableMinimumFrameDurationOnPhone</key>\n\t<true/>\n\
         \t<key>UIStatusBarHidden</key>\n\t<true/>\n\
         \t<key>UIViewControllerBasedStatusBarAppearance</key>\n\t<false/>\n\
         \t<key>UISupportedInterfaceOrientations</key>\n\t<array>\n{orient}\t</array>\n\
         \t<key>UISupportedInterfaceOrientations~ipad</key>\n\t<array>\n{orient}\t</array>\n\
         {icons}\
         </dict>\n</plist>\n",
        name = esc(app.name.trim()),
        id = esc(&bundle_id(&app.id)),
        exe = esc(executable),
        version = esc(&app.version),
        build = app.build,
        platform = target.platform(),
    )
}

/// Lay out `<Name>.app` in `output_path` around an already-built runtime:
/// the executable, `Info.plist`, `PkgInfo`, the icons and the staged game,
/// all at the bundle's root (an iOS bundle is flat). Returns the bundle.
pub fn write_ios_bundle(
    project_path: &Path,
    output_path: &Path,
    runtime: &Path,
    target: IosTarget,
    debug_overlay: bool,
) -> Result<PathBuf> {
    let app = crate::app::read(project_path)?;
    let exe = crate::desktop::binary_name(&app.name);
    let bundle = output_path.join(format!("{}.app", app.name.trim()));
    let _ = std::fs::remove_dir_all(&bundle);
    let packs = crate::asset_packs::read_settings(project_path);
    let packs_out = crate::asset_packs::packs_beside_bundle(output_path, "iOS", &packs);
    crate::stage::stage_game(project_path, &bundle, debug_overlay, &packs_out)?;
    let dest = bundle.join(&exe);
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
                // iOS draws an icon's transparency black: lay it on the
                // background colour the launcher would have shown.
                for (file, size) in IOS_ICONS {
                    flat_icon(&img, *size, app.icon_background).save(bundle.join(file))?;
                }
                has_icon = true;
            }
            Err(e) => log::warn!("settings.app.icon {icon} could not be read ({e}); the app has no icon"),
        }
    }
    std::fs::write(bundle.join("Info.plist"), ios_info_plist(&app, &exe, target, has_icon))?;
    std::fs::write(bundle.join("PkgInfo"), "APPL????")?;
    Ok(bundle)
}

/// The icon at `size`, laid on `background` - iOS draws an icon's
/// transparency black.
fn flat_icon(img: &image::DynamicImage, size: u32, background: [u8; 3]) -> image::RgbImage {
    let [r, g, b] = background;
    let scaled = img.resize_exact(size, size, image::imageops::FilterType::Lanczos3).to_rgba8();
    let mut flat = image::RgbImage::from_pixel(size, size, image::Rgb([r, g, b]));
    for (x, y, p) in scaled.enumerate_pixels() {
        let a = p[3] as u32;
        let mix = |c: u8, bg: u8| ((c as u32 * a + bg as u32 * (255 - a)) / 255) as u8;
        flat.put_pixel(x, y, image::Rgb([mix(p[0], r), mix(p[1], g), mix(p[2], b)]));
    }
    flat
}

/// The asset catalog for an icon: one 1024 px universal image, which Xcode
/// (since 14) and the App Store take as the whole set. `(path, contents)`
/// pairs under the `.xcassets` folder.
pub fn icon_catalog_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Contents.json", r#"{ "info": { "author": "jengine", "version": 1 } }"#),
        (
            "AppIcon.appiconset/Contents.json",
            r#"{ "images": [ { "filename": "icon-1024.png", "idiom": "universal", "platform": "ios", "size": "1024x1024" } ], "info": { "author": "jengine", "version": 1 } }"#,
        ),
    ]
}

/// Give the bundle its icon the way Xcode does: an asset catalog compiled by
/// `actool` into `Assets.car`, and the plist's icon keys replaced by the ones
/// `actool` reports (`CFBundleIconName` among them), which current iOS and an
/// App Store upload read. The flat PNGs `write_ios_bundle` left stay, for an
/// older iOS. A failure is logged and leaves those as they were.
fn compile_icon_catalog(project_path: &Path, output_path: &Path, bundle: &Path, target: IosTarget) -> Result<()> {
    let app = crate::app::read(project_path)?;
    let Some(icon) = &app.icon else { return Ok(()) };
    let img = image::open(project_path.join(icon))?;
    let work = output_path.join("icon-catalog");
    let _ = std::fs::remove_dir_all(&work);
    let catalog = work.join("Assets.xcassets");
    for (path, contents) in icon_catalog_files() {
        let file = catalog.join(path);
        std::fs::create_dir_all(file.parent().unwrap())?;
        std::fs::write(file, contents)?;
    }
    flat_icon(&img, 1024, app.icon_background).save(catalog.join("AppIcon.appiconset/icon-1024.png"))?;
    let partial = work.join("partial.plist");
    let platform = match target {
        IosTarget::Simulator => "iphonesimulator",
        IosTarget::Device => "iphoneos",
    };
    run(
        Command::new("xcrun")
            .args(["actool", "--compile"])
            .arg(bundle)
            .args(["--platform", platform, "--minimum-deployment-target", MIN_IOS, "--app-icon", "AppIcon"])
            .args(["--target-device", "iphone", "--target-device", "ipad", "--output-partial-info-plist"])
            .arg(&partial)
            .arg(&catalog),
        "actool",
    )?;
    let plist = bundle.join("Info.plist");
    for key in ["CFBundleIcons", "CFBundleIcons~ipad"] {
        let _ = Command::new("/usr/libexec/PlistBuddy").args(["-c", &format!("Delete :{key}")]).arg(&plist).output();
    }
    run(
        Command::new("/usr/libexec/PlistBuddy").args(["-c", &format!("Merge {}", partial.display())]).arg(&plist),
        "PlistBuddy merge",
    )?;
    let _ = std::fs::remove_dir_all(&work);
    Ok(())
}

/// Build, sign and (optionally) install and start an iOS app. Only on a Mac.
/// Returns the bundle's path.
pub fn build_ios(engine_path: &Path, project_path: &Path, output_path: &Path, opts: &IosOptions) -> Result<PathBuf> {
    if !cfg!(target_os = "macos") {
        bail!(
            "an iOS build is made on a Mac: Apple's linker, codesign and the simulator exist nowhere else. \
             Run `jengine ios-build <project>` there (Xcode, and `rustup target add {}`).",
            opts.target.triple()
        );
    }
    let app = crate::app::read(project_path)?;
    let id = bundle_id(&app.id);
    if id != app.id {
        log::info!("the iOS bundle id is {id} (settings.app.id {} has characters iOS does not allow)", app.id);
    }
    if opts.target == IosTarget::Device && app.id_defaulted {
        log::warn!(
            "settings.app.id is not set, so the bundle id is {id} - a provisioning profile must name it"
        );
    }

    // Compile for the target. Without a pin, Metal is the only API iOS has.
    let mut plan = crate::windows::plan_native_runtime(engine_path, project_path, opts.release, "ios")?;
    if !plan.args.iter().any(|a| a == "--no-default-features") {
        plan.args.extend(["--no-default-features".into(), "--features".into(), "backend-metal".into()]);
    }
    plan.args.extend(["--target".into(), opts.target.triple().into()]);
    // The binary's own minimum OS, so it matches `MinimumOSVersion` in the
    // plist (rustc's default for the iOS targets is older).
    let status = crate::progress::run_streaming(
        Command::new("cargo").args(&plan.args).current_dir(&plan.build_dir).env("IPHONEOS_DEPLOYMENT_TARGET", MIN_IOS),
        "cargo",
    )
    .context("running cargo")?;
    if !status.success() {
        bail!(
            "the iOS build failed ({status}). Is the target installed? `rustup target add {}`",
            opts.target.triple()
        );
    }
    let runtime = crate::cargo_target_dir(&plan.build_dir).join(opts.target.triple()).join(plan.profile).join(&plan.binary_stem);

    let output_path = output_path.join(opts.target.folder());
    std::fs::create_dir_all(&output_path)?;
    let bundle = write_ios_bundle(project_path, &output_path, &runtime, opts.target, opts.debug_overlay)?;
    if let Err(e) = compile_icon_catalog(project_path, &output_path, &bundle, opts.target) {
        log::warn!("the icon's asset catalog could not be built ({e:#}); the app keeps its plain icon files");
    }
    sign(&bundle, opts)?;
    crate::progress::step(&format!("iOS build complete: {}", bundle.display()));
    if !opts.install {
        crate::progress::step(&match opts.target {
            IosTarget::Simulator => format!(
                "This is an iPhone simulator app, not a Mac app - opened from Finder it crashes before it starts. \
                 Run it with `jengine ios-build <project> --run`, or `xcrun simctl install booted \"{}\"` \
                 then `xcrun simctl launch booted {}`.",
                bundle.display(),
                id
            ),
            IosTarget::Device => format!(
                "This is an iPhone app, not a Mac app. Install it with `jengine ios-build <project> --target device --run`, \
                 or `xcrun devicectl device install app --device <id> \"{}\"`.",
                bundle.display()
            ),
        });
    }

    if opts.install {
        install_and_launch(&bundle, &id, opts)?;
    }
    Ok(bundle)
}

fn run(cmd: &mut Command, what: &str) -> Result<()> {
    let status = crate::progress::run_streaming(cmd, what).with_context(|| format!("running {what}"))?;
    if !status.success() {
        bail!("{what} failed ({status})");
    }
    Ok(())
}

/// Ad hoc for the simulator; with the identity and the profile's
/// entitlements for a device.
fn sign(bundle: &Path, opts: &IosOptions) -> Result<()> {
    if opts.target == IosTarget::Simulator {
        return run(Command::new("codesign").args(["--force", "--sign", "-"]).arg(bundle), "codesign");
    }
    let identity = opts
        .identity
        .clone()
        .or_else(|| std::env::var("JENGINE_IOS_IDENTITY").ok())
        .context("a device build is signed by a development identity: --identity \"Apple Development: ...\" (`security find-identity -v -p codesigning` lists them)")?;
    let profile = opts
        .profile
        .clone()
        .or_else(|| std::env::var_os("JENGINE_IOS_PROFILE").map(PathBuf::from))
        .context("a device build needs a provisioning profile naming the app and the device: --profile <x.mobileprovision>")?;
    std::fs::copy(&profile, bundle.join("embedded.mobileprovision"))
        .with_context(|| format!("copying {}", profile.display()))?;
    // The profile is a signed plist; its Entitlements dict is what the app
    // may claim, and codesign wants it as a file of its own.
    let decoded = Command::new("security").args(["cms", "-D", "-i"]).arg(&profile).output().context("running security cms")?;
    if !decoded.status.success() {
        bail!("could not read the provisioning profile {}", profile.display());
    }
    let dir = bundle.parent().unwrap_or(bundle);
    let profile_plist = dir.join("profile.plist");
    std::fs::write(&profile_plist, &decoded.stdout)?;
    let entitlements = Command::new("/usr/libexec/PlistBuddy")
        .args(["-x", "-c", "Print :Entitlements"])
        .arg(&profile_plist)
        .output()
        .context("running PlistBuddy")?;
    let _ = std::fs::remove_file(&profile_plist);
    if !entitlements.status.success() {
        bail!("the provisioning profile {} has no entitlements", profile.display());
    }
    let ent_path = dir.join("entitlements.plist");
    std::fs::write(&ent_path, &entitlements.stdout)?;
    let result = run(
        Command::new("codesign")
            .args(["--force", "--sign", &identity, "--entitlements"])
            .arg(&ent_path)
            .arg(bundle),
        "codesign",
    );
    let _ = std::fs::remove_file(&ent_path);
    result
}

fn install_and_launch(bundle: &Path, id: &str, opts: &IosOptions) -> Result<()> {
    match opts.target {
        IosTarget::Simulator => {
            // Boot one if none is: the first available iPhone.
            let booted = Command::new("xcrun").args(["simctl", "list", "devices", "booted"]).output()?;
            if !String::from_utf8_lossy(&booted.stdout).contains("(Booted)") {
                let list = Command::new("xcrun").args(["simctl", "list", "devices", "available"]).output()?;
                let text = String::from_utf8_lossy(&list.stdout);
                let udid = first_iphone(&text).context("no simulator is available - make one in Xcode (Window > Devices and Simulators)")?;
                run(Command::new("xcrun").args(["simctl", "boot", &udid]), "simctl boot")?;
            }
            open_simulator_app();
            run(Command::new("xcrun").args(["simctl", "install", "booted"]).arg(bundle), "simctl install")?;
            if opts.launch {
                run(Command::new("xcrun").args(["simctl", "launch", "booted", id]), "simctl launch")?;
            }
        }
        IosTarget::Device => {
            let device = match &opts.device {
                Some(d) => d.clone(),
                None => {
                    let list = Command::new("xcrun").args(["devicectl", "list", "devices"]).output()?;
                    first_device(&String::from_utf8_lossy(&list.stdout))
                        .context("no device is connected (`xcrun devicectl list devices`), or name one with --device")?
                }
            };
            run(
                Command::new("xcrun").args(["devicectl", "device", "install", "app", "--device", &device]).arg(bundle),
                "devicectl install",
            )?;
            if opts.launch {
                run(
                    Command::new("xcrun").args(["devicectl", "device", "process", "launch", "--device", &device, id]),
                    "devicectl launch",
                )?;
            }
        }
    }
    Ok(())
}

/// Show the simulator's window. A device booted by `simctl` runs headless,
/// and `open -a Simulator` finds nothing until Launch Services has seen
/// Xcode's copy, so it is opened by its path inside the active Xcode -
/// `Simulator.app` up to Xcode 26, `DeviceHub.app` from Xcode 27, which has no
/// Simulator.app.
fn open_simulator_app() {
    let developer = Command::new("xcode-select").arg("-p").output().ok().map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()));
    let app = developer.and_then(|d| {
        [d.join("Applications/Simulator.app"), d.join("../Applications/DeviceHub.app")].into_iter().find(|p| p.exists())
    });
    let _ = match app {
        Some(app) => Command::new("open").arg(app).status(),
        None => Command::new("open").args(["-a", "Simulator"]).status(),
    };
}

/// The first iPhone's UDID in `simctl list devices available`:
/// `    iPhone 15 (4F3C...-...) (Shutdown)`.
pub fn first_iphone(list: &str) -> Option<String> {
    list.lines().map(str::trim).filter(|l| l.starts_with("iPhone")).find_map(|l| {
        let open = l.find(" (")? + 2;
        let close = l[open..].find(')')? + open;
        let udid = &l[open..close];
        (udid.len() == 36 && udid.matches('-').count() == 4).then(|| udid.to_string())
    })
}

/// The first connected device's identifier in `devicectl list devices`,
/// whose rows are `Name   Hostname   Identifier   State   Model`.
///
/// Two things Xcode 27 changed: the list carries **booted simulators** too,
/// marked `simulated` in its last column (installing a phone build there fails
/// with EBADARCH), and a phone's identifier is its UDID -
/// `00008130-00023D360A30001C`, 25 characters - where it used to be a
/// 36-character UUID. So any identifier of hex digits and hyphens counts.
pub fn first_device(list: &str) -> Option<String> {
    let is_id = |w: &str| {
        w.len() >= 24 && w.contains('-') && w.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
    };
    list.lines()
        .filter(|l| l.contains("connected") && !l.contains("unavailable") && !l.contains("simulated"))
        .find_map(|l| l.split_whitespace().find(|w| is_id(w)).map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(dir: &Path, app: serde_json::Value) {
        std::fs::create_dir_all(dir.join("assets").join("scenes")).unwrap();
        std::fs::write(dir.join("assets").join("scenes").join("main.scene"), "{}").unwrap();
        let doc = serde_json::json!({ "name": "pocket", "settings": { "title": "Pocket", "app": app } });
        std::fs::write(dir.join("project.jengine"), doc.to_string()).unwrap();
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jengine-ios-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_flat_bundle_with_its_game_beside_it() {
        let root = temp("bundle");
        let proj = root.join("proj");
        project(
            &proj,
            serde_json::json!({ "id": "com.example.pocket", "version": "1.2", "build": 3, "orientation": "landscape", "icon": "icon.png" }),
        );
        image::RgbaImage::from_pixel(64, 64, image::Rgba([200, 40, 40, 128])).save(proj.join("icon.png")).unwrap();
        let runtime = root.join("runtime-bin");
        std::fs::write(&runtime, b"not really a binary").unwrap();

        let bundle = write_ios_bundle(&proj, &root.join("out"), &runtime, IosTarget::Simulator, true).unwrap();
        assert!(bundle.ends_with("Pocket.app"));
        // Flat: executable, plist and game at the root, as game_root expects.
        assert!(bundle.join("pocket").is_file());
        assert!(bundle.join("project.jengine").is_file());
        assert!(bundle.join("assets").is_dir());
        assert_eq!(std::fs::read_to_string(bundle.join("PkgInfo")).unwrap(), "APPL????");
        for (file, size) in IOS_ICONS {
            let icon = image::open(bundle.join(file)).unwrap();
            assert_eq!((icon.width(), icon.height()), (*size, *size), "{file}");
            // Flattened onto the background: no alpha left for iOS to blacken.
            assert!(!icon.color().has_alpha(), "{file}");
        }
        let plist = std::fs::read_to_string(bundle.join("Info.plist")).unwrap();
        for needle in [
            "<string>com.example.pocket</string>",
            "<key>CFBundleExecutable</key>\n\t<string>pocket</string>",
            "<string>1.2</string>",
            "<key>CFBundleVersion</key>\n\t<string>3</string>",
            "<string>iPhoneSimulator</string>",
            "UIInterfaceOrientationLandscapeLeft",
            "<key>UILaunchScreen</key>",
            // Without it an iPhone holds every app at 60 Hz on a 120 Hz
            // ProMotion panel - the test lab drew 16.7 ms frames on one.
            "<key>CADisableMinimumFrameDurationOnPhone</key>\n\t<true/>",
            // iOS 27 refuses to launch an app without the scene lifecycle.
            "<key>UIApplicationSceneManifest</key>",
            "<key>UIWindowSceneSessionRoleApplication</key>",
            "AppIcon60x60",
        ] {
            assert!(plist.contains(needle), "Info.plist lacks {needle}:\n{plist}");
        }
        assert!(!plist.contains("UIInterfaceOrientationPortrait"));
    }

    #[test]
    fn a_device_plist_names_the_device_platform() {
        let root = temp("device");
        project(&root, serde_json::json!({ "orientation": "any" }));
        let app = crate::app::read(&root).unwrap();
        let plist = ios_info_plist(&app, "pocket", IosTarget::Device, false);
        assert!(plist.contains("<string>iPhoneOS</string>"));
        assert!(plist.contains("UIInterfaceOrientationPortrait"));
        assert!(!plist.contains("CFBundleIcons"));
    }

    #[test]
    fn simulators_and_devices_are_found_in_xcruns_lists() {
        let sims = "== Devices ==\n-- iOS 17.5 --\n    iPad Air (5th generation) (11111111-2222-3333-4444-555555555555) (Shutdown)\n    iPhone 15 (4F3C1A2B-1111-2222-3333-444455556666) (Shutdown)\n";
        assert_eq!(first_iphone(sims).as_deref(), Some("4F3C1A2B-1111-2222-3333-444455556666"));
        assert_eq!(first_iphone("== Devices ==\n"), None);
        let devices = "Name        Hostname                    Identifier                             State                Model\n\
                       ---------   -------------------------   ------------------------------------   ------------------   -----\n\
                       My iPhone   My-iPhone.coredevice.local   ABCDEF01-2345-6789-ABCD-EF0123456789   available (paired)   iPhone 15\n";
        assert_eq!(first_device(devices), None, "paired but not connected");
        let connected = devices.replace("available (paired)", "connected");
        assert_eq!(first_device(&connected).as_deref(), Some("ABCDEF01-2345-6789-ABCD-EF0123456789"));
        // Xcode 27: a booted simulator is listed too, and a phone is named by
        // its 25-character UDID. The phone is the one meant.
        let xcode27 = "Name            Hostname   Identifier                                    State       Model                            Reality  \n\
                       -------------   --------   -------------------------------------------   ---------   ------------------------------   ---------\n\
                       iPhone 18 Pro              3F682021-FBC3-4534-B5BC-D0802F58ADCB (UDID)   connected   iPhone 18 Pro (iPhone19,2)       simulated\n\
                       Juha's iPhone              00008130-00023D360A30001C (UDID)              connected   iPhone 15 Pro Max (iPhone16,2)   physical \n";
        assert_eq!(first_device(xcode27).as_deref(), Some("00008130-00023D360A30001C"));
    }

    #[test]
    fn a_bundle_id_has_no_underscores() {
        assert_eq!(bundle_id("com.jengine.engine_test_lab"), "com.jengine.engine-test-lab");
        assert_eq!(bundle_id("com.example.pocket"), "com.example.pocket");
        assert_eq!(bundle_id("com.x.a b+c"), "com.x.a-b-c");
    }

    #[test]
    fn the_icon_catalog_names_one_1024_image() {
        let files = icon_catalog_files();
        for (path, contents) in &files {
            serde_json::from_str::<serde_json::Value>(contents).unwrap_or_else(|e| panic!("{path}: {e}"));
        }
        let (_, set) = files.iter().find(|(p, _)| p.ends_with("AppIcon.appiconset/Contents.json")).unwrap();
        let set: serde_json::Value = serde_json::from_str(set).unwrap();
        assert_eq!(set["images"][0]["filename"], "icon-1024.png");
        assert_eq!(set["images"][0]["size"], "1024x1024");
    }

    #[test]
    fn refuses_off_a_mac() {
        if cfg!(target_os = "macos") {
            return;
        }
        let root = temp("refuse");
        project(&root, serde_json::json!({}));
        let opts = IosOptions {
            target: IosTarget::Simulator,
            release: false,
            debug_overlay: true,
            install: false,
            launch: false,
            device: None,
            identity: None,
            profile: None,
        };
        let err = build_ios(&root, &root, &root.join("out"), &opts).unwrap_err().to_string();
        assert!(err.contains("on a Mac"), "{err}");
    }
}
