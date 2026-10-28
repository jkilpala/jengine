//! Luau backend adapter for the Script Bridge.
//!
//! Implements [`super::super::adapter::ScriptAdapter`] using `mlua` with the `luau` feature.
//! Each loaded script gets its own sandboxed environment table that inherits read-only
//! engine globals via `__index`. Mutations flow exclusively through queued
//! [`BridgeCommand`]s; reads come from the per-frame [`BridgeReadSnapshot`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use mlua::prelude::*;
use mlua::{Compiler, Function as LuaFunction, Table as LuaTable};

use super::super::adapter::{ScriptAdapter, ScriptHandle};
use super::super::capability::Capabilities;
use super::super::command::BridgeCommand;
use super::super::context::{BridgeContext, BridgeReadSnapshot};
use super::super::diagnostics::{BackendKind, ScriptError, ScriptErrorKind, SourceLocation};
use super::super::module::{null_resolver, ModuleResolver};
use super::super::metadata::parse_extends;

/// Mutable state shared between the adapter and the closures registered into Luau.
///
/// The closures are owned by Lua (they outlive any single lifecycle call), so we cannot
/// borrow `BridgeContext` directly. Instead the adapter copies the snapshot reference and
/// per-call data into this state before each call and drains queued commands afterwards.
struct BridgeState {
    /// Snapshot pointer valid only for the duration of an active lifecycle call.
    snapshot: Option<*const BridgeReadSnapshot>,
    /// Commands queued during the active call.
    pending: Vec<BridgeCommand>,
    /// Phase 5.1: shared spawn-id counter set by the active lifecycle call.
    /// `None` outside a call window.
    spawn_counter: Option<Rc<Cell<u64>>>,
    /// Phase 5.5: shared sound-handle counter, same lifetime rules.
    sound_counter: Option<Rc<Cell<u64>>>,
    /// Phase 5.6: shared tween-id counter, same lifetime rules.
    tween_counter: Option<Rc<Cell<u64>>>,
    /// Phase 5.8: shared event-listener-id counter, same lifetime rules.
    event_listener_counter: Option<Rc<Cell<u64>>>,
    /// Phase 5.8: shared timer-id counter, same lifetime rules.
    timer_counter: Option<Rc<Cell<u64>>>,
    /// Script-reuse Phase A: host-supplied module resolver invoked by the
    /// `require` global. Defaults to [`null_resolver`] which always errors.
    resolver: ModuleResolver,
    /// Data-oriented design: shared registry of loaded `.data` tables read by
    /// the `get_data` global. Static for the session; installed once by the
    /// host. Defaults to an empty registry so `get_data` returns `nil`.
    data_registry: Rc<crate::data::DataRegistry>,
}

impl BridgeState {
    fn new() -> Self {
        Self {
            snapshot: None,
            pending: Vec::new(),
            spawn_counter: None,
            sound_counter: None,
            tween_counter: None,
            event_listener_counter: None,
            timer_counter: None,
            resolver: null_resolver(),
            data_registry: Rc::new(crate::data::DataRegistry::new()),
        }
    }

    fn snapshot(&self) -> LuaResult<&BridgeReadSnapshot> {
        // SAFETY: snapshot pointer is set on entry to a lifecycle call and cleared on exit.
        // Callbacks can only fire while Luau code is executing inside that call window.
        unsafe {
            self.snapshot
                .and_then(|p| p.as_ref())
                .ok_or_else(|| LuaError::RuntimeError("bridge snapshot unavailable".into()))
        }
    }

    fn alloc_spawn_id(&self) -> LuaResult<u64> {
        let counter = self
            .spawn_counter
            .as_ref()
            .ok_or_else(|| LuaError::RuntimeError("bridge spawn counter unavailable".into()))?;
        let id = counter.get();
        counter.set(id + 1);
        Ok(id)
    }

    fn alloc_sound_id(&self) -> LuaResult<u64> {
        let counter = self
            .sound_counter
            .as_ref()
            .ok_or_else(|| LuaError::RuntimeError("bridge sound counter unavailable".into()))?;
        let id = counter.get();
        counter.set(id + 1);
        Ok(id)
    }

    fn alloc_tween_id(&self) -> LuaResult<u32> {
        let counter = self
            .tween_counter
            .as_ref()
            .ok_or_else(|| LuaError::RuntimeError("bridge tween counter unavailable".into()))?;
        let id = counter.get();
        counter.set(id + 1);
        Ok(id as u32)
    }

    fn alloc_event_listener_id(&self) -> LuaResult<u32> {
        let counter = self
            .event_listener_counter
            .as_ref()
            .ok_or_else(|| LuaError::RuntimeError("bridge event listener counter unavailable".into()))?;
        let id = counter.get();
        counter.set(id + 1);
        Ok(id as u32)
    }

    fn alloc_timer_id(&self) -> LuaResult<u32> {
        let counter = self
            .timer_counter
            .as_ref()
            .ok_or_else(|| LuaError::RuntimeError("bridge timer counter unavailable".into()))?;
        let id = counter.get();
        counter.set(id + 1);
        Ok(id as u32)
    }
}

/// Per-script bookkeeping after a successful load.
struct LoadedScript {
    /// Sandboxed environment table for this script.
    env: LuaTable,
    /// Optional `init(entity)` lifecycle hook.
    init: Option<LuaFunction>,
    /// Optional `update(entity, dt)` lifecycle hook.
    update: Option<LuaFunction>,
    /// Source path (for diagnostics).
    path: String,
}

/// Luau adapter — primary native scripting backend (Phase 3).
pub struct LuauAdapter {
    lua: Lua,
    state: Rc<RefCell<BridgeState>>,
    scripts: HashMap<u64, LoadedScript>,
    next_handle: u64,
    compiler: Compiler,
    /// Capabilities the host advertises. Mirrored from runner; reserved for future gating.
    #[allow(dead_code)]
    capabilities: Capabilities,
}

impl LuauAdapter {
    /// Construct a sandboxed Luau VM with bridge globals registered.
    pub fn new(capabilities: Capabilities) -> Result<Self, ScriptError> {
        // The vendored Luau's maths through `libm` before anything compiles:
        // its constant folding computes through the same table.
        crate::scripting::script_math::install_luau_table();
        let lua = Lua::new();
        // Before the sandbox makes `math` read-only.
        install_script_random(&lua).map_err(|e| make_error(None, ScriptErrorKind::Runtime, e))?;
        // Sandbox: makes stdlib tables readonly, removes io/package/loadstring/most of debug,
        // and isolates per-environment writes through __index.
        lua.sandbox(true).map_err(|e| make_error(None, ScriptErrorKind::Runtime, e))?;

        let state = Rc::new(RefCell::new(BridgeState::new()));

        register_bridge_globals(&lua, &state)
            .map_err(|e| make_error(None, ScriptErrorKind::Runtime, e))?;

        // Script-reuse Phase A: per-VM module cache + cycle-detection set live
        // in the Lua registry so the cached export `LuaValue`s stay alive
        // across calls without us having to thread their lifetimes through
        // Rust. The `require` global reads/writes both tables.
        let cache = lua
            .create_table()
            .map_err(|e| make_error(None, ScriptErrorKind::Runtime, e))?;
        let loading = lua
            .create_table()
            .map_err(|e| make_error(None, ScriptErrorKind::Runtime, e))?;
        lua.set_named_registry_value(MODULE_CACHE_REG, cache)
            .map_err(|e| make_error(None, ScriptErrorKind::Runtime, e))?;
        lua.set_named_registry_value(MODULE_LOADING_REG, loading)
            .map_err(|e| make_error(None, ScriptErrorKind::Runtime, e))?;
        register_require_global(&lua, &state)
            .map_err(|e| make_error(None, ScriptErrorKind::Runtime, e))?;
        install_namespace_tables(&lua, &crate::scripting::namespaces::core_entries())
            .map_err(|e| make_error(None, ScriptErrorKind::Runtime, e))?;

        let compiler = Compiler::new()
            .set_optimization_level(2)
            .set_debug_level(1);

        Ok(Self {
            lua,
            state,
            scripts: HashMap::new(),
            next_handle: 1,
            compiler,
            capabilities,
        })
    }

    /// Install the host-side resolver consulted by the `require` script global.
    /// Replaces any previously installed resolver. Modules already cached are
    /// not re-evaluated.
    pub fn set_module_resolver(&mut self, resolver: ModuleResolver) {
        self.state.borrow_mut().resolver = resolver;
    }

    /// Install the shared data-table registry read by the `get_data` global.
    /// Replaces any previously installed registry.
    pub fn set_data_registry(&mut self, registry: Rc<crate::data::DataRegistry>) {
        self.state.borrow_mut().data_registry = registry;
    }

    /// Drop every cached module export so the next `require()` re-runs the
    /// module body. Used by the host's hot-reload path when a library script
    /// changes on disk.
    pub fn invalidate_module_cache(&self) -> LuaResult<()> {
        let cache: LuaTable = self.lua.named_registry_value(MODULE_CACHE_REG)?;
        cache.clear()
    }

    /// Build a fresh per-script environment that inherits globals via `__index`.
    fn new_script_env(&self) -> LuaResult<LuaTable> {
        let env = self.lua.create_table()?;
        let meta = self.lua.create_table()?;
        meta.set("__index", self.lua.globals())?;
        env.set_metatable(Some(meta));
        Ok(env)
    }

    /// Set the snapshot pointer for the duration of `f` and clear it afterwards.
    ///
    /// Hot-path optimization: when the runner has already opened a frame via
    /// [`Self::frame_begin`] (state.snapshot is `Some`), this is a thin
    /// pass-through that just runs `f`. State install/teardown happens once
    /// per frame in `frame_begin`/`frame_end` instead of once per entity.
    fn with_snapshot<R>(
        &self,
        ctx: &mut BridgeContext<'_>,
        f: impl FnOnce(&Self) -> Result<R, ScriptError>,
    ) -> Result<R, ScriptError> {
        let frame_active = self.state.borrow().snapshot.is_some();
        if frame_active {
            return f(self);
        }
        {
            let mut s = self.state.borrow_mut();
            s.snapshot = Some(ctx.read as *const BridgeReadSnapshot);
            s.spawn_counter = Some(ctx.spawn_counter());
            s.sound_counter = Some(ctx.sound_counter());
            s.tween_counter = Some(ctx.tween_counter());
            s.event_listener_counter = Some(ctx.event_listener_counter());
            s.timer_counter = Some(ctx.timer_counter());
            s.pending.clear();
        }
        let res = f(self);
        let drained: Vec<BridgeCommand> = {
            let mut s = self.state.borrow_mut();
            s.snapshot = None;
            s.spawn_counter = None;
            s.sound_counter = None;
            s.tween_counter = None;
            s.event_listener_counter = None;
            s.timer_counter = None;
            std::mem::take(&mut s.pending)
        };
        for cmd in drained {
            ctx.push_command(cmd);
        }
        res
    }
}

impl ScriptAdapter for LuauAdapter {
    /// Hot-path frame setup. Installs the constant-per-frame state into the
    /// adapter's shared `Rc<RefCell<State>>` once so the per-entity
    /// `update`/`init` calls don't pay 6 `borrow_mut` field assignments and
    /// 5 `Rc::clone`s each. Paired with [`Self::frame_end`].
    fn frame_begin(&mut self, ctx: &mut BridgeContext<'_>) {
        let mut s = self.state.borrow_mut();
        s.snapshot = Some(ctx.read as *const BridgeReadSnapshot);
        s.spawn_counter = Some(ctx.spawn_counter());
        s.sound_counter = Some(ctx.sound_counter());
        s.tween_counter = Some(ctx.tween_counter());
        s.event_listener_counter = Some(ctx.event_listener_counter());
        s.timer_counter = Some(ctx.timer_counter());
        s.pending.clear();
    }

    /// Drain frame-scoped pending commands into `ctx.commands` and clear the
    /// installed state pointers so post-frame callbacks (legacy single-call
    /// paths) re-arm via [`Self::with_snapshot`].
    fn frame_end(&mut self, ctx: &mut BridgeContext<'_>) {
        let drained: Vec<BridgeCommand> = {
            let mut s = self.state.borrow_mut();
            s.snapshot = None;
            s.spawn_counter = None;
            s.sound_counter = None;
            s.tween_counter = None;
            s.event_listener_counter = None;
            s.timer_counter = None;
            std::mem::take(&mut s.pending)
        };
        for cmd in drained {
            ctx.push_command(cmd);
        }
    }

    fn load(&mut self, path: &str, source: &str) -> Result<ScriptHandle, ScriptError> {
        let env = self
            .new_script_env()
            .map_err(|e| make_error(Some(path.to_string()), ScriptErrorKind::Runtime, e))?;

        // Phase B — if the script declares `extends = "..."`, load the
        // parent module first and chain it into the env so missing
        // `init`/`update`/handler functions fall through to the parent.
        // The parent goes through the same `load_module_inner` path the
        // `require` global uses, so multi-level chains work transparently.
        if let Some(parent_path) = parse_extends(source) {
            let parent_value = load_module_inner(&self.lua, &self.state, &parent_path)
                .map_err(|e| {
                    classify_error(Some(path.to_string()), e)
                })?;
            // Replace the env's metatable so `__index` resolves child →
            // parent → globals. We lose direct table __index → globals,
            // so emulate it with a function-based fallback.
            let LuaValue::Table(parent_tbl) = parent_value.clone() else {
                return Err(make_error_msg(
                    Some(path.to_string()),
                    ScriptErrorKind::Runtime,
                    &format!(
                        "extends '{parent_path}' did not return a table; library scripts must return a table of fields/functions"
                    ),
                ));
            };
            let meta = self.lua.create_table()
                .map_err(|e| make_error(Some(path.to_string()), ScriptErrorKind::Runtime, e))?;
            // __index walks parent first, then globals. Parent itself
            // chains to its own parent via its metatable, so multi-level
            // is automatic.
            let parent_clone = parent_tbl.clone();
            let globals = self.lua.globals();
            let lookup = self.lua.create_function(move |_, (_t, k): (LuaTable, LuaValue)| -> LuaResult<LuaValue> {
                let v: LuaValue = parent_clone.get(k.clone())?;
                if !matches!(v, LuaValue::Nil) {
                    return Ok(v);
                }
                let g: LuaValue = globals.get(k)?;
                Ok(g)
            }).map_err(|e| make_error(Some(path.to_string()), ScriptErrorKind::Runtime, e))?;
            meta.set("__index", lookup)
                .map_err(|e| make_error(Some(path.to_string()), ScriptErrorKind::Runtime, e))?;
            env.set_metatable(Some(meta));
            // Expose `super` for explicit parent dispatch.
            env.set("super", parent_tbl)
                .map_err(|e| make_error(Some(path.to_string()), ScriptErrorKind::Runtime, e))?;
        }

        // Compile to bytecode with optimization+debug, then load with a per-script env.
        let bytecode = self.compiler.compile(source).map_err(|e| {
            make_error(Some(path.to_string()), ScriptErrorKind::Syntax, e)
        })?;

        let chunk = self
            .lua
            .load(&bytecode)
            .set_name(path)
            .set_environment(env.clone());
        chunk
            .exec()
            .map_err(|e| classify_error(Some(path.to_string()), e))?;

        // `env.get` walks the metatable chain natively, so an inherited
        // `init`/`update` is captured here without extra plumbing.
        let init: Option<LuaFunction> = env.get("init").ok();
        let update: Option<LuaFunction> = env.get("update").ok();

        let handle = ScriptHandle(self.next_handle);
        self.next_handle += 1;
        self.scripts.insert(
            handle.0,
            LoadedScript { env, init, update, path: path.to_string() },
        );
        Ok(handle)
    }

    fn init(
        &mut self,
        handle: ScriptHandle,
        entity_id: u64,
        ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        let Some(loaded) = self.scripts.get(&handle.0) else {
            return Err(make_error_msg(None, ScriptErrorKind::Runtime, "unknown script handle"));
        };
        let Some(init_fn) = loaded.init.as_ref() else { return Ok(()); };
        // Hot path: borrow `path` instead of cloning per call. Only the
        // error path materialises a `String`. `with_snapshot` takes `&self`
        // (RefCell-mutates internally), so the `loaded` borrow can stay
        // live across the call.
        let path: &str = &loaded.path;
        self.with_snapshot(ctx, |_this| {
            init_fn
                .call::<()>(entity_id)
                .map_err(|e| classify_error(Some(path.to_string()), e))
        })
    }

    fn update(
        &mut self,
        handle: ScriptHandle,
        entity_id: u64,
        ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        let Some(loaded) = self.scripts.get(&handle.0) else {
            return Err(make_error_msg(None, ScriptErrorKind::Runtime, "unknown script handle"));
        };
        let Some(update_fn) = loaded.update.as_ref() else { return Ok(()); };
        // Hot path: borrow `path`, only allocate on error. See `init`.
        let path: &str = &loaded.path;
        let dt = ctx.dt;
        // Each script's cost, while the profiler records (`profiling::trace`).
        let t0 = crate::profiling::trace::enabled().then(crate::profiling::now_ms);
        let result = self.with_snapshot(ctx, |_this| {
            update_fn
                .call::<()>((entity_id, dt))
                .map_err(|e| classify_error(Some(path.to_string()), e))
        });
        if let Some(t0) = t0 {
            crate::profiling::trace::script(path, crate::profiling::now_ms() - t0);
        }
        result
    }

    fn named_callback(
        &mut self,
        handle: ScriptHandle,
        entity_id: u64,
        fn_name: &str,
        ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        let Some(loaded) = self.scripts.get(&handle.0) else {
            return Err(make_error_msg(None, ScriptErrorKind::Runtime, "unknown script handle"));
        };
        let Ok(callback): LuaResult<LuaFunction> = loaded.env.get(fn_name) else {
            return Ok(());
        };
        let path: &str = &loaded.path;
        let t0 = crate::profiling::trace::enabled().then(crate::profiling::now_ms);
        let result = self.with_snapshot(ctx, |_this| {
            callback
                .call::<()>(entity_id)
                .map_err(|e| classify_error(Some(path.to_string()), e))
        });
        if let Some(t0) = t0 {
            crate::profiling::trace::script(path, crate::profiling::now_ms() - t0);
        }
        result
    }

    /// Whether the **script itself** defines a function of this name.
    ///
    /// `raw_get`, not `get`, and the difference is the whole point: a script's
    /// environment falls through to the engine's globals, and one of those
    /// globals is `on_event` - the *registration* function. A plain `get`
    /// therefore answers "yes" for every script ever loaded, and the event
    /// dispatch would call the engine's own registration function with a
    /// handler's arguments. That is not hypothetical; it is what the first
    /// version of this did, and
    /// `a_script_with_no_handler_at_all_is_silent_rather_than_an_error`
    /// caught it: `bad argument #2: error converting Lua string to u64`.
    ///
    /// A raw read sees only what the script put there - and then what its
    /// `extends` parents put on their tables, reached through `super`. That
    /// chain is tables all the way up, never the globals, so it cannot answer
    /// with the engine's `on_event`; and without it an inherited
    /// `on_<event>` fired in the editor, which looks names up through the
    /// script's metatable, and not here.
    fn has_function(&self, handle: ScriptHandle, fn_name: &str) -> bool {
        self.scripts.get(&handle.0).is_some_and(|loaded| {
            loaded.env.raw_get::<LuaFunction>(fn_name).is_ok()
                || matches!(
                    loaded.env.raw_get::<LuaValue>("super"),
                    Ok(LuaValue::Table(parent)) if parent.get::<LuaFunction>(fn_name).is_ok()
                )
        })
    }

    /// Phase D — args-bearing callback. Marshals each [`Value`] via a Lua-
    /// aware helper so strings allocate against the live `&Lua` handle
    /// (the bare [`value_to_lua`] returns Nil for strings — see its docs).
    fn named_callback_with_args(
        &mut self,
        handle: ScriptHandle,
        entity_id: u64,
        fn_name: &str,
        args: &[crate::scripting::bridge::value::Value],
        ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        let Some(loaded) = self.scripts.get(&handle.0) else {
            return Err(make_error_msg(None, ScriptErrorKind::Runtime, "unknown script handle"));
        };
        let Ok(callback): LuaResult<LuaFunction> = loaded.env.get(fn_name) else {
            return Ok(());
        };
        let path: &str = &loaded.path;
        let path_owned = path.to_string();
        self.with_snapshot(ctx, |this| {
            let mut multi: Vec<LuaValue> = Vec::with_capacity(args.len() + 1);
            multi.push(LuaValue::Integer(entity_id as mlua::Integer));
            for a in args {
                multi.push(value_to_lua_in(&this.lua, a).map_err(|e| {
                    classify_error(Some(path_owned.clone()), e)
                })?);
            }
            callback
                .call::<()>(LuaMultiValue::from_vec(multi))
                .map_err(|e| classify_error(Some(path_owned.clone()), e))
        })
    }

    /// Phase D — payload-bearing event. Looks up `on_<event>` (matching the
    /// existing event dispatcher in `register_event_listener` / `event`)
    /// and invokes it with `(entity, data)`.
    fn event_with_data(
        &mut self,
        handle: ScriptHandle,
        entity_id: u64,
        event_name: &str,
        data: &crate::scripting::bridge::value::Value,
        ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        // `event_name` is already the handler name — the `on_` prefix for
        // user events is applied by the runner's dispatch_event*, so adding
        // one here would look for `on_on_spawn`.
        let handler_name = event_name;
        self.named_callback_with_args(
            handle,
            entity_id,
            handler_name,
            std::slice::from_ref(data),
            ctx,
        )
    }

    fn unload(&mut self, handle: ScriptHandle) -> Result<(), ScriptError> {
        self.scripts.remove(&handle.0);
        Ok(())
    }

    /// Slice A — install a registry-backed function as a Lua global. The
    /// closure marshals call args per `descriptor.args`, dispatches into
    /// `descriptor.handler`, and converts the returned [`Value`] back to
    /// Lua. Errors propagate as Lua runtime errors. Functions with no live
    /// `handler` are skipped (they remain metadata-only).
    fn install_dynamic_function(
        &mut self,
        name: &str,
        descriptor: &crate::package::script_api::ScriptFunctionDescriptor,
    ) {
        let Some(handler) = descriptor.handler.clone() else {
            return;
        };
        let arg_types: Vec<crate::package::script_api::ScriptValueType> = descriptor
            .args
            .iter()
            .map(|a| a.arg_type.clone())
            .collect();
        let state = Rc::clone(&self.state);
        let fn_name_for_err = name.to_string();
        let lua_fn_result = self.lua.create_function(move |_lua, lua_args: LuaMultiValue| {
            let mut values: Vec<crate::scripting::bridge::value::Value> = Vec::with_capacity(arg_types.len());
            let mut iter = lua_args.into_iter();
            for ty in &arg_types {
                let lv = iter.next().unwrap_or(LuaValue::Nil);
                values.push(lua_to_value(ty, lv).map_err(|e| {
                    LuaError::RuntimeError(format!(
                        "{}: arg conversion failed: {}",
                        fn_name_for_err, e
                    ))
                })?);
            }
            // Borrow state, build ctx, invoke handler, then drop borrow before
            // converting return (Value -> LuaValue may allocate Lua tables).
            let result = {
                let mut s = state.borrow_mut();
                let snapshot = match s.snapshot {
                    // SAFETY: snapshot pointer is set by frame_begin/with_snapshot
                    // and only borrowed for the duration of this call window.
                    Some(p) => unsafe { &*p },
                    None => {
                        return Err(LuaError::RuntimeError(format!(
                            "{}: bridge snapshot unavailable",
                            fn_name_for_err
                        )));
                    }
                };
                let mut ctx = crate::package::script_api::ScriptCallCtx {
                    snapshot,
                    commands: &mut s.pending,
                };
                handler(&mut ctx, &values)
            };
            match result {
                Ok(v) => Ok(value_to_lua(&v)),
                Err(e) => Err(LuaError::RuntimeError(format!("{}: {}", fn_name_for_err, e))),
            }
        });
        match lua_fn_result {
            Ok(lua_fn) => {
                if let Err(e) = self.lua.globals().set(name, lua_fn) {
                    log::warn!("install_dynamic_function: failed to bind '{}': {}", name, e);
                }
            }
            Err(e) => {
                log::warn!("install_dynamic_function: failed to create '{}': {}", name, e);
            }
        }
    }

    fn install_package_command(&mut self, name: &str, tag: &str, arg_names: &[String]) {
        let state = Rc::clone(&self.state);
        let tag = tag.to_string();
        let arg_names: Vec<String> = arg_names.to_vec();
        let fn_name = name.to_string();

        let lua_fn = self.lua.create_function(move |_lua, lua_args: LuaMultiValue| {
            let mut args = serde_json::Map::new();
            let mut iter = lua_args.into_iter();
            for key in &arg_names {
                let value = iter.next().unwrap_or(LuaValue::Nil);
                // Recursive, so a table of strings or of tables arrives
                // whole - as it does from the editor's runtime. The flat
                // converter kept only arrays of numbers and turned anything
                // else into null.
                args.insert(key.clone(), lua_to_json(&value));
            }
            state
                .borrow_mut()
                .pending
                .push(crate::scripting::bridge::BridgeCommand::PackageCall {
                    tag: tag.clone(),
                    args: serde_json::Value::Object(args),
                });
            Ok(())
        });

        match lua_fn {
            Ok(f) => {
                if let Err(e) = self.lua.globals().set(name, f) {
                    log::warn!("install_package_command: binding '{fn_name}' failed: {e}");
                }
            }
            Err(e) => log::warn!("install_package_command: creating '{fn_name}' failed: {e}"),
        }
    }

    fn install_namespaces(&mut self, entries: &[crate::scripting::namespaces::NamespacedName]) {
        if let Err(e) = install_namespace_tables(&self.lua, entries) {
            log::warn!("install_namespaces: {e}");
        }
    }

    /// Phase D — Trait-level entry point that the runner uses to wire a host-
    /// supplied `require` resolver. Delegates to the inherent
    /// [`Self::set_module_resolver`] which performs the registry update + re-
    /// installs the `require` global.
    fn set_module_resolver(&mut self, resolver: ModuleResolver) {
        LuauAdapter::set_module_resolver(self, resolver);
    }

    fn set_data_registry(&mut self, registry: Rc<crate::data::DataRegistry>) {
        LuauAdapter::set_data_registry(self, registry);
    }

    /// Phase D — Trait-level forward to [`Self::invalidate_module_cache`].
    /// Logs and swallows errors so the asset-watcher caller can stay
    /// infallible; a stale cache is a recoverable warning, not a hard fail.
    fn invalidate_module_cache(&mut self) {
        if let Err(e) = LuauAdapter::invalidate_module_cache(self) {
            log::warn!("[LuauAdapter] invalidate_module_cache failed: {}", e);
        }
    }
}

