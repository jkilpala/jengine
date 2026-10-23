# Scripting Performance: Lessons Learned & Alternatives

> The raw benchmark files this document cites (`docs/baselines/...`) and the
> `projects/scripting-perf` harness that captured them were removed in Sep
> 2026; the numbers predate the wgpu-era renderer and nothing compared
> against them any more. They are in git history: `git show
> dab0d5c:docs/baselines/<path>`. The conclusions below are what was kept.

## Context

During BunnyMark optimization (April 2026), we traced a massive performance gap between the native runtime and the WASM/editor runtimes. At 20,000 bouncing entities the native runtime achieved **143 FPS** — but only after moving per-entity physics out of JavaScript and into a native Rust ECS system. This document records what we learned and the architectural choices that followed.

---

## The Problem: Script-Per-Entity Doesn't Scale

The original design gave every bunny a `script` component running `bunny.js` via QuickJS (native) or browser JS (WASM/editor). Each bunny's `update()` function ran every frame to apply gravity, integrate velocity, and bounce off walls.

### Measured Cost (20,000 entities)

| Runtime | Script time per frame | FPS |
|---------|----------------------|-----|
| Native (QuickJS, per-entity script) | ~62 ms | 15 |
| WASM (browser JS, batched) | ~1.2 ms | 60 |
| Native (after moving to Rust ECS) | ~1.5 ms | 143 |

The native scripting path was **40x slower** than native Rust for the same logic.

### Root Causes

1. **Per-entity function call overhead** — Each entity's `update()` crossed the Rust↔QuickJS boundary individually. With 20K entities that's 20K boundary crossings per frame.

2. **O(n²) API lookups** — Script API functions like `get_position()`, `get_custom()` used linear Vec scans to find entity data. Each of 20K entities scanning a 20K-length Vec = 400M operations/frame.

3. **Snapshot rebuilds** — `snapshot_world()` iterated ALL entities every frame to build script-visible state, even when only a few entities had scripts.

4. **No batching** — WASM/editor runtimes batch all `update()` calls into a single JS function call. Native had no equivalent.

---

## Optimizations Applied

### 1. Move hot-loop logic to native Rust (biggest win)

Created `Velocity2D` and `BounceArea` ECS components with a native `velocity_system()`:

```rust
pub fn velocity_system(world: &mut World, dt: f32) {
    for (_, (transform, vel, bounce)) in
        world.query_mut::<(&mut Transform, &mut Velocity2D, Option<&BounceArea>)>()
    {
        vel.y -= vel.gravity * dt;
        transform.position.x += vel.x * dt;
        transform.position.y += vel.y * dt;
        // ... bounce logic
    }
}
```

**Result**: 20K entities went from **15 FPS to 143 FPS**. The spawner script still runs (it calls `set_velocity()` on spawn), but per-frame physics is pure Rust.

### 2. Replace Vec lookups with HashMap (O(1) reads)

Script API getters (`get_position`, `get_rotation`, `get_scale`, `get_sprite_color`, etc.) switched from `Vec::iter().find()` to `HashMap<u64, Data>` keyed by entity ID.

### 3. Lazy snapshots — only process scripted entities

New `snapshot_*_for()` variants accept a list of scripted entity IDs and only snapshot those, instead of iterating the entire world.

### 4. Direct world pointer reads (zero-copy)

Getter functions read directly from the hecs `World` via a stored `*const World` pointer, bypassing the snapshot entirely for read-only queries.

### 5. Cached Persistent<Function> handles

QuickJS function handles are cached per script path, avoiding repeated lookups in the JS engine's global scope.

---

## Architecture: When to Use Scripts vs Native Systems

### Use scripts for **cold logic** (runs infrequently or on few entities):

- Spawner logic (spawn N entities on input)
- Game state management (score tracking, level transitions)
- UI update handlers (button clicks, menu navigation)
- One-shot initialization
- Event-driven responses (collision callbacks, timer callbacks)
- Prototyping new mechanics before committing to Rust

### Use native Rust systems for **hot loops** (runs every frame on many entities):

- Physics integration (velocity, gravity, collisions)
- Transform updates (following paths, orbiting)
- Animation state machines
- Particle simulation
- Spatial queries (nearest-neighbor, frustum culling)
- Any per-entity logic that must run at >1000 entities

### Rule of thumb

> **If a script's `update()` runs on more than ~100 entities per frame, move it to a native system.**

The script can still _configure_ the system (set initial velocity, change gravity) — it just shouldn't _run_ the math every frame.

---

## Cross-Runtime Considerations

The engine runs on three runtimes, each with different scripting characteristics:

| | Native (QuickJS/mlua) | WASM (browser JS) | Editor Viewport (browser JS) |
|---|---|---|---|
| **Script engine** | QuickJS (rquickjs) / Lua 5.4 (mlua) | Browser V8/SpiderMonkey | Browser V8/SpiderMonkey |
| **Boundary cost** | High (Rust↔C FFI) | Medium (WASM↔JS) | Medium (WASM↔JS) |
| **Batching** | Per-entity calls | Single batched call | Single batched call |
| **State sync** | Direct ECS pointer | Float64Array bulk transfer | Float64Array bulk transfer |
| **JIT** | No (interpreter only) | Yes (V8 TurboFan) | Yes (V8 TurboFan) |

Browser JS with JIT compilation is **significantly faster** than QuickJS for per-entity update loops. But even browser JS can't match native Rust ECS iteration — the boundary-crossing and GC overhead still matters at scale.

---

## SceneData Velocity System (WASM/Editor)

The WASM runtime and editor viewport don't use hecs — they operate on `SceneData` directly. A separate `velocity_system_scene()` was added:

```rust
pub fn velocity_system_scene(scene: &mut SceneData, dt: f32) {
    for entity in &mut scene.entities {
        let (Some(transform), Some(vel)) =
            (&mut entity.components.transform, &mut entity.components.velocity)
        else { continue; };
        // ... same gravity + bounce logic
    }
}
```

Key detail: `sync_js_state_to_scene()` copies positions from JS → Rust every frame. For velocity-driven entities, this must be **skipped** (Rust is authoritative for their positions). The sync function now checks `entity.components.velocity.is_none()` before overwriting positions.

---

## Sprite Batch Limit

`MAX_SPRITES = 8192` in the SpriteBatch. Index buffer uses `u16`, capping at 65535 vertices (16383 sprites max). The native runtime's `render_sprites()` already auto-flushes via `batch.is_full()` → `batch.flush()`. The viewport and web runtime rendering loops needed the same auto-flush added — without it, sprites beyond 8192 were silently dropped.

---

## Alternatives Considered

### 1. Compile scripts to WASM (not pursued)

Compile Lua/JS scripts to WASM modules at build time. Would eliminate the interpreter overhead but adds massive build complexity and loses hot-reload. Only makes sense if scripting perf is the primary bottleneck AND native systems aren't an option.

### 2. AOT-compile scripts to Rust (not pursued)

Transpile script logic to generated Rust code during the build step. Maximum performance but loses all runtime flexibility. Essentially auto-generates ECS systems from scripts.

### 3. Replace QuickJS with V8 on native (not pursued)

Embedding V8 would bring JIT compilation to native. But V8 is a 30MB+ dependency, has complex build requirements, and the real win comes from avoiding per-entity scripting entirely.

### 4. Current approach: split architecture (adopted)

- Scripts for cold/infrequent logic → QuickJS/Lua (works fine)
- Native ECS systems for hot loops → Rust (maximum performance)
- `set_velocity()` / `set_*()` bridge between them → scripts configure, Rust executes

This gives the best of both worlds: rapid iteration via scripts for game logic, native performance for simulation.

---

## Performance Summary

| Approach | 20K entities | Notes |
|----------|-------------|-------|
| Per-entity JS (QuickJS) | 15 FPS | O(n²) lookups, 20K boundary crossings |
| Per-entity JS (browser, batched) | 60 FPS | JIT + single batch call |
| Native Rust ECS system | 143 FPS (debug) / 159 FPS (release) | Zero overhead, SIMD-friendly |

**Takeaway**: For any game shipping with jEngine, hot-loop logic should be a native system. Scripts are for orchestration, not simulation.

---

## Step 4 Pre-Baseline (2026-04-28) — Per-Entity JS vs Lua, 60 Hz

Captured immediately before starting Step 4 (batch JS systems) of the scripting redesign. Bench harness: `jengine-runtime --bench`, headless renderer, fixed dt 1/60, 600 measurement frames after 120-frame warmup, default tick rate (60 Hz, no `tick_rate` header on the bench scripts so they hit the legacy compat path). Same `seed=3405691582`. Each entity owns one script (per-entity model). All numbers are per-frame averages; `scripts_total = prepare + run_lua + run_js + route + apply`.

| Variant | Entities | scripts_total (ms) | prepare | run_lua | run_js | route | apply | frame avg (ms) | frame p99 | FPS  | host calls (lua/js) | writeback bytes/frame |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 20k_lua_only | 20 000 | **65.72** | 12.45 | **43.86** |  0.00 | 1.27 | 8.13 |  67.74 |  73.86 | 14.8 | 0 / 0 | 3 840 000 |
| 20k_js_only  | 20 000 | **91.42** | 12.23 |  0.00 | **70.19** | 1.32 | 7.68 |  93.44 | 101.84 | 10.7 | 0 / 40 000 | 3 840 000 |
| 30k_lua_only | 30 000 | **98.81** | 19.74 | **65.10** |  0.00 | 1.91 | 12.06 | 101.84 | 108.70 |  9.8 | 0 / 0 | 5 760 000 |
| 30k_js_only  | 30 000 | **135.09** | 19.72 |  0.00 | **101.73** | 1.99 | 11.66 | 138.19 | 152.13 |  7.2 | 0 / 60 000 | 5 760 000 |

Raw JSON: `docs/baselines/step4-pre/20k_js_only.json`, `20k_lua_only.json`, `30k_js_only.json`, `30k_lua_only.json`.

### What this tells us

1. **JS per-entity is ~1.6× more expensive than Lua per-entity** at the same entity count (70.19 vs 43.86 ms at 20k; 101.73 vs 65.10 ms at 30k). The Luau adapter has the cheaper boundary today.
2. **`prepare` scales linearly with entity count** (~0.6 µs/entity) and is the floor regardless of language — it's the snapshot-collection walk in `ScriptSystem`. Step 4 will not reduce this number for entities that remain on the per-entity path; it only matters that batched systems skip it.
3. **`run_js` dominates total cost for JS at scale** — 77% of `scripts_total` at 20k, 75% at 30k. This is the budget Step 4 needs to compress: the spec target is **<2–4 ms scripts_ms** for 20k batched movers, i.e. an **~8–10× improvement vs run_js alone**, or **~20–25× vs scripts_total** if `prepare` is also bypassed for batched entities.
4. **`route + apply` is small but non-trivial** (≈9 ms at 20k JS) — it's the command-buffer drain. Step 4's batch path writes back through the same writeback channel (3.84 MB/frame at 20k = 192 bytes/entity × 20k), so a 20k all-batched run should still pay roughly this much in `apply` unless writeback is also redesigned. **Out of scope for Step 4** per the plan; flagged for Step 5+.
5. **`vm_to_rust_host_calls_js = 2 per entity` (40k for 20k entities)** — these are `set_position` (or equivalent) calls from `update()`. Step 4's success criterion "near-zero host calls inside the loop" means this number should drop to ~0 per batched entity (replaced by direct TypedArray writes) — making it a clean, measurable counter for the post-baseline.
6. **30k extrapolates linearly from 20k** for both adapters (scripts_total ratio ~1.50 vs entity ratio 1.50). No cliff yet; we're firmly CPU-bound on script execution.

### Targets for Step 4 post-baseline (same 20k_js variant, batched)

- `scripts_total` ≤ **4 ms** (vs 91.42 ms today → **23× speedup**)
- `run_js` ≤ **2 ms** (vs 70.19 ms today → **35× speedup**)
- `vm_to_rust_host_calls_js` ≤ **2 per frame** (the batched `update(ctx, q)` call itself, vs 40 000 today)
- `frame_ms.avg` ≤ **6 ms** (60 FPS-safe at 20k pure-JS — currently 93.44 ms)
- New counters from the spec (`batch_systems_run`, `batch_buffer_bytes_in/out`, etc.) wired and non-zero.

---

## Step 4 Walking Skeleton (2026-04-28) — Batched JS Velocity Integrator

End-to-end vertical slice of the Step 4 design: a `BatchScriptComponent { path }` marker, a single `BatchJsRuntime` (rquickjs `Context::full` sandbox, no `engine` global, no per-entity bridge), one `run_batch_scripts` system that gathers Velocity2D/Transform columns into TypedArrays, calls `update(ctx, q)` once per script per frame, and writes the mutated `q.write.Position2D.{x,y}` back to ECS. The bench fixture is a `JsBatchMover` prefab whose `mover_batch.js` does pure velocity integration via `q.write.Position2D.x[i] += q.read.Velocity2D.x[i] * ctx.dt`.

