/**
 * Rigging a picture: a mesh over a sprite's opaque pixels, each vertex
 * weighted to the bones near it, written as the `skinned_mesh` the engine
 * draws (Rust `animation::skinned_mesh`). The skin editor
 * (`SkinEditor.svelte`) does every step through these functions.
 *
 * Spaces: the picture is in **pixels** (y down, as images are); the mesh and
 * the bones are in the **skeleton root's** space (y up). `Placement` maps
 * one to the other. A vertex's weights are keyed by bone **name**, so bones
 * can be added, removed and moved after painting; offsets - each vertex in
 * each influencing bone's bind frame, which is what the engine skins with -
 * are worked out only when the mesh is written.
 *
 * Every function returns new data; none mutates its argument.
 */
import { IDENTITY, apply, mul, type Affine, type SkelBone } from "./skelEdit";

/** Where the picture sits in root space: its top-left corner, and the
 *  size of one pixel in world units. */
export interface Placement {
  x: number;
  y: number;
  scale: number;
}

export type Weights = Record<string, number>;

export interface SkinMesh {
  /** Root-space positions. */
  vertices: [number, number][];
  uvs: [number, number][];
  /** Three a triangle, counter-clockwise in root space. */
  indices: number[];
  weights: Weights[];
}

export function pixelToRoot(p: Placement, px: number, py: number): [number, number] {
  return [p.x + px * p.scale, p.y - py * p.scale];
}
export function rootToPixel(p: Placement, x: number, y: number): [number, number] {
  return [(x - p.x) / p.scale, (p.y - y) / p.scale];
}

/** A picture centred on the root, one unit a pixel. */
export function centredPlacement(width: number, height: number, scale = 1): Placement {
  return { x: (-width / 2) * scale, y: (height / 2) * scale, scale };
}

// ─── The mesh ───────────────────────────────────────────────────────────

/**
 * A grid mesh over the picture's opaque pixels: the picture cut into
 * `cell`-pixel squares, every square holding a pixel whose alpha is over
 * `threshold` kept as two triangles, corners shared. A kept square covers
 * all its pixels, so the shape is never cut into - the texture's own alpha
 * draws the outline - and a regular grid bends evenly at every joint.
 * `rgba` is the picture's pixels, four bytes each.
 */
export function gridMesh(
  rgba: ArrayLike<number>,
  width: number,
  height: number,
  cell: number,
  threshold: number,
  place: Placement,
): SkinMesh {
  const c = Math.max(2, Math.round(cell));
  const cols = Math.ceil(width / c);
  const rows = Math.ceil(height / c);
  const solid = (cx: number, cy: number) => {
    const x1 = Math.min(width, (cx + 1) * c);
    const y1 = Math.min(height, (cy + 1) * c);
    for (let y = cy * c; y < y1; y++) {
      for (let x = cx * c; x < x1; x++) {
        if (rgba[(y * width + x) * 4 + 3] > threshold) return true;
      }
    }
    return false;
  };
  const corner = new Map<number, number>();
  const mesh: SkinMesh = { vertices: [], uvs: [], indices: [], weights: [] };
  const vertex = (gx: number, gy: number) => {
    const key = gy * (cols + 1) + gx;
    let i = corner.get(key);
    if (i === undefined) {
      const px = Math.min(gx * c, width);
      const py = Math.min(gy * c, height);
      i = mesh.vertices.length;
      corner.set(key, i);
      mesh.vertices.push(pixelToRoot(place, px, py));
      mesh.uvs.push([px / width, py / height]);
      mesh.weights.push({});
    }
    return i;
  };
  for (let cy = 0; cy < rows; cy++) {
    for (let cx = 0; cx < cols; cx++) {
      if (!solid(cx, cy)) continue;
      const tl = vertex(cx, cy);
      const tr = vertex(cx + 1, cy);
      const bl = vertex(cx, cy + 1);
      const br = vertex(cx + 1, cy + 1);
      // Counter-clockwise with y up: bottom-left, bottom-right, top-right.
      mesh.indices.push(bl, br, tr, bl, tr, tl);
    }
  }
  return mesh;
}

// ─── Weights ────────────────────────────────────────────────────────────

/** Distance from `p` to the segment `a`-`b`. */
export function segmentDistance(p: [number, number], a: [number, number], b: [number, number]): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const len2 = dx * dx + dy * dy;
  const f = len2 > 0 ? Math.min(1, Math.max(0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2)) : 0;
  return Math.hypot(p[0] - (a[0] + dx * f), p[1] - (a[1] + dy * f));
}

