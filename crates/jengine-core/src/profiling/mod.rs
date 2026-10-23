//! Frame profiling — performance stats collection and reporting.
//!
//! Provides `FrameProfiler` for timing named sections of the game loop
//! and `FrameStats` for reading aggregated per-frame performance data.
//! Platform timing: `std::time::Instant` on native, `Performance.now()` on WASM.
//!
//! ## Two layers (Step 1 redesign)
//!
//! - **`FrameStats`** — legacy flat struct, kept for editor / window-title /
//!   scripting-API back-compat. Populated from the new layer below.
//! - **`FrameTimings` + `FrameCounters`** — the new source of truth.
//!   Tree-shaped, non-overlapping children. `other_ms` is derived, never
//!   measured. See [`docs/archive/scripting-redesign-plan.md`] Step 1 + the
//!   v1 baseline schema in `docs/archive/scripting-redesign-step1-profiling.md`.

use std::collections::HashMap;

/// Number of frames to keep in the rolling history buffer.
const HISTORY_SIZE: usize = 120;

/// Number of recent frames used to compute smoothed FPS.
const FPS_SMOOTHING_FRAMES: usize = 60;

/// Heap / linear-memory accounting. See [`memory::TrackingAllocator`].
pub mod memory;

/// Spans, per-script costs and per-pass draws, recorded into captures. See
/// [`trace`].
pub mod trace;

/// Aggregated performance statistics for a single frame.
///
/// **Back-compat shim.** Read by the editor, window title, and the
/// scripting API (`engine.frame_stats()`). New code should consume
/// [`FrameTimings`] / [`FrameCounters`] instead.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FrameStats {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub update_time_ms: f32,
    pub render_time_ms: f32,
    pub physics_time_ms: f32,
    pub script_time_ms: f32,
    pub draw_calls: u32,
    pub sprite_count: u32,
    pub entity_count: u32,
    pub particle_count: u32,
    pub batch_flushes: u32,
    /// Bytes the process is holding: reserved wasm linear memory in a
    /// browser, live heap natively. 0 when the host has not installed
    /// `memory::TrackingAllocator` and is not on wasm.
    pub memory_bytes: u64,
    /// High-water mark of the live heap. 0 without a tracking allocator.
    pub memory_peak_bytes: u64,
    /// Bytes live on the heap right now. Equal to `memory_bytes` off wasm;
    /// in a browser it is the smaller, more useful number, because
    /// `memory_bytes` there is reserved linear memory, which never shrinks.
    /// Watch this one to tell a leak from allocator high-water: a scene
    /// reload that returns here to its old value leaked nothing, however
    /// much reserved memory grew.
    pub memory_live_bytes: u64,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self {
            fps: 0.0,
            frame_time_ms: 0.0,
            update_time_ms: 0.0,
            render_time_ms: 0.0,
            physics_time_ms: 0.0,
            script_time_ms: 0.0,
            draw_calls: 0,
            sprite_count: 0,
            entity_count: 0,
            memory_bytes: 0,
            memory_peak_bytes: 0,
            memory_live_bytes: 0,
            particle_count: 0,
            batch_flushes: 0,
        }
    }
}

/// Per-frame script-subsystem timing breakdown (milliseconds).
///
/// Buckets are non-overlapping; their sum is the wall-clock spent inside
/// `ScriptSystem::run`. Populated by `ScriptSystem` every frame (not
/// gated by `JENGINE_SCRIPT_PROFILE` — the env var only toggles logging).
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct ScriptTimings {
    /// Clear previous frame state + bridge snapshot + subsystem snapshot
    /// + collect/load entity scripts.
    pub prepare_ms: f32,
    pub run_lua_ms: f32,
    pub run_js_ms: f32,
    /// Translate bridge commands into engine `ScriptCommand`s.
    pub route_ms: f32,
    /// Apply commands to the world (spawn/despawn/set_field/...).
    pub apply_ms: f32,
}

impl ScriptTimings {
    pub fn total_ms(&self) -> f32 {
        self.prepare_ms + self.run_lua_ms + self.run_js_ms + self.route_ms + self.apply_ms
    }
}

/// Per-frame nested timing tree. Step 1 source of truth.
///
/// Children are non-overlapping. `other_ms` is **derived**
/// (`frame_ms - scripts.total - physics_ms - render_ms - swap_ms`),
/// never measured. `swap_ms` is kept separate from CPU-sim totals so
/// vsync wait can't masquerade as frame cost.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct FrameTimings {
    pub frame_ms: f32,
    pub scripts: ScriptTimings,
    pub physics_ms: f32,
    pub render_ms: f32,
    pub swap_ms: f32,
    pub other_ms: f32,
}

