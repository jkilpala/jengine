//! Scene data format — JSON-serializable scene representation.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::animation::SpriteAnimator;
use crate::animation::AnimStateMachine;
use crate::ecs::components::*;
use crate::package::component_registry::ComponentRegistry;
use crate::particles::ParticleEmitter;
use crate::renderer::postprocess::PostProcessSettings;
use crate::tilemap::Tilemap;
use crate::ui::{UIButton, UIElement, UIImage, UIPanel, UIText,
    UIProgressBar, UISlider, UIToggle, UITextInput};

/// A complete scene — serializable to/from JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneData {
    /// Scene format version, preserved so a **v2** scene survives a
    /// round-trip through this v1 struct.
    ///
    /// The editor loads every scene as `SceneData` and re-serializes it to
    /// push into the viewport. Without this field the version was silently
    /// dropped, so a v2 scene arrived looking like v1 — and `load_scene_auto`
    /// then took the `ComponentBundle` path, where a package component is an
    /// unknown key that lands in `CustomComponents` instead of becoming a real
    /// component. The package's system found nothing and did nothing, on an
    /// otherwise healthy-looking scene.
    ///
    /// `skip_serializing_if` keeps v1 files byte-identical rather than
    /// stamping `"version": 1` across every scene in the repo.
    #[serde(default = "default_scene_version", skip_serializing_if = "is_v1")]
    pub version: u32,
    pub name: String,
    pub entities: Vec<EntityData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_process: Option<PostProcessSettings>,
    /// Optional `.sortlayers` asset ID that becomes the active set when
    /// this scene loads. `None` keeps raw `z_order` values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_layers: Option<String>,
    /// Every other top-level key, carried through untouched: `audio_refs`,
    /// `atlas_refs`, `paths`, `texture_refs` and whatever comes next.
    ///
    /// The editor loads a scene as this struct and saves it back, and serde
    /// dropped any key without a field. So saving a scene in the editor
    /// deleted its `audio_refs` (68 scenes have one), `atlas_refs` (52) and
    /// `paths` from disk, and Play - which pushes the editor's re-serialized
    /// copy to the viewport - ran without them: test-lab scene 64's patrols
    /// stood still in the editor because their routes never arrived
    /// (BUGS #157). A new scene-level key needs no field here to survive.
    #[serde(flatten, default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_scene_version() -> u32 {
    1
}

fn is_v1(v: &u32) -> bool {
    *v <= 1
}

impl Default for SceneData {
    fn default() -> Self {
        Self {
            version: 1,
            name: "Untitled Scene".to_string(),
            entities: Vec::new(),
            post_process: None,
            sort_layers: None,
            extra: Default::default(),
        }
    }
}

/// A single entity's serializable representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityData {
    pub name: String,
    /// Parent entity name. `None` means top-level (no parent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub components: ComponentBundle,
    /// The entities hung under this one (Oct 2026; Godot's nested scenes,
    /// Unity's prefab hierarchies): a prefab's parts - a cart's wheels, a
    /// catapult's arm - each its own `EntityData` with children of its own,
    /// placed in this one's frame. A child may be a `prefab_instance`, so a
    /// prefab can be built of prefabs. A scene flattens them into entities
    /// `parent`ed by name when it loads (`scene::prefab`); a script's
    /// `spawn_prefab` spawns the whole tree in every runtime
    /// (`scene::prefab::spawn_children`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<EntityData>,
}

/// Bundle of optional components. Each field corresponds to a built-in component type.
/// Custom (user-defined) components are captured via `#[serde(flatten)]` into the `custom` map.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComponentBundle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transform: Option<Transform>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite: Option<SpriteComponent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<ScriptComponent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextComponent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera_follow: Option<CameraFollow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<Camera2DComponent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Tags>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite_animator: Option<SpriteAnimator>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anim_state_machine: Option<AnimStateMachine>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tilemap: Option<Tilemap>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub particle_emitter: Option<ParticleEmitter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_element: Option<UIElement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_text: Option<UIText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_image: Option<UIImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_panel: Option<UIPanel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_button: Option<UIButton>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rigid_body: Option<RigidBody2D>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collider: Option<Collider2D>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joint: Option<Joint2D>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_source: Option<AudioSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_progress_bar: Option<UIProgressBar>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_slider: Option<UISlider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_toggle: Option<UIToggle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_text_input: Option<UITextInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gui: Option<GuiComponent>,
    /// On-screen sticks and buttons for a touch screen - see [`TouchControls`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub touch_controls: Option<TouchControls>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<MaterialComponent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point_light: Option<PointLight2D>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spot_light: Option<SpotLight2D>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directional_light: Option<crate::ecs::components::DirectionalLight2D>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ambient_light: Option<AmbientLight>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ambient_occlusion: Option<crate::ecs::components::AmbientOcclusion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toon_lighting: Option<crate::ecs::components::ToonLighting>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light_occluder: Option<LightOccluder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lightmap: Option<LightmapComponent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub velocity: Option<Velocity2D>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounce_area: Option<BounceArea>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gravity: Option<Gravity2D>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifetime: Option<Lifetime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub despawn_offscreen: Option<DespawnOffscreen>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trail: Option<crate::ecs::components::Trail>,
    // ── Step 2D native behaviors ──
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceleration: Option<Acceleration2D>,
    /// Slice 5.3 batch-script vocabulary — see [`Drag2D`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drag2d: Option<Drag2D>,
    /// Slice 5.3 batch-script vocabulary — see [`Force2D`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force2d: Option<Force2D>,
    /// Slice 5.3 batch-script vocabulary — see [`Wind2D`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind2d: Option<Wind2D>,
    /// Slice 5.3 batch-script vocabulary — see [`Impulse2D`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impulse2d: Option<Impulse2D>,
    /// Slice 5.3 batch-script vocabulary — see [`Torque2D`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub torque2d: Option<Torque2D>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_speed: Option<MaxSpeed>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oscillator: Option<Oscillator>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow_target: Option<FollowTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seek_target: Option<SeekTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tween: Option<Tween>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patrol_path: Option<PatrolPathRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow_caster: Option<ShadowCaster>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_path: Option<VectorPath>,
    /// Step 4 walking-skeleton — see [`BatchScriptComponent`] in `ecs::components`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_script: Option<BatchScriptComponent>,
    // ── Step 6 native behaviors ──
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_axis: Option<InputAxis2D>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_action_impulse: Option<InputActionImpulse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_area: Option<TriggerArea2D>,
    // ── Step 6 reopen — broader native behavior vocabulary ──
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wrap_bounds: Option<WrapBounds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clamp_bounds: Option<ClampBounds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub despawn_out_of_bounds: Option<DespawnOutOfBounds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face_velocity: Option<FaceVelocity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angular_velocity: Option<AngularVelocity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flicker: Option<Flicker>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shake: Option<Shake>,
    // ── Step 7 behavior graphs ──
    /// Slice 7.6 — attaches a `.behavior` graph asset to the entity.
    /// Loaded + bound by [`crate::behavior::BehaviorGraphSystem`] at
    /// `sync_instances` time. See [`BehaviorGraphComponent`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behavior_graph: Option<BehaviorGraphComponent>,
    // ── Combat (data-driven health + trigger hitboxes) ──
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<Health>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hitbox: Option<Hitbox>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub despawn_on_death: Option<DespawnOnDeath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unlit: Option<crate::ecs::components::Unlit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_bar: Option<HealthBar>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obstacle: Option<Obstacle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub separation: Option<Separation>,
    // These three were registered but had no field here, so a v1 scene that
    // authored one lost it on load - in every runtime, since they all load
    // through this bundle. Test-lab scene 94's map had no collider at all
    // and its crates fell out of the world (BUGS #164).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tilemap_collider: Option<crate::ecs::components::TilemapCollider>,
    /// Tilemap layers that cast shadows (Oct 2026).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tilemap_occluder: Option<crate::ecs::components::TilemapOccluder>,
    /// The light layers its sprite is on (Oct 2026).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_layers: Option<crate::ecs::components::LightLayers>,
    /// The frame's colour grade as a look-up table (Oct 2026).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_lut: Option<crate::ecs::components::ColorLut>,
    /// A skeleton that can go limp (Oct 2026). See [`crate::ecs::ragdoll`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ragdoll: Option<crate::ecs::ragdoll::Ragdoll>,
    /// Keyframe clips (Oct 2026). See [`crate::ecs::timeline`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<crate::ecs::timeline::Timeline>,
    /// A behaviour tree (Oct 2026). See [`crate::ecs::behavior_tree`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behavior_tree: Option<crate::ecs::behavior_tree::BehaviorTree>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character_controller: Option<crate::ecs::components::CharacterController2D>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bulk_entity: Option<crate::ecs::components::BulkEntity>,
    /// 2.2 — Prefab variants / overrides. When set, the runtime calls
    /// [`crate::scene::expand_prefab_instances`] before `load_scene`
    /// to merge the referenced prefab template's components onto this
    /// entity, then applies dotted-path overrides. The marker is
    /// retained on the spawned entity for tooling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefab_instance: Option<PrefabInstance>,
    /// Slice 3.5.B — skeletal animation foundation.
    /// Marks an entity as a bone in a skeleton. See
    /// [`crate::ecs::components::Bone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bone: Option<Bone>,
    /// Slice 3.5.B — marks the root of a skeleton instance. See
    /// [`crate::ecs::components::SkeletonRoot`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skeleton_root: Option<SkeletonRoot>,
    /// Slice 3.5.C — plays a skeletal animation clip against the
    /// `SkeletonRoot` on the same entity. See
    /// [`crate::animation::skeletal::SkeletalPlayer`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skeletal_player: Option<crate::animation::skeletal::SkeletalPlayer>,
    /// A body that breaks into a grid of pieces when hit hard enough. See
    /// [`crate::ecs::fracture::Breakable`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breakable: Option<crate::ecs::fracture::Breakable>,
    /// [`crate::ecs::cloth::Cloth`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloth: Option<crate::ecs::cloth::Cloth>,
    /// [`crate::ecs::cloth::Rope`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rope: Option<crate::ecs::cloth::Rope>,
    /// [`SpriteMask`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite_mask: Option<SpriteMask>,
    /// [`Masked`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub masked: Option<Masked>,
    /// [`crate::particles::gpu::GpuParticles`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_particles: Option<crate::particles::gpu::GpuParticles>,
    /// [`RenderLayers`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_layers: Option<RenderLayers>,
    /// [`crate::tilemap::autotile::Autotile`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autotile: Option<crate::tilemap::autotile::Autotile>,
    /// [`crate::ecs::carve::Carvable`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carvable: Option<crate::ecs::carve::Carvable>,
    /// [`crate::ecs::paint::Paintable`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paintable: Option<crate::ecs::paint::Paintable>,
    /// [`crate::ecs::fluid::Fluid`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fluid: Option<crate::ecs::fluid::Fluid>,
    /// [`crate::renderer::spline_shape::SplineShape`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spline_shape: Option<crate::renderer::spline_shape::SplineShape>,
    /// [`crate::ecs::area_effect::AreaEffect`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area_effect: Option<crate::ecs::area_effect::AreaEffect>,
    /// [`crate::navigation::NavRegion`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nav_region: Option<crate::navigation::NavRegion>,
    /// [`crate::navigation::NavAgent`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nav_agent: Option<crate::navigation::NavAgent>,
    /// [`crate::navigation::NavObstacle`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nav_obstacle: Option<crate::navigation::NavObstacle>,
    /// [`crate::ui::accessibility::Accessible`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessible: Option<crate::ui::accessibility::Accessible>,
    /// [`crate::animation::jiggle::Jiggle`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jiggle: Option<crate::animation::jiggle::Jiggle>,
    /// [`crate::ecs::visibility::VisibilityNotifier`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility_notifier: Option<crate::ecs::visibility::VisibilityNotifier>,
    /// [`crate::ecs::path_follow::PathFollow`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_follow: Option<crate::ecs::path_follow::PathFollow>,
    /// [`crate::ecs::sorting_group::SortingGroup`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sorting_group: Option<crate::ecs::sorting_group::SortingGroup>,
    /// [`crate::renderer::line2d::Line2D`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<crate::renderer::line2d::Line2D>,
    /// [`crate::ecs::canvas_group::CanvasGroup`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canvas_group: Option<crate::ecs::canvas_group::CanvasGroup>,
    /// [`crate::ecs::mirror::Mirror`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mirror: Option<crate::ecs::mirror::Mirror>,
    /// [`crate::ecs::water::WaterSurface`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub water_surface: Option<crate::ecs::water::WaterSurface>,
    /// [`crate::ecs::post_volume::PostVolume`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_volume: Option<crate::ecs::post_volume::PostVolume>,
    /// [`crate::ecs::pixel_sim::PixelSim`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pixel_sim: Option<crate::ecs::pixel_sim::PixelSim>,
    /// [`crate::ecs::smoke::Smoke`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smoke: Option<crate::ecs::smoke::Smoke>,
    /// [`crate::ecs::rewind::Rewind`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rewind: Option<crate::ecs::rewind::Rewind>,
    /// [`crate::ecs::rewind::Rewindable`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rewindable: Option<crate::ecs::rewind::Rewindable>,
    /// [`crate::ecs::fog::Fog`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fog: Option<crate::ecs::fog::Fog>,
    /// [`crate::ecs::wfc::WfcGenerator`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wfc: Option<crate::ecs::wfc::WfcGenerator>,
    /// [`crate::ecs::vision::Vision`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision: Option<crate::ecs::vision::Vision>,
    /// [`crate::ecs::day_cycle::DayCycle`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day_cycle: Option<crate::ecs::day_cycle::DayCycle>,
    /// [`crate::ecs::floating_origin::FloatingOrigin`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub floating_origin: Option<crate::ecs::floating_origin::FloatingOrigin>,
    /// [`crate::ecs::time_scale::TimeScale`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_scale: Option<crate::ecs::time_scale::TimeScale>,
    /// [`crate::ecs::cast_shadow::CastShadow`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cast_shadow: Option<crate::ecs::cast_shadow::CastShadow>,
    /// [`crate::ecs::auto_normal::AutoNormal`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_normal: Option<crate::ecs::auto_normal::AutoNormal>,
    /// [`crate::ecs::tile_light::TileLight`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tile_light: Option<crate::ecs::tile_light::TileLight>,
    /// [`crate::ecs::tile_light::TileLightSource`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tile_light_source: Option<crate::ecs::tile_light::TileLightSource>,
    /// [`crate::ecs::trajectory::Trajectory`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory: Option<crate::ecs::trajectory::Trajectory>,
    /// [`crate::ecs::text_path::TextPath`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_path: Option<crate::ecs::text_path::TextPath>,
    /// [`crate::ecs::beat_clock::BeatClock`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub beat_clock: Option<crate::ecs::beat_clock::BeatClock>,
    /// [`crate::ecs::displacement::DisplacementField`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub displacement_field: Option<crate::ecs::displacement::DisplacementField>,
    /// [`crate::ecs::displacement::Displacer`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub displacer: Option<crate::ecs::displacement::Displacer>,
    /// [`crate::ecs::displacement::FieldBend`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_bend: Option<crate::ecs::displacement::FieldBend>,
    /// [`crate::ecs::smooth_terrain::SmoothTerrain`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smooth_terrain: Option<crate::ecs::smooth_terrain::SmoothTerrain>,
    /// [`crate::ecs::fog_of_war::FogOfWar`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fog_of_war: Option<crate::ecs::fog_of_war::FogOfWar>,
    /// [`crate::ecs::fog_of_war::Sight`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sight: Option<crate::ecs::fog_of_war::Sight>,
    /// [`crate::ecs::fog_of_war::FogHidden`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fog_hidden: Option<crate::ecs::fog_of_war::FogHidden>,
    /// [`crate::ecs::dialogue::Dialogue`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dialogue: Option<crate::ecs::dialogue::Dialogue>,
    /// [`crate::ui::drag_drop::UIDraggable`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_draggable: Option<crate::ui::drag_drop::UIDraggable>,
    /// [`crate::ui::drag_drop::UIDropSlot`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_drop_slot: Option<crate::ui::drag_drop::UIDropSlot>,
    /// [`crate::ecs::sprite_stack::SpriteStack`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite_stack: Option<crate::ecs::sprite_stack::SpriteStack>,
    /// [`crate::ecs::mode7::Mode7`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode7: Option<crate::ecs::mode7::Mode7>,
    /// [`crate::ecs::mode7::Mode7Sprite`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode7_sprite: Option<crate::ecs::mode7::Mode7Sprite>,
    /// [`crate::ecs::silhouette::Silhouette`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silhouette: Option<crate::ecs::silhouette::Silhouette>,
    /// [`crate::ecs::silhouette::SilhouetteOccluder`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silhouette_occluder: Option<crate::ecs::silhouette::SilhouetteOccluder>,
    /// [`crate::ecs::sprite_tilt::SpriteTilt`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite_tilt: Option<crate::ecs::sprite_tilt::SpriteTilt>,
    /// [`crate::ecs::afterimage::Afterimage`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub afterimage: Option<crate::ecs::afterimage::Afterimage>,
    /// [`crate::ecs::slice::Sliceable`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sliceable: Option<crate::ecs::slice::Sliceable>,
    /// [`crate::ecs::portal::Portal`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal: Option<crate::ecs::portal::Portal>,
    /// [`crate::ecs::jelly::Jelly`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jelly: Option<crate::ecs::jelly::Jelly>,
    /// [`crate::ecs::offscreen_marker::OffscreenMarker`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offscreen_marker: Option<crate::ecs::offscreen_marker::OffscreenMarker>,
    /// [`crate::ui::radial_menu::UIRadialMenu`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_radial_menu: Option<crate::ui::radial_menu::UIRadialMenu>,
    /// [`crate::ui::speech_bubble::SpeechBubble`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speech_bubble: Option<crate::ui::speech_bubble::SpeechBubble>,
    /// [`crate::ui::toast::ToastStack`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toast_stack: Option<crate::ui::toast::ToastStack>,
    /// [`crate::ecs::sprite_outline::SpriteOutline`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sprite_outline: Option<crate::ecs::sprite_outline::SpriteOutline>,
    /// [`crate::ecs::lightning::Lightning`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lightning: Option<crate::ecs::lightning::Lightning>,
    /// [`crate::ecs::flow::FlowPath`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow_path: Option<crate::ecs::flow::FlowPath>,
    /// [`crate::ecs::nav_links::NavLink`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nav_link: Option<crate::ecs::nav_links::NavLink>,
    /// [`crate::ecs::path_morph::PathMorph`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_morph: Option<crate::ecs::path_morph::PathMorph>,
    /// [`crate::ecs::snow::SnowCover`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snow_cover: Option<crate::ecs::snow::SnowCover>,
    /// [`crate::video::VideoPlayer`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_player: Option<crate::video::VideoPlayer>,
    /// [`crate::renderer::freeform_light::FreeformLight`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freeform_light: Option<crate::renderer::freeform_light::FreeformLight>,
    /// [`crate::audio::zones::AudioZone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_zone: Option<crate::audio::zones::AudioZone>,
    /// [`crate::audio::spatial::AudioListener`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_listener: Option<crate::audio::spatial::AudioListener>,
    /// Slice 3.5.E — deformable triangle mesh skinned to the
    /// `SkeletonRoot` on the same entity. See
    /// [`crate::animation::skinned_mesh::SkinnedMesh`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skinned_mesh: Option<crate::animation::skinned_mesh::SkinnedMesh>,
    /// A textured polygon drawn in the sprite pass. See
    /// [`crate::ecs::components::Mesh2D`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh: Option<crate::ecs::components::Mesh2D>,
    /// Slice 3.5.F — two-bone IK constraint on the end (effector)
    /// bone. See [`crate::ecs::components::IkTwoBone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ik_two_bone: Option<crate::ecs::components::IkTwoBone>,
    /// Slice 3.5.F — look-at IK constraint on a bone.
    /// See [`crate::ecs::components::IkLookAt`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ik_look_at: Option<crate::ecs::components::IkLookAt>,
    /// A FABRIK chain on its end bone (Oct 2026).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ik_chain: Option<crate::ecs::components::IkChain>,
    /// A collider traced from the sprite (Oct 2026).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite_collider: Option<crate::ecs::components::SpriteCollider>,
    /// A moving surface (Oct 2026).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conveyor: Option<crate::ecs::components::Conveyor>,
    /// Light that bounces (Oct 2026).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_illumination: Option<crate::ecs::components::GlobalIllumination>,
    /// Parts by category and label (Oct 2026).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite_library: Option<crate::ecs::sprite_library::SpriteLibrary>,
    /// Which part of its library this sprite shows (Oct 2026).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite_resolver: Option<crate::ecs::sprite_library::SpriteResolver>,
    /// Custom components — any key not matching a built-in component name.
    #[serde(flatten)]
    pub custom: HashMap<String, serde_json::Value>,
}

