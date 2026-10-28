//! Browser-script host for the deployed web runtime.
//!
//! Feature-backlog 3.7 (Path 1): brings user Lua/JS scripts to the deployed
//! `jengine-web` WASM runtime so behavior matches the native runtime and the
//! editor viewport. This is a focused port of the viewport's
//! `__jengine_play` bridge (compile → seed namespace → `update` → drain
//! commands → write back) adapted for [`WebApp`].
//!
//! ## Parity scope vs. the editor viewport
//! - Supported: `init` / `update`, transform + sprite-color read/write,
//!   input + camera + custom-component + tilemap + GUI-state mirroring,
//!   trigger callbacks (`on_trigger_*` / `on_entered_trigger` /
//!   `on_exited_trigger`), collision callbacks (`on_collision_enter` /
//!   `on_collision_exit`, fed by the rapier step in `tick_world` when the
//!   `physics` feature is on), physics commands (force / impulse / velocity /
//!   angular velocity / gravity / body type), and the core command set (log,
//!   spawn/despawn, tags, velocity, animation, tweens, particles, tilemap
//!   edits, camera shake, light edits).
//! - NOT yet supported (warned once per command kind): audio, GUI/UI overlay
//!   creation, joints, materials, and post-process. Physics force / impulse /
//!   gravity now apply when the project enables a physics backend; with
//!   physics disabled the commands are silently consumed.

use jengine_core::scripting::bridge::adapters::js::mirror::{self, play_namespace};
use std::cell::RefCell;
use std::collections::HashSet;

use hecs::{Entity, World};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use jengine_core::ecs::components::{
    ScriptComponent, ScriptLanguage,
};

use super::WebApp;

// ---------------------------------------------------------------------------
// Local logging (the parent crate's console_log! macro isn't in scope here).
// ---------------------------------------------------------------------------

fn log_line(msg: &str) {
    if super::logs_on() {
        web_sys::console::log_1(&JsValue::from_str(msg));
    }
}
fn warn_line(msg: &str) {
    web_sys::console::warn_1(&JsValue::from_str(msg));
}

// ---------------------------------------------------------------------------
// Script instance + entity-id mapping
// ---------------------------------------------------------------------------


// A compiled script attached to one entity, the spawn map and the calls into
// a script are shared with the editor viewport (`adapters::js::scripts`).
pub use jengine_core::scripting::bridge::adapters::js::scripts::ScriptInstance;
pub(crate) use jengine_core::scripting::bridge::adapters::js::scripts::script_names;
use jengine_core::scripting::bridge::adapters::js::scripts::{SCRIPT_ENTITY_MAP, find_entity_by_id, note_spawn};
#[cfg(feature = "physics")]
use jengine_core::scripting::bridge::adapters::js::scripts::call_script_fn;

