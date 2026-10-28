# Mobile platforms — Android first, then iOS

**Status (2026-09-27):** Phase 1 built on branch `mobile-android`: an APK
of engine-test-lab runs on the Android emulator. Not yet tried on a phone.
Decided: a **native** runtime (winit + wgpu + Luau), not the web build inside a
WebView wrapper, and **Android before iOS**.

## Why native rather than a wrapped web build

A Tauri-mobile or Capacitor shell around the existing web build would ship
sooner, but it runs Lua through Fengari (a Lua VM written in JS, far slower
than Luau), and WebGPU inside mobile WebViews is patchy enough that it would
need the WebGL2 fallback. The native runtime already exists and already runs
every game; the work is porting its shell, not the engine.

Android goes first because it can be built and tested on the Windows dev
machine (emulator + a phone over `adb`). iOS needs a Mac or a macOS CI runner
for Xcode and signing.

## What already holds

- **The renderer asks for nothing a phone lacks** -
  `wgpu::Features::empty()` and `Limits::downlevel_defaults()`
  (`renderer/backend/wgpu/renderer.rs`), the same bar WebGL2 meets. Vulkan
  and Metal are already backend features, and Project Settings already
  stores `ios` / `android` backend rows (`docs/graphics-backends.md`).
- **No dependency is a known blocker.** rapier is pure Rust; audio is kira on
  cpal 0.18, which uses AAudio on Android (no oboe / C++) and CoreAudio on
  iOS; Luau is vendored C++ built by `cc`, and the JIT is not enabled - iOS
  forbids JIT; `jengine-net` is rustls + ring, which cross-compile.
- **UI scaling** (`settings.ui_scale_mode`) already makes one UI fit any
  resolution.
- **The web runtime already maps the first touch onto the left mouse button**,
  so there is a behaviour to copy rather than invent.
- **Saves already take a base directory** (`SaveSystem::set_base_dir`).

## What breaks

1. **Lifecycle.** `App::resumed` builds all state once and there is no
   `suspended` handler (`jengine-runtime/src/lib.rs`), and `WgpuRenderer`
   owns its `Surface` for life. Android destroys the native window every
   time the app is backgrounded, so the surface has to be dropped on
   `suspended` and rebuilt on `resumed`, keeping the device, queue, textures
   and pipelines. This is the core refactor.
2. **Entry point.** `run_with` assumes `main()`, `clap` arguments,
   `project.jengine` in the working directory and `env_logger` on stderr.
   Android enters through `android_main(AndroidApp)` in a `cdylib`, and stderr
   goes nowhere there.
3. **Assets are read with `std::fs`** in ~100 places across ~17 files.
   Assets inside an APK are not files. (iOS bundle resources *are* files; its
   problem is that `./saves` inside the bundle is read-only.)
4. **Native has no touch input at all** - `handle_window_event` has no
   `WindowEvent::Touch` arm - and no runtime has a multi-touch script API.
5. **Most arcade games are keyboard games**, so touch-as-mouse only covers
   the click-driven ones; the rest need on-screen controls.
6. **Desktop-only dependencies in the native build:** `basis-universal` (a
   C++ *encoder* pulled in by core's `native` feature, used only by the
   builder), `rquickjs` via `js-adapter-host` (only JS scripts use it:
   bunnymark and test-lab scene 58), `notify` (hot reload), `gilrs`
   (gamepads) and `clap`.

## Phases

### Phase 0 — play the web build on a phone

No engine change. Tells which games already work by touch and whether UI
scaling holds on a phone screen. WebGPU needs a secure context, so either
`adb reverse` to reach `localhost` or enable `backend-webgl`.

### Phase 1 — Android first light (native)

**Built (2026-09-27), and seen running on the emulator** (Android 16,
x86_64 with 16 KB pages): engine-test-lab's hub draws full screen at
2400 x 1080 on Vulkan, taps choose scenes, the back button returns to the
hub, and Home then relaunch comes back drawing and answering taps. **A
click-driven management game plays by touch** on the emulator too (buying, a
price slider, opening the day, customers served). **Not yet seen on a phone** - that is what remains
of the done line below.

Done when the test-lab hub and a click-driven game run on the
emulator and on a phone, and survive being backgrounded and resumed.

#### Building one