/// Warn about custom components authored under a `"custom"` wrapper.
///
/// `ComponentBundle.custom` is `#[serde(flatten)]`, so it swallows every key
/// that is not a known built-in. Authoring
///
/// ```json
/// "components": { "custom": { "Stats": { "hp": 100 } } }
/// ```
///
/// therefore produces one custom component *named* `custom`, whose value is
/// an object — not a `Stats` component. `get_custom(e, "Stats", "hp")` then
/// returns nil forever.
///
/// This is silent, survives a save/load round trip unchanged, and is usually
/// masked by an `or 0` fallback in the script that reads it. Scene 37 in the
/// test lab shipped that way and rendered zeros for months. The names have to
/// sit directly under `components`.
fn warn_about_nested_custom(
    entity_name: &str,
    custom: &HashMap<String, serde_json::Value>,
) {
    let Some(nested) = custom.get("custom") else {
        return;
    };
    // Only complain when it looks like the mistake: an object of objects is a
    // wrapper, while a scalar is someone who genuinely wanted a component
    // called "custom" and is entitled to it.
    let inner_names: Vec<&str> = nested
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(_, v)| v.is_object())
                .map(|(k, _)| k.as_str())
                .collect()
        })
        .unwrap_or_default();
    if inner_names.is_empty() {
        return;
    }
    log::warn!(
        "[scene] entity '{entity_name}' has a custom component named 'custom' wrapping \
         {inner_names:?}. `custom` is not a component name - it is the catch-all field, \
         so these arrived as one component called 'custom' and `get_custom(e, \"{}\", ...)` \
         will return nil. Move them directly under `components`.",
        inner_names[0]
    );
}

