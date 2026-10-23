/**
 * A project's mixer (`settings.audio_mixer`, Rust `audio::MixerLayout`) as
 * Project Settings -> Audio edits it: the buses a game starts with, parents
 * first, each with a volume, a mute and its effects. `master`, `sfx` and
 * `music` always exist in the engine; the editor shows them whether the
 * layout names them or not. Every function returns a new layout.
 */

export interface Effects {
  lowpass: number;
  reverb: number;
  room: number;
  echo: number;
  echo_time: number;
  echo_feedback: number;
  distortion: number;
  compress: number;
  eq_low: number;
  eq_mid: number;
  eq_high: number;
}

export interface Bus {
  name: string;
  parent?: string;
  volume: number;
  muted?: boolean;
  effects?: Partial<Effects>;
}

export interface Layout {
  buses: Bus[];
}

export const BUILT_IN = ["master", "sfx", "music"] as const;

/** `BusEffects::default()`. */
export const EFFECT_DEFAULTS: Effects = {
  lowpass: 0,
  reverb: 0,
  room: 0.5,
  echo: 0,
  echo_time: 0.35,
  echo_feedback: 0.4,
  distortion: 0,
  compress: 0,
  eq_low: 0,
  eq_mid: 0,
  eq_high: 0,
};

/** Each effect's range, as `AudioMixer::set_effect` clamps it, and what it
 *  is, for the panel. `lowpass` 0 is off. */
export const EFFECTS: { key: keyof Effects; label: string; min: number; max: number; step: number; doc: string }[] = [
  { key: "eq_low", label: "EQ low", min: -24, max: 24, step: 0.5, doc: "Low shelf at 250 Hz, decibels" },
  { key: "eq_mid", label: "EQ mid", min: -24, max: 24, step: 0.5, doc: "Bell at 1 kHz, decibels" },
  { key: "eq_high", label: "EQ high", min: -24, max: 24, step: 0.5, doc: "High shelf at 4 kHz, decibels" },
  { key: "distortion", label: "Distortion", min: 0, max: 1, step: 0.01, doc: "Soft clip: a crackling radio at 0.6" },
  { key: "lowpass", label: "Low-pass", min: 0, max: 20000, step: 50, doc: "Cutoff in hertz - muffles what is above; 0 is off" },
  { key: "compress", label: "Compress", min: 0, max: 1, step: 0.01, doc: "Pull loud toward quiet, and make the whole up again" },
  { key: "echo", label: "Echo", min: 0, max: 1, step: 0.01, doc: "How much echo is heard" },
  { key: "echo_time", label: "Echo time", min: 0.02, max: 2, step: 0.01, doc: "Seconds between echoes" },
  { key: "echo_feedback", label: "Echo feedback", min: 0, max: 0.95, step: 0.01, doc: "How much of each echo comes back" },
  { key: "reverb", label: "Reverb", min: 0, max: 1, step: 0.01, doc: "How much reverb is mixed in" },
  { key: "room", label: "Room", min: 0, max: 1, step: 0.01, doc: "A closet to a cathedral" },
];

/** The buses to show: the layout's, with the three built-ins added (at full
 *  volume, no effects) where it does not name them, parents first. */
export function shownBuses(layout: Layout | undefined): Bus[] {
  const buses = [...(layout?.buses ?? [])];
  for (const name of BUILT_IN) {
    if (!buses.some((b) => b.name === name)) buses.push({ name, volume: 1, ...(name === "master" ? {} : { parent: "master" }) });
  }
  return parentsFirst(buses);
}

/** A bus's parent as the engine reads it: none for master, else master
 *  when unnamed. */
export function parentOf(b: Bus): string | null {
  return b.name === "master" ? null : (b.parent ?? "master");
}

/** Buses ordered so each comes after its parent (the order the engine
 *  makes them in), siblings in the order given. */
