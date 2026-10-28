# jEngine - agent guide

A 2D game engine in Rust (wgpu + hecs + Lua scripting) with a Tauri/Svelte
editor. This file is loaded automatically by Claude Code; it is the short
version of what a person or an agent changing the engine needs to know.
Humans: start with [README.md](README.md) and [CONTRIBUTING.md](CONTRIBUTING.md).

## Where things are

| Path | What |
|---|---|
| `crates/jengine-core` | The engine: wgpu renderer, ECS (`hecs`), scene format, scripting bridge, input, audio, physics glue |
| `crates/jengine-runtime` | Native runtime (winit + wgpu, Luau scripts) |
| `crates/jengine-web` | WASM runtime (WebGPU / WebGL2, Lua through Fengari) |
| `crates/jengine-editor-viewport` | WASM module the editor runs Play mode in |
| `crates/jengine-builder` | Build pipeline and the `jengine` CLI |
| `crates/jengine-server` | Game server framework - read its README and `LIMITS.md` |
| `crates/jengine-relay`, `-netcode`, `-liveops` | Turn-based relay; what servers and clients share |
| `packages/` | Optional engine packages (physics, audio, particles, parallax, swarm, net, online, tracker) |
| `editor/` | Tauri 2 + Svelte + TypeScript editor |
| `projects/engine-test-lab` | One scene per engine feature - the QA project |
| `projects/visual-showcase` | Renderer showcase scenes |
| `website/` | Docs site (Astro + Starlight) - partly generated, see below |

## Documentation

- [`docs/scripting-api.md`](docs/scripting-api.md) - the script API.
  **Keep it updated when you add or change a script-facing function**; the
  website's API reference, the editor's completions and the AI spec are all
  generated from it.
- [`docs/writing-packages.md`](docs/writing-packages.md) - before touching the
  package system.
- [`docs/parity-gaps.md`](docs/parity-gaps.md) - every place the runtimes
  disagree. Fixing one means marking it FIXED there in the same change.
- [`docs/architecture-debt.md`](docs/architecture-debt.md) - the structural
  problems, ranked.
- `website/` pages under `api-reference/`, `features/` and `packages/` are
  **generated** (`npm run sync` in `website/`): fix the source, never the page.
  A package's page is its `README.md`.

## Parity - the #1 source of bugs

There is no single place that registers a script global. Four execution paths
each have their own, and compiling proves nothing about whether a function
exists at runtime:

| Runtime | Lua dialect | Globals registered in |
|---|---|---|
| Native | Luau via `mlua` | `scripting/bridge/adapters/luau.rs` |
| Editor viewport | Fengari (Lua 5.3 in JS) | `editor/src/lib/lua-runtime.ts` -> `BRIDGE_FUNCTIONS` |
| Browser preamble | JS | `scripting/bridge/adapters/js/browser.rs` |
| Web | Fengari, the same bundle as the editor | `jengine-web/src/script_host.rs` |

A function wired in one and forgotten in another is the most common bug here,
and it is silent: `attempt to call a nil value` in one runtime, or a command
that does nothing. Adding a script-facing function means wiring the apply arm
in `ecs/systems.rs`, `BridgeCommand`, `ScriptCommand`, the translator, the
Luau global, the browser preamble, `BRIDGE_FUNCTIONS`, both browser hosts,
`package/core_script_api.rs`, **its namespace in `scripting/namespaces.rs`**
and `docs/scripting-api.md`. Follow `.github/skills/parity-expert/SKILL.md`.
A command that needs only the world can be written once in
`scripting/host_commands.rs` (`SHARED_WORLD_TAGS`).

Scripts call **namespaced** names - `Sprite.set_color(e, ...)` - built from
the one list in `scripting/namespaces.rs`; a package's namespace comes from
its crate name (`jengine-parallax` is `Parallax`). A package's script
commands go through `register_script_commands`, which every runtime
dispatches; `register_script_api` is native-only.

