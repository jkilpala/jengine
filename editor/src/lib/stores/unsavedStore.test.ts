import { describe, it, expect, beforeEach } from "vitest";
import { get } from "svelte/store";
import { unsavedWork, saveAllUnsaved } from "./unsavedStore";
import { dirty as sceneDirty, scenePath } from "./sceneStore";
import { scriptTabs } from "./scriptEditorStore";
import { guiDirty } from "./guiEditorStore";
import { fontDirty } from "./fontEditorStore";
import { tilemapDirty } from "./tilemapEditorStore";
import {
  sceneTabs,
  activeSceneTab,
  clearSceneTabs,
} from "./sceneTabsStore";

/**
 * This store is the only thing standing between a window close and losing
 * every open editor. Fourteen separate `dirty` flags feed it, and the failure
 * mode is silent: a flag nobody aggregates is work nobody is warned about.
 */

beforeEach(() => {
  clearSceneTabs();
  sceneDirty.set(false);
  scenePath.set(null);
  scriptTabs.set([]);
  guiDirty.set(false);
  fontDirty.set(false);
  tilemapDirty.set(false);
});

describe("unsavedWork", () => {
  it("is empty when nothing is modified", () => {
    expect(get(unsavedWork)).toEqual([]);
  });

  it("names the scene by its filename", () => {
    scenePath.set("/projects/game/assets/scenes/hub.scene");
    sceneDirty.set(true);
    expect(get(unsavedWork)).toEqual(["Scene: hub.scene"]);
  });

  it("calls an unsaved scene untitled rather than showing null", () => {
    scenePath.set(null);
    sceneDirty.set(true);
    expect(get(unsavedWork)).toEqual(["Scene: untitled"]);
  });

  it("lists only the dirty script tabs", () => {
    scriptTabs.set([
      { path: "/p/a.lua", content: "", language: "lua", dirty: true, eol: "\n" as const },
      { path: "/p/b.lua", content: "", language: "lua", dirty: false, eol: "\n" as const },
      { path: "/p/c.lua", content: "", language: "lua", dirty: true, eol: "\n" as const },
    ]);
    expect(get(unsavedWork)).toEqual(["Script: a.lua", "Script: c.lua"]);
  });

  it("reports every dirty editor together", () => {
    // The case the prompt exists for: several editors open, and the user
    // needs to know which ones rather than 'you have unsaved changes'.
    scenePath.set("/p/main.scene");
    sceneDirty.set(true);
    scriptTabs.set([
      { path: "/p/player.lua", content: "", language: "lua", dirty: true, eol: "\n" as const },
    ]);
    guiDirty.set(true);
    fontDirty.set(true);

    const items = get(unsavedWork);
    expect(items).toContain("Scene: main.scene");
    expect(items).toContain("Script: player.lua");
    expect(items).toContain("GUI document");
    expect(items).toContain("Font");
    expect(items).toHaveLength(4);
  });

  it("names a dirty scene the user has switched away from", () => {
    // The bug this covers: `dirty` describes only the scene currently in the
    // editor. Edit scene A, switch to B, close the window — A's changes were
    // invisible to the prompt and went out with the window.
    sceneTabs.set([
      {
        path: "/p/a.scene",
        scene: { name: "A", entities: [] } as never,
        dirty: true,
        undo: [],
        redo: [],
        selection: new Set(),
        sceneSelected: false,
      },
      {
        path: "/p/b.scene",
        scene: { name: "B", entities: [] } as never,
        dirty: false,
        undo: [],
        redo: [],
        selection: new Set(),
        sceneSelected: false,
      },
    ]);
    activeSceneTab.set(1);

    expect(get(unsavedWork)).toEqual(["Scene: a.scene"]);
  });

  it("does not name the active scene twice", () => {
    // The active tab's snapshot and the live `dirty` flag describe the same
    // scene; listing both would show it two times in the prompt.
    scenePath.set("/p/a.scene");
    sceneDirty.set(true);
    sceneTabs.set([
      {
        path: "/p/a.scene",
        scene: { name: "A", entities: [] } as never,
        dirty: true,
        undo: [],
        redo: [],
        selection: new Set(),
        sceneSelected: false,
      },
    ]);
    activeSceneTab.set(0);

    expect(get(unsavedWork)).toEqual(["Scene: a.scene"]);
  });

  it("reacts to a flag clearing", () => {
    tilemapDirty.set(true);
    expect(get(unsavedWork)).toEqual(["Tilemap"]);
    tilemapDirty.set(false);
    expect(get(unsavedWork)).toEqual([]);
  });
});

describe("saveAllUnsaved", () => {
  it("reports nothing to do when everything is clean", async () => {
    expect(await saveAllUnsaved()).toEqual([]);
  });

  it("names what it could not save rather than claiming success", async () => {
    // The dialog's Save button is only trustworthy if a failure comes back.
    // A dirty scene that is in no tab has no path to write to, so it cannot
    // be saved from here — and saying so is the whole point.
    sceneDirty.set(true);
    scenePath.set(null);
    const failed = await saveAllUnsaved();
    expect(failed.length).toBeGreaterThan(0);
    expect(failed[0]).toContain("Scene");
  });

  it("does not report editors that were never dirty", async () => {
    guiDirty.set(false);
    fontDirty.set(false);
    expect(await saveAllUnsaved()).toEqual([]);
  });
});
