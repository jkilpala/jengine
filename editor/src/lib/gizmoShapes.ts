/**
 * What the scene view draws over a selected entity besides its collider:
 * the reach of its lights, the range of its sound, its area effect's box,
 * its nav region and light outlines, its camera's confine area. All in world
 * units here; the viewport projects them (`worldToLocal`) and draws them as
 * SVG, read-only - the point lists among them are dragged with the points
 * editor (`pointFields.ts`).
 */
import { toSrgb } from "./curves";

export type Point = [number, number];

/** An entity's resolved world transform, as `worldTransformOf` gives it. */
export interface WorldXf {
  x: number;
  y: number;
  rotation: number;
  scaleX: number;
  scaleY: number;
}

export type Gizmo =
  | { kind: "circle"; center: Point; radius: number; color: string; dashed?: boolean; label?: string }
  | { kind: "path"; points: Point[]; closed: boolean; color: string; dashed?: boolean; label?: string };

/** A point in the entity's space, placed in the world: scaled, turned,
 *  moved - the order the engine composes a transform in. */
export function placeLocal(p: Point, xf: WorldXf): Point {
  const sx = p[0] * xf.scaleX;
  const sy = p[1] * xf.scaleY;
  const c = Math.cos(xf.rotation);
  const s = Math.sin(xf.rotation);
  return [xf.x + sx * c - sy * s, xf.y + sx * s + sy * c];
}

/** A linear RGB colour as CSS, the way the screen shows it. */
function css(rgb: unknown, fallback: string): string {
  if (!Array.isArray(rgb) || rgb.length < 3) return fallback;
  const [r, g, b] = rgb.slice(0, 3).map((c) => Math.round(toSrgb(Number(c) || 0) * 255));
  return `rgb(${r}, ${g}, ${b})`;
}

function points(list: unknown): Point[] {
  return Array.isArray(list) ? list.filter((p) => Array.isArray(p) && p.length >= 2).map((p) => [Number(p[0]), Number(p[1])]) : [];
}

/** A box `w` x `h` about the entity's origin, placed in the world. */
function box(w: number, h: number, xf: WorldXf): Point[] {
  return (
    [
      [-w / 2, -h / 2],
      [w / 2, -h / 2],
      [w / 2, h / 2],
      [-w / 2, h / 2],
    ] as Point[]
  ).map((p) => placeLocal(p, xf));
}

const ARC_STEPS = 24;

/** The gizmos for one entity's components. */
export function gizmosFor(components: Record<string, unknown>, xf: WorldXf): Gizmo[] {
  const out: Gizmo[] = [];
  const at: Point = [xf.x, xf.y];
  const c = components as Record<string, Record<string, unknown> | undefined>;

  const point = c.point_light;
  if (point && Number(point.radius) > 0) {
    out.push({ kind: "circle", center: at, radius: Number(point.radius), color: css(point.color, "var(--ed-warn)"), label: "light" });
  }

  const spot = c.spot_light;
  if (spot && Number(spot.radius) > 0) {
    // Aimed by the entity's turn plus its own direction, as the renderer does.
    const aim = xf.rotation + (Number(spot.direction) || 0);
    const half = Math.max(0, Number(spot.angle) || 0);
    const r = Number(spot.radius);
    const arc: Point[] = [at];
    for (let i = 0; i <= ARC_STEPS; i++) {
      const a = aim - half + (2 * half * i) / ARC_STEPS;
      arc.push([at[0] + Math.cos(a) * r, at[1] + Math.sin(a) * r]);
    }
    out.push({ kind: "path", points: arc, closed: true, color: css(spot.color, "var(--ed-warn)"), label: "spot" });
  }

  const freeform = c.freeform_light;
  const freePts = points(freeform?.points);
  if (freeform && freePts.length >= 3) {
    out.push({ kind: "path", points: freePts.map((p) => placeLocal(p, xf)), closed: true, color: css(freeform.color, "var(--ed-warn)"), label: "light" });
  }

  const occluder = c.light_occluder;
  if (occluder && occluder.enabled !== false) {
    const poly = points(occluder.polygon);
    if (poly.length >= 3) {
      out.push({ kind: "path", points: poly.map((p) => placeLocal(p, xf)), closed: true, color: "var(--ed-muted)", dashed: true, label: "occluder" });
    } else if (!occluder.from_sprite && !occluder.from_alpha && Array.isArray(occluder.size)) {
      out.push({ kind: "path", points: box(Number(occluder.size[0]), Number(occluder.size[1]), xf), closed: true, color: "var(--ed-muted)", dashed: true, label: "occluder" });
    }
  }

  const audio = c.audio_source;
  if (audio?.spatial) {
    const min = Number(audio.min_distance) || 0;
    const max = Number(audio.max_distance) || 0;
    if (min > 0) out.push({ kind: "circle", center: at, radius: min, color: "var(--ed-a3)", label: "full volume" });
    if (max > 0) out.push({ kind: "circle", center: at, radius: max, color: "var(--ed-a3)", dashed: true, label: "silent" });
  }

  const area = c.area_effect;
  if (area && Array.isArray(area.size)) {
    out.push({ kind: "path", points: box(Number(area.size[0]), Number(area.size[1]), xf), closed: true, color: "var(--ed-a2)", label: "area" });
  }

  const nav = c.nav_region;
  const outline = points(nav?.outline);
  if (outline.length >= 3) {
    out.push({ kind: "path", points: outline.map((p) => placeLocal(p, xf)), closed: true, color: "var(--ed-ok)", dashed: true, label: "nav region" });
  }

  // A camera's confine area is in world units already.
  const confine = points(c.camera?.confine);
  if (confine.length >= 3) {
    out.push({ kind: "path", points: confine, closed: true, color: "var(--ed-accent)", dashed: true, label: "confine" });
  }
  return out;
}
