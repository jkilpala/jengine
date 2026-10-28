//! JavaScript backend adapter for the Script Bridge — web-only.
//!
//! Phase 4 strategy is **Option B**: native runs Luau via [`super::luau::LuauAdapter`],
//! web (`jengine-web` + `jengine-editor-viewport`) runs the same conceptual script
//! through this adapter, which transpiles Luau → JS internally and dispatches via the
//! browser's JS engine. The transpiler is private to this module — no other crate
//! should depend on it. See `docs/archive/scripting-architecture-plan.md` §8 Phase 4.
//!
//! ## Phase 4.4 status
//! - Module layout + feature gate (`js-adapter`) in place.
//! - Transpiler quarantined as [`transpiler`] (canonical implementation).
//! - On `wasm32`, [`load`](JsAdapter::load) compiles the script through
//!   [`browser::compile`], owning the resulting `(init, update, on_event)` JS
//!   function values. Consumer crates pull them via
//!   [`script_functions`](JsAdapter::script_functions) or
//!   [`clone_script_functions`](JsAdapter::clone_script_functions) and dispatch
//!   them through their existing batched per-frame runner.
//! - Phase 4.6 will route those handles through [`super::super::context::BridgeContext`]
//!   instead of the global `window.__jengine_play.*` state for native↔web parity.

use std::collections::HashMap;

use super::super::adapter::{ScriptAdapter, ScriptHandle};
use super::super::context::BridgeContext;
use super::super::diagnostics::{BackendKind, ScriptError, ScriptErrorKind};

#[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
use rquickjs::{Function, Persistent};

#[cfg(target_arch = "wasm32")]
mod browser;

/// Publish the loaded packages' script globals into the browser script
/// preamble.
///
/// Re-exported rather than making the whole `browser` module public: this is
/// the only thing outside the bridge that a runtime needs from it. Call once,
/// after `PackageHost::initialize` and before any script compiles - globals
/// published later do not reach wrappers already built.
#[cfg(target_arch = "wasm32")]
pub fn set_package_script_globals(js: String) {
    browser::set_package_globals_js(js);
}
#[cfg(target_arch = "wasm32")]
mod fengari;
/// What the browser hosts publish into `__jengine_play` through a cache.
#[cfg(target_arch = "wasm32")]
pub mod publish;
/// The per-frame script mirror both browser hosts publish and read back.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod mirror;
/// The browser hosts' running scripts: instances, the spawn map, and the calls
/// into a script.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod scripts;
/// Script commands both browser hosts apply the same way, needing more than
/// the world (audio, tweens, saves, input, ...).
// Fades and the post-process stack are the renderer's, and both hosts have
// it; a package built for the web on its own does not, and needs none of this.
#[cfg(all(target_arch = "wasm32", feature = "web", feature = "gpu-wgpu"))]
pub mod host_dispatch;
/// Synchronous host callbacks the browser runtimes install for reads that
/// cannot be deferred (pathfinding). Public: the viewport and web crates
/// drive the snapshot.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod nav;
/// `Locale.*`: synchronous hooks over the process-wide language
/// (`crate::locale`), installed with [`nav`]'s.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod locale;
/// `Input.rumble` / `Input.vibrate`: hooks over the process-wide haptics
/// queue (`crate::input::haptics`), installed with [`nav`]'s.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod haptics;
/// `Display.*` / `Cursor.*`: one hook over the process-wide display settings
/// (`crate::display`), installed with [`nav`]'s.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod display;
/// `Scene.load_additive` and kin: one hook over the process-wide streaming
/// requests (`crate::scene::streaming`), installed with [`nav`]'s.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod streaming;
/// `Packs.*`: one hook over the process-wide asset packs
/// (`crate::asset::packs`), installed with [`nav`]'s.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod packs;
/// `LiveOps.*`: one hook over the process-wide live ops (`crate::liveops`),
/// installed with [`nav`]'s.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod liveops;
/// The synchronous `raycast` callback, the sibling of [`nav`]. Needs the
/// `physics` feature as well as `web`, because there is nothing to cast
/// against without it.
#[cfg(all(target_arch = "wasm32", feature = "web", feature = "physics"))]
pub mod ray;
/// The input snapshot browser-runtime scripts read. Public because both the
/// editor viewport and the web runtime publish it, and they must publish the
/// same thing — see the module docs.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod input_mirror;
/// `Input.is_action_held` and kin, pads' identity, prompts: one hook over the
/// frame's `InputView`, installed with [`nav`]'s.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod input_query;
/// `Audio.crossfade_music` and kin: one hook over the process-wide music
/// director (`crate::audio::music`), installed with [`nav`]'s.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod music;
/// `Noise.*`: one hook over [`crate::noise`], installed with [`nav`]'s.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod noise;
/// `math.*`, `^` and `%` for Fengari: [`crate::scripting::script_math`] as
/// `globalThis.__jengine_script_math`, installed with [`nav`]'s.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod script_math;
/// `Audio.get_spectrum` / `get_level` / `synth`: one hook over
/// [`crate::audio::analysis`], installed with [`nav`]'s.
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod analysis;
/// `Screenshot.take` / `ready`: one hook over [`crate::renderer::screenshot`].
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod screenshot;
/// `Accessibility.announce`: one hook over [`crate::ui::accessibility`].
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod accessibility;
#[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
mod host;
mod transpiler;

