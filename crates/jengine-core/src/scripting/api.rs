//! Engine API exposed to scripts — the bridge between game scripts and engine internals.

use std::collections::{HashMap, HashSet};

use crate::animation::easing::EasingFunction;
use crate::animation::tween::{TweenManager, TweenTarget};

/// A game event fired by a script, dispatched to registered listeners.
#[derive(Debug, Clone)]
pub struct GameEvent {
    pub name: String,
    pub sender: u64,
    pub data: Option<serde_json::Value>,
}

/// Debug draw command — rendered for one frame then discarded.
#[derive(Debug, Clone)]
pub enum DebugDrawCommand {
    Line { x1: f32, y1: f32, x2: f32, y2: f32, r: f32, g: f32, b: f32 },
    Rect { x: f32, y: f32, w: f32, h: f32, r: f32, g: f32, b: f32 },
    Circle { x: f32, y: f32, radius: f32, r: f32, g: f32, b: f32 },
}

/// A script timer — fires a named callback after a delay.
#[derive(Debug, Clone)]
pub struct Timer {
    pub id: u32,
    pub entity_id: u64,
    pub callback_name: String,
    pub delay: f32,
    pub elapsed: f32,
    pub repeating: bool,
}

/// A registered GUI click handler — maps a node click to a script callback.
#[derive(Debug, Clone)]
pub struct GuiClickHandler {
    pub gui_id: String,
    pub node_id: String,
    pub entity_id: u64,
    pub callback_name: String,
}

/// Read-only world view that scripts query each frame. Rebuilt by snapshot
/// systems before script execution. Holds the `world_ptr` + `entity_map` used
/// for zero-copy ECS reads during script invocation.
///
/// P8.1 split: see [`ScriptState`] for the full container.
pub struct ScriptSnapshot {
    /// Read-only snapshot of entity positions for scripts to query.
    pub positions: HashMap<u64, (f32, f32)>,
    /// Read-only snapshot of entity rotations for scripts to query.
    pub rotations: HashMap<u64, f32>,
    /// Read-only snapshot of entity scales for scripts to query.
    pub scales: HashMap<u64, (f32, f32)>,
    /// Read-only snapshot of entity sprite colors for scripts to query.
    pub sprite_colors: HashMap<u64, [f32; 4]>,
    /// Read-only snapshot of entity sprite anchors: entity_id → [ax, ay].
    pub sprite_anchors: HashMap<u64, [f32; 2]>,
    /// Read-only snapshot of sprite info for point lookups:
    /// entity_id → (texture_id, size_w, size_h).
    pub sprite_info: HashMap<u64, (String, f32, f32)>,
    /// Read-only snapshot of entity names for scripts to query.
    pub names: HashMap<u64, String>,
    /// Read-only snapshot of custom component data per entity.
    pub custom_components: HashMap<u64, HashMap<String, serde_json::Value>>,
    /// Read-only snapshot of entity tags.
    pub tags: HashMap<u64, std::collections::HashSet<String>>,
    /// Delta time for the current frame.
    pub delta_time: f32,
    /// Cached frame stats snapshot for scripts to query.
    pub frame_stats: crate::profiling::FrameStats,
    /// Velocity snapshots for scripts to query: entity_id → (vx, vy).
    pub velocities: HashMap<u64, (f32, f32)>,
    /// Angular velocity snapshots: entity_id → omega.
    pub angular_velocities: HashMap<u64, f32>,
    /// Gravity snapshot for scripts to query.
    pub gravity: (f32, f32),
    /// Contact events from the last physics step.
    pub contact_events: Vec<crate::physics::ContactEvent>,
    /// Physics world pointer for synchronous queries (raycast). Set during snapshot_physics.
    pub physics_world_ptr: Option<*const crate::physics::PhysicsWorld>,
    /// World pointer for direct ECS reads during script execution (avoids cloning).
    /// SAFETY: Valid only during ScriptSystem::run() — cleared after scripts finish.
    pub world_ptr: Option<*const hecs::World>,
    /// Entity handle map: raw entity ID (u64) → hecs::Entity for direct world queries.
    pub entity_map: HashMap<u64, hecs::Entity>,
    /// Snapshot of GUI component data for scripts to query: entity_id → (gui_id, visible).
    pub gui_snapshots: HashMap<u64, (String, bool)>,
    /// Name of the currently active scene.
    pub current_scene_name: String,
}

impl Default for ScriptSnapshot {
    fn default() -> Self {
        Self {
            positions: HashMap::new(),
            rotations: HashMap::new(),
            scales: HashMap::new(),
            sprite_colors: HashMap::new(),
            sprite_anchors: HashMap::new(),
            sprite_info: HashMap::new(),
            names: HashMap::new(),
            custom_components: HashMap::new(),
            tags: HashMap::new(),
            delta_time: 0.0,
            frame_stats: crate::profiling::FrameStats::default(),
            velocities: HashMap::new(),
            angular_velocities: HashMap::new(),
            gravity: (0.0, -9.81),
            contact_events: Vec::new(),
            physics_world_ptr: None,
            world_ptr: None,
            entity_map: HashMap::new(),
            gui_snapshots: HashMap::new(),
            current_scene_name: String::new(),
        }
    }
}

/// Per-frame input state. Written by `snapshot_input` once per frame, read by
/// bridge runners and `process_gui_clicks`.
///
/// P8.1 split: see [`ScriptState`] for the full container.
#[derive(Default)]
pub struct ScriptInput {
    /// Keys currently held down (string names like "W", "Space", "Left").
    pub pressed_keys: HashSet<String>,
    /// Keys pressed this frame (string names).
    pub just_pressed_keys: HashSet<String>,
    /// Mouse wheel delta for this frame.
    pub mouse_wheel_delta: f32,
    /// Action mappings: action name → pressed state.
    pub actions_pressed: HashSet<String>,
    /// Action mappings: action name → just pressed this frame.
    pub actions_just_pressed: HashSet<String>,
    /// Action mappings: action name → just released this frame.
    pub actions_just_released: HashSet<String>,
    /// Mouse position snapshot for hit-testing.
    pub mouse_position: (f32, f32),
    /// Whether the left mouse button was just pressed this frame.
    pub mouse_left_just_pressed: bool,
    /// Whether the left mouse button is held down right now.
    pub mouse_left: bool,
    /// Whether the left mouse button was just released this frame.
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
    /// Whether a GUI scroll gesture owns the pointer this frame.
    ///
    /// Set by the runtime from its `GuiScrollState`. `process_gui_clicks`
    /// swallows the release when it is true, so dragging a list of buttons
    /// scrolls without also activating the button the drag started on.
    ///
    /// A separate flag rather than a parameter because this is read deep
    /// inside `ScriptSystem::update`, which has no access to the runtime's
    /// scroll state.
    pub gui_scroll_dragging: bool,
    /// Screen size for GUI layout computation.
    pub screen_size: (f32, f32),
}

/// A tween a script started, not yet given its start value.
#[derive(Debug, Clone)]
pub struct PendingTween {
    /// The id the bridge already returned to the script, so a later
    /// `cancel_tween(id)` matches.
    pub tween_id: u32,
    pub entity_id: u64,
    pub target: TweenTarget,
    pub end_value: glam::Vec4,
    pub duration: f32,
    pub easing: EasingFunction,
}

/// Long-lived engine state mutated via bridge commands and ticked by dedicated
/// systems: prefab templates, timers, event listeners, GUI click bindings,
/// tween manager, particle simulator, audio handles, debug draw queue.
///
/// P8.1 split: see [`ScriptState`] for the full container.
pub struct ScriptServices {
    /// Loaded prefab templates: prefab name → EntityData.
    pub prefabs: HashMap<String, crate::scene::format::EntityData>,
    /// Next entity ID for pre-allocated spawned entity IDs.
    pub next_spawn_id: u64,
    /// Spawned entity IDs this frame: (name, pre-allocated_id).
    pub spawned_entities: Vec<(String, u64)>,
    /// Pending script loads: (entity_id, script_path).
    /// Applied during `apply_script_commands` by adding ScriptComponent.
    pub pending_script_loads: Vec<(u64, String)>,
    /// Event listeners: event_name → [(entity_id, listener_index)].
    pub event_listeners: HashMap<String, Vec<(u64, u32)>>,
    /// Events fired this frame (collected from SendEvent commands).
    pub pending_events: Vec<GameEvent>,
    /// Debug draw commands for this frame (auto-cleared).
    pub debug_draws: Vec<DebugDrawCommand>,
    /// Active timers.
    pub timers: Vec<Timer>,
    /// Next timer ID.
    pub next_timer_id: u32,
    /// Fired timer callbacks this frame: (entity_id, callback_name).
    pub fired_timers: Vec<(u64, String)>,
    /// Whether the debug overlay is visible.
    pub debug_overlay_visible: bool,
    /// Tween manager — drives property tweens.
    pub tween_manager: TweenManager,
    /// Tweens a script started this frame, waiting for the frame's commands
    /// to be applied so each starts from where its entity really is (a
    /// `set_position` just before a `tween_position` is common). See
    /// `ScriptSystem::start_pending_tweens`.
    pub pending_tweens: Vec<PendingTween>,
    /// Centralized particle simulation.
    pub particle_system: crate::particles::ParticleSystem,
    /// Last sound handle returned by play_sound (for scripts to reference).
    pub last_sound_handle: u64,
    /// Registered GUI click handlers: (gui_id, node_id, entity_id, callback_name).
    pub gui_click_handlers: Vec<GuiClickHandler>,
    /// How the UI is scaled on this project (see `ui::scaling`). The GUI hit
    /// test has to agree with the renderer about what a pixel is, so the
    /// runtime sets this from project settings at startup.
    pub ui_scale: crate::ui::scaling::UiScale,
    /// Fired GUI click callbacks this frame: (entity_id, callback_name).
    /// Per-frame queue of `(entity_id, callback_name, node_id)` triples
    /// populated by [`Self::process_gui_clicks`] when a registered GUI
    /// node was clicked this frame. The dispatcher invokes
    /// `callback(entity, node_id)` so a single `on_gui_click` handler
    /// can route by node id.
    pub fired_gui_clicks: Vec<(u64, String, String)>,
    /// `.gui` nodes pressed without a pointer (`ecs::systems::navigate_ui`),
    /// as `(gui_id, node_id)`. The host queues them; the next
    /// [`Self::process_gui_clicks`] fires their handlers exactly as a click.
    pub pending_gui_activations: Vec<(String, String)>,
    /// Pending camera-shake requests queued by scripts this frame as
    /// `(amplitude, duration, frequency)`. The host runtime drains this
    /// vector after `ScriptSystem::run` and applies each entry to its
    /// active [`crate::renderer::camera::Camera2D`] via `start_shake`.
    /// Stored here (not in the ECS World) because `Camera2D` is a
    /// runtime-owned resource, not a component.
    pub pending_camera_shakes: Vec<(f32, f32, f32)>,
    /// 2.3 — runtime save-slot store. Bridge commands (`SaveSet`,
    /// `SaveCommit`, `SaveClear`) mutate this directly inside
    /// `route_bridge_commands`; `ScriptSystem::run` snapshots its contents
    /// into `BridgeReadSnapshot.save_data` each frame so `save_get` reads
    /// see latest values without taking another borrow.
    pub save_system: crate::save::SaveSystem,
    /// 2.4 — audio mixer bus tree. Bridge `Audio*Bus*` commands route
    /// into this directly; `apply_audio_commands` consults it to apply
    /// effective volumes when starting sounds and to re-apply on dirty
    /// buses for in-flight tracks. Snapshotted into
    /// `BridgeReadSnapshot.audio_mixer` so script reads (`audio_get_bus_volume`)
    /// don't re-borrow the live mixer.
    pub mixer: crate::audio::AudioMixer,
    /// 2.5 — queue of `BridgeCommand::Input*` variants that mutate the
    /// runtime [`crate::input::InputState`]. `route_bridge_commands`
    /// pushes them here; the host runtime drains via
    /// [`crate::input::apply_input_remap`] after `ScriptSystem::run`.
    /// Stored here (not in InputState) because `route_bridge_commands`
    /// only has access to `ScriptState`, not the live `InputState`.
    pub pending_input_remaps: Vec<crate::scripting::bridge::BridgeCommand>,
}

