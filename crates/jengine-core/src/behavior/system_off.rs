//! The behaviour-graph system in a build without behaviour graphs
//! (`docs/engine-features-plan.md`).
//!
//! Every host calls the graph system at fixed points in its frame - sync,
//! the pre-simulation tick, contacts, triggers, input, the barrier - and
//! `frame_order_is_the_same_everywhere` holds those calls in place, so a
//! build that leaves graphs out keeps the calls and answers them here with
//! nothing: the hosts' frames are the same code in every build. A scene that
//! does name a graph says once that this build has none.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use hecs::World;

use crate::behavior::command::{CommandRouteCounters, PrefabSource};
use crate::ecs::components::BehaviorGraphComponent;

pub use crate::ecs::camera_rig::mouse_world_position;

/// A graph that could not be attached. Never made in a build without
/// graphs; the hosts read its fields when there is one.
#[derive(Debug)]
pub struct BehaviorSyncError {
    pub entity: hecs::Entity,
    pub path: PathBuf,
    pub source: String,
}

/// The graph system with no graphs to run.
#[derive(Debug, Default)]
pub struct BehaviorGraphSystem;

impl BehaviorGraphSystem {
    pub fn new() -> Self {
        Self
    }

    fn say_missing(world: &World) {
        if world.query::<&BehaviorGraphComponent>().iter().next().is_some() {
            crate::features::missing("behavior", "a behaviour graph");
        }
    }

    pub fn sync_instances(&mut self, world: &mut World, _project_root: &Path) -> Vec<BehaviorSyncError> {
        Self::say_missing(world);
        Vec::new()
    }

    pub fn sync_instances_inline(&mut self, world: &mut World, _graphs: &HashMap<String, String>) -> Vec<BehaviorSyncError> {
        Self::say_missing(world);
        Vec::new()
    }

    pub fn tick_pre_simulation(&mut self, _world: &mut World, _dt: f32) {}

    pub fn dispatch_input(&mut self, _input: &crate::input::InputState, _world: &mut World) {}

    pub fn deliver_script_event(&mut self, _name: &str, _data: Option<&serde_json::Value>, _world: &mut World) {}

    pub fn dispatch_trigger_overlaps_from_world(&mut self, _world: &mut World) -> usize {
        0
    }

    pub fn dispatch_contact_events(&mut self, _events: &[crate::physics::ContactEvent], _world: &mut World) -> usize {
        0
    }

    pub fn flush_commands_and_route_events(
        &mut self,
        _world: &mut World,
        _prefabs: Option<&PrefabSource<'_>>,
    ) -> CommandRouteCounters {
        CommandRouteCounters::default()
    }
}
