import { writable, derived, get } from "svelte/store";
import {
  readSortLayersFile,
  saveSortLayersFile,
  type SortLayersFile,
  type SortLayer,
} from "../api/sortLayers";
import { logInfo, logError } from "./consoleStore";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/** The current `.sortlayers` file being edited (null if no file open). */
export const sortLayersData = writable<SortLayersFile | null>(null);

/** Full path to the `.sortlayers` file on disk. */
export const sortLayersFilePath = writable<string | null>(null);

/** Whether the file has unsaved changes. */
export const sortLayersDirty = writable(false);

const undoStack = writable<string[]>([]);
const redoStack = writable<string[]>([]);

export const canUndoSortLayers = derived(undoStack, ($s) => $s.length > 0);
export const canRedoSortLayers = derived(redoStack, ($s) => $s.length > 0);

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function snapshot(): string {
  const data = get(sortLayersData);
  return data ? JSON.stringify(data) : "";
}

function pushUndo() {
  const s = snapshot();
  undoStack.update((stack) => [...stack.slice(-49), s]);
  redoStack.set([]);
}

// ---------------------------------------------------------------------------
// Mutations
// ---------------------------------------------------------------------------

export function executeSortLayers(
  _label: string,
  mutate: (data: SortLayersFile) => void,
) {
  const data = get(sortLayersData);
  if (!data) return;
  pushUndo();
  const clone = structuredClone(data);
  mutate(clone);
  sortLayersData.set(clone);
  sortLayersDirty.set(true);
}

export function undoSortLayers() {
  const stack = get(undoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  redoStack.update((r) => [...r, current]);
  const prev = stack[stack.length - 1];
  undoStack.update((s) => s.slice(0, -1));
  sortLayersData.set(JSON.parse(prev));
  sortLayersDirty.set(true);
}

export function redoSortLayers() {
  const stack = get(redoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  undoStack.update((u) => [...u, current]);
  const next = stack[stack.length - 1];
  redoStack.update((r) => r.slice(0, -1));
  sortLayersData.set(JSON.parse(next));
  sortLayersDirty.set(true);
}

export function addSortLayer(layer: SortLayer = { name: "new_layer", base_z: 0 }) {
  executeSortLayers("Add layer", (d) => {
    d.layers.push({ ...layer });
  });
}

export function removeSortLayer(index: number) {
  executeSortLayers("Remove layer", (d) => {
    d.layers.splice(index, 1);
  });
}

export function moveSortLayer(index: number, direction: -1 | 1) {
  executeSortLayers("Reorder layer", (d) => {
    const target = index + direction;
    if (target < 0 || target >= d.layers.length) return;
    const [item] = d.layers.splice(index, 1);
    d.layers.splice(target, 0, item);
  });
}

export function updateSortLayer(index: number, patch: Partial<SortLayer>) {
  executeSortLayers("Edit layer", (d) => {
    if (index < 0 || index >= d.layers.length) return;
    d.layers[index] = { ...d.layers[index], ...patch };
  });
}

// ---------------------------------------------------------------------------
// File operations
// ---------------------------------------------------------------------------

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

export async function openSortLayersFile(fullPath: string) {
  try {
    const { projectPath, relativePath } = splitPath(fullPath);
    const data = await readSortLayersFile(projectPath, relativePath);
    sortLayersData.set(data);
    sortLayersFilePath.set(fullPath);
    sortLayersDirty.set(false);
    undoStack.set([]);
    redoStack.set([]);
    logInfo(`Opened sort layers: ${fullPath.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to open sort layers file: ${e}`);
  }
}

export async function saveCurrentSortLayers() {
  const data = get(sortLayersData);
  const path = get(sortLayersFilePath);
  if (!data || !path) return;
  try {
    await saveSortLayersFile(path, data);
    sortLayersDirty.set(false);
    logInfo(`Saved sort layers: ${path.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to save sort layers: ${e}`);
  }
}

export function closeSortLayersEditor() {
  sortLayersData.set(null);
  sortLayersFilePath.set(null);
  sortLayersDirty.set(false);
  undoStack.set([]);
  redoStack.set([]);
}
