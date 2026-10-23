import { describe, expect, it } from "vitest";
import {
  cssGradient,
  formatKeys,
  hexOf,
  insertKey,
  moveKey,
  parseKeys,
  removeKey,
  rgbOf,
  sample,
  sortKeys,
  splitIntensity,
  valueRange,
} from "./curves";

describe("curve keys", () => {
  const curve = [
    [0, 0.5],
    [0.5, 1],
    [1, 1.5],
  ];

  it("samples linearly and holds the ends", () => {
    expect(sample(curve, 0.25, [1])[0]).toBeCloseTo(0.75);
    expect(sample(curve, 0.75, [1])[0]).toBeCloseTo(1.25);
    expect(sample([[0.2, 2], [0.8, 4]], 0, [1])).toEqual([2]);
    expect(sample([[0.2, 2], [0.8, 4]], 1, [1])).toEqual([4]);
    expect(sample([], 0.5, [1])).toEqual([1]);
  });

  it("samples unsorted keys as if sorted", () => {
    expect(sample([[1, 2], [0, 0]], 0.5, [1])[0]).toBeCloseTo(1);
  });

  it("inserts a key on the curve without changing it", () => {
    const { keys, index } = insertKey(curve, 0.25, [1]);
    expect(index).toBe(1);
    expect(keys[1]).toEqual([0.25, 0.75]);
    for (const t of [0, 0.1, 0.25, 0.4, 0.9]) {
      expect(sample(keys, t, [1])[0]).toBeCloseTo(sample(curve, t, [1])[0]);
    }
  });

  it("inserts a key with given values, clamping t, into an empty curve", () => {
    expect(insertKey([], 1.4, [1], [3])).toEqual({ keys: [[1, 3]], index: 0 });
    expect(insertKey([], 0.5, [1]).keys).toEqual([[0.5, 1]]);
  });

  it("removes a key", () => {
    expect(removeKey(curve, 1)).toEqual([
      [0, 0.5],
      [1, 1.5],
    ]);
    expect(curve).toHaveLength(3);
  });

  it("moves a key past its neighbour and reports where it went", () => {
    const { keys, index } = moveKey(curve, 0, 0.75, [2]);
    expect(keys.map((k) => k[0])).toEqual([0.5, 0.75, 1]);
    expect(index).toBe(1);
    expect(keys[index]).toEqual([0.75, 2]);
    const clamped = moveKey(curve, 2, -3);
    expect(clamped.keys[clamped.index]).toEqual([0, 1.5]);
  });

  it("keeps equal times in their order when sorting", () => {
    expect(sortKeys([[0.5, 1], [0.5, 2], [0, 0]])).toEqual([[0, 0], [0.5, 1], [0.5, 2]]);
  });

  it("parses and formats the text fallback", () => {
    expect(parseKeys("", 2)).toEqual({ keys: [] });
    expect(parseKeys("[[1, 2], [0, 1]]", 2)).toEqual({ keys: [[0, 1], [1, 2]] });
    expect(parseKeys("[[0, 1, 1]]", 2)).toHaveProperty("error");
    expect(parseKeys("[[0, \"a\"]]", 2)).toHaveProperty("error");
    expect(parseKeys("not json", 2)).toHaveProperty("error");
    const text = formatKeys(curve);
    expect(parseKeys(text, 2)).toEqual({ keys: curve });
    const long = [[0, 1], [0.25, 1], [0.5, 1], [1, 1]];
    expect(formatKeys(long)).toContain("\n");
    expect(parseKeys(formatKeys(long), 2)).toEqual({ keys: long });
    expect(formatKeys([])).toBe("");
  });

  it("widens the shown range to the keys", () => {
    expect(valueRange([[0, 0.5]])).toEqual({ min: 0, max: 1 });
    expect(valueRange([[0, 2]]).max).toBeGreaterThan(2);
    expect(valueRange([[0, -1]]).min).toBeLessThan(-1);
  });
});

describe("gradient colours", () => {
  it("round-trips the picker through sRGB", () => {
    const rgb = [0.2, 0.5, 0.9];
    const back = rgbOf(hexOf(rgb));
    back.forEach((c, i) => expect(c).toBeCloseTo(rgb[i], 2));
    expect(hexOf([1, 1, 1])).toBe("#ffffff");
    expect(hexOf([0, 0, 0])).toBe("#000000");
  });

  it("splits an HDR colour into a pickable colour and an intensity", () => {
    expect(splitIntensity([0.5, 0.2, 1])).toEqual({ rgb: [0.5, 0.2, 1], intensity: 1 });
    const { rgb, intensity } = splitIntensity([4, 2, 1]);
    expect(intensity).toBe(4);
    expect(rgb).toEqual([1, 0.5, 0.25]);
  });

  it("draws a gradient as a CSS gradient from its first to its last colour", () => {
    const css = cssGradient([
      [0, 1, 0, 0, 1],
      [1, 0, 0, 1, 0],
    ]);
    expect(css.startsWith("linear-gradient(to right, rgba(255, 0, 0, 1.000) 0.0%")).toBe(true);
    expect(css.endsWith("rgba(0, 0, 255, 0.000) 100.0%)")).toBe(true);
  });
});