#[cfg(test)]
#[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host", feature = "native"))]
mod parity;

#[cfg(test)]
#[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host", feature = "native"))]
mod perf;

#[cfg(target_arch = "wasm32")]
pub use browser::{format_entity_label, format_script_error, BrowserScriptFns};

/// Public re-export of the canonical Lua → JS transpiler.
///
/// Used during the P4.4/P4.5 cutover so the legacy play-mode paths in `jengine-web`
/// and `jengine-editor-viewport` can call the single source of truth instead of each
/// keeping their own copy. After Phases 5–6 the transpiler stays private; consumer
/// crates should go through [`JsAdapter::load`] instead.
pub use transpiler::transpile_lua_to_js;

/// Per-script bookkeeping after a successful load.
struct LoadedScript {
    /// Source path (for diagnostics).
    #[allow(dead_code)] // surfaced via diagnostics in P4.6
    path: String,
    /// Transpiled JavaScript source (host-side / for diagnostics).
    #[allow(dead_code)]
    js_source: String,
}

/// Slice A.JS — descriptor fields needed to install a dynamic function into
/// the host QuickJS engine once it has been lazily constructed.
/// [`crate::package::script_api::ScriptFunctionDescriptor`] is intentionally
/// non-`Clone` because the metadata-only path doesn't need it, so we keep
/// only the install-relevant subset here.
#[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
struct PendingDynamicFn {
    name: String,
    arg_types: Vec<crate::package::script_api::ScriptValueType>,
    handler: crate::package::script_api::ScriptHandler,
}

/// JavaScript adapter — web-only scripting backend (Phase 4).
///
/// Compiles Luau (or hand-written JS) to a browser-executable form on `load`, then
/// dispatches lifecycle calls through the JS engine.
pub struct JsAdapter {
    scripts: HashMap<u64, LoadedScript>,
    next_handle: u64,
    /// Compiled per-script JS function triples. Only populated on `wasm32` —
    /// the host-side build keeps the map empty (skeleton lifecycle).
    #[cfg(target_arch = "wasm32")]
    fns: HashMap<u64, BrowserScriptFns>,
    /// Per-script host-side bookkeeping (init/update Persistent functions).
    /// Declared **before** `host` so the Persistents are dropped first —
    /// QuickJS asserts at runtime drop that no GC objects remain alive.
    #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
    host_scripts: HashMap<u64, host::HostScript>,
    /// Host-side QuickJS engine — only present when the parity-test feature is
    /// enabled. Production native builds never instantiate this.
    #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
    host: Option<host::HostJs>,
    /// Script-reuse Phase A: resolver staged before the host engine has been
    /// lazily constructed. Applied to the freshly-built host on first
    /// [`Self::load`]. Once the host exists, [`Self::set_module_resolver`]
    /// installs directly and clears this field.
    #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
    pending_resolver: Option<crate::scripting::bridge::ModuleResolver>,
    /// Slice A.JS — dynamic function descriptors staged before the host
    /// engine has been lazily constructed. Applied to the freshly-built host
    /// on first [`Self::load`]. Once the host exists,
    /// [`Self::install_dynamic_function`] installs directly. We store only
    /// the fields the installer needs because [`crate::package::script_api
    /// ::ScriptFunctionDescriptor`] is intentionally non-`Clone`.
    #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
    pending_dynamic_fns: Vec<PendingDynamicFn>,
    /// Data-oriented design — data-table registry staged before the host
    /// engine has been lazily constructed. Applied to the freshly-built host
    /// on first [`Self::load`].
    #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
    pending_data_registry: Option<std::rc::Rc<crate::data::DataRegistry>>,
    /// Phase D — wasm-bindgen [`Closure`] keeping the browser-side
    /// `__resolve_module__` callback alive for as long as `require()`
    /// might fire. Replaced (and the previous one dropped) on each call to
    /// [`Self::set_module_resolver`].
    #[cfg(target_arch = "wasm32")]
    module_resolver_closure:
        Option<wasm_bindgen::closure::Closure<dyn FnMut(String) -> wasm_bindgen::JsValue>>,
}

impl Default for JsAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl JsAdapter {
    pub fn new() -> Self {
        Self {
            scripts: HashMap::new(),
            next_handle: 1,
            #[cfg(target_arch = "wasm32")]
            fns: HashMap::new(),
            #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
            host_scripts: HashMap::new(),
            #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
            host: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
            pending_resolver: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
            pending_dynamic_fns: Vec::new(),
            #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
            pending_data_registry: None,
            #[cfg(target_arch = "wasm32")]
            module_resolver_closure: None,
        }
    }

