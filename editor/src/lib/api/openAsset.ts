import { get } from "svelte/store";
import { scene, scenePath, dirty } from "../stores/sceneStore";
import { clearSelection } from "../stores/selectionStore";
import { clearHistory } from "../stores/commandStore";
import { logInfo, logError } from "../stores/consoleStore";
import { loadScene } from "./scene";
import {
  focusOrCreateSceneTab,
  findSceneTab,
  closeSceneTab,
} from "../stores/sceneTabsStore";
import { openInCodeEditor, type ProjectInfo } from "./project";

/**
 * Open a project file in whichever editor handles its type.
 *
 * Extracted from the asset browser's double-click handler so every way of
 * reaching a file — double-click, Ctrl+P, a find-in-files result — lands in
 * the same editor. Three copies of a fifteen-way extension switch would drift
 * the moment a new asset type is added, and the symptom would be "double-click
 * opens the tilemap editor but Ctrl+P opens raw JSON".
 *
 * Every panel is summoned by a `CustomEvent` on `window` that `App.svelte`
 * listens for; that indirection predates this function and is what lets a
 * component with no knowledge of the tab bar open a tab.
 *
 * `relPath` is project-root-relative with forward slashes (`assets/x/y.lua`) —
 * the shape `AssetNode.path` and a search hit's `path` both already use.
 *
 * `line` is honoured only where the target editor can act on it (scripts
 * today); elsewhere it is ignored rather than being an error, so a
 * find-in-files hit in a `.scene` still opens the scene.
 */
export interface OpenAssetOptions {
  /** 1-based line to reveal, for editors that support it. */
  line?: number;
}

/** Fire the `CustomEvent` that `App.svelte` turns into a tab. */
function openPanel(name: string, detail: Record<string, unknown>) {
  window.dispatchEvent(new CustomEvent(name, { detail }));
}

/**
 * Load a `.scene` into the editor, guarding unsaved work.
 *
 * The guard lives here rather than at each call site because every entry
 * point needs it: replacing the open scene throws away the undo stack, and a
 * double-click or a stray Ctrl+P selection used to do that silently.
 */
async function openScene(fullPath: string, relPath: string): Promise<void> {
  // Already open? Switch to it, edits and undo history intact. This is what
  // makes tabs worth having - re-opening a scene you are working on must not
  // silently reload it from disk.
  if (!focusOrCreateSceneTab(fullPath)) {
    logInfo(`Switched to scene: ${relPath}`);
    return;
  }
  try {
    const s = await loadScene(fullPath);
    scene.set(s);
    scenePath.set(fullPath);
    clearSelection();
    dirty.set(false);
    clearHistory();
    logInfo(`Opened scene: ${relPath}`);
  } catch (e) {
    logError(`Failed to open scene: ${e}`);
    // The tab was created optimistically; drop it so a failed load does not
    // leave an empty tab pointing at an unreadable file.
    const idx = findSceneTab(fullPath);
    if (idx >= 0) closeSceneTab(idx);
  }
}

/**
 * The project's entry scene, as a project-root-relative path, or `null` when
 * none is set.
 *
 * `entry_scene` (and the legacy top-level `default_scene`) are stored relative
 * to `assets/`, matching how `jengine-runtime` resolves them — so this adds
 * the prefix that `openAsset` expects.
 */
export function entryScenePath(proj: ProjectInfo): string | null {
  const rel = proj.settings?.entry_scene ?? proj.default_scene;
  return rel ? `assets/${rel}` : null;
}

/**
 * Open the project's entry scene — the one the runtime boots into.
 *
 * Goes through `openAsset`, so it inherits the unsaved-changes guard; the
 * start screen deliberately does not, because nothing can be dirty yet when a
 * project has only just been opened.
 */
export async function openEntryScene(proj: ProjectInfo): Promise<void> {
  const rel = entryScenePath(proj);
  if (!rel) {
    logError(
      "No entry scene set for this project — set one in Project Settings.",
    );
    return;
  }
  await openAsset(proj, rel);
}

/**
 * True when this editor knows how to open `relPath`.
 *
 * Used by Ctrl+P to grey out — rather than hide — a file it cannot open, so
 * the list stays a faithful view of the project.
 */
export function canOpenAsset(relPath: string): boolean {
  return HANDLED_EXTS.has(extensionOf(relPath));
}