impl Default for ScriptServices {
    fn default() -> Self {
        Self {
            prefabs: HashMap::new(),
            next_spawn_id: 1_000_000, // High start to avoid collision with hecs entity IDs
            spawned_entities: Vec::new(),
            pending_script_loads: Vec::new(),
            event_listeners: HashMap::new(),
            pending_events: Vec::new(),
            debug_draws: Vec::new(),
            timers: Vec::new(),
            next_timer_id: 1,
            fired_timers: Vec::new(),
            debug_overlay_visible: false,
            tween_manager: TweenManager::new(),
            pending_tweens: Vec::new(),
            particle_system: crate::particles::ParticleSystem::new(),
            last_sound_handle: 0,
            gui_click_handlers: Vec::new(),
            ui_scale: crate::ui::scaling::UiScale::default(),
            fired_gui_clicks: Vec::new(),
            pending_gui_activations: Vec::new(),
            pending_camera_shakes: Vec::new(),
            save_system: crate::save::SaveSystem::new(),
            mixer: crate::audio::AudioMixer::new(),
            pending_input_remaps: Vec::new(),
        }
    }
}

// ----------------------------------------------------------------------
// P8.2a — read accessors on ScriptSnapshot
// ----------------------------------------------------------------------

impl ScriptSnapshot {
    /// The world lent for this frame - the one deref of `world_ptr`, set and
    /// cleared by `ScriptSystem::run` around the runners' tick.
    pub fn world(&self) -> Option<&hecs::World> {
        // SAFETY: see `BridgeReadSnapshot::world`; same contract.
        self.world_ptr.map(|wp| unsafe { &*wp })
    }

    fn live_entity(&self, entity_id: u64) -> Option<(&hecs::World, hecs::Entity)> {
        Some((self.world()?, *self.entity_map.get(&entity_id)?))
    }

    /// The physics world lent for this frame, if any.
    pub fn physics(&self) -> Option<&crate::physics::PhysicsWorld> {
        // SAFETY: as `world`.
        self.physics_world_ptr.map(|p| unsafe { &*p })
    }

    /// Get position of an entity (for scripts).
    /// Reads directly from the ECS world via world_ptr (zero-copy).
    pub fn get_position(&self, entity_id: u64) -> Option<(f32, f32)> {
        if let Some((world, entity)) = self.live_entity(entity_id)
            && let Ok(t) = world.get::<&crate::ecs::components::Transform>(entity)
        {
            return Some((t.position.x, t.position.y));
        }
        self.positions.get(&entity_id).copied()
    }

    /// Get rotation of an entity (for scripts).
    pub fn get_rotation(&self, entity_id: u64) -> Option<f32> {
        if let Some((world, entity)) = self.live_entity(entity_id)
            && let Ok(t) = world.get::<&crate::ecs::components::Transform>(entity)
        {
            return Some(t.rotation);
        }
        self.rotations.get(&entity_id).copied()
    }

    /// Get scale of an entity (for scripts).
    pub fn get_scale(&self, entity_id: u64) -> Option<(f32, f32)> {
        if let Some((world, entity)) = self.live_entity(entity_id)
            && let Ok(t) = world.get::<&crate::ecs::components::Transform>(entity)
        {
            return Some((t.scale.x, t.scale.y));
        }
        self.scales.get(&entity_id).copied()
    }

    /// Get sprite color of an entity (for scripts).
    pub fn get_sprite_color(&self, entity_id: u64) -> Option<[f32; 4]> {
        if let Some((world, entity)) = self.live_entity(entity_id)
            && let Ok(s) = world.get::<&crate::ecs::components::SpriteComponent>(entity)
        {
            return Some(s.color);
        }
        self.sprite_colors.get(&entity_id).copied()
    }

    /// Get sprite anchor of an entity (for scripts).
    pub fn get_sprite_anchor(&self, entity_id: u64) -> Option<[f32; 2]> {
        if let Some((world, entity)) = self.live_entity(entity_id)
            && let Ok(s) = world.get::<&crate::ecs::components::SpriteComponent>(entity)
        {
            return Some(s.anchor);
        }
        self.sprite_anchors.get(&entity_id).copied()
    }

    /// Get entity ID by name (for scripts).
    /// Reads directly from the ECS world via world_ptr to avoid cloning all name strings.
    pub fn get_entity_by_name(&self, name: &str) -> Option<u64> {
        if let Some(world) = self.world() {
            for (entity, n) in world.query::<&crate::ecs::components::Name>().iter() {
                if n.0 == name {
                    return Some(entity.id() as u64);
                }
            }
            return None;
        }
        self.names
            .iter()
            .find(|(_, n)| n.as_str() == name)
            .map(|(id, _)| *id)
    }

    /// Get delta time for the current frame.
    pub fn get_delta_time(&self) -> f32 {
        self.delta_time
    }

    /// Get a custom component field value for an entity (for scripts).
    /// Reads directly from the ECS world via world_ptr for zero-copy access.
    pub fn get_custom_field(
        &self,
        entity_id: u64,
        component_name: &str,
        field_name: &str,
    ) -> Option<serde_json::Value> {
        if let Some((world, entity)) = self.live_entity(entity_id)
            && let Ok(custom) = world.get::<&crate::ecs::components::CustomComponents>(entity)
        {
            if let Some(comp) = custom.0.get(component_name) {
                if let Some(val) = comp.get(field_name) {
                    return Some(val.clone());
                }
                let fields: Vec<&str> = comp.as_object()
                    .map(|o| o.keys().map(|k| k.as_str()).collect())
                    .unwrap_or_default();
                log::warn!(
                    "get_custom: \"{component_name}.{field_name}\" not found on entity {entity_id}. Available fields: {fields:?}"
                );
                return None;
            }
            log::warn!(
                "get_custom: entity {entity_id} has no component \"{component_name}\""
            );
            return None;
        }
        self.custom_components
            .get(&entity_id)
            .and_then(|map| map.get(component_name))
            .and_then(|comp| comp.get(field_name))
            .cloned()
    }

    /// Get the name of the currently active scene.
    pub fn get_current_scene_name(&self) -> &str {
        &self.current_scene_name
    }

    /// Check if an entity has a specific tag.
    pub fn has_tag(&self, entity_id: u64, tag: &str) -> bool {
        if let Some((world, entity)) = self.live_entity(entity_id) {
            if let Ok(tags) = world.get::<&crate::ecs::components::Tags>(entity) {
                return tags.0.contains(tag);
            }
            return false;
        }
        self.tags
            .get(&entity_id)
            .map(|tags| tags.contains(tag))
            .unwrap_or(false)
    }

    /// Get all entity IDs that have a given tag.
    pub fn get_entities_by_tag(&self, tag: &str) -> Vec<u64> {
        if let Some(world) = self.world() {
            return world.query::<&crate::ecs::components::Tags>()
                .iter()
                .filter(|(_, tags)| tags.0.contains(tag))
                .map(|(entity, _)| entity.id() as u64)
                .collect();
        }
        self.tags
            .iter()
            .filter(|(_, tags)| tags.contains(tag))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Get current FPS (smoothed over recent frames).
    pub fn get_fps(&self) -> f32 {
        self.frame_stats.fps
    }

    /// Get current frame time in milliseconds.
    pub fn get_frame_time(&self) -> f32 {
        self.frame_stats.frame_time_ms
    }

    /// Get total entity count.
    pub fn get_entity_count(&self) -> u32 {
        self.frame_stats.entity_count
    }

    /// Get draw call count for this frame.
    pub fn get_draw_calls(&self) -> u32 {
        self.frame_stats.draw_calls
    }

    /// Get the linear velocity of an entity.
    pub fn get_velocity(&self, entity_id: u64) -> Option<(f32, f32)> {
        self.velocities.get(&entity_id).copied()
    }

    /// Get the angular velocity of an entity.
    pub fn get_angular_velocity(&self, entity_id: u64) -> Option<f32> {
        self.angular_velocities.get(&entity_id).copied()
    }

    /// Get the current world gravity.
    pub fn get_gravity(&self) -> (f32, f32) {
        self.gravity
    }

    /// Get contact events from the last physics step.
    pub fn get_contacts(&self) -> &[crate::physics::ContactEvent] {
        &self.contact_events
    }

    /// Perform a raycast against the physics world (synchronous).
    /// Returns None if no hit or physics world not available.
    pub fn raycast(&self, ox: f32, oy: f32, dx: f32, dy: f32, max_dist: f32) -> Option<crate::physics::RayHit> {
        let physics = self.physics()?;
        physics.raycast(ox, oy, dx, dy, max_dist)
    }

    /// Get a property from a GUI node. Returns json value from snapshots.
    /// Note: This reads from the engine's GUI document assets (not deferred).
    pub fn gui_get(&self, _gui_id: &str, _node_id: &str, _property: &str) -> Option<serde_json::Value> {
        // Lookup will be done in the script backends directly from AssetManager
        // This is a placeholder — actual implementation reads from the asset cache.
        None
    }

    // -- P8.2c — snapshot builders moved from EngineApi --

    /// Snapshot entity transforms, sprites, names, tags, and custom components from the world.
    ///
    /// Sets up `world_ptr` for direct ECS reads during script execution.
    /// Only builds the `entity_map` (entity ID → hecs::Entity handle).
    /// All data (positions, rotations, colors, etc.) is read lazily via `world_ptr`
    /// when scripts actually request it, rather than copying into HashMaps every frame.
    pub fn snapshot_world(&mut self, world: &hecs::World) {
        // Store a raw pointer for direct ECS reads during script execution.
        // SAFETY: The pointer is valid for the duration of ScriptSystem::run(),
        // which holds the &mut World borrow. No mutation occurs until apply_script_commands.
        self.world_ptr = Some(world as *const hecs::World);

        self.entity_map.clear();
        self.velocities.clear();

        // Build entity_map from all entities (needed for ID → Entity handle resolution).
        // This is a single lightweight pass — no component data is copied.
        for (entity, _) in world.query::<()>().iter() {
            self.entity_map.insert(entity.id() as u64, entity);
        }

        // Snapshot Velocity2D — these are lightweight (just two f32s per entity)
        // and needed by both scripts and physics snapshot.
        for (entity, vel) in world.query::<&crate::ecs::components::Velocity2D>().iter() {
            let id = entity.id() as u64;
            self.velocities.insert(id, (vel.x, vel.y));
        }

        self.gui_snapshots.clear();
        for (entity, gui) in world.query::<&crate::ecs::components::GuiComponent>().iter() {
            let id = entity.id() as u64;
            self.gui_snapshots.insert(id, (gui.gui_id.clone(), gui.visible));
        }
    }

    /// Snapshot physics velocities from the PhysicsWorld for scripts to query.
    #[cfg(feature = "physics")]
    pub fn snapshot_physics(&mut self, physics: &crate::physics::PhysicsWorld, world: &hecs::World) {
        self.velocities.clear();
        self.angular_velocities.clear();
        for (entity, _rb) in world.query::<&crate::ecs::components::RigidBody2D>().iter() {
            let eid = entity.id() as u64;
            if let Some((vx, vy)) = physics.get_velocity(eid) {
                self.velocities.insert(eid, (vx, vy));
            }
            if let Some(omega) = physics.get_angular_velocity(eid) {
                self.angular_velocities.insert(eid, omega);
            }
        }
        self.gravity = physics.get_gravity();
        // Store physics world pointer for synchronous queries (e.g. raycast).
        self.physics_world_ptr = Some(physics as *const crate::physics::PhysicsWorld);
    }
}

/// Resolve every entity's named atlas points, normalised to its sprite
/// region, as `entity -> name -> (nx, ny)`.
///
/// Free-standing because the editor viewport and web runtime need the same
/// map to publish for `get_sprite_point`, and neither owns a
/// [`ScriptState`]. Was inline in `snapshot_sprite_points`, which is now a
/// one-line delegate.
pub fn compute_sprite_points(
    world: &hecs::World,
    assets: &crate::asset::AssetManager,
) -> std::collections::HashMap<u64, std::collections::HashMap<String, (f32, f32)>> {
    let mut out: std::collections::HashMap<u64, std::collections::HashMap<String, (f32, f32)>> =
        std::collections::HashMap::new();
    // No atlas has a point - most projects - so no sprite does: nothing to walk.
    if !assets.any_sprite_points() {
        return out;
    }
    // The animator from the same query, and the last texture's region
    // remembered: this runs over every sprite every frame, and a crowd of one
    // sprite paid a component lookup and a string-hashed region lookup each.
    let mut last_region: Option<(usize, usize, bool)> = None;
    for (entity, (sprite, animator)) in world
        .query::<(&crate::ecs::components::SpriteComponent, Option<&crate::animation::SpriteAnimator>)>()
        .iter()
    {
        let id = entity.id() as u64;
        if animator.is_none() {
            // Most sprites: no animation, and a texture with no points.
            let key = (sprite.texture_id.as_ptr() as usize, sprite.texture_id.len());
            if let Some((p, l, has)) = last_region
                && (p, l) == key
                && !has
            {
                continue;
            }
            let has = assets.get_sprite_region(&sprite.texture_id).is_some_and(|r| !r.points.is_empty());
            last_region = Some((key.0, key.1, has));
            if !has {
                continue;
            }
        }

        let anim_points = animator
            .and_then(|animator| {
                let clip_name = animator.current_clip.as_ref()?;
                let frame_idx = animator.current_frame;
                if let Some((atlas_id, _)) = assets.atlas_lookup_entry(&sprite.texture_id) {
                    assets.get_animation_frame_points(&atlas_id, clip_name, frame_idx)
                } else {
                    None
                }
            });

        if let Some(points) = anim_points {
            let entry = out.entry(id).or_default();
            for point in points {
                if let Some(region) = assets.get_sprite_region(&sprite.texture_id) {
                    let nx = if region.width > 0 { point.x / region.width as f32 } else { 0.0 };
                    let ny = if region.height > 0 { point.y / region.height as f32 } else { 0.0 };
                    entry.insert(point.name.clone(), (nx, ny));
                }
            }
        } else if let Some(region) = assets.get_sprite_region(&sprite.texture_id) {
            if region.points.is_empty() {
                continue;
            }
            let entry = out.entry(id).or_default();
            for point in &region.points {
                let nx = if region.width > 0 { point.x / region.width as f32 } else { 0.0 };
                let ny = if region.height > 0 { point.y / region.height as f32 } else { 0.0 };
                entry.insert(point.name.clone(), (nx, ny));
            }
        }
    }
    out
}

// ----------------------------------------------------------------------
// P8.2a — read accessors on ScriptInput
// ----------------------------------------------------------------------

impl ScriptInput {
    /// Is a key currently held down?
    pub fn is_key_pressed(&self, key: &str) -> bool {
        self.pressed_keys.contains(key)
    }