```powershell
cargo run -p jengine-builder --bin jengine -- android-build projects/<name> --run
#   --abi x86_64     for the emulator (replaces settings.android.abis)
#   --install        adb-install onto the one connected device; --run also starts it
#   --dev            unoptimised Rust
adb logcat -s jengine    # the game's log
```

Or **Build → Android** in the editor. The output is
`target/android-build/<project>/<profile>/<app id>-<version>.apk`, signed with
the debug key. Needs the SDK (Android Studio's default location, or
`ANDROID_HOME`), an NDK r25+ (`ANDROID_NDK_HOME`, or the newest under
`<sdk>/ndk`), the platform named by `target_sdk`, `cargo install cargo-ndk`,
and a JDK for `apksigner`.

#### What an APK is made of

No Gradle - NativeActivity has no Java code, so the SDK's build-tools make the
APK directly (`jengine_builder::android`):

1. `generate_project_android` writes `<project>/.jengine/android/`, a `cdylib`
   named `main` whose `android_main` registers the project's packages and calls
   `jengine_runtime::android::run_with`. Always generated: there is no stock
   artifact to fall back to.
2. `cargo ndk` builds `libmain.so` per ABI, with `-z max-page-size=16384`.
3. `stage::stage_game` lays the game out exactly as a Windows build does
   (shared code now), plus `jengine-files.txt` - the list the runtime unpacks
   from, since the NDK's asset manager cannot list subdirectories.
4. `aapt2` compiles the icons and links the manifest from `settings.app`.
5. The `zip` crate adds `lib/` and `assets/` - **not** `aapt2 -A`, which
   silently skips directories starting with `_`, and the asset pipeline
   writes `sprites/_atlases/`.
6. `zipalign -P 16 -f 4`, then `apksigner` with `~/.android/debug.keystore`
   (made with `keytool` if missing).

At launch `jengine_runtime::android` unpacks the assets into internal
storage once per build (327 files in 120 ms), `chdir`s there, points
`JENGINE_SAVE_DIR` beside it and runs the ordinary event loop.

#### App settings

`settings.app` (shared with iOS later) and `settings.android` in
`project.jengine`, edited under **Project Settings → Mobile App**:
id, display name, version, build number, icon, icon background, orientation,
min / target SDK and ABIs. Every field is optional; `jengine_builder::app`
holds the defaults and validation, and `editor/src/lib/mobileApp.ts` mirrors
them for the form. The icon becomes legacy mipmaps at five densities plus an
adaptive icon (the picture in the middle 72 dp of a 108 dp foreground, over
`icon_background`). Verified on the emulator: name, version, build number and
icon all come through.

#### What running it found

Each of these compiled and linked cleanly, and failed only on the device:

- **`libc++_shared.so`.** Luau is C++, the `cc` crate links the shared C++
  runtime on Android, and cargo-ndk does not copy it - an APK without it
  installs and then cannot load. The builder bundles it when `libmain.so`
  names it.
- **SIGBUS in the dynamic linker** with the libraries compressed and
  extracted at install: zero-filling the tail of the writable segment ran
  past the end of the unpacked file on a 16 KB-page device. Libraries are
  now stored uncompressed, 16 KB-aligned and loaded from the APK in place
  (`extractNativeLibs="false"`), which is Google's recommended layout anyway.
- **A 2048 px surface cap.** `Limits::downlevel_defaults()` limits textures -
  and so the swapchain - to 2048, and the phone is 2400 wide. That was a
  desktop bug too: a window wider than 2048 (fullscreen on a 1440p monitor)
  panicked in `Surface::configure`. Size limits now come from the adapter
  (`using_resolution`), natively and on the web.
- **winit 0.30 drops Android's `onDestroy`** (a `TODO` in its source) and
  refuses a second event loop per process. So the process exits when the
  game does - otherwise the next launch reuses the cached process and cannot
  start - and a second `android_main` while the first runs also ends the
  process, rather than leaving `onDestroy` waiting forever. **Known limit:**
  an activity destroyed while its process lives (the "Don't keep activities"
  developer option) still leaves the old event loop parked until the next
  launch ends the process. Tapping Home and coming back does not destroy it.

#### The runtime side

- `WgpuRenderer::surface` is an `Option`: `suspend()` drops it, `resume()`
  rebuilds it at the window's current size and checks the colour format the
  pipelines were built for.
