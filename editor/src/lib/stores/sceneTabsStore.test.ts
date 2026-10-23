import { describe, it, expect, beforeEach, vi } from "vitest";

// The store writes through `api/scene`, which talks to Tauri. Only the write
// is faked - everything else under test is real.
const written: { path: string; data: SceneData }[] = [];
vi.mock("../api/scene", () => ({
  saveScene: async (path: string, data: SceneData) => {
    written.push({ path, data });
  },
}));
import { get } from "svelte/store";
import { scene, scenePath, dirty } from "./sceneStore";
import { undoStack, redoStack, canUndo, clearHistory } from "./commandStore";
import { selectedEntities, sceneSelected } from "./selectionStore";
import {
  sceneTabs,
  activeSceneTab,
  activateSceneTab,
  adoptCurrentAsTab,
  focusOrCreateSceneTab,
  closeSceneTab,
  clearSceneTabs,
  findSceneTab,
  isSceneTabDirty,
  sceneTabName,
  openNewSceneTab,
  saveSceneTabAs,
} from "./sceneTabsStore";
import type { SceneData } from "../api/scene";

/**
 * These pin the one property that makes tabs worth having and is invisible
 * until it is wrong: **switching restores that scene's own undo history and
 * selection**. Getting it backwards means Ctrl+Z replays an edit from a
 * different document, which corrupts a scene quietly.
 */

function makeScene(name: string): SceneData {
  return { name, entities: [] } as SceneData;
}

/**
 * Put a scene into the live stores as if it had just been loaded.
 *
 * Uses `clearHistory()` rather than emptying the stacks by hand, because that
 * is what a real load does — and `canUndo` is a separate store that only the
 * real path keeps in step. Setting the arrays directly would leave it stale
 * and make this helper prove something production never does.
 */
function loadInto(path: string, name: string) {
  scene.set(makeScene(name));
  scenePath.set(path);
  dirty.set(false);
  clearHistory();
  selectedEntities.set(new Set());
  sceneSelected.set(false);
}

/** A command, shaped like the undo stack expects. */
function command(label: string) {
  return { label, before: makeScene("b"), after: makeScene("a") };
}

beforeEach(() => {
  clearSceneTabs();
  loadInto("/p/a.scene", "A");
});

