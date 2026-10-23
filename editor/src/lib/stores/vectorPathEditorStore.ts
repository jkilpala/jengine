import { writable, get } from "svelte/store";
import type { PathCommand } from "../api/scene";

export interface VectorPathEditorData {
  commands: PathCommand[];
  fill_color: [number, number, number, number] | null;
  stroke_color: [number, number, number, number] | null;
  stroke_width: number;
  z_order: number;
  closed: boolean;
}

/** The vector path data currently being edited (null when closed). */
export const vectorPathData = writable<VectorPathEditorData | null>(null);

/** Entity ID being edited (so we can write changes back). */
export const vectorPathEntityId = writable<number | null>(null);

/** Dirty flag — true when unsaved changes exist. */
export const vectorPathDirty = writable(false);

/** Index of the currently selected command (null = nothing selected). */
export const vpSelectedCommand = writable<number | null>(null);

/** Current tool: "select" to move points, "pen" to add points. */
export const vpTool = writable<"select" | "pen">("select");

// ---- Undo / redo ----
let undoStack: string[] = [];
let redoStack: string[] = [];

export const canUndoVP = writable(false);
export const canRedoVP = writable(false);

function snapshot(): string {
  return JSON.stringify(get(vectorPathData));
}

/** Execute a mutation on the vector path data with undo support. */
export function executeVP(
  _label: string,
  fn: (data: VectorPathEditorData) => void,
) {
  const data = get(vectorPathData);
  if (!data) return;
  undoStack.push(snapshot());
  redoStack = [];
  fn(data);
  vectorPathData.set(structuredClone(data));
  vectorPathDirty.set(true);
  canUndoVP.set(true);
  canRedoVP.set(false);
}

export function undoVP() {
  if (undoStack.length === 0) return;
  redoStack.push(snapshot());
  const prev = undoStack.pop()!;
  vectorPathData.set(JSON.parse(prev));
  vectorPathDirty.set(true);
  canUndoVP.set(undoStack.length > 0);
  canRedoVP.set(true);
}

export function redoVP() {
  if (redoStack.length === 0) return;
  undoStack.push(snapshot());
  const next = redoStack.pop()!;
  vectorPathData.set(JSON.parse(next));
  vectorPathDirty.set(true);
  canUndoVP.set(true);
  canRedoVP.set(redoStack.length > 0);
}

/** Open the vector path editor with data from an entity. */
export function openVectorPathEditor(
  entityId: number,
  data: VectorPathEditorData,
) {
  undoStack = [];
  redoStack = [];
  canUndoVP.set(false);
  canRedoVP.set(false);
  vectorPathEntityId.set(entityId);
  vectorPathData.set(structuredClone(data));
  vectorPathDirty.set(false);
  vpSelectedCommand.set(null);
  vpTool.set("select");
}

/** Close the vector path editor. */
export function closeVectorPathEditor() {
  vectorPathData.set(null);
  vectorPathEntityId.set(null);
  vpSelectedCommand.set(null);
  undoStack = [];
  redoStack = [];
}