impl FrameTimings {
    pub fn cpu_sim_ms(&self) -> f32 {
        self.scripts.total_ms() + self.physics_ms + self.render_ms + self.other_ms
    }
}

/// Per-frame counters. See `docs/archive/scripting-redesign-step1-profiling.md`
/// for the full v1 schema (units, proxy/stub flags).
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct FrameCounters {
    // directional VM crossings
    pub rust_to_vm_calls: u64,
    pub vm_to_rust_host_calls_lua: u64,
    pub vm_to_rust_host_calls_js: u64,

    // script work
    pub script_invocations: u64,
    pub entities_ticked: u64,
    pub events_dispatched: u64,
    pub events_queued: u64,         // 0 in v1 — Step 3 scheduler
    pub events_delivered: u64,
    pub events_dropped_or_deferred: u64, // 0 in v1 — Step 3

    // data movement
    pub script_snapshot_bytes_in: u64, // proxy: HashMap entries × size_of K+V
    pub script_writeback_bytes_out: u64, // proxy: cmd count × size_of::<BridgeCommand>
    pub script_property_gets: u64,
    pub script_property_sets: u64,

    // commands
    pub commands_count: u64,
    pub commands_bytes_proxy: u64, // proxy: count × size_of::<BridgeCommand>

    // ── Step 4 batch JS systems ──
    /// Number of distinct batch systems invoked this frame.
    pub batch_systems_run: u64,
    /// Bytes copied from ECS columns into per-call mirror TypedArrays.
    pub batch_copy_in_bytes: u64,
    /// Bytes written back from mirror TypedArrays into ECS columns.
    pub batch_copy_out_bytes: u64,
    /// Bytes considered dirty after the batch returned. Slice 5.4b:
    /// post-skip writeback bytes \u2014 a column whose script left every
    /// element unchanged contributes 0 here while still contributing
    /// `count * 8` to `batch_copy_out_bytes` (the would-be budget).
    /// Granularity is per-element for Lua `Table` mode and per-column
    /// (whole-axis bulk skip) for Lua `UserData` / `Buffer` modes and
    /// the JS path. The walking-skeleton (slice 4.x) JS run path still
    /// copies unconditionally; its `batch_dirty_bytes` equals
    /// `batch_copy_out_bytes`.
    pub batch_dirty_bytes: u64,
    /// Number of contiguous chunks the batch was split across. Step 4.1:
    /// always 1 per system run.
    pub batch_chunks: u64,
    /// JS function invocations inside the batch path (typically 1 per system).
    pub batch_vm_calls: u64,
    /// Microseconds spent in the copy-in (Rust→TypedArray) phase.
    pub batch_copy_in_us: u64,
    /// Microseconds spent inside the JS `update(ctx, q)` call.
    pub batch_run_us: u64,
    /// Microseconds spent in the copy-out (TypedArray→Rust) phase.
    pub batch_copy_out_us: u64,
    /// Slice 5.4c: per-(entity × write-column) ECS scatter calls
    /// elided this frame because the runtime reported the column
    /// clean. Each elided slot is one `world.get::<&mut T>` lookup
    /// the dispatcher did NOT have to make. A 10k-entity batch with
    /// 3 write columns where the script wrote nothing contributes
    /// 30000 here. Compare against `batch_systems_run *
    /// (writes_per_system * count)` to get the elision rate.
    pub batch_writeback_scatters_skipped: u64,
    /// Slice 5.5a: number of batch script workers in the pool this
    /// frame. Always `1` in 5.5a; 5.5b populates this with the
    /// actual pool size (`min(physical_cores, 4)` by default).
    /// `0` means no batch script pool is initialised this frame.
    pub script_worker_count: u32,
    /// Slice 5.5a: high-water VM memory across the pool this frame,
    /// in bytes. Always `0` in 5.5a (the pool returns 0 from
    /// `vm_memory_bytes`); 5.5c populates this from `Lua::used_memory`
    /// and `rquickjs::Runtime::memory_usage`.
    pub vm_memory_bytes_per_worker: u64,

    // ── Slice 5.5b.1 batch worker channel counters ──
    /// Number of batch script workers alive (and able to accept work)
    /// this frame. Distinct from `script_worker_count` which counts
    /// pool slots regardless of liveness. In 5.5b.1: 0 in `--no-batch-pool`
    /// (Direct) mode, 1 with the JS worker alone, 2 once a Lua script
    /// loads.
    pub batch_workers_active: u32,
    /// Maximum queue depth observed across all worker request channels
    /// this frame. Stays at 0 in 5.5b.1 (synchronous send/recv per
    /// dispatch); 5.5b.2 populates this once chunked dispatch can
    /// enqueue multiple requests before any reply lands.
    pub batch_worker_queue_depth_max: u32,
    /// Wall-clock micros the dispatcher spent inside `pool.run_*_batch`
    /// across all batches this frame, including channel send + recv
    /// for Pooled mode. The (b)−(a) delta on this counter at N=1 is
    /// the *pure* per-batch channel overhead.
    pub batch_worker_dispatch_us: u64,
    /// Bytes copied into the request payload before send (read +
    /// write + optional column f32 data). Charged in both Direct and
    /// Pooled modes so the (a) vs (b) summary tables compare cleanly
    /// on every counter except the channel itself.
    pub batch_payload_clone_bytes: u64,
    /// Bytes the worker shipped back inside [`RunBatchOk::writes`].
    /// Equals the write-column f32 byte count per batch. Same charging
    /// discipline as `batch_payload_clone_bytes`.
    pub batch_response_bytes: u64,

    // ── Slice 5.5c batch worker memory ──
    /// Slice 5.5c — high-water VM allocator footprint (bytes) across
    /// all batch script worker chunks dispatched this frame. Sampled
    /// on the worker thread *after* `run_batch` returned (worker
    /// idle, no in-flight allocations) and max-aggregated across
    /// chunks by the dispatcher. JS source: `rquickjs::Runtime::
    /// memory_usage().malloc_size`. Lua source: `Lua::used_memory()`.
    pub script_worker_memory_bytes_max: u64,
    /// Slice 5.5c — sum of per-chunk VM allocator footprints
    /// dispatched this frame. Same source/sampling as `_max`. Useful
    /// for spotting the case where a single worker dominates vs. all
    /// workers grow uniformly (`max == total / n_workers` ↔ uniform).
    pub script_worker_memory_bytes_total: u64,

    // ── Step 7 behavior graphs (spec §6) ──
    /// Total entities ticked this frame across all behavior graph
    /// instances. Sum of `OnTick` invocations only — event handlers
    /// (`OnSpawn` / `OnCollision` / etc.) are bookkept under
    /// `events_dispatched` instead.
    pub behavior_entities_ticked: u64,
    /// Total `OpInstruction`s executed this frame across all behavior
    /// graph instances (every trigger kind).
    pub behavior_ops_executed: u64,
    /// Structural commands pushed by behavior graph ops this frame
    /// (`SpawnEntity` / `Despawn` / `PlaySound`). Drained at the
    /// Phase 7 command barrier; this counter records *emission*, not
    /// successful application.
    pub behavior_commands_emitted: u64,
    /// Milliseconds the OnTick phase exceeded its configured per-
    /// frame budget by (telemetry only — graphs always run; spec §6
    /// forbids drop for v1).
    pub behavior_overbudget_ms: f32,
    /// Milliseconds the event-dispatch phase (Phase 6) exceeded its
    /// configured budget by. Reported separately because critical
    /// events (collision/trigger) may not be deferred.
    pub behavior_critical_overbudget_ms: f32,
}

