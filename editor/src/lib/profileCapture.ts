/**
 * Reading a profiler capture (Rust `profiling::trace`): frames of spans -
 * named, nested by depth, each with its CPU time and the draws made inside
 * it - and each script's cost. The Profiler panel (`ProfilerPanel.svelte`)
 * shows a capture through these functions: a tree of spans averaged over
 * the frames chosen (with each node's own time, its children's taken off),
 * the scripts by cost, and the render passes with their draws.
 *
 * The editor records one by asking the viewport for each frame's record
 * (`viewport_take_profile_frame`) and adding the scripts Fengari timed; the
 * native runtime writes the same shape (`JENGINE_PROFILE_CAPTURE`).
 */

export interface SpanRecord {
  name: string;
  depth: number;
  start_ms: number;
  ms: number;
  draws?: number;
}
export interface ScriptCost {
  path: string;
  ms: number;
  calls: number;
}
export interface FrameRecord {
  index: number;
  /** Wall time since the frame before - the frame rate, vsync included. */
  ms: number;
  spans: SpanRecord[];
  scripts?: ScriptCost[];
  draws?: number;
}
export interface Capture {
  runtime: string;
  frames: FrameRecord[];
}

/** The CPU time a frame's top-level spans account for. */
export function tracedMs(frame: FrameRecord): number {
  return frame.spans.reduce((s, x) => (x.depth === 0 ? s + x.ms : s), 0);
}

export interface SpanNode {
  /** Names from the top, joined by ` > `. */
  key: string;
  name: string;
  depth: number;
  /** Per frame, over the frames read: average and worst. */
  ms: number;
  maxMs: number;
  /** Its own time: `ms` less its children's. */
  selfMs: number;
  /** Times it ran per frame (a span called twice a frame is one node). */
  calls: number;
  draws: number;
  children: SpanNode[];
}

/**
 * The spans of `frames` as one tree, averaged per frame: spans of the same
 * name under the same parent are one node (summed within a frame), so a
 * system that runs twice a frame shows once, at its whole cost.
 */
export function spanTree(frames: FrameRecord[]): SpanNode[] {
  interface Acc {
    name: string;
    depth: number;
    total: number;
    max: number;
    calls: number;
    draws: number;
    children: string[];
  }
  const nodes = new Map<string, Acc>();
  const roots: string[] = [];
  for (const f of frames) {
    const perFrame = new Map<string, number>();
    const stack: string[] = [];
    for (const s of f.spans) {
      stack.length = Math.min(stack.length, s.depth);
      const parent = stack[s.depth - 1];
      const key = parent === undefined ? s.name : `${parent} > ${s.name}`;
      stack[s.depth] = key;
      let acc = nodes.get(key);
      if (!acc) {
        acc = { name: s.name, depth: s.depth, total: 0, max: 0, calls: 0, draws: 0, children: [] };
        nodes.set(key, acc);
        if (parent === undefined) roots.push(key);
        else nodes.get(parent)?.children.push(key);
      }
      acc.total += s.ms;
      acc.calls += 1;
      acc.draws += s.draws ?? 0;
      perFrame.set(key, (perFrame.get(key) ?? 0) + s.ms);
    }
    for (const [key, ms] of perFrame) {
      const acc = nodes.get(key)!;
      acc.max = Math.max(acc.max, ms);
    }
  }
  const n = Math.max(1, frames.length);
  const build = (key: string): SpanNode => {
    const a = nodes.get(key)!;
    const children = a.children.map(build).sort((x, y) => y.ms - x.ms);
    const ms = a.total / n;
    return {
      key,
      name: a.name,
      depth: a.depth,
      ms,
      maxMs: a.max,
      selfMs: Math.max(0, ms - children.reduce((s, c) => s + c.ms, 0)),
      calls: a.calls / n,
      draws: a.draws / n,
      children,
    };
  };
  return roots.map(build).sort((x, y) => y.ms - x.ms);
}

/** Every node of a tree, parents before children (for a flat table). */
export function flatten(tree: SpanNode[], open: (key: string) => boolean = () => true): SpanNode[] {
  const out: SpanNode[] = [];
  const walk = (ns: SpanNode[]) => {
    for (const n of ns) {
      out.push(n);
      if (open(n.key)) walk(n.children);
    }
  };
  walk(tree);
  return out;
}

