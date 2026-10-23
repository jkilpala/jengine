/**
 * A tile's properties in its tileset (Rust `tilemap::TilesetDef`), edited by
 * the tilemap editor's tile panel: its **animation** (frames of other tiles,
 * seconds each), its **random variants** (tiles it is drawn as, by weight),
 * its **data** (any JSON a game reads with `Tilemap.get_tile_data`) and its
 * **spawn** (a prefab it becomes when the scene loads). Each is keyed by the
 * tile's local id; an empty value removes the entry, so a tileset carries
 * only what is set. Also the brushes' cell maths: a rectangle, and a pick
 * from several tiles.
 *
 * Functions change the tileset passed in (the editor hands them the copy
 * `executeTilemap` gives its mutation).
 */
import type { TilesetDefData } from "./api/tilemap";

export interface Frame {
  tile: number;
  duration: number;
}

export type Tileset = TilesetDefData & {
  data?: Record<string, unknown>;
  random?: { tile: number; variants: [number, number][] }[];
  spawns?: { tile: number; prefab: string; keep?: boolean }[];
};

// ─── Animation ───────────────────────────────────────────────────────────

export function animationOf(ts: Tileset, tile: number): { frames: Frame[]; scatter: boolean } {
  const a = ts.animations?.find((x) => x.tile === tile);
  return { frames: a?.frames.map((f) => ({ ...f })) ?? [], scatter: !!a?.scatter };
}

/** Set a tile's frames (none removes its animation). */
export function setAnimation(ts: Tileset, tile: number, frames: Frame[], scatter = false): void {
  const rest = (ts.animations ?? []).filter((a) => a.tile !== tile);
  if (frames.length > 0) rest.push({ tile, frames: frames.map((f) => ({ ...f })), ...(scatter ? { scatter: true } : {}) });
  rest.sort((a, b) => a.tile - b.tile);
  if (rest.length > 0) ts.animations = rest;
  else delete ts.animations;
}

/** Frames for `count` tiles in a row from `first`, `duration` each - the
 *  usual way a sheet lays out an animation. */
export function framesInARow(first: number, count: number, duration: number): Frame[] {
  return Array.from({ length: Math.max(0, count) }, (_, i) => ({ tile: first + i, duration }));
}

// ─── Random variants ────────────────────────────────────────────────────

export function variantsOf(ts: Tileset, tile: number): [number, number][] {
  return ts.random?.find((r) => r.tile === tile)?.variants.map((v) => [v[0], v[1]] as [number, number]) ?? [];
}

/** Set the tiles a tile is drawn as, with weights (none removes it). */
export function setVariants(ts: Tileset, tile: number, variants: [number, number][]): void {
  const rest = (ts.random ?? []).filter((r) => r.tile !== tile);
  if (variants.length > 0) rest.push({ tile, variants: variants.map((v) => [v[0], v[1]] as [number, number]) });
  rest.sort((a, b) => a.tile - b.tile);
  if (rest.length > 0) ts.random = rest;
  else delete ts.random;
}

// ─── Data ───────────────────────────────────────────────────────────────

export function dataOf(ts: Tileset, tile: number): unknown {
  return ts.data?.[String(tile)];
}

/** Set a tile's data (undefined or null removes it). */
export function setData(ts: Tileset, tile: number, value: unknown): void {
  const data = { ...(ts.data ?? {}) };
  if (value === undefined || value === null) delete data[String(tile)];
  else data[String(tile)] = value;
  if (Object.keys(data).length > 0) ts.data = data;
  else delete ts.data;
}

// ─── Spawns ─────────────────────────────────────────────────────────────

export function spawnOf(ts: Tileset, tile: number): { prefab: string; keep: boolean } | null {
  const s = ts.spawns?.find((x) => x.tile === tile);
  return s ? { prefab: s.prefab, keep: !!s.keep } : null;
}

/** Make a tile become `prefab` when the scene loads (an empty prefab
 *  removes it); `keep` leaves the tile drawn under it. */
export function setSpawn(ts: Tileset, tile: number, prefab: string, keep = false): void {
  const rest = (ts.spawns ?? []).filter((s) => s.tile !== tile);
  if (prefab.trim()) rest.push({ tile, prefab: prefab.trim(), ...(keep ? { keep: true } : {}) });
  rest.sort((a, b) => a.tile - b.tile);
  if (rest.length > 0) ts.spawns = rest;
  else delete ts.spawns;
}

// ─── Brushes ────────────────────────────────────────────────────────────

/** Every cell of the rectangle between two corners (either order), clipped
 *  to a `width` x `height` layer. */
export function rectCells(a: [number, number], b: [number, number], width: number, height: number): [number, number][] {
  const out: [number, number][] = [];
  const x0 = Math.max(0, Math.min(a[0], b[0]));
  const x1 = Math.min(width - 1, Math.max(a[0], b[0]));
  const y0 = Math.max(0, Math.min(a[1], b[1]));
  const y1 = Math.min(height - 1, Math.max(a[1], b[1]));
  for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) out.push([x, y]);
  return out;
}

/** The tile a random brush paints: one of `tiles`, chosen by `rand`
 *  (0..1) - so a stroke over several tiles scatters them. */
export function pickTile(tiles: number[], rand: number): number | null {
  if (tiles.length === 0) return null;
  return tiles[Math.min(tiles.length - 1, Math.floor(rand * tiles.length))];
}

/** A block of cells copied from a layer, for the stamp brush. */
export interface Block {
  width: number;
  height: number;
  /** Row by row; `null` is an empty cell, which a stamp leaves alone. */
  tiles: (number | null)[];
}

/** The cells of `layer` in the rectangle between two cells (clamped to
 *  the map), or null when it holds no cell. */
export function copyBlock(
  layer: { width: number; height: number; tiles: (number | null)[] },
  a: [number, number],
  b: [number, number],
): Block | null {
  const cells = rectCells(a, b, layer.width, layer.height);
  if (cells.length === 0) return null;
  const x0 = Math.min(...cells.map((c) => c[0]));
  const y0 = Math.min(...cells.map((c) => c[1]));
  const x1 = Math.max(...cells.map((c) => c[0]));
  const y1 = Math.max(...cells.map((c) => c[1]));
  const width = x1 - x0 + 1;
  const height = y1 - y0 + 1;
  const tiles: (number | null)[] = [];
  for (let y = y0; y <= y1; y++) for (let x = x0; x <= x1; x++) tiles.push(layer.tiles[y * layer.width + x] ?? null);
  return { width, height, tiles };
}

/** Where a stamp of `block` with its top-left at `at` writes: each filled
 *  cell that lands on the map, as `[x, y, tile]`. */
export function stampCells(block: Block, at: [number, number], mapWidth: number, mapHeight: number): [number, number, number][] {
  const out: [number, number, number][] = [];
  for (let y = 0; y < block.height; y++) {
    for (let x = 0; x < block.width; x++) {
      const tile = block.tiles[y * block.width + x];
      const mx = at[0] + x;
      const my = at[1] + y;
      if (tile === null || tile === undefined || mx < 0 || my < 0 || mx >= mapWidth || my >= mapHeight) continue;
      out.push([mx, my, tile]);
    }
  }
  return out;
}
