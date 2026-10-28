//! Phase 3.c-3 — load a real `.scene` file and render every
//! `Transform + SpriteComponent` entity through the wgpu pipeline.
//!
//! Behaviour:
//! - Auto-detects v1 / v2 scene format via [`load_scene_value`].
//! - Builds a [`ComponentRegistry`] populated with the engine's core
//!   component types (so v2 scenes deserialize) but does **not** spin up
//!   scripts, physics, audio, particles, GUI, or any of the heavier
//!   subsystems — this slice only validates the sprite render path end-
//!   to-end against authored content.
//! - Resolves each unique `texture_id` against a small list of candidate
//!   roots derived from the scene path: `<scene_dir>/..`,
//!   `<scene_dir>/../..`, and the workspace cwd. The first existing file
//!   wins; missing textures are logged once and the entity is dropped on
//!   render.
//! - Honors any active `Camera2DComponent` for view/projection and
//!   `Transform.position` for the camera position. Falls back to a
//!   centred ortho matching the window viewport.
//!
//! Run via:
//!
//! ```bash
//! cd projects/engine-test-lab && cargo run -p jengine-runtime -- --scene assets/scenes/hub.scene
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use glam::UVec2;
use hecs::World;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

use jengine_core::asset::{AssetManager, RawTextureData};
use jengine_core::audio::{AudioBackend, NullAudioBackend};
use jengine_core::behavior::system::BehaviorGraphSystem;
use jengine_core::ecs::components::Camera2DComponent;
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::{InputAction, InputState, KeyCode};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::system_schedule::SystemPhase;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::particles::ParticleSystem;
use jengine_core::physics::PhysicsWorld;
use jengine_core::renderer::backend::wgpu::{
    WgpuLightingPass, WgpuLitSpritePipeline, WgpuPostProcessStack, WgpuRenderer, WgpuSpriteBatch,
};
use jengine_core::renderer::camera::Camera2D;
use jengine_core::renderer::graph_pipeline_cache::{GraphPipelineCache, GraphPipelineKey};
use jengine_core::renderer::lighting::has_lights;
use jengine_core::debug_overlay::DebugOverlayState;
use jengine_core::renderer::text::{built_in_font_atlas, FontData};
use jengine_core::scene::format::load_scene_value;
use jengine_core::scene::manager::{SceneAction, SceneManager};
use winit::keyboard::PhysicalKey;

// Step 7.1 — the standalone `run` + `WgpuSceneApp` event-loop entry
// point was removed. The unified `App` in `main.rs` now owns the
// event loop; `--gpu-backend wgpu` simply hands a scene path to
// `App` which, in `resumed`, calls `build_state` and parks the
// resulting `WgpuSceneState` in `RuntimeMode::Wgpu`. Per-event
// dispatch goes through `handle_window_event` below.

pub(crate) struct WgpuSceneState {
    /// Set by `SceneAction::Quit` (script `quit_game()`); the event loop
    /// checks it each frame and exits. The action drain runs deep inside the
    /// frame update where the event loop is not in scope.
    pub(crate) quit_requested: bool,
    /// `JENGINE_SHOT`: screenshots to take, then quit (`shot_env`).
    pub(crate) shot: Option<crate::shot_env::ShotEnv>,
    /// Wall time of the last frame, for `shot`.
    shot_clock: Instant,
    /// Multiplier on the dt fed to the world (script `set_time_scale`).
    /// 0.0 is a true pause: everything downstream derives from this one dt.
    pub(crate) time_scale: f32,
    /// The device has sent raw mouse motion (`DeviceEvent::MouseMotion`),
    /// so `InputState::mouse_delta` comes from that; until then it comes from
    /// the cursor's own moves (`last_cursor`), so a platform without raw
    /// motion still has a delta.
    raw_mouse_motion: bool,
    last_cursor: Option<(f32, f32)>,
    #[allow(dead_code)]
    window:       Arc<Window>,
    renderer:     WgpuRenderer,
    sprite_batch: WgpuSpriteBatch,
    /// Forward-shaded lit-sprite pipeline + flat-normal fallback. Entities
    /// opt in via `MaterialComponent { material_id == "lit_sprite" }`.
    /// Constructed upfront so the per-frame branch is just a query check.
    lit_sprite_batch:    WgpuLitSpritePipeline,
    flat_normal_texture: jengine_core::renderer::backend::wgpu::WgpuTexture,
    /// Phase 3.c-4 Step 4a — the wgpu scene runner now caches textures
    /// through the shared `AssetManager` (raw RGBA bytes + lazy wgpu
    /// upload via `ensure_wgpu_texture`) instead of an ad-hoc HashMap.
    /// This keeps the wgpu path on the same asset surface the gl path
    /// uses, so future asset features (atlases, hot reload, package
    /// system) work uniformly. Step 4c will collapse the gl `Texture`
    /// cache into the same store.
    assets:       AssetManager,
    /// Texture ids the renderer dropped draws for. After each frame the ones
    /// not asked for yet are loaded from disk (`textures_to_request`), so a
    /// sprite whose texture nothing preloaded - one a package created, say -
    /// appears a frame late rather than never.
    missing_warned: HashSet<String>,
    /// Ids already asked for, so a texture that is not on disk is looked for
    /// once, not every frame.
    texture_requested: HashSet<String>,
    /// Where texture ids resolve on disk, for those on-demand loads.
    texture_roots: Vec<PathBuf>,
    /// `asset::packs::generation()` when `texture_requested` was last
    /// cleared: a pack mounted since may hold a texture given up on.
    #[cfg(feature = "packs")]
    packs_generation: u64,
    /// What the scene is cleared to: the project's `background_color`
    /// (set by `lib.rs` after the state is built). It used to be a constant
    /// here, so native ignored the setting the editor honoured.
    pub(crate) clear_color: [f32; 4],
    world:        World,
    /// Wall-clock timestamp of the previous frame, for computing
    /// `dt` to feed `ParticleSystem::update`. Initialised in
    /// `build_state` so the very first frame sees a non-zero but
    /// tiny dt.
    last_frame: Instant,
    /// Fingers: on-screen controls (`TouchControls`) and the one standing in
    /// for the left mouse button. See `input::touch_controls`.
    touch: jengine_core::input::touch_controls::TouchControlsState,
    /// Snaps the raw wall-clock delta onto the refresh grid of whichever
    /// display the window is on, so the simulation advances by an even
    /// amount per shown frame. Without it a perfectly steady frame rate
    /// still judders, because the gap between two frame starts absorbs
    /// swapchain acquire and composition scheduling jitter. Most visible
    /// on a multi-monitor setup with mismatched refresh rates, where the
    /// present cadence beats against scanout.
    pacer: jengine_core::time::FramePacer,
    /// Gameplay in fixed steps when the project asks (`settings.simulation_hz`,
    /// `ecs::fixed_step`); one step of the frame's `dt` otherwise.
    fixed_step: jengine_core::ecs::fixed_step::FixedStep,
    /// Frames until the pacer re-reads the monitor's refresh rate. The
    /// window can be dragged between displays that do not share one, and
    /// snapping to the wrong grid is worse than not snapping.
    pacer_recheck_in: u32,
    /// F3 overlay + `engine.frame_stats()` numbers. Accumulated across the
    /// frame and handed to the script system once, just before rendering.
    /// `render_time_ms` / `draw_calls` describe the previous frame - they
    /// are only known after the pass the overlay is drawn inside.
    frame_stats: jengine_core::profiling::FrameStats,
    /// Smoothed FPS for the overlay, so the readout doesn't jitter.
    fps_ema: f32,
    /// Registered packages plus the schedule their systems live in.
    ///
    /// Nothing owned these before — `register_systems` was called by no
    /// runtime, so a package's systems never ran and parallax (the one
    /// built-in with no hardcoded path) was inert. See BUGS.md #13.
    package_host: jengine_core::package::host::PackageHost,
    /// The component registry the scene loader consults, kept on the state so
    /// package components survive a scene reload. It used to be rebuilt from
    /// `register_core_components` at each of three load sites, which would
    /// have dropped every package component on reload.
    registry: ComponentRegistry,
    /// Batch-script worker pool, created on the first frame that finds a
    /// `BatchScriptComponent` in the world.
    ///
    /// Lazy because building it spawns worker threads and JS runtimes, which
    /// is real cost for the overwhelming majority of scenes that use no batch
    /// scripts. `None` after a failed build too, so a broken pool is reported
    /// once rather than retried every frame.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    batch_pool: Option<jengine_core::scripting::worker_pool::BatchScriptPool>,
    /// Set once a pool build has been attempted, so failure is not retried.
    /// On mobile, where there is no pool, set once the scene has been told so.
    batch_pool_tried: bool,
    /// Batch scripts that failed to load, so a missing or malformed one is
    /// reported once instead of re-read every frame.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    batch_load_failed: HashSet<String>,
    /// Counters `run_batch_scripts` fills (worker count, VM memory). Kept on
    /// the state because they are per-frame and the dispatcher writes them.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    batch_counters: jengine_core::profiling::FrameCounters,
    /// Wall-clock anchor for the runtime's monotonic clock. Used by
    /// `WgpuSpriteBatch::set_time` so graph-driven materials (built
    /// from `.shader` files by `shader_codegen`) can sample
    /// `globals.time` for animated effects.
    start_time: Instant,
    /// Slice 13b-iv — cache of wgpu pipelines compiled from `.shader`
    /// graph files. Keyed by `(graph_path, blend_mode)`. Populated
    /// lazily on first sprite flush that resolves a material with
    /// `shader_graph_path = Some(…)`.
    graph_pipelines: GraphPipelineCache,
    /// Project root (parent dir of `project.jengine`) used to resolve
    /// `.material` and `.shader` asset paths. `None` when the runner
    /// wasn't launched from inside a project tree — graph materials
    /// are unsupported in that case.
    project_root: Option<PathBuf>,
    /// Phase 3.c-4 Step 6d — postprocess scaffold. The scene is
    /// rendered into either `post_process.scene_target()` (no lights
    /// path) or `light_pass.scene_target()` then composited into
    /// `post_process.scene_target()` (lit path). `execute()` blits
    /// the result to the swapchain. Created upfront so resize stays
    /// uniform; the blit pipeline is cheap.
    post_process: WgpuPostProcessStack,
    /// Phase 3.c-4 Step 6d — lighting pass. Always constructed so
    /// resize can forward unconditionally; only used per-frame when
    /// `has_lights(&world)` is true. The cold path (no lights) skips
    /// `render_lights` + `composite` entirely.
    light_pass:   WgpuLightingPass,
    /// Each view's own world buffers (`ecs::views`), by target id.
    view_targets: std::collections::HashMap<String, jengine_core::ecs::wgpu_systems::ViewTargets>,
    /// The videos playing (`jengine_core::video`), decoded by Media
    /// Foundation on Windows (`video_mf`).
    #[cfg(feature = "video")]
    video: jengine_core::video::VideoSystem,
    /// Phase 3.c-4 7-prep wedge 4 — metadata for the engine's
    /// built-in 5×7 monospace debug font. The matching atlas RGBA
    /// bytes are registered in [`AssetManager`] under the texture id
    /// [`BUILTIN_FONT_ID`] so `render_text_wgpu` / `render_ui_wgpu`
    /// can flush through the shared `ensure_wgpu_texture` upload
    /// path. Backend-agnostic (no GPU types) so a future wgpu
    /// AppState can take ownership without touching `glow`.
    font_data:    FontData,
    /// 7-prep wedge 6 — input + scripts + scene manager. Mirrors the
    /// gl AppState's slice of the same name. `input` collects key /
    /// mouse events from `window_event` and is consumed by
    /// `script_system.run` each frame; `scene_manager` absorbs
    /// `load_scene` / push / pop commands the script API queues
    /// (drained + logged each frame — actual scene transitions land
    /// once the runner grows hot-reload, in Step 7).
    input:        InputState,
    pub(crate) script_system: ScriptSystem,
    scene_manager: SceneManager,
    /// 7-prep wedge 12 — behavior-graph runtime. `sync_instances`
    /// is called on the initial scene load and after every
    /// `reload_scene`; `tick_pre_simulation` runs each frame
    /// before scripts (priority 1 in spec). Trigger dispatch +
    /// command flush mirror the gl AppState's RedrawRequested order.
    behavior_system: BehaviorGraphSystem,
    /// 7-prep wedge 7 — current scene's source path. Updated by
    /// `reload_scene` when a script queues `SceneAction::Load`.
    /// Used to (a) recompute candidate roots for asset resolution
    /// and (b) resolve relative scene paths the script asks for.
    scene_path:   PathBuf,
    #[allow(dead_code)]
    scene_name:   String,
    /// 7-prep wedge 11 — shared `Path2D` waypoints for `PatrolPathRef`
    /// entities. Mirrors the gl AppState's `path_library` field.
    /// `None` by default; populated by future scene-load support or
    /// scripts. `update_patrol_paths` tolerates `None` without panicking.
    path_library: Option<jengine_core::ecs::path::Path2DLibrary>,
    /// Drag / flick / elastic state for scrolling GUI containers.
    gui_scroll: jengine_core::ui::scroll::GuiScrollState,
    /// How the UI is scaled when the window is not the size the project's UI
    /// was authored for. Read from `settings.width` / `height` /
    /// `ui_scale_mode` in `project.jengine`.
    ui_scale: jengine_core::ui::scaling::UiScale,
    /// `settings.frame_rate_cap` for this platform. Uncapped, every frame
    /// requests the next as before; capped, `about_to_wait` holds the loop
    /// until the next frame is due, so it sleeps rather than spins.
    frame_cap: jengine_core::time::FrameCap,
    /// What a script asked of the window - its mode, vsync, the frame cap,
    /// the cursor (`jengine_core::display`) - applied each frame.
    display: crate::display_native::NativeDisplay,
    /// A focused text field's IME, soft keyboard and clipboard.
    text: crate::text_native::NativeText,
    /// The UI as the platform's screen reader sees it.
    #[cfg(feature = "accessibility")]
    accessibility: crate::a11y_native::NativeAccessibility,
    /// Scenes loading in the background (`Scene.load_additive`, `Scene.preload`).
    streaming: crate::streaming_native::NativeStreaming,
    /// 7-prep wedge 9 — persistent [`Camera2D`]. Replaces the prior
    /// per-frame `compute_camera` that rebuilt the camera from
    /// scratch each frame and discarded any smoothing state.
    /// Updated each frame via `jengine_core::ecs::systems::update_camera`,
    /// which honors the active `Camera2DComponent` (zoom / viewport)
    /// and applies `CameraFollow` exponential smoothing in world
    /// space. Reset to a fresh `Camera2D` on `reload_scene`.
    camera:       Camera2D,
    /// Where the camera was last frame. `DespawnOffscreen`'s `behind_only`
    /// needs to know which way the view is travelling, and "behind" is the
    /// opposite of that.
    prev_camera:  glam::Vec2,
    /// Slice 8 — audio backend. `KiraAudioBackend` on healthy
    /// hosts, `NullAudioBackend` if the audio device couldn't be
    /// opened (CI, headless, etc.). `play_sound` / `play_music`
    /// commands queued by scripts are drained into this backend
    /// once per frame from `tick`.
    audio:        Box<dyn AudioBackend>,
    /// The sounds `AudioSource`s are playing, by entity (`audio::spatial`).
    spatial_audio: jengine_core::audio::spatial::SpatialAudio,
    /// Slice 9 — rapier2d physics world. `None` when the project's
    /// `settings.physics.backend` is missing / not `"rapier2d"`, or
    /// when no `project.jengine` is found. Mirrors the editor
    /// viewport's Stage 2.3 wiring so scenes that author
    /// `RigidBody2D` + `Collider2D` components actually simulate in
    /// the native runtime.
    physics:      Option<PhysicsWorld>,
    /// Slice 12 — push/pop scene stack. Each snapshot holds the
    /// serialized world state plus the original scene path + name so
    /// `pop_scene` can restore relative-path asset resolution exactly
    /// where the pushed scene left off. Independent of
    /// `SceneManager::scene_stack`, which only ever held empty data
    /// (push/pop were no-ops before this slice).
    scene_stack:  Vec<SceneSnapshot>,
    /// Slice 12.A — fullscreen fade overlay state. `pipeline` is
    /// always constructed; `active` is `Some` while a transition
    /// is in flight. Reset to `None` when the tween completes.
    fade_overlay: jengine_core::fade_overlay::FadeOverlay,
    fade_active:  Option<jengine_core::fade_overlay::TransitionState>,
    /// P1.5 — live asset hot-reload. `None` when disabled (the default
    /// or when `JENGINE_HOT_RELOAD` is unset / falsy). When `Some`, the
    /// per-frame `tick` drains debounced filesystem changes and routes
    /// each to the right reload entry point (scene / script / texture).
    hot_reload:   Option<crate::hot_reload::HotReload>,
    /// Feature 3.3 — runtime debug overlay (F3 = stats HUD, F12 =
    /// entity inspector). Persists across frames so a single tap
    /// toggles cleanly. Also synced from the script-driven
    /// `set_debug_overlay(visible)` API via `sync_from_script`.
    debug_overlay: DebugOverlayState,
    /// False when the build turned the developer overlays off
    /// (`settings.debug_overlay` in project.jengine). F3 / F12 and the
    /// script-driven `set_debug_overlay` then do nothing.
    debug_overlay_enabled: bool,
    /// Native gamepad polling. `None` when no gamepad subsystem is available,
    /// in which case pad bindings stay inert — the same as having nothing
    /// plugged in.
    gamepad: Option<crate::gamepad_native::GamepadBackend>,
}

/// Slice 12 — a saved scene state that `push_scene` records and
/// `pop_scene` restores. Stores the fully-serialized v2 JSON of the
/// world at push time so all script-mutated component state survives
/// the round trip, plus the original `scene_path` / `scene_name` so
/// candidate-root resolution after pop matches the original load.
struct SceneSnapshot {
    json: String,
    /// The pushed scene's own `simulation_hz`, which the snapshot's JSON
    /// does not carry - put back on pop.
    simulation_hz: Option<f32>,
    scene_path: PathBuf,
    scene_name: String,
    /// Slice 12 follow-up — captured camera so `pop_scene` restores
    /// the exact viewport (position, zoom, smoothing target) the
    /// pushed scene was using. Without this, popping snapped back
    /// to origin / zoom 1.0 even when the previous scene had a
    /// follow-cam mid-flight.
    camera: Camera2D,
    /// Same story for the effect chain: `apply_scene` replaces it from
    /// the scene being loaded, and a pop's snapshot JSON carries no
    /// `post_process` block, so without this a pop drops the pushed-over
    /// scene's bloom (BUGS #84).
    post_process: jengine_core::renderer::postprocess::PostProcessSettings,
}

/// Texture id under which the built-in font atlas RGBA bytes are
/// registered in the [`AssetManager`]. Using a `__`-prefixed namespace
/// to keep it out of any user texture id collision space, mirroring
/// the `"__white"` convention used by the particle fallback.
const BUILTIN_FONT_ID: &str = "__font/builtin";

/// Step 7.1 — extracted from the deleted `WgpuSceneApp::window_event`
/// so the main-runtime `App` can dispatch winit events to a
/// `WgpuSceneState` owned in its `RuntimeMode::Wgpu` variant. The
/// body is identical to the original handler; no behaviour change.
/// The scene's on-screen touch controls, if an entity carries some.

/// This runtime's part of `ecs::frame_draw`: a material is read from the
/// project's files on first sight, and a texture with no pixels is loaded
/// after the frame (`load_requested_textures`, off the `warned` set).
struct NativeDrawHost<'a> {
    project_root: Option<&'a std::path::Path>,
}

