# jEngine

A 2D game engine in Rust: a wgpu renderer, an ECS, Lua scripting, physics,
audio and a game server framework, with an editor to build your game in.

**Documentation: [jengine.hirakame.com](https://jengine.hirakame.com)**

> **0.1.0 is the first public release.** The engine has been used to build a
> lot of games, but it is new to everyone else: expect rough edges, and
> expect the script API and file formats to change before 1.0. See
> [Versioning & stability](https://jengine.hirakame.com/getting-started/versioning/).

## What is in it

- **Rendering** on wgpu: sprites, textured meshes, tilemaps (square, isometric,
  hexagonal), text with distance-field fonts, 2D lighting with soft shadows,
  normal maps and bounced light, particles on the CPU and the GPU, HDR, bloom
  and a stack of post effects, shader graphs.
- **Scripting** in Lua - the same scripts run natively on Luau and in the
  editor and the browser on a Lua 5.3 VM - with behaviour graphs and behaviour
  trees for things that should not need code.
- **Physics** on rapier: bodies, joints, a character controller, ragdolls,
  breakable and sliceable bodies, water and area effects.
- **Audio** with mixer buses, effects, spatial sound and a music director.
- **UI**: entity UI and `.gui` documents, controller navigation, text input,
  screen readers, touch controls, scaling for any screen.
- **Packages**: Rust crates that add components, systems and script commands,
  linked per project.
- **Networking**: `jengine-server`, a framework for game servers (rooms, state
  sync with prediction, accounts, a store, clusters, deployment and a web
  console), and the client package that talks to it.
- **The editor**: scenes, prefabs, tilemaps, animation, shader and behaviour
  graphs, a profiler, builds for every platform from one menu.

## Platforms

| Platform | Status |
|---|---|
| Windows | Runtime and editor - the primary platform |
| Web | WebGPU, with a WebGL2 fallback |
| Android | Builds an APK or App Bundle; tested on the emulator |
| Linux | Builds and runs; tested under WSL only |
| macOS | Type-checked only - never linked or run |
| iOS | Experimental - has never run |

The editor runs on Windows, and on macOS in WKWebView with less testing.

## Getting started

You need [Rust](https://rustup.rs) (the repo's `rust-toolchain.toml` picks the
toolchain), Node 20+, and on Windows the Visual Studio Build Tools and
WebView2. The full list, including the Android setup, is in the
[installation guide](https://jengine.hirakame.com/getting-started/installation/).

```powershell
git clone https://github.com/jkilpala/jengine.git
cd jengine
./setup-dev.ps1     # wasm-pack, tauri-cli and the wasm target, once
./run-editor.ps1    # builds the editor's viewport and starts the editor
```

On macOS or Linux, use the `.sh` twin of each script.

To run a project without the editor, or start a new one:

```sh
cargo run -p jengine-builder --bin jengine -- run projects/engine-test-lab
cargo run -p jengine-builder --bin jengine -- project new ../my-game
```

`cargo install --path crates/jengine-builder --bin jengine` installs the CLI
as `jengine`. Builds compile the engine from source, so the CLI and the editor
need this checkout: they find it through the `JENGINE_HOME` environment
variable, or by looking upwards from where they run.

## What is where

| Path | What |
|---|---|
| `crates/jengine-core` | The engine: renderer, ECS, scenes, scripting, input, audio, physics glue |
| `crates/jengine-runtime` | The native runtime (Windows, Linux, macOS, Android, iOS) |
| `crates/jengine-web` | The web runtime (WASM) |
| `crates/jengine-editor-viewport` | The engine as it runs inside the editor |
| `crates/jengine-builder` | The build pipeline and the `jengine` CLI |
| `crates/jengine-server`, `-relay`, `-netcode`, `-liveops` | Game servers, and what servers and clients share |
| `packages/` | The packages the engine ships: physics, audio, particles, parallax, swarm, net, online, tracker |
| `editor/` | The editor (Tauri 2 + Svelte) |
| `templates/` | Starting points for new projects and packages |
| `projects/engine-test-lab` | One scene per engine feature, the QA project |
| `projects/visual-showcase` | Renderer showcase scenes |
| `website/` | The documentation site |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security issues: [SECURITY.md](SECURITY.md).

## Licence

MIT - see [LICENSE](LICENSE). The libraries and fonts jEngine is built on are
listed with their licences in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md);
the editor shows them under Help.
