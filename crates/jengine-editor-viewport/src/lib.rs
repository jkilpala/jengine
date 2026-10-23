//! jEngine Editor Viewport — WASM/WebGPU module.
//!
//! Phase 8.2e rebuild: from-scratch wgpu rewrite of the original
//! ~7000 LOC glow viewport. **Every `#[wasm_bindgen]` signature
//! from the previous version is preserved verbatim** so the Svelte
//! editor frontend continues to load and call without changes.
//!
//! ## What works (Stage 2 complete — 2.1 → 2.7)
//! - Async WebGPU device acquisition kicked off from the (sync)
//!   `viewport_init` call. All other entry points early-return with
//!   sensible defaults until the device is ready.
//! - Scene loading via [`load_scene_auto`] into a [`hecs::World`]; a
//!   parallel `scene_entities: Vec<Entity>` keeps the editor's
//!   index-based addressing working.
//! - Texture and GUI asset uploads via [`AssetManager`].
//! - Editor-mode rendering: tilemaps, sprites (multi-texture batched),
//!   text, GUI documents, particles, lighting compositing — all using
//!   the same `*_wgpu` render systems the native runtime uses.
//! - Letterboxed game viewport when `viewport_set_camera_size` is set.
//! - **Play mode** (Stage 2.1): per-frame `tick_world` with input,
//!   action axes, scene snapshot/restore on play/stop.
//! - **Behavior graphs** (Stage 2.2): pre-loaded `.behavior` graphs
//!   instantiate per entity on play; ticked each frame.
//! - **Physics** (Stage 2.3): rapier2d world built on play from
//!   editor-supplied gravity/length-unit; stepped each tick.
//! - **Scene transitions** (Stage 2.4): play-mode `push/pop/replace`
//!   with a snapshot stack; runtime entity round-trip via v2 JSON.
//! - **Asset previews** (Stage 2.5): particle / animation / tilemap
//!   previews each own a separate `<canvas>` + wgpu surface.
//! - **PiP / frame stats / debug bounds** (Stage 2.6): second
//!   `RenderPass` with a custom camera into a swap-chain sub-rect;
//!   rolling fps history; entity / collider bounds JSON.
//! - **Lighting compositing** (Stage 2.7): `gather_lights` →
//!   `WgpuLightingPass.render_lights` + `composite` runs between
//!   the scene pass and post-process when `show_lighting` is on
//!   and the world has any light components.
//!
//! ## Remaining stubs (return `Ok(())` / defaults)
//! - **`viewport_get_debug_draws`** — script-emitted debug shapes.
//!   Lua/QuickJS are native-only; reserved for the future
//!   browser-script backend.
//! - **Per-stage frame profiling** — `viewport_get_frame_stats`
//!   reports `0` for `draw_calls` / `physics_time_ms` /
//!   `script_time_ms` / `update_time_ms` (only `fps` and total
//!   `frame_time_ms` are real today).

#![cfg(target_arch = "wasm32")]
#![allow(clippy::too_many_arguments)]

use jengine_core::scripting::bridge::adapters::js::mirror::{self, json_to_jsvalue, play_namespace};
use jengine_core::audio::web_audio::WebAudioBackend;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;

use glam::{UVec2, Vec2};
use hecs::{Entity, World};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlCanvasElement;

/// Count heap allocations so the profiler panel can report live memory
/// next to reserved linear memory - the pair is what distinguishes a leak
/// from allocator high-water. Two relaxed atomic ops per allocation; see
/// `jengine_core::profiling::memory`.
#[global_allocator]
static ALLOC: jengine_core::profiling::memory::TrackingAllocator<std::alloc::System> =
    jengine_core::profiling::memory::TrackingAllocator::new(std::alloc::System);

use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::behavior::system::BehaviorGraphSystem;
use jengine_core::ecs::components::{
    BehaviorGraphComponent, Camera2DComponent, Collider2D, ColliderShape,
    GuiComponent, SpriteComponent, TextComponent, Transform,
};
use jengine_core::ecs::wgpu_systems::{
    render_particles_wgpu, render_sprites_wgpu_multi, render_sprites_wgpu_multi_materialized,
    render_tilemaps_wgpu,
};
use jengine_core::input::{InputAction, InputState, KeyCode};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::package::system_schedule::SystemPhase;
use jengine_core::particles::{ParticleEmitter, ParticleSystem};
use jengine_core::physics::PhysicsWorld;
use jengine_core::renderer::backend::wgpu::{
    WgpuLightingPass, WgpuPostProcessStack, WgpuRenderer, WgpuShapeBatch, WgpuSpriteBatch,
};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::graph_pipeline_cache::{GraphPipelineCache, GraphPipelineKey};
use jengine_core::renderer::shader_graph::ShaderGraph;
use jengine_core::renderer::text::{built_in_font_atlas, FontData};
use jengine_core::scene::format::{
    load_scene_auto, load_scene_v2, EntityDataV2, SceneDataV2,
};
use jengine_core::scripting::bridge::adapters::js::BrowserScriptFns;
use jengine_core::ui::gui_document::{load_gui, GuiDocument};
use jengine_core::renderer::lighting::has_lights;

// ---------------------------------------------------------------------------
// Stage 2.9 — Browser script types
// ---------------------------------------------------------------------------

/// Editor-supplied script source bundle. Mirrors the shape the legacy
/// glow-viewport accepted via `viewport_play` / `viewport_apply_scene_transition`.
#[derive(Debug, Clone, serde::Deserialize)]
struct ScriptSource {
    entity_index: usize,
    /// Editor-supplied display name; surfaced in error messages once
    /// Stage 2.9c wires `format_entity_label`. Currently retained on the
    /// payload for forward-compat.
    #[serde(default)]
    #[allow(dead_code)]
    entity_name: String,
    #[serde(default)]
    source_path: String,
    source: String,
    /// `"Lua"` or `"JavaScript"`; anything else is treated as JS.
    #[serde(default)]
    language: String,
}

// A compiled script attached to one entity, the spawn map and the calls into
// a script are shared with the web runtime (`adapters::js::scripts`).
use jengine_core::scripting::bridge::adapters::js::scripts::{
    SCRIPT_ENTITY_MAP, ScriptInstance, call_script_fn, compile_browser_script, find_entity_by_id, note_spawn,
    script_names,
};

const BUILTIN_FONT_ID: &str = "__font/builtin";

// ---------------------------------------------------------------------------
// Console logging
// ---------------------------------------------------------------------------

macro_rules! console_log { ($($arg:tt)*) => { web_sys::console::log_1(&format!($($arg)*).into()) }; }
macro_rules! console_warn { ($($arg:tt)*) => { web_sys::console::warn_1(&format!($($arg)*).into()) }; }
macro_rules! console_error { ($($arg:tt)*) => { web_sys::console::error_1(&format!($($arg)*).into()) }; }

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct ViewportApp {
    /// `None` until the async wgpu init resolves.
    gpu: Option<GpuState>,
    /// Pending canvas + size captured at viewport_init time so the
    /// async device-acquisition task knows what to bind to.
    canvas: HtmlCanvasElement,
    canvas_size: UVec2,
    /// What a game asked of the page in Play - fullscreen, the cursor
    /// (`jengine_core::display::web`); given back on Stop.
    display: jengine_core::display::web::WebDisplay,
    /// The hidden textarea a focused text field types through in Play
    /// (`jengine_core::input::text_entry::web`); let go on Stop.
    text_proxy: jengine_core::input::text_entry::web::TextProxy,
    /// How the UI is scaled when the canvas is not the size the project's UI
    /// was authored for. Set by the editor from project settings; until then
    /// it is the default 1280x720 scale-to-fit.
    ui_scale: jengine_core::ui::scaling::UiScale,
    /// The device preview's simulated safe area, as fractions of the canvas
    /// (left, top, right, bottom): re-applied to `ui_scale.safe` in pixels
    /// every frame, so resizing the panel keeps the notch where it belongs.
    safe_fraction: [f32; 4],
    /// The post-process chain the edited scene was showing before Play
    /// started, restored on Stop.
    ///
    /// Effects are session state the world does not own, like looping music:
    /// a script that switches bloom on during Play leaves it on afterwards,
    /// and the snapshot cannot carry it back because `SceneDataV2` has no
    /// `post_process` field (BUGS #84).
    pre_play_post_process: Option<jengine_core::renderer::postprocess::PostProcessSettings>,
    /// The edited scene's name, put back on Stop - Play may load others.
    pre_play_scene_name: String,
    /// Set to true once the async init is in flight (so repeat
    /// `viewport_init` calls don't double-spawn).
    init_dispatched: bool,
    /// CPU-side scene + assets — mutate freely before/after gpu is up.
    world: World,
    scene_entities: Vec<Entity>,
    /// How many leading `scene_entities` slots came from the authored
    /// scene. Anything at or past this index was adopted from the live
    /// world by [`viewport_get_runtime_entities`] — i.e. spawned by a
    /// script during play.
    authored_entity_count: usize,
    registry: ComponentRegistry,
    /// Registered packages and the schedule holding their systems.
    ///
    /// The editor gets one for the same reason the native runtime does: a
    /// package that only works natively is a package nobody can author
    /// against. Note packages are linked at compile time, so a *user's* own
    /// package reaches the editor only when the viewport WASM is rebuilt with
    /// it — see `PackageHost`'s docs.
    package_host: jengine_core::package::host::PackageHost,
    /// Prefab templates, mirrored in Rust so scene loads can expand
    /// `prefab_instance` entries. The JS copy on `__jengine_play.prefabs`
    /// still exists for the browser bridge's `spawn_prefab`; this one serves
    /// the *authored* path, which had no expansion at all until now — an
    /// entity built from a PrefabInstance marker simply came up empty in the
    /// editor while working natively.
    prefabs: std::collections::HashMap<String, jengine_core::scene::format::EntityData>,
    /// The prefabs as the editor pushed them; `prefabs` is these with every
    /// variant resolved (`scene::prefab::resolve_prefab_variants`), redone
    /// as each arrives, since a variant may come before its base.
    raw_prefabs: std::collections::HashMap<String, jengine_core::scene::format::EntityData>,
    assets: AssetManager,
    missing_warned: HashSet<String>,
    /// Texture ids already handed to the editor to load
    /// (`viewport_take_texture_requests`).
    texture_requested: HashSet<String>,
    /// Material ids to ask the editor for (`viewport_take_material_requests`):
    /// every prefab's, queued as it arrives, so a spawned sprite's graph is
    /// here before the first one is drawn.
    material_wanted: std::collections::BTreeSet<String>,
    /// Material ids already handed to the editor to load.
    material_requested: HashSet<String>,
    font_data: FontData,
    camera: Camera2D,
    /// Where the camera was last frame, for `DespawnOffscreen`'s
    /// `behind_only`: "behind" is the opposite of the way it is travelling.
    prev_camera: glam::Vec2,
    clear_color: [f32; 4],
    selected: i32,
    /// Letterboxed game resolution. `None` means use full canvas.
    game_viewport: Option<[f32; 2]>,
    show_lighting: bool,
    particle_system: ParticleSystem,
    /// Logs accumulated for `viewport_get_logs`. Drains on each call.
    log_buffer: Vec<String>,
    /// 2.3 save system. Play mode in the editor writes to the same
    /// `localStorage` a web build uses, so a save made while testing is the
    /// save the deployed game reads - which is the point of testing it here.
    save_system: jengine_core::save::SaveSystem,
    /// 2.4 — audio mixer bus tree. Plain data from `jengine_core::audio`;
    /// the browser audio backend has no bus concept, so the effective volume
    /// is applied when a sound plays, exactly as the native audio apply does.
    mixer: jengine_core::audio::AudioMixer,
    /// Active scene fade / screen flash, advanced and drawn each frame.
    fade_active: Option<jengine_core::fade_overlay::TransitionState>,
    /// Overlay pipeline, created lazily on first use — most sessions never
    /// fade, and it owns a render pipeline.
    fade_overlay: Option<jengine_core::fade_overlay::FadeOverlay>,
    /// 2.6 — tilemaps as of the start of this frame, for the synchronous
    /// `tilemap_find_path` host callback. Shared with the installed JS
    /// closures, which is why it is an `Rc<RefCell<…>>` separate from the
    /// app: a callback that reached back into the app would double-borrow
    /// while scripts are running.
    nav_snapshot: std::rc::Rc<
        std::cell::RefCell<jengine_core::scripting::bridge::adapters::js::nav::NavSnapshot>,
    >,
    /// Every collider as of the start of this frame, for the synchronous
    /// `raycast` host callback. Separate from the app for the same reason
    /// `nav_snapshot` is: the callback runs while scripts hold the app's
    /// `borrow_mut()`, so it must not reach back for the world.
    ray_snapshot: std::rc::Rc<
        std::cell::RefCell<jengine_core::physics::ray::RaySnapshot>,
    >,
    /// Pre-loaded `.behavior` graph JSON keyed by graph_path. Stored
    /// for Stage 2 play-mode wiring; ignored in Stage 1.
    pending_behaviors: HashMap<String, String>,
    /// Slice 13d — pre-parsed shader graphs keyed by `.shader` path.
    /// The editor pushes each graph via `viewport_load_shader_graph`
    /// before play mode so `flush_by_id` can lazily compile pipelines
    /// without filesystem access.
    pending_shader_graphs: HashMap<String, ShaderGraph>,
    /// Slice 13d — `performance.now()` reading captured at startup.
    /// Used to feed `WgpuSpriteBatch::set_time` each frame so graph
    /// shaders that read `globals.time.x` (e.g. pulsing emission)
    /// animate in the editor viewport, mirroring the native runtime.
    start_time_ms: f64,
    /// Stage 1: play mode is a stub; this flag flips on `viewport_play`
    /// so the editor's "is playing" status reads correctly.
    playing: bool,
    /// Play mode is running but frozen: the world stops ticking while
    /// rendering continues, so the editor can inspect and edit a
    /// stationary frame. Meaningless unless `playing`.
    paused: bool,
    /// One-shot: advance a single fixed-dt frame on the next render and
    /// return to the paused state.
    step_pending: bool,
    /// Stage 2.1 — Input state shared between editor canvas listeners
    /// and the per-frame `tick_world` call. Action axes are populated
    /// on `viewport_play` from the editor-supplied `actions_json`.
    input: InputState,
    /// On-screen touch controls. The editor has no touch screen; a layout
    /// with `show: "always"` is worked with the mouse, which is how one is
    /// tried out here. See `input::touch_controls`.
    touch: jengine_core::input::touch_controls::TouchControlsState,
    /// Stage 2.1 — `performance.now()` reading from the previous render.
    /// `None` means "first frame after entering play"; the next render
    /// seeds it and uses dt=0 for that frame.
    last_tick_ms: Option<f64>,
    /// The display's rate, worked out from the frames (a browser does not
    /// say), and the pacer it feeds - which turns the 16 / 17 ms a
    /// millisecond clock reports at 60 Hz into an even step, as native's
    /// pacer does with its own jitter (`jengine_core::time`).
    refresh_estimate: jengine_core::time::RefreshEstimate,
    pacer: jengine_core::time::FramePacer,
    /// Play time since trails last recorded: `tick_play_world` adds its dt,
    /// the render records after resolving the hierarchy and takes it.
    trail_dt: f32,
    /// Stage 2.1 — Snapshot of the world (as `SceneDataV2` JSON) taken
    /// at `viewport_play` time. Restored on `viewport_stop` so the
    /// editor can return the user to their pre-play scene state. The
    /// returned JSON is also the value `viewport_stop` hands back to
    /// the Svelte toolbar for inspector refresh.
    play_snapshot: Option<String>,
    /// Stage 2.4 — the scenes under the current one, for play-mode
    /// push/pop. `viewport_apply_scene_transition` pushes here when
    /// transitioning while playing; `viewport_pop_play_scene` pops the
    /// top entry and restores it (re-running scripts on the restored
    /// world). Cleared when play mode exits via `viewport_stop`.
    /// One [`PushedScene`] per `push_scene` still waiting for its pop.
    scene_stack: Vec<PushedScene>,
    /// Stage 2.9i — most recent `scripts_json` payload (from
    /// `viewport_play` or `viewport_apply_scene_transition`) so we can
    /// push it onto `scene_stack` alongside the scene snapshot when
    /// the editor stacks another scene over the current play state.
    last_scripts_json: String,
    /// Stage 2.2 — Behavior-graph runtime. Cache + per-entity
    /// instances live here; populated from `pending_behaviors` on
    /// `viewport_play`. WASM-safe (uses `js_sys::Date::now()`).
    behavior_system: BehaviorGraphSystem,
    /// Stage 2.3 — rapier2d physics world. `None` until
    /// `viewport_play` builds one from the editor-supplied
    /// gravity/length-unit; dropped on `viewport_stop`.
    physics: Option<PhysicsWorld>,
    /// Patrol routes declared by the current scene, for `PatrolPathRef`.
    ///
    /// Rebuilt from the scene's `paths` block on every load and transition.
    /// `None` when the scene declares none, which `update_patrol_paths`
    /// tolerates — it used to be `None` unconditionally, so patrol entities
    /// never moved in this runtime (BUGS #7).
    path_library: Option<jengine_core::ecs::path::Path2DLibrary>,
    /// The gravity + length-unit `physics` was built from, kept so a scene
    /// transition can build a fresh one.
    ///
    /// A transition replaces `app.world`, which invalidates every body handle,
    /// so the old `PhysicsWorld` must go. It used to just go — set to `None`
    /// and never rebuilt — which killed physics for the rest of the play
    /// session, because only `viewport_play` created one and a transition does
    /// not re-enter play. Every scene reached from a hub menu therefore ran
    /// without physics in the editor while working natively and on web, both
    /// of which rebuild here (BUGS #48).
    ///
    /// `None` means the project asked for a backend the engine does not have,
    /// in which case there is deliberately nothing to rebuild.
    physics_config: Option<(f32, f32, f32, Option<f32>)>,
    /// Drag / flick / elastic state for scrolling GUI containers.
    gui_scroll: jengine_core::ui::scroll::GuiScrollState,
    /// Stage 2.6 — Pending picture-in-picture render request. Set by
    /// `viewport_render_pip`; consumed (drained) at the end of the
    /// next `render_editor` and rendered as a second pass with a
    /// custom camera into a sub-rect of the swap-chain texture.
    pip_request: Option<PipRequest>,
    /// Stage 2.6 — Rolling frame-time history (ms), capped at 120 entries.
    /// Pushed each `viewport_render` call.
    frame_history: VecDeque<f32>,
    /// Stage 2.6 — last frame's render stats for `viewport_get_frame_stats`.
    frame_stats: FrameStats,
    /// Stage 2.9 — compiled browser-script handles, keyed by entity id
    /// (`Entity::id()`). Populated by `viewport_play` from `scripts_json`;
    /// dropped on `viewport_stop`. Cloned per-entity so two entities sharing
    /// the same source still each carry their own JsValue refs.
    script_fns: HashMap<u32, ScriptInstance>,
    /// Stage 2.9 — cache of compiled fns keyed by source_path. Lets two
    /// entities pointing at the same script reuse one compiled JS triple.
    script_cache: HashMap<String, BrowserScriptFns>,
    /// Debug shapes from scripts (`draw_debug_*`) and the F4 overlay during
    /// the most recent tick. Drained by `viewport_get_debug_draws`, as the
    /// JSON the editor's overlay draws.
    debug_draws: Vec<jengine_core::debug_draw::DebugDrawCommand>,
    /// F4 in Play: outline every collider and joint. Sent to the editor's
    /// overlay through `debug_draws`, the same channel as `draw_debug_*`.
    physics_debug: bool,
    /// P1.3 — camera-shake requests queued by browser-JS scripts via the
    /// `cmsh` command tag. Drained each tick and forwarded to
    /// `Camera2D::start_shake`. Stored as `(amplitude, duration,
    /// frequency)`; a zero frequency means "use engine default".
    pending_camera_shakes: Vec<(f32, f32, f32)>,
    /// Stage 2.9 — set to `true` once `__jengine_play` has been seeded
    /// in the current play session. Reset on `viewport_stop`.
    play_namespace_ready: bool,
    /// Stage 2.9c — host for script-driven property tweens (position,
    /// rotation, scale, sprite color). Distinct from
    /// `behaviors::update_tweens` which advances the ECS Tween component.
    /// Persists across play sessions but the manager is cheap and gets
    /// implicitly cleared because completed tweens auto-drain.
    tween_manager: jengine_core::animation::tween::TweenManager,
    /// The videos playing (`jengine_core::video`), each a `<video>` element
    /// whose file the editor hands over (`viewport_load_video`).
    video: jengine_core::video::VideoSystem,
    /// Stage 2.9g — GUI click callback registry. Keyed by
    /// `(gui_id, node_id)`; value is `(script-entity-id, callback_name)`.
    /// Populated by the `goc` command. Editor-side GUI rendering calls
    /// `viewport_dispatch_gui_click(gui_id, node_id)` when the user
    /// clicks the matching node, and we invoke the script callback on
    /// the registered entity. Cleared on `viewport_stop`.
    gui_click_callbacks: HashMap<(String, String), (u32, String)>,
    /// Clips already reported as unplayable, so a missing sound logs once
    /// instead of once per call. A combat script calls play_sound on every hit;
    /// at 48 enemies that flooded the console, and each line is a DOM update in
    /// the editor — the logging cost more than the sound would have.
    audio_warned: std::collections::HashSet<String>,
    /// Script `set_time_scale`. Multiplies the dt fed to the world tick, so
    /// 0.0 pauses behaviours, velocity, lifetimes and animation together.
    time_scale: f32,
    /// The last play tick's wall-clock dt, before `time_scale`: what a fade
    /// or flash advances by when the frame is drawn.
    real_dt: f32,
    /// Stage 2.9k — dynamic `load_script(entity, path)` requests
    /// emitted by browser scripts (`lds` command). The editor owns
    /// the asset DB, so the viewport queues `(entity_id, path)` pairs
    /// here and exposes them via `viewport_drain_pending_script_loads`.
    /// The editor reads each pair, fetches the source, and calls
    /// `viewport_inject_script` to compile + register + init it.
    /// Cleared on `viewport_stop` and at the start of
    /// `load_and_run_scripts` (since a fresh scene invalidates pending
    /// requests against the previous world).
    pending_script_loads: Vec<(u32, String)>,
    /// Slice 8 — Web Audio backend for editor preview. Lazily
    /// initialised on the first audio command (browsers block
    /// AudioContext creation until a user gesture). Replaces the
    /// historical "preview no-op" log lines.
    web_audio: WebAudioBackend,
    /// The sounds `AudioSource`s are playing, by entity (`audio::spatial`).
    spatial_audio: jengine_core::audio::spatial::SpatialAudio,
    /// Entities that have already had `on_spawn` dispatched. `init` runs on
    /// the frame a script is installed; `on_spawn` fires once on the frame
    /// after, matching the documented contract and the native runner.
    spawn_notified: std::collections::HashSet<u32>,
}

/// A scene parked by `push_scene` while another plays over it.
struct PushedScene {
    /// The world **as it was when pushed** - what `pop_scene` restores.
    ///
    /// This used to be `play_snapshot`, the copy taken when the scene was
    /// *loaded*, so every pop in the editor rewound the scene to its start:
    /// a monster-catching RPG's hero came back from each battle on the start tile.
    /// Native and web always snapshotted the live world here.
    live: String,
    /// `play_snapshot` as it stood at the push - the scene as loaded, which
    /// becomes `play_snapshot` again on pop. Kept apart from `live` so a
    /// Stop after a pop still hands back an authored scene, not one with
    /// play-mode state in it.
    loaded: String,
    scripts: String,
    /// The pushed scene's name, for `Scene.get_name()` after the pop.
    name: String,
    /// The effect chain rides along because the snapshot cannot carry it -
    /// `SceneDataV2` has no `post_process` field - and a pop that does not
    /// restore it leaves the pushed-over scene's bloom running on the scene
    /// underneath (BUGS #84).
    post_process: jengine_core::renderer::postprocess::PostProcessSettings,
}

/// Stage 2.6 — pending picture-in-picture render. Editor passes the
/// game camera (centered at cam_x/cam_y with cam_viewport_w/h logical
/// size and zoom) plus the destination pixel rect on the swap chain
/// (pip_x is from the *bottom-left*, matching the historic GL convention
/// the editor still emits).
#[derive(Clone, Copy)]
struct PipRequest {
    cam_x: f32,
    cam_y: f32,
    cam_zoom: f32,
    cam_vw: f32,
    cam_vh: f32,
    px: f32,
    py: f32,
    pw: f32,
    ph: f32,
}

/// Stage 2.6 + 2.8 — last-frame profiling counters, surfaced via
/// `viewport_get_frame_stats` JSON. Sub-stage times (physics, script,
/// update, render) are measured with `performance.now()` for
/// sub-millisecond resolution.
#[derive(Default, Clone, Copy)]
struct FrameStats {
    fps: f32,
    frame_time_ms: f32,
    entity_count: u32,
    sprite_count: u32,
    /// Wall-clock `Date.now()` of the previous render, used to derive
    /// `frame_time_ms` for the current call. `None` on the first frame.
    last_render_ms: Option<f64>,
    /// Stage 2.8 — `WgpuLightingPass` + post-process + scene render
    /// pass. Wall-clock duration of `render_editor` measured with
    /// `performance.now()`.
    render_time_ms: f32,
    /// Stage 2.8 — rapier2d step + ECS sync (play mode only).
    physics_time_ms: f32,
    /// Stage 2.8 — behavior-graph ticks + command flush + trigger
    /// dispatch (play mode only). Reserved for the future browser
    /// JS/Lua tick when scripts land.
    script_time_ms: f32,
    /// Stage 2.8 — total `tick_play_world` time minus physics +
    /// script (i.e. velocity, behaviors, animations, particles).
    update_time_ms: f32,
    /// Stage 2.9j — total wgpu draw calls issued by the sprite batch
    /// during the most recent `render_editor` (covers main scene pass
    /// + PiP pass + UI/GUI flushes; everything goes through the same
    /// `WgpuSpriteBatch::flush` which increments `frame_draw_calls`).
    /// Reset via `sprite_batch.begin_frame()` at the start of every render.
    draw_calls: u32,
}

/// The prefix `material_pipeline` notes a missing material under in
/// `missing_warned`.
const MATERIAL_REQUEST: &str = "__material:";

/// The editor's part of `ecs::frame_draw`: materials are built from the
/// shader graphs the editor pushed, and a texture with no pixels is asked of
/// the editor (through `missing_warned`, which `viewport_take_texture_requests`
/// reads).
struct EditorDrawHost<'a> {
    graphs: &'a HashMap<String, ShaderGraph>,
}

impl jengine_core::ecs::frame_draw::DrawHost for EditorDrawHost<'_> {
    fn material_pipeline(
        &mut self,
        assets: &mut AssetManager,
        graph_pipelines: &mut GraphPipelineCache,
        warned: &mut HashSet<String>,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        batch: &WgpuSpriteBatch,
        mid: &str,
    ) -> Option<std::sync::Arc<wgpu::RenderPipeline>> {
        material_pipeline(assets, self.graphs, graph_pipelines, warned, device, format, batch, mid)
    }
    fn texture_missing(&mut self, tex_id: &str) {
        console_log!("viewport: texture '{}' not loaded yet - asking the editor for it", tex_id);
    }
    fn warn(&mut self, message: &str) {
        console_warn!("viewport: {}", message);
    }
}

/// The graph pipeline a sprite's material draws with, or `None` for the
/// default one: built from the shader graph the editor pushed
/// (`viewport_load_shader_graph`) on first use. One function for the world
/// pass and the emission pass, which unlit sprites draw in when the scene is
/// lit - the emission pass used a plain flush until Sep 2026, so an unlit
/// sprite's graph material vanished there. A material the editor has not
/// pushed is noted for `viewport_take_material_requests`.
#[allow(clippy::too_many_arguments)]
fn material_pipeline(
    assets: &AssetManager,
    graphs: &HashMap<String, ShaderGraph>,
    graph_pipelines: &mut GraphPipelineCache,
    warned: &mut HashSet<String>,
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    batch: &WgpuSpriteBatch,
    mid: &str,
) -> Option<std::sync::Arc<wgpu::RenderPipeline>> {
    let Some(material) = assets.get_material(mid) else {
        // Not pushed yet - one only a spawned prefab or a script names, say.
        // Noted for `viewport_take_material_requests`; drawn plain until the
        // editor answers.
        if !jengine_core::renderer::material::is_builtin_material(mid) {
            warned.insert(format!("{MATERIAL_REQUEST}{mid}"));
        }
        return None;
    };
    let (graph_path, blend_mode) = material.shader_graph_path.as_ref().map(|p| (p.clone(), material.blend_mode))?;
    let key = GraphPipelineKey { shader_graph_path: graph_path.clone(), blend_mode };
    if let Some(p) = graph_pipelines.get(&key, format) {
        return Some(std::sync::Arc::clone(p));
    }
    let Some(graph) = graphs.get(&graph_path) else {
        if warned.insert(format!("__gp:{graph_path}")) {
            console_warn!(
                "viewport: material '{}' references missing shader graph '{}' (push via viewport_load_shader_graph)",
                mid,
                graph_path
            );
        }
        return None;
    };
    match graph_pipelines.insert_from_graph(device, format, batch, key, graph) {
        Ok(p) => Some(p),
        Err(e) => {
            if warned.insert(format!("__gp:{graph_path}")) {
                console_warn!("viewport: graph pipeline build failed for '{}': {:#}", graph_path, e);
            }
            None
        }
    }
}

