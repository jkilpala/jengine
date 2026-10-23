import { describe, expect, it } from "vitest";
import { isBuiltinFontId, isFontAsset } from "./projectFontsStore";

describe("isFontAsset", () => {
  it("takes .font descriptors and the font files the engine bakes from", () => {
    expect(isFontAsset("fonts/ui.font")).toBe(true);
    expect(isFontAsset("fonts/kotoba.otf")).toBe(true);
    expect(isFontAsset("fonts/noto/NotoSansJP-Regular-subset.ttf")).toBe(true);
    expect(isFontAsset("fonts/LOUD.TTF")).toBe(true);
  });

  it("leaves everything else out, an atlas beside its .font included", () => {
    expect(isFontAsset("fonts/ui.png")).toBe(false);
    expect(isFontAsset("fonts/OFL.txt")).toBe(false);
    expect(isFontAsset("scenes/title.scene")).toBe(false);
  });
});

describe("isBuiltinFontId", () => {
  it("knows both spellings of the built-in face", () => {
    expect(isBuiltinFontId("")).toBe(true);
    expect(isBuiltinFontId("default")).toBe(true);
    expect(isBuiltinFontId("fonts/kotoba.otf")).toBe(false);
  });
});
