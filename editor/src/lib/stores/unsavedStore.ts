import { derived, get } from "svelte/store";
import { dirty as sceneDirty, scenePath } from "./sceneStore";
import { scriptTabs, saveScriptTab } from "./scriptEditorStore";
import {
  sceneTabs,
  activeSceneTab,
  saveSceneTab,
  saveSceneTabAs,
} from "./sceneTabsStore";
import { animDirty, saveCurrentAnim } from "./animationEditorStore";
import { behaviorDirty, saveBehavior } from "./behaviorEditorStore";
import { dataSchemaDirty, saveCurrentDataSchema } from "./dataSchemaEditorStore";
import { dataTableDirty, saveCurrentDataTable } from "./dataTableEditorStore";
import { fontDirty, saveCurrentFont } from "./fontEditorStore";
import { guiDirty, saveCurrentGui } from "./guiEditorStore";
import { particleDirty, saveCurrentParticle } from "./particleEditorStore";
import { prefabDirty, saveCurrentPrefab } from "./prefabEditorStore";
import { sortLayersDirty, saveCurrentSortLayers } from "./sortLayersEditorStore";
import { stateMachineDirty } from "./stateMachineEditorStore";
import { tilemapDirty, saveCurrentTilemap } from "./tilemapEditorStore";
import { vectorPathDirty } from "./vectorPathEditorStore";

/**
 * Everything with unsaved changes, named for a confirmation prompt.
 *
 * Each editor tracks its own `dirty` flag — fourteen of them — and until this
 * existed nothing looked at them together. Closing the window discarded the
 * lot without a word.
 *
 * Deliberately a list of labels rather than a boolean: a prompt that says
 * *what* is unsaved is one someone can act on, and "you have unsaved changes"
 * on a window with eight editors open is not.
 *
 * `shaderMarkDirty` is not included — it holds a callback, not a flag, and the
 * shader editor exposes no dirty state to read.
 */
export const unsavedWork = derived(
  [
    sceneDirty,
    scenePath,
    sceneTabs,
    activeSceneTab,
    scriptTabs,
    animDirty,
    behaviorDirty,
    dataSchemaDirty,
    dataTableDirty,
    fontDirty,
    guiDirty,
    particleDirty,
    prefabDirty,
    sortLayersDirty,
    stateMachineDirty,
    tilemapDirty,
    vectorPathDirty,
  ],
  ([
    $sceneDirty,
    $scenePath,
    $sceneTabs,
    $activeSceneTab,
    $scriptTabs,
    $anim,
    $behavior,
    $dataSchema,
    $dataTable,
    $font,
    $gui,
    $particle,
    $prefab,
    $sortLayers,
    $stateMachine,
    $tilemap,
    $vectorPath,
  ]): string[] => {
    const out: string[] = [];
    const sceneName = (path: string | null) =>
      `Scene: ${path?.split(/[\\/]/).pop() ?? "untitled"}`;
    if ($sceneDirty) {
      out.push(sceneName($scenePath));
    }
    // Scene tabs other than the active one. The live `dirty` flag describes
    // only the scene currently in the editor, so a scene that was edited and
    // then switched away from was invisible here — and closing the window
    // discarded it without a word, which is the exact failure this store
    // exists to prevent.
    $sceneTabs.forEach((tab, i) => {
      if (i !== $activeSceneTab && tab.dirty) {
        out.push(sceneName(tab.path));
      }
    });
    for (const tab of $scriptTabs) {
      if (tab.dirty) out.push(`Script: ${tab.path.split(/[\\/]/).pop()}`);
    }
    const simple: [boolean, string][] = [
      [$anim, "Animation"],
      [$behavior, "Behavior graph"],
      [$dataSchema, "Data schema"],
      [$dataTable, "Data table"],
      [$font, "Font"],
      [$gui, "GUI document"],
      [$particle, "Particle effect"],
      [$prefab, "Prefab"],
      [$sortLayers, "Sort layers"],
      [$stateMachine, "State machine"],
      [$tilemap, "Tilemap"],
      [$vectorPath, "Vector path"],
    ];
    for (const [isDirty, label] of simple) {
      if (isDirty) out.push(label);
    }
    return out;
  },
);

