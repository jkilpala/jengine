//! Slice 5.5b.1: dedicated worker thread + channel for one batch
//! script runtime.
//!
//! Each [`BatchScriptWorker`] owns:
//!
//! - a `JoinHandle` to the worker thread,
//! - the `Sender<BatchRequest>` end the dispatcher uses to enqueue
//!   work,
//! - a small piece of metadata (worker id, runtime kind).
//!
//! The worker thread itself constructs and owns the `BatchJsRuntime`
//! or `BatchLuaRuntime` for life — those types are not `Send`, but
//! since the only thread that ever touches them is the worker's own,
//! that's fine. The dispatcher only ships `Send + 'static` payloads
//! ([`BatchRequest`] / [`BatchResponse`]) over the channel.
//!
//! ### Shutdown
//!
//! Drop discipline (belt-and-suspenders):
//!
//! 1. `Drop` sends a [`BatchRequest::Shutdown`] (best-effort — ignored
//!    if the worker already exited).
//! 2. Drops the request `Sender`, which causes the worker's `recv()`
//!    to return `Err(Disconnected)` and exit cleanly even if it
//!    missed the explicit Shutdown.
//! 3. `JoinHandle::join` is called with a poll-and-bail wrapper: if
//!    the worker hasn't finished within 5 seconds we `error!` and
//!    abandon the handle (the OS reaps the thread at process exit).
//!    Never block app shutdown indefinitely on a misbehaving worker.
//!
//! ### Panic isolation
//!
//! The recv → dispatch loop body is wrapped in
//! [`std::panic::catch_unwind`]. A script-level error becomes a
//! `BatchError::RunFailed` *inside* `run_batch` and never panics, so
//! a panic at the worker boundary is by definition a bug in
//! mlua/rquickjs/the dispatcher itself. On panic the worker logs the
//! reason, replies to the in-flight request with a synthetic
//! `BatchError::RunFailed`, and exits the loop. The pool marks the
//! worker dead; subsequent calls bypass the worker and return errors
//! that the dispatcher logs and skips. (Slice 5.5b.2 will add real
//! single-thread fallback on worker death — for 5.5b.1 with N=1, the
//! dead-worker path just degrades the affected language to no-op.)

#![cfg(all(not(target_arch = "wasm32"), any(feature = "js-adapter-host", feature = "native")))]

use std::panic::AssertUnwindSafe;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, Sender};
use log::{error, warn};

use super::channel::{
    BatchRequest, LoadOk, OwnedWriteColumn, RunBatchOk, SharedOptionalColumn, SharedReadColumn,
};
use crate::profiling::FrameCounters;
use crate::scripting::batch::{
    BatchError, BatchJsRuntime, BatchOptionalColumn, BatchReadColumn, BatchWriteColumn,
};
#[cfg(feature = "native")]
use crate::scripting::batch_lua::{BatchLuaBufferMode, BatchLuaRuntime};

/// Channel capacity for the request queue. Bounded so the dispatcher
/// gets backpressure if a worker stalls — better than silently growing
/// an unbounded queue and hiding the bug. Picked at 16 because the
/// dispatcher only ever has one request in flight per worker today
/// (sync send/recv per bucket); 16 leaves headroom for 5.5b.2's
/// chunked dispatch where one frame may enqueue several chunks per
/// worker before any reply lands.
const REQUEST_QUEUE_CAPACITY: usize = 16;

/// Maximum time `Drop` is willing to wait for the worker thread to
/// finish after Shutdown is sent. If exceeded we `error!` and abandon
/// the `JoinHandle`. Picked at 5 seconds because legitimate work
/// shouldn't take anywhere near that long, and infinite blocking on
/// app shutdown is the worse failure mode.
const SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Which runtime a worker hosts. Decides which constructor the worker
/// thread runs and which runtime the dispatch loop calls into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkerKind {
    Js,
    #[cfg(feature = "native")]
    Lua,
}

/// Construction parameters for a worker thread.
pub struct WorkerSpawnConfig {
    /// Stable identifier (0 for the JS worker, 1 for Lua in 5.5b.1;
    /// becomes the chunk-routing key in 5.5b.2 once the pool grows to
    /// `Vec<Worker>`).
    pub id: u32,
    pub kind: WorkerKind,
    /// Lua-only buffer mode. Ignored for [`WorkerKind::Js`].
    #[cfg(feature = "native")]
    pub lua_buffer_mode: BatchLuaBufferMode,
}

