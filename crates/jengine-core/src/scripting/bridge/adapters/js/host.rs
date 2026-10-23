//! Host-side JS execution path for [`super::JsAdapter`] via `rquickjs`.
//!
//! **Test/parity-only.** Production web builds run the same conceptual script in
//! the browser via [`super::browser`]; this module exists so the native test
//! suite can drive a fixture Luau script through both [`super::super::luau::LuauAdapter`]
//! and the JS adapter and assert that both emit identical [`BridgeCommand`] streams.
//!
//! The bridge globals registered here mirror Luau's surface exactly:
//! - mutating: `set_position`, `set_rotation`, `set_scale`, `log`
//! - reads: `get_position`, `get_rotation`, `get_scale`, `get_name`
//!
//! Each loaded script is wrapped in an IIFE that returns its `init`/`update`
//! functions, so script-local declarations don't pollute the shared QuickJS
//! global object across multiple scripts loaded into one adapter.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rquickjs::convert::List;
use rquickjs::function::MutFn;
use rquickjs::function::Opt;
use rquickjs::function::Rest;
use rquickjs::{Context, Ctx, Function, Object, Persistent, Runtime};

use super::super::super::command::BridgeCommand;
use super::super::super::context::{BridgeContext, BridgeReadSnapshot};
use super::super::super::diagnostics::{BackendKind, ScriptError, ScriptErrorKind};
use super::super::super::module::{null_resolver, ModuleResolver};

/// Mutable state shared between the host JS engine and the closures registered
/// into QuickJS. Same pattern as [`super::super::luau::LuauAdapter`]'s
/// `BridgeState`: the snapshot pointer is set on entry to a lifecycle call and
/// cleared on exit, so callbacks can only fire while a call is active.
pub(super) struct HostBridgeState {
    snapshot: Option<*const BridgeReadSnapshot>,
    pending: Vec<BridgeCommand>,
    /// Phase 5.1: shared spawn-id counter (mirrors Luau adapter).
    spawn_counter: Option<Rc<Cell<u64>>>,
    /// Phase 5.5: shared sound-handle counter.
    sound_counter: Option<Rc<Cell<u64>>>,
    /// Phase 5.6: shared tween-id counter.
    tween_counter: Option<Rc<Cell<u64>>>,
    /// Phase 5.8: shared event-listener-id counter.
    event_listener_counter: Option<Rc<Cell<u64>>>,
    /// Phase 5.8: shared timer-id counter.
    timer_counter: Option<Rc<Cell<u64>>>,
    /// Script-reuse Phase A: host-supplied module resolver invoked by the
    /// JS-side `require` wrapper through the `__resolve_module__` global.
    resolver: ModuleResolver,
    /// Data-oriented design: shared registry of loaded `.data` tables read by
    /// the `get_data` global. Static for the session; installed by the host.
    data_registry: Rc<crate::data::DataRegistry>,
}

impl HostBridgeState {
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

    fn snapshot(&self) -> Option<&BridgeReadSnapshot> {
        // SAFETY: snapshot pointer lifetime is bracketed by `with_state`.
        unsafe { self.snapshot.and_then(|p| p.as_ref()) }
    }

    fn alloc_spawn_id(&self) -> Option<u64> {
        let counter = self.spawn_counter.as_ref()?;
        let id = counter.get();
        counter.set(id + 1);
        Some(id)
    }

    fn alloc_sound_id(&self) -> Option<u64> {
        let counter = self.sound_counter.as_ref()?;
        let id = counter.get();
        counter.set(id + 1);
        Some(id)
    }

    fn alloc_tween_id(&self) -> Option<u32> {
        let counter = self.tween_counter.as_ref()?;
        let id = counter.get();
        counter.set(id + 1);
        Some(id as u32)
    }

    fn alloc_event_listener_id(&self) -> Option<u32> {
        let counter = self.event_listener_counter.as_ref()?;
        let id = counter.get();
        counter.set(id + 1);
        Some(id as u32)
    }

    fn alloc_timer_id(&self) -> Option<u32> {
        let counter = self.timer_counter.as_ref()?;
        let id = counter.get();
        counter.set(id + 1);
        Some(id as u32)
    }
}

/// Per-script bookkeeping after a successful host load.
pub(super) struct HostScript {
    pub(super) init: Option<Persistent<Function<'static>>>,
    pub(super) update: Option<Persistent<Function<'static>>>,
    /// P7.7 wedge 19 — resolves arbitrary named callbacks (e.g.
    /// `on_gui_click`-routed handlers, animation `on_event` listeners) by
    /// `eval`ing the identifier against the IIFE's enclosing scope. Mirrors
    /// the `lookup` pattern added for `BrowserScriptFns` in wedge 15.
    pub(super) lookup: Option<Persistent<Function<'static>>>,
    pub(super) path: String,
}

/// Host JS engine — one QuickJS runtime + context shared across all scripts in
/// the owning [`super::JsAdapter`].
pub(super) struct HostJs {
    pub(super) state: Rc<RefCell<HostBridgeState>>,
    /// Declared **before** `runtime` so the Context drops first — QuickJS
    /// asserts at runtime drop that no live objects remain.
    pub(super) context: Context,
    #[allow(dead_code)] // kept alive for the context's lifetime
    runtime: Runtime,
}

impl HostJs {
    pub(super) fn new() -> Result<Self, ScriptError> {
        let runtime = Runtime::new().map_err(|e| make_err(None, ScriptErrorKind::Runtime, e))?;
        let context = Context::full(&runtime)
            .map_err(|e| make_err(None, ScriptErrorKind::Runtime, e))?;
        let state = Rc::new(RefCell::new(HostBridgeState::new()));
        register_globals(&context, &state)?;
        install_require_globals(&context, &state)?;
        Ok(Self { state, context, runtime })
    }

    /// Install the host-side resolver consulted by the `require` script global.
    /// Replaces any previously installed resolver. Modules already cached are
    /// not re-evaluated — call [`Self::invalidate_module_cache`] first if you
    /// need to drop them (e.g. on hot-reload).
    pub(super) fn set_module_resolver(&self, resolver: ModuleResolver) {
        self.state.borrow_mut().resolver = resolver;
    }

    /// Install the shared data-table registry read by the `get_data` global.
    pub(super) fn set_data_registry(&self, registry: Rc<crate::data::DataRegistry>) {
        self.state.borrow_mut().data_registry = registry;
    }

    /// Drop every cached module export. The next `require()` re-runs the
    /// module body via the resolver.
    pub(super) fn invalidate_module_cache(&self) -> Result<(), ScriptError> {
        self.context
            .with(|ctx| -> rquickjs::Result<()> {
                let _: rquickjs::Value = ctx.eval(
                    "__module_cache__ = {}; __module_loading__ = {}; undefined",
                )?;
                Ok(())
            })
            .map_err(|e| make_err(None, ScriptErrorKind::Runtime, e))
    }

    /// Compile a transpiled JS source. Wraps it in an IIFE so script-local
    /// `function init/update` definitions are scoped, then extracts and persists
    /// the returned functions.
    ///
    /// `extends_parent` (Phase B) — when `Some(parent_path)`, the wrapper
    /// resolves the parent module via `require` first, then sets the
    /// returned exports' `[[Prototype]]` to it. As a result `result.get("init")`
    /// walks the proto chain natively, and `lookup` falls through to the
    /// parent's lookup for inherited named callbacks.
    pub(super) fn load(
        &self,
        path: &str,
        js_source: &str,
        extends_parent: Option<&str>,
    ) -> Result<HostScript, ScriptError> {
        // The parent require + proto chain are emitted only when `extends`
        // is set so the no-inheritance hot path stays a single eval with
        // no extra function calls.
        let (parent_prelude, proto_chain, lookup_fallback) = match extends_parent {
            Some(p) => (
                // JSON-encode the path so embedded quotes can't break out
                // of the string literal. Library paths are user-provided.
                format!(
                    "var __parent__ = globalThis.require({});\n",
                    serialize_js_string(p)
                ),
                "if (__parent__) Object.setPrototypeOf(__exports__, __parent__);".to_string(),
                "if (__parent__ && typeof __parent__.lookup === 'function') return __parent__.lookup(name);".to_string(),
            ),
            None => (String::new(), String::new(), String::new()),
        };
        let wrapped = format!(
            "(function() {{\n\
            {parent_prelude}\
            {js_source}\n\
            var __exports__ = {{ \
            lookup: function(name) {{ \
            try {{ var fn = eval(name); if (typeof fn === 'function') return fn; }} catch (_e) {{}} \
            {lookup_fallback} \
            return null; \
            }} \
            }};\n\
            if (typeof init !== 'undefined') __exports__.init = init;\n\
            if (typeof update !== 'undefined') __exports__.update = update;\n\
            {proto_chain}\n\
            return __exports__;\n\
            }})()"
        );
        let path_owned = path.to_string();
        self.context.with(|ctx| -> Result<HostScript, ScriptError> {
            let result: Object = ctx.eval(wrapped).map_err(|e| {
                let msg = format_quickjs_error(&ctx, e);
                ScriptError {
                    backend: BackendKind::JavaScript,
                    script_path: path_owned.clone(),
                    location: None,
                    kind: ScriptErrorKind::Syntax,
                    message: msg,
                    bridge_call: None,
                }
            })?;
            let init: Option<Function> = result.get("init").ok();
            let update: Option<Function> = result.get("update").ok();
            let lookup: Option<Function> = result.get("lookup").ok();
            Ok(HostScript {
                init: init.map(|f| Persistent::save(&ctx, f)),
                update: update.map(|f| Persistent::save(&ctx, f)),
                lookup: lookup.map(|f| Persistent::save(&ctx, f)),
                path: path_owned,
            })
        })
    }

