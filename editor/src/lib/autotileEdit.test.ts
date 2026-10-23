import { describe, expect, it } from "vitest";
import {
  nextTerrainStart,
  renameRuleLayer,
  ruleFor,
  setRule,
  terrainSwatch,
  tilemapFromComponent,
  writeTilemapToComponent,
} from "./autotileEdit";

// Scene 151's rule: two terrains on one 16-wide sheet.
const a = { layers: [{ layer: "ground", terrains: [1, 17] }] };

describe("the autotile rule", () => {
  it("finds a layer's terrains", () => {
    expect(ruleFor(a, "ground")!.terrains).toEqual([1, 17]);
    expect(ruleFor(a, "walls")).toBeUndefined();
    expect(ruleFor(null, "ground")).toBeUndefined();
  });

  it("swatches each terrain by its whole tile", () => {
    expect(terrainSwatch(a.layers[0], 1)).toBe(16);
    expect(terrainSwatch(a.layers[0], 2)).toBe(32);
  });

  it("puts the next terrain after the last", () => {
    expect(nextTerrainStart(a.layers[0], 1)).toBe(33);
    expect(nextTerrainStart(undefined, 5)).toBe(5);
  });

  it("sets, replaces and clears a layer's rule, keeping its place", () => {
    const two = setRule(a, "water", [33]);
    expect(two!.layers.map((l) => l.layer)).toEqual(["ground", "water"]);
    expect(setRule(two, "ground", [1])!.layers.map((l) => [l.layer, l.terrains])).toEqual([
      ["ground", [1]],
      ["water", [33]],
    ]);
    expect(setRule(a, "ground", null)).toBeNull();
    expect(a.layers[0].terrains).toEqual([1, 17]);
  });

  it("follows a renamed layer", () => {
    expect(renameRuleLayer(a, "ground", "land")!.layers[0].layer).toBe("land");
    expect(renameRuleLayer(null, "a", "b")).toBeNull();
  });
});

describe("an inline map", () => {
  const tm = {
    tile_size: [40, 40],
    tilesets: [{ texture_id: "t.png", first_gid: 1 }],
    layers: [{ name: "ground", width: 2, height: 1, tiles: [1, null] }],
    tilemap_extra: { keep: true },
  };

  it("is read as the editor's map", () => {
    const d = tilemapFromComponent(tm);
    expect(d.tile_size).toEqual([40, 40]);
    expect((d as any).tilemap_extra).toBeUndefined();
  });

  it("is written back without losing what the editor does not edit", () => {
    const d = tilemapFromComponent(tm);
    d.layers[0].tiles[1] = 2;
    d.orientation = "isometric";
    const out = writeTilemapToComponent(tm, d) as any;
    expect(out.layers[0].tiles).toEqual([1, 2]);
    expect(out.orientation).toBe("isometric");
    expect(out.tilemap_extra).toEqual({ keep: true });
    expect((tm.layers[0].tiles as unknown[])[1]).toBeNull();
  });
});