impl jengine_core::ecs::frame_draw::DrawHost for NativeDrawHost<'_> {
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
        material_pipeline(assets, graph_pipelines, self.project_root, warned, device, format, batch, mid)
    }
    fn texture_missing(&mut self, tex_id: &str) {
        log::debug!("[wgpu-scene] texture '{tex_id}' not loaded yet - loading it after this frame");
    }
    fn warn(&mut self, message: &str) {
        log::warn!("[wgpu-scene] {message}");
    }
}

/// The graph pipeline a sprite's material draws with, or `None` for the
/// default one: loads the `.material` on first sight (from `assets/<id>` or
/// `<id>`, under the project root) and builds its shader graph's pipeline.
/// One function for the world pass and the emission pass, which unlit
/// sprites draw in when the scene is lit - the emission pass used a plain
/// flush until Sep 2026, so an unlit sprite's graph material vanished there.
#[allow(clippy::too_many_arguments)]
fn material_pipeline(
    assets: &mut jengine_core::asset::AssetManager,
    graph_pipelines: &mut GraphPipelineCache,
    project_root: Option<&std::path::Path>,
    warned: &mut std::collections::HashSet<String>,
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    batch: &WgpuSpriteBatch,
    mid: &str,
) -> Option<std::sync::Arc<wgpu::RenderPipeline>> {
    if !assets.has_material(mid) {
        let mut loaded = false;
        if let Some(root) = project_root {
            let roots = [root.join("assets"), root.to_path_buf()];
            if let Some(candidate) = jengine_core::asset::find_in_roots(&format!("{mid}.material"), &roots) {
                if let Err(e) = assets.load_material(mid, candidate.to_string_lossy().as_ref()) {
                    log::warn!("[wgpu-scene] failed to load material '{mid}': {e:#}");
                }
                loaded = true;
            }
        }
        if !loaded && warned.insert(format!("__mat:{mid}")) {
            log::debug!("[wgpu-scene] no .material asset for id '{mid}' - default pipeline");
        }
    }
    let (graph_path, blend_mode) =
        assets.get_material(mid).and_then(|m| m.shader_graph_path.as_ref().map(|p| (p.clone(), m.blend_mode)))?;
    let key = GraphPipelineKey { shader_graph_path: graph_path, blend_mode };
    // A failure is logged once by the cache; the default pipeline stands in.
    graph_pipelines.get_or_build(device, format, batch, project_root, &key).ok()
}

fn touch_controls_of(world: &World) -> Option<jengine_core::ecs::components::TouchControls> {
    world
        .query::<&jengine_core::ecs::components::TouchControls>()
        .iter()
        .next()
        .map(|(_, c)| c.clone())
}

/// The insets of a safe rectangle within a screen, each `(x, y, w, h)` in
/// physical pixels - how far in from each edge the safe area starts.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
pub(crate) fn insets_between(screen: (i32, i32, u32, u32), safe: (i32, i32, u32, u32)) -> jengine_core::ui::scaling::SafeInsets {
    let left = (safe.0 - screen.0).max(0) as f32;
    let top = (safe.1 - screen.1).max(0) as f32;
    let right = ((screen.0 + screen.2 as i32) - (safe.0 + safe.2 as i32)).max(0) as f32;
    let bottom = ((screen.1 + screen.3 as i32) - (safe.1 + safe.3 as i32)).max(0) as f32;
    jengine_core::ui::scaling::SafeInsets { left, top, right, bottom }
}

/// Ask the platform where the screen is covered (a notch) and lay the UI out
/// clear of it. Desktop windows have no cut-outs. Kept in step in both
/// places a `UiScale` lives: this runner's, which draws the UI, and the
/// script system's, which hit-tests `.gui` clicks.
pub(crate) fn refresh_safe_insets(state: &mut WgpuSceneState) {
    #[cfg(target_os = "android")]
    let insets = crate::android::safe_insets().unwrap_or_default();
    // iOS: winit's inner frame is the safe area and its outer frame the
    // screen, so the insets are the difference.
    #[cfg(target_os = "ios")]
    let insets = match (state.window.inner_position(), state.window.outer_position()) {
        (Ok(inner), Ok(outer)) => insets_between(
            (outer.x, outer.y, state.window.outer_size().width, state.window.outer_size().height),
            (inner.x, inner.y, state.window.inner_size().width, state.window.inner_size().height),
        ),
        _ => jengine_core::ui::scaling::SafeInsets::ZERO,
    };
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let insets = jengine_core::ui::scaling::SafeInsets::ZERO;
    if insets != state.ui_scale.safe {
        log::info!(
            "safe area insets: left {} top {} right {} bottom {}",
            insets.left, insets.top, insets.right, insets.bottom
        );
    }
    state.ui_scale.safe = insets;
    state.script_system.api.services.ui_scale.safe = insets;
}

/// The app left the foreground (a phone backgrounded it). Drops the surface,
/// whose window is going away, and pauses what was sounding; nothing is
/// simulated until [`resume`]. Held keys and the finger are let go, since
/// their release will never arrive.
pub(crate) fn suspend(state: &mut WgpuSceneState) {
    log::info!("[wgpu-scene] suspended");
    state.renderer.suspend();
    state.audio.suspend();
    state.input.release_all();
    state.touch.release_all();
}

/// Back in the foreground: a new surface for the window, the sounds
/// [`suspend`] paused, and a frame clock that does not count the time away -
/// otherwise the first frame back would be one enormous step.
pub(crate) fn resume(state: &mut WgpuSceneState) {
    if let Err(e) = state.renderer.resume() {
        log::error!("[wgpu-scene] could not rebuild the surface on resume: {e:#}");
        return;
    }
    let (w, h) = state.renderer.viewport_size();
    state.post_process.resize(&state.renderer.device, w, h);
    state.light_pass.resize(&state.renderer.device, w, h);
    state.audio.unsuspend();
    state.last_frame = Instant::now();
    // The phone may have been turned while away, taking the notch with it.
    refresh_safe_insets(state);
    log::info!("[wgpu-scene] resumed at {w}x{h}");
    state.window.request_redraw();
}

/// The device's raw mouse motion: `Input.get_mouse_delta`, which keeps
/// counting at the window edge and while the pointer is locked
/// (`Cursor.set_locked`). It arrives whether or not the window has focus,
/// so only a focused window takes it.
pub(crate) fn handle_device_event(state: &mut WgpuSceneState, event: winit::event::DeviceEvent) {
    if let winit::event::DeviceEvent::MouseMotion { delta } = event
        && state.window.has_focus()
    {
        state.raw_mouse_motion = true;
        state.input.mouse_motion(delta.0 as f32, delta.1 as f32);
    }
}

pub(crate) fn handle_window_event(
    state: &mut WgpuSceneState,
    event_loop: &ActiveEventLoop,
    event: WindowEvent,
) {
        // The screen reader sees every event first (focus, resizing, ...).
        #[cfg(feature = "accessibility")]
        state.accessibility.process_event(&state.window.clone(), &event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                let w = size.width.max(1);
                let h = size.height.max(1);
                state.renderer.resize(UVec2::new(w, h));
                // Step 6d — forward resize to the offscreen targets so
                // the postprocess blit and the light buffer track the
                // swapchain dimensions.
                state.post_process.resize(&state.renderer.device, w, h);
                state.light_pass.resize(&state.renderer.device, w, h);
                // A resize on a phone is a rotation: the notch changed edge.
                refresh_safe_insets(state);
            }
            WindowEvent::RedrawRequested => {
                // Backgrounded: no surface to draw to, and a game that kept
                // simulating would carry on without its player. Nor is the
                // next redraw requested, which parks the loop; `resume`
                // requests it again.
                if state.renderer.is_suspended() {
                    return;
                }
                // What a script asked of the window last frame: its mode,
                // vsync, the frame cap, the cursor.
                state.display.sync(
                    &state.window,
                    event_loop,
                    &mut state.renderer,
                    &mut state.frame_cap,
                    &state.assets,
                    &mut state.missing_warned,
                );
                state.text.sync(&state.window, &state.ui_scale);
                sync_streaming(state);
                #[cfg(feature = "packs")]
                sync_asset_packs(state);
                if let Err(e) = render_frame(state) {
                    log::warn!("[wgpu-scene] frame error: {e:#}");
                }
                // Checked on the frame path, not with input: a script calling
                // quit_game() must take effect whether or not a key is touched.
                if let Some(shot) = state.shot.as_mut() {
                    let now = Instant::now();
                    let wall = (now - state.shot_clock).as_secs_f32();
                    state.shot_clock = now;
                    if shot.tick(wall) {
                        state.quit_requested = true;
                    }
                }
                if state.quit_requested {
                    event_loop.exit();
                    return;
                }
                // A capped loop is paced by `about_to_wait` instead.
                if !state.frame_cap.is_capped() {
                    state.window.request_redraw();
                }
            }
            // 7-prep wedge 6 — input plumbing. Same shape as gl
            // AppState. `begin_frame` is called at the END of
            // `render_frame` so just_pressed/released stay live for
            // exactly one tick.
            WindowEvent::KeyboardInput { event, .. } => {
                // Text entry rides the same event but a different field:
                // `physical_key` is which key, `text` is what the layout
                // produced. Shift, AltGr, dead keys and IME all make those
                // two different answers, so a text field has to read the
                // second. `type_str` drops control characters.
                if event.state.is_pressed()
                    && let Some(text) = event.text.as_ref()
                {
                    state.input.type_str(text);
                }
                // Android's back button has no physical key code, only a
                // logical `BrowserBack`, and winit consumes it so the
                // activity does not close. It means what Escape means to
                // every game here: back out of this screen.
                let code = match event.physical_key {
                    PhysicalKey::Code(key) => crate::map_key(key),
                    _ => None,
                }
                .or_else(|| {
                    (event.logical_key
                        == winit::keyboard::Key::Named(winit::keyboard::NamedKey::BrowserBack))
                    .then_some(KeyCode::Escape)
                });
                if let Some(code) = code {
                    if event.state.is_pressed() {
                        state.input.key_down(code);
                        // ctrl+V over a focused text field: the clipboard.
                        crate::text_native::NativeText::paste_if_asked(code, &mut state.input);
                        // Feature 3.3 — F3 / F12 toggle the runtime debug
                        // overlay. Filter out auto-repeat so holding the
                        // key doesn't flicker the panel.
                        if !event.repeat {
                            match code {
                                // Inert when the build disabled the overlays.
                                KeyCode::F3 if state.debug_overlay_enabled => {
                                    state.debug_overlay.toggle_stats()
                                }
                                KeyCode::F12 if state.debug_overlay_enabled => {
                                    state.debug_overlay.toggle_inspector()
                                }
                                // F4: collider and joint outlines.
                                KeyCode::F4 if state.debug_overlay_enabled => {
                                    state.debug_overlay.toggle_physics()
                                }
                                _ => {}
                            }
                        }
                    } else {
                        state.input.key_up(code);
                    }
                }
                // Escape used to close the window here, unconditionally. Every
                // game that opens a pause menu or backs out of a screen on
                // Escape - a game's pause, hub, settings and summary screens;
                // the test lab's "back to hub" - quit instead in a native
                // build. `quit_game()` is how a game quits.
            }
            // An unfocused window gets no key-up events, so anything held
            // through alt-tab would stay pressed until pressed again.
            // An IME composing (shown in the focused field) or committing.
            WindowEvent::Ime(ime) => crate::text_native::NativeText::ime(&ime, &mut state.input),
            WindowEvent::Focused(false) => {
                state.input.release_all();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let at = (position.x as f32, position.y as f32);
                if !state.raw_mouse_motion
                    && let Some(was) = state.last_cursor
                {
                    state.input.mouse_motion(at.0 - was.0, at.1 - was.1);
                }
                state.last_cursor = Some(at);
                state.input.mouse_position = at;
            }
            WindowEvent::MouseInput { state: btn_state, button, .. } => {
                let pressed = btn_state.is_pressed();
                match button {
                    winit::event::MouseButton::Left  => state.input.set_mouse_button(true, pressed),
                    winit::event::MouseButton::Right => state.input.set_mouse_button(false, pressed),
                    winit::event::MouseButton::Middle => state.input.set_middle_mouse_button(pressed),
                    _ => {}
                }
            }
            // Queued, not applied: the next frame's `touch.tick` decides
            // whether a finger works an on-screen control or is the left
            // mouse button (the first free finger is, as on the web), so
            // every click-driven game and both UI systems work by touch.
            WindowEvent::Touch(touch) => {
                use jengine_core::input::touch_controls::TouchPhase as Phase;
                use winit::event::TouchPhase;
                let phase = match touch.phase {
                    TouchPhase::Started => Phase::Started,
                    TouchPhase::Moved => Phase::Moved,
                    TouchPhase::Ended | TouchPhase::Cancelled => Phase::Ended,
                };
                state.touch.push(
                    touch.id,
                    phase,
                    touch.location.x as f32,
                    touch.location.y as f32,
                );
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let y = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y)   => y,
                    winit::event::MouseScrollDelta::PixelDelta(pos)   => pos.y as f32 / 120.0,
                };
                state.input.mouse_wheel(y);
            }
            _ => {}
        }
}

/// Build the package host for a project, honouring its `enabled_packages`.
///
/// Packages are Rust crates linked at compile time — there is no dynamic
/// loading — so this can only offer packages this binary was built with. A
/// project's own package, resolved by the builder into a Cargo dependency,
/// becomes available here once the runtime is rebuilt with it.
///
/// The built-in four are all registered by default when a project says
/// nothing, which matches what every existing `project.jengine` lists.
///
/// **Registering physics and audio here does not double-run them.** The
/// runtime drives both on its own hardcoded path and deliberately does not
/// share `PhysicsWorld` / the audio backend as resources; both packages'
/// systems begin by looking their resource up and doing nothing when it is
/// absent. Registering them still buys their components and script-API
/// metadata.
fn build_package_host(
    scene_path: &Path,
    add_packages: &crate::PackageHook,
) -> jengine_core::package::host::PackageHost {
    use jengine_core::package::host::PackageHost;

    let enabled = load_project_enabled_packages(scene_path);
    let is_on = |name: &str| enabled.as_ref().is_none_or(|list| list.iter().any(|n| n == name));

    let mut host = PackageHost::new();
    if is_on("jengine-physics") {
        host.add_package(Box::new(jengine_physics::PhysicsPackage));
    }
    if is_on("jengine-audio") {
        host.add_package(Box::new(jengine_audio::AudioPackage));
    }
    if is_on("jengine-particles") {
        host.add_package(Box::new(jengine_particles::ParticlesPackage));
    }
    if is_on("jengine-parallax") {
        host.add_package(Box::new(jengine_parallax::ParallaxPackage));
    }
    if is_on("jengine-swarm") {
        host.add_package(Box::new(jengine_swarm::SwarmPackage));
    }
    // Project packages last, so a generated per-project runtime can add its
    // own. They are not filtered by `enabled_packages`: a project that linked
    // a package deliberately is not also asked to list it.
    add_packages(&mut host);
    host
}

/// Read `settings.enabled_packages` from the project beside the scene.
///
/// `None` means the project said nothing, which enables everything built in —
/// a project written before the setting existed should not silently lose
/// physics.
fn load_project_enabled_packages(scene_path: &Path) -> Option<Vec<String>> {
    let root = find_project_root(scene_path)?;
    let text = std::fs::read_to_string(root.join("project.jengine")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let arr = v.get("settings")?.get("enabled_packages")?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|s| s.as_str().map(str::to_string))
            .collect(),
    )
}

/// The UI scale the project beside this scene asks for.
///
/// The reference size is `settings.width` / `settings.height` - the window the
/// project is authored against, which every project already declares - so
/// turning scaling on costs no new field. `settings.ui_scale_mode` opts out.
/// A project that cannot be read scales against the 1280x720 default rather
/// than not at all, because an unscaled UI is the failure this exists to fix.
/// The frame-rate cap this build runs at (`settings.frame_rate_cap`).
fn load_project_frame_cap(scene_path: &Path) -> jengine_core::time::FrameCap {
    let fps = project_frame_cap_fps(scene_path);
    if fps > 0 {
        log::info!("frame rate capped at {fps} fps (settings.frame_rate_cap)");
    }
    jengine_core::time::FrameCap::new(fps)
}

/// `settings.frame_rate_cap` for this platform, in frames a second (0: none).
fn project_frame_cap_fps(scene_path: &Path) -> u32 {
    use jengine_core::time::{CapPlatform, FrameRateCaps};
    let project = find_project_root(scene_path)
        .and_then(|root| std::fs::read_to_string(root.join("project.jengine")).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .unwrap_or_default();
    FrameRateCaps::from_project(&project).for_platform(CapPlatform::native())
}

/// Between events: with a frame-rate cap, request the next frame once it is
/// due and otherwise sleep until it is. Uncapped, frames request their own
/// successors (`RedrawRequested`) and this does nothing.
pub(crate) fn about_to_wait(state: &mut WgpuSceneState, event_loop: &ActiveEventLoop) {
    use winit::event_loop::ControlFlow;
    if !state.frame_cap.is_capped() || state.renderer.is_suspended() {
        return;
    }
    let since = state.last_frame.elapsed().as_secs_f64();
    if state.frame_cap.ready(since) {
        event_loop.set_control_flow(ControlFlow::Wait);
        state.window.request_redraw();
    } else {
        let wait = std::time::Duration::from_secs_f64(state.frame_cap.min_gap() - since);
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + wait));
    }
}

/// `settings.texture_filter` - how the project's textures are sampled
/// (`jengine_core::renderer::texture_filter`). Absent: everything nearest.
fn load_project_texture_filters(scene_path: &Path) -> jengine_core::renderer::texture_filter::TextureFilterRules {
    use jengine_core::renderer::texture_filter::TextureFilterRules;
    let Some(root) = find_project_root(scene_path) else {
        return TextureFilterRules::default();
    };
    let Ok(text) = std::fs::read_to_string(root.join("project.jengine")) else {
        return TextureFilterRules::default();
    };
    let v = serde_json::from_str::<serde_json::Value>(&text).unwrap_or_default();
    TextureFilterRules::from_settings(v.get("settings"))
}

/// `settings.simulation_hz` - gameplay in fixed steps (`ecs::fixed_step`).
/// Absent: one step a frame, as always.
fn load_project_fixed_step(scene_path: &Path) -> jengine_core::ecs::fixed_step::FixedStep {
    use jengine_core::ecs::fixed_step::FixedStep;
    let Some(root) = find_project_root(scene_path) else {
        return FixedStep::new();
    };
    let Ok(text) = std::fs::read_to_string(root.join("project.jengine")) else {
        return FixedStep::new();
    };
    let v = serde_json::from_str::<serde_json::Value>(&text).unwrap_or_default();
    FixedStep::from_settings(v.get("settings"))
}

/// `settings.gpu_preference` - which of two GPUs to ask for
/// (`jengine_core::renderer::gpu_preference`). Absent: the fast one.
fn load_project_gpu_preference(scene_path: &Path) -> jengine_core::renderer::gpu_preference::GpuPreference {
    use jengine_core::renderer::gpu_preference::GpuPreference;
    let Some(root) = find_project_root(scene_path) else {
        return GpuPreference::default();
    };
    let Ok(text) = std::fs::read_to_string(root.join("project.jengine")) else {
        return GpuPreference::default();
    };
    let v = serde_json::from_str::<serde_json::Value>(&text).unwrap_or_default();
    GpuPreference::from_settings(v.get("settings"))
}

