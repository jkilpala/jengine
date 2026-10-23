import { invoke } from "@tauri-apps/api/core";

import type { AnimationClipData } from "./animation";
import type { EmitShape, ParticleBlendMode, ParticleCollision, SubEmitter, Turbulence } from "./particle";

export type PathCommand =
  | { type: "MoveTo"; x: number; y: number }
  | { type: "LineTo"; x: number; y: number }
  | { type: "QuadTo"; cx: number; cy: number; x: number; y: number }
  | { type: "CubicTo"; c1x: number; c1y: number; c2x: number; c2y: number; x: number; y: number }
  | { type: "Close" };

export interface ComponentBundle {
  transform?: {
    position: [number, number];
    rotation: number;
    scale: [number, number];
  };
  sprite?: {
    texture_id: string;
    color: [number, number, number, number];
    z_order: number;
    size?: [number, number];
    uv_rect?: [number, number, number, number];
    /** Region of the atlas registered under `texture_id`. The engine fills
     *  `uv_rect` in from it at load, so a re-packed sheet needs no edits. */
    region?: string;
    /** Pivot as a fraction of `size`; [0.5, 0.5] (center) when omitted. */
    anchor?: [number, number];
    /** Optional named sort layer id (declared in the scene's `.sortlayers` asset). */
    sort_layer?: string;
    /** Sort against the other sprites in this z band by position: lower on
     *  screen draws in front. For scenes where y stands in for depth. */
    y_sort?: boolean;
  };
  script?: {
    source_path: string;
    language: string;
  };
  text?: {
    text: string;
    font_id: string;
    font_scale: number;
    color: [number, number, number, number];
    z_order: number;
    /** Optional named sort layer id (declared in the scene's `.sortlayers` asset). */
    sort_layer?: string;
    /** Read `text` as markup (`renderer::rich_text`). */
    rich?: boolean;
    /** Typewriter: characters a second; absent or 0 shows it all. */
    reveal_speed?: number;
    /** An outline round the glyphs: width in world units, linear colour. */
    outline?: { width: number; color: [number, number, number, number] };
    /** A shadow under the text: offset (y up), colour, softness. */
    shadow?: { offset: [number, number]; color: [number, number, number, number]; softness: number };
  };
  camera_follow?: {
    smoothing: number;
    offset: [number, number];
    bounds?: [number, number, number, number];
  };
  camera?: {
    zoom: number;
    is_active: boolean;
    viewport_width: number;
    viewport_height: number;
    /** World units per art pixel for a pixel-perfect camera; 0 or absent draws at the screen's resolution. */
    pixel_size?: number;
    /** With `pixel_size`: whole screen pixels per art pixel (`Camera2D::integer_view`). */
    integer_scale?: boolean;
    /** Draw into a texture of this name instead of the screen (a view); empty or absent is the main camera. */
    target?: string;
    /** The target texture's size in pixels; 0 takes the screen's. */
    target_size?: [number, number];
    /** The render layers this camera draws, a bitmask against each entity's `render_layers`; all when absent. */
    cull_mask?: number;
    /** Draw onto this part of the screen - x, y, w, h as fractions from the top-left (split screen); absent or all 0 is the whole screen. */
    screen_rect?: [number, number, number, number];
    /** Which active camera draws the screen: the highest priority, the first on a tie; absent is 0. */
    priority?: number;
    /** Seconds the view eases over to this camera when it takes the screen; absent or 0 cuts. */
    blend?: number;
    /** A polygon in world units this camera's whole view stays inside; absent is none. */
    confine?: [number, number][];
  };
  tags?: string[];

  // -- Phase 4B: Physics --
  rigid_body?: {
    body_type: "Dynamic" | "Static" | "Kinematic";
    mass: number;
    gravity_scale: number;
    linear_damping: number;
    angular_damping: number;
    fixed_rotation: boolean;
    /** Sweep the shape along its path (continuous collision).
     *  Off by default; for anything small and fast. */
    ccd?: boolean;
  };
  collider?: {
    shape: { Box: { width: number; height: number } }
      | { Circle: { radius: number } }
      | { Capsule: { half_height: number; radius: number } }
      | { Segment: { a: [number, number]; b: [number, number] } }
      | { Triangle: { a: [number, number]; b: [number, number]; c: [number, number] } }
      | { ConvexPolygon: { vertices: [number, number][] } }
      | { Polyline: { vertices: [number, number][] } }
      | { HeightField: { heights: number[]; scale: [number, number] } }
      | { ConvexDecomposition: { vertices: [number, number][]; loops?: number[] } };
    offset: [number, number];
    is_sensor: boolean;
    friction: number;
    restitution: number;
    density: number;
    collision_membership?: number;
    collision_filter?: number;
  };