export interface ScriptRow {
  path: string;
  /** Per frame, over the frames read. */
  ms: number;
  maxMs: number;
  calls: number;
}

/** Each script's cost per frame, the dearest first. */
export function scriptTable(frames: FrameRecord[]): ScriptRow[] {
  const acc = new Map<string, { total: number; max: number; calls: number }>();
  for (const f of frames) {
    for (const s of f.scripts ?? []) {
      const a = acc.get(s.path) ?? { total: 0, max: 0, calls: 0 };
      a.total += s.ms;
      a.max = Math.max(a.max, s.ms);
      a.calls += s.calls;
      acc.set(s.path, a);
    }
  }
  const n = Math.max(1, frames.length);
  return [...acc]
    .map(([path, a]) => ({ path, ms: a.total / n, maxMs: a.max, calls: a.calls / n }))
    .sort((x, y) => y.ms - x.ms || x.path.localeCompare(y.path));
}

export interface PassRow {
  name: string;
  ms: number;
  draws: number;
}

/** The render passes - every span under `render` - with their draws. */
export function passTable(tree: SpanNode[]): PassRow[] {
  const render = tree.find((n) => n.name === "render");
  if (!render) return [];
  const out: PassRow[] = [];
  const walk = (ns: SpanNode[], prefix: string) => {
    for (const n of ns) {
      const name = prefix ? `${prefix} > ${n.name}` : n.name;
      if (n.children.length === 0) out.push({ name, ms: n.ms, draws: n.draws });
      else walk(n.children, name);
    }
  };
  walk(render.children, "");
  return out.sort((a, b) => b.draws - a.draws || b.ms - a.ms);
}

/** What a span counts as on the frame bars: the part of its name before
 *  the first dot (`render`, `physics`, `motion`, `scripts`, ...). */
export function category(name: string): string {
  return name.split(".")[0];
}

/** One frame's top-level time by category, for its stacked bar. */
export function frameStack(frame: FrameRecord): { category: string; ms: number }[] {
  const by = new Map<string, number>();
  for (const s of frame.spans) if (s.depth === 0) by.set(category(s.name), (by.get(category(s.name)) ?? 0) + s.ms);
  return [...by].map(([c, ms]) => ({ category: c, ms })).sort((a, b) => a.category.localeCompare(b.category));
}

/** The frame whose spans took longest, or -1 for none. */
export function worstFrame(frames: FrameRecord[]): number {
  let best = -1;
  let ms = -1;
  frames.forEach((f, i) => {
    const t = tracedMs(f);
    if (t > ms) {
      ms = t;
      best = i;
    }
  });
  return best;
}

/** A frame record with the scripts' costs Fengari measured added (the
 *  editor's scripts run in JavaScript, out of the engine's sight). */
export function withScripts(frame: FrameRecord, costs: Map<string, { ms: number; calls: number }>): FrameRecord {
  const scripts: ScriptCost[] = [...(frame.scripts ?? [])];
  for (const [path, c] of costs) {
    const at = scripts.findIndex((s) => s.path === path);
    if (at >= 0) scripts[at] = { path, ms: scripts[at].ms + c.ms, calls: scripts[at].calls + c.calls };
    else scripts.push({ path, ms: c.ms, calls: c.calls });
  }
  scripts.sort((a, b) => b.ms - a.ms);
  return { ...frame, scripts };
}

/** A capture read from a file, refused if it is not one. */
export function parseCapture(text: string): Capture {
  const raw = JSON.parse(text);
  if (!raw || !Array.isArray(raw.frames)) throw new Error("not a profiler capture (no frames)");
  for (const f of raw.frames) {
    if (!Array.isArray(f?.spans) || typeof f.ms !== "number") throw new Error("not a profiler capture (a frame without spans)");
  }
  return { runtime: String(raw.runtime ?? "?"), frames: raw.frames };
}

export function formatCapture(c: Capture): string {
  return JSON.stringify(c);
}

/** Frames kept while recording; past it the oldest go. */
export const MAX_FRAMES = 3600;

/** Append a frame, dropping the oldest past `max`. Returns the array. */
export function pushFrame(frames: FrameRecord[], f: FrameRecord, max = MAX_FRAMES): FrameRecord[] {
  frames.push(f);
  if (frames.length > max) frames.splice(0, frames.length - max);
  return frames;
}
