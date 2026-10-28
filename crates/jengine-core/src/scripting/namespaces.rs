//! Script functions in namespaces: `Sprite.set_color(e, 1, 0, 0, 1)`.
//!
//! **The one list.** Every runtime builds its namespace tables from here -
//! the Luau adapter directly, the editor and web builds from the copy their
//! hosts publish on `__jengine_play.__script_namespaces` - so a function
//! cannot be `Sprite.set_color` in one runtime and missing in another. The
//! hand-written lowercase tables this replaces (`sprite.get_color` and
//! friends, in the browser preamble only) were exactly that failure: native
//! never had them, and the editor's returned a table where native returns
//! four numbers, because the multi-return rule is keyed by the flat name.
//!
//! Each entry points at a flat global that already exists, and the flat name
//! keeps working. The namespaced name is the documented one; shipped game
//! scripts use it (`every_shipped_script_uses_namespaced_names` holds them to
//! it), and the flat globals stay for Rust test snippets and for JavaScript
//! scripts, which keep the flat API.
//!
//! **Why capitalised.** The lowercase names collide with the variables games
//! use: `entity` is the parameter of nearly every callback, so an `entity`
//! namespace is hidden inside `update(entity, dt)`; `text`, `camera`,
//! `timer`, `data` and `gui` are common locals; `script` is the header table
//! every extending script assigns; and `math` / `debug` are Lua's own
//! libraries, which native's sandbox makes read-only. See
//! `docs/script-namespaces-plan.md`.
//!
//! **Packages** get one namespace each, from the crate name (`jengine-example`
//! is `Example`). Where every command a package registers shares a prefix, the
//! prefix goes: `example_talk` is `Example.talk`. [`package_entries`] is the rule.

use std::collections::{BTreeMap, BTreeSet};

use crate::package::script_commands::PackageCommandInfo;

