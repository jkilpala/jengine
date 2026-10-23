//! Per-frame runner that drives a [`ScriptAdapter`] for opted-in entities.
//!
//! Phase 2 shadow-mode runner. It owns:
//! - one boxed adapter,
//! - a load cache keyed by source path,
//! - a per-frame command queue translated to legacy [`crate::scripting::api::ScriptCommand`]s
//!   so the engine apply path is unchanged.
//!
//! The runner does **not** snapshot the world or apply commands. The caller (Phase 2:
//! [`crate::ecs::systems::ScriptSystem`]) supplies a snapshot and forwards translated
//! commands through the existing apply pipeline.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::adapter::{ScriptAdapter, ScriptHandle};
use super::capability::Capabilities;
use super::command::BridgeCommand;
use super::context::{BridgeContext, BridgeReadSnapshot};
use super::diagnostics::ScriptError;
use super::metadata::EngineEvent;

/// Step 1 instrumentation: per-runner counters drained by the host once
/// per frame to populate `FrameCounters`. Each field counts crossings
/// from Rust→VM (`*_calls`) or VM→Rust (`commands_emitted`). All bumps
/// are non-atomic single-threaded `u64` adds (sub-ns), always-on.
#[derive(Debug, Default, Clone, Copy)]
pub struct RunnerStats {
    pub init_calls: u64,
    pub update_calls: u64,
    pub named_calls: u64,
    pub event_calls: u64,
    pub commands_emitted: u64,
    /// Step 3.3 — number of dispatches the budget gate refused this
    /// frame (Drop policy) or deferred to a later frame (Defer
    /// policy). Mirrored into `FrameCounters.events_dropped_or_deferred`
    /// by the ScriptSystem.
    pub events_dropped_or_deferred: u64,
}

/// Step 3.3 — what to do when a per-frame budget is exceeded.
///
/// `Warn` is the default — it logs once-per-frame and runs the
/// invocation anyway, matching pre-budget behaviour. `Drop` skips
/// the call entirely (preferred for shipping games where dropping
/// frames is worse than dropping the script callback). `Defer` queues
/// the call onto an internal pending list capped at
/// [`BudgetConfig::deferred_cap`]; pending entries are drained at the
/// start of the next frame's dispatches before any new calls run.
/// Defer entries that don't fit in `deferred_cap` are counted as
/// `events_dropped_or_deferred` and dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum BudgetPolicy {
    #[default]
    Warn,
    Drop,
    Defer,
}


/// Step 3.3 — per-frame execution budget for script dispatches.
///
/// A zero value on `max_script_ms_per_frame` or `max_invocations_per_frame`
/// disables that cap ("unlimited"); the default is unlimited + Warn,
/// which preserves pre-Step 3 behaviour when no caller opts in. Both
/// caps apply to event-class dispatches: `dispatch_event`,
/// `dispatch_named`, and the lifecycle hooks (`on_spawn`,
/// `on_despawn`). `update()` is *not* charged — throttle that with
/// per-script `tick_rate` (Step 3a) instead.
#[derive(Debug, Clone, Copy)]
pub struct BudgetConfig {
    pub max_script_ms_per_frame: f32,
    pub max_invocations_per_frame: u32,
    pub overflow_policy: BudgetPolicy,
    pub deferred_cap: u32,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            max_script_ms_per_frame: 0.0,
            max_invocations_per_frame: 0,
            overflow_policy: BudgetPolicy::Warn,
            deferred_cap: 0,
        }
    }
}

/// Per-frame work item: which entity to drive with which loaded script.
///
/// Step 3a — `tick_interval_frames` is the number of host frames between
/// `update()` calls for this entity (1 = every frame, 6 = ~10 Hz at the
/// 60 Hz engine base). `tick_phase` is a deterministic offset in
/// `[0, tick_interval_frames)` chosen at load time so a population of
/// entities at the same tick rate spreads its work evenly across the
/// interval instead of synchronising on one frame.
#[derive(Debug, Clone)]
struct Loaded {
    handle: ScriptHandle,
    initialized: bool,
    tick_interval_frames: u8,
    tick_phase: u8,
    /// The frame `update` last ran on, so a throttled script is handed the
    /// time since then - which is not always one interval: an input frame
    /// runs it early (see `run_frame`).
    last_update_frame: Option<u64>,
    /// Step 3.1 — lifecycle events this entity's script declared via
    /// the `events = [...]` header. Stored per-entity (not per-path)
    /// because the same script source can be attached to entities with
    /// different runtime requirements in the future. Empty `Vec` is
    /// the common case: most scripts don't subscribe to any lifecycle
    /// events. Keeping it as `Vec` (not `SmallVec`) avoids a new
    /// workspace dep — measured cost is one heap alloc per loaded
    /// entity, paid once at load time, never on the hot path.
    subscribed_events: Vec<EngineEvent>,
}

/// A bridge-mode runner around a single adapter.
pub struct ScriptBridgeRunner {
    adapter: Box<dyn ScriptAdapter>,
    /// `source_path` → handle (script reused across entities).
    by_path: HashMap<String, ScriptHandle>,
    /// `entity_id` → load state.
    by_entity: HashMap<u64, Loaded>,
    /// Paths that previously failed to load — skipped on subsequent frames.
    failed_paths: HashSet<String>,
    /// Step 3 — set of script paths we've already warned about for
    /// missing tick-rate metadata. Each legacy script (no
    /// `script = { tick_rate / tickHz }` header) gets exactly one
    /// warning the first time it's loaded, so the editor / runtime log
    /// stays readable even when the same legacy script is attached to
    /// thousands of entities.
    warned_legacy_paths: HashSet<String>,
    /// Commands queued during the current `run_frame` call.
    commands: Vec<BridgeCommand>,
    /// Capability set advertised by the host this run.
    capabilities: Capabilities,
    /// Persistent counters used to construct each `BridgeContext`. Hoisted
    /// onto the runner so we don't pay 5 `Rc<Cell<u64>>` heap allocations
    /// per entity per frame (the dominant cost in `BridgeContext::new` at
    /// thousands of scripted entities). Counter state is monotonic across
    /// frames, which matches the engine's existing semantics — adapter
    /// callbacks already see the same counter Rc swapped into their state
    /// via `with_snapshot`.
    spawn_counter: Rc<Cell<u64>>,
    sound_counter: Rc<Cell<u64>>,
    tween_counter: Rc<Cell<u64>>,
    event_listener_counter: Rc<Cell<u64>>,
    timer_counter: Rc<Cell<u64>>,
    /// Step 1 instrumentation — see [`RunnerStats`]. Drained per frame
    /// by `ScriptSystem` via [`Self::take_stats`].
    stats: RunnerStats,
    /// Step 3a — monotonically incrementing frame counter, bumped at
    /// the top of every [`Self::run_frame`] call. Used together with
    /// `Loaded::tick_interval_frames` and `Loaded::tick_phase` to
    /// decide which scripts are due to `update()` this frame.
    frame_index: u64,
    /// The last [`DT_HISTORY`] frames' `dt`, a ring indexed by
    /// `frame_index`: a script that ticks every `n` frames is handed the
    /// sum of the last `n`, the time since it last ran.
    recent_dt: [f32; DT_HISTORY],
    /// Step 3a — default tick rate (Hz) applied to scripts that don't
    /// declare their own `tick_rate` / `tickHz` header. Defaults to
    /// `tick_base_hz` (i.e. "every frame"). The bench harness can
    /// lower this via [`Self::set_default_tick_hz`] to measure the
    /// `scripts_ms` impact of dropping legacy scripts to 10 Hz without
    /// editing every source file.
    tick_default_hz: u32,
    /// Step 3a — nominal engine tick rate (Hz). 60 today; left as a
    /// field so a future fixed-timestep mode can override it without
    /// chasing magic numbers through the file.
    tick_base_hz: u32,
    /// Step 3.1 — hot-dispatch subscription index. For each
    /// well-known [`EngineEvent`], holds the entity ids whose loaded
    /// scripts declared that event in their `events = [...]` header.
    /// Built incrementally at load time (one push per subscription),
    /// torn down per-entity in [`Self::notify_despawn`] /
    /// [`Self::reset`]. Lookup is O(1) on the kind, then iterate the
    /// subscriber list — dispatch is `O(subscribers)` per event, not
    /// `O(all_loaded)`. The plan calls for `(entity_id, event_kind)
    /// → SmallVec<ScriptInstanceId>` for *targeted* events; today
    /// scripts are 1:1 with entities (one ScriptComponent per entity)
    /// so the entity_id IS the instance id, and the targeted variant
    /// reduces to "is `eid` in `subscriptions[kind]`". When per-entity
    /// multi-script support lands, the value type becomes `Vec<(eid,
    /// instance_id)>` without changing the consumer call sites.
    subscriptions: HashMap<EngineEvent, Vec<u64>>,
    /// Step 3.3 — per-frame execution budget. Defaults to
    /// unlimited + Warn (pre-Step 3 behaviour). Hosts opt in via
    /// [`Self::set_budget`]. Checked before each event-class
    /// dispatch in [`Self::charge_budget`].
    budget: BudgetConfig,
    /// Step 3.3 — number of event-class dispatches charged this
    /// frame. Reset by [`Self::start_frame`] before any work runs.
    /// `update()` doesn't count — use tick_rate to throttle that.
    frame_invocations: u32,
    /// Step 3.3 — cumulative ms spent inside event-class dispatches
    /// this frame, charged via wall-clock `Instant::now()` deltas
    /// around each call. Reset by [`Self::start_frame`].
    frame_elapsed_ms: f32,
    /// Step 3.3 — once-per-frame latch so a Warn-policy budget
    /// overflow logs at most once per frame (per runner). Reset by
    /// [`Self::start_frame`].
    frame_warned: bool,
    /// Step 3.3 — deferred dispatches that overflowed budget under
    /// the Defer policy. Drained at the *next* frame's
    /// [`Self::start_frame`] before any new calls run. Bounded by
    /// `budget.deferred_cap`; entries that don't fit are dropped and
    /// counted in `stats.events_dropped_or_deferred`.
    deferred: Vec<DeferredCall>,
}

