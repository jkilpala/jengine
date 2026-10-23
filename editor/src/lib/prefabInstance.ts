/**
 * Linked prefab instances in the editor.
 *
 * A scene entity carrying `prefab_instance: { prefab_id, overrides }` is an
 * instance: what it is, is the prefab's components, its own components over
 * them, then the dotted-path overrides - the merge `scene::prefab` does when a
 * scene loads (`merge_instance`, `merge_object`, `set_by_path`). The editor
 * keeps the entity in that form: the Inspector shows the **resolved** entity,
 * and an edit made to it is turned back into overrides (`diffEdit`), so the
 * prefab's later changes still reach every instance that did not override
 * them.
 *
 * The merge is written twice - here and in Rust - so both are checked against
 * one fixture, `crates/jengine-core/tests/fixtures/prefab_merge.json`
 * (`prefab_merge_matches_the_editor` in Rust, `prefabInstance.test.ts` here).
 * Change one and the other's test fails.
 *
 * One known difference: Rust round-trips an authored component through its
 * struct, which fills every field it omits with the default. The editor only
 * ever authors whole components, so the two agree on what it writes.
 */

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Components = Record<string, unknown>;

export interface PrefabInstanceData {
  prefab_id: string;
  overrides?: Record<string, unknown>;
}

export interface PrefabEntity {
  name: string;
  components: Components;
  children?: PrefabEntity[];
  parent?: string | null;
}

/** The id a scene's `prefab_instance` names a prefab by: its `name`, or its
 *  file's stem when it has none - the rule the runtimes key their prefab maps
 *  by (`load_prefab_map`). The editor used the stem alone. */
export function prefabKey(entry: { name: string; data?: unknown }): string {
  const own = (entry.data as { name?: unknown } | undefined)?.name;
  return typeof own === "string" && own !== "" ? own : entry.name;
}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

const clone = <T>(v: T): T => (v === undefined ? v : structuredClone(v));

/** `merge_object`: every key of `overlay` written into `base`; objects merge
 *  key by key, anything else replaces; a `null` (or absent) overlay value is
 *  skipped, so an unset component does not clobber the prefab's. */
export function mergeObject(base: Record<string, unknown>, overlay: Record<string, unknown>): void {
  for (const [key, value] of Object.entries(overlay)) {
    if (value === null || value === undefined) continue;
    const current = base[key];
    if (isObject(current) && isObject(value)) mergeObject(current, value);
    else base[key] = clone(value);
  }
}

/** `set_by_path`: assign at a dotted path. A number segment indexes an array
 *  (out of range is ignored); a missing object key on the way is created as
 *  `{}`; a path that meets a value it cannot descend into is ignored. */
export function setByPath(root: Record<string, unknown>, path: string, value: unknown): void {
  const segments = path.split(".");
  let cursor: unknown = root;
  for (let i = 0; i < segments.length; i++) {
    const seg = segments[i];
    const last = i === segments.length - 1;
    const index = /^\d+$/.test(seg) ? Number(seg) : NaN;
    if (Array.isArray(cursor) && !Number.isNaN(index)) {
      if (last) {
        if (index < cursor.length) cursor[index] = clone(value);
        return;
      }
      cursor = cursor[index];
      if (cursor === undefined) return;
      continue;
    }
    if (!isObject(cursor)) return;
    if (last) {
      cursor[seg] = clone(value);
      return;
    }
    if (!(seg in cursor)) cursor[seg] = {};
    cursor = cursor[seg];
  }
}

/** An instance's components: the template's, the entity's own over them,
 *  then the overrides; `prefab_instance` kept as the marker it is. */
export function resolveInstance(template: Components, entity: Components): Components {
  const instance = entity.prefab_instance as PrefabInstanceData | undefined;
  const merged = clone(template) ?? {};
  const authored: Components = { ...entity };
  delete authored.prefab_instance;
  mergeObject(merged, authored);
  for (const [path, value] of Object.entries(instance?.overrides ?? {})) {
    setByPath(merged, path, value);
  }
  // A component overridden to null is removed, as serde reads it as `None`.
  for (const [key, value] of Object.entries(merged)) {
    if (value === null) delete merged[key];
  }
  if (instance) merged.prefab_instance = clone(instance);
  return merged;
}

/** `resolve_prefab_variants`: a prefab whose components carry a
 *  `prefab_instance` extends the prefab it names - the base's components,
 *  its own over them, then its overrides; a variant of a variant works, and a
 *  missing base or a cycle leaves the prefab as written. Keyed by name. */