    /// Was a key pressed this frame?
    pub fn is_key_just_pressed(&self, key: &str) -> bool {
        self.just_pressed_keys.contains(key)
    }

    /// Is an action currently pressed?
    pub fn is_action_pressed(&self, action: &str) -> bool {
        self.actions_pressed.contains(action)
    }

    /// Was an action just pressed this frame?
    pub fn is_action_just_pressed(&self, action: &str) -> bool {
        self.actions_just_pressed.contains(action)
    }

    /// Was an action just released this frame?
    pub fn is_action_just_released(&self, action: &str) -> bool {
        self.actions_just_released.contains(action)
    }

    /// Get mouse wheel delta for this frame.
    pub fn get_mouse_wheel(&self) -> f32 {
        self.mouse_wheel_delta
    }

    // -- P8.2c — snapshot builder moved from EngineApi --

    /// Snapshot input state so scripts can query key presses and actions.
    pub fn snapshot_input(&mut self, input: &crate::input::InputState) {
        self.pressed_keys.clear();
        self.just_pressed_keys.clear();
        for &key in crate::input::ALL_KEYS {
            if input.is_pressed(key) {
                self.pressed_keys.insert(key.name().to_string());
            }
            if input.is_just_pressed(key) {
                self.just_pressed_keys.insert(key.name().to_string());
            }
        }
        self.mouse_wheel_delta = input.get_mouse_wheel();
        self.mouse_position = input.mouse_position;
        self.mouse_left_just_pressed = input.mouse_left_just_pressed;
        self.mouse_left = input.mouse_left;
        self.mouse_left_just_released = input.mouse_left_just_released;
        self.mouse_right_just_pressed = input.mouse_right_just_pressed;
        self.mouse_right = input.mouse_right;
        self.mouse_right_just_released = input.mouse_right_just_released;
        self.mouse_middle_just_pressed = input.mouse_middle_just_pressed;
        self.mouse_middle = input.mouse_middle;
        self.mouse_middle_just_released = input.mouse_middle_just_released;
        self.gui_scroll_dragging = input.gui_scroll_dragging;

        // Snapshot action states
        self.actions_pressed.clear();
        self.actions_just_pressed.clear();
        self.actions_just_released.clear();
        for action_name in input.get_actions().keys() {
            if input.is_action_pressed(action_name) {
                self.actions_pressed.insert(action_name.clone());
            }
            if input.is_action_just_pressed(action_name) {
                self.actions_just_pressed.insert(action_name.clone());
            }
            if input.is_action_just_released(action_name) {
                self.actions_just_released.insert(action_name.clone());
            }
        }
    }
}

// ----------------------------------------------------------------------
// P8.2a — read accessors on ScriptServices
// ----------------------------------------------------------------------

impl ScriptServices {
    /// Get total alive particle count from the particle system.
    pub fn get_particle_count(&self) -> u32 {
        self.particle_system.alive_count()
    }

    /// Get the last sound handle returned by play_sound.
    pub fn get_last_sound_handle(&self) -> u64 {
        self.last_sound_handle
    }

    // -- P8.2b — non-command mutators moved from EngineApi --

    /// Register an event listener for an entity. Returns a listener index.
    pub fn register_event_listener(&mut self, event_name: String, entity_id: u64) -> u32 {
        let listeners = self.event_listeners.entry(event_name).or_default();
        let idx = listeners.len() as u32;
        listeners.push((entity_id, idx));
        idx
    }

    /// Remove an event listener for an entity.
    pub fn remove_event_listener(&mut self, event_name: &str, entity_id: u64) {
        if let Some(listeners) = self.event_listeners.get_mut(event_name) {
            listeners.retain(|(eid, _)| *eid != entity_id);
        }
    }

    /// Draw a debug line for one frame.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_debug_line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, r: f32, g: f32, b: f32) {
        self.debug_draws
            .push(DebugDrawCommand::Line { x1, y1, x2, y2, r, g, b });
    }

    /// Draw a debug rectangle outline for one frame.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_debug_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, g: f32, b: f32) {
        self.debug_draws
            .push(DebugDrawCommand::Rect { x, y, w, h, r, g, b });
    }

    /// Draw a debug circle outline for one frame.
    pub fn draw_debug_circle(&mut self, x: f32, y: f32, radius: f32, r: f32, g: f32, b: f32) {
        self.debug_draws
            .push(DebugDrawCommand::Circle { x, y, radius, r, g, b });
    }

    /// Schedule a one-shot timer. Returns a timer ID.
    pub fn set_timer(&mut self, entity_id: u64, delay: f32, callback_name: String) -> u32 {
        let id = self.next_timer_id;
        self.next_timer_id += 1;
        self.timers.push(Timer {
            id,
            entity_id,
            callback_name,
            delay,
            elapsed: 0.0,
            repeating: false,
        });
        id
    }

    /// Schedule a repeating interval timer. Returns a timer ID.
    pub fn set_interval(&mut self, entity_id: u64, interval: f32, callback_name: String) -> u32 {
        let id = self.next_timer_id;
        self.next_timer_id += 1;
        self.timers.push(Timer {
            id,
            entity_id,
            callback_name,
            delay: interval,
            elapsed: 0.0,
            repeating: true,
        });
        id
    }

    /// Cancel a timer by ID.
    pub fn cancel_timer(&mut self, timer_id: u32) {
        self.timers.retain(|t| t.id != timer_id);
    }

    /// Advance all timers by dt. Fired callbacks are stored in `fired_timers`.
    /// Removes one-shot timers after firing. Resets interval timers.
    pub fn tick_timers(&mut self, dt: f32) {
        let mut to_remove = Vec::new();
        for timer in &mut self.timers {
            timer.elapsed += dt;
            if timer.elapsed >= timer.delay {
                self.fired_timers
                    .push((timer.entity_id, timer.callback_name.clone()));
                if timer.repeating {
                    timer.elapsed -= timer.delay;
                } else {
                    to_remove.push(timer.id);
                }
            }
        }
        self.timers.retain(|t| !to_remove.contains(&t.id));
    }

    /// Drop everything here that belonged to the scene being replaced.
    ///
    /// **The one place that decides what a scene owns**, so the four runtimes
    /// have a contract to match rather than each remembering a list.
    ///
    /// The distinction is entity ids. A `Timer`, an event listener and a GUI
    /// click handler each carry a raw `entity_id: u64`, and the next scene
    /// **reuses those ids** - so a survivor does not merely linger, it fires
    /// its callback on whatever unrelated entity inherits the number. The
    /// tween manager was given exactly this treatment for exactly this reason
    /// and the comment on it says so; these three were simply missed, and
    /// nothing noticed because every game in the repo loads one scene and
    /// stays in it.
    ///
    /// What deliberately survives, and must:
    ///
    /// - `prefabs`, because templates are shared and reloading them per scene
    ///   would be a per-scene stall for no gain;
    /// - `save_store`, which is how anything carries score across a scene at
    ///   all - a compilation of microgames has nowhere else to keep it;
    /// - `ui_scale` and `debug_overlay_visible`, which are settings rather
    ///   than scene state;
    /// - the monotonic counters (`next_timer_id`, `next_spawn_id`,
    ///   `last_sound_handle`), because rewinding them would let a stale id
    ///   from an old scene match a fresh one.
    pub fn clear_scene_scoped(&mut self) {
        // Entity-keyed, and the reason this function exists.
        self.timers.clear();
        self.event_listeners.clear();
        self.gui_click_handlers.clear();

        // Queues that are drained per frame, but can be holding entity ids
        // from the outgoing scene at the moment of the swap.
        self.fired_timers.clear();
        self.fired_gui_clicks.clear();
        self.pending_events.clear();
        self.pending_script_loads.clear();
        self.spawned_entities.clear();
        self.debug_draws.clear();
        self.pending_camera_shakes.clear();
    }

    /// Toggle the debug overlay visibility from scripts.
    pub fn set_debug_overlay(&mut self, visible: bool) {
        self.debug_overlay_visible = visible;
    }

    /// Cancel a specific tween by ID.
    pub fn cancel_tween(&mut self, tween_id: u32) {
        self.tween_manager.cancel_tween(tween_id);
    }

    /// Cancel all tweens for an entity.
    pub fn cancel_all_tweens(&mut self, entity_id: u64) {
        self.tween_manager.cancel_all(entity_id);
    }

    /// Register a click handler for a GUI node. When the user clicks on the node,
    /// the named callback function is called on the registering entity's script.
    pub fn gui_on_click(&mut self, entity_id: u64, gui_id: String, node_id: String, callback_name: String) {
        // Avoid duplicate registrations
        if !self.gui_click_handlers.iter().any(|h| {
            h.gui_id == gui_id && h.node_id == node_id && h.entity_id == entity_id && h.callback_name == callback_name
        }) {
            self.gui_click_handlers.push(GuiClickHandler {
                gui_id,
                node_id,
                entity_id,
                callback_name,
            });
        }
    }

    /// Process GUI click handlers against current mouse state.
    /// Call after snapshot_input each frame. Populates `fired_gui_clicks`.
    ///
    /// P8.2d: Takes `&ScriptInput` and `&AssetManager` explicitly so callers
    /// can borrow only the sub-structs they need, enabling tighter borrows
    /// (e.g. still mutating unrelated `ScriptState` fields elsewhere).
    ///
    /// `visible` lists the `gui_id`s currently shown by some entity's
    /// `GuiComponent`. Handlers are registered once in `init()` and live for
    /// the entity's lifetime, so without this filter every button in every
    /// *hidden* panel stays clickable: a click anywhere in the game that
    /// happened to fall inside a hidden button's rect fired its callback.
    pub fn process_gui_clicks(
        &mut self,
        input: &ScriptInput,
        assets: &crate::asset::AssetManager,
        visible: &std::collections::HashSet<String>,
    ) {
        self.fired_gui_clicks.clear();
        // A controller's press on a focused node is a click on it - the same
        // handlers, the same visibility rule, the same refusal of a disabled
        // button.
        for (gui_id, node_id) in std::mem::take(&mut self.pending_gui_activations) {
            if !visible.contains(&gui_id)
                || assets.get_gui(&gui_id).is_none_or(|d| d.refuses_clicks(&node_id))
            {
                continue;
            }
            for handler in &self.gui_click_handlers {
                if handler.gui_id == gui_id && handler.node_id == node_id {
                    self.fired_gui_clicks.push((
                        handler.entity_id,
                        handler.callback_name.clone(),
                        handler.node_id.clone(),
                    ));
                }
            }
        }
        // Release, not press: a scroll drag must not also press the button it started on.
    // Buttons therefore activate on *release*, which is what every touch
    // surface and desktop toolkit does anyway, and `GuiScrollState` vetoes the
    // release when the gesture turned out to be a scroll. Firing on press made
    // dragging a list of buttons impossible: the press alone activated one.
        //
        // The veto is the caller's job — it owns the scroll state — so a
        // runtime that scrolls passes `false` here after a drag.
        if !input.mouse_left_just_released
            || input.gui_scroll_dragging
            || self.gui_click_handlers.is_empty()
        {
            return;
        }
        // Into virtual pixels, the space the layout below is computed in.
        let (pw, ph) = input.screen_size;
        if pw <= 0.0 || ph <= 0.0 {
            return;
        }
        let mouse = self.ui_scale.to_ui(input.mouse_position.0, input.mouse_position.1, pw, ph);
        let size = self.ui_scale.virtual_size(pw, ph);
        let hits = crate::ui::layout::click_hits(
            self.gui_click_handlers.iter().map(|h| (h.gui_id.as_str(), h.node_id.as_str())),
            visible,
            assets,
            mouse,
            size,
        );
        for handler in &self.gui_click_handlers {
            if hits.iter().any(|(g, n)| *g == handler.gui_id && *n == handler.node_id) {
                self.fired_gui_clicks.push((handler.entity_id, handler.callback_name.clone(), handler.node_id.clone()));
            }
        }
    }
}

