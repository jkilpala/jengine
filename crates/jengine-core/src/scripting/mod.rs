//! Scripting system — bridge-based backend for Lua/Luau/JavaScript.
//!
//! P7.5d: legacy `lua_backend` / `js_backend` / `ScriptEngine` trait have
//! been deleted. All native script execution now flows through
//! [`bridge::ScriptBridgeRunner`] and the per-language adapters
//! ([`bridge::LuauAdapter`], [`bridge::JsAdapter`]).

pub mod api;
pub mod bridge;
pub mod host_commands;
pub mod intern;
pub mod namespaces;
pub mod persistent;
pub mod script_math;
pub mod spawn_aliases;
pub mod spawn_map;
pub mod timers;

// Batch scripts - JS and Lua alike - run only through `worker_pool`, whose
// backends always hold a QuickJS runtime, and are dispatched only under
// `js-adapter-host` (`ecs::behaviors::run_batch_scripts`). So the whole family
// sits behind it: that keeps `rquickjs`, which has no prebuilt bindings for
// Android or iOS, out of a build that leaves the feature off.
#[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host", feature = "native"))]
pub mod batch;

#[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host", feature = "native"))]
pub mod batch_lua;

#[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host", feature = "native"))]
pub mod worker_pool;

#[cfg(all(not(target_arch = "wasm32"), any(feature = "js-adapter-host", feature = "native")))]
pub mod prng;

/// Opaque script identifier kept for legacy callers (scene format etc.).
/// Not used by the bridge runner, which uses [`bridge::ScriptHandle`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScriptId(pub u64);
