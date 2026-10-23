//! Slice 5.5b.1: batch script worker pool — channel-based + direct
//! backends behind one main-thread API.
//!
//! ### Background
//!
//! - Slice 5.5a introduced [`BatchScriptPool`] as a thin wrapper
//!   holding `BatchJsRuntime` + `Option<BatchLuaRuntime>` directly. No
//!   threading, no channels — purely structural to give the dispatcher
//!   one owner to grow into.
//! - Slice 5.5b.1 (this file) keeps the public name and surface but
//!   adds a second backend that runs each runtime on its own dedicated
//!   thread, communicating via crossbeam channels. The Direct backend
//!   stays for benchmarking and as the fallback path on worker init
//!   failure (per the locked Q&A decision).
//!
//! ### Why two backends?
//!
//! The Direct backend is the **baseline (a)** in the 5.5b.1 A/B harness
//! — it's literally the 5.4c code path captured fresh. The Pooled
//! backend is **(b)** — the new code path. The delta between them at
//! N=1 worker is the *pure channel overhead*, the number that gates
//! whether 5.5b.2 (N>1 worker pool) is worth pursuing. The two
//! backends share one dispatcher so the comparison is bit-identical
//! everywhere except where the channel itself is.
//!
//! ### Why channels and not the `mlua` `send` feature?
//!
//! Flipping mlua to `["luau", "send"]` would have cascaded
//! `Send + 'static` constraints across the per-entity bridge
//! (`LuauAdapter`, `BridgeState`, `F32Buffer`) — code unrelated to
//! batch parallelism. Worker-thread + channel keeps non-`Send`
//! runtimes pinned to their owning thread; only the request/response
//! payloads cross the boundary, and they're trivially `Send`. See
//! [`docs/scripting-performance-lessons.md`] subsection
//! "Why dedicated-worker-thread over mlua send" for the full argument.

#![cfg(all(not(target_arch = "wasm32"), any(feature = "js-adapter-host", feature = "native")))]

mod channel;
mod worker;

use std::collections::{HashMap, HashSet};

use log::{info, warn};

use crate::profiling::FrameCounters;
use crate::scripting::batch::{BatchError, BatchJsRuntime};
#[cfg(feature = "native")]
use crate::scripting::batch_lua::BatchLuaRuntime;
// Public because `load_lua` takes it: a caller outside this module has to be
// able to name the argument.
pub use crate::scripting::batch_lua::BatchLuaBufferMode;

pub use channel::{
    payload_bytes_in, response_bytes, OwnedOptionalColumn, OwnedReadColumn, OwnedWriteColumn,
    SharedOptionalColumn, SharedReadColumn,
};
pub use worker::WorkerKind;

/// How a [`BatchScriptPool`] runs its batch runtimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolMode {
    /// Slice 5.5a behaviour: runtimes live in the pool struct,
    /// `run_batch` is called synchronously on the main thread. Used as
    /// the (a) baseline in the 5.5b.1 channel-overhead A/B and as the
    /// fallback on worker init failure.
    Direct,
    /// Slice 5.5b.1 behaviour: each runtime lives on its own
    /// dedicated worker thread; calls cross a crossbeam channel.
    Pooled,
}

/// Cached schemas for one loaded script. Populated at load time so the
/// dispatcher's per-frame `is_loaded` / `*_schema` queries stay
/// synchronous in both backends — no per-frame round-trip just to
/// fetch static metadata.
#[derive(Debug, Clone, Default)]
struct CachedSchema {
    read: Vec<String>,
    write: Vec<String>,
    read_optional: Vec<String>,
}

/// JS-runtime backend storage.
enum JsBackend {
    Direct(BatchJsRuntime),
    /// Slice 5.5b.2: vector of N>=1 worker threads, each owning its
    /// own `BatchJsRuntime`. Chunked dispatch splits one batch call
    /// across all live workers in parallel; merge happens in
    /// chunk-id (== worker index) order so results are deterministic.
    /// Workers that die (panic or channel disconnect) are removed
    /// from the vec — if it ever becomes empty the next dispatch
    /// returns an init error and the dispatcher logs + skips.
    Pooled(Vec<worker::BatchScriptWorker>),
}

#[cfg(feature = "native")]
enum LuaBackend {
    Direct(BatchLuaRuntime),
    Pooled(Vec<worker::BatchScriptWorker>),
}

/// Wraps the batch script runtimes the dispatcher consumes.
///
/// 5.5b.1 invariants:
///
/// - Holds at most one JS backend (`worker_count` for JS = 1) and at
///   most one Lua backend.
/// - All schema queries (`*_loaded`, `*_read_schema`, etc.) hit a
///   main-thread cache — both backends populate the cache at load.
/// - Counters surfaced per frame:
///     * `script_worker_count` — how many workers the pool owns
///       (Direct = 0 worker threads, Pooled = 1 per active backend).
///     * `vm_memory_bytes_per_worker` — high-water VM memory across
///       workers this frame. Returns 0 in 5.5b.1 (real measurement
///       lands in 5.5c).
///     * `batch_workers_active` — how many backends are currently
///       alive (and so capable of accepting work).
///     * `batch_payload_clone_bytes` / `batch_response_bytes` /
///       `batch_worker_dispatch_us` — per-frame channel cost.
pub struct BatchScriptPool {
    mode: PoolMode,
    js: JsBackend,
    #[cfg(feature = "native")]
    lua: Option<LuaBackend>,
    /// Loaded-script schema cache, keyed by `(WorkerKind, path)`.
    schemas: HashMap<(WorkerKind, String), CachedSchema>,
    /// Mirror of `schemas.keys()` for fast `is_*_loaded` queries.
    loaded: HashSet<(WorkerKind, String)>,
    #[cfg(feature = "native")]
    lua_buffer_mode: Option<BatchLuaBufferMode>,
    /// Slice 5.5b.2: target worker count per language. The Lua
    /// backend reads this when it lazily spawns its worker vector on
    /// first `load_lua`. Always 1 in Direct mode (the value isn't
    /// used). Always `>= 1`.
    #[cfg(feature = "native")]
    workers_per_kind: u32,
    /// Slice 5.5c — deterministic-PRNG scene seed. Controls the
    /// stream that `Math.random()` / `math.random()` draws from
    /// inside batch scripts. Same value across runs → identical
    /// script outputs given the same workload. Default 0 (set via
    /// [`Self::set_scene_seed`]).
    scene_seed: u64,
    /// Slice 5.5c — monotonically advancing frame counter, mixed
    /// into the per-chunk PRNG seed so consecutive frames don't
    /// observe the same random stream. Bumped by
    /// [`Self::advance_frame`] (called by `run_batch_scripts` once
    /// per frame).
    frame: u64,
}

