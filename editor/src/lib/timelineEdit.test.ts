import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  addClip,
  addEvent,
  addTrack,
  clipLength,
  currentClipName,
  currentValue,
  freshClipName,
  moveEvent,
  moveKey,
  previewAt,
  removeClip,
  removeKey,
  renameClip,
  resolveTarget,
  sampleTrack,
  setKey,
  type Clip,
  type Track,
} from "./timelineEdit";

// The engine's own answers (`timeline_sample_matches_the_editor`).
const fixture = JSON.parse(
  readFileSync(new URL("../../../crates/jengine-core/tests/fixtures/timeline_sample.json", import.meta.url), "utf-8"),
) as { cases: { name: string; track: Track; t: number; expected: unknown }[] };

describe("sampling matches the engine's", () => {
  for (const c of fixture.cases) {
    it(c.name, () => {
      const got = sampleTrack(c.track, c.t);
      if (typeof c.expected === "number") expect(got as number).toBeCloseTo(c.expected, 4);
      else if (Array.isArray(c.expected) && c.expected.every((v) => typeof v === "number"))
        (got as number[]).forEach((v, i) => expect(v).toBeCloseTo((c.expected as number[])[i], 4));
      else expect(got).toEqual(c.expected);
    });
  }
});

describe("clips", () => {
  it("adds, names, renames and removes clips", () => {
    let tl = addClip({}, "walk");
    expect(currentClipName(tl)).toBe("walk");
    expect(freshClipName(tl)).toBe("clip");
    expect(freshClipName(tl, "walk")).toBe("walk 2");
    tl = addClip(tl, "run");
    tl = renameClip(tl, "walk", "stroll");
    expect(Object.keys(tl.clips!).sort()).toEqual(["run", "stroll"]);
    expect(tl.clip).toBe("stroll");
    expect(renameClip(tl, "run", "stroll")).toBe(tl);
    tl = removeClip(tl, "stroll");
    expect(currentClipName(tl)).toBe("run");
  });

  it("is as long as its duration, or its last key", () => {
    expect(clipLength({ duration: 3, tracks: [{ keys: [[5, 1]] }] })).toBe(3);
    expect(clipLength({ tracks: [{ keys: [[1, 0], [2.5, 1]] }], events: [[1.5, "x"]] })).toBe(2.5);
  });
});

describe("keys", () => {
  const clip = (): Clip => ({ tracks: [{ component: "transform", field: "position", keys: [[0, [0, 0]], [1, [10, 0]]] }] });

  it("sets a key at a time, replacing one already there", () => {
    const c = clip();
    expect(setKey(c, 0, 0.5, [5, 5])).toBe(1);
    expect(setKey(c, 0, 1.00001, [20, 0])).toBe(2);
    expect(c.tracks![0].keys).toEqual([[0, [0, 0]], [0.5, [5, 5]], [1, [20, 0]]]);
  });

  it("moves a key past another and removes keys", () => {
    const c = clip();
    expect(moveKey(c, 0, 0, 2)).toBe(1);
    expect(c.tracks![0].keys!.map((k) => k[0])).toEqual([1, 2]);
    // Landing on a key replaces it.
    expect(moveKey(c, 0, 1, 1)).toBe(0);
    expect(c.tracks![0].keys).toEqual([[1, [0, 0]]]);
    removeKey(c, 0, 0);
    expect(c.tracks![0].keys).toEqual([]);
    addTrack(c, { component: "sprite", field: "color" });
    expect(c.tracks![1]).toEqual({ target: "", ease: "linear", keys: [], component: "sprite", field: "color" });
  });

  it("orders event keys", () => {
    const c: Clip = {};
    addEvent(c, 2, "b");
    expect(addEvent(c, 1, "a")).toBe(0);
    expect(moveEvent(c, 0, 3)).toBe(1);
    expect(c.events).toEqual([[2, "b"], [3, "a"]]);
  });
});

describe("previewing the scene", () => {
  const scene = () => [
    { name: "Stage", components: { timeline: {} } },
    { name: "Puppet", parent: "Stage", components: { transform: { position: [0, 0], rotation: 0 }, sprite: { z_order: 1 } } },
    { name: "Puppet", components: { transform: { position: [99, 99], rotation: 0 } } },
  ];

  it("finds a target below the timeline's entity first", () => {
    expect(resolveTarget(scene(), 0, "Puppet")).toBe(1);
    expect(resolveTarget(scene(), 0, "")).toBe(0);
    expect(resolveTarget(scene(), 0, "Nobody")).toBe(-1);
  });

  it("poses the scene at a time without touching it", () => {
    const s = scene();
    const clip: Clip = {
      tracks: [
        { target: "Puppet", component: "transform", field: "position", keys: [[0, [0, 0]], [2, [100, 0]]] },
        { target: "Puppet", component: "sprite", field: "z_order", keys: [[0, 1], [2, 4]] },
      ],
    };
    const p = previewAt(s, 0, clip, 1);
    expect((p[1].components.transform as any).position).toEqual([50, 0]);
    // A number over an integer is rounded, as the engine writes it.
    expect((p[1].components.sprite as any).z_order).toBe(3);
    expect((s[1].components.transform as any).position).toEqual([0, 0]);
    expect(currentValue(s[1], "transform", "position")).toEqual([0, 0]);
  });
});