  joint?: {
    target_entity: string;
    joint_type:
      | { Revolute: { anchor: [number, number]; limits?: [number, number]; motor_speed: number; motor_max_torque: number } }
      | { Fixed: { anchor_a: [number, number]; anchor_b: [number, number] } }
      | { Prismatic: { anchor: [number, number]; axis: [number, number]; limits?: [number, number]; motor_speed: number; motor_max_force: number } }
      | { Rope: { anchor_a: [number, number]; anchor_b: [number, number]; max_distance: number } }
      | { Spring: { anchor_a: [number, number]; anchor_b: [number, number]; rest_length: number; stiffness: number; damping: number } }
      | { Wheel: { anchor?: [number, number]; anchor_b?: [number, number]; axis?: [number, number]; frequency?: number; damping_ratio?: number; limits?: [number, number] | null; motor_speed?: number; motor_max_torque?: number } }
      | { Motor: { linear_offset?: [number, number]; angular_offset?: number; max_force?: number; max_torque?: number; frequency?: number; damping_ratio?: number } }
      | { Friction: { anchor_a?: [number, number]; anchor_b?: [number, number]; max_force?: number; max_torque?: number } }
      | { Target: { anchor?: [number, number]; target?: [number, number]; frequency?: number; damping_ratio?: number; max_force?: number } };
    collide_connected: boolean;
  };

  // -- Phase 4A: Audio --
  audio_source?: {
    clip_id: string;
    volume: number;
    pitch: number;
    looping: boolean;
    auto_play: boolean;
    /** Heard from where it is: louder near the listener, panned to its side. */
    spatial?: boolean;
    min_distance?: number;
    max_distance?: number;
    pan_width?: number;
  };

  // -- Phase 4C: Animation --
  sprite_animator?: {
    // One definition of a clip, in `api/animation.ts`. This was a second,
    // slightly different copy: its frames required a `uv_rect` and typed
    // triggers as `unknown[]`.
    clips?: Record<string, AnimationClipData>;
    current_clip?: string;
    current_frame: number;
    elapsed: number;
    playing: boolean;
    speed: number;
    random_start?: boolean;
    /** Path (relative to assets/) to a .anim file whose clips should
     * be inlined at scene-load time. The editor must resolve this
     * before passing the scene to the viewport WASM. */
    clip_source?: string;
  };

  anim_state_machine?: {
    states: Record<string, {
      clip_name: string;
      transitions: {
        target: string;
        conditions: (
          | { type: 'BoolEquals'; param: string; value: boolean }
          | { type: 'FloatGreaterThan'; param: string; threshold: number }
          | { type: 'FloatLessThan'; param: string; threshold: number }
          | { type: 'ClipFinished' }
          | { type: 'TriggerSet'; param: string }
        )[];
      }[];
    }>;
    current_state?: string;
    parameters?: Record<string, { type: 'Bool'; value: boolean } | { type: 'Float'; value: number } | { type: 'Trigger' }>;
  };

  // -- Phase 4D: Tilemap --
  tilemap?: {
    tile_size: [number, number];
    /** Square cells (the default), diamonds or hexagons. */
    orientation?: 'orthogonal' | 'isometric' | 'hexagonal';
    /** A hexagon's flat-side length (hexagonal maps). */
    hex_side?: number;
    /** 'y' (pointy-topped, rows pushed - the default) or 'x' (flat-topped, columns pushed). */
    stagger_axis?: 'x' | 'y';
    /** Which rows or columns are pushed: 'odd' (the default) or 'even'. */
    stagger_index?: 'odd' | 'even';
    /** Optional path to a .tilemap asset file (editor only). */
    source?: string;
    layers: {
      name: string;
      tiles: (number | null)[];
      width: number;
      height: number;
      visible: boolean;
      y_sort?: boolean;
      offset?: [number, number];
    }[];
    tilesets: {
      texture_id: string;
      tile_width: number;
      tile_height: number;
      columns: number;
      tile_count: number;
      first_gid: number;
      image_width: number;
      image_height: number;
      animations?: { tile: number; frames: { tile: number; duration: number }[]; scatter?: boolean }[];
      shapes?: { tile: number; collision?: [number, number][]; occluder?: [number, number][] }[];
    }[];
    z_order?: number;
    /** Optional named sort layer id (declared in the scene's `.sortlayers` asset). */
    sort_layer?: string;
  };

