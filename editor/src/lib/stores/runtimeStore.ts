import { writable } from "svelte/store";
import type { EntityData } from "../api/scene";

/**
 * Live entity data from the WASM runtime during play mode.
 * Updated each frame for the currently selected entity.
 * Edits written here go to the WASM world temporarily (not persisted on stop).
 */
export const runtimeEntity = writable<EntityData | null>(null);

/** One live entity as reported by `viewport_get_runtime_entities`. */
export interface RuntimeEntityRow {
  /** Index into the viewport's `scene_entities` — the handle every
   *  runtime IPC call (select, inspect, bounds) takes. Sparse: slots of
   *  despawned entities are skipped, never renumbered. */
  index: number;
  name: string;
  /** Index of this entity's parent, or null for a root. */
  parent: number | null;
  /** Spawned by a script during play rather than authored in the scene. */
  spawned: boolean;
  /** Engine-managed view entity (health-bar quad, name tag). Hidden in
   *  the Scene panel unless the user asks for them. */
  internal: boolean;
  /** Coarse component hint for the row icon. */
  kind: string;
}

/**
 * The live world during play mode — every entity, including the ones
 * scripts spawned. Polled from the viewport a few times a second while
 * playing, empty otherwise.
 */
export const runtimeEntities = writable<RuntimeEntityRow[]>([]);