/// The window's title: the game's - `settings.title`, else the name under its
/// icon (`settings.app.name`), else the project's name. It used to be
/// `jEngine — wgpu scene: <scene>`, which a player has no use for.
fn load_project_title(scene_path: &Path) -> Option<String> {
    let root = find_project_root(scene_path)?;
    let text = std::fs::read_to_string(root.join("project.jengine")).ok()?;
    let v = serde_json::from_str::<serde_json::Value>(&text).ok()?;
    project_title(&v)
}

fn project_title(project: &serde_json::Value) -> Option<String> {
    let settings = project.get("settings");
    [
        settings.and_then(|s| s.get("title")),
        settings.and_then(|s| s.get("app")).and_then(|a| a.get("name")),
        project.get("name"),
    ]
    .into_iter()
    .flatten()
    .filter_map(serde_json::Value::as_str)
    .map(str::trim)
    .find(|s| !s.is_empty())
    .map(str::to_string)
}

/// `settings.audio_mixer` - the buses a game starts with
/// (`jengine_core::audio::MixerLayout`). Absent: master, sfx and music.
fn load_project_mixer(scene_path: &Path) -> Option<jengine_core::audio::MixerLayout> {
    let root = find_project_root(scene_path)?;
    let text = std::fs::read_to_string(root.join("project.jengine")).ok()?;
    let v = serde_json::from_str::<serde_json::Value>(&text).ok()?;
    jengine_core::audio::MixerLayout::from_project(&v)
}

fn load_project_ui_scale(scene_path: &Path) -> jengine_core::ui::scaling::UiScale {
    use jengine_core::ui::scaling::{UiScale, UiScaleMode};
    let Some(root) = find_project_root(scene_path) else {
        return UiScale::default();
    };
    let Ok(text) = std::fs::read_to_string(root.join("project.jengine")) else {
        return UiScale::default();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return UiScale::default();
    };
    let settings = v.get("settings");
    let num = |key: &str| {
        settings
            .and_then(|s| s.get(key))
            .and_then(|n| n.as_f64())
            .map(|n| n as f32)
            .filter(|n| *n > 0.0)
    };
    let mut scale = match (num("width"), num("height")) {
        (Some(w), Some(h)) => UiScale::from_reference(w, h),
        _ => UiScale::default(),
    };
    scale.mode = UiScaleMode::from_setting(
        settings.and_then(|s| s.get("ui_scale_mode")).and_then(|m| m.as_str()),
    );
    scale
}

pub(crate) fn build_state(
    event_loop: &ActiveEventLoop,
    scene_path: &Path,
    add_packages: &crate::PackageHook,
) -> Result<WgpuSceneState> {
    let json = std::fs::read_to_string(scene_path)
        .with_context(|| format!("reading scene file {}", scene_path.display()))?;

    // Asset packs before anything is loaded: one already downloaded must be
    // mounted for the first scene to find what it brought.
    #[cfg(feature = "packs")]
    init_asset_packs(scene_path);

    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);

    // Packages register into the same registry, before the scene is parsed:
    // a package component has to be known to the loader, not registered after
    // it has already skipped the key.
    let mut package_host = build_package_host(scene_path, add_packages);
    package_host.initialize(&mut registry);
    // Script spawns apply without this registry in hand; a prefab's package
    // components are found through the published copy (BUGS #109).
    jengine_core::package::component_registry::install_global(registry.clone());
    // Package script globals reach every runtime through the command path;
    // publishing the registry here is what lets the native apply loop find
    // the same handler the browser hosts call.
    if !package_host.script_commands().is_empty() {
        log::info!(
            "[wgpu-scene] {} package script command(s): {}",
            package_host.script_commands().len(),
            package_host.script_commands().names().join(", ")
        );
    }
    let package_commands = package_host.take_script_commands();

    // `.anim`, `.particle` and `.tilemap` references, inlined before the
    // scene is deserialized. Same asset search roots as textures.
    let candidates = candidate_roots(scene_path);
    // Parsed once, here: every step after reads or rewrites this value, where
    // each used to parse the text again.
    let mut scene: serde_json::Value = serde_json::from_str(&json)
        .with_context(|| format!("parsing scene file {}", scene_path.display()))?;
    inline_scene_sources(&mut scene, &candidates);

    // Backlog 2.2 — load `.prefab` files into a local map BEFORE
    // expanding `prefab_instance` entries in the scene JSON. The same
    // map is later cloned into `script_system.api.services.prefabs`
    // so `spawn_prefab(name)` continues to work at runtime.
    let prefabs = load_prefab_map(scene_path);

    // Expand any v1 entities authored as `prefab_instance` (template
    // id + dotted-path overrides) into a fully-materialised
    // ComponentBundle. v2 scenes are not yet supported by this pass
    // and pass through unchanged.
    jengine_core::scene::expand_prefab_instances_in_value(&mut scene, &prefabs, "wgpu-scene");

    // The scene-wide fields, kept; the scene itself goes to the loader whole.
    let scene_header = jengine_core::scene::format::scene_header(&scene);
    let mut world = World::new();
    load_scene_value(scene, &mut world, &registry)
        .with_context(|| format!("parsing scene file {}", scene_path.display()))?;

    let scene_name = jengine_core::scene::name_in(&scene_header).unwrap_or_else(|| {
        scene_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("scene")
            .to_string()
    });
    jengine_core::scene::set_current_name(&scene_name);

    log::info!(
        "[wgpu-scene] loaded '{scene_name}' from {} ({} entities)",
        scene_path.display(),
        world.len()
    );
    log::info!("[wgpu-scene] texture search roots: {:?}", candidates);

    // Window size: prefer active camera viewport, else 1280x720.
    let (win_w, win_h) = pick_window_size(&world);

    // Hidden until the screen reader's adapter exists: AccessKit must be
    // set up before a window is first shown (`a11y_native`).
    let title = load_project_title(scene_path).unwrap_or_else(|| format!("jEngine — {scene_name}"));
    let mut window_attrs = WindowAttributes::default()
        .with_title(&title)
        .with_inner_size(LogicalSize::new(win_w, win_h))
        .with_visible(false);
    // One of several copies the editor's Build & Run started: its cell of
    // the primary monitor, and its number in the title (`instance_tile`).
    #[cfg(not(target_os = "android"))]
    if let Some(tile) = crate::instance_tile::Tile::from_env()
        && let Some(monitor) = event_loop.primary_monitor().or_else(|| event_loop.available_monitors().next())
    {
        let (pos, size) = (monitor.position(), monitor.size());
        let (x, y, w, h) = tile.cell(crate::instance_tile::usable_area((pos.x, pos.y), (size.width, size.height)));
        let (fw, fh) = crate::instance_tile::FRAME;
        window_attrs = window_attrs
            .with_title(format!("{title}{}", tile.title_suffix()))
            .with_position(winit::dpi::PhysicalPosition::new(x, y))
            .with_inner_size(winit::dpi::PhysicalSize::new(w.saturating_sub(fw).max(320), h.saturating_sub(fh).max(240)));
    }
    let window = Arc::new(event_loop.create_window(window_attrs)?);
    #[cfg(feature = "accessibility")]
    let accessibility = crate::a11y_native::NativeAccessibility::new(event_loop, &window, &title);
    window.set_visible(true);

    let renderer = WgpuRenderer::new_with_gpu(window.clone(), load_project_gpu_preference(scene_path))?;
    // The world is drawn in `scene_format` (a float target where the adapter
    // has one); the batch switches to `color_format` for the UI, which is
    // drawn after the post chain onto the display.
    let sprite_batch = WgpuSpriteBatch::new(&renderer.device, renderer.scene_format)?;
    let lit_sprite_batch = WgpuLitSpritePipeline::new(&renderer.device, renderer.scene_format)?;
    let flat_normal_texture =
        WgpuLitSpritePipeline::flat_normal_texture(&renderer.device, &renderer.queue);

    // Pre-load every unique sprite texture into the AssetManager raw-bytes
    // cache; the actual wgpu upload is deferred to `ensure_wgpu_texture`
    // on first render. This matches Step 4a's design: decode-once, upload-
    // when-needed, share the cache across whichever backend ends up active.
    // Data-oriented design — every `.data` / `.dataschema` under the
    // project's `assets/` dir, published before the textures are preloaded:
    // a conversation's portraits are named only in its table
    // (`ecs::dialogue::portrait_ids`).
    let mut data_registry = {
        let assets_root = scene_path
            .parent()
            .and_then(|p| p.parent())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("assets"));
        assets_root.is_dir().then(|| {
            let registry = jengine_core::data::DataRegistry::load_from_assets_dir(&assets_root);
            // Packages read the same tables from Rust through the published copy.
            jengine_core::data::install_global(std::sync::Arc::new(registry.clone()));
            log::info!(
                "[wgpu-scene] loaded {} data table(s) from {}",
                registry.table_count(),
                assets_root.display()
            );
            registry
        })
    };
    let mut assets = AssetManager::new();
    assets.set_texture_filters(load_project_texture_filters(scene_path));
    load_world_textures(&world, &prefabs, &candidates, &mut assets);

    // Resolve top-level `atlas_refs` — preloads each `.atlas`'s sibling
    // PNG into the texture cache and registers `AtlasData` + per-region
    // mappings so the script bridge's `set_sprite_region(entity,
    // atlas_id, region_name)` (and `get_sprite_region` queries) can
    // resolve the region without filesystem access at runtime. No-op on
    // scenes without an `atlas_refs` array.
    //
    // From the header: prefab expansion keeps every top-level field (it
    // merges back what the typed `SceneData` round-trip drops), so this no
    // longer re-reads the authored file for its `atlas_refs` / `audio_refs`.
    // Wedge 11 — patrol routes. The library, the component and the walk system
    // have all existed since the component was added; nothing ever filled the
    // library, so `PatrolPathRef` moved nothing in any runtime (BUGS #7).
    let path_library = Some(jengine_core::scene::format::scene_metadata(&scene_header))
        .filter(|meta| !meta.paths.is_empty())
        .map(|meta| {
            let mut lib = jengine_core::ecs::path::Path2DLibrary::new();
            for (id, path) in meta.paths {
                lib.insert(id, path);
            }
            log::info!("[wgpu-scene] patrol paths: {}", lib.len());
            lib
        });
    {
        let value = &scene_header;
        let slerrs = jengine_core::asset::sort_layers::resolve_scene_sort_layers(
            value,
            &candidates,
            &mut assets,
        );
        for e in &slerrs {
            log::warn!("[wgpu-scene] sort_layers resolve: {e}");
        }
        let aerrs = jengine_core::renderer::atlas_asset::resolve_atlas_refs(
            value,
            &candidates,
            &mut assets,
        );
        for e in &aerrs {
            log::warn!("[wgpu-scene] atlas resolve: {e}");
        }
        let aresolved = value
            .get("atlas_refs")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        if aresolved > 0 {
            log::info!(
                "[wgpu-scene] resolved {} atlas reference(s) ({} error(s))",
                aresolved,
                aerrs.len()
            );
        }

        // Slice 8 — resolve top-level `audio_refs`; loads each clip
        // file's raw bytes into the AssetManager audio cache under
        // its forward-slash path as the clip id. The audio backend
        // decodes on first `play_sound`/`play_music` via
        // `apply_audio_commands`. No-op on scenes without an
        // `audio_refs` array.
        let auerrs = jengine_core::audio::audio_asset::resolve_audio_refs(
            value,
            &candidates,
            &mut assets,
        );
        for e in &auerrs {
            log::warn!("[wgpu-scene] audio resolve: {e}");
        }
        let auresolved = value
            .get("audio_refs")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        if auresolved > 0 {
            log::info!(
                "[wgpu-scene] resolved {} audio reference(s) ({} error(s))",
                auresolved,
                auerrs.len()
            );
        }
    }

    // Phase 3.c-4 7-prep wedge 3 — register a built-in 1x1 white RGBA
    // texture under the id `"__white"`. Used as the fallback texture
    // for `render_particles_wgpu` when the emitter has no `texture_id`
    // (the system passes `None` and the flush callback maps that to
    // this id). Cheap: 4 bytes of bytes-cache + one tiny wgpu upload
    // on first use. The wgpu pipeline samples this as solid white so
    // the particle's per-vertex color comes through unmodulated.
    assets.insert_texture_bytes(
        "__white".to_string(),
        RawTextureData { width: 1, height: 1, rgba: vec![255, 255, 255, 255] },
    );
    // What on-screen touch controls are drawn with.
    jengine_core::input::touch_controls::insert_textures(&mut assets);

    // Wedge 4 — register the built-in 5×7 debug font atlas. CPU-side
    // bytes generated once via `built_in_font_atlas()`; the wgpu
    // upload happens lazily on first text/UI flush via
    // `ensure_wgpu_texture`. `font_data` is stashed on the state
    // struct so the render pass can pass `&FontData` to the text /
    // UI systems without re-cloning per frame.
    let (font_rgba, font_w, font_h, font_data) = built_in_font_atlas();
    assets.insert_texture_bytes(
        BUILTIN_FONT_ID.to_string(),
        RawTextureData { width: font_w, height: font_h, rgba: font_rgba },
    );
    // A distance field (`renderer::text`): drawn sharp at any size.
    assets.mark_sdf(BUILTIN_FONT_ID);

    // 7-prep wedge 5 — load any .gui files referenced by visible
    // `GuiComponent`s so `render_gui_wgpu` actually has documents to
    // walk.
    load_world_guis(&world, &candidates, &mut assets);

    // Custom bitmap fonts named by `TextComponent.font_id`. Like `.gui`
    // documents these are discovered from the world rather than a scene-level
    // refs array, so there is nothing to author twice.
    load_world_fonts(&world, &candidates, &mut assets);

    // Step 6d — build the offscreen pipeline. `post_process` always
    // owns the path to the swapchain (its `execute` does the final
    // blit). `light_pass` is built upfront so resize can forward
    // unconditionally; per-frame the lit branch is only taken when
    // `has_lights(&world)` is true.
    let surface_size = renderer.viewport_size();
    let mut post_process = WgpuPostProcessStack::new(
        &renderer.device,
        surface_size.0.max(1),
        surface_size.1.max(1),
        renderer.scene_format,
        renderer.color_format,
    )?;
    // Phase 8.2e — apply scene-authored post-process settings (if any)
    // so bloom / vignette / etc. authored in the .scene file render
    // from frame 0 without the script needing to re-toggle them.
    if let Some(settings) = jengine_core::renderer::postprocess::PostProcessSettings::from_scene_value(&scene_header) {
        post_process.set_settings(settings);
    }
    let light_pass = WgpuLightingPass::new(
        &renderer.device,
        surface_size.0.max(1),
        surface_size.1.max(1),
        renderer.scene_format,
    )?;

    // 7-prep wedge 6 — script system, scene manager, input. Best-
    // effort: walk up from the scene path looking for a
    // `project.jengine` so we can load any defined `input_actions`
    // (so e.g. `is_action_just_pressed("flap")` actually fires).
    // Missing project file is fine — the runner stays usable on bare
    // scenes, the script will just see all actions un-bound.
    let mut input = InputState::new();
    if let Some(actions) = load_project_input_actions(scene_path) {
        log::info!("[wgpu-scene] loaded {} input action(s) from project.jengine", actions.len());
        input.load_actions(&actions);
    }

    // Bind the package globals into the Luau VM before any script compiles.
    // They only queue a `PackageCall`; the work happens at apply time.
    let mut script_system = ScriptSystem::new()
        .with_context(|| "constructing ScriptSystem for wgpu-scene")?;
    script_system.install_package_commands(&package_commands);
    // The GUI click hit test lives in `ScriptServices` and computes its own
    // layout, so it needs the same scale the renderer draws with or a click
    // lands somewhere the button is not.
    script_system.api.services.ui_scale = load_project_ui_scale(scene_path);
    // The project's mixer: its buses and their effects before any script
    // touches them (`settings.audio_mixer`).
    if let Some(layout) = load_project_mixer(scene_path) {
        script_system.api.services.mixer.apply_layout(&layout);
    }
    // Published for the apply loop, which reaches it without a borrow of the
    // world it is mutating.
    jengine_core::package::script_commands::install_global(package_commands);
    // Mirror gl AppState — wire fs module resolver so attached
    // scripts that `require()` siblings can resolve paths.
    {
        let scripts_root = scene_path
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("scripts"))
            .unwrap_or_else(|| std::path::PathBuf::from("assets/scripts"));
        // Installed whether or not the folder exists: a build whose
        // scripts all went into asset packs has none until one mounts.
        let resolver = jengine_core::scripting::bridge::fs_resolver(scripts_root.clone());
        script_system.set_module_resolver(resolver);
        log::info!(
            "[wgpu-scene] script module resolver rooted at {}",
            scripts_root.display()
        );
    }

    // Data-oriented design — load every `.data` / `.dataschema` under the
    // project's `assets/` dir into a registry so scripts can read them via
    // `get_data(table, id, field)`.
    {
        let assets_root = scene_path
            .parent()
            .and_then(|p| p.parent())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("assets"));
        if assets_root.is_dir() {
            // Script sources are read against the same root, wherever the
            // process was launched from.
            script_system.set_script_root(assets_root.clone());
            // Loaded (and published) before the textures were preloaded.
            if let Some(registry) = data_registry.take() {
                script_system.set_data_registry(std::rc::Rc::new(registry));
            }
        }
    }

    // Skeletal animation — load every `.skelanim` under `assets/` so a
    // `SkeletalPlayer` has clips to play. Nothing used to do this in any
    // runtime, which left clip playback authorable in theory and dead in
    // practice.
    {
        let assets_root = scene_path
            .parent()
            .and_then(|p| p.parent())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("assets"));
        if assets_root.is_dir() {
            let n = assets.load_skeletal_clips_from_dir(&assets_root);
            if n > 0 {
                log::info!("[wgpu-scene] loaded {n} skeletal clip(s)");
            }
            // And every `.rig`, for `Skeleton.spawn_rig` - which nothing fed
            // until Oct 2026, so it spawned nothing in any runtime.
            let n = assets.load_skeleton_rigs_from_dir(&assets_root);
            if n > 0 {
                log::info!("[wgpu-scene] loaded {n} skeleton rig(s)");
            }
        }
    }

    // Load `*.prefab` files from the project's `assets/prefabs/` so
    // scripts can call `spawn_prefab("Name")`. The lookup key is the
    // prefab's `name` field (falls back to file stem). Prefabs were
    // already loaded into `prefabs` earlier to drive 2.2 prefab-
    // instance expansion; here we just clone them into the script
    // services map so the runtime API stays unchanged.
    if !prefabs.is_empty() {
        let count = prefabs.len();
        for (key, entity_data) in &prefabs {
            script_system
                .api
                .services
                .prefabs_mut()
                .insert(key.clone(), entity_data.clone());
        }
        log::info!("[wgpu-scene] registered {} prefab(s) for script spawn_prefab", count);
    }

    let scene_manager = SceneManager::new();

    // Slice 8 — best-effort audio backend. Falls back to a no-op
    // `NullAudioBackend` if the host can't open an audio device
    // (CI runners, headless boxes) so `play_sound` / `play_music`
    // remain harmless rather than killing the scene.
    let audio: Box<dyn AudioBackend> = match jengine_core::audio::kira_backend::KiraAudioBackend::new() {
        Ok(b) => {
            log::info!("[wgpu-scene] audio backend ready (kira)");
            Box::new(b)
        }
        Err(e) => {
            log::warn!("[wgpu-scene] audio backend init failed ({e}); using NullAudioBackend");
            Box::new(NullAudioBackend)
        }
    };

    // Wedge 12 — behavior-graph runtime. Built before the
    // initial sync so `OnSpawn` events fire on this frame for
    // entities the scene shipped with. `assets/` is the project
    // convention used by the gl AppState; we follow the same.
    let mut behavior_system = BehaviorGraphSystem::new();
    let bg_root = std::path::Path::new("assets");
    let bg_errors = behavior_system.sync_instances(&mut world, bg_root);
    for err in &bg_errors {
        log::warn!(
            "[wgpu-scene] behavior graph load failed for entity {:?} ({}): {}",
            err.entity, err.path.display(), err.source
        );
    }

    // Slice 9 — build the physics world from the project's
    // `settings.physics` block (best-effort lookup walking up from
    // the scene path). Defaults match the editor viewport: skip
    // construction when the backend is not `rapier2d`.
    let physics = build_physics_from_project(scene_path);

    // Slice 12.A — fullscreen fade overlay for scene transition polish.
    let fade_overlay =
        jengine_core::fade_overlay::FadeOverlay::new(&renderer.device, renderer.color_format)?;

    // P1.5 — optional live asset hot-reload. Toggled by JENGINE_HOT_RELOAD;
    // failure to set up the OS watcher is logged + downgraded to None so
    // the runtime keeps running.
    let project_root_for_state = find_project_root(scene_path);
    let hot_reload = if crate::hot_reload::is_enabled_via_env() {
        let roots = candidate_roots(scene_path);
        let watch_root = project_root_for_state
            .clone()
            .or_else(|| roots.first().cloned())
            .unwrap_or_else(|| PathBuf::from("."));
        match crate::hot_reload::HotReload::new(&watch_root, roots) {
            Ok(hr) => {
                log::info!(
                    "[HotReload] watching `{}` (set JENGINE_HOT_RELOAD=0 to disable)",
                    watch_root.display()
                );
                Some(hr)
            }
            Err(e) => {
                log::warn!(
                    "[HotReload] failed to start watcher on `{}`: {e:?} — disabled",
                    watch_root.display()
                );
                None
            }
        }
    } else {
        None
    };

    // Videos are files under the scene's `assets/`, like everything else.
    #[cfg(feature = "video")]
    let video = {
        let root = scene_path
            .parent()
            .and_then(|p| p.parent())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("assets"));
        native_video_system(root)
    };
    Ok(WgpuSceneState {
        quit_requested: false,
        shot: {
            crate::shot_env::seed_persistent_from_env();
            crate::shot_env::ShotEnv::from_env()
        },
        shot_clock: Instant::now(),
        time_scale: 1.0,
        raw_mouse_motion: false,
        last_cursor: None,
        window,
        renderer,
        sprite_batch,
        lit_sprite_batch,
        flat_normal_texture,
        assets,
        missing_warned: HashSet::new(),
        texture_requested: HashSet::new(),
        texture_roots: candidates.clone(),
        #[cfg(feature = "packs")]
        packs_generation: jengine_core::asset::packs::generation(),
        clear_color: [0.05, 0.05, 0.08, 1.0],
        world,
        camera: Camera2D::new(
            (surface_size.0 as f32).max(1.0),
            (surface_size.1 as f32).max(1.0),
        ),
        prev_camera: glam::Vec2::ZERO,
        path_library,
        gui_scroll: Default::default(),
        ui_scale: load_project_ui_scale(scene_path),
        frame_cap: load_project_frame_cap(scene_path),
        display: crate::display_native::NativeDisplay::new(project_frame_cap_fps(scene_path)),
        text: Default::default(),
        #[cfg(feature = "accessibility")]
        accessibility,
        streaming: Default::default(),
        last_frame: Instant::now(),
        touch: jengine_core::input::touch_controls::TouchControlsState::new(),
        pacer: jengine_core::time::FramePacer::new(),
        fixed_step: {
            // The project's rate, or the first scene's own.
            let mut f = load_project_fixed_step(scene_path);
            f.enter_scene(jengine_core::ecs::fixed_step::scene_simulation_hz(&scene_header));
            f
        },
        pacer_recheck_in: 0,
        frame_stats: jengine_core::profiling::FrameStats::default(),
        fps_ema: 0.0,
        package_host,
        registry,
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        batch_pool: None,
        batch_pool_tried: false,
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        batch_load_failed: HashSet::new(),
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        batch_counters: Default::default(),
        start_time: Instant::now(),
        graph_pipelines: GraphPipelineCache::new(),
        project_root: project_root_for_state,
        post_process,
        light_pass,
        view_targets: Default::default(),
        #[cfg(feature = "video")]
        video,
        font_data,
        input,
        script_system,
        scene_manager,
        behavior_system,
        scene_path: scene_path.to_path_buf(),
        scene_name,
        audio,
        spatial_audio: jengine_core::audio::spatial::SpatialAudio::new(),
        physics,
        scene_stack: Vec::new(),
        fade_overlay,
        fade_active: None,
        hot_reload,
        debug_overlay: DebugOverlayState::default(),
        debug_overlay_enabled: load_project_debug_overlay(scene_path),
        gamepad: crate::gamepad_native::GamepadBackend::new(),
    })
}