impl BatchScriptPool {
    /// Construct a Direct-mode pool — the 5.5a behaviour. Used by the
    /// `--no-batch-pool` CLI flag (the (a) baseline) and as the
    /// automatic fallback on worker spawn failure.
    pub fn new_direct(
        #[cfg(feature = "native")] lua_buffer_mode: Option<BatchLuaBufferMode>,
    ) -> Result<Self, BatchError> {
        let js = BatchJsRuntime::new()?;
        #[cfg(feature = "native")]
        let lua = match lua_buffer_mode {
            Some(mode) => Some(LuaBackend::Direct(BatchLuaRuntime::new(mode)?)),
            None => None,
        };
        Ok(Self {
            mode: PoolMode::Direct,
            js: JsBackend::Direct(js),
            #[cfg(feature = "native")]
            lua,
            schemas: HashMap::new(),
            loaded: HashSet::new(),
            #[cfg(feature = "native")]
            lua_buffer_mode,
            #[cfg(feature = "native")]
            workers_per_kind: 1,
            scene_seed: 0,
            frame: 0,
        })
    }

    /// Construct a Pooled-mode pool with `workers_per_kind` worker
    /// threads per language. Spawns the JS workers eagerly; the Lua
    /// workers are spawned lazily by `load_lua` when the first Lua
    /// script loads.
    ///
    /// `workers_per_kind` is clamped to `>= 1` (a 0-worker pool would
    /// be useless — there'd be nothing to dispatch to).
    ///
    /// Per the locked Q&A decision: if any spawn fails, the caller
    /// should `warn!` and fall back to [`Self::new_direct`] — see
    /// [`Self::new_pooled_or_fallback`] for the canonical pattern.
    pub fn new_pooled(
        workers_per_kind: u32,
        #[cfg(feature = "native")] lua_buffer_mode: Option<BatchLuaBufferMode>,
    ) -> Result<Self, BatchError> {
        let n = workers_per_kind.max(1);
        let mut js_workers = Vec::with_capacity(n as usize);
        for i in 0..n {
            let w = worker::BatchScriptWorker::spawn(worker::WorkerSpawnConfig {
                id: i,
                kind: WorkerKind::Js,
                #[cfg(feature = "native")]
                lua_buffer_mode: lua_buffer_mode.unwrap_or(BatchLuaBufferMode::Buffer),
            })?;
            js_workers.push(w);
        }
        Ok(Self {
            mode: PoolMode::Pooled,
            js: JsBackend::Pooled(js_workers),
            #[cfg(feature = "native")]
            lua: None,
            schemas: HashMap::new(),
            loaded: HashSet::new(),
            #[cfg(feature = "native")]
            lua_buffer_mode,
            #[cfg(feature = "native")]
            workers_per_kind: n,
            scene_seed: 0,
            frame: 0,
        })
    }

    /// Try Pooled mode first; on failure log a warning and fall back
    /// to Direct mode. This is the canonical constructor for the
    /// runtime — it honours the "auto-fall-back to single-threaded
    /// with warn" Q&A decision in one place.
    pub fn new_pooled_or_fallback(
        workers_per_kind: u32,
        #[cfg(feature = "native")] lua_buffer_mode: Option<BatchLuaBufferMode>,
    ) -> Result<Self, BatchError> {
        match Self::new_pooled(
            workers_per_kind,
            #[cfg(feature = "native")]
            lua_buffer_mode,
        ) {
            Ok(p) => Ok(p),
            Err(e) => {
                warn!(
                    "[BatchScriptPool] worker pool init failed ({}); falling back to direct \
                     (single-threaded) batch dispatch",
                    e
                );
                Self::new_direct(
                    #[cfg(feature = "native")]
                    lua_buffer_mode,
                )
            }
        }
    }

    pub fn mode(&self) -> PoolMode {
        self.mode
    }

    // ─── 5.5a compatibility shim ────────────────────────────────────
    //
    // Slice 5.5b.1 step 1 lands the channel + worker infrastructure
    // and the new owned-column run API, but does NOT yet rewire the
    // `run_batch_scripts` dispatcher to use it. Until step 2 lands
    // (dispatcher rewire), the runtime constructs a Direct-mode pool
    // via `BatchScriptPool::new(...)` and the dispatcher reaches in
    // with the legacy `js()` / `lua()` accessors. These shims work
    // only in Direct mode and panic in Pooled mode — the new API
    // (`run_js_batch` / `run_lua_batch` / `load_js` / `load_lua`) is
    // the one Pooled callers must use.

    /// 5.5a-compatible constructor. Always Direct mode. New callers
    /// should prefer [`Self::new_pooled_or_fallback`] explicitly so
    /// the choice of backend is visible at the call site.
    pub fn new(
        #[cfg(feature = "native")] lua_buffer_mode: Option<BatchLuaBufferMode>,
    ) -> Result<Self, BatchError> {
        Self::new_direct(
            #[cfg(feature = "native")]
            lua_buffer_mode,
        )
    }

    /// 5.5a-compatible read accessor. Direct mode only.
    pub fn js(&self) -> &BatchJsRuntime {
        match &self.js {
            JsBackend::Direct(rt) => rt,
            JsBackend::Pooled(_) => panic!(
                "BatchScriptPool::js() called in Pooled mode — use run_js_batch / load_js instead"
            ),
        }
    }

    /// 5.5a-compatible mutable accessor. Direct mode only.
    pub fn js_mut(&mut self) -> &mut BatchJsRuntime {
        match &mut self.js {
            JsBackend::Direct(rt) => rt,
            JsBackend::Pooled(_) => panic!(
                "BatchScriptPool::js_mut() called in Pooled mode — use run_js_batch / load_js instead"
            ),
        }
    }

