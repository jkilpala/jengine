//! Script Bridge API — language-neutral scripting contract.
//!
//! This module is the **stable boundary** between the engine and any scripting backend
//! (Luau and JS today; the contract stays language-neutral so additional adapters can be
//! added without changing the engine). Adapters depend on this module; the engine
//! consumes commands produced by adapters through this module.
//!
//! ## Layers
//! 1. **Engine Core API** — Rust internals (ECS, assets, runtime). Not exposed here.
//! 2. **Script Bridge API** — this module. Neutral values, commands, lifecycle, diagnostics.
//! 3. **Backend Adapters** — implement [`adapter::ScriptAdapter`] using this module.
//!
//! See `docs/archive/scripting-architecture-plan.md` for the full design.
//!
//! ## Status
//! Phase 1 — skeleton only. Types compile but are not yet wired into [`crate::ecs::systems::ScriptSystem`].

#![allow(dead_code)] // Phase 1: skeleton — wired up in Phase 2.

pub mod adapter;
pub mod adapters;
pub mod capability;
pub mod command;
pub mod context;
pub mod diagnostics;
pub mod metadata;
pub mod module;
pub mod runner;
pub mod snapshot;
pub mod translator;
pub mod value;

#[cfg(any(feature = "web", feature = "js-adapter-host"))]
pub use adapters::js::transpile_lua_to_js;
#[cfg(any(feature = "web", feature = "js-adapter-host"))]
pub use adapters::JsAdapter;
#[cfg(feature = "native")]
pub use adapters::LuauAdapter;
pub use adapter::{NoopAdapter, ScriptAdapter, ScriptHandle};
pub use capability::{Capabilities, Capability};
pub use command::BridgeCommand;
pub use context::{BridgeContext, BridgeReadSnapshot};
pub use diagnostics::{BackendKind, ScriptError, ScriptErrorKind, SourceLocation};
pub use module::{null_resolver, ModuleResolver};
#[cfg(not(target_arch = "wasm32"))]
pub use module::fs_resolver;
pub use runner::ScriptBridgeRunner;
pub use runner::{BudgetConfig, BudgetPolicy};
pub use snapshot::{
    snapshot_contacts, snapshot_custom_components, snapshot_custom_components_borrow,
    snapshot_gui_props, snapshot_input, snapshot_tilemaps, snapshot_world, snapshot_world_borrow, snapshot_world_borrow_into,
};
#[cfg(feature = "physics")]
pub use snapshot::snapshot_physics;
pub use snapshot::{snapshot_characters, snapshot_velocities};
pub use translator::{to_script_command, to_script_command_opt, to_script_commands};
pub use value::{HandleKind, Value};