thread_local! {
    /// Command kinds we've already warned about (once per session) so an
    /// unsupported command issued every frame doesn't spam the console.
    static WARNED_CMDS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

// ---------------------------------------------------------------------------
// `__jengine_play` namespace helpers
// ---------------------------------------------------------------------------


/// The per-frame publish, shared with the other browser host
/// (`adapters::js::mirror`): this host lends what it reads.
fn sync_world_to_play_namespace(app: &WebApp) {
    SCRIPT_ENTITY_MAP.with(|m| {
        mirror::sync_world_to_play_namespace(&mirror::PlayWorld {
            world: &app.world,
            assets: &app.assets,
            camera: &app.camera,
            canvas_size: app.canvas_size,
            frame_stats: mirror::PlayStats {
                fps: app.frame_stats.fps,
                frame_time_ms: app.frame_stats.frame_time_ms,
                entity_count: app.frame_stats.entity_count as u32,
                draw_calls: app.frame_stats.draw_calls as u32,
            },
            input: &app.input,
            mixer: &app.mixer,
            spawns: &m.borrow(),
            particle_count: app.particle_system.alive_count(),
        })
    });
}

/// Scripts' transform writes back into the world (`adapters::js::mirror`).
fn sync_play_namespace_to_world(app: &mut WebApp) {
    mirror::sync_play_namespace_to_world(&mut mirror::PlayWorldMut { world: &mut app.world });
}


/// Data-oriented design — seed `window.__jengine_play.data_tables` from a JSON
/// string of the form `{ "<table id>": { "rows": [...] }, ... }` so the
/// `get_data(table, id, field)` script global can read authored `.data` tables.
/// Called once at startup after the runtime has fetched every table.
pub fn seed_data_tables(tables_json: &str) {
    if let Some(play) = play_namespace() {
        match js_sys::JSON::parse(tables_json) {
            Ok(obj) => {
                let _ = js_sys::Reflect::set(
                    &play,
                    &JsValue::from_str("data_tables"),
                    &obj,
                );
            }
            Err(_) => {
                web_sys::console::warn_1(
                    &"failed to parse data_tables JSON for seeding".into(),
                );
            }
        }
    }
}

/// Publish package script-global names on `window.__jengine_play`.
///
/// The Lua runtime bundle (`lua-runtime.ts`, `packageFunctionNames`) reads
/// these and binds them beside its hardcoded `BRIDGE_FUNCTIONS` allowlist.
/// Without it a package global exists in the JS preamble but is never bound
/// into Lua, which looks exactly like the function not existing. The editor
/// viewport publishes the same field.
pub fn publish_package_api_names(
    commands: &jengine_core::package::script_commands::PackageCommandRegistry,
) {
    let Some(play) = play_namespace() else {
        return;
    };
    let arr = js_sys::Array::new();
    for name in &commands.names() {
        arr.push(&JsValue::from_str(name));
    }
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("__package_api_names"), &arr);
    jengine_core::scripting::namespaces::publish(&play, commands.commands());
}

/// Seed `window.__jengine_play.prefab_scripts` — the map `require()`
/// resolves against.
///
/// Both the Fengari shim (editor) and this runtime look modules up here, so
/// a web build has to fill it the way the editor's Toolbar does before play.
/// Without it, every script containing a `require` compiles and then dies at
/// init with "module not found".
pub fn seed_prefab_scripts(scripts_json: &str) {
    if let Some(play) = play_namespace() {
        match js_sys::JSON::parse(scripts_json) {
            Ok(obj) => {
                let _ = js_sys::Reflect::set(
                    &play,
                    &JsValue::from_str("prefab_scripts"),
                    &obj,
                );
            }
            Err(_) => {
                web_sys::console::warn_1(
                    &"failed to parse prefab_scripts JSON for seeding".into(),
                );
            }
        }
    }
}

/// Seed `window.__jengine_play.prefabs` with the project's prefabs so the
/// browser `spawn_prefab` wrapper has component data to send with its `spf`
/// command. Without this the wrapper posts `data: null` and the runtime
/// spawns a bare Name+Transform entity — the prefab appears to spawn and
/// then does nothing. Mirrors what the editor's Toolbar installs before
/// `viewport_play`.
/// Add scripts to `window.__jengine_play.prefab_scripts` - the modules
/// `require` reads - keeping those already there. An asset pack's scripts
/// arrive this way when it mounts.
#[cfg(feature = "packs")]
pub fn add_prefab_scripts(scripts_json: &str) {
    let Some(play) = play_namespace() else { return };
    let Ok(added) = js_sys::JSON::parse(scripts_json) else {
        web_sys::console::warn_1(&"failed to parse prefab_scripts JSON to add".into());
        return;
    };
    let key = JsValue::from_str("prefab_scripts");
    let existing = js_sys::Reflect::get(&play, &key).unwrap_or(JsValue::UNDEFINED);
    if existing.is_object() {
        let _ = js_sys::Object::assign(&existing.into(), &added.into());
    } else {
        let _ = js_sys::Reflect::set(&play, &key, &added);
    }
}

pub fn seed_prefabs(prefabs_json: &str) {
    if let Some(play) = play_namespace() {
        match js_sys::JSON::parse(prefabs_json) {
            Ok(obj) => {
                let _ = js_sys::Reflect::set(&play, &JsValue::from_str("prefabs"), &obj);
            }
            Err(_) => {
                web_sys::console::warn_1(&"failed to parse prefabs JSON for seeding".into());
            }
        }
    }
}

