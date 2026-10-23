import { describe, it, expect, beforeEach, vi } from "vitest";

// The animation store writes through `api/animation`, which talks to Tauri.
// Faking the write lets the save path run for real up to the disk.
let saveShouldFail = false;
vi.mock("../api/animation", () => ({
  readAnimFile: async () => ({ clips: {} }),
  saveAnimFile: async () => {
    if (saveShouldFail) throw new Error("disk is full");
  },
  defaultMultiClipAnim: () => ({ clips: {} }),
}));

import { get } from "svelte/store";
import {
  EDITOR_CLOSERS,
  EDITOR_KINDS,
  canSaveEditor,
  editorLabel,
  isEditorDirty,
  pendingEditorClose,
  requestEditorClose,
  resolveEditorClose,
  type EditorKind,
} from "./editorClose";
import { unsavedWork } from "./unsavedStore";
// Vite hands these over as plain strings, so the source scan below needs no
// filesystem access and no node type declarations.
import AppSource from "../../App.svelte?raw";
import AnimationEditorSource from "../AnimationEditor.svelte?raw";
import BehaviorEditorSource from "../BehaviorEditor.svelte?raw";
import FontEditorSource from "../FontEditor.svelte?raw";
import GuiEditorSource from "../GuiEditor.svelte?raw";
import ParticleEditorSource from "../ParticleEditor.svelte?raw";
import PrefabEditorSource from "../PrefabEditor.svelte?raw";
import SortLayersEditorSource from "../SortLayersEditor.svelte?raw";
import StateMachineEditorSource from "../StateMachineEditor.svelte?raw";
import TilemapEditorSource from "../TilemapEditor.svelte?raw";
import VectorPathEditorSource from "../VectorPathEditor.svelte?raw";
import { animData, animDirty, animFilePath, selectedClipName } from "./animationEditorStore";
import { behaviorDirty } from "./behaviorEditorStore";
import { dataSchemaDirty } from "./dataSchemaEditorStore";
import { dataTableDirty } from "./dataTableEditorStore";
import { fontDirty } from "./fontEditorStore";
import { guiDirty, guiDocument } from "./guiEditorStore";
import { particleDirty } from "./particleEditorStore";
import { prefabDirty, prefabData } from "./prefabEditorStore";
import { sortLayersDirty } from "./sortLayersEditorStore";
import { stateMachineDirty, stateMachineData } from "./stateMachineEditorStore";
import { tilemapDirty } from "./tilemapEditorStore";
import { vectorPathDirty } from "./vectorPathEditorStore";

/**
 * Every asset editor used to close by calling its store's `close*Editor()`
 * straight from the button - which resets the dirty flag and drops the edits,
 * while the same tab was showing the unsaved dot (BUGS #70). Twelve editors,
 * two close buttons each, and not one of them asked.
 *
 * The failure is silent by construction: the work is simply gone, and the
 * editor looks exactly as it would have if it had been saved. So the tests
 * that matter are the ones asserting a close **did not** happen.
 */

beforeEach(() => {
  saveShouldFail = false;
  pendingEditorClose.set(null);
  for (const kind of EDITOR_KINDS) {
    // Each store's dirty flag is a plain writable; clear them all so one
    // test's leftovers cannot answer another's question.
    EDITOR_CLOSERS[kind].close();
  }
  for (const kind of EDITOR_KINDS) dirtyFlagFor(kind).set(false);
});

describe("requestEditorClose", () => {
  it("closes a clean editor without asking", () => {
    guiDocument.set({ name: "hud", root: {} } as never);
    guiDirty.set(false);

    requestEditorClose("gui");

    expect(get(pendingEditorClose)).toBeNull();
    expect(get(guiDocument)).toBeNull();
  });

  it("does not close a dirty editor - it asks", () => {
    prefabData.set({ name: "Enemy" } as never);
    prefabDirty.set(true);

    requestEditorClose("prefab");

    expect(get(pendingEditorClose)).toBe("prefab");
    expect(get(prefabData)).not.toBeNull();
  });

  it("asks for every editor that can hold unsaved work", () => {
    // The point of the registry: one editor missing from it is one editor
    // that still discards silently, and nothing else would notice.
    for (const kind of EDITOR_KINDS) {
      pendingEditorClose.set(null);
      const closer = EDITOR_CLOSERS[kind];
      const flag = dirtyFlagFor(kind);
      flag.set(true);

      requestEditorClose(kind);

      expect(get(pendingEditorClose), `${closer.label} closed without asking`).toBe(kind);
      flag.set(false);
    }
  });
});