/// Save a hecs World to SceneData.
pub fn save_scene(world: &hecs::World) -> SceneData {
    let mut entities = Vec::new();

    for (entity, _) in world.query::<()>().iter() {
        // Transient health-bar sprites are managed by `sync_health_bars`
        // at runtime; never persist them to the scene.
        if world.get::<&HealthBarView>(entity).is_ok() {
            continue;
        }
        // Transient name-tag text entities are managed by `sync_name_tags`.
        if world.get::<&NameTagView>(entity).is_ok() {
            continue;
        }
        let name = world
            .get::<&Name>(entity)
            .map(|n| n.0.clone())
            .unwrap_or_else(|_| format!("Entity_{}", entity.id()));

        let transform = world.get::<&Transform>(entity).ok().map(|t| (*t).clone());
        let sprite = world
            .get::<&SpriteComponent>(entity)
            .ok()
            .map(|s| (*s).clone());
        let script = world
            .get::<&ScriptComponent>(entity)
            .ok()
            .map(|s| (*s).clone());
        let text = world
            .get::<&TextComponent>(entity)
            .ok()
            .map(|t| (*t).clone());
        let camera_follow = world
            .get::<&CameraFollow>(entity)
            .ok()
            .map(|c| (*c).clone());
        let camera = world
            .get::<&Camera2DComponent>(entity)
            .ok()
            .map(|c| (*c).clone());
        let custom = world
            .get::<&CustomComponents>(entity)
            .ok()
            .map(|c| c.0.clone())
            .unwrap_or_default();
        let tags = world
            .get::<&Tags>(entity)
            .ok()
            .map(|t| (*t).clone())
            .filter(|t| !t.0.is_empty());
        let sprite_animator = world
            .get::<&SpriteAnimator>(entity)
            .ok()
            .map(|a| (*a).clone());
        let anim_state_machine = world
            .get::<&AnimStateMachine>(entity)
            .ok()
            .map(|a| (*a).clone());
        let tilemap = world
            .get::<&Tilemap>(entity)
            .ok()
            .map(|t| (*t).clone());
        let particle_emitter = world
            .get::<&ParticleEmitter>(entity)
            .ok()
            .map(|e| (*e).clone());
        let ui_element = world
            .get::<&UIElement>(entity)
            .ok()
            .map(|e| (*e).clone());
        let ui_text = world
            .get::<&UIText>(entity)
            .ok()
            .map(|e| (*e).clone());
        let ui_image = world
            .get::<&UIImage>(entity)
            .ok()
            .map(|e| (*e).clone());
        let ui_panel = world
            .get::<&UIPanel>(entity)
            .ok()
            .map(|e| (*e).clone());
        let ui_button = world
            .get::<&UIButton>(entity)
            .ok()
            .map(|e| (*e).clone());
        let rigid_body = world
            .get::<&RigidBody2D>(entity)
            .ok()
            .map(|e| (*e).clone());
        let collider = world
            .get::<&Collider2D>(entity)
            .ok()
            .map(|e| (*e).clone());
        let joint = world
            .get::<&Joint2D>(entity)
            .ok()
            .map(|e| (*e).clone());
        let audio_source = world
            .get::<&AudioSource>(entity)
            .ok()
            .map(|e| (*e).clone());
        let ui_progress_bar = world
            .get::<&UIProgressBar>(entity)
            .ok()
            .map(|e| (*e).clone());
        let ui_slider = world
            .get::<&UISlider>(entity)
            .ok()
            .map(|e| (*e).clone());
        let ui_toggle = world
            .get::<&UIToggle>(entity)
            .ok()
            .map(|e| (*e).clone());
        let ui_text_input = world
            .get::<&UITextInput>(entity)
            .ok()
            .map(|e| (*e).clone());
        let gui = world
            .get::<&GuiComponent>(entity)
            .ok()
            .map(|e| (*e).clone());
        let touch_controls = world
            .get::<&TouchControls>(entity)
            .ok()
            .map(|e| (*e).clone());
        let material = world
            .get::<&MaterialComponent>(entity)
            .ok()
            .map(|e| (*e).clone());
        let point_light = world
            .get::<&PointLight2D>(entity)
            .ok()
            .map(|e| (*e).clone());
        let spot_light = world
            .get::<&SpotLight2D>(entity)
            .ok()
            .map(|e| (*e).clone());
        let directional_light = world
            .get::<&crate::ecs::components::DirectionalLight2D>(entity)
            .ok()
            .map(|e| (*e).clone());
        let ambient_light = world
            .get::<&AmbientLight>(entity)
            .ok()
            .map(|e| (*e).clone());
        let ambient_occlusion = world
            .get::<&crate::ecs::components::AmbientOcclusion>(entity)
            .ok()
            .map(|e| *e);
        let toon_lighting = world
            .get::<&crate::ecs::components::ToonLighting>(entity)
            .ok()
            .map(|e| *e);
        let light_occluder = world
            .get::<&LightOccluder>(entity)
            .ok()
            .map(|e| (*e).clone());
        let lightmap = world
            .get::<&LightmapComponent>(entity)
            .ok()
            .map(|e| (*e).clone());
        let velocity = world
            .get::<&Velocity2D>(entity)
            .ok()
            .map(|e| (*e).clone());
        let bounce_area = world
            .get::<&BounceArea>(entity)
            .ok()
            .map(|e| (*e).clone());
        let gravity = world
            .get::<&Gravity2D>(entity)
            .ok()
            .map(|e| *e);
        let lifetime = world
            .get::<&Lifetime>(entity)
            .ok()
            .map(|e| *e);
        let despawn_offscreen = world
            .get::<&DespawnOffscreen>(entity)
            .ok()
            .map(|e| *e);
        let trail = world
            .get::<&crate::ecs::components::Trail>(entity)
            .ok()
            .map(|e| crate::ecs::components::Trail { points: Vec::new(), ..(*e).clone() });
        let acceleration = world
            .get::<&Acceleration2D>(entity)
            .ok()
            .map(|e| *e);
        let drag2d = world
            .get::<&Drag2D>(entity)
            .ok()
            .map(|e| *e);
        let force2d = world
            .get::<&Force2D>(entity)
            .ok()
            .map(|e| *e);
        let wind2d = world
            .get::<&Wind2D>(entity)
            .ok()
            .map(|e| *e);
        let impulse2d = world
            .get::<&Impulse2D>(entity)
            .ok()
            .map(|e| *e);
        let torque2d = world
            .get::<&Torque2D>(entity)
            .ok()
            .map(|e| *e);
        let max_speed = world
            .get::<&MaxSpeed>(entity)
            .ok()
            .map(|e| *e);
        let oscillator = world
            .get::<&Oscillator>(entity)
            .ok()
            .map(|e| *e);
        let follow_target = world
            .get::<&FollowTarget>(entity)
            .ok()
            .map(|e| (*e).clone());
        let seek_target = world
            .get::<&SeekTarget>(entity)
            .ok()
            .map(|e| (*e).clone());
        let tween = world
            .get::<&Tween>(entity)
            .ok()
            .map(|e| *e);
        let patrol_path = world
            .get::<&PatrolPathRef>(entity)
            .ok()
            .map(|e| (*e).clone());
        let shadow_caster = world
            .get::<&ShadowCaster>(entity)
            .ok()
            .map(|e| (*e).clone());
        let vector_path = world
            .get::<&VectorPath>(entity)
            .ok()
            .map(|e| (*e).clone());
        let batch_script = world
            .get::<&BatchScriptComponent>(entity)
            .ok()
            .map(|e| (*e).clone());
        // Step 6 native behaviors
        let input_axis = world
            .get::<&InputAxis2D>(entity)
            .ok()
            .map(|e| (*e).clone());
        let input_action_impulse = world
            .get::<&InputActionImpulse>(entity)
            .ok()
            .map(|e| (*e).clone());
        let trigger_area = world
            .get::<&TriggerArea2D>(entity)
            .ok()
            .map(|e| (*e).clone());
        // Step 6 reopen — broader native vocabulary.
        let wrap_bounds = world.get::<&WrapBounds>(entity).ok().map(|e| *e);
        let clamp_bounds = world.get::<&ClampBounds>(entity).ok().map(|e| *e);
        let despawn_out_of_bounds = world
            .get::<&DespawnOutOfBounds>(entity)
            .ok()
            .map(|e| *e);
        let face_velocity = world.get::<&FaceVelocity>(entity).ok().map(|e| *e);
        let angular_velocity = world.get::<&AngularVelocity>(entity).ok().map(|e| *e);
        let flicker = world.get::<&Flicker>(entity).ok().map(|e| *e);
        let shake = world.get::<&Shake>(entity).ok().map(|e| *e);
        // Step 7 behavior graphs
        let behavior_graph = world
            .get::<&BehaviorGraphComponent>(entity)
            .ok()
            .map(|e| (*e).clone());
        // Combat — data-driven health + trigger hitboxes.
        let health = world.get::<&Health>(entity).ok().map(|e| (*e).clone());
        let hitbox = world.get::<&Hitbox>(entity).ok().map(|e| (*e).clone());
        let despawn_on_death = world
            .get::<&DespawnOnDeath>(entity)
            .ok()
            .map(|e| (*e).clone());
        let unlit = world
            .get::<&crate::ecs::components::Unlit>(entity)
            .ok()
            .map(|e| (*e).clone());
        let health_bar = world.get::<&HealthBar>(entity).ok().map(|e| (*e).clone());
        let obstacle = world.get::<&Obstacle>(entity).ok().map(|e| (*e).clone());
        let separation = world.get::<&Separation>(entity).ok().map(|e| (*e).clone());
        let tilemap_collider = world.get::<&crate::ecs::components::TilemapCollider>(entity).ok().map(|e| (*e).clone());
        let tilemap_occluder = world.get::<&crate::ecs::components::TilemapOccluder>(entity).ok().map(|e| (*e).clone());
        let light_layers = world.get::<&crate::ecs::components::LightLayers>(entity).ok().map(|e| *e);
        let color_lut = world.get::<&crate::ecs::components::ColorLut>(entity).ok().map(|e| (*e).clone());
        let ragdoll = world.get::<&crate::ecs::ragdoll::Ragdoll>(entity).ok().map(|e| (*e).clone());
        let timeline = world.get::<&crate::ecs::timeline::Timeline>(entity).ok().map(|e| (*e).clone());
        let behavior_tree = world.get::<&crate::ecs::behavior_tree::BehaviorTree>(entity).ok().map(|e| (*e).clone());
        let character_controller =
            world.get::<&crate::ecs::components::CharacterController2D>(entity).ok().map(|e| (*e).clone());
        let bulk_entity = world.get::<&crate::ecs::components::BulkEntity>(entity).ok().map(|e| *e);
        // 2.2 prefab marker (retained after expansion).
        let prefab_instance = world
            .get::<&PrefabInstance>(entity)
            .ok()
            .map(|e| (*e).clone());
        // Slice 3.5.B — skeletal animation foundation.
        let bone = world.get::<&Bone>(entity).ok().map(|e| (*e).clone());
        let skeleton_root = world
            .get::<&SkeletonRoot>(entity)
            .ok()
            .map(|e| (*e).clone());
        // Slice 3.5.C — skeletal animation playback.
        let skeletal_player = world
            .get::<&crate::animation::skeletal::SkeletalPlayer>(entity)
            .ok()
            .map(|e| (*e).clone());
        let breakable = world.get::<&crate::ecs::fracture::Breakable>(entity).ok().map(|e| (*e).clone());
        let cloth = world.get::<&crate::ecs::cloth::Cloth>(entity).ok().map(|e| (*e).clone());
        let rope = world.get::<&crate::ecs::cloth::Rope>(entity).ok().map(|e| (*e).clone());
        let sprite_mask = world.get::<&SpriteMask>(entity).ok().map(|e| (*e).clone());
        let masked = world.get::<&Masked>(entity).ok().map(|e| (*e).clone());
        let gpu_particles = world.get::<&crate::particles::gpu::GpuParticles>(entity).ok().map(|e| (*e).clone());
        let render_layers = world.get::<&RenderLayers>(entity).ok().map(|e| *e);
        let autotile = world.get::<&crate::tilemap::autotile::Autotile>(entity).ok().map(|e| (*e).clone());
        let paintable = world.get::<&crate::ecs::paint::Paintable>(entity).ok().map(|p| crate::ecs::paint::Paintable {
            pending: Vec::new(),
            state: Default::default(),
            ..(*p).clone()
        });
        // A liquid's particles are runtime state: saved, it is poured again.
        let fluid = world.get::<&crate::ecs::fluid::Fluid>(entity).ok().map(|f| crate::ecs::fluid::Fluid {
            particles: Vec::new(),
            pour_carry: 0.0,
            lit: Vec::new(),
            ..(*f).clone()
        });
        let carvable = world.get::<&crate::ecs::carve::Carvable>(entity).ok().map(|c| crate::ecs::carve::Carvable {
            pending: Vec::new(),
            state: Default::default(),
            ..(*c).clone()
        });
        let spline_shape =
            world.get::<&crate::renderer::spline_shape::SplineShape>(entity).ok().map(|s| (*s).clone());
        let area_effect = world.get::<&crate::ecs::area_effect::AreaEffect>(entity).ok().map(|a| (*a).clone());
        let nav_region = world.get::<&crate::navigation::NavRegion>(entity).ok().map(|r| crate::navigation::NavRegion { baked: None, ..(*r).clone() });
        let nav_agent = world.get::<&crate::navigation::NavAgent>(entity).ok().map(|a| (*a).clone());
        let nav_obstacle = world.get::<&crate::navigation::NavObstacle>(entity).ok().map(|o| (*o).clone());
        let accessible = world.get::<&crate::ui::accessibility::Accessible>(entity).ok().map(|a| (*a).clone());
        let audio_listener = world.get::<&crate::audio::spatial::AudioListener>(entity).ok().map(|a| (*a).clone());
        let audio_zone = world.get::<&crate::audio::zones::AudioZone>(entity).ok().map(|z| (*z).clone());
        let freeform_light =
            world.get::<&crate::renderer::freeform_light::FreeformLight>(entity).ok().map(|l| (*l).clone());
        let jiggle = world
            .get::<&crate::animation::jiggle::Jiggle>(entity)
            .ok()
            .map(|j| crate::animation::jiggle::Jiggle { state: None, ..(*j).clone() });
        let visibility_notifier = world
            .get::<&crate::ecs::visibility::VisibilityNotifier>(entity)
            .ok()
            .map(|n| crate::ecs::visibility::VisibilityNotifier { on_screen: None, held: 0, ..(*n).clone() });
        let path_follow = world
            .get::<&crate::ecs::path_follow::PathFollow>(entity)
            .ok()
            .map(|f| crate::ecs::path_follow::PathFollow { returning: false, ..(*f).clone() });
        let sorting_group = world.get::<&crate::ecs::sorting_group::SortingGroup>(entity).ok().map(|g| (*g).clone());
        let line = world.get::<&crate::renderer::line2d::Line2D>(entity).ok().map(|l| (*l).clone());
        let canvas_group = world.get::<&crate::ecs::canvas_group::CanvasGroup>(entity).ok().map(|g| (*g).clone());
        let mirror = world.get::<&crate::ecs::mirror::Mirror>(entity).ok().map(|m| (*m).clone());
        let post_volume = world.get::<&crate::ecs::post_volume::PostVolume>(entity).ok().map(|v| (*v).clone());
        // The cells are runtime state: a saved simulation starts again.
        let pixel_sim = world.get::<&crate::ecs::pixel_sim::PixelSim>(entity).ok().map(|p| crate::ecs::pixel_sim::PixelSim {
            state: Default::default(),
            ..(*p).clone()
        });
        // The air is runtime state: a saved smoke starts clear.
        let smoke = world.get::<&crate::ecs::smoke::Smoke>(entity).ok().map(|s| crate::ecs::smoke::Smoke {
            state: Default::default(),
            ..(*s).clone()
        });
        // A recording is runtime state: a saved scene starts with none.
        let rewind = world.get::<&crate::ecs::rewind::Rewind>(entity).ok().map(|r| crate::ecs::rewind::Rewind {
            state: Default::default(),
            ..(*r).clone()
        });
        let rewindable =
            world.get::<&crate::ecs::rewind::Rewindable>(entity).ok().map(|r| crate::ecs::rewind::Rewindable {
                state: Default::default(),
                ..(*r).clone()
            });
        let fog = world.get::<&crate::ecs::fog::Fog>(entity).ok().map(|f| (*f).clone());
        // Its queue and the region in hand are runtime state.
        let wfc = world.get::<&crate::ecs::wfc::WfcGenerator>(entity).ok().map(|g| crate::ecs::wfc::WfcGenerator {
            state: Default::default(),
            ..(*g).clone()
        });
        let vision = world.get::<&crate::ecs::vision::Vision>(entity).ok().map(|v| crate::ecs::vision::Vision {
            state: Default::default(),
            ..(*v).clone()
        });
        let day_cycle =
            world.get::<&crate::ecs::day_cycle::DayCycle>(entity).ok().map(|d| crate::ecs::day_cycle::DayCycle {
                last_hour: None,
                ..(*d).clone()
            });
        let floating_origin = world
            .get::<&crate::ecs::floating_origin::FloatingOrigin>(entity)
            .ok()
            .map(|f| crate::ecs::floating_origin::FloatingOrigin { moved: Default::default(), ..(*f).clone() });
        let time_scale = world.get::<&crate::ecs::time_scale::TimeScale>(entity).ok().map(|t| *t);
        let cast_shadow = world.get::<&crate::ecs::cast_shadow::CastShadow>(entity).ok().map(|c| (*c).clone());
        let auto_normal = world
            .get::<&crate::ecs::auto_normal::AutoNormal>(entity)
            .ok()
            .map(|a| crate::ecs::auto_normal::AutoNormal { key: 0, ..(*a).clone() });
        let tile_light = world
            .get::<&crate::ecs::tile_light::TileLight>(entity)
            .ok()
            .map(|t| crate::ecs::tile_light::TileLight { key: 0, ..(*t).clone() });
        let tile_light_source =
            world.get::<&crate::ecs::tile_light::TileLightSource>(entity).ok().map(|t| (*t).clone());
        let trajectory = world.get::<&crate::ecs::trajectory::Trajectory>(entity).ok().map(|t| (*t).clone());
        let text_path = world.get::<&crate::ecs::text_path::TextPath>(entity).ok().map(|t| (*t).clone());
        let beat_clock = world.get::<&crate::ecs::beat_clock::BeatClock>(entity).ok().map(|t| (*t).clone());
        let displacement_field = world.get::<&crate::ecs::displacement::DisplacementField>(entity).ok().map(|t| (*t).clone());
        let displacer = world.get::<&crate::ecs::displacement::Displacer>(entity).ok().map(|t| (*t).clone());
        let field_bend = world.get::<&crate::ecs::displacement::FieldBend>(entity).ok().map(|t| (*t).clone());
        let smooth_terrain = world.get::<&crate::ecs::smooth_terrain::SmoothTerrain>(entity).ok().map(|t| (*t).clone());
        let fog_of_war = world.get::<&crate::ecs::fog_of_war::FogOfWar>(entity).ok().map(|t| (*t).clone());
        let sight = world.get::<&crate::ecs::fog_of_war::Sight>(entity).ok().map(|t| (*t).clone());
        let fog_hidden = world.get::<&crate::ecs::fog_of_war::FogHidden>(entity).ok().map(|t| (*t).clone());
        let dialogue = world.get::<&crate::ecs::dialogue::Dialogue>(entity).ok().map(|t| (*t).clone());
        let ui_draggable = world.get::<&crate::ui::drag_drop::UIDraggable>(entity).ok().map(|t| (*t).clone());
        let ui_drop_slot = world.get::<&crate::ui::drag_drop::UIDropSlot>(entity).ok().map(|t| (*t).clone());
        let sprite_stack = world.get::<&crate::ecs::sprite_stack::SpriteStack>(entity).ok().map(|t| (*t).clone());
        let mode7 = world.get::<&crate::ecs::mode7::Mode7>(entity).ok().map(|t| (*t).clone());
        let mode7_sprite = world.get::<&crate::ecs::mode7::Mode7Sprite>(entity).ok().map(|t| (*t).clone());
        let silhouette = world.get::<&crate::ecs::silhouette::Silhouette>(entity).ok().map(|t| (*t).clone());
        let silhouette_occluder = world.get::<&crate::ecs::silhouette::SilhouetteOccluder>(entity).ok().map(|t| (*t).clone());
        let sprite_tilt = world.get::<&crate::ecs::sprite_tilt::SpriteTilt>(entity).ok().map(|t| (*t).clone());
        let afterimage = world.get::<&crate::ecs::afterimage::Afterimage>(entity).ok().map(|t| (*t).clone());
        let sliceable = world.get::<&crate::ecs::slice::Sliceable>(entity).ok().map(|t| (*t).clone());
        let portal = world.get::<&crate::ecs::portal::Portal>(entity).ok().map(|t| (*t).clone());
        let jelly = world.get::<&crate::ecs::jelly::Jelly>(entity).ok().map(|t| (*t).clone());
        let offscreen_marker = world.get::<&crate::ecs::offscreen_marker::OffscreenMarker>(entity).ok().map(|t| (*t).clone());
        let ui_radial_menu = world.get::<&crate::ui::radial_menu::UIRadialMenu>(entity).ok().map(|t| (*t).clone());
        let speech_bubble = world.get::<&crate::ui::speech_bubble::SpeechBubble>(entity).ok().map(|t| (*t).clone());
        let toast_stack = world.get::<&crate::ui::toast::ToastStack>(entity).ok().map(|t| (*t).clone());
        let sprite_outline = world.get::<&crate::ecs::sprite_outline::SpriteOutline>(entity).ok().map(|t| (*t).clone());
        let lightning = world.get::<&crate::ecs::lightning::Lightning>(entity).ok().map(|t| (*t).clone());
        let flow_path = world.get::<&crate::ecs::flow::FlowPath>(entity).ok().map(|t| (*t).clone());
        let nav_link = world.get::<&crate::ecs::nav_links::NavLink>(entity).ok().map(|t| (*t).clone());
        let path_morph = world.get::<&crate::ecs::path_morph::PathMorph>(entity).ok().map(|t| (*t).clone());
        let snow_cover = world.get::<&crate::ecs::snow::SnowCover>(entity).ok().map(|t| (*t).clone());
        // The surface's motion is runtime state: a saved water is still.
        let water_surface = world.get::<&crate::ecs::water::WaterSurface>(entity).ok().map(|w| crate::ecs::water::WaterSurface {
            heights: Vec::new(),
            speeds: Vec::new(),
            ..(*w).clone()
        });
        let video_player = world.get::<&crate::video::VideoPlayer>(entity).ok().map(|v| (*v).clone());
        // Slice 3.5.E — skinned mesh.
        let skinned_mesh = world
            .get::<&crate::animation::skinned_mesh::SkinnedMesh>(entity)
            .ok()
            .map(|e| (*e).clone());
        let mesh = world.get::<&crate::ecs::components::Mesh2D>(entity).ok().map(|e| (*e).clone());
        // Slice 3.5.F — IK constraints.
        let ik_two_bone = world
            .get::<&crate::ecs::components::IkTwoBone>(entity)
            .ok()
            .map(|e| (*e).clone());
        let ik_look_at = world
            .get::<&crate::ecs::components::IkLookAt>(entity)
            .ok()
            .map(|e| (*e).clone());
        let ik_chain = world
            .get::<&crate::ecs::components::IkChain>(entity)
            .ok()
            .map(|e| (*e).clone());
        let sprite_collider = world
            .get::<&crate::ecs::components::SpriteCollider>(entity)
            .ok()
            .map(|e| (*e).clone());
        let conveyor = world
            .get::<&crate::ecs::components::Conveyor>(entity)
            .ok()
            .map(|e| (*e).clone());
        let global_illumination = world
            .get::<&crate::ecs::components::GlobalIllumination>(entity)
            .ok()
            .map(|e| *e);
        let sprite_library = world
            .get::<&crate::ecs::sprite_library::SpriteLibrary>(entity)
            .ok()
            .map(|e| (*e).clone());
        let sprite_resolver = world
            .get::<&crate::ecs::sprite_library::SpriteResolver>(entity)
            .ok()
            .map(|e| (*e).clone());

        let parent_name: Option<String> = world
            .get::<&Parent>(entity)
            .ok()
            .and_then(|p| world.get::<&Name>(p.0).ok().map(|n| n.0.clone()));

        entities.push(EntityData {
            children: Vec::new(),
            name,
            parent: parent_name,
            components: ComponentBundle {
                transform,
                sprite,
                script,
                text,
                camera_follow,
                camera,
                tags,
                sprite_animator,
                anim_state_machine,
                tilemap,
                particle_emitter,
                ui_element,
                ui_text,
                ui_image,
                ui_panel,
                ui_button,
                rigid_body,
                collider,
                joint,
                audio_source,
                ui_progress_bar,
                ui_slider,
                ui_toggle,
                ui_text_input,
                gui,
                touch_controls,
                material,
                point_light,
                spot_light,
                directional_light,
                ambient_light,
                ambient_occlusion,
                toon_lighting,
                light_occluder,
                lightmap,
                velocity,
                bounce_area,
                gravity,
                lifetime,
                despawn_offscreen,
                trail,
                acceleration,
                drag2d,
                force2d,
                wind2d,
                impulse2d,
                torque2d,
                max_speed,
                oscillator,
                follow_target,
                seek_target,
                tween,
                patrol_path,
                shadow_caster,
                vector_path,
                batch_script,
                input_axis,
                input_action_impulse,
                trigger_area,
                wrap_bounds,
                clamp_bounds,
                despawn_out_of_bounds,
                face_velocity,
                angular_velocity,
                flicker,
                shake,
                behavior_graph,
                health,
                hitbox,
                despawn_on_death,
                unlit,
                health_bar,
                obstacle,
                separation,
                tilemap_collider,
                tilemap_occluder,
                light_layers,
                color_lut,
                ragdoll,
                timeline,
                behavior_tree,
                character_controller,
                bulk_entity,
                prefab_instance,
                bone,
                skeleton_root,
                skeletal_player,
                breakable,
                cloth,
                rope,
                sprite_mask,
                masked,
                gpu_particles,
                render_layers,
                autotile,
                carvable,
                paintable,
                fluid,
                spline_shape,
                area_effect,
                nav_region,
                nav_agent,
                nav_obstacle,
                accessible,
                audio_zone,
                freeform_light,
                jiggle,
                visibility_notifier,
                path_follow,
                sorting_group,
                line,
                canvas_group,
                mirror,
                water_surface,
                post_volume,
                pixel_sim,
                smoke,
                rewind,
                rewindable,
                fog,
                wfc,
                vision,
                day_cycle,
                floating_origin,
                time_scale,
                cast_shadow,
                auto_normal,
                tile_light,
                tile_light_source,
                trajectory,
                text_path,
                beat_clock,
                displacement_field,
                displacer,
                field_bend,
                smooth_terrain,
                fog_of_war,
                sight,
                fog_hidden,
                dialogue,
                ui_draggable,
                ui_drop_slot,
                sprite_stack,
                mode7,
                mode7_sprite,
                silhouette,
                silhouette_occluder,
                sprite_tilt,
                afterimage,
                sliceable,
                portal,
                jelly,
                offscreen_marker,
                ui_radial_menu,
                speech_bubble,
                toast_stack,
                sprite_outline,
                lightning,
                flow_path,
                nav_link,
                path_morph,
                snow_cover,
                video_player,
                audio_listener,
                skinned_mesh,
                mesh,
                ik_two_bone,
                ik_look_at,
                ik_chain,
                sprite_collider,
                conveyor,
                sprite_library,
                sprite_resolver,
                global_illumination,
                custom,
            },
        });
    }

    SceneData {
        // Saving from a world produces the v1 typed shape by definition.
        version: 1,
        name: "Scene".to_string(),
        entities,
        post_process: None,
        sort_layers: None,
        extra: Default::default(),
    }
}