/// The API object passed to scripts during init/update calls.
/// Scripts call methods on this to interact with the engine (move entities, spawn, log, etc.).
///
/// P8.1 (2026-04-23): Fields are split into four concern-scoped sub-structs —
/// [`ScriptSnapshot`] (read-only world view), [`ScriptInput`] (per-frame
/// input state), [`ScriptServices`] (long-lived engine state) and the
/// `commands` queue. Methods stay on `EngineApi` for now; their bodies access
/// `self.snapshot.X` / `self.input.X` / `self.services.X` so the categorisation
/// is visible from the call site. P8.2 will move method definitions into the
/// sub-struct impls and migrate external callers.
pub struct ScriptState {
    /// Read-only world view (positions, sprites, animations, …) snapshotted
    /// before each script tick.
    pub snapshot: ScriptSnapshot,
    /// Per-frame input state (keys, mouse, actions, screen size).
    pub input: ScriptInput,
    /// Long-lived engine services (timers, tweens, particles, audio handles,
    /// event listeners, GUI bindings, debug draw queue, prefab templates).
    pub services: ScriptServices,
    /// Outgoing mutation queue drained by `apply_script_commands` after each
    /// script tick.
    pub commands: Vec<ScriptCommand>,
}

/// Commands that scripts can issue, applied to the ECS world after script execution.
#[derive(Debug, Clone)]
pub enum ScriptCommand {
    /// A package-contributed script global. See
    /// [`BridgeCommand::PackageCall`](crate::scripting::bridge::BridgeCommand::PackageCall).
    PackageCall { tag: String, args: serde_json::Value },
    SetPosition { entity_id: u64, x: f32, y: f32 },
    SetRotation { entity_id: u64, radians: f32 },
    SetScale { entity_id: u64, x: f32, y: f32 },
    /// Overwrite an entity's `Health` (`current` + `max`). A non-positive
    /// `max` leaves the existing `max` untouched.
    SetHealth { entity_id: u64, current: f32, max: f32 },
    /// Overwrite an entity's `Hitbox` combat stats (`damage`, `knockback`,
    /// `retrigger`); `active` + cooldowns are preserved.
    SetHitbox { entity_id: u64, damage: f32, knockback: f32, retrigger: f32 },
    /// Slice 3.5.A — attach or detach an entity in the parent hierarchy.
    /// `parent_id == None` detaches the child so it becomes a root entity.
    SetParent { entity_id: u64, parent_id: Option<u64> },
    /// Slice 3.5.C — mutate a `SkeletalPlayer`. See
    /// [`crate::scripting::bridge::command::BridgeCommand::SetSkeletalPlayer`]
    /// for the per-field semantics.
    SetSkeletalPlayer {
        entity_id: u64,
        clip: Option<String>,
        time: Option<f32>,
        speed: Option<f32>,
        looping: Option<bool>,
        playing: Option<bool>,
    },
    SetSpriteColor { entity_id: u64, r: f32, g: f32, b: f32, a: f32 },
    /// `set_camera_zoom(zoom)` - the active camera's zoom, 1.0 being the
    /// framing the scene was authored at.
    SetCameraZoom { zoom: f32 },
    /// `set_camera_pixel_size(size)` - the active camera's world units per
    /// art pixel, 0 to draw at the screen's resolution.
    SetCameraPixelSize { size: f32 },
    SetSpriteAnchor { entity_id: u64, x: f32, y: f32 },
    /// The size a sprite is drawn at, in world units (`Sprite.set_size`).
    SetSpriteSize { entity_id: u64, w: f32, h: f32 },
    /// Draw order of the entity's sprite and text (whichever it has).
    SetZOrder { entity_id: u64, z: i32 },
    /// Colour of the entity's world text.
    SetTextColor { entity_id: u64, r: f32, g: f32, b: f32, a: f32 },
    /// Whether the entity's `Trail` records new points.
    SetTrailEmitting { entity_id: u64, on: bool },
    /// The entity's `mesh`: vertices and 1-based triangle indices as the
    /// script gave them (`host_commands::mesh_from_script`).
    SetMesh { entity_id: u64, vertices: serde_json::Value, indices: serde_json::Value },
    /// Forget every point of the entity's `Trail` - after a teleport.
    ClearTrail { entity_id: u64 },
    /// Queue a hole of `radius` at world `(x, y)` in a carvable sprite.
    CarveSprite { entity_id: u64, x: f32, y: f32, radius: f32 },
    /// Paint a stamp into a paintable sprite (`ecs::paint`).
    PaintSprite { entity_id: u64, x: f32, y: f32, size: f32, color: [f32; 4], shape: String, rotation: f32, depth: f32, blend: String },
    /// The velocity a `CharacterController2D` asks for, and whether it drops
    /// through one-way platforms (`None` leaves that as it was).
    MoveCharacter { entity_id: u64, vx: f32, vy: f32, drop_through: Option<bool> },
    SetSpriteRegion { entity_id: u64, atlas_id: String, region_name: String },
    /// Show `label` from the sprite library (`ecs::sprite_library::set_label`).
    SetSpriteLabel { entity_id: u64, label: String, category: Option<String> },
    /// Draw the sprite as an explicit quad (local-space corner offsets
    /// `[TL, TR, BR, BL]` from the entity position). Used for faked
    /// perspective walls. `None` clears it back to a normal rectangle.
    SetSpriteCorners { entity_id: u64, corners: Option<[[f32; 2]; 4]> },
    SpawnEntity {
        name: String,
        components: Box<Option<crate::scene::format::ComponentBundle>>,
        spawn_id: Option<u64>,
        /// A prefab's parts, spawned under it (`EntityData::children`).
        children: Vec<crate::scene::format::EntityData>,
    },
    /// Slice 3.5.D — instantiate a [`crate::animation::rig::SkeletonRig`] from
    /// the asset manager. `spawn_id` is the pre-allocated id of the rig's root
    /// bone entity; children are real hecs entities and must be reached via
    /// `find_bone` after applying.
    SpawnRig { rig_id: String, spawn_id: u64 },
    /// Slice 3.5.F — set the world-space target of an `IkTwoBone` or
    /// `IkLookAt` constraint on the given entity.
    SetIkTarget { entity_id: u64, x: f32, y: f32 },
    /// See [`crate::scripting::bridge::command::BridgeCommand::CrossfadeAnimation`].
    CrossfadeAnimation { entity_id: u64, clip: String, seconds: f32 },
    /// See [`crate::scripting::bridge::command::BridgeCommand::SetAnimationBlend`].
    SetAnimationBlend { entity_id: u64, clip: String, weight: f32 },
    /// See [`crate::scripting::bridge::command::BridgeCommand::SetAnimationBlendPosition`].
    SetAnimationBlendPosition { entity_id: u64, x: f32, y: f32 },
    /// See [`crate::scripting::bridge::command::BridgeCommand::AnimationLayer`].
    AnimationLayer { entity_id: u64, op: String, layer: String, clip: String, value: f32, seconds: f32, looping: bool },
    /// See [`crate::scripting::bridge::command::BridgeCommand::SetConveyorSpeed`].
    SetConveyorSpeed { entity_id: u64, speed: f32 },
    /// See [`crate::scripting::bridge::command::BridgeCommand::SetIkWeight`].
    SetIkWeight { entity_id: u64, weight: f32 },
    DestroyEntity { entity_id: u64 },
    Log { message: String },
    SetCustomField {
        entity_id: u64,
        component_name: String,
        field_name: String,
        value: serde_json::Value,
    },
    // Phase 3C: Event bus
    SendEvent { name: String, sender: u64, data: Option<serde_json::Value> },
    // Phase 3D: Scene transitions
    LoadScene { path: String },
    QuitGame,
    SetTimeScale { scale: f32 },
    PushScene { path: String },
    PopScene,
    /// Slice 12.A — fullscreen fade overlay transitions (color is RGB 0..=1).
    FadeIn { duration_s: f32, color: [f32; 3] },
    FadeOut { duration_s: f32, color: [f32; 3] },
    /// P1.3 — Screen flash overlay. Starts at full `color.a` opacity and
    /// decays linearly to clear over `duration_s`. Shares the fade-overlay
    /// pipeline on the native runtime; editor viewport logs + ignores.
    ScreenFlash { duration_s: f32, color: [f32; 4] },
    // Phase 3F: Tags
    AddTag { entity_id: u64, tag: String },
    RemoveTag { entity_id: u64, tag: String },
    // Phase 4C: Animation
    PlayAnimation { entity_id: u64, clip_name: String },
    StopAnimation { entity_id: u64 },
    SetAnimationSpeed { entity_id: u64, speed: f32 },
    // Animation state machine
    SetAnimBool { entity_id: u64, param: String, value: bool },
    SetAnimFloat { entity_id: u64, param: String, value: f32 },
    SetAnimTrigger { entity_id: u64, param: String },
    StartStateMachine { entity_id: u64, state_name: String },
    StartTween {
        entity_id: u64,
        target: TweenTarget,
        end_value: [f32; 4],
        duration: f32,
        easing: EasingFunction,
    },
    CancelTween { tween_id: u32 },
    CancelAllTweens { entity_id: u64 },
    // Phase 4D: Tilemap
    SetTile {
        entity_id: u64,
        layer_name: String,
        x: u32,
        y: u32,
        tile_id: Option<u32>,
    },
    // Phase 4F: Particles
    EmitBurst { entity_id: u64, count: u32 },
    SetEmitterActive { entity_id: u64, active: bool },
    SetEmitterRate { entity_id: u64, rate: f32 },
    /// Write any emitter field by name (`particles::set_emitter_field`).
    SetEmitterField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any cloth or rope field by name (`ecs::cloth::set_cloth_field`).
    SetClothField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `path_follow` field by name (`ecs::path_follow::set_path_follow_field`).
    SetPathFollowField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `line` field by name (`renderer::line2d::set_line_field`).
    SetLineField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `trail` field by name (`ecs::trails::set_trail_field`).
    SetTrailField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `fluid` field by name (`ecs::fluid::set_fluid_field`).
    SetFluidField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `mirror` field by name (`ecs::mirror::set_mirror_field`).
    SetMirrorField { entity_id: u64, field: String, value: serde_json::Value },
    /// How much of a bar or radial fill is drawn (`Sprite.set_fill`).
    SetSpriteFill { entity_id: u64, amount: f32 },
    /// Paint a material into a pixel simulation (`PixelSim.paint`).
    PixelPaint { entity_id: u64, x: f32, y: f32, radius: f32, material: String },
    /// Smoke and a push at a world point (`Smoke.puff`).
    SmokePuff { entity_id: u64, x: f32, y: f32, radius: f32, amount: f32, vx: f32, vy: f32 },
    /// Any smoke field by name (`Smoke.set`).
    SetSmokeField { entity_id: u64, field: String, value: serde_json::Value },
    /// A rewind clock's mode or speed (`Rewind.set`).
    SetRewindField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any fog field by name (`Fog.set`).
    SetFogField { entity_id: u64, field: String, value: serde_json::Value },
    /// A region of a wfc generator's layer to fill (`Wfc.generate`).
    WfcGenerate { entity_id: u64, x: i32, y: i32, w: i32, h: i32 },
    /// Any vision field by name (`Vision.set`).
    SetVisionField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any day_cycle field by name (`DayCycle.set`).
    SetDayField { entity_id: u64, field: String, value: serde_json::Value },
    /// A floating_origin field by name (`FloatingOrigin.set`).
    SetOriginField { entity_id: u64, field: String, value: serde_json::Value },
    /// An entity's own pace (`TimeScale.set`).
    SetTimeScaleField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any cast_shadow field by name (`CastShadow.set`).
    SetCastShadowField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any auto_normal field by name (`AutoNormal.set`).
    SetAutoNormalField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any tile_light / tile_light_source field by name (`TileLight.set`).
    SetTileLightField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any trajectory field by name (`Trajectory.set`).
    SetTrajectoryField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any text_path field by name (`TextPath.set`).
    SetTextPathField { entity_id: u64, field: String, value: serde_json::Value },
    /// Any beat_clock field by name (`BeatClock.set`).
    SetBeatClockField { entity_id: u64, field: String, value: serde_json::Value },
    /// Dig or build a smooth terrain round a world point (`Terrain.edit`).
    TerrainEdit { entity_id: u64, x: f32, y: f32, radius: f32, amount: f32 },
    /// `Dialogue.start` / `advance` / `choose` / `stop` (`ecs::dialogue::op`).
    DialogueOp { entity_id: u64, op: String, arg: String },
    /// `UI.drop(item, slot)` (`ui::drag_drop::drop_named`).
    UiDrop { entity_id: u64, slot: String },
    /// `Mode7.place(e, x, y, angle)` (`ecs::mode7::place`).
    Mode7Place { entity_id: u64, x: f32, y: f32, angle: f32 },
    /// `Mode7.set(e, field, value)` (`ecs::mode7::set_field`).
    Mode7Set { entity_id: u64, field: String, value: serde_json::Value },
    /// `SpriteTilt.set(e, field, value)` (`ecs::sprite_tilt::set_field`).
    SpriteTiltSet { entity_id: u64, field: String, value: serde_json::Value },
    /// `Afterimage.set(e, field, value)` (`ecs::afterimage::set_field`).
    AfterimageSet { entity_id: u64, field: String, value: serde_json::Value },
    /// `Physics.slice(x0, y0, x1, y1)` (`ecs::slice`).
    PhysicsSlice { x0: f32, y0: f32, x1: f32, y1: f32 },
    /// `Portal.set(e, field, value)` (`ecs::portal::set_field`).
    PortalSet { entity_id: u64, field: String, value: serde_json::Value },
    /// `Jelly.poke(e, x, y, strength, radius)` (`ecs::jelly::poke`).
    JellyPoke { entity_id: u64, x: f32, y: f32, strength: f32, radius: f32 },
    /// `Jelly.set(e, field, value)` (`ecs::jelly::set_field`).
    JellySet { entity_id: u64, field: String, value: serde_json::Value },
    /// `Marker.set(e, field, value)` (`ecs::offscreen_marker::set_field`).
    MarkerSet { entity_id: u64, field: String, value: serde_json::Value },
    /// `Radial.set(e, field, value)` (`ui::radial_menu::set_field`).
    RadialSet { entity_id: u64, field: String, value: serde_json::Value },
    /// `Bubble.say(e, text, seconds)` (`ui::speech_bubble::say`).
    BubbleSay { entity_id: u64, text: String, seconds: f32 },
    /// `Bubble.set(e, field, value)` (`ui::speech_bubble::set_field`).
    BubbleSet { entity_id: u64, field: String, value: serde_json::Value },
    /// `Outline.set(e, field, value)` (`ecs::sprite_outline::set_field`).
    OutlineSet { entity_id: u64, field: String, value: serde_json::Value },
    /// `Lightning.strike(e, x, y)` (`ecs::lightning::strike`).
    LightningStrike { entity_id: u64, x: f32, y: f32 },
    /// `Lightning.set(e, field, value)` (`ecs::lightning::set_field`).
    LightningSet { entity_id: u64, field: String, value: serde_json::Value },
    /// `Flow.set(e, field, value)` (`ecs::flow::set_field`).
    FlowSet { entity_id: u64, field: String, value: serde_json::Value },
    /// `NavLink.set(e, field, value)` (`ecs::nav_links::set_field`).
    NavLinkSet { entity_id: u64, field: String, value: serde_json::Value },
    /// `Morph.to(e, target, seconds)` (`ecs::path_morph::morph_to`).
    MorphTo { entity_id: u64, target: String, seconds: f32 },
    /// `Morph.set(e, field, value)` (`ecs::path_morph::set_field`).
    MorphSet { entity_id: u64, field: String, value: serde_json::Value },
    /// `Toast.show(e, title, text, icon, seconds)` (`ui::toast::show`).
    ToastShow { entity_id: u64, title: String, text: String, icon: String, seconds: f32 },
    /// `Toast.set(e, field, value)` (`ui::toast::set_field`).
    ToastSet { entity_id: u64, field: String, value: serde_json::Value },
    /// `Snow.set(e, field, value)` (`ecs::snow::set_field`).
    SnowSet { entity_id: u64, field: String, value: serde_json::Value },
    /// `FogOfWar.reset(e)`: the map unseen again (`ecs::fog_of_war::reset_fog`).
    FogOfWarReset { entity_id: u64 },
    /// `FogOfWar.set(e, field, value)` (`ecs::fog_of_war::set_fog_of_war_field`).
    FogOfWarSet { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `water_surface` field by name (`ecs::water::set_water_field`).
    SetWaterField { entity_id: u64, field: String, value: serde_json::Value },
    /// Push a water's surface at world `x` (`ecs::water::splash_world`).
    WaterSplash { entity_id: u64, x: f32, strength: f32 },
    /// Write any `canvas_group` field by name (`ecs::canvas_group::set_canvas_group_field`).
    SetCanvasGroupField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `video_player` field by name (`video::set_video_field`).
    SetVideoField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `ragdoll` field by name (`ecs::ragdoll::set_ragdoll_field`).
    SetRagdollField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any `timeline` field by name (`ecs::timeline::set_timeline_field`).
    SetTimelineField { entity_id: u64, field: String, value: serde_json::Value },
    /// `BehaviorTree.set` (`ecs::behavior_tree::set_behavior_tree_field`).
    SetBehaviorTreeField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any area-effect field by name (`ecs::area_effect::set_area_field`).
    SetAreaField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any camera field by name (`ecs::camera_rig::set_camera_field`).
    SetCameraField { entity_id: u64, field: String, value: serde_json::Value },
    /// Write any camera-follow field by name (`components::set_follow_field`).
    SetCameraFollow { entity_id: u64, field: String, value: serde_json::Value },
    /// The part of the screen a camera draws onto (`screen_rect`; all zero
    /// for none).
    SetCameraScreenRect { entity_id: u64, rect: [f32; 4] },
    /// Where a `nav_agent` walks to; `None` stops it (`navigation::set_nav_target`).
    SetNavTarget { entity_id: u64, target: Option<[f32; 2]> },
    /// Write any nav agent or region field by name (`navigation::set_nav_field`).
    SetNavField { entity_id: u64, field: String, value: serde_json::Value },
    // Phase 4E: UI
    SetUIText { entity_id: u64, text: String },
    /// Set one property on an entity-UI widget (progress bar value, slider
    /// position, toggle state, text-input contents, ...).
    SetUiProp { entity_id: u64, property: String, value: serde_json::Value },
    /// Update `text` on a `TextComponent` (with `UIText` fallback).
    SetText { entity_id: u64, text: String },
    SetUIVisible { entity_id: u64, visible: bool },
    SetUIPosition { entity_id: u64, anchor: crate::ui::UIAnchor, offset_x: f32, offset_y: f32 },
    CreateUIText {
        name: String,
        text: String,
        anchor: crate::ui::UIAnchor,
        offset_x: f32,
        offset_y: f32,
        color: [f32; 4],
        font_size: f32,
        alignment: crate::ui::TextAlign,
    },
    CreateUIPanel {
        name: String,
        anchor: crate::ui::UIAnchor,
        offset_x: f32,
        offset_y: f32,
        width: f32,
        height: f32,
        bg_color: [f32; 4],
    },
    // Phase 4E-GUI: GUI Document API
    GuiShow { gui_id: String },
    GuiHide { gui_id: String },
    GuiSetNodeProp { gui_id: String, node_id: String, property: String, value: serde_json::Value },
    GuiFocus { gui_id: String, node_id: String },
    // Phase 4B: Physics
    ApplyForce { entity_id: u64, fx: f32, fy: f32 },
    ApplyImpulse { entity_id: u64, ix: f32, iy: f32 },
    SetVelocity { entity_id: u64, vx: f32, vy: f32 },
    SetAngularVelocity { entity_id: u64, omega: f32 },
    SetGravity { gx: f32, gy: f32 },
    SetBodyType { entity_id: u64, body_type: String },
    AddJoint { entity_name: String, target_name: String, joint_json: String },
    RemoveJoint { entity_name: String },
    SetMotorSpeed { entity_name: String, speed: f32 },
    // Phase 4A: Audio
    /// `sound_id` is the id the script was handed (0 when none was), which
    /// the host aliases to the backend's (`audio::music::alias`).
    PlaySound { clip_id: String, volume: f32, pitch: f32, pan: f32, sound_id: u64, looping: bool },
    PlayMusic { clip_id: String, volume: f32, looping: bool },
    StopSound { handle_id: u64 },
    /// Stop every playing instance of one clip, by asset path — the usable
    /// form of per-sound stop, since `play_sound` is deferred and returns no
    /// handle.
    StopSoundByClip { clip_id: String },
    StopMusic,
    PauseMusic,
    ResumeMusic,
    SetSoundVolume { handle_id: u64, volume: f32 },
    SetMusicVolume { volume: f32 },
    SetMasterVolume { volume: f32 },
    StopAllSounds,
    PauseAllSounds,
    ResumeAllSounds,
    // Material system
    SetMaterial { entity_id: u64, material_id: String },
    SetNormalMap { entity_id: u64, normal_map_id: Option<String> },
    SetMaterialUniform { entity_id: u64, name: String, value: crate::renderer::material::UniformValue },
    // Post-processing
    SetPostProcessEnabled { effect_name: String, enabled: bool },
    SetPostProcessParam { effect_name: String, param_name: String, value: crate::renderer::material::UniformValue },
    // 2D Lighting
    SetLightColor { entity_id: u64, r: f32, g: f32, b: f32 },
    SetLightIntensity { entity_id: u64, intensity: f32 },
    /// How high a light stands: the tangent of its elevation. For a sun it
    /// also sets how long shadows are.
    SetLightHeight { entity_id: u64, height: f32 },
    /// The light layers a light lights (`light_mask`, Oct 2026).
    SetLightMask { entity_id: u64, mask: u32 },
    /// The frame's colour grade (`color_lut`, Oct 2026): its strip and
    /// strength; an empty id removes it.
    SetColorLut { texture_id: String, strength: f32 },
    SetLightRadius { entity_id: u64, radius: f32 },
    SetAmbientLight { r: f32, g: f32, b: f32, intensity: f32 },
    /// The scene's toon lighting; `bands <= 0` removes it.
    SetToonLighting { bands: f32, softness: Option<f32>, dither: Option<f32> },
    /// The scene's ambient occlusion; a `strength` of none removes it.
    SetAmbientOcclusion { radius: Option<f32>, strength: Option<f32> },
    /// The scene's global illumination; a `strength` of 0 removes it.
    SetGlobalIllumination { strength: Option<f32>, scale: Option<f32> },
}

impl Default for ScriptState {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptState {
    pub fn new() -> Self {
        Self {
            snapshot: ScriptSnapshot::default(),
            input: ScriptInput::default(),
            services: ScriptServices::default(),
            commands: Vec::new(),
        }
    }

