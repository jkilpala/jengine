//! jEngine web runtime — wgpu/WebGPU scene renderer.
//!
//! Phase 8.2d rebuild: this is a wasm-twin of
//! `crates/jengine-runtime/src/wgpu_scene_runner.rs`. It loads a `.scene`
//! file via fetch, builds a `hecs::World` via `load_scene_auto`, then runs
//! the same `*_wgpu` render systems jengine-core ships for the native
//! runtime.
//!
//! ## Packages
//! The runtime hosts a `PackageHost` like native and the editor viewport:
//! the built-in packages, plus a project's own when a generated crate
//! (`jengine_builder::packages::generate_project_web`) registers them through
//! [`register_project_package`] and starts the runtime with [`run`]. A stock
//! build starts from the `standalone` feature's `start` instead.
//!
//! Scripts (Lua/JS) ARE wired as of feature-backlog 3.7: `init` / `update`,
//! transform + sprite-color sync, input, animation, tweens, particles,
//! tilemap edits, camera shake, light edits, trigger callbacks, and — when
//! the project enables a physics backend — rapier physics stepping with
//! `on_collision_enter` / `on_collision_exit` callbacks and physics commands
//! (force / impulse / velocity / gravity / body type). See [`script_host`]
//! for the supported command set.
//!
//! Requires a WebGPU-capable browser (Chrome/Edge 113+, Safari 18+,
//! Firefox Nightly).

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use glam::UVec2;
use hecs::World;
use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{HtmlCanvasElement, Request, RequestInit, RequestMode, Response};

use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::ecs::components::GuiComponent;
use jengine_core::input::{browser_keeps_default, BrowserKeyMods, InputAction, InputState, KeyCode};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::particles::ParticleSystem;
use jengine_core::renderer::backend::wgpu::{
    WgpuLightingPass, WgpuPostProcessStack, WgpuRenderer, WgpuSpriteBatch,
};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::lighting::has_lights;
use jengine_core::renderer::text::{built_in_font_atlas, FontData};
use jengine_core::scene::format::load_scene_auto;
use jengine_core::renderer::atlas::AtlasData;
use jengine_core::ui::gui_document::{load_gui, GuiDocument};

/// Texture id under which the built-in font atlas RGBA bytes are
/// registered in [`AssetManager`]. Mirrors the constant in the native
/// `wgpu_scene_runner`.
const BUILTIN_FONT_ID: &str = "__font/builtin";

// ---------------------------------------------------------------------------
// Console logging macros
// ---------------------------------------------------------------------------

macro_rules! console_log {
    ($($arg:tt)*) => { web_sys::console::log_1(&format!($($arg)*).into()) };
}
macro_rules! console_warn {
    ($($arg:tt)*) => { web_sys::console::warn_1(&format!($($arg)*).into()) };
}
macro_rules! console_error {
    ($($arg:tt)*) => { web_sys::console::error_1(&format!($($arg)*).into()) };
}

// Browser-script host (feature-backlog 3.7 — scripting parity).
mod script_host;

// A web portal's pause and mute (docs/web-platforms.md).
mod platform;

// ---------------------------------------------------------------------------
// Browser helpers
// ---------------------------------------------------------------------------

fn window() -> web_sys::Window {
    web_sys::window().expect("no global `window`")
}

fn performance() -> web_sys::Performance {
    window().performance().expect("no `performance`")
}

fn request_animation_frame(f: &Closure<dyn FnMut(f64)>) {
    window()
        .request_animation_frame(f.as_ref().unchecked_ref())
        .expect("requestAnimationFrame failed");
}

/// Follow a web portal's pause and mute (`platform::read`); true while the
/// game is held - from the frame after the pause, so the frame it pauses on
/// still runs and the `platform_pause` event the adapter queued reaches the
/// scripts before everything stops. Held means no update, no drawing, no
/// input and a suspended audio context: YouTube Playables asks for exactly
/// that, and Poki and CrazyGames for it during an ad.
fn apply_host_state(app_cell: &Rc<RefCell<WebApp>>, timestamp: f64) -> bool {
    let host = platform::read();
    let mut a = app_cell.borrow_mut();
    let was = a.host;
    if host.muted != was.muted {
        a.web_audio.set_host_muted(host.muted);
    }
    if host.paused != was.paused {
        a.web_audio.set_host_paused(host.paused);
        if host.paused {
            a.input.release_all();
        } else {
            // Resume from here, not from where the pause began.
            a.last_time = timestamp;
            a.last_drawn = f64::NEG_INFINITY;
        }
    }
    a.host = host;
    host.paused && was.paused
}

/// Tell the page the first frame is drawn: a `jengine-ready` event on `window`,
/// and `window.__jengine_ready = true` for a listener that comes later.
///
/// Nothing in the engine listens. It is for the host page: a portal's loading
/// screen waits for it (`tools/jworld/publish.mjs` calls the jCabinet SDK's
/// `gameLoaded()` on it). Until then the page only knows the wasm started,
/// and the project, scene, scripts and textures still had to arrive.
fn announce_ready() {
    let w = window();
    let _ = js_sys::Reflect::set(&w, &JsValue::from_str("__jengine_ready"), &JsValue::TRUE);
    if let Ok(event) = web_sys::Event::new("jengine-ready") {
        let _ = w.dispatch_event(&event);
    }
}

fn get_canvas() -> Result<HtmlCanvasElement, JsValue> {
    let document = window()
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let el = document
        .get_element_by_id("jengine-canvas")
        .ok_or_else(|| JsValue::from_str("no #jengine-canvas element"))?;
    el.dyn_into::<HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str("#jengine-canvas is not a <canvas>"))
}

/// Resolve an asset id to a fetch URL under the web root.
///
/// Asset ids — `sprite.texture_id`, `gui_id`, `script.source_path`,
/// `settings.entry_scene`, … — are stored **relative to the project's
/// `assets/` directory** (`sprites/hero.png`, `scenes/boot.scene`). That is
/// what the native runtime joins onto its `assets` root and what the editor
/// resolves against `<project>/assets`. The web build copies that tree to
/// `assets/` under the web root, so the fetch URL is the id plus that
/// prefix — without it every asset 404s.
///
/// Ids that already carry the prefix pass through unchanged, which keeps
/// the builder's already-rooted `data_tables` list and any older project
/// that stored prefixed ids working. Absolute URLs pass through too.
fn asset_url(id: &str) -> String {
    if id.starts_with("assets/")
        || id.starts_with('/')
        || id.starts_with("http://")
        || id.starts_with("https://")
    {
        id.to_string()
    } else {
        format!("assets/{id}")
    }
}

async fn fetch_text(url: &str) -> Result<String, JsValue> {
    // A downloaded asset pack serves its files before the network does.
    if let Some(bytes) = jengine_core::asset::packs::web::read(url) {
        return String::from_utf8(bytes).map_err(|_| JsValue::from_str(&format!("{url}: not UTF-8")));
    }
    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);
    let req = Request::new_with_str_and_init(url, &opts)?;
    let resp_value = JsFuture::from(window().fetch_with_request(&req)).await?;
    let resp: Response = resp_value.dyn_into()?;
    if !resp.ok() {
        return Err(JsValue::from_str(&format!(
            "fetch {url}: HTTP {}",
            resp.status()
        )));
    }
    let text_value = JsFuture::from(resp.text()?).await?;
    text_value
        .as_string()
        .ok_or_else(|| JsValue::from_str("response not a string"))
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
    if let Some(bytes) = jengine_core::asset::packs::web::read(url) {
        return Ok(bytes);
    }
    let opts = RequestInit::new();
    opts.set_method("GET");
    opts.set_mode(RequestMode::Cors);
    let req = Request::new_with_str_and_init(url, &opts)?;
    let resp_value = JsFuture::from(window().fetch_with_request(&req)).await?;
    let resp: Response = resp_value.dyn_into()?;
    if !resp.ok() {
        return Err(JsValue::from_str(&format!(
            "fetch {url}: HTTP {}",
            resp.status()
        )));
    }
    let buf_value = JsFuture::from(resp.array_buffer()?).await?;
    let array = js_sys::Uint8Array::new(&buf_value);
    Ok(array.to_vec())
}