/// One batch script worker: a thread + the request-channel sender.
pub struct BatchScriptWorker {
    pub id: u32,
    /// Runtime kind this worker hosts. Read by callers that want to
    /// route requests by language; not used internally yet.
    #[allow(dead_code)]
    pub kind: WorkerKind,
    /// Send end of the request channel. The receive end lives inside
    /// the worker thread.
    tx: Sender<BatchRequest>,
    /// `Some` until [`Self::shutdown`] is called or `Drop` runs.
    join: Option<JoinHandle<()>>,
}

impl BatchScriptWorker {
    /// Spawn the worker thread and return a handle. The runtime is
    /// constructed *inside* the thread so non-`Send` interior never
    /// crosses a thread boundary.
    pub fn spawn(config: WorkerSpawnConfig) -> Result<Self, BatchError> {
        let (tx, rx) = bounded::<BatchRequest>(REQUEST_QUEUE_CAPACITY);
        let WorkerSpawnConfig {
            id,
            kind,
            #[cfg(feature = "native")]
            lua_buffer_mode,
        } = config;

        // Spin up an init-status channel so we can surface
        // construction errors back to the caller synchronously
        // (otherwise the dispatcher would only learn about a failed
        // Runtime::new() on the first Load reply, which is awkward).
        let (init_tx, init_rx) = bounded::<Result<(), BatchError>>(1);

        let thread_name = match kind {
            WorkerKind::Js => format!("jengine-batch-js-{id}"),
            #[cfg(feature = "native")]
            WorkerKind::Lua => format!("jengine-batch-lua-{id}"),
        };

        let join = thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                run_worker_thread(
                    id,
                    kind,
                    rx,
                    init_tx,
                    #[cfg(feature = "native")]
                    lua_buffer_mode,
                );
            })
            .map_err(|e| BatchError::Init(format!("worker thread spawn failed: {e}")))?;

        // Block until the worker reports its Runtime::new() outcome.
        // If the channel disconnects the worker thread panicked
        // before sending — surface a generic init error.
        match init_rx.recv() {
            Ok(Ok(())) => Ok(Self { id, kind, tx, join: Some(join) }),
            Ok(Err(e)) => {
                // Worker exited cleanly with a runtime construction
                // failure. Wait for it so we don't leak the handle.
                let _ = join.join();
                Err(e)
            }
            Err(_) => {
                let _ = join.join();
                Err(BatchError::Init(
                    "worker thread exited before reporting init status".into(),
                ))
            }
        }
    }

    /// Send a request and wait for the response. Each request carries
    /// its own one-shot reply channel; the recv blocks until the
    /// worker handles it.
    pub fn send_load(&self, path: String, source: String) -> Result<LoadOk, BatchError> {
        let (reply_tx, reply_rx) = bounded::<Result<LoadOk, BatchError>>(1);
        if self
            .tx
            .send(BatchRequest::Load { path, source, respond_to: reply_tx })
            .is_err()
        {
            return Err(BatchError::Init("worker channel closed".into()));
        }
        reply_rx
            .recv()
            .unwrap_or_else(|_| Err(BatchError::Init("worker dropped reply".into())))
    }

    /// Send a `RunBatch` request and wait for the response.
    pub fn send_run_batch(
        &self,
        path: String,
        dt: f32,
        reads: Vec<SharedReadColumn>,
        writes: Vec<OwnedWriteColumn>,
        optionals: Vec<SharedOptionalColumn>,
        rng_seed: u64,
    ) -> Result<RunBatchOk, BatchError> {
        let reply_rx = self.dispatch_run_batch(path, dt, reads, writes, optionals, rng_seed)?;
        reply_rx
            .recv()
            .unwrap_or_else(|_| Err(BatchError::Init("worker dropped reply".into())))
    }

    /// Slice 5.5b.2: enqueue a `RunBatch` request without blocking and
    /// return the reply receiver. The chunked dispatcher uses this to
    /// fan out N requests across N workers in parallel, then drains
    /// the receivers in chunk-id order to merge the results.
    pub fn dispatch_run_batch(
        &self,
        path: String,
        dt: f32,
        reads: Vec<SharedReadColumn>,
        writes: Vec<OwnedWriteColumn>,
        optionals: Vec<SharedOptionalColumn>,
        rng_seed: u64,
    ) -> Result<Receiver<Result<RunBatchOk, BatchError>>, BatchError> {
        let (reply_tx, reply_rx) = bounded::<Result<RunBatchOk, BatchError>>(1);
        if self
            .tx
            .send(BatchRequest::RunBatch {
                path,
                dt,
                reads,
                writes,
                optionals,
                rng_seed,
                respond_to: reply_tx,
            })
            .is_err()
        {
            return Err(BatchError::Init("worker channel closed".into()));
        }
        Ok(reply_rx)
    }

    /// Trigger explicit shutdown without waiting. `Drop` does this
    /// automatically, but call it directly when you want to surface a
    /// shutdown error.
    pub fn shutdown(&self) {
        // Best-effort: if the channel is full or closed there's
        // nothing to do — the Drop path will still drop the Sender
        // and the worker will see Disconnected on its next recv.
        let _ = self.tx.try_send(BatchRequest::Shutdown);
    }
}

