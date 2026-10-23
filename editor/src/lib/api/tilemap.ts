import { invoke } from "@tauri-apps/api/core";
import { writeTextFile } from "@tauri-apps/plugin-fs";

/** Tileset definition. Matches Rust `TilesetDef`. */
export interface TilesetDefData {
  texture_id: string;
  tile_width: number;
  tile_height: number;
  columns: number;
  tile_count: number;
  first_gid: number;
  image_width: number;
  image_height: number;
  /** Pixels round the sheet's edge (Tiled's `margin`); absent is 0. */
  margin?: number;
  /** Pixels between neighbouring tiles (Tiled's `spacing`); absent is 0. */
  spacing?: number;
  /** Animated tiles. Matches Rust `TileAnimation`. */
  animations?: { tile: number; frames: { tile: number; duration: number }[]; scatter?: boolean }[];
  /** Tiles that are not the whole cell. Matches Rust `TileShape` (see `tileShapes.ts`). */
  shapes?: import("../tileShapes").TileShapeData[];
}

/** A single tile layer. Matches Rust `TilemapLayer`. */
export interface TilemapLayerData {
  name: string;
  /** Flattened tile data — null = empty tile, number = global tile ID. */
  tiles: (number | null)[];
  width: number;
  height: number;
  visible: boolean;
  /** Whether this layer is a collision layer (not rendered in-game). */
  collision?: boolean;
  /** Draw this layer's tiles sorted with y-sorted sprites, by cell. */
  y_sort?: boolean;
  /** Where the layer is drawn, moved this far (world units, y up). */
  offset?: [number, number];
}

/** Complete tilemap data. Matches Rust `Tilemap`. */
export interface TilemapData {
  tile_size: [number, number];
  /** Square cells (the default), diamonds or hexagons. Matches Rust `TileOrientation`. */
  orientation?: "orthogonal" | "isometric" | "hexagonal";
  /** A hexagon's flat-side length (hexagonal maps). */
  hex_side?: number;
  /** "y" (pointy-topped, the default) or "x" (flat-topped). */
  stagger_axis?: "x" | "y";
  /** "odd" (the default) or "even". */
  stagger_index?: "odd" | "even";
  tilesets: TilesetDefData[];
  layers: TilemapLayerData[];
}

/** Create a default empty tilemap. */
export function defaultTilemap(
  width: number = 20,
  height: number = 15,
  tileW: number = 16,
  tileH: number = 16,
): TilemapData {
  return {
    tile_size: [tileW, tileH],
    tilesets: [],
    layers: [
      {
        name: "Ground",
        tiles: new Array(width * height).fill(null),
        width,
        height,
        visible: true,
        collision: false,
      },
    ],
  };
}

/** Resolve a global tile ID to (tileset index, local tile id). */
export function resolveGid(
  tilesets: TilesetDefData[],
  gid: number,
): { tilesetIdx: number; localId: number } | null {
  let best: { tilesetIdx: number; localId: number } | null = null;
  for (let i = 0; i < tilesets.length; i++) {
    const ts = tilesets[i];
    if (gid >= ts.first_gid) {
      const local = gid - ts.first_gid;
      if (local < ts.tile_count) {
        if (!best || local < best.localId) {
          best = { tilesetIdx: i, localId: local };
        }
      }
    }
  }
  return best;
}

/** Compute UV rect [u_min, v_min, u_max, v_max] for a local tile id within a tileset. */
/** Where a tile sits on its sheet, in sheet pixels - past the margin and the
 * spacing between tiles, as `TilesetDef::tile_uv` reads it. */
export function tileRect(
  ts: TilesetDefData,
  localId: number,
): { x: number; y: number; w: number; h: number } {
  const margin = ts.margin ?? 0;
  const spacing = ts.spacing ?? 0;
  const col = localId % ts.columns;
  const row = Math.floor(localId / ts.columns);
  return {
    x: margin + col * (ts.tile_width + spacing),
    y: margin + row * (ts.tile_height + spacing),
    w: ts.tile_width,
    h: ts.tile_height,
  };
}

/** The tile under a point of the sheet (sheet pixels), or null where there is
 * none - outside the sheet, in the margin, or in the spacing between tiles. */
export function tileAtSheetPoint(ts: TilesetDefData, px: number, py: number): number | null {
  if (ts.columns === 0) return null;
  const margin = ts.margin ?? 0;
  const spacing = ts.spacing ?? 0;
  const fx = px - margin;
  const fy = py - margin;
  if (fx < 0 || fy < 0) return null;
  const col = Math.floor(fx / (ts.tile_width + spacing));
  const row = Math.floor(fy / (ts.tile_height + spacing));
  if (fx - col * (ts.tile_width + spacing) >= ts.tile_width) return null;
  if (fy - row * (ts.tile_height + spacing) >= ts.tile_height) return null;
  if (col >= ts.columns) return null;
  const localId = row * ts.columns + col;
  return localId < ts.tile_count ? localId : null;
}

export function tileUv(
  ts: TilesetDefData,
  localId: number,
): [number, number, number, number] | null {
  if (
    localId >= ts.tile_count ||
    ts.columns === 0 ||
    ts.image_width === 0 ||
    ts.image_height === 0
  )
    return null;
  const r = tileRect(ts, localId);
  return [
    r.x / ts.image_width,
    r.y / ts.image_height,
    (r.x + r.w) / ts.image_width,
    (r.y + r.h) / ts.image_height,
  ];
}

/** Read a `.tilemap` file from the project assets. */
export async function readTilemapFile(
  projectPath: string,
  relativePath: string,
): Promise<TilemapData> {
  const bytes = await invoke<number[]>("read_asset_binary", {
    projectPath,
    relativePath,
  });
  const text = new TextDecoder().decode(new Uint8Array(bytes));
  return JSON.parse(text) as TilemapData;
}

/** Save a `.tilemap` file to disk (overwrites existing). */
export async function saveTilemapFile(
  fullPath: string,
  data: TilemapData,
): Promise<void> {
  const json = JSON.stringify(data, null, 2);
  await writeTextFile(fullPath, json);
}

/**
 * Inline every `tilemap.source` `.tilemap` reference in a scene clone so the
 * WASM viewport (which has no filesystem access) sees fully-resolved
 * `tile_size` / `tilesets` / `layers`. The native runtime does the same via
 * `resolve_tilemap_sources` at scene-load time. The `source` field is kept for
 * traceability. Mutates `sceneData` in place; returns how many were resolved.
 */
export async function resolveTilemapSourcesInScene(
  projectPath: string,
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  sceneData: { entities?: any[] },
  onError?: (entityName: string, err: unknown) => void,
): Promise<number> {
  const entities = sceneData.entities ?? [];
  let resolved = 0;
  for (const ent of entities) {
    const tilemap = ent?.components?.tilemap;
    const source: unknown = tilemap?.source;
    if (!tilemap || typeof source !== "string" || source.length === 0) continue;
    try {
      const data = await readTilemapFile(projectPath, source);
      tilemap.tile_size = data.tile_size;
      if (data.orientation) tilemap.orientation = data.orientation;
      tilemap.tilesets = data.tilesets;
      tilemap.layers = data.layers;
      resolved += 1;
    } catch (err) {
      onError?.(ent?.name ?? "<unnamed>", err);
    }
  }
  return resolved;
}
