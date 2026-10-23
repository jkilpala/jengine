# Runtime profiling — the F3 overlay

Every runtime draws the same debug overlay (`crates/jengine-core/src/debug_overlay.rs`):

- **F3** — stats HUD (top right)
- **F12** — entity inspector (top left)
- **F4** — physics outlines: every collider (grey static, green dynamic, blue
  kinematic, yellow sensor) and every joint as an orange line from its anchor
  to its target. Drawn from the components, so it shows what the scene
  *authored* (`debug_draw::physics_debug_commands`). In the editor it works in
  Play; the Scene view has its own collider outlines on **C**.

All three are gated per build; see [Turning it off in a build](#turning-it-off-in-a-build).

## `frame` is a rate, not a cost

**`frame` is the wall-clock gap between one frame and the next**, so it is
bounded below by whatever paces the loop — vsync in a browser, the display's
refresh rate natively. On a 165 Hz monitor it reads 6.00 ms whether the frame
did 6 ms of work or 0.5 ms; the rest is the browser waiting.

This is worth stating because the obvious inference is wrong and looks
authoritative. Subtracting the phase rows from `frame` appears to reveal
milliseconds of unaccounted overhead — but when the loop is vsync-limited that
difference *is the wait*, and chasing it is chasing nothing. It only means
something when the phases add up to more than the refresh interval, which is
exactly when there is a real problem to find.

The editor's profiler panel therefore shows a second number, **CPU**: wall time
actually spent inside the render callback, averaged over the panel's refresh
interval. That is the one to watch. `CPU` minus the phase rows is the editor's
own per-frame cost — the `viewport_get_*` calls, the JSON, the store writes —
and is the only place that overhead is visible (see
[`architecture-debt.md`](architecture-debt.md) #7). The phase bar underneath is
proportioned against `CPU` for the same reason: against `frame` its leftover
segment was mostly vsync idle wearing the colour of engine work.

The native F3 overlay has no equivalent row yet; `frame` there carries the same
caveat.

## Pacing is invisible to every average

`frame` is an average too, and a perfect one can hide motion that judders:
at 165 Hz a native Vulkan window on a hybrid-GPU laptop averages 6.06 ms
while delivering 10 / 2 ms pairs. Set **`JENGINE_DT_LOG=1`** before starting
the native runtime and it logs every frame's raw wall delta beside the paced
delta the simulation got (`time::FramePacer`) - one line a frame, so run it
for a few seconds. Paced deltas that sit flat on the refresh period while
the raw ones jump are the pacer doing its job; raw ones passed through
unpaced, outside a real stall, are the thing to report.

## The rows

```
FPS     133.9
frame    6.10ms     wall-clock frame time (not scaled by set_time_scale)
script   0.42ms     script phase: init/update + queued bridge commands
phys     0.11ms     rapier step + ECS sync, 0.00 when physics is off
render   1.80ms     the render pass, from begin_frame to present
draws       12      sprite-batch flushes
sprites     84      entities carrying a SpriteComponent
partcl     210      live particles
entity     106      live entities (read from the world, always current)
mem      12.0MB
heap      5.4MB     browser only
peak      6.1MB
```

`render` and `draws` describe the **previous** frame. The overlay is drawn from
inside the pass that produces them, so this frame's numbers do not exist yet.
Every other row is this frame's.

## Reading the memory rows

Three different questions, three numbers:

| Row | Meaning |
|---|---|
| `mem` | What the process holds. In a browser this is **reserved wasm linear memory**; natively it is the live heap. |
| `heap` | Bytes live on the heap right now. Shown only when it differs from `mem` — i.e. on the web and in the editor viewport. |
| `peak` | High-water mark of `heap` since start. |

**Reserved wasm memory never shrinks.** WebAssembly grows its linear memory on
demand and never returns it, so `mem` in a browser is a high-water mark of
allocator pressure, not a live figure. Loading a scene allocates a whole new
world, parses JSON, and decodes textures; if that transient peak is higher than
anything before it, `mem` steps up and stays there — with nothing leaked.

So: **watch `heap`, not `mem`, when chasing a leak.** Return to a scene twice
and compare `heap` at the same point each time. Flat `heap` with rising `mem` is
allocator high-water and will plateau. Rising `heap` is a real leak.

Neither number covers:

- **GPU memory** — textures, render targets, and buffers live in the driver.
- **The JavaScript heap** — on the web that includes every Fengari Lua state
  (`compileLua` in `editor/src/lib/lua-runtime.ts` builds one state per script
  file, per scene load) and all the wasm-bindgen glue. A browser tab's reported
  memory is mostly this, which is why it dwarfs `mem`.

Use the browser's own memory profiler for those two.

The counters come from `profiling::memory::TrackingAllocator`, which each host
installs as its `#[global_allocator]` — the native runtime, the web runtime, and
the editor viewport all do. A host that doesn't shows `mem n/a`; nothing else
depends on tracking being present.

## Turning it off in a build

Build Settings → **Debug Overlay** (both the Windows and Web panels). On by
default. Unchecking it writes `settings.debug_overlay: false` into the
`project.jengine` that ships beside the game, and both runtimes then ignore F3 /
F4 / F12 entirely — including the script-driven `set_debug_overlay()` on native.

A missing key means enabled, so running a scene straight out of a source tree,
or a build made before the option existed, keeps the overlays.

## Captures: where the time went

The overlay's rows are phase totals. For a breakdown, record a **capture**
(`profiling::trace`, Oct 2026): every frame's spans - named, nested, each
with its CPU time and the draw calls made inside it - and each script's
cost by file.

- **In the editor:** the **Profiler** panel (bottom dock; *Show Profiler* in
  the command palette). *Record* while the scene plays; click a frame's bar
  (Shift for a range), *All*, or *Worst frame*; the span tree shows each
  span's time per frame, its own time (children taken off), the worst
  frame's and its draws, beside the scripts by cost and the render passes
  by draws. *Save* / *Open* read and write `.jprof` files.
- **Natively:** `JENGINE_PROFILE_CAPTURE=<path>` before starting the
  runtime records the last `JENGINE_PROFILE_FRAMES` frames (600) and
  rewrites the file each time that many more have passed, and at exit.
  Open it in the editor's panel.

Things to know when reading one:

- **Times are CPU.** A render span is the time to build and encode that
  pass, not the GPU's time drawing it.
- **A span's own time is real work too**: `render.world`'s is collecting
  and sorting the world's drawables before any pass draws them.
- **A span called twice a frame is one row** (`hierarchy ×2.0`), at its
  whole cost.
- **Untraced time** - the frame minus its top-level spans - is the host's
  own work (the editor's JSON and store writes, input, the present) plus
  whatever has no span yet. Behaviour graphs have one span for all of them.
- Off, a span costs a thread-local read: **a new shared system should open
  one** (`let _span = crate::profiling::trace::span("name");` - dotted
  names, the part before the first dot is the colour on the frame bars).

## Where the numbers come from

Each runtime fills its own `FrameStats`; there is no shared frame loop.

| Runtime | Filled in |
|---|---|
| Native | `wgpu_scene_runner.rs` `render_frame`, published via `ScriptSystem::update_frame_stats` — which is also what `engine.frame_stats()` reads from a script |
| Web | `jengine-web/src/lib.rs` `render_frame` |
| Editor viewport | `jengine-editor-viewport/src/lib.rs`, its own struct, surfaced in the editor's profiler panel rather than the overlay |

If a row reads a flat `0.00` on one runtime only, that runtime is not filling
it — check its frame function before believing the number.
