---
name: parity-expert
description: "Script-API & asset parity expert for jEngine's four runtimes (native, web, editor viewport, and the Fengari Lua VM). Use when: adding or changing a script-facing function (set_*, get_*, spawn_*, etc.), wiring a new asset type into the editor, debugging 'attempt to call a nil value (global ...)' or 'missing texture/atlas/asset' errors that appear in ONE runtime but not another, or auditing whether a feature reaches the editor. Knows the exact per-site checklist so a global registered on native doesn't silently stay nil in the editor."
---

> **Before you start:** check [`docs/parity-gaps.md`](../../../docs/parity-gaps.md)
> for whether the thing you are about to wire is already a known gap — and when
> you close one, mark it **FIXED** there in the same change. The ratchet tests
> fail if a listed gap is closed without updating its entry, which is what keeps
> that file honest.

# Parity Expert (Script API + Asset Pipeline)

The single source of truth for making a script-facing function or an asset type
work **identically on every jEngine runtime**. Born from a real bug where
`set_sprite_region` worked on native but threw `attempt to call a nil value
(global 'set_sprite_region')` in the editor because the editor uses a *completely
separate* script execution path that nobody updated.

Use this skill BEFORE you implement, and again as a verification checklist AFTER.
It exists so the same multi-runtime gap never costs a debugging session twice.

## The #1 Rule

> **There is no single place that registers a script global or loads an asset.**
> Native, web, the editor viewport, and the Fengari Lua VM each have their own
> registration/upload path. Implementing one does NOT implement the others.
> Compilation passing means **nothing** about runtime parity here.

## The Four Script Execution Paths

| Runtime | Lua executes via | Globals registered in | Asset source |
|---------|------------------|------------------------|--------------|
| **Native** | `mlua` with **Luau** (Lua 5.1-based) | `scripting/bridge/adapters/luau.rs` globals + `ecs/systems.rs` apply-handlers | `std::fs` (raw scene file) |
| **Web** (`jengine-web`) | **Fengari**, the same VM and bundle as the editor (`lua-runtime.js`) | `jengine-web/src/script_host.rs` `drain_play_commands_into` | `fetch` |
| **Editor viewport** (`jengine-editor-viewport`) | **Fengari real Lua VM** | `editor/src/lib/lua-runtime.ts` `BRIDGE_FUNCTIONS` + viewport `drain_play_commands_into` | editor pushes bytes (no fs!) |
| **Browser JS preamble** | host JS (the IIFE the Fengari bridge calls into) | `crates/.../adapters/js/browser.rs` IIFE | — |

### How the editor's Lua path ACTUALLY works (the part that bites)

The editor does **not** use the string-based Lua→JS transpiler for globals. It
runs a real Lua VM (`fengari-web`) in `editor/src/lib/lua-runtime.ts`:

1. `registerBridge` iterates the hardcoded `BRIDGE_FUNCTIONS: string[]` list.
2. For each name it calls `window.__jengine_play.__api_lookup(name)`.
3. `__api_lookup` is browser.rs's `ns_lookup` closure → it does `eval(name)`
   inside the wrapper **IIFE scope** in
   `crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs`.
4. If that returns a function, it's installed as a Lua global.

**Therefore a new editor script global needs BOTH:**
- (a) the `function name(...) { ... }` defined in browser.rs's IIFE (so `eval`
  resolves it), AND
- (b) the bare name added to `BRIDGE_FUNCTIONS` in `lua-runtime.ts` (so the Lua
  global is actually installed).

Missing (b) is the classic `attempt to call a nil value (global 'X')` thrown
from `lua-runtime.ts`. Missing (a) makes the global install as `nil` silently.

Namespace methods (`sprite.set_region(...)`) come from the namespace objects in
the browser.rs IIFE (e.g. `var sprite = { ..., set_region: set_sprite_region }`)
which lua-runtime.ts rebuilds by enumerating keys for each name in `NAMESPACES`.
If your function returns multiple values (`local x, y = get_foo(e)`), add the
bare name to `MULTI_RETURN` in lua-runtime.ts too.

## Recipe: Add / change a script-facing function

Do ALL of these. Skipping any one leaves a runtime broken.