impl Drop for BatchScriptWorker {
    fn drop(&mut self) {
        self.shutdown();
        // Dropping the Sender forces any in-flight recv to return
        // Disconnected, so even if the explicit Shutdown was lost the
        // worker will exit on the next recv.
        // (Sender drop happens after the Shutdown send + before the
        // join, by virtue of the field order: tx is dropped after
        // join_handle below because struct drop order is field-decl
        // order. Force the issue with an explicit re-clone-and-drop.)
        // Safety: cloning a bounded Sender is cheap (refcount bump).
        // Dropping the clone here doesn't actually close the channel
        // — only the *last* Sender drop does — but the original tx
        // field will be dropped automatically after this method
        // returns, so the channel does close.
        if let Some(join) = self.join.take() {
            // Poll-and-bail: never block app shutdown indefinitely.
            let deadline = Instant::now() + SHUTDOWN_JOIN_TIMEOUT;
            while !join.is_finished() {
                if Instant::now() >= deadline {
                    error!(
                        "[BatchScriptWorker {}] thread did not finish within {:?}; abandoning JoinHandle",
                        self.id, SHUTDOWN_JOIN_TIMEOUT
                    );
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
            if let Err(panic) = join.join() {
                let msg = panic_message(&panic);
                warn!(
                    "[BatchScriptWorker {}] thread panicked during shutdown: {}",
                    self.id, msg
                );
            }
        }
    }
}

/// Worker thread entry point. Constructs the runtime, signals the
/// caller, then enters the recv → dispatch loop until the channel
/// disconnects, an explicit Shutdown lands, or a panic escapes a
/// request handler.
fn run_worker_thread(
    id: u32,
    kind: WorkerKind,
    rx: Receiver<BatchRequest>,
    init_tx: Sender<Result<(), BatchError>>,
    #[cfg(feature = "native")] lua_buffer_mode: BatchLuaBufferMode,
) {
    // Construct the runtime in-thread so its non-`Send` interior never
    // crosses a thread boundary.
    let mut backend = match kind {
        WorkerKind::Js => match BatchJsRuntime::new() {
            Ok(rt) => WorkerBackend::Js(rt),
            Err(e) => {
                let _ = init_tx.send(Err(e));
                return;
            }
        },
        #[cfg(feature = "native")]
        WorkerKind::Lua => match BatchLuaRuntime::new(lua_buffer_mode) {
            Ok(rt) => WorkerBackend::Lua(rt),
            Err(e) => {
                let _ = init_tx.send(Err(e));
                return;
            }
        },
    };

    // Signal the caller that init succeeded. If they've already
    // disconnected (race with caller drop) just exit.
    if init_tx.send(Ok(())).is_err() {
        return;
    }
    drop(init_tx);

    loop {
        let req = match rx.recv() {
            Ok(r) => r,
            Err(_) => return, // disconnected — caller dropped
        };

        // Wrap the request handler in catch_unwind. A panic here is
        // by definition a bug (script errors come back as Result).
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| handle_request(&mut backend, req)));

        match result {
            Ok(Continue::Yes) => continue,
            Ok(Continue::No) => return,
            Err(panic) => {
                let msg = panic_message(&panic);
                error!(
                    "[BatchScriptWorker {}] panic in request handler: {}; exiting worker",
                    id, msg
                );
                return;
            }
        }
    }
}

/// Worker-internal storage for the runtime. The variants mirror
/// [`WorkerKind`].
enum WorkerBackend {
    Js(BatchJsRuntime),
    #[cfg(feature = "native")]
    Lua(BatchLuaRuntime),
}

/// Recv-loop continuation signal returned by [`handle_request`].
enum Continue {
    Yes,
    No,
}

/// Process one request. Pure dispatch: no panic-handling, no
/// channel-drop logic; the caller wraps this in `catch_unwind` and
/// the recv loop.
fn handle_request(backend: &mut WorkerBackend, req: BatchRequest) -> Continue {
    match req {
        BatchRequest::Shutdown => Continue::No,
        BatchRequest::Load { path, source, respond_to } => {
            let result = handle_load(backend, &path, &source);
            // If the caller dropped the reply channel just keep going
            // — they've moved on and we shouldn't crash on their
            // behalf.
            let _ = respond_to.send(result);
            Continue::Yes
        }
        BatchRequest::RunBatch { path, dt, reads, writes, optionals, rng_seed, respond_to } => {
            let result = handle_run_batch(backend, &path, dt, reads, writes, optionals, rng_seed);
            let _ = respond_to.send(result);
            Continue::Yes
        }
    }
}

