//! The Android entry point.
//!
//! Android does not call `main`. The activity loads `libmain.so` and calls its
//! `android_main`, which a generated project crate
//! (`jengine_builder::android`) defines as a one-line call to [`run_with`].
//!
//! Everything after startup is the desktop runtime: the same event loop, the
//! same scene runner, the same scripts. What differs is getting to it:
//!
//! - **Logging** goes to logcat (`adb logcat -s jengine`). Stderr goes nowhere
//!   on Android, so without this a failed start is a black screen.
//! - **Assets** are inside the APK, which is a zip and not a directory, and
//!   the engine reads its assets with `std::fs` in a hundred places. So the
//!   first launch of each build unpacks them into the app's private storage
//!   and runs from there, exactly as a desktop build runs from its folder.
//!   The builder writes the file list ([`MANIFEST`]), because the NDK's asset
//!   manager cannot list subdirectories.
//! - **Saves** go beside the unpacked tree, not inside it, since a new build
//!   replaces the tree.
//! - **One activity per process.** winit creates one event loop per process
//!   and refuses a second, and 0.30 drops Android's `onDestroy` on the floor
//!   (a `TODO` in its source), so an event loop never learns its activity is
//!   gone. Android keeps a finished app's process cached, though, and calls
//!   `android_main` again in it on the next launch - which then cannot start.
//!   So the process exits when the game does, and if a second activity
//!   arrives while the first still runs, that too ends the process: the
//!   alternative is the old activity's `onDestroy` waiting forever on an
//!   event loop that will not return, which Android reports as the app not
//!   responding.

use std::ffi::CString;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use winit::event_loop::EventLoop;
use winit::platform::android::EventLoopBuilderExtAndroid;

pub use winit::platform::android::activity::AndroidApp;

use crate::PackageHook;

/// The list of files the builder packed into the APK's `assets/`, one path
/// per line after a first `build <id>` line. The id changes with every build,
/// which is what tells a launch that its unpacked copy is stale.
pub const MANIFEST: &str = "jengine-files.txt";

/// Set once an `android_main` has started in this process.
static STARTED: AtomicBool = AtomicBool::new(false);

/// The activity, kept for asking it about the screen (`safe_insets`).
static APP: std::sync::OnceLock<AndroidApp> = std::sync::OnceLock::new();

/// Raise or lower the soft keyboard: a text field took focus or let it go
/// (`text_native`).
pub(crate) fn show_keyboard(show: bool) {
    if let Some(app) = APP.get() {
        if show {
            app.show_soft_input(true);
        } else {
            app.hide_soft_input(false);
        }
    }
}

/// The display cut-out - notch, camera hole, rounded corner - as insets in
/// physical pixels, or `None` if the activity could not be asked.
///
/// `getRootWindowInsets().getDisplayCutout()` on the activity's decor view,
/// over JNI: a read, which unlike hiding the system bars is allowed off the
/// UI thread. No cut-out (a phone without one, or before Android 9) is zero
/// insets. The immersive activity lets the game draw under the cut-out
/// (`LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES`), which is what makes these
/// insets the UI's business; asked again on every resume and resize, since
/// turning the phone moves the notch to another edge.
pub(crate) fn safe_insets() -> Option<jengine_core::ui::scaling::SafeInsets> {
    use jni::objects::JObject;
    let app = APP.get()?;
    // SAFETY: the pointers are the process's JavaVM and the activity's global
    // reference, both owned by android-activity and alive while it runs.
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
    // A local frame, so the references made here are freed however often
    // this is asked - the game's thread stays attached for the process.
    let result = env.with_local_frame(16, |env| -> jni::errors::Result<Option<[i32; 4]>> {
        let window = env.call_method(&activity, "getWindow", "()Landroid/view/Window;", &[])?.l()?;
        let decor = env.call_method(&window, "getDecorView", "()Landroid/view/View;", &[])?.l()?;
        let insets = env
            .call_method(&decor, "getRootWindowInsets", "()Landroid/view/WindowInsets;", &[])?
            .l()?;
        if insets.is_null() {
            return Ok(None); // not attached to a window yet
        }
        let cutout = env
            .call_method(&insets, "getDisplayCutout", "()Landroid/view/DisplayCutout;", &[])?
            .l()?;
        if cutout.is_null() {
            return Ok(Some([0; 4]));
        }
        let mut side = |name: &str| -> jni::errors::Result<i32> {
            env.call_method(&cutout, name, "()I", &[])?.i()
        };
        Ok(Some([
            side("getSafeInsetLeft")?,
            side("getSafeInsetTop")?,
            side("getSafeInsetRight")?,
            side("getSafeInsetBottom")?,
        ]))
    });
    let insets = match result {
        Ok(v) => v?,
        Err(e) => {
            // A Java exception left pending would poison every later call.
            let _ = env.exception_clear();
            log::warn!("could not read the display cut-out: {e}");
            return None;
        }
    };
    let [left, top, right, bottom] = insets.map(|v| v.max(0) as f32);
    Some(jengine_core::ui::scaling::SafeInsets { left, top, right, bottom })
}