    /// 5.5a-compatible read accessor. Direct mode only.
    #[cfg(feature = "native")]
    pub fn lua(&self) -> Option<&BatchLuaRuntime> {
        match &self.lua {
            None => None,
            Some(LuaBackend::Direct(rt)) => Some(rt),
            Some(LuaBackend::Pooled(_)) => panic!(
                "BatchScriptPool::lua() called in Pooled mode — use run_lua_batch / load_lua instead"
            ),
        }
    }

    /// 5.5a-compatible lazy initialiser + mutable accessor.
    /// Direct mode only.
    #[cfg(feature = "native")]
    pub fn ensure_lua(
        &mut self,
        mode: BatchLuaBufferMode,
    ) -> Result<&mut BatchLuaRuntime, BatchError> {
        if matches!(self.mode, PoolMode::Pooled) {
            return Err(BatchError::Init(
                "ensure_lua() called in Pooled mode — use load_lua instead".into(),
            ));
        }
        if self.lua.is_none() {
            self.lua_buffer_mode = Some(mode);
            self.lua = Some(LuaBackend::Direct(BatchLuaRuntime::new(mode)?));
        }
        match self.lua.as_mut().expect("just-initialised lua backend") {
            LuaBackend::Direct(rt) => Ok(rt),
            LuaBackend::Pooled(_) => unreachable!("guarded by mode check above"),
        }
    }

    /// Number of workers currently in the pool. In 5.5b.1 this is 1
    /// for Pooled mode (or 0 if both backends are dead) and 0 for
    /// Direct mode (no worker *threads*, just the main thread).
    /// Slice 5.5b.2 grows this to `min(physical_cores, 4)`.
    pub fn worker_count(&self) -> u32 {
        match self.mode {
            PoolMode::Direct => 0,
            PoolMode::Pooled => self.batch_workers_active(),
        }
    }

    /// Per-frame `batch_workers_active` counter — number of live
    /// worker threads across all backends. Used by the dispatcher to
    /// populate `FrameCounters::batch_workers_active`.
    pub fn batch_workers_active(&self) -> u32 {
        let mut n: u32 = 0;
        if let JsBackend::Pooled(workers) = &self.js {
            n += workers.len() as u32;
        }
        #[cfg(feature = "native")]
        if let Some(LuaBackend::Pooled(workers)) = &self.lua {
            n += workers.len() as u32;
        }
        n
    }

    /// VM memory in bytes for worker `idx`. Returns 0 in 5.5b.1 — real
    /// measurement (using `Lua::used_memory()` and
    /// `Runtime::memory_usage`) lands in 5.5c.
    pub fn vm_memory_bytes(&self, _idx: u32) -> u64 {
        0
    }

    // ─── Slice 5.5c determinism state ──────────────────────────────

    /// Slice 5.5c — set the deterministic-PRNG scene seed. Default is
    /// `0`. Two runs with the same `scene_seed`, identical workload,
    /// and identical frame sequence produce bit-identical script
    /// outputs (including any `Math.random()` / `math.random()`
    /// draws). Call once before the first `run_batch_scripts` of a
    /// session if you want a non-default seed; otherwise `0` is fine.
    pub fn set_scene_seed(&mut self, seed: u64) {
        self.scene_seed = seed;
    }

    /// Slice 5.5c — current scene seed (default `0`).
    pub fn scene_seed(&self) -> u64 {
        self.scene_seed
    }

    /// Slice 5.5c — bump the per-frame counter mixed into per-chunk
    /// PRNG seeds. Called by `run_batch_scripts` once per frame
    /// before dispatching any batches. Two consecutive frames thus
    /// observe independent random streams even if every other input
    /// is identical.
    pub fn advance_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// Slice 5.5c — current frame counter.
    pub fn frame(&self) -> u64 {
        self.frame
    }

    // ─── load API ───────────────────────────────────────────────────

    /// Load a JS batch script. Caches the parsed schemas on success so
    /// subsequent dispatcher queries are main-thread-cheap.
    pub fn load_js(&mut self, path: &str, source: &str) -> Result<(), BatchError> {
        let schema = match &mut self.js {
            JsBackend::Direct(rt) => {
                rt.load(path, source)?;
                CachedSchema {
                    read: rt.read_schema(path).unwrap_or(&[]).to_vec(),
                    write: rt.write_schema(path).unwrap_or(&[]).to_vec(),
                    read_optional: rt.read_optional_schema(path).unwrap_or(&[]).to_vec(),
                }
            }
            JsBackend::Pooled(workers) => {
                if workers.is_empty() {
                    return Err(BatchError::Init("JS worker pool is empty".into()));
                }
                // Replicate the load on every worker so chunked
                // dispatch can route to any of them. Take the schema
                // from the first reply — they all parse the same
                // source so they must agree.
                let mut first: Option<channel::LoadOk> = None;
                for w in workers.iter() {
                    let ok = w.send_load(path.to_string(), source.to_string())?;
                    if first.is_none() {
                        first = Some(ok);
                    }
                }
                let ok = first.expect("at least one worker present");
                CachedSchema {
                    read: ok.read_schema,
                    write: ok.write_schema,
                    read_optional: ok.read_optional_schema,
                }
            }
        };
        let key = (WorkerKind::Js, path.to_string());
        self.loaded.insert(key.clone());
        self.schemas.insert(key, schema);
        Ok(())
    }