    /// Load an *already-transpiled* JS source. Used by the web/editor-viewport
    /// runtimes when they fetched a hand-written `.js` script (no transpile
    /// needed) or when they pre-transpiled `.luau` themselves during the P4.4
    /// transition. Once those call sites route raw Luau through [`Self::load`]
    /// this can be removed (or kept as a debug shortcut).
    #[cfg(target_arch = "wasm32")]
    pub fn load_compiled(
        &mut self,
        path: &str,
        js_source: &str,
    ) -> Result<ScriptHandle, ScriptError> {
        let fns = browser::compile(path, js_source)?;
        let handle = self.alloc_handle(path, js_source.to_string());
        self.fns.insert(handle.0, fns);
        Ok(handle)
    }

    /// Borrow the compiled JS functions for a loaded script. Returns `None` if
    /// the handle is unknown or the script failed to compile.
    #[cfg(target_arch = "wasm32")]
    pub fn script_functions(&self, handle: ScriptHandle) -> Option<&BrowserScriptFns> {
        self.fns.get(&handle.0)
    }

    /// Clone the compiled JS functions for a loaded script. JsValue is a cheap
    /// refcounted handle, so cloning is fine for prefab caches that need their
    /// own copy of the function trio.
    #[cfg(target_arch = "wasm32")]
    pub fn clone_script_functions(&self, handle: ScriptHandle) -> Option<BrowserScriptFns> {
        self.fns.get(&handle.0).cloned()
    }

    fn alloc_handle(&mut self, path: &str, js_source: String) -> ScriptHandle {
        let handle = ScriptHandle(self.next_handle);
        self.next_handle += 1;
        self.scripts
            .insert(handle.0, LoadedScript { path: path.to_string(), js_source });
        handle
    }

    /// Script-reuse Phase A: install the host-side resolver consulted by the
    /// script-side `require` global. Mirrors
    /// [`super::luau::LuauAdapter::set_module_resolver`].
    ///
    /// On the WASM/browser path this is currently a no-op — the browser
    /// runtime resolves modules via its own fetch path. (Wired in Phase D.)
    #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
    pub fn set_module_resolver(&mut self, resolver: crate::scripting::bridge::ModuleResolver) {
        if let Some(host) = self.host.as_ref() {
            host.set_module_resolver(resolver);
        } else {
            // Host engine is built lazily on first `load()`. Stash the
            // resolver so it's installed on the freshly-built engine.
            self.pending_resolver = Some(resolver);
        }
    }

    /// Stub for the non-wasm/no-host build (compiles `JsAdapter` without a
    /// JS engine — used by smoke tests). No-op.
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "js-adapter-host")))]
    pub fn set_module_resolver(&mut self, _resolver: crate::scripting::bridge::ModuleResolver) {
        // intentionally empty
    }

    /// Data-oriented design — install the `.data` registry read by `get_data`.
    /// Host build: forwards to the engine (or stages until it's built).
    #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
    pub fn set_data_registry(&mut self, registry: std::rc::Rc<crate::data::DataRegistry>) {
        if let Some(host) = self.host.as_ref() {
            host.set_data_registry(registry);
        } else {
            self.pending_data_registry = Some(registry);
        }
    }

    /// No-op for the non-wasm/no-host smoke-test build.
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "js-adapter-host")))]
    pub fn set_data_registry(&mut self, _registry: std::rc::Rc<crate::data::DataRegistry>) {
        // intentionally empty
    }

    /// No-op on wasm: the browser path reads data tables from the
    /// `window.__jengine_play.data_tables` global, not a Rust registry.
    #[cfg(target_arch = "wasm32")]
    pub fn set_data_registry(&mut self, _registry: std::rc::Rc<crate::data::DataRegistry>) {
        // intentionally empty
    }

    /// Phase D — wasm32 install of the browser-side `require()` infrastructure.
    /// Mirrors the host build's behaviour: cached modules are NOT re-evaluated;
    /// call [`Self::invalidate_module_cache`] first if you need them dropped.
    #[cfg(target_arch = "wasm32")]
    pub fn set_module_resolver(&mut self, resolver: crate::scripting::bridge::ModuleResolver) {
        match browser::install_require_globals(resolver) {
            Ok(cb) => {
                // Drop any previous closure only AFTER the new one is
                // installed on the JS side, so there's never a window where
                // `require()` calls into freed Rust memory.
                self.module_resolver_closure = Some(cb);
            }
            Err(e) => {
                log::error!(
                    "[JsAdapter] failed to install browser require resolver: {}",
                    e.message
                );
            }
        }
    }

    /// Phase D — drop every cached browser module export. Safe to call when
    /// no resolver has been installed (no-op).
    #[cfg(target_arch = "wasm32")]
    pub fn invalidate_module_cache(&self) {
        if let Err(e) = browser::invalidate_module_cache() {
            log::warn!(
                "[JsAdapter] invalidate_module_cache failed: {}",
                e.message
            );
        }
    }
}