/** A bone as a segment in root space, for weighting and picking. */
export interface BoneSegment {
  name: string;
  origin: [number, number];
  tip: [number, number];
}

/**
 * Weights by distance: each vertex leans on its `influences` nearest bones,
 * each by `1 / d^power` (the nearest bone gets most of it, the next eases in
 * near a joint), normalised to 1 and with weights under 2% dropped.
 */
export function autoWeights(vertices: [number, number][], bones: BoneSegment[], influences = 2, power = 3): Weights[] {
  if (bones.length === 0) return vertices.map(() => ({}));
  return vertices.map((v) => {
    const near = bones
      .map((b) => ({ name: b.name, d: segmentDistance(v, b.origin, b.tip) }))
      .sort((a, b) => a.d - b.d)
      .slice(0, Math.max(1, influences));
    // A vertex on a bone is that bone's alone.
    if (near[0].d < 1e-6) return { [near[0].name]: 1 };
    const raw = near.map((n) => ({ name: n.name, w: 1 / Math.pow(n.d, power) }));
    return normalise(Object.fromEntries(raw.map((r) => [r.name, r.w])));
  });
}

/** Weights summing to 1, the smallest (under 2%) dropped. */
export function normalise(w: Weights, floor = 0.02): Weights {
  let sum = Object.values(w).reduce((s, x) => s + Math.max(0, x), 0);
  if (sum <= 0) return {};
  const kept = Object.entries(w).filter(([, x]) => x / sum >= floor);
  sum = kept.reduce((s, [, x]) => s + x, 0);
  return Object.fromEntries(kept.map(([k, x]) => [k, x / sum]));
}

/**
 * Paint `bone`'s weight at the vertices `hits` (each with a falloff 0..1)
 * up by `amount` (or down, negative), the others scaled to fill the rest.
 * A vertex whose only bone is painted down hands the weight to `heir(i)` -
 * the nearest other bone - as weight has to go somewhere.
 */
export function paintWeights(
  weights: Weights[],
  hits: { i: number; f: number }[],
  bone: string,
  amount: number,
  heir: (i: number) => string | null,
): Weights[] {
  const out = weights.slice();
  for (const { i, f } of hits) {
    const w = { ...(weights[i] ?? {}) };
    const cur = w[bone] ?? 0;
    const target = amount >= 0 ? cur + (1 - cur) * Math.min(1, amount * f) : cur * (1 - Math.min(1, -amount * f));
    const others = Object.entries(w).filter(([k]) => k !== bone);
    const rest = others.reduce((s, [, x]) => s + x, 0);
    const next: Weights = {};
    if (rest > 1e-9) {
      for (const [k, x] of others) next[k] = (x / rest) * (1 - target);
    } else if (target < 1) {
      const h = heir(i);
      if (h && h !== bone) next[h] = 1 - target;
      else {
        out[i] = w;
        continue;
      }
    }
    if (target > 1e-4) next[bone] = target;
    out[i] = normalise(next, 0.005);
  }
  return out;
}

/** Each vertex's weights averaged with its neighbours' by `amount` - to
 *  soften a seam a brush left. */
export function smoothWeights(weights: Weights[], indices: number[], hits: { i: number; f: number }[], amount: number): Weights[] {
  const neighbours = new Map<number, Set<number>>();
  for (let t = 0; t + 2 < indices.length; t += 3) {
    const tri = [indices[t], indices[t + 1], indices[t + 2]];
    for (const a of tri) for (const b of tri) if (a !== b) (neighbours.get(a) ?? neighbours.set(a, new Set()).get(a)!).add(b);
  }
  const out = weights.slice();
  for (const { i, f } of hits) {
    const ns = [...(neighbours.get(i) ?? [])];
    if (ns.length === 0) continue;
    const avg: Weights = {};
    for (const n of ns) for (const [k, x] of Object.entries(weights[n] ?? {})) avg[k] = (avg[k] ?? 0) + x / ns.length;
    const k = Math.min(1, amount * f);
    const mixed: Weights = {};
    for (const name of new Set([...Object.keys(weights[i] ?? {}), ...Object.keys(avg)])) {
      mixed[name] = (weights[i]?.[name] ?? 0) * (1 - k) + (avg[name] ?? 0) * k;
    }
    out[i] = normalise(mixed, 0.005);
  }
  return out;
}