/// Tracks per-frame timing for named sections and computes rolling statistics.
pub struct FrameProfiler {
    /// Frame start timestamp in milliseconds.
    frame_start_ms: f64,
    /// Active section start timestamps (section name → start ms).
    section_starts: HashMap<String, f64>,
    /// Accumulated section durations for the current frame (section name → ms).
    section_durations: HashMap<String, f64>,
    /// Counters set externally (draw_calls, sprite_count, etc.).
    counters: HashMap<String, u32>,
    /// Rolling history of frame times (ms) for FPS smoothing and graph display.
    frame_time_history: Vec<f32>,
    /// Index into circular buffer.
    history_index: usize,
    /// Number of frames recorded so far (caps at HISTORY_SIZE).
    history_count: usize,
    /// Most recent computed stats (legacy).
    last_stats: FrameStats,
    /// Most recent nested timings (Step 1 source of truth).
    last_timings: FrameTimings,
    /// Most recent per-frame counters.
    last_counters: FrameCounters,
    /// Per-frame `ScriptTimings` written by `ScriptSystem` before `end_frame`.
    pending_script_timings: ScriptTimings,
    /// Per-frame `FrameCounters` written before `end_frame`.
    pending_counters: FrameCounters,
    /// Optional unbounded history captured during a benchmark window.
    /// `None` outside benchmarks; `Some(Vec)` inside `--bench` mode.
    history: Option<BenchHistory>,
}