    /// Load a Lua batch script. Lazily creates the Lua backend if
    /// none exists yet (matches the 5.5a `ensure_lua` pattern).
    #[cfg(feature = "native")]
    pub fn load_lua(
        &mut self,
        path: &str,
        source: &str,
        mode: BatchLuaBufferMode,
    ) -> Result<(), BatchError> {
        if self.lua.is_none() {
            self.lua_buffer_mode = Some(mode);
            self.lua = Some(match self.mode {
                PoolMode::Direct => LuaBackend::Direct(BatchLuaRuntime::new(mode)?),
                PoolMode::Pooled => {
                    let n = self.workers_per_kind;
                    let mut workers = Vec::with_capacity(n as usize);
                    for i in 0..n {
                        // Worker IDs in the Lua pool start at a high
                        // offset so logs distinguish them from JS.
                        let w = worker::BatchScriptWorker::spawn(worker::WorkerSpawnConfig {
                            id: 1000 + i,
                            kind: WorkerKind::Lua,
                            lua_buffer_mode: mode,
                        })?;
                        workers.push(w);
                    }
                    LuaBackend::Pooled(workers)
                }
            });
        }
        let lua = self.lua.as_mut().expect("just-initialised lua backend");
        let schema = match lua {
            LuaBackend::Direct(rt) => {
                rt.load(path, source)?;
                CachedSchema {
                    read: rt.read_schema(path).unwrap_or(&[]).to_vec(),
                    write: rt.write_schema(path).unwrap_or(&[]).to_vec(),
                    read_optional: rt.read_optional_schema(path).unwrap_or(&[]).to_vec(),
                }
            }
            LuaBackend::Pooled(workers) => {
                if workers.is_empty() {
                    return Err(BatchError::Init("Lua worker pool is empty".into()));
                }
                let mut first: Option<channel::LoadOk> = None;
                for w in workers.iter() {
                    let ok = w.send_load(path.to_string(), source.to_string())?;
                    if first.is_none() {
                        first = Some(ok);
                    }
                }
                let ok = first.expect("at least one worker present");
                CachedSchema {
                    read: ok.read_schema,
                    write: ok.write_schema,
                    read_optional: ok.read_optional_schema,
                }
            }
        };
        let key = (WorkerKind::Lua, path.to_string());
        self.loaded.insert(key.clone());
        self.schemas.insert(key, schema);
        Ok(())
    }

    // ─── schema query API (main-thread, synchronous) ────────────────

    pub fn is_js_loaded(&self, path: &str) -> bool {
        self.loaded.contains(&(WorkerKind::Js, path.to_string()))
    }

    #[cfg(feature = "native")]
    pub fn is_lua_loaded(&self, path: &str) -> bool {
        self.loaded.contains(&(WorkerKind::Lua, path.to_string()))
    }

    pub fn js_read_schema(&self, path: &str) -> &[String] {
        self.schemas
            .get(&(WorkerKind::Js, path.to_string()))
            .map(|s| s.read.as_slice())
            .unwrap_or(&[])
    }

    pub fn js_write_schema(&self, path: &str) -> &[String] {
        self.schemas
            .get(&(WorkerKind::Js, path.to_string()))
            .map(|s| s.write.as_slice())
            .unwrap_or(&[])
    }

    pub fn js_read_optional_schema(&self, path: &str) -> &[String] {
        self.schemas
            .get(&(WorkerKind::Js, path.to_string()))
            .map(|s| s.read_optional.as_slice())
            .unwrap_or(&[])
    }

    #[cfg(feature = "native")]
    pub fn lua_read_schema(&self, path: &str) -> &[String] {
        self.schemas
            .get(&(WorkerKind::Lua, path.to_string()))
            .map(|s| s.read.as_slice())
            .unwrap_or(&[])
    }

    #[cfg(feature = "native")]
    pub fn lua_write_schema(&self, path: &str) -> &[String] {
        self.schemas
            .get(&(WorkerKind::Lua, path.to_string()))
            .map(|s| s.write.as_slice())
            .unwrap_or(&[])
    }

    #[cfg(feature = "native")]
    pub fn lua_read_optional_schema(&self, path: &str) -> &[String] {
        self.schemas
            .get(&(WorkerKind::Lua, path.to_string()))
            .map(|s| s.read_optional.as_slice())
            .unwrap_or(&[])
    }

    // ─── run API ───────────────────────────────────────────────────

    /// Run one JS batch invocation. The dispatcher passes
    /// pre-populated owned columns; the pool routes them either to a
    /// synchronous `run_batch` (Direct) or a worker thread (Pooled),
    /// updates `counters`, and returns the (mutated) write columns +
    /// per-column dirty mask.
    pub fn run_js_batch(
        &mut self,
        path: &str,
        dt: f32,
        reads: Vec<OwnedReadColumn>,
        writes: Vec<OwnedWriteColumn>,
        optionals: Vec<OwnedOptionalColumn>,
        system_id: u32,
        counters: &mut FrameCounters,
    ) -> Result<RunBatchOutput, BatchError> {
        let payload_bytes = payload_bytes_in(&reads, &writes, &optionals);
        let scene_seed = self.scene_seed;
        let frame = self.frame;
        match &mut self.js {
            JsBackend::Direct(rt) => run_direct_js(
                rt, path, dt, reads, writes, optionals, counters, payload_bytes,
                scene_seed, frame, system_id,
            ),
            JsBackend::Pooled(workers) => run_pooled_chunked(
                workers,
                "JS",
                path,
                dt,
                reads,
                writes,
                optionals,
                counters,
                payload_bytes,
                scene_seed,
                frame,
                system_id,
            ),
        }
    }

    #[cfg(feature = "native")]
    pub fn run_lua_batch(
        &mut self,
        path: &str,
        dt: f32,
        reads: Vec<OwnedReadColumn>,
        writes: Vec<OwnedWriteColumn>,
        optionals: Vec<OwnedOptionalColumn>,
        system_id: u32,
        counters: &mut FrameCounters,
    ) -> Result<RunBatchOutput, BatchError> {
        let payload_bytes = payload_bytes_in(&reads, &writes, &optionals);
        let scene_seed = self.scene_seed;
        let frame = self.frame;
        let lua = self
            .lua
            .as_mut()
            .ok_or_else(|| BatchError::Init("Lua backend not initialised".into()))?;
        match lua {
            LuaBackend::Direct(rt) => run_direct_lua(
                rt, path, dt, reads, writes, optionals, counters, payload_bytes,
                scene_seed, frame, system_id,
            ),
            LuaBackend::Pooled(workers) => run_pooled_chunked(
                workers,
                "Lua",
                path,
                dt,
                reads,
                writes,
                optionals,
                counters,
                payload_bytes,
                scene_seed,
                frame,
                system_id,
            ),
        }
    }