// --- Bridge globals registration ----------------------------------------------------------

/// Lua registry key holding the per-VM module-export cache table
/// (`{ [path] = exported_value }`).
const MODULE_CACHE_REG: &str = "__jengine_module_cache__";
/// Lua registry key holding the in-progress set used by `require` for cycle
/// detection (`{ [path] = true }`).
const MODULE_LOADING_REG: &str = "__jengine_module_loading__";

/// Install the `require(path)` global. The closure captures the bridge state
/// (for the host resolver) and pulls the cache + cycle-detection tables out of
/// the registry on each call. Modules execute in a fresh sandboxed env that
/// inherits globals via `__index` — same shape as entity scripts.
fn register_require_global(lua: &Lua, state: &Rc<RefCell<BridgeState>>) -> LuaResult<()> {
    let s = state.clone();
    let f = lua.create_function(move |lua, path: String| -> LuaResult<LuaValue> {
        load_module_inner(lua, &s, &path)
    })?;
    lua.globals().set("require", f)?;
    Ok(())
}

/// Shared module-loading core used by both the script-side `require` global
/// and the entity-loader's `extends` resolution. Cache hit → return cached
/// value. Cycle → error. Otherwise: resolve source via the host resolver,
/// execute in a fresh sandboxed env, capture the chunk's `return` value,
/// chain its prototype to its own `extends` parent (recursive), cache the
/// result, return it.
fn load_module_inner(
    lua: &Lua,
    state: &Rc<RefCell<BridgeState>>,
    path: &str,
) -> LuaResult<LuaValue> {
    let cache: LuaTable = lua.named_registry_value(MODULE_CACHE_REG)?;
    // Cache hit: return the previously evaluated export. `nil` exports are
    // treated as a miss (Lua's idiomatic "absent" value); modules that
    // genuinely want to export nothing can `return false`.
    let cached: LuaValue = cache.get(path)?;
    if !matches!(cached, LuaValue::Nil) {
        return Ok(cached);
    }

    let loading: LuaTable = lua.named_registry_value(MODULE_LOADING_REG)?;
    let in_progress: bool = loading.get(path)?;
    if in_progress {
        return Err(LuaError::RuntimeError(format!(
            "require('{path}'): circular dependency detected"
        )));
    }

    let source = {
        let st = state.borrow();
        (st.resolver)(path).map_err(LuaError::RuntimeError)?
    };

    loading.set(path, true)?;

    // Phase B — if the module declares `extends = "..."`, recursively load
    // the parent first so we can chain the returned table's metatable.
    // Recursion happens before evaluating *this* module so the cycle marker
    // for `path` is still set, catching `A extends B extends A`.
    let parent: Option<LuaValue> = match parse_extends(&source) {
        Some(parent_path) => Some(load_module_inner(lua, state, &parent_path)?),
        None => None,
    };

    // Build a fresh sandboxed env, inheriting globals (so the module sees
    // `require`, bridge API, etc.).
    let env = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.set("__index", lua.globals())?;
    env.set_metatable(Some(meta));
    // Expose `super` to the module body when it extends another module —
    // useful for library scripts that want to call the parent's helpers.
    if let Some(p) = parent.as_ref() {
        env.set("super", p.clone())?;
    }

    // Compile + run; capture the chunk's `return` value as the export.
    let chunk = lua
        .load(&source)
        .set_name(format!("@{path}"))
        .set_environment(env);
    let exec_result: LuaResult<LuaValue> = chunk.call(());

    // Remove the in-progress marker before propagating errors so a failed
    // module doesn't permanently poison the loading set.
    let _ = loading.set(path, LuaValue::Nil);

    let value = exec_result?;

    // Chain inheritance on the returned export so look-ups in the child
    // walk parent → grandparent naturally via Lua metatables. We only
    // chain when the returned export is itself a table (the common shape
    // for library modules).
    if let (LuaValue::Table(child_tbl), Some(LuaValue::Table(parent_tbl))) =
        (&value, parent.as_ref())
    {
        // Preserve any pre-existing metatable's other fields by mutating
        // (or creating) one whose __index is the parent table. Modules
        // rarely set their own metatable, so the simple replace path
        // covers >99% of real cases.
        let m = lua.create_table()?;
        m.set("__index", parent_tbl.clone())?;
        child_tbl.set_metatable(Some(m));
    }

    cache.set(path, value.clone())?;
    Ok(value)
}

/// Build one read-only table per namespace, each field the flat global it
/// names - the same function, so `Sprite.set_color` and `set_sprite_color`
/// cannot behave differently. Read-only because natively every script shares
/// these tables: one script assigning `Sprite.set_color = nil` would break
/// the rest. An entry whose flat global is missing is logged and skipped
/// rather than installed as nil.
fn install_namespace_tables(
    lua: &Lua,
    entries: &[crate::scripting::namespaces::NamespacedName],
) -> LuaResult<()> {
    let globals = lua.globals();
    let mut tables: Vec<(String, LuaTable)> = Vec::new();
    for e in entries {
        let value: LuaValue = globals.get(e.flat.as_str())?;
        if matches!(value, LuaValue::Nil) {
            log::warn!("namespace {}: no global '{}' to point at", e.qualified(), e.flat);
            continue;
        }
        let table = match tables.iter().find(|(ns, _)| *ns == e.namespace) {
            Some((_, t)) => t.clone(),
            None => {
                let t = lua.create_table()?;
                tables.push((e.namespace.clone(), t.clone()));
                t
            }
        };
        table.set(e.short.as_str(), value)?;
    }
    for (ns, table) in tables {
        table.set_readonly(true);
        globals.set(ns, table)?;
    }
    Ok(())
}

/// `math.random` and `math.randomseed` over [`crate::scripting::script_math`]:
/// one stream every script draws from, the same numbers for the same seed
/// natively and in the browsers (whose Fengari calls the same functions).
/// Luau's own generator is a different algorithm from Lua 5.3's, so a seed
/// meant two different games. Installed before the sandbox makes `math`
/// read-only.
fn install_script_random(lua: &Lua) -> LuaResult<()> {
    use crate::scripting::script_math;
    let math: LuaTable = lua.globals().get("math")?;
    let random = lua.create_function(|_, (m, n): (Option<f64>, Option<f64>)| -> LuaResult<f64> {
        script_math::random(m, n).map_err(|e| LuaError::RuntimeError(format!("bad argument to 'random' ({e})")))
    })?;
    math.set("random", random)?;
    let seed = lua.create_function(|_, x: f64| -> LuaResult<()> {
        script_math::seed(x);
        Ok(())
    })?;
    math.set("randomseed", seed)?;
    Ok(())
}

