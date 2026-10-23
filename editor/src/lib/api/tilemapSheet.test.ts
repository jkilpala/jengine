import { describe, expect, it } from "vitest";
import { tileAtSheetPoint, tileRect, tileUv, type TilesetDefData } from "./tilemap";

// 2 x 2 tiles of 16 px, a 1 px margin and 3 px between them: a 37 px sheet.
// The same numbers as `tileset_uv_skips_margin_and_spacing` in Rust.
const spaced: TilesetDefData = {
  texture_id: "t.png",
  tile_width: 16,
  tile_height: 16,
  columns: 2,
  tile_count: 4,
  first_gid: 1,
  image_width: 37,
  image_height: 37,
  margin: 1,
  spacing: 3,
};

describe("tile sheets with a margin and spacing", () => {
  it("place each tile past them, as the engine does", () => {
    expect(tileRect(spaced, 3)).toEqual({ x: 20, y: 20, w: 16, h: 16 });
    expect(tileUv(spaced, 1)!.map((v) => Math.round(v * 37))).toEqual([20, 1, 36, 17]);
  });

  it("pick the tile under a point, and nothing in a gap", () => {
    expect(tileAtSheetPoint(spaced, 1, 1)).toBe(0);
    expect(tileAtSheetPoint(spaced, 25, 30)).toBe(3);
    expect(tileAtSheetPoint(spaced, 0, 5)).toBeNull(); // the margin
    expect(tileAtSheetPoint(spaced, 18, 5)).toBeNull(); // the spacing
    expect(tileAtSheetPoint(spaced, 36.5, 5)).toBeNull(); // past the last column
  });

  it("read a sheet without either as before", () => {
    const plain = { ...spaced, margin: undefined, spacing: undefined, image_width: 32, image_height: 32 };
    expect(tileRect(plain, 3)).toEqual({ x: 16, y: 16, w: 16, h: 16 });
    expect(tileAtSheetPoint(plain, 31, 0)).toBe(1);
  });
});
