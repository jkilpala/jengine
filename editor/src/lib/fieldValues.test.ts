import { describe, expect, it } from "vitest";
import { assetsWithExtensions, jsonText, parseJsonText } from "./fieldValues";

describe("json fields", () => {
  it("shows a short value on one line and a long one indented", () => {
    expect(jsonText(undefined)).toBe("");
    expect(jsonText(null)).toBe("null");
    expect(jsonText({ a: 1 })).toBe('{"a":1}');
    const long = { clips: { walk: { tracks: [[0, 1], [1, 2]] } }, playing: true };
    expect(jsonText(long)).toContain("\n");
    expect(parseJsonText(jsonText(long))).toEqual({ value: long });
  });

  it("unsets on empty text and reports bad JSON", () => {
    expect(parseJsonText("  ")).toEqual({ value: undefined });
    expect(parseJsonText("[1, 2]")).toEqual({ value: [1, 2] });
    expect(parseJsonText("{oops")).toHaveProperty("error");
  });
});

describe("asset fields", () => {
  it("offers the project's files of the field's types", () => {
    const files = ["sprites\\hero.PNG", "sounds/hit.wav", "sprites/a.jpg", "x.png.bak", "noext"];
    expect(assetsWithExtensions(files, ["png", "jpg"])).toEqual(["sprites/a.jpg", "sprites/hero.PNG"]);
    expect(assetsWithExtensions(files, [".wav"])).toEqual(["sounds/hit.wav"]);
    expect(assetsWithExtensions(files, [])).toEqual([]);
  });
});