impl ScriptAdapter for JsAdapter {
    /// Hot-path frame setup. Forwards to the host engine's `frame_begin`
    /// when present so the per-entity update loop doesn't pay the
    /// state-install dance every entity. WASM/browser path is a no-op.
    fn frame_begin(&mut self, _ctx: &mut BridgeContext<'_>) {
        #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
        {
            if let Some(host) = self.host.as_ref() {
                host.frame_begin(_ctx);
            }
        }
    }

    /// Counterpart to [`Self::frame_begin`]. Drains any commands queued by
    /// the per-entity loop into `ctx` and clears the installed state. WASM
    /// path is a no-op.
    fn frame_end(&mut self, _ctx: &mut BridgeContext<'_>) {
        #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
        {
            if let Some(host) = self.host.as_ref() {
                host.frame_end(_ctx);
            }
        }
    }

    fn load(&mut self, path: &str, source: &str) -> Result<ScriptHandle, ScriptError> {
        // Hand-written `.js` sources are passed through verbatim; only Luau
        // (`.lua` / `.luau` / no-extension) sources go through the transpiler.
        // Without this guard, real JS like `if (x) { y = -y; }` gets mangled
        // because the transpiler treats `{ ... }` as Lua table constructors.
        let is_js = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("js") || e.eq_ignore_ascii_case("mjs"))
            .unwrap_or(false);
        let js_source = if is_js {
            source.to_string()
        } else {
            transpiler::transpile(source)
        };

        // Phase B — parse `extends = "..."` from the original source (the
        // header lives inside `script = { ... }` which the transpiler
        // strips, so we must inspect the pre-transpile text). Recognises
        // both `extends = "lib/foo"` (Lua) and `extends: "lib/foo"` (JS).
        // Only the host path consumes it for now; browser is Phase D.
        #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
        let extends_parent = super::super::metadata::parse_extends(source);

