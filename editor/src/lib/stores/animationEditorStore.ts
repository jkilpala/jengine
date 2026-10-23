import { writable, derived, get } from "svelte/store";
import type { AnimationClipData, MultiClipAnimData } from "../api/animation";
import { readAnimFile, saveAnimFile, defaultMultiClipAnim } from "../api/animation";
import { logInfo, logError } from "./consoleStore";
import { scene } from "./sceneStore";
import { execute } from "./commandStore";
import { selectedEntity } from "./selectionStore";
import { editThroughInstance, viewOf } from "../prefabInstance";
import { prefabLibrary, scenePrefabs } from "./prefabLibraryStore";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/** The full multi-clip animation data being edited (null if no file open). */
export const animData = writable<MultiClipAnimData | null>(null);

/** Currently selected clip name. */
export const selectedClipName = writable<string | null>(null);

/** Full path to the .anim file on disk. */
export const animFilePath = writable<string | null>(null);

/** Whether the data has unsaved changes. */
export const animDirty = writable(false);

/** Entity index linked to this animation editor (for bidirectional sync). -1 = none. */
export const linkedEntityIndex = writable<number>(-1);

/** Currently selected frame index in the timeline. */
export const selectedFrameIndex = writable<number>(0);

/** Undo/redo history. */
const undoStack = writable<string[]>([]);
const redoStack = writable<string[]>([]);

// ---------------------------------------------------------------------------
// Derived
// ---------------------------------------------------------------------------

/** The currently selected clip (convenience derived). */
export const animClip = derived(
  [animData, selectedClipName],
  ([$data, $name]) => {
    if (!$data || !$name) return null;
    return $data.clips[$name] ?? null;
  },
);

/** List of clip names in the current animation data. */
export const clipNames = derived(animData, ($data) =>
  $data ? Object.keys($data.clips) : [],
);

export const canUndoAnim = derived(undoStack, ($s) => $s.length > 0);
export const canRedoAnim = derived(redoStack, ($s) => $s.length > 0);

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function snapshot(): string {
  const data = get(animData);
  return data ? JSON.stringify(data) : "";
}

function pushUndo() {
  const s = snapshot();
  undoStack.update((stack) => [...stack.slice(-49), s]);
  redoStack.set([]);
}

// ---------------------------------------------------------------------------
// Mutations (all push undo)
// ---------------------------------------------------------------------------

/** Execute a mutation on the entire multi-clip data with undo support. */
export function executeAnimData(
  _label: string,
  mutate: (data: MultiClipAnimData) => void,
) {
  const data = get(animData);
  if (!data) return;
  pushUndo();
  const clone = structuredClone(data);
  mutate(clone);
  animData.set(clone);
  animDirty.set(true);
}

/** Execute a mutation on the currently selected clip with undo support. */
export function executeAnim(
  _label: string,
  mutate: (clip: AnimationClipData) => void,
) {
  const data = get(animData);
  const name = get(selectedClipName);
  if (!data || !name || !data.clips[name]) return;
  pushUndo();
  const clone = structuredClone(data);
  mutate(clone.clips[name]);
  animData.set(clone);
  animDirty.set(true);
}

