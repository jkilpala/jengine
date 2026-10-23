import { writable } from "svelte/store";
import type { SceneData } from "../api/scene";

/** Whether the engine is in play mode (scripts running). */
export const isPlaying = writable(false);

/**
 * Whether play mode is frozen. The viewport keeps rendering, so the
 * editor can pick, drag gizmos, and edit the Inspector against a world
 * that is not moving. Only meaningful while `isPlaying`.
 */
export const isPaused = writable(false);

/**
 * Where the editor was when play mode started, captured by the Toolbar's
 * play handler and consumed by its stop handler.
 *
 * Scripts can switch scenes during play (`load_scene`), and the viewport
 * re-snapshots on every transition — so `viewport_stop()` hands back the
 * *last* scene played, not the one you pressed Play in. Restoring from
 * this snapshot instead puts the editor back on the scene (and file path,
 * and dirty flag) it started from.
 */
export const playReturn = writable<PlayReturn | null>(null);

export interface PlayReturn {
  scene: SceneData;
  path: string | null;
  dirty: boolean;
}