fn register_bridge_globals(lua: &Lua, state: &Rc<RefCell<BridgeState>>) -> LuaResult<()> {
    let globals = lua.globals();

    // Mutating commands.
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y): (u64, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetPosition { entity_id, x, y });
            Ok(())
        })?;
        globals.set("set_position", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, current, max): (u64, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetHealth { entity_id, current, max });
            Ok(())
        })?;
        globals.set("set_health", f)?;
    }
    {
        // `get_health(entity)` -> current, max (or nil when the entity has no
        // Health). `set_health` shipped without a reader, so a script could
        // damage something and never show its HP.
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<(Option<f32>, Option<f32>)> {
            let st = s.borrow();
            match st.snapshot()?.health_of(entity_id) {
                Some((current, max)) => Ok((Some(current), Some(max))),
                None => Ok((None, None)),
            }
        })?;
        globals.set("get_health", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, damage, knockback, retrigger): (u64, f32, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetHitbox { entity_id, damage, knockback, retrigger });
            Ok(())
        })?;
        globals.set("set_hitbox", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, radians): (u64, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetRotation { entity_id, radians });
            Ok(())
        })?;
        globals.set("set_rotation", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y): (u64, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetScale { entity_id, x, y });
            Ok(())
        })?;
        globals.set("set_scale", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, message: String| {
            s.borrow_mut().pending.push(BridgeCommand::Log { message });
            Ok(())
        })?;
        globals.set("log", f)?;
    }
    {
        // `print` goes where `log` goes, as it does in the editor and on the
        // web. It was Luau's own, writing to stdout, so a script's `print`
        // reached the console in two runtimes and nowhere visible in this one.
        // Arguments are joined with tabs, as Lua's `print` joins them.
        let s = state.clone();
        let f = lua.create_function(move |lua, args: LuaMultiValue| {
            let tostring: LuaFunction = lua.globals().get("tostring")?;
            let mut parts = Vec::with_capacity(args.len());
            for v in args {
                parts.push(tostring.call::<String>(v)?);
            }
            s.borrow_mut().pending.push(BridgeCommand::Log { message: parts.join("\t") });
            Ok(())
        })?;
        globals.set("print", f)?;
    }
    // Slice 3.5.A — entity-parent attach/detach.
    // `set_parent(child, parent)` — parent may be `nil` to detach.
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (child_id, parent_id): (u64, Option<u64>)| {
            s.borrow_mut().pending.push(BridgeCommand::SetParent { child_id, parent_id });
            Ok(())
        })?;
        globals.set("set_parent", f)?;
    }
    // Slice 3.5.C — `set_animation_time` moves a skeletal player's head.
    // `play_animation`, `stop_animation` and `set_animation_speed` are bound
    // once, below (Phase 5.6), and reach sprite and skeletal animation
    // alike; a skeletal copy of each used to be bound here and then
    // silently overwritten by those.
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, t): (u64, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetSkeletalPlayer {
                entity_id,
                clip: None,
                time: Some(t),
                speed: None,
                looping: None,
                playing: None,
            });
            Ok(())
        })?;
        globals.set("set_animation_time", f)?;
    }

    // Read accessors.
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<(Option<f32>, Option<f32>)> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            // nil for an entity that does not exist, as every runtime answers.
            Ok(snap.get_position(entity_id).map_or((None, None), |(x, y)| (Some(x), Some(y))))
        })?;
        globals.set("get_position", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<Option<f32>> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.get_rotation(entity_id))
        })?;
        globals.set("get_rotation", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<(Option<f32>, Option<f32>)> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            // nil for an entity that does not exist, as every runtime answers.
            Ok(snap.get_scale(entity_id).map_or((None, None), |(x, y)| (Some(x), Some(y))))
        })?;
        globals.set("get_scale", f)?;
    }

    // Slice 3.5.A — world-space transform getters. Prefer the cached
    // `WorldTransform` (populated by `resolve_world_transforms` before
    // script tick); fall back to local Transform when no parent chain.
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<(Option<f32>, Option<f32>)> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            // nil for an entity that does not exist, as every runtime answers.
            Ok(snap.get_world_position(entity_id).map_or((None, None), |(x, y)| (Some(x), Some(y))))
        })?;
        globals.set("get_world_position", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<Option<f32>> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.get_world_rotation(entity_id))
        })?;
        globals.set("get_world_rotation", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<(Option<f32>, Option<f32>)> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            // nil for an entity that does not exist, as every runtime answers.
            Ok(snap.get_world_scale(entity_id).map_or((None, None), |(x, y)| (Some(x), Some(y))))
        })?;
        globals.set("get_world_scale", f)?;
    }
    // Slice 3.5.B — skeleton lookup. Returns `nil` (Lua: nothing / Lua-nil)
    // when the bone or skeleton is missing.
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (root_id, bone_name): (u64, String)| -> LuaResult<Option<u64>> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.find_bone(root_id, &bone_name))
        })?;
        globals.set("find_bone", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, skeleton_name: String| -> LuaResult<Option<u64>> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.find_skeleton(&skeleton_name))
        })?;
        globals.set("find_skeleton", f)?;
    }
    // Slice 3.5.C — skeletal-player read accessors.
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<Option<f32>> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.get_animation_time(entity_id))
        })?;
        globals.set("get_animation_time", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<Option<String>> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.get_name(entity_id))
        })?;
        globals.set("get_name", f)?;
    }

    // --- Phase 5.1: entity spawn / destroy / find / tags ---------------------
    {
        let s = state.clone();
        let f = lua.create_function(move |_, name: String| -> LuaResult<u64> {
            let mut st = s.borrow_mut();
            let id = st.alloc_spawn_id()?;
            st.pending.push(BridgeCommand::SpawnEntity { name, prefab: None, spawn_id: id });
            Ok(id)
        })?;
        globals.set("spawn_entity", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, prefab: String| -> LuaResult<u64> {
            let mut st = s.borrow_mut();
            let id = st.alloc_spawn_id()?;
            st.pending.push(BridgeCommand::SpawnEntity {
                name: prefab.clone(),
                prefab: Some(prefab),
                spawn_id: id,
            });
            Ok(id)
        })?;
        globals.set("spawn_prefab", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, rig_id: String| -> LuaResult<u64> {
            let mut st = s.borrow_mut();
            let id = st.alloc_spawn_id()?;
            st.pending.push(BridgeCommand::SpawnRig { rig_id, spawn_id: id });
            Ok(id)
        })?;
        globals.set("spawn_rig", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y): (u64, f32, f32)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetIkTarget { entity_id, x, y });
            Ok(())
        })?;
        globals.set("set_ik_target", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, clip, seconds): (u64, String, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::CrossfadeAnimation { entity_id, clip, seconds });
            Ok(())
        })?;
        globals.set("crossfade_animation", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, clip, weight): (u64, Option<String>, Option<f32>)| {
            s.borrow_mut().pending.push(BridgeCommand::SetAnimationBlend {
                entity_id,
                clip: clip.unwrap_or_default(),
                weight: weight.unwrap_or(0.0),
            });
            Ok(())
        })?;
        globals.set("set_animation_blend", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y): (u64, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetAnimationBlendPosition { entity_id, x, y });
            Ok(())
        })?;
        globals.set("set_animation_blend_position", f)?;
    }
    {
        // Animation layers (`SkeletalPlayer::layer_op`): play a clip on a
        // layer, fade it out, or fade it to a weight.
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, layer, clip, seconds, looping): (u64, String, String, Option<f32>, Option<bool>)| {
                s.borrow_mut().pending.push(BridgeCommand::AnimationLayer {
                    entity_id,
                    op: "play".into(),
                    layer,
                    clip,
                    value: 1.0,
                    seconds: seconds.unwrap_or(0.2),
                    looping: looping.unwrap_or(false),
                });
                Ok(())
            },
        )?;
        globals.set("animation_play_layer", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, layer, seconds): (u64, String, Option<f32>)| {
            s.borrow_mut().pending.push(BridgeCommand::AnimationLayer {
                entity_id,
                op: "stop".into(),
                layer,
                clip: String::new(),
                value: 0.0,
                seconds: seconds.unwrap_or(0.2),
                looping: false,
            });
            Ok(())
        })?;
        globals.set("animation_stop_layer", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, layer, weight, seconds): (u64, String, f32, Option<f32>)| {
            s.borrow_mut().pending.push(BridgeCommand::AnimationLayer {
                entity_id,
                op: "weight".into(),
                layer,
                clip: String::new(),
                value: weight,
                seconds: seconds.unwrap_or(0.0),
                looping: false,
            });
            Ok(())
        })?;
        globals.set("animation_set_layer_weight", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, speed): (u64, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetConveyorSpeed { entity_id, speed });
            Ok(())
        })?;
        globals.set("set_conveyor_speed", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, weight): (u64, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetIkWeight { entity_id, weight });
            Ok(())
        })?;
        globals.set("set_ik_weight", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| {
            s.borrow_mut().pending.push(BridgeCommand::DestroyEntity { entity_id });
            Ok(())
        })?;
        globals.set("destroy_entity", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, tag): (u64, String)| {
            s.borrow_mut().pending.push(BridgeCommand::AddTag { entity_id, tag });
            Ok(())
        })?;
        globals.set("add_tag", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, tag): (u64, String)| {
            s.borrow_mut().pending.push(BridgeCommand::RemoveTag { entity_id, tag });
            Ok(())
        })?;
        globals.set("remove_tag", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, tag): (u64, String)| -> LuaResult<bool> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.has_tag(entity_id, &tag))
        })?;
        globals.set("has_tag", f)?;
    }
    // ---- Math helpers ----------------------------------------------------
    // Bare globals, matching the browser preamble exactly (`lerp` is
    // unclamped, `clamp` is min/max order-insensitive only as far as
    // `Math.min(Math.max(..))` is). Luau ships `math.clamp`, but scripts
    // written in the editor call the bare names, and those were `nil` here.
    // In `f64` and through `script_math`, as the browsers compute them: in
    // `f32` here they answered differently from the editor for any input.
    {
        let f = lua.create_function(|_, (a, b, t): (f64, f64, f64)| -> LuaResult<f64> {
            Ok(crate::scripting::script_math::lerp(a, b, t))
        })?;
        globals.set("lerp", f)?;
    }
    {
        let f = lua.create_function(|_, (v, mn, mx): (f32, f32, f32)| -> LuaResult<f32> {
            Ok(v.max(mn).min(mx))
        })?;
        globals.set("clamp", f)?;
    }
    {
        let f = lua.create_function(
            |_, (x1, y1, x2, y2): (f64, f64, f64, f64)| -> LuaResult<f64> {
                Ok(crate::scripting::script_math::distance(x1, y1, x2, y2))
            },
        )?;
        globals.set("distance", f)?;
    }
    {
        let f = lua.create_function(
            |_, (x1, y1, x2, y2): (f64, f64, f64, f64)| -> LuaResult<f64> {
                Ok(crate::scripting::script_math::angle_between(x1, y1, x2, y2))
            },
        )?;
        globals.set("angle_between", f)?;
    }
    // ---- Frame stats -----------------------------------------------------
    // Registered as metadata in `core_script_api.rs`, which installs
    // nothing, so these were `nil` on native while the browser preamble
    // implemented them: a script calling `get_fps()` worked in the editor
    // and failed in the shipped native game. They read the same
    // `FrameStats` the F3 overlay draws.
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<f32> {
            let st = s.borrow();
            Ok(st.snapshot()?.frame_stats.fps)
        })?;
        globals.set("get_fps", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<f32> {
            let st = s.borrow();
            Ok(st.snapshot()?.frame_stats.frame_time_ms)
        })?;
        globals.set("get_frame_time", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<f64> {
            let st = s.borrow();
            Ok(st.snapshot()?.music_position)
        })?;
        globals.set("get_music_position", f)?;
    }
    {
        // Seconds since the Unix epoch, as a float. The wall clock: what a
        // game measures time *away* with, which the frame clock cannot do
        // because it stops when the process does.
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<f64> {
            let st = s.borrow();
            Ok(st.snapshot()?.unix_time)
        })?;
        globals.set("get_unix_time", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<f32> {
            let st = s.borrow();
            Ok(st.snapshot()?.dt)
        })?;
        globals.set("get_delta_time", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<u32> {
            let st = s.borrow();
            Ok(st.snapshot()?.frame_stats.draw_calls)
        })?;
        globals.set("get_draw_calls", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<u32> {
            let st = s.borrow();
            Ok(st.snapshot()?.frame_stats.entity_count)
        })?;
        globals.set("get_entity_count", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<()> {
            let mut st = s.borrow_mut();
            let stats = st.snapshot()?.frame_stats.clone();
            // Same one-line shape the browser preamble logs, so script
            // output does not change with where it runs.
            let message = format!(
                "FPS: {:.1} | Frame: {:.2}ms | Entities: {} | Draws: {}",
                stats.fps, stats.frame_time_ms, stats.entity_count, stats.draw_calls
            );
            st.pending.push(BridgeCommand::Log { message });
            Ok(())
        })?;
        globals.set("log_stats", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, name: String| -> LuaResult<Option<u64>> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.entity_by_name(&name))
        })?;
        globals.set("find_by_name", f)?;
    }
    {
        // `Entity.find_child(parent, "Wheel")`: a part of a prefab tree.
        let s = state.clone();
        let f = lua.create_function(move |_, (parent, path): (u64, String)| -> LuaResult<Option<u64>> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.child_by_name(parent, &path))
        })?;
        globals.set("find_child", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |lua, tag: String| -> LuaResult<LuaTable> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            let mut ids: Vec<u64> = snap
                .tags
                .iter()
                .filter_map(|(id, t)| if t.contains(&tag) { Some(*id) } else { None })
                .collect();
            // Fallback for the production hot-path snapshot
            // (`snapshot_world_borrow`) which leaves `snap.tags` empty
            // and instead exposes the live world via `world_ptr`.
            // Without this fallback, scripts using `find_by_tag` get
            // an empty list even when entities have `Tags` components.
            if ids.is_empty()
                && let (Some(wp), false) = (snap.world_ptr, snap.entity_handles.is_empty())
            {
                let world = unsafe { &*wp };
                for (entity, tags) in world
                    .query::<&crate::ecs::components::Tags>()
                    .iter()
                {
                    if tags.0.contains(&tag) {
                        ids.push(entity.id() as u64);
                    }
                }
            }
            ids.sort_unstable();
            let table = lua.create_table()?;
            for (i, id) in ids.into_iter().enumerate() {
                table.set(i + 1, id)?;
            }
            Ok(table)
        })?;
        globals.set("find_by_tag", f)?;
    }
    // ---- Browser-name aliases --------------------------------------------
    // The browser preamble exposes the same two lookups under a second
    // pair of names, and `core_script_api.rs` documents those names — but
    // documenting is not installing, so a script written against
    // `get_entity_by_name` worked in the editor and was `nil` on native.
    //
    // These follow the BROWSER convention deliberately: a missing name
    // answers -1, not nil. `find_by_name` returns nil, and a script
    // tested in the editor checks `if e == -1`, which would silently
    // never fire if the alias answered nil instead.
    {
        let s = state.clone();
        let f = lua.create_function(move |_, name: String| -> LuaResult<i64> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.entity_by_name(&name).map(|id| id as i64).unwrap_or(-1))
        })?;
        globals.set("get_entity_by_name", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |lua, tag: String| -> LuaResult<mlua::Table> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            let mut ids: Vec<u64> = snap
                .tags
                .iter()
                .filter(|(_, tags)| tags.iter().any(|t| *t == tag))
                .map(|(id, _)| *id)
                .collect();
            ids.sort_unstable();
            let table = lua.create_table()?;
            for (i, id) in ids.into_iter().enumerate() {
                table.set(i + 1, id)?;
            }
            Ok(table)
        })?;
        globals.set("get_entities_by_tag", f)?;
    }
    // ---- Physics writes + reads ------------------------------------------
    // Documented in `core_script_api.rs` but never installed, so scripts
    // using them worked in the editor and were `nil` on native. The
    // `ScriptCommand` apply arms already existed; these add the bridge
    // path that reaches them.
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (gx, gy): (f32, f32)| -> LuaResult<()> {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetGravity { gx, gy });
            Ok(())
        })?;
        globals.set("set_gravity", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<(f32, f32)> {
            let st = s.borrow();
            Ok(st.snapshot()?.gravity)
        })?;
        globals.set("get_gravity", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, body_type): (u64, String)| -> LuaResult<()> {
                s.borrow_mut().pending.push(BridgeCommand::SetBodyType {
                    entity_id,
                    body_type,
                });
                Ok(())
            },
        )?;
        globals.set("set_body_type", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_,
                  (entity_name, target_name, joint_json): (String, String, String)|
                  -> LuaResult<()> {
                s.borrow_mut().pending.push(BridgeCommand::AddJoint {
                    entity_name,
                    target_name,
                    joint_json,
                });
                Ok(())
            },
        )?;
        globals.set("add_joint", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_name: String| -> LuaResult<()> {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::RemoveJoint { entity_name });
            Ok(())
        })?;
        globals.set("remove_joint", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_name, speed): (String, f32)| -> LuaResult<()> {
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetMotorSpeed { entity_name, speed });
                Ok(())
            },
        )?;
        globals.set("set_motor_speed", f)?;
    }
    {
        let s = state.clone();
        // `(a, b, started)` triples from the previous physics step, the
        // same list `on_collision_*` callbacks are dispatched from.
        let f = lua.create_function(move |lua, ()| -> LuaResult<mlua::Table> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            let out = lua.create_table_with_capacity(snap.contacts.len(), 0)?;
            for (i, (a, b, started, impulse, x, y)) in snap.contacts.iter().enumerate() {
                // 1-3 are what this always was; 4-6 are how hard and where,
                // appended so an existing script reading three fields is
                // unaffected.
                let row = lua.create_table_with_capacity(6, 0)?;
                row.raw_set(1, *a)?;
                row.raw_set(2, *b)?;
                row.raw_set(3, *started)?;
                row.raw_set(4, *impulse)?;
                row.raw_set(5, *x)?;
                row.raw_set(6, *y)?;
                out.raw_set(i + 1, row)?;
            }
            Ok(out)
        })?;
        globals.set("get_contacts", f)?;
    }
    #[cfg(feature = "physics")]
    {
        let s = state.clone();
        // raycast(ox, oy, dx, dy, max_dist) -> table or nil.
        //
        // **The last script function that was missing on native.** Every
        // other one had been wired and the docs' list of absentees was
        // stale; this one really was not there, because it is the only
        // read that cannot be answered from a snapshot. A ray is cast from
        // an origin and direction the script chooses, so there is nothing
        // to precompute, and the two obvious alternatives are both worse:
        // copying the collider set every frame is not cheap, and
        // reimplementing ray-vs-shape would drift away from the rapier the
        // rest of the engine collides with.
        //
        // So it borrows the live world through `snap.physics_ptr`, exactly
        // as `get_custom` borrows the ECS through `world_ptr`. That was
        // always the answer; what made it look impossible is that
        // `ScriptState` already had a `physics_world_ptr` and a working
        // `raycast` on it, and **nothing has ever called the method that
        // sets the pointer** - so the route existed, was dead, and read as
        // having been tried.
        //
        // The fields are `RayHit`'s own names, so the Rust and the Lua
        // agree without a translation table in between.
        let f = lua.create_function(
            move |lua, (ox, oy, dx, dy, max_dist): (f32, f32, f32, f32, f32)| -> LuaResult<Option<mlua::Table>> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                let Some(hit) = snap.raycast(ox, oy, dx, dy, max_dist) else {
                    return Ok(None);
                };
                let t = lua.create_table_with_capacity(0, 6)?;
                t.raw_set("entity_id", hit.entity_id)?;
                t.raw_set("x", hit.point_x)?;
                t.raw_set("y", hit.point_y)?;
                t.raw_set("normal_x", hit.normal_x)?;
                t.raw_set("normal_y", hit.normal_y)?;
                t.raw_set("distance", hit.distance)?;
                Ok(Some(t))
            },
        )?;
        globals.set("raycast", f)?;
    }
    // Shape queries (`physics::query`): what overlaps a circle, a box or a
    // point - every entity a collider of which touches it, each once, in
    // order of id - and a circle swept along a line, answered as `raycast`.
    #[cfg(feature = "physics")]
    {
        use crate::physics::query::QueryShape;
        let ids_table = |lua: &Lua, ids: Vec<u64>| -> LuaResult<mlua::Table> {
            let t = lua.create_table_with_capacity(ids.len(), 0)?;
            for (i, id) in ids.into_iter().enumerate() {
                t.raw_set(i + 1, id)?;
            }
            Ok(t)
        };
        {
            let s = state.clone();
            let f = lua.create_function(move |lua, (x, y, radius): (f32, f32, f32)| {
                let st = s.borrow();
                let snap = st.snapshot()?;
                ids_table(lua, snap.overlap(&QueryShape::Circle { x, y, radius }))
            })?;
            globals.set("overlap_circle", f)?;
        }
        {
            let s = state.clone();
            let f = lua.create_function(
                move |lua, (x, y, width, height, rotation): (f32, f32, f32, f32, Option<f32>)| {
                    let st = s.borrow();
                    let snap = st.snapshot()?;
                    let q = QueryShape::Box { x, y, width, height, rotation: rotation.unwrap_or(0.0) };
                    ids_table(lua, snap.overlap(&q))
                },
            )?;
            globals.set("overlap_box", f)?;
        }
        {
            let s = state.clone();
            let f = lua.create_function(move |lua, (x, y): (f32, f32)| {
                let st = s.borrow();
                let snap = st.snapshot()?;
                ids_table(lua, snap.overlap(&QueryShape::Point { x, y }))
            })?;
            globals.set("overlap_point", f)?;
        }
        {
            let s = state.clone();
            let f = lua.create_function(
                move |lua, (ox, oy, dx, dy, radius, max_dist): (f32, f32, f32, f32, f32, f32)| -> LuaResult<Option<mlua::Table>> {
                    let st = s.borrow();
                    let snap = st.snapshot()?;
                    let Some(hit) = snap.circle_cast(ox, oy, dx, dy, radius, max_dist) else {
                        return Ok(None);
                    };
                    let t = lua.create_table_with_capacity(0, 6)?;
                    t.raw_set("entity_id", hit.entity_id)?;
                    t.raw_set("x", hit.point_x)?;
                    t.raw_set("y", hit.point_y)?;
                    t.raw_set("normal_x", hit.normal_x)?;
                    t.raw_set("normal_y", hit.normal_y)?;
                    t.raw_set("distance", hit.distance)?;
                    Ok(Some(t))
                },
            )?;
            globals.set("circle_cast", f)?;
        }
    }
    {
        let s = state.clone();
        // find_in_radius(x, y, radius[, tag]) -> array of entity ids whose
        // Transform position is within `radius` of (x, y). When `tag` is
        // supplied, only entities carrying that tag are returned. Read-only:
        // returns immediately from the per-frame snapshot (no command queued).
        let f = lua.create_function(
            move |lua, (cx, cy, radius, tag): (f32, f32, f32, Option<String>)| -> LuaResult<LuaTable> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                let r2 = radius * radius;
                let mut ids: Vec<u64> = Vec::new();
                // Hot path: iterate the live world when the borrow snapshot
                // exposes it (production frame loop). Mirrors the find_by_tag
                // world_ptr fallback.
                if let Some(wp) = snap.world_ptr {
                    let world = unsafe { &*wp };
                    for (entity, transform) in world
                        .query::<&crate::ecs::components::Transform>()
                        .iter()
                    {
                        if let Some(ref t) = tag {
                            let has = world
                                .get::<&crate::ecs::components::Tags>(entity)
                                .map(|tags| tags.0.contains(t))
                                .unwrap_or(false);
                            if !has {
                                continue;
                            }
                        }
                        let dx = transform.position.x - cx;
                        let dy = transform.position.y - cy;
                        if dx * dx + dy * dy <= r2 {
                            ids.push(entity.id() as u64);
                        }
                    }
                } else {
                    // Fallback: eager snapshot maps (tests / parity harness).
                    for (id, &(px, py)) in snap.positions.iter() {
                        if let Some(ref t) = tag
                            && !snap.has_tag(*id, t)
                        {
                            continue;
                        }
                        let dx = px - cx;
                        let dy = py - cy;
                        if dx * dx + dy * dy <= r2 {
                            ids.push(*id);
                        }
                    }
                }
                ids.sort_unstable();
                let table = lua.create_table()?;
                for (i, id) in ids.into_iter().enumerate() {
                    table.set(i + 1, id)?;
                }
                Ok(table)
            },
        )?;
        globals.set("find_in_radius", f)?;
    }

    // --- Phase 5.2: sprite + material ---------------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, r, g, b, a): (u64, f32, f32, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetSpriteColor { entity_id, r, g, b, a });
            Ok(())
        })?;
        globals.set("set_sprite_color", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y): (u64, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetSpriteAnchor { entity_id, x, y });
            Ok(())
        })?;
        globals.set("set_sprite_anchor", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, w, h): (u64, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetSpriteSize { entity_id, w, h });
            Ok(())
        })?;
        globals.set("set_sprite_size", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, z): (u64, f64)| {
            s.borrow_mut().pending.push(BridgeCommand::SetZOrder { entity_id, z: z as i32 });
            Ok(())
        })?;
        globals.set("set_z_order", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, r, g, b, a): (u64, f32, f32, f32, Option<f32>)| {
            s.borrow_mut().pending.push(BridgeCommand::SetTextColor { entity_id, r, g, b, a: a.unwrap_or(1.0) });
            Ok(())
        })?;
        globals.set("set_text_color", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, on): (u64, bool)| {
            s.borrow_mut().pending.push(BridgeCommand::SetTrailEmitting { entity_id, on });
            Ok(())
        })?;
        globals.set("set_trail_emitting", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, vertices, indices): (u64, LuaValue, LuaValue)| {
            s.borrow_mut().pending.push(BridgeCommand::SetMesh {
                entity_id,
                vertices: lua_to_json(&vertices),
                indices: lua_to_json(&indices),
            });
            Ok(())
        })?;
        globals.set("set_mesh", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| {
            s.borrow_mut().pending.push(BridgeCommand::ClearTrail { entity_id });
            Ok(())
        })?;
        globals.set("clear_trail", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y, radius): (u64, f32, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::CarveSprite { entity_id, x, y, radius });
            Ok(())
        })?;
        globals.set("carve_sprite", f)?;
    }
    {
        // `paint_sprite(e, x, y, size, r, g, b, a, shape, rotation, blend)` -
        // `Sprite.paint`: a stamp in a paintable sprite (`ecs::paint`).
        let s = state.clone();
        #[allow(clippy::type_complexity)]
        let f = lua.create_function(
            move |_,
                  (entity_id, x, y, size, r, g, b, a, shape, rotation, blend): (
                u64,
                f32,
                f32,
                f32,
                Option<f32>,
                Option<f32>,
                Option<f32>,
                Option<f32>,
                Option<String>,
                Option<f32>,
                Option<String>,
            )| {
                s.borrow_mut().pending.push(BridgeCommand::PaintSprite {
                    entity_id,
                    x,
                    y,
                    size,
                    color: [r.unwrap_or(1.0), g.unwrap_or(1.0), b.unwrap_or(1.0), a.unwrap_or(1.0)],
                    shape: shape.unwrap_or_default(),
                    rotation: rotation.unwrap_or(0.0),
                    depth: 0.0,
                    blend: blend.unwrap_or_default(),
                });
                Ok(())
            },
        )?;
        globals.set("paint_sprite", f)?;
    }
    {
        // `press_sprite(e, x, y, size, depth, shape, rotation)` - `Sprite.press`:
        // a dent in a paintable sprite's normal map (`ecs::paint`).
        let s = state.clone();
        #[allow(clippy::type_complexity)]
        let f = lua.create_function(
            move |_,
                  (entity_id, x, y, size, depth, shape, rotation): (
                u64,
                f32,
                f32,
                f32,
                Option<f32>,
                Option<String>,
                Option<f32>,
            )| {
                s.borrow_mut().pending.push(BridgeCommand::PaintSprite {
                    entity_id,
                    x,
                    y,
                    size,
                    color: [1.0; 4],
                    shape: shape.unwrap_or_else(|| "disc".to_string()),
                    rotation: rotation.unwrap_or(0.0),
                    depth: depth.unwrap_or(1.0),
                    blend: String::new(),
                });
                Ok(())
            },
        )?;
        globals.set("press_sprite", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, vx, vy, drop): (u64, f32, f32, Option<bool>)| {
            s.borrow_mut().pending.push(BridgeCommand::MoveCharacter { entity_id, vx, vy, drop_through: drop });
            Ok(())
        })?;
        globals.set("move_character", f)?;
    }
    {
        // get_character(e) -> what its CharacterController2D did last step,
        // or nil when it has none.
        let s = state.clone();
        let f = lua.create_function(move |lua, entity_id: u64| -> LuaResult<Option<LuaTable>> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            let Some(c) = snap.characters.get(&entity_id) else { return Ok(None) };
            let t = lua.create_table()?;
            t.set("grounded", c.grounded)?;
            t.set("ground_nx", c.ground_normal[0])?;
            t.set("ground_ny", c.ground_normal[1])?;
            t.set("ground_entity", c.ground_entity)?;
            t.set("wall", c.wall)?;
            t.set("ceiling", c.ceiling)?;
            t.set("sliding", c.sliding)?;
            t.set("vx", c.moved[0])?;
            t.set("vy", c.moved[1])?;
            t.set("touching", lua.create_sequence_from(c.touching.iter().copied())?)?;
            Ok(Some(t))
        })?;
        globals.set("get_character", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, atlas_id, region_name): (u64, String, String)| {
                s.borrow_mut().pending.push(BridgeCommand::SetSpriteRegion {
                    entity_id,
                    atlas_id,
                    region_name,
                });
                Ok(())
            },
        )?;
        globals.set("set_sprite_region", f)?;
        // `Sprite.set_label(entity, label, category)`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, label, category): (u64, String, Option<String>)| {
            s.borrow_mut().pending.push(BridgeCommand::SetSpriteLabel { entity_id, label, category });
            Ok(())
        })?;
        globals.set("set_sprite_label", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_,
                  (entity_id, x0, y0, x1, y1, x2, y2, x3, y3): (
                u64,
                f32,
                f32,
                f32,
                f32,
                f32,
                f32,
                f32,
                f32,
            )| {
                s.borrow_mut().pending.push(BridgeCommand::SetSpriteCorners {
                    entity_id,
                    corners: Some([[x0, y0], [x1, y1], [x2, y2], [x3, y3]]),
                });
                Ok(())
            },
        )?;
        globals.set("set_sprite_corners", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, entity_id: u64| -> LuaResult<(f32, f32, f32, f32)> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                let c = snap.get_sprite_color(entity_id).unwrap_or([1.0, 1.0, 1.0, 1.0]);
                Ok((c[0], c[1], c[2], c[3]))
            },
        )?;
        globals.set("get_sprite_color", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<(f32, f32)> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            let a = snap.get_sprite_anchor(entity_id).unwrap_or([0.5, 0.5]);
            Ok((a[0], a[1]))
        })?;
        globals.set("get_sprite_anchor", f)?;
    }
    {
        let s = state.clone();
        // Named atlas point in world space, or (0, 0) when the sprite has
        // no such point - matching the browser preamble, which answers
        // [0, 0] rather than nil so callers can use the result directly.
        let f = lua.create_function(
            move |_, (entity_id, name): (u64, String)| -> LuaResult<(f32, f32)> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                Ok(snap
                    .sprite_points
                    .get(&entity_id)
                    .and_then(|pts| pts.get(&name))
                    .copied()
                    .unwrap_or((0.0, 0.0)))
            },
        )?;
        globals.set("get_sprite_point", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, material_id): (u64, String)| {
            s.borrow_mut().pending.push(BridgeCommand::SetMaterial { entity_id, material_id });
            Ok(())
        })?;
        globals.set("set_material", f)?;
    }
    {
        let s = state.clone();
        // `nil` (or "") takes the normal map away.
        let f = lua.create_function(move |_, (entity_id, normal_map_id): (u64, Option<String>)| {
            s.borrow_mut().pending.push(BridgeCommand::SetNormalMap { entity_id, normal_map_id });
            Ok(())
        })?;
        globals.set("set_normal_map", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<Option<String>> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.get_material(entity_id))
        })?;
        globals.set("get_material", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, name, value): (u64, String, f32)| {
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetMaterialUniformFloat { entity_id, name, value });
                Ok(())
            },
        )?;
        globals.set("set_material_uniform_float", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, name, x, y, z, w): (u64, String, f32, f32, f32, f32)| {
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetMaterialUniformVec4 {
                        entity_id,
                        name,
                        value: [x, y, z, w],
                    });
                Ok(())
            },
        )?;
        globals.set("set_material_uniform_vec4", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, name, x, y): (u64, String, f32, f32)| {
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetMaterialUniformVec2 {
                        entity_id,
                        name,
                        value: [x, y],
                    });
                Ok(())
            },
        )?;
        globals.set("set_material_uniform_vec2", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, name, value): (u64, String, i32)| {
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetMaterialUniformInt {
                        entity_id,
                        name,
                        value,
                    });
                Ok(())
            },
        )?;
        globals.set("set_material_uniform_int", f)?;
    }

    // --- Phase 5.3: input + action bindings ---------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(move |_, key: String| -> LuaResult<bool> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.keys_pressed.contains(&key))
        })?;
        globals.set("is_key_pressed", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, key: String| -> LuaResult<bool> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.keys_just_pressed.contains(&key))
        })?;
        globals.set("is_key_just_pressed", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, action: String| -> LuaResult<bool> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.actions_pressed.contains(&action))
        })?;
        globals.set("is_action_pressed", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, action: String| -> LuaResult<bool> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.actions_just_pressed.contains(&action))
        })?;
        globals.set("is_action_just_pressed", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, action: String| -> LuaResult<bool> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.actions_just_released.contains(&action))
        })?;
        globals.set("is_action_just_released", f)?;
    }
    // 2.5 / BUGS #9 — analog input. `action_axis_value` had existed in
    // `InputState`, with tests, reachable from no script and no system.
    {
        let s = state.clone();
        let f = lua.create_function(move |_, action: String| -> LuaResult<f32> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.action_axes.get(&action).copied().unwrap_or(0.0))
        })?;
        globals.set("get_action_axis", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<u32> {
            let st = s.borrow();
            Ok(st.snapshot()?.pads.len() as u32)
        })?;
        globals.set("get_gamepad_count", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, pad: usize| -> LuaResult<bool> {
            let st = s.borrow();
            Ok(st.snapshot()?.pads.get(pad).is_some_and(|p| p.connected))
        })?;
        globals.set("is_gamepad_connected", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (pad, axis): (usize, usize)| -> LuaResult<f32> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap
                .pads
                .get(pad)
                .filter(|p| p.connected)
                .and_then(|p| p.axes.get(axis).copied())
                .unwrap_or(0.0))
        })?;
        globals.set("get_gamepad_axis", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (pad, button): (usize, usize)| -> LuaResult<bool> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap
                .pads
                .get(pad)
                .filter(|p| p.connected)
                .is_some_and(|p| p.buttons.contains(&button)))
        })?;
        globals.set("is_gamepad_button_pressed", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<(f32, f32)> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.mouse_position)
        })?;
        globals.set("get_mouse_position", f)?;
    }
    {
        let s = state.clone();
        // Same conversion the browser preamble does: normalised offset from
        // screen centre, scaled by the camera's visible extents over zoom,
        // with Y flipped because pixel-Y grows downward and world-Y up. With
        // no camera reported (zoom 0) it degrades to screen space rather
        // than dividing by zero.
        let f = lua.create_function(move |_, ()| -> LuaResult<(f32, f32)> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            let (mx, my) = snap.mouse_position;
            Ok(snapshot_screen_to_world(snap, mx, my))
        })?;
        globals.set("get_mouse_world_position", f)?;
    }
    {
        let s = state.clone();
        // Every finger: `{ id, x, y, start_x, start_y, wx, wy, phase,
        // on_control }`, in the order they landed. Screen coordinates are
        // `get_mouse_position`'s; `wx, wy` are the world point under the
        // finger, by the same conversion as `get_mouse_world_position`.
        let f = lua.create_function(move |lua, ()| -> LuaResult<mlua::Table> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            let out = lua.create_table_with_capacity(snap.touches.len(), 0)?;
            for (i, t) in snap.touches.iter().enumerate() {
                let row = lua.create_table()?;
                let (wx, wy) = snapshot_screen_to_world(snap, t.position.0, t.position.1);
                row.set("id", t.id as f64)?;
                row.set("x", t.position.0)?;
                row.set("y", t.position.1)?;
                row.set("start_x", t.start.0)?;
                row.set("start_y", t.start.1)?;
                row.set("wx", wx)?;
                row.set("wy", wy)?;
                row.set("phase", t.stage.name())?;
                row.set("on_control", t.on_control)?;
                out.raw_set(i + 1, row)?;
            }
            Ok(out)
        })?;
        globals.set("get_touches", f)?;
    }
    {
        // `get_gestures()`: this frame's taps, swipes, pans, pinches and
        // twists (`input::gestures`), with the world point under each and a
        // pan's move in world units, as `get_touches` gives them.
        let s = state.clone();
        let f = lua.create_function(move |lua, ()| -> LuaResult<mlua::Table> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            let out = lua.create_table_with_capacity(snap.gestures.len(), 0)?;
            for (i, g) in snap.gestures.iter().enumerate() {
                let row = lua.create_table()?;
                let (wx, wy) = snapshot_screen_to_world(snap, g.x, g.y);
                let (ex, ey) = snapshot_screen_to_world(snap, g.x + g.dx, g.y + g.dy);
                row.set("kind", g.kind.name())?;
                row.set("x", g.x)?;
                row.set("y", g.y)?;
                row.set("wx", wx)?;
                row.set("wy", wy)?;
                row.set("dx", g.dx)?;
                row.set("dy", g.dy)?;
                row.set("wdx", ex - wx)?;
                row.set("wdy", ey - wy)?;
                row.set("scale", g.scale)?;
                row.set("angle", g.angle)?;
                row.set("speed", g.speed)?;
                row.set("phase", g.phase)?;
                out.raw_set(i + 1, row)?;
            }
            Ok(out)
        })?;
        globals.set("get_gestures", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<(f32, f32)> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.screen_size)
        })?;
        globals.set("get_screen_size", f)?;
    }
    {
        let s = state.clone();
        // get_camera_zoom() -> the active camera's zoom, 1.0 when no camera
        // has been reported.
        //
        // `set_camera_zoom` has existed on its own since the camera could
        // zoom at all, which was fine while the game was the only thing
        // setting it. `CameraFollow.frame_tag` changed that: the **engine**
        // now chooses the zoom while framing a set, so a game that wants to
        // know how far out the view is - to scale a HUD, to work out what is
        // on screen - had no way to ask. A setter without a getter is only
        // an asymmetry until something else starts writing the value.
        //
        // The number is already published to every runtime as part of
        // `camera_view` for `get_mouse_world_position`, so this is a read of
        // state that was there all along.
        let f = lua.create_function(move |_, ()| -> LuaResult<f32> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            let zoom = snap.camera_view.4;
            Ok(if zoom > 0.0 { zoom } else { 1.0 })
        })?;
        globals.set("get_camera_zoom", f)?;
    }
    {
        // `Camera.get_position()`: the rendered centre, as the host reports
        // it in `camera_view` - where a following camera actually is, which
        // the camera entity's own transform is not.
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<(f32, f32)> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok((snap.camera_view.0, snap.camera_view.1))
        })?;
        globals.set("get_camera_position", f)?;
        // `Camera.set_follow(entity, field, value)`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetCameraFollow { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_camera_follow", f)?;
        // `Camera.set(entity, field, value)`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetCameraField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_camera_field", f)?;
        // `Camera.set_screen_rect(entity, x, y, w, h)`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y, w, h): (u64, f32, f32, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetCameraScreenRect { entity_id, rect: [x, y, w, h] });
            Ok(())
        })?;
        globals.set("set_camera_screen_rect", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<bool> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.mouse_left_just_pressed)
        })?;
        globals.set("is_mouse_left_just_pressed", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<bool> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.mouse_left)
        })?;
        globals.set("is_mouse_left_pressed", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<bool> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.mouse_left_just_released)
        })?;
        globals.set("is_mouse_left_just_released", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<bool> {
            let st = s.borrow();
            Ok(st.snapshot()?.mouse_right_just_pressed)
        })?;
        globals.set("is_mouse_right_just_pressed", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<bool> {
            let st = s.borrow();
            Ok(st.snapshot()?.mouse_right)
        })?;
        globals.set("is_mouse_right_pressed", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<bool> {
            let st = s.borrow();
            Ok(st.snapshot()?.mouse_right_just_released)
        })?;
        globals.set("is_mouse_right_just_released", f)?;
    }
    // The middle button, the same three ways (Oct 2026).
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<bool> { Ok(s.borrow().snapshot()?.mouse_middle_just_pressed) })?;
        globals.set("is_mouse_middle_just_pressed", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<bool> { Ok(s.borrow().snapshot()?.mouse_middle) })?;
        globals.set("is_mouse_middle_pressed", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<bool> { Ok(s.borrow().snapshot()?.mouse_middle_just_released) })?;
        globals.set("is_mouse_middle_just_released", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<f32> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.mouse_wheel_delta)
        })?;
        globals.set("get_mouse_wheel", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<(f32, f32)> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.mouse_delta)
        })?;
        globals.set("get_mouse_delta", f)?;
    }

    // --- Phase 5.4: physics --------------------------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, fx, fy): (u64, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::ApplyForce { entity_id, fx, fy });
            Ok(())
        })?;
        globals.set("apply_force", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, ix, iy): (u64, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::ApplyImpulse { entity_id, ix, iy });
            Ok(())
        })?;
        globals.set("apply_impulse", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, vx, vy): (u64, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetVelocity { entity_id, vx, vy });
            Ok(())
        })?;
        globals.set("set_velocity", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, omega): (u64, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetAngularVelocity { entity_id, omega });
            Ok(())
        })?;
        globals.set("set_angular_velocity", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<(f32, f32)> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.get_velocity(entity_id).unwrap_or((0.0, 0.0)))
        })?;
        globals.set("get_velocity", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<f32> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.get_angular_velocity(entity_id))
        })?;
        globals.set("get_angular_velocity", f)?;
    }
    {
        // Discrete contact accessors keep the cross-platform shape simple:
        // returning a Vec<(u64, u64, bool)> across both Lua and JS would force
        // adapter-specific table/array packing. Discrete getters mirror what
        // the host engines already expose to scripts.
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<usize> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.contacts.len())
        })?;
        globals.set("contact_count", f)?;
    }
    {
        let s = state.clone();
        // Returns `(a, b, started, impulse, x, y)`. The first three are what
        // it always returned, so a script unpacking three values is
        // unaffected; the rest say how hard the two hit and where.
        let f = lua.create_function(move |_, index: usize| -> LuaResult<(u64, u64, bool, f32, f32, f32)> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            // Lua-style 1-based index in scripts; convert to 0-based here.
            let i = index.saturating_sub(1);
            let row = snap
                .contacts
                .get(i)
                .copied()
                .ok_or_else(|| LuaError::RuntimeError(format!("contact index {index} out of range")))?;
            Ok(row)
        })?;
        globals.set("get_contact", f)?;
    }

    // --- Phase 5.5: audio ----------------------------------------------------
    {
        // play_sound(clip_id, volume?, pitch?, pan?, loop?) -> sound id.
        // Returns the pre-allocated id so scripts can pass it to
        // stop_sound / set_sound_volume / set_sound_pitch in the same frame;
        // the host aliases it to the backend's sound (`audio::music::alias`).
        // `pan` is -1.0 hard left to 1.0 hard right and defaults to centre.
        let s = state.clone();
        let f = lua.create_function(
            move |_,
                  (clip_id, volume, pitch, pan, looping): (
                String,
                Option<f32>,
                Option<f32>,
                Option<f32>,
                Option<bool>,
            )|
                  -> LuaResult<u64> {
                let mut st = s.borrow_mut();
                let sound_id = st.alloc_sound_id()?;
                st.pending.push(BridgeCommand::PlaySound {
                    clip_id,
                    volume: volume.unwrap_or(1.0),
                    pitch: pitch.unwrap_or(1.0),
                    pan: pan.unwrap_or(0.0).clamp(-1.0, 1.0),
                    sound_id,
                    looping: looping.unwrap_or(false),
                });
                Ok(sound_id)
            },
        )?;
        globals.set("play_sound", f)?;
    }
    {
        // play_sound_at(clip_id, x, y[, volume[, pitch]]) -> sound id. A
        // one-shot heard from where it happens (`audio::spatial`): its
        // volume and pan worked out here from the live listener, then played
        // as `play_sound` plays.
        let s = state.clone();
        let f = lua.create_function(
            move |_, (clip_id, x, y, volume, pitch): (String, f32, f32, Option<f32>, Option<f32>)| -> LuaResult<u64> {
                let mut st = s.borrow_mut();
                let (gain, pan) = st
                    .snapshot()
                    .ok()
                    .and_then(|snap| snap.world().map(|w| crate::audio::spatial::at_point(w, x, y)))
                    .unwrap_or((1.0, 0.0));
                let sound_id = st.alloc_sound_id()?;
                st.pending.push(BridgeCommand::PlaySound {
                    clip_id,
                    volume: volume.unwrap_or(1.0) * gain,
                    pitch: pitch.unwrap_or(1.0),
                    pan,
                    sound_id,
                    looping: false,
                });
                Ok(sound_id)
            },
        )?;
        globals.set("play_sound_at", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (clip_id, volume, looping): (String, Option<f32>, Option<bool>)| {
                s.borrow_mut().pending.push(BridgeCommand::PlayMusic {
                    clip_id,
                    volume: volume.unwrap_or(1.0),
                    looping: looping.unwrap_or(true),
                });
                Ok(())
            },
        )?;
        globals.set("play_music", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, handle_id: u64| {
            s.borrow_mut().pending.push(BridgeCommand::StopSound { handle_id });
            Ok(())
        })?;
        globals.set("stop_sound", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| {
            s.borrow_mut().pending.push(BridgeCommand::StopMusic);
            Ok(())
        })?;
        globals.set("stop_music", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| {
            s.borrow_mut().pending.push(BridgeCommand::PauseMusic);
            Ok(())
        })?;
        globals.set("pause_music", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| {
            s.borrow_mut().pending.push(BridgeCommand::ResumeMusic);
            Ok(())
        })?;
        globals.set("resume_music", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| {
            s.borrow_mut().pending.push(BridgeCommand::StopAllSounds);
            Ok(())
        })?;
        globals.set("stop_all_sounds", f)?;
    }
    {
        // `stop_sound_by_clip(clip_id)` — stops every live instance of one
        // clip. `stop_sound(handle)` cannot serve here: play_sound is deferred,
        // so no handle exists when the script returns.
        let s = state.clone();
        let f = lua.create_function(move |_, clip_id: String| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::StopSoundByClip { clip_id });
            Ok(())
        })?;
        globals.set("stop_sound_by_clip", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| {
            s.borrow_mut().pending.push(BridgeCommand::PauseAllSounds);
            Ok(())
        })?;
        globals.set("pause_all_sounds", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| {
            s.borrow_mut().pending.push(BridgeCommand::ResumeAllSounds);
            Ok(())
        })?;
        globals.set("resume_all_sounds", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (handle_id, volume): (u64, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetSoundVolume { handle_id, volume });
            Ok(())
        })?;
        globals.set("set_sound_volume", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, volume: f32| {
            s.borrow_mut().pending.push(BridgeCommand::SetMusicVolume { volume });
            Ok(())
        })?;
        globals.set("set_music_volume", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, volume: f32| {
            s.borrow_mut().pending.push(BridgeCommand::SetMasterVolume { volume });
            Ok(())
        })?;
        globals.set("set_master_volume", f)?;
    }

    // --- Phase 5.6: animation ------------------------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, clip_name): (u64, String)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::PlayAnimation { entity_id, clip_name });
            Ok(())
        })?;
        globals.set("play_animation", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::StopAnimation { entity_id });
            Ok(())
        })?;
        globals.set("stop_animation", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, speed): (u64, f32)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetAnimationSpeed { entity_id, speed });
            Ok(())
        })?;
        globals.set("set_animation_speed", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<bool> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.is_animation_playing(entity_id))
        })?;
        globals.set("is_animation_playing", f)?;
    }
    {
        // Returns "" when no clip is active. Empty string is the "absent"
        // sentinel both adapters can produce uniformly without optional types.
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<String> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.get_current_animation(entity_id))
        })?;
        globals.set("get_current_animation", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, param, value): (u64, String, bool)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetAnimBool { entity_id, param, value });
            Ok(())
        })?;
        globals.set("set_anim_bool", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, param, value): (u64, String, f32)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetAnimFloat { entity_id, param, value });
            Ok(())
        })?;
        globals.set("set_anim_float", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, param): (u64, String)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetAnimTrigger { entity_id, param });
            Ok(())
        })?;
        globals.set("set_anim_trigger", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, state_name): (u64, String)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::StartStateMachine { entity_id, state_name });
            Ok(())
        })?;
        globals.set("start_state_machine", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<String> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.get_anim_state(entity_id))
        })?;
        globals.set("get_anim_state", f)?;
    }
    {
        // tween_position(entity, x, y, dur, easing?) -> tween_id (u32 as u64).
        // Mirrors EngineApi::tween_position; end_value packs (x, y, 0, 0) so
        // the legacy `[f32; 4]` shape is preserved when translated.
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, x, y, duration, easing): (u64, f32, f32, f32, Option<String>)| -> LuaResult<u64> {
                let mut st = s.borrow_mut();
                let tween_id = st.alloc_tween_id()?;
                st.pending.push(BridgeCommand::StartTween {
                    entity_id,
                    target: "position".into(),
                    end_value: [x, y, 0.0, 0.0],
                    duration,
                    easing: easing.unwrap_or_else(|| "linear".into()),
                    tween_id,
                });
                Ok(tween_id as u64)
            },
        )?;
        globals.set("tween_position", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, radians, duration, easing): (u64, f32, f32, Option<String>)| -> LuaResult<u64> {
                let mut st = s.borrow_mut();
                let tween_id = st.alloc_tween_id()?;
                st.pending.push(BridgeCommand::StartTween {
                    entity_id,
                    target: "rotation".into(),
                    end_value: [radians, 0.0, 0.0, 0.0],
                    duration,
                    easing: easing.unwrap_or_else(|| "linear".into()),
                    tween_id,
                });
                Ok(tween_id as u64)
            },
        )?;
        globals.set("tween_rotation", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, x, y, duration, easing): (u64, f32, f32, f32, Option<String>)| -> LuaResult<u64> {
                let mut st = s.borrow_mut();
                let tween_id = st.alloc_tween_id()?;
                st.pending.push(BridgeCommand::StartTween {
                    entity_id,
                    target: "scale".into(),
                    end_value: [x, y, 0.0, 0.0],
                    duration,
                    easing: easing.unwrap_or_else(|| "linear".into()),
                    tween_id,
                });
                Ok(tween_id as u64)
            },
        )?;
        globals.set("tween_scale", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, r, g, b, a, duration, easing): (u64, f32, f32, f32, f32, f32, Option<String>)| -> LuaResult<u64> {
                let mut st = s.borrow_mut();
                let tween_id = st.alloc_tween_id()?;
                st.pending.push(BridgeCommand::StartTween {
                    entity_id,
                    target: "sprite_color".into(),
                    end_value: [r, g, b, a],
                    duration,
                    easing: easing.unwrap_or_else(|| "linear".into()),
                    tween_id,
                });
                Ok(tween_id as u64)
            },
        )?;
        globals.set("tween_color", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, tween_id: u32| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::CancelTween { tween_id });
            Ok(())
        })?;
        globals.set("cancel_tween", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::CancelAllTweens { entity_id });
            Ok(())
        })?;
        globals.set("cancel_all_tweens", f)?;
    }

    // --- Phase 5.7: scene ----------------------------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(move |_, path: String| {
            s.borrow_mut().pending.push(BridgeCommand::LoadScene { path });
            Ok(())
        })?;
        globals.set("load_scene", f)?;
    }
    {
        // `scene_transition(path, style, seconds, texture)` -
        // `Scene.transition`: arm the process-wide transition
        // (`renderer::transition`), then load as `load_scene` does.
        let s = state.clone();
        let f = lua.create_function(move |_, (path, style, seconds, texture): (String, Option<String>, Option<f32>, Option<String>)| {
            crate::renderer::transition::arm(style.as_deref().unwrap_or("fade"), seconds.unwrap_or(0.6), texture.as_deref().unwrap_or(""));
            s.borrow_mut().pending.push(BridgeCommand::LoadScene { path });
            Ok(())
        })?;
        globals.set("scene_transition", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, path: String| {
            s.borrow_mut().pending.push(BridgeCommand::PushScene { path });
            Ok(())
        })?;
        globals.set("push_scene", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| {
            s.borrow_mut().pending.push(BridgeCommand::QuitGame);
            Ok(())
        })?;
        globals.set("quit_game", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, scale: f32| {
            s.borrow_mut().pending.push(BridgeCommand::SetTimeScale { scale });
            Ok(())
        })?;
        globals.set("set_time_scale", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| {
            s.borrow_mut().pending.push(BridgeCommand::PopScene);
            Ok(())
        })?;
        globals.set("pop_scene", f)?;
    }
    // Slice 12.A — fade transition overlay. `fade_in(dur)` or
    // `fade_in(dur, r, g, b)` (RGB defaults to black when omitted).
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (duration_s, r, g, b): (f32, Option<f32>, Option<f32>, Option<f32>)| {
                let color = [r.unwrap_or(0.0), g.unwrap_or(0.0), b.unwrap_or(0.0)];
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::FadeIn { duration_s, color });
                Ok(())
            },
        )?;
        globals.set("fade_in", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (duration_s, r, g, b): (f32, Option<f32>, Option<f32>, Option<f32>)| {
                let color = [r.unwrap_or(0.0), g.unwrap_or(0.0), b.unwrap_or(0.0)];
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::FadeOut { duration_s, color });
                Ok(())
            },
        )?;
        globals.set("fade_out", f)?;
    }
    // P1.3 — screen flash. `screen_flash(dur)` defaults to opaque white;
    // `screen_flash(dur, r, g, b)` keeps alpha=1; `screen_flash(dur, r, g, b, a)`
    // controls the initial opacity.
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_,
                  (duration_s, r, g, b, a): (
                f32,
                Option<f32>,
                Option<f32>,
                Option<f32>,
                Option<f32>,
            )| {
                let color = [
                    r.unwrap_or(1.0),
                    g.unwrap_or(1.0),
                    b.unwrap_or(1.0),
                    a.unwrap_or(1.0),
                ];
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::ScreenFlash { duration_s, color });
                Ok(())
            },
        )?;
        globals.set("screen_flash", f)?;
    }
    {
        // The name the host recorded when it loaded the scene.
        let f = lua.create_function(|_, ()| -> LuaResult<String> { Ok(crate::scene::current_name()) })?;
        globals.set("get_current_scene_name", f)?;
    }

    // --- Dynamic script loading (P7.8) -----------------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, path): (u64, String)| {
            s.borrow_mut().pending.push(BridgeCommand::LoadScript { entity_id, path });
            Ok(())
        })?;
        globals.set("load_script", f)?;
    }

    // --- Phase 5.8: events & timers ----------------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (name, sender, data): (String, u64, LuaValue)| {
                let data = event_data_text(&data);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SendEvent { name, sender, data });
                Ok(())
            },
        )?;
        globals.set("send_event", f)?;
    }
    // Phase C: targeted send_event_to — same payload as send_event but only
    // the `target` entity's listener fires. Runner-side dispatch is bridge-only.
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_,
                  (name, sender, target, data): (String, u64, u64, LuaValue)| {
                let data = event_data_text(&data);
                s.borrow_mut().pending.push(BridgeCommand::SendEventTo {
                    name,
                    sender,
                    target,
                    data,
                });
                Ok(())
            },
        )?;
        globals.set("send_event_to", f)?;
    }
    // Phase C: synchronous-style call(caller, target, "method", ...args).
    // Args are marshalled through bridge `Value` (number/string/bool/nil for
    // v1 — tables not supported). Returns nil; future versions may marshal a
    // return value back. Recursion-depth cap lives on the runner.
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_,
                  (caller, target, method, args): (
                u64,
                u64,
                String,
                mlua::Variadic<LuaValue>,
            )| {
                use crate::scripting::bridge::value::Value as V;
                let mut bridge_args: Vec<V> = Vec::with_capacity(args.len());
                for a in args.iter() {
                    let v = match a {
                        LuaValue::Nil => V::Nil,
                        LuaValue::Boolean(b) => V::Bool(*b),
                        LuaValue::Integer(i) => V::Number(*i as f64),
                        LuaValue::Number(n) => V::Number(*n),
                        LuaValue::String(s) => V::String(s.to_str()?.to_string()),
                        other => {
                            return Err(LuaError::RuntimeError(format!(
                                "call(): unsupported arg type '{}' (only nil/bool/number/string in v1)",
                                other.type_name()
                            )));
                        }
                    };
                    bridge_args.push(v);
                }
                s.borrow_mut().pending.push(BridgeCommand::CallScriptMethod {
                    caller,
                    target,
                    method,
                    args: bridge_args,
                });
                Ok(LuaValue::Nil)
            },
        )?;
        globals.set("call", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (event_name, entity_id): (String, u64)| -> LuaResult<u32> {
                let listener_id = s.borrow().alloc_event_listener_id()?;
                s.borrow_mut().pending.push(BridgeCommand::RegisterEventListener {
                    event_name,
                    entity_id,
                    listener_id,
                });
                Ok(listener_id)
            },
        )?;
        globals.set("on_event", f)?;
    }
    {
        let s = state.clone();
        // The browser preamble's name for `on_event`, and the one
        // `core_script_api.rs` documents. Same registration; returning the
        // listener id is a superset of the browser, which returns nothing.
        let f = lua.create_function(
            move |_, (event_name, entity_id): (String, u64)| -> LuaResult<u32> {
                let listener_id = s.borrow().alloc_event_listener_id()?;
                s.borrow_mut().pending.push(BridgeCommand::RegisterEventListener {
                    event_name,
                    entity_id,
                    listener_id,
                });
                Ok(listener_id)
            },
        )?;
        globals.set("register_event_listener", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (event_name, entity_id): (String, u64)| {
                s.borrow_mut().pending.push(BridgeCommand::RemoveEventListener {
                    event_name,
                    entity_id,
                });
                Ok(())
            },
        )?;
        globals.set("remove_event_listener", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, delay, callback_name): (u64, f32, String)| -> LuaResult<u32> {
                let timer_id = s.borrow().alloc_timer_id()?;
                s.borrow_mut().pending.push(BridgeCommand::SetTimer {
                    entity_id,
                    delay,
                    callback_name,
                    repeating: false,
                    timer_id,
                });
                Ok(timer_id)
            },
        )?;
        globals.set("set_timer", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, interval, callback_name): (u64, f32, String)| -> LuaResult<u32> {
                let timer_id = s.borrow().alloc_timer_id()?;
                s.borrow_mut().pending.push(BridgeCommand::SetTimer {
                    entity_id,
                    delay: interval,
                    callback_name,
                    repeating: true,
                    timer_id,
                });
                Ok(timer_id)
            },
        )?;
        globals.set("set_interval", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, timer_id: u32| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::CancelTimer { timer_id });
            Ok(())
        })?;
        globals.set("cancel_timer", f)?;
    }

    // --- Phase 5.9: UI elements --------------------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_,
                  (text, anchor, offset_x, offset_y, r, g, b, a, font_size, alignment): (
                String,
                String,
                f32,
                f32,
                Option<f32>,
                Option<f32>,
                Option<f32>,
                Option<f32>,
                Option<f32>,
                Option<String>,
            )|
                  -> LuaResult<u64> {
                // Trailing color/font/align args are optional. Defaults match
                // `UIText::default` (white / 16.0 / "left") and the browser
                // wrapper coerces missing values the same way.
                let color = [
                    r.unwrap_or(1.0),
                    g.unwrap_or(1.0),
                    b.unwrap_or(1.0),
                    a.unwrap_or(1.0),
                ];
                let font_size = font_size.unwrap_or(16.0);
                let alignment = alignment.unwrap_or_else(|| "left".to_string());
                let entity_id = s.borrow().alloc_spawn_id()?;
                s.borrow_mut().pending.push(BridgeCommand::CreateUIText {
                    text,
                    anchor,
                    offset_x,
                    offset_y,
                    color,
                    font_size,
                    alignment,
                    entity_id,
                });
                Ok(entity_id)
            },
        )?;
        globals.set("create_ui_text", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_,
                  (anchor, offset_x, offset_y, width, height, r, g, b, a): (
                String, f32, f32, f32, f32, Option<f32>, Option<f32>, Option<f32>, Option<f32>,
            )|
                  -> LuaResult<u64> {
                // The colour is optional, as it is in the browser runtimes:
                // a translucent black panel. It was required here, so the
                // documented five-argument call threw natively (scene 40).
                let bg_color = [r.unwrap_or(0.0), g.unwrap_or(0.0), b.unwrap_or(0.0), a.unwrap_or(0.5)];
                let entity_id = s.borrow().alloc_spawn_id()?;
                s.borrow_mut().pending.push(BridgeCommand::CreateUIPanel {
                    anchor,
                    offset_x,
                    offset_y,
                    width,
                    height,
                    bg_color,
                    entity_id,
                });
                Ok(entity_id)
            },
        )?;
        globals.set("create_ui_panel", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, text): (u64, String)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetUIText { entity_id, text });
            Ok(())
        })?;
        globals.set("set_ui_text", f)?;
    }
    {
        // `set_ui_prop(entity, property, value)` — the entity-UI twin of
        // `gui_set`. Value arrives as a Lua value and is converted to JSON so
        // strings, numbers, booleans and {r,g,b,a} tables all ride the same
        // command.
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, property, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetUiProp { entity_id, property, value });
                Ok(())
            },
        )?;
        globals.set("set_ui_prop", f)?;
    }
    {
        // `set_emitter_field(entity, field, value)` - `Particles.set`: any
        // emitter field by name, the value converted as `set_ui_prop`'s is.
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetEmitterField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_emitter_field", f)?;
    }
    {
        // `set_cloth_field(entity, field, value)` - `Cloth.set`: any cloth or
        // rope field by name, the value converted as `set_emitter_field`'s is.
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetClothField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_cloth_field", f)?;
    }
    {
        // `set_path_follow_field(entity, field, value)` - `PathFollow.set`:
        // any `path_follow` field by name (`path` switches tracks).
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetPathFollowField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_path_follow_field", f)?;
    }
    {
        // `set_trail_field(entity, field, value)` - `Trail.set`: any `trail`
        // field by name (a pooled ribbon's colour, its width).
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetTrailField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_trail_field", f)?;
    }
    {
        // `set_line_field(entity, field, value)` - `Line.set`: any `line`
        // field by name (`points` with a table of `{x, y}` pairs).
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetLineField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_line_field", f)?;
    }
    {
        // `set_fluid_field(entity, field, value)` - `Fluid.set`: any `fluid`
        // field by name (`pour_rate` to pour; `clear` empties it).
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetFluidField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_fluid_field", f)?;
    }
    {
        // `set_mirror_field(entity, field, value)` - `Mirror.set`: any
        // `mirror` field by name (`enabled` to turn it off).
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetMirrorField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_mirror_field", f)?;
    }
    {
        // `smooth_terrain_edit(entity, x, y, radius, amount)` - `Terrain.edit`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y, radius, amount): (u64, f32, f32, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::TerrainEdit { entity_id, x, y, radius, amount });
            Ok(())
        })?;
        globals.set("smooth_terrain_edit", f)?;
    }
    // `Dialogue.start(e, node)` / `advance(e)` / `choose(e, n)` / `stop(e)`:
    // one command, `DialogueOp`, with its op.
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, node): (u64, Option<String>)| {
            let arg = node.unwrap_or_default();
            s.borrow_mut().pending.push(BridgeCommand::DialogueOp { entity_id, op: "start".into(), arg });
            Ok(())
        })?;
        globals.set("dialogue_start", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| {
            s.borrow_mut().pending.push(BridgeCommand::DialogueOp { entity_id, op: "advance".into(), arg: String::new() });
            Ok(())
        })?;
        globals.set("dialogue_advance", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, n): (u64, f64)| {
            s.borrow_mut().pending.push(BridgeCommand::DialogueOp { entity_id, op: "choose".into(), arg: format!("{n}") });
            Ok(())
        })?;
        globals.set("dialogue_choose", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| {
            s.borrow_mut().pending.push(BridgeCommand::DialogueOp { entity_id, op: "stop".into(), arg: String::new() });
            Ok(())
        })?;
        globals.set("dialogue_stop", f)?;
    }
    {
        // `ui_drop(item, slot)` - `UI.drop`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, slot): (u64, String)| {
            s.borrow_mut().pending.push(BridgeCommand::UiDrop { entity_id, slot });
            Ok(())
        })?;
        globals.set("ui_drop", f)?;
    }
    {
        // `mode7_place(entity, x, y, angle)` - `Mode7.place`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y, angle): (u64, f32, f32, Option<f32>)| {
            s.borrow_mut().pending.push(BridgeCommand::Mode7Place { entity_id, x, y, angle: angle.unwrap_or(0.0) });
            Ok(())
        })?;
        globals.set("mode7_place", f)?;
    }
    {
        // `mode7_set(entity, field, value)` - `Mode7.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::Mode7Set { entity_id, field, value });
            Ok(())
        })?;
        globals.set("mode7_set", f)?;
    }
    {
        // `stage_place(entity, x, y, z)` - `Stage.place`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y, z): (u64, f32, f32, Option<f32>)| {
            s.borrow_mut().pending.push(BridgeCommand::StagePlace { entity_id, x, y, z: z.unwrap_or(0.0) });
            Ok(())
        })?;
        globals.set("stage_place", f)?;
    }
    {
        // `stage_set(entity, field, value)` - `Stage.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::StageSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("stage_set", f)?;
    }
    {
        // `sprite_tilt_set(entity, field, value)` - `SpriteTilt.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SpriteTiltSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("sprite_tilt_set", f)?;
    }
    {
        // `afterimage_set(entity, field, value)` - `Afterimage.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::AfterimageSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("afterimage_set", f)?;
    }
    {
        // `physics_slice(x0, y0, x1, y1)` - `Physics.slice`.
        let s = state.clone();
        let f = lua.create_function(move |_, (x0, y0, x1, y1): (f32, f32, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::PhysicsSlice { x0, y0, x1, y1 });
            Ok(())
        })?;
        globals.set("physics_slice", f)?;
    }
    {
        // `portal_set(entity, field, value)` - `Portal.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::PortalSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("portal_set", f)?;
    }
    {
        // `jelly_poke(entity, x, y, strength, radius)` - `Jelly.poke`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y, strength, radius): (u64, f32, f32, Option<f32>, Option<f32>)| {
            let (strength, radius) = (strength.unwrap_or(300.0), radius.unwrap_or(40.0));
            s.borrow_mut().pending.push(BridgeCommand::JellyPoke { entity_id, x, y, strength, radius });
            Ok(())
        })?;
        globals.set("jelly_poke", f)?;
        // `jelly_set(entity, field, value)` - `Jelly.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::JellySet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("jelly_set", f)?;
    }
    {
        // `marker_set(entity, field, value)` - `Marker.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::MarkerSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("marker_set", f)?;
    }
    {
        // `radial_set(entity, field, value)` - `Radial.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::RadialSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("radial_set", f)?;
    }
    {
        // `bubble_say(entity, text, seconds)` - `Bubble.say`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, text, seconds): (u64, String, Option<f32>)| {
            s.borrow_mut().pending.push(BridgeCommand::BubbleSay { entity_id, text, seconds: seconds.unwrap_or(-1.0) });
            Ok(())
        })?;
        globals.set("bubble_say", f)?;
        // `bubble_set(entity, field, value)` - `Bubble.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::BubbleSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("bubble_set", f)?;
    }
    {
        // `outline_set(entity, field, value)` - `Outline.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::OutlineSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("outline_set", f)?;
    }
    {
        // `lightning_strike(entity, x, y)` - `Lightning.strike`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y): (u64, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::LightningStrike { entity_id, x, y });
            Ok(())
        })?;
        globals.set("lightning_strike", f)?;
        // `lightning_set(entity, field, value)` - `Lightning.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::LightningSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("lightning_set", f)?;
    }
    {
        // `flow_set(entity, field, value)` - `Flow.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::FlowSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("flow_set", f)?;
    }
    {
        // `nav_link_set(entity, field, value)` - `NavLink.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::NavLinkSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("nav_link_set", f)?;
    }
    {
        // `morph_to(entity, target, seconds)` - `Morph.to`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, target, seconds): (u64, String, Option<f32>)| {
            s.borrow_mut().pending.push(BridgeCommand::MorphTo { entity_id, target, seconds: seconds.unwrap_or(-1.0) });
            Ok(())
        })?;
        globals.set("morph_to", f)?;
        // `morph_set(entity, field, value)` - `Morph.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::MorphSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("morph_set", f)?;
    }
    {
        // `toast_show(entity, title, text, icon, seconds)` - `Toast.show`.
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, title, text, icon, seconds): (u64, String, Option<String>, Option<String>, Option<f32>)| {
                s.borrow_mut().pending.push(BridgeCommand::ToastShow {
                    entity_id,
                    title,
                    text: text.unwrap_or_default(),
                    icon: icon.unwrap_or_default(),
                    seconds: seconds.unwrap_or(0.0),
                });
                Ok(())
            },
        )?;
        globals.set("toast_show", f)?;
        // `toast_set(entity, field, value)` - `Toast.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::ToastSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("toast_set", f)?;
    }
    {
        // `snow_set(entity, field, value)` - `Snow.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SnowSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("snow_set", f)?;
    }
    {
        // `fog_of_war_reset(entity)` - `FogOfWar.reset`: the map unseen again.
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| {
            s.borrow_mut().pending.push(BridgeCommand::FogOfWarReset { entity_id });
            Ok(())
        })?;
        globals.set("fog_of_war_reset", f)?;
    }
    {
        // `fog_of_war_set(entity, field, value)` - `FogOfWar.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::FogOfWarSet { entity_id, field, value });
            Ok(())
        })?;
        globals.set("fog_of_war_set", f)?;
    }
    {
        // `set_beat_clock_field(entity, field, value)` - `BeatClock.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetBeatClockField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_beat_clock_field", f)?;
    }
    {
        // `set_text_path_field(entity, field, value)` - `TextPath.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetTextPathField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_text_path_field", f)?;
    }
    {
        // `set_trajectory_field(entity, field, value)` - `Trajectory.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetTrajectoryField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_trajectory_field", f)?;
    }
    {
        // `set_tile_light_field(entity, field, value)` - `TileLight.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetTileLightField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_tile_light_field", f)?;
    }
    {
        // `set_auto_normal_field(entity, field, value)` - `AutoNormal.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetAutoNormalField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_auto_normal_field", f)?;
    }
    {
        // `set_cast_shadow_field(entity, field, value)` - `CastShadow.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetCastShadowField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_cast_shadow_field", f)?;
    }
    {
        // `set_time_scale_field(entity, field, value)` - `TimeScale.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetTimeScaleField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_time_scale_field", f)?;
    }
    {
        // `set_origin_field(entity, field, value)` - `FloatingOrigin.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetOriginField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_origin_field", f)?;
    }
    {
        // `set_day_field(entity, field, value)` - `DayCycle.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetDayField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_day_field", f)?;
    }
    {
        // `set_vision_field(entity, field, value)` - `Vision.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetVisionField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_vision_field", f)?;
    }
    {
        // `wfc_generate(entity, x, y, w, h)` - `Wfc.generate`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y, w, h): (u64, f64, f64, f64, f64)| {
            s.borrow_mut().pending.push(BridgeCommand::WfcGenerate {
                entity_id,
                x: x.floor() as i32,
                y: y.floor() as i32,
                w: w.floor() as i32,
                h: h.floor() as i32,
            });
            Ok(())
        })?;
        globals.set("wfc_generate", f)?;
    }
    {
        // `set_fog_field(entity, field, value)` - `Fog.set`: any field.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetFogField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_fog_field", f)?;
    }
    {
        // `set_rewind_field(entity, field, value)` - `Rewind.set`: the
        // clock's mode or speed.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetRewindField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_rewind_field", f)?;
    }
    {
        // `smoke_puff(entity, x, y, radius, amount, vx, vy)` - `Smoke.puff`.
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, x, y, radius, amount, vx, vy): (u64, f32, f32, f32, f32, Option<f32>, Option<f32>)| {
                s.borrow_mut().pending.push(BridgeCommand::SmokePuff {
                    entity_id,
                    x,
                    y,
                    radius,
                    amount,
                    vx: vx.unwrap_or(0.0),
                    vy: vy.unwrap_or(0.0),
                });
                Ok(())
            },
        )?;
        globals.set("smoke_puff", f)?;
    }
    {
        // `set_smoke_field(entity, field, value)` - `Smoke.set`: any field by
        // name, or a source's as "sources.<n>.<field>".
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetSmokeField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_smoke_field", f)?;
    }
    {
        // `pixel_paint(entity, x, y, radius, material)` - `PixelSim.paint`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y, radius, material): (u64, f32, f32, f32, String)| {
            s.borrow_mut().pending.push(BridgeCommand::PixelPaint { entity_id, x, y, radius, material });
            Ok(())
        })?;
        globals.set("pixel_paint", f)?;
    }
    {
        // `set_sprite_fill(entity, amount)` - `Sprite.set_fill`: how much of
        // a bar or radial fill is drawn.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, amount): (u64, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetSpriteFill { entity_id, amount });
            Ok(())
        })?;
        globals.set("set_sprite_fill", f)?;
    }
    {
        // `set_water_field(entity, field, value)` - `Water.set`: any
        // `water_surface` field by name (`chop` for a storm).
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetWaterField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_water_field", f)?;
    }
    {
        // `water_splash(entity, x, strength)` - `Water.splash`: push the
        // surface at world x.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, strength): (u64, f32, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::WaterSplash { entity_id, x, strength });
            Ok(())
        })?;
        globals.set("water_splash", f)?;
    }
    {
        // `set_canvas_group_field(entity, field, value)` - `CanvasGroup.set`:
        // any `canvas_group` field by name (`opacity` fades it).
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetCanvasGroupField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_canvas_group_field", f)?;
    }
    {
        // `set_video_field(entity, field, value)` - `Video.set`:
        // any `video_player` field by name (`playing`, `seek`, `source` ...).
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetVideoField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_video_field", f)?;
    }
    {
        // `set_ragdoll_field(entity, field, value)` - `Ragdoll.set`:
        // any `ragdoll` field by name (`active` to go limp, `limit` ...).
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetRagdollField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_ragdoll_field", f)?;
    }
    {
        // `set_timeline_field(entity, field, value)` - `Timeline.set`:
        // any `timeline` field by name (`clip` to play one, `time` to seek ...).
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetTimelineField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_timeline_field", f)?;
    }
    {
        // `set_behavior_tree_field(entity, field, value)` - `BehaviorTree.set`.
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetBehaviorTreeField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_behavior_tree_field", f)?;
    }
    {
        // `set_area_field(entity, field, value)` - `Area.set`: any field of an
        // area effect by name (`ecs::area_effect`).
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, field, value): (u64, String, LuaValue)| {
                let value = lua_value_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetAreaField { entity_id, field, value });
                Ok(())
            },
        )?;
        globals.set("set_area_field", f)?;
    }
    {
        // `Nav.set_target(entity, x, y)` / `Nav.stop(entity)` / `Nav.set`.
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, x, y): (u64, Option<f32>, Option<f32>)| {
            let target = x.zip(y).map(|(x, y)| [x, y]);
            s.borrow_mut().pending.push(BridgeCommand::SetNavTarget { entity_id, target });
            Ok(())
        })?;
        globals.set("set_nav_target", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| {
            s.borrow_mut().pending.push(BridgeCommand::SetNavTarget { entity_id, target: None });
            Ok(())
        })?;
        globals.set("stop_nav", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, field, value): (u64, String, LuaValue)| {
            let value = lua_value_to_json(&value);
            s.borrow_mut().pending.push(BridgeCommand::SetNavField { entity_id, field, value });
            Ok(())
        })?;
        globals.set("set_nav_field", f)?;
    }
    {
        // `Nav.find_path(x0, y0, x1, y1)` and `Nav.closest_point(x, y)`,
        // answered from the lent world (`navigation`).
        let s = state.clone();
        let f = lua.create_function(
            move |lua, (x0, y0, x1, y1): (f32, f32, f32, f32)| -> LuaResult<Option<mlua::Table>> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                let Some(path) = snap.nav_find_path((x0, y0), (x1, y1)) else {
                    return Ok(None);
                };
                let table = lua.create_table_with_capacity(path.len(), 0)?;
                for (i, (x, y)) in path.into_iter().enumerate() {
                    let pair = lua.create_table_with_capacity(2, 0)?;
                    pair.raw_set(1, x)?;
                    pair.raw_set(2, y)?;
                    table.raw_set(i + 1, pair)?;
                }
                Ok(Some(table))
            },
        )?;
        globals.set("nav_find_path", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, (x, y): (f32, f32)| -> LuaResult<(Option<f32>, Option<f32>)> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(match snap.nav_closest_point((x, y)) {
                Some((x, y)) => (Some(x), Some(y)),
                None => (None, None),
            })
        })?;
        globals.set("nav_closest_point", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, entity_id: u64| -> LuaResult<bool> {
            let st = s.borrow();
            Ok(st.snapshot()?.nav_is_moving(entity_id))
        })?;
        globals.set("nav_is_moving", f)?;
    }
    {
        // `Platform.*`: a web build's portal (`docs/web-platforms.md`). A
        // native build has none, so these answer as a web build with
        // `web_platform: "none"` does - and `show_ad` still answers, through
        // the event a portal would send, so a game's ad flow runs everywhere.
        // One `globals.set` per name, spelled out: `script_global_parity`
        // finds the native surface by reading these calls.
        let f = lua.create_function(|_, ()| Ok("none"))?;
        globals.set("platform_name", f)?;
        globals.set("platform_game_ready", lua.create_function(|_, ()| Ok(()))?)?;
        globals.set("platform_gameplay_start", lua.create_function(|_, ()| Ok(()))?)?;
        globals.set("platform_gameplay_stop", lua.create_function(|_, ()| Ok(()))?)?;
        globals.set("platform_happy_time", lua.create_function(|_, ()| Ok(()))?)?;
        let f = lua.create_function(|_, (_value, _board): (Option<f64>, Option<String>)| Ok(()))?;
        globals.set("platform_submit_score", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, (kind, _reward): (Option<String>, Option<String>)| {
            let kind = if kind.as_deref() == Some("rewarded") { "rewarded" } else { "midgame" };
            let data = serde_json::json!({ "kind": kind, "shown": false, "rewarded": false });
            s.borrow_mut().pending.push(BridgeCommand::SendEvent {
                name: "platform_ad_finished".to_string(),
                sender: 0,
                data: Some(data.to_string()),
            });
            Ok(())
        })?;
        globals.set("platform_show_ad", f)?;
    }
    {
        // `Input.rumble` / `Input.vibrate`: the process-wide haptics queue
        // (`crate::input::haptics`), played by the runtime's gamepad poll.
        let f = lua.create_function(|_, (pad, strong, weak, seconds): (Option<i64>, Option<f32>, Option<f32>, Option<f32>)| {
            crate::input::haptics::rumble(
                pad.unwrap_or(0).max(0) as usize,
                strong.unwrap_or(0.0),
                weak.unwrap_or(0.0),
                seconds.unwrap_or(0.0),
            );
            Ok(())
        })?;
        globals.set("input_rumble", f)?;
        let f = lua.create_function(|_, (seconds, strength): (Option<f32>, Option<f32>)| {
            crate::input::haptics::vibrate(seconds.unwrap_or(0.0), strength.unwrap_or(1.0));
            Ok(())
        })?;
        globals.set("input_vibrate", f)?;
        // `Replay.*`: the process-wide input replay (`crate::input::replay`).
        let f = lua.create_function(|_, ()| {
            crate::input::replay::record();
            Ok(())
        })?;
        globals.set("replay_record", f)?;
        let f = lua.create_function(|_, ()| Ok(crate::input::replay::play()))?;
        globals.set("replay_play", f)?;
        let f = lua.create_function(|_, ()| {
            crate::input::replay::stop();
            Ok(())
        })?;
        globals.set("replay_stop", f)?;
        let f = lua.create_function(|_, ()| {
            crate::input::replay::clear();
            Ok(())
        })?;
        globals.set("replay_clear", f)?;
        let f = lua.create_function(|_, ()| Ok(crate::input::replay::mode().name()))?;
        globals.set("replay_status", f)?;
        let f = lua.create_function(|_, ()| Ok(crate::input::replay::frames() as f64))?;
        globals.set("replay_frames", f)?;
        let f = lua.create_function(|_, ()| Ok(crate::input::replay::progress() as f64))?;
        globals.set("replay_progress", f)?;
        let f = lua.create_function(|_, key: String| Ok(crate::input::replay::live_key(&key)))?;
        globals.set("replay_live_key", f)?;
        // `Audio.set_variation`: the process-wide per-clip table
        // (`crate::audio::variation`), read where each backend plays a
        // one-shot - no world, so no snapshot.
        let f = lua.create_function(|_, (clip, pitch, volume): (String, Option<f32>, Option<f32>)| {
            crate::audio::variation::set(&clip, pitch.unwrap_or(0.0), volume.unwrap_or(0.0));
            Ok(())
        })?;
        globals.set("audio_set_variation", f)?;
        // `Audio.follow_time` / `Time.get_scale`: the process-wide time scale.
        let f = lua.create_function(|_, on: bool| {
            crate::time::set_audio_follows_time(on);
            Ok(())
        })?;
        globals.set("audio_follow_time", f)?;
        let f = lua.create_function(|_, ()| Ok(crate::time::world_scale()))?;
        globals.set("get_time_scale", f)?;
        // `State.*`: save states are requests the physics frame answers
        // (`crate::ecs::save_state`) - no snapshot, no command.
        let f = lua.create_function(|_, slot: String| {
            let _ = &slot;
            crate::feature!("save_state", "State.save", { crate::ecs::save_state::request_save(&slot) });
            Ok(())
        })?;
        globals.set("state_save", f)?;
        let f = lua.create_function(|_, slot: String| {
            let _ = &slot;
            crate::feature!("save_state", "State.load", { crate::ecs::save_state::request_load(&slot) });
            Ok(())
        })?;
        globals.set("state_load", f)?;
        let f = lua.create_function(|_, slot: String| {
            #[cfg(feature = "save_state")]
            return Ok(crate::ecs::save_state::has(&slot));
            #[cfg(not(feature = "save_state"))]
            {
                let _ = slot;
                crate::features::missing("save_state", "State.has");
                Ok(false)
            }
        })?;
        globals.set("state_has", f)?;
        let f = lua.create_function(|_, slot: String| {
            let _ = &slot;
            crate::feature!("save_state", "State.forget", { crate::ecs::save_state::request_forget(&slot) });
            Ok(())
        })?;
        globals.set("state_forget", f)?;
        // `Locale.*`: the process-wide language and string tables
        // (`crate::locale`) - no world, so no snapshot.
        let f = lua.create_function(|_, (key, args): (String, Option<LuaValue>)| {
            let args = args.map(|a| lua_value_to_json(&a));
            Ok(crate::locale::translate(&key, args.as_ref()))
        })?;
        globals.set("locale_get", f)?;
        let f = lua.create_function(|_, code: Option<String>| {
            crate::locale::set_locale(code.as_deref().unwrap_or(""));
            Ok(())
        })?;
        globals.set("locale_set", f)?;
        let f = lua.create_function(|_, ()| Ok(crate::locale::current()))?;
        globals.set("locale_current", f)?;
        let f = lua.create_function(|lua, ()| {
            let codes = crate::locale::available();
            let table = lua.create_table_with_capacity(codes.len(), 0)?;
            for (i, c) in codes.into_iter().enumerate() {
                table.raw_set(i + 1, c)?;
            }
            Ok(table)
        })?;
        globals.set("locale_list", f)?;
        let f = lua.create_function(|_, code: String| Ok(crate::locale::language_name(&code)))?;
        globals.set("locale_name", f)?;
        // `Scene.load_additive` / `unload` / `preload` / `status` /
        // `progress`: the process-wide streaming requests
        // (`crate::scene::streaming`), loaded by the runner - no world, so
        // no snapshot.
        let f = lua.create_function(|_, (path, x, y): (String, Option<f32>, Option<f32>)| {
            Ok(crate::scene::streaming::load_additive(&path, x.unwrap_or(0.0), y.unwrap_or(0.0)))
        })?;
        globals.set("scene_load_additive", f)?;
        let f = lua.create_function(|_, handle: f64| {
            crate::scene::streaming::unload(handle.max(0.0) as u32);
            Ok(())
        })?;
        globals.set("scene_unload", f)?;
        let f = lua.create_function(|_, path: String| {
            crate::scene::streaming::preload(&path);
            Ok(())
        })?;
        globals.set("scene_preload", f)?;
        let f = lua.create_function(|_, of: LuaValue| {
            Ok(match of {
                LuaValue::String(s) => crate::scene::streaming::preload_status(&s.to_string_lossy()).name(),
                LuaValue::Integer(n) => crate::scene::streaming::status(n.max(0) as u32).name(),
                LuaValue::Number(n) => crate::scene::streaming::status(n.max(0.0) as u32).name(),
                _ => "unloaded",
            })
        })?;
        globals.set("scene_status", f)?;
        let f = lua.create_function(|_, of: LuaValue| {
            Ok(match of {
                LuaValue::String(s) => crate::scene::streaming::preload_status(&s.to_string_lossy()).progress(),
                LuaValue::Integer(n) => crate::scene::streaming::status(n.max(0) as u32).progress(),
                LuaValue::Number(n) => crate::scene::streaming::status(n.max(0.0) as u32).progress(),
                _ => 0.0,
            })
        })?;
        globals.set("scene_progress", f)?;
        // `Packs.*`: the process-wide asset packs (`crate::asset::packs`),
        // fetched by the runner - no world, so no snapshot. Every global is
        // one op of `script_op`, which the browsers' hook shares.
        #[cfg(not(feature = "packs"))]
        let pack_fn = |_op: &'static str| {
            lua.create_function(|_, _arg: Option<String>| {
                crate::features::missing("packs", "Packs.*");
                Ok(LuaValue::Nil)
            })
        };
        #[cfg(feature = "packs")]
        let pack_fn = |op: &'static str| {
            lua.create_function(move |lua, arg: Option<String>| {
                use crate::asset::packs::{ScriptAnswer, script_op};
                Ok(match script_op(op, arg.as_deref().unwrap_or("")) {
                    ScriptAnswer::Nothing => LuaValue::Nil,
                    ScriptAnswer::Text(s) => LuaValue::String(lua.create_string(&s)?),
                    ScriptAnswer::Number(n) => LuaValue::Number(n),
                    ScriptAnswer::List(names) => {
                        let table = lua.create_table_with_capacity(names.len(), 0)?;
                        for (i, n) in names.into_iter().enumerate() {
                            table.raw_set(i + 1, n)?;
                        }
                        LuaValue::Table(table)
                    }
                })
            })
        };
        globals.set("pack_download", pack_fn("download")?)?;
        globals.set("pack_remove", pack_fn("remove")?)?;
        globals.set("pack_set_url", pack_fn("set_url")?)?;
        globals.set("pack_status", pack_fn("status")?)?;
        globals.set("pack_progress", pack_fn("progress")?)?;
        globals.set("pack_size", pack_fn("size")?)?;
        globals.set("pack_error", pack_fn("error")?)?;
        globals.set("pack_of", pack_fn("of")?)?;
        globals.set("pack_list", pack_fn("list")?)?;
        // `LiveOps.*`: the process-wide live ops (`crate::liveops`), fetched
        // by the runner - no world, so no snapshot. One `script_op`, shared
        // with the browsers' hook.
        #[cfg(feature = "liveops")]
        let liveops_answer = |lua: &Lua, a: crate::liveops::ScriptAnswer| -> LuaResult<LuaValue> {
            use crate::liveops::ScriptAnswer;
            Ok(match a {
                ScriptAnswer::Nothing => LuaValue::Nil,
                ScriptAnswer::Text(s) => LuaValue::String(lua.create_string(&s)?),
                ScriptAnswer::Number(n) => LuaValue::Number(n),
                ScriptAnswer::Bool(b) => LuaValue::Boolean(b),
                ScriptAnswer::List(names) => {
                    let table = lua.create_table_with_capacity(names.len(), 0)?;
                    for (i, n) in names.into_iter().enumerate() {
                        table.raw_set(i + 1, n)?;
                    }
                    LuaValue::Table(table)
                }
                ScriptAnswer::Json(v) => json_to_lua(lua, &v)?,
            })
        };
        // `LiveOps.get(key, default)`: the script's own default when nothing
        // - not the service, not the game's defaults - has a value.
        #[cfg(feature = "liveops")]
        let f = lua.create_function(move |lua, (key, default): (String, LuaValue)| match crate::liveops::script_op("get", &key) {
            crate::liveops::ScriptAnswer::Nothing => Ok(default),
            a => liveops_answer(lua, a),
        })?;
        #[cfg(not(feature = "liveops"))]
        let f = lua.create_function(|_, (_key, default): (String, LuaValue)| {
            crate::features::missing("liveops", "LiveOps.*");
            Ok(default)
        })?;
        globals.set("liveops_get", f)?;
        // Every argument as text, as the browsers' hook gets it: `nil` "",
        // a whole number without its `.0`.
        #[cfg(not(feature = "liveops"))]
        let liveops_fn = |_op: &'static str| {
            lua.create_function(|_, _args: LuaMultiValue| {
                crate::features::missing("liveops", "LiveOps.*");
                Ok(LuaValue::Nil)
            })
        };
        #[cfg(feature = "liveops")]
        let liveops_fn = |op: &'static str| {
            lua.create_function(move |lua, args: LuaMultiValue| {
                let text: Vec<String> = args
                    .iter()
                    .map(|v| match v {
                        LuaValue::Nil => String::new(),
                        LuaValue::Boolean(b) => b.to_string(),
                        LuaValue::Integer(i) => i.to_string(),
                        LuaValue::Number(n) => crate::liveops::number_text(*n),
                        LuaValue::String(s) => s.to_string_lossy().to_string(),
                        _ => String::new(),
                    })
                    .collect();
                let refs: Vec<&str> = text.iter().map(String::as_str).collect();
                liveops_answer(lua, crate::liveops::script_call(op, &refs))
            })
        };
        globals.set("liveops_keys", liveops_fn("keys")?)?;
        globals.set("liveops_status", liveops_fn("status")?)?;
        globals.set("liveops_error", liveops_fn("error")?)?;
        globals.set("liveops_fetch", liveops_fn("fetch")?)?;
        globals.set("liveops_fetching", liveops_fn("fetching")?)?;
        globals.set("liveops_variant", liveops_fn("variant")?)?;
        globals.set("liveops_events", liveops_fn("events")?)?;
        globals.set("liveops_event_active", liveops_fn("event_active")?)?;
        globals.set("liveops_event_ends", liveops_fn("event_ends")?)?;
        globals.set("liveops_event_starts", liveops_fn("event_starts")?)?;
        globals.set("liveops_event_name", liveops_fn("event_name")?)?;
        globals.set("liveops_set_player", liveops_fn("set_player")?)?;
        globals.set("liveops_server_time", liveops_fn("server_time")?)?;
        globals.set("liveops_revision", liveops_fn("revision")?)?;
        globals.set("liveops_release", liveops_fn("release")?)?;
        globals.set("liveops_updated", liveops_fn("updated")?)?;
        globals.set("liveops_expose", liveops_fn("expose")?)?;
        globals.set("liveops_set_attribute", liveops_fn("set_attribute")?)?;
        // `Display.*` / `Cursor.*`: the process-wide display settings
        // (`crate::display`), applied by the runner once a frame - no
        // world, so no snapshot.
        let f = lua.create_function(|_, mode: String| {
            crate::display::set_mode(&mode);
            Ok(())
        })?;
        globals.set("display_set_mode", f)?;
        let f = lua.create_function(|_, ()| Ok(crate::display::info().mode.name()))?;
        globals.set("display_get_mode", f)?;
        let f = lua.create_function(|_, (w, h): (Option<f64>, Option<f64>)| {
            crate::display::set_size(w.unwrap_or(0.0).max(0.0) as u32, h.unwrap_or(0.0).max(0.0) as u32);
            Ok(())
        })?;
        globals.set("display_set_size", f)?;
        let f = lua.create_function(|_, ()| -> LuaResult<(u32, u32)> {
            let i = crate::display::info();
            Ok((i.width, i.height))
        })?;
        globals.set("display_get_size", f)?;
        let f = lua.create_function(|lua, ()| {
            let modes = crate::display::info().modes;
            let table = lua.create_table_with_capacity(modes.len(), 0)?;
            for (i, (w, h, hz)) in modes.into_iter().enumerate() {
                let m = lua.create_table()?;
                m.set("w", w)?;
                m.set("h", h)?;
                m.set("hz", hz)?;
                table.raw_set(i + 1, m)?;
            }
            Ok(table)
        })?;
        globals.set("display_get_resolutions", f)?;
        let f = lua.create_function(|_, on: Option<bool>| {
            crate::display::set_vsync(on.unwrap_or(true));
            Ok(())
        })?;
        globals.set("display_set_vsync", f)?;
        let f = lua.create_function(|_, ()| Ok(crate::display::info().vsync))?;
        globals.set("display_get_vsync", f)?;
        let f = lua.create_function(|_, n: Option<f64>| {
            crate::display::set_max_fps(n.map(|n| n.round() as i64).unwrap_or(-1));
            Ok(())
        })?;
        globals.set("display_set_max_fps", f)?;
        let f = lua.create_function(|_, ()| Ok(crate::display::info().max_fps))?;
        globals.set("display_get_max_fps", f)?;
        let f = lua.create_function(|_, name: String| Ok(crate::display::info().supports.get(&name)))?;
        globals.set("display_supports", f)?;
        let f = lua.create_function(|_, on: Option<bool>| {
            crate::display::set_cursor_visible(on.unwrap_or(true));
            Ok(())
        })?;
        globals.set("cursor_set_visible", f)?;
        let f = lua.create_function(|_, on: Option<bool>| {
            crate::display::set_cursor_confined(on.unwrap_or(true));
            Ok(())
        })?;
        globals.set("cursor_set_confined", f)?;
        let f = lua.create_function(|_, on: Option<bool>| {
            crate::display::set_cursor_locked(on.unwrap_or(true));
            Ok(())
        })?;
        globals.set("cursor_set_locked", f)?;
        let f = lua.create_function(|_, name: Option<String>| {
            crate::display::set_cursor_icon(name.as_deref().unwrap_or("default"));
            Ok(())
        })?;
        globals.set("cursor_set_icon", f)?;
        let f = lua.create_function(|_, (id, hx, hy): (Option<String>, Option<f32>, Option<f32>)| {
            crate::display::set_cursor_image(id.as_deref().unwrap_or(""), hx.unwrap_or(0.0), hy.unwrap_or(0.0));
            Ok(())
        })?;
        globals.set("cursor_set_image", f)?;
    }
    #[cfg(feature = "noise")]
    {
        // `Noise.*`: pure functions of their arguments (`crate::noise`) - no
        // world, so no snapshot. A seed may be any number.
        let seed = |s: Option<f64>| s.map(crate::noise::seed_from).unwrap_or(0);
        let f = lua.create_function(move |_, (x, y, s): (f64, f64, Option<f64>)| Ok(crate::noise::perlin(x, y, seed(s))))?;
        globals.set("noise_perlin", f)?;
        let f = lua.create_function(move |_, (x, y, z, s): (f64, f64, f64, Option<f64>)| {
            Ok(crate::noise::perlin3(x, y, z, seed(s)))
        })?;
        globals.set("noise_perlin3", f)?;
        let f = lua.create_function(move |_, (x, y, s): (f64, f64, Option<f64>)| Ok(crate::noise::simplex(x, y, seed(s))))?;
        globals.set("noise_simplex", f)?;
        let f = lua.create_function(move |_, (x, y, s): (f64, f64, Option<f64>)| -> LuaResult<(f64, f64, f64)> {
            let c = crate::noise::cellular(x, y, seed(s));
            Ok((c.f1, c.f2, c.value))
        })?;
        globals.set("noise_cellular", f)?;
        let f = lua.create_function(|_, (x, y, opts): (f64, f64, Option<LuaValue>)| {
            let opts = opts.map(|o| lua_value_to_json(&o)).unwrap_or(serde_json::Value::Null);
            Ok(crate::noise::Fractal::from_json(&opts).sample(x, y))
        })?;
        globals.set("noise_fractal", f)?;
        let f = lua.create_function(
            |lua, (x0, y0, w, h, step, opts): (f64, f64, f64, f64, Option<f64>, Option<LuaValue>)| {
                let opts = opts.map(|o| lua_value_to_json(&o)).unwrap_or(serde_json::Value::Null);
                let values = crate::noise::script_grid(x0, y0, w, h, step.unwrap_or(1.0), &opts);
                let table = lua.create_table_with_capacity(values.len(), 0)?;
                for (i, v) in values.into_iter().enumerate() {
                    table.raw_set(i + 1, v)?;
                }
                Ok(table)
            },
        )?;
        globals.set("noise_grid", f)?;
    }
    #[cfg(not(feature = "noise"))]
    {
        // `Noise.*` in a build without noise: each logs once, answers zero.
        let zero = |lua: &Lua| {
            lua.create_function(|_, _args: LuaMultiValue| {
                crate::features::missing("noise", "Noise.*");
                Ok(0.0)
            })
        };
        for name in ["noise_perlin", "noise_perlin3", "noise_simplex", "noise_fractal"] {
            globals.set(name, zero(lua)?)?;
        }
        let f = lua.create_function(|_, _args: LuaMultiValue| {
            crate::features::missing("noise", "Noise.*");
            Ok((0.0, 0.0, 0.0))
        })?;
        globals.set("noise_cellular", f)?;
        let f = lua.create_function(|lua, _args: LuaMultiValue| {
            crate::features::missing("noise", "Noise.*");
            lua.create_table()
        })?;
        globals.set("noise_grid", f)?;
    }
    {
        // `Audio.get_spectrum` / `get_level` / `synth`: the process-wide
        // analysis ring and synth queue (`crate::audio::analysis`) - no world.
        let f = lua.create_function(|lua, n: Option<f64>| {
            let bands = crate::audio::analysis::spectrum(n.unwrap_or(16.0).max(1.0) as usize);
            let table = lua.create_table_with_capacity(bands.len(), 0)?;
            for (i, v) in bands.into_iter().enumerate() {
                table.raw_set(i + 1, v)?;
            }
            Ok(table)
        })?;
        globals.set("audio_get_spectrum", f)?;
        let f = lua.create_function(|_, ()| Ok(crate::audio::analysis::level()))?;
        globals.set("audio_get_level", f)?;
        let f = lua.create_function(|_, opts: Option<LuaValue>| {
            let opts = opts.map(|o| lua_value_to_json(&o)).unwrap_or(serde_json::Value::Null);
            crate::audio::analysis::request(crate::audio::analysis::Voice::from_json(&opts));
            Ok(())
        })?;
        globals.set("audio_synth", f)?;
    }
    {
        // `Screenshot.take` / `ready`: the process-wide request queue
        // (`crate::renderer::screenshot`) - no world.
        let f = lua.create_function(|_, opts: Option<LuaValue>| {
            let opts = opts.map(|o| lua_value_to_json(&o)).unwrap_or(serde_json::Value::Null);
            Ok(crate::renderer::screenshot::request(&opts))
        })?;
        globals.set("screenshot_take", f)?;
        let f = lua.create_function(|_, id: u64| Ok(crate::renderer::screenshot::is_done(id)))?;
        globals.set("screenshot_ready", f)?;
    }
    {
        // `Accessibility.announce`: the screen reader's next words
        // (`crate::ui::accessibility`) - no world.
        let f = lua.create_function(|_, text: String| {
            let _ = &text;
            crate::feature!("accessibility", "Accessibility.announce", { crate::ui::accessibility::announce(&text) });
            Ok(())
        })?;
        globals.set("accessibility_announce", f)?;
        let f = lua.create_function(|_, ()| {
            #[cfg(feature = "accessibility")]
            return Ok(crate::ui::accessibility::focused_speech());
            #[cfg(not(feature = "accessibility"))]
            {
                crate::features::missing("accessibility", "Accessibility.speech");
                Ok(String::new())
            }
        })?;
        globals.set("accessibility_speech", f)?;
    }
    {
        // `get_ui_prop(entity, property)` — reads back what `set_ui_prop`
        // writes, plus the button state the UI system maintains. Immediate,
        // not deferred: a getter that queued a command could not return a
        // value.
        let s = state.clone();
        let f = lua.create_function(move |lua, (entity_id, property): (u64, String)| {
            let st = s.borrow();
            match st.snapshot()?.ui_prop_of(entity_id, &property) {
                Some(v) => json_to_lua(lua, &v),
                None => Ok(LuaValue::Nil),
            }
        })?;
        globals.set("get_ui_prop", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, text): (u64, String)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetText { entity_id, text });
            Ok(())
        })?;
        globals.set("set_text", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, visible): (u64, bool)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetUIVisible { entity_id, visible });
            Ok(())
        })?;
        globals.set("set_ui_visible", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, anchor, offset_x, offset_y): (u64, String, f32, f32)| {
                s.borrow_mut().pending.push(BridgeCommand::SetUIPosition {
                    entity_id,
                    anchor,
                    offset_x,
                    offset_y,
                });
                Ok(())
            },
        )?;
        globals.set("set_ui_position", f)?;
    }

    // --- Phase 5.9: GUI document API ---------------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(move |_, gui_id: String| {
            s.borrow_mut().pending.push(BridgeCommand::GuiShow { gui_id });
            Ok(())
        })?;
        globals.set("gui_show", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, gui_id: String| {
            s.borrow_mut().pending.push(BridgeCommand::GuiHide { gui_id });
            Ok(())
        })?;
        globals.set("gui_hide", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (gui_id, node_id, property, value): (String, String, String, mlua::Value)| {
                // Stringify scalars at the FFI boundary so Lua can pass booleans
                // and numbers naturally. The translator parses the string back
                // into a typed `serde_json::Value` so widget setters that need
                // a bool/number (Slider/Toggle/visible) receive the right type.
                let value_s = match value {
                    mlua::Value::String(s) => s.to_str()?.to_string(),
                    mlua::Value::Integer(i) => i.to_string(),
                    mlua::Value::Number(n) => n.to_string(),
                    mlua::Value::Boolean(b) => b.to_string(),
                    mlua::Value::Nil => "null".to_string(),
                    // A colour, a uv rect, a shadow: a table is JSON - a list
                    // an array, keys an object - which is what the property
                    // parser reads. It was a hard error here, and in the
                    // browsers `String(table)` made "1,0,0,1" and was dropped.
                    mlua::Value::Table(t) => lua_table_to_json(&t).to_string(),
                    other => {
                        return Err(mlua::Error::RuntimeError(format!(
                            "gui_set: value must be a string, number, bool, table or nil, got {:?}",
                            other.type_name()
                        )))
                    }
                };
                s.borrow_mut().pending.push(BridgeCommand::GuiSetNodeProp {
                    gui_id,
                    node_id,
                    property,
                    value: value_s,
                });
                Ok(())
            },
        )?;
        globals.set("gui_set", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (gui_id, node_id, property): (String, String, String)| -> LuaResult<String> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                Ok(snap.gui_prop(&gui_id, &node_id, &property).unwrap_or_default())
            },
        )?;
        globals.set("gui_get", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, gui_id, node_id, callback_name): (u64, String, String, String)| {
                s.borrow_mut().pending.push(BridgeCommand::GuiOnClick {
                    entity_id,
                    gui_id,
                    node_id,
                    callback_name,
                });
                Ok(())
            },
        )?;
        globals.set("gui_on_click", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (gui_id, node_id): (String, String)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::GuiFocus { gui_id, node_id });
            Ok(())
        })?;
        globals.set("gui_focus", f)?;
    }

    // --- Phase 5.10: particles ----------------------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, count): (u64, u32)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::EmitBurst { entity_id, count });
            Ok(())
        })?;
        globals.set("emit_burst", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, active): (u64, bool)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetEmitterActive { entity_id, active });
            Ok(())
        })?;
        globals.set("set_emitter_active", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, rate): (u64, f32)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetEmitterRate { entity_id, rate });
            Ok(())
        })?;
        globals.set("set_emitter_rate", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<u32> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            Ok(snap.particle_count)
        })?;
        globals.set("get_particle_count", f)?;
    }

    // --- Phase 5.11: tilemap ------------------------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, layer_name, x, y): (u64, String, i32, i32)| -> LuaResult<i32> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                Ok(snap.tilemap_get_tile(entity_id, &layer_name, x, y))
            },
        )?;
        globals.set("get_tile", f)?;
        // `Tilemap.get_tile_data(entity, layer, x, y, field)`.
        let s = state.clone();
        let f = lua.create_function(
            move |lua, (entity_id, layer_name, x, y, field): (u64, String, i32, i32, Option<String>)| -> LuaResult<LuaValue> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                match snap.tilemap_tile_data(entity_id, &layer_name, x, y, field.as_deref()) {
                    Some(v) => json_to_lua(lua, &v),
                    None => Ok(LuaValue::Nil),
                }
            },
        )?;
        globals.set("get_tile_data", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            // `Option<f64>` rather than `i32`: `nil` is what the docs
            // promise clears a cell, and taking an `i32` made that a hard
            // error on native and native alone.
            move |_, (entity_id, layer_name, x, y, tile_id): (u64, String, u32, u32, Option<f64>)| {
                let tile_id = crate::tilemap::script_tile_id(tile_id);
                s.borrow_mut().pending.push(BridgeCommand::SetTile {
                    entity_id,
                    layer_name,
                    x,
                    y,
                    tile_id,
                });
                Ok(())
            },
        )?;
        globals.set("set_tile", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, layer_name, wx, wy): (u64, String, f32, f32)| -> LuaResult<i32> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                Ok(snap.tilemap_get_tile_at_world(entity_id, &layer_name, wx, wy))
            },
        )?;
        globals.set("get_tile_at_world", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, wx, wy): (u64, f32, f32)| -> LuaResult<(i32, i32)> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                Ok(snap.tilemap_world_to_tile(entity_id, wx, wy).unwrap_or((0, 0)))
            },
        )?;
        globals.set("world_to_tile", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, tx, ty): (u64, i32, i32)| -> LuaResult<(f32, f32)> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                Ok(snap.tilemap_tile_to_world(entity_id, tx, ty).unwrap_or((0.0, 0.0)))
            },
        )?;
        globals.set("tile_to_world", f)?;
    }
    // The cells next to a cell (six on a hexagonal map) and the steps between
    // two - what a hex game's movement range is counted with.
    {
        let s = state.clone();
        let f = lua.create_function(move |lua, (entity_id, tx, ty): (u64, i32, i32)| -> LuaResult<mlua::Table> {
            let st = s.borrow();
            let snap = st.snapshot()?;
            let cells = snap.tilemap_neighbors(entity_id, tx, ty);
            let table = lua.create_table_with_capacity(cells.len(), 0)?;
            for (i, (cx, cy)) in cells.into_iter().enumerate() {
                let pair = lua.create_table_with_capacity(2, 0)?;
                pair.raw_set(1, cx)?;
                pair.raw_set(2, cy)?;
                table.raw_set(i + 1, pair)?;
            }
            Ok(table)
        })?;
        globals.set("tilemap_neighbors", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (entity_id, ax, ay, bx, by): (u64, i32, i32, i32, i32)| -> LuaResult<i32> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                Ok(snap.tilemap_distance(entity_id, (ax, ay), (bx, by)))
            },
        )?;
        globals.set("tilemap_distance", f)?;
    }
    // 2.6 — A* path-finding. `mode` defaults to `"manhattan"`; pass
    // `"diagonal"` for 8-direction movement with octile heuristic.
    // Returns nil when no path exists or the tilemap is unknown.
    {
        let s = state.clone();
        let f = lua.create_function(
            move |lua,
                  (entity_id, layer_name, sx, sy, gx, gy, mode): (
                u64,
                String,
                i32,
                i32,
                i32,
                i32,
                Option<String>,
            )|
                  -> LuaResult<Option<mlua::Table>> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                let mode = mode.unwrap_or_else(|| "manhattan".to_string());
                let Some(path) =
                    snap.tilemap_find_path(entity_id, &layer_name, (sx, sy), (gx, gy), &mode)
                else {
                    return Ok(None);
                };
                let table = lua.create_table_with_capacity(path.len(), 0)?;
                for (i, (tx, ty)) in path.into_iter().enumerate() {
                    let pair = lua.create_table_with_capacity(2, 0)?;
                    pair.raw_set(1, tx)?;
                    pair.raw_set(2, ty)?;
                    table.raw_set(i + 1, pair)?;
                }
                Ok(Some(table))
            },
        )?;
        globals.set("tilemap_find_path", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |lua,
                  (entity_id, layer_name, sx, sy, gx, gy, mode): (
                u64,
                String,
                f32,
                f32,
                f32,
                f32,
                Option<String>,
            )|
                  -> LuaResult<Option<mlua::Table>> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                let mode = mode.unwrap_or_else(|| "manhattan".to_string());
                let Some(path) = snap.tilemap_find_path_world(
                    entity_id,
                    &layer_name,
                    (sx, sy),
                    (gx, gy),
                    &mode,
                ) else {
                    return Ok(None);
                };
                let table = lua.create_table_with_capacity(path.len(), 0)?;
                for (i, (wx, wy)) in path.into_iter().enumerate() {
                    let pair = lua.create_table_with_capacity(2, 0)?;
                    pair.raw_set(1, wx)?;
                    pair.raw_set(2, wy)?;
                    table.raw_set(i + 1, pair)?;
                }
                Ok(Some(table))
            },
        )?;
        globals.set("tilemap_find_path_world", f)?;
    }

    // --- Phase 5.12: lighting -----------------------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, r, g, b): (u64, f32, f32, f32)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetLightColor { entity_id, r, g, b });
            Ok(())
        })?;
        globals.set("set_light_color", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, intensity): (u64, f32)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetLightIntensity { entity_id, intensity });
            Ok(())
        })?;
        globals.set("set_light_intensity", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, height): (u64, f32)| {
            s.borrow_mut().pending.push(BridgeCommand::SetLightHeight { entity_id, height });
            Ok(())
        })?;
        globals.set("set_light_height", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, mask): (u64, f64)| {
            let mask = mask.clamp(0.0, u32::MAX as f64) as u32;
            s.borrow_mut().pending.push(BridgeCommand::SetLightMask { entity_id, mask });
            Ok(())
        })?;
        globals.set("set_light_mask", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (texture_id, strength): (Option<String>, Option<f32>)| {
            s.borrow_mut().pending.push(BridgeCommand::SetColorLut {
                texture_id: texture_id.unwrap_or_default(),
                strength: strength.unwrap_or(1.0),
            });
            Ok(())
        })?;
        globals.set("set_color_lut", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (entity_id, radius): (u64, f32)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetLightRadius { entity_id, radius });
            Ok(())
        })?;
        globals.set("set_light_radius", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (r, g, b, intensity): (f32, f32, f32, f32)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetAmbientLight { r, g, b, intensity });
            Ok(())
        })?;
        globals.set("set_ambient_light", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (bands, softness, dither): (f32, Option<f32>, Option<f32>)| {
            s.borrow_mut().pending.push(BridgeCommand::SetToonLighting { bands, softness, dither });
            Ok(())
        })?;
        globals.set("set_toon_lighting", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (radius, strength): (Option<f32>, Option<f32>)| {
            s.borrow_mut().pending.push(BridgeCommand::SetAmbientOcclusion { radius, strength });
            Ok(())
        })?;
        globals.set("set_ambient_occlusion", f)?;
        // `Lighting.set_global_illumination(strength, scale)`.
        let s = state.clone();
        let f = lua.create_function(move |_, (strength, scale): (Option<f32>, Option<f32>)| {
            s.borrow_mut().pending.push(BridgeCommand::SetGlobalIllumination { strength, scale });
            Ok(())
        })?;
        globals.set("set_global_illumination", f)?;
    }

    // --- Phase 5.12: post-process ------------------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (effect_name, enabled): (String, bool)| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetPostProcessEnabled { effect_name, enabled });
            Ok(())
        })?;
        globals.set("set_post_process_enabled", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (effect_name, param_name, value): (String, String, f32)| {
                s.borrow_mut().pending.push(BridgeCommand::SetPostProcessFloat {
                    effect_name,
                    param_name,
                    value,
                });
                Ok(())
            },
        )?;
        globals.set("set_post_process_float", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_,
                  (effect_name, param_name, x, y, z, w): (String, String, f32, f32, f32, f32)| {
                s.borrow_mut().pending.push(BridgeCommand::SetPostProcessVec4 {
                    effect_name,
                    param_name,
                    value: [x, y, z, w],
                });
                Ok(())
            },
        )?;
        globals.set("set_post_process_vec4", f)?;
    }
    // The browser preamble spells these with `_param_`, and
    // `core_script_api.rs` documents that spelling — so a script written
    // in the editor called a name that did not exist on native. Same
    // commands, second name.
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (effect, param, value): (String, String, f32)| {
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SetPostProcessFloat {
                        effect_name: effect,
                        param_name: param,
                        value,
                    });
                Ok(())
            },
        )?;
        globals.set("set_post_process_param_float", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (effect, param, x, y, z, w): (String, String, f32, f32, f32, f32)| {
                s.borrow_mut().pending.push(BridgeCommand::SetPostProcessVec4 {
                    effect_name: effect,
                    param_name: param,
                    value: [x, y, z, w],
                });
                Ok(())
            },
        )?;
        globals.set("set_post_process_param_vec4", f)?;
    }

    // --- Phase 5.12: debug drawing & overlay -------------------------------
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (x1, y1, x2, y2, r, g, b): (f32, f32, f32, f32, f32, f32, f32)| {
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::DrawDebugLine { x1, y1, x2, y2, r, g, b });
                Ok(())
            },
        )?;
        globals.set("draw_debug_line", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (x, y, w, h, r, g, b): (f32, f32, f32, f32, f32, f32, f32)| {
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::DrawDebugRect { x, y, w, h, r, g, b });
                Ok(())
            },
        )?;
        globals.set("draw_debug_rect", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (x, y, radius, r, g, b): (f32, f32, f32, f32, f32, f32)| {
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::DrawDebugCircle { x, y, radius, r, g, b });
                Ok(())
            },
        )?;
        globals.set("draw_debug_circle", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, visible: bool| {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SetDebugOverlay { visible });
            Ok(())
        })?;
        globals.set("set_debug_overlay", f)?;
    }

    // P1.3 — camera shake. `frequency` is optional in user code; Luau
    // passes `nil` through as 0.0 here and `Camera2D::start_shake`
    // substitutes a sane default (20 Hz). Restarting overrides any
    // in-flight shake.
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (amplitude, duration, frequency): (f32, f32, Option<f32>)| {
                s.borrow_mut().pending.push(BridgeCommand::CameraShake {
                    amplitude,
                    duration,
                    frequency: frequency.unwrap_or(0.0),
                });
                Ok(())
            },
        )?;
        globals.set("camera_shake", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, zoom: f32| {
            s.borrow_mut().pending.push(BridgeCommand::SetCameraZoom { zoom });
            Ok(())
        })?;
        globals.set("set_camera_zoom", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, size: f32| {
            s.borrow_mut().pending.push(BridgeCommand::SetCameraPixelSize { size });
            Ok(())
        })?;
        globals.set("set_camera_pixel_size", f)?;
    }

    // --- Phase 5.13: custom component access ---------------------------------
    {
        let s = state.clone();
        // Hot path: take `mlua::String` for the name args instead of
        // `String`. mlua's `String` borrows the Luau side's interned bytes,
        // so the per-call cost is two pointer copies rather than two heap
        // allocations. At 10k scripted entities each calling `get_custom`
        // twice per frame, this saves ~40k String allocations / frame.
        let f = lua.create_function(
            move |lua_ctx,
                  (entity_id, component_name, field_name): (
                u64,
                mlua::String,
                mlua::String,
            )|
                  -> LuaResult<LuaValue> {
                let comp_str = component_name.to_str()?;
                let field_str = field_name.to_str()?;
                let st = s.borrow();
                let snap = st.snapshot()?;
                // Fast f64 path first — typical hot-loop scripts (movers,
                // accumulators, counters) read numeric fields. Skips the
                // `Value::clone` in `get_custom_field` for the common case.
                // Numbers only: a boolean must come back as a boolean.
                if let Some(n) = snap.get_custom_field_number(entity_id, &comp_str, &field_str) {
                    return Ok(LuaValue::Number(n));
                }
                match snap.get_custom_field(entity_id, &comp_str, &field_str) {
                    Some(serde_json::Value::Number(n)) => {
                        Ok(LuaValue::Number(n.as_f64().unwrap_or(0.0)))
                    }
                    Some(serde_json::Value::Bool(b)) => Ok(LuaValue::Boolean(b)),
                    Some(serde_json::Value::String(s)) => {
                        Ok(LuaValue::String(lua_ctx.create_string(&s)?))
                    }
                    Some(serde_json::Value::Null) | None => Ok(LuaValue::Nil),
                    // Arrays and objects come back as tables, as they do in
                    // the editor and on the web. They used to be nil here
                    // only, so a package publishing a list for scripts to
                    // read (a ball game's `BallState.events`,
                    // jengine-net's `inbox`) worked in the editor and was
                    // silently empty in a native build.
                    Some(v) => json_to_lua(lua_ctx, &v),
                }
            },
        )?;
        globals.set("get_custom", f)?;
    }
    // --- Data-oriented design: static data-table access ----------------------
    {
        let s = state.clone();
        // `get_data(table, id, field)` reads a field from a loaded `.data`
        // table row. Full-fidelity: numbers/strings/bools return as Luau
        // scalars; arrays and nested objects (including reference values
        // `{ table = ..., id = ... }`) return as Luau tables so scripts can
        // follow references with a second `get_data` call.
        let f = lua.create_function(
            move |lua_ctx, (table_id, row_id, field): (mlua::String, mlua::String, mlua::String)|
                  -> LuaResult<LuaValue> {
                let table_str = table_id.to_str()?;
                let row_str = row_id.to_str()?;
                let field_str = field.to_str()?;
                let st = s.borrow();
                match st.data_registry.get(&table_str, &row_str, &field_str) {
                    Some(value) => json_to_lua(lua_ctx, value),
                    None => Ok(LuaValue::Nil),
                }
            },
        )?;
        globals.set("get_data", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_,
                  (entity_id, component_name, field_name, value): (
                u64,
                mlua::String,
                mlua::String,
                LuaValue,
            )|
                  -> LuaResult<()> {
                let json_val = match value {
                    LuaValue::Number(n) => serde_json::Value::from(n),
                    // Coerce integers to f64 so we stay parity-compatible with
                    // the host JS adapter (which is f64-only). Luau itself
                    // treats all numbers as doubles, so this is lossless.
                    LuaValue::Integer(n) => serde_json::Value::from(n as f64),
                    LuaValue::Boolean(b) => serde_json::Value::from(b),
                    LuaValue::String(s) => {
                        serde_json::Value::String(s.to_string_lossy().to_string())
                    }
                    LuaValue::Nil => serde_json::Value::Null,
                    // A table is stored as an array or object, the same way
                    // `save_set` stores one - it used to become null here.
                    LuaValue::Table(_) => lua_to_json(&value),
                    _ => serde_json::Value::Null,
                };
                // Intern via mlua's GC-borrowed `&str` so we never allocate
                // a per-call Rust `String` for the field/component label.
                let comp_arc = crate::scripting::intern::intern(
                    &component_name.to_string_lossy(),
                );
                let field_arc = crate::scripting::intern::intern(
                    &field_name.to_string_lossy(),
                );
                s.borrow_mut().pending.push(BridgeCommand::SetCustomField {
                    entity_id,
                    component_name: comp_arc,
                    field_name: field_arc,
                    value: json_val,
                });
                Ok(())
            },
        )?;
        globals.set("set_custom", f)?;
    }
    {
        // Phase 6.x — batched custom-component writer.
        // Hot path: scripts mutating multiple fields per frame on the same
        // component (e.g. mover scripts writing `velocity_x`/`velocity_y`)
        // collapse into ONE bridge crossing + ONE deferred command.
        let s = state.clone();
        let f = lua.create_function(
            move |_,
                  (entity_id, component_name, fields): (
                u64,
                mlua::String,
                LuaTable,
            )|
                  -> LuaResult<()> {
                // Pre-allocate; mover-style scripts pass 2-4 fields.
                let mut out: Vec<(std::sync::Arc<str>, serde_json::Value)> =
                    Vec::with_capacity(4);
                for pair in fields.pairs::<mlua::String, mlua::Value>() {
                    let (k, v) = pair?;
                    let json_val = match v {
                        mlua::Value::Number(n) => serde_json::Value::from(n),
                        mlua::Value::Integer(n) => serde_json::Value::from(n as f64),
                        mlua::Value::Boolean(b) => serde_json::Value::from(b),
                        mlua::Value::String(s) => {
                            serde_json::Value::String(s.to_string_lossy().to_string())
                        }
                        mlua::Value::Nil => serde_json::Value::Null,
                        mlua::Value::Table(_) => lua_to_json(&v),
                        _ => serde_json::Value::Null,
                    };
                    let key_arc = crate::scripting::intern::intern(&k.to_string_lossy());
                    out.push((key_arc, json_val));
                }
                let comp_arc = crate::scripting::intern::intern(
                    &component_name.to_string_lossy(),
                );
                s.borrow_mut().pending.push(BridgeCommand::SetCustomFields {
                    entity_id,
                    component_name: comp_arc,
                    fields: out,
                });
                Ok(())
            },
        )?;
        globals.set("set_fields", f)?;
    }

    // --- Persistent K/V store (survives scene transitions) ---
    {
        let f = lua.create_function(|_, (key, value): (String, f64)| {
            crate::scripting::persistent::set_number(&key, value);
            Ok(())
        })?;
        globals.set("persistent_set_number", f)?;
    }
    {
        let f = lua.create_function(|_, key: String| -> LuaResult<Option<f64>> {
            Ok(crate::scripting::persistent::get_number(&key))
        })?;
        globals.set("persistent_get_number", f)?;
    }
    {
        let f = lua.create_function(|_, (key, value): (String, String)| {
            crate::scripting::persistent::set_string(&key, &value);
            Ok(())
        })?;
        globals.set("persistent_set_string", f)?;
    }
    {
        let f = lua.create_function(|_, key: String| -> LuaResult<Option<String>> {
            Ok(crate::scripting::persistent::get_string(&key))
        })?;
        globals.set("persistent_get_string", f)?;
    }
    {
        let f = lua.create_function(|_, key: String| -> LuaResult<bool> {
            Ok(crate::scripting::persistent::has(&key))
        })?;
        globals.set("persistent_has", f)?;
    }
    {
        let f = lua.create_function(|_, ()| {
            crate::scripting::persistent::clear();
            Ok(())
        })?;
        globals.set("persistent_clear", f)?;
    }

    // ── 2.3 Save game system ────────────────────────────────────────────
    // `save_set(slot, key, value)` — queue a write; nothing is persisted
    // until `save_commit(slot)` is called. `save_get(slot, key)` reads
    // synchronously from the per-frame snapshot.
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (slot, key, value): (String, String, LuaValue)| -> LuaResult<()> {
                let json = lua_to_json(&value);
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::SaveSet { slot, key, value: json });
                Ok(())
            },
        )?;
        globals.set("save_set", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |lua_ctx, (slot, key): (String, String)| -> LuaResult<LuaValue> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                match snap.get_save_value(&slot, &key) {
                    Some(v) => json_to_lua(lua_ctx, v),
                    None => Ok(LuaValue::Nil),
                }
            },
        )?;
        globals.set("save_get", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, slot: String| -> LuaResult<()> {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SaveCommit { slot });
            Ok(())
        })?;
        globals.set("save_commit", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, slot: String| -> LuaResult<()> {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::SaveClear { slot });
            Ok(())
        })?;
        globals.set("save_clear", f)?;
    }

    // ── 2.4 Audio mixer (bus tree) ──────────────────────────────────────
    // `audio_create_bus(name, parent?, volume?)`, `audio_set_bus_volume`,
    // `audio_get_bus_volume`, `audio_set_bus_muted`, `audio_is_bus_muted`,
    // `audio_set_bus_solo`, `audio_remove_bus`. All writes flow through
    // bridge commands; reads use the per-frame snapshot.
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (name, parent, volume): (String, Option<String>, Option<f32>)| -> LuaResult<()> {
                s.borrow_mut().pending.push(BridgeCommand::AudioCreateBus {
                    name,
                    parent,
                    volume: volume.unwrap_or(1.0),
                });
                Ok(())
            },
        )?;
        globals.set("audio_create_bus", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, name: String| -> LuaResult<()> {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::AudioRemoveBus { name });
            Ok(())
        })?;
        globals.set("audio_remove_bus", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (name, volume): (String, f32)| -> LuaResult<()> {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::AudioSetBusVolume { name, volume });
            Ok(())
        })?;
        globals.set("audio_set_bus_volume", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (name, effect, value): (String, String, f32)| -> LuaResult<()> {
            s.borrow_mut().pending.push(BridgeCommand::AudioSetBusEffect { name, effect, value });
            Ok(())
        })?;
        globals.set("audio_set_bus_effect", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, name: String| -> LuaResult<Option<f32>> {
            Ok(s.borrow().snapshot()?.get_bus_volume(&name))
        })?;
        globals.set("audio_get_bus_volume", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (name, muted): (String, bool)| -> LuaResult<()> {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::AudioSetBusMuted { name, muted });
            Ok(())
        })?;
        globals.set("audio_set_bus_muted", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, name: String| -> LuaResult<bool> {
            Ok(s.borrow().snapshot()?.is_bus_muted(&name))
        })?;
        globals.set("audio_is_bus_muted", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, (name, solo): (String, bool)| -> LuaResult<()> {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::AudioSetBusSolo { name, solo });
            Ok(())
        })?;
        globals.set("audio_set_bus_solo", f)?;
    }

    // ── 2.5 Input action remapping ──────────────────────────────────────
    // `input_get_actions()`, `input_get_bindings(name)`,
    // `input_set_bindings(name, keys)`, `input_add_binding(name, key)`,
    // `input_remove_binding(name, key)`, `input_clear_bindings(name)`,
    // `input_reset_bindings()`, `input_get_dead_zone(name)`,
    // `input_set_dead_zone(name, value)`. Writes flow through bridge
    // commands; reads use the per-frame snapshot.
    {
        let s = state.clone();
        let f = lua.create_function(move |lua, _: ()| -> LuaResult<mlua::Table> {
            let names = s.borrow().snapshot()?.list_action_names();
            let table = lua.create_table_with_capacity(names.len(), 0)?;
            for (i, name) in names.into_iter().enumerate() {
                table.raw_set(i + 1, name)?;
            }
            Ok(table)
        })?;
        globals.set("input_get_actions", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |lua, action: String| -> LuaResult<Option<mlua::Table>> {
                let st = s.borrow();
                let snap = st.snapshot()?;
                let Some(keys) = snap.get_action_bindings(&action) else {
                    return Ok(None);
                };
                let table = lua.create_table_with_capacity(keys.len(), 0)?;
                for (i, k) in keys.iter().enumerate() {
                    table.raw_set(i + 1, k.clone())?;
                }
                Ok(Some(table))
            },
        )?;
        globals.set("input_get_bindings", f)?;
    }
    {
        // `input_get_just_pressed()` - what went down this frame, by binding
        // name: a rebinding screen's "press a key".
        let s = state.clone();
        let f = lua.create_function(move |lua, _: ()| -> LuaResult<mlua::Table> {
            let st = s.borrow();
            let names = &st.snapshot()?.just_pressed_bindings;
            let table = lua.create_table_with_capacity(names.len(), 0)?;
            for (i, name) in names.iter().enumerate() {
                table.raw_set(i + 1, name.clone())?;
            }
            Ok(table)
        })?;
        globals.set("input_get_just_pressed", f)?;
    }
    {
        // `input_get_typed()` - the characters typed this frame, as the
        // layout made them: a command key that is a character (a roguelike's `?`
        // and `<`) rather than a place on the keyboard.
        let s = state.clone();
        let f = lua.create_function(move |_, _: ()| -> LuaResult<String> {
            let st = s.borrow();
            Ok(st.snapshot()?.typed.clone())
        })?;
        globals.set("input_get_typed", f)?;
    }
    // Crossfades, stem layers, playlists, pitch and seek - the music
    // director (`audio::music`), which every host drives once a frame. The
    // arguments go through the one parser the browsers' hook calls too.
    let music = |op: &'static str| {
        lua.create_function(move |lua, args: mlua::MultiValue| -> LuaResult<LuaValue> {
            let args: Vec<serde_json::Value> = args.iter().map(lua_value_to_json).collect();
            Ok(match crate::audio::music::script_op(op, &args) {
                serde_json::Value::String(s) => LuaValue::String(lua.create_string(&s)?),
                serde_json::Value::Number(n) => LuaValue::Number(n.as_f64().unwrap_or(0.0)),
                _ => LuaValue::Nil,
            })
        })
    };
    globals.set("audio_crossfade_music", music("crossfade")?)?;
    globals.set("audio_play_music_layers", music("layers")?)?;
    globals.set("audio_set_music_layer", music("layer")?)?;
    globals.set("audio_set_playlist", music("playlist")?)?;
    globals.set("audio_next_track", music("next")?)?;
    globals.set("audio_set_music_pitch", music("pitch")?)?;
    globals.set("audio_seek_music", music("seek")?)?;
    globals.set("audio_set_sound_pitch", music("sound_pitch")?)?;
    globals.set("audio_get_music_clip", music("clip")?)?;
    globals.set("audio_get_music_layer", music("layer_volume")?)?;
    {
        // Hold, tap and double-tap on actions; pad names and families; the
        // device used last; button prompts - all answered by
        // `input::interactions::InputView`, which the browsers' hook reads too.
        let s = state.clone();
        let f = lua.create_function(move |_, action: String| -> LuaResult<f32> {
            Ok(s.borrow().snapshot()?.input_view.hold_time(&action))
        })?;
        globals.set("get_action_hold_time", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, (action, secs): (String, Option<f32>)| -> LuaResult<bool> {
            Ok(s.borrow().snapshot()?.input_view.held(&action, secs))
        })?;
        globals.set("is_action_held", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, (action, secs): (String, Option<f32>)| -> LuaResult<bool> {
            Ok(s.borrow().snapshot()?.input_view.tapped(&action, secs))
        })?;
        globals.set("is_action_tapped", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, (action, secs): (String, Option<f32>)| -> LuaResult<bool> {
            Ok(s.borrow().snapshot()?.input_view.double_tapped(&action, secs))
        })?;
        globals.set("is_action_double_tapped", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, pad: Option<i64>| -> LuaResult<String> {
            Ok(s.borrow().snapshot()?.input_view.pad_name(pad.unwrap_or(0).max(0) as usize).to_string())
        })?;
        globals.set("get_gamepad_name", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, pad: Option<i64>| -> LuaResult<String> {
            Ok(s.borrow().snapshot()?.input_view.pad_type(pad.unwrap_or(0).max(0) as usize).to_string())
        })?;
        globals.set("get_gamepad_type", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<String> {
            Ok(s.borrow().snapshot()?.input_view.last_device.name().to_string())
        })?;
        globals.set("get_last_input_device", f)?;
        let s = state.clone();
        let f = lua.create_function(move |_, ()| -> LuaResult<i64> {
            Ok(s.borrow().snapshot()?.input_view.last_pad.map(|p| p as i64).unwrap_or(-1))
        })?;
        globals.set("get_last_gamepad", f)?;
        let s = state.clone();
        let f = lua.create_function(move |lua, (action, style): (String, Option<String>)| -> LuaResult<mlua::Table> {
            let p = s.borrow().snapshot()?.input_view.prompt(&action, style.as_deref());
            let t = lua.create_table()?;
            t.set("label", p.label)?;
            t.set("glyph", p.glyph)?;
            t.set("binding", p.binding)?;
            t.set("style", p.style)?;
            Ok(t)
        })?;
        globals.set("get_action_prompt", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (action, keys): (String, Vec<String>)| -> LuaResult<()> {
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::InputSetBindings { action, keys });
                Ok(())
            },
        )?;
        globals.set("input_set_bindings", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (action, key): (String, String)| -> LuaResult<()> {
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::InputAddBinding { action, key });
                Ok(())
            },
        )?;
        globals.set("input_add_binding", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (action, key): (String, String)| -> LuaResult<()> {
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::InputRemoveBinding { action, key });
                Ok(())
            },
        )?;
        globals.set("input_remove_binding", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, action: String| -> LuaResult<()> {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::InputClearBindings { action });
            Ok(())
        })?;
        globals.set("input_clear_bindings", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, _: ()| -> LuaResult<()> {
            s.borrow_mut()
                .pending
                .push(BridgeCommand::InputResetBindings);
            Ok(())
        })?;
        globals.set("input_reset_bindings", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(move |_, action: String| -> LuaResult<f32> {
            Ok(s.borrow().snapshot()?.get_action_dead_zone(&action))
        })?;
        globals.set("input_get_dead_zone", f)?;
    }
    {
        let s = state.clone();
        let f = lua.create_function(
            move |_, (action, value): (String, f32)| -> LuaResult<()> {
                s.borrow_mut()
                    .pending
                    .push(BridgeCommand::InputSetDeadZone { action, value });
                Ok(())
            },
        )?;
        globals.set("input_set_dead_zone", f)?;
    }

    Ok(())
}

