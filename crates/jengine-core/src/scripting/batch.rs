//! Step 4 walking-skeleton: batch JS systems via `rquickjs` `TypedArray`s.
//!
//! This module is intentionally **isolated** from the per-entity script bridge
//! (`scripting::bridge`). A batch system is a separate model — it gets a
//! sandboxed JS context with **no engine globals**, just `update(ctx, q)`.
//!
//! ### Walking-skeleton scope
//!
//! - JS-only (`rquickjs` host). No browser path yet; `BatchScriptComponent`
//!   on a WASM build is a no-op (TODO Step 4 cross-platform).
//! - One declared system per script module. Scripts may export the
//!   `update` entry point either as a bare `function update` declaration
//!   or via `module.exports.update` / `exports.update`. The harness no
//!   longer text-rewrites `export function`; scripts that need ES module
//!   syntax must wait for full module support (Step 4.3+).
//! - Per-script `system` metadata (Slice 4.2): scripts declare
//!   `system.query.read` / `system.query.write` as arrays of component
//!   names. The walking-skeleton runner only supports the fixed contract
//!   `read = ["Velocity2D"]`, `write = ["Position2D"]` and rejects loads
//!   that declare anything else (so a future broader runner can't
//!   silently ignore mismatched specs).
//! - Mirror buffers are **freshly allocated per call** as `Float32Array`s.
//!   No cross-frame buffer reuse (Step 4 spec — preserves "stale views are
//!   safe" semantics).
//! - Single-threaded. Parallel/worker model is Step 4C.

#![cfg(all(not(target_arch = "wasm32"), any(feature = "js-adapter-host", feature = "native")))]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use rquickjs::function::MutFn;
use rquickjs::{Context, Function, Object, Persistent, Runtime, TypedArray};

use crate::profiling::FrameCounters;
use crate::scripting::prng::DetRng;

/// One declared typed state field on a batch system (Slice 5.1).
///
/// Slice 5.1 supports `f32` only. `entity` / `u8` / vector types are
/// planned for later slices but currently rejected at load time.
#[derive(Debug, Clone)]
pub struct BatchStateField {
    pub name: String,
    pub default: f32,
}

/// Declared query schema for a batch system (Slice 4.2).
///
/// Parsed from the script's `system.query.{read,write,read_optional,state}`
/// at load time. The walking-skeleton runner only honours the fixed
/// Velocity2D → Position2D contract for read/write columns; Slice 5.1
/// adds typed state columns alongside; Slice 5.2 adds a single optional
/// read column (`Acceleration2D`).
#[derive(Debug, Clone, Default)]
pub struct BatchSystemSpec {
    pub read: Vec<String>,
    pub write: Vec<String>,
    /// Slice 5.2: components the script reads only when present. The
    /// dispatcher partitions entities by which of these are attached and
    /// invokes the script once per archetype combination, passing
    /// `q.read_optional.<Name>` as a populated column or `null`/`nil`.
    /// Walking-skeleton accepts only `[]` or `["Acceleration2D"]`.
    pub read_optional: Vec<String>,
    /// Optional declared tick rate (Hz). Informational only for Slice
    /// 4.2; the scheduler does not consult it yet.
    pub tick_rate: Option<u32>,
    /// Typed local state columns (Slice 5.1). Order is preserved from
    /// declaration order so per-entity `Vec<f32>` slots line up
    /// deterministically across reloads.
    pub state: Vec<BatchStateField>,
}

/// Maximum total component columns a single batch system may declare
/// across `read` + `write` + `read_optional` (Slice 5.3 cap). Picked low
/// enough that the optional-presence partition (`2^|read_optional|`
/// buckets) stays bounded even in the worst case, and forces script
/// authors to split monolithic systems into focused ones.
pub const MAX_BATCH_COLUMNS: usize = 12;

/// Component names a Slice 5.3 batch script may reference in
/// `system.query.{read,write,read_optional}`. Each entry is a 2-field
/// `{x: f32, y: f32}` component the dispatcher knows how to gather and
/// scatter (see `ecs::systems::run_batch_scripts`).
///
/// Order is the canonical iteration order used when building 2^k
/// presence bitmasks for the partition step.
pub const KNOWN_BATCH_COMPONENTS: &[&str] = &[
    "Position2D",
    "Velocity2D",
    "Acceleration2D",
    "Drag2D",
    "Force2D",
    "Wind2D",
    "Impulse2D",
    "Torque2D",
];

/// One read column for a batch invocation. `name` matches a
/// [`KNOWN_BATCH_COMPONENTS`] entry; `x` / `y` are the per-entity scalar
/// arrays (length == batch count).
pub struct BatchReadColumn<'a> {
    pub name: &'a str,
    pub x: &'a [f32],
    pub y: &'a [f32],
}

/// One write column for a batch invocation. Pre-populated by the caller
/// with the current ECS values so the script can read-modify-write in
/// place. The dispatcher scatters the (possibly mutated) `x`/`y` slices
/// back into the ECS after the call.
pub struct BatchWriteColumn<'a> {
    pub name: &'a str,
    pub x: &'a mut [f32],
    pub y: &'a mut [f32],
}

/// One optional read column for a batch invocation. `value` is `Some`
/// when *every* entity in the batch has the component (the dispatcher
/// partitions by archetype) and `None` otherwise — in which case the
/// script sees `q.read_optional.<Name>` as JS `null` / Lua `nil`.
pub struct BatchOptionalColumn<'a> {
    pub name: &'a str,
    pub value: Option<(&'a [f32], &'a [f32])>,
}

/// Per-system bookkeeping after a successful load.
struct LoadedBatchScript {
    update: Persistent<Function<'static>>,
    /// Source path, kept for diagnostics + dedup keying.
    #[allow(dead_code)]
    path: String,
    /// Header-declared schema. `None` for legacy scripts that don't
    /// declare `system` (accepted with a warn, treated as the default
    /// Velocity2D → Position2D contract). Used by `state_schema()`.
    spec: Option<BatchSystemSpec>,
}

/// One QuickJS Runtime + Context per process, shared across all batch
/// systems. Sandboxed: no `engine`, no `set_position`, no globals beyond the
/// JS standard library. Scripts can only mutate the TypedArrays they're
/// handed in `q.write.*`.
pub struct BatchJsRuntime {
    /// Drop order matters: scripts (Persistents) drop before context, context
    /// drops before runtime. QuickJS asserts at runtime drop that no live
    /// objects remain.
    scripts: HashMap<String, LoadedBatchScript>,
    context: Context,
    runtime: Runtime,
    /// Slice 5.5c — deterministic RNG that backs the in-script
    /// `Math.random()` shim. Reseeded by [`Self::set_rng_seed`] before
    /// each `run_batch` invocation so the per-(scene_seed, frame,
    /// system_id, chunk_id) stream is reproducible.
    rng: Rc<RefCell<DetRng>>,
}

/// Errors a batch system can produce. Walking-skeleton variant: stringly typed.
#[derive(Debug, thiserror::Error)]
pub enum BatchError {
    #[error("rquickjs init failed: {0}")]
    Init(String),
    #[error("script `{path}` failed to load: {message}")]
    Load { path: String, message: String },
    #[error("script `{path}` did not export `update`")]
    MissingUpdate { path: String },
    #[error(
        "script `{path}` declares system.query incompatible with the walking-skeleton runner: \
         expected read=[\"Velocity2D\"], write=[\"Position2D\"], read_optional in {{[], [\"Acceleration2D\"]}}; \
         got read={read:?}, write={write:?}, read_optional={read_optional:?}"
    )]
    SpecMismatch {
        path: String,
        read: Vec<String>,
        write: Vec<String>,
        /// Slice 5.2: also surfaced so dispatcher writers can see which
        /// optional component was rejected.
        read_optional: Vec<String>,
    },
    #[error("script `{path}` threw at runtime: {message}")]
    RunFailed { path: String, message: String },
    /// Slice 5.3: the declared total of `read` + `write` + `read_optional`
    /// exceeds [`MAX_BATCH_COLUMNS`].
    #[error(
        "script `{path}` declares {total} batch columns (read={read}, write={write}, read_optional={read_optional}); \
         the per-system cap is {max}. Split the system into smaller focused systems."
    )]
    TooManyColumns {
        path: String,
        read: usize,
        write: usize,
        read_optional: usize,
        total: usize,
        max: usize,
    },
    /// Slice 5.3: a component name appears in `read` / `write` /
    /// `read_optional` that the dispatcher does not recognise.
    #[error(
        "script `{path}` references unknown batch component `{name}` in `{bucket}`; \
         known components: {known:?}"
    )]
    UnknownComponent {
        path: String,
        name: String,
        bucket: &'static str,
        known: Vec<&'static str>,
    },
    /// Slice 5.3: the same component name appears in more than one of
    /// `read` / `write` / `read_optional`. Each component must live in
    /// exactly one bucket — scripts that need read-modify-write put the
    /// component in `write` (it's pre-populated with the ECS snapshot).
    #[error(
        "script `{path}` declares component `{name}` in multiple buckets ({buckets:?}); \
         each component must appear in exactly one of read / write / read_optional"
    )]
    DuplicateComponent {
        path: String,
        name: String,
        buckets: Vec<&'static str>,
    },
}