/// Step 3.3 — a queued event-class dispatch awaiting budget headroom.
#[derive(Debug, Clone)]
struct DeferredCall {
    entity_id: u64,
    handler: String,
    /// `true` when this came from `dispatch_named`, `false` for
    /// `dispatch_event`. We honour the original entry path on replay
    /// so stats counters (`named_calls` vs `event_calls`) stay
    /// truthful.
    is_named: bool,
}

impl ScriptBridgeRunner {
    pub fn new(adapter: Box<dyn ScriptAdapter>, capabilities: Capabilities) -> Self {
        Self {
            adapter,
            by_path: HashMap::new(),
            by_entity: HashMap::new(),
            failed_paths: HashSet::new(),
            warned_legacy_paths: HashSet::new(),
            commands: Vec::new(),
            capabilities,
            spawn_counter: Rc::new(Cell::new(1_000_000)),
            sound_counter: Rc::new(Cell::new(2_000_000)),
            tween_counter: Rc::new(Cell::new(3_000_000)),
            event_listener_counter: Rc::new(Cell::new(4_000_000)),
            timer_counter: Rc::new(Cell::new(5_000_000)),
            stats: RunnerStats::default(),
            frame_index: 0,
            recent_dt: [0.0; DT_HISTORY],
            tick_default_hz: 60,
            tick_base_hz: 60,
            subscriptions: HashMap::new(),
            budget: BudgetConfig::default(),
            frame_invocations: 0,
            frame_elapsed_ms: 0.0,
            frame_warned: false,
            deferred: Vec::new(),
        }
    }

    /// Step 3.3 — install a per-frame execution budget. Pass a
    /// `BudgetConfig` with both caps zero to revert to unlimited.
    /// Changing the policy at runtime is safe; in-flight overflow
    /// state (`frame_warned`, deferred queue) is left untouched until
    /// the next [`Self::start_frame`].
    pub fn set_budget(&mut self, cfg: BudgetConfig) {
        self.budget = cfg;
    }

    /// Slice A — install every dynamic function from a
    /// [`crate::package::script_api::ScriptApiRegistry`] into the underlying
    /// adapter. Functions without a `handler` are skipped silently by the
    /// adapter (they remain metadata-only). Safe to call multiple times if
    /// new packages are loaded after construction.
    pub fn install_script_api(
        &mut self,
        api: &crate::package::script_api::ScriptApiRegistry,
    ) {
        for desc in api.iter() {
            if desc.handler.is_some() {
                self.adapter.install_dynamic_function(&desc.name, desc);
            }
        }
    }

    /// Install one package-contributed script global.
    ///
    /// The global queues a `PackageCall`; nothing runs until the command is
    /// applied, which is what makes the same handler usable by hosts that
    /// have no bridge snapshot.
    pub fn install_package_command(&mut self, name: &str, tag: &str, arg_names: &[String]) {
        self.adapter.install_package_command(name, tag, arg_names);
    }

    /// Install namespace tables over globals already installed - the
    /// packages', after their commands. Core's are installed with the VM.
    pub fn install_namespaces(&mut self, entries: &[crate::scripting::namespaces::NamespacedName]) {
        self.adapter.install_namespaces(entries);
    }

    /// Step 3.3 — caller-visible view of the active budget. Useful
    /// for tests + for the editor's runtime inspector.
    pub fn budget(&self) -> BudgetConfig {
        self.budget
    }

    /// Step 3.3 — reset the per-frame budget counters and replay
    /// any deferred dispatches from the previous frame. Returns the
    /// list of `(entity_id, handler, is_named)` triples the caller
    /// should re-dispatch (under the *current* frame's budget) by
    /// invoking [`Self::dispatch_event`] / [`Self::dispatch_named`]
    /// in order. Pulling this out into a returned list — rather than
    /// re-entering dispatch from inside the runner — keeps the
    /// snapshot/dt arguments under the caller's control (the deferred
    /// call from frame N runs against frame N+1's snapshot).
    pub fn start_frame(&mut self) -> Vec<(u64, String, bool)> {
        self.frame_invocations = 0;
        self.frame_elapsed_ms = 0.0;
        self.frame_warned = false;
        std::mem::take(&mut self.deferred)
            .into_iter()
            .map(|d| (d.entity_id, d.handler, d.is_named))
            .collect()
    }

    /// Step 3.3 — budget gate. Returns `true` if the dispatch should
    /// proceed, `false` if the caller must skip it. Side effects per
    /// policy:
    ///
    /// * `Warn`  — logs at most once/frame, returns `true`.
    /// * `Drop`  — bumps `events_dropped_or_deferred`, returns `false`.
    /// * `Defer` — pushes onto deferred queue if room, otherwise
    ///   counts as dropped; returns `false` either way.
    ///
    /// On successful proceed (`true`), increments `frame_invocations`.
    /// Time charged separately via [`Self::charge_elapsed`] after the
    /// call completes. `entity_id` / `handler` / `is_named` are only
    /// consulted on the Defer path.
    fn charge_budget(
        &mut self,
        entity_id: u64,
        handler: &str,
        is_named: bool,
    ) -> bool {
        let inv_capped =
            self.budget.max_invocations_per_frame > 0
                && self.frame_invocations >= self.budget.max_invocations_per_frame;
        let ms_capped = self.budget.max_script_ms_per_frame > 0.0
            && self.frame_elapsed_ms >= self.budget.max_script_ms_per_frame;
        if !inv_capped && !ms_capped {
            self.frame_invocations = self.frame_invocations.saturating_add(1);
            return true;
        }
        match self.budget.overflow_policy {
            BudgetPolicy::Warn => {
                if !self.frame_warned {
                    log::warn!(
                        "Script budget exceeded (invocations={}, elapsed_ms={:.2}); cap inv={} ms={:.1}; policy=Warn (running anyway)",
                        self.frame_invocations,
                        self.frame_elapsed_ms,
                        self.budget.max_invocations_per_frame,
                        self.budget.max_script_ms_per_frame,
                    );
                    self.frame_warned = true;
                }
                self.frame_invocations = self.frame_invocations.saturating_add(1);
                true
            }
            BudgetPolicy::Drop => {
                self.stats.events_dropped_or_deferred += 1;
                false
            }
            BudgetPolicy::Defer => {
                if (self.deferred.len() as u32) < self.budget.deferred_cap {
                    self.deferred.push(DeferredCall {
                        entity_id,
                        handler: handler.to_string(),
                        is_named,
                    });
                }
                // Either way (queued or queue-full), the original
                // dispatch is suppressed this frame and counts as
                // dropped-or-deferred.
                self.stats.events_dropped_or_deferred += 1;
                false
            }
        }
    }

    /// Step 3.3 — record wall-clock ms spent inside a dispatch so
    /// subsequent [`Self::charge_budget`] calls see the cumulative
    /// load. Always called, even when budget is disabled (cost is one
    /// `f32` add); harmless overhead.
    fn charge_elapsed(&mut self, ms: f32) {
        self.frame_elapsed_ms += ms;
    }

    /// Step 3.1 — number of entities currently subscribed to the
    /// given engine event. Used by tests and by editor lints (Slice
    /// 3.5) that warn about pathological subscription counts.
    pub fn subscriber_count(&self, kind: EngineEvent) -> usize {
        self.subscriptions.get(&kind).map(|v| v.len()).unwrap_or(0)
    }

    /// Step 3.1 — true if `entity_id` is currently subscribed to
    /// `kind`. O(subscribers); used by the runtime to decide whether
    /// to bother building event payloads for a specific event class.
    pub fn is_subscribed(&self, entity_id: u64, kind: EngineEvent) -> bool {
        self.subscriptions
            .get(&kind)
            .is_some_and(|v| v.contains(&entity_id))
    }

    /// Step 3.1 — snapshot of every entity_id currently loaded in this
    /// runner. The caller (ScriptSystem) uses this to compute the
    /// despawn diff each frame: any id present here but absent from
    /// the world's live entities needs `notify_despawn` to fire its
    /// lifecycle hook (if subscribed) and clean up.
    /// Returns a fresh `Vec` so the borrow on `self` is released
    /// immediately — the typical caller iterates and then mutably
    /// re-enters `notify_despawn`.
    pub fn loaded_entity_ids(&self) -> Vec<u64> {
        self.by_entity.keys().copied().collect()
    }

