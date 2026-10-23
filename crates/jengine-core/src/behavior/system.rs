//! `BehaviorGraphSystem` — drives behavior graph execution.
//!
//! Slice 7.2 wires Phase 2 (`tick_pre_simulation`) per spec §4.4.
//! Slice 7.3 adds:
//! - Lifecycle dispatch (`OnSpawn` fires after `sync_instances`
//!   attaches a new instance; `OnDespawn` fires before detach).
//! - Phase 6 hooks (`dispatch_collision`, `dispatch_trigger`,
//!   `dispatch_timer`) that callers (collision system, trigger
//!   volumes, timer queue) invoke to push events into the graph.
//! - O(1) subscription pruning via [`crate::behavior::CompiledGraph::subscriptions`].
//!
//! Per-instance allocation is zero on the hot path: pin scratch and
//! the counters struct are owned by the system and reused every event.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

/// Cross-platform monotonic timer. `std::time::Instant::now()` panics
/// on wasm32 ("time not implemented on this platform"), so on the web
/// we fall back to `performance.now()` via `js_sys::Date::now()`.
#[derive(Copy, Clone)]
struct MonoTimer {
    #[cfg(not(target_arch = "wasm32"))]
    start: Instant,
    #[cfg(target_arch = "wasm32")]
    start_ms: f64,
}

impl MonoTimer {
    fn now() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        { Self { start: Instant::now() } }
        #[cfg(target_arch = "wasm32")]
        { Self { start_ms: js_sys::Date::now() } }
    }

    fn elapsed_us(&self) -> u64 {
        #[cfg(not(target_arch = "wasm32"))]
        { self.start.elapsed().as_micros() as u64 }
        #[cfg(target_arch = "wasm32")]
        { ((js_sys::Date::now() - self.start_ms) * 1000.0) as u64 }
    }
}

use hecs::World;
use log::warn;
use thiserror::Error;

use crate::behavior::cache::{BehaviorCacheError, BehaviorGraphCache, GraphId};
use crate::behavior::command::{BehaviorCommand, CommandRouteCounters};
use crate::behavior::compile::KeyFilter;
use crate::behavior::event::{EventPayload, TriggerKind};
use crate::behavior::instances::BehaviorInstances;
use crate::behavior::interpreter::{tick_event, BehaviorCounters, BehaviorExecContext};
use crate::behavior::pin::PinFrame;
use crate::ecs::components::BehaviorGraphComponent;
use crate::profiling::FrameCounters;

#[derive(Debug)]
pub struct BehaviorGraphSystem {
    pub cache: BehaviorGraphCache,
    pub instances: BehaviorInstances,
    pins: PinFrame,
    pub counters: BehaviorCounters,
    /// Per-frame OnTick budget in milliseconds. When the wall-clock
    /// time spent inside [`Self::tick_pre_simulation`] exceeds this,
    /// the overrun is recorded in `counters.tick_overbudget_us`
    /// (telemetry only; the graphs still run — spec §6 forbids drop
    /// for v1) and a single `warn!` is emitted per frame. `0.0`
    /// disables the check.
    pub tick_budget_ms: f32,
    /// Per-frame event-dispatch budget in milliseconds. Sums the
    /// time spent inside all `dispatch_*` calls (Phase 6). Same
    /// telemetry-only contract as `tick_budget_ms`.
    pub event_budget_ms: f32,
    /// Latched once per frame to avoid log spam when the budget is
    /// blown by a wide margin. Cleared at the top of each
    /// `tick_pre_simulation`.
    overbudget_warned_this_frame: bool,
    /// Structural commands queued by graph ops (`SpawnEntity`,
    /// `Despawn`, `PlaySound`). Drained via
    /// [`Self::flush_commands`] at the Phase 7 command barrier.
    commands: Vec<BehaviorCommand>,
    /// Entities whose graph load failed; cached so we don't retry
    /// every frame and spam the log.
    failed_entities: HashSet<hecs::Entity>,
    /// Instances attached this `sync_instances` call — OnSpawn fires
    /// for them at the end of `sync_instances`. Stored as entity
    /// handles so dispatch can resolve to the live `&mut state` (and
    /// any `Cooldown` / `Once` writes inside an OnSpawn handler
    /// persist to the real instance, not a clone).
    pending_spawn: Vec<hecs::Entity>,
    /// Instances about to be detached — OnDespawn fires before they
    /// drop. Drained at the end of `sync_instances` (before the actual
    /// detach calls so the index lookup still resolves).
    pending_despawn: Vec<hecs::Entity>,
}

