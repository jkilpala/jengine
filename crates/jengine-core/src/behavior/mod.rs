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

// The commands a graph pushes and the plain types they come back as are
// every build's: prefab spawning shares `spawn_entity_data`, and the hosts'
// barrier reads `CommandRouteCounters`. The graphs themselves are the
// `behavior` engine feature; without it `system` is a stand-in that runs none
// (`system_off.rs`), so every host's frame is the same code in every build.
pub mod command;
#[cfg(feature = "behavior")]
pub mod cache;
#[cfg(feature = "behavior")]
pub mod catalog;
#[cfg(feature = "behavior")]
pub mod compile;
#[cfg(feature = "behavior")]
pub mod event;
#[cfg(feature = "behavior")]
pub mod format;
#[cfg(feature = "behavior")]
pub mod instances;
#[cfg(feature = "behavior")]
pub mod interpreter;
#[cfg(feature = "behavior")]
pub mod pin;
#[cfg(feature = "behavior")]
pub mod state;
#[cfg(feature = "behavior")]
pub mod system;
#[cfg(not(feature = "behavior"))]
#[path = "system_off.rs"]
pub mod system;

#[cfg(feature = "behavior")]
pub use cache::{BehaviorCacheError, BehaviorGraphCache, GraphId};
#[cfg(feature = "behavior")]
pub use catalog::{
    op_catalog, DefaultValue, EffectKind, OpDescriptor, ParamDescriptor, PinDescriptor, PinType,
};
pub use command::{
    graph_value_from_script_data, route_behavior_commands, route_behavior_commands_with,
    spawn_entity_data, BehaviorCommand, CommandRouteCounters, EmitEventOutcome,
    PlaySoundOutcome, PrefabSource,
};
#[cfg(feature = "behavior")]
pub use compile::{
    compile, BoolInput, BoundsLiteral, CompareOp, CompileDiagnostic, CompileError,
    CompileWarning, CompiledGraph, EventProgram, FloatInput, KeyFilter, OpInstruction,
    PayloadPins, Vec2Input,
};
#[cfg(feature = "behavior")]
pub use event::{EventMask, EventPayload, KeyEdge, TriggerKind};
#[cfg(feature = "behavior")]
pub use format::{
    BehaviorAsset, BehaviorEdge, BehaviorFormatError, BehaviorNode, ControlPort, EditorMetadata,
    EditorNodePosition, PinBinding, BEHAVIOR_SCHEMA_VERSION,
};
#[cfg(feature = "behavior")]
pub use instances::BehaviorInstances;
#[cfg(feature = "behavior")]
pub use interpreter::{tick_event, tick_graph, BehaviorCounters, BehaviorExecContext};
#[cfg(feature = "behavior")]
pub use pin::{BoolSlot, EntitySlot, FloatSlot, PinFrame, PinLayout, Vec2Slot};
#[cfg(feature = "behavior")]
pub use state::{GraphInstanceState, StateBoolSlot, StateCounterSlot, StateFloatSlot, StateLayout};
pub use system::{BehaviorGraphSystem, BehaviorSyncError};