export function parentsFirst(buses: Bus[]): Bus[] {
  const out: Bus[] = [];
  const placed = new Set<string>();
  const visit = (b: Bus, depth: number) => {
    if (placed.has(b.name) || depth > buses.length) return;
    const p = parentOf(b);
    const parent = p ? buses.find((x) => x.name === p) : undefined;
    if (parent && !placed.has(parent.name)) visit(parent, depth + 1);
    if (!placed.has(b.name)) {
      placed.add(b.name);
      out.push(b);
    }
  };
  for (const b of buses) visit(b, 0);
  return out;
}

/** How deep a bus sits under master (master is 0). */
export function depthOf(buses: Bus[], name: string): number {
  let d = 0;
  let cur = buses.find((b) => b.name === name);
  while (cur && parentOf(cur) && d < 32) {
    const p = parentOf(cur);
    cur = buses.find((b) => b.name === p);
    d++;
  }
  return d;
}

/** Write the shown buses back as a layout: only what differs from the
 *  engine's own (a built-in at full volume with nothing set is left out). */
export function toLayout(buses: Bus[]): Layout {
  return {
    buses: parentsFirst(buses).filter((b) => {
      const builtIn = (BUILT_IN as readonly string[]).includes(b.name);
      const plainParent = b.name === "master" ? !b.parent : (b.parent ?? "master") === "master";
      return !(builtIn && b.volume === 1 && !b.muted && plainParent && Object.keys(b.effects ?? {}).length === 0);
    }),
  };
}

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));

/** Set one effect, clamped to its range; the default value is left out. */
export function withEffect(b: Bus, key: keyof Effects, value: number): Bus {
  const spec = EFFECTS.find((e) => e.key === key)!;
  const v = clamp(Number.isFinite(value) ? value : 0, spec.min, spec.max);
  const effects = { ...(b.effects ?? {}) };
  if (v === EFFECT_DEFAULTS[key]) delete effects[key];
  else effects[key] = v;
  const out: Bus = { ...b, effects };
  if (Object.keys(effects).length === 0) delete out.effects;
  return out;
}

/** A bus's effect, or its default. */
export function effectOf(b: Bus, key: keyof Effects): number {
  return b.effects?.[key] ?? EFFECT_DEFAULTS[key];
}

/** Add a bus under `parent` with a name not yet used. */
export function addBus(buses: Bus[], parent: string, base = "bus"): Bus[] {
  let name = base;
  for (let n = 2; buses.some((b) => b.name === name); n++) name = `${base}${n}`;
  return [...buses, { name, parent, volume: 1 }];
}

/** Remove a bus (never a built-in); its children move to its parent, as
 *  `AudioMixer::remove_bus` does. */
export function removeBus(buses: Bus[], name: string): Bus[] {
  if ((BUILT_IN as readonly string[]).includes(name)) return buses;
  const gone = buses.find((b) => b.name === name);
  const up = gone ? (parentOf(gone) ?? "master") : "master";
  return buses.filter((b) => b.name !== name).map((b) => (parentOf(b) === name ? { ...b, parent: up } : b));
}

/** Rename a bus (never a built-in, nor to a name in use), and its
 *  children's parent with it. */
export function renameBus(buses: Bus[], from: string, to: string): Bus[] {
  const name = to.trim();
  if (!name || (BUILT_IN as readonly string[]).includes(from) || buses.some((b) => b.name === name)) return buses;
  return buses.map((b) => (b.name === from ? { ...b, name } : parentOf(b) === from ? { ...b, parent: name } : b));
}

/** Whether `ancestor` is `name` or above it - a bus cannot be moved under
 *  itself or its own children. */
export function isUnder(buses: Bus[], name: string, ancestor: string): boolean {
  let cur: string | null = name;
  for (let i = 0; cur && i < 64; i++) {
    if (cur === ancestor) return true;
    const b = buses.find((x) => x.name === cur);
    cur = b ? parentOf(b) : null;
  }
  return false;
}
