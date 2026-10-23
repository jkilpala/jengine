//! Per-frame context handed to a [`super::adapter::ScriptAdapter`].
//!
//! The context is intentionally narrow:
//! - read-only snapshot data is exposed via accessor methods, not by handing out the whole
//!   engine state;
//! - mutations are queued through [`BridgeContext::push_command`];
//! - capability flags are queryable so scripts can branch on host support.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::capability::Capabilities;
use super::command::BridgeCommand;

/// One gamepad, as a script sees it.
///
/// Raw: axes are not dead-zoned, because a dead zone belongs to an *action*
/// binding rather than to the device. A script reading a stick directly picks
/// its own threshold; one that wants the bound, dead-zoned value asks for the
/// action axis instead.
#[derive(Debug, Clone, Default)]
pub struct PadSnapshot {
    /// Whether the pad is plugged in right now.
    pub connected: bool,
    /// Axis values in `-1.0..=1.0`, by index.
    pub axes: Vec<f32>,
    /// Buttons held right now, by index.
    pub buttons: HashSet<usize>,
}

/// Read snapshot of the world, refreshed by the engine before each adapter call batch.
#[derive(Debug, Default, Clone)]
pub struct BridgeReadSnapshot {
    /// This frame's delta time in seconds, as handed to `update`. Time-scaled,
    /// unlike `frame_stats.frame_time_ms`, which is wall clock.
    pub dt: f32,
    /// Last frame's performance counters, for `get_fps` / `get_frame_time` /
    /// `get_draw_calls` / `get_entity_count` / `log_stats`.
    ///
    /// These were registered as metadata in `core_script_api.rs` and never
    /// installed on the Luau VM, so they worked in the browser runtimes and
    /// were `nil` on native. Reading them from the snapshot is what makes the
    /// native globals possible.
    pub frame_stats: crate::profiling::FrameStats,
    /// Seconds into the music, for `get_music_position`; 0 with none.
    pub music_position: f64,
    /// Seconds since the Unix epoch, for `get_unix_time`. The wall clock,
    /// not the frame clock: it is what a game that has to survive being
    /// closed measures "how long was I away" with.
    pub unix_time: f64,
    /// Entity world positions.
    pub positions: HashMap<u64, (f32, f32)>,
    /// Entity rotations in radians.
    pub rotations: HashMap<u64, f32>,
    /// Entity scales.
    pub scales: HashMap<u64, (f32, f32)>,
    /// Entity names (where present).
    pub names: HashMap<u64, String>,
    /// Reverse lookup: name → entity id (Phase 5.1: `find_by_name`).
    /// If multiple entities share a name only the lowest-id one is recorded.
    pub name_to_id: HashMap<String, u64>,
    /// Tags per entity (Phase 5.1: `has_tag`).
    pub tags: HashMap<u64, HashSet<String>>,
    /// Sprite RGBA tint per entity (Phase 5.2: `get_sprite_color`).
    pub sprite_colors: HashMap<u64, [f32; 4]>,
    /// Named points per entity, resolved from the atlas, for
    /// `get_sprite_point` — muzzle tips, hardpoints, and the like.
    pub sprite_points: HashMap<u64, HashMap<String, (f32, f32)>>,
    /// Sprite anchor per entity (Phase 5.2: `get_sprite_anchor`).
    pub sprite_anchors: HashMap<u64, [f32; 2]>,
    /// Active material id per entity (Phase 5.2: `get_material`).
    pub materials: HashMap<u64, String>,

    // --- Phase 5.3: input snapshot ----------------------------------------
    /// Keys currently held this frame (key names — see `crate::input::ALL_KEYS`).
    pub keys_pressed: HashSet<String>,
    /// Keys that transitioned to pressed this frame.
    pub keys_just_pressed: HashSet<String>,
    /// Action names currently held.
    pub actions_pressed: HashSet<String>,
    /// Action names that transitioned to pressed this frame.
    pub actions_just_pressed: HashSet<String>,
    /// Action names that transitioned to released this frame.
    pub actions_just_released: HashSet<String>,
    /// Analog value per action, signed and dead-zoned, `-1.0..=1.0`.
    ///
    /// A button or key binding reads 1.0 while held, so a keyboard and a stick
    /// can drive the same action without the script knowing which is attached.
    pub action_axes: HashMap<String, f32>,
    /// Gamepads, by index. Disconnected slots are kept so indices stay stable
    /// across a reconnect — check [`PadSnapshot::connected`].
    pub pads: Vec<PadSnapshot>,
    /// Mouse position in window coordinates (matches `screen_size`
    /// units — physical pixels on native, CSS pixels in the editor
    /// viewport / web runtime).
    pub mouse_position: (f32, f32),
    /// Render surface size in window coordinates (same units as
    /// `mouse_position`). Use this to convert mouse to world space:
    /// `wx = mx - sw/2; wy = sh/2 - my` for a camera at the origin.
    pub screen_size: (f32, f32),
    /// Whether the left mouse button transitioned to pressed this frame.
    pub mouse_left_just_pressed: bool,
    /// Whether the left mouse button is held down right now. Drag-style
    /// interactions need the sustained state, not just the two edges.
    pub mouse_left: bool,
    /// Whether the left mouse button transitioned to released this frame.
    pub mouse_left_just_released: bool,
    /// The right mouse button, the same three ways. The engine has always
    /// tracked it (behaviour graphs read it); scripts could not.
    pub mouse_right_just_pressed: bool,
    pub mouse_right: bool,
    pub mouse_right_just_released: bool,
    /// The middle button, the same three ways (Oct 2026).
    pub mouse_middle_just_pressed: bool,
    pub mouse_middle: bool,
    pub mouse_middle_just_released: bool,
    /// What went down this frame, as binding names
    /// (`InputState::just_pressed_bindings`).
    pub just_pressed_bindings: Vec<String>,
    /// The characters typed this frame, as the keyboard layout made them
    /// (`InputState::typed_chars`) - `?` is `?` on any layout.
    pub typed: String,
    /// Hold / tap / double-tap timing, pad names and families, the device
    /// used last - what `Input.is_action_held`, `get_gamepad_type` and
    /// `get_prompt` answer from (`input::interactions::InputView`).
    pub input_view: crate::input::interactions::InputView,
    /// Mouse wheel delta accumulated this frame.
    pub mouse_wheel_delta: f32,
    /// How far the mouse moved this frame (`InputState::mouse_delta`), for
    /// `get_mouse_delta`.
    pub mouse_delta: (f32, f32),
    /// Every finger on the screen (`InputState::touches`), for
    /// `get_touches`.
    pub touches: Vec<crate::input::Touch>,
    /// This frame's gestures (`InputState::gestures`), for `get_gestures`.
    pub gestures: Vec<crate::input::gestures::Gesture>,