/// Hit-test a left-click against every registered `gui_on_click` binding
/// and invoke the winning callback.
///
/// The editor viewport gets clicks handed to it by the editor's DOM layer;
/// a browser build has no such layer, so the runtime hit-tests the GUI
/// itself - the same job `ScriptApi::process_gui_clicks` does natively,
/// against the same `ui::layout::compute_layout` rects. Called before the
/// script update phase so a click and its response land on one frame.
pub fn process_gui_clicks(app: &mut WebApp) {
    // Release, not press: a scroll drag must not also press the button it
    // started on, so buttons activate on release and `GuiScrollState` vetoes
    // the release of a gesture that turned out to be a scroll.
    if !app.input.mouse_left_just_released
        || app.gui_scroll.is_dragging()
        || app.gui_click_callbacks.is_empty()
    {
        return;
    }
    let (pw, ph) = app.input.screen_size;
    if pw <= 0.0 || ph <= 0.0 {
        return;
    }
    // Into virtual pixels, the space the layout is computed in.
    let hits = jengine_core::ui::layout::click_hits(
        app.gui_click_callbacks.keys().map(|(g, n)| (g.as_str(), n.as_str())),
        &jengine_core::ui::layout::visible_gui_ids(&app.world),
        &app.assets,
        app.ui_scale.to_ui(app.input.mouse_position.0, app.input.mouse_position.1, pw, ph),
        app.ui_scale.virtual_size(pw, ph),
    );
    for (gui_id, node_id) in hits {
        dispatch_gui_click(app, &gui_id, &node_id);
    }
}

/// Invoke the callback registered for `(gui_id, node_id)`.
///
/// Mirrors the viewport: a named callback is looked up on the script's
/// export table, and the default `on_gui_click` uses the dedicated slot.
/// Arguments are `(entity, node_id)`, matching the native runner - the gui
/// id is implicit from where the binding was registered.
/// Call a script's named lifecycle handler, if it defines one.
///
/// Mirrors the editor viewport: resolve by name through the script's `lookup`
/// and skip silently when absent, rather than consulting an `events = {...}`
/// subscription index (which only the native `BridgeRunner` maintains).
/// Call what the engine owes entities' own scripts (`ecs::callbacks`):
/// `on_screen_entered`, `on_path_end` ...
pub fn dispatch_entity_callbacks(app: &mut WebApp) {
    for owed in jengine_core::ecs::callbacks::take() {
        call_lifecycle_handler(app, owed.entity.id(), owed.handler);
    }
}

fn call_lifecycle_handler(app: &mut WebApp, eid: u32, handler: &str) {
    jengine_core::scripting::bridge::adapters::js::scripts::call_handler(&app.script_fns, eid, handler, &mut |m| warn_line(&format!("[web] {m}")));
}

/// Fire `on_despawn` for every scripted entity, newest first, then forget
/// them. Used when the whole world goes away at once (a scene transition) —
/// the only chance a script gets to release what the world does not own.
pub fn dispatch_despawn_for_all(app: &mut WebApp) {
    let mut ids: Vec<u32> = app.script_fns.keys().copied().collect();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    for eid in ids {
        call_lifecycle_handler(app, eid, "on_despawn");
    }
    drain_play_commands_into(app);
    app.spawn_notified.clear();
}

pub(crate) fn dispatch_gui_click(app: &mut WebApp, gui_id: &str, node_id: &str) {
    jengine_core::scripting::bridge::adapters::js::scripts::call_gui_click(&app.script_fns, &app.gui_click_callbacks, gui_id, node_id, &mut |m| {
        warn_line(&format!("[web] {m}"))
    });
}

/// A scene change requested by a script, as posted by the browser
/// `load_scene` / `push_scene` / `pop_scene` shims.
pub struct PendingScene {
    /// `"load"`, `"push"`, or `"pop"`.
    pub action: String,
    /// Target scene path, relative to `assets/`. Absent for `"pop"`.
    pub path: Option<String>,
}

