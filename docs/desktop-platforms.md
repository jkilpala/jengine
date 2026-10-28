# Desktop platforms - Linux and macOS

Windows is the platform this repo is developed on. Since Oct 2026 the builder
also makes **Linux** and **macOS** desktop builds (`jengine_builder::desktop`):

```powershell
jengine linux-build projects/<name>   # a folder: the game binary, project.jengine, assets/
jengine macos-build projects/<name>   # <Name>.app - on a Mac only
```

Both take `--out <dir>`, `--debug` and `--no-report`, default to
`target/<platform>-build/<project>/`, and are in the editor's Build Settings
(Linux and macOS tabs, the `build_desktop` command).

## What a build is

Both build the project's generated runtime crate (`.jengine/runtime/`, the
same one a Windows build makes) with that platform's graphics backend pins
(`settings.graphics_backends.linux` / `.macos`; Vulkan and Metal by default),
then stage the game beside it with the Windows build's own `stage_game`.

- **Linux**: a folder - the binary named after the game, `project.jengine`,
  `assets/`, and `icon.png` when `settings.app.icon` names one.
- **macOS**: `<Name>.app` - `Contents/MacOS/<binary>`, the staged game in
  `Contents/Resources/`, an `Info.plist` from `settings.app` (name, bundle
  id with underscores made hyphens as on iOS, `ios::bundle_id`; version,
  build; `LSMinimumSystemVersion` 11.0) and `AppIcon.icns`
  made from the icon (`icns_from_image`, PNG entries `ic07`-`ic10`) -
  then signed ad hoc (`codesign --force --deep --sign -`), since Apple
  Silicon runs no unsigned code and the bundle is assembled after the linker
  signed the binary.

**The runtime finds its game beside itself** (`jengine_runtime::game_root`):
the first of the working directory, the executable's directory, and the
executable's `../Resources` (a bundle) that holds a `project.jengine`
becomes the working directory. A Linux binary double-clicked from a file
manager, or an app started by Finder, has a working directory that is
neither; before this only Windows' `Build & Run` (which sets it) worked.

**A game in a `.app` saves outside it** (`jengine_runtime::bundle_save_dir`):
saves default to `./saves`, which inside a bundle is `Contents/Resources` -
writing there breaks the bundle's signature (and on iOS the bundle is
read-only). So when the game's folder is inside a `.app` and
`JENGINE_SAVE_DIR` is unset, it becomes `~/Library/Application
Support/<bundle id>/saves`, the id read from the bundle's `Info.plist`. A
Linux or Windows folder keeps saving beside itself.

## Linux from Windows: WSL

On Windows, `linux-build` runs cargo **inside WSL** (`wsl.exe -e bash -c
...`, streamed into the editor's Build panel). The generated crate's path
dependencies are relative, so it builds from `/mnt/c/...` unchanged; the
target directory is `~/.jengine/target/<project>-linux` inside WSL (override
with `JENGINE_WSL_TARGET`), because building on the Windows drive from Linux
is several times slower. The distribution needs Rust and:

```bash
sudo apt install build-essential pkg-config libasound2-dev libudev-dev \
  libxkbcommon-x11-0 libwayland-client0 libvulkan1 mesa-vulkan-drivers
```

(`desktop::LINUX_PACKAGES`; a failed build prints it.) `~/.jengine/wsl-env.sh`
is sourced first if it exists - for a toolchain set up **without root**,
which is how this repo's machine did it: `apt-get download` + `dpkg -x` into
a prefix, then `PKG_CONFIG_SYSROOT_DIR`, `LIBRARY_PATH`, `RUSTFLAGS -L` and
`LD_LIBRARY_PATH` pointed at it. Two traps there: a `.so` dev link in the
prefix is relative and dangles (`libudev.so -> libudev.so.1`, which is in
`/lib`, not the prefix - `unable to find library -ludev`); and ALSA needs
`ALSA_CONFIG_PATH` at the prefix's `alsa.conf`, the prefix's `pulseaudio/`
directory on `LD_LIBRARY_PATH` (for `libpulsecommon`), and a `~/.asoundrc`
routing `default` to the pulse plugin - WSLg's sound is a PulseAudio server.
Without them the runtime logs "audio backend init failed" and runs silent
(`NullAudioBackend`), which is correct behaviour on a machine with no sound.

**Run it under WSLg** to see it: it opens as a Wayland window on the Windows
desktop. WSL has no GPU Vulkan driver, so `VK_ICD_FILENAMES` points at Mesa's
lavapipe (CPU) - slow, but the real Vulkan path.

### What WSL found

- **wgpu's GL fallback panics under WSLg** (`wgpu-hal gles/egl.rs`, an
  `unwrap` on `None` while making the instance), before any adapter is asked
  for. `create_instance` now makes a **primary** instance (Vulkan / Metal /
  DX12) and only when that has no adapter makes the secondary (GL / DX11)
  one, under `catch_unwind`. `WGPU_BACKEND` still picks one outright.