// --- Error mapping ------------------------------------------------------------------------

fn make_error<E: std::fmt::Display>(
    script_path: Option<String>,
    kind: ScriptErrorKind,
    err: E,
) -> ScriptError {
    let message = err.to_string();
    let location = parse_location(&message);
    ScriptError {
        backend: BackendKind::Luau,
        script_path: script_path.unwrap_or_default(),
        location,
        kind,
        message,
        bridge_call: None,
    }
}

fn make_error_msg(
    script_path: Option<String>,
    kind: ScriptErrorKind,
    message: &str,
) -> ScriptError {
    ScriptError {
        backend: BackendKind::Luau,
        script_path: script_path.unwrap_or_default(),
        location: None,
        kind,
        message: message.to_string(),
        bridge_call: None,
    }
}

/// Distinguish syntax vs runtime errors when mlua surfaces them.
fn classify_error(script_path: Option<String>, e: LuaError) -> ScriptError {
    let kind = match &e {
        LuaError::SyntaxError { .. } => ScriptErrorKind::Syntax,
        _ => ScriptErrorKind::Runtime,
    };
    make_error(script_path, kind, e)
}

/// Parse a `[string "path"]:LINE: ...` or `path:LINE:COL: ...` style prefix into a
/// [`SourceLocation`]. Returns `None` if no recognizable line marker is present.
fn parse_location(message: &str) -> Option<SourceLocation> {
    // Try `]:LINE:` first (mlua's chunk-name-decorated form).
    if let Some(idx) = message.find("]:") {
        let rest = &message[idx + 2..];
        let line_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !line_str.is_empty()
            && let Ok(line) = line_str.parse::<u32>() {
                return Some(SourceLocation { line, column: None });
            }
    }
    // Fallback: bare `:LINE:` after the first colon.
    let parts: Vec<&str> = message.splitn(3, ':').collect();
    if parts.len() >= 2
        && let Ok(line) = parts[1].trim().parse::<u32>() {
            return Some(SourceLocation { line, column: None });
        }
    None
}