    /// The loaded entities whose script defines `fn_name`, in id order -
    /// what `fixed_update` is dispatched to.
    pub fn entities_defining(&self, fn_name: &str) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .by_entity
            .iter()
            .filter(|(_, l)| self.adapter.has_function(l.handle, fn_name))
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Step 3a — override the default tick rate applied to scripts that
    /// do not declare their own `tick_rate` / `tickHz` header. Clamped
    /// to `[1, tick_base_hz]` because faster-than-base or zero values
    /// don't make sense. Affects only entities that haven't been loaded
    /// yet — already-loaded scripts keep the interval recorded at load
    /// time.
    pub fn set_default_tick_hz(&mut self, hz: u32) {
        self.tick_default_hz = hz.clamp(1, self.tick_base_hz);
    }

    /// Step 3a — read back the engine's nominal tick rate. Tests use
    /// this to construct expected interval values without duplicating
    /// the constant.
    pub fn tick_base_hz(&self) -> u32 {
        self.tick_base_hz
    }

    /// Step 3a — the frame index that the *next* call to [`Self::run_frame`]
    /// will use when evaluating its schedule. Callers compute their
    /// own tick-skip lists ahead of time (so that the cull-distance
    /// skip set and the tick-schedule skip set can be merged into one
    /// sorted vector before `run_frame` is called) and need this value
    /// to do so.
    pub fn next_frame_index(&self) -> u64 {
        self.frame_index
    }

    /// Step 3a — append every entity id whose `update()` should be
    /// SKIPPED this frame because its tick interval doesn't align with
    /// the runner's current `frame_index`. Output is appended in
    /// `by_entity` iteration order (i.e. unsorted); the caller is
    /// responsible for any sort/dedup it needs (the existing
    /// cull-distance skip path already sorts before passing to
    /// `run_frame`, so callers typically push tick skips into the same
    /// Vec then sort once).
    ///
    /// Entities running at the engine base rate (`tick_interval_frames
    /// == 1`) are never appended — they always tick.
    pub fn append_tick_skips(&self, frame_index: u64, out: &mut Vec<u64>) {
        for (eid, loaded) in &self.by_entity {
            if loaded.tick_interval_frames <= 1 {
                continue;
            }
            let interval = loaded.tick_interval_frames as u64;
            let phase = loaded.tick_phase as u64;
            if frame_index % interval != phase {
                out.push(*eid);
            }
        }
    }

    /// Replace the host capability set (called by the host when capabilities change).
    pub fn set_capabilities(&mut self, caps: Capabilities) {
        self.capabilities = caps;
    }

    /// Phase D — Forward a `require` resolver into the underlying adapter.
    /// Adapters without `require` support (Noop) silently ignore.
    pub fn set_module_resolver(&mut self, resolver: super::module::ModuleResolver) {
        self.adapter.set_module_resolver(resolver);
    }

    /// Data-oriented design — forward the shared `.data` table registry into
    /// the underlying adapter (read by the `get_data` script global).
    pub fn set_data_registry(&mut self, registry: std::rc::Rc<crate::data::DataRegistry>) {
        self.adapter.set_data_registry(registry);
    }

    /// Phase D — Drop the cached `require()` exports on the underlying
    /// adapter. Call when an asset-watcher reports a `.lua` / `.luau` /
    /// `.js` file change so the next `require(name)` re-runs the module
    /// body via the resolver. Errors are logged inside the adapter and
    /// swallowed so callers stay infallible.
    pub fn invalidate_module_cache(&mut self) {
        self.adapter.invalidate_module_cache();
    }

    /// Returns true if the given source path has already been loaded (or
    /// failed to load) and therefore does NOT need its source text supplied
    /// again. Hot-path callers use this to avoid reading the script file
    /// from disk every frame for every entity that shares a script.
    pub fn is_path_loaded(&self, path: &str) -> bool {
        self.by_path.contains_key(path) || self.failed_paths.contains(path)
    }

    /// P1.5 — hot-reload entry point. Forget every cached load for `path`
    /// (the `by_path → handle` mapping, the failed-paths denylist, and
    /// every `by_entity` entry whose handle points at it), then unload
    /// the underlying handle from the adapter. After this call the next
    /// `run_frame` will treat each affected entity as freshly attached:
    /// it will request source text again, the adapter will compile a
    /// fresh handle, and `on_ready` / `init` will run on the new code.
    ///
    /// Returns `true` if anything was unloaded (path was known) so
    /// callers can log the hot-reload effect; `false` if the path
    /// wasn't loaded (e.g. file changed before any entity used it —
    /// nothing to do, the next attach will pick up the new contents
    /// from disk anyway).
    pub fn unload_path(&mut self, path: &str) -> bool {
        let was_failed = self.failed_paths.remove(path);
        let Some(handle) = self.by_path.remove(path) else {
            return was_failed;
        };
        // Drop every per-entity load tied to this handle so the next
        // `run_frame` re-initialises it from scratch (re-reading source).
        self.by_entity.retain(|_, loaded| loaded.handle != handle);
        // Best-effort unload — adapter errors only mean the underlying
        // backend already let it go; the hot-reload path stays infallible.
        if let Err(e) = self.adapter.unload(handle) {
            log::warn!(
                "[ScriptBridgeRunner] unload({path}) on hot-reload failed: {e:?}"
            );
        }
        true
    }

    /// Drain Step 1 per-frame counters; called by `ScriptSystem` after
    /// `run_frame` + `dispatch_named` + `dispatch_event` have all run.
    pub fn take_stats(&mut self) -> RunnerStats {
        std::mem::take(&mut self.stats)
    }