    // --- Phase 5.4: physics snapshot --------------------------------------
    /// Linear velocity per entity (sourced from `Velocity2D` component, or
    /// from the physics world when the `physics` feature is active).
    pub velocities: HashMap<u64, (f32, f32)>,
    /// Each `CharacterController2D` as the last physics step left it, for
    /// `get_character`.
    pub characters: HashMap<u64, crate::ecs::components::CharacterController2D>,
    /// Angular velocity per entity (radians per second). Only populated when
    /// physics is enabled.
    pub angular_velocities: HashMap<u64, f32>,
    /// Contacts produced by the previous physics step. Each tuple is
    /// `(entity_a, entity_b, started)` where `started == true` means the
    /// contact began this frame and `false` means it ended.
    /// `(a, b, started, impulse, x, y)` - who touched, whether it began or
    /// ended, how hard the solver had to push, and where. The last three
    /// were not there before Sep 2026: a script could learn *that* two
    /// things touched and never how hard, so "break on a heavy landing but
    /// not on a nudge" had to be guessed from velocities read a frame late.
    pub contacts: Vec<(u64, u64, bool, f32, f32, f32)>,
    /// Active camera as `(centre_x, centre_y, viewport_w, viewport_h, zoom)`,
    /// for `get_mouse_world_position`. Reported by the host each frame; a
    /// zoom of 0 means no camera has been reported and the getter falls back
    /// to treating screen space as world space.
    pub camera_view: (f32, f32, f32, f32, f32),
    /// The active camera's rotation in radians, for the same conversion.
    /// A separate field rather than a sixth tuple slot, so every existing
    /// reader of `camera_view` keeps working.
    pub camera_rotation: f32,
    /// World gravity as configured for the physics world, for `get_gravity`.
    /// `(0.0, 0.0)` when physics is disabled, which is what a script would
    /// observe anyway.
    pub gravity: (f32, f32),

    // --- Phase 5.6: animation snapshot ------------------------------------
    /// Whether each entity's sprite animator is currently playing.
    pub anim_playing: HashMap<u64, bool>,
    /// Currently active sprite-animation clip name per entity.
    pub anim_clip_names: HashMap<u64, String>,
    /// Currently active animation state-machine state name per entity.
    pub anim_state_names: HashMap<u64, String>,

    // The scene's name is not here: `get_current_scene_name` reads
    // `scene::current_name()`, which each host records when it loads a scene.

    // --- Phase 5.9: GUI snapshot ------------------------------------------
    /// Current GUI node property values, keyed by `(gui_id, node_id, property)`.
    /// Read by `gui_get`; absent entries surface as the empty-string sentinel
    /// in both adapters (as `get_current_scene_name` answers "" for no scene).
    pub gui_node_props: HashMap<(String, String, String), String>,

    // --- Phase 5.10: particle snapshot ------------------------------------
    /// Total alive particle count across all emitters
    /// (mirrors `EngineApi::get_particle_count`).
    pub particle_count: u32,

    // --- Phase 5.11: tilemap snapshot -------------------------------------
    /// Per-entity tilemap clones used to answer read-side queries
    /// (`get_tile`, `get_tile_at_world`, `world_to_tile`, `tile_to_world`).
    /// Mirrors `EngineApi::tilemap_snapshots`.
    pub tilemap_snapshots: HashMap<u64, crate::tilemap::Tilemap>,
    /// Per-entity transform `(x, y)` paired with each tilemap snapshot.
    /// Mirrors `EngineApi::tilemap_transforms`.
    pub tilemap_transforms: HashMap<u64, (f32, f32)>,

    // --- Phase 5.13: custom component snapshot ----------------------------
    /// Per-entity custom-component data, mirroring
    /// [`crate::ecs::components::CustomComponents`]. Outer key is entity id,
    /// inner key is component name (e.g. `"Health"`); the inner JSON value
    /// is expected to be an object mapping field names to field values.
    /// Read by `get_custom`; absent entries surface as `nil`/`undefined`.
    ///
    /// Hot-path callers (the engine frame loop) should leave this empty and
    /// instead populate [`world_ptr`] + [`entity_handles`] so reads borrow
    /// directly from the live ECS world. Tests and the parity harness still
    /// populate this map with owned data.
    pub custom_components: HashMap<u64, HashMap<String, serde_json::Value>>,

    /// Optional pointer to the live ECS world. When `Some`, `get_custom_field`
    /// (and other future zero-copy reads) borrow component data through
    /// [`entity_handles`] instead of consulting the owned `custom_components`
    /// map. Mirrors the `world_ptr` field on
    /// [`crate::scripting::api::ScriptState`].
    ///
    /// SAFETY: callers must clear this back to `None` (or rebuild it) before
    /// the engine mutates the world. The pointer is only valid for the
    /// duration of a single bridge-runner invocation.
    pub world_ptr: Option<*const hecs::World>,
    /// The asset manager lent for the frame, under the same contract as
    /// `world_ptr`: set by `ScriptSystem::run` beside it, read only while
    /// scripts run, never while the frame holds `&mut` to the assets. Reads
    /// through it are what `gui_get` answers with, live, instead of a
    /// per-frame copy of every readable property of every GUI node.
    pub assets_ptr: Option<*const crate::asset::AssetManager>,

    /// Optional pointer to the live physics world, for the one script
    /// function that cannot be answered from a snapshot: `raycast`.
    ///
    /// A ray has to be cast against the colliders *as they are*, and there
    /// is no useful way to precompute one - the origin and direction come
    /// from the script. Copying the collider set into the snapshot each
    /// frame is not cheap, and reimplementing ray-vs-shape would diverge
    /// from rapier, so this borrows the real world for the duration of the
    /// bridge tick exactly as [`world_ptr`](Self::world_ptr) does.
    ///
    /// This is why `raycast` was the last script function missing on
    /// native. `ScriptState` has carried a `physics_world_ptr` and a
    /// working `raycast` for a long time, but **nothing ever called the
    /// method that sets it** - that whole path is dead, and the live
    /// native path is this struct. The pointer needed to be here.
    ///
    /// SAFETY: as `world_ptr` - valid only for one bridge-runner
    /// invocation, and the caller clears or rebuilds it before the engine
    /// steps physics again.
    #[cfg(feature = "physics")]
    pub physics_ptr: Option<*const crate::physics::PhysicsWorld>,

    /// Entity-id → `hecs::Entity` handle map used together with `world_ptr`.
    /// Populated for every scripted entity whose component data the bridge
    /// might need to read live during this frame.
    pub entity_handles: HashMap<u64, hecs::Entity>,

