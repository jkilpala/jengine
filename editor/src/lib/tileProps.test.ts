import { describe, expect, it } from "vitest";
import {
  animationOf,
  copyBlock,
  stampCells,
  dataOf,
  framesInARow,
  pickTile,
  rectCells,
  setAnimation,
  setData,
  setSpawn,
  setVariants,
  spawnOf,
  variantsOf,
  type Tileset,
} from "./tileProps";

const tileset = (): Tileset => ({
  texture_id: "t.png",
  tile_width: 16,
  tile_height: 16,
  columns: 8,
  tile_count: 64,
  first_gid: 1,
  image_width: 128,
  image_height: 128,
});

describe("tile properties", () => {
  it("sets and clears an animation, keeping the list sorted", () => {
    const ts = tileset();
    setAnimation(ts, 5, framesInARow(5, 3, 0.2));
    setAnimation(ts, 2, [{ tile: 2, duration: 1 }], true);
    expect(ts.animations?.map((a) => a.tile)).toEqual([2, 5]);
    expect(animationOf(ts, 5).frames.map((f) => f.tile)).toEqual([5, 6, 7]);
    expect(animationOf(ts, 2).scatter).toBe(true);
    setAnimation(ts, 5, []);
    setAnimation(ts, 2, []);
    expect(ts.animations).toBeUndefined();
    expect(animationOf(ts, 9)).toEqual({ frames: [], scatter: false });
  });

  it("sets variants, data and spawns, and leaves nothing behind when cleared", () => {
    const ts = tileset();
    setVariants(ts, 3, [[3, 4], [11, 1]]);
    expect(variantsOf(ts, 3)).toEqual([[3, 4], [11, 1]]);
    setData(ts, 3, { kind: "lava", damage: 5 });
    expect(dataOf(ts, 3)).toEqual({ kind: "lava", damage: 5 });
    setSpawn(ts, 7, "prefabs/chest.prefab", true);
    expect(spawnOf(ts, 7)).toEqual({ prefab: "prefabs/chest.prefab", keep: true });
    setVariants(ts, 3, []);
    setData(ts, 3, null);
    setSpawn(ts, 7, " ");
    expect(ts).toEqual(tileset());
  });
});

describe("brushes", () => {
  it("covers a rectangle dragged either way, clipped to the layer", () => {
    expect(rectCells([2, 1], [0, 0], 10, 10)).toEqual([[0, 0], [1, 0], [2, 0], [0, 1], [1, 1], [2, 1]]);
    expect(rectCells([-2, -2], [0, 0], 1, 1)).toEqual([[0, 0]]);
  });

  it("picks among several tiles", () => {
    expect(pickTile([4, 5, 6], 0)).toBe(4);
    expect(pickTile([4, 5, 6], 0.99)).toBe(6);
    expect(pickTile([4, 5, 6], 1)).toBe(6);
    expect(pickTile([], 0.5)).toBeNull();
  });
});

describe("the stamp brush", () => {
  // 3 x 2:  1 2 .
  //         4 . 6
  const layer = { width: 3, height: 2, tiles: [1, 2, null, 4, null, 6] };

  it("copies the block between two cells, in either order, clamped to the map", () => {
    expect(copyBlock(layer, [2, 1], [0, 0])).toEqual({ width: 3, height: 2, tiles: [1, 2, null, 4, null, 6] });
    expect(copyBlock(layer, [1, 0], [5, 5])).toEqual({ width: 2, height: 2, tiles: [2, null, null, 6] });
    expect(copyBlock(layer, [7, 7], [9, 9])).toBeNull();
  });

  it("stamps the filled cells where they land on the map, empty ones left alone", () => {
    const block = copyBlock(layer, [0, 0], [1, 1])!; // 1 2 / 4 .
    expect(stampCells(block, [1, 0], 3, 2)).toEqual([
      [1, 0, 1],
      [2, 0, 2],
      [1, 1, 4],
    ]);
    // Off the right edge: only what lands.
    expect(stampCells(block, [2, 1], 3, 2)).toEqual([[2, 1, 1]]);
  });
});