impl Default for BehaviorGraphSystem {
    fn default() -> Self {
        Self {
            cache: BehaviorGraphCache::default(),
            instances: BehaviorInstances::default(),
            pins: PinFrame::default(),
            counters: BehaviorCounters::default(),
            // Spec §6: telemetry-only budgets. Defaults give a
            // sensible 4 ms / 2 ms split at 60 Hz; callers can
            // tighten or zero them out per project.
            tick_budget_ms: 4.0,
            event_budget_ms: 2.0,
            overbudget_warned_this_frame: false,
            commands: Vec::new(),
            failed_entities: HashSet::new(),
            pending_spawn: Vec::new(),
            pending_despawn: Vec::new(),
        }
    }
}

/// The most `OnTimer` firings one instance gets in a single frame.
pub const MAX_TIMER_CATCH_UP: u32 = 4;

/// Screen pixels -> world, through the scene's active `Camera2DComponent`
/// (or an unzoomed camera at the origin the size of the screen). The same
/// arithmetic as `Camera2D::screen_to_world` and the browser preamble's
/// `get_mouse_world_position`, the camera's own rotation included.
pub fn mouse_world_position(world: &World, mouse: (f32, f32), screen: (f32, f32)) -> glam::Vec2 {
    use crate::ecs::components::{Camera2DComponent, Transform};
    let (sw, sh) = (screen.0.max(1.0), screen.1.max(1.0));
    let (mut cx, mut cy, mut vw, mut vh, mut zoom) = (0.0, 0.0, sw, sh, 1.0);
    let mut rotation = 0.0f32;
    let main = crate::ecs::camera_rig::main_camera(world);
    for (e, (t, cam)) in world.query::<(&Transform, &Camera2DComponent)>().iter() {
        if Some(e) != main {
            continue;
        }
        cx = t.position.x;
        cy = t.position.y;
        rotation = t.rotation;
        if cam.viewport_width > 0.0 && cam.viewport_height > 0.0 {
            // The world the screen actually shows, as the projection has it.
            let view = cam.view_on(glam::Vec2::new(sw, sh));
            vw = view.x;
            vh = view.y;
        }
        if cam.zoom > 0.0 {
            zoom = cam.zoom;
        }
        break;
    }
    let nx = (mouse.0 - sw * 0.5) / sw;
    let ny = (sh * 0.5 - mouse.1) / sh;
    let (ox, oy) = (nx * (vw / zoom), ny * (vh / zoom));
    let (sin, cos) = rotation.sin_cos();
    glam::Vec2::new(cx + ox * cos - oy * sin, cy + ox * sin + oy * cos)
}

impl BehaviorGraphSystem {
    pub fn new() -> Self {
        Self::default()
    }

    /// Walk all entities with `BehaviorGraphComponent`, ensure their
    /// graphs are loaded, and bind instances. Drops instances for
    /// entities that no longer have the component or that have been
    /// despawned. `project_root` is the directory the
    /// `graph_path` field is resolved against.
    ///
    /// Lifecycle events: any newly-attached instance subscribed to
    /// `OnSpawn` fires immediately at the end of this call; any
    /// about-to-be-detached instance subscribed to `OnDespawn` fires
    /// before its state is dropped.
    pub fn sync_instances(
        &mut self,
        world: &mut World,
        project_root: &Path,
    ) -> Vec<BehaviorSyncError> {
        self.sync_with(world, |cache, graph_path| {
            let path = project_root.join(graph_path);
            Some((cache.load_from_path(&path), path))
        })
    }

    /// [`sync_instances`](Self::sync_instances) for a runtime with no
    /// filesystem: graph JSON comes from `graphs`, keyed by `graph_path`, as
    /// the editor viewport and web builds hold it. A graph not in the map
    /// yet is skipped quietly and bound on a later call.
    ///
    /// This is the per-frame call for those runtimes, and it is what makes
    /// them prune: before it, both bound new instances but never dropped
    /// one, so a despawned entity kept its graph forever and `OnDespawn`
    /// never fired there (test-lab BUGS #93). It also fires `OnSpawn` for
    /// what it binds, so callers must not dispatch it again.
    pub fn sync_instances_inline(
        &mut self,
        world: &mut World,
        graphs: &HashMap<String, String>,
    ) -> Vec<BehaviorSyncError> {
        self.sync_with(world, |cache, graph_path| {
            let json = graphs.get(graph_path)?;
            let key = PathBuf::from(graph_path);
            Some((cache.load_from_json_with_key(key.clone(), json), key))
        })
    }

