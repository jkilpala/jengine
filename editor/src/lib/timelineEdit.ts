/**
 * Editing a `timeline` component (Rust `ecs::timeline`) as the JSON it is:
 * named clips, each with tracks of `[t, value]` keys on one field of one
 * component, and event keys `[t, name]`. The dope sheet
 * (`TimelinePanel.svelte`) does every edit through these functions, and
 * scrubs the scene with `previewAt`, which samples tracks the engine's way -
 * `sampleTrack` is checked against Rust on one fixture
 * (`crates/jengine-core/tests/fixtures/timeline_sample.json`).
 *
 * Every function returns new data; none mutates its argument.
 */
import { getAtPath } from "./pointFields";

export type Ease = "linear" | "step" | "in" | "out" | "in_out";
export type Key = [number, unknown];

export interface Track {
  target?: string;
  component?: string;
  field?: string;
  keys?: Key[];
  ease?: Ease;
}

export interface Clip {
  duration?: number;
  tracks?: Track[];
  events?: [number, string][];
}

export interface TimelineData {
  clips?: Record<string, Clip>;
  clip?: string;
  playing?: boolean;
  time?: number;
  speed?: number;
  looping?: boolean;
}

export const EASES: Ease[] = ["linear", "step", "in", "out", "in_out"];

const clone = <T>(v: T): T => structuredClone(v);
const SAME_TIME = 1e-4;

// ─── Sampling, as the engine samples ────────────────────────────────────

export function easeApply(ease: Ease | undefined, f: number): number {
  const x = Math.min(1, Math.max(0, f));
  switch (ease ?? "linear") {
    case "step":
      return 0;
    case "in":
      return x * x;
    case "out":
      return 1 - (1 - x) * (1 - x);
    case "in_out":
      return x * x * (3 - 2 * x);
    default:
      return x;
  }
}

function lerp(a: unknown, b: unknown, f: number): unknown {
  if (typeof a === "number" && typeof b === "number") return a + (b - a) * f;
  if (
    Array.isArray(a) &&
    Array.isArray(b) &&
    a.length === b.length &&
    [...a, ...b].every((v) => typeof v === "number")
  ) {
    return a.map((x, i) => (x as number) + ((b[i] as number) - (x as number)) * f);
  }
  return f >= 1 ? b : a;
}

/** A track's value at `t`: the first key before it, the last after, numbers
 *  and same-length number lists eased between, anything else held. */
export function sampleTrack(track: Track, t: number): unknown {
  const keys = track.keys ?? [];
  if (keys.length === 0) return undefined;
  if (t <= keys[0][0] || keys.length === 1) return clone(keys[0][1]);
  let i = -1;
  for (let k = keys.length - 1; k >= 0; k--) {
    if (keys[k][0] <= t) {
      i = k;
      break;
    }
  }
  if (i < 0) return clone(keys[0][1]);
  const [t0, v0] = keys[i];
  const next = keys[i + 1];
  if (!next) return clone(v0);
  const [t1, v1] = next;
  const f = t1 > t0 ? easeApply(track.ease, (t - t0) / (t1 - t0)) : 1;
  return clone(lerp(v0, v1, f));
}

/** A clip's length: its `duration`, or its last key or event. */
export function clipLength(clip: Clip): number {
  if ((clip.duration ?? 0) > 0) return clip.duration!;
  let end = 0;
  for (const tr of clip.tracks ?? []) for (const k of tr.keys ?? []) end = Math.max(end, k[0]);
  for (const e of clip.events ?? []) end = Math.max(end, e[0]);
  return end;
}

/** The clip a timeline plays: `clip`, or the first by name. */
export function currentClipName(tl: TimelineData): string | null {
  const names = Object.keys(tl.clips ?? {}).sort();
  if (tl.clip && tl.clip in (tl.clips ?? {})) return tl.clip;
  return names[0] ?? null;
}

// ─── Clips ───────────────────────────────────────────────────────────────

/** A clip name not yet used: `base`, else `base 2`, `base 3`... */
export function freshClipName(tl: TimelineData, base = "clip"): string {
  const used = new Set(Object.keys(tl.clips ?? {}));
  if (!used.has(base)) return base;
  for (let n = 2; ; n++) if (!used.has(`${base} ${n}`)) return `${base} ${n}`;
}

export function addClip(tl: TimelineData, name: string): TimelineData {
  const out = clone(tl);
  out.clips = { ...(out.clips ?? {}), [name]: { duration: 0, tracks: [], events: [] } };
  if (!out.clip) out.clip = name;
  return out;
}

export function removeClip(tl: TimelineData, name: string): TimelineData {
  const out = clone(tl);
  delete out.clips?.[name];
  if (out.clip === name) out.clip = "";
  return out;
}

/** Rename a clip, and the timeline's choice of clip with it. Unchanged if
 *  the new name is empty or taken. */
export function renameClip(tl: TimelineData, from: string, to: string): TimelineData {
  const name = to.trim();
  if (!name || name === from || name in (tl.clips ?? {}) || !(from in (tl.clips ?? {}))) return tl;
  const out = clone(tl);
  const clips: Record<string, Clip> = {};
  for (const [k, v] of Object.entries(out.clips!)) clips[k === from ? name : k] = v;
  out.clips = clips;
  if (out.clip === from) out.clip = name;
  return out;
}

/** `fn` on a copy of clip `name`. */
export function editClip(tl: TimelineData, name: string, fn: (clip: Clip) => void): TimelineData {
  const out = clone(tl);
  const clip = out.clips?.[name];
  if (clip) fn(clip);
  return out;
}

