// Fengari (real Lua 5.3 VM in JS) runtime for the editor viewport.
//
// Sets `globalThis.__jengine_lua_compile(label, luaSource) -> {init, update,
// on_event, on_gui_click, lookup}` so the viewport WASM can route Lua scripts
// through a real Lua VM instead of the brittle string-based Lua\u2192JS transpiler.
//
// All bridge functions are pulled lazily from `window.__jengine_play.__api_lookup`
// (installed by `jengine_core::scripting::bridge::adapters::js::fengari`) so we
// don't have to keep a duplicate registry in sync with `browser.rs`.

import { lua, lauxlib, lualib, to_luastring, to_jsstring } from 'fengari-web';

// ---------- bridge function registry ----------
// Bare-name globals exposed to Lua scripts (mirror of `build_wrapper` in
// crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs).
const BRIDGE_FUNCTIONS: string[] = [
  // 2.3 save system. `save_get` reads the `save_data` mirror the host
  // republishes each drain; the other three queue commands.
  'save_set', 'save_get', 'save_commit', 'save_clear',
  // 3.5.A hierarchy. World-transform reads come from the `world_transforms`
  // mirror; `set_parent` queues a command.
  'set_parent', 'get_world_position', 'get_world_rotation', 'get_world_scale',
  // 2.6 tilemap pathfinding. Synchronous, answered by a host callback over a
  // snapshot of the tilemaps - one A* implementation, not two.
  'tilemap_find_path', 'tilemap_find_path_world',
  // Hex-aware cell neighbours and step counts, answered the same way.
  'tilemap_neighbors', 'tilemap_distance',
  // A one-shot heard from where it happens (positional audio).
  'play_sound_at',
  // Screen flash — the fades were already here; this one was native-only.
  'screen_flash',
  // Targeted events. The broadcast `send_event` was already available.
  'send_event_to',
  // Cross-script method dispatch.
  'call',
  // 3.5.B/C skeletal rigs. `spawn_rig` spawns a `.rig` the editor pushed in
  // (`viewport_load_skeleton_rig`); until Oct 2026 no runtime loaded one.
  'spawn_rig', 'find_skeleton', 'find_bone', 'get_animation_time', 'set_animation_time',
  'set_ik_target', 'set_ik_weight', 'crossfade_animation', 'set_animation_blend', 'set_animation_blend_position',
  'animation_play_layer', 'animation_stop_layer', 'animation_set_layer_weight', 'set_conveyor_speed',
  // 2.4 audio mixer busses. Reads come from the `audio_buses` mirror.
  'audio_create_bus', 'audio_remove_bus', 'audio_set_bus_volume', 'audio_set_bus_effect', 'audio_set_variation', 'audio_follow_time',
  'audio_get_bus_volume', 'audio_set_bus_muted', 'audio_is_bus_muted',
  'audio_set_bus_solo',
  // 2.5 input remapping. Reads come from the `input_actions` mirror.
  'input_get_actions', 'input_get_bindings', 'input_get_just_pressed', 'input_get_typed', 'input_get_dead_zone',
  'input_set_bindings', 'input_add_binding', 'input_remove_binding',
  'input_clear_bindings', 'input_reset_bindings', 'input_set_dead_zone',
  // transforms
  'get_position', 'set_position', 'get_rotation', 'set_rotation',
  'get_scale', 'set_scale',
  // logging
  'log',
  // sprites
  'get_sprite_color', 'set_sprite_color',
  'get_sprite_anchor', 'set_sprite_anchor', 'set_sprite_size', 'get_sprite_point', 'set_z_order', 'set_text_color', 'set_trail_emitting', 'clear_trail', 'carve_sprite', 'paint_sprite', 'press_sprite', 'move_character', 'get_character',
  'is_mouse_right_pressed', 'is_mouse_right_just_pressed', 'is_mouse_right_just_released',
  'is_mouse_middle_pressed', 'is_mouse_middle_just_pressed', 'is_mouse_middle_just_released',
  'set_sprite_region', 'set_sprite_label', 'set_sprite_corners',
  // entity / scene queries
  'get_entity_by_name', 'find_by_name', 'find_child', 'get_name',
  'destroy_entity', 'spawn_entity', 'spawn_prefab',
  'add_tag', 'remove_tag', 'has_tag', 'get_entities_by_tag', 'find_by_tag',
  'find_in_radius',
  // time
  'get_delta_time',
  // input
  'is_key_pressed', 'is_key_just_pressed',
  'get_mouse_position', 'is_mouse_left_just_pressed',
  'is_mouse_left_pressed', 'is_mouse_left_just_released',
  'is_action_pressed', 'is_action_just_pressed', 'is_action_just_released',
  // Analog input (BUGS #9). `get_action_axis` is the rebindable path and the
  // one a game should prefer; the `gamepad_*` four are raw device access for
  // when a stick really is a stick.
  'get_action_axis',
  'get_gamepad_count', 'is_gamepad_connected',
  'get_gamepad_axis', 'is_gamepad_button_pressed',
  'get_mouse_wheel', 'get_mouse_delta', 'get_screen_size', 'get_mouse_world_position', 'get_touches', 'get_gestures',
  'get_camera_position', 'set_camera_follow', 'set_camera_screen_rect', 'set_camera_field',
  // custom components
  'get_custom', 'set_custom', 'set_fields',
  // combat
  'set_health',
  'get_health',
  'set_hitbox',
  // data-oriented design tables
  'get_data',
  // events
  'send_event', 'register_event_listener', 'remove_event_listener', 'on_event',
  // camera
  'camera_shake',
  'set_camera_zoom', 'get_camera_zoom', 'set_camera_pixel_size',
  // debug draw / overlays
  'draw_debug_line', 'draw_debug_rect', 'draw_debug_circle',
  'set_debug_overlay', 'log_stats',
  'get_fps', 'get_music_position', 'get_unix_time', 'get_frame_time', 'get_entity_count', 'get_draw_calls',
  // utilities
  'lerp', 'clamp', 'distance', 'angle_between',
  // timers
  'set_timer', 'set_interval', 'cancel_timer',
  // scenes
  'load_scene', 'quit_game', 'set_time_scale', 'get_time_scale', 'push_scene', 'pop_scene', 'get_current_scene_name',
  'load_script',
  'fade_in', 'fade_out',
  // persistent k/v (survives scene transitions)
  'persistent_set_number', 'persistent_get_number',
  'persistent_set_string', 'persistent_get_string',
  'persistent_has', 'persistent_clear',
  // animation
  'play_animation', 'stop_animation', 'set_animation_speed', 'is_animation_playing',
  'get_current_animation', 'set_anim_bool', 'set_anim_float', 'set_anim_trigger',
  'start_state_machine', 'get_anim_state',
  // tween
  'tween_position', 'tween_rotation', 'tween_scale', 'tween_color',
  'cancel_tween', 'cancel_all_tweens',
  // tilemap
  'get_tile', 'get_tile_data', 'set_tile', 'get_tile_at_world', 'world_to_tile', 'tile_to_world',
  // particles
  'emit_burst', 'set_emitter_active', 'set_emitter_rate', 'set_emitter_field', 'get_particle_count',
  // cloth and rope
  'set_cloth_field',
  'set_path_follow_field',
  'set_line_field',
  'set_trail_field',
  'set_fluid_field',
  'set_mirror_field',
  'set_sprite_fill',
  'pixel_paint',
  'smoke_puff',
  'set_smoke_field',
  'set_rewind_field',
  'set_fog_field',
  'wfc_generate',
  'set_vision_field',
  'set_day_field',
  'set_origin_field',
  'set_time_scale_field',
  'set_cast_shadow_field',
  'set_auto_normal_field',
  'set_tile_light_field',
  'set_trajectory_field',
  'set_text_path_field',
  'set_beat_clock_field',
  'smooth_terrain_edit',
  'dialogue_start', 'dialogue_advance', 'dialogue_choose', 'dialogue_stop',
  'ui_drop', 'mode7_place', 'mode7_set', 'scene_transition', 'sprite_tilt_set', 'afterimage_set', 'physics_slice', 'portal_set', 'jelly_poke', 'jelly_set', 'marker_set', 'radial_set', 'bubble_say', 'bubble_set', 'outline_set', 'lightning_strike', 'lightning_set', 'flow_set', 'nav_link_set', 'morph_to', 'morph_set', 'toast_show', 'toast_set', 'replay_record', 'replay_play', 'replay_stop', 'replay_clear', 'replay_status', 'replay_frames', 'replay_progress', 'replay_live_key', 'snow_set', 'fog_of_war_reset', 'fog_of_war_set',
  'state_save', 'state_load', 'state_has', 'state_forget',
  'set_water_field',
  'water_splash',
  'set_canvas_group_field',
  'set_video_field',
  'set_ragdoll_field',
  'set_timeline_field', 'set_behavior_tree_field',
  // area effects
  'set_area_field',
  // navigation
  'set_nav_target', 'stop_nav', 'set_nav_field', 'nav_find_path', 'nav_closest_point', 'nav_is_moving',
  // rumble and vibration
  'input_rumble', 'input_vibrate',
  // the web portal a build was made for (docs/web-platforms.md)
  'platform_name', 'platform_game_ready', 'platform_gameplay_start', 'platform_gameplay_stop',
  'platform_happy_time', 'platform_submit_score', 'platform_show_ad',
  // Hold / tap / double-tap, pad names and families, prompts.
  'get_action_hold_time', 'is_action_held', 'is_action_tapped', 'is_action_double_tapped',
  'get_gamepad_name', 'get_gamepad_type', 'get_last_input_device', 'get_last_gamepad', 'get_action_prompt',
  // Crossfades, stem layers, playlists, pitch and seek (the music director).
  'audio_crossfade_music', 'audio_play_music_layers', 'audio_set_music_layer', 'audio_get_music_layer',
  'audio_set_playlist', 'audio_next_track', 'audio_get_music_clip', 'audio_set_music_pitch', 'audio_seek_music',
  'audio_set_sound_pitch',
  // localization
  'locale_get', 'locale_set', 'locale_current', 'locale_list', 'locale_name',
  // display and cursor settings
  'display_set_mode', 'display_get_mode', 'display_set_size', 'display_get_size', 'display_get_resolutions',
  'display_set_vsync', 'display_get_vsync', 'display_set_max_fps', 'display_get_max_fps', 'display_supports',
  'cursor_set_visible', 'cursor_set_confined', 'cursor_set_locked', 'cursor_set_icon', 'cursor_set_image',
  // noise
  'noise_perlin', 'noise_perlin3', 'noise_simplex', 'noise_cellular', 'noise_fractal', 'noise_grid',
  // the spectrum of the mix, synthesised notes
  'audio_get_spectrum', 'audio_get_level', 'audio_synth',
  // screenshots
  'screenshot_take', 'screenshot_ready',
  // a screen reader
  'accessibility_announce', 'accessibility_speech',
  // scenes loaded in the background and laid into this one
  'scene_load_additive', 'scene_unload', 'scene_preload', 'scene_status', 'scene_progress',
  // asset packs, downloaded when needed
  'pack_download', 'pack_remove', 'pack_set_url', 'pack_status', 'pack_progress', 'pack_size',
  'pack_error', 'pack_of', 'pack_list',
  'liveops_get', 'liveops_keys', 'liveops_status', 'liveops_error', 'liveops_fetch',
  'liveops_fetching', 'liveops_variant', 'liveops_events', 'liveops_event_active',
  'liveops_event_ends', 'liveops_event_starts', 'liveops_event_name', 'liveops_set_player',
  'liveops_server_time', 'liveops_revision', 'liveops_release', 'liveops_updated',
  'liveops_expose', 'liveops_set_attribute',
  // ui / gui
  'create_ui_text', 'create_ui_panel', 'set_ui_text', 'set_ui_prop', 'get_ui_prop', 'set_text',
  'set_ui_visible', 'set_ui_position',
  'gui_show', 'gui_hide', 'gui_set', 'gui_get', 'gui_on_click', 'gui_focus',
  // physics
  'apply_force', 'apply_impulse', 'set_velocity', 'get_velocity',
  'set_angular_velocity', 'get_angular_velocity',
  'set_gravity', 'get_gravity', 'set_body_type', 'get_contacts', 'raycast',
  // Shape queries: what overlaps a circle, box or point, and a swept circle.
  'overlap_circle', 'overlap_box', 'overlap_point', 'circle_cast',
  'add_joint', 'remove_joint', 'set_motor_speed',
  'contact_count', 'get_contact',
  // audio
  'play_sound', 'play_music', 'stop_sound', 'stop_music',
  'pause_music', 'resume_music',
  'stop_all_sounds', 'pause_all_sounds', 'resume_all_sounds', 'stop_sound_by_clip',
  'set_sound_volume', 'set_music_volume', 'set_master_volume',
  // material / postprocess
  'set_material', 'get_material', 'set_normal_map',
  'set_material_uniform_float', 'set_material_uniform_vec2',
  'set_material_uniform_vec4', 'set_material_uniform_int',
  'set_post_process_enabled', 'set_post_process_param_float',
  'set_post_process_param_vec4',
  'set_post_process_float', 'set_post_process_vec4',
  // lighting
  'set_ambient_light', 'set_light_color', 'set_light_intensity', 'set_light_height', 'set_light_mask', 'set_color_lut', 'set_mesh', 'set_toon_lighting', 'set_ambient_occlusion', 'set_global_illumination', 'set_light_radius',
  // print is special (alias for log)
];