export function resolveVariants(prefabs: Map<string, PrefabEntity>): Map<string, PrefabEntity> {
  const done = new Map<string, PrefabEntity>();
  const resolve = (name: string, visiting: string[]): PrefabEntity | undefined => {
    const hit = done.get(name);
    if (hit) return hit;
    const raw = prefabs.get(name);
    if (!raw) return undefined;
    const instance = raw.components.prefab_instance as PrefabInstanceData | undefined;
    if (!instance) {
      done.set(name, raw);
      return raw;
    }
    if (visiting.includes(name)) return raw;
    const base = resolve(instance.prefab_id, [...visiting, name]);
    const out = base
      ? {
          ...raw,
          components: resolveInstance(base.components, raw.components),
          children: [...(base.children ?? []), ...(raw.children ?? [])],
        }
      : raw;
    done.set(name, out);
    return out;
  };
  const out = new Map<string, PrefabEntity>();
  for (const name of prefabs.keys()) {
    const r = resolve(name, []);
    if (r) out.set(name, r);
  }
  return out;
}

// ─── Editing an instance ─────────────────────────────────────────────────

const equal = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** The leaf paths where `before` and `after` differ, below `prefix`: objects
 *  are walked key by key, anything else (arrays included) is one value. A key
 *  that is gone is reported with `null`, which unsets it. */
export function changedLeaves(prefix: string, before: unknown, after: unknown, out: [string, unknown][]): void {
  if (equal(before, after)) return;
  if (isObject(before) && isObject(after)) {
    for (const key of new Set([...Object.keys(before), ...Object.keys(after)])) {
      changedLeaves(`${prefix}.${key}`, before[key], after[key], out);
    }
    return;
  }
  out.push([prefix, after === undefined ? null : clone(after)]);
}

/** Write an override at `path`, keeping the set unambiguous: overrides are
 *  applied in no fixed order, so a path never sits beside one of its
 *  ancestors. Under an ancestor override the value goes into it; over
 *  descendants it replaces them. */
export function putOverride(overrides: Record<string, unknown>, path: string, value: unknown): void {
  for (const existing of Object.keys(overrides)) {
    if (path.startsWith(existing + ".")) {
      const holder: Record<string, unknown> = { v: overrides[existing] };
      setByPath(holder, `v.${path.slice(existing.length + 1)}`, value);
      overrides[existing] = holder.v;
      return;
    }
  }
  for (const existing of Object.keys(overrides)) {
    if (existing.startsWith(path + ".")) delete overrides[existing];
  }
  overrides[path] = clone(value);
}

/** Turn an edit of an instance's resolved components (`before` -> `after`)
 *  into edits of the entity as stored: a component the entity authors itself
 *  is written whole; a change to one the prefab supplies becomes an override
 *  at the changed field's path; a component added is authored; a component
 *  removed loses the entity's copy and, if the prefab has it, is overridden
 *  to `null`. Returns the new stored components. */
export function diffEdit(stored: Components, template: Components, before: Components, after: Components): Components {
  const out = clone(stored);
  const instance: PrefabInstanceData = clone(out.prefab_instance as PrefabInstanceData) ?? { prefab_id: "" };
  const overrides: Record<string, unknown> = { ...(instance.overrides ?? {}) };
  for (const key of new Set([...Object.keys(before), ...Object.keys(after)])) {
    if (key === "prefab_instance") continue;
    const b = before[key];
    const a = after[key];
    if (equal(a, b)) continue;
    const authored = stored[key] !== null && stored[key] !== undefined;
    if (a === undefined) {
      delete out[key];
      for (const path of Object.keys(overrides)) {
        if (path === key || path.startsWith(key + ".")) delete overrides[path];
      }
      if (template[key] !== undefined && template[key] !== null) overrides[key] = null;
      continue;
    }
    if (authored || b === undefined) {
      out[key] = clone(a);
      continue;
    }
    const leaves: [string, unknown][] = [];
    changedLeaves(key, b, a, leaves);
    for (const [path, value] of leaves) putOverride(overrides, path, value);
  }
  instance.overrides = overrides;
  if (Object.keys(overrides).length === 0) delete instance.overrides;
  out.prefab_instance = instance;
  return out;
}

/** Whether the field at `path` (`"sprite.color"`, or a whole `"sprite"`)
 *  differs from the prefab on this instance: the entity authors the
 *  component, or an override sits at the path, above it or below it. */