/// Park a `load_scene(path)` request on `window.__jengine_play.pending_scene`,
/// the slot [`take_pending_scene`] polls - `{ action, path }`, as the browser
/// shim writes it. A behavior graph's `ChangeScene` goes through here.
pub fn post_scene_load(path: &str) {
    let Some(play) = play_namespace() else { return };
    let req = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&req, &JsValue::from_str("action"), &JsValue::from_str("load"));
    let _ = js_sys::Reflect::set(&req, &JsValue::from_str("path"), &JsValue::from_str(path));
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("pending_scene"), &req);
}

/// Read and clear `window.__jengine_play.pending_scene`.
///
/// The browser bridge parks scene requests on that slot rather than in the
/// command queue, so the host has to poll it. Clearing on read is what
/// keeps one `load_scene` call from being replayed every frame.
pub fn take_pending_scene() -> Option<PendingScene> {
    let play = play_namespace()?;
    let key = JsValue::from_str("pending_scene");
    let val = js_sys::Reflect::get(&play, &key).ok()?;
    if val.is_undefined() || val.is_null() {
        return None;
    }
    let _ = js_sys::Reflect::set(&play, &key, &JsValue::NULL);
    let obj: js_sys::Object = val.dyn_into().ok()?;
    let action = js_value_string(&obj, "action").unwrap_or_default();
    if action.is_empty() {
        return None;
    }
    Some(PendingScene {
        path: js_value_string(&obj, "path"),
        action,
    })
}


fn js_value_string(obj: &js_sys::Object, key: &str) -> Option<String> {
    js_sys::Reflect::get(obj, &JsValue::from_str(key))
        .ok()
        .and_then(|v| v.as_string())
}

/// Find a live entity by raw id. Consults `SCRIPT_ENTITY_MAP` first so
/// script-side `idx` ids resolve to the real `Entity`, then falls back to
/// a linear scan.
/// Lend the world and the script-id table to the shared command handler.
///
/// `resolve_entity` is why that trait exists rather than the shared code
/// scanning for itself: `SCRIPT_ENTITY_MAP` below is per-host state, so a
/// handler that scanned the world would disagree with this host on any id the
/// table has remapped.
impl jengine_core::scripting::host_commands::ScriptWorldHost for WebApp {
    fn world(&mut self) -> &mut World {
        &mut self.world
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        find_entity_by_id(&self.world, id)
    }
}

fn set_play_namespace_dt(dt: f32) {
    if let Some(play) = play_namespace() {
        let _ = js_sys::Reflect::set(
            &play,
            &JsValue::from_str("dt"),
            &JsValue::from_f64(dt as f64),
        );
    }
}



// ---------------------------------------------------------------------------
// Compilation + invocation
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Script discovery + install
// ---------------------------------------------------------------------------

/// A `ScriptComponent` the world wants loaded. The runtime fetches each
/// `source_path` (async) before calling [`install_scripts`].
pub struct ScriptRequest {
    pub entity_id: u32,
    pub source_path: String,
    pub language: String,
}

/// A fetched + ready-to-compile script.
///
/// Cloneable so a scene snapshot can keep the sources it was running and
/// reinstall them when the stack pops back.
#[derive(Clone)]
pub struct LoadedScript {
    pub entity_id: u32,
    pub source_path: String,
    pub language: String,
    pub source: String,
}

/// Gather every `ScriptComponent` in the world so the runtime can fetch
/// their source files before compiling.
pub fn collect_script_requests(world: &World) -> Vec<ScriptRequest> {
    let mut out = Vec::new();
    for (entity, sc) in world.query::<&ScriptComponent>().iter() {
        let language = match sc.language {
            ScriptLanguage::Lua => "Lua",
            ScriptLanguage::JavaScript => "JavaScript",
        }
        .to_string();
        out.push(ScriptRequest {
            entity_id: entity.id(),
            source_path: sc.source_path.clone(),
            language,
        });
    }
    out
}