The dialects differ in the **standard library** too: Luau has `math.atan2`,
Fengari has a two-argument `math.atan(y, x)` and no `atan2`; `string.format("%d",
x)` on a fractional number truncates in Luau and throws in Fengari. Stick to
the common subset and floor numbers yourself.

The guards: `script_global_parity`, `host_command_parity`,
`preamble_command_tags`, `script_namespaces_reach_every_runtime`,
`play_state_reaches_browser_hosts`, and `node tools/conformance.mjs`, which
runs test-lab scene 224 natively and on the web and diffs every getter's answer.

## Test scenes - every engine feature needs one

`projects/engine-test-lab` holds one focused scene per engine feature,
reachable from a hub menu, run in every runtime to compare them. **An engine
feature is not finished until it has a scene there** - the recipe is in that
project's README under "Adding a scene".

## Build & run

Windows + PowerShell is the primary workflow; each script has a `.sh` twin for
macOS and Linux.

```powershell
./setup-dev.ps1         # once: wasm-pack, tauri-cli, the wasm target
./run-editor.ps1        # rebuilds the viewport WASM (release) and starts the editor
./build-viewport.ps1    # the viewport WASM alone (~3 min)
cargo run -p jengine-builder --bin jengine -- run projects/engine-test-lab
cargo run -p jengine-builder --bin jengine -- project new ../my-game
```

| Changed | Rebuild |
|---|---|
| Lua, scenes, prefabs, data, textures, sounds | None - reload the editor |
| `editor/src/**` | Vite hot reload |
| Rust in `jengine-core` or `jengine-editor-viewport` | `./build-viewport.ps1`, then reload |
| Rust for native | `cargo run -p jengine-runtime` |

`cargo check -p jengine-core` does **not** compile the renderer (it is behind
the `gpu-wgpu` feature): after touching `renderer/` or `ecs/wgpu_systems.rs`,
run `cargo check -p jengine-editor-viewport --target wasm32-unknown-unknown`.

## Checks

```powershell
./focused-test.ps1 -Tests <your_tests>   # per change: hosts compile, core + guards + goldens
cargo test --workspace -j 4              # -j 4, or link.exe runs out of memory
npm test --prefix editor
cargo test -p jengine-core --features gpu-wgpu --test render_golden   # pixels; needs a GPU
```

`render_golden` is the only check that looks at pixels. Without a GPU adapter
it prints why and passes, so run it on a machine with one before trusting a
render change. Cargo prints progress to stderr, so trust `test result: ok` in
the output over PowerShell's exit code.

## Branches and versions

Never commit directly to `main` or to a `release-*` branch. Work goes on a
branch made from `main` (`feat/`, `fix/`, `docs/`, `chore/`) and comes back
through a pull request. Versions are `MAJOR.MINOR.PATCH`: a whole new
generation, new features, fixes to what exists. The workspace `version` in
`Cargo.toml` is the engine's, and every crate and package follows it. The
editor, the website and each `packages/*/package.toml` carry the same number,
and the `versions_agree` test checks them. A package's `engine_version` is
the minimum engine it needs, not its own version. `CONTRIBUTING.md` has the
full rules and the release steps.

## Conventions

- Components derive `Debug, Clone, Serialize, Deserialize` and live in
  `ecs/components.rs`; systems are free functions.
- `anyhow::Result` for fallible functions, `thiserror` for library errors,
  `log` macros - never `println!` in library code.
- Asset files use semantic extensions (`.scene`, `.prefab`, `.atlas`), never
  `.scene.json`.
- Colours are **linear**; the screen is sRGB. Convert colours picked as sRGB.
- A new ECS component: the struct in `ecs/components.rs`, registration in
  `package/core_registration.rs`, and its field in `scene/format.rs`'s
  `ComponentBundle` (including `add_to_builder`).

Nothing in this repo can confirm how a change *feels* in-game; visual and
game-feel checks are a person's job - say so rather than implying a change
was seen working.