// Functions whose JS return value `[a, b, ...]` should land on the Lua stack
// as multiple return values (so `local x, y = get_position(e)` works) instead
// of a single Lua table. Mirrors mlua's native behaviour.
// Bridge functions that return several values. The JS side hands back an
// array; only a name in this set is unpacked into separate Lua values -
// anything else arrives as ONE table. So a native function returning a tuple
// and missing here is a different function in this runtime: `get_health`
// was, and `local hp, max = get_health(e)` gave a table and nil, which the
// first arithmetic on it turned into an error that killed the whole update
// (test-lab scene 36). `multi_return_matches_native` fails on a gap.
const MULTI_RETURN: Set<string> = new Set([
  'get_health', 'get_contact',
  'get_position', 'get_scale', 'get_sprite_color', 'get_velocity',
  'get_mouse_position', 'get_mouse_delta', 'get_screen_size', 'get_mouse_world_position', 'get_sprite_anchor', 'get_sprite_point',
  'get_gravity', 'world_to_tile', 'tile_to_world', 'nav_closest_point', 'get_camera_position',
  'get_world_position', 'get_world_scale', 'display_get_size', 'noise_cellular',
]);

/**
 * Script namespaces - `Sprite.set_color` and the rest - as the host
 * published them: `[namespace, short name, flat global]` rows from
 * `jengine_core::scripting::namespaces`, the one list every runtime builds
 * from. Native builds the same tables from the same rows.
 */