/// Compile + attach the fetched scripts and run their `init`. Mirrors the
/// viewport's `load_and_run_scripts`, but keyed off entity ids the caller
/// already resolved (web has no editor handing it a `scripts_json`).
pub fn install_scripts(app: &mut WebApp, loaded: Vec<LoadedScript>) {
    app.script_fns.clear();
    app.script_cache.clear();
    app.tween_manager = jengine_core::animation::tween::TweenManager::new();
    // A new world stops the old one's films.
    #[cfg(feature = "video")]
    app.video.clear();
    app.play_namespace_ready = false;
    // Spawn handles, published tilemaps, the scene's timers and listeners.
    jengine_core::scripting::bridge::adapters::js::scripts::forget_scene();
    if loaded.is_empty() {
        return;
    }
    let mut started = Vec::new();
    let mut warn = |m: String| warn_line(&format!("[web] {m}"));
    for ls in &loaded {
        let label = jengine_core::scripting::bridge::adapters::js::scripts::label_for(&ls.source_path, ls.entity_id);
        if let Some(inst) = jengine_core::scripting::bridge::adapters::js::scripts::attach(
            &mut app.script_fns,
            &mut app.script_cache,
            ls.entity_id,
            &ls.source_path,
            label,
            &ls.source,
            &ls.language,
            &mut warn,
        ) {
            started.push((ls.entity_id, inst));
        }
    }
    log_line(&format!("[web] scripts: loaded={}, errors={}", started.len(), loaded.len() - started.len()));
    if started.is_empty() {
        return;
    }
    prepare_play_namespace(app);
    jengine_core::scripting::bridge::adapters::js::scripts::run_inits(&started, &mut |m| warn_line(&format!("[web] {m}")));
    drain_play_commands_into(app);
    sync_play_namespace_to_world(app);
}

/// Ready the play namespace for the scripts about to run: the world
/// published and the synchronous hooks installed.
fn prepare_play_namespace(app: &mut WebApp) {
    // Seed the namespace so init() can read positions, etc.
    sync_world_to_play_namespace(app);
    // Spawn handles and UI ids are not seeded here: the preamble starts them
    // at a million (`FIRST_SPAWN_ID`), out of the range real entity ids live
    // in. They were seeded at the largest id plus 1,024 and 2,048, so a scene
    // spawning a thousand things handed out a UI id equal to a spawn handle.
    // 2.6 — install the synchronous pathfinding callbacks and seed their
    // snapshot, so a `tilemap_find_path` call from `init()` already has data.
    app.nav_snapshot.borrow_mut().refresh_named(&app.world, &script_names());
    if let Err(e) = jengine_core::scripting::bridge::adapters::js::nav::install_nav_callbacks(
        std::rc::Rc::clone(&app.nav_snapshot),
    ) {
        console_warn!("install_nav_callbacks failed: {:?}", e);
    }
    // And `raycast`, which the preamble has always called and no host has
    // ever installed - so it returned null in every web build from the day
    // the function was specified.
    #[cfg(feature = "physics")]
    {
        app.ray_snapshot.borrow_mut().refresh_named(&app.world, &script_names());
        if let Err(e) = jengine_core::scripting::bridge::adapters::js::ray::install_ray_callback(
            std::rc::Rc::clone(&app.ray_snapshot),
        ) {
            console_warn!("install_ray_callback failed: {:?}", e);
        }
    }
    app.play_namespace_ready = true;
}

/// Compile and start the scripts of entities added to a running world - an
/// additive chunk (`scene::streaming`): `install_scripts` without clearing
/// anything first.
pub fn attach_scripts(app: &mut WebApp, loaded: Vec<LoadedScript>) {
    let mut started = Vec::new();
    let mut warn = |m: String| warn_line(&format!("[web] {m}"));
    for ls in &loaded {
        let label = jengine_core::scripting::bridge::adapters::js::scripts::label_for(&ls.source_path, ls.entity_id);
        if let Some(inst) = jengine_core::scripting::bridge::adapters::js::scripts::attach(
            &mut app.script_fns,
            &mut app.script_cache,
            ls.entity_id,
            &ls.source_path,
            label,
            &ls.source,
            &ls.language,
            &mut warn,
        ) {
            started.push((ls.entity_id, inst));
        }
    }
    if started.is_empty() {
        return;
    }
    if app.play_namespace_ready {
        sync_world_to_play_namespace(app);
    } else {
        prepare_play_namespace(app);
    }
    jengine_core::scripting::bridge::adapters::js::scripts::run_inits(&started, &mut |m| warn_line(&format!("[web] {m}")));
    drain_play_commands_into(app);
    sync_play_namespace_to_world(app);
}

