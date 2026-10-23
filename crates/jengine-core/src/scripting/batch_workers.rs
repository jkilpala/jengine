//! Slice 4C.1: persistent JS batch worker pool.
//!
//! Each worker thread owns ONE [`BatchJsRuntime`] (one rquickjs `Runtime` +
//! `Context` + loaded scripts). The pool is created once at startup with the
//! full script source list; every worker loads every script independently
//! into its own VM, so dispatch is just "send a job → wait for the result".
//!
//! Cross-thread communication uses owned `Vec<f32>` job/result payloads —
//! no shared mutable state and no cross-thread VM access. `rquickjs::Runtime`
//! is `!Send`, but each runtime is constructed AND dropped on its own worker
//! thread, so the pool itself only ever moves the channel endpoints across
//! thread boundaries.
//!
//! Scope (Slice 4C.1):
//! - JS only. Lua workers are a follow-up slice (mlua needs the `send`
//!   feature, plus the per-script env/buffer machinery has to be made
//!   thread-safe; out of scope here).
//! - One walking-skeleton entry point: `run_velocity_to_position`.
//! - Static script set: scripts are loaded into every worker at pool
//!   construction. Live reload is a follow-up.

#![cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::profiling::FrameCounters;

use super::batch::{BatchError, BatchJsRuntime, BatchStateField};

/// A unit of work sent to a worker.
enum Job {
    Run {
        path: String,
        dt: f32,
        vx: Vec<f32>,
        vy: Vec<f32>,
        px: Vec<f32>,
        py: Vec<f32>,
        /// Slice 5.1: typed state columns, one per declared field, in
        /// the script's schema order. Empty for scripts with no state.
        state: Vec<Vec<f32>>,
        /// Slice 5.2: optional Acceleration2D columns. `Some((ax, ay))`
        /// when every entity in this chunk has the component, `None`
        /// otherwise (the dispatcher partitions by archetype before
        /// chunking, so a single Job is uniform).
        accel: Option<(Vec<f32>, Vec<f32>)>,
    },
    Stop,
}

/// Per-job result returned by the worker.
struct JobResult {
    px: Vec<f32>,
    py: Vec<f32>,
    /// Slice 5.1: state columns written back by the script.
    state: Vec<Vec<f32>>,
    counters: FrameCounters,
    elapsed_us: u64,
    error: Option<String>,
}

/// One worker = one OS thread + one channel pair.
struct Worker {
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<JobResult>,
    handle: Option<JoinHandle<()>>,
}

/// Persistent pool of JS batch workers. Drop joins all threads cleanly.
pub struct BatchJsWorkerPool {
    workers: Vec<Worker>,
    /// Slice 5.1: per-script state schemas, parsed once at pool
    /// construction by a transient main-thread runtime. The pool
    /// dispatcher uses these to size + initialize per-entity state
    /// columns before sending Jobs to workers. Workers also have the
    /// schemas in their own runtimes (parsed independently from the
    /// same source); this cache exists so the dispatcher can look up
    /// `state.len()` without a round-trip.
    schemas: HashMap<String, Vec<BatchStateField>>,
}

impl BatchJsWorkerPool {
    /// Spawn `worker_count` threads, each constructing a fresh
    /// `BatchJsRuntime` and loading every `(path, source)` pair into it.
    /// Errors during script load are logged inside the worker but do not
    /// abort the pool — a missing path will just return `MissingUpdate`
    /// when dispatched, matching the single-VM behaviour.
    pub fn new(
        worker_count: usize,
        scripts: &[(String, String)],
    ) -> Result<Self, BatchError> {
        if worker_count == 0 {
            return Err(BatchError::Init("worker_count must be >= 1".to_string()));
        }
        // Slice 5.1: parse schemas once on the main thread so the
        // dispatcher can size state columns without round-trips.
        // A transient runtime is the simplest way; it's torn down
        // before any worker is spawned.
        let mut schemas: HashMap<String, Vec<BatchStateField>> = HashMap::new();
        {
            let mut probe = BatchJsRuntime::new()?;
            for (path, src) in scripts {
                if probe.load(path, src).is_ok() {
                    // Record EVERY successfully loaded script (including
                    // those with empty schemas) so the dispatcher can
                    // distinguish "loaded with no state" (Some(&[]))
                    // from "not loaded" (None).
                    let schema = probe.state_schema(path).unwrap_or(&[]).to_vec();
                    schemas.insert(path.clone(), schema);
                }
            }
        }
        let mut workers = Vec::with_capacity(worker_count);
        for worker_idx in 0..worker_count {
            let (job_tx, job_rx) = mpsc::channel::<Job>();
            let (res_tx, res_rx) = mpsc::channel::<JobResult>();
            let scripts_cloned: Vec<(String, String)> = scripts.to_vec();
            let handle = thread::Builder::new()
                .name(format!("batch-js-worker-{}", worker_idx))
                .spawn(move || worker_loop(worker_idx, scripts_cloned, job_rx, res_tx))
                .map_err(|e| BatchError::Init(format!("spawn worker: {e}")))?;
            workers.push(Worker { tx: job_tx, rx: res_rx, handle: Some(handle) });
        }
        Ok(Self { workers, schemas })
    }

