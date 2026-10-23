import { derived, writable } from "svelte/store";
import { readAssetBinary } from "../api/project";
import type { Tables } from "../localeEdit";
import { projectAssets } from "./projectAssetsStore";
import { project } from "./projectStore";

/** Bumped to read the string tables again (after a table is saved). */
export const localeTablesChanged = writable(0);

/**
 * The project's string tables - every `.data` under `locale/` - keyed as the
 * engine keys data tables (assets-relative, no `.data`). Read once per
 * project, asset refresh and `localeTablesChanged`.
 */
export const localeTables = derived(
  [project, projectAssets, localeTablesChanged],
  ([$project, $assets], set) => {
    if (!$project) {
      set({});
      return;
    }
    const files = $assets.filter((f) => f.replace(/\\/g, "/").startsWith("locale/") && f.endsWith(".data"));
    let live = true;
    const decoder = new TextDecoder();
    Promise.all(
      files.map(async (rel) => {
        try {
          const parsed = JSON.parse(decoder.decode(new Uint8Array(await readAssetBinary($project.path, rel))));
          return [rel.replace(/\\/g, "/").replace(/\.data$/, ""), Array.isArray(parsed?.rows) ? parsed.rows : []] as const;
        } catch {
          return null;
        }
      }),
    ).then((entries) => {
      if (!live) return;
      const tables: Tables = {};
      for (const e of entries) if (e) tables[e[0]] = e[1];
      set(tables);
    });
    return () => {
      live = false;
    };
  },
  {} as Tables,
);

/** The language the scene view previews `text_key` text in; empty for the
 *  scene as authored. Edit mode only - in Play the game's language rules. */
export const previewLanguage = writable("");
