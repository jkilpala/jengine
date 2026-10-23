import { writable, derived, get } from "svelte/store";
import type { GuiDocument, GuiNode, GuiNodeType } from "../api/gui";
import {
  defaultGuiDocument,
  defaultGuiNode,
  createNodeType,
  generateNodeId,
  findNode,
  findParent,
  removeNode,
} from "../api/gui";
import { readGuiFile, saveGuiFile } from "../api/gui";
import { logInfo, logError } from "./consoleStore";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/** The current GUI document being edited (null if no file open). */
export const guiDocument = writable<GuiDocument | null>(null);

/** Full path to the .gui file on disk. */
export const guiFilePath = writable<string | null>(null);

/** Whether the document has unsaved changes. */
export const guiDirty = writable(false);

/** ID of the currently selected node. */
export const selectedNodeId = writable<string | null>(null);

/** Undo/redo history. */
const undoStack = writable<string[]>([]);
const redoStack = writable<string[]>([]);

// ---------------------------------------------------------------------------
// Derived
// ---------------------------------------------------------------------------

/** The selected node object (derived from document + selectedNodeId). */
export const selectedNode = derived(
  [guiDocument, selectedNodeId],
  ([$doc, $id]) => {
    if (!$doc || !$id) return null;
    return findNode($doc.root, $id);
  }
);

export const canUndo = derived(undoStack, ($s) => $s.length > 0);
export const canRedo = derived(redoStack, ($s) => $s.length > 0);

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function snapshot(): string {
  const doc = get(guiDocument);
  return doc ? JSON.stringify(doc) : "";
}

function pushUndo() {
  const s = snapshot();
  undoStack.update((stack) => [...stack.slice(-49), s]);
  redoStack.set([]);
}

// ---------------------------------------------------------------------------
// Mutations (all push undo)
// ---------------------------------------------------------------------------

/** Execute a mutation on the document with undo support. */
export function executeGui(label: string, mutate: (doc: GuiDocument) => void) {
  const doc = get(guiDocument);
  if (!doc) return;
  pushUndo();
  const clone = structuredClone(doc);
  mutate(clone);
  guiDocument.set(clone);
  guiDirty.set(true);
}

export function undo() {
  const stack = get(undoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  redoStack.update((r) => [...r, current]);
  const prev = stack[stack.length - 1];
  undoStack.update((s) => s.slice(0, -1));
  guiDocument.set(JSON.parse(prev));
  guiDirty.set(true);
}

export function redo() {
  const stack = get(redoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  undoStack.update((u) => [...u, current]);
  const next = stack[stack.length - 1];
  redoStack.update((r) => r.slice(0, -1));
  guiDocument.set(JSON.parse(next));
  guiDirty.set(true);
}

// ---------------------------------------------------------------------------
// File operations
// ---------------------------------------------------------------------------

export async function openGuiFile(fullPath: string) {
  try {
    const doc = await readGuiFile(fullPath);
    guiDocument.set(doc);
    guiFilePath.set(fullPath);
    guiDirty.set(false);
    selectedNodeId.set(null);
    undoStack.set([]);
    redoStack.set([]);
    logInfo(`Opened GUI: ${fullPath.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to open GUI file: ${e}`);
  }
}

export async function saveCurrentGui() {
  const doc = get(guiDocument);
  const path = get(guiFilePath);
  if (!doc || !path) return;
  try {
    await saveGuiFile(path, doc);
    guiDirty.set(false);
    logInfo(`Saved GUI: ${path.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to save GUI: ${e}`);
  }
}

export function closeGuiEditor() {
  guiDocument.set(null);
  guiFilePath.set(null);
  guiDirty.set(false);
  selectedNodeId.set(null);
  undoStack.set([]);
  redoStack.set([]);
}

// ---------------------------------------------------------------------------
// Node operations
// ---------------------------------------------------------------------------

export function addChild(parentId: string, typeName: string) {
  executeGui(`Add ${typeName}`, (doc) => {
    const parent = findNode(doc.root, parentId);
    if (!parent) return;
    const nodeType = createNodeType(typeName);
    const id = generateNodeId(typeName.toLowerCase());
    const node = defaultGuiNode(id, nodeType);
    // Give new nodes a reasonable default size
    node.style.width = { type: "Fixed", value: 120 };
    node.style.height = { type: "Fixed", value: 40 };
    parent.children.push(node);
    selectedNodeId.set(id);
  });
}

export function deleteNode(nodeId: string) {
  if (nodeId === "root") return; // Cannot delete root
  executeGui("Delete node", (doc) => {
    removeNode(doc.root, nodeId);
  });
  const sel = get(selectedNodeId);
  if (sel === nodeId) selectedNodeId.set(null);
}

export function duplicateNode(nodeId: string) {
  if (nodeId === "root") return;
  executeGui("Duplicate node", (doc) => {
    const parent = findParent(doc.root, nodeId);
    const original = findNode(doc.root, nodeId);
    if (!parent || !original) return;
    const clone = structuredClone(original);
    // Give new unique IDs to the clone and all descendants
    function reassignIds(n: GuiNode) {
      n.id = generateNodeId(n.id.split("_")[0]);
      for (const c of n.children) reassignIds(c);
    }
    reassignIds(clone);
    const idx = parent.children.findIndex((c) => c.id === nodeId);
    parent.children.splice(idx + 1, 0, clone);
    selectedNodeId.set(clone.id);
  });
}

export function moveNode(nodeId: string, newParentId: string, insertIndex: number) {
  if (nodeId === "root") return;
  executeGui("Move node", (doc) => {
    const node = findNode(doc.root, nodeId);
    if (!node) return;
    // Prevent moving node into itself
    if (findNode(node, newParentId)) return;
    const clone = structuredClone(node);
    removeNode(doc.root, nodeId);
    const newParent = findNode(doc.root, newParentId);
    if (!newParent) return;
    const safeIdx = Math.min(insertIndex, newParent.children.length);
    newParent.children.splice(safeIdx, 0, clone);
  });
}

export function renameNode(nodeId: string, newId: string) {
  executeGui("Rename node", (doc) => {
    const node = findNode(doc.root, nodeId);
    if (node) node.id = newId;
  });
  if (get(selectedNodeId) === nodeId) selectedNodeId.set(newId);
}

export function updateNodeStyle(nodeId: string, field: string, value: unknown) {
  executeGui(`Change ${field}`, (doc) => {
    const node = findNode(doc.root, nodeId);
    if (node) (node.style as Record<string, unknown>)[field] = value;
  });
}

export function updateNodeType(nodeId: string, updater: (nt: GuiNodeType) => void) {
  executeGui("Update node", (doc) => {
    const node = findNode(doc.root, nodeId);
    if (node) updater(node.node_type);
  });
}