    /// Reset commands for a new frame.
    pub fn clear(&mut self) {
        self.commands.clear();
        self.services.spawned_entities.clear();
        self.services.pending_events.clear();
        self.services.debug_draws.clear();
        self.services.fired_timers.clear();
        self.services.fired_gui_clicks.clear();
    }

    // -- Functions callable from scripts --

    pub fn set_position(&mut self, entity_id: u64, x: f32, y: f32) {
        if x.is_nan() || y.is_nan() {
            log::warn!("set_position(entity {entity_id}) received NaN — check for nil/undefined arithmetic in your script");
            return;
        }
        self.commands.push(ScriptCommand::SetPosition {
            entity_id,
            x,
            y,
        });
    }

    pub fn set_rotation(&mut self, entity_id: u64, radians: f32) {
        self.commands
            .push(ScriptCommand::SetRotation { entity_id, radians });
    }

    pub fn set_scale(&mut self, entity_id: u64, x: f32, y: f32) {
        self.commands
            .push(ScriptCommand::SetScale { entity_id, x, y });
    }

    pub fn set_sprite_color(&mut self, entity_id: u64, r: f32, g: f32, b: f32, a: f32) {
        self.commands.push(ScriptCommand::SetSpriteColor {
            entity_id,
            r,
            g,
            b,
            a,
        });
    }

