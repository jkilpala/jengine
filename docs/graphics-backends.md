# Graphics backends

wgpu compiles a **separate code path per graphics API** and picks one at
runtime. A build should only carry the APIs its target can actually use:
DX12 and Vulkan are dead weight in an iOS build, Metal is dead weight on
Windows, and a WebGPU-only web build has no use for any of them.

Since Sep 2026 backends are **opt-in per consumer** rather than a single
list in the workspace manifest.

## The features

`jengine-core` exposes one feature per backend. Each forwards to wgpu, and
the native ones also pull in `wgpu-core` — wgpu's own backend features gate
on `wgc?/…`, so enabling `dx12` alone compiles nothing:

| feature | wgpu | targets |
|---|---|---|
| `backend-dx12` | `wgc` + `dx12` | Windows |
| `backend-vulkan` | `wgc` + `vulkan-portability` | Windows, Linux, Android |
| `backend-metal` | `wgc` + `metal` | macOS, iOS |
| `backend-webgl` | `webgl` (pulls `wgc` itself) | browsers without WebGPU |
| `backend-webgpu` | `webgpu` | browsers with WebGPU |

`vulkan-portability` is the odd name: wgpu has no plain `vulkan` feature.
Vulkan is on by default only for unix-not-Apple targets through wgpu-core's
own target table, and this is the switch that turns it on anywhere else.

**Any crate that enables `jengine-core/gpu-wgpu` must also name a backend**,
or wgpu compiles with none and fails at adapter request — at runtime, not at
build time. The consumers today:

| crate | backends |
|---|---|
| `jengine-web` | `backend-webgpu` (default), optionally `backend-webgl` |
| `jengine-editor-viewport` | `backend-webgpu` |
| `jengine-runtime` | `backend-dx12` + `backend-vulkan` + `backend-metal` (default) |
| `jengine-benchmark`, `jengine-benchmark-web` | as above per target |

The native runtime's default covers every desktop API from one binary —
wgpu `cfg`s out what cannot apply, so Metal costs a Windows build nothing.

## Per-project selection

`project.jengine` can pin backends per platform:

```json
"settings": {
  "graphics_backends": {
    "web": ["webgpu"],
    "windows": ["dx12"],
    "macos": ["metal"],
    "ios": ["metal"],
    "android": ["vulkan"]
  }
}
```

Editable under **Project Settings → Graphics**. The builder maps these onto
the cargo features above (`jengine_builder::graphics`). Omitting a platform,
or the whole block, uses the crate defaults. Order matters: wgpu tries
backends in the order they are compiled in.

**Android builds** (Sep 2026, `jengine_builder::android`) accept only
`vulkan`, and on Android any native backend also compiles wgpu's **GLES**
path - wgpu enables it by target once `wgpu-core` is in - so a phone without
usable Vulkan still gets an adapter (`create_instance` asks for
`PRIMARY | SECONDARY`). macOS and iOS have no build target yet — the settings
exist so a project can express the intent before those paths land.

## Which GPU

A backend is an API; on a machine with two GPUs - a laptop's integrated
and discrete ones - each API can reach either. `settings.gpu_preference`
(Oct 2026, Project Settings -> Graphics -> GPU) says which a built game asks
for:

```json
"settings": { "gpu_preference": "low_power" }
```

`high_performance`, the default and the only behaviour before, asks for the
discrete GPU; `low_power` for the integrated one, which saves battery and is
plenty for a light game. The native runtime gets what it asks for; a web
build passes the request to the browser, which may ignore it
(`renderer::gpu_preference`).

**Chrome and Edge do ignore it** on a hybrid laptop: WebGPU is pinned to the
compositor's adapter, the GPU of the primary display. Measured on this repo's
laptop (Oct 2026), the showcase hub in plain Chrome got the 780M (`amd`,
`rdna-3`) at 15 ms a frame, p95 36; the same page with Chromium's
`--force_high_performance_gpu` got the 4070 (`nvidia`, `lovelace`) at a flat
6.06 ms, the display's 165 Hz. So **judge a web build's speed only after
checking its adapter** - the console's `wgpu adapter:` line, or in DevTools
`(await navigator.gpu.requestAdapter()).info`. A player fixes it outside the
page: Windows Settings -> Display -> Graphics -> the browser -> High
performance.

**The editor's own GPU is not a project setting.** It is a WebView2 page, and
WebView2 picks the GPU of the primary display whatever wgpu requests, so the
editor passes Chromium `--force_high_performance_gpu` - by default, or not,
per the editor preference beside the project setting ("Let Windows choose").
It is saved in the editor's app data directory and applies from the next
start, since browser arguments are fixed when WebView2 starts. See
`editor/src-tauri/src/commands/editor_prefs.rs`.

## When to add a backend

- **WebGL2** — the fallback for browsers without WebGPU (Firefox stable,
  Safari before 18). Costs wasm size; only worth it if those browsers are in
  scope.
- **Vulkan on Windows** — an alternative for drivers where DX12 misbehaves.

## Size

Measured on a full game's web build (Sep 2026):

| | raw | gzipped |
|---|---|---|
| before | 4.46 MB | 1.56 MB |
| after | **2.54 MB** | **0.86 MB** |

Note what did *not* contribute: trimming backend features changed the wasm by
**zero bytes**, because wgpu already `cfg`s out backends that cannot apply to
the target. The win came from build flags (LTO + `opt-level="z"` +
`codegen-units=1`, now on by default for release builds) and from subsetting
the embedded font (`crates/jengine-core/assets/fonts/subset_builtin_font.py`,
607 KB → 8 KB).

The backend features are about *correctness of intent* and about native and
mobile binaries — not about shrinking the web build.