    /// Set the snapshot pointer for the duration of `f` and drain any commands
    /// queued by the closure into `ctx`.
    ///
    /// Hot-path optimization: when the runner has already opened a frame via
    /// `JsAdapter::frame_begin`, this is a thin pass-through. State install
    /// and drain happen once per frame instead of once per entity.
    pub(super) fn with_state<R>(
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

    /// Hot-path frame setup. Counterpart to [`Self::frame_end`]. Installs
    /// the frame-constant state once so per-entity `update`/`init` calls
    /// don't pay 6 borrow-mut field assignments + 5 `Rc::clone`s each.
    pub(super) fn frame_begin(&self, ctx: &mut BridgeContext<'_>) {
        let mut s = self.state.borrow_mut();
        s.snapshot = Some(ctx.read as *const BridgeReadSnapshot);
        s.spawn_counter = Some(ctx.spawn_counter());
        s.sound_counter = Some(ctx.sound_counter());
        s.tween_counter = Some(ctx.tween_counter());
        s.event_listener_counter = Some(ctx.event_listener_counter());
        s.timer_counter = Some(ctx.timer_counter());
        s.pending.clear();
    }

    /// Hot-path frame teardown. Drains pending commands once and clears
    /// installed state pointers so post-frame callbacks re-arm via
    /// [`Self::with_state`].
    pub(super) fn frame_end(&self, ctx: &mut BridgeContext<'_>) {
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

    /// Slice A.JS — install a registry-backed function as a QuickJS global.
    /// Mirrors [`super::super::luau::LuauAdapter::install_dynamic_function`]:
    /// the closure marshals call args per `descriptor.args` into bridge
    /// [`crate::scripting::bridge::value::Value`]s, dispatches into
    /// `descriptor.handler`, then converts the returned `Value` back to JS.
    /// Functions whose descriptor has no `handler` are skipped silently
    /// (they remain metadata-only).
    pub(super) fn install_dynamic_function(
        &self,
        name: &str,
        descriptor: &crate::package::script_api::ScriptFunctionDescriptor,
    ) -> Result<(), ScriptError> {
        let Some(handler) = descriptor.handler.clone() else {
            return Ok(());
        };
        let arg_types: Vec<crate::package::script_api::ScriptValueType> = descriptor
            .args
            .iter()
            .map(|a| a.arg_type.clone())
            .collect();
        self.install_dynamic_function_parts(name, &arg_types, handler)
    }

    /// Variant of [`Self::install_dynamic_function`] taking already-
    /// destructured handler parts. Used by [`super::JsAdapter`] when draining
    /// descriptors stashed before the host engine was lazily constructed
    /// (the bare descriptor is intentionally non-`Clone`).
    pub(super) fn install_dynamic_function_parts(
        &self,
        name: &str,
        arg_types: &[crate::package::script_api::ScriptValueType],
        handler: crate::package::script_api::ScriptHandler,
    ) -> Result<(), ScriptError> {
        let arg_types: Vec<crate::package::script_api::ScriptValueType> = arg_types.to_vec();
        let fn_name_for_err = name.to_string();
        let state = Rc::clone(&self.state);
        let name_owned = name.to_string();
        self.context
            .with(|ctx| -> rquickjs::Result<()> {
                let globals = ctx.globals();
                let f = Function::new(
                    ctx.clone(),
                    MutFn::new(
                        // Delegates to `dispatch_dynamic_handler` so the single
                        // `for<'js>` HRTB lives on the free function. The closure
                        // intentionally leaves arg types unannotated so all three
                        // lifetimes unify under the free fn's `'js`.
                        move |ctx, args| {
                            dispatch_dynamic_handler(
                                ctx,
                                args,
                                &arg_types,
                                &handler,
                                &state,
                                &fn_name_for_err,
                            )
                        },
                    ),
                )?;
                globals.set(name_owned.as_str(), f)?;
                Ok(())
            })
            .map_err(|e| make_err(None, ScriptErrorKind::Runtime, e))
    }
}

/// Free helper invoked by the [`HostJs::install_dynamic_function_parts`]
/// closure. Pulling this out gives the `'js` lifetime a single binding on the
/// function signature so closure-inference doesn't end up with separate
/// lifetimes for the `Ctx` argument and the returned `Value`.
fn dispatch_dynamic_handler<'js>(
    ctx: Ctx<'js>,
    args: Rest<rquickjs::Value<'js>>,
    arg_types: &[crate::package::script_api::ScriptValueType],
    handler: &crate::package::script_api::ScriptHandler,
    state: &Rc<RefCell<HostBridgeState>>,
    fn_name_for_err: &str,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    // 1. Marshal JS args -> bridge Values per declared types.
    let mut values: Vec<crate::scripting::bridge::value::Value> =
        Vec::with_capacity(arg_types.len());
    let mut iter = args.0.into_iter();
    for ty in arg_types {
        let jsv = iter.next();
        let v = js_value_to_bridge(ty, jsv.as_ref()).map_err(|e| {
            rquickjs::Error::new_from_js_message(
                "value",
                "bridge",
                format!("{}: arg conversion failed: {}", fn_name_for_err, e),
            )
        })?;
        values.push(v);
    }
    // 2. Borrow state, dispatch handler, drop borrow before constructing the
    //    JS return (which may allocate via `ctx`).
    let result = {
        let mut s = state.borrow_mut();
        let snapshot = match s.snapshot {
            // SAFETY: snapshot pointer is set by frame_begin / with_state
            // and only borrowed for the duration of this call window.
            Some(p) => unsafe { &*p },
            None => {
                return Err(rquickjs::Error::new_from_js_message(
                    "bridge",
                    "snapshot",
                    format!("{}: bridge snapshot unavailable", fn_name_for_err),
                ));
            }
        };
        let mut call_ctx = crate::package::script_api::ScriptCallCtx {
            snapshot,
            commands: &mut s.pending,
        };
        handler(&mut call_ctx, &values)
    };
    match result {
        Ok(v) => bridge_value_to_js(&ctx, &v),
        Err(e) => Err(rquickjs::Error::new_from_js_message(
            "handler",
            "error",
            format!("{}: {}", fn_name_for_err, e),
        )),
    }
}

/// Convert a single QuickJS argument into the bridge [`Value`] declared by the
/// descriptor type. Missing args become `Nil` so handlers can decide how to
/// react. Numeric and entity-id types accept both integer and float JS values.
fn js_value_to_bridge(
    ty: &crate::package::script_api::ScriptValueType,
    jsv: Option<&rquickjs::Value<'_>>,
) -> Result<crate::scripting::bridge::value::Value, String> {
    use crate::package::script_api::ScriptValueType as T;
    use crate::scripting::bridge::value::Value as V;
    let v = match jsv {
        Some(v) => v,
        None => return Ok(V::Nil),
    };
    match ty {
        T::Nil => Ok(V::Nil),
        // `Any` takes whatever the script passed: the descriptor cannot know
        // the type because it depends on a sibling argument (set_ui_prop's
        // property name picks the field, and so the field's type).
        T::Any => {
            if let Some(b) = v.as_bool() {
                Ok(V::Bool(b))
            } else if let Some(n) = v.as_number().or_else(|| v.as_int().map(|i| i as f64)) {
                Ok(V::Number(n))
            } else if let Some(s) = v.as_string() {
                Ok(V::String(
                    s.to_string()
                        .map_err(|e| format!("string conversion failed: {}", e))?,
                ))
            } else {
                Ok(V::Nil)
            }
        }
        T::Bool => Ok(V::Bool(
            v.as_bool()
                .or_else(|| v.as_int().map(|i| i != 0))
                .or_else(|| v.as_number().map(|n| n != 0.0))
                .unwrap_or(false),
        )),
        T::Int | T::Float => {
            let n = v
                .as_number()
                .or_else(|| v.as_int().map(|i| i as f64))
                .or_else(|| v.as_bool().map(|b| if b { 1.0 } else { 0.0 }))
                .ok_or_else(|| format!("expected number, got {:?}", v.type_of()))?;
            Ok(V::Number(n))
        }
        T::String => {
            if let Some(s) = v.as_string() {
                let s = s
                    .to_string()
                    .map_err(|e| format!("string conversion failed: {}", e))?;
                Ok(V::String(s))
            } else {
                Err(format!("expected string, got {:?}", v.type_of()))
            }
        }
        T::EntityId => {
            let n = v
                .as_number()
                .or_else(|| v.as_int().map(|i| i as f64))
                .ok_or_else(|| format!("expected entity id, got {:?}", v.type_of()))?;
            Ok(V::EntityId(n as u64))
        }
        T::Vec2 => {
            if let Some(obj) = v.as_object() {
                let x: f64 = obj.get("x").or_else(|_| obj.get(0)).unwrap_or(0.0);
                let y: f64 = obj.get("y").or_else(|_| obj.get(1)).unwrap_or(0.0);
                Ok(V::Vec2(x as f32, y as f32))
            } else {
                Err(format!("expected Vec2 object/array, got {:?}", v.type_of()))
            }
        }
        // Only ever a return type (`get_sprite_color`); no function takes one.
        T::Rgba => Err("rgba is a return type only".to_string()),
        T::VarArgs => {
            // Best-effort coercion mirroring lua_to_value's VarArgs arm.
            Ok(if v.is_null() || v.is_undefined() {
                V::Nil
            } else if let Some(b) = v.as_bool() {
                V::Bool(b)
            } else if let Some(n) = v.as_number() {
                V::Number(n)
            } else if let Some(i) = v.as_int() {
                V::Number(i as f64)
            } else if let Some(s) = v.as_string() {
                V::String(
                    s.to_string()
                        .map_err(|e| format!("string conversion failed: {}", e))?,
                )
            } else {
                V::Nil
            })
        }
    }
}

/// Convert a bridge [`Value`] back into a QuickJS value for return marshalling.
/// `Value::Nil` becomes `undefined`. Strings allocate a new JS string in `ctx`.
/// Vec2 / Color become 2- and 4-element JS arrays. Containers (Array / Map /
/// Handle) currently flatten to `undefined` — same scope as
/// [`super::super::luau::value_to_lua`] today; Slice A.JS handlers (auto-
/// accessors) all return scalars or Nil.
fn bridge_value_to_js<'js>(
    ctx: &Ctx<'js>,
    v: &crate::scripting::bridge::value::Value,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    use crate::scripting::bridge::value::Value as V;
    use rquickjs::IntoJs;
    match v {
        V::Nil => Ok(rquickjs::Value::new_undefined(ctx.clone())),
        V::Bool(b) => Ok(rquickjs::Value::new_bool(ctx.clone(), *b)),
        V::Number(n) => Ok(rquickjs::Value::new_number(ctx.clone(), *n)),
        V::String(s) => s.as_str().into_js(ctx),
        V::EntityId(id) => Ok(rquickjs::Value::new_number(ctx.clone(), *id as f64)),
        V::Vec2(x, y) => {
            let arr = rquickjs::Array::new(ctx.clone())?;
            arr.set(0, *x)?;
            arr.set(1, *y)?;
            Ok(arr.into_value())
        }
        V::Color(r, g, b, a) => {
            let arr = rquickjs::Array::new(ctx.clone())?;
            arr.set(0, *r)?;
            arr.set(1, *g)?;
            arr.set(2, *b)?;
            arr.set(3, *a)?;
            Ok(arr.into_value())
        }
        V::Handle(_, id) => Ok(rquickjs::Value::new_number(ctx.clone(), *id as f64)),
        V::Array(_) | V::Map(_) => Ok(rquickjs::Value::new_undefined(ctx.clone())),
    }
}

