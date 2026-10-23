import { writable, derived, get } from "svelte/store";
import type { TilemapData } from "../api/tilemap";
import { readTilemapFile, saveTilemapFile } from "../api/tilemap";
import { logInfo, logError } from "./consoleStore";
import { scene, dirty } from "./sceneStore";
import { execute } from "./commandStore";
import { tilemapFromComponent, writeTilemapToComponent, type AutotileData } from "../autotileEdit";

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/** The current tilemap being edited (null if no file open). */
export const tilemapData = writable<TilemapData | null>(null);

/** Full path to the .tilemap file on disk. */
export const tilemapFilePath = writable<string | null>(null);

/** Whether the tilemap has unsaved changes. */
export const tilemapDirty = writable(false);

/** The scene entity the map belongs to (by name): the one it was opened
 *  from, or the first whose `tilemap.source` is the file. Its `autotile`
 *  component says which layers are painted in terrains. */
export const tilemapEntity = writable<string | null>(null);

/** Whether the map is that entity's own (inline), saved back into the
 *  scene rather than to a `.tilemap` file. */
export const tilemapInline = writable(false);

/** The entity's `autotile` rule, edited beside the map (`autotileEdit.ts`). */
export const tilemapAutotile = writable<AutotileData | null>(null);

/** Which terrain the brush paints on an autotiled layer (1-based). */
export const selectedTerrain = writable<number>(1);

/** Currently selected layer index. */
export const selectedLayerIndex = writable<number>(0);

/** Currently selected tile ID for painting (null = eraser). */
export const selectedTileId = writable<number | null>(null);

/** Current brush mode. */
export const brushMode = writable<"paint" | "erase" | "fill" | "rect" | "stamp">("paint");

/** The block the stamp brush lays (`tileProps.copyBlock`): copied with
 *  Ctrl held as a rectangle is let go. */
export const stampBlock = writable<import("../tileProps").Block | null>(null);

/** The tiles a random brush paints from: Ctrl- or Shift-click in the palette
 *  adds one. With more than one, each painted cell takes one at random. */
export const selectedTileIds = writable<number[]>([]);

/** Undo/redo history. */
const undoStack = writable<string[]>([]);
const redoStack = writable<string[]>([]);

// ---------------------------------------------------------------------------
// Derived
// ---------------------------------------------------------------------------

export const canUndoTilemap = derived(undoStack, ($s) => $s.length > 0);
export const canRedoTilemap = derived(redoStack, ($s) => $s.length > 0);

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/** The map and its terrain rule, together: one undo step covers both. */
function snapshot(): string {
  const data = get(tilemapData);
  return data ? JSON.stringify({ map: data, autotile: get(tilemapAutotile) }) : "";
}

function restore(text: string) {
  const s = JSON.parse(text);
  // Older snapshots (before the rule rode along) are the map alone.
  if (s && s.map) {
    tilemapData.set(s.map);
    tilemapAutotile.set(s.autotile ?? null);
  } else {
    tilemapData.set(s);
  }
}

/** Change the terrain rule, with undo. */
export function setTilemapAutotile(next: AutotileData | null) {
  if (!get(tilemapData)) return;
  pushUndo();
  tilemapAutotile.set(next);
  tilemapDirty.set(true);
}

function pushUndo() {
  const s = snapshot();
  undoStack.update((stack) => [...stack.slice(-49), s]);
  redoStack.set([]);
}

// ---------------------------------------------------------------------------
// Mutations (all push undo)
// ---------------------------------------------------------------------------

/** Execute a mutation on the tilemap data with undo support. */
export function executeTilemap(
  _label: string,
  mutate: (data: TilemapData) => void,
) {
  const data = get(tilemapData);
  if (!data) return;
  pushUndo();
  const clone = structuredClone(data);
  mutate(clone);
  tilemapData.set(clone);
  tilemapDirty.set(true);
}

/** Start a brush stroke: one undo step for the whole drag, however many
 *  cells it paints (`updateTilemapLive` makes the edits). Painting used to
 *  be a step per cell, so undoing one stroke took a Ctrl+Z per tile. */
export function beginTilemapStroke() {
  pendingStroke = get(tilemapData) ? snapshot() : null;
}

/** The map as a stroke began, kept until its first edit makes it an undo
 *  step - a click that changes nothing leaves none. */
let pendingStroke: string | null = null;

/** An edit inside a stroke begun with `beginTilemapStroke` - no undo step
 *  of its own. */
export function updateTilemapLive(mutate: (data: TilemapData) => void) {
  const data = get(tilemapData);
  if (!data) return;
  if (pendingStroke !== null) {
    const before = pendingStroke;
    undoStack.update((stack) => [...stack.slice(-49), before]);
    redoStack.set([]);
    pendingStroke = null;
  }
  const clone = structuredClone(data);
  mutate(clone);
  tilemapData.set(clone);
  tilemapDirty.set(true);
}

