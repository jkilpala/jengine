/**
 * The scene view's editing aids, as plain functions: snapping a dragged
 * value to a step, choosing what a box drag selects, moving a group without
 * moving a child twice, and carrying one entity's Inspector edit to every
 * other selected entity.
 */
import { setByPath } from "./prefabInstance";

// ─── Snapping ────────────────────────────────────────────────────────────

export interface SnapSettings {
  /** Snap while dragging (Ctrl inverts it for the drag). */
  enabled: boolean;
  /** World units a move snaps to. */
  grid: number;
  /** Degrees a turn snaps to. */
  angle: number;
  /** The step a scale snaps to. */
  scale: number;
}

export const DEFAULT_SNAP: SnapSettings = { enabled: false, grid: 16, angle: 15, scale: 0.1 };

/** Whether this drag snaps: the setting, turned over while Ctrl is held. */
export function snapping(settings: SnapSettings, ctrlHeld: boolean): boolean {
  return settings.enabled !== ctrlHeld;
}

/** `v` to the nearest multiple of `step` (a step of 0 or less is no snap),
 *  rounded clean of float noise. */
export function snapTo(v: number, step: number): number {
  if (!(step > 0)) return v;
  return Math.round(Math.round(v / step) * step * 1e6) / 1e6;
}

/** An angle in radians to the nearest multiple of `degrees`. */
export function snapAngle(radians: number, degrees: number): number {
  if (!(degrees > 0)) return radians;
  const step = (degrees * Math.PI) / 180;
  return Math.round(radians / step) * step;
}

/** Snap settings as stored (any of them may be missing or wrong). */
export function parseSnap(raw: unknown): SnapSettings {
  const o = (raw && typeof raw === "object" ? raw : {}) as Partial<Record<keyof SnapSettings, unknown>>;
  const num = (v: unknown, d: number) => (typeof v === "number" && v > 0 ? v : d);
  return {
    enabled: typeof o.enabled === "boolean" ? o.enabled : DEFAULT_SNAP.enabled,
    grid: num(o.grid, DEFAULT_SNAP.grid),
    angle: num(o.angle, DEFAULT_SNAP.angle),
    scale: num(o.scale, DEFAULT_SNAP.scale),
  };
}

// ─── Box select ──────────────────────────────────────────────────────────

/** An entity's picking box, as `viewport_get_entity_bounds` gives it. */
export interface EntityBounds {
  index: number;
  x: number;
  y: number;
  width: number;
  height: number;
  rotation: number;
}

/** The entities a box drag from `a` to `b` (world points, any corners)
 *  selects: those whose centre is inside it - what you see an entity as
 *  "being", whatever its size. */
export function entitiesInBox(bounds: EntityBounds[], a: [number, number], b: [number, number]): number[] {
  const minX = Math.min(a[0], b[0]);
  const maxX = Math.max(a[0], b[0]);
  const minY = Math.min(a[1], b[1]);
  const maxY = Math.max(a[1], b[1]);
  return bounds.filter((e) => e.x >= minX && e.x <= maxX && e.y >= minY && e.y <= maxY).map((e) => e.index);
}

// ─── Moving a group ──────────────────────────────────────────────────────

/** The selected entities a group transform moves itself: a child of another
 *  selected entity moves with its parent, and moving it as well would carry
 *  it twice. `parentOf(i)` is the index of `i`'s parent, or -1. */
export function topLevelSelection(selected: Iterable<number>, parentOf: (i: number) => number): number[] {
  const set = new Set(selected);
  const out: number[] = [];
  for (const i of set) {
    let p = parentOf(i);
    let guard = 0;
    let covered = false;
    while (p >= 0 && guard++ < 1000) {
      if (set.has(p)) {
        covered = true;
        break;
      }
      p = parentOf(p);
    }
    if (!covered) out.push(i);
  }
  return out.sort((x, y) => x - y);
}

// ─── Editing several entities ────────────────────────────────────────────

/** The paths where `before` and `after` differ. An array with one element
 *  changed is that element - an edit of a position's x is
 *  `transform.position.0`, so carrying it leaves every other entity's y
 *  alone - and with several (or a new length) the whole array. A key that
 *  is gone is reported with `null`. */
function changedPaths(prefix: string, before: unknown, after: unknown, out: [string, unknown][]): void {
  if (JSON.stringify(before) === JSON.stringify(after)) return;
  const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
  if (isObj(before) && isObj(after)) {
    for (const key of new Set([...Object.keys(before), ...Object.keys(after)])) {
      changedPaths(`${prefix}.${key}`, before[key], after[key], out);
    }
    return;
  }
  if (Array.isArray(before) && Array.isArray(after) && before.length === after.length) {
    // One element changed (a typed x, an alpha): carry just it. Several (a
    // colour picked, a vector pasted): the whole value is what was meant.
    const changed = after.map((v, i) => JSON.stringify(v) !== JSON.stringify(before[i]));
    const which = changed.indexOf(true);
    if (changed.filter(Boolean).length === 1) {
      changedPaths(`${prefix}.${which}`, before[which], after[which], out);
      return;
    }
  }
  out.push([prefix, after === undefined ? null : structuredClone(after)]);
}

/** Carry an edit of one entity (`before` -> `after`, its components) to
 *  `other`'s components, field by field: only the fields the edit changed
 *  are written, and only into components `other` has - or, when the edit
 *  added a whole component, that component is added to it too. A component
 *  the edit removed is removed from `other`. */
export function carryEdit(
  other: Record<string, unknown>,
  before: Record<string, unknown>,
  after: Record<string, unknown>,
): void {
  for (const key of new Set([...Object.keys(before), ...Object.keys(after)])) {
    const b = before[key];
    const a = after[key];
    if (JSON.stringify(a) === JSON.stringify(b)) continue;
    if (a === undefined) {
      delete other[key];
      continue;
    }
    if (b === undefined || other[key] === undefined || other[key] === null) {
      if (b === undefined) other[key] = structuredClone(a);
      continue;
    }
    const leaves: [string, unknown][] = [];
    changedPaths(key, b, a, leaves);
    for (const [path, value] of leaves) {
      if (value === null) {
        // A field the edit unset.
        const parts = path.split(".");
        const last = parts.pop()!;
        let cursor: unknown = other;
        for (const p of parts) cursor = cursor && typeof cursor === "object" ? (cursor as Record<string, unknown>)[p] : undefined;
        if (cursor && typeof cursor === "object") delete (cursor as Record<string, unknown>)[last];
      } else {
        setByPath(other, path, value);
      }
    }
  }
}

/** The fields of component `comp` whose values differ among `entities`
 *  (those that have it) - shown as "mixed" when several are selected. A
 *  field some have and others lack differs too. */
export function mixedFields(entities: { components: object }[], comp: string): string[] {
  const values = entities
    .map((e) => (e.components as Record<string, unknown>)[comp])
    .filter((c): c is Record<string, unknown> => !!c && typeof c === "object");
  if (values.length < 2) return [];
  const keys = new Set(values.flatMap((v) => Object.keys(v)));
  return [...keys]
    .filter((k) => {
      const first = JSON.stringify(values[0][k] ?? null);
      return values.some((v) => JSON.stringify(v[k] ?? null) !== first);
    })
    .sort();
}