impl BatchJsRuntime {
    pub fn new() -> Result<Self, BatchError> {
        let runtime = Runtime::new().map_err(|e| BatchError::Init(e.to_string()))?;
        let context = Context::full(&runtime).map_err(|e| BatchError::Init(e.to_string()))?;
        // Slice 5.5c — install deterministic `Math.random` shim. The
        // RNG is reseeded by the worker before each `run_batch` call;
        // we just install the override once at construction time.
        let rng = Rc::new(RefCell::new(DetRng::new(0)));
        let rng_for_js = rng.clone();
        context
            .with(|ctx| -> rquickjs::Result<()> {
                let math: Object = ctx.globals().get("Math")?;
                let f = Function::new(
                    ctx.clone(),
                    MutFn::new(move || -> f64 { rng_for_js.borrow_mut().next_f64_unit() }),
                )?;
                math.set("random", f)?;
                Ok(())
            })
            .map_err(|e| BatchError::Init(format!("installing Math.random shim: {e}")))?;
        Ok(Self { scripts: HashMap::new(), context, runtime, rng })
    }

    /// Slice 5.5c — reseed the deterministic RNG. The worker thread
    /// calls this before each `run_batch` with a per-chunk seed
    /// derived via `prng::mix_seed(scene_seed, frame, system_id,
    /// chunk_id)`, so two runs with identical inputs produce
    /// bit-identical script outputs.
    pub fn set_rng_seed(&self, seed: u64) {
        self.rng.borrow_mut().reseed(seed);
    }

    /// Slice 5.5c — current QuickJS allocator footprint in bytes.
    /// Sampled at frame boundaries (worker idle) so the value
    /// reflects steady state, not in-flight allocations. `0` if the
    /// runtime reports a negative malloc_size (shouldn't happen).
    pub fn memory_bytes(&self) -> u64 {
        let usage = self.runtime.memory_usage();
        usage.malloc_size.max(0) as u64
    }

    /// Returns true if this path has already been loaded.
    pub fn is_loaded(&self, path: &str) -> bool {
        self.scripts.contains_key(path)
    }

    /// Load a JS source as a batch system. The source runs inside an
    /// IIFE that exposes a fresh `module = { exports: {} }` and `exports`
    /// alias so scripts can use any of:
    ///
    /// ```js
    /// // (a) bare declarations
    /// function update(ctx, q) { ... }
    /// const system = { query: { read: ["Velocity2D"], write: ["Position2D"] } };
    ///
    /// // (b) CommonJS-style
    /// module.exports.update = function (ctx, q) { ... };
    /// module.exports.system = { query: { ... } };
    ///
    /// // (c) `exports` shorthand
    /// exports.update = function (ctx, q) { ... };
    /// exports.system = { query: { ... } };
    /// ```
    ///
    /// Slice 4.2 dropped the `export function` text rewrite; scripts that
    /// previously relied on `export function update(...)` must drop the
    /// `export` keyword (or migrate to `module.exports`).
    ///
    /// The wrapper extracts `update` and (optionally) `system` from
    /// either `module.exports` or the IIFE's local scope (declarations
    /// shadow `module.exports` in case both are provided). If the script
    /// declares `system.query`, the harness validates that
    /// `read == ["Velocity2D"]` and `write == ["Position2D"]`; mismatches
    /// fail loudly so the future broader runner can't silently ignore
    /// what scripts ask for.
    pub fn load(&mut self, path: &str, source: &str) -> Result<(), BatchError> {
        // Wrapper notes:
        // - `module = { exports: {} }` and `exports = module.exports` are
        //   bound *before* the user source so CommonJS-style assignments
        //   work.
        // - Local `function update` / `const system` declarations from
        //   the user source take precedence over `module.exports.*` so
        //   the simplest scripts (option (a) above) just work.
        // - The wrapper returns `{ update, system }` so the Rust side
        //   can pull both with one `eval`.
        let wrapped = format!(
            "(function() {{\n\
             const module = {{ exports: {{}} }};\n\
             const exports = module.exports;\n\
             {source}\n\
             const __upd = (typeof update !== 'undefined') ? update : module.exports.update;\n\
             const __sys = (typeof system !== 'undefined') ? system : module.exports.system;\n\
             return {{ update: (typeof __upd === 'function') ? __upd : null, system: __sys || null }};\n\
             }})()",
        );
        let path_owned = path.to_string();
        let script = self.context.with(|ctx| -> Result<LoadedBatchScript, BatchError> {
            let result: Object = ctx.eval(wrapped).map_err(|e| BatchError::Load {
                path: path_owned.clone(),
                message: e.to_string(),
            })?;
            let update: Option<Function> = result.get("update").ok();
            let update = update.ok_or_else(|| BatchError::MissingUpdate { path: path_owned.clone() })?;
            let spec = parse_system_spec(&result);
            if let Some(s) = &spec {
                validate_walking_skeleton_spec(&path_owned, s)?;
            } else {
                log::warn!(
                    "[BatchScript] '{}' has no `system` header — accepting with the default \
                     Velocity2D → Position2D contract. Add `const system = {{ query: {{ \
                     read: [\"Velocity2D\"], write: [\"Position2D\"] }} }};` to silence this warn.",
                    path_owned
                );
            }
            Ok(LoadedBatchScript {
                update: Persistent::save(&ctx, update),
                path: path_owned.clone(),
                spec,
            })
        })?;
        self.scripts.insert(path.to_string(), script);
        Ok(())
    }

    /// Number of distinct scripts currently loaded. Useful for tests and
    /// diagnostics confirming multi-script dispatch.
    pub fn loaded_count(&self) -> usize {
        self.scripts.len()
    }

    /// Slice 5.1: declared typed state schema for a loaded script, in
    /// declaration order. Returns `None` if the script wasn't loaded or
    /// declared no `system.query.state`. Empty slice means "loaded, no
    /// state". The dispatcher uses this to size + initialize per-entity
    /// state vectors against the script's defaults.
    pub fn state_schema(&self, path: &str) -> Option<&[BatchStateField]> {
        self.scripts
            .get(path)
            .map(|s| s.spec.as_ref().map(|sp| sp.state.as_slice()).unwrap_or(&[]))
    }

    /// Slice 5.2: declared optional read components for a loaded script.
    /// Returns `None` if the script isn't loaded; `Some(&[])` for loaded
    /// scripts that didn't declare `read_optional`. Walking-skeleton
    /// callers will see at most `["Acceleration2D"]` here — anything
    /// else would have been rejected by `validate_walking_skeleton_spec`
    /// at load time.
    pub fn read_optional_schema(&self, path: &str) -> Option<&[String]> {
        self.scripts
            .get(path)
            .map(|s| s.spec.as_ref().map(|sp| sp.read_optional.as_slice()).unwrap_or(&[]))
    }

    /// Slice 5.3: declared `read` components for a loaded script.
    /// `Some(&[])` for loaded scripts without an explicit `read`.
    pub fn read_schema(&self, path: &str) -> Option<&[String]> {
        self.scripts
            .get(path)
            .map(|s| s.spec.as_ref().map(|sp| sp.read.as_slice()).unwrap_or(&[]))
    }

    /// Slice 5.3: declared `write` components for a loaded script.
    pub fn write_schema(&self, path: &str) -> Option<&[String]> {
        self.scripts
            .get(path)
            .map(|s| s.spec.as_ref().map(|sp| sp.write.as_slice()).unwrap_or(&[]))
    }

