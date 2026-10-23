import { writable, derived, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import { logInfo, logError } from "./consoleStore";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface ScriptTab {
  path: string;
  /** Always LF-separated - see [`normalizeEol`]. */
  content: string;
  language: "lua" | "javascript";
  dirty: boolean;
  /** The line ending the file had on disk, restored when it is written back. */
  eol: "\r\n" | "\n";
}

/**
 * Strip CR from a file's line endings, remembering which it used.
 *
 * CodeMirror normalises its document to LF no matter what you hand it. Keeping
 * the store's copy in the file's own form therefore guaranteed a mismatch:
 * the editor held LF, the store held CRLF, and the "sync store to editor"
 * effect pushed the CRLF text back in on every open - which CodeMirror
 * normalised again, fired `docChanged`, and marked the tab **dirty before the
 * user had typed a character**. Every CRLF script in the project opened with
 * a dot on it and asked to be saved on close.
 *
 * So the store speaks LF, like the editor, and `eol` carries what the file
 * wants back. That also stops the first real save from silently rewriting
 * every line ending in the file.
 */
export function normalizeEol(text: string): {
  content: string;
  eol: "\r\n" | "\n";
} {
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  return { content: text.split(/\r\n?|\n/).join("\n"), eol };
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/** All currently open script tabs. */
export const scriptTabs = writable<ScriptTab[]>([]);

/** The path of the active script tab (null if no scripts open). */
export const activeScriptPath = writable<string | null>(null);

/**
 * When set to a 1-based line number, ScriptEditor will jump to that line
 * and clear this store. Set by the console double-click handler.
 */
export const requestedScriptLine = writable<number | null>(null);

// Backward-compat derived stores used by ScriptEditor.svelte
export const scriptContent = derived(
  [scriptTabs, activeScriptPath],
  ([tabs, path]) => (path ? (tabs.find((t) => t.path === path)?.content ?? null) : null),
);
export const scriptFilePath = derived(
  [scriptTabs, activeScriptPath],
  ([_tabs, path]) => path,
);
export const scriptLanguage = derived(
  [scriptTabs, activeScriptPath],
  ([tabs, path]) =>
    (path ? (tabs.find((t) => t.path === path)?.language ?? "javascript") : "javascript") as
      | "lua"
      | "javascript",
);
export const scriptDirty = derived(
  [scriptTabs, activeScriptPath],
  ([tabs, path]) => (path ? (tabs.find((t) => t.path === path)?.dirty ?? false) : false),
);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function detectLanguage(path: string): "lua" | "javascript" {
  // Luau (native's dialect) is Lua to the editor: one highlighter, one runtime.
  if (/\.luau?$/i.test(path)) return "lua";
  return "javascript";
}

function splitPath(fullPath: string): { projectPath: string; relativePath: string } {
  const normalized = fullPath.replace(/\\/g, "/");
  const assetsIdx = normalized.indexOf("/assets/");
  if (assetsIdx >= 0) {
    return {
      projectPath: normalized.slice(0, assetsIdx),
      relativePath: normalized.slice(assetsIdx + "/assets/".length),
    };
  }
  const lastSlash = normalized.lastIndexOf("/");
  return {
    projectPath: normalized.slice(0, lastSlash),
    relativePath: normalized.slice(lastSlash + 1),
  };
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

export async function openScriptFile(fullPath: string) {
  const tabs = get(scriptTabs);
  if (tabs.find((t) => t.path === fullPath)) {
    // Already open — just switch to it
    activeScriptPath.set(fullPath);
    return;
  }
  try {
    const { projectPath, relativePath } = splitPath(fullPath);
    const bytes = await invoke<number[]>("read_asset_binary", { projectPath, relativePath });
    const text = new TextDecoder().decode(new Uint8Array(bytes));
    const { content, eol } = normalizeEol(text);
    const tab: ScriptTab = {
      path: fullPath,
      content,
      language: detectLanguage(fullPath),
      dirty: false,
      eol,
    };
    scriptTabs.update((t) => [...t, tab]);
    activeScriptPath.set(fullPath);
    logInfo(`Opened script: ${fullPath.split(/[\\/]/).pop()}`);
  } catch (e) {
    logError(`Failed to open script: ${e}`);
  }
}

/**
 * Re-read open script tabs whose files changed underneath them.
 *
 * Replace-in-files writes to disk; a tab still holds what it read when it
 * opened. Leaving it stale is not just a display problem - that stale copy is
 * what the next save writes, so the replacement would be silently undone.
 *
 * A tab with unsaved edits is left alone and named in the console instead:
 * reloading it would discard the edits, and keeping it means the two versions
 * are the user's to reconcile. Returns the paths it could not refresh.
 *
 * `changedPaths` are **project-root-relative**, the shape search results come
 * in (`assets/scripts/boot.lua`); tab paths are absolute. Taking the root as
 * an argument rather than comparing the two shapes directly is the whole
 * point: the first version of this compared them raw, matched nothing, and
 * quietly did nothing at all.
 */
export async function reloadScriptTabsFromDisk(
  projectPath: string,
  changedPaths: string[],
): Promise<string[]> {
  const norm = (v: string) => v.replace(/\\/g, "/").replace(/\/+/g, "/").toLowerCase();
  const root = norm(projectPath).replace(/\/$/, "");
  const changed = new Set(changedPaths.map((rel) => norm(`${root}/${rel}`)));
  const conflicts: string[] = [];

  for (const tab of get(scriptTabs)) {
    if (!changed.has(norm(tab.path))) continue;
    if (tab.dirty) {
      conflicts.push(scriptTabName(tab.path));
      continue;
    }
    try {
      const { projectPath, relativePath } = splitPath(tab.path);
      const bytes = await invoke<number[]>("read_asset_binary", {
        projectPath,
        relativePath,
      });
      const { content, eol } = normalizeEol(
        new TextDecoder().decode(new Uint8Array(bytes)),
      );
      scriptTabs.update((tabs) =>
        tabs.map((t) =>
          t.path === tab.path ? { ...t, content, eol, dirty: false } : t,
        ),
      );
    } catch (e) {
      logError(`Could not reload ${scriptTabName(tab.path)}: ${e}`);
      conflicts.push(scriptTabName(tab.path));
    }
  }
  return conflicts;
}

/** Close a specific script tab by path. */
/** Whether the script at `path` has unsaved edits. */
export function isScriptTabDirty(path: string): boolean {
  return get(scriptTabs).find((t) => t.path === path)?.dirty ?? false;
}

/** Filename of a script tab, for prompts. */
export function scriptTabName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/**
 * Close a script tab, discarding whatever is in it.
 *
 * Asks nothing. The prompt belongs to the UI, because a useful one offers
 * *save* alongside discard and cancel, and a store cannot render that — the
 * same split the scene tabs use. Callers that might be dropping unsaved work
 * check [`isScriptTabDirty`] first.
 */
export function closeScriptTab(path: string) {
  const tabs = get(scriptTabs);
  const idx = tabs.findIndex((t) => t.path === path);
  if (idx < 0) return;
  const newTabs = tabs.filter((t) => t.path !== path);
  scriptTabs.set(newTabs);
  const active = get(activeScriptPath);
  if (active === path) {
    const next = newTabs[Math.min(idx, newTabs.length - 1)];
    activeScriptPath.set(next?.path ?? null);
  }
}

/** Close all open script tabs (used when project closes). */
export function closeAllScriptTabs() {
  scriptTabs.set([]);
  activeScriptPath.set(null);
}

/** Close the currently active tab (backward compat for ScriptEditor.svelte). */
export function closeScriptEditor() {
  const path = get(activeScriptPath);
  if (path) closeScriptTab(path);
}

/**
 * Save a specific script tab.
 *
 * Returns whether it reached disk, so a caller saving in order to close does
 * not then close and lose the work anyway.
 */
export async function saveScriptTab(path: string): Promise<boolean> {
  const tab = get(scriptTabs).find((t) => t.path === path);
  if (!tab) return false;
  try {
    // Written back in the file's own line ending, not the editor's.
    const text =
      tab.eol === "\r\n"
        ? tab.content.split("\n").join("\r\n")
        : tab.content;
    await writeTextFile(path, text);
    scriptTabs.update((tabs) =>
      tabs.map((t) => (t.path === path ? { ...t, dirty: false } : t)),
    );
    logInfo(`Saved script: ${scriptTabName(path)}`);
    return true;
  } catch (e) {
    logError(`Failed to save script: ${e}`);
    return false;
  }
}

/** Save the currently active script tab. */
export async function saveCurrentScript() {
  const path = get(activeScriptPath);
  if (path) await saveScriptTab(path);
}

/**
 * Update content of the active script tab (called by CodeMirror on keystrokes).
 *
 * Text identical to what the tab already holds is not an edit. CodeMirror
 * fires `docChanged` for programmatic dispatches too, so without this any
 * echo between store and editor would show up as unsaved work.
 */
export function updateScriptContent(newContent: string) {
  const path = get(activeScriptPath);
  if (!path) return;
  scriptTabs.update((tabs) =>
    tabs.map((t) =>
      t.path === path && newContent !== t.content
        ? { ...t, content: newContent, dirty: true }
        : t,
    ),
  );
}