describe("scene tabs", () => {
  it("adopts the open scene so the strip is never empty", () => {
    adoptCurrentAsTab();
    expect(get(sceneTabs)).toHaveLength(1);
    expect(get(activeSceneTab)).toBe(0);
    expect(get(sceneTabs)[0].path).toBe("/p/a.scene");
  });

  it("does not create a duplicate tab for the same path", () => {
    adoptCurrentAsTab();
    adoptCurrentAsTab();
    expect(get(sceneTabs)).toHaveLength(1);
  });

  it("does not stack up untitled tabs", () => {
    // An unsaved scene has a null path, which matches nothing by path — so
    // without an identity check every re-run would add another tab.
    clearSceneTabs();
    scenePath.set(null);
    adoptCurrentAsTab();
    adoptCurrentAsTab();
    adoptCurrentAsTab();
    expect(get(sceneTabs)).toHaveLength(1);
  });

  it("reports a new path as needing a load, and an open one as not", () => {
    adoptCurrentAsTab();
    expect(focusOrCreateSceneTab("/p/b.scene")).toBe(true);
    loadInto("/p/b.scene", "B");
    adoptCurrentAsTab();
    // Re-opening must switch, not reload: a reload would discard edits.
    expect(focusOrCreateSceneTab("/p/a.scene")).toBe(false);
  });

  it("restores each scene's own undo history", () => {
    // The property that matters most. Undoing in A must not replay a command
    // captured against B.
    adoptCurrentAsTab();
    undoStack.set([command("edit A")]);

    focusOrCreateSceneTab("/p/b.scene");
    loadInto("/p/b.scene", "B");
    undoStack.set([command("edit B1"), command("edit B2")]);

    activateSceneTab(0);
    expect(get(scenePath)).toBe("/p/a.scene");
    expect(get(undoStack)).toHaveLength(1);
    expect(get(undoStack)[0].label).toBe("edit A");

    activateSceneTab(1);
    expect(get(undoStack)).toHaveLength(2);
    expect(get(undoStack)[1].label).toBe("edit B2");
  });

  it("refreshes canUndo when the stacks are swapped", () => {
    // Swapping the arrays is not enough: the toolbar reads a separate flag,
    // and without recomputing it advertises the previous scene's history.
    adoptCurrentAsTab();
    undoStack.set([command("edit A")]);
    focusOrCreateSceneTab("/p/b.scene");
    loadInto("/p/b.scene", "B");
    expect(get(canUndo)).toBe(false);

    activateSceneTab(0);
    expect(get(canUndo)).toBe(true);
  });

  it("restores each scene's own selection", () => {
    // Entity index 7 means a different entity in each scene, so carrying a
    // selection across would select something arbitrary.
    adoptCurrentAsTab();
    selectedEntities.set(new Set([2, 5]));

    focusOrCreateSceneTab("/p/b.scene");
    loadInto("/p/b.scene", "B");
    selectedEntities.set(new Set([9]));

    activateSceneTab(0);
    expect([...get(selectedEntities)].sort()).toEqual([2, 5]);
  });

  it("carries the dirty flag per tab", () => {
    adoptCurrentAsTab();
    dirty.set(true);

    focusOrCreateSceneTab("/p/b.scene");
    loadInto("/p/b.scene", "B");
    expect(get(dirty)).toBe(false);

    activateSceneTab(0);
    expect(get(dirty)).toBe(true);
  });

  it("switching to the active tab changes nothing", () => {
    adoptCurrentAsTab();
    undoStack.set([command("edit A")]);
    activateSceneTab(0);
    expect(get(undoStack)).toHaveLength(1);
  });

  it("closing a clean inactive tab only shifts indices", () => {
    adoptCurrentAsTab();
    focusOrCreateSceneTab("/p/b.scene");
    loadInto("/p/b.scene", "B");
    adoptCurrentAsTab();
    expect(get(activeSceneTab)).toBe(1);

    expect(closeSceneTab(0)).toBe(true);
    expect(get(sceneTabs)).toHaveLength(1);
    expect(get(activeSceneTab)).toBe(0);
    expect(get(scenePath)).toBe("/p/b.scene");
  });

  it("closing the active tab falls to a neighbour", () => {
    adoptCurrentAsTab();
    focusOrCreateSceneTab("/p/b.scene");
    loadInto("/p/b.scene", "B");
    adoptCurrentAsTab();

    expect(closeSceneTab(1)).toBe(true);
    expect(get(activeSceneTab)).toBe(0);
    expect(get(scenePath)).toBe("/p/a.scene");
  });

  describe("the same file spelled differently", () => {
    // Ctrl+P builds `${proj.path}/assets/...`, which on Windows mixes
    // separators; the File > Open dialog returns backslashes throughout.
    // Comparing those as strings gave a duplicate tab for a scene already
    // open, and the two copies then diverged as you edited one.
    const MIXED = "C:\\proj/assets/scenes/a.scene";
    const NATIVE = "C:\\proj\\assets\\scenes\\a.scene";

    it("matches across separator styles", () => {
      clearSceneTabs();
      loadInto(MIXED, "A");
      adoptCurrentAsTab();
      expect(findSceneTab(NATIVE)).toBe(0);
    });

    it("switches instead of opening a second tab", () => {
      clearSceneTabs();
      loadInto(MIXED, "A");
      adoptCurrentAsTab();
      // false means "already open, switched to it" — no load needed.
      expect(focusOrCreateSceneTab(NATIVE)).toBe(false);
      expect(get(sceneTabs)).toHaveLength(1);
    });

    it("ignores case, because Windows does", () => {
      clearSceneTabs();
      loadInto("C:/proj/assets/scenes/A.scene", "A");
      adoptCurrentAsTab();
      expect(findSceneTab("c:/proj/assets/scenes/a.scene")).toBe(0);
    });

    it("still tells genuinely different scenes apart", () => {
      clearSceneTabs();
      loadInto(MIXED, "A");
      adoptCurrentAsTab();
      expect(findSceneTab("C:/proj/assets/scenes/b.scene")).toBe(-1);
    });
  });

  it("finds a tab by path and reports -1 for an unknown one", () => {
    adoptCurrentAsTab();
    expect(findSceneTab("/p/a.scene")).toBe(0);
    expect(findSceneTab("/p/nope.scene")).toBe(-1);
    expect(findSceneTab(null)).toBe(-1);
  });

  it("reports the active tab's dirtiness from the live store", () => {
    // The dot in the tab strip and the close prompt must read the same
    // value, or one says there is unsaved work while the other closes
    // without asking — which is how work disappeared.
    adoptCurrentAsTab();
    expect(isSceneTabDirty(0)).toBe(false);
    dirty.set(true);
    expect(isSceneTabDirty(0)).toBe(true);
  });

  it("reports a parked tab's dirtiness from its snapshot", () => {
    adoptCurrentAsTab();
    dirty.set(true);

    focusOrCreateSceneTab("/p/b.scene");
    loadInto("/p/b.scene", "B");
    adoptCurrentAsTab();

    // A is parked and was dirty when it was put away.
    expect(isSceneTabDirty(0)).toBe(true);
    expect(isSceneTabDirty(1)).toBe(false);
  });

  it("names a tab by filename, and an unsaved one plainly", () => {
    adoptCurrentAsTab();
    expect(sceneTabName(0)).toBe("a.scene");

    clearSceneTabs();
    scenePath.set(null);
    adoptCurrentAsTab();
    expect(sceneTabName(0)).toBe("Untitled Scene");
  });

  it("closing no longer prompts on its own", () => {
    // The prompt moved to the UI so it can offer Save as well as Discard;
    // the store closes what it is told to close. If this starts returning
    // false for a dirty tab, the prompt has crept back in and the dialog
    // would be asked twice.
    adoptCurrentAsTab();
    dirty.set(true);
    expect(closeSceneTab(0)).toBe(true);
  });

  describe("File > New", () => {
    it("opens a tab without disturbing the current one", () => {
      // Nothing is discarded, which is why New asks no questions.
      adoptCurrentAsTab();
      dirty.set(true);
      undoStack.set([command("edit A")]);

      openNewSceneTab(makeScene("Untitled"));

      expect(get(sceneTabs)).toHaveLength(2);
      expect(get(activeSceneTab)).toBe(1);
      expect(get(scenePath)).toBeNull();
      expect(get(dirty)).toBe(false);
      expect(get(undoStack)).toHaveLength(0);

      // A is intact, edits and history and all.
      activateSceneTab(0);
      expect(get(scenePath)).toBe("/p/a.scene");
      expect(get(dirty)).toBe(true);
      expect(get(undoStack)[0].label).toBe("edit A");
    });

    it("gives each new scene its own tab", () => {
      // Two never-saved scenes both have a null path, so they cannot be told
      // apart by path — inferring the tab would make the second reuse the
      // first and silently drop what was in it.
      adoptCurrentAsTab();
      openNewSceneTab(makeScene("First"));
      openNewSceneTab(makeScene("Second"));

      expect(get(sceneTabs)).toHaveLength(3);
      expect(get(scene).name).toBe("Second");
      activateSceneTab(1);
      expect(get(scene).name).toBe("First");
    });
  });

  it("clears everything when the project closes", () => {
    adoptCurrentAsTab();
    clearSceneTabs();
    expect(get(sceneTabs)).toHaveLength(0);
    expect(get(activeSceneTab)).toBe(-1);
  });
});

