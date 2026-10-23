//! The browser hosts' running scripts: one compiled instance per scripted
//! entity, the spawn map that turns a script's handle into an entity, and the
//! calls into a script - `init` / `update`, a named lifecycle handler, an event
//! listener, `fixed_update`.
//!
//! The editor viewport and the web runtime each carried all of this, and the
//! copies differed only in where a warning went (`docs/architecture-debt.md`,
//! #2). A host keeps its instances in its own `HashMap<u32, ScriptInstance>`
//! and lends it to these functions with a `warn` sink; the spawn map is a
//! thread-local here, one per host since each is its own wasm module.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use hecs::{Entity, World};
use wasm_bindgen::{JsCast, JsValue};

use super::BrowserScriptFns;
use crate::scripting::spawn_map::SpawnMap;

/// A compiled browser script attached to one entity.
#[derive(Clone)]
pub struct ScriptInstance {
    pub fns: BrowserScriptFns,
    /// What errors name the script by: its source path, or `entity_<id>`.
    pub label: String,
}

/// A host's running scripts, by entity id.
pub type Instances = HashMap<u32, ScriptInstance>;

thread_local! {
    /// Script-side handle (returned by `spawn_entity` / `spawn_prefab` /
    /// `create_ui_*`) -> the entity it names. Cleared when a world is
    /// replaced, so a stale handle cannot reach into the next one. A
    /// `SpawnMap`, which keeps its reversed views built once per change.
    pub static SCRIPT_ENTITY_MAP: RefCell<SpawnMap> = RefCell::new(SpawnMap::new());
}

/// Resolve a script's entity id: its spawn handle first, then a live id.
pub fn find_entity_by_id(world: &World, id: u32) -> Option<Entity> {
    if let Some(e) = SCRIPT_ENTITY_MAP.with(|m| m.borrow().get(&id).copied())
        && world.contains(e)
    {
        return Some(e);
    }
    world.iter().find(|e| e.entity().id() == id).map(|e| e.entity())
}

/// Record a script-spawned entity under the handle the script holds.
pub fn note_spawn(handle: u32, entity: Entity) {
    SCRIPT_ENTITY_MAP.with(|m| {
        m.borrow_mut().insert(handle, entity);
    });
}

/// Entity id -> the handle a script holds for it, for results that name an
/// entity (ray hits, contacts, velocity rows).
pub fn script_names() -> Rc<HashMap<u64, u64>> {
    SCRIPT_ENTITY_MAP.with(|m| m.borrow().by_id())
}

/// Compile one script with the browser adapter. `language` is `"Lua"` or
/// anything else for JavaScript; the label gets the extension the adapter
/// picks its compiler by.
pub fn compile_browser_script(label: &str, source: &str, language: &str) -> Result<BrowserScriptFns, String> {
    use crate::scripting::bridge::ScriptAdapter;
    use crate::scripting::bridge::adapters::JsAdapter;
    let mut adapter = JsAdapter::new();
    let is_lua = language.eq_ignore_ascii_case("Lua");
    let lower = label.to_ascii_lowercase();
    let path_hint: String = if is_lua && !lower.ends_with(".lua") && !lower.ends_with(".luau") {
        format!("{label}.lua")
    } else if !is_lua && !lower.ends_with(".js") && !lower.ends_with(".mjs") {
        format!("{label}.js")
    } else {
        label.to_string()
    };
    let handle = adapter.load(&path_hint, source).map_err(|e| e.message)?;
    adapter.clone_script_functions(handle).ok_or_else(|| "compile returned no fns".to_string())
}

/// Call a compiled script function as `(entity_id, dt)`. A missing function
/// is not an error; a throw comes back as its message, with a line number
/// when there is one.
pub fn call_script_fn(fn_value: &JsValue, entity_id: u32, dt: f32) -> Result<(), String> {
    if !fn_value.is_function() {
        return Ok(());
    }
    let f: &js_sys::Function = fn_value.unchecked_ref();
    match f.call2(&JsValue::NULL, &JsValue::from_f64(entity_id as f64), &JsValue::from_f64(dt as f64)) {
        Ok(_) => Ok(()),
        Err(e) => Err(super::format_script_error(&e, 0, "")),
    }
}