export function isOverridden(stored: Components, path: string): boolean {
  const component = path.split(".")[0];
  if (component in stored && component !== "prefab_instance" && stored[component] != null) return true;
  const overrides = (stored.prefab_instance as PrefabInstanceData | undefined)?.overrides ?? {};
  return Object.keys(overrides).some(
    (p) => p === path || path.startsWith(p + ".") || p.startsWith(path + "."),
  );
}

/** Undo the instance's change at `path`: the overrides at and below it go,
 *  and at a whole component the entity's own copy too, so the prefab's
 *  shows through again. An override *above* the path (a whole component
 *  overridden) has the path's value put back from the template. */
export function revertPath(stored: Components, template: Components, path: string): Components {
  const out = clone(stored);
  const instance: PrefabInstanceData = clone(out.prefab_instance as PrefabInstanceData) ?? { prefab_id: "" };
  const overrides: Record<string, unknown> = { ...(instance.overrides ?? {}) };
  for (const p of Object.keys(overrides)) {
    if (p === path || p.startsWith(path + ".")) delete overrides[p];
  }
  const ancestor = Object.keys(overrides).find((p) => path.startsWith(p + "."));
  if (ancestor) {
    const holder: Record<string, unknown> = { v: overrides[ancestor] };
    const fromTemplate = path.split(".").reduce<unknown>((v, seg) => (isObject(v) || Array.isArray(v) ? (v as Record<string, unknown>)[seg] : undefined), template);
    setByPath(holder, `v.${path.slice(ancestor.length + 1)}`, fromTemplate ?? null);
    overrides[ancestor] = holder.v;
  }
  if (!path.includes(".")) delete out[path];
  instance.overrides = overrides;
  if (Object.keys(overrides).length === 0) delete instance.overrides;
  out.prefab_instance = instance;
  return out;
}

/** A new instance of a prefab, as a scene stores it: a link to the prefab
 *  and its own transform - the prefab's, so it lands as authored - which is
 *  what the scene view's gizmo moves. Everything else comes from the prefab,
 *  so a later change to it reaches this instance. */
export function newInstance(prefabId: string, prefab: PrefabEntity | undefined): PrefabEntity {
  const transform = clone(prefab?.components.transform) ?? { position: [0, 0], rotation: 0, scale: [1, 1] };
  return {
    name: prefab?.name || prefabId,
    parent: null,
    components: { transform, prefab_instance: { prefab_id: prefabId } },
  };
}

/** The instance turned into a plain entity: the prefab's components merged
 *  in and the link dropped, so later changes to the prefab no longer reach
 *  it (Unity's *Unpack Prefab*). */
export function unpackInstance(template: Components, stored: Components): Components {
  const out = resolveInstance(template, stored);
  delete out.prefab_instance;
  return out;
}

/** The prefab, as written, with the instance's value at `path` made its own
 *  (Unity's *Apply to Prefab*). A variant takes it as one of its own
 *  overrides - written into its components, a part of a component would be
 *  filled with defaults by the engine and clobber its base's - and a plain
 *  prefab into its components; `null` at a whole component removes it. */
export function applyToPrefab(prefab: PrefabEntity, path: string, value: unknown): PrefabEntity {
  const out = clone(prefab);
  const variant = out.components.prefab_instance as PrefabInstanceData | undefined;
  if (variant) {
    const overrides = { ...(variant.overrides ?? {}) };
    putOverride(overrides, path, value);
    out.components.prefab_instance = { ...variant, overrides };
  } else if (value === null && !path.includes(".")) {
    delete out.components[path];
  } else {
    setByPath(out.components, path, value);
  }
  return out;
}

/** What an instance changes from its prefab, one row per thing to revert or
 *  apply: each component it authors itself (but its transform, which every
 *  instance has) and each override path. */
export function instanceChanges(stored: Components): { path: string; value: unknown; own: boolean }[] {
  const rows: { path: string; value: unknown; own: boolean }[] = [];
  for (const [key, value] of Object.entries(stored)) {
    if (key === "prefab_instance" || key === "transform" || value === null || value === undefined) continue;
    rows.push({ path: key, value, own: true });
  }
  const overrides = (stored.prefab_instance as PrefabInstanceData | undefined)?.overrides ?? {};
  for (const [path, value] of Object.entries(overrides)) rows.push({ path, value, own: false });
  return rows.sort((a, b) => a.path.localeCompare(b.path));
}

// ─── A part of a placed instance, laid over in a scene ───────────────────

/** Where a scene's instance parts are kept in a prefab map
 *  (`prefabParts.withSceneParts`): `part:Cart/Wheel`. */
export const PART_KEY = "part:";

/** A part as the scene shows it: its overlay's components merged over it,
 *  as the engine merges them (`flatten_children`). */
