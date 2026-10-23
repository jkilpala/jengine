import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  atlasJson,
  atlasPathFor,
  clipFrom,
  duplicateNames,
  hasContent,
  nameRegions,
  readingOrder,
  rowsOf,
  sliceAuto,
  sliceCount,
  sliceGrid,
} from "./sheetSlicing";

/** A w x h RGBA image with the given pixels opaque. */
function image(w: number, h: number, opaque: [number, number][]): Uint8ClampedArray {
  const px = new Uint8ClampedArray(w * h * 4);
  for (const [x, y] of opaque) px[(y * w + x) * 4 + 3] = 255;
  return px;
}

/** Every pixel of a box. */
function box(x: number, y: number, w: number, h: number): [number, number][] {
  const out: [number, number][] = [];
  for (let j = y; j < y + h; j++) for (let i = x; i < x + w; i++) out.push([i, j]);
  return out;
}

describe("grid slicing", () => {
  it("cuts whole cells row by row, leaving out a partial one", () => {
    const cells = sliceGrid(100, 64, { cellWidth: 32, cellHeight: 32 });
    expect(cells).toHaveLength(6);
    expect(cells[3]).toEqual({ x: 0, y: 32, width: 32, height: 32 });
  });

  it("honours an offset and spacing", () => {
    const cells = sliceGrid(70, 36, { cellWidth: 32, cellHeight: 32, offsetX: 2, offsetY: 2, spacingX: 2 });
    expect(cells.map((c) => [c.x, c.y])).toEqual([[2, 2], [36, 2]]);
  });

  it("cuts by count", () => {
    const cells = sliceCount(128, 64, 4, 2);
    expect(cells).toHaveLength(8);
    expect(cells[5]).toEqual({ x: 32, y: 32, width: 32, height: 32 });
    expect(sliceCount(10, 10, 0, 1)).toEqual([]);
  });

  it("knows an empty cell", () => {
    const px = image(8, 4, [[5, 1]]);
    expect(hasContent(px, 8, { x: 0, y: 0, width: 4, height: 4 })).toBe(false);
    expect(hasContent(px, 8, { x: 4, y: 0, width: 4, height: 4 })).toBe(true);
  });
});

describe("automatic slicing", () => {
  it("finds each island as its bounding box, in reading order", () => {
    const px = image(20, 12, [...box(12, 1, 3, 3), ...box(1, 2, 4, 2), ...box(2, 8, 5, 3)]);
    expect(sliceAuto(px, 20, 12)).toEqual([
      { x: 1, y: 2, width: 4, height: 2 },
      { x: 12, y: 1, width: 3, height: 3 },
      { x: 2, y: 8, width: 5, height: 3 },
    ]);
  });

  it("joins diagonal neighbours and near strokes, drops specks", () => {
    const px = image(16, 8, [[1, 1], [2, 2], [3, 3], ...box(8, 1, 2, 2), ...box(11, 1, 2, 2), [14, 6]]);
    const merged = sliceAuto(px, 16, 8, { gap: 2, minSize: 2 });
    expect(merged).toEqual([
      { x: 1, y: 1, width: 3, height: 3 },
      { x: 8, y: 1, width: 5, height: 2 },
    ]);
  });
});

describe("regions", () => {
  it("orders and rows boxes as a sheet reads", () => {
    const rects = [
      { x: 40, y: 0, width: 10, height: 10 },
      { x: 0, y: 2, width: 10, height: 10 },
      { x: 0, y: 20, width: 10, height: 10 },
    ];
    expect(readingOrder(rects).map((r) => r.x + "," + r.y)).toEqual(["0,2", "40,0", "0,20"]);
    expect(rowsOf(rects)).toEqual([0, 0, 1]);
  });

  it("names regions, finds duplicates and writes the atlas", () => {
    const regions = nameRegions(sliceCount(64, 32, 2, 1), "hero");
    expect(regions.map((r) => r.name)).toEqual(["hero_0", "hero_1"]);
    expect(duplicateNames([...regions, { ...regions[0] }])).toEqual(["hero_0"]);
    const clip = clipFrom("walk", regions, 8);
    expect(atlasJson(64, 32, regions, [clip])).toEqual({
      width: 64,
      height: 32,
      regions: [
        { name: "hero_0", x: 0, y: 0, width: 32, height: 32 },
        { name: "hero_1", x: 32, y: 0, width: 32, height: 32 },
      ],
      animation_clips: [{ name: "walk", fps: 8, looping: true, frames: [{ region_name: "hero_0" }, { region_name: "hero_1" }] }],
    });
    expect(atlasPathFor("assets/art/hero.png")).toBe("assets/art/hero.atlas");
    // Exactly what the engine is tested to read (`sliced_sheets_load_as_atlases`).
    const fixture = JSON.parse(
      readFileSync(new URL("../../../crates/jengine-core/tests/fixtures/sliced_sheet.atlas", import.meta.url), "utf-8"),
    );
    expect(atlasJson(64, 32, regions, [clip])).toEqual(fixture);
  });
});