- `App::suspended` / a second `resumed` call `wgpu_scene_runner::suspend` /
  `resume`: surface, `AudioBackend::suspend` / `unsuspend` (resumes only what
  it paused, not what the game paused), held input released, and a frame
  clock that does not count the time away. No frame runs while suspended.
- `WindowEvent::Touch`: the first finger is the left mouse button, as on the
  web runtime. Android's back button (logical `BrowserBack`, consumed by
  winit) is `Escape`.
- gilrs has no Android backend; controllers arrive another way (see
  "Game controllers" under Phase 4).

#### Step 1 - it compiles (2026-09-27)

`cargo ndk -t arm64-v8a -P 26 check -p jengine-runtime` builds clean. Two
changes were needed:

- `rquickjs-sys` has no prebuilt bindings for Android or iOS. Core's `native`
  feature no longer pulls it in (nor `basis-universal`, an encoder only the
  builder uses and which it depends on itself); the batch-script family
  (`batch`, `batch_lua`, `worker_pool`) moved behind `js-adapter-host`, which
  is the only feature that ever dispatched it. The runtime enables
  `js-adapter-host` for every target except Android and iOS, and on those
  logs once if a scene has batch scripts. **Mobile runs Lua scripts only.**
- winit needs an activity type on Android: `android-native-activity` for now
  (no Java, no AndroidX); GameActivity is the Phase 3 candidate for the soft
  keyboard.

#### Left in Phase 1

- Run it on a phone (arm64).
- ~~The navigation bar still shows over the game~~ - **done (2026-09-27)**:
  `settings.android.immersive` (default on) ships a ten-line NativeActivity
  subclass (`crates/jengine-builder/assets/android/JEngineActivity.java`,
  compiled by `javac` + `d8`) that hides the system bars whenever the window
  has focus and draws under a notch. It has to be Java: only the UI thread may
  touch the window, and `android_main` is not it. Seen on the emulator: the
  bars go, and stay gone across background and resume.
- `libmain.so` is 24 MB uncompressed for the test lab - worth an `opt-level`
  / LTO look before a store build.

### Phase 2 — iOS first light

Reuses Phase 1's entry split, lifecycle and touch. Needs a Mac or a macOS
runner for everything past `cargo check`.

#### Step 1 - it compiles, and the bundle exists (2026-10-12)

**The whole runtime type-checks for `aarch64-apple-ios`** with no change -
Phase 1 had already made the features mobile-aware (`js-adapter-host` off,
winit's iOS backend, kira on CoreAudio, gilrs' dummy backend). Checked from
WSL: cc-rs asks `xcrun` for the iOS SDK, so the check sets `SDKROOT` to a
stand-in and has zig compile the vendored C with its macOS headers - an
object nothing links; the Rust is checked for real.

