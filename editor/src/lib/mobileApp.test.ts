import { describe, expect, it } from "vitest";
import { cleanSettings, defaultAppId, defaultOrientation, validateAppId } from "./mobileApp";

// The same cases as `jengine_builder::app`'s tests, so the editor and the
// build agree on what an id may be.
describe("validateAppId", () => {
  it("accepts reverse-domain ids and an empty one", () => {
    expect(validateAppId("com.example.game")).toBeNull();
    expect(validateAppId("io.jengine.Arcade_2")).toBeNull();
    expect(validateAppId("")).toBeNull();
  });

  it("refuses what Android refuses, naming the bad part", () => {
    expect(validateAppId("game")).toMatch(/two dot-separated/);
    expect(validateAppId("com.example.2048")).toMatch(/'2048'/);
    expect(validateAppId("com..game")).toMatch(/''/);
    expect(validateAppId("com.my-game")).toMatch(/'my-game'/);
    expect(validateAppId("com.game.")).toMatch(/''/);
  });
});

describe("defaults", () => {
  it("makes an id out of any project name", () => {
    expect(defaultAppId("example-arcade")).toBe("com.jengine.example_arcade");
    expect(defaultAppId("2048 deluxe!")).toBe("com.jengine.g2048_deluxe_");
    expect(validateAppId(defaultAppId("2048 deluxe!"))).toBeNull();
  });

  it("orients by the project's shape", () => {
    expect(defaultOrientation(1280, 720)).toBe("landscape");
    expect(defaultOrientation(720, 1280)).toBe("portrait");
  });
});

describe("cleanSettings", () => {
  it("drops what a cleared field binds, and trims text", () => {
    expect(
      cleanSettings({ id: " com.a.b ", name: "", build: null, min_sdk: NaN, abis: [], target_sdk: 0 }),
    ).toEqual({ id: "com.a.b" });
  });

  it("returns undefined when nothing is authored", () => {
    expect(cleanSettings({ id: "", version: "  " })).toBeUndefined();
    expect(cleanSettings(undefined)).toBeUndefined();
  });

  it("keeps real values", () => {
    const v = { version: "2.0", build: 7, abis: ["x86_64"], orientation: "any" };
    expect(cleanSettings(v)).toEqual(v);
  });
});