Same harness as the pre-baseline (headless, fixed dt 1/60, 600 frames, seed `0xcafebabe`). The new variants are **JS-only** runs of the batched mover prefab — no Lua, no per-entity JS, no native — so every script tick is a single VM call.

| Variant | Entities | scripts_total¹ (ms) | batch_run_us | batch_copy_in_us | batch_copy_out_us | batch_vm_calls | host calls (lua/js) | frame avg (ms) | frame p99 |
|---|---|---|---|---|---|---|---|---|---|
| 20k_js_batch_only | 20 000 | **4.25** |  4 199 | 39 |  8 | **1** | 0 / **0** |  7.68 |  9.12 |
| 30k_js_batch_only | 30 000 | **6.32** |  6 244 | 60 | 16 | **1** | 0 / **0** | 11.39 | 13.08 |

¹ `scripts_total` here is `prepare + run_lua + run_js + route + apply + batch` where `batch = (copy_in + run + copy_out) µs / 1000`. `prepare/run_lua/run_js/route/apply` are all 0 because no entity has a per-entity script.

Raw JSON: `docs/baselines/step4-walking-skeleton/20k_js_batch_only.json`, `30k_js_batch_only.json`.

### Headline numbers vs pre-baseline (20k JS)

| metric | per-entity JS (pre) | batched JS (this slice) | speedup |
|---|---|---|---|
| `scripts_total` | 91.42 ms | **4.25 ms** | **21.5×** |
| dominant cost (`run_js` → `batch_run_us`) | 70.19 ms | **4.20 ms** | **16.7×** |
| `vm_to_rust_host_calls_js` | 40 000/frame | **0/frame** | ✓ (target: ≤2) |
| `frame_ms.avg` | 93.44 ms | **7.68 ms** | **12.2×** |

### What this tells us

1. **The host-call target is met by construction** — the sandbox has no `engine` global and exposes only `q.read/q.write` TypedArrays. Counter is `0` per frame, vs the pre-baseline's 40 000. Crossing the boundary once per script per frame is the entire point of the design and the data confirms it.
2. **The `scripts_total ≤4 ms` and `run ≤2 ms` targets are missed by ~6%/~110%.** `batch_run_us = 4 199` is dominated by the JS interpreter walking 20 000 iterations of a pure number-crunching loop with TypedArray indexed access. Copy-in/copy-out (47 µs combined, ~1.1% of `batch`) is essentially free — the bottleneck is **inside QuickJS**, not the bridge. Closing the gap requires either (a) JIT/AOT (out of scope), (b) chunking + reducing per-element JS work (Step 4.2), or (c) accepting that 4 ms is good enough and spending the remaining budget elsewhere. Decision deferred to Step 4.2 review.
3. **Linear scaling holds: 30k batch = 1.50× of 20k batch** (6.32 / 4.25 = 1.49), matching the pre-baseline's linearity. There is no allocation cliff and no hidden per-frame overhead — `BatchJsRuntime` reuses one Context, one Persistent `update` Function, and four pre-allocated TypedArrays per script.
4. **Copy bandwidth is negligible at this size.** 320 kB in + 160 kB out per frame at 20k entities, transferred in 47 µs combined → ~10 GB/s of effective throughput, well within memcpy. This validates the "TypedArray over JSON" choice; a wider component schema (3–4 vec2 columns) would still fit comfortably under 200 µs per frame.
5. **`other_ms` (`frame - scripts - render - swap`) is 7.58 ms at 20k**, larger than the entire script budget. With per-entity JS gone, the next bottleneck for headless benches will be ECS query/iteration overhead, not scripting. Worth profiling if frame_ms.avg becomes the constraint.

### What's next (Step 4.2)

- Drop the `export function update` text rewrite — load via `Module::evaluate` or expose CommonJS-style `module.exports`.
- Spec-compliant header parsing: read `tick_rate`, `priority`, `requires { read: [...], write: [...] }` from a JSDoc-like block instead of hard-coding Velocity2D→Position2D.
- Multi-script dispatch: per-script TypedArray pool keyed by `(script_path, schema_hash)`.
- Then re-measure and decide whether the remaining `~4 ms run_js / 20k` warrants chunking + Web Worker–style fanout, or whether it's already in the noise for real games.

## Step 4 Slice 4.2 (2026-04-28) — Header parsing + multi-script dispatch

Slice 4.2 replaced the `source.replace("export function update", "function update")` hack with a proper IIFE wrapper that binds `module = { exports: {} }` and `exports = module.exports` ahead of user source, then returns `{ update, system }` from local declarations or `module.exports.*`. The `system` constant is now parsed into a `BatchSystemSpec { read, write, tick_rate }` and validated at load time against the walking-skeleton runner's fixed Velocity2D→Position2D contract — a mismatched spec hard-fails with `BatchError::SpecMismatch` instead of running with the wrong data. Scripts that omit `system` still load (one-shot warn) for back-compat.

| variant | scripts.batch (ms) | batch_run_us | frame_ms.avg | vs Slice 4.1 |
|---------|--------------------|--------------|--------------|--------------|
| 20k_js_batch_only | **3.80** | **3 755** | **6.94** | -10.6% scripts, -9.6% frame |
| 30k_js_batch_only | **5.68** | **5 613** | **10.47** | -10.1% scripts, -8.1% frame |

The numbers are within typical bench noise but **consistently better at both sizes** — likely because the new wrapper evaluates user source exactly once inside the IIFE rather than going through `replace()` + an extra eval round-trip per call site. Counters are otherwise identical to Slice 4.1 (host calls 0/frame, 1 batch system, 1 VM call, 1 chunk per frame).

Test coverage grew from 3 → 7 unit tests for `scripting::batch`:
- `loads_commonjs_style_export` — `module.exports.update = …` loads and runs.
- `rejects_mismatched_system_spec` — `read=["Health"]` fails with `SpecMismatch`.
- `accepts_script_without_system_header` — back-compat path warns + loads.
- `multi_script_dispatch_keeps_scripts_isolated` — two distinct scripts in one runtime maintain independent `update` Persistents (no global-leak).

### What's next (Step 4B and beyond)

- **Step 4B**: Lua/Luau batch mover. Reuse the `BatchSystemSpec` shape; biggest open question is how to expose mirror buffers — `mlua` has no `TypedArray` equivalent, so probably `userdata` wrapping a `Vec<f32>` slice with `__index`/`__newindex`, or accept a copy through a `LuaTable` of numbers if the overhead is tolerable at 20k.
- **Step 4C**: parallel batch script workers. The 4–6 ms `run_js` is now plain QuickJS interpreter time — chunking the entity range across N workers (each with a long-lived `BatchJsRuntime`) is the obvious next lever if real games hit it. Keep the runner single-VM until measured demand.

## Step 4B Slice 4B.1 (2026-04-28) — Lua batch mover, table vs userdata buffer modes

Slice 4B.1 ports the JS batch walking skeleton to Luau (`mlua` 0.10) so we can compare the two scripting backends apples-to-apples on the *same* hot path. Code-side changes: a `ScriptLang` enum (`Js` | `Lua`) was added to `BatchScriptComponent` (defaults to `Js` for back-compat), `run_batch_scripts` now groups by `(lang, path)` and dispatches to the matching runtime, and a new `BatchLuaRuntime` mirrors `BatchJsRuntime` field-for-field.

The interesting design question for Lua was **how to expose `q.read.Velocity2D.x` / `q.write.Position2D.x` to scripts**, since Luau has no native typed-array. Slice 4B.1 ships two strategies — selectable via `--lua-batch-buffer {table|userdata}` — so the cost of each is measured directly. A `--batch-noop` flag rewrites every batch script's path stem to `mover_batch_noop.<ext>` after spawn, isolating the per-frame *marshalling* cost from in-script arithmetic. The same noop control was added retroactively for the JS runtime so the comparison is fair.

### Headline numbers (30k entities, 600 frames headless, seed 0xcafebabe, dt=1/60)

| variant                              | frame_ms.avg | scripts.batch (ms) | cp_in (µs) | run (µs)  | cp_out (µs) |
|--------------------------------------|--------------|--------------------|------------|-----------|-------------|
| `30k_js_batch_only` (real work)      | 10.47        | 5.68               | 50         | **5 613** | 12          |
| `30k_js_batch_noop` (FFI only)       | 4.52         | 0.06               | 49         | 2         | 5           |
| `30k_lua_batch_table` (real work)    | **9.06**     | 3.64               | 1 449      | **686**   | 1 501       |
| `30k_lua_batch_noop_table`           | 7.91         | 2.98               | 1 465      | 1         | 1 515       |
| `30k_lua_batch_userdata` (real work) | 18.24        | **13.20**          | 117        | **13 061**| 16          |
| `30k_lua_batch_noop_userdata`        | 4.86         | 0.12               | 110        | 3         | 6           |

(20k numbers track the 30k numbers proportionally — see `docs/baselines/step4-walking-skeleton-lua/20k_*.json` and `docs/baselines/step4-walking-skeleton/20k_js_batch_noop.json`.)

### Lessons

1. **Luau loop arithmetic is ~8× faster than QuickJS at this load**. Subtracting the noop control: Lua-table real `run` is ~685 µs; JS real `run` is ~5 611 µs. Same `for i = 1, n do px[i] = px[i] + vx[i] * dt end` shape, same 30k iteration count. Luau's register-based VM with type-narrowing wins the inner loop decisively.

2. **Lua *userdata* mode is catastrophic for dense access patterns**. Every `px[i]` access invokes the Rust `__index`/`__newindex` metamethod. At 30k entities × 4 accesses per iteration that's 480k metamethod round-trips per frame, costing ~13 ms — a **19× slowdown vs the table mode** doing the exact same arithmetic. Userdata-with-metamethods is the wrong tool when scripts touch every element on every frame; it would make sense for sparse indexing or for buffers that scripts pass around without iterating.

3. **Table mode's marshalling cost is the new bottleneck**. `lua.create_sequence_from(slice.iter().copied())` runs in ~1.45 ms per 30k-element column; we build four (vx, vy, px, py) per frame and read two back. Total marshalling: ~2.95 ms — about *four* times what the actual arithmetic costs. Compare to JS `Float32Array`: ~50 µs total (a single zero-copy `ArrayBuffer::new_with_bytes` on the in side, zero-copy borrow on the out side). A Luau `buffer` mode (proposed Slice 4B.2) should close most of this gap because Luau buffers expose a contiguous byte view that mlua can populate via `Buffer::write_*`.

4. **The "Lua beats JS on full work" result is mostly an illusion**. Lua-table real frame is 9.1 ms vs JS 10.5 ms, but that's only because Lua's slow marshalling (~3 ms) is comparable to JS's fast loop (~5.6 ms). Build a Luau buffer mode and the picture changes: Lua marshalling drops toward 100 µs while the loop stays at ~700 µs, so Lua would beat JS by ~5 ms on 30k. Until then, **Lua table mode is competitive with JS only because both runtimes have *different* dominant costs that happen to add up similarly**.

5. **The noop control is now a permanent part of the test matrix**. It cleanly separates "FFI + buffer construction cost" from "in-script work cost", which made the userdata-vs-table tradeoff legible at a glance. Future buffer modes (Luau `buffer`, SoA-pointers, persistent buffers) should always ship with their noop counterpart.

### What's next

- **Slice 4B.2 candidate**: Luau `buffer` mode (`mlua::Buffer` + `buffer.readf32` / `buffer.writef32`). Expected to drop Lua-table cp_in from ~1.45 ms to ~50 µs based on the JS Float32Array number. Likely the cheapest single optimization in the whole batch system.
- **Defer Step 4C (parallel workers) until 4B.2 lands.** If Luau-buffer makes single-thread Lua finish a 30k batch in ~1 ms total, parallel workers might not be worth their architectural complexity for any realistic project budget.
- **Userdata mode stays in the codebase** as a reference point and a sparse-access fallback. It's not a mistake — it's just the wrong shape for dense per-element scripts. Document this clearly in the scripting API guide so users don't reach for it by default.
- **Broader `system` parsing**: `tick_rate`, `priority`, `requires { read, write }` with arbitrary component names — gated on Step 4B landing so we design the spec against two backends, not one.

## Step 4B Slice 4B.2 (2026-04-28) — Luau native `buffer` mode

Slice 4B.2 added the third `BatchLuaBufferMode::Buffer` variant using `mlua::Buffer` and the Luau-native `buffer.readf32(b, byteOffset)` / `buffer.writef32(b, byteOffset, v)` API. Host marshalling collapses to a single contiguous memcpy per column (`lua.create_buffer(f32_slice_as_bytes(c))`) plus a `Buffer::to_vec()` on readback. The hypothesis from Slice 4B.1 was that this would win on every axis.