/** The vertices within `radius` of `p`, each with a smooth falloff. */
export function brushHits(vertices: [number, number][], p: [number, number], radius: number): { i: number; f: number }[] {
  const out: { i: number; f: number }[] = [];
  vertices.forEach((v, i) => {
    const d = Math.hypot(v[0] - p[0], v[1] - p[1]);
    if (d <= radius) {
      const x = d / radius;
      out.push({ i, f: 1 - x * x * (3 - 2 * x) });
    }
  });
  return out;
}

/** Remove a bone from every vertex, its weight going to `heir`. */
export function handOver(weights: Weights[], bone: string, heir: string | null): Weights[] {
  return weights.map((w) => {
    if (!(bone in w)) return w;
    const next = { ...w };
    const x = next[bone];
    delete next[bone];
    if (heir) next[heir] = (next[heir] ?? 0) + x;
    return normalise(next, 0.005);
  });
}

/** Rename a bone in every vertex's weights. */
export function renameWeights(weights: Weights[], from: string, to: string): Weights[] {
  return weights.map((w) => {
    if (!(from in w)) return w;
    const next = { ...w, [to]: (w[to] ?? 0) + w[from] };
    delete next[from];
    return next;
  });
}

// ─── Skinning ───────────────────────────────────────────────────────────

/** Each bone's frame in root space, by name. */
export function boneFrames(skel: SkelBone[]): Map<string, Affine> {
  const frames: Affine[] = [];
  const out = new Map<string, Affine>();
  skel.forEach((b, i) => {
    const cs = Math.cos(b.local.rotation);
    const sn = Math.sin(b.local.rotation);
    const local: Affine = [
      cs * b.local.scale[0],
      sn * b.local.scale[0],
      -sn * b.local.scale[1],
      cs * b.local.scale[1],
      b.local.position[0],
      b.local.position[1],
    ];
    frames[i] = mul(b.parent >= 0 ? frames[b.parent] : IDENTITY, local);
    out.set(b.name, frames[i]);
  });
  return out;
}

export function invert(m: Affine): Affine {
  const det = m[0] * m[3] - m[2] * m[1];
  if (Math.abs(det) < 1e-12) return IDENTITY;
  const a = m[3] / det;
  const b = -m[1] / det;
  const c = -m[2] / det;
  const d = m[0] / det;
  return [a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])];
}

/** The mesh as the bones in `posed` bend it from the bind pose `bind` -
 *  the sum the engine works out (`compute_skinned_positions`). A vertex
 *  none of whose bones exists stays where it is. */
export function skin(mesh: SkinMesh, bind: Map<string, Affine>, posed: Map<string, Affine>): [number, number][] {
  return mesh.vertices.map((v, i) => {
    let x = 0;
    let y = 0;
    let total = 0;
    for (const [name, w] of Object.entries(mesh.weights[i] ?? {})) {
      const b = bind.get(name);
      const p = posed.get(name);
      if (!b || !p) continue;
      const off = apply(invert(b), v[0], v[1]);
      const q = apply(p, off[0], off[1]);
      x += q[0] * w;
      y += q[1] * w;
      total += w;
    }
    return total > 1e-6 ? [x / total, y / total] : v;
  });
}

// ─── The component ──────────────────────────────────────────────────────

export interface SkinnedMeshData {
  texture_id: string;
  color?: [number, number, number, number];
  z_order?: number;
  bones: string[];
  vertices: {
    rest_position: [number, number];
    uv: [number, number];
    influences: { bone_index: number; weight: number; offset: [number, number] }[];
  }[];
  indices: number[];
}

const r4 = (x: number) => Math.round(x * 1e4) / 1e4;

/**
 * The `skinned_mesh` the engine draws: every bone named in `order`, each
 * vertex's weights (at most `maxInfluences`, renormalised) with its offset
 * in each bone's bind frame. A vertex left with no weight is pinned to the
 * nearest bone in `fallback`, so nothing in the mesh is left behind at its
 * rest position when the skeleton moves.
 */
