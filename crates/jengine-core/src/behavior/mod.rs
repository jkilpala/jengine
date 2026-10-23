//! Behavior graph runtime (Step 7).
//!
//! v1 ships in slices — see [`docs/step7-behavior-graph-spec.md`](../../../../docs/step7-behavior-graph-spec.md).
//!
//! - Slice 7.1: `.behavior` JSON asset format ([`format`]).
//! - Slice 7.2 (this commit): typed pin storage ([`pin`]), per-instance
//!   state stub ([`state`]), JSON → typed compiler ([`compile`]),
//!   zero-allocation interpreter ([`interpreter`]), interned graph
//!   cache ([`cache`]), graph-grouped instance store ([`instances`]),
//!   and the [`system::BehaviorGraphSystem`] wiring.
//!
//! Public entry point for hosts: [`system::BehaviorGraphSystem`].

pub mod cache;
pub mod catalog;
pub mod command;
pub mod compile;
pub mod event;
pub mod format;
pub mod instances;
pub mod interpreter;
pub mod pin;
pub mod state;
pub mod system;

pub use cache::{BehaviorCacheError, BehaviorGraphCache, GraphId};
pub use catalog::{
    op_catalog, DefaultValue, EffectKind, OpDescriptor, ParamDescriptor, PinDescriptor, PinType,
};
pub use command::{
    graph_value_from_script_data, route_behavior_commands, route_behavior_commands_with,
    spawn_entity_data, BehaviorCommand, CommandRouteCounters, EmitEventOutcome,
    PlaySoundOutcome, PrefabSource,
};
pub use compile::{
    compile, BoolInput, BoundsLiteral, CompareOp, CompileDiagnostic, CompileError,
    CompileWarning, CompiledGraph, EventProgram, FloatInput, KeyFilter, OpInstruction,
    PayloadPins, Vec2Input,
};
pub use event::{EventMask, EventPayload, KeyEdge, TriggerKind};
pub use format::{
    BehaviorAsset, BehaviorEdge, BehaviorFormatError, BehaviorNode, ControlPort, EditorMetadata,
    EditorNodePosition, PinBinding, BEHAVIOR_SCHEMA_VERSION,
};
pub use instances::BehaviorInstances;
pub use interpreter::{tick_event, tick_graph, BehaviorCounters, BehaviorExecContext};
pub use pin::{BoolSlot, EntitySlot, FloatSlot, PinFrame, PinLayout, Vec2Slot};
pub use state::{GraphInstanceState, StateBoolSlot, StateCounterSlot, StateFloatSlot, StateLayout};
pub use system::{BehaviorGraphSystem, BehaviorSyncError};