/// Slice 9 — walk up from `scene_path` looking for `project.jengine`;
/// when found, build a fresh [`PhysicsWorld`] from its
/// `settings.physics` block. Returns `None` if no project file is
/// found, parsing fails, or `backend` is anything other than
/// `rapier2d` (case-insensitive). Mirrors the editor viewport's
/// Stage 2.3 "rapier2d or skip" policy.
fn build_physics_from_project(scene_path: &Path) -> Option<PhysicsWorld> {
    let mut dir = scene_path.parent()?;
    for _ in 0..6 {
        let candidate = dir.join("project.jengine");
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate).ok()?;
            let v: serde_json::Value = serde_json::from_str(&text).ok()?;
            let phys = v.get("settings")?.get("physics")?;
            let backend = phys
                .get("backend")
                .and_then(|b| b.as_str())
                .unwrap_or("");
            if !backend.is_empty() && !backend.eq_ignore_ascii_case("rapier2d") {
                log::info!(
                    "[wgpu-scene] physics backend '{backend}' not supported \
                     (only rapier2d); physics disabled"
                );
                return None;
            }
            let gx = phys.get("gravity_x").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
            let gy = phys.get("gravity_y").and_then(|x| x.as_f64()).unwrap_or(-9.8) as f32;
            let lu = phys.get("length_unit").and_then(|x| x.as_f64()).unwrap_or(100.0) as f32;
            // The fixed step, if the project chose one (1/60 s otherwise). The field
            // was in every project and the editor's settings panel long before
            // anything read it.
            let fixed_timestep = phys.get("fixed_timestep").and_then(|x| x.as_f64()).map(|x| x as f32);
            log::info!(
                "[wgpu-scene] physics: rapier2d gravity=({gx}, {gy}) length_unit={lu} fixed_timestep={}",
                fixed_timestep.unwrap_or(1.0 / 60.0)
            );
            let mut world = PhysicsWorld::new(gx, gy, lu);
            if let Some(dt) = fixed_timestep {
                world.set_fixed_timestep(dt);
            }
            return Some(world);
        }
        dir = dir.parent()?;
    }
    None
}

/// Walk up from `scene_path` looking for a `project.jengine`; when
/// found, return its parent directory (the project root). Used by the
/// graph-pipeline cache to resolve `.shader` and `.material` paths
/// stored in scenes / materials.
fn find_project_root(scene_path: &Path) -> Option<PathBuf> {
    let mut dir = scene_path.parent()?;
    for _ in 0..6 {
        if dir.join("project.jengine").is_file() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
    None
}

/// Backlog 2.2 — read every `*.prefab` file from the project's
/// `assets/prefabs/` directory and return them keyed by the prefab's
/// `name` (falls back to the file stem). The returned map is consumed
/// by both `expand_prefab_instances_in_json` (compile-time scene
/// authoring) and the runtime `spawn_prefab(name)` script API.
fn load_prefab_map(
    scene_path: &Path,
) -> std::collections::HashMap<String, jengine_core::scene::format::EntityData> {
    let mut out = std::collections::HashMap::new();
    let prefabs_root = scene_path
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("prefabs"))
        .unwrap_or_else(|| std::path::PathBuf::from("assets/prefabs"));
    if !prefabs_root.is_dir() {
        return out;
    }
    let entries = match std::fs::read_dir(&prefabs_root) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("prefab") {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(err) => {
                log::warn!("[wgpu-scene] failed to read prefab {}: {}", path.display(), err);
                continue;
            }
        };
        match jengine_core::scene::parse_prefab(&path.to_string_lossy(), &text) {
            Ok((key, entity_data)) => {
                out.insert(key, entity_data);
            }
            Err(err) => {
                log::warn!("[wgpu-scene] failed to parse prefab {}: {}", path.display(), err);
            }
        }
    }
    if !out.is_empty() {
        log::info!("[wgpu-scene] loaded {} prefab(s) from {}", out.len(), prefabs_root.display());
    }
    // A variant extends another prefab (`scene::prefab::resolve_prefab_variants`).
    jengine_core::scene::resolve_prefab_variants(&out)
}


/// Whether this build ships the F3 / F12 developer overlays.
///
/// Reads `settings.debug_overlay` from the project beside the scene. A
/// missing file or key means enabled, so running a scene straight out of a
/// source tree (or from a build made before the option existed) keeps the
/// overlays.
fn load_project_debug_overlay(scene_path: &Path) -> bool {
    let Some(root) = find_project_root(scene_path) else {
        return true;
    };
    let Ok(text) = std::fs::read_to_string(root.join("project.jengine")) else {
        return true;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return true;
    };
    v.get("settings")
        .and_then(|s| s.get("debug_overlay"))
        .and_then(|b| b.as_bool())
        .unwrap_or(true)
}

/// Walk up from `scene_path` looking for a `project.jengine`; when
/// found, return its `settings.input_actions` array. Best-effort —
/// any failure logs at debug level and returns `None`.
fn load_project_input_actions(scene_path: &Path) -> Option<Vec<InputAction>> {
    // 2.5 — prefer a sibling `.input` asset if any are present. Scan the
    // project root for the first `*.input` file (alphabetical) before
    // falling back to the embedded `project.jengine.settings.input_actions`
    // list. This is cheap (one ReadDir) and lets games opt into the
    // asset workflow without touching legacy projects.
    if let Some(project_root) = find_project_root(scene_path)
        && let Ok(read) = std::fs::read_dir(&project_root)
    {
        let mut entries: Vec<_> = read
            .flatten()
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("input"))
            })
            .collect();
        entries.sort_by_key(|e| e.path());
        if let Some(entry) = entries.first()
            && let Ok(text) = std::fs::read_to_string(entry.path())
            && let Ok(Some(map)) = jengine_core::input::InputMap::from_json_str(&text)
        {
            return Some(map.actions);
        }
    }

    let mut dir = scene_path.parent()?;
    for _ in 0..6 {
        let candidate = dir.join("project.jengine");
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate).ok()?;
            let v: serde_json::Value = serde_json::from_str(&text).ok()?;
            let arr = v.get("settings")?.get("input_actions")?.as_array()?;
            let mut out = Vec::with_capacity(arr.len());
            for a in arr {
                if let Ok(action) = serde_json::from_value::<InputAction>(a.clone()) {
                    out.push(action);
                }
            }
            return Some(out);
        }
        dir = dir.parent()?;
    }
    None
}

/// Decode every unique `SpriteComponent.texture_id` referenced by
/// `world` — **and by any prefab** — into the `AssetManager` raw-bytes
/// cache. Already-cached ids are skipped (cheap re-entry on scene reload —
/// only NEW textures pay the decode cost).
///
/// Prefabs matter because nothing in the world references their textures
/// until a script spawns one, which is mid-frame and far too late to read
/// an image off disk. A prefab-only sheet therefore drew nothing at all,
/// silently (BUGS #137).
fn load_world_textures(
    world: &World,
    prefabs: &std::collections::HashMap<String, jengine_core::scene::format::EntityData>,
    candidates: &[PathBuf],
    assets: &mut AssetManager,
) {
    let mut wanted: HashSet<String> = HashSet::new();
    wanted.extend(jengine_core::scene::format::prefab_texture_ids(prefabs.values()));
    // The one list of what a world draws with - shared with the web runtime,
    // whose own copy had drifted from this one (and neither knew `UIImage`).
    wanted.extend(jengine_core::scene::format::world_texture_ids(world));
    let mut loaded = 0usize;
    let mut skipped = 0usize;
    for tex_id in &wanted {
        // `__white` and kin are built in, never files (as `textures_to_request`).
        if assets.get_texture_bytes(tex_id).is_some() || tex_id.starts_with("__") {
            skipped += 1;
            continue;
        }
        match resolve_texture_path(tex_id, candidates) {
            Some(resolved) => {
                match RawTextureData::from_file(resolved.to_string_lossy().as_ref()) {
                    Ok(raw) => {
                        log::info!(
                            "[wgpu-scene] cached texture bytes '{tex_id}' from {} ({}x{})",
                            resolved.display(), raw.width, raw.height
                        );
                        assets.insert_texture_bytes(tex_id.clone(), raw);
                        loaded += 1;
                    }
                    Err(e) => log::warn!(
                        "[wgpu-scene] failed to decode texture '{tex_id}' at {}: {e:#}",
                        resolved.display()
                    ),
                }
            }
            None => log::warn!("{}", not_found("texture", tex_id)),
        }
    }
    log::info!(
        "[wgpu-scene] {} sprite textures cached ({} unique, {} reused, {} missing)",
        loaded,
        wanted.len(),
        skipped,
        wanted.len().saturating_sub(loaded + skipped),
    );
}

/// Load, from disk, the textures the renderer found missing this frame.
fn load_requested_textures(state: &mut WgpuSceneState) {
    let ids = jengine_core::asset::textures_to_request(
        &state.missing_warned,
        &mut state.texture_requested,
        &state.assets,
    );
    for tex_id in ids {
        let Some(path) = resolve_texture_path(&tex_id, &state.texture_roots) else {
            log::warn!("{}", not_found("texture", &tex_id));
            continue;
        };
        match RawTextureData::from_file(path.to_string_lossy().as_ref()) {
            Ok(raw) => {
                log::info!("[wgpu-scene] loaded texture '{tex_id}' on demand ({}x{})", raw.width, raw.height);
                state.assets.insert_texture_bytes(tex_id.clone(), raw);
                state.missing_warned.remove(&tex_id);
            }
            Err(e) => log::warn!("[wgpu-scene] failed to decode texture '{tex_id}' at {}: {e:#}", path.display()),
        }
    }
}

/// Load every `.font` named by a `TextComponent.font_id` in `world`.
///
/// Thin wrapper over [`font_asset::load_fonts`] that reports failures the way
/// the rest of the scene load does: a warning per font, never fatal. A font
/// that fails to load leaves its text rendering in the built-in face, which is
/// visibly wrong but still readable - and the warning names the file.
fn load_world_fonts(world: &World, candidates: &[PathBuf], assets: &mut AssetManager) {
    use jengine_core::ecs::components::GuiComponent;
    use jengine_core::renderer::font_asset;
    let mut wanted = font_asset::collect_font_ids(world);
    // `.gui` widgets carry their own `font_id`, so a document can name a font
    // no `TextComponent` uses. Runs after `load_world_guis`, which is what
    // put these documents in the cache to be scanned.
    for (_, gc) in world.query::<&GuiComponent>().iter() {
        if let Some(doc) = assets.get_gui(&gc.gui_id) {
            font_asset::collect_gui_font_ids(doc, &mut wanted);
        }
    }
    if wanted.is_empty() {
        return;
    }
    for e in font_asset::load_fonts(&wanted, candidates, assets) {
        log::warn!("[wgpu-scene] font: {e}");
    }
}

/// Mirror of `load_world_textures` for `.gui` documents referenced by
/// any `GuiComponent`. Already-cached gui ids are skipped.
fn load_world_guis(world: &World, candidates: &[PathBuf], assets: &mut AssetManager) {
    use jengine_core::ecs::components::GuiComponent;
    use jengine_core::ui::gui_document::{load_gui, GuiNode};
    let mut wanted: HashSet<String> = HashSet::new();
    for (_, gc) in world.query::<&GuiComponent>().iter() {
        if !gc.gui_id.is_empty() {
            wanted.insert(gc.gui_id.clone());
        }
    }

    // Image nodes' textures and rich Text nodes' icons (`GuiNode::texture_ids`).
    fn collect_image_textures(node: &GuiNode, out: &mut HashSet<String>) {
        let mut ids = std::collections::BTreeSet::new();
        node.texture_ids(&mut ids);
        out.extend(ids);
    }

    let mut gui_textures: HashSet<String> = HashSet::new();
    for gui_id in &wanted {
        if assets.get_gui(gui_id).is_some() {
            // Already loaded — still scan its Image nodes so reloads
            // discover newly-referenced textures.
            if let Some(doc) = assets.get_gui(gui_id) {
                collect_image_textures(&doc.root, &mut gui_textures);
            }
            continue;
        }
        let mut found = false;
        if let Some(p) = jengine_core::asset::find_in_roots(gui_id, candidates) {
            match std::fs::read_to_string(&p) {
                Ok(json) => match load_gui(&json) {
                    Ok(doc) => {
                        log::info!(
                            "[wgpu-scene] loaded gui document '{gui_id}' from {}",
                            p.display()
                        );
                        collect_image_textures(&doc.root, &mut gui_textures);
                        assets.insert_gui(gui_id.clone(), doc);
                        found = true;
                    }
                    Err(e) => log::warn!(
                        "[wgpu-scene] failed to parse gui '{gui_id}' at {}: {e:#}",
                        p.display()
                    ),
                },
                Err(e) => log::warn!(
                    "[wgpu-scene] failed to read gui '{gui_id}' at {}: {e:#}",
                    p.display()
                ),
            }
        }
        if !found {
            log::warn!("{}", not_found("gui", gui_id));
        }
    }

    // Pre-load any textures referenced by Image nodes in the loaded
    // gui documents so the UI render pass doesn't drop draws on the
    // first frame.
    for tex_id in &gui_textures {
        // `__` ids are written at runtime, never read from a file: a camera's
        // `target`, a package's minimap. An Image naming one was warned about
        // as "not found" at every scene load (three Strategy Classics games).
        if assets.get_texture_bytes(tex_id).is_some() || tex_id.starts_with("__") {
            continue;
        }
        if let Some(resolved) = resolve_texture_path(tex_id, candidates) {
            match RawTextureData::from_file(resolved.to_string_lossy().as_ref()) {
                Ok(raw) => {
                    log::info!(
                        "[wgpu-scene] cached gui texture bytes '{tex_id}' from {} ({}x{})",
                        resolved.display(), raw.width, raw.height
                    );
                    assets.insert_texture_bytes(tex_id.clone(), raw);
                }
                Err(e) => log::warn!(
                    "[wgpu-scene] failed to decode gui texture '{tex_id}' at {}: {e:#}",
                    resolved.display()
                ),
            }
        } else {
            log::warn!("[wgpu-scene] gui texture '{tex_id}' not found in any search root");
        }
    }
}