**It didn't.** The numbers tell a more interesting story.

### Headline numbers (30k entities, 600 frames headless, seed 0xcafebabe, dt=1/60)

| variant                              | frame_ms.avg | scripts.batch (ms) | cp_in (µs) | run (µs)   | cp_out (µs) |
|--------------------------------------|--------------|--------------------|------------|------------|-------------|
| `30k_lua_batch_table` (real work)    | **9.06**     | **3.64**           | 1 449      | **686**    | 1 501       |
| `30k_lua_batch_userdata` (real work) | 18.24        | 13.20              | 117        | 13 061     | 16          |
| `30k_lua_batch_buffer` (real work)   | 14.17        | 8.89               | **47**     | 8 825      | **19**      |
| `30k_lua_batch_noop_table`           | 7.91         | 2.98               | 1 465      | 1          | 1 515       |
| `30k_lua_batch_noop_userdata`        | 4.86         | 0.12               | 110        | 3          | 6           |
| `30k_lua_batch_noop_buffer`          | **4.58**     | **0.06**           | **44**     | 0          | 14          |

(20k numbers track proportionally — see `docs/baselines/step4-walking-skeleton-lua/20k_lua_batch_buffer{,_noop}.json`.)

### Lessons

1. **Buffer mode wins host marshalling decisively** — `cp_in` drops from 1 449 µs (table) to 47 µs (buffer), a **30× improvement**, and matches JS `Float32Array` (~50 µs) almost exactly. `cp_out` drops similarly (1 501 → 19 µs). The noop control confirms this is pure marshalling cost: `30k_lua_batch_noop_buffer` runs the entire batch system in **58 µs** (60 µs scripts.batch). That's the floor of what the FFI layer can do for a 30k-entity column on this VM.

2. **But per-element script access is ~13× slower with `buffer.readf32` than with table indexing**. Run cost goes the wrong way: 686 µs (table) → 8 825 µs (buffer). Each `buffer.readf32(px, off)` is a function call into the Luau standard library — it doesn't get inlined the way bytecode-level `t[i]` does. With 30k entities × (4 reads + 2 writes) per iteration = 180 000 buffer function calls per frame, the per-call overhead (~50 ns) dominates.

3. **Net result: buffer mode is SLOWER than table mode for dense access** (8.89 ms vs 3.64 ms total at 30k — table wins by 2.4×). This is the opposite of what was predicted in Slice 4B.1's lesson #4. The marshalling savings don't pay for the per-element function-call tax once you sum across the whole hot path.

4. **When buffer mode WOULD win**: scripts that touch only a small fraction of elements per frame, or scripts that hand the buffer to native code (e.g. a future SIMD helper) without iterating in script. For sparse access at, say, 10% of elements, marshalling savings would still apply (1 400 µs saved per frame) while per-element cost would drop to ~880 µs — buffer wins. For ≥30% dense access, table mode wins.

5. **Lua's three buffer modes now occupy three distinct positions on the access-density vs marshalling-cost tradeoff**:
   - **`Table`** (default): cheap per-element access (bytecode `t[i]`), expensive bulk marshalling. Best for **dense access at small/medium N**.
   - **`Buffer`**: cheap bulk marshalling, expensive per-element access (function call). Best for **sparse access** or **large N where you only touch a fraction**.
   - **`UserData`**: catastrophically expensive per-element access (Rust metamethod call), cheap-ish marshalling. **Never use for dense access**; potentially useful for opaque handles passed between scripts.

6. **The original "Lua beats JS" framing from Slice 4B.1 lesson #4 still holds, but for a different reason.** With buffer mode failing to deliver the predicted speedup, the path to "Lua decisively beats JS on dense batch workloads" is *not* a faster marshalling layer — it's the fact that Luau's bytecode loop is already 8× faster than QuickJS's. The bottleneck is genuinely the marshalling, but only because the loop is so cheap. To make Lua *also* match JS on cp_in/cp_out without sacrificing run cost would need either (a) a custom Luau opcode (rejected: not in scope), (b) a userdata that exposes a `Vec<f32>` reference with bytecode-fast indexing (Luau doesn't support this — `__index` is mandatory for userdata), or (c) accepting the table marshalling cost as the price of admission, which is what we do.

7. **Diagnostic lesson: always pass `--bench` when running `jengine-runtime` for measurement.** The 4B.2 baseline capture initially hung silently because the runner script omitted `--bench` — the runtime parsed all the bench-specific flags, ignored them, fell through to interactive game mode, and never exited. Symptom looked like a runtime regression (CPU burning, no JSON output); actual cause was a one-character CLI omission. **Lesson**: when you see "the bench is hanging and burning CPU", the first hypothesis should be "is `--bench` actually set?" before opening the source. Add this check to every future baseline-capture script.

### What's next

- **Decision: skip Slice 4B.3 (custom Luau opcodes / SoA userdata).** Buffer mode landed at 8.83 ms run cost; the additional engineering to drive it down to table-mode levels (~700 µs) would essentially be reimplementing typed-array semantics on top of Luau, and the table mode already gives us those numbers natively. Better return on time elsewhere.
- **Move to Step 4C (parallel batch workers)** as the next concrete optimization. With per-VM Lua at ~3.6 ms for 30k entities, splitting the work across 4 workers could push the walking-skeleton workload to ~1 ms — competitive with hand-written native systems.
- **Document buffer mode as a power-user opt-in** in `docs/scripting-api.md` once the API stabilizes. Default scripts should use table mode unless they have a measured sparse-access pattern.
- **Keep userdata mode** as a reference (lesson 5) but warn loudly in any docs that suggest it.

## Step 4C Slice 4C.1 (2026-04-28) — JS batch worker pool

Slice 4C.1 added a persistent worker-thread pool for JS batch scripts. New module `crates/jengine-core/src/scripting/batch_workers.rs` exposes `BatchJsWorkerPool::new(n, &[(path, source)])` which spawns `n` OS threads, each constructing its own `BatchJsRuntime` (rquickjs `Runtime` + `Context`) and loading every script. Communication is `std::sync::mpsc` channels with owned `Vec<f32>` job/result payloads — `rquickjs::Runtime` is `!Send`, but each runtime is constructed *on* its worker thread and never crosses a thread boundary, so there's no Send constraint to fight. `run_batch_scripts` in `ecs/systems.rs` now takes `Option<&BatchJsWorkerPool>`; when present, JS-lang batches partition their column data evenly across workers and dispatch in parallel. Lua continues single-VM (mlua workers deferred — see below).

CLI: `--batch-workers N` (default `0` = original single-VM path; `>=1` = construct pool of that size). 3 unit tests verify byte-identical output vs single-VM at N=1,2,4.

### Headline numbers (30k entities, 600 frames headless, seed 0xcafebabe, dt=1/60)

| variant                          | frame_ms.avg | scripts.batch (sum, ms) | wall savings vs control |
|----------------------------------|--------------|-------------------------|-------------------------|
| `30k_js_batch_workers0` (control) | 10.93        | 5.56                    | —                       |
| `30k_js_batch_workers1`           | 11.26        | 5.64                    | **−0.33 ms** (regression) |
| `30k_js_batch_workers2`           | 8.57         | 5.98                    | +2.36 ms                |
| `30k_js_batch_workers4`           | 7.28         | 6.67                    | +3.65 ms                |
| `30k_js_batch_workers8`           | 6.58         | 7.58                    | +4.35 ms                |

Baselines at `docs/baselines/step4c-walking-skeleton-workers/`.

### Lessons

1. **Wall savings scale near-linearly to physical core count.** At 30k entities the parallel section's wall time drops from 5.56 ms (control) → 3.20 ms (N=2) → 1.91 ms (N=4) → 1.21 ms (N=8). Speedup factors: 1.74×, 2.91×, 4.59×. Sub-linear because (a) each worker pays its own fixed FFI overhead, (b) channel send/recv adds ~30 µs per round-trip, (c) on Windows the OS thread scheduler is not perfectly fair. Even with these losses, **a 30k-entity JS batch system that took 11 ms can be brought below 7 ms with 8 workers** — meaningful for any project hitting the 60-fps frame budget.

2. **SUM(run_us) GROWS with worker count** — not what you'd expect from "parallel = faster". Total CPU time goes from 5 558 µs (N=0) → 7 580 µs (N=8), a **36% overhead at N=8**. Sources, ranked by likely contribution:
   - Each chunk allocates its own four `Float32Array`s in its own QuickJS heap. With N=8 you're doing 8× the typed-array allocations per frame.
   - Each chunk pays the per-call `Function::call` FFI cost (~20-50 µs). N=8 = 8 calls per frame × per-system instead of 1.
   - The host-side `Vec<f32>` slice copies into the job payload and back from the result are O(chunk_size); they sum to 1× the total data either way, but the channel send/recv copies the `Vec` headers across thread boundaries, costing a few µs each.
   - Cache-locality losses: each worker thread owns a separate VM heap; on a multi-core machine with separate L1/L2, the same work touches more total cache lines than a single-threaded run would.
   The lesson: **parallelism trades CPU efficiency for wall-clock speed**. On a power-constrained device (laptop, mobile), N=2 is probably the sweet spot. On a desktop with idle cores, N=4-8 wins.

3. **N=1 pool is a small regression vs single-VM** (11.26 ms vs 10.93 ms — 0.33 ms slower). The pool overhead (channel send + thread context switch + result recv) is real, and at N=1 there's no parallelism benefit to amortize it against. **Default `--batch-workers 0`** (single-VM path) is correct; N=1 should be reserved for parity tests, never recommended in production.

4. **Determinism preserved by chunk-id-ordered merge.** Workers run in parallel but the main thread `recv()`s their results in worker-id order and copies back into `position_x[lo..hi]` / `position_y[lo..hi]` slices indexed by chunk position. The output is byte-identical to single-VM regardless of N. Verified by `pool_matches_single_vm_for_n_workers` test using strict `assert_eq!` on the f32 result vectors.

5. **`Vec<f32>` payloads are good enough for this workload.** A more cache-friendly design would use `Arc<Vec<f32>>` for read-only inputs (`vx`, `vy`) so the worker doesn't need its own copy, but the test data shows the per-frame `to_vec()` clones aren't even on the hot-path radar (cp_in stays at ~50-200 µs even at N=8). Premature optimization avoided.

6. **Lua workers are deferred.** Two reasons: (a) `mlua::Lua` is `!Send` by default; the `send` feature would have to be enabled in the workspace, and that has performance trade-offs we haven't measured. (b) The `BatchLuaRuntime` per-script env table machinery and the buffer-mode `F32Buffer` userdata both use `Rc<RefCell<...>>`, which is `!Send`; making them `Send` (via `Arc<Mutex<...>>`) would slow down the single-VM hot path. Lua single-VM table mode already runs 30k entities in 3.64 ms total — well under the parallel JS pool's wall time at any N. There's no clear win in adding Lua workers until per-script work grows substantially (Step 5 broader API, larger systems).

7. **Pool memory cost (informational, not measured this slice).** Each worker holds an independent `Runtime` + `Context` + parsed scripts. From the rquickjs docs, a fresh QuickJS runtime is ~50-100 KB; loaded scripts are tens of KB each. At N=8 with the walking-skeleton mover, that's roughly 1 MB of pool overhead. Acceptable for now; needs to be tracked as a counter (`script_worker_memory_bytes`) before recommending high N values for memory-constrained targets.

### What's next

- **Skip Slice 4C.2 (Lua workers)** until a real project demonstrates Lua batch scripts taking >5 ms wall time. Single-VM Luau is already fast enough.
- **Default remains `--batch-workers 0`** in bench mode and runtime config. Document N=2-4 as the recommended setting for projects with >5k JS-batch entities; N=8+ for desktop targets with idle cores.
- **Move to Step 5 (broaden batch API)**: typed state columns, multiple read/write components, optional reads, queued events, dirty masks. The walking-skeleton's fixed Velocity2D→Position2D contract is the only thing keeping the pool implementation small; broader specs need broader job payloads. Worth designing the job shape against a real second batch script (e.g. flocking or particle steering) before locking it in.
- **Add `script_worker_memory_bytes` counter** as part of Step 5 instrumentation. Surface in the editor profiler so users can see the cost before bumping N.

## Step 5 Slice 5.1 (2026-04-28) — Typed local state columns

Slice 5.1 added per-entity typed state to batch JS scripts. Scripts declare their state schema in the system header alongside read/write:

```js
const system = {
  query: {
    read:  ["Velocity2D"],
    write: ["Position2D"],
    state: { acc: { type: "f32", default: 0.0 } }
  }
};
function update(ctx, q) {
  const acc = q.state.acc;          // Float32Array, length q.count
  const vx  = q.read.Velocity2D.x;  // unchanged
  for (let i = 0; i < q.count; i++) acc[i] += ctx.dt * vx[i];
}
```