describe("saving a scene that has never been saved", () => {
  // Save in the close prompt used to fail outright for an untitled scene:
  // there was nowhere to write, and the only sign was a console line behind
  // the modal. Choosing a path has to actually stick to the tab.
  beforeEach(() => {
    written.length = 0;
    clearSceneTabs();
  });

  it("adopts the chosen path", async () => {
    openNewSceneTab({ name: "Untitled Scene", entities: [] } as SceneData);
    expect(get(sceneTabs)[0].path).toBeNull();

    expect(await saveSceneTabAs(0, "C:/proj/assets/scenes/new.scene")).toBe(true);

    expect(get(sceneTabs)[0].path).toBe("C:/proj/assets/scenes/new.scene");
    expect(get(sceneTabs)[0].dirty).toBe(false);
    expect(written).toHaveLength(1);
  });

  it("names the scene after the file it landed in", async () => {
    // Otherwise every scene saved this way is called "Untitled Scene"
    // *inside* the file, whatever the filename says.
    openNewSceneTab({ name: "Untitled Scene", entities: [] } as SceneData);
    await saveSceneTabAs(0, "C:/proj/assets/scenes/arena.scene");
    expect(written[0].data.name).toBe("arena");
  });

  it("leaves a name the user chose alone", async () => {
    openNewSceneTab({ name: "Boss Room", entities: [] } as SceneData);
    await saveSceneTabAs(0, "C:/proj/assets/scenes/arena.scene");
    expect(written[0].data.name).toBe("Boss Room");
  });
});
