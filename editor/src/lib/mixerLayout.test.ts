import { describe, expect, it } from "vitest";
import {
  addBus,
  depthOf,
  effectOf,
  isUnder,
  parentsFirst,
  removeBus,
  renameBus,
  shownBuses,
  toLayout,
  withEffect,
  type Bus,
} from "./mixerLayout";

describe("the project mixer", () => {
  it("always shows the three built-in buses, parents first", () => {
    const shown = shownBuses({ buses: [{ name: "ambience", parent: "music", volume: 0.5 }] });
    expect(shown.map((b) => b.name)).toEqual(["master", "music", "ambience", "sfx"]);
    expect(depthOf(shown, "ambience")).toBe(2);
  });

  it("writes back only what differs from the engine's own buses", () => {
    expect(toLayout(shownBuses(undefined))).toEqual({ buses: [] });
    const quiet = shownBuses(undefined).map((b) => (b.name === "music" ? { ...b, volume: 0.4 } : b));
    expect(toLayout(quiet)).toEqual({ buses: [{ name: "music", parent: "master", volume: 0.4 }] });
  });

  it("orders a child after a parent listed later", () => {
    const b: Bus[] = [{ name: "x", parent: "y", volume: 1 }, { name: "y", volume: 1 }, { name: "master", volume: 1 }];
    expect(parentsFirst(b).map((x) => x.name)).toEqual(["master", "y", "x"]);
  });

  it("clamps an effect to the engine's range and leaves defaults out", () => {
    let b: Bus = { name: "fx", volume: 1 };
    b = withEffect(b, "eq_mid", 99);
    expect(effectOf(b, "eq_mid")).toBe(24);
    b = withEffect(b, "room", 0.5);
    expect(b.effects).toEqual({ eq_mid: 24 });
    b = withEffect(b, "eq_mid", 0);
    expect(b.effects).toBeUndefined();
    expect(effectOf(b, "echo_time")).toBe(0.35);
  });

  it("adds, renames and removes buses, keeping children attached", () => {
    let buses = shownBuses(undefined);
    buses = addBus(buses, "music");
    buses = addBus(buses, "bus");
    expect(buses.map((b) => b.name)).toContain("bus2");
    buses = renameBus(buses, "bus", "ambience");
    buses = renameBus(buses, "bus2", "ambience");
    expect(buses.filter((b) => b.name === "ambience")).toHaveLength(1);
    expect(renameBus(buses, "music", "tunes")).toBe(buses);
    buses = buses.map((b) => (b.name === "bus2" ? { ...b, parent: "ambience" } : b));
    buses = removeBus(buses, "ambience");
    expect(buses.find((b) => b.name === "bus2")?.parent).toBe("music");
    expect(removeBus(buses, "master")).toBe(buses);
    expect(isUnder(buses, "bus2", "master")).toBe(true);
    expect(isUnder(buses, "music", "bus2")).toBe(false);
  });
});