/// Slice 12 — resolve a script-supplied scene path string against the
/// candidate roots derived from the current scene path. Extracted out
/// of `reload_scene` so `push_scene` can pre-resolve before snapshotting
/// (failing-fast keeps the stack consistent on bad paths).
fn resolve_scene_path(state: &WgpuSceneState, requested: &str) -> Result<PathBuf> {
    let direct = PathBuf::from(requested);
    if direct.is_file() {
        return Ok(direct);
    }
    let roots = candidate_roots(&state.scene_path);
    // The roots, then a downloaded asset pack (`asset::packs`).
    if let Some(p) = jengine_core::asset::find_in_roots(requested, &roots) {
        return Ok(p);
    }
    let hint = pack_hint(requested).map(|h| format!(" (it is {h})")).unwrap_or_default();
    Err(anyhow::anyhow!(
        "could not resolve scene path '{requested}' relative to '{}'{hint}",
        state.scene_path.display()
    ))
}

/// Slice 12 — serialize the current world to a v2 JSON string suitable
/// for restoration via `apply_scene`. Uses the same component
/// registry that the load path builds (`register_core_components`) so
/// every script-mutated component round-trips with full fidelity.
fn snapshot_world(
    world: &hecs::World,
    scene_name: &str,
    registry: &ComponentRegistry,
) -> Result<String> {
    // Takes the live registry rather than rebuilding a core-only one: a
    // snapshot built without the package components would silently drop them
    // on restore.
    let mut snap = jengine_core::scene::snapshot_scene_v2(world, registry);
    snap.name = scene_name.to_string();
    serde_json::to_string(&snap).context("serializing scene snapshot")
}

/// 7-prep wedge 7 — apply a `SceneAction::Load` requested by a
/// script. Resolves `requested` against the current scene's parent
/// dir then against `candidate_roots`, swaps the world, decodes any
/// new textures + GUIs into the existing asset cache, resets the
/// particle simulation, and updates `state.scene_path` /
/// `state.scene_name`. Slice 12 also wires Push/Pop via the same
/// `apply_scene` helper that this fn delegates to.
fn reload_scene(state: &mut WgpuSceneState, requested: &str) -> Result<()> {
    let path = resolve_scene_path(state, requested)?;
    let json = std::fs::read_to_string(&path)
        .with_context(|| format!("reading scene file {}", path.display()))?;
    // Every scene a script loads comes through here - every game launched
    // from a hub - and this used to skip what `build_state` did: an external
    // tilemap arrived empty (a platformer had no ground), clips and emitters
    // without their files - and, until Oct 2026, its prefab instances, which
    // loaded unexpanded natively alone. Not in `apply_scene`, which also
    // restores `pop_scene` snapshots: re-reading the files there would undo
    // tiles the game had changed.
    let scene = prepare_scene(&json, &path, state.script_system.api.services.prefabs(), "wgpu-scene")?;
    let scene_name = jengine_core::scene::name_in(&scene).unwrap_or_else(|| {
        path.file_stem().and_then(|s| s.to_str()).unwrap_or("scene").to_string()
    });
    apply_scene(state, scene, path, scene_name)
}

/// A scene file's text, parsed - the one parse it gets - with its `.anim`,
/// `.particle` and `.tilemap` references inlined and its prefab instances
/// expanded: what every scene a script enters or streams in goes through.
fn prepare_scene(
    json: &str,
    path: &Path,
    prefabs: &std::collections::HashMap<String, jengine_core::scene::format::EntityData>,
    context: &str,
) -> Result<serde_json::Value> {
    let mut scene: serde_json::Value =
        serde_json::from_str(json).with_context(|| format!("parsing scene file {}", path.display()))?;
    inline_scene_sources(&mut scene, &candidate_roots(path));
    jengine_core::scene::expand_prefab_instances_in_value(&mut scene, prefabs, context);
    Ok(scene)
}

/// `settings.liveops` (`jengine_core::liveops`), then `settings.asset_packs`
/// and the build's `assets/packs.json` (`jengine_core::asset::packs`): the
/// live ops snapshot believed last time - whose content release the packs
/// are laid under before anything mounts - and what this game's packs are,
/// and which are cached already.
#[cfg(feature = "packs")]
fn init_asset_packs(scene_path: &Path) {
    use jengine_core::asset::packs::{PackSettings, native};
    let root = find_project_root(scene_path);
    let settings = root
        .as_ref()
        .and_then(|r| std::fs::read_to_string(r.join("project.jengine")).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("settings").cloned())
        .unwrap_or_default();
    #[cfg(feature = "liveops")]
    let live = jengine_core::liveops::native::init(&settings);
    #[cfg(not(feature = "liveops"))]
    let live = None;
    let packs = PackSettings::from_settings(&settings);
    let assets_root = root.map(|r| r.join("assets")).unwrap_or_else(|| PathBuf::from("assets"));
    native::init(&assets_root, &packs, candidate_roots(scene_path), live.as_ref());
}

/// Start the pack downloads scripts asked for; once a pack has mounted, ask
/// again for the textures given up on while it was remote, and add what it
/// brought to what this runtime read once at startup - its prefabs (and
/// their textures), data tables, skeletal clips and rigs. Its scripts need
/// nothing: they are read when an entity first uses one, through
/// `find_in_roots`, and `require` looks in the packs itself.
#[cfg(feature = "packs")]
fn sync_asset_packs(state: &mut WgpuSceneState) {
    use jengine_core::asset::packs;
    // Live ops first: a snapshot it believes may change the packs.
    #[cfg(feature = "liveops")]
    jengine_core::liveops::native::sync();
    packs::native::sync();
    let generation = packs::generation();
    if generation != state.packs_generation {
        state.packs_generation = generation;
        state.texture_requested.clear();
    }
    for content in packs::native::arrived() {
        if content.is_empty() {
            continue;
        }
        let added = content.add_prefabs(state.script_system.api.services.prefabs_mut());
        if !added.is_empty() {
            let new: std::collections::HashMap<_, _> = added
                .iter()
                .filter_map(|n| state.script_system.api.services.prefabs().get(n).map(|d| (n.clone(), d.clone())))
                .collect();
            load_world_textures(&World::new(), &new, &state.texture_roots, &mut state.assets);
        }
        let mut tables = 0;
        if !content.tables.is_empty() || !content.schemas.is_empty() {
            let mut registry = jengine_core::data::global().map(|r| (*r).clone()).unwrap_or_default();
            tables = content.add_data(&mut registry);
            jengine_core::data::install_global(std::sync::Arc::new(registry.clone()));
            state.script_system.set_data_registry(std::rc::Rc::new(registry));
        }
        let skeletal = content.add_skeletal(&mut state.assets);
        log::info!(
            "[packs] '{}' brought {} prefab(s), {tables} data table(s), {skeletal} skeletal clip(s) and rig(s), {} script(s)",
            content.name,
            added.len(),
            content.scripts.len()
        );
    }
}

/// `jengine_core::scene::streaming` this frame (`streaming_native`): start
/// what scripts asked for, take in the textures the workers decoded, and lay
/// in each chunk whose last texture has arrived. A chunk's scripts start on
/// the next script run, as any new scripted entity's do.
fn sync_streaming(state: &mut WgpuSceneState) {
    use jengine_core::scene::streaming::{self as st, Request, Status};
    for req in st::take_requests() {
        match req {
            Request::Unload { handle } => {
                let gone = st::unload_chunk(&mut state.world, handle);
                state.streaming.jobs.retain(|j| j.handle != Some(handle));
                log::info!("[stream] chunk {handle} unloaded ({} entities)", gone.len());
            }
            Request::Additive { handle, path, offset } => start_stream_job(state, Some(handle), &path, offset),
            Request::Preload { path } => start_stream_job(state, None, &path, [0.0, 0.0]),
        }
    }
    while let Ok((id, tex, raw)) = state.streaming.rx.try_recv() {
        let Some(job) = state.streaming.jobs.iter_mut().find(|j| j.id == id) else { continue };
        match raw {
            Ok(raw) => state.assets.insert_texture_bytes(tex.clone(), raw),
            Err(e) => log::warn!("[stream] texture '{tex}' did not decode: {e}"),
        }
        job.waiting.remove(&tex);
    }
    let mut done = Vec::new();
    for (i, job) in state.streaming.jobs.iter().enumerate() {
        if job.waiting.is_empty() {
            done.push(i);
            continue;
        }
        let p = job.progress();
        match job.handle {
            Some(h) => st::set_status(h, Status::Loading(p)),
            None => st::set_preload(&job.requested, Status::Loading(p)),
        }
    }
    for i in done.into_iter().rev() {
        let mut job = state.streaming.jobs.remove(i);
        let Some(handle) = job.handle else {
            st::set_preload(&job.requested, Status::Loaded);
            log::info!("[stream] preloaded '{}' ({} textures)", job.requested, job.total);
            continue;
        };
        if !st::is_wanted(handle) {
            continue;
        }
        let chunk = job.chunk.take().unwrap_or_default();
        let candidates = candidate_roots(&job.scene_path);
        load_world_guis(&chunk, &candidates, &mut state.assets);
        load_world_fonts(&chunk, &candidates, &mut state.assets);
        let pairs = jengine_core::scene::streaming::merge(&mut state.world, chunk, handle, glam::Vec2::from(job.offset));
        st::set_status(handle, Status::Loaded);
        log::info!("[stream] chunk {handle} '{}' laid in ({} entities)", job.requested, pairs.len());
    }
}

/// Read and parse a scene for streaming, and set its textures decoding.
fn start_stream_job(state: &mut WgpuSceneState, handle: Option<u32>, requested: &str, offset: [f32; 2]) {
    use jengine_core::scene::streaming::{self as st, Status};
    let fail = |why: String| {
        log::warn!("[stream] '{requested}' failed: {why}");
        match handle {
            Some(h) => st::set_status(h, Status::Failed),
            None => st::set_preload(requested, Status::Failed),
        }
    };
    let path = match resolve_scene_path(state, requested) {
        Ok(p) => p,
        Err(e) => return fail(format!("{e:#}")),
    };
    let json = match std::fs::read_to_string(&path) {
        Ok(j) => j,
        Err(e) => return fail(format!("reading {}: {e}", path.display())),
    };
    let candidates = candidate_roots(&path);
    let scene = match prepare_scene(&json, &path, state.script_system.api.services.prefabs(), "stream") {
        Ok(s) => s,
        Err(e) => return fail(format!("{e:#}")),
    };
    // Sounds and atlases are small and read here; textures go to a worker.
    for e in jengine_core::renderer::atlas_asset::resolve_atlas_refs(&scene, &candidates, &mut state.assets) {
        log::warn!("[stream] atlas: {e}");
    }
    for e in jengine_core::audio::audio_asset::resolve_audio_refs(&scene, &candidates, &mut state.assets) {
        log::warn!("[stream] audio: {e}");
    }
    let mut chunk = World::new();
    if let Err(e) = load_scene_value(scene, &mut chunk, &state.registry) {
        return fail(format!("{e:#}"));
    }
    let mut files = Vec::new();
    for tex in jengine_core::scene::format::world_texture_ids(&chunk) {
        if tex.starts_with("__") || state.assets.get_texture_bytes(&tex).is_some() {
            continue;
        }
        match resolve_texture_path(&tex, &candidates) {
            Some(p) => files.push((tex, p)),
            None => log::warn!("[stream] texture '{tex}' not found in any search root"),
        }
    }
    state.streaming.next_id += 1;
    let id = state.streaming.next_id;
    let total = files.len();
    let waiting = files.iter().map(|(t, _)| t.clone()).collect();
    state.streaming.decode(id, files);
    state.streaming.jobs.push(crate::streaming_native::Job {
        id,
        handle,
        requested: requested.to_string(),
        offset,
        scene_path: path,
        chunk: handle.map(|_| chunk),
        waiting,
        total,
    });
}

/// Inline a scene's `.anim` / `.particle` / `.tilemap` references
/// (`scene::sources`), logging what was found and what was not. A scene
/// that is not valid JSON is returned as it is, for the loader to report.
fn inline_scene_sources(value: &mut serde_json::Value, candidates: &[PathBuf]) {
    let errors = jengine_core::scene::sources::inline_sources(value, candidates);
    for e in &errors {
        log::warn!("[wgpu-scene] source resolve: {e:#}");
    }
    let (anims, particles, tilemaps) = jengine_core::scene::sources::count_sources(value);
    if anims + particles + tilemaps > 0 {
        log::info!(
            "[wgpu-scene] inlined {anims} .anim, {particles} .particle, {tilemaps} .tilemap              reference(s) ({} error(s))",
            errors.len()
        );
    }
}

/// Slice 12 — swap `state.world` to whatever is in `json` and rebuild
/// every per-scene cache + system that `reload_scene` used to touch
/// inline. Shared by `reload_scene` (file-backed) and the push/pop
/// path (in-memory snapshot). `path` is stored on the state so future
/// relative asset resolution and `push_scene` / `pop_scene` walks the
/// correct candidate roots — when restoring a snapshot this is the
/// original scene path, not a temp file.
fn apply_scene(
    state: &mut WgpuSceneState,
    scene: serde_json::Value,
    path: PathBuf,
    scene_name: String,
) -> Result<()> {
    // The scene-wide fields, kept; the scene itself goes to the loader whole.
    let scene_header = jengine_core::scene::format::scene_header(&scene);
    let mut new_world = World::new();
    load_scene_value(scene, &mut new_world, &state.registry)
        .with_context(|| format!("parsing scene source for {}", path.display()))?;
    // A new world owes no fixed-step time and has nothing to interpolate -
    // and steps at its own `simulation_hz` if it chose one.
    state.fixed_step.enter_scene(jengine_core::ecs::fixed_step::scene_simulation_hz(&scene_header));
    // Spawn handles named entities of the scene being replaced.
    jengine_core::scripting::spawn_aliases::clear();

    let candidates = candidate_roots(&path);
    state.texture_roots = candidates.clone();
    log::info!(
        "[wgpu-scene] applied '{scene_name}' from {} ({} entities)",
        path.display(),
        new_world.len()
    );

    let prefabs = state.script_system.api.services.prefabs().clone();
    load_world_textures(&new_world, &prefabs, &candidates, &mut state.assets);
    load_world_guis(&new_world, &candidates, &mut state.assets);
    load_world_fonts(&new_world, &candidates, &mut state.assets);

    // Slice 8 — re-run atlas + audio refs against the reloaded
    // scene JSON so any new `atlas_refs` / `audio_refs` are
    // preloaded into the asset cache. Errors are logged, never
    // fatal, matching `build_state`.
    {
        let value = &scene_header;
        let slerrs = jengine_core::asset::sort_layers::resolve_scene_sort_layers(
            value,
            &candidates,
            &mut state.assets,
        );
        for e in &slerrs {
            log::warn!("[wgpu-scene] sort_layers resolve (reload): {e}");
        }
        let aerrs = jengine_core::renderer::atlas_asset::resolve_atlas_refs(
            value,
            &candidates,
            &mut state.assets,
        );
        for e in &aerrs {
            log::warn!("[wgpu-scene] atlas resolve (reload): {e}");
        }
        let auerrs = jengine_core::audio::audio_asset::resolve_audio_refs(
            value,
            &candidates,
            &mut state.assets,
        );
        for e in &auerrs {
            log::warn!("[wgpu-scene] audio resolve (reload): {e}");
        }
    }

    // The outgoing scene's scripts are about to stop existing. Fire their
    // `on_despawn` while the old world is still in place, so a handler that
    // reads its own entity gets real values — and, more importantly, so it
    // gets the chance at all. Nothing else in the swap can see the old
    // entities: by the time `reset_initialized()` runs below, the new scene
    // has already claimed the same entity ids.
    state
        .script_system
        .notify_scene_teardown(&mut state.world, None, Some(&mut state.assets), 0.0);

    state.world = new_world;
    // Its additive chunks went with the old world; preloads stay, being
    // for scenes still to come.
    jengine_core::scene::streaming::scene_replaced();
    state.streaming.jobs.retain(|j| j.handle.is_none());
    jengine_core::scene::set_current_name(&scene_name);
    state.scene_name = scene_name;
    state.scene_path = path;
    state.script_system.api.services.particle_system = ParticleSystem::new();
    state.missing_warned.clear();
    // P1.5 — hot-reload candidate roots depend on the active scene
    // (engine convention: `<project>/assets/scenes/foo.scene` with
    // texture ids relative to `<project>/assets/`). Refresh the strip
    // roots so post-reload watcher events resolve into the new scene's
    // asset-id namespace.
    if let Some(hr) = state.hot_reload.as_mut() {
        hr.rebind_roots(candidate_roots(&state.scene_path));
    }
    // Slice 9 — drop the old PhysicsWorld; body handles are tied to
    // the old world's entities. The new world's bodies are
    // (re)created lazily by `ensure_physics_bodies` on the next tick.
    // Re-read `project.jengine` so scenes in different projects honor
    // their own gravity / length_unit.
    state.physics = build_physics_from_project(&state.scene_path);
    // Phase 8.2e — replace the post-process effect chain with whatever
    // the new scene declares (or clear it if the scene has none).
    state
        .post_process
        .set_settings(jengine_core::renderer::postprocess::PostProcessSettings::from_scene_value(&scene_header).unwrap_or_default());
    // Wedge 9 — reset the persistent camera so the new scene
    // starts cleanly (zoom 1.0, position zero) instead of inheriting
    // the prior scene's smoothed-follow position. The next frame's
    // `update_camera` call will snap to the new scene's active
    // camera entity (or its `CameraFollow` target on a slight delay).
    let (vw, vh) = state.renderer.viewport_size();
    state.camera = Camera2D::new((vw as f32).max(1.0), (vh as f32).max(1.0));
    // Critical — clear the script system's per-entity init flags so
    // scripts on the new scene fire `on_ready` on their first tick.
    // Without this, stale handles from the old world would also keep
    // re-issuing whatever load_scene the script's setup path runs,
    // causing an infinite reload loop.
    state.script_system.reset_initialized();

    // Wedge 12 — re-bind behavior-graph instances against the new
    // world. Detaches stale entity instances, fires `OnSpawn` for
    // freshly-spawned graph entities. Errors are logged but
    // non-fatal (the entity stays in `failed_entities` until the
    // next sync).
    let bg_root = std::path::Path::new("assets");
    let bg_errors = state.behavior_system.sync_instances(&mut state.world, bg_root);
    for err in &bg_errors {
        log::warn!(
            "[wgpu-scene] behavior graph load failed for entity {:?} ({}): {}",
            err.entity, err.path.display(), err.source
        );
    }

    Ok(())
}

