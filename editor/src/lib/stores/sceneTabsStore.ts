import { writable, get } from "svelte/store";
import type { SceneData } from "../api/scene";
import { saveScene } from "../api/scene";
import { logInfo, logError } from "./consoleStore";
import { scene, scenePath, dirty } from "./sceneStore";
import { undoStack, redoStack, refreshUndoFlags, type Command } from "./commandStore";
import { selectedEntities, sceneSelected } from "./selectionStore";

/**
 * One open scene, with everything that belongs to *that* scene rather than to
 * the editor.
 *
 * Undo history and selection are per-tab because they are meaningless across
 * documents: undoing in scene A must not replay a command captured in scene B,
 * and entity index 7 refers to a different entity in each.
 */
export interface SceneTab {
  /** Absolute path, or null for a scene that has never been saved. */
  path: string | null;
  scene: SceneData;
  dirty: boolean;
  undo: Command[];
  redo: Command[];
  selection: Set<number>;
  sceneSelected: boolean;
}

/** Every open scene, in tab-strip order. */
export const sceneTabs = writable<SceneTab[]>([]);

/** Index into `sceneTabs` of the scene currently in the editor, or -1. */
export const activeSceneTab = writable(-1);

/**
 * Copy the live editor stores into the tab they belong to.
 *
 * The live stores stay the single source of truth for the *active* scene —
 * that is what keeps the fifty-odd components that read `$scene` working
 * unchanged. A tab is where a scene parks while another is in front.
 */
function captureActive() {
  const idx = get(activeSceneTab);
  if (idx < 0) return;
  sceneTabs.update((tabs) => {
    const tab = tabs[idx];
    if (!tab) return tabs;
    const next = [...tabs];
    next[idx] = {
      ...tab,
      path: get(scenePath),
      scene: get(scene),
      dirty: get(dirty),
      undo: get(undoStack),
      redo: get(redoStack),
      selection: get(selectedEntities),
      sceneSelected: get(sceneSelected),
    };
    return next;
  });
}

/** Load a tab's state into the live editor stores. */
function restore(tab: SceneTab) {
  scene.set(tab.scene);
  scenePath.set(tab.path);
  dirty.set(tab.dirty);
  undoStack.set(tab.undo);
  redoStack.set(tab.redo);
  refreshUndoFlags();
  selectedEntities.set(tab.selection);
  sceneSelected.set(tab.sceneSelected);
}

/**
 * Open `data` as a new tab and switch to it.
 *
 * For File > New. Explicit rather than leaving the tab to be inferred from a
 * path change, because a never-saved scene has a `null` path and two of them
 * are indistinguishable that way — inferring would make a second New reuse
 * the first untitled tab and quietly drop what was in it.
 *
 * Parks the outgoing scene first, so nothing is lost and no prompt is needed:
 * a new tab discards nothing.
 */
export function openNewSceneTab(data: SceneData) {
  captureActive();
  const tab: SceneTab = {
    path: null,
    scene: data,
    dirty: false,
    undo: [],
    redo: [],
    selection: new Set(),
    sceneSelected: false,
  };
  sceneTabs.update((tabs) => [...tabs, tab]);
  activeSceneTab.set(get(sceneTabs).length - 1);
  restore(tab);
}

/**
 * Compare two scene paths as *files*, not as strings.
 *
 * The same scene arrives spelled differently depending on how it was opened:
 * Ctrl+P and the start screen build `` `${proj.path}/assets/...` ``, which on
 * Windows mixes separators (`C:\\proj/assets/a.scene`), while the File > Open
 * dialog returns a native path with backslashes throughout. Comparing those as
 * strings says they are different files, so opening a scene by a second route
 * gave you a duplicate tab of a scene you already had open — with the two
 * copies then diverging as you edited one of them.
 *
 * Separators are normalised, and the comparison is case-insensitive because
 * the editor runs on Windows, where `A.scene` and `a.scene` *are* the same
 * file and treating them as two would reintroduce the same bug.
 */