/// `(namespace, short name, flat global)` for every core script function.
pub const CORE: &[(&str, &str, &str)] = &[
    // Entity
    ("Entity", "spawn", "spawn_entity"),
    ("Entity", "spawn_prefab", "spawn_prefab"),
    ("Entity", "destroy", "destroy_entity"),
    ("Entity", "find_by_name", "find_by_name"),
    ("Entity", "find_child", "find_child"),
    ("Entity", "find_by_tag", "find_by_tag"),
    ("Entity", "find_in_radius", "find_in_radius"),
    ("Entity", "get_name", "get_name"),
    ("Entity", "add_tag", "add_tag"),
    ("Entity", "remove_tag", "remove_tag"),
    ("Entity", "has_tag", "has_tag"),
    ("Entity", "get_custom", "get_custom"),
    ("Entity", "set_custom", "set_custom"),
    ("Entity", "set_fields", "set_fields"),
    ("Entity", "get_health", "get_health"),
    ("Entity", "set_health", "set_health"),
    ("Entity", "set_hitbox", "set_hitbox"),
    ("Entity", "set_parent", "set_parent"),
    // Transform
    ("Transform", "get_position", "get_position"),
    ("Transform", "set_position", "set_position"),
    ("Transform", "get_rotation", "get_rotation"),
    ("Transform", "set_rotation", "set_rotation"),
    ("Transform", "get_scale", "get_scale"),
    ("Transform", "set_scale", "set_scale"),
    ("Transform", "get_world_position", "get_world_position"),
    ("Transform", "get_world_rotation", "get_world_rotation"),
    ("Transform", "get_world_scale", "get_world_scale"),
    // Sprite
    ("Sprite", "get_color", "get_sprite_color"),
    ("Sprite", "set_color", "set_sprite_color"),
    ("Sprite", "get_anchor", "get_sprite_anchor"),
    ("Sprite", "set_anchor", "set_sprite_anchor"),
    ("Sprite", "set_size", "set_sprite_size"),
    ("Sprite", "carve", "carve_sprite"),
    ("Sprite", "paint", "paint_sprite"),
    ("Sprite", "press", "press_sprite"),
    ("Sprite", "get_point", "get_sprite_point"),
    ("Sprite", "set_region", "set_sprite_region"),
    ("Sprite", "set_label", "set_sprite_label"),
    ("Sprite", "set_corners", "set_sprite_corners"),
    ("Sprite", "set_z_order", "set_z_order"),
    // Material
    ("Material", "get", "get_material"),
    ("Material", "set", "set_material"),
    ("Material", "set_normal_map", "set_normal_map"),
    ("Material", "set_float", "set_material_uniform_float"),
    ("Material", "set_vec2", "set_material_uniform_vec2"),
    ("Material", "set_vec4", "set_material_uniform_vec4"),
    ("Material", "set_int", "set_material_uniform_int"),
    // Text
    ("Text", "set", "set_text"),
    ("Text", "set_color", "set_text_color"),
    // Trail
    ("Trail", "set_emitting", "set_trail_emitting"),
    ("Trail", "clear", "clear_trail"),
    // Mesh
    ("Mesh", "set", "set_mesh"),
    // Animation
    ("Animation", "play", "play_animation"),
    ("Animation", "crossfade", "crossfade_animation"),
    ("Animation", "set_blend", "set_animation_blend"),
    ("Animation", "set_blend_position", "set_animation_blend_position"),
    ("Animation", "play_layer", "animation_play_layer"),
    ("Animation", "stop_layer", "animation_stop_layer"),
    ("Animation", "set_layer_weight", "animation_set_layer_weight"),
    ("Animation", "stop", "stop_animation"),
    ("Animation", "set_speed", "set_animation_speed"),
    ("Animation", "is_playing", "is_animation_playing"),
    ("Animation", "get_current", "get_current_animation"),
    ("Animation", "get_time", "get_animation_time"),
    ("Animation", "set_time", "set_animation_time"),
    ("Animation", "set_bool", "set_anim_bool"),
    ("Animation", "set_float", "set_anim_float"),
    ("Animation", "set_trigger", "set_anim_trigger"),
    ("Animation", "start_state_machine", "start_state_machine"),
    ("Animation", "get_state", "get_anim_state"),
    // Skeleton
    ("Skeleton", "find", "find_skeleton"),
    ("Skeleton", "find_bone", "find_bone"),
    ("Skeleton", "set_ik_target", "set_ik_target"),
    ("Skeleton", "set_ik_weight", "set_ik_weight"),
    ("Skeleton", "spawn_rig", "spawn_rig"),
    // Tween
    ("Tween", "position", "tween_position"),
    ("Tween", "rotation", "tween_rotation"),
    ("Tween", "scale", "tween_scale"),
    ("Tween", "color", "tween_color"),
    ("Tween", "cancel", "cancel_tween"),
    ("Tween", "cancel_all", "cancel_all_tweens"),
    // Timer
    ("Timer", "after", "set_timer"),
    ("Timer", "every", "set_interval"),
    ("Timer", "cancel", "cancel_timer"),
    // Event
    ("Event", "on", "on_event"),
    ("Event", "send", "send_event"),
    ("Event", "send_to", "send_event_to"),
    ("Event", "off", "remove_event_listener"),
    // Input
    ("Input", "is_key_pressed", "is_key_pressed"),
    ("Input", "is_key_just_pressed", "is_key_just_pressed"),
    ("Input", "is_action_pressed", "is_action_pressed"),
    ("Input", "is_action_just_pressed", "is_action_just_pressed"),
    ("Input", "is_action_just_released", "is_action_just_released"),
    ("Input", "get_action_axis", "get_action_axis"),
    ("Input", "get_mouse_position", "get_mouse_position"),
    ("Input", "get_mouse_world_position", "get_mouse_world_position"),
    ("Input", "get_mouse_wheel", "get_mouse_wheel"),
    ("Input", "get_mouse_delta", "get_mouse_delta"),
    ("Input", "get_touches", "get_touches"),
    ("Input", "get_gestures", "get_gestures"),
    ("Input", "is_mouse_left_pressed", "is_mouse_left_pressed"),
    ("Input", "is_mouse_left_just_pressed", "is_mouse_left_just_pressed"),
    ("Input", "is_mouse_left_just_released", "is_mouse_left_just_released"),
    ("Input", "is_mouse_right_pressed", "is_mouse_right_pressed"),
    ("Input", "is_mouse_right_just_pressed", "is_mouse_right_just_pressed"),
    ("Input", "is_mouse_right_just_released", "is_mouse_right_just_released"),
    ("Input", "is_mouse_middle_pressed", "is_mouse_middle_pressed"),
    ("Input", "is_mouse_middle_just_pressed", "is_mouse_middle_just_pressed"),
    ("Input", "is_mouse_middle_just_released", "is_mouse_middle_just_released"),
    ("Input", "get_gamepad_count", "get_gamepad_count"),
    ("Input", "is_gamepad_connected", "is_gamepad_connected"),
    ("Input", "get_gamepad_axis", "get_gamepad_axis"),
    ("Input", "is_gamepad_button_pressed", "is_gamepad_button_pressed"),
    ("Input", "get_actions", "input_get_actions"),
    ("Input", "get_bindings", "input_get_bindings"),
    ("Input", "get_just_pressed", "input_get_just_pressed"),
    ("Input", "get_typed", "input_get_typed"),
    ("Input", "set_bindings", "input_set_bindings"),
    ("Input", "add_binding", "input_add_binding"),
    ("Input", "remove_binding", "input_remove_binding"),
    ("Input", "clear_bindings", "input_clear_bindings"),
    ("Input", "reset_bindings", "input_reset_bindings"),
    ("Input", "get_dead_zone", "input_get_dead_zone"),
    ("Input", "set_dead_zone", "input_set_dead_zone"),
    ("Input", "get_hold_time", "get_action_hold_time"),
    ("Input", "is_action_held", "is_action_held"),
    ("Input", "is_action_tapped", "is_action_tapped"),
    ("Input", "is_action_double_tapped", "is_action_double_tapped"),
    ("Input", "get_gamepad_name", "get_gamepad_name"),
    ("Input", "get_gamepad_type", "get_gamepad_type"),
    ("Input", "get_last_device", "get_last_input_device"),
    ("Input", "get_last_gamepad", "get_last_gamepad"),
    ("Input", "get_prompt", "get_action_prompt"),
    // Physics
    ("Physics", "apply_force", "apply_force"),
    ("Physics", "apply_impulse", "apply_impulse"),
    ("Physics", "get_velocity", "get_velocity"),
    ("Physics", "set_velocity", "set_velocity"),
    ("Physics", "get_angular_velocity", "get_angular_velocity"),
    ("Physics", "set_angular_velocity", "set_angular_velocity"),
    ("Physics", "get_gravity", "get_gravity"),
    ("Physics", "set_gravity", "set_gravity"),
    ("Physics", "set_body_type", "set_body_type"),
    ("Physics", "set_conveyor_speed", "set_conveyor_speed"),
    ("Physics", "raycast", "raycast"),
    ("Physics", "slice", "physics_slice"),
    ("Physics", "overlap_circle", "overlap_circle"),
    ("Physics", "overlap_box", "overlap_box"),
    ("Physics", "overlap_point", "overlap_point"),
    ("Physics", "circle_cast", "circle_cast"),
    ("Physics", "get_contacts", "get_contacts"),
    ("Physics", "contact_count", "contact_count"),
    ("Physics", "get_contact", "get_contact"),
    ("Physics", "add_joint", "add_joint"),
    ("Physics", "remove_joint", "remove_joint"),
    ("Physics", "set_motor_speed", "set_motor_speed"),
    ("Physics", "move_character", "move_character"),
    ("Physics", "get_character", "get_character"),
    // Audio
    ("Audio", "play_sound", "play_sound"),
    ("Audio", "play_sound_at", "play_sound_at"),
    ("Audio", "stop_sound", "stop_sound"),
    ("Audio", "stop_sound_by_clip", "stop_sound_by_clip"),
    ("Audio", "set_sound_volume", "set_sound_volume"),
    ("Audio", "play_music", "play_music"),
    ("Audio", "stop_music", "stop_music"),
    ("Audio", "pause_music", "pause_music"),
    ("Audio", "resume_music", "resume_music"),
    ("Audio", "set_music_volume", "set_music_volume"),
    ("Audio", "get_music_position", "get_music_position"),
    ("Audio", "set_master_volume", "set_master_volume"),
    ("Audio", "stop_all", "stop_all_sounds"),
    ("Audio", "pause_all", "pause_all_sounds"),
    ("Audio", "resume_all", "resume_all_sounds"),
    ("Audio", "create_bus", "audio_create_bus"),
    ("Audio", "remove_bus", "audio_remove_bus"),
    ("Audio", "get_bus_volume", "audio_get_bus_volume"),
    ("Audio", "set_bus_volume", "audio_set_bus_volume"),
    ("Audio", "set_bus_effect", "audio_set_bus_effect"),
    ("Audio", "set_variation", "audio_set_variation"),
    ("Audio", "follow_time", "audio_follow_time"),
    ("Audio", "crossfade_music", "audio_crossfade_music"),
    ("Audio", "play_music_layers", "audio_play_music_layers"),
    ("Audio", "set_music_layer", "audio_set_music_layer"),
    ("Audio", "get_music_layer", "audio_get_music_layer"),
    ("Audio", "set_playlist", "audio_set_playlist"),
    ("Audio", "next_track", "audio_next_track"),
    ("Audio", "get_music_clip", "audio_get_music_clip"),
    ("Audio", "set_music_pitch", "audio_set_music_pitch"),
    ("Audio", "seek_music", "audio_seek_music"),
    ("Audio", "set_sound_pitch", "audio_set_sound_pitch"),
    ("Audio", "is_bus_muted", "audio_is_bus_muted"),
    ("Audio", "set_bus_muted", "audio_set_bus_muted"),
    ("Audio", "set_bus_solo", "audio_set_bus_solo"),
    // Scene
    ("Scene", "load", "load_scene"),
    ("Scene", "transition", "scene_transition"),
    ("Scene", "push", "push_scene"),
    ("Scene", "pop", "pop_scene"),
    ("Scene", "load_additive", "scene_load_additive"),
    ("Scene", "unload", "scene_unload"),
    ("Scene", "preload", "scene_preload"),
    ("Scene", "status", "scene_status"),
    ("Scene", "progress", "scene_progress"),
    // Asset packs, downloaded when needed (`asset::packs`)
    ("Packs", "download", "pack_download"),
    ("Packs", "remove", "pack_remove"),
    ("Packs", "set_url", "pack_set_url"),
    ("Packs", "status", "pack_status"),
    ("Packs", "progress", "pack_progress"),
    ("Packs", "size", "pack_size"),
    ("Packs", "error", "pack_error"),
    ("Packs", "of", "pack_of"),
    ("Packs", "list", "pack_list"),
    ("LiveOps", "get", "liveops_get"),
    ("LiveOps", "keys", "liveops_keys"),
    ("LiveOps", "status", "liveops_status"),
    ("LiveOps", "error", "liveops_error"),
    ("LiveOps", "fetch", "liveops_fetch"),
    ("LiveOps", "fetching", "liveops_fetching"),
    ("LiveOps", "variant", "liveops_variant"),
    ("LiveOps", "events", "liveops_events"),
    ("LiveOps", "event_active", "liveops_event_active"),
    ("LiveOps", "event_ends", "liveops_event_ends"),
    ("LiveOps", "event_starts", "liveops_event_starts"),
    ("LiveOps", "event_name", "liveops_event_name"),
    ("LiveOps", "set_player", "liveops_set_player"),
    ("LiveOps", "server_time", "liveops_server_time"),
    ("LiveOps", "revision", "liveops_revision"),
    ("LiveOps", "release", "liveops_release"),
    ("LiveOps", "updated", "liveops_updated"),
    ("LiveOps", "expose", "liveops_expose"),
    ("LiveOps", "set_attribute", "liveops_set_attribute"),
    ("Scene", "get_name", "get_current_scene_name"),
    ("Scene", "quit", "quit_game"),
    ("Scene", "fade_in", "fade_in"),
    ("Scene", "fade_out", "fade_out"),
    // Camera
    ("Camera", "shake", "camera_shake"),
    ("Camera", "flash", "screen_flash"),
    ("Camera", "get_zoom", "get_camera_zoom"),
    ("Camera", "get_position", "get_camera_position"),
    ("Camera", "set_follow", "set_camera_follow"),
    ("Camera", "set", "set_camera_field"),
    ("Camera", "set_screen_rect", "set_camera_screen_rect"),
    ("Camera", "set_zoom", "set_camera_zoom"),
    ("Camera", "set_pixel_size", "set_camera_pixel_size"),
    ("Camera", "get_screen_size", "get_screen_size"),
    // Time
    ("Time", "get_delta", "get_delta_time"),
    ("Time", "get_unix", "get_unix_time"),
    ("Time", "set_scale", "set_time_scale"),
    ("Time", "get_scale", "get_time_scale"),
    // Tilemap
    ("Tilemap", "get_tile", "get_tile"),
    ("Tilemap", "get_tile_data", "get_tile_data"),
    ("Tilemap", "set_tile", "set_tile"),
    ("Tilemap", "get_tile_at_world", "get_tile_at_world"),
    ("Tilemap", "world_to_tile", "world_to_tile"),
    ("Tilemap", "tile_to_world", "tile_to_world"),
    ("Tilemap", "find_path", "tilemap_find_path"),
    ("Tilemap", "find_path_world", "tilemap_find_path_world"),
    ("Tilemap", "neighbors", "tilemap_neighbors"),
    ("Tilemap", "distance", "tilemap_distance"),
    // Particles
    ("Particles", "emit_burst", "emit_burst"),
    ("Particles", "set_active", "set_emitter_active"),
    ("Particles", "set_rate", "set_emitter_rate"),
    ("Particles", "set", "set_emitter_field"),
    ("Particles", "get_count", "get_particle_count"),
    // Cloth and rope
    ("Cloth", "set", "set_cloth_field"),
    ("PathFollow", "set", "set_path_follow_field"),
    ("Line", "set", "set_line_field"),
    ("Trail", "set", "set_trail_field"),
    ("Fluid", "set", "set_fluid_field"),
    ("Mirror", "set", "set_mirror_field"),
    ("Sprite", "set_fill", "set_sprite_fill"),
    ("PixelSim", "paint", "pixel_paint"),
    ("Smoke", "puff", "smoke_puff"),
    ("Smoke", "set", "set_smoke_field"),
    ("Rewind", "set", "set_rewind_field"),
    ("Fog", "set", "set_fog_field"),
    ("Wfc", "generate", "wfc_generate"),
    ("Vision", "set", "set_vision_field"),
    ("DayCycle", "set", "set_day_field"),
    ("FloatingOrigin", "set", "set_origin_field"),
    ("TimeScale", "set", "set_time_scale_field"),
    ("CastShadow", "set", "set_cast_shadow_field"),
    ("AutoNormal", "set", "set_auto_normal_field"),
    ("TileLight", "set", "set_tile_light_field"),
    ("Trajectory", "set", "set_trajectory_field"),
    ("TextPath", "set", "set_text_path_field"),
    ("BeatClock", "set", "set_beat_clock_field"),
    ("Terrain", "edit", "smooth_terrain_edit"),
    ("Dialogue", "start", "dialogue_start"),
    ("Dialogue", "advance", "dialogue_advance"),
    ("Dialogue", "choose", "dialogue_choose"),
    ("Dialogue", "stop", "dialogue_stop"),
    ("UI", "drop", "ui_drop"),
    ("Mode7", "place", "mode7_place"),
    ("Mode7", "set", "mode7_set"),
    ("Stage", "place", "stage_place"),
    ("Stage", "set", "stage_set"),
    ("SpriteTilt", "set", "sprite_tilt_set"),
    ("Afterimage", "set", "afterimage_set"),
    ("Portal", "set", "portal_set"),
    ("Jelly", "poke", "jelly_poke"),
    ("Jelly", "set", "jelly_set"),
    ("Marker", "set", "marker_set"),
    ("Radial", "set", "radial_set"),
    ("Bubble", "say", "bubble_say"),
    ("Bubble", "set", "bubble_set"),
    ("Outline", "set", "outline_set"),
    ("Lightning", "strike", "lightning_strike"),
    ("Lightning", "set", "lightning_set"),
    ("Flow", "set", "flow_set"),
    ("NavLink", "set", "nav_link_set"),
    ("Morph", "to", "morph_to"),
    ("Morph", "set", "morph_set"),
    ("Toast", "show", "toast_show"),
    ("Toast", "set", "toast_set"),
    ("Replay", "record", "replay_record"),
    ("Replay", "play", "replay_play"),
    ("Replay", "stop", "replay_stop"),
    ("Replay", "clear", "replay_clear"),
    ("Replay", "status", "replay_status"),
    ("Replay", "frames", "replay_frames"),
    ("Replay", "progress", "replay_progress"),
    ("Replay", "live_key", "replay_live_key"),
    ("Snow", "set", "snow_set"),
    ("FogOfWar", "reset", "fog_of_war_reset"),
    ("FogOfWar", "set", "fog_of_war_set"),
    ("State", "save", "state_save"),
    ("State", "load", "state_load"),
    ("State", "has", "state_has"),
    ("State", "forget", "state_forget"),
    ("Water", "set", "set_water_field"),
    ("Water", "splash", "water_splash"),
    ("CanvasGroup", "set", "set_canvas_group_field"),
    ("Video", "set", "set_video_field"),
    ("Ragdoll", "set", "set_ragdoll_field"),
    ("Timeline", "set", "set_timeline_field"),
    ("BehaviorTree", "set", "set_behavior_tree_field"),
    // Area effects
    ("Area", "set", "set_area_field"),
    ("Nav", "set_target", "set_nav_target"),
    ("Nav", "stop", "stop_nav"),
    ("Nav", "set", "set_nav_field"),
    ("Nav", "find_path", "nav_find_path"),
    ("Nav", "closest_point", "nav_closest_point"),
    ("Nav", "is_moving", "nav_is_moving"),
    ("Input", "rumble", "input_rumble"),
    ("Input", "vibrate", "input_vibrate"),
    ("Locale", "get", "locale_get"),
    ("Locale", "set", "locale_set"),
    ("Locale", "current", "locale_current"),
    ("Locale", "list", "locale_list"),
    ("Locale", "name", "locale_name"),
    ("Display", "set_mode", "display_set_mode"),
    ("Display", "get_mode", "display_get_mode"),
    ("Display", "set_size", "display_set_size"),
    ("Display", "get_size", "display_get_size"),
    ("Display", "get_resolutions", "display_get_resolutions"),
    ("Display", "set_vsync", "display_set_vsync"),
    ("Display", "get_vsync", "display_get_vsync"),
    ("Display", "set_max_fps", "display_set_max_fps"),
    ("Display", "get_max_fps", "display_get_max_fps"),
    ("Display", "supports", "display_supports"),
    ("Cursor", "set_visible", "cursor_set_visible"),
    ("Cursor", "set_confined", "cursor_set_confined"),
    ("Cursor", "set_locked", "cursor_set_locked"),
    ("Cursor", "set_icon", "cursor_set_icon"),
    ("Cursor", "set_image", "cursor_set_image"),
    // Noise
    ("Noise", "perlin", "noise_perlin"),
    ("Noise", "perlin3", "noise_perlin3"),
    ("Noise", "simplex", "noise_simplex"),
    ("Noise", "cellular", "noise_cellular"),
    ("Noise", "fractal", "noise_fractal"),
    ("Noise", "grid", "noise_grid"),
    ("Audio", "get_spectrum", "audio_get_spectrum"),
    ("Audio", "get_level", "audio_get_level"),
    ("Audio", "synth", "audio_synth"),
    ("Screenshot", "take", "screenshot_take"),
    ("Screenshot", "ready", "screenshot_ready"),
    ("Accessibility", "announce", "accessibility_announce"),
    ("Accessibility", "speech", "accessibility_speech"),
    // Lighting
    ("Lighting", "set_ambient", "set_ambient_light"),
    ("Lighting", "set_color", "set_light_color"),
    ("Lighting", "set_intensity", "set_light_intensity"),
    ("Lighting", "set_height", "set_light_height"),
    ("Lighting", "set_mask", "set_light_mask"),
    ("Lighting", "set_radius", "set_light_radius"),
    ("Lighting", "set_toon", "set_toon_lighting"),
    ("Lighting", "set_occlusion", "set_ambient_occlusion"),
    ("Lighting", "set_global_illumination", "set_global_illumination"),
    // PostProcess
    ("PostProcess", "set_enabled", "set_post_process_enabled"),
    ("PostProcess", "set_float", "set_post_process_float"),
    ("PostProcess", "set_vec4", "set_post_process_vec4"),
    ("PostProcess", "set_lut", "set_color_lut"),
    // UI
    ("UI", "create_text", "create_ui_text"),
    ("UI", "create_panel", "create_ui_panel"),
    ("UI", "set_text", "set_ui_text"),
    ("UI", "set_prop", "set_ui_prop"),
    ("UI", "get_prop", "get_ui_prop"),
    ("UI", "set_visible", "set_ui_visible"),
    ("UI", "set_position", "set_ui_position"),
    // GUI
    ("GUI", "show", "gui_show"),
    ("GUI", "hide", "gui_hide"),
    ("GUI", "get", "gui_get"),
    ("GUI", "set", "gui_set"),
    ("GUI", "on_click", "gui_on_click"),
    ("GUI", "focus", "gui_focus"),
    // Save
    ("Save", "get", "save_get"),
    ("Save", "set", "save_set"),
    ("Save", "commit", "save_commit"),
    ("Save", "clear", "save_clear"),
    // Persistent
    ("Persistent", "get_number", "persistent_get_number"),
    ("Persistent", "set_number", "persistent_set_number"),
    ("Persistent", "get_string", "persistent_get_string"),
    ("Persistent", "set_string", "persistent_set_string"),
    ("Persistent", "has", "persistent_has"),
    ("Persistent", "clear", "persistent_clear"),
    // Debug
    ("Debug", "draw_line", "draw_debug_line"),
    ("Debug", "draw_rect", "draw_debug_rect"),
    ("Debug", "draw_circle", "draw_debug_circle"),
    ("Debug", "set_overlay", "set_debug_overlay"),
    ("Debug", "log_stats", "log_stats"),
    ("Debug", "get_fps", "get_fps"),
    ("Debug", "get_frame_time", "get_frame_time"),
    ("Debug", "get_draw_calls", "get_draw_calls"),
    ("Debug", "get_entity_count", "get_entity_count"),
    // Data
    ("Data", "get", "get_data"),
    // Script
    ("Script", "call", "call"),
    ("Script", "load", "load_script"),
    // Math
    ("Math", "lerp", "lerp"),
    ("Math", "clamp", "clamp"),
    ("Math", "distance", "distance"),
    ("Math", "angle_between", "angle_between"),
    // Platform - the web portal a build was made for (`docs/web-platforms.md`)
    ("Platform", "name", "platform_name"),
    ("Platform", "game_ready", "platform_game_ready"),
    ("Platform", "gameplay_start", "platform_gameplay_start"),
    ("Platform", "gameplay_stop", "platform_gameplay_stop"),
    ("Platform", "happy_time", "platform_happy_time"),
    ("Platform", "submit_score", "platform_submit_score"),
    ("Platform", "show_ad", "platform_show_ad"),
];