    // --- 2.3: save snapshot ----------------------------------------------
    /// Cloned snapshot of every loaded save slot. Populated once per frame
    /// by `ScriptSystem` from its [`crate::save::SaveSystem`]. Read by
    /// `save_get`; writes flow back via `BridgeCommand::SaveSet` and friends.
    pub save_data: HashMap<String, HashMap<String, serde_json::Value>>,

    // --- 2.4: audio mixer snapshot ----------------------------------------
    /// Snapshot of the current audio mixer bus tree. Populated each frame
    /// from [`crate::audio::AudioMixer`] so script reads
    /// (`audio_get_bus_volume`, `audio_is_bus_muted`) don't re-borrow the
    /// live mixer. Writes flow through `BridgeCommand::Audio*` variants.
    pub audio_mixer: HashMap<String, crate::audio::AudioBus>,

    // --- 2.5: input remapping snapshot ------------------------------------
    /// Current key bindings per action. Populated from
    /// [`crate::input::InputState`] each frame so script reads
    /// (`input_get_bindings`, `input_get_actions`) don't re-borrow the
    /// live input state. Writes flow through `BridgeCommand::Input*`.
    pub action_bindings: HashMap<String, Vec<String>>,
    /// Per-action dead-zone (reserved for gamepad axes; `0.0` for
    /// keyboard-only bindings).
    pub action_dead_zones: HashMap<String, f32>,
}

impl BridgeReadSnapshot {
    /// The world lent for this frame, if a frame is running.
    ///
    /// **The one place the pointer is dereferenced.** `ScriptSystem::run`
    /// sets `world_ptr` before the runners tick and clears it before
    /// anything mutates the world; a snapshot built by hand for a test has
    /// none and every accessor falls back to its eager maps. Fifty
    /// accessors each carried their own `unsafe` deref and their own copy of
    /// this sentence; auditing the contract means auditing this function.
    pub fn world(&self) -> Option<&hecs::World> {
        // SAFETY: see above - set and cleared around one bridge tick by the
        // frame loop, which holds the only `&mut World` and does not use it
        // while the pointer is set.
        self.world_ptr.map(|wp| unsafe { &*wp })
    }

    /// The assets lent for this frame, if the host lent them. Same contract
    /// as [`Self::world`].
    pub fn assets(&self) -> Option<&crate::asset::AssetManager> {
        // SAFETY: as `world`: set and cleared around one bridge tick by the
        // frame loop, which does not touch the assets while it is set.
        self.assets_ptr.map(|a| unsafe { &*a })
    }

    /// `gui_get`: one readable property of one node of one loaded GUI
    /// document, read from the document itself. Falls back to the eager
    /// `gui_node_props` map for a snapshot built by hand.
    pub fn gui_prop(&self, gui_id: &str, node_id: &str, property: &str) -> Option<String> {
        if let Some(assets) = self.assets() {
            let doc = assets.get_gui(gui_id)?;
            let node = doc.root.find_node(node_id)?;
            return super::snapshot::gui_node_prop(node, property);
        }
        self.gui_node_props
            .get(&(gui_id.to_string(), node_id.to_string(), property.to_string()))
            .cloned()
    }

    /// `find_entity_by_name`: the first entity called `name`, read from the
    /// world. This used to be answered from `name_to_id`, a map rebuilt every
    /// frame with a `String` clone per named entity for a lookup most frames
    /// never make; the map stays only for a snapshot built by hand.
    pub fn entity_by_name(&self, name: &str) -> Option<u64> {
        if let Some(world) = self.world() {
            return world
                .query::<&crate::ecs::components::Name>()
                .iter()
                .find(|(_, n)| n.0 == name)
                .map(|(e, _)| e.id() as u64);
        }
        self.name_to_id.get(name).copied()
    }

    /// `Entity.find_child`: the part of `parent_id` called `path`
    /// ([`crate::ecs::hierarchy::find_child`]), as the id a script knows it
    /// by. Needs the lent world; a snapshot built by hand has no hierarchy.
    pub fn child_by_name(&self, parent_id: u64, path: &str) -> Option<u64> {
        let world = self.world()?;
        let parent = self
            .entity_handles
            .get(&parent_id)
            .copied()
            .or_else(|| crate::scripting::spawn_aliases::resolve(parent_id))?;
        let child = crate::ecs::hierarchy::find_child(world, parent, path)?;
        Some(crate::scripting::spawn_aliases::script_id(child.id() as u64))
    }

    /// The lent world and the live entity behind `entity_id`, together.
    fn live_entity(&self, entity_id: u64) -> Option<(&hecs::World, hecs::Entity)> {
        Some((self.world()?, *self.entity_handles.get(&entity_id)?))
    }

    /// The physics world lent for this frame, if the host lent one. Same
    /// contract as [`Self::world`].
    #[cfg(feature = "physics")]
    pub fn physics(&self) -> Option<&crate::physics::PhysicsWorld> {
        // SAFETY: as `world`: set for one bridge tick by the host that owns
        // the physics world and cleared before it is stepped again.
        self.physics_ptr.map(|p| unsafe { &*p })
    }

    /// 2.3 — synchronous read for `save_get(slot, key)`. Returns `None` if
    /// the slot or key is unknown.
    pub fn get_save_value(&self, slot: &str, key: &str) -> Option<&serde_json::Value> {
        self.save_data.get(slot).and_then(|m| m.get(key))
    }

    /// 2.4 — synchronous read for `audio_get_bus_volume(name)`. Returns
    /// `None` for unknown buses.
    pub fn get_bus_volume(&self, name: &str) -> Option<f32> {
        self.audio_mixer.get(name).map(|b| b.volume)
    }

    /// 2.4 — synchronous read for `audio_is_bus_muted(name)`.
    pub fn is_bus_muted(&self, name: &str) -> bool {
        self.audio_mixer.get(name).is_some_and(|b| b.muted)
    }

    /// 2.5 — synchronous read for `input_get_bindings(action)`. Returns
    /// `None` for unknown actions, an empty slice for actions whose
    /// bindings were cleared.
    pub fn get_action_bindings(&self, action: &str) -> Option<&Vec<String>> {
        self.action_bindings.get(action)
    }

    /// 2.5 — synchronous read for `input_get_dead_zone(action)`. Defaults
    /// to `0.0` for unknown actions.
    pub fn get_action_dead_zone(&self, action: &str) -> f32 {
        self.action_dead_zones.get(action).copied().unwrap_or(0.0)
    }