`jengine ios-build` (`jengine_builder::ios`, and an iOS tab in the editor's
Build Settings) builds **without an Xcode project**, as Android builds
without Gradle: cargo for `aarch64-apple-ios-sim` (the simulator, the
default) or `aarch64-apple-ios` (`--target device`), Metal only unless the
project pins `graphics_backends.ios`, then a flat `<Name>.app` - the
executable, `Info.plist` (id, name, version, build, orientations,
`UILaunchScreen` so it is full screen, iPhone and iPad), `PkgInfo`, icons
named by `CFBundleIconFiles` and flattened onto `icon_background` (iOS
draws an icon's transparency black), and the staged game at the bundle's
root. The simulator's copy is signed ad hoc; a device's with
`--identity` and `--profile` (the profile's entitlements extracted with
`security cms` and `PlistBuddy`, and embedded). `--install` / `--run` go
through `xcrun simctl` (booting the first iPhone if none is) or `xcrun
devicectl` (Xcode 15+, the first connected device or `--udid`).

What the runtime needed: nothing for finding its files (`game_root` looks
beside the executable) or saving (`bundle_save_dir` puts saves in the app's
`Library/Application Support`, since the bundle is read-only), and the
**notch**: winit's iOS window reports the safe area as its inner frame
inside the outer one, so `refresh_safe_insets` takes the difference
(`insets_between`).

**It runs on the simulator** (Oct 2026, iOS 27 on an iPhone 18 Pro
simulator, Xcode 27): the test lab's hub draws full screen in landscape on
Metal, finds its game in the bundle and its saves in Application Support,
and survives a trip to the background (`suspended`, then `resumed` and
drawing). Three things were needed, none of which a Windows check could see:

- **iOS 27 refuses an app without the UIScene lifecycle** - UIKit traps
  (SIGTRAP in `_UIApplicationEvaluateRuntimeIssueForNoSceneLifecycleAdoption`)
  as the first scene is created, before any engine code draws. The plist
  declares a `UIApplicationSceneManifest` now (no delegate class).
- That makes the app scene-based, where a window in no `UIWindowScene` is
  never shown - so the screen stays on the black launch screen while the
  engine runs happily behind it. winit 0.30 knows nothing of scenes; the
  vendored winit attaches its window (`vendor/winit/JENGINE-PATCH.md`).
- The generated runtime crate did not `[patch]` winit to the vendored copy
  (only the Android one did), so the fix above was silently not in the build.

Then, played with (through Xcode 27's DeviceHub):

- **The frame loop ran one frame and stopped.** winit delivers
  `RedrawRequested` from inside `drawRect:`, and UIKit drops a
  `setNeedsDisplay` made there - which is where every game asks for its next
  frame. Taps and keys arrived and nothing ran to read them. Frames now come
  from a display link in the vendored winit (below).
- **No hardware keyboard.** winit 0.30 on iOS reports only soft-keyboard
  text. The vendored winit now takes UIKit's `pressesBegan` / `pressesEnded`
  on its window and maps the HID usage to a key (`ios/keyboard.rs`), so an
  iPad keyboard - or a Mac's, into the simulator - works. Fn+arrow is Page
  Up / Down there.
- Two **native** bugs every platform had, found by pressing keys at a scene
  that runs its script below 60 Hz: such a script saw about one press in six
  (`docs/parity-gaps.md`), and a frame that ended early - a scene load, a
  lost surface - kept its key edges, so one Page Down ran Test Lab from
  scene 02 to 226. `render_frame` clears them after any exit now.

Also: `IPHONEOS_DEPLOYMENT_TARGET` follows `MinimumOSVersion` (the binary
said 14.0, the plist 15.0), and `--run` opens `DeviceHub.app` when there is no
`Simulator.app` (Xcode 27 has none).

**It runs on an iPhone** (2026-10-24, an iPhone 15 Pro Max on iOS 27,
signed with a free Apple ID's development identity and the profile Xcode made
for it): it installs, starts, draws the hub on the A17 Pro's GPU, sees five
fingers at once (scene 108), keeps its UI clear of the Dynamic Island (177 px
a side in landscape, 60 at the bottom) and comes back from the background.
What the phone found:

- **An iOS bundle id cannot hold an underscore**, and the default
  (`com.jengine.engine_test_lab`, from the project's folder) did - valid for
  Android, whose package names refuse hyphens instead. `ios::bundle_id`
  turns any character iOS refuses into a hyphen, for the plist and the
  install alike; Android keeps its spelling.
- **`devicectl` lists booted simulators too** (Xcode 27, `simulated` in its
  last column), and a phone's identifier is now its 25-character UDID - so
  `--run` installed a phone build onto the simulator (EBADARCH).
  `first_device` skips simulated rows and takes any hex-and-hyphen id.
- **60 Hz on a 120 Hz screen.** The plist key
  `CADisableMinimumFrameDurationOnPhone` lifts the system's cap, and it was
  not enough: frames came from UIKit's own display pass, which stays at 60.
  The vendored winit drives them from a `CADisplayLink` asking for the
  screen's maximum (`frame_rate.rs`; its first version registered the link
  in no run-loop mode and it never fired). Median frame 16.7 ms became 8.3.
- **A phone has no Esc.** Both test projects' `nav.lua` take three fingers
  swiped right as "back to the hub" (seen working on the phone). The soft
  keyboard comes up for a focused field (scene 205) and types into it.
- **Scene 16 had only keys.** Its audio checks were Space, M, P and so on,
  so there was no way to play a sound on a phone; it carries
  `touch_controls` buttons pressing the same keys now.
- **No icon** - neither project sets `settings.app.icon`, so iOS shows its
  placeholder. With one set, the build now compiles an asset catalog with
  `actool` (`Assets.car`, `CFBundleIconName`), as Xcode and a store upload
  expect, keeping the plain PNGs for an older iOS.

#### Left in Phase 2

- Still unseen on a phone: rotation, and a save surviving a restart
  (scene 38). Sound plays (scene 16, through its on-screen buttons).
- An `.ipa` for TestFlight, and a distribution identity.
- Video (`video_player`) shows a black texture, as on Linux and macOS.

### Phase 3 — touch as a first-class input

**On-screen controls: built (2026-09-27).** A `touch_controls` component
(`ecs::components::TouchControls`) lays out sticks and buttons, anchored to
screen corners in UI pixels; each **presses keys** - a key by name, or an input
action's first keyboard key - so a keyboard game (actions or raw keys,
scripts or packages) plays by touch unchanged. The logic is shared
(`input::touch_controls`): hosts queue fingers, `tick` before anything reads
input, and draw `draw_list` with `render_touch_controls_wgpu`. The finger that
is not on a control is the left mouse button, now decided there too, for
native and web alike. Shown once a touch is seen (`show: "touch"`, the
default) or always (`"always"` - the mouse works it on a desktop and in the
editor). Test-lab scene 105; `touch_controls_reach_every_runtime` is the
guard. Seen on the emulator: the stick presses a diagonal as two actions and
the knob follows the finger; a button bound to an action presses its key.
**Not yet seen:** two fingers at once (`adb input` has one pointer), the web
runtime on a phone, and the editor with the mouse.

**Arcade layouts: built (2026-09-27).** Fifty-four arcade scenes carry a
layout written from each game's own control hints: a stick, up to four
labelled buttons, and a small II (Escape) / OK (Enter) pair for the menus.
The scenes are generated, so the layouts live in the project's own
`add_touch_controls.py` tool - re-run it after regenerating a scene; it
refuses a scene it has no decision for.
`every_shipped_binding_presses_a_key` fails a binding that would press
nothing. Seen on the emulator: a dungeon crawler played by stick and OK, a
platformer run and jumped. On a 20:9 phone the side margin is narrower than a
thumb-sized stick, so the stick overlaps a game's edge UI (the crawler's
command panel) - usable; its size is a feel decision.