    fn sync_with(
        &mut self,
        world: &mut World,
        mut load: impl FnMut(
            &mut BehaviorGraphCache,
            &str,
        ) -> Option<(Result<GraphId, BehaviorCacheError>, PathBuf)>,
    ) -> Vec<BehaviorSyncError> {
        let mut errors = Vec::new();
        let mut alive: HashSet<hecs::Entity> = HashSet::new();

        for (entity, comp) in world.query::<&BehaviorGraphComponent>().iter() {
            alive.insert(entity);
            if self.instances.contains(entity) {
                continue;
            }
            let Some((loaded, path)) = load(&mut self.cache, &comp.graph_path) else {
                continue;
            };
            match loaded {
                Ok(id) => {
                    let layout = self.cache.get(id).state_layout;
                    self.instances.attach(entity, id, &layout);
                    self.failed_entities.remove(&entity);
                    self.pending_spawn.push(entity);
                }
                Err(e) => {
                    if self.failed_entities.insert(entity) {
                        errors.push(BehaviorSyncError {
                            entity,
                            path,
                            source: e,
                        });
                    }
                }
            }
        }

        // Remember where every live instance is, for the `OnDespawn` it may
        // get on a later sync - by which time its entity will be gone. One
        // transform copy per graph entity, which is small beside ticking it.
        for inst in self.instances.instances.iter_mut() {
            if let Ok(t) = world.get::<&crate::ecs::components::Transform>(inst.entity) {
                inst.last_seen = Some((*t).clone());
            }
        }

        // Detach instances whose entity no longer carries the
        // component or has been despawned. Queue them for OnDespawn,
        // flush, then perform the actual detaches — the index lookup
        // would fail post-detach.
        let stale: Vec<hecs::Entity> = self
            .instances
            .instances
            .iter()
            .map(|s| s.entity)
            .filter(|e| !alive.contains(e))
            .collect();
        for e in &stale {
            self.pending_despawn.push(*e);
        }

        self.flush_lifecycle(world);

        for e in stale {
            self.instances.detach(e);
        }

        errors
    }

    /// Drain pending spawn / despawn queues, dispatching the matching
    /// lifecycle event into each instance.
    fn flush_lifecycle(&mut self, world: &mut World) {
        // OnDespawn first — handlers see the instance's state, and its
        // entity's **transform** as of the last sync (`last_seen`), before
        // the instance is dropped. Not the rest of the entity: it has
        // usually left the world already, so a custom field or a velocity
        // read here answers zero. Put anything a death handler needs into
        // the graph as a literal.
        let despawned = std::mem::take(&mut self.pending_despawn);
        for e in despawned {
            if let Some(idx) = self.instances.index_of(e) {
                self.dispatch_into_state(idx, world, TriggerKind::OnDespawn, &EventPayload::Lifecycle);
            }
        }
        let spawned = std::mem::take(&mut self.pending_spawn);
        for e in spawned {
            if let Some(idx) = self.instances.index_of(e) {
                self.dispatch_into_state(idx, world, TriggerKind::OnSpawn, &EventPayload::Lifecycle);
            }
        }
    }

    /// Attach `entity` to a graph compiled directly from `json` (no
    /// disk I/O). Mostly for tests; production scenes go through
    /// `sync_instances`.
    ///
    /// Note: this does NOT fire OnSpawn — callers that want spawn
    /// semantics should follow up with [`Self::dispatch_spawn`].
    pub fn attach_inline(
        &mut self,
        entity: hecs::Entity,
        key: PathBuf,
        json: &str,
    ) -> Result<GraphId, BehaviorCacheError> {
        let id = self.cache.load_from_json_with_key(key, json)?;
        let layout = self.cache.get(id).state_layout;
        self.instances.attach(entity, id, &layout);
        Ok(id)
    }