    /// 2.5 — sorted list of every known action name (for
    /// `input_get_actions()`).
    pub fn list_action_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.action_bindings.keys().cloned().collect();
        v.sort();
        v
    }

    /// Hot-path accessors for transform / sprite / velocity / name / tag /
    /// material reads. Each accessor first checks `world_ptr` + the matching
    /// entry in `entity_handles` and reads the component live from the ECS
    /// world; that path is taken by the engine frame loop so per-frame
    /// `snapshot_world_borrow` doesn't have to clone or insert anything for
    /// these fields. When `world_ptr` is `None` (tests, parity harness) the
    /// accessors fall back to the owned HashMap fields.
    //
    /// Read the position of `entity_id`. Returns `None` when the entity
    /// has no `Transform` (or, in test mode, no `positions` entry).
    pub fn get_position(&self, entity_id: u64) -> Option<(f32, f32)> {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            return world
                .get::<&crate::ecs::components::Transform>(entity)
                .ok()
                .map(|t| (t.position.x, t.position.y));
        }
        self.positions.get(&entity_id).copied()
    }

    /// Read the rotation (radians) of `entity_id`.
    pub fn get_rotation(&self, entity_id: u64) -> Option<f32> {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            return world
                .get::<&crate::ecs::components::Transform>(entity)
                .ok()
                .map(|t| t.rotation);
        }
        self.rotations.get(&entity_id).copied()
    }

    /// Read the scale of `entity_id`.
    pub fn get_scale(&self, entity_id: u64) -> Option<(f32, f32)> {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            return world
                .get::<&crate::ecs::components::Transform>(entity)
                .ok()
                .map(|t| (t.scale.x, t.scale.y));
        }
        self.scales.get(&entity_id).copied()
    }

    /// Slice 3.5.A — read the resolved world-space position of `entity_id`.
    /// Prefers the cached `WorldTransform` (populated by
    /// `resolve_world_transforms` before script tick); falls back to the
    /// local `Transform` for entities without a parent or pre-resolver runs.
    pub fn get_world_position(&self, entity_id: u64) -> Option<(f32, f32)> {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            if let Ok(w) = world.get::<&crate::ecs::components::WorldTransform>(entity) {
                return Some((w.position.x, w.position.y));
            }
            return world
                .get::<&crate::ecs::components::Transform>(entity)
                .ok()
                .map(|t| (t.position.x, t.position.y));
        }
        self.positions.get(&entity_id).copied()
    }

    /// Slice 3.5.A — read the resolved world-space rotation (radians).
    pub fn get_world_rotation(&self, entity_id: u64) -> Option<f32> {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            if let Ok(w) = world.get::<&crate::ecs::components::WorldTransform>(entity) {
                return Some(w.rotation);
            }
            return world
                .get::<&crate::ecs::components::Transform>(entity)
                .ok()
                .map(|t| t.rotation);
        }
        self.rotations.get(&entity_id).copied()
    }

    /// Slice 3.5.A — read the resolved world-space scale.
    pub fn get_world_scale(&self, entity_id: u64) -> Option<(f32, f32)> {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            if let Ok(w) = world.get::<&crate::ecs::components::WorldTransform>(entity) {
                return Some((w.scale.x, w.scale.y));
            }
            return world
                .get::<&crate::ecs::components::Transform>(entity)
                .ok()
                .map(|t| (t.scale.x, t.scale.y));
        }
        self.scales.get(&entity_id).copied()
    }

    /// Slice 3.5.B — locate a bone by name within the skeleton rooted at
    /// `root_id`. Returns the bone entity's id (as a `u64`), or `None` if
    /// no bone with that name is a descendant of `root_id`. Requires
    /// `world_ptr` to be set (i.e. called from inside a bridge tick).
    pub fn find_bone(&self, root_id: u64, bone_name: &str) -> Option<u64> {
        let world = self.world()?;
        let &root = self.entity_handles.get(&root_id)?;
        crate::ecs::skeleton::find_bone(world, root, bone_name).map(|e| e.id() as u64)
    }

    /// Slice 3.5.B — locate the first skeleton root entity whose
    /// `SkeletonRoot::name` matches `skeleton_name`.
    pub fn find_skeleton(&self, skeleton_name: &str) -> Option<u64> {
        let world = self.world()?;
        crate::ecs::skeleton::find_skeleton(world, skeleton_name).map(|e| e.id() as u64)
    }

    /// Slice 3.5.C — read the current playback time (seconds) of a
    /// `SkeletalPlayer`. Returns `None` if the entity lacks the
    /// component or `world_ptr` is unavailable.
    pub fn get_animation_time(&self, entity_id: u64) -> Option<f32> {
        let world = self.world()?;
        let &entity = self.entity_handles.get(&entity_id)?;
        world
            .get::<&crate::animation::skeletal::SkeletalPlayer>(entity)
            .ok()
            .map(|p| p.time)
    }

    /// True when the entity's sprite animator or skeleton is advancing
    /// (`animation::is_playing`).
    pub fn is_animation_playing(&self, entity_id: u64) -> bool {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            return crate::animation::is_playing(world, entity);
        }
        self.anim_playing.get(&entity_id).copied().unwrap_or(false)
    }

    /// The clip the entity plays, or "" (`animation::current_clip_name`).
    pub fn get_current_animation(&self, entity_id: u64) -> String {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            return crate::animation::current_clip_name(world, entity).unwrap_or_default();
        }
        self.anim_clip_names.get(&entity_id).cloned().unwrap_or_default()
    }

    /// The entity's state-machine state, or "" (`animation::state_name`).
    pub fn get_anim_state(&self, entity_id: u64) -> String {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            return crate::animation::state_name(world, entity).unwrap_or_default();
        }
        self.anim_state_names.get(&entity_id).cloned().unwrap_or_default()
    }

    /// Read the sprite RGBA tint of `entity_id`.
    pub fn get_sprite_color(&self, entity_id: u64) -> Option<[f32; 4]> {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            // A sprite's tint, else a mesh's: `set_color` writes either.
            return world
                .get::<&crate::ecs::components::SpriteComponent>(entity)
                .ok()
                .map(|s| s.color)
                .or_else(|| world.get::<&crate::ecs::components::Mesh2D>(entity).ok().map(|m| m.color));
        }
        self.sprite_colors.get(&entity_id).copied()
    }

    /// Read the sprite anchor of `entity_id`.
    pub fn get_sprite_anchor(&self, entity_id: u64) -> Option<[f32; 2]> {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            return world
                .get::<&crate::ecs::components::SpriteComponent>(entity)
                .ok()
                .map(|s| s.anchor);
        }
        self.sprite_anchors.get(&entity_id).copied()
    }

    /// Read the active material id of `entity_id`.
    pub fn get_material(&self, entity_id: u64) -> Option<String> {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            return world
                .get::<&crate::ecs::components::MaterialComponent>(entity)
                .ok()
                .map(|m| m.material_id.to_string());
        }
        self.materials.get(&entity_id).cloned()
    }

    /// Read the linear velocity of `entity_id`. Prefers the `velocities`
    /// HashMap (populated by `snapshot_physics` when the physics feature
    /// is active so it sees rapier's authoritative values), falling back
    /// to the live `Velocity2D` component via `world_ptr`.
    pub fn get_velocity(&self, entity_id: u64) -> Option<(f32, f32)> {
        // Physics answers by entity id; a spawn handle resolves to it first,
        // or a spawned body reads as standing still.
        if let Some(v) = self.velocities.get(&self.real_id(entity_id)).copied() {
            return Some(v);
        }
        // The frame's run lends the physics world rather than copying every
        // body's velocity into `velocities` - which it stopped doing, so a
        // falling body read its `Velocity2D` seed (0) natively. Ask it.
        #[cfg(feature = "physics")]
        if let Some(v) = self.physics().and_then(|p| p.get_velocity(self.real_id(entity_id))) {
            return Some(v);
        }
        if let Some((world, entity)) = self.live_entity(entity_id) {
            return world
                .get::<&crate::ecs::components::Velocity2D>(entity)
                .ok()
                .map(|v| (v.x, v.y));
        }
        None
    }

    /// Read the angular velocity of `entity_id` (a spawn handle resolves).
    pub fn get_angular_velocity(&self, entity_id: u64) -> f32 {
        let id = self.real_id(entity_id);
        if let Some(w) = self.angular_velocities.get(&id).copied() {
            return w;
        }
        #[cfg(feature = "physics")]
        if let Some(w) = self.physics().and_then(|p| p.get_angular_velocity(id)) {
            return w;
        }
        0.0
    }

    /// The entity id behind a script's id: a spawn handle resolves to the
    /// entity it became, anything else is already one.
    pub fn real_id(&self, id: u64) -> u64 {
        if id >= crate::scripting::spawn_aliases::FIRST_SPAWN_ID
            && let Some(e) = self.entity_handles.get(&id)
        {
            return e.id() as u64;
        }
        id
    }

    /// Read the name of `entity_id`.
    pub fn get_name(&self, entity_id: u64) -> Option<String> {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            return world
                .get::<&crate::ecs::components::Name>(entity)
                .ok()
                .map(|n| n.0.clone());
        }
        self.names.get(&entity_id).cloned()
    }

    /// Check whether `entity_id` has tag `tag`.
    pub fn has_tag(&self, entity_id: u64, tag: &str) -> bool {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            return world
                .get::<&crate::ecs::components::Tags>(entity)
                .map(|t| t.0.contains(tag))
                .unwrap_or(false);
        }
        self.tags
            .get(&entity_id)
            .is_some_and(|t| t.contains(tag))
    }

    /// Look up a single custom-component field. Returns `None`
    /// if the entity has no `CustomComponents`, no component with that name,
    /// or the component object has no field with that name. Mirrors
    /// [`crate::scripting::api::ScriptState::get_custom_field`].
    /// Read one entity-UI widget property, as JSON.
    ///
    /// Live through `world_ptr` rather than snapshotted: widget state changes
    /// within the frame (a button's `pressed` is written by the UI system) and
    /// only a handful of entities carry widgets.
    pub fn ui_prop_of(&self, entity_id: u64, property: &str) -> Option<serde_json::Value> {
        let (world, entity) = self.live_entity(entity_id)?;
        crate::ui::read_ui_prop(world, entity, property)
    }

    /// Read an entity's `Health` as `(current, max)`.
    ///
    /// Reads live through `world_ptr` rather than copying every Health into
    /// the per-frame snapshot, the same trick `get_custom_field` uses: HP is
    /// wanted by a handful of entities, not all of them.
    pub fn health_of(&self, entity_id: u64) -> Option<(f32, f32)> {
        let (world, entity) = self.live_entity(entity_id)?;
        let h = world.get::<&crate::ecs::components::Health>(entity).ok()?;
        Some((h.current, h.max))
    }

    /// Cast a ray against the live physics world.
    ///
    /// `None` when there is no hit, and also when no physics world is
    /// attached - a script cannot tell those apart, which is the same
    /// contract the browser hosts have always advertised.
    ///
    /// `dir` does not have to be normalised; `PhysicsWorld::raycast`
    /// normalises it so `max_dist` and the reported distance are in world
    /// units either way.
    #[cfg(feature = "physics")]
    pub fn raycast(
        &self,
        ox: f32,
        oy: f32,
        dx: f32,
        dy: f32,
        max_dist: f32,
    ) -> Option<crate::physics::RayHit> {
        let physics = self.physics()?;
        // Named as the script knows it: a spawned body by its handle.
        physics.raycast(ox, oy, dx, dy, max_dist).map(|mut hit| {
            hit.entity_id = crate::scripting::spawn_aliases::script_id(hit.entity_id);
            hit
        })
    }

    /// Without the physics feature there is nothing to cast against, and a
    /// script gets the same `nil` it would get from a miss.
    #[cfg(not(feature = "physics"))]
    pub fn raycast(&self, _ox: f32, _oy: f32, _dx: f32, _dy: f32, _max: f32) -> Option<()> {
        None
    }

    /// Every entity a collider of which overlaps `q`, against the live
    /// physics world (`physics::query`), named as the script knows them.
    /// Empty when there is no physics world.
    #[cfg(feature = "physics")]
    pub fn overlap(&self, q: &crate::physics::query::QueryShape) -> Vec<u64> {
        let Some(physics) = self.physics() else { return Vec::new() };
        let mut ids: Vec<u64> =
            physics.overlap(q).into_iter().map(crate::scripting::spawn_aliases::script_id).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// A circle swept along a line against the live physics world.
    #[cfg(feature = "physics")]
    pub fn circle_cast(&self, ox: f32, oy: f32, dx: f32, dy: f32, radius: f32, max_dist: f32) -> Option<crate::physics::RayHit> {
        let physics = self.physics()?;
        physics.circle_cast(ox, oy, dx, dy, radius, max_dist).map(|mut hit| {
            hit.entity_id = crate::scripting::spawn_aliases::script_id(hit.entity_id);
            hit
        })
    }

    pub fn get_custom_field(
        &self,
        entity_id: u64,
        component_name: &str,
        field_name: &str,
    ) -> Option<serde_json::Value> {
        // Hot path: read live from the world via world_ptr + entity_handles.
        // Avoids cloning every entity's full JSON map into the snapshot each
        // frame (the dominant cost at 1000+ scripted entities). The returned
        // value is cloned because the `hecs::Ref` guard cannot outlive this
        // call; for typical game data (numbers, bools) the clone is cheap.
        if let Some((world, entity)) = self.live_entity(entity_id) {
            if let Ok(custom) =
                world.get::<&crate::ecs::components::CustomComponents>(entity)
            {
                return custom
                    .0
                    .get(component_name)
                    .and_then(|c| c.as_object())
                    .and_then(|o| o.get(field_name))
                    .cloned();
            }
            return None;
        }
        // Fallback: owned snapshot map (used by tests and parity harness).
        let comps = self.custom_components.get(&entity_id)?;
        let comp = comps.get(component_name)?;
        comp.as_object()?.get(field_name).cloned()
    }

    /// [`Self::get_custom_field_f64`] for a field that is a number, and
    /// `None` for anything else - a boolean included. The Luau binding's
    /// fast path: it used `get_custom_field_f64`, whose boolean coercion
    /// turned `true` / `false` into `1` / `0` before the typed path could
    /// return a Lua boolean, so on native `get_custom(...) == true` was
    /// never true and a `false` field tested truthy (0 is true in Lua)
    /// (BUGS #108).
    pub fn get_custom_field_number(&self, entity_id: u64, component_name: &str, field_name: &str) -> Option<f64> {
        let number = |v: &serde_json::Value| match v {
            serde_json::Value::Number(n) => n.as_f64(),
            _ => None,
        };
        if let Some((world, entity)) = self.live_entity(entity_id) {
            let custom = world.get::<&crate::ecs::components::CustomComponents>(entity).ok()?;
            return number(custom.0.get(component_name)?.as_object()?.get(field_name)?);
        }
        number(self.custom_components.get(&entity_id)?.get(component_name)?.as_object()?.get(field_name)?)
    }

    /// Hot-path fast read for numeric custom-component fields. Returns the
    /// field as `f64` without allocating or cloning any `serde_json::Value`.
    /// Booleans coerce to 0.0/1.0 to match the script-side `get_custom`
    /// contract (Lua/JS adapters already collapse non-numeric, non-boolean
    /// fields to 0.0). At 30k entities × ~2 reads/frame this skips ~60k
    /// `Value::clone` calls per frame compared to [`Self::get_custom_field`].
    pub fn get_custom_field_f64(
        &self,
        entity_id: u64,
        component_name: &str,
        field_name: &str,
    ) -> Option<f64> {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            if let Ok(custom) =
                world.get::<&crate::ecs::components::CustomComponents>(entity)
            {
                let v = custom
                    .0
                    .get(component_name)
                    .and_then(|c| c.as_object())
                    .and_then(|o| o.get(field_name))?;
                return match v {
                    serde_json::Value::Number(n) => n.as_f64(),
                    serde_json::Value::Bool(true) => Some(1.0),
                    serde_json::Value::Bool(false) => Some(0.0),
                    _ => None,
                };
            }
            return None;
        }
        // Fallback: owned snapshot map.
        let v = self
            .custom_components
            .get(&entity_id)?
            .get(component_name)?
            .as_object()?
            .get(field_name)?;
        match v {
            serde_json::Value::Number(n) => n.as_f64(),
            serde_json::Value::Bool(true) => Some(1.0),
            serde_json::Value::Bool(false) => Some(0.0),
            _ => None,
        }
    }
    /// The tilemap of `entity_id` and its entity position, handed to `f`.
    ///
    /// Live through `world_ptr` when a frame lent it - the way every other
    /// accessor here reads - and from the eager `tilemap_snapshots` maps
    /// otherwise, which is what a test that builds a snapshot by hand fills.
    /// Reading live is the whole point: the eager path deep-copied every
    /// tilemap in the world into the snapshot each frame, so a builder game's
    /// million tiles were memcpy'd for a lookup that touches one.
    fn with_tilemap<R>(
        &self,
        entity_id: u64,
        f: impl FnOnce(&crate::tilemap::Tilemap, Option<(f32, f32)>) -> R,
    ) -> Option<R> {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            let tilemap = world.get::<&crate::tilemap::Tilemap>(entity).ok()?;
            // Its world place: a parented map is where its parent carries it.
            let pos = world
                .get::<&crate::ecs::components::Transform>(entity)
                .ok()
                .map(|t| crate::tilemap::world_origin(world, entity, &t))
                .map(|p| (p.x, p.y));
            return Some(f(&tilemap, pos));
        }
        let tilemap = self.tilemap_snapshots.get(&entity_id)?;
        Some(f(tilemap, self.tilemap_transforms.get(&entity_id).copied()))
    }

    /// Read the tile id at grid `(x, y)` on `layer_name` for `entity_id`.
    /// Returns `-1` for missing tilemap/layer or out-of-bounds, `0` for an
    /// empty in-bounds tile, or the positive tile GID. Mirrors
    /// `EngineApi::get_tile`.
    pub fn tilemap_get_tile(&self, entity_id: u64, layer_name: &str, x: i32, y: i32) -> i32 {
        self.with_tilemap(entity_id, |tilemap, _| {
            let Some(layer) = tilemap.find_layer(layer_name) else { return -1; };
            if x < 0 || y < 0 || (x as u32) >= layer.width || (y as u32) >= layer.height {
                return -1;
            }
            layer.get_tile(x as u32, y as u32).map(|v| v as i32).unwrap_or(0)
        })
        .unwrap_or(-1)
    }

    /// `Tilemap.get_tile_data(e, layer, x, y, field)`: the custom data of
    /// the cell's tile - one field, or the whole object when `field` is
    /// `None`. `None` for an empty cell, a tile with none, or no such field.
    pub fn tilemap_tile_data(
        &self,
        entity_id: u64,
        layer_name: &str,
        x: i32,
        y: i32,
        field: Option<&str>,
    ) -> Option<serde_json::Value> {
        self.with_tilemap(entity_id, |tilemap, _| {
            let layer = tilemap.find_layer(layer_name)?;
            if x < 0 || y < 0 || (x as u32) >= layer.width || (y as u32) >= layer.height {
                return None;
            }
            let data = tilemap.tile_data(layer.get_tile(x as u32, y as u32)?)?;
            match field {
                Some(f) => data.get(f).cloned(),
                None => Some(data.clone()),
            }
        })
        .flatten()
    }

    /// Read the tile id at world position `(wx, wy)` on `layer_name`.
    /// Same sentinel convention as `tilemap_get_tile`.
    pub fn tilemap_get_tile_at_world(
        &self,
        entity_id: u64,
        layer_name: &str,
        wx: f32,
        wy: f32,
    ) -> i32 {
        self.with_tilemap(entity_id, |tilemap, pos| {
            let Some((ex, ey)) = pos else { return -1; };
            let (tx, ty) = tilemap.world_to_tile(ex, ey, wx, wy);
            if tx < 0 || ty < 0 {
                return -1;
            }
            let Some(layer) = tilemap.find_layer(layer_name) else { return -1; };
            if (tx as u32) >= layer.width || (ty as u32) >= layer.height {
                return -1;
            }
            layer.get_tile(tx as u32, ty as u32).map(|v| v as i32).unwrap_or(0)
        })
        .unwrap_or(-1)
    }

    /// Convert a world position to tile-grid coordinates. `None` when the
    /// entity has no tilemap or transform.
    pub fn tilemap_world_to_tile(
        &self,
        entity_id: u64,
        wx: f32,
        wy: f32,
    ) -> Option<(i32, i32)> {
        self.with_tilemap(entity_id, |tilemap, pos| {
            pos.map(|(ex, ey)| tilemap.world_to_tile(ex, ey, wx, wy))
        })
        .flatten()
    }

    /// Convert tile-grid coordinates to a world position. `None` when the
    /// entity has no tilemap or transform.
    pub fn tilemap_tile_to_world(
        &self,
        entity_id: u64,
        tx: i32,
        ty: i32,
    ) -> Option<(f32, f32)> {
        self.with_tilemap(entity_id, |tilemap, pos| {
            pos.map(|(ex, ey)| tilemap.tile_to_world(ex, ey, tx, ty))
        })
        .flatten()
    }

    /// The cells next to `(tx, ty)` that are on the map: four on a square or
    /// isometric map, six on a hexagonal one. Empty for an unknown tilemap.
    pub fn tilemap_neighbors(&self, entity_id: u64, tx: i32, ty: i32) -> Vec<(i32, i32)> {
        self.with_tilemap(entity_id, |tilemap, _| tilemap.neighbours_on_map(tx, ty)).unwrap_or_default()
    }

    /// Steps between two cells, ignoring what is on them (`Tilemap::cell_distance`).
    /// `-1` for an unknown tilemap.
    pub fn tilemap_distance(&self, entity_id: u64, a: (i32, i32), b: (i32, i32)) -> i32 {
        self.with_tilemap(entity_id, |tilemap, _| tilemap.cell_distance(a, b) as i32).unwrap_or(-1)
    }

    /// `Nav.find_path`: the shortest walkable way between two world points
    /// through the `nav_region` the first is in, both ends included.
    pub fn nav_find_path(&self, from: (f32, f32), to: (f32, f32)) -> Option<Vec<(f32, f32)>> {
        let graphs = crate::navigation::graphs(self.world()?);
        let path = crate::navigation::find_path_in(&graphs, from.into(), to.into())?;
        Some(path.into_iter().map(|p| (p.x, p.y)).collect())
    }

    /// `Nav.is_moving`: whether the agent still has somewhere to walk to.
    pub fn nav_is_moving(&self, entity_id: u64) -> bool {
        self.live_entity(entity_id)
            .and_then(|(world, e)| world.get::<&crate::navigation::NavAgent>(e).ok().map(|a| a.is_moving()))
            .unwrap_or(false)
    }

    /// `Nav.closest_point`: the walkable point nearest `p`.
    pub fn nav_closest_point(&self, p: (f32, f32)) -> Option<(f32, f32)> {
        let graphs = crate::navigation::graphs(self.world()?);
        crate::navigation::closest_point_in(&graphs, p.into()).map(|q| (q.x, q.y))
    }

    /// 2.6 — A* path-find on `layer_name` between two tile coordinates.
    /// `mode_str` accepts `"manhattan"` (4-dir, default) or `"diagonal"`
    /// (8-dir, octile heuristic, no corner cutting). Returns the tile
    /// path including both endpoints, or `None` if there is no route /
    /// the tilemap or layer is unknown / either endpoint is blocked.
    pub fn tilemap_find_path(
        &self,
        entity_id: u64,
        layer_name: &str,
        start: (i32, i32),
        goal: (i32, i32),
        mode_str: &str,
    ) -> Option<Vec<(i32, i32)>> {
        self.with_tilemap(entity_id, |tilemap, _| {
            crate::tilemap::find_path(
                tilemap,
                layer_name,
                start,
                goal,
                crate::tilemap::PathMode::from_str_lossy(mode_str),
            )
        })
        .flatten()
    }

    /// 2.6 — same as [`Self::tilemap_find_path`] but with world-space
    /// inputs and outputs. The returned points are tile centres.
    pub fn tilemap_find_path_world(
        &self,
        entity_id: u64,
        layer_name: &str,
        start_world: (f32, f32),
        goal_world: (f32, f32),
        mode_str: &str,
    ) -> Option<Vec<(f32, f32)>> {
        self.with_tilemap(entity_id, |tilemap, pos| {
            let (ex, ey) = pos?;
            let start = tilemap.world_to_tile(ex, ey, start_world.0, start_world.1);
            let goal = tilemap.world_to_tile(ex, ey, goal_world.0, goal_world.1);
            let tiles = crate::tilemap::find_path(
                tilemap,
                layer_name,
                start,
                goal,
                crate::tilemap::PathMode::from_str_lossy(mode_str),
            )?;
            Some(
                tiles
                    .into_iter()
                    .map(|(tx, ty)| tilemap.tile_to_world(ex, ey, tx, ty))
                    .collect(),
            )
        })
        .flatten()
    }
}