export function toSkinnedMesh(
  mesh: SkinMesh,
  skel: SkelBone[],
  texture: string,
  keep: Partial<SkinnedMeshData> = {},
  maxInfluences = 4,
): SkinnedMeshData {
  const frames = boneFrames(skel);
  const order = skel.map((b) => b.name);
  const index = new Map(order.map((n, i) => [n, i]));
  const inverse = new Map([...frames].map(([n, m]) => [n, invert(m)]));
  const segments = segmentsOf(skel);
  const vertices = mesh.vertices.map((v, i) => {
    let w = Object.entries(mesh.weights[i] ?? {}).filter(([n, x]) => index.has(n) && x > 0);
    if (w.length === 0 && segments.length > 0) {
      const near = segments.reduce((a, b) => (segmentDistance(v, b.origin, b.tip) < segmentDistance(v, a.origin, a.tip) ? b : a));
      w = [[near.name, 1]];
    }
    w = w.sort((a, b) => b[1] - a[1]).slice(0, maxInfluences);
    const sum = w.reduce((s, [, x]) => s + x, 0) || 1;
    return {
      rest_position: [r4(v[0]), r4(v[1])] as [number, number],
      uv: [r4(mesh.uvs[i][0]), r4(mesh.uvs[i][1])] as [number, number],
      influences: w.map(([n, x]) => {
        const off = apply(inverse.get(n)!, v[0], v[1]);
        return { bone_index: index.get(n)!, weight: r4(x / sum), offset: [r4(off[0]), r4(off[1])] as [number, number] };
      }),
    };
  });
  return {
    color: [1, 1, 1, 1],
    z_order: 0,
    ...keep,
    texture_id: texture,
    bones: order,
    vertices,
    indices: mesh.indices.slice(),
  };
}

/** A skeleton's bones as segments in root space. */
export function segmentsOf(skel: SkelBone[]): BoneSegment[] {
  const frames = boneFrames(skel);
  return skel.map((b) => {
    const m = frames.get(b.name)!;
    return { name: b.name, origin: [m[4], m[5]], tip: apply(m, b.length, 0) };
  });
}

/** An engine `skinned_mesh` read back for editing: each vertex where the
 *  skeleton's bind pose puts it, its weights by bone name. */
export function fromSkinnedMesh(data: SkinnedMeshData, skel: SkelBone[]): SkinMesh {
  const frames = boneFrames(skel);
  const vertices: [number, number][] = [];
  const weights: Weights[] = [];
  for (const v of data.vertices ?? []) {
    let x = 0;
    let y = 0;
    let total = 0;
    const w: Weights = {};
    for (const inf of v.influences ?? []) {
      const name = data.bones?.[inf.bone_index];
      if (name === undefined) continue;
      w[name] = (w[name] ?? 0) + inf.weight;
      const m = frames.get(name);
      if (!m) continue;
      const q = apply(m, inf.offset[0], inf.offset[1]);
      x += q[0] * inf.weight;
      y += q[1] * inf.weight;
      total += inf.weight;
    }
    vertices.push(total > 1e-6 ? [x / total, y / total] : [v.rest_position[0], v.rest_position[1]]);
    weights.push(normalise(w, 0));
  }
  return { vertices, uvs: (data.vertices ?? []).map((v) => [v.uv[0], v.uv[1]]), indices: (data.indices ?? []).slice(), weights };
}

/** Where a mesh's picture sits, worked out from its vertices and UVs (a
 *  least-squares fit of `x = x0 + u w s`, `y = y0 - v h s`), so a mesh
 *  made elsewhere can be redrawn over its picture. */
export function placementOf(mesh: SkinMesh, width: number, height: number): Placement | null {
  const n = mesh.vertices.length;
  if (n < 2) return null;
  // Fit s and the offsets: x - u w s = x0, y + v h s = y0.
  let su = 0;
  let sx = 0;
  let suu = 0;
  let sux = 0;
  let sv = 0;
  let sy = 0;
  let svv = 0;
  let svy = 0;
  mesh.vertices.forEach((p, i) => {
    const u = mesh.uvs[i][0] * width;
    const v = -mesh.uvs[i][1] * height;
    su += u;
    sx += p[0];
    suu += u * u;
    sux += u * p[0];
    sv += v;
    sy += p[1];
    svv += v * v;
    svy += v * p[1];
  });
  // One scale for both axes: minimise both residuals together.
  const num = n * (sux + svy) - (su * sx + sv * sy);
  const den = n * (suu + svv) - (su * su + sv * sv);
  if (Math.abs(den) < 1e-9) return null;
  const s = num / den;
  if (!(s > 0)) return null;
  return { scale: s, x: (sx - s * su) / n, y: (sy - s * sv) / n };
}
