import { describe, expect, it } from "vitest";
import {
  isPanelShown,
  layoutWithPanelHidden,
  layoutWithPanelShown,
  upgradeLayout,
  type EditorLayout,
} from "./layoutStore";

// A layout saved before panels could be switched off: every panel docked.
function oldLayout(): EditorLayout {
  return {
    zones: {
      left: { groups: [{ panels: ["scene", "assets"], active: "scene" }] },
      right: { groups: [{ panels: ["inspector"], active: "inspector" }] },
      bottom: {
        groups: [
          {
            panels: ["console", "history", "build", "search", "servers", "timeline", "localisation", "mixer", "debugger", "profiler"],
            active: "mixer",
          },
        ],
      },
    },
    leftWidth: 220,
    rightWidth: 240,
    bottomHeight: 150,
  };
}

describe("panel visibility", () => {
  it("an old layout keeps four bottom tabs and switches the rest off", () => {
    const l = upgradeLayout(oldLayout());
    expect(l.zones.bottom.groups[0].panels).toEqual(["console", "build", "timeline", "profiler"]);
    expect(new Set(l.hidden)).toEqual(new Set(["history", "search", "servers", "localisation", "mixer", "debugger"]));
    // The active tab was switched off, so another takes its place.
    expect(l.zones.bottom.groups[0].panels).toContain(l.zones.bottom.groups[0].active);
    expect(l.version).toBe(2);
  });

  it("a panel the user moved out of the console's group stays where they put it", () => {
    const old = oldLayout();
    old.zones.bottom.groups[0].panels = old.zones.bottom.groups[0].panels.filter((p) => p !== "mixer");
    old.zones.right.groups[0].panels.push("mixer");
    const l = upgradeLayout(old);
    expect(l.zones.right.groups[0].panels).toContain("mixer");
    expect(l.hidden).not.toContain("mixer");
  });

  it("an upgraded layout is left alone the next time it loads", () => {
    const once = upgradeLayout(oldLayout());
    const shown = layoutWithPanelShown(once, "mixer");
    const again = upgradeLayout(structuredClone(shown));
    expect(isPanelShown(again, "mixer")).toBe(true);
  });

  it("a switched-off panel is not docked again by the upgrade", () => {
    const l = layoutWithPanelHidden(upgradeLayout(oldLayout()), "build");
    const loaded = upgradeLayout(structuredClone(l));
    expect(isPanelShown(loaded, "build")).toBe(false);
    expect(loaded.hidden).toContain("build");
  });

  it("a panel this layout predates is docked beside the console", () => {
    const l = upgradeLayout(oldLayout());
    l.zones.bottom.groups[0].panels = l.zones.bottom.groups[0].panels.filter((p) => p !== "profiler");
    const loaded = upgradeLayout(l);
    expect(loaded.zones.bottom.groups[0].panels).toContain("profiler");
  });

  it("hiding and showing round-trips, and showing brings the panel to the front", () => {
    const l = upgradeLayout(oldLayout());
    const hidden = layoutWithPanelHidden(l, "timeline");
    expect(isPanelShown(hidden, "timeline")).toBe(false);
    const back = layoutWithPanelShown(hidden, "timeline");
    expect(isPanelShown(back, "timeline")).toBe(true);
    expect(back.zones.bottom.groups[0].active).toBe("timeline");
    expect(back.hidden).not.toContain("timeline");
  });

  it("a shown panel goes home: the scene tree to the left, the inspector to the right", () => {
    let l = upgradeLayout(oldLayout());
    l = layoutWithPanelHidden(l, "scene");
    l = layoutWithPanelHidden(l, "inspector");
    expect(l.zones.right.groups).toHaveLength(0);
    l = layoutWithPanelShown(layoutWithPanelShown(l, "scene"), "inspector");
    expect(l.zones.left.groups[0].panels).toContain("scene");
    expect(l.zones.right.groups[0].panels).toEqual(["inspector"]);
  });
});