// ─── Tracks and keys (on a clip) ─────────────────────────────────────────

/** A track's row title: `target.component.field`, the target left out for
 *  the timeline's own entity. */
export function trackTitle(track: Track): string {
  return `${track.target ? track.target + " · " : ""}${track.component ?? "?"}.${track.field ?? "?"}`;
}

export function addTrack(clip: Clip, track: Track): void {
  clip.tracks = [...(clip.tracks ?? []), { target: "", ease: "linear", keys: [], ...track }];
}

/** Put a key at `t` on track `i`: the key already there (within a
 *  ten-thousandth of a second) gets the value, else one is added in order.
 *  Returns the key's index. */
export function setKey(clip: Clip, i: number, t: number, value: unknown): number {
  const track = clip.tracks?.[i];
  if (!track) return -1;
  const keys = (track.keys ??= []);
  const time = Math.max(0, t);
  const at = keys.findIndex((k) => Math.abs(k[0] - time) < SAME_TIME);
  if (at >= 0) {
    keys[at] = [keys[at][0], clone(value)];
    return at;
  }
  const key: Key = [time, clone(value)];
  keys.push(key);
  keys.sort((a, b) => a[0] - b[0]);
  return keys.indexOf(key);
}

/** Move key `k` of track `i` to `t` (not below 0), keeping the keys in
 *  order; a key it lands on is replaced. Returns its new index. */
export function moveKey(clip: Clip, i: number, k: number, t: number): number {
  const track = clip.tracks?.[i];
  const keys = track?.keys;
  if (!keys || !keys[k]) return -1;
  const moving = keys[k];
  const time = Math.max(0, t);
  track!.keys = keys.filter((key, j) => j === k || Math.abs(key[0] - time) >= SAME_TIME);
  moving[0] = time;
  track!.keys.sort((a, b) => a[0] - b[0]);
  return track!.keys.indexOf(moving);
}

export function removeKey(clip: Clip, i: number, k: number): void {
  const track = clip.tracks?.[i];
  if (track?.keys) track.keys = track.keys.filter((_, j) => j !== k);
}

// ─── Event keys ─────────────────────────────────────────────────────────

export function addEvent(clip: Clip, t: number, name: string): number {
  const ev: [number, string] = [Math.max(0, t), name];
  clip.events = [...(clip.events ?? []), ev].sort((a, b) => a[0] - b[0]);
  return clip.events.indexOf(ev);
}

export function moveEvent(clip: Clip, e: number, t: number): number {
  const ev = clip.events?.[e];
  if (!ev) return -1;
  ev[0] = Math.max(0, t);
  clip.events!.sort((a, b) => a[0] - b[0]);
  return clip.events!.indexOf(ev);
}

export function removeEvent(clip: Clip, e: number): void {
  clip.events = (clip.events ?? []).filter((_, j) => j !== e);
}

// ─── Previewing a scene at a time ───────────────────────────────────────

export interface SceneEntity {
  name: string;
  parent?: string | null;
  components: Record<string, unknown>;
}

/** Which entity a track names: the timeline's own for an empty target,
 *  else one of that name below the timeline's entity first, then any. */
export function resolveTarget(entities: SceneEntity[], owner: number, target: string | undefined): number {
  if (!target) return owner;
  const ownerName = entities[owner]?.name;
  const below = (i: number) => {
    let p = entities[i]?.parent;
    for (let guard = 0; p && guard < 1000; guard++) {
      if (p === ownerName) return true;
      p = entities.find((e) => e.name === p)?.parent;
    }
    return false;
  };
  const named = entities.map((e, i) => [e, i] as const).filter(([e]) => e.name === target);
  const inside = named.find(([, i]) => below(i));
  return (inside ?? named[0])?.[1] ?? -1;
}

/** Set a dotted path in place; a number written over an integer is
 *  rounded, as the engine does so a track can drive a `z_order`. */
function setPath(root: Record<string, unknown>, path: string, value: unknown): boolean {
  const parts = path.split(".").filter(Boolean);
  let at: unknown = root;
  for (let n = 0; n < parts.length - 1; n++) {
    at = at && typeof at === "object" ? (at as Record<string, unknown>)[parts[n]] : undefined;
  }
  if (!at || typeof at !== "object") return false;
  const last = parts[parts.length - 1];
  const holder = at as Record<string, unknown>;
  const old = holder[last];
  holder[last] = typeof value === "number" && Number.isInteger(old) ? Math.round(value) : clone(value);
  return true;
}

/** The scene's entities with clip `clip` of the timeline on entity `owner`
 *  posed at `t` - what the scene view shows while the playhead is dragged.
 *  The entities passed in are not changed. */
export function previewAt(entities: SceneEntity[], owner: number, clip: Clip, t: number): SceneEntity[] {
  const out = clone(entities);
  for (const track of clip.tracks ?? []) {
    if (!track.component || !track.field) continue;
    const value = sampleTrack(track, t);
    if (value === undefined) continue;
    const i = resolveTarget(out, owner, track.target);
    const comp = out[i]?.components?.[track.component];
    if (comp && typeof comp === "object") setPath(comp as Record<string, unknown>, track.field, value);
  }
  return out;
}

/** The value a field has now, to key it: `component.field` on `entity`. */
export function currentValue(entity: SceneEntity | undefined, component: string, field: string): unknown {
  return entity ? clone(getAtPath(entity.components?.[component], field)) : undefined;
}
