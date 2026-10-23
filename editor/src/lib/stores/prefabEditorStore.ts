import { writable, derived, get } from "svelte/store";
import type { EntityData } from "../api/scene";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
import { logInfo, logError } from "./consoleStore";
import { notifyPrefabsChanged } from "./prefabLibraryStore";
import { partAt, replacePart, type PartPath } from "../prefabParts";
import type { PrefabEntity } from "../prefabInstance";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/** The whole prefab being edited, its parts included (null if no file is
 *  open) - what is saved, and what undo snapshots. */
export const prefabDoc = writable<EntityData | null>(null);

/** Which part the stage and the inspector edit: child indices from the
 *  root, `[]` for the root. */
export const prefabFocus = writable<PartPath>([]);

const focusedOf = (doc: EntityData | null, focus: PartPath): EntityData | null =>
  doc ? ((partAt(doc as unknown as PrefabEntity, focus) as unknown as EntityData) ?? doc) : null;

/**
 * The focused part - the root unless a part is chosen in the prefab
 * editor's tree. Everything that shows or edits "the prefab" (the stage,
 * the prefab inspector) reads and writes this, so it edits the chosen part,
 * and its writes land in place in `prefabDoc`.
 */
export const prefabData = {
  subscribe: derived([prefabDoc, prefabFocus], ([$doc, $focus]) => focusedOf($doc, $focus)).subscribe,
  set(part: EntityData | null) {
    // Nothing open (or cleared): this is the whole prefab, at its root.
    if (!part || !get(prefabDoc)) {
      prefabDoc.set(part);
      prefabFocus.set([]);
      return;
    }
    const focus = get(prefabFocus);
    prefabDoc.update((d) =>
      d ? (replacePart(d as unknown as PrefabEntity, focus, () => part as unknown as PrefabEntity) as unknown as EntityData) : d,
    );
  },
  update(fn: (part: EntityData) => EntityData) {
    const focus = get(prefabFocus);
    prefabDoc.update((d) => {
      if (!d) return d;
      return replacePart(d as unknown as PrefabEntity, focus, (p) =>
        fn(structuredClone(p) as unknown as EntityData) as unknown as PrefabEntity,
      ) as unknown as EntityData;
    });
  },
};

/** Edit the whole prefab's part tree (add, remove, move, rename a part),
 *  with undo, and focus `focus` after. */
export function executePrefabTree(fn: (doc: EntityData) => EntityData, focus?: PartPath) {
  if (!get(prefabDoc)) return;
  pushUndo();
  prefabDoc.update((d) => (d ? fn(structuredClone(d)) : d));
  if (focus) prefabFocus.set(focus);
  prefabDirty.set(true);
}

/** Full path to the .prefab file on disk. */
export const prefabFilePath = writable<string | null>(null);

/** Whether the prefab has unsaved changes. */
export const prefabDirty = writable(false);

/** Undo/redo history. */
const undoStack = writable<string[]>([]);
const redoStack = writable<string[]>([]);

// ---------------------------------------------------------------------------
// Derived
// ---------------------------------------------------------------------------

export const canUndo = derived(undoStack, ($s) => $s.length > 0);
export const canRedo = derived(redoStack, ($s) => $s.length > 0);

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function snapshot(): string {
  const data = get(prefabDoc);
  return data ? JSON.stringify(data) : "";
}

/** After an undo, the focused part may be gone: fall back to the root. */
function keepFocus() {
  const d = get(prefabDoc);
  if (d && !partAt(d as unknown as PrefabEntity, get(prefabFocus))) prefabFocus.set([]);
}

function pushUndo() {
  const snap = snapshot();
  undoStack.update((s) => [...s, snap]);
  redoStack.set([]);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/** Execute a mutation on the prefab with undo support. */
export function executePrefab(
  _label: string,
  fn: (data: EntityData) => EntityData,
) {
  if (!get(prefabDoc)) return;
  pushUndo();
  prefabData.update(fn);
  prefabDirty.set(true);
}

/**
 * Begin a drag-style edit: pushes one undo snapshot up front, then the
 * caller streams changes through `updatePrefabLive` so a whole gizmo drag
 * collapses into a single undo step.
 */
export function beginPrefabEdit() {
  if (!get(prefabDoc)) return;
  pushUndo();
}

/**
 * Apply a mutation *without* pushing undo — for the per-mousemove updates
 * of a drag opened with `beginPrefabEdit`.
 */
export function updatePrefabLive(fn: (data: EntityData) => EntityData) {
  if (!get(prefabDoc)) return;
  prefabData.update(fn);
  prefabDirty.set(true);
}

/** Undo the last prefab change. */
export function undoPrefab() {
  const stack = get(undoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  redoStack.update((s) => [...s, current]);
  const prev = stack[stack.length - 1];
  undoStack.update((s) => s.slice(0, -1));
  prefabDoc.set(JSON.parse(prev));
  keepFocus();
  prefabDirty.set(true);
}

/** Redo the last undone prefab change. */
export function redoPrefab() {
  const stack = get(redoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  undoStack.update((s) => [...s, current]);
  const next = stack[stack.length - 1];
  redoStack.update((s) => s.slice(0, -1));
  prefabDoc.set(JSON.parse(next));
  keepFocus();
  prefabDirty.set(true);
}

/** Open a .prefab file for editing. */
export async function openPrefabFile(fullPath: string) {
  try {
    const contents = await readTextFile(fullPath);
    const data: EntityData = JSON.parse(contents);
    prefabDoc.set(data);
    prefabFocus.set([]);
    prefabFilePath.set(fullPath);
    prefabDirty.set(false);
    undoStack.set([]);
    redoStack.set([]);
    logInfo(`Opened prefab: ${fullPath.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to open prefab: ${e}`);
  }
}

/** Save the current prefab to disk. */
export async function saveCurrentPrefab() {
  const data = get(prefabDoc);
  const path = get(prefabFilePath);
  if (!data || !path) return;
  try {
    const json = JSON.stringify(data, null, 2);
    await writeTextFile(path, json);
    prefabDirty.set(false);
    // Its instances in open scenes, and the viewport's prefab map.
    notifyPrefabsChanged();
    logInfo(`Saved prefab: ${path.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to save prefab: ${e}`);
  }
}

/** Close the prefab editor without saving. */
export function closePrefabEditor() {
  prefabDoc.set(null);
  prefabFocus.set([]);
  prefabFilePath.set(null);
  prefabDirty.set(false);
  undoStack.set([]);
  redoStack.set([]);
}