function samePath(a: string | null, b: string | null): boolean {
  if (!a || !b) return false;
  const norm = (v: string) =>
    v.replace(/\\/g, "/").replace(/\/+/g, "/").toLowerCase();
  return norm(a) === norm(b);
}

/** Index of the tab holding `path`, or -1. */
export function findSceneTab(path: string | null): number {
  if (!path) return -1;
  return get(sceneTabs).findIndex((t) => samePath(t.path, path));
}

/**
 * Show the scene at `index`, parking whatever was in front.
 *
 * A no-op when it is already active, so clicking the current tab does not
 * churn the viewport.
 */
export function activateSceneTab(index: number) {
  const tabs = get(sceneTabs);
  if (index < 0 || index >= tabs.length) return;
  if (get(activeSceneTab) === index) return;
  captureActive();
  // Re-read: `captureActive` rewrote the array.
  const tab = get(sceneTabs)[index];
  activeSceneTab.set(index);
  restore(tab);
}

/**
 * Adopt whatever is currently in the live stores as a tab.
 *
 * Called when a scene arrives by a route that predates tabs — the start
 * screen's entry-scene load, or a brand new scene — so the strip always
 * reflects what is actually open instead of appearing empty until the second
 * scene is opened.
 */
export function adoptCurrentAsTab() {
  const path = get(scenePath);
  const active = get(activeSceneTab);
  const tabs = get(sceneTabs);

  // Already the active tab: just refresh its snapshot. Checked before the
  // path lookup because an unsaved scene has a `null` path, which matches
  // nothing - without this, every re-run would add another "Untitled Scene".
  // `===` first, so a never-saved scene (both paths null) still counts as
  // itself; `samePath` deliberately refuses a null, since an untitled scene is
  // not "the same file" as anything.
  if (
    active >= 0 &&
    tabs[active] &&
    (tabs[active].path === path || samePath(tabs[active].path, path))
  ) {
    captureActive();
    return;
  }
  const existing = findSceneTab(path);
  if (existing >= 0) {
    activeSceneTab.set(existing);
    captureActive();
    return;
  }
  const tab: SceneTab = {
    path,
    scene: get(scene),
    dirty: get(dirty),
    undo: get(undoStack),
    redo: get(redoStack),
    selection: get(selectedEntities),
    sceneSelected: get(sceneSelected),
  };
  sceneTabs.update((tabs) => [...tabs, tab]);
  activeSceneTab.set(get(sceneTabs).length - 1);
}

/**
 * Open `path` in a tab, focusing it if already open.
 *
 * Returns true when the caller still needs to load the scene from disk —
 * i.e. this is a new tab. An already-open scene is switched to as-is,
 * unsaved edits and undo history intact, which is the whole point of tabs.
 */
export function focusOrCreateSceneTab(path: string): boolean {
  const existing = findSceneTab(path);
  if (existing >= 0) {
    activateSceneTab(existing);
    return false;
  }
  captureActive();
  const placeholder: SceneTab = {
    path,
    scene: { name: "", entities: [] },
    dirty: false,
    undo: [],
    redo: [],
    selection: new Set(),
    sceneSelected: false,
  };
  sceneTabs.update((tabs) => [...tabs, placeholder]);
  activeSceneTab.set(get(sceneTabs).length - 1);
  return true;
}

/** Whether the scene in tab `index` has unsaved edits. */
export function isSceneTabDirty(index: number): boolean {
  const tab = get(sceneTabs)[index];
  if (!tab) return false;
  // The active tab's truth is the live store; a parked tab's is its snapshot.
  return get(activeSceneTab) === index ? get(dirty) : tab.dirty;
}

/** Display name for a tab, for prompts. */
export function sceneTabName(index: number): string {
  const tab = get(sceneTabs)[index];
  return tab?.path?.split(/[\\/]/).pop() ?? "Untitled Scene";
}

/**
 * Write tab `index` to disk.
 *
 * Works for a parked tab as well as the active one: a tab carries its own
 * scene, so saving does not require switching to it first — which would make
 * "save this one and close it" a visible flicker through a scene the user did
 * not ask to see.
 *
 * Returns false if it could not be saved, so a caller does not then close the
 * tab and lose the work anyway.
 */