// --- Slice A: dynamic-function value marshalling -----------------------------------------

/// Convert one Lua value into a bridge [`Value`] following the descriptor
/// type. `Vec2` consumes a single Lua table `{x=, y=}` (or array
/// `{x, y}`); `EntityId` and integer-shaped numerics accept Lua integers
/// or floats.
/// A Lua table as JSON, the way `gui_set` wants a value: a sequence is an
/// array, anything else an object keyed by its string (or number) keys,
/// nested tables recursing. Functions and userdata are null.
fn lua_table_to_json(t: &LuaTable) -> serde_json::Value {
    use serde_json::Value as J;
    fn one(v: LuaValue, depth: u32) -> J {
        match v {
            LuaValue::Boolean(b) => J::Bool(b),
            LuaValue::Integer(i) => J::from(i as f64),
            LuaValue::Number(n) => J::from(n),
            LuaValue::String(s) => s.to_str().map(|s| J::String(s.to_string())).unwrap_or(J::Null),
            LuaValue::Table(t) if depth < 16 => table(&t, depth + 1),
            _ => J::Null,
        }
    }
    fn table(t: &LuaTable, depth: u32) -> J {
        let len = t.raw_len();
        if len > 0 {
            return J::Array((1..=len).map(|i| one(t.raw_get::<LuaValue>(i).unwrap_or(LuaValue::Nil), depth)).collect());
        }
        let mut map = serde_json::Map::new();
        for (k, v) in t.clone().pairs::<LuaValue, LuaValue>().flatten() {
            let key = match k {
                LuaValue::String(s) => s.to_str().map(|s| s.to_string()).unwrap_or_default(),
                LuaValue::Integer(i) => i.to_string(),
                LuaValue::Number(n) => n.to_string(),
                _ => continue,
            };
            map.insert(key, one(v, depth));
        }
        J::Object(map)
    }
    table(t, 0)
}