/// Per-frame samples retained during a benchmark window. Drained by the
/// runtime to compute avg/p50/p95/p99/max for the JSON record.
#[derive(Debug, Default, Clone)]
pub struct BenchHistory {
    pub timings: Vec<FrameTimings>,
    pub counters: Vec<FrameCounters>,
}

impl FrameProfiler {
    pub fn new() -> Self {
        Self {
            frame_start_ms: 0.0,
            section_starts: HashMap::new(),
            section_durations: HashMap::new(),
            counters: HashMap::new(),
            frame_time_history: vec![0.0; HISTORY_SIZE],
            history_index: 0,
            history_count: 0,
            last_stats: FrameStats::default(),
            last_timings: FrameTimings::default(),
            last_counters: FrameCounters::default(),
            pending_script_timings: ScriptTimings::default(),
            pending_counters: FrameCounters::default(),
            history: None,
        }
    }

    /// Enable benchmark history capture. Pre-allocates `capacity` frames.
    /// Call once before the measurement window begins.
    pub fn enable_history(&mut self, capacity: usize) {
        self.history = Some(BenchHistory {
            timings: Vec::with_capacity(capacity),
            counters: Vec::with_capacity(capacity),
        });
    }

    /// Take the captured benchmark history (resets the buffer to None).
    pub fn take_history(&mut self) -> Option<BenchHistory> {
        self.history.take()
    }

    /// Set per-frame `ScriptTimings`. Called by `ScriptSystem` at the
    /// end of `run_via_bridge`, before the runtime's `end_frame`.
    pub fn set_script_timings(&mut self, t: ScriptTimings) {
        self.pending_script_timings = t;
    }

    /// Set per-frame `FrameCounters`. Called by `ScriptSystem` after it
    /// has aggregated counters from all bridge runners.
    pub fn set_counters(&mut self, c: FrameCounters) {
        self.pending_counters = c;
    }

    pub fn last_timings(&self) -> &FrameTimings {
        &self.last_timings
    }

    pub fn last_counters(&self) -> &FrameCounters {
        &self.last_counters
    }

    /// Call at the start of each frame.
    pub fn begin_frame(&mut self) {
        self.frame_start_ms = now_ms();
        self.section_starts.clear();
        self.section_durations.clear();
        self.counters.clear();
        self.pending_script_timings = ScriptTimings::default();
        self.pending_counters = FrameCounters::default();
    }

    /// Start timing a named section (e.g. "scripts", "render_sprites").
    pub fn begin_section(&mut self, name: &str) {
        self.section_starts.insert(name.to_string(), now_ms());
    }

    /// End timing a named section. Accumulates if called multiple times per frame.
    pub fn end_section(&mut self, name: &str) {
        if let Some(start) = self.section_starts.remove(name) {
            let elapsed = now_ms() - start;
            *self.section_durations.entry(name.to_string()).or_insert(0.0) += elapsed;
        }
    }

    /// Set an integer counter for this frame (e.g. draw_calls, sprite_count).
    pub fn set_counter(&mut self, name: &str, value: u32) {
        self.counters.insert(name.to_string(), value);
    }

    /// Add to an integer counter for this frame.
    pub fn add_counter(&mut self, name: &str, value: u32) {
        *self.counters.entry(name.to_string()).or_insert(0) += value;
    }