    /// Walking-skeleton update. Caller has already gathered the column data
    /// for one batch system invocation; this:
    ///
    /// 1. Allocates four fresh `Float32Array`s (vx, vy, px, py).
    /// 2. Builds the `q` object with `read`/`write` views and `count`.
    /// 3. Calls `update(ctx, q)` once.
    /// 4. Reads the (possibly mutated) `px`/`py` views back into the caller's
    ///    `position_x` / `position_y` slices.
    ///
    /// `counters` is updated with all 8 `batch_*` fields for this invocation.
    /// The caller is responsible for accumulating `batch_systems_run` /
    /// `batch_chunks` / `batch_vm_calls` (this fn does not increment them
    /// because the caller may decide to skip empty batches).
    ///
    /// Returns the elapsed time inside the JS call in microseconds.
    pub fn run_velocity_to_position(
        &self,
        path: &str,
        dt: f32,
        velocity_x: &[f32],
        velocity_y: &[f32],
        position_x: &mut [f32],
        position_y: &mut [f32],
        state: &mut [Vec<f32>],
        // Slice 5.2: optional Acceleration2D column. Some((ax, ay)) when
        // EVERY entity in this batch has the component (the dispatcher
        // partitions by archetype so a single batch is uniform); None
        // when no entity in this batch has it. Both slices must have
        // length velocity_x.len() when Some.
        accel: Option<(&[f32], &[f32])>,
        counters: &mut FrameCounters,
    ) -> Result<u64, BatchError> {
        debug_assert_eq!(velocity_x.len(), velocity_y.len());
        debug_assert_eq!(position_x.len(), position_y.len());
        debug_assert_eq!(velocity_x.len(), position_x.len());
        if let Some((ax, ay)) = accel {
            debug_assert_eq!(ax.len(), velocity_x.len(), "accel x column wrong length");
            debug_assert_eq!(ay.len(), velocity_x.len(), "accel y column wrong length");
        }

        let count = velocity_x.len();
        if count == 0 {
            return Ok(0);
        }

        let script = self
            .scripts
            .get(path)
            .ok_or_else(|| BatchError::MissingUpdate { path: path.to_string() })?;

        // Slice 5.1: state schema lookup. Validate state column lengths
        // up front; mismatches are a programmer error in the dispatcher.
        let state_schema: &[BatchStateField] =
            script.spec.as_ref().map(|s| s.state.as_slice()).unwrap_or(&[]);
        debug_assert_eq!(
            state.len(),
            state_schema.len(),
            "state column count must match script's declared schema (script={path}, declared={}, got={})",
            state_schema.len(),
            state.len(),
        );
        for (i, col) in state.iter().enumerate() {
            debug_assert_eq!(
                col.len(),
                count,
                "state column `{}` has wrong length (got {}, expected {})",
                state_schema.get(i).map(|f| f.name.as_str()).unwrap_or("?"),
                col.len(),
                count,
            );
        }

        let path_owned = path.to_string();
        let elapsed_us = self.context.with(|ctx| -> Result<u64, BatchError> {
            // ── copy-in ───────────────────────────────────────────────
            let copy_in_start = Instant::now();
            let vx = TypedArray::<f32>::new_copy(ctx.clone(), velocity_x).map_err(|e| {
                BatchError::RunFailed { path: path_owned.clone(), message: e.to_string() }
            })?;
            let vy = TypedArray::<f32>::new_copy(ctx.clone(), velocity_y).map_err(|e| {
                BatchError::RunFailed { path: path_owned.clone(), message: e.to_string() }
            })?;
            let px = TypedArray::<f32>::new_copy(ctx.clone(), &*position_x).map_err(|e| {
                BatchError::RunFailed { path: path_owned.clone(), message: e.to_string() }
            })?;
            let py = TypedArray::<f32>::new_copy(ctx.clone(), &*position_y).map_err(|e| {
                BatchError::RunFailed { path: path_owned.clone(), message: e.to_string() }
            })?;

            // Slice 5.1: typed state columns. Each declared field is
            // exposed as `q.state.<name>` Float32Array. We retain handles
            // so we can read mutations back after the call.
            let mut state_handles: Vec<TypedArray<'_, f32>> =
                Vec::with_capacity(state_schema.len());
            let state_obj = if state_schema.is_empty() {
                None
            } else {
                let obj = Object::new(ctx.clone()).map_err(|e| BatchError::RunFailed {
                    path: path_owned.clone(),
                    message: e.to_string(),
                })?;
                for (field, col) in state_schema.iter().zip(state.iter()) {
                    let arr = TypedArray::<f32>::new_copy(ctx.clone(), col.as_slice()).map_err(
                        |e| BatchError::RunFailed {
                            path: path_owned.clone(),
                            message: e.to_string(),
                        },
                    )?;
                    let handle = arr.clone();
                    obj.set(field.name.as_str(), arr).ok();
                    state_handles.push(handle);
                }
                Some(obj)
            };

            // Build q.read.Velocity2D and q.write.Position2D nested objects.
            let read_vel = Object::new(ctx.clone()).map_err(|e| BatchError::RunFailed {
                path: path_owned.clone(),
                message: e.to_string(),
            })?;
            read_vel.set("x", vx.clone()).ok();
            read_vel.set("y", vy.clone()).ok();
            let read = Object::new(ctx.clone()).map_err(|e| BatchError::RunFailed {
                path: path_owned.clone(),
                message: e.to_string(),
            })?;
            read.set("Velocity2D", read_vel).ok();

            // Slice 5.2: q.read_optional.Acceleration2D — either a
            // populated { x, y } object of Float32Arrays, or `null` so
            // scripts can `if (q.read_optional.Acceleration2D) { ... }`.
            // The walking-skeleton runner only knows about Acceleration2D
            // here; broader runners would iterate the script's
            // declared `read_optional` schema.
            let read_optional_obj = Object::new(ctx.clone()).map_err(|e| BatchError::RunFailed {
                path: path_owned.clone(),
                message: e.to_string(),
            })?;
            if let Some((ax, ay)) = accel {
                let ax_arr = TypedArray::<f32>::new_copy(ctx.clone(), ax).map_err(|e| {
                    BatchError::RunFailed { path: path_owned.clone(), message: e.to_string() }
                })?;
                let ay_arr = TypedArray::<f32>::new_copy(ctx.clone(), ay).map_err(|e| {
                    BatchError::RunFailed { path: path_owned.clone(), message: e.to_string() }
                })?;
                let accel_obj = Object::new(ctx.clone()).map_err(|e| BatchError::RunFailed {
                    path: path_owned.clone(),
                    message: e.to_string(),
                })?;
                accel_obj.set("x", ax_arr).ok();
                accel_obj.set("y", ay_arr).ok();
                read_optional_obj.set("Acceleration2D", accel_obj).ok();
            } else {
                // Explicit null so scripts using truthy checks see the
                // absence (vs `undefined` which is also falsy but less
                // obvious in serializers / debug prints).
                read_optional_obj
                    .set("Acceleration2D", rquickjs::Value::new_null(ctx.clone()))
                    .ok();
            }

            let write_pos = Object::new(ctx.clone()).map_err(|e| BatchError::RunFailed {
                path: path_owned.clone(),
                message: e.to_string(),
            })?;
            // Hold a clone of px/py so we can read back from the SAME
            // ArrayBuffer the script mutated. (Setting moves them into JS land.)
            let px_handle = px.clone();
            let py_handle = py.clone();
            write_pos.set("x", px).ok();
            write_pos.set("y", py).ok();
            let write = Object::new(ctx.clone()).map_err(|e| BatchError::RunFailed {
                path: path_owned.clone(),
                message: e.to_string(),
            })?;
            write.set("Position2D", write_pos).ok();

            let q = Object::new(ctx.clone()).map_err(|e| BatchError::RunFailed {
                path: path_owned.clone(),
                message: e.to_string(),
            })?;
            q.set("count", count as u32).ok();
            q.set("read", read).ok();
            q.set("read_optional", read_optional_obj).ok();
            q.set("write", write).ok();
            if let Some(state_obj) = state_obj {
                q.set("state", state_obj).ok();
            }

            let context_obj = Object::new(ctx.clone()).map_err(|e| BatchError::RunFailed {
                path: path_owned.clone(),
                message: e.to_string(),
            })?;
            context_obj.set("dt", dt).ok();

            let copy_in_us = copy_in_start.elapsed().as_micros() as u64;

            // ── run ───────────────────────────────────────────────────
            let run_start = Instant::now();
            let update_fn = script.update.clone().restore(&ctx).map_err(|e| {
                BatchError::RunFailed { path: path_owned.clone(), message: e.to_string() }
            })?;
            let _: rquickjs::Value = update_fn
                .call((context_obj, q))
                .map_err(|e| BatchError::RunFailed { path: path_owned.clone(), message: e.to_string() })?;
            let run_us = run_start.elapsed().as_micros() as u64;

            // ── copy-out ──────────────────────────────────────────────
            let copy_out_start = Instant::now();
            // Reborrow the underlying bytes — TypedArray::as_bytes returns
            // None only if the buffer was detached. Walking-skeleton scripts
            // do not detach.
            let px_bytes = px_handle.as_bytes().ok_or_else(|| BatchError::RunFailed {
                path: path_owned.clone(),
                message: "px ArrayBuffer detached".into(),
            })?;
            let py_bytes = py_handle.as_bytes().ok_or_else(|| BatchError::RunFailed {
                path: path_owned.clone(),
                message: "py ArrayBuffer detached".into(),
            })?;
            // SAFETY: `as_bytes` returns the raw bytes of an `f32` typed
            // array; len is `count * 4` and alignment is 4-byte (f32 over
            // ArrayBuffer guarantees alignment). We only read from the
            // resulting slice.
            let px_floats: &[f32] = unsafe {
                std::slice::from_raw_parts(px_bytes.as_ptr() as *const f32, count)
            };
            let py_floats: &[f32] = unsafe {
                std::slice::from_raw_parts(py_bytes.as_ptr() as *const f32, count)
            };
            position_x.copy_from_slice(px_floats);
            position_y.copy_from_slice(py_floats);
            // (dirty accounting below)

            // Slice 5.1: read state columns back the same way.
            for (i, handle) in state_handles.iter().enumerate() {
                let bytes = handle.as_bytes().ok_or_else(|| BatchError::RunFailed {
                    path: path_owned.clone(),
                    message: format!(
                        "state column `{}` ArrayBuffer detached",
                        state_schema[i].name
                    ),
                })?;
                // SAFETY: as for px/py above.
                let floats: &[f32] = unsafe {
                    std::slice::from_raw_parts(bytes.as_ptr() as *const f32, count)
                };
                state[i].copy_from_slice(floats);
            }
            let copy_out_us = copy_out_start.elapsed().as_micros() as u64;

            let bytes_per_col = std::mem::size_of_val(velocity_x) as u64;
            let state_cols = state_schema.len() as u64;
            let opt_cols = if accel.is_some() { 2u64 } else { 0 };
            counters.batch_copy_in_bytes += bytes_per_col * (4 + state_cols + opt_cols); // vx, vy, px, py + state in + accel in
            counters.batch_copy_out_bytes += bytes_per_col * (2 + state_cols); // px, py + state out
            // Slice 5.4b: walking-skeleton path always copies; mark all
            // would-be writeback bytes as dirty until the per-column
            // dirty mask is wired here too.
            counters.batch_dirty_bytes += bytes_per_col * (2 + state_cols);
            counters.batch_copy_in_us += copy_in_us;
            counters.batch_run_us += run_us;
            counters.batch_copy_out_us += copy_out_us;

            Ok(run_us)
        })?;

        Ok(elapsed_us)
    }

