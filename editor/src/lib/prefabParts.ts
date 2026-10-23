/**
 * A prefab's **parts** (Rust `EntityData::children`) in the editor: the
 * prefab editor's part tree, and a placed instance's parts in a scene.
 *
 * In a scene, the engine hangs an instance's parts out as entities named
 * `Cart/Wheel` (`#2` for a second of one name) with `parent` the instance
 * (`scene::prefab::flatten_children`), a part that is itself an instance
 * resolved through its prefab first. An authored scene entity of a part's
 * name and parent is **laid over** that part - merged over it as an instance
 * is over its prefab. The engine reads that entity's components typed, so
 * each must be whole: an overlay holds a whole component for every one that
 * differs from the part (`diffOverlay`), never a partial one.
 *
 * `instanceParts` names parts the engine's way; both are checked on one
 * fixture (`crates/jengine-core/tests/fixtures/prefab_parts.json`,
 * `prefab_parts_match_the_editor`).
 *
 * Every function returns new data; none mutates its argument.
 */
import { PART_KEY, resolveInstance, type Components, type PrefabEntity, type PrefabInstanceData } from "./prefabInstance";

export { PART_KEY, resolveOverlay, diffOverlay } from "./prefabInstance";

const clone = <T>(v: T): T => (v === undefined ? v : structuredClone(v));

// ─── An instance's parts in a scene ─────────────────────────────────────

/** One part of a placed instance, as the scene will hold it. */
export interface ScenePart {
  /** `Cart/Wheel`, `Cart/Wheel#2`, `Cart/Mast/Flag`. */
  name: string;
  /** The instance, or the part above. */
  parent: string;
  /** The part's components before any overlay. */
  components: Components;
  /** The instance it belongs to (the scene entity's name). */
  instance: string;
}

/** A part that is an instance itself, resolved through its prefab: its
 *  components merged, and the prefab's parts before its own. */
function expandNode(node: PrefabEntity, prefabs: Map<string, PrefabEntity>, depth: number): PrefabEntity {
  const id = (node.components?.prefab_instance as PrefabInstanceData | undefined)?.prefab_id;
  const template = id && depth < 8 ? prefabs.get(id) : undefined;
  if (!template) return node;
  return {
    ...node,
    components: resolveInstance(template.components, node.components),
    children: [...(template.children ?? []), ...(node.children ?? [])],
  };
}

/** The parts of the instance `instanceName` of prefab `prefab` (variant
 *  resolved), named and parented as the engine hangs them. */
export function instanceParts(instanceName: string, prefab: PrefabEntity, prefabs: Map<string, PrefabEntity>): ScenePart[] {
  const out: ScenePart[] = [];
  const hang = (parent: string, children: PrefabEntity[], depth: number) => {
    const seen = new Map<string, number>();
    for (const raw of children) {
      const child = expandNode(raw, prefabs, depth);
      const n = (seen.get(child.name) ?? 0) + 1;
      seen.set(child.name, n);
      const own = n === 1 ? child.name : `${child.name}#${n}`;
      const name = `${parent}/${own}`;
      out.push({ name, parent, components: clone(child.components ?? {}), instance: instanceName });
      hang(name, child.children ?? [], depth + 1);
    }
  };
  hang(instanceName, prefab.children ?? [], 1);
  return out;
}

/** The prefab map, plus every part of every instance in `entities` under
 *  `part:<name>` - what `viewOf` / `editThroughInstance` resolve an overlay
 *  entity by. */
export function withSceneParts(
  prefabs: Map<string, PrefabEntity>,
  entities: { name: string; components: object }[],
): Map<string, PrefabEntity> {
  const out = new Map(prefabs);
  // Parts of instances placed in the scene, and of parts that are instances
  // (those are expanded by `instanceParts` already).
  for (const e of entities) {
    const id = ((e.components as Components).prefab_instance as PrefabInstanceData | undefined)?.prefab_id;
    const prefab = id ? prefabs.get(id) : undefined;
    if (!prefab?.children?.length) continue;
    for (const p of instanceParts(e.name, prefab, prefabs)) {
      out.set(PART_KEY + p.name, { name: p.name, parent: p.parent, components: p.components });
    }
  }
  return out;
}

