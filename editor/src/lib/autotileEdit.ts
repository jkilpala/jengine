/**
 * Terrain painting in the tilemap editor (Rust `tilemap::autotile`): which
 * layers of a map are painted in **terrain numbers** is said by the
 * `autotile` component on the map's entity - each such layer's terrains, by
 * the first gid of each one's sixteen tiles (in corner-mask order: bit 1 the
 * top-left corner, 2 top-right, 4 bottom-left, 8 bottom-right, so a
 * terrain's whole tile is `first + 15`). The editor reads and writes that
 * component beside the map, and the preview draws the dual grid through the
 * engine itself (`viewport_tilemap_preview_set_autotile`).
 *
 * Also: a map held inline on an entity's `tilemap` component, edited as a
 * `TilemapData` and written back without losing the fields the editor does
 * not know.
 *
 * Every function returns new data; none mutates its argument.
 */
import type { TilemapData } from "./api/tilemap";

export interface AutotileLayerData {
  layer: string;
  terrains: number[];
}
export interface AutotileData {
  layers: AutotileLayerData[];
}

/** The terrain rule for layer `name`, if it is painted in terrains. */
export function ruleFor(a: AutotileData | null | undefined, name: string): AutotileLayerData | undefined {
  return a?.layers.find((l) => l.layer === name);
}

/** The component with layer `name`'s terrains set (`null` makes it a plain
 *  layer again); `null` when no layer is left autotiled. */
export function setRule(a: AutotileData | null | undefined, name: string, terrains: number[] | null): AutotileData | null {
  const layers = (a?.layers ?? []).filter((l) => l.layer !== name).map((l) => ({ ...l, terrains: [...l.terrains] }));
  if (terrains) {
    const at = (a?.layers ?? []).findIndex((l) => l.layer === name);
    const rule = { layer: name, terrains: [...terrains] };
    if (at >= 0) layers.splice(at, 0, rule);
    else layers.push(rule);
  }
  return layers.length > 0 ? { layers } : null;
}

/** A layer renamed: its rule follows it. */
export function renameRuleLayer(a: AutotileData | null | undefined, from: string, to: string): AutotileData | null {
  if (!a) return null;
  return { layers: a.layers.map((l) => (l.layer === from ? { ...l, layer: to } : { ...l, terrains: [...l.terrains] })) };
}

/** The gid of terrain `n`'s (1-based) whole tile - every corner its own -
 *  what a palette swatch shows. */
export function terrainSwatch(rule: AutotileLayerData, n: number): number {
  return rule.terrains[n - 1] + 15;
}

/** Where the next terrain's sixteen tiles would start: after the last
 *  terrain's, or the map's first tile. */
export function nextTerrainStart(rule: AutotileLayerData | undefined, firstGid: number): number {
  const t = rule?.terrains ?? [];
  return t.length > 0 ? t[t.length - 1] + 16 : firstGid;
}

/** The fields of a `tilemap` component the editor edits as a map. */
const MAP_FIELDS = ["tile_size", "orientation", "hex_side", "stagger_axis", "stagger_index", "tilesets", "layers"] as const;

/** An entity's inline `tilemap` component as the editor's map data. */
export function tilemapFromComponent(tm: Record<string, unknown>): TilemapData {
  const out: Record<string, unknown> = {};
  for (const k of MAP_FIELDS) if (tm[k] !== undefined) out[k] = structuredClone(tm[k]);
  out.tile_size ??= [16, 16];
  out.tilesets ??= [];
  out.layers ??= [];
  return out as unknown as TilemapData;
}

/** The component with the edited map written into it; every other field
 *  (its `source`, colliders' settings, anything newer) kept as it was. */
export function writeTilemapToComponent(tm: Record<string, unknown>, data: TilemapData): Record<string, unknown> {
  const out: Record<string, unknown> = structuredClone(tm);
  const d = data as unknown as Record<string, unknown>;
  for (const k of MAP_FIELDS) {
    if (d[k] === undefined) delete out[k];
    else out[k] = structuredClone(d[k]);
  }
  return out;
}