function extensionOf(relPath: string): string {
  const name = relPath.split("/").pop() ?? relPath;
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

const HANDLED_EXTS = new Set([
  "scene",
  "prefab",
  "lua",
  "luau",
  "js",
  "gui",
  "atlas",
  "shader",
  "particle",
  "anim",
  "font",
  "tilemap",
  "material",
  "skelanim",
  "rig",
  "behavior",
  "sortlayers",
  "dataschema",
  "data",
  "ogg",
  "wav",
  "mp3",
  "flac",
  "png",
  "jpg",
  "jpeg",
  "webp",
  "mp4",
  "webm",
  "mov",
  "ttf",
  "otf",
  // A Rust package's files (`packages/`), which go to the code editor.
  "rs",
  "toml",
  "md",
]);

export async function openAsset(
  proj: ProjectInfo,
  relPath: string,
  opts: OpenAssetOptions = {},
): Promise<void> {
  const fullPath = `${proj.path}/${relPath}`;
  // Several editors take a path already relative to `assets/`, because they
  // re-prefix it themselves.
  const underAssets = relPath.startsWith("assets/")
    ? relPath.slice("assets/".length)
    : relPath;

  switch (extensionOf(relPath)) {
    case "scene":
      await openScene(fullPath, relPath);
      return;
    case "prefab":
      openPanel("open-prefab-editor", { fullPath });
      return;
    case "lua":
    case "luau":
    case "js":
      openPanel("open-script-editor", { fullPath, line: opts.line });
      return;
    case "gui":
      openPanel("open-gui-editor", {
        projectPath: proj.path,
        relativePath: relPath,
      });
      return;
    case "atlas":
      openPanel("open-atlas-editor", {
        projectPath: proj.path,
        atlasPath: relPath,
      });
      return;
    case "shader":
      // `shaderLoad` prefixes `assets/` internally.
      openPanel("open-shader-editor", {
        projectPath: proj.path,
        relativePath: underAssets,
      });
      return;
    case "particle":
      openPanel("open-particle-editor", { fullPath });
      return;
    case "anim":
      openPanel("open-animation-editor", { fullPath });
      return;
    case "font":
      openPanel("open-font-editor", { fullPath });
      return;
    case "tilemap":
      openPanel("open-tilemap-editor", { fullPath });
      return;
    case "behavior":
      openPanel("open-behavior-editor", { fullPath });
      logInfo(`Opened behavior graph: ${relPath}`);
      return;
    case "sortlayers":
      openPanel("open-sortlayers-editor", { fullPath });
      logInfo(`Opened sort layers: ${relPath}`);
      return;
    case "dataschema":
      openPanel("open-data-schema-editor", { fullPath });
      logInfo(`Opened data schema: ${relPath}`);
      return;
    case "data":
      openPanel("open-data-table-editor", { fullPath });
      logInfo(`Opened data table: ${relPath}`);
      return;
    case "ogg":
    case "wav":
    case "mp3":
    case "flac":
      openPanel("open-audio-preview", {
        projectPath: proj.path,
        relativePath: underAssets,
      });
      return;
    case "png":
    case "jpg":
    case "jpeg":
    case "webp":
      openPanel("open-image-preview", {
        projectPath: proj.path,
        relativePath: relPath,
      });
      return;
    case "skelanim":
    case "rig":
      // The skeletal editor (`SkeletalEditor.svelte`): a clip's dope sheet
      // over a posed skeleton, or a rig's bones.
      openPanel("open-skeletal-editor", { projectPath: proj.path, relativePath: relPath });
      return;
    case "mp4":
    case "webm":
    case "mov":
    case "ttf":
    case "otf":
      // A video plays, a font shows its glyphs (`MediaPreview.svelte`).
      openPanel("open-media-preview", { projectPath: proj.path, relativePath: relPath });
      return;
    case "material":
      // The material editor (`MaterialEditor.svelte`); it has "Open as text"
      // for the configured external editor.
      openPanel("open-material-editor", { projectPath: proj.path, relativePath: relPath });
      return;
    case "rs":
    case "toml":
    case "md":
      // A package's source: the editor has no Rust editor of its own, so the
      // configured code editor (Project Settings), or VS Code / Cursor.
      try {
        await openInCodeEditor(proj.path, relPath, proj.settings?.code_editor ?? "");
        logInfo(`Opened ${relPath} in the code editor`);
      } catch (e) {
        logError(`${e}`);
      }
      return;
    default:
      // Reached only from callers that skipped `canOpenAsset`; say so rather
      // than doing nothing, which reads as a broken click.
      logError(`No editor for ${relPath}`);
  }
}
