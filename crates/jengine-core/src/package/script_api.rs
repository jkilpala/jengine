//! Script API registry — dynamic registration of script-facing functions.
//!
//! Packages register functions here at startup. During `ScriptEngine`
//! initialization, the registry is iterated to create Lua/JS globals
//! dynamically, keeping the script backends decoupled from specific APIs.
//!
//! ## Slice A — live handlers
//!
//! Descriptors may carry an optional [`ScriptHandler`] closure. When present,
//! the language adapters install the function as a global; calls from scripts
//! are marshalled to/from the bridge [`Value`] type and dispatched into the
//! handler. Descriptors without a handler remain pure metadata used for docs
//! and editor tooltips (legacy behaviour).
//!
//! Handlers receive a [`ScriptCallCtx`] which exposes the per-frame
//! [`BridgeReadSnapshot`] and a `&mut Vec<BridgeCommand>` for queueing
//! deferred world mutations. The most useful command for package authors is
//! [`crate::scripting::bridge::command::BridgeCommand::SetComponentField`],
//! which applies a single-field update to any component registered via
//! [`crate::package::component_registry::ComponentRegistry`].

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::scripting::bridge::command::BridgeCommand;
use crate::scripting::bridge::context::BridgeReadSnapshot;
use crate::scripting::bridge::diagnostics::ScriptError;
use crate::scripting::bridge::value::Value;

// ──────────────────────────── Script Value Types ────────────────────────────

/// Type descriptor for script function arguments and return values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptValueType {
    Nil,
    Bool,
    Int,
    Float,
    String,
    /// A pair of floats (e.g. Vec2 returned as (f32, f32) in Lua, [f32, f32] in JS).
    Vec2,
    /// Four floats, a colour's `r, g, b, a` - four return values in Lua, an
    /// array of four in JS.
    Rgba,
    /// Entity ID (u64).
    EntityId,
    /// Variable-length argument list.
    VarArgs,
    /// Any scriptable value — for arguments whose type depends on another
    /// argument, like `set_ui_prop`'s value (a string, number, flag or color
    /// depending on the property named).
    Any,
}

impl fmt::Display for ScriptValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nil => write!(f, "nil"),
            Self::Bool => write!(f, "bool"),
            Self::Int => write!(f, "int"),
            Self::Float => write!(f, "float"),
            Self::String => write!(f, "string"),
            Self::Vec2 => write!(f, "vec2"),
            Self::Rgba => write!(f, "rgba"),
            Self::EntityId => write!(f, "entity_id"),
            Self::VarArgs => write!(f, "..."),
            Self::Any => write!(f, "any"),
        }
    }
}

// ──────────────────────────── Script Function Descriptor ────────────────────────────

/// Per-call context handed to a [`ScriptHandler`]. Read access goes through
/// `snapshot`; mutations are queued onto `commands` and applied by the engine
/// after the script tick completes.
pub struct ScriptCallCtx<'a> {
    /// Read-only world snapshot, valid for the duration of this call.
    pub snapshot: &'a BridgeReadSnapshot,
    /// Deferred command queue. Push [`BridgeCommand`] values here to mutate
    /// the world. The most general command for custom components is
    /// [`BridgeCommand::SetComponentField`].
    pub commands: &'a mut Vec<BridgeCommand>,
}

/// Handler closure invoked when a script calls into a registry-backed
/// function. Adapters convert their native call args into [`Value`]s in the
/// order declared by [`ScriptFunctionDescriptor::args`], dispatch through
/// the handler, then convert the returned `Value` back to the native
/// representation. A [`Value::Nil`] return maps to no return value.
pub type ScriptHandler = Arc<
    dyn for<'a> Fn(&mut ScriptCallCtx<'a>, &[Value]) -> Result<Value, ScriptError>
        + Send
        + Sync,
>;

/// Describes a single script-facing function registered by a package.
///
/// Used for documentation and for the Lua/JS backends to know what
/// globals to create and how to marshall arguments. When `handler` is
/// `Some`, adapters install a real callable global that dispatches into the
/// closure; when `None`, the descriptor is metadata only.
pub struct ScriptFunctionDescriptor {
    /// Global function name as seen in scripts (e.g. "apply_force").
    pub name: String,

    /// Which package registered this function.
    pub package: String,

    /// Description for auto-generated docs / editor tooltips.
    pub description: String,

    /// Argument signatures.
    pub args: Vec<ScriptArg>,

    /// Return type.
    pub returns: ScriptValueType,

    /// Optional Rust closure invoked when scripts call this function.
    /// `None` keeps the legacy "metadata-only" behaviour.
    pub handler: Option<ScriptHandler>,
}

/// A single argument in a script function signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptArg {
    /// Argument name (for documentation).
    pub name: String,
    /// Argument type.
    pub arg_type: ScriptValueType,
}

// ──────────────────────────── Script API Registry ────────────────────────────

/// Registry of all script-facing functions.
///
/// Populated during engine initialization by core and packages,
/// then consumed by the Lua/JS backends to create global functions.
pub struct ScriptApiRegistry {
    functions: Vec<ScriptFunctionDescriptor>,
}

impl ScriptApiRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
        }
    }

    /// Register a script function descriptor.
    pub fn register(&mut self, descriptor: ScriptFunctionDescriptor) {
        log::trace!(
            "Script API registered: {}() from package '{}'",
            descriptor.name,
            descriptor.package
        );
        self.functions.push(descriptor);
    }

    /// Convenience builder for registering a metadata-only function (no live
    /// handler). Equivalent to building the descriptor manually with
    /// `handler: None`.
    pub fn add_function(
        &mut self,
        name: &str,
        package: &str,
        description: &str,
        args: Vec<ScriptArg>,
        returns: ScriptValueType,
    ) {
        self.register(ScriptFunctionDescriptor {
            name: name.to_string(),
            package: package.to_string(),
            description: description.to_string(),
            args,
            returns,
            handler: None,
        });
    }

    /// Convenience builder for registering a function with a live handler.
    /// Adapters install this as a callable global; metadata-only descriptors
    /// stay docs-only.
    pub fn add_function_with_handler<F>(
        &mut self,
        name: &str,
        package: &str,
        description: &str,
        args: Vec<ScriptArg>,
        returns: ScriptValueType,
        handler: F,
    ) where
        F: for<'a> Fn(&mut ScriptCallCtx<'a>, &[Value]) -> Result<Value, ScriptError>
            + Send
            + Sync
            + 'static,
    {
        self.register(ScriptFunctionDescriptor {
            name: name.to_string(),
            package: package.to_string(),
            description: description.to_string(),
            args,
            returns,
            handler: Some(Arc::new(handler)),
        });
    }

    /// Iterate over all registered function descriptors.
    pub fn iter(&self) -> impl Iterator<Item = &ScriptFunctionDescriptor> {
        self.functions.iter()
    }

    /// Get all function names.
    pub fn function_names(&self) -> Vec<&str> {
        self.functions.iter().map(|f| f.name.as_str()).collect()
    }

    /// Get functions registered by a specific package.
    pub fn by_package(&self, package: &str) -> Vec<&ScriptFunctionDescriptor> {
        self.functions
            .iter()
            .filter(|f| f.package == package)
            .collect()
    }

    /// Number of registered functions.
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }
}

impl Default for ScriptApiRegistry {
    fn default() -> Self {
        Self::new()
    }
}
