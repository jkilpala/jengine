//! The [`ScriptAdapter`] trait every backend implements.
//!
//! Adapters own:
//! - source loading / compilation / sandboxing,
//! - lifecycle dispatch,
//! - marshalling between [`super::value::Value`] and the native runtime,
//! - diagnostics extraction.
//!
//! Adapters do **not** own:
//! - ECS access (only via [`super::context::BridgeContext`] reads + queued commands),
//! - asset loading,
//! - scheduling.

use super::context::BridgeContext;
use super::diagnostics::ScriptError;

/// Opaque handle the engine uses to refer to a loaded script in a specific adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScriptHandle(pub u64);

/// Unified contract every script backend implements.
pub trait ScriptAdapter {
    /// Load and compile a script from source.
    fn load(&mut self, path: &str, source: &str) -> Result<ScriptHandle, ScriptError>;

    /// Hot-path optimization: called by the runner once per `run_frame`
    /// before the per-entity update loop. Adapters that share frame-constant
    /// state across all entity calls (snapshot pointer, capability set,
    /// counters, etc.) install it here so `update`/`init`/`named_callback`
    /// don't have to rebuild it per entity. Default impl is a no-op so most
    /// adapters need not care.
    fn frame_begin(&mut self, _ctx: &mut BridgeContext<'_>) {}

    /// Counterpart to [`Self::frame_begin`]. Called once after the entity
    /// loop, even if some entity updates errored. Adapters drain any
    /// frame-scoped command buffer into `ctx.commands` here and clear the
    /// frame state. Default impl is a no-op.
    fn frame_end(&mut self, _ctx: &mut BridgeContext<'_>) {}

    /// Call `init(entity)` on the loaded script.
    fn init(
        &mut self,
        handle: ScriptHandle,
        entity_id: u64,
        ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError>;

    /// Call `update(entity, dt)` on the loaded script.
    fn update(
        &mut self,
        handle: ScriptHandle,
        entity_id: u64,
        ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError>;

    /// Hot-path batch counterpart to [`Self::update`]. Called by the runner
    /// once per frame with the full list of `(handle, entity_id)` pairs that
    /// need an `update` tick. Adapters with expensive per-call setup (the JS
    /// host pays one `Context::with` enter per call) override this to amortise
    /// that cost across the whole batch. The default impl just loops through
    /// `update`, so adapters with no setup cost (Luau) need not override.
    fn update_batch(
        &mut self,
        entries: &[(ScriptHandle, u64)],
        ctx: &mut BridgeContext<'_>,
        errors: &mut Vec<ScriptError>,
    ) {
        for &(handle, entity_id) in entries {
            if let Err(e) = self.update(handle, entity_id, ctx) {
                errors.push(e);
            }
        }
    }

    /// Dispatch a lifecycle or user event to the script.
    ///
    /// `handler_name` is the **final function name** to call — `on_spawn`,
    /// `on_despawn`, `on_score_changed` — not the bare event name. The
    /// `on_` prefix for user events is applied by
    /// [`super::runner::ScriptBridgeRunner::dispatch_event`], because the
    /// engine's own lifecycle events already carry it via
    /// `EngineEvent::handler_name()`.
    ///
    /// The default forwards to [`Self::event_with_data`] with no payload, so
    /// an adapter only has to implement one of the two. It used to be a
    /// do-nothing `Ok(())`, which no adapter overrode — every `on_spawn`,
    /// `on_despawn` and broadcast `send_event` was silently swallowed.
    fn event(
        &mut self,
        handle: ScriptHandle,
        entity_id: u64,
        handler_name: &str,
        ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        self.event_with_data(
            handle,
            entity_id,
            handler_name,
            &crate::scripting::bridge::value::Value::Nil,
            ctx,
        )
    }

    /// Whether the script defines a global function by this name.
    ///
    /// Needed because a missing handler is indistinguishable from one that ran
    /// and did nothing: `named_callback` returns `Ok(())` either way. The
    /// event dispatch uses it to decide between the two spellings a handler
    /// can have - see `ScriptBridgeRunner::dispatch_event`. Defaults to
    /// `true`, so an adapter that cannot answer keeps the old behaviour of
    /// trying the specific name only.
    fn has_function(&self, _handle: ScriptHandle, _fn_name: &str) -> bool {
        true
    }

    /// Invoke a named global function on the script (timers, GUI callbacks, etc).
    fn named_callback(
        &mut self,
        _handle: ScriptHandle,
        _entity_id: u64,
        _fn_name: &str,
        _ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        Ok(())
    }

    /// Phase D — Args-bearing variant of [`Self::named_callback`]. Used by
    /// [`super::command::BridgeCommand::CallScriptMethod`] dispatch so the
    /// caller-supplied positional args reach the receiver as
    /// `function method(entity, ...args)`. Default impl forwards to
    /// [`Self::named_callback`], dropping the args — adapters that can
    /// marshal [`super::value::Value`] to their native runtime override
    /// this to carry them through.
    fn named_callback_with_args(
        &mut self,
        handle: ScriptHandle,
        entity_id: u64,
        fn_name: &str,
        _args: &[super::value::Value],
        ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        self.named_callback(handle, entity_id, fn_name, ctx)
    }

    /// Phase D — Payload-bearing variant of [`Self::event`]. Used by
    /// [`super::command::BridgeCommand::SendEventTo`] dispatch so the
    /// caller-supplied data reaches the receiver as
    /// `function on_<event>(entity, data)`.
    ///
    /// `handler_name` is the final function name, as for [`Self::event`].
    ///
    /// **This is the one an adapter should implement**: [`Self::event`]
    /// defaults to calling it with no payload. It must NOT default to
    /// forwarding back to [`Self::event`] — the two would call each other
    /// forever, which is exactly what happened when `event`'s do-nothing
    /// default was replaced with a forward. An adapter that implements
    /// neither silently drops events, which is the pre-existing behaviour
    /// this default preserves.
    fn event_with_data(
        &mut self,
        _handle: ScriptHandle,
        _entity_id: u64,
        _handler_name: &str,
        _data: &super::value::Value,
        _ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        Ok(())
    }

    /// Unload a script and release any backend-side resources.
    fn unload(&mut self, handle: ScriptHandle) -> Result<(), ScriptError>;

    /// Slice A — install a script-facing global function whose body is a
    /// Rust closure. Adapters that support live handler dispatch (Luau, JS)
    /// override this to bind `name` as a global that marshals call args
    /// per `descriptor.args`, builds a [`super::value::Value`] slice,
    /// invokes the closure, and converts the returned `Value` back to the
    /// native return. Adapters without script execution (e.g. `NoopAdapter`)
    /// fall back to the default no-op.
    ///
    /// Called once per registered descriptor at startup, after [`Self::load`]
    /// is available but before any [`Self::init`] / [`Self::update`] calls.
    fn install_dynamic_function(
        &mut self,
        _name: &str,
        _descriptor: &crate::package::script_api::ScriptFunctionDescriptor,
    ) {
    }

    /// Install a package-contributed script global that queues a command.
    ///
    /// Unlike [`Self::install_dynamic_function`] this needs no snapshot: the
    /// global only packages its arguments into a
    /// [`BridgeCommand::PackageCall`](crate::scripting::bridge::BridgeCommand::PackageCall)
    /// and the work happens at apply time — which is what lets the identical
    /// handler run in the browser hosts, where no snapshot exists.
    ///
    /// `arg_names` are the parameter names in positional order; the global
    /// maps its positional arguments onto them so the handler reads them by
    /// name.
    fn install_package_command(
        &mut self,
        _name: &str,
        _tag: &str,
        _arg_names: &[String],
    ) {
    }

    /// Install namespace tables - `Sprite.set_color` and the rest - each
    /// entry pointing at a flat global already installed. The list is
    /// [`crate::scripting::namespaces`]. Default no-op: JavaScript scripts
    /// keep the flat API.
    fn install_namespaces(&mut self, _entries: &[crate::scripting::namespaces::NamespacedName]) {}

    /// Phase D — Install a `require()` module resolver. Adapters that support
    /// shared library loading (Luau, JS host) override this to forward to
    /// their internal `set_module_resolver`. Default no-op for adapters
    /// without `require` support.
    fn set_module_resolver(&mut self, _resolver: super::module::ModuleResolver) {}

    /// Data-oriented design — install the shared registry of loaded `.data`
    /// tables read by the `get_data` script global. Adapters that support it
    /// (Luau, JS host) override this; default is a no-op.
    fn set_data_registry(&mut self, _registry: std::rc::Rc<crate::data::DataRegistry>) {}

    /// Phase D — Drop every cached `require()` module export. The next
    /// `require(name)` re-runs the module body via the installed resolver.
    /// Wired to asset-watcher hot-reload for `.lua` / `.luau` / `.js` files
    /// under `assets/scripts/`. Default no-op.
    fn invalidate_module_cache(&mut self) {}
}

/// Test-only no-op adapter used to validate the trait shape and Phase 2 wiring.
pub struct NoopAdapter {
    next: u64,
}

impl Default for NoopAdapter {
    fn default() -> Self {
        Self { next: 1 }
    }
}

impl NoopAdapter {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ScriptAdapter for NoopAdapter {
    fn load(&mut self, _path: &str, _source: &str) -> Result<ScriptHandle, ScriptError> {
        let h = ScriptHandle(self.next);
        self.next += 1;
        Ok(h)
    }

    fn init(
        &mut self,
        _handle: ScriptHandle,
        _entity_id: u64,
        _ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        Ok(())
    }

    fn update(
        &mut self,
        _handle: ScriptHandle,
        _entity_id: u64,
        _ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        Ok(())
    }

    fn unload(&mut self, _handle: ScriptHandle) -> Result<(), ScriptError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::capability::Capabilities;
    use super::super::context::BridgeReadSnapshot;
    use super::*;

    #[test]
    fn noop_adapter_lifecycle() {
        let mut a = NoopAdapter::new();
        let h = a.load("p", "src").unwrap();
        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        let mut ctx = BridgeContext::new(0.016, &read, Capabilities::empty(), &mut cmds);
        a.init(h, 1, &mut ctx).unwrap();
        a.update(h, 1, &mut ctx).unwrap();
        a.event(h, 1, "evt", &mut ctx).unwrap();
        a.named_callback(h, 1, "cb", &mut ctx).unwrap();
        a.unload(h).unwrap();
        assert_eq!(cmds.len(), 0);
    }

    #[test]
    fn noop_load_returns_unique_handles() {
        let mut a = NoopAdapter::new();
        let h1 = a.load("a", "").unwrap();
        let h2 = a.load("b", "").unwrap();
        assert_ne!(h1, h2);
    }
}