    /// Drive every supplied entity for one frame.
    ///
    /// `loads` is the list of `(entity_id, source_path, source_text)` for entities that
    /// opted into the bridge this frame. Sources are loaded lazily on first sight.
    /// Returns the bridge command queue — caller is responsible for translating and
    /// applying them.
    ///
    /// `update_skip` is a SORTED-ASCENDING slice of entity IDs whose `update()`
    /// should NOT be called this frame (used for cold-entity culling: scripts
    /// far from the active camera). `init()` and `load` still run for these
    /// entities — only the per-frame `update` is gated. Pass `&[]` to disable
    /// culling. The slice MUST be sorted; the runner uses `binary_search`.
    pub fn run_frame<P: AsRef<str>>(
        &mut self,
        loads: &[(u64, P, String)],
        snapshot: &BridgeReadSnapshot,
        dt: f32,
        update_skip: &[u64],
    ) -> (Vec<BridgeCommand>, Vec<ScriptError>) {
        self.commands.clear();
        let mut errors: Vec<ScriptError> = Vec::new();
        // Step 3a — bump the runner-local frame counter once per call.
        // The schedule check below uses `(frame_index % interval) ==
        // phase` to decide which entities are due to update; bumping
        // here (before the loop) means the very first frame after
        // construction is `frame_index = 0`, which any entity with
        // `tick_phase = 0` will treat as due. Entities loaded mid-frame
        // therefore tick on their next aligned frame, not immediately.
        let frame_index = self.frame_index;
        self.frame_index = self.frame_index.wrapping_add(1);
        self.recent_dt[(frame_index % DT_HISTORY as u64) as usize] = dt;

        // Build a single per-frame ctx that every entity update reuses. The
        // counter Rcs and the &mut commands sink are constant for the whole
        // frame — building a fresh ctx per entity (5 `Rc::clone`s each) was
        // the dominant constant-overhead per scripted entity. Adapters that
        // implement `frame_begin`/`frame_end` (Luau, JS host) install their
        // shared state once via `frame_begin` so per-entity calls also skip
        // 6 borrow-mut field assignments each.
        let mut ctx = BridgeContext::with_full_counters(
            dt,
            snapshot,
            self.capabilities.clone(),
            &mut self.commands,
            Rc::clone(&self.spawn_counter),
            Rc::clone(&self.sound_counter),
            Rc::clone(&self.tween_counter),
            Rc::clone(&self.event_listener_counter),
            Rc::clone(&self.timer_counter),
        );
        self.adapter.frame_begin(&mut ctx);

        // Two passes: lazy load + init() per uninitialized entity, then
        // batched update() across all entities. The split lets adapters with
        // expensive per-call setup (JS host pays one `Context::with` per
        // call) amortise that cost via `update_batch`. Lua's default impl
        // just loops, so it's a no-op there.
        let mut update_entries: Vec<(ScriptHandle, u64)> = Vec::with_capacity(loads.len());
        // How many frames each entry's `update` covers, for the `dt` it is
        // handed: the frames since it last ran.
        let mut update_intervals: Vec<u16> = Vec::with_capacity(loads.len());
        // A frame on which input changed runs every throttled script too.
        // A press is "just pressed" for exactly one frame, so a script
        // ticking every sixth frame saw about one press in six: Test Lab
        // scene 02 (`tick_rate = 10`) ignored Esc and Page Up until one
        // happened to land on its frame - natively only, since the browser
        // hosts tick every script every frame. Running on input frames
        // shows each edge once, on the frame it happened, as they do.
        let input_frame = has_input_edges(snapshot);
        for (entity_id, path, source) in loads {
            // Borrowed: the script system hands the world's own path strings
            // in, so an already-loaded entity costs no allocation here.
            let path = path.as_ref();
            // Lazy load + cache by path.
            if !self.by_entity.contains_key(entity_id) {
                if self.failed_paths.contains(path) {
                    continue;
                }
                let handle = if let Some(h) = self.by_path.get(path) {
                    *h
                } else {
                    match self.adapter.load(path, source) {
                        Ok(h) => {
                            self.by_path.insert(path.to_string(), h);
                            h
                        }
                        Err(e) => {
                            self.failed_paths.insert(path.to_string());
                            errors.push(e);
                            continue;
                        }
                    }
                };
                let interval = tick_interval_for(
                    source,
                    *entity_id,
                    self.tick_default_hz,
                    self.tick_base_hz,
                );
                let phase = tick_phase_for(*entity_id, source, interval);
                // Step 3 — legacy compatibility warning. Scripts that
                // don't declare a tick-rate header keep running at the
                // engine base rate (`tick_default_hz`, normally 60 Hz)
                // so behaviour doesn't silently change underneath
                // existing projects — but we emit a one-time warning
                // per script path so authors know to add
                // `script = { tick_rate = N }` (Lua) or
                // `script = { tickHz: N }` (JS).
                if super::metadata::parse_tick_rate(source).is_none()
                    && self.warned_legacy_paths.insert(path.to_string())
                {
                    log::warn!(
                        "[Script] {} has no tick-rate metadata; running at {} Hz \
                         (legacy default). Add `script = {{ tick_rate = N }}` (Lua) \
                         or `script = {{ tickHz: N }}` (JS) at the top of the file \
                         to opt into a lower rate.",
                        path,
                        self.tick_default_hz,
                    );
                }
                self.by_entity.insert(
                    *entity_id,
                    Loaded {
                        handle,
                        initialized: false,
                        tick_interval_frames: interval,
                        tick_phase: phase,
                        last_update_frame: None,
                        subscribed_events: super::metadata::parse_subscribed_events(source),
                    },
                );
                // Step 3.1 — register this entity in the subscription
                // index for every event it declared. Done here (after
                // by_entity.insert) so the index is populated before
                // the first dispatch could possibly happen, and before
                // any potential init() error path — we clean up on
                // notify_despawn / reset, never on init failure (init
                // errors don't unload the script today, see the
                // existing behaviour where failed_paths is keyed on
                // load errors only).
                if let Some(loaded) = self.by_entity.get(entity_id) {
                    for kind in &loaded.subscribed_events {
                        self.subscriptions
                            .entry(*kind)
                            .or_default()
                            .push(*entity_id);
                    }
                }
            }

            let loaded = match self.by_entity.get(entity_id).cloned() {
                Some(l) => l,
                None => continue,
            };

            // init() once per entity.
            if !loaded.initialized {
                self.stats.init_calls += 1;
                if let Err(e) = self.adapter.init(loaded.handle, *entity_id, &mut ctx) {
                    errors.push(e);
                    continue;
                }
                if let Some(slot) = self.by_entity.get_mut(entity_id) {
                    slot.initialized = true;
                }
                // Step 3.1 — fire `on_spawn` exactly once, right after
                // init() succeeds, for entities subscribed to the
                // Spawn event. We dispatch through the same per-frame
                // ctx so the on_spawn handler shares the snapshot,
                // command sink, and counters with the surrounding
                // frame; commands it emits land in the same
                // `self.commands` Vec the run_frame caller consumes.
                // The subscription check is O(subscribers) on a
                // typically-small list; for entities with no Spawn
                // subscription it's a single HashMap lookup that
                // misses, then early-out.
                if loaded
                    .subscribed_events.contains(&EngineEvent::Spawn)
                {
                    self.stats.event_calls += 1;
                    if let Err(e) = self.adapter.event(
                        loaded.handle,
                        *entity_id,
                        EngineEvent::Spawn.handler_name(),
                        &mut ctx,
                    ) {
                        errors.push(e);
                    }
                }
            }

            // Step 3a — schedule gate. Skip the per-entity `update()` push
            // when this entity's tick interval doesn't align with the
            // current frame. `init()` above always runs (newly-loaded
            // scripts must initialise before they are first eligible to
            // tick), and `update_skip` from the caller still applies
            // afterwards as a separate culling pass. Entities at the base
            // rate (`tick_interval_frames == 1`) skip the modulo entirely
            // for the hot path.
            if loaded.tick_interval_frames > 1 && !input_frame {
                let interval = loaded.tick_interval_frames as u64;
                let phase = loaded.tick_phase as u64;
                if frame_index % interval != phase {
                    continue;
                }
            }

            // An every-frame script covers one frame, as it always has - even
            // after `update_skip` culled it for a while.
            let covers = match loaded.last_update_frame {
                _ if loaded.tick_interval_frames <= 1 => 1,
                Some(last) => frame_index.saturating_sub(last),
                None => loaded.tick_interval_frames as u64,
            };
            update_entries.push((loaded.handle, *entity_id));
            update_intervals.push(covers.clamp(1, DT_HISTORY as u64) as u16);
        }

        // Cold-entity culling: drop any update_entries whose id is in the
        // sorted `update_skip` slice. Done as a single retain() so we keep
        // O(N log K) total cost where K = culled count, vs O(N) HashSet
        // build. For the common case (`update_skip.is_empty()`) this is a
        // no-op short-circuit.
        if !update_skip.is_empty() {
            let mut i = 0;
            update_intervals.retain(|_| {
                let keep = update_skip.binary_search(&update_entries[i].1).is_err();
                i += 1;
                keep
            });
            update_entries.retain(|(_, eid)| update_skip.binary_search(eid).is_err());
        }
        for (_, entity_id) in &update_entries {
            if let Some(loaded) = self.by_entity.get_mut(entity_id) {
                loaded.last_update_frame = Some(frame_index);
            }
        }

        // Batched update pass — adapters override `update_batch` to share
        // per-call setup (e.g. one `Context::with` enter for the whole frame).
        self.stats.update_calls += update_entries.len() as u64;
        if update_intervals.iter().all(|&n| n <= 1) {
            self.adapter.update_batch(&update_entries, &mut ctx, &mut errors);
        } else {
            // A script that ticks every `n` frames is handed the time since
            // it last ran - the last `n` frames' dt - not this frame's alone.
            // It used to get one frame's, so `tick_rate = 30` on a 165 Hz
            // display ran a script's clock at a third of the wall's
            // (`t = t + dt`), natively only: the browser hosts tick every
            // frame. One batch per interval, the rate-free ones first.
            let mut intervals: Vec<u16> = update_intervals.clone();
            intervals.sort_unstable();
            intervals.dedup();
            for n in intervals {
                let group: Vec<(ScriptHandle, u64)> = update_entries
                    .iter()
                    .zip(&update_intervals)
                    .filter(|(_, k)| **k == n)
                    .map(|(e, _)| *e)
                    .collect();
                ctx.dt = window_dt(&self.recent_dt, frame_index, n);
                self.adapter.update_batch(&group, &mut ctx, &mut errors);
            }
            ctx.dt = dt;
        }

        self.adapter.frame_end(&mut ctx);
        drop(ctx);
        let cmds = std::mem::take(&mut self.commands);
        self.stats.commands_emitted += cmds.len() as u64;
        (cmds, errors)
    }

    /// P7.5c — Dispatch a named callback (timer / GUI click) to a previously
    /// loaded entity. Returns `(commands, error)` — `error` is `Some` only
    /// when the adapter raised one. Returns `(empty, None)` if the entity is
    /// unknown to this runner (i.e. it lives in the other-language runner).
    pub fn dispatch_named(
        &mut self,
        entity_id: u64,
        fn_name: &str,
        snapshot: &BridgeReadSnapshot,
        dt: f32,
    ) -> (Vec<BridgeCommand>, Option<ScriptError>) {
        let Some(handle) = self.by_entity.get(&entity_id).map(|l| l.handle) else {
            return (Vec::new(), None);
        };
        // Step 3.3 — budget gate. If Drop or Defer policy refuses
        // this dispatch, return early with no commands and no error;
        // the call has been counted in `events_dropped_or_deferred`
        // already (and queued for next frame under Defer).
        if !self.charge_budget(entity_id, fn_name, true) {
            return (Vec::new(), None);
        }
        let t0 = std::time::Instant::now();
        self.commands.clear();
        let mut ctx = BridgeContext::with_full_counters(
            dt,
            snapshot,
            self.capabilities.clone(),
            &mut self.commands,
            Rc::clone(&self.spawn_counter),
            Rc::clone(&self.sound_counter),
            Rc::clone(&self.tween_counter),
            Rc::clone(&self.event_listener_counter),
            Rc::clone(&self.timer_counter),
        );
        let err = self
            .adapter
            .named_callback(handle, entity_id, fn_name, &mut ctx)
            .err();
        let cmds = std::mem::take(&mut self.commands);
        self.stats.named_calls += 1;
        self.stats.commands_emitted += cmds.len() as u64;
        self.charge_elapsed(t0.elapsed().as_secs_f32() * 1000.0);
        (cmds, err)
    }

