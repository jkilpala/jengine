import { derived } from "svelte/store";
import { listAssets } from "../api/project";
import { assetRefreshRequest, project } from "./projectStore";

/** Every file under the open project's `assets/`, relative to it - what an
 *  asset field's picker offers. Read once per project and per asset refresh
 *  (`requestAssetRefresh`), not once per Inspector that shows a picker. */
let cache: { key: string; files: Promise<string[]> } | null = null;

export const projectAssets = derived(
  [project, assetRefreshRequest],
  ([$project, $refresh], set) => {
    if (!$project) {
      set([]);
      return;
    }
    const key = `${$project.path}#${$refresh}`;
    if (cache?.key !== key) {
      cache = { key, files: listAssets($project.path).catch(() => []) };
    }
    let live = true;
    cache.files.then((files) => live && set(files));
    return () => {
      live = false;
    };
  },
  [] as string[],
);