That play-through also found a loader bug: the native runner inlined
`.tilemap` / `.anim` / `.particle` files only for the first scene, and the
web runtime never inlined tilemaps or particles, so every arcade game
launched from the hub lost its external tilemap (the platformer had no ground).
`scene::sources` is the one place now, and builds inline those files into
every scene they ship.

Still to do:

- A multi-touch script API for games that want fingers rather than keys,
  wired through all four runtimes.
- The soft keyboard for `TextInput` (likely the move to GameActivity).

### Phase 4 — platform polish and store builds

**Wide screens: fixed (2026-09-27).** Phones are about 20:9, wider than the
16:9 the games are authored at, and the emulator showed two problems. The UI
kept its authored left edge, so a UI placed in absolute positions (one game's
99 `TopLeft` elements) sat left of centre: `ui_scale_mode: "ScaleAndCenter"`
now lays it out at the authored size and centres it, and the arcade opts in;
edge-anchored UIs keep `ScaleWithScreen`. And a camera with a fixed viewport
stretched the world to the screen's shape: `Camera2D::fit_view` now shows the
authored area plus more world along the longer axis, in every runtime and in
every screen-to-world conversion (the editor's Game view already did this for
drawing, and not for mouse picking). See "UI scaling" in `CLAUDE.md`.

**Immersive: built (2026-09-27)** - see Phase 1's list.

**Store builds: built (2026-09-27).** `--release-key` signs with the
project's own key (`settings.android.keystore` / `key_alias`, Project
Settings -> Mobile App) and `--bundle` also writes the `.aab` Google Play
takes (Build -> Android has both, with a password box that is never saved):

```powershell
# once: a release key - keep it out of git and backed up; an app can never
# be updated with a different one (Play App Signing holds the real one, and
# this becomes the upload key)
keytool -genkeypair -keystore release.jks -alias upload -keyalg RSA -keysize 2048 -validity 10000

$env:JENGINE_KEYSTORE_PASSWORD = "..."   # and JENGINE_KEY_PASSWORD if the key's differs
cargo run -p jengine-builder --bin jengine -- android-build projects/<name> --release-key --bundle
```