/// Script globals that stay global.
pub const GLOBALS: &[&str] = &["log", "print", "require"];

/// Old flat names that duplicate another function, so they get no namespaced
/// name of their own: `(flat, the namespaced name to use instead)`.
pub const DUPLICATES: &[(&str, &str)] = &[
    ("get_entity_by_name", "Entity.find_by_name"),
    ("get_entities_by_tag", "Entity.find_by_tag"),
    ("register_event_listener", "Event.on"),
    ("set_post_process_param_float", "PostProcess.set_float"),
    ("set_post_process_param_vec4", "PostProcess.set_vec4"),
];

/// One namespaced name and the flat global it resolves to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamespacedName {
    pub namespace: String,
    pub short: String,
    pub flat: String,
}

impl NamespacedName {
    /// `Sprite.set_color`.
    pub fn qualified(&self) -> String {
        format!("{}.{}", self.namespace, self.short)
    }
}

/// The core table as entries.
pub fn core_entries() -> Vec<NamespacedName> {
    CORE.iter()
        .map(|(ns, short, flat)| NamespacedName {
            namespace: ns.to_string(),
            short: short.to_string(),
            flat: flat.to_string(),
        })
        .collect()
}

/// Every core namespace name.
pub fn core_namespaces() -> BTreeSet<&'static str> {
    CORE.iter().map(|(ns, _, _)| *ns).collect()
}

