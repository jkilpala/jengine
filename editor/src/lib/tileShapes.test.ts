import { describe, expect, it } from "vitest";
import type { TilesetDefData } from "./api/tilemap";
import { SHAPE_PRESETS, presetOf, setPreset } from "./tileShapes";

const tileset = (): TilesetDefData => ({
  texture_id: "t.png",
  tile_width: 32,
  tile_height: 16,
  columns: 4,
  tile_count: 8,
  first_gid: 1,
  image_width: 128,
  image_height: 32,
});

describe("tile shape presets", () => {
  it("round-trips every preset", () => {
    for (const p of SHAPE_PRESETS) {
      const ts = tileset();
      setPreset(ts, 3, p.id);
      expect(presetOf(ts, 3)).toBe(p.id);
      expect(presetOf(ts, 2)).toBe("full");
    }
  });

  it("writes outlines in the tile's pixels, and nothing for a whole tile", () => {
    const ts = tileset();
    setPreset(ts, 1, "slope_up");
    expect(ts.shapes).toEqual([{ tile: 1, collision: [[0, 16], [32, 16], [32, 0]] }]);
    setPreset(ts, 0, "glass");
    expect(ts.shapes?.map((s) => s.tile)).toEqual([0, 1]);
    expect(ts.shapes?.[0]).toEqual({ tile: 0, occluder: [] });
    setPreset(ts, 1, "full");
    setPreset(ts, 0, "full");
    expect(ts.shapes).toBeUndefined();
  });

  it("calls an imported shape custom and keeps it", () => {
    const ts = tileset();
    ts.shapes = [{ tile: 5, collision: [[0, 0], [10, 0], [10, 10]] }];
    expect(presetOf(ts, 5)).toBe("custom");
    setPreset(ts, 2, "none");
    expect(ts.shapes.find((s) => s.tile === 5)?.collision).toEqual([[0, 0], [10, 0], [10, 10]]);
  });
});