/// Context passed to a script adapter during a lifecycle call.
pub struct BridgeContext<'a> {
    /// Frame delta time in seconds.
    pub dt: f32,
    /// Read-only world snapshot for the current frame.
    pub read: &'a BridgeReadSnapshot,
    /// Capability flags the host advertises this run.
    pub capabilities: Capabilities,
    /// Command queue — adapters push deferred mutations here.
    commands: &'a mut Vec<BridgeCommand>,
    /// Phase 5.1: shared monotonic counter used by `spawn_entity`/`spawn_prefab`
    /// to pre-allocate entity IDs so the calling script can reference the
    /// freshly-spawned entity in the same frame. Wrapped in `Rc<Cell>` because
    /// adapter callbacks (e.g. `Function::new` closures in mlua/rquickjs)
    /// need to mutate it without borrowing `BridgeContext` itself.
    spawn_counter: Rc<Cell<u64>>,
    /// Phase 5.5: monotonic counter used by `play_sound` to allocate audio
    /// handles. Kept separate from `spawn_counter` so the entity-id and
    /// sound-handle ID spaces don't collide visually in logs/debuggers.
    sound_counter: Rc<Cell<u64>>,
    /// Phase 5.6: monotonic counter used by `tween_*` to allocate tween ids.
    /// Stored as `u64` for a uniform counter type even though tween ids are
    /// `u32` at the API surface — adapters cast on the way out.
    tween_counter: Rc<Cell<u64>>,
    /// Phase 5.8: monotonic counter used by `on_event` to allocate listener
    /// indices. Pre-allocated bridge-side so scripts can store the id
    /// synchronously even though the engine wires the listener up later.
    event_listener_counter: Rc<Cell<u64>>,
    /// Phase 5.8: monotonic counter used by `set_timer`/`set_interval` to
    /// allocate timer ids. Same rationale as the other counters.
    timer_counter: Rc<Cell<u64>>,
}