/// A string a debuggable build was launched with, if any - `scene` (where
/// to start: `adb shell am start -n <app>/<activity> --es scene
/// scenes/05-prefabs.scene`) or `backend` (`gl`, to test the GLES fallback).
///
/// This is what `jengine android-smoke` walks a project with, one fresh
/// process per scene. Only a **debuggable** build listens - a debug-key
/// build is (`android:debuggable`), a release-key build is not - so a
/// shipped game cannot be started anywhere but its entry scene by another
/// app's intent.
fn launch_extra(app: &AndroidApp, name: &str) -> Option<String> {
    use jni::objects::{JObject, JString};
    // `ApplicationInfo.FLAG_DEBUGGABLE`.
    const FLAG_DEBUGGABLE: i32 = 2;
    // SAFETY: as in `safe_insets`.
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
    let result = env.with_local_frame(8, |env| -> jni::errors::Result<Option<String>> {
        let info = env
            .call_method(&activity, "getApplicationInfo", "()Landroid/content/pm/ApplicationInfo;", &[])?
            .l()?;
        if env.get_field(&info, "flags", "I")?.i()? & FLAG_DEBUGGABLE == 0 {
            return Ok(None);
        }
        let intent = env.call_method(&activity, "getIntent", "()Landroid/content/Intent;", &[])?.l()?;
        if intent.is_null() {
            return Ok(None);
        }
        let key = env.new_string(name)?;
        let value = env
            .call_method(&intent, "getStringExtra", "(Ljava/lang/String;)Ljava/lang/String;", &[(&key).into()])?
            .l()?;
        if value.is_null() {
            return Ok(None);
        }
        let value: String = env.get_string(&JString::from(value))?.into();
        Ok(Some(value))
    });
    match result {
        Ok(scene) => scene.filter(|s| !s.is_empty()),
        Err(e) => {
            let _ = env.exception_clear();
            log::warn!("could not read the launch intent: {e}");
            None
        }
    }
}

/// Whether Android still knows the input device `id`
/// (`InputDevice.getDevice(id) != null`). A controller switched off sends no
/// event to say so; `gamepad_android` asks this once a second. Answers `true`
/// when it cannot ask, so a failed call never unplugs a working pad.
pub(crate) fn input_device_exists(id: i32) -> bool {
    use jni::objects::JValue;
    let Some(app) = APP.get() else { return true };
    // SAFETY: as in `safe_insets`.
    let Ok(vm) = (unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) }) else {
        return true;
    };
    let Ok(mut env) = vm.attach_current_thread() else { return true };
    let result = env.with_local_frame(4, |env| -> jni::errors::Result<bool> {
        let device = env
            .call_static_method(
                "android/view/InputDevice",
                "getDevice",
                "(I)Landroid/view/InputDevice;",
                &[JValue::Int(id)],
            )?
            .l()?;
        Ok(!device.is_null())
    });
    match result {
        Ok(exists) => exists,
        Err(e) => {
            let _ = env.exception_clear();
            log::warn!("could not ask whether input device {id} exists: {e}");
            true
        }
    }
}

