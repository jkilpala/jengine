import { describe, expect, it } from "vitest";
import { filterFor, withTextureFilter } from "./textureFilter";

const tf = { default: "linear" as const, overrides: { "sprites/pixel/": "nearest" as const } };

describe("a texture's filter", () => {
  it("is the longest prefix's, else the default", () => {
    expect(filterFor(tf, "sprites/pixel/hero.png")).toEqual({ mode: "nearest", rule: "sprites/pixel/" });
    expect(filterFor(tf, "sprites/bg.png")).toEqual({ mode: "linear", rule: null });
    expect(filterFor(undefined, "x.png")).toEqual({ mode: "nearest", rule: null });
  });

  it("set for one texture, beats its folder's rule", () => {
    const own = withTextureFilter(tf, "sprites/pixel/sky.png", "linear");
    expect(filterFor(own, "sprites/pixel/sky.png")).toEqual({ mode: "linear", rule: "sprites/pixel/sky.png" });
    expect(filterFor(own, "sprites/pixel/hero.png").mode).toBe("nearest");
  });

  it("dropped, leaves the rest - and nothing at all when nothing is left", () => {
    const own = withTextureFilter(tf, "a.png", "nearest");
    expect(withTextureFilter(own, "a.png", null)).toEqual(tf);
    expect(withTextureFilter({ overrides: { "a.png": "linear" } }, "a.png", null)).toBeUndefined();
    expect(tf.overrides).toEqual({ "sprites/pixel/": "nearest" });
  });
});