    /// Phase 2 of the per-frame pipeline. Run after `update_input_actions`
    /// and before native motion + scripts.
    pub fn tick_pre_simulation(&mut self, world: &mut World, dt: f32) {
        // Reset per-frame counters; preserve cache + instances.
        self.counters = BehaviorCounters::default();
        self.overbudget_warned_this_frame = false;
        // A scene with no graphs has nothing to time: measured anyway, an OS
        // stall during the empty loop was reported as graphs over budget
        // ("0 entities, 0 ops") in scenes that have none.
        if self.instances.instances.is_empty() {
            return;
        }
        let started = MonoTimer::now();

        // Iterate by graph bucket for cache locality (spec §4.5).
        // Take a snapshot of (graph_id, instance_index) pairs so we
        // can borrow `world` mutably inside the loop.
        let plan: Vec<(GraphId, Vec<usize>)> = self
            .instances
            .by_graph
            .iter()
            .map(|(id, indices)| (*id, indices.clone()))
            .collect();

        for (graph_id, indices) in plan {
            let graph = self.cache.get(graph_id).clone();
            if !graph.subscriptions.contains(TriggerKind::OnTick) {
                continue;
            }
            for idx in indices {
                let state = &mut self.instances.instances[idx];
                let mut ctx = BehaviorExecContext {
                    dt,
                    pins: &mut self.pins,
                    counters: &mut self.counters,
                    commands: &mut self.commands,
                };
                tick_event(
                    &graph,
                    state,
                    world,
                    &mut ctx,
                    TriggerKind::OnTick,
                    &EventPayload::Tick { dt },
                );
            }
        }

        self.advance_timers(world, dt);

        let elapsed_us = started.elapsed_us();
        self.counters.tick_time_us = elapsed_us;
        if self.tick_budget_ms > 0.0 {
            let budget_us = (self.tick_budget_ms * 1000.0) as u64;
            if elapsed_us > budget_us {
                let over = elapsed_us - budget_us;
                self.counters.tick_overbudget_us = over;
                // Nothing ran: the time was the process being descheduled,
                // not a graph, so there is nothing to warn about.
                let ran = self.counters.instances_ticked > 0 || self.counters.ops_executed > 0;
                if ran && !self.overbudget_warned_this_frame {
                    self.overbudget_warned_this_frame = true;
                    warn!(
                        "[behavior] OnTick over budget: {:.3} ms (budget {:.3} ms, {} entities, {} ops)",
                        elapsed_us as f32 / 1000.0,
                        self.tick_budget_ms,
                        self.counters.instances_ticked,
                        self.counters.ops_executed,
                    );
                }
            }
        }
    }

    // ── Phase 6 dispatch hooks (spec §7.3) ──

    /// Manually queue an OnSpawn event for `entity`. Called by
    /// callers that bypass `sync_instances` (e.g. inline test
    /// fixtures, scripted spawns).
    pub fn dispatch_spawn(&mut self, entity: hecs::Entity, world: &mut World) {
        self.dispatch_lifecycle(entity, world, TriggerKind::OnSpawn);
    }

    /// Manually queue an OnDespawn event for `entity`. Fires before
    /// `detach` so the handler still sees the instance.
    pub fn dispatch_despawn(&mut self, entity: hecs::Entity, world: &mut World) {
        self.dispatch_lifecycle(entity, world, TriggerKind::OnDespawn);
    }

    fn dispatch_lifecycle(
        &mut self,
        entity: hecs::Entity,
        world: &mut World,
        kind: TriggerKind,
    ) {
        let Some(idx) = self.instances.index_of(entity) else {
            return;
        };
        self.dispatch_into_state(idx, world, kind, &EventPayload::Lifecycle);
    }

    /// OnCollision dispatch — the collision system calls this for the
    /// `self_entity` side of each contact pair (caller invokes twice
    /// for the symmetric case if both entities have graphs).
    pub fn dispatch_collision(
        &mut self,
        self_entity: hecs::Entity,
        other: hecs::Entity,
        enter: bool,
        world: &mut World,
    ) {
        let Some(idx) = self.instances.index_of(self_entity) else {
            return;
        };
        self.dispatch_into_state(
            idx,
            world,
            TriggerKind::OnCollision,
            &EventPayload::Collision { other, enter },
        );
    }

    /// OnTrigger dispatch — trigger-volume overlap events.
    pub fn dispatch_trigger(
        &mut self,
        self_entity: hecs::Entity,
        other: hecs::Entity,
        world: &mut World,
    ) {
        let Some(idx) = self.instances.index_of(self_entity) else {
            return;
        };
        self.dispatch_into_state(
            idx,
            world,
            TriggerKind::OnTrigger,
            &EventPayload::TriggerOverlap { other },
        );
    }

