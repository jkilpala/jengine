import { describe, expect, it } from "vitest";
import { spriteLibraryTextures } from "./spriteLibrary";

describe("spriteLibraryTextures", () => {
  it("lists every part's texture", () => {
    const comps = {
      sprite_library: {
        categories: {
          hat: { summer: { texture_id: "dolls/hat_summer.png" }, winter: { texture_id: "dolls/hat_winter.png" } },
          coat: { rain: { texture_id: "dolls/coat.atlas", region: "rain" } },
        },
      },
    };
    expect(spriteLibraryTextures(comps).sort()).toEqual(["dolls/coat.atlas", "dolls/hat_summer.png", "dolls/hat_winter.png"]);
  });

  it("is empty without a library, and skips malformed entries", () => {
    expect(spriteLibraryTextures({ sprite: { texture_id: "x.png" } })).toEqual([]);
    expect(spriteLibraryTextures(null)).toEqual([]);
    expect(spriteLibraryTextures({ sprite_library: { categories: { hat: { a: { texture_id: "" }, b: null } } } })).toEqual([]);
  });
});
