import { writable } from "svelte/store";
import { parseSnap, type SnapSettings } from "../sceneEditing";
import type { SceneData } from "../api/scene";

export type GizmoMode = "move" | "rotate" | "scale";
export type GizmoSpace = "global" | "local";
export type ViewMode = "scene" | "game";

/** Active gizmo tool mode. */
export const gizmoMode = writable<GizmoMode>("move");

/** Gizmo coordinate space — global (world axes) or local (entity rotation). */
export const gizmoSpace = writable<GizmoSpace>("global");

/** Viewport view mode: scene (editor camera) or game (active game camera). */
export const viewMode = writable<ViewMode>("scene");

/** Whether collider wireframes are shown in the viewport. */
export const showColliders = writable(true);

/** Whether the profiler/stats panel is shown in the viewport. */
export const showProfiler = writable(false);

/** Polygon collider edit mode — entity index being edited, or -1 if not editing. */
export const polygonEditEntity = writable<number>(-1);

/** Which point list of that entity is edited, as a dotted path into its
 *  components (`"line.points"`, `"camera.confine"` - see `pointFields.ts`).
 *  Null: the entity's collider polygon. Set it before `polygonEditEntity`. */
export const polygonEditPath = writable<string | null>(null);

/** Whether the lighting preview is active in the viewport. */
export const showLighting = writable(false);

/** Whether joint connection gizmos are shown in the viewport. */
export const showJoints = writable(false);

/** Snapping while dragging in the scene view (`sceneEditing.ts`). A
 *  per-person convenience, so it is kept in this browser's storage, not the
 *  project; storage may be unavailable, which leaves the defaults. */
const SNAP_KEY = "jengine.editor.snap";
function loadSnap(): SnapSettings {
  try {
    const raw = localStorage.getItem(SNAP_KEY);
    return parseSnap(raw ? JSON.parse(raw) : null);
  } catch {
    return parseSnap(null);
  }
}
export const snapSettings = writable<SnapSettings>(loadSnap());
snapSettings.subscribe((v) => {
  try {
    localStorage.setItem(SNAP_KEY, JSON.stringify(v));
  } catch {
    // Not persisted; the setting still holds for this session.
  }
});

/** A scene the viewport shows instead of the real one, while it is set - the
 *  timeline panel's scrub, posing the scene at the playhead without writing
 *  a single value to the scene store. Null (the default) shows the scene. */
export const scenePreview = writable<SceneData | null>(null);
