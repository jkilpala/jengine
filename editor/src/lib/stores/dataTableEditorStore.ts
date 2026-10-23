import { writable, derived, get } from "svelte/store";
import type { DataSchema, DataTable, DataRow } from "../api/dataEditor";
import {
  loadDataTable,
  saveDataTable,
  listDataSchemas,
  loadDataSchema,
} from "../api/dataEditor";
import { project } from "./projectStore";
import { logInfo, logError } from "./consoleStore";

/** The table currently being edited (null if none open). */
export const dataTable = writable<DataTable | null>(null);

/** The schema the open table conforms to (null if it could not be resolved). */
export const dataTableSchema = writable<DataSchema | null>(null);

/** Absolute path to the `.data` file on disk. */
export const dataTablePath = writable<string | null>(null);

/** Whether the table has unsaved changes. */
export const dataTableDirty = writable(false);

/** Index of the selected row (-1 = none). */
export const selectedRow = writable<number>(-1);

const undoStack = writable<string[]>([]);
const redoStack = writable<string[]>([]);

export const canUndoTable = derived(undoStack, ($s) => $s.length > 0);
export const canRedoTable = derived(redoStack, ($s) => $s.length > 0);

function snapshot(): string {
  const data = get(dataTable);
  return data ? JSON.stringify(data) : "";
}

function pushUndo() {
  undoStack.update((stack) => [...stack.slice(-49), snapshot()]);
  redoStack.set([]);
}

/** Mutate the table with undo support. */
export function executeTable(
  _label: string,
  mutate: (data: DataTable) => void,
) {
  const data = get(dataTable);
  if (!data) return;
  pushUndo();
  const clone = structuredClone(data);
  mutate(clone);
  dataTable.set(clone);
  dataTableDirty.set(true);
}

export function undoTable() {
  const stack = get(undoStack);
  if (stack.length === 0) return;
  redoStack.update((r) => [...r, snapshot()]);
  const prev = stack[stack.length - 1];
  undoStack.update((s) => s.slice(0, -1));
  dataTable.set(JSON.parse(prev));
  dataTableDirty.set(true);
}

export function redoTable() {
  const stack = get(redoStack);
  if (stack.length === 0) return;
  undoStack.update((u) => [...u, snapshot()]);
  const next = stack[stack.length - 1];
  redoStack.update((r) => r.slice(0, -1));
  dataTable.set(JSON.parse(next));
  dataTableDirty.set(true);
}

/**
 * Resolve and load the schema a table declares by scanning the project for a
 * matching `.dataschema`. Sets `dataTableSchema` to null if none is found.
 */
async function resolveSchema(schemaName: string) {
  const proj = get(project);
  if (!proj) {
    dataTableSchema.set(null);
    return;
  }
  try {
    const schemas = await listDataSchemas(proj.path);
    const match = schemas.find((s) => s.name === schemaName);
    if (!match) {
      dataTableSchema.set(null);
      logError(`No .dataschema found for schema '${schemaName}'`);
      return;
    }
    const fullPath = `${proj.path}/assets/${match.relative_path}`;
    dataTableSchema.set(await loadDataSchema(fullPath));
  } catch (e) {
    dataTableSchema.set(null);
    logError(`Failed to resolve schema '${schemaName}': ${e}`);
  }
}

export async function openDataTableFile(fullPath: string) {
  try {
    const data = await loadDataTable(fullPath);
    dataTable.set(data);
    dataTablePath.set(fullPath);
    dataTableDirty.set(false);
    selectedRow.set(data.rows.length > 0 ? 0 : -1);
    undoStack.set([]);
    redoStack.set([]);
    await resolveSchema(data.schema);
    logInfo(`Opened data table: ${fullPath.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to open data table: ${e}`);
  }
}

export async function saveCurrentDataTable() {
  const data = get(dataTable);
  const path = get(dataTablePath);
  if (!data || !path) return;
  try {
    await saveDataTable(path, data);
    dataTableDirty.set(false);
    logInfo(`Saved data table: ${path.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to save data table: ${e}`);
  }
}

export function closeDataTableEditor() {
  dataTable.set(null);
  dataTableSchema.set(null);
  dataTablePath.set(null);
  dataTableDirty.set(false);
  selectedRow.set(-1);
  undoStack.set([]);
  redoStack.set([]);
}

/** Build a fresh row from schema defaults with a unique id. */
export function makeDefaultRow(schema: DataSchema, existingIds: string[]): DataRow {
  let base = "new_row";
  let id = base;
  let n = 1;
  while (existingIds.includes(id)) {
    id = `${base}_${n++}`;
  }
  const row: DataRow = { id };
  for (const field of schema.fields) {
    row[field.name] = structuredClone(field.default ?? null);
  }
  return row;
}
