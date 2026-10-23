import { describe, expect, it } from "vitest";
import { isFileTexture, prefabTextureIds } from "./prefabTextures";

describe("prefabTextureIds", () => {
  it("reads a prefab's own textures", () => {
    const p = { name: "Lamp", components: { sprite: { texture_id: "lamp.png" }, point_light: { cookie: "c.png" } } };
    expect(prefabTextureIds(p).sort()).toEqual(["c.png", "lamp.png"]);
  });

  it("walks its parts, and theirs", () => {
    const wagon = {
      name: "Wagon",
      components: { sprite: { texture_id: "bed.png" } },
      children: [
        { name: "Barrel", components: { sprite: { texture_id: "barrel.png" } } },
        { name: "Pole", components: {}, children: [{ name: "Pennant", components: { sprite: { texture_id: "pennant.png" } } }] },
      ],
    };
    expect(prefabTextureIds(wagon).sort()).toEqual(["barrel.png", "bed.png", "pennant.png"]);
  });

  it("takes nothing from nothing", () => {
    expect(prefabTextureIds(null)).toEqual([]);
    expect(prefabTextureIds({ components: { sprite: { texture_id: "" } } })).toEqual([]);
  });
});

describe("isFileTexture", () => {
  it("leaves out the engine's own textures, which are never files", () => {
    expect(isFileTexture("__white")).toBe(false);
    expect(isFileTexture("__screen/cam")).toBe(false);
    expect(isFileTexture("")).toBe(false);
    expect(isFileTexture(undefined)).toBe(false);
    expect(isFileTexture("sprites/hero.png")).toBe(true);
  });

  it("keeps them out of a prefab's list", () => {
    const p = { name: "Box", components: { sprite: { texture_id: "__white" }, lightmap: { texture_id: "glow.png" } } };
    expect(prefabTextureIds(p)).toEqual(["glow.png"]);
  });
});