export function undoTilemap() {
  const stack = get(undoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  redoStack.update((r) => [...r, current]);
  const prev = stack[stack.length - 1];
  undoStack.update((s) => s.slice(0, -1));
  restore(prev);
  tilemapDirty.set(true);
}

export function redoTilemap() {
  const stack = get(redoStack);
  if (stack.length === 0) return;
  const current = snapshot();
  undoStack.update((u) => [...u, current]);
  const next = stack[stack.length - 1];
  redoStack.update((r) => r.slice(0, -1));
  restore(next);
  tilemapDirty.set(true);
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

export async function openTilemapFile(fullPath: string, entityName?: string) {
  try {
    const { projectPath, relativePath } = splitPath(fullPath);
    const data = await readTilemapFile(projectPath, relativePath);
    tilemapData.set(data);
    tilemapFilePath.set(fullPath);
    // The entity the map belongs to: the one that opened it, else the
    // first that names the file.
    const ents = get(scene).entities;
    const owner =
      ents.find((e) => e.name === entityName) ??
      ents.find((e) => (e.components as Record<string, any>)?.tilemap?.source === relativePath);
    tilemapEntity.set(owner?.name ?? null);
    tilemapInline.set(false);
    tilemapAutotile.set(structuredClone((owner?.components as Record<string, any>)?.autotile ?? null));
    tilemapDirty.set(false);
    selectedLayerIndex.set(0);
    selectedTileId.set(null);
    undoStack.set([]);
    redoStack.set([]);
    logInfo(`Opened tilemap: ${relativePath}`);
  } catch (e) {
    logError(`Failed to open tilemap: ${e}`);
  }
}

/** Open an entity's own (inline) map, and its terrain rule. */
export function openEntityTilemap(entityName: string) {
  const owner = get(scene).entities.find((e) => e.name === entityName);
  const tm = (owner?.components as Record<string, any> | undefined)?.tilemap;
  if (!owner || !tm) {
    logError(`No tilemap on "${entityName}"`);
    return;
  }
  tilemapData.set(tilemapFromComponent(tm));
  tilemapFilePath.set(null);
  tilemapEntity.set(owner.name);
  tilemapInline.set(true);
  tilemapAutotile.set(structuredClone((owner.components as Record<string, any>).autotile ?? null));
  tilemapDirty.set(false);
  selectedLayerIndex.set(0);
  selectedTileId.set(null);
  undoStack.set([]);
  redoStack.set([]);
  logInfo(`Opened ${owner.name}'s tilemap`);
}

/** Write the terrain rule (and, for an inline map, the map) into the
 *  owning entity - one scene undo step. */
function writeToEntity(data: TilemapData, inline: boolean) {
  const name = get(tilemapEntity);
  if (!name) return false;
  const autotile = get(tilemapAutotile);
  let found = false;
  execute(inline ? `Edit ${name}'s tilemap` : `Edit ${name}'s terrains`, (s) => {
    const e = s.entities.find((x) => x.name === name);
    if (!e) return s;
    found = true;
    const comps = e.components as Record<string, any>;
    if (inline) comps.tilemap = writeTilemapToComponent(comps.tilemap ?? {}, data);
    if (autotile) comps.autotile = structuredClone(autotile);
    else delete comps.autotile;
    return s;
  });
  return found;
}

export async function saveCurrentTilemap() {
  const data = get(tilemapData);
  const fullPath = get(tilemapFilePath);
  if (data && get(tilemapInline)) {
    if (writeToEntity(data, true)) {
      tilemapDirty.set(false);
      logInfo("Tilemap written into the scene.");
    } else logError(`The entity "${get(tilemapEntity)}" is no longer in the scene.`);
    return;
  }
  if (!data || !fullPath) return;
  try {
    await saveTilemapFile(fullPath, data);
    tilemapDirty.set(false);

    // Sync back to any scene entities whose tilemap.source matches this file
    const relativePath = extractRelativePath(fullPath);
    if (relativePath) {
      const currentScene = get(scene);
      let updated = false;
      for (const entity of currentScene.entities) {
        const tm = entity.components?.tilemap;
        if (tm && tm.source === relativePath) {
          tm.tile_size = data.tile_size;
          tm.layers = structuredClone(data.layers);
          tm.tilesets = structuredClone(data.tilesets);
          updated = true;
        }
      }
      if (updated) {
        scene.set(currentScene);
        dirty.set(true);
        logInfo("Updated scene entities referencing this tilemap.");
      }
    }
    // The terrain rule lives on the entity, not in the file.
    const owner = get(tilemapEntity);
    const had = owner ? (get(scene).entities.find((e) => e.name === owner)?.components as Record<string, any>)?.autotile : undefined;
    if (owner && JSON.stringify(had ?? null) !== JSON.stringify(get(tilemapAutotile))) writeToEntity(data, false);

    logInfo("Tilemap saved.");
  } catch (e) {
    logError(`Failed to save tilemap: ${e}`);
  }
}

/** Extract relative asset path from a full file path (e.g. "tilemaps/overworld.tilemap"). */
function extractRelativePath(fullPath: string): string | null {
  const normalized = fullPath.replace(/\\/g, "/");
  const assetsIdx = normalized.indexOf("/assets/");
  if (assetsIdx >= 0) {
    return normalized.slice(assetsIdx + "/assets/".length);
  }
  return null;
}

export function closeTilemapEditor() {
  tilemapData.set(null);
  tilemapFilePath.set(null);
  tilemapEntity.set(null);
  tilemapInline.set(false);
  tilemapAutotile.set(null);
  tilemapDirty.set(false);
  selectedLayerIndex.set(0);
  selectedTileId.set(null);
  brushMode.set("paint");
  selectedTileIds.set([]);
  undoStack.set([]);
  redoStack.set([]);
}