/// Convert a Lua value to JSON for `set_ui_prop`.
///
/// Hand-written because mlua's `serialize` feature is off in this build, and
/// enabling it to serve one call site would pull serde into every Lua value
/// conversion. Only the shapes a widget property can take are handled: scalars,
/// and a sequence table for a `[r, g, b, a]` color.
/// An event's payload as the text it travels as - what the browser preamble
/// makes of the same value with `JSON.stringify`: a string as it is, a
/// number, boolean or table as JSON, nil as no payload. The binding used to
/// take a string only, so `Event.send(name, e, { d = 1 })` threw natively
/// while the editor delivered the table.
fn event_data_text(v: &LuaValue) -> Option<String> {
    match v {
        LuaValue::Nil => None,
        LuaValue::String(s) => s.to_str().ok().map(|s| s.to_string()),
        LuaValue::Integer(i) => Some(i.to_string()),
        LuaValue::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => Some(format!("{}", *n as i64)),
        other => Some(lua_value_to_json(other).to_string()),
    }
}

fn lua_value_to_json(v: &LuaValue) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        LuaValue::Boolean(b) => J::Bool(*b),
        LuaValue::Integer(i) => J::from(*i as f64),
        LuaValue::Number(n) => J::from(*n),
        LuaValue::String(s) => s.to_str().map(|s| J::String(s.to_string())).unwrap_or(J::Null),
        // A sequence is an array and anything keyed an object, nested as
        // deep as it goes - `Locale.get(key, { n = 3 })` needs the keys. It
        // used to read number sequences only, so every keyed table arrived
        // as `null`. An empty table is an empty **list**, `[]`, in every
        // runtime (Fengari's `luaToJs` agrees): Lua cannot tell `[]` from
        // `{}`, and an empty list is what scripts set - `Line.set(e,
        // "points", {})` arrived as `null` and was refused by a sequence
        // field. See docs/parity-gaps.md.
        LuaValue::Table(t) => {
            let len = t.raw_len();
            let pairs: Vec<(LuaValue, LuaValue)> = t.clone().pairs::<LuaValue, LuaValue>().filter_map(|r| r.ok()).collect();
            if pairs.is_empty() {
                return J::Array(Vec::new());
            }
            if len > 0 && pairs.len() == len {
                return J::Array((1..=len).map(|i| lua_value_to_json(&t.raw_get::<LuaValue>(i).unwrap_or(LuaValue::Nil))).collect());
            }
            let mut map = serde_json::Map::new();
            for (k, v) in &pairs {
                let key = match k {
                    LuaValue::String(s) => s.to_str().map(|s| s.to_string()).unwrap_or_default(),
                    LuaValue::Integer(i) => i.to_string(),
                    LuaValue::Number(n) => n.to_string(),
                    _ => continue,
                };
                map.insert(key, lua_value_to_json(v));
            }
            J::Object(map)
        }
        _ => J::Null,
    }
}

