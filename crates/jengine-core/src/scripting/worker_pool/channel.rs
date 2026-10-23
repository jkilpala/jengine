//! Slice 5.5b.1: channel protocol shared between the dispatcher and a
//! batch script worker thread.
//!
//! Dedicated-worker-thread + channel model (see
//! [`docs/archive/scripting-redesign-plan.md`] Slice 5.5b for the rationale).
//! Each batch language (JS, Lua) gets a long-lived worker thread that
//! owns its `rquickjs::Runtime` / `mlua::Lua` for life. Runtimes never
//! cross threads — only the request/response payloads defined in this
//! module do, and every variant is `Send + 'static` by construction.
//!
//! Why not the `mlua` `send` feature? See
//! [`docs/scripting-performance-lessons.md`] subsection
//! "Why dedicated-worker-thread over mlua send".
//!
//! Wire format:
//!
//! - Main thread → worker: [`BatchRequest`] (Load / RunBatch / Shutdown).
//! - Worker → main thread: [`BatchResponse`] (per-request reply).
//!
//! Each request carries a `respond_to: Sender<BatchResponse>` so the
//! main thread can wait for the matching reply without coupling the
//! request channel to a single in-flight call.

#![cfg(all(not(target_arch = "wasm32"), any(feature = "js-adapter-host", feature = "native")))]

use std::ops::Range;
use std::sync::Arc;

use crossbeam_channel::Sender;

use crate::scripting::batch::BatchError;

/// One read column shipped across the channel as owned data. Mirrors
/// [`crate::scripting::batch::BatchReadColumn`] but with `Vec<f32>`
/// instead of borrowed slices so the payload is `Send + 'static`.
#[derive(Debug, Clone)]
pub struct OwnedReadColumn {
    pub name: String,
    pub x: Vec<f32>,
    pub y: Vec<f32>,
}

/// Slice 5.5c.1 — zero-copy read column for the chunked Pooled path.
///
/// Each `Arc<Vec<f32>>` is allocated *without copying its elements*
/// (`Arc::new(vec)` only wraps the Vec metadata; the heap buffer
/// stays put). Per-chunk fan-out then ships only a refcount bump +
/// a sub-`range`. At N=4 with 30k×5r workloads this drops ~1 MB of
/// per-frame main-thread `memcpy` to ~0.
///
/// `Arc<[f32]>` would force a memcpy on `Arc::from(Vec)` (stdlib
/// allocates fresh `ArcInner<[T]>` storage); `Arc<Vec<f32>>` does
/// not. We deref to `&[f32]` via `Vec`'s `Deref` for slicing.
///
/// Worker side reads via `&self.x[self.range.clone()]`.
#[derive(Debug, Clone)]
pub struct SharedReadColumn {
    pub name: String,
    pub x: Arc<Vec<f32>>,
    pub y: Arc<Vec<f32>>,
    pub range: Range<usize>,
}

/// One write column shipped across the channel. Pre-populated by the
/// dispatcher before send; the worker mutates the vectors in place
/// (read-modify-write semantics) and ships them back inside
/// [`RunBatchOk::writes`].
#[derive(Debug, Clone)]
pub struct OwnedWriteColumn {
    pub name: String,
    pub x: Vec<f32>,
    pub y: Vec<f32>,
}

/// One optional read column. `value` is `Some` when every entity in
/// the chunk has the component, `None` otherwise.
#[derive(Debug, Clone)]
pub struct OwnedOptionalColumn {
    pub name: String,
    pub value: Option<(Vec<f32>, Vec<f32>)>,
}

/// Slice 5.5c.1 — zero-copy optional column counterpart to
/// [`SharedReadColumn`]. Same `Arc<Vec<f32>>` + sub-range pattern.
#[derive(Debug, Clone)]
pub struct SharedOptionalColumn {
    pub name: String,
    pub value: Option<(Arc<Vec<f32>>, Arc<Vec<f32>>)>,
    pub range: Range<usize>,
}