impl<'a> BridgeContext<'a> {
    /// Build a new context. Engine code constructs this each frame.
    ///
    /// `spawn_counter` is shared with the engine — callers should keep one
    /// counter per scripting subsystem and pass an `Rc` clone in. Default
    /// engine convention starts at `1_000_000` to avoid colliding with hecs
    /// entity IDs (mirrors [`crate::scripting::api::ScriptState::next_spawn_id`]).
    pub fn new(
        dt: f32,
        read: &'a BridgeReadSnapshot,
        capabilities: Capabilities,
        commands: &'a mut Vec<BridgeCommand>,
    ) -> Self {
        Self {
            dt,
            read,
            capabilities,
            commands,
            spawn_counter: Rc::new(Cell::new(1_000_000)),
            sound_counter: Rc::new(Cell::new(2_000_000)),
            tween_counter: Rc::new(Cell::new(3_000_000)),
            event_listener_counter: Rc::new(Cell::new(4_000_000)),
            timer_counter: Rc::new(Cell::new(5_000_000)),
        }
    }

    /// Same as [`Self::new`] but with an externally-owned counter (used by
    /// the engine's frame loop to keep allocation contiguous across frames).
    pub fn with_spawn_counter(
        dt: f32,
        read: &'a BridgeReadSnapshot,
        capabilities: Capabilities,
        commands: &'a mut Vec<BridgeCommand>,
        spawn_counter: Rc<Cell<u64>>,
    ) -> Self {
        Self {
            dt,
            read,
            capabilities,
            commands,
            spawn_counter,
            sound_counter: Rc::new(Cell::new(2_000_000)),
            tween_counter: Rc::new(Cell::new(3_000_000)),
            event_listener_counter: Rc::new(Cell::new(4_000_000)),
            timer_counter: Rc::new(Cell::new(5_000_000)),
        }
    }