/// `jengine-example` -> `Example`, `my-cool_pkg` -> `MyCoolPkg`.
pub fn package_namespace(package: &str) -> String {
    let base = package.strip_prefix("jengine-").unwrap_or(package);
    base.split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Lua keywords, which cannot follow a `.`: `Ns.end(...)` does not parse.
fn is_lua_keyword(name: &str) -> bool {
    matches!(
        name,
        "and" | "break" | "do" | "else" | "elseif" | "end" | "false" | "for" | "function"
            | "goto" | "if" | "in" | "local" | "nil" | "not" | "or" | "repeat" | "return"
            | "then" | "true" | "until" | "while"
    )
}

fn usable_short(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && !is_lua_keyword(name)
}

/// The namespaced names of the packages' commands.
///
/// One namespace per package, from its name. If every command the package
/// registered starts with the same `prefix_`, the prefix is dropped; a
/// command whose remainder would not be a usable Lua field name (`x_end`)
/// keeps its whole name. A package whose namespace is taken - by core, or by
/// a package registered earlier - gets none, and says so in the log: its
/// flat globals still work.
pub fn package_entries(commands: &[PackageCommandInfo]) -> Vec<NamespacedName> {
    let mut by_package: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut order: Vec<&str> = Vec::new();
    for info in commands {
        let names = by_package.entry(info.package.as_str()).or_default();
        if names.is_empty() {
            order.push(info.package.as_str());
        }
        names.push(info.name.as_str());
    }

    let mut taken: BTreeSet<String> = core_namespaces().iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    for package in order {
        let namespace = package_namespace(package);
        if namespace.is_empty() || !taken.insert(namespace.clone()) {
            log::error!(
                "[package] {package}: the script namespace '{namespace}' is already taken; \
                 its commands stay reachable only by their flat names"
            );
            continue;
        }
        let names = &by_package[package];
        let prefix = common_prefix(names);
        for name in names {
            let short = prefix
                .as_deref()
                .and_then(|p| name.strip_prefix(p))
                .filter(|rest| usable_short(rest))
                .unwrap_or(name);
            out.push(NamespacedName {
                namespace: namespace.clone(),
                short: short.to_string(),
                flat: name.to_string(),
            });
        }
    }
    out
}

/// `realm_` when every name is `realm_<something>`, else `None`.
fn common_prefix(names: &[&str]) -> Option<String> {
    let first = names.first()?;
    let head = first.split_once('_')?.0;
    let prefix = format!("{head}_");
    names
        .iter()
        .all(|n| n.len() > prefix.len() && n.starts_with(&prefix))
        .then_some(prefix)
}

/// Core and package entries together - what a runtime installs.
pub fn all_entries(commands: &[PackageCommandInfo]) -> Vec<NamespacedName> {
    let mut out = core_entries();
    out.extend(package_entries(commands));
    out
}

/// `[["Sprite","set_color","set_sprite_color"], ...]`, the shape the browser
/// hosts publish and `lua-runtime.ts` reads.
pub fn to_json(entries: &[NamespacedName]) -> String {
    let rows: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| serde_json::json!([e.namespace, e.short, e.flat]))
        .collect();
    serde_json::Value::Array(rows).to_string()
}