/// The script's own function called `name`, if it defines one.
pub fn handler(inst: &ScriptInstance, name: &str) -> Option<js_sys::Function> {
    if !inst.fns.lookup.is_function() {
        return None;
    }
    let lookup: &js_sys::Function = inst.fns.lookup.unchecked_ref();
    let f = lookup.call1(&JsValue::NULL, &JsValue::from_str(name)).ok()?;
    f.is_function().then(|| f.unchecked_into())
}

fn message(e: &JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}

/// Call `handler(entity)` on `eid`'s script - `on_despawn`, `on_screen_entered`
/// and the rest of what `ecs::callbacks` owes - if it has one.
pub fn call_handler(instances: &Instances, eid: u32, name: &str, warn: &mut dyn FnMut(String)) {
    let Some(inst) = instances.get(&eid) else { return };
    let Some(f) = handler(inst, name) else { return };
    if let Err(e) = f.call1(&JsValue::NULL, &JsValue::from_f64(eid as f64)) {
        warn(format!("{name} '{}' failed: {}", inst.label, message(&e)));
    }
}

/// `on_animation_event(entity, { name, data })` on `eid`'s script.
pub fn call_animation_event(
    instances: &Instances,
    eid: u32,
    name: &str,
    data: Option<&serde_json::Value>,
    warn: &mut dyn FnMut(String),
) {
    let Some(inst) = instances.get(&eid) else { return };
    let Some(f) = handler(inst, "on_animation_event") else { return };
    let payload = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&payload, &JsValue::from_str("name"), &JsValue::from_str(name));
    if let Some(d) = data {
        let _ = js_sys::Reflect::set(&payload, &JsValue::from_str("data"), &super::mirror::json_to_js(d));
    }
    if let Err(e) = f.call2(&JsValue::NULL, &JsValue::from_f64(eid as f64), &payload) {
        warn(format!("on_animation_event '{}' failed: {}", inst.label, message(&e)));
    }
}

/// Call the listeners of event `name` - the entities `add_event_listener`
/// recorded in `__jengine_play.event_listeners`, or only `only` for
/// `send_event_to`.
///
/// **Two spellings, because both are in use.** `on_<event>(entity, data)` is
/// the documented one and what native calls; the browser hosts called a
/// function named `on_event`. Specific name first, `on_event` as the
/// fallback, so both keep working (BUGS #150). The specific handler gets its
/// payload as a table; `on_event` keeps its four-argument form
/// (`host_commands::event_handler_args`).
pub fn dispatch_event(
    instances: &Instances,
    name: &str,
    sender: u32,
    data_js: &JsValue,
    only: Option<u32>,
    warn: &mut dyn FnMut(String),
) {
    let listener_ids: Vec<u32> = super::mirror::play_namespace()
        .and_then(|p| js_sys::Reflect::get(&p, &JsValue::from_str("event_listeners")).ok())
        .and_then(|v| {
            v.is_object()
                .then(|| js_sys::Reflect::get(&v.unchecked_into::<js_sys::Object>(), &JsValue::from_str(name)).ok())
                .flatten()
        })
        .filter(|v| v.is_instance_of::<js_sys::Array>())
        .map(|v| {
            let arr = v.unchecked_into::<js_sys::Array>();
            (0..arr.length()).filter_map(|i| arr.get(i).as_f64().map(|f| f as u32)).collect()
        })
        .unwrap_or_default();
    let specific = if name.starts_with("on_") { name.to_string() } else { format!("on_{name}") };
    for lid in listener_ids {
        if only.is_some_and(|t| t != lid) {
            continue;
        }
        let Some(inst) = instances.get(&lid) else { continue };
        let (target, is_specific) = match handler(inst, &specific) {
            Some(f) => (JsValue::from(f), true),
            None => (inst.fns.on_event.clone(), false),
        };
        if !target.is_function() {
            continue;
        }
        let f: &js_sys::Function = target.unchecked_ref();
        let args = crate::scripting::host_commands::event_handler_args(is_specific, lid, name, sender, data_js);
        if let Err(e) = f.apply(&JsValue::NULL, &args) {
            warn(format!("on_event '{}' for '{name}' failed: {}", inst.label, message(&e)));
        }
    }
}