// ---------------------------------------------------------------------------
// Per-frame orchestration
// ---------------------------------------------------------------------------

/// Run the browser-script update phase for this frame: refresh the
/// namespace, invoke each entity's `update`, dispatch trigger callbacks,
/// drain queued commands, write mutations back to the ECS, and advance
/// script-driven tweens. Call this once per frame, right after `tick_world`.
/// Hand an animation frame's events to the scripts on the entities whose
/// clips reached those frames: `on_animation_event(entity, { name, data })`.
///
/// New in Sep 2026, and new in all four runtimes at once: the frames fired,
/// every runtime threw the result away, and the handler name was one nothing
/// ever called.
pub fn dispatch_animation_events(
    app: &mut crate::WebApp,
    events: &[(u64, String, Option<serde_json::Value>)],
) {
    for (entity_id, name, data) in events {
        jengine_core::scripting::bridge::adapters::js::scripts::call_animation_event(&app.script_fns, *entity_id as u32, name, data.as_ref(), &mut |m| warn_line(&format!("[web] {m}")));
    }
    // A frame's `EmitEvent` is an event scripts can listen for, so it also
    // reaches the ordinary listeners - a game whose animations are on its
    // sprites and whose logic is on one controller entity would otherwise
    // hear nothing. Native and the viewport do the same.
    for (entity_id, name, data) in events {
        let data_js = match data {
            Some(d) => JsValue::from_str(&d.to_string()),
            None => JsValue::NULL,
        };
        dispatch_event_to_listeners(app, name, *entity_id as u32, &data_js, None);
        // On to graphs as well, as native's script bus and the viewport do.
        app.behavior_system.deliver_script_event(name, data.as_ref(), &mut app.world);
    }
}

/// Graphs -> scripts: each of this frame's graph `EmitEvent`s reaches the
/// `on_<name>` listeners with `{ value, x, y }`, the payload every runtime
/// builds from `EmitEventOutcome::script_payload`. Called by the frame
/// right after the behaviour barrier.
pub fn dispatch_graph_events(
    app: &mut crate::WebApp,
    events: &[jengine_core::behavior::EmitEventOutcome],
) {
    for ev in events {
        let data_js = JsValue::from_str(&ev.script_payload().to_string());
        dispatch_event_to_listeners(app, &ev.name, 0, &data_js, None);
    }
}

/// Entity-UI clicks -> scripts: each clicked button reaches the `ui_click`
/// listeners with `{ button = id }`, as native and the viewport deliver it.
pub fn dispatch_ui_clicks(app: &mut crate::WebApp, clicked: &[u64]) {
    for &button in clicked {
        let data = jengine_core::ecs::systems::ui_click_payload(button);
        let data_js = JsValue::from_str(&data.to_string());
        dispatch_event_to_listeners(app, jengine_core::ecs::systems::UI_CLICK_EVENT, 0, &data_js, None);
    }
}

/// Call the listener's handler for `name`, on every entity listening.
///
/// **Two spellings, because both are in use and each runtime only knew one.**
/// `on_<event_name>` is the documented convention and what native calls; this
/// host called a function literally named `on_event`. A script written from
/// the docs therefore did nothing here, and one written against this host did
/// nothing on native. Specific name first, general one as a fallback, so both
/// keep working. BUGS #150.
///
/// **One implementation, because there used to be two.** The `evt` command arm
/// had its own inline copy of this loop, so fixing the handler-name lookup
/// here changed nothing for a script's `send_event` - only for an animation
/// frame's `EmitEvent`, the one caller this function had. That is
/// `docs/architecture-debt.md`'s hand-copied host code biting exactly as
/// described. `only` is `send_event_to`'s target, which is the one thing the
/// copy did that this did not.
fn dispatch_event_to_listeners(app: &mut WebApp, name: &str, sender: u32, data_js: &JsValue, only: Option<u32>) {
    jengine_core::scripting::bridge::adapters::js::scripts::dispatch_event(&app.script_fns, name, sender, data_js, only, &mut |m| warn_line(&format!("[web] {m}")));
}