// ---------------------------------------------------------------------------
// Project file structures
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
struct WebProjectFile {
    #[serde(default)]
    settings: WebProjectSettings,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct WebProjectSettings {
    name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    /// `"ConstantPixelSize"` turns UI scaling off; anything else (including
    /// absent) scales the UI to fit against `width` x `height`.
    ui_scale_mode: Option<String>,
    /// Per-platform frame-rate caps; this runtime reads `web`.
    frame_rate_cap: jengine_core::time::FrameRateCaps,
    /// How textures are sampled (`jengine_core::renderer::texture_filter`).
    texture_filter: jengine_core::renderer::texture_filter::TextureFilterRules,
    /// Which GPU to ask the browser for (`jengine_core::renderer::gpu_preference`).
    gpu_preference: jengine_core::renderer::gpu_preference::GpuPreference,
    /// The project's mixer: buses and effects before any script touches them
    /// (`jengine_core::audio::MixerLayout`).
    audio_mixer: jengine_core::audio::MixerLayout,
    /// Asset packs downloaded when needed (`jengine_core::asset::packs`).
    asset_packs: jengine_core::asset::packs::PackSettings,
    /// Live ops (`jengine_core::liveops`), read by it as it is written.
    liveops: serde_json::Value,
    /// The app's identity; live ops reads its `version`.
    app: serde_json::Value,
    /// The clear colour. Project files call it `background_color` (the
    /// editor writes that and honours it); this read only `bg_color`, which
    /// no project file contains, so a web build ignored the setting.
    #[serde(alias = "background_color")]
    bg_color: Option<[f32; 4]>,
    entry_scene: Option<String>,
    input_actions: Vec<InputAction>,
    /// Backlog 2.2 — list of `.prefab` asset paths to fetch up-front
    /// so the scene loader can expand any `prefab_instance` entries
    /// before `load_scene_auto` runs. Empty by default; the web build
    /// has no filesystem walker, so the project file (or its builder
    /// pipeline) must list which prefabs are reachable.
    prefabs: Vec<String>,
    /// Data-oriented design — list of `.data` table fetch URLs (relative to
    /// the web root, e.g. `assets/data/enemies.data`) emitted by the web
    /// builder. Fetched up-front and seeded onto
    /// `window.__jengine_play.data_tables` so the `get_data` script global
    /// can read them. The web build has no filesystem walker.
    #[serde(default)]
    data_tables: Vec<String>,
    /// Feature 3.7 — physics simulation settings. When `backend` is not
    /// `"none"` (and the `physics` cargo feature is on) a rapier
    /// `PhysicsWorld` is created so collision callbacks + physics commands
    /// work identically to native + the editor viewport.
    physics: WebPhysicsSettings,
    /// Whether F3 / F12 open the developer overlays. Written by the web
    /// builder from the build option of the same name; absent in older
    /// builds, which is why it defaults to enabled.
    #[serde(default = "default_debug_overlay")]
    debug_overlay: bool,
}

/// Missing key = overlay available, so a project file written before the
/// build option existed behaves as it always did.
fn default_debug_overlay() -> bool {
    true
}

/// Physics block from `project.jengine` `settings.physics`. Defaults to a
/// disabled (`"none"`) backend so projects without physics stay cheap.
#[derive(Debug, Deserialize)]
#[serde(default)]
struct WebPhysicsSettings {
    gravity_x: f32,
    gravity_y: f32,
    length_unit: f32,
    backend: String,
    /// Seconds per fixed physics step; 1/60 unless the project chose otherwise.
    fixed_timestep: f32,
}

impl Default for WebPhysicsSettings {
    fn default() -> Self {
        Self {
            gravity_x: 0.0,
            gravity_y: 0.0,
            length_unit: 100.0,
            backend: "none".to_string(),
            fixed_timestep: 1.0 / 60.0,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct WebApp {
    renderer: WgpuRenderer,
    sprite_batch: WgpuSpriteBatch,
    assets: AssetManager,
    missing_warned: HashSet<String>,
    /// Texture ids already being fetched on demand (`poll_missing_textures`).
    texture_requested: HashSet<String>,
    /// `asset::packs::generation()` when `texture_requested` was last cleared.
    packs_generation: u64,
    /// The data tables as the script namespace holds them (`data_tables`),
    /// kept so a pack's tables can be added to them when it mounts.
    data_tables: serde_json::Map<String, serde_json::Value>,
    /// Graph-material pipelines, per shader graph, blend and format.
    graph_pipelines: jengine_core::renderer::graph_pipeline_cache::GraphPipelineCache,
    /// Shader graphs fetched for the materials in use, by the path their
    /// `.material` names.
    shader_graphs: std::collections::HashMap<String, jengine_core::renderer::shader_graph::ShaderGraph>,
    /// Material ids a draw asked for and nothing has loaded
    /// (`poll_missing_materials` fetches them), and those already asked for.
    material_missing: HashSet<String>,
    material_requested: HashSet<String>,
    world: World,
    camera: Camera2D,
    /// Where the camera was last frame, for `DespawnOffscreen`'s
    /// `behind_only`: "behind" is the opposite of the way it is travelling.
    prev_camera: glam::Vec2,
    particle_system: ParticleSystem,
    post_process: WgpuPostProcessStack,
    light_pass: WgpuLightingPass,
    /// Each view's own world buffers (`ecs::views`), by target id.
    view_targets: std::collections::HashMap<String, jengine_core::ecs::wgpu_systems::ViewTargets>,
    /// Forward pass for `lit_sprite` materials, and the flat normal a material
    /// without `normal_map_id` shades against. The regular sprite pass skips
    /// `lit_sprite` entities expecting this to draw them; until Sep 2026 only
    /// the native runtime had one, so on the web they were never drawn.
    lit_sprite_batch: jengine_core::renderer::backend::wgpu::WgpuLitSpritePipeline,
    flat_normal: jengine_core::renderer::backend::wgpu::WgpuTexture,
    font_data: FontData,
    input: InputState,
    /// `settings.frame_rate_cap.web`, and when the last frame was drawn
    /// (the `requestAnimationFrame` timestamp, ms): a capped build skips the
    /// callbacks that come too soon.
    frame_cap: jengine_core::time::FrameCap,
    last_drawn: f64,
    /// `settings.frame_rate_cap.web`, which `Display.set_max_fps(-1)` goes
    /// back to, and the cap in force.
    project_fps: u32,
    fps: u32,
    /// What a script asked of the page - fullscreen, the cursor, the frame
    /// cap (`jengine_core::display::web`) - applied each frame.
    display: jengine_core::display::web::WebDisplay,
    /// The hidden textarea a focused text field types through
    /// (`jengine_core::input::text_entry::web`).
    text_proxy: jengine_core::input::text_entry::web::TextProxy,
    /// Frames until the safe-area insets are read again - often enough to
    /// follow a phone turning, not so often it costs a style recalculation
    /// every frame.
    safe_area_recheck_in: u32,
    /// The canvas, for measuring where it sits on the page.
    canvas: HtmlCanvasElement,
    /// Fingers: on-screen controls (`TouchControls`) and the one standing in
    /// for the left mouse button. See `input::touch_controls`.
    touch: jengine_core::input::touch_controls::TouchControlsState,
    last_time: f64,
    /// The display's rate, worked out from the frames, and the pacer it
    /// feeds: the `requestAnimationFrame` timestamp is rounded to a
    /// millisecond in WebKit and jitters everywhere (`jengine_core::time`).
    refresh_estimate: jengine_core::time::RefreshEstimate,
    pacer: jengine_core::time::FramePacer,
    /// When the runtime started (`performance().now()`), for the sprite
    /// batch's clock - what a graph's `TimeInput` reads.
    start_time: f64,
    /// Script `set_time_scale`: multiplies the world dt (0.0 = true pause).
    pub(crate) time_scale: f32,
    /// A web portal's pause and mute as last applied (`apply_host_state`).
    host: platform::HostState,
    canvas_size: UVec2,
    /// Feature 3.3 — runtime debug overlay (F3 stats / F12 inspector).
    debug_overlay: jengine_core::debug_overlay::DebugOverlayState,
    /// False when the build turned the developer overlays off; the keys
    /// then do nothing at all.
    debug_overlay_enabled: bool,
    /// Rolling FPS for the stats panel — built inline each frame.
    fps_ema: f32,
    /// Stats behind the F3 overlay. Filled as the frame runs, so the phase
    /// timings and counters are this frame's; `render_time_ms` and
    /// `draw_calls` are necessarily the previous frame's, because the
    /// overlay is drawn from inside the render pass that produces them.
    frame_stats: jengine_core::profiling::FrameStats,
    /// Feature 3.7 — browser-script state (parity with native + viewport).
    /// Compiled scripts keyed by `Entity::id()`.
    script_fns: std::collections::HashMap<u32, script_host::ScriptInstance>,
    /// Entities that have already had `on_spawn` dispatched. `init` runs on
    /// the frame a script is installed; `on_spawn` fires once on the frame
    /// after, matching the documented contract and the native runner.
    spawn_notified: std::collections::HashSet<u32>,
    /// Compiled-script cache keyed by source path (one compile per file).
    script_cache: std::collections::HashMap<
        String,
        jengine_core::scripting::bridge::adapters::js::BrowserScriptFns,
    >,
    /// Script-driven tween manager (set_position / tween_to / etc.).
    tween_manager: jengine_core::animation::tween::TweenManager,
    /// The videos playing (`jengine_core::video`), each a `<video>` element
    /// loading its file from `assets/`.
    video: jengine_core::video::VideoSystem,
    /// True once scripts compiled and `init` ran — gates the per-frame hook.
    play_namespace_ready: bool,
    /// Camera shakes queued by scripts this frame `(amplitude, duration, frequency)`.
    pending_camera_shakes: Vec<(f32, f32, f32)>,
    /// Component registry — used by the `spf` (spawn_prefab) script command.
    registry: ComponentRegistry,
    /// The built-in packages plus the project's own (see
    /// [`register_project_package`]). One host for the session, as natively
    /// and in the editor: a scene change replaces the world, not the
    /// packages.
    package_host: jengine_core::package::host::PackageHost,
    /// Feature 3.7 — rapier physics world. `None` when the project disables
    /// physics (`backend == "none"`) or the `physics` cargo feature is off.
    /// When present it is stepped each frame in `tick_world`, producing the
    /// contact events that drive `on_collision_*` script callbacks.
    #[cfg_attr(not(feature = "physics"), allow(dead_code))]
    physics: Option<jengine_core::physics::PhysicsWorld>,
    /// Declarative behavior graphs. The browser has no filesystem, so
    /// graphs are fetched up-front into `pending_behaviors` and bound with
    /// `attach_inline` — the same route the editor viewport takes.
    behavior_system: jengine_core::behavior::BehaviorGraphSystem,
    /// `.behavior` JSON keyed by `graph_path`, for binding graphs on
    /// entities that appear mid-play (script `spawn_prefab`, graph
    /// `SpawnEntity`).
    pending_behaviors: std::collections::HashMap<String, String>,
    /// Prefab library, kept for scene transitions (re-seeded into the
    /// script namespace when a new scene loads).
    prefab_map: std::collections::HashMap<String, jengine_core::scene::format::EntityData>,
    /// Web Audio backend — the same one the editor viewport uses
    /// (`jengine_core::audio::web_audio`), so a sound that plays in the
    /// editor plays in the browser build.
    pub(crate) web_audio: jengine_core::audio::web_audio::WebAudioBackend,
    /// The sounds `AudioSource`s are playing, by entity (`audio::spatial`).
    pub(crate) spatial_audio: jengine_core::audio::spatial::SpatialAudio,
    /// Clips already reported as missing, so a sound fired every frame
    /// doesn't flood the console.
    pub(crate) audio_warned: HashSet<String>,
    /// 2.3 save system — `localStorage` on this target. Scripts write through
    /// `save_set` / `save_commit`; reads are answered from the JS-side mirror
    /// (see the preamble's `__save_slot`), which reads the same storage.
    pub(crate) save_system: jengine_core::save::SaveSystem,
    /// 2.4 — audio mixer bus tree. Plain data from `jengine_core::audio`;
    /// the browser audio backend has no bus concept, so the effective volume
    /// is applied when a sound plays, exactly as the native audio apply does.
    pub(crate) mixer: jengine_core::audio::AudioMixer,
    /// Active scene fade / screen flash. Web builds ignored `fade_in` /
    /// `fade_out` / `screen_flash` entirely until the overlay moved into
    /// jengine-core.
    /// Script-emitted debug shapes for this frame, drawn after the world and
    /// cleared each frame. The editor viewport forwards these to its Svelte
    /// overlay instead; a web build has no overlay, so it draws them itself.
    pub(crate) debug_draws: Vec<jengine_core::debug_draw::DebugDrawCommand>,
    pub(crate) fade_active: Option<jengine_core::fade_overlay::TransitionState>,
    /// Overlay pipeline, built on first use.
    pub(crate) fade_overlay: Option<jengine_core::fade_overlay::FadeOverlay>,
    /// 2.6 — tilemaps as of the start of this frame, for the synchronous
    /// `tilemap_find_path` host callback. Shared with the installed JS
    /// closures, which is why it is an `Rc<RefCell<…>>` separate from the
    /// app: a callback that reached back into the app would double-borrow
    /// while scripts are running.
    pub(crate) nav_snapshot: std::rc::Rc<
        std::cell::RefCell<jengine_core::scripting::bridge::adapters::js::nav::NavSnapshot>,
    >,
    /// Every collider as of the start of this frame, for the synchronous
    /// `raycast` callback. Separate from the app for the same reason the nav
    /// snapshot is. Unlike the viewport, this crate has its own `physics`
    /// feature, so it is gated.
    #[cfg(feature = "physics")]
    pub(crate) ray_snapshot: std::rc::Rc<
        std::cell::RefCell<jengine_core::physics::ray::RaySnapshot>,
    >,
    /// Physics settings, kept so a scene transition can rebuild the world
    /// (body handles are tied to entities that are about to be dropped).
    /// `None` when the project disables physics.
    #[cfg_attr(not(feature = "physics"), allow(dead_code))]
    physics_config: Option<(f32, f32, f32)>,
    /// Patrol routes declared by the current scene, for `PatrolPathRef`.
    ///
    /// Rebuilt from the scene's `paths` block on load and on every transition.
    /// It used to be `None` unconditionally at the call site, so patrol
    /// entities never moved here (BUGS #7).
    path_library: Option<jengine_core::ecs::path::Path2DLibrary>,
    /// Drag / flick / elastic state for scrolling GUI containers. Touch events
    /// are mapped onto the mouse fields, so this is the touch path too.
    gui_scroll: jengine_core::ui::scroll::GuiScrollState,
    /// Scripts currently installed, so a pushed scene can be restored with
    /// the sources it was running.
    current_scripts: Vec<script_host::LoadedScript>,
    /// `push_scene` stack — snapshots of the scenes below this one.
    scene_stack: Vec<SceneSnapshot>,
    /// A scene change is fetching. The world keeps rendering but stops
    /// ticking, and further requests are ignored until it lands.
    transitioning: bool,
    /// Holds the `require()` resolver closure installed on `globalThis`.
    /// Dropping the adapter invalidates that closure, so it lives as long as
    /// the app does.
    _module_adapter: Option<jengine_core::scripting::bridge::adapters::JsAdapter>,
    /// `gui_on_click` bindings: (gui_id, node_id) -> (entity, callback).
    /// Hit-tested against the GUI layout each frame by
    /// `script_host::process_gui_clicks`.
    /// How the UI is scaled on this project, from `settings.width` /
    /// `height` / `ui_scale_mode` in `project.jengine`.
    pub(crate) ui_scale: jengine_core::ui::scaling::UiScale,
    pub(crate) gui_click_callbacks:
        std::collections::HashMap<(String, String), (u32, String)>,
}

/// A scene parked by `push_scene`, with the scripts it was running.
///
/// Snapshot semantics match the native runner (`SceneSnapshot` in
/// `wgpu_scene_runner`) rather than re-reading the file: a pop returns to
/// the world as it was, not to the scene's authored state.
struct SceneSnapshot {
    scene_json: String,
    scripts: Vec<script_host::LoadedScript>,
    /// The effect chain the pushed-over scene was rendering with. The
    /// snapshot JSON cannot carry it (`SceneDataV2` has no `post_process`
    /// field), and `apply_scene_json` replaces the chain from the scene it
    /// loads, so a pop without this drops the effects (BUGS #84).
    post_process: jengine_core::renderer::postprocess::PostProcessSettings,
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

/// Track heap allocations so the debug overlay can report memory. On wasm
/// the more useful figure is the module's reserved linear memory, which
/// `profiling::memory::resident_bytes` prefers; this adds the live-heap and
/// peak numbers underneath it.
#[global_allocator]
static ALLOC: jengine_core::profiling::memory::TrackingAllocator<std::alloc::System> =
    jengine_core::profiling::memory::TrackingAllocator::new(std::alloc::System);

/// A project package's registration step, installed by a generated
/// per-project web crate (`jengine_builder::packages::generate_project_web`).
///
/// A plain `fn` pointer, as in the editor viewport: a generated crate only
/// ever passes a non-capturing function.
pub type ProjectPackageAdder = fn(&mut jengine_core::package::host::PackageHost);

thread_local! {
    /// Packages a generated crate registered before [`run`] started the
    /// runtime. Read in `init_and_run`, before the component registry is
    /// built, so a package component in the entry scene deserializes as that
    /// component rather than as custom data.
    static PROJECT_PACKAGES: RefCell<Vec<ProjectPackageAdder>> = const { RefCell::new(Vec::new()) };
    /// Every script `require` can reach on the transpiler fallback path, by
    /// asset path: the build's (`fetch_project_scripts`), then each mounted
    /// asset pack's.
    static MODULE_SOURCES: RefCell<std::collections::HashMap<String, String>> = RefCell::new(std::collections::HashMap::new());
}

/// Register a project package to be added to this runtime's `PackageHost`.
///
/// Must be called before [`run`]; a generated web crate does this from its
/// own module-start function and then calls `run`. A later call has no
/// effect, because the host is initialized once, at startup.
pub fn register_project_package(adder: ProjectPackageAdder) {
    PROJECT_PACKAGES.with(|p| p.borrow_mut().push(adder));
}

/// The module-start function of a stock web build.
///
/// Behind the default-on `standalone` feature because wasm-bindgen allows one
/// start function per module: a generated per-project crate turns
/// `standalone` off, registers its packages from a start function of its own,
/// and then calls [`run`] - so a stock build is exactly what it was, and a
/// project build still has one start.
#[cfg(feature = "standalone")]
#[wasm_bindgen(start)]
pub fn start() {
    run();
}

/// Start the runtime: load the project, then run the frame loop.
///
/// Returns at once - the work is async (every asset arrives over `fetch`).
pub fn run() {
    console_error_panic_hook::set_once();
    let _ = console_log_init();
    console_log!("jEngine web runtime (wgpu) initializing...");
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = init_and_run().await {
            console_error!("jEngine init failed: {:?}", e);
        }
    });
}

/// Which GPU the browser handed WebGPU, from its own `GPUAdapter.info`.
///
/// wgpu's adapter info is blank on the web (`wgpu adapter:  (Other, ...)`),
/// and the answer matters: on a hybrid laptop Chrome runs WebGPU on the GPU
/// of the primary display, which is the integrated one - the showcase hub
/// measured 15 ms a frame there and a flat 6 on the discrete card. A second
/// `requestAdapter` returns the same adapter and costs nothing to speak of.
async fn log_browser_gpu() {
    use js_sys::Reflect;
    let get = |o: &JsValue, k: &str| Reflect::get(o, &JsValue::from_str(k)).ok().filter(|v| !v.is_undefined() && !v.is_null());
    let Some(gpu) = get(&window().navigator().into(), "gpu") else {
        console_log!("GPU: no WebGPU in this browser - drawing through WebGL");
        return;
    };
    let Some(request) = get(&gpu, "requestAdapter").and_then(|f| f.dyn_into::<js_sys::Function>().ok()) else {
        return;
    };
    let Ok(promise) = request.call0(&gpu) else { return };
    let Ok(adapter) = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(promise)).await else {
        return;
    };
    let Some(info) = get(&adapter, "info") else { return };
    let field = |k: &str| get(&info, k).and_then(|v| v.as_string()).filter(|s| !s.is_empty());
    let mut parts: Vec<String> = ["vendor", "architecture", "device", "description"]
        .iter()
        .filter_map(|k| field(k))
        .collect();
    if parts.is_empty() {
        parts.push("(the browser does not say)".into());
    }
    console_log!("GPU: WebGPU on {}", parts.join(" "));
}

fn console_log_init() -> Result<(), log::SetLoggerError> {
    // It used to set the level and install nothing, so the engine's own log
    // lines - which adapter and backend the renderer got among them - never
    // reached the console.
    jengine_core::web_log::init(log::LevelFilter::Info);
    Ok(())
}


/// The UI scale a project asks for.
///
/// The reference is the project's authored `width` x `height`, which every
/// project already declares, so scaling costs no new field;
/// `settings.ui_scale_mode` opts out. See `jengine_core::ui::scaling`.
fn ui_scale_from_settings(
    width: f32,
    height: f32,
    mode: Option<&str>,
) -> jengine_core::ui::scaling::UiScale {
    use jengine_core::ui::scaling::{UiScale, UiScaleMode};
    let mut scale = UiScale::from_reference(width, height);
    scale.mode = UiScaleMode::from_setting(mode);
    scale
}

async fn init_and_run() -> Result<(), JsValue> {
    // 1. Load project.jengine (best-effort — defaults if missing).
    let project: WebProjectFile = match fetch_text("project.jengine").await {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| JsValue::from_str(&format!("parse project.jengine: {e}")))?,
        Err(_) => {
            console_warn!("project.jengine not found, using defaults");
            WebProjectFile::default()
        }
    };
    let s = &project.settings;
    let width = s.width.unwrap_or(1280);
    let height = s.height.unwrap_or(720);
    let entry_scene = s
        .entry_scene
        .clone()
        .unwrap_or_else(|| "assets/scenes/main.scene".to_string());
    let bg_color = s.bg_color.unwrap_or([0.05, 0.05, 0.08, 1.0]);
    let ui_scale_mode = s.ui_scale_mode.clone();
    let texture_filters = s.texture_filter.clone();
    let gpu_preference = s.gpu_preference;
    let project_fps = s.frame_rate_cap.for_platform(jengine_core::time::CapPlatform::Web);
    let frame_cap = jengine_core::time::FrameCap::new(project_fps);
    // Asset packs, and the ones the Cache API still holds, before any scene:
    // a script must never see a cached pack as remote. Only a project that
    // declares packs asks for the manifest, so no other build logs a 404.
    let pack_manifest = if s.asset_packs.packs.is_empty() {
        None
    } else {
        fetch_text(&asset_url(jengine_core::asset::packs::MANIFEST_FILE)).await.ok()
    };
    // Live ops first: the snapshot believed last time names a content
    // release the packs are laid under before anything mounts.
    let live = jengine_core::liveops::web::init(&serde_json::json!({ "liveops": s.liveops, "app": s.app }), false);
    jengine_core::asset::packs::web::init(pack_manifest, &s.asset_packs, live.as_ref()).await;

    console_log!(
        "Project: {} ({}x{}) entry={}",
        s.name.as_deref().unwrap_or("(unnamed)"),
        width,
        height,
        entry_scene
    );

    // 2. Configure canvas.
    let canvas = get_canvas()?;
    canvas.set_width(width);
    canvas.set_height(height);

    // 3. Fetch + parse scene.
    let scene_json = fetch_text(&asset_url(&entry_scene)).await?;

    // 3.b — Backlog 2.2: fetch any project-listed prefabs and expand
    // `prefab_instance` entries before loading. v2 scenes pass through
    // untouched (handled by the expansion helper); a missing prefab
    // logs a warning and leaves the marker in place so the
    // diagnostic is visible.
    // The web builder writes `assets/prefab_manifest.json` listing every
    // `.prefab` it copied, but nothing ever populates `settings.prefabs` —
    // so on a stock build this list was empty and `spawn_prefab` had
    // nothing to spawn. Prefer an explicit project list when present, fall
    // back to the manifest.
    let mut prefab_paths: Vec<String> = s.prefabs.clone();
    if prefab_paths.is_empty() {
        #[derive(Deserialize)]
        struct PrefabManifest {
            #[serde(default)]
            prefabs: Vec<String>,
        }
        match fetch_text("assets/prefab_manifest.json").await {
            Ok(text) => match serde_json::from_str::<PrefabManifest>(&text) {
                Ok(m) => {
                    if !m.prefabs.is_empty() {
                        console_log!("prefab manifest: {} prefab(s)", m.prefabs.len());
                    }
                    prefab_paths = m.prefabs;
                }
                Err(e) => console_warn!("parse prefab_manifest.json: {}", e),
            },
            // No manifest is normal for hand-assembled web roots.
            Err(_) => {}
        }
    }

    let mut prefab_map: std::collections::HashMap<
        String,
        jengine_core::scene::format::EntityData,
    > = std::collections::HashMap::new();
    for prefab_path in &prefab_paths {
        match fetch_text(&asset_url(prefab_path)).await {
            Ok(text) => match jengine_core::scene::parse_prefab(prefab_path, &text) {
                Ok((key, entity_data)) => {
                    prefab_map.insert(key, entity_data);
                }
                Err(err) => {
                    console_warn!("parse prefab {}: {}", prefab_path, err);
                }
            },
            Err(err) => {
                console_warn!("fetch prefab {}: {:?}", prefab_path, err);
            }
        }
    }
    // A variant extends another prefab (`scene::prefab::resolve_prefab_variants`);
    // `app.prefab_map` keeps this, so later scenes and spawns see it resolved.
    let prefab_map = jengine_core::scene::resolve_prefab_variants(&prefab_map);
    let scene_json = jengine_core::scene::expand_prefab_instances_in_json(&scene_json, &prefab_map, "web");

    // Hand the prefab map to the script namespace so `spawn_prefab` has
    // component data to spawn (the browser wrapper reads
    // `__jengine_play.prefabs`).
    if !prefab_map.is_empty() {
        match serde_json::to_string(&prefab_map) {
            Ok(json) => script_host::seed_prefabs(&json),
            Err(e) => console_warn!("serialise prefabs for script namespace: {}", e),
        }
    }

    // Say which Lua path is live. Fengari is the intended one - the same VM
    // the editor runs, so behaviour verified there carries over. The
    // transpiler fallback accepts a narrower language and can differ
    // silently, so a build running on it should be loud about why.
    report_lua_runtime();

    // Every `.lua` / `.js` in the project, for `require()`. Must happen
    // before the first script compiles: module resolution is synchronous
    // inside the VM, so nothing can be fetched on demand.
    let project_scripts = fetch_project_scripts().await;
    if !project_scripts.is_empty() {
        let mut map = serde_json::Map::new();
        for (path, source) in &project_scripts {
            // Key both ways, matching the editor: scripts `require` either
            // spelling and the resolver tries the list verbatim.
            map.insert(path.clone(), serde_json::Value::String(source.clone()));
            map.insert(
                format!("assets/{path}"),
                serde_json::Value::String(source.clone()),
            );
        }
        let json = serde_json::to_string(&serde_json::Value::Object(map))
            .unwrap_or_else(|_| "{}".to_string());
        script_host::seed_prefab_scripts(&json);
        console_log!("loaded {} script module(s) for require()", project_scripts.len());
    }
    // The transpiler fallback path resolves `require` through a Rust
    // resolver rather than the Fengari shim, so install one over the same
    // sources. Kept alive for the session by the returned adapter.
    let module_adapter = install_module_resolver(&project_scripts);