  // -- Phase 4E: UI --
  ui_element?: {
    anchor: "TopLeft" | "TopCenter" | "TopRight" | "CenterLeft" | "Center" | "CenterRight" | "BottomLeft" | "BottomCenter" | "BottomRight";
    offset: [number, number];
    size: [number, number];
    visible: boolean;
    z_order: number;
  };
  ui_text?: {
    text: string;
    font_id: string;
    font_size: number;
    color: [number, number, number, number];
    alignment: "Left" | "Center" | "Right";
    rich?: boolean;
    reveal_speed?: number;
  };
  ui_image?: {
    texture_id: string;
    color: [number, number, number, number];
    uv_rect?: [number, number, number, number];
  };
  ui_panel?: {
    bg_color: [number, number, number, number];
  };
  ui_button?: {
    normal_color: [number, number, number, number];
    hover_color: [number, number, number, number];
    pressed_color: [number, number, number, number];
    disabled: boolean;
  };
  ui_progress_bar?: {
    value: number;
    min: number;
    max: number;
    fg_color: [number, number, number, number];
    bg_color: [number, number, number, number];
    border_color: [number, number, number, number];
  };
  ui_slider?: {
    value: number;
    min: number;
    max: number;
    track_color: [number, number, number, number];
    thumb_color: [number, number, number, number];
    thumb_size: number;
  };
  ui_toggle?: {
    checked: boolean;
    on_color: [number, number, number, number];
    off_color: [number, number, number, number];
    thumb_color: [number, number, number, number];
  };
  ui_text_input?: {
    text: string;
    placeholder: string;
    font_id: string;
    font_size: number;
    color: [number, number, number, number];
    bg_color: [number, number, number, number];
  };
  gui?: {
    gui_id: string;
    visible: boolean;
    variables: Record<string, unknown>;
  };

  // -- Materials --
  material?: {
    material_id: string;
    blend_mode?: string;
    uniforms: Record<string, unknown>;
  };

  // -- Phase 4F: Particles --
  particle_emitter?: {
    texture_id?: string;
    emission_rate: number;
    max_particles: number;
    lifetime: [number, number];
    speed: [number, number];
    angle: [number, number];
    color_start: [number, number, number, number];
    color_end: [number, number, number, number];
    size_start: [number, number];
    size_end: [number, number];
    gravity_modifier: number;
    angular_velocity: [number, number];
    burst_count: number;
    active: boolean;
    world_space: boolean;
    z_order: number;
    /** Optional named sort layer id (declared in the scene's `.sortlayers` asset). */
    sort_layer?: string;
    /** How particles blend with what is under them. */
    blend_mode?: ParticleBlendMode;
      /** Where on the emitter a particle starts. */
    emit_shape?: EmitShape;
      /** `[t, r, g, b, a]` keys: colour over life. */
    color_curve?: number[][];
      /** `[t, multiplier]` keys: size over life. */
    size_curve?: number[][];
      /** Stretch along velocity, seconds of travel. */
    stretch?: number;
      /** Air resistance per second. */
    drag?: number;
      /** A swirling flow the particles drift along. */
    turbulence?: Turbulence | null;
    collision?: ParticleCollision | null;
    sub_emitters?: SubEmitter[];
  };

  // -- Lighting --
  point_light?: {
    color: [number, number, number];
    intensity: number;
    radius: number;
    falloff: number;
    /** A texture the light shines through (`PointLight2D::cookie`). */
    cookie?: string;
    cookie_scale?: number;
    /** The light layers it lights (Rust `light_mask`); absent is every layer. */
    light_mask?: number;
    /** Takes light away instead of adding it (Rust `subtract`). */
    subtract?: boolean;
  };
  spot_light?: {
    color: [number, number, number];
    intensity: number;
    radius: number;
    angle: number;
    direction: number;
    falloff: number;
    /** A texture the light shines through (`PointLight2D::cookie`). */
    cookie?: string;
    cookie_scale?: number;
    /** The light layers it lights (Rust `light_mask`); absent is every layer. */
    light_mask?: number;
    /** Takes light away instead of adding it (Rust `subtract`). */
    subtract?: boolean;
  };
  ambient_light?: {
    color: [number, number, number];
    intensity: number;
  };
  lightmap?: {
    texture_id: string;
    uv_rect: [number, number, number, number];
    intensity: number;
  };
  shadow_caster?: {
    enabled: boolean;
    color: [number, number, number, number];
    offset: [number, number];
    height: number;
    scale: [number, number];
    skew: number;
  };
  vector_path?: {
    commands: PathCommand[];
    fill_color: [number, number, number, number] | null;
    stroke_color: [number, number, number, number] | null;
    stroke_width: number;
    z_order: number;
    closed: boolean;
    /** Optional named sort layer id (declared in the scene's `.sortlayers` asset). */
    sort_layer?: string;
  };

