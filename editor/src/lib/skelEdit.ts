/**
 * Editing skeletal animation as the data it is (Rust `animation::skeletal`,
 * `animation::rig`): a `.skelanim` clip - per bone, keys on its rotation,
 * position and scale - a `.rig` - bones with a parent, a bind pose and a
 * length - and a `skeletal_player`'s blend space and layers.
 *
 * The clip editor (`SkeletalEditor.svelte`) poses its preview with
 * `sampleScalar` / `sampleVec2`, and the inspector shows a blend space's
 * weights with `blendWeights`; all three are checked against the engine on
 * one fixture (`crates/jengine-core/tests/fixtures/skeletal_sample.json`),
 * so what the editor shows is what plays.
 *
 * Every edit returns new data; none mutates its argument.
 */

export interface Vec2Key {
  time: number;
  value: [number, number];
}
export interface ScalarKey {
  time: number;
  value: number;
}
export interface BoneTrack {
  bone_name: string;
  position?: Vec2Key[];
  rotation?: ScalarKey[];
  scale?: Vec2Key[];
}
export interface SkelClip {
  name: string;
  duration: number;
  tracks?: BoneTrack[];
}

export type Channel = "rotation" | "position" | "scale";
export const CHANNELS: Channel[] = ["rotation", "position", "scale"];

export interface RigBone {
  name: string;
  parent?: string;
  position?: [number, number];
  /** Radians. */
  rotation?: number;
  scale?: [number, number];
  length?: number;
}
export interface Rig {
  name: string;
  bones: RigBone[];
}

const clone = <T>(v: T): T => structuredClone(v);
/** Keys closer than this are one key. */
export const SAME_TIME = 1e-4;

// ─── Sampling, as the engine samples ────────────────────────────────────

/** A scalar track at `t`: linear between keys, the first / last key held
 *  outside them; undefined for no keys (the bone keeps its own value). */
export function sampleScalar(keys: ScalarKey[] | undefined, t: number): number | undefined {
  if (!keys || keys.length === 0) return undefined;
  if (keys.length === 1 || t <= keys[0].time) return keys[0].value;
  const last = keys[keys.length - 1];
  if (t >= last.time) return last.value;
  for (let i = 0; i + 1 < keys.length; i++) {
    const a = keys[i];
    const b = keys[i + 1];
    if (t >= a.time && t <= b.time) {
      const f = (t - a.time) / Math.max(b.time - a.time, 1.1920929e-7);
      return a.value + (b.value - a.value) * f;
    }
  }
  return last.value;
}

/** A position or scale track at `t`; see `sampleScalar`. */
export function sampleVec2(keys: Vec2Key[] | undefined, t: number): [number, number] | undefined {
  if (!keys || keys.length === 0) return undefined;
  if (keys.length === 1 || t <= keys[0].time) return [...keys[0].value];
  const last = keys[keys.length - 1];
  if (t >= last.time) return [...last.value];
  for (let i = 0; i + 1 < keys.length; i++) {
    const a = keys[i];
    const b = keys[i + 1];
    if (t >= a.time && t <= b.time) {
      const f = (t - a.time) / Math.max(b.time - a.time, 1.1920929e-7);
      return [a.value[0] + (b.value[0] - a.value[0]) * f, a.value[1] + (b.value[1] - a.value[1]) * f];
    }
  }
  return [...last.value];
}

/** Gradient band weights of a blend space's points at `p`, summing to 1 -
 *  `animation::skeletal::blend_weights`. */
export function blendWeights(points: [number, number][], p: [number, number]): number[] {
  const w = points.map((a, i) => {
    let h = 1;
    points.forEach((b, j) => {
      const abx = b[0] - a[0];
      const aby = b[1] - a[1];
      const len2 = abx * abx + aby * aby;
      if (j !== i && len2 > 1e-12) h = Math.min(h, 1 - ((p[0] - a[0]) * abx + (p[1] - a[1]) * aby) / len2);
    });
    return Math.max(h, 0);
  });
  const sum = w.reduce((s, x) => s + x, 0);
  if (sum > 0) return w.map((x) => x / sum);
  let k = -1;
  let best = Infinity;
  points.forEach((a, i) => {
    const d = (a[0] - p[0]) ** 2 + (a[1] - p[1]) ** 2;
    if (d < best) {
      best = d;
      k = i;
    }
  });
  if (k >= 0) w[k] = 1;
  return w;
}

// ─── A skeleton to pose ─────────────────────────────────────────────────