/// Every running script's `fixed_update`, if it defines one.
#[cfg(feature = "physics")]
pub fn fixed_update_fns(app: &WebApp) -> Vec<(u32, JsValue)> {
    if !app.play_namespace_ready {
        return Vec::new();
    }
    jengine_core::scripting::bridge::adapters::js::scripts::fixed_update_fns(&app.script_fns)
}

/// One fixed step's scripts: the world as the last step left it published,
/// each `fixed_update(entity, dt)` called, their commands applied - as native
/// and the editor viewport do.
#[cfg(feature = "physics")]
pub fn run_fixed_update(app: &mut WebApp, fns: &[(u32, JsValue)], fixed_dt: f32) {
    mirror::publish_physics_for_scripts(&app.world, app.physics.as_ref(), &[]);
    sync_world_to_play_namespace(app);
    set_play_namespace_dt(fixed_dt);
    for (eid, f) in fns {
        if let Err(e) = call_script_fn(f, *eid, fixed_dt) {
            warn_line(&format!("[web] fixed_update on entity {eid} failed: {e}"));
        }
    }
    drain_play_commands_into(app);
}

pub fn run_scripts_frame(app: &mut WebApp, dt: f32, pending_contacts: &[(u32, u32, bool, f32, f32, f32)]) {
    if !app.play_namespace_ready || app.script_fns.is_empty() {
        return;
    }
    sync_world_to_play_namespace(app);
    set_play_namespace_dt(dt);
    // `on_spawn`, `update`, timers, triggers and contacts - shared with the
    // editor viewport.
    jengine_core::scripting::bridge::adapters::js::scripts::run_frame(
        &app.script_fns,
        &mut app.spawn_notified,
        &app.world,
        dt,
        pending_contacts,
        &mut |m| warn_line(&format!("[web] {m}")),
    );

    // The drain writes back the script's transforms first; this catches what
    // callbacks fired during the drain wrote.
    drain_play_commands_into(app);
    sync_play_namespace_to_world(app);

    // Advance script-driven tweens (set_position / tween_to etc.).
    jengine_core::ecs::systems::tick_tweens(&mut app.world, &mut app.tween_manager, dt);
}

// ---------------------------------------------------------------------------
// World -> namespace sync
// ---------------------------------------------------------------------------

/// Push the current world transforms / sprite colors / names / input /
/// camera / custom-components / gui-state / tilemaps into the JS namespace.







// ---------------------------------------------------------------------------
// Namespace -> world writeback
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// Command queue drainer (Path 1 subset)
// ---------------------------------------------------------------------------