/// What Android calls input device `id` and its USB vendor id
/// (`InputDevice.getName` / `getVendorId`) - which family a controller is,
/// for button prompts.
pub(crate) fn input_device_identity(id: i32) -> Option<(String, Option<u16>)> {
    use jni::objects::{JString, JValue};
    let app = APP.get()?;
    // SAFETY: as in `safe_insets`.
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let result = env.with_local_frame(8, |env| -> jni::errors::Result<Option<(String, Option<u16>)>> {
        let device = env
            .call_static_method(
                "android/view/InputDevice",
                "getDevice",
                "(I)Landroid/view/InputDevice;",
                &[JValue::Int(id)],
            )?
            .l()?;
        if device.is_null() {
            return Ok(None);
        }
        let name = env.call_method(&device, "getName", "()Ljava/lang/String;", &[])?.l()?;
        let name: String = env.get_string(&JString::from(name))?.into();
        let vendor = env.call_method(&device, "getVendorId", "()I", &[])?.i()?;
        Ok(Some((name, (vendor > 0).then_some(vendor as u16))))
    });
    match result {
        Ok(found) => found,
        Err(e) => {
            let _ = env.exception_clear();
            log::warn!("could not ask what input device {id} is: {e}");
            None
        }
    }
}

/// Buzz `vibrator` once: `VibrationEffect.createOneShot(ms, amplitude)`
/// (Android 8+), or `cancel()` for nothing.
fn vibrate_with(env: &mut jni::JNIEnv<'_>, vibrator: &jni::objects::JObject<'_>, ms: i64, amplitude: i32) -> jni::errors::Result<()> {
    use jni::objects::JValue;
    if vibrator.is_null() {
        return Ok(());
    }
    if ms <= 0 || amplitude <= 0 {
        env.call_method(vibrator, "cancel", "()V", &[])?;
        return Ok(());
    }
    let effect = env
        .call_static_method(
            "android/os/VibrationEffect",
            "createOneShot",
            "(JI)Landroid/os/VibrationEffect;",
            &[JValue::Long(ms), JValue::Int(amplitude.clamp(1, 255))],
        )?
        .l()?;
    env.call_method(vibrator, "vibrate", "(Landroid/os/VibrationEffect;)V", &[(&effect).into()])?;
    Ok(())
}

/// Shared by both vibrators: attach, run `f` in a local frame, and clear
/// and log any Java exception rather than leaving it pending.
fn with_env(what: &str, f: impl FnOnce(&mut jni::JNIEnv<'_>, &jni::objects::JObject<'_>) -> jni::errors::Result<()>) {
    use jni::objects::JObject;
    let Some(app) = APP.get() else { return };
    // SAFETY: as in `safe_insets`.
    let Ok(vm) = (unsafe { jni::JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) }) else { return };
    let Ok(mut env) = vm.attach_current_thread() else { return };
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
    if let Err(e) = env.with_local_frame(16, |env| f(env, &activity)) {
        let _ = env.exception_clear();
        log::warn!("could not {what}: {e}");
    }
}

/// `Input.rumble` on an Android controller: its own vibrator
/// (`InputDevice.getVibrator`), one motor, so the stronger of the two.
pub(crate) fn vibrate_input_device(id: i32, ms: i64, amplitude: i32) {
    with_env("rumble a controller", |env, _| {
        use jni::objects::JValue;
        let device = env
            .call_static_method("android/view/InputDevice", "getDevice", "(I)Landroid/view/InputDevice;", &[JValue::Int(id)])?
            .l()?;
        if device.is_null() {
            return Ok(());
        }
        let vibrator = env.call_method(&device, "getVibrator", "()Landroid/os/Vibrator;", &[])?.l()?;
        if vibrator.is_null() || !env.call_method(&vibrator, "hasVibrator", "()Z", &[])?.z()? {
            return Ok(());
        }
        vibrate_with(env, &vibrator, ms, amplitude)
    });
}

/// `Input.vibrate`: the phone's own vibrator (the `VIBRATE` permission the
/// builder writes into every manifest).
pub(crate) fn vibrate_phone(ms: i64, amplitude: i32) {
    with_env("vibrate the phone", |env, activity| {
        let name = env.new_string("vibrator")?;
        let vibrator = env
            .call_method(activity, "getSystemService", "(Ljava/lang/String;)Ljava/lang/Object;", &[(&name).into()])?
            .l()?;
        vibrate_with(env, &vibrator, ms, amplitude)
    });
}

