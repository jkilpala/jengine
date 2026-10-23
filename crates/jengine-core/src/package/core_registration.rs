//! Core component registration — registers all built-in components with the
//! package system's `ComponentRegistry`.
//!
//! Called during engine initialization before any packages register their own
//! components, ensuring the core types are always available.

use super::component_registry::ComponentRegistry;

use crate::animation::skeletal::SkeletalPlayer;
use crate::animation::skinned_mesh::SkinnedMesh;
use crate::animation::{AnimStateMachine, SpriteAnimator};
use crate::ecs::components::{Mesh2D, Bone, IkLookAt, IkTwoBone, LightOccluder, SkeletonRoot,
    Acceleration2D, AmbientLight, AngularVelocity, AudioSource, BatchScriptComponent,
    BehaviorGraphComponent, BulkEntity, BounceArea, Camera2DComponent, CameraFollow, CharacterController2D, ClampBounds, Collider2D,
    DespawnOnDeath, DespawnOutOfBounds, Unlit, Drag2D, FaceVelocity, Flicker, FollowTarget, Force2D,
    Gravity2D, GuiComponent, Health, HealthBar, Hitbox, Impulse2D, InputActionImpulse, InputAxis2D,
    Joint2D,
    DespawnOffscreen, Lifetime, LightmapComponent, MaterialComponent, MaxSpeed, Obstacle, Oscillator, PatrolPathRef,
    PointLight2D, RigidBody2D, ScriptComponent, SeekTarget, Separation, Shake, ShadowCaster,
    SpotLight2D,
    SpriteComponent, Tags, TextComponent, TilemapCollider, TilemapOccluder, Torque2D, LightLayers, ColorLut, TouchControls, Transform, TriggerArea2D, Tween, VectorPath,
    Velocity2D, Wind2D, WrapBounds, PrefabInstance,
};
use crate::particles::ParticleEmitter;
use crate::tilemap::Tilemap;
use crate::ui::{
    UIButton, UIElement, UIImage, UIPanel, UIProgressBar, UISlider, UIText, UITextInput, UIToggle,
};