    /// Advance every `OnTimer` instance's clock by `dt` and fire the ones
    /// whose interval elapsed. There used to be no clock at all - the doc
    /// here spoke of "the timer queue", which did not exist, and
    /// `dispatch_timer` had no caller, so `OnTimer` never fired in any
    /// runtime (test-lab BUGS #93). Part of `tick_pre_simulation`, so every
    /// runtime gets it without a call of its own.
    ///
    /// A long frame fires at most [`MAX_TIMER_CATCH_UP`] times, so a stall
    /// does not replay a burst of queued firings.
    fn advance_timers(&mut self, world: &mut World, dt: f32) {
        let mut firings: Vec<(usize, f32)> = Vec::new();
        for (idx, state) in self.instances.instances.iter_mut().enumerate() {
            let graph = self.cache.get(state.graph);
            let Some(interval) = graph
                .programs
                .get(&TriggerKind::OnTimer)
                .and_then(|p| p.timer_interval)
            else {
                continue;
            };
            state.timer_elapsed += dt;
            let mut fired = 0;
            while state.timer_elapsed >= interval {
                state.timer_elapsed -= interval;
                if fired < MAX_TIMER_CATCH_UP {
                    state.timer_ticks += 1;
                    firings.push((idx, state.timer_ticks as f32));
                    fired += 1;
                }
            }
        }
        for (idx, tick_index) in firings {
            self.dispatch_into_state(
                idx,
                world,
                TriggerKind::OnTimer,
                &EventPayload::Timer { tick_index },
            );
        }
    }

    /// Fire `OnKey` and `OnMouseClick` for this frame's input. The one call
    /// a runtime makes for graph input, once per frame before the command
    /// barrier.
    ///
    /// The click position handed to the graph is converted to world space
    /// through the scene's active camera and `input.screen_size` (which every
    /// runtime fills each frame), with the same arithmetic as
    /// `get_mouse_world_position`.
    ///
    /// Before this, no runtime called `dispatch_input_keys` or
    /// `dispatch_mouse_click`, so neither trigger ever fired (BUGS #93).
    pub fn dispatch_input(
        &mut self,
        input: &crate::input::InputState,
        world: &mut World,
    ) {
        self.dispatch_input_keys(input, world);
        if input.mouse_left_just_pressed || input.mouse_right_just_pressed {
            let pos = mouse_world_position(world, input.mouse_position, input.screen_size);
            if input.mouse_left_just_pressed {
                self.dispatch_mouse_click(pos, 0, world);
            }
            if input.mouse_right_just_pressed {
                self.dispatch_mouse_click(pos, 1, world);
            }
        }
    }

    /// OnTimer dispatch for one instance. [`advance_timers`](Self::advance_timers)
    /// is the normal caller; this is public for tests and hosts that drive
    /// time themselves.
    pub fn dispatch_timer(
        &mut self,
        self_entity: hecs::Entity,
        tick_index: f32,
        world: &mut World,
    ) {
        let Some(idx) = self.instances.index_of(self_entity) else {
            return;
        };
        self.dispatch_into_state(
            idx,
            world,
            TriggerKind::OnTimer,
            &EventPayload::Timer { tick_index },
        );
    }

    /// OnKey dispatch — walks every instance whose graph subscribes
    /// to `OnKey` and fires the program if the per-frame `InputState`
    /// matches the program's static `(key, edge)` filter (compiled
    /// into [`crate::behavior::compile::EventProgram::key_filter`]).
    ///
    /// Cost contract: graphs without an `OnKey` trigger node never
    /// appear in this loop's hot path — the early `subscriptions`
    /// check skips them. Graphs with one `OnKey` node pay one
    /// `HashSet::contains` lookup per frame for `Pressed` /
    /// `Released` (they fire only on the matching transition) or
    /// per-frame for `Held`.
    ///
    /// Call site: the runtime invokes this once per frame in Phase 6
    /// (event dispatch), after `update_input_actions` and before
    /// `flush_commands`. It's safe to call with no live graphs (the
    /// instance loop is a no-op).
    pub fn dispatch_input_keys(
        &mut self,
        input: &crate::input::InputState,
        world: &mut World,
    ) {
        use crate::behavior::event::KeyEdge;
        // Snapshot (instance_index, key_filter) pairs so we can
        // mutate `self` inside the dispatch loop without holding a
        // borrow on `self.instances`.
        let mut firings: Vec<(usize, KeyFilter)> = Vec::new();
        for (idx, state) in self.instances.instances.iter().enumerate() {
            let graph = self.cache.get(state.graph);
            if !graph.subscriptions.contains(TriggerKind::OnKey) {
                continue;
            }
            let Some(prog) = graph.programs.get(&TriggerKind::OnKey) else {
                continue;
            };
            let Some(filter) = prog.key_filter else {
                // Defensive: an OnKey program should always have a
                // filter (compiler enforces). Skip rather than panic.
                continue;
            };
            let fires = match filter.edge {
                KeyEdge::Pressed => input.is_just_pressed(filter.key),
                KeyEdge::Released => input.is_just_released(filter.key),
                KeyEdge::Held => input.is_pressed(filter.key),
            };
            if fires {
                firings.push((idx, filter));
            }
        }
        for (idx, filter) in firings {
            self.dispatch_into_state(
                idx,
                world,
                TriggerKind::OnKey,
                &EventPayload::Key {
                    key: filter.key,
                    edge: filter.edge,
                },
            );
        }
    }

