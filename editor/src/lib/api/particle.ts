import { invoke } from "@tauri-apps/api/core";
import { writeTextFile } from "@tauri-apps/plugin-fs";
export { PARTICLE_PRESETS, type ParticlePreset } from "./particle-presets";

/** Matches the Rust `ParticleEmitter` struct from jengine-core. */
export interface ParticleEmitterData {
  /**
   * Optional reference to a `.particle` asset whose fields are merged into
   * this emitter by `resolveParticleSourcesInScene` before the scene is
   * pushed to the viewport. Scene-level fields win on conflict.
   */
  particle_source?: string;
  texture_id?: string;
  emission_rate: number;
  max_particles: number;
  lifetime: [number, number];
  speed: [number, number];
  angle: [number, number];
  color_start: [number, number, number, number];
  color_end: [number, number, number, number];
  size_start: [number, number];
  size_end: [number, number];
  gravity_modifier: number;
  angular_velocity: [number, number];
  burst_count: number;
  active: boolean;
  world_space: boolean;
  z_order: number;
  /** How particles blend with what is under them. */
  blend_mode?: ParticleBlendMode;
  /** Where on the emitter a particle starts. */
  emit_shape?: EmitShape;
  /** `[t, r, g, b, a]` keys: colour over life. */
  color_curve?: number[][];
  /** `[t, multiplier]` keys: size over life. */
  size_curve?: number[][];
  /** Stretch along velocity, seconds of travel. */
  stretch?: number;
  /** Air resistance per second. */
  drag?: number;
  /** A swirling flow the particles drift along. */
  turbulence?: Turbulence | null;
  /** What the particles do when they meet a collider. */
  collision?: ParticleCollision | null;
  /** Other emitters, by entity name, burst where these die or hit. */
  sub_emitters?: SubEmitter[];
}

/** Rust `particles::ParticleCollision`. */
export interface ParticleCollision {
  response: "kill" | "bounce";
  bounce: number;
  friction: number;
  mask: number;
}

/** Rust `particles::SubEmitter`. */
export interface SubEmitter {
  on: "death" | "collision";
  emitter: string;
  count: number;
  chance: number;
}

export type ParticleBlendMode = "Alpha" | "Additive" | "Multiply" | "Screen" | "PremultipliedAlpha";

/** Rust `particles::EmitShape`, tagged by `kind`. */
export type EmitShape =
  | { kind: "point" }
  | { kind: "circle"; radius: number; edge?: boolean }
  | { kind: "rect"; size: [number, number] }
  | { kind: "line"; length: number }
  /** Along the entity's own `vector_path`. */
  | { kind: "path" }
  /** Over the entity's own sprite's opaque pixels, or only its outline. */
  | { kind: "sprite"; edge?: boolean; cutoff?: number };

/** A texture sheet each particle plays (Rust `ParticleFlipbook`). */
export interface ParticleFlipbook {
  columns: number;
  rows: number;
  /** Frames used, 0 for all. */
  frames: number;
  /** "life": over the particle's life; "fps": frames a second; "random": one frame held. */
  mode: "life" | "fps" | "random";
  fps: number;
  cycles: number;
  random_start: boolean;
}

/** Light from the particles (Rust `ParticleLights`): a shadowless point light per particle. */
export interface ParticleLights {
  /** One particle in `every` carries a light. */
  every: number;
  /** At most this many an emitter. */
  max: number;
  radius: number;
  intensity: number;
  falloff: number;
  use_particle_color: boolean;
  /** Linear RGB, used when `use_particle_color` is off. */
  color: [number, number, number];
  scale_with_size: boolean;
  fade: boolean;
}

/** A ribbon behind each particle (Rust `ParticleTrail`). */
export interface ParticleTrail {
  /** Points kept, 2 to 16. */
  length: number;
  /** Seconds between two points. */
  interval: number;
  /** Width at the head, a share of the particle's size. */
  width: number;
  taper: boolean;
  fade: boolean;
}

/** Rust `particles::Turbulence`. */
export interface Turbulence {
  strength: number;
  scale: number;
  speed: number;
}

/** Default emitter values matching Rust `ParticleEmitter::default()`. */
export function defaultParticleEmitter(): ParticleEmitterData {
  return {
    emission_rate: 20,
    max_particles: 256,
    lifetime: [0.5, 1.5],
    speed: [50, 100],
    angle: [0, Math.PI * 2],
    color_start: [1, 1, 1, 1],
    color_end: [1, 1, 1, 0],
    size_start: [8, 12],
    size_end: [2, 4],
    gravity_modifier: 0,
    angular_velocity: [0, 0],
    burst_count: 0,
    active: true,
    world_space: true,
    z_order: 0,
  };
}

/** Read a `.particle` file from the project assets. */
export async function readParticleFile(
  projectPath: string,
  relativePath: string,
): Promise<ParticleEmitterData> {
  const bytes = await invoke<number[]>("read_asset_binary", {
    projectPath,
    relativePath,
  });
  const text = new TextDecoder().decode(new Uint8Array(bytes));
  return JSON.parse(text) as ParticleEmitterData;
}

/** Save a `.particle` file to disk (overwrites existing). */
export async function saveParticleFile(
  fullPath: string,
  data: ParticleEmitterData,
): Promise<void> {
  const json = JSON.stringify(data, null, 2);
  await writeTextFile(fullPath, json);
}

/**
 * Walk a scene's entities and inline any `particle_emitter.particle_source`
 * references by reading the referenced `.particle` file. Mutates `sceneData`
 * in place — for each field present in the file but missing on the entity's
 * emitter, the file's value is inserted (scene-level overrides win).
 * Errors per entity are logged via the optional `onError` callback and do
 * not abort.
 *
 * Mirrors the native runtime's `resolve_particle_sources` pass
 * (crates/jengine-core/src/particles/asset.rs) — the editor must run
 * this before handing the scene JSON to the viewport WASM since WASM
 * has no filesystem access.
 */
export async function resolveParticleSourcesInScene(
  projectPath: string,
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  sceneData: { entities?: any[] },
  onError?: (entityName: string, err: unknown) => void,
): Promise<number> {
  const entities = sceneData.entities ?? [];
  let resolved = 0;
  for (const ent of entities) {
    const components = ent?.components;
    const emitter = components?.particle_emitter;
    const source: unknown = emitter?.particle_source;
    if (!emitter || typeof source !== "string" || source.length === 0)
      continue;
    try {
      const data = (await readParticleFile(projectPath, source)) as Record<
        string,
        unknown
      >;
      for (const [key, val] of Object.entries(data)) {
        if (key === "particle_source") continue;
        if (!(key in emitter)) {
          emitter[key] = val;
        }
      }
      resolved += 1;
    } catch (err) {
      onError?.(ent?.name ?? "<unnamed>", err);
    }
  }
  return resolved;
}
