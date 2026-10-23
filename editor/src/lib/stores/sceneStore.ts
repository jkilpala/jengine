import { writable } from "svelte/store";
import type { SceneData } from "../api/scene";

/** The currently loaded scene. */
export const scene = writable<SceneData>({
  name: "Untitled Scene",
  entities: [],
});

/** File path of the currently loaded scene (null if unsaved). */
export const scenePath = writable<string | null>(null);

/** Whether the scene has unsaved changes. */
export const dirty = writable(false);