/// Register the bridge global function set on the QuickJS context. Names match
/// `LuauAdapter::register_bridge_globals` exactly so the same Luau source
/// (post-transpile) drives both backends identically.
fn register_globals(context: &Context, state: &Rc<RefCell<HostBridgeState>>) -> Result<(), ScriptError> {
    context
        .with(|ctx| -> rquickjs::Result<()> {
            let globals = ctx.globals();

            // --- Mutating commands ------------------------------------------------
            {
                let s = state.clone();
                globals.set(
                    "set_position",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, x: f64, y: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::SetPosition {
                            entity_id: entity_id as u64,
                            x: x as f32,
                            y: y as f32,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_rotation",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, radians: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::SetRotation {
                            entity_id: entity_id as u64,
                            radians: radians as f32,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_scale",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, x: f64, y: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::SetScale {
                            entity_id: entity_id as u64,
                            x: x as f32,
                            y: y as f32,
                        });
                    }))?,
                )?;
            }
            // Slice 3.5.A — entity-parent attach/detach. `parent_id` of
            // `null`/`undefined` detaches the child so it becomes a root.
            {
                let s = state.clone();
                globals.set(
                    "set_parent",
                    Function::new(ctx.clone(), MutFn::new(move |child_id: f64, parent_id: Option<f64>| {
                        s.borrow_mut().pending.push(BridgeCommand::SetParent {
                            child_id: child_id as u64,
                            parent_id: parent_id.map(|p| p as u64),
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "log",
                    Function::new(ctx.clone(), MutFn::new(move |message: String| {
                        s.borrow_mut().pending.push(BridgeCommand::Log { message });
                    }))?,
                )?;
            }

            // --- Read accessors ---------------------------------------------------
            // Luau returns `(x, y)` tuples; JS returns `[x, y]` arrays. The
            // transpiler renders Lua `local x, y = get_position(e)` as the JS
            // destructuring `var [x, y] = get_position(e)`, so `List<(f32, f32)>`
            // (which becomes a JS array) is the right shape.
            {
                let s = state.clone();
                globals.set(
                    "get_position",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| -> List<(f32, f32)> {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return List((0.0, 0.0)) };
                        let (x, y) = snap.get_position(entity_id as u64).unwrap_or((0.0, 0.0));
                        List((x, y))
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_rotation",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| -> f32 {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return 0.0 };
                        snap.get_rotation(entity_id as u64).unwrap_or(0.0)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_scale",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| -> List<(f32, f32)> {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return List((1.0, 1.0)) };
                        let (x, y) = snap.get_scale(entity_id as u64).unwrap_or((1.0, 1.0));
                        List((x, y))
                    }))?,
                )?;
            }
            // Slice 3.5.A — world-space transform getters (resolved through
            // the Parent chain by `resolve_world_transforms`).
            {
                let s = state.clone();
                globals.set(
                    "get_world_position",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| -> List<(f32, f32)> {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return List((0.0, 0.0)) };
                        let (x, y) = snap.get_world_position(entity_id as u64).unwrap_or((0.0, 0.0));
                        List((x, y))
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_world_rotation",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| -> f32 {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return 0.0 };
                        snap.get_world_rotation(entity_id as u64).unwrap_or(0.0)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_world_scale",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| -> List<(f32, f32)> {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return List((1.0, 1.0)) };
                        let (x, y) = snap.get_world_scale(entity_id as u64).unwrap_or((1.0, 1.0));
                        List((x, y))
                    }))?,
                )?;
            }
            // Slice 3.5.B — skeleton lookup. Returns `null` when the bone
            // / skeleton is missing.
            {
                let s = state.clone();
                globals.set(
                    "find_bone",
                    Function::new(ctx.clone(), MutFn::new(move |root_id: f64, bone_name: String| -> Option<f64> {
                        let st = s.borrow();
                        let snap = st.snapshot()?;
                        snap.find_bone(root_id as u64, &bone_name).map(|id| id as f64)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "find_skeleton",
                    Function::new(ctx.clone(), MutFn::new(move |skeleton_name: String| -> Option<f64> {
                        let st = s.borrow();
                        let snap = st.snapshot()?;
                        snap.find_skeleton(&skeleton_name).map(|id| id as f64)
                    }))?,
                )?;
            }
            // Slice 3.5.C — `set_animation_time`; play / stop / speed are
            // bound once, below, for sprite and skeletal animation alike.
            {
                let s = state.clone();
                globals.set(
                    "set_animation_time",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, t: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::SetSkeletalPlayer {
                            entity_id: entity_id as u64,
                            clip: None,
                            time: Some(t as f32),
                            speed: None,
                            looping: None,
                            playing: None,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_animation_time",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| -> Option<f64> {
                        let st = s.borrow();
                        let snap = st.snapshot()?;
                        snap.get_animation_time(entity_id as u64).map(|t| t as f64)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_name",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| -> Option<String> {
                        let st = s.borrow();
                        let snap = st.snapshot()?;
                        snap.get_name(entity_id as u64)
                    }))?,
                )?;
            }

            // --- Phase 5.1: entity spawn / destroy / find / tags --------------
            {
                let s = state.clone();
                globals.set(
                    "spawn_entity",
                    Function::new(ctx.clone(), MutFn::new(move |name: String| -> f64 {
                        let mut st = s.borrow_mut();
                        let id = st.alloc_spawn_id().unwrap_or(0);
                        st.pending.push(BridgeCommand::SpawnEntity { name, prefab: None, spawn_id: id });
                        id as f64
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "spawn_prefab",
                    Function::new(ctx.clone(), MutFn::new(move |prefab: String| -> f64 {
                        let mut st = s.borrow_mut();
                        let id = st.alloc_spawn_id().unwrap_or(0);
                        st.pending.push(BridgeCommand::SpawnEntity {
                            name: prefab.clone(),
                            prefab: Some(prefab),
                            spawn_id: id,
                        });
                        id as f64
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "spawn_rig",
                    Function::new(ctx.clone(), MutFn::new(move |rig_id: String| -> f64 {
                        let mut st = s.borrow_mut();
                        let id = st.alloc_spawn_id().unwrap_or(0);
                        st.pending.push(BridgeCommand::SpawnRig { rig_id, spawn_id: id });
                        id as f64
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_ik_target",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, x: f64, y: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::SetIkTarget {
                            entity_id: entity_id as u64,
                            x: x as f32,
                            y: y as f32,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "crossfade_animation",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, clip: String, seconds: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::CrossfadeAnimation {
                            entity_id: entity_id as u64,
                            clip,
                            seconds: seconds as f32,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_animation_blend",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, clip: String, weight: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::SetAnimationBlend {
                            entity_id: entity_id as u64,
                            clip,
                            weight: weight as f32,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "animation_play_layer",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, layer: String, clip: String, seconds: Opt<f64>, looping: Opt<bool>| {
                        s.borrow_mut().pending.push(BridgeCommand::AnimationLayer {
                            entity_id: entity_id as u64,
                            op: "play".into(),
                            layer,
                            clip,
                            value: 1.0,
                            seconds: seconds.0.unwrap_or(0.2) as f32,
                            looping: looping.0.unwrap_or(false),
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "animation_stop_layer",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, layer: String, seconds: Opt<f64>| {
                        s.borrow_mut().pending.push(BridgeCommand::AnimationLayer {
                            entity_id: entity_id as u64,
                            op: "stop".into(),
                            layer,
                            clip: String::new(),
                            value: 0.0,
                            seconds: seconds.0.unwrap_or(0.2) as f32,
                            looping: false,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "animation_set_layer_weight",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, layer: String, weight: f64, seconds: Opt<f64>| {
                        s.borrow_mut().pending.push(BridgeCommand::AnimationLayer {
                            entity_id: entity_id as u64,
                            op: "weight".into(),
                            layer,
                            clip: String::new(),
                            value: weight as f32,
                            seconds: seconds.0.unwrap_or(0.0) as f32,
                            looping: false,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_animation_blend_position",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, x: f64, y: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::SetAnimationBlendPosition {
                            entity_id: entity_id as u64,
                            x: x as f32,
                            y: y as f32,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_conveyor_speed",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, speed: f64| {
                        s.borrow_mut()
                            .pending
                            .push(BridgeCommand::SetConveyorSpeed { entity_id: entity_id as u64, speed: speed as f32 });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_ik_weight",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, weight: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::SetIkWeight {
                            entity_id: entity_id as u64,
                            weight: weight as f32,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "destroy_entity",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::DestroyEntity { entity_id: entity_id as u64 });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "add_tag",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, tag: String| {
                        s.borrow_mut().pending.push(BridgeCommand::AddTag { entity_id: entity_id as u64, tag });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "remove_tag",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, tag: String| {
                        s.borrow_mut().pending.push(BridgeCommand::RemoveTag { entity_id: entity_id as u64, tag });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "has_tag",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, tag: String| -> bool {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return false };
                        snap.has_tag(entity_id as u64, &tag)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "find_by_name",
                    Function::new(ctx.clone(), MutFn::new(move |name: String| -> Option<f64> {
                        let st = s.borrow();
                        let snap = st.snapshot()?;
                        snap.entity_by_name(&name).map(|id| id as f64)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "find_by_tag",
                    Function::new(ctx.clone(), MutFn::new(move |tag: String| -> Vec<f64> {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return Vec::new() };
                        let mut ids: Vec<u64> = snap
                            .tags
                            .iter()
                            .filter_map(|(id, t)| if t.contains(&tag) { Some(*id) } else { None })
                            .collect();
                        ids.sort_unstable();
                        ids.into_iter().map(|id| id as f64).collect()
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "find_in_radius",
                    Function::new(ctx.clone(), MutFn::new(move |cx: f32, cy: f32, radius: f32, tag: Option<String>| -> Vec<f64> {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return Vec::new() };
                        let r2 = radius * radius;
                        let mut ids: Vec<u64> = Vec::new();
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
                        ids.sort_unstable();
                        ids.into_iter().map(|id| id as f64).collect()
                    }))?,
                )?;
            }

            // --- Phase 5.2: sprite + material ---------------------------------
            {
                let s = state.clone();
                globals.set(
                    "set_sprite_color",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, r: f32, g: f32, b: f32, a: f32| {
                            s.borrow_mut().pending.push(BridgeCommand::SetSpriteColor {
                                entity_id: entity_id as u64,
                                r, g, b, a,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_z_order",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, z: f64| {
                            s.borrow_mut().pending.push(BridgeCommand::SetZOrder {
                                entity_id: entity_id as u64,
                                z: z as i32,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_text_color",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, r: f64, g: f64, b: f64, a: Opt<f64>| {
                            s.borrow_mut().pending.push(BridgeCommand::SetTextColor {
                                entity_id: entity_id as u64,
                                r: r as f32,
                                g: g as f32,
                                b: b as f32,
                                a: a.0.unwrap_or(1.0) as f32,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_trail_emitting",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, on: bool| {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::SetTrailEmitting { entity_id: entity_id as u64, on });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "clear_trail",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64| {
                            s.borrow_mut().pending.push(BridgeCommand::ClearTrail { entity_id: entity_id as u64 });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "carve_sprite",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, x: f64, y: f64, radius: f64| {
                            s.borrow_mut().pending.push(BridgeCommand::CarveSprite {
                                entity_id: entity_id as u64,
                                x: x as f32,
                                y: y as f32,
                                radius: radius as f32,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "paint_sprite",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(
                            move |entity_id: f64,
                                  x: f64,
                                  y: f64,
                                  size: f64,
                                  rest: rquickjs::function::Rest<rquickjs::Value>| {
                                let num = |i: usize, d: f32| rest.0.get(i).and_then(|v| v.as_number()).map_or(d, |n| n as f32);
                                let shape = rest.0.get(4).and_then(|v| v.as_string()).and_then(|s| s.to_string().ok()).unwrap_or_default();
                                s.borrow_mut().pending.push(BridgeCommand::PaintSprite {
                                    entity_id: entity_id as u64,
                                    x: x as f32,
                                    y: y as f32,
                                    size: size as f32,
                                    color: [num(0, 1.0), num(1, 1.0), num(2, 1.0), num(3, 1.0)],
                                    shape,
                                    rotation: num(5, 0.0),
                                    depth: 0.0,
                                    blend: rest.0.get(6).and_then(|v| v.as_string()).and_then(|s| s.to_string().ok()).unwrap_or_default(),
                                });
                            },
                        ),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "press_sprite",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(
                            move |entity_id: f64,
                                  x: f64,
                                  y: f64,
                                  size: f64,
                                  rest: rquickjs::function::Rest<rquickjs::Value>| {
                                let num = |i: usize, d: f32| rest.0.get(i).and_then(|v| v.as_number()).map_or(d, |n| n as f32);
                                let shape = rest.0.get(1).and_then(|v| v.as_string()).and_then(|s| s.to_string().ok()).unwrap_or_else(|| "disc".to_string());
                                s.borrow_mut().pending.push(BridgeCommand::PaintSprite {
                                    entity_id: entity_id as u64,
                                    x: x as f32,
                                    y: y as f32,
                                    size: size as f32,
                                    color: [1.0; 4],
                                    shape,
                                    rotation: num(2, 0.0),
                                    depth: num(0, 1.0),
                                    blend: String::new(),
                                });
                            },
                        ),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_sprite_anchor",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, x: f32, y: f32| {
                            s.borrow_mut().pending.push(BridgeCommand::SetSpriteAnchor {
                                entity_id: entity_id as u64,
                                x, y,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_sprite_size",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, w: f32, h: f32| {
                            s.borrow_mut().pending.push(BridgeCommand::SetSpriteSize {
                                entity_id: entity_id as u64,
                                w, h,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_sprite_region",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, atlas_id: String, region_name: String| {
                            s.borrow_mut().pending.push(BridgeCommand::SetSpriteRegion {
                                entity_id: entity_id as u64,
                                atlas_id,
                                region_name,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_sprite_corners",
                    Function::new(
                        ctx.clone(),
                        // JS form: set_sprite_corners(entity, [x0,y0,x1,y1,x2,y2,x3,y3]).
                        // An array avoids rquickjs's closure-arity limit (9 scalar
                        // params is unsupported); Lua uses 8 positional scalars.
                        MutFn::new(move |entity_id: f64, c: Vec<f32>| {
                            if c.len() >= 8 {
                                s.borrow_mut().pending.push(BridgeCommand::SetSpriteCorners {
                                    entity_id: entity_id as u64,
                                    corners: Some([
                                        [c[0], c[1]], [c[2], c[3]],
                                        [c[4], c[5]], [c[6], c[7]],
                                    ]),
                                });
                            }
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_sprite_color",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64| -> List<(f32, f32, f32, f32)> {
                            let st = s.borrow();
                            let snap = match st.snapshot() {
                                Some(s) => s,
                                None => return List((1.0, 1.0, 1.0, 1.0)),
                            };
                            let c = snap.get_sprite_color(entity_id as u64).unwrap_or([1.0, 1.0, 1.0, 1.0]);
                            List((c[0], c[1], c[2], c[3]))
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_sprite_anchor",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64| -> List<(f32, f32)> {
                            let st = s.borrow();
                            let snap = match st.snapshot() {
                                Some(s) => s,
                                None => return List((0.5, 0.5)),
                            };
                            let a = snap.get_sprite_anchor(entity_id as u64).unwrap_or([0.5, 0.5]);
                            List((a[0], a[1]))
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_material",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, material_id: String| {
                            s.borrow_mut().pending.push(BridgeCommand::SetMaterial {
                                entity_id: entity_id as u64,
                                material_id,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_normal_map",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, normal_map_id: Option<String>| {
                            s.borrow_mut().pending.push(BridgeCommand::SetNormalMap {
                                entity_id: entity_id as u64,
                                normal_map_id,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_material",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64| -> Option<String> {
                            let st = s.borrow();
                            let snap = st.snapshot()?;
                            snap.get_material(entity_id as u64)
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_material_uniform_float",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, name: String, value: f32| {
                            s.borrow_mut().pending.push(BridgeCommand::SetMaterialUniformFloat {
                                entity_id: entity_id as u64,
                                name,
                                value,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_material_uniform_vec4",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(
                            move |entity_id: f64, name: String, x: f32, y: f32, z: f32, w: f32| {
                                s.borrow_mut().pending.push(
                                    BridgeCommand::SetMaterialUniformVec4 {
                                        entity_id: entity_id as u64,
                                        name,
                                        value: [x, y, z, w],
                                    },
                                );
                            },
                        ),
                    )?,
                )?;
            }

            // --- Phase 5.3: input + action bindings ---------------------------
            {
                let s = state.clone();
                globals.set(
                    "is_key_pressed",
                    Function::new(ctx.clone(), MutFn::new(move |key: String| -> bool {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return false };
                        snap.keys_pressed.contains(&key)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "is_key_just_pressed",
                    Function::new(ctx.clone(), MutFn::new(move |key: String| -> bool {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return false };
                        snap.keys_just_pressed.contains(&key)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "is_action_pressed",
                    Function::new(ctx.clone(), MutFn::new(move |action: String| -> bool {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return false };
                        snap.actions_pressed.contains(&action)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "is_action_just_pressed",
                    Function::new(ctx.clone(), MutFn::new(move |action: String| -> bool {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return false };
                        snap.actions_just_pressed.contains(&action)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "is_action_just_released",
                    Function::new(ctx.clone(), MutFn::new(move |action: String| -> bool {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return false };
                        snap.actions_just_released.contains(&action)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_mouse_position",
                    Function::new(ctx.clone(), MutFn::new(move || -> List<(f32, f32)> {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return List((0.0, 0.0)) };
                        List(snap.mouse_position)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_screen_size",
                    Function::new(ctx.clone(), MutFn::new(move || -> List<(f32, f32)> {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return List((0.0, 0.0)) };
                        List(snap.screen_size)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "is_mouse_left_just_pressed",
                    Function::new(ctx.clone(), MutFn::new(move || -> bool {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return false };
                        snap.mouse_left_just_pressed
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "is_mouse_left_pressed",
                    Function::new(ctx.clone(), MutFn::new(move || -> bool {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return false };
                        snap.mouse_left
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "is_mouse_left_just_released",
                    Function::new(ctx.clone(), MutFn::new(move || -> bool {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return false };
                        snap.mouse_left_just_released
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_mouse_wheel",
                    Function::new(ctx.clone(), MutFn::new(move || -> f32 {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return 0.0 };
                        snap.mouse_wheel_delta
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_mouse_delta",
                    Function::new(ctx.clone(), MutFn::new(move || -> List<(f32, f32)> {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return List((0.0, 0.0)) };
                        List(snap.mouse_delta)
                    }))?,
                )?;
            }

            // --- Phase 5.4: physics ------------------------------------------
            {
                let s = state.clone();
                globals.set(
                    "apply_force",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, fx: f32, fy: f32| {
                            s.borrow_mut().pending.push(BridgeCommand::ApplyForce {
                                entity_id: entity_id as u64,
                                fx,
                                fy,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "apply_impulse",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, ix: f32, iy: f32| {
                            s.borrow_mut().pending.push(BridgeCommand::ApplyImpulse {
                                entity_id: entity_id as u64,
                                ix,
                                iy,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_velocity",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, vx: f32, vy: f32| {
                            s.borrow_mut().pending.push(BridgeCommand::SetVelocity {
                                entity_id: entity_id as u64,
                                vx,
                                vy,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_angular_velocity",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, omega: f32| {
                            s.borrow_mut().pending.push(BridgeCommand::SetAngularVelocity {
                                entity_id: entity_id as u64,
                                omega,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_velocity",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64| -> List<(f32, f32)> {
                            let st = s.borrow();
                            let snap = match st.snapshot() {
                                Some(s) => s,
                                None => return List((0.0, 0.0)),
                            };
                            List(snap.get_velocity(entity_id as u64).unwrap_or((0.0, 0.0)))
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_angular_velocity",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| -> f32 {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return 0.0 };
                        snap.get_angular_velocity(entity_id as u64)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "contact_count",
                    Function::new(ctx.clone(), MutFn::new(move || -> f64 {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return 0.0 };
                        snap.contacts.len() as f64
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                // Returns [entity_a, entity_b, started, impulse, x, y]
                // flattened into a JS array; the transpiler renders the Luau
                // call site as `var [a, b, started] = get_contact(i)` so the
                // destructure matches and ignores the rest.
                globals.set(
                    "get_contact",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |index: f64| -> List<(f64, f64, bool, f64, f64, f64)> {
                            let st = s.borrow();
                            let snap = match st.snapshot() {
                                Some(s) => s,
                                None => return List((0.0, 0.0, false, 0.0, 0.0, 0.0)),
                            };
                            let i = (index as usize).saturating_sub(1);
                            match snap.contacts.get(i).copied() {
                                // How hard and where, appended - an existing
                                // caller reading three values is unaffected.
                                Some((a, b, started, impulse, x, y)) => List((
                                    a as f64,
                                    b as f64,
                                    started,
                                    impulse as f64,
                                    x as f64,
                                    y as f64,
                                )),
                                None => List((0.0, 0.0, false, 0.0, 0.0, 0.0)),
                            }
                        }),
                    )?,
                )?;
            }

            // --- Phase 5.5: audio --------------------------------------------
            // play_sound(clip_id, volume?, pitch?) -> sound_handle (f64).
            // Returns the pre-allocated handle so scripts get a synchronous
            // value to pass into stop_sound / set_sound_volume.
            {
                let s = state.clone();
                globals.set(
                    "play_sound",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |clip_id: String, volume: Opt<f32>, pitch: Opt<f32>, pan: Opt<f32>| -> f64 {
                            let mut st = s.borrow_mut();
                            let sound_id = st.alloc_sound_id().unwrap_or(0);
                            st.pending.push(BridgeCommand::PlaySound {
                                clip_id,
                                volume: volume.0.unwrap_or(1.0),
                                pitch: pitch.0.unwrap_or(1.0),
                                pan: pan.0.unwrap_or(0.0).clamp(-1.0, 1.0),
                                sound_id,
                                looping: false,
                            });
                            sound_id as f64
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "play_music",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |clip_id: String, volume: Opt<f32>, looping: Opt<bool>| {
                            s.borrow_mut().pending.push(BridgeCommand::PlayMusic {
                                clip_id,
                                volume: volume.0.unwrap_or(1.0),
                                looping: looping.0.unwrap_or(true),
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "stop_sound",
                    Function::new(ctx.clone(), MutFn::new(move |handle_id: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::StopSound {
                            handle_id: handle_id as u64,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "stop_music",
                    Function::new(ctx.clone(), MutFn::new(move || {
                        s.borrow_mut().pending.push(BridgeCommand::StopMusic);
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "pause_music",
                    Function::new(ctx.clone(), MutFn::new(move || {
                        s.borrow_mut().pending.push(BridgeCommand::PauseMusic);
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "resume_music",
                    Function::new(ctx.clone(), MutFn::new(move || {
                        s.borrow_mut().pending.push(BridgeCommand::ResumeMusic);
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "stop_all_sounds",
                    Function::new(ctx.clone(), MutFn::new(move || {
                        s.borrow_mut().pending.push(BridgeCommand::StopAllSounds);
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "pause_all_sounds",
                    Function::new(ctx.clone(), MutFn::new(move || {
                        s.borrow_mut().pending.push(BridgeCommand::PauseAllSounds);
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "resume_all_sounds",
                    Function::new(ctx.clone(), MutFn::new(move || {
                        s.borrow_mut().pending.push(BridgeCommand::ResumeAllSounds);
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_sound_volume",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |handle_id: f64, volume: f32| {
                            s.borrow_mut().pending.push(BridgeCommand::SetSoundVolume {
                                handle_id: handle_id as u64,
                                volume,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_music_volume",
                    Function::new(ctx.clone(), MutFn::new(move |volume: f32| {
                        s.borrow_mut().pending.push(BridgeCommand::SetMusicVolume { volume });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_master_volume",
                    Function::new(ctx.clone(), MutFn::new(move |volume: f32| {
                        s.borrow_mut().pending.push(BridgeCommand::SetMasterVolume { volume });
                    }))?,
                )?;
            }

            // --- Phase 5.6: animation ----------------------------------------
            {
                let s = state.clone();
                globals.set(
                    "play_animation",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, clip_name: String| {
                            s.borrow_mut().pending.push(BridgeCommand::PlayAnimation {
                                entity_id: entity_id as u64,
                                clip_name,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "stop_animation",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| {
                        s.borrow_mut()
                            .pending
                            .push(BridgeCommand::StopAnimation { entity_id: entity_id as u64 });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_animation_speed",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, speed: f32| {
                            s.borrow_mut().pending.push(BridgeCommand::SetAnimationSpeed {
                                entity_id: entity_id as u64,
                                speed,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "is_animation_playing",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| -> bool {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return false };
                        snap.is_animation_playing(entity_id as u64)
                    }))?,
                )?;
            }
            {
                // "" means no clip is active \u2014 same sentinel as luau side.
                let s = state.clone();
                globals.set(
                    "get_current_animation",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| -> String {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return String::new() };
                        snap.get_current_animation(entity_id as u64)
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_anim_bool",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, param: String, value: bool| {
                            s.borrow_mut().pending.push(BridgeCommand::SetAnimBool {
                                entity_id: entity_id as u64,
                                param,
                                value,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_anim_float",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, param: String, value: f32| {
                            s.borrow_mut().pending.push(BridgeCommand::SetAnimFloat {
                                entity_id: entity_id as u64,
                                param,
                                value,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_anim_trigger",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, param: String| {
                            s.borrow_mut().pending.push(BridgeCommand::SetAnimTrigger {
                                entity_id: entity_id as u64,
                                param,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "start_state_machine",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, state_name: String| {
                            s.borrow_mut().pending.push(BridgeCommand::StartStateMachine {
                                entity_id: entity_id as u64,
                                state_name,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_anim_state",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| -> String {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return String::new() };
                        snap.get_anim_state(entity_id as u64)
                    }))?,
                )?;
            }
            // tween_position(entity, x, y, dur, easing?) -> tween_id (f64).
            // end_value packs (x, y, 0, 0) so the legacy `[f32; 4]` shape is
            // preserved when translated.
            {
                let s = state.clone();
                globals.set(
                    "tween_position",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(
                            move |entity_id: f64, x: f32, y: f32, duration: f32, easing: Opt<String>| -> f64 {
                                let mut st = s.borrow_mut();
                                let tween_id = st.alloc_tween_id().unwrap_or(0);
                                st.pending.push(BridgeCommand::StartTween {
                                    entity_id: entity_id as u64,
                                    target: "position".into(),
                                    end_value: [x, y, 0.0, 0.0],
                                    duration,
                                    easing: easing.0.unwrap_or_else(|| "linear".into()),
                                    tween_id,
                                });
                                tween_id as f64
                            },
                        ),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "tween_rotation",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(
                            move |entity_id: f64, radians: f32, duration: f32, easing: Opt<String>| -> f64 {
                                let mut st = s.borrow_mut();
                                let tween_id = st.alloc_tween_id().unwrap_or(0);
                                st.pending.push(BridgeCommand::StartTween {
                                    entity_id: entity_id as u64,
                                    target: "rotation".into(),
                                    end_value: [radians, 0.0, 0.0, 0.0],
                                    duration,
                                    easing: easing.0.unwrap_or_else(|| "linear".into()),
                                    tween_id,
                                });
                                tween_id as f64
                            },
                        ),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "tween_scale",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(
                            move |entity_id: f64, x: f32, y: f32, duration: f32, easing: Opt<String>| -> f64 {
                                let mut st = s.borrow_mut();
                                let tween_id = st.alloc_tween_id().unwrap_or(0);
                                st.pending.push(BridgeCommand::StartTween {
                                    entity_id: entity_id as u64,
                                    target: "scale".into(),
                                    end_value: [x, y, 0.0, 0.0],
                                    duration,
                                    easing: easing.0.unwrap_or_else(|| "linear".into()),
                                    tween_id,
                                });
                                tween_id as f64
                            },
                        ),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "tween_color",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(
                            move |entity_id: f64, r: f32, g: f32, b: f32, a: f32, duration: f32, easing: Opt<String>| -> f64 {
                                let mut st = s.borrow_mut();
                                let tween_id = st.alloc_tween_id().unwrap_or(0);
                                st.pending.push(BridgeCommand::StartTween {
                                    entity_id: entity_id as u64,
                                    target: "sprite_color".into(),
                                    end_value: [r, g, b, a],
                                    duration,
                                    easing: easing.0.unwrap_or_else(|| "linear".into()),
                                    tween_id,
                                });
                                tween_id as f64
                            },
                        ),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "cancel_tween",
                    Function::new(ctx.clone(), MutFn::new(move |tween_id: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::CancelTween {
                            tween_id: tween_id as u32,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "cancel_all_tweens",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64| {
                        s.borrow_mut()
                            .pending
                            .push(BridgeCommand::CancelAllTweens { entity_id: entity_id as u64 });
                    }))?,
                )?;
            }

            // --- Phase 5.7: scene --------------------------------------------
            {
                let s = state.clone();
                globals.set(
                    "load_scene",
                    Function::new(ctx.clone(), MutFn::new(move |path: String| {
                        s.borrow_mut().pending.push(BridgeCommand::LoadScene { path });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "push_scene",
                    Function::new(ctx.clone(), MutFn::new(move |path: String| {
                        s.borrow_mut().pending.push(BridgeCommand::PushScene { path });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "pop_scene",
                    Function::new(ctx.clone(), MutFn::new(move || {
                        s.borrow_mut().pending.push(BridgeCommand::PopScene);
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "quit_game",
                    Function::new(ctx.clone(), MutFn::new(move || {
                        s.borrow_mut().pending.push(BridgeCommand::QuitGame);
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_time_scale",
                    Function::new(ctx.clone(), MutFn::new(move |scale: f64| {
                        s.borrow_mut()
                            .pending
                            .push(BridgeCommand::SetTimeScale { scale: scale as f32 });
                    }))?,
                )?;
            }
            // Slice 12.A — fade transition overlay. Called as
            // `fade_in(dur)` or `fade_in(dur, r, g, b)` from JS.
            {
                let s = state.clone();
                globals.set(
                    "fade_in",
                    Function::new(ctx.clone(), MutFn::new(
                        move |duration_s: f32, r: rquickjs::Value, g: rquickjs::Value, b: rquickjs::Value| {
                            let color = [
                                r.as_number().map(|v| v as f32).unwrap_or(0.0),
                                g.as_number().map(|v| v as f32).unwrap_or(0.0),
                                b.as_number().map(|v| v as f32).unwrap_or(0.0),
                            ];
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::FadeIn { duration_s, color });
                        },
                    ))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "fade_out",
                    Function::new(ctx.clone(), MutFn::new(
                        move |duration_s: f32, r: rquickjs::Value, g: rquickjs::Value, b: rquickjs::Value| {
                            let color = [
                                r.as_number().map(|v| v as f32).unwrap_or(0.0),
                                g.as_number().map(|v| v as f32).unwrap_or(0.0),
                                b.as_number().map(|v| v as f32).unwrap_or(0.0),
                            ];
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::FadeOut { duration_s, color });
                        },
                    ))?,
                )?;
            }
            // P1.3 — screen flash. `screen_flash(dur)` defaults to opaque white;
            // up to 4 channels may be supplied to control tint + initial alpha.
            {
                let s = state.clone();
                globals.set(
                    "screen_flash",
                    Function::new(ctx.clone(), MutFn::new(
                        move |duration_s: f32,
                              r: rquickjs::Value,
                              g: rquickjs::Value,
                              b: rquickjs::Value,
                              a: rquickjs::Value| {
                            let color = [
                                r.as_number().map(|v| v as f32).unwrap_or(1.0),
                                g.as_number().map(|v| v as f32).unwrap_or(1.0),
                                b.as_number().map(|v| v as f32).unwrap_or(1.0),
                                a.as_number().map(|v| v as f32).unwrap_or(1.0),
                            ];
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::ScreenFlash { duration_s, color });
                        },
                    ))?,
                )?;
            }
            {
                globals.set(
                    "get_current_scene_name",
                    Function::new(ctx.clone(), MutFn::new(crate::scene::current_name))?,
                )?;
            }

            // --- Phase 5.8: events & timers ---------------------------------
            {
                let s = state.clone();
                globals.set(
                    "send_event",
                    Function::new(ctx.clone(), MutFn::new(move |name: String, sender: f64, data: Opt<String>| {
                        s.borrow_mut().pending.push(BridgeCommand::SendEvent {
                            name,
                            sender: sender as u64,
                            data: data.0,
                        });
                    }))?,
                )?;
            }
            // Phase C: targeted send_event_to(name, sender, target, data?)
            {
                let s = state.clone();
                globals.set(
                    "send_event_to",
                    Function::new(ctx.clone(), MutFn::new(move |name: String, sender: f64, target: f64, data: Opt<String>| {
                        s.borrow_mut().pending.push(BridgeCommand::SendEventTo {
                            name,
                            sender: sender as u64,
                            target: target as u64,
                            data: data.0,
                        });
                    }))?,
                )?;
            }
            // Phase C: call(caller, target, method, ...args). Args restricted
            // to nil/bool/number/string for v1 — tables/arrays unsupported.
            {
                let s = state.clone();
                globals.set(
                    "call",
                    Function::new(ctx.clone(), MutFn::new(
                        move |caller: f64, target: f64, method: String, args: rquickjs::function::Rest<rquickjs::Value>| -> rquickjs::Result<()> {
                            use crate::scripting::bridge::value::Value as V;
                            let mut bridge_args: Vec<V> = Vec::with_capacity(args.0.len());
                            for a in args.0.iter() {
                                let v = if a.is_null() || a.is_undefined() {
                                    V::Nil
                                } else if let Some(b) = a.as_bool() {
                                    V::Bool(b)
                                } else if let Some(n) = a.as_number() {
                                    V::Number(n)
                                } else if let Some(i) = a.as_int() {
                                    V::Number(i as f64)
                                } else if let Some(s) = a.as_string() {
                                    V::String(s.to_string()?)
                                } else {
                                    return Err(rquickjs::Error::new_from_js_message(
                                        "value", "primitive",
                                        "call(): unsupported arg type (only nil/bool/number/string in v1)",
                                    ));
                                };
                                bridge_args.push(v);
                            }
                            s.borrow_mut().pending.push(BridgeCommand::CallScriptMethod {
                                caller: caller as u64,
                                target: target as u64,
                                method,
                                args: bridge_args,
                            });
                            Ok(())
                        },
                    ))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "on_event",
                    Function::new(ctx.clone(), MutFn::new(move |event_name: String, entity_id: f64| -> f64 {
                        let mut st = s.borrow_mut();
                        let listener_id = st.alloc_event_listener_id().unwrap_or(0);
                        st.pending.push(BridgeCommand::RegisterEventListener {
                            event_name,
                            entity_id: entity_id as u64,
                            listener_id,
                        });
                        listener_id as f64
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "remove_event_listener",
                    Function::new(ctx.clone(), MutFn::new(move |event_name: String, entity_id: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::RemoveEventListener {
                            event_name,
                            entity_id: entity_id as u64,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_timer",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, delay: f32, callback_name: String| -> f64 {
                        let mut st = s.borrow_mut();
                        let timer_id = st.alloc_timer_id().unwrap_or(0);
                        st.pending.push(BridgeCommand::SetTimer {
                            entity_id: entity_id as u64,
                            delay,
                            callback_name,
                            repeating: false,
                            timer_id,
                        });
                        timer_id as f64
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_interval",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, interval: f32, callback_name: String| -> f64 {
                        let mut st = s.borrow_mut();
                        let timer_id = st.alloc_timer_id().unwrap_or(0);
                        st.pending.push(BridgeCommand::SetTimer {
                            entity_id: entity_id as u64,
                            delay: interval,
                            callback_name,
                            repeating: true,
                            timer_id,
                        });
                        timer_id as f64
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "cancel_timer",
                    Function::new(ctx.clone(), MutFn::new(move |timer_id: f64| {
                        s.borrow_mut()
                            .pending
                            .push(BridgeCommand::CancelTimer { timer_id: timer_id as u32 });
                    }))?,
                )?;
            }

            // --- Phase 5.9: UI elements ------------------------------------
            {
                let s = state.clone();
                globals.set(
                    "create_ui_text",
                    Function::new(ctx.clone(), MutFn::new(move |
                        text: String,
                        anchor: String,
                        offset_x: f32,
                        offset_y: f32,
                        rest: Rest<rquickjs::Value>,
                    | -> f64 {
                        // Trailing args (all optional) in order:
                        //   r, g, b, a, font_size, alignment
                        // Missing color components default to white, font_size
                        // to 16.0, alignment to "left" — matching the Luau
                        // adapter's `Option` defaults and `UIText::default`.
                        let r_at = |i: usize| -> Option<f32> {
                            rest.0.get(i).and_then(|v| v.as_number().map(|n| n as f32))
                        };
                        let color = [
                            r_at(0).unwrap_or(1.0),
                            r_at(1).unwrap_or(1.0),
                            r_at(2).unwrap_or(1.0),
                            r_at(3).unwrap_or(1.0),
                        ];
                        let font_size = r_at(4).unwrap_or(16.0);
                        let alignment = rest
                            .0
                            .get(5)
                            .and_then(|v| v.as_string().and_then(|s| s.to_string().ok()))
                            .unwrap_or_else(|| "left".to_string());
                        let mut st = s.borrow_mut();
                        let entity_id = st.alloc_spawn_id().unwrap_or(0);
                        st.pending.push(BridgeCommand::CreateUIText {
                            text,
                            anchor,
                            offset_x,
                            offset_y,
                            color,
                            font_size,
                            alignment,
                            entity_id,
                        });
                        entity_id as f64
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "create_ui_panel",
                    Function::new(ctx.clone(), MutFn::new(move |
                        anchor: String,
                        offset_x: f32,
                        offset_y: f32,
                        width: f32,
                        height: f32,
                        rest: Rest<f32>,
                    | -> f64 {
                        // rquickjs caps `MutFn` IntoJsFunc impls at 8 args, so the
                        // trailing RGBA arrives as `Rest<f32>` and is unpacked here.
                        // Missing components default to 0.0 (alpha to 1.0) for parity
                        // with the Luau adapter, which always receives 9 explicit args.
                        let bg_color = [
                            rest.0.first().copied().unwrap_or(0.0),
                            rest.0.get(1).copied().unwrap_or(0.0),
                            rest.0.get(2).copied().unwrap_or(0.0),
                            rest.0.get(3).copied().unwrap_or(1.0),
                        ];
                        let mut st = s.borrow_mut();
                        let entity_id = st.alloc_spawn_id().unwrap_or(0);
                        st.pending.push(BridgeCommand::CreateUIPanel {
                            anchor,
                            offset_x,
                            offset_y,
                            width,
                            height,
                            bg_color,
                            entity_id,
                        });
                        entity_id as f64
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_ui_text",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, text: String| {
                        s.borrow_mut().pending.push(BridgeCommand::SetUIText {
                            entity_id: entity_id as u64,
                            text,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_text",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, text: String| {
                        s.borrow_mut().pending.push(BridgeCommand::SetText {
                            entity_id: entity_id as u64,
                            text,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_ui_visible",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, visible: bool| {
                        s.borrow_mut().pending.push(BridgeCommand::SetUIVisible {
                            entity_id: entity_id as u64,
                            visible,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_ui_position",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, anchor: String, offset_x: f32, offset_y: f32| {
                        s.borrow_mut().pending.push(BridgeCommand::SetUIPosition {
                            entity_id: entity_id as u64,
                            anchor,
                            offset_x,
                            offset_y,
                        });
                    }))?,
                )?;
            }

            // --- Phase 5.9: GUI document API -------------------------------
            {
                let s = state.clone();
                globals.set(
                    "gui_show",
                    Function::new(ctx.clone(), MutFn::new(move |gui_id: String| {
                        s.borrow_mut().pending.push(BridgeCommand::GuiShow { gui_id });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "gui_hide",
                    Function::new(ctx.clone(), MutFn::new(move |gui_id: String| {
                        s.borrow_mut().pending.push(BridgeCommand::GuiHide { gui_id });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "gui_set",
                    Function::new(ctx.clone(), MutFn::new(move |gui_id: String, node_id: String, property: String, value: String| {
                        s.borrow_mut().pending.push(BridgeCommand::GuiSetNodeProp {
                            gui_id,
                            node_id,
                            property,
                            value,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "gui_get",
                    Function::new(ctx.clone(), MutFn::new(move |gui_id: String, node_id: String, property: String| -> String {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return String::new() };
                        snap.gui_prop(&gui_id, &node_id, &property).unwrap_or_default()
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "gui_on_click",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, gui_id: String, node_id: String, callback_name: String| {
                        s.borrow_mut().pending.push(BridgeCommand::GuiOnClick {
                            entity_id: entity_id as u64,
                            gui_id,
                            node_id,
                            callback_name,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "gui_focus",
                    Function::new(ctx.clone(), MutFn::new(move |gui_id: String, node_id: String| {
                        s.borrow_mut()
                            .pending
                            .push(BridgeCommand::GuiFocus { gui_id, node_id });
                    }))?,
                )?;
            }

            // --- Phase 5.10: particles -------------------------------------
            {
                let s = state.clone();
                globals.set(
                    "emit_burst",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, count: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::EmitBurst {
                            entity_id: entity_id as u64,
                            count: count as u32,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_emitter_active",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, active: bool| {
                        s.borrow_mut().pending.push(BridgeCommand::SetEmitterActive {
                            entity_id: entity_id as u64,
                            active,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_emitter_rate",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, rate: f32| {
                        s.borrow_mut().pending.push(BridgeCommand::SetEmitterRate {
                            entity_id: entity_id as u64,
                            rate,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_emitter_field",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |entity_id: f64, field: String, value: rquickjs::Value<'_>| {
                            let value = if let Some(b) = value.as_bool() {
                                serde_json::Value::Bool(b)
                            } else if let Some(n) = value.as_number().or_else(|| value.as_int().map(|i| i as f64)) {
                                serde_json::Value::from(n)
                            } else if let Some(a) = value.as_array() {
                                serde_json::Value::Array(
                                    a.iter::<f64>().filter_map(|v| v.ok()).map(serde_json::Value::from).collect(),
                                )
                            } else if let Some(t) = value.as_string().and_then(|s| s.to_string().ok()) {
                                serde_json::Value::String(t)
                            } else {
                                serde_json::Value::Null
                            };
                            s.borrow_mut().pending.push(BridgeCommand::SetEmitterField {
                                entity_id: entity_id as u64,
                                field,
                                value,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_particle_count",
                    Function::new(ctx.clone(), MutFn::new(move || -> f64 {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return 0.0 };
                        snap.particle_count as f64
                    }))?,
                )?;
            }

            // --- Phase 5.11: tilemap ---------------------------------------
            {
                let s = state.clone();
                globals.set(
                    "get_tile",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, layer_name: String, x: i32, y: i32| -> f64 {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return -1.0 };
                        snap.tilemap_get_tile(entity_id as u64, &layer_name, x, y) as f64
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_tile",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, layer_name: String, x: f64, y: f64, tile_id: Option<f64>| {
                        let tile_id = crate::tilemap::script_tile_id(tile_id);
                        s.borrow_mut().pending.push(BridgeCommand::SetTile {
                            entity_id: entity_id as u64,
                            layer_name,
                            x: x as u32,
                            y: y as u32,
                            tile_id,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "get_tile_at_world",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, layer_name: String, wx: f32, wy: f32| -> f64 {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return -1.0 };
                        snap.tilemap_get_tile_at_world(entity_id as u64, &layer_name, wx, wy) as f64
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "world_to_tile",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, wx: f32, wy: f32| -> List<(i32, i32)> {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return List((0, 0)) };
                        List(snap.tilemap_world_to_tile(entity_id as u64, wx, wy).unwrap_or((0, 0)))
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "tile_to_world",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, tx: i32, ty: i32| -> List<(f32, f32)> {
                        let st = s.borrow();
                        let snap = match st.snapshot() { Some(s) => s, None => return List((0.0, 0.0)) };
                        List(snap.tilemap_tile_to_world(entity_id as u64, tx, ty).unwrap_or((0.0, 0.0)))
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "tilemap_neighbors",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, tx: i32, ty: i32| -> Vec<List<(i32, i32)>> {
                        let st = s.borrow();
                        let Some(snap) = st.snapshot() else { return Vec::new() };
                        snap.tilemap_neighbors(entity_id as u64, tx, ty).into_iter().map(List).collect()
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "tilemap_distance",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, ax: i32, ay: i32, bx: i32, by: i32| -> i32 {
                        let st = s.borrow();
                        let Some(snap) = st.snapshot() else { return -1 };
                        snap.tilemap_distance(entity_id as u64, (ax, ay), (bx, by))
                    }))?,
                )?;
            }
            // 2.6 — A* path-finding. Returns `null` on failure, or a JS
            // array of `[tx, ty]` pairs (`tilemap_find_path`) /
            // `[wx, wy]` world centres (`tilemap_find_path_world`). The
            // optional `mode` argument is `"manhattan"` or `"diagonal"`.
            {
                let s = state.clone();
                globals.set(
                    "tilemap_find_path",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, layer_name: String, sx: i32, sy: i32, gx: i32, gy: i32, mode: rquickjs::Value<'_>| -> Option<Vec<List<(i32, i32)>>> {
                        let st = s.borrow();
                        let snap = st.snapshot()?;
                        let mode_str = mode.as_string().and_then(|s| s.to_string().ok()).unwrap_or_else(|| "manhattan".to_string());
                        let path = snap.tilemap_find_path(entity_id as u64, &layer_name, (sx, sy), (gx, gy), &mode_str)?;
                        Some(path.into_iter().map(List).collect())
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "tilemap_find_path_world",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, layer_name: String, sx: f32, sy: f32, gx: f32, gy: f32, mode: rquickjs::Value<'_>| -> Option<Vec<List<(f32, f32)>>> {
                        let st = s.borrow();
                        let snap = st.snapshot()?;
                        let mode_str = mode.as_string().and_then(|s| s.to_string().ok()).unwrap_or_else(|| "manhattan".to_string());
                        let path = snap.tilemap_find_path_world(entity_id as u64, &layer_name, (sx, sy), (gx, gy), &mode_str)?;
                        Some(path.into_iter().map(List).collect())
                    }))?,
                )?;
            }

            // --- Phase 5.12: lighting --------------------------------------
            {
                let s = state.clone();
                globals.set(
                    "set_light_color",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, r: f32, g: f32, b: f32| {
                        s.borrow_mut().pending.push(BridgeCommand::SetLightColor {
                            entity_id: entity_id as u64,
                            r,
                            g,
                            b,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_light_intensity",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, intensity: f32| {
                        s.borrow_mut().pending.push(BridgeCommand::SetLightIntensity {
                            entity_id: entity_id as u64,
                            intensity,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_color_lut",
                    Function::new(ctx.clone(), MutFn::new(move |texture_id: String, strength: f32| {
                        s.borrow_mut().pending.push(BridgeCommand::SetColorLut { texture_id, strength });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_light_mask",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, mask: f64| {
                        s.borrow_mut().pending.push(BridgeCommand::SetLightMask {
                            entity_id: entity_id as u64,
                            mask: mask.clamp(0.0, u32::MAX as f64) as u32,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_light_height",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, height: f32| {
                        s.borrow_mut().pending.push(BridgeCommand::SetLightHeight {
                            entity_id: entity_id as u64,
                            height,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_light_radius",
                    Function::new(ctx.clone(), MutFn::new(move |entity_id: f64, radius: f32| {
                        s.borrow_mut().pending.push(BridgeCommand::SetLightRadius {
                            entity_id: entity_id as u64,
                            radius,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_ambient_light",
                    Function::new(ctx.clone(), MutFn::new(move |r: f32, g: f32, b: f32, intensity: f32| {
                        s.borrow_mut().pending.push(BridgeCommand::SetAmbientLight {
                            r,
                            g,
                            b,
                            intensity,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_toon_lighting",
                    Function::new(ctx.clone(), MutFn::new(move |bands: f32, softness: Opt<f32>, dither: Opt<f32>| {
                        s.borrow_mut().pending.push(BridgeCommand::SetToonLighting { bands, softness: softness.0, dither: dither.0 });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_ambient_occlusion",
                    Function::new(ctx.clone(), MutFn::new(move |radius: Opt<f32>, strength: Opt<f32>| {
                        s.borrow_mut().pending.push(BridgeCommand::SetAmbientOcclusion { radius: radius.0, strength: strength.0 });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_global_illumination",
                    Function::new(ctx.clone(), MutFn::new(move |strength: Opt<f32>, scale: Opt<f32>| {
                        s.borrow_mut().pending.push(BridgeCommand::SetGlobalIllumination { strength: strength.0, scale: scale.0 });
                    }))?,
                )?;
            }

            // --- Phase 5.12: post-process ----------------------------------
            {
                let s = state.clone();
                globals.set(
                    "set_post_process_enabled",
                    Function::new(ctx.clone(), MutFn::new(move |effect_name: String, enabled: bool| {
                        s.borrow_mut().pending.push(BridgeCommand::SetPostProcessEnabled {
                            effect_name,
                            enabled,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_post_process_float",
                    Function::new(ctx.clone(), MutFn::new(move |effect_name: String, param_name: String, value: f32| {
                        s.borrow_mut().pending.push(BridgeCommand::SetPostProcessFloat {
                            effect_name,
                            param_name,
                            value,
                        });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_post_process_vec4",
                    Function::new(ctx.clone(), MutFn::new(move |
                        effect_name: String,
                        param_name: String,
                        x: f32, y: f32, z: f32, w: f32,
                    | {
                        s.borrow_mut().pending.push(BridgeCommand::SetPostProcessVec4 {
                            effect_name,
                            param_name,
                            value: [x, y, z, w],
                        });
                    }))?,
                )?;
            }

            // --- Phase 5.12: debug drawing & overlay -----------------------
            {
                let s = state.clone();
                globals.set(
                    "draw_debug_line",
                    Function::new(ctx.clone(), MutFn::new(move |
                        x1: f32, y1: f32, x2: f32, y2: f32, r: f32, g: f32, b: f32,
                    | {
                        s.borrow_mut()
                            .pending
                            .push(BridgeCommand::DrawDebugLine { x1, y1, x2, y2, r, g, b });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "draw_debug_rect",
                    Function::new(ctx.clone(), MutFn::new(move |
                        x: f32, y: f32, w: f32, h: f32, r: f32, g: f32, b: f32,
                    | {
                        s.borrow_mut()
                            .pending
                            .push(BridgeCommand::DrawDebugRect { x, y, w, h, r, g, b });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "draw_debug_circle",
                    Function::new(ctx.clone(), MutFn::new(move |
                        x: f32, y: f32, radius: f32, r: f32, g: f32, b: f32,
                    | {
                        s.borrow_mut()
                            .pending
                            .push(BridgeCommand::DrawDebugCircle { x, y, radius, r, g, b });
                    }))?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_debug_overlay",
                    Function::new(ctx.clone(), MutFn::new(move |visible: bool| {
                        s.borrow_mut()
                            .pending
                            .push(BridgeCommand::SetDebugOverlay { visible });
                    }))?,
                )?;
            }

            // P1.3 — camera shake. JS callers pass `frequency` as 0 (or
            // omit it via `||0` in user code) to get the engine default;
            // `Camera2D::start_shake` substitutes 20 Hz in that case.
            {
                let s = state.clone();
                globals.set(
                    "camera_shake",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |amplitude: f32, duration: f32, frequency: f32| {
                            s.borrow_mut().pending.push(BridgeCommand::CameraShake {
                                amplitude,
                                duration,
                                frequency,
                            });
                        }),
                    )?,
                )?;
            }

            // --- Phase 5.13: custom component access -------------------------
            // The host JS adapter is parity-only; rquickjs `MutFn` closures
            // can't easily return dynamic types, so this surface is restricted
            // to numeric fields (which is all bunnymark / real game scripts
            // use today). Luau retains full Number/Bool/String fidelity.
            {
                let s = state.clone();
                globals.set(
                    "get_custom",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |
                            entity_id: f64,
                            component_name: String,
                            field_name: String,
                        | -> f64 {
                            let st = s.borrow();
                            let snap = match st.snapshot() { Some(s) => s, None => return 0.0 };
                            // Fast path: avoid the per-call `serde_json::Value`
                            // clone (`get_custom_field` clones for the script
                            // API) by reading the f64 directly.
                            snap.get_custom_field_f64(
                                entity_id as u64,
                                &component_name,
                                &field_name,
                            )
                            .unwrap_or(0.0)
                        }),
                    )?,
                )?;
            }
            // --- Data-oriented design: static data-table access -------------
            // Like `get_custom` above, the host JS adapter is numeric-only
            // (rquickjs `MutFn` can't easily return dynamic types). Returns 0
            // for non-numeric / missing fields. Luau retains full fidelity.
            {
                let s = state.clone();
                globals.set(
                    "get_data",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |
                            table_id: String,
                            row_id: String,
                            field_name: String,
                        | -> f64 {
                            let st = s.borrow();
                            st.data_registry
                                .get_f64(&table_id, &row_id, &field_name)
                                .unwrap_or(0.0)
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "set_custom",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |
                            entity_id: f64,
                            component_name: String,
                            field_name: String,
                            value: f64,
                        | {
                            // Intern field/component labels so the
                            // BridgeCommand carries `Arc<str>`s instead
                            // of fresh per-call `String`s.
                            let comp_arc = crate::scripting::intern::intern(&component_name);
                            let field_arc = crate::scripting::intern::intern(&field_name);
                            s.borrow_mut().pending.push(BridgeCommand::SetCustomField {
                                entity_id: entity_id as u64,
                                component_name: comp_arc,
                                field_name: field_arc,
                                value: serde_json::Value::from(value),
                            });
                        }),
                    )?,
                )?;
            }
            {
                // Phase 6.x — batched custom-component writer (parity with
                // Lua's `set_fields`). Numeric values only, matching the
                // host JS adapter's existing restriction on `set_custom`.
                let s = state.clone();
                globals.set(
                    "set_fields",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |
                            entity_id: f64,
                            component_name: String,
                            obj: Object<'_>,
                        | -> rquickjs::Result<()> {
                            let mut out: Vec<(std::sync::Arc<str>, serde_json::Value)> =
                                Vec::with_capacity(4);
                            for prop in obj.props::<String, f64>() {
                                let (k, v) = prop?;
                                let key_arc = crate::scripting::intern::intern(&k);
                                out.push((key_arc, serde_json::Value::from(v)));
                            }
                            let comp_arc = crate::scripting::intern::intern(&component_name);
                            s.borrow_mut().pending.push(BridgeCommand::SetCustomFields {
                                entity_id: entity_id as u64,
                                component_name: comp_arc,
                                fields: out,
                            });
                            Ok(())
                        }),
                    )?,
                )?;
            }

            // --- Persistent K/V store (survives scene transitions) ---
            globals.set(
                "persistent_set_number",
                Function::new(ctx.clone(), MutFn::new(move |key: String, value: f64| {
                    crate::scripting::persistent::set_number(&key, value);
                }))?,
            )?;
            globals.set(
                "persistent_get_number",
                Function::new(ctx.clone(), MutFn::new(move |key: String| -> Option<f64> {
                    crate::scripting::persistent::get_number(&key)
                }))?,
            )?;
            globals.set(
                "persistent_set_string",
                Function::new(ctx.clone(), MutFn::new(move |key: String, value: String| {
                    crate::scripting::persistent::set_string(&key, &value);
                }))?,
            )?;
            globals.set(
                "persistent_get_string",
                Function::new(ctx.clone(), MutFn::new(move |key: String| -> Option<String> {
                    crate::scripting::persistent::get_string(&key)
                }))?,
            )?;
            globals.set(
                "persistent_has",
                Function::new(ctx.clone(), MutFn::new(move |key: String| -> bool {
                    crate::scripting::persistent::has(&key)
                }))?,
            )?;
            globals.set(
                "persistent_clear",
                Function::new(ctx.clone(), MutFn::new(move || {
                    crate::scripting::persistent::clear();
                }))?,
            )?;

            // ── 2.3 Save game system ────────────────────────────────────
            // Browser JSON values are exchanged as strings so we don't have
            // to ship a recursive rquickjs ↔ serde_json converter. Callers
            // are expected to use `JSON.stringify` / `JSON.parse`:
            //
            //   save_set("slot1", "inventory", JSON.stringify(inv));
            //   var inv = JSON.parse(save_get("slot1", "inventory") || "null");
            {
                let s = state.clone();
                globals.set(
                    "save_set",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |slot: String, key: String, value: String| {
                            let json = serde_json::from_str(&value)
                                .unwrap_or(serde_json::Value::String(value));
                            s.borrow_mut().pending.push(BridgeCommand::SaveSet {
                                slot,
                                key,
                                value: json,
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "save_get",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |slot: String, key: String| -> Option<String> {
                            let st = s.borrow();
                            let snap = st.snapshot()?;
                            let v = snap.get_save_value(&slot, &key)?;
                            Some(serde_json::to_string(v).unwrap_or_default())
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "save_commit",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |slot: String| {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::SaveCommit { slot });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "save_clear",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |slot: String| {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::SaveClear { slot });
                        }),
                    )?,
                )?;
            }

            // ── 2.4 Audio mixer (bus tree) ──────────────────────────────
            {
                let s = state.clone();
                globals.set(
                    "audio_create_bus",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |name: String, parent: Option<String>, volume: Option<f32>| {
                            s.borrow_mut().pending.push(BridgeCommand::AudioCreateBus {
                                name,
                                parent,
                                volume: volume.unwrap_or(1.0),
                            });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "audio_remove_bus",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |name: String| {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::AudioRemoveBus { name });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "audio_set_bus_volume",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |name: String, volume: f32| {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::AudioSetBusVolume { name, volume });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "audio_set_bus_effect",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |name: String, effect: String, value: f32| {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::AudioSetBusEffect { name, effect, value });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "audio_get_bus_volume",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |name: String| -> Option<f32> {
                            s.borrow().snapshot()?.get_bus_volume(&name)
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "audio_set_bus_muted",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |name: String, muted: bool| {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::AudioSetBusMuted { name, muted });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "audio_is_bus_muted",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |name: String| -> bool {
                            s.borrow()
                                .snapshot()
                                .map(|snap| snap.is_bus_muted(&name))
                                .unwrap_or(false)
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "audio_set_bus_solo",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |name: String, solo: bool| {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::AudioSetBusSolo { name, solo });
                        }),
                    )?,
                )?;
            }

            // ── 2.5 Input action remapping ─────────────────────────────
            // Writes flow through bridge commands and are drained by the
            // runtime via `crate::input::apply_input_remap`. Reads use
            // the per-frame `BridgeReadSnapshot`.
            {
                let s = state.clone();
                globals.set(
                    "input_get_actions",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move || -> Vec<String> {
                            s.borrow()
                                .snapshot()
                                .map(|snap| snap.list_action_names())
                                .unwrap_or_default()
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "input_get_bindings",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |action: String| -> Option<Vec<String>> {
                            s.borrow()
                                .snapshot()?
                                .get_action_bindings(&action)
                                .cloned()
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "input_set_bindings",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |action: String, keys: Vec<String>| {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::InputSetBindings { action, keys });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "input_add_binding",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |action: String, key: String| {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::InputAddBinding { action, key });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "input_remove_binding",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |action: String, key: String| {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::InputRemoveBinding { action, key });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "input_clear_bindings",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |action: String| {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::InputClearBindings { action });
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "input_reset_bindings",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move || {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::InputResetBindings);
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "input_get_dead_zone",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |action: String| -> f32 {
                            s.borrow()
                                .snapshot()
                                .map(|snap| snap.get_action_dead_zone(&action))
                                .unwrap_or(0.0)
                        }),
                    )?,
                )?;
            }
            {
                let s = state.clone();
                globals.set(
                    "input_set_dead_zone",
                    Function::new(
                        ctx.clone(),
                        MutFn::new(move |action: String, value: f32| {
                            s.borrow_mut()
                                .pending
                                .push(BridgeCommand::InputSetDeadZone { action, value });
                        }),
                    )?,
                )?;
            }

            Ok(())
        })
        .map_err(|e| make_err(None, ScriptErrorKind::Runtime, e))
}

/// Script-reuse Phase A: install the `require()` global on the JS context.
///
/// We can't easily build the wrapping/eval/cache loop from inside a Rust
/// `MutFn` closure (it would need a `Ctx<'js>` parameter and lifetime-juggling
/// `Persistent` cache entries), so we split the work:
///
/// - **Rust side**: a single `__resolve_module__(path) -> [ok, body]` callback
///   that calls the host resolver and, when the path looks like Lua, runs the
///   raw source through [`super::transpiler::transpile`]. The returned tuple
///   distinguishes success from failure without having to throw a JS exception
///   from Rust.
/// - **JS side**: a small wrapper installed via `Context::eval` defines
///   `__module_cache__`, `__module_loading__`, and `require()` on globals.
///   The wrapper handles cache lookups, cycle detection, and `eval`-ing the
///   module body inside an IIFE so `return value` at the top of the source
///   becomes the module export — same convention as Lua chunks.
fn install_require_globals(
    context: &Context,
    state: &Rc<RefCell<HostBridgeState>>,
) -> Result<(), ScriptError> {
    context
        .with(|ctx| -> rquickjs::Result<()> {
            let globals = ctx.globals();

            // Rust callback: returns [true, js_source] on success or
            // [false, error_message] on failure. Pure data — no exception
            // construction needed on the Rust side.
            let s = state.clone();
            globals.set(
                "__resolve_module__",
                Function::new(
                    ctx.clone(),
                    MutFn::new(move |path: String| -> List<(bool, String)> {
                        let resolver = s.borrow().resolver.clone();
                        match resolver(&path) {
                            Ok(source) => {
                                // Path-based language detection mirrors
                                // `JsAdapter::load`: bare/`.lua`/`.luau`
                                // sources go through the Luau→JS transpiler;
                                // `.js`/`.mjs` pass through verbatim.
                                let js = if path_is_lua(&path) {
                                    super::transpiler::transpile(&source)
                                } else {
                                    source
                                };
                                List((true, js))
                            }
                            Err(e) => List((false, e)),
                        }
                    }),
                )?,
            )?;

            // JS-side wrapper. Single eval; defines two cache objects and the
            // `require` function. The IIFE wrapping mirrors `HostJs::load`: a
            // module's top-level `return X` becomes the export.
            let install_src = r#"
                globalThis.__module_cache__ = {};
                globalThis.__module_loading__ = {};
                globalThis.require = function(path) {
                    if (Object.prototype.hasOwnProperty.call(__module_cache__, path)) {
                        return __module_cache__[path];
                    }
                    if (__module_loading__[path]) {
                        throw new Error("require('" + path + "'): circular dependency detected");
                    }
                    var r = __resolve_module__(path);
                    if (!r[0]) {
                        throw new Error(r[1]);
                    }
                    var body = r[1];
                    __module_loading__[path] = true;
                    try {
                        var mod = eval("(function() {\n" + body + "\n})()");
                        __module_cache__[path] = mod;
                        return mod;
                    } finally {
                        delete __module_loading__[path];
                    }
                };
            "#;
            let _: rquickjs::Value = ctx.eval(install_src)?;
            Ok(())
        })
        .map_err(|e| make_err(None, ScriptErrorKind::Runtime, e))
}

/// Path-extension heuristic mirroring `JsAdapter::load`: paths ending in
/// `.lua`/`.luau`, or paths with no recognised JS extension, are treated as
/// Lua source and run through the transpiler before evaluation.
fn path_is_lua(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".lua") || lower.ends_with(".luau") {
        return true;
    }
    if lower.ends_with(".js") || lower.ends_with(".mjs") {
        return false;
    }
    // Unknown extension or no extension: default to Lua so library paths
    // like `require("lib/math")` work without forcing a `.lua` suffix on the
    // script side. Hosts that want JS-only modules can supply explicit `.js`.
    true
}

fn make_err<E: std::fmt::Display>(
    script_path: Option<String>,
    kind: ScriptErrorKind,
    err: E,
) -> ScriptError {
    ScriptError {
        backend: BackendKind::JavaScript,
        script_path: script_path.unwrap_or_default(),
        location: None,
        kind,
        message: err.to_string(),
        bridge_call: None,
    }
}

/// Extract a human-readable message from a `rquickjs::Error`. For
/// `Error::Exception` (the variant used when JS code throws), the actual
/// thrown value is sitting on the `Ctx` and must be retrieved via
/// `Ctx::catch`. We render it via `String(value)` semantics: `Error` instances
/// stringify as `"Error: <message>"`; primitives stringify directly.
fn format_quickjs_error(ctx: &Ctx<'_>, err: rquickjs::Error) -> String {
    if matches!(err, rquickjs::Error::Exception) {
        let exc = ctx.catch();
        // Try `.message` first (Error instances), fall back to `String(exc)`.
        if let Some(obj) = exc.as_object()
            && let Ok(msg) = obj.get::<_, String>("message")
        {
            return msg;
        }
        if let Some(s) = exc.as_string()
            && let Ok(s) = s.to_string()
        {
            return s;
        }
        return format!("{exc:?}");
    }
    err.to_string()
}

/// Encode an arbitrary string as a JS double-quoted string literal —
/// escaping `\\`, `"`, and control characters. Used to safely interpolate
/// user-provided paths into the IIFE wrapper. Equivalent to
/// `JSON.stringify(s)` for plain strings.
fn serialize_js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