    /// OnEvent dispatch — fires every instance whose graph subscribes
    /// to `OnEvent` *and* whose program's `name_filter` equals `name`.
    /// `value` is written into the program's payload pin.
    /// Deliver a **script's** `send_event` to every graph listening for it.
    ///
    /// Each runtime calls this for the events its scripts sent this frame,
    /// before the barrier, so a handler's commands flush with the rest.
    /// Before Sep 2026 nothing did: `dispatch_event` was called only from
    /// inside this system, for events *graphs* emitted, so a wave director
    /// written in Lua had no way to tell a graph-driven enemy anything.
    ///
    /// **Do not pass events that came from graphs.** Those already reached
    /// every `OnEvent` at the barrier; a runtime that forwards them to its
    /// scripts and then hands its script events back here delivers each one
    /// to graphs twice.
    pub fn deliver_script_event(
        &mut self,
        name: &str,
        data: Option<&serde_json::Value>,
        world: &mut World,
    ) {
        let value = super::command::graph_value_from_script_data(data);
        self.dispatch_event(name, value, world);
    }

    pub fn dispatch_event(&mut self, name: &str, value: f32, world: &mut World) {
        let mut firings: Vec<usize> = Vec::new();
        for (idx, state) in self.instances.instances.iter().enumerate() {
            let graph = self.cache.get(state.graph);
            if !graph.subscriptions.contains(TriggerKind::OnEvent) {
                continue;
            }
            let Some(prog) = graph.programs.get(&TriggerKind::OnEvent) else {
                continue;
            };
            let Some(filter) = prog.name_filter.as_deref() else {
                continue;
            };
            if filter == name {
                firings.push(idx);
            }
        }
        for idx in firings {
            self.dispatch_into_state(
                idx,
                world,
                TriggerKind::OnEvent,
                &EventPayload::Event { value },
            );
        }
    }

    /// OnMouseClick dispatch — fires every instance whose graph
    /// subscribes to `OnMouseClick` and whose `mouse_filter` matches
    /// `button` (0 = left, 1 = right). `pos` is written into the
    /// program's mouse-position payload pin.
    pub fn dispatch_mouse_click(
        &mut self,
        pos: glam::Vec2,
        button: u8,
        world: &mut World,
    ) {
        let mut firings: Vec<usize> = Vec::new();
        for (idx, state) in self.instances.instances.iter().enumerate() {
            let graph = self.cache.get(state.graph);
            if !graph.subscriptions.contains(TriggerKind::OnMouseClick) {
                continue;
            }
            let Some(prog) = graph.programs.get(&TriggerKind::OnMouseClick) else {
                continue;
            };
            let Some(filter) = prog.mouse_filter else {
                continue;
            };
            if filter.matches(button) {
                firings.push(idx);
            }
        }
        for idx in firings {
            self.dispatch_into_state(
                idx,
                world,
                TriggerKind::OnMouseClick,
                &EventPayload::MouseClick { pos, button },
            );
        }
    }

