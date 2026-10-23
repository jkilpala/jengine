import { describe, expect, it } from "vitest";
import {
  PP_EFFECTS,
  defaultEffects,
  effectsFor,
  getColor,
  getParam,
  hexToLinear,
  linearToHex,
  withEffects,
  withHdr,
  withParam,
} from "./postProcess";
import type { PostProcessSettings } from "./api/scene";

const hdrScene: PostProcessSettings = {
  hdr: true,
  tonemap: "agx",
  exposure: 1.3,
  effects: [{ name: "bloom", enabled: true, params: { threshold: { Float: 1.2 } } }],
};

describe("parameter names", () => {
  it("are the renderer's, never the old u_-prefixed ones", () => {
    for (const spec of PP_EFFECTS) {
      for (const p of spec.params) expect(p.key.startsWith("u_")).toBe(false);
      if (spec.color) expect(spec.color.key.startsWith("u_")).toBe(false);
    }
  });

  it("default to values that leave the image alone", () => {
    const grade = defaultEffects().find((e) => e.name === "color_grade")!;
    // Brightness is a multiplier: 0 would draw black.
    expect(getParam(grade, "brightness")).toBe(1);
    expect(getParam(grade, "contrast")).toBe(1);
    expect(getColor(grade, "tint_color")[3]).toBe(0);
  });
});

describe("editing", () => {
  it("keeps hdr, tonemap and exposure when an effect changes", () => {
    const effects = withParam(effectsFor(hdrScene), 0, "intensity", { Float: 2 });
    const next = withEffects(hdrScene, effects);
    expect(next.hdr).toBe(true);
    expect(next.tonemap).toBe("agx");
    expect(next.exposure).toBe(1.3);
    expect(getParam(next.effects[0], "intensity")).toBe(2);
    expect(getParam(next.effects[0], "threshold")).toBe(1.2);
  });

  it("lists every known effect, the scene's own first", () => {
    const rows = effectsFor(hdrScene);
    expect(rows[0].name).toBe("bloom");
    expect(rows.map((r) => r.name).sort()).toEqual(PP_EFFECTS.map((e) => e.name).sort());
    expect(rows.filter((r) => r.name !== "bloom").every((r) => !r.enabled)).toBe(true);
  });

  it("turning HDR off saves the scene as LDR, with no stray tonemap", () => {
    const off = withHdr(hdrScene, { hdr: false });
    expect(off.hdr).toBeUndefined();
    expect(off.tonemap).toBeUndefined();
    expect(off.exposure).toBeUndefined();
    expect(off.effects).toHaveLength(1);
  });

  it("turning HDR on defaults to ACES at exposure 1", () => {
    const on = withHdr({ effects: [] }, { hdr: true });
    expect(on).toMatchObject({ hdr: true, tonemap: "aces", exposure: 1 });
  });
});

describe("colour pickers", () => {
  it("show linear colour as the sRGB a picker means", () => {
    // sRGB 128 is linear 0.2158.
    expect(linearToHex([0.2158, 0.2158, 0.2158, 1])).toBe("#808080");
    const back = hexToLinear("#808080", 0.5);
    expect(back[0]).toBeCloseTo(0.2158, 3);
    expect(back[3]).toBe(0.5);
  });
});
