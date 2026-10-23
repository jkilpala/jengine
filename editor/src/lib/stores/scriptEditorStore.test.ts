import { describe, it, expect, beforeEach, vi } from "vitest";

// Only the file read is faked; the reload logic under test is real.
let fileOnDisk = "";
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async () => Array.from(new TextEncoder().encode(fileOnDisk)),
}));
import { get } from "svelte/store";
import {
  scriptTabs,
  activeScriptPath,
  closeScriptTab,
  isScriptTabDirty,
  scriptTabName,
  updateScriptContent,
  normalizeEol,
  reloadScriptTabsFromDisk,
} from "./scriptEditorStore";

/**
 * The close path is the interesting part. It used to prompt from inside the
 * store with `confirm`, which could only offer discard or cancel — and a
 * native dialog that fails to render hands the caller the default, losing the
 * work silently. The prompt now lives in the UI so it can offer *save*, and
 * these pin the contract that split relies on.
 */

function tab(path: string, dirty = false) {
  return { path, content: "", language: "lua" as const, dirty, eol: "\n" as const };
}

beforeEach(() => {
  scriptTabs.set([]);
  activeScriptPath.set(null);
});

describe("script tabs", () => {
  describe("a script is not dirty until you type in it", () => {
    // CodeMirror normalises its document to LF whatever you hand it. While
    // the store kept the file's own CRLF, the two never agreed, the sync
    // effect pushed the text back in, `docChanged` fired, and every CRLF
    // script in the project opened already marked as unsaved - then asked to
    // be saved on close.
    it("ignores an update that changes nothing", () => {
      scriptTabs.set([tab("/p/a.lua")]);
      activeScriptPath.set("/p/a.lua");
      updateScriptContent("");
      expect(isScriptTabDirty("/p/a.lua")).toBe(false);
    });

    it("still notices a real edit", () => {
      scriptTabs.set([tab("/p/a.lua")]);
      activeScriptPath.set("/p/a.lua");
      updateScriptContent("local x = 1");
      expect(isScriptTabDirty("/p/a.lua")).toBe(true);
    });

    it("stores LF content whatever the file used", () => {
      // The mismatch is the bug; the tab has to speak the editor's dialect
      // and remember the file's separately.
      const crlf = "a" + "\r\n" + "b";
      expect(normalizeEol(crlf)).toEqual({
        content: "a" + "\n" + "b",
        eol: "\r\n",
      });
      expect(normalizeEol("a" + "\n" + "b").eol).toBe("\n");
    });
  });

  it("reports which tabs are dirty", () => {
    scriptTabs.set([tab("/p/a.lua", true), tab("/p/b.lua")]);
    expect(isScriptTabDirty("/p/a.lua")).toBe(true);
    expect(isScriptTabDirty("/p/b.lua")).toBe(false);
  });

  it("treats an unknown path as clean rather than throwing", () => {
    // The close handler asks before it looks the tab up; a missing tab must
    // not be reported as unsaved work.
    expect(isScriptTabDirty("/p/gone.lua")).toBe(false);
  });

  it("names a tab by filename", () => {
    expect(scriptTabName("/projects/game/assets/scripts/player.lua")).toBe(
      "player.lua",
    );
    expect(scriptTabName("C:\\\\projects\\\\game\\\\a.lua")).toBe("a.lua");
  });

  it("closes without prompting", () => {
    // If a prompt creeps back into the store the dialog would ask twice, and
    // the second answer would be the one that counted.
    scriptTabs.set([tab("/p/a.lua", true)]);
    closeScriptTab("/p/a.lua");
    expect(get(scriptTabs)).toHaveLength(0);
  });

  it("moves focus to a neighbour when the active tab closes", () => {
    scriptTabs.set([tab("/p/a.lua"), tab("/p/b.lua"), tab("/p/c.lua")]);
    activeScriptPath.set("/p/b.lua");
    closeScriptTab("/p/b.lua");
    expect(get(scriptTabs)).toHaveLength(2);
    expect(get(activeScriptPath)).not.toBe("/p/b.lua");
    expect(get(activeScriptPath)).not.toBeNull();
  });

  it("leaves the active tab alone when another closes", () => {
    scriptTabs.set([tab("/p/a.lua"), tab("/p/b.lua")]);
    activeScriptPath.set("/p/b.lua");
    closeScriptTab("/p/a.lua");
    expect(get(activeScriptPath)).toBe("/p/b.lua");
  });

  it("clears the active path when the last tab closes", () => {
    scriptTabs.set([tab("/p/a.lua")]);
    activeScriptPath.set("/p/a.lua");
    closeScriptTab("/p/a.lua");
    expect(get(activeScriptPath)).toBeNull();
  });

  it("closing an unknown path is a no-op", () => {
    scriptTabs.set([tab("/p/a.lua")]);
    closeScriptTab("/p/nope.lua");
    expect(get(scriptTabs)).toHaveLength(1);
  });

});

describe("a file replaced on disk while its tab is open", () => {
  // Replace-in-files writes to disk. A tab still holding what it read at
  // open time is not just stale on screen - that copy is what the next save
  // writes, so the replacement would be silently undone.
  //
  // Note the two path shapes, which is what broke the first version: search
  // reports project-relative paths, tabs hold absolute ones. Every test here
  // uses the real pairing rather than a matching fake on both sides.
  const ROOT = "C:/Work/game";

  beforeEach(() => {
    scriptTabs.set([]);
    activeScriptPath.set(null);
  });

  it("re-reads a tab with no unsaved edits", async () => {
    scriptTabs.set([tab(`${ROOT}/assets/scripts/a.lua`)]);
    fileOnDisk = "local replaced = 1";

    expect(
      await reloadScriptTabsFromDisk(ROOT, ["assets/scripts/a.lua"]),
    ).toEqual([]);
    expect(get(scriptTabs)[0].content).toBe("local replaced = 1");
    expect(get(scriptTabs)[0].dirty).toBe(false);
  });

  it("matches a native tab path against a relative result", async () => {
    // The exact case that silently did nothing: backslashes one side,
    // forward slashes and no root the other.
    scriptTabs.set([tab("C:\\Work\\game\\assets\\scripts\\a.lua")]);
    fileOnDisk = "changed";
    await reloadScriptTabsFromDisk(ROOT, ["assets/scripts/a.lua"]);
    expect(get(scriptTabs)[0].content).toBe("changed");
  });

  it("refuses to overwrite unsaved edits, and says which", async () => {
    scriptTabs.set([tab(`${ROOT}/assets/scripts/a.lua`, true)]);
    const before = get(scriptTabs)[0].content;
    fileOnDisk = "from disk";

    expect(
      await reloadScriptTabsFromDisk(ROOT, ["assets/scripts/a.lua"]),
    ).toEqual(["a.lua"]);
    expect(get(scriptTabs)[0].content).toBe(before);
    expect(get(scriptTabs)[0].dirty).toBe(true);
  });

  it("leaves tabs whose files were not touched", async () => {
    scriptTabs.set([
      tab(`${ROOT}/assets/scripts/a.lua`),
      tab(`${ROOT}/assets/scripts/b.lua`),
    ]);
    fileOnDisk = "changed";
    await reloadScriptTabsFromDisk(ROOT, ["assets/scripts/a.lua"]);
    expect(get(scriptTabs)[1].content).toBe("");
  });
});