    // Inline `.anim` `clip_source` references. The component itself never
    // loads anything, so without this an animator that points at a shared
    // `.anim` file starts with zero clips and the sprite never animates.
    let scene_json = inline_anim_sources_web(&scene_json).await;

    // 3.c — Data-oriented design: fetch every project-listed `.data` table and
    // seed `window.__jengine_play.data_tables` so the `get_data` script global
    // works identically to native + the editor viewport. Keyed by the table
    // path relative to `assets/` with the `.data` extension stripped.
    let data_tables = {
        let mut tables = serde_json::Map::new();
        for table_path in &s.data_tables {
            match fetch_text(&asset_url(table_path)).await {
                Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(parsed) => {
                        let rows = parsed
                            .get("rows")
                            .cloned()
                            .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
                        let key = table_path
                            .trim_start_matches("assets/")
                            .trim_end_matches(".data")
                            .to_string();
                        tables.insert(key, serde_json::json!({ "rows": rows }));
                    }
                    Err(err) => console_warn!("parse data table {}: {}", table_path, err),
                },
                Err(err) => console_warn!("fetch data table {}: {:?}", table_path, err),
            }
        }
        let count = tables.len();
        seed_data_tables(&tables);
        console_log!("Seeded {} data table(s) for get_data", count);
        tables
    };

    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    // Packages register into the same registry the scene loader uses, before
    // any scene is parsed - or a package component in the scene lands in
    // `CustomComponents` and the package never sees it. The same four
    // built-ins as the editor viewport: `jengine-physics` and
    // `jengine-particles` do nothing here, because this runtime steps rapier
    // and ticks its `ParticleSystem` itself and shares neither as a resource
    // (the resource contract in `docs/writing-packages.md`); parallax and
    // swarm run as systems, as they do in the editor.
    let mut package_host = jengine_core::package::host::PackageHost::new();
    #[cfg(feature = "physics")]
    package_host.add_package(Box::new(jengine_physics::PhysicsPackage));
    package_host.add_package(Box::new(jengine_particles::ParticlesPackage));
    package_host.add_package(Box::new(jengine_parallax::ParallaxPackage));
    package_host.add_package(Box::new(jengine_swarm::SwarmPackage));
    // Then the project's own, from a generated web crate if one is in use.
    PROJECT_PACKAGES.with(|p| {
        let adders = p.borrow();
        if !adders.is_empty() {
            console_log!("{} project package(s) to register", adders.len());
        }
        for add in adders.iter() {
            add(&mut package_host);
        }
    });
    package_host.initialize(&mut registry);
    // Package script globals have to exist in the browser preamble before
    // any script compiles, or they are simply undefined.
    jengine_core::scripting::bridge::adapters::js::set_package_script_globals(
        package_host.script_commands().browser_globals_js(),
    );
    if !package_host.script_commands().is_empty() {
        console_log!(
            "{} package script command(s): {}",
            package_host.script_commands().len(),
            package_host.script_commands().names().join(", ")
        );
    }
    // Fengari installs globals from an allowlist that cannot know about
    // packages, so the names go onto `__jengine_play` for it to read.
    // With them, every script namespace, which `lua-runtime.ts` builds its
    // `Sprite.set_color` tables from.
    script_host::publish_package_api_names(package_host.script_commands());
    // Published process-wide: the command drain dispatches while `app` is
    // borrowed as the world host, so it could not also lend a registry it
    // owned.
    jengine_core::package::script_commands::install_global(package_host.take_script_commands());

    let mut world = World::new();
    load_scene_auto(&scene_json, &mut world, &registry)
        .map_err(|e| JsValue::from_str(&format!("parse scene: {e}")))?;
    jengine_core::scene::set_current_name(&jengine_core::scene::name_of(&scene_json).unwrap_or_default());
    console_log!("Loaded scene with {} entities", world.len());

    // 4. Build wgpu renderer (async — adapter+device acquisition).
    let renderer = WgpuRenderer::new_from_canvas_async_with_gpu(canvas.clone(), UVec2::new(width, height), gpu_preference)
        .await
        .map_err(|e| JsValue::from_str(&format!("WgpuRenderer init: {e}")))?;
    log_browser_gpu().await;
    // The world is drawn in `scene_format` (float where the adapter has one -
    // WebGPU always does; WebGL2 may not); the batch switches to
    // `color_format` for the UI, drawn onto the display after the post chain.
    let sprite_batch = WgpuSpriteBatch::new(&renderer.device, renderer.scene_format)
        .map_err(|e| JsValue::from_str(&format!("WgpuSpriteBatch init: {e}")))?;
    let mut post_process = WgpuPostProcessStack::new(
        &renderer.device,
        width.max(1),
        height.max(1),
        renderer.scene_format,
        renderer.color_format,
    )
    .map_err(|e| JsValue::from_str(&format!("WgpuPostProcessStack init: {e}")))?;
    // The entry scene's authored chain, so effects render from frame 0 without
    // a script re-toggling them - the same thing native does at startup.
    post_process.set_settings(
        jengine_core::renderer::postprocess::PostProcessSettings::from_scene_json(&scene_json)
            .unwrap_or_default(),
    );
    let lit_sprite_batch = jengine_core::renderer::backend::wgpu::WgpuLitSpritePipeline::new(
        &renderer.device,
        renderer.scene_format,
    )
    .map_err(|e| JsValue::from_str(&format!("WgpuLitSpritePipeline init: {e}")))?;
    let flat_normal = jengine_core::renderer::backend::wgpu::WgpuLitSpritePipeline::flat_normal_texture(
        &renderer.device,
        &renderer.queue,
    );
    let light_pass = WgpuLightingPass::new(
        &renderer.device,
        width.max(1),
        height.max(1),
        renderer.scene_format,
    )
    .map_err(|e| JsValue::from_str(&format!("WgpuLightingPass init: {e}")))?;

    // 5. Pre-load all sprite textures + GUI documents via fetch.
    let mut assets = AssetManager::new();
    assets.set_texture_filters(texture_filters);

    // 1×1 white fallback for untextured particle emitters / UI quads.
    assets.insert_texture_bytes(
        "__white".to_string(),
        RawTextureData {
            width: 1,
            height: 1,
            rgba: vec![255, 255, 255, 255],
        },
    );
    // What on-screen touch controls are drawn with.
    jengine_core::input::touch_controls::insert_textures(&mut assets);

    // Built-in font atlas (lazy wgpu upload on first text/UI flush).
    let (font_rgba, font_w, font_h, font_data) = built_in_font_atlas();
    assets.insert_texture_bytes(
        BUILTIN_FONT_ID.to_string(),
        RawTextureData {
            width: font_w,
            height: font_h,
            rgba: font_rgba,
        },
    );
    // A distance field (`renderer::text`): drawn sharp at any size.
    assets.mark_sdf(BUILTIN_FONT_ID);

    fetch_world_textures(&world, &mut assets).await;
    insert_atlases(&mut assets, fetch_scene_atlases(&scene_json).await);
    fetch_world_guis(&world, &mut assets).await;
    fetch_world_fonts(&world, &mut assets).await;
    load_skeletal_clips(&mut assets).await;

    // Prefab textures too — nothing in the scene references them until a
    // script spawns one, and by then there is no way to fetch.
    {
        let ids: Vec<String> = prefab_texture_ids(&prefab_map)
            .into_iter()
            .filter(|id| assets.get_texture_bytes(id).is_none())
            .collect();
        if !ids.is_empty() {
            console_log!("preloading {} prefab texture(s)", ids.len());
            for (id, raw) in fetch_textures(ids).await {
                assets.insert_texture_bytes(id, raw);
            }
        }
    }

    // Behavior graphs referenced by this scene.
    let mut pending_behaviors: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    fetch_world_behaviors(&world, &mut pending_behaviors).await;
    for path in prefab_behavior_paths(&prefab_map) {
        if pending_behaviors.contains_key(&path) {
            continue;
        }
        match fetch_text(&asset_url(&path)).await {
            Ok(text) => {
                pending_behaviors.insert(path, text);
            }
            Err(e) => console_warn!("fetch prefab behavior '{}': {:?}", path, e),
        }
    }

    // Audio clips declared in the scene's top-level `audio_refs`. Decoding
    // is async inside the backend; clips become playable a few ms later.
    let mut web_audio = jengine_core::audio::web_audio::WebAudioBackend::new();
    fetch_scene_audio(&scene_json, &mut web_audio).await;

    // Feature 3.7 — fetch user script sources referenced by ScriptComponents
    // so behavior matches native + the editor viewport. Sources are fetched
    // up-front (deduped) here; compilation + init happen after the app is
    // built (see `install_scripts` below).
    let loaded_scripts = fetch_world_scripts(&world).await;

    // Scene-level sort layers — the same fetch the transition path uses.
    if let Some((id, text)) = fetch_scene_sort_layers(&scene_json).await {
        match assets.load_sort_layers_from_str(&id, &text) {
            Ok(()) => assets.set_active_sort_layers(Some(id)),
            Err(e) => console_log!("[web] sort_layers parse failed: {e:#}"),
        }
    }

    // 6. Apply background clear color.
    let mut renderer = renderer;
    renderer.set_clear_color(bg_color[0], bg_color[1], bg_color[2], bg_color[3]);

    // 7. Build InputState with project-defined actions.
    let mut input = InputState::new();
    if !s.input_actions.is_empty() {
        input.load_actions(&s.input_actions);
        console_log!("Loaded {} input action(s)", s.input_actions.len());
    }

    // 7.b — Build the physics world (feature 3.7). Only when the project
    // requests a backend and the `physics` cargo feature is compiled in;
    // otherwise collisions/physics commands are inert (parity with the
    // builder's `--no-default-features` path).
    #[cfg(feature = "physics")]
    let physics: Option<jengine_core::physics::PhysicsWorld> = if s.physics.backend != "none" {
        console_log!(
            "Physics enabled: backend={}, gravity=({}, {}), length_unit={}",
            s.physics.backend,
            s.physics.gravity_x,
            s.physics.gravity_y,
            s.physics.length_unit,
        );
        let mut world = jengine_core::physics::PhysicsWorld::new(
            s.physics.gravity_x,
            s.physics.gravity_y,
            s.physics.length_unit,
        );
        world.set_fixed_timestep(s.physics.fixed_timestep);
        Some(world)
    } else {
        None
    };
    #[cfg(not(feature = "physics"))]
    let physics: Option<jengine_core::physics::PhysicsWorld> = None;

    // Remembered so a scene transition can rebuild the physics world from
    // scratch — rapier body handles belong to the entities being replaced.
    let physics_config: Option<(f32, f32, f32)> = if s.physics.backend != "none" {
        Some((
            s.physics.gravity_x,
            s.physics.gravity_y,
            s.physics.length_unit,
        ))
    } else {
        None
    };

    let app = Rc::new(RefCell::new(WebApp {
        renderer,
        sprite_batch,
        assets,
        missing_warned: HashSet::new(),
        texture_requested: HashSet::new(),
        packs_generation: jengine_core::asset::packs::generation(),
        data_tables,
        graph_pipelines: jengine_core::renderer::graph_pipeline_cache::GraphPipelineCache::new(),
        shader_graphs: std::collections::HashMap::new(),
        // A prefab's materials are fetched from the start, not on the first
        // draw that wants one: a graph over a white quad drawn plain for the
        // frames the fetch takes is a white box.
        material_missing: jengine_core::scene::format::prefab_material_ids(prefab_map.values()).into_iter().collect(),
        material_requested: HashSet::new(),
        world,
        camera: Camera2D::new(width as f32, height as f32),
        prev_camera: glam::Vec2::ZERO,
        particle_system: ParticleSystem::new(),
        post_process,
        light_pass,
        view_targets: Default::default(),
        lit_sprite_batch,
        flat_normal,
        font_data,
        input,
        touch: jengine_core::input::touch_controls::TouchControlsState::new(),
        frame_cap,
        last_drawn: f64::NEG_INFINITY,
        project_fps,
        fps: project_fps,
        display: Default::default(),
        text_proxy: Default::default(),
        safe_area_recheck_in: 0,
        canvas: canvas.clone(),
        last_time: performance().now(),
        refresh_estimate: jengine_core::time::RefreshEstimate::new(),
        pacer: jengine_core::time::FramePacer::new(),
        start_time: performance().now(),
        time_scale: 1.0,
        host: platform::HostState::default(),
        canvas_size: UVec2::new(width, height),
        debug_overlay: jengine_core::debug_overlay::DebugOverlayState::default(),
        debug_overlay_enabled: project.settings.debug_overlay,
        frame_stats: jengine_core::profiling::FrameStats::default(),
        fps_ema: 0.0,
        script_fns: std::collections::HashMap::new(),
        spawn_notified: std::collections::HashSet::new(),
        script_cache: std::collections::HashMap::new(),
        tween_manager: jengine_core::animation::tween::TweenManager::new(),
        video: jengine_core::video::VideoSystem::new(Box::new(|source: &str| {
            let url = asset_url(source);
            // A video from an asset pack plays from a Blob of its bytes.
            Some(Box::new(jengine_core::video::web::WebVideoDecoder::new(Box::new(move || {
                Some(jengine_core::asset::packs::web::blob_url(&url).unwrap_or_else(|| url.clone()))
            })))
                as Box<dyn jengine_core::video::VideoDecoder>)
        })),
        play_namespace_ready: false,
        pending_camera_shakes: Vec::new(),
        registry,
        package_host,
        physics,
        behavior_system: jengine_core::behavior::BehaviorGraphSystem::new(),
        pending_behaviors,
        prefab_map,
        web_audio,
        spatial_audio: jengine_core::audio::spatial::SpatialAudio::new(),
        audio_warned: HashSet::new(),
        save_system: jengine_core::save::SaveSystem::new(),
        mixer: {
            let mut mixer = jengine_core::audio::AudioMixer::new();
            mixer.apply_layout(&s.audio_mixer);
            mixer
        },
        debug_draws: Vec::new(),
        fade_active: None,
        fade_overlay: None,
        nav_snapshot: std::rc::Rc::new(std::cell::RefCell::new(
            jengine_core::scripting::bridge::adapters::js::nav::NavSnapshot::new(),
        )),
        #[cfg(feature = "physics")]
        ray_snapshot: std::rc::Rc::new(std::cell::RefCell::new(
            jengine_core::physics::ray::RaySnapshot::new(),
        )),
        physics_config,
        path_library: build_path_library(&scene_json),
        gui_scroll: Default::default(),
        current_scripts: loaded_scripts.clone(),
        scene_stack: Vec::new(),
        transitioning: false,
        ui_scale: ui_scale_from_settings(width as f32, height as f32, ui_scale_mode.as_deref()),
        gui_click_callbacks: std::collections::HashMap::new(),
        _module_adapter: module_adapter,
    }));

    // 8. Wire input listeners.
    install_input_listeners(&canvas, app.clone())?;

    // 8.b — Compile + attach scripts and run their `init` (parity 3.7).
    if !loaded_scripts.is_empty() {
        let mut app_ref = app.borrow_mut();
        script_host::install_scripts(&mut app_ref, loaded_scripts);
    }

    // 9. Start RAF loop.
    let f: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    let app_loop = app.clone();
    let mut announced = false;
    *g.borrow_mut() = Some(Closure::new(move |timestamp: f64| {
        // A frame-rate cap skips the callbacks that come too soon; the next
        // drawn frame's dt covers the skipped time, so the game clock keeps
        // up with the wall. A portal holding the game (`apply_host_state`)
        // skips them all.
        let due = !apply_host_state(&app_loop, timestamp) && {
            let mut a = app_loop.borrow_mut();
            let due = a.frame_cap.ready((timestamp - a.last_drawn) / 1000.0);
            if due {
                a.last_drawn = timestamp;
            }
            due
        };
        if due {
            if let Err(e) = render_frame(&app_loop, timestamp) {
                console_error!("frame error: {:?}", e);
            }
            if !announced {
                announced = true;
                announce_ready();
            }
            // Outside `render_frame`'s borrow: a transition is async and
            // needs the cell to itself once its assets land.
            poll_scene_transition(&app_loop);
            // Scenes streamed in or preloaded (`Scene.load_additive`).
            poll_streaming(&app_loop);
            poll_asset_packs(&app_loop);
            poll_missing_textures(&app_loop);
            poll_missing_materials(&app_loop);
        }
        request_animation_frame(f.borrow().as_ref().unwrap());
    }));
    request_animation_frame(g.borrow().as_ref().unwrap());

    Ok(())
}

// ---------------------------------------------------------------------------
// Asset preloading
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Scene transitions
// ---------------------------------------------------------------------------

/// Start a scene change if a script asked for one.
///
/// Called once per frame from the RAF closure, outside `render_frame`'s
/// borrow: the work is async (every asset arrives over `fetch`), so it runs
/// as a detached task and swaps the world in when everything has landed.
/// Build a patrol-path library from a scene's declared routes.
///
/// `None` when the scene declares none, which `update_patrol_paths` tolerates
/// and which keeps the common case free.
fn build_path_library(scene_json: &str) -> Option<jengine_core::ecs::path::Path2DLibrary> {
    let meta = jengine_core::scene::format::read_scene_metadata(scene_json).ok()?;
    if meta.paths.is_empty() {
        return None;
    }
    let mut lib = jengine_core::ecs::path::Path2DLibrary::new();
    for (id, path) in meta.paths {
        lib.insert(id, path);
    }
    Some(lib)
}