  // -- Step 2D: native data-driven behaviors --
  acceleration?: { x: number; y: number };
  max_speed?: { value: number };
  oscillator?: {
    amplitude: [number, number];
    frequency: [number, number];
    phase: [number, number];
    elapsed: number;
    center?: [number, number] | null;
  };
  follow_target?: {
    target: { kind: string; value?: string };
    offset: [number, number];
    max_speed: number;
    stop_distance: number;
  };
  seek_target?: {
    target: { kind: string; value?: string };
    accel: number;
    max_speed: number;
  };
  tween?: {
    target: "PositionX" | "PositionY" | "Rotation" | "ScaleX" | "ScaleY" | "Opacity";
    from: number;
    to: number;
    duration: number;
    elapsed: number;
    ease: "Linear" | "EaseInQuad" | "EaseOutQuad" | "EaseInOutQuad";
    on_complete: "Stop" | "Loop" | "PingPong";
  };
  patrol_path?: {
    path_id: string;
    speed: number;
    index: number;
    mode: "Loop" | "PingPong" | "Once";
    forward: boolean;
  };

  /**
   * Step 7 / Slice 7.6: declarative behavior graph attached to this entity.
   *
   * - `graph_path` is relative to the project's `assets/` root (e.g.
   *   `"behaviors/player-mover.behavior"`) and points at a `.behavior`
   *   asset.
   * - `overrides` is a free-form bag of pin-default overrides keyed by
   *   parameter name. Values must be JSON-serializable so they round-trip
   *   through `serde_json::Value` on the Rust side. Edited as raw JSON in
   *   the Inspector for v1 — the node-graph editor (separate slice plan)
   *   will replace this with typed pin editors.
   */
  behavior_graph?: {
    graph_path: string;
    overrides?: Record<string, unknown>;
  };

  /** Custom components keyed by component name. */
  [key: string]: unknown;
}

export interface EntityData {
  name: string;
  parent?: string | null;
  components: ComponentBundle;
  /** A prefab's parts, each in its parent's frame (`EntityData::children`);
   *  a scene flattens them into entities when it loads. */
  children?: EntityData[];
}

export interface PostProcessEffectSettings {
  name: string;
  enabled: boolean;
  params: Record<string, unknown>;
}

export interface PostProcessSettings {
  /** Let light and colour exceed 1.0 until the output pass tonemaps them.
   *  Opt-in: an LDR scene clamps light at 1.0, as it always has. */
  hdr?: boolean;
  /** `aces` (the default for an HDR scene), `agx`, `reinhard` or `none`. */
  tonemap?: "aces" | "agx" | "reinhard" | "none";
  /** Multiplies the scene before the tonemap; HDR only. */
  exposure?: number;
  effects: PostProcessEffectSettings[];
}

export interface SceneData {
  /** Scene format version. **Must be preserved on round-trip**: the editor
   *  loads a scene, re-serializes it and pushes that to the viewport, and a
   *  dropped version made every v2 scene arrive looking like v1 - which
   *  silently demotes package components to `CustomComponents` and stops the
   *  package that owns them from seeing anything. */
  version?: number;
  name: string;
  entities: EntityData[];
  post_process?: PostProcessSettings;
  /** Project-relative path to a `.sortlayers` asset (e.g. `"sortlayers/main.sortlayers"`).
   * Resolved by the editor before scene load and pushed into the viewport WASM. */
  sort_layers?: string;
  // Any other top-level key (`audio_refs`, `atlas_refs`, `paths`, ...) is
  // carried through untouched: the Rust `SceneData` keeps them in a flattened
  // map, so they reach the viewport for Play and survive a save (BUGS #157).
  // Clone scenes with structuredClone / spread, never by listing fields.
}

export function createNewScene(): Promise<SceneData> {
  return invoke<SceneData>("create_new_scene");
}

export function loadScene(path: string): Promise<SceneData> {
  return invoke<SceneData>("load_scene", { path });
}

export function saveScene(path: string, scene: SceneData): Promise<void> {
  return invoke("save_scene", { path, scene });
}

export function addEntity(scene: SceneData, name: string): Promise<SceneData> {
  return invoke<SceneData>("add_entity", { scene, name });
}

export function removeEntity(scene: SceneData, index: number): Promise<SceneData> {
  return invoke<SceneData>("remove_entity", { scene, index });
}

export function updateEntityName(scene: SceneData, index: number, name: string): Promise<SceneData> {
  return invoke<SceneData>("update_entity_name", { scene, index, name });
}

export interface ScriptSourceEntry {
  entity_index: number;
  entity_name: string;
  source: string;
  source_path: string;
}

export interface ScriptLoadResult {
  entries: ScriptSourceEntry[];
  warnings: string[];
}

export function loadScriptSources(projectPath: string, scene: SceneData): Promise<ScriptLoadResult> {
  return invoke<ScriptLoadResult>("load_script_sources", { projectPath, scene });
}