fn lua_to_value(
    ty: &crate::package::script_api::ScriptValueType,
    v: LuaValue,
) -> LuaResult<crate::scripting::bridge::value::Value> {
    use crate::package::script_api::ScriptValueType as T;
    use crate::scripting::bridge::value::Value as V;
    match ty {
        T::Nil => Ok(V::Nil),
        T::Bool => match v {
            LuaValue::Boolean(b) => Ok(V::Bool(b)),
            LuaValue::Nil => Ok(V::Bool(false)),
            other => Err(LuaError::FromLuaConversionError {
                from: other.type_name(),
                to: "bool".into(),
                message: None,
            }),
        },
        T::Int | T::Float => match v {
            LuaValue::Integer(i) => Ok(V::Number(i as f64)),
            LuaValue::Number(n) => Ok(V::Number(n)),
            other => Err(LuaError::FromLuaConversionError {
                from: other.type_name(),
                to: "number".into(),
                message: None,
            }),
        },
        // `Any` takes the value as-is: the descriptor cannot know the type
        // because it depends on a sibling argument (set_ui_prop's property).
        T::Any => match v {
            LuaValue::Nil => Ok(V::Nil),
            LuaValue::Boolean(b) => Ok(V::Bool(b)),
            LuaValue::Integer(i) => Ok(V::Number(i as f64)),
            LuaValue::Number(n) => Ok(V::Number(n)),
            LuaValue::String(s) => Ok(V::String(s.to_str()?.to_string())),
            other => Err(LuaError::FromLuaConversionError {
                from: other.type_name(),
                to: "any".into(),
                message: None,
            }),
        },
        T::String => match v {
            LuaValue::String(s) => Ok(V::String(s.to_str()?.to_string())),
            other => Err(LuaError::FromLuaConversionError {
                from: other.type_name(),
                to: "string".into(),
                message: None,
            }),
        },
        T::EntityId => match v {
            LuaValue::Integer(i) => Ok(V::EntityId(i as u64)),
            LuaValue::Number(n) => Ok(V::EntityId(n as u64)),
            other => Err(LuaError::FromLuaConversionError {
                from: other.type_name(),
                to: "entity_id".into(),
                message: None,
            }),
        },
        T::Vec2 => match v {
            LuaValue::Table(t) => {
                let x: f64 = t
                    .get::<f64>("x")
                    .or_else(|_| t.get::<f64>(1))
                    .unwrap_or(0.0);
                let y: f64 = t
                    .get::<f64>("y")
                    .or_else(|_| t.get::<f64>(2))
                    .unwrap_or(0.0);
                Ok(V::Vec2(x as f32, y as f32))
            }
            other => Err(LuaError::FromLuaConversionError {
                from: other.type_name(),
                to: "vec2".into(),
                message: None,
            }),
        },
        // Only ever a return type (`get_sprite_color`); no function takes one.
        T::Rgba => Err(LuaError::FromLuaConversionError {
            from: v.type_name(),
            to: "rgba".into(),
            message: Some("rgba is a return type only".into()),
        }),
        T::VarArgs => {
            // Best-effort: accept any single value, store it as-is using a
            // recursive heuristic. Rare path; not optimised.
            Ok(match v {
                LuaValue::Nil => V::Nil,
                LuaValue::Boolean(b) => V::Bool(b),
                LuaValue::Integer(i) => V::Number(i as f64),
                LuaValue::Number(n) => V::Number(n),
                LuaValue::String(s) => V::String(s.to_str()?.to_string()),
                _ => V::Nil,
            })
        }
    }
}

/// Convert a bridge [`Value`] back into a Lua value for return marshalling.
/// `Value::Nil` becomes `LuaValue::Nil` (no return). Tables for Vec2/Color
/// follow `{x=, y=}` / `{r=, g=, b=, a=}` conventions used elsewhere in the
/// adapter.
fn value_to_lua(v: &crate::scripting::bridge::value::Value) -> LuaValue {
    use crate::scripting::bridge::value::Value as V;
    match v {
        V::Nil => LuaValue::Nil,
        V::Bool(b) => LuaValue::Boolean(*b),
        V::Number(n) => LuaValue::Number(*n),
        V::String(s) => {
            // Allocating a Lua string requires a Lua handle which we don't
            // have here; return a Nil and log. Handlers that need to return
            // strings must use a richer ctx in a follow-up slice.
            log::trace!("value_to_lua: returning string '{}' as nil (no Lua handle in scope)", s);
            LuaValue::Nil
        }
        V::EntityId(id) => LuaValue::Integer(*id as mlua::Integer),
        V::Vec2(_, _) | V::Color(..) | V::Handle(..) | V::Array(_) | V::Map(_) => {
            // Same limitation: complex returns need a Lua handle. Slice A
            // handlers (parallax) all return Nil so this is fine for now.
            LuaValue::Nil
        }
    }
}

/// Phase D — Lua-handle-aware variant of [`value_to_lua`]. Used by
/// [`LuauAdapter::named_callback_with_args`] /
/// [`LuauAdapter::event_with_data`] so primitive payloads (including
/// strings, EntityId, Vec2, Color) reach the receiver intact instead of
/// being silently coerced to nil. Complex containers
/// (`Array`/`Map`/`Handle`) still flatten to nil — Phase D scope.
fn value_to_lua_in(
    lua: &Lua,
    v: &crate::scripting::bridge::value::Value,
) -> LuaResult<LuaValue> {
    use crate::scripting::bridge::value::Value as V;
    Ok(match v {
        V::Nil => LuaValue::Nil,
        V::Bool(b) => LuaValue::Boolean(*b),
        V::Number(n) => LuaValue::Number(*n),
        V::String(s) => LuaValue::String(lua.create_string(s)?),
        V::EntityId(id) => LuaValue::Integer(*id as mlua::Integer),
        V::Vec2(x, y) => {
            let t = lua.create_table()?;
            t.set("x", *x)?;
            t.set("y", *y)?;
            LuaValue::Table(t)
        }
        V::Color(r, g, b, a) => {
            let t = lua.create_table()?;
            t.set("r", *r)?;
            t.set("g", *g)?;
            t.set("b", *b)?;
            t.set("a", *a)?;
            LuaValue::Table(t)
        }
        V::Handle(_, id) => LuaValue::Integer(*id as mlua::Integer),
        // Tables, recursively. These were **nil** until Sep 2026, which made
        // every payload-bearing event on native useless: a collision
        // handler's documented `{ other = ... }` and an animation frame's
        // event data both arrived as nothing, and indexing them errored the
        // handler out. The browser hosts built real objects all along, so
        // this was also a one-runtime silent difference.
        V::Array(items) => {
            let t = lua.create_table()?;
            for (i, item) in items.iter().enumerate() {
                t.set(i + 1, value_to_lua_in(lua, item)?)?;
            }
            LuaValue::Table(t)
        }
        V::Map(map) => {
            let t = lua.create_table()?;
            for (k, v) in map {
                t.set(k.as_str(), value_to_lua_in(lua, v)?)?;
            }
            LuaValue::Table(t)
        }
    })
}

// --- recursive serde_json::Value ↔ LuaValue helpers ----------------------------
//
// Used by `save_get` / `save_set` and by the custom-component bridge
// (`get_custom`, `set_custom`, `set_fields`), so nested tables and arrays
// round-trip instead of flattening to nil.
fn json_to_lua(lua: &Lua, v: &serde_json::Value) -> LuaResult<LuaValue> {
    match v {
        serde_json::Value::Null => Ok(LuaValue::Nil),
        serde_json::Value::Bool(b) => Ok(LuaValue::Boolean(*b)),
        serde_json::Value::Number(n) => Ok(LuaValue::Number(n.as_f64().unwrap_or(0.0))),
        serde_json::Value::String(s) => Ok(LuaValue::String(lua.create_string(s)?)),
        serde_json::Value::Array(arr) => {
            let t = lua.create_table()?;
            for (i, item) in arr.iter().enumerate() {
                // Lua arrays are 1-indexed.
                t.set(i as mlua::Integer + 1, json_to_lua(lua, item)?)?;
            }
            Ok(LuaValue::Table(t))
        }
        serde_json::Value::Object(map) => {
            let t = lua.create_table()?;
            for (k, val) in map.iter() {
                t.set(k.as_str(), json_to_lua(lua, val)?)?;
            }
            Ok(LuaValue::Table(t))
        }
    }
}

/// A Lua number as JSON the way the browsers write it: a whole number is an
/// integer (`12`, as `JSON.stringify` gives), anything else a float. Luau has
/// one number type, so `12` arrived as `12.0` - and serde refuses `12.0` for a
/// `u32` field, so a package command whose argument table held a count
/// (`Shooter.emitter_configure(e, { phases = {{{ count = 12 }}} })`) failed
/// natively and only natively.
fn lua_number_json(n: f64) -> serde_json::Value {
    const SAFE: f64 = 9_007_199_254_740_992.0; // 2^53: every integer below it is exact
    if n.fract() == 0.0 && n.abs() < SAFE {
        serde_json::Value::from(n as i64)
    } else {
        serde_json::Value::from(n)
    }
}

fn lua_to_json(v: &LuaValue) -> serde_json::Value {
    match v {
        LuaValue::Nil => serde_json::Value::Null,
        LuaValue::Boolean(b) => serde_json::Value::Bool(*b),
        LuaValue::Integer(n) => serde_json::Value::from(*n),
        LuaValue::Number(n) => lua_number_json(*n),
        LuaValue::String(s) => serde_json::Value::String(s.to_string_lossy().to_string()),
        LuaValue::Table(t) => {
            // Decide array vs object: if every key is a positive integer
            // forming a 1..=n run, emit a JSON array; otherwise an object.
            let len = t.raw_len() as i64;
            let mut is_array = len > 0;
            if is_array {
                for i in 1..=len {
                    if t.contains_key(i).unwrap_or(false) {
                        continue;
                    }
                    is_array = false;
                    break;
                }
            }
            // Also fall back to object form if the table has any
            // non-integer keys (mixed maps).
            let mut has_non_int = false;
            for (k, _) in t.clone().pairs::<LuaValue, LuaValue>().flatten() {
                if !matches!(k, LuaValue::Integer(_)) {
                    has_non_int = true;
                    break;
                }
            }
            if is_array && !has_non_int {
                let mut arr = Vec::with_capacity(len as usize);
                for i in 1..=len {
                    let val: LuaValue = t.get(i).unwrap_or(LuaValue::Nil);
                    arr.push(lua_to_json(&val));
                }
                serde_json::Value::Array(arr)
            } else {
                let mut map = serde_json::Map::new();
                for (k, val) in t.clone().pairs::<LuaValue, LuaValue>().flatten() {
                    let key = match k {
                        LuaValue::String(s) => s.to_string_lossy().to_string(),
                        LuaValue::Integer(n) => n.to_string(),
                        LuaValue::Number(n) => n.to_string(),
                        _ => continue,
                    };
                    map.insert(key, lua_to_json(&val));
                }
                // An empty table is an empty list, as in `lua_value_to_json`
                // and in Fengari's `luaToJs` - one answer in every runtime.
                if map.is_empty() {
                    return serde_json::Value::Array(Vec::new());
                }
                serde_json::Value::Object(map)
            }
        }
        _ => serde_json::Value::Null,
    }
}

// --- Tests --------------------------------------------------------------------------------