```
1. NATIVE behaviour      → crates/jengine-core/src/ecs/systems.rs
   (the authoritative apply-handler, e.g. SetSpriteRegion). This is the
   reference semantics every other path must mirror exactly.

2. BROWSER IIFE global   → crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs
   - Add `function my_fn(...) { window.__jengine_play.commands.push({ t:'<tag>', ... }); }`
   - **`<tag>` must be unused.** The tag namespace is flat and undeclared, and
     every runtime dispatches on it with a `match`, so reusing one is nearly
     silent: the first arm wins and the older function stops working. Only the
     runtime whose arm happens to come second warns "unreachable pattern".
     `set_time_scale` shipped as `ts`, which `tween_scale` already owned.
     Guard: `cargo test -p jengine-core --test preamble_command_tags`.
     near a sibling (set_sprite_anchor is a good template).
   - Add it to any namespace object it belongs to (e.g. `sprite`, `input`).
   - REMEMBER: this file is a Rust format! string — braces are DOUBLED `{{ }}`.
   - This single edit serves web, editor-viewport, AND the Fengari __api_lookup.

3. EDITOR Lua VM list     → editor/src/lib/lua-runtime.ts
   - Add the bare name to BRIDGE_FUNCTIONS.
   - If it returns multiple values, also add to MULTI_RETURN.

4. COMMAND DISPATCH — ask first: does it need anything but the world?

   4a. WORLD-ONLY (preferred) → crates/jengine-core/src/scripting/host_commands.rs
       - Add a `"<tag>" => { ... }` arm to `handle_world_command` and the tag to
         `SHARED_WORLD_TAGS`. Both browser hosts dispatch through it before
         their own `match`, so this is ONE edit instead of steps 4b and 5.
       - Read fields via the `CommandArgs` trait (`args.number/text/flag/json`),
         not `js_value_*`; resolve the entity with the `target` helper, which
         goes through the host's own id table.
       - **Write a test.** This is the only dispatch site a native test can
         reach — `JsonArgs(json!({...}))` drives it directly. The arms in 4b/5
         only run in a browser, which is why they went untested for months.

   4b. NEEDS HOST STATE → crates/jengine-editor-viewport/src/lib.rs
       - Audio, tweens, the save system, the scene stack, `SCRIPT_ENTITY_MAP`:
         these are per-host, so the arm stays here. Add a `"<tag>" => { ... }`
         arm in `drain_play_commands_into` mirroring the native apply-handler.
         Helpers: find_entity_by_id, js_value_f64, js_value_string; warn-once
         via the `unhandled_kinds` set; log to app.log_buffer; mutate via
         app.world.get::<&mut Component>(entity).

5. WEB command drain       → crates/jengine-web/src/script_host.rs
   - Only if you took 4b. Add the SAME `"<tag>"` arm in its own
     `drain_play_commands_into` (separate copy from the viewport). Often
     forgotten — its absence is a web-only no-op, not a crash, so it slips
     through. Taking 4a instead removes this step entirely.

6. DOCS                    → docs/scripting-api.md
   - Add the row. State honestly which runtimes support it.

7. REBUILD the editor wasm → .\build-viewport.ps1   (wasm-pack)
   - Regenerates editor/src/lib/viewport-wasm/* glue. Rust edits in steps 2/4
     do NOT reach the running editor until this runs AND the page is reloaded
     (HMR reloads TS but not the wasm).
```

## Recipe: Wire a new asset type into the editor

The WASM viewport has **no filesystem**. The native runtime reads files from
disk; the editor must push every byte in.

```
1. NATIVE resolve         → how native loads it (e.g. wgpu_scene_runner.rs
   build_state resolves scene `atlas_refs` from the RAW scene file).

   ⚠ Gotcha: prefab expansion / typed (de)serialization DROPS unknown
   top-level scene fields. Both native build_state AND the editor must read the
   ORIGINAL authored `.scene` JSON to recover fields like `atlas_refs` /
   `audio_refs` — the typed loader (Rust SceneData / Tauri load_scene) silently
   discards them.

2. UPLOAD fn in viewport   → crates/jengine-editor-viewport/src/lib.rs
   - Add `#[wasm_bindgen] pub fn viewport_load_<thing>(id, json_or_bytes)`
     that parses and inserts into app.assets (mirror viewport_load_gui /
     viewport_load_texture).