describe("resolveEditorClose", () => {
  it("discards, closing the editor and dropping the edits", async () => {
    guiDocument.set({ name: "hud", root: {} } as never);
    guiDirty.set(true);
    requestEditorClose("gui");

    await expect(resolveEditorClose("discard")).resolves.toBe(true);
    expect(get(guiDocument)).toBeNull();
    expect(get(pendingEditorClose)).toBeNull();
  });

  it("cancels, leaving the editor exactly as it was", async () => {
    prefabData.set({ name: "Enemy" } as never);
    prefabDirty.set(true);
    requestEditorClose("prefab");

    await expect(resolveEditorClose("cancel")).resolves.toBe(false);
    expect(get(prefabData)).not.toBeNull();
    expect(get(prefabDirty)).toBe(true);
    expect(get(pendingEditorClose)).toBeNull();
  });

  it("saves, then closes", async () => {
    animFilePath.set("/p/assets/animations/player.anim");
    animData.set({ clips: { idle: { name: "idle", frames: [], frame_duration: 0.1, looping: true } } });
    selectedClipName.set("idle");
    animDirty.set(true);
    requestEditorClose("animation");

    await expect(resolveEditorClose("save")).resolves.toBe(true);
    expect(get(animData)).toBeNull();
  });

  it("does NOT close when the save fails", async () => {
    // Closing after a failed save loses exactly the work the user asked to
    // keep - the worst outcome of the three, and the easiest to write by
    // accident.
    saveShouldFail = true;
    animFilePath.set("/p/assets/animations/player.anim");
    animData.set({ clips: { idle: { name: "idle", frames: [], frame_duration: 0.1, looping: true } } });
    selectedClipName.set("idle");
    animDirty.set(true);
    requestEditorClose("animation");

    await expect(resolveEditorClose("save")).resolves.toBe(false);
    expect(get(animData)).not.toBeNull();
    expect(get(animDirty)).toBe(true);
  });

  it("does not close an editor it cannot save", async () => {
    // The state machine applies to the entity from inside its own component,
    // so there is nothing to call. The prompt hides Save; if one is asked for
    // anyway, refusing beats closing.
    stateMachineData.set({ states: [], transitions: [] } as never);
    stateMachineDirty.set(true);
    requestEditorClose("statemachine");

    await expect(resolveEditorClose("save")).resolves.toBe(false);
    expect(get(stateMachineData)).not.toBeNull();
  });
});

describe("the registry and the unsaved-work list", () => {
  it("covers exactly the same editors", () => {
    // Two lists of the same twelve editors, written out separately: the one
    // the window-close prompt reads and the one the tab-close prompt reads.
    // They drift the moment someone adds a thirteenth editor to one of them.
    for (const kind of EDITOR_KINDS) {
      dirtyFlagFor(kind).set(true);
    }
    const listed = get(unsavedWork);
    for (const kind of EDITOR_KINDS) {
      expect(listed, `${editorLabel(kind)} is missing from unsavedWork`).toContain(
        editorLabel(kind),
      );
    }
    // And nothing in that list is an editor the close guard has never heard
    // of - scenes and scripts are prefixed, everything else is an editor.
    const labels = EDITOR_KINDS.map(editorLabel);
    for (const item of listed) {
      if (item.startsWith("Scene:") || item.startsWith("Script:")) continue;
      expect(labels, `${item} has no entry in EDITOR_CLOSERS`).toContain(item);
    }
    for (const kind of EDITOR_KINDS) {
      dirtyFlagFor(kind).set(false);
    }
  });

  it("knows which editors can be saved from the prompt", () => {
    expect(canSaveEditor("animation")).toBe(true);
    expect(canSaveEditor("behavior")).toBe(true);
    // These two apply to the entity instead of writing a file.
    expect(canSaveEditor("statemachine")).toBe(false);
    expect(canSaveEditor("vectorpath")).toBe(false);
  });

  it("reports dirtiness straight from each editor's own flag", () => {
    expect(isEditorDirty("gui")).toBe(false);
    guiDirty.set(true);
    expect(isEditorDirty("gui")).toBe(true);
    guiDirty.set(false);
  });
});