export function undoAnim() {
  const stack = get(undoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  redoStack.update((r) => [...r, current]);
  const prev = stack[stack.length - 1];
  undoStack.update((s) => s.slice(0, -1));
  const restored = JSON.parse(prev) as MultiClipAnimData;
  animData.set(restored);
  // If selected clip no longer exists, pick the first one
  const name = get(selectedClipName);
  if (name && !restored.clips[name]) {
    const first = Object.keys(restored.clips)[0] ?? null;
    selectedClipName.set(first);
  }
  animDirty.set(true);
}

export function redoAnim() {
  const stack = get(redoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  undoStack.update((u) => [...u, current]);
  const next = stack[stack.length - 1];
  redoStack.update((r) => r.slice(0, -1));
  const restored = JSON.parse(next) as MultiClipAnimData;
  animData.set(restored);
  const name = get(selectedClipName);
  if (name && !restored.clips[name]) {
    const first = Object.keys(restored.clips)[0] ?? null;
    selectedClipName.set(first);
  }
  animDirty.set(true);
}

// ---------------------------------------------------------------------------
// Clip management
// ---------------------------------------------------------------------------

export function selectClip(name: string) {
  selectedClipName.set(name);
  selectedFrameIndex.set(0);
}

export function addClip(name: string) {
  executeAnimData("Add clip", (data) => {
    data.clips[name] = {
      name,
      frame_duration: 0.1,
      looping: true,
      frames: [{ uv_rect: [0, 0, 1, 1] }],
    };
  });
  selectedClipName.set(name);
  selectedFrameIndex.set(0);
}

export function removeClip(name: string) {
  const data = get(animData);
  if (!data || Object.keys(data.clips).length <= 1) return;
  executeAnimData("Remove clip", (d) => {
    delete d.clips[name];
  });
  if (get(selectedClipName) === name) {
    const remaining = Object.keys(get(animData)?.clips ?? {});
    selectedClipName.set(remaining[0] ?? null);
    selectedFrameIndex.set(0);
  }
}

export function renameClip(oldName: string, newName: string) {
  if (!newName.trim() || oldName === newName) return;
  executeAnimData("Rename clip", (data) => {
    const clip = data.clips[oldName];
    if (!clip) return;
    clip.name = newName;
    data.clips[newName] = clip;
    delete data.clips[oldName];
  });
  if (get(selectedClipName) === oldName) {
    selectedClipName.set(newName);
  }
}

export function duplicateClip(name: string) {
  const data = get(animData);
  if (!data || !data.clips[name]) return;
  let newName = `${name}_copy`;
  let i = 2;
  while (data.clips[newName]) {
    newName = `${name}_copy${i++}`;
  }
  executeAnimData("Duplicate clip", (d) => {
    d.clips[newName] = structuredClone(d.clips[name]);
    d.clips[newName].name = newName;
  });
  selectedClipName.set(newName);
  selectedFrameIndex.set(0);
}

// ---------------------------------------------------------------------------
// File operations
// ---------------------------------------------------------------------------

/** Extract projectPath and relativePath from a full file path. */
function splitPath(fullPath: string): {
  projectPath: string;
  relativePath: string;
} {
  const normalized = fullPath.replace(/\\/g, "/");
  const assetsIdx = normalized.indexOf("/assets/");
  if (assetsIdx >= 0) {
    return {
      projectPath: normalized.slice(0, assetsIdx),
      relativePath: normalized.slice(assetsIdx + "/assets/".length),
    };
  }
  const lastSlash = normalized.lastIndexOf("/");
  return {
    projectPath: normalized.slice(0, lastSlash),
    relativePath: normalized.slice(lastSlash + 1),
  };
}

export async function openAnimFile(fullPath: string) {
  try {
    const { projectPath, relativePath } = splitPath(fullPath);
    const data = await readAnimFile(projectPath, relativePath);
    animData.set(data);
    animFilePath.set(fullPath);
    animDirty.set(false);
    selectedFrameIndex.set(0);
    const first = Object.keys(data.clips)[0] ?? null;
    selectedClipName.set(first);
    undoStack.set([]);
    redoStack.set([]);
    logInfo(`Opened animation: ${fullPath.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to open animation file: ${e}`);
  }
}

export async function saveCurrentAnim() {
  const data = get(animData);
  const path = get(animFilePath);
  if (!data || !path) return;
  try {
    await saveAnimFile(path, data);
    animDirty.set(false);
    logInfo(`Saved animation: ${path.split(/[\\/]/).pop()}`);
    // Sync back to linked entity
    syncToLinkedEntity();
  } catch (e) {
    logError(`Failed to save animation: ${e}`);
  }
}

// ---------------------------------------------------------------------------
// Entity linking
// ---------------------------------------------------------------------------

/** Build a sprite_animator clips record from MultiClipAnimData. */
function buildEntityClips(data: MultiClipAnimData) {
  // `AnimationClipData` from `api/animation.ts`, not a second copy of it: this
  // one required a `uv_rect`, which a frame naming a region does not have.
  const clips: Record<string, AnimationClipData> = {};
  for (const [name, clip] of Object.entries(data.clips)) {
    clips[name] = {
      name: clip.name,
      frames: clip.frames.map((f) => ({
        uv_rect: f.uv_rect
          ? ([...f.uv_rect] as [number, number, number, number])
          : undefined,
        region: f.region,
        texture_id: f.texture_id ?? undefined,
        triggers: f.triggers ?? [],
      })),
      frame_duration: clip.frame_duration,
      looping: clip.looping,
    };
  }
  return clips;
}

/** Push current animation data to the linked entity's sprite_animator. */
export function syncToLinkedEntity() {
  const data = get(animData);
  const idx = get(linkedEntityIndex);
  if (!data || idx < 0) return;
  const s = get(scene);
  if (!s.entities[idx]) return;
  const clips = buildEntityClips(data);
  const prefabs = get(scenePrefabs);
  execute("Sync animation to entity", (s) => {
    const stored = s.entities[idx];
    if (!stored) return s;
    if (!stored.components) stored.components = {};
    // Through the instance: an animator the entity's prefab supplies becomes
    // an override rather than a copy written over it.
    editThroughInstance(stored, prefabs, (entity) => {
    const existing = entity.components.sprite_animator;
    entity.components.sprite_animator = {
      clips,
      current_clip: existing?.current_clip ?? Object.keys(clips)[0] ?? null,
      current_frame: 0,
      elapsed: 0,
      playing: existing?.playing ?? true,
      speed: existing?.speed ?? 1.0,
      random_start: existing?.random_start ?? false,
    };
    if (data.texture_id && entity.components.sprite) {
      entity.components.sprite.texture_id = data.texture_id;
    }
    });
    return s;
  });
}

/** Link the currently selected entity to this animation editor. */
export function linkSelectedEntity() {
  const idx = get(selectedEntity);
  if (idx < 0) return;
  linkedEntityIndex.set(idx);
  syncToLinkedEntity();
  logInfo(`Linked animation to entity #${idx}`);
}

/** Unlink the entity. */
export function unlinkEntity() {
  linkedEntityIndex.set(-1);
  logInfo("Unlinked animation from entity");
}

/** Load sprite_animator data from the selected entity into the animation editor. */
export function loadFromEntity() {
  const idx = get(selectedEntity);
  if (idx < 0) return;
  const s = get(scene);
  // As it is: an instance's animator may come from its prefab.
  const stored = s.entities[idx];
  const entity = stored ? viewOf(stored, get(scenePrefabs)) : stored;
  if (!entity?.components?.sprite_animator) {
    logError("Selected entity has no sprite_animator component");
    return;
  }
  const sa = entity.components.sprite_animator;
  const clips: Record<string, AnimationClipData> = {};
  for (const [name, clip] of Object.entries(sa.clips)) {
    clips[name] = {
      name: clip.name,
      frames: clip.frames.map((f) => ({
        uv_rect: f.uv_rect
          ? ([...f.uv_rect] as [number, number, number, number])
          : undefined,
        region: f.region,
        texture_id: f.texture_id,
        triggers: f.triggers as AnimationClipData["frames"][0]["triggers"],
      })),
      frame_duration: clip.frame_duration,
      looping: clip.looping,
    };
  }
  const textureId = entity.components.sprite?.texture_id;
  const data: MultiClipAnimData = {
    clips,
    texture_id: textureId || undefined,
  };
  animData.set(data);
  const first = Object.keys(clips)[0] ?? null;
  selectedClipName.set(first);
  selectedFrameIndex.set(0);
  linkedEntityIndex.set(idx);
  animDirty.set(true);
  undoStack.set([]);
  redoStack.set([]);
  logInfo(`Loaded animation from entity: ${entity.name || `#${idx}`}`);
}

export function closeAnimEditor() {
  animData.set(null);
  selectedClipName.set(null);
  animFilePath.set(null);
  animDirty.set(false);
  selectedFrameIndex.set(0);
  linkedEntityIndex.set(-1);
  undoStack.set([]);
  redoStack.set([]);
}
