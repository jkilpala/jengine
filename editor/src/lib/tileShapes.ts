/**
 * Per-tile shapes (`TilesetDef.shapes`, Rust `tilemap::TileShape`): what part
 * of a tile is solid and what part casts a shadow. The tilemap editor offers
 * the common ones as presets; a shape typed in by hand, or imported from
 * Tiled, shows as "custom" and is left alone.
 *
 * Points are in the tile's pixels, (0, 0) its top-left, y down.
 */
import type { TilesetDefData } from "./api/tilemap";

export type Points = [number, number][];

/** Matches Rust `TileShape`. Absent `collision`: the whole cell; empty:
 *  nothing. Absent `occluder`: the collision outline; empty: no shadow. */
export interface TileShapeData {
  tile: number;
  collision?: Points;
  occluder?: Points;
}

export type ShapePreset =
  | "full"
  | "none"
  | "slope_up"
  | "slope_down"
  | "half_bottom"
  | "half_top"
  | "glass"
  | "custom";

export const SHAPE_PRESETS: { id: Exclude<ShapePreset, "custom">; label: string }[] = [
  { id: "full", label: "Whole tile" },
  { id: "none", label: "Not solid" },
  { id: "slope_up", label: "Slope rising right" },
  { id: "slope_down", label: "Slope rising left" },
  { id: "half_bottom", label: "Bottom half" },
  { id: "half_top", label: "Top half" },
  { id: "glass", label: "Solid, no shadow" },
];

/** The outline a preset gives a `w` x `h` pixel tile. */
function outline(preset: ShapePreset, w: number, h: number): Points | undefined {
  switch (preset) {
    case "slope_up":
      return [[0, h], [w, h], [w, 0]];
    case "slope_down":
      return [[0, 0], [w, h], [0, h]];
    case "half_bottom":
      return [[0, h / 2], [w, h / 2], [w, h], [0, h]];
    case "half_top":
      return [[0, 0], [w, 0], [w, h / 2], [0, h / 2]];
    default:
      return undefined;
  }
}

/** The shape entry a preset makes for tile `tile`, or null for "full" (a
 *  tile with no entry is the whole cell). */
export function shapeForPreset(
  preset: Exclude<ShapePreset, "custom">,
  tile: number,
  w: number,
  h: number,
): TileShapeData | null {
  switch (preset) {
    case "full":
      return null;
    case "none":
      return { tile, collision: [] };
    case "glass":
      return { tile, occluder: [] };
    default:
      return { tile, collision: outline(preset, w, h) };
  }
}

const same = (a: Points | undefined, b: Points | undefined) =>
  JSON.stringify(a ?? null) === JSON.stringify(b ?? null);

/** Which preset tile `tile` of `ts` has, "custom" when none matches. */
export function presetOf(ts: TilesetDefData, tile: number): ShapePreset {
  const s = ts.shapes?.find((x) => x.tile === tile);
  if (!s) return "full";
  for (const p of SHAPE_PRESETS) {
    const want = shapeForPreset(p.id, tile, ts.tile_width, ts.tile_height);
    if (want && same(want.collision, s.collision) && same(want.occluder, s.occluder)) return p.id;
  }
  return "custom";
}

/** Give tile `tile` of `ts` a preset, in place. */
export function setPreset(ts: TilesetDefData, tile: number, preset: Exclude<ShapePreset, "custom">): void {
  const rest = (ts.shapes ?? []).filter((x) => x.tile !== tile);
  const made = shapeForPreset(preset, tile, ts.tile_width, ts.tile_height);
  const next = made ? [...rest, made].sort((a, b) => a.tile - b.tile) : rest;
  if (next.length > 0) ts.shapes = next;
  else delete ts.shapes;
}