    /// Phase D — Args-bearing variant of [`Self::dispatch_named`]. Forwards
    /// `args` through [`super::adapter::ScriptAdapter::named_callback_with_args`]
    /// so the receiver sees `function method(entity, ...args)`. Adapters that
    /// don't override the args-bearing trait method silently drop the args
    /// (the default impl just delegates back to `named_callback`).
    pub fn dispatch_named_with_args(
        &mut self,
        entity_id: u64,
        fn_name: &str,
        args: &[super::value::Value],
        snapshot: &BridgeReadSnapshot,
        dt: f32,
    ) -> (Vec<BridgeCommand>, Option<ScriptError>) {
        let Some(handle) = self.by_entity.get(&entity_id).map(|l| l.handle) else {
            return (Vec::new(), None);
        };
        if !self.charge_budget(entity_id, fn_name, true) {
            return (Vec::new(), None);
        }
        let t0 = std::time::Instant::now();
        self.commands.clear();
        let mut ctx = BridgeContext::with_full_counters(
            dt,
            snapshot,
            self.capabilities.clone(),
            &mut self.commands,
            Rc::clone(&self.spawn_counter),
            Rc::clone(&self.sound_counter),
            Rc::clone(&self.tween_counter),
            Rc::clone(&self.event_listener_counter),
            Rc::clone(&self.timer_counter),
        );
        let err = self
            .adapter
            .named_callback_with_args(handle, entity_id, fn_name, args, &mut ctx)
            .err();
        let cmds = std::mem::take(&mut self.commands);
        self.stats.named_calls += 1;
        self.stats.commands_emitted += cmds.len() as u64;
        self.charge_elapsed(t0.elapsed().as_secs_f32() * 1000.0);
        (cmds, err)
    }

    /// P7.5c — Dispatch an event callback to a previously loaded entity.
    pub fn dispatch_event(
        &mut self,
        entity_id: u64,
        event_name: &str,
        sender: u64,
        snapshot: &BridgeReadSnapshot,
        dt: f32,
    ) -> (Vec<BridgeCommand>, Option<ScriptError>) {
        let Some(handle) = self.by_entity.get(&entity_id).map(|l| l.handle) else {
            return (Vec::new(), None);
        };
        if !self.charge_budget(entity_id, event_name, false) {
            return (Vec::new(), None);
        }
        let t0 = std::time::Instant::now();
        self.commands.clear();
        let mut ctx = BridgeContext::with_full_counters(
            dt,
            snapshot,
            self.capabilities.clone(),
            &mut self.commands,
            Rc::clone(&self.spawn_counter),
            Rc::clone(&self.sound_counter),
            Rc::clone(&self.tween_counter),
            Rc::clone(&self.event_listener_counter),
            Rc::clone(&self.timer_counter),
        );
        // User events name a handler by convention: `send_event("score_up")`
        // calls `on_score_up`. Engine lifecycle events arrive here already
        // prefixed via `EngineEvent::handler_name()`, so only prefix when the
        // caller has not.
        let handler = if event_name.starts_with("on_") {
            event_name.to_string()
        } else {
            format!("on_{event_name}")
        };
        let err = if self.adapter.has_function(handle, &handler) {
            self.adapter.event(handle, entity_id, &handler, &mut ctx).err()
        } else if self.adapter.has_function(handle, "on_event") {
            // **The other spelling.** The browser hosts have always called a
            // function literally named `on_event`, with
            // `(self, name, sender, data)`, and several projects are written
            // that way. On native
            // those handlers never fired, because only `on_<event>` was ever
            // looked up and a missing handler is silent.
            //
            // Specific name first, so nothing that works today changes; this
            // is only reached when the script has no `on_<event>` at all.
            // BUGS #150.
            let raw = event_name.strip_prefix("on_").unwrap_or(event_name);
            let args = [
                crate::scripting::bridge::value::Value::String(raw.to_string()),
                crate::scripting::bridge::value::Value::EntityId(sender),
                crate::scripting::bridge::value::Value::Nil,
            ];
            self.adapter
                .named_callback_with_args(handle, entity_id, "on_event", &args, &mut ctx)
                .err()
        } else {
            // Listening but with neither handler defined, which is ordinary:
            // most entities subscribe to nothing. Note the `on_event` check
            // above has to be the **raw** one, because a script's environment
            // inherits the engine globals - and one of those globals is
            // `on_event`, the registration function. Without that, every
            // script without a specific handler would have the engine's own
            // registration function called with a handler's arguments.
            None
        };
        let cmds = std::mem::take(&mut self.commands);
        self.stats.event_calls += 1;
        self.stats.commands_emitted += cmds.len() as u64;
        self.charge_elapsed(t0.elapsed().as_secs_f32() * 1000.0);
        (cmds, err)
    }

    /// Phase D — Payload-bearing variant of [`Self::dispatch_event`]. Forwards
    /// `data` through [`super::adapter::ScriptAdapter::event_with_data`] so
    /// `function on_<event>(entity, data)` receives the value. Adapters that
    /// don't override the payload-bearing trait method silently drop `data`.
    pub fn dispatch_event_with_data(
        &mut self,
        entity_id: u64,
        event_name: &str,
        data: &super::value::Value,
        snapshot: &BridgeReadSnapshot,
        dt: f32,
    ) -> (Vec<BridgeCommand>, Option<ScriptError>) {
        let Some(handle) = self.by_entity.get(&entity_id).map(|l| l.handle) else {
            return (Vec::new(), None);
        };
        if !self.charge_budget(entity_id, event_name, false) {
            return (Vec::new(), None);
        }
        let t0 = std::time::Instant::now();
        self.commands.clear();
        let mut ctx = BridgeContext::with_full_counters(
            dt,
            snapshot,
            self.capabilities.clone(),
            &mut self.commands,
            Rc::clone(&self.spawn_counter),
            Rc::clone(&self.sound_counter),
            Rc::clone(&self.tween_counter),
            Rc::clone(&self.event_listener_counter),
            Rc::clone(&self.timer_counter),
        );
        // Same convention as `dispatch_event`: user events name a handler,
        // engine lifecycle events arrive already prefixed.
        let handler = if event_name.starts_with("on_") {
            event_name.to_string()
        } else {
            format!("on_{event_name}")
        };
        let err = self
            .adapter
            .event_with_data(handle, entity_id, &handler, data, &mut ctx)
            .err();
        let cmds = std::mem::take(&mut self.commands);
        self.stats.event_calls += 1;
        self.stats.commands_emitted += cmds.len() as u64;
        self.charge_elapsed(t0.elapsed().as_secs_f32() * 1000.0);
        (cmds, err)
    }

    /// Forget all per-entity init state. Call after scene transitions.
    pub fn reset(&mut self) {
        self.by_entity.clear();
        self.by_path.clear();
        self.failed_paths.clear();
        self.subscriptions.clear();
        // Step 3.3 — a scene transition invalidates any deferred
        // calls (their target entities are gone) and resets the
        // per-frame budget tally to zero so the new scene starts with
        // a clean budget. The budget *config* is preserved on purpose;
        // callers configure it once at startup.
        self.deferred.clear();
        self.frame_invocations = 0;
        self.frame_elapsed_ms = 0.0;
        self.frame_warned = false;
        if let Some(handles) = self.collect_handles() {
            for h in handles {
                let _ = self.adapter.unload(h);
            }
        }
    }

    /// Step 3.1 — notify the runner that an entity has been despawned
    /// (removed from the world). If the entity is loaded, dispatches
    /// `on_despawn` to its script if subscribed, then removes the
    /// entity from `by_entity` and the subscription index. Returns the
    /// `(commands, error)` pair from the dispatch (commands the caller
    /// must apply normally; error is `Some` only when the adapter
    /// raised one).
    ///
    /// Idempotent: calling on an unknown entity_id is a silent no-op
    /// returning `(empty, None)`. The caller is allowed (and
    /// encouraged) to call this for every entity it considers gone
    /// every frame — the lookup miss is one HashMap probe.
    ///
    /// SAFETY note: handlers fired here MUST NOT assume the world
    /// still contains the entity. The bridge snapshot's `entity_map`
    /// may already be missing the corresponding `hecs::Entity`; script
    /// code that calls `get_position(entity)` etc. on its own id from
    /// inside `on_despawn` will get the snapshot fallback (`None` /
    /// last-known value), which is the documented contract.
    pub fn notify_despawn(
        &mut self,
        entity_id: u64,
        snapshot: &BridgeReadSnapshot,
        dt: f32,
    ) -> (Vec<BridgeCommand>, Option<ScriptError>) {
        let Some(loaded) = self.by_entity.get(&entity_id).cloned() else {
            return (Vec::new(), None);
        };
        let mut err: Option<ScriptError> = None;
        if loaded
            .subscribed_events.contains(&EngineEvent::Despawn)
        {
            // Step 3.3 — budget gate. We honour it for on_despawn
            // even though the entity is going away — if the host
            // explicitly Drops over-budget calls, dropping a despawn
            // hook is preferable to blowing the frame. The cleanup
            // (by_entity/subscriptions removal) below still runs so
            // we don't leak the entity's slot.
            if self.charge_budget(entity_id, EngineEvent::Despawn.handler_name(), false) {
                let t0 = std::time::Instant::now();
                self.commands.clear();
                let mut ctx = BridgeContext::with_full_counters(
                    dt,
                    snapshot,
                    self.capabilities.clone(),
                    &mut self.commands,
                    Rc::clone(&self.spawn_counter),
                    Rc::clone(&self.sound_counter),
                    Rc::clone(&self.tween_counter),
                    Rc::clone(&self.event_listener_counter),
                    Rc::clone(&self.timer_counter),
                );
                err = self
                    .adapter
                    .event(
                        loaded.handle,
                        entity_id,
                        EngineEvent::Despawn.handler_name(),
                        &mut ctx,
                    )
                    .err();
                self.stats.event_calls += 1;
                self.charge_elapsed(t0.elapsed().as_secs_f32() * 1000.0);
            }
        }
        // Drop the entity from the per-entity map and remove its
        // subscription-index entries (one swap_remove per declared
        // event — events are typically <= 4 per script so this is a
        // handful of bytes touched). We deliberately do NOT unload the
        // ScriptHandle: other entities may still be using the same
        // path, and the path cache (`by_path`) is shared. Per-handle
        // unload only happens at `reset()` time today; future work
        // (refcounted handles) would change this.
        self.by_entity.remove(&entity_id);
        for kind in &loaded.subscribed_events {
            if let Some(v) = self.subscriptions.get_mut(kind)
                && let Some(pos) = v.iter().position(|e| *e == entity_id) {
                    v.swap_remove(pos);
                }
        }
        let cmds = std::mem::take(&mut self.commands);
        self.stats.commands_emitted += cmds.len() as u64;
        (cmds, err)
    }