    /// Test-only: explicitly shut down all worker threads. Production
    /// code relies on `Drop` to do the same thing.
    #[cfg(test)]
    pub fn shutdown(&mut self) {
        if let JsBackend::Pooled(workers) = &mut self.js {
            for w in workers.drain(..) {
                w.shutdown();
                drop(w);
            }
        }
        #[cfg(feature = "native")]
        if let Some(LuaBackend::Pooled(workers)) = self.lua.as_mut() {
            for w in workers.drain(..) {
                w.shutdown();
                drop(w);
            }
        }
    }
}

impl Drop for BatchScriptPool {
    fn drop(&mut self) {
        // Workers handle their own shutdown in their Drop impls; this
        // is just a place to log the final state for debugging.
        if matches!(self.mode, PoolMode::Pooled) {
            info!(
                "[BatchScriptPool] dropping pool (mode=Pooled, active_workers={})",
                self.batch_workers_active()
            );
        }
    }
}

/// Output of a single `run_*_batch` call. The dispatcher consumes
/// `writes` for scatter and `write_dirty` to skip clean columns.
#[derive(Debug)]
pub struct RunBatchOutput {
    pub writes: Vec<OwnedWriteColumn>,
    pub write_dirty: Vec<bool>,
    /// Wall-clock micros the runtime spent inside `run_batch`.
    pub elapsed_us: u64,
    /// Slice 5.5c — high-water VM memory across the chunks of this
    /// batch (bytes). Sampled at frame boundaries on each worker.
    pub worker_memory_bytes_max: u64,
    /// Slice 5.5c — sum of per-chunk VM memory readings.
    pub worker_memory_bytes_total: u64,
}

fn run_direct_js(
    rt: &BatchJsRuntime,
    path: &str,
    dt: f32,
    reads: Vec<OwnedReadColumn>,
    mut writes: Vec<OwnedWriteColumn>,
    optionals: Vec<OwnedOptionalColumn>,
    counters: &mut FrameCounters,
    payload_bytes: u64,
    scene_seed: u64,
    frame: u64,
    system_id: u32,
) -> Result<RunBatchOutput, BatchError> {
    use crate::scripting::batch::{BatchOptionalColumn, BatchReadColumn, BatchWriteColumn};
    use crate::scripting::prng;

    let read_cols: Vec<BatchReadColumn<'_>> = reads
        .iter()
        .map(|c| BatchReadColumn { name: c.name.as_str(), x: &c.x, y: &c.y })
        .collect();
    let mut write_cols: Vec<BatchWriteColumn<'_>> = writes
        .iter_mut()
        .map(|c| BatchWriteColumn { name: c.name.as_str(), x: c.x.as_mut_slice(), y: c.y.as_mut_slice() })
        .collect();
    let opt_cols: Vec<BatchOptionalColumn<'_>> = optionals
        .iter()
        .map(|c| BatchOptionalColumn {
            name: c.name.as_str(),
            value: c.value.as_ref().map(|(x, y)| (x.as_slice(), y.as_slice())),
        })
        .collect();
    let mut write_dirty: Vec<bool> = vec![false; write_cols.len()];

    // Slice 5.5c — reseed before invoking, sample memory after.
    rt.set_rng_seed(prng::mix_seed(scene_seed, frame, system_id, 0));

    let dispatch_start = std::time::Instant::now();
    let elapsed_us = rt.run_batch(
        path,
        dt,
        &read_cols,
        &mut write_cols,
        &opt_cols,
        &mut [],
        &mut write_dirty,
        counters,
    )?;
    let dispatch_us = dispatch_start.elapsed().as_micros() as u64;

    drop(read_cols);
    drop(write_cols);
    drop(opt_cols);

    let worker_memory_bytes = rt.memory_bytes();

    counters.batch_payload_clone_bytes += payload_bytes;
    counters.batch_response_bytes += response_bytes(&writes);
    counters.batch_worker_dispatch_us += dispatch_us;
    if worker_memory_bytes > counters.script_worker_memory_bytes_max {
        counters.script_worker_memory_bytes_max = worker_memory_bytes;
    }
    counters.script_worker_memory_bytes_total += worker_memory_bytes;

    Ok(RunBatchOutput {
        writes,
        write_dirty,
        elapsed_us,
        worker_memory_bytes_max: worker_memory_bytes,
        worker_memory_bytes_total: worker_memory_bytes,
    })
}

#[cfg(feature = "native")]
fn run_direct_lua(
    rt: &BatchLuaRuntime,
    path: &str,
    dt: f32,
    reads: Vec<OwnedReadColumn>,
    mut writes: Vec<OwnedWriteColumn>,
    optionals: Vec<OwnedOptionalColumn>,
    counters: &mut FrameCounters,
    payload_bytes: u64,
    scene_seed: u64,
    frame: u64,
    system_id: u32,
) -> Result<RunBatchOutput, BatchError> {
    use crate::scripting::batch::{BatchOptionalColumn, BatchReadColumn, BatchWriteColumn};
    use crate::scripting::prng;

    let read_cols: Vec<BatchReadColumn<'_>> = reads
        .iter()
        .map(|c| BatchReadColumn { name: c.name.as_str(), x: &c.x, y: &c.y })
        .collect();
    let mut write_cols: Vec<BatchWriteColumn<'_>> = writes
        .iter_mut()
        .map(|c| BatchWriteColumn { name: c.name.as_str(), x: c.x.as_mut_slice(), y: c.y.as_mut_slice() })
        .collect();
    let opt_cols: Vec<BatchOptionalColumn<'_>> = optionals
        .iter()
        .map(|c| BatchOptionalColumn {
            name: c.name.as_str(),
            value: c.value.as_ref().map(|(x, y)| (x.as_slice(), y.as_slice())),
        })
        .collect();
    let mut write_dirty: Vec<bool> = vec![false; write_cols.len()];

    rt.set_rng_seed(prng::mix_seed(scene_seed, frame, system_id, 0));

    let dispatch_start = std::time::Instant::now();
    let elapsed_us = rt.run_batch(
        path,
        dt,
        &read_cols,
        &mut write_cols,
        &opt_cols,
        &mut [],
        &mut write_dirty,
        counters,
    )?;
    let dispatch_us = dispatch_start.elapsed().as_micros() as u64;

    drop(read_cols);
    drop(write_cols);
    drop(opt_cols);

    let worker_memory_bytes = rt.memory_bytes();

    counters.batch_payload_clone_bytes += payload_bytes;
    counters.batch_response_bytes += response_bytes(&writes);
    counters.batch_worker_dispatch_us += dispatch_us;
    if worker_memory_bytes > counters.script_worker_memory_bytes_max {
        counters.script_worker_memory_bytes_max = worker_memory_bytes;
    }
    counters.script_worker_memory_bytes_total += worker_memory_bytes;

    Ok(RunBatchOutput {
        writes,
        write_dirty,
        elapsed_us,
        worker_memory_bytes_max: worker_memory_bytes,
        worker_memory_bytes_total: worker_memory_bytes,
    })
}

