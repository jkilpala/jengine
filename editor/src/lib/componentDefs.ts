// ---------------------------------------------------------------------------
// Shared component metadata — used by both Inspector and PrefabInspector
// ---------------------------------------------------------------------------

/** Display-order list of every built-in component key. */
export const KNOWN_COMPONENTS: readonly string[] = [
  "transform",
  "sprite",
  "material",
  "script",
  "tags",
  "text",
  "camera_follow",
  "camera",
  "rigid_body",
  "collider",
  "joint",
  "velocity",
  "bounce_area",
  "audio_source",
  "sprite_animator",
  "anim_state_machine",
  "tilemap",
  "particle_emitter",
  "ui_element",
  "ui_text",
  "ui_image",
  "ui_panel",
  "ui_button",
  "ui_progress_bar",
  "ui_slider",
  "ui_toggle",
  "ui_text_input",
  "gui",
  "point_light",
  "spot_light",
  "ambient_light",
  "lightmap",
  "shadow_caster",
  "vector_path",
  // ── Step 2D behaviors (native data-driven motion) ──
  "acceleration",
  "max_speed",
  "oscillator",
  "follow_target",
  "seek_target",
  "tween",
  "patrol_path",
  // ── Step 7 behavior graph (declarative scripting) ──
  "behavior_graph",
];

/** Set of built-in component keys (for distinguishing custom components). */
export const BUILTIN_KEYS = new Set(KNOWN_COMPONENTS);

/** Default data for every built-in component type. */
export const COMPONENT_DEFAULTS: Record<string, unknown> = {
  transform: { position: [0, 0], rotation: 0, scale: [1, 1] },
  sprite: { texture_id: "", color: [1, 1, 1, 1], z_order: 0 },
  script: { source_path: "", language: "Lua" },
  tags: [],
  text: {
    text: "",
    font_id: "",
    font_scale: 1,
    color: [1, 1, 1, 1],
    z_order: 0,
  },
  camera_follow: { smoothing: 0.1, offset: [0, 0], frame_padding: 140 },
  camera: {
    zoom: 1,
    is_active: true,
    viewport_width: 1280,
    viewport_height: 720,
  },
  rigid_body: {
    body_type: "Dynamic",
    mass: 1,
    gravity_scale: 1,
    linear_damping: 0,
    angular_damping: 0,
    fixed_rotation: false,
    ccd: false,
  },
  collider: {
    shape: { Box: { width: 1, height: 1 } },
    offset: [0, 0],
    is_sensor: false,
    friction: 0.5,
    restitution: 0,
    density: 1,
    collision_membership: 0xffff,
    collision_filter: 0xffff,
  },
  joint: {
    target_entity: "",
    joint_type: {
      Revolute: { anchor: [0, 0], anchor_b: [0, 0], motor_speed: 0, motor_max_torque: 0 },
    },
    collide_connected: false,
  },
  velocity: { x: 0, y: 0, gravity: 0 },
  bounce_area: {
    min_x: -640,
    min_y: -360,
    max_x: 640,
    max_y: 360,
    damping: 0.9,
  },
  audio_source: {
    clip_id: "",
    volume: 1,
    pitch: 1,
    looping: false,
    auto_play: false,
  },
  sprite_animator: {
    clips: {},
    current_frame: 0,
    elapsed: 0,
    playing: false,
    speed: 1,
    random_start: false,
  },
  anim_state_machine: {
    states: {},
    current_state: undefined,
    parameters: {},
  },
  tilemap: {
    tile_size: [16, 16],
    layers: [],
    tilesets: [],
  },
  particle_emitter: {
    emission_rate: 20,
    max_particles: 256,
    lifetime: [0.5, 1.5],
    speed: [50, 100],
    angle: [0, 6.2832],
    color_start: [1, 1, 1, 1],
    color_end: [1, 1, 1, 0],
    size_start: [8, 12],
    size_end: [2, 4],
    gravity_modifier: 0,
    angular_velocity: [0, 0],
    burst_count: 0,
    active: true,
    world_space: true,
    z_order: 0,
  },
  ui_element: {
    anchor: "TopLeft",
    offset: [0, 0],
    size: [100, 30],
    visible: true,
    z_order: 0,
  },
  ui_text: {
    text: "Text",
    font_id: "",
    font_size: 1,
    color: [1, 1, 1, 1],
    alignment: "Left",
  },
  ui_image: { texture_id: "", color: [1, 1, 1, 1] },
  ui_panel: { bg_color: [0.2, 0.2, 0.2, 0.8] },
  ui_button: {
    normal_color: [0.3, 0.3, 0.3, 1],
    hover_color: [0.4, 0.4, 0.4, 1],
    pressed_color: [0.2, 0.2, 0.2, 1],
    disabled: false,
  },
  ui_progress_bar: {
    value: 0.5,
    min: 0,
    max: 1,
    fg_color: [0.2, 0.8, 0.3, 1],
    bg_color: [0.2, 0.2, 0.2, 1],
    border_color: [0.4, 0.4, 0.4, 1],
  },
  ui_slider: {
    value: 0.5,
    min: 0,
    max: 1,
    track_color: [0.3, 0.3, 0.3, 1],
    thumb_color: [0.8, 0.8, 0.8, 1],
    thumb_size: 12,
  },
  ui_toggle: {
    checked: false,
    on_color: [0.2, 0.8, 0.3, 1],
    off_color: [0.4, 0.4, 0.4, 1],
    thumb_color: [1, 1, 1, 1],
  },
  ui_text_input: {
    text: "",
    placeholder: "Enter text...",
    font_id: "",
    font_size: 1,
    color: [1, 1, 1, 1],
    bg_color: [0.15, 0.15, 0.2, 1],
  },
  material: { material_id: "default_sprite", uniforms: {} },
  gui: { gui_id: "", visible: true, variables: {} },
  point_light: {
    color: [1, 1, 1],
    intensity: 1,
    radius: 200,
    falloff: 2,
  },
  spot_light: {
    color: [1, 1, 1],
    intensity: 1,
    radius: 300,
    angle: 0.785,
    direction: 0,
    falloff: 2,
  },
  ambient_light: {
    color: [1, 1, 1],
    intensity: 0.3,
  },
  lightmap: {
    texture_id: "",
    uv_rect: [0, 0, 1, 1],
    intensity: 1,
  },
  shadow_caster: {
    enabled: true,
    color: [0, 0, 0, 0.4],
    offset: [2, -2],
    height: 1,
    scale: [1, 1],
    skew: 0,
  },
  vector_path: {
    commands: [],
    fill_color: [1, 1, 1, 1],
    stroke_color: null,
    stroke_width: 1,
    z_order: 0,
    closed: false,
  },
  // ── Step 2D behaviors ──
  acceleration: { x: 0, y: 0 },
  max_speed: { value: 1000 },
  oscillator: {
    amplitude: [32, 0],
    frequency: [1, 0],
    phase: [0, 0],
    elapsed: 0,
    center: null,
  },
  follow_target: {
    target: { kind: "SelfEntity" },
    offset: [0, 0],
    max_speed: 200,
    stop_distance: 4,
  },
  seek_target: {
    target: { kind: "SelfEntity" },
    accel: 400,
    max_speed: 300,
  },
  tween: {
    target: "Opacity",
    from: 0,
    to: 1,
    duration: 1,
    elapsed: 0,
    ease: "Linear",
    on_complete: "Stop",
  },
  patrol_path: {
    path_id: "",
    speed: 100,
    index: 0,
    mode: "Loop",
    forward: true,
  },
  // ── Step 7 behavior graph (declarative scripting) ──
  // `overrides` is a free-form bag keyed by parameter name; it round-
  // trips through `serde_json::Value` on the Rust side. The Inspector
  // edits it as raw JSON for v1.
  behavior_graph: { graph_path: "", overrides: {} },
};