    /// As [`Self::with_spawn_counter`] but also accepts an external sound
    /// counter so the engine can keep audio handle allocation contiguous
    /// across frames.
    pub fn with_counters(
        dt: f32,
        read: &'a BridgeReadSnapshot,
        capabilities: Capabilities,
        commands: &'a mut Vec<BridgeCommand>,
        spawn_counter: Rc<Cell<u64>>,
        sound_counter: Rc<Cell<u64>>,
    ) -> Self {
        Self {
            dt,
            read,
            capabilities,
            commands,
            spawn_counter,
            sound_counter,
            tween_counter: Rc::new(Cell::new(3_000_000)),
            event_listener_counter: Rc::new(Cell::new(4_000_000)),
            timer_counter: Rc::new(Cell::new(5_000_000)),
        }
    }

    /// As [`Self::with_counters`] but also accepts an external tween counter.
    /// Preferred constructor in the frame loop once tween allocation needs to
    /// stay contiguous.
    pub fn with_all_counters(
        dt: f32,
        read: &'a BridgeReadSnapshot,
        capabilities: Capabilities,
        commands: &'a mut Vec<BridgeCommand>,
        spawn_counter: Rc<Cell<u64>>,
        sound_counter: Rc<Cell<u64>>,
        tween_counter: Rc<Cell<u64>>,
    ) -> Self {
        Self {
            dt,
            read,
            capabilities,
            commands,
            spawn_counter,
            sound_counter,
            tween_counter,
            event_listener_counter: Rc::new(Cell::new(4_000_000)),
            timer_counter: Rc::new(Cell::new(5_000_000)),
        }
    }