A release build is refused, before anything compiles, without its own app id
(`com.jengine.*` is made up from the project name), a keystore, an alias or
the password. Passwords reach `apksigner` / `jarsigner` only through their
own environment. The bundle is the same parts linked by
`aapt2 --proto-format` into `bundletool`'s module layout; `bundletool` is not
in the SDK - put `bundletool-all-<version>.jar` from its GitHub releases in
`<sdk>/bundletool/` or point `BUNDLETOOL_JAR` at it. **Verified on the
emulator:** the `.aab` passes `bundletool validate`, `bundletool build-apks`
turns it into the base + `split_config.x86_64` APKs Play would serve, and
those install and run; the release-signed APK and bundle carry the release
certificate (`apksigner verify`, `jarsigner -verify`) and the APK installs and
runs. **Not verified:** an actual upload to Play Console - the first one also
enrols the app in Play App Signing, which only the account holder can do.

**Frame-rate cap: built (2026-09-27).** `settings.frame_rate_cap` per
platform (`desktop`, `web`, `android`; Project Settings -> Display): Android
defaults to 60 for battery and heat, the others to the display's rate.
`time::FrameCap` lets a frame start once 90% of its interval has passed, so on
a vsynced display frames land on evenly spaced refreshes (120 Hz at 60 = every
second one). The native runner sleeps in `about_to_wait` rather than spinning;
the web runtime skips `requestAnimationFrame` callbacks. Measured on the
emulator with a cap of 30 on its 60 Hz display: 32.6 fps, median frame gap
33.1 ms - the slack lets an occasional frame land a refresh early where the
display's own timing is loose. It replaced `settings.target_fps`, which the
editor offered and no runtime read.

**Safe area: built (2026-09-27).** `UiScale::safe` holds the insets a
platform reports - Android's display cut-out read over JNI
(`getRootWindowInsets().getDisplayCutout()`, again on resume and rotation),
the browser's `env(safe-area-inset-*)` through a probe element, the editor's
device preview - and both UI systems and the touch controls are laid out
inside what is left, while the world still fills the screen under the notch.
The immersive activity asks for `LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES`,
which is what makes the insets the UI's business rather than Android's
letterbox. For planning, the Game view and the GUI editor have a device menu:
five profiles (20:9 punch-hole, 19.5:9 notch, 19.5:9 island, plain 16:9, 4:3
tablet; portrait or landscape) that letterbox the view, draw the cut-out and
rounded corners with a dashed safe-area outline, and hand the engine the same
insets, so the UI moves in the preview exactly as on the device. Seen on the
emulator with Android's simulated tall cut-out: a 132 px left inset read,
scene 105's stick moved clear of it and answered there. Not yet seen: the
editor preview (the viewport and editor build, the maths is unit-tested), the
web probe on a phone. Scene 106.

The web runtime's canvas keeps the project's resolution and is stretched to
the page, so on a phone's browser it distorts - the camera fix cannot help a
buffer that is 16:9. Sizing the canvas buffer to the page is the web's own
wide-screen fix, still to do.

Still to do: render scale (thermals),
`on_app_pause` / resume events so a game can save before the OS kills it, an
iOS archive.

**Checked before a real phone (2026-09-27).** Two automatic checks exist now,
because the Android code is compiled by nothing on a desktop:
`cargo ndk -t arm64-v8a check -p jengine-runtime`, and `jengine
android-smoke <project>`, which starts every scene of a project in its own
process on the connected device (a debug-key build honours a `scene` launch
extra) and fails on a process that died, an engine error or panic in logcat,
or a flat screenshot - then backgrounds, resumes and turns the app.
`--backend gl` does the same on the GLES fallback, through a `backend`
extra that sets `WGPU_BACKEND`.

What the first pass over the test lab found:

- **Three bugs, none of them Android's.** Native `Transform.get_*` threw on
  an entity that no longer existed (the editor answered `0, 0`); native
  `UI.create_panel` required the colour the other runtimes default; and both
  script-header parsers read a header out of a comment, so scene 103's own
  controller never ran anywhere. Fixed and recorded in `parity-gaps.md`.
  They surfaced here because this is the first thing that runs every scene
  natively and reads the log.