describe("the close buttons", () => {
  /**
   * Nothing above can catch a button wired straight to `close*Editor()`.
   *
   * That is exactly how this bug existed: the guard is only a guard if every
   * button goes through it, and a new editor's ✕ is one line someone writes
   * without ever reading this file. Both close buttons - the tab's ✕ in
   * `App.svelte` and the editor's own - live in markup, so the only check is
   * on the source.
   */
  const CLOSERS = [
    "closeAnimEditor",
    "closeBehaviorEditor",
    "closeDataSchemaEditor",
    "closeDataTableEditor",
    "closeFontEditor",
    "closeGuiEditor",
    "closeParticleEditor",
    "closePrefabEditor",
    "closeSortLayersEditor",
    "closeStateMachineEditor",
    "closeTilemapEditor",
    "closeVectorPathEditor",
  ];

  const COMPONENTS: [string, string][] = [
    ["AnimationEditor.svelte", AnimationEditorSource],
    ["BehaviorEditor.svelte", BehaviorEditorSource],
    ["FontEditor.svelte", FontEditorSource],
    ["GuiEditor.svelte", GuiEditorSource],
    ["ParticleEditor.svelte", ParticleEditorSource],
    ["PrefabEditor.svelte", PrefabEditorSource],
    ["SortLayersEditor.svelte", SortLayersEditorSource],
    ["StateMachineEditor.svelte", StateMachineEditorSource],
    ["TilemapEditor.svelte", TilemapEditorSource],
    ["VectorPathEditor.svelte", VectorPathEditorSource],
  ];

  it("never call an editor's close directly", () => {
    for (const [name, src] of COMPONENTS) {
      for (const fn of CLOSERS) {
        expect(
          src.includes(`onclick={${fn}}`),
          `${name} closes through ${fn} instead of requestEditorClose, so it ` +
            "discards unsaved work without asking",
        ).toBe(false);
      }
    }
  });

  it("are all routed through the guard", () => {
    const app = AppSource;

    // The bulk reset on project change is the one deliberate direct close -
    // it runs when the project is swapped out from under every editor - so
    // the check is that each closer appears *only* there.
    for (const fn of CLOSERS) {
      const direct = app.split(`${fn}();`).length - 1;
      expect(
        direct,
        `${fn} is called directly ${direct} times in App.svelte; only the ` +
          "project-change reset may do that - a tab's ✕ must ask first",
      ).toBeLessThanOrEqual(1);
    }
    // Every editor must be closable *somewhere*, and every such button must
    // ask. The GUI editor has no tab ✕ - it closes only from inside its own
    // panel - so the search covers the components too.
    const sources = [app, ...COMPONENTS.map(([, src]) => src)];
    for (const kind of EDITOR_KINDS) {
      expect(
        sources.some((src) => src.includes(`requestEditorClose("${kind}")`)),
        `nothing asks before closing the ${editorLabel(kind)} editor`,
      ).toBe(true);
    }
  });
});

/** The writable behind each editor's dirty flag, for driving the tests. */
function dirtyFlagFor(kind: EditorKind) {
  const flags = {
    animation: animDirty,
    behavior: behaviorDirty,
    dataschema: dataSchemaDirty,
    datatable: dataTableDirty,
    font: fontDirty,
    gui: guiDirty,
    particle: particleDirty,
    prefab: prefabDirty,
    sortlayers: sortLayersDirty,
    statemachine: stateMachineDirty,
    tilemap: tilemapDirty,
    vectorpath: vectorPathDirty,
  } as const;
  return flags[kind];
}