/// Register all built-in engine component types with the given registry.
///
/// This must be called before packages register their own components,
/// and before any scene loading that uses the registry.
pub fn register_core_components(registry: &mut ComponentRegistry) {
    // ── Core ──
    registry.register_defaulted::<Transform>("transform", "Transform", "Core");
    registry.register_defaulted::<Tags>("tags", "Tags", "Core");
    registry.register_defaulted::<CameraFollow>("camera_follow", "Camera Follow", "Core");
    registry.register_defaulted::<Camera2DComponent>("camera", "Camera 2D", "Core");

    // ── Rendering ──
    registry.register_defaulted::<SpriteComponent>("sprite", "Sprite", "Rendering");
    registry.register_defaulted::<Mesh2D>("mesh", "Mesh", "Rendering");
    registry.register_defaulted::<TextComponent>("text", "Text", "Rendering");
    registry.register::<Tilemap>("tilemap", "Tilemap", "Rendering");
    registry.register_defaulted::<MaterialComponent>("material", "Material", "Rendering");
    registry.register_defaulted::<crate::ecs::components::Trail>("trail", "Trail", "Rendering");

    // ── 2D Lighting ──
    registry.register_defaulted::<PointLight2D>("point_light", "Point Light 2D", "Lighting");
    registry.register_defaulted::<SpotLight2D>("spot_light", "Spot Light 2D", "Lighting");
    registry.register_defaulted::<crate::ecs::components::DirectionalLight2D>("directional_light", "Directional Light 2D", "Lighting");
    registry.register_defaulted::<AmbientLight>("ambient_light", "Ambient Light", "Lighting");
    registry.register_defaulted::<crate::ecs::components::AmbientOcclusion>("ambient_occlusion", "Ambient Occlusion", "Lighting");
    registry.register_defaulted::<crate::ecs::components::GlobalIllumination>("global_illumination", "Global Illumination", "Lighting");
    registry.register_defaulted::<crate::ecs::components::ToonLighting>("toon_lighting", "Toon Lighting", "Lighting");
    registry.register_defaulted::<LightmapComponent>("lightmap", "Lightmap", "Lighting");
    registry.register_defaulted::<ShadowCaster>("shadow_caster", "Shadow Caster", "Lighting");
    registry.register_defaulted::<LightOccluder>("light_occluder", "Light Occluder", "Lighting");

    // ── Animation ──
    registry.register_defaulted::<SpriteAnimator>("sprite_animator", "Sprite Animator", "Animation");
    registry.register_defaulted::<AnimStateMachine>("anim_state_machine", "Anim State Machine", "Animation");

    // ── Skeleton ──
    //
    // These were only ever in the v1 `ComponentBundle`, so a v2 scene - every
    // package game's - could not hold a rig, and the editor's Play snapshot
    // (which is v2) stripped one on Stop. BUGS #151.
    registry.register_defaulted::<SkeletonRoot>("skeleton_root", "Skeleton Root", "Skeleton");
    registry.register_defaulted::<Bone>("bone", "Bone", "Skeleton");
    registry.register_defaulted::<IkTwoBone>("ik_two_bone", "IK Two Bone", "Skeleton");
    registry.register_defaulted::<IkLookAt>("ik_look_at", "IK Look At", "Skeleton");
    registry.register_defaulted::<crate::ecs::components::IkChain>("ik_chain", "IK Chain", "Skeleton");
    registry.register_defaulted::<crate::ecs::components::SpriteCollider>("sprite_collider", "Sprite Collider", "Physics");
    registry.register_defaulted::<crate::ecs::components::Conveyor>("conveyor", "Conveyor", "Physics");
    registry.register_defaulted::<SkinnedMesh>("skinned_mesh", "Skinned Mesh", "Skeleton");
    registry.register_defaulted::<SkeletalPlayer>("skeletal_player", "Skeletal Player", "Skeleton");
    registry.register_defaulted::<crate::ecs::fracture::Breakable>("breakable", "Breakable", "Physics");
    registry.register_defaulted::<crate::ecs::cloth::Cloth>("cloth", "Cloth", "Rendering");
    registry.register_defaulted::<crate::ecs::cloth::Rope>("rope", "Rope", "Rendering");
    registry.register_defaulted::<crate::ecs::components::SpriteMask>("sprite_mask", "Sprite Mask", "Rendering");
    registry.register_defaulted::<crate::ecs::components::Masked>("masked", "Masked", "Rendering");
    registry.register_defaulted::<crate::particles::gpu::GpuParticles>("gpu_particles", "GPU Particles", "Rendering");
    registry.register_defaulted::<crate::ecs::components::RenderLayers>("render_layers", "Render Layers", "Rendering");
    registry.register_defaulted::<crate::tilemap::autotile::Autotile>("autotile", "Autotile", "Rendering");
    registry.register_defaulted::<crate::ecs::carve::Carvable>("carvable", "Carvable", "Rendering");
    registry.register_defaulted::<crate::ecs::paint::Paintable>("paintable", "Paintable", "Rendering");
    registry.register_defaulted::<crate::ecs::fluid::Fluid>("fluid", "Fluid", "Rendering");
    registry.register_defaulted::<crate::renderer::spline_shape::SplineShape>("spline_shape", "Spline Shape", "Rendering");
    registry.register_defaulted::<crate::ecs::area_effect::AreaEffect>("area_effect", "Area Effect", "Physics");
    registry.register_defaulted::<crate::animation::jiggle::Jiggle>("jiggle", "Jiggle", "Animation");
    registry.register_defaulted::<crate::ecs::path_follow::PathFollow>("path_follow", "Path Follow", "Behaviors");
    registry.register_defaulted::<crate::ecs::sorting_group::SortingGroup>("sorting_group", "Sorting Group", "Rendering");
    registry.register_defaulted::<crate::ecs::sprite_library::SpriteLibrary>("sprite_library", "Sprite Library", "Rendering");
    registry.register_defaulted::<crate::ecs::sprite_library::SpriteResolver>("sprite_resolver", "Sprite Resolver", "Rendering");
    registry.register_defaulted::<crate::renderer::line2d::Line2D>("line", "Line", "Rendering");
    registry.register_defaulted::<crate::ecs::canvas_group::CanvasGroup>("canvas_group", "Canvas Group", "Rendering");
    registry.register_defaulted::<crate::ecs::mirror::Mirror>("mirror", "Mirror", "Rendering");
    registry.register_defaulted::<crate::ecs::water::WaterSurface>("water_surface", "Water Surface", "Physics");
    registry.register_defaulted::<crate::ecs::post_volume::PostVolume>("post_volume", "Post Volume", "Rendering");
    registry.register_defaulted::<crate::ecs::pixel_sim::PixelSim>("pixel_sim", "Pixel Simulation", "Physics");
    registry.register_defaulted::<crate::ecs::smoke::Smoke>("smoke", "Smoke", "Physics");
    registry.register_defaulted::<crate::ecs::rewind::Rewind>("rewind", "Rewind Clock", "Gameplay");
    registry.register_defaulted::<crate::ecs::rewind::Rewindable>("rewindable", "Rewindable", "Gameplay");
    registry.register_defaulted::<crate::ecs::fog::Fog>("fog", "Fog", "Rendering");
    registry.register_defaulted::<crate::ecs::wfc::WfcGenerator>("wfc", "WFC Generator", "Tilemap");
    registry.register_defaulted::<crate::ecs::vision::Vision>("vision", "Vision", "Gameplay");
    registry.register_defaulted::<crate::ecs::day_cycle::DayCycle>("day_cycle", "Day Cycle", "Rendering");
    registry.register_defaulted::<crate::ecs::floating_origin::FloatingOrigin>("floating_origin", "Floating Origin", "Gameplay");
    registry.register_defaulted::<crate::ecs::time_scale::TimeScale>("time_scale", "Time Scale", "Gameplay");
    registry.register_defaulted::<crate::ecs::cast_shadow::CastShadow>("cast_shadow", "Cast Shadow", "Rendering");
    registry.register_defaulted::<crate::ecs::auto_normal::AutoNormal>("auto_normal", "Auto Normal", "Rendering");
    registry.register_defaulted::<crate::ecs::tile_light::TileLight>("tile_light", "Tile Light", "Rendering");
    registry.register_defaulted::<crate::ecs::tile_light::TileLightSource>("tile_light_source", "Tile Light Source", "Rendering");
    registry.register_defaulted::<crate::ecs::trajectory::Trajectory>("trajectory", "Trajectory", "Physics");
    registry.register_defaulted::<crate::ecs::text_path::TextPath>("text_path", "Text Path", "Rendering");
    registry.register_defaulted::<crate::ecs::beat_clock::BeatClock>("beat_clock", "Beat Clock", "Audio");
    registry.register_defaulted::<crate::ecs::displacement::DisplacementField>(
        "displacement_field",
        "Displacement Field",
        "Rendering",
    );
    registry.register_defaulted::<crate::ecs::displacement::Displacer>("displacer", "Displacer", "Rendering");
    registry.register_defaulted::<crate::ecs::displacement::FieldBend>("field_bend", "Field Bend", "Rendering");
    registry.register_defaulted::<crate::ecs::smooth_terrain::SmoothTerrain>("smooth_terrain", "Smooth Terrain", "Physics");
    registry.register_defaulted::<crate::ecs::fog_of_war::FogOfWar>("fog_of_war", "Fog of War", "Rendering");
    registry.register_defaulted::<crate::ecs::fog_of_war::Sight>("sight", "Sight", "Rendering");
    registry.register_defaulted::<crate::ecs::fog_of_war::FogHidden>("fog_hidden", "Fog Hidden", "Rendering");
    registry.register_defaulted::<crate::ecs::dialogue::Dialogue>("dialogue", "Dialogue", "Gameplay");
    registry.register_defaulted::<crate::ui::drag_drop::UIDraggable>("ui_draggable", "UI Draggable", "UI");
    registry.register_defaulted::<crate::ui::drag_drop::UIDropSlot>("ui_drop_slot", "UI Drop Slot", "UI");
    registry.register_defaulted::<crate::ecs::sprite_stack::SpriteStack>("sprite_stack", "Sprite Stack", "Rendering");
    registry.register_defaulted::<crate::ecs::mode7::Mode7>("mode7", "Mode 7 Plane", "Rendering");
    registry.register_defaulted::<crate::ecs::mode7::Mode7Sprite>("mode7_sprite", "Mode 7 Sprite", "Rendering");
    registry.register_defaulted::<crate::ecs::silhouette::Silhouette>("silhouette", "Silhouette", "Rendering");
    registry.register_defaulted::<crate::ecs::silhouette::SilhouetteOccluder>("silhouette_occluder", "Silhouette Occluder", "Rendering");
    registry.register_defaulted::<crate::ecs::sprite_tilt::SpriteTilt>("sprite_tilt", "Sprite Tilt", "Rendering");
    registry.register_defaulted::<crate::ecs::afterimage::Afterimage>("afterimage", "Afterimage", "Rendering");
    registry.register_defaulted::<crate::ecs::slice::Sliceable>("sliceable", "Sliceable", "Physics");
    registry.register_defaulted::<crate::ecs::portal::Portal>("portal", "Portal", "Rendering");
    registry.register_defaulted::<crate::ecs::jelly::Jelly>("jelly", "Jelly", "Rendering");
    registry.register_defaulted::<crate::ecs::offscreen_marker::OffscreenMarker>("offscreen_marker", "Off-screen Marker", "UI");
    registry.register_defaulted::<crate::ui::radial_menu::UIRadialMenu>("ui_radial_menu", "Radial Menu", "UI");
    registry.register_defaulted::<crate::ui::speech_bubble::SpeechBubble>("speech_bubble", "Speech Bubble", "UI");
    registry.register_defaulted::<crate::ui::toast::ToastStack>("toast_stack", "Toast Stack", "UI");
    registry.register_defaulted::<crate::ecs::sprite_outline::SpriteOutline>("sprite_outline", "Sprite Outline", "Rendering");
    registry.register_defaulted::<crate::ecs::lightning::Lightning>("lightning", "Lightning", "Rendering");
    registry.register_defaulted::<crate::ecs::flow::FlowPath>("flow_path", "Flow Path", "Rendering");
    registry.register_defaulted::<crate::ecs::nav_links::NavLink>("nav_link", "Nav Link", "Navigation");
    registry.register_defaulted::<crate::ecs::path_morph::PathMorph>("path_morph", "Path Morph", "Rendering");
    registry.register_defaulted::<crate::ecs::snow::SnowCover>("snow_cover", "Snow Cover", "Rendering");
    registry.register_defaulted::<crate::video::VideoPlayer>("video_player", "Video Player", "Rendering");
    registry.register_defaulted::<crate::ecs::visibility::VisibilityNotifier>(
        "visibility_notifier",
        "Visibility Notifier",
        "Behaviors",
    );
    registry.register_defaulted::<crate::renderer::freeform_light::FreeformLight>("freeform_light", "Freeform Light", "Rendering");
    registry.register_defaulted::<crate::audio::zones::AudioZone>("audio_zone", "Audio Zone", "Audio");
    registry.register_defaulted::<crate::navigation::NavRegion>("nav_region", "Nav Region", "Navigation");
    registry.register_defaulted::<crate::navigation::NavAgent>("nav_agent", "Nav Agent", "Navigation");
    registry.register_defaulted::<crate::navigation::NavObstacle>("nav_obstacle", "Nav Obstacle", "Navigation");
    registry.register_defaulted::<crate::ui::accessibility::Accessible>("accessible", "Accessible", "UI");
    registry.register_defaulted::<crate::audio::spatial::AudioListener>("audio_listener", "Audio Listener", "Audio");

    // ── Scripting ──
    registry.register::<ScriptComponent>("script", "Script", "Scripting");

    // ── Physics ──
    registry.register_defaulted::<RigidBody2D>("rigid_body", "Rigid Body 2D", "Physics");
    registry.register_defaulted::<Collider2D>("collider", "Collider 2D", "Physics");
    registry.register_defaulted::<Joint2D>("joint", "Joint 2D", "Physics");
    registry.register_defaulted::<CharacterController2D>("character_controller", "Character Controller 2D", "Physics");
    registry.register_defaulted::<TilemapCollider>("tilemap_collider", "Tilemap Collider", "Physics");
    registry.register_defaulted::<TilemapOccluder>("tilemap_occluder", "Tilemap Occluder", "Lighting");
    registry.register_defaulted::<LightLayers>("light_layers", "Light Layers", "Lighting");
    registry.register_defaulted::<ColorLut>("color_lut", "Color LUT", "Rendering");
    registry.register_defaulted::<crate::ecs::ragdoll::Ragdoll>("ragdoll", "Ragdoll", "Physics");
    registry.register_defaulted::<crate::ecs::timeline::Timeline>("timeline", "Timeline", "Animation");
    registry.register_defaulted::<crate::ecs::behavior_tree::BehaviorTree>("behavior_tree", "Behavior Tree", "AI");

    // ── Audio ──
    registry.register_defaulted::<AudioSource>("audio_source", "Audio Source", "Audio");

    // ── Effects ──
    registry.register_defaulted::<ParticleEmitter>("particle_emitter", "Particle Emitter", "Effects");

    // ── UI (immediate-mode widgets) ──
    registry.register_defaulted::<UIElement>("ui_element", "UI Element", "UI");
    registry.register_defaulted::<UIText>("ui_text", "UI Text", "UI");
    registry.register_defaulted::<UIImage>("ui_image", "UI Image", "UI");
    registry.register_defaulted::<UIPanel>("ui_panel", "UI Panel", "UI");
    registry.register_defaulted::<UIButton>("ui_button", "UI Button", "UI");
    registry.register_defaulted::<UIProgressBar>("ui_progress_bar", "UI Progress Bar", "UI");
    registry.register_defaulted::<UISlider>("ui_slider", "UI Slider", "UI");
    registry.register_defaulted::<UIToggle>("ui_toggle", "UI Toggle", "UI");
    registry.register_defaulted::<UITextInput>("ui_text_input", "UI Text Input", "UI");

    // ── GUI (document-based) ──
    registry.register_defaulted::<GuiComponent>("gui", "GUI", "UI");
    registry.register_defaulted::<TouchControls>("touch_controls", "Touch Controls", "UI");

    // ── Behaviors (Step 2D — native data-driven motion) ──
    registry.register_defaulted::<Velocity2D>("velocity", "Velocity 2D", "Behaviors");
    registry.register_defaulted::<BounceArea>("bounce_area", "Bounce Area", "Behaviors");
    registry.register_defaulted::<Gravity2D>("gravity", "Gravity 2D", "Behaviors");
    registry.register_defaulted::<Lifetime>("lifetime", "Lifetime", "Behaviors");
    registry.register_defaulted::<DespawnOffscreen>("despawn_offscreen", "Despawn Offscreen", "Behaviors");
    registry.register_defaulted::<Acceleration2D>("acceleration", "Acceleration 2D", "Behaviors");
    registry.register_defaulted::<Drag2D>("drag2d", "Drag 2D", "Behaviors");
    registry.register_defaulted::<Force2D>("force2d", "Force 2D", "Behaviors");
    registry.register_defaulted::<Wind2D>("wind2d", "Wind 2D", "Behaviors");
    registry.register_defaulted::<Impulse2D>("impulse2d", "Impulse 2D", "Behaviors");
    registry.register_defaulted::<Torque2D>("torque2d", "Torque 2D", "Behaviors");
    registry.register_defaulted::<MaxSpeed>("max_speed", "Max Speed", "Behaviors");
    registry.register_defaulted::<Oscillator>("oscillator", "Oscillator", "Behaviors");
    registry.register_defaulted::<FollowTarget>("follow_target", "Follow Target", "Behaviors");
    registry.register_defaulted::<SeekTarget>("seek_target", "Seek Target", "Behaviors");
    registry.register_defaulted::<Tween>("tween", "Tween", "Behaviors");
    registry.register_defaulted::<PatrolPathRef>("patrol_path", "Patrol Path", "Behaviors");
    registry.register_defaulted::<VectorPath>("vector_path", "Vector Path", "Behaviors");
    registry.register_defaulted::<BatchScriptComponent>("batch_script", "Batch Script", "Behaviors");
    registry.register_defaulted::<InputAxis2D>("input_axis", "Input Axis 2D", "Behaviors");
    registry.register_defaulted::<InputActionImpulse>("input_action_impulse", "Input Action Impulse", "Behaviors");
    registry.register_defaulted::<TriggerArea2D>("trigger_area", "Trigger Area 2D", "Behaviors");
    registry.register_defaulted::<BehaviorGraphComponent>("behavior_graph", "Behavior Graph", "Behaviors");

    // ── Combat (data-driven health + trigger hitboxes) ──
    registry.register_defaulted::<Health>("health", "Health", "Combat");
    registry.register_defaulted::<Hitbox>("hitbox", "Hitbox", "Combat");
    registry.register_defaulted::<DespawnOnDeath>("despawn_on_death", "Despawn On Death", "Combat");
    registry.register_defaulted::<Unlit>("unlit", "Unlit", "Rendering");
    registry.register_defaulted::<HealthBar>("health_bar", "Health Bar", "Combat");
    registry.register_defaulted::<Obstacle>("obstacle", "Obstacle", "Combat");
    registry.register_defaulted::<Separation>("separation", "Separation", "Combat");

    // ── Performance hints ──
    registry.register_defaulted::<BulkEntity>("bulk_entity", "Bulk Entity", "Performance");

    // ── Behaviors (Step 6 reopen — broader native vocabulary) ──
    registry.register_defaulted::<WrapBounds>("wrap_bounds", "Wrap Bounds", "Behaviors");
    registry.register_defaulted::<ClampBounds>("clamp_bounds", "Clamp Bounds", "Behaviors");
    registry.register_defaulted::<DespawnOutOfBounds>(
        "despawn_out_of_bounds",
        "Despawn Out Of Bounds",
        "Behaviors",
    );
    registry.register_defaulted::<FaceVelocity>("face_velocity", "Face Velocity", "Behaviors");
    registry.register_defaulted::<AngularVelocity>("angular_velocity", "Angular Velocity", "Behaviors");
    registry.register_defaulted::<Flicker>("flicker", "Flicker", "Behaviors");
    registry.register_defaulted::<Shake>("shake", "Shake", "Behaviors");

    // ── Prefab variants (2.2) ──
    registry.register_defaulted::<PrefabInstance>("prefab_instance", "Prefab Instance", "Core");

    log::info!("Registered {} core components", registry.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every component id `register_core_components` installs.
    ///
    /// **Adding a component means adding it here.** The editor spawns
    /// generically through this registry, so a missed registration is
    /// invisible until a scene silently drops the component on load.
    ///
    /// This used to be a bare `assert_eq!(registry.len(), 60)`, which went
    /// stale by seven entries and told nobody which seven. Naming them makes
    /// the failure say exactly what drifted.
    const EXPECTED_CORE_COMPONENTS: &[&str] = &[
    "acceleration", "ambient_light", "breakable", "cloth", "rope", "sprite_mask", "masked", "gpu_particles", "render_layers", "autotile", "carvable", "paintable", "fluid", "spline_shape", "area_effect", "nav_region", "nav_agent", "nav_obstacle", "accessible", "behavior_tree", "audio_zone", "freeform_light", "jiggle", "visibility_notifier", "path_follow", "sorting_group", "line", "canvas_group", "mirror", "water_surface", "post_volume", "pixel_sim", "smoke", "rewind", "rewindable", "fog", "wfc", "vision", "day_cycle", "floating_origin", "time_scale", "cast_shadow", "auto_normal", "tile_light", "tile_light_source", "trajectory", "text_path", "beat_clock", "displacement_field", "displacer", "field_bend", "smooth_terrain", "fog_of_war", "sight", "fog_hidden", "dialogue", "ui_draggable", "ui_drop_slot", "sprite_stack", "mode7", "mode7_sprite", "silhouette", "silhouette_occluder", "sprite_tilt", "afterimage", "sliceable", "portal", "jelly", "offscreen_marker", "ui_radial_menu", "speech_bubble", "toast_stack", "sprite_outline", "lightning", "flow_path", "nav_link", "path_morph", "snow_cover", "video_player", "audio_listener", "ambient_occlusion", "global_illumination", "angular_velocity", "audio_source",
    "anim_state_machine", "bone", "ik_look_at", "ik_two_bone", "ik_chain", "sprite_collider", "conveyor", "sprite_library", "sprite_resolver", "light_occluder", "light_layers", "color_lut", "ragdoll", "timeline",
    "skeletal_player", "skeleton_root", "skinned_mesh",
    "batch_script", "behavior_graph", "bounce_area", "camera",
    "camera_follow", "character_controller", "clamp_bounds", "collider", "despawn_on_death", "unlit",
    "despawn_out_of_bounds", "despawn_offscreen", "directional_light", "drag2d", "face_velocity", "flicker",
    "follow_target", "force2d", "gravity", "gui", "touch_controls",
    "health", "health_bar", "hitbox", "impulse2d",
    "input_action_impulse", "input_axis", "joint", "lifetime",
    "lightmap", "material", "max_speed", "bulk_entity", "obstacle",
    "oscillator", "particle_emitter", "patrol_path", "point_light",
    "prefab_instance", "rigid_body", "script", "seek_target",
    "separation", "shadow_caster", "shake", "spot_light",
    "mesh", "sprite", "sprite_animator", "tags", "text",
    "tilemap", "tilemap_collider", "tilemap_occluder", "toon_lighting", "torque2d", "trail", "transform", "trigger_area",
    "tween", "ui_button", "ui_element", "ui_image",
    "ui_panel", "ui_progress_bar", "ui_slider", "ui_text",
    "ui_text_input", "ui_toggle", "vector_path", "velocity",
    "wind2d", "wrap_bounds",
    ];

    #[test]
    fn register_all_core_components() {
        let mut registry = ComponentRegistry::new();
        register_core_components(&mut registry);

        let registered: std::collections::BTreeSet<&str> =
            registry.iter().map(|d| d.name.as_str()).collect();
        let expected: std::collections::BTreeSet<&str> =
            EXPECTED_CORE_COMPONENTS.iter().copied().collect();
        let missing: Vec<&&str> = expected.difference(&registered).collect();
        let unlisted: Vec<&&str> = registered.difference(&expected).collect();
        assert!(
            missing.is_empty() && unlisted.is_empty(),
            "core component registry drifted - not registered: {missing:?}, \
             registered but not in EXPECTED_CORE_COMPONENTS: {unlisted:?}"
        );
        // Registering two components under one id would silently overwrite:
        // `descriptors` is keyed by id, so the set above would still match.
        assert_eq!(registry.len(), EXPECTED_CORE_COMPONENTS.len());

        // Spot-check some lookups
        assert!(registry.get("transform").is_some());
        assert!(registry.get("sprite").is_some());
        assert!(registry.get("rigid_body").is_some());
        assert!(registry.get("gui").is_some());
        assert!(registry.get("ui_button").is_some());
        assert!(registry.get("tilemap").is_some());
        assert!(registry.get("audio_source").is_some());
        // Step 2D behaviors
        assert!(registry.get("oscillator").is_some());
        assert!(registry.get("tween").is_some());
        assert!(registry.get("patrol_path").is_some());

        // Verify name_of reverse lookup
        assert_eq!(registry.name_of::<Transform>(), Some("transform"));
        assert_eq!(registry.name_of::<SpriteComponent>(), Some("sprite"));
    }

    #[test]
    fn core_components_serialize_roundtrip() {
        let mut registry = ComponentRegistry::new();
        register_core_components(&mut registry);

        // Create a world with some components
        let mut world = hecs::World::new();
        let entity = world.spawn((
            Transform::default(),
            SpriteComponent::default(),
            Tags(["player".to_string()].into_iter().collect()),
        ));

        // Serialize via registry
        let components = registry.serialize_entity(&world, entity);
        assert!(components.contains_key("transform"));
        assert!(components.contains_key("sprite"));
        assert!(components.contains_key("tags"));
        assert!(!components.contains_key("rigid_body")); // not on this entity

        // Deserialize into a new entity
        let mut builder = hecs::EntityBuilder::new();
        let unknown = registry.deserialize_into_builder(&mut builder, &components);
        assert!(unknown.is_empty());

        let mut world2 = hecs::World::new();
        let entity2 = world2.spawn(builder.build());

        // Verify components exist
        assert!(world2.get::<&Transform>(entity2).is_ok());
        assert!(world2.get::<&SpriteComponent>(entity2).is_ok());
        assert!(world2.get::<&Tags>(entity2).is_ok());
    }
}