    fn collect_handles(&self) -> Option<Vec<ScriptHandle>> {
        if self.by_path.is_empty() {
            None
        } else {
            Some(self.by_path.values().copied().collect())
        }
    }
}

/// How many frames of `dt` the runner remembers - more than any tick
/// interval can be (`tick_interval_frames` is a `u8`).
const DT_HISTORY: usize = 256;

/// Whether input changed this frame: a key, action or mouse button went
/// down or came up.
fn has_input_edges(snap: &BridgeReadSnapshot) -> bool {
    !snap.keys_just_pressed.is_empty()
        || !snap.actions_just_pressed.is_empty()
        || !snap.actions_just_released.is_empty()
        || !snap.just_pressed_bindings.is_empty()
        || snap.mouse_left_just_pressed
        || snap.mouse_left_just_released
        || snap.mouse_right_just_pressed
        || snap.mouse_right_just_released
        || snap.mouse_middle_just_pressed
        || snap.mouse_middle_just_released
}

/// The time since a script ticking every `n` frames last ran: the last `n`
/// frames' dt, this one included.
fn window_dt(recent: &[f32; DT_HISTORY], frame_index: u64, n: u16) -> f32 {
    let n = (n.max(1) as u64).min(frame_index + 1);
    (0..n).map(|k| recent[((frame_index - k) % DT_HISTORY as u64) as usize]).sum()
}

/// Step 3a — compute the per-entity tick interval in host frames from
/// the script source's declared `tick_rate` / `tickHz` header (if any),
/// falling back to `default_hz` when the header is absent. The result
/// is clamped to `[1, 255]` since we store it in a `u8` and an interval
/// > 255 frames (= 0.23 Hz at 60 Hz base) is firmly in "use a timer"
/// > territory.
fn tick_interval_for(source: &str, _entity_id: u64, default_hz: u32, base_hz: u32) -> u8 {
    let declared = super::metadata::parse_tick_rate(source).unwrap_or(default_hz);
    let declared = declared.clamp(1, base_hz);
    let interval = (base_hz / declared).max(1);
    interval.min(u8::MAX as u32) as u8
}

/// Step 3a — deterministic per-entity tick phase in `[0, interval)`.
/// The phase is chosen so a population of entities at the same tick
/// rate spreads evenly across the interval rather than synchronising
/// on one frame. We mix the entity id (so entities pick distinct
/// phases without any global counter) with the source length, then
/// reduce modulo `interval` so the result is always within the valid
/// range used by the schedule check.
fn tick_phase_for(entity_id: u64, source: &str, interval: u8) -> u8 {
    if interval <= 1 {
        return 0;
    }
    let mut h = entity_id.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= source.len() as u64;
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    (h % interval as u64) as u8
}

#[cfg(test)]
mod tests {
    use super::super::adapter::NoopAdapter;
    use super::*;

    /// Step 3.1 test helper — adapter that emits a `Log` command from
    /// `init`, `update`, and `event` so tests can assert exactly which
    /// hooks fired in what order via the resulting BridgeCommand stream.
    /// Returns errors only if explicitly configured (none of the tests
    /// here use that path; keeping the field private + always-false).
    struct TraceAdapter {
        next: u64,
    }
    impl TraceAdapter {
        fn new() -> Self {
            Self { next: 1 }
        }
    }
    impl super::super::adapter::ScriptAdapter for TraceAdapter {
        fn load(&mut self, _path: &str, _source: &str) -> Result<ScriptHandle, ScriptError> {
            let h = ScriptHandle(self.next);
            self.next += 1;
            Ok(h)
        }
        fn init(
            &mut self,
            _h: ScriptHandle,
            entity_id: u64,
            ctx: &mut BridgeContext<'_>,
        ) -> Result<(), ScriptError> {
            ctx.push_command(BridgeCommand::Log {
                message: format!("init:{entity_id}"),
            });
            Ok(())
        }
        fn update(
            &mut self,
            _h: ScriptHandle,
            entity_id: u64,
            ctx: &mut BridgeContext<'_>,
        ) -> Result<(), ScriptError> {
            ctx.push_command(BridgeCommand::Log {
                message: format!("update:{entity_id}"),
            });
            Ok(())
        }
        // The mock implements the payload-bearing variant, because that is
        // the one the trait's `event` default forwards to. Implementing only
        // `event` would leave `dispatch_event_with_data` hitting the no-op
        // default and silently observing nothing.
        fn event_with_data(
            &mut self,
            _h: ScriptHandle,
            entity_id: u64,
            event_name: &str,
            _data: &crate::scripting::bridge::value::Value,
            ctx: &mut BridgeContext<'_>,
        ) -> Result<(), ScriptError> {
            ctx.push_command(BridgeCommand::Log {
                message: format!("event:{event_name}:{entity_id}"),
            });
            Ok(())
        }
        fn unload(&mut self, _h: ScriptHandle) -> Result<(), ScriptError> {
            Ok(())
        }
    }

