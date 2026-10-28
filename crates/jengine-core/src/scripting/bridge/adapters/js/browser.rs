//! Browser JS execution layer for [`super::JsAdapter`] — `wasm32` only.
//!
//! Migrated in Phase 4.4 from `crates/jengine-web/src/lib.rs::load_browser_script`.
//! Owns:
//! - The sandbox wrapper template (`window.__jengine_play.*` script-facing API).
//! - The `js_sys::eval` compile path that materialises a script's
//!   `(init, update, on_event)` JS functions.
//!
//! The web/editor-viewport runtimes consume [`BrowserScriptFns`] handles via
//! [`super::JsAdapter::script_functions`] / [`super::JsAdapter::clone_script_functions`]
//! and dispatch them through their existing batched per-frame runner. Phase 4.6
//! will route those same handles through [`crate::scripting::bridge`]'s
//! [`BridgeContext`](super::super::super::context::BridgeContext) for native↔web parity.

use std::sync::OnceLock;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use super::super::super::diagnostics::{BackendKind, ScriptError, ScriptErrorKind};

/// The JS function values produced by compiling a browser script, plus the
/// metadata needed for source-mapped diagnostics.
///
/// `JsValue` is cheap to clone (it's a refcounted handle into the JS heap), so
/// callers can stash copies into per-entity records (prefab caches, etc).
///
/// `prefix_lines` records how many lines the wrapper template inserts BEFORE
/// the user's source — needed so [`format_script_error`] can subtract the
/// wrapper offset and report user-source line numbers.
#[derive(Clone)]
pub struct BrowserScriptFns {
    pub init: JsValue,
    pub update: JsValue,
    pub on_event: JsValue,
    pub on_gui_click: JsValue,
    /// Per-handler callback resolver — `lookup(name)` returns the named
    /// function from the script's IIFE scope (or `null`). Used by the GUI
    /// click dispatcher to honour the `callback_name` argument passed to
    /// `gui_on_click(entity, gui, node, "on_resume")`. P7.7 wedge 15.
    pub lookup: JsValue,
    /// Like `lookup` but returns **any** non-null value, including namespace
    /// objects (e.g. `event`, `animation`). Only used for the Fengari
    /// `__api_lookup` stub so the Lua bridge can populate namespace tables.
    pub ns_lookup: JsValue,
    pub prefix_lines: usize,
}

/// Compile a transpiled-JS script source into [`BrowserScriptFns`].
///
/// The `path` is only used to attribute diagnostics. `js_source` must already be
/// JavaScript (Lua callers should run [`super::transpile_lua_to_js`] first; the
/// adapter does this automatically in [`super::JsAdapter::load`]).
pub(super) fn compile(path: &str, js_source: &str) -> Result<BrowserScriptFns, ScriptError> {
    let wrapper = build_wrapper(js_source);
    let evaled = js_sys::eval(&wrapper).map_err(|e| eval_error(path, &e))?;
    let obj: js_sys::Object = evaled.dyn_into().map_err(|_| ScriptError {
        backend: BackendKind::JavaScript,
        script_path: path.to_string(),
        location: None,
        kind: ScriptErrorKind::Runtime,
        message: "browser script wrapper did not return an object".to_string(),
        bridge_call: None,
    })?;
    let init = js_sys::Reflect::get(&obj, &"init".into()).map_err(|e| eval_error(path, &e))?;
    let update = js_sys::Reflect::get(&obj, &"update".into()).map_err(|e| eval_error(path, &e))?;
    let on_event =
        js_sys::Reflect::get(&obj, &"on_event".into()).map_err(|e| eval_error(path, &e))?;
    let on_gui_click = js_sys::Reflect::get(&obj, &"on_gui_click".into())
        .map_err(|e| eval_error(path, &e))?;
    let lookup =
        js_sys::Reflect::get(&obj, &"lookup".into()).map_err(|e| eval_error(path, &e))?;
    let ns_lookup =
        js_sys::Reflect::get(&obj, &"ns_lookup".into()).map_err(|e| eval_error(path, &e))?;
    Ok(BrowserScriptFns {
        init,
        update,
        on_event,
        on_gui_click,
        lookup,
        ns_lookup,
        prefix_lines: wrapper_prefix_lines(),
    })
}

/// Number of lines the wrapper inserts before the user source. Computed once
/// per process by inserting a unique marker into the template and locating it.
fn wrapper_prefix_lines() -> usize {
    static N: OnceLock<usize> = OnceLock::new();
    *N.get_or_init(|| {
        const MARKER: &str = "\x00JENGINE_PREFIX_MARKER\x00";
        let probe = build_wrapper(MARKER);
        let pos = probe.find(MARKER).unwrap_or(0);
        probe[..pos].chars().filter(|c| *c == '\n').count()
    })
}

// ---------------------------------------------------------------------------
// Phase D — browser-side `require()` parity
// ---------------------------------------------------------------------------

/// Install (or replace) the browser-side `require()` global, backed by the
/// supplied resolver. Mirrors `host.rs::install_require_globals`:
///
/// - Stores a wasm-bindgen [`Closure`] on `window.__jengine_play.__resolve_module`
///   that calls the Rust resolver and transpiles Lua → JS when the path's
///   extension warrants it.
/// - Idempotently installs a `globalThis.require` polyfill that consults a
///   process-wide module cache (`window.__jengine_play.__module_cache__`)
///   before falling through to the resolver.
///
/// The returned [`Closure`] MUST be kept alive for as long as the resolver
/// should be callable from JS — drop it (or replace it via another call) to
/// uninstall. [`super::JsAdapter`] stashes it in a field for that reason.
pub(super) fn install_require_globals(
    resolver: super::super::super::module::ModuleResolver,
) -> Result<wasm_bindgen::closure::Closure<dyn FnMut(String) -> JsValue>, ScriptError> {
    use wasm_bindgen::closure::Closure;

    // Closure: `(path) -> [ok: bool, payload: string]`. Same shape as the
    // host build's `__resolve_module__` so any future browser-side eval
    // wrapper can mirror the host's `require` body verbatim.
    let resolver_for_closure = resolver;
    let cb = Closure::wrap(Box::new(move |path: String| -> JsValue {
        let arr = js_sys::Array::new();
        match (resolver_for_closure)(&path) {
            Ok(source) => {
                let js = if path_is_lua(&path) {
                    super::transpiler::transpile_lua_to_js(&source)
                } else {
                    source
                };
                arr.push(&JsValue::from_bool(true));
                arr.push(&JsValue::from_str(&js));
            }
            Err(e) => {
                arr.push(&JsValue::from_bool(false));
                arr.push(&JsValue::from_str(&e));
            }
        }
        arr.into()
    }) as Box<dyn FnMut(String) -> JsValue>);

    let window = web_sys::window().ok_or_else(|| missing_window_err())?;
    let play = ensure_play_namespace(&window)?;
    js_sys::Reflect::set(
        &play,
        &JsValue::from_str("__resolve_module__"),
        cb.as_ref().unchecked_ref(),
    )
    .map_err(|e| install_err("__resolve_module__", &e))?;

    // Idempotent install of the JS-side `require` + module caches. Bound to
    // `globalThis.require` (matches host.rs) — script authors can call
    // `require("lib/utils")` from any IIFE without extra setup.
    let install_src = r#"
        (function() {
            var play = window.__jengine_play;
            if (!play.__module_cache__) play.__module_cache__ = {};
            if (!play.__module_loading__) play.__module_loading__ = {};
            globalThis.require = function(path) {
                var cache = play.__module_cache__;
                var loading = play.__module_loading__;
                if (Object.prototype.hasOwnProperty.call(cache, path)) return cache[path];
                if (loading[path]) {
                    throw new Error("require('" + path + "'): circular dependency detected");
                }
                if (typeof play.__resolve_module__ !== 'function') {
                    throw new Error("require('" + path + "'): no module resolver installed");
                }
                var r = play.__resolve_module__(path);
                if (!r[0]) throw new Error(r[1]);
                var body = r[1];
                loading[path] = true;
                try {
                    var mod = eval("(function() {\n" + body + "\n})()");
                    cache[path] = mod;
                    return mod;
                } finally {
                    delete loading[path];
                }
            };
        })();
    "#;
    js_sys::eval(install_src).map_err(|e| install_err("globalThis.require", &e))?;

    Ok(cb)
}

/// Drop every cached browser-side module export. Next `require(name)` re-runs
/// the module body via the installed resolver. Mirrors
/// `HostJs::invalidate_module_cache`.
pub(super) fn invalidate_module_cache() -> Result<(), ScriptError> {
    let _ = js_sys::eval(
        "if (window.__jengine_play) { \
           window.__jengine_play.__module_cache__ = {}; \
           window.__jengine_play.__module_loading__ = {}; \
         }",
    )
    .map_err(|e| install_err("invalidate_module_cache", &e))?;
    Ok(())
}

fn path_is_lua(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".lua") || lower.ends_with(".luau") {
        return true;
    }
    if lower.ends_with(".js") || lower.ends_with(".mjs") {
        return false;
    }
    // No / unknown extension: default to Lua so `require("lib/math")` works.
    true
}

fn ensure_play_namespace(window: &web_sys::Window) -> Result<js_sys::Object, ScriptError> {
    let play = js_sys::Reflect::get(window, &JsValue::from_str("__jengine_play"))
        .map_err(|e| install_err("window.__jengine_play", &e))?;
    if play.is_object() {
        Ok(play.unchecked_into())
    } else {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(window, &JsValue::from_str("__jengine_play"), &obj)
            .map_err(|e| install_err("window.__jengine_play", &e))?;
        Ok(obj)
    }
}

fn missing_window_err() -> ScriptError {
    ScriptError {
        backend: BackendKind::JavaScript,
        script_path: String::new(),
        location: None,
        kind: ScriptErrorKind::Runtime,
        message: "no window global available; require() needs a browser context"
            .to_string(),
        bridge_call: None,
    }
}

fn install_err(what: &str, err: &JsValue) -> ScriptError {
    let detail = err
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(err, &JsValue::from_str("message"))
                .ok()
                .and_then(|m| m.as_string())
        })
        .unwrap_or_else(|| format!("{err:?}"));
    ScriptError {
        backend: BackendKind::JavaScript,
        script_path: String::new(),
        location: None,
        kind: ScriptErrorKind::Runtime,
        message: format!("install {what}: {detail}"),
        bridge_call: None,
    }
}

/// Build a human-readable label for an entity+script,
/// e.g. `"'Player' (entity 4) [player.lua]"`.
///
/// Used by both `jengine-web` and `jengine-editor-viewport` runtimes when
/// formatting script error messages — lifted to core in P7.7 wedge 13 to
/// stop the duplicate copies drifting.
pub fn format_entity_label(entity_index: usize, entity_name: &str, source_path: &str) -> String {
    let name_part = if entity_name.is_empty() {
        format!("entity {}", entity_index)
    } else {
        format!("'{}' (entity {})", entity_name, entity_index)
    };
    if source_path.is_empty() {
        name_part
    } else {
        format!("{} [{}]", name_part, source_path)
    }
}

/// Format a script error from a `JsValue` into a readable message with line number.
///
/// `prefix_lines` is the number of lines in the eval wrapper before the user code,
/// used to adjust raw eval line numbers to the user's source line. For scripts
/// loaded via [`compile`], pass [`BrowserScriptFns::prefix_lines`].
///
/// `script_name` is the path/identifier the wrapper used in its `//# sourceURL`
/// directive (typically the source file path); used to recognise stack-trace
/// lines that name this script.
///
/// Lifted from `jengine-editor-viewport` in P7.7 wedge 13.
pub fn format_script_error(e: &JsValue, prefix_lines: usize, script_name: &str) -> String {
    if let Some(err) = e.dyn_ref::<js_sys::Error>() {
        let message = err.message();
        let msg_str: String = message.into();

        let stack = js_sys::Reflect::get(err, &"stack".into())
            .ok()
            .and_then(|v| v.as_string());

        if let Some(stack_str) = stack {
            let source_marker = format!("{}:", script_name);
            let markers: Vec<&str> = vec![&source_marker, "<anonymous>:", "eval:", "Function:"];

            for stack_line in stack_str.lines() {
                for marker in &markers {
                    if let Some(pos) = stack_line.find(marker) {
                        let after = &stack_line[pos + marker.len()..];
                        if let Some(colon) = after.find(':') {
                            if let Ok(raw_line) = after[..colon].parse::<usize>() {
                                let user_line = raw_line.saturating_sub(prefix_lines);
                                if user_line > 0 {
                                    return format!("line {}: {}", user_line, msg_str);
                                }
                            }
                        }
                    }
                }
            }
        }

        for prop in &["lineNumber", "line"] {
            if let Ok(val) = js_sys::Reflect::get(err, &JsValue::from_str(prop)) {
                if let Some(raw_line) = val.as_f64() {
                    let raw_line = raw_line as usize;
                    let user_line = raw_line.saturating_sub(prefix_lines);
                    if user_line > 0 {
                        return format!("line {}: {}", user_line, msg_str);
                    }
                }
            }
        }

        return msg_str;
    }

    // Fallback: try plain JS string / `.message` lookup before stringifying.
    if let Some(s) = e.as_string() {
        return s;
    }
    if let Ok(m) = js_sys::Reflect::get(e, &"message".into()) {
        if let Some(s) = m.as_string() {
            return s;
        }
    }
    format!("{:?}", e)
}

fn eval_error(path: &str, err: &JsValue) -> ScriptError {
    let msg = err
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(err, &"message".into())
                .ok()
                .and_then(|m| m.as_string())
        })
        .unwrap_or_else(|| format!("{err:?}"));
    ScriptError {
        backend: BackendKind::JavaScript,
        script_path: path.to_string(),
        location: None,
        kind: ScriptErrorKind::Syntax,
        message: msg,
        bridge_call: None,
    }
}