function scriptNamespaces(): [string, string, string][] {
  const w: any = globalThis as any;
  const play = w.window && w.window.__jengine_play;
  const rows = play && play.__script_namespaces;
  return Array.isArray(rows)
    ? rows.filter(
        (r: any) => Array.isArray(r) && r.length === 3 && r.every((x: any) => typeof x === 'string'),
      )
    : [];
}

// ---------- Lua <-> JS conversion helpers ----------

function luaToJs(L: any, idx: number): any {
  const t = lua.lua_type(L, idx);
  if (t === lua.LUA_TNIL || t === lua.LUA_TNONE) return null;
  if (t === lua.LUA_TBOOLEAN) return lua.lua_toboolean(L, idx);
  if (t === lua.LUA_TNUMBER) return lua.lua_tonumber(L, idx);
  if (t === lua.LUA_TSTRING) return lua.lua_tojsstring(L, idx);
  if (t === lua.LUA_TTABLE) {
    // Detect array vs hash by seeing if rawlen > 0 and indices 1..n are present.
    const len = lua.lua_rawlen(L, idx);
    if (len > 0) {
      const arr: any[] = [];
      for (let i = 1; i <= len; i++) {
        lua.lua_rawgeti(L, idx, i);
        arr.push(luaToJs(L, -1));
        lua.lua_pop(L, 1);
      }
      return arr;
    }
    // hash table -> plain object
    const obj: Record<string, any> = {};
    let keys = 0;
    lua.lua_pushnil(L);
    const absIdx = idx > 0 ? idx : lua.lua_gettop(L) + idx;
    while (lua.lua_next(L, absIdx) !== 0) {
      keys++;
      // key at -2, value at -1
      let key: string;
      const kt = lua.lua_type(L, -2);
      if (kt === lua.LUA_TSTRING) key = lua.lua_tojsstring(L, -2);
      else if (kt === lua.LUA_TNUMBER) key = String(lua.lua_tonumber(L, -2));
      else key = '';
      obj[key] = luaToJs(L, -1);
      lua.lua_pop(L, 1); // pop value, keep key for next iter
    }
    // An empty table is an empty list, as on native (luau.rs's
    // `lua_value_to_json`): Lua cannot tell `[]` from `{}`, and an empty
    // list is what a script sets - `Line.set(e, "points", {})`.
    if (keys === 0) return [];
    return obj;
  }
  // userdata / functions / threads — opaque, just return null.
  return null;
}