/// A screen point (window pixels, y down) as the world point under it: the
/// same steps as `Camera2D::screen_to_world`, on the numbers the snapshot
/// carries - offset from the centre, scaled by the view over zoom, turned by
/// the camera's rotation. The browser preamble does the same arithmetic on
/// what its host publishes. With no screen or camera reported it degrades to
/// screen space rather than dividing by zero.
fn snapshot_screen_to_world(
    snap: &crate::scripting::bridge::context::BridgeReadSnapshot,
    x: f32,
    y: f32,
) -> (f32, f32) {
    let (sw, sh) = snap.screen_size;
    if sw <= 0.0 || sh <= 0.0 {
        return (x, y);
    }
    let (cx, cy, vw, vh, zoom) = snap.camera_view;
    if zoom <= 0.0 {
        return (x - sw * 0.5, sh * 0.5 - y);
    }
    let vw = if vw > 0.0 { vw } else { sw };
    let vh = if vh > 0.0 { vh } else { sh };
    let nx = (x - sw * 0.5) / sw;
    let ny = (sh * 0.5 - y) / sh;
    let (ox, oy) = (nx * (vw / zoom), ny * (vh / zoom));
    let (sin, cos) = snap.camera_rotation.sin_cos();
    (cx + ox * cos - oy * sin, cy + ox * sin + oy * cos)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A script's numbers come out of `script_math` to the bit, every way
    /// Luau reaches them: the `math` library and its fast calls (arguments
    /// known only at run time), the compiler's constant folding (literals),
    /// `^` with a constant and with a variable exponent, `%`, and
    /// `math.random` after a seed. The browsers' Fengari calls the same
    /// functions, so this is half of "the same number everywhere"; the
    /// conformance scene (test lab 298) is the other half.
    #[test]
    fn script_maths_come_from_script_math() {
        use crate::scripting::script_math as m;
        crate::scripting::script_math::install_luau_table();
        let lua = Lua::new();
        install_script_random(&lua).unwrap();
        lua.sandbox(true).unwrap();
        // As the adapter compiles: level 2 folds constants and fast-calls builtins.
        let compiler = Compiler::new().set_optimization_level(2).set_debug_level(1);
        let source = r#"
            return function(x, y, a, b, e)
                math.randomseed(42)
                return {
                    math.sin(x), math.cos(x), math.tan(x), math.asin(y), math.acos(y),
                    math.atan(x), math.atan2(y, x), math.atan(y, x), math.exp(x),
                    math.log(x), math.log(x, 10), math.log(x, 3), math.log10(x),
                    math.pow(x, y), x ^ y, x ^ e, a ^ 0.5, a ^ e, x ^ 3,
                    math.sinh(y), math.cosh(y), math.tanh(y),
                    a % b,
                    math.sin(1.234), math.atan2(-1.5, 0.25), 1.7 ^ 2.3, math.exp(1.7),
                    math.random(), math.random(1, 6), math.random(100),
                }
            end
        "#;
        let bytecode = compiler.compile(source).unwrap();
        let f: LuaFunction = lua.load(&bytecode).eval().unwrap();
        let (x, y, a, b, e) = (1.234, 0.3, 5.3, 0.1, 0.5);
        let got: Vec<f64> = f.call((x, y, a, b, e)).unwrap();
        m::seed(42.0);
        let want = [
            m::sin(x), m::cos(x), m::tan(x), m::asin(y), m::acos(y),
            m::atan(x), m::atan2(y, x), m::atan2(y, x), m::exp(x),
            m::log(x, None), m::log(x, Some(10.0)), m::log(x, Some(3.0)), m::log10(x),
            m::pow(x, y), m::pow(x, y), m::pow(x, e), m::pow(a, 0.5), m::pow(a, e), m::pow(x, 3.0),
            m::sinh(y), m::cosh(y), m::tanh(y),
            m::modulo(a, b),
            m::sin(1.234), m::atan2(-1.5, 0.25), m::pow(1.7, 2.3), m::exp(1.7),
            m::random_unit(), m::random_between(1.0, 6.0).unwrap(), m::random_between(1.0, 100.0).unwrap(),
        ];
        assert_eq!(got.len(), want.len());
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert_eq!(g.to_bits(), w.to_bits(), "value {i}: Luau {g:e}, script_math {w:e}");
        }
        // `%` fused into one instruction is a different number for these
        // operands - so this test would see `-ffp-contract` come back.
        let fused = (-(a / b).floor()).mul_add(b, a);
        assert_ne!(fused.to_bits(), m::modulo(a, b).to_bits(), "pick operands a fused `%` gets wrong");
    }

    /// Whole numbers reach a package command as integers, as they do from
    /// the browsers, so a `u32` field deserializes; fractions stay floats.
    #[test]
    fn whole_numbers_reach_package_commands_as_integers() {
        let lua = mlua::Lua::new();
        let t: LuaValue = lua.load("return { count = 12, speed = 2.5, list = { 1, 2, 3 }, big = 2^60 }").eval().unwrap();
        let j = lua_to_json(&t);
        assert_eq!(j["count"], serde_json::json!(12));
        assert!(j["count"].is_u64(), "{j}");
        assert_eq!(j["speed"], serde_json::json!(2.5));
        assert_eq!(j["list"], serde_json::json!([1, 2, 3]));
        assert!(j["big"].is_f64(), "past 2^53 a whole number stays a float: {j}");
        #[derive(serde::Deserialize)]
        struct Def { count: u32, speed: f32 }
        let d: Def = serde_json::from_value(j).unwrap();
        assert_eq!((d.count, d.speed), (12, 2.5));
    }

    /// The numbers a script's text is checked on: specials, whole numbers
    /// either side of 2^31 and 2^53, the edges of fixed notation, fractions,
    /// the smallest and largest doubles, and a spread of others.
    fn number_text_cases() -> Vec<f64> {
        let mut v = vec![
            0.0, -0.0, 1.0, -1.0, 3.0, 0.1, 0.5, 1.0 / 3.0, 2.0 / 3.0, 5.3 % 0.1, 1e-5, 1e-6, 1e-7, 0.000123,
            123456.789, 1e15, 1e16, 1e17, 1e20, 1e21, 1e22, 1.5e21, 2147483647.0, 2147483648.0, -2147483648.0,
            -2147483649.0, 4294967296.0, 4e9, 9007199254740992.0, 9007199254740993.0, 1.7976931348623157e308,
            5e-324, 2.2250738585072014e-308, 1e100, 1e-100, 12.5, -0.0001, 100.0, 1234567890123.0,
            f64::INFINITY, f64::NEG_INFINITY, f64::NAN,
        ];
        // A spread over the exponents, from a fixed seed.
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        for k in 0..200 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let exponent = ((x >> 20) % 80) as i32 - 40;
            let m = (x >> 11) as f64 / (1u64 << 53) as f64;
            v.push((if k % 2 == 0 { m } else { -m }) * crate::detmath::DetMath::dpowi(10f64, exponent));
            v.push(((x >> 33) as i64 - (1 << 30)) as f64);
        }
        v
    }

    /// What a number prints as, by the native Luau - `tostring` and
    /// `string.format("%d")` - written to `tests/fixtures/lua_number_text.json`,
    /// which the editor's test (`editor/src/lib/scriptMath.test.ts`) holds
    /// the patched Fengari to. `JENGINE_UPDATE_FIXTURE=1` writes it again.
    #[test]
    fn numbers_print_as_the_fixture_says() {
        let lua = Lua::new();
        let compiler = Compiler::new().set_optimization_level(2).set_debug_level(1);
        let f: LuaFunction = lua
            .load(&compiler.compile("return function(x) return tostring(x), string.format('%d', x) end").unwrap())
            .eval()
            .unwrap();
        let rows: Vec<serde_json::Value> = number_text_cases()
            .into_iter()
            .map(|x| {
                let (text, d): (String, String) = f.call(x).unwrap();
                // `%d` is compared below 2^53: past it Luau prints the
                // double's exact integer and the browsers' formatter its
                // shortest digits (`4478344367032335360` / `..400`).
                let d = (x.is_finite() && x.abs() < 9007199254740992.0).then_some(d);
                serde_json::json!({ "bits": format!("{:016x}", x.to_bits()), "text": text, "d": d })
            })
            .collect();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lua_number_text.json");
        let body = serde_json::to_string_pretty(&rows).unwrap() + "\n";
        if std::env::var("JENGINE_UPDATE_FIXTURE").is_ok() {
            std::fs::write(&path, &body).unwrap();
        }
        let have = std::fs::read_to_string(&path).expect("JENGINE_UPDATE_FIXTURE=1 writes the fixture");
        assert_eq!(have, body, "Luau prints these differently now; JENGINE_UPDATE_FIXTURE=1 rewrites the fixture");
        // Spot checks on what the editor used to get wrong.
        let text = |x: f64| -> String { f.call::<(String, String)>(x).unwrap().0 };
        assert_eq!(text(3.0), "3");
        assert_eq!(text(1.0 / 3.0), "0.3333333333333333");
        assert_eq!(text(4e9), "4000000000");
        assert_eq!(text(1e21), "1e+21");
        assert_eq!(f.call::<(String, String)>(37.99).unwrap().1, "37", "Luau's %d drops the fraction");
    }

    fn empty_ctx<'a>(read: &'a BridgeReadSnapshot, cmds: &'a mut Vec<BridgeCommand>) -> BridgeContext<'a> {
        BridgeContext::new(0.016, read, Capabilities::empty(), cmds)
    }

    #[test]
    fn loads_and_runs_lifecycle() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let src = r#"
            local ticks = 0
            function init(entity) log("init " .. entity) end
            function update(entity, dt) ticks = ticks + 1; set_position(entity, 1.5, 2.5) end
        "#;
        let h = a.load("test.luau", src).unwrap();

        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        let mut ctx = empty_ctx(&read, &mut cmds);
        a.init(h, 42, &mut ctx).unwrap();
        a.update(h, 42, &mut ctx).unwrap();

        assert!(matches!(cmds[0], BridgeCommand::Log { ref message } if message == "init 42"));
        assert_eq!(
            cmds[1],
            BridgeCommand::SetPosition { entity_id: 42, x: 1.5, y: 2.5 }
        );
    }

    #[test]
    fn read_snapshot_position_visible() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let src = r#"
            function update(entity, dt)
                local x, y = get_position(entity)
                set_position(entity, x + 1, y + 2)
            end
        "#;
        let h = a.load("read.luau", src).unwrap();

        let mut read = BridgeReadSnapshot::default();
        read.positions.insert(7, (10.0, 20.0));
        let mut cmds = Vec::new();
        let mut ctx = empty_ctx(&read, &mut cmds);
        a.update(h, 7, &mut ctx).unwrap();

        assert_eq!(cmds, vec![BridgeCommand::SetPosition { entity_id: 7, x: 11.0, y: 22.0 }]);
    }

    #[test]
    fn gui_set_takes_a_table_as_json() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let src = r#"
            function update(entity, dt)
                gui_set("hud.gui", "card", "bg_color", { 1, 0.5, 0, 1 })
                gui_set("hud.gui", "card", "shadow", { color = { 0, 0, 0, 0.5 }, offset = { 0, 4 }, blur = 8 })
            end
        "#;
        let h = a.load("gui.luau", src).unwrap();
        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        let mut ctx = empty_ctx(&read, &mut cmds);
        a.update(h, 1, &mut ctx).unwrap();
        let values: Vec<serde_json::Value> = cmds
            .iter()
            .filter_map(|c| match c {
                BridgeCommand::GuiSetNodeProp { value, .. } => serde_json::from_str(value).ok(),
                _ => None,
            })
            .collect();
        assert_eq!(values[0], serde_json::json!([1.0, 0.5, 0.0, 1.0]));
        assert_eq!(values[1]["blur"], serde_json::json!(8.0));
        assert_eq!(values[1]["offset"], serde_json::json!([0.0, 4.0]));
    }

    #[test]
    fn syntax_error_is_classified() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let err = a.load("bad.luau", "function broken(").unwrap_err();
        assert_eq!(err.backend, BackendKind::Luau);
        assert_eq!(err.kind, ScriptErrorKind::Syntax);
        assert_eq!(err.script_path, "bad.luau");
    }

    #[test]
    fn runtime_error_carries_line() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let src = "function update(entity, dt)\n    error(\"boom\")\nend\n";
        let h = a.load("rt.luau", src).unwrap();
        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        let mut ctx = empty_ctx(&read, &mut cmds);
        let err = a.update(h, 1, &mut ctx).unwrap_err();
        assert_eq!(err.backend, BackendKind::Luau);
        assert_eq!(err.kind, ScriptErrorKind::Runtime);
        assert!(err.location.is_some(), "expected line info, got: {}", err.message);
    }

    #[test]
    fn sandbox_blocks_loadstring() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        // loadstring is removed by Luau sandbox — referencing it yields nil and calling fails.
        let err = a
            .load("sb.luau", "loadstring(\"return 1\")()")
            .unwrap_err();
        assert_eq!(err.backend, BackendKind::Luau);
        assert_eq!(err.kind, ScriptErrorKind::Runtime);
    }

    #[test]
    fn scripts_have_isolated_globals() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let h1 = a
            .load("a.luau", "function update(e, dt) my_var = 1 end")
            .unwrap();
        let h2 = a
            .load("b.luau", "function update(e, dt) if my_var ~= nil then error(\"leak\") end end")
            .unwrap();
        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        let mut ctx = empty_ctx(&read, &mut cmds);
        a.update(h1, 1, &mut ctx).unwrap();
        a.update(h2, 1, &mut ctx).expect("script B should not see script A's globals");
    }

    // --- Script-reuse Phase A: require() -------------------------------------

    fn install_resolver<F>(a: &mut LuauAdapter, f: F)
    where
        F: Fn(&str) -> Result<String, String> + 'static,
    {
        a.set_module_resolver(std::rc::Rc::new(f));
    }

    #[test]
    fn require_returns_module_export() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        install_resolver(&mut a, |path| match path {
            "lib/math" => Ok("return { add = function(x, y) return x + y end }".into()),
            other => Err(format!("not found: {other}")),
        });
        let h = a
            .load(
                "user.luau",
                r#"
                local m = require("lib/math")
                function update(e, dt) set_position(e, m.add(1, 2), 0) end
            "#,
            )
            .unwrap();
        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        let mut ctx = empty_ctx(&read, &mut cmds);
        a.update(h, 5, &mut ctx).unwrap();
        assert_eq!(
            cmds,
            vec![BridgeCommand::SetPosition { entity_id: 5, x: 3.0, y: 0.0 }]
        );
    }

    #[test]
    fn require_caches_module_body() {
        // Module body increments a host-visible counter on every evaluation.
        // Two `require` calls must trigger only ONE evaluation.
        let count = std::rc::Rc::new(std::cell::Cell::new(0u32));
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let count_for_resolver = count.clone();
        install_resolver(&mut a, move |path| {
            assert_eq!(path, "lib/once");
            count_for_resolver.set(count_for_resolver.get() + 1);
            Ok("return { tag = 'ok' }".into())
        });
        a.load(
            "user.luau",
            r#"
            local a = require("lib/once")
            local b = require("lib/once")
            assert(a == b, "require should return the cached table")
            function update(e, dt) end
        "#,
        )
        .unwrap();
        assert_eq!(count.get(), 1, "resolver invoked once");
    }

    #[test]
    fn require_detects_circular_dependency() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        install_resolver(&mut a, |path| match path {
            "lib/a" => Ok(r#"require("lib/b"); return {}"#.into()),
            "lib/b" => Ok(r#"require("lib/a"); return {}"#.into()),
            other => Err(format!("not found: {other}")),
        });
        let err = a
            .load("user.luau", r#"local m = require("lib/a"); function update(e, dt) end"#)
            .unwrap_err();
        assert_eq!(err.backend, BackendKind::Luau);
        assert!(
            err.message.contains("circular"),
            "expected circular-dependency message, got: {}",
            err.message
        );
    }

    #[test]
    fn require_without_resolver_errors_clearly() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let err = a
            .load(
                "user.luau",
                r#"local m = require("lib/missing"); function update(e, dt) end"#,
            )
            .unwrap_err();
        assert_eq!(err.backend, BackendKind::Luau);
        assert!(
            err.message.contains("no module resolver"),
            "got: {}",
            err.message
        );
    }

    // --- Phase B: extends (prototype inheritance) -----------------------------

    #[test]
    fn extends_inherits_parent_update_when_child_omits_it() {
        // Parent module exports an `update` that fires SetPosition. Child
        // declares `extends = "lib/parent"` and defines NO `update` — the
        // engine must fall through to the parent's.
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        install_resolver(&mut a, |path| match path {
            "lib/parent" => Ok(r#"
                local M = {}
                function M.update(entity, dt)
                    set_position(entity, 7, 8)
                end
                return M
            "#.into()),
            other => Err(format!("not found: {other}")),
        });
        let h = a.load(
            "child.luau",
            r#"
                script = { extends = "lib/parent" }
                -- no update defined; should inherit parent's
            "#,
        ).unwrap();
        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        let mut ctx = empty_ctx(&read, &mut cmds);
        a.update(h, 42, &mut ctx).unwrap();
        assert_eq!(
            cmds,
            vec![BridgeCommand::SetPosition { entity_id: 42, x: 7.0, y: 8.0 }]
        );
    }

    #[test]
    fn extends_child_overrides_parent_update() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        install_resolver(&mut a, |path| match path {
            "lib/parent" => Ok(r#"
                local M = {}
                function M.update(entity, dt)
                    set_position(entity, 1, 1)
                end
                return M
            "#.into()),
            other => Err(format!("not found: {other}")),
        });
        let h = a.load(
            "child.luau",
            r#"
                script = { extends = "lib/parent" }
                function update(entity, dt)
                    set_position(entity, 99, 99)
                end
            "#,
        ).unwrap();
        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        let mut ctx = empty_ctx(&read, &mut cmds);
        a.update(h, 1, &mut ctx).unwrap();
        assert_eq!(
            cmds,
            vec![BridgeCommand::SetPosition { entity_id: 1, x: 99.0, y: 99.0 }]
        );
    }

    #[test]
    fn extends_super_dispatches_to_parent() {
        // Child defines its own update that calls super.update() to delegate
        // to the parent. Both should fire — parent's SetPosition first, then
        // child's tweak.
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        install_resolver(&mut a, |path| match path {
            "lib/parent" => Ok(r#"
                local M = {}
                function M.update(entity, dt)
                    set_position(entity, 10, 20)
                end
                return M
            "#.into()),
            other => Err(format!("not found: {other}")),
        });
        let h = a.load(
            "child.luau",
            r#"
                script = { extends = "lib/parent" }
                function update(entity, dt)
                    super.update(entity, dt)
                    set_position(entity, 30, 40)
                end
            "#,
        ).unwrap();
        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        let mut ctx = empty_ctx(&read, &mut cmds);
        a.update(h, 5, &mut ctx).unwrap();
        assert_eq!(
            cmds,
            vec![
                BridgeCommand::SetPosition { entity_id: 5, x: 10.0, y: 20.0 },
                BridgeCommand::SetPosition { entity_id: 5, x: 30.0, y: 40.0 },
            ]
        );
    }

    #[test]
    fn extends_multilevel_chain_resolves_grandparent() {
        // Child extends Parent extends Grandparent. Child defines nothing;
        // Parent defines nothing; only Grandparent defines `update`. The
        // chain must walk all the way up.
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        install_resolver(&mut a, |path| match path {
            "lib/grandparent" => Ok(r#"
                local M = {}
                function M.update(entity, dt)
                    set_position(entity, 100, 200)
                end
                return M
            "#.into()),
            "lib/parent" => Ok(r#"
                script = { extends = "lib/grandparent" }
                local M = {}
                return M
            "#.into()),
            other => Err(format!("not found: {other}")),
        });
        let h = a.load(
            "child.luau",
            r#"
                script = { extends = "lib/parent" }
            "#,
        ).unwrap();
        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        let mut ctx = empty_ctx(&read, &mut cmds);
        a.update(h, 9, &mut ctx).unwrap();
        assert_eq!(
            cmds,
            vec![BridgeCommand::SetPosition { entity_id: 9, x: 100.0, y: 200.0 }]
        );
    }

    #[test]
    fn extends_missing_parent_errors_clearly() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        install_resolver(&mut a, |path| Err(format!("not found: {path}")));
        let err = a.load(
            "child.luau",
            r#"script = { extends = "lib/missing" }"#,
        ).unwrap_err();
        assert!(
            err.message.contains("not found"),
            "expected resolver error to surface, got: {}",
            err.message
        );
    }

    #[test]
    fn extends_non_table_export_errors_clearly() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        install_resolver(&mut a, |path| match path {
            "lib/bad" => Ok("return 42".into()),
            other => Err(format!("not found: {other}")),
        });
        let err = a.load(
            "child.luau",
            r#"script = { extends = "lib/bad" }"#,
        ).unwrap_err();
        assert!(
            err.message.contains("did not return a table"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn extends_inherits_an_event_handler_and_not_the_engines_on_event() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        install_resolver(&mut a, |path| match path {
            "lib/grandparent" => Ok("local M = {}\nfunction M.on_ping(e, data) end\nreturn M".into()),
            "lib/parent" => Ok("script = { extends = \"lib/grandparent\" }\nreturn {}".into()),
            other => Err(format!("not found: {other}")),
        });
        let h = a.load("child.luau", r#"script = { extends = "lib/parent" }"#).unwrap();
        assert!(a.has_function(h, "on_ping"), "two levels up, through super");
        // `on_event` is an engine global; the chain must not reach it.
        assert!(!a.has_function(h, "on_event"));
        assert!(!a.has_function(h, "on_pong"));
    }

    // --- Namespaces (`scripting::namespaces`) --------------------------------

    fn run_update(a: &mut LuauAdapter, source: &str) -> Result<Vec<BridgeCommand>, String> {
        let h = a.load("ns.luau", source).map_err(|e| e.message)?;
        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        let mut ctx = empty_ctx(&read, &mut cmds);
        a.update(h, 4, &mut ctx).map_err(|e| e.message)?;
        drop(ctx);
        Ok(cmds)
    }

    #[test]
    fn a_namespaced_name_is_the_flat_function() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let cmds = run_update(
            &mut a,
            r#"
            function update(e, dt)
                Sprite.set_color(e, 1, 0, 0, 1)
                Transform.set_position(e, 3, 4)
                log(tostring(Sprite.set_color == set_sprite_color))
            end
            "#,
        )
        .unwrap();
        assert_eq!(
            cmds,
            vec![
                BridgeCommand::SetSpriteColor { entity_id: 4, r: 1.0, g: 0.0, b: 0.0, a: 1.0 },
                BridgeCommand::SetPosition { entity_id: 4, x: 3.0, y: 4.0 },
                BridgeCommand::Log { message: "true".into() },
            ]
        );
    }

    #[test]
    fn every_core_namespaced_name_resolves_natively() {
        // Every entry points at a global the adapter really installs - an
        // entry naming a flat global that does not exist is skipped with a
        // warning, which this turns into a failure.
        let a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let globals = a.lua.globals();
        let mut missing = Vec::new();
        for (ns, short, _) in crate::scripting::namespaces::CORE {
            let table: LuaResult<LuaTable> = globals.get(*ns);
            let ok = table.is_ok_and(|t| t.get::<LuaFunction>(*short).is_ok());
            if !ok {
                missing.push(format!("{ns}.{short}"));
            }
        }
        assert!(missing.is_empty(), "not callable on native: {missing:?}");
    }

    #[test]
    fn namespace_tables_are_read_only() {
        // Natively every script shares them; one script's assignment must not
        // reach the others.
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let err = run_update(&mut a, "function update(e, dt) Sprite.set_color = nil end").unwrap_err();
        assert!(err.contains("readonly"), "{err}");
    }

    #[test]
    fn a_package_gets_a_namespace_over_its_commands() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let args = vec!["line".to_string()];
        a.install_package_command("example_say", "pkg/jengine-example/example_say", &args);
        a.install_package_command("example_rest", "pkg/jengine-example/example_rest", &[]);
        let infos: Vec<_> = [("example_say", args.clone()), ("example_rest", vec![])]
            .into_iter()
            .map(|(n, args)| crate::package::script_commands::PackageCommandInfo {
                name: n.into(),
                tag: format!("pkg/jengine-example/{n}"),
                package: "jengine-example".into(),
                args,
            })
            .collect();
        a.install_namespaces(&crate::scripting::namespaces::package_entries(&infos));
        let cmds = run_update(&mut a, r#"function update(e, dt) Example.say("hello") end"#).unwrap();
        assert_eq!(
            cmds,
            vec![BridgeCommand::PackageCall {
                tag: "pkg/jengine-example/example_say".into(),
                args: serde_json::json!({ "line": "hello" }),
            }]
        );
    }

    #[test]
    fn print_goes_to_the_log_joined_with_tabs() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let cmds = run_update(&mut a, r#"function update(e, dt) print("a", 2, true) end"#).unwrap();
        assert_eq!(cmds, vec![BridgeCommand::Log { message: "a\t2\ttrue".into() }]);
    }

    /// An inheritance demo, from disk (`tests/fixtures/inheritance`): `goblin` extends `lib/npc` extends
    /// `lib/creature`, defines no `update` of its own, and calls up through
    /// `super`. Its first version defined the parent's `update` as a module
    /// global - which nothing that extends a module can see - while its
    /// comments promised the goblin inherited it. The editor half of this is
    /// in `editor/src/lib/luaRuntime.test.ts`.
    #[test]
    fn the_inheritance_demo_inherits_across_two_levels() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/inheritance/scripts");
        let src = std::fs::read_to_string(root.join("goblin.luau")).unwrap();
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        a.set_module_resolver(crate::scripting::bridge::module::fs_resolver(root));
        let h = a.load("goblin.luau", &src).unwrap();
        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        {
            let mut ctx = empty_ctx(&read, &mut cmds);
            a.init(h, 1, &mut ctx).unwrap();
            for _ in 0..60 {
                a.update(h, 1, &mut ctx).unwrap();
            }
        }
        let logs: Vec<&str> = cmds
            .iter()
            .filter_map(|c| match c {
                BridgeCommand::Log { message } => Some(message.as_str()),
                _ => None,
            })
            .collect();
        // `super` is lib/npc; `greet` is lib/creature's, found through it.
        assert!(logs.iter().any(|m| m.contains("hello — I am a npc")), "{logs:?}");
        // The goblin has no `update`: this is lib/creature's, two levels up.
        assert!(logs.iter().any(|m| m.contains("tick 60")), "{logs:?}");
    }

    /// An entity that does not exist reads as `nil`, as it does in the
    /// browser runtimes - it used to throw, killing the caller's `update`
    /// every frame (test-lab scene 18, found by `jengine android-smoke`).
    #[test]
    fn transform_reads_of_a_missing_entity_are_nil() {
        let mut a = LuauAdapter::new(Capabilities::empty()).unwrap();
        let h = a
            .load(
                "probe.luau",
                r#"
                function update(entity, dt)
                    local x, y = get_position(99)
                    local wx, wy = get_world_position(99)
                    local sx, sy = get_scale(99)
                    log(tostring(x) .. " " .. tostring(y) .. " " .. tostring(get_rotation(99)) .. " "
                        .. tostring(wx) .. " " .. tostring(wy) .. " " .. tostring(get_world_rotation(99)) .. " "
                        .. tostring(sx) .. " " .. tostring(get_world_scale(99)))
                end
                "#,
            )
            .unwrap();
        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        {
            let mut ctx = empty_ctx(&read, &mut cmds);
            a.update(h, 1, &mut ctx).unwrap();
        }
        let logs: Vec<&str> = cmds
            .iter()
            .filter_map(|c| match c {
                BridgeCommand::Log { message } => Some(message.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(logs, ["nil nil nil nil nil nil nil nil"]);
    }
}
