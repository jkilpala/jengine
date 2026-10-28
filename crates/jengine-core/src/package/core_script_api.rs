//! Core script API registration — catalogs all built-in script-facing functions
//! in the `ScriptApiRegistry` for documentation, editor tooltips, and future
//! dynamic dispatch.
//!
//! The actual Lua/JS function bindings live in the backends. This module registers
//! descriptors (signatures) so the registry knows what functions exist.

use super::script_api::{ScriptApiRegistry, ScriptArg, ScriptValueType};

/// Helper macro for concise function registration.
macro_rules! reg {
    ($api:expr, $name:expr, $desc:expr, $ret:expr $(, $arg_name:expr => $arg_type:expr)* $(,)?) => {
        $api.add_function(
            $name,
            "jengine-core",
            $desc,
            vec![$(ScriptArg { name: $arg_name.to_string(), arg_type: $arg_type }),*],
            $ret,
        );
    };
}

use ScriptValueType::*;

/// Register all built-in script API function descriptors.
///
/// This populates the registry with metadata for all ~100 core functions.
/// Packages can add their own functions alongside these.
pub fn register_core_script_api(api: &mut ScriptApiRegistry) {
    // ── Transform ──
    reg!(api, "get_position", "Get entity position", Vec2, "entity_id" => EntityId);
    reg!(api, "set_position", "Set entity position", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float);
    reg!(api, "get_rotation", "Get entity rotation in radians", Float, "entity_id" => EntityId);
    reg!(api, "set_rotation", "Set entity rotation in radians", Nil, "entity_id" => EntityId, "radians" => Float);
    reg!(api, "get_scale", "Get entity scale", Vec2, "entity_id" => EntityId);
    reg!(api, "set_scale", "Set entity scale", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float);

    // ── Sprite ──
    reg!(api, "get_sprite_color", "Get sprite RGBA color", Rgba, "entity_id" => EntityId);
    reg!(api, "set_sprite_color", "Set sprite RGBA color", Nil, "entity_id" => EntityId, "r" => Float, "g" => Float, "b" => Float, "a" => Float);
    reg!(api, "get_sprite_anchor", "Get sprite anchor point", Vec2, "entity_id" => EntityId);
    reg!(api, "set_sprite_anchor", "Set sprite anchor point", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float);
    reg!(api, "set_sprite_size", "Set the size a sprite is drawn at, in world units", Nil, "entity_id" => EntityId, "w" => Float, "h" => Float);
    reg!(api, "paint_sprite", "Paint a stamp (splat, disc or streak) into a paintable sprite, in world units", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float, "size" => Float, "r" => Float, "g" => Float, "b" => Float, "a" => Float, "shape" => String, "rotation" => Float, "blend" => String);
    reg!(api, "press_sprite", "Press a dent (negative depth: a ridge) into a paintable sprite's normal map, in world units", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float, "size" => Float, "depth" => Float, "shape" => String, "rotation" => Float);
    reg!(api, "carve_sprite", "Cut a hole in a carvable sprite, in world units", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float, "radius" => Float);
    reg!(api, "set_z_order", "Set the draw order of an entity's sprite and text", Nil, "entity_id" => EntityId, "z" => Int);
    reg!(api, "set_trail_emitting", "Start or stop an entity's trail recording points", Nil, "entity_id" => EntityId, "on" => Bool);
    reg!(api, "set_toon_lighting", "Step the scene's light into bands (0 removes it), optionally dithered", Nil, "bands" => Int, "softness" => Float, "dither" => Float);
    reg!(api, "set_global_illumination", "Light that bounces: strength (0 removes it) and scale (frame pixels to a GI texel, 4 by default)", Nil, "strength" => Float, "scale" => Float);
    reg!(api, "set_ambient_occlusion", "Darken ambient light near occluder outlines (strength 0 removes it)", Nil, "radius" => Float, "strength" => Float);
    reg!(api, "set_mesh", "Replace an entity's mesh: vertices { x, y, u, v } and 1-based triangle indices", Nil, "entity_id" => EntityId, "vertices" => Any, "indices" => Any);
    reg!(api, "clear_trail", "Forget every point of an entity's trail", Nil, "entity_id" => EntityId);
    reg!(api, "set_text_color", "Set the colour of an entity's world text", Nil, "entity_id" => EntityId, "r" => Float, "g" => Float, "b" => Float, "a" => Float);
    reg!(api, "move_character", "Set the velocity a character controller asks for", Nil, "entity_id" => EntityId, "vx" => Float, "vy" => Float, "drop_through" => Bool);
    reg!(api, "get_character", "What a character controller did last physics step", Any, "entity_id" => EntityId);
    reg!(api, "is_mouse_right_pressed", "Whether the right mouse button is held", Bool);
    reg!(api, "is_mouse_right_just_pressed", "Whether the right mouse button was pressed this frame", Bool);
    reg!(api, "is_mouse_right_just_released", "Whether the right mouse button was released this frame", Bool);
    reg!(api, "is_mouse_middle_pressed", "Whether the middle mouse button is held", Bool);
    reg!(api, "is_mouse_middle_just_pressed", "Whether the middle mouse button was pressed this frame", Bool);
    reg!(api, "is_mouse_middle_just_released", "Whether the middle mouse button was released this frame", Bool);
    reg!(api, "set_sprite_label", "Show a sprite library part by label: on a part (optionally moving it to another category), or on the library's entity every part under it that has the label (only `category`'s, if given)", Nil, "entity_id" => EntityId, "label" => String, "category" => String);
    reg!(api, "set_sprite_region", "Swap sprite to a named region of a runtime-loaded atlas", Nil, "entity_id" => EntityId, "atlas_id" => String, "region_name" => String);
    reg!(api, "set_normal_map", "Give an entity's material a normal map (a texture id the scene loads), or nil to take it away", Nil, "entity_id" => EntityId, "normal_map_id" => String);
    reg!(api, "get_sprite_point", "Get named sprite point in world coords", Vec2, "entity_id" => EntityId, "point_name" => String);

    // ── Entity ──
    reg!(api, "get_entity_by_name", "Find entity by name", EntityId, "name" => String);
    reg!(api, "destroy_entity", "Destroy an entity", Nil, "entity_id" => EntityId);
    reg!(api, "spawn_entity", "Spawn a new empty entity", EntityId, "name" => String);
    reg!(api, "spawn_prefab", "Spawn entity from prefab", EntityId, "prefab_name" => String);

    // ── Input ──
    reg!(api, "is_key_pressed", "Check if key is held down", Bool, "key" => String);
    reg!(api, "is_key_just_pressed", "Check if key was pressed this frame", Bool, "key" => String);
    reg!(api, "is_action_pressed", "Check if action is active", Bool, "action" => String);
    reg!(api, "is_action_just_pressed", "Check if action started this frame", Bool, "action" => String);
    reg!(api, "is_action_just_released", "Check if action ended this frame", Bool, "action" => String);
    reg!(api, "get_mouse_wheel", "Get mouse wheel delta", Float);
    reg!(api, "get_mouse_delta", "How far the mouse moved this frame, in pixels (x right, y down) - raw motion, so it counts at the window edge and while locked", Vec2);
    reg!(api, "get_gestures", "This frame's gestures: { kind, x, y, wx, wy, dx, dy, wdx, wdy, scale, angle, speed, phase }", Any);
    reg!(api, "get_touches", "Every finger on a touch screen: { id, x, y, start_x, start_y, wx, wy, phase, on_control }", Any);

    // ── Custom Components ──
    reg!(api, "get_custom", "Get custom component field value", Float, "entity_id" => EntityId, "component" => String, "field" => String);
    reg!(api, "set_custom", "Set custom component field value", Nil, "entity_id" => EntityId, "component" => String, "field" => String, "value" => Float);
    reg!(api, "set_health", "Overwrite native Health (current + max)", Nil, "entity_id" => EntityId, "current" => Float, "max" => Float);
    reg!(api, "get_health", "Read native Health as (current, max)", Float, "entity_id" => EntityId);
    reg!(api, "set_hitbox", "Overwrite native Hitbox (damage, knockback, retrigger)", Nil, "entity_id" => EntityId, "damage" => Float, "knockback" => Float, "retrigger" => Float);

    // ── Tags ──
    reg!(api, "has_tag", "Check if entity has tag", Bool, "entity_id" => EntityId, "tag" => String);
    reg!(api, "add_tag", "Add tag to entity", Nil, "entity_id" => EntityId, "tag" => String);
    reg!(api, "remove_tag", "Remove tag from entity", Nil, "entity_id" => EntityId, "tag" => String);
    reg!(api, "get_entities_by_tag", "Get all entities with tag", VarArgs, "tag" => String);

    // ── Events ──
    reg!(api, "send_event", "Send a game event", Nil, "event_name" => String, "sender" => EntityId, "data" => String);
    reg!(api, "on_event", "Listen for a game event", Int, "event_name" => String, "entity_id" => EntityId);
    reg!(api, "remove_event_listener", "Remove event listener", Nil, "event_name" => String, "entity_id" => EntityId);

    // ── Scene ──
    reg!(api, "load_scene", "Load a scene by path", Nil, "path" => String);
    reg!(api, "scene_transition", "Load a scene, wiping the old frame away over it (fade, iris, diamond, slide, wipe, dissolve)", Nil, "path" => String, "style" => String, "seconds" => Float, "texture" => String);
    reg!(api, "push_scene", "Push scene onto stack", Nil, "path" => String);
    reg!(api, "pop_scene", "Pop scene from stack", Nil);
    reg!(api, "quit_game", "Request application shutdown", Nil);
    reg!(api, "set_time_scale", "Scale world delta time (0 = pause)", Nil, "scale" => Float);
    reg!(api, "get_time_scale", "How fast the world's time runs (as of the last frame)", Float);
    reg!(api, "get_current_scene_name", "Get current scene name", String);
    reg!(api, "fade_in", "Fullscreen fade overlay: opaque -> clear over duration_s (color defaults to black)",
         Nil, "duration_s" => Float, "r" => Float, "g" => Float, "b" => Float);
    reg!(api, "fade_out", "Fullscreen fade overlay: clear -> opaque over duration_s (color defaults to black)",
         Nil, "duration_s" => Float, "r" => Float, "g" => Float, "b" => Float);

    // ── Debug Drawing ──
    reg!(api, "draw_debug_line", "Draw debug line", Nil, "x1" => Float, "y1" => Float, "x2" => Float, "y2" => Float, "r" => Float, "g" => Float, "b" => Float);
    reg!(api, "draw_debug_rect", "Draw debug rectangle", Nil, "x" => Float, "y" => Float, "w" => Float, "h" => Float, "r" => Float, "g" => Float, "b" => Float);
    reg!(api, "draw_debug_circle", "Draw debug circle", Nil, "x" => Float, "y" => Float, "radius" => Float, "r" => Float, "g" => Float, "b" => Float);

    // ── Camera Effects ──
    // P1.3 — frequency=0 makes the engine pick a sensible default (~20 Hz).
    reg!(api, "set_camera_pixel_size",
         "Draw the world at one pixel per `size` world units and scale it up (0: the screen's resolution)",
         Nil, "size" => Float);
    reg!(api, "set_camera_zoom",
         "Set the active camera's zoom (1.0 = the authored framing, higher is closer in)",
         Nil, "zoom" => Float);
    reg!(api, "get_camera_position", "Where the active camera is looking (its rendered centre, following included): x, y", Vec2);
    reg!(api, "set_camera_screen_rect", "Draw this camera onto part of the screen: x, y, w, h as fractions from the top-left (all 0 for the whole screen again)", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float, "w" => Float, "h" => Float);
    reg!(api, "set_camera_field", "Set any camera field by name (priority to hand it the view, blend, confine, zoom, is_active, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_camera_follow", "Set any camera_follow field by name (smoothing, dead_zone, look_ahead, offset, bounds, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "get_camera_zoom",
         "The active camera's zoom. Worth asking now that `CameraFollow.frame_tag` lets the engine choose it",
         Float);
    reg!(api, "camera_shake", "Apply screen-shake to the active camera (frequency=0 → default 20 Hz)",
         Nil, "amplitude" => Float, "duration" => Float, "frequency" => Float);

    // P1.3 — fullscreen flash overlay (RGBA; alpha is the initial opacity).
    reg!(api, "screen_flash",
         "Fullscreen flash overlay; alpha is initial opacity, fades linearly to 0 over duration",
         Nil, "duration" => Float, "r" => Float, "g" => Float, "b" => Float, "a" => Float);

    // ── Math ──
    reg!(api, "lerp", "Linear interpolation", Float, "a" => Float, "b" => Float, "t" => Float);
    reg!(api, "clamp", "Clamp value to range", Float, "value" => Float, "min" => Float, "max" => Float);
    reg!(api, "distance", "Distance between two points", Float, "x1" => Float, "y1" => Float, "x2" => Float, "y2" => Float);
    reg!(api, "angle_between", "Angle between two points", Float, "x1" => Float, "y1" => Float, "x2" => Float, "y2" => Float);

    // ── Timers ──
    reg!(api, "set_timer", "Set a one-shot timer", Int, "entity_id" => EntityId, "delay" => Float, "callback" => String);
    reg!(api, "set_interval", "Set a repeating timer", Int, "entity_id" => EntityId, "interval" => Float, "callback" => String);
    reg!(api, "cancel_timer", "Cancel a timer", Nil, "timer_id" => Int);

    // ── Animation ──
    reg!(api, "play_animation", "Play an animation clip - sprite or skeletal, whichever the entity has", Nil, "entity_id" => EntityId, "clip_name" => String);
    reg!(api, "crossfade_animation", "Start a skeletal clip, fading the current pose out over `seconds`", Nil, "entity_id" => EntityId, "clip" => String, "seconds" => Float);
    reg!(api, "set_animation_blend", "Mix a second skeletal clip in at the same phase (weight 0..1)", Nil, "entity_id" => EntityId, "clip" => String, "weight" => Float);
    reg!(api, "animation_play_layer", "Play a clip on a skeleton's layer (its mask's bones), fading in over `seconds` (0.2); `loop` false fades it out when it ends", Nil, "entity_id" => EntityId, "layer" => String, "clip" => String, "seconds" => Float, "loop" => Bool);
    reg!(api, "animation_stop_layer", "Fade a skeleton's layer out over `seconds` (0.2)", Nil, "entity_id" => EntityId, "layer" => String, "seconds" => Float);
    reg!(api, "animation_set_layer_weight", "Fade a skeleton's layer to `weight` (0..1) over `seconds`", Nil, "entity_id" => EntityId, "layer" => String, "weight" => Float, "seconds" => Float);
    reg!(api, "set_animation_blend_position", "Move a skeletal player's 2D blend space (its clips mixed by where the position is)", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float);
    reg!(api, "set_conveyor_speed", "How fast a conveyor slides what rests on it, along its +x (world units a second)", Nil, "entity_id" => EntityId, "speed" => Float);
    reg!(api, "set_ik_weight", "How much of a two-bone IK solve applies (0..1)", Nil, "entity_id" => EntityId, "weight" => Float);
    reg!(api, "stop_animation", "Stop sprite animation", Nil, "entity_id" => EntityId);
    reg!(api, "set_animation_speed", "Set animation playback speed", Nil, "entity_id" => EntityId, "speed" => Float);
    reg!(api, "is_animation_playing", "Check if animation is playing", Bool, "entity_id" => EntityId);

    // ── Tweens ──
    reg!(api, "tween_position", "Tween entity position", Int, "entity_id" => EntityId, "x" => Float, "y" => Float, "duration" => Float, "easing" => String);
    reg!(api, "tween_rotation", "Tween entity rotation", Int, "entity_id" => EntityId, "radians" => Float, "duration" => Float, "easing" => String);
    reg!(api, "tween_scale", "Tween entity scale", Int, "entity_id" => EntityId, "sx" => Float, "sy" => Float, "duration" => Float, "easing" => String);
    reg!(api, "tween_color", "Tween sprite color", Int, "entity_id" => EntityId, "r" => Float, "g" => Float, "b" => Float, "a" => Float, "duration" => Float, "easing" => String);
    reg!(api, "cancel_tween", "Cancel a tween", Nil, "tween_id" => Int);
    reg!(api, "cancel_all_tweens", "Cancel all tweens for entity", Nil, "entity_id" => EntityId);

    // ── Tilemap ──
    reg!(api, "get_tile", "Get tile ID at grid position", Int, "entity_id" => EntityId, "layer" => String, "x" => Int, "y" => Int);
    reg!(api, "get_tile_data", "The custom data of the tile at a grid position: one field, or the whole table with no field; nil when none", Any, "entity_id" => EntityId, "layer" => String, "x" => Int, "y" => Int, "field" => String);
    reg!(api, "set_tile", "Set tile ID at grid position", Nil, "entity_id" => EntityId, "layer" => String, "x" => Int, "y" => Int, "tile_id" => Int);
    reg!(api, "get_tile_at_world", "Get tile at world position", Int, "entity_id" => EntityId, "layer" => String, "wx" => Float, "wy" => Float);
    reg!(api, "world_to_tile", "Convert world coords to tile coords", Vec2, "entity_id" => EntityId, "wx" => Float, "wy" => Float);
    reg!(api, "tile_to_world", "Convert tile coords to world coords", Vec2, "entity_id" => EntityId, "tx" => Int, "ty" => Int);
    reg!(api, "tilemap_neighbors", "The cells next to a cell that are on the map - six on a hexagonal map; a list of {x, y} pairs", VarArgs, "entity_id" => EntityId, "tx" => Int, "ty" => Int);
    reg!(api, "tilemap_distance", "Steps between two cells, ignoring what is on them (hex steps on a hexagonal map)", Int, "entity_id" => EntityId, "ax" => Int, "ay" => Int, "bx" => Int, "by" => Int);
    // 2.6 — A* path-finding. `mode` is "manhattan" (default) or "diagonal".
    reg!(api, "tilemap_find_path", "A* on a tilemap layer; returns tile-coord pairs or nil", VarArgs, "entity_id" => EntityId, "layer" => String, "start_x" => Int, "start_y" => Int, "goal_x" => Int, "goal_y" => Int, "mode" => String);
    reg!(api, "tilemap_find_path_world", "A* with world-space inputs; returns tile-centre world pairs or nil", VarArgs, "entity_id" => EntityId, "layer" => String, "start_x" => Float, "start_y" => Float, "goal_x" => Float, "goal_y" => Float, "mode" => String);

    // ── Particles ──
    reg!(api, "emit_burst", "Emit particle burst", Nil, "entity_id" => EntityId, "count" => Int);
    reg!(api, "set_emitter_active", "Enable/disable emitter", Nil, "entity_id" => EntityId, "active" => Bool);
    reg!(api, "set_emitter_rate", "Set particle emission rate", Nil, "entity_id" => EntityId, "rate" => Float);
    reg!(api, "set_emitter_field", "Set any particle emitter field by name (angle, speed, turbulence.strength, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "get_particle_count", "Get total active particles", Int);
    reg!(api, "set_behavior_tree_field", "Set a behavior tree's root (a new tree), enabled, loop, or restart", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_timeline_field", "Set any timeline field by name: clip (starts it), playing, time (seeks), speed, looping", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_ragdoll_field", "Set any ragdoll field by name (active, thickness, limit, density, friction, damping)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_video_field", "Set any video_player field by name (source, playing, looping, speed, seek)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_canvas_group_field", "Set any canvas_group field by name (opacity, tint, z_order)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_trail_field", "Set any trail field by name (color, color_end, width, lifetime, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_line_field", "Set any line field by name (points, width, gradient, joint, texture_mode, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "smooth_terrain_edit", "Dig (amount > 0) or build (< 0) a smooth terrain round a world point", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float, "radius" => Float, "amount" => Float);
    reg!(api, "dialogue_start", "Play a conversation from `node` (the dialogue's `start` when empty)", Nil, "entity_id" => EntityId, "node" => String);
    reg!(api, "dialogue_advance", "Finish the line still typing, else go on to the next", Nil, "entity_id" => EntityId);
    reg!(api, "dialogue_choose", "Take choice `n` (from 1) of the line on show", Nil, "entity_id" => EntityId, "n" => Float);
    reg!(api, "dialogue_stop", "End the conversation", Nil, "entity_id" => EntityId);
    reg!(api, "ui_drop", "Put a draggable item in the slot named `slot`, by the rules a drop follows (stack, swap or refuse)", Nil, "entity_id" => EntityId, "slot" => String);
    reg!(api, "mode7_place", "Stand a Mode 7 sprite (or a plane's camera) at a ground point, facing `angle`", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float, "angle" => Float);
    reg!(api, "mode7_set", "Set any field of a mode7_sprite, else of a mode7 plane, by name", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "stage_place", "Stand a stage billboard (or move a stage quad's corner, or point a stage's camera) at a stage point", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float, "z" => Float);
    reg!(api, "stage_set", "Set any field of a stage_billboard, else a stage_quad, else a stage, by name", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "fog_of_war_reset", "The fog_of_war map unseen again: every cell's visible and explored share cleared", Nil, "entity_id" => EntityId);
    reg!(api, "fog_of_war_set", "Set any fog_of_war field by name (size, cell, unseen_color, explored_alpha, reveal_speed, max_rate)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "snow_set", "Set any snow_cover field by name (depth, max_depth, rate, color, slope_limit, bumps, cutoff, step)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "replay_record", "Arm an input recording: it starts with the next scene load", Nil);
    reg!(api, "replay_play", "Arm playback of the recording from the next scene load; false when nothing was recorded", Bool);
    reg!(api, "replay_stop", "Stop recording or playing (what was recorded is kept)", Nil);
    reg!(api, "replay_clear", "Forget what was recorded", Nil);
    reg!(api, "replay_status", "idle, armed, recording, playing or finished", String);
    reg!(api, "replay_frames", "How many frames were recorded", Int);
    reg!(api, "replay_progress", "How far playback has got, 0 to 1", Float);
    reg!(api, "replay_live_key", "Whether the player pressed this key this frame, even while a replay feeds everything else", Bool, "key" => String);
    reg!(api, "toast_show", "Queue a toast on the entity's toast_stack: a title, text, an icon texture and seconds (0: the stack's own)", Nil, "entity_id" => EntityId, "title" => String, "text" => String, "icon" => String, "seconds" => Float);
    reg!(api, "toast_set", "Set any toast_stack field by name (corner, margin, size, gap, max_shown, seconds, slide, texture, color, title_color, text_color, icon_size)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "morph_to", "Ease the entity's vector_path into the shape of the named entity's over seconds (leave out for the morph's own duration)", Nil, "entity_id" => EntityId, "target" => String, "seconds" => Float);
    reg!(api, "morph_set", "Set any path_morph field by name (target, duration, points, ease, progress)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "nav_link_set", "Set any nav_link field by name (from, to, one_way, cost, duration, kind, enabled)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "flow_set", "Set any flow_path field by name (path, width, speed, bank_speed, grip, tag, texture, tile, color, soft)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "lightning_strike", "Strike lightning from the entity now, landing at (x, y)", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float);
    reg!(api, "lightning_set", "Set any lightning field by name (end, spread, interval, jag, branches, width, glow, color, glow_color, duration, restrikes, light_intensity, light_radius, flash)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "outline_set", "Set any sprite_outline field by name (enabled, color, width, glow, pulse, pulse_amount)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "bubble_say", "Say text in a speech bubble over the entity for seconds (0 keeps it; leave out for the bubble's own)", Nil, "entity_id" => EntityId, "text" => String, "seconds" => Float);
    reg!(api, "bubble_set", "Set any speech_bubble field by name (max_width, font_size, padding, offset, texture, tail_texture, color, text_color, reveal_speed, duration)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "radial_set", "Set any ui_radial_menu field by name (sections, radius, dead_zone, start, hover_scale, hover_tint, idle_tint, open)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "marker_set", "Set any offscreen_marker field by name (target, margin, arrow_texture, icon_texture, icon_offset, label, label_format, units_per_meter, enabled)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "jelly_poke", "Push a jelly's points near a world point in (strength in units a second, within radius)", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float, "strength" => Float, "radius" => Float);
    reg!(api, "jelly_set", "Set any jelly field by name (cols, rows, stiffness, damping, coupling, response, max_stretch)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "portal_set", "Set any portal field by name (pair, width, depth, resolution, tint, z_order, carry, enabled)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "physics_slice", "Cut every sliceable body the segment from (x0, y0) to (x1, y1) crosses into two halves", Nil, "x0" => Float, "y0" => Float, "x1" => Float, "y1" => Float);
    reg!(api, "afterimage_set", "Set any afterimage field by name (emitting, interval, lifetime, color, end_color, min_speed, max_echoes, flat, blend_mode)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "sprite_tilt_set", "Set any sprite_tilt field by name (yaw, pitch, target, speed, distance, back_texture, back_color, shine)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_beat_clock_field", "Set any beat_clock field by name (bpm, offset, beats_per_bar, follow_music, running)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_text_path_field", "Set any text_path field by name (path, offset, side, scroll, spacing, repeat)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "state_save", "Save the whole world (entities, components, bodies) under a slot name", Nil, "slot" => String);
    reg!(api, "state_load", "Put the world back as a slot holds it", Nil, "slot" => String);
    reg!(api, "state_has", "Whether a slot holds a saved world", Bool, "slot" => String);
    reg!(api, "state_forget", "Drop a saved world", Nil, "slot" => String);
    reg!(api, "set_trajectory_field", "Set any trajectory field by name (velocity, active, seconds, every, line, hit_line)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_tile_light_field", "Set any tile_light or tile_light_source field by name (decay, solid_decay, intensity, color, on, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_auto_normal_field", "Set any auto_normal field by name (bevel, strength, shape, cutoff)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_cast_shadow_field", "Set any cast_shadow field by name (opacity, lights, light, view, lean, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_time_scale_field", "Set an entity's own pace: scale, or ignore_world to run on real time", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_origin_field", "Set a floating_origin field (threshold, enabled)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_day_field", "Set any day_cycle field by name (hour jumps the clock, hours_per_second speeds it)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_vision_field", "Set any vision field by name (radius, fov, direction, color, targets, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "wfc_generate", "Queue a region (in cells) of a wfc generator's layer to be filled with tiles that fit", Nil, "entity_id" => EntityId, "x" => Int, "y" => Int, "w" => Int, "h" => Int);
    reg!(api, "set_fog_field", "Set any fog field by name (density, color, scatter, wind, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_rewind_field", "Set a rewind clock's mode (record, rewind, hold) or speed", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "smoke_puff", "Puff smoke into a smoke grid at a world point, with a push (vx, vy world units a second)", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float, "radius" => Float, "amount" => Float, "vx" => Float, "vy" => Float);
    reg!(api, "set_smoke_field", "Set any smoke field by name, or a source's as \"sources.<n>.<field>\" (n from 1)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "pixel_paint", "Paint a disc of a material (sand, water, oil, fire, smoke, lava, steam, stone, wood, empty) into a pixel simulation at a world point", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float, "radius" => Float, "material" => String);
    reg!(api, "set_sprite_fill", "Set how much of a bar or radial fill is drawn (0..1)", Nil, "entity_id" => EntityId, "amount" => Float);
    reg!(api, "set_water_field", "Set any water_surface field by name (chop, swell_height, buoyancy, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "water_splash", "Push a water surface at world x: strength is the speed given to it (down is negative)", Nil, "entity_id" => EntityId, "x" => Float, "strength" => Float);
    reg!(api, "set_mirror_field", "Set any mirror field by name (enabled, tint, depth, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_fluid_field", "Set any fluid field by name (pour_rate, viscosity, cooling, ...); clear empties it", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_path_follow_field", "Set any path_follow field by name (path, speed, progress, mode, playing, lateral, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_cloth_field", "Set any cloth or rope field by name (wind, gust, flutter, length, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "set_nav_target", "Send a nav_agent walking to (x, y) through its nav_region", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float);
    reg!(api, "stop_nav", "Stop a nav_agent where it stands", Nil, "entity_id" => EntityId);
    reg!(api, "set_nav_field", "Set any nav_agent (or nav_region) field by name (speed, radius, avoidance, agent_radius, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);
    reg!(api, "nav_find_path", "Shortest walkable path between two world points through a nav_region: { {x, y}, ... } or nil", VarArgs, "x0" => Float, "y0" => Float, "x1" => Float, "y1" => Float);
    reg!(api, "input_rumble", "Rumble gamepad `pad`: strong and weak motors 0..1 for `seconds` (0 stops it)", Nil, "pad" => Int, "strong" => Float, "weak" => Float, "seconds" => Float);
    reg!(api, "input_vibrate", "Vibrate the device (a phone) for `seconds` at `strength` 0..1", Nil, "seconds" => Float, "strength" => Float);
    // ── Platform: the web portal a build was made for (docs/web-platforms.md) ──
    reg!(api, "platform_name", "The web portal this build was made for: jcabinet, poki, crazygames, youtube, or none", String);
    reg!(api, "platform_game_ready", "Tell the portal the game is interactive (a menu, not a splash); only when web_game_ready is \"script\"", Nil);
    reg!(api, "platform_gameplay_start", "Tell the portal the player is playing (level start, resume)", Nil);
    reg!(api, "platform_gameplay_stop", "Tell the portal play has stopped (menu, pause, level end, game over)", Nil);
    reg!(api, "platform_happy_time", "Tell the portal something great just happened (a boss beaten, a high score)", Nil);
    reg!(api, "platform_submit_score", "Send a score to the portal (YouTube's best score, a jCabinet leaderboard)", Nil, "value" => Int, "board" => String);
    reg!(api, "platform_show_ad", "Ask the portal for an ad, \"midgame\" or \"rewarded\" (reward: YouTube's id for this kind of reward); the platform_ad_finished event answers { kind, shown, rewarded }", Nil, "kind" => String, "reward" => String);
    reg!(api, "locale_get", "A string-table key in the current language, plural form and {placeholders} filled from args; the key itself when missing", String, "key" => String, "args" => Any);
    reg!(api, "locale_set", "Choose the language by code (empty: follow the system)", Nil, "code" => String);
    reg!(api, "locale_current", "The language in use, as a code", String);
    reg!(api, "locale_list", "The language codes the string tables carry", VarArgs);
    reg!(api, "locale_name", "A language's own name (its _name row), or its code", String, "code" => String);
    reg!(api, "display_set_mode", "Window mode: windowed, borderless or fullscreen (a resolution of its own; a browser takes borderless)", Nil, "mode" => String);
    reg!(api, "display_get_mode", "The window mode the runtime has: windowed, borderless or fullscreen", String);
    reg!(api, "display_set_size", "The window's size in pixels, or the fullscreen resolution (0, 0: leave it)", Nil, "w" => Int, "h" => Int);
    reg!(api, "display_get_size", "The drawable size in pixels: w, h", VarArgs);
    reg!(api, "display_get_resolutions", "The resolutions the screen offers, largest first: { {w, h, hz}, ... }", VarArgs);
    reg!(api, "display_set_vsync", "Wait for the display's refresh (true) or not", Nil, "on" => Bool);
    reg!(api, "display_get_vsync", "Whether frames wait for the display's refresh", Bool);
    reg!(api, "display_set_max_fps", "The most frames a second (0: no cap; nil or -1: the project's settings.frame_rate_cap)", Nil, "fps" => Int);
    reg!(api, "display_get_max_fps", "The frame cap in force (0: none)", Int);
    reg!(api, "display_supports", "Whether this runtime can: fullscreen, exclusive, window_size, vsync, max_fps, cursor_image, cursor_confine", Bool, "feature" => String);
    reg!(api, "cursor_set_visible", "Show or hide the mouse cursor over the game", Nil, "on" => Bool);
    reg!(api, "cursor_set_confined", "Keep the cursor inside the window (not in a browser)", Nil, "on" => Bool);
    reg!(api, "cursor_set_locked", "Lock and hide the pointer; read its motion with get_mouse_delta (a browser needs a recent click)", Nil, "on" => Bool);
    reg!(api, "cursor_set_icon", "A system cursor: default, pointer, crosshair, text, move, wait, progress, help, not_allowed, grab, grabbing", Nil, "name" => String);
    reg!(api, "noise_perlin", "Perlin noise at a point, about -1..1, zero on whole numbers", Float, "x" => Float, "y" => Float, "seed" => Int);
    reg!(api, "noise_perlin3", "Perlin noise in 3D, about -1..1 - z is usually time", Float, "x" => Float, "y" => Float, "z" => Float, "seed" => Int);
    reg!(api, "noise_simplex", "Simplex noise at a point, about -1..1", Float, "x" => Float, "y" => Float, "seed" => Int);
    reg!(api, "noise_cellular", "Cellular (Worley) noise: f1, f2 (distances to the nearest two points, in cells) and the nearest cell's value 0..1", VarArgs, "x" => Float, "y" => Float, "seed" => Int);
    reg!(api, "noise_fractal", "Octaves of noise summed: { kind, mode, octaves, lacunarity, gain, frequency, seed, z }", Float, "x" => Float, "y" => Float, "opts" => Any);
    reg!(api, "audio_get_spectrum", "The mix's spectrum: n bands (16 by default) from 40 Hz to 16 kHz, low to high, each 0..1 (-70..0 dB)", VarArgs, "bands" => Int);
    reg!(api, "audio_get_level", "The mix's loudness now, 0..1 (RMS)", Float);
    reg!(api, "audio_synth", "Play a synthesised note: { wave, freq or note, slide, duration, attack, decay, sustain, release, volume, bus }", Nil, "opts" => Any);
    reg!(api, "screenshot_take", "Copy the next frame into a texture and/or a file: { texture, file, scale }; returns its id", Int, "opts" => Any);
    reg!(api, "screenshot_ready", "Whether screenshot id has been written", Bool, "id" => Int);
    reg!(api, "accessibility_announce", "Have a screen reader say text (a live region)", Nil, "text" => String);
    reg!(api, "accessibility_speech", "The focused UI node as a screen reader says it (empty if none)", String);
    reg!(api, "noise_grid", "w x h fractal samples, row by row, step apart from x0, y0 - a whole map in one call", VarArgs, "x0" => Float, "y0" => Float, "w" => Int, "h" => Int, "step" => Float, "opts" => Any);
    reg!(api, "cursor_set_image", "A texture as the cursor, its hot spot in pixels from its top left ('' for the system cursor)", Nil, "texture_id" => String, "hot_x" => Int, "hot_y" => Int);
    reg!(api, "scene_load_additive", "Add a scene to the running one, offset by (x, y), once its assets are in: a chunk handle", Int, "path" => String, "x" => Float, "y" => Float);
    reg!(api, "scene_unload", "Take an additive chunk out again", Nil, "handle" => Int);
    reg!(api, "scene_preload", "Load a scene's assets in the background so a later Scene.load is quick", Nil, "path" => String);
    reg!(api, "scene_status", "A chunk's (by handle) or a preload's (by path) status: loading, loaded, failed or unloaded", String, "of" => Any);
    reg!(api, "scene_progress", "How far a chunk (by handle) or a preload (by path) has loaded, 0..1", Float, "of" => Any);
    reg!(api, "pack_download", "Fetch an asset pack the build left out; a pack already here or on its way is left alone", Nil, "name" => String);
    reg!(api, "pack_remove", "Delete a downloaded asset pack's local copy (it is remote again)", Nil, "name" => String);
    reg!(api, "pack_set_url", "Where asset packs are downloaded from from now on (https://... or a folder)", Nil, "url" => String);
    reg!(api, "pack_status", "An asset pack's status: remote, downloading, installed, failed or unknown", String, "name" => String);
    reg!(api, "pack_progress", "How far an asset pack has downloaded, 0..1 (1 once installed)", Float, "name" => String);
    reg!(api, "pack_size", "An asset pack's download size in bytes (0 when local or unknown)", Float, "name" => String);
    reg!(api, "pack_error", "Why an asset pack's last download failed, or ''", String, "name" => String);
    reg!(api, "pack_of", "The asset pack an asset path is in, or '' when it ships with the game", String, "path" => String);
    reg!(api, "pack_list", "Every asset pack this game has, by name", Any);
    reg!(api, "liveops_get", "A remote config value: the live ops service's for this player, or the game's default (settings.liveops.defaults), or `default`", Any, "key" => String, "default" => Any);
    reg!(api, "liveops_keys", "Every remote config key there is a value for", Any);
    reg!(api, "liveops_status", "Where the values come from: off, defaults, cached or live", String);
    reg!(api, "liveops_error", "Why the last fetch or snapshot was refused, or ''", String);
    reg!(api, "liveops_fetch", "Ask the live ops service again now", Nil);
    reg!(api, "liveops_fetching", "Whether a fetch is on its way", Bool);
    reg!(api, "liveops_variant", "The variant of an A/B experiment this install is in, or ''", String, "experiment" => String);
    reg!(api, "liveops_events", "The ids of the live events on now", Any);
    reg!(api, "liveops_event_active", "Whether a live event is on now", Bool, "id" => String);
    reg!(api, "liveops_event_ends", "Seconds until a live event that is on ends (0 when it is not on)", Float, "id" => String);
    reg!(api, "liveops_event_starts", "Seconds until a live event starts (0 once it has, or if unknown)", Float, "id" => String);
    reg!(api, "liveops_event_name", "A live event's name, as the service calls it", String, "id" => String);
    reg!(api, "liveops_set_player", "Say who is signed in, so rules naming players reach them; asks again", Nil, "id" => String);
    reg!(api, "liveops_server_time", "The live ops service's clock, in Unix seconds - for event timers a changed device clock cannot cheat", Float);
    reg!(api, "liveops_revision", "The revision of the snapshot believed (0 with none)", Float);
    reg!(api, "liveops_release", "The content release the service names (0 with none)", Float);
    reg!(api, "liveops_updated", "A number that goes up whenever the values may have changed", Float);
    reg!(api, "liveops_expose", "Tell the live ops service this install saw its variant of an experiment (no event), or did `event` in it (`value` summed); false when live ops are off", Bool, "experiment" => String, "event" => String, "value" => Float);
    reg!(api, "liveops_set_attribute", "An attribute of this game the service's rules can target (\"\" takes it away); asks again", Nil, "name" => String, "value" => Any);
    reg!(api, "nav_is_moving", "Whether a nav_agent still has somewhere to walk to (false once it arrives or is stopped)", Bool, "entity_id" => EntityId);
    reg!(api, "nav_closest_point", "The walkable point nearest (x, y): x, y or nil", VarArgs, "x" => Float, "y" => Float);
    reg!(api, "set_area_field", "Set any area effect field by name (buoyancy, force, attract, drag, enabled, ...)", Nil, "entity_id" => EntityId, "field" => String, "value" => Any);

    // ── UI ──
    reg!(api, "create_ui_text", "Create UI text element", EntityId, "text" => String, "anchor" => String, "ox" => Float, "oy" => Float);
    reg!(api, "create_ui_panel", "Create UI panel", EntityId, "anchor" => String, "ox" => Float, "oy" => Float, "w" => Float, "h" => Float);
    reg!(api, "set_ui_text", "Update UI text content", Nil, "entity_id" => EntityId, "text" => String);
    reg!(api, "set_ui_prop", "Set a widget property (value, checked, text, disabled, visible, color, bg_color, texture_id, region)", Nil, "entity_id" => EntityId, "property" => String, "value" => Any);
    reg!(api, "get_ui_prop", "Read a widget property (value, checked, text, pressed, hovered, focused, disabled, visible, texture_id, region, color, bg_color)", Any, "entity_id" => EntityId, "property" => String);
    reg!(api, "set_ui_visible", "Show/hide UI element", Nil, "entity_id" => EntityId, "visible" => Bool);
    reg!(api, "set_ui_position", "Set UI element position", Nil, "entity_id" => EntityId, "anchor" => String, "ox" => Float, "oy" => Float);

    // ── GUI Document ──
    reg!(api, "gui_show", "Show GUI document", Nil, "gui_id" => String);
    reg!(api, "gui_hide", "Hide GUI document", Nil, "gui_id" => String);
    reg!(api, "gui_set", "Set GUI node property", Nil, "gui_id" => String, "node_id" => String, "property" => String, "value" => String);
    reg!(api, "gui_get", "Get GUI node property", String, "gui_id" => String, "node_id" => String, "property" => String);
    reg!(api, "gui_on_click", "Register click handler", Nil, "entity_id" => EntityId, "gui_id" => String, "node_id" => String, "callback" => String);
    reg!(api, "gui_focus", "Focus GUI node", Nil, "gui_id" => String, "node_id" => String);

    // ── Physics ──
    reg!(api, "apply_force", "Apply force to body", Nil, "entity_id" => EntityId, "fx" => Float, "fy" => Float);
    reg!(api, "apply_impulse", "Apply impulse to body", Nil, "entity_id" => EntityId, "ix" => Float, "iy" => Float);
    reg!(api, "set_velocity", "Set body linear velocity", Nil, "entity_id" => EntityId, "vx" => Float, "vy" => Float);
    reg!(api, "get_velocity", "Get body linear velocity", Vec2, "entity_id" => EntityId);
    reg!(api, "set_angular_velocity", "Set angular velocity", Nil, "entity_id" => EntityId, "omega" => Float);
    reg!(api, "get_angular_velocity", "Get angular velocity", Float, "entity_id" => EntityId);
    reg!(api, "set_gravity", "Set world gravity", Nil, "gx" => Float, "gy" => Float);
    reg!(api, "get_gravity", "Get world gravity", Vec2);
    reg!(api, "set_body_type", "Set rigid body type", Nil, "entity_id" => EntityId, "body_type" => String);
    reg!(api, "get_contacts", "Get contact events from last step", VarArgs);
    reg!(api, "raycast", "Cast a ray and get hit info", VarArgs, "ox" => Float, "oy" => Float, "dx" => Float, "dy" => Float, "max_dist" => Float);
    reg!(api, "overlap_circle", "Every entity a collider of which overlaps the circle, each once, by id", VarArgs, "x" => Float, "y" => Float, "radius" => Float);
    reg!(api, "overlap_box", "Every entity a collider of which overlaps the (turned) box, each once, by id", VarArgs, "x" => Float, "y" => Float, "width" => Float, "height" => Float, "rotation" => Float);
    reg!(api, "overlap_point", "Every entity a collider of which contains the point, by id", VarArgs, "x" => Float, "y" => Float);
    reg!(api, "circle_cast", "Sweep a circle along a line; hit info like raycast, or nil", VarArgs, "ox" => Float, "oy" => Float, "dx" => Float, "dy" => Float, "radius" => Float, "max_dist" => Float);
    reg!(api, "add_joint", "Add joint between entities", Nil, "entity_name" => String, "target_name" => String, "joint_json" => String);
    reg!(api, "remove_joint", "Remove joint from entity", Nil, "entity_name" => String);
    reg!(api, "set_motor_speed", "Set joint motor speed", Nil, "entity_name" => String, "speed" => Float);

    // ── Audio ──
    reg!(api, "play_sound", "Play a sound effect; returns an id that stop_sound / set_sound_volume / set_sound_pitch reach. `loop` repeats it until stopped", Int, "clip_id" => String, "volume" => Float, "pitch" => Float, "pan" => Float, "loop" => Bool);
    reg!(api, "play_sound_at", "Play a one-shot heard from (x, y): louder near the listener, panned to its side", Int, "clip_id" => String, "x" => Float, "y" => Float, "volume" => Float, "pitch" => Float);
    reg!(api, "play_music", "Play background music", Nil, "clip_id" => String, "volume" => Float, "looping" => Bool);
    reg!(api, "stop_sound", "Stop a sound by handle", Nil, "handle_id" => Int);
    reg!(api, "stop_music", "Stop background music", Nil);
    reg!(api, "get_music_position", "Seconds into the music track (0 with none)", Float);
    reg!(api, "get_unix_time", "Seconds since the Unix epoch - the wall clock, for time away", Float);
    reg!(api, "pause_music", "Pause background music", Nil);
    reg!(api, "resume_music", "Resume background music", Nil);
    reg!(api, "set_sound_volume", "Set sound volume", Nil, "handle_id" => Int, "volume" => Float);
    reg!(api, "set_music_volume", "Set music volume", Nil, "volume" => Float);
    reg!(api, "set_master_volume", "Set master volume", Nil, "volume" => Float);
    reg!(api, "stop_sound_by_clip", "Stop every playing instance of one clip", Nil, "clip_id" => String);
    reg!(api, "stop_all_sounds", "Stop every playing sound (and music)", Nil);
    reg!(api, "pause_all_sounds", "Pause every playing sound (and music)", Nil);
    reg!(api, "resume_all_sounds", "Resume every paused sound (and music)", Nil);

    // ── Profiling ──
    reg!(api, "get_fps", "Get current FPS", Float);
    reg!(api, "get_frame_time", "Get frame time in ms", Float);
    reg!(api, "get_entity_count", "Get total entity count", Int);
    reg!(api, "get_draw_calls", "Get draw call count", Int);
    reg!(api, "log_stats", "Log performance stats", Nil);
    reg!(api, "set_debug_overlay", "Toggle debug overlay", Nil, "visible" => Bool);
    reg!(api, "get_delta_time", "Get delta time in seconds", Float);

    // ── Utility ──
    reg!(api, "log", "Log a message", Nil, "message" => String);

    // ── Save Game (2.3) ──
    reg!(api, "save_set", "Queue a write to a save slot (commit to persist)", Nil, "slot" => String, "key" => String, "value" => VarArgs);
    reg!(api, "save_get", "Read a value from a save slot (returns nil if missing)", VarArgs, "slot" => String, "key" => String);
    reg!(api, "save_commit", "Persist the named save slot to disk / localStorage", Nil, "slot" => String);
    reg!(api, "save_clear", "Drop every key from a save slot (deletes the file on commit)", Nil, "slot" => String);

    // ── Audio Mixer (2.4) ──
    reg!(api, "audio_create_bus", "Create a mixer bus (parent default master, volume 0..1)", Nil, "name" => String, "parent" => String, "volume" => Float);
    reg!(api, "audio_remove_bus", "Remove a custom mixer bus (master is protected)", Nil, "name" => String);
    reg!(api, "audio_set_bus_volume", "Set a bus's linear volume (0..1)", Nil, "name" => String, "volume" => Float);
    reg!(api, "audio_set_bus_effect", "Set a bus's effect: lowpass (Hz, 0 off), reverb / room / echo / distortion / compress (0..1), echo_time (s), echo_feedback (0..0.95) or eq_low / eq_mid / eq_high (dB)", Nil, "name" => String, "effect" => String, "value" => Float);
    reg!(api, "audio_crossfade_music", "Fade the music to `clip` over `seconds` (an empty clip fades to silence)", Nil, "clip" => String, "seconds" => Float, "volume" => Float, "looping" => Bool);
    reg!(api, "audio_play_music_layers", "Start stems together, each at its volume, fading in over `seconds`", Nil, "clips" => Any, "volumes" => Any, "seconds" => Float);
    reg!(api, "audio_set_music_layer", "Fade stem `index` (from 1) to `volume` over `seconds`", Nil, "index" => Int, "volume" => Float, "seconds" => Float);
    reg!(api, "audio_get_music_layer", "The volume stem `index` (from 1) is heading to", Float, "index" => Int);
    reg!(api, "audio_set_playlist", "Play tracks through, crossfading into the next: options { crossfade, shuffle, loop, volume }", Nil, "clips" => Any, "options" => Any);
    reg!(api, "audio_next_track", "Crossfade the playlist into its next track now", Nil);
    reg!(api, "audio_get_music_clip", "The clip of the music playing now, empty for none", String);
    reg!(api, "audio_set_music_pitch", "Play the music faster and higher (1 is as recorded)", Nil, "pitch" => Float);
    reg!(api, "audio_seek_music", "Move the music (every stem) to `seconds`", Nil, "seconds" => Float);
    reg!(api, "audio_set_sound_pitch", "Change a playing sound's pitch by the id play_sound returned", Nil, "sound" => Int, "pitch" => Float);
    reg!(api, "audio_follow_time", "Bend music and new one-shots with the world's time scale (off by default)", Nil, "on" => Bool);
    reg!(api, "audio_set_variation", "Vary every one-shot of a clip: pitch up to `pitch` semitones and volume up to `volume` (a fraction) either way; both 0 removes it", Nil, "clip" => String, "pitch" => Float, "volume" => Float);
    reg!(api, "audio_get_bus_volume", "Read a bus's volume (nil if unknown)", VarArgs, "name" => String);
    reg!(api, "audio_set_bus_muted", "Mute / unmute a bus and its descendants", Nil, "name" => String, "muted" => Bool);
    reg!(api, "audio_is_bus_muted", "True if the named bus is muted", Bool, "name" => String);
    reg!(api, "audio_set_bus_solo", "Solo a bus (any solo triggers solo-isolate)", Nil, "name" => String, "solo" => Bool);

    // ── Input Action Remapping (2.5) ──
    reg!(api, "input_get_actions", "List every loaded input action name (sorted)", VarArgs);
    reg!(api, "input_get_bindings", "Get the key list for an action (nil if unknown)", VarArgs, "action" => String);
    reg!(api, "input_set_bindings", "Replace an action's bindings (creates the action if new)", Nil, "action" => String, "keys" => VarArgs);
    reg!(api, "get_action_hold_time", "Seconds an action has been held, 0 while it is up", Float, "action" => String);
    reg!(api, "is_action_held", "True on the one frame an action's press reaches `seconds` (default 0.5) - ask several for charge tiers", Bool, "action" => String, "seconds" => Float);
    reg!(api, "is_action_tapped", "True when an action is released before `seconds` (default 0.25) - tap-or-hold on one button", Bool, "action" => String, "seconds" => Float);
    reg!(api, "is_action_double_tapped", "True on the second of two presses within `seconds` (default 0.3), counted in pairs", Bool, "action" => String, "seconds" => Float);
    reg!(api, "get_gamepad_name", "What the platform calls pad `n`, empty when none", String, "pad" => Int);
    reg!(api, "get_gamepad_type", "A pad's family: xbox, playstation, switch, generic - or empty for none", String, "pad" => Int);
    reg!(api, "get_last_input_device", "The device used last: keyboard, mouse, gamepad, touch or none", String);
    reg!(api, "get_last_gamepad", "The pad used last, or -1", Int);
    reg!(api, "get_action_prompt", "{ label, glyph, binding, style } for an action on the device in use, or in a named style", Any, "action" => String, "style" => String);
    reg!(api, "input_get_typed", "The characters typed this frame, as the keyboard layout made them - `?` on any layout - empty while a text field has the keys", String);
    reg!(api, "input_get_just_pressed", "Every input that went down this frame, as binding names (keys, Pad0_A, Pad0_LeftStickX+) - for a press-a-key rebinding screen", VarArgs);
    reg!(api, "input_add_binding", "Add a key to an action (idempotent)", Nil, "action" => String, "key" => String);
    reg!(api, "input_remove_binding", "Remove a key from an action", Nil, "action" => String, "key" => String);
    reg!(api, "input_clear_bindings", "Clear every binding on an action", Nil, "action" => String);
    reg!(api, "input_reset_bindings", "Restore every action to its load-time defaults", Nil);
    reg!(api, "input_get_dead_zone", "Get an action's dead zone (0..1)", Float, "action" => String);
    reg!(api, "input_set_dead_zone", "Set an action's dead zone (0..1, clamped)", Nil, "action" => String, "value" => Float);

    // ── Bound in every runtime, described nowhere until Oct 2026 ──
    // These had a namespace and a binding but no descriptor, so the editor
    // showed no tooltip, the API index printed `(…)` for their arguments and
    // the AI spec described them as "<Namespace> API function.".
    // Arguments follow the browser preamble's parameter names.
    reg!(api, "find_by_name", "Find the first entity with this name (nil if none)", EntityId, "name" => String);
    reg!(api, "find_child", "A part of an entity by name - its children first, then theirs; a path walks a/b (nil if none)", EntityId, "parent" => EntityId, "path" => String);
    reg!(api, "find_by_tag", "Every entity carrying this tag, as an array", VarArgs, "tag" => String);
    reg!(api, "find_in_radius", "Every entity within radius of (x, y), optionally only those with a tag", VarArgs, "x" => Float, "y" => Float, "radius" => Float, "tag" => String);
    reg!(api, "get_name", "An entity's name (nil if it has none)", String, "entity_id" => EntityId);
    reg!(api, "set_fields", "Write several fields of a component at once from a table", Nil, "entity_id" => EntityId, "component" => String, "fields" => Any);
    reg!(api, "set_parent", "Hang an entity from another, keeping its local transform", Nil, "child" => EntityId, "parent" => EntityId);
    reg!(api, "get_world_position", "An entity's position after its parents are applied", Vec2, "entity_id" => EntityId);
    reg!(api, "get_world_rotation", "An entity's rotation in radians after its parents are applied", Float, "entity_id" => EntityId);
    reg!(api, "get_world_scale", "An entity's scale after its parents are applied", Vec2, "entity_id" => EntityId);
    reg!(api, "set_sprite_corners", "Draw a sprite as this quad: corners TL, TR, BR, BL as offsets from its position (eight numbers or one table)", Nil, "entity_id" => EntityId, "x0" => Float, "y0" => Float, "x1" => Float, "y1" => Float, "x2" => Float, "y2" => Float, "x3" => Float, "y3" => Float);
    reg!(api, "get_material", "The id of an entity's material (nil if it has none)", String, "entity_id" => EntityId);
    reg!(api, "set_material", "Give an entity a material: a built-in name or a .material path", Nil, "entity_id" => EntityId, "material_id" => String);
    reg!(api, "set_material_uniform_float", "Set a float parameter on an entity's material", Nil, "entity_id" => EntityId, "name" => String, "value" => Float);
    reg!(api, "set_material_uniform_int", "Set an integer parameter on an entity's material", Nil, "entity_id" => EntityId, "name" => String, "value" => Int);
    reg!(api, "set_material_uniform_vec2", "Set a vec2 parameter on an entity's material", Nil, "entity_id" => EntityId, "name" => String, "x" => Float, "y" => Float);
    reg!(api, "set_material_uniform_vec4", "Set a vec4 parameter on an entity's material", Nil, "entity_id" => EntityId, "name" => String, "x" => Float, "y" => Float, "z" => Float, "w" => Float);
    reg!(api, "set_text", "Replace an entity's world text", Nil, "entity_id" => EntityId, "text" => String);
    reg!(api, "get_current_animation", "The clip an entity is playing (nil if none)", String, "entity_id" => EntityId);
    reg!(api, "get_anim_state", "The current state of an entity's animation state machine", String, "entity_id" => EntityId);
    reg!(api, "get_animation_time", "Seconds into the current clip", Float, "entity_id" => EntityId);
    reg!(api, "set_animation_time", "Jump the current clip to a time in seconds", Nil, "entity_id" => EntityId, "time" => Float);
    reg!(api, "set_anim_bool", "Set a boolean parameter of the animation state machine", Nil, "entity_id" => EntityId, "param" => String, "value" => Bool);
    reg!(api, "set_anim_float", "Set a number parameter of the animation state machine", Nil, "entity_id" => EntityId, "param" => String, "value" => Float);
    reg!(api, "set_anim_trigger", "Fire a trigger parameter of the animation state machine", Nil, "entity_id" => EntityId, "param" => String);
    reg!(api, "start_state_machine", "Start an entity's animation state machine in a state", Nil, "entity_id" => EntityId, "state" => String);
    reg!(api, "find_skeleton", "Find a skeleton's root entity by name", EntityId, "name" => String);
    reg!(api, "find_bone", "Find a bone under a skeleton root by name", EntityId, "root" => EntityId, "bone_name" => String);
    reg!(api, "set_ik_target", "Point an entity's two-bone IK at a world position", Nil, "entity_id" => EntityId, "x" => Float, "y" => Float);
    reg!(api, "spawn_rig", "Spawn a skeletal rig from a .rig asset, by its path under assets/ (rigs/arm.rig)", EntityId, "rig_id" => String);
    reg!(api, "send_event_to", "Send an event to one entity's script only (graphs do not hear it)", Nil, "event_name" => String, "sender" => EntityId, "target" => EntityId, "data" => Any);
    reg!(api, "get_action_axis", "How far an action is pressed, 0..1 (analog for a stick binding)", Float, "action" => String);
    reg!(api, "get_gamepad_axis", "A gamepad axis, -1..1; stick up is positive", Float, "pad" => Int, "axis" => Int);
    reg!(api, "get_gamepad_count", "How many gamepad slots have been seen, connected or not", Int);
    reg!(api, "get_mouse_position", "The mouse in screen pixels", Vec2);
    reg!(api, "get_mouse_world_position", "The mouse in world units, through the camera", Vec2);
    reg!(api, "is_gamepad_button_pressed", "Whether a gamepad button is held", Bool, "pad" => Int, "button" => Int);
    reg!(api, "is_gamepad_connected", "Whether a gamepad slot has a pad in it", Bool, "pad" => Int);
    reg!(api, "is_mouse_left_pressed", "Whether the left mouse button is held", Bool);
    reg!(api, "is_mouse_left_just_pressed", "Whether the left mouse button was pressed this frame", Bool);
    reg!(api, "is_mouse_left_just_released", "Whether the left mouse button was released this frame", Bool);
    reg!(api, "contact_count", "How many contact rows the last physics step reported", Int);
    reg!(api, "get_contact", "One contact row (1-based): entity a, entity b, and whether it started", VarArgs, "index" => Int);
    reg!(api, "get_screen_size", "The window size in physical pixels", Vec2);
    reg!(api, "set_ambient_light", "Set the scene's ambient light colour and intensity", Nil, "r" => Float, "g" => Float, "b" => Float, "intensity" => Float);
    reg!(api, "set_light_color", "Set a light's colour", Nil, "entity_id" => EntityId, "r" => Float, "g" => Float, "b" => Float);
    reg!(api, "set_color_lut", "Grade the frame through a colour look-up strip (texture id, strength 0-1); an empty id removes it", Nil, "texture_id" => String, "strength" => Float);
    reg!(api, "set_light_mask", "Set the light layers a light lights (a bitmask; a sprite's light_layers must share a bit)", Nil, "entity_id" => EntityId, "mask" => Int);
    reg!(api, "set_light_height", "Set how high a light sits (tangent of its elevation; 0.5 default)", Nil, "entity_id" => EntityId, "height" => Float);
    reg!(api, "set_light_intensity", "Set a light's intensity", Nil, "entity_id" => EntityId, "intensity" => Float);
    reg!(api, "set_light_radius", "Set a light's radius", Nil, "entity_id" => EntityId, "radius" => Float);
    reg!(api, "set_post_process_enabled", "Turn a post effect on or off", Nil, "effect" => String, "enabled" => Bool);
    reg!(api, "set_post_process_float", "Set a number parameter of a post effect", Nil, "effect" => String, "param" => String, "value" => Float);
    reg!(api, "set_post_process_vec4", "Set a vec4 parameter of a post effect", Nil, "effect" => String, "param" => String, "x" => Float, "y" => Float, "z" => Float, "w" => Float);
    reg!(api, "persistent_set_number", "Store a number that survives restarts", Nil, "key" => String, "value" => Float);
    reg!(api, "persistent_get_number", "Read a stored number (nil if missing)", Float, "key" => String);
    reg!(api, "persistent_set_string", "Store a string that survives restarts", Nil, "key" => String, "value" => String);
    reg!(api, "persistent_get_string", "Read a stored string (nil if missing)", String, "key" => String);
    reg!(api, "persistent_has", "Whether a persistent key exists", Bool, "key" => String);
    reg!(api, "persistent_clear", "Forget every persistent key", Nil);
    reg!(api, "get_data", "Read a field of a row in a data table", Any, "table" => String, "id" => String, "field" => String);
    reg!(api, "call", "Call a method on another entity's script, synchronously (number/string/bool/nil arguments)", Nil, "caller" => EntityId, "target" => EntityId, "method" => String, "args" => VarArgs);
    reg!(api, "load_script", "Attach a Lua or JS script to an entity at runtime", Nil, "entity_id" => EntityId, "path" => String);

    log::info!("Registered {} core script API functions", api.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_all_core_script_functions() {
        let mut api = ScriptApiRegistry::new();
        register_core_script_api(&mut api);

        // Verify a reasonable number of functions registered
        assert!(api.len() >= 100, "Expected 100+ functions, got {}", api.len());

        // Verify some key functions exist
        let names = api.function_names();
        assert!(names.contains(&"get_position"));
        assert!(names.contains(&"set_position"));
        assert!(names.contains(&"is_key_pressed"));
        assert!(names.contains(&"apply_force"));
        assert!(names.contains(&"play_sound"));
        assert!(names.contains(&"log"));

        // All functions should be from jengine-core
        for func in api.iter() {
            assert_eq!(func.package, "jengine-core");
        }
    }
}