function jsToLua(L: any, value: any): void {
  if (value === undefined || value === null) {
    lua.lua_pushnil(L);
    return;
  }
  const tp = typeof value;
  if (tp === 'number') {
    lua.lua_pushnumber(L, value);
    return;
  }
  if (tp === 'boolean') {
    lua.lua_pushboolean(L, value ? 1 : 0);
    return;
  }
  if (tp === 'string') {
    lua.lua_pushstring(L, to_luastring(value));
    return;
  }
  if (Array.isArray(value)) {
    // The wrapper's `__lua_arr(...)` returns a Proxy whose indexed `get`
    // intercept only responds to keys >= 1 (Lua-style). For those, we must
    // read `value[i+1]` — reading `value[i]` (0-indexed) would yield nil for
    // i=0 and shift everything off by one, producing all-nil Lua tables
    // (root cause of `find_by_tag("player")` returning nothing in editor
    // play mode → HUD reports GAME OVER instantly).
    const isLuaArr = (value as any).__lua_arr === true;
    lua.lua_createtable(L, value.length, 0);
    for (let i = 0; i < value.length; i++) {
      const v = isLuaArr ? (value as any)[i + 1] : value[i];
      jsToLua(L, v);
      lua.lua_rawseti(L, -2, i + 1);
    }
    return;
  }
  if (tp === 'object') {
    // Prefer Proxy-friendly key iteration via Object.keys.
    const keys = Object.keys(value);
    lua.lua_createtable(L, 0, keys.length);
    for (const k of keys) {
      jsToLua(L, (value as any)[k]);
      lua.lua_setfield(L, -2, to_luastring(k));
    }
    return;
  }
  // functions / symbols — push nil.
  lua.lua_pushnil(L);
}

// Build a Lua C-function (signature `(L) => number`) that pulls args off the
// Lua stack, calls the JS bridge function, and pushes the return value(s).
function makeBridge(jsFn: Function, name: string): (L: any) => number {
  const isMulti = MULTI_RETURN.has(name);
  return (L: any) => {
    const nargs = lua.lua_gettop(L);
    const args: any[] = new Array(nargs);
    for (let i = 0; i < nargs; i++) args[i] = luaToJs(L, i + 1);
    let result: any;
    try {
      result = jsFn.apply(null, args);
    } catch (err) {
      lua.lua_pushstring(L, to_luastring(`bridge ${name}: ${(err as Error).message || err}`));
      return lua.lua_error(L);
    }
    // A command returns `undefined` and gives Lua nothing, as a native `()`
    // does. A getter with no answer returns `null`, and that is one `nil`, as
    // native's `None` is: zero values made `tonumber(Save.get(s, k))` on a
    // missing key a `tonumber()`, which Lua 5.3 refuses ("value expected")
    // while Luau is happy - the showcase hub died in init on the web only.
    if (result === undefined) return 0;
    if (result === null) {
      lua.lua_pushnil(L);
      return 1;
    }
    if (isMulti && Array.isArray(result)) {
      for (const v of result) jsToLua(L, v);
      return result.length;
    }
    jsToLua(L, result);
    return 1;
  };
}

function getApiLookup(): ((name: string) => Function | null) | null {
  const w: any = globalThis as any;
  const play = w.window && w.window.__jengine_play;
  if (!play) return null;
  const fn = play.__api_lookup;
  return typeof fn === 'function' ? fn : null;
}

/**
 * Script globals contributed by packages, published by the host.
 *
 * `BRIDGE_FUNCTIONS` is a hardcoded allowlist of core's surface, so a
 * package's globals could never appear in it — that is precisely why a
 * package's script API used to be `nil` in this runtime. The host writes the
 * names it registered onto `__jengine_play`, and they are installed through
 * exactly the same lookup as core's.
 */
function packageFunctionNames(): string[] {
  const w: any = globalThis as any;
  const play = w.window && w.window.__jengine_play;
  const names = play && play.__package_api_names;
  return Array.isArray(names) ? names.filter((n: any) => typeof n === 'string') : [];
}

/** Where the Lua code that called into the host is: `path:line`, or '' when
 *  that cannot be told (a call from the host itself, a chunk with no lines). */
function callerLocation(L: any): string {
  try {
    const ar = new lua.lua_Debug();
    if (!lua.lua_getstack(L, 1, ar)) return '';
    lua.lua_getinfo(L, to_luastring('Sl'), ar);
    if (!(ar.currentline > 0)) return '';
    return `${to_jsstring(ar.short_src)}:${ar.currentline}`;
  } catch (_e) {
    return '';
  }
}

/** A log line that carries where it was written: `[@path:line] text`. Only
 *  when the host asks (`__jengine_log_locations`, set by the editor, whose
 *  Console reads the tag back off in `viewportLog.ts` and shows it as the
 *  row's link) - a deployed web build's console gets the plain line. */
function located(L: any, text: string): string {
  if ((globalThis as any).__jengine_log_locations !== true) return text;
  const at = callerLocation(L);
  return at ? `[@${at}] ${text}` : text;
}