    fn logs(cmds: &[BridgeCommand]) -> Vec<String> {
        cmds.iter()
            .filter_map(|c| match c {
                BridgeCommand::Log { message } => Some(message.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn noop_runner_executes_lifecycle() {
        let mut runner = ScriptBridgeRunner::new(Box::new(NoopAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        let loads = vec![(7u64, "p.luau".to_string(), "src".to_string())];
        let (cmds, errs) = runner.run_frame(&loads, &snap, 0.016, &[]);
        assert!(cmds.is_empty(), "noop produces no commands");
        assert!(errs.is_empty(), "noop produces no errors");
        // Second frame should not re-init.
        let (cmds2, errs2) = runner.run_frame(&loads, &snap, 0.016, &[]);
        assert!(cmds2.is_empty());
        assert!(errs2.is_empty());
    }

    /// P1.5 — `unload_path` drops the per-path cache entry and every
    /// per-entity load tied to it, then asks the adapter to unload.
    /// After unload, the same `(entity, path)` load on the next frame
    /// must re-attach (fresh handle, fresh `init`).
    #[test]
    fn unload_path_drops_handle_and_per_entity_state() {
        let mut runner = ScriptBridgeRunner::new(Box::new(TraceAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        let loads = vec![(7u64, "p.luau".to_string(), "src".to_string())];

        // Frame 1 — initial attach + init.
        let (cmds, _) = runner.run_frame(&loads, &snap, 0.016, &[]);
        assert_eq!(logs(&cmds), vec!["init:7".to_string(), "update:7".to_string()]);
        assert!(runner.is_path_loaded("p.luau"));

        // Unload the path — should report it was loaded.
        assert!(runner.unload_path("p.luau"));
        assert!(!runner.is_path_loaded("p.luau"));

        // Frame 2 — same loads → entity must re-init (fresh attach).
        let (cmds2, _) = runner.run_frame(&loads, &snap, 0.016, &[]);
        assert_eq!(
            logs(&cmds2),
            vec!["init:7".to_string(), "update:7".to_string()],
            "post-unload re-attach should fire init again"
        );

        // Unloading an unknown path is a cheap no-op.
        assert!(!runner.unload_path("nonexistent.luau"));
    }

    /// P7.7 wedge 19 — runner-level coverage that JS scripts can dispatch
    /// arbitrary named callbacks. Before wedge 19 this path silently
    /// no-op'd on native because `JsAdapter::named_callback` was a stub,
    /// so timer-fired and GUI-click-fired callbacks for JS scripts in
    /// production never invoked anything. The adapter-level test in
    /// `js::tests::named_callback_resolves_via_lookup` exercises the
    /// adapter directly; this test exercises the same path through the
    /// runner that production uses (`ScriptSystem::dispatch_bridge_named`
    /// at `crates/jengine-core/src/ecs/systems.rs:~1955`).
    #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
    #[test]
    fn js_runner_dispatches_named_callback() {
        use super::super::adapters::JsAdapter;
        use super::super::command::BridgeCommand;
        let mut runner = ScriptBridgeRunner::new(Box::new(JsAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        // Source is Luau syntax — the adapter transpiles before QuickJS sees it.
        let src = "function on_resume(entity)\n  log('runner_named')\nend\n";
        let loads = vec![(11u64, "runner_named.luau".to_string(), src.to_string())];
        // First frame loads + inits the script.
        let (_cmds, errs) = runner.run_frame(&loads, &snap, 0.016, &[]);
        assert!(errs.is_empty(), "load+init should succeed; got {errs:?}");
        // Now dispatch the named callback that production wires through
        // `dispatch_bridge_named` for fired timers + GUI clicks.
        let (cmds, err) = runner.dispatch_named(11, "on_resume", &snap, 0.016);
        assert!(err.is_none(), "named dispatch should not error; got {err:?}");
        let logged = cmds
            .iter()
            .any(|c| matches!(c, BridgeCommand::Log { message } if message == "runner_named"));
        assert!(logged, "expected on_resume to fire and log; got {cmds:?}");
        // Missing handler → no error, no commands.
        let (cmds2, err2) = runner.dispatch_named(11, "no_such", &snap, 0.016);
        assert!(err2.is_none());
        assert!(cmds2.is_empty(), "missing handler should not emit commands; got {cmds2:?}");
        // Unknown entity → silent no-op (matches production behaviour for
        // entities that live in the other-language runner).
        let (cmds3, err3) = runner.dispatch_named(999, "on_resume", &snap, 0.016);
        assert!(err3.is_none());
        assert!(cmds3.is_empty());
    }

    /// Phase D — args plumbing. The receiver's `function method(entity, ...args)`
    /// must see each primitive arg dispatched through
    /// [`ScriptBridgeRunner::dispatch_named_with_args`]. Mirror of
    /// `js_runner_dispatches_named_callback` with payloads.
    #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
    #[test]
    fn js_runner_dispatches_named_with_args() {
        use super::super::adapters::JsAdapter;
        use super::super::command::BridgeCommand;
        use super::super::value::Value as V;
        let mut runner = ScriptBridgeRunner::new(Box::new(JsAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        // Receiver concatenates args into a log line so the test can verify
        // each primitive type round-trips through QuickJS.
        let src = "function damage(entity, amount, source, crit)\n  log('hit:' .. amount .. ':' .. source .. ':' .. tostring(crit))\nend\n";
        let loads = vec![(11u64, "args.luau".to_string(), src.to_string())];
        let (_cmds, errs) = runner.run_frame(&loads, &snap, 0.016, &[]);
        assert!(errs.is_empty(), "load+init should succeed; got {errs:?}");
        let args = vec![V::Number(10.0), V::String("fire".into()), V::Bool(true)];
        let (cmds, err) = runner.dispatch_named_with_args(11, "damage", &args, &snap, 0.016);
        assert!(err.is_none(), "args dispatch should not error; got {err:?}");
        let logged = cmds
            .iter()
            .any(|c| matches!(c, BridgeCommand::Log { message } if message == "hit:10:fire:true"));
        assert!(logged, "expected damage handler to receive all args; got {cmds:?}");
    }

    /// Phase D — event payload plumbing. `dispatch_event_with_data` should
    /// resolve `on_<event>(entity, data)` and pass the value through.
    #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
    #[test]
    fn js_runner_dispatches_event_with_data() {
        use super::super::adapters::JsAdapter;
        use super::super::command::BridgeCommand;
        use super::super::value::Value as V;
        let mut runner = ScriptBridgeRunner::new(Box::new(JsAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        let src = "function on_hit(entity, data)\n  log('on_hit:' .. data)\nend\n";
        let loads = vec![(22u64, "ev.luau".to_string(), src.to_string())];
        let (_cmds, errs) = runner.run_frame(&loads, &snap, 0.016, &[]);
        assert!(errs.is_empty(), "load+init should succeed; got {errs:?}");
        let payload = V::String("ouch".into());
        let (cmds, err) = runner.dispatch_event_with_data(22, "hit", &payload, &snap, 0.016);
        assert!(err.is_none(), "event dispatch should not error; got {err:?}");
        let logged = cmds
            .iter()
            .any(|c| matches!(c, BridgeCommand::Log { message } if message == "on_hit:ouch"));
        assert!(logged, "expected on_hit to receive data; got {cmds:?}");
    }

    // ---------------- Slice 3.1: subscription index + spawn/despawn ----------------

    /// A script that doesn't declare an `events = [...]` header has no
    /// subscriptions, no on_spawn dispatch, and `subscriber_count`
    /// returns 0 for every kind. Baseline / regression test for the
    /// "we didn't accidentally subscribe everyone to everything" case.
    #[test]
    fn unsubscribed_script_does_not_dispatch_lifecycle_events() {
        let mut runner =
            ScriptBridgeRunner::new(Box::new(TraceAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        // No `events = [...]` field; tick_rate header optional.
        let src = "script = { tick_rate = 60 }\n".to_string();
        let loads = vec![(7u64, "p.luau".to_string(), src)];
        let (cmds, errs) = runner.run_frame(&loads, &snap, 0.016, &[]);
        assert!(errs.is_empty());
        // Should see init + update only — no event:on_spawn line.
        let log = logs(&cmds);
        assert_eq!(log, vec!["init:7".to_string(), "update:7".to_string()]);
        assert_eq!(runner.subscriber_count(EngineEvent::Spawn), 0);
        assert!(!runner.is_subscribed(7, EngineEvent::Spawn));
    }

    /// Declaring `events = ["on_spawn"]` causes:
    ///   1. the entity is recorded in `subscriptions[Spawn]`
    ///   2. `on_spawn` is dispatched exactly once, right after `init()`,
    ///      sharing the same per-frame ctx (commands land in the same
    ///      Vec the run_frame caller consumes).
    /// Subsequent frames do NOT re-fire `on_spawn`.
    #[test]
    fn on_spawn_fires_once_after_init_when_subscribed() {
        let mut runner =
            ScriptBridgeRunner::new(Box::new(TraceAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        let src = "script = { tick_rate = 60, events = { \"on_spawn\" } }\n".to_string();
        let loads = vec![(42u64, "p.luau".to_string(), src)];
        let (cmds1, errs1) = runner.run_frame(&loads, &snap, 0.016, &[]);
        assert!(errs1.is_empty());
        assert_eq!(
            logs(&cmds1),
            vec![
                "init:42".to_string(),
                "event:on_spawn:42".to_string(),
                "update:42".to_string(),
            ],
        );
        assert_eq!(runner.subscriber_count(EngineEvent::Spawn), 1);
        assert!(runner.is_subscribed(42, EngineEvent::Spawn));
        // Frame 2: only `update`, no re-init, no second on_spawn.
        let (cmds2, errs2) = runner.run_frame(&loads, &snap, 0.016, &[]);
        assert!(errs2.is_empty());
        assert_eq!(logs(&cmds2), vec!["update:42".to_string()]);
    }

    /// `notify_despawn` dispatches `on_despawn` if subscribed, then
    /// removes the entity from both `by_entity` and the subscription
    /// index. Idempotent for unknown ids.
    #[test]
    fn on_despawn_fires_and_index_is_cleaned_up() {
        let mut runner =
            ScriptBridgeRunner::new(Box::new(TraceAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        let src =
            "script = { events = { \"on_spawn\", \"on_despawn\" } }\n".to_string();
        let loads = vec![(9u64, "p.luau".to_string(), src)];
        let (_cmds, errs) = runner.run_frame(&loads, &snap, 0.016, &[]);
        assert!(errs.is_empty());
        assert_eq!(runner.subscriber_count(EngineEvent::Spawn), 1);
        assert_eq!(runner.subscriber_count(EngineEvent::Despawn), 1);

        // Despawn it.
        let (cmds, err) = runner.notify_despawn(9, &snap, 0.016);
        assert!(err.is_none());
        assert_eq!(logs(&cmds), vec!["event:on_despawn:9".to_string()]);
        assert_eq!(runner.subscriber_count(EngineEvent::Spawn), 0);
        assert_eq!(runner.subscriber_count(EngineEvent::Despawn), 0);
        assert!(!runner.is_subscribed(9, EngineEvent::Despawn));

        // Idempotent — calling again is a silent no-op.
        let (cmds2, err2) = runner.notify_despawn(9, &snap, 0.016);
        assert!(err2.is_none());
        assert!(cmds2.is_empty());
        // Unknown entity id — silent no-op.
        let (cmds3, err3) = runner.notify_despawn(12345, &snap, 0.016);
        assert!(err3.is_none());
        assert!(cmds3.is_empty());
    }

    /// Despawn-without-subscription must NOT dispatch `on_despawn` but
    /// MUST still remove the entity from `by_entity`. Guards against
    /// silently leaking handles for the common case (no subscription).
    #[test]
    fn notify_despawn_without_subscription_only_removes_entity() {
        let mut runner =
            ScriptBridgeRunner::new(Box::new(TraceAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        let src = "script = { tick_rate = 60 }\n".to_string();
        let loads = vec![(3u64, "p.luau".to_string(), src)];
        let (_, _) = runner.run_frame(&loads, &snap, 0.016, &[]);
        // Confirm loaded.
        assert!(!runner.is_subscribed(3, EngineEvent::Despawn));
        let (cmds, err) = runner.notify_despawn(3, &snap, 0.016);
        assert!(err.is_none());
        assert!(cmds.is_empty(), "no on_despawn handler => no commands");
        // Passing the same id again finds nothing — entity was removed.
        let (cmds2, _) = runner.notify_despawn(3, &snap, 0.016);
        assert!(cmds2.is_empty());
    }

    /// `reset()` must clear the subscription index too — otherwise
    /// scene transitions would leave dangling entity ids that point at
    /// since-unloaded handles.
    #[test]
    fn reset_clears_subscription_index() {
        let mut runner =
            ScriptBridgeRunner::new(Box::new(TraceAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        let src =
            "script = { events = { \"on_spawn\", \"on_collision_enter\" } }\n".to_string();
        let loads = vec![(1u64, "a.luau".to_string(), src.clone())];
        let _ = runner.run_frame(&loads, &snap, 0.016, &[]);
        assert_eq!(runner.subscriber_count(EngineEvent::Spawn), 1);
        assert_eq!(runner.subscriber_count(EngineEvent::CollisionEnter), 1);
        runner.reset();
        assert_eq!(runner.subscriber_count(EngineEvent::Spawn), 0);
        assert_eq!(runner.subscriber_count(EngineEvent::CollisionEnter), 0);
    }

    /// Multiple entities sharing the same subscribed event all show up
    /// in the index in load order. `notify_despawn` removes only the
    /// requested entity; others remain subscribed.
    #[test]
    fn multiple_subscribers_indexed_independently() {
        let mut runner =
            ScriptBridgeRunner::new(Box::new(TraceAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        let src = "script = { events = { \"on_spawn\" } }\n".to_string();
        let loads = vec![
            (1u64, "p.luau".to_string(), src.clone()),
            (2u64, "p.luau".to_string(), src.clone()),
            (3u64, "p.luau".to_string(), src),
        ];
        let _ = runner.run_frame(&loads, &snap, 0.016, &[]);
        assert_eq!(runner.subscriber_count(EngineEvent::Spawn), 3);
        assert!(runner.is_subscribed(1, EngineEvent::Spawn));
        assert!(runner.is_subscribed(2, EngineEvent::Spawn));
        assert!(runner.is_subscribed(3, EngineEvent::Spawn));

        let (_, _) = runner.notify_despawn(2, &snap, 0.016);
        assert_eq!(runner.subscriber_count(EngineEvent::Spawn), 2);
        assert!(runner.is_subscribed(1, EngineEvent::Spawn));
        assert!(!runner.is_subscribed(2, EngineEvent::Spawn));
        assert!(runner.is_subscribed(3, EngineEvent::Spawn));
    }

    // ---------------- Slice 3.3: budget caps + overflow policies ----------------

    /// Default budget is unlimited + Warn — every dispatch proceeds and
    /// `events_dropped_or_deferred` stays at zero. Baseline / regression
    /// guard.
    #[test]
    fn default_budget_is_unlimited() {
        let mut runner =
            ScriptBridgeRunner::new(Box::new(TraceAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        let src = "script = { events = { \"on_spawn\" } }\n".to_string();
        let _ = runner.run_frame(
            &[(1u64, "p.luau".to_string(), src)],
            &snap,
            0.016,
            &[],
        );
        // Fire 50 named callbacks — none should be dropped.
        for _ in 0..50 {
            let (_, err) = runner.dispatch_named(1, "any", &snap, 0.016);
            assert!(err.is_none());
        }
        assert_eq!(runner.stats.events_dropped_or_deferred, 0);
    }

    /// Drop policy with `max_invocations_per_frame = 2` lets the first
    /// two dispatches through and drops the rest. The dropped count is
    /// reported on `RunnerStats`.
    #[test]
    fn drop_policy_caps_invocations() {
        let mut runner =
            ScriptBridgeRunner::new(Box::new(TraceAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        let src = "script = { events = { \"on_spawn\" } }\n".to_string();
        let _ = runner.run_frame(
            &[(1u64, "p.luau".to_string(), src)],
            &snap,
            0.016,
            &[],
        );
        // Frame 1's on_spawn already counts as 1 invocation against
        // the budget — start frame 2 to reset and isolate.
        runner.start_frame();
        runner.set_budget(BudgetConfig {
            max_script_ms_per_frame: 0.0,
            max_invocations_per_frame: 2,
            overflow_policy: BudgetPolicy::Drop,
            deferred_cap: 0,
        });

        let (cmds_a, _) = runner.dispatch_event(1, "on_x", 0, &snap, 0.016);
        let (cmds_b, _) = runner.dispatch_event(1, "on_y", 0, &snap, 0.016);
        let (cmds_c, _) = runner.dispatch_event(1, "on_z", 0, &snap, 0.016);
        let (cmds_d, _) = runner.dispatch_event(1, "on_w", 0, &snap, 0.016);

        // First two should produce log commands; third + fourth dropped.
        assert!(!cmds_a.is_empty(), "first dispatch should run");
        assert!(!cmds_b.is_empty(), "second dispatch should run");
        assert!(cmds_c.is_empty(), "third dispatch should be dropped");
        assert!(cmds_d.is_empty(), "fourth dispatch should be dropped");
        assert_eq!(runner.stats.events_dropped_or_deferred, 2);
    }

    /// Defer policy queues over-budget calls onto the deferred list,
    /// `start_frame` returns them next frame for re-dispatch, and the
    /// queue cap is honoured. Entries that don't fit the cap are
    /// counted as dropped.
    #[test]
    fn defer_policy_queues_then_replays() {
        let mut runner =
            ScriptBridgeRunner::new(Box::new(TraceAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        let src = "script = { events = { \"on_spawn\" } }\n".to_string();
        let _ = runner.run_frame(
            &[(1u64, "p.luau".to_string(), src)],
            &snap,
            0.016,
            &[],
        );
        runner.start_frame();
        runner.set_budget(BudgetConfig {
            max_script_ms_per_frame: 0.0,
            max_invocations_per_frame: 1,
            overflow_policy: BudgetPolicy::Defer,
            deferred_cap: 2,
        });

        // Dispatch 4 events: first runs, next 2 deferred (cap=2),
        // 4th overflows the queue and is dropped.
        let _ = runner.dispatch_event(1, "on_a", 0, &snap, 0.016);
        let _ = runner.dispatch_event(1, "on_b", 0, &snap, 0.016);
        let _ = runner.dispatch_event(1, "on_c", 0, &snap, 0.016);
        let _ = runner.dispatch_event(1, "on_d", 0, &snap, 0.016);
        assert_eq!(runner.stats.events_dropped_or_deferred, 3);

        // start_frame returns the 2 queued entries (for next-frame
        // replay) and resets the per-frame counters.
        let replay = runner.start_frame();
        assert_eq!(
            replay,
            vec![
                (1u64, "on_b".to_string(), false),
                (1u64, "on_c".to_string(), false),
            ],
        );
        // Queue is now drained.
        let replay2 = runner.start_frame();
        assert!(replay2.is_empty());
    }

    /// Warn policy logs but still runs the dispatch (preserves
    /// pre-budget semantics for hosts that just want telemetry).
    /// We can't assert on the log line directly here, but we can
    /// assert that the call still ran (commands emitted) and that
    /// `events_dropped_or_deferred` stayed at zero.
    #[test]
    fn warn_policy_runs_dispatch_anyway() {
        let mut runner =
            ScriptBridgeRunner::new(Box::new(TraceAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        let src = "script = { events = { \"on_spawn\" } }\n".to_string();
        let _ = runner.run_frame(
            &[(1u64, "p.luau".to_string(), src)],
            &snap,
            0.016,
            &[],
        );
        runner.start_frame();
        runner.set_budget(BudgetConfig {
            max_script_ms_per_frame: 0.0,
            max_invocations_per_frame: 1,
            overflow_policy: BudgetPolicy::Warn,
            deferred_cap: 0,
        });
        let (cmds_a, _) = runner.dispatch_event(1, "on_a", 0, &snap, 0.016);
        let (cmds_b, _) = runner.dispatch_event(1, "on_b", 0, &snap, 0.016);
        assert!(!cmds_a.is_empty());
        assert!(!cmds_b.is_empty(), "Warn policy must still run the call");
        assert_eq!(runner.stats.events_dropped_or_deferred, 0);
    }

    /// `reset()` (scene transition) clears deferred calls and per-frame
    /// budget counters but keeps the budget *config* — hosts configure
    /// budget once and expect it to survive scene loads.
    #[test]
    fn reset_preserves_budget_config_but_clears_deferred() {
        let mut runner =
            ScriptBridgeRunner::new(Box::new(TraceAdapter::new()), Capabilities::empty());
        let snap = BridgeReadSnapshot::default();
        let src = "script = { events = { \"on_spawn\" } }\n".to_string();
        let _ = runner.run_frame(
            &[(1u64, "p.luau".to_string(), src)],
            &snap,
            0.016,
            &[],
        );
        let cfg = BudgetConfig {
            max_script_ms_per_frame: 0.0,
            max_invocations_per_frame: 1,
            overflow_policy: BudgetPolicy::Defer,
            deferred_cap: 4,
        };
        runner.set_budget(cfg);
        runner.start_frame();
        let _ = runner.dispatch_event(1, "on_a", 0, &snap, 0.016);
        let _ = runner.dispatch_event(1, "on_b", 0, &snap, 0.016); // queued
        runner.reset();
        // Config preserved.
        assert_eq!(runner.budget().max_invocations_per_frame, 1);
        assert_eq!(runner.budget().overflow_policy, BudgetPolicy::Defer);
        // Queue + per-frame counters cleared. start_frame returns nothing.
        let replay = runner.start_frame();
        assert!(replay.is_empty());
    }
}