/// Every running script's `fixed_update`, by entity id.
pub fn fixed_update_fns(instances: &Instances) -> Vec<(u32, JsValue)> {
    let mut out: Vec<(u32, JsValue)> =
        instances.iter().filter_map(|(eid, inst)| handler(inst, "fixed_update").map(|f| (*eid, f.into()))).collect();
    out.sort_by_key(|(eid, _)| *eid);
    out
}

/// A contact the physics step reported, for `on_collision_enter` / `_exit`:
/// `(a, b, started, impulse, x, y)`.
pub type Contact = (u32, u32, bool, f32, f32, f32);

/// Call `name(entity, data)` on `eid`'s script, `data` an object of numbers.
fn call_with(instances: &Instances, eid: u32, name: &str, data: &[(&str, f64)], warn: &mut dyn FnMut(String)) {
    let Some(inst) = instances.get(&eid) else { return };
    let Some(f) = handler(inst, name) else { return };
    let obj = js_sys::Object::new();
    for (key, value) in data {
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(key), &JsValue::from_f64(*value));
    }
    if let Err(e) = f.call2(&JsValue::NULL, &JsValue::from_f64(eid as f64), &obj) {
        warn(format!("{name} '{}' failed: {}", inst.label, message(&e)));
    }
}

/// The scripts' part of a frame, after the host has published the world and
/// `dt`, and before it drains their commands:
///
/// - `on_spawn` once per entity, on the first frame after its `init` - by
///   then every other entity in the scene exists, which is why it is not
///   part of `init`;
/// - every `update(entity, dt)`;
/// - the timers `set_timer` / `set_interval` queued, on the scaled `dt`;
/// - trigger crossings: `on_trigger_enter` / `_exit(zone, { other })` on the
///   zone and `on_entered_trigger` / `on_exited_trigger(e, { trigger })` on
///   whatever crossed;
/// - this step's contacts: `on_collision_enter` / `_exit(e, { other,
///   impulse, x, y })` on both sides. No zero guard - `0` is the first
///   entity a world spawns, not a sentinel.
pub fn run_frame(
    instances: &Instances,
    spawn_notified: &mut std::collections::HashSet<u32>,
    world: &World,
    dt: f32,
    contacts: &[Contact],
    warn: &mut dyn FnMut(String),
) {
    let mut ids: Vec<u32> = instances.keys().copied().collect();
    ids.sort_unstable();
    for &eid in &ids {
        if spawn_notified.insert(eid) {
            call_handler(instances, eid, "on_spawn", warn);
        }
    }
    for &eid in &ids {
        let inst = &instances[&eid];
        if let Err(e) = call_script_fn(&inst.fns.update, eid, dt) {
            warn(format!("update '{}' failed: {e}", inst.label));
        }
    }
    if let Some(play) = super::mirror::play_namespace() {
        for (eid, cb) in crate::scripting::timers::advance_play_timers(&play, dt) {
            call_handler(instances, eid, &cb, warn);
        }
    }
    for (zone, overlaps) in world.query::<&crate::ecs::components::TriggerOverlaps>().iter() {
        let zone_id = zone.id();
        for (crossings, enter) in [(&overlaps.entered, true), (&overlaps.exited, false)] {
            for &bits in crossings {
                let other_id = Entity::from_bits(bits).map(|e| e.id()).unwrap_or(0);
                let (on_zone, on_other) = if enter {
                    ("on_trigger_enter", "on_entered_trigger")
                } else {
                    ("on_trigger_exit", "on_exited_trigger")
                };
                call_with(instances, zone_id, on_zone, &[("other", other_id as f64)], warn);
                call_with(instances, other_id, on_other, &[("trigger", zone_id as f64)], warn);
            }
        }
    }
    for &(a, b, started, impulse, x, y) in contacts {
        let name = if started { "on_collision_enter" } else { "on_collision_exit" };
        for (me, other) in [(a, b), (b, a)] {
            let data = [("other", other as f64), ("impulse", impulse as f64), ("x", x as f64), ("y", y as f64)];
            call_with(instances, me, name, &data, warn);
        }
    }
}

/// `gui_on_click` bindings: `(document, node)` -> `(entity, handler)`.
pub type GuiClickBindings = HashMap<(String, String), (u32, String)>;

