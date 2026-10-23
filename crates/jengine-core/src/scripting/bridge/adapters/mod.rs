//! Concrete [`super::ScriptAdapter`] implementations.
//!
//! Adapters are independently feature-gated so a build can pull in only the backends it
//! needs.
//! - Phase 3: [`luau`] — native primary backend.
//! - Phase 4: [`js`] — web backend (transpiles Luau → JS internally).

#[cfg(any(feature = "web", feature = "js-adapter-host"))]
pub mod js;
#[cfg(feature = "native")]
pub mod luau;

#[cfg(any(feature = "web", feature = "js-adapter-host"))]
pub use js::JsAdapter;
#[cfg(feature = "native")]
pub use luau::LuauAdapter;