/// Chunked dispatch across `workers`. Splits the entity range into
/// `workers.len()` contiguous blocks (remainder distributed to the
/// first chunks), fires one `RunBatch` request per worker in
/// parallel, then drains the replies in chunk-id order so the merged
/// `OwnedWriteColumn` vectors are deterministic.
///
/// `dispatch_us` is wall-clock from "send first chunk" to "received
/// last reply" — the parallelism win surfaces here. `payload_bytes`
/// and `response_bytes` are charged on the *original* sizes (sum of
/// chunk sizes equals the un-chunked total).
fn run_pooled_chunked(
    workers: &mut Vec<worker::BatchScriptWorker>,
    label: &'static str,
    path: &str,
    dt: f32,
    reads: Vec<OwnedReadColumn>,
    writes: Vec<OwnedWriteColumn>,
    optionals: Vec<OwnedOptionalColumn>,
    counters: &mut FrameCounters,
    payload_bytes: u64,
    scene_seed: u64,
    frame: u64,
    system_id: u32,
) -> Result<RunBatchOutput, BatchError> {
    if workers.is_empty() {
        return Err(BatchError::Init(format!("{label} worker pool is empty")));
    }

    // ── Determine entity count from any column. All read/write
    // columns have the same length by dispatcher invariant. If there
    // are no entities (e.g. an empty bucket) just return empty
    // writes — nothing to scatter.
    let n_entities = reads
        .first()
        .map(|c| c.x.len())
        .or_else(|| writes.first().map(|c| c.x.len()))
        .or_else(|| {
            optionals
                .iter()
                .find_map(|c| c.value.as_ref().map(|(x, _)| x.len()))
        })
        .unwrap_or(0);

    let n_workers = workers.len();

    // Slice 5.5c.1 — wrap each `Vec<f32>` column in `Arc<Vec<f32>>`
    // (true zero-copy: `Arc::new(vec)` only wraps the Vec metadata,
    // the heap buffer stays put — NO element memcpy). Per-chunk
    // fan-out then ships only refcount bumps + a sub-`range`. Using
    // `Arc<Vec<f32>>` instead of `Arc<[f32]>` matters: stdlib's
    // `Arc::from(Vec<T>)` allocates fresh `ArcInner<[T]>` storage and
    // memcpys all elements; `Arc::new(vec)` does not.
    let shared_reads: Vec<(String, std::sync::Arc<Vec<f32>>, std::sync::Arc<Vec<f32>>)> = reads
        .into_iter()
        .map(|c| (c.name, std::sync::Arc::new(c.x), std::sync::Arc::new(c.y)))
        .collect();
    let shared_opts: Vec<(String, Option<(std::sync::Arc<Vec<f32>>, std::sync::Arc<Vec<f32>>)>)> =
        optionals
            .into_iter()
            .map(|c| {
                let value = c
                    .value
                    .map(|(x, y)| (std::sync::Arc::new(x), std::sync::Arc::new(y)));
                (c.name, value)
            })
            .collect();

    // Single-chunk fast path: matches 5.5b.1 exactly. Avoids the
    // chunk-build cost when N=1 or there's nothing to split. Still
    // wraps in Shared* with a full-range descriptor so the worker
    // signature is uniform.
    if n_workers == 1 || n_entities <= 1 {
        let full_range = 0..n_entities;
        let single_reads: Vec<SharedReadColumn> = shared_reads
            .iter()
            .map(|(name, x, y)| SharedReadColumn {
                name: name.clone(),
                x: x.clone(),
                y: y.clone(),
                range: full_range.clone(),
            })
            .collect();
        let single_opts: Vec<SharedOptionalColumn> = shared_opts
            .iter()
            .map(|(name, value)| SharedOptionalColumn {
                name: name.clone(),
                value: value.as_ref().map(|(x, y)| (x.clone(), y.clone())),
                range: full_range.clone(),
            })
            .collect();
        let dispatch_start = std::time::Instant::now();
        let single_seed = crate::scripting::prng::mix_seed(scene_seed, frame, system_id, 0);
        let result = workers[0].send_run_batch(path.to_string(), dt, single_reads, writes, single_opts, single_seed);
        let dispatch_us = dispatch_start.elapsed().as_micros() as u64;
        return finish_pooled_single(workers, label, result, counters, payload_bytes, dispatch_us);
    }

    // ── Compute block partition (chunk_size_base + remainder).
    let chunk_size_base = n_entities / n_workers;
    let remainder = n_entities % n_workers;
    let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(n_workers);
    let mut cursor = 0usize;
    for i in 0..n_workers {
        let extra = if i < remainder { 1 } else { 0 };
        let end = cursor + chunk_size_base + extra;
        ranges.push((cursor, end));
        cursor = end;
    }
    debug_assert_eq!(cursor, n_entities);

    // ── Build chunked column descriptors. Reads/optionals are
    // zero-copy (Arc clone + range — Slice 5.5c.1). Writes still
    // need owned per-chunk Vec<f32>'s because the worker mutates
    // them in place and ships them back inside the response. We
    // measured an Arc<Vec<f32>> + worker-side scratch copy variant
    // in 5.5c.2 and rejected it (see `docs/scripting-performance-
    // lessons.md` “Slice 5.5c.2 — rejected”): the move only shifts
    // the same memcpy from main to worker, the default-N=1 user
    // pays an extra full-column copy, and measured wins were ≤3%.
    let dispatch_start = std::time::Instant::now();
    let mut receivers: Vec<crossbeam_channel::Receiver<Result<channel::RunBatchOk, BatchError>>> =
        Vec::with_capacity(n_workers);
    for (chunk_id, &(start, end)) in ranges.iter().enumerate() {
        let chunk_range = start..end;
        let chunk_reads: Vec<SharedReadColumn> = shared_reads
            .iter()
            .map(|(name, x, y)| SharedReadColumn {
                name: name.clone(),
                x: x.clone(),
                y: y.clone(),
                range: chunk_range.clone(),
            })
            .collect();
        let chunk_writes: Vec<OwnedWriteColumn> = writes
            .iter()
            .map(|c| OwnedWriteColumn {
                name: c.name.clone(),
                x: c.x[start..end].to_vec(),
                y: c.y[start..end].to_vec(),
            })
            .collect();
        let chunk_opts: Vec<SharedOptionalColumn> = shared_opts
            .iter()
            .map(|(name, value)| SharedOptionalColumn {
                name: name.clone(),
                value: value.as_ref().map(|(x, y)| (x.clone(), y.clone())),
                range: chunk_range.clone(),
            })
            .collect();

        let w = &workers[chunk_id];
        let chunk_seed = crate::scripting::prng::mix_seed(scene_seed, frame, system_id, chunk_id as u32);
        let rx = w.dispatch_run_batch(path.to_string(), dt, chunk_reads, chunk_writes, chunk_opts, chunk_seed)?;
        receivers.push(rx);
    }

    // ── Recv in chunk-id order — ensures deterministic merge.
    let n_write_cols = writes.len();
    let mut merged_writes: Vec<OwnedWriteColumn> = writes
        .into_iter()
        .map(|c| OwnedWriteColumn { name: c.name, x: Vec::with_capacity(n_entities), y: Vec::with_capacity(n_entities) })
        .collect();
    let mut merged_dirty: Vec<bool> = vec![false; n_write_cols];
    let mut max_elapsed_us: u64 = 0;
    let mut chunk_mem_max: u64 = 0;
    let mut chunk_mem_total: u64 = 0;

    for (chunk_id, rx) in receivers.into_iter().enumerate() {
        let result = rx
            .recv()
            .unwrap_or_else(|_| Err(BatchError::Init("worker dropped reply".into())));
        match result {
            Ok(ok) => {
                accumulate_frame_counters(counters, &ok.counter_deltas);
                for (col_idx, mut col) in ok.writes.into_iter().enumerate() {
                    merged_writes[col_idx].x.append(&mut col.x);
                    merged_writes[col_idx].y.append(&mut col.y);
                }
                for (col_idx, dirty) in ok.write_dirty.into_iter().enumerate() {
                    if col_idx < merged_dirty.len() {
                        merged_dirty[col_idx] |= dirty;
                    }
                }
                if ok.elapsed_us > max_elapsed_us {
                    max_elapsed_us = ok.elapsed_us;
                }
                if ok.worker_memory_bytes > chunk_mem_max {
                    chunk_mem_max = ok.worker_memory_bytes;
                }
                chunk_mem_total += ok.worker_memory_bytes;
            }
            Err(e) => {
                if matches!(&e, BatchError::Init(msg) if msg.contains("worker channel closed")
                    || msg.contains("worker dropped reply"))
                {
                    warn!(
                        "[BatchScriptPool] {label} worker {} disconnected during chunked dispatch",
                        chunk_id
                    );
                }
                // Bail out — partial merge would be non-deterministic
                // and the dispatcher can't scatter half-results.
                return Err(e);
            }
        }
    }

    let dispatch_us = dispatch_start.elapsed().as_micros() as u64;
    counters.batch_payload_clone_bytes += payload_bytes;
    counters.batch_response_bytes += response_bytes(&merged_writes);
    counters.batch_worker_dispatch_us += dispatch_us;
    if chunk_mem_max > counters.script_worker_memory_bytes_max {
        counters.script_worker_memory_bytes_max = chunk_mem_max;
    }
    counters.script_worker_memory_bytes_total += chunk_mem_total;

    Ok(RunBatchOutput {
        writes: merged_writes,
        write_dirty: merged_dirty,
        elapsed_us: max_elapsed_us,
        worker_memory_bytes_max: chunk_mem_max,
        worker_memory_bytes_total: chunk_mem_total,
    })
}