/// Stage 2.8 — `performance.now()` reading in milliseconds. Falls back
/// to `Date.now()` if the Performance API isn't available (shouldn't
/// happen in a real browser, but we handle SSR / worker contexts).
fn perf_now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or_else(js_sys::Date::now)
}

/// GPU-side state — populated once async device acquisition resolves.
struct GpuState {
    renderer: WgpuRenderer,
    sprite_batch: WgpuSpriteBatch,
    shape_batch: WgpuShapeBatch,
    post_process: WgpuPostProcessStack,
    light_pass: WgpuLightingPass,
    /// Each view's own world buffers (`ecs::views`), by target id.
    view_targets: std::collections::HashMap<String, jengine_core::ecs::wgpu_systems::ViewTargets>,
    /// Forward pass for `lit_sprite` materials, and the 1x1 flat normal a
    /// material without `normal_map_id` shades against.
    ///
    /// The regular sprite pass skips every `lit_sprite` entity on the
    /// understanding that this pass draws it. Until Sep 2026 only the native
    /// runtime had one, so in the editor those sprites were simply never
    /// drawn - scene 12's two rows were absent, and what read as a subtle
    /// normal-map effect was the point lights glowing on an empty background.
    lit_sprite_batch: jengine_core::renderer::backend::wgpu::WgpuLitSpritePipeline,
    flat_normal: jengine_core::renderer::backend::wgpu::WgpuTexture,
    /// Slice 13d — cache of compiled graph-driven pipelines, keyed by
    /// `(shader_graph_path, blend_mode)`. Populated lazily by the
    /// scene flush closure when a `MaterialComponent` references a
    /// `.material` whose `shader_graph_path` is in
    /// `ViewportApp::pending_shader_graphs`.
    graph_pipelines: GraphPipelineCache,
}

thread_local! {
    static APP: RefCell<Option<Rc<RefCell<ViewportApp>>>> = const { RefCell::new(None) };
}

fn with_app<R>(f: impl FnOnce(&mut ViewportApp) -> R) -> Option<R> {
    APP.with(|cell| {
        let outer = cell.borrow();
        let app_rc = outer.as_ref()?.clone();
        drop(outer);
        let mut app = app_rc.borrow_mut();
        Some(f(&mut app))
    })
}

fn get_canvas(id: &str) -> Result<HtmlCanvasElement, JsValue> {
    let document = web_sys::window()
        .ok_or_else(|| JsValue::from_str("no window"))?
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let el = document
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("no #{id} element")))?;
    el.dyn_into::<HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str(&format!("#{id} is not a <canvas>")))
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// A project package's registration step, installed by a generated per-project
/// viewport wrapper.
///
/// A plain `fn` pointer rather than a closure so the list is `Copy` and needs
/// no allocation — a generated wrapper only ever passes a non-capturing
/// function.
pub type ProjectPackageAdder = fn(&mut jengine_core::package::host::PackageHost);

thread_local! {
    /// Packages a wrapper crate registered before `viewport_init` ran.
    ///
    /// The editor viewport is one prebuilt WASM module shared by every
    /// project, so a project's own Rust package cannot be linked into it
    /// ahead of time. `jengine-builder` generates a wrapper crate that depends
    /// on this one plus the project's packages and calls
    /// [`register_project_package`] from its `#[wasm_bindgen(start)]`, which
    /// runs during module init — before the editor calls `viewport_init`.
    ///
    /// Read rather than drained, so a second `viewport_init` still gets them.
    static PROJECT_PACKAGES: RefCell<Vec<ProjectPackageAdder>> = const { RefCell::new(Vec::new()) };
}

/// Register a project package to be added to the viewport's `PackageHost`.
///
/// Must be called before `viewport_init`; a generated wrapper does this from
/// its module-start function. Calling it later logs and is otherwise ignored,
/// because the host has already been initialized against a registry the
/// package is missing from.
pub fn register_project_package(adder: ProjectPackageAdder) {
    PROJECT_PACKAGES.with(|p| p.borrow_mut().push(adder));
}

/// Publish a Rust panic somewhere the *editor* can see it.
///
/// `console_error_panic_hook` prints to the browser console, which in a Tauri
/// window means devtools — so a panic in here looked like "the editor crashed"
/// with nothing to go on. Worse, wasm has no unwinding: a panic traps the
/// module, every later call throws, and the render loop just stops. Anything
/// that tried to report the panic *through* wasm would be reporting from a
/// corpse.
///
/// So the hook writes the message straight onto `window`, in plain JS, where it
/// survives the module dying. `Viewport.svelte` reads it and puts it in the
/// Console panel.
fn install_panic_reporter() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let message = format!("{info}");
            if let Some(window) = web_sys::window() {
                // Only the first panic: later ones are usually the module
                // being poked after it already trapped, and they would bury
                // the one that mattered.
                let key = JsValue::from_str("__jengine_viewport_panic");
                let already = js_sys::Reflect::get(&window, &key)
                    .map(|v| !v.is_undefined() && !v.is_null())
                    .unwrap_or(false);
                if !already {
                    let _ = js_sys::Reflect::set(&window, &key, &JsValue::from_str(&message));
                }
            }
            previous(info);
        }));
    });
}

#[wasm_bindgen]
pub fn viewport_init(canvas_id: &str) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    install_panic_reporter();
    // Set a level and installed no logger until Oct 2026, so the engine's
    // log lines went nowhere in the editor.
    jengine_core::web_log::init(log::LevelFilter::Info);

    let canvas = get_canvas(canvas_id)?;
    let w = canvas.width().max(1);
    let h = canvas.height().max(1);

    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);

    // Packages register into the same registry the scene loader uses, before
    // any scene is parsed. `jengine-audio` is absent by design: it enables
    // jengine-core's kira-backed `audio` feature, which is native-only, and
    // the viewport plays sound through `web_audio` instead.
    let mut package_host = jengine_core::package::host::PackageHost::new();
    package_host.add_package(Box::new(jengine_physics::PhysicsPackage));
    package_host.add_package(Box::new(jengine_particles::ParticlesPackage));
    package_host.add_package(Box::new(jengine_parallax::ParallaxPackage));
    package_host.add_package(Box::new(jengine_swarm::SwarmPackage));
    // Then the project's own, from a generated wrapper if one is in use.
    PROJECT_PACKAGES.with(|p| {
        let adders = p.borrow();
        if !adders.is_empty() {
            log::info!("[viewport] {} project package(s) to register", adders.len());
        }
        for add in adders.iter() {
            add(&mut package_host);
        }
    });
    package_host.initialize(&mut registry);
    // Package script globals have to exist in the browser preamble before any
    // script compiles, or they are simply undefined - the gap that made a
    // package's script API native-only.
    jengine_core::scripting::bridge::adapters::js::set_package_script_globals(
        package_host.script_commands().browser_globals_js(),
    );
    if !package_host.script_commands().is_empty() {
        log::info!(
            "[viewport] {} package script command(s): {}",
            package_host.script_commands().len(),
            package_host.script_commands().names().join(", ")
        );
    }
    // The Fengari bridge installs globals from an allowlist that cannot know
    // about packages, so the names go onto `__jengine_play` for it to read.
    // With them, every script namespace - core's and the packages' - which
    // `lua-runtime.ts` builds its `Sprite.set_color` tables from.
    publish_package_api_names(package_host.script_commands());
    // Published process-wide: dispatch happens while `app` is borrowed as the
    // world host, so it could not also lend a registry it owned.
    jengine_core::package::script_commands::install_global(
        package_host.take_script_commands(),
    );

    let (font_rgba, font_w, font_h, font_data) = built_in_font_atlas();
    let mut assets = AssetManager::new();
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

    let app = ViewportApp {
        gpu: None,
        canvas: canvas.clone(),
        canvas_size: UVec2::new(w, h),
        ui_scale: jengine_core::ui::scaling::UiScale::default(),
        safe_fraction: [0.0; 4],
        pre_play_post_process: None,
        pre_play_scene_name: String::new(),
        init_dispatched: false,
        world: World::new(),
        scene_entities: Vec::new(),
        authored_entity_count: 0,
        registry,
        assets,
        package_host,
        prefabs: std::collections::HashMap::new(),
        raw_prefabs: std::collections::HashMap::new(),
        display: Default::default(),
        text_proxy: Default::default(),
        missing_warned: HashSet::new(),
        texture_requested: HashSet::new(),
        material_wanted: std::collections::BTreeSet::new(),
        material_requested: HashSet::new(),
        font_data,
        camera: Camera2D::new(w as f32, h as f32),
        prev_camera: glam::Vec2::ZERO,
        clear_color: [0.05, 0.05, 0.08, 1.0],
        selected: -1,
        game_viewport: None,
        show_lighting: true,
        particle_system: ParticleSystem::new(),
        log_buffer: Vec::new(),
        save_system: jengine_core::save::SaveSystem::new(),
        mixer: jengine_core::audio::AudioMixer::new(),
        fade_active: None,
        fade_overlay: None,
        nav_snapshot: std::rc::Rc::new(std::cell::RefCell::new(
            jengine_core::scripting::bridge::adapters::js::nav::NavSnapshot::new(),
        )),
        ray_snapshot: std::rc::Rc::new(std::cell::RefCell::new(
            jengine_core::physics::ray::RaySnapshot::new(),
        )),
        pending_behaviors: HashMap::new(),
        pending_shader_graphs: HashMap::new(),
        start_time_ms: perf_now_ms(),
        playing: false,
        paused: false,
        step_pending: false,
        input: InputState::new(),
        touch: jengine_core::input::touch_controls::TouchControlsState::new(),
        last_tick_ms: None,
        refresh_estimate: jengine_core::time::RefreshEstimate::new(),
        pacer: jengine_core::time::FramePacer::new(),
        trail_dt: 0.0,
        play_snapshot: None,
        scene_stack: Vec::new(),
        last_scripts_json: String::new(),
        behavior_system: BehaviorGraphSystem::new(),
        physics: None,
        physics_config: None,
        path_library: None,
        gui_scroll: Default::default(),
        pip_request: None,
        frame_history: VecDeque::with_capacity(128),
        frame_stats: FrameStats::default(),
        script_fns: HashMap::new(),
        script_cache: HashMap::new(),
        debug_draws: Vec::new(),
        physics_debug: false,
        pending_camera_shakes: Vec::new(),
        play_namespace_ready: false,
        tween_manager: jengine_core::animation::tween::TweenManager::new(),
        video: editor_video_system(),
        gui_click_callbacks: HashMap::new(),
        audio_warned: std::collections::HashSet::new(),
        time_scale: 1.0,
        real_dt: 0.0,
        pending_script_loads: Vec::new(),
        web_audio: WebAudioBackend::new(),
        spatial_audio: jengine_core::audio::spatial::SpatialAudio::new(),
        spawn_notified: std::collections::HashSet::new(),
    };

    let app_rc = Rc::new(RefCell::new(app));
    APP.with(|cell| *cell.borrow_mut() = Some(app_rc.clone()));

    // Stage 2.1 — install canvas + document input listeners now so
    // anything that flips `playing` later (toolbar, hot-reload) gets
    // a live `InputState` without further plumbing. Listeners stay
    // installed in edit mode too — `tick_world` is gated on `playing`
    // so the captured state simply isn't read.
    if let Err(e) = install_input_listeners(&canvas, app_rc.clone()) {
        console_warn!("viewport: input listener install failed: {:?}", e);
    }

    spawn_gpu_init(app_rc);

    Ok(())
}

fn spawn_gpu_init(app_rc: Rc<RefCell<ViewportApp>>) {
    {
        let mut app = app_rc.borrow_mut();
        if app.init_dispatched || app.gpu.is_some() {
            return;
        }
        app.init_dispatched = true;
    }
    let canvas = app_rc.borrow().canvas.clone();
    let size = app_rc.borrow().canvas_size;

    wasm_bindgen_futures::spawn_local(async move {
        match WgpuRenderer::new_from_canvas_async(canvas, size).await {
            Ok(mut renderer) => {
                // The world is drawn in `scene_format` (float where the
                // adapter has one); the batch switches to `color_format` for
                // the UI, drawn onto the display after the post chain.
                let sb = match WgpuSpriteBatch::new(&renderer.device, renderer.scene_format) {
                    Ok(b) => b,
                    Err(e) => {
                        console_error!("WgpuSpriteBatch init failed: {e}");
                        return;
                    }
                };
                let pp = match WgpuPostProcessStack::new(
                    &renderer.device,
                    size.x.max(1),
                    size.y.max(1),
                    renderer.scene_format,
                    renderer.color_format,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        console_error!("WgpuPostProcessStack init failed: {e}");
                        return;
                    }
                };
                let lp = match WgpuLightingPass::new(
                    &renderer.device,
                    size.x.max(1),
                    size.y.max(1),
                    renderer.scene_format,
                ) {
                    Ok(l) => l,
                    Err(e) => {
                        console_error!("WgpuLightingPass init failed: {e}");
                        return;
                    }
                };
                let lit = match jengine_core::renderer::backend::wgpu::WgpuLitSpritePipeline::new(
                    &renderer.device,
                    renderer.scene_format,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        console_error!("WgpuLitSpritePipeline init failed: {e}");
                        return;
                    }
                };
                let flat_normal =
                    jengine_core::renderer::backend::wgpu::WgpuLitSpritePipeline::flat_normal_texture(
                        &renderer.device,
                        &renderer.queue,
                    );
                let cc = app_rc.borrow().clear_color;
                renderer.set_clear_color(cc[0], cc[1], cc[2], cc[3]);

                // The editor grid is drawn in the world pass.
                let shb = match WgpuShapeBatch::new(&renderer.device, renderer.scene_format) {
                    Ok(b) => b,
                    Err(e) => {
                        console_error!("WgpuShapeBatch init failed: {e}");
                        return;
                    }
                };

                app_rc.borrow_mut().gpu = Some(GpuState {
                    renderer,
                    sprite_batch: sb,
                    shape_batch: shb,
                    post_process: pp,
                    light_pass: lp,
                    view_targets: Default::default(),
                    lit_sprite_batch: lit,
                    flat_normal,
                    graph_pipelines: GraphPipelineCache::new(),
                });
                console_log!("viewport: WebGPU device ready");
            }
            Err(e) => {
                console_error!(
                    "viewport: WebGPU init failed: {e}. \
                    Browser must be Chrome/Edge 113+, Safari 18+, or Firefox Nightly."
                );
            }
        }
    });
}

#[wasm_bindgen]
pub fn viewport_resize(width: u32, height: u32) {
    with_app(|app| {
        let size = UVec2::new(width.max(1), height.max(1));
        app.canvas_size = size;
        // Scripts need this: get_screen_size() reads input.screen_size, and
        // without it the editor reported (0, 0) while the GUI laid itself out
        // against canvas_size — so any script hit-testing against GUI rects was
        // working in a different space than the one being drawn.
        app.input.screen_size = (size.x as f32, size.y as f32);
        app.camera.resize(size.x as f32, size.y as f32);
        if let Some(gpu) = app.gpu.as_mut() {
            gpu.renderer.resize(size);
            // Resize the post and lighting targets in place. This used to
            // build new stacks, which threw away the scene's post settings:
            // resizing the editor's Game view turned its bloom off.
            gpu.post_process.resize(&gpu.renderer.device, size.x, size.y);
            gpu.light_pass.resize(&gpu.renderer.device, size.x, size.y);
        }
    });
}

#[wasm_bindgen]
pub fn viewport_set_camera(x: f32, y: f32, zoom: f32) {
    with_app(|app| {
        app.camera.position = Vec2::new(x, y);
        app.camera.zoom = zoom.max(0.0001);
    });
}

#[wasm_bindgen]
pub fn viewport_set_camera_size(width: f32, height: f32) {
    with_app(|app| {
        if width > 0.0 && height > 0.0 {
            app.game_viewport = Some([width, height]);
        }
    });
}

/// Tell the viewport what size the project's UI was authored for, and
/// whether to scale it.
///
/// `mode` is `"ScaleWithScreen"`, `"ScaleAndCenter"` or
/// `"ConstantPixelSize"`; anything else is read as the first, because a typo
/// in a settings field should not silently turn a feature off. Both UI systems and their pointer hit tests use it, so
/// a click keeps landing where the widget is drawn.
#[wasm_bindgen]
pub fn viewport_set_ui_scale(reference_w: f32, reference_h: f32, mode: &str) {
    use jengine_core::ui::scaling::{UiScale, UiScaleMode};
    with_app(|app| {
        let mode = UiScaleMode::from_setting(Some(mode));
        let reference = if reference_w > 0.0 && reference_h > 0.0 {
            glam::Vec2::new(reference_w, reference_h)
        } else {
            UiScale::default().reference
        };
        // The safe insets are the host's, not the project's: keep them.
        app.ui_scale = UiScale { reference, mode, safe: app.ui_scale.safe };
    });
}

/// Tell the viewport how the project's textures are sampled: the project's
/// whole `settings` object as JSON, of which its `texture_filter` is read
/// (`jengine_core::renderer::texture_filter`). A change re-uploads every
/// texture the next time it is drawn.
#[wasm_bindgen]
pub fn viewport_set_texture_filters(settings_json: &str) {
    use jengine_core::renderer::texture_filter::TextureFilterRules;
    let settings = serde_json::from_str::<serde_json::Value>(settings_json).ok();
    let rules = TextureFilterRules::from_settings(settings.as_ref());
    with_app(|app| app.assets.set_texture_filters(rules));
}

/// The project's asset packs (`settings.asset_packs`): the project's whole
/// `settings` object as JSON. The editor has every pack's files on disk, so
/// each declared pack is local and `installed` - `Packs.download` has
/// nothing to fetch here, and a game's download flow is tried in a build
/// (`jengine_core::asset::packs`).
#[wasm_bindgen]
pub fn viewport_set_asset_packs(settings_json: &str) {
    let settings = serde_json::from_str::<serde_json::Value>(settings_json).unwrap_or_default();
    let packs = jengine_core::asset::packs::PackSettings::from_settings(&settings);
    jengine_core::asset::packs::configure(None, &packs);
}

/// Live ops for Play (`settings.liveops`, `jengine_core::liveops`): the
/// project's whole `settings` object as JSON. Play asks the service for the
/// editor's channel (`editor_channel`, `dev` by default) as platform
/// `editor`, and a content release is not applied - the editor's packs are
/// its project's own files. Called as each play session starts, so each
/// starts with a fetch.
#[wasm_bindgen]
pub fn viewport_set_liveops(settings_json: &str) {
    let settings = serde_json::from_str::<serde_json::Value>(settings_json).unwrap_or_default();
    jengine_core::liveops::web::init(&settings, true);
}

/// Set the mixer to the project's layout (`settings.audio_mixer`): the
/// project's whole `settings` object as JSON. The editor calls it when the
/// project's settings change and as each play session starts, which also
/// puts back whatever the last session's scripts changed.
#[wasm_bindgen]
pub fn viewport_set_audio_mixer(settings_json: &str) {
    let Ok(settings) = serde_json::from_str::<serde_json::Value>(settings_json) else { return };
    let layout = jengine_core::audio::MixerLayout::from_settings(&settings).unwrap_or_default();
    with_app(|app| app.mixer.apply_layout(&layout));
}

#[wasm_bindgen]
pub fn viewport_clear_game_viewport() {
    with_app(|app| {
        app.game_viewport = None;
    });
}

#[wasm_bindgen]
pub fn viewport_set_clear_color(r: f32, g: f32, b: f32, a: f32) {
    with_app(|app| {
        app.clear_color = [r, g, b, a];
        if let Some(gpu) = app.gpu.as_mut() {
            gpu.renderer.set_clear_color(r, g, b, a);
        }
    });
}

thread_local! {
    /// The editor grid's lines and the two origin axes, linear RGBA. The
    /// editor hands its palette over (`viewport_set_grid_colors`); these are
    /// what a build drew before it could.
    static GRID_COLORS: Cell<[[f32; 4]; 3]> = const {
        Cell::new([
            [0.30, 0.30, 0.42, 0.55],
            [0.80, 0.28, 0.28, 0.80],
            [0.28, 0.80, 0.40, 0.80],
        ])
    };
}

/// The scene view's grid in the editor's palette: twelve linear floats -
/// grid lines, the X axis, the Y axis, each RGBA. Anything else is ignored.
#[wasm_bindgen]
pub fn viewport_set_grid_colors(colors: Vec<f32>) {
    if colors.len() != 12 {
        return;
    }
    let c = |i: usize| [colors[i], colors[i + 1], colors[i + 2], colors[i + 3]];
    GRID_COLORS.with(|g| g.set([c(0), c(4), c(8)]));
}

#[wasm_bindgen]
pub fn viewport_set_selected(index: i32) {
    with_app(|app| app.selected = index);
}

#[wasm_bindgen]
pub fn viewport_set_show_lighting(enabled: bool) {
    with_app(|app| app.show_lighting = enabled);
}

// ---------------------------------------------------------------------------
// Asset loading
// ---------------------------------------------------------------------------

/// Texture ids the renderer found missing that the editor should load, as a
/// JSON array - each once. The viewport has no filesystem, so it asks: the
/// editor reads each file and answers with `viewport_load_texture`. This is
/// how a sprite whose texture no scene named (a package's own sprites, a
/// texture swapped in at runtime) gets drawn.
#[wasm_bindgen]
pub fn viewport_take_texture_requests() -> String {
    with_app(|app| {
        let ids = jengine_core::asset::textures_to_request(
            &app.missing_warned,
            &mut app.texture_requested,
            &app.assets,
        );
        serde_json::to_string(&ids).unwrap_or_else(|_| "[]".to_string())
    })
    .unwrap_or_else(|| "[]".to_string())
}

/// Material ids the editor should load, as a JSON array - each once: every
/// prefab's (queued by `viewport_load_prefab`) and any a draw asked for and
/// did not have. The editor answers with `viewport_load_material` and the
/// material's `viewport_load_shader_graph`. It used to push only what scene
/// entities named, so a graph material only a spawned prefab used drew with
/// the default shader - Spellcraft's shockwaves, a graph over a white quad,
/// flashed as white boxes.
#[wasm_bindgen]
pub fn viewport_take_material_requests() -> String {
    with_app(|app| {
        let noted = app.missing_warned.iter().filter_map(|w| w.strip_prefix(MATERIAL_REQUEST));
        let ids: Vec<String> = app
            .material_wanted
            .iter()
            .map(String::as_str)
            .chain(noted)
            .filter(|id| app.assets.get_material(id).is_none() && !app.material_requested.contains(*id))
            .map(str::to_string)
            .collect::<std::collections::BTreeSet<String>>()
            .into_iter()
            .collect();
        app.material_requested.extend(ids.iter().cloned());
        serde_json::to_string(&ids).unwrap_or_else(|_| "[]".to_string())
    })
    .unwrap_or_else(|| "[]".to_string())
}

thread_local! {
    /// Each video's Blob URL, made from the bytes the editor handed over.
    static VIDEO_URLS: std::cell::RefCell<std::collections::HashMap<String, String>> = Default::default();
    /// Videos a decoder is waiting for, not yet asked of the editor.
    static VIDEO_WANTED: std::cell::RefCell<std::collections::BTreeSet<String>> = Default::default();
    /// Videos already asked for, so each is asked once.
    static VIDEO_ASKED: std::cell::RefCell<std::collections::BTreeSet<String>> = Default::default();
}

/// The editor's videos: a `<video>` element each, whose URL is the Blob the
/// editor hands over once it has read the file - asked for through
/// `viewport_take_video_requests`, as textures are.
fn editor_video_system() -> jengine_core::video::VideoSystem {
    jengine_core::video::VideoSystem::new(Box::new(|source: &str| {
        let source = source.to_string();
        VIDEO_WANTED.with(|w| w.borrow_mut().insert(source.clone()));
        let resolve = move || VIDEO_URLS.with(|m| m.borrow().get(&source).cloned());
        Some(Box::new(jengine_core::video::web::WebVideoDecoder::new(Box::new(resolve)))
            as Box<dyn jengine_core::video::VideoDecoder>)
    }))
}

/// Video files the editor should read and hand over with
/// `viewport_load_video`, as a JSON array - each once.
#[wasm_bindgen]
pub fn viewport_take_video_requests() -> String {
    let wanted: Vec<String> = VIDEO_WANTED.with(|w| std::mem::take(&mut *w.borrow_mut()).into_iter().collect());
    let fresh: Vec<String> = VIDEO_ASKED.with(|a| {
        let mut a = a.borrow_mut();
        wanted.into_iter().filter(|s| a.insert(s.clone())).collect()
    });
    serde_json::to_string(&fresh).unwrap_or_else(|_| "[]".to_string())
}

/// A video file's bytes, read by the editor: kept as a Blob URL for the
/// `<video>` element that plays it.
#[wasm_bindgen]
pub fn viewport_load_video(source: &str, data: &[u8]) -> Result<(), JsValue> {
    let kind = if source.ends_with(".webm") { "video/webm" } else { "video/mp4" };
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(data));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(kind);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    if let Some(old) = VIDEO_URLS.with(|m| m.borrow_mut().insert(source.to_string(), url)) {
        let _ = web_sys::Url::revoke_object_url(&old);
    }
    Ok(())
}