export interface Local {
  position: [number, number];
  rotation: number;
  scale: [number, number];
}
/** One bone of a skeleton to preview: its parent's index (-1 for a root),
 *  parents always before their children. */
export interface SkelBone {
  name: string;
  parent: number;
  length: number;
  local: Local;
}

function ordered(bones: { name: string; parentName?: string; length: number; local: Local }[]): SkelBone[] {
  const names = new Set(bones.map((b) => b.name));
  const out: SkelBone[] = [];
  const index = new Map<string, number>();
  let pending = bones.slice();
  // Parents first; a bone whose parent is missing (or in a cycle) is a root.
  for (let guard = 0; pending.length > 0 && guard <= bones.length; guard++) {
    const next: typeof pending = [];
    for (const b of pending) {
      const p = b.parentName && names.has(b.parentName) && b.parentName !== b.name ? b.parentName : undefined;
      if (p === undefined || index.has(p)) {
        index.set(b.name, out.length);
        out.push({ name: b.name, parent: p === undefined ? -1 : index.get(p)!, length: b.length, local: b.local });
      } else next.push(b);
    }
    pending = next;
  }
  for (const b of pending) {
    index.set(b.name, out.length);
    out.push({ name: b.name, parent: -1, length: b.length, local: b.local });
  }
  return out;
}

/** A `.rig`'s bones in their bind pose. */
export function skeletonFromRig(rig: Rig): SkelBone[] {
  return ordered(
    (rig.bones ?? []).map((b) => ({
      name: b.name,
      parentName: b.parent ?? undefined,
      length: b.length ?? 1,
      local: { position: b.position ?? [0, 0], rotation: b.rotation ?? 0, scale: b.scale ?? [1, 1] },
    })),
  );
}

/** A scene entity, as the scene file has it. */
export interface SceneEntity {
  name: string;
  parent?: string | null;
  components: Record<string, any>;
}

/** The skeletons in a scene: each entity with a `skeleton_root`. */
export function sceneSkeletons(entities: SceneEntity[]): { index: number; name: string }[] {
  return entities
    .map((e, index) => ({ e, index }))
    .filter(({ e }) => e.components?.skeleton_root)
    .map(({ e, index }) => ({ index, name: e.components.skeleton_root?.name || e.name }));
}

function localOf(e: SceneEntity): Local {
  const t = e.components?.transform ?? {};
  return { position: t.position ?? [0, 0], rotation: t.rotation ?? 0, scale: t.scale ?? [1, 1] };
}

/** A scene skeleton's bone as a rig bone, with the entity it is. */
export type SceneRigBone = RigBone & { entity: string };

/** The bones under the scene skeleton rooted at entity `root` (the root
 *  itself if it is a bone): each `bone` entity, parented to its nearest
 *  bone ancestor, with its entity's name. Bones are found by entity name,
 *  as a v1 scene parents. */
export function sceneRigBones(entities: SceneEntity[], root: number): SceneRigBone[] {
  const rootName = entities[root]?.name;
  if (rootName === undefined) return [];
  const byName = new Map<string, SceneEntity>();
  for (const e of entities) if (!byName.has(e.name)) byName.set(e.name, e);
  const chain = (e: SceneEntity): string[] => {
    const up: string[] = [];
    let p = e.parent ?? undefined;
    for (let guard = 0; p && guard < 1000; guard++) {
      up.push(p);
      if (p === rootName) break;
      p = byName.get(p)?.parent ?? undefined;
    }
    return up;
  };
  const out: SceneRigBone[] = [];
  for (const e of entities) {
    const bone = e.components?.bone;
    if (!bone) continue;
    const up = chain(e);
    if (e !== entities[root] && !up.includes(rootName)) continue;
    const parentEntity = e === entities[root] ? undefined : up.map((n) => byName.get(n)).find((p) => p && p.components?.bone && p !== e);
    const l = localOf(e);
    out.push({
      name: bone.name ?? e.name,
      entity: e.name,
      ...(parentEntity ? { parent: parentEntity.components.bone.name ?? parentEntity.name } : {}),
      position: l.position,
      rotation: l.rotation,
      scale: l.scale,
      length: bone.length ?? 0,
    });
  }
  return out;
}

/** The bones under the scene skeleton rooted at entity `root`, ready to
 *  pose (see `sceneRigBones`). */
export function skeletonFromScene(entities: SceneEntity[], root: number): SkelBone[] {
  return skeletonFromRig({ name: "", bones: sceneRigBones(entities, root) });
}