/** The parts of every instance in `entities` that no scene entity lays
 *  over yet - the scene tree's rows for them. */
export function unclaimedParts(
  prefabs: Map<string, PrefabEntity>,
  entities: { name: string; parent?: string | null; components: object }[],
): ScenePart[] {
  const have = new Set(entities.map((e) => `${e.parent ?? ""}\u0000${e.name}`));
  const out: ScenePart[] = [];
  for (const e of entities) {
    const id = ((e.components as Components).prefab_instance as PrefabInstanceData | undefined)?.prefab_id;
    const prefab = id ? prefabs.get(id) : undefined;
    if (!prefab?.children?.length) continue;
    for (const p of instanceParts(e.name, prefab, prefabs)) {
      if (!have.has(`${p.parent}\u0000${p.name}`)) out.push(p);
    }
  }
  return out;
}

// ─── A prefab's part tree ───────────────────────────────────────────────

/** A part's place: child indices from the root (`[]` is the root). */
export type PartPath = number[];

export function partAt(root: PrefabEntity, path: PartPath): PrefabEntity | undefined {
  let at: PrefabEntity | undefined = root;
  for (const i of path) at = at?.children?.[i];
  return at;
}

/** The root with the part at `path` replaced by `fn`'s result. */
export function replacePart(root: PrefabEntity, path: PartPath, fn: (part: PrefabEntity) => PrefabEntity): PrefabEntity {
  if (path.length === 0) return fn(clone(root));
  const next = clone(root);
  let parent = next;
  for (const i of path.slice(0, -1)) parent = parent.children![i];
  const last = path[path.length - 1];
  parent.children![last] = fn(parent.children![last]);
  return next;
}

/** A name none of `siblings` has: `base`, `base 2`, ... */
export function freshPartName(siblings: PrefabEntity[], base = "Part"): string {
  const have = new Set(siblings.map((s) => s.name));
  if (!have.has(base)) return base;
  for (let n = 2; ; n++) if (!have.has(`${base} ${n}`)) return `${base} ${n}`;
}

/** A new empty part under `path`; returns the root and the new part's path. */
export function addPart(root: PrefabEntity, path: PartPath, name?: string): { root: PrefabEntity; path: PartPath } {
  let at = 0;
  const next = replacePart(root, path, (p) => {
    const children = p.children ?? [];
    at = children.length;
    return {
      ...p,
      children: [
        ...children,
        {
          name: name ?? freshPartName(children),
          components: { transform: { position: [0, 0], rotation: 0, scale: [1, 1] } },
        },
      ],
    };
  });
  return { root: next, path: [...path, at] };
}

/** Remove the part at `path` (not the root), and its parts with it. */
export function removePart(root: PrefabEntity, path: PartPath): PrefabEntity {
  if (path.length === 0) return root;
  return replacePart(root, path.slice(0, -1), (p) => ({
    ...p,
    children: (p.children ?? []).filter((_, i) => i !== path[path.length - 1]),
  }));
}

/** A copy of the part at `path` beside it; returns the copy's path. */
export function duplicatePart(root: PrefabEntity, path: PartPath): { root: PrefabEntity; path: PartPath } {
  if (path.length === 0) return { root, path };
  const at = path[path.length - 1];
  const next = replacePart(root, path.slice(0, -1), (p) => {
    const children = [...(p.children ?? [])];
    const copy = clone(children[at]);
    copy.name = freshPartName(children, copy.name);
    children.splice(at + 1, 0, copy);
    return { ...p, children };
  });
  return { root: next, path: [...path.slice(0, -1), at + 1] };
}

/** Move the part at `path` by `by` among its siblings (draw order: a later
 *  part is spawned, and so sorted, after). Returns its new path. */