    /// Call at the end of each frame to finalize stats.
    pub fn end_frame(&mut self) -> &FrameStats {
        let frame_time_ms = (now_ms() - self.frame_start_ms) as f32;

        // Record in circular buffer
        self.frame_time_history[self.history_index] = frame_time_ms;
        self.history_index = (self.history_index + 1) % HISTORY_SIZE;
        if self.history_count < HISTORY_SIZE {
            self.history_count += 1;
        }

        // Compute smoothed FPS from recent frames
        let smoothing_count = self.history_count.min(FPS_SMOOTHING_FRAMES);
        let avg_frame_time = if smoothing_count > 0 {
            let mut sum = 0.0f32;
            for i in 0..smoothing_count {
                let idx = (self.history_index + HISTORY_SIZE - 1 - i) % HISTORY_SIZE;
                sum += self.frame_time_history[idx];
            }
            sum / smoothing_count as f32
        } else {
            frame_time_ms
        };
        let fps = if avg_frame_time > 0.0 {
            1000.0 / avg_frame_time
        } else {
            0.0
        };

        // Helper to read a section duration
        let section = |name: &str| -> f32 {
            self.section_durations.get(name).copied().unwrap_or(0.0) as f32
        };
        let counter = |name: &str| -> u32 {
            self.counters.get(name).copied().unwrap_or(0)
        };

        // Step 1: section names match the runtime's actual `begin_section`
        // callsites ("scripts", "physics", "render"). The dead
        // "render_sprites" / "render_tilemap" / ... aggregation in the old
        // FrameStats was always 0 — the runtime never emits those names.
        //
        // `script_time_ms` prefers the structured `ScriptTimings` total
        // (set by `ScriptSystem` before this call); falls back to the
        // legacy `"scripts"` section if no structured value was set.
        let scripts = self.pending_script_timings;
        let scripts_total = scripts.total_ms();
        let script_time = if scripts_total > 0.0 {
            scripts_total
        } else {
            section("scripts")
        };
        let physics_time = section("physics");
        let render_time = section("render");
        let swap_time = section("swap");

        // `update_time_ms` kept as scripts + physics for back-compat
        // (editor/window-title path). New code: read `FrameTimings` directly.
        let update_time = script_time + physics_time;

        // Derived `other_ms`: anything not scripts/physics/render/swap.
        // Negative results round to 0 (timing noise across sub-ms ranges).
        let other_ms = (frame_time_ms
            - script_time
            - physics_time
            - render_time
            - swap_time)
            .max(0.0);

        self.last_timings = FrameTimings {
            frame_ms: frame_time_ms,
            scripts,
            physics_ms: physics_time,
            render_ms: render_time,
            swap_ms: swap_time,
            other_ms,
        };
        self.last_counters = self.pending_counters;

        if let Some(h) = self.history.as_mut() {
            h.timings.push(self.last_timings);
            h.counters.push(self.last_counters);
        }

        self.last_stats = FrameStats {
            fps,
            frame_time_ms,
            update_time_ms: update_time,
            render_time_ms: render_time,
            physics_time_ms: physics_time,
            script_time_ms: script_time,
            draw_calls: counter("draw_calls"),
            sprite_count: counter("sprite_count"),
            entity_count: counter("entity_count"),
            particle_count: counter("particle_count"),
            batch_flushes: counter("batch_flushes"),
            // Sampled per frame rather than accumulated: both are plain
            // atomic loads, and the value only means anything as a
            // point-in-time reading.
            memory_bytes: memory::resident_bytes() as u64,
            memory_peak_bytes: memory::peak_bytes() as u64,
            memory_live_bytes: memory::live_bytes() as u64,
        };

        &self.last_stats
    }

    /// Get the most recently computed frame stats.
    pub fn last_stats(&self) -> &FrameStats {
        &self.last_stats
    }

    /// Get the rolling frame time history for graph display.
    /// Returns times in order from oldest to newest.
    pub fn frame_time_history(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.history_count);
        for i in 0..self.history_count {
            let idx = (self.history_index + HISTORY_SIZE - self.history_count + i) % HISTORY_SIZE;
            out.push(self.frame_time_history[idx]);
        }
        out
    }
}

impl Default for FrameProfiler {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Platform-specific high-resolution timing
// ---------------------------------------------------------------------------

/// Milliseconds on a monotonic, high-resolution clock with an arbitrary
/// start, on every platform (`Instant` natively, `performance.now()` in a
/// browser, where `Instant` panics). For measuring between two moments in
/// one process - a package stamping an input against its own frame step,
/// say - not for dates.
#[cfg(not(target_arch = "wasm32"))]
pub fn now_ms() -> f64 {
    use std::time::Instant;

    thread_local! {
        static EPOCH: Instant = Instant::now();
    }

    EPOCH.with(|epoch| epoch.elapsed().as_secs_f64() * 1000.0)
}

/// Seconds since the Unix epoch, as a float.
///
/// The *wall clock*, which is a different thing from [`now_ms`] and the one
/// a game needs when it has to survive being closed: how long was I away,
/// when did this run start, is the daily reward due. [`now_ms`] is
/// monotonic with an arbitrary start, so two of its readings across a
/// restart mean nothing at all.
///
/// It can go backwards - the player owns the clock, and a save holding a
/// time in the future is a thing that happens. Anything computing elapsed
/// time from it should clamp at zero rather than trusting the difference.
#[cfg(not(target_arch = "wasm32"))]
pub fn unix_time() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Seconds since the Unix epoch, from the browser's own clock.
#[cfg(target_arch = "wasm32")]
pub fn unix_time() -> f64 {
    js_sys::Date::now() / 1000.0
}

/// Get current time in milliseconds (high-resolution) — WASM via Performance.now().
#[cfg(target_arch = "wasm32")]
pub fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}
