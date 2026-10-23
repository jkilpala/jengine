import { derived, writable } from "svelte/store";
import { listPrefabs, type PrefabEntry } from "../api/project";
import { prefabKey, resolveVariants, type PrefabEntity } from "../prefabInstance";
import { assetRefreshRequest, project } from "./projectStore";
import { scene } from "./sceneStore";
import { withSceneParts } from "../prefabParts";

/** Bumped when a `.prefab` is written, so everything showing a prefab -
 *  the Inspector's instances, the viewport's prefab map - reads it again. */
export const prefabsChanged = writable(0);
export function notifyPrefabsChanged() {
  prefabsChanged.update((n) => n + 1);
}

export interface PrefabLibrary {
  /** Every prefab file, as listed. */
  entries: PrefabEntry[];
  /** Each prefab as written, by the id an instance names it with. */
  raw: Map<string, PrefabEntity>;
  /** Each prefab with its variant base merged in - what an instance of it
   *  is built from. */
  resolved: Map<string, PrefabEntity>;
}

const EMPTY: PrefabLibrary = { entries: [], raw: new Map(), resolved: new Map() };

/** The open project's prefabs, read once per project, asset refresh and
 *  prefab save. */
export const prefabLibrary = derived(
  [project, assetRefreshRequest, prefabsChanged],
  ([$project], set) => {
    if (!$project) {
      set(EMPTY);
      return;
    }
    let live = true;
    listPrefabs($project.path)
      .then((entries) => {
        if (!live) return;
        const raw = new Map<string, PrefabEntity>();
        for (const e of entries) raw.set(prefabKey(e), e.data as PrefabEntity);
        set({ entries, raw, resolved: resolveVariants(raw) });
      })
      .catch(() => live && set(EMPTY));
    return () => {
      live = false;
    };
  },
  EMPTY,
);

/** The prefabs, plus every part of every instance placed in the open scene
 *  (`prefabParts.withSceneParts`) - the map every scene-side editor resolves
 *  entities through (`viewOf`, `editThroughInstance`), so a scene entity
 *  laid over a part is shown and edited as that part. */
export const scenePrefabs = derived([prefabLibrary, scene], ([$lib, $scene]) =>
  withSceneParts($lib.resolved, $scene?.entities ?? []),
);
