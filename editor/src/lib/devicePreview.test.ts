import { describe, expect, it } from "vitest";
import {
  DEVICE_PROFILES,
  cutoutShapes,
  fitRect,
  profileById,
  safeFractions,
  safeRect,
  screenAspect,
} from "./devicePreview";

const hole = profileById("phone-hole")!;
const notch = profileById("phone-notch")!;

describe("fitRect", () => {
  it("letterboxes a wide screen into a squarer panel, centred", () => {
    const r = fitRect(1000, 800, 20 / 9);
    expect(r.width).toBe(1000);
    expect(r.height).toBe(450);
    expect(r.y).toBe(175);
  });

  it("pillarboxes into a wider panel", () => {
    const r = fitRect(2000, 600, 16 / 9);
    expect(r.height).toBe(600);
    expect(r.width).toBe(1066);
    expect(r.x).toBe(467);
  });
});

describe("safe area", () => {
  it("matches what Android reports for a punch-hole phone", () => {
    // 132 px on 2400 x 1080, the number the emulator's cut-out reported.
    const r = safeRect(hole, false, 2400, 1080);
    expect(r.x).toBeCloseTo(0.122 * 1080, 0);
    expect(r.y).toBe(0);
    expect(r.width).toBeCloseTo(2400 - 0.122 * 1080, 0);
  });

  it("keeps both sides and the home bar for a notch phone", () => {
    const f = safeFractions(notch, false);
    expect(f.left).toBeGreaterThan(0);
    expect(f.right).toBeCloseTo(f.left);
    expect(f.bottom).toBeCloseTo(0.056);
  });

  it("turns with the phone: the camera edge is the top in portrait", () => {
    const f = safeFractions(hole, true);
    expect(f.top).toBeGreaterThan(0);
    expect(f.left).toBe(0);
    expect(screenAspect(hole, true)).toBeCloseTo(9 / 20);
  });

  it("stays well inside the screen for every profile", () => {
    for (const p of DEVICE_PROFILES) {
      for (const portrait of [false, true]) {
        const f = safeFractions(p, portrait);
        for (const v of [f.left, f.top, f.right, f.bottom]) {
          expect(v).toBeGreaterThanOrEqual(0);
          expect(v).toBeLessThan(0.45); // what the viewport accepts
        }
      }
    }
  });
});

describe("cut-out shapes", () => {
  it("puts the camera on the left in landscape and on top in portrait", () => {
    const [l] = cutoutShapes(hole, false, 2400, 1080);
    expect(l.x).toBeLessThan(100);
    expect(l.y + l.height / 2).toBeCloseTo(540);
    const [p] = cutoutShapes(hole, true, 1080, 2400);
    expect(p.y).toBeLessThan(100);
    expect(p.x + p.width / 2).toBeCloseTo(540);
  });

  it("lies inside the safe inset it causes", () => {
    for (const p of DEVICE_PROFILES) {
      const shapes = cutoutShapes(p, false, 2400, 1080);
      const safe = safeRect(p, false, 2400, 1080);
      for (const s of shapes) expect(s.x + s.width).toBeLessThanOrEqual(safe.x + 0.5);
    }
  });

  it("draws nothing for a phone without one", () => {
    expect(cutoutShapes(profileById("phone-plain")!, false, 1920, 1080)).toEqual([]);
  });
});
