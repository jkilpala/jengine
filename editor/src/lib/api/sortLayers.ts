import { invoke } from "@tauri-apps/api/core";
import { writeTextFile } from "@tauri-apps/plugin-fs";

/** Matches Rust `crate::asset::sort_layers::SortLayer`. */
export interface SortLayer {
  name: string;
  base_z: number;
}

/** Matches Rust `crate::asset::sort_layers::SortLayers` JSON form. */
export interface SortLayersFile {
  layers: SortLayer[];
}

/** Reasonable starter template for a brand new `.sortlayers` asset. */
export function defaultSortLayersFile(): SortLayersFile {
  return {
    layers: [
      { name: "background", base_z: -1000 },
      { name: "default", base_z: 0 },
      { name: "foreground", base_z: 1000 },
      { name: "ui", base_z: 10000 },
    ],
  };
}

/** Read a `.sortlayers` file from the project assets. */
export async function readSortLayersFile(
  projectPath: string,
  relativePath: string,
): Promise<SortLayersFile> {
  const bytes = await invoke<number[]>("read_asset_binary", {
    projectPath,
    relativePath,
  });
  const text = new TextDecoder().decode(new Uint8Array(bytes));
  const parsed = JSON.parse(text);
  if (!parsed || !Array.isArray(parsed.layers)) {
    return { layers: [] };
  }
  return parsed as SortLayersFile;
}

/** Save a `.sortlayers` file to disk (overwrites existing). */
export async function saveSortLayersFile(
  fullPath: string,
  data: SortLayersFile,
): Promise<void> {
  const json = JSON.stringify(data, null, 2);
  await writeTextFile(fullPath, json);
}
