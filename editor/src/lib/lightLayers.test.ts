import { describe, expect, it } from "vitest";
import { layerOn, toggleLayer } from "./lightLayers";

describe("light layers", () => {
  it("reads an absent mask as every layer", () => {
    for (let i = 0; i < 8; i++) expect(layerOn(undefined, i)).toBe(true);
    expect(layerOn(1, 0)).toBe(true);
    expect(layerOn(1, 1)).toBe(false);
  });

  it("flips one layer, and is absent again with all eight on", () => {
    const off = toggleLayer(undefined, 1);
    expect(off).toBe(0xfd);
    expect(layerOn(off, 1)).toBe(false);
    expect(toggleLayer(off, 1)).toBeUndefined();
    expect(toggleLayer(1, 0)).toBe(0);
  });
});
