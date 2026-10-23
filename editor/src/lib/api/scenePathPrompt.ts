import { get } from "svelte/store";
import { save } from "@tauri-apps/plugin-dialog";
import { project } from "../stores/projectStore";

/**
 * Ask where a never-saved scene should live.
 *
 * Shared because two places need it and must behave identically: closing a
 * scene tab, and closing the editor with an untitled scene open. Returns null
 * when the user backs out, which every caller has to treat as "do not close" -
 * the scene is still unsaved, and going ahead discards exactly what Save was
 * asked to keep.
 */
export async function promptForScenePath(): Promise<string | null> {
  const proj = get(project);
  const chosen = await save({
    filters: [{ name: "Scene", extensions: ["scene"] }],
    defaultPath: proj ? `${proj.path}/assets` : undefined,
  });
  return (chosen as string | null) ?? null;
}