/// Start the pack downloads scripts asked for (`asset::packs::web`); once a
/// pack has mounted, ask again for the textures given up on while it was
/// remote, and add what it brought to what this runtime loaded once at boot:
/// prefabs (and their behaviour graphs), the scripts `require` can reach,
/// data tables, skeletal clips and rigs. A script on an entity needs
/// nothing - it is fetched when its scene loads, through `fetch_text`.
fn poll_asset_packs(app_cell: &Rc<RefCell<WebApp>>) {
    use jengine_core::asset::packs;
    // Live ops first: a snapshot it believes may change the packs.
    jengine_core::liveops::web::sync();
    packs::web::sync();
    let generation = packs::generation();
    let arrived = packs::web::arrived();
    let mut app = app_cell.borrow_mut();
    let app = &mut *app;
    if generation != app.packs_generation {
        app.packs_generation = generation;
        app.texture_requested.clear();
    }
    for content in arrived {
        if content.is_empty() {
            continue;
        }
        let added = content.add_prefabs(&mut app.prefab_map);
        if !added.is_empty() {
            match serde_json::to_string(&app.prefab_map) {
                Ok(json) => script_host::seed_prefabs(&json),
                Err(e) => console_warn!("serialise prefabs for script namespace: {}", e),
            }
            let new: std::collections::HashMap<_, _> =
                added.iter().filter_map(|n| app.prefab_map.get(n).map(|d| (n.clone(), d.clone()))).collect();
            for path in prefab_behavior_paths(&new) {
                if app.pending_behaviors.contains_key(&path) {
                    continue;
                }
                if let Some(text) = packs::web::read(&path).and_then(|b| String::from_utf8(b).ok()) {
                    app.pending_behaviors.insert(path, text);
                    continue;
                }
                let cell = app_cell.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    match fetch_text(&asset_url(&path)).await {
                        Ok(text) => {
                            cell.borrow_mut().pending_behaviors.insert(path, text);
                        }
                        Err(e) => console_warn!("fetch prefab behavior '{}': {:?}", path, e),
                    }
                });
            }
        }
        if !content.scripts.is_empty() {
            add_require_modules(&content.scripts);
        }
        let rows = content.table_rows();
        if !rows.is_empty() {
            app.data_tables.extend(rows);
            seed_data_tables(&app.data_tables);
        }
        let skeletal = content.add_skeletal(&mut app.assets);
        console_log!(
            "[packs] '{}' brought {} prefab(s), {} data table(s), {} skeletal clip(s) and rig(s), {} script(s)",
            content.name,
            added.len(),
            content.tables.len(),
            skeletal,
            content.scripts.len()
        );
    }
}

/// Fetch the textures the renderer found missing - a sprite whose texture no
/// scene named, such as a package's own sprites - and install each as it
/// arrives. Called once per frame from the RAF closure, like
/// `poll_scene_transition`; the fetch runs detached.

fn poll_missing_textures(app_cell: &Rc<RefCell<WebApp>>) {
    let ids = {
        let mut app = app_cell.borrow_mut();
        let app = &mut *app;
        jengine_core::asset::textures_to_request(&app.missing_warned, &mut app.texture_requested, &app.assets)
    };
    if ids.is_empty() {
        return;
    }
    let app_cell = app_cell.clone();
    wasm_bindgen_futures::spawn_local(async move {
        for (id, raw) in fetch_textures(ids).await {
            let mut app = app_cell.borrow_mut();
            app.assets.insert_texture_bytes(id.clone(), raw);
            app.missing_warned.remove(&id);
        }
    });
}

/// Fetch the `.material` files a draw asked for and nothing has loaded, and
/// each one's shader graph, as they arrive - the lazy path textures take,
/// so a scene's materials, a prefab's and one a script switches to at run
/// time all load the same way. Until Sep 2026 web builds drew every graph
/// material with the default sprite shader: nothing fetched them.
fn poll_missing_materials(app_cell: &Rc<RefCell<WebApp>>) {
    let ids: Vec<String> = {
        let mut app = app_cell.borrow_mut();
        let app = &mut *app;
        let new: Vec<String> = app.material_missing.difference(&app.material_requested).cloned().collect();
        for id in &new {
            app.material_requested.insert(id.clone());
        }
        new
    };
    if ids.is_empty() {
        return;
    }
    let app_cell = app_cell.clone();
    wasm_bindgen_futures::spawn_local(async move {
        for mid in ids {
            // As native looks: `assets/<id>.material`, then `<id>.material`.
            let mut json = None;
            for url in [asset_url(&format!("{mid}.material")), format!("{mid}.material")] {
                if let Ok(text) = fetch_text(&url).await {
                    json = Some(text);
                    break;
                }
            }
            let Some(json) = json else {
                console_warn!("material '{}' has no .material file - drawn with the default shader", mid);
                continue;
            };
            let graph_path = {
                let mut app = app_cell.borrow_mut();
                if let Err(e) = app.assets.load_material_from_str(&mid, &json) {
                    console_warn!("material '{}': {:#}", mid, e);
                    continue;
                }
                app.assets.get_material(&mid).and_then(|m| m.shader_graph_path.clone())
            };
            let Some(path) = graph_path else { continue };
            if app_cell.borrow().shader_graphs.contains_key(&path) {
                continue;
            }
            match fetch_text(&asset_url(&path)).await {
                Ok(text) => match serde_json::from_str::<jengine_core::renderer::shader_graph::ShaderGraph>(&text) {
                    Ok(graph) => {
                        app_cell.borrow_mut().shader_graphs.insert(path, graph);
                    }
                    Err(e) => console_warn!("shader graph '{}': {}", path, e),
                },
                Err(e) => console_warn!("fetch shader graph '{}': {:?}", path, e),
            }
        }
    });
}

/// This runtime's part of `ecs::frame_draw`: materials are looked up among
/// the graphs it has fetched (one not loaded yet is noted for
/// `poll_missing_materials`), and a texture with no pixels is being fetched.
struct WebDrawHost<'a> {
    graphs: &'a std::collections::HashMap<String, jengine_core::renderer::shader_graph::ShaderGraph>,
    missing: &'a mut HashSet<String>,
}

impl jengine_core::ecs::frame_draw::DrawHost for WebDrawHost<'_> {
    fn material_pipeline(
        &mut self,
        assets: &mut AssetManager,
        graph_pipelines: &mut jengine_core::renderer::graph_pipeline_cache::GraphPipelineCache,
        warned: &mut HashSet<String>,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        batch: &WgpuSpriteBatch,
        mid: &str,
    ) -> Option<std::sync::Arc<wgpu::RenderPipeline>> {
        material_pipeline(assets, self.graphs, graph_pipelines, self.missing, warned, device, format, batch, mid)
    }
    fn texture_missing(&mut self, tex_id: &str) {
        console_log!("texture '{}' not loaded yet - fetching it", tex_id);
    }
    fn warn(&mut self, message: &str) {
        console_warn!("{}", message);
    }
}

/// The graph pipeline a sprite's material draws with, or `None` for the
/// default one. A material not loaded yet is noted for
/// `poll_missing_materials` and drawn plain until it arrives. One function
/// for the world pass and the emission pass (unlit sprites in a lit scene).
#[allow(clippy::too_many_arguments)]
fn material_pipeline(
    assets: &AssetManager,
    graphs: &std::collections::HashMap<String, jengine_core::renderer::shader_graph::ShaderGraph>,
    graph_pipelines: &mut jengine_core::renderer::graph_pipeline_cache::GraphPipelineCache,
    missing: &mut HashSet<String>,
    warned: &mut HashSet<String>,
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    batch: &WgpuSpriteBatch,
    mid: &str,
) -> Option<std::sync::Arc<wgpu::RenderPipeline>> {
    use jengine_core::renderer::graph_pipeline_cache::GraphPipelineKey;
    if jengine_core::renderer::material::is_builtin_material(mid) {
        return None;
    }
    let Some(material) = assets.get_material(mid) else {
        missing.insert(mid.to_string());
        return None;
    };
    let graph_path = material.shader_graph_path.clone()?;
    let key = GraphPipelineKey { shader_graph_path: graph_path.clone(), blend_mode: material.blend_mode };
    if let Some(p) = graph_pipelines.get(&key, format) {
        return Some(std::sync::Arc::clone(p));
    }
    let graph = graphs.get(&graph_path)?;
    match graph_pipelines.insert_from_graph(device, format, batch, key, graph) {
        Ok(p) => Some(p),
        Err(e) => {
            if warned.insert(format!("__gp:{graph_path}")) {
                console_warn!("graph pipeline build failed for '{}': {:#}", graph_path, e);
            }
            None
        }
    }
}

/// `jengine_core::scene::streaming` for a web build: each additive chunk or
/// preload is fetched on its own task - the scene, its textures, documents,
/// fonts, sounds and scripts - while the frame keeps drawing; a chunk is then
/// laid into the world and its scripts started. An unload takes the chunk
/// out and its script instances with it.
fn poll_streaming(app_cell: &Rc<RefCell<WebApp>>) {
    use jengine_core::scene::streaming::{self as st, Request};
    for req in st::take_requests() {
        match req {
            Request::Unload { handle } => {
                let mut app = app_cell.borrow_mut();
                let gone = st::unload_chunk(&mut app.world, handle);
                for e in gone {
                    app.script_fns.remove(&e.id());
                }
            }
            Request::Additive { handle, path, offset } => {
                let app_cell = app_cell.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Err(e) = stream_scene(&app_cell, Some(handle), &path, offset).await {
                        console_warn!("[stream] '{}' failed: {:?}", path, e);
                        st::set_status(handle, st::Status::Failed);
                    }
                });
            }
            Request::Preload { path } => {
                let app_cell = app_cell.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Err(e) = stream_scene(&app_cell, None, &path, [0.0, 0.0]).await {
                        console_warn!("[stream] preload '{}' failed: {:?}", path, e);
                        st::set_preload(&path, st::Status::Failed);
                    }
                });
            }
        }
    }
}

/// Fetch one streamed scene and everything it needs; lay it in when it is a
/// chunk still wanted.
async fn stream_scene(
    app_cell: &Rc<RefCell<WebApp>>,
    handle: Option<u32>,
    rel_path: &str,
    offset: [f32; 2],
) -> Result<(), JsValue> {
    use jengine_core::scene::streaming::{self as st, Status};
    let progress = |p: f32| match handle {
        Some(h) => st::set_status(h, Status::Loading(p)),
        None => st::set_preload(rel_path, Status::Loading(p)),
    };
    let text = fetch_text(&asset_url(rel_path)).await?;
    let scene_json = inline_anim_sources_web(&text).await;
    let (prefab_map, registry) = {
        let app = app_cell.borrow();
        (app.prefab_map.clone(), app.registry.clone())
    };
    let scene_json = jengine_core::scene::expand_prefab_instances_in_json(&scene_json, &prefab_map, "web");
    let mut chunk = World::new();
    load_scene_auto(&scene_json, &mut chunk, &registry).map_err(|e| JsValue::from_str(&format!("load scene: {e}")))?;
    progress(0.1);
    let (missing_textures, missing_guis, missing_fonts, mut behaviors) = {
        let app = app_cell.borrow();
        let tex: Vec<String> = wanted_texture_ids(&chunk)
            .into_iter()
            .filter(|id| app.assets.get_texture_bytes(id).is_none())
            .collect();
        let guis: Vec<String> = wanted_gui_ids(&chunk).into_iter().filter(|id| app.assets.get_gui(id).is_none()).collect();
        (tex, guis, missing_font_ids(&chunk, &app.assets), app.pending_behaviors.clone())
    };
    // Textures one at a time, so the progress moves as they arrive.
    let n = missing_textures.len().max(1) as f32;
    for (i, id) in missing_textures.into_iter().enumerate() {
        for (id, raw) in fetch_textures(vec![id]).await {
            app_cell.borrow_mut().assets.insert_texture_bytes(id, raw);
        }
        progress(0.1 + 0.7 * (i + 1) as f32 / n);
    }
    let guis = fetch_guis(missing_guis).await;
    let gui_textures = {
        let ids = gui_texture_ids(&guis, &app_cell.borrow().assets);
        fetch_textures(ids).await
    };
    let fonts = fetch_fonts(missing_fonts).await;
    fetch_world_behaviors(&chunk, &mut behaviors).await;
    let audio_clips = fetch_scene_audio_bytes(&scene_json).await;
    let atlases = fetch_scene_atlases(&scene_json).await;
    let scripts = if handle.is_some() { fetch_world_scripts(&chunk).await } else { Vec::new() };

    let mut app = app_cell.borrow_mut();
    insert_atlases(&mut app.assets, atlases);
    for (id, raw) in gui_textures {
        app.assets.insert_texture_bytes(id, raw);
    }
    for (id, doc) in guis {
        app.assets.insert_gui(id, doc);
    }
    insert_fonts(&mut app.assets, fonts);
    app.pending_behaviors = behaviors;
    for (clip_id, bytes) in audio_clips {
        if let Err(e) = app.web_audio.load_clip(&clip_id, &bytes) {
            console_warn!("decode audio '{}': {}", clip_id, e);
        }
    }
    let Some(handle) = handle else {
        st::set_preload(rel_path, Status::Loaded);
        return Ok(());
    };
    if !st::is_wanted(handle) {
        return Ok(());
    }
    let pairs = st::merge(&mut app.world, chunk, handle, glam::Vec2::from(offset));
    let ids: std::collections::HashMap<u32, u32> = pairs.iter().map(|(o, n)| (o.id(), n.id())).collect();
    let scripts: Vec<script_host::LoadedScript> = scripts
        .into_iter()
        .filter_map(|mut s| {
            s.entity_id = *ids.get(&s.entity_id)?;
            Some(s)
        })
        .collect();
    script_host::attach_scripts(&mut app, scripts);
    st::set_status(handle, Status::Loaded);
    console_log!("[stream] chunk {} '{}' laid in ({} entities)", handle, rel_path, pairs.len());
    Ok(())
}

fn poll_scene_transition(app_cell: &Rc<RefCell<WebApp>>) {
    let Some(pending) = script_host::take_pending_scene() else {
        return;
    };
    {
        let mut app = app_cell.borrow_mut();
        if app.transitioning {
            // A change is already in flight; drop this one rather than
            // racing two worlds into the same slot.
            return;
        }
        app.transitioning = true;
    }
    let app_cell = app_cell.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let result = match pending.action.as_str() {
            "pop" => pop_scene(&app_cell).await,
            action => {
                let path = pending.path.clone().unwrap_or_default();
                if path.is_empty() {
                    Err(JsValue::from_str("scene transition without a path"))
                } else {
                    load_scene_transition(&app_cell, &path, action == "push").await
                }
            }
        };
        if let Err(e) = result {
            console_error!("scene transition failed: {:?}", e);
        }
        app_cell.borrow_mut().transitioning = false;
    });
}

/// Fetch `rel_path` and make it the running scene, optionally pushing the
/// outgoing one onto the scene stack.
async fn load_scene_transition(
    app_cell: &Rc<RefCell<WebApp>>,
    rel_path: &str,
    push: bool,
) -> Result<(), JsValue> {
    console_log!("scene transition -> {}", rel_path);
    let text = fetch_text(&asset_url(rel_path)).await?;
    let scene_json = inline_anim_sources_web(&text).await;
    let (prefab_map, registry) = {
        let app = app_cell.borrow();
        (app.prefab_map.clone(), app.registry.clone())
    };
    let scene_json = jengine_core::scene::expand_prefab_instances_in_json(&scene_json, &prefab_map, "web");
    apply_scene_json(app_cell, scene_json, &registry, push).await
}

/// Restore the scene most recently pushed by `push_scene`.
async fn pop_scene(app_cell: &Rc<RefCell<WebApp>>) -> Result<(), JsValue> {
    let (snapshot, registry) = {
        let mut app = app_cell.borrow_mut();
        let registry = app.registry.clone();
        match app.scene_stack.pop() {
            Some(s) => (s, registry),
            None => {
                console_warn!("pop_scene with an empty scene stack — ignored");
                return Ok(());
            }
        }
    };
    console_log!("scene transition <- pop");
    apply_scene_json(app_cell, snapshot.scene_json.clone(), &registry, false).await?;
    // A pop restores the scripts the snapshot was running, not whatever the
    // scene file declares — the world came back mid-flight, so its scripts
    // must too.
    let mut app = app_cell.borrow_mut();
    // `apply_scene_json` above set the chain from the snapshot's JSON, which
    // has no `post_process` block - so put back the one the scene was
    // actually rendering with.
    app.post_process.set_settings(snapshot.post_process.clone());
    let scripts = snapshot.scripts.clone();
    app.current_scripts = scripts.clone();
    script_host::install_scripts(&mut app, scripts);
    Ok(())
}