#[wasm_bindgen]
pub fn viewport_load_texture(texture_id: &str, data: &[u8]) -> Result<(), JsValue> {
    with_app(|app| match RawTextureData::from_bytes(data) {
        Ok(raw) => {
            // Force re-upload by dropping any prior wgpu texture for this id.
            // (AssetManager re-uploads lazily on next render via ensure_wgpu_texture.)
            app.assets.insert_texture_bytes(texture_id.to_string(), raw);
            app.missing_warned.remove(texture_id);
            Ok(())
        }
        Err(e) => Err(JsValue::from_str(&format!(
            "decode '{texture_id}': {e}"
        ))),
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

/// Slice 8 — push audio bytes (WAV/OGG/MP3) into the viewport's Web
/// Audio backend. `clip_id` is the asset path the script will pass
/// to `play_sound` / `play_music`. Decoding is async; the clip is
/// playable once `AudioContext.decodeAudioData` resolves (typically
/// a few ms after this call returns).
#[wasm_bindgen]
pub fn viewport_load_audio(clip_id: &str, data: &[u8]) -> Result<(), JsValue> {
    // A clip that now exists should be able to report a future failure again.
    let _ = with_app(|app| app.audio_warned.remove(clip_id));
    with_app(|app| match app.web_audio.load_clip(clip_id, data) {
        Ok(()) => Ok(()),
        Err(e) => {
            app.log_buffer
                .push(format!("[Audio] load '{clip_id}' failed: {e}"));
            Err(JsValue::from_str(&e))
        }
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

/// Seed the play namespace's data tables for `get_data`.
///
/// `tables_json` is an object of `{ "<table id>": { "rows": [ ... ] } }`,
/// where the id is the assets-relative path without the `.data` extension —
/// the same shape the web runtime builds from the builder's table list, and
/// the same one the browser preamble's `get_data` indexes into.
///
/// Native walks `assets/` itself (`DataRegistry::load_from_assets_dir`) and
/// the web build gets a list baked in by the builder; the editor viewport has
/// no filesystem, so the editor reads the files and pushes them here. Without
/// it `get_data` silently returned nil in the editor only.
/// Register a `.skelanim` clip so `SkeletalPlayer` entities can play it.
///
/// `clip_id` is the assets-relative path (`animations/walk.skelanim`), the id
/// a scene's `SkeletalPlayer.clip` names. Native walks `assets/` for these
/// itself; the viewport has no filesystem, so the editor pushes them.
#[wasm_bindgen]
pub fn viewport_load_skeletal_clip(clip_id: &str, json: &str) -> Result<(), JsValue> {
    let clip = serde_json::from_str::<jengine_core::animation::skeletal::SkeletalClip>(json)
        .map_err(|e| JsValue::from_str(&format!("parse skeletal clip '{clip_id}': {e}")))?;
    with_app(|app| {
        app.assets.insert_skeletal_clip(clip_id.to_string(), clip);
    })
    .ok_or_else(|| JsValue::from_str("viewport not initialised"))
}

/// Register a `.rig` (a `SkeletonRig` as JSON) for `Skeleton.spawn_rig`.
///
/// `rig_id` is the assets-relative path (`rigs/arm.rig`), the id a script
/// names. The editor pushes every one in the project, as it does clips; until
/// Oct 2026 no runtime loaded rigs at all.
#[wasm_bindgen]
pub fn viewport_load_skeleton_rig(rig_id: &str, json: &str) -> Result<(), JsValue> {
    let rig = serde_json::from_str::<jengine_core::animation::rig::SkeletonRig>(json)
        .map_err(|e| JsValue::from_str(&format!("parse skeleton rig '{rig_id}': {e}")))?;
    with_app(|app| {
        app.assets.insert_skeleton_rig(rig_id.to_string(), rig);
    })
    .ok_or_else(|| JsValue::from_str("viewport not initialised"))
}

#[wasm_bindgen]
pub fn viewport_seed_data_tables(tables_json: &str) -> Result<(), JsValue> {
    let play = play_namespace()
        .ok_or_else(|| JsValue::from_str("play namespace not ready"))?;
    let parsed = js_sys::JSON::parse(tables_json)
        .map_err(|_| JsValue::from_str("data tables JSON did not parse"))?;
    js_sys::Reflect::set(&play, &JsValue::from_str("data_tables"), &parsed)?;
    // And for packages, which read them from Rust (`data::global`).
    match jengine_core::data::DataRegistry::from_tables_json(tables_json) {
        Ok(registry) => jengine_core::data::install_global(std::sync::Arc::new(registry)),
        Err(e) => log::warn!("[viewport] data tables for packages did not parse: {e}"),
    }
    Ok(())
}

#[wasm_bindgen]
pub fn viewport_load_gui(gui_id: &str, json: &str) -> Result<(), JsValue> {
    with_app(|app| match load_gui(json) {
        Ok(doc) => {
            app.assets.insert_gui(gui_id.to_string(), doc);
            Ok(())
        }
        Err(e) => Err(JsValue::from_str(&format!("parse gui '{gui_id}': {e}"))),
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

/// Push a runtime sprite atlas into the viewport's asset cache so
/// scripts can call `set_sprite_region(entity, atlas_id, region)` in
/// play mode. The native runtime resolves a scene's top-level
/// `atlas_refs` from disk; the WASM viewport has no filesystem, so the
/// editor reads each `.atlas` file and forwards it here. `atlas_id` is
/// the sibling-PNG path (e.g. `atlases/dungeon.png`) — the same id the
/// PNG is uploaded under via `viewport_load_texture` and the same id
/// scripts pass to `set_sprite_region`. `json` is the raw `.atlas`
/// descriptor (`{ width, height, regions: [...] }`).
#[wasm_bindgen]
pub fn viewport_load_atlas(atlas_id: &str, json: &str) -> Result<(), JsValue> {
    use jengine_core::renderer::atlas::AtlasData;
    with_app(|app| match serde_json::from_str::<AtlasData>(json) {
        Ok(data) => {
            app.assets.register_atlas(atlas_id.to_string(), data);
            Ok(())
        }
        Err(e) => Err(JsValue::from_str(&format!("parse atlas '{atlas_id}': {e}"))),
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

/// Push a bitmap font into the viewport so `TextComponent.font_id` can name
/// it. The native runtime scans the world for referenced fonts and reads them
/// off disk; the WASM viewport has no filesystem, so the editor reads the
/// `.font` file and forwards it here.
///
/// `font_id` is the `.font` asset path exactly as a `font_id` spells it (e.g.
/// `fonts/ui.font`). The atlas image must be pushed separately via
/// `viewport_load_texture` under the **sibling id** - `fonts/ui.png` - which
/// is what `font_asset::font_texture_id` derives and what the render pass
/// binds. Pushing the descriptor without its image draws every glyph from
/// whatever texture happened to be bound, so the editor sends both.
/// Publish package script-global names on `window.__jengine_play`.
///
/// `lua-runtime.ts` reads these and installs them alongside its hardcoded
/// `BRIDGE_FUNCTIONS` allowlist. Without it a package global exists in the
/// browser preamble but is never bound into Lua, which looks exactly like the
/// function not existing.
fn publish_package_api_names(
    commands: &jengine_core::package::script_commands::PackageCommandRegistry,
) {
    let names = commands.names();
    let Some(win) = web_sys::window() else {
        return;
    };
    let key = JsValue::from_str("__jengine_play");
    let play = match js_sys::Reflect::get(&win, &key) {
        Ok(v) if v.is_object() => v,
        _ => {
            let obj = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&win, &key, &obj);
            obj.into()
        }
    };
    let arr = js_sys::Array::new();
    for name in &names {
        arr.push(&JsValue::from_str(name));
    }
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("__package_api_names"), &arr);
    jengine_core::scripting::namespaces::publish(&play, commands.commands());
}

#[wasm_bindgen]
pub fn viewport_load_font(font_id: &str, json: &str) -> Result<(), JsValue> {
    with_app(|app| {
        jengine_core::renderer::font_asset::insert_font_json(&mut app.assets, font_id, json)
            .map_err(|e| JsValue::from_str(&format!("{e:#}")))
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

/// Push a TTF / OTF so a `font_id` naming it draws in it
/// (`renderer::dynamic_font`): the bytes are the font, and its atlas is made
/// here as text asks for glyphs - there is no image to send.
#[wasm_bindgen]
pub fn viewport_load_font_file(font_id: &str, bytes: &[u8]) -> Result<(), JsValue> {
    with_app(|app| {
        jengine_core::renderer::dynamic_font::insert_font_file(&mut app.assets, font_id, bytes)
            .map_err(|e| JsValue::from_str(&format!("font '{font_id}': {e}")))
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

/// The texture id a given `.font` path's atlas image must be uploaded under.
///
/// Exposed so the editor derives the pairing with the same code the renderer
/// resolves it with, rather than reimplementing "swap the extension" in
/// TypeScript where it could drift.
#[wasm_bindgen]
pub fn viewport_font_texture_id(font_id: &str) -> String {
    jengine_core::renderer::font_asset::font_texture_id(font_id)
}

#[wasm_bindgen]
pub fn viewport_load_behavior(graph_path: &str, json: &str) -> Result<(), JsValue> {
    with_app(|app| {
        app.pending_behaviors
            .insert(graph_path.to_string(), json.to_string());
        Ok(())
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

/// Slice 13d — push a `.material` JSON into the viewport's `AssetManager`.
/// Mirrors the native `AssetManager::load_material` (which reads from
/// disk) for the WASM path where the editor is the source of truth. Must
/// be called before play mode so `MaterialComponent.material_id` lookups
/// during render route through `flush_with_pipeline`.
#[wasm_bindgen]
pub fn viewport_load_material(material_id: &str, json: &str) -> Result<(), JsValue> {
    with_app(|app| {
        app.assets
            .load_material_from_str(material_id, json)
            .map_err(|e| JsValue::from_str(&format!("parse material '{material_id}': {e:#}")))
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

/// Sort layers 1.4 — push a `.sortlayers` JSON into the viewport's
/// `AssetManager` under the given id (typically the project-relative
/// path, e.g. `"sortlayers/main.sortlayers"`). The active-set is
/// driven separately from the scene's `sort_layers` field via
/// `viewport_set_active_sort_layers` (or implicitly by
/// `viewport_update_scene` if the scene JSON carries the field).
#[wasm_bindgen]
pub fn viewport_load_sort_layers(id: &str, json: &str) -> Result<(), JsValue> {
    with_app(|app| {
        app.assets
            .load_sort_layers_from_str(id, json)
            .map_err(|e| JsValue::from_str(&format!("parse sort_layers '{id}': {e:#}")))
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

/// Sort layers 1.4 — set (or clear with `None`) the active sort_layers
/// asset id used by all render fns this frame. Mirrors the scene-level
/// `sort_layers` field — call this whenever the scene's selection
/// changes (or when the asset content is hot-reloaded with the same id).
#[wasm_bindgen]
pub fn viewport_set_active_sort_layers(id: Option<String>) {
    with_app(|app| {
        app.assets.set_active_sort_layers(id);
    });
}

/// Slice 13d — push a `.shader` graph JSON into the viewport's pending
/// cache. The first time a sprite flush sees a material whose
/// `shader_graph_path` matches, `GraphPipelineCache::insert_from_graph`
/// compiles a wgpu pipeline against this graph and caches it.
#[wasm_bindgen]
pub fn viewport_load_shader_graph(graph_path: &str, json: &str) -> Result<(), JsValue> {
    let graph: ShaderGraph = serde_json::from_str(json)
        .map_err(|e| JsValue::from_str(&format!("parse shader graph '{graph_path}': {e}")))?;
    with_app(|app| {
        app.pending_shader_graphs
            .insert(graph_path.to_string(), graph);
        Ok(())
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

/// Editor-side prefab installer. The native runtime loads
/// `assets/prefabs/*.prefab` files itself at startup; the WASM viewport
/// has no filesystem, so the editor frontend pulls the prefab JSON via
/// the `list_prefabs` Tauri command and forwards each one here. The
/// JSON is parsed and stashed on `window.__jengine_play.prefabs[name]`
/// so the browser bridge's `spawn_prefab(name)` finds the data.
#[wasm_bindgen]
pub fn viewport_load_prefab(name: &str, json: &str) -> Result<(), JsValue> {
    let parsed = js_sys::JSON::parse(json)
        .map_err(|e| JsValue::from_str(&format!("parse prefab '{name}': {e:?}")))?;
    let play = play_namespace()
        .ok_or_else(|| JsValue::from_str("no window for prefab install"))?;
    let prefabs_key = JsValue::from_str("prefabs");
    let prefabs = match js_sys::Reflect::get(&play, &prefabs_key) {
        Ok(v) if v.is_object() => v.unchecked_into::<js_sys::Object>(),
        _ => {
            let obj = js_sys::Object::new();
            js_sys::Reflect::set(&play, &prefabs_key, &obj)
                .map_err(|e| JsValue::from_str(&format!("set prefabs: {e:?}")))?;
            obj
        }
    };
    js_sys::Reflect::set(&prefabs, &JsValue::from_str(name), &parsed)
        .map_err(|e| JsValue::from_str(&format!("set prefab '{name}': {e:?}")))?;
    // Mirror it into the Rust map so authored `prefab_instance` entries can be
    // expanded at scene load, not just script-driven `spawn_prefab` calls.
    match serde_json::from_str::<jengine_core::scene::format::EntityData>(json) {
        Ok(data) => {
            with_app(|app| {
                // Keyed by the name the editor pushed. Native keys by the
                // template's own `name` field, so a prefab whose inner name
                // differs from its filename is registered under both — a
                // scene may reference either, and one missing key means a
                // silently empty entity.
                if !data.name.is_empty() && data.name != name {
                    app.raw_prefabs.insert(data.name.clone(), data.clone());
                }
                app.material_wanted.extend(jengine_core::scene::format::prefab_material_ids([&data]));
                app.raw_prefabs.insert(name.to_string(), data);
                // Variants resolved against everything loaded so far; each
                // variant's merged components replace what the browser
                // bridge's `spawn_prefab` reads for it.
                app.prefabs = jengine_core::scene::resolve_prefab_variants(&app.raw_prefabs);
                // A prefab with parts too: its parts that are prefabs come expanded.
                for (key, data) in app.prefabs.iter().filter(|(_, d)| d.components.prefab_instance.is_some() || !d.children.is_empty()) {
                    if let Ok(text) = serde_json::to_string(data)
                        && let Ok(value) = js_sys::JSON::parse(&text)
                    {
                        let _ = js_sys::Reflect::set(&prefabs, &JsValue::from_str(key), &value);
                    }
                }
            });
        }
        Err(e) => {
            log::warn!("[viewport] prefab '{name}' is not an EntityData: {e}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Scene management
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn viewport_update_scene(json: &str) -> Result<(), JsValue> {
    with_app(|app| {
        let mut new_world = World::new();
        let json = &jengine_core::scene::expand_prefab_instances_in_json(
            json,
            &app.prefabs,
            "viewport",
        );
        load_scene_auto(json, &mut new_world, &app.registry)
            .map_err(|e| JsValue::from_str(&format!("parse scene: {e}")))?;
        // Apply scene-level sort_layers reference (the .sortlayers
        // asset itself is supplied via a separate IPC push from the
        // editor host; this just marks which id is active so render
        // code can resolve `sort_layer` strings to base_z offsets).
        let meta = jengine_core::scene::format::read_scene_metadata(json)
            .map_err(|e| JsValue::from_str(&format!("parse scene meta: {e}")))?;
        app.assets.set_active_sort_layers(meta.sort_layers);
        app.path_library = build_path_library(meta.paths);
        jengine_core::scene::set_current_name(&jengine_core::scene::name_of(json).unwrap_or_default());
        // Capture insertion order for index-based IPC addressing.
        // hecs doesn't preserve insertion order across iter(), but
        // load_scene_auto spawns entities sequentially; we re-derive
        // the order by sorting by entity id (which load_scene_auto
        // assigns monotonically).
        let mut ents: Vec<Entity> = new_world.iter().map(|e| e.entity()).collect();
        ents.sort_by_key(|e| e.id());
        app.scene_entities = ents;
        app.authored_entity_count = app.scene_entities.len();
        app.world = new_world;
        // Phase 8.2e — mirror runtime: replace the post-process effect
        // chain with whatever the new scene declares (cleared if none).
        let pp = jengine_core::renderer::postprocess::PostProcessSettings::from_scene_json(json).unwrap_or_default();
        if let Some(gpu) = app.gpu.as_mut() {
            gpu.post_process.set_settings(pp);
        }
        Ok(())
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

#[wasm_bindgen]
pub fn viewport_get_runtime_entity(index: usize) -> String {
    with_app(|app| {
        let Some(&entity) = app.scene_entities.get(index) else {
            return "null".to_string();
        };
        // Stage 2.4 — serialize the live world entity through the
        // component registry (same path `save_scene_v2` walks per
        // entity) plus any `CustomComponents` map so script-created
        // dynamic components survive the round trip. Returns an
        // `EntityDataV2`-shaped object so the inspector can diff it
        // against the on-disk scene format directly.
        let name = app
            .world
            .get::<&jengine_core::ecs::components::Name>(entity)
            .map(|n| n.0.clone())
            .unwrap_or_else(|_| format!("Entity_{}", entity.id()));
        let mut components = app.registry.serialize_entity(&app.world, entity);
        if let Ok(custom) = app
            .world
            .get::<&jengine_core::ecs::components::CustomComponents>(entity)
        {
            for (k, v) in &custom.0 {
                components.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        let mut entity_data = EntityDataV2::new(name, components);
        entity_data.parent = app
            .world
            .get::<&jengine_core::ecs::components::Parent>(entity)
            .ok()
            .and_then(|p| app.world.get::<&jengine_core::ecs::components::Name>(p.0).ok().map(|n| n.0.clone()));
        serde_json::to_string(&entity_data).unwrap_or_else(|_| "null".to_string())
    })
    .unwrap_or_else(|| "null".to_string())
}

#[wasm_bindgen]
pub fn viewport_set_runtime_entity(index: usize, json: &str) -> Result<(), JsValue> {
    // Stage 2.4 — apply an inspector edit to a live world entity.
    //
    // hecs has no "replace all components by name" primitive and the
    // registry's spawn/serialize hooks operate on `EntityBuilder`, not
    // on existing entities. Easiest correct path: despawn the old
    // entity, spawn a fresh one with the new component bundle, then
    // patch `scene_entities[index]` so subsequent IPC keeps resolving
    // the same logical entity. External references are by-name
    // (`EntityRef::Named("Hero")`), so the swap is invisible to
    // behaviors / follow targets / patrol paths.
    with_app(|app| {
        let Some(old_entity) = app.scene_entities.get(index).copied() else {
            return Err(JsValue::from_str(&format!(
                "viewport_set_runtime_entity: index {index} out of range"
            )));
        };
        let data: EntityDataV2 = serde_json::from_str(json)
            .map_err(|e| JsValue::from_str(&format!("parse entity: {e}")))?;

        let mut builder = hecs::EntityBuilder::new();
        builder.add(jengine_core::ecs::components::Name(data.name.clone()));
        let unknown = app
            .registry
            .deserialize_into_builder(&mut builder, &data.components);
        if !unknown.is_empty() {
            builder.add(jengine_core::ecs::components::CustomComponents(unknown));
        }

        // The edit replaces the entity, and a hierarchy points at entities:
        // keep the one it hung from and move its children onto the new one,
        // or an inspector edit to a parent strands everything under it.
        use jengine_core::ecs::components::Parent;
        if let Ok(p) = app.world.get::<&Parent>(old_entity).map(|p| p.0) {
            builder.add(Parent(p));
        }
        let children: Vec<hecs::Entity> = app
            .world
            .query::<&Parent>()
            .iter()
            .filter(|(_, p)| p.0 == old_entity)
            .map(|(e, _)| e)
            .collect();
        let _ = app.world.despawn(old_entity);
        let new_entity = app.world.spawn(builder.build());
        app.scene_entities[index] = new_entity;
        for c in children {
            let _ = app.world.insert_one(c, Parent(new_entity));
        }

        // Behavior-graph instances are entity-keyed; the old entity's
        // instance (if any) is now stale. Re-attach if the new bundle
        // still carries a `BehaviorGraphComponent` and we have its
        // JSON cached. Best-effort — failures log and continue.
        if let Ok(bg) = app
            .world
            .get::<&BehaviorGraphComponent>(new_entity)
            .map(|c| c.graph_path.clone())
        {
            if let Some(graph_json) = app.pending_behaviors.get(&bg).cloned() {
                if let Err(e) = app.behavior_system.attach_inline(
                    new_entity,
                    PathBuf::from(&bg),
                    &graph_json,
                ) {
                    app.log_buffer.push(format!(
                        "[viewport] behavior re-attach '{bg}' failed: {e:?}"
                    ));
                } else if app.playing {
                    app.behavior_system.dispatch_spawn(new_entity, &mut app.world);
                }
            }
        }
        Ok(())
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

// ---------------------------------------------------------------------------
// Play mode (Stage 2.1: snapshot + restore + tick_world; scripts/physics/
// behavior graphs land in Stage 2.2)
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn viewport_play(
    scripts_json: &str,
    actions_json: &str,
    gravity_x: f32,
    gravity_y: f32,
    length_unit: f32,
    physics_backend: &str,
    fixed_timestep: Option<f32>,
) -> Result<(), JsValue> {
    with_app(|app| {
        // Snapshot current scene → SceneDataV2 JSON so `viewport_stop`
        // can restore the pre-play state (and hand it back to the
        // editor for inspector refresh).
        // A fresh play session starts silent: if the previous one left music
        // running (or Stop was bypassed some other way) it would otherwise
        // play under the new session with no owner.
        app.web_audio.stop_all_oneshots();
        jengine_core::audio::music::reset();
        // Remember the chain the edited scene is showing, so Stop can put it
        // back. Play is free to change it - scene 14 does, from its script.
        app.pre_play_post_process = app
            .gpu
            .as_ref()
            .map(|gpu| gpu.post_process.settings().clone());
        app.pre_play_scene_name = jengine_core::scene::current_name();
        // Snapshot, not save: bulk-spawned entities are left out. See
        // `snapshot_scene_v2`.
        let snapshot = jengine_core::scene::snapshot_scene_v2(&app.world, &app.registry);
        match serde_json::to_string(&snapshot) {
            Ok(json) => app.play_snapshot = Some(json),
            Err(e) => {
                app.play_snapshot = None;
                app.log_buffer
                    .push(format!("[viewport] play snapshot failed: {e}"));
            }
        }

        // Parse the editor-supplied action map. Empty / malformed
        // payloads degrade to "no actions" — the scene is still
        // playable via direct key polling from behaviors.
        match serde_json::from_str::<Vec<InputAction>>(actions_json) {
            Ok(actions) if !actions.is_empty() => {
                app.input = InputState::new();
                app.input.load_actions(&actions);
                app.log_buffer
                    .push(format!("[viewport] loaded {} input action(s)", actions.len()));
            }
            Ok(_) => {
                app.input = InputState::new();
            }
            Err(e) => {
                app.input = InputState::new();
                app.log_buffer
                    .push(format!("[viewport] actions_json parse failed: {e}"));
            }
        }

        app.last_tick_ms = None;
        app.playing = true;
        app.paused = false;
        app.step_pending = false;
        app.log_buffer.push("[viewport] entering play mode".to_string());

        // Stage 2.2 — attach behavior-graph instances. Walk the world
        // for entities carrying `BehaviorGraphComponent` and feed each
        // into `attach_inline` from the `pending_behaviors` cache the
        // editor pre-uploaded via `viewport_load_behavior`. We use
        // `attach_inline` (not `sync_instances`) because the viewport
        // has no project_root — graph JSON arrives over IPC, not from
        // disk. Then dispatch OnSpawn for each fresh instance.
        let pairs: Vec<(Entity, String)> = app
            .world
            .query::<&BehaviorGraphComponent>()
            .iter()
            .map(|(e, c)| (e, c.graph_path.clone()))
            .collect();
        let mut attached = 0usize;
        let mut missing = 0usize;
        let mut newly_spawned: Vec<Entity> = Vec::new();
        for (entity, path) in pairs {
            let Some(json) = app.pending_behaviors.get(&path).cloned() else {
                missing += 1;
                continue;
            };
            match app
                .behavior_system
                .attach_inline(entity, PathBuf::from(&path), &json)
            {
                Ok(_) => {
                    attached += 1;
                    newly_spawned.push(entity);
                }
                Err(e) => {
                    app.log_buffer
                        .push(format!("[viewport] behavior attach '{path}' failed: {e:?}"));
                }
            }
        }
        for entity in newly_spawned {
            app.behavior_system.dispatch_spawn(entity, &mut app.world);
        }
        if attached > 0 || missing > 0 {
            app.log_buffer.push(format!(
                "[viewport] behavior graphs: attached={attached}, missing={missing}"
            ));
        }

        // Stage 2.9 — compile + attach browser scripts via shared helper
        // so scene transitions can rerun this on the new world.
        load_and_run_scripts(app, scripts_json);


        // Stage 2.3 — fresh PhysicsWorld every play. The
        // `physics_backend` arg is honored on a "rapier2d or skip"
        // basis: the only backend the engine ships today is rapier2d.
        // Anything else logs and runs without physics.
        if physics_backend.eq_ignore_ascii_case("rapier2d") || physics_backend.is_empty() {
            app.physics_config = Some((gravity_x, gravity_y, length_unit, fixed_timestep));
            let mut world = PhysicsWorld::new(gravity_x, gravity_y, length_unit);
            if let Some(dt) = fixed_timestep {
                world.set_fixed_timestep(dt);
            }
            app.physics = Some(world);
        } else {
            app.physics_config = None;
            app.physics = None;
            app.log_buffer.push(format!(
                "[viewport] unknown physics backend '{physics_backend}', physics disabled"
            ));
        }
    });
    Ok(())
}

#[wasm_bindgen]
pub fn viewport_stop() -> Result<String, JsValue> {
    let restored = with_app(|app| {
        app.playing = false;
        // A game that went fullscreen or hid the cursor gives the editor back.
        let canvas = app.canvas.clone();
        app.display.release(&canvas);
        app.text_proxy.release();
        jengine_core::scene::streaming::scene_replaced();
        app.paused = false;
        app.step_pending = false;
        // A game paused or slowed with `Time.set_scale` leaves the next Play
        // at full speed - it carried into it before, frozen.
        app.time_scale = 1.0;
        jengine_core::time::set_world_scale(1.0);
        jengine_core::time::set_audio_follows_time(false);
        app.last_tick_ms = None;
        // Reset input so a held key from play mode doesn't bleed back
        // into the editor.
        app.input = InputState::new();
        // Drop behavior runtime state — instances are entity-keyed
        // and the world is about to be replaced from snapshot. Fresh
        // system on next play.
        app.behavior_system = BehaviorGraphSystem::new();
        // Drop physics — body handles are tied to the soon-to-be-
        // replaced world entities. Fresh world on next play.
        app.physics = None;
        // Stage 2.9 — drop compiled scripts; entity ids will be
        // reassigned by load_scene_v2. Namespace state stays in JS
        // memory but is reseeded on next play.
        app.script_fns.clear();
        app.script_cache.clear();
        app.debug_draws.clear();
        app.play_namespace_ready = false;
        // Stage 2.9i — drop the script payload + tween manager so a
        // fresh play session starts clean.
        app.last_scripts_json.clear();
        app.tween_manager = jengine_core::animation::tween::TweenManager::new();
        // A new world stops the old one's films: dropping a decoder pauses
        // its element.
        app.video.clear();
        // Stage 2.9e — drop spawned-entity id mappings so a fresh
        // play session starts with a clean idx -> Entity slate.
        SCRIPT_ENTITY_MAP.with(|m| m.borrow_mut().clear());
        jengine_core::scripting::bridge::adapters::js::publish::forget_published_tilemaps();
        // Stage 2.9g — drop GUI click bindings; they refer to
        // script entities that no longer exist.
        app.gui_click_callbacks.clear();
        // Stage 2.9k — drop pending script-load requests; the
        // entity ids would resolve against the post-stop world.
        app.pending_script_loads.clear();
        // Stage 2.4 — clear scene stack so a subsequent edit-mode
        // scene change doesn't accidentally pop into a stale snapshot.
        app.scene_stack.clear();
        // Give every script its teardown callback before the world is
        // replaced from the snapshot — this is a despawn as far as scripts are
        // concerned, and it is their only chance to release anything the world
        // does not own.
        dispatch_despawn_for_all(app);
        // Silence the session. Looping music is owned by the audio backend,
        // not the world, so tearing down the world leaves it playing — the
        // hum from a scene kept going after Stop, with nothing left on screen
        // to explain it and no way to stop it short of closing the editor.
        // One-shot SFX are not tracked by the backend and cannot be stopped,
        // but they are short; the looping music is what actually strands.
        app.web_audio.stop_all_oneshots();
        jengine_core::audio::music::reset();
        // And put the effect chain back. Same reason as the music above: the
        // world does not own it, so restoring the world leaves whatever the
        // session switched on still running over the edited scene (BUGS #84).
        if let Some(pp) = app.pre_play_post_process.take()
            && let Some(gpu) = app.gpu.as_mut()
        {
            gpu.post_process.set_settings(pp);
        }

        let snapshot = app.play_snapshot.take();
        jengine_core::scene::set_current_name(&app.pre_play_scene_name);
        match snapshot {
            Some(json) => match serde_json::from_str::<SceneDataV2>(&json) {
                Ok(data) => {
                    let mut new_world = hecs::World::new();
                    load_scene_v2(&data, &mut new_world, &app.registry);
                    let mut ents: Vec<hecs::Entity> =
                        new_world.iter().map(|e| e.entity()).collect();
                    ents.sort_by_key(|e| e.id());
                    app.scene_entities = ents;
        app.authored_entity_count = app.scene_entities.len();
                    app.world = new_world;
                    app.log_buffer
                        .push("[viewport] play stopped, scene restored".to_string());
                    json
                }
                Err(e) => {
                    app.log_buffer
                        .push(format!("[viewport] snapshot restore failed: {e}"));
                    "{}".to_string()
                }
            },
            None => "{}".to_string(),
        }
    });
    Ok(restored.unwrap_or_else(|| "{}".to_string()))
}

#[wasm_bindgen]
pub fn viewport_is_playing() -> bool {
    with_app(|app| app.playing).unwrap_or(false)
}

/// Simulated dt for a single [`viewport_step`], in milliseconds.
const STEP_DT_MS: f64 = 1000.0 / 60.0;

/// Freeze or resume the play-mode world.
///
/// While paused the render loop keeps drawing, so the frame stays on
/// screen and the editor's gizmos, picking, and Inspector keep working
/// against a world that isn't moving under the cursor. Pausing clears
/// `last_tick_ms` so resuming doesn't hand the first tick the entire
/// pause duration as its dt.
#[wasm_bindgen]
pub fn viewport_set_paused(paused: bool) {
    with_app(|app| {
        if !app.playing {
            return;
        }
        app.paused = paused;
        app.step_pending = false;
        app.last_tick_ms = None;
    });
}

#[wasm_bindgen]
pub fn viewport_is_paused() -> bool {
    with_app(|app| app.playing && app.paused).unwrap_or(false)
}

/// Advance one fixed-dt frame while paused, then stay paused. No-op
/// unless play mode is running and paused.
#[wasm_bindgen]
pub fn viewport_step() {
    with_app(|app| {
        if app.playing && app.paused {
            app.step_pending = true;
        }
    });
}

/// Write the local `Transform` of a live entity.
///
/// The gizmo path for play mode. Unlike `viewport_set_runtime_entity` —
/// which despawns and respawns the entity to swap its whole component
/// bundle — this touches one component in place, so it is safe to call
/// every mouse-move and keeps the entity handle (and every script
/// reference to it) intact. Returns `false` if the index is stale or the
/// entity has no `Transform`.
#[wasm_bindgen]
pub fn viewport_set_entity_transform(
    index: usize,
    x: f32,
    y: f32,
    rotation: f32,
    scale_x: f32,
    scale_y: f32,
) -> bool {
    with_app(|app| {
        let Some(&entity) = app.scene_entities.get(index) else {
            return false;
        };
        let Ok(mut t) = app.world.get::<&mut Transform>(entity) else {
            return false;
        };
        t.position = glam::Vec2::new(x, y);
        t.rotation = rotation;
        t.scale = glam::Vec2::new(scale_x, scale_y);
        true
    })
    .unwrap_or(false)
}

/// Resolved world transform of a live entity, for placing the editor's
/// gizmo during play. Falls back to the local `Transform` for unparented
/// entities. Returns `"null"` when the index is stale.
#[wasm_bindgen]
pub fn viewport_get_entity_world_transform(index: usize) -> String {
    with_app(|app| {
        let Some(&entity) = app.scene_entities.get(index) else {
            return "null".to_string();
        };
        let Some(w) = jengine_core::ecs::hierarchy::world_transform_of(&app.world, entity)
        else {
            return "null".to_string();
        };
        serde_json::json!({
            "x": w.position.x,
            "y": w.position.y,
            "rotation": w.rotation,
            "scale_x": w.scale.x,
            "scale_y": w.scale.y,
        })
        .to_string()
    })
    .unwrap_or_else(|| "null".to_string())
}

/// Stage 2.9g — dispatch a GUI node click to whichever script
/// registered for it via `gui_on_click`. Returns `true` if a
/// callback was found and invoked. The editor calls this from its
/// DOM-side GUI rendering (or wgpu hit-test) when a button/clickable
/// node is pressed during play mode.
#[wasm_bindgen]
pub fn viewport_dispatch_gui_click(gui_id: &str, node_id: &str) -> bool {
    with_app(|app| dispatch_gui_click_impl(app, gui_id, node_id)).unwrap_or(false)
}

/// Inner dispatch usable when we already hold `&mut ViewportApp`
/// (e.g. from `dispatch_pending_gui_clicks` inside `tick_play_world`).
/// Calling `viewport_dispatch_gui_click` from there would trigger a
/// nested `with_app` borrow and panic the RefCell.
/// Call a script's named lifecycle handler, if it defines one.
///
/// The viewport has no `events = { ... }` subscription index — it resolves the

/// Hand an animation frame's events to the scripts on the entities whose
/// clips reached those frames: `on_animation_event(entity, { name, data })`.
///
/// The same shape as the collision dispatch below it, and new for the same
/// reason: the frames fired, the runtime threw the result away, and the
/// handler name was one nothing ever called.
fn dispatch_animation_events(app: &mut ViewportApp, events: &[(u64, String, Option<serde_json::Value>)]) {
    for (entity_id, name, data) in events {
        jengine_core::scripting::bridge::adapters::js::scripts::call_animation_event(&app.script_fns, *entity_id as u32, name, data.as_ref(), &mut |m| app.log_buffer.push(format!("[viewport] {m}")));
    }
    // A frame's `EmitEvent` is, as its name says, an event scripts can
    // listen for, so it also goes to the ordinary listeners - a game whose
    // animations are on its sprites and whose logic is on one controller
    // entity would otherwise hear nothing. Native does the same.
    for (entity_id, name, data) in events {
        let data_js = match data {
            Some(d) => JsValue::from_str(&d.to_string()),
            None => JsValue::NULL,
        };
        dispatch_event_to_listeners(app, name, *entity_id as u32, &data_js, None);
        // Native puts these on its script bus as `SendEvent`, and that bus
        // now reaches graphs - so they do here too, or an `OnEvent` graph
        // listening for a footstep would hear it on one runtime only.
        app.behavior_system.deliver_script_event(name, data.as_ref(), &mut app.world);
    }
}

/// Call the listener's handler for `name`, on every entity listening.
///
/// **Two spellings, because both are in use and each runtime only knew one.**
/// The documented convention - and what native has always called - is
/// `on_<event_name>`: `send_event("score_up")` calls `on_score_up(entity)`.
/// The browser hosts instead called a function literally named `on_event`,
/// with `(self, name, sender, data)`. Neither knew about the other, so a
/// script written from the docs fired on native and **silently did nothing**
/// here (test-lab scene 48 had been dead in the editor for its whole life),
/// while projects written against this spelling did nothing on native.
///
/// So try the specific name first, and fall back to the general one. Both
/// keep working, and a script can be written either way. BUGS #150.
///
/// **One implementation, because there used to be two.** The `evt` command arm
/// had its own inline copy of this loop, so fixing the handler-name lookup
/// here changed nothing for a script's `send_event` - only for an animation
/// frame's `EmitEvent`, the one caller this function had. That is
/// `docs/architecture-debt.md`'s hand-copied host code biting exactly as
/// described: the copy was found by the fix visibly not working in the editor.
/// `only` is `send_event_to`'s target, which is the one thing the copy did
/// that this did not.
fn dispatch_event_to_listeners(app: &mut ViewportApp, name: &str, sender: u32, data_js: &JsValue, only: Option<u32>) {
    jengine_core::scripting::bridge::adapters::js::scripts::dispatch_event(&app.script_fns, name, sender, data_js, only, &mut |m| app.log_buffer.push(format!("[viewport] {m}")));
}

/// handler by name through the script's `lookup` and skips silently when the
/// script does not define it. That matches how `on_collision_enter` and the
/// GUI click callbacks are dispatched here.
fn call_lifecycle_handler(app: &mut ViewportApp, eid: u32, handler: &str) {
    jengine_core::scripting::bridge::adapters::js::scripts::call_handler(&app.script_fns, eid, handler, &mut |m| app.log_buffer.push(format!("[viewport] {m}")));
}

/// Fire `on_despawn` for every scripted entity, newest first, then forget them.
///
/// Used wherever the whole world goes away at once — a scene transition or
/// Stop. Without this a script has no moment to release anything it owns that
/// the world does not: the obvious case is looping music, which lives in the
/// audio backend and happily outlives the entity that started it.
fn dispatch_despawn_for_all(app: &mut ViewportApp) {
    let mut ids: Vec<u32> = app.script_fns.keys().copied().collect();
    // Descending so later-spawned entities tear down before their spawners,
    // which is the order a script author expects.
    ids.sort_unstable_by(|a, b| b.cmp(a));
    for eid in ids {
        call_lifecycle_handler(app, eid, "on_despawn");
    }
    // Handlers may have queued commands (stop_music, save_set, ...); let them
    // land before the world is replaced.
    drain_play_commands_into(app);
    app.spawn_notified.clear();
}

fn dispatch_gui_click_impl(app: &mut ViewportApp, gui_id: &str, node_id: &str) -> bool {
    let log = &mut app.log_buffer;
    jengine_core::scripting::bridge::adapters::js::scripts::call_gui_click(&app.script_fns, &app.gui_click_callbacks, gui_id, node_id, &mut |m| {
        log.push(format!("[viewport] {m}"))
    })
}

/// Internal helper invoked from `tick_play_world` when the mouse was
/// left-clicked this frame. Walks every registered `(gui_id, node_id)`
/// callback, runs the GUI layout for the matching document at the
/// current canvas size, hit-tests the click position, and dispatches
/// the script callback for each node whose rect contains the cursor.
///
/// Matches the native runner's `process_gui_clicks` flow so the
/// editor preview behaves the same as `cargo run -p jengine-runtime`
/// for `gui_on_click(...)` registrations.
fn dispatch_pending_gui_clicks(app: &mut ViewportApp) {
    let (cw, ch) = (app.canvas_size.x as f32, app.canvas_size.y as f32);
    let size = app.ui_scale.virtual_size(cw, ch);
    if size.0 <= 0.0 || size.1 <= 0.0 {
        return;
    }
    // Virtual pixels, like the layout this hit-tests against.
    let hits = jengine_core::ui::layout::click_hits(
        app.gui_click_callbacks.keys().map(|(g, n)| (g.as_str(), n.as_str())),
        &jengine_core::ui::layout::visible_gui_ids(&app.world),
        &app.assets,
        app.ui_scale.to_ui(app.input.mouse_position.0, app.input.mouse_position.1, cw, ch),
        size,
    );
    for (gid, nid) in hits {
        dispatch_gui_click_impl(app, &gid, &nid);
    }
}

/// Stage 2.9k — drain pending `load_script(entity, path)` requests
/// that scripts emitted via the `lds` command. Returns a JSON array
/// of `[entity_id, path]` pairs and clears the queue. Editor calls
/// this after every `viewport_render`, fetches each script source
/// from its asset DB, then pushes them back in via
/// `viewport_inject_script`. Returns `"[]"` when empty.
#[wasm_bindgen]
pub fn viewport_drain_pending_script_loads() -> String {
    let Some(json) = with_app(|app| {
        if app.pending_script_loads.is_empty() {
            return "[]".to_string();
        }
        let drained: Vec<(u32, String)> = app.pending_script_loads.drain(..).collect();
        serde_json::to_string(&drained).unwrap_or_else(|_| "[]".to_string())
    }) else {
        return "[]".to_string();
    };
    json
}

/// Stage 2.9k — compile a script source against `entity_id` and run
/// its `init(entity_id, 0.0)` immediately. Used by the editor to
/// fulfill `load_script` requests drained from
/// `viewport_drain_pending_script_loads`, and is also safe to call
/// proactively (e.g. eager-loading scripts without a script-side
/// request). Idempotent: re-injecting against the same entity replaces
/// the previous `script_fns` entry. `language` is `"Lua"` or
/// `"JavaScript"` (anything else is treated as JS). `source_path` is
/// used as the cache key + the `//# sourceURL` label for diagnostics.
/// Returns `true` on success, `false` if the entity is missing or the
/// source fails to compile (compile errors land in `log_buffer` and
/// surface through `viewport_get_logs`).
#[wasm_bindgen]
pub fn viewport_inject_script(
    entity_id: u32,
    source: String,
    language: String,
    source_path: String,
) -> bool {
    with_app(|app| {
        let Some(entity) = find_entity_by_id(&app.world, entity_id) else {
            app.log_buffer.push(format!(
                "[viewport] inject_script({entity_id}, '{source_path}'): entity not found"
            ));
            return false;
        };
        let label = if source_path.is_empty() {
            format!("entity_{entity_id}")
        } else {
            source_path.clone()
        };
        // Pass raw source through `compile_browser_script`; the adapter picks
        // the right path (Fengari for Lua when available, transpiler fallback,
        // or pass-through for JS).
        let fns = match compile_browser_script(&label, &source, &language) {
            Ok(f) => f,
            Err(e) => {
                app.log_buffer
                    .push(format!("[viewport] compile '{label}' failed: {e}"));
                return false;
            }
        };
        if !source_path.is_empty() {
            app.script_cache.insert(source_path.clone(), fns.clone());
        }
        app.script_fns.insert(
            entity.id(),
            ScriptInstance { fns: fns.clone(), label: label.clone() },
        );
        // Run init() immediately so the script is hot this frame; mirror
        // the play-start ordering so commands emitted from init take
        // effect right away.
        if app.play_namespace_ready {
            sync_world_to_play_namespace(app);
            if let Err(e) = call_script_fn(&fns.init, entity.id(), 0.0) {
                app.log_buffer
                    .push(format!("[viewport] init '{label}' failed: {e}"));
            }
            drain_play_commands_into(app);
            sync_play_namespace_to_world(app);
        }
        true
    })
    .unwrap_or(false)
}

/// Park a `load_scene(path)` request where the editor polls for one, exactly
/// as the browser `load_scene` shim does - `{ action, path }` is all the
/// editor reads. A behavior graph's `ChangeScene` goes through here.
fn post_scene_load(path: &str) {
    let Some(play) = play_namespace() else { return };
    let req = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&req, &JsValue::from_str("action"), &JsValue::from_str("load"));
    let _ = js_sys::Reflect::set(&req, &JsValue::from_str("path"), &JsValue::from_str(path));
    let _ = js_sys::Reflect::set(&play, &JsValue::from_str("pending_scene"), &req);
}

#[wasm_bindgen]
pub fn viewport_get_pending_scene() -> String {
    // Stage 2.9d — browser scripts emit scene transitions by writing
    // `__jengine_play.pending_scene = { action, json?, path? }` from
    // `load_scene` / `push_scene` / `pop_scene` (see browser wrapper).
    // Read it here, clear the slot, and hand the JSON string back so
    // the editor host can call `viewport_apply_scene_transition` /
    // `viewport_pop_play_scene`. Returns "null" when nothing pending.
    // Empty string, not "null". This returned the four characters `null` when
    // nothing was pending, and the editor's `if (pendingJson)` treated that as
    // a real answer: `JSON.parse("null")` is `null`, and reading `.action` off
    // it threw a TypeError that the surrounding `catch` swallowed — **once per
    // frame, every frame of play**. An exception is not a cheap way to say
    // "nothing happened".
    let Some(play) = play_namespace() else {
        return String::new();
    };
    let key = JsValue::from_str("pending_scene");
    let val = match js_sys::Reflect::get(&play, &key) {
        Ok(v) if !v.is_undefined() && !v.is_null() => v,
        _ => return String::new(),
    };
    // Stringify and clear in one shot so we never replay the same request.
    let json = js_sys::JSON::stringify(&val)
        .ok()
        .and_then(|s| s.as_string())
        .unwrap_or_default();
    let _ = js_sys::Reflect::set(&play, &key, &JsValue::NULL);
    json
}

#[wasm_bindgen]
pub fn viewport_apply_scene_transition(
    scene_json: &str,
    scripts_json: &str,
) -> Result<(), JsValue> {
    // Stage 2.4 / 2.9i — replace the live world with `scene_json` (v1
    // or v2). If a play snapshot exists (i.e. we're in play mode),
    // push `(snapshot, last_scripts_json)` onto `scene_stack` first
    // so a subsequent `viewport_pop_play_scene` can restore the prior
    // scene AND its scripts. Behavior-graph and physics state are
    // reset because their handles are entity-keyed and the world is
    // about to be cleared. Input state is preserved (held keys carry
    // through the transition, matching the old glow-viewport
    // behavior). When `scripts_json` is non-empty we re-run the
    // load+init pipeline on the new world so script entities are
    // alive against the new entity-id space.
    with_app(|app| {
        let was_playing = app.playing;
        // The outgoing scene's scripts are about to stop existing. Give them
        // `on_despawn` first — it is their only chance to release anything the
        // world does not own (looping music being the obvious one, since the
        // audio backend outlives every world).
        if was_playing {
            dispatch_despawn_for_all(app);
        }
        if let Some(loaded) = app.play_snapshot.take() {
            let post_process = app
                .gpu
                .as_ref()
                .map(|gpu| gpu.post_process.settings().clone())
                .unwrap_or_default();
            // The world as it stands, not as it was loaded - see `PushedScene`.
            let live = serde_json::to_string(&jengine_core::scene::snapshot_scene_v2(&app.world, &app.registry))
                .unwrap_or_else(|_| loaded.clone());
            app.scene_stack.push(PushedScene {
                live,
                loaded,
                scripts: std::mem::take(&mut app.last_scripts_json),
                name: jengine_core::scene::current_name(),
                post_process,
            });
        }
        let mut new_world = World::new();
        let scene_json = &jengine_core::scene::expand_prefab_instances_in_json(
            scene_json,
            &app.prefabs,
            "viewport",
        );
        jengine_core::scene::set_current_name(&jengine_core::scene::name_of(scene_json).unwrap_or_default());
        load_scene_auto(scene_json, &mut new_world, &app.registry)
            .map_err(|e| JsValue::from_str(&format!("parse scene: {e}")))?;
        let meta = jengine_core::scene::format::read_scene_metadata(scene_json)
            .map_err(|e| JsValue::from_str(&format!("parse scene meta: {e}")))?;
        app.assets.set_active_sort_layers(meta.sort_layers);
        app.path_library = build_path_library(meta.paths);
        let mut ents: Vec<Entity> = new_world.iter().map(|e| e.entity()).collect();
        ents.sort_by_key(|e| e.id());
        app.scene_entities = ents;
        app.authored_entity_count = app.scene_entities.len();
        app.world = new_world;
        // Its additive chunks went with the old world.
        jengine_core::scene::streaming::scene_replaced();
        app.behavior_system = BehaviorGraphSystem::new();
        // Rebuild rather than drop: the outgoing world's body handles are
        // dead, but the *session* still wants physics. Dropping it here left
        // every scene reached by a transition running without physics for the
        // rest of play — see `physics_config` (BUGS #48).
        app.physics = app
            .physics_config
            .map(|(gx, gy, lu, dt)| {
                let mut world = PhysicsWorld::new(gx, gy, lu);
                if let Some(dt) = dt {
                    world.set_fixed_timestep(dt);
                }
                world
            });
        // Phase 8.2e — apply the scene's post-process chain (or clear).
        let pp = jengine_core::renderer::postprocess::PostProcessSettings::from_scene_json(scene_json).unwrap_or_default();
        if let Some(gpu) = app.gpu.as_mut() {
            gpu.post_process.set_settings(pp);
        }
        // Re-snapshot so `viewport_stop` returns the new scene (not
        // the original) when play mode ends here.
        if was_playing {
            let snap = jengine_core::scene::snapshot_scene_v2(&app.world, &app.registry);
            if let Ok(json) = serde_json::to_string(&snap) {
                app.play_snapshot = Some(json);
            }
        }
        // Stage 2.9i — recompile + run scripts against the new world.
        // `load_and_run_scripts` clears stale `script_fns`,
        // `tween_manager`, GUI bindings, and the `SCRIPT_ENTITY_MAP`
        // before reloading. Skipped when not playing — scripts only
        // run during play mode anyway.
        if was_playing {
            load_and_run_scripts(app, scripts_json);
        } else {
            app.script_fns.clear();
            app.script_cache.clear();
            app.debug_draws.clear();
            app.gui_click_callbacks.clear();
            app.tween_manager = jengine_core::animation::tween::TweenManager::new();
        // A new world stops the old one's films: dropping a decoder pauses
        // its element.
        app.video.clear();
            app.last_scripts_json = scripts_json.to_string();
            SCRIPT_ENTITY_MAP.with(|m| m.borrow_mut().clear());
            jengine_core::scripting::bridge::adapters::js::publish::forget_published_tilemaps();
        }
        app.log_buffer
            .push("[viewport] scene transition applied".to_string());
        Ok(())
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

#[wasm_bindgen]
pub fn viewport_pop_play_scene() -> Result<(), JsValue> {
    // Stage 2.4 / 2.9i — pop the most recently pushed
    // `(scene_snapshot, scripts_json)` pair off `scene_stack` (set by
    // `viewport_apply_scene_transition` while playing) and restore
    // both. No-op if the stack is empty.
    with_app(|app| {
        let Some(PushedScene { live, loaded, scripts: scripts_json, name, post_process }) = app.scene_stack.pop() else {
            return Ok(());
        };
        jengine_core::scene::set_current_name(&name);
        // Restore the chain the pushed-over scene was rendering with.
        if let Some(gpu) = app.gpu.as_mut() {
            gpu.post_process.set_settings(post_process);
        }
        let was_playing = app.playing;
        let data: SceneDataV2 = serde_json::from_str(&live)
            .map_err(|e| JsValue::from_str(&format!("parse stack snapshot: {e}")))?;
        let mut new_world = World::new();
        load_scene_v2(&data, &mut new_world, &app.registry);
        let mut ents: Vec<Entity> = new_world.iter().map(|e| e.entity()).collect();
        ents.sort_by_key(|e| e.id());
        app.scene_entities = ents;
        app.authored_entity_count = app.scene_entities.len();
        app.world = new_world;
        // Its additive chunks went with the old world.
        jengine_core::scene::streaming::scene_replaced();
        app.behavior_system = BehaviorGraphSystem::new();
        // Rebuild rather than drop: the outgoing world's body handles are
        // dead, but the *session* still wants physics. Dropping it here left
        // every scene reached by a transition running without physics for the
        // rest of play — see `physics_config` (BUGS #48).
        app.physics = app
            .physics_config
            .map(|(gx, gy, lu, dt)| {
                let mut world = PhysicsWorld::new(gx, gy, lu);
                if let Some(dt) = dt {
                    world.set_fixed_timestep(dt);
                }
                world
            });
        if was_playing {
            app.play_snapshot = Some(loaded);
            load_and_run_scripts(app, &scripts_json);
        } else {
            app.script_fns.clear();
            app.script_cache.clear();
            app.debug_draws.clear();
            app.gui_click_callbacks.clear();
            app.tween_manager = jengine_core::animation::tween::TweenManager::new();
        // A new world stops the old one's films: dropping a decoder pauses
        // its element.
        app.video.clear();
            app.last_scripts_json = scripts_json;
            SCRIPT_ENTITY_MAP.with(|m| m.borrow_mut().clear());
            jengine_core::scripting::bridge::adapters::js::publish::forget_published_tilemaps();
        }
        app.log_buffer
            .push("[viewport] scene popped from play stack".to_string());
        Ok(())
    })
    .unwrap_or_else(|| Err(JsValue::from_str("viewport not initialized")))
}

// ---------------------------------------------------------------------------
// Debug / queries (Stage 1: empty defaults)
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn viewport_get_logs() -> String {
    // Empty string for "nothing new", so the caller can skip the parse and the
    // loop. This runs every frame while playing and a script that logs on every
    // frame is a bug, so the empty case *is* the case.
    with_app(|app| {
        if app.log_buffer.is_empty() {
            return String::new();
        }
        let drained = std::mem::take(&mut app.log_buffer);
        serde_json::to_string(&drained).unwrap_or_default()
    })
    .unwrap_or_default()
}

#[wasm_bindgen]
pub fn viewport_get_debug_draws() -> String {
    // Drained per call: the shapes scripts drew this frame (the shared
    // `ddl` / `ddr` / `ddc` arms) and the F4 outlines, as one JSON array the
    // editor renders as an overlay.
    with_app(|app| {
        // Empty string for "no shapes this frame", so the editor neither parses
        // nor replaces its array — replacing it is a Svelte store write and a
        // re-render for nothing.
        if app.debug_draws.is_empty() {
            return String::new();
        }
        let drained: Vec<String> = std::mem::take(&mut app.debug_draws)
            .iter()
            .map(jengine_core::debug_draw::debug_command_json)
            .collect();
        format!("[{}]", drained.join(","))
    })
    .unwrap_or_default()
}

thread_local! {
    /// Fingerprint of the last entity list handed to the editor.
    ///
    /// The Scene panel polls this list every few frames, and building it walks
    /// the whole world: four random component lookups per entity to classify
    /// it, a `format!` for its name, a `serde_json` object, then the whole
    /// thing serialized, `JSON.parse`d and pushed into a Svelte store. At
    /// 8,000 entities that is a large spike once every `RUNTIME_LIST_INTERVAL`
    /// frames — a sawtooth in the frame graph, and invisible to the engine's
    /// phase timers because it is the editor calling in.
    ///
    /// The list only changes when something spawns or dies, so almost every
    /// rebuild produced an identical string. Comparing a hash of the result
    /// lets the caller skip the parse, the store write and the row re-render
    /// entirely — which is where the bulk of the spike was.
    static LAST_ENTITY_LIST_HASH: RefCell<Option<u64>> = const { RefCell::new(None) };

    /// Topology of the world at the last poll, from
    /// [`jengine_core::ecs::topology::topology_fingerprint`].
    ///
    /// `LAST_ENTITY_LIST_HASH` above spares the *editor* the parse and the
    /// re-render, but the snapshot was still built every poll in order to be
    /// hashed — four component sets, a name formatted per entity and a serde
    /// pass, all to discover nothing had changed. Measured at 23.1 ms per poll
    /// at 16,000 entities against 0.64 ms to fingerprint (36x; see
    /// `jengine-core/tests/entity_list_poll_cost.rs`), and 0.64 ms against
    /// 0.019 ms even at 500 — which is why the sawtooth was visible in small
    /// scenes too.
    ///
    /// So ask the cheap question first. The fingerprint covers exactly what
    /// the panel displays — which entities exist, what components they carry,
    /// their names and their parents — and deliberately not component values,
    /// so a world that merely *moved* answers "unchanged".
    static LAST_ENTITY_TOPOLOGY: RefCell<Option<u64>> = const { RefCell::new(None) };
}

/// List the live world for the editor's Scene panel during play.
///
/// The editor addresses entities by their index in `scene_entities`, and
/// script-spawned entities (`spawn_prefab`, `create_entity`, HUD text, …)
/// never enter that Vec — so the panel could only ever show the authored
/// scene, and spawned entities were uninspectable. This call first
/// *adopts* every world entity missing from `scene_entities` by appending
/// it, which hands it an index the existing IPC
/// (`viewport_get_runtime_entity`, `viewport_set_selected`,
/// `viewport_get_entity_bounds`) already understands.
///
/// Indices are never reused or compacted: a despawned entity leaves an
/// inert slot behind rather than renumbering everything the editor is
/// holding mid-frame. Dead slots are simply skipped in the output, so the
/// returned array is the set of *live* entities and `index` values are
/// sparse. `scene_entities` is rebuilt wholesale by `viewport_stop` and by
/// the scene-transition paths, so the slots do not accumulate across
/// play sessions or scene changes.
///
/// Returns `[{ index, name, parent, spawned, internal }]`, where `parent`
/// is the index of the entity's `Parent` (or `null`), `spawned` marks the
/// adopted entities, and `internal` marks engine-managed view entities
/// (health-bar quads, name tags) that the editor hides by default.
///
/// Entity list for the editor's Scene panel, or `""` when unchanged.
///
/// An empty string means "same as last time" — the caller keeps what it has
/// and skips the parse and the store update.
#[wasm_bindgen]
pub fn viewport_get_runtime_entities() -> String {
    use jengine_core::ecs::components::{
        BulkEntity, HealthBarView, Name, NameTagView, Parent, TextComponent,
    };

    /// Most entities the Scene panel will list before giving up.
    ///
    /// A backstop for bulk entities that were not marked `BulkEntity`. The
    /// panel renders one DOM row per entity with several event handlers and
    /// does not virtualise, so a list this long is already past the point of
    /// being usable - and far past the point of being affordable.
    const MAX_LISTED: usize = 2_000;

    with_app(|app| {
        // Cheap question first: has anything the panel shows actually changed?
        //
        // Safe to return before the adoption and trim steps below, because
        // both are driven by entities appearing or disappearing — and that is
        // precisely what an unchanged fingerprint rules out. If the world's
        // entity set is identical, adoption finds nothing to adopt and the
        // trim finds no dead tail, so skipping them changes nothing.
        // `authored_entity_count` is the other input the rows depend on (it
        // decides the `spawned` flag) and it does not live in the world, so
        // mix it in. Every site that assigns it also replaces `app.world`
        // wholesale with a fresh one — and a fresh `hecs::World` restarts its
        // ids, so two scenes of the same shape *can* fingerprint alike.
        // Mixing here rather than invalidating at those four sites means a
        // fifth one added later cannot forget to.
        let topology = {
            use std::hash::Hash;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            jengine_core::ecs::topology::topology_fingerprint(&app.world).hash(&mut h);
            app.authored_entity_count.hash(&mut h);
            std::hash::Hasher::finish(&h)
        };
        let same_topology = LAST_ENTITY_TOPOLOGY.with(|t| {
            let mut t = t.borrow_mut();
            let same = *t == Some(topology);
            *t = Some(topology);
            same
        });
        if same_topology {
            // An empty string is the caller's "unchanged" signal, the same one
            // the hash comparison at the bottom produces.
            return String::new();
        }

        let authored = app.authored_entity_count;

        // Four sequential scans instead of four random lookups per entity.
        let collect_set = |f: &dyn Fn(&World) -> Vec<Entity>| -> HashSet<Entity> {
            f(&app.world).into_iter().collect()
        };
        let kind_particle =
            collect_set(&|w: &World| w.query::<&ParticleEmitter>().iter().map(|(e, _)| e).collect());
        let kind_text =
            collect_set(&|w: &World| w.query::<&TextComponent>().iter().map(|(e, _)| e).collect());
        let kind_camera = collect_set(&|w: &World| {
            w.query::<&Camera2DComponent>().iter().map(|(e, _)| e).collect()
        });
        let kind_sprite =
            collect_set(&|w: &World| w.query::<&SpriteComponent>().iter().map(|(e, _)| e).collect());

        // Reclaim trailing dead slots before adopting. A play session that
        // fires thousands of projectiles would otherwise grow this Vec
        // without bound. Only the tail is trimmed, so no *live* entity is
        // ever renumbered — the editor's held indices stay valid.
        while app.scene_entities.len() > authored
            && app
                .scene_entities
                .last()
                .is_some_and(|&e| !app.world.contains(e))
        {
            app.scene_entities.pop();
        }

        let known: HashSet<Entity> = app.scene_entities.iter().copied().collect();
        let mut adopted: Vec<Entity> = app
            .world
            .iter()
            .map(|e| e.entity())
            .filter(|e| !known.contains(e))
            .collect();
        adopted.sort_by_key(|e| e.id());
        app.scene_entities.extend(adopted);

        // Parent lookups only ever resolve to live entities, so the map
        // is built from those rather than from every (mostly dead) slot.
        let index_of: HashMap<Entity, usize> = app
            .scene_entities
            .iter()
            .enumerate()
            .filter(|&(_, &e)| app.world.contains(e))
            .map(|(i, &e)| (e, i))
            .collect();
        let mut out = Vec::with_capacity(app.scene_entities.len());
        let mut skipped_bulk = 0usize;
        for (i, &entity) in app.scene_entities.iter().enumerate() {
            if !app.world.contains(entity) {
                continue;
            }
            // Bulk simulation entities are not listed: the panel renders a DOM
            // row each and does not virtualise, so 16,000 boids cost roughly
            // 68 ms a frame against 24 ms of engine work - and a list that long
            // is no use to anyone. See `BulkEntity`.
            if app.world.get::<&BulkEntity>(entity).is_ok() {
                skipped_bulk += 1;
                continue;
            }
            if out.len() >= MAX_LISTED {
                skipped_bulk += 1;
                continue;
            }
            let name = app
                .world
                .get::<&Name>(entity)
                .map(|n| n.0.clone())
                .unwrap_or_else(|_| format!("Entity_{}", entity.id()));
            let parent = app
                .world
                .get::<&Parent>(entity)
                .ok()
                .and_then(|p| index_of.get(&p.0).copied());
            let internal = app.world.get::<&HealthBarView>(entity).is_ok()
                || app.world.get::<&NameTagView>(entity).is_ok();
            // Coarse type hint so the Scene panel can pick the same row
            // icon it uses for authored entities.
            // From the sets gathered above rather than four random component
            // lookups per entity - that was ~32,000 archetype probes per poll
            // at 8,000 entities.
            let kind = if kind_particle.contains(&entity) {
                "particle"
            } else if kind_text.contains(&entity) {
                "text"
            } else if kind_camera.contains(&entity) {
                "camera"
            } else if kind_sprite.contains(&entity) {
                "sprite"
            } else {
                ""
            };
            out.push(serde_json::json!({
                "index": i,
                "name": name,
                "parent": parent,
                "spawned": i >= authored,
                "internal": internal,
                "kind": kind,
            }));
        }
        // Said once per change rather than per frame, so it is visible without
        // becoming noise.
        if skipped_bulk > 0 {
            log::debug!(
                "[viewport] Scene panel listing {} entities; {skipped_bulk} hidden                  (BulkEntity, or beyond the {MAX_LISTED} cap)",
                out.len()
            );
        }
        let json = serde_json::to_string(&out).unwrap_or_else(|_| "[]".to_string());
        // Compare on the serialized result: it captures names, parents and
        // kinds as well as the set of entities, so a rename shows up too.
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&json, &mut hasher);
        let hash = std::hash::Hasher::finish(&hasher);
        let unchanged = LAST_ENTITY_LIST_HASH.with(|h| {
            let mut h = h.borrow_mut();
            let same = *h == Some(hash);
            *h = Some(hash);
            same
        });
        if unchanged { String::new() } else { json }
    })
    .unwrap_or_else(|| "[]".to_string())
}

#[wasm_bindgen]
pub fn viewport_get_entity_bounds() -> String {
    let Some(json) = with_app(|app| {
        let mut out = Vec::new();
        // Text metrics depend on which font the entity names, so picking has
        // to resolve `font_id` the same way the renderer does.
        let frame_fonts = jengine_core::renderer::font_asset::FrameFonts::gather(
            &app.assets,
            &app.font_data,
            BUILTIN_FONT_ID,
        );
        for (i, &entity) in app.scene_entities.iter().enumerate() {
            let Ok(t) = app.world.get::<&Transform>(entity) else {
                continue;
            };
            // Text is measured, not guessed. Its box is anchored top-left and
            // sized by the string, so the 64x64 centred fallback below missed
            // it almost entirely - a long label was unclickable except near
            // its origin, and a scaled title was unclickable full stop.
            if let Ok(tc) = app.world.get::<&TextComponent>(entity)
                && !tc.text.is_empty()
            {
                let (font, _) = frame_fonts.resolve(&tc.font_id);
                let b = jengine_core::renderer::text_layout::text_bounds(
                    font,
                    &tc.text,
                    (t.position.x, t.position.y),
                    tc.font_scale * t.scale.x,
                    (tc.wrap_width > 0.0).then_some(tc.wrap_width),
                );
                out.push(serde_json::json!({
                    "index": i as i32,
                    "x": b.center_x,
                    "y": b.center_y,
                    "width": b.width,
                    "height": b.height,
                    // The renderer does not rotate text, so picking must not
                    // either, or the hit box drifts off the glyphs.
                    "rotation": 0.0,
                    "z_order": tc.z_order,
                }));
                continue;
            }
            // The size the renderer draws: explicit, else the texture's own.
            let (w, h, z) = match app.world.get::<&SpriteComponent>(entity) {
                Ok(sp) => {
                    let d = sp.drawn_size(Vec2::from(SpriteComponent::FALLBACK_SIZE));
                    (d.x, d.y, sp.z_order)
                }
                Err(_) => (64.0_f32, 64.0_f32, 0_i32),
            };
            out.push(serde_json::json!({
                "index": i as i32,
                "x": t.position.x,
                "y": t.position.y,
                "width": w * t.scale.x,
                "height": h * t.scale.y,
                "rotation": t.rotation,
                "z_order": z,
            }));
        }
        serde_json::Value::Array(out).to_string()
    }) else {
        return "[]".to_string();
    };
    json
}

#[wasm_bindgen]
pub fn viewport_get_collider_bounds() -> String {
    // Sizes as authored, **not** multiplied by the transform's scale:
    // `PhysicsWorld::add_collider` ignores scale, so an outline that applied
    // it drew a scaled entity's collider at a size rapier never simulated.
    // This overlay exists to show what physics sees.
    let Some(json) = with_app(|app| {
        let mut out = Vec::new();
        for (_, (t, c)) in app.world.query::<(&Transform, &Collider2D)>().iter() {
            let (kind, payload) = match &c.shape {
                ColliderShape::Box { width, height } => (
                    "box",
                    serde_json::json!({
                        "width": *width,
                        "height": *height,
                    }),
                ),
                ColliderShape::Circle { radius } => (
                    "circle",
                    serde_json::json!({
                        "radius": *radius,
                    }),
                ),
                ColliderShape::Capsule { half_height, radius } => (
                    "capsule",
                    serde_json::json!({
                        "half_height": *half_height,
                        "radius": *radius,
                    }),
                ),
                _ => ("unsupported", serde_json::json!({})),
            };
            out.push(serde_json::json!({
                "x": t.position.x + c.offset[0],
                "y": t.position.y + c.offset[1],
                "rotation": t.rotation,
                "is_sensor": c.is_sensor,
                "kind": kind,
                "shape": payload,
            }));
        }
        serde_json::Value::Array(out).to_string()
    }) else {
        return "[]".to_string();
    };
    json
}

#[wasm_bindgen]
pub fn viewport_get_game_camera() -> String {
    let Some(json) = with_app(|app| {
        // The main camera (`camera_rig::main_camera`; must have a Transform).
        let main = jengine_core::ecs::camera_rig::main_camera(&app.world);
        for (e, (t, cam)) in app
            .world
            .query::<(&Transform, &Camera2DComponent)>()
            .iter()
        {
            if Some(e) != main {
                continue;
            }
            let obj = serde_json::json!({
                "x": t.position.x,
                "y": t.position.y,
                "zoom": cam.zoom,
                "viewport_w": cam.viewport_width,
                "viewport_h": cam.viewport_height,
            });
            return obj.to_string();
        }
        // Fallback: editor camera.
        let cam = &app.camera;
        serde_json::json!({
            "x": cam.position.x,
            "y": cam.position.y,
            "zoom": cam.zoom,
            "viewport_w": cam.width(),
            "viewport_h": cam.height(),
        })
        .to_string()
    }) else {
        return "null".to_string();
    };
    json
}

#[wasm_bindgen]
pub fn viewport_get_frame_stats() -> String {
    let Some(json) = with_app(|app| {
        let s = app.frame_stats;
        serde_json::json!({
            "fps": s.fps,
            "frame_time_ms": s.frame_time_ms,
            "entity_count": s.entity_count,
            "sprite_count": s.sprite_count,
            "render_time_ms": s.render_time_ms,
            "update_time_ms": s.update_time_ms,
            // These two were computed every tick but never emitted, so the
            // profiler showed a hard 0.00 ms for Scripts and Physics and left
            // most of the frame unattributed — which is exactly the part you
            // need when chasing a spike.
            "script_time_ms": s.script_time_ms,
            "physics_time_ms": s.physics_time_ms,
            // Stage 2.9j — wgpu draw calls (sprite batch flushes)
            // captured during the most recent `render_editor`.
            "draw_calls": s.draw_calls,
            // Reserved wasm linear memory for the viewport module. The
            // editor is a separate process from a shipped build, so treat
            // this as "what play mode is holding", not as a size the game
            // will have in a browser.
            "memory_bytes": jengine_core::profiling::memory::resident_bytes() as u64,
            "memory_peak_bytes": jengine_core::profiling::memory::peak_bytes() as u64,
            // Live heap. Reserved wasm memory never shrinks, so it alone
            // makes every scene load look like a leak; this is the figure
            // that comes back down when a scene is dropped.
            "memory_live_bytes": jengine_core::profiling::memory::live_bytes() as u64,
        })
        .to_string()
    }) else {
        return "{}".to_string();
    };
    json
}

/// Start or stop recording frames for the editor's Profiler panel
/// (`profiling::trace`); off, the spans cost a thread-local read each.
#[wasm_bindgen]
pub fn viewport_set_profiling(on: bool) {
    jengine_core::profiling::trace::set_enabled(on);
}

/// The frame recorded since the last call, as JSON (`trace::FrameRecord`),
/// or `null` when not recording. The editor calls it once a frame, after
/// `viewport_render`, and adds the scripts' costs Fengari measured.
#[wasm_bindgen]
pub fn viewport_take_profile_frame() -> String {
    match jengine_core::profiling::trace::take_frame() {
        Some(frame) => serde_json::to_string(&frame).unwrap_or_else(|_| "null".into()),
        None => "null".into(),
    }
}

#[wasm_bindgen]
pub fn viewport_get_frame_history() -> String {
    let Some(json) = with_app(|app| {
        let v: Vec<f32> = app.frame_history.iter().copied().collect();
        serde_json::to_string(&v).unwrap_or_else(|_| "[]".to_string())
    }) else {
        return "[]".to_string();
    };
    json
}

// ---------------------------------------------------------------------------
// Main render
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn viewport_render() {
    let Some(app_rc) = APP.with(|c| c.borrow().clone()) else {
        return;
    };
    let mut app_ref = app_rc.borrow_mut();
    let app: &mut ViewportApp = &mut app_ref;
    if app.gpu.is_none() {
        return; // device not ready yet
    }
    if app.playing && !app.paused {
        tick_play_world(app);
    } else if app.playing && app.step_pending {
        // Step: advance exactly one frame's worth of simulation. The dt
        // is synthesized rather than measured, so a step is the same
        // size however long you sat on the button.
        app.step_pending = false;
        app.last_tick_ms = Some(perf_now_ms() - STEP_DT_MS);
        tick_play_world(app);
        // Drop the timestamp so resuming starts from dt = 0 instead of
        // charging the world for the time spent paused.
        app.last_tick_ms = None;
    } else if !app.playing {
        // Stage 2.8 \u2014 zero out play-only sub-stage timings in edit mode
        // so the profiler panel doesn't display stale values from the
        // last play session.
        app.frame_stats.physics_time_ms = 0.0;
        app.frame_stats.script_time_ms = 0.0;
        app.frame_stats.update_time_ms = 0.0;
    }
    let render_start = perf_now_ms();
    if let Err(e) = render_editor(app) {
        console_warn!("viewport_render: {e}");
    }
    app.frame_stats.render_time_ms = (perf_now_ms() - render_start) as f32;
    // Stage 2.9j — capture the per-frame draw-call total written by
    // every `WgpuSpriteBatch::flush` invoked during `render_editor`.
    if let Some(gpu) = app.gpu.as_ref() {
        app.frame_stats.draw_calls = gpu.sprite_batch.frame_draw_calls;
    }
    if app.playing {
        // After the frame is rendered, mark transient input as consumed
        // so "just-pressed" semantics work across the next render.
        app.input.begin_frame();
    }

    // Stage 2.6 — sample frame timing + counters for the profiler panel.
    let now = perf_now_ms();
    let dt_ms = match app.frame_stats.last_render_ms {
        Some(prev) => ((now - prev) as f32).max(0.0),
        None => 0.0,
    };
    app.frame_stats.last_render_ms = Some(now);
    app.frame_stats.frame_time_ms = dt_ms;
    app.frame_stats.fps = if dt_ms > 0.0 { 1000.0 / dt_ms } else { 0.0 };
    app.frame_stats.entity_count = app.world.iter().count() as u32;
    app.frame_stats.sprite_count = app.world.query::<&SpriteComponent>().iter().count() as u32;
    if app.frame_history.len() >= 120 {
        app.frame_history.pop_front();
    }
    app.frame_history.push_back(dt_ms);
}

// ---------------------------------------------------------------------------
// Editor grid
// ---------------------------------------------------------------------------

/// Round `raw` up to a "nice" world-space step: nearest value in the series
/// 1, 2, 5, 10, 20, 50, 100, ...
fn nice_grid_step(raw: f32) -> f32 {
    if raw <= 0.0 {
        return 1.0;
    }
    let exp = raw.log10().floor();
    let base = 10_f32.powf(exp);
    let frac = raw / base;
    let nice = if frac < 1.5 {
        1.0
    } else if frac < 3.5 {
        2.0
    } else if frac < 7.5 {
        5.0
    } else {
        10.0
    };
    (base * nice).max(0.001)
}

/// Push a vertical quad (screen-stable width) into the shape batch.
fn push_vline(batch: &mut WgpuShapeBatch, x: f32, y0: f32, y1: f32, half_w: f32, color: [f32; 4]) {
    let positions = [
        [x - half_w, y0],
        [x + half_w, y0],
        [x + half_w, y1],
        [x - half_w, y1],
    ];
    batch.draw_tessellated(&positions, &[0, 1, 2, 0, 2, 3], color);
}

/// Push a horizontal quad (screen-stable height) into the shape batch.
fn push_hline(batch: &mut WgpuShapeBatch, y: f32, x0: f32, x1: f32, half_h: f32, color: [f32; 4]) {
    let positions = [
        [x0, y - half_h],
        [x1, y - half_h],
        [x1, y + half_h],
        [x0, y + half_h],
    ];
    batch.draw_tessellated(&positions, &[0, 1, 2, 0, 2, 3], color);
}

/// Draw the scene-mode editor grid into `pass` using `batch`.
///
/// Grid adapts its step to the camera zoom so there are always ~10-20
/// visible cells. Origin axes (X red, Y green) are drawn slightly thicker.
fn draw_editor_grid(
    batch: &mut WgpuShapeBatch,
    camera: &Camera2D,
    queue: &wgpu::Queue,
    pass: &mut wgpu::RenderPass<'_>,
) {
    let zoom = camera.zoom;
    let half_w = camera.width() / (2.0 * zoom);
    let half_h = camera.height() / (2.0 * zoom);
    let cx = camera.position.x;
    let cy = camera.position.y;

    let left   = cx - half_w;
    let right  = cx + half_w;
    let bottom = cy - half_h;
    let top    = cy + half_h;

    // Aim for ~14 cell gaps across the wider axis.
    let raw_step = (half_w * 2.0).max(half_h * 2.0) / 14.0;
    let step = nice_grid_step(raw_step);

    // Line half-thicknesses in world units (constant screen width).
    let lt      = 0.5 / zoom; // regular grid: ~1 px
    let axis_lt = 1.5 / zoom; // origin axes:  ~3 px

    // Colors: the editor's palette (`viewport_set_grid_colors`).
    let [grid_color, axis_x_color, axis_y_color] = GRID_COLORS.with(|g| g.get());

    // Add a small margin so lines extend slightly past the visible edge.
    let margin = step;

    batch.begin();

    // Vertical lines.
    let first_x = (left / step).floor() * step;
    let mut x = first_x;
    while x <= right + margin {
        let on_axis = x.abs() < step * 0.01;
        push_vline(
            batch,
            x,
            bottom - margin,
            top + margin,
            if on_axis { axis_lt } else { lt },
            if on_axis { axis_y_color } else { grid_color },
        );
        x += step;
        if batch.is_full() {
            break;
        }
    }

    // Horizontal lines.
    let first_y = (bottom / step).floor() * step;
    let mut y = first_y;
    while y <= top + margin {
        let on_axis = y.abs() < step * 0.01;
        push_hline(
            batch,
            y,
            left - margin,
            right + margin,
            if on_axis { axis_lt } else { lt },
            if on_axis { axis_x_color } else { grid_color },
        );
        y += step;
        if batch.is_full() {
            break;
        }
    }

    let vp = camera.view_projection();
    batch.flush(queue, pass, &vp);
}

fn render_editor(app: &mut ViewportApp) -> Result<(), String> {
    let UVec2 { x: vw, y: vh } = app.canvas_size;
    apply_safe_fraction(app);
    let ui_scale = app.ui_scale;
    // Game-tab override: when JS has called `viewport_set_camera_size`
    // (i.e. we're rendering the Game view of an active scene camera),
    // size the camera to the game's virtual resolution instead of the
    // editor canvas pixels. Otherwise zoom is interpreted relative to
    // canvas size and HUD/text sized for the game viewport renders at
    // the wrong scale (e.g. font_scale 22 against an 851-px-wide
    // canvas instead of a 1280-px-wide game viewport).
    let (cam_w, cam_h) = match app.game_viewport {
        Some([w, h]) => {
            // Preserve square pixels: show more world along the canvas's
            // longer axis rather than stretching 1280x720 over a canvas of
            // another shape (e.g. 1013x580). This was the one runtime that
            // did it; `Camera2D::fit_view` is that rule, now everyone's.
            let surface = glam::Vec2::new(vw as f32, vh as f32);
            let mut view = Camera2D::fit_view(glam::Vec2::new(w.max(1.0), h.max(1.0)), surface, false);
            // In Play a pixel-perfect camera may scale by whole pixels.
            if app.playing && app.camera.integer_scale {
                view = Camera2D::integer_view(view, surface, app.camera.pixel_size, app.camera.zoom);
            }
            (view.x, view.y)
        }
        None => (vw as f32, vh as f32),
    };
    app.camera.resize(cam_w, cam_h);
    // A pixel-perfect camera draws the world at the art's resolution, through
    // a camera on the art's grid - in Play; editing draws at the canvas's.
    let mut camera = app.camera;
    if !app.playing {
        camera.pixel_size = 0.0;
        // Editing shows every layer, whatever the scene's camera culls.
        camera.cull_mask = u32::MAX;
    }
    let camera = match app.gpu.as_mut() {
        Some(gpu) => jengine_core::ecs::wgpu_systems::apply_pixel_plan(
            camera,
            (vw, vh),
            &gpu.renderer.device,
            &mut gpu.post_process,
            &mut gpu.light_pass,
        ),
        None => camera,
    };
    // What this frame can see, for culling before the world pass (which sets
    // it again per camera).
    jengine_core::renderer::frame_view::set_frame_view(&camera);

    // Slice 3.5.A — resolve world-space transforms from parent chains
    // BEFORE the render pass reads them. Runs in both edit and play modes
    // so the inspector reflects parented placement live.
    // Dynamic bodies are drawn blended between their last two physics
    // steps; a frame that did not step (edit mode) blends nothing.
    if let Some(physics) = app.physics.as_mut() {
        physics.blend_transforms_for_render(&mut app.world);
    }
    jengine_core::ecs::hierarchy::resolve_world_transforms(&mut app.world);
    // Trails record where things ended up this frame (`ecs::trails`) - in
    // play only, or dragging an entity in the Scene tab would leave points
    // that never age.
    if app.playing {
        let dt = std::mem::take(&mut app.trail_dt);
        jengine_core::ecs::trails::tick_trails(&mut app.world, dt);
        jengine_core::ecs::cloth::tick_cloth(&mut app.world, dt);
    }
    // Holes queued in carvable sprites, cut from their pixels (`ecs::carve`).
    jengine_core::ecs::carve::apply_carves(&mut app.world, &mut app.assets);

    // Same reason, for sprites and animation frames that name an atlas region
    // instead of carrying a uv rect: until this runs they keep the default
    // full-texture rect and draw the whole sheet. It used to run only inside
    // `tick_play_world`, so every region-named sprite in a game's scene
    // and prefabs looked correct in Play and showed the entire sheet in the
    // Scene tab. Already-resolved sprites are skipped, so this is a no-op once
    // the atlases are in.
    jengine_core::ecs::systems::resolve_sprite_regions(&mut app.world, &app.assets);

    let gpu = app.gpu.as_mut().ok_or("gpu not ready")?;
    // Stage 2.9j — reset per-frame draw-call counter before any
    // `flush()` calls. The batch counts every flush across the main
    // scene pass, the PiP pass, and the UI/GUI flushes.
    gpu.sprite_batch.begin_frame();
    // Slice 13d — feed the elapsed wall-clock seconds into the sprite
    // batch so graph shaders that read `globals.time.x` (TimeInput
    // node) animate in the editor viewport, matching the native
    // runtime.
    let elapsed_secs = ((perf_now_ms() - app.start_time_ms) / 1000.0) as f32;
    gpu.sprite_batch.set_time(elapsed_secs);

    let mut frame = match gpu.renderer.begin_frame() {
        Ok(f) => f,
        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
            gpu.renderer.resize(app.canvas_size);
            return Ok(());
        }
        Err(e) => return Err(format!("surface error: {e:?}")),
    };

    // On-screen touch controls, resolved before the closures borrow `app`.
    let touch_draws = {
        let (cw, ch) = (app.canvas_size.x as f32, app.canvas_size.y as f32);
        let (sw, sh) = app.ui_scale.screen_virtual_size(cw, ch);
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

    // Stage 2.7 — decide whether to take the lit path BEFORE we open
    // the scene render pass; the choice picks which target to bind.
    let lighting_active = app.show_lighting && has_lights(&app.world);

    // Views, the world, the post chain and the UI: one function every host
    // shares (`ecs::frame_draw`). The editor's part is its policy - shader
    // graphs it was pushed, textures it asks the editor for - and the grid
    // drawn under the scene while editing.
    {
        use jengine_core::ecs::frame_draw::{FrameDraw, UiScene, WorldScene, draw_frame};
        let scene_clear = gpu.renderer.clear_color;
        let sort_layers = app.assets.active_sort_layers().clone();
        // Lifted out of `app.assets` before it is lent `&mut`; each font is
        // shared by `Arc`, so this is a refcount bump.
        let frame_fonts = jengine_core::renderer::font_asset::FrameFonts::gather(
            &app.assets,
            &app.font_data,
            BUILTIN_FONT_ID,
        );
        let mut host = EditorDrawHost { graphs: &app.pending_shader_graphs };
        let queue = &gpu.renderer.queue;
        let shape_batch = &mut gpu.shape_batch;
        let mut grid = |pass: &mut wgpu::RenderPass<'_>, camera: &Camera2D| {
            draw_editor_grid(shape_batch, camera, queue, pass);
        };
        draw_frame(
            FrameDraw {
                device: &gpu.renderer.device,
                queue,
                scene_format: gpu.renderer.scene_format,
                color_format: gpu.renderer.color_format,
                world: &app.world,
                assets: &mut app.assets,
                warned: &mut app.missing_warned,
                graph_pipelines: &mut gpu.graph_pipelines,
                sprite_batch: &mut gpu.sprite_batch,
                lit_sprite_batch: &mut gpu.lit_sprite_batch,
                flat_normal: &gpu.flat_normal,
                particle_system: &app.particle_system,
                frame_fonts: &frame_fonts,
                sort_layers: &sort_layers,
                post: &mut gpu.post_process,
                light: &mut gpu.light_pass,
                view_targets: &mut gpu.view_targets,
                host: &mut host,
            },
            &mut frame,
            WorldScene {
                camera,
                surface: (vw, vh),
                lighting: lighting_active,
                clear: scene_clear,
                // Script shapes and F4 outlines go to the editor's own
                // overlay (`viewport_get_debug_draws`), not into the frame.
                debug_draws: &[],
                physics_outlines: false,
                // The grid, under the scene, while editing.
                underlay: if app.playing { None } else { Some(&mut grid) },
            },
            UiScene { ui_scale, gui_jobs: &gui_jobs, touch_draws: &touch_draws, overlay: None },
        );
    }

    // Stage 2.7 — the lighting composite and the post chain both run inside
    // the block above: the composite between the world pass and the pass for
    // things that must not be lit twice, the post chain before the UI.

    // Stage 2.6 — picture-in-picture: render the world a second time
    // through a custom camera into a sub-rect of the swap-chain texture.
    if let Some(req) = app.pip_request.take() {
        let mut pip_cam = Camera2D::new(req.cam_vw.max(1.0), req.cam_vh.max(1.0));
        pip_cam.position = Vec2::new(req.cam_x, req.cam_y);
        pip_cam.zoom = if req.cam_zoom > 0.0 { req.cam_zoom } else { 1.0 };
        let pip_vp = pip_cam.view_projection();
        // The picture-in-picture draws through its own camera; cull against it.
        // A second camera onto the world: canvas groups' members as they are
        // (their pictures are drawn for the main camera's view).
        pip_cam.pass = jengine_core::renderer::camera::CameraPass::View;
        jengine_core::renderer::frame_view::set_frame_view(&pip_cam);

        // Editor sends `pip_y` from the *bottom-left* (legacy GL
        // convention); wgpu's render-pass viewport is top-left.
        let canvas_h = vh as f32;
        let wgpu_y = (canvas_h - req.py - req.ph).max(0.0);
        let pw = req.pw.max(1.0).min(vw as f32);
        let ph = req.ph.max(1.0).min(canvas_h);
        let px = req.px.max(0.0).min(vw as f32 - 1.0);

        let mut pass =
            frame
                .encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("jengine.viewport.pip"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &frame.view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
        pass.set_viewport(px, wgpu_y, pw, ph, 0.0, 1.0);
        pass.set_scissor_rect(px as u32, wgpu_y as u32, pw as u32, ph as u32);

        let device = &gpu.renderer.device;
        let queue = &gpu.renderer.queue;
        // Drawn straight onto the display, after the post chain: the
        // display's format, not the scene's.
        let color_format = gpu.renderer.color_format;
        let sort_layers = app.assets.active_sort_layers().clone();
        let assets = &mut app.assets;
        let warned = &mut app.missing_warned;
        let pending_shader_graphs = &app.pending_shader_graphs;
        let world = &app.world;
        let sprite_batch = &mut gpu.sprite_batch;
        sprite_batch.set_target_format(device, color_format);
        let graph_pipelines = &mut gpu.graph_pipelines;
        let particle_system = &app.particle_system;
        let mut flush_by_id = |batch: &mut WgpuSpriteBatch, tex_id: &str, material_id: Option<&str>| {
            if assets.get_wgpu_texture(tex_id).is_none() {
                if !assets.has_texture_bytes(tex_id) {
                    if warned.insert(tex_id.to_string()) {
                        console_log!("viewport(pip): texture '{}' not loaded yet - asking the editor for it", tex_id);
                    }
                    batch.discard_pending();
                    return;
                }
                if let Err(e) = assets.ensure_wgpu_texture(tex_id, device, queue) {
                    if warned.insert(tex_id.to_string()) {
                        console_warn!("viewport(pip): upload failed for '{}': {}", tex_id, e);
                    }
                    batch.discard_pending();
                    return;
                }
            }
            // Slice 13d — same graph-material routing as the main pass.
            let mut pipeline_override: Option<std::sync::Arc<wgpu::RenderPipeline>> = None;
            if let Some(mid) = material_id {
                let graph_info = assets.get_material(mid).and_then(|m| {
                    m.shader_graph_path
                        .as_ref()
                        .map(|p| (p.clone(), m.blend_mode))
                });
                if let Some((graph_path, blend_mode)) = graph_info {
                    let key = GraphPipelineKey {
                        shader_graph_path: graph_path.clone(),
                        blend_mode,
                    };
                    if let Some(p) = graph_pipelines.get(&key, color_format) {
                        pipeline_override = Some(std::sync::Arc::clone(p));
                    } else if let Some(graph) = pending_shader_graphs.get(&graph_path) {
                        if let Ok(p) = graph_pipelines.insert_from_graph(
                            device,
                            color_format,
                            batch,
                            key,
                            graph,
                        ) {
                            pipeline_override = Some(p);
                        }
                    }
                }
            }
            let tex = assets.get_wgpu_texture(tex_id).expect("just ensured");
            match pipeline_override {
                Some(pipeline) => {
                    batch.flush_with_pipeline(device, queue, &mut pass, &pip_vp, tex, &pipeline);
                }
                None => {
                    batch.flush(device, queue, &mut pass, &pip_vp, tex);
                }
            }
        };
        render_tilemaps_wgpu(
            world,
            &sort_layers,
            sprite_batch,
            &pip_cam,
            &mut |b: &mut WgpuSpriteBatch, t: &str| flush_by_id(b, t, None),
        );
        render_sprites_wgpu_multi_materialized(
            world,
            &sort_layers,
            sprite_batch,
            Vec2::splat(64.0),
            &mut flush_by_id,
        );
        let mut flush_particle = |batch: &mut WgpuSpriteBatch, tex_id: Option<&str>| {
            let id = tex_id.unwrap_or("__white");
            flush_by_id(batch, id, None);
        };
        render_particles_wgpu(world, &sort_layers, particle_system, sprite_batch, &mut flush_particle);
    }

    // Fade / flash overlay, composited after everything else so the effect
    // is not bloomed or vignetted — the ordering the native runner uses.
    // The pipeline is built on first use: most sessions never fade, and it
    // owns a render pipeline.
    // A fade or flash runs on **real** time: it is the screen, not the world,
    // so a hitstop (time scale 0) must not freeze it at full brightness and
    // a slow motion must not stretch it (Shooter Classics' Void Armada).
    // dt is not threaded into `render_editor`; the play tick owns the clock
    // and leaves its real dt here. (It used to advance by `frame_time_ms` -
    // the frame's CPU work, a fraction of a millisecond - so a flash in the
    // editor lasted many times longer than asked.)
    if let Some(t) = app.fade_active.as_mut() {
        t.elapsed += app.real_dt;
        let color = t.current_color();
        let done = t.is_done();
        if app.fade_overlay.is_none() {
            match jengine_core::fade_overlay::FadeOverlay::new(
                &gpu.renderer.device,
                gpu.renderer.color_format,
            ) {
                Ok(o) => app.fade_overlay = Some(o),
                Err(e) => {
                    app.log_buffer
                        .push(format!("[viewport] fade overlay init failed: {e}"));
                    app.fade_active = None;
                }
            }
        }
        if let Some(overlay) = app.fade_overlay.as_ref() {
            overlay.render(&gpu.renderer.queue, &mut frame.encoder, &frame.view, color);
        }
        if done {
            app.fade_active = None;
        }
    }

    frame.present(&gpu.renderer.queue);
    Ok(())
}

#[wasm_bindgen]
pub fn viewport_render_pip(
    cam_x: f32,
    cam_y: f32,
    cam_zoom: f32,
    cam_viewport_w: f32,
    cam_viewport_h: f32,
    pip_x: f32,
    pip_y: f32,
    pip_w: f32,
    pip_h: f32,
) {
    // Capture the request; `render_editor` consumes it on its next
    // pass. The editor calls `viewport_render` then `viewport_render_pip`
    // back-to-back inside its raf loop, so the PiP appears one frame
    // later — invisible at 60 Hz, and avoids a second frame acquisition.
    with_app(|app| {
        app.pip_request = Some(PipRequest {
            cam_x,
            cam_y,
            cam_zoom,
            cam_vw: cam_viewport_w,
            cam_vh: cam_viewport_h,
            px: pip_x,
            py: pip_y,
            pw: pip_w,
            ph: pip_h,
        });
    });
}

// ---------------------------------------------------------------------------
// Stage 2.5 — Asset previews (particle / animation / tilemap)
//
// Each preview owns its own `<canvas>`, wgpu surface + device, sprite batch,
// and a tiny private `World` containing the single subject entity. They are
// completely independent of the main viewport so editor panels can mount
// /unmount in any order.
//
// All three previews share the same generic `PreviewState` + render loop;
// the only differences are how `start` populates the world and how `update`
// advances the simulation each frame. Fixed internal texture IDs are used
// for `*_set_texture` overrides:
//   - `__preview/particle/tex`   particle emitter override
//   - `__preview/anim/tex`       animation clip texture
//   - `__preview/tilemap/ts/{i}` tilemap tileset i
// ---------------------------------------------------------------------------

const PREVIEW_PARTICLE_TEX: &str = "__preview/particle/tex";
const PREVIEW_ANIM_TEX: &str = "__preview/anim/tex";

struct PreviewGpu {
    renderer: WgpuRenderer,
    sprite_batch: WgpuSpriteBatch,
}

struct PreviewState {
    canvas: HtmlCanvasElement,
    canvas_size: UVec2,
    init_dispatched: bool,
    gpu: Option<PreviewGpu>,
    world: World,
    entity: Option<Entity>,
    assets: AssetManager,
    missing_warned: HashSet<String>,
    camera: Camera2D,
    particle_system: ParticleSystem,
    clear_color: [f32; 4],
    /// Tilemap-only: show the grid overlay.
    show_grid: bool,
    /// Tilemap-only: hovered tile (in tile coords) for highlight.
    highlight_tile: Option<(i32, i32)>,
}

impl PreviewState {
    fn new(canvas: HtmlCanvasElement, w: u32, h: u32) -> Self {
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes(
            "__white".to_string(),
            RawTextureData {
                width: 1,
                height: 1,
                rgba: vec![255, 255, 255, 255],
            },
        );
        Self {
            canvas,
            canvas_size: UVec2::new(w.max(1), h.max(1)),
            init_dispatched: false,
            gpu: None,
            world: World::new(),
            entity: None,
            assets,
            missing_warned: HashSet::new(),
            camera: Camera2D::new(w as f32, h as f32),
            particle_system: ParticleSystem::new(),
            clear_color: [0.08, 0.08, 0.10, 1.0],
            show_grid: false,
            highlight_tile: None,
        }
    }
}

#[derive(Copy, Clone)]
enum PreviewSlot {
    Particle,
    Anim,
    Tilemap,
}

thread_local! {
    static PARTICLE_PREVIEW: RefCell<Option<Rc<RefCell<PreviewState>>>> = const { RefCell::new(None) };
    static ANIM_PREVIEW: RefCell<Option<Rc<RefCell<PreviewState>>>> = const { RefCell::new(None) };
    static TILEMAP_PREVIEW: RefCell<Option<Rc<RefCell<PreviewState>>>> = const { RefCell::new(None) };
}

fn slot_cell(slot: PreviewSlot) -> &'static std::thread::LocalKey<RefCell<Option<Rc<RefCell<PreviewState>>>>> {
    match slot {
        PreviewSlot::Particle => &PARTICLE_PREVIEW,
        PreviewSlot::Anim => &ANIM_PREVIEW,
        PreviewSlot::Tilemap => &TILEMAP_PREVIEW,
    }
}

fn with_preview<R>(slot: PreviewSlot, f: impl FnOnce(&mut PreviewState) -> R) -> Option<R> {
    slot_cell(slot).with(|cell| {
        let outer = cell.borrow();
        let rc = outer.as_ref()?.clone();
        drop(outer);
        let mut s = rc.borrow_mut();
        Some(f(&mut s))
    })
}

fn preview_set(slot: PreviewSlot, rc: Rc<RefCell<PreviewState>>) {
    slot_cell(slot).with(|cell| *cell.borrow_mut() = Some(rc));
}

fn init_preview(slot: PreviewSlot, canvas_id: &str) -> Result<(), JsValue> {
    let canvas = get_canvas(canvas_id)?;
    let w = canvas.width().max(1);
    let h = canvas.height().max(1);
    let state = PreviewState::new(canvas, w, h);
    let rc = Rc::new(RefCell::new(state));
    preview_set(slot, rc.clone());
    spawn_preview_gpu(rc);
    Ok(())
}

fn spawn_preview_gpu(rc: Rc<RefCell<PreviewState>>) {
    {
        let mut s = rc.borrow_mut();
        if s.init_dispatched || s.gpu.is_some() {
            return;
        }
        s.init_dispatched = true;
    }
    let canvas = rc.borrow().canvas.clone();
    let size = rc.borrow().canvas_size;
    wasm_bindgen_futures::spawn_local(async move {
        match WgpuRenderer::new_from_canvas_async(canvas, size).await {
            Ok(mut renderer) => {
                let sb = match WgpuSpriteBatch::new(&renderer.device, renderer.color_format) {
                    Ok(b) => b,
                    Err(e) => {
                        console_error!("preview WgpuSpriteBatch init failed: {e}");
                        return;
                    }
                };
                let cc = rc.borrow().clear_color;
                renderer.set_clear_color(cc[0], cc[1], cc[2], cc[3]);
                rc.borrow_mut().gpu = Some(PreviewGpu { renderer, sprite_batch: sb });
            }
            Err(e) => console_error!("preview WebGPU init failed: {e}"),
        }
    });
}

/// Generic preview render: clear, draw tilemaps + sprites + particles for
/// the preview's tiny world. Tilemap slot also overlays grid + highlight.
fn render_preview(slot: PreviewSlot) {
    let _ = with_preview(slot, |s| {
        let UVec2 { x: vw, y: vh } = s.canvas_size;
        s.camera.resize(vw as f32, vh as f32);
        let mut camera = s.camera;
        // A preview is its own little world: draw all of it. As the main
        // pass it would skip whatever matched the scene's canvas-group
        // members (`ecs::canvas_group`), which are entities of another world.
        camera.pass = jengine_core::renderer::camera::CameraPass::View;
        let view_projection = camera.view_projection();
        // What this pass can see, for the sprite passes to cull against.
        jengine_core::renderer::frame_view::set_frame_view(&camera);

        let gpu = match s.gpu.as_mut() {
            Some(g) => g,
            None => return,
        };
        let mut frame = match gpu.renderer.begin_frame() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                gpu.renderer.resize(s.canvas_size);
                return;
            }
            Err(e) => {
                console_warn!("preview surface error: {e:?}");
                return;
            }
        };

        let device = &gpu.renderer.device;
        let queue = &gpu.renderer.queue;
        let sort_layers = s.assets.active_sort_layers().clone();
        let assets = &mut s.assets;
        let warned = &mut s.missing_warned;
        let world = &s.world;
        let sprite_batch = &mut gpu.sprite_batch;
        let particle_system = &s.particle_system;
        let show_grid = s.show_grid;
        let highlight_tile = s.highlight_tile;

        {
            let mut pass = frame.begin_main_pass();

            let mut flush_by_id = |batch: &mut WgpuSpriteBatch, tex_id: &str| {
                if assets.get_wgpu_texture(tex_id).is_none() {
                    if !assets.has_texture_bytes(tex_id) {
                        if warned.insert(tex_id.to_string()) {
                            console_warn!("preview: missing texture '{}'", tex_id);
                        }
                        batch.discard_pending();
                        return;
                    }
                    if let Err(e) = assets.ensure_wgpu_texture(tex_id, device, queue) {
                        if warned.insert(tex_id.to_string()) {
                            console_warn!("preview: upload failed for '{}': {}", tex_id, e);
                        }
                        batch.discard_pending();
                        return;
                    }
                }
                let tex = assets.get_wgpu_texture(tex_id).expect("just ensured");
                batch.flush(device, queue, &mut pass, &view_projection, tex);
            };

            render_tilemaps_wgpu(world, &sort_layers, sprite_batch, &camera, &mut flush_by_id);
            render_sprites_wgpu_multi(world, &sort_layers, sprite_batch, Vec2::splat(64.0), &mut flush_by_id);
            let mut flush_particle = |batch: &mut WgpuSpriteBatch, tex_id: Option<&str>| {
                let id = tex_id.unwrap_or("__white");
                flush_by_id(batch, id);
            };
            render_particles_wgpu(world, &sort_layers, particle_system, sprite_batch, &mut flush_particle);

            // Tilemap-only overlays: grid + hovered tile highlight.
            if matches!(slot, PreviewSlot::Tilemap) {
                draw_tilemap_overlay(
                    world,
                    sprite_batch,
                    show_grid,
                    highlight_tile,
                    &mut flush_by_id,
                );
            }
        }

        frame.present(&gpu.renderer.queue);
    });
}

/// Draw an axis-aligned grid + a hovered-tile highlight for the tilemap
/// preview. Uses the `__white` 1x1 texture; sprite batch already supports
/// rectangular quads via `draw_sprite_anchored`.
fn draw_tilemap_overlay(
    world: &World,
    batch: &mut WgpuSpriteBatch,
    show_grid: bool,
    highlight: Option<(i32, i32)>,
    flush: &mut dyn FnMut(&mut WgpuSpriteBatch, &str),
) {
    use jengine_core::tilemap::Tilemap;
    // Where the map is drawn: its world origin, carried by a parent if it has one.
    let mut subjects: Vec<(glam::Vec2, Tilemap)> = world
        .query::<(&Transform, &Tilemap)>()
        .iter()
        .map(|(e, (t, tm))| (jengine_core::tilemap::world_origin(world, e, t), tm.clone()))
        .collect();
    if subjects.is_empty() {
        return;
    }
    let (origin, tilemap) = subjects.remove(0);

    let grid_max = tilemap
        .layers
        .iter()
        .map(|l| (l.width, l.height))
        .fold((0u32, 0u32), |(w, h), (lw, lh)| (w.max(lw), h.max(lh)));
    if grid_max.0 == 0 || grid_max.1 == 0 {
        return;
    }
    let tw = tilemap.tile_size[0];
    let th = tilemap.tile_size[1];
    let gw = grid_max.0 as f32 * tw;
    let gh = grid_max.1 as f32 * th;
    // `origin` is the top-left of tile (0,0).

    batch.begin();
    let line_thickness = 1.0;
    let grid_color = glam::Vec4::new(1.0, 1.0, 1.0, 0.12);

    // A hexagonal map's grid is every cell's hexagon outlined (shared edges
    // drawn twice, which the faint colour hides), and the highlight one of
    // them in gold. `Tilemap::cell_outline` is the shape the renderer and
    // the colliders use too.
    if tilemap.is_hex() {
        let line = |batch: &mut WgpuSpriteBatch, a: Vec2, b: Vec2, width: f32, color: glam::Vec4| {
            let d = b - a;
            batch.draw_sprite_anchored((a + b) * 0.5, Vec2::new(d.length(), width), d.y.atan2(d.x), color, [0.0, 0.0, 1.0, 1.0], [0.5, 0.5]);
        };
        let outline = |batch: &mut WgpuSpriteBatch, c: i32, r: i32, width: f32, color: glam::Vec4| {
            let pts: Vec<Vec2> = tilemap.cell_outline(c, r).into_iter().map(|(x, y)| origin + Vec2::new(x, y)).collect();
            for i in 0..pts.len() {
                line(batch, pts[i], pts[(i + 1) % pts.len()], width, color);
            }
        };
        if show_grid {
            for r in 0..grid_max.1 as i32 {
                for c in 0..grid_max.0 as i32 {
                    outline(batch, c, r, line_thickness, grid_color);
                }
            }
        }
        if let Some((hx, hy)) = highlight
            && hx >= 0
            && hy >= 0
            && (hx as u32) < grid_max.0
            && (hy as u32) < grid_max.1
        {
            outline(batch, hx, hy, 2.0, glam::Vec4::new(1.0, 0.85, 0.2, 0.9));
        }
        flush(batch, "__white");
        return;
    }

    // An isometric map's grid runs along its diagonals: every line is a
    // turned quad between two grid points, and the highlight is the cell's
    // diamond outlined.
    if tilemap.orientation == jengine_core::tilemap::TileOrientation::Isometric {
        let at = |c: f32, r: f32| {
            let (x, y) = tilemap.grid_to_local(c, r);
            origin + Vec2::new(x, y)
        };
        let line = |batch: &mut WgpuSpriteBatch, a: Vec2, b: Vec2, width: f32, color: glam::Vec4| {
            let d = b - a;
            batch.draw_sprite_anchored((a + b) * 0.5, Vec2::new(d.length(), width), d.y.atan2(d.x), color, [0.0, 0.0, 1.0, 1.0], [0.5, 0.5]);
        };
        let (cols, rows) = (grid_max.0 as f32, grid_max.1 as f32);
        if show_grid {
            for col in 0..=grid_max.0 {
                line(batch, at(col as f32, 0.0), at(col as f32, rows), line_thickness, grid_color);
            }
            for row in 0..=grid_max.1 {
                line(batch, at(0.0, row as f32), at(cols, row as f32), line_thickness, grid_color);
            }
        }
        if let Some((hx, hy)) = highlight
            && hx >= 0
            && hy >= 0
            && (hx as u32) < grid_max.0
            && (hy as u32) < grid_max.1
        {
            let (c, r) = (hx as f32, hy as f32);
            let corners = [at(c, r), at(c + 1.0, r), at(c + 1.0, r + 1.0), at(c, r + 1.0)];
            for i in 0..4 {
                line(batch, corners[i], corners[(i + 1) % 4], 2.0, glam::Vec4::new(1.0, 0.85, 0.2, 0.9));
            }
        }
        flush(batch, "__white");
        return;
    }

    if show_grid {
        // vertical lines
        for col in 0..=grid_max.0 {
            let x = origin.x + col as f32 * tw;
            batch.draw_sprite_anchored(
                Vec2::new(x, origin.y - gh * 0.5),
                Vec2::new(line_thickness, gh),
                0.0,
                grid_color,
                [0.0, 0.0, 1.0, 1.0],
                [0.5, 0.5],
            );
        }
        // horizontal lines
        for row in 0..=grid_max.1 {
            let y = origin.y - row as f32 * th;
            batch.draw_sprite_anchored(
                Vec2::new(origin.x + gw * 0.5, y),
                Vec2::new(gw, line_thickness),
                0.0,
                grid_color,
                [0.0, 0.0, 1.0, 1.0],
                [0.5, 0.5],
            );
        }
    }

    if let Some((hx, hy)) = highlight {
        if hx >= 0 && hy >= 0 && (hx as u32) < grid_max.0 && (hy as u32) < grid_max.1 {
            let cx = origin.x + hx as f32 * tw + tw * 0.5;
            let cy = origin.y - hy as f32 * th - th * 0.5;
            batch.draw_sprite_anchored(
                Vec2::new(cx, cy),
                Vec2::new(tw, th),
                0.0,
                glam::Vec4::new(1.0, 0.85, 0.2, 0.30),
                [0.0, 0.0, 1.0, 1.0],
                [0.5, 0.5],
            );
        }
    }

    flush(batch, "__white");
}

// ---------------------------------------------------------------------------
// Particle preview
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn viewport_particle_preview_init(canvas_id: &str) -> Result<(), JsValue> {
    init_preview(PreviewSlot::Particle, canvas_id)
}

#[wasm_bindgen]
pub fn viewport_particle_preview_start(emitter_json: &str) -> Result<(), JsValue> {
    let emitter: ParticleEmitter = serde_json::from_str(emitter_json)
        .map_err(|e| JsValue::from_str(&format!("particle emitter json: {e}")))?;
    with_preview(PreviewSlot::Particle, |s| {
        // Wipe old subject (keep particle pool — it'll fade naturally).
        if let Some(e) = s.entity.take() {
            let _ = s.world.despawn(e);
        }
        s.particle_system = ParticleSystem::new();
        let mut emitter = emitter;
        // The preview only has two textures it can resolve: the built-in
        // `__white` 1x1 quad, and whatever bytes the editor uploaded under
        // `PREVIEW_PARTICLE_TEX`. If the emitter's `texture_id` points at a
        // project asset (e.g. "sprites/white32.png") that we never loaded,
        // the renderer would silently discard every particle. So:
        //   - If the override sentinel is already uploaded → use it.
        //   - Otherwise → clear texture_id so particles render as white
        //     quads. `viewport_particle_preview_set_texture` will patch the
        //     live emitter to use the sentinel once the bytes arrive.
        if s.assets.has_texture_bytes(PREVIEW_PARTICLE_TEX) {
            emitter.texture_id = Some(PREVIEW_PARTICLE_TEX.to_string());
        } else if let Some(id) = emitter.texture_id.as_deref() {
            if !s.assets.has_texture_bytes(id) {
                emitter.texture_id = None;
            }
        }
        let entity = s.world.spawn((
            Transform {
                position: Vec2::ZERO,
                rotation: 0.0,
                scale: Vec2::ONE,
            },
            emitter,
        ));
        s.entity = Some(entity);
    })
    .ok_or_else(|| JsValue::from_str("particle preview not initialized"))
}

#[wasm_bindgen]
pub fn viewport_particle_preview_update(dt: f32) {
    with_preview(PreviewSlot::Particle, |s| {
        // IMPORTANT: pool key must match what `render_particles_wgpu` uses
        // when it calls `draw_data` — namely `entity.id() as u64`. Using
        // `to_bits().get()` (NonZeroU64 with generation bits) would store
        // pools under a key the renderer never looks up, leaving the
        // preview canvas blank even though alive_count > 0.
        jengine_core::particles::tick_particles(&s.world, &mut s.particle_system, dt);
    });
}

#[wasm_bindgen]
pub fn viewport_particle_preview_render() {
    render_preview(PreviewSlot::Particle);
}

#[wasm_bindgen]
pub fn viewport_particle_preview_stop() {
    with_preview(PreviewSlot::Particle, |s| {
        if let Some(e) = s.entity.take() {
            let _ = s.world.despawn(e);
        }
        s.particle_system = ParticleSystem::new();
    });
}

#[wasm_bindgen]
pub fn viewport_particle_preview_burst(count: u32) {
    with_preview(PreviewSlot::Particle, |s| {
        let entity = match s.entity {
            Some(e) => e,
            None => return,
        };
        // Snapshot to avoid borrow conflicts.
        let snap: Option<(u64, ParticleEmitter, Vec2)> = s
            .world
            .query_one::<(&Transform, &ParticleEmitter)>(entity)
            .ok()
            // `entity.id()`, the key `update` and the renderer use. This took
            // `to_bits()` - the generation-tagged id the comment in
            // `viewport_particle_preview_update` warns about - so the burst went
            // into a pool nothing drew, and `update` pruned it next frame.
            .and_then(|mut q| q.get().map(|(t, em)| (entity.id() as u64, em.clone(), t.position)));
        if let Some((id, em, pos)) = snap {
            s.particle_system.burst(id, &em, pos, count);
        }
    });
}

#[wasm_bindgen]
pub fn viewport_particle_preview_resize(width: u32, height: u32) {
    with_preview(PreviewSlot::Particle, |s| {
        let size = UVec2::new(width.max(1), height.max(1));
        s.canvas_size = size;
        s.camera.resize(size.x as f32, size.y as f32);
        if let Some(g) = s.gpu.as_mut() {
            g.renderer.resize(size);
        }
    });
}

#[wasm_bindgen]
pub fn viewport_particle_preview_set_texture(data: &[u8]) -> Result<(), JsValue> {
    with_preview(PreviewSlot::Particle, |s| match RawTextureData::from_bytes(data) {
        Ok(raw) => {
            s.assets.insert_texture_bytes(PREVIEW_PARTICLE_TEX.to_string(), raw);
            s.missing_warned.remove(PREVIEW_PARTICLE_TEX);
            // Patch the live emitter, if any.
            if let Some(entity) = s.entity {
                if let Ok(mut em) = s.world.get::<&mut ParticleEmitter>(entity) {
                    em.texture_id = Some(PREVIEW_PARTICLE_TEX.to_string());
                }
            }
            Ok(())
        }
        Err(e) => Err(JsValue::from_str(&format!("particle texture decode: {e}"))),
    })
    .unwrap_or_else(|| Err(JsValue::from_str("particle preview not initialized")))
}

#[wasm_bindgen]
pub fn viewport_particle_preview_clear_texture() {
    with_preview(PreviewSlot::Particle, |s| {
        if let Some(entity) = s.entity {
            if let Ok(mut em) = s.world.get::<&mut ParticleEmitter>(entity) {
                em.texture_id = None;
            }
        }
    });
}

#[wasm_bindgen]
pub fn viewport_particle_preview_alive_count() -> u32 {
    with_preview(PreviewSlot::Particle, |s| s.particle_system.alive_count()).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Animation preview
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn viewport_anim_preview_init(canvas_id: &str) -> Result<(), JsValue> {
    init_preview(PreviewSlot::Anim, canvas_id)
}

#[wasm_bindgen]
pub fn viewport_anim_preview_start(clip_json: &str) -> Result<(), JsValue> {
    use jengine_core::animation::{AnimationClip, SpriteAnimator};
    let clip: AnimationClip = serde_json::from_str(clip_json)
        .map_err(|e| JsValue::from_str(&format!("anim clip json: {e}")))?;
    with_preview(PreviewSlot::Anim, |s| {
        if let Some(e) = s.entity.take() {
            let _ = s.world.despawn(e);
        }
        let clip_name = clip.name.clone();
        let mut animator = SpriteAnimator::default();
        animator.clips.insert(clip_name.clone(), clip);
        animator.play(&clip_name);
        // A frame may name an atlas region instead of carrying a uv rect. The
        // runtime resolves those through `resolve_sprite_regions`; this preview
        // builds its animator by hand and so has to do it itself, or every
        // region-named frame keeps the default full-texture rect and the
        // preview shows the whole sheet.
        animator.regions_resolved =
            animator.resolve_frame_regions(Some(PREVIEW_ANIM_TEX), &s.assets);

        // First-frame sprite snapshot. The texture id always points at the
        // preview override slot when one has been uploaded; otherwise it
        // uses whatever the clip's first frame referenced.
        let (uv, frame_tex) = animator
            .current_frame_result()
            .map(|r| (r.uv_rect, r.texture_id))
            .unwrap_or(([0.0, 0.0, 1.0, 1.0], None));
        let texture_id = if s.assets.has_texture_bytes(PREVIEW_ANIM_TEX) {
            PREVIEW_ANIM_TEX.to_string()
        } else {
            frame_tex.unwrap_or_else(|| "__white".to_string())
        };
        let sprite = SpriteComponent {
            texture_id: texture_id.into(),
            color: [1.0, 1.0, 1.0, 1.0],
            z_order: 0,
            uv_rect: Some(uv),
            size: Some([128.0, 128.0]),
            anchor: [0.5, 0.5],
            fill: None,
            sort_layer: None,
            region: None,
            region_resolved: false,
            corners: None,
            ..SpriteComponent::default()
        };
        let entity = s.world.spawn((
            Transform { position: Vec2::ZERO, rotation: 0.0, scale: Vec2::ONE },
            sprite,
            animator,
        ));
        s.entity = Some(entity);
    })
    .ok_or_else(|| JsValue::from_str("anim preview not initialized"))
}

#[wasm_bindgen]
pub fn viewport_anim_preview_update(dt: f32) {
    use jengine_core::animation::SpriteAnimator;
    with_preview(PreviewSlot::Anim, |s| {
        let entity = match s.entity {
            Some(e) => e,
            None => return,
        };
        // Tick animator first, then patch sprite uv/texture.
        let result = s
            .world
            .get::<&mut SpriteAnimator>(entity)
            .ok()
            .and_then(|mut a| a.tick(dt));
        if let Some(r) = result {
            if let Ok(mut sprite) = s.world.get::<&mut SpriteComponent>(entity) {
                sprite.uv_rect = Some(r.uv_rect);
                if !s.assets.has_texture_bytes(PREVIEW_ANIM_TEX) {
                    if let Some(tex) = r.texture_id {
                        sprite.texture_id = tex.into();
                    }
                }
            }
        }
    });
}

#[wasm_bindgen]
pub fn viewport_anim_preview_render() {
    render_preview(PreviewSlot::Anim);
}

#[wasm_bindgen]
pub fn viewport_anim_preview_stop() {
    with_preview(PreviewSlot::Anim, |s| {
        if let Some(e) = s.entity.take() {
            let _ = s.world.despawn(e);
        }
    });
}

#[wasm_bindgen]
pub fn viewport_anim_preview_set_frame(idx: u32) {
    use jengine_core::animation::SpriteAnimator;
    with_preview(PreviewSlot::Anim, |s| {
        let entity = match s.entity {
            Some(e) => e,
            None => return,
        };
        let snap = if let Ok(mut a) = s.world.get::<&mut SpriteAnimator>(entity) {
            a.current_frame = idx as usize;
            a.elapsed = 0.0;
            a.current_frame_result()
        } else {
            None
        };
        if let Some(r) = snap {
            if let Ok(mut sprite) = s.world.get::<&mut SpriteComponent>(entity) {
                sprite.uv_rect = Some(r.uv_rect);
                if !s.assets.has_texture_bytes(PREVIEW_ANIM_TEX) {
                    if let Some(tex) = r.texture_id {
                        sprite.texture_id = tex.into();
                    }
                }
            }
        }
    });
}

#[wasm_bindgen]
pub fn viewport_anim_preview_set_texture(data: &[u8]) -> Result<(), JsValue> {
    with_preview(PreviewSlot::Anim, |s| match RawTextureData::from_bytes(data) {
        Ok(raw) => {
            s.assets.insert_texture_bytes(PREVIEW_ANIM_TEX.to_string(), raw);
            s.missing_warned.remove(PREVIEW_ANIM_TEX);
            if let Some(entity) = s.entity {
                if let Ok(mut sprite) = s.world.get::<&mut SpriteComponent>(entity) {
                    sprite.texture_id = PREVIEW_ANIM_TEX.into();
                }
            }
            Ok(())
        }
        Err(e) => Err(JsValue::from_str(&format!("anim texture decode: {e}"))),
    })
    .unwrap_or_else(|| Err(JsValue::from_str("anim preview not initialized")))
}

/// Give the animation preview the packed atlas its sheet's regions are named in.
///
/// `atlas_json` is **packed metadata** (`width` / `height` / `regions`) - the
/// shape a `.atlas` beside the sheet carries, not an atlas *definition*.
/// Registered under the preview's own texture id, which is the id
/// `resolve_frame_regions` is asked to look regions up in, so the preview needs
/// no notion of the project's real texture path.
///
/// Safe to call after `viewport_anim_preview_start`: it re-resolves the live
/// animator, so the editor may load the sheet and the atlas in either order.
#[wasm_bindgen]
pub fn viewport_anim_preview_set_atlas(atlas_json: &str) -> Result<(), JsValue> {
    use jengine_core::animation::SpriteAnimator;
    let data: jengine_core::renderer::atlas::AtlasData = serde_json::from_str(atlas_json)
        .map_err(|e| JsValue::from_str(&format!("anim atlas json: {e}")))?;
    with_preview(PreviewSlot::Anim, |s| {
        s.assets.register_atlas(PREVIEW_ANIM_TEX.to_string(), data);
        let Some(entity) = s.entity else {
            return Ok(());
        };
        let snap = if let Ok(mut a) = s.world.get::<&mut SpriteAnimator>(entity) {
            a.regions_resolved = a.resolve_frame_regions(Some(PREVIEW_ANIM_TEX), &s.assets);
            a.current_frame_result()
        } else {
            None
        };
        if let Some(r) = snap
            && let Ok(mut sprite) = s.world.get::<&mut SpriteComponent>(entity)
        {
            sprite.uv_rect = Some(r.uv_rect);
        }
        Ok(())
    })
    .unwrap_or_else(|| Err(JsValue::from_str("anim preview not initialized")))
}

#[wasm_bindgen]
pub fn viewport_anim_preview_resize(width: u32, height: u32) {
    with_preview(PreviewSlot::Anim, |s| {
        let size = UVec2::new(width.max(1), height.max(1));
        s.canvas_size = size;
        s.camera.resize(size.x as f32, size.y as f32);
        if let Some(g) = s.gpu.as_mut() {
            g.renderer.resize(size);
        }
    });
}

#[wasm_bindgen]
pub fn viewport_anim_preview_current_frame() -> u32 {
    use jengine_core::animation::SpriteAnimator;
    with_preview(PreviewSlot::Anim, |s| {
        let entity = s.entity?;
        s.world
            .get::<&SpriteAnimator>(entity)
            .ok()
            .map(|a| a.current_frame as u32)
    })
    .flatten()
    .unwrap_or(0)
}

#[wasm_bindgen]
pub fn viewport_anim_preview_is_playing() -> bool {
    use jengine_core::animation::SpriteAnimator;
    with_preview(PreviewSlot::Anim, |s| {
        let entity = s.entity?;
        s.world
            .get::<&SpriteAnimator>(entity)
            .ok()
            .map(|a| a.is_playing())
    })
    .flatten()
    .unwrap_or(false)
}

#[wasm_bindgen]
pub fn viewport_anim_preview_play() {
    use jengine_core::animation::SpriteAnimator;
    with_preview(PreviewSlot::Anim, |s| {
        if let Some(entity) = s.entity {
            if let Ok(mut a) = s.world.get::<&mut SpriteAnimator>(entity) {
                a.playing = true;
            }
        }
    });
}

#[wasm_bindgen]
pub fn viewport_anim_preview_pause() {
    use jengine_core::animation::SpriteAnimator;
    with_preview(PreviewSlot::Anim, |s| {
        if let Some(entity) = s.entity {
            if let Ok(mut a) = s.world.get::<&mut SpriteAnimator>(entity) {
                a.playing = false;
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Tilemap preview
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub fn viewport_tilemap_preview_init(canvas_id: &str) -> Result<(), JsValue> {
    init_preview(PreviewSlot::Tilemap, canvas_id)
}

#[wasm_bindgen]
pub fn viewport_tilemap_preview_load(tilemap_json: &str) -> Result<(), JsValue> {
    use jengine_core::tilemap::Tilemap;
    let tilemap: Tilemap = serde_json::from_str(tilemap_json)
        .map_err(|e| JsValue::from_str(&format!("tilemap json: {e}")))?;
    with_preview(PreviewSlot::Tilemap, |s| {
        if let Some(e) = s.entity.take() {
            let _ = s.world.despawn(e);
        }
        // Anchor the map at the origin so tile (0,0) sits at world (0,0).
        let entity = s.world.spawn((
            Transform { position: Vec2::ZERO, rotation: 0.0, scale: Vec2::ONE },
            tilemap,
        ));
        s.entity = Some(entity);
    });
    Ok(())
}

/// The tilemap editor's `autotile` rule (`tilemap::autotile::Autotile`) on
/// the preview map, as the entity being edited carries it - so an autotiled
/// layer draws its terrains on the dual grid as it will play. Empty or
/// `null` takes it off. The editor calls it after every
/// `viewport_tilemap_preview_load`, which makes a fresh entity.
#[wasm_bindgen]
pub fn viewport_tilemap_preview_set_autotile(autotile_json: &str) -> Result<(), JsValue> {
    use jengine_core::tilemap::autotile::Autotile;
    // (Not a string `match`: the host-parity guard reads string arms in this
    // file as script command tags.)
    let text = autotile_json.trim();
    let rule: Option<Autotile> = if text.is_empty() || text == "null" {
        None
    } else {
        Some(serde_json::from_str(text).map_err(|e| JsValue::from_str(&format!("autotile json: {e}")))?)
    };
    with_preview(PreviewSlot::Tilemap, |s| {
        let Some(e) = s.entity else { return };
        match rule {
            Some(r) => {
                let _ = s.world.insert_one(e, r);
            }
            None => {
                let _ = s.world.remove_one::<Autotile>(e);
            }
        }
    });
    Ok(())
}

#[wasm_bindgen]
pub fn viewport_tilemap_preview_render() {
    render_preview(PreviewSlot::Tilemap);
}

#[wasm_bindgen]
pub fn viewport_tilemap_preview_resize(width: u32, height: u32) {
    with_preview(PreviewSlot::Tilemap, |s| {
        let size = UVec2::new(width.max(1), height.max(1));
        s.canvas_size = size;
        s.camera.resize(size.x as f32, size.y as f32);
        if let Some(g) = s.gpu.as_mut() {
            g.renderer.resize(size);
        }
    });
}

#[wasm_bindgen]
pub fn viewport_tilemap_preview_set_camera(x: f32, y: f32, zoom: f32) {
    with_preview(PreviewSlot::Tilemap, |s| {
        s.camera.position = Vec2::new(x, y);
        s.camera.zoom = zoom.max(0.01);
    });
}

#[wasm_bindgen]
pub fn viewport_tilemap_preview_set_tile(layer_idx: u32, x: u32, y: u32, gid: i32) {
    use jengine_core::tilemap::Tilemap;
    with_preview(PreviewSlot::Tilemap, |s| {
        let entity = match s.entity {
            Some(e) => e,
            None => return,
        };
        if let Ok(mut tm) = s.world.get::<&mut Tilemap>(entity) {
            if let Some(layer) = tm.layers.get_mut(layer_idx as usize) {
                let id = if gid < 0 { None } else { Some(gid as u32) };
                layer.set_tile(x, y, id);
            }
        }
    });
}

#[wasm_bindgen]
pub fn viewport_tilemap_preview_load_tileset_texture(
    idx: u32,
    data: &[u8],
) -> Result<(), JsValue> {
    use jengine_core::tilemap::Tilemap;
    let tex_id = format!("__preview/tilemap/ts/{idx}");
    with_preview(PreviewSlot::Tilemap, |s| match RawTextureData::from_bytes(data) {
        Ok(raw) => {
            s.assets.insert_texture_bytes(tex_id.clone(), raw);
            s.missing_warned.remove(&tex_id);
            // Repoint the matching tileset def at the override id so the
            // renderer's tileset bucket key resolves to our uploaded bytes.
            if let Some(entity) = s.entity {
                if let Ok(mut tm) = s.world.get::<&mut Tilemap>(entity) {
                    if let Some(ts) = tm.tilesets.get_mut(idx as usize) {
                        ts.texture_id = tex_id.clone();
                    }
                }
            }
            Ok(())
        }
        Err(e) => Err(JsValue::from_str(&format!("tileset texture decode: {e}"))),
    })
    .unwrap_or_else(|| Err(JsValue::from_str("tilemap preview not initialized")))
}

#[wasm_bindgen]
pub fn viewport_tilemap_preview_screen_to_tile(screen_x: f32, screen_y: f32) -> Vec<i32> {
    use jengine_core::tilemap::Tilemap;
    with_preview(PreviewSlot::Tilemap, |s| {
        let entity = match s.entity {
            Some(e) => e,
            None => return vec![0, 0],
        };
        let tm = match s.world.get::<&Tilemap>(entity) {
            Ok(t) => t,
            Err(_) => return vec![0, 0],
        };
        // Browser screen coords: origin top-left, +Y down. Camera2D uses
        // world-space Y-up. Convert: world = cam.position + (screen - half) * (1/zoom),
        // then flip Y.
        let half_w = s.canvas_size.x as f32 * 0.5;
        let half_h = s.canvas_size.y as f32 * 0.5;
        let zoom = s.camera.zoom.max(0.0001);
        let world_x = s.camera.position.x + (screen_x - half_w) / zoom;
        let world_y = s.camera.position.y - (screen_y - half_h) / zoom;
        let entity_pos = s
            .world
            .get::<&Transform>(entity)
            .map(|t| t.position)
            .unwrap_or(Vec2::ZERO);
        let (tx, ty) = tm.world_to_tile(entity_pos.x, entity_pos.y, world_x, world_y);
        vec![tx, ty]
    })
    .unwrap_or_else(|| vec![0, 0])
}

#[wasm_bindgen]
pub fn viewport_tilemap_preview_set_highlight(x: i32, y: i32) {
    with_preview(PreviewSlot::Tilemap, |s| {
        s.highlight_tile = if x < 0 || y < 0 { None } else { Some((x, y)) };
    });
}

#[wasm_bindgen]
pub fn viewport_tilemap_preview_set_grid(show: bool) {
    with_preview(PreviewSlot::Tilemap, |s| s.show_grid = show);
}

#[wasm_bindgen]
pub fn viewport_tilemap_preview_get_camera() -> Vec<f32> {
    with_preview(PreviewSlot::Tilemap, |s| {
        vec![s.camera.position.x, s.camera.position.y, s.camera.zoom]
    })
    .unwrap_or_else(|| vec![0.0, 0.0, 1.0])
}

// ---------------------------------------------------------------------------
// Stage 2.1 — Play-mode tick + canvas input listeners.
//
// Mirrors `jengine_runtime::wgpu_scene_runner::tick_world` but runs in
// the browser. Scripts, behavior graphs, physics, and path libraries
// are deliberately OUT for this slice; they land in Stage 2.2+.
// ---------------------------------------------------------------------------

/// Turn the device preview's safe-area fractions into the pixels `UiScale`
/// works in, for the canvas as it is now.
fn apply_safe_fraction(app: &mut ViewportApp) {
    let (w, h) = (app.canvas_size.x as f32, app.canvas_size.y as f32);
    let [l, t, r, b] = app.safe_fraction;
    app.ui_scale.safe = jengine_core::ui::scaling::SafeInsets {
        left: l * w,
        top: t * h,
        right: r * w,
        bottom: b * h,
    };
}

/// The editor's device preview: pretend the canvas is a phone whose
/// cut-outs cover these fractions of it (left, top, right, bottom). Both UI
/// systems and the touch controls lay out clear of them exactly as on the
/// device; the world still fills the canvas. All zero turns it off.
#[wasm_bindgen]
pub fn viewport_set_safe_area(left: f32, top: f32, right: f32, bottom: f32) {
    with_app(|app| {
        let clamp = |v: f32| if v.is_finite() { v.clamp(0.0, 0.45) } else { 0.0 };
        app.safe_fraction = [clamp(left), clamp(top), clamp(right), clamp(bottom)];
        apply_safe_fraction(app);
    });
}

/// The device preview is a touch screen worked with a mouse: show layouts
/// that wait for a touch (`show: "touch"`), and let the mouse press them.
#[wasm_bindgen]
pub fn viewport_set_touch_preview(on: bool) {
    with_app(|app| app.touch.assume_touch_screen(on));
}

/// The scene's on-screen touch controls, if an entity carries some.
fn touch_controls_of(world: &hecs::World) -> Option<jengine_core::ecs::components::TouchControls> {
    world
        .query::<&jengine_core::ecs::components::TouchControls>()
        .iter()
        .next()
        .map(|(_, c)| c.clone())
}

fn tick_play_world(app: &mut ViewportApp) {
    use jengine_core::ecs::behaviors;
    use jengine_core::ecs::systems as sys;

    // Stage 2.8 — reset sub-stage accumulators; we additively bank
    // physics + script time inside the tick and fold the rest into
    // `update_time_ms` at the end.
    let tick_start = perf_now_ms();
    let mut physics_ms: f32 = 0.0;
    let mut script_ms: f32 = 0.0;

    // dt from `performance.now()`, paced onto the display's refresh grid and
    // clamped to a sane upper bound — same policy as the native runner,
    // prevents burst-tick after a window blur / debugger pause.
    let now = perf_now_ms();
    // Through the input replay (`input::replay`).
    let real_dt = jengine_core::input::replay::frame_dt(match app.last_tick_ms.replace(now) {
        Some(prev) => {
            let raw = ((now - prev) / 1000.0) as f32;
            let rate = app.refresh_estimate.observe(raw);
            app.pacer.set_refresh_millihertz(rate);
            app.pacer.snap(raw).clamp(0.0, 0.1)
        }
        None => 0.0,
    });
    app.real_dt = real_dt;
    let dt = real_dt * app.time_scale;
    app.trail_dt += dt;

    // On-screen touch controls first, so the keys they hold reach every
    // reader this frame. Only the mouse can work them here (`show: "always"`).
    {
        let controls = touch_controls_of(&app.world);
        let (cw, ch) = (app.canvas_size.x as f32, app.canvas_size.y as f32);
        apply_safe_fraction(app);
        app.touch.tick(controls.as_ref(), &mut app.input, &app.ui_scale, (cw, ch));
    }

    // Pads before anything reads actions - and every frame, not only when
    // some script runs, which is where this used to be: a scene whose input
    // reached graphs or components alone never saw a controller in the
    // editor. The Gamepad API is poll-only, and a browser reports no pads
    // until one is pressed, so an empty result is normal.
    jengine_core::input::gamepad_web::poll_gamepads(&mut app.input);
    // Live ops: a fetch when one is due (`jengine_core::liveops`).
    jengine_core::liveops::web::sync();
    // A focused text field types through the hidden textarea: what it got
    // goes into this frame's input *before* the fields read it (and before a
    // replay keeps the frame's input). Synced after them (beside the
    // scripts, where it was), its characters were cleared by `begin_frame`
    // unread, so once the proxy had focus nothing typed reached any field.
    app.text_proxy.sync(&app.canvas, &app.ui_scale, &mut app.input);
    // Hold / tap / double-tap timing, now every device has reported.
    app.input.update_interactions();

    let world = &mut app.world;
    let input = &app.input;

    behaviors::update_input_actions(world, input);

    // Parity with the native runner's per-frame `sync_instances`: bind
    // graphs for entities spawned mid-play (script `spawn_prefab`, behavior
    // `SpawnEntity`) and - what this used to skip - drop the instances of
    // despawned ones, firing `OnDespawn`. The viewport has no filesystem, so
    // the JSON comes from `pending_behaviors` (`viewport_load_behavior`).
    if app.playing {
        for e in app
            .behavior_system
            .sync_instances_inline(&mut app.world, &app.pending_behaviors)
        {
            app.log_buffer
                .push(format!("[viewport] behavior attach '{}' failed: {}", e.path.display(), e.source));
        }
    }

    // Phase 2 of the runtime pipeline — behavior graphs tick before
    // motion/animation so reactions land on this frame's transform.
    let s0 = perf_now_ms();
    // The stepped physics pose back in every body's transform before
    // anything this frame reads one (the last frame drew the blend).
    if let Some(physics) = app.physics.as_mut() {
        physics.restore_stepped_poses(&mut app.world);
    }
    jengine_core::profiling::trace::time("behavior_graphs", || {
        app.behavior_system.tick_pre_simulation(&mut app.world, dt)
    });
    script_ms += (perf_now_ms() - s0) as f32;

    // The motion step every host shares. Batch scripts do not run here, so
    // an entity carrying one moves by its components.
    behaviors::tick_motion(&mut app.world, dt, app.path_library.as_ref(), false);
    // `AudioSource { auto_play: true }` — a sound carried by the entity
    // rather than played by a script (`WebAudioBackend::start_autoplay`).
    let log = &mut app.log_buffer;
    app.web_audio.start_autoplay(&mut app.world, &mut app.mixer, &mut app.spatial_audio, &mut |m| {
        log.push(format!("[viewport] {m}"))
    });
    // Positional audio: each spatial source at the volume and pan of where it
    // is now, and the sounds of sources that have gone, stopped.
    app.web_audio.apply_spatial(&app.world, &mut app.mixer, &mut app.spatial_audio);
    let world = &mut app.world;

    // Stage 2.3 — physics step. Mirrors the package-system
    // `physics_step` lambda in `packages/jengine-physics/src/lib.rs`,
    // minus the script-command application (no scripts in viewport).
    // Borrow shuffle: drop `world` reborrow, run physics (which
    // takes `&app.world` and `&mut app.world` plus `app.physics`
    // and `app.behavior_system`), then reborrow `world` for the
    // remainder of the pipeline.
    let _ = world;
    // ── package systems: PreUpdate + ScriptExec ───────────────────────────
    // Same phase placement as the native runner, so a package behaves the
    // same in the editor as in a build.
    app.package_host.run_phase_with(
        SystemPhase::PreUpdate,
        &mut app.world,
        dt,
        jengine_core::package::host::FrameResources { input: &mut app.input, assets: &mut app.assets, camera: app.camera },
    );
    app.package_host.run_phase_with(
        SystemPhase::ScriptExec,
        &mut app.world,
        dt,
        jengine_core::package::host::FrameResources { input: &mut app.input, assets: &mut app.assets, camera: app.camera },
    );

    let mut pending_contacts: Vec<(u32, u32, bool, f32, f32, f32)> = Vec::new();
    let fixed_fns = fixed_update_fns(app);
    if app.physics.is_some() {
        let p0 = perf_now_ms();
        let fixed_steps = {
            let physics = app.physics.as_mut().expect("checked above");
            jengine_core::ecs::systems::physics_frame_begin(&mut app.world, physics, dt);
            if fixed_fns.is_empty() { None } else { Some((physics.begin_frame(dt), physics.fixed_dt())) }
        };
        let contacts = match fixed_steps {
            None => app.physics.as_mut().map(|p| p.step(dt)).unwrap_or_default(),
            // `fixed_update` scripts: one call before each fixed step, each
            // reading the world as the step before it left it - as native.
            Some((steps, fixed_dt)) => {
                for _ in 0..steps {
                    run_fixed_update(app, &fixed_fns, fixed_dt);
                    let Some(physics) = app.physics.as_mut() else { break };
                    jengine_core::ecs::systems::physics_fixed_step(&mut app.world, physics, fixed_dt);
                }
                app.physics.as_mut().map(|p| p.end_frame()).unwrap_or_default()
            }
        };
        // A script's scene load can replace the physics world mid-frame.
        if let Some(physics) = app.physics.as_mut() {
        jengine_core::ecs::systems::physics_frame_end(&mut app.world, physics, &contacts, &mut app.particle_system, dt);
        physics_ms += (perf_now_ms() - p0) as f32;
        // Route contacts into behavior graphs — same as the native
        // host's Phase 6 collision integration.
        let s1 = perf_now_ms();
        let _disp = app
            .behavior_system
            .dispatch_contact_events(&contacts, &mut app.world);
        // Stash contacts for the browser-script phase below so
        // user scripts get `on_collision_enter` / `on_collision_exit`
        // in the same tick. `ContactEvent.entity_a/b` are already
        // u32-shaped (`.id() as u64`), matching `script_fns` keys.
        for ev in &contacts {
            let started =
                matches!(ev.event_type, jengine_core::physics::ContactType::Started);
            pending_contacts.push((ev.entity_a as u32, ev.entity_b as u32, started, ev.impulse, ev.point.0, ev.point.1));
        }
        script_ms += (perf_now_ms() - s1) as f32;
        }
    }

    // F4: collider and joint outlines, the same key and the same shapes as
    // native and web (`debug_draw::physics_debug_commands`).
    if app.input.is_just_pressed(KeyCode::F4) {
        app.physics_debug = !app.physics_debug;
    }
    if app.physics_debug {
        for cmd in jengine_core::debug_draw::physics_debug_commands(&app.world) {
            app.debug_draws.push(cmd);
        }
    }
    behaviors::update_trigger_areas(&mut app.world);

    // The graph barrier, right after the trigger areas - where every host
    // has it since Oct 2026 (this one had it after combat, animation and
    // the lifetimes): trigger overlaps dispatched, graph input fired, and
    // the structural commands (spawn, despawn, attach) flushed, so combat
    // sees what the graphs did this frame.
    let s2 = perf_now_ms();
    // Graph input triggers (`OnKey`, `OnMouseClick`) - the same core call in
    // every runtime; nothing fired them before (BUGS #93).
    app.behavior_system.dispatch_input(&app.input, &mut app.world);
    let _trig_dispatched = app
        .behavior_system
        .dispatch_trigger_overlaps_from_world(&mut app.world);
    let routed = app.behavior_system.flush_commands_and_route_events(
        &mut app.world,
        Some(&jengine_core::behavior::PrefabSource {
            registry: &app.registry,
            prefabs: &app.prefabs,
        }),
    );
    // Graphs -> scripts: each graph event reaches the `on_<name>` listeners
    // with `{ value, x, y }`, the same payload native builds. Their commands
    // drain with the next frame's, as an animation event's do.
    for ev in &routed.emit_event_requests {
        let data_js = JsValue::from_str(&ev.script_payload().to_string());
        dispatch_event_to_listeners(app, &ev.name, 0, &data_js, None);
    }
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
            app.log_buffer.push(format!(
                "[Audio] behavior PlaySound('{}') - {e} (further occurrences suppressed)",
                r.sound_id
            ));
        }
    }
    if let Some(path) = routed.change_scene_requests.last() {
        post_scene_load(path);
    }
    script_ms += (perf_now_ms() - s2) as f32;


    // Combat — trigger-hitbox damage, i-frames, death, health mirror.
    let world = &mut app.world;
    let sprite_events = sys::tick_combat_and_animation(world, &app.assets, dt);
    // Typed characters land in the focused GUI TextInput. `world` is a
    // reborrow of `app.world`, so this goes through `app` directly.
    let _ = world;
    jengine_core::ecs::systems::apply_text_input(
        &mut app.world,
        &mut app.assets,
        &mut app.input,
    );
    // Menus without a pointer: arrows / D-pad / stick move focus, A / Enter
    // press it, Tab walks the document. A press goes where a click goes.
    let nav = jengine_core::ecs::systems::navigate_ui(
        &mut app.world,
        &mut app.assets,
        &mut app.input,
        &app.ui_scale,
        dt,
    );
    for (gui_id, node_id) in &nav.gui_activations {
        let _ = dispatch_gui_click_impl(app, gui_id, node_id);
    }
    for button in nav.ui_clicks {
        let data = jengine_core::ecs::systems::ui_click_payload(button);
        let data_js = JsValue::from_str(&data.to_string());
        dispatch_event_to_listeners(app, jengine_core::ecs::systems::UI_CLICK_EVENT, 0, &data_js, None);
    }
    // What a clip's frames fired: a sound plays here, an event goes to the
    // script on that entity as `on_animation_event`. Both were computed and
    // dropped (`let _sprite_events = ...`) in every runtime until Sep 2026.
    if !sprite_events.is_empty() {
        let (sounds, events) = jengine_core::animation::split_triggers(&sprite_events);
        for (_, clip, volume) in sounds {
            if let Err(e) = app.web_audio.play_sound_on_bus(
                &mut app.mixer,
                jengine_core::audio::SFX_BUS,
                &clip,
                volume,
                1.0,
                0.0,
                false,
            ) && app.audio_warned.insert(clip.clone())
            {
                app.log_buffer
                    .push(format!("[Audio] animation frame '{clip}' — {e} (further occurrences suppressed)"));
            }
        }
        dispatch_animation_events(app, &events);
    }
    let world = &mut app.world;

    let expired = behaviors::update_lifetimes(world, dt);
    for e in expired {
        let _ = world.despawn(e);
    }
    // Anything carrying `DespawnOffscreen` that has left the view. Every
    // scrolling game used to walk its own list in a script and compare each
    // thing against the camera by hand.
    {
        let view = (app.camera.position, app.camera.half_extents());
        let travel = app.camera.position - app.prev_camera;
        let gone = behaviors::despawn_offscreen(&mut app.world, view, travel);
        for e in gone {
            // Tweens are keyed by the real id, and hecs hands this id to the next
            // spawn - a tween left running would drive that newcomer instead.
            app.tween_manager.cancel_all(e.id() as u64);
            let _ = app.world.despawn(e);
        }
        app.prev_camera = app.camera.position;
        // Videos (`jengine_core::video`): this frame's pictures into their
        // textures, before anything draws them.
        app.video.tick(&mut app.world, &mut app.assets, dt);
        // `visibility_notifier`s that came on screen or left it.
        jengine_core::ecs::visibility::tick_visibility(&mut app.world, view);
        // What the engine owes scripts (`ecs::callbacks`): those, a
        // `path_follow` at its end ...
        for owed in jengine_core::ecs::callbacks::take() {
            call_lifecycle_handler(app, owed.entity.id(), owed.handler);
        }
    }

    // ── package systems: Physics + PostPhysics ────────────────────────────
    // The built-in physics package registers in `Physics` and no-ops here:
    // the viewport steps rapier directly above and does not share
    // `PhysicsWorld` as a resource.
    app.package_host.run_phase_with(
        SystemPhase::Physics,
        &mut app.world,
        dt,
        jengine_core::package::host::FrameResources { input: &mut app.input, assets: &mut app.assets, camera: app.camera },
    );
    app.package_host.run_phase_with(
        SystemPhase::PostPhysics,
        &mut app.world,
        dt,
        jengine_core::package::host::FrameResources { input: &mut app.input, assets: &mut app.assets, camera: app.camera },
    );

    // Hit-test the mouse against registered GUI click callbacks BEFORE
    // the script update phase, so a button click and the script
    // response land on the same frame. Mirrors `process_gui_clicks`
    // in the native runner; the viewport's `gui_click_callbacks` map
    // was populated by the `goc` command earlier this tick.
    // Wheel scrolling of `Overflow::Scroll` GUI containers, ahead of the
    // click hit-test so both agree on where the rows are this frame. Shared
    // with the native runner and the web runtime - see
    // `ecs::systems::apply_gui_wheel_scroll`.
    // UI space is virtual pixels (see `ui::scaling`), so the pointer is
    // divided into it before any UI hit test. World-space mouse handling is
    // untouched: `input.mouse_position` stays physical for everyone else.
    let screen = app
        .ui_scale
        .virtual_size(app.canvas_size.x as f32, app.canvas_size.y as f32);
    let ui_mouse = app.ui_scale.to_ui(
        app.input.mouse_position.0,
        app.input.mouse_position.1,
        app.canvas_size.x as f32,
        app.canvas_size.y as f32,
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
        for button in clicked {
            let data = jengine_core::ecs::systems::ui_click_payload(button);
            let data_js = JsValue::from_str(&data.to_string());
            dispatch_event_to_listeners(app, jengine_core::ecs::systems::UI_CLICK_EVENT, 0, &data_js, None);
        }
    }
    // Release, not press: a scroll drag must not also press the button it started on.
    // Buttons therefore activate on *release*, which is what every touch
    // surface and desktop toolkit does anyway, and `GuiScrollState` vetoes the
    // release when the gesture turned out to be a scroll. Firing on press made
    // dragging a list of buttons impossible: the press alone activated one.
    if app.input.mouse_left_just_released
        && !app.gui_scroll.is_dragging()
        && !app.gui_click_callbacks.is_empty()
    {
        dispatch_pending_gui_clicks(app);
    }

    // Stage 2.9 — browser script update phase. Refresh `__jengine_play`
    // from the live world (positions, rotations, scales, names, dt,
    // input snapshots), invoke each entity's `update(entity_id, dt)`,
    // pull mutations back to the ECS, then drain queued commands
    // (log, debug_draw, set_position, destroy_entity).
    if app.play_namespace_ready && !app.script_fns.is_empty() {
        let s3 = perf_now_ms();
        let _scripts = jengine_core::profiling::trace::span("scripts");
        // What the game asked of the page last frame: fullscreen, the
        // cursor. Play in the editor is deliberately uncapped, so no cap.
        app.display.sync(&app.canvas, &app.assets, &mut app.missing_warned, None);
        // Refresh the pathfinding snapshot before any script runs, so a
        // `tilemap_find_path` call sees this frame's tiles and never touches
        // the live world.
        app.nav_snapshot.borrow_mut().refresh_named(&app.world, &script_names());
        // And the ray snapshot, for the same reason and with the same
        // timing. No `cfg` on it either, for the reason immediately below:
        // `physics` is a core feature this crate turns on unconditionally
        // and has no feature of its own by that name, so a
        // `cfg(feature = "physics")` here would be permanently false.
        app.ray_snapshot.borrow_mut().refresh_named(&app.world, &script_names());
        publish_physics_for_scripts(app, &pending_contacts);
        sync_world_to_play_namespace(app);
        set_play_namespace_dt(dt);
        // `on_spawn`, `update`, timers, triggers and contacts - shared with
        // the web runtime.
        let log = &mut app.log_buffer;
        jengine_core::scripting::bridge::adapters::js::scripts::run_frame(
            &app.script_fns,
            &mut app.spawn_notified,
            &app.world,
            dt,
            &pending_contacts,
            &mut |m| log.push(format!("[viewport] {m}")),
        );
        // The drain writes back the script's transforms first; this catches
        // what callbacks fired during the drain wrote.
        drain_play_commands_into(app);
        sync_play_namespace_to_world(app);
        script_ms += (perf_now_ms() - s3) as f32;
    }

    // Stage 2.9c \u2014 advance script-driven tweens. Runs after the script
    // tick so this frame's `tween_position` / `tween_color` calls land in
    // the manager before we sample them; the first sample applies a 0%
    // value (equal to start) so motion only begins next frame, matching
    // the native runner.
    sys::tick_tweens(&mut app.world, &mut app.tween_manager, dt);

    // Particle simulation in play mode (editor mode also calls this
    // path inside render_editor — but only the playing tick advances
    // emission timers, so we drive it here too).
    jengine_core::particles::tick_particles(&app.world, &mut app.particle_system, dt);

    // Apply the active `Camera2DComponent` (zoom, viewport size,
    // position) and smooth toward any `CameraFollow` target. Only
    // active in Game-tab mode (signaled by JS via
    // `viewport_set_camera_size`); in Scene-tab mode the user
    // controls the editor camera freely while play ticks.
    if app.game_viewport.is_some() {
        // To the canvas first, as native and web do: `update_camera` fits the
        // camera to whatever size it holds, and the render leaves it at a
        // fitted world size - harmless to refit, but an integer-scaled view
        // (`Camera2D::integer_view`) needs the canvas's pixels.
        let UVec2 { x: cw, y: ch } = app.canvas_size;
        app.camera.resize(cw as f32, ch as f32);
        sys::update_camera(&app.world, &mut app.camera, dt);
    }
    // P1.3 — drain camera-shake requests queued by scripts this
    // tick (via the browser-JS `cmsh` command dispatcher) and
    // advance the shake decay. Only meaningful in Game-tab mode but
    // harmless to run unconditionally — a zero-amplitude shake is a
    // no-op.
    for (amp, dur, freq) in app.pending_camera_shakes.drain(..) {
        app.camera.start_shake(amp, dur, freq);
    }
    app.camera.tick_effects(dt);

    // Slice 3.5.C — advance skeletal animation players in play mode.
    // The resolver call in `render_editor` will then pick up the new
    // bone poses on the same frame. Edit mode intentionally does NOT
    // run this so the inspector can author bind / static poses
    // without playback fighting the user.
    jengine_core::animation::skeletal::update_skeletal_players(
        &mut app.world,
        app.assets.skeletal_clips(),
        dt,
    );
    // Slice 3.5.F — apply IK constraints (only in play mode, mirroring
    // the skeletal player). The render_editor resolver call picks up
    // the resulting pose on the same frame.
    jengine_core::animation::ik::solve_ik(&mut app.world);

    // ── package systems: Animation + PreRender ────────────────────────────
    // `PreRender` is where `parallax_scroll` lives, and it reads the active
    // camera, so it has to run after everything that could move the camera
    // this tick. `Render` / `PostRender` are not run here for the same reason
    // as natively: no renderer is shared through the `ResourceMap`, so a draw
    // system would run and silently draw nothing.
    app.package_host.run_phase_with(
        SystemPhase::Animation,
        &mut app.world,
        dt,
        jengine_core::package::host::FrameResources { input: &mut app.input, assets: &mut app.assets, camera: app.camera },
    );
    app.package_host.run_phase_with(
        SystemPhase::PreRender,
        &mut app.world,
        dt,
        jengine_core::package::host::FrameResources { input: &mut app.input, assets: &mut app.assets, camera: app.camera },
    );
    // Sprites a package spawned or re-regioned in the phases since the last
    // resolve draw right on their first frame (BUGS #110).
    jengine_core::ecs::systems::resolve_sprite_regions(&mut app.world, &app.assets);

    // Stage 2.8 — fold totals into FrameStats. `update_ms` is
    // tick-total minus physics minus script (i.e. velocity, behaviors,
    // animations, particles, lifetime cleanup).
    let total_ms = (perf_now_ms() - tick_start) as f32;
    app.frame_stats.physics_time_ms = physics_ms;
    app.frame_stats.script_time_ms = script_ms;
    app.frame_stats.update_time_ms = (total_ms - physics_ms - script_ms).max(0.0);
}

// ---------------------------------------------------------------------------
// Stage 2.9 — Browser-script bridge helpers
//
// The compiled scripts produced by `BrowserScriptFns` read engine state and
// queue mutations through a `window.__jengine_play` JS namespace populated
// by these helpers each tick. We use a minimal subset of the legacy
// glow-viewport namespace: positions/rotations/scales/sprite_colors/names
// + dt + commands array + debug_draws sink. The full surface (action
// bindings, key-just-pressed, mouse, custom components, etc.) lands in
// Stage 2.9b.
// ---------------------------------------------------------------------------

/// The per-frame publish, shared with the other browser host
/// (`adapters::js::mirror`): this host lends what it reads.
fn sync_world_to_play_namespace(app: &ViewportApp) {
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
fn sync_play_namespace_to_world(app: &mut ViewportApp) {
    mirror::sync_play_namespace_to_world(&mut mirror::PlayWorldMut { world: &mut app.world });
}


/// Read a numeric field off a JS command object. Returns `None` when the
/// field is missing or not coerceable to f64.
fn js_value_f64(obj: &js_sys::Object, key: &str) -> Option<f64> {
    js_sys::Reflect::get(obj, &JsValue::from_str(key))
        .ok()
        .and_then(|v| v.as_f64())
}



fn js_value_string(obj: &js_sys::Object, key: &str) -> Option<String> {
    js_sys::Reflect::get(obj, &JsValue::from_str(key))
        .ok()
        .and_then(|v| v.as_string())
}

/// Find a live entity by raw id. Linear scan — fine because the editor's
/// scenes are small (hundreds of entities at most) and command bursts are
/// rare per tick.
///
/// Stage 2.9e — also consults `SCRIPT_ENTITY_MAP` first so script-side
/// `idx` ids returned from `spawn_entity()` / `spawn_prefab()` resolve
/// to the real `hecs::Entity` they were mapped to at spawn time. The
/// map entry is only honoured when the entity still exists (despawned
/// entries fall through to the world scan and return `None`).
/// Lend the world and the script-id table to the shared command handler.
///
/// `resolve_entity` is why this trait exists rather than the shared code
/// scanning for itself: `SCRIPT_ENTITY_MAP` below is per-host state, so a
/// handler that scanned the world would disagree with this host on any id the
/// table has remapped.
impl jengine_core::scripting::host_commands::ScriptWorldHost for ViewportApp {
    fn world(&mut self) -> &mut World {
        &mut self.world
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        find_entity_by_id(&self.world, id)
    }
}

/// Build a patrol-path library from a scene's declared routes.
///
/// `None` for a scene with no routes, which is what `update_patrol_paths`
/// expects and what keeps the common case free.
fn build_path_library(
    paths: std::collections::HashMap<String, jengine_core::ecs::path::Path2D>,
) -> Option<jengine_core::ecs::path::Path2DLibrary> {
    if paths.is_empty() {
        return None;
    }
    let mut lib = jengine_core::ecs::path::Path2DLibrary::new();
    for (id, path) in paths {
        lib.insert(id, path);
    }
    Some(lib)
}

/// Stage 2.9i — compile + attach browser scripts and run their `init`.
/// Used by `viewport_play` (initial play) and the scene-transition
/// paths so scripts get rebuilt against the new world. Also stashes
/// `scripts_json` in `app.last_scripts_json` so push/pop preserves it.
/// Clears `script_fns`, `script_cache`, `debug_draws`,
/// `gui_click_callbacks`, `tween_manager`, and `SCRIPT_ENTITY_MAP`
/// before reloading so stale handles from the previous world can't
/// leak into the new one.
/// Ready the play namespace for the scripts about to run: the world
/// published and the synchronous hooks installed.
fn prepare_play_namespace(app: &mut ViewportApp) {
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
        app.log_buffer.push(format!("[viewport] install_nav_callbacks failed: {e:?}"));
    }
    // The same, for `raycast`. The preamble has always called
    // `state.physics_raycast` and no host has ever installed it, so the
    // function returned null in the editor from the day it was specified.
    app.ray_snapshot.borrow_mut().refresh_named(&app.world, &script_names());
    if let Err(e) = jengine_core::scripting::bridge::adapters::js::ray::install_ray_callback(
        std::rc::Rc::clone(&app.ray_snapshot),
    ) {
        app.log_buffer.push(format!("[viewport] install_ray_callback failed: {e:?}"));
    }
    app.play_namespace_ready = true;
}
/// `Scene.load_additive` / `preload` requests for the editor to read files
/// for, as JSON - `[{ "kind": "additive", "handle", "path", "x", "y" } |
/// { "kind": "preload", "path" }]`, empty when there are none. An unload is
/// done here, there being nothing to read. The editor calls this each frame
/// in Play and answers with [`viewport_add_scene_chunk`] or
/// [`viewport_set_stream_status`].
#[wasm_bindgen]
pub fn viewport_take_stream_requests() -> String {
    use jengine_core::scene::streaming::{self as st, Request};
    let requests = st::take_requests();
    if requests.is_empty() {
        return String::new();
    }
    let mut out = Vec::new();
    with_app(|app| {
        for req in requests {
            match req {
                Request::Unload { handle } => {
                    for e in st::unload_chunk(&mut app.world, handle) {
                        app.script_fns.remove(&e.id());
                    }
                }
                Request::Additive { handle, path, offset } => out.push(serde_json::json!({
                    "kind": "additive", "handle": handle, "path": path, "x": offset[0], "y": offset[1],
                })),
                Request::Preload { path } => out.push(serde_json::json!({ "kind": "preload", "path": path })),
            }
        }
    });
    serde_json::to_string(&out).unwrap_or_default()
}

/// Lay a streamed scene into the running one as chunk `handle`, offset by
/// `(x, y)`, and start its scripts (`scripts_json` as for a scene
/// transition, `entity_index` counting the scene file's entities). A chunk
/// unloaded while the editor read its files is dropped.
#[wasm_bindgen]
pub fn viewport_add_scene_chunk(handle: u32, scene_json: &str, scripts_json: &str, x: f32, y: f32) -> Result<(), JsValue> {
    use jengine_core::scene::streaming::{self as st, Status};
    if !st::is_wanted(handle) {
        return Ok(());
    }
    with_app(|app| {
        let scene_json = jengine_core::scene::expand_prefab_instances_in_json(scene_json, &app.prefabs, "viewport");
        let mut chunk = World::new();
        if let Err(e) = load_scene_auto(&scene_json, &mut chunk, &app.registry) {
            st::set_status(handle, Status::Failed);
            return Err(JsValue::from_str(&format!("parse chunk: {e}")));
        }
        let pairs = st::merge(&mut app.world, chunk, handle, glam::Vec2::new(x, y));
        let sources = serde_json::from_str::<Vec<ScriptSource>>(scripts_json).unwrap_or_default();
        let mut started = Vec::new();
        {
            let log = &mut app.log_buffer;
            let mut warn = |m: String| log.push(format!("[viewport] {m}"));
            for src in &sources {
                let Some(&(_, entity)) = pairs.get(src.entity_index) else { continue };
                let label = jengine_core::scripting::bridge::adapters::js::scripts::label_for(&src.source_path, src.entity_index);
                if let Some(inst) = jengine_core::scripting::bridge::adapters::js::scripts::attach(
                    &mut app.script_fns,
                    &mut app.script_cache,
                    entity.id(),
                    &src.source_path,
                    label,
                    &src.source,
                    &src.language,
                    &mut warn,
                ) {
                    started.push((entity.id(), inst));
                }
            }
        }
        if !started.is_empty() {
            if app.play_namespace_ready {
                sync_world_to_play_namespace(app);
            } else {
                prepare_play_namespace(app);
            }
            let log = &mut app.log_buffer;
            jengine_core::scripting::bridge::adapters::js::scripts::run_inits(&started, &mut |m| log.push(format!("[viewport] {m}")));
            drain_play_commands_into(app);
        }
        st::set_status(handle, Status::Loaded);
        app.log_buffer.push(format!("[stream] chunk {handle} laid in ({} entities)", pairs.len()));
        Ok(())
    })
    .unwrap_or(Ok(()))
}

/// The editor read a preload (`kind: "preload"`, `handle` < 0, by `path`)
/// or failed a chunk: `status` is `"loaded"` or `"failed"`.
#[wasm_bindgen]
pub fn viewport_set_stream_status(handle: i32, path: &str, status: &str) {
    use jengine_core::scene::streaming::{self as st, Status};
    let s = if status == "loaded" { Status::Loaded } else { Status::Failed };
    if handle >= 0 {
        st::set_status(handle as u32, s);
    } else {
        st::set_preload(path, s);
    }
}

fn load_and_run_scripts(app: &mut ViewportApp, scripts_json: &str) {
    app.script_fns.clear();
    app.script_cache.clear();
    app.debug_draws.clear();
    app.gui_click_callbacks.clear();
    // Stage 2.9k — stale `lds` requests from the previous world
    // would resolve against entity ids that no longer exist.
    app.pending_script_loads.clear();
    app.tween_manager = jengine_core::animation::tween::TweenManager::new();
    app.play_namespace_ready = false;
    // Spawn handles, published tilemaps, the scene's timers and listeners.
    jengine_core::scripting::bridge::adapters::js::scripts::forget_scene();
    app.last_scripts_json = scripts_json.to_string();
    let sources = match serde_json::from_str::<Vec<ScriptSource>>(scripts_json) {
        Ok(s) => s,
        Err(e) => {
            // Empty payloads are common (scenes without scripts) — only
            // log when the JSON was non-trivial.
            if !scripts_json.trim().is_empty() && scripts_json.trim() != "[]" {
                app.log_buffer
                    .push(format!("[viewport] scripts_json parse failed: {e}"));
            }
            return;
        }
    };
    if sources.is_empty() {
        return;
    }
    let mut started = Vec::new();
    let mut errors = 0usize;
    {
        let log = &mut app.log_buffer;
        let mut warn = |m: String| log.push(format!("[viewport] {m}"));
        for src in &sources {
            let Some(&entity) = app.scene_entities.get(src.entity_index) else {
                warn(format!("script entity_index {} out of range", src.entity_index));
                errors += 1;
                continue;
            };
            let label = jengine_core::scripting::bridge::adapters::js::scripts::label_for(&src.source_path, src.entity_index);
            match jengine_core::scripting::bridge::adapters::js::scripts::attach(
                &mut app.script_fns,
                &mut app.script_cache,
                entity.id(),
                &src.source_path,
                label,
                &src.source,
                &src.language,
                &mut warn,
            ) {
                Some(inst) => started.push((entity.id(), inst)),
                None => errors += 1,
            }
        }
    }
    if !started.is_empty() || errors > 0 {
        app.log_buffer.push(format!("[viewport] scripts: loaded={}, errors={errors}", started.len()));
    }
    if started.is_empty() {
        return;
    }
    prepare_play_namespace(app);
    let log = &mut app.log_buffer;
    jengine_core::scripting::bridge::adapters::js::scripts::run_inits(&started, &mut |m| log.push(format!("[viewport] {m}")));
    drain_play_commands_into(app);
    sync_play_namespace_to_world(app);
}

/// Push the current world transforms / sprite colors / names into the JS
/// namespace. Called once per frame before invoking `update()`.









fn set_play_namespace_dt(dt: f32) {
    if let Some(play) = play_namespace() {
        let _ = js_sys::Reflect::set(
            &play,
            &JsValue::from_str("dt"),
            &JsValue::from_f64(dt as f64),
        );
    }
}


/// Drain `__jengine_play.commands`. A command goes to the shared world arms
/// (`host_commands`), then the shared host arms (`host_dispatch`), and only
/// what neither knows reaches this host's own `match` - the ones that call
/// into its script instances or scene loading. An unknown tag is noted in the
/// log once per tick.
fn drain_play_commands_into(app: &mut ViewportApp) {
    let Some(play) = play_namespace() else { return };
    // What the scripts wrote through `set_position` and kin reaches the world
    // before their commands do, as it does natively: `juice:burst` moves an
    // emitter and bursts it in one call, and drained first the burst came
    // from where the emitter last was. It also keeps a `fixed_update`'s
    // writes, which the next publish's reset of the dirty lists discarded.
    // (A write to an entity spawned this frame is a deferred `dtw` command,
    // so nothing here waits on the drain registering spawns.)
    sync_play_namespace_to_world(app);
    let cmds = match js_sys::Reflect::get(&play, &JsValue::from_str("commands")) {
        Ok(v) if v.is_instance_of::<js_sys::Array>() => v.unchecked_into::<js_sys::Array>(),
        _ => return,
    };
    let mut unhandled_kinds: HashSet<String> = HashSet::new();
    // Snapshot the length before iteration. Script callbacks fired during
    // dispatch (e.g. `on_event` invoking `log()`) push NEW commands onto
    // this same array; we must only consume up to `snapshot_len` so the
    // tail-trim below doesn't discard them. New tail entries are drained
    // on the next frame.
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
        // file and `jengine-web/src/script_host.rs`, where nothing could test
        // them — they only ran in a browser. See `docs/architecture-debt.md` #2.
        if jengine_core::scripting::host_commands::handle_world_command(
            app,
            kind.as_str(),
            &jengine_core::scripting::host_commands::JsArgs(&cmd_obj),
        ) {
            continue;
        }
        // Then package-contributed commands, dispatched through the same
        // function the native runtime uses - one implementation of every
        // package command, reached from every runtime.
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
                warn: &mut |m: String| app.log_buffer.push(format!("[viewport] {m}")),
                post_process: app.gpu.as_mut().map(|g| &mut g.post_process),
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
                if let Some(m) = js_sys::Reflect::get(&cmd_obj, &JsValue::from_str("m"))
                    .ok()
                    .and_then(|v| v.as_string())
                {
                    app.log_buffer.push(m);
                }
            }

            // quit_game(). There is no window to close in the editor, so the
            // honest equivalent is to stop Play.
            "quit" => {
                app.log_buffer.push(
                    "[viewport] quit_game() — stopping play".to_string(),
                );
                app.playing = false;
            }
            // Dynamically load a script into a live entity. Compiles via
            // the same `compile_browser_script` path used at play start;
            // overwrites any prior entry so re-loading is idempotent.
            "lds" => {
                let eid = js_value_f64(&cmd_obj, "e").unwrap_or(-1.0) as i64;
                let path = js_value_string(&cmd_obj, "p").unwrap_or_default();
                if eid < 0 || path.is_empty() {
                    continue;
                }
                // Stage 2.9k — the editor owns the asset DB, so we
                // queue the request; editor calls
                // `viewport_drain_pending_script_loads` after each
                // `viewport_render`, fetches the source, then calls
                // `viewport_inject_script(eid, source, language, path)`
                // to compile + register + init it. Idempotent: if the
                // editor pushes a duplicate path, the latest source
                // simply overwrites the prior `script_fns` entry.
                app.pending_script_loads.push((eid as u32, path));
            }
            other => {
                unhandled_kinds.insert(other.to_string());
            }
        }
    }
    publish_save_data(app, &play);
    // After every command this drain applied: forget finished sounds and push
    // any bus change (`ab` above) to the sounds still playing.
    app.web_audio.sync_mixer(&mut app.mixer);
    // The music - crossfades, layers, a playlist - and its playhead.
    jengine_core::audio::music::drive(&mut jengine_core::audio::web_audio::WebOut {
        audio: &mut app.web_audio,
        mixer: &mut app.mixer,
    });
    // Remove only the entries we processed; any pushes that happened
    // during this drain (script-callback side-effects, e.g. `log()` from
    // inside `on_event`) are preserved at the new front of the array and
    // will be drained next frame. We invoke `Array.prototype.splice(0, N)`
    // via Reflect so we pass exactly 2 arguments — `js_sys::Array::splice`
    // takes a mandatory `item` parameter that would otherwise insert
    // `undefined` at index 0.
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
    if !unhandled_kinds.is_empty() {
        let mut kinds: Vec<String> = unhandled_kinds.into_iter().collect();
        kinds.sort();
        app.log_buffer.push(format!(
            "[viewport] script commands not yet implemented: {}",
            kinds.join(",")
        ));
    }
}

/// Write `gravity` and `contacts` onto `window.__jengine_play.state`, the
/// object `get_gravity()` / `get_contacts()` read. Contacts are
/// `[[a, b, started], ...]`, matching the `(a, b, started)` triples the
/// native snapshot exposes.
/// Gravity, contacts and velocities for the scripts about to run. Velocities
/// go through the same two functions native's snapshot uses, so
/// `get_velocity` means the same thing in every runtime: the component
/// value, overlaid with rapier's for rigid bodies.
fn publish_physics_for_scripts(app: &ViewportApp, contacts: &[(u32, u32, bool, f32, f32, f32)]) {
    mirror::publish_physics_for_scripts(&app.world, app.physics.as_ref(), contacts);
}

/// Every running script's `fixed_update`, if it defines one.
fn fixed_update_fns(app: &ViewportApp) -> Vec<(u32, JsValue)> {
    if !app.play_namespace_ready {
        return Vec::new();
    }
    jengine_core::scripting::bridge::adapters::js::scripts::fixed_update_fns(&app.script_fns)
}

/// One fixed step's scripts: the world as the last step left it published,
/// each `fixed_update(entity, dt)` called, their commands applied.
fn run_fixed_update(app: &mut ViewportApp, fns: &[(u32, JsValue)], fixed_dt: f32) {
    publish_physics_for_scripts(app, &[]);
    sync_world_to_play_namespace(app);
    set_play_namespace_dt(fixed_dt);
    for (eid, f) in fns {
        if let Err(e) = call_script_fn(f, *eid, fixed_dt) {
            app.log_buffer.push(format!("[viewport] fixed_update on entity {eid} failed: {e}"));
        }
    }
    drain_play_commands_into(app);
}


/// Mirror the save store onto `window.__jengine_play.save_data`.
///
/// `save_get` has to answer synchronously - Lua has no await - so the script
/// side reads a plain object rather than calling into Rust. Republishing
/// after each drain is what makes a value written this frame, or lazily
/// loaded from storage by `commit`, visible to the next `save_get`.
///
/// Only slots the session has touched are published; `SaveSystem` loads a
/// slot lazily on first access, so an untouched slot has nothing to mirror
/// yet and `save_get` on it correctly returns nil until it is read natively.
fn publish_save_data(app: &ViewportApp, play: &js_sys::Object) {
    let snapshot = app.save_system.snapshot_all();
    if snapshot.is_empty() {
        return;
    }
    let as_json = serde_json::to_value(&snapshot).unwrap_or(serde_json::Value::Null);
    let value = json_to_jsvalue(&as_json);
    let _ = js_sys::Reflect::set(play, &JsValue::from_str("save_data"), &value);
}

/// Set on a keydown by the editor's key router (`editor/src/lib/api/playKeys.ts`)
/// when the editor keeps a press during play - a play control, a Scene-view
/// tool, a keystroke into a text field. The game must not also act on it.
const EDITOR_KEY_FLAG: &str = "__jengineEditorKey";

fn editor_kept_key(e: &web_sys::KeyboardEvent) -> bool {
    js_sys::Reflect::get(e, &JsValue::from_str(EDITOR_KEY_FLAG))
        .map(|v| v.is_truthy())
        .unwrap_or(false)
}

fn install_input_listeners(
    canvas: &HtmlCanvasElement,
    app: Rc<RefCell<ViewportApp>>,
) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;

    {
        let app_kd = app.clone();
        let kd = Closure::<dyn FnMut(_)>::new(move |e: web_sys::KeyboardEvent| {
            if let Ok(mut a) = app_kd.try_borrow_mut() {
                // Paused play routes the keyboard to the editor, not
                // the game — otherwise gizmo hotkeys would also drive
                // the frozen player around.
                if a.playing && !a.paused && !editor_kept_key(&e) {
                    // Text entry: `code` is the physical key, `key` is what
                    // the layout produced. A printable keystroke reports a
                    // single-character `key` ("a", "A", "ä"); everything else
                    // is a name like "Shift" or "Enter", which is why the
                    // length check is the filter.
                    // The hidden textarea a text field types through brings
                    // its own characters (`TextProxy`).
                    let key = e.key();
                    let proxy = jengine_core::input::text_entry::web::TextProxy::is_proxy_event(&e);
                    if !proxy && key.chars().count() == 1 && !e.ctrl_key() && !e.meta_key() {
                        a.input.type_str(&key);
                    }
                    if let Some(code) = map_browser_key(&e.code()) {
                        a.input.key_down(code);
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
            // Releases are applied even while paused. Only presses are
            // routed to the editor during a pause; gating the release too
            // meant a key held when Pause was clicked and let go before
            // resuming stayed down forever. `key_up` on a key that is not
            // held is a no-op, so there is nothing to protect.
            if let Some(code) = map_browser_key(&e.code()) {
                if let Ok(mut a) = app_ku.try_borrow_mut() {
                    a.input.key_up(code);
                }
            }
        });
        document.add_event_listener_with_callback("keyup", ku.as_ref().unchecked_ref())?;
        ku.forget();
    }
    {
        let app_mm = app.clone();
        let mm = Closure::<dyn FnMut(_)>::new(move |e: web_sys::MouseEvent| {
            if let Ok(mut a) = app_mm.try_borrow_mut() {
                if a.playing {
                    a.input.mouse_position = (e.offset_x() as f32, e.offset_y() as f32);
                    // `get_mouse_delta`, in the same CSS pixels: the event's
                    // own motion keeps coming while the pointer is locked.
                    a.input.mouse_motion(e.movement_x() as f32, e.movement_y() as f32);
                }
            }
        });
        canvas.add_event_listener_with_callback("mousemove", mm.as_ref().unchecked_ref())?;
        mm.forget();
    }
    {
        let app_md = app.clone();
        let md = Closure::<dyn FnMut(_)>::new(move |e: web_sys::MouseEvent| {
            if let Ok(mut a) = app_md.try_borrow_mut() {
                if a.playing {
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
                }
            }
        });
        canvas.add_event_listener_with_callback("mousedown", md.as_ref().unchecked_ref())?;
        md.forget();
    }
    {
        let app_mu = app.clone();
        let mu = Closure::<dyn FnMut(_)>::new(move |e: web_sys::MouseEvent| {
            if let Ok(mut a) = app_mu.try_borrow_mut() {
                match e.button() {
                    0 => a.input.set_mouse_button(true, false),
                    1 => a.input.set_middle_mouse_button(false),
                    2 => a.input.set_mouse_button(false, false),
                    _ => {}
                }
            }
        });
        // On the window, not the canvas, and ungated like the key-up above: a
        // press dragged off the viewport, or released after Stop, otherwise
        // left the button held.
        if let Some(window) = web_sys::window() {
            window.add_event_listener_with_callback("mouseup", mu.as_ref().unchecked_ref())?;
        }
        mu.forget();
    }
    {
        // Losing focus - alt-tab, or clicking into another editor panel's
        // text field - delivers no key-up, so held keys would stick.
        let app_blur = app.clone();
        let blur = Closure::<dyn FnMut()>::new(move || {
            if let Ok(mut a) = app_blur.try_borrow_mut() {
                a.input.release_all();
            }
        });
        if let Some(window) = web_sys::window() {
            window.add_event_listener_with_callback("blur", blur.as_ref().unchecked_ref())?;
        }
        blur.forget();
    }
    {
        let app_wh = app.clone();
        let wh = Closure::<dyn FnMut(_)>::new(move |e: web_sys::WheelEvent| {
            if let Ok(mut a) = app_wh.try_borrow_mut() {
                if a.playing {
                    a.input.mouse_wheel(-e.delta_y() as f32 / 120.0);
                    e.prevent_default();
                }
            }
        });
        canvas.add_event_listener_with_callback("wheel", wh.as_ref().unchecked_ref())?;
        wh.forget();
    }

    Ok(())
}

/// A browser `KeyboardEvent.code` as a [`KeyCode`].
///
/// One line, because this used to be a hand-written `match` here *and* an
/// almost identical one in the other browser runtime - both listing the same
/// partial set and both silently dropping everything else, Backspace and
/// PageUp/PageDown among them. `from_browser_code` derives the whole enum
/// from the key names instead, so it cannot fall behind.
fn map_browser_key(code: &str) -> Option<KeyCode> {
    KeyCode::from_browser_code(code)
}