/// 7-prep wedge 8 — run the standard ECS update phase against
/// `world`. Order mirrors the gl AppState's RedrawRequested handler
/// (see `crates/jengine-runtime/src/main.rs`):
///
/// 1. `update_input_actions` — InputAxis2D / InputActionImpulse
///    write Velocity2D BEFORE this frame's integration.
/// 2. `velocity_system` — gravity + integrate previous frame's
///    velocity into Transform.position.
/// 3. Motion behaviors — accel, seek, max_speed, follow, oscillator,
///    tween, patrol_paths.
/// 4. Orientation — face_velocity, angular_velocity.
/// 5. Bounds — wrap, clamp, despawn_out_of_bounds.
/// 6. Transient effects — shake, flicker (with their expiry sweeps).
/// 7. Triggers — update_trigger_areas (after motion so this frame's
///    positions are accounted for).
/// 8. Animations — sprite, ui, animation state machines.
/// 9. UI — buttons, gui focus tick (mouse-pos & focus state only;
///    actual input dispatch lives in the script API).
/// 10. Lifetimes — `update_lifetimes` then despawn.
///
/// Behaviour graphs and physics stay OUT of this function — they need state
/// `WgpuSceneState` owns, so the caller drives them around this call.
///
/// Package systems stay out for the same reason and are no longer homeless:
/// `WgpuSceneState` owns a `PackageHost`, and the caller runs each
/// `SystemPhase` at the matching point in the frame. (This comment used to say
/// they "need the SystemSchedule + PackageRegistry the gl AppState owns" —
/// that AppState had not existed since the glow cut, which is exactly why no
/// package system ran at all. See BUGS.md #13.)
///
/// Batch scripts stay out too: they need the worker pool the state owns, so
/// `run_batch_scripts_if_any` drives them from the caller.
/// Run every `BatchScriptComponent` in the world, building the worker pool on
/// first use.
///
/// Split out of `tick_world` because the pool lives on `WgpuSceneState` and
/// `tick_world` deliberately takes only the world. `run_batch_scripts` had no
/// caller in any runtime, which made `BatchScriptComponent` — and the whole
/// Slice 5.3 batch vocabulary (`Drag2D`, `Force2D`, `Wind2D`, `Impulse2D`,
/// `Torque2D`) that exists only to serve it — inert everywhere but unit tests.
///
/// Native-only by construction: the pool is threads plus a QuickJS runtime,
/// neither of which exists on wasm, so batch scripts stay a native
/// optimisation rather than a portable feature. Nor on Android or iOS, where
/// QuickJS is not built (see the stub below).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn run_batch_scripts_if_any(state: &mut WgpuSceneState, dt: f32) {
    use jengine_core::ecs::components::{BatchScriptComponent, ScriptLang};

    // Cheap early-out: the query is over one component and most scenes have
    // none, so an unused feature costs a query per frame and nothing else.
    if state
        .world
        .query::<&BatchScriptComponent>()
        .iter()
        .next()
        .is_none()
    {
        return;
    }

    if !state.batch_pool_tried {
        state.batch_pool_tried = true;
        // Direct mode: one in-process runtime, no worker threads. The pooled
        // mode exists for the perf harness; a scene runner wants predictable
        // startup over throughput, and Direct is also the documented fallback
        // when worker spawn fails.
        match jengine_core::scripting::worker_pool::BatchScriptPool::new_direct(None) {
            Ok(pool) => {
                log::info!("[wgpu-scene] batch script pool ready (direct mode)");
                state.batch_pool = Some(pool);
            }
            Err(e) => log::error!("[wgpu-scene] batch script pool unavailable: {e}"),
        }
    }

    let Some(pool) = state.batch_pool.as_mut() else { return };

    // Load any batch script the world references but the pool has not seen.
    //
    // `run_batch_scripts` silently `continue`s past a script that is not
    // loaded, and **nothing anywhere loaded them** — only unit tests and the
    // perf harness ever called `load_js` / `load_lua`. So wiring the
    // dispatcher in was necessary but not sufficient; without this the batch
    // components stay just as inert, with no error to explain why.
    let mut wanted: Vec<(ScriptLang, String)> = Vec::new();
    for (_, batch) in state.world.query::<&BatchScriptComponent>().iter() {
        let key = (batch.lang, batch.path.clone());
        if !wanted.contains(&key) {
            wanted.push(key);
        }
    }
    for (lang, path) in wanted {
        let already = match lang {
            ScriptLang::Js => pool.is_js_loaded(&path),
            ScriptLang::Lua => pool.is_lua_loaded(&path),
        };
        if already || state.batch_load_failed.contains(&path) {
            continue;
        }
        let Some(root) = state.project_root.as_ref() else {
            log::warn!("[wgpu-scene] batch script '{path}': no project root to resolve against");
            state.batch_load_failed.insert(path);
            continue;
        };
        let full = root.join("assets").join(&path);
        let source = match std::fs::read_to_string(&full) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[wgpu-scene] batch script '{}': {e}", full.display());
                state.batch_load_failed.insert(path);
                continue;
            }
        };
        let loaded = match lang {
            ScriptLang::Js => pool.load_js(&path, &source),
            ScriptLang::Lua => pool.load_lua(
                &path,
                &source,
                jengine_core::scripting::worker_pool::BatchLuaBufferMode::Buffer,
            ),
        };
        match loaded {
            Ok(()) => log::info!("[wgpu-scene] batch script loaded: {path}"),
            Err(e) => {
                // Recorded so a script with a bad schema is reported once
                // rather than re-read and re-rejected every frame.
                log::error!("[wgpu-scene] batch script '{path}' rejected: {e}");
                state.batch_load_failed.insert(path);
            }
        }
    }

    jengine_core::ecs::behaviors::run_batch_scripts(
        &mut state.world,
        pool,
        dt,
        &mut state.batch_counters,
    );
}

/// Android and iOS build no QuickJS (`rquickjs` has no bindings for either),
/// and the batch pool is built on it - so a batch script is inert there. Say
/// so once, rather than leaving the scene to wonder why nothing moves.
#[cfg(any(target_os = "android", target_os = "ios"))]
fn run_batch_scripts_if_any(state: &mut WgpuSceneState, _dt: f32) {
    use jengine_core::ecs::components::BatchScriptComponent;
    if state.batch_pool_tried {
        return;
    }
    if state.world.query::<&BatchScriptComponent>().iter().next().is_some() {
        state.batch_pool_tried = true;
        log::warn!("[wgpu-scene] batch scripts are not supported on mobile; they will not run");
    }
}

/// P1.5 — pull pending filesystem changes from the optional
/// `AssetWatcher` and route each one to the right reload entry
/// point. No-op when hot-reload is disabled. Errors during a single
/// reload are logged and swallowed so one bad file doesn't stop the
/// remaining changes (or kill the runtime).
fn drain_hot_reload(state: &mut WgpuSceneState) {
    use crate::hot_reload::HotReloadKind;
    let Some(hr) = state.hot_reload.as_ref() else {
        return;
    };
    let changes = hr.drain();
    if changes.is_empty() {
        return;
    }
    // Snapshot current scene id (relative to its candidate roots) so
    // we can match `.scene` events against it without re-stripping.
    let scene_rel = {
        let roots = candidate_roots(&state.scene_path);
        crate::hot_reload::classify(&state.scene_path, &roots).asset_id
    };
    for change in changes {
        match &change.kind {
            HotReloadKind::Scene => {
                if !change.asset_id.is_empty() && change.asset_id == scene_rel {
                    log::info!(
                        "[HotReload] scene `{}` changed — reloading",
                        change.asset_id
                    );
                    if let Err(e) = reload_scene(state, &change.asset_id) {
                        log::warn!(
                            "[HotReload] scene reload failed for `{}`: {e:?}",
                            change.asset_id
                        );
                    }
                } else {
                    log::debug!(
                        "[HotReload] ignoring scene `{}` (not the active scene)",
                        change.absolute.display()
                    );
                }
            }
            HotReloadKind::LuaScript | HotReloadKind::JsScript => {
                if change.asset_id.is_empty() {
                    log::debug!(
                        "[HotReload] script change at `{}` not under any candidate root — skipping",
                        change.absolute.display()
                    );
                    continue;
                }
                let hit = state.script_system.reload_script_path(&change.asset_id);
                if hit {
                    log::info!(
                        "[HotReload] script `{}` reloaded (next frame re-attaches)",
                        change.asset_id
                    );
                } else {
                    log::debug!(
                        "[HotReload] script `{}` changed but no entity had it loaded",
                        change.asset_id
                    );
                }
            }
            HotReloadKind::Texture => {
                if change.asset_id.is_empty() {
                    log::debug!(
                        "[HotReload] texture change at `{}` not under any candidate root — skipping",
                        change.absolute.display()
                    );
                    continue;
                }
                let abs_str = change.absolute.to_string_lossy().to_string();
                match state.assets.reload_texture_bytes(&change.asset_id, &abs_str) {
                    Ok(()) => {
                        // Force the next sprite draw to notice the texture
                        // is gone from the missing-warned set so any
                        // "missing" sticky warning clears cleanly.
                        state.missing_warned.remove(&change.asset_id);
                        log::info!(
                            "[HotReload] texture `{}` reloaded",
                            change.asset_id
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "[HotReload] texture reload failed for `{}`: {e:?}",
                            change.asset_id
                        );
                    }
                }
            }
            HotReloadKind::Unsupported(label) => {
                log::info!(
                    "[HotReload] TODO: {label} reload not yet wired (changed: {})",
                    change.absolute.display()
                );
            }
            HotReloadKind::Irrelevant => {}
        }
    }
}

thread_local! {
    /// The capture `JENGINE_PROFILE_CAPTURE` asks for: decided on the first
    /// frame, `Some(None)` when not asked.
    static PROFILE: std::cell::RefCell<Option<Option<jengine_core::profiling::trace::CaptureWriter>>> =
        const { std::cell::RefCell::new(None) };
}

/// One frame, and then the end of this frame's input edges.
///
/// The edges are cleared here, after the frame however it ended, because
/// the frame has several early exits - a scene load, a lost surface, a
/// quit - and each one that returned before a `begin_frame` at its bottom
/// kept a key's just-pressed alive into the next frame. A Page Down that
/// loaded a scene was then read again by the new scene, which loaded the
/// next: Test Lab ran from scene 02 to 226 in a couple of seconds.
fn render_frame(state: &mut WgpuSceneState) -> Result<()> {
    let result = render_frame_body(state);
    state.input.begin_frame();
    result
}