- **GLES is already in every Android build** - wgpu compiles it for every
  unix target and `create_instance` asks for `PRIMARY | SECONDARY` - so a
  phone without working Vulkan falls back rather than failing. The whole lab
  runs on it on the emulator (114 of 115 on the first pass; the one failure,
  a libc unwind request on scene 90, did not recur in six reruns).
- **Android's "Viewing full screen" notice** covers an immersive app the
  first time it runs and holds focus until tapped. A player sees it once;
  the smoke test marks it seen, since its HOME press otherwise dismissed the
  notice instead of backgrounding the game.
- Startup warnings that were not faults - wgpu's EGL probe and gilrs having
  no Android backend - are filtered or said at info, so a log scanned for
  faults does not start with six of them.

Still not seen: a real phone, two fingers, audio across a real
background/resume (the emulator's audio is not a phone's), heat.

**Game controllers (2026-09-27).** Apple Arcade requires them, and Google
Play's programmes ask about them. On Android nothing read one: gilrs has no
backend there, and winit, which drains the input queue itself, turned a
thumbstick's joystick `MotionEvent` into a **touch** at the stick's own
`-1..1` coordinates - so pushing a stick pressed the top-left corner of the
screen (seen on the emulator in scene 04, before anything was built). Now:

- **A vendored winit** (`vendor/winit`, 0.30.13 plus one hook, documented in
  its `JENGINE-PATCH.md`) lets the runtime see each input event first. The
  root workspace and the generated Android crate both `[patch]` to it.
- **`jengine-runtime::gamepad_android`** claims controller events - `BUTTON_*`
  keys, D-pad keys only from a gamepad source (a keyboard's arrows share
  their codes), and joystick-source motion - and keeps per-device state:
  sticks (`X`/`Y`, and `Z`/`RZ` or `RX`/`RY` for the right one), triggers as
  axes (`LTRIGGER` or `BRAKE`), the D-pad as a hat. It feeds the same pad
  model as the desktop and the web, so `Pad0_A` and every input action work
  unchanged. A controller that is switched off sends nothing, so once a
  second the runtime asks Android (`InputDevice.getDevice`) which still exist.
- **The on-screen controls step aside** while a controller is in use and
  come back on the next touch - in `input::touch_controls`, from
  `InputState::gamepad_in_use`, so every runtime does it.
- **Stick Y is positive up** everywhere now. It had been positive down (the
  browser's convention, which the desktop copied by flipping gilrs) while
  every binding and script in the repo treated up as `+` - so on a real
  controller every game moved upside down, on every runtime. Nobody had
  plugged one in. `gamepad::stick_y_up` is the one conversion;
  `stick_up_is_positive_everywhere` guards it.
- `android-smoke` presses a button and a stick at the app and fails unless
  the engine logs the controller.

Checked on the emulator with `adb shell input gamepad` / `joystick`: scene
44's lights and markers follow the stick (up is up), A reads held, and scene
105's controls hide on a pad press and return on a tap. Not yet: a real
controller over Bluetooth, two at once, rumble.

**Menus from a controller (2026-09-28).** A `.gui` document could be walked
with Tab and nothing pressed the focused button; entity UI had no focus at
all. `ecs::systems::navigate_ui` (every host, once a frame) now walks
`focusable` nodes with the arrows, a D-pad or the stick - spatially, to the
nearest node that way - presses with Enter / Space / A through the same path
a click takes, scrolls a focused row into view, and does the same for
entity UI marked `focusable` (sliders take left / right). Documents whose
script runs its own cursor opt out with `"navigable": false`: all 112 of the
arcade's, whose `lib/menu.lua` already reads `menu_*` actions bound to pad
buttons. Seen on the emulator with injected D-pad / A: test-lab scene 107's
grid, disabled button, scrolling list and entity UI, and the lab's own hub
(D-pad down, A loads a scene). `android-smoke` now does the last one to the
entry scene of any project and warns when nothing happens.

One trap found on the way: `adb shell input keyevent` presses and releases
inside one frame, so navigation read nothing *held*; it reads the press edge
too now, and the Android pad tracker keeps a tapped button down for one
frame - a real, quick tap would have hit the same.

### Later

An `AssetSource` layer in core instead of extract-on-first-launch (it would
also let the web runtime's fetch path share the code), and compressed
textures (ASTC / ETC2) - every texture is RGBA8 today.