    pub fn set_z_order(&mut self, entity_id: u64, z: i32) {
        self.commands.push(ScriptCommand::SetZOrder { entity_id, z });
    }

    pub fn set_text_color(&mut self, entity_id: u64, r: f32, g: f32, b: f32, a: f32) {
        self.commands.push(ScriptCommand::SetTextColor { entity_id, r, g, b, a });
    }

    pub fn set_trail_emitting(&mut self, entity_id: u64, on: bool) {
        self.commands.push(ScriptCommand::SetTrailEmitting { entity_id, on });
    }

    pub fn set_mesh(&mut self, entity_id: u64, vertices: serde_json::Value, indices: serde_json::Value) {
        self.commands.push(ScriptCommand::SetMesh { entity_id, vertices, indices });
    }

    pub fn clear_trail(&mut self, entity_id: u64) {
        self.commands.push(ScriptCommand::ClearTrail { entity_id });
    }

    pub fn carve_sprite(&mut self, entity_id: u64, x: f32, y: f32, radius: f32) {
        self.commands.push(ScriptCommand::CarveSprite { entity_id, x, y, radius });
    }

    /// `Sprite.paint` (`ecs::paint`).
    #[allow(clippy::too_many_arguments)]
    pub fn paint_sprite(&mut self, entity_id: u64, x: f32, y: f32, size: f32, color: [f32; 4], shape: String, rotation: f32) {
        self.commands.push(ScriptCommand::PaintSprite { entity_id, x, y, size, color, shape, rotation, depth: 0.0, blend: String::new() });
    }

    /// `Sprite.press`: a dent (or, with a negative depth, a ridge) in a
    /// paintable sprite's normal map.
    pub fn press_sprite(&mut self, entity_id: u64, x: f32, y: f32, size: f32, depth: f32, shape: String, rotation: f32) {
        self.commands.push(ScriptCommand::PaintSprite { entity_id, x, y, size, color: [1.0; 4], shape, rotation, depth, blend: String::new() });
    }

    pub fn move_character(&mut self, entity_id: u64, vx: f32, vy: f32, drop_through: Option<bool>) {
        self.commands.push(ScriptCommand::MoveCharacter { entity_id, vx, vy, drop_through });
    }

    pub fn set_sprite_size(&mut self, entity_id: u64, w: f32, h: f32) {
        self.commands.push(ScriptCommand::SetSpriteSize { entity_id, w, h });
    }

    pub fn set_sprite_anchor(&mut self, entity_id: u64, x: f32, y: f32) {
        self.commands
            .push(ScriptCommand::SetSpriteAnchor { entity_id, x, y });
    }

    pub fn set_sprite_region(
        &mut self,
        entity_id: u64,
        atlas_id: String,
        region_name: String,
    ) {
        self.commands.push(ScriptCommand::SetSpriteRegion {
            entity_id,
            atlas_id,
            region_name,
        });
    }

    pub fn set_sprite_corners(&mut self, entity_id: u64, corners: Option<[[f32; 2]; 4]>) {
        self.commands
            .push(ScriptCommand::SetSpriteCorners { entity_id, corners });
    }

    pub fn set_material(&mut self, entity_id: u64, material_id: String) {
        self.commands
            .push(ScriptCommand::SetMaterial { entity_id, material_id });
    }

    pub fn set_normal_map(&mut self, entity_id: u64, normal_map_id: Option<String>) {
        self.commands
            .push(ScriptCommand::SetNormalMap { entity_id, normal_map_id });
    }

    pub fn set_material_uniform(
        &mut self,
        entity_id: u64,
        name: String,
        value: crate::renderer::material::UniformValue,
    ) {
        self.commands
            .push(ScriptCommand::SetMaterialUniform { entity_id, name, value });
    }

    pub fn set_post_process_enabled(&mut self, effect_name: String, enabled: bool) {
        self.commands
            .push(ScriptCommand::SetPostProcessEnabled { effect_name, enabled });
    }

    pub fn set_post_process_param(
        &mut self,
        effect_name: String,
        param_name: String,
        value: crate::renderer::material::UniformValue,
    ) {
        self.commands
            .push(ScriptCommand::SetPostProcessParam { effect_name, param_name, value });
    }

    pub fn set_light_color(&mut self, entity_id: u64, r: f32, g: f32, b: f32) {
        self.commands
            .push(ScriptCommand::SetLightColor { entity_id, r, g, b });
    }

    pub fn set_color_lut(&mut self, texture_id: String, strength: f32) {
        self.commands.push(ScriptCommand::SetColorLut { texture_id, strength });
    }

    pub fn set_light_mask(&mut self, entity_id: u64, mask: u32) {
        self.commands.push(ScriptCommand::SetLightMask { entity_id, mask });
    }

    pub fn set_light_height(&mut self, entity_id: u64, height: f32) {
        self.commands.push(ScriptCommand::SetLightHeight { entity_id, height });
    }

    pub fn set_light_intensity(&mut self, entity_id: u64, intensity: f32) {
        self.commands
            .push(ScriptCommand::SetLightIntensity { entity_id, intensity });
    }

    pub fn set_light_radius(&mut self, entity_id: u64, radius: f32) {
        self.commands
            .push(ScriptCommand::SetLightRadius { entity_id, radius });
    }

    pub fn set_ambient_light(&mut self, r: f32, g: f32, b: f32, intensity: f32) {
        self.commands
            .push(ScriptCommand::SetAmbientLight { r, g, b, intensity });
    }

    pub fn set_toon_lighting(&mut self, bands: f32, softness: Option<f32>, dither: Option<f32>) {
        self.commands.push(ScriptCommand::SetToonLighting { bands, softness, dither });
    }

    pub fn set_ambient_occlusion(&mut self, radius: Option<f32>, strength: Option<f32>) {
        self.commands.push(ScriptCommand::SetAmbientOcclusion { radius, strength });
    }

    pub fn set_global_illumination(&mut self, strength: Option<f32>, scale: Option<f32>) {
        self.commands.push(ScriptCommand::SetGlobalIllumination { strength, scale });
    }

    pub fn destroy_entity(&mut self, entity_id: u64) {
        self.commands
            .push(ScriptCommand::DestroyEntity { entity_id });
    }

    pub fn log(&mut self, message: String) {
        self.commands.push(ScriptCommand::Log { message });
    }

    /// Queue a set-custom-field command (applied after all scripts run).
    pub fn set_custom_field(
        &mut self,
        entity_id: u64,
        component_name: String,
        field_name: String,
        value: serde_json::Value,
    ) {
        self.commands.push(ScriptCommand::SetCustomField {
            entity_id,
            component_name,
            field_name,
            value,
        });
    }

    // -- Phase 3B: Entity Spawning --

    /// Spawn a new empty entity with just a Name + default Transform.
    /// Returns a pre-allocated entity ID that can be used immediately.
    pub fn spawn_entity(&mut self, name: String) -> u64 {
        let id = self.services.next_spawn_id;
        self.services.next_spawn_id += 1;
        self.services.spawned_entities.push((name.clone(), id));
        self.commands.push(ScriptCommand::SpawnEntity {
            name,
            components: Box::new(None),
            spawn_id: Some(id),
            children: Vec::new(),
        });
        id
    }

    /// Spawn an entity from a loaded prefab template.
    /// Returns a pre-allocated entity ID, or 0 if the prefab is not found.
    pub fn spawn_prefab(&mut self, prefab_name: &str) -> u64 {
        if let Some(prefab) = self.services.prefabs.get(prefab_name) {
            let id = self.services.next_spawn_id;
            self.services.next_spawn_id += 1;
            self.services.spawned_entities.push((prefab.name.clone(), id));
            self.commands.push(ScriptCommand::SpawnEntity {
                name: prefab.name.clone(),
                components: Box::new(Some(prefab.components.clone())),
                spawn_id: Some(id),
                children: prefab.children.clone(),
            });
            id
        } else {
            log::warn!("Prefab '{}' not found", prefab_name);
            0
        }
    }

    // -- Phase 3C: Event Bus --

    /// Fire a named event from the current entity.
    pub fn send_event(&mut self, name: String, sender: u64, data: Option<serde_json::Value>) {
        self.commands.push(ScriptCommand::SendEvent {
            name,
            sender,
            data,
        });
    }

    // -- Phase 3D: Scene Transitions --

    /// Load a new scene (replace current).
    pub fn load_scene(&mut self, path: String) {
        self.commands.push(ScriptCommand::LoadScene { path });
    }

    /// Request application shutdown. Honoured by whichever runtime is hosting
    /// the script; there is nothing to close in a browser tab.
    pub fn quit_game(&mut self) {
        self.commands.push(ScriptCommand::QuitGame);
    }

    /// Scale the delta time driving the world. 1.0 is normal, 0.0 pauses
    /// everything, values between give slow motion.
    pub fn set_time_scale(&mut self, scale: f32) {
        self.commands.push(ScriptCommand::SetTimeScale { scale });
    }

    /// Push current scene onto stack and load a new one (~for pause menus).
    pub fn push_scene(&mut self, path: String) {
        self.commands.push(ScriptCommand::PushScene { path });
    }

    /// Pop the scene stack and return to the previous scene.
    pub fn pop_scene(&mut self) {
        self.commands.push(ScriptCommand::PopScene);
    }

    /// Slice 12.A — fade overlay (opaque → clear) over `duration_s` seconds.
    /// `color` is the starting RGB (0..=1); defaults to black at the call sites.
    /// Typically called from `on_ready` for a polish-reveal effect.
    pub fn fade_in(&mut self, duration_s: f32, color: [f32; 3]) {
        self.commands.push(ScriptCommand::FadeIn { duration_s, color });
    }

    /// Slice 12.A — fade overlay (clear → opaque) over `duration_s` seconds.
    /// Useful for covering an upcoming scene swap; the next scene should
    /// queue a `fade_in` of the same color from its `on_ready`.
    pub fn fade_out(&mut self, duration_s: f32, color: [f32; 3]) {
        self.commands.push(ScriptCommand::FadeOut { duration_s, color });
    }

    /// P1.3 — Fullscreen screen-flash. `color.a` is the *initial* opacity
    /// (clamped to 0..=1) and decays linearly to clear over `duration_s`.
    /// Distinct from [`Self::fade_in`] which always starts at full opacity.
    pub fn screen_flash(&mut self, duration_s: f32, color: [f32; 4]) {
        self.commands
            .push(ScriptCommand::ScreenFlash { duration_s, color });
    }

    // -- Phase 3F: Tags --

    /// Queue an add-tag command.
    pub fn add_tag(&mut self, entity_id: u64, tag: String) {
        self.commands.push(ScriptCommand::AddTag { entity_id, tag });
    }

    /// Queue a remove-tag command.
    pub fn remove_tag(&mut self, entity_id: u64, tag: String) {
        self.commands
            .push(ScriptCommand::RemoveTag { entity_id, tag });
    }

    // -- Phase 4G: Profiling --

    /// Log all frame stats to the console.
    pub fn log_stats(&mut self) {
        let s = &self.snapshot.frame_stats;
        let msg = format!(
            "FPS: {:.1} | Frame: {:.2}ms | Update: {:.2}ms | Render: {:.2}ms | Draw calls: {} | Sprites: {} | Entities: {}",
            s.fps, s.frame_time_ms, s.update_time_ms, s.render_time_ms,
            s.draw_calls, s.sprite_count, s.entity_count,
        );
        self.commands.push(ScriptCommand::Log { message: msg });
    }

    // -- Phase 4C: Animation --

    /// Play a named animation clip on an entity.
    pub fn play_animation(&mut self, entity_id: u64, clip_name: String) {
        self.commands.push(ScriptCommand::PlayAnimation {
            entity_id,
            clip_name,
        });
    }