function registerBridge(L: any, lookup: (name: string) => Function | null): void {
  // Bare globals. Core's allowlist plus whatever the packages registered.
  for (const name of [...BRIDGE_FUNCTIONS, ...packageFunctionNames()]) {
    const fn = lookup(name);
    if (typeof fn !== 'function') continue;
    // `log` is told where it was called from, as `print` is below.
    const bridged = name === 'log' ? makeBridge(fn, name) : null;
    lua.lua_pushjsfunction(
      L,
      bridged
        ? (Lp: any) => {
            if (lua.lua_gettop(Lp) >= 1 && lua.lua_isstring(Lp, 1)) {
              lua.lua_pushstring(Lp, to_luastring(located(Lp, lua.lua_tojsstring(Lp, 1))));
              lua.lua_replace(Lp, 1);
            }
            return bridged(Lp);
          }
        : makeBridge(fn, name),
    );
    lua.lua_setglobal(L, to_luastring(name));
  }
  // Standard Lua `print` -> log bridge.
  const logFn = lookup('log');
  if (typeof logFn === 'function') {
    lua.lua_pushjsfunction(L, (Lp: any) => {
      const n = lua.lua_gettop(Lp);
      const parts: string[] = [];
      for (let i = 1; i <= n; i++) {
        if (lua.lua_isstring(Lp, i)) parts.push(lua.lua_tojsstring(Lp, i));
        else parts.push(String(luaToJs(Lp, i)));
      }
      try { logFn(located(Lp, parts.join('\t'))); } catch (_e) { /* swallow */ }
      return 0;
    });
    lua.lua_setglobal(L, to_luastring('print'));
  }
  // Namespace tables, from the published rows. Each field is the flat
  // global's own function, so `Sprite.get_color` unpacks its four values
  // exactly as `get_sprite_color` does - the hand-made tables this replaces
  // wrapped by the short name, and `sprite.get_color` returned one table
  // instead of four numbers. `lua-runtime.ts` used to add `lerp` and friends
  // to Lua's own `math` too, which native's sandbox makes read-only; they are
  // `Math.lerp` now, in both.
  const tables = new Map<string, [string, string][]>();
  for (const [ns, short, flat] of scriptNamespaces()) {
    if (!tables.has(ns)) tables.set(ns, []);
    tables.get(ns)!.push([short, flat]);
  }
  // Each field is the flat global's own function object, installed above -
  // so `Sprite.set_color == set_sprite_color`, as it is natively.
  for (const [ns, fields] of tables) {
    lua.lua_createtable(L, 0, fields.length);
    for (const [short, flat] of fields) {
      lua.lua_getglobal(L, to_luastring(flat));
      if (!lua.lua_isfunction(L, -1)) {
        lua.lua_pop(L, 1);
        continue;
      }
      lua.lua_setfield(L, -2, to_luastring(short));
    }
    lua.lua_setglobal(L, to_luastring(ns));
  }
  // Lua-side `require` backed by `window.__jengine_play.prefab_scripts`
  // (populated by the editor when entering Play mode). Mirrors mlua's
  // package-loader behaviour: cached after first load, returns whatever
  // the module chunk returns (or `true` if it returns nothing).
  installRequire(L);
}

// Resolve a `require("path")` argument against the prefab_scripts cache.
// Tries: path, path.lua, path.luau, assets/path, assets/path.lua, assets/path.luau,
// and dot-to-slash conversion (so `require "lib.foo"` -> `lib/foo.lua`).
function findModuleSource(rawPath: string): { source: string; canonical: string } | null {
  const w: any = globalThis as any;
  const play = w.window && w.window.__jengine_play;
  const scripts = play && (play.prefab_scripts as Record<string, string> | undefined);
  if (!scripts) return null;
  const dotted = rawPath.replace(/\./g, '/');
  const candidates = [
    rawPath,
    `${rawPath}.lua`,
    `${rawPath}.luau`,
    `assets/${rawPath}`,
    `assets/${rawPath}.lua`,
    `assets/${rawPath}.luau`,
    // Native mlua require is rooted at `assets/scripts/`, so a script
    // doing `require("lua/nav")` expects `assets/scripts/lua/nav.lua`.
    // Mirror that here so editor preview matches native resolution.
    `scripts/${rawPath}`,
    `scripts/${rawPath}.lua`,
    `scripts/${rawPath}.luau`,
    `assets/scripts/${rawPath}`,
    `assets/scripts/${rawPath}.lua`,
    `assets/scripts/${rawPath}.luau`,
    dotted,
    `${dotted}.lua`,
    `${dotted}.luau`,
    `assets/${dotted}`,
    `assets/${dotted}.lua`,
    `assets/${dotted}.luau`,
    `scripts/${dotted}`,
    `scripts/${dotted}.lua`,
    `scripts/${dotted}.luau`,
    `assets/scripts/${dotted}`,
    `assets/scripts/${dotted}.lua`,
    `assets/scripts/${dotted}.luau`,
  ];
  for (const c of candidates) {
    const src = scripts[c];
    if (typeof src === 'string') return { source: src, canonical: c };
  }
  return null;
}

// ---------- script inheritance (`extends`) ----------
//
// A script can declare `script = { extends = "lib/npc" }`: the module that
// path names is loaded first, and whatever the script does not define itself
// - `init`, `update`, a handler - is looked up on the table that module
// returns, then on that module's own parent, and so on. `super` is that
// table, for calling the parent explicitly. A module can extend another the
// same way. This is the native Luau adapter's rule (`adapters/luau.rs`
// `load` and `load_module_inner`), transcribed; until Sep 2026 this runtime
// ignored `extends` entirely, so a script that inherited its `update` stood
// still in the editor and on the web.

function isIdentChar(c: string | undefined): boolean {
  return c !== undefined && /[A-Za-z0-9_]/.test(c);
}

function skipWs(s: string, i: number): number {
  while (i < s.length && ' \t\r\n'.includes(s[i])) i++;
  return i;
}

/** The inside of the first `script = { ... }` table, as `metadata.rs` finds it. */
/**
 * If a comment starts at `i`, the index just past it - native's
 * `metadata::comment_end`: Lua `--` and `--[[ ]]` (any number of `=`), JS
 * `//` and `/* *\/`. A header a comment describes is not a header.
 */