/// Build a world from `scene_json`, fetch everything it needs, and swap it
/// in — the shared tail of load / push / pop.
///
/// All fetching happens before any `RefCell` borrow is held, so the render
/// loop keeps drawing the outgoing scene while assets arrive; the swap
/// itself is one synchronous borrow.
async fn apply_scene_json(
    app_cell: &Rc<RefCell<WebApp>>,
    scene_json: String,
    registry: &ComponentRegistry,
    push: bool,
) -> Result<(), JsValue> {
    let mut world = World::new();
    load_scene_auto(&scene_json, &mut world, registry)
        .map_err(|e| JsValue::from_str(&format!("load scene: {e}")))?;

    // Assets the new world needs that the cache doesn't already hold.
    let (missing_textures, missing_guis, missing_fonts, mut behaviors) = {
        let app = app_cell.borrow();
        let mut wanted = wanted_texture_ids(&world);
        // Prefabs are project-wide, so a scene entered later can spawn one
        // whose texture the previous scenes never needed.
        wanted.extend(prefab_texture_ids(&app.prefab_map));
        let tex: Vec<String> = wanted
            .into_iter()
            .filter(|id| app.assets.get_texture_bytes(id).is_none())
            .collect();
        let guis: Vec<String> = wanted_gui_ids(&world)
            .into_iter()
            .filter(|id| app.assets.get_gui(id).is_none())
            .collect();
        let fonts = missing_font_ids(&world, &app.assets);
        (tex, guis, fonts, app.pending_behaviors.clone())
    };
    let textures = fetch_textures(missing_textures).await;
    let guis = fetch_guis(missing_guis).await;
    let gui_textures = {
        let ids = gui_texture_ids(&guis, &app_cell.borrow().assets);
        fetch_textures(ids).await
    };
    let fonts = fetch_fonts(missing_fonts).await;
    fetch_world_behaviors(&world, &mut behaviors).await;
    {
        let paths = {
            let app = app_cell.borrow();
            prefab_behavior_paths(&app.prefab_map)
        };
        for path in paths {
            if behaviors.contains_key(&path) {
                continue;
            }
            match fetch_text(&asset_url(&path)).await {
                Ok(text) => {
                    behaviors.insert(path, text);
                }
                Err(e) => console_warn!("fetch prefab behavior '{}': {:?}", path, e),
            }
        }
    }
    let audio_clips = fetch_scene_audio_bytes(&scene_json).await;
    let atlases = fetch_scene_atlases(&scene_json).await;
    let sort_layers = fetch_scene_sort_layers(&scene_json).await;
    let scripts = fetch_world_scripts(&world).await;

    // Snapshot the outgoing scene before it is dropped.
    let snapshot = if push {
        let app = app_cell.borrow();
        let data = jengine_core::scene::format::snapshot_scene_v2(&app.world, &app.registry);
        serde_json::to_string(&data).ok().map(|scene_json| SceneSnapshot {
            scene_json,
            scripts: app.current_scripts.clone(),
            post_process: app.post_process.settings().clone(),
        })
    } else {
        None
    };

    let mut app = app_cell.borrow_mut();
    if let Some(snapshot) = snapshot {
        app.scene_stack.push(snapshot);
    }

    for (id, raw) in textures.into_iter().chain(gui_textures) {
        app.assets.insert_texture_bytes(id, raw);
    }
    insert_atlases(&mut app.assets, atlases);
    for (id, doc) in guis {
        app.assets.insert_gui(id, doc);
    }
    insert_fonts(&mut app.assets, fonts);
    app.pending_behaviors = behaviors;
    let mut audio_loaded = 0usize;
    for (clip_id, bytes) in audio_clips {
        match app.web_audio.load_clip(&clip_id, &bytes) {
            Ok(()) => audio_loaded += 1,
            Err(e) => console_warn!("decode audio '{}': {}", clip_id, e),
        }
    }
    if audio_loaded > 0 {
        console_log!("loaded {} audio clip(s)", audio_loaded);
    }
    match sort_layers {
        Some((id, text)) => match app.assets.load_sort_layers_from_str(&id, &text) {
            Ok(()) => app.assets.set_active_sort_layers(Some(id)),
            Err(e) => console_warn!("sort_layers parse failed: {:#}", e),
        },
        None => app.assets.set_active_sort_layers(None),
    }
    app.path_library = build_path_library(&scene_json);

    // The outgoing scene's scripts are about to stop existing. Fire their
    // `on_despawn` while the old world is still in place — it is the only
    // moment they can release anything the world does not own, such as
    // looping audio.
    script_host::dispatch_despawn_for_all(&mut app);

    // Swap the world and drop every piece of per-scene runtime state whose
    // handles point at entities that no longer exist.
    app.world = world;
    // Its additive chunks went with the old world.
    jengine_core::scene::streaming::scene_replaced();
    jengine_core::scene::set_current_name(&jengine_core::scene::name_of(&scene_json).unwrap_or_default());
    app.particle_system = ParticleSystem::new();
    // The effect chain belongs to the scene, not to the session: a scene
    // declaring none gets none, rather than inheriting the last scene's
    // bloom. Native and the editor viewport have always replaced it here;
    // web read the block nowhere at all, so a scene that authored effects
    // rendered without them and script-set effects followed the player into
    // every scene after (BUGS #84).
    app.post_process
        .set_settings(jengine_core::renderer::postprocess::PostProcessSettings::from_scene_json(&scene_json).unwrap_or_default());
    // Bindings point at entities of the outgoing scene.
    app.gui_click_callbacks.clear();
    app.behavior_system = jengine_core::behavior::BehaviorGraphSystem::new();
    app.audio_warned.clear();
    #[cfg(feature = "physics")]
    {
        app.physics = app
            .physics_config
            .map(|(gx, gy, lu)| jengine_core::physics::PhysicsWorld::new(gx, gy, lu));
    }

    // `install_scripts` clears script state and runs `init` on the new
    // world. A pop overrides this with its snapshot's scripts.
    app.current_scripts = scripts.clone();
    script_host::install_scripts(&mut app, scripts);
    Ok(())
}

/// Log which Lua implementation will compile game scripts.
///
/// `lua-runtime.js` sets `globalThis.__jengine_lua_runtime = "fengari"` when
/// it loads; its absence means the page did not ship or could not load the
/// bundle, and `JsAdapter::load` will fall back to the Lua->JS transpiler.
fn report_lua_runtime() {
    let installed = web_sys::window()
        .and_then(|w| {
            js_sys::Reflect::get(&w, &JsValue::from_str("__jengine_lua_runtime")).ok()
        })
        .and_then(|v| v.as_string());
    match installed.as_deref() {
        Some(name) => console_log!("Lua runtime: {} (real Lua VM, same as the editor)", name),
        None => console_warn!(
            "Lua runtime: transpiler fallback - lua-runtime.js did not load.              Scripts are translated to JavaScript and may not match the editor.              See docs/web-lua-runtime-proposal.md"
        ),
    }
}

/// Fetch every script the project ships, listed by the web builder in
/// `assets/script_manifest.json`.
///
/// The entity scripts in a scene are fetched separately (`fetch_world_scripts`);
/// this is the whole set, because a `require` can reach any of them.
async fn fetch_project_scripts() -> Vec<(String, String)> {
    #[derive(Deserialize)]
    struct ScriptManifest {
        /// Sources keyed by asset-relative path - one request for the lot.
        #[serde(default)]
        sources: std::collections::HashMap<String, String>,
        /// Path list only, as older builds emitted. Fetched one at a time.
        #[serde(default)]
        scripts: Vec<String>,
    }
    let text = match fetch_text("assets/script_manifest.json").await {
        Ok(t) => t,
        // Builds predating the manifest; `require` stays unavailable rather
        // than failing the boot.
        Err(_) => return Vec::new(),
    };
    let manifest: ScriptManifest = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            console_warn!("parse script_manifest.json: {}", e);
            return Vec::new();
        }
    };
    if !manifest.sources.is_empty() {
        let mut out: Vec<(String, String)> = manifest.sources.into_iter().collect();
        // Deterministic order so logs and `require` cache seeding do not
        // depend on hash iteration order.
        out.sort_by(|a, b| a.0.cmp(&b.0));
        return out;
    }
    // Fallback for a manifest written before sources were inlined: one
    // request per script, which is why the inlined form exists.
    let mut out = Vec::with_capacity(manifest.scripts.len());
    for path in manifest.scripts {
        match fetch_text(&asset_url(&path)).await {
            Ok(source) => out.push((path, source)),
            Err(e) => console_warn!("fetch script module '{}': {:?}", path, e),
        }
    }
    out
}

/// Install `globalThis.require` for the transpiled fallback path.
///
/// Mirrors the candidate spellings the Fengari shim accepts, so a
/// `require("lib/foo")` resolves the same on either path.
fn install_module_resolver(
    scripts: &[(String, String)],
) -> Option<jengine_core::scripting::bridge::adapters::JsAdapter> {
    if scripts.is_empty() {
        return None;
    }
    MODULE_SOURCES.with(|m| m.borrow_mut().extend(scripts.iter().cloned()));
    let resolver: jengine_core::scripting::bridge::ModuleResolver =
        std::rc::Rc::new(move |raw: &str| {
            let dotted = raw.replace('.', "/");
            for stem in [raw, dotted.as_str()] {
                for candidate in [
                    stem.to_string(),
                    format!("{stem}.lua"),
                    format!("{stem}.luau"),
                    format!("scripts/{stem}"),
                    format!("scripts/{stem}.lua"),
                    format!("scripts/{stem}.luau"),
                ] {
                    if let Some(src) = MODULE_SOURCES.with(|m| m.borrow().get(&candidate).cloned()) {
                        return Ok(src);
                    }
                }
            }
            Err(format!("module '{raw}' not found in the project's scripts"))
        });
    let mut adapter = jengine_core::scripting::bridge::adapters::JsAdapter::new();
    adapter.set_module_resolver(resolver);
    Some(adapter)
}

/// Seed the data tables into the script namespace (`get_data`), and for
/// packages, which read them from Rust (`data::global`) - nothing published
/// that copy on the web once, so a package's tables were empty here.
fn seed_data_tables(tables: &serde_json::Map<String, serde_json::Value>) {
    let json = serde_json::to_string(tables).unwrap_or_else(|_| "{}".to_string());
    script_host::seed_data_tables(&json);
    match jengine_core::data::DataRegistry::from_tables_json(&json) {
        Ok(registry) => jengine_core::data::install_global(std::sync::Arc::new(registry)),
        Err(e) => console_warn!("data tables for packages did not parse: {}", e),
    }
}

/// Add scripts to what `require` can reach - the Fengari shim's
/// `prefab_scripts`, under both spellings as at boot, and the transpiler
/// fallback's map.
fn add_require_modules(scripts: &[(String, String)]) {
    let mut map = serde_json::Map::new();
    for (path, source) in scripts {
        map.insert(path.clone(), serde_json::Value::String(source.clone()));
        map.insert(format!("assets/{path}"), serde_json::Value::String(source.clone()));
    }
    script_host::add_prefab_scripts(&serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string()));
    MODULE_SOURCES.with(|m| m.borrow_mut().extend(scripts.iter().cloned()));
}

/// Fetch and decode the clips a scene declares in `audio_refs`.
///
/// Mirrors the native runner's `resolve_audio_refs`, which reads the same
/// top-level array off the scene JSON and seeds the asset cache — scripts
/// call `play_sound("sounds/hit.wav")` with the id as authored.
async fn fetch_scene_audio(
    scene_json: &str,
    audio: &mut jengine_core::audio::web_audio::WebAudioBackend,
) {
    let mut loaded = 0usize;
    for (clip_id, bytes) in fetch_scene_audio_bytes(scene_json).await {
        match audio.load_clip(&clip_id, &bytes) {
            Ok(()) => loaded += 1,
            Err(e) => console_warn!("decode audio '{}': {}", clip_id, e),
        }
    }
    if loaded > 0 {
        console_log!("loaded {} audio clip(s)", loaded);
    }
}

/// Bind behavior graphs for any entity carrying a `BehaviorGraphComponent`
/// that the system hasn't attached yet, using the pre-fetched JSON cache.
///
/// Through the same `sync_instances_inline` the editor uses, so instances of
/// despawned entities are dropped (firing `OnDespawn`) as well as new ones
/// bound - this used to only ever add (BUGS #93).
fn bind_pending_behavior_graphs(app: &mut WebApp) {
    if app.pending_behaviors.is_empty() {
        return;
    }
    for e in app
        .behavior_system
        .sync_instances_inline(&mut app.world, &app.pending_behaviors)
    {
        console_warn!("behavior attach '{}' failed: {}", e.path.display(), e.source);
    }
}

/// Every `.skelanim` and `.rig` in the build, from the manifest the builder
/// writes.
///
/// Native walks `assets/` for them and the editor pushes every one it
/// finds; a browser can do neither, so until Sep 2026 a web build loaded
/// none and every `SkeletalPlayer` held its bind pose. Loaded once at boot
/// and kept across scenes: `play_animation` can name any clip, any time.
async fn load_skeletal_clips(assets: &mut AssetManager) {
    #[derive(Deserialize)]
    struct ClipManifest {
        #[serde(default)]
        clips: std::collections::HashMap<String, serde_json::Value>,
        // Every `.rig`, for `Skeleton.spawn_rig` (Oct 2026).
        #[serde(default)]
        rigs: std::collections::HashMap<String, serde_json::Value>,
    }
    // A build from before the manifest has no clips to find.
    let Ok(text) = fetch_text("assets/skelanim_manifest.json").await else { return };
    let manifest: ClipManifest = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            console_warn!("parse skelanim_manifest.json: {}", e);
            return;
        }
    };
    let mut loaded = 0;
    for (id, clip) in manifest.clips {
        match serde_json::from_value::<jengine_core::animation::skeletal::SkeletalClip>(clip) {
            Ok(c) => {
                assets.insert_skeletal_clip(id, c);
                loaded += 1;
            }
            Err(e) => console_warn!("skeletal clip '{}': {}", id, e),
        }
    }
    if loaded > 0 {
        console_log!("loaded {} skeletal clip(s)", loaded);
    }
    let mut rigs = 0;
    for (id, rig) in manifest.rigs {
        match serde_json::from_value::<jengine_core::animation::rig::SkeletonRig>(rig) {
            Ok(r) => {
                assets.insert_skeleton_rig(id, r);
                rigs += 1;
            }
            Err(e) => console_warn!("skeleton rig '{}': {}", id, e),
        }
    }
    if rigs > 0 {
        console_log!("loaded {} skeleton rig(s)", rigs);
    }
}

/// Fetch every `.behavior` graph a world references.
///
/// Native walks `assets/` from disk in `sync_instances`; the browser can't,
/// so the graphs a scene needs are fetched once at load and bound from this
/// cache. Without it every `behavior_graph` entity just stands still.
async fn fetch_world_behaviors(
    world: &World,
    cache: &mut std::collections::HashMap<String, String>,
) {
    use jengine_core::ecs::components::BehaviorGraphComponent;
    let mut wanted: HashSet<String> = HashSet::new();
    for (_, bg) in world.query::<&BehaviorGraphComponent>().iter() {
        if !bg.graph_path.is_empty() && !cache.contains_key(&bg.graph_path) {
            wanted.insert(bg.graph_path.clone());
        }
    }
    for path in &wanted {
        match fetch_text(&asset_url(path)).await {
            Ok(text) => {
                cache.insert(path.clone(), text);
            }
            Err(e) => console_warn!("fetch behavior '{}': {:?}", path, e),
        }
    }
    if !wanted.is_empty() {
        console_log!("loaded {} behavior graph(s)", wanted.len());
    }
}

/// Fetch every `.anim` a scene references and inline its clips.
///
/// The disk-based twin is `animation::asset::resolve_anim_sources`; both
/// funnel through the same `collect_clip_sources` / `inline_anim_sources`
/// pair in jengine-core, so the browser and the native runtime rewrite the
/// scene identically. Returns the scene JSON unchanged if it parses badly
/// or references nothing.
async fn inline_anim_sources_web(scene_json: &str) -> String {
    use jengine_core::animation::asset::{
        collect_clip_sources, inline_anim_sources, parse_anim_asset, AnimAssetFile,
    };

    let mut value: serde_json::Value = match serde_json::from_str(scene_json) {
        Ok(v) => v,
        Err(_) => return scene_json.to_string(),
    };
    let sources = collect_clip_sources(&value);
    if sources.is_empty() {
        return scene_json.to_string();
    }

    let mut loaded: std::collections::HashMap<String, AnimAssetFile> =
        std::collections::HashMap::new();
    for src in sources {
        match fetch_bytes(&asset_url(&src)).await {
            Ok(bytes) => match parse_anim_asset(&bytes, &src) {
                Ok(asset) => {
                    loaded.insert(src, asset);
                }
                Err(e) => console_warn!("parse .anim '{}': {}", src, e),
            },
            Err(e) => console_warn!("fetch .anim '{}': {:?}", src, e),
        }
    }
    let count = loaded.len();
    for err in inline_anim_sources(&mut value, &loaded) {
        console_warn!("inline .anim: {}", err);
    }
    if count > 0 {
        console_log!("inlined {} .anim clip source(s)", count);
    }
    serde_json::to_string(&value).unwrap_or_else(|_| scene_json.to_string())
}

/// Texture ids a prefab can draw with.
///
/// Entities spawned by `spawn_prefab` never appear in the scene file, so
/// `wanted_texture_ids` (which walks the loaded world) cannot see them and
/// their textures are never fetched — the enemy spawns and renders nothing
/// while the console repeats "dropping draws for missing texture". The
/// editor pre-loads these before play; a web build has to do the same.
///
/// Covers the sprite's own texture and any per-frame override in a
/// `sprite_animator` clip, since an animation can swap sheets mid-clip.
/// The textures a prefab will want once something spawns one.
///
/// This used to be the web runtime's own copy, and the native runtime had
/// none at all - so a prefab-only sheet worked on the web and drew nothing
/// on native (BUGS #137). Both call the same collector now, which also
/// covers particle emitters, lightmaps and a prefab's own tilemap.
fn prefab_texture_ids(
    prefabs: &std::collections::HashMap<String, jengine_core::scene::format::EntityData>,
) -> HashSet<String> {
    jengine_core::scene::format::prefab_texture_ids(prefabs.values())
        .into_iter()
        .collect()
}