    /// Stop the current animation on an entity.
    pub fn stop_animation(&mut self, entity_id: u64) {
        self.commands
            .push(ScriptCommand::StopAnimation { entity_id });
    }

    /// Set the playback speed multiplier for an entity's animator.
    pub fn set_animation_speed(&mut self, entity_id: u64, speed: f32) {
        self.commands.push(ScriptCommand::SetAnimationSpeed {
            entity_id,
            speed,
        });
    }

    /// Set a boolean parameter on an entity's animation state machine.
    pub fn set_anim_bool(&mut self, entity_id: u64, param: String, value: bool) {
        self.commands.push(ScriptCommand::SetAnimBool {
            entity_id,
            param,
            value,
        });
    }

    /// Set a float parameter on an entity's animation state machine.
    pub fn set_anim_float(&mut self, entity_id: u64, param: String, value: f32) {
        self.commands.push(ScriptCommand::SetAnimFloat {
            entity_id,
            param,
            value,
        });
    }

    /// Set a one-shot trigger on an entity's animation state machine.
    pub fn set_anim_trigger(&mut self, entity_id: u64, param: String) {
        self.commands.push(ScriptCommand::SetAnimTrigger {
            entity_id,
            param,
        });
    }

    /// Start the animation state machine at a named state.
    pub fn start_state_machine(&mut self, entity_id: u64, state_name: String) {
        self.commands.push(ScriptCommand::StartStateMachine {
            entity_id,
            state_name,
        });
    }

    /// Start a position tween. Returns a tween ID.
    pub fn tween_position(
        &mut self,
        entity_id: u64,
        x: f32,
        y: f32,
        duration: f32,
        easing: EasingFunction,
    ) -> u32 {
        // Get current position as start value
        let (sx, sy) = self.snapshot.get_position(entity_id).unwrap_or((0.0, 0.0));
        self.services.tween_manager.add_tween(
            entity_id,
            TweenTarget::Position,
            glam::Vec4::new(sx, sy, 0.0, 0.0),
            glam::Vec4::new(x, y, 0.0, 0.0),
            duration,
            easing,
        )
    }

    /// Start a rotation tween. Returns a tween ID.
    pub fn tween_rotation(
        &mut self,
        entity_id: u64,
        radians: f32,
        duration: f32,
        easing: EasingFunction,
    ) -> u32 {
        let start = self.snapshot.get_rotation(entity_id).unwrap_or(0.0);
        self.services.tween_manager.add_tween(
            entity_id,
            TweenTarget::Rotation,
            glam::Vec4::new(start, 0.0, 0.0, 0.0),
            glam::Vec4::new(radians, 0.0, 0.0, 0.0),
            duration,
            easing,
        )
    }

    /// Start a scale tween. Returns a tween ID.
    pub fn tween_scale(
        &mut self,
        entity_id: u64,
        sx: f32,
        sy: f32,
        duration: f32,
        easing: EasingFunction,
    ) -> u32 {
        let (cur_sx, cur_sy) = self.snapshot.get_scale(entity_id).unwrap_or((1.0, 1.0));
        self.services.tween_manager.add_tween(
            entity_id,
            TweenTarget::Scale,
            glam::Vec4::new(cur_sx, cur_sy, 0.0, 0.0),
            glam::Vec4::new(sx, sy, 0.0, 0.0),
            duration,
            easing,
        )
    }

    /// Start a sprite color tween. Returns a tween ID.
    #[allow(clippy::too_many_arguments)]
    pub fn tween_color(
        &mut self,
        entity_id: u64,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
        duration: f32,
        easing: EasingFunction,
    ) -> u32 {
        let cur = self.snapshot.get_sprite_color(entity_id).unwrap_or([1.0, 1.0, 1.0, 1.0]);
        self.services.tween_manager.add_tween(
            entity_id,
            TweenTarget::SpriteColor,
            glam::Vec4::from(cur),
            glam::Vec4::new(r, g, b, a),
            duration,
            easing,
        )
    }

    // -- Phase 4D: Tilemap API --

    /// Queue a SetTile command.
    pub fn set_tile(
        &mut self,
        entity_id: u64,
        layer_name: String,
        x: u32,
        y: u32,
        tile_id: Option<u32>,
    ) {
        self.commands.push(ScriptCommand::SetTile {
            entity_id,
            layer_name,
            x,
            y,
            tile_id,
        });
    }

    // -- Phase 4F: Particle API --

    /// Emit a burst of particles from an entity's emitter.
    pub fn emit_burst(&mut self, entity_id: u64, count: u32) {
        self.commands.push(ScriptCommand::EmitBurst { entity_id, count });
    }

    /// Set whether an emitter is actively spawning.
    pub fn set_emitter_active(&mut self, entity_id: u64, active: bool) {
        self.commands.push(ScriptCommand::SetEmitterActive { entity_id, active });
    }

    /// Set emission rate (particles/sec) on an emitter.
    pub fn set_emitter_rate(&mut self, entity_id: u64, rate: f32) {
        self.commands.push(ScriptCommand::SetEmitterRate { entity_id, rate });
    }

    /// Write any emitter field by name (`particles::set_emitter_field`).
    pub fn set_emitter_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetEmitterField { entity_id, field, value });
    }