fn render_frame_body(state: &mut WgpuSceneState) -> Result<()> {
    // P1.5 — drain any filesystem changes the watcher caught since
    // the previous frame. Cheap when nothing happened (the inner
    // `drain_changed` returns an empty vec); routes scene / script /
    // texture reloads through the existing entry points.
    drain_hot_reload(state);

    // Wedge 3 \u2014 advance the particle simulation. dt clamped to a
    // sane upper bound so a paused-window resume doesn't burst-spawn
    // a frame's worth of particles.
    let now = Instant::now();
    let raw_dt = (now - state.last_frame).as_secs_f32();
    state.last_frame = now;

    // Re-read the refresh rate a few times a second rather than per frame:
    // `current_monitor` is a window→monitor lookup, and the answer only
    // changes when the window is dragged to another display.
    if state.pacer_recheck_in == 0 {
        let rate = state
            .window
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz());
        let before = state.pacer.period();
        state.pacer.set_refresh_millihertz(rate);
        // Logged on change only, which covers both startup and a drag onto
        // a display with a different rate. A `None` here means the pacer is
        // disabled and dt is raw - worth seeing rather than guessing at.
        if state.pacer.period() != before {
            match state.pacer.period() {
                Some(p) => log::info!("frame pacing: snapping dt to {:.2} Hz", 1.0 / p),
                None => log::info!("frame pacing: no usable refresh rate, dt left raw"),
            }
        }
        state.pacer_recheck_in = 30;
    } else {
        state.pacer_recheck_in -= 1;
    }

    // Snap before the clamp: a stall is left un-snapped by the pacer and
    // still has to be capped, and the clamp must be the last word so a
    // resumed window cannot burst a frame's worth of simulation.
    let frame_dt = state.pacer.snap(raw_dt);
    // Through the input replay (`input::replay`): kept while recording,
    // handed back while playing.
    let real_dt = jengine_core::input::replay::frame_dt(frame_dt.clamp(0.0, 0.1));
    let dt = real_dt * state.time_scale;
    // `JENGINE_DT_LOG=1`: every frame's wall delta beside the one the
    // simulation got - the only way to see pacing, which no average shows.
    static DT_LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *DT_LOG.get_or_init(|| std::env::var_os("JENGINE_DT_LOG").is_some()) {
        log::info!("[dt] raw {:.3} ms  paced {:.3} ms", raw_dt * 1000.0, frame_dt * 1000.0);
    }

    // Overlay stats. The *raw* wall-clock frame time - not the time-scaled
    // `dt`, so a slow-motion effect doesn't read as a slow frame, and not
    // the snapped one, so the readout keeps measuring what the frame
    // actually cost instead of echoing the refresh rate back. Memory is two
    // atomic loads (see `profiling::memory`) - `memory_bytes` and
    // `memory_live_bytes` agree off wasm.
    if raw_dt > 0.0 {
        let inst = 1.0 / raw_dt;
        state.fps_ema = if state.fps_ema > 0.0 {
            state.fps_ema * 0.9 + inst * 0.1
        } else {
            inst
        };
    }
    state.frame_stats.fps = state.fps_ema;
    state.frame_stats.frame_time_ms = raw_dt * 1000.0;
    state.frame_stats.memory_bytes = jengine_core::profiling::memory::resident_bytes() as u64;
    state.frame_stats.memory_live_bytes = jengine_core::profiling::memory::live_bytes() as u64;
    state.frame_stats.memory_peak_bytes = jengine_core::profiling::memory::peak_bytes() as u64;

    let (vw, vh) = state.renderer.viewport_size();
    state.camera.resize(vw as f32, vh as f32);
    // The screen's size, for `get_screen_size` and the UI.
    state.script_system.api.input.screen_size = (vw as f32, vh as f32);
    state.input.screen_size = (vw as f32, vh as f32);

    // Fingers first, so a stick press and the key it holds reach every
    // reader - behaviour graphs, packages, scripts - on the same frame.
    {
        let controls = touch_controls_of(&state.world);
        state.touch.tick(
            controls.as_ref(),
            &mut state.input,
            &state.ui_scale,
            (vw as f32, vh as f32),
        );
    }
    // Poll pads before anything reads actions, so a stick press and the
    // action it drives land on the same frame.
    if let Some(gamepad) = state.gamepad.as_mut() {
        gamepad.poll(&mut state.input);
    }
    #[cfg(target_os = "android")]
    crate::gamepad_android::poll(&mut state.input);
    // Hold / tap / double-tap timing, now every device has reported.
    state.input.update_interactions();
    // The true pose back in everything the last frame drew blended: the
    // physics' bodies, and with fixed steps everything a step moved.
    if let Some(physics) = state.physics.as_mut() {
        physics.restore_stepped_poses(&mut state.world);
    }
    state.fixed_step.restore(&mut state.world);
    // The gameplay: once with this frame's `dt`, or - with
    // `settings.simulation_hz` - as many fixed steps as real time pays for
    // (`ecs::fixed_step`), each seeing a press once.
    // Once a frame, before any step: what must happen even on a frame that
    // runs no step - reading the network, opening a lockstep gate.
    state.package_host.run_phase_with(
        SystemPhase::Frame,
        &mut state.world,
        real_dt,
        jengine_core::package::host::FrameResources { input: &mut state.input, assets: &mut state.assets, camera: state.camera },
    );
    let plan = state.fixed_step.plan(real_dt, state.time_scale);
    for step in 0..plan.steps {
        if step > 0 {
            state.input.clear_edges();
        }
        state.fixed_step.begin_step(&state.world);
        simulation_step(state, plan.dt)?;
    }
    if plan.steps == 0 {
        state.input.hold_edges();
    }
    // Everything a step moved, drawn between the last two steps - before the
    // camera follows it, or a followed thing would judder against the view.
    state.fixed_step.blend_for_draw(&mut state.world);

    state.audio.update();
    // After `update`, so sounds that finished this frame are pruned, and every
    // frame, so a bus change reaches playing sounds without waiting for some
    // other audio command.
    jengine_core::audio::sync_mixer(&mut *state.audio, &mut state.script_system.api.services.mixer);
    // The music - crossfades, layers, a playlist - and its playhead, for
    // `get_music_position` and packages.
    jengine_core::audio::music::drive(&mut jengine_core::audio::music::NativeOut {
        audio: &mut *state.audio,
        assets: &state.assets,
        mixer: &mut state.script_system.api.services.mixer,
    });

    // Slice 12.A — drain queued fade-overlay transitions before scene
    // actions so a `fade_in()` queued from the outgoing scene's last
    // tick still arrives at the runner; the overlay state lives on
    // `state` (outside the world) so it survives reload/push/pop.
    let transitions = state.scene_manager.take_pending_transitions();
    for t in transitions {
        use jengine_core::scene::manager::TransitionRequest;
        state.fade_active = Some(match t {
            TransitionRequest::FadeIn { duration_s, color } => {
                jengine_core::fade_overlay::TransitionState::new_fade_in(duration_s, color)
            }
            TransitionRequest::FadeOut { duration_s, color } => {
                jengine_core::fade_overlay::TransitionState::new_fade_out(duration_s, color)
            }
            TransitionRequest::Flash { duration_s, color } => {
                jengine_core::fade_overlay::TransitionState::new_flash(
                    duration_s,
                    [color[0], color[1], color[2]],
                    color[3],
                )
            }
        });
    }

    // Drain any queued scene action. Slice 12 wires Push/Pop on top
    // of the same `apply_scene` pipeline that Load uses, with an
    // in-memory v2 snapshot stack on `state.scene_stack`. All three
    // arms `return Ok(())` so the rest of this frame doesn't run on a
    // half-loaded world.
    // A scene transition needs the old scene presented (and copied) once
    // before it goes, and this frame would not be: hold the change a frame.
    let hold = jengine_core::renderer::transition::holds_scene_change();
    if let Some(action) = if hold { None } else { state.scene_manager.take_pending() } {
        match action {
            SceneAction::SetTimeScale(s) => {
                state.time_scale = s.max(0.0);
                jengine_core::time::set_world_scale(state.time_scale);
            }
            SceneAction::Quit => {
                log::info!("[wgpu-scene] script requested Quit — shutting down");
                state.quit_requested = true;
                return Ok(());
            }
            SceneAction::Load(p) => {
                log::info!("[wgpu-scene] script requested Load({p}) — reloading");
                if let Err(e) = reload_scene(state, &p) {
                    log::warn!("[wgpu-scene] scene reload failed: {e:#}");
                }
                // After a reload everything below this point would be
                // operating on a fresh world that hasn't been ticked
                // yet — bail out for this frame; the next frame will
                // run normally on the new scene.
                return Ok(());
            }
            SceneAction::Push(p) => {
                log::info!(
                    "[wgpu-scene] script requested Push({p}) — depth {} -> {}",
                    state.scene_stack.len(),
                    state.scene_stack.len() + 1
                );
                // Pre-resolve target so a bad path doesn't corrupt the
                // stack with a snapshot we can't navigate back to.
                match resolve_scene_path(state, &p) {
                    Err(e) => log::warn!("[wgpu-scene] scene push failed: {e:#}"),
                    Ok(_target) => match snapshot_world(&state.world, &state.scene_name, &state.registry) {
                        Err(e) => log::warn!("[wgpu-scene] scene snapshot failed: {e:#}"),
                        Ok(json) => {
                            state.scene_stack.push(SceneSnapshot {
                                json,
                                simulation_hz: state.fixed_step.scene_hz(),
                                scene_path: state.scene_path.clone(),
                                scene_name: state.scene_name.clone(),
                                camera: state.camera,
                                post_process: state.post_process.settings().clone(),
                            });
                            if let Err(e) = reload_scene(state, &p) {
                                log::warn!("[wgpu-scene] scene push (load) failed: {e:#}");
                                // Roll back the snapshot so depth tracks reality.
                                state.scene_stack.pop();
                            }
                        }
                    },
                }
                return Ok(());
            }
            SceneAction::Pop => {
                match state.scene_stack.pop() {
                    None => log::warn!("[wgpu-scene] script requested Pop but scene stack is empty"),
                    Some(snap) => {
                        log::info!(
                            "[wgpu-scene] script requested Pop — restoring '{}' (depth -> {})",
                            snap.scene_name,
                            state.scene_stack.len()
                        );
                        let restored_camera = snap.camera;
                        let restored_post = snap.post_process;
                        let restored_hz = snap.simulation_hz;
                        let restored = serde_json::from_str(&snap.json)
                            .context("parsing the scene snapshot")
                            .and_then(|scene| apply_scene(state, scene, snap.scene_path, snap.scene_name));
                        if let Err(e) = restored {
                            log::warn!("[wgpu-scene] scene pop failed: {e:#}");
                        } else {
                            // Restore the captured camera and effect chain
                            // after apply_scene reset both.
                            state.camera = restored_camera;
                            state.post_process.set_settings(restored_post);
                            state.fixed_step.enter_scene(restored_hz);
                        }
                    }
                }
                return Ok(());
            }
        }
    }

    // Wedge 9 — keep the persistent Camera2D in sync with the
    // window viewport, then let `update_camera` apply the active
    // `Camera2DComponent` (zoom / viewport_w/h) and smooth toward
    // the `CameraFollow` target - after the scripts, so a script's camera
    // move lands this frame, as in the browsers.
    jengine_core::ecs::systems::update_camera(&state.world, &mut state.camera, dt);
    // The camera shakes the scripts queued this frame, then the shake decay,
    // before the view matrix is formed.
    for (amp, dur, freq) in state
        .script_system
        .api
        .services
        .pending_camera_shakes
        .drain(..)
    {
        state.camera.start_shake(amp, dur, freq);
    }
    state.camera.tick_effects(dt);
    // A pixel-perfect camera draws the world at the art's resolution, through
    // a camera on the art's grid; scripts still see `state.camera`.
    let camera = jengine_core::ecs::wgpu_systems::apply_pixel_plan(
        state.camera,
        (vw, vh),
        &state.renderer.device,
        &mut state.post_process,
        &mut state.light_pass,
    );
    // What this frame can see, for culling before the world pass (which sets
    // it again per camera).
    jengine_core::renderer::frame_view::set_frame_view(&camera);
    // Slice 3.5.A — resolve world-space transforms from local Transform +
    // optional Parent chains, BEFORE any consumer reads `WorldTransform`.
    // Free for entities without a Parent (one pass + Transform→WorldTransform
    // copy). Must run AFTER physics/behavior systems mutate `Transform`.
    // Slice 3.5.C — first advance skeletal animation players so the bones
    // they drive show up in the resolver's input on the same frame.
    // Skeletons, IK, the physics blend, world transforms, trails, cloth and
    // carving - the order every host poses its world for drawing in.
    // With fixed steps the bodies were blended with everything else above.
    let blend_bodies = if state.fixed_step.is_on() { None } else { state.physics.as_mut() };
    jengine_core::ecs::systems::pose_world_for_draw(&mut state.world, &mut state.assets, blend_bodies, dt);

    // ── package systems: Animation ────────────────────────────────────────
    state.package_host.run_phase_with(
        SystemPhase::Animation,
        &mut state.world,
        dt,
        jengine_core::package::host::FrameResources { input: &mut state.input, assets: &mut state.assets, camera: state.camera },
    );

    // The one particle system is the script system's, so a script's
    // `Particles.emit_burst` lands in what is drawn. Native used to keep a
    // second one here: bursts went into the script system's, which was
    // ticked - every emitter simulated twice - and never drawn.
    jengine_core::particles::tick_particles(&state.world, &mut state.script_system.api.services.particle_system, dt);

    // ── package systems: PreRender ────────────────────────────────────────
    // Runs after every transform consumer above has settled and immediately
    // before the draw, which is what `parallax_scroll` needs: it reads the
    // active camera's position, so running it earlier would leave the layers
    // a frame behind the camera.
    //
    // The `Render` and `PostRender` phases are deliberately NOT run. A system
    // there would execute but could not draw anything — the renderer, sprite
    // batch and GPU state are owned by `WgpuSceneState` and are not in the
    // `ResourceMap`, and handing them out safely is its own design problem.
    // Running the phases anyway would invite packages to register draw code
    // that silently does nothing.
    state.package_host.run_phase_with(
        SystemPhase::PreRender,
        &mut state.world,
        dt,
        jengine_core::package::host::FrameResources { input: &mut state.input, assets: &mut state.assets, camera: state.camera },
    );
    // Package systems spawn sprites and switch their atlas regions in these
    // phases, after `tick_world` resolved regions for the frame: resolve
    // again, or each new one draws its whole atlas for a frame (BUGS #110).
    // Only unresolved sprites are touched.
    jengine_core::ecs::systems::resolve_sprite_regions(&mut state.world, &state.assets);

    // Counters describe the world about to be drawn. Publishing into the
    // script system feeds both the F3 overlay (which reads the snapshot)
    // and `engine.frame_stats()` from a script - nothing else fills them,
    // so without this the overlay reads zero on every row.
    state.frame_stats.entity_count = state.world.len();
    state.frame_stats.sprite_count = state
        .world
        .query::<&jengine_core::ecs::components::SpriteComponent>()
        .iter()
        .count() as u32;
    state.frame_stats.particle_count = state.script_system.api.services.particle_system.alive_count();
    let published = state.frame_stats.clone();
    state.script_system.update_frame_stats(&published);

    let render_start = Instant::now();
    let mut frame = match state.renderer.begin_frame() {
        Ok(f) => f,
        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
            state.renderer.resize(UVec2::new(vw.max(1), vh.max(1)));
            return Ok(());
        }
        Err(e) => return Err(anyhow::anyhow!("surface error: {e:?}")),
    };

    // Per-frame full reset of the sprite/lit-sprite batches — rewinds the
    // per-flush vertex buffer pool so each batch within this frame writes
    // to its own GPU buffer (otherwise wgpu would coalesce all the
    // `queue.write_buffer` calls and every draw would read the last
    // flush's vertices).
    state.sprite_batch.begin_frame();

    // Surface the monotonic clock to `globals.time` for graph-driven
    // materials (Slice 13b). The vanilla sprite shader ignores the field.
    state.sprite_batch.set_time(state.start_time.elapsed().as_secs_f32());

    // Step 6d — decide the scene render destination. Lit scenes go
    // into `light_pass.scene_target()` so the composite step can
    // sample the scene without a feedback loop; unlit scenes render
    // straight into `post_process.scene_target()` and skip the
    // lighting passes entirely.
    let lighting_active = has_lights(&state.world);
    let [r, g, b, a] = state.clear_color.map(f64::from);
    let scene_clear = wgpu::Color { r, g, b, a };

    // Custom bitmap fonts, lifted out of `state.assets` for the same reason
    // and at the same point as `gui_jobs` below: the draw closures capture
    // `&mut state.assets`, so anything read from it must be resolved first.
    // `FrameFonts` shares each font by `Arc`, so this is a refcount bump per
    // loaded font rather than a copy of its glyph table.
    let frame_fonts = jengine_core::renderer::font_asset::FrameFonts::gather(
        &state.assets,
        &state.font_data,
        BUILTIN_FONT_ID,
    );

    // 7-prep wedge 5 — resolve every visible `GuiComponent`'s document
    // BEFORE we build the render closures. Cloned so the doc refs no
    // longer alias `state.assets`, which the `ui_flush` closure
    // captures `&mut`. Cheap: typical scenes have 0–3 GuiDocuments.
    let gui_jobs: Vec<(jengine_core::ui::gui_document::GuiDocument, jengine_core::ecs::components::GuiComponent)> = state
        .world
        .query::<&jengine_core::ecs::components::GuiComponent>()
        .iter()
        .filter(|(_, c)| c.visible)
        .filter_map(|(_, c)| {
            state
                .assets
                .get_gui(&c.gui_id)
                .map(|d| (d.clone(), c.clone()))
        })
        .collect();

    // On-screen touch controls, over the game's own UI.
    let touch_draws = {
        let (sw, sh) = state.ui_scale.screen_virtual_size(vw as f32, vh as f32);
        state.touch.draw_list(touch_controls_of(&state.world).as_ref(), glam::Vec2::new(sw, sh))
    };
    // The F3 stats / F12 inspector. Honours the script-driven
    // `set_debug_overlay(visible)` flag too - `sync_from_script(true)` only
    // activates when nothing else is showing, so F12 is not clobbered.
    let script_vis = state.script_system.api.services.debug_overlay_visible && state.debug_overlay_enabled;
    state.debug_overlay.sync_from_script(script_vis);
    let overlay_stats = state.script_system.api.snapshot.frame_stats.clone();

    // Views, the world, the post chain and the UI: one function every host
    // shares (`ecs::frame_draw`). This runtime's part is its policy - a
    // material read from the project's files, a texture loaded after this
    // frame (`load_requested_textures`).
    {
        use jengine_core::ecs::frame_draw::{FrameDraw, UiScene, WorldScene, draw_frame};
        // Snapshot the active sort layers before the assets are lent `&mut`.
        let sort_layers = state.assets.active_sort_layers().clone();
        let mut host = NativeDrawHost { project_root: state.project_root.as_deref() };
        let overlay = (state.debug_overlay_enabled && state.debug_overlay.is_visible())
            .then_some((&state.debug_overlay, &overlay_stats));
        draw_frame(
            FrameDraw {
                device: &state.renderer.device,
                queue: &state.renderer.queue,
                scene_format: state.renderer.scene_format,
                color_format: state.renderer.color_format,
                world: &state.world,
                assets: &mut state.assets,
                warned: &mut state.missing_warned,
                graph_pipelines: &mut state.graph_pipelines,
                sprite_batch: &mut state.sprite_batch,
                lit_sprite_batch: &mut state.lit_sprite_batch,
                flat_normal: &state.flat_normal_texture,
                particle_system: &state.script_system.api.services.particle_system,
                frame_fonts: &frame_fonts,
                sort_layers: &sort_layers,
                post: &mut state.post_process,
                light: &mut state.light_pass,
                view_targets: &mut state.view_targets,
                host: &mut host,
            },
            &mut frame,
            WorldScene {
                camera,
                surface: (vw, vh),
                lighting: lighting_active,
                clear: scene_clear,
                debug_draws: &state.script_system.api.services.debug_draws,
                physics_outlines: state.debug_overlay.physics,
                underlay: None,
            },
            UiScene { ui_scale: state.ui_scale, gui_jobs: &gui_jobs, touch_draws: &touch_draws, overlay },
        );
    }

    // The lighting composite and the post chain both run inside the block
    // above: the composite between the world pass and the pass for things
    // that must not be lit twice, the post chain before the UI.

    // Slice 12.A — tick & composite the fade overlay on top of the
    // post-processed swapchain image. The overlay deliberately runs
    // after PP so it isn't bloomed / vignetted, mirroring how a
    // typical engine renders "fade to black" cinematics.
    // A fade or flash runs on **real** time: it is the screen, not the world,
    // so a hitstop (time scale 0) must not freeze it at full brightness and
    // a slow motion must not stretch it (Shooter Classics' Void Armada).
    let fade_done = if let Some(ref mut t) = state.fade_active {
        t.elapsed += real_dt;
        let color = t.current_color();
        state.fade_overlay.render(
            &state.renderer.queue,
            &mut frame.encoder,
            &frame.view,
            color,
        );
        t.is_done()
    } else {
        false
    };
    if fade_done {
        state.fade_active = None;
    }

    frame.present(&state.renderer.queue);
    load_requested_textures(state);

    // Cost of the pass just submitted, plus the draw calls both sprite
    // batches recorded. Read back here and shown one frame later, since the
    // overlay is drawn from inside that pass.
    state.frame_stats.render_time_ms = render_start.elapsed().as_secs_f32() * 1000.0;
    state.frame_stats.draw_calls =
        state.sprite_batch.frame_draw_calls + state.lit_sprite_batch.frame_draw_calls;

    // `JENGINE_PROFILE_CAPTURE=<path>`: every frame's spans, scripts and
    // draws, written as a capture the editor's Profiler panel opens
    // (`profiling::trace::CaptureWriter`).
    PROFILE.with(|p| {
        let mut p = p.borrow_mut();
        if let Some(w) = p.get_or_insert_with(|| jengine_core::profiling::trace::CaptureWriter::from_env("native")) {
            w.frame();
        }
    });

    // Input edges are cleared by `render_frame`, after this returns.
    Ok(())
}

