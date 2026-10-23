//! Per-instance persistent state for stateful behavior ops.
//!
//! Slice 7.4A wires the first stateful gates: `Cooldown` (a per-
//! instance `f32` countdown) and `Once` (a per-instance `bool`
//! flag). Slot newtypes mirror the pin slots in [`crate::behavior::pin`]
//! but index into `GraphInstanceState` storage that *persists* across
//! ticks, where pin slots reset every event.
//!
//! See [`docs/step7-behavior-graph-spec.md`](../../../../docs/step7-behavior-graph-spec.md) §4.3.

use crate::behavior::cache::GraphId;

/// How many persistent state slots an instance of this graph needs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateLayout {
    pub floats: u16,
    pub bools: u16,
    pub counters: u16,
}

/// Index into `GraphInstanceState.floats`. Used by `Cooldown` etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateFloatSlot(pub u16);

/// Index into `GraphInstanceState.bools`. Used by `Once` etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateBoolSlot(pub u16);

/// Index into `GraphInstanceState.counters`. Used by `ForCount` etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateCounterSlot(pub u16);

/// Per-(entity, graph) persistent state. Boxed slices avoid pointer
/// invalidation when the parent `BehaviorInstances` vec grows.
#[derive(Debug, Clone)]
pub struct GraphInstanceState {
    pub graph: GraphId,
    pub entity: hecs::Entity,
    pub floats: Box<[f32]>,
    pub bools: Box<[bool]>,
    pub counters: Box<[u32]>,
    /// Seconds accumulated towards the next `OnTimer` firing.
    pub timer_elapsed: f32,
    /// `OnTimer` firings so far - the trigger's `tick_index` output.
    pub timer_ticks: u32,
    /// Where the entity was the last time the system looked - position,
    /// rotation and scale - so an **`OnDespawn` handler can still ask**.
    ///
    /// `OnDespawn` is dispatched when a sync notices the entity has gone,
    /// which is *after* it has left the world. Without this every transform
    /// read in the handler failed and answered zero: a splitter's halves, a
    /// death burst and a dropped coin all appeared at the origin, while the
    /// comment on `flush_lifecycle` promised handlers the entity "in its
    /// last-known shape". Refreshed on every sync, so it is at most one
    /// frame old.
    pub last_seen: Option<crate::ecs::components::Transform>,
}

impl GraphInstanceState {
    pub fn new(graph: GraphId, entity: hecs::Entity, layout: &StateLayout) -> Self {
        Self {
            graph,
            entity,
            floats: vec![0.0; layout.floats as usize].into_boxed_slice(),
            bools: vec![false; layout.bools as usize].into_boxed_slice(),
            counters: vec![0; layout.counters as usize].into_boxed_slice(),
            timer_elapsed: 0.0,
            timer_ticks: 0,
            last_seen: None,
        }
    }
}