- The runtime, unchanged otherwise, renders scenes correctly on Vulkan
  (test-lab hub, scene 101 - text, sprites, HUD values).

## macOS: on a Mac

A macOS build needs Apple's linker and SDK, which exist only on a Mac, so
`build_macos` refuses anywhere else with that reason (and the editor's macOS
tab says so and disables Build). **It has been built and run on a Mac** (Oct
2026, an M1 Pro on macOS 27 with Xcode 27's Command Line Tools): `jengine
macos-build projects/engine-test-lab` linked and signed ad hoc on the first
try, and the binary ran the hub on Metal (Fifo, an `Rgba16Float` scene
target), with kira audio and its scripts, finding its game in
`Contents/Resources`. The showcase ran too - its first slow scene was
GPU-bound on soft sun shadows, not a Mac problem (the shader counts a
pixel's inside once now; see CLAUDE.md on a sun's `softness`). Unchecked by
eye: opening it from Finder, sound, a pad (the rest of B55).

**A macOS build was checked again on 2026-10-24** (M1 Pro, macOS 27):
`engine-test-lab` built in under three minutes from a warm cache. The bundle
passed `codesign --verify --deep --strict`, and with `env -i` and a working
directory of `/` (what Finder gives it) it ran the hub on Metal. `spctl
--assess` **rejects** it, as it does any ad-hoc signature, so a downloaded
copy is quarantined and refused on another Mac. Players need a Developer ID
build, notarized (see "Left"). The binary is arm64 only.

**An app started from Finder has the system's bare PATH**
(`/usr/bin:/bin:/usr/sbin:/sbin`), not the shell's, because rustup and
Homebrew add theirs in shell profiles. Builds call `cargo` and friends by
name, so the editor adds `~/.cargo/bin` (or `$CARGO_HOME/bin`),
`/opt/homebrew/bin` and `/usr/local/bin` to its own PATH when it starts
(`add_tool_dirs_to_path` in `editor/src-tauri/src/lib.rs`). An editor run
from a terminal never showed the problem.

**The editor runs on macOS** too, in WKWebView, started by `./run-editor.sh`.
Two Mac-specific things: WKWebView holds `requestAnimationFrame` near 60 a
second on a 120 Hz panel, so the editor switches WebKit's
`PreferPageRenderingUpdatesNear60FPSEnabled` off at start
(`editor_window::allow_full_frame_rate`, private API, guarded), and the docs
site needs an LTS Node - Node 26 cannot build its `better-sqlite3`.

Before any of that, from Windows:

- The bundle (layout, `Info.plist`, `.icns`) is unit-tested on any host
  (`desktop::tests`).
- **Core and the whole native runtime type-check for `aarch64-apple-darwin`**
  from WSL, with zig as the C compiler for Luau and QuickJS (`cargo check
  --target aarch64-apple-darwin -p jengine-runtime --features
  backend-metal`; cc-rs's own `--target=arm64-apple-macosx` must be filtered
  out of the zig wrapper, which cannot parse it). That covers every `cfg`
  the runtime has for macOS - winit, Metal, AccessKit, gilrs, arboard, kira's
  CoreAudio. Nothing was linked.

What is left of the first Mac check is manual-verification B55.

## What differs by platform

| | Windows | Linux | macOS |
|---|---|---|---|
| Video (`video_player`) | Media Foundation | black texture, logged once | black texture, logged once |
| System language | `GetUserDefaultLocaleName` | `LC_ALL` / `LC_MESSAGES` / `LANG` | the same variables - **unset for an app started by Finder**, so it falls back to English until a script calls `Locale.set` |
| Gamepads | gilrs (XInput / WGI) | gilrs (evdev - the user needs read access to `/dev/input`) | gilrs (IOKit) |
| Screen reader | UI Automation | AT-SPI | NSAccessibility |

## Left

- A Linux **AppImage** or a `.desktop` file; a Developer ID signature and
  notarization (an ad-hoc signed `.app` runs on the Mac that built it;
  another Mac's Gatekeeper refuses it once downloaded - which needs the
  paid Apple Developer Program, so for now a player lets it through with
  *Open Anyway*, as the editor's own download notes say). An Intel build is
  not planned: Apple Silicon is the Mac that current Xcode supports.

**The editor itself ships for macOS** since 0.1.1: `./build-editor-release.sh`
(the `.ps1`'s twin) makes the `.app`, a `.dmg` and a zip, ad-hoc signed.
docs/releasing.md has the steps and the smoke test.
- Video on Linux (GStreamer) and macOS (AVFoundation).
- **iOS** is a phone, so it lives with Android: `jengine ios-build`, Phase 2
  of [`mobile-platforms-proposal.md`](mobile-platforms-proposal.md).