export function movePart(root: PrefabEntity, path: PartPath, by: number): { root: PrefabEntity; path: PartPath } {
  if (path.length === 0) return { root, path };
  const at = path[path.length - 1];
  const siblings = partAt(root, path.slice(0, -1))?.children ?? [];
  const to = Math.max(0, Math.min(siblings.length - 1, at + by));
  if (to === at) return { root, path };
  const next = replacePart(root, path.slice(0, -1), (p) => {
    const children = [...(p.children ?? [])];
    const [moved] = children.splice(at, 1);
    children.splice(to, 0, moved);
    return { ...p, children };
  });
  return { root: next, path: [...path.slice(0, -1), to] };
}

/** Rename a part; refused (the root unchanged) for an empty name or one a
 *  sibling has, since the scene finds parts by name. */
export function renamePart(root: PrefabEntity, path: PartPath, name: string): PrefabEntity {
  const trimmed = name.trim();
  if (!trimmed || path.length === 0) return root;
  const siblings = partAt(root, path.slice(0, -1))?.children ?? [];
  if (siblings.some((s, i) => i !== path[path.length - 1] && s.name === trimmed)) return root;
  return replacePart(root, path, (p) => ({ ...p, name: trimmed }));
}

/** Every part, parents first, with its path and depth (the tree's rows). */
export function partRows(root: PrefabEntity): { path: PartPath; depth: number; part: PrefabEntity }[] {
  const out: { path: PartPath; depth: number; part: PrefabEntity }[] = [];
  const walk = (p: PrefabEntity, path: PartPath) => {
    out.push({ path, depth: path.length, part: p });
    (p.children ?? []).forEach((c, i) => walk(c, [...path, i]));
  };
  walk(root, []);
  return out;
}

// ─── Where parts sit ────────────────────────────────────────────────────

/** A 2D affine `[a, b, c, d, tx, ty]`. */
export type Affine = [number, number, number, number, number, number];
const IDENTITY: Affine = [1, 0, 0, 1, 0, 0];

function mul(p: Affine, q: Affine): Affine {
  return [
    p[0] * q[0] + p[2] * q[1],
    p[1] * q[0] + p[3] * q[1],
    p[0] * q[2] + p[2] * q[3],
    p[1] * q[2] + p[3] * q[3],
    p[0] * q[4] + p[2] * q[5] + p[4],
    p[1] * q[4] + p[3] * q[5] + p[5],
  ];
}
function invert(m: Affine): Affine {
  const det = m[0] * m[3] - m[2] * m[1];
  if (Math.abs(det) < 1e-12) return IDENTITY;
  const a = m[3] / det;
  const b = -m[1] / det;
  const c = -m[2] / det;
  const d = m[0] / det;
  return [a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])];
}

/** A part's own transform as an affine (translate, rotate, scale). */
export function localFrame(part: PrefabEntity): Affine {
  const t = (part.components?.transform ?? {}) as { position?: number[]; rotation?: number; scale?: number[] };
  const [x, y] = t.position ?? [0, 0];
  const r = t.rotation ?? 0;
  const [sx, sy] = t.scale ?? [1, 1];
  const cs = Math.cos(r);
  const sn = Math.sin(r);
  return [cs * sx, sn * sx, -sn * sy, cs * sy, x, y];
}

/**
 * Every part's frame in the space the part at `focus` is placed in - its
 * parent's (prefab space for the root). The prefab editor's stage draws in
 * that space, so the focused part's gizmo is its own transform, unchanged,
 * and every other part is drawn where it sits around it.
 */
export function framesIn(root: PrefabEntity, focus: PartPath): { path: PartPath; frame: Affine; part: PrefabEntity }[] {
  const world = new Map<string, Affine>();
  const out: { path: PartPath; frame: Affine; part: PrefabEntity }[] = [];
  for (const row of partRows(root)) {
    const parent = row.path.length === 0 ? IDENTITY : world.get(row.path.slice(0, -1).join("/"))!;
    const frame = mul(parent, localFrame(row.part));
    world.set(row.path.join("/"), frame);
    out.push({ path: row.path, frame, part: row.part });
  }
  const space = focus.length === 0 ? IDENTITY : (world.get(focus.slice(0, -1).join("/")) ?? IDENTITY);
  const into = invert(space);
  return out.map((o) => ({ ...o, frame: mul(into, o.frame) }));
}