/**
 * Save everything [`unsavedWork`] lists.
 *
 * The dialog's Save button has to mean what it says: offering "Save" and then
 * writing only the document that happens to be in front would lose the rest
 * while telling the user it had saved. So this walks the same sources
 * `unsavedWork` reads.
 *
 * Scene and script tabs are addressed individually — a parked tab carries its
 * own content, so saving it needs no switching. The single-document editors
 * have only a `saveCurrent*`, which is enough because each can hold one
 * document at a time.
 *
 * Returns what could **not** be saved, empty when everything reached disk.
 * A caller closing something must check it: closing after a failed save loses
 * exactly the work the user asked to keep.
 */
export async function saveAllUnsaved(
  askScenePath?: () => Promise<string | null>,
): Promise<string[]> {
  const failed: string[] = [];

  const tabs = get(sceneTabs);
  for (let i = 0; i < tabs.length; i++) {
    const isActive = get(activeSceneTab) === i;
    const isDirty = isActive ? get(sceneDirty) : tabs[i].dirty;
    if (!isDirty) continue;

    // A never-saved scene has nowhere to write. The caller supplies the file
    // dialog - a store must not open one - and backing out of it counts as a
    // failure, not a skip, so nothing closes over the top of it.
    if (tabs[i].path === null) {
      const chosen = askScenePath ? await askScenePath() : null;
      if (!chosen || !(await saveSceneTabAs(i, chosen))) {
        failed.push("Scene: untitled (needs a location)");
      }
      continue;
    }

    if (!(await saveSceneTab(i))) {
      failed.push(`Scene: ${tabs[i].path?.split(/[\\/]/).pop() ?? "untitled"}`);
    }
  }
  // A scene open before tabs existed, or one never adopted into a tab.
  if (get(sceneDirty) && tabs.length === 0) {
    failed.push("Scene: (not in a tab - save it from File > Save)");
  }

  for (const tab of get(scriptTabs)) {
    if (!tab.dirty) continue;
    if (!(await saveScriptTab(tab.path))) {
      failed.push(`Script: ${tab.path.split(/[\\/]/).pop()}`);
    }
  }

  // The single-document editors. Their `saveCurrent*` reports its own
  // failures to the console and leaves the dirty flag set, so a flag still
  // standing afterwards is the signal that it did not land.
  //
  // The state machine and vector path editors have no entry point here: they
  // apply to the entity from inside their own components rather than writing
  // a file, so "Save" cannot reach them and says so instead of pretending.
  const single: [() => boolean, () => unknown, string][] = [
    [() => get(animDirty), saveCurrentAnim, "Animation"],
    [() => get(behaviorDirty), saveBehavior, "Behavior graph"],
    [() => get(dataSchemaDirty), saveCurrentDataSchema, "Data schema"],
    [() => get(dataTableDirty), saveCurrentDataTable, "Data table"],
    [() => get(fontDirty), saveCurrentFont, "Font"],
    [() => get(guiDirty), saveCurrentGui, "GUI document"],
    [() => get(particleDirty), saveCurrentParticle, "Particle effect"],
    [() => get(prefabDirty), saveCurrentPrefab, "Prefab"],
    [() => get(sortLayersDirty), saveCurrentSortLayers, "Sort layers"],
    [() => get(stateMachineDirty), null as never, "State machine"],
    [() => get(tilemapDirty), saveCurrentTilemap, "Tilemap"],
    [() => get(vectorPathDirty), null as never, "Vector path"],
  ];
  for (const [isDirty, save, label] of single) {
    if (!isDirty()) continue;
    if (typeof save !== "function") {
      // No save entry point exists for this editor; say so rather than
      // implying it was written.
      failed.push(`${label} (no save available - save it from its own panel)`);
      continue;
    }
    await save();
    if (isDirty()) failed.push(label);
  }

  return failed;
}