/** Component dependency map — when adding A, ensure these exist first. */
export const COMPONENT_DEPS: Record<string, string[]> = {
  sprite: ["transform"],
  material: ["sprite"],
  text: ["transform"],
  script: ["transform"],
  camera: ["transform"],
  camera_follow: ["transform"],
  rigid_body: ["transform"],
  collider: ["transform", "rigid_body"],
  joint: ["transform", "rigid_body"],
  velocity: ["transform"],
  bounce_area: ["transform"],
  audio_source: ["transform"],
  sprite_animator: ["transform", "sprite"],
  anim_state_machine: ["transform", "sprite", "sprite_animator"],
  tilemap: ["transform"],
  particle_emitter: ["transform"],
  gui: ["transform"],
  point_light: ["transform"],
  spot_light: ["transform"],
  ambient_light: ["transform"],
  shadow_caster: ["transform", "sprite"],
  vector_path: ["transform"],
  ui_text: ["ui_element"],
  ui_image: ["ui_element"],
  ui_panel: ["ui_element"],
  ui_button: ["ui_element"],
  ui_progress_bar: ["ui_element"],
  ui_slider: ["ui_element"],
  ui_toggle: ["ui_element"],
  ui_text_input: ["ui_element"],
  // ── Step 2D behaviors ──
  acceleration: ["transform", "velocity"],
  max_speed: ["transform", "velocity"],
  oscillator: ["transform"],
  follow_target: ["transform"],
  seek_target: ["transform", "velocity"],
  tween: ["transform"],
  patrol_path: ["transform"],
};

export interface ComponentCategory {
  label: string;
  icon: string;
  items: string[];
}