    /// Write any cloth or rope field by name (`ecs::cloth::set_cloth_field`).
    pub fn set_cloth_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetClothField { entity_id, field, value });
    }

    /// Write any `path_follow` field by name (`ecs::path_follow::set_path_follow_field`).
    pub fn set_path_follow_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetPathFollowField { entity_id, field, value });
    }

    /// Write any `trail` field by name (`ecs::trails::set_trail_field`).
    pub fn set_trail_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetTrailField { entity_id, field, value });
    }

    /// Write any `line` field by name (`renderer::line2d::set_line_field`).
    pub fn set_line_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetLineField { entity_id, field, value });
    }

    /// Smoke and a push at a world point.
    #[allow(clippy::too_many_arguments)]
    pub fn smoke_puff(&mut self, entity_id: u64, x: f32, y: f32, radius: f32, amount: f32, vx: f32, vy: f32) {
        self.commands.push(ScriptCommand::SmokePuff { entity_id, x, y, radius, amount, vx, vy });
    }

    /// Dig or build a smooth terrain round a world point.
    pub fn smooth_terrain_edit(&mut self, entity_id: u64, x: f32, y: f32, radius: f32, amount: f32) {
        self.commands.push(ScriptCommand::TerrainEdit { entity_id, x, y, radius, amount });
    }

    /// Drive a conversation: `op` is `start` (`arg` a node), `advance`,
    /// `choose` (`arg` the number) or `stop`.
    pub fn dialogue_op(&mut self, entity_id: u64, op: &str, arg: &str) {
        self.commands.push(ScriptCommand::DialogueOp { entity_id, op: op.to_string(), arg: arg.to_string() });
    }

    /// Put a draggable item in the slot named `slot`, as a drop would.
    pub fn ui_drop(&mut self, entity_id: u64, slot: &str) {
        self.commands.push(ScriptCommand::UiDrop { entity_id, slot: slot.to_string() });
    }

    /// Stand a Mode 7 sprite (or a plane's camera) at a ground point.
    pub fn mode7_place(&mut self, entity_id: u64, x: f32, y: f32, angle: f32) {
        self.commands.push(ScriptCommand::Mode7Place { entity_id, x, y, angle });
    }

    /// Any field of a `mode7_sprite`, else of a `mode7`, by name.
    pub fn mode7_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::Mode7Set { entity_id, field, value });
    }

    /// Any `sprite_tilt` field by name.
    pub fn sprite_tilt_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SpriteTiltSet { entity_id, field, value });
    }

    /// Any `afterimage` field by name.
    pub fn afterimage_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::AfterimageSet { entity_id, field, value });
    }

    /// Cut every sliceable the segment crosses.
    pub fn physics_slice(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        self.commands.push(ScriptCommand::PhysicsSlice { x0, y0, x1, y1 });
    }

    /// Any `portal` field by name.
    pub fn portal_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::PortalSet { entity_id, field, value });
    }

    /// Push a jelly's points near a world point in.
    pub fn jelly_poke(&mut self, entity_id: u64, x: f32, y: f32, strength: f32, radius: f32) {
        self.commands.push(ScriptCommand::JellyPoke { entity_id, x, y, strength, radius });
    }

    /// Any `jelly` field by name.
    pub fn jelly_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::JellySet { entity_id, field, value });
    }

    /// Any `offscreen_marker` field by name.
    pub fn marker_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::MarkerSet { entity_id, field, value });
    }

    /// Any `ui_radial_menu` field by name.
    pub fn radial_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::RadialSet { entity_id, field, value });
    }

    /// Say `text` in a bubble for `seconds`.
    pub fn bubble_say(&mut self, entity_id: u64, text: String, seconds: f32) {
        self.commands.push(ScriptCommand::BubbleSay { entity_id, text, seconds });
    }

    /// Any `speech_bubble` field by name.
    pub fn bubble_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::BubbleSet { entity_id, field, value });
    }

    /// Any `sprite_outline` field by name.
    pub fn outline_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::OutlineSet { entity_id, field, value });
    }

    /// Strike now, landing at `(x, y)`.
    pub fn lightning_strike(&mut self, entity_id: u64, x: f32, y: f32) {
        self.commands.push(ScriptCommand::LightningStrike { entity_id, x, y });
    }

    /// Any `lightning` field by name.
    pub fn lightning_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::LightningSet { entity_id, field, value });
    }

    /// Any `flow_path` field by name.
    pub fn flow_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::FlowSet { entity_id, field, value });
    }

    /// Any `nav_link` field by name.
    pub fn nav_link_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::NavLinkSet { entity_id, field, value });
    }

    /// Turn the entity's path into another entity's shape over `seconds`.
    pub fn morph_to(&mut self, entity_id: u64, target: String, seconds: f32) {
        self.commands.push(ScriptCommand::MorphTo { entity_id, target, seconds });
    }

    /// Any `path_morph` field by name.
    pub fn morph_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::MorphSet { entity_id, field, value });
    }

    /// Queue a toast on the stack.
    pub fn toast_show(&mut self, entity_id: u64, title: String, text: String, icon: String, seconds: f32) {
        self.commands.push(ScriptCommand::ToastShow { entity_id, title, text, icon, seconds });
    }

    /// Any `toast_stack` field by name.
    pub fn toast_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::ToastSet { entity_id, field, value });
    }

    /// Any `snow_cover` field by name.
    pub fn snow_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SnowSet { entity_id, field, value });
    }

    /// `FogOfWar.reset(e)`: every cell unseen and unexplored again.
    pub fn fog_of_war_reset(&mut self, entity_id: u64) {
        self.commands.push(ScriptCommand::FogOfWarReset { entity_id });
    }

    /// `FogOfWar.set(e, field, value)`: any `fog_of_war` field by name.
    pub fn fog_of_war_set(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::FogOfWarSet { entity_id, field, value });
    }

    /// Any beat_clock field by name.
    pub fn set_beat_clock_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetBeatClockField { entity_id, field, value });
    }

    /// Any text_path field by name.
    pub fn set_text_path_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetTextPathField { entity_id, field, value });
    }

    /// Any trajectory field by name.
    pub fn set_trajectory_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetTrajectoryField { entity_id, field, value });
    }

    /// Any tile_light / tile_light_source field by name.
    pub fn set_tile_light_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetTileLightField { entity_id, field, value });
    }

    /// Any auto_normal field by name.
    pub fn set_auto_normal_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetAutoNormalField { entity_id, field, value });
    }

    /// Any cast_shadow field by name.
    pub fn set_cast_shadow_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetCastShadowField { entity_id, field, value });
    }

    /// An entity's own pace.
    pub fn set_time_scale_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetTimeScaleField { entity_id, field, value });
    }

    /// A floating_origin field by name.
    pub fn set_origin_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetOriginField { entity_id, field, value });
    }

    /// Any day_cycle field by name.
    pub fn set_day_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetDayField { entity_id, field, value });
    }

    /// Any vision field by name.
    pub fn set_vision_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetVisionField { entity_id, field, value });
    }

    /// A region of a wfc generator's layer to fill.
    pub fn wfc_generate(&mut self, entity_id: u64, x: i32, y: i32, w: i32, h: i32) {
        self.commands.push(ScriptCommand::WfcGenerate { entity_id, x, y, w, h });
    }

    /// Any fog field by name.
    pub fn set_fog_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetFogField { entity_id, field, value });
    }

    /// A rewind clock's mode or speed.
    pub fn set_rewind_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetRewindField { entity_id, field, value });
    }

    /// Any smoke field by name.
    pub fn set_smoke_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetSmokeField { entity_id, field, value });
    }

    /// Paint a material into a pixel simulation.
    pub fn pixel_paint(&mut self, entity_id: u64, x: f32, y: f32, radius: f32, material: String) {
        self.commands.push(ScriptCommand::PixelPaint { entity_id, x, y, radius, material });
    }

    /// How much of a bar or radial fill is drawn.
    pub fn set_sprite_fill(&mut self, entity_id: u64, amount: f32) {
        self.commands.push(ScriptCommand::SetSpriteFill { entity_id, amount });
    }

    /// Write any `water_surface` field by name.
    pub fn set_water_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetWaterField { entity_id, field, value });
    }

    /// Push a water's surface at world `x`.
    pub fn water_splash(&mut self, entity_id: u64, x: f32, strength: f32) {
        self.commands.push(ScriptCommand::WaterSplash { entity_id, x, strength });
    }

    /// Write any `mirror` field by name (`ecs::mirror::set_mirror_field`).
    pub fn set_mirror_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetMirrorField { entity_id, field, value });
    }

    /// Write any `fluid` field by name (`ecs::fluid::set_fluid_field`).
    pub fn set_fluid_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetFluidField { entity_id, field, value });
    }

    /// Write any `canvas_group` field by name (`ecs::canvas_group::set_canvas_group_field`).
    pub fn set_canvas_group_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetCanvasGroupField { entity_id, field, value });
    }

    /// Write any `video_player` field by name (`video::set_video_field`).
    pub fn set_video_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetVideoField { entity_id, field, value });
    }

    /// Write any `ragdoll` field by name (`ecs::ragdoll::set_ragdoll_field`).
    pub fn set_ragdoll_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetRagdollField { entity_id, field, value });
    }

    /// Write any `timeline` field by name (`ecs::timeline::set_timeline_field`).
    pub fn set_timeline_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetTimelineField { entity_id, field, value });
    }

    /// `BehaviorTree.set` (`ecs::behavior_tree::set_behavior_tree_field`).
    pub fn set_behavior_tree_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetBehaviorTreeField { entity_id, field, value });
    }

    /// Write any area-effect field by name (`ecs::area_effect::set_area_field`).
    pub fn set_area_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetAreaField { entity_id, field, value });
    }

    /// Write any camera field by name.
    pub fn set_camera_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetCameraField { entity_id, field, value });
    }

    /// Write any camera-follow field by name.
    pub fn set_camera_follow(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetCameraFollow { entity_id, field, value });
    }

    /// The part of the screen a camera draws onto (`screen_rect`).
    pub fn set_camera_screen_rect(&mut self, entity_id: u64, rect: [f32; 4]) {
        self.commands.push(ScriptCommand::SetCameraScreenRect { entity_id, rect });
    }

    /// Where a `nav_agent` walks to; `None` stops it.
    pub fn set_nav_target(&mut self, entity_id: u64, target: Option<[f32; 2]>) {
        self.commands.push(ScriptCommand::SetNavTarget { entity_id, target });
    }

    /// Write any nav agent or region field by name.
    pub fn set_nav_field(&mut self, entity_id: u64, field: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::SetNavField { entity_id, field, value });
    }

    // -- Phase 4E: UI API --

    /// Create a UI text entity from scripts. Returns a pre-allocated entity ID.
    /// `color` defaults to white, `font_size` to 16.0, `alignment` to Left when
    /// scripts omit them (handled by the bridge adapters).
    #[allow(clippy::too_many_arguments)]
    pub fn create_ui_text(
        &mut self,
        text: String,
        anchor: crate::ui::UIAnchor,
        offset_x: f32,
        offset_y: f32,
        color: [f32; 4],
        font_size: f32,
        alignment: crate::ui::TextAlign,
    ) -> u64 {
        let id = self.services.next_spawn_id;
        self.services.next_spawn_id += 1;
        let name = format!("UIText_{}", id);
        self.services.spawned_entities.push((name.clone(), id));
        self.commands.push(ScriptCommand::CreateUIText {
            name,
            text,
            anchor,
            offset_x,
            offset_y,
            color,
            font_size,
            alignment,
        });
        id
    }

    /// Create a UI panel entity from scripts. Returns a pre-allocated entity ID.
    #[allow(clippy::too_many_arguments)]
    pub fn create_ui_panel(
        &mut self,
        anchor: crate::ui::UIAnchor,
        offset_x: f32,
        offset_y: f32,
        width: f32,
        height: f32,
        bg_color: [f32; 4],
    ) -> u64 {
        let id = self.services.next_spawn_id;
        self.services.next_spawn_id += 1;
        let name = format!("UIPanel_{}", id);
        self.services.spawned_entities.push((name.clone(), id));
        self.commands.push(ScriptCommand::CreateUIPanel {
            name,
            anchor,
            offset_x,
            offset_y,
            width,
            height,
            bg_color,
        });
        id
    }

    /// Update the text content of a UIText entity.
    pub fn set_ui_text(&mut self, entity_id: u64, text: String) {
        self.commands
            .push(ScriptCommand::SetUIText { entity_id, text });
    }

    /// Update the `text` field on a `TextComponent` entity (in-scene text).
    /// Falls back to `UIText` if the entity has no `TextComponent`.
    pub fn set_text(&mut self, entity_id: u64, text: String) {
        self.commands
            .push(ScriptCommand::SetText { entity_id, text });
    }

    /// Show or hide a UI element.
    pub fn set_ui_visible(&mut self, entity_id: u64, visible: bool) {
        self.commands
            .push(ScriptCommand::SetUIVisible { entity_id, visible });
    }

    /// Reposition a UI element.
    pub fn set_ui_position(
        &mut self,
        entity_id: u64,
        anchor: crate::ui::UIAnchor,
        offset_x: f32,
        offset_y: f32,
    ) {
        self.commands.push(ScriptCommand::SetUIPosition {
            entity_id,
            anchor,
            offset_x,
            offset_y,
        });
    }

    // -- Phase 4E-GUI: GUI Document API --

    /// Load and display a .gui asset by attaching a GuiComponent to the calling entity.
    pub fn gui_load(&mut self, _entity_id: u64, gui_id: String) {
        // Spawn a GuiComponent on the entity via SetCustomField won't work since
        // GuiComponent is a distinct type. Use a dedicated command pattern:
        // We queue a GuiShow which will also handle creation if missing.
        self.commands.push(ScriptCommand::GuiShow { gui_id });
    }

    /// Show a GUI (set visible=true on the entity that owns this gui_id).
    pub fn gui_show(&mut self, gui_id: String) {
        self.commands.push(ScriptCommand::GuiShow { gui_id });
    }

    /// Hide a GUI (set visible=false on the entity that owns this gui_id).
    pub fn gui_hide(&mut self, gui_id: String) {
        self.commands.push(ScriptCommand::GuiHide { gui_id });
    }

    /// Set a property on a GUI node. Property can be "text", "value", "checked", "visible", "label".
    pub fn gui_set(&mut self, gui_id: String, node_id: String, property: String, value: serde_json::Value) {
        self.commands.push(ScriptCommand::GuiSetNodeProp { gui_id, node_id, property, value });
    }

    /// Set keyboard focus to a specific GUI node. Pass empty string to clear focus.
    pub fn gui_focus(&mut self, gui_id: String, node_id: String) {
        self.commands.push(ScriptCommand::GuiFocus { gui_id, node_id });
    }

    // -- Phase 4B: Physics API --

    /// Apply a force to an entity's body (accumulated over the frame).
    pub fn apply_force(&mut self, entity_id: u64, fx: f32, fy: f32) {
        self.commands
            .push(ScriptCommand::ApplyForce { entity_id, fx, fy });
    }

    /// Apply an impulse to an entity's body (instant velocity change).
    pub fn apply_impulse(&mut self, entity_id: u64, ix: f32, iy: f32) {
        self.commands
            .push(ScriptCommand::ApplyImpulse { entity_id, ix, iy });
    }

    /// Set the linear velocity of an entity's body directly.
    pub fn set_velocity(&mut self, entity_id: u64, vx: f32, vy: f32) {
        self.commands
            .push(ScriptCommand::SetVelocity { entity_id, vx, vy });
    }

    /// Set the angular velocity of an entity's body.
    pub fn set_angular_velocity(&mut self, entity_id: u64, omega: f32) {
        self.commands
            .push(ScriptCommand::SetAngularVelocity { entity_id, omega });
    }

    /// Set the world gravity.
    pub fn set_gravity(&mut self, gx: f32, gy: f32) {
        self.commands.push(ScriptCommand::SetGravity { gx, gy });
    }

    /// Change the body type of an entity at runtime.
    /// `body_type` should be "dynamic", "static", or "kinematic".
    pub fn set_body_type(&mut self, entity_id: u64, body_type: String) {
        self.commands
            .push(ScriptCommand::SetBodyType { entity_id, body_type });
    }

    /// Create a joint between two entities at runtime (by name, deferred).
    pub fn add_joint(&mut self, entity_name: String, target_name: String, joint_json: String) {
        self.commands.push(ScriptCommand::AddJoint { entity_name, target_name, joint_json });
    }

    /// Remove a joint from an entity (by name, deferred).
    pub fn remove_joint(&mut self, entity_name: String) {
        self.commands.push(ScriptCommand::RemoveJoint { entity_name });
    }

    /// Set motor speed on an entity's joint (by name, deferred).
    pub fn set_motor_speed(&mut self, entity_name: String, speed: f32) {
        self.commands.push(ScriptCommand::SetMotorSpeed { entity_name, speed });
    }

    // -- Phase 4A: Audio API --

    /// Play a one-shot sound effect, centred.
    pub fn play_sound(&mut self, clip_id: String, volume: f32, pitch: f32) {
        self.play_sound_panned(clip_id, volume, pitch, 0.0);
    }

    /// Play a one-shot sound effect at a stereo position: `pan` is -1.0 hard
    /// left, 0.0 centre, 1.0 hard right.
    pub fn play_sound_panned(&mut self, clip_id: String, volume: f32, pitch: f32, pan: f32) {
        self.commands.push(ScriptCommand::PlaySound {
            clip_id,
            volume,
            pitch,
            pan: pan.clamp(-1.0, 1.0),
            sound_id: 0,
            looping: false,
        });
    }

    /// Play background music (stops any currently playing music).
    pub fn play_music(&mut self, clip_id: String, volume: f32, looping: bool) {
        self.commands.push(ScriptCommand::PlayMusic {
            clip_id,
            volume,
            looping,
        });
    }

    /// Stop a specific playing sound by handle.
    pub fn stop_sound(&mut self, handle_id: u64) {
        self.commands
            .push(ScriptCommand::StopSound { handle_id });
    }

    /// Stop the currently playing music.
    pub fn stop_music(&mut self) {
        self.commands.push(ScriptCommand::StopMusic);
    }

    /// Pause the currently playing music.
    pub fn pause_music(&mut self) {
        self.commands.push(ScriptCommand::PauseMusic);
    }

    /// Resume paused music.
    pub fn resume_music(&mut self) {
        self.commands.push(ScriptCommand::ResumeMusic);
    }

    /// Set the volume of a specific playing sound.
    pub fn set_sound_volume(&mut self, handle_id: u64, volume: f32) {
        self.commands
            .push(ScriptCommand::SetSoundVolume { handle_id, volume });
    }

    /// Set the volume of the currently playing music.
    pub fn set_music_volume(&mut self, volume: f32) {
        self.commands
            .push(ScriptCommand::SetMusicVolume { volume });
    }

    /// Set the master volume for all audio.
    pub fn set_master_volume(&mut self, volume: f32) {
        self.commands
            .push(ScriptCommand::SetMasterVolume { volume });
    }

    /// Stop every currently playing sound (and music).
    pub fn stop_all_sounds(&mut self) {
        self.commands.push(ScriptCommand::StopAllSounds);
    }

    /// Pause every currently playing sound (and music). Useful for pause overlays.
    pub fn pause_all_sounds(&mut self) {
        self.commands.push(ScriptCommand::PauseAllSounds);
    }

    /// Resume every currently paused sound (and music). Pairs with `pause_all_sounds`.
    pub fn resume_all_sounds(&mut self) {
        self.commands.push(ScriptCommand::ResumeAllSounds);
    }

}