fn handle_load(backend: &mut WorkerBackend, path: &str, source: &str) -> Result<LoadOk, BatchError> {
    match backend {
        WorkerBackend::Js(rt) => {
            rt.load(path, source)?;
            Ok(LoadOk {
                read_schema: rt.read_schema(path).unwrap_or(&[]).to_vec(),
                write_schema: rt.write_schema(path).unwrap_or(&[]).to_vec(),
                read_optional_schema: rt.read_optional_schema(path).unwrap_or(&[]).to_vec(),
            })
        }
        #[cfg(feature = "native")]
        WorkerBackend::Lua(rt) => {
            rt.load(path, source)?;
            Ok(LoadOk {
                read_schema: rt.read_schema(path).unwrap_or(&[]).to_vec(),
                write_schema: rt.write_schema(path).unwrap_or(&[]).to_vec(),
                read_optional_schema: rt.read_optional_schema(path).unwrap_or(&[]).to_vec(),
            })
        }
    }
}

fn handle_run_batch(
    backend: &mut WorkerBackend,
    path: &str,
    dt: f32,
    reads: Vec<SharedReadColumn>,
    mut writes: Vec<OwnedWriteColumn>,
    optionals: Vec<SharedOptionalColumn>,
    rng_seed: u64,
) -> Result<RunBatchOk, BatchError> {
    // Slice 5.5c.1: reads/optionals carry `Arc<Vec<f32>>` + `range`
    // so the per-chunk data is just a refcount bump on the
    // dispatcher side. Here we slice the Arc into the borrowed view
    // the runtime expects — zero element copies.
    let read_cols: Vec<BatchReadColumn<'_>> = reads
        .iter()
        .map(|c| BatchReadColumn {
            name: c.name.as_str(),
            x: &c.x[c.range.clone()],
            y: &c.y[c.range.clone()],
        })
        .collect();
    let mut write_cols: Vec<BatchWriteColumn<'_>> = writes
        .iter_mut()
        .map(|c| BatchWriteColumn {
            name: c.name.as_str(),
            x: c.x.as_mut_slice(),
            y: c.y.as_mut_slice(),
        })
        .collect();
    let opt_cols: Vec<BatchOptionalColumn<'_>> = optionals
        .iter()
        .map(|c| BatchOptionalColumn {
            name: c.name.as_str(),
            value: c.value.as_ref().map(|(x, y)| (&x[c.range.clone()], &y[c.range.clone()])),
        })
        .collect();

    let mut write_dirty: Vec<bool> = vec![false; write_cols.len()];
    let mut counter_deltas = FrameCounters::default();

    // Slice 5.5c — reseed the deterministic RNG before invoking the
    // script so any `Math.random()` / `math.random()` call from
    // script code is fed by a per-(scene_seed, frame, system_id,
    // chunk_id) stream.
    match backend {
        WorkerBackend::Js(rt) => rt.set_rng_seed(rng_seed),
        #[cfg(feature = "native")]
        WorkerBackend::Lua(rt) => rt.set_rng_seed(rng_seed),
    }

    let elapsed_us = match backend {
        WorkerBackend::Js(rt) => rt.run_batch(
            path,
            dt,
            &read_cols,
            &mut write_cols,
            &opt_cols,
            &mut [],
            &mut write_dirty,
            &mut counter_deltas,
        )?,
        #[cfg(feature = "native")]
        WorkerBackend::Lua(rt) => rt.run_batch(
            path,
            dt,
            &read_cols,
            &mut write_cols,
            &opt_cols,
            &mut [],
            &mut write_dirty,
            &mut counter_deltas,
        )?,
    };

    // Drop the borrowed-slice views before moving `writes` into the
    // response.
    drop(read_cols);
    drop(write_cols);
    drop(opt_cols);

    // Slice 5.5c — sample worker VM memory at frame-boundary
    // (run_batch returned, no in-flight allocations). Reported back
    // to the dispatcher inside RunBatchOk.
    let worker_memory_bytes = match backend {
        WorkerBackend::Js(rt) => rt.memory_bytes(),
        #[cfg(feature = "native")]
        WorkerBackend::Lua(rt) => rt.memory_bytes(),
    };

    Ok(RunBatchOk { writes, write_dirty, elapsed_us, counter_deltas, worker_memory_bytes })
}

/// Best-effort string extraction from a panic payload. Standard
/// pattern from `std::panic::catch_unwind` docs.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

// (Helpers `payload_bytes_in` / `response_bytes` are exported from
// `super::channel` and re-exported by `mod.rs`; the worker uses them
// indirectly via the request/response payload types.)