/// Call the handler bound to a click on `(gui_id, node_id)` as
/// `handler(entity, node_id)`, as native does - the document is implicit in
/// where it was bound. The default `on_gui_click` has its own slot; any other
/// name is looked up. `false` when nothing was bound or it did not run.
pub fn call_gui_click(
    instances: &Instances,
    bindings: &GuiClickBindings,
    gui_id: &str,
    node_id: &str,
    warn: &mut dyn FnMut(String),
) -> bool {
    let Some((eid, cb)) = bindings.get(&(gui_id.to_string(), node_id.to_string())) else { return false };
    let Some(inst) = instances.get(eid) else { return false };
    let target: JsValue = if cb == "on_gui_click" || cb.is_empty() {
        inst.fns.on_gui_click.clone()
    } else {
        handler(inst, cb).map(JsValue::from).unwrap_or(JsValue::NULL)
    };
    if !target.is_function() {
        warn(format!("gui click '{gui_id}/{node_id}' - callback '{cb}' missing on '{}'", inst.label));
        return false;
    }
    let f: &js_sys::Function = target.unchecked_ref();
    if let Err(e) = f.call2(&JsValue::NULL, &JsValue::from_f64(*eid as f64), &JsValue::from_str(node_id)) {
        warn(format!("gui click '{gui_id}/{node_id}' on '{}' failed: {}", inst.label, message(&e)));
        return false;
    }
    true
}

/// Compiled scripts by source path, so a script on many entities is compiled
/// once a scene.
pub type CompileCache = HashMap<String, BrowserScriptFns>;

/// What errors call a script: its source path, or `entity_<n>` for one with
/// no file.
pub fn label_for(source_path: &str, fallback: impl std::fmt::Display) -> String {
    if source_path.is_empty() { format!("entity_{fallback}") } else { source_path.to_string() }
}

/// A script's functions, compiled once per path (`cache`); a script with no
/// path is compiled every time.
pub fn compile_cached(
    cache: &mut CompileCache,
    source_path: &str,
    label: &str,
    source: &str,
    language: &str,
) -> Result<BrowserScriptFns, String> {
    if !source_path.is_empty()
        && let Some(f) = cache.get(source_path)
    {
        return Ok(f.clone());
    }
    let f = compile_browser_script(label, source, language)?;
    if !source_path.is_empty() {
        cache.insert(source_path.to_string(), f.clone());
    }
    Ok(f)
}

/// Compile `entity_id`'s script (through the cache) and attach it, or warn
/// that it did not compile. The instance is returned for `run_inits`.
#[allow(clippy::too_many_arguments)]
pub fn attach(
    instances: &mut Instances,
    cache: &mut CompileCache,
    entity_id: u32,
    source_path: &str,
    label: String,
    source: &str,
    language: &str,
    warn: &mut dyn FnMut(String),
) -> Option<ScriptInstance> {
    match compile_cached(cache, source_path, &label, source, language) {
        Ok(fns) => {
            let inst = ScriptInstance { fns, label };
            instances.insert(entity_id, inst.clone());
            Some(inst)
        }
        Err(e) => {
            warn(format!("compile '{label}' failed: {e}"));
            None
        }
    }
}

/// Each newly attached script's `init(entity)`, in the order given.
pub fn run_inits(started: &[(u32, ScriptInstance)], warn: &mut dyn FnMut(String)) {
    for (eid, inst) in started {
        if let Err(e) = call_script_fn(&inst.fns.init, *eid, 0.0) {
            warn(format!("init '{}' failed: {e}", inst.label));
        }
    }
}

/// What a scene's scripts leave behind outside the world, forgotten when the
/// world is replaced: the spawn handles, the tilemaps published to them, and
/// the play state the scene owns in JS - its timers and event listeners carry
/// raw entity ids the next scene reuses. Nothing cleared those for the life
/// of the engine until scenes churned (a game loading a new scene every few seconds: one copy
/// hearing twenty-six pings after twenty-five reloads).
pub fn forget_scene() {
    SCRIPT_ENTITY_MAP.with(|m| m.borrow_mut().clear());
    super::publish::forget_published_tilemaps();
    if let Some(play) = super::mirror::play_namespace() {
        crate::scripting::timers::clear_scene_scoped_play_state(&play);
    }
}