    /// As [`Self::with_all_counters`] but also accepts external event-listener
    /// and timer counters (Phase 5.8). Preferred constructor in the frame loop
    /// once the engine wires events/timers through the bridge.
    #[allow(clippy::too_many_arguments)]
    pub fn with_full_counters(
        dt: f32,
        read: &'a BridgeReadSnapshot,
        capabilities: Capabilities,
        commands: &'a mut Vec<BridgeCommand>,
        spawn_counter: Rc<Cell<u64>>,
        sound_counter: Rc<Cell<u64>>,
        tween_counter: Rc<Cell<u64>>,
        event_listener_counter: Rc<Cell<u64>>,
        timer_counter: Rc<Cell<u64>>,
    ) -> Self {
        Self {
            dt,
            read,
            capabilities,
            commands,
            spawn_counter,
            sound_counter,
            tween_counter,
            event_listener_counter,
            timer_counter,
        }
    }

    /// Borrow a clone of the spawn counter handle for adapter callbacks.
    pub fn spawn_counter(&self) -> Rc<Cell<u64>> {
        self.spawn_counter.clone()
    }

    /// Borrow a clone of the sound counter handle for adapter callbacks.
    pub fn sound_counter(&self) -> Rc<Cell<u64>> {
        self.sound_counter.clone()
    }

    /// Borrow a clone of the tween counter handle for adapter callbacks.
    pub fn tween_counter(&self) -> Rc<Cell<u64>> {
        self.tween_counter.clone()
    }

    /// Borrow a clone of the event-listener counter handle for adapter callbacks.
    pub fn event_listener_counter(&self) -> Rc<Cell<u64>> {
        self.event_listener_counter.clone()
    }

    /// Borrow a clone of the timer counter handle for adapter callbacks.
    pub fn timer_counter(&self) -> Rc<Cell<u64>> {
        self.timer_counter.clone()
    }

    /// Allocate the next spawn id (engine-side helper for tests / non-callback
    /// code that wants direct allocation).
    pub fn alloc_spawn_id(&self) -> u64 {
        let id = self.spawn_counter.get();
        self.spawn_counter.set(id + 1);
        id
    }

    /// Queue a deferred command.
    pub fn push_command(&mut self, cmd: BridgeCommand) {
        self.commands.push(cmd);
    }

    /// Number of commands queued so far this frame.
    pub fn pending_command_count(&self) -> usize {
        self.commands.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_count() {
        let read = BridgeReadSnapshot::default();
        let mut cmds = Vec::new();
        let mut ctx = BridgeContext::new(0.016, &read, Capabilities::empty(), &mut cmds);
        ctx.push_command(BridgeCommand::Log { message: "hi".into() });
        assert_eq!(ctx.pending_command_count(), 1);
        assert_eq!(cmds.len(), 1);
    }
}
