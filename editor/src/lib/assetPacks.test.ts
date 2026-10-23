import { describe, expect, it } from "vitest";
import { cleanPacks, includeText, newPackName, packProblem, parseIncludes, validPackName } from "./assetPacks";

describe("asset pack settings", () => {
  it("names follow the engine's rule", () => {
    expect(validPackName("forest-2_b")).toBe(true);
    for (const bad of ["", "Forest", "a/b", "a b", "ä"]) expect(validPackName(bad)).toBe(false);
  });

  it("include paths are one per line, with forward slashes", () => {
    expect(parseIncludes(" sprites/forest/ \r\n\nmusic\\forest.ogg\n")).toEqual(["sprites/forest/", "music/forest.ogg"]);
    expect(includeText(["a/", "b.png"])).toBe("a/\nb.png");
  });

  it("a bad or repeated name is a problem", () => {
    expect(packProblem({ packs: [{ name: "forest", include: [] }] })).toBeNull();
    expect(packProblem({ packs: [{ name: "Forest", include: [] }] })).toContain("not a pack name");
    expect(packProblem({ packs: [{ name: "a", include: [] }, { name: "a", include: [] }] })).toContain("two packs");
    expect(packProblem(undefined)).toBeNull();
  });

  it("an empty setting is dropped on save", () => {
    expect(cleanPacks({ url: " ", packs: [] })).toBeUndefined();
    expect(cleanPacks(undefined)).toBeUndefined();
    expect(cleanPacks({ url: "https://cdn/x/", packs: [{ name: " forest ", include: ["a/", " "] }] })).toEqual({
      url: "https://cdn/x/",
      packs: [{ name: "forest", include: ["a/"] }],
    });
  });

  it("a new pack takes the first free name", () => {
    expect(newPackName(undefined)).toBe("pack");
    expect(newPackName({ packs: [{ name: "pack", include: [] }, { name: "pack-2", include: [] }] })).toBe("pack-3");
  });
});