/// Publish the namespaces onto `__jengine_play` for the Fengari runtime.
/// Both browser hosts call this - one site, so they cannot publish different
/// lists.
#[cfg(feature = "web")]
pub fn publish(play: &wasm_bindgen::JsValue, commands: &[PackageCommandInfo]) {
    use wasm_bindgen::JsValue;
    let json = to_json(&all_entries(commands));
    if let Ok(value) = js_sys::JSON::parse(&json) {
        let _ = js_sys::Reflect::set(play, &JsValue::from_str("__script_namespaces"), &value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(package: &str, name: &str) -> PackageCommandInfo {
        PackageCommandInfo {
            name: name.into(),
            tag: format!("pkg/{package}/{name}"),
            package: package.into(),
            args: Vec::new(),
        }
    }

    #[test]
    fn the_core_table_has_no_duplicates() {
        let mut flats = BTreeSet::new();
        let mut qualified = BTreeSet::new();
        for e in core_entries() {
            assert!(flats.insert(e.flat.clone()), "{} is in two namespaces", e.flat);
            assert!(qualified.insert(e.qualified()), "{} is listed twice", e.qualified());
            assert!(usable_short(&e.short), "{} is not a usable field name", e.qualified());
        }
        for (flat, _) in DUPLICATES {
            assert!(!flats.contains(*flat), "{flat} is both namespaced and a duplicate");
        }
    }

    #[test]
    fn a_package_is_named_after_its_crate() {
        assert_eq!(package_namespace("jengine-example"), "Example");
        assert_eq!(package_namespace("jengine-ui"), "Ui");
        assert_eq!(package_namespace("my-cool_pkg"), "MyCoolPkg");
    }

    #[test]
    fn a_shared_prefix_is_dropped_and_a_mixed_set_is_kept_whole() {
        let got = package_entries(&[
            info("jengine-example", "example_talk"),
            info("jengine-example", "example_end_talk"),
            info("jengine-toys", "kite_fly"),
            info("jengine-toys", "ball_bounce"),
        ]);
        let q: Vec<String> = got.iter().map(|e| format!("{} -> {}", e.qualified(), e.flat)).collect();
        assert_eq!(
            q,
            [
                "Example.talk -> example_talk",
                "Example.end_talk -> example_end_talk",
                "Toys.kite_fly -> kite_fly",
                "Toys.ball_bounce -> ball_bounce",
            ]
        );
    }

    #[test]
    fn a_remainder_that_is_a_keyword_keeps_the_whole_name() {
        let got = package_entries(&[info("jengine-x", "x_end"), info("jengine-x", "x_go")]);
        assert_eq!(got[0].qualified(), "X.x_end");
        assert_eq!(got[1].qualified(), "X.go");
    }

    #[test]
    fn a_package_cannot_take_a_core_namespace() {
        // `jengine-sprite` would be `Sprite`: its commands stay flat rather
        // than replacing or joining core's table.
        let got = package_entries(&[info("jengine-sprite", "sprite_wobble")]);
        assert!(got.is_empty(), "{got:?}");
    }
}