/// Run the engine inside an Android activity, registering the project's
/// packages through `add_packages` as [`crate::run_with`] does.
///
/// Never returns an error: there is nobody to return it to. A failure is
/// logged, and returning ends the activity.
pub fn run_with(app: AndroidApp, add_packages: &PackageHook) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            // wgpu probes GLES as well as Vulkan at startup and warns about
            // what EGL lacks each time - four lines per launch that say
            // nothing about the game.
            .with_filter(android_logger::FilterBuilder::new().parse("info,wgpu_hal=error").build())
            .with_tag("jengine"),
    );
    // A panic's message goes to stderr, which Android discards - so a crash
    // would leave nothing in logcat but the signal. Log it first.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        default_hook(info);
    }));

    if STARTED.swap(true, Ordering::SeqCst) {
        log::warn!(
            "Android started a second activity in a process whose game is still running;              ending the process so the next launch starts clean"
        );
        std::process::exit(0);
    }

    log::info!("jEngine Runtime starting (Android)...");
    if let Err(e) = run(app, add_packages) {
        log::error!("jEngine failed: {e:#}");
    }
    // The game is over (`quit_game`, or a failed start). Android would keep
    // this process for the next launch, where a second event loop cannot be
    // built - so end it here.
    log::info!("jEngine Runtime exiting");
    std::process::exit(0);
}

fn run(app: AndroidApp, add_packages: &PackageHook) -> Result<()> {
    let _ = APP.set(app.clone());
    let data = app
        .internal_data_path()
        .context("the activity has no internal data path")?;
    let game_root = data.join("game");
    unpack_assets(&app, &game_root)?;

    // The runtime reads `project.jengine` and `assets/` relative to where it
    // runs, as a desktop build does from its own folder.
    std::env::set_current_dir(&game_root)
        .with_context(|| format!("entering {}", game_root.display()))?;
    // SAFETY: nothing else is running yet - the event loop, the audio thread
    // and every worker start after this - so no thread can be reading the
    // environment while it changes.
    unsafe {
        std::env::set_var("JENGINE_SAVE_DIR", data.join("saves"));
    }

    // A debug build told which graphics backend to use (`--es backend gl`).
    if let Some(backend) = launch_extra(&app, "backend") {
        log::info!("graphics backend {backend}, as the launch asked");
        // SAFETY: as above - nothing else runs yet.
        unsafe {
            std::env::set_var("WGPU_BACKEND", backend);
        }
    }

    let settings = crate::load_project_settings();
    let entry = match launch_extra(&app, "scene") {
        Some(scene) => {
            log::info!("starting at {scene}, as the launch asked");
            scene
        }
        None => settings
            .entry_scene
            .clone()
            .context("project.jengine names no entry_scene or default_scene")?,
    };
    let scene_path = Path::new("assets").join(entry);

    // Controllers are claimed before winit turns their sticks into touches.
    crate::gamepad_android::install();
    let event_loop = EventLoop::builder().with_android_app(app).build()?;
    crate::run_event_loop(event_loop, settings, scene_path, add_packages)
}

/// Unpack the APK's assets into `root`, unless this build already did.
///
/// The stamp is written last, so a launch killed half way through unpacks
/// again next time rather than running from a partial tree.
fn unpack_assets(app: &AndroidApp, root: &Path) -> Result<()> {
    let manifest = read_asset(app, MANIFEST)
        .with_context(|| format!("the APK has no {MANIFEST} - was it built by jengine?"))?;
    let manifest = String::from_utf8(manifest).context("asset manifest is not UTF-8")?;
    let mut lines = manifest.lines();
    let build = lines.next().unwrap_or_default().to_string();

    let stamp = root.join(".jengine-build");
    if std::fs::read_to_string(&stamp).is_ok_and(|s| s == build) {
        log::info!("assets already unpacked ({build})");
        return Ok(());
    }

    let started = std::time::Instant::now();
    if root.exists() {
        std::fs::remove_dir_all(root)
            .with_context(|| format!("clearing the old unpacked build at {}", root.display()))?;
    }
    let mut count = 0usize;
    for rel in lines.filter(|l| !l.is_empty()) {
        let bytes = read_asset(app, rel)?;
        let dest = root.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, bytes).with_context(|| format!("writing {}", dest.display()))?;
        count += 1;
    }
    std::fs::write(&stamp, &build)?;
    log::info!(
        "unpacked {count} asset file(s) for {build} in {:.0} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

fn read_asset(app: &AndroidApp, path: &str) -> Result<Vec<u8>> {
    let name = CString::new(path).with_context(|| format!("asset path {path:?}"))?;
    let mut asset = app
        .asset_manager()
        .open(&name)
        .with_context(|| format!("asset {path} is not in the APK"))?;
    let mut bytes = Vec::new();
    asset
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading asset {path}"))?;
    Ok(bytes)
}