/// Wrap the user's transpiled JS source with the script-facing API and namespace
/// objects. The wrapper returns `{ init, update, on_event }` for the adapter to
/// extract via `Reflect::get`.
fn build_wrapper(source: &str) -> String {
    format!(
        r#"
(function() {{
  // Lua string method polyfills (idempotent)
  if (!String.prototype.__jengine_lua_polyfilled) {{
    String.prototype.__jengine_lua_polyfilled = true;
    String.prototype.upper = function() {{ return this.toUpperCase(); }};
    String.prototype.lower = function() {{ return this.toLowerCase(); }};
    String.prototype.len = function() {{ return this.length; }};
    String.prototype.rep = function(n, sep) {{
      if (sep === undefined) return this.repeat(Math.max(0, n|0));
      var parts = []; for (var i = 0; i < n; i++) parts.push(String(this));
      return parts.join(sep);
    }};
    String.prototype.reverse = function() {{ return this.split('').reverse().join(''); }};
    String.prototype.sub = function(i, j) {{
      var len = this.length;
      if (i === undefined) i = 1;
      if (j === undefined) j = len;
      if (i < 0) i = Math.max(len + i + 1, 1);
      if (j < 0) j = len + j + 1;
      if (i < 1) i = 1;
      if (j > len) j = len;
      if (i > j) return '';
      return this.substring(i - 1, j);
    }};
    String.prototype.byte = function(i) {{
      i = (i === undefined) ? 1 : i;
      if (i < 0) i = this.length + i + 1;
      return this.charCodeAt(i - 1);
    }};
    String.prototype.find = function(pat, init) {{
      init = (init === undefined) ? 1 : init;
      if (init < 0) init = Math.max(this.length + init + 1, 1);
      var idx = this.indexOf(pat, init - 1);
      if (idx < 0) return null;
      return [idx + 1, idx + pat.length];
    }};
  }}
  // Lua-style `string` library (minimal — covers what scripts actually use).
  // Bound on globalThis so transpiled `string.format(...)` calls resolve.
  if (typeof globalThis.string === 'undefined') {{
    globalThis.string = {{
      format: function(fmt) {{
        var args = Array.prototype.slice.call(arguments, 1);
        var ai = 0;
        return String(fmt).replace(
          /%%|%([-+0 #]*)(\d+)?(?:\.(\d+))?([dioxXfgeEcsq])/g,
          function(m, flags, width, prec, kind) {{
            if (m === '%%') return '%';
            var v = args[ai++];
            flags = flags || '';
            width = width ? parseInt(width, 10) : 0;
            prec = (prec !== undefined) ? parseInt(prec, 10) : -1;
            var s;
            switch (kind) {{
              case 'd': case 'i':
                s = (parseInt(v, 10) || 0).toString();
                if (flags.indexOf('+') >= 0 && +v >= 0) s = '+' + s;
                break;
              case 'o': s = (parseInt(v, 10) || 0).toString(8); break;
              case 'x': s = (parseInt(v, 10) || 0).toString(16); break;
              case 'X': s = (parseInt(v, 10) || 0).toString(16).toUpperCase(); break;
              case 'f': case 'g': case 'e': case 'E': {{
                var n = Number(v) || 0;
                if (prec >= 0) {{
                  if (kind === 'e' || kind === 'E') s = n.toExponential(prec);
                  else if (kind === 'g') s = n.toPrecision(prec || 1);
                  else s = n.toFixed(prec);
                }} else {{
                  s = (kind === 'f') ? n.toFixed(6) : String(n);
                }}
                if (kind === 'E') s = s.toUpperCase();
                if (flags.indexOf('+') >= 0 && n >= 0) s = '+' + s;
                break;
              }}
              case 'c':
                s = String.fromCharCode(parseInt(v, 10) || 0);
                break;
              case 'q':
                s = '"' + String(v).replace(/[\\"\n\r\0]/g, function(ch) {{
                  if (ch === '\n') return '\\n';
                  if (ch === '\r') return '\\r';
                  if (ch === '\0') return '\\0';
                  return '\\' + ch;
                }}) + '"';
                break;
              case 's':
              default:
                s = (v === undefined || v === null) ? '' : String(v);
                if (prec >= 0) s = s.substring(0, prec);
                break;
            }}
            if (width > s.length) {{
              var pad = (flags.indexOf('-') < 0 && flags.indexOf('0') >= 0
                && (kind === 'd' || kind === 'i' || kind === 'f'
                    || kind === 'x' || kind === 'X' || kind === 'o'))
                ? '0' : ' ';
              var fill = new Array(width - s.length + 1).join(pad);
              s = (flags.indexOf('-') >= 0) ? s + fill : fill + s;
            }}
            return s;
          }}
        );
      }},
      upper: function(s) {{ return String(s).toUpperCase(); }},
      lower: function(s) {{ return String(s).toLowerCase(); }},
      len:   function(s) {{ return String(s).length; }},
      rep:   function(s, n, sep) {{ return String(s).rep(n, sep); }},
      reverse: function(s) {{ return String(s).reverse(); }},
      sub:   function(s, i, j) {{ return String(s).sub(i, j); }},
      byte:  function(s, i) {{ return String(s).byte(i); }},
      char:  function() {{
        var out = '';
        for (var i = 0; i < arguments.length; i++) out += String.fromCharCode(arguments[i]);
        return out;
      }},
      find:  function(s, pat, init) {{ return String(s).find(pat, init); }},
    }};
  }}
  // Lua-style 1-indexed array proxy. Used to wrap arrays returned by
  // script-facing APIs like `get_entities_by_tag` so that `t[1]` (Lua
  // convention) reads the first element. The underlying array is
  // 0-indexed so `for (var i = 0; ...)` iteration would skip element 0;
  // the matching ipairs transpilation now uses 1-based indexing too,
  // so both styles end up reading via `proxy[i]` for i in 1..length.
  // `length` and other props pass through unchanged.
  function __lua_arr(a) {{
    if (!a || typeof a !== 'object' || a.__lua_arr) return a;
    var p = new Proxy(a, {{
      get: function(t, k) {{
        if (k === '__lua_arr') return true;
        if (typeof k === 'string' && /^-?\d+$/.test(k)) {{
          var i = parseInt(k, 10);
          if (i >= 1) return t[i - 1];
          return undefined;
        }}
        return t[k];
      }},
      set: function(t, k, v) {{
        if (typeof k === 'string' && /^-?\d+$/.test(k)) {{
          var i = parseInt(k, 10);
          if (i >= 1) {{ t[i - 1] = v; return true; }}
          return true;
        }}
        t[k] = v; return true;
      }},
      has: function(t, k) {{
        if (typeof k === 'string' && /^-?\d+$/.test(k)) {{
          var i = parseInt(k, 10);
          return i >= 1 && i <= t.length;
        }}
        return k in t;
      }}
    }});
    return p;
  }}
  function get_ui_prop(entity, property) {{
    // Hosts hydrate `ui_props` as entity -> {{ property: value }} for every
    // entity carrying a UI widget. Absent reaches Lua as nil, so "no such
    // widget" stays distinguishable from a real false or 0.
    var p = (window.__jengine_play.ui_props || {{}})[entity];
    if (!p) return null;
    var v = p[property];
    return v === undefined ? null : v;
  }}
  function set_ui_prop(entity, property, value) {{
    window.__jengine_play.commands.push({{ t: 'uip', e: entity, p: property, v: value }});
  }}
  function get_health(entity) {{
    // Hosts hydrate `healths` as [current, max]; absent means the entity has
    // no Health, which reaches Lua as nil rather than 0 so a caller can tell
    // "no health component" from "dead".
    var h = (window.__jengine_play.healths || {{}})[entity];
    return h ? [h[0], h[1]] : [null, null];
  }}
  // Transform access goes through flat Float32Arrays indexed by entity id,
  // not per-entity JS objects. The host fills them with one bulk copy per
  // frame; building 8,000 little arrays and 8,000 string keys instead cost
  // 31 ms a frame, for entities scripts never read. See BUGS.md #30.
  //
  // `__ent` resolves a script-side handle to a real entity id. Entities a
  // script spawned are addressed by an allocated index rather than an
  // `Entity::id()`, so the host publishes a small `alias` map for those.
  function __ent(entity) {{
    var a = window.__jengine_play.alias;
    if (a) {{ var r = a[entity]; if (r !== undefined) return r; }}
    return entity;
  }}
  // Writes are recorded rather than scanned for: the host applies exactly the
  // entities that were written, instead of walking every Transform in the
  // world looking for changes.
  function __dirty(list, id) {{
    var d = window.__jengine_play[list];
    if (d) d.push(id);
  }}
  // A slot the host did not fill this frame holds NaN: the entity does not
  // exist (despawned, or spawned and not created yet), and the read is nil,
  // as it is natively. It used to be (0, 0), or the last position of the
  // entity that had the id - so a script asking "is it still there?" was
  // told yes forever (scene 18).
  function get_position(entity) {{
    var id = __ent(entity), p = window.__jengine_play.positions;
    if (!p || id * 2 + 1 >= p.length || p[id * 2] !== p[id * 2]) return null;
    return [p[id * 2], p[id * 2 + 1]];
  }}
  // Writes to an entity the mirror has no slot for are sent as a command
  // instead of being dropped.
  //
  // `spawn_entity` / `spawn_prefab` allocate their handle here in JS and return
  // it immediately, but the entity is not created until the host drains the
  // command queue, after this script returns. So the most natural thing a
  // script can do -- spawn something and place it -- wrote past the end of the
  // mirror and was silently discarded. Commands drain in order, so by the time
  // the host reaches this one the spawn has happened and the handle resolves.
  //
  // Only on the slow path: an ordinary entity still takes the typed-array
  // write, which is what makes the mirror worth having.
  // Handles `spawn_entity` / `spawn_prefab` returned whose entity the host
  // has not created yet. A write to one must go through the command queue,
  // after the spawn: the typed arrays grow ahead of use, so once enough has
  // been spawned a new handle already fits inside them, the write landed in
  // a slot the host applied before the spawn existed, and the move was lost -
  // scene 87's prefabs all appeared in the middle of the screen (BUGS #161).
  // Settled lazily: a handle the host has published an alias for is real.
  function __pending(entity) {{
    var play = window.__jengine_play, ps = play.pending_spawn;
    if (!ps || !ps[entity]) return false;
    var a = play.alias;
    if (a && a[entity] !== undefined) {{ delete ps[entity]; return false; }}
    return true;
  }}
  function __note_spawn(idx) {{
    var play = window.__jengine_play;
    (play.pending_spawn = play.pending_spawn || {{}})[idx] = true;
  }}
  function __defer(kind, entity, a, b, c, d) {{
    window.__jengine_play.commands.push(
      {{ t: 'dtw', k: kind, e: entity, a: a, b: b, c: c, d: d }});
  }}
  function set_position(entity, x, y) {{
    var id = __ent(entity), p = window.__jengine_play.positions;
    if (!p || id * 2 + 1 >= p.length || __pending(entity)) {{ __defer('p', entity, x, y); return; }}
    p[id * 2] = x; p[id * 2 + 1] = y;
    __dirty('dirty_positions', id);
  }}
  function set_rotation(entity, r) {{
    var id = __ent(entity), a = window.__jengine_play.rotations;
    if (!a || id >= a.length || __pending(entity)) {{ __defer('r', entity, r); return; }}
    a[id] = r;
    __dirty('dirty_rotations', id);
  }}
  function set_scale(entity, x, y) {{
    var id = __ent(entity), s = window.__jengine_play.scales;
    if (!s || id * 2 + 1 >= s.length || __pending(entity)) {{ __defer('s', entity, x, y); return; }}
    s[id * 2] = x; s[id * 2 + 1] = y;
    __dirty('dirty_scales', id);
  }}
  function log(msg) {{
    window.__jengine_play.commands.push({{ t: 'log', m: String(msg) }});
  }}
  function get_rotation(entity) {{
    var id = __ent(entity), a = window.__jengine_play.rotations;
    if (!a || id >= a.length || a[id] !== a[id]) return null;
    return a[id];
  }}
  function get_scale(entity) {{
    var id = __ent(entity), s = window.__jengine_play.scales;
    if (!s || id * 2 + 1 >= s.length || s[id * 2] !== s[id * 2]) return null;
    return [s[id * 2], s[id * 2 + 1]];
  }}
  function get_sprite_color(entity) {{
    var id = __ent(entity), c = window.__jengine_play.sprite_colors;
    if (!c || id * 4 + 3 >= c.length) return [1, 1, 1, 1];
    return [c[id * 4], c[id * 4 + 1], c[id * 4 + 2], c[id * 4 + 3]];
  }}
  function set_sprite_color(entity, r, g, b, a) {{
    var id = __ent(entity), c = window.__jengine_play.sprite_colors;
    if (!c || id * 4 + 3 >= c.length || __pending(entity)) {{ __defer('c', entity, r, g, b, a); return; }}
    c[id * 4] = r; c[id * 4 + 1] = g; c[id * 4 + 2] = b; c[id * 4 + 3] = a;
    __dirty('dirty_colors', id);
  }}
  function get_entity_by_name(name) {{
    var idx = window.__jengine_play.names[name];
    return idx !== undefined ? idx : -1;
  }}
  // `Entity.find_child(parent, "Wheel")`: a part of a prefab tree, read
  // from `kids` (publish::publish_kids) - its children first, then theirs,
  // matched by the name after the last '/'; a path is walked a segment at
  // a time. Same rule as ecs::hierarchy::find_child.
  function find_child(parent, path) {{
    var p = window.__jengine_play;
    var kids = p.kids || {{}};
    var at = parent;
    var segs = String(path).split('/');
    for (var i = 0; i < segs.length; i++) {{
      if (segs[i] === '') continue;
      var level = [at];
      var found = null;
      while (found === null && level.length > 0) {{
        var next = [];
        for (var j = 0; j < level.length && found === null; j++) {{
          var list = kids[level[j]] || [];
          for (var k = 0; k < list.length; k += 2) {{
            next.push(list[k]);
            if (found === null && list[k + 1] === segs[i]) found = list[k];
          }}
        }}
        level = next;
      }}
      if (found === null) return null;
      at = found;
    }}
    return at === parent ? null : at;
  }}
  function get_name(entity) {{
    var names = window.__jengine_play.names || {{}};
    for (var k in names) {{ if (names[k] === entity) return k; }}
    return "";
  }}
  function get_delta_time() {{
    return window.__jengine_play.dt || 0;
  }}
  function is_key_pressed(key) {{
    return !!window.__jengine_play.pressed_keys[key];
  }}
  function is_key_just_pressed(key) {{
    return !!window.__jengine_play.just_pressed_keys[key];
  }}
  function get_mouse_position() {{
    var p = window.__jengine_play;
    return [p.mouse_x || 0, p.mouse_y || 0];
  }}
  function get_screen_size() {{
    var p = window.__jengine_play;
    return [p.screen_w || 0, p.screen_h || 0];
  }}
  // Where the camera is looking - the rendered centre, following and all.
  function get_camera_position() {{
    var p = window.__jengine_play;
    return [p.camera_x || 0, p.camera_y || 0];
  }}
  function set_camera_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'scmf', e: entity, f: field, v: value }}); }}
  function set_camera_follow(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'scfl', e: entity, f: field, v: value }}); }}
  function set_camera_screen_rect(entity, x, y, w, h) {{ window.__jengine_play.commands.push({{ t: 'scsr', e: entity, x: x || 0, y: y || 0, w: w || 0, h: h || 0 }}); }}
  function get_camera_zoom() {{
    var p = window.__jengine_play;
    return (typeof p.camera_zoom === 'number' && p.camera_zoom > 0) ? p.camera_zoom : 1.0;
  }}
  // Every finger, as native's `get_touches`: screen point, where it landed,
  // the world point under it (by `get_mouse_world_position`'s arithmetic)
  // and its phase. The host publishes the list (`input_mirror::publish`).
  function get_touches() {{
    var list = window.__jengine_play.touches || [];
    var out = [];
    for (var i = 0; i < list.length; i++) {{
      var t = list[i];
      var w = __screen_to_world(t.x, t.y);
      out.push({{ id: t.id, x: t.x, y: t.y, start_x: t.sx, start_y: t.sy, wx: w[0], wy: w[1],
                  phase: t.phase, on_control: !!t.c }});
    }}
    return out;
  }}
  // This frame's gestures, as native's `get_gestures`: the world point
  // under each and a pan's move in world units (`input_mirror::publish`).
  function get_gestures() {{
    var list = window.__jengine_play.gestures || [];
    var out = [];
    for (var i = 0; i < list.length; i++) {{
      var g = list[i];
      var w = __screen_to_world(g.x, g.y);
      var e = __screen_to_world(g.x + g.dx, g.y + g.dy);
      out.push({{ kind: g.kind, x: g.x, y: g.y, wx: w[0], wy: w[1], dx: g.dx, dy: g.dy,
                  wdx: e[0] - w[0], wdy: e[1] - w[1], scale: g.scale, angle: g.angle,
                  speed: g.speed, phase: g.phase }});
    }}
    return out;
  }}
  function get_mouse_world_position() {{
    var p = window.__jengine_play;
    return __screen_to_world(p.mouse_x || 0, p.mouse_y || 0);
  }}
  function __screen_to_world(mx, my) {{
    var p = window.__jengine_play;
    var sw = p.screen_w || 1;
    var sh = p.screen_h || 1;
    var vw = (typeof p.camera_vw === 'number' && p.camera_vw > 0) ? p.camera_vw : sw;
    var vh = (typeof p.camera_vh === 'number' && p.camera_vh > 0) ? p.camera_vh : sh;
    var zoom = (typeof p.camera_zoom === 'number' && p.camera_zoom > 0) ? p.camera_zoom : 1.0;
    var cx = p.camera_x || 0;
    var cy = p.camera_y || 0;
    // Normalised screen offset from centre (-0.5..+0.5), then scale by
    // the camera's visible world extents (viewport / zoom). Y is flipped
    // because browser pixel-Y grows downward but world-Y is up.
    var nx = (mx - sw * 0.5) / sw;
    var ny = (sh * 0.5 - my) / sh;
    var ox = nx * (vw / zoom);
    var oy = ny * (vh / zoom);
    // Turned by the camera's own rotation, as every other runtime does.
    var rot = p.camera_rot || 0;
    var sin = Math.sin(rot), cos = Math.cos(rot);
    return [cx + ox * cos - oy * sin, cy + ox * sin + oy * cos];
  }}
  function is_mouse_left_just_pressed() {{
    return !!window.__jengine_play.mouse_just_pressed;
  }}
  function is_mouse_left_pressed() {{
    return !!window.__jengine_play.mouse_down;
  }}
  function is_mouse_left_just_released() {{
    return !!window.__jengine_play.mouse_just_released;
  }}
  // The right button, which the engine has always tracked and scripts could
  // not read until Sep 2026.
  function is_mouse_right_just_pressed() {{
    return !!window.__jengine_play.mouse_right_just_pressed;
  }}
  function is_mouse_right_pressed() {{
    return !!window.__jengine_play.mouse_right_down;
  }}
  function is_mouse_right_just_released() {{
    return !!window.__jengine_play.mouse_right_just_released;
  }}
  // The middle button (Oct 2026).
  function is_mouse_middle_just_pressed() {{
    return !!window.__jengine_play.mouse_middle_just_pressed;
  }}
  function is_mouse_middle_pressed() {{
    return !!window.__jengine_play.mouse_middle_down;
  }}
  function is_mouse_middle_just_released() {{
    return !!window.__jengine_play.mouse_middle_just_released;
  }}
  function destroy_entity(entity) {{
    window.__jengine_play.commands.push({{ t: 'de', e: entity }});
  }}
  // Actions are resolved by the engine, not re-derived here.
  //
  // These used to walk `action_bindings` and test each binding against
  // `pressed_keys` -- a map built from keyboard KeyCodes, so a gamepad binding
  // like `Pad0_A` was never found and pad actions were silently dead in every
  // browser runtime while working natively (BUGS #9). The host now publishes
  // the answers from Rust's `InputState`, which is the same code the native
  // runtime resolves with.
  function is_action_pressed(action) {{
    var a = window.__jengine_play.actions_pressed;
    return !!(a && a[action]);
  }}
  function is_action_just_pressed(action) {{
    var a = window.__jengine_play.actions_just_pressed;
    return !!(a && a[action]);
  }}
  function is_action_just_released(action) {{
    var a = window.__jengine_play.actions_just_released;
    return !!(a && a[action]);
  }}
  // Analog value of an action, signed and dead-zoned, -1.0..=1.0. A button or
  // key binding reads 1.0 while held, so a keyboard and a stick can drive the
  // same action and a script does not care which is plugged in.
  function get_action_axis(action) {{
    var a = window.__jengine_play.action_axes;
    var v = a ? a[action] : 0;
    return typeof v === 'number' ? v : 0;
  }}
  // Raw pad access, for a game that wants a stick rather than a bound action.
  // Prefer actions where the binding should be rebindable.
  function get_gamepad_count() {{
    var p = window.__jengine_play.pads;
    return p ? p.length : 0;
  }}
  function is_gamepad_connected(pad) {{
    var p = window.__jengine_play.pads;
    return !!(p && p[pad] && p[pad].connected);
  }}
  function get_gamepad_axis(pad, axis) {{
    var p = window.__jengine_play.pads;
    if (!p || !p[pad] || !p[pad].connected) return 0;
    var v = p[pad].axes[axis];
    return typeof v === 'number' ? v : 0;
  }}
  function is_gamepad_button_pressed(pad, button) {{
    var p = window.__jengine_play.pads;
    if (!p || !p[pad] || !p[pad].connected) return false;
    return !!p[pad].buttons[button];
  }}
  // Hold / tap / double-tap, pads' names and families, the device used last
  // and button prompts: one hook over the engine's `InputView`, which native
  // reads from its snapshot (`input::interactions`).
  function __input_query(op, a, b) {{ var s = window.__jengine_play.state; return (s && s.input_query) ? s.input_query(op, a, b) : null; }}
  function get_action_hold_time(action) {{ return __input_query('hold_time', action) || 0; }}
  function is_action_held(action, seconds) {{ return !!__input_query('held', action, seconds); }}
  function is_action_tapped(action, seconds) {{ return !!__input_query('tapped', action, seconds); }}
  function is_action_double_tapped(action, seconds) {{ return !!__input_query('double_tapped', action, seconds); }}
  function get_gamepad_name(pad) {{ return __input_query('pad_name', pad || 0) || ''; }}
  function get_gamepad_type(pad) {{ return __input_query('pad_type', pad || 0) || ''; }}
  function get_last_input_device() {{ return __input_query('last_device') || 'none'; }}
  function get_last_gamepad() {{ var r = __input_query('last_pad'); return typeof r === 'number' ? r : -1; }}
  function get_action_prompt(action, style) {{ return __input_query('prompt', action, style) || {{ label: '', glyph: '', binding: '', style: 'keyboard' }}; }}
  function get_mouse_wheel() {{
    return window.__jengine_play.mouse_wheel_delta || 0;
  }}
  function get_mouse_delta() {{
    var p = window.__jengine_play;
    return [p.mouse_dx || 0, p.mouse_dy || 0];
  }}
  // ---- 2.4 audio mixer busses -------------------------------------------
  // Reads come from the `audio_buses` mirror; writes queue one `ab` command
  // discriminated by `op`. Bus volume is applied when a sound plays, so a
  // change here is audible, not just queryable.
  function __audio_bus(name) {{
    var buses = window.__jengine_play.audio_buses || {{}};
    return buses[name] || null;
  }}
  function audio_create_bus(name, parent, volume) {{
    window.__jengine_play.commands.push({{
      t: 'ab', op: 'create', n: name, p: parent || null,
      v: (volume === undefined || volume === null) ? 1.0 : volume
    }});
  }}
  function audio_remove_bus(name) {{
    window.__jengine_play.commands.push({{ t: 'ab', op: 'remove', n: name }});
  }}
  function audio_set_bus_volume(name, volume) {{
    window.__jengine_play.commands.push({{ t: 'ab', op: 'vol', n: name, v: volume }});
  }}
  function audio_set_variation(clip, pitch, volume) {{ var s = window.__jengine_play.state; if (s && s.audio_set_variation) s.audio_set_variation(String(clip), pitch || 0, volume || 0); }}
  function state_save(slot) {{ var s = window.__jengine_play.state; if (s && s.state_save) s.state_save(String(slot)); }}
  function state_load(slot) {{ var s = window.__jengine_play.state; if (s && s.state_load) s.state_load(String(slot)); }}
  function state_has(slot) {{ var s = window.__jengine_play.state; return (s && s.state_has) ? !!s.state_has(String(slot)) : false; }}
  function state_forget(slot) {{ var s = window.__jengine_play.state; if (s && s.state_forget) s.state_forget(String(slot)); }}
  function audio_follow_time(on) {{ var s = window.__jengine_play.state; if (s && s.audio_follow_time) s.audio_follow_time(!!on); }}
  function get_time_scale() {{ var s = window.__jengine_play.state; return (s && s.time_get_scale) ? s.time_get_scale() : 1; }}
  function audio_set_bus_effect(name, effect, value) {{
    window.__jengine_play.commands.push({{ t: 'ab', op: 'fx', n: name, e: effect, v: value }});
  }}
  function audio_get_bus_volume(name) {{
    var b = __audio_bus(name);
    return b ? b.volume : 1.0;
  }}
  function audio_set_bus_muted(name, muted) {{
    window.__jengine_play.commands.push({{ t: 'ab', op: 'mute', n: name, b: !!muted }});
  }}
  function audio_is_bus_muted(name) {{
    var b = __audio_bus(name);
    return b ? !!b.muted : false;
  }}
  function audio_set_bus_solo(name, solo) {{
    window.__jengine_play.commands.push({{ t: 'ab', op: 'solo', n: name, b: !!solo }});
  }}
  // ---- 2.5 input remapping ---------------------------------------------
  // Reads use the `input_actions` mirror the host republishes each frame;
  // writes queue one `ib` command discriminated by `op`, so a rebinding menu
  // behaves the same here as on native.
  function __input_action(action) {{
    var actions = window.__jengine_play.input_actions || {{}};
    return actions[action] || null;
  }}
  function input_get_actions() {{
    var actions = window.__jengine_play.input_actions || {{}};
    var out = [];
    for (var name in actions) {{ out.push(name); }}
    out.sort();
    return out;
  }}
  function input_get_bindings(action) {{
    var a = __input_action(action);
    return a ? a.keys.slice() : null;
  }}
  // What went down this frame, by binding name (Oct 2026).
  function input_get_just_pressed() {{
    return (window.__jengine_play.just_pressed_bindings || []).slice();
  }}
  // The characters typed this frame, as the layout made them (Oct 2026).
  function input_get_typed() {{
    return window.__jengine_play.typed || "";
  }}
  function input_get_dead_zone(action) {{
    var a = __input_action(action);
    return a ? (a.dead_zone || 0) : 0;
  }}
  function input_set_bindings(action, keys) {{
    window.__jengine_play.commands.push({{ t: 'ib', op: 'set', a: action, k: keys }});
  }}
  function input_add_binding(action, key) {{
    window.__jengine_play.commands.push({{ t: 'ib', op: 'add', a: action, key: key }});
  }}
  function input_remove_binding(action, key) {{
    window.__jengine_play.commands.push({{ t: 'ib', op: 'remove', a: action, key: key }});
  }}
  function input_clear_bindings(action) {{
    window.__jengine_play.commands.push({{ t: 'ib', op: 'clear', a: action }});
  }}
  function input_reset_bindings() {{
    window.__jengine_play.commands.push({{ t: 'ib', op: 'reset' }});
  }}
  function input_set_dead_zone(action, value) {{
    window.__jengine_play.commands.push({{ t: 'ib', op: 'dz', a: action, v: value }});
  }}
  // ---- 2.6 tilemap pathfinding ------------------------------------------
  // Answered synchronously by a host callback over a per-frame snapshot of
  // the tilemaps (`adapters/js/nav.rs`), so the same A* runs here as on
  // native. Returns `[[x, y], ...]`, or null when there is no path - or when
  // no host installed the hook.
  // The cells next to a cell, and the steps between two - six neighbours on a
  // hexagonal map. Answered by the host from its tilemap snapshot.
  function tilemap_neighbors(entity, tx, ty) {{
    var s = window.__jengine_play.state;
    return (s && s.tilemap_neighbors) ? s.tilemap_neighbors(entity, tx, ty) : [];
  }}
  function tilemap_distance(entity, ax, ay, bx, by) {{
    var s = window.__jengine_play.state;
    return (s && s.tilemap_distance) ? s.tilemap_distance(entity, ax, ay, bx, by) : -1;
  }}
  function tilemap_find_path(entity, layer, sx, sy, gx, gy, mode) {{
    var s = window.__jengine_play.state;
    if (s && s.tilemap_find_path) {{
      return s.tilemap_find_path(entity, layer, sx, sy, gx, gy, mode || 'manhattan');
    }}
    return null;
  }}
  function tilemap_find_path_world(entity, layer, sx, sy, gx, gy, mode) {{
    var s = window.__jengine_play.state;
    if (s && s.tilemap_find_path_world) {{
      return s.tilemap_find_path_world(entity, layer, sx, sy, gx, gy, mode || 'manhattan');
    }}
    return null;
  }}
  // ---- 3.5.A hierarchy / world transforms -------------------------------
  // `world_transforms` is `[x, y, rotation, sx, sy]` per entity, republished
  // each frame from the resolved `WorldTransform`. An entity with no parent
  // chain has no cached entry, and for it the local transform IS the world
  // transform - the same fallback the native getters make.
  function __world_tf(entity) {{
    var w = window.__jengine_play.world_transforms || {{}};
    var t = w[entity];
    if (t) return t;
    // Through the accessors: they know the typed-array layout and the alias
    // map, and this used to index the per-entity objects directly.
    var p = get_position(entity);
    if (!p) return null;
    var r = get_rotation(entity);
    var s = get_scale(entity);
    return [p[0], p[1], r, s[0], s[1]];
  }}
  function get_world_position(entity) {{
    var t = __world_tf(entity);
    return t ? [t[0], t[1]] : null;
  }}
  function get_world_rotation(entity) {{
    var t = __world_tf(entity);
    return t ? t[2] : null;
  }}
  function get_world_scale(entity) {{
    var t = __world_tf(entity);
    return t ? [t[3], t[4]] : null;
  }}
  function set_parent(child, parent) {{
    // `nil` / omitted parent detaches, matching the native
    // native `SetParent` with a `None` parent_id.
    var p = (parent === null || parent === undefined) ? null : parent;
    window.__jengine_play.commands.push({{ t: 'sprt', c: child, p: p }});
  }}
  // ---- 2.3 save system -------------------------------------------------
  // `save_data` is a slot -> key -> value mirror the host republishes after
  // every command drain, so `save_get` can answer synchronously (Lua has no
  // await) and still see values loaded from storage. Writes update the mirror
  // AND queue the command, exactly like `set_custom`.
  // A slot the host has not mirrored yet is read straight out of
  // localStorage, in the same `jengine_save:<slot>` / {{version,data}} format
  // `save::SaveSystem` writes on wasm. Without this the first `save_get` of a
  // session - typically in `init()`, before anything has been written - would
  // answer nil for a value that is sitting in storage. Native gets this from
  // `ensure_loaded`; here the storage is readable synchronously, so read it.
  function __save_slot(slot) {{
    var d = window.__jengine_play.save_data || {{}};
    if (d[slot]) return d[slot];
    var loaded = {{}};
    try {{
      var raw = window.localStorage.getItem('jengine_save:' + slot);
      if (raw) {{
        var parsed = JSON.parse(raw);
        // Unknown schema versions are ignored rather than guessed at, which
        // is what the Rust loader does.
        if (parsed && parsed.version === 1 && parsed.data) loaded = parsed.data;
      }}
    }} catch (e) {{ /* private mode, quota, corrupt entry - treat as empty */ }}
    d[slot] = loaded;
    window.__jengine_play.save_data = d;
    return loaded;
  }}
  function save_set(slot, key, value) {{
    var d = window.__jengine_play.save_data || {{}};
    var s = __save_slot(slot);
    s[key] = value;
    d[slot] = s;
    window.__jengine_play.save_data = d;
    window.__jengine_play.commands.push({{ t: 'svst', s: slot, k: key, v: value }});
  }}
  function save_get(slot, key) {{
    var s = __save_slot(slot);
    return s && s[key] !== undefined ? s[key] : null;
  }}
  function save_commit(slot) {{
    window.__jengine_play.commands.push({{ t: 'svcm', s: slot }});
  }}
  function save_clear(slot) {{
    var d = window.__jengine_play.save_data || {{}};
    delete d[slot];
    window.__jengine_play.save_data = d;
    window.__jengine_play.commands.push({{ t: 'svcl', s: slot }});
  }}
  function get_custom(entity, component_name, field_name) {{
    var customs = window.__jengine_play.custom_components || {{}};
    var ent = customs[entity];
    var comp = ent ? ent[component_name] : null;
    return comp && comp[field_name] !== undefined ? comp[field_name] : null;
  }}
  function get_data(table_id, row_id, field_name) {{
    // Data-oriented design: read a field from a loaded `.data` table row.
    // The host populates `window.__jengine_play.data_tables` (keyed by table
    // id) with `{{ rows: [{{ id, ...fields }}] }}`. Returns null if missing.
    var tables = window.__jengine_play.data_tables || {{}};
    var key = String(table_id).replace(/\\.data$/, '');
    var table = tables[key];
    if (!table || !table.rows) return null;
    for (var i = 0; i < table.rows.length; i++) {{
      if (table.rows[i].id === row_id) {{
        var v = table.rows[i][field_name];
        return v === undefined ? null : v;
      }}
    }}
    return null;
  }}
  function set_custom(entity, component_name, field_name, value) {{
    var customs = window.__jengine_play.custom_components || {{}};
    if (!customs[entity]) customs[entity] = {{}};
    if (!customs[entity][component_name]) customs[entity][component_name] = {{}};
    customs[entity][component_name][field_name] = value;
    window.__jengine_play.custom_components = customs;
    // Round-trip to ECS so the write survives the next-tick rebuild
    // of `custom_components` from the world. Mirrors native bridge's
    // `BridgeCommand::SetCustomField`.
    window.__jengine_play.commands.push({{ t: 'sc', e: entity, cn: component_name, fn: field_name, v: value }});
  }}
  function set_health(entity, current, max) {{
    // Overwrite native Health (current + max); non-positive max keeps the
    // existing max. Mirrors BridgeCommand::SetHealth.
    window.__jengine_play.commands.push({{ t: 'shp', e: entity, cur: current, max: max }});
  }}
  function set_hitbox(entity, damage, knockback, retrigger) {{
    // Overwrite native Hitbox combat stats. Mirrors BridgeCommand::SetHitbox.
    window.__jengine_play.commands.push({{ t: 'shb', e: entity, dmg: damage, kb: knockback, rt: retrigger }});
  }}
  // Phase 6.x parity helper: matches the native `set_fields` adapter API.
  // Writes are pushed directly into the same per-entity shadow map used
  // by `set_custom`, so reads from `get_custom` see batched writes
  // immediately without going through the bridge command queue.
  function set_fields(entity, component_name, fields) {{
    var customs = window.__jengine_play.custom_components || {{}};
    if (!customs[entity]) customs[entity] = {{}};
    if (!customs[entity][component_name]) customs[entity][component_name] = {{}};
    var bucket = customs[entity][component_name];
    for (var k in fields) {{
      if (Object.prototype.hasOwnProperty.call(fields, k)) {{
        bucket[k] = fields[k];
        // Round-trip each field to ECS (same as set_custom).
        window.__jengine_play.commands.push({{ t: 'sc', e: entity, cn: component_name, fn: k, v: fields[k] }});
      }}
    }}
    window.__jengine_play.custom_components = customs;
  }}
  // --- Persistent K/V store (survives scene transitions) ---
  // Lives on `window.__jengine_persistent`, separate from the per-scene
  // `__jengine_play` namespace that scene reloads rebuild.
  // A mirror of the engine's one store (`scripting::persistent`): reads are
  // synchronous from here, every write also goes to the store as a command,
  // and whenever the host publishes a changed `state.persistent_json` the
  // mirror is replaced from it - which is how a package's writes arrive.
  // `persistent_mirror` is preamble-internal, not a script global.
  if (!window.__jengine_persistent) {{
    window.__jengine_persistent = {{ numbers: {{}}, strings: {{}} }};
  }}
  function persistent_mirror() {{
    var st = window.__jengine_play && window.__jengine_play.state;
    var j = st && st.persistent_json;
    if (typeof j === 'string' && j !== window.__jengine_persistent_json) {{
      window.__jengine_persistent_json = j;
      try {{
        var parsed = JSON.parse(j);
        window.__jengine_persistent = {{ numbers: parsed.numbers || {{}}, strings: parsed.strings || {{}} }};
      }} catch (e) {{}}
    }}
    return window.__jengine_persistent;
  }}
  function persistent_set_number(key, value) {{
    persistent_mirror().numbers[key] = +value;
    window.__jengine_play.commands.push({{ t: 'psn', k: String(key), n: +value }});
  }}
  function persistent_get_number(key) {{
    var v = persistent_mirror().numbers[key];
    return v === undefined ? null : v;
  }}
  function persistent_set_string(key, value) {{
    persistent_mirror().strings[key] = String(value);
    window.__jengine_play.commands.push({{ t: 'pss', k: String(key), s: String(value) }});
  }}
  function persistent_get_string(key) {{
    var v = persistent_mirror().strings[key];
    return v === undefined ? null : v;
  }}
  function persistent_has(key) {{
    var p = persistent_mirror();
    return Object.prototype.hasOwnProperty.call(p.numbers, key)
        || Object.prototype.hasOwnProperty.call(p.strings, key);
  }}
  function persistent_clear() {{
    var p = persistent_mirror();
    p.numbers = {{}};
    p.strings = {{}};
    window.__jengine_play.commands.push({{ t: 'pcl' }});
  }}
  function send_event(name, entity, data) {{
    var d = (data === null || data === undefined) ? null : (typeof data === 'string' ? data : JSON.stringify(data));
    window.__jengine_play.commands.push({{ t: 'evt', n: name, e: entity, d: d }});
  }}
  // ---- 3.5.B/C skeletal rigs --------------------------------------------
  // Reads come from mirrors the host republishes each frame; native answers
  // the same three by querying the live world, which is not available from
  // inside a script call here.
  function find_skeleton(name) {{
    var roots = window.__jengine_play.skeletons || {{}};
    var id = roots[name];
    return id === undefined ? null : id;
  }}
  function find_bone(root, bone_name) {{
    // `__ent`: a `spawn_rig` handle names its root, as it does natively.
    var bones = (window.__jengine_play.bones || {{}})[__ent(root)] || {{}};
    var id = bones[bone_name];
    return id === undefined ? null : id;
  }}
  function get_animation_time(entity) {{
    var times = window.__jengine_play.animation_times || {{}};
    var t = times[entity];
    return t === undefined ? 0 : t;
  }}
  function set_animation_time(entity, time) {{
    window.__jengine_play.commands.push({{ t: 'sat2', e: entity, v: time }});
  }}
  function set_ik_target(entity, x, y) {{
    window.__jengine_play.commands.push({{ t: 'ikt', e: entity, x: x, y: y }});
  }}
  function crossfade_animation(entity, clip, seconds) {{
    window.__jengine_play.commands.push({{ t: 'caf', e: entity, clip: clip, s: seconds }});
  }}
  function set_animation_blend(entity, clip, weight) {{
    window.__jengine_play.commands.push({{ t: 'sabl', e: entity, clip: clip || '', w: weight || 0 }});
  }}
  // Animation layers: a clip on part of a skeleton, faded in and out.
  function animation_play_layer(entity, layer, clip, seconds, loop) {{
    window.__jengine_play.commands.push({{ t: 'anly', e: entity, op: 'play', l: layer, c: clip, v: 1, s: (seconds === undefined || seconds === null) ? 0.2 : seconds, lp: loop === true }});
  }}
  function animation_stop_layer(entity, layer, seconds) {{
    window.__jengine_play.commands.push({{ t: 'anly', e: entity, op: 'stop', l: layer, c: '', v: 0, s: (seconds === undefined || seconds === null) ? 0.2 : seconds, lp: false }});
  }}
  function animation_set_layer_weight(entity, layer, weight, seconds) {{
    window.__jengine_play.commands.push({{ t: 'anly', e: entity, op: 'weight', l: layer, c: '', v: weight || 0, s: seconds || 0, lp: false }});
  }}
  function set_animation_blend_position(entity, x, y) {{
    window.__jengine_play.commands.push({{ t: 'sabp', e: entity, x: x || 0, y: y || 0 }});
  }}
  function set_conveyor_speed(entity, speed) {{
    window.__jengine_play.commands.push({{ t: 'scvs', e: entity, s: speed || 0 }});
  }}
  function set_ik_weight(entity, weight) {{
    window.__jengine_play.commands.push({{ t: 'ikw', e: entity, w: weight }});
  }}
  // Cross-script method dispatch. `caller` is passed through so the callee
  // can answer; the host resolves `method` on the target's own script.
  function call(caller, target, method) {{
    var extra = Array.prototype.slice.call(arguments, 3);
    window.__jengine_play.commands.push({{
      t: 'csm', c: caller, tgt: target, m: method, a: extra
    }});
  }}
  // Targeted variant: same payload as `send_event`, but only `target`'s
  // listener fires. Argument order matches native's
  // `send_event_to(name, sender, target, data)`.
  function send_event_to(name, sender, target, data) {{
    var d = (data === null || data === undefined) ? null : (typeof data === 'string' ? data : JSON.stringify(data));
    window.__jengine_play.commands.push({{ t: 'evt', n: name, e: sender, tgt: target, d: d }});
  }}
  function register_event_listener(name, entity) {{
    var listeners = window.__jengine_play.event_listeners || {{}};
    if (!listeners[name]) listeners[name] = [];
    listeners[name].push(entity);
    window.__jengine_play.event_listeners = listeners;
  }}
  function remove_event_listener(name, entity) {{
    var listeners = window.__jengine_play.event_listeners || {{}};
    if (listeners[name]) {{
      listeners[name] = listeners[name].filter(function(e) {{ return e !== entity; }});
    }}
  }}
  function draw_debug_line(x1, y1, x2, y2, r, g, b) {{
    window.__jengine_play.commands.push({{ t: 'ddl', x1: x1, y1: y1, x2: x2, y2: y2, r: r, g: g, b: b }});
  }}
  function draw_debug_rect(x, y, w, h, r, g, b) {{
    window.__jengine_play.commands.push({{ t: 'ddr', x: x, y: y, w: w, h: h, r: r, g: g, b: b }});
  }}
  function draw_debug_circle(x, y, radius, r, g, b) {{
    window.__jengine_play.commands.push({{ t: 'ddc', x: x, y: y, rad: radius, r: r, g: g, b: b }});
  }}
  // The active camera's zoom; 1.0 is the framing the scene was authored at.
  // Drained as the shared `scz` world command, so every host applies the same
  // code.
  function set_camera_zoom(zoom) {{
    window.__jengine_play.commands.push({{ t: 'scz', z: zoom }});
  }}
  // World units per art pixel for a pixel-perfect camera; 0 turns it off.
  function set_camera_pixel_size(size) {{
    window.__jengine_play.commands.push({{ t: 'scpx', s: size }});
  }}
  function camera_shake(amplitude, duration, frequency) {{
    // P1.3 — frequency is optional; the engine treats 0 as "use default".
    window.__jengine_play.commands.push({{
      t: 'cmsh',
      a: amplitude,
      d: duration,
      f: (frequency === null || frequency === undefined) ? 0 : frequency,
    }});
  }}
  function lerp(a, b, t) {{ return a + (b - a) * t; }}
  function clamp(v, mn, mx) {{ return Math.min(Math.max(v, mn), mx); }}
  function distance(x1, y1, x2, y2) {{ return Math.sqrt((x2-x1)*(x2-x1)+(y2-y1)*(y2-y1)); }}
  // Through the engine's `libm` (`script_math`), as native computes it; the
  // browser's `Math.atan2` differs in the last bit.
  function angle_between(x1, y1, x2, y2) {{
    var m = globalThis.__jengine_script_math;
    return m ? m.atan2(y2 - y1, x2 - x1) : Math.atan2(y2 - y1, x2 - x1);
  }}
  function set_timer(entity, delay, callback) {{
    var timers = window.__jengine_play.timers || [];
    var id = (window.__jengine_play.next_timer_id || 1);
    window.__jengine_play.next_timer_id = id + 1;
    timers.push({{ id: id, entity: entity, delay: delay, elapsed: 0, repeat: false, cb: callback }});
    window.__jengine_play.timers = timers;
    return id;
  }}
  function set_interval(entity, interval, callback) {{
    var timers = window.__jengine_play.timers || [];
    var id = (window.__jengine_play.next_timer_id || 1);
    window.__jengine_play.next_timer_id = id + 1;
    timers.push({{ id: id, entity: entity, delay: interval, elapsed: 0, repeat: true, cb: callback }});
    window.__jengine_play.timers = timers;
    return id;
  }}
  function cancel_timer(timer_id) {{
    var timers = window.__jengine_play.timers || [];
    window.__jengine_play.timers = timers.filter(function(t) {{ return t.id !== timer_id; }});
  }}
  // Spawn handles start at a million, as native's do (`FIRST_SPAWN_ID`): out
  // of the range real entity ids live in, so a handle can never be read as
  // some other entity, and the hosts can name a spawned body by its handle.
  function spawn_entity(name) {{
    var idx = window.__jengine_play.next_entity_id || 1000000;
    window.__jengine_play.next_entity_id = idx + 1;
    window.__jengine_play.commands.push({{ t: 'se', name: name || 'Entity', idx: idx }});
    __note_spawn(idx);
    return idx;
  }}
  function spawn_prefab(prefab_name) {{
    var prefabs = window.__jengine_play.prefabs || {{}};
    var entry = prefabs[prefab_name];
    // Editor-side prefabs are installed as full `EntityData`
    // ({{name, components}}); the bridge `spf` handler wants only
    // the components map, mirroring the native path which sends
    // `prefab.components`. Unwrap if needed; pass through if the
    // entry already looks like a bare component map.
    var data = null;
    var kids = null;
    if (entry) {{
      if (entry.components && typeof entry.components === 'object') {{
        data = entry.components;
        // A prefab's parts (`EntityData.children`), spawned under it.
        if (entry.children && entry.children.length) kids = JSON.stringify(entry.children);
      }} else {{
        data = entry;
      }}
    }}
    var idx = window.__jengine_play.next_entity_id || 1000000;
    window.__jengine_play.next_entity_id = idx + 1;
    window.__jengine_play.commands.push({{ t: 'spf', name: prefab_name, data: data ? JSON.stringify(data) : null, kids: kids, idx: idx }});
    __note_spawn(idx);
    return idx;
  }}
  // `spawn_rig(rig_id)`: a `.rig` by its path under `assets/`, every bone an
  // entity. Known to the script by the handle, like a prefab, until the host
  // spawns it; the handle names the rig's root bone.
  function spawn_rig(rig_id) {{
    var idx = window.__jengine_play.next_entity_id || 1000000;
    window.__jengine_play.next_entity_id = idx + 1;
    window.__jengine_play.commands.push({{ t: 'srig', r: String(rig_id || ''), idx: idx }});
    __note_spawn(idx);
    return idx;
  }}
  function add_tag(entity, tag) {{
    window.__jengine_play.commands.push({{ t: 'at', e: entity, tag: tag }});
    var tags = window.__jengine_play.tags || {{}};
    if (!tags[entity]) tags[entity] = [];
    if (tags[entity].indexOf(tag) === -1) tags[entity].push(tag);
    window.__jengine_play.tags = tags;
  }}
  function remove_tag(entity, tag) {{
    window.__jengine_play.commands.push({{ t: 'rt', e: entity, tag: tag }});
    var tags = window.__jengine_play.tags || {{}};
    if (tags[entity]) tags[entity] = tags[entity].filter(function(t) {{ return t !== tag; }});
  }}
  function has_tag(entity, tag) {{
    var tags = window.__jengine_play.tags || {{}};
    return tags[entity] ? tags[entity].indexOf(tag) !== -1 : false;
  }}
  function get_entities_by_tag(tag) {{
    var tags = window.__jengine_play.tags || {{}};
    var result = [];
    var seen = [];
    for (var e in tags) {{
      var t = tags[e];
      if (t.indexOf(tag) === -1) continue;
      // The host mirrors every script-spawned entity under BOTH its real
      // entity id and its script-side alias id, so a naive walk of these keys
      // returns the same entity twice. Both keys are set to the SAME array
      // object, so identity is an exact alias test. Without this every
      // per-entity script loop ran twice per frame: double status ticks,
      // double damage numbers, doubled enemy counts.
      if (seen.indexOf(t) !== -1) continue;
      seen.push(t);
      result.push(parseInt(e));
    }}
    return __lua_arr(result);
  }}
  function find_in_radius(cx, cy, radius, tag) {{
    var positions = window.__jengine_play.positions;
    var tags = window.__jengine_play.tags || {{}};
    var r2 = radius * radius;
    var result = [];
    if (!positions) return __lua_arr(result);
    // `live_ids` lists the entities that actually have a Transform. The old
    // `for (var e in positions)` cannot work against a flat typed array - it
    // would walk every slot including the gaps - and this is faster anyway,
    // since it visits live entities only and needs no dedupe: each entity
    // appears once, by real id.
    // `live_ids` is a persistent array reused between frames, so only the
    // first `live_count` entries are this frame's - the tail is stale.
    var live = window.__jengine_play.live_ids || [];
    var live_n = window.__jengine_play.live_count;
    if (live_n === undefined) live_n = live.length;
    for (var li = 0; li < live_n; li++) {{
      var e = live[li];
      if (tag !== undefined && tag !== null) {{
        var t = tags[e];
        if (!t || t.indexOf(tag) === -1) continue;
      }}
      var dx = positions[e * 2] - cx;
      var dy = positions[e * 2 + 1] - cy;
      if (dx * dx + dy * dy <= r2) result.push(e);
    }}
    return __lua_arr(result);
  }}
  function quit_game() {{
    window.__jengine_play.commands.push({{ t: 'quit' }});
  }}
  function set_time_scale(scale) {{
    window.__jengine_play.commands.push({{ t: 'tsc', s: scale }});
  }}
  function load_scene(path) {{
    fetch('assets/' + path).then(function(r) {{ return r.text(); }}).then(function(json) {{
      window.__jengine_play.pending_scene = {{ action: 'load', json: json, path: path }};
    }}).catch(function(e) {{ console.error('load_scene failed:', e); }});
  }}
  function push_scene(path) {{
    fetch('assets/' + path).then(function(r) {{ return r.text(); }}).then(function(json) {{
      window.__jengine_play.pending_scene = {{ action: 'push', json: json, path: path }};
    }}).catch(function(e) {{ console.error('push_scene failed:', e); }});
  }}
  function pop_scene() {{ window.__jengine_play.pending_scene = {{ action: 'pop' }}; }}
  // `Scene.transition`: arm the process-wide transition, then load.
  function scene_transition(path, style, seconds, texture) {{
    var st = window.__jengine_play.state;
    if (st && st.scene_transition_arm) {{ st.scene_transition_arm(style || 'fade', seconds == null ? 0.6 : seconds, texture || ''); }}
    load_scene(path);
  }}
  // Slice 12.A — fade transitions. `fade_in(dur)` or `fade_in(dur, r, g, b)`.
  function fade_in(dur, r, g, b) {{
    window.__jengine_play.commands.push({{ t: 'fdin', d: dur, r: r || 0, g: g || 0, b: b || 0 }});
  }}
  function fade_out(dur, r, g, b) {{
    window.__jengine_play.commands.push({{ t: 'fdou', d: dur, r: r || 0, g: g || 0, b: b || 0 }});
  }}
  // P1.3 — screen_flash(dur) defaults to opaque white; channels are optional.
  function screen_flash(dur, r, g, b, a) {{
    window.__jengine_play.commands.push({{
      t: 'scrf', d: dur,
      r: (r === undefined ? 1 : r),
      g: (g === undefined ? 1 : g),
      b: (b === undefined ? 1 : b),
      a: (a === undefined ? 1 : a)
    }});
  }}
  function get_current_scene_name() {{ return window.__jengine_play.scene_name || ''; }}
  function load_script(entity, path) {{ window.__jengine_play.commands.push({{ t: 'ls', e: entity, p: path }}); }}
  function get_fps() {{ return window.__jengine_play.fps || 0; }}
  function get_music_position() {{ return window.__jengine_play.music_position || 0; }}
  // The browser has its own wall clock, so this needs nothing from the host.
  function get_unix_time() {{ return Date.now() / 1000; }}
  function get_frame_time() {{ return window.__jengine_play.frame_time || 0; }}
  function get_entity_count() {{ return window.__jengine_play.entity_count || 0; }}
  function get_draw_calls() {{ return window.__jengine_play.draw_calls || 0; }}
  function log_stats() {{
    var s = 'FPS: ' + (window.__jengine_play.fps || 0).toFixed(1)
      + ' | Frame: ' + (window.__jengine_play.frame_time || 0).toFixed(2) + 'ms'
      + ' | Entities: ' + (window.__jengine_play.entity_count || 0)
      + ' | Draw calls: ' + (window.__jengine_play.draw_calls || 0);
    window.__jengine_play.commands.push({{ t: 'log', m: s }});
  }}
  function set_debug_overlay(visible) {{ window.__jengine_play.debug_overlay_visible = !!visible; }}
  function play_animation(entity, clip_name) {{ window.__jengine_play.commands.push({{ t: 'pa', e: entity, clip: clip_name }}); }}
  function stop_animation(entity) {{ window.__jengine_play.commands.push({{ t: 'sa', e: entity }}); }}
  function set_animation_speed(entity, speed) {{ window.__jengine_play.commands.push({{ t: 'sas', e: entity, speed: speed }}); }}
  function is_animation_playing(entity) {{ return !!(window.__jengine_play.animation_states || {{}})[entity]; }}
  function tween_position(entity, x, y, duration, easing) {{
    var id = (window.__jengine_play.next_tween_id || 1); window.__jengine_play.next_tween_id = id + 1;
    window.__jengine_play.commands.push({{ t: 'tp', e: entity, x: x, y: y, dur: duration, ease: easing || 'Linear', tid: id }}); return id;
  }}
  function tween_rotation(entity, radians, duration, easing) {{
    var id = (window.__jengine_play.next_tween_id || 1); window.__jengine_play.next_tween_id = id + 1;
    window.__jengine_play.commands.push({{ t: 'tr', e: entity, r: radians, dur: duration, ease: easing || 'Linear', tid: id }}); return id;
  }}
  function tween_scale(entity, sx, sy, duration, easing) {{
    var id = (window.__jengine_play.next_tween_id || 1); window.__jengine_play.next_tween_id = id + 1;
    window.__jengine_play.commands.push({{ t: 'ts', e: entity, sx: sx, sy: sy, dur: duration, ease: easing || 'Linear', tid: id }}); return id;
  }}
  function tween_color(entity, r, g, b, a, duration, easing) {{
    var id = (window.__jengine_play.next_tween_id || 1); window.__jengine_play.next_tween_id = id + 1;
    window.__jengine_play.commands.push({{ t: 'tc', e: entity, r: r, g: g, b: b, a: a, dur: duration, ease: easing || 'Linear', tid: id }}); return id;
  }}
  function cancel_tween(tween_id) {{ window.__jengine_play.commands.push({{ t: 'ct', tid: tween_id }}); }}
  function cancel_all_tweens(entity) {{ window.__jengine_play.commands.push({{ t: 'cat', e: entity }}); }}
  function get_tile(entity, layer_name, x, y) {{
    var tm = (window.__jengine_play.tilemaps || {{}})[entity]; if (!tm) return -1;
    for (var i = 0; i < tm.layers.length; i++) {{ if (tm.layers[i].name === layer_name) {{
      var l = tm.layers[i]; if (x < 0 || y < 0 || x >= l.width || y >= l.height) return -1;
      var v = l.tiles[y * l.width + x]; return (v === null || v === undefined) ? 0 : v; }} }} return -1;
  }}
  function get_tile_data(entity, layer_name, x, y, field) {{
    var gid = get_tile(entity, layer_name, x, y); if (gid <= 0) return null;
    var tm = (window.__jengine_play.tilemaps || {{}})[entity]; var d = tm && tm.data && tm.data[gid]; if (!d) return null;
    if (field === undefined || field === null) return d; var v = d[field]; return v === undefined ? null : v;
  }}
  function set_tile(entity, layer_name, x, y, tile_id) {{ window.__jengine_play.commands.push({{ t: 'stl', e: entity, ln: layer_name, x: x, y: y, tid: tile_id }}); }}
  function get_tile_at_world(entity, layer_name, world_x, world_y) {{
    var tm = (window.__jengine_play.tilemaps || {{}})[entity]; if (!tm) return -1;
    var t = world_to_tile(entity, world_x, world_y);
    return get_tile(entity, layer_name, t[0], t[1]);
  }}
  // `Tilemap::local_to_grid` and `grid_to_local`, square or isometric. A
  // hexagonal map's cell is picked by the host (`Tilemap::cell_at`), so there
  // is no second copy of that here.
  function world_to_tile(entity, world_x, world_y) {{
    var tm = (window.__jengine_play.tilemaps || {{}})[entity]; if (!tm) return [0, 0];
    var s = window.__jengine_play.state;
    if (tm.hex) return (s && s.tilemap_world_to_tile && s.tilemap_world_to_tile(entity, world_x, world_y)) || [0, 0];
    var pos = get_world_position(entity) || [0, 0];
    var x = world_x - pos[0], y = world_y - pos[1], tw = tm.tile_size[0], th = tm.tile_size[1];
    if (tm.iso) return [Math.floor(x / tw - y / th), Math.floor(-y / th - x / tw)];
    return [Math.floor(x / tw), Math.floor(-y / th)];
  }}
  function tile_to_world(entity, tile_x, tile_y) {{
    var tm = (window.__jengine_play.tilemaps || {{}})[entity]; if (!tm) return [0, 0];
    var s = window.__jengine_play.state;
    if (tm.hex) return (s && s.tilemap_tile_to_world && s.tilemap_tile_to_world(entity, tile_x, tile_y)) || [0, 0];
    var pos = get_world_position(entity) || [0, 0];
    var c = tile_x + 0.5, r = tile_y + 0.5, tw = tm.tile_size[0], th = tm.tile_size[1];
    if (tm.iso) return [pos[0] + (c - r) * tw * 0.5, pos[1] - (c + r) * th * 0.5];
    return [pos[0] + c * tw, pos[1] - r * th];
  }}
  function emit_burst(entity, count) {{ window.__jengine_play.commands.push({{ t: 'eb', e: entity, c: count }}); }}
  function set_emitter_active(entity, active) {{ window.__jengine_play.commands.push({{ t: 'sea', e: entity, a: !!active }}); }}
  function set_emitter_rate(entity, rate) {{ window.__jengine_play.commands.push({{ t: 'ser', e: entity, r: rate }}); }}
  function set_emitter_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'sef', e: entity, f: field, v: value }}); }}
  function set_cloth_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'scf', e: entity, f: field, v: value }}); }}
  function set_path_follow_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'spff', e: entity, f: field, v: value }}); }}
  function set_line_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'slf', e: entity, f: field, v: value }}); }}
  function set_trail_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'strl', e: entity, f: field, v: value }}); }}
  function set_fluid_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'sffl', e: entity, f: field, v: value === undefined ? null : value }}); }}
  function set_mirror_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'smrf', e: entity, f: field, v: value === undefined ? null : value }}); }}
  function smooth_terrain_edit(entity, x, y, radius, amount) {{ window.__jengine_play.commands.push({{ t: 'stte', e: entity, x: x, y: y, r: radius, a: amount }}); }}
  function dialogue_start(entity, node) {{ window.__jengine_play.commands.push({{ t: 'dlg', e: entity, op: 'start', a: (node === undefined || node === null) ? '' : String(node) }}); }}
  function dialogue_advance(entity) {{ window.__jengine_play.commands.push({{ t: 'dlg', e: entity, op: 'advance', a: '' }}); }}
  function dialogue_choose(entity, n) {{ window.__jengine_play.commands.push({{ t: 'dlg', e: entity, op: 'choose', a: String(n) }}); }}
  function dialogue_stop(entity) {{ window.__jengine_play.commands.push({{ t: 'dlg', e: entity, op: 'stop', a: '' }}); }}
  function ui_drop(entity, slot) {{ window.__jengine_play.commands.push({{ t: 'uidp', e: entity, s: String(slot) }}); }}
  function mode7_place(entity, x, y, angle) {{ window.__jengine_play.commands.push({{ t: 'm7p', e: entity, x: x, y: y, a: angle || 0 }}); }}
  function mode7_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'm7s', e: entity, f: field, v: value }}); }}
  function stage_place(entity, x, y, z) {{ window.__jengine_play.commands.push({{ t: 'stgp', e: entity, x: x, y: y, z: z || 0 }}); }}
  function stage_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'stgs', e: entity, f: field, v: value }}); }}
  function sprite_tilt_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'tilt', e: entity, f: field, v: value }}); }}
  function afterimage_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'aimg', e: entity, f: field, v: value }}); }}
  function physics_slice(x0, y0, x1, y1) {{ window.__jengine_play.commands.push({{ t: 'pslc', x0: x0, y0: y0, x1: x1, y1: y1 }}); }}
  function portal_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'prtl', e: entity, f: field, v: value }}); }}
  function jelly_poke(entity, x, y, strength, radius) {{ window.__jengine_play.commands.push({{ t: 'jlpk', e: entity, x: x, y: y, s: strength == null ? 300 : strength, r: radius == null ? 40 : radius }}); }}
  function jelly_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'jlst', e: entity, f: field, v: value }}); }}
  function marker_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'mkst', e: entity, f: field, v: value }}); }}
  function radial_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'rdst', e: entity, f: field, v: value }}); }}
  function bubble_say(entity, text, seconds) {{ window.__jengine_play.commands.push({{ t: 'bbsy', e: entity, x: String(text == null ? '' : text), s: seconds == null ? -1 : seconds }}); }}
  function bubble_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'bbst', e: entity, f: field, v: value }}); }}
  function outline_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'olst', e: entity, f: field, v: value }}); }}
  function lightning_strike(entity, x, y) {{ window.__jengine_play.commands.push({{ t: 'ltsk', e: entity, x: x, y: y }}); }}
  function lightning_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'ltst', e: entity, f: field, v: value }}); }}
  function flow_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'flst', e: entity, f: field, v: value }}); }}
  function nav_link_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'nvlk', e: entity, f: field, v: value }}); }}
  function morph_to(entity, target, seconds) {{ window.__jengine_play.commands.push({{ t: 'mrto', e: entity, n: String(target == null ? '' : target), s: seconds == null ? -1 : seconds }}); }}
  function morph_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'mrst', e: entity, f: field, v: value }}); }}
  function toast_show(entity, title, text, icon, seconds) {{ window.__jengine_play.commands.push({{ t: 'tsho', e: entity, n: String(title == null ? '' : title), x: String(text == null ? '' : text), i: String(icon == null ? '' : icon), s: seconds == null ? 0 : seconds }}); }}
  function toast_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'tsst', e: entity, f: field, v: value }}); }}
  function snow_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'snwc', e: entity, f: field, v: value }}); }}
  function fog_of_war_reset(entity) {{ window.__jengine_play.commands.push({{ t: 'fwrs', e: entity }}); }}
  function fog_of_war_set(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'fwsf', e: entity, f: field, v: value === undefined ? null : value }}); }}
  function set_beat_clock_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'sbcf', e: entity, f: field, v: value }}); }}
  function set_text_path_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'stxp', e: entity, f: field, v: value }}); }}
  function set_trajectory_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'strj', e: entity, f: field, v: value }}); }}
  function set_tile_light_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'stlt', e: entity, f: field, v: value }}); }}
  function set_auto_normal_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'sanf', e: entity, f: field, v: value }}); }}
  function set_cast_shadow_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'scsh', e: entity, f: field, v: value }}); }}
  function set_time_scale_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'stsf', e: entity, f: field, v: value }}); }}
  function set_origin_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'sfof', e: entity, f: field, v: value }}); }}
  function set_day_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'sdcf', e: entity, f: field, v: value }}); }}
  function set_vision_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'svsf', e: entity, f: field, v: value }}); }}
  function wfc_generate(entity, x, y, w, h) {{ window.__jengine_play.commands.push({{ t: 'swfc', e: entity, x: x, y: y, w: w, h: h }}); }}
  function set_fog_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'sfgf', e: entity, f: field, v: value }}); }}
  function set_rewind_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'srwf', e: entity, f: field, v: value }}); }}
  function smoke_puff(entity, x, y, radius, amount, vx, vy) {{ window.__jengine_play.commands.push({{ t: 'smpf', e: entity, x: x, y: y, r: radius, a: amount, vx: vx || 0, vy: vy || 0 }}); }}
  function set_smoke_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'ssmf', e: entity, f: field, v: value }}); }}
  function pixel_paint(entity, x, y, radius, material) {{ window.__jengine_play.commands.push({{ t: 'spxp', e: entity, x: x, y: y, r: radius, m: material }}); }}
  function set_sprite_fill(entity, amount) {{ window.__jengine_play.commands.push({{ t: 'ssfl', e: entity, a: amount }}); }}
  function set_water_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'swtf', e: entity, f: field, v: value === undefined ? null : value }}); }}
  function water_splash(entity, x, strength) {{ window.__jengine_play.commands.push({{ t: 'swsp', e: entity, x: x, s: strength }}); }}
  function set_canvas_group_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'scgf', e: entity, f: field, v: value }}); }}
  function set_video_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'svf', e: entity, f: field, v: value }}); }}
  function set_ragdoll_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'srdf', e: entity, f: field, v: value }}); }}
  function set_timeline_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'stlf', e: entity, f: field, v: value }}); }}
  function set_behavior_tree_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'sbtf', e: entity, f: field, v: value === undefined ? null : value }}); }}
  function set_area_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'saef', e: entity, f: field, v: value }}); }}
  // Navigation (`navigation`): agents are steered by command, paths are
  // answered by the host from its nav snapshot.
  function set_nav_target(entity, x, y) {{ window.__jengine_play.commands.push({{ t: 'snt', e: entity, x: x, y: y }}); }}
  function stop_nav(entity) {{ window.__jengine_play.commands.push({{ t: 'snst', e: entity }}); }}
  function set_nav_field(entity, field, value) {{ window.__jengine_play.commands.push({{ t: 'snvf', e: entity, f: field, v: value }}); }}
  function nav_find_path(x0, y0, x1, y1) {{ var s = window.__jengine_play.state; return (s && s.nav_find_path) ? s.nav_find_path(x0, y0, x1, y1) : null; }}
  // Rumble and vibration (`input::haptics`): queued by the host, played by
  // its gamepad poll.
  function input_rumble(pad, strong, weak, seconds) {{ var s = window.__jengine_play.state; if (s && s.input_rumble) s.input_rumble(pad || 0, strong || 0, weak || 0, seconds || 0); }}
  function input_vibrate(seconds, strength) {{ var s = window.__jengine_play.state; if (s && s.input_vibrate) s.input_vibrate(seconds || 0, (strength === undefined || strength === null) ? 1 : strength); }}
  // `Platform.*`: the web portal a build was made for, through the adapter
  // the page installs as `window.__jengine_platform` (the builder's
  // `assets/platform/`, docs/web-platforms.md). Nothing here reaches the
  // host. Without an adapter - the editor, a `none` build, QuickJS - they
  // answer as native does, and `show_ad` still answers with the event.
  function __platform_adapter() {{ return (typeof window !== 'undefined' && window.__jengine_platform) || null; }}
  function platform_name() {{ var p = __platform_adapter(); return p ? p.name : 'none'; }}
  function platform_game_ready() {{ var p = __platform_adapter(); if (p) p.gameReady(true); }}
  function platform_gameplay_start() {{ var p = __platform_adapter(); if (p) p.gameplayStart(); }}
  function platform_gameplay_stop() {{ var p = __platform_adapter(); if (p) p.gameplayStop(); }}
  function platform_happy_time() {{ var p = __platform_adapter(); if (p) p.happyTime(); }}
  function platform_submit_score(value, board) {{ var p = __platform_adapter(); if (p) p.submitScore(Math.floor(Number(value) || 0), board ? String(board) : ''); }}
  function platform_show_ad(kind, reward) {{
    var k = kind === 'rewarded' ? 'rewarded' : 'midgame';
    var p = __platform_adapter();
    if (p) {{ p.showAd(k, reward ? String(reward) : ''); return; }}
    window.__jengine_play.commands.push({{ t: 'evt', n: 'platform_ad_finished', e: 0, d: JSON.stringify({{ kind: k, shown: false, rewarded: false }}) }});
  }}
  // Localization (`locale`): answered by the host from the string tables.
  function locale_get(key, args) {{ var s = window.__jengine_play.state; return (s && s.locale_get) ? s.locale_get(key, args) : key; }}
  function locale_set(code) {{ var s = window.__jengine_play.state; if (s && s.locale_set) s.locale_set(code || ''); }}
  function locale_current() {{ var s = window.__jengine_play.state; return (s && s.locale_current) ? s.locale_current() : 'en'; }}
  function locale_list() {{ var s = window.__jengine_play.state; return (s && s.locale_list) ? s.locale_list() : []; }}
  function locale_name(code) {{ var s = window.__jengine_play.state; return (s && s.locale_name) ? s.locale_name(code) : code; }}
  // Display and cursor settings (`display`): one host hook, `state.display`.
  function __display(op, a, b, c) {{ var s = window.__jengine_play.state; return (s && s.display) ? s.display(op, a, b, c) : null; }}
  function display_set_mode(mode) {{ __display('set_mode', mode); }}
  function display_get_mode() {{ var r = __display('get_mode'); return r === null ? 'windowed' : r; }}
  function display_set_size(w, h) {{ __display('set_size', w, h); }}
  function display_get_size() {{ return __display('get_size') || [0, 0]; }}
  function display_get_resolutions() {{ return __display('get_resolutions') || []; }}
  function display_set_vsync(on) {{ __display('set_vsync', on); }}
  function display_get_vsync() {{ var r = __display('get_vsync'); return r === null ? true : r; }}
  function display_set_max_fps(n) {{ __display('set_max_fps', n); }}
  function display_get_max_fps() {{ return __display('get_max_fps') || 0; }}
  function display_supports(name) {{ return !!__display('supports', name); }}
  function cursor_set_visible(on) {{ __display('cursor_visible', on); }}
  function cursor_set_confined(on) {{ __display('cursor_confined', on); }}
  function cursor_set_locked(on) {{ __display('cursor_locked', on); }}
  function cursor_set_icon(name) {{ __display('cursor_icon', name); }}
  function cursor_set_image(texture_id, hot_x, hot_y) {{ __display('cursor_image', texture_id || '', hot_x || 0, hot_y || 0); }}
  // Input replays (`input::replay`): one host hook, `state.replay`.
  function __replay(op, arg) {{ var s = window.__jengine_play.state; return (s && s.replay) ? s.replay(op, String(arg || '')) : null; }}
  function replay_record() {{ __replay('record'); }}
  function replay_play() {{ return !!__replay('play'); }}
  function replay_stop() {{ __replay('stop'); }}
  function replay_clear() {{ __replay('clear'); }}
  function replay_status() {{ return __replay('status') || 'idle'; }}
  function replay_frames() {{ return __replay('frames') || 0; }}
  function replay_progress() {{ return __replay('progress') || 0; }}
  function replay_live_key(key) {{ return !!__replay('live_key', key); }}
  // Noise (`noise`): one host hook, `state.noise`, over jengine_core::noise -
  // the functions native calls, so a seed is the same field everywhere.
  function __noise(op, a, b, c, d, e, f) {{ var s = window.__jengine_play.state; return (s && s.noise) ? s.noise(op, a, b, c, d, e, f) : null; }}
  function noise_perlin(x, y, seed) {{ return __noise('perlin', x, y, seed) || 0; }}
  function noise_perlin3(x, y, z, seed) {{ return __noise('perlin3', x, y, z, seed) || 0; }}
  function noise_simplex(x, y, seed) {{ return __noise('simplex', x, y, seed) || 0; }}
  function noise_cellular(x, y, seed) {{ return __noise('cellular', x, y, seed) || [0, 0, 0]; }}
  function noise_fractal(x, y, opts) {{ return __noise('fractal', x, y, opts) || 0; }}
  function noise_grid(x0, y0, w, h, step, opts) {{ return __noise('grid', x0, y0, w, h, step, opts) || []; }}
  // The mix's spectrum and synthesised notes (`audio_analysis`): one hook,
  // `state.audio_analysis`, over jengine_core::audio::analysis.
  function __analysis(op, a) {{ var s = window.__jengine_play.state; return (s && s.audio_analysis) ? s.audio_analysis(op, a) : null; }}
  function audio_get_spectrum(n) {{ return __analysis('spectrum', n) || []; }}
  function audio_get_level() {{ return __analysis('level') || 0; }}
  function audio_synth(opts) {{ __analysis('synth', opts); }}
  // Screenshots (`screenshot`): one hook, `state.screenshot`, over
  // jengine_core::renderer::screenshot.
  function __shot(op, a) {{ var s = window.__jengine_play.state; return (s && s.screenshot) ? s.screenshot(op, a) : null; }}
  function screenshot_take(opts) {{ return __shot('take', opts) || 0; }}
  function screenshot_ready(id) {{ return !!__shot('ready', id); }}
  // Accessibility (`accessibility`): the screen reader's next words.
  function accessibility_announce(text) {{ var s = window.__jengine_play.state; if (s && s.accessibility_announce) s.accessibility_announce(String(text)); }}
  function accessibility_speech() {{ var s = window.__jengine_play.state; return (s && s.accessibility_speech) ? String(s.accessibility_speech()) : ''; }}
  // Scenes loaded in the background and laid into this one (`scene::streaming`).
  function __streaming(op, a, b, c) {{ var s = window.__jengine_play.state; return (s && s.streaming) ? s.streaming(op, a, b, c) : null; }}
  function scene_load_additive(path, x, y) {{ var h = __streaming('load_additive', path, x || 0, y || 0); return h === null ? 0 : h; }}
  function scene_unload(handle) {{ __streaming('unload', handle); }}
  function scene_preload(path) {{ __streaming('preload', path); }}
  function scene_status(of) {{ var r = __streaming('status', of); return r === null ? 'unloaded' : r; }}
  function scene_progress(of) {{ return __streaming('progress', of) || 0; }}
  // Asset packs downloaded when needed (`asset::packs`).
  function __packs(op, a) {{ var s = window.__jengine_play.state; return (s && s.packs) ? s.packs(op, a === undefined || a === null ? '' : String(a)) : null; }}
  function pack_download(name) {{ __packs('download', name); }}
  function pack_remove(name) {{ __packs('remove', name); }}
  function pack_set_url(url) {{ __packs('set_url', url); }}
  function pack_status(name) {{ var r = __packs('status', name); return r === null ? 'unknown' : r; }}
  function pack_progress(name) {{ return __packs('progress', name) || 0; }}
  function pack_size(name) {{ return __packs('size', name) || 0; }}
  function pack_error(name) {{ var r = __packs('error', name); return r === null ? '' : r; }}
  function pack_of(path) {{ var r = __packs('of', path); return r === null ? '' : r; }}
  function pack_list() {{ var r = __packs('list'); return r === null ? [] : r; }}
  // Live ops: remote config, experiments and events (`crate::liveops`).
  function __liveops_arg(a) {{ return (a === undefined || a === null) ? '' : String(a); }}
  function __liveops(op, a, b, c) {{ var s = window.__jengine_play.state; return (s && s.liveops) ? s.liveops(op, __liveops_arg(a), __liveops_arg(b), __liveops_arg(c)) : null; }}
  function liveops_get(key, d) {{ var r = __liveops('get', key); return (r === null || r === undefined) ? (d === undefined ? null : d) : r; }}
  function liveops_keys() {{ var r = __liveops('keys'); return r === null ? [] : r; }}
  function liveops_status() {{ var r = __liveops('status'); return r === null ? '' : r; }}
  function liveops_error() {{ var r = __liveops('error'); return r === null ? '' : r; }}
  function liveops_fetch() {{ __liveops('fetch'); }}
  function liveops_fetching() {{ var r = __liveops('fetching'); return r === null ? false : r; }}
  function liveops_variant(experiment) {{ var r = __liveops('variant', experiment); return r === null ? '' : r; }}
  function liveops_events() {{ var r = __liveops('events'); return r === null ? [] : r; }}
  function liveops_event_active(id) {{ var r = __liveops('event_active', id); return r === null ? false : r; }}
  function liveops_event_ends(id) {{ var r = __liveops('event_ends', id); return r === null ? 0 : r; }}
  function liveops_event_starts(id) {{ var r = __liveops('event_starts', id); return r === null ? 0 : r; }}
  function liveops_event_name(id) {{ var r = __liveops('event_name', id); return r === null ? '' : r; }}
  function liveops_set_player(id) {{ __liveops('set_player', id); }}
  function liveops_server_time() {{ var r = __liveops('server_time'); return r === null ? 0 : r; }}
  function liveops_revision() {{ var r = __liveops('revision'); return r === null ? 0 : r; }}
  function liveops_release() {{ var r = __liveops('release'); return r === null ? 0 : r; }}
  function liveops_updated() {{ var r = __liveops('updated'); return r === null ? 0 : r; }}
  function liveops_expose(experiment, event, value) {{ var r = __liveops('expose', experiment, event, value); return r === null ? false : r; }}
  function liveops_set_attribute(name, value) {{ __liveops('set_attribute', name, value); }}
  function nav_is_moving(entity) {{ var s = window.__jengine_play.state; return (s && s.nav_is_moving) ? s.nav_is_moving(entity) : false; }}
  function nav_closest_point(x, y) {{ var s = window.__jengine_play.state; return (s && s.nav_closest_point) ? s.nav_closest_point(x, y) : null; }}
  function get_particle_count() {{ return window.__jengine_play.particle_count || 0; }}
  function create_ui_text(text, anchor, offset_x, offset_y, r, g, b, a, font_size, alignment) {{
    // Trailing color/font/align args are optional. Defaults match
    // `UIText::default` (white / 16.0 / "left") so existing 4-arg call
    // sites stay unchanged.
    // UI elements share the spawn handles' counter, as natively.
    var id = window.__jengine_play.next_entity_id || 1000000;
    window.__jengine_play.next_entity_id = id + 1;
    window.__jengine_play.commands.push({{
      t: 'cut', id: id, txt: text, a: anchor, ox: offset_x, oy: offset_y,
      cr: (r === undefined ? 1.0 : +r),
      cg: (g === undefined ? 1.0 : +g),
      cb: (b === undefined ? 1.0 : +b),
      ca: (a === undefined ? 1.0 : +a),
      fs: (font_size === undefined ? 16.0 : +font_size),
      al: (alignment === undefined || alignment === null ? "left" : String(alignment))
    }});
    return id;
  }}
  function create_ui_panel(anchor, offset_x, offset_y, width, height, r, g, b, a) {{
    // Trailing RGBA optional; defaults to a translucent black panel
    // (matches the legacy viewport `cup` default before the styling
    // extension landed).
    // UI elements share the spawn handles' counter, as natively.
    var id = window.__jengine_play.next_entity_id || 1000000;
    window.__jengine_play.next_entity_id = id + 1;
    window.__jengine_play.commands.push({{
      t: 'cup', id: id, a: anchor, ox: offset_x, oy: offset_y, w: width, h: height,
      cr: (r === undefined ? 0.0 : +r),
      cg: (g === undefined ? 0.0 : +g),
      cb: (b === undefined ? 0.0 : +b),
      ca: (a === undefined ? 0.5 : +a)
    }});
    return id;
  }}
  function set_ui_text(entity, text) {{ window.__jengine_play.commands.push({{ t: 'sut', e: entity, txt: text }}); }}
  function set_text(entity, text) {{ window.__jengine_play.commands.push({{ t: 'st', e: entity, txt: text }}); }}
  function set_ui_visible(entity, visible) {{ window.__jengine_play.commands.push({{ t: 'suv', e: entity, v: !!visible }}); }}
  function set_ui_position(entity, anchor, offset_x, offset_y) {{ window.__jengine_play.commands.push({{ t: 'sup', e: entity, a: anchor, ox: offset_x, oy: offset_y }}); }}
  function gui_show(gui_id) {{ window.__jengine_play.commands.push({{ t: 'gs', gid: gui_id }}); }}
  function gui_hide(gui_id) {{ window.__jengine_play.commands.push({{ t: 'gh', gid: gui_id }}); }}
  function gui_set(gui_id, node_id, property, value) {{
    // A table from Lua arrives as an array or object: JSON, as native sends it.
    var v = (value !== null && typeof value === 'object') ? JSON.stringify(value) : String(value);
    window.__jengine_play.commands.push({{ t: 'gsp', gid: gui_id, nid: node_id, p: property, v: v }});
  }}
  function gui_get(gui_id, node_id, property) {{
    var s = window.__jengine_play.gui_state || {{}};
    var doc = s[gui_id]; if (!doc) return "";
    var node = doc[node_id]; if (!node) return "";
    var v = node[property];
    // Stringify. Native stores GUI props as strings (`gui_node_props` is a
    // HashMap<_, String>) and the docs promise `string`, but the host
    // hydrates JS-native types — so `gui_get(gui, node, "checked")` handed
    // back a boolean here and the string "true" natively. A script written
    // against one silently misbehaved on the other: `gui_get(...) ~= "true"`
    // is always true when the value is a boolean.
    return (v === undefined || v === null) ? "" : String(v);
  }}
  function gui_on_click(entity, arg2, arg3, arg4) {{ var gid,nid,cb; if(typeof arg4==='string'){{gid=arg2;nid=arg3;cb=arg4;}}else{{gid=(window.__jengine_play.gui_ids||{{}})[entity]||'';nid=arg2;cb='on_gui_click';}} window.__jengine_play.commands.push({{ t: 'goc', e: entity, gid: gid, nid: nid, cb: cb }}); }}
  function gui_focus(gui_id, node_id) {{ window.__jengine_play.commands.push({{ t: 'gfo', gid: gui_id || '', nid: node_id || '' }}); }}
  function apply_force(entity, fx, fy) {{ window.__jengine_play.commands.push({{ t: 'af', e: entity, fx: fx, fy: fy }}); }}
  function apply_impulse(entity, ix, iy) {{ window.__jengine_play.commands.push({{ t: 'ai', e: entity, ix: ix, iy: iy }}); }}
  function set_velocity(entity, vx, vy) {{ window.__jengine_play.commands.push({{ t: 'sv', e: entity, vx: vx, vy: vy }}); }}
  function get_velocity(entity) {{
    var s = window.__jengine_play.state;
    // A Float32Array indexed by entity id, `[vx, vy]` at `id * 2`, NaN where it has none.
    var id = __ent(entity), v = s && s.velocities;
    if (v && id * 2 + 1 < v.length && v[id * 2] === v[id * 2]) return [v[id * 2], v[id * 2 + 1]];
    return [0, 0];
  }}
  function set_angular_velocity(entity, omega) {{ window.__jengine_play.commands.push({{ t: 'sav', e: entity, o: omega }}); }}
  function get_angular_velocity(entity) {{
    var s = window.__jengine_play.state;
    var id = __ent(entity), w = s && s.angular_velocities;
    if (w && id < w.length && w[id] === w[id]) return w[id];
    return 0;
  }}
  function set_gravity(gx, gy) {{ window.__jengine_play.commands.push({{ t: 'sg', gx: gx, gy: gy }}); }}
  function get_gravity() {{ var s = window.__jengine_play.state; return (s && s.gravity) ? s.gravity : [0, -9.81]; }}
  function set_body_type(entity, body_type) {{ window.__jengine_play.commands.push({{ t: 'sbt', e: entity, bt: body_type }}); }}
  function get_contacts() {{ var s = window.__jengine_play.state; return (s && s.contacts) ? s.contacts : []; }}
  function raycast(ox, oy, dx, dy, max_dist) {{ var s = window.__jengine_play.state; if (s && s.physics_raycast) return s.physics_raycast(ox, oy, dx, dy, max_dist); return null; }}
  // Shape queries, answered by the host from its ray snapshot (`physics::query`).
  function overlap_circle(x, y, radius) {{ var s = window.__jengine_play.state; return (s && s.physics_overlap) ? s.physics_overlap(0, x, y, radius, 0, 0) : []; }}
  function overlap_box(x, y, width, height, rotation) {{ var s = window.__jengine_play.state; return (s && s.physics_overlap) ? s.physics_overlap(1, x, y, width, height, rotation || 0) : []; }}
  function overlap_point(x, y) {{ var s = window.__jengine_play.state; return (s && s.physics_overlap) ? s.physics_overlap(2, x, y, 0, 0, 0) : []; }}
  function circle_cast(ox, oy, dx, dy, radius, max_dist) {{ var s = window.__jengine_play.state; if (s && s.physics_circle_cast) return s.physics_circle_cast(ox, oy, dx, dy, radius, max_dist); return null; }}
  function add_joint(entity_name, target_name, joint_json) {{ window.__jengine_play.commands.push({{ t: 'aj', en: entity_name, tn: target_name, jj: joint_json }}); }}
  function remove_joint(entity_name) {{ window.__jengine_play.commands.push({{ t: 'rj', en: entity_name }}); }}
  function set_motor_speed(entity_name, speed) {{ window.__jengine_play.commands.push({{ t: 'sms', en: entity_name, sp: speed }}); }}
  // `pan` is -1.0 hard left to 1.0 hard right; omitted means centre, so
  // existing calls are unchanged.
  // A sound's id, handed back at once; the host records which sound it
  // became (`audio::music::alias`), so stop / volume / pitch reach it.
  function __sound_id() {{ var p = window.__jengine_play; p.__sid = (p.__sid || 1000000) + 1; return p.__sid; }}
  function play_sound(clip_id, volume, pitch, pan, loop) {{
    var sid = __sound_id();
    window.__jengine_play.commands.push({{
      t: 'ps', c: clip_id, v: volume || 1.0, p: pitch || 1.0, pan: pan || 0.0, sid: sid, l: loop === true
    }});
    return sid;
  }}
  // A one-shot heard from where it happens: the host works out its volume
  // and pan from the listener (`audio::spatial`).
  function play_sound_at(clip_id, x, y, volume, pitch) {{
    var sid = __sound_id();
    window.__jengine_play.commands.push({{
      t: 'psa', c: clip_id, x: x, y: y, v: volume || 1.0, p: pitch || 1.0, sid: sid
    }});
    return sid;
  }}
  function play_music(clip_id, volume, looping) {{ window.__jengine_play.commands.push({{ t: 'pm', c: clip_id, v: volume || 1.0, l: looping !== false }}); }}
  // Crossfades, stem layers, playlists, pitch and seek: one hook over the
  // engine's music director (`audio::music`), as native calls it.
  function __music(op, args) {{ var s = window.__jengine_play.state; return (s && s.music) ? s.music(op, args) : null; }}
  function audio_crossfade_music(clip, seconds, volume, looping) {{ __music('crossfade', [clip, seconds, volume, looping]); }}
  function audio_play_music_layers(clips, volumes, seconds) {{ __music('layers', [clips, volumes, seconds]); }}
  function audio_set_music_layer(index, volume, seconds) {{ __music('layer', [index, volume, seconds]); }}
  function audio_set_playlist(clips, options) {{ __music('playlist', [clips, options]); }}
  function audio_next_track() {{ __music('next', []); }}
  function audio_set_music_pitch(pitch) {{ __music('pitch', [pitch]); }}
  function audio_seek_music(seconds) {{ __music('seek', [seconds]); }}
  function audio_set_sound_pitch(sound, pitch) {{ __music('sound_pitch', [sound, pitch]); }}
  function audio_get_music_clip() {{ return __music('clip', []) || ''; }}
  function audio_get_music_layer(index) {{ return __music('layer_volume', [index]) || 0; }}
  function stop_sound(handle_id) {{ window.__jengine_play.commands.push({{ t: 'sts', h: handle_id }}); }}
  function stop_music() {{ window.__jengine_play.commands.push({{ t: 'stm' }}); }}
  function pause_music() {{ window.__jengine_play.commands.push({{ t: 'pam' }}); }}
  function resume_music() {{ window.__jengine_play.commands.push({{ t: 'rem' }}); }}
  function set_sound_volume(handle_id, volume) {{ window.__jengine_play.commands.push({{ t: 'ssv', h: handle_id, v: volume }}); }}
  function set_music_volume(volume) {{ window.__jengine_play.commands.push({{ t: 'smv', v: volume }}); }}
  function set_master_volume(volume) {{ window.__jengine_play.commands.push({{ t: 'smav', v: volume }}); }}
  function stop_all_sounds() {{ window.__jengine_play.commands.push({{ t: 'stal' }}); }}
  function stop_sound_by_clip(clip_id) {{
    window.__jengine_play.commands.push({{ t: 'ssbc', c: clip_id }});
  }}
  function pause_all_sounds() {{ window.__jengine_play.commands.push({{ t: 'paal' }}); }}
  function resume_all_sounds() {{ window.__jengine_play.commands.push({{ t: 'real' }}); }}
  function get_current_animation(entity) {{
    var clips = window.__jengine_play.animation_clip_names || {{}};
    return clips[entity] || '';
  }}
  function set_anim_bool(entity, param, value) {{
    window.__jengine_play.commands.push({{ t: 'sab', e: entity, param: param, value: !!value }});
  }}
  function set_anim_float(entity, param, value) {{
    window.__jengine_play.commands.push({{ t: 'saf', e: entity, param: param, value: value }});
  }}
  function set_anim_trigger(entity, param) {{
    window.__jengine_play.commands.push({{ t: 'sat', e: entity, param: param }});
  }}
  function start_state_machine(entity, state_name) {{
    window.__jengine_play.commands.push({{ t: 'ssm', e: entity, state: state_name }});
  }}
  function get_anim_state(entity) {{
    var states = window.__jengine_play.anim_states || {{}};
    return states[entity] || '';
  }}
  function get_sprite_anchor(entity) {{
    var a = window.__jengine_play.sprite_anchors || {{}};
    return a[entity] || [0.5, 0.5];
  }}
  // Draw order of an entity's sprite and text, changed at run time: the
  // hovered card of a hand on top of its neighbours.
  function set_z_order(entity, z) {{
    window.__jengine_play.commands.push({{ t: 'szo', e: entity, z: z }});
  }}
  // A CharacterController2D: the velocity it asks the next physics step for,
  // and what it did in the last one.
  function move_character(entity, vx, vy, drop) {{
    var c = {{ t: 'mvc', e: entity, vx: vx, vy: vy }};
    if (drop !== undefined && drop !== null) c.drop = !!drop;
    window.__jengine_play.commands.push(c);
  }}
  function get_character(entity) {{
    var s = window.__jengine_play.state;
    if (!s || !s.characters) return null;
    for (var i = 0; i < s.characters.length; i++) {{
      var r = s.characters[i];
      if (r[0] === entity) return {{ grounded: r[1], ground_nx: r[2], ground_ny: r[3],
        ground_entity: r[4] < 0 ? null : r[4], wall: r[5], ceiling: r[6], sliding: r[7], vx: r[8], vy: r[9],
        touching: r[10] }};
    }}
    return null;
  }}
  // The colour of an entity's world text: a judgement popup, a warning.
  function set_text_color(entity, r, g, b, a) {{
    window.__jengine_play.commands.push({{ t: 'stc', e: entity, r: r, g: g, b: b, a: (a === undefined || a === null) ? 1 : a }});
  }}
  // A trail: stop or start recording points, or forget them all.
  // The entity's mesh: a list of vertices ({{ x, y, u, v }} or keyed) and
  // 1-based triangle indices (none is a fan). `mesh_from_script` reads both.
  function set_mesh(entity, vertices, indices) {{
    window.__jengine_play.commands.push({{ t: 'smsh', e: entity, vs: vertices, ix: indices }});
  }}
  function set_trail_emitting(entity, on) {{
    window.__jengine_play.commands.push({{ t: 'tre', e: entity, on: !!on }});
  }}
  function clear_trail(entity) {{
    window.__jengine_play.commands.push({{ t: 'trc', e: entity }});
  }}
  // A hole in a carvable sprite, in world units (`ecs::carve`).
  function carve_sprite(entity, x, y, radius) {{
    window.__jengine_play.commands.push({{ t: 'scv', e: entity, x: x, y: y, r: radius }});
  }}
  // A stamp painted into a paintable sprite, in world units (`ecs::paint`).
  function paint_sprite(entity, x, y, size, r, g, b, a, shape, rotation, blend) {{
    window.__jengine_play.commands.push({{ t: 'spnt', e: entity, x: x, y: y, s: size,
      cr: r === undefined ? 1 : r, cg: g === undefined ? 1 : g, cb: b === undefined ? 1 : b, ca: a === undefined ? 1 : a,
      sh: shape || '', rot: rotation || 0, bl: blend || '' }});
  }}
  // A dent (negative depth: a ridge) in a paintable sprite's normal map.
  function press_sprite(entity, x, y, size, depth, shape, rotation) {{
    window.__jengine_play.commands.push({{ t: 'spnt', e: entity, x: x, y: y, s: size,
      dp: depth === undefined ? 1 : depth, sh: shape || 'disc', rot: rotation || 0 }});
  }}
  function set_sprite_anchor(entity, ax, ay) {{
    window.__jengine_play.commands.push({{ t: 'ssa', e: entity, ax: ax, ay: ay }});
  }}
  function set_sprite_size(entity, w, h) {{
    window.__jengine_play.commands.push({{ t: 'sssz', e: entity, w: w, h: h }});
  }}
  // Swap a sprite to a named region of a runtime-loaded atlas. Mirrors the
  // native `set_sprite_region(entity, atlas_id, region_name)` bridge; the
  // host drains `ssr` and looks the UV rect up from the loaded AtlasData.
  function set_sprite_label(entity, label, category) {{ window.__jengine_play.commands.push({{ t: 'spl', e: entity, l: String(label), c: category == null ? null : String(category) }}); }}
  function set_sprite_region(entity, atlas_id, region_name) {{
    window.__jengine_play.commands.push({{ t: 'ssr', e: entity, a: atlas_id, r: region_name }});
  }}
  // Draw a sprite as an explicit quad of 4 local-space [x,y] corner offsets
  // (order TL,TR,BR,BL) from the entity position. Accepts BOTH call shapes so
  // every front-end agrees: Lua/Fengari (and transpiled-Lua) pass 8 scalars
  // `(e, x0,y0, x1,y1, x2,y2, x3,y3)`; hand-written JS passes a flat array
  // `(e, [x0,y0, ...])`. The host drains `scor` and sets sprite.corners.
  function set_sprite_corners(entity, a, b, c2, d, e2, f, g, h) {{
    var v = Array.isArray(a) ? a : [a, b, c2, d, e2, f, g, h];
    window.__jengine_play.commands.push({{ t: 'scor', e: entity, x0: v[0], y0: v[1], x1: v[2], y1: v[3], x2: v[4], y2: v[5], x3: v[6], y3: v[7] }});
  }}
  function get_sprite_point(entity, name) {{
    var pts = (window.__jengine_play.sprite_points || {{}})[entity] || {{}};
    return pts[name] || [0, 0];
  }}
  function set_material(entity, material_id) {{
    window.__jengine_play.commands.push({{ t: 'smat', e: entity, m: material_id }});
  }}
  function set_normal_map(entity, normal_map_id) {{
    window.__jengine_play.commands.push({{ t: 'snm', e: entity, m: (normal_map_id === undefined || normal_map_id === null) ? '' : normal_map_id }});
  }}
  function get_material(entity) {{
    var m = window.__jengine_play.materials || {{}};
    var v = m[entity];
    return (v === undefined || v === null) ? null : v;
  }}
  function set_material_uniform_float(entity, name, value) {{
    window.__jengine_play.commands.push({{ t: 'smu', e: entity, n: name, ut: 'f', v: value }});
  }}
  function set_material_uniform_vec2(entity, name, x, y) {{
    window.__jengine_play.commands.push({{ t: 'smu', e: entity, n: name, ut: 'v2', v: [x, y] }});
  }}
  function set_material_uniform_vec4(entity, name, x, y, z, w) {{
    window.__jengine_play.commands.push({{ t: 'smu', e: entity, n: name, ut: 'v4', v: [x, y, z, w] }});
  }}
  function set_material_uniform_int(entity, name, value) {{
    window.__jengine_play.commands.push({{ t: 'smu', e: entity, n: name, ut: 'i', v: value }});
  }}
  function set_post_process_enabled(effect_name, enabled) {{
    window.__jengine_play.commands.push({{ t: 'sppe', n: effect_name, v: enabled }});
  }}
  function set_post_process_param_float(effect_name, param_name, value) {{
    window.__jengine_play.commands.push({{ t: 'sppp', n: effect_name, p: param_name, ut: 'f', v: value }});
  }}
  function set_post_process_param_vec4(effect_name, param_name, x, y, z, w) {{
    window.__jengine_play.commands.push({{ t: 'sppp', n: effect_name, p: param_name, ut: 'v4', v: [x, y, z, w] }});
  }}
  // Lighting setters (P7.7 wedge 2) — push commands applied against the first
  // matching light component in the scene.
  function set_ambient_light(r, g, b, intensity) {{
    window.__jengine_play.commands.push({{ t: 'sal', r: r, g: g, b: b, i: intensity }});
  }}
  function set_light_color(entity, r, g, b) {{
    window.__jengine_play.commands.push({{ t: 'slc', e: entity, r: r, g: g, b: b }});
  }}
  function set_light_intensity(entity, intensity) {{
    window.__jengine_play.commands.push({{ t: 'sli', e: entity, i: intensity }});
  }}
  // How high a light stands (the tangent of its elevation); a sun's shadows
  // lengthen as it lowers.
  // The frame's colour grade: a LUT strip and its strength ('' removes it).
  function set_color_lut(texture, strength) {{
    window.__jengine_play.commands.push({{ t: 'slut', id: texture || '', s: strength == null ? 1 : strength }});
  }}
  function set_light_mask(entity, mask) {{
    window.__jengine_play.commands.push({{ t: 'slm', e: entity, m: mask }});
  }}
  function set_light_height(entity, height) {{
    window.__jengine_play.commands.push({{ t: 'slh', e: entity, h: height }});
  }}
  // The scene's toon lighting (bands 0 removes it) and ambient occlusion
  // (strength 0 removes it); nil arguments are the components' defaults.
  function set_toon_lighting(bands, softness, dither) {{
    window.__jengine_play.commands.push({{ t: 'stoon', b: bands, s: softness, d: dither }});
  }}
  function set_global_illumination(strength, scale) {{
    window.__jengine_play.commands.push({{ t: 'sgi', s: strength, k: scale }});
  }}
  function set_ambient_occlusion(radius, strength) {{
    window.__jengine_play.commands.push({{ t: 'sao', r: radius, s: strength }});
  }}
  function set_light_radius(entity, radius) {{
    window.__jengine_play.commands.push({{ t: 'slr', e: entity, r: radius }});
  }}
  // Contact iteration helpers — bridge shape (native uses indexed access).
  function contact_count() {{
    var s = window.__jengine_play.state;
    return (s && s.contacts) ? s.contacts.length : 0;
  }}
  // 1-based, as native's and as `for i = 1, contact_count()` loops expect.
  // This indexed from 0, so it skipped the first contact and answered nil
  // for the last.
  function get_contact(i) {{
    var s = window.__jengine_play.state;
    if (!s || !s.contacts || !s.contacts[i - 1]) return null;
    return s.contacts[i - 1];
  }}
  // --- Native bridge name aliases (P7.7 wedge 1) ---
  // Browser shim historically used different names than the native bridge.
  // Adding aliases so scripts written against the native surface
  // (`docs/scripting-api.md`) work portably across both targets.
  // `find_by_name` answers null (nil in Lua) for a name nothing has, as
  // native does and the docs say; `get_entity_by_name` keeps the browser's
  // old -1. Until Oct 2026 this was an alias of it, so a script's
  // `if e == nil then break end` never broke in the editor or on the web -
  // a game's slot loop spun forever on Play and froze the editor.
  function find_by_name(name) {{
    var idx = window.__jengine_play.names[name];
    return idx !== undefined ? idx : null;
  }}
  var find_by_tag = get_entities_by_tag;
  var set_post_process_float = set_post_process_param_float;
  var set_post_process_vec4 = set_post_process_param_vec4;
  // Namespaces (`Sprite.set_color`) are not declared here: they are built
  // from `scripting::namespaces` - by the Luau adapter natively, and by
  // `lua-runtime.ts` from the list the hosts publish. The hand-written
  // lowercase tables that stood here existed in the browser alone.
  // Package-contributed globals. Declared here so they share the scope
  // `ns_lookup` evaluates against - a package global outside it is simply
  // undefined in a browser, which is the whole bug this closes.
{}
  {}
  // Capture the script-defined `on_event` callback AFTER the script source has
  // been inserted. This handles both:
  //   - `function on_event(...)` (function declaration, hoisted to IIFE scope top)
  //   - `var on_event = function(...)` (expression, assigned during source execution)
  // In both cases `on_event` is the script's callback at this point.
  // We then overwrite `on_event` with `register_event_listener` so that any
  // call to `on_event(name, ent)` inside `init` correctly registers a listener.
  var __script_on_event = (typeof on_event === 'function') ? on_event : null;
  var on_event = register_event_listener;
  return {{
    init: typeof init === 'function' ? init : null,
    update: typeof update === 'function' ? update : null,
    on_event: (typeof __script_on_event === 'function') ? __script_on_event : null,
    on_gui_click: typeof on_gui_click === 'function' ? on_gui_click : null,
    // P7.7 wedge 15: resolve arbitrary script-author callback names declared
    // in this IIFE scope, so per-handler `gui_on_click(... , "on_resume")`
    // can route to the named export instead of always firing `on_gui_click`.
    lookup: function(name) {{
      try {{ var fn = eval(name); return (typeof fn === 'function') ? fn : null; }}
      catch (_e) {{ return null; }}
    }},
    ns_lookup: function(name) {{
      try {{ var v = eval(name); return (v != null) ? v : null; }}
      catch (_e) {{ return null; }}
    }}
  }};
}})()
"#,
        package_globals_js(),
        source
    )
}

/// JS declaring the loaded packages' script globals, spliced into every
/// script wrapper.
///
/// Set once by the runtime after its `PackageHost` initialises, because
/// packages are only known then. Empty until it is — a build with no packages
/// pays nothing, and a wrapper compiled before registration simply has no
/// package globals, which is correct rather than broken.
static PACKAGE_GLOBALS_JS: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Publish the package globals for subsequent script compiles.
///
/// Call after `PackageHost::initialize` and before any script compiles.
/// Scripts already compiled keep the globals they were built with; this is a
/// once-at-startup switch, not a hot path.
pub fn set_package_globals_js(js: String) {
    match PACKAGE_GLOBALS_JS.write() {
        Ok(mut slot) => *slot = js,
        // Poisoned only if a previous writer panicked. Package globals are
        // worth less than the run, so carry on without them.
        Err(e) => {
            log::error!("[package] could not publish script globals: {e}");
        }
    }
}

fn package_globals_js() -> String {
    PACKAGE_GLOBALS_JS
        .read()
        .map(|s| s.clone())
        .unwrap_or_default()
}
