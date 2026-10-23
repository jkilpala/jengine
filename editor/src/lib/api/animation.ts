import { invoke } from "@tauri-apps/api/core";
import { writeTextFile } from "@tauri-apps/plugin-fs";

/** A trigger that fires when a specific animation frame is reached. */
export type FrameTriggerData =
  | { type: "PlaySound"; clip_id: string; volume?: number }
  | { type: "EmitEvent"; event_name: string; data?: unknown };

/** A single frame in a sprite animation. Matches Rust `AnimationFrame`. */
export interface AnimationFrameData {
  /** UV rectangle: [u_min, v_min, u_max, v_max]. Optional: a frame that names
   *  a `region` has it filled in from the atlas at load time. */
  uv_rect?: [number, number, number, number];
  /** Region of the atlas registered under this frame's `texture_id`, or the
   *  sprite's texture. Resolved by the engine, so re-packing a sheet needs no
   *  edits here. */
  region?: string;
  /** Optional per-frame texture path (relative to assets/). */
  texture_id?: string | null;
  /** Triggers that fire when this frame is reached. */
  triggers?: FrameTriggerData[];
}

/** Animatable property names. Matches Rust `AnimatableProperty`. */
export type AnimatableProperty =
  | "PositionX"
  | "PositionY"
  | "Rotation"
  | "ScaleX"
  | "ScaleY"
  | "ColorR"
  | "ColorG"
  | "ColorB"
  | "ColorA"
  // UI Element
  | "UiOffsetX"
  | "UiOffsetY"
  | "UiSizeX"
  | "UiSizeY"
  // UI Text
  | "UiTextFontSize"
  | "UiTextColorR"
  | "UiTextColorG"
  | "UiTextColorB"
  | "UiTextColorA"
  // UI Image
  | "UiImageColorR"
  | "UiImageColorG"
  | "UiImageColorB"
  | "UiImageColorA"
  // UI Panel
  | "UiPanelColorR"
  | "UiPanelColorG"
  | "UiPanelColorB"
  | "UiPanelColorA"
  // UI Button
  | "UiButtonColorR"
  | "UiButtonColorG"
  | "UiButtonColorB"
  | "UiButtonColorA"
  // UI Progress Bar
  | "UiProgressValue"
  | "UiProgressFgR"
  | "UiProgressFgG"
  | "UiProgressFgB"
  | "UiProgressFgA"
  // UI Slider
  | "UiSliderValue";

/** A single keyframe — value at a specific frame index. */
export interface KeyframeData {
  frame: number;
  value: number;
}

/** A property animation track — keyframes for one property. */
export interface PropertyTrackData {
  property: AnimatableProperty;
  keyframes: KeyframeData[];
}

/** An animation clip. Matches Rust `AnimationClip`. */
export interface AnimationClipData {
  name: string;
  frames: AnimationFrameData[];
  /** Seconds per frame. */
  frame_duration: number;
  looping: boolean;
  /** Property animation tracks (keyframe-driven). */
  property_tracks?: PropertyTrackData[];
}

/** Default animation clip with a single full-texture frame. */
export function defaultAnimationClip(name = "idle"): AnimationClipData {
  return {
    name,
    frame_duration: 0.1,
    looping: true,
    frames: [{ uv_rect: [0, 0, 1, 1] }],
  };
}

/**
 * Multi-clip animation file format.
 * Contains multiple named clips and an optional shared texture.
 */
export interface MultiClipAnimData {
  clips: Record<string, AnimationClipData>;
  /** Shared texture for all clips (relative to assets/). */
  texture_id?: string;
}

/** Default multi-clip animation data. */
export function defaultMultiClipAnim(): MultiClipAnimData {
  return {
    clips: {
      idle: defaultAnimationClip("idle"),
    },
    texture_id: undefined,
  };
}

/**
 * Detect whether raw JSON is single-clip (has "name" at root)
 * or multi-clip (has "clips" at root).
 */
function isMultiClip(raw: Record<string, unknown>): boolean {
  return "clips" in raw && typeof raw.clips === "object" && !("name" in raw);
}

/** Convert a single-clip to multi-clip format. */
export function singleToMulti(clip: AnimationClipData): MultiClipAnimData {
  return {
    clips: { [clip.name]: clip },
    texture_id: undefined,
  };
}

/** Read an `.anim` file — returns multi-clip format (auto-converts single-clip). */
export async function readAnimFile(
  projectPath: string,
  relativePath: string,
): Promise<MultiClipAnimData> {
  const bytes = await invoke<number[]>("read_asset_binary", {
    projectPath,
    relativePath,
  });
  const text = new TextDecoder().decode(new Uint8Array(bytes));
  const raw = JSON.parse(text) as Record<string, unknown>;
  if (isMultiClip(raw)) {
    return raw as unknown as MultiClipAnimData;
  }
  // Legacy single-clip format
  return singleToMulti(raw as unknown as AnimationClipData);
}

/** Save an `.anim` file to disk (always writes multi-clip format). */
export async function saveAnimFile(
  fullPath: string,
  data: MultiClipAnimData,
): Promise<void> {
  const json = JSON.stringify(data, null, 2);
  await writeTextFile(fullPath, json);
}

/**
 * Walk a scene's entities and inline any `sprite_animator.clip_source`
 * references by reading the referenced `.anim` file. Mutates `sceneData`
 * in place. When the `.anim` file declares a `texture_id` and the sibling
 * sprite has none, the sprite's texture is backfilled. Errors per entity
 * are logged via the optional `onError` callback and do not abort.
 *
 * This mirrors the native runtime's `resolve_anim_sources` pass
 * (crates/jengine-core/src/animation/asset.rs) — the editor must run
 * this before handing the scene JSON to the viewport WASM since WASM
 * has no filesystem access.
 */
export async function resolveAnimSourcesInScene(
  projectPath: string,
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  sceneData: { entities?: any[] },
  onError?: (entityName: string, err: unknown) => void,
): Promise<number> {
  const entities = sceneData.entities ?? [];
  let resolved = 0;
  for (const ent of entities) {
    const components = ent?.components;
    const animator = components?.sprite_animator;
    const source: unknown = animator?.clip_source;
    if (!animator || typeof source !== "string" || source.length === 0)
      continue;
    try {
      const data = await readAnimFile(projectPath, source);
      animator.clips = data.clips;
      const sprite = components?.sprite;
      if (
        sprite &&
        data.texture_id &&
        (!sprite.texture_id || sprite.texture_id.length === 0)
      ) {
        sprite.texture_id = data.texture_id;
      }
      resolved += 1;
    } catch (err) {
      onError?.(ent?.name ?? "<unnamed>", err);
    }
  }
  return resolved;
}