/** The skeleton posed by `clip` at `t`: each animated channel replaces the
 *  bone's own value, as `update_skeletal_players` does. */
export function poseAt(skel: SkelBone[], clip: SkelClip | null, t: number): SkelBone[] {
  if (!clip) return skel;
  const tracks = new Map((clip.tracks ?? []).map((tr) => [tr.bone_name, tr]));
  return skel.map((b) => {
    const tr = tracks.get(b.name);
    if (!tr) return b;
    return {
      ...b,
      local: {
        position: sampleVec2(tr.position, t) ?? b.local.position,
        rotation: sampleScalar(tr.rotation, t) ?? b.local.rotation,
        scale: sampleVec2(tr.scale, t) ?? b.local.scale,
      },
    };
  });
}

/** A 2D affine map: `[a, b, c, d, tx, ty]` takes (x, y) to
 *  (a x + c y + tx, b x + d y + ty). */
export type Affine = [number, number, number, number, number, number];
export const IDENTITY: Affine = [1, 0, 0, 1, 0, 0];

function localAffine(l: Local): Affine {
  const cs = Math.cos(l.rotation);
  const sn = Math.sin(l.rotation);
  // translate * rotate * scale
  return [cs * l.scale[0], sn * l.scale[0], -sn * l.scale[1], cs * l.scale[1], l.position[0], l.position[1]];
}
export function mul(p: Affine, q: Affine): Affine {
  return [
    p[0] * q[0] + p[2] * q[1],
    p[1] * q[0] + p[3] * q[1],
    p[0] * q[2] + p[2] * q[3],
    p[1] * q[2] + p[3] * q[3],
    p[0] * q[4] + p[2] * q[5] + p[4],
    p[1] * q[4] + p[3] * q[5] + p[5],
  ];
}
export function apply(m: Affine, x: number, y: number): [number, number] {
  return [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
}
/** The inverse of the linear part applied to a direction. */
function unapplyLinear(m: Affine, x: number, y: number): [number, number] {
  const det = m[0] * m[3] - m[2] * m[1];
  if (Math.abs(det) < 1e-12) return [x, y];
  return [(m[3] * x - m[2] * y) / det, (-m[1] * x + m[0] * y) / det];
}

export interface WorldBone {
  /** The bone's frame in the skeleton's space. */
  m: Affine;
  origin: [number, number];
  tip: [number, number];
}

/** Every bone's frame, joint and tip in the skeleton root's space - the
 *  hierarchy composed in full, so a mirrored or squashed parent shows as
 *  the engine draws it. */
export function worldPose(skel: SkelBone[]): WorldBone[] {
  const out: WorldBone[] = [];
  for (const b of skel) {
    const parent = b.parent >= 0 ? out[b.parent].m : IDENTITY;
    const m = mul(parent, localAffine(b.local));
    out.push({ m, origin: [m[4], m[5]], tip: apply(m, b.length, 0) });
  }
  return out;
}

/** The local rotation that points bone `i` from its joint toward `p`
 *  (skeleton space) - what dragging a bone's tip keys. */
export function rotationToward(skel: SkelBone[], world: WorldBone[], i: number, p: [number, number]): number {
  const b = skel[i];
  const parent = b.parent >= 0 ? world[b.parent].m : IDENTITY;
  const o = world[i].origin;
  const [x, y] = unapplyLinear(parent, p[0] - o[0], p[1] - o[1]);
  if (Math.hypot(x, y) < 1e-9) return b.local.rotation;
  const a = Math.atan2(y, x);
  // The nearest turn to the current angle, so a drag never jumps a whole turn.
  const cur = b.local.rotation;
  return a + Math.round((cur - a) / (2 * Math.PI)) * 2 * Math.PI;
}

/** The local position that puts bone `i`'s joint at `p` (skeleton space). */
export function positionAt(skel: SkelBone[], world: WorldBone[], i: number, p: [number, number]): [number, number] {
  const b = skel[i];
  const parent = b.parent >= 0 ? world[b.parent].m : IDENTITY;
  return unapplyLinear(parent, p[0] - parent[4], p[1] - parent[5]);
}

/** The bone nearest `p` within `reach` (its segment from joint to tip),
 *  or -1. */
export function boneAt(world: WorldBone[], p: [number, number], reach: number): number {
  let best = -1;
  let bestD = reach;
  world.forEach((w, i) => {
    const [ax, ay] = w.origin;
    const dx = w.tip[0] - ax;
    const dy = w.tip[1] - ay;
    const len2 = dx * dx + dy * dy;
    const f = len2 > 0 ? Math.min(1, Math.max(0, ((p[0] - ax) * dx + (p[1] - ay) * dy) / len2)) : 0;
    const d = Math.hypot(p[0] - (ax + dx * f), p[1] - (ay + dy * f));
    if (d <= bestD) {
      bestD = d;
      best = i;
    }
  });
  return best;
}

/** The box round every joint and tip, for framing the preview. */
export function bounds(world: WorldBone[]): [number, number, number, number] {
  if (world.length === 0) return [-1, -1, 1, 1];
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (const w of world) {
    for (const [x, y] of [w.origin, w.tip]) {
      x0 = Math.min(x0, x);
      y0 = Math.min(y0, y);
      x1 = Math.max(x1, x);
      y1 = Math.max(y1, y);
    }
  }
  return [x0, y0, x1, y1];
}

/** How many of `names` the skeleton has - to pick the skeleton a clip was
 *  made for. */
export function coverage(skel: SkelBone[], names: string[]): number {
  const have = new Set(skel.map((b) => b.name));
  return names.filter((n) => have.has(n)).length;
}

// ─── Clip edits ─────────────────────────────────────────────────────────

export function parseClip(text: string): SkelClip {
  const raw = JSON.parse(text);
  return { name: raw.name ?? "", duration: Number(raw.duration) || 0, ...raw, tracks: raw.tracks ?? [] };
}

/** The clip as the engine reads it: tracks in their order, keys by time,
 *  empty channels left out (as serde skips them). */
export function formatClip(clip: SkelClip): string {
  const tracks = (clip.tracks ?? []).map((tr) => {
    const out: BoneTrack = { bone_name: tr.bone_name };
    for (const ch of CHANNELS) {
      const keys = tr[ch] as (ScalarKey | Vec2Key)[] | undefined;
      if (keys && keys.length > 0) (out as any)[ch] = keys;
    }
    return out;
  });
  return JSON.stringify({ ...clip, tracks }, null, 2) + "\n";
}

export function trackFor(clip: SkelClip, bone: string): BoneTrack | undefined {
  return (clip.tracks ?? []).find((t) => t.bone_name === bone);
}

/** Every distinct key time on a bone's track, sorted. */
export function keyTimes(track: BoneTrack | undefined): number[] {
  const out: number[] = [];
  for (const ch of CHANNELS) {
    for (const k of (track?.[ch] ?? []) as { time: number }[]) {
      if (!out.some((t) => Math.abs(t - k.time) < SAME_TIME)) out.push(k.time);
    }
  }
  return out.sort((a, b) => a - b);
}

function withTrack(clip: SkelClip, bone: string, fn: (tr: BoneTrack) => void): SkelClip {
  const next = clone(clip);
  next.tracks = next.tracks ?? [];
  let tr = next.tracks.find((t) => t.bone_name === bone);
  if (!tr) {
    tr = { bone_name: bone };
    next.tracks.push(tr);
  }
  fn(tr);
  return next;
}

/** Key `channel` of `bone` at `t` (replacing a key at the same time).
 *  Returns the clip and the key's index. */
export function setKey(
  clip: SkelClip,
  bone: string,
  channel: Channel,
  t: number,
  value: number | [number, number],
): { clip: SkelClip; index: number } {
  let index = 0;
  const next = withTrack(clip, bone, (tr) => {
    const keys = ((tr[channel] as any[]) ?? []).slice();
    const key = { time: t, value: Array.isArray(value) ? [value[0], value[1]] : value };
    const same = keys.findIndex((k) => Math.abs(k.time - t) < SAME_TIME);
    if (same >= 0) {
      keys[same] = { ...key, time: keys[same].time };
      index = same;
    } else {
      keys.push(key);
      keys.sort((a, b) => a.time - b.time);
      index = keys.indexOf(key);
    }
    (tr as any)[channel] = keys;
  });
  return { clip: next, index };
}

export function removeKey(clip: SkelClip, bone: string, channel: Channel, k: number): SkelClip {
  return withTrack(clip, bone, (tr) => {
    const keys = ((tr[channel] as any[]) ?? []).slice();
    keys.splice(k, 1);
    (tr as any)[channel] = keys;
  });
}

/** Move key `k` to time `t` (onto another key's time replaces that key).
 *  Returns the clip and the key's new index. */
export function moveKey(clip: SkelClip, bone: string, channel: Channel, k: number, t: number): { clip: SkelClip; index: number } {
  let index = k;
  const next = withTrack(clip, bone, (tr) => {
    const keys = ((tr[channel] as any[]) ?? []).slice();
    const key = { ...keys[k], time: Math.max(0, t) };
    keys.splice(k, 1);
    const same = keys.findIndex((o) => Math.abs(o.time - key.time) < SAME_TIME);
    if (same >= 0) keys.splice(same, 1);
    keys.push(key);
    keys.sort((a, b) => a.time - b.time);
    index = keys.indexOf(key);
    (tr as any)[channel] = keys;
  });
  return { clip: next, index };
}

/** Move every key a bone has at time `from` to `to` - its summary diamond. */
export function moveBoneKeys(clip: SkelClip, bone: string, from: number, to: number): SkelClip {
  let next = clip;
  for (const ch of CHANNELS) {
    const keys = (trackFor(next, bone)?.[ch] ?? []) as { time: number }[];
    const k = keys.findIndex((key) => Math.abs(key.time - from) < SAME_TIME);
    if (k >= 0) next = moveKey(next, bone, ch, k, to).clip;
  }
  return next;
}

/** Remove every key a bone has at time `t`. */
export function removeBoneKeys(clip: SkelClip, bone: string, t: number): SkelClip {
  return withTrack(clip, bone, (tr) => {
    for (const ch of CHANNELS) {
      const keys = (tr[ch] as { time: number }[] | undefined) ?? [];
      (tr as any)[ch] = keys.filter((k) => Math.abs(k.time - t) >= SAME_TIME);
    }
  });
}

export function removeTrack(clip: SkelClip, bone: string): SkelClip {
  return { ...clone(clip), tracks: (clip.tracks ?? []).filter((t) => t.bone_name !== bone).map(clone) };
}

/** A bone's value on one channel at `t`: its track's, or the skeleton's
 *  own when that channel is not animated. */
export function valueAt(skel: SkelBone[], clip: SkelClip, bone: string, channel: Channel, t: number): number | [number, number] {
  const b = skel.find((x) => x.name === bone);
  const tr = trackFor(clip, bone);
  if (channel === "rotation") return sampleScalar(tr?.rotation, t) ?? b?.local.rotation ?? 0;
  const own = channel === "position" ? (b?.local.position ?? [0, 0]) : (b?.local.scale ?? [1, 1]);
  return sampleVec2(tr?.[channel], t) ?? [own[0], own[1]];
}

/** Key the bone's current value on each of `channels` at `t`. */
export function keyPose(clip: SkelClip, skel: SkelBone[], bone: string, t: number, channels: Channel[]): SkelClip {
  let next = clip;
  for (const ch of channels) next = setKey(next, bone, ch, t, valueAt(skel, next, bone, ch, t)).clip;
  return next;
}

/** Times snapped to a grid of `step` seconds (0 for none). */
export function snap(t: number, step: number): number {
  if (!(step > 0)) return Math.max(0, t);
  return Math.max(0, Math.round(t / step) * step);
}

// ─── Rig edits ──────────────────────────────────────────────────────────

export function parseRig(text: string): Rig {
  const raw = JSON.parse(text);
  return { ...raw, name: raw.name ?? "", bones: raw.bones ?? [] };
}

/** One bone a line, as the hand-written rigs are laid out. */
export function formatRig(rig: Rig): string {
  const { bones, ...rest } = rig;
  const head = JSON.stringify(rest, null, 2).replace(/\n}$/, "");
  const lines = (bones ?? []).map((b) => {
    const out: Record<string, unknown> = { name: b.name };
    if (b.parent) out.parent = b.parent;
    out.position = b.position ?? [0, 0];
    out.rotation = b.rotation ?? 0;
    if (b.scale && (b.scale[0] !== 1 || b.scale[1] !== 1)) out.scale = b.scale;
    out.length = b.length ?? 1;
    return "    " + JSON.stringify(out).replace(/([,:])(?=["\[\d-])/g, "$1 ").replace(/^\{/, "{ ").replace(/\}$/, " }");
  });
  const sep = Object.keys(rest).length > 0 ? ",\n" : "\n";
  return `${head}${sep}  "bones": [\n${lines.join(",\n")}\n  ]\n}\n`;
}

/** A name no bone has: `base`, `base_2`, ... */
export function freshBoneName(rig: Rig, base = "bone"): string {
  const have = new Set(rig.bones.map((b) => b.name));
  if (!have.has(base)) return base;
  for (let n = 2; ; n++) if (!have.has(`${base}_${n}`)) return `${base}_${n}`;
}

/** A new bone at the tip of `parent` (or at the origin, as a root). */
export function addBone(rig: Rig, parent: string | null): { rig: Rig; name: string } {
  const name = freshBoneName(rig);
  const p = parent ? rig.bones.find((b) => b.name === parent) : undefined;
  const bone: RigBone = {
    name,
    ...(p ? { parent: p.name } : {}),
    position: p ? [p.length ?? 1, 0] : [0, 0],
    rotation: 0,
    length: p?.length ?? 50,
  };
  return { rig: { ...clone(rig), bones: [...clone(rig.bones), bone] }, name };
}

/** Remove a bone; its children move up to its parent, keeping their
 *  place in the parent's frame (their local offset composed with the
 *  removed bone's). */
export function removeBone(rig: Rig, name: string): Rig {
  const gone = rig.bones.find((b) => b.name === name);
  if (!gone) return rig;
  const g = localAffine({ position: gone.position ?? [0, 0], rotation: gone.rotation ?? 0, scale: gone.scale ?? [1, 1] });
  const bones = rig.bones
    .filter((b) => b.name !== name)
    .map((b) => {
      if (b.parent !== name) return clone(b);
      const pos = apply(g, ...(b.position ?? [0, 0]));
      const out: RigBone = { ...clone(b), position: pos, rotation: (b.rotation ?? 0) + (gone.rotation ?? 0) };
      if (gone.parent) out.parent = gone.parent;
      else delete out.parent;
      return out;
    });
  return { ...clone(rig), bones };
}

/** Rename a bone, and every child's link to it. */
export function renameBone(rig: Rig, from: string, to: string): Rig {
  if (!to || from === to || rig.bones.some((b) => b.name === to)) return rig;
  return {
    ...clone(rig),
    bones: rig.bones.map((b) => ({
      ...clone(b),
      ...(b.name === from ? { name: to } : {}),
      ...(b.parent === from ? { parent: to } : {}),
    })),
  };
}

/** Bone `name` and every bone below it. */
export function subtree(rig: Rig, name: string): Set<string> {
  const out = new Set([name]);
  for (let grew = true; grew; ) {
    grew = false;
    for (const b of rig.bones) {
      if (b.parent && out.has(b.parent) && !out.has(b.name)) {
        out.add(b.name);
        grew = true;
      }
    }
  }
  return out;
}

/** Change one of a bone's fields; a parent that would make a loop is
 *  refused. */
export function setBone(rig: Rig, name: string, patch: Partial<RigBone>): Rig {
  if (patch.parent && subtree(rig, name).has(patch.parent)) return rig;
  return {
    ...clone(rig),
    bones: rig.bones.map((b) => {
      if (b.name !== name) return clone(b);
      const out = { ...clone(b), ...clone(patch) };
      if (!out.parent) delete out.parent;
      return out;
    }),
  };
}

// ─── A skeletal player's blend space and layers ─────────────────────────

export interface BlendPoint {
  clip: string;
  at: [number, number];
}
export interface BlendSpace {
  points: BlendPoint[];
  position?: [number, number];
}
export interface AnimLayer {
  name?: string;
  clip?: string;
  time?: number;
  speed?: number;
  looping?: boolean;
  playing?: boolean;
  weight?: number;
  mask?: string[];
  mode?: "override" | "additive";
  release?: number;
}

/** The square a blend space is drawn in: -1..1 each way, grown to hold
 *  every point and the position. */
export function blendExtent(space: BlendSpace): number {
  let r = 1;
  for (const p of space.points) r = Math.max(r, Math.abs(p.at[0]), Math.abs(p.at[1]));
  const pos = space.position ?? [0, 0];
  r = Math.max(r, Math.abs(pos[0]), Math.abs(pos[1]));
  return Math.ceil(r * 1.1 * 4) / 4;
}

/** A layer name no layer has. */
export function freshLayerName(layers: AnimLayer[], base = "layer"): string {
  const have = new Set(layers.map((l) => l.name));
  if (!have.has(base)) return base;
  for (let n = 2; ; n++) if (!have.has(`${base}_${n}`)) return `${base}_${n}`;
}

/** A clip's path as a `skeletal_player` names it: under `assets/`. */
export function clipId(path: string): string {
  return path.replace(/\\/g, "/").replace(/^.*?\/assets\//, "").replace(/^assets\//, "");
}