3. EDITOR preload          → editor/src/lib/Viewport.svelte
   - Add a `loadScene<Thing>(...)` that reads bytes via
     readAssetBinary(proj.path, id) (binary) or readTextFile(scenePath) (raw
     scene), then calls the new viewport_load_* fn.
   - Dedupe with a `Set` like `loadedTextures`.
   - Call it alongside loadSceneTextures at ALL its call sites (initial scene
     subscribe, project subscribe, AND script-driven scene transition — the
     transition passes the target scene path, not get(scenePath)).
   - Import the new wasm export in the top import block.

4. ID CONVENTION           → match native exactly.
   - Atlases: id = sibling-PNG path. `derive_atlas_id` normalizes `\`→`/` and
     swaps the extension to `.png` (atlases/x.atlas → atlases/x.png). Scripts
     pass that `.png` id to set_sprite_region; the editor must push the atlas
     data AND the sibling PNG texture under that same id.

5. REBUILD                 → .\build-viewport.ps1, then reload the editor page.
```

## Failure-signature → cause cheat sheet

| Symptom (and where it prints) | Most likely missing site |
|-------------------------------|--------------------------|
| `attempt to call a nil value (global 'X')` from `lua-runtime.ts` | Name not in `BRIDGE_FUNCTIONS` (step 3) and/or no IIFE function (step 2) |
| Same nil error on native (`mlua`) | Native bridge/apply-handler missing (step 1) |
| Global callable but does nothing in editor; no error | Viewport `drain_play_commands_into` arm missing (step 4) |
| Works native + editor, no-op on web | `jengine-web` drain arm missing (step 5) |
| `viewport: missing texture/atlas '<id>'` | Editor never pushed the asset (asset recipe steps 2-3) or wrong id convention (step 4) |
| Native logs no `resolved N atlas reference(s)`, all regions "not found" | Top-level scene refs dropped by typed/prefab round-trip — resolve from raw file |
| Editor change in `.rs` has no effect after save | Forgot `build-viewport.ps1` + page reload |
| `eval is not defined` / undefined in `__api_lookup` | Function defined OUTSIDE the browser.rs IIFE scope |

## Pre-merge verification checklist

- [ ] Native apply-handler exists and is the semantic reference (`ecs/systems.rs`)
- [ ] browser.rs IIFE has the `function` (doubled braces) + namespace entry
- [ ] `lua-runtime.ts` BRIDGE_FUNCTIONS (+ MULTI_RETURN if multi-value)
- [ ] Editor viewport `drain_play_commands_into` arm mirrors native
- [ ] `jengine-web` `script_host.rs` drain arm mirrors native (or documented gap)
- [ ] Asset types: `viewport_load_*` export + `Viewport.svelte` preload at ALL call sites
- [ ] Asset ids match the native convention exactly (e.g. atlas → sibling `.png`)
- [ ] `docs/scripting-api.md` updated with honest per-runtime support
- [ ] `cargo check -p jengine-core -p jengine-editor-viewport -p jengine-web`
- [ ] `.\build-viewport.ps1` run; editor page hard-reloaded and the path tested live
- [ ] Tested the actual script in the editor in Play mode (not just compiled)

## Notes & known gaps (keep current)

- The editor and web builds both run **real Lua 5.3** via Fengari; native runs
  **Luau**. Stick to the common subset, standard library included (see
  "Parity" in `CLAUDE.md` for `math.atan2` and `%d`).
- `jengine-web` and the editor viewport each have a SEPARATE copy of
  `drain_play_commands_into`. They drift easily — always update both, unless
  the command needs only the world: then write it once in
  `scripting/host_commands.rs` and add its tag to `SHARED_WORLD_TAGS`.
- `set_sprite_region` parity reference implementation lives across: browser.rs,
  lua-runtime.ts (BRIDGE_FUNCTIONS), the viewport and web `ssr` arms +
  `viewport_load_atlas`, and `Viewport.svelte` `loadSceneAtlases`.

## Namespaces

A script function is installed under its **flat** name everywhere above,
and then gathered into its **namespace** (`Sprite.set_color`) by every
runtime from `crates/jengine-core/src/scripting/namespaces.rs`. So a new
function also needs its row in `CORE` there -
`script_namespaces_reach_every_runtime` fails without one - and the docs and
game scripts use the namespaced name. Nothing else about the namespaces is
per-runtime: do not add namespace tables by hand anywhere.

## Related

- [`docs/parity-gaps.md`](../../../docs/parity-gaps.md) — every known place the
  four runtimes disagree, and which ratchet test guards it.
