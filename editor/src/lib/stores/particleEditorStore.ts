import { writable, derived, get } from "svelte/store";
import type { ParticleEmitterData } from "../api/particle";
import { readParticleFile, saveParticleFile } from "../api/particle";
import { logInfo, logError } from "./consoleStore";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/** The current particle emitter being edited (null if no file open). */
export const particleEmitter = writable<ParticleEmitterData | null>(null);

/** Full path to the .particle file on disk. */
export const particleFilePath = writable<string | null>(null);

/** Whether the emitter has unsaved changes. */
export const particleDirty = writable(false);

/** Undo/redo history. */
const undoStack = writable<string[]>([]);
const redoStack = writable<string[]>([]);

// ---------------------------------------------------------------------------
// Derived
// ---------------------------------------------------------------------------

export const canUndoParticle = derived(undoStack, ($s) => $s.length > 0);
export const canRedoParticle = derived(redoStack, ($s) => $s.length > 0);

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function snapshot(): string {
  const data = get(particleEmitter);
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

/** Execute a mutation on the emitter with undo support. */
export function executeParticle(
  _label: string,
  mutate: (data: ParticleEmitterData) => void,
) {
  const data = get(particleEmitter);
  if (!data) return;
  pushUndo();
  const clone = structuredClone(data);
  mutate(clone);
  particleEmitter.set(clone);
  particleDirty.set(true);
}

export function undoParticle() {
  const stack = get(undoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  redoStack.update((r) => [...r, current]);
  const prev = stack[stack.length - 1];
  undoStack.update((s) => s.slice(0, -1));
  particleEmitter.set(JSON.parse(prev));
  particleDirty.set(true);
}

export function redoParticle() {
  const stack = get(redoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  undoStack.update((u) => [...u, current]);
  const next = stack[stack.length - 1];
  redoStack.update((r) => r.slice(0, -1));
  particleEmitter.set(JSON.parse(next));
  particleDirty.set(true);
}

// ---------------------------------------------------------------------------
// File operations
// ---------------------------------------------------------------------------

/** Extract projectPath and relativePath from a full file path. */
function splitPath(fullPath: string): {
  projectPath: string;
  relativePath: string;
} {
  // The full path is: <projectPath>/assets/<rest>
  // or: <projectPath>/<relativePath>
  const normalized = fullPath.replace(/\\/g, "/");
  const assetsIdx = normalized.indexOf("/assets/");
  if (assetsIdx >= 0) {
    return {
      projectPath: normalized.slice(0, assetsIdx),
      relativePath: normalized.slice(assetsIdx + "/assets/".length),
    };
  }
  // Fallback: last path segment is the file
  const lastSlash = normalized.lastIndexOf("/");
  return {
    projectPath: normalized.slice(0, lastSlash),
    relativePath: normalized.slice(lastSlash + 1),
  };
}

export async function openParticleFile(fullPath: string) {
  try {
    const { projectPath, relativePath } = splitPath(fullPath);
    const data = await readParticleFile(projectPath, relativePath);
    particleEmitter.set(data);
    particleFilePath.set(fullPath);
    particleDirty.set(false);
    undoStack.set([]);
    redoStack.set([]);
    logInfo(`Opened particle emitter: ${fullPath.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to open particle file: ${e}`);
  }
}

export async function saveCurrentParticle() {
  const data = get(particleEmitter);
  const path = get(particleFilePath);
  if (!data || !path) return;
  try {
    await saveParticleFile(path, data);
    particleDirty.set(false);
    logInfo(`Saved particle emitter: ${path.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to save particle emitter: ${e}`);
  }
}

export function closeParticleEditor() {
  particleEmitter.set(null);
  particleFilePath.set(null);
  particleDirty.set(false);
  undoStack.set([]);
  redoStack.set([]);
}