    /// Number of worker threads in the pool.
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    /// Slice 5.1: declared state schema for `path`, or `None` if the
    /// script wasn't loaded into the pool's probe runtime. Returns
    /// `Some(&[])` for loaded scripts that declared no state.
    pub fn state_schema(&self, path: &str) -> Option<&[BatchStateField]> {
        self.schemas.get(path).map(Vec::as_slice)
    }

    /// Partition the four input columns evenly across workers, dispatch,
    /// and collect results in worker-id order so positions are written
    /// back to the original indices. Counters are SUMMED across workers
    /// (so cp_in/run/cp_out reflect total CPU work); the returned
    /// `elapsed_us` is the MAX of per-worker elapsed (i.e. wall-clock
    /// proxy for the parallel section).
    pub fn run_velocity_to_position(
        &self,
        path: &str,
        dt: f32,
        velocity_x: &[f32],
        velocity_y: &[f32],
        position_x: &mut [f32],
        position_y: &mut [f32],
        state: &mut [Vec<f32>],
        // Slice 5.2: optional Acceleration2D columns; `None` skips the
        // optional field for every chunk. When `Some`, the slices are
        // sub-sliced per chunk just like vx/vy.
        accel: Option<(&[f32], &[f32])>,
        counters: &mut FrameCounters,
    ) -> Result<u64, BatchError> {
        debug_assert_eq!(velocity_x.len(), velocity_y.len());
        debug_assert_eq!(position_x.len(), position_y.len());
        debug_assert_eq!(velocity_x.len(), position_x.len());
        if let Some((ax, ay)) = accel {
            debug_assert_eq!(ax.len(), velocity_x.len());
            debug_assert_eq!(ay.len(), velocity_x.len());
        }

        let n = velocity_x.len();
        let workers = self.workers.len();
        if n == 0 || workers == 0 {
            return Ok(0);
        }

        // Slice 5.1: validate state column lengths up front.
        for (i, col) in state.iter().enumerate() {
            debug_assert_eq!(
                col.len(),
                n,
                "state column #{i} has wrong length (got {}, expected {n})",
                col.len(),
            );
        }

        let chunk = n.div_ceil(workers);
        let mut sent = 0usize;
        for (i, w) in self.workers.iter().enumerate() {
            let lo = i * chunk;
            if lo >= n {
                break;
            }
            let hi = ((i + 1) * chunk).min(n);
            // Slice 5.1: slice each state column into the per-chunk
            // payload. Empty `state` produces empty `state_chunk`.
            let state_chunk: Vec<Vec<f32>> =
                state.iter().map(|col| col[lo..hi].to_vec()).collect();
            let accel_chunk = accel.map(|(ax, ay)| (ax[lo..hi].to_vec(), ay[lo..hi].to_vec()));
            let job = Job::Run {
                path: path.to_string(),
                dt,
                vx: velocity_x[lo..hi].to_vec(),
                vy: velocity_y[lo..hi].to_vec(),
                px: position_x[lo..hi].to_vec(),
                py: position_y[lo..hi].to_vec(),
                state: state_chunk,
                accel: accel_chunk,
            };
            w.tx.send(job).map_err(|e| BatchError::RunFailed {
                path: path.to_string(),
                message: format!("worker {i} send failed: {e}"),
            })?;
            sent += 1;
        }

        let mut max_elapsed = 0u64;
        let mut first_error: Option<BatchError> = None;
        for (i, w) in self.workers.iter().take(sent).enumerate() {
            let lo = i * chunk;
            let hi = ((i + 1) * chunk).min(n);
            let res = w.rx.recv().map_err(|e| BatchError::RunFailed {
                path: path.to_string(),
                message: format!("worker {i} recv failed: {e}"),
            })?;
            position_x[lo..hi].copy_from_slice(&res.px);
            position_y[lo..hi].copy_from_slice(&res.py);
            // Slice 5.1: scatter state columns back at the same offset.
            for (col, returned) in state.iter_mut().zip(res.state.iter()) {
                col[lo..hi].copy_from_slice(returned);
            }
            counters.batch_copy_in_us += res.counters.batch_copy_in_us;
            counters.batch_run_us += res.counters.batch_run_us;
            counters.batch_copy_out_us += res.counters.batch_copy_out_us;
            counters.batch_copy_in_bytes += res.counters.batch_copy_in_bytes;
            counters.batch_copy_out_bytes += res.counters.batch_copy_out_bytes;
            counters.batch_dirty_bytes += res.counters.batch_dirty_bytes;
            counters.batch_vm_calls += res.counters.batch_vm_calls;
            if res.elapsed_us > max_elapsed {
                max_elapsed = res.elapsed_us;
            }
            if first_error.is_none() {
                if let Some(msg) = res.error {
                    first_error = Some(BatchError::RunFailed {
                        path: path.to_string(),
                        message: msg,
                    });
                }
            }
        }

        if let Some(e) = first_error {
            Err(e)
        } else {
            Ok(max_elapsed)
        }
    }
}