/// Single-worker reply-handling path — used when N=1 or n_entities<=1.
/// Mirrors the 5.5b.1 `run_pooled` helper exactly.
fn finish_pooled_single(
    workers: &mut Vec<worker::BatchScriptWorker>,
    label: &'static str,
    result: Result<channel::RunBatchOk, BatchError>,
    counters: &mut FrameCounters,
    payload_bytes: u64,
    dispatch_us: u64,
) -> Result<RunBatchOutput, BatchError> {
    match result {
        Ok(ok) => {
            accumulate_frame_counters(counters, &ok.counter_deltas);
            counters.batch_payload_clone_bytes += payload_bytes;
            counters.batch_response_bytes += response_bytes(&ok.writes);
            counters.batch_worker_dispatch_us += dispatch_us;
            if ok.worker_memory_bytes > counters.script_worker_memory_bytes_max {
                counters.script_worker_memory_bytes_max = ok.worker_memory_bytes;
            }
            counters.script_worker_memory_bytes_total += ok.worker_memory_bytes;
            Ok(RunBatchOutput {
                writes: ok.writes,
                write_dirty: ok.write_dirty,
                elapsed_us: ok.elapsed_us,
                worker_memory_bytes_max: ok.worker_memory_bytes,
                worker_memory_bytes_total: ok.worker_memory_bytes,
            })
        }
        Err(e) => {
            if matches!(&e, BatchError::Init(msg) if msg.contains("worker channel closed")) {
                warn!("[BatchScriptPool] {label} worker disconnected; removing from pool");
                if !workers.is_empty() {
                    workers.remove(0);
                }
            }
            Err(e)
        }
    }
}

