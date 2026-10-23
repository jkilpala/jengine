import { describe, expect, it } from "vitest";
import { clampBorders, readFill, writeFill } from "./spriteFill";

describe("a sprite's fill", () => {
  it("reads every spelling", () => {
    expect(readFill(undefined)).toEqual({ kind: "stretch" });
    expect(readFill("Stretch")).toEqual({ kind: "stretch" });
    expect(readFill({ NineSlice: { left: 4, right: 4, top: 6 } })).toEqual({
      kind: "nine",
      borders: { left: 4, right: 4, top: 6, bottom: 0 },
    });
    expect(readFill({ Tile: { tile_size: [16, 8] } })).toEqual({ kind: "tile", tileSize: [16, 8] });
  });

  it("writes what serde reads, nothing for a stretch", () => {
    expect(writeFill({ kind: "stretch" })).toBeUndefined();
    expect(writeFill({ kind: "nine", borders: { left: 1, right: 2, top: 3, bottom: 4 } })).toEqual({
      NineSlice: { left: 1, right: 2, top: 3, bottom: 4 },
    });
    expect(writeFill({ kind: "tile", tileSize: [8, 8] })).toEqual({ Tile: { tile_size: [8, 8] } });
  });

  it("keeps borders inside the source", () => {
    expect(clampBorders({ left: -2, right: 30, top: 5.4, bottom: 40 }, 24, 32)).toEqual({ left: 0, right: 24, top: 5, bottom: 27 });
  });
});