State values round-trip through `BatchScriptComponent.state: Vec<f32>` (new field, `#[serde(default, skip_serializing_if = "Vec::is_empty")]` keeps existing scenes byte-identical on save). The host parses the schema once at script load, sorts fields alphabetically by name for stable on-disk layout, and threads a `&mut [Vec<f32>]` (one column per field) through the runtime + worker pool the same way it already threads `px`/`py`. This slice is JS-only; Lua scripts that declare state get a one-time warn-and-ignore (Slice 5.1b will mirror).

### Lessons

1. **State schema sort order is part of the on-disk contract.** `BatchScriptComponent.state` is `Vec<f32>` indexed positionally, so field N's slot in the vec depends on the schema's declared order. Reorder the JS object literal and you've silently scrambled every existing entity's state. Sorting alphabetically at parse time means the order is determined by *field name*, not by source-text position — adding a new field rearranges existing slots, but at least the rearrangement is predictable and rename-driven instead of edit-position-driven. A future migration system (Step 5.x) will likely want explicit field IDs anyway, but for the walking skeleton the sorted-by-name convention is the cheapest safe default.

2. **Lazy default-init in the dispatcher catches three real cases for free.** When the dispatcher sees `batch.state.len() != schema.len()`, it resets to `schema.iter().map(|f| f.default).collect()`. This handles (a) freshly spawned entities (component default is `Vec::new()`), (b) live script reload that changed the schema, and (c) scenes saved before Slice 5.1 (no `state` field → defaults to `Vec::new()` via serde). One branch, three back-compat scenarios. Important: it's lossy — a schema change *will* erase existing state. Migration tooling is a future concern; for the walking skeleton, "reset on shape mismatch" is the right behaviour.

3. **Worker pool needs a separate schema cache.** The pool dispatcher is on the main thread, but the script-parsing happens *inside* each worker's `BatchJsRuntime`. To know how many state columns to allocate for a job, the dispatcher would otherwise need a round-trip through the channel. Solution: at `BatchJsWorkerPool::new`, construct one transient `BatchJsRuntime` on the main thread, load all scripts into it, extract `state_schema` for each, store as `HashMap<String, Vec<BatchStateField>>`, drop the probe. Cost: one extra runtime construction + script parse at startup (one-time, milliseconds). Benefit: zero per-frame overhead for schema lookup. Workers still parse independently and remain authoritative for their own runs.

4. **Test scripts must avoid chunk-local indices when verifying determinism across worker counts.** First version of `pool_state_columns_match_single_vm` used `acc[i] += ctx.dt * (i + 1)` and failed: with N=4 workers and 64 entities, each chunk sees `i = 0..15`, so the accumulators diverged from the single-VM `i = 0..63`. The script was wrong, not the implementation. Real per-entity state should depend on entity-local data (component values), not on the chunk-local index. Updated to `acc[i] += ctx.dt * vx[i]` and the test passes. Worth flagging in `docs/scripting-api.md`: the index `i` inside `update` is **chunk-local, not entity-id**, and scripts that depend on a stable per-entity ordering need to read it from a component.

5. **Counter accounting was almost an off-by-one trap.** Pre-5.1 the per-batch counters multiplied by 4 (for vx/vy/px/py) for `cp_in_bytes` and 2 (for px/py) for `cp_out_bytes`/`dirty_bytes`. With N state columns, those become `(4 + N)` and `(2 + N)` respectively — easy to forget when state is a "new feature" rather than a "new I/O column". Caught it during the signature change pass; future broader-schema work (multiple reads/writes) will need the same multiplier discipline.

6. **The `Persistent<Function>`-adjacent state handles must outlive the call.** The runtime keeps `Vec<Persistent<Object>>` for each frame's state arrays so it can read them back after `update` returns. Dropping them inside the call's `with` closure would invalidate the typed-array views the script writes to. Same gotcha as the `q.read/q.write` arrays from Step 4 — solution copy-pasted: collect handles into a `Vec` declared outside the closure, use `clone()` to put one set on `q.state` and keep one set for copy-out.

7. **Schema field types are deliberately f32-only this slice.** The parse path warns and skips non-f32 fields rather than erroring — easier to extend later (i32, bool, vec2 by packing into 2 columns) without breaking existing scripts that only use `f32`. The component vec is `Vec<f32>` and would need to become `Vec<u8>` (with per-field offset tables) or `enum BatchStateValue { F32(f32), I32(i32), ... }` to support mixed types. Both options have their own trade-offs and need a real second-script driver before committing.

### What's next

- **~~Slice 5.1b: Lua state mirror.~~** ✅ Landed 2026-04-29 — see "Step 5 Slice 5.1b" section below.
- **Slice 5.2 candidates (pick one based on real-project need):**
  - Multiple reads/writes (scripts that touch >1 component per side).
  - Optional reads (`q.read_optional.X` returns `null`-or-Float32Array, scripts handle absence).
  - Queued spawn/despawn requests (`q.spawn_requests.push({...})`).
  - Additional state types (i32, bool — needs design for component layout).
- **Defer until needed:** dirty masks, per-archetype chunking, structural-change deferred application.

## Step 5 Slice 5.1b (2026-04-29) — Lua state mirror

Slice 5.1b extended typed local state to the Lua batch runtime, closing the JS/Lua parity gap from Slice 5.1. Lua scripts now declare the same `system.query.state` schema, and the host exposes `q.state.<name>` per entity column using whichever buffer mode (`Table` / `UserData` / `Buffer`) the runtime was configured with — exactly mirroring how `q.write.Position2D.x` is exposed.

### Lessons

