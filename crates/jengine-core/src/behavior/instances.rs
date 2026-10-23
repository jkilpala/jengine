//! Per-entity behavior-graph instance store.
//!
//! Slice 7.2 layout matches spec §4.5: instances are grouped by
//! `GraphId` so the per-frame loop walks one compiled program with hot
//! op-fn pointers, then iterates that graph's instances. Future
//! parallelization runs whole graph buckets in parallel without any
//! data shuffling.

use std::collections::HashMap;

use crate::behavior::cache::GraphId;
use crate::behavior::state::{GraphInstanceState, StateLayout};

#[derive(Debug, Default)]
pub struct BehaviorInstances {
    pub instances: Vec<GraphInstanceState>,
    /// `graph_id → indices into `instances`.
    pub by_graph: HashMap<GraphId, Vec<usize>>,
    /// `entity → index into `instances` for O(1) cleanup on despawn.
    by_entity: HashMap<hecs::Entity, usize>,
}

impl BehaviorInstances {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.instances.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    pub fn contains(&self, entity: hecs::Entity) -> bool {
        self.by_entity.contains_key(&entity)
    }

    pub fn index_of(&self, entity: hecs::Entity) -> Option<usize> {
        self.by_entity.get(&entity).copied()
    }

    /// Attach `entity` to `graph`. No-op if already attached to the
    /// same graph; replaces the binding if attached to a different
    /// graph.
    pub fn attach(&mut self, entity: hecs::Entity, graph: GraphId, layout: &StateLayout) {
        if let Some(idx) = self.by_entity.get(&entity).copied() {
            if self.instances[idx].graph == graph {
                return;
            }
            self.detach_index(idx);
        }
        let idx = self.instances.len();
        self.instances
            .push(GraphInstanceState::new(graph, entity, layout));
        self.by_graph.entry(graph).or_default().push(idx);
        self.by_entity.insert(entity, idx);
    }

    /// Drop the instance for `entity`, if any. Used by the system
    /// when entities despawn or lose their `BehaviorGraphComponent`.
    pub fn detach(&mut self, entity: hecs::Entity) {
        if let Some(idx) = self.by_entity.get(&entity).copied() {
            self.detach_index(idx);
        }
    }

    fn detach_index(&mut self, idx: usize) {
        let graph = self.instances[idx].graph;
        let entity = self.instances[idx].entity;
        // swap_remove keeps `instances` dense; fix up any indices that
        // pointed at the moved tail.
        let last = self.instances.len() - 1;
        self.instances.swap_remove(idx);
        self.by_entity.remove(&entity);
        // Drop `idx` from the graph bucket and re-point `last → idx` if a swap happened.
        if let Some(bucket) = self.by_graph.get_mut(&graph) {
            if let Some(pos) = bucket.iter().position(|&v| v == idx) {
                bucket.swap_remove(pos);
            }
            if bucket.is_empty() {
                self.by_graph.remove(&graph);
            }
        }
        if idx != last {
            // The instance previously at `last` is now at `idx`. Fix up
            // its bucket entry and entity map.
            let moved = &self.instances[idx];
            if let Some(bucket) = self.by_graph.get_mut(&moved.graph)
                && let Some(pos) = bucket.iter().position(|&v| v == last)
            {
                bucket[pos] = idx;
            }
            self.by_entity.insert(moved.entity, idx);
        }
    }
}