export function resolveOverlay(part: Components, overlay: Components): Components {
  const merged = clone(part) ?? {};
  mergeObject(merged, overlay);
  return merged;
}

/** The overlay that makes `part` look like `edited`: each component that
 *  differs, **whole** - the engine reads an overlay's components typed, so a
 *  partial one would not load. A component the part has and the edit
 *  dropped cannot be said by an overlay, so it stays. */
export function diffOverlay(part: Components, edited: Components): Components {
  const out: Components = {};
  for (const [key, value] of Object.entries(edited)) {
    if (value === null || value === undefined) continue;
    if (!equal(value, part[key])) out[key] = clone(value);
  }
  return out;
}

/** The part `entity` lays over, when it is a scene entity of a placed
 *  instance part's name and parent (from a map made by `withSceneParts`). */
export function partOf(entity: EntityLike, prefabs: Map<string, PrefabEntity>): PrefabEntity | undefined {
  const part = prefabs.get(PART_KEY + entity.name);
  return part && (entity.parent ?? null) === (part.parent ?? null) ? part : undefined;
}

/** Anything shaped like an entity - the scene's `EntityData` or a prefab. */
export interface EntityLike {
  name: string;
  parent?: string | null;
  components: object;
}

/** An entity as it is: a linked instance resolved through its prefab (from
 *  `prefabs`, the variant-resolved map), anything else - or an instance whose
 *  prefab is missing - as stored. */
export function viewOf<E extends EntityLike>(entity: E, prefabs: Map<string, PrefabEntity>): E {
  const stored = entity.components as Components;
  const id = (stored.prefab_instance as PrefabInstanceData | undefined)?.prefab_id;
  const template = id ? prefabs.get(id)?.components : undefined;
  if (template) return { ...entity, components: resolveInstance(template, stored) };
  // A part of a placed instance, laid over in this scene.
  const part = partOf(entity, prefabs);
  return part ? { ...entity, components: resolveOverlay(part.components, stored) } : entity;
}

/** Edit an entity as it is (`viewOf`), in place: `mutate` changes the
 *  resolved entity, and a linked instance keeps only what then differs from
 *  its prefab (`diffEdit`). Every editor surface that changes an entity -
 *  the Inspector, the scene view's gizmo - goes through this, so an instance
 *  is never edited as if its prefab's components were its own. */
export function editThroughInstance<E extends EntityLike>(
  entity: E,
  prefabs: Map<string, PrefabEntity>,
  mutate: (view: E) => void,
): void {
  const stored = entity.components as Components;
  const id = (stored.prefab_instance as PrefabInstanceData | undefined)?.prefab_id;
  const template = id ? prefabs.get(id)?.components : undefined;
  if (!template) {
    const part = partOf(entity, prefabs);
    if (part) {
      // An overlay keeps the components that differ from its part, whole.
      const view = { ...entity, components: resolveOverlay(part.components, stored) } as E;
      mutate(view);
      entity.name = view.name;
      entity.parent = view.parent;
      entity.components = diffOverlay(part.components, view.components as Components) as E["components"];
      return;
    }
    mutate(entity);
    return;
  }
  const before = resolveInstance(template, stored);
  const view = { ...entity, components: clone(before) } as E;
  mutate(view);
  entity.name = view.name;
  entity.parent = view.parent;
  entity.components = diffEdit(stored, template, before, view.components as Components) as E["components"];
}

/** What an entity changes from what it is built from, by component: for an
 *  instance (`instanceChanges`) each component it authors itself and each
 *  override path under its component; for a part overlay (`asPart`) each
 *  component it holds, whole. The Inspector marks these. */
export function changedByComponent(stored: Components, asPart = false): Map<string, string[]> {
  const out = new Map<string, string[]>();
  const add = (comp: string, path: string) => out.set(comp, [...(out.get(comp) ?? []), path]);
  if (asPart) {
    for (const [key, value] of Object.entries(stored)) if (value !== null && value !== undefined) add(key, key);
    return out;
  }
  if (!stored.prefab_instance) return out;
  for (const row of instanceChanges(stored)) add(row.path.split(".")[0], row.path);
  return out;
}

/** Whether `field` of `comp` is among what an entity changes
 *  (`changedByComponent`): the whole component, or a path at or under the
 *  field. */
export function fieldChanged(changes: Map<string, string[]>, comp: string, field: string): boolean {
  return (changes.get(comp) ?? []).some((p) => p === comp || p === `${comp}.${field}` || p.startsWith(`${comp}.${field}.`));
}