/** Grouped categories for the "+ Add Component" dropdown. */
export const COMPONENT_CATEGORIES: Record<string, ComponentCategory> = {
  core: { label: "Core", icon: "⚙", items: ["transform", "script"] },
  visuals: {
    label: "Visuals",
    icon: "🖌",
    items: [
      "sprite",
      "material",
      "text",
      "sprite_animator",
      "anim_state_machine",
      "tilemap",
      "particle_emitter",
      "vector_path",
    ],
  },
  physics: {
    label: "Physics",
    icon: "🔧",
    items: ["rigid_body", "collider", "joint", "velocity", "bounce_area"],
  },
  audio: { label: "Audio", icon: "🔊", items: ["audio_source"] },
  camera: { label: "Camera", icon: "📷", items: ["camera_follow", "camera"] },
  ui: {
    label: "UI",
    icon: "🖼",
    items: [
      "ui_element",
      "ui_text",
      "ui_image",
      "ui_panel",
      "ui_button",
      "ui_progress_bar",
      "ui_slider",
      "ui_toggle",
      "ui_text_input",
      "gui",
    ],
  },
  lighting: {
    label: "Lighting",
    icon: "💡",
    items: [
      "point_light",
      "spot_light",
      "ambient_light",
      "lightmap",
      "shadow_caster",
    ],
  },
  behaviors: {
    label: "Behaviors",
    icon: "🎯",
    items: [
      "acceleration",
      "max_speed",
      "oscillator",
      "follow_target",
      "seek_target",
      "tween",
      "patrol_path",
      "behavior_graph",
    ],
  },
};

/** Format a component key for display: "sprite_animator" → "Sprite Animator" */
export function formatComponentName(key: string): string {
  return key
    .replace(/_/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase());
}

/**
 * Given a set of addable component keys, return categorized groups
 * (filtering out components already present).
 */
export function categorizeComponents(
  addable: string[],
): Array<{ key: string; label: string; icon: string; items: string[] }> {
  const addableSet = new Set(addable);
  const categories: Array<{
    key: string;
    label: string;
    icon: string;
    items: string[];
  }> = [];

  for (const [key, cat] of Object.entries(COMPONENT_CATEGORIES)) {
    const filtered = cat.items.filter((c) => addableSet.has(c));
    if (filtered.length > 0) {
      categories.push({
        key,
        label: cat.label,
        icon: cat.icon,
        items: filtered,
      });
    }
  }

  // Custom/unknown components as their own category (always present so the
  // "New Custom Component" shortcut is always reachable from the add menu).
  const allCategorized = new Set(
    Object.values(COMPONENT_CATEGORIES).flatMap((c) => c.items),
  );
  const customItems = addable.filter((c) => !allCategorized.has(c));
  categories.push({
    key: "custom",
    label: "Custom",
    icon: "📦",
    items: customItems,
  });

  return categories;
}

/**
 * Fields of a component with a hand-written Inspector section that the
 * section does not draw. The Inspector draws these under the section with
 * the widgets derived from the component (`undrawnFields` in
 * `Inspector.svelte`, through `PackageInspector`). Giving one a hand-drawn
 * row means taking it off this list - `hand_written_sections_draw_every_field`
 * fails a field that is drawn twice, or by neither.
 */
export const UNDRAWN_FIELDS: Record<string, readonly string[]> = {
  sprite: ["corners"],
  script: ["cull_distance"],
  text: ["wrap_width", "align", "text_key"],
  camera_follow: ["bounds", "frame_tag", "frame_padding", "zoom_range", "dead_zone", "look_ahead", "look_ahead_max", "look_ahead_smoothing"],
  camera: ["stretch", "confine"],
  collider: ["one_way"],
  joint: ["break_force", "break_torque"],
  audio_source: ["bus_id", "occlusion", "muffle"],
  sprite_animator: ["clip_source"],
  particle_emitter: ["particle_source", "angle", "angular_velocity", "color_choices"],
  ui_element: ["focusable"],
  ui_text: ["outline", "shadow", "text_key"],
  ui_image: ["uv_rect", "region", "nine_slice"],
  ui_progress_bar: ["border_color"],
  ui_toggle: ["thumb_color"],
  ui_text_input: ["max_length", "password"],
  point_light: ["shadow_softness", "height", "casts_shadows"],
  spot_light: ["shadow_softness", "height", "casts_shadows"],
  lightmap: ["uv_rect", "size"],
  oscillator: ["center"],
  seek_target: ["slowing_radius"],
};

/** `UNDRAWN_FIELDS` for the prefab inspector's own few sections
 *  (`prefab_sections_draw_every_field`). */
export const PREFAB_UNDRAWN_FIELDS: Record<string, readonly string[]> = {
  sprite: ["uv_rect", "anchor", "fill", "sort_layer", "y_sort", "corners"],
  script: ["cull_distance"],
  collider: ["shape", "collision_membership", "collision_filter", "one_way"],
  sprite_animator: ["clip_source"],
};

/**
 * Fields no Inspector row draws: state the engine keeps while playing, or a
 * field its section sets some way the coverage test cannot read.
 */
export const NOT_DRAWN_FIELDS: Record<string, readonly string[]> = {
  // Set by the section's atlas-region picker, with `uv_rect`.
  sprite: ["region"],
  gui: ["focused_node_id"],
  sprite_animator: ["current_frame", "elapsed"],
  oscillator: ["elapsed"],
  tween: ["elapsed"],
  patrol_path: ["index", "forward"],
};
