import { get, writable } from "svelte/store";

import { animDirty, closeAnimEditor, saveCurrentAnim } from "./animationEditorStore";
import { behaviorDirty, closeBehaviorEditor, saveBehavior } from "./behaviorEditorStore";
import {
  dataSchemaDirty,
  closeDataSchemaEditor,
  saveCurrentDataSchema,
} from "./dataSchemaEditorStore";
import {
  dataTableDirty,
  closeDataTableEditor,
  saveCurrentDataTable,
} from "./dataTableEditorStore";
import { fontDirty, closeFontEditor, saveCurrentFont } from "./fontEditorStore";
import { guiDirty, closeGuiEditor, saveCurrentGui } from "./guiEditorStore";
import {
  particleDirty,
  closeParticleEditor,
  saveCurrentParticle,
} from "./particleEditorStore";
import { prefabDirty, closePrefabEditor, saveCurrentPrefab } from "./prefabEditorStore";
import {
  sortLayersDirty,
  closeSortLayersEditor,
  saveCurrentSortLayers,
} from "./sortLayersEditorStore";
import { stateMachineDirty, closeStateMachineEditor } from "./stateMachineEditorStore";
import { tilemapDirty, closeTilemapEditor, saveCurrentTilemap } from "./tilemapEditorStore";
import { vectorPathDirty, closeVectorPathEditor } from "./vectorPathEditorStore";

/**
 * Closing an asset editor, with the same Save / Discard / Cancel prompt the
 * scene and script tabs have always had.
 *
 * Every asset editor used to close by calling its store's `close*Editor()`
 * straight from the button, which resets the dirty flag and drops the edits —
 * while the very same tab was showing the unsaved dot. Twelve editors, two
 * close buttons each (the tab's ✕ and the editor's own), and not one of them
 * asked (BUGS #70).
 *
 * The guard lives here rather than in `App.svelte` so that **both** buttons go
 * through it without either one having to know a dialog exists, and so it can
 * be tested without a DOM. `App.svelte` only renders the dialog when
 * [`pendingEditorClose`] is set and hands the answer back to
 * [`resolveEditorClose`].
 */
export type EditorKind =
  | "animation"
  | "behavior"
  | "dataschema"
  | "datatable"
  | "font"
  | "gui"
  | "particle"
  | "prefab"
  | "sortlayers"
  | "statemachine"
  | "tilemap"
  | "vectorpath";

interface EditorCloser {
  /** How the editor is named in the prompt. Matches `unsavedWork`'s labels. */
  label: string;
  isDirty: () => boolean;
  /**
   * Write the document, or `null` when the editor has no save entry point of
   * its own.
   *
   * The state machine and vector path editors *apply to the entity* rather
   * than writing a file, and do it from inside their own components, so there
   * is nothing a store can call. Their prompt drops the Save button instead of
   * offering one that silently does nothing — see [`canSaveEditor`].
   */
  save: (() => Promise<unknown> | unknown) | null;
  close: () => void;
}

/**
 * Every editor that can hold unsaved work, and how to ask it about that work.
 *
 * A new editor added to the centre strip belongs here. `editorClose.test.ts`
 * fails if this list and `unsavedWork`'s drift apart, because an editor
 * missing from one of them is an editor whose work is silently discarded.
 */
export const EDITOR_CLOSERS: Record<EditorKind, EditorCloser> = {
  animation: {
    label: "Animation",
    isDirty: () => get(animDirty),
    save: saveCurrentAnim,
    close: closeAnimEditor,
  },
  behavior: {
    label: "Behavior graph",
    isDirty: () => get(behaviorDirty),
    save: saveBehavior,
    close: closeBehaviorEditor,
  },
  dataschema: {
    label: "Data schema",
    isDirty: () => get(dataSchemaDirty),
    save: saveCurrentDataSchema,
    close: closeDataSchemaEditor,
  },
  datatable: {
    label: "Data table",
    isDirty: () => get(dataTableDirty),
    save: saveCurrentDataTable,
    close: closeDataTableEditor,
  },
  font: {
    label: "Font",
    isDirty: () => get(fontDirty),
    save: saveCurrentFont,
    close: closeFontEditor,
  },
  gui: {
    label: "GUI document",
    isDirty: () => get(guiDirty),
    save: saveCurrentGui,
    close: closeGuiEditor,
  },
  particle: {
    label: "Particle effect",
    isDirty: () => get(particleDirty),
    save: saveCurrentParticle,
    close: closeParticleEditor,
  },
  prefab: {
    label: "Prefab",
    isDirty: () => get(prefabDirty),
    save: saveCurrentPrefab,
    close: closePrefabEditor,
  },
  sortlayers: {
    label: "Sort layers",
    isDirty: () => get(sortLayersDirty),
    save: saveCurrentSortLayers,
    close: closeSortLayersEditor,
  },
  statemachine: {
    label: "State machine",
    isDirty: () => get(stateMachineDirty),
    save: null,
    close: closeStateMachineEditor,
  },
  tilemap: {
    label: "Tilemap",
    isDirty: () => get(tilemapDirty),
    save: saveCurrentTilemap,
    close: closeTilemapEditor,
  },
  vectorpath: {
    label: "Vector path",
    isDirty: () => get(vectorPathDirty),
    save: null,
    close: closeVectorPathEditor,
  },
};

export const EDITOR_KINDS = Object.keys(EDITOR_CLOSERS) as EditorKind[];

/** The editor whose close is waiting on the dialog, or null. */
export const pendingEditorClose = writable<EditorKind | null>(null);

/** What the prompt should say is about to be lost. */
export function editorLabel(kind: EditorKind): string {
  return EDITOR_CLOSERS[kind].label;
}

/** Whether this editor can write its document from the prompt. */
export function canSaveEditor(kind: EditorKind): boolean {
  return EDITOR_CLOSERS[kind].save !== null;
}

export function isEditorDirty(kind: EditorKind): boolean {
  return EDITOR_CLOSERS[kind].isDirty();
}

/**
 * Close an editor, asking first when it has unsaved edits.
 *
 * Both close buttons call this. A clean editor closes immediately — the
 * prompt is for work that would be lost, and one on every close would train
 * people to dismiss it.
 */
export function requestEditorClose(kind: EditorKind): void {
  if (isEditorDirty(kind)) {
    pendingEditorClose.set(kind);
    return;
  }
  EDITOR_CLOSERS[kind].close();
}

/**
 * Apply the answer from the prompt.
 *
 * Returns whether the editor was closed, so a caller can tell a cancel from a
 * failed save. **A save that does not land must not close**: closing then
 * would throw away exactly the work the user just asked to keep, which is the
 * failure this whole path exists to prevent. Each `saveCurrent*` reports its
 * own error and leaves the dirty flag standing, so the flag is the signal.
 */
export async function resolveEditorClose(
  choice: "save" | "discard" | "cancel",
): Promise<boolean> {
  const kind = get(pendingEditorClose);
  pendingEditorClose.set(null);
  if (kind === null || choice === "cancel") return false;

  const editor = EDITOR_CLOSERS[kind];
  if (choice === "save") {
    if (!editor.save) return false;
    await editor.save();
    if (editor.isDirty()) return false;
  }
  editor.close();
  return true;
}