    fn dispatch_into_state(
        &mut self,
        idx: usize,
        world: &mut World,
        kind: TriggerKind,
        payload: &EventPayload,
    ) {
        let state = &mut self.instances.instances[idx];
        let graph = self.cache.get(state.graph);
        if !graph.subscriptions.contains(kind) {
            return;
        }
        let started = MonoTimer::now();
        let mut ctx = BehaviorExecContext {
            dt: 0.0,
            pins: &mut self.pins,
            counters: &mut self.counters,
            commands: &mut self.commands,
        };
        tick_event(graph, state, world, &mut ctx, kind, payload);
        let elapsed_us = started.elapsed_us();
        self.counters.event_time_us =
            self.counters.event_time_us.saturating_add(elapsed_us);
        if self.event_budget_ms > 0.0 {
            let budget_us = (self.event_budget_ms * 1000.0) as u64;
            if self.counters.event_time_us > budget_us {
                let over = self.counters.event_time_us - budget_us;
                self.counters.event_overbudget_us = over;
                if !self.overbudget_warned_this_frame {
                    self.overbudget_warned_this_frame = true;
                    warn!(
                        "[behavior] event dispatch over budget: {:.3} ms (budget {:.3} ms, {} events)",
                        self.counters.event_time_us as f32 / 1000.0,
                        self.event_budget_ms,
                        self.counters.events_dispatched,
                    );
                }
            }
        }
    }
}

#[derive(Debug, Error)]
#[error("behavior graph load failed for entity {entity:?} at {path}: {source}")]
pub struct BehaviorSyncError {
    pub entity: hecs::Entity,
    pub path: PathBuf,
    #[source]
    pub source: BehaviorCacheError,
}

impl BehaviorGraphSystem {
    /// Phase 7 (command barrier). Drains the structural-command queue
    /// pushed by `SpawnEntity` / `Despawn` / `PlaySound` ops this
    /// frame and applies it to `world`. Returns counters + a list of
    /// `PlaySound` requests for the host to forward to the audio
    /// backend.
    ///
    /// Per spec §4.4 the host should call this **after** behavior
    /// dispatch and **before** the script-bridge command drain so
    /// script commands (priority 2) naturally override the spawned /
    /// despawned entities.
    pub fn flush_commands(&mut self, world: &mut World) -> CommandRouteCounters {
        self.flush_commands_with(world, None)
    }

    fn flush_commands_with(
        &mut self,
        world: &mut World,
        prefabs: Option<&super::command::PrefabSource<'_>>,
    ) -> CommandRouteCounters {
        let drained = std::mem::take(&mut self.commands);
        super::command::route_behavior_commands_with(drained, world, prefabs)
    }

    /// [`flush_commands`](Self::flush_commands), then deliver every
    /// `EmitEvent` it drained to the graphs' `OnEvent` handlers and flush
    /// what those handlers queued - so a cross-graph reaction (scene 18's
    /// collision ping turning the counter box green) lands the same frame as
    /// the contact that caused it.
    ///
    /// This is the Phase 7 barrier every runtime should call, with its
    /// prefabs so a graph's `SpawnEntity` builds the real thing. The
    /// returned `play_sound_requests` and `change_scene_requests` are the
    /// caller's to forward - see `behavior::command`. Only the
    /// native runner used to do the second half, inline; the editor viewport
    /// and web flushed and dropped the drained events, so `OnEvent` never
    /// fired there (test-lab BUGS #92). One level deep, as native was: an
    /// event emitted by an `OnEvent` handler waits for the next frame's
    /// barrier rather than looping here.
    pub fn flush_commands_and_route_events(
        &mut self,
        world: &mut World,
        prefabs: Option<&super::command::PrefabSource<'_>>,
    ) -> CommandRouteCounters {
        let mut counters = self.flush_commands_with(world, prefabs);
        if counters.emit_event_requests.is_empty() {
            return counters;
        }
        for ev in counters.emit_event_requests.clone() {
            self.dispatch_event(&ev.name, ev.value.unwrap_or(0.0), world);
        }
        let second = self.flush_commands_with(world, prefabs);
        counters.spawned += second.spawned;
        counters.despawned += second.despawned;
        counters.despawned_by_tag += second.despawned_by_tag;
        counters.missing_despawn_targets += second.missing_despawn_targets;
        counters.missing_prefabs += second.missing_prefabs;
        counters.play_sound_requests.extend(second.play_sound_requests);
        counters.play_animation_requests.extend(second.play_animation_requests);
        counters.stop_animation_requests.extend(second.stop_animation_requests);
        counters.change_scene_requests.extend(second.change_scene_requests);
        // Deliberately not merged: `second.emit_event_requests` are the
        // next frame's, and nothing re-queues them. Native dropped them too.
        counters
    }

    /// Pending command queue length — used by tests to assert ops
    /// pushed without applying.
    pub fn pending_command_count(&self) -> usize {
        self.commands.len()
    }