export async function saveSceneTab(index: number): Promise<boolean> {
  const tabs = get(sceneTabs);
  const tab = tabs[index];
  if (!tab) return false;

  const isActive = get(activeSceneTab) === index;
  const path = isActive ? get(scenePath) : tab.path;
  if (!path) {
    // Never saved, so there is nowhere to write. Choosing a location needs a
    // file dialog, which is the UI's job - see `saveSceneTabAs`. Callers that
    // can ask must do so before getting here; those that cannot report the
    // failure rather than dropping the scene.
    logError(
      `${sceneTabName(index)} has never been saved — choose a location for it.`,
    );
    return false;
  }

  const data = isActive ? get(scene) : tab.scene;
  try {
    await saveScene(path, data);
    if (isActive) dirty.set(false);
    sceneTabs.update((list) =>
      list.map((t, i) => (i === index ? { ...t, dirty: false } : t)),
    );
    logInfo(`Saved scene: ${path}`);
    return true;
  } catch (e) {
    logError(`Failed to save scene: ${e}`);
    return false;
  }
}

/**
 * Give tab `index` a path for the first time, then write it there.
 *
 * Split from [`saveSceneTab`] because choosing the path needs a file dialog
 * and a store should not open one. Without this an untitled scene could not
 * be saved from the close prompt at all: Save had nowhere to write, failed,
 * and left the reason in the console - behind the modal the user was looking
 * at.
 *
 * The scene's *name* follows the filename while it is still the placeholder,
 * matching File > Save; otherwise every scene saved this way is called
 * "Untitled Scene" inside the file.
 */
export async function saveSceneTabAs(
  index: number,
  path: string,
): Promise<boolean> {
  const tabs = get(sceneTabs);
  const tab = tabs[index];
  if (!tab) return false;

  const isActive = get(activeSceneTab) === index;
  const data = isActive ? get(scene) : tab.scene;
  const filename = path.replace(/^.*[\\/]/, "").replace(/\.scene$/i, "");
  if (data.name === "Untitled Scene") data.name = filename;

  try {
    await saveScene(path, data);
  } catch (e) {
    logError(`Failed to save scene: ${e}`);
    return false;
  }

  if (isActive) {
    scene.set(data);
    scenePath.set(path);
    dirty.set(false);
  }
  sceneTabs.update((list) =>
    list.map((t, i) =>
      i === index ? { ...t, path, scene: data, dirty: false } : t,
    ),
  );
  logInfo(`Saved scene: ${path}`);
  return true;
}

/**
 * Close a tab, discarding whatever is in it.
 *
 * Asks nothing: the prompt is the UI's job, because a useful one offers
 * *save* as well as discard and cancel, and a store cannot render that.
 * Callers that might be dropping unsaved work check
 * [`isSceneTabDirty`] first.
 *
 * Closing the last tab leaves the live stores alone rather than blanking the
 * editor — an empty viewport with no way back is a worse state than the scene
 * you were just looking at.
 */
export function closeSceneTab(index: number): boolean {
  const tabs = get(sceneTabs);
  const tab = tabs[index];
  if (!tab) return false;

  const isActive = get(activeSceneTab) === index;

  if (!isActive) {
    // Simple case: the live stores are untouched, only indices shift.
    sceneTabs.update((t) => t.filter((_, i) => i !== index));
    activeSceneTab.update((a) => (a > index ? a - 1 : a));
    return true;
  }

  const remaining = tabs.filter((_, i) => i !== index);
  sceneTabs.set(remaining);
  if (remaining.length === 0) {
    activeSceneTab.set(-1);
    return true;
  }
  // Fall to the neighbour on the left, or the new first tab.
  const nextIndex = Math.min(index, remaining.length - 1);
  activeSceneTab.set(nextIndex);
  restore(remaining[nextIndex]);
  return true;
}

/** Forget every tab — used when the project closes. */
export function clearSceneTabs() {
  sceneTabs.set([]);
  activeSceneTab.set(-1);
}
