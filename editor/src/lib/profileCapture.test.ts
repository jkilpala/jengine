import { describe, expect, it } from "vitest";
import {
  flatten,
  frameStack,
  parseCapture,
  passTable,
  pushFrame,
  scriptTable,
  spanTree,
  tracedMs,
  withScripts,
  worstFrame,
  type FrameRecord,
} from "./profileCapture";

const span = (name: string, depth: number, ms: number, draws = 0) => ({ name, depth, start_ms: 0, ms, draws });

// Two frames as `profiling::trace` writes them: parents before children.
const a: FrameRecord = {
  index: 0,
  ms: 16.6,
  spans: [
    span("motion", 0, 1.0),
    span("motion.integrate", 1, 0.4),
    span("hierarchy", 0, 0.2),
    span("hierarchy", 0, 0.2),
    span("render", 0, 4.0, 10),
    span("render.world", 1, 3.0, 6),
    span("render.sprites", 2, 2.0, 5),
    span("render.lights", 2, 0.5, 1),
    span("render.ui", 1, 0.5, 4),
  ],
  scripts: [{ path: "scripts/hero.lua", ms: 0.3, calls: 1 }],
  draws: 10,
};
const b: FrameRecord = {
  index: 1,
  ms: 16.7,
  spans: [span("motion", 0, 3.0), span("motion.integrate", 1, 2.0), span("render", 0, 2.0, 8), span("render.world", 1, 2.0, 8), span("render.sprites", 2, 1.0, 8)],
  scripts: [
    { path: "scripts/hero.lua", ms: 0.1, calls: 1 },
    { path: "scripts/cam.lua", ms: 0.2, calls: 2 },
  ],
  draws: 8,
};

describe("a capture's tree", () => {
  const tree = spanTree([a, b]);
  const find = (key: string) => flatten(tree).find((n) => n.key === key)!;

  it("averages each span per frame and keeps the worst", () => {
    expect(find("render").ms).toBeCloseTo(3);
    expect(find("render").maxMs).toBeCloseTo(4);
    expect(find("motion > motion.integrate").ms).toBeCloseTo(1.2);
    expect(tree.map((n) => n.name)).toEqual(["render", "motion", "hierarchy"]);
  });

  it("is one node for a span run twice a frame, at its whole cost", () => {
    const h = find("hierarchy");
    expect(h.ms).toBeCloseTo(0.2); // 0.4 in one frame of two
    expect(h.maxMs).toBeCloseTo(0.4);
    expect(h.calls).toBe(1);
  });

  it("gives each node its own time, its children's taken off", () => {
    expect(find("render > render.world").selfMs).toBeCloseTo(2.5 - 1.75);
    expect(find("motion").selfMs).toBeCloseTo(2 - 1.2);
  });

  it("nests by depth, a child under the span open at its depth", () => {
    expect(find("render > render.world > render.lights").depth).toBe(2);
    expect(find("render > render.ui").children).toEqual([]);
    expect(flatten(tree, (k) => k !== "render").map((n) => n.key)).toEqual(["render", "motion", "motion > motion.integrate", "hierarchy"]);
  });

  it("lists the render passes with their draws", () => {
    expect(passTable(tree)).toEqual([
      { name: "render.world > render.sprites", ms: 1.5, draws: 6.5 },
      { name: "render.ui", ms: 0.25, draws: 2 },
      { name: "render.world > render.lights", ms: 0.25, draws: 0.5 },
    ]);
  });
});

describe("a capture's scripts and frames", () => {
  it("costs each script per frame, the dearest first", () => {
    expect(scriptTable([a, b])).toEqual([
      { path: "scripts/hero.lua", ms: 0.2, maxMs: 0.3, calls: 1 },
      { path: "scripts/cam.lua", ms: 0.1, maxMs: 0.2, calls: 1 },
    ]);
  });

  it("stacks a frame's bar by what its spans are", () => {
    expect(tracedMs(a)).toBeCloseTo(5.4);
    expect(frameStack(a)).toEqual([
      { category: "hierarchy", ms: 0.4 },
      { category: "motion", ms: 1 },
      { category: "render", ms: 4 },
    ]);
    expect(worstFrame([a, b])).toBe(0);
    expect(worstFrame([])).toBe(-1);
  });

  it("adds the scripts Fengari timed to the engine's frame", () => {
    const f = withScripts(b, new Map([["scripts/cam.lua", { ms: 0.5, calls: 1 }], ["scripts/new.lua", { ms: 1, calls: 1 }]]));
    expect(f.scripts!.map((s) => [s.path, +s.ms.toFixed(3), s.calls])).toEqual([
      ["scripts/new.lua", 1, 1],
      ["scripts/cam.lua", 0.7, 3],
      ["scripts/hero.lua", 0.1, 1],
    ]);
    expect(b.scripts!.length).toBe(2);
  });

  it("keeps the newest frames while recording", () => {
    const frames: FrameRecord[] = [];
    for (let i = 0; i < 5; i++) pushFrame(frames, { ...a, index: i }, 3);
    expect(frames.map((f) => f.index)).toEqual([2, 3, 4]);
  });

  it("reads a capture file, and refuses what is not one", () => {
    expect(parseCapture(JSON.stringify({ runtime: "native", frames: [a] })).frames.length).toBe(1);
    expect(() => parseCapture("{}")).toThrow(/not a profiler capture/);
    expect(() => parseCapture(JSON.stringify({ frames: [{ ms: 1 }] }))).toThrow(/not a profiler capture/);
  });
});