    /// Spec §6: copy this frame's behavior counters into the
    /// host-owned `FrameCounters` struct. Call once per frame after
    /// `flush_commands` (Phase 7) so all telemetry is final.
    /// Counters are *added* into the existing fields so callers can
    /// safely fold multiple subsystems into the same `FrameCounters`.
    pub fn snapshot_into_frame_counters(&self, fc: &mut FrameCounters) {
        let c = &self.counters;
        fc.behavior_entities_ticked =
            fc.behavior_entities_ticked.saturating_add(c.instances_ticked as u64);
        fc.behavior_ops_executed =
            fc.behavior_ops_executed.saturating_add(c.ops_executed as u64);
        fc.behavior_commands_emitted =
            fc.behavior_commands_emitted.saturating_add(c.commands_emitted as u64);
        fc.behavior_overbudget_ms += c.tick_overbudget_us as f32 / 1000.0;
        fc.behavior_critical_overbudget_ms += c.event_overbudget_us as f32 / 1000.0;
    }

    /// Slice 7.6c — convenience wrapper for the host's Phase 6
    /// trigger-volume integration. Walks every `TriggerOverlaps`
    /// component and dispatches `OnTrigger` once per *newly-entered*
    /// other entity. Call after `update_trigger_areas` and before
    /// `flush_commands` so any spawned/despawned commands queued by
    /// trigger handlers land in this frame's barrier.
    ///
    /// Returns the number of dispatch calls made (one per
    /// (trigger, entered) pair, regardless of whether the trigger
    /// entity actually subscribed — `dispatch_trigger` is itself a
    /// no-op when no instance matches).
    pub fn dispatch_trigger_overlaps_from_world(&mut self, world: &mut World) -> usize {
        use crate::ecs::components::TriggerOverlaps;
        // Snapshot (trigger, entered_bits) so we can mutate `self`
        // while iterating without holding a `World` query borrow.
        let mut pairs: Vec<(hecs::Entity, Vec<u64>)> = Vec::new();
        for (entity, overlaps) in world.query::<&TriggerOverlaps>().iter() {
            if overlaps.entered.is_empty() {
                continue;
            }
            pairs.push((entity, overlaps.entered.clone()));
        }
        let mut dispatched = 0usize;
        for (trigger, entered) in pairs {
            for bits in entered {
                if bits == 0 {
                    continue;
                }
                let Some(other) = hecs::Entity::from_bits(bits) else {
                    continue;
                };
                self.dispatch_trigger(trigger, other, world);
                dispatched += 1;
            }
        }
        dispatched
    }

    /// Slice 7.6c — convenience wrapper for the host's Phase 6
    /// collision integration. Walks the per-frame contact list
    /// produced by the physics step and dispatches `OnCollision` to
    /// both sides of each pair (Started → `enter = true`, Stopped →
    /// `enter = false`). Returns the number of dispatch calls made.
    ///
    /// `PhysicsWorld` keys bodies by `Entity::id()` - the bare index, no
    /// generation - so that is what a real `ContactEvent` carries. This used
    /// to decode it with `Entity::from_bits`, which rejects a zero
    /// generation: every contact from a real physics step was dropped, and
    /// `OnCollision` never fired in any runtime (test-lab BUGS #92). The
    /// tests built their events with `to_bits()`, which the physics layer
    /// never produces. Both encodings keep the index in the low 32 bits, so
    /// resolve by index against the live world and accept either.
    pub fn dispatch_contact_events(
        &mut self,
        events: &[crate::physics::ContactEvent],
        world: &mut World,
    ) -> usize {
        if events.is_empty() {
            return 0;
        }
        let live: HashMap<u32, hecs::Entity> =
            world.iter().map(|e| (e.entity().id(), e.entity())).collect();
        let resolve = |bits: u64| live.get(&(bits as u32)).copied();
        let mut dispatched = 0usize;
        for ev in events {
            let enter = matches!(ev.event_type, crate::physics::ContactType::Started);
            for (self_bits, other_bits) in
                [(ev.entity_a, ev.entity_b), (ev.entity_b, ev.entity_a)]
            {
                let (Some(self_e), Some(other_e)) = (resolve(self_bits), resolve(other_bits))
                else {
                    continue;
                };
                self.dispatch_collision(self_e, other_e, enter, world);
                dispatched += 1;
            }
        }
        dispatched
    }
}
