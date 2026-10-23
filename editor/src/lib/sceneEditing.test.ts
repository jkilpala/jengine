import { describe, expect, it } from "vitest";
import {
  carryEdit,
  mixedFields,
  entitiesInBox,
  parseSnap,
  snapAngle,
  snapping,
  snapTo,
  topLevelSelection,
  DEFAULT_SNAP,
} from "./sceneEditing";

describe("snapping", () => {
  it("snaps to steps, and Ctrl turns it over", () => {
    expect(snapTo(23, 16)).toBe(16);
    expect(snapTo(25, 16)).toBe(32);
    expect(snapTo(-9, 16)).toBe(-16);
    expect(snapTo(0.34, 0.1)).toBe(0.3);
    expect(snapTo(7, 0)).toBe(7);
    expect(snapAngle((40 * Math.PI) / 180, 15)).toBeCloseTo((45 * Math.PI) / 180);
    expect(snapping({ ...DEFAULT_SNAP, enabled: false }, true)).toBe(true);
    expect(snapping({ ...DEFAULT_SNAP, enabled: true }, true)).toBe(false);
    expect(snapping({ ...DEFAULT_SNAP, enabled: true }, false)).toBe(true);
  });

  it("reads stored settings defensively", () => {
    expect(parseSnap(null)).toEqual(DEFAULT_SNAP);
    expect(parseSnap({ enabled: true, grid: 8, angle: -3, scale: "x" })).toEqual({ ...DEFAULT_SNAP, enabled: true, grid: 8 });
  });
});

describe("box select", () => {
  it("takes the entities whose centre is inside, dragged either way", () => {
    const b = [
      { index: 0, x: 0, y: 0, width: 500, height: 500, rotation: 0 },
      { index: 1, x: 50, y: 50, width: 10, height: 10, rotation: 0 },
      { index: 2, x: 200, y: 0, width: 10, height: 10, rotation: 0 },
    ];
    expect(entitiesInBox(b, [100, 100], [-10, -10])).toEqual([0, 1]);
    expect(entitiesInBox(b, [40, 40], [60, 60])).toEqual([1]);
  });
});

describe("moving a group", () => {
  it("leaves out what moves with a selected parent", () => {
    // 0 <- 1 <- 2, and 3 alone.
    const parents = [-1, 0, 1, -1];
    expect(topLevelSelection([2, 1, 3], (i) => parents[i])).toEqual([1, 3]);
    expect(topLevelSelection([2, 0], (i) => parents[i])).toEqual([0]);
    expect(topLevelSelection([2], (i) => parents[i])).toEqual([2]);
  });
});

describe("editing several entities", () => {
  it("writes only the fields the edit changed, where the other has them", () => {
    const before = { sprite: { color: [1, 1, 1, 1], z_order: 2 }, transform: { position: [0, 0] } };
    const after = { sprite: { color: [1, 0, 0, 1], z_order: 2 }, transform: { position: [0, 0] } };
    const other: Record<string, unknown> = { sprite: { color: [0, 1, 0, 1], z_order: 9 }, transform: { position: [5, 5] } };
    carryEdit(other, before, after);
    expect(other).toEqual({ sprite: { color: [1, 0, 0, 1], z_order: 9 }, transform: { position: [5, 5] } });
    // An entity without the component is not given it by a field edit.
    const bare: Record<string, unknown> = { transform: { position: [1, 1] } };
    carryEdit(bare, before, after);
    expect(bare).toEqual({ transform: { position: [1, 1] } });
  });

  it("changes one coordinate without copying the other", () => {
    const other: Record<string, unknown> = { transform: { position: [5, 7], rotation: 0 } };
    carryEdit(other, { transform: { position: [0, 0] } }, { transform: { position: [10, 0] } });
    expect(other).toEqual({ transform: { position: [10, 7], rotation: 0 } });
    // An alpha alone keeps each entity's own colour.
    const tinted: Record<string, unknown> = { sprite: { color: [0, 1, 0, 1] } };
    carryEdit(tinted, { sprite: { color: [1, 1, 1, 1] } }, { sprite: { color: [1, 1, 1, 0.5] } });
    expect(tinted).toEqual({ sprite: { color: [0, 1, 0, 0.5] } });
  });

  it("adds, removes and unsets as the edit did", () => {
    const other: Record<string, unknown> = { sprite: { color: [1, 1, 1, 1], region: "a" }, tags: ["x"] };
    carryEdit(other, { sprite: { region: "b" }, tags: ["y"] }, { sprite: {}, trail: { width: 4 } });
    expect(other).toEqual({ sprite: { color: [1, 1, 1, 1] }, trail: { width: 4 } });
  });
});

describe("mixed values across a selection", () => {
  it("lists the fields that differ, a missing field among them", () => {
    const ents = [
      { components: { sprite: { color: [1, 1, 1, 1], z_order: 2, size: [8, 8] } } },
      { components: { sprite: { color: [1, 0, 0, 1], z_order: 2 } } },
      { components: {} },
    ];
    expect(mixedFields(ents, "sprite")).toEqual(["color", "size"]);
  });

  it("is nothing with one entity having the component", () => {
    expect(mixedFields([{ components: { sprite: { a: 1 } } }, { components: {} }], "sprite")).toEqual([]);
  });
});
