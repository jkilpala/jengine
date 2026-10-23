<script lang="ts">
  import { undoTarget } from "./lib/stores/undoTargetStore";
  import { untrack } from "svelte";
  import Toolbar from "./lib/Toolbar.svelte";
  import SceneTree from "./lib/SceneTree.svelte";
  import AssetBrowser from "./lib/AssetBrowser.svelte";
  import Inspector from "./lib/Inspector.svelte";
  import Viewport from "./lib/Viewport.svelte";
  import Console from "./lib/Console.svelte";
  import BuildOutput from "./lib/BuildOutput.svelte";
  import ServersPanel from "./lib/ServersPanel.svelte";
  import TimelinePanel from "./lib/TimelinePanel.svelte";
  import LocalisationPanel from "./lib/LocalisationPanel.svelte";
  import MixerPanel from "./lib/MixerPanel.svelte";
  import DebuggerPanel from "./lib/DebuggerPanel.svelte";
  import ProfilerPanel from "./lib/ProfilerPanel.svelte";
  // Installs the Lua debugger before any script compiles, and its keys.
  import "./lib/stores/debuggerStore";
  import SheetSlicer from "./lib/SheetSlicer.svelte";
  import MaterialEditor from "./lib/MaterialEditor.svelte";
  import SkeletalEditor from "./lib/SkeletalEditor.svelte";
  import SkinEditor from "./lib/SkinEditor.svelte";
  import MediaPreview from "./lib/MediaPreview.svelte";
  import { buildAndRun, closeInstances, instanceCount } from "./lib/stores/runStore";
  import { appendServerLine } from "./lib/stores/serverStore";
  import {
    requestSearchFocus,
    requestReplaceFocus,
  } from "./lib/stores/searchStore";
  import { unsavedWork, saveAllUnsaved } from "./lib/stores/unsavedStore";
  import {
    pendingEditorClose,
    requestEditorClose,
    resolveEditorClose,
    editorLabel,
    canSaveEditor,
  } from "./lib/stores/editorClose";
  import UnsavedDialog, {
    type UnsavedChoice,
  } from "./lib/UnsavedDialog.svelte";
  import { openEntryScene } from "./lib/api/openAsset";
  import { isTextEditingTarget } from "./lib/api/textTarget";
  import {
    installPlayKeyRouter,
    isPauseKey,
    isStepKey,
    isStopKey,
  } from "./lib/api/playKeys";
  import { isPlaying, isPaused } from "./lib/stores/playStore";
  import { viewMode } from "./lib/stores/viewportStore";
  import { promptForScenePath } from "./lib/api/scenePathPrompt";
  import { requestAssetRefresh } from "./lib/stores/projectStore";
  import {
    adoptCurrentAsTab,
    clearSceneTabs,
    sceneTabs,
    activeSceneTab,
    activateSceneTab,
    closeSceneTab,
    isSceneTabDirty,
    saveSceneTab,
    saveSceneTabAs,
    sceneTabName,
  } from "./lib/stores/sceneTabsStore";
  import CommandPalette from "./lib/CommandPalette.svelte";
  import { registerActions } from "./lib/stores/actionStore";
  import { checkToolsAfterBuildFailure } from "./lib/api/buildTools";
  import { canUndo, canRedo } from "./lib/stores/commandStore";
  import { guardWindowClose } from "./lib/api/editorWindow";
  import FindInFiles from "./lib/FindInFiles.svelte";
  import QuickOpen from "./lib/QuickOpen.svelte";
  import { listen } from "@tauri-apps/api/event";
  import { appendBuildLine, beginBuild, endBuild } from "./lib/stores/buildStore";
  import DockZone from "./lib/DockZone.svelte";
  import ProjectSettings from "./lib/ProjectSettings.svelte";
  import ManageComponents from "./lib/ManageComponents.svelte";
  import BuildSettings from "./lib/BuildSettings.svelte";
  import SpritesheetEditor from "./lib/SpritesheetEditor.svelte";
  import AtlasEditor from "./lib/AtlasEditor.svelte";
  import StartScreen from "./lib/StartScreen.svelte";
  import Toasts from "./lib/Toasts.svelte";
  import PackageManager from "./lib/PackageManager.svelte";
  import NewRustPackage from "./lib/NewRustPackage.svelte";
  import PackagePanel from "./lib/PackagePanel.svelte";
  import GuiEditor from "./lib/GuiEditor.svelte";
  import ImagePreview from "./lib/ImagePreview.svelte";
  import ShaderEditor from "./lib/ShaderEditor.svelte";
  import PrefabEditor from "./lib/PrefabEditor.svelte";
  import ParticleEditor from "./lib/ParticleEditor.svelte";
  import AnimationEditor from "./lib/AnimationEditor.svelte";
  import StateMachineEditor from "./lib/StateMachineEditor.svelte";
  import VectorPathEditor from "./lib/VectorPathEditor.svelte";
  import FontEditor from "./lib/FontEditor.svelte";
  import TilemapEditor from "./lib/TilemapEditor.svelte";
  import DataSchemaEditor from "./lib/DataSchemaEditor.svelte";
  import DataTableEditor from "./lib/DataTableEditor.svelte";
  import ScriptEditor from "./lib/ScriptEditor.svelte";
  import BehaviorEditor from "./lib/BehaviorEditor.svelte";
  import SortLayersEditor from "./lib/SortLayersEditor.svelte";
  import AudioPreview from "./lib/AudioPreview.svelte";
  import KeyboardShortcuts from "./lib/KeyboardShortcuts.svelte";
  import AboutDialog from "./lib/AboutDialog.svelte";
  import ThirdPartyLicenses from "./lib/ThirdPartyLicenses.svelte";
  import UndoHistory from "./lib/UndoHistory.svelte";
  import SceneSwitcher from "./lib/SceneSwitcher.svelte";
  import type { ShaderGraph } from "./lib/api/shaderEditor";
  import { shaderLoad, shaderCreateNew } from "./lib/api/shaderEditor";
  import { toggleDevtools, reloadEditorWindow } from "./lib/api/editorWindow";
  import { logError } from "./lib/stores/consoleStore";
  import {
    viewportPackageStatus,
    rebuildProjectViewport,
    switchProjectViewport,
    type ViewportStatus,
  } from "./lib/api/build";
  import { clearReopen, markReopen, reloadIntoProject } from "./lib/viewportReload";
  import { scene, dirty, scenePath } from "./lib/stores/sceneStore";
  import { project } from "./lib/stores/projectStore";
  import { undo, redo } from "./lib/stores/commandStore";
  import {
    selectedEntity,
    selectEntity,
    selectedEntities,
  } from "./lib/stores/selectionStore";
  import { entityClipboard } from "./lib/stores/clipboardStore";
  import { pushCommand } from "./lib/stores/commandStore";
  import { get } from "svelte/store";
  import {
    layout,
    setZoneSize,
    isPanelDragging,
    packagePanels,
    registerPackagePanel,
    focusPanel,
  } from "./lib/stores/layoutStore";
  import {
    guiDocument,
    guiDirty,
    openGuiFile,
    closeGuiEditor,
    saveCurrentGui,
  } from "./lib/stores/guiEditorStore";
  import {
    prefabData,
    prefabDirty,
    openPrefabFile,
    closePrefabEditor,
    saveCurrentPrefab,
    undoPrefab,
    redoPrefab,
  } from "./lib/stores/prefabEditorStore";
  import {
    particleEmitter,
    particleDirty,
    openParticleFile,
    closeParticleEditor,
    saveCurrentParticle,
  } from "./lib/stores/particleEditorStore";
  import {
    animData,
    animClip,
    animDirty,
    selectedClipName,
    openAnimFile,
    closeAnimEditor,
    saveCurrentAnim,
  } from "./lib/stores/animationEditorStore";
  import {
    stateMachineData,
    stateMachineDirty,
    openStateMachineEditor,
    closeStateMachineEditor,
  } from "./lib/stores/stateMachineEditorStore";
  import {
    vectorPathData,
    vectorPathDirty,
    openVectorPathEditor,
    closeVectorPathEditor,
  } from "./lib/stores/vectorPathEditorStore";
  import {
    fontData,
    fontDirty,
    openFontFile,
    closeFontEditor,
    saveCurrentFont,
  } from "./lib/stores/fontEditorStore";
  import {
    tilemapData,
    tilemapDirty,
    openTilemapFile,
    openEntityTilemap,
    closeTilemapEditor,
    saveCurrentTilemap,
  } from "./lib/stores/tilemapEditorStore";
  import {
    dataSchema,
    dataSchemaDirty,
    openDataSchemaFile,
    closeDataSchemaEditor,
    saveCurrentDataSchema,
  } from "./lib/stores/dataSchemaEditorStore";
  import {
    dataTable,
    dataTableDirty,
    openDataTableFile,
    closeDataTableEditor,
    saveCurrentDataTable,
  } from "./lib/stores/dataTableEditorStore";
  import {
    scriptTabs,
    activeScriptPath,
    openScriptFile,
    closeScriptTab,
    isScriptTabDirty,
    scriptTabName,
    saveScriptTab,
    closeAllScriptTabs,
    saveCurrentScript,
    requestedScriptLine,
  } from "./lib/stores/scriptEditorStore";
  import {
    behaviorJson,
    behaviorFilePath,
    openBehaviorFile,
    closeBehaviorEditor,
  } from "./lib/stores/behaviorEditorStore";
  import {
    sortLayersData,
    sortLayersFilePath,
    sortLayersDirty,
    openSortLayersFile,
    closeSortLayersEditor,
    saveCurrentSortLayers,
  } from "./lib/stores/sortLayersEditorStore";
  import {
    packageSchema,
    reloadPackageSchema,
  } from "./lib/stores/packageSchemaStore";
  import type { PanelId, ZoneId } from "./lib/stores/layoutStore";

  let showSettings = $state(false);
  let showComponents = $state(false);
  let showBuild = $state(false);
  let showSpritesheet = $state(false);
  let showAtlasEditor = $state(false);
  let atlasEditorPath = $state<string | undefined>(undefined);
  let showPackages = $state(false);
  let showNewRustPackage = $state(false);
  let audioPreviewRef: AudioPreview | undefined = $state();
  let showShortcuts = $state(false);
  let showAbout = $state(false);
  let showLicenses = $state(false);
  let showSceneSwitcher = $state(false);
  let showQuickOpen = $state(false);
  let showCommandPalette = $state(false);
  let centerTab = $state<
    | "viewport"
    | "gui"
    | "image"
    | "shader"
    | "prefab"
    | "particle"
    | "animation"
    | "statemachine"
    | "font"
    | "tilemap"
    | "script"
    | "vectorpath"
    | "behavior"
    | "sortlayers"
    | "dataschema"
    | "datatable"
  >("viewport");

  // The tab strip scrolls sideways once the documents outgrow it, so the tab
  // being activated is not necessarily on screen - activating one from the
  // Scene panel or by opening an asset would otherwise change nothing visible.
  let centerTabsEl = $state<HTMLDivElement | null>(null);

  $effect(() => {
    // Tracked so this re-runs whenever the active document changes, in
    // either dimension. `void` rather than a bare read: the intent is the
    // subscription, and a bare expression statement reads like a mistake.
    void centerTab;
    void $activeSceneTab;
    const strip = centerTabsEl;
    if (!strip) return;
    // After the DOM has the new `.active`, not before.
    requestAnimationFrame(() => {
      strip
        .querySelector(".center-tab.active")
        ?.scrollIntoView({ block: "nearest", inline: "nearest" });
    });
  });

  // Shader editor state
  let shaderGraph = $state<ShaderGraph | null>(null);
  let shaderProjectPath = $state<string | undefined>(undefined);
  let shaderRelativePath = $state<string | undefined>(undefined);

  let leftHasContent = $derived($layout.zones.left.groups.length > 0);
  let rightHasContent = $derived($layout.zones.right.groups.length > 0);
  let bottomHasContent = $derived($layout.zones.bottom.groups.length > 0);

  // Expose handleSave for global Ctrl+S — dispatches a custom event the Toolbar listens for
  function dispatchGlobalSave() {
    if (centerTab === "gui") {
      saveCurrentGui();
    } else if (centerTab === "prefab") {
      saveCurrentPrefab();
    } else if (centerTab === "particle") {
      saveCurrentParticle();
    } else if (centerTab === "animation") {
      saveCurrentAnim();
    } else if (centerTab === "font") {
      saveCurrentFont();
    } else if (centerTab === "tilemap") {
      saveCurrentTilemap();
    } else if (centerTab === "script") {
      saveCurrentScript();
    } else if (centerTab === "sortlayers") {
      saveCurrentSortLayers();
    } else if (centerTab === "dataschema") {
      saveCurrentDataSchema();
    } else if (centerTab === "datatable") {
      saveCurrentDataTable();
    } else {
      window.dispatchEvent(new CustomEvent("editor-save"));
    }
  }

  // Listen for GUI editor open requests from AssetBrowser
  async function handleOpenGuiEditor(e: Event) {
    const detail = (e as CustomEvent).detail;
    if (detail?.projectPath && detail?.relativePath) {
      const fullPath = detail.projectPath + "/" + detail.relativePath;
      await openGuiFile(fullPath);
      centerTab = "gui";
    }
  }

  // Auto-switch tab when GUI editor closes
  $effect(() => {
    if (!$guiDocument && centerTab === "gui") {
      centerTab = "viewport";
    }
  });

  // Menu items for things whose state lives here, not in the Toolbar. Each
  // runs the *same* code as its shortcut, so a menu item and its key label
  // cannot drift apart.
  $effect(() => {
    const handlers: Record<string, () => void> = {
      "editor-quick-open": () => (showQuickOpen = true),
      "editor-command-palette": () => (showCommandPalette = true),
      "editor-find-in-files": () => {
        focusPanel("search");
        requestSearchFocus();
      },
      "editor-replace-in-files": () => {
        focusPanel("search");
        requestReplaceFocus();
      },
    };
    for (const [name, fn] of Object.entries(handlers)) {
      window.addEventListener(name, fn);
    }
    return () => {
      for (const [name, fn] of Object.entries(handlers)) {
        window.removeEventListener(name, fn);
      }
    };
  });

  // File > Open Scene - the same dialog Ctrl+O opens. The menu used to raise
  // a native file picker instead, so one label named two different dialogs
  // and neither could honestly claim the shortcut.
  $effect(() => {
    const handler = () => (showSceneSwitcher = true);
    window.addEventListener("editor-open-scene", handler);
    return () => window.removeEventListener("editor-open-scene", handler);
  });

  // Help menu → keyboard shortcut reference (same as F1)
  $effect(() => {
    const handler = () => (showShortcuts = true);
    window.addEventListener("editor-shortcuts", handler);
    return () => window.removeEventListener("editor-shortcuts", handler);
  });

  // Help menu → About jEngine, and the third-party licences.
  $effect(() => {
    const about = () => (showAbout = true);
    const licenses = () => (showLicenses = true);
    window.addEventListener("editor-about", about);
    window.addEventListener("editor-licenses", licenses);
    return () => {
      window.removeEventListener("editor-about", about);
      window.removeEventListener("editor-licenses", licenses);
    };
  });

  // Listen for prefab editor open requests from AssetBrowser
  $effect(() => {
    const handler = async (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.fullPath) {
        await openPrefabFile(detail.fullPath);
        centerTab = "prefab";
      }
    };
    window.addEventListener("open-prefab-editor", handler);
    return () => window.removeEventListener("open-prefab-editor", handler);
  });

  // Auto-switch tab when prefab editor closes
  $effect(() => {
    if (!$prefabData && centerTab === "prefab") {
      centerTab = "viewport";
    }
  });

  // Listen for particle editor open requests from AssetBrowser
  $effect(() => {
    const handler = async (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.fullPath) {
        await openParticleFile(detail.fullPath);
        centerTab = "particle";
      }
    };
    window.addEventListener("open-particle-editor", handler);
    return () => window.removeEventListener("open-particle-editor", handler);
  });

  // Auto-switch tab when particle editor closes
  $effect(() => {
    if (!$particleEmitter && centerTab === "particle") {
      centerTab = "viewport";
    }
  });

  // Listen for animation editor open requests from AssetBrowser
  $effect(() => {
    const handler = async (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.fullPath) {
        await openAnimFile(detail.fullPath);
        centerTab = "animation";
      }
    };
    window.addEventListener("open-animation-editor", handler);
    return () => window.removeEventListener("open-animation-editor", handler);
  });

  // Auto-switch tab when animation editor closes
  $effect(() => {
    if (!$animData && centerTab === "animation") {
      centerTab = "viewport";
    }
  });

  // Listen for state machine editor open requests from Inspector
  $effect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.entityId != null && detail?.data) {
        openStateMachineEditor(detail.entityId, detail.data);
        centerTab = "statemachine";
      }
    };
    window.addEventListener("open-statemachine-editor", handler);
    return () =>
      window.removeEventListener("open-statemachine-editor", handler);
  });

  // Auto-switch tab when state machine editor closes
  $effect(() => {
    if (!$stateMachineData && centerTab === "statemachine") {
      centerTab = "viewport";
    }
  });

  // Listen for vector path editor open requests from Inspector
  $effect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.entityId != null && detail?.data) {
        openVectorPathEditor(detail.entityId, detail.data);
        centerTab = "vectorpath";
      }
    };
    window.addEventListener("open-vectorpath-editor", handler);
    return () => window.removeEventListener("open-vectorpath-editor", handler);
  });

  // Auto-switch tab when vector path editor closes
  $effect(() => {
    if (!$vectorPathData && centerTab === "vectorpath") {
      centerTab = "viewport";
    }
  });

  // Handle state machine apply — write data back to entity
  $effect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.entityId != null && detail?.data && $scene) {
        const idx = detail.entityId as number;
        if (idx >= 0 && idx < $scene.entities.length) {
          const before = structuredClone($scene);
          const after = structuredClone($scene);
          after.entities[idx].components.anim_state_machine = structuredClone(
            detail.data,
          );
          pushCommand("Apply state machine", before, after);
        }
      }
    };
    window.addEventListener("statemachine-apply", handler);
    return () => window.removeEventListener("statemachine-apply", handler);
  });

  // Listen for font editor open requests from AssetBrowser
  $effect(() => {
    const handler = async (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.fullPath) {
        await openFontFile(detail.fullPath);
        centerTab = "font";
      }
    };
    window.addEventListener("open-font-editor", handler);
    return () => window.removeEventListener("open-font-editor", handler);
  });

  // Auto-switch tab when font editor closes
  $effect(() => {
    if (!$fontData && centerTab === "font") {
      centerTab = "viewport";
    }
  });

  // Listen for tilemap editor open requests from AssetBrowser
  $effect(() => {
    const handler = async (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.fullPath) {
        // From an entity, its terrain rule comes along (`autotile`).
        await openTilemapFile(detail.fullPath, detail.entity);
        centerTab = "tilemap";
      } else if (detail?.entity) {
        // An entity's own map, held in the scene.
        openEntityTilemap(detail.entity);
        centerTab = "tilemap";
      }
    };
    window.addEventListener("open-tilemap-editor", handler);
    return () => window.removeEventListener("open-tilemap-editor", handler);
  });

  // Auto-switch tab when tilemap editor closes
  $effect(() => {
    if (!$tilemapData && centerTab === "tilemap") {
      centerTab = "viewport";
    }
  });

  // Closing the window used to discard every unsaved editor silently —
  // fourteen separate `dirty` flags and nothing looking at them together.
  // The answer arrives from the in-app dialog rather than `confirm`, so the
  // user gets Save as an option and the prompt cannot fail to render.
  let closeChoice: ((choice: UnsavedChoice) => void) | null = $state(null);

  /** Script tab waiting on the unsaved-changes dialog, or null. */
  let pendingScriptClose = $state<string | null>(null);

  /**
   * Close a script tab, asking first when it has unsaved edits.
   *
   * Same three-way choice the scene tabs use — a `confirm` could only offer
   * discard or cancel, and *save* is usually what someone means.
   */
  function requestScriptClose(path: string) {
    if (isScriptTabDirty(path)) {
      pendingScriptClose = path;
      return;
    }
    closeScriptTab(path);
  }

  /**
   * Everything open in the centre strip, in the order it is drawn.
   *
   * Ctrl+Tab walks this. It mirrors the markup below rather than generating
   * it - the tabs differ too much in what they show to come from a loop - so
   * a tab added there wants an entry here too. Missing one costs a stop in
   * the cycle, not correctness.
   */
  const openDocuments = $derived.by(() => {
    const docs: { key: string; show: () => void }[] = [];
    if ($sceneTabs.length === 0) {
      docs.push({ key: "viewport", show: () => (centerTab = "viewport") });
    }
    $sceneTabs.forEach((_tab, i) => {
      docs.push({
        key: `scene:${i}`,
        show: () => {
          activateSceneTab(i);
          centerTab = "viewport";
        },
      });
    });
    const single: [unknown, typeof centerTab][] = [
      [$guiDocument, "gui"],
      [previewImage, "image"],
      [shaderGraph, "shader"],
      [$prefabData, "prefab"],
      [$particleEmitter, "particle"],
      [$animData, "animation"],
      [$stateMachineData, "statemachine"],
      [$vectorPathData, "vectorpath"],
      [$fontData, "font"],
      [$tilemapData, "tilemap"],
      [$dataSchema, "dataschema"],
      [$dataTable, "datatable"],
    ];
    for (const [open, tab] of single) {
      if (open) docs.push({ key: tab, show: () => (centerTab = tab) });
    }
    for (const tab of $scriptTabs) {
      docs.push({
        key: `script:${tab.path}`,
        show: () => {
          activeScriptPath.set(tab.path);
          centerTab = "script";
        },
      });
    }
    if ($behaviorJson) docs.push({ key: "behavior", show: () => (centerTab = "behavior") });
    if ($sortLayersData) docs.push({ key: "sortlayers", show: () => (centerTab = "sortlayers") });
    return docs;
  });

  /** Which entry of [`openDocuments`] is in front, or -1. */
  function currentDocumentIndex(): number {
    const key =
      centerTab === "viewport"
        ? $sceneTabs.length === 0
          ? "viewport"
          : `scene:${$activeSceneTab}`
        : centerTab === "script"
          ? `script:${$activeScriptPath}`
          : centerTab;
    return openDocuments.findIndex((d) => d.key === key);
  }

  /** Step `delta` documents along the strip, wrapping at both ends. */
  function cycleDocument(delta: number) {
    const docs = openDocuments;
    if (docs.length < 2) return;
    const at = currentDocumentIndex();
    const next = (((at < 0 ? 0 : at + delta) % docs.length) + docs.length) % docs.length;
    docs[next].show();
  }

  /** Index of the scene tab whose close is waiting on the dialog, or null. */
  let pendingSceneClose = $state<number | null>(null);

  function requestSceneClose(index: number) {
    // The dot and the prompt must agree: if one says there is unsaved work,
    // the other cannot quietly close without asking.
    if (isSceneTabDirty(index)) {
      pendingSceneClose = index;
      return;
    }
    closeSceneTab(index);
  }

  async function resolveSceneClose(choice: UnsavedChoice) {
    const index = pendingSceneClose;
    pendingSceneClose = null;
    if (index === null || choice === "cancel") return;

    if (choice === "save") {
      // A never-saved scene has nowhere to write yet, so Save has to ask
      // where. Backing out of that dialog cancels the close too - the scene
      // is still unsaved, and closing it now would throw away exactly what
      // the user just said they wanted to keep.
      const saved =
        get(sceneTabs)[index]?.path === null
          ? await saveSceneTabAsWithPrompt(index)
          : await saveSceneTab(index);
      if (!saved) return;
    }
    closeSceneTab(index);
  }

  /** Ask where an untitled scene should live, then write it there. */
  async function saveSceneTabAsWithPrompt(index: number): Promise<boolean> {
    const chosen = await promptForScenePath();
    if (!chosen) return false;
    const ok = await saveSceneTabAs(index, chosen);
    if (ok) requestAssetRefresh();
    return ok;
  }

  /**
   * The active scene's dirty flag lives in the live store, not its snapshot -
   * a snapshot is only refreshed when the tab is parked, so reading it for the
   * active tab shows a stale dot.
   */
  function sceneTabDirty(index: number): boolean {
    return index === $activeSceneTab ? $dirty : ($sceneTabs[index]?.dirty ?? false);
  }

  // The script panel has its own close button; it asks through here so there
  // is one prompt rather than one per button.
  $effect(() => {
    const handler = () => {
      const path = $activeScriptPath;
      if (path) requestScriptClose(path);
    };
    window.addEventListener("request-close-script", handler);
    return () => window.removeEventListener("request-close-script", handler);
  });

  async function resolveScriptClose(choice: UnsavedChoice) {
    const path = pendingScriptClose;
    pendingScriptClose = null;
    if (!path || choice === "cancel") return;
    if (choice === "save" && !(await saveScriptTab(path))) {
      // The write failed; keep the tab rather than lose what it holds.
      return;
    }
    closeScriptTab(path);
  }

  $effect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    guardWindowClose(async () => {
      if ($unsavedWork.length === 0) return true;
      const choice = await new Promise<UnsavedChoice>((resolve) => {
        closeChoice = resolve;
      });
      closeChoice = null;
      if (choice === "cancel") return false;
      if (choice === "save") {
        // Every item the dialog listed, not just the active editor. This used
        // to call `dispatchGlobalSave()`, which is Ctrl+S: it saved whatever
        // happened to be in front and the window closed over the rest. The
        // dialog names them all, so Save has to mean all of them.
        const failed = await saveAllUnsaved(promptForScenePath);
        if (failed.length > 0) {
          // Closing now would discard exactly what Save was asked to keep.
          logError(`Not saved, so the editor stayed open: ${failed.join(", ")}`);
          return false;
        }
      }
      return true;
    })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch((e) => logError(`Close guard: ${e}`));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  });

  // Scenes can arrive without going through `openAsset` — the start screen's
  // entry-scene load, File > New Scene. Watching the path keeps the strip
  // honest whatever route was taken, instead of each of those call sites
  // having to remember to register a tab.
  $effect(() => {
    const path = $scenePath;
    void path;
    if ($project) adoptCurrentAsTab();
  });

  $effect(() => {
    if (!$project) clearSceneTabs();
  });

  // Everything the palette can run. Registered in one place so the list is
  // reviewable — a command that exists only as a shortcut is invisible to
  // anyone who does not already know the shortcut, which is the whole reason
  // a palette earns its keep.
  $effect(() => {
    return registerActions([
      {
        id: "file.save",
        title: "Save",
        category: "File",
        keys: "Ctrl+S",
        run: dispatchGlobalSave,
      },
      {
        id: "file.new-rust-package",
        title: "New Rust Package\u2026",
        category: "File",
        run: () => (showNewRustPackage = true),
      },
      {
        id: "file.quick-open",
        title: "Go to File\u2026",
        category: "File",
        keys: "Ctrl+P",
        run: () => (showQuickOpen = true),
      },
      {
        id: "file.entry-scene",
        title: "Open Entry Scene",
        category: "File",
        keys: "Ctrl+Home",
        run: () => {
          if ($project) void openEntryScene($project);
        },
      },
      {
        id: "file.open-scene",
        title: "Open Scene\u2026",
        category: "File",
        keys: "Ctrl+O",
        run: () => (showSceneSwitcher = true),
      },
      {
        id: "edit.undo",
        title: "Undo",
        category: "Edit",
        keys: "Ctrl+Z",
        enabled: () => $canUndo,
        run: undo,
      },
      {
        id: "edit.redo",
        title: "Redo",
        category: "Edit",
        keys: "Ctrl+Shift+Z",
        enabled: () => $canRedo,
        run: redo,
      },
      {
        id: "edit.duplicate",
        title: "Duplicate Selection",
        category: "Edit",
        keys: "Ctrl+D",
        run: () => window.dispatchEvent(new CustomEvent("editor-duplicate")),
      },
      {
        id: "search.find-in-files",
        title: "Find in Files",
        category: "Search",
        keys: "Ctrl+Shift+F",
        run: () => {
          focusPanel("search");
          requestSearchFocus();
        },
      },
      {
        id: "search.replace-in-files",
        title: "Replace in Files",
        category: "Search",
        keys: "Ctrl+Shift+H",
        run: () => {
          focusPanel("search");
          requestReplaceFocus();
        },
      },
      {
        id: "play.pause",
        title: "Pause / Resume",
        category: "Play",
        keys: "Ctrl+Shift+P",
        run: () => window.dispatchEvent(new CustomEvent("editor-pause")),
      },
      {
        id: "play.stop",
        title: "Stop Play Mode",
        category: "Play",
        keys: "Shift+F5",
        run: () => window.dispatchEvent(new CustomEvent("editor-stop")),
      },
      {
        id: "play.step",
        title: "Step One Frame",
        category: "Play",
        keys: "Ctrl+Alt+P",
        run: () => window.dispatchEvent(new CustomEvent("editor-step")),
      },
      {
        id: "view.console",
        title: "Show Console",
        category: "View",
        run: () => focusPanel("console"),
      },
      {
        id: "build.run",
        title: "Build & Run (native, all launched instances)",
        category: "Build",
        keys: "Ctrl+B",
        run: () => void buildAndRun(false),
      },
      {
        id: "build.run-scene",
        title: "Build & Run Current Scene",
        category: "Build",
        keys: "Ctrl+Shift+B",
        run: () => void buildAndRun(true),
      },
      {
        id: "build.close-instances",
        title: "Close Running Instances",
        category: "Build",
        keys: "Ctrl+J",
        run: () => void closeInstances(),
      },
      ...[1, 2, 3, 4].map((n) => ({
        id: `build.instances-${n}`,
        title: `Launched Instance Count: ${n}`,
        category: "Build",
        run: () => instanceCount.set(n),
      })),
      {
        id: "view.servers",
        title: "Show Servers (run, deploy and watch test servers)",
        category: "View",
        run: () => focusPanel("servers"),
      },
      {
        id: "view.profiler",
        title: "Show Profiler (record a capture: time by system, script and render pass)",
        category: "View",
        run: () => focusPanel("profiler"),
      },
      {
        id: "view.debugger",
        title: "Show Debugger (breakpoints, call stack and locals of Play's Lua scripts)",
        category: "View",
        run: () => focusPanel("debugger"),
      },
      {
        id: "view.mixer",
        title: "Show Mixer (the project's audio buses and their effects)",
        category: "View",
        run: () => focusPanel("mixer"),
      },
      {
        id: "view.localisation",
        title: "Show Localisation (preview a language, find untranslated text)",
        category: "View",
        run: () => focusPanel("localisation"),
      },
      {
        id: "view.timeline",
        title: "Show Timeline (the selected entity's timeline clips as a dope sheet)",
        category: "View",
        run: () => focusPanel("timeline"),
      },
      {
        id: "view.build",
        title: "Show Build Output",
        category: "View",
        run: () => focusPanel("build"),
      },
      {
        id: "view.search",
        title: "Show Search",
        category: "View",
        run: () => focusPanel("search"),
      },
      {
        id: "view.shortcuts",
        title: "Keyboard Shortcuts",
        category: "View",
        keys: "F1",
        run: () => (showShortcuts = true),
      },
      {
        id: "help.about",
        title: "About jEngine",
        category: "Help",
        run: () => (showAbout = true),
      },
      {
        id: "help.licenses",
        title: "Third-Party Licenses",
        category: "Help",
        run: () => (showLicenses = true),
      },
      {
        id: "project.settings",
        title: "Project Settings\u2026",
        category: "Project",
        run: () => (showSettings = true),
      },
      {
        id: "project.build",
        title: "Build Settings\u2026",
        category: "Project",
        run: () => (showBuild = true),
      },
      {
        id: "project.components",
        title: "Manage Components\u2026",
        category: "Project",
        run: () => (showComponents = true),
      },
      {
        id: "project.rebuild-viewport",
        title: "Rebuild Project Viewport",
        category: "Project",
        run: rebuildViewport,
      },
      {
        id: "view.devtools",
        title: "Toggle Devtools",
        category: "View",
        keys: "F12",
        run: () =>
          void toggleDevtools().catch((err) => logError(`Devtools: ${err}`)),
      },
      {
        id: "view.reload",
        title: "Reload Editor Window",
        category: "View",
        keys: "Ctrl+R",
        run: reloadEditorWindow,
      },
    ]);
  });

  // Listen for script editor open requests from AssetBrowser
  $effect(() => {
    const handler = async (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.fullPath) {
        await openScriptFile(detail.fullPath);
        if (detail.line) requestedScriptLine.set(detail.line as number);
        centerTab = "script";
      }
    };
    window.addEventListener("open-script-editor", handler);
    return () => window.removeEventListener("open-script-editor", handler);
  });

  // Auto-switch tab when all script tabs close
  $effect(() => {
    if ($scriptTabs.length === 0 && centerTab === "script") {
      centerTab = "viewport";
    }
  });

  // Listen for behavior editor open requests from AssetBrowser
  $effect(() => {
    const handler = async (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.fullPath) {
        await openBehaviorFile(detail.fullPath);
        centerTab = "behavior";
      }
    };
    window.addEventListener("open-behavior-editor", handler);
    return () => window.removeEventListener("open-behavior-editor", handler);
  });

  // Auto-switch tab when behavior editor closes
  $effect(() => {
    if (!$behaviorJson && centerTab === "behavior") {
      centerTab = "viewport";
    }
  });

  // Listen for sort layers editor open requests from AssetBrowser
  $effect(() => {
    const handler = async (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.fullPath) {
        await openSortLayersFile(detail.fullPath);
        centerTab = "sortlayers";
      }
    };
    window.addEventListener("open-sortlayers-editor", handler);
    return () => window.removeEventListener("open-sortlayers-editor", handler);
  });

  // Auto-switch tab when sort layers editor closes
  $effect(() => {
    if (!$sortLayersData && centerTab === "sortlayers") {
      centerTab = "viewport";
    }
  });

  // Listen for data schema editor open requests from AssetBrowser
  $effect(() => {
    const handler = async (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.fullPath) {
        await openDataSchemaFile(detail.fullPath);
        centerTab = "dataschema";
      }
    };
    window.addEventListener("open-data-schema-editor", handler);
    return () => window.removeEventListener("open-data-schema-editor", handler);
  });

  // Auto-switch tab when data schema editor closes
  $effect(() => {
    if (!$dataSchema && centerTab === "dataschema") {
      centerTab = "viewport";
    }
  });

  // The Servers panel's two logs: the local server's output, and builds /
  // deploys. Listened to here for the same reason as build output below - a
  // deploy started and then hidden is still captured.
  $effect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;
    listen<{ source: "local" | "fleet"; text: string }>("server-output", (e) =>
      appendServerLine(e.payload.source, e.payload.text),
    ).then((unlisten) => {
      if (cancelled) unlisten();
      else stop = unlisten;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  });

  // Build output streamed from the Tauri side (cargo / wasm-pack). Lives
  // here rather than in the Build panel so a build started with the panel
  // hidden - or with the Build Settings dialog closed - is still captured.
  $effect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;
    listen<{ text: string; replaces_previous: boolean }>(
      "build-output",
      (e) => appendBuildLine(e.payload.text, e.payload.replaces_previous),
    ).then(
      (unlisten) => {
        if (cancelled) unlisten();
        else stop = unlisten;
      },
    );
    return () => {
      cancelled = true;
      stop?.();
    };
  });

  // Listen for data table editor open requests from AssetBrowser
  $effect(() => {
    const handler = async (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.fullPath) {
        await openDataTableFile(detail.fullPath);
        centerTab = "datatable";
      }
    };
    window.addEventListener("open-data-table-editor", handler);
    return () => window.removeEventListener("open-data-table-editor", handler);
  });

  // Auto-switch tab when data table editor closes
  $effect(() => {
    if (!$dataTable && centerTab === "datatable") {
      centerTab = "viewport";
    }
  });

  // Listen for audio preview open requests from AssetBrowser
  $effect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.projectPath && detail?.relativePath) {
        audioPreviewRef?.open(detail.projectPath, detail.relativePath);
      }
    };
    window.addEventListener("open-audio-preview", handler);
    return () => window.removeEventListener("open-audio-preview", handler);
  });

  // Close preview tabs when project changes (close/open/new)
  $effect(() => {
    void $project; // track project changes
    untrack(() => {
      previewImage = null;
      closeGuiEditor();
      closePrefabEditor();
      closeParticleEditor();
      closeAnimEditor();
      closeStateMachineEditor();
      closeVectorPathEditor();
      closeFontEditor();
      closeTilemapEditor();
      closeBehaviorEditor();
      closeSortLayersEditor();
      closeAllScriptTabs();
      closeDataSchemaEditor();
      closeDataTableEditor();
      audioPreviewRef?.close();
      shaderGraph = null;
      shaderProjectPath = undefined;
      shaderRelativePath = undefined;
      if (
        centerTab === "image" ||
        centerTab === "gui" ||
        centerTab === "shader" ||
        centerTab === "prefab" ||
        centerTab === "particle" ||
        centerTab === "animation" ||
        centerTab === "statemachine" ||
        centerTab === "vectorpath" ||
        centerTab === "font" ||
        centerTab === "tilemap" ||
        centerTab === "script" ||
        centerTab === "behavior" ||
        centerTab === "sortlayers" ||
        centerTab === "dataschema" ||
        centerTab === "datatable"
      )
        centerTab = "viewport";
    });
  });

  // Listen for image preview requests from AssetBrowser
  let previewImage = $state<{
    projectPath: string;
    relativePath: string;
  } | null>(null);
  $effect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.projectPath && detail?.relativePath) {
        previewImage = {
          projectPath: detail.projectPath,
          relativePath: detail.relativePath,
        };
        centerTab = "image";
      }
    };
    window.addEventListener("open-image-preview", handler);
    return () => window.removeEventListener("open-image-preview", handler);
  });

  function closeImagePreview() {
    previewImage = null;
    if (centerTab === "image") centerTab = "viewport";
  }

  // Listen for open-shader-editor events from AssetBrowser
  $effect(() => {
    const handler = async (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.projectPath && detail?.relativePath) {
        try {
          const json = await shaderLoad(
            detail.projectPath,
            detail.relativePath,
          );
          shaderGraph = JSON.parse(json);
          shaderProjectPath = detail.projectPath;
          shaderRelativePath = detail.relativePath;
          centerTab = "shader";
        } catch (err) {
          console.error("Failed to load shader:", err);
        }
      }
    };
    window.addEventListener("open-shader-editor", handler);
    return () => window.removeEventListener("open-shader-editor", handler);
  });

  // Auto-switch tab when shader editor closes
  $effect(() => {
    if (!shaderGraph && centerTab === "shader") {
      centerTab = "viewport";
    }
  });

  function closeShaderEditor() {
    shaderGraph = null;
    shaderProjectPath = undefined;
    shaderRelativePath = undefined;
    if (centerTab === "shader") centerTab = "viewport";
  }

  // Listen for custom open-gui-editor events from AssetBrowser
  $effect(() => {
    window.addEventListener("open-gui-editor", handleOpenGuiEditor);
    return () =>
      window.removeEventListener("open-gui-editor", handleOpenGuiEditor);
  });

  // Listen for open-spritesheet-editor events from AssetBrowser
  let spritesheetAtlasPath = $state<string | undefined>(undefined);
  $effect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      spritesheetAtlasPath = detail?.atlasPath;
      showSpritesheet = true;
    };
    window.addEventListener("open-spritesheet-editor", handler);
    return () => window.removeEventListener("open-spritesheet-editor", handler);
  });

  // The material editor, opened on a `.material` (openAsset).
  let materialEditorPath = $state<{ projectPath: string; relativePath: string } | null>(null);
  $effect(() => {
    const handler = (e: Event) => {
      const d = (e as CustomEvent).detail;
      if (d?.projectPath && d?.relativePath) materialEditorPath = { projectPath: d.projectPath, relativePath: d.relativePath };
    };
    window.addEventListener("open-material-editor", handler);
    return () => window.removeEventListener("open-material-editor", handler);
  });

  // The skeletal editor, opened on a `.skelanim` or a `.rig` (openAsset).
  let skeletalEditorPath = $state<{ projectPath: string; relativePath: string } | null>(null);
  $effect(() => {
    const handler = (e: Event) => {
      const d = (e as CustomEvent).detail;
      if (d?.projectPath && d?.relativePath) skeletalEditorPath = { projectPath: d.projectPath, relativePath: d.relativePath };
    };
    window.addEventListener("open-skeletal-editor", handler);
    return () => window.removeEventListener("open-skeletal-editor", handler);
  });

  // The skin editor: a scene skeleton (index), or a new character rigged
  // from a picture (index -1 and its texture).
  let skinEditor = $state<{ index: number; texture: string } | null>(null);
  $effect(() => {
    const handler = (e: Event) => {
      const d = (e as CustomEvent).detail;
      if (typeof d?.index === "number") skinEditor = { index: d.index, texture: d.texture ?? "" };
    };
    window.addEventListener("open-skin-editor", handler);
    return () => window.removeEventListener("open-skin-editor", handler);
  });

  // A video or font file's preview (openAsset).
  let mediaPreview = $state<{ projectPath: string; relativePath: string } | null>(null);
  $effect(() => {
    const handler = (e: Event) => {
      const d = (e as CustomEvent).detail;
      if (d?.projectPath && d?.relativePath) mediaPreview = { projectPath: d.projectPath, relativePath: d.relativePath };
    };
    window.addEventListener("open-media-preview", handler);
    return () => window.removeEventListener("open-media-preview", handler);
  });

  // The sprite-sheet slicer, opened on an image from the asset browser.
  let sheetSlicerPath = $state<string | null>(null);
  $effect(() => {
    const handler = (e: Event) => {
      const path = (e as CustomEvent).detail?.imagePath;
      if (typeof path === "string") sheetSlicerPath = path;
    };
    window.addEventListener("open-sheet-slicer", handler);
    return () => window.removeEventListener("open-sheet-slicer", handler);
  });

  // Listen for open-atlas-editor events from AssetBrowser
  $effect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.atlasPath) {
        atlasEditorPath = detail.atlasPath;
        showAtlasEditor = true;
      }
    };
    window.addEventListener("open-atlas-editor", handler);
    return () => window.removeEventListener("open-atlas-editor", handler);
  });

  // ── Editor viewport package staleness ───────────────────────────────────
  //
  // Rust packages are linked at compile time, so a project's own package only
  // runs in the editor if the viewport WASM was built with it. The viewport
  // directory is shared between projects, so it carries whichever project it
  // was built for last. Left unchecked the symptom is a package that does
  // nothing, with nothing on screen explaining why.
  let viewportStatus = $state<ViewportStatus | null>(null);
  let viewportRebuilding = $state(false);
  let viewportBannerDismissed = $state(false);
  /** Loading the project's own cached build; the page reloads when done. */
  let viewportSwitching = $state(false);
  let lastViewportCheck = $state("");

  $effect(() => {
    const proj = $project;
    if (!proj?.path) return;
    if (proj.path === lastViewportCheck) return;
    lastViewportCheck = proj.path;
    viewportBannerDismissed = false;
    viewportPackageStatus(proj.path)
      .then((status) => {
        // The project's own build is current: load it rather than ask for a
        // compile. A reload loses unsaved work, so with any the banner offers
        // it instead of doing it. Nothing is wrong in this case, so the
        // warning is never shown - only a quiet line until the page reloads.
        if (status.needs_rebuild && status.cached && get(unsavedWork).length === 0) {
          viewportStatus = null;
          void loadCachedViewport();
          return;
        }
        viewportStatus = status;
        if (status.needs_rebuild) logError(status.reason);
      })
      // A failed check must not block opening a project — it is advisory.
      .catch((e) => logError(`Could not check viewport packages: ${e}`));
  });

  async function loadCachedViewport() {
    const proj = $project;
    if (!proj?.path || viewportRebuilding) return;
    if (
      get(unsavedWork).length > 0 &&
      !confirm(
        `Loading this project's viewport reloads the editor, discarding unsaved work:\n\n${get(unsavedWork).join("\n")}\n\nContinue?`,
      )
    )
      return;
    viewportRebuilding = true;
    viewportSwitching = true;
    markReopen(proj.path);
    try {
      if (await switchProjectViewport(proj.path)) {
        reloadIntoProject(proj.path);
        return;
      }
      // Went stale between the check and the click: rebuild after all.
      clearReopen();
      viewportSwitching = false;
      viewportStatus = await viewportPackageStatus(proj.path);
      logError(viewportStatus.reason);
    } catch (e) {
      clearReopen();
      viewportSwitching = false;
      logError(`${e}`);
    } finally {
      viewportRebuilding = false;
    }
  }

  /** Ask again whether the viewport carries the project's packages. */
  async function refreshViewportStatus() {
    const proj = $project;
    if (!proj?.path || viewportRebuilding) return;
    try {
      viewportStatus = await viewportPackageStatus(proj.path);
      viewportBannerDismissed = false;
    } catch (e) {
      logError(`${e}`);
    }
  }

  async function rebuildViewport() {
    const proj = $project;
    if (!proj?.path || viewportRebuilding) return;
    viewportRebuilding = true;
    // Same treatment as a game build: the Build panel shows the live compile,
    // which takes a couple of minutes.
    beginBuild(`Editor viewport - ${proj.name ?? proj.path}`);
    focusPanel("build");
    // The new module is copied into place at the end of the build.
    if (get(unsavedWork).length === 0) markReopen(proj.path);
    try {
      const linked = await rebuildProjectViewport(proj.path);
      // The build is in the editor's module directory now, but the page still
      // runs the old one. Reload into the project unless that would lose work.
      const reload = linked.length > 0 && get(unsavedWork).length === 0;
      endBuild(
        linked.length
          ? reload
            ? `Viewport rebuilt with: ${linked.join(", ")}. Reloading the editor to load it...`
            : `Viewport rebuilt with: ${linked.join(", ")}. Save your work and reload the editor (Ctrl+R) to load it.`
          : "Viewport rebuilt (no project packages).",
      );
      if (reload) {
        reloadIntoProject(proj.path);
        return;
      }
      clearReopen();
      viewportStatus = await viewportPackageStatus(proj.path);
    } catch (e) {
      clearReopen();
      endBuild(`Viewport rebuild failed: ${e}`);
      logError(`Viewport rebuild failed: ${e}`);
      void checkToolsAfterBuildFailure("viewport");
    } finally {
      viewportRebuilding = false;
    }
  }

  // Reload package schema when project or enabled_packages changes
  let lastSchemaKey = $state("");
  $effect(() => {
    if ($project) {
      const key = JSON.stringify([
        $project.path,
        $project.settings.enabled_packages ?? [],
      ]);
      if (key !== lastSchemaKey) {
        lastSchemaKey = key;
        reloadPackageSchema(
          $project.settings.enabled_packages ?? [],
          $project.path,
        );
      }
    }
  });

  // Register package panels whenever the schema updates
  $effect(() => {
    const schema = $packageSchema;
    if (!schema) return;
    for (const panel of schema.panels) {
      const panelId = `pkg:${panel.name}` as PanelId;
      const zone = (
        ["left", "right", "bottom"].includes(panel.default_zone)
          ? panel.default_zone
          : "right"
      ) as ZoneId;
      registerPackagePanel(panelId, {
        name: panel.name,
        icon: panel.icon,
        defaultZone: zone,
        url: `${panel.package_path}|||${panel.web_bundle_path}`,
        packageName: panel.name,
      });
    }
  });

  /** Returns true if the event target is inside a text-editing context
   *  (CodeMirror, input, textarea, contentEditable) where the browser's
   *  native clipboard / keyboard handling should not be intercepted. */

  /** Returns true when there is a non-collapsed text selection anywhere in
   *  the document — used so Ctrl+C copies selected text instead of entities. */
  function hasTextSelection(): boolean {
    const sel = window.getSelection();
    return !!sel && !sel.isCollapsed;
  }

  // During running play the game owns the keyboard; this decides, per
  // press, which keys the editor keeps. A key given to the game never
  // reaches `handleKeydown` or any other window-level shortcut.
  $effect(() =>
    installPlayKeyRouter((e) => ({
      playing: get(isPlaying),
      paused: get(isPaused),
      view: get(viewMode),
      editable: isTextEditingTarget(e),
      textSelected: hasTextSelection(),
    })),
  );

  function handleKeydown(e: KeyboardEvent) {
    // Window-level keys, handled before the project guard so they work on
    // the start screen too. WebView2's own handling of these is switched
    // off at startup (see `editor_window::disable_browser_accelerator_keys`)
    // so that F3 and friends reach the editor — which means reload and
    // devtools are ours to implement.
    if (
      (e.key === "F5" && !e.shiftKey) ||
      (e.ctrlKey && !e.shiftKey && e.key === "r")
    ) {
      e.preventDefault();
      reloadEditorWindow();
      return;
    }
    if (e.key === "F12" || (e.ctrlKey && e.shiftKey && e.key === "I")) {
      e.preventDefault();
      void toggleDevtools().catch((err) => logError(`Devtools: ${err}`));
      return;
    }
    // Play controls: Unity's pause and step, and Visual Studio's stop (F5
    // is reload here, and Escape belongs to the game). These are the keys
    // `playKeys` lets through while the game owns the keyboard.
    if (isPauseKey(e)) {
      e.preventDefault();
      window.dispatchEvent(new CustomEvent("editor-pause"));
      return;
    }
    if (isStepKey(e)) {
      e.preventDefault();
      window.dispatchEvent(new CustomEvent("editor-step"));
      return;
    }
    if (isStopKey(e)) {
      e.preventDefault();
      window.dispatchEvent(new CustomEvent("editor-stop"));
      return;
    }
    if (!$project) return;
    // Ctrl+Home - back to the scene the runtime boots into.
    if (e.ctrlKey && !e.altKey && !e.shiftKey && e.key === "Home") {
      e.preventDefault();
      if ($project) void openEntryScene($project);
      return;
    }
    // Build & Run (Defold's Ctrl+B), the open scene with shift, and Ctrl+J to
    // close what it started.
    if (e.ctrlKey && !e.altKey && e.code === "KeyB") {
      e.preventDefault();
      void buildAndRun(e.shiftKey);
      return;
    }
    if (e.ctrlKey && !e.altKey && !e.shiftKey && e.code === "KeyJ") {
      e.preventDefault();
      void closeInstances();
      return;
    }
    // Command palette. VS Code's Ctrl+Shift+P is already Unity's pause
    // binding, so this uses Rider's "Find Action" chord instead.
    if (e.ctrlKey && e.shiftKey && !e.altKey && e.code === "KeyA") {
      e.preventDefault();
      showCommandPalette = !showCommandPalette;
      return;
    }
    // Find in files. Focuses the Search panel's box, opening the panel first
    // if it is not the active tab in its dock.
    if (e.ctrlKey && e.shiftKey && !e.altKey && e.code === "KeyF") {
      e.preventDefault();
      focusPanel("search");
      requestSearchFocus();
      return;
    }
    // Replace in files - the same panel with its replace row open.
    if (e.ctrlKey && e.shiftKey && !e.altKey && e.code === "KeyH") {
      e.preventDefault();
      focusPanel("search");
      requestReplaceFocus();
      return;
    }
    // Ctrl+Tab — back to the script you were just in. Handled outside the
    // plain-Ctrl branch because Tab arrives as `e.key === "Tab"` and the
    // browser would otherwise move focus out of the editor.
    if (e.ctrlKey && !e.altKey && e.key === "Tab") {
      e.preventDefault();
      // Walks every open document, not just the last two scripts: with
      // scenes in the same strip, "the next file" is what the key means
      // everywhere else. Shift goes back.
      cycleDocument(e.shiftKey ? -1 : 1);
      return;
    }
    if (e.key === "F1") {
      e.preventDefault();
      showShortcuts = !showShortcuts;
      return;
    }
    if (e.ctrlKey && !e.altKey) {
      if (e.key === "s" && !e.shiftKey) {
        e.preventDefault();
        dispatchGlobalSave();
      } else if (e.key === "p" && !e.shiftKey) {
        // Quick open. Ctrl+Shift+P and Ctrl+Alt+P are the play controls and
        // are handled above, so this branch only ever sees the plain chord.
        e.preventDefault();
        showQuickOpen = !showQuickOpen;
      } else if (e.key === "o" && !e.shiftKey) {
        e.preventDefault();
        showSceneSwitcher = !showSceneSwitcher;
      } else if (e.key === "d" && !e.shiftKey) {
        e.preventDefault();
        window.dispatchEvent(new CustomEvent("editor-duplicate"));
      } else if (e.key === "c" && !e.shiftKey) {
        // Let the browser handle Ctrl+C in text contexts or when text is selected
        if (isTextEditingTarget(e) || hasTextSelection()) return;
        e.preventDefault();
        const sel = get(selectedEntities);
        if (sel.size > 0) {
          const currentScene = get(scene);
          const entities = [...sel]
            .sort((a, b) => a - b)
            .map((i) => currentScene.entities[i])
            .filter(Boolean);
          if (entities.length > 0) {
            entityClipboard.set(entities.map((e) => structuredClone(e)));
          }
        }
      } else if (e.key === "v" && !e.shiftKey) {
        // Let the browser handle Ctrl+V in text contexts
        if (isTextEditingTarget(e)) return;
        e.preventDefault();
        const clipboard = get(entityClipboard);
        if (clipboard && clipboard.length > 0) {
          const currentScene = get(scene);
          const before = structuredClone(currentScene);
          const updated = structuredClone(currentScene);
          const existingNames = new Set(
            updated.entities.map((e: { name: string }) => e.name),
          );
          for (const ent of clipboard) {
            const clone = structuredClone(ent);
            let name = `${ent.name}_copy`;
            let counter = 1;
            while (existingNames.has(name)) {
              name = `${ent.name}_copy${counter++}`;
            }
            clone.name = name;
            existingNames.add(name);
            updated.entities.push(clone);
          }
          pushCommand(
            `Paste ${clipboard.length} entit${clipboard.length > 1 ? "ies" : "y"}`,
            before,
            updated,
          );
          selectEntity(updated.entities.length - 1);
        }
      } else if (e.key === "z" && !e.shiftKey) {
        // An editor open over the scene with a history of its own claims the
        // keys (`undoTargetStore`); the prefab editor keeps its own history
        // too - route to it while its tab is active so a gizmo drag isn't
        // undone out of the scene.
        const target = undoTarget();
        if (target?.ignoreTyping && isTextEditingTarget(e)) return;
        e.preventDefault();
        if (target) target.undo();
        else if (centerTab === "prefab" && $prefabData) undoPrefab();
        else undo();
      } else if ((e.key === "z" && e.shiftKey) || e.key === "y") {
        const target = undoTarget();
        if (target?.ignoreTyping && isTextEditingTarget(e)) return;
        e.preventDefault();
        if (target) target.redo();
        else if (centerTab === "prefab" && $prefabData) redoPrefab();
        else redo();
      }
    }
  }

  function startResize(
    e: PointerEvent,
    prop: "leftWidth" | "rightWidth" | "bottomHeight",
    axis: "x" | "y",
    invert: boolean,
    min: number,
    max: number,
  ) {
    e.preventDefault();
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const start = axis === "x" ? e.clientX : e.clientY;
    const startVal = $layout[prop];

    function onMove(ev: PointerEvent) {
      const delta = (axis === "x" ? ev.clientX : ev.clientY) - start;
      const newVal = Math.round(
        Math.max(min, Math.min(max, startVal + (invert ? -delta : delta))),
      );
      setZoneSize(prop, newVal);
    }
    function onUp() {
      target.removeEventListener("pointermove", onMove);
      target.removeEventListener("pointerup", onUp);
    }
    target.addEventListener("pointermove", onMove);
    target.addEventListener("pointerup", onUp);
  }

  /** A script tab's kind label: its language, by extension. */
  function scriptKind(path: string): string {
    const ext = path.split(".").pop()?.toLowerCase();
    return ext === "lua" ? "lua" : ext === "js" ? "js" : "script";
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<Toasts />

{#if !$project}
  <StartScreen />
{:else}
  {#snippet renderPanel(panelId: PanelId)}
    {#if panelId === "scene"}
      <SceneTree />
    {:else if panelId === "assets"}
      <AssetBrowser />
    {:else if panelId === "inspector"}
      <Inspector
        showGuiInspector={centerTab === "gui"}
        showPrefabInspector={centerTab === "prefab"}
        onmanagecomponents={() => (showComponents = true)}
      />
    {:else if panelId === "console"}
      <Console />
    {:else if panelId === "build"}
      <BuildOutput />
    {:else if panelId === "servers"}
      <ServersPanel />
    {:else if panelId === "timeline"}
      <TimelinePanel />
    {:else if panelId === "localisation"}
      <LocalisationPanel />
    {:else if panelId === "mixer"}
      <MixerPanel />
    {:else if panelId === "debugger"}
      <DebuggerPanel />
    {:else if panelId === "profiler"}
      <ProfilerPanel />
    {:else if panelId === "search"}
      <FindInFiles />
    {:else if panelId === "history"}
      <UndoHistory />
    {:else if panelId.startsWith("pkg:")}
      {@const info = $packagePanels.get(panelId)}
      {#if info}
        <PackagePanel panelInfo={info} />
      {:else}
        <div style="padding: 12px; color: var(--ed-faint); font-size: 12px;">
          Panel not available
        </div>
      {/if}
    {/if}
  {/snippet}

  <div class="editor-layout">
    {#if viewportSwitching}
      <div class="viewport-package-banner vpb-info">
        <span class="vpb-text">Loading this project's viewport...</span>
      </div>
    {:else if viewportStatus?.needs_rebuild && !viewportBannerDismissed}
      <div class="viewport-package-banner">
        <span class="vpb-icon">&#9888;</span>
        <span class="vpb-text">
          {viewportStatus.reason}
          {#if viewportStatus.cached}
            This project's own build is current - loading it reloads the editor.
          {:else}
            Its systems and components will not run here until it is rebuilt.
          {/if}
        </span>
        {#if viewportStatus.cached}
          <button
            class="vpb-action"
            onclick={loadCachedViewport}
            disabled={viewportRebuilding}
          >
            Load project viewport
          </button>
        {:else}
          <button
            class="vpb-action"
            onclick={rebuildViewport}
            disabled={viewportRebuilding}
          >
            {viewportRebuilding ? "Rebuilding..." : "Rebuild viewport"}
          </button>
        {/if}
        <button
          class="vpb-dismiss"
          title="Dismiss until the next project change"
          onclick={() => (viewportBannerDismissed = true)}>&#215;</button
        >
      </div>
    {/if}
    <Toolbar
      onsettings={() => (showSettings = true)}
      oncomponents={() => (showComponents = true)}
      onbuild={() => (showBuild = true)}
      onspritesheet={() => {
        spritesheetAtlasPath = undefined;
        showSpritesheet = true;
      }}
      onpackages={() => (showPackages = true)}
      onnewrustpackage={() => (showNewRustPackage = true)}
    />
    <div class="main-area">
      <!-- Left zone -->
      {#if leftHasContent || $isPanelDragging}
        <div
          class="zone side-zone"
          class:zone-empty={!leftHasContent}
          style="width: {leftHasContent ? $layout.leftWidth + 'px' : '28px'}"
        >
          <DockZone
            zone="left"
            groups={$layout.zones.left.groups}
            {renderPanel}
          />
        </div>
        {#if leftHasContent}
          <div
            class="resize-handle-v"
            onpointerdown={(e) =>
              startResize(e, "leftWidth", "x", false, 160, 500)}
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize left panel"
          ></div>
        {/if}
      {/if}

      <!-- Center: Tabs + Bottom zone -->
      <div class="center-area">
        <!-- Tab bar -->
        {#if $sceneTabs.length > 1 || $guiDocument || previewImage || shaderGraph || $prefabData || $particleEmitter || $animData || $fontData || $tilemapData || $scriptTabs.length > 0 || $behaviorJson || $vectorPathData || $dataSchema || $dataTable}
          <!-- A tab names its kind in mono before its name, as in the design:
               SCENE, LUA, PREFAB, SHADER ... each in its own hue. -->
          {#snippet tabKind(kind: string)}
            <span class="tab-kind" data-kind={kind}>{kind}</span>
          {/snippet}
          <div class="center-tabs" bind:this={centerTabsEl}>
            <!-- Scenes are open documents like everything else here, so they
                 sit in this strip rather than in a second one nested under a
                 "Viewport" tab - where opening a script hid them. The bare
                 Viewport button remains only when no scene is open as a tab,
                 so there is always a way back to it. -->
            {#if $sceneTabs.length === 0}
              <button
                class="center-tab"
                class:active={centerTab === "viewport"}
                onclick={() => (centerTab = "viewport")}>Viewport</button
              >
            {/if}
            {#each $sceneTabs as tab, i (tab.path ?? i)}
              <button
                class="center-tab"
                class:active={centerTab === "viewport" && i === $activeSceneTab}
                title={tab.path ?? "Unsaved scene"}
                onclick={() => {
                  activateSceneTab(i);
                  centerTab = "viewport";
                }}
              >
{@render tabKind("scene")}{tab.path?.split(/[\\\\/]/).pop() ??
                  "Untitled Scene"}{#if sceneTabDirty(i)}<span class="tab-dot"
                    >●</span
                  >{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestSceneClose(i);
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestSceneClose(i);
                    }
                  }}>✕</span
                >
              </button>
            {/each}
            {#if $guiDocument}
              <button
                class="center-tab"
                class:active={centerTab === "gui"}
                onclick={() => (centerTab = "gui")}
              >
                {@render tabKind("gui")}GUI Editor{#if $guiDirty}<span class="tab-dot">●</span>{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestEditorClose("gui");
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestEditorClose("gui");
                    }
                  }}>✕</span
                >
              </button>
            {/if}
            {#if previewImage}
              <button
                class="center-tab"
                class:active={centerTab === "image"}
                onclick={() => (centerTab = "image")}
              >
                {@render tabKind("image")}{previewImage.relativePath.split("/").pop()}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    closeImagePreview();
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      closeImagePreview();
                    }
                  }}>✕</span
                >
              </button>
            {/if}
            {#if shaderGraph}
              <button
                class="center-tab"
                class:active={centerTab === "shader"}
                onclick={() => (centerTab = "shader")}
              >
                {@render tabKind("shader")}{shaderGraph.meta.name || "Shader"}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    closeShaderEditor();
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      closeShaderEditor();
                    }
                  }}>✕</span
                >
              </button>
            {/if}
            {#if $prefabData}
              <button
                class="center-tab"
                class:active={centerTab === "prefab"}
                onclick={() => (centerTab = "prefab")}
              >
                {@render tabKind("prefab")}{$prefabData.name}{#if $prefabDirty}<span class="tab-dot">●</span>{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestEditorClose("prefab");
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestEditorClose("prefab");
                    }
                  }}>✕</span
                >
              </button>
            {/if}
            {#if $particleEmitter}
              <button
                class="center-tab"
                class:active={centerTab === "particle"}
                onclick={() => (centerTab = "particle")}
              >
                {@render tabKind("particles")}Particle{#if $particleDirty}<span class="tab-dot">●</span>{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestEditorClose("particle");
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestEditorClose("particle");
                    }
                  }}>✕</span
                >
              </button>
            {/if}
            {#if $animData}
              <button
                class="center-tab"
                class:active={centerTab === "animation"}
                onclick={() => (centerTab = "animation")}
              >
                {@render tabKind("anim")}{$selectedClipName || "Animation"}{#if $animDirty}<span class="tab-dot">●</span>{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestEditorClose("animation");
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestEditorClose("animation");
                    }
                  }}>✕</span
                >
              </button>
            {/if}
            {#if $stateMachineData}
              <button
                class="center-tab"
                class:active={centerTab === "statemachine"}
                onclick={() => (centerTab = "statemachine")}
              >
                {@render tabKind("states")}State Machine{#if $stateMachineDirty}<span class="tab-dot">●</span>{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestEditorClose("statemachine");
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestEditorClose("statemachine");
                    }
                  }}>✕</span
                >
              </button>
            {/if}
            {#if $vectorPathData}
              <button
                class="center-tab"
                class:active={centerTab === "vectorpath"}
                onclick={() => (centerTab = "vectorpath")}
              >
                {@render tabKind("path")}Vector Path{#if $vectorPathDirty}<span class="tab-dot">●</span>{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestEditorClose("vectorpath");
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestEditorClose("vectorpath");
                    }
                  }}>✕</span
                >
              </button>
            {/if}
            {#if $fontData}
              <button
                class="center-tab"
                class:active={centerTab === "font"}
                onclick={() => (centerTab = "font")}
              >
                {@render tabKind("font")}Font{#if $fontDirty}<span class="tab-dot">●</span>{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestEditorClose("font");
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestEditorClose("font");
                    }
                  }}>✕</span
                >
              </button>
            {/if}
            {#if $tilemapData}
              <button
                class="center-tab"
                class:active={centerTab === "tilemap"}
                onclick={() => (centerTab = "tilemap")}
              >
                {@render tabKind("tilemap")}Tilemap{#if $tilemapDirty}<span class="tab-dot">●</span>{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestEditorClose("tilemap");
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestEditorClose("tilemap");
                    }
                  }}>✕</span
                >
              </button>
            {/if}
            {#if $dataSchema}
              <button
                class="center-tab"
                class:active={centerTab === "dataschema"}
                onclick={() => (centerTab = "dataschema")}
              >
                {@render tabKind("schema")}Schema{#if $dataSchemaDirty}<span class="tab-dot">●</span>{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestEditorClose("dataschema");
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestEditorClose("dataschema");
                    }
                  }}>✕</span
                >
              </button>
            {/if}
            {#if $dataTable}
              <button
                class="center-tab"
                class:active={centerTab === "datatable"}
                onclick={() => (centerTab = "datatable")}
              >
                {@render tabKind("data")}Data{#if $dataTableDirty}<span class="tab-dot">●</span>{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestEditorClose("datatable");
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestEditorClose("datatable");
                    }
                  }}>✕</span
                >
              </button>
            {/if}
            {#each $scriptTabs as tab (tab.path)}
              <button
                class="center-tab"
                class:active={centerTab === "script" &&
                  $activeScriptPath === tab.path}
                onclick={() => {
                  activeScriptPath.set(tab.path);
                  centerTab = "script";
                }}
              >
                {@render tabKind(scriptKind(tab.path))}{tab.path.split(/[\\/]/).pop()}{#if tab.dirty}<span class="tab-dot">●</span>{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestScriptClose(tab.path);
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestScriptClose(tab.path);
                    }
                  }}>✕</span
                >
              </button>
            {/each}
            {#if $behaviorJson}
              <button
                class="center-tab"
                class:active={centerTab === "behavior"}
                onclick={() => (centerTab = "behavior")}
              >
                {@render tabKind("behavior")}{($behaviorFilePath ?? "").split(/[\\/]/).pop() ??
                  "Behavior"}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestEditorClose("behavior");
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestEditorClose("behavior");
                    }
                  }}>✕</span
                >
              </button>
            {/if}
            {#if $sortLayersData}
              <button
                class="center-tab"
                class:active={centerTab === "sortlayers"}
                onclick={() => (centerTab = "sortlayers")}
              >
                {@render tabKind("layers")}{($sortLayersFilePath ?? "").split(/[\\/]/).pop() ??
                  "Sort Layers"}{#if $sortLayersDirty}<span class="tab-dot">●</span>{/if}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e: MouseEvent) => {
                    e.stopPropagation();
                    requestEditorClose("sortlayers");
                  }}
                  onkeydown={(e: KeyboardEvent) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      requestEditorClose("sortlayers");
                    }
                  }}>✕</span
                >
              </button>
            {/if}
          </div>
        {/if}
        <div class="viewport-area">
          {#if centerTab === "shader" && shaderGraph}
            <ShaderEditor
              bind:graph={shaderGraph}
              projectPath={shaderProjectPath}
              relativePath={shaderRelativePath}
              onclose={closeShaderEditor}
            />
          {:else if centerTab === "image" && previewImage}
            <ImagePreview
              projectPath={previewImage.projectPath}
              relativePath={previewImage.relativePath}
              onclose={closeImagePreview}
            />
          {:else if centerTab === "gui" && $guiDocument}
            <GuiEditor />
          {:else if centerTab === "prefab" && $prefabData}
            <PrefabEditor />
          {:else if centerTab === "particle" && $particleEmitter}
            <ParticleEditor />
          {:else if centerTab === "animation" && $animData}
            <AnimationEditor />
          {:else if centerTab === "statemachine" && $stateMachineData}
            <StateMachineEditor />
          {:else if centerTab === "vectorpath" && $vectorPathData}
            <VectorPathEditor />
          {:else if centerTab === "font" && $fontData}
            <FontEditor />
          {:else if centerTab === "tilemap" && $tilemapData}
            <TilemapEditor />
          {:else if centerTab === "script" && $scriptTabs.length > 0}
            <ScriptEditor />
          {:else if centerTab === "behavior" && $behaviorJson}
            <BehaviorEditor />
          {:else if centerTab === "sortlayers" && $sortLayersData}
            <SortLayersEditor />
          {:else if centerTab === "dataschema" && $dataSchema}
            <DataSchemaEditor />
          {:else if centerTab === "datatable" && $dataTable}
            <DataTableEditor />
          {:else}
            <Viewport />
          {/if}
        </div>
        <AudioPreview bind:this={audioPreviewRef} />
        {#if bottomHasContent || $isPanelDragging}
          {#if bottomHasContent}
            <div
              class="resize-handle-h"
              onpointerdown={(e) =>
                startResize(e, "bottomHeight", "y", true, 80, 500)}
              role="separator"
              aria-orientation="horizontal"
              aria-label="Resize bottom panel"
            ></div>
          {/if}
          <div
            class="zone bottom-zone"
            class:zone-empty={!bottomHasContent}
            style="height: {bottomHasContent
              ? $layout.bottomHeight + 'px'
              : '28px'}"
          >
            <DockZone
              zone="bottom"
              groups={$layout.zones.bottom.groups}
              {renderPanel}
            />
          </div>
        {/if}
      </div>

      <!-- Right zone -->
      {#if rightHasContent || $isPanelDragging}
        {#if rightHasContent}
          <div
            class="resize-handle-v"
            onpointerdown={(e) =>
              startResize(e, "rightWidth", "x", true, 200, 500)}
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize right panel"
          ></div>
        {/if}
        <div
          class="zone side-zone"
          class:zone-empty={!rightHasContent}
          style="width: {rightHasContent ? $layout.rightWidth + 'px' : '28px'}"
        >
          <DockZone
            zone="right"
            groups={$layout.zones.right.groups}
            {renderPanel}
          />
        </div>
      {/if}
    </div>
    {#if $dirty || $guiDirty}
      <div class="status-bar dirty">Unsaved changes</div>
    {:else}
      <div class="status-bar">Ready</div>
    {/if}
  </div>

  {#if showSettings}
    <ProjectSettings onclose={() => (showSettings = false)} />
  {/if}

  {#if showComponents}
    <ManageComponents onclose={() => (showComponents = false)} />
  {/if}

  {#if showBuild}
    <BuildSettings onclose={() => (showBuild = false)} />
  {/if}

  {#if showPackages}
    <PackageManager
      onclose={() => (showPackages = false)}
      onnewrustpackage={() => (showNewRustPackage = true)}
    />
  {/if}

  {#if showNewRustPackage}
    <NewRustPackage
      onclose={() => {
        showNewRustPackage = false;
        // A package was likely added: ask again whether the viewport needs a
        // rebuild, so the banner offers one if "Rebuild now" was skipped.
        void refreshViewportStatus();
      }}
      onrebuild={() => void rebuildViewport()}
    />
  {/if}

  {#if showShortcuts}
    <KeyboardShortcuts onclose={() => (showShortcuts = false)} />
  {/if}

  {#if showAbout}
    <AboutDialog
      onclose={() => (showAbout = false)}
      onlicenses={() => {
        showAbout = false;
        showLicenses = true;
      }}
    />
  {/if}

  {#if showLicenses}
    <ThirdPartyLicenses onclose={() => (showLicenses = false)} />
  {/if}

  {#if pendingSceneClose !== null}
    <UnsavedDialog
      title="Close scene"
      items={[sceneTabName(pendingSceneClose)]}
      onchoose={resolveSceneClose}
    />
  {/if}

  {#if pendingScriptClose}
    <UnsavedDialog
      title="Close script"
      items={[scriptTabName(pendingScriptClose)]}
      onchoose={resolveScriptClose}
    />
  {/if}
  {#if $pendingEditorClose}
    <UnsavedDialog
      title="Close {editorLabel($pendingEditorClose).toLowerCase()}"
      items={[editorLabel($pendingEditorClose)]}
      canSave={canSaveEditor($pendingEditorClose)}
      onchoose={(c) => void resolveEditorClose(c)}
    />
  {/if}
  {#if closeChoice}
    <UnsavedDialog
      title="Close the editor"
      items={$unsavedWork}
      onchoose={(c) => closeChoice?.(c)}
    />
  {/if}
  {#if showQuickOpen}
    <QuickOpen onclose={() => (showQuickOpen = false)} />
  {/if}
  {#if showCommandPalette}
    <CommandPalette onclose={() => (showCommandPalette = false)} />
  {/if}
  {#if showSceneSwitcher}
    <SceneSwitcher onclose={() => (showSceneSwitcher = false)} />
  {/if}

  {#if showSpritesheet}
    <SpritesheetEditor
      atlasPath={spritesheetAtlasPath}
      onclose={() => {
        showSpritesheet = false;
        spritesheetAtlasPath = undefined;
      }}
    />
  {/if}

  {#if materialEditorPath}
    <MaterialEditor
      projectPath={materialEditorPath.projectPath}
      relativePath={materialEditorPath.relativePath}
      onclose={() => (materialEditorPath = null)}
    />
  {/if}

  {#if skeletalEditorPath}
    {#key skeletalEditorPath.relativePath}
      <SkeletalEditor
        projectPath={skeletalEditorPath.projectPath}
        relativePath={skeletalEditorPath.relativePath}
        onclose={() => (skeletalEditorPath = null)}
      />
    {/key}
  {/if}

  {#if skinEditor}
    {#key skinEditor}
      <SkinEditor rootIndex={skinEditor.index} texture={skinEditor.texture} onclose={() => (skinEditor = null)} />
    {/key}
  {/if}

  {#if mediaPreview}
    {#key mediaPreview.relativePath}
      <MediaPreview projectPath={mediaPreview.projectPath} relativePath={mediaPreview.relativePath} onclose={() => (mediaPreview = null)} />
    {/key}
  {/if}

  {#if sheetSlicerPath}
    <SheetSlicer imagePath={sheetSlicerPath} onclose={() => (sheetSlicerPath = null)} />
  {/if}

  {#if showAtlasEditor && atlasEditorPath}
    <AtlasEditor
      atlasPath={atlasEditorPath}
      onclose={() => {
        showAtlasEditor = false;
        atlasEditorPath = undefined;
      }}
    />
  {/if}
{/if}

<style>
  .editor-layout {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
  }

  /* Shown when the loaded viewport WASM does not carry this project's Rust
     packages. Amber rather than red: nothing is broken, but anything the
     package provides is inert until a rebuild, and silently so. */
  .viewport-package-banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 12px;
    background: color-mix(in srgb, var(--ed-warn) 13%, var(--ed-base));
    border-bottom: 1px solid color-mix(in srgb, var(--ed-warn) 40%, transparent);
    color: var(--ed-ink);
    font-size: 12px;
    flex-shrink: 0;
  }

  .vpb-icon {
    font-size: 14px;
    line-height: 1;
    color: var(--ed-warn);
  }

  .vpb-text {
    flex: 1;
    min-width: 0;
  }

  /* Nothing is wrong while a cached viewport loads - not a warning colour. */
  .viewport-package-banner.vpb-info {
    background: var(--ed-mantle);
    border-bottom-color: var(--ed-line);
    color: var(--ed-muted);
  }

  .vpb-action {
    background: none;
    border: 1px solid color-mix(in srgb, var(--ed-warn) 55%, transparent);
    color: var(--ed-warn);
    padding: 4px 10px;
    border-radius: 3px;
    cursor: pointer;
    font-size: 12px;
    white-space: nowrap;
  }

  .vpb-action:hover:not(:disabled) {
    background: color-mix(in srgb, var(--ed-warn) 18%, transparent);
  }

  .vpb-action:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .vpb-dismiss {
    background: none;
    border: none;
    color: var(--ed-muted);
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
    padding: 0 4px;
  }

  .vpb-dismiss:hover {
    color: var(--ed-ink);
  }

  .main-area {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .zone {
    flex-shrink: 0;
    overflow: hidden;
  }

  .side-zone {
    display: flex;
    flex-direction: column;
  }

  .zone-empty {
    opacity: 0.6;
  }

  .bottom-zone {
    flex-shrink: 0;
  }

  .center-area {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .viewport-area {
    flex: 1;
    min-height: 0;
  }

  /* The strip scrolls sideways rather than squeezing its tabs.
     Flex items shrink by default, so with a dozen documents open every tab
     was crushed below its own content width: the labels bled out of their
     boxes and the close button - an inline span - wrapped onto a second row
     that fell outside this 32px strip, where it flickered as the pointer
     chased it. `flex-shrink: 0` on the tab is the fix for both; this just
     gives the overflow somewhere to go. */
  .center-tabs {
    display: flex;
    background: var(--ed-mantle);
    border-bottom: 1px solid var(--ed-line);
    flex-shrink: 0;
    height: 34px;
    align-items: stretch;
    overflow-x: auto;
    /* Only sideways: a tab must never be scrolled out of the strip vertically. */
    overflow-y: hidden;
    scroll-behavior: smooth;
  }

  /* Thin and dim, so it reads as an affordance rather than furniture. It sits
     inside the strip's height, which is why that is 34px and not 32px. */
  .center-tabs::-webkit-scrollbar {
    height: 6px;
  }
  .center-tabs::-webkit-scrollbar-track {
    background: transparent;
  }
  .center-tabs::-webkit-scrollbar-thumb {
    background: var(--ed-raised);
    border-radius: 3px;
  }
  .center-tabs:hover::-webkit-scrollbar-thumb {
    background: var(--ed-line);
  }

  .center-tab {
    position: relative;
    /* Never squeeze: overflow scrolls the strip instead. */
    flex-shrink: 0;
    /* A long scene name is truncated rather than allowed to own the strip. */
    max-width: 240px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: left;
    /* Right padding reserves the close button's lane, so the ellipsis eats
       the label and never the button. */
    padding: 0 26px 0 12px;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    border-right: 1px solid var(--ed-line);
    color: var(--ed-faint);
    font-size: 12px;
    cursor: pointer;
    font-family: inherit;
    transition:
      color 0.1s,
      border-color 0.1s;
  }

  .center-tab:hover {
    color: var(--ed-text);
    background: var(--ed-base);
  }

  .center-tab.active {
    color: var(--ed-ink);
    background: var(--ed-base);
    box-shadow: inset 0 2px 0 var(--ed-accent);
  }

  /* Parked in the lane the tab's right padding reserves, so it is in the same
     place on every tab whatever the label does, and cannot wrap. 16x16 is a
     real hit target - it used to be a 10px glyph with no box of its own. */
  .tab-close {
    position: absolute;
    right: 5px;
    top: 50%;
    transform: translateY(-50%);
    width: 16px;
    height: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 3px;
    font-size: 10px;
    line-height: 1;
    opacity: 0.5;
    cursor: pointer;
  }

  .tab-close:hover {
    opacity: 1;
    background: var(--ed-line);
    color: var(--ed-err);
  }

  /* Vertical resize handle (between left/center and center/right) */
  .resize-handle-v {
    width: 5px;
    cursor: col-resize;
    flex-shrink: 0;
    position: relative;
    z-index: 1;
  }

  .resize-handle-v::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 2px;
    width: 1px;
    background: var(--ed-raised);
    transition: background 0.15s;
  }

  .resize-handle-v:hover::after,
  .resize-handle-v:active::after {
    background: var(--ed-accent);
  }

  /* Horizontal resize handle (between viewport and bottom zone) */
  .resize-handle-h {
    height: 5px;
    cursor: row-resize;
    flex-shrink: 0;
    position: relative;
    z-index: 1;
  }

  .resize-handle-h::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: 2px;
    height: 1px;
    background: var(--ed-raised);
    transition: background 0.15s;
  }

  .resize-handle-h:hover::after,
  .resize-handle-h:active::after {
    background: var(--ed-accent);
  }

  .status-bar {
    padding: 2px 10px;
    font-size: 11px;
    color: var(--ed-faint);
    background: var(--ed-mantle);
    border-top: 1px solid var(--ed-line);
    flex-shrink: 0;
  }

  /* Matches the scene tab strip's dot: the same thing must not look like
     two different things depending on which strip it is in. */
  /* Unsaved: an orange dot where the close button sits, the button itself
     shown on hover - as in the design. */
  .tab-dot {
    position: absolute;
    right: 9px;
    top: 50%;
    width: 7px;
    height: 7px;
    margin-top: -3.5px;
    border-radius: 50%;
    background: var(--ed-a3);
    font-size: 0;
    pointer-events: none;
  }

  .center-tab:hover .tab-dot {
    display: none;
  }

  .center-tab:has(.tab-dot):not(:hover) .tab-close {
    opacity: 0;
  }

  .tab-kind {
    margin-right: 8px;
    font-family: var(--ed-mono);
    font-size: 9.5px;
    font-weight: 500;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--ed-muted);
  }

  .tab-kind[data-kind="scene"] {
    color: var(--ed-accent);
  }
  .tab-kind[data-kind="lua"],
  .tab-kind[data-kind="js"],
  .tab-kind[data-kind="script"] {
    color: var(--ed-a2);
  }
  .tab-kind[data-kind="prefab"] {
    color: var(--ed-a3);
  }
  .tab-kind[data-kind="gui"],
  .tab-kind[data-kind="anim"],
  .tab-kind[data-kind="states"] {
    color: var(--ed-cat-6);
  }
  .tab-kind[data-kind="image"] {
    color: var(--ed-cat-5);
  }
  .tab-kind[data-kind="particles"],
  .tab-kind[data-kind="behavior"] {
    color: var(--ed-cat-8);
  }
  .tab-kind[data-kind="schema"],
  .tab-kind[data-kind="data"] {
    color: var(--ed-cat-4);
  }
  .tab-kind[data-kind="path"] {
    color: var(--ed-cat-2);
  }

  .status-bar.dirty {
    color: var(--ed-warn);
  }
</style>