    /// Slice 5.3 generic dispatch: any subset of [`KNOWN_BATCH_COMPONENTS`]
    /// declared in `read` / `write` / `read_optional`. The dispatcher
    /// (`ecs::systems::run_batch_scripts`) is responsible for partitioning
    /// entities into archetype-uniform batches so every entry of
    /// `optional_reads[i].value` is either fully `Some` or fully `None`
    /// for this batch.
    ///
    /// All `reads`/`writes`/`optional_reads.value` x/y slices must have
    /// length `count` (taken from the first column found, or 0 → no-op).
    /// `state` columns must match the script's declared schema.
    /// Slice 5.4c: `write_dirty` is an optional output channel for the
    /// dispatcher. When `write_dirty.len() == writes.len()`, the runtime
    /// fills slot `i` with `true` iff the script left write column `i`'s
    /// x **or** y axis non-bit-equal to its pre-script value. The
    /// dispatcher uses this to skip the per-entity ECS scatter for
    /// clean columns. Pass `&mut []` to opt out (the runtime then does
    /// not record per-column dirty info; only the global
    /// `batch_dirty_bytes` counter is updated).
    pub fn run_batch(
        &self,
        path: &str,
        dt: f32,
        reads: &[BatchReadColumn<'_>],
        writes: &mut [BatchWriteColumn<'_>],
        optional_reads: &[BatchOptionalColumn<'_>],
        state: &mut [Vec<f32>],
        write_dirty: &mut [bool],
        counters: &mut FrameCounters,
    ) -> Result<u64, BatchError> {
        debug_assert!(
            write_dirty.is_empty() || write_dirty.len() == writes.len(),
            "write_dirty must be empty or match writes.len()"
        );
        // Slice 5.4c: clear caller-supplied dirty buffer; we only ever
        // set entries to true.
        for d in write_dirty.iter_mut() {
            *d = false;
        }
        // Determine batch size from the first available column.
        let count = reads
            .first()
            .map(|c| c.x.len())
            .or_else(|| writes.first().map(|c| c.x.len()))
            .or_else(|| optional_reads.iter().find_map(|c| c.value.map(|(x, _)| x.len())))
            .unwrap_or(0);
        if count == 0 {
            return Ok(0);
        }
        for c in reads {
            debug_assert_eq!(c.x.len(), count, "read column `{}` x mismatch", c.name);
            debug_assert_eq!(c.y.len(), count, "read column `{}` y mismatch", c.name);
        }
        for c in writes.iter() {
            debug_assert_eq!(c.x.len(), count, "write column `{}` x mismatch", c.name);
            debug_assert_eq!(c.y.len(), count, "write column `{}` y mismatch", c.name);
        }
        for c in optional_reads {
            if let Some((x, y)) = c.value {
                debug_assert_eq!(x.len(), count, "opt column `{}` x mismatch", c.name);
                debug_assert_eq!(y.len(), count, "opt column `{}` y mismatch", c.name);
            }
        }

        let script = self
            .scripts
            .get(path)
            .ok_or_else(|| BatchError::MissingUpdate { path: path.to_string() })?;

        let state_schema: &[BatchStateField] =
            script.spec.as_ref().map(|s| s.state.as_slice()).unwrap_or(&[]);
        debug_assert_eq!(state.len(), state_schema.len());

        let path_owned = path.to_string();
        let opt_present_count = optional_reads.iter().filter(|c| c.value.is_some()).count();

        let elapsed_us = self.context.with(|ctx| -> Result<u64, BatchError> {
            let copy_in_start = Instant::now();
            let mk_err = |e: rquickjs::Error| BatchError::RunFailed {
                path: path_owned.clone(),
                message: e.to_string(),
            };

            // ── read columns → q.read.<Name> = { x, y } ──
            let read_obj = Object::new(ctx.clone()).map_err(mk_err)?;
            for c in reads {
                let xa = TypedArray::<f32>::new_copy(ctx.clone(), c.x).map_err(mk_err)?;
                let ya = TypedArray::<f32>::new_copy(ctx.clone(), c.y).map_err(mk_err)?;
                let inner = Object::new(ctx.clone()).map_err(mk_err)?;
                inner.set("x", xa).ok();
                inner.set("y", ya).ok();
                read_obj.set(c.name, inner).ok();
            }

            // ── write columns → q.write.<Name> = { x, y }; keep handles ──
            let write_obj = Object::new(ctx.clone()).map_err(mk_err)?;
            // Stable index → (x_handle, y_handle) for copy-out.
            let mut write_handles: Vec<(TypedArray<'_, f32>, TypedArray<'_, f32>)> =
                Vec::with_capacity(writes.len());
            for c in writes.iter() {
                let xa = TypedArray::<f32>::new_copy(ctx.clone(), &*c.x).map_err(mk_err)?;
                let ya = TypedArray::<f32>::new_copy(ctx.clone(), &*c.y).map_err(mk_err)?;
                let xh = xa.clone();
                let yh = ya.clone();
                let inner = Object::new(ctx.clone()).map_err(mk_err)?;
                inner.set("x", xa).ok();
                inner.set("y", ya).ok();
                write_obj.set(c.name, inner).ok();
                write_handles.push((xh, yh));
            }

            // ── optional reads → q.read_optional.<Name> = { x, y } | null ──
            let opt_obj = Object::new(ctx.clone()).map_err(mk_err)?;
            for c in optional_reads {
                if let Some((x, y)) = c.value {
                    let xa = TypedArray::<f32>::new_copy(ctx.clone(), x).map_err(mk_err)?;
                    let ya = TypedArray::<f32>::new_copy(ctx.clone(), y).map_err(mk_err)?;
                    let inner = Object::new(ctx.clone()).map_err(mk_err)?;
                    inner.set("x", xa).ok();
                    inner.set("y", ya).ok();
                    opt_obj.set(c.name, inner).ok();
                } else {
                    opt_obj
                        .set(c.name, rquickjs::Value::new_null(ctx.clone()))
                        .ok();
                }
            }

            // ── state columns → q.state.<field> = Float32Array ──
            let mut state_handles: Vec<TypedArray<'_, f32>> =
                Vec::with_capacity(state_schema.len());
            let state_obj = if state_schema.is_empty() {
                None
            } else {
                let obj = Object::new(ctx.clone()).map_err(mk_err)?;
                for (field, col) in state_schema.iter().zip(state.iter()) {
                    let arr =
                        TypedArray::<f32>::new_copy(ctx.clone(), col.as_slice()).map_err(mk_err)?;
                    let h = arr.clone();
                    obj.set(field.name.as_str(), arr).ok();
                    state_handles.push(h);
                }
                Some(obj)
            };

            let q = Object::new(ctx.clone()).map_err(mk_err)?;
            q.set("count", count as u32).ok();
            q.set("read", read_obj).ok();
            q.set("write", write_obj).ok();
            q.set("read_optional", opt_obj).ok();
            if let Some(s) = state_obj {
                q.set("state", s).ok();
            }

            let context_obj = Object::new(ctx.clone()).map_err(mk_err)?;
            context_obj.set("dt", dt).ok();

            let copy_in_us = copy_in_start.elapsed().as_micros() as u64;

            let run_start = Instant::now();
            let update_fn = script.update.clone().restore(&ctx).map_err(mk_err)?;
            let _: rquickjs::Value = update_fn.call((context_obj, q)).map_err(mk_err)?;
            let run_us = run_start.elapsed().as_micros() as u64;

            // ── copy-out write columns + state ──
            // Slice 5.4b: per-column bulk dirty mask. The pre-script
            // ECS values are still in `writes[i].x` / `state[i]`
            // (cp_in copied them into the typed array, the script
            // mutated the typed array, the ECS slice is unchanged).
            // A whole-column `==` compare is one `memcmp` over
            // `count * 4` bytes — cheap relative to the
            // `copy_from_slice` it can elide.
            let copy_out_start = Instant::now();
            let mut dirty_bytes: u64 = 0;
            let force = dirty_disabled();
            for (i, (xh, yh)) in write_handles.iter().enumerate() {
                let xb = xh.as_bytes().ok_or_else(|| BatchError::RunFailed {
                    path: path_owned.clone(),
                    message: format!("write column `{}` x detached", writes[i].name),
                })?;
                let yb = yh.as_bytes().ok_or_else(|| BatchError::RunFailed {
                    path: path_owned.clone(),
                    message: format!("write column `{}` y detached", writes[i].name),
                })?;
                // SAFETY: f32 typed array bytes; len = count*4; alignment 4.
                let xs: &[f32] =
                    unsafe { std::slice::from_raw_parts(xb.as_ptr() as *const f32, count) };
                let ys: &[f32] =
                    unsafe { std::slice::from_raw_parts(yb.as_ptr() as *const f32, count) };
                let mut col_dirty = false;
                if force || writes[i].x != xs {
                    writes[i].x.copy_from_slice(xs);
                    dirty_bytes += std::mem::size_of_val(xs) as u64;
                    col_dirty = true;
                }
                if force || writes[i].y != ys {
                    writes[i].y.copy_from_slice(ys);
                    dirty_bytes += std::mem::size_of_val(ys) as u64;
                    col_dirty = true;
                }
                if let Some(slot) = write_dirty.get_mut(i) {
                    *slot = col_dirty;
                }
            }
            for (i, h) in state_handles.iter().enumerate() {
                let bytes = h.as_bytes().ok_or_else(|| BatchError::RunFailed {
                    path: path_owned.clone(),
                    message: format!(
                        "state column `{}` detached",
                        state_schema[i].name
                    ),
                })?;
                let floats: &[f32] =
                    unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const f32, count) };
                if force || state[i].as_slice() != floats {
                    state[i].copy_from_slice(floats);
                    dirty_bytes += std::mem::size_of_val(floats) as u64;
                }
            }
            let copy_out_us = copy_out_start.elapsed().as_micros() as u64;

            // Accounting: each column = 2 axes (x,y) × `count * sizeof::<f32>()`.
            let bytes_per_axis = (count * std::mem::size_of::<f32>()) as u64;
            let read_axes = 2 * reads.len() as u64;
            let write_axes = 2 * writes.len() as u64;
            let opt_axes = 2 * opt_present_count as u64;
            let state_cols = state_schema.len() as u64;
            counters.batch_copy_in_bytes +=
                bytes_per_axis * (read_axes + write_axes + opt_axes + state_cols);
            counters.batch_copy_out_bytes += bytes_per_axis * (write_axes + state_cols);
            // Slice 5.4b: actual bytes written back (post-skip).
            counters.batch_dirty_bytes += dirty_bytes;
            counters.batch_copy_in_us += copy_in_us;
            counters.batch_run_us += run_us;
            counters.batch_copy_out_us += copy_out_us;

            Ok(run_us)
        })?;

        Ok(elapsed_us)
    }
}

/// Slice 5.4b A/B toggle: when env `JENGINE_BATCH_DIRTY_DISABLE=1`,
/// the dirty-mask `==` compare is bypassed and writebacks always copy
/// (mimicking slice 5.4a behaviour). Used only for back-to-back
/// baseline captures. Cached on first read.
fn dirty_disabled() -> bool {
    use std::sync::OnceLock;
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("JENGINE_BATCH_DIRTY_DISABLE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

/// Pull `system.query.{read,write}` (string arrays) and `system.tick_rate`
/// (number) out of the wrapper's return object. Returns `None` if `system`
/// is missing or has no `query` table — callers treat that as the legacy
/// implicit Velocity2D → Position2D contract.
fn parse_system_spec(result: &Object<'_>) -> Option<BatchSystemSpec> {
    let system: Object = result.get("system").ok()?;
    let query: Object = system.get("query").ok()?;
    let read = read_string_array(&query, "read").unwrap_or_default();
    let write = read_string_array(&query, "write").unwrap_or_default();
    let read_optional = read_string_array(&query, "read_optional").unwrap_or_default();
    let tick_rate: Option<u32> = system
        .get::<_, f64>("tick_rate")
        .ok()
        .and_then(|n| if n.is_finite() && n > 0.0 { Some(n.round() as u32) } else { None });
    let state = parse_state_schema(&query);
    Some(BatchSystemSpec { read, write, read_optional, tick_rate, state })
}

/// Parse `system.query.state` (an object map of `{ name: { type, default } }`)
/// into a Vec of [`BatchStateField`]. Slice 5.1: `type: "f32"` only.
/// Unknown types are logged and skipped (warn-and-continue keeps the
/// engine running while users iterate on schemas; future slices upgrade
/// this to a hard error once `entity` / `u8` / vector types ship).
fn parse_state_schema(query: &Object<'_>) -> Vec<BatchStateField> {
    let Ok(state_obj): Result<Object, _> = query.get("state") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in state_obj.props::<String, Object>().flatten() {
        let (name, field) = entry;
        let ty: String = field.get("type").unwrap_or_else(|_| "f32".to_string());
        if ty != "f32" {
            log::warn!(
                "[BatchScript] state field `{}` has unsupported type `{}` (Slice 5.1 supports `f32` only) — skipping",
                name,
                ty,
            );
            continue;
        }
        let default: f32 = field.get::<_, f64>("default").unwrap_or(0.0) as f32;
        out.push(BatchStateField { name, default });
    }
    // Property iteration order in QuickJS is insertion order, but we
    // sort by name to make the on-disk per-entity state vec layout
    // deterministic across rebuilds / minor source edits that reorder
    // declarations. Dispatcher relies on this ordering being stable.
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn read_string_array(obj: &Object<'_>, key: &str) -> Option<Vec<String>> {
    use rquickjs::Array;
    let arr: Array = obj.get(key).ok()?;
    let len = arr.len();
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        if let Ok(s) = arr.get::<String>(i) {
            out.push(s);
        }
    }
    Some(out)
}

/// Cross-module entry point for the batch validator. The Lua runtime
/// in `batch_lua.rs` calls this via a thin wrapper so JS and Lua paths
/// enforce identical Slice 5.3 rules. Underscore-prefixed because it's
/// not part of the stable public API — callers should use `load()`.
#[doc(hidden)]
pub fn __validate_batch_spec(path: &str, spec: &BatchSystemSpec) -> Result<(), BatchError> {
    validate_walking_skeleton_spec(path, spec)
}

/// Slice 5.3 validator: allow any subset of [`KNOWN_BATCH_COMPONENTS`]
/// in `read` / `write` / `read_optional`, with two structural rules:
///
/// 1. Total entries across the three buckets ≤ [`MAX_BATCH_COLUMNS`].
/// 2. Each component name appears in exactly one bucket (a script that
///    wants read-modify-write puts the component in `write`; the
///    dispatcher pre-populates write columns with the current ECS
///    snapshot).
///
/// Unknown component names are a hard load-time error so typos can't
/// silently produce empty columns at runtime.
///
/// The legacy walking-skeleton contract (`read=["Velocity2D"]`,
/// `write=["Position2D"]`, optional `read_optional=["Acceleration2D"]`)
/// still passes — it's a strict subset of the new vocabulary.
fn validate_walking_skeleton_spec(path: &str, spec: &BatchSystemSpec) -> Result<(), BatchError> {
    use std::collections::HashMap as Map;
    let total = spec.read.len() + spec.write.len() + spec.read_optional.len();
    if total > MAX_BATCH_COLUMNS {
        return Err(BatchError::TooManyColumns {
            path: path.to_string(),
            read: spec.read.len(),
            write: spec.write.len(),
            read_optional: spec.read_optional.len(),
            total,
            max: MAX_BATCH_COLUMNS,
        });
    }
    let check_known = |name: &str, bucket: &'static str| -> Result<(), BatchError> {
        if KNOWN_BATCH_COMPONENTS.contains(&name) {
            Ok(())
        } else {
            Err(BatchError::UnknownComponent {
                path: path.to_string(),
                name: name.to_string(),
                bucket,
                known: KNOWN_BATCH_COMPONENTS.to_vec(),
            })
        }
    };
    for n in &spec.read { check_known(n, "read")?; }
    for n in &spec.write { check_known(n, "write")?; }
    for n in &spec.read_optional { check_known(n, "read_optional")?; }

    // Cross-bucket uniqueness: track which buckets each name lives in.
    let mut by_name: Map<&str, Vec<&'static str>> = Map::new();
    for n in &spec.read { by_name.entry(n.as_str()).or_default().push("read"); }
    for n in &spec.write { by_name.entry(n.as_str()).or_default().push("write"); }
    for n in &spec.read_optional { by_name.entry(n.as_str()).or_default().push("read_optional"); }
    for (name, buckets) in &by_name {
        if buckets.len() > 1 {
            return Err(BatchError::DuplicateComponent {
                path: path.to_string(),
                name: (*name).to_string(),
                buckets: buckets.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_and_runs_identity_update() {
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load("identity.js", "function update(ctx, q) { /* no-op */ }").unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![1.0_f32, 2.0, 3.0];
        let vy = vec![4.0_f32, 5.0, 6.0];
        let mut px = vec![10.0_f32, 20.0, 30.0];
        let mut py = vec![40.0_f32, 50.0, 60.0];
        rt.run_velocity_to_position(
            "identity.js",
            1.0 / 60.0,
            &vx,
            &vy,
            &mut px,
            &mut py,
            &mut [],
            None,
            &mut counters,
        )
        .unwrap();
        // No mutation in identity update — values come back unchanged.
        assert_eq!(px, vec![10.0, 20.0, 30.0]);
        assert_eq!(py, vec![40.0, 50.0, 60.0]);
        assert!(counters.batch_copy_in_bytes > 0);
        assert!(counters.batch_copy_out_bytes > 0);
    }

    #[test]
    fn integrates_velocity_into_position() {
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load(
            "mover.js",
            "function update(ctx, q) {
                const px = q.write.Position2D.x;
                const py = q.write.Position2D.y;
                const vx = q.read.Velocity2D.x;
                const vy = q.read.Velocity2D.y;
                for (let i = 0; i < q.count; i++) {
                    px[i] = px[i] + vx[i] * ctx.dt;
                    py[i] = py[i] + vy[i] * ctx.dt;
                }
            }",
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![10.0_f32, 20.0];
        let vy = vec![-5.0_f32, 0.0];
        let mut px = vec![0.0_f32, 100.0];
        let mut py = vec![0.0_f32, 100.0];
        rt.run_velocity_to_position("mover.js", 0.5, &vx, &vy, &mut px, &mut py, &mut [], None, &mut counters)
            .unwrap();
        assert!((px[0] - 5.0).abs() < 1e-5, "px[0]={}", px[0]);
        assert!((px[1] - 110.0).abs() < 1e-5, "px[1]={}", px[1]);
        assert!((py[0] - (-2.5)).abs() < 1e-5, "py[0]={}", py[0]);
        assert!((py[1] - 100.0).abs() < 1e-5, "py[1]={}", py[1]);
    }

    #[test]
    fn empty_count_is_a_noop() {
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load("noop.js", "function update(ctx, q) { throw new Error('should not run'); }")
            .unwrap();
        let mut counters = FrameCounters::default();
        let vx: Vec<f32> = vec![];
        let vy: Vec<f32> = vec![];
        let mut px: Vec<f32> = vec![];
        let mut py: Vec<f32> = vec![];
        // count == 0 short-circuits before the script runs.
        rt.run_velocity_to_position("noop.js", 0.016, &vx, &vy, &mut px, &mut py, &mut [], None, &mut counters)
            .unwrap();
        assert_eq!(counters.batch_copy_in_bytes, 0);
    }

    // ── Slice 4.2 ─────────────────────────────────────────────────────

    /// `module.exports.update = ...` style scripts load and run.
    #[test]
    fn loads_commonjs_style_export() {
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load(
            "cjs.js",
            "module.exports.update = function(ctx, q) {
                const px = q.write.Position2D.x;
                const vx = q.read.Velocity2D.x;
                for (let i = 0; i < q.count; i++) px[i] += vx[i] * ctx.dt;
            };
            module.exports.system = { query: { read: ['Velocity2D'], write: ['Position2D'] } };",
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![10.0_f32];
        let vy = vec![0.0_f32];
        let mut px = vec![1.0_f32];
        let mut py = vec![0.0_f32];
        rt.run_velocity_to_position("cjs.js", 0.5, &vx, &vy, &mut px, &mut py, &mut [], None, &mut counters)
            .unwrap();
        assert!((px[0] - 6.0).abs() < 1e-5, "px[0]={}", px[0]);
    }

    /// A script declaring an unknown component (e.g. read=["Health"]) is
    /// rejected at load time with `UnknownComponent`, NOT silently accepted.
    #[test]
    fn rejects_mismatched_system_spec() {
        let mut rt = BatchJsRuntime::new().unwrap();
        let err = rt
            .load(
                "bad.js",
                "function update(ctx, q) {}\n\
                 const system = { query: { read: ['Health'], write: ['Position2D'] } };",
            )
            .unwrap_err();
        match err {
            BatchError::UnknownComponent { name, bucket, .. } => {
                assert_eq!(name, "Health");
                assert_eq!(bucket, "read");
            }
            other => panic!("expected UnknownComponent, got {other:?}"),
        }
    }

    /// A script that omits `system` entirely is accepted (with a warn) and
    /// runs against the implicit Velocity2D → Position2D contract.
    /// Back-compat for scripts authored before Slice 4.2.
    #[test]
    fn accepts_script_without_system_header() {
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load("legacy.js", "function update(ctx, q) {}").unwrap();
        assert_eq!(rt.loaded_count(), 1);
    }

    /// Two distinct scripts can be loaded into one runtime and dispatched
    /// independently — each keeps its own `update` Persistent and its own
    /// view of `q`. Confirms multi-script dispatch isn't accidentally
    /// shared via the QuickJS global object.
    #[test]
    fn multi_script_dispatch_keeps_scripts_isolated() {
        let mut rt = BatchJsRuntime::new().unwrap();
        // Script A doubles px via velocity. Script B subtracts velocity.
        rt.load(
            "double.js",
            "function update(ctx, q) {
                const px = q.write.Position2D.x;
                const vx = q.read.Velocity2D.x;
                for (let i = 0; i < q.count; i++) px[i] = px[i] + vx[i];
            }",
        )
        .unwrap();
        rt.load(
            "subtract.js",
            "function update(ctx, q) {
                const px = q.write.Position2D.x;
                const vx = q.read.Velocity2D.x;
                for (let i = 0; i < q.count; i++) px[i] = px[i] - vx[i];
            }",
        )
        .unwrap();
        assert_eq!(rt.loaded_count(), 2);

        let mut counters = FrameCounters::default();
        let vx = vec![5.0_f32];
        let vy = vec![0.0_f32];

        let mut px_a = vec![10.0_f32];
        let mut py_a = vec![0.0_f32];
        rt.run_velocity_to_position("double.js", 1.0, &vx, &vy, &mut px_a, &mut py_a, &mut [], None, &mut counters)
            .unwrap();
        assert_eq!(px_a, vec![15.0]);

        let mut px_b = vec![10.0_f32];
        let mut py_b = vec![0.0_f32];
        rt.run_velocity_to_position("subtract.js", 1.0, &vx, &vy, &mut px_b, &mut py_b, &mut [], None, &mut counters)
            .unwrap();
        assert_eq!(px_b, vec![5.0]);
    }

    // ── Step 4 exit criterion: "stale views are safe" ────────────────

    /// A script that stashes a `q.write.Position2D.x` typed-array view
    /// on `globalThis` from frame N, and then writes garbage through
    /// that stale view on frame N+1, must not corrupt the ECS column
    /// for frame N+1. Per the Step 4 spec the per-call mirror buffers
    /// are freshly allocated each frame, so the stored view points at
    /// abandoned memory (still live as long as JS holds it, but no
    /// longer wired to ECS).
    #[test]
    fn stale_view_stored_across_frames_does_not_corrupt_ecs() {
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load(
            "stash.js",
            "function update(ctx, q) {
                if (globalThis.__stale) {
                    // Frame N+1: scribble through the OLD view from frame N.
                    const stale = globalThis.__stale;
                    for (let i = 0; i < stale.length; i++) stale[i] = 9999.0;
                }
                // Stash the CURRENT frame's write view for next frame.
                globalThis.__stale = q.write.Position2D.x;
                // Do real work on the current view too.
                const px = q.write.Position2D.x;
                const vx = q.read.Velocity2D.x;
                for (let i = 0; i < q.count; i++) px[i] = px[i] + vx[i] * ctx.dt;
            }",
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![10.0_f32, 20.0];
        let vy = vec![0.0_f32, 0.0];

        // Frame N: stashes view, integrates once.
        let mut px = vec![0.0_f32, 0.0];
        let mut py = vec![0.0_f32, 0.0];
        rt.run_velocity_to_position("stash.js", 1.0, &vx, &vy, &mut px, &mut py, &mut [], None, &mut counters)
            .unwrap();
        assert_eq!(px, vec![10.0, 20.0]);

        // Frame N+1: scribbles 9999.0 through the stale view, then
        // integrates again on the FRESH view. ECS column reflects the
        // fresh-view write, NOT the stale-view garbage.
        rt.run_velocity_to_position("stash.js", 1.0, &vx, &vy, &mut px, &mut py, &mut [], None, &mut counters)
            .unwrap();
        assert_eq!(
            px,
            vec![20.0, 40.0],
            "stale view from previous frame must not bleed into ECS column"
        );
    }

    // ── Slice 5.1: typed local state columns ──────────────────────────

    /// A script declaring a single `f32` state field gets it parsed into
    /// the schema with the declared default. Sort order is alphabetic
    /// regardless of declaration order in source.
    #[test]
    fn parses_state_schema_with_defaults() {
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load(
            "stateful.js",
            "function update(ctx, q) {}\n\
             const system = {\n\
               query: {\n\
                 read: ['Velocity2D'],\n\
                 write: ['Position2D'],\n\
                 state: {\n\
                   cooldown: { type: 'f32', default: 1.5 },\n\
                   accumulator: { type: 'f32', default: 0.0 }\n\
                 }\n\
               }\n\
             };",
        )
        .unwrap();
        let schema = rt.state_schema("stateful.js").expect("loaded");
        assert_eq!(schema.len(), 2);
        // Sorted alphabetically.
        assert_eq!(schema[0].name, "accumulator");
        assert_eq!(schema[0].default, 0.0);
        assert_eq!(schema[1].name, "cooldown");
        assert_eq!(schema[1].default, 1.5);
    }

    /// Scripts without state declare an empty schema; legacy scripts
    /// (no `system` header at all) also get the empty schema.
    #[test]
    fn empty_state_schema_for_legacy_and_stateless() {
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load("legacy.js", "function update(ctx, q) {}").unwrap();
        rt.load(
            "stateless.js",
            "function update(ctx, q) {}\n\
             const system = { query: { read: ['Velocity2D'], write: ['Position2D'] } };",
        )
        .unwrap();
        assert_eq!(rt.state_schema("legacy.js").unwrap().len(), 0);
        assert_eq!(rt.state_schema("stateless.js").unwrap().len(), 0);
        assert!(rt.state_schema("missing.js").is_none());
    }

    /// State columns survive across multiple invocations: the script
    /// increments `accumulator` by `dt` each frame; after 3 calls with
    /// dt=0.1 the value is 0.3 (within f32 precision).
    #[test]
    fn state_persists_across_invocations() {
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load(
            "ticker.js",
            "function update(ctx, q) {\n\
               const acc = q.state.accumulator;\n\
               for (let i = 0; i < q.count; i++) acc[i] += ctx.dt;\n\
             }\n\
             const system = {\n\
               query: {\n\
                 read: ['Velocity2D'],\n\
                 write: ['Position2D'],\n\
                 state: { accumulator: { type: 'f32', default: 0.0 } }\n\
               }\n\
             };",
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![0.0_f32, 0.0];
        let vy = vec![0.0_f32, 0.0];
        let mut px = vec![0.0_f32, 0.0];
        let mut py = vec![0.0_f32, 0.0];
        let mut state = vec![vec![0.0_f32, 0.0]];
        for _ in 0..3 {
            rt.run_velocity_to_position(
                "ticker.js",
                0.1,
                &vx,
                &vy,
                &mut px,
                &mut py,
                &mut state,
                None,
                &mut counters,
            )
            .unwrap();
        }
        assert!((state[0][0] - 0.3).abs() < 1e-5, "got {}", state[0][0]);
        assert!((state[0][1] - 0.3).abs() < 1e-5, "got {}", state[0][1]);
    }

    /// Scripts can read AND write state — flip-flop a sign every frame
    /// using a `mode` cooldown, and apply it to position.
    #[test]
    fn state_round_trip_drives_position() {
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load(
            "flip.js",
            "function update(ctx, q) {\n\
               const px = q.write.Position2D.x;\n\
               const sign = q.state.sign;\n\
               for (let i = 0; i < q.count; i++) {\n\
                 px[i] += sign[i];\n\
                 sign[i] = -sign[i];\n\
               }\n\
             }\n\
             const system = {\n\
               query: {\n\
                 read: ['Velocity2D'],\n\
                 write: ['Position2D'],\n\
                 state: { sign: { type: 'f32', default: 1.0 } }\n\
               }\n\
             };",
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![0.0_f32];
        let vy = vec![0.0_f32];
        let mut px = vec![0.0_f32];
        let mut py = vec![0.0_f32];
        let mut state = vec![vec![1.0_f32]]; // initialized to default
        // After 4 calls: +1, -1, +1, -1 => px == 0; sign back to +1.
        for _ in 0..4 {
            rt.run_velocity_to_position(
                "flip.js",
                0.0,
                &vx,
                &vy,
                &mut px,
                &mut py,
                &mut state,
                None,
                &mut counters,
            )
            .unwrap();
        }
        assert_eq!(px, vec![0.0]);
        assert_eq!(state[0], vec![1.0]);
    }

    // ── Slice 5.2: optional reads ─────────────────────────────────────

    /// A script declaring `read_optional: ['Acceleration2D']` is
    /// accepted and the schema is exposed via `read_optional_schema`.
    #[test]
    fn parses_read_optional_schema() {
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load(
            "opt.js",
            "function update(ctx, q) {}\n\
             const system = { query: {\n\
               read: ['Velocity2D'], write: ['Position2D'],\n\
               read_optional: ['Acceleration2D']\n\
             } };",
        )
        .unwrap();
        let schema = rt.read_optional_schema("opt.js").expect("loaded");
        assert_eq!(schema, &["Acceleration2D".to_string()]);
        // Scripts without read_optional get an empty slice.
        rt.load("none.js", "function update(ctx, q) {}").unwrap();
        assert_eq!(rt.read_optional_schema("none.js").unwrap().len(), 0);
        assert!(rt.read_optional_schema("missing.js").is_none());
    }

    /// A script declaring an unknown `read_optional` component is
    /// rejected at load time with `UnknownComponent` carrying the
    /// offending name + bucket label.
    #[test]
    fn rejects_unknown_optional_component() {
        let mut rt = BatchJsRuntime::new().unwrap();
        let err = rt
            .load(
                "bad_opt.js",
                "function update(ctx, q) {}\n\
                 const system = { query: {\n\
                   read: ['Velocity2D'], write: ['Position2D'],\n\
                   read_optional: ['Health']\n\
                 } };",
            )
            .unwrap_err();
        match err {
            BatchError::UnknownComponent { name, bucket, .. } => {
                assert_eq!(name, "Health");
                assert_eq!(bucket, "read_optional");
            }
            other => panic!("expected UnknownComponent, got {other:?}"),
        }
    }

    /// Slice 5.3: a spec exceeding [`MAX_BATCH_COLUMNS`] (12) total
    /// across read/write/read_optional is rejected at load time with
    /// `TooManyColumns` carrying the breakdown.
    #[test]
    fn rejects_spec_over_column_cap() {
        // 8 reads + 8 writes overlap a vocabulary that has 8 entries; we
        // can't actually duplicate names (would hit DuplicateComponent
        // first), so build 7 reads + 1 write + 5 read_optional = 13
        // total. Use Wind2D in read_optional to keep all names known.
        let src = "function update(ctx, q) {}\n\
                   const system = { query: {\n\
                     read: ['Position2D','Velocity2D','Acceleration2D','Drag2D','Force2D','Wind2D','Impulse2D'],\n\
                     write: ['Torque2D'],\n\
                     read_optional: []\n\
                   } };";
        // 7+1+0 = 8, fine. Build a bigger one by adding write columns
        // with bogus names — but those would hit UnknownComponent
        // first. Instead we synthesize via read_optional duplicates of
        // unknown names? Unknown also fires first. So use a known name
        // appearing in BOTH read and write — that fires
        // DuplicateComponent first. The cap is enforceable in practice
        // only when the vocabulary has > 12 entries, which it doesn't
        // today. Verify the cap path with a direct call to the
        // validator instead.
        let _ = src;
        use super::__validate_batch_spec;
        let spec = BatchSystemSpec {
            read: (0..7).map(|i| format!("R{i}")).collect(),
            write: (0..6).map(|i| format!("W{i}")).collect(),
            read_optional: vec![],
            tick_rate: None,
            state: vec![],
        };
        let err = __validate_batch_spec("cap.js", &spec).unwrap_err();
        match err {
            BatchError::TooManyColumns { read, write, read_optional, total, max, .. } => {
                assert_eq!(read, 7);
                assert_eq!(write, 6);
                assert_eq!(read_optional, 0);
                assert_eq!(total, 13);
                assert_eq!(max, MAX_BATCH_COLUMNS);
            }
            other => panic!("expected TooManyColumns, got {other:?}"),
        }
    }

    /// Slice 5.3: a name listed in both `read` and `write` is a load-time
    /// error (read-modify-write goes in `write` only — the dispatcher
    /// pre-fills writes with the current ECS snapshot).
    #[test]
    fn rejects_duplicate_component_across_buckets() {
        let mut rt = BatchJsRuntime::new().unwrap();
        let err = rt
            .load(
                "dup.js",
                "function update(ctx, q) {}\n\
                 const system = { query: {\n\
                   read: ['Velocity2D'],\n\
                   write: ['Velocity2D','Position2D']\n\
                 } };",
            )
            .unwrap_err();
        match err {
            BatchError::DuplicateComponent { name, buckets, .. } => {
                assert_eq!(name, "Velocity2D");
                assert!(buckets.contains(&"read") && buckets.contains(&"write"));
            }
            other => panic!("expected DuplicateComponent, got {other:?}"),
        }
    }

    /// Slice 5.3: a multi-column spec (3 reads + 2 writes) loads cleanly
    /// and runs end-to-end through `run_batch`, with all write columns
    /// scattered back into the caller's slices.
    #[test]
    fn run_batch_3r_2w_roundtrip() {
        use super::{BatchOptionalColumn, BatchReadColumn, BatchWriteColumn};
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load(
            "multi.js",
            // read = Acceleration2D, Drag2D, Wind2D
            // write = Velocity2D, Position2D
            // body: vel.x = 10*ax + drag.x + wind.x; pos.x = vel.x * dt
            "function update(ctx, q) {\n\
               const n = q.count;\n\
               const ax = q.read.Acceleration2D.x;\n\
               const dx = q.read.Drag2D.x;\n\
               const wx = q.read.Wind2D.x;\n\
               const vx = q.write.Velocity2D.x;\n\
               const px = q.write.Position2D.x;\n\
               for (let i=0;i<n;i++) {\n\
                 vx[i] = 10.0*ax[i] + dx[i] + wx[i];\n\
                 px[i] = vx[i] * ctx.dt;\n\
               }\n\
             }\n\
             const system = { query: {\n\
               read: ['Acceleration2D','Drag2D','Wind2D'],\n\
               write: ['Velocity2D','Position2D']\n\
             } };",
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let ax = vec![1.0_f32, 2.0];
        let ay = vec![0.0_f32, 0.0];
        let dx = vec![3.0_f32, 4.0];
        let dy = vec![0.0_f32, 0.0];
        let wx = vec![5.0_f32, 6.0];
        let wy = vec![0.0_f32, 0.0];
        let mut vx = vec![0.0_f32, 0.0];
        let mut vy = vec![0.0_f32, 0.0];
        let mut px = vec![0.0_f32, 0.0];
        let mut py = vec![0.0_f32, 0.0];
        let reads = vec![
            BatchReadColumn { name: "Acceleration2D", x: &ax, y: &ay },
            BatchReadColumn { name: "Drag2D", x: &dx, y: &dy },
            BatchReadColumn { name: "Wind2D", x: &wx, y: &wy },
        ];
        // Build writes by iterating to satisfy borrow checker.
        {
            let mut writes = vec![
                BatchWriteColumn { name: "Velocity2D", x: vx.as_mut_slice(), y: vy.as_mut_slice() },
                BatchWriteColumn { name: "Position2D", x: px.as_mut_slice(), y: py.as_mut_slice() },
            ];
            let opts: Vec<BatchOptionalColumn<'_>> = vec![];
            rt.run_batch("multi.js", 0.5, &reads, &mut writes, &opts, &mut [], &mut [], &mut counters)
                .unwrap();
        }
        // vx[0] = 10*1 + 3 + 5 = 18; px[0] = 18*0.5 = 9
        assert!((vx[0] - 18.0).abs() < 1e-5);
        assert!((px[0] - 9.0).abs() < 1e-5);
        // vx[1] = 10*2 + 4 + 6 = 30; px[1] = 30*0.5 = 15
        assert!((vx[1] - 30.0).abs() < 1e-5);
        assert!((px[1] - 15.0).abs() < 1e-5);
        // Counters: 3 read cols × 2 axes + 2 write cols × 2 axes = 10 axes in,
        // 2 write cols × 2 axes = 4 axes out. bytes_per_axis = 2 entries × 4 = 8.
        assert_eq!(counters.batch_copy_in_bytes, 10 * 8);
        assert_eq!(counters.batch_copy_out_bytes, 4 * 8);
    }

    /// When `accel` is `Some`, the script sees `q.read_optional.Acceleration2D`
    /// as a populated object and can integrate it into velocity. The
    /// counter for `batch_copy_in_bytes` includes the two extra columns.
    #[test]
    fn runtime_with_accel_uses_acceleration() {
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load(
            "use_accel.js",
            "function update(ctx, q) {\n\
               const px = q.write.Position2D.x;\n\
               const vx = q.read.Velocity2D.x;\n\
               const a = q.read_optional.Acceleration2D;\n\
               if (a) {\n\
                 for (let i = 0; i < q.count; i++) px[i] += (vx[i] + a.x[i]) * ctx.dt;\n\
               } else {\n\
                 for (let i = 0; i < q.count; i++) px[i] += vx[i] * ctx.dt;\n\
               }\n\
             }\n\
             const system = { query: {\n\
               read: ['Velocity2D'], write: ['Position2D'],\n\
               read_optional: ['Acceleration2D']\n\
             } };",
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![1.0_f32, 2.0];
        let vy = vec![0.0_f32, 0.0];
        let mut px = vec![0.0_f32, 0.0];
        let mut py = vec![0.0_f32, 0.0];
        let ax = vec![10.0_f32, 100.0];
        let ay = vec![0.0_f32, 0.0];
        rt.run_velocity_to_position(
            "use_accel.js", 1.0, &vx, &vy, &mut px, &mut py, &mut [],
            Some((&ax, &ay)), &mut counters,
        )
        .unwrap();
        assert!((px[0] - 11.0).abs() < 1e-5, "px[0]={}", px[0]);
        assert!((px[1] - 102.0).abs() < 1e-5, "px[1]={}", px[1]);
        // 6 columns in (vx, vy, px, py + ax, ay), 2 columns out.
        let bpc = (2 * std::mem::size_of::<f32>()) as u64;
        assert_eq!(counters.batch_copy_in_bytes, bpc * 6);
        assert_eq!(counters.batch_copy_out_bytes, bpc * 2);
    }

    /// When `accel` is `None`, the script sees `q.read_optional.Acceleration2D == null`
    /// and the script's else-branch runs. Counters reflect 4 in-columns.
    #[test]
    fn runtime_without_accel_skips_field() {
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load(
            "use_accel.js",
            "function update(ctx, q) {\n\
               const px = q.write.Position2D.x;\n\
               const vx = q.read.Velocity2D.x;\n\
               const a = q.read_optional.Acceleration2D;\n\
               if (a) { throw new Error('should be null'); }\n\
               for (let i = 0; i < q.count; i++) px[i] += vx[i] * ctx.dt;\n\
             }\n\
             const system = { query: {\n\
               read: ['Velocity2D'], write: ['Position2D'],\n\
               read_optional: ['Acceleration2D']\n\
             } };",
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![3.0_f32];
        let vy = vec![0.0_f32];
        let mut px = vec![0.0_f32];
        let mut py = vec![0.0_f32];
        rt.run_velocity_to_position(
            "use_accel.js", 2.0, &vx, &vy, &mut px, &mut py, &mut [],
            None, &mut counters,
        )
        .unwrap();
        assert!((px[0] - 6.0).abs() < 1e-5, "px[0]={}", px[0]);
        let bpc = std::mem::size_of::<f32>() as u64;
        assert_eq!(counters.batch_copy_in_bytes, bpc * 4);
    }

    // ── Slice 5.4c: per-column write_dirty out-channel ──────────────

    /// A noop script must leave every entry in `write_dirty` `false`,
    /// signalling the dispatcher that it can skip the per-entity scatter
    /// for every write column.
    #[test]
    fn write_dirty_all_false_for_noop_script() {
        use super::{BatchReadColumn, BatchWriteColumn};
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load(
            "noop.js",
            "function update(ctx, q) { /* writes nothing */ }\n\
             const system = { query: { read: ['Velocity2D'], write: ['Position2D','Acceleration2D'] } };",
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![1.0_f32, 2.0, 3.0];
        let vy = vec![0.0_f32, 0.0, 0.0];
        let mut px = vec![10.0_f32, 20.0, 30.0];
        let mut py = vec![0.0_f32, 0.0, 0.0];
        let mut ax = vec![0.0_f32, 0.0, 0.0];
        let mut ay = vec![0.0_f32, 0.0, 0.0];
        let reads = vec![BatchReadColumn { name: "Velocity2D", x: &vx, y: &vy }];
        {
            let mut writes = vec![
                BatchWriteColumn { name: "Position2D", x: px.as_mut_slice(), y: py.as_mut_slice() },
                BatchWriteColumn { name: "Acceleration2D", x: ax.as_mut_slice(), y: ay.as_mut_slice() },
            ];
            let mut dirty = [true, true]; // pre-seeded with `true` to prove the runtime clears them
            rt.run_batch("noop.js", 0.5, &reads, &mut writes, &[], &mut [], &mut dirty, &mut counters)
                .unwrap();
            assert_eq!(dirty, [false, false], "noop must leave all columns clean");
        }
    }

    /// A script that only writes the first write column must mark only
    /// that slot dirty; the second column stays clean.
    #[test]
    fn write_dirty_marks_only_touched_column() {
        use super::{BatchReadColumn, BatchWriteColumn};
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load(
            "partial.js",
            "function update(ctx, q) {\n\
               const px = q.write.Position2D.x;\n\
               for (let i=0;i<q.count;i++) px[i] = 999.0;\n\
             }\n\
             const system = { query: { read: ['Velocity2D'], write: ['Position2D','Acceleration2D'] } };",
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![0.0_f32; 4];
        let vy = vec![0.0_f32; 4];
        let mut px = vec![1.0_f32; 4];
        let mut py = vec![2.0_f32; 4];
        let mut ax = vec![3.0_f32; 4];
        let mut ay = vec![4.0_f32; 4];
        let reads = vec![BatchReadColumn { name: "Velocity2D", x: &vx, y: &vy }];
        {
            let mut writes = vec![
                BatchWriteColumn { name: "Position2D", x: px.as_mut_slice(), y: py.as_mut_slice() },
                BatchWriteColumn { name: "Acceleration2D", x: ax.as_mut_slice(), y: ay.as_mut_slice() },
            ];
            let mut dirty = [false, false];
            rt.run_batch("partial.js", 1.0, &reads, &mut writes, &[], &mut [], &mut dirty, &mut counters)
                .unwrap();
            assert_eq!(dirty, [true, false], "only Position2D was written");
        }
        assert_eq!(px, vec![999.0_f32; 4]);
        assert_eq!(ax, vec![3.0_f32; 4], "Acceleration2D must remain untouched");
        assert_eq!(ay, vec![4.0_f32; 4], "Acceleration2D must remain untouched");
    }

    /// Empty `write_dirty` slice means the caller opted out — runtime
    /// must not panic and global counters still advance.
    #[test]
    fn write_dirty_empty_slice_is_opt_out() {
        use super::{BatchReadColumn, BatchWriteColumn};
        let mut rt = BatchJsRuntime::new().unwrap();
        rt.load(
            "noop.js",
            "function update(ctx, q) {}\n\
             const system = { query: { read: ['Velocity2D'], write: ['Position2D'] } };",
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![0.0_f32; 2];
        let vy = vec![0.0_f32; 2];
        let mut px = vec![0.0_f32; 2];
        let mut py = vec![0.0_f32; 2];
        let reads = vec![BatchReadColumn { name: "Velocity2D", x: &vx, y: &vy }];
        let mut writes = vec![BatchWriteColumn {
            name: "Position2D", x: px.as_mut_slice(), y: py.as_mut_slice(),
        }];
        rt.run_batch("noop.js", 0.5, &reads, &mut writes, &[], &mut [], &mut [], &mut counters)
            .unwrap();
        assert!(counters.batch_copy_out_bytes > 0);
    }
}
