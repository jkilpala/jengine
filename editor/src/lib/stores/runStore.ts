import { get, writable } from "svelte/store";
import { buildAndRunNative, stopNativeInstances } from "../api/build";
import { beginBuild, buildRunning, endBuild } from "./buildStore";
import { logError, logInfo } from "./consoleStore";
import { focusPanel } from "./layoutStore";
import { project } from "./projectStore";
import { scenePath } from "./sceneStore";

/**
 * Build & Run (Build menu, Ctrl+B): the project's native runtime compiled
 * and started from the project folder - one copy, or up to four side by
 * side, each with its own saves (`jengine_builder::native_run`), as Defold's
 * "Launched Instance Count". The count is the editor's, kept across starts.
 */
export const MAX_INSTANCES = 4;
const COUNT_KEY = "jengine-editor-launch-instances";

function loadCount(): number {
  try {
    const n = Number(localStorage.getItem(COUNT_KEY));
    return Number.isInteger(n) && n >= 1 && n <= MAX_INSTANCES ? n : 1;
  } catch {
    return 1;
  }
}

export const instanceCount = writable<number>(loadCount());
instanceCount.subscribe((n) => {
  try {
    localStorage.setItem(COUNT_KEY, String(n));
  } catch {
    // A private window or blocked storage: the count lasts the session.
  }
});

/**
 * An open scene's path as the runtime takes it - under the project's
 * `assets/` - or null when it is not in there.
 */
export function sceneUnderAssets(projectPath: string, fullPath: string | null): string | null {
  if (!fullPath) return null;
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
  const assets = norm(projectPath) + "/assets/";
  const full = norm(fullPath);
  return full.toLowerCase().startsWith(assets.toLowerCase()) ? full.slice(assets.length) : null;
}

/** Build, then start the instances; `currentScene` starts at the open scene
 * instead of the project's start scene. Its output is in the Build panel. */
export async function buildAndRun(currentScene: boolean): Promise<void> {
  const proj = get(project);
  if (!proj || get(buildRunning)) return;
  const count = get(instanceCount);
  let scene: string | null = null;
  if (currentScene) {
    scene = sceneUnderAssets(proj.path, get(scenePath));
    if (!scene) {
      logError("Build & Run Current Scene: the open scene is not under the project's assets/ folder.");
      return;
    }
  }
  const what = `${scene ?? "the start scene"}, ${count} instance${count === 1 ? "" : "s"}`;
  logInfo(`Build & Run: ${what}`);
  beginBuild(`Build & Run (native, release) - ${what}`);
  focusPanel("build");
  try {
    const result = await buildAndRunNative(proj.path, scene, count);
    endBuild(`Build & Run: ${result}`);
  } catch (e) {
    logError(`Build & Run failed: ${e}`);
    endBuild(`Build & Run failed: ${e}`);
  }
}

export async function closeInstances(): Promise<void> {
  try {
    const n = await stopNativeInstances();
    logInfo(n > 0 ? `Closed ${n} running instance${n === 1 ? "" : "s"}.` : "No instances were running.");
  } catch (e) {
    logError(`Closing instances failed: ${e}`);
  }
}
