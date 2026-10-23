/**
 * Point lists the scene view edits by dragging: a polygon collider's
 * vertices, and every `Points` field the derived inspector shows - a line's
 * points, a spline shape's, a nav region's outline, a freeform light's, a
 * light occluder's polygon, a camera's confine area. One edit mode in
 * `Viewport.svelte` serves them all, addressed by the field's dotted path
 * into the entity's components.
 */

export type Point = [number, number];

/** Point lists that are in world units rather than the entity's own space.
 *  Everything else turns and moves with its entity. */
const WORLD_SPACE = new Set(["camera.confine"]);

/** The value at a dotted path, or undefined. */
export function getAtPath(root: unknown, path: string): unknown {
  let cursor = root;
  for (const seg of path.split(".")) {
    if (cursor === null || typeof cursor !== "object") return undefined;
    cursor = (cursor as Record<string, unknown>)[seg];
  }
  return cursor;
}

/** The path of a collider shape's vertex list, or null for a shape that has
 *  none (a box, a circle, a height field). */
export function colliderPointsPath(shape: unknown): string | null {
  if (!shape || typeof shape !== "object") return null;
  for (const kind of ["ConvexPolygon", "Polyline", "ConvexDecomposition"]) {
    if (kind in shape) return `collider.shape.${kind}.vertices`;
  }
  return null;
}

export interface PointListInfo {
  /** Whether the last point joins the first. */
  closed: boolean;
  /** The fewest points the list may be left with. */
  min: number;
  /** In world units, not the entity's space. */
  world: boolean;
}

/** How to draw and edit the list at `path` on `components`: a collider's
 *  polyline and any component whose `closed` flag is off are open lines;
 *  everything else is a closed outline. */
export function pointListInfo(components: Record<string, unknown>, path: string): PointListInfo {
  const component = path.split(".")[0];
  let closed = true;
  if (path.startsWith("collider.shape.Polyline")) closed = false;
  else {
    const flag = getAtPath(components, `${component}.closed`);
    if (typeof flag === "boolean") closed = flag;
  }
  return { closed, min: closed ? 3 : 2, world: WORLD_SPACE.has(path) };
}

/** The points at `path`, or an empty list when the field is absent (a list
 *  saved only when not empty). */
export function readPoints(components: Record<string, unknown>, path: string): Point[] {
  const v = getAtPath(components, path);
  return Array.isArray(v) ? (v as Point[]) : [];
}

/** The list at `path` to change in place, created (with any missing parent)
 *  when absent. */
export function pointsToEdit(components: Record<string, unknown>, path: string): Point[] {
  const segs = path.split(".");
  let cursor = components as Record<string, unknown>;
  for (const seg of segs.slice(0, -1)) {
    if (cursor[seg] === null || typeof cursor[seg] !== "object") cursor[seg] = {};
    cursor = cursor[seg] as Record<string, unknown>;
  }
  const last = segs[segs.length - 1];
  if (!Array.isArray(cursor[last])) cursor[last] = [];
  return cursor[last] as Point[];
}

/** Where a new point goes: into the edge it lands nearest (so a click on an
 *  outline adds a corner there, not at the end), or at the end when the list
 *  is too short to have edges or the click is far from all of them. Returns
 *  the index to insert at. `closed` counts the edge back to the first point;
 *  an open line past either end grows from that end. */
export function insertionIndex(points: Point[], p: Point, closed: boolean, reach = Infinity): number {
  const edges = closed ? points.length : points.length - 1;
  if (points.length < 2) return points.length;
  let best = points.length;
  let bestDist = reach;
  for (let i = 0; i < edges; i++) {
    const a = points[i];
    const b = points[(i + 1) % points.length];
    const dx = b[0] - a[0];
    const dy = b[1] - a[1];
    const len2 = dx * dx + dy * dy;
    const t = len2 > 0 ? Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2)) : 0;
    const d = Math.hypot(p[0] - (a[0] + dx * t), p[1] - (a[1] + dy * t));
    if (d < bestDist) {
      bestDist = d;
      // An open line grows from whichever end the point is beyond.
      if (!closed && i === 0 && t === 0) best = 0;
      else if (!closed && i === edges - 1 && t === 1) best = points.length;
      else best = i + 1;
    }
  }
  return best;
}