impl Drop for BatchJsWorkerPool {
    fn drop(&mut self) {
        for w in &self.workers {
            let _ = w.tx.send(Job::Stop);
        }
        for w in &mut self.workers {
            if let Some(h) = w.handle.take() {
                let _ = h.join();
            }
        }
    }
}

fn worker_loop(
    worker_idx: usize,
    scripts: Vec<(String, String)>,
    job_rx: mpsc::Receiver<Job>,
    res_tx: mpsc::Sender<JobResult>,
) {
    let mut rt = match BatchJsRuntime::new() {
        Ok(rt) => rt,
        Err(e) => {
            log::error!("[BatchWorker {worker_idx}] init failed: {e}");
            return;
        }
    };
    for (path, src) in &scripts {
        if let Err(e) = rt.load(path, src) {
            log::error!("[BatchWorker {worker_idx}] load '{path}' failed: {e}");
        }
    }
    log::info!(
        "[BatchWorker {worker_idx}] ready ({} scripts loaded)",
        rt.loaded_count()
    );

    while let Ok(job) = job_rx.recv() {
        match job {
            Job::Stop => break,
            Job::Run { path, dt, vx, vy, mut px, mut py, mut state, accel } => {
                let mut counters = FrameCounters::default();
                let start = Instant::now();
                let accel_ref = accel.as_ref().map(|(ax, ay)| (ax.as_slice(), ay.as_slice()));
                let result = rt.run_velocity_to_position(
                    &path, dt, &vx, &vy, &mut px, &mut py, &mut state, accel_ref, &mut counters,
                );
                let elapsed_us = start.elapsed().as_micros() as u64;
                let res = match result {
                    Ok(_) => JobResult { px, py, state, counters, elapsed_us, error: None },
                    Err(e) => JobResult {
                        px,
                        py,
                        state,
                        counters,
                        elapsed_us,
                        error: Some(e.to_string()),
                    },
                };
                if res_tx.send(res).is_err() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = r#"
        const system = { query: { read: ["Velocity2D"], write: ["Position2D"] } };
        function update(ctx, q) {
            const n = q.count;
            const vx = q.read.Velocity2D.x;
            const vy = q.read.Velocity2D.y;
            const px = q.write.Position2D.x;
            const py = q.write.Position2D.y;
            const dt = ctx.dt;
            for (let i = 0; i < n; i++) {
                px[i] = px[i] + vx[i] * dt;
                py[i] = py[i] + vy[i] * dt;
            }
        }
    "#;

    fn make_columns(n: usize) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>) {
        let vx: Vec<f32> = (0..n).map(|i| (i as f32) * 0.5).collect();
        let vy: Vec<f32> = (0..n).map(|i| (i as f32) * -0.25).collect();
        let px: Vec<f32> = (0..n).map(|i| (i as f32) * 10.0).collect();
        let py: Vec<f32> = (0..n).map(|i| (i as f32) * 20.0).collect();
        (vx, vy, px, py)
    }

    #[test]
    fn pool_matches_single_vm_for_n_workers() {
        // Reference: single-VM run.
        let mut single = BatchJsRuntime::new().expect("single rt");
        single.load("test.js", SCRIPT).expect("load single");
        let (vx, vy, mut px_single, mut py_single) = make_columns(127);
        let mut counters_single = FrameCounters::default();
        single
            .run_velocity_to_position(
                "test.js",
                1.0 / 60.0,
                &vx,
                &vy,
                &mut px_single,
                &mut py_single,
                &mut [],
                None,
                &mut counters_single,
            )
            .expect("single run");

        // Same input, dispatched through pools of 1, 2, 4 workers.
        for worker_count in [1usize, 2, 4] {
            let pool = BatchJsWorkerPool::new(
                worker_count,
                &[("test.js".to_string(), SCRIPT.to_string())],
            )
            .expect("pool");
            let (vx2, vy2, mut px2, mut py2) = make_columns(127);
            let mut c = FrameCounters::default();
            pool.run_velocity_to_position(
                "test.js",
                1.0 / 60.0,
                &vx2,
                &vy2,
                &mut px2,
                &mut py2,
                &mut [],
                None,
                &mut c,
            )
            .expect("pool run");
            assert_eq!(px2, px_single, "px mismatch at worker_count={worker_count}");
            assert_eq!(py2, py_single, "py mismatch at worker_count={worker_count}");
        }
    }

    #[test]
    fn pool_handles_more_workers_than_entities() {
        // 4 workers, 3 entities — only 3 workers should receive jobs.
        let pool =
            BatchJsWorkerPool::new(4, &[("test.js".to_string(), SCRIPT.to_string())]).expect("pool");
        let (vx, vy, mut px, mut py) = make_columns(3);
        let expected_x: Vec<f32> = (0..3).map(|i| (i as f32) * 10.0 + (i as f32) * 0.5 / 60.0).collect();
        let mut c = FrameCounters::default();
        pool.run_velocity_to_position("test.js", 1.0 / 60.0, &vx, &vy, &mut px, &mut py, &mut [], None, &mut c)
            .expect("run");
        for i in 0..3 {
            assert!((px[i] - expected_x[i]).abs() < 1e-4, "i={i}");
        }
    }

    #[test]
    fn pool_empty_input_is_noop() {
        let pool =
            BatchJsWorkerPool::new(2, &[("test.js".to_string(), SCRIPT.to_string())]).expect("pool");
        let mut px = Vec::<f32>::new();
        let mut py = Vec::<f32>::new();
        let mut c = FrameCounters::default();
        let elapsed = pool
            .run_velocity_to_position("test.js", 1.0 / 60.0, &[], &[], &mut px, &mut py, &mut [], None, &mut c)
            .expect("run");
        assert_eq!(elapsed, 0);
        assert_eq!(c.batch_run_us, 0);
    }

    /// Slice 5.1: a stateful script dispatched through a 4-worker pool
    /// produces the same per-entity state as a single-VM run after the
    /// same number of invocations. Verifies state partition + scatter is
    /// chunk-id-deterministic.
    #[test]
    fn pool_state_columns_match_single_vm() {
        const STATEFUL: &str = r#"
            const system = {
              query: {
                read: ["Velocity2D"],
                write: ["Position2D"],
                state: { acc: { type: "f32", default: 0.0 } }
              }
            };
            function update(ctx, q) {
                // Per-entity stateful update — uses only the entity's own
                // velocity, not the chunk-local index, so the result is
                // identical regardless of how the column is partitioned.
                const acc = q.state.acc;
                const vx = q.read.Velocity2D.x;
                for (let i = 0; i < q.count; i++) acc[i] += ctx.dt * vx[i];
            }
        "#;
        let n = 64;

        // Reference: single-VM, 5 invocations.
        let mut single = BatchJsRuntime::new().unwrap();
        single.load("s.js", STATEFUL).unwrap();
        let (vx, vy, mut px, mut py) = make_columns(n);
        let mut state_single = vec![vec![0.0_f32; n]];
        let mut c = FrameCounters::default();
        for _ in 0..5 {
            single
                .run_velocity_to_position(
                    "s.js", 0.1, &vx, &vy, &mut px, &mut py, &mut state_single, None, &mut c,
                )
                .unwrap();
        }

        // Pool with 4 workers, same input.
        let pool = BatchJsWorkerPool::new(4, &[("s.js".to_string(), STATEFUL.to_string())])
            .expect("pool");
        // Schema lookup works.
        let schema = pool.state_schema("s.js").expect("schema present");
        assert_eq!(schema.len(), 1);
        assert_eq!(schema[0].name, "acc");

        let (vx2, vy2, mut px2, mut py2) = make_columns(n);
        let mut state_pool = vec![vec![0.0_f32; n]];
        let mut c2 = FrameCounters::default();
        for _ in 0..5 {
            pool.run_velocity_to_position(
                "s.js", 0.1, &vx2, &vy2, &mut px2, &mut py2, &mut state_pool, None, &mut c2,
            )
            .unwrap();
        }
        assert_eq!(state_pool, state_single, "state must match single-VM byte-for-byte");
    }
}
