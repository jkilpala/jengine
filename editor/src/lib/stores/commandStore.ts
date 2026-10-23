import { writable, get } from "svelte/store";
import { scene, dirty } from "./sceneStore";
import type { SceneData } from "../api/scene";

/** A reversible command that captures a scene snapshot before/after a mutation. */
export interface Command {
  /** Human-readable description of the action. */
  label: string;
  /** Scene state before the action. */
  before: SceneData;
  /** Scene state after the action. */
  after: SceneData;
}

const MAX_UNDO = 50;

export const undoStack = writable<Command[]>([]);
export const redoStack = writable<Command[]>([]);

/** Whether undo is available. */
export const canUndo = writable(false);
/** Whether redo is available. */
export const canRedo = writable(false);

function updateFlags() {
  canUndo.set(get(undoStack).length > 0);
  canRedo.set(get(redoStack).length > 0);
}

/**
 * Recompute `canUndo` / `canRedo` after the stacks are replaced wholesale.
 *
 * Scene tabs swap both stacks when switching documents; without this the
 * toolbar keeps advertising the previous scene's history.
 */
export function refreshUndoFlags() {
  updateFlags();
}

/**
 * Execute a scene mutation as an undoable command.
 * Takes a snapshot before, applies `mutate`, then takes a snapshot after.
 */
export function execute(label: string, mutate: (s: SceneData) => SceneData) {
  const before = structuredClone(get(scene));
  const after = mutate(structuredClone(before));

  undoStack.update((stack) => {
    stack.push({ label, before, after });
    if (stack.length > MAX_UNDO) stack.shift();
    return stack;
  });
  redoStack.set([]);

  scene.set(after);
  dirty.set(true);
  updateFlags();
}

/**
 * Push an already-computed before/after pair onto the undo stack.
 * Useful when the mutation is async (e.g., Tauri command returns new scene).
 */
export function pushCommand(label: string, before: SceneData, after: SceneData) {
  undoStack.update((stack) => {
    stack.push({ label, before: structuredClone(before), after: structuredClone(after) });
    if (stack.length > MAX_UNDO) stack.shift();
    return stack;
  });
  redoStack.set([]);

  scene.set(after);
  dirty.set(true);
  updateFlags();
}

/** Undo the last command. */
export function undo() {
  const stack = get(undoStack);
  if (stack.length === 0) return;

  const cmd = stack.pop()!;
  undoStack.set(stack);

  redoStack.update((r) => {
    r.push(cmd);
    return r;
  });

  scene.set(structuredClone(cmd.before));
  dirty.set(true);
  updateFlags();
}

/** Redo the last undone command. */
export function redo() {
  const stack = get(redoStack);
  if (stack.length === 0) return;

  const cmd = stack.pop()!;
  redoStack.set(stack);

  undoStack.update((u) => {
    u.push(cmd);
    return u;
  });

  scene.set(structuredClone(cmd.after));
  dirty.set(true);
  updateFlags();
}

/** Clear all undo/redo history (e.g., when loading a new scene). */
export function clearHistory() {
  undoStack.set([]);
  redoStack.set([]);
  updateFlags();
}