/// Behavior-graph paths a prefab can bind.
///
/// Same blind spot as `prefab_texture_ids`: `fetch_world_behaviors` walks the
/// loaded world, so a graph that only ever arrives via `spawn_prefab` is
/// never fetched and the spawned entity stands still.
fn prefab_behavior_paths(
    prefabs: &std::collections::HashMap<String, jengine_core::scene::format::EntityData>,
) -> HashSet<String> {
    let mut out = HashSet::new();
    for entity in prefabs.values() {
        let Ok(value) = serde_json::to_value(&entity.components) else {
            continue;
        };
        if let Some(path) = value
            .get("behavior_graph")
            .and_then(|b| b.get("graph_path"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            out.insert(path.to_string());
        }
    }
    out
}

/// Texture ids a loaded world draws with.
fn wanted_texture_ids(world: &World) -> HashSet<String> {
    // The one list, shared with native. This used to be the web's own copy,
    // and it had drifted: no particle, animation-frame, tileset or `UIImage`
    // textures.
    jengine_core::scene::format::world_texture_ids(world).into_iter().collect()
}

/// GUI document ids referenced by a world.
fn wanted_gui_ids(world: &World) -> HashSet<String> {
    let mut wanted: HashSet<String> = HashSet::new();
    for (_, gc) in world.query::<&GuiComponent>().iter() {
        if !gc.gui_id.is_empty() {
            wanted.insert(gc.gui_id.clone());
        }
    }
    wanted
}

/// Fetch + decode textures by id, returning what arrived.
///
/// Returns rather than inserting so scene transitions can do the fetching
/// without holding a borrow on the app across an `await`.
async fn fetch_textures(ids: Vec<String>) -> Vec<(String, RawTextureData)> {
    let mut out = Vec::new();
    for tex_id in ids {
        // Texture ids are relative to `assets/` (e.g. "sprites/foo.png").
        match fetch_bytes(&asset_url(&tex_id)).await {
            Ok(bytes) => match RawTextureData::from_bytes(&bytes) {
                Ok(raw) => {
                    console_log!(
                        "cached texture '{}' ({}x{}, {} bytes)",
                        tex_id,
                        raw.width,
                        raw.height,
                        bytes.len()
                    );
                    out.push((tex_id, raw));
                }
                Err(e) => console_warn!("decode failed for '{}': {}", tex_id, e),
            },
            Err(e) => console_warn!("fetch failed for '{}': {:?}", tex_id, e),
        }
    }
    out
}

/// Fetch + parse GUI documents by id.
async fn fetch_guis(ids: Vec<String>) -> Vec<(String, GuiDocument)> {
    let mut out = Vec::new();
    for gui_id in ids {
        match fetch_text(&asset_url(&gui_id)).await {
            Ok(text) => match load_gui(&text) {
                Ok(doc) => {
                    console_log!("loaded gui '{}'", gui_id);
                    out.push((gui_id, doc));
                }
                Err(e) => console_warn!("parse gui '{}': {}", gui_id, e),
            },
            Err(e) => console_warn!("fetch gui '{}': {:?}", gui_id, e),
        }
    }
    out
}

async fn fetch_world_textures(world: &World, assets: &mut AssetManager) {
    let ids: Vec<String> = wanted_texture_ids(world)
        .into_iter()
        .filter(|id| assets.get_texture_bytes(id).is_none())
        .collect();
    for (id, raw) in fetch_textures(ids).await {
        assets.insert_texture_bytes(id, raw);
    }
}

/// Fetch each `.font` descriptor together with its sibling atlas image.
///
/// Both or neither. Registering a descriptor whose atlas never arrived would
/// draw every glyph from whatever texture happened to be bound - the same
/// all-or-nothing rule the native loader applies in `font_asset::load_fonts`.
/// A fetched font: a `.font` with its atlas, or a TTF / OTF's bytes
/// (`renderer::dynamic_font`, whose atlas is made at runtime).
enum FetchedFont {
    Bitmap(String, String, RawTextureData),
    File(String, Vec<u8>),
}

async fn fetch_fonts(ids: Vec<String>) -> Vec<FetchedFont> {
    let mut out = Vec::new();
    for font_id in ids {
        if jengine_core::renderer::dynamic_font::is_font_file(&font_id) {
            match fetch_bytes(&asset_url(&font_id)).await {
                Ok(bytes) => out.push(FetchedFont::File(font_id, bytes)),
                Err(e) => console_warn!("fetch font '{}': {:?}", font_id, e),
            }
            continue;
        }
        let texture_id = jengine_core::renderer::font_asset::font_texture_id(&font_id);
        let json = match fetch_text(&asset_url(&font_id)).await {
            Ok(t) => t,
            Err(e) => {
                console_warn!("fetch font '{}': {:?}", font_id, e);
                continue;
            }
        };
        match fetch_bytes(&asset_url(&texture_id)).await {
            Ok(bytes) => match RawTextureData::from_bytes(&bytes) {
                Ok(raw) => {
                    console_log!("loaded font '{}' (atlas '{}')", font_id, texture_id);
                    out.push(FetchedFont::Bitmap(font_id, json, raw));
                }
                Err(e) => console_warn!("decode font atlas '{}': {}", texture_id, e),
            },
            Err(e) => console_warn!("fetch font atlas '{}': {:?}", texture_id, e),
        }
    }
    out
}

/// Register fetched fonts, descriptor first so a parse failure leaves no
/// orphan atlas in the texture cache.
fn insert_fonts(assets: &mut AssetManager, fonts: Vec<FetchedFont>) {
    use jengine_core::renderer::font_asset;
    for font in fonts {
        let (font_id, json, raw) = match font {
            FetchedFont::Bitmap(id, json, raw) => (id, json, raw),
            FetchedFont::File(id, bytes) => {
                if let Err(e) = jengine_core::renderer::dynamic_font::insert_font_file(assets, &id, &bytes) {
                    console_warn!("font '{}': {}", id, e);
                }
                continue;
            }
        };
        match font_asset::insert_font_json(assets, &font_id, &json) {
            Ok(()) => {
                assets.insert_texture_bytes(font_asset::font_texture_id(&font_id), raw);
            }
            Err(e) => console_warn!("{:#}", e),
        }
    }
}

/// Which fonts this world needs that the cache does not already hold.
fn missing_font_ids(world: &World, assets: &AssetManager) -> Vec<String> {
    use jengine_core::renderer::font_asset;
    let mut wanted = font_asset::collect_font_ids(world);
    // `.gui` widgets name fonts of their own; the documents are fetched
    // before this runs, so they can be scanned from the cache.
    for (_, gc) in world.query::<&GuiComponent>().iter() {
        if let Some(doc) = assets.get_gui(&gc.gui_id) {
            font_asset::collect_gui_font_ids(doc, &mut wanted);
        }
    }
    wanted
        .into_iter()
        .filter(|id| !assets.has_font_data(id))
        .collect()
}

async fn fetch_world_fonts(world: &World, assets: &mut AssetManager) {
    let ids = missing_font_ids(world, assets);
    let fonts = fetch_fonts(ids).await;
    insert_fonts(assets, fonts);
}

async fn fetch_world_guis(world: &World, assets: &mut AssetManager) {
    let ids: Vec<String> = wanted_gui_ids(world)
        .into_iter()
        .filter(|id| assets.get_gui(id).is_none())
        .collect();
    let guis = fetch_guis(ids).await;
    for (id, raw) in fetch_textures(gui_texture_ids(&guis, assets)).await {
        assets.insert_texture_bytes(id, raw);
    }
    for (id, doc) in guis {
        assets.insert_gui(id, doc);
    }
}

/// The textures `.gui` documents name - their `Image` nodes and rich-text
/// icons - that are not loaded yet. Nothing else names them: the texture
/// preload walks the world and the prefabs, and a document's images are in
/// neither. So a web build fetched none of them and drew every `.gui` image
/// as nothing, with one "dropping UI draws for missing texture" warning (the
/// showcase's Menus & HUD lost its icon sheet). Native preloads them the same
/// way (`GuiNode::texture_ids`).
fn gui_texture_ids(guis: &[(String, GuiDocument)], assets: &AssetManager) -> Vec<String> {
    let mut ids = std::collections::BTreeSet::new();
    for (_, doc) in guis {
        doc.root.texture_ids(&mut ids);
    }
    // `__` ids are written at runtime (a camera's target), never fetched.
    ids.into_iter().filter(|id| assets.get_texture_bytes(id).is_none() && !id.starts_with("__")).collect()
}

/// Fetch the script sources every `ScriptComponent` in a world points at,
/// de-duplicated so a file shared by ten entities is fetched once.
async fn fetch_world_scripts(world: &World) -> Vec<script_host::LoadedScript> {
    let requests = script_host::collect_script_requests(world);
    let mut sources: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut out = Vec::new();
    for req in requests {
        let source = if let Some(cached) = sources.get(&req.source_path) {
            cached.clone()
        } else {
            match fetch_text(&asset_url(&req.source_path)).await {
                Ok(text) => {
                    sources.insert(req.source_path.clone(), text.clone());
                    text
                }
                Err(e) => {
                    console_warn!("fetch script {}: {:?}", req.source_path, e);
                    continue;
                }
            }
        };
        out.push(script_host::LoadedScript {
            entity_id: req.entity_id,
            source_path: req.source_path,
            language: req.language,
            source,
        });
    }
    out
}

/// Fetch a scene's `.sortlayers` asset, if it declares one.
async fn fetch_scene_sort_layers(scene_json: &str) -> Option<(String, String)> {
    let meta = jengine_core::scene::format::read_scene_metadata(scene_json).ok()?;
    let rel = meta.sort_layers?;
    match fetch_text(&asset_url(&rel)).await {
        Ok(text) => Some((rel, text)),
        Err(e) => {
            console_warn!("sort_layers fetch failed: {:?}", e);
            None
        }
    }
}

/// Fetch every atlas a scene lists in `atlas_refs`: its `.atlas` metadata and
/// the sibling image, both or neither.
///
/// Native and the editor both resolved `atlas_refs`; the web runtime ignored
/// the field, so an atlas image still loaded (sprites name it) but no regions
/// were registered - `set_sprite_region` did nothing and animation frames
/// naming a region never resolved.
async fn fetch_scene_atlases(scene_json: &str) -> Vec<(String, AtlasData, RawTextureData)> {
    let mut out = Vec::new();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(scene_json) else {
        return out;
    };
    for rel in jengine_core::renderer::atlas::scene_atlas_refs(&value) {
        let atlas_id = jengine_core::renderer::atlas::derive_atlas_id(&rel);
        let data = match fetch_text(&asset_url(&rel)).await {
            Ok(text) => match serde_json::from_str::<AtlasData>(&text) {
                Ok(d) => d,
                Err(e) => {
                    console_warn!("parse atlas '{}': {}", rel, e);
                    continue;
                }
            },
            Err(e) => {
                console_warn!("fetch atlas '{}': {:?}", rel, e);
                continue;
            }
        };
        match fetch_bytes(&asset_url(&atlas_id)).await {
            Ok(bytes) => match RawTextureData::from_bytes(&bytes) {
                Ok(raw) => out.push((atlas_id, data, raw)),
                Err(e) => console_warn!("decode atlas image '{}': {}", atlas_id, e),
            },
            Err(e) => console_warn!("fetch atlas image '{}': {:?}", atlas_id, e),
        }
    }
    out
}

fn insert_atlases(assets: &mut AssetManager, atlases: Vec<(String, AtlasData, RawTextureData)>) {
    for (id, data, raw) in atlases {
        assets.insert_texture_bytes(id.clone(), raw);
        assets.register_atlas(id, data);
    }
}

/// Fetch the raw bytes of every clip in a scene's `audio_refs`.
async fn fetch_scene_audio_bytes(scene_json: &str) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(scene_json) else {
        return out;
    };
    let Some(refs) = value.get("audio_refs").and_then(|v| v.as_array()) else {
        return out;
    };
    for entry in refs {
        let Some(clip_id) = entry.as_str() else { continue };
        if clip_id.is_empty() {
            continue;
        }
        match fetch_bytes(&asset_url(clip_id)).await {
            Ok(bytes) => out.push((clip_id.to_string(), bytes)),
            Err(e) => console_warn!("fetch audio '{}': {:?}", clip_id, e),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Input listeners
// ---------------------------------------------------------------------------

fn install_input_listeners(
    canvas: &HtmlCanvasElement,
    app: Rc<RefCell<WebApp>>,
) -> Result<(), JsValue> {
    let document = window()
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;

    // Keyboard — listen on document so canvas doesn't need focus.
    {
        let app_kd = app.clone();
        let kd = Closure::<dyn FnMut(_)>::new(move |e: web_sys::KeyboardEvent| {
            // The hidden textarea a text field types through: its keys are
            // the player's, its characters arrive as its input events, and
            // it needs its keys' default action to receive them.
            let proxy = jengine_core::input::text_entry::web::TextProxy::is_proxy_event(&e);
            // A page embedding the game may have its own form; what is typed
            // there is not the player's.
            if !proxy && key_goes_to_page_field(&e) {
                return;
            }
            // The page is the game: F3 must not also open find, nor Space
            // scroll the page, nor Tab walk focus off the canvas.
            let mods = BrowserKeyMods {
                ctrl: e.ctrl_key(),
                shift: e.shift_key(),
                alt: e.alt_key(),
                meta: e.meta_key(),
            };
            if (!proxy || e.code() == "Tab") && !browser_keeps_default(&e.code(), mods, app_kd.borrow().debug_overlay_enabled) {
                e.prevent_default();
            }
            // Keyboard counts as activation too - a player who only ever
            // presses keys would otherwise get a permanently silent game.
            app_kd.borrow_mut().web_audio.resume_if_suspended();
            // Text entry: `code` is the physical key, `key` is what the layout
            // produced. A printable keystroke reports a single-character
            // `key` ("a", "A", "ä"); anything else is a name like "Shift".
            {
                let key = e.key();
                if !proxy && key.chars().count() == 1 && !e.ctrl_key() && !e.meta_key() {
                    app_kd.borrow_mut().input.type_str(&key);
                }
            }
            if let Some(code) = map_browser_key(&e.code()) {
                let mut a = app_kd.borrow_mut();
                a.input.key_down(code);
                // Feature 3.3 — F3 / F12 toggle the runtime debug overlay.
                // `KeyboardEvent.repeat` filters held-key auto-repeat.
                if !e.repeat() {
                    match code {
                        // Inert when the build disabled the overlays.
                        KeyCode::F3 if a.debug_overlay_enabled => {
                            a.debug_overlay.toggle_stats()
                        }
                        KeyCode::F12 if a.debug_overlay_enabled => {
                            a.debug_overlay.toggle_inspector()
                        }
                        // F4: collider and joint outlines.
                        KeyCode::F4 if a.debug_overlay_enabled => {
                            a.debug_overlay.toggle_physics()
                        }
                        _ => {}
                    }
                }
            }
        });
        document.add_event_listener_with_callback("keydown", kd.as_ref().unchecked_ref())?;
        kd.forget();
    }
    {
        let app_ku = app.clone();
        let ku = Closure::<dyn FnMut(_)>::new(move |e: web_sys::KeyboardEvent| {
            if let Some(code) = map_browser_key(&e.code()) {
                app_ku.borrow_mut().input.key_up(code);
            }
        });
        document.add_event_listener_with_callback("keyup", ku.as_ref().unchecked_ref())?;
        ku.forget();
    }

    // Mouse — listen on canvas.
    {
        let app_mm = app.clone();
        let canvas_mm = canvas.clone();
        let mm = Closure::<dyn FnMut(_)>::new(move |e: web_sys::MouseEvent| {
            let mut a = app_mm.borrow_mut();
            a.input.mouse_position = canvas_mouse_position(&canvas_mm, &e);
            // `get_mouse_delta`: the event's own motion, which keeps coming
            // while the pointer is locked (`Cursor.set_locked`), in the same
            // canvas pixels as the position.
            let rect = canvas_mm.get_bounding_client_rect();
            let (sx, sy) = if rect.width() > 0.0 && rect.height() > 0.0 {
                (canvas_mm.width() as f64 / rect.width(), canvas_mm.height() as f64 / rect.height())
            } else {
                (1.0, 1.0)
            };
            a.input.mouse_motion((e.movement_x() as f64 * sx) as f32, (e.movement_y() as f64 * sy) as f32);
        });
        canvas.add_event_listener_with_callback("mousemove", mm.as_ref().unchecked_ref())?;
        mm.forget();
    }
    {
        let app_md = app.clone();
        let canvas_md = canvas.clone();
        let md = Closure::<dyn FnMut(_)>::new(move |e: web_sys::MouseEvent| {
            let mut a = app_md.borrow_mut();
            // A click is also the moment the browser will let a suspended
            // AudioContext start.
            a.web_audio.resume_if_suspended();
            // Take the position from the press too: a click that arrives
            // without a preceding move (touch, or the pointer entering over
            // the button) would otherwise be tested against a stale point.
            a.input.mouse_position = canvas_mouse_position(&canvas_md, &e);
            match e.button() {
                0 => a.input.set_mouse_button(true, true),
                1 => {
                    // The browser would start its autoscroll.
                    e.prevent_default();
                    a.input.set_middle_mouse_button(true);
                }
                2 => a.input.set_mouse_button(false, true),
                _ => {}
            }
        });
        canvas.add_event_listener_with_callback("mousedown", md.as_ref().unchecked_ref())?;
        md.forget();
    }
    {
        let app_mu = app.clone();
        let mu = Closure::<dyn FnMut(_)>::new(move |e: web_sys::MouseEvent| {
            let mut a = app_mu.borrow_mut();
            match e.button() {
                0 => a.input.set_mouse_button(true, false),
                1 => a.input.set_middle_mouse_button(false),
                2 => a.input.set_mouse_button(false, false),
                _ => {}
            }
        });
        // On the window, not the canvas: a press that drags off the canvas
        // releases outside it, and a canvas listener never heard that - the
        // button stayed held.
        window().add_event_listener_with_callback("mouseup", mu.as_ref().unchecked_ref())?;
        mu.forget();
    }
    {
        // A blurred page gets no key-up or mouse-up events, so anything held
        // through alt-tab or a tab switch would stay pressed until pressed
        // again. `blur` covers the window losing focus; `visibilitychange`
        // covers a hidden tab, which does not always blur first.
        let app_blur = app.clone();
        let blur = Closure::<dyn FnMut()>::new(move || {
            if let Ok(mut a) = app_blur.try_borrow_mut() {
                a.input.release_all();
            }
        });
        window().add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref())?;
        blur.forget();

        let app_vis = app.clone();
        let doc_vis = document.clone();
        let vis = Closure::<dyn FnMut()>::new(move || {
            if doc_vis.hidden()
                && let Ok(mut a) = app_vis.try_borrow_mut()
            {
                a.input.release_all();
            }
        });
        document.add_event_listener_with_callback("visibilitychange", vis.as_ref().unchecked_ref())?;
        vis.forget();
    }
    {
        // Right-click is a game input (the `mousedown` arm above feeds button
        // 2 to `InputState`), so the browser's own menu on top of it is just
        // in the way. Suppressed on the canvas only - a page embedding the
        // game keeps its normal menu everywhere else. Mirrors what the editor
        // does in `main.ts`.
        let cm = Closure::<dyn FnMut(_)>::new(move |e: web_sys::MouseEvent| {
            e.prevent_default();
        });
        canvas.add_event_listener_with_callback("contextmenu", cm.as_ref().unchecked_ref())?;
        cm.forget();
    }
    {
        let app_wh = app.clone();
        let wh = Closure::<dyn FnMut(_)>::new(move |e: web_sys::WheelEvent| {
            app_wh.borrow_mut().input.mouse_wheel(-e.delta_y() as f32 / 120.0);
            e.prevent_default();
        });
        canvas.add_event_listener_with_callback("wheel", wh.as_ref().unchecked_ref())?;
        wh.forget();
    }
    // Touch — every finger is queued, by its identifier, for the next
    // frame's `touch.tick`: a finger on an on-screen control presses its
    // keys, and the first free one is the left mouse button, so every
    // click-driven UI works on a phone without the game knowing touch exists.
    // The same shared code as the native runtime (`input::touch_controls`).
    for (event, phase) in [
        ("touchstart", jengine_core::input::touch_controls::TouchPhase::Started),
        ("touchmove", jengine_core::input::touch_controls::TouchPhase::Moved),
        ("touchend", jengine_core::input::touch_controls::TouchPhase::Ended),
        // `touchcancel` fires when the system takes the gesture away (a call
        // arrives, the notification shade opens). Treating it as a release is
        // what stops a button sticking down forever.
        ("touchcancel", jengine_core::input::touch_controls::TouchPhase::Ended),
    ] {
        let app_t = app.clone();
        let canvas_t = canvas.clone();
        let handler = Closure::<dyn FnMut(_)>::new(move |e: web_sys::TouchEvent| {
            // Without this the browser scrolls, zooms or fires a synthetic
            // click ~300ms later, and the game gets the input twice.
            e.prevent_default();
            let mut a = app_t.borrow_mut();
            if phase == jengine_core::input::touch_controls::TouchPhase::Started {
                // A touch is also the gesture that lets a suspended
                // AudioContext start — on mobile it is usually the only one.
                a.web_audio.resume_if_suspended();
            }
            let touches = e.changed_touches();
            for i in 0..touches.length() {
                let Some(touch) = touches.get(i) else { continue };
                let (x, y) = canvas_touch_position(&canvas_t, &touch);
                // Identifiers are small non-negative integers per touch.
                a.touch.push(touch.identifier() as u32 as u64, phase, x, y);
            }
        });
        canvas.add_event_listener_with_callback(event, handler.as_ref().unchecked_ref())?;
        handler.forget();
    }


    Ok(())
}

/// Map a browser `KeyboardEvent.code` string (e.g. `"ArrowLeft"`, `"KeyW"`,
/// `"Space"`) to a [`KeyCode`]. Coverage is the same set the native runner
/// supports via winit. Unrecognized codes return `None` and are dropped.
/// Mouse position in the canvas's drawing-buffer space.
///
/// The page stretches the canvas with `width: 100%; height: 100%` while its
/// buffer stays at the project's resolution, so CSS pixels and the
/// coordinates everything else works in (GUI layout, `get_mouse_position`,
/// `screen_size`) are two different spaces. Without this scaling, clicks
/// land wherever the window happens to be sized to — the graphics look
/// right and nothing you press responds.
/// First touch's position in canvas pixels, scaled the same way
/// [`canvas_mouse_position`] scales a mouse event — the canvas is CSS-stretched
/// to fit the window while its buffer stays at the project resolution, and
/// mixing the two spaces is what put hit-boxes in the wrong place before.
fn canvas_touch_position(canvas: &HtmlCanvasElement, touch: &web_sys::Touch) -> (f32, f32) {
    let rect = canvas.get_bounding_client_rect();
    let (w, h) = (rect.width(), rect.height());
    if w <= 0.0 || h <= 0.0 {
        return (touch.client_x() as f32, touch.client_y() as f32);
    }
    let x = (touch.client_x() as f64 - rect.left()) * canvas.width() as f64 / w;
    let y = (touch.client_y() as f64 - rect.top()) * canvas.height() as f64 / h;
    (x as f32, y as f32)
}

fn canvas_mouse_position(canvas: &HtmlCanvasElement, e: &web_sys::MouseEvent) -> (f32, f32) {
    let rect = canvas.get_bounding_client_rect();
    let (w, h) = (rect.width(), rect.height());
    if w <= 0.0 || h <= 0.0 {
        return (e.offset_x() as f32, e.offset_y() as f32);
    }
    let x = (e.client_x() as f64 - rect.left()) * canvas.width() as f64 / w;
    let y = (e.client_y() as f64 - rect.top()) * canvas.height() as f64 / h;
    (x as f32, y as f32)
}

/// A browser `KeyboardEvent.code` as a [`KeyCode`].
///
/// One line, because this used to be a hand-written `match` here *and* an
/// almost identical one in the other browser runtime - both listing the same
/// partial set and both silently dropping everything else, Backspace and
/// PageUp/PageDown among them. `from_browser_code` derives the whole enum
/// from the key names instead, so it cannot fall behind.
/// The key is being typed into a text field on the host page.
fn key_goes_to_page_field(e: &web_sys::KeyboardEvent) -> bool {
    let Some(el) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else {
        return false;
    };
    matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
        || el.closest("[contenteditable]:not([contenteditable='false'])").ok().flatten().is_some()
}

fn map_browser_key(code: &str) -> Option<KeyCode> {
    KeyCode::from_browser_code(code)
}

// ---------------------------------------------------------------------------
// Per-frame tick + render
// ---------------------------------------------------------------------------

/// Advance ECS behaviors + physics for one frame.
///
/// Returns the contact events produced by the physics step this frame as
/// `(entity_a_id, entity_b_id, started)` tuples so `run_scripts_frame` can
/// dispatch `on_collision_*` callbacks (feature 3.7). The vector is empty
/// when physics is disabled or the `physics` feature is off.
fn tick_world(app: &mut WebApp, dt: f32) -> Vec<(u32, u32, bool, f32, f32, f32)> {
    use jengine_core::ecs::behaviors;
    use jengine_core::ecs::systems as sys;

    behaviors::update_input_actions(&mut app.world, &app.input);

    // Bind graphs for entities that appeared since the last frame
    // (script `spawn_prefab`, graph `SpawnEntity`). Already-bound entities
    // are skipped, so steady state costs one query. Mirrors the viewport.
    bind_pending_behavior_graphs(app);

    // Behavior graphs tick before motion so reactions land on this frame's
    // transform — the ordering the native runner uses.
    // The stepped physics pose back in every body's transform before
    // anything this frame reads one (the last frame drew the blend).
    if let Some(physics) = app.physics.as_mut() {
        physics.restore_stepped_poses(&mut app.world);
    }
    jengine_core::profiling::trace::time("behavior_graphs", || {
        app.behavior_system.tick_pre_simulation(&mut app.world, dt)
    });

    // The motion step every host shares. Batch scripts do not run here, so
    // an entity carrying one moves by its components.
    behaviors::tick_motion(&mut app.world, dt, app.path_library.as_ref(), false);
    // `AudioSource { auto_play: true }` — a sound carried by the entity
    // rather than played by a script.
    app.web_audio.start_autoplay(&mut app.world, &mut app.mixer, &mut app.spatial_audio, &mut |m| console_warn!("{m}"));
    // Positional audio: each spatial source at the volume and pan of where it
    // is now, and the sounds of sources that have gone, stopped.
    app.web_audio.apply_spatial(&app.world, &mut app.mixer, &mut app.spatial_audio);

    // ── package systems: PreUpdate + ScriptExec ───────────────────────────
    // The editor viewport's placement: this runtime's frame has the
    // viewport's shape (world tick, then scripts), so a package behaves here
    // as it does in the editor.
    use jengine_core::package::host::FrameResources;
    use jengine_core::package::system_schedule::SystemPhase;
    app.package_host.run_phase_with(
        SystemPhase::PreUpdate,
        &mut app.world,
        dt,
        FrameResources { input: &mut app.input, assets: &mut app.assets, camera: app.camera },
    );
    app.package_host.run_phase_with(
        SystemPhase::ScriptExec,
        &mut app.world,
        dt,
        FrameResources { input: &mut app.input, assets: &mut app.assets, camera: app.camera },
    );

    // Physics step (feature 3.7). Matches the editor viewport ordering:
    // run AFTER movement behaviors so kinematic bodies follow `Velocity2D`,
    // and BEFORE `update_trigger_areas` so trigger overlaps see the moved
    // transforms. Dynamic bodies fall under gravity; the returned contacts
    // drive `on_collision_*` script callbacks.
    #[cfg_attr(not(feature = "physics"), allow(unused_mut))]
    let mut pending_contacts: Vec<(u32, u32, bool, f32, f32, f32)> = Vec::new();
    #[cfg(feature = "physics")]
    let fixed_fns = script_host::fixed_update_fns(app);
    #[cfg(feature = "physics")]
    if app.physics.is_some() {
        let physics_start = performance().now();
        let fixed_steps = {
            let physics = app.physics.as_mut().expect("checked above");
            sys::physics_frame_begin(&mut app.world, physics, dt);
            if fixed_fns.is_empty() { None } else { Some((physics.begin_frame(dt), physics.fixed_dt())) }
        };
        let contacts = match fixed_steps {
            None => app.physics.as_mut().map(|p| p.step(dt)).unwrap_or_default(),
            // `fixed_update` scripts: one call before each fixed step, each
            // reading the world as the step before it left it - as native.
            Some((steps, fixed_dt)) => {
                for _ in 0..steps {
                    script_host::run_fixed_update(app, &fixed_fns, fixed_dt);
                    let Some(physics) = app.physics.as_mut() else { break };
                    sys::physics_fixed_step(&mut app.world, physics, fixed_dt);
                }
                app.physics.as_mut().map(|p| p.end_frame()).unwrap_or_default()
            }
        };
        // A script's scene load can replace the physics world mid-frame.
        if let Some(physics) = app.physics.as_mut() {
        sys::physics_frame_end(&mut app.world, physics, &contacts, &mut app.particle_system, dt);
        let _ = app
            .behavior_system
            .dispatch_contact_events(&contacts, &mut app.world);
        for ev in &contacts {
            let started =
                matches!(ev.event_type, jengine_core::physics::ContactType::Started);
            pending_contacts.push((ev.entity_a as u32, ev.entity_b as u32, started, ev.impulse, ev.point.0, ev.point.1));
        }
        }
        app.frame_stats.physics_time_ms = (performance().now() - physics_start) as f32;
    }

    behaviors::update_trigger_areas(&mut app.world);

    // Trigger-overlap dispatch + structural-command flush — the Phase 7
    // barrier from the native runner. Behavior-graph spawns/despawns
    // materialize here, before the next frame's tick sees them.
    // Graph input triggers (`OnKey`, `OnMouseClick`) - the same core call in
    // every runtime; nothing fired them before (BUGS #93).
    app.behavior_system.dispatch_input(&app.input, &mut app.world);
    let _ = app
        .behavior_system
        .dispatch_trigger_overlaps_from_world(&mut app.world);
    let routed = app.behavior_system.flush_commands_and_route_events(
        &mut app.world,
        Some(&jengine_core::behavior::PrefabSource {
            registry: &app.registry,
            prefabs: &app.prefab_map,
        }),
    );
    script_host::dispatch_graph_events(app, &routed.emit_event_requests);
    // Sound and scene changes need the host: the same paths a script's
    // `play_sound` (`"ps"`) and `load_scene` take (BUGS #93).
    for r in &routed.play_sound_requests {
        if let Err(e) = app.web_audio.play_sound_on_bus(
            &mut app.mixer,
            jengine_core::audio::SFX_BUS,
            &r.sound_id,
            r.volume,
            1.0,
            0.0,
            false,
        ) && app.audio_warned.insert(r.sound_id.clone())
        {
            web_sys::console::warn_1(
                &format!(
                    "[Audio] behavior PlaySound('{}') - {e} (further occurrences suppressed)",
                    r.sound_id
                )
                .into(),
            );
        }
    }
    if let Some(path) = routed.change_scene_requests.last() {
        script_host::post_scene_load(path);
    }

    // ── package systems: Physics + PostPhysics ────────────────────────────
    // After this runtime's own step, so a package sees this frame's results.
    // The built-in physics package registers in `Physics` and does nothing
    // here: rapier is stepped above and `PhysicsWorld` is not shared as a
    // resource.
    app.package_host.run_phase_with(
        SystemPhase::Physics,
        &mut app.world,
        dt,
        FrameResources { input: &mut app.input, assets: &mut app.assets, camera: app.camera },
    );
    app.package_host.run_phase_with(
        SystemPhase::PostPhysics,
        &mut app.world,
        dt,
        FrameResources { input: &mut app.input, assets: &mut app.assets, camera: app.camera },
    );

    // Combat — trigger-hitbox damage, i-frames, death, health mirror.
    let sprite_events = sys::tick_combat_and_animation(&mut app.world, &app.assets, dt);
    // What a clip's frames fired: a sound plays here, an event goes to the
    // script on that entity as `on_animation_event`. Every runtime computed
    // these and threw them away until Sep 2026.
    if !sprite_events.is_empty() {
        let (sounds, events) = jengine_core::animation::split_triggers(&sprite_events);
        for (_, clip, volume) in sounds {
            let _ = app.web_audio.play_sound_on_bus(
                &mut app.mixer,
                jengine_core::audio::SFX_BUS,
                &clip,
                volume,
                1.0,
                0.0,
                false,
            );
        }
        script_host::dispatch_animation_events(app, &events);
    }
    // Typed characters land in the focused GUI TextInput.
    sys::apply_text_input(&mut app.world, &mut app.assets, &mut app.input);
    // Menus without a pointer: arrows / D-pad / stick move focus, A / Enter
    // press it, Tab walks the document. A press goes where a click goes.
    let nav = sys::navigate_ui(&mut app.world, &mut app.assets, &mut app.input, &app.ui_scale, dt);
    for (gui_id, node_id) in &nav.gui_activations {
        script_host::dispatch_gui_click(app, gui_id, node_id);
    }
    script_host::dispatch_ui_clicks(app, &nav.ui_clicks);

    let expired = behaviors::update_lifetimes(&mut app.world, dt);
    for e in expired {
        let _ = app.world.despawn(e);
    }
    // Anything carrying `DespawnOffscreen` that has left the view.
    {
        let view = (app.camera.position, app.camera.half_extents());
        let travel = app.camera.position - app.prev_camera;
        let gone = behaviors::despawn_offscreen(&mut app.world, view, travel);
        for e in gone {
            let _ = app.world.despawn(e);
        }
        // Videos (`jengine_core::video`): this frame's pictures into their
        // textures, before anything draws them.
        app.video.tick(&mut app.world, &mut app.assets, dt);
        // `visibility_notifier`s that came on screen or left it.
        jengine_core::ecs::visibility::tick_visibility(&mut app.world, view);
        // What the engine owes scripts (`ecs::callbacks`): those, a
        // `path_follow` at its end ...
        script_host::dispatch_entity_callbacks(app);
        app.prev_camera = app.camera.position;
    }

    pending_contacts
}

/// The browser's `env(safe-area-inset-*)` - where a notch or rounded corner
/// covers the page (the HTML asks for `viewport-fit=cover`, so the canvas
/// reaches under it) - in canvas pixels.
///
/// CSS has no API for these values, only the `env()` function, so a hidden
/// probe element is padded with them and its computed padding read back. The
/// canvas is CSS-stretched over the page while its buffer keeps the project's
/// size, hence the scale on each axis. A browser without a notch, or one that
/// knows nothing of `env()`, reads zero.
fn read_safe_insets(canvas: &HtmlCanvasElement) -> jengine_core::ui::scaling::SafeInsets {
    use jengine_core::ui::scaling::SafeInsets;
    let doc = window().document();
    let Some(doc) = doc else { return SafeInsets::ZERO };
    let probe = match doc.get_element_by_id("jengine-safe-area-probe") {
        Some(el) => el,
        None => {
            let Ok(el) = doc.create_element("div") else { return SafeInsets::ZERO };
            el.set_id("jengine-safe-area-probe");
            let _ = el.set_attribute(
                "style",
                "position:fixed;left:0;top:0;width:0;height:0;visibility:hidden;pointer-events:none;                 padding:env(safe-area-inset-top,0px) env(safe-area-inset-right,0px)                  env(safe-area-inset-bottom,0px) env(safe-area-inset-left,0px);",
            );
            if let Some(body) = doc.body() {
                let _ = body.append_child(&el);
            }
            el
        }
    };
    let Ok(Some(style)) = window().get_computed_style(&probe) else { return SafeInsets::ZERO };
    let px = |prop: &str| -> f32 {
        style
            .get_property_value(prop)
            .ok()
            .and_then(|v| v.trim().trim_end_matches("px").parse::<f32>().ok())
            .unwrap_or(0.0)
    };
    let rect = canvas.get_bounding_client_rect();
    let sx = if rect.width() > 0.0 { canvas.width() as f64 / rect.width() } else { 1.0 } as f32;
    let sy = if rect.height() > 0.0 { canvas.height() as f64 / rect.height() } else { 1.0 } as f32;
    SafeInsets {
        left: px("padding-left") * sx,
        top: px("padding-top") * sy,
        right: px("padding-right") * sx,
        bottom: px("padding-bottom") * sy,
    }
}

/// The scene's on-screen touch controls, if an entity carries some.
fn touch_controls_of(world: &hecs::World) -> Option<jengine_core::ecs::components::TouchControls> {
    world
        .query::<&jengine_core::ecs::components::TouchControls>()
        .iter()
        .next()
        .map(|(_, c)| c.clone())
}

fn render_frame(app_cell: &Rc<RefCell<WebApp>>, timestamp: f64) -> Result<(), JsValue> {
    let mut app_ref = app_cell.borrow_mut();
    // Reborrow as &mut WebApp so the borrow checker can do disjoint-field
    // borrows; through `RefMut`'s Deref it can't split per-field.
    let app: &mut WebApp = &mut *app_ref;

    // Wall-clock time since the last frame, before `time_scale` is applied:
    // the overlay reports how fast the game is running, which slow motion
    // must not distort.
    let frame_dt = ((timestamp - app.last_time) / 1000.0) as f32;
    let rate = app.refresh_estimate.observe(frame_dt);
    app.pacer.set_refresh_millihertz(rate);
    // Through the input replay (`input::replay`).
    let real_dt = jengine_core::input::replay::frame_dt(app.pacer.snap(frame_dt).clamp(0.0, 0.1));
    let dt = real_dt * app.time_scale;
    app.last_time = timestamp;
    // Rolling FPS estimate for the debug overlay (3.3). Exponential
    // moving average so the stats HUD doesn't jitter wildly.
    if frame_dt > 0.0 {
        let inst = 1.0 / frame_dt;
        app.fps_ema = if app.fps_ema > 0.0 {
            app.fps_ema * 0.9 + inst * 0.1
        } else {
            inst
        };
    }

    // Camera viewport tracking.
    let UVec2 { x: vw, y: vh } = app.canvas_size;
    app.camera.resize(vw as f32, vh as f32);
    // Same omission the editor viewport had: get_screen_size() reads this, and
    // the GUI lays out against canvas_size, so they have to agree.
    app.input.screen_size = (vw as f32, vh as f32);

    // Where a notch covers the page, re-read every half second or so.
    if app.safe_area_recheck_in == 0 {
        app.safe_area_recheck_in = 30;
        app.ui_scale.safe = read_safe_insets(&app.canvas);
    } else {
        app.safe_area_recheck_in -= 1;
    }

    // Fingers first, so a stick press and the key it holds reach every
    // reader - behaviour graphs, packages, scripts - on the same frame.
    {
        let controls = touch_controls_of(&app.world);
        app.touch.tick(controls.as_ref(), &mut app.input, &app.ui_scale, (vw as f32, vh as f32));
    }

    // Frozen while a scene change is fetching: this world is about to be
    // replaced, and ticking it would let the outgoing scene's scripts queue
    // work (or another transition) against state on its way out. The last
    // frame stays on screen until the new scene lands.
    if !app.transitioning {
        // Pads before anything reads actions: the world tick's input actions
        // are the first reader. The Gamepad API is poll-only, and a browser
        // reports no pads until one is pressed, so an empty result is normal.
        jengine_core::input::gamepad_web::poll_gamepads(&mut app.input);
        // A focused text field types through the hidden textarea: what it
        // got goes into this frame's input *before* the fields read it (and
        // before a replay keeps the frame's input). Synced after them
        // (beside the scripts, where it was), its characters were cleared by
        // `begin_frame` unread, so once the proxy had focus nothing typed
        // reached any field.
        app.text_proxy.sync(&app.canvas, &app.ui_scale, &mut app.input);
        // Hold / tap / double-tap timing, now every device has reported.
        app.input.update_interactions();

        // ECS update phase.
        app.frame_stats.physics_time_ms = 0.0;
        let update_start = performance().now();
        let pending_contacts = tick_world(app, dt);
        // Physics is stepped inside `tick_world` and times itself; charge
        // the rest to `update` so the two rows do not double-count.
        app.frame_stats.update_time_ms = ((performance().now() - update_start) as f32
            - app.frame_stats.physics_time_ms)
            .max(0.0);
        let script_start = performance().now();

        // GUI clicks first: a button press and the script it triggers
        // should land on the same frame, and the handler may well be the
        // thing that queues this frame's scene change.
        // Wheel scrolling of `Overflow::Scroll` GUI containers runs first so
        // the click below hit-tests against the rows actually on screen.
        // UI space is virtual pixels (see `ui::scaling`), so the pointer is
        // divided into it before any UI hit test. `input.mouse_position` stays
        // physical for world-space uses.
        let (pw, ph) = app.input.screen_size;
        let screen = app.ui_scale.virtual_size(pw, ph);
        let ui_mouse = app.ui_scale.to_ui(
            app.input.mouse_position.0,
            app.input.mouse_position.1,
            pw,
            ph,
        );
        if app.input.mouse_wheel_delta != 0.0 {
            jengine_core::ecs::systems::apply_gui_wheel_scroll(
                &app.world,
                &mut app.assets,
                ui_mouse,
                app.input.mouse_wheel_delta,
                screen,
            );
        }
    // Pointer drag / flick / elastic ends, for the same containers the wheel
    // scrolls. Runs before the click dispatch below so a gesture that turned
    // out to be a scroll does not also press the button under the finger.
    {
        let pointer = jengine_core::ui::scroll::PointerInput {
            x: ui_mouse.0,
            y: ui_mouse.1,
            down: app.input.mouse_left,
            just_pressed: app.input.mouse_left_just_pressed,
            just_released: app.input.mouse_left_just_released,
        };
        jengine_core::ecs::systems::update_gui_scroll_drag(
            &app.world,
            &mut app.assets,
            &mut app.gui_scroll,
            pointer,
            screen,
            dt,
        );
        // Publish the gesture so the click dispatch — which lives deep in
        // `ScriptSystem::update` and sees only an `InputState` — can swallow
        // the release that ended a scroll.
        app.input.gui_scroll_dragging = app.gui_scroll.is_dragging();
    }
        // Entity-UI widgets. Buttons, sliders, toggles and text inputs all
        // answer the pointer here. `update_ui_buttons` had existed since the
        // widgets did and no runtime ever called it, and the other three had
        // no interaction code at all - script-driven or nothing (BUGS #61).
        // Runs beside the GUI pointer work so both UI systems hit-test
        // against the same frame's mouse.
    {
        let mut clicked = Vec::new();
        jengine_core::ecs::systems::update_ui_widgets(
            &mut app.world,
            &mut app.input,
            ui_mouse,
            screen.0,
            screen.1,
            &mut clicked,
        );
        // Each click reaches scripts as `ui_click`. The list was collected
        // and dropped until Sep 2026, so a button could only be polled.
        script_host::dispatch_ui_clicks(app, &clicked);
    }
        script_host::process_gui_clicks(app);

        // What a script asked of the page last frame: fullscreen, the
        // cursor, the frame cap.
        if let Some(cap) = app.display.sync(&app.canvas, &app.assets, &mut app.missing_warned, Some(app.fps)) {
            app.fps = cap.unwrap_or(app.project_fps);
            app.frame_cap = jengine_core::time::FrameCap::new(app.fps);
        }
        // Refresh the pathfinding snapshot before any script runs, so a
        // `tilemap_find_path` call sees this frame's tiles and never touches
        // the live world.
        app.nav_snapshot.borrow_mut().refresh_named(&app.world, &crate::script_host::script_names());
        // And the colliders, for `raycast`, with the same timing.
        #[cfg(feature = "physics")]
        app.ray_snapshot.borrow_mut().refresh_named(&app.world, &crate::script_host::script_names());
        // Gravity, contacts and velocities for the scripts about to run.
        jengine_core::scripting::bridge::adapters::js::mirror::publish_physics_for_scripts(
            &app.world,
            app.physics.as_ref(),
            &pending_contacts,
        );

        // Script update phase (feature 3.7 \u2014 parity with native + viewport).
        // Runs after ECS behaviors so trigger overlaps are fresh; may queue
        // camera shakes (drained below) and spawn/despawn entities. Physics
        // contacts from `tick_world` drive `on_collision_*` callbacks.
        jengine_core::profiling::trace::time("scripts", || script_host::run_scripts_frame(app, dt, &pending_contacts));
        app.frame_stats.script_time_ms = (performance().now() - script_start) as f32;
    }

    // Resolve the active game camera AFTER scripts so script-driven camera
    // moves / shakes land on the same frame. Drain script-queued shakes,
    // then advance shake / zoom decay.
    jengine_core::ecs::systems::update_camera(&app.world, &mut app.camera, dt);
    for (amp, dur, freq) in app.pending_camera_shakes.drain(..) {
        app.camera.start_shake(amp, dur, freq);
    }
    app.camera.tick_effects(dt);
    // A pixel-perfect camera draws the world at the art's resolution, through
    // a camera on the art's grid; scripts still see `app.camera`.
    let camera = jengine_core::ecs::wgpu_systems::apply_pixel_plan(
        app.camera,
        (app.canvas_size.x, app.canvas_size.y),
        &app.renderer.device,
        &mut app.post_process,
        &mut app.light_pass,
    );
    // What this frame can see, for culling before the world pass (which sets
    // it again per camera).
    jengine_core::renderer::frame_view::set_frame_view(&camera);

    // Slice 3.5.A — resolve world-space transforms before rendering.
    // Slice 3.5.C — first advance skeletal animation players.
    // Skeletons, IK, the physics blend, world transforms, trails, cloth and
    // carving - the order every host poses its world for drawing in.
    jengine_core::ecs::systems::pose_world_for_draw(&mut app.world, &mut app.assets, app.physics.as_mut(), dt);

    // ── package systems: Animation + PreRender ────────────────────────────
    // After everything that could move the camera this frame - `PreRender`
    // is where `parallax_scroll` reads it. `Render` / `PostRender` are not
    // run, as in every host: no renderer is shared through the resource map,
    // so a draw system would run and draw nothing. Frozen with the rest of
    // the world tick while a scene change is fetching.
    if !app.transitioning {
        use jengine_core::package::host::FrameResources;
        use jengine_core::package::system_schedule::SystemPhase;
        app.package_host.run_phase_with(
            SystemPhase::Animation,
            &mut app.world,
            dt,
            FrameResources { input: &mut app.input, assets: &mut app.assets, camera: app.camera },
        );
        app.package_host.run_phase_with(
            SystemPhase::PreRender,
            &mut app.world,
            dt,
            FrameResources { input: &mut app.input, assets: &mut app.assets, camera: app.camera },
        );
        // Sprites a package spawned or re-regioned in the phases since
        // `tick_world` resolved regions draw right on their first frame
        // (BUGS #110).
        jengine_core::ecs::systems::resolve_sprite_regions(&mut app.world, &app.assets);
    }

    // Particle update.
    jengine_core::particles::tick_particles(&app.world, &mut app.particle_system, dt);

    // Counters + memory for the overlay. Sampled here, after the world has
    // been ticked and before anything is drawn, so they describe the world
    // this frame actually renders.
    app.frame_stats.fps = app.fps_ema;
    app.frame_stats.frame_time_ms = frame_dt * 1000.0;
    app.frame_stats.entity_count = app.world.len();
    app.frame_stats.sprite_count = app
        .world
        .query::<&jengine_core::ecs::components::SpriteComponent>()
        .iter()
        .count() as u32;
    app.frame_stats.particle_count = app.particle_system.alive_count();
    // `memory_bytes` is reserved wasm linear memory here, which never
    // shrinks; `memory_live_bytes` is the heap figure that does. The overlay
    // shows both - see `debug_overlay::memory_lines`.
    app.frame_stats.memory_bytes = jengine_core::profiling::memory::resident_bytes() as u64;
    app.frame_stats.memory_live_bytes = jengine_core::profiling::memory::live_bytes() as u64;
    app.frame_stats.memory_peak_bytes = jengine_core::profiling::memory::peak_bytes() as u64;

    let render_start = performance().now();

    // Acquire frame.
    let mut frame = match app.renderer.begin_frame() {
        Ok(f) => f,
        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
            let size = app.canvas_size;
            app.renderer.resize(size);
            return Ok(());
        }
        Err(e) => return Err(JsValue::from_str(&format!("surface error: {e:?}"))),
    };

    // Per-frame full reset of the sprite batch — rewinds the per-flush
    // vertex buffer pool so each batch within this frame writes to its own
    // GPU buffer (otherwise wgpu coalesces all `queue.write_buffer` calls
    // and every draw reads the last flush's vertices).
    app.sprite_batch.begin_frame();
    // Seconds since start, for graph materials' `TimeInput` - native and
    // the editor always fed it; the web left every animated graph still.
    app.sprite_batch.set_time(((performance().now() - app.start_time) / 1000.0) as f32);

    let lighting_active = has_lights(&app.world);
    let scene_clear = app.renderer.clear_color;

    // On-screen touch controls, resolved before the closures borrow `app`.
    let touch_draws = {
        let UVec2 { x: vw, y: vh } = app.canvas_size;
        let (sw, sh) = app.ui_scale.screen_virtual_size(vw as f32, vh as f32);
        app.touch.draw_list(touch_controls_of(&app.world).as_ref(), glam::Vec2::new(sw, sh))
    };

    // Pre-resolve gui jobs so the ui_flush closure can mutably hold assets.
    let gui_jobs: Vec<(GuiDocument, GuiComponent)> = app
        .world
        .query::<&GuiComponent>()
        .iter()
        .filter(|(_, c)| c.visible)
        .filter_map(|(_, c)| app.assets.get_gui(&c.gui_id).map(|d| (d.clone(), c.clone())))
        .collect();

    // Views, the world, the post chain and the UI: one function every host
    // shares (`ecs::frame_draw`). This host's part is its policy - its
    // material lookup and what it says about a texture it is fetching.
    {
        use jengine_core::ecs::frame_draw::{FrameDraw, UiScene, WorldScene, draw_frame};
        let renderer = &app.renderer;
        let sort_layers = app.assets.active_sort_layers().clone();
        // Lifted out of `app.assets` before it is lent `&mut`; each font is
        // shared by `Arc`, so this is a refcount bump.
        let frame_fonts = jengine_core::renderer::font_asset::FrameFonts::gather(
            &app.assets,
            &app.font_data,
            BUILTIN_FONT_ID,
        );
        let mut host = WebDrawHost { graphs: &app.shader_graphs, missing: &mut app.material_missing };
        // This runtime assembles its own `FrameStats` (the timings above);
        // `render_time_ms` and `draw_calls` are last frame's.
        let overlay = app.debug_overlay.is_visible().then_some((&app.debug_overlay, &app.frame_stats));
        draw_frame(
            FrameDraw {
                device: &renderer.device,
                queue: &renderer.queue,
                scene_format: renderer.scene_format,
                color_format: renderer.color_format,
                world: &app.world,
                assets: &mut app.assets,
                warned: &mut app.missing_warned,
                graph_pipelines: &mut app.graph_pipelines,
                sprite_batch: &mut app.sprite_batch,
                lit_sprite_batch: &mut app.lit_sprite_batch,
                flat_normal: &app.flat_normal,
                particle_system: &app.particle_system,
                frame_fonts: &frame_fonts,
                sort_layers: &sort_layers,
                post: &mut app.post_process,
                light: &mut app.light_pass,
                view_targets: &mut app.view_targets,
                host: &mut host,
            },
            &mut frame,
            WorldScene {
                camera,
                surface: (app.canvas_size.x, app.canvas_size.y),
                lighting: lighting_active,
                clear: scene_clear,
                debug_draws: &app.debug_draws,
                physics_outlines: app.debug_overlay.physics,
                underlay: None,
            },
            UiScene { ui_scale: app.ui_scale, gui_jobs: &gui_jobs, touch_draws: &touch_draws, overlay },
        );
    }

    // The lighting composite and the post chain both run inside the block
    // above: the composite between the world pass and the pass for things
    // that must not be lit twice, the post chain before the UI.

    // Fade / flash overlay — after post-processing, so a fade to black is
    // not bloomed. Same pipeline and ordering as the native runner.
    // A fade or flash runs on **real** time: it is the screen, not the world,
    // so a hitstop (time scale 0) must not freeze it at full brightness and
    // a slow motion must not stretch it (Shooter Classics' Void Armada).
    if let Some(t) = app.fade_active.as_mut() {
        t.elapsed += real_dt;
        let color = t.current_color();
        let done = t.is_done();
        if app.fade_overlay.is_none() {
            match jengine_core::fade_overlay::FadeOverlay::new(
                &app.renderer.device,
                app.renderer.color_format,
            ) {
                Ok(o) => app.fade_overlay = Some(o),
                Err(e) => {
                    console_warn!("fade overlay init failed: {}", e);
                    app.fade_active = None;
                }
            }
        }
        if let Some(overlay) = app.fade_overlay.as_ref() {
            overlay.render(&app.renderer.queue, &mut frame.encoder, &frame.view, color);
        }
        if done {
            app.fade_active = None;
        }
    }

    frame.present(&app.renderer.queue);

    // Read back what the pass cost and how many draws it took. Both land in
    // the overlay one frame later, which is why everything else is sampled
    // before the pass rather than all at once here.
    // Debug shapes are per-frame: a script re-emits them every tick, so
    // keeping them would both leak and smear every shape across the frame
    // it was drawn in.
    app.debug_draws.clear();

    app.frame_stats.render_time_ms = (performance().now() - render_start) as f32;
    app.frame_stats.draw_calls = app.sprite_batch.frame_draw_calls;

    app.input.begin_frame();
    Ok(())
}
