import { describe, expect, it } from "vitest";
import { cleanLiveOps, liveOpsProblem, newKey, parseValue, renameKey, validKey, valueText } from "./liveOps";

describe("live ops settings", () => {
  it("reads a default as JSON when it is, else as text", () => {
    expect(parseValue("10")).toBe(10);
    expect(parseValue("true")).toBe(true);
    expect(parseValue('{"a": [1, 2]}')).toEqual({ a: [1, 2] });
    expect(parseValue("Hello there")).toBe("Hello there");
    expect(parseValue('"10"')).toBe("10");
    expect(valueText("Hello")).toBe("Hello");
    expect(valueText(10)).toBe("10");
    expect(valueText({ a: 1 })).toBe('{"a":1}');
  });

  it("says what would not work", () => {
    expect(liveOpsProblem(undefined)).toBeNull();
    expect(liveOpsProblem({ defaults: { coins: 1 } })).toBeNull();
    expect(liveOpsProblem({ url: "ops.example.com", public_key: "jlo-public-aa" })).toContain("https://");
    expect(liveOpsProblem({ url: "https://ops.example.com" })).toContain("public key");
    expect(liveOpsProblem({ url: "http://127.0.0.1:9001", public_key: "jlo-public-aa" })).toBeNull();
    expect(liveOpsProblem({ defaults: { "a b": 1 } })).toContain("not a key");
    expect(validKey("shop.layout-v2_x")).toBe(true);
  });

  it("saves only what is set", () => {
    expect(cleanLiveOps({ url: " ", channel: "", defaults: {} })).toBeUndefined();
    expect(cleanLiveOps({ url: " https://x ", channel: "live", defaults: { a: 1, "": 2 } })).toEqual({
      url: "https://x",
      channel: "live",
      defaults: { a: 1 },
    });
    // What the page does not edit yet survives a save.
    expect(cleanLiveOps({ region: " FI ", attributes: { store: "steam", "": 1 } })).toEqual({ region: "FI", attributes: { store: "steam" } });
  });

  it("names and renames keys without losing their place", () => {
    expect(newKey(undefined)).toBe("key");
    expect(newKey({ defaults: { key: 1, key_2: 2 } })).toBe("key_3");
    expect(Object.keys(renameKey({ a: 1, b: 2, c: 3 }, "b", "z"))).toEqual(["a", "z", "c"]);
    expect(renameKey({ a: 1, b: 2 }, "a", "b")).toEqual({ a: 1, b: 2 });
  });
});