        #[cfg(target_arch = "wasm32")]
        {
            // Lua sources: prefer the Fengari (real Lua VM) compile path when
            // the editor frontend has installed `globalThis.__jengine_lua_compile`.
            // Falls through to the legacy transpiler+eval path otherwise so that
            // pure-WASM (non-editor) embeddings keep working.
            let fns = if !is_js {
                match fengari::try_compile(path, source) {
                    Some(Ok(f)) => f,
                    Some(Err(e)) => return Err(e),
                    None => browser::compile(path, &js_source)?,
                }
            } else {
                browser::compile(path, &js_source)?
            };
            let handle = self.alloc_handle(path, js_source);
            self.fns.insert(handle.0, fns);
            Ok(handle)
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            #[cfg(feature = "js-adapter-host")]
            {
                // Lazily initialise the host JS engine on first load.
                if self.host.is_none() {
                    let engine = host::HostJs::new()?;
                    if let Some(r) = self.pending_resolver.take() {
                        engine.set_module_resolver(r);
                    }
                    if let Some(reg) = self.pending_data_registry.take() {
                        engine.set_data_registry(reg);
                    }
                    // Slice A.JS — drain any descriptors registered before the
                    // host engine was built.
                    for pending in std::mem::take(&mut self.pending_dynamic_fns) {
                        if let Err(e) = engine.install_dynamic_function_parts(
                            &pending.name,
                            &pending.arg_types,
                            pending.handler.clone(),
                        ) {
                            log::warn!(
                                "[JsAdapter/host] install_dynamic_function pending '{}' failed: {}",
                                pending.name,
                                e.message
                            );
                        }
                    }
                    self.host = Some(engine);
                }
                let engine = self.host.as_ref().expect("host engine just initialised");
                let host_script = engine.load(path, &js_source, extends_parent.as_deref())?;
                let handle = self.alloc_handle(path, js_source);
                self.host_scripts.insert(handle.0, host_script);
                Ok(handle)
            }
            #[cfg(not(feature = "js-adapter-host"))]
            {
                // Host build without the parity-test feature: skeleton lifecycle.
                // Phase 4.6 parity tests enable `js-adapter-host` to exercise this
                // path through QuickJS; production never enables it.
                Ok(self.alloc_handle(path, js_source))
            }
        }
    }

    fn init(
        &mut self,
        handle: ScriptHandle,
        _entity_id: u64,
        _ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        if !self.scripts.contains_key(&handle.0) {
            return Err(unknown_handle());
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
        {
            return host_dispatch(self, handle, _entity_id, _ctx, HostLifecycle::Init);
        }
        // Per-frame init/update dispatch on `wasm32` is currently driven by the
        // consumer crates' batched runner via `script_functions(...)`. Phase 4.6
        // will move that loop into the bridge so this method does the call.
        #[allow(unreachable_code)]
        Ok(())
    }

    fn update(
        &mut self,
        handle: ScriptHandle,
        _entity_id: u64,
        _ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        if !self.scripts.contains_key(&handle.0) {
            return Err(unknown_handle());
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
        {
            return host_dispatch(self, handle, _entity_id, _ctx, HostLifecycle::Update);
        }
        #[allow(unreachable_code)]
        Ok(())
    }

    fn named_callback(
        &mut self,
        handle: ScriptHandle,
        _entity_id: u64,
        _fn_name: &str,
        _ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        if !self.scripts.contains_key(&handle.0) {
            return Err(unknown_handle());
        }
        // P7.7 wedge 19 — native host JS adapter resolves the callback by
        // name through the per-script `lookup` function (added in HostScript
        // alongside init/update). Init/update use cached Persistent handles
        // for zero-overhead per-frame dispatch; named callbacks (rare,
        // typically `on_event` / `on_gui_click`-routed handlers) pay one
        // QuickJS function call to resolve. Mirrors the browser-side
        // `lookup_fn` wired in wedge 15 + the LuauAdapter's named_callback
        // path.
        #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
        {
            return host_dispatch(
                self,
                handle,
                _entity_id,
                _ctx,
                HostLifecycle::Named(_fn_name),
            );
        }
        #[allow(unreachable_code)]
        Ok(())
    }

    fn unload(&mut self, handle: ScriptHandle) -> Result<(), ScriptError> {
        self.scripts.remove(&handle.0);
        #[cfg(target_arch = "wasm32")]
        {
            self.fns.remove(&handle.0);
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
        {
            self.host_scripts.remove(&handle.0);
        }
        Ok(())
    }

    /// Phase D — args-bearing named callback. Routes through the existing
    /// `lookup` / `host_dispatch` machinery with a new `NamedWithArgs` arm
    /// that builds a runtime-sized `function::Args`.
    fn named_callback_with_args(
        &mut self,
        handle: ScriptHandle,
        _entity_id: u64,
        _fn_name: &str,
        _args: &[crate::scripting::bridge::value::Value],
        _ctx: &mut BridgeContext<'_>,
    ) -> Result<(), ScriptError> {
        if !self.scripts.contains_key(&handle.0) {
            return Err(unknown_handle());
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
        {
            return host_dispatch(
                self,
                handle,
                _entity_id,
                _ctx,
                HostLifecycle::NamedWithArgs(_fn_name, _args),
            );
        }
        #[allow(unreachable_code)]
        Ok(())
    }

    /// Phase D — payload-bearing event. Mirrors the LuauAdapter
    /// implementation: looks up `on_<event>` and invokes it with
    /// `(entity, data)`.
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

    /// Hot-path override: batch the per-entity `update` calls so the QuickJS
    /// `Context::with` enter happens once for the entire frame instead of
    /// once per entity. At 15k JS-scripted entities this was the dominant
    /// per-call overhead in `run_js` (~3.2 \u00b5s/entity vs Lua's 2.2 \u00b5s).
    /// The browser/wasm path falls through to the default per-entity loop
    /// because there's no shared context to enter \u2014 each function call
    /// dispatches directly through `js_sys::Function::call`.
    #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
    fn update_batch(
        &mut self,
        entries: &[(ScriptHandle, u64)],
        ctx: &mut BridgeContext<'_>,
        errors: &mut Vec<ScriptError>,
    ) {
        if entries.is_empty() {
            return;
        }
        host_update_batch(self, entries, ctx, errors);
    }

    /// Phase D — Trait-level entry point that the runner uses to wire a host-
    /// supplied `require` resolver. Delegates to the inherent
    /// [`Self::set_module_resolver`] which forwards to the QuickJS host (or
    /// stashes the resolver until the host engine is lazily constructed).
    fn set_module_resolver(&mut self, resolver: crate::scripting::bridge::ModuleResolver) {
        JsAdapter::set_module_resolver(self, resolver);
    }

    /// Data-oriented design — forward the `.data` registry to the inherent
    /// [`Self::set_data_registry`].
    fn set_data_registry(&mut self, registry: std::rc::Rc<crate::data::DataRegistry>) {
        JsAdapter::set_data_registry(self, registry);
    }

    /// Phase D — Drop the cached module exports on whichever JS execution
    /// path is active (QuickJS host or browser wrapper). Errors are logged
    /// and swallowed so the asset-watcher caller stays infallible.
    fn invalidate_module_cache(&mut self) {
        #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
        {
            if let Some(host) = self.host.as_ref()
                && let Err(e) = host.invalidate_module_cache()
            {
                log::warn!("[JsAdapter/host] invalidate_module_cache failed: {}", e.message);
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            JsAdapter::invalidate_module_cache(self);
        }
    }

    /// Slice A.JS — install a registry-backed dynamic function. On the host
    /// path (native + `js-adapter-host`) this forwards into the QuickJS
    /// engine if it has been lazily constructed, otherwise the descriptor is
    /// stashed in [`Self::pending_dynamic_fns`] and applied on the next
    /// `load`. On the browser (wasm32) path this currently logs and no-ops:
    /// the browser bridge is not yet routed through [`BridgeContext`]
    /// (tracked by Phase 4.6); script-callable handlers in deployed web
    /// builds need that integration before they can fire.
    fn install_dynamic_function(
        &mut self,
        name: &str,
        descriptor: &crate::package::script_api::ScriptFunctionDescriptor,
    ) {
        let Some(handler) = descriptor.handler.as_ref().cloned() else {
            return;
        };
        #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
        {
            let arg_types: Vec<crate::package::script_api::ScriptValueType> = descriptor
                .args
                .iter()
                .map(|a| a.arg_type.clone())
                .collect();
            if let Some(engine) = self.host.as_ref() {
                if let Err(e) =
                    engine.install_dynamic_function_parts(name, &arg_types, handler)
                {
                    log::warn!(
                        "[JsAdapter/host] install_dynamic_function '{}' failed: {}",
                        name,
                        e.message
                    );
                }
            } else {
                self.pending_dynamic_fns.push(PendingDynamicFn {
                    name: name.to_string(),
                    arg_types,
                    handler,
                });
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = handler;
            log::debug!(
                "[JsAdapter/browser] install_dynamic_function '{}' is a no-op pending Phase 4.6 (browser bridge routed through BridgeContext)",
                name
            );
        }
        #[cfg(all(not(target_arch = "wasm32"), not(feature = "js-adapter-host")))]
        {
            let _ = handler;
            log::trace!(
                "[JsAdapter/native-skeleton] install_dynamic_function '{}' skipped (host engine not compiled in)",
                name
            );
        }
    }
}

fn unknown_handle() -> ScriptError {
    ScriptError {
        backend: BackendKind::JavaScript,
        script_path: String::new(),
        location: None,
        kind: ScriptErrorKind::Runtime,
        message: "unknown script handle".to_string(),
        bridge_call: None,
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
#[derive(Clone, Copy)]
enum HostLifecycle<'a> {
    Init,
    Update,
    /// P7.7 wedge 19 — named callback resolved through the per-script
    /// `lookup` function (e.g. `on_event`, `on_gui_click`-routed handlers).
    Named(&'a str),
    /// Phase D — args-bearing named callback. Receiver sees
    /// `function method(entity, ...args)`.
    NamedWithArgs(&'a str, &'a [crate::scripting::bridge::value::Value]),
}

#[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
fn host_dispatch(
    adapter: &mut JsAdapter,
    handle: ScriptHandle,
    entity_id: u64,
    ctx: &mut BridgeContext<'_>,
    which: HostLifecycle<'_>,
) -> Result<(), ScriptError> {
    let engine = adapter.host.as_ref().ok_or_else(|| ScriptError {
        backend: BackendKind::JavaScript,
        script_path: String::new(),
        location: None,
        kind: ScriptErrorKind::Runtime,
        message: "host JS engine not initialised".into(),
        bridge_call: None,
    })?;
    let host_script = adapter.host_scripts.get(&handle.0).ok_or_else(unknown_handle)?;
    // Hot path: borrow `path` as `&str` so the success path doesn't allocate.
    // `host_script` and `engine` both borrow disjoint fields of `adapter`
    // (host_scripts vs host) so the split-borrow lets them coexist.
    let path: &str = &host_script.path;
    // For Init/Update we hold a cached Persistent. For Named we resolve via
    // the per-script `lookup` function each call — missing callbacks return
    // `Ok(())` (script author chose not to define the handler, same
    // semantics as Init/Update being absent).
    enum Resolved {
        Cached(Persistent<Function<'static>>),
        Lookup(Persistent<Function<'static>>, String),
    }
    let resolved = match which {
        HostLifecycle::Init => host_script.init.clone().map(Resolved::Cached),
        HostLifecycle::Update => host_script.update.clone().map(Resolved::Cached),
        HostLifecycle::Named(name) => host_script
            .lookup
            .clone()
            .map(|lk| Resolved::Lookup(lk, name.to_string())),
        HostLifecycle::NamedWithArgs(name, _) => host_script
            .lookup
            .clone()
            .map(|lk| Resolved::Lookup(lk, name.to_string())),
    };
    let Some(resolved) = resolved else { return Ok(()); };
    let dt = ctx.dt;
    engine.with_state(ctx, |engine| {
        engine.context.with(|qctx| -> Result<(), ScriptError> {
            let func: Function = match resolved {
                Resolved::Cached(p) => p.restore(&qctx).map_err(|e| ScriptError {
                    backend: BackendKind::JavaScript,
                    script_path: path.to_string(),
                    location: None,
                    kind: ScriptErrorKind::Runtime,
                    message: format!("restore: {e}"),
                    bridge_call: None,
                })?,
                Resolved::Lookup(lk, name) => {
                    let lookup_fn = lk.restore(&qctx).map_err(|e| ScriptError {
                        backend: BackendKind::JavaScript,
                        script_path: path.to_string(),
                        location: None,
                        kind: ScriptErrorKind::Runtime,
                        message: format!("restore lookup: {e}"),
                        bridge_call: None,
                    })?;
                    let resolved_val: rquickjs::Value = lookup_fn
                        .call((name.as_str(),))
                        .map_err(|e| ScriptError {
                            backend: BackendKind::JavaScript,
                            script_path: path.to_string(),
                            location: None,
                            kind: ScriptErrorKind::Runtime,
                            message: format!("lookup({name}): {e}"),
                            bridge_call: None,
                        })?;
                    // `lookup` returns `null` for undefined / non-function
                    // names — silently no-op rather than erroring (matches
                    // missing init/update behaviour above).
                    if !resolved_val.is_function() {
                        return Ok(());
                    }
                    Function::from_value(resolved_val).map_err(|e| ScriptError {
                        backend: BackendKind::JavaScript,
                        script_path: path.to_string(),
                        location: None,
                        kind: ScriptErrorKind::Runtime,
                        message: format!("lookup({name}) cast: {e}"),
                        bridge_call: None,
                    })?
                }
            };
            let call_result: rquickjs::Result<()> = match which {
                HostLifecycle::Init => func.call((entity_id as f64,)),
                HostLifecycle::Update => func.call((entity_id as f64, dt as f64)),
                HostLifecycle::Named(_) => func.call((entity_id as f64,)),
                HostLifecycle::NamedWithArgs(_, args) => {
                    // Phase D — marshal bridge `Value`s onto a `function::Args`
                    // builder so we can call with a runtime-determined arity.
                    // v1 only supports nil/bool/number/string/EntityId (matches
                    // the `call(...)` global's intake validator). Other
                    // variants are coerced to `null`.
                    use crate::scripting::bridge::value::Value as V;
                    let mut a = rquickjs::function::Args::new(qctx.clone(), args.len() + 1);
                    a.push_arg(entity_id as f64).map_err(|e| ScriptError {
                        backend: BackendKind::JavaScript,
                        script_path: path.to_string(),
                        location: None,
                        kind: ScriptErrorKind::Runtime,
                        message: format!("push entity_id: {e}"),
                        bridge_call: None,
                    })?;
                    for v in args {
                        let push_res = match v {
                            V::Nil => a.push_arg(rquickjs::Value::new_null(qctx.clone())),
                            V::Bool(b) => a.push_arg(*b),
                            V::Number(n) => a.push_arg(*n),
                            V::String(s) => a.push_arg(s.as_str()),
                            V::EntityId(id) => a.push_arg(*id as f64),
                            _ => a.push_arg(rquickjs::Value::new_null(qctx.clone())),
                        };
                        push_res.map_err(|e| ScriptError {
                            backend: BackendKind::JavaScript,
                            script_path: path.to_string(),
                            location: None,
                            kind: ScriptErrorKind::Runtime,
                            message: format!("push arg: {e}"),
                            bridge_call: None,
                        })?;
                    }
                    func.call_arg(a)
                }
            };
            call_result.map_err(|e| ScriptError {
                backend: BackendKind::JavaScript,
                script_path: path.to_string(),
                location: None,
                kind: ScriptErrorKind::Runtime,
                message: format!("{e}"),
                bridge_call: None,
            })
        })
    })
}

/// Hot-path batch dispatcher for `update`. Mirrors [`host_dispatch`] but
/// folds N per-entity `Context::with` enters into a single one. Errors are
/// collected into the runner's `errors` Vec so a single failing script
/// doesn't break the rest of the frame.
#[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
fn host_update_batch(
    adapter: &mut JsAdapter,
    entries: &[(ScriptHandle, u64)],
    ctx: &mut BridgeContext<'_>,
    errors: &mut Vec<ScriptError>,
) {
    let Some(engine) = adapter.host.as_ref() else {
        errors.push(ScriptError {
            backend: BackendKind::JavaScript,
            script_path: String::new(),
            location: None,
            kind: ScriptErrorKind::Runtime,
            message: "host JS engine not initialised".into(),
            bridge_call: None,
        });
        return;
    };
    let dt = ctx.dt;
    engine.with_state(ctx, |engine| -> Result<(), ScriptError> {
        engine.context.with(|qctx| {
            // Hot path: cache the per-handle restored Function so 15k
            // entities sharing the same script don't each pay
            // `Persistent::restore` (~hundreds of ns x N entities). Only
            // ~5-20 distinct script handles in practice; a Vec-as-map
            // (linear scan) wins over HashMap for that size.
            let mut restored: Vec<(u64, Function, String)> = Vec::with_capacity(8);
            for &(handle, entity_id) in entries {
                // Find or insert the restored function for this handle.
                let cached_idx = restored.iter().position(|(h, _, _)| *h == handle.0);
                let idx = if let Some(i) = cached_idx {
                    i
                } else {
                    let Some(host_script) = adapter.host_scripts.get(&handle.0) else {
                        errors.push(unknown_handle());
                        continue;
                    };
                    let Some(update_persistent) = host_script.update.as_ref() else {
                        // Script has no update; insert nothing and skip.
                        continue;
                    };
                    let path = host_script.path.clone();
                    match update_persistent.clone().restore(&qctx) {
                        Ok(f) => {
                            restored.push((handle.0, f, path));
                            restored.len() - 1
                        }
                        Err(e) => {
                            errors.push(ScriptError {
                                backend: BackendKind::JavaScript,
                                script_path: path,
                                location: None,
                                kind: ScriptErrorKind::Runtime,
                                message: format!("restore: {e}"),
                                bridge_call: None,
                            });
                            continue;
                        }
                    }
                };
                let (_, func, path) = &restored[idx];
                if let Err(e) = func.call::<_, ()>((entity_id as f64, dt as f64)) {
                    errors.push(ScriptError {
                        backend: BackendKind::JavaScript,
                        script_path: path.clone(),
                        location: None,
                        kind: ScriptErrorKind::Runtime,
                        message: format!("{e}"),
                        bridge_call: None,
                    });
                }
            }
        });
        Ok(())
    })
    .ok();
}

#[cfg(test)]
mod tests {
    use super::super::super::capability::Capabilities;
    use super::super::super::command::BridgeCommand;
    use super::super::super::context::BridgeReadSnapshot;
    use super::*;

    #[test]
    fn skeleton_lifecycle_compiles_and_runs() {
        let mut a = JsAdapter::new();
        let h = a.load("test.luau", "function update(entity, dt) end").unwrap();
        let read = BridgeReadSnapshot::default();
        let mut cmds: Vec<BridgeCommand> = Vec::new();
        let mut ctx = BridgeContext::new(0.016, &read, Capabilities::empty(), &mut cmds);
        a.init(h, 1, &mut ctx).unwrap();
        a.update(h, 1, &mut ctx).unwrap();
        a.unload(h).unwrap();
    }

    #[test]
    fn unknown_handle_errors() {
        let mut a = JsAdapter::new();
        let read = BridgeReadSnapshot::default();
        let mut cmds: Vec<BridgeCommand> = Vec::new();
        let mut ctx = BridgeContext::new(0.016, &read, Capabilities::empty(), &mut cmds);
        let bogus = ScriptHandle(999);
        let err = a.update(bogus, 1, &mut ctx).unwrap_err();
        assert_eq!(err.backend, BackendKind::JavaScript);
        assert_eq!(err.kind, ScriptErrorKind::Runtime);
    }

    /// P7.7 wedge 19 — host JS adapter can resolve and dispatch arbitrary
    /// named callbacks via the `lookup` field on `HostScript`. Previously
    /// `named_callback` was a no-op stub; this test exercises both the
    /// "callback exists and runs" path and the "callback missing → silent
    /// no-op" path (matching the missing-init/missing-update semantics).
    #[cfg(all(not(target_arch = "wasm32"), feature = "js-adapter-host"))]
    #[test]
    fn named_callback_resolves_via_lookup() {
        // Use a side-effect a Rust caller can observe: emit a debug log
        // command from the callback through `print_log`, which the bridge
        // translates into a `BridgeCommand::Log`.
        let mut a = JsAdapter::new();
        // Source is written in Luau syntax — JsAdapter::load runs it through
        // the in-tree transpiler before handing it to QuickJS, so braces
        // would be misread as Lua table literals. `log()` is bridged to a
        // BridgeCommand::Log that the test can observe.
        // NOTE: extension must NOT be `.js` — `.js` paths are now passed
        // through to QuickJS verbatim and the Luau-style `function ... end`
        // syntax would fail to parse.
        let src = "function on_resume(entity)\n  log('on_resume')\nend\n";
        let h = a.load("test_named.luau", src).unwrap();
        let read = BridgeReadSnapshot::default();
        let mut cmds: Vec<BridgeCommand> = Vec::new();
        {
            let mut ctx = BridgeContext::new(0.016, &read, Capabilities::empty(), &mut cmds);
            a.named_callback(h, 42, "on_resume", &mut ctx).unwrap();
        }
        let logged = cmds.iter().any(|c| matches!(c, BridgeCommand::Log { message } if message == "on_resume"));
        assert!(logged, "expected on_resume to run and emit log; got {cmds:?}");

        // Missing callback → silent no-op (no error, no commands added).
        let before = cmds.len();
        {
            let mut ctx = BridgeContext::new(0.016, &read, Capabilities::empty(), &mut cmds);
            a.named_callback(h, 42, "no_such_handler", &mut ctx).unwrap();
        }
        assert_eq!(cmds.len(), before, "missing handler should not emit commands");
        a.unload(h).unwrap();
    }
}