function commentEnd(s: string, i: number): number | null {
  const lineEnd = () => {
    const n = s.indexOf('\n', i);
    return n < 0 ? s.length : n;
  };
  if (s.startsWith('--', i)) {
    const long = /^--\[(=*)\[/.exec(s.slice(i, i + 64));
    if (long) {
      const close = ']' + long[1] + ']';
      const n = s.indexOf(close, i + long[0].length);
      return n < 0 ? s.length : n + close.length;
    }
    return lineEnd();
  }
  if (s.startsWith('//', i)) return lineEnd();
  if (s.startsWith('/*', i)) {
    const n = s.indexOf('*/', i + 2);
    return n < 0 ? s.length : n + 2;
  }
  return null;
}

function scriptTableBody(source: string): string | null {
  for (let i = 0; i + 6 <= source.length; i++) {
    const end = commentEnd(source, i);
    if (end !== null) {
      i = end - 1;
      continue;
    }
    if (!source.startsWith('script', i)) continue;
    if (isIdentChar(source[i - 1]) || isIdentChar(source[i + 6])) continue;
    let j = skipWs(source, i + 6);
    if (source[j] === '=') j = skipWs(source, j + 1);
    if (source[j] !== '{') continue;
    let depth = 1;
    for (let k = j + 1; k < source.length; k++) {
      if (source[k] === '{') depth++;
      else if (source[k] === '}' && --depth === 0) return source.slice(j + 1, k);
    }
    return null;
  }
  return null;
}

/**
 * The `extends` path a script declares, or null. Same rule as native's
 * `metadata::parse_extends`: a quoted string after `extends =` (or `:`)
 * inside the script's `script = { ... }` header.
 */
export function parseExtends(source: string): string | null {
  const body = scriptTableBody(source);
  if (body === null) return null;
  for (let i = body.indexOf('extends'); i >= 0; i = body.indexOf('extends', i + 1)) {
    if (isIdentChar(body[i - 1]) || isIdentChar(body[i + 7])) continue;
    let j = skipWs(body, i + 7);
    if (body[j] !== '=' && body[j] !== ':') continue;
    j = skipWs(body, j + 1);
    const quote = body[j];
    if (quote !== '"' && quote !== "'") continue;
    const end = body.indexOf(quote, j + 1);
    if (end > j) return body.slice(j + 1, end);
  }
  return null;
}

const REQUIRE_CACHE = '__jengine_require_cache';
const REQUIRE_LOADING = '__jengine_require_loading';

/**
 * Load a module and push what it returns; the one loader behind both
 * `require` and `extends`, as on native. Raises a Lua error on failure, so
 * call it from inside a protected call.
 *
 * Each module runs in its own environment that reads through to the globals,
 * so a module's top-level `function f()` stays in the module - native has
 * always done this, and before Sep 2026 this runtime ran modules straight in
 * the requiring script's globals instead.
 */
function loadModule(L: any, path: string): void {
  lua.lua_getfield(L, lua.LUA_REGISTRYINDEX, to_luastring(REQUIRE_CACHE));
  lua.lua_getfield(L, -1, to_luastring(path));
  if (!lua.lua_isnil(L, -1)) {
    lua.lua_remove(L, -2);
    return;
  }
  lua.lua_pop(L, 2);

  lua.lua_getfield(L, lua.LUA_REGISTRYINDEX, to_luastring(REQUIRE_LOADING));
  const loadingIdx = lua.lua_gettop(L);
  lua.lua_getfield(L, loadingIdx, to_luastring(path));
  const inProgress = lua.lua_toboolean(L, -1);
  lua.lua_pop(L, 1);
  if (inProgress) {
    lua.lua_pushstring(L, to_luastring(`require('${path}'): circular dependency detected`));
    lua.lua_error(L);
    return;
  }

  const found = findModuleSource(path);
  if (!found) {
    lua.lua_pushstring(L, to_luastring(`require('${path}'): module not found in prefab_scripts`));
    lua.lua_error(L);
    return;
  }

  lua.lua_pushboolean(L, 1);
  lua.lua_setfield(L, loadingIdx, to_luastring(path));
  const clearLoading = () => {
    lua.lua_pushnil(L);
    lua.lua_setfield(L, loadingIdx, to_luastring(path));
  };

  // The parent first, while this module is still marked loading, so
  // `A extends B extends A` is caught rather than recursing forever.
  const parentPath = parseExtends(found.source);
  let parentIdx = 0;
  if (parentPath !== null) {
    lua.lua_pushjsfunction(L, (Lp: any) => {
      loadModule(Lp, parentPath);
      return 1;
    });
    if (lua.lua_pcall(L, 0, 1, 0) !== lua.LUA_OK) {
      clearLoading();
      lua.lua_error(L);
      return;
    }
    parentIdx = lua.lua_gettop(L);
  }

  // env = setmetatable({ super = parent }, { __index = _G })
  lua.lua_createtable(L, 0, 1);
  const envIdx = lua.lua_gettop(L);
  lua.lua_createtable(L, 0, 1);
  lua.lua_pushglobaltable(L);
  lua.lua_setfield(L, -2, to_luastring('__index'));
  lua.lua_setmetatable(L, envIdx);
  if (parentIdx) {
    lua.lua_pushvalue(L, parentIdx);
    lua.lua_setfield(L, envIdx, to_luastring('super'));
  }

  const buf = to_luastring(found.source);
  if (lauxlib.luaL_loadbuffer(L, buf, buf.length, to_luastring(`=${found.canonical}`)) !== lua.LUA_OK) {
    clearLoading();
    lua.lua_error(L);
    return;
  }
  // A main chunk's first upvalue is its _ENV.
  lua.lua_pushvalue(L, envIdx);
  lua.lua_setupvalue(L, -2, 1);
  if (lua.lua_pcall(L, 0, 1, 0) !== lua.LUA_OK) {
    clearLoading();
    lua.lua_error(L);
    return;
  }
  clearLoading();

  // A module that returns nothing is `true`, the conventional sentinel.
  if (lua.lua_isnil(L, -1)) {
    lua.lua_pop(L, 1);
    lua.lua_pushboolean(L, 1);
  }
  // A child table reads through to its parent's.
  if (parentIdx && lua.lua_istable(L, -1) && lua.lua_istable(L, parentIdx)) {
    lua.lua_createtable(L, 0, 1);
    lua.lua_pushvalue(L, parentIdx);
    lua.lua_setfield(L, -2, to_luastring('__index'));
    lua.lua_setmetatable(L, -2);
  }

  lua.lua_getfield(L, lua.LUA_REGISTRYINDEX, to_luastring(REQUIRE_CACHE));
  lua.lua_pushvalue(L, -2);
  lua.lua_setfield(L, -2, to_luastring(path));
  lua.lua_pop(L, 1);

  // Leave only the module's value where the caller's stack ended.
  lua.lua_replace(L, loadingIdx);
  lua.lua_settop(L, loadingIdx);
}

function installRequire(L: any): void {
  // Per-Lua-state cache and in-progress set, in the registry.
  lua.lua_createtable(L, 0, 0);
  lua.lua_setfield(L, lua.LUA_REGISTRYINDEX, to_luastring(REQUIRE_CACHE));
  lua.lua_createtable(L, 0, 0);
  lua.lua_setfield(L, lua.LUA_REGISTRYINDEX, to_luastring(REQUIRE_LOADING));

  lua.lua_pushjsfunction(L, (Lp: any) => {
    if (lua.lua_gettop(Lp) < 1 || !lua.lua_isstring(Lp, 1)) {
      lua.lua_pushstring(Lp, to_luastring("require: expected string argument"));
      return lua.lua_error(Lp);
    }
    loadModule(Lp, lua.lua_tojsstring(Lp, 1));
    return 1;
  });
  lua.lua_setglobal(L, to_luastring('require'));
}

/**
 * Chain a script's globals to the module it `extends`, as native chains its
 * environment: a name the script does not define is looked up on the
 * parent's table (and its parents) before it is reported missing. `super` is
 * the parent. Throws if the parent cannot be loaded or is not a table.
 */
function applyExtends(L: any, label: string, parentPath: string): void {
  lua.lua_pushjsfunction(L, (Lp: any) => {
    loadModule(Lp, parentPath);
    return 1;
  });
  if (lua.lua_pcall(L, 0, 1, 0) !== lua.LUA_OK) {
    const msg = lua.lua_tojsstring(L, -1) || '<lua error>';
    lua.lua_pop(L, 1);
    throw new Error(`Lua load '${label}': extends '${parentPath}': ${msg}`);
  }
  if (!lua.lua_istable(L, -1)) {
    lua.lua_pop(L, 1);
    throw new Error(
      `Lua load '${label}': extends '${parentPath}' did not return a table; library scripts must return a table of fields/functions`,
    );
  }
  const parentIdx = lua.lua_gettop(L);
  lua.lua_pushglobaltable(L);
  lua.lua_createtable(L, 0, 1);
  lua.lua_pushvalue(L, parentIdx);
  lua.lua_setfield(L, -2, to_luastring('__index'));
  lua.lua_setmetatable(L, -2);
  lua.lua_pop(L, 1);
  lua.lua_setglobal(L, to_luastring('super'));
}

// Script errors go to whatever host installed a sink. The editor installs
// one that writes to its Console panel; a deployed web build leaves it unset
// and the messages land in the browser console. Keeping this file free of
// editor imports is what lets the same source ship in both.
type ScriptErrorSink = (
  message: string,
  scriptPath?: string,
  scriptLine?: number,
) => void;

/** A Lua error without its leading `path:line: `, which the row's link shows. */
function errorText(msg: string): string {
  return msg.replace(/^[^:\n]+\.(?:luau?|js):\d+:\s*/i, '');
}

function logError(
  message: string,
  scriptPath?: string,
  scriptLine?: number,
): void {
  const sink = (globalThis as any).__jengine_script_error as
    | ScriptErrorSink
    | undefined;
  if (typeof sink === "function") {
    sink(message, scriptPath, scriptLine);
    return;
  }
  console.error(message);
}

interface CompiledScript {
  init: ((entityId: number) => void) | null;
  update: ((entityId: number, dt: number) => void) | null;
  on_event: ((entityId: number, name: string, payload: any) => void) | null;
  on_gui_click: ((entityId: number, gid: string, nid: string) => void) | null;
  lookup: (name: string) => Function | null;
}

// ---------- the debugger's hold on script calls ----------
//
// The editor's Lua debugger (`luaDebugger.ts`) installs itself at
// `globalThis.__jengine_lua_debugger`; a web build has none, and then every
// call below is the plain `lua_pcall` it always was. While the debugger is
// armed, a callback runs as a coroutine so a breakpoint's line hook can
// suspend it, and while it is paused no script runs at all.

interface DebugHold {
  armed: boolean;
  paused: boolean;
  applyHook(L: any): void;
  suspendedAt(s: { co: any; L: any; finish: (status: number) => void }): void;
  /** The callback being called, which names a frame Lua cannot (a function
   *  started by `lua_resume` has no name of its own). */
  calling?: string;
}

function debugHold(): DebugHold | undefined {
  return (globalThis as any).__jengine_lua_debugger;
}

/** Every script state, for the debugger to hook as it arms. */
const scriptStates: { L: any; label: string }[] = [];
(globalThis as any).__jengine_lua_states = scriptStates;

/** Call the function in the registry at `ref` with `args`; `report` is told
 *  of an error's message. */
/** Each Lua state's script, for the profiler's per-script costs. Weak, so a
 *  state goes with its scene. */
const stateLabels = new WeakMap<object, string>();

/** Where the editor's Profiler collects each script's cost while it
 *  records (`profileCapture.ts`): absent, nothing is timed. */
type ScriptCosts = Map<string, { ms: number; calls: number }>;
function profileCosts(): ScriptCosts | undefined {
  return (globalThis as any).__jengine_profile_scripts as ScriptCosts | undefined;
}

function callScript(L: any, ref: number, args: any[], name: string, report: (msg: string) => void): void {
  const costs = profileCosts();
  if (!costs) {
    callScriptTimed(L, ref, args, name, report);
    return;
  }
  const t0 = performance.now();
  try {
    callScriptTimed(L, ref, args, name, report);
  } finally {
    const label = stateLabels.get(L) ?? "?";
    const c = costs.get(label) ?? { ms: 0, calls: 0 };
    c.ms += performance.now() - t0;
    c.calls += 1;
    costs.set(label, c);
  }
}

function callScriptTimed(L: any, ref: number, args: any[], name: string, report: (msg: string) => void): void {
  const dbg = debugHold();
  if (dbg?.paused) return;
  if (dbg?.armed) {
    const co = lua.lua_newthread(L);
    const coRef = lauxlib.luaL_ref(L, lua.LUA_REGISTRYINDEX);
    lua.lua_rawgeti(co, lua.LUA_REGISTRYINDEX, ref);
    for (const a of args) jsToLua(co, a);
    dbg.calling = name;
    const finish = (status: number) => {
      if (status !== lua.LUA_OK && status !== lua.LUA_YIELD) {
        report(lua.lua_tojsstring(co, -1) || '<lua error>');
      }
      lauxlib.luaL_unref(L, lua.LUA_REGISTRYINDEX, coRef);
    };
    const status = lua.lua_resume(co, L, args.length);
    if (status === lua.LUA_YIELD && debugHold()?.paused) {
      dbg.suspendedAt({ co, L, finish });
      return;
    }
    finish(status);
    return;
  }
  lua.lua_rawgeti(L, lua.LUA_REGISTRYINDEX, ref);
  for (const a of args) jsToLua(L, a);
  const status = lua.lua_pcall(L, args.length, 0, 0);
  if (status !== lua.LUA_OK) {
    const msg = lua.lua_tojsstring(L, -1) || '<lua error>';
    lua.lua_pop(L, 1);
    report(msg);
  }
}

function makeCallable(L: any, fnName: string, multi: boolean = false): Function | null {
  // Look up _G[fnName]; if missing, return null.
  lua.lua_getglobal(L, to_luastring(fnName));
  const exists = lua.lua_isfunction(L, -1);
  // Stash the function in the Lua registry so we can call it later without
  // depending on globals (scripts may overwrite them).
  if (!exists) { lua.lua_pop(L, 1); return null; }
  const ref = lauxlib.luaL_ref(L, lua.LUA_REGISTRYINDEX);
  let errorLogged = false;
  // Results were never handed back; `multi` is kept for the call sites.
  void multi;
  return (...args: any[]) => {
    if (errorLogged) return undefined;
    callScript(L, ref, args, fnName, (msg) => {
      const scriptMatch = msg.match(/^([^:\n]+\.luau?):/);
      const scriptPath = scriptMatch?.[1];
      const lineMatch = msg.match(/\.(?:luau?|js):(\d+):/i);
      const scriptLine = lineMatch ? parseInt(lineMatch[1], 10) : undefined;
      console.error(`[lua] ${fnName}: ${msg}`);
      logError(`${errorText(msg)} - in ${fnName}()`, scriptPath, scriptLine);
      errorLogged = true;
    });
    return undefined;
  };
}

function compileLua(label: string, source: string): CompiledScript {
  const lookup = getApiLookup();
  if (!lookup) {
    throw new Error(
      '__jengine_play.__api_lookup not installed (viewport not initialised yet)',
    );
  }
  const L = lauxlib.luaL_newstate();
  stateLabels.set(L, label);
  lualib.luaL_openlibs(L);
  registerBridge(L, lookup);
  // Known to the debugger, and hooked now if it is armed. Only when there is
  // one: kept with no debugger, the list would hold every state a web build
  // ever made, scene after scene.
  const dbg = debugHold();
  if (dbg) {
    scriptStates.push({ L, label });
    dbg.applyHook(L);
  }

  // Before the body runs, so the body can already call through `super`.
  const parentPath = parseExtends(source);
  if (parentPath !== null) applyExtends(L, label, parentPath);

  const chunkName = `=${label || '?'}`;
  const buf = to_luastring(source);
  const loadStatus = lauxlib.luaL_loadbuffer(
    L,
    buf,
    buf.length,
    to_luastring(chunkName),
  );
  if (loadStatus !== lua.LUA_OK) {
    const msg = lua.lua_tojsstring(L, -1) || '<lua load error>';
    throw new Error(`Lua load '${label}': ${msg}`);
  }
  const runStatus = lua.lua_pcall(L, 0, 0, 0);
  if (runStatus !== lua.LUA_OK) {
    const msg = lua.lua_tojsstring(L, -1) || '<lua run error>';
    throw new Error(`Lua run '${label}': ${msg}`);
  }

  // After the chunk runs, the script's `function on_event(...)` declaration
  // (if any) has overwritten the bridge-registered `_G.on_event` (which was
  // bound to `register_event_listener`). Capture the script's callback now,
  // then restore the global back to the registration helper so that calls
  // like `on_event("evt", ent)` inside `init` / `update` correctly register
  // listeners rather than recursively invoking the script's callback.
  const scriptOnEvent = makeCallable(L, 'on_event') as any;
  const registerListenerFn = lookup('register_event_listener');
  if (typeof registerListenerFn === 'function') {
    lua.lua_pushjsfunction(L, makeBridge(registerListenerFn, 'on_event'));
    lua.lua_setglobal(L, to_luastring('on_event'));
  }

  return {
    init: makeCallable(L, 'init') as any,
    update: makeCallable(L, 'update') as any,
    on_event: scriptOnEvent,
    on_gui_click: makeCallable(L, 'on_gui_click') as any,
    lookup: (name: string) => {
      lua.lua_getglobal(L, to_luastring(name));
      const isFn = lua.lua_isfunction(L, -1);
      if (!isFn) { lua.lua_pop(L, 1); return null; }
      const ref = lauxlib.luaL_ref(L, lua.LUA_REGISTRYINDEX);
      return (...args: any[]) => {
        callScript(L, ref, args, name, (msg) => {
          const scriptMatch = msg.match(/^([^:\n]+\.luau?):/);
          const scriptPath = scriptMatch?.[1];
          const lineMatch = msg.match(/\.(?:luau?|js):(\d+):/i);
          const scriptLine = lineMatch ? parseInt(lineMatch[1], 10) : undefined;
          console.error(`[lua] ${name}: ${msg}`);
          logError(`${errorText(msg)} - in ${name}()`, scriptPath, scriptLine);
        });
      };
    },
  };
}

// Expose the compile entry point on globalThis so the wasm-side
// `JsAdapter::load` can dispatch to it via Reflect::get.
(globalThis as any).__jengine_lua_compile = compileLua;

// Mark that the runtime is loaded so callers can introspect.
(globalThis as any).__jengine_lua_runtime = 'fengari';

export { compileLua };