1. **Mode-dispatched build helper >> three copy-paste arms.** The first instinct was to inline state-column construction into each `BatchLuaBufferMode` arm of the existing `(q, write_handles) = match …` block. That would have meant three near-identical 10-line stanzas inside an already-long function. Pulling the work into a single `build_state_columns(lua, mode, schema, state, &q, &err)` helper that returns a `StateHandles` enum keeps the per-mode complexity in one place and makes the call site one line. The `WriteHandles` enum stays untouched (good — it's already proven), and the parallel `StateHandles` enum is intentional symmetry rather than a refactor of the original.

2. **`StateHandles` enum mirrors `WriteHandles` one-for-one — and that's worth the duplication.** Both enums carry "host-side handles to script-visible buffers" but for different roles (write columns vs state columns). Trying to fold them into a single `Handles { write_x, write_y, state: Vec<…> }` struct would have required either `Option`-wrapping (because `Buffer` mode handles are a different type than `Table` mode handles) or a generic-over-mode design. Two parallel enums with identical variants is uglier on paper but reads better at the call sites — copy-out is `match write_handles { … }; match state_handles { … }` with the same shape twice.

3. **`LuaBuffer::clone()` is cheap but the safety story isn't obvious.** Like `Persistent<Object>` in QuickJS, mlua's `Buffer` is a `ValueRef` that refers to the underlying Luau buffer; cloning it is a refcount bump, not a memcpy. The host can hand one clone to the script via `state_tbl.set(name, buf)` and keep another for `copy_buffer_into_f32_slice` after `update` returns. Same pattern as Step 4's read/write columns — copy-pasted intentionally.

4. **Counter accounting carried over verbatim from JS.** `bytes_per_col * (4 + n_state)` for in, `(2 + n_state)` for out/dirty. The `bytes_per_col` recomputation (`count * size_of::<f32>()` instead of the JS path's `size_of_val(velocity_x)`) is a stylistic difference, not a semantic one — both produce the same number. Worth keeping the formulas literally identical so a future "compare JS vs Lua per-batch byte budget" benchmark is meaningful.

5. **Schema-parsing path is symmetric to the JS one — but the `mlua` API forced a slightly different shape.** `parse_state_schema` in batch_lua.rs iterates `state_tbl.pairs::<String, LuaTable>()`, which auto-rejects entries whose key isn't a string (rare but possible from a misconfigured script). The JS path didn't need this guard because object keys are always strings. Both paths sort alphabetically by name afterwards so the on-disk `Vec<f32>` layout is binary-compatible across languages — switching a script from Lua to JS (or vice versa) preserves accumulated state.

6. **The dispatcher change was a 4-line edit.** Lifting `state_schema` lookup to cover both languages amounted to: (a) replace `ScriptLang::Lua => Vec::new()` with `ScriptLang::Lua => lua_runtime.and_then(|rt| rt.state_schema(&path))…`; and (b) add `&mut state_cols,` to the Lua arm's call. The existing gather/scatter against `BatchScriptComponent.state` already worked because it's gated on `n_state > 0` — Slice 5.1 had that abstraction right.

7. **Test harness factored to one helper across three modes.** `run_state_round_trip(mode, body)` constructs a runtime, loads a script with the given body, runs it, and asserts the round-trip. The body is mode-specific (Table/UserData use 1-indexed `q.state.t[i]`, Buffer uses `buffer.readf32`/`writef32` with byte offsets) but the harness is shared. This catches "wrote state correctly in Table mode but forgot the byte-offset arithmetic in Buffer mode" with one set of expected values across three tests.

### What's next

- **Slice 5.2 candidates** unchanged from Slice 5.1's "What's next".
- A future cross-language perf bench (JS Float32Array vs Lua Buffer vs Lua F32Buffer userdata vs Lua table) would be useful — the byte budgets are now directly comparable.

## Step 5 Slice 5.2 (2026-04-28) — Optional component reads

Slice 5.2 added `read_optional` to the batch script schema. A script can now declare components it *may* use without committing every entity to having them:

```js
const system = {
    query: {
        read:           ["Velocity2D"],
        write:          ["Position2D"],
        read_optional:  ["Acceleration2D"],
    },
};

function update(ctx, q) {
    const a = q.read_optional.Acceleration2D;       // object or null
    if (a) { /* read a.x[i], a.y[i] */ } else { /* skip */ }
}
```

The dispatcher partitions a script's matched entities by archetype before each VM call so the inner loop sees a uniform population: every entity in a given call has every listed optional component, or none do. Walking-skeleton scope: only `Acceleration2D` is accepted; unknown component names fail at load via `BatchError::SpecMismatch`.

### Lessons

1. **Per-batch all-or-nothing partition keeps the inner loop monomorphic.** The alternative — exposing `q.read_optional.X` as a sparse view with per-entity null checks — would have forced a branch inside every iteration and (worse) made the JS engine deoptimize the tight loop to handle the polymorphism. By bucketing entities into "with-accel" and "without-accel" before each call, the script only branches **once per batch** on `if (a)` and then runs a clean monomorphic loop. The bench numbers confirm this is essentially free overhead: at 30k entities, JS scripts.batch goes 6.18ms (0%) → 7.20ms (50%) → 8.15ms (100%), almost perfectly linear in the *amount of accel-using work* with no inflection at the partition boundary.

2. **Two passes per frame, but the per-pass cost is what you'd expect.** When `wants_accel` is true, the dispatcher runs once for the with-accel bucket (gather + script + scatter, with optional columns built) and once for the without-accel bucket (same but with `accel = None`). At 50% split this means ~15k entities per pass, but the *per-entity* cost is unchanged — gather/scatter are linear and the script's inner loop runtime scales with `q.count`. Total batch ms ≈ (with_accel_count / total) × cost_with + (without_accel_count / total) × cost_without; the partition itself is a single `iter().partition()` over entities and doesn't show up in profiles.

3. **Race protection in the gather pass is mandatory, not paranoid.** Between `entities.partition(|e| world.get::<&Acceleration2D>(*e).is_ok())` and the actual gather of `(ax, ay)` columns, a script side-effect (or external system) could have removed `Acceleration2D` from an entity in the with-accel bucket. The gather loop re-fetches the component for each entity and skips the entity if it's gone — without that re-check, the column would silently get garbage from `unwrap_or` and the script would integrate against an Acceleration2D that no longer exists. Defensive but cheap (one extra `world.get` per entity per pass; same cost as the partition predicate itself).

4. **JS uses explicit `null`, Lua uses `nil` — and exposing the parent table is worth it for Lua.** In JS, `q.read_optional.Acceleration2D` is always defined and is either an object (when present) or literal `null`; scripts use `if (a)` which handles both with one branch. In Lua, `q.read_optional` is **always a table** (even when no optional components are present) so scripts can safely write `if q.read_optional.Foo then` without first checking that `q.read_optional` itself exists. This gives a uniform probing pattern across both languages, even though the underlying nullability story differs.

5. **All three Lua buffer modes get the same `build_read_optional_columns` helper.** Mirroring Slice 5.1b's `build_state_columns` design, optional column construction is a single helper that dispatches on `BatchLuaBufferMode`. Table mode builds `lua.create_sequence_from(ax)`, UserData mode wraps `Rc<RefCell<Vec<f32>>>` in `F32Buffer`, Buffer mode allocates a Luau buffer and `buffer.writef32`s the values in. The call site is one line; the per-mode complexity stays in one place. Six new tests exercise all three modes individually plus the absent-component path.

6. **`SpecMismatch { read, write, read_optional, .. }` extension was a free win.** The original error variant used `..` in match arms (Slice 4.1 forethought), so adding the new field broke nothing. The walking-skeleton validator just got one more "must equal `[]` or `["Acceleration2D"]`" branch.

7. **Bench `--batch-accel-script` separates "spawn the component" from "run a script that uses it".** Two orthogonal flags: `--spawn-with-accel-pct N` populates Acceleration2D on the first N% of spawned entities (deterministic via seeded RNG order), `--batch-accel-script` rewrites `mover_batch.js` → `mover_batch_accel.js` (similar for Lua Table mode). Combining them is what makes the dispatcher actually do the partition work; either alone is a no-op vs the baseline. Same pattern as `--batch-noop`.

8. **Counter accounting extends naturally.** `bytes_per_col * (4 + n_state + n_opt)` for in, where `n_opt = 2` per optional 2-field component when present, `0` otherwise. Out/dirty unchanged (optional components are read-only). Symmetric across JS and Lua paths, so a future "compare optional-read overhead" bench would have apples-to-apples numbers.

### Baselines (30k entities, 600 frames headless, seed 0xcafebabe, dt=1/60, frame-cap-fps 240)

| Variant | scripts.batch | frame.avg |
|---|---:|---:|
| 30k_js_batch_optaccel_0pct   | 6.18 ms | 11.18 ms |
| 30k_js_batch_optaccel_50pct  | 7.20 ms | 12.56 ms |
| 30k_js_batch_optaccel_100pct | 8.15 ms | 13.85 ms |
| 30k_lua_batch_optaccel_0pct (Table)   | 3.66 ms |  9.11 ms |
| 30k_lua_batch_optaccel_50pct (Table)  | 4.22 ms | 10.08 ms |
| 30k_lua_batch_optaccel_100pct (Table) | 4.73 ms | 10.92 ms |

Linear scaling in both languages: each +50% accel-pct adds ≈+1ms (JS) / ≈+0.55ms (Lua Table). Lua Table is the cheapest mode here partly because the accel script does very simple per-entity work where Lua's table-lookup overhead is amortized; Buffer mode would likely overtake at higher per-entity arithmetic densities (deferred until a real script needs it).

### What's next

- **Slice 5.3 candidates (no clear winner yet — pick when a real project demands one):**
  - Multiple reads/writes in `read` / `write` (>1 component per side).
  - Multiple optional reads (`read_optional` already supports this in shape; extend the validator beyond the walking-skeleton restriction once a real script needs it).
  - Queued spawn/despawn requests (`q.spawn_requests.push({...})`).
  - Typed events emitted from batch (`q.emit.hit_count++; q.emit.hit_target[j] = …`).
  - Additional state types (i32, bool — needs design for component layout).
- **Defer until needed:** dirty masks, per-archetype chunking (already implicit with Slice 5.2's partition), structural-change deferred application.

## Step 5 Slice 5.3 (2026-04-28) — Multiple read/write columns

Slice 5.3 lifted the walking-skeleton restriction (`read=["Velocity2D"], write=["Position2D"]`) and made `BatchSystemSpec.read` / `.write` arbitrary-length lists drawn from an 8-component vocabulary: `Position2D`, `Velocity2D`, `Acceleration2D`, `Drag2D`, `Force2D`, `Wind2D`, `Impulse2D`, `Torque2D`. New `MAX_BATCH_COLUMNS = 12` cap applies to the sum of `read + write + read_optional`. Two new errors: `TooManyColumns` and `DuplicateComponent`. The dispatcher in `ecs::systems::run_batch_scripts` is now generic over component name — a gather/scatter closure pair per vocab type, `BatchReadColumn` / `BatchWriteColumn` / `BatchOptionalColumn` slices passed into both runtimes (no runtime change to `BatchJsRuntime::run_batch` or `BatchLuaRuntime::run_batch` was needed; they already iterated columns).

Bench harness gained `--batch-shape {2r_1w|3r_2w|5r_3w}` plus `--batch-shape-noop`. `attach_batch_shape_components` populates the missing components with bounded random values (Acceleration ±30, Drag 0..0.5, Wind ±10, Force ±5, Impulse ±2, Torque ±1). `rewrite_batch_paths_to_shape` preserves the trailing `_table`/`_userdata`/`_buffer` mode suffix from the input basename so each Lua mode gets its own script. 24 mover scripts authored under `projects/scripting-perf/assets/scripts/`: 3 JS real + 3 JS noop + 9 Lua real + 9 Lua noop.

### Headline numbers (10k entities, 300 frames headless, seed 1, dt=1/60, real - noop body cost)

`batch_run_us` (script body only, host marshalling stripped):

| shape | js | lua_table | lua_userdata | lua_buffer |
|-------|---:|----------:|-------------:|-----------:|
| 2r/1w | 2.79 ms | 0.37 ms | 5.74 ms | 4.12 ms |
| 3r/2w | 5.26 ms | 0.63 ms | 10.10 ms | 7.89 ms |
| 5r/3w | 8.35 ms | 1.05 ms | 19.30 ms | 13.88 ms |

Total batch time (`cp_in + run + cp_out`, real body) for the 5r/3w shape at 10k:
**lua_table 3.89 ms < js 8.45 ms < lua_buffer 14.00 ms < lua_userdata 19.54 ms.**

Counter math validated: bytes scale linearly across the 1k/5k/10k sweep. For 5r/3w specifically: 64/24 KiB at 1k → 312/117 KiB at 5k → 625/234 KiB at 10k (per-frame in/out). All 72 baselines saved under `docs/baselines/step5-slice5.3/`.

### Lessons

1. **Lua table mode JITs better than Lua buffer mode for column-style workloads.** This is the *opposite* of what we expected after Slice 4B.2 (where buffer mode crushed table mode for the simple v→p integrator). At 1k entities the buffer mode is already 12× slower than table mode at 2r/1w (523 µs vs 43 µs). At 10k/5r3w the gap is ~13×. The difference: `buffer.readf32(buf, off)` is a host call into Rust per element. With a tiny script body (one multiply-add), Rust's call overhead dominates. With many reads per iteration (5r/3w = 10 reads + 6 writes per element), it dominates even more. Index-table iteration `tbl[i]` is the single Luau opcode the JIT optimizes hardest. **Recommendation: default Lua batch scripts to `Table` mode unless profiling shows the cp_in/cp_out tax exceeds ~30% of `scripts.batch`.** Buffer mode wins only when the script has very few accesses per element (so the host-call cost is amortized) or when avoiding the per-frame `create_sequence_from` allocation matters more than per-element speed.

2. **Luau loops beat QuickJS by 3–8× when given plain indexed tables.** Across all three shapes, lua_table run time is ~5× lower than js run time for the same body. This is the same headline as Slice 4B.1 (~8× at 30k), now reproduced across multiple shapes.

3. **Marshalling tax (`cp_in + cp_out`) is the hidden cost of Lua table mode.** At 10k/5r3w the table-mode total is `2.24 + 1.05 + 1.60 = 4.89` ms, of which only 1.05 ms is the script body — 78% of the wall time is `create_sequence_from` round-tripping floats. JS pays ~0.09 ms for the same marshalling because Float32Array views are zero-copy. **The next big lever for Lua isn't a faster runtime, it's killing `create_sequence_from` overhead** — either by switching the dispatcher to write directly into pre-allocated Lua tables (reuse across frames) or by accepting the per-element host-call cost of buffer mode and amortizing it via cheaper buffer init.

4. **Counter math is the contract.** With three shapes × three buffer modes × ±noop variants in flight, the only way to validate "did the script *actually* see all the columns" is to assert `batch_copy_in_bytes` and `batch_copy_out_bytes` against the expected formula `(n_read + n_write + n_optional_present) * 2 * n_entities * 4`. The contract test `run_batch_3r_2w_roundtrip` codifies this.

5. **Snapshot writes BEFORE dropping the source borrow.** When the dispatcher iterated a `Vec<BatchWriteColumn>` borrowed from `write_cols.iter_mut()`, then tried to scatter via `write_cols.into_iter()`, rustc bounced it (E0505 — moved borrowed value). Fix: materialize `Vec<(name, Vec<f32>, Vec<f32>)>` from the `writes` slice in place, then drop the borrows, then scatter. Tiny refactor pattern that's worth remembering when bridging "borrowed view of column data" → "owned data for ECS update".

### What's next

- **Slice 5.4 (dirty masks)** is now well-motivated by the Lua table marshalling cost — but only if `cp_out` is the bottleneck; here it's *less* than `cp_in` (1.60 ms vs 2.24 ms), so the win would be smaller than expected. Reusing pre-allocated Lua tables across frames (skip `create_sequence_from` entirely) would attack `cp_in` directly, which is the bigger half.
- **Step 4C parallel workers** are appealing for shapes ≥ 3r/2w where total wall-time exceeds 5 ms — splitting across 4 workers could land 5r/3w lua_table around 1 ms total instead of 4 ms. But Slice 4C.1 already demonstrated the JS pool; the open question is whether sharing one Luau VM across workers is sound (probably needs one VM per worker).
- **Defer:** typed events from batch, queued spawn/despawn (no real-project demand yet).



## Step 5 Slice 5.4a (2026-04-29) — Reused Lua handles across frames

Slice 5.4a attacks the marshalling tax flagged in Slice 5.3 (lua_table at 10k/5r3w spent 78% of its wall time round-tripping floats through `create_sequence_from`). The fix: cache the per-script `q` / `ctx` wrapper tables and the per-mode column storage handles inside the `BatchLuaRuntime`, rebuild only when the script reloads, and grow column storage in place when the batch size increases.

### Headline numbers (apples-to-apples rerun, 300 frames @ seed=1, headless, 10k entities)

`batch_copy_in_us` / frame, before vs after slice 5.4a:

| backend | shape | 5.3 (no-cache) | 5.4a (cache) | delta |
|---|---|---:|---:|---:|
| lua_table    | 5r/3w | 4321 µs | 2348 µs | **-1973 µs (-46%)** |
| lua_table    | 3r/2w | 2905 µs | 1454 µs | **-1451 µs (-50%)** |
| lua_table    | 2r/1w | 1422 µs |  763 µs | **- 659 µs (-46%)** |
| lua_userdata | 5r/3w |  222 µs |   51 µs | **- 171 µs (-77%)** |
| lua_userdata | 3r/2w |  122 µs |   32 µs | **-  90 µs (-74%)** |
| lua_userdata | 2r/1w |   72 µs |   16 µs | **-  56 µs (-78%)** |
| lua_buffer   | 5r/3w |  125 µs |   54 µs | **-  71 µs (-57%)** |
| lua_buffer   | 3r/2w |   77 µs |   28 µs | **-  49 µs (-64%)** |
| lua_buffer   | 2r/1w |   63 µs |   17 µs | **-  46 µs (-73%)** |
| js (control) | any   | unchanged within ±10 µs noise band | | |

`batch_run_us` and `batch_copy_out_us` are unchanged within noise on every config — the cache touches neither the script body nor write-back. `frame_ms.avg` shifts track `cp_in`: lua_table 10k 5r/3w improves 14.52 → 12.75 ms (-1.77 ms); lua_userdata 5r/3w stays flat because `run_us` (~19 ms) dominates and the cp_in win is too small to move the wall clock.

Full 36-row diff: `docs/baselines/slice5.4a-vs-5.3-rerun-diff.txt`. Rerun baselines under `docs/baselines/step5-slice5.{3,4a}-rerun/`.

### Lessons

1. **`create_sequence_from` is faster than a `raw_set` loop in mlua/Luau, even when the destination table is reused.** First implementation of Table mode caching kept the inner `.x` / `.y` column tables alive and refilled them with `tbl.raw_set(i, val)` per element. Result: cp_in *got worse*, not better — the per-call FFI hop into mlua dominates. Switching to "cache the outer wrappers, rebuild inner sequences per call via `create_sequence_from`" gave the -46% headline above. The Luau API is built around batch transfers; using it that way costs one Rust→Lua hop per column instead of one per element. **General lesson: in mlua, prefer one bulk call over many small ones, even if it allocates.** UserData and Buffer modes don't have this issue because they cache the storage handle directly (one userdata or one Luau buffer); growth is in-place via `Vec<f32>::resize`.
2. **When adding an A/B toggle around a timed window, verify which side of the timer the toggled work falls on.** First implementation of `JENGINE_BATCH_LUA_NO_CACHE` reset the cache to `None` at end-of-frame; the rebuild then ran *outside* the `cp_in` start instant on the next frame. Smoke test showed 5.3-rerun (no-cache) cp_in *lower* than 5.4a (cache) — the diff was inverted because the rebuild cost was leaking into untimed code. Fix: move the cache-reset and lazy-init *inside* the `let copy_in_start = Instant::now()` window. Smoke-test the toggle by comparing two configs with known expected delta before kicking off a long capture.
3. **Apples-to-apples means back-to-back on the same machine state.** First diff of slice 5.4a vs the original slice 5.3 baselines showed *every* config (including untouched JS) regressing in `frame_ms.avg`, because the two captures ran days apart with different background load. Solution: alternate the two configs in the same loop (5.3 then 5.4a per shape × backend × N), so noise drift averages across the diff. The JS rows then become a per-row control — if JS sits in a ±10 µs noise band, the Lua deltas are signal. Without that control we'd have shipped the wrong claim.
4. **Cache only ever grows.** A one-frame spike to N=10k keeps 10k worth of column storage alive forever. Steady-state allocations after warmup: zero. Accepted trade-off — batch script populations are typically monotonic-ish, and the alternative (shrink heuristics) is a knob without a real-project signal yet.

### What's next

- **Slice 5.4b (dirty masks for `cp_out`).** `cp_out` is now the next-largest marshalling cost (lua_table 10k/5r3w: 1359 µs). Skip writeback for entities whose write columns are bit-identical to the snapshot. Likely savings: ~50% on `cp_out` if scripts touch ~half of entities per frame; less if everything moves every frame.
- **Slice 5.5 (Luau worker pool).** At 10k/5r3w lua_table, total batch wall time is ~3.95 ms (cache enabled). Splitting across 4 workers could land it under 1 ms. Open question: one Luau VM per worker means the per-script env table machinery duplicates per worker; needs a sharing strategy for read-only state.
- **Defer:** dirty masks for `cp_in` (would require change tracking in the source ECS components, larger refactor).

## Step 5 Slice 5.4b (2026-04-29) — Dirty masks for `cp_out`

**Goal.** Skip ECS writebacks for write columns the script left unchanged. Both Lua (all 3 buffer modes) and JS (slice-5.3 generic `run_batch` path) get dirty-aware copy-out. The walking-skeleton (slice 4.x) JS path is intentionally left conservative — `batch_lua.rs` documents why.

**Granularity.** Per-mode, intentionally asymmetric:

- **Lua Table mode** — per-element bit-equality compare *inside the existing copy_out loop*. The loop already does one `raw_get` per slot, so the compare and conditional store are essentially free. Granularity = 4 bytes.
- **Lua UserData / Buffer + JS** — whole-column slice `==` followed by conditional `copy_from_slice`. One memcmp replaces one memcpy when the column is clean. Granularity = `count × 4` bytes (full axis).

**Counter semantics.** `batch_dirty_bytes` now means "bytes actually written back, post-skip". `batch_copy_out_bytes` keeps its meaning of "would-be writeback budget". Their ratio directly reveals what fraction of the writeback was elided. NaN columns are always reported dirty (sound, conservative — `NaN == NaN` is false). `-0.0` and `+0.0` stay distinct (bit-equality, not float-equality).

**A/B toggle.** `JENGINE_BATCH_DIRTY_DISABLE=1` short-circuits the compare and always copies. Cached per-process via `OnceLock<bool>` so both sides of the A/B pay an identical relaxed atomic load — the cost is symmetric and inside the timed `cp_out` window. Same lesson 5.4a #2 applies: every per-frame toggle must live inside the timer, not outside.

**Validation.** 6 new unit tests (noop + partial-write × 3 buffer modes); all 49 batch tests green. 144-config back-to-back A/B capture (`docs/baselines/step5-slice5.{4a-rerun2,4b-rerun}/`) toggling `JENGINE_BATCH_DIRTY_DISABLE` per pair.

**Counter results (correctness ✓).** Every noop variant collapses `batch_dirty_bytes` from `N/N` to `0/N` exactly as designed:

| 10k 5r3w noop  | dirty/budget bytes |
|----------------|--------------------|
| js             | `0 / 240000`       |
| lua_table      | `0 / 240000`       |
| lua_userdata   | `0 / 240000`       |
| lua_buffer     | `0 / 240000`       |

**Wall-clock results (no measurable speedup ✗).** Both always-update and noop synthetic show `cp_out_us` deltas inside ±20 µs noise on every config — and on the noop synthetic the cp_out *increases* by ~10–30 µs at 10k entities. The compare cost equals the elided copy cost.

| 10k 5r3w | 5.4a `cp_out_us` | 5.4b `cp_out_us` | delta   |
|----------|------------------|------------------|---------|
| js noop          | 6   | 23  | +17 µs |
| lua_userdata noop | 7  | 28  | +21 µs |
| lua_buffer noop  | 13  | 33  | +20 µs |
| lua_table real    | 1312 | 1340 | +28 µs (noise) |

**Empirical lesson.** At these slice sizes (≤960 KB/frame on the largest config), `[f32]::eq` (compiles to a SIMD memcmp) is *not* faster than `copy_from_slice` (compiles to a SIMD memcpy) on x86. Both are memory-bandwidth-bound and equally vectorised. Skipping a copy doesn't save time when the compare costs the same.

**So why ship it?** Because the **counter visibility** is the actual deliverable. Three downstream wins become possible only because we now know which columns were left clean:

1. **ECS change-detection / observers.** Future `ChangeTracking<T>` queries can flip a "modified-this-frame" bit only when the script's writeback was non-empty.
2. **Network replication / save streaming.** Skip serializing components whose script left them identical.
3. **Render-side dirty culling.** `Transform → matrix recompute` can short-circuit when a Position2D writeback was elided.

None of these are currently consumers, so the slice ships without a wall-clock win — but with the counter plumbing that the next slice can leverage. The cp_out memcpy cost is being paid twice in 5.4b (once as memcmp during the compare, once as memcpy when dirty); but on the cleanest workloads (noop) we pay only the memcmp. The break-even depends on hit rate: at 50% dirty columns, total work ≈ 1.5× memcpy = roughly equal (memcpy is ~2× slower than memcmp on noisy real workloads).

**Lessons added.**

1. **A vectorised memcmp ≈ a vectorised memcpy on x86.** "Skip the copy" is a real optimisation only when the copy is genuinely expensive (much larger working sets, cache-cold pages, NUMA hops). For the in-cache slices we're shoving around (≤1 MB), don't expect raw-bandwidth wins from a `==` short-circuit. Plan for the win to come from the *consumer* of the dirty signal, not the elision itself.
2. **Counter semantics deserve their own design pass.** `batch_dirty_bytes == batch_copy_out_bytes` reads natural ("everything dirty") but is wrong if you want to query the elision ratio. We split: `dirty_bytes` = post-skip actuals; `copy_out_bytes` = pre-skip budget. Their delta is the slice's primary KPI. Document it next to the counter definition (we did) so future readers don't conflate them.
3. **The A/B toggle still belongs inside the timer.** Lesson 5.4a #2 generalises: every per-frame conditional we add for instrumentation/parity must run inside the timed window, with both branches paying the same lookup cost. A `OnceLock<bool>` lookup per call is one relaxed atomic load — symmetric, and inside the `cp_out_start..elapsed` bracket on both sides of the A/B.
4. **Synthetic noop is a correctness fixture, not a perf showcase.** `mover_batch_*_noop_*.lua/js` exists precisely to isolate marshalling cost; expecting it to demonstrate the *win* of an elision conflates two things. Real wins from elision will only show up against a workload that ALSO has expensive downstream work (observers, replication, etc.) that the elision can short-circuit. We don't have one yet.

### What's next (after 5.4b)

- **Slice 5.5 (Luau worker pool).** Same delta as before — total batch wall time at 10k/5r3w lua_table is still ~3.95 ms post-5.4b; splitting across 4 workers remains the highest-leverage knob. Open question (unchanged): per-VM env duplication.
- **Slice 5.4c (downstream change-detection consumer).** Wire one of the three potential consumers (observers, replication, render dirty culling) so the dirty counter has a downstream that pays the elision dividend.
- **Defer:** dirty masks for `cp_in` (still requires change tracking in source ECS components).

## Step 5 Slice 5.4c (2026-04-30) — Dispatcher consumes the dirty mask

Slice 5.4b made the runtime *know* which write columns the script left clean, but the dispatcher in `run_batch_scripts` (`crates/jengine-core/src/ecs/systems.rs`) didn't *care*: after every batch it iterated `scatter_data × order` and called `world.get::<&mut T>(entity)` for every (entity, write-column) pair, unconditionally. For a 10k-entity batch with 3 write columns that's 30 000 hecs lookups per frame — paid even when the script wrote nothing.

5.4c plumbs a per-column dirty bool out of the runtime so the dispatcher can short-circuit the scatter loop entirely for clean columns.

### What changed

- `BatchJsRuntime::run_batch` and `BatchLuaRuntime::run_batch` gained a parameter `write_dirty: &mut [bool]` (between `state` and `counters`). The runtime clears the buffer to `false` on entry, then sets `write_dirty[i] = true` when write column `i`'s x or y axis was modified. Passing `&mut []` opts out (no per-column info recorded; legacy back-compat for older callers / tests).
- `run_batch_scripts` allocates a `Vec<bool>` of length `writes.len()` per dispatch, forwards it to the runtime, then iterates `scatter_data` with `enumerate()`. Clean columns increment a new counter `batch_writeback_scatters_skipped` by `order.len()` and `continue` — skipping every per-entity hecs lookup for that column.
- `FrameCounters::batch_writeback_scatters_skipped: u64` reports the total elided (entity × write-column) slots this frame. Compare against `batch_systems_run × writes_per_system × count` to compute elision rate.

### Test coverage

- 3 new JS unit tests: noop must zero `write_dirty`; partial write must mark only the touched column; empty `&mut []` is a valid opt-out and doesn't panic.
- 6 new Lua unit tests: noop + partial-write × 3 buffer modes (Table, UserData, Buffer).
- All 6 existing Lua test call sites updated to pass `&mut []` for the new parameter.
- Final: jengine-core lib 248/248 tests green; batch suite 58/58 (was 49).

### Lessons added

1. **An optimisation isn't done until its consumer ships.** Slice 5.4b finished the *measurement* (post-skip dirty bytes counter) and the *elision* (memcmp + conditional memcpy inside the runtime), but the dirty info never escaped the runtime's stack frame. The downstream cost it could elide — the dispatcher's per-entity `world.get::<&mut T>` — kept paying full price. 5.4c is the same insight as 5.4a Lesson #4 generalised: ship the producer AND at least one consumer in the same slice, or split them and explicitly schedule the consumer next.
2. **`Vec<bool>` allocations on the per-batch hot path are fine.** `writes.len()` is `≤ 12` in practice (write_schema cardinality). A 12-byte zeroed allocation per dispatch is ~30 ns — well below the noise floor of a batch that does anything useful. Don't pre-optimise this with `SmallVec` unless a profile demands it.
3. **An optional out-channel via `&mut []`-as-opt-out is a clean back-compat escape.** Adding a parameter to a hot trait-shaped API breaks 7+ callers. Sentinel-length-0 lets old tests stay one-line-changed (`&mut []`) while new callers get the info they need. The runtime documents the contract: empty slice ⇒ no per-column info recorded, only the existing global `batch_dirty_bytes` counter is updated. Validated by `debug_assert!(write_dirty.is_empty() || write_dirty.len() == writes.len())`.
4. **`enumerate().continue` beats partition for hot paths.** The clean-column skip is a single branch + counter increment. No `partition_in_place`, no separate "dirty_indices: Vec<usize>" allocation. The dispatcher pays one bool check per (column × frame) regardless of whether it skips or scatters — symmetric, predictable, easy to reason about in the timing sense Lesson 5.4a #2 demands.

### What's next (after 5.4c)

- ~~**Capture A/B baseline.**~~ ✅ Done 2026-04-29 — see "A/B baseline" section below. Median NOOP win −5%, max −14.1%.
- **Slice 5.5 (Luau worker pool).** Still the highest-remaining-leverage knob for full-update workloads where the dispatcher can't elide anything.
- **Defer:** generalising `write_dirty` to per-element granularity at the dispatcher level (would require a `Bitvec` per column and per-element `world.get` short-circuit — only worth it if a future profile shows the per-entity scatter dominating even on partial-dirty workloads).

### A/B baseline (captured 2026-04-29)

300 frames @ seed=1, headless, 144 configs via `JENGINE_BATCH_DIRTY_DISABLE` toggle. Full table: `docs/baselines/slice5.4c-vs-5.4b-summary.txt`.

NOOP workload (where the scatter-skip dividend lives) — consistent wins:

| config                      | 5.4b ms | 5.4c ms | delta   |
|-----------------------------|--------:|--------:|--------:|
| 10k js          5r3w noop  |   7.200 |   6.595 |  −8.4%  |
| 10k js          3r2w noop  |   4.868 |   4.448 |  ∓8.6%  |
| 10k lua_table   5r3w noop  |  11.553 |  10.786 |  −6.6%  |
| 10k lua_buffer  5r3w noop  |   7.413 |   6.796 |  −8.3%  |
| 10k lua_buffer  3r2w noop  |   5.510 |   4.733 | −14.1%  |
|  5k lua_table   3r2w noop  |   4.405 |   3.813 | −13.4%  |
|  5k js          3r2w noop  |   2.916 |   2.572 | −11.8%  |

Median NOOP win: **−5% across 36 configs; max −14.1%.** REAL workload deltas all within ±0.8 ms (noise) — expected, since full-update scripts touch every column every frame and can't elide anything.

**Lesson 5 (the missing piece from 5.4b).** A counter-only optimisation has zero wall-clock value until a downstream consumer pays the elision dividend. 5.4b's noop frame_ms was unchanged from 5.4a (~7.2 ms @ 10k js 5r3w). 5.4c plumbed the same dirty info one level up and the noop dropped to 6.6 ms — a real 8% wall-clock win from a 30-line dispatcher change. The compare/copy elision inside the runtime saved bytes that nobody was actually paying for; the scatter elision saved hecs lookups that everybody was. Lesson: **measure what's expensive at the layer above the one you're optimising, not the layer you're touching.**

## Step 5 Slice 5.5a (2026-04-29) — `BatchScriptPool` wrapper (single-worker spike)

Pure structural slice: introduce the single owner the dispatcher will need before parallelism is on the table. No Send work, no threading — the wrapper owns one JS worker and (lazily) one Lua worker, both type aliases over the existing runtimes. Worker count is hardcoded to `1`; `vm_memory_bytes_per_worker` returns 0. The point is to give 5.5b a fixed surface to grow `Vec<Worker>` into without further dispatcher churn.

### Changes
- New module `crates/jengine-core/src/scripting/worker_pool.rs` exporting `BatchScriptPool`.
- Dispatcher signature: `run_batch_scripts(world, &BatchScriptPool, dt, counters)` (was 5 args, now 4) and explicitly gated on `feature = "native"` (the Lua codepath was always required anyway — the pre-5.5a signature took `Option<&BatchLuaRuntime>` unconditionally).
- Runtime now owns a single `batch_pool: Option<BatchScriptPool>` (was two fields). Constructed conditional-eagerly when the scene actually contains any `BatchScriptComponent`. The empty-JS-runtime fallback hack at the dispatch site is gone.
- `FrameCounters::script_worker_count` (`u32`) and `FrameCounters::vm_memory_bytes_per_worker` (`u64`) — both default to 0; the dispatcher fills them at the top of every batch frame.
- `bench::CountersAvg` mirror updated so the new fields surface in the JSON capture (also added `batch_writeback_scatters_skipped`, which had been missed).

### Numbers
A/B baseline (300 frames @ seed=1, headless, 144 configs) in `docs/baselines/slice5.5a-vs-5.4c-summary.txt`. There's no env-var toggle for 5.5a — it's just current binary vs the 5.4c JSONs. Frame-time deltas are within run-to-run noise: NOOP rows show a mix of small wins (−7 to −40%) and small losses (+0.5 to +7%); REAL rows similar. No systematic regression. The wrapper is free, as designed.

### Lesson 6 — *Land structural slices before behaviour-changing ones.*
The wrapper changes nothing observable at runtime, so the only validation needed is "everything still compiles and tests stay green." Doing this as a standalone slice (rather than folding it into 5.5b's threading work) means when 5.5b breaks, the only suspect is the threading. The 5.4c benchmark history is full of runs where a structural change and a behaviour change shipped together and the bisect cost a day; this avoids that.

## Step 5 Slice 5.5b.1 (2026-04-29) — Channel + dedicated worker thread (N=1)

5.5b.1 wraps the existing `BatchJsRuntime` / `BatchLuaRuntime` in a dedicated worker thread per language with a bounded(16) crossbeam channel + one-shot reply, behind the same `BatchScriptPool` surface added in 5.5a. Two backends ship together: **Direct** (synchronous, main-thread, semantically 5.4c) and **Pooled** (channel-routed). One CLI flag (`--no-batch-pool`) and one constructor (`new_pooled_or_fallback`) decide which one runs; the dispatcher charges the same payload/response/dispatch counters in both modes so the only frame_ms delta isolates the channel itself.

### A/B baseline (captured 2026-04-29)

300 frames @ seed=1, headless, single 30k entity `5r_3w` workload per backend. Full pair under `docs/baselines/step5-slice5.5b.1/`.

| backend     | (a) Direct ms | (b) Pooled ms | channel cost |
|-------------|--------------:|--------------:|-------------:|
| `js`        | 47.754        | 47.459        | **−0.295 ms (−0.62%)** |
| `lua_table` | 36.520        | 35.642        | **−0.878 ms (−2.41%)** |

The channel cost at N=1 is **negative**. The dispatcher is fully synchronous (`send → recv` on every batch), so this isn't parallelism — it's cache locality. Running the script on a separate physical core leaves the main thread's L1/L2 warm for the per-bucket ECS gather and the post-run scatter, both of which are linear hecs lookups that pay heavily for cache misses at 30k entities.

### Why dedicated-worker-thread over mlua `send`

The original 5.5b plan was "flip the mlua `send` Cargo feature, audit `BatchLuaRuntime` for `!Send` interior, ship `Vec<Worker>` directly." That plan died in the first 30 minutes of implementation: enabling `["luau", "send"]` on `mlua = "0.10"` broke compile in 11 unrelated places — every existing `Lua`/`Function`/`Table`/`UserData` API method became `Send`-bounded and our entire script-host trampoline stopped type-checking. The breakage was structural (mlua's send-mode types are a parallel namespace, not a superset), not "fix a few `Rc` to `Arc`," and nothing in the existing test suite would tell us *which* of the now-`Send`-bounded things had genuine `!Send` interior we'd miss until production. We reverted, took a checkpoint, and pivoted.

The pivot — **the `mlua::Lua` never crosses a thread boundary** — is what made this slice cheap. The runtime is *constructed inside* the worker thread (via `init_tx` after spawn). Only `Send + 'static` payloads (`BatchRequest` / `BatchResponse` — both plain f32 column data + `String` schemas) cross the channel. That means we keep the unmodified single-threaded mlua build *and* get one VM per OS thread without any of the audit work the `send` feature would have forced. The Pooled backend literally owns a `BatchJsRuntime` / `BatchLuaRuntime` value the same shape as Direct — it just lives behind a `Sender<BatchRequest>` instead of a `&mut self` reference. When 5.5b.2 grows N>1 workers we just spawn N copies of the same code; nothing about the runtime's thread-affinity assumptions changes.

**Lesson 7.** When a "flip a feature flag" plan blows up, look for the version where the foreign type *never crosses the boundary at all*. Channels carrying owned bytes are almost always cheaper than making the foreign type itself thread-safe — especially when the foreign type is from a vendored C library (Luau, QuickJS) that you don't control. The `send` feature was trying to make the VM safe to *share*; we didn't need shared, we needed *colocated* (one VM, one thread, forever). Channels gave us colocated for one afternoon of work; `send` would have cost a week of audit and probably unrelated regressions.


## Step 5 Slice 5.5b.2 (2026-04-29) — N workers per language, chunked dispatch

5.5b.2 grows the Pooled backend from `Option<Worker>` to `Vec<Worker>` and adds chunked fan-out. The dispatcher block-partitions the entity range across N workers (chunk_size_base + remainder distributed to the first chunks), fans out via a new `BatchScriptWorker::dispatch_run_batch` that returns the bounded(1) reply receiver without blocking, and **drains receivers in chunk-id order** appending writes via `Vec::append` and ORing dirty masks. The merged column is bit-identical to an N=1 run regardless of worker count — verified by the new `pooled_js_n2_chunked_round_trip` test (9 entities → 5+4 split → element-wise compare against precomputed expected). N is resolved by `resolve_batch_workers(cli)`: env `JENGINE_BATCH_WORKERS` > CLI `--batch-workers <N>` (when > 0) > default 1.

### Scaling table (captured 2026-04-29)

300 frames @ seed=1, headless, single 30k entity `5r_3w` workload per backend per N. Full set under `docs/baselines/step5-slice5.5b.2/`.

| backend     | N=1     | N=2            | N=4            | N=8            |
|-------------|--------:|---------------:|---------------:|---------------:|
| `js`        | 46.408  | 35.115 / 1.32× | 29.878 / 1.55× | 27.849 / 1.67× |
| `lua_table` | 35.871  | 30.802 / 1.16× | 26.691 / 1.34× | 25.754 / 1.39× |

Per the pre-registered 5.5b.2 thresholds (≥1.5× at N=2 AND ≥2.5× at N=4 → SHIP; <1.2× at N=2 → DROP):

* `js`        → **MARGINAL** — 1.32× at N=2 misses the 1.5× bar; never reaches 2.5×.
* `lua_table` → **DROP**     — 1.16× at N=2 trips the drop trigger outright.

### Why scaling stalls

The worker-side compute *does* parallelise — `batch_worker_dispatch_us` shrinks ~linearly with N (js: 24239 → 13323 → 7862 → 5043; lua_table: 14681 → 8637 → 4644 → 3408). Yet wall-clock frame_ms barely halves at N=8. The bottleneck has migrated *out* of the script VM and *into* the main-thread pre/post work that the dispatcher cannot parallelise:

1. **Per-chunk payload allocation.** `run_pooled_chunked` builds N chunks via `c.x[start..end].to_vec()` for every read/write/optional column. At 30k entities × 5 read columns × 2 axes (x,y) = 300k floats copied per frame *just to slice the input N ways*, and that copy lives on the main thread before any worker can start.
2. **Scatter/append on merge.** Each `Vec::append` of a chunk's write column is O(chunk_size) memmove on the main thread. With N=8 we do 8 appends per write column instead of one in-place update — same total bytes, more cache flushes.
3. **The ECS gather/scatter that 5.5b.1 already optimised.** The hecs read into `OwnedReadColumn` at the start of each frame and the scatter back at the end are *unchanged* from 5.5b.1; they were already the dominant cost at N=1, and adding workers does nothing to shrink them.

### Lesson 8 — *More workers is the wrong axis once `dispatch_us` drops below the gather/scatter floor.*

5.4c shipped `5r_3w` at ~37 ms/frame at 30k entities and *every* slice since has shaved <1 ms or, in 5.5b.2's case, paid the wall-clock price of N parallel chunk allocations to recover ~10 ms on JS and ~10 ms on Lua. The shape of the curve is unmistakable: `dispatch_us` halves on every doubling of N but `frame_ms` improves by ~1.2× then ~1.07× then ~1.04×. We're spending parallelism on a part of the frame that's already small. The Pareto-better axis is **fewer payload bytes**, not more workers — zero-copy column slicing (e.g. `Arc<[f32]>` chunks via index ranges instead of `to_vec()`) or a shared-arena scatter pattern that lets the worker thread own the destination column directly. 5.5c picks one of these.

### Decision

Ship the N>1 infrastructure (it's correct, tested, and zero-cost when N=1) but **default `JENGINE_BATCH_WORKERS=1`** until 5.5c removes the chunk-allocation tax. Document `--batch-workers 2` as an opt-in for JS-heavy workloads where the 1.32× is worth the wasted threads, and explicitly *not* recommend N>1 for Lua.

## Step 5 Slice 5.5c.1 (2026-04-29) — zero-copy READ columns via `Arc<Vec<f32>>`

5.5c.1 attacks the per-frame chunk-allocation tax surfaced by 5.5b.2's scaling table. Scope (per the kickoff decision "A — minimal"): swap the `c.x[start..end].to_vec()` per-chunk allocation pattern for shared, refcounted column storage with a sub-`range` descriptor. Writes are out of scope here — they still allocate per chunk because the worker mutates them in place; lifting that cost is 5.5c.2 territory.

The implementation lives in `crates/jengine-core/src/scripting/worker_pool/`:

* `channel.rs` — new `SharedReadColumn { name, x: Arc<Vec<f32>>, y: Arc<Vec<f32>>, range: Range<usize> }` and `SharedOptionalColumn` (same shape with `Option<(Arc<Vec<f32>>, Arc<Vec<f32>>)>`). `BatchRequest::RunBatch` now consumes `Vec<SharedReadColumn>` / `Vec<SharedOptionalColumn>`; `OwnedReadColumn` / `OwnedOptionalColumn` are kept for the Direct/Inline path and the public `run_*_batch` API.
* `worker.rs::handle_run_batch` — slices via `&c.x[c.range.clone()]` (zero copy: `Arc<Vec<f32>>::deref()` → `&Vec<f32>` → slice ops are pointer arithmetic only).
* `mod.rs::run_pooled_chunked` — once per dispatch, wrap each input column in `Arc::new(c.x)` (consumes the Vec; **no element copy**). Per-chunk fan-out then only clones the Arcs (one atomic increment per column per chunk) and sets a fresh `range`. The N=1 / single-chunk fast path also wraps in Shared\* with a full-range descriptor so the worker signature stays uniform.

### Results (30k_5r_3w, 300 frames, headless, seed=1)

| backend     | N=1 (b2 → c1)            | N=2                       | N=4                       | N=8                       |
|-------------|--------------------------|---------------------------|---------------------------|---------------------------|
| `js`        | 46.408 → 45.152 (−2.7%)  | 35.115 → 34.242 (−2.5%)   | 29.878 → 29.407 (−1.6%)   | 27.849 → 26.169 (−6.0%)   |
| `lua_table` | 35.871 → 35.111 (−2.1%)  | 30.802 → 30.091 (−2.3%)   | 26.691 → 25.909 (−2.9%)   | 25.754 → 25.229 (−2.0%)   |

`batch_worker_dispatch_us` shrank 5-20% across all (N, backend) cells. 250/250 lib tests pass; the 2-worker chunked round-trip test confirms the merged column is bit-identical to the 5.5b.2 result.

### Lesson 9 — *`Arc::from(Vec<T>)` is not zero-copy. `Arc::new(Vec<T>)` is.*

The first 5.5c.1 attempt used `Arc<[f32]>` (built via `Arc::from(c.x)`) under the assumption that "shared immutable column" meant "wrap the Vec in an Arc-backed slice." That reads naturally as zero-copy. It is not. Stdlib's `impl<T> From<Vec<T>> for Arc<[T]>` allocates a fresh `ArcInner<[T]>` of exactly the right size and `memcpy`s every element across — the original Vec's allocation is dropped. We were paying the same total bytes-copied as the 5.5b.2 per-chunk scheme but on the main thread, serialised, plus an extra atomic refcount op per chunk. The N=1 case got *strictly worse* (was already zero copy → now one full extra column copy). Net: js +7-12%, lua_table N=1 +14.6%.

The fix was a one-character change at the type level — `Arc<[f32]>` → `Arc<Vec<f32>>` — combined with `Arc::from(c.x)` → `Arc::new(c.x)`. `Arc::new(vec)` allocates a tiny `ArcInner<Vec<T>>` containing only the Vec header (24 bytes on 64-bit: ptr/len/cap); the heap buffer the Vec already owned is simply moved into the Arc by ownership transfer. **Zero element copy.** The slice operations downstream (`&arc[range]`, `arc.iter()`) work transparently via `Deref<Target=Vec<T>>` then `Vec`'s own `Deref<Target=[T]>`.

The takeaway is sharper than the specific API: when the goal is "share a buffer across threads without copying," reach for `Arc<Vec<T>>` or `Arc<Box<[T]>>`, and double-check the allocation behaviour of any `From`/`Into` conversion before assuming it's pointer-only. Stdlib's `From<Vec<T>> for Arc<[T]>` (and the symmetric `From<Vec<T>> for Box<[T]>`) both shrink-and-copy because they're guaranteeing tight layouts and exact-size headers. The Vec-wrapping form trades 24 bytes of indirection for true ownership transfer — almost always the right call for this problem.

### Decision

Ship 5.5c.1 — uniform small win at every N including N=1, no correctness change, no regression. The N>1 scaling characteristics are essentially unchanged from 5.5b.2 (still gather/scatter floor-bound), so the `JENGINE_BATCH_WORKERS=1` default stands. The remaining axes worth exploring: 5.5c.2 (write-side per-worker scratch arena to remove the per-chunk `Vec::with_capacity` + `Vec::append` cost), the originally-scoped 5.5c (RNG determinism + memory accounting), or promoting 5.6 (structural-ops batching). Decision pending.

## Step 5 Slice 5.5c.2 — rejected

5.5c.2 attempted to extend the 5.5c.1 zero-copy primitive to write-input columns. Same shape: a new `SharedWriteColumn { name, x: Arc<Vec<f32>>, y: Arc<Vec<f32>>, range: Range<usize> }` consumed once per dispatch via `Arc::new`; per-chunk fan-out then only clones Arcs and sets a sub-range. Because writes need a *mutable* per-chunk buffer (the script does read-modify-write and we ship the mutated buffer back via `RunBatchOk::writes`), the worker copied its Arc-slice into a fresh local `Vec<f32>` on entry to `handle_run_batch`. The hypothesis: same total bytes copied, but on the worker thread in parallel with the other workers, so N>1 should win.

It didn't. Measured against 5.5c.1 on the standard 30k_5r_3w matrix:

| backend     | N=1     | N=2     | N=4     | N=8     |
|-------------|---------|---------|---------|---------|
| `js`        | **+2.1%** ❌ | +1.3%   | −0.4%   | **+1.7%** ❌ |
| `lua_table` | **+2.7%** ❌ | −3.4%   | +0.6%   | −1.5%   |

`batch_worker_dispatch_us` actually *grew* in 4 of 8 cells. Mixed/negative.

### Why it didn't work

Two reasons, one structural and one quantitative:

1. **The N=1 fast path got worse.** In 5.5c.1, the single-chunk path (taken when `JENGINE_BATCH_WORKERS=1`, which is the documented default) sent `Vec<OwnedWriteColumn>` directly through the channel and the worker mutated those Vecs in place — *zero copies on writes at N=1*. In 5.5c.2 the worker received `Vec<SharedWriteColumn>` and could no longer mutate the Arc'd buffer in place, so it had to do `c.x[range].to_vec()` to obtain a mutable buffer. That's one *extra* full-column memcpy at N=1 that didn't exist before, plus the Arc atomic-refcount overhead. Net: a 2-3% regression on the default deployment for no benefit.
2. **The write-side payload is too small to fight for.** At 30k_5r_3w the write payload is 3 cols × 2 axes × 30k × 4 B = ~720 KB/frame. That's bounded by L2/L3 memory bandwidth (tens of GB/s on the test machine) — the absolute cost is on the order of ~30 µs. At N>1 the saving from moving that off the main thread is theoretically that ~30 µs minus channel/Arc overhead, which empirically came out as somewhere between −3% (lua N=2) and +1-2% (js N=8). Below the noise floor of any decision worth making.

### Architectural endpoint

**5.5c.1 is the architectural endpoint for the chunk-payload axis.** Reads are zero-copy; writes are at their physical floor. The only remaining option for further gains on this axis would be a persistent per-worker scratch arena round-tripped across frames (so the worker holds onto its mutable `Vec<f32>` across dispatches and the dispatcher merges directly into a new column buffer rather than receiving a fresh one). That requires buffer-ownership-state tracking spanning multiple frames, complicates the channel protocol, and the expected gain is the same ~30 µs (because the underlying memcpy is bandwidth-bound, not allocation-overhead-bound) — i.e., bounded above by the same ~3% that 5.5c.2 achieved at its best. Not worth the complexity.

If a contributor proposes "let's eliminate the last write-side memcpy" in the future, point them at this section. The number to beat is "a measured ≥10% sustained win across the matrix without regressing the N=1 default" — anything less is in the noise floor.

### Decision

Reject 5.5c.2. Revert cleanly (no feature flag, no commented code, no stub infrastructure left behind — codebase looks like 5.5c.1). Pivot to the originally-scoped Slice 5.5c (deterministic PRNG seeding + `script_worker_memory_bytes` counter + determinism test) to close Step 5 commitments.

## Step 5 Slice 5.5c (2026-04-30) — RNG determinism + worker memory counter

Closes Step 5 with the originally-scoped Slice 5.5c work: replace the host's non-deterministic `Math.random()` / `math.random()` with a seeded PRNG so batch scripts are reproducible across runs, and surface per-worker JS/Lua heap usage as a frame counter.

### What shipped

1. **`scripting/prng.rs`** — `splitmix64` finalizer for seed mixing + `xorshift64*` for the stream. `mix_seed(scene_seed, frame, system_id, chunk_id)` chains three splitmix calls into a single `u64`. Zero-state guard substitutes `0xDEAD_BEEF_CAFE_BABE`. `next_f64_unit()` masks the top 53 bits and divides by `2^53` for unbiased `[0, 1)`.
2. **Runtime shims** — `BatchJsRuntime` installs a `Math.random` closure (`MutFn` over `Rc<RefCell<DetRng>>`) at construction. `BatchLuaRuntime` installs a `math.random` shim that switches on argument count (0 → number, 1 → `[1, n]`, 2 → `[m, n]`).
3. **Per-batch reseed** — `BatchScriptPool` carries `scene_seed: u64` + `frame: u64`; `run_batch_scripts` calls `pool.advance_frame()` once per frame, then for each script `system_id = stable_path_hash(path)` and the worker dispatcher computes `mix_seed(scene_seed, frame, system_id, chunk_id)` per chunk and sends it through `BatchRequest::RunBatch { rng_seed, .. }`. The worker calls `rt.set_rng_seed(rng_seed)` before invoking `update`.
4. **Memory counter** — `BatchJsRuntime::memory_bytes()` returns `runtime.memory_usage().malloc_size`; `BatchLuaRuntime::memory_bytes()` returns `lua.used_memory()`. The worker samples *after* the batch completes (frame boundary). Pool aggregates as `worker_memory_bytes_max` + `worker_memory_bytes_total` into `FrameCounters`.
5. **Integration test** — `tests/script_determinism.rs` (1024 entities × 60 frames) covers four properties:
   - `js_batch_is_reproducible_across_runs` — same seed/N → bit-identical writes.
   - `lua_batch_is_reproducible_across_runs` — same.
   - `js_batch_chunk0_prefix_matches_across_worker_counts` — chunk-0 prefix matches across N ∈ {1, 2, 4, 8}.
   - `lua_batch_chunk0_prefix_matches_across_worker_counts` — same.

### Lessons

**1. Per-chunk seeding gives per-chunk determinism, not full N-independence.** With `(scene_seed, frame, system_id, chunk_id)` as the seed tuple, scripts in chunk 0 share one RNG stream and scripts in chunk 1 share another. If the script consumes one `Math.random()` per entity, then under N=1 entity 512 sees the 513th draw from stream S0, while under N=2 it sees the 1st draw from stream S1. The test design has to reflect that — assert chunk-0 prefix equality across N, full-column equality only for same-N reruns. We initially wrote a stricter "all entities equal across N" test and it correctly failed; the fix was to weaken the assertion to what the spec actually guarantees, not to redesign the seeding. Per-entity seeding (one xorshift state per entity) would give true N-independence at the cost of seeding throughput; out of scope for 5.5c.

**2. `mlua` Luau sandbox order is load-bearing.** `lua.sandbox(true)` freezes globals as read-only. The first cut of `BatchLuaRuntime::new()` enabled the sandbox first and *then* tried to `globals().set("math", ...)` on the frozen `math` table — the call silently failed (no error returned, but the shim wasn't installed) and 37 batch_lua tests went red on the first run. Fix: install the shim *before* `lua.sandbox(true)`. Generalises to: any global rebinding must precede sandbox enablement; the sandbox is not a no-op gate, it's a one-way mutation.

**3. `mlua::Integer` is `i32` under the Luau backend.** `mlua` pre-1.0 with the `luau` feature uses 32-bit integers (Lua 5.4's `lua_Integer` would be 64-bit). The shim's `(m, n)` integer-range branch needed explicit `as i64` widening for the multiplication, then a final `as mlua::Integer` cast back. Without it, `cargo check --features native` failed with "expected i32, found i64" mid-arithmetic. Lock the cast at the boundary and do all internal math at `i64`.

**4. Seed tuple deserves per-script, per-frame mixing, not just per-pool.** First sketch had `BatchScriptPool::set_scene_seed` flow into `set_rng_seed` directly. That gives same script + same frame ≠ same seed when there are multiple scripts (collision risk) and same script + different frames = same seed (catastrophic — frame-to-frame sequences would repeat). The four-component tuple isn't padding, it's the minimum factoring to keep streams independent across all four axes. The `splitmix64` chain is cheap (3 multiplications + 3 xorshifts), well below per-batch overhead.

**5. `rquickjs::function::MutFn` is the right wrapper for Rust state owned by the runtime.** `Function::new(ctx.clone(), MutFn::new(move || -> f64 { rng.borrow_mut().next_f64_unit() }))` captures the `Rc<RefCell<DetRng>>` and gives JS a callable that mutates the host RNG. `Func::from` doesn't allow `FnMut` closures; `MutFn` is the documented way. Single-threaded by construction (the worker thread owns the runtime + the RNG together), so `Rc<RefCell<_>>` is sound — no `Send` required across the JS→Rust boundary.

### Decision

Ship 5.5c. Step 5 done. The remaining `script_worker_memory_bytes_max/_total` counters are now wired through `FrameCounters` but not yet surfaced in the benchmark UI/JSON output — that lands with the next benchmark refresh. The deterministic-PRNG infrastructure is the foundation Slice 5.6 (structural-ops batching) and any future replay/snapshot tooling will build on.
