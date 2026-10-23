import { writable, derived, get } from "svelte/store";
import type { DataSchema } from "../api/dataEditor";
import { loadDataSchema, saveDataSchema } from "../api/dataEditor";
import { logInfo, logError } from "./consoleStore";

/** The schema currently being edited (null if none open). */
export const dataSchema = writable<DataSchema | null>(null);

/** Absolute path to the `.dataschema` file on disk. */
export const dataSchemaPath = writable<string | null>(null);

/** Whether the schema has unsaved changes. */
export const dataSchemaDirty = writable(false);

/** Index of the selected field row (-1 = none). */
export const selectedField = writable<number>(-1);

const undoStack = writable<string[]>([]);
const redoStack = writable<string[]>([]);

export const canUndoSchema = derived(undoStack, ($s) => $s.length > 0);
export const canRedoSchema = derived(redoStack, ($s) => $s.length > 0);

function snapshot(): string {
  const data = get(dataSchema);
  return data ? JSON.stringify(data) : "";
}

function pushUndo() {
  undoStack.update((stack) => [...stack.slice(-49), snapshot()]);
  redoStack.set([]);
}

/** Mutate the schema with undo support. */
export function executeSchema(
  _label: string,
  mutate: (data: DataSchema) => void,
) {
  const data = get(dataSchema);
  if (!data) return;
  pushUndo();
  const clone = structuredClone(data);
  mutate(clone);
  dataSchema.set(clone);
  dataSchemaDirty.set(true);
}

export function undoSchema() {
  const stack = get(undoStack);
  if (stack.length === 0) return;
  redoStack.update((r) => [...r, snapshot()]);
  const prev = stack[stack.length - 1];
  undoStack.update((s) => s.slice(0, -1));
  dataSchema.set(JSON.parse(prev));
  dataSchemaDirty.set(true);
}

export function redoSchema() {
  const stack = get(redoStack);
  if (stack.length === 0) return;
  undoStack.update((u) => [...u, snapshot()]);
  const next = stack[stack.length - 1];
  redoStack.update((r) => r.slice(0, -1));
  dataSchema.set(JSON.parse(next));
  dataSchemaDirty.set(true);
}

export async function openDataSchemaFile(fullPath: string) {
  try {
    const data = await loadDataSchema(fullPath);
    dataSchema.set(data);
    dataSchemaPath.set(fullPath);
    dataSchemaDirty.set(false);
    selectedField.set(data.fields.length > 0 ? 0 : -1);
    undoStack.set([]);
    redoStack.set([]);
    logInfo(`Opened data schema: ${fullPath.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to open data schema: ${e}`);
  }
}

export async function saveCurrentDataSchema() {
  const data = get(dataSchema);
  const path = get(dataSchemaPath);
  if (!data || !path) return;
  try {
    await saveDataSchema(path, data);
    dataSchemaDirty.set(false);
    logInfo(`Saved data schema: ${path.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to save data schema: ${e}`);
  }
}

export function closeDataSchemaEditor() {
  dataSchema.set(null);
  dataSchemaPath.set(null);
  dataSchemaDirty.set(false);
  selectedField.set(-1);
  undoStack.set([]);
  redoStack.set([]);
}