/// Add `delta`'s counter values into `dst`. Field-by-field; intentionally
/// explicit so adding a new counter requires updating this fn.
fn accumulate_frame_counters(dst: &mut FrameCounters, delta: &FrameCounters) {
    dst.batch_copy_in_bytes += delta.batch_copy_in_bytes;
    dst.batch_copy_out_bytes += delta.batch_copy_out_bytes;
    dst.batch_dirty_bytes += delta.batch_dirty_bytes;
    dst.batch_copy_in_us += delta.batch_copy_in_us;
    dst.batch_run_us += delta.batch_run_us;
    dst.batch_copy_out_us += delta.batch_copy_out_us;
    // The dispatcher manages systems_run / chunks / vm_calls /
    // scatters_skipped / worker_count / vm_memory itself; the worker
    // doesn't touch them, so they stay at default(0) in `delta` and
    // contribute nothing here. Channel-specific counters
    // (payload_clone_bytes / response_bytes / worker_dispatch_us /
    // workers_active) are populated by the pool, not the worker.
}

// --- tests ---------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 5.5b.1: round-trip a JS batch through the **Pooled** backend.
    /// Loads a tiny mover script, sends one `RunBatch`, asserts the
    /// write column got the expected mutation back across the channel,
    /// and explicitly shuts the worker thread down so we exercise the
    /// shutdown path too. This is the canary that the channel + worker
    /// + counter-merge path is alive end-to-end.
    #[test]
    fn pooled_js_round_trip() {
        let mut pool = BatchScriptPool::new_pooled(1, None)
            .expect("new_pooled");
        assert_eq!(pool.mode(), PoolMode::Pooled);
        assert!(pool.batch_workers_active() >= 1);

        pool.load_js(
            "rt.js",
            "module.exports.update = function(ctx, q) {
                const px = q.write.Position2D.x;
                const vx = q.read.Velocity2D.x;
                for (let i = 0; i < q.count; i++) px[i] += vx[i] * ctx.dt;
            };
            module.exports.system = { query: { read: ['Velocity2D'], write: ['Position2D'] } };",
        )
        .expect("load_js");
        assert!(pool.is_js_loaded("rt.js"));

        let reads = vec![OwnedReadColumn {
            name: "Velocity2D".to_string(),
            x: vec![10.0_f32, 20.0],
            y: vec![0.0_f32, 0.0],
        }];
        let writes = vec![OwnedWriteColumn {
            name: "Position2D".to_string(),
            x: vec![1.0_f32, 2.0],
            y: vec![0.0_f32, 0.0],
        }];
        let mut counters = FrameCounters::default();
        let out = pool
            .run_js_batch("rt.js", 0.5, reads, writes, vec![], 0, &mut counters)
            .expect("run_js_batch");

        assert_eq!(out.writes.len(), 1);
        let pos_x = &out.writes[0].x;
        assert!((pos_x[0] - 6.0).abs() < 1e-5, "px[0]={}", pos_x[0]);
        assert!((pos_x[1] - 12.0).abs() < 1e-5, "px[1]={}", pos_x[1]);

        // Pooled-mode counters: both payload + response bytes were
        // charged, and dispatch_us ticked at least 1µs.
        assert!(counters.batch_payload_clone_bytes > 0, "payload bytes = 0");
        assert!(counters.batch_response_bytes > 0, "response bytes = 0");

        // Explicit shutdown — the Drop impl would do the same, but
        // doing it here covers the test-only `shutdown` API too.
        pool.shutdown();
        assert_eq!(pool.batch_workers_active(), 0);
    }

    /// 5.5b.2: round-trip a JS batch through a **2-worker** Pooled
    /// pool. Verifies that chunked dispatch correctly:
    ///
    /// 1. Splits the entity range into 2 contiguous chunks (5 + 4 for
    ///    9 entities — exercises the "remainder distributed to first
    ///    chunks" path).
    /// 2. Replicates the script load on every worker (so chunk 1
    ///    actually has the script available).
    /// 3. Merges chunk results back in chunk-id order so the merged
    ///    `OwnedWriteColumn.x` values match what a single-worker run
    ///    would have produced.
    #[test]
    fn pooled_js_n2_chunked_round_trip() {
        let mut pool = BatchScriptPool::new_pooled(2, None)
            .expect("new_pooled(2)");
        assert_eq!(pool.batch_workers_active(), 2);

        pool.load_js(
            "rt.js",
            "module.exports.update = function(ctx, q) {
                const px = q.write.Position2D.x;
                const vx = q.read.Velocity2D.x;
                for (let i = 0; i < q.count; i++) px[i] += vx[i] * ctx.dt;
            };
            module.exports.system = { query: { read: ['Velocity2D'], write: ['Position2D'] } };",
        )
        .expect("load_js");

        // 9 entities → chunk_size 4 + remainder 1 → chunk 0 gets 5,
        // chunk 1 gets 4. Pre-compute expected px = px0 + vx*dt for
        // each i so we can assert determinism *and* correctness.
        let n = 9usize;
        let dt = 0.25_f32;
        let vx: Vec<f32> = (0..n).map(|i| i as f32 + 1.0).collect();
        let px0: Vec<f32> = (0..n).map(|i| (i as f32) * 10.0).collect();
        let expected: Vec<f32> = vx.iter().zip(px0.iter()).map(|(v, p)| p + v * dt).collect();

        let reads = vec![OwnedReadColumn {
            name: "Velocity2D".to_string(),
            x: vx,
            y: vec![0.0; n],
        }];
        let writes = vec![OwnedWriteColumn {
            name: "Position2D".to_string(),
            x: px0,
            y: vec![0.0; n],
        }];
        let mut counters = FrameCounters::default();
        let out = pool
            .run_js_batch("rt.js", dt, reads, writes, vec![], 0, &mut counters)
            .expect("run_js_batch");

        assert_eq!(out.writes.len(), 1);
        assert_eq!(out.writes[0].x.len(), n, "merged length matches input");
        for i in 0..n {
            assert!(
                (out.writes[0].x[i] - expected[i]).abs() < 1e-5,
                "px[{i}] = {} expected {}",
                out.writes[0].x[i],
                expected[i]
            );
        }

        pool.shutdown();
        assert_eq!(pool.batch_workers_active(), 0);
    }
}