impl ComponentBundle {
    /// Add every component this bundle carries to `builder` - **the one
    /// list**.
    ///
    /// Two hand-written copies of it used to exist: the v1 scene loader and
    /// the native `SpawnEntity` arm, and the arm was a subset - 45 of the 77
    /// fields were missing at one point, `material` among them, so a prefab
    /// carrying one worked in the editor (which spawns generically through
    /// the registry) and silently lost it on native. Both go through here
    /// now, and `spawned_prefabs_keep_their_components` checks this method
    /// against the struct's fields.
    ///
    /// Two things stay with the callers because they differ on purpose:
    /// `custom` (the loader keeps it as data; the spawn arm first lifts any
    /// package components out of it), and the default `Transform` an entity
    /// without one gets (the loader gives one to renderables only; a spawned
    /// entity always gets one).
    pub fn add_to_builder(&self, builder: &mut hecs::EntityBuilder) {
        if let Some(ref transform) = self.transform {
            builder.add(transform.clone());
        }
        if let Some(ref sprite) = self.sprite {
            builder.add(sprite.clone());
        }
        if let Some(ref script) = self.script {
            builder.add(script.clone());
        }
        if let Some(ref text) = self.text {
            builder.add(text.clone());
        }
        if let Some(ref camera_follow) = self.camera_follow {
            builder.add(camera_follow.clone());
        }
        if let Some(ref camera) = self.camera {
            builder.add(camera.clone());
        }
        if let Some(ref tags) = self.tags {
            builder.add(tags.clone());
        }
        if let Some(ref sprite_animator) = self.sprite_animator {
            builder.add(sprite_animator.clone());
        }
        if let Some(ref anim_state_machine) = self.anim_state_machine {
            builder.add(anim_state_machine.clone());
        }
        if let Some(ref tilemap) = self.tilemap {
            builder.add(tilemap.clone());
        }
        if let Some(ref particle_emitter) = self.particle_emitter {
            builder.add(particle_emitter.clone());
        }
        if let Some(ref ui_element) = self.ui_element {
            builder.add(ui_element.clone());
        }
        if let Some(ref ui_text) = self.ui_text {
            builder.add(ui_text.clone());
        }
        if let Some(ref ui_image) = self.ui_image {
            builder.add(ui_image.clone());
        }
        if let Some(ref ui_panel) = self.ui_panel {
            builder.add(ui_panel.clone());
        }
        if let Some(ref ui_button) = self.ui_button {
            builder.add(ui_button.clone());
        }
        if let Some(ref rigid_body) = self.rigid_body {
            builder.add(rigid_body.clone());
        }
        if let Some(ref collider) = self.collider {
            builder.add(collider.clone());
        }
        if let Some(ref joint) = self.joint {
            builder.add(joint.clone());
        }
        if let Some(ref audio_source) = self.audio_source {
            builder.add(audio_source.clone());
        }
        if let Some(ref ui_progress_bar) = self.ui_progress_bar {
            builder.add(ui_progress_bar.clone());
        }
        if let Some(ref ui_slider) = self.ui_slider {
            builder.add(ui_slider.clone());
        }
        if let Some(ref ui_toggle) = self.ui_toggle {
            builder.add(ui_toggle.clone());
        }
        if let Some(ref ui_text_input) = self.ui_text_input {
            builder.add(ui_text_input.clone());
        }
        if let Some(ref gui) = self.gui {
            builder.add(gui.clone());
        }
        if let Some(ref touch_controls) = self.touch_controls {
            builder.add(touch_controls.clone());
        }
        if let Some(ref material) = self.material {
            builder.add(material.clone());
        }
        if let Some(ref point_light) = self.point_light {
            builder.add(point_light.clone());
        }
        if let Some(ref spot_light) = self.spot_light {
            builder.add(spot_light.clone());
        }
        if let Some(ref directional_light) = self.directional_light {
            builder.add(directional_light.clone());
        }
        if let Some(ref ambient_light) = self.ambient_light {
            builder.add(ambient_light.clone());
        }
        if let Some(ambient_occlusion) = self.ambient_occlusion {
            builder.add(ambient_occlusion);
        }
        if let Some(toon_lighting) = self.toon_lighting {
            builder.add(toon_lighting);
        }
        if let Some(ref light_occluder) = self.light_occluder {
            builder.add(light_occluder.clone());
        }
        if let Some(ref lightmap) = self.lightmap {
            builder.add(lightmap.clone());
        }
        if let Some(ref velocity) = self.velocity {
            builder.add(velocity.clone());
        }
        if let Some(ref bounce_area) = self.bounce_area {
            builder.add(bounce_area.clone());
        }
        if let Some(gravity) = self.gravity {
            builder.add(gravity);
        }
        if let Some(lifetime) = self.lifetime {
            builder.add(lifetime);
        }
        if let Some(despawn_offscreen) = self.despawn_offscreen {
            builder.add(despawn_offscreen);
        }
        if let Some(trail) = &self.trail {
            builder.add(trail.clone());
        }
        if let Some(acceleration) = self.acceleration {
            builder.add(acceleration);
        }
        if let Some(drag2d) = self.drag2d {
            builder.add(drag2d);
        }
        if let Some(force2d) = self.force2d {
            builder.add(force2d);
        }
        if let Some(wind2d) = self.wind2d {
            builder.add(wind2d);
        }
        if let Some(impulse2d) = self.impulse2d {
            builder.add(impulse2d);
        }
        if let Some(torque2d) = self.torque2d {
            builder.add(torque2d);
        }
        if let Some(max_speed) = self.max_speed {
            builder.add(max_speed);
        }
        if let Some(oscillator) = self.oscillator {
            builder.add(oscillator);
        }
        if let Some(ref follow_target) = self.follow_target {
            builder.add(follow_target.clone());
        }
        if let Some(ref seek_target) = self.seek_target {
            builder.add(seek_target.clone());
        }
        if let Some(tween) = self.tween {
            builder.add(tween);
        }
        if let Some(ref patrol_path) = self.patrol_path {
            builder.add(patrol_path.clone());
        }
        if let Some(ref shadow_caster) = self.shadow_caster {
            builder.add(shadow_caster.clone());
        }
        if let Some(ref vector_path) = self.vector_path {
            builder.add(vector_path.clone());
        }
        if let Some(ref batch_script) = self.batch_script {
            builder.add(batch_script.clone());
        }
            // Step 6 native behaviors
        if let Some(ref input_axis) = self.input_axis {
            builder.add(input_axis.clone());
        }
        if let Some(ref input_action_impulse) = self.input_action_impulse {
            builder.add(input_action_impulse.clone());
        }
        if let Some(ref trigger_area) = self.trigger_area {
            builder.add(trigger_area.clone());
        }
            // Step 6 reopen — broader native vocabulary.
        if let Some(wrap_bounds) = self.wrap_bounds {
            builder.add(wrap_bounds);
        }
        if let Some(clamp_bounds) = self.clamp_bounds {
            builder.add(clamp_bounds);
        }
        if let Some(despawn_oob) = self.despawn_out_of_bounds {
            builder.add(despawn_oob);
        }
        if let Some(face_velocity) = self.face_velocity {
            builder.add(face_velocity);
        }
        if let Some(angular_velocity) = self.angular_velocity {
            builder.add(angular_velocity);
        }
        if let Some(flicker) = self.flicker {
            builder.add(flicker);
        }
        if let Some(shake) = self.shake {
            builder.add(shake);
        }
            // Step 7 behavior graphs
        if let Some(ref behavior_graph) = self.behavior_graph {
            builder.add(behavior_graph.clone());
        }
            // Combat — data-driven health + trigger hitboxes.
        if let Some(ref health) = self.health {
            builder.add(health.clone());
        }
        if let Some(ref hitbox) = self.hitbox {
            builder.add(hitbox.clone());
        }
        if let Some(ref despawn_on_death) = self.despawn_on_death {
            builder.add(despawn_on_death.clone());
        }
        if let Some(ref unlit) = self.unlit {
            builder.add(unlit.clone());
        }
        if let Some(ref health_bar) = self.health_bar {
            builder.add(health_bar.clone());
        }
        if let Some(ref obstacle) = self.obstacle {
            builder.add(obstacle.clone());
        }
        if let Some(ref separation) = self.separation {
            builder.add(separation.clone());
        }
        if let Some(ref tilemap_occluder) = self.tilemap_occluder {
            builder.add(tilemap_occluder.clone());
        }
        if let Some(light_layers) = self.light_layers {
            builder.add(light_layers);
        }
        if let Some(ref color_lut) = self.color_lut {
            builder.add(color_lut.clone());
        }
        if let Some(ref ragdoll) = self.ragdoll {
            builder.add(ragdoll.clone());
        }
        if let Some(ref timeline) = self.timeline {
            builder.add(timeline.clone());
        }
        if let Some(ref tree) = self.behavior_tree {
            builder.add(tree.clone());
        }
        if let Some(ref tilemap_collider) = self.tilemap_collider {
            builder.add(tilemap_collider.clone());
        }
        if let Some(ref character_controller) = self.character_controller {
            builder.add(character_controller.clone());
        }
        if let Some(bulk_entity) = self.bulk_entity {
            builder.add(bulk_entity);
        }
            // 2.2 prefab marker — retained on the spawned entity so
            // tooling / scripts can ask which prefab produced it.
        if let Some(ref prefab_instance) = self.prefab_instance {
            builder.add(prefab_instance.clone());
        }
            // Slice 3.5.B — skeletal animation foundation.
        if let Some(ref bone) = self.bone {
            builder.add(bone.clone());
        }
        if let Some(ref skeleton_root) = self.skeleton_root {
            builder.add(skeleton_root.clone());
        }
            // Slice 3.5.C — skeletal animation playback.
        if let Some(ref skeletal_player) = self.skeletal_player {
            builder.add(skeletal_player.clone());
        }
        if let Some(ref breakable) = self.breakable {
            builder.add(breakable.clone());
        }
        if let Some(ref cloth) = self.cloth {
            builder.add(cloth.clone());
        }
        if let Some(ref rope) = self.rope {
            builder.add(rope.clone());
        }
        if let Some(ref m) = self.sprite_mask {
            builder.add(m.clone());
        }
        if let Some(ref m) = self.masked {
            builder.add(m.clone());
        }
        if let Some(ref g) = self.gpu_particles {
            builder.add(g.clone());
        }
        if let Some(l) = self.render_layers {
            builder.add(l);
        }
        if let Some(ref a) = self.autotile {
            builder.add(a.clone());
        }
        if let Some(ref c) = self.carvable {
            builder.add(c.clone());
        }
        if let Some(ref p) = self.paintable {
            builder.add(p.clone());
        }
        if let Some(ref f) = self.fluid {
            builder.add(f.clone());
        }
        if let Some(ref s) = self.spline_shape {
            builder.add(s.clone());
        }
        if let Some(ref a) = self.area_effect {
            builder.add(a.clone());
        }
        if let Some(ref r) = self.nav_region {
            builder.add(r.clone());
        }
        if let Some(ref a) = self.nav_agent {
            builder.add(a.clone());
        }
        if let Some(ref o) = self.nav_obstacle {
            builder.add(o.clone());
        }
        if let Some(ref a) = self.accessible {
            builder.add(a.clone());
        }
        if let Some(ref z) = self.audio_zone {
            builder.add(z.clone());
        }
        if let Some(ref l) = self.freeform_light {
            builder.add(l.clone());
        }
        if let Some(ref j) = self.jiggle {
            builder.add(j.clone());
        }
        if let Some(ref n) = self.visibility_notifier {
            builder.add(n.clone());
        }
        if let Some(ref f) = self.path_follow {
            builder.add(f.clone());
        }
        if let Some(ref g) = self.sorting_group {
            builder.add(g.clone());
        }
        if let Some(ref l) = self.line {
            builder.add(l.clone());
        }
        if let Some(ref g) = self.canvas_group {
            builder.add(g.clone());
        }
        if let Some(ref m) = self.mirror {
            builder.add(m.clone());
        }
        if let Some(ref w) = self.water_surface {
            builder.add(w.clone());
        }
        if let Some(ref v) = self.post_volume {
            builder.add(v.clone());
        }
        if let Some(ref p) = self.pixel_sim {
            builder.add(p.clone());
        }
        if let Some(ref s) = self.smoke {
            builder.add(s.clone());
        }
        if let Some(ref r) = self.rewind {
            builder.add(r.clone());
        }
        if let Some(ref r) = self.rewindable {
            builder.add(r.clone());
        }
        if let Some(ref f) = self.fog {
            builder.add(f.clone());
        }
        if let Some(ref g) = self.wfc {
            builder.add(g.clone());
        }
        if let Some(ref v) = self.vision {
            builder.add(v.clone());
        }
        if let Some(ref d) = self.day_cycle {
            builder.add(d.clone());
        }
        if let Some(ref f) = self.floating_origin {
            builder.add(f.clone());
        }
        if let Some(t) = self.time_scale {
            builder.add(t);
        }
        if let Some(ref c) = self.cast_shadow {
            builder.add(c.clone());
        }
        if let Some(ref a) = self.auto_normal {
            builder.add(a.clone());
        }
        if let Some(ref t) = self.tile_light {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.tile_light_source {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.trajectory {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.text_path {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.beat_clock {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.displacement_field {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.displacer {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.field_bend {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.smooth_terrain {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.fog_of_war {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.sight {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.fog_hidden {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.dialogue {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.ui_draggable {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.ui_drop_slot {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.sprite_stack {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.mode7 {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.mode7_sprite {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.silhouette {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.silhouette_occluder {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.sprite_tilt {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.afterimage {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.sliceable {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.portal {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.jelly {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.offscreen_marker {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.ui_radial_menu {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.speech_bubble {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.toast_stack {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.sprite_outline {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.lightning {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.flow_path {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.nav_link {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.path_morph {
            builder.add(t.clone());
        }
        if let Some(ref t) = self.snow_cover {
            builder.add(t.clone());
        }
        if let Some(ref v) = self.video_player {
            builder.add(v.clone());
        }
        if let Some(ref a) = self.audio_listener {
            builder.add(a.clone());
        }
            // Slice 3.5.E — skinned mesh.
        if let Some(ref skinned_mesh) = self.skinned_mesh {
            builder.add(skinned_mesh.clone());
        }
        if let Some(ref mesh) = self.mesh {
            builder.add(mesh.clone());
        }
            // Slice 3.5.F — IK constraints.
        if let Some(ref ik) = self.ik_two_bone {
            builder.add(ik.clone());
        }
        if let Some(ref ik) = self.ik_look_at {
            builder.add(ik.clone());
        }
        if let Some(ref ik) = self.ik_chain {
            builder.add(ik.clone());
        }
        if let Some(ref ik) = self.sprite_collider {
            builder.add(ik.clone());
        }
        if let Some(ref ik) = self.conveyor {
            builder.add(ik.clone());
        }
        if let Some(ref l) = self.sprite_library {
            builder.add(l.clone());
        }
        if let Some(gi) = self.global_illumination {
            builder.add(gi);
        }
        if let Some(ref r) = self.sprite_resolver {
            builder.add(r.clone());
        }
    }
}

/// Load SceneData into a hecs World (clears existing entities first).
pub fn load_scene(data: &SceneData, world: &mut hecs::World) {
    let _ = load_scene_with_mapping(data, world);
}

/// Identical to [`load_scene`], but also returns a `Vec<hecs::Entity>`
/// where index `i` is the entity spawned for `data.entities[i]`. Used
/// by surfaces that need a stable scene-index → world-entity map
/// (e.g. the editor viewport's behavior-graph mirror, where SceneData
/// remains the source of truth for rendering and a parallel hecs World
/// is maintained only so `BehaviorGraphSystem` has somewhere to run).
pub fn load_scene_with_mapping(
    data: &SceneData,
    world: &mut hecs::World,
) -> Vec<hecs::Entity> {
    world.clear();
    let mut mapping = Vec::with_capacity(data.entities.len());

    for entity_data in &data.entities {
        let mut builder = hecs::EntityBuilder::new();
        builder.add(Name(entity_data.name.clone()));

        if entity_data.components.transform.is_none()
            && (entity_data.components.sprite.is_some()
            || entity_data.components.sprite_animator.is_some()
            || entity_data.components.anim_state_machine.is_some()
            || entity_data.components.tilemap.is_some()
            || entity_data.components.camera.is_some()
            || entity_data.components.vector_path.is_some())
        {
            // Entities with renderable/camera components need a Transform for
            // the ECS query to match. Add a default one at the origin.
            builder.add(Transform::default());
        }
        entity_data.components.add_to_builder(&mut builder);
        if !entity_data.components.custom.is_empty() {
            warn_about_nested_custom(&entity_data.name, &entity_data.components.custom);
            builder.add(CustomComponents(entity_data.components.custom.clone()));
        }

        let entity = world.spawn(builder.build());
        mapping.push(entity);
    }
    // Second pass: resolve `parent` name references into `Parent` components.
    // We do this after every entity is spawned so forward references resolve
    // regardless of authoring order.
    for (i, entity_data) in data.entities.iter().enumerate() {
        let Some(parent_name) = entity_data.parent.as_deref() else { continue };
        // Find first entity in the scene with this name. Names are not
        // guaranteed unique; first-match keeps behavior deterministic.
        let parent_entity = data
            .entities
            .iter()
            .position(|e| e.name == parent_name)
            .map(|idx| mapping[idx]);
        match parent_entity {
            Some(p) if p != mapping[i] => {
                let _ = world.insert_one(mapping[i], Parent(p));
            }
            Some(_) => {
                log::warn!(
                    "Scene entity '{}' lists itself as parent — ignoring.",
                    entity_data.name
                );
            }
            None => {
                log::warn!(
                    "Scene entity '{}' has unknown parent '{}' — ignoring.",
                    entity_data.name,
                    parent_name
                );
            }
        }
    }
    mapping
}

// ──────────────────────────── V2: Registry-Based Scene Format ────────────────────────────

/// V2 scene data — components stored as a dynamic JSON map instead of `ComponentBundle`.
///
/// The JSON shape is identical to v1 but components are serialized/deserialized
/// through the `ComponentRegistry`, so package-provided components are supported
/// without modifying this code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneDataV2 {
    /// Format version — always 2 for registry-based scenes.
    #[serde(default = "default_version")]
    pub version: u32,
    pub name: String,
    pub entities: Vec<EntityDataV2>,
    /// Optional `.sortlayers` asset ID that becomes the active set when
    /// this scene loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_layers: Option<String>,
}

fn default_version() -> u32 {
    2
}

impl Default for SceneDataV2 {
    fn default() -> Self {
        Self {
            version: 2,
            name: "Untitled Scene".to_string(),
            entities: Vec::new(),
            sort_layers: None,
        }
    }
}

/// V2 entity data — component map is fully dynamic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityDataV2 {
    pub name: String,
    /// The entity this one hangs from, by name - the same field, and the
    /// same first-match rule, as a v1 scene's. Until Sep 2026 a v2 entity
    /// had no way to say it, and `save_scene_v2` dropped every `Parent`: a
    /// package game could not author a rig, and the editor's Play snapshot
    /// flattened the scene on Stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The parent's position in `entities`, which a save writes when the
    /// parent's name is shared - two legs both called `Leg` - so a foot
    /// cannot land on its namesake's leg. Preferred over `parent` when valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_index: Option<usize>,
    /// All components as `{ "component_name": { ... }, ... }`.
    pub components: HashMap<String, serde_json::Value>,
}

impl EntityDataV2 {
    pub fn new(name: String, components: HashMap<String, serde_json::Value>) -> Self {
        Self { name, parent: None, parent_index: None, components }
    }
}

/// Save a hecs World to `SceneDataV2` using the component registry.
///
/// Registered components are serialized via their `serialize_fn`.
/// `CustomComponents` data (script-created dynamic components) is flattened
/// into the same map. `Name` is extracted to the entity-level `name` field.
pub fn save_scene_v2(world: &hecs::World, registry: &ComponentRegistry) -> SceneDataV2 {
    save_scene_v2_inner(world, registry, false)
}

/// Snapshot a world for a scene push / pop, skipping bulk-spawned entities.
///
/// # Why this is not just `save_scene_v2`
///
/// A snapshot is taken on `push_scene`, on `pop_scene` and at the start of a
/// play session, so its cost is a hitch in the middle of the game rather than
/// something a save dialog pays. It is also the one case where preserving
/// *everything* is not obviously right: entities marked [`BulkEntity`] are
/// spawned in bulk by something that owns them — a flock, a particle-like
/// swarm — and that owner respawns them. Writing 16,000 boids into a snapshot
/// so they can be restored identically costs real time and restores state
/// nobody addressed individually in the first place.
///
/// So this skips them and `save_scene_v2` does not. A genuine save must keep
/// every entity, including any a scene chose to author with the marker; only
/// the transient in-play snapshot may drop them.
///
/// `BulkEntity` already means "spawned in bulk, not addressed individually" —
/// it governs the script mirror and the editor's Scene panel for the same
/// reason. This is the third thing it decides, and a game that wants its
/// bulk-spawned entities restored across a scene push simply does not set it.
pub fn snapshot_scene_v2(world: &hecs::World, registry: &ComponentRegistry) -> SceneDataV2 {
    save_scene_v2_inner(world, registry, true)
}

fn save_scene_v2_inner(
    world: &hecs::World,
    registry: &ComponentRegistry,
    skip_bulk: bool,
) -> SceneDataV2 {
    use crate::ecs::components::BulkEntity;
    let mut entities = Vec::new();
    let mut saved: Vec<hecs::Entity> = Vec::new();

    for (entity, _) in world.query::<()>().iter() {
        if skip_bulk && world.get::<&BulkEntity>(entity).is_ok() {
            continue;
        }
        saved.push(entity);
        let name = world
            .get::<&Name>(entity)
            .map(|n| n.0.clone())
            .unwrap_or_else(|_| format!("Entity_{}", entity.id()));

        let mut components = registry.serialize_entity(world, entity);

        // Flatten CustomComponents entries into the component map
        if let Ok(custom) = world.get::<&CustomComponents>(entity) {
            for (k, v) in &custom.0 {
                // Don't overwrite registered components with custom data
                if !components.contains_key(k) {
                    components.insert(k.clone(), v.clone());
                }
            }
        }

        entities.push(EntityDataV2::new(name, components));
    }

    // Parents, once every entity has its place in the list. By name, and by
    // index as well wherever the name alone would pick the wrong entity.
    let index_of: HashMap<hecs::Entity, usize> = saved.iter().enumerate().map(|(i, e)| (*e, i)).collect();
    let names: Vec<String> = entities.iter().map(|e| e.name.clone()).collect();
    let mut name_count: HashMap<&str, usize> = HashMap::new();
    for n in &names {
        *name_count.entry(n.as_str()).or_default() += 1;
    }
    for (i, entity) in saved.iter().enumerate() {
        let Ok(p) = world.get::<&Parent>(*entity) else { continue };
        // A parent that is not being saved (a bulk entity in a snapshot,
        // say) has nothing to point at.
        let Some(&pi) = index_of.get(&p.0) else { continue };
        entities[i].parent = Some(names[pi].clone());
        if name_count.get(names[pi].as_str()).copied().unwrap_or(0) > 1 {
            entities[i].parent_index = Some(pi);
        }
    }

    SceneDataV2 {
        version: 2,
        name: "Scene".to_string(),
        entities,
        sort_layers: None,
    }
}

/// Load `SceneDataV2` into a hecs World using the component registry.
///
/// Registered components are deserialized via their `deserialize_fn`.
/// Unknown keys (not in the registry) are collected into a `CustomComponents`
/// component on the entity.
pub fn load_scene_v2(data: &SceneDataV2, world: &mut hecs::World, registry: &ComponentRegistry) {
    world.clear();

    for entity_data in &data.entities {
        let mut builder = hecs::EntityBuilder::new();
        builder.add(Name(entity_data.name.clone()));

        let unknown = registry.deserialize_into_builder(&mut builder, &entity_data.components);

        // Store unrecognized keys as CustomComponents
        if !unknown.is_empty() {
            builder.add(CustomComponents(unknown));
        }

        world.spawn(builder.build());
    }
    resolve_v2_parents(data, world);
}

/// Give every entity of a freshly loaded v2 scene the `Parent` it names.
/// `load_scene_v2` spawns into an empty world in list order, so the scene's
/// entities are the world's, in that order.
fn resolve_v2_parents(data: &SceneDataV2, world: &mut hecs::World) {
    if data.entities.iter().all(|e| e.parent.is_none() && e.parent_index.is_none()) {
        return;
    }
    let mut spawned: Vec<hecs::Entity> = world.iter().map(|e| e.entity()).collect();
    spawned.sort_by_key(|e| e.id());
    if spawned.len() != data.entities.len() {
        log::warn!("v2 scene: {} entities spawned for {} listed - parents not resolved", spawned.len(), data.entities.len());
        return;
    }
    for (i, e) in data.entities.iter().enumerate() {
        let by_index = e.parent_index.filter(|&pi| pi < spawned.len() && pi != i);
        let target = by_index.or_else(|| {
            let name = e.parent.as_deref()?;
            let found = data.entities.iter().position(|o| o.name == name);
            if found.is_none() {
                log::warn!("Scene entity '{}' has unknown parent '{}' - ignoring.", e.name, name);
            }
            found
        });
        match target {
            Some(pi) if pi != i => {
                let _ = world.insert_one(spawned[i], Parent(spawned[pi]));
            }
            Some(_) => log::warn!("Scene entity '{}' lists itself as parent - ignoring.", e.name),
            None => {}
        }
    }
}

/// Convert a v1 `SceneData` (with `ComponentBundle`) to v2 format.
///
/// Useful for migrating existing scene files. The conversion loads the v1 scene
/// into a temporary world, then re-saves it using the registry.
pub fn upgrade_scene_v1_to_v2(
    v1: &SceneData,
    registry: &ComponentRegistry,
) -> SceneDataV2 {
    let mut world = hecs::World::new();
    load_scene(v1, &mut world);
    let mut v2 = save_scene_v2(&world, registry);
    v2.name = v1.name.clone();
    v2
}

/// Load a scene from JSON, auto-detecting v1 or v2 format.
///
/// If the JSON has `"version": 2`, it's loaded as v2. Otherwise it's treated
/// as v1 (`ComponentBundle` format) and loaded via the legacy path.
pub fn load_scene_auto(
    json: &str,
    world: &mut hecs::World,
    registry: &ComponentRegistry,
) -> Result<(), serde_json::Error> {
    // Peek at the version field
    let raw: serde_json::Value = serde_json::from_str(json)?;
    let version = raw.get("version").and_then(|v| v.as_u64()).unwrap_or(1);

    if version >= 2 {
        let data: SceneDataV2 = serde_json::from_value(raw)?;
        load_scene_v2(&data, world, registry);
    } else {
        let data: SceneData = serde_json::from_value(raw)?;
        load_scene(&data, world);
    }
    Ok(())
}

/// Scene-level metadata that lives outside the per-entity data.
///
/// Callers that need to apply scene-wide settings (e.g. the active
/// `.sortlayers` set) can read this without re-deserializing the whole
/// entity list. Populated for both v1 and v2 scene JSON.
// `Eq` is gone since `paths` arrived: `Path2D` holds floats, and comparing
// waypoint coordinates for exact equality is not a meaningful thing to want.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SceneMetadata {
    /// Asset id of the `.sortlayers` set this scene wants active.
    pub sort_layers: Option<String>,
    /// Named waypoint paths for `PatrolPathRef` entities in this scene.
    ///
    /// # Why these live in the scene
    ///
    /// `Path2D` and `Path2DLibrary` and the patrol system have existed, with
    /// tests, since the component was added — but nothing ever filled the
    /// library, so `PatrolPathRef` moved nothing in any runtime (BUGS #7). The
    /// missing piece was never the algorithm, it was a way to get points in.
    ///
    /// A patrol route is level geometry: it is placed against the walls of one
    /// scene and means nothing in another. Putting it here rather than in a
    /// separate `.path` asset means every runtime gets it from the scene
    /// parsing it already does — no third loader, and no push API for the
    /// editor, which has no filesystem of its own. Many entities in a scene can
    /// still share one route, which is the reason `Path2D` is a shared asset
    /// rather than a `Vec<Vec2>` on the component.
    ///
    /// If routes ever need to be shared *across* scenes, a `.path` asset
    /// referenced by id is the natural next step — `PatrolPathRef.path_id` is
    /// already a string key and would not change.
    pub paths: std::collections::HashMap<String, crate::ecs::path::Path2D>,
}

/// Parse only the scene-level metadata fields from a scene JSON string.
/// Cheap; does not allocate the entity list. Works for v1 and v2.
pub fn read_scene_metadata(json: &str) -> Result<SceneMetadata, serde_json::Error> {
    let raw: serde_json::Value = serde_json::from_str(json)?;
    let sort_layers = raw
        .get("sort_layers")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    // A malformed path is dropped rather than failing the whole scene load:
    // the rest of the level is still playable, and the patrol system already
    // skips an id it cannot resolve.
    let mut paths = std::collections::HashMap::new();
    if let Some(map) = raw.get("paths").and_then(|v| v.as_object()) {
        for (id, value) in map {
            match serde_json::from_value::<crate::ecs::path::Path2D>(value.clone()) {
                Ok(path) => {
                    paths.insert(id.clone(), path);
                }
                Err(e) => log::warn!("scene path '{id}' is malformed and was skipped: {e}"),
            }
        }
    }
    Ok(SceneMetadata { sort_layers, paths })
}

/// Every texture a set of prefabs will need once something spawns one.
///
/// A runtime preloads textures by walking the **world**, which is the right
/// answer for everything the scene names and the wrong answer for
/// everything it does not: a prefab's sheet is referenced by nothing until
/// a script spawns from it, mid-frame, far too late to fetch an image. The
/// result is a prefab that spawns, lives, moves and draws nothing at all,
/// with no warning anywhere - the missing-texture warning only fires for
/// ids something asked for.
///
/// The web runtime had its own copy of this from the start; native did not,
/// which is how a game's RTS came to have an invisible army (BUGS #137).
pub fn prefab_texture_ids<'a>(
    prefabs: impl IntoIterator<Item = &'a EntityData>,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    // A prefab's parts (`EntityData::children`) need theirs too: a wagon's
    // barrels are named by nothing but the wagon.
    let mut todo: Vec<&EntityData> = prefabs.into_iter().collect();
    while let Some(entity) = todo.pop() {
        todo.extend(entity.children.iter());
        let Ok(value) = serde_json::to_value(&entity.components) else {
            continue;
        };
        for key in ["sprite", "mesh", "trail", "line", "particle_emitter", "lightmap", "ui_image", "sprite_stack", "mode7"] {
            if let Some(id) = value
                .get(key)
                .and_then(|s| s.get("texture_id"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                out.insert(id.to_string());
            }
        }
        // Running water's picture (`ecs::flow`).
        if let Some(id) = value.get("flow_path").and_then(|t| t.get("texture")).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            out.insert(id.to_string());
        }
        // A toast stack's card and icons (`ui::toast`).
        if let Some(t) = value.get("toast_stack") {
            if let Some(id) = t.get("texture").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                out.insert(id.to_string());
            }
            for id in t.get("icons").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str()) {
                out.insert(id.to_string());
            }
        }
        // A speech bubble's pictures (`ui::speech_bubble`).
        for key in ["texture", "tail_texture"] {
            if let Some(id) = value.get("speech_bubble").and_then(|t| t.get(key)).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                out.insert(id.to_string());
            }
        }
        // An off-screen marker's arrow and icon (`ecs::offscreen_marker`).
        for key in ["arrow_texture", "icon_texture"] {
            if let Some(id) = value.get("offscreen_marker").and_then(|t| t.get(key)).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                out.insert(id.to_string());
            }
        }
        // What a sliceable shows inside (`ecs::slice`), once cut.
        if let Some(id) = value.get("sliceable").and_then(|t| t.get("inside_texture")).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            out.insert(id.to_string());
        }
        // A tilted sprite's back (`ecs::sprite_tilt`), shown only once it is
        // turned over.
        if let Some(id) = value.get("sprite_tilt").and_then(|t| t.get("back_texture")).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            out.insert(id.to_string());
        }
        // A sprite library's parts (`ecs::sprite_library`).
        if let Some(cats) = value.get("sprite_library").and_then(|l| l.get("categories")).and_then(|c| c.as_object()) {
            for entry in cats.values().filter_map(|c| c.as_object()).flat_map(|c| c.values()) {
                if let Some(id) = entry.get("texture_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    out.insert(id.to_string());
                }
            }
        }
        // A light's cookie (`PointLight2D::cookie`), named by nothing else.
        for key in ["point_light", "spot_light"] {
            if let Some(id) = value
                .get(key)
                .and_then(|s| s.get("cookie"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                out.insert(id.to_string());
            }
        }
        // A sprite animator can swap the texture per frame.
        if let Some(clips) = value
            .get("sprite_animator")
            .and_then(|a| a.get("clips"))
            .and_then(|c| c.as_object())
        {
            for clip in clips.values() {
                let Some(frames) = clip.get("frames").and_then(|f| f.as_array()) else {
                    continue;
                };
                for frame in frames {
                    if let Some(id) = frame
                        .get("texture_id")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        out.insert(id.to_string());
                    }
                }
            }
        }
        // And a prefab can carry a whole tilemap.
        if let Some(sets) = value
            .get("tilemap")
            .and_then(|t| t.get("tilesets"))
            .and_then(|t| t.as_array())
        {
            for set in sets {
                if let Some(id) = set
                    .get("texture_id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    out.insert(id.to_string());
                }
            }
        }
    }
    out
}

/// The material files a prefab draws with once something spawns one -
/// built-in ids (`default_sprite`, `additive`, ...) name no file and are left
/// out.
///
/// The same blind spot [`prefab_texture_ids`] closes, for materials: the
/// editor pushed only the materials scene entities named, so a graph material
/// that only a spawned prefab used never reached its viewport, and the sprite
/// drew with the default shader - the showcase's Spellcraft shockwaves (a
/// graph over a white quad) flashed as white boxes in the editor while
/// native, which loads materials on demand, drew them. The browser hosts
/// fetch these as soon as the prefabs arrive.
pub fn prefab_material_ids<'a>(
    prefabs: impl IntoIterator<Item = &'a EntityData>,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for entity in prefabs {
        let Ok(value) = serde_json::to_value(&entity.components) else {
            continue;
        };
        if let Some(id) = value
            .get("material")
            .and_then(|m| m.get("material_id"))
            .and_then(|v| v.as_str())
            .filter(|s| !crate::renderer::material::is_builtin_material(s))
        {
            out.insert(id.to_string());
        }
    }
    out
}

/// Every texture a loaded world draws with, for a runtime to fetch before
/// the first frame.
///
/// Native and the web runtime each kept their own copy of this walk, and
/// the two had drifted: the web's knew nothing of particle, animation-frame
/// or tileset textures, and **neither knew `UIImage`** - so an entity-UI
/// icon on a sheet no sprite shared drew nothing outside the editor (which
/// pulls whatever the renderer misses). One list now, beside
/// [`prefab_texture_ids`], which is the same question asked of prefabs.
pub fn world_texture_ids(world: &hecs::World) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let mut add = |id: &str| {
        if !id.is_empty() {
            out.insert(id.to_string());
        }
    };
    for (_, s) in world.query::<&SpriteComponent>().iter() {
        add(&s.texture_id);
    }
    // A mesh names a texture the way a sprite does.
    for (_, m) in world.query::<&Mesh2D>().iter() {
        add(&m.texture_id);
    }
    // Every part a sprite library can swap in: nothing names one until it
    // is worn, which is too late to read a file.
    for (_, l) in world.query::<&crate::ecs::sprite_library::SpriteLibrary>().iter() {
        for id in l.texture_ids() {
            add(id);
        }
    }
    // A Mode 7 plane's ground (`ecs::mode7`).
    for (_, m) in world.query::<&crate::ecs::mode7::Mode7>().iter() {
        add(&m.texture_id);
    }
    // A sprite stack's strip of slices (`ecs::sprite_stack`).
    for (_, s) in world.query::<&crate::ecs::sprite_stack::SpriteStack>().iter() {
        add(&s.texture_id);
    }
    // Running water's picture (`ecs::flow`).
    for (_, f) in world.query::<&crate::ecs::flow::FlowPath>().iter() {
        add(&f.texture);
    }
    // A toast stack's card and the icons its toasts will show (`ui::toast`).
    for (_, s) in world.query::<&crate::ui::toast::ToastStack>().iter() {
        add(&s.texture);
        for icon in &s.icons {
            add(icon);
        }
    }
    // A speech bubble's pictures (`ui::speech_bubble`): drawn only once
    // someone speaks.
    for (_, b) in world.query::<&crate::ui::speech_bubble::SpeechBubble>().iter() {
        add(&b.texture);
        add(&b.tail_texture);
    }
    // An off-screen marker's arrow and icon (`ecs::offscreen_marker`): it
    // swaps between them.
    for (_, m) in world.query::<&crate::ecs::offscreen_marker::OffscreenMarker>().iter() {
        add(&m.arrow_texture);
        add(&m.icon_texture);
    }
    // What a sliceable shows inside (`ecs::slice`): drawn only once cut.
    for (_, s) in world.query::<&crate::ecs::slice::Sliceable>().iter() {
        add(&s.inside_texture);
    }
    // A tilted sprite's back (`ecs::sprite_tilt`): nothing names it until
    // the sprite is turned over.
    for (_, t) in world.query::<&crate::ecs::sprite_tilt::SpriteTilt>().iter() {
        add(&t.back_texture);
    }
    // A conversation's portraits (`ecs::dialogue`), read from its table.
    for (_, d) in world.query::<&crate::ecs::dialogue::Dialogue>().iter() {
        for id in crate::ecs::dialogue::portrait_ids(d) {
            add(&id);
        }
    }
    // A spline shape's fill and edge (`renderer::spline_shape`).
    for (_, s) in world.query::<&crate::renderer::spline_shape::SplineShape>().iter() {
        add(&s.fill_texture);
        add(&s.edge_texture);
    }
    // A lit material's normal map: without it the lit pass shades against
    // the flat fallback and looks like a plain sprite.
    for (_, m) in world.query::<&MaterialComponent>().iter() {
        if let Some(id) = m.normal_map_id.as_deref() {
            add(id);
        }
    }
    // A lightmap is drawn into the light buffer, so nothing else names it
    // (BUGS #95).
    // A colour grade's strip: nothing draws it, so nothing else names it.
    for (_, l) in world.query::<&crate::ecs::components::ColorLut>().iter() {
        add(&l.texture_id);
    }
    // A falling-sand grid's seed picture (`ecs::pixel_sim`), read for its
    // pixels and drawn by nothing.
    for (_, p) in world.query::<&crate::ecs::pixel_sim::PixelSim>().iter() {
        add(&p.seed_texture);
    }
    for (_, l) in world.query::<&LightmapComponent>().iter() {
        add(&l.texture_id);
    }
    // A light's cookie is drawn through by the light, so nothing else names
    // it either.
    for (_, l) in world.query::<&crate::ecs::components::PointLight2D>().iter() {
        add(&l.cookie);
    }
    for (_, l) in world.query::<&crate::ecs::components::SpotLight2D>().iter() {
        add(&l.cookie);
    }
    // A rich text's inline icons (`renderer::rich_text`), world and UI.
    for (_, t) in world.query::<&TextComponent>().iter() {
        if t.rich && t.text.contains("[icon=") {
            for icon in crate::renderer::rich_text::parse(&t.text, 1).icons {
                add(&t.text[icon.id]);
            }
        }
    }
    for (_, t) in world.query::<&crate::ui::UIText>().iter() {
        if t.rich && t.text.contains("[icon=") {
            for icon in crate::renderer::rich_text::parse(&t.text, 1).icons {
                add(&t.text[icon.id]);
            }
        }
    }
    // A texture mask is never drawn, only read (`renderer::mask`).
    for (_, m) in world.query::<&crate::ecs::components::SpriteMask>().iter() {
        if let Some(id) = m.shape.texture_id() {
            add(id);
        }
    }
    for (_, e) in world.query::<&ParticleEmitter>().iter() {
        if let Some(id) = e.texture_id.as_deref() {
            add(id);
        }
    }
    // A clip frame can swap the sprite's texture as it plays.
    for (_, a) in world.query::<&SpriteAnimator>().iter() {
        for clip in a.clips.values() {
            for frame in &clip.frames {
                if let Some(id) = frame.texture_id.as_deref() {
                    add(id);
                }
            }
        }
    }
    for (_, t) in world.query::<&Tilemap>().iter() {
        for set in &t.tilesets {
            add(&set.texture_id);
        }
    }
    for (_, i) in world.query::<&UIImage>().iter() {
        add(&i.texture_id);
    }
    // A trail stretches its texture along the ribbon.
    for (_, t) in world.query::<&crate::ecs::components::Trail>().iter() {
        add(&t.texture_id);
    }
    // So does a line (`renderer::line2d`).
    for (_, l) in world.query::<&crate::renderer::line2d::Line2D>().iter() {
        add(&l.texture_id);
    }
    // A video's frames are written, not loaded (`video`).
    for id in crate::video::video_texture_ids(world) {
        out.remove(&id);
    }
    // A camera's target is drawn, not loaded (`ecs::views`).
    for id in crate::ecs::views::render_target_ids(world) {
        out.remove(&id);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    /// A trail's texture is fetched like a sprite's, in a scene and in a
    /// prefab - and so is a mesh's in a prefab. Neither collector knew
    /// `trail`, so the showcase's Trails drew no ribbon at all: the texture
    /// was never loaded and a draw with no texture is skipped.
    #[test]
    fn trails_and_meshes_name_textures_to_preload() {
        let mut world = hecs::World::new();
        world.spawn((crate::ecs::components::Trail { texture_id: "fx/ribbon.png".into(), ..Default::default() },));
        assert!(world_texture_ids(&world).contains("fx/ribbon.png"));

        let prefab: EntityData = serde_json::from_value(serde_json::json!({
            "name": "Meteor",
            "components": { "trail": { "texture_id": "fx/streak.png" } }
        }))
        .unwrap();
        assert!(prefab_texture_ids([&prefab]).contains("fx/streak.png"));
    }

    /// An entity-UI icon is fetched like a sprite: nothing else in a scene
    /// has to name its sheet.
    #[test]
    fn a_ui_image_names_a_texture_to_preload() {
        let mut world = hecs::World::new();
        world.spawn((UIElement::default(), UIImage { texture_id: "ui/icons.png".into(), ..Default::default() }));
        world.spawn((SpriteComponent { texture_id: "a.png".into(), ..Default::default() },));
        let ids = world_texture_ids(&world);
        assert!(ids.contains("ui/icons.png"), "{ids:?}");
        assert!(ids.contains("a.png"));

        let prefab: EntityData = serde_json::from_value(serde_json::json!({
            "name": "Icon",
            "components": { "ui_element": {"anchor": "TopLeft", "offset": [0.0, 0.0], "size": [32.0, 32.0]},
                            "ui_image": {"texture_id": "ui/prefab.png"} }
        }))
        .unwrap();
        assert!(prefab_texture_ids([&prefab]).contains("ui/prefab.png"));
    }

    #[test]
    fn scene_roundtrip() {
        let mut world = hecs::World::new();

        // Spawn a few entities with different component combinations
        world.spawn((
            Name("Player".to_string()),
            Transform {
                position: Vec2::new(10.0, 20.0),
                rotation: 1.5,
                scale: Vec2::new(2.0, 3.0),
            },
            SpriteComponent {
                texture_id: "player.png".into(),
                color: [1.0, 0.5, 0.0, 1.0],
                z_order: 5,
                size: Some([32.0, 64.0]),
                uv_rect: None,
                anchor: [0.5, 0.5],
                fill: None,
                sort_layer: None,
                region: None,
                region_resolved: false,
                corners: None,
                ..SpriteComponent::default()
            },
        ));
        world.spawn((
            Name("Background".to_string()),
            Transform::default(),
            SpriteComponent {
                texture_id: "bg.png".into(),
                color: [1.0, 1.0, 1.0, 1.0],
                z_order: -10,
                size: None,
                uv_rect: None,
                anchor: [0.5, 0.5],
                fill: None,
                sort_layer: None,
                region: None,
                region_resolved: false,
                corners: None,
                ..SpriteComponent::default()
            },
        ));
        world.spawn((
            Name("ScriptOnly".to_string()),
            ScriptComponent {
                language: ScriptLanguage::Lua,
                source_path: "scripts/enemy.lua".to_string(),
                cull_distance: None,
            },
        ));

        // Save
        let scene_data = save_scene(&world);
        let json = serde_json::to_string_pretty(&scene_data).unwrap();

        // Verify JSON is valid
        let parsed: SceneData = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.entities.len(), 3);

        // Load into a fresh world
        let mut world2 = hecs::World::new();
        load_scene(&parsed, &mut world2);

        // Verify roundtrip
        let scene_data2 = save_scene(&world2);
        assert_eq!(scene_data2.entities.len(), 3);

        // Find the Player entity and check its data
        let player = scene_data2
            .entities
            .iter()
            .find(|e| e.name == "Player")
            .unwrap();
        let t = player.components.transform.as_ref().unwrap();
        assert!((t.position.x - 10.0).abs() < f32::EPSILON);
        assert!((t.position.y - 20.0).abs() < f32::EPSILON);
        assert!((t.rotation - 1.5).abs() < f32::EPSILON);

        let s = player.components.sprite.as_ref().unwrap();
        assert_eq!(s.texture_id, "player.png");
        assert_eq!(s.z_order, 5);

        // Find the ScriptOnly entity
        let script_entity = scene_data2
            .entities
            .iter()
            .find(|e| e.name == "ScriptOnly")
            .unwrap();
        assert!(script_entity.components.transform.is_none());
        assert!(script_entity.components.sprite.is_none());
        let sc = script_entity.components.script.as_ref().unwrap();
        assert_eq!(sc.source_path, "scripts/enemy.lua");
    }

    #[test]
    fn scene_json_format() {
        let scene = SceneData {
            version: 1,
            name: "Test".to_string(),
            post_process: None,
            extra: Default::default(),
            entities: vec![EntityData {
                children: Vec::new(),
                name: "Entity1".to_string(),
                parent: None,
                components: ComponentBundle {
                    transform: Some(Transform::default()),
                    ..Default::default()
                },
            }],
            sort_layers: None,
        };

        let json = serde_json::to_string_pretty(&scene).unwrap();
        // Verify empty components are omitted (skip_serializing_if)
        assert!(!json.contains("\"sprite\""));
        assert!(!json.contains("\"script\""));
        assert!(json.contains("\"transform\""));
    }

    #[test]
    fn scene_v2_roundtrip() {
        use crate::package::core_registration::register_core_components;
        use crate::package::component_registry::ComponentRegistry;

        let mut registry = ComponentRegistry::new();
        register_core_components(&mut registry);

        let mut world = hecs::World::new();

        world.spawn((
            Name("Player".to_string()),
            Transform {
                position: Vec2::new(10.0, 20.0),
                rotation: 1.5,
                scale: Vec2::new(2.0, 3.0),
            },
            SpriteComponent {
                texture_id: "player.png".into(),
                color: [1.0, 0.5, 0.0, 1.0],
                z_order: 5,
                size: Some([32.0, 64.0]),
                uv_rect: None,
                anchor: [0.5, 0.5],
                fill: None,
                sort_layer: None,
                region: None,
                region_resolved: false,
                corners: None,
                ..SpriteComponent::default()
            },
        ));
        world.spawn((
            Name("ScriptOnly".to_string()),
            ScriptComponent {
                language: ScriptLanguage::Lua,
                source_path: "scripts/enemy.lua".to_string(),
                cull_distance: None,
            },
        ));

        // Save via registry
        let scene_v2 = save_scene_v2(&world, &registry);
        assert_eq!(scene_v2.version, 2);
        assert_eq!(scene_v2.entities.len(), 2);

        let json = serde_json::to_string_pretty(&scene_v2).unwrap();
        assert!(json.contains("\"version\": 2"));

        // Load into fresh world via registry
        let parsed: SceneDataV2 = serde_json::from_str(&json).unwrap();
        let mut world2 = hecs::World::new();
        load_scene_v2(&parsed, &mut world2, &registry);

        // Re-save and verify
        let scene_v2_2 = save_scene_v2(&world2, &registry);
        assert_eq!(scene_v2_2.entities.len(), 2);

        let player = scene_v2_2
            .entities
            .iter()
            .find(|e| e.name == "Player")
            .unwrap();
        let t: Transform =
            serde_json::from_value(player.components["transform"].clone()).unwrap();
        assert!((t.position.x - 10.0).abs() < f32::EPSILON);
        assert!((t.rotation - 1.5).abs() < f32::EPSILON);

        let s: SpriteComponent =
            serde_json::from_value(player.components["sprite"].clone()).unwrap();
        assert_eq!(s.texture_id, "player.png");
        assert_eq!(s.z_order, 5);
    }

    #[test]
    fn scene_v1_to_v2_upgrade() {
        use crate::package::core_registration::register_core_components;
        use crate::package::component_registry::ComponentRegistry;

        let mut registry = ComponentRegistry::new();
        register_core_components(&mut registry);

        // Create a v1 scene
        let v1 = SceneData {
            version: 1,
            name: "Legacy Level".to_string(),
            post_process: None,
            extra: Default::default(),
            entities: vec![EntityData {
                children: Vec::new(),
                name: "OldEntity".to_string(),
                parent: None,
                components: ComponentBundle {
                    transform: Some(Transform {
                        position: Vec2::new(5.0, 10.0),
                        rotation: 0.0,
                        scale: Vec2::ONE,
                    }),
                    sprite: Some(SpriteComponent::default()),
                    ..Default::default()
                },
            }],
            sort_layers: None,
        };

        // Upgrade
        let v2 = upgrade_scene_v1_to_v2(&v1, &registry);
        assert_eq!(v2.version, 2);
        assert_eq!(v2.name, "Legacy Level");
        assert_eq!(v2.entities.len(), 1);
        assert!(v2.entities[0].components.contains_key("transform"));
        assert!(v2.entities[0].components.contains_key("sprite"));
    }

    #[test]
    fn scene_load_auto_v1() {
        use crate::package::core_registration::register_core_components;
        use crate::package::component_registry::ComponentRegistry;

        let mut registry = ComponentRegistry::new();
        register_core_components(&mut registry);

        // v1 JSON (no "version" field)
        let json = r#"{
            "name": "V1 Scene",
            "entities": [{
                "name": "A",
                "components": {
                    "transform": {"position": [1.0, 2.0], "rotation": 0.0, "scale": [1.0, 1.0]}
                }
            }]
        }"#;

        let mut world = hecs::World::new();
        load_scene_auto(json, &mut world, &registry).unwrap();
        assert_eq!(world.len(), 1);
    }

    #[test]
    fn scene_load_auto_v2() {
        use crate::package::core_registration::register_core_components;
        use crate::package::component_registry::ComponentRegistry;

        let mut registry = ComponentRegistry::new();
        register_core_components(&mut registry);

        // v2 JSON (has "version": 2)
        let json = r#"{
            "version": 2,
            "name": "V2 Scene",
            "entities": [{
                "name": "B",
                "components": {
                    "transform": {"position": [3.0, 4.0], "rotation": 0.0, "scale": [1.0, 1.0]}
                }
            }]
        }"#;

        let mut world = hecs::World::new();
        load_scene_auto(json, &mut world, &registry).unwrap();
        assert_eq!(world.len(), 1);

        // Verify the transform was loaded correctly
        for (_, transform) in world.query::<&Transform>().iter() {
            assert!((transform.position.x - 3.0).abs() < f32::EPSILON);
            assert!((transform.position.y - 4.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn a_custom_wrapper_is_detected_as_the_nesting_mistake() {
        // The shape scene 37 shipped with: an object of objects under
        // "custom". `warn_about_nested_custom` is what now says so.
        let mut custom = HashMap::new();
        custom.insert(
            "custom".to_string(),
            serde_json::json!({ "Stats": { "hp": 100 }, "Loot": { "gold": 5 } }),
        );
        // The function only logs, so this asserts the classification it uses:
        // an object of objects under the `custom` key is the mistake.
        let nested = custom.get("custom").unwrap();
        let inner: Vec<&str> = nested
            .as_object()
            .unwrap()
            .iter()
            .filter(|(_, v)| v.is_object())
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(inner.len(), 2, "both wrapped components are named");
        warn_about_nested_custom("Box1", &custom);
    }

    #[test]
    fn a_scalar_component_named_custom_is_left_alone() {
        // Someone may legitimately want a component called "custom"; only the
        // object-of-objects shape is the wrapper mistake.
        let mut custom = HashMap::new();
        custom.insert("custom".to_string(), serde_json::json!(42));
        warn_about_nested_custom("E", &custom);

        let mut flat = HashMap::new();
        flat.insert("custom".to_string(), serde_json::json!({ "hp": 1 }));
        // `hp` is a scalar, so this is a real component with real fields.
        warn_about_nested_custom("E", &flat);
    }

    #[test]
    fn correctly_authored_components_do_not_warn() {
        let mut custom = HashMap::new();
        custom.insert("Stats".to_string(), serde_json::json!({ "hp": 100 }));
        warn_about_nested_custom("E", &custom);
    }

    #[test]
    fn the_wrapper_mistake_loads_as_one_component_named_custom() {
        // The behaviour the warning exists to explain, pinned so nobody
        // "fixes" the loader into silently unwrapping it - that would change
        // what an existing scene means.
        let scene = r#"{"name":"p","entities":[{"name":"E","components":{
            "custom": { "Stats": { "hp": 100 } }
        }}]}"#;
        let mut reg = ComponentRegistry::new();
        crate::package::core_registration::register_core_components(&mut reg);
        let mut world = hecs::World::new();
        load_scene_auto(scene, &mut world, &reg).unwrap();
        let mut q = world.query::<&CustomComponents>();
        let (_, c) = q.iter().next().unwrap();
        assert!(c.0.contains_key("custom"));
        assert!(
            !c.0.contains_key("Stats"),
            "the wrapper is not unwrapped - that is why it needs a warning"
        );
    }

    #[test]
    fn scene_v2_custom_components_preserved() {
        use crate::package::core_registration::register_core_components;
        use crate::package::component_registry::ComponentRegistry;

        let mut registry = ComponentRegistry::new();
        register_core_components(&mut registry);

        // Create entity with a custom component
        let mut world = hecs::World::new();
        let mut customs = HashMap::new();
        customs.insert(
            "health".to_string(),
            serde_json::json!({"current": 100, "max": 100}),
        );
        world.spawn((
            Name("Player".to_string()),
            Transform::default(),
            CustomComponents(customs),
        ));

        // Save → the custom component should appear in the map
        let v2 = save_scene_v2(&world, &registry);
        let player = &v2.entities[0];
        assert!(player.components.contains_key("transform"));
        assert!(player.components.contains_key("health"));

        // Load back → custom component should go into CustomComponents
        let mut world2 = hecs::World::new();
        load_scene_v2(&v2, &mut world2, &registry);

        for (_, (_, custom)) in world2.query::<(&Transform, &CustomComponents)>().iter() {
            assert!(custom.0.contains_key("health"));
            assert_eq!(custom.0["health"]["current"], 100);
        }
    }

    #[test]
    fn behavior_graph_component_round_trips_through_scene_save_load() {
        // Slice 7.6 — `BehaviorGraphComponent` survives save → JSON →
        // load with `graph_path` + `overrides` intact, and lands on
        // the entity ready for `BehaviorGraphSystem::sync_instances`.
        use serde_json::json;

        let mut world = hecs::World::new();
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("speed".to_string(), json!(150.0));
        overrides.insert(
            "target".to_string(),
            json!({ "x": 1.0, "y": -2.0 }),
        );
        world.spawn((
            Name("Hero".to_string()),
            Transform::default(),
            BehaviorGraphComponent {
                graph_path: "graphs/player.behavior".to_string(),
                overrides: overrides.clone(),
            },
        ));

        let scene = save_scene(&world);
        let json_text = serde_json::to_string_pretty(&scene).expect("serialize");
        assert!(
            json_text.contains("behavior_graph"),
            "behavior_graph field must serialize, got:\n{json_text}"
        );
        assert!(
            json_text.contains("graphs/player.behavior"),
            "graph_path must serialize, got:\n{json_text}"
        );

        let parsed: SceneData = serde_json::from_str(&json_text).expect("deserialize");
        let mut loaded = hecs::World::new();
        load_scene(&parsed, &mut loaded);

        let mut found = 0usize;
        for (_e, bg) in loaded.query::<&BehaviorGraphComponent>().iter() {
            assert_eq!(bg.graph_path, "graphs/player.behavior");
            assert_eq!(bg.overrides.len(), 2);
            assert_eq!(bg.overrides.get("speed"), Some(&json!(150.0)));
            assert_eq!(
                bg.overrides.get("target"),
                Some(&json!({ "x": 1.0, "y": -2.0 })),
            );
            found += 1;
        }
        assert_eq!(found, 1, "expected exactly one BehaviorGraphComponent on load");
    }

    #[test]
    fn behavior_graph_component_omitted_when_absent() {
        // Slice 7.6 — entities without a BehaviorGraphComponent must
        // not emit a `behavior_graph: null` field (skip_serializing_if
        // contract; keeps existing `.scene` files diff-clean).
        let mut world = hecs::World::new();
        world.spawn((Name("Plain".to_string()), Transform::default()));

        let scene = save_scene(&world);
        let json_text = serde_json::to_string(&scene).expect("serialize");
        assert!(
            !json_text.contains("behavior_graph"),
            "absent component must not appear in JSON, got:\n{json_text}"
        );
    }

    // ── Sort layers 1.4 — regression tests ──

    #[test]
    fn sort_layer_round_trips_through_sprite_save_load() {
        // SpriteComponent.sort_layer must survive a save → JSON → load
        // cycle and land back on the entity. Guards against accidental
        // omission from save_scene/load_scene path or a stale serde
        // attribute.
        let mut world = hecs::World::new();
        world.spawn((
            Name("Hud".to_string()),
            Transform::default(),
            SpriteComponent {
                texture_id: "hud.png".into(),
                color: [1.0, 1.0, 1.0, 1.0],
                z_order: 3,
                size: None,
                uv_rect: None,
                anchor: [0.5, 0.5],
                fill: None,
                sort_layer: Some("ui".to_string()),
                region: None,
                region_resolved: false,
                corners: None,
                ..SpriteComponent::default()
            },
        ));

        let scene = save_scene(&world);
        let json = serde_json::to_string(&scene).expect("serialize");
        assert!(
            json.contains("\"sort_layer\""),
            "sort_layer must appear in JSON, got: {json}"
        );

        let parsed: SceneData = serde_json::from_str(&json).expect("parse");
        let sprite = parsed.entities[0]
            .components
            .sprite
            .as_ref()
            .expect("sprite missing after load");
        assert_eq!(sprite.sort_layer.as_deref(), Some("ui"));
    }

    #[test]
    fn sprite_without_sort_layer_omits_field_in_json() {
        // `sort_layer: None` must not pollute existing scenes with a
        // `"sort_layer": null` row (skip_serializing_if contract).
        let mut world = hecs::World::new();
        world.spawn((
            Name("Plain".to_string()),
            Transform::default(),
            SpriteComponent {
                texture_id: "p.png".into(),
                color: [1.0, 1.0, 1.0, 1.0],
                z_order: 0,
                size: None,
                uv_rect: None,
                anchor: [0.5, 0.5],
                fill: None,
                sort_layer: None,
                region: None,
                region_resolved: false,
                corners: None,
                ..SpriteComponent::default()
            },
        ));
        let scene = save_scene(&world);
        let json = serde_json::to_string(&scene).expect("serialize");
        assert!(
            !json.contains("sort_layer"),
            "absent sort_layer must not appear in JSON, got: {json}"
        );
    }

    #[test]
    fn scene_sort_layers_field_round_trips() {
        // `SceneData.sort_layers` (the active-set reference) must
        // survive save/load and also be discoverable via the
        // lightweight `read_scene_metadata` helper used by the runtime
        // before deserializing the full scene.
        let scene = SceneData {
            version: 1,
            name: "Level".into(),
            entities: vec![],
            post_process: None,
            sort_layers: Some("sortlayers/main.sortlayers".into()),
            extra: Default::default(),
        };
        let json = serde_json::to_string(&scene).expect("serialize");
        let parsed: SceneData = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed.sort_layers.as_deref(), Some("sortlayers/main.sortlayers"));

        let meta = read_scene_metadata(&json).expect("metadata");
        assert_eq!(meta.sort_layers.as_deref(), Some("sortlayers/main.sortlayers"));
    }

    #[test]
    fn scene_without_sort_layers_omits_field_in_json() {
        // Same diff-clean contract as components.
        let scene = SceneData::default();
        let json = serde_json::to_string(&scene).expect("serialize");
        assert!(
            !json.contains("sort_layers"),
            "absent sort_layers must not appear in JSON, got: {json}"
        );
    }

    #[test]
    fn named_sort_layer_outranks_higher_raw_z_order() {
        // Render-time interleave guarantee: a sprite tagged with a
        // foreground layer (`base_z = 1000`) must sort above a sprite
        // with no layer and a high raw `z_order = 999`. This is the
        // core promise of the sort_layers system — without it scenes
        // would have to rely on fragile magic z numbers.
        use crate::asset::sort_layers::{SortLayer, SortLayers};

        let layers = SortLayers::from_layers(vec![
            SortLayer { name: "bg".into(), base_z: -1000 },
            SortLayer { name: "default".into(), base_z: 0 },
            SortLayer { name: "fg".into(), base_z: 1000 },
        ]);

        // Three sprites; ordered low → high by effective z.
        let untagged_high = SpriteComponent {
            texture_id: "a".into(),
            color: [1.0; 4],
            z_order: 999,
            size: None,
            uv_rect: None,
            anchor: [0.5, 0.5],
            fill: None,
            sort_layer: None,
            region: None,
            region_resolved: false,
            corners: None,
            ..SpriteComponent::default()
        };
        let fg_low = SpriteComponent {
            texture_id: "b".into(),
            color: [1.0; 4],
            z_order: 0,
            size: None,
            uv_rect: None,
            anchor: [0.5, 0.5],
            fill: None,
            sort_layer: Some("fg".into()),
            region: None,
            region_resolved: false,
            corners: None,
            ..SpriteComponent::default()
        };
        let bg_high = SpriteComponent {
            texture_id: "c".into(),
            color: [1.0; 4],
            z_order: 999,
            size: None,
            uv_rect: None,
            anchor: [0.5, 0.5],
            fill: None,
            sort_layer: Some("bg".into()),
            region: None,
            region_resolved: false,
            corners: None,
            ..SpriteComponent::default()
        };

        let mut items = [&untagged_high, &fg_low, &bg_high];
        items.sort_by_key(|s| layers.resolve(s.sort_layer.as_deref(), s.z_order));
        let order: Vec<&str> =
            items.iter().map(|s| s.texture_id.as_str()).collect();
        assert_eq!(
            order,
            vec!["c", "a", "b"],
            "expected bg < untagged < fg (resolved z = -1 / 999 / 1000)"
        );
    }

    #[test]
    fn unknown_sort_layer_falls_back_to_z_order_only() {
        // Defensive: a scene that references a layer name not defined
        // in the active `.sortlayers` (e.g. user renamed it) must not
        // panic and must degrade to the raw `z_order` so the entity is
        // still visible somewhere sensible.
        use crate::asset::sort_layers::{SortLayer, SortLayers};
        let layers = SortLayers::from_layers(vec![SortLayer {
            name: "ui".into(),
            base_z: 5000,
        }]);
        let s = SpriteComponent {
            texture_id: "x".into(),
            color: [1.0; 4],
            z_order: 42,
            size: None,
            uv_rect: None,
            anchor: [0.5, 0.5],
            fill: None,
            sort_layer: Some("typo".into()),
            region: None,
            region_resolved: false,
            corners: None,
            ..SpriteComponent::default()
        };
        assert_eq!(layers.resolve(s.sort_layer.as_deref(), s.z_order), 42);
    }

    #[test]
    fn light_occluder_round_trips_through_scene_save_load() {
        // LightOccluder must survive a save → JSON → load cycle and
        // land back on the entity with its size/enabled intact.
        let mut world = hecs::World::new();
        world.spawn((
            Name("Wall".to_string()),
            Transform::default(),
            LightOccluder {
                enabled: true,
                size: [48.0, 12.0],
                from_sprite: true,
                polygon: vec![[0.0, 0.0], [16.0, 0.0], [8.0, 16.0]],
                shadow_tint: [0.8, 0.1, 0.1],
                shadow_opacity: 0.6,
                self_shadow: true,
                shadow_length: 120.0,
                shadows_casters: false,
                ..Default::default()
            },
        ));

        let scene = save_scene(&world);
        let json = serde_json::to_string(&scene).expect("serialize");
        assert!(
            json.contains("\"light_occluder\""),
            "light_occluder must appear in JSON, got: {json}"
        );

        let parsed: SceneData = serde_json::from_str(&json).expect("parse");
        let occ = parsed.entities[0]
            .components
            .light_occluder
            .as_ref()
            .expect("light_occluder missing after load");
        assert!(occ.enabled);
        assert_eq!(occ.size, [48.0, 12.0]);
        assert!(occ.from_sprite);
        assert_eq!(occ.polygon, vec![[0.0, 0.0], [16.0, 0.0], [8.0, 16.0]]);
        assert_eq!(occ.shadow_tint, [0.8, 0.1, 0.1]);
        assert_eq!(occ.shadow_opacity, 0.6);
        assert!(occ.self_shadow, "self_shadow must survive a round trip");
        assert_eq!(occ.shadow_length, 120.0, "shadow_length must survive a round trip");
    }
}