/// Drain `__jengine_play.commands`. A command goes to the shared world arms
/// (`host_commands`), then the shared host arms (`host_dispatch`), and only
/// what neither knows reaches this host's own `match` - the ones that call
/// into its script instances or scene loading.
fn drain_play_commands_into(app: &mut WebApp) {
    let Some(play) = play_namespace() else {
        return;
    };
    // What the scripts wrote through `set_position` and kin reaches the world
    // before their commands do, as it does natively: `juice:burst` moves an
    // emitter and bursts it in one call, and drained first the burst came
    // from where the emitter last was. It also keeps a `fixed_update`'s
    // writes, which the next publish's reset of the dirty lists discarded.
    sync_play_namespace_to_world(app);
    let cmds = match js_sys::Reflect::get(&play, &JsValue::from_str("commands")) {
        Ok(v) if v.is_instance_of::<js_sys::Array>() => v.unchecked_into::<js_sys::Array>(),
        _ => return,
    };
    let mut unhandled_kinds: HashSet<String> = HashSet::new();
    let snapshot_len = cmds.length();
    for i in 0..snapshot_len {
        let cmd = cmds.get(i);
        if !cmd.is_object() {
            continue;
        }
        let cmd_obj: js_sys::Object = cmd.unchecked_into();
        let kind = js_sys::Reflect::get(&cmd_obj, &JsValue::from_str("t"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        // Commands that need nothing but the world are implemented once, in
        // `jengine_core::scripting::host_commands`, and dispatched here before
        // this host's own arms. They used to be copied verbatim between this
        // file and `jengine-editor-viewport/src/lib.rs`, where nothing could
        // test them — they only ran in a browser. See
        // `docs/architecture-debt.md` #2.
        if jengine_core::scripting::host_commands::handle_world_command(
            app,
            kind.as_str(),
            &jengine_core::scripting::host_commands::JsArgs(&cmd_obj),
        ) {
            continue;
        }
        // Then package-contributed commands, dispatched through the same
        // function native and the editor viewport use - one implementation
        // of every package command, reached from every runtime.
        if jengine_core::package::script_commands::dispatch_global(
            app,
            kind.as_str(),
            &jengine_core::scripting::host_commands::JsArgs(&cmd_obj),
        ) {
            continue;
        }
        // Then the arms both browser hosts share that need more than the
        // world - audio, tweens, saves, input, fades (`adapters::js::host_dispatch`).
        if jengine_core::scripting::bridge::adapters::js::host_dispatch::handle_host_command(
            kind.as_str(),
            &cmd_obj,
            &mut jengine_core::scripting::bridge::adapters::js::host_dispatch::HostParts {
                world: &mut app.world,
                mixer: &mut app.mixer,
                web_audio: &mut app.web_audio,
                tween_manager: &mut app.tween_manager,
                save_system: &mut app.save_system,
                input: &mut app.input,
                particle_system: &mut app.particle_system,
                behavior_system: &mut app.behavior_system,
                fade_active: &mut app.fade_active,
                time_scale: &mut app.time_scale,
                find_entity_by_id,
                note_spawn,
                unhandled_kinds: &mut unhandled_kinds,
                physics: app.physics.as_mut(),
                warn: &mut |m: String| log::warn!("{m}"),
                post_process: Some(&mut app.post_process),
                debug_draws: &mut app.debug_draws,
                assets: &mut app.assets,
                gui_click_callbacks: &mut app.gui_click_callbacks,
                registry: &app.registry,
                pending_camera_shakes: &mut app.pending_camera_shakes,
                audio_warned: &mut app.audio_warned,
                script_fns: &mut app.script_fns,
                script_cache: &mut app.script_cache,
                spawn_notified: &mut app.spawn_notified,
            },
        ) {
            continue;
        }
        match kind.as_str() {
            "log" => {
                if let Some(m) = js_value_string(&cmd_obj, "m") {
                    log_line(&m);
                }
            }
            // quit_game(). A browser tab has nothing to close and a page may
            // not close itself, so this is a logged no-op by design — the menu
            // that calls it should hide its Quit button on web.
            "quit" => {
                log_line("quit_game() ignored: the web runtime has no window to close");
            }

            other => {
                unhandled_kinds.insert(other.to_string());
            }
        }
    }
    // After every command this drain applied: forget finished sounds and push
    // any bus change (`ab` above) to the sounds still playing.
    app.web_audio.sync_mixer(&mut app.mixer);
    // The music - crossfades, layers, a playlist - and its playhead.
    jengine_core::audio::music::drive(&mut jengine_core::audio::web_audio::WebOut {
        audio: &mut app.web_audio,
        mixer: &mut app.mixer,
    });
    // Trim only the commands we consumed; callbacks firing during this
    // drain push NEW commands that we keep for the next frame.
    if snapshot_len > 0 {
        if let Ok(splice_fn) = js_sys::Reflect::get(&cmds, &JsValue::from_str("splice")) {
            if let Some(f) = splice_fn.dyn_ref::<js_sys::Function>() {
                let args = js_sys::Array::new();
                args.push(&JsValue::from_f64(0.0));
                args.push(&JsValue::from_f64(snapshot_len as f64));
                let _ = f.apply(&cmds, &args);
            }
        }
    }
    // Warn once per unsupported command kind so per-frame issuers don't spam.
    for kind in unhandled_kinds {
        let first = WARNED_CMDS.with(|s| s.borrow_mut().insert(kind.clone()));
        if first {
            warn_line(&format!(
                "[web] script command '{kind}' is not supported in the web runtime yet"
            ));
        }
    }
}