/// One step of the gameplay: input actions, behaviour graphs, motion, the
/// physics, triggers, animation, UI, lifetimes and the scripts with what they
/// asked for - everything a fixed-step simulation (`settings.simulation_hz`)
/// runs once per step rather than once per frame. `render_frame_body` calls
/// it once a frame with the frame's `dt` otherwise.
fn simulation_step(state: &mut WgpuSceneState, dt: f32) -> Result<()> {
    // Input actions read this frame's keys and pads.
    jengine_core::ecs::behaviors::update_input_actions(&mut state.world, &state.input);

    // Wedge 12 — behavior-graph priority 1 tick. Per spec §4.4 the
    // behavior runtime fires BEFORE the script bridge so any
    // commands it queues are flushed at the Phase 7 barrier below
    // and naturally overridden by script commands (priority 2).
    //
    // Re-sync instances every frame first so entities spawned at
    // runtime (script `spawn_prefab`, behavior `SpawnEntity`) that
    // carry a `BehaviorGraphComponent` get bound + `OnSpawn`-fired,
    // and despawned graph entities are detached (no instance leak).
    // Cheap: already-bound entities early-out and graph JSON is
    // path-cached, so the steady-state cost is one world query.
    let bg_root = std::path::Path::new("assets");
    let bg_errors = state.behavior_system.sync_instances(&mut state.world, bg_root);
    for err in &bg_errors {
        log::warn!(
            "[wgpu-scene] behavior graph load failed for entity {:?} ({}): {}",
            err.entity, err.path.display(), err.source
        );
    }
    jengine_core::profiling::trace::time("behavior_graphs", || {
        state.behavior_system.tick_pre_simulation(&mut state.world, dt)
    });

    // The world first, then the scripts - the browsers' order, which all
    // three hosts keep since Oct 2026: a script reads this frame's physics,
    // as Unity's `Update` does after `FixedUpdate`. Native ran its scripts
    // first and so read the frame before (`parity-gaps.md`, "The order of a
    // frame").
    let update_start = Instant::now();
    // Before the motion behaviors, so a batch script writing Force2D / Wind2D
    // / Acceleration2D this frame is integrated in the same frame.
    run_batch_scripts_if_any(state, dt);
    // The motion step every host shares. This runtime runs batch scripts,
    // so their entities' motion is theirs.
    jengine_core::ecs::behaviors::tick_motion(&mut state.world, dt, state.path_library.as_ref(), true);

    // `AudioSource { auto_play: true }` — entities that carry a sound rather
    // than a script that plays one. Each plays on its own `bus_id`; a looping
    // source loops as its own sound instead of replacing the music.
    {
        let requests = jengine_core::ecs::behaviors::collect_autoplay_audio(&mut state.world);
        if !requests.is_empty() {
            jengine_core::ecs::systems::play_autoplay_audio(
                &requests,
                &mut *state.audio,
                &state.assets,
                &mut state.script_system.api.services.mixer,
                &mut state.spatial_audio,
            );
        }
    }
    // Positional audio: each spatial source at the volume and pan of where it
    // is now, and the sounds of sources that have gone, stopped.
    jengine_core::ecs::systems::apply_spatial_audio(
        &state.world,
        &mut *state.audio,
        &mut state.script_system.api.services.mixer,
        &mut state.spatial_audio,
    );

    // ── package systems: PreUpdate ────────────────────────────────────────
    // Each `run_phase` is placed at the point in this frame that matches the
    // phase's documented meaning, so a package system lands where its docs
    // say it will. All are cheap no-ops when no package registered for the
    // phase.
    state.package_host.run_phase_with(
        SystemPhase::PreUpdate,
        &mut state.world,
        dt,
        jengine_core::package::host::FrameResources { input: &mut state.input, assets: &mut state.assets, camera: state.camera },
    );

    // ── package systems: ScriptExec ───────────────────────────────────────
    state.package_host.run_phase_with(
        SystemPhase::ScriptExec,
        &mut state.world,
        dt,
        jengine_core::package::host::FrameResources { input: &mut state.input, assets: &mut state.assets, camera: state.camera },
    );

    // The physics step, after motion (so kinematic bodies follow it, and
    // last frame's script impulses apply through `ensure_physics_bodies`) and
    // before the trigger areas and the graph barrier, so contacts feed the
    // queue the barrier flushes.
    state.frame_stats.physics_time_ms = 0.0;
    if let Some(physics) = state.physics.as_mut() {
        let physics_start = Instant::now();
        jengine_core::ecs::systems::physics_frame_begin(&mut state.world, physics, dt);
        let contacts = if state.script_system.has_fixed_update() {
            // `fixed_update` scripts: one call before each fixed step, each
            // reading the world as the step before it left it.
            let steps = physics.begin_frame(dt);
            let fixed_dt = physics.fixed_dt();
            for _ in 0..steps {
                state.script_system.set_physics_ptr(physics);
                let commands = state.script_system.run_fixed(
                    &mut state.world,
                    fixed_dt,
                    &state.input,
                    Some(&mut state.scene_manager),
                    Some(&mut state.assets),
                );
                state.script_system.clear_physics_ptr();
                jengine_core::ecs::systems::apply_physics_commands(&commands, physics, &state.world);
                jengine_core::ecs::systems::apply_audio_commands(
                    &commands,
                    &mut *state.audio,
                    &state.assets,
                    &mut state.script_system.api,
                );
                // A body spawned by the call steps with the rest, and a force
                // set in `fixed_update` pushes for this step.
                jengine_core::ecs::systems::physics_fixed_step(&mut state.world, physics, fixed_dt);
            }
            physics.end_frame()
        } else {
            physics.step(dt)
        };
        jengine_core::ecs::systems::physics_frame_end(
            &mut state.world,
            physics,
            &contacts,
            &mut state.script_system.api.services.particle_system,
            dt,
        );
        let _disp = state
            .behavior_system
            .dispatch_contact_events(&contacts, &mut state.world);
        // Contacts for the scripts' `on_collision_enter` / `_exit`, which
        // run later this frame.
        state.script_system.queue_contact_events(&contacts);
        // The script system does not own the physics world, so hand it the
        // gravity `get_gravity()` reports.
        state.script_system.set_world_gravity(physics.get_gravity());
        state.frame_stats.physics_time_ms = physics_start.elapsed().as_secs_f32() * 1000.0;
    }

    // ── package systems: Physics ──────────────────────────────────────────
    // After the runtime's own step, so a package system sees this frame's
    // results. The built-in `jengine-physics` package registers here too and
    // does nothing, because the runtime deliberately does not share
    // `PhysicsWorld` as a resource — it drives physics directly above.
    state.package_host.run_phase_with(
        SystemPhase::Physics,
        &mut state.world,
        dt,
        jengine_core::package::host::FrameResources { input: &mut state.input, assets: &mut state.assets, camera: state.camera },
    );

    // Trigger areas after the physics step, so overlaps see where things
    // ended up.
    jengine_core::ecs::behaviors::update_trigger_areas(&mut state.world);

    // The graph barrier: this frame's trigger-enter events into the
    // behaviour runtime, then its structural commands flushed. Graph commands
    // land this frame; the scripts, which run later, see what they did.
    // Graph input triggers (`OnKey`, `OnMouseClick`) - the same core call in
    // every runtime; nothing fired them before (BUGS #93).
    state.behavior_system.dispatch_input(&state.input, &mut state.world);
    let _trig_dispatched = state
        .behavior_system
        .dispatch_trigger_overlaps_from_world(&mut state.world);
    // Scripts -> graphs: what scripts `send_event`ed this frame reaches the
    // graphs' `OnEvent` before the barrier, so a handler's commands flush
    // with everything else's.
    for ev in state.script_system.take_events_for_graphs() {
        state
            .behavior_system
            .deliver_script_event(&ev.name, ev.data.as_ref(), &mut state.world);
    }
    // Then re-dispatch any `EmitEvent` behaviors queued this frame, so
    // cross-graph `OnEvent` handlers (scene 18's collision pings driving a
    // separate counter graph) fire in the same frame as the contact. One
    // shared method so the editor and web do exactly this too.
    let routed = state.behavior_system.flush_commands_and_route_events(
        &mut state.world,
        Some(&jengine_core::behavior::PrefabSource {
            registry: &state.registry,
            prefabs: state.script_system.api.services.prefabs(),
        }),
    );
    // Graphs -> scripts: their events reach `on_<name>` next frame.
    state.script_system.queue_graph_events(&routed.emit_event_requests);
    // Sound and scene changes need the runtime; forward them through the
    // exact paths a script's `play_sound` / `load_scene` take (BUGS #93).
    if !routed.play_sound_requests.is_empty() {
        let sounds: Vec<jengine_core::scripting::api::ScriptCommand> = routed
            .play_sound_requests
            .iter()
            .map(|r| jengine_core::scripting::api::ScriptCommand::PlaySound {
                clip_id: r.sound_id.clone(),
                volume: r.volume,
                pitch: 1.0,
                pan: 0.0,
                sound_id: 0,
                looping: false,
            })
            .collect();
        jengine_core::ecs::systems::apply_audio_commands(
            &sounds,
            &mut *state.audio,
            &state.assets,
            &mut state.script_system.api,
        );
    }
    for path in routed.change_scene_requests {
        state
            .scene_manager
            .queue_action(jengine_core::scene::manager::SceneAction::Load(path));
    }

    // ── package systems: PostPhysics ──────────────────────────────────────
    state.package_host.run_phase_with(
        SystemPhase::PostPhysics,
        &mut state.world,
        dt,
        jengine_core::package::host::FrameResources { input: &mut state.input, assets: &mut state.assets, camera: state.camera },
    );

    // Combat - hitboxes, i-frames, death, the health mirror - after the trigger
    // areas and the graph barrier, then the sprite animations and their state
    // machines.
    let sprite_events = jengine_core::ecs::systems::tick_combat_and_animation(&mut state.world, &state.assets, dt);
    // What a clip's frames fired: a sound plays, an event goes to the script
    // on that entity. Every runtime computed these and dropped them until
    // Sep 2026, so a sound authored on a frame was silent and
    // `on_animation_event` was a handler nothing ever called.
    if !sprite_events.is_empty() {
        let (sounds, _) = jengine_core::animation::split_triggers(&sprite_events);
        for (_, clip_id, volume) in sounds {
            jengine_core::ecs::systems::play_sfx(
                &mut *state.audio,
                &state.assets,
                &mut state.script_system.api.services.mixer,
                &clip_id,
                volume,
            );
        }
        state.script_system.queue_animation_events(&sprite_events);
    }
    // Typed characters land in the focused GUI TextInput. Kept out of
    // `tick_world` because GUI documents live in the AssetManager rather than
    // the world, and threading assets through the whole ECS tick for one
    // widget is not worth it.
    jengine_core::ecs::systems::apply_text_input(
        &mut state.world,
        &mut state.assets,
        &mut state.input,
    );
    // Menus without a pointer: arrows / D-pad / stick move focus, A / Enter
    // press it, Tab walks the document. A press reaches scripts as a click
    // would - a `.gui` node through this frame's `process_gui_clicks`, an
    // entity-UI button as `ui_click`.
    let nav = jengine_core::ecs::systems::navigate_ui(
        &mut state.world,
        &mut state.assets,
        &mut state.input,
        &state.ui_scale,
        dt,
    );
    state.script_system.api.services.pending_gui_activations.extend(nav.gui_activations);
    // The tree `navigate_ui` just gathered, to the screen reader.
    #[cfg(feature = "accessibility")]
    state.accessibility.update();
    for button in nav.ui_clicks {
        state.script_system.queue_engine_event(
            jengine_core::ecs::systems::UI_CLICK_EVENT,
            jengine_core::ecs::systems::ui_click_payload(button),
        );
    }
    let expired = jengine_core::ecs::behaviors::update_lifetimes(&mut state.world, dt);
    for e in expired {
        let _ = state.world.despawn(e);
    }

    // Anything carrying `DespawnOffscreen` that has left the view. Every
    // scrolling game used to walk its own list of spawned things in Lua and
    // compare each one against the camera by hand.
    {
        let view = (state.camera.position, state.camera.half_extents());
        let travel = state.camera.position - state.prev_camera;
        let gone = jengine_core::ecs::behaviors::despawn_offscreen(&mut state.world, view, travel);
        for e in gone {
            let _ = state.world.despawn(e);
        }
        // Videos (`jengine_core::video`): this frame's pictures into their
        // textures, before anything draws them.
        #[cfg(feature = "video")]
        state.video.tick(&mut state.world, &mut state.assets, dt);
        // `visibility_notifier`s that came on screen or left it.
        jengine_core::ecs::visibility::tick_visibility(&mut state.world, view);
        // What the engine owes scripts (`ecs::callbacks`): those, a
        // `path_follow` at its end ... - dispatched by this frame's run.
        state.script_system.queue_entity_callbacks(&jengine_core::ecs::callbacks::take());
        state.prev_camera = state.camera.position;
    }

    // The world tick, less the physics step, which times itself.
    state.frame_stats.update_time_ms =
        (update_start.elapsed().as_secs_f32() * 1000.0 - state.frame_stats.physics_time_ms).max(0.0);

    // The UI before the scripts, as both browser hosts have it: a focused
    // text field takes the keys typed into it (`ui::text_edit`), so a
    // script reading H or Escape this frame must come after it - and the
    // scroll gesture below is published for the click dispatch inside
    // this frame's script run, not the next one's.
    // GUI scrolling: wheel, then pointer drag / flick / elastic ends. Native
    // had neither — `apply_gui_wheel_scroll` was written to be shared by all
    // three runtimes and only the browser two ever called it, so a scrolling
    // list here could be moved by nothing but a script.
    {
        // UI space is virtual pixels (see `ui::scaling`), so the pointer is
        // divided into it before any UI hit test. `input.mouse_position` stays
        // physical for world-space uses.
        let (pw, ph) = state.input.screen_size;
        let screen = state.ui_scale.virtual_size(pw, ph);
        let ui_mouse = state.ui_scale.to_ui(
            state.input.mouse_position.0,
            state.input.mouse_position.1,
            pw,
            ph,
        );
        if state.input.mouse_wheel_delta != 0.0 {
            jengine_core::ecs::systems::apply_gui_wheel_scroll(
                &state.world,
                &mut state.assets,
                ui_mouse,
                state.input.mouse_wheel_delta,
                screen,
            );
        }
        let pointer = jengine_core::ui::scroll::PointerInput {
            x: ui_mouse.0,
            y: ui_mouse.1,
            down: state.input.mouse_left,
            just_pressed: state.input.mouse_left_just_pressed,
            just_released: state.input.mouse_left_just_released,
        };
        jengine_core::ecs::systems::update_gui_scroll_drag(
            &state.world,
            &mut state.assets,
            &mut state.gui_scroll,
            pointer,
            screen,
            dt,
        );
        // Publish the gesture so the click dispatch — which lives deep in
        // `ScriptSystem::update` and sees only an `InputState` — can swallow
        // the release that ended a scroll.
        state.input.gui_scroll_dragging = state.gui_scroll.is_dragging();
        // Entity-UI widgets. Buttons, sliders, toggles and text inputs all
        // answer the pointer here. `update_ui_buttons` had existed since the
        // widgets did and no runtime ever called it, and the other three had
        // no interaction code at all - script-driven or nothing (BUGS #61).
        // Runs beside the GUI pointer work so both UI systems hit-test
        // against the same frame's mouse.
        let mut clicked = Vec::new();
        jengine_core::ecs::systems::update_ui_widgets(
            &mut state.world,
            &mut state.input,
            ui_mouse,
            screen.0,
            screen.1,
            &mut clicked,
        );
        // Each click reaches scripts as `ui_click`. The list was collected
        // and dropped until Sep 2026, so a button could only be polled.
        for button in clicked {
            state.script_system.queue_engine_event(
                jengine_core::ecs::systems::UI_CLICK_EVENT,
                jengine_core::ecs::systems::ui_click_payload(button),
            );
        }
    }
    // Where scripts' "near the camera" is: the camera the player saw last
    // frame, since this frame's is decided after the scripts.
    state.script_system.set_viewport_center(Some(state.camera.position));

    {
        // Screen -> world for `get_mouse_world_position`. Reported here, just
        // before scripts run, so a click handler converts against the camera
        // the player is actually looking through this frame.
        //
        // The view is the main camera's `view_on` the screen, as the
        // projection has it (and as both browser hosts and behaviour graphs
        // report it). `state.camera` cannot answer here: the frame resized
        // it to the surface, and `update_camera` fits it only after the
        // scripts - so a script converted against the screen's own pixels,
        // which is right only for a window exactly the authored size. A
        // 1280 x 720 camera on a 2622 x 1206 phone put every tap at 1.7x its
        // distance from the centre (the showcase hub's rows answered none).
        let (sw, sh) = state.input.screen_size;
        let (vw, vh) = {
            let main = jengine_core::ecs::camera_rig::main_camera(&state.world);
            main.and_then(|e| {
                let cam = state.world.get::<&jengine_core::ecs::components::Camera2DComponent>(e).ok()?;
                (cam.viewport_width > 0.0 && cam.viewport_height > 0.0).then(|| {
                    let view = cam.view_on(glam::Vec2::new(sw, sh));
                    (view.x, view.y)
                })
            })
            .unwrap_or_else(|| state.camera.viewport_size())
        };
        state.script_system.set_camera_view((
            state.camera.position.x,
            state.camera.position.y,
            vw,
            vh,
            state.camera.zoom,
        ));
        state.script_system.set_camera_rotation(state.camera.rotation);
    }
    let script_start = Instant::now();
    // Lend the physics world to the script system for exactly this call, so
    // `raycast` can be answered against the colliders as they are. It is
    // the only script read that cannot come from a snapshot, and the
    // borrow is scoped rather than stored because a scene load replaces
    // `state.physics` and a kept pointer would dangle.
    //
    // Taken out and put back rather than borrowed, because `run` takes
    // `&mut state.world` and `state.physics` lives in the same struct.
    //
    // **No `cfg(feature = "physics")` here**, and that is not an oversight:
    // `physics` is a *jengine-core* feature that this crate turns on
    // unconditionally and has no feature of its own by that name, so a
    // `cfg` on it is permanently false. The first version of this block
    // carried one and was compiled out entirely - `raycast` bound fine and
    // answered nothing, which is the same silent shape as the bug it was
    // written to fix. The editor viewport has a comment about this trap
    // over its gravity publish, which had the same hole.
    let lent = state.physics.take();
    if let Some(physics) = lent.as_ref() {
        state.script_system.set_physics_ptr(physics);
    }
    state.script_system.run(
        &mut state.world,
        dt,
        &state.input,
        Some(&mut state.scene_manager),
        Some(&mut state.assets),
    );
    {
        state.script_system.clear_physics_ptr();
        // A script can load a scene, and the new scene builds its own
        // physics world - so only put the borrowed one back if nothing
        // replaced it while it was out.
        if state.physics.is_none() {
            state.physics = lent;
        }
    }
    state.frame_stats.script_time_ms = script_start.elapsed().as_secs_f32() * 1000.0;

    // 2.5 — apply queued input remaps before the next frame's
    // input snapshot. Bridge commands (`input_set_bindings`,
    // `input_add_binding`, etc.) buffer here because
    // `route_bridge_commands` only has access to `ScriptState`,
    // not the live `InputState` owned by the runtime.
    if !state.script_system.api.services.pending_input_remaps.is_empty() {
        let drained: Vec<_> = state
            .script_system
            .api
            .services
            .pending_input_remaps
            .drain(..)
            .collect();
        for cmd in &drained {
            jengine_core::input::apply_input_remap(&mut state.input, cmd);
        }
    }

    // Phase 8.2e — route post-process script commands to the
    // wgpu post-process stack. `apply_script_commands` skips them
    // because the stack lives in the runtime, not in ECS systems.
    {
        use jengine_core::scripting::api::ScriptCommand;
        for cmd in &state.script_system.api.commands {
            match cmd {
                ScriptCommand::SetPostProcessEnabled { effect_name, enabled } => {
                    state.post_process.set_effect_enabled(effect_name, *enabled);
                }
                ScriptCommand::SetPostProcessParam { effect_name, param_name, value } => {
                    state
                        .post_process
                        .set_effect_param(effect_name, param_name, value.clone());
                }
                _ => {}
            }
        }
    }

    // Slice 8 — audio commands queued by scripts this frame.
    // `apply_audio_commands` lazy-loads clips from the AssetManager on first
    // reference and ignores every non-audio command, so it gets the whole
    // list. The runner used to filter it through its own list of audio
    // variants, which had drifted: `stop_sound_by_clip` and the three
    // `*_all_sounds` were missing and did nothing natively. The list is taken
    // and put back rather than cloned, because `apply_script_commands` in
    // `tick_world` drains the same vec later.
    {
        let commands = std::mem::take(&mut state.script_system.api.commands);
        jengine_core::ecs::systems::apply_audio_commands(
            &commands,
            &mut *state.audio,
            &state.assets,
            &mut state.script_system.api,
        );
        let queued_meanwhile = std::mem::replace(&mut state.script_system.api.commands, commands);
        state.script_system.api.commands.extend(queued_meanwhile);
    }
    // Physics-related script commands: apply_impulse, apply_force,
    // set_angular_velocity, set_gravity, set_body_type, add_joint,
    // remove_joint, set_motor_speed.
    //
    // **None of these did anything on native until this block existed.**
    // `apply_script_commands` lists all eight and deliberately ignores
    // them, on the understanding that `apply_physics_commands` handles
    // them instead - and the only caller of that is the jengine-physics
    // package, whose system does nothing here because this runtime
    // drives physics on its own path and does not share `PhysicsWorld`
    // as a resource. So the handoff had no landing site: every one of
    // them was dropped in silence while working perfectly in the editor,
    // which has its own inline arm. Test-lab scene 17's jump was dead
    // natively for as long as it had existed. BUGS #144.
    //
    // Taken and put back rather than cloned, exactly as the audio block
    // above does. The step has run this frame, so an impulse lands in the
    // next one - as in the browsers.
    if let Some(physics) = state.physics.as_mut() {
        let commands = std::mem::take(&mut state.script_system.api.commands);
        jengine_core::ecs::systems::apply_physics_commands(&commands, physics, &state.world);
        let queued_meanwhile = std::mem::replace(&mut state.script_system.api.commands, commands);
        state.script_system.api.commands.extend(queued_meanwhile);
    }

    Ok(())
}

/// Look up the desired window size from the active camera's viewport
/// hints, falling back to 1280x720.
fn pick_window_size(world: &World) -> (f32, f32) {
    if let Some(main) = jengine_core::ecs::camera_rig::main_camera(world)
        && let Ok(cam) = world.get::<&Camera2DComponent>(main)
    {
        return (cam.viewport_width.max(1.0), cam.viewport_height.max(1.0));
    }
    (1280.0, 720.0)
}

/// Build a small ordered list of candidate roots for resolving a sprite
/// `texture_id`. Order matters: the first existing file wins.
///
/// Engine convention is `<project>/assets/scenes/foo.scene` with texture
/// ids relative to `<project>/assets/`, but some demos put the scene one
/// level shallower (`<project>/assets/default.scene`). We try both layouts plus the workspace cwd.
fn candidate_roots(scene_path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut push = |p: PathBuf| {
        if !out.iter().any(|q: &PathBuf| q == &p) {
            out.push(p);
        }
    };
    if let Some(parent) = scene_path.parent() {
        if let Some(grand) = parent.parent() {
            push(grand.to_path_buf());           // .../assets/
            if let Some(great) = grand.parent() {
                push(great.to_path_buf());       // .../<project>/
            }
        }
        push(parent.to_path_buf());              // .../scenes/ (last resort)
    }
    push(PathBuf::from("."));
    out
}

fn resolve_texture_path(tex_id: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    // The roots, then a downloaded asset pack (`asset::packs`).
    jengine_core::asset::find_in_roots(tex_id, roots)
}

/// The asset pack `id` is waiting for, when that is why it is missing
/// (`jengine_core::asset::packs`); none in a build without packs.
fn pack_hint(id: &str) -> Option<String> {
    #[cfg(feature = "packs")]
    return jengine_core::asset::packs::missing_hint(id);
    #[cfg(not(feature = "packs"))]
    {
        let _ = id;
        None
    }
}

/// "not found in any search root", plus the asset pack it is waiting for
/// when that is why.
fn not_found(what: &str, id: &str) -> String {
    match pack_hint(id) {
        Some(hint) => format!("[wgpu-scene] {what} '{id}' not found: it is {hint}"),
        None => format!("[wgpu-scene] {what} '{id}' not found in any search root"),
    }
}


/// The video system this platform has: Media Foundation on Windows (each
/// video decoded on a thread of its own), none elsewhere yet.
#[cfg(feature = "video")]
fn native_video_system(assets_root: std::path::PathBuf) -> jengine_core::video::VideoSystem {
    #[cfg(windows)]
    {
        jengine_core::video::VideoSystem::new(Box::new(move |source: &str| {
            let Some(path) = jengine_core::asset::find_in_roots(source, std::slice::from_ref(&assets_root)) else {
                log::warn!("video '{source}': no such file under {}", assets_root.display());
                return None;
            };
            Some(Box::new(crate::video_mf::MfDecoder::open(path)) as Box<dyn jengine_core::video::VideoDecoder>)
        }))
    }
    #[cfg(not(windows))]
    {
        let _ = assets_root;
        jengine_core::video::VideoSystem::unsupported()
    }
}

#[cfg(test)]
mod safe_area_tests {
    use super::insets_between;

    #[test]
    fn a_notched_phone_held_sideways() {
        // An iPhone in landscape: 2532 x 1170, the notch's side and the home
        // indicator kept clear (132 left and right, 63 at the bottom).
        let i = insets_between((0, 0, 2532, 1170), (132, 0, 2268, 1107));
        assert_eq!((i.left, i.top, i.right, i.bottom), (132.0, 0.0, 132.0, 63.0));
        // No notch: nothing covered.
        let i = insets_between((0, 0, 800, 600), (0, 0, 800, 600));
        assert_eq!((i.left, i.top, i.right, i.bottom), (0.0, 0.0, 0.0, 0.0));
    }
}

#[cfg(test)]
mod title_tests {
    use super::project_title;
    use serde_json::json;

    #[test]
    fn the_window_is_named_after_the_game() {
        let full = json!({"name": "lab", "settings": {"title": "Test Lab", "app": {"name": "Lab"}}});
        assert_eq!(project_title(&full).as_deref(), Some("Test Lab"));
        let app = json!({"name": "lab", "settings": {"title": " ", "app": {"name": "Lab"}}});
        assert_eq!(project_title(&app).as_deref(), Some("Lab"), "a blank title falls through");
        assert_eq!(project_title(&json!({"name": "lab"})).as_deref(), Some("lab"));
        assert_eq!(project_title(&json!({})), None);
    }
}