/// Successful `RunBatch` outcome.
#[derive(Debug)]
pub struct RunBatchOk {
    /// Final write-column values after the script ran. The dispatcher
    /// scatters these back into ECS components.
    pub writes: Vec<OwnedWriteColumn>,
    /// Per-write-column dirty mask (Slice 5.4c). When `false`, the
    /// dispatcher skips the per-entity scatter for that column.
    pub write_dirty: Vec<bool>,
    /// Wall-clock microseconds the worker spent inside `run_batch`,
    /// excluding channel send/recv. Same value the runtime returns
    /// today; surfaced separately so the dispatcher can compute the
    /// channel-overhead delta.
    pub elapsed_us: u64,
    /// Counters the worker accumulated during `run_batch`. The
    /// dispatcher folds these into the per-frame `FrameCounters`
    /// after recv. We ship the whole struct (it's `Copy + Default`)
    /// rather than a hand-curated delta type — keeps additions cheap.
    pub counter_deltas: crate::profiling::FrameCounters,
    /// Slice 5.5c — worker VM allocator footprint sampled *after*
    /// `run_batch` returned (worker idle, batch fully complete). The
    /// dispatcher max-aggregates this across chunks into the
    /// per-frame `script_worker_memory_bytes_max` counter and sums
    /// it into `script_worker_memory_bytes_total`.
    pub worker_memory_bytes: u64,
}

/// Reply to a [`BatchRequest::Load`].
#[derive(Debug)]
pub struct LoadOk {
    /// The schemas the runtime parsed from the script. Cached on the
    /// main thread so the dispatcher's per-frame `is_loaded` /
    /// `read_schema` / `write_schema` / `read_optional_schema` calls
    /// stay synchronous (no per-frame round-trip just to fetch static
    /// metadata).
    pub read_schema: Vec<String>,
    pub write_schema: Vec<String>,
    pub read_optional_schema: Vec<String>,
}

/// Requests the dispatcher sends to the worker.
pub enum BatchRequest {
    /// Load a script source. The worker compiles it and replies with
    /// the parsed schemas (so the main thread can populate its cache).
    Load {
        path: String,
        source: String,
        respond_to: Sender<Result<LoadOk, BatchError>>,
    },
    /// Run a single batched call. The worker invokes the script with
    /// the supplied columns, accumulates counter deltas, and ships the
    /// (possibly mutated) write columns back.
    RunBatch {
        path: String,
        dt: f32,
        reads: Vec<SharedReadColumn>,
        writes: Vec<OwnedWriteColumn>,
        optionals: Vec<SharedOptionalColumn>,
        /// Slice 5.5c — deterministic RNG seed for this batch.
        /// The worker reseeds its embedded RNG before invoking the
        /// script so any `Math.random()` / `math.random()` call from
        /// script code is fed by the seeded stream. Same seed across
        /// runs → identical script outputs.
        rng_seed: u64,
        respond_to: Sender<Result<RunBatchOk, BatchError>>,
    },
    /// Explicit shutdown. The worker drains pending work, replies to
    /// all in-flight `respond_to` channels with a synthetic error if
    /// it can, and returns from its run loop. See `worker.rs` for the
    /// shutdown discipline.
    Shutdown,
}

/// Sum of bytes a single `RunBatch` payload moves across the channel
/// in each direction. Used for the `batch_payload_clone_bytes` /
/// `batch_response_bytes` counters so we can quantify channel pressure.
///
/// Counts only the f32 column data (the meaningful copy cost). String
/// names and Vec headers are excluded — they're constant per script
/// and dwarfed by the column bytes.
#[inline]
pub fn payload_bytes_in(
    reads: &[OwnedReadColumn],
    writes: &[OwnedWriteColumn],
    optionals: &[OwnedOptionalColumn],
) -> u64 {
    let mut bytes: u64 = 0;
    for c in reads {
        bytes += (c.x.len() + c.y.len()) as u64 * 4;
    }
    for c in writes {
        bytes += (c.x.len() + c.y.len()) as u64 * 4;
    }
    for c in optionals {
        if let Some((x, y)) = &c.value {
            bytes += (x.len() + y.len()) as u64 * 4;
        }
    }
    bytes
}

#[inline]
pub fn response_bytes(writes: &[OwnedWriteColumn]) -> u64 {
    let mut bytes: u64 = 0;
    for c in writes {
        bytes += (c.x.len() + c.y.len()) as u64 * 4;
    }
    bytes
}
