import { writable, derived, get } from "svelte/store";
import type { FontFileData } from "../api/font";
import { readFontFile, saveFontFile, siblingAtlasPath } from "../api/font";
import { logInfo, logError } from "./consoleStore";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/** The current font being edited (null if no file open). */
export const fontData = writable<FontFileData | null>(null);

/** Full path to the .font file on disk. */
export const fontFilePath = writable<string | null>(null);

/** Whether the font has unsaved changes. */
export const fontDirty = writable(false);

/** Index of the currently selected glyph (-1 = none). */
export const selectedGlyph = writable<number>(-1);

/** Undo/redo history. */
const undoStack = writable<string[]>([]);
const redoStack = writable<string[]>([]);

// ---------------------------------------------------------------------------
// Derived
// ---------------------------------------------------------------------------

export const canUndoFont = derived(undoStack, ($s) => $s.length > 0);
export const canRedoFont = derived(redoStack, ($s) => $s.length > 0);

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function snapshot(): string {
  const data = get(fontData);
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

/** Execute a mutation on the font data with undo support. */
export function executeFont(
  _label: string,
  mutate: (data: FontFileData) => void,
) {
  const data = get(fontData);
  if (!data) return;
  pushUndo();
  const clone = structuredClone(data);
  mutate(clone);
  fontData.set(clone);
  fontDirty.set(true);
}

export function undoFont() {
  const stack = get(undoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  redoStack.update((r) => [...r, current]);
  const prev = stack[stack.length - 1];
  undoStack.update((s) => s.slice(0, -1));
  fontData.set(JSON.parse(prev));
  fontDirty.set(true);
}

export function redoFont() {
  const stack = get(redoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  undoStack.update((u) => [...u, current]);
  const next = stack[stack.length - 1];
  redoStack.update((r) => r.slice(0, -1));
  fontData.set(JSON.parse(next));
  fontDirty.set(true);
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

export async function openFontFile(fullPath: string) {
  try {
    const { projectPath, relativePath } = splitPath(fullPath);
    const data = await readFontFile(projectPath, relativePath);
    // A `.font` carries no texture reference - the runtime pairs it with the
    // sibling image by stem, and files generated outside the editor (jSuite,
    // and anything TTF-derived) have no `texture_id` at all. Without this the
    // atlas preview opens blank and the glyph editor is unusable until you
    // hand-pick an image that the engine was never going to consult.
    if (!data.texture_id) {
      data.texture_id = siblingAtlasPath(relativePath);
    }
    fontData.set(data);
    fontFilePath.set(fullPath);
    fontDirty.set(false);
    selectedGlyph.set(-1);
    undoStack.set([]);
    redoStack.set([]);
    logInfo(`Opened font: ${fullPath.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to open font file: ${e}`);
  }
}

export async function saveCurrentFont() {
  const data = get(fontData);
  const path = get(fontFilePath);
  if (!data || !path) return;
  try {
    await saveFontFile(path, data);
    fontDirty.set(false);
    logInfo(`Saved font: ${path.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to save font: ${e}`);
  }
}

export function closeFontEditor() {
  fontData.set(null);
  fontFilePath.set(null);
  fontDirty.set(false);
  selectedGlyph.set(-1);
  undoStack.set([]);
  redoStack.set([]);
}
