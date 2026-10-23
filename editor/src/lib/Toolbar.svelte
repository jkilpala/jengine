<script lang="ts">
  import { luaDebugger } from "./luaDebugger";
  import WindowControls from "./WindowControls.svelte";
  import { isMac } from "./api/editorWindow";
  import { debuggerHoldsPause } from "./stores/debuggerStore";
  import { get } from "svelte/store";
  import { scene, scenePath, dirty } from "./stores/sceneStore";
  import { project, requestAssetRefresh } from "./stores/projectStore";
  import {
    selectedEntity,
    selectEntity,
    clearSelection,
  } from "./stores/selectionStore";
  import {
    logInfo,
    logWarn,
    logError,
    clearLogs,
  } from "./stores/consoleStore";
  import {
    pushCommand,
    clearHistory,
    undo,
    redo,
    canUndo,
    canRedo,
  } from "./stores/commandStore";
  import { createNewScene, saveScene, addEntity } from "./api/scene";
  import UnsavedDialog, {
    type UnsavedChoice,
  } from "./UnsavedDialog.svelte";
  import { unsavedWork, saveAllUnsaved } from "./stores/unsavedStore";
  import { promptForScenePath } from "./api/scenePathPrompt";
  import { openNewSceneTab } from "./stores/sceneTabsStore";
  import { loadScriptSources } from "./api/scene";
  import { resolveAnimSourcesInScene } from "./api/animation";
  import { resolveParticleSourcesInScene } from "./api/particle";
  import {
    listAssets,
    listPrefabs,
    readAssetBinary,
    showInFileManager,
  } from "./api/project";
  import { toggleDevtools, reloadEditorWindow } from "./api/editorWindow";
  import { isPlaying, isPaused, playReturn } from "./stores/playStore";
  import {
    resetLayout,
    layout,
    togglePanel,
    isPanelShown,
    packagePanels,
    PANEL_NAMES,
    ALL_BUILTIN_PANELS,
    type PanelId,
  } from "./stores/layoutStore";
  import { buildAndRun, closeInstances, instanceCount, MAX_INSTANCES } from "./stores/runStore";
  import { buildRunning } from "./stores/buildStore";
  import {
    showColliders,
    showProfiler,
    gizmoMode,
    viewMode,
  } from "./stores/viewportStore";
  import { isFileTexture } from "./prefabTextures";
  import { onMount, onDestroy } from "svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { packageSchema } from "./stores/packageSchemaStore";
  import type { MenuItemDescriptor, ToolbarAction } from "./api/packages";
  import { invoke } from "@tauri-apps/api/core";
  import { palette, themeMode, PALETTES } from "./stores/themeStore";
  import { LINKS } from "./links";
  import { openExternalUrl, openLogsFolder } from "./api/help";
  import BuildToolsDialog from "./BuildToolsDialog.svelte";
  import { BUILD_TOOLS_EVENT, openBuildToolsDialog } from "./api/buildTools";
  import { registerActions } from "./stores/actionStore";

  let {
    onsettings,
    oncomponents,
    onbuild,
    onspritesheet,
    onpackages,
    onnewrustpackage,
  }: {
    onsettings?: () => void;
    oncomponents?: () => void;
    onbuild?: () => void;
    onspritesheet?: () => void;
    onpackages?: () => void;
    onnewrustpackage?: () => void;
  } = $props();

  let openMenu = $state<string | null>(null);
  /** The open flyout inside a menu (View > Panels). */
  let subMenu = $state<string | null>(null);

  /** Every panel View > Panels can switch: the built-in ones, then packages'. */
  let panelList: PanelId[] = $derived([
    ...ALL_BUILTIN_PANELS,
    ...([...$packagePanels.keys()] as PanelId[]),
  ]);

  /** Menu items from packages, grouped by top-level menu path. */
  let packageMenuGroups = $derived.by(() => {
    if (!$packageSchema?.menu_items.length)
      return new Map<string, MenuItemDescriptor[]>();
    const groups = new Map<string, MenuItemDescriptor[]>();
    for (const item of $packageSchema.menu_items) {
      const slash = item.path.indexOf("/");
      const top = slash > 0 ? item.path.slice(0, slash) : item.path;
      if (!groups.has(top)) groups.set(top, []);
      groups.get(top)!.push(item);
    }
    return groups;
  });

  /** Toolbar actions from packages. */
  let packageToolbarActions: ToolbarAction[] = $derived(
    $packageSchema?.toolbar_actions ?? [],
  );

  // Listen for global Ctrl+S event from App.svelte
  function onGlobalSave() {
    handleSave();
  }
  function onGlobalDuplicate() {
    handleDuplicate();
  }
  function onGlobalPause() {
    handlePauseToggle();
  }
  function onGlobalStep() {
    handleStep();
  }
  function onGlobalStop() {
    if (get(isPlaying)) void handleStop();
  }
  // Help > Check Build Tools, also opened by a failed build's toast.
  let showBuildTools = $state(false);
  function onShowBuildTools() {
    showBuildTools = true;
  }
  onMount(() => {
    window.addEventListener(BUILD_TOOLS_EVENT, onShowBuildTools);
    return registerActions([
      { id: "help.check-build-tools", title: "Check Build Tools", category: "Help", run: openBuildToolsDialog },
    ]);
  });
  onDestroy(() => window.removeEventListener(BUILD_TOOLS_EVENT, onShowBuildTools));
  onMount(() => {
    window.addEventListener("editor-save", onGlobalSave);
    window.addEventListener("editor-duplicate", onGlobalDuplicate);
    window.addEventListener("editor-pause", onGlobalPause);
    window.addEventListener("editor-step", onGlobalStep);
    window.addEventListener("editor-stop", onGlobalStop);
  });
  onDestroy(() => {
    window.removeEventListener("editor-save", onGlobalSave);
    window.removeEventListener("editor-duplicate", onGlobalDuplicate);
    window.removeEventListener("editor-pause", onGlobalPause);
    window.removeEventListener("editor-step", onGlobalStep);
    window.removeEventListener("editor-stop", onGlobalStop);
  });

  function toggleMenu(name: string) {
    openMenu = openMenu === name ? null : name;
  }

  function closeMenus() {
    openMenu = null;
    subMenu = null;
  }

  // --- Unsaved-work prompt --------------------------------------------------
  //
  // Both of these throw work away, so both ask first — with Save as a real
  // option, not the two answers `confirm` allows.

  /** What the pending action is waiting to do, or null when nothing is. */
  let pendingAction = $state<null | {
    title: string;
    items: string[];
    run: () => void | Promise<void>;
  }>(null);

  /**
   * Run `action`, asking first if anything is unsaved.
   *
   * `unsavedWork` rather than the scene's `dirty` flag: closing a project
   * discards every open editor, and warning only about the scene meant a
   * modified script went out in silence.
   */
  function guarded(title: string, action: () => void | Promise<void>) {
    const items = get(unsavedWork);
    if (items.length === 0) {
      void action();
      return;
    }
    pendingAction = { title, items, run: action };
  }

  async function resolvePending(choice: UnsavedChoice) {
    const pending = pendingAction;
    pendingAction = null;
    if (!pending || choice === "cancel") return;
    if (choice === "save") {
      const failed = await saveAllUnsaved(promptForScenePath);
      if (failed.length > 0) {
        // Going ahead now would discard exactly what the user asked to keep.
        logError(`Not saved, so nothing was closed: ${failed.join(", ")}`);
        return;
      }
    }
    await pending.run();
  }

  function handleCloseProject() {
    closeMenus();
    guarded("Close project", () => {
      project.set(null);
      scene.set({ name: "Untitled Scene", entities: [] });
      scenePath.set(null);
      clearSelection();
      dirty.set(false);
      clearHistory();
    });
  }

  /**
   * File > New — a new scene in its own tab.
   *
   * Asks nothing, because nothing is discarded: the scene you were on stays
   * in its tab, unsaved edits and all. The prompt that used to be here dates
   * from when New *replaced* the open scene.
   */
  async function handleNew() {
    closeMenus();
    const s = await createNewScene();
    openNewSceneTab(s);
    clearSelection();
    clearHistory();
    logInfo("Created new scene");
  }

  /**
   * File > Open Scene - raises the in-app scene list, exactly as Ctrl+O does.
   *
   * It used to raise the OS file picker, which meant the menu item and the
   * shortcut opened two different dialogs under one name. The in-app one is
   * the better of the two anyway: it lists every scene in the project,
   * filters as you type, and marks the ones with unsaved work. Nothing is
   * lost by dropping the native picker - a scene outside the project's
   * `assets/` cannot resolve its own sprites or scripts.
   */
  function handleOpen() {
    closeMenus();
    window.dispatchEvent(new CustomEvent("editor-open-scene"));
  }

  /**
   * Menu items whose state lives in `App.svelte`.
   *
   * These were reachable only by shortcut or command palette, which means
   * only by someone who already knew they existed - a menu bar is where you
   * look to find out that a feature is there at all.
   */
  function fire(event: string) {
    closeMenus();
    window.dispatchEvent(new CustomEvent(event));
  }

  /** Help > Documentation: the site's front doors. */
  const DOC_LINKS = [
    { label: "jEngine Documentation", url: LINKS.site },
    { label: "Getting Started", url: LINKS.gettingStarted },
    { label: "Scripting API", url: LINKS.scriptingApi },
    { label: "Components Reference", url: LINKS.components },
    { label: "Packages", url: LINKS.packages },
  ];

  /** A web page, in the system browser. */
  function openLink(url: string) {
    closeMenus();
    openExternalUrl(url).catch((e) => logError(`Failed to open ${url}: ${e}`));
  }

  async function handleSave() {
    closeMenus();
    if (get(isPlaying)) {
      // The scene store follows the running game across `load_scene`
      // transitions, so a save here could write the scene you happen to be
      // playing over the one you were editing.
      logWarn("Stop play mode before saving the scene");
      return;
    }
    let path = get(scenePath);

    if (!path) {
      const proj = get(project);
      const p = await save({
        filters: [{ name: "Scene", extensions: ["scene"] }],
        defaultPath: proj ? proj.path + "/assets" : undefined,
      });
      if (!p) return;
      path = p;
      scenePath.set(path);
    }

    const currentScene = get(scene);
    const filename = path.replace(/^.*[\\/]/, "").replace(/\.scene$/i, "");
    if (currentScene.name === "Untitled Scene") {
      currentScene.name = filename;
      scene.set(currentScene);
    }
    try {
      await saveScene(path, currentScene);
      dirty.set(false);
      logInfo(`Saved scene: ${path}`);
      requestAssetRefresh();
    } catch (e) {
      logError(`Failed to save scene: ${e}`);
    }
  }

  async function handleAddEntity() {
    closeMenus();
    const currentScene = get(scene);
    const entityNum = currentScene.entities.length + 1;
    const name = `Entity_${entityNum}`;
    try {
      const before = structuredClone(currentScene);
      const s = await addEntity(currentScene, name);
      pushCommand(`Add entity "${name}"`, before, s);
      selectEntity(s.entities.length - 1);
      logInfo(`Added entity: ${name}`);
    } catch (e) {
      logError(`Failed to add entity: ${e}`);
    }
  }

  function handleUndo() {
    closeMenus();
    undo();
  }

  function handleRedo() {
    closeMenus();
    redo();
  }

  function handleDuplicate() {
    closeMenus();
    const idx = get(selectedEntity);
    if (idx < 0) return;
    const currentScene = get(scene);
    const source = currentScene.entities[idx];
    if (!source) return;
    const before = structuredClone(currentScene);
    const clone = structuredClone(source);
    clone.name = `${source.name}_copy`;
    currentScene.entities.splice(idx + 1, 0, clone);
    pushCommand(`Duplicate "${source.name}"`, before, currentScene);
    selectEntity(idx + 1);
    logInfo(`Duplicated entity: ${source.name}`);
  }

  async function handlePlay() {
    const proj = get(project);
    if (!proj) {
      logError("Open a project first before playing");
      return;
    }
    const currentScene = get(scene);
    // Remember where we started so Stop comes back here even if scripts
    // switch scenes mid-session (see `playReturn`).
    playReturn.set({
      scene: structuredClone(currentScene),
      path: get(scenePath),
      dirty: get(dirty),
    });
    try {
      // Inline `.anim` clip_source and `.particle` particle_source
      // references so the viewport world has fully-populated
      // SpriteAnimator.clips and ParticleEmitter fields when play
      // starts. Mutates a structured clone so we don't pollute the
      // edit-mode store with resolved data.
      const playScene = structuredClone(currentScene);
      try {
        const resolved = await resolveAnimSourcesInScene(
          proj.path,
          playScene,
          (entName, err) =>
            logError(`[anim] resolve clip_source on '${entName}': ${err}`),
        );
        const pResolved = await resolveParticleSourcesInScene(
          proj.path,
          playScene,
          (entName, err) =>
            logError(
              `[particle] resolve particle_source on '${entName}': ${err}`,
            ),
        );
        if (resolved > 0 || pResolved > 0) {
          // Push the resolved scene into the viewport world so the
          // SpriteAnimator / ParticleEmitter components carry their
          // inlined data into play mode.
          const wasmMod = await import(
            "./viewport-wasm/jengine_editor_viewport.js"
          );
          (wasmMod as any).viewport_update_scene(JSON.stringify(playScene));
          // Upload any textures the .anim / .particle files backfilled
          // onto sprites / emitters. Without this, the first run after
          // opening a scene shows blank quads for animator sprites and
          // particle emitters — the viewport's loadSceneTextures ran on
          // the original scene (texture_id missing) and only catches up
          // on subsequent plays once the resolved snapshot reaches the
          // scene store.
          const texIds = new Set<string>();
          for (const ent of (playScene as any).entities ?? []) {
            const t = ent?.components?.sprite?.texture_id;
            if (typeof t === "string" && t.length > 0) texIds.add(t);
            const pt = ent?.components?.particle_emitter?.texture_id;
            if (typeof pt === "string" && pt.length > 0) texIds.add(pt);
            const clips = ent?.components?.sprite_animator?.clips;
            if (clips) {
              for (const c of Object.values(clips) as any[]) {
                for (const f of c?.frames ?? []) {
                  if (
                    typeof f?.texture_id === "string" &&
                    f.texture_id.length > 0
                  )
                    texIds.add(f.texture_id);
                }
              }
            }
          }
          for (const texId of texIds) {
            if (!isFileTexture(texId)) continue;
            try {
              const bytes = await readAssetBinary(proj.path, texId);
              (wasmMod as any).viewport_load_texture(
                texId,
                new Uint8Array(bytes),
              );
            } catch {
              // non-fatal — viewport will log a missing-texture warning
            }
          }
          if (resolved > 0)
            logInfo(`[anim] resolved ${resolved} .anim reference(s)`);
          if (pResolved > 0)
            logInfo(`[particle] resolved ${pResolved} .particle reference(s)`);
        }
      } catch (e) {
        logError(`Failed to resolve .anim/.particle references: ${e}`);
      }
      const result = await loadScriptSources(proj.path, currentScene);
      // Show warnings for missing/unreadable scripts
      for (const warning of result.warnings) {
        logError(`Script warning: ${warning}`);
      }
      if (result.entries.length === 0 && result.warnings.length > 0) {
        logError("No scripts could be loaded — fix script paths and try again");
        return;
      }
      // Import viewport WASM play function
      const wasm = await import("./viewport-wasm/jengine_editor_viewport.js");

      // Pre-upload any `.behavior` graph JSONs referenced by entities
      // in the scene. The viewport's `viewport_play` reads from a
      // pending_behaviors map populated by `viewport_load_behavior`,
      // so this MUST happen before `viewport_play(...)` below.
      try {
        const decoder = new TextDecoder();
        const behaviorPaths = new Set<string>();
        for (const ent of currentScene.entities ?? []) {
          const path = (ent.components as any)?.behavior_graph?.graph_path;
          if (path) behaviorPaths.add(path);
        }
        // Also include graphs referenced only by prefabs. Entities
        // spawned at runtime (script `spawn_prefab`) carry the prefab's
        // `behavior_graph`; the viewport binds them per-frame from this
        // same `pending_behaviors` map, so their graph JSON must be
        // uploaded up-front even though no scene entity references it.
        try {
          const prefabs = await listPrefabs(proj.path);
          for (const p of prefabs) {
            const path = (p.data as Record<string, any>)?.components
              ?.behavior_graph?.graph_path;
            if (path) behaviorPaths.add(path);
          }
        } catch (e) {
          logError(`Failed to scan prefabs for behavior graphs: ${e}`);
        }
        for (const path of behaviorPaths) {
          try {
            const bytes = await readAssetBinary(proj.path, path);
            const json = decoder.decode(new Uint8Array(bytes));
            (wasm as any).viewport_load_behavior(path, json);
          } catch (e) {
            logError(`Failed to load behavior '${path}': ${e}`);
          }
        }
      } catch (e) {
        logError(`Failed to pre-load behavior graphs: ${e}`);
      }

      // Pre-populate `window.__jengine_play.prefab_scripts` BEFORE
      // `viewport_play` so the Lua `require()` shim can find sibling
      // modules during initial compile. Without this, the first play
      // of a scene whose script does `require('lua/nav')` (or similar)
      // fails with "module not found in prefab_scripts" — subsequent
      // plays only work because the dict from the previous run is still
      // on `window.__jengine_play`.
      let prefabsForPlay: Awaited<ReturnType<typeof listPrefabs>> = [];
      try {
        prefabsForPlay = await listPrefabs(proj.path);
        const w = window as unknown as Record<string, unknown>;
        if (!w.__jengine_play) w.__jengine_play = {};

        const prefabScripts: Record<string, string> = {};
        for (const p of prefabsForPlay) {
          const script = (p.data as Record<string, any>)?.components?.script;
          if (script?.source_path) {
            try {
              const bytes = await readAssetBinary(
                proj.path,
                script.source_path,
              );
              const source = new TextDecoder().decode(new Uint8Array(bytes));
              prefabScripts[script.source_path] = source;
              const norm = script.source_path.replace(/\\/g, "/");
              if (norm.startsWith("assets/")) {
                prefabScripts[norm.slice("assets/".length)] = source;
              } else {
                prefabScripts[`assets/${norm}`] = source;
              }
            } catch {
              // non-fatal
            }
          }
        }

        try {
          const assets = await listAssets(proj.path);
          for (const path of assets) {
            const norm = path.replace(/\\/g, "/");
            // `.luau` too: a module an `extends` names is as often one as not.
            if (!/\.(lua|luau|js)$/.test(norm)) continue;
            if (prefabScripts[norm] || prefabScripts[`assets/${norm}`])
              continue;
            try {
              const bytes = await readAssetBinary(proj.path, norm);
              const source = new TextDecoder().decode(new Uint8Array(bytes));
              prefabScripts[norm] = source;
              prefabScripts[`assets/${norm}`] = source;
            } catch {
              // non-fatal
            }
          }
        } catch {
          // non-fatal
        }

        (w.__jengine_play as Record<string, unknown>).prefab_scripts =
          prefabScripts;
      } catch (e) {
        logError(`Failed to pre-load scripts for play mode: ${e}`);
      }

      // Data-oriented design — pre-populate `window.__jengine_play.data_tables`
      // so the Lua/JS `get_data(table, id, field)` global can read authored
      // `.data` tables in Play mode. Keyed by the table path relative to
      // `assets/` with the `.data` extension stripped (matches the ref
      // `table` values and the native registry ids).
      try {
        const w = window as unknown as Record<string, unknown>;
        if (!w.__jengine_play) w.__jengine_play = {};
        const dataTables: Record<string, unknown> = {};
        const assets = await listAssets(proj.path);
        for (const path of assets) {
          const norm = path.replace(/\\/g, "/");
          if (!norm.endsWith(".data")) continue;
          try {
            const bytes = await readAssetBinary(proj.path, norm);
            const parsed = JSON.parse(
              new TextDecoder().decode(new Uint8Array(bytes)),
            ) as { rows?: unknown };
            const key = norm.replace(/^assets\//, "").replace(/\.data$/, "");
            dataTables[key] = {
              rows: Array.isArray(parsed.rows) ? parsed.rows : [],
            };
          } catch {
            // non-fatal
          }
        }
        (w.__jengine_play as Record<string, unknown>).data_tables = dataTables;
      } catch (e) {
        logError(`Failed to pre-load data tables for play mode: ${e}`);
      }

      // Slice 8 — pre-upload top-level `audio_refs` audio clips into
      // the viewport's Web Audio backend. The scene's SceneData type
      // doesn't carry `audio_refs` (it's authored only on the JSON
      // file), so we re-read the scene file from disk to pick it up.
      try {
        const scenePathStr = get(scenePath);
        if (scenePathStr) {
          // Project-relative path for readAssetBinary: strip the
          // project root prefix if present, and strip the leading
          // `assets/` because read_asset_binary already prepends it.
          let rel = scenePathStr.replace(/\\/g, "/");
          const root = proj.path.replace(/\\/g, "/").replace(/\/$/, "");
          if (rel.startsWith(root + "/")) rel = rel.slice(root.length + 1);
          if (rel.startsWith("assets/")) rel = rel.slice("assets/".length);
          try {
            const sceneBytes = await readAssetBinary(proj.path, rel);
            const sceneJson = JSON.parse(
              new TextDecoder().decode(new Uint8Array(sceneBytes)),
            ) as { audio_refs?: string[] };
            const refs = Array.isArray(sceneJson.audio_refs)
              ? sceneJson.audio_refs
              : [];
            for (const clipPath of refs) {
              try {
                const bytes = await readAssetBinary(proj.path, clipPath);
                (wasm as any).viewport_load_audio(
                  clipPath,
                  new Uint8Array(bytes),
                );
              } catch (e) {
                logError(`Failed to load audio clip '${clipPath}': ${e}`);
              }
            }
            if (refs.length > 0) {
              logInfo(`[audio] uploaded ${refs.length} clip(s) to viewport`);
            }
          } catch (e) {
            logError(`Failed to read scene for audio_refs: ${e}`);
          }
        }
      } catch (e) {
        logError(`Failed to pre-load audio_refs: ${e}`);
      }

      const actionsJson = JSON.stringify(proj.settings.input_actions ?? []);
      const gravityX = proj.settings.physics?.gravity_x ?? 0;
      const gravityY = proj.settings.physics?.gravity_y ?? -9.8;
      const lengthUnit = proj.settings.physics?.length_unit ?? 1.0;
      const physicsBackend = proj.settings.physics?.backend ?? "rapier2d";
      // Seconds per fixed physics step (the Project Settings field); undefined leaves 1/60.
      const fixedTimestep = proj.settings.physics?.fixed_timestep;
      wasm.viewport_play(
        JSON.stringify(result.entries),
        actionsJson,
        gravityX,
        gravityY,
        lengthUnit,
        physicsBackend,
        fixedTimestep,
      );
      // Inject prefabs into the play state so scripts can call spawn_prefab()
      try {
        const prefabMap: Record<string, unknown> = {};
        for (const p of prefabsForPlay) {
          prefabMap[p.name] = p.data;
        }
        const w = window as unknown as Record<string, unknown>;
        if (!w.__jengine_play) w.__jengine_play = {};
        (w.__jengine_play as Record<string, unknown>).prefabs = prefabMap;

        // Pre-load textures referenced by prefabs so spawned entities render
        const { viewport_load_texture } = await import(
          "./viewport-wasm/jengine_editor_viewport.js"
        );
        for (const p of prefabsForPlay) {
          const texId = (p.data as Record<string, any>)?.components?.sprite
            ?.texture_id;
          if (isFileTexture(texId)) {
            try {
              const bytes = await readAssetBinary(proj.path, texId);
              viewport_load_texture(texId, new Uint8Array(bytes));
            } catch {
              // texture may already be loaded or missing — non-fatal
            }
          }
        }
      } catch (e) {
        logError(`Failed to load prefabs for play mode: ${e}`);
      }
      isPlaying.set(true);
      isPaused.set(false);
      viewMode.set("game");
      logInfo("Play mode started");
    } catch (e) {
      logError(`Failed to start play mode: ${e}`);
    }
  }

  /** Freeze / resume the running world (Unity's Pause). */
  async function handlePauseToggle() {
    try {
      // Stopped at a breakpoint: going on is the debugger's continue, so the
      // stopped script finishes before the world runs again.
      if (debuggerHoldsPause()) {
        luaDebugger.continue();
        return;
      }
      const wasm = await import("./viewport-wasm/jengine_editor_viewport.js");
      if (!wasm.viewport_is_playing()) return;
      const next = !get(isPaused);
      wasm.viewport_set_paused(next);
      isPaused.set(wasm.viewport_is_paused());
      logInfo(next ? "Play mode paused" : "Play mode resumed");
    } catch (e) {
      logError(`Failed to toggle pause: ${e}`);
    }
  }

  /** Advance one frame while paused (Unity's Step). */
  async function handleStep() {
    try {
      const wasm = await import("./viewport-wasm/jengine_editor_viewport.js");
      if (!wasm.viewport_is_playing()) return;
      if (!wasm.viewport_is_paused()) {
        // Stepping from a running world implies pausing it first, which
        // is what the button offers to do.
        wasm.viewport_set_paused(true);
        isPaused.set(true);
      }
      wasm.viewport_step();
    } catch (e) {
      logError(`Failed to step: ${e}`);
    }
  }

  /**
   * Put the editor back on the scene play mode started from.
   *
   * Preferred over the snapshot `viewport_stop()` returns: the viewport
   * re-snapshots on every in-play scene transition, so its payload is the
   * last scene played, and it carries the inlined `.anim` / `.particle`
   * data play mode needs rather than the authored source refs. Falls back
   * to the viewport payload only when there is no captured return point.
   */
  function restorePlayReturn(fallbackJson: string | null) {
    isPaused.set(false);
    // A script stopped at a breakpoint is dropped with the session.
    luaDebugger.reset();
    const ret = get(playReturn);
    playReturn.set(null);
    if (ret) {
      scene.set(ret.scene);
      scenePath.set(ret.path);
      dirty.set(ret.dirty);
      // The selection index came from the scene we are restoring, but a
      // click during play may have moved it past that scene's entities.
      if (get(selectedEntity) >= ret.scene.entities.length) clearSelection();
      return ret.path;
    }
    if (fallbackJson) {
      try {
        scene.set(JSON.parse(fallbackJson));
      } catch (e) {
        logError(`Failed to parse restored scene: ${e}`);
      }
    }
    return null;
  }

  async function handleStop() {
    try {
      const wasm = await import("./viewport-wasm/jengine_editor_viewport.js");
      // Defensive: the viewport WASM module reinitialises its `APP`
      // RefCell whenever `viewport_init` is called (canvas remount,
      // HMR, etc.), which clears `play_state` while the frontend
      // `isPlaying` store still says true. In that case
      // `viewport_stop()` would throw "not playing" and leave the UI
      // wedged in play mode. Detect the desync and recover quietly.
      if (!wasm.viewport_is_playing()) {
        restorePlayReturn(null);
        isPlaying.set(false);
        viewMode.set("scene");
        logInfo(
          "Play mode already stopped (viewport was reloaded) — UI state reset",
        );
        return;
      }
      const restoredJson = wasm.viewport_stop();
      isPlaying.set(false);
      viewMode.set("scene");
      const path = restorePlayReturn(restoredJson);
      logInfo(
        path
          ? `Play mode stopped — back on ${path.split(/[\\/]/).pop()}`
          : "Play mode stopped — scene restored",
      );
    } catch (e) {
      // Last-resort recovery — even if viewport_stop blew up, force
      // the frontend out of play mode so the user isn't stuck.
      restorePlayReturn(null);
      isPlaying.set(false);
      viewMode.set("scene");
      logError(`Failed to stop play mode: ${e} (UI state forced to stopped)`);
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
{#if openMenu}
  <div
    class="menu-backdrop"
    onclick={closeMenus}
    oncontextmenu={(e) => {
      e.preventDefault();
      closeMenus();
    }}
  ></div>
{/if}

<!-- The toolbar is the window's title bar: its empty space and the brand drag
     the window (double-click maximizes), and its right end holds the window's
     buttons. On macOS the traffic lights sit over its left end instead. -->
<div class="toolbar" class:mac={isMac} role="toolbar" aria-label="Main toolbar" data-tauri-drag-region>
  <div class="brand" aria-label="jEngine" data-tauri-drag-region>
    <span class="brand-sun" aria-hidden="true"></span>
    <span class="brand-word">j<em>Engine</em></span>
  </div>

  <!-- File menu -->
  <div class="menu-wrapper">
    <button
      class="menu-trigger"
      class:active={openMenu === "file"}
      onclick={() => toggleMenu("file")}
      aria-haspopup="true"
      aria-expanded={openMenu === "file"}>File</button
    >
    {#if openMenu === "file"}
      <div class="menu-dropdown" role="menu">
        <button role="menuitem" onclick={handleNew}>
          <span class="menu-label">New Scene</span>
        </button>
        <button role="menuitem" onclick={handleOpen}>
          <span class="menu-label">Open Scene</span>
          <span class="menu-shortcut">Ctrl+O</span>
        </button>
        <button role="menuitem" onclick={() => fire("editor-quick-open")}>
          <span class="menu-label">Go to File…</span>
          <span class="menu-shortcut">Ctrl+P</span>
        </button>
        <button role="menuitem" onclick={handleSave} disabled={$isPlaying}>
          <span class="menu-label">Save Scene</span>
          <span class="menu-shortcut">Ctrl+S</span>
        </button>
        <div class="menu-divider"></div>
        <button
          role="menuitem"
          onclick={() => {
            closeMenus();
            onnewrustpackage?.();
          }}
        >
          <span class="menu-label">New Rust Package…</span>
        </button>
        <div class="menu-divider"></div>
        <button role="menuitem" onclick={handleCloseProject}>
          <span class="menu-label">Close Project</span>
        </button>
      </div>
    {/if}
  </div>

  <!-- Edit menu -->
  <div class="menu-wrapper">
    <button
      class="menu-trigger"
      class:active={openMenu === "edit"}
      onclick={() => toggleMenu("edit")}
      aria-haspopup="true"
      aria-expanded={openMenu === "edit"}>Edit</button
    >
    {#if openMenu === "edit"}
      <div class="menu-dropdown" role="menu">
        <button role="menuitem" onclick={handleUndo} disabled={!$canUndo}>
          <span class="menu-label">Undo</span>
          <span class="menu-shortcut">Ctrl+Z</span>
        </button>
        <button role="menuitem" onclick={handleRedo} disabled={!$canRedo}>
          <span class="menu-label">Redo</span>
          <span class="menu-shortcut">Ctrl+Y</span>
        </button>
        <button role="menuitem" onclick={() => fire("editor-rename")}>
          <span class="menu-label">Rename…</span>
          <span class="menu-shortcut">F2</span>
        </button>
        <div class="menu-divider"></div>
        <button role="menuitem" onclick={() => fire("editor-find-in-files")}>
          <span class="menu-label">Find in Files…</span>
          <span class="menu-shortcut">Ctrl+Shift+F</span>
        </button>
        <button role="menuitem" onclick={() => fire("editor-replace-in-files")}>
          <span class="menu-label">Replace in Files…</span>
          <span class="menu-shortcut">Ctrl+Shift+H</span>
        </button>
        <div class="menu-divider"></div>
        <button role="menuitem" onclick={handleAddEntity} disabled={$isPlaying}>
          <span class="menu-label">Add Entity</span>
        </button>
        <button
          role="menuitem"
          onclick={handleDuplicate}
          disabled={$isPlaying || $selectedEntity < 0}
        >
          <span class="menu-label">Duplicate Entity</span>
          <span class="menu-shortcut">Ctrl+D</span>
        </button>
        <div class="menu-divider"></div>
        <button
          role="menuitem"
          onclick={() => {
            closeMenus();
            oncomponents?.();
          }}
        >
          <span class="menu-label">Components…</span>
        </button>
        <button
          role="menuitem"
          onclick={() => {
            closeMenus();
            onsettings?.();
          }}
        >
          <span class="menu-label">Project Settings…</span>
        </button>
        <button
          role="menuitem"
          onclick={() => {
            closeMenus();
            onpackages?.();
          }}
        >
          <span class="menu-label">Packages…</span>
        </button>
        <div class="menu-divider"></div>
        <button
          role="menuitem"
          onclick={() => {
            closeMenus();
            resetLayout();
          }}
        >
          <span class="menu-label">Reset Layout</span>
        </button>
      </div>
    {/if}
  </div>

  <!-- View menu -->
  <div class="menu-wrapper">
    <button
      class="menu-trigger"
      class:active={openMenu === "view"}
      onclick={() => toggleMenu("view")}
      aria-haspopup="true"
      aria-expanded={openMenu === "view"}>View</button
    >
    {#if openMenu === "view"}
      <div class="menu-dropdown" role="menu">
        <button role="menuitem" onclick={() => fire("editor-command-palette")}>
          <span class="menu-check"></span>
          <span class="menu-label">Command Palette…</span>
          <span class="menu-shortcut">Ctrl+Shift+A</span>
        </button>
        <!-- Which dock panels are shown. Stays open, so several can be
             switched at once; a switched-off panel comes back here, from the
             command palette, or when something needs it (a build, a search). -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="menu-sub"
          onmouseenter={() => (subMenu = "panels")}
          onmouseleave={() => (subMenu = null)}
        >
          <button
            role="menuitem"
            aria-haspopup="true"
            aria-expanded={subMenu === "panels"}
            class:active={subMenu === "panels"}
            onclick={() => (subMenu = subMenu === "panels" ? null : "panels")}
          >
            <span class="menu-check"></span>
            <span class="menu-label">Panels</span>
            <span class="menu-shortcut">›</span>
          </button>
          {#if subMenu === "panels"}
            <div class="menu-dropdown menu-flyout" role="menu">
              {#each panelList as panel (panel)}
                <button
                  role="menuitemcheckbox"
                  aria-checked={isPanelShown($layout, panel)}
                  onclick={() => togglePanel(panel)}
                >
                  <span class="menu-check">{isPanelShown($layout, panel) ? "✓" : ""}</span>
                  <span class="menu-label">{PANEL_NAMES[panel]}</span>
                </button>
              {/each}
              <div class="menu-divider"></div>
              <button
                role="menuitem"
                onclick={() => {
                  closeMenus();
                  resetLayout();
                }}
              >
                <span class="menu-check"></span>
                <span class="menu-label">Reset Layout</span>
              </button>
            </div>
          {/if}
        </div>
        <div class="menu-divider"></div>
        <button
          role="menuitem"
          onclick={() => {
            showColliders.update((v) => !v);
            closeMenus();
          }}
        >
          <span class="menu-check">{$showColliders ? "✓" : ""}</span>
          <span class="menu-label">Collider Wireframes</span>
          <span class="menu-shortcut">C</span>
        </button>
        <button
          role="menuitem"
          onclick={() => {
            showProfiler.update((v) => !v);
            closeMenus();
          }}
        >
          <span class="menu-check">{$showProfiler ? "✓" : ""}</span>
          <span class="menu-label">Profiler Overlay</span>
          <span class="menu-shortcut">F3</span>
        </button>
        <div class="menu-divider"></div>
        <button
          role="menuitem"
          onclick={() => {
            gizmoMode.set("move");
            closeMenus();
          }}
        >
          <span class="menu-check">{$gizmoMode === "move" ? "✓" : ""}</span>
          <span class="menu-label">Move Tool</span>
          <span class="menu-shortcut">W</span>
        </button>
        <button
          role="menuitem"
          onclick={() => {
            gizmoMode.set("rotate");
            closeMenus();
          }}
        >
          <span class="menu-check">{$gizmoMode === "rotate" ? "✓" : ""}</span>
          <span class="menu-label">Rotate Tool</span>
          <span class="menu-shortcut">E</span>
        </button>
        <button
          role="menuitem"
          onclick={() => {
            gizmoMode.set("scale");
            closeMenus();
          }}
        >
          <span class="menu-check">{$gizmoMode === "scale" ? "✓" : ""}</span>
          <span class="menu-label">Scale Tool</span>
          <span class="menu-shortcut">R</span>
        </button>
        <div class="menu-heading">Theme</div>
        {#each PALETTES as p}
          <button
            role="menuitemradio"
            aria-checked={$palette === p.id}
            onclick={() => {
              palette.set(p.id);
              closeMenus();
            }}
          >
            <span class="menu-check">{$palette === p.id ? "✓" : ""}</span>
            <span class="menu-label">{p.label}</span>
            <span class="theme-swatch" data-swatch={p.id} aria-hidden="true"></span>
          </button>
        {/each}
        <button
          role="menuitemcheckbox"
          aria-checked={$themeMode === "light"}
          onclick={() => {
            themeMode.update((m) => (m === "light" ? "dark" : "light"));
            closeMenus();
          }}
        >
          <span class="menu-check">{$themeMode === "light" ? "✓" : ""}</span>
          <span class="menu-label">Light Mode</span>
        </button>
      </div>
    {/if}
  </div>

  <!-- Build menu -->
  <div class="menu-wrapper">
    <button
      class="menu-trigger"
      class:active={openMenu === "build"}
      onclick={() => toggleMenu("build")}
      aria-haspopup="true"
      aria-expanded={openMenu === "build"}>Build</button
    >
    {#if openMenu === "build"}
      <div class="menu-dropdown" role="menu">
        <!-- Build & Run: the native game, built and started straight from the
             project folder - as many copies as picked below, side by side,
             each with its own saves (stores/runStore.ts). -->
        <button
          role="menuitem"
          onclick={() => {
            closeMenus();
            void buildAndRun(false);
          }}
          disabled={$buildRunning}
          title="Compile the native game (release) and start it at the project's start scene"
        >
          <span class="menu-label">Build &amp; Run</span>
          <span class="menu-shortcut">Ctrl+B</span>
        </button>
        <button
          role="menuitem"
          onclick={() => {
            closeMenus();
            void buildAndRun(true);
          }}
          disabled={$buildRunning || !$scenePath}
          title="The same, starting at the scene open in the editor"
        >
          <span class="menu-label">Build &amp; Run Current Scene</span>
          <span class="menu-shortcut">Ctrl+Shift+B</span>
        </button>
        <button
          role="menuitem"
          onclick={() => {
            closeMenus();
            void closeInstances();
          }}
        >
          <span class="menu-label">Close Running Instances</span>
          <span class="menu-shortcut">Ctrl+J</span>
        </button>
        <div class="menu-heading">Launched Instance Count</div>
        {#each Array.from({ length: MAX_INSTANCES }, (_, i) => i + 1) as n}
          <button
            role="menuitemradio"
            aria-checked={$instanceCount === n}
            onclick={() => {
              instanceCount.set(n);
              closeMenus();
            }}
          >
            <span class="menu-check">{$instanceCount === n ? "✓" : ""}</span>
            <span class="menu-label">{n} Instance{n === 1 ? "" : "s"}</span>
          </button>
        {/each}
        <div class="menu-divider"></div>
        <button
          role="menuitem"
          onclick={() => {
            closeMenus();
            onbuild?.();
          }}
          disabled={$isPlaying}
        >
          <span class="menu-label">Build Settings…</span>
        </button>
        <div class="menu-divider"></div>
        <button
          role="menuitem"
          onclick={() => {
            closeMenus();
            onspritesheet?.();
          }}
        >
          <span class="menu-label">Spritesheet Editor…</span>
        </button>
      </div>
    {/if}
  </div>

  <!-- Debug menu — the window-level actions WebView2 used to own -->
  <div class="menu-wrapper">
    <button
      class="menu-trigger"
      class:active={openMenu === "debug"}
      onclick={() => toggleMenu("debug")}
      aria-haspopup="true"
      aria-expanded={openMenu === "debug"}>Debug</button
    >
    {#if openMenu === "debug"}
      <div class="menu-dropdown" role="menu">
        <button
          role="menuitem"
          onclick={() => {
            closeMenus();
            reloadEditorWindow();
          }}
        >
          <span class="menu-label">Reload Editor Window</span>
          <span class="menu-shortcut">Ctrl+R</span>
        </button>
        <button
          role="menuitem"
          onclick={() => {
            closeMenus();
            void toggleDevtools().catch((e) => logError(`Devtools: ${e}`));
          }}
        >
          <span class="menu-label">Toggle Developer Tools</span>
          <span class="menu-shortcut">F12</span>
        </button>
        <div class="menu-divider"></div>
        <button
          role="menuitem"
          onclick={() => {
            showProfiler.update((v) => !v);
            closeMenus();
          }}
        >
          <span class="menu-check">{$showProfiler ? "✓" : ""}</span>
          <span class="menu-label">Profiler Overlay</span>
          <span class="menu-shortcut">F3</span>
        </button>
        <button
          role="menuitem"
          onclick={() => {
            clearLogs();
            closeMenus();
            logInfo("Console cleared");
          }}
        >
          <span class="menu-label">Clear Console</span>
        </button>
        <div class="menu-divider"></div>
        <button
          role="menuitem"
          disabled={!$project}
          onclick={() => {
            closeMenus();
            const proj = get(project);
            if (proj) {
              showInFileManager(proj.path, "").catch((e) =>
                logError(`Failed to open project folder: ${e}`),
              );
            }
          }}
        >
          <span class="menu-label">Reveal Project Folder</span>
        </button>
      </div>
    {/if}
  </div>

  <!-- Help menu: the documentation, the shortcuts, where to report a bug,
       and what this build is. Every address is in links.ts. -->
  <div class="menu-wrapper">
    <button
      class="menu-trigger"
      class:active={openMenu === "help"}
      onclick={() => toggleMenu("help")}
      aria-haspopup="true"
      aria-expanded={openMenu === "help"}>Help</button
    >
    {#if openMenu === "help"}
      <div class="menu-dropdown" role="menu">
        <div class="menu-heading">Documentation</div>
        {#each DOC_LINKS as doc (doc.url)}
          <button role="menuitem" onclick={() => openLink(doc.url)} title={doc.url}>
            <span class="menu-label">{doc.label}</span>
            <span class="menu-shortcut">↗</span>
          </button>
        {/each}
        <div class="menu-divider"></div>
        <button role="menuitem" onclick={() => fire("editor-shortcuts")}>
          <span class="menu-label">Keyboard Shortcuts…</span>
          <span class="menu-shortcut">F1</span>
        </button>
        <div class="menu-divider"></div>
        <button role="menuitem" onclick={() => openLink(LINKS.releaseNotes)} title={LINKS.releaseNotes}>
          <span class="menu-label">Release Notes</span>
          <span class="menu-shortcut">↗</span>
        </button>
        <button role="menuitem" onclick={() => openLink(LINKS.reportIssue)} title={LINKS.reportIssue}>
          <span class="menu-label">Report an Issue</span>
          <span class="menu-shortcut">↗</span>
        </button>
        <button
          role="menuitem"
          onclick={() => {
            closeMenus();
            openLogsFolder().catch((e) => logError(`Failed to open the logs folder: ${e}`));
          }}
        >
          <span class="menu-label">Open Logs Folder</span>
        </button>
        <button role="menuitem" onclick={() => fire(BUILD_TOOLS_EVENT)}>
          <span class="menu-label">Check Build Tools…</span>
        </button>
        <div class="menu-divider"></div>
        <button role="menuitem" onclick={() => fire("editor-licenses")}>
          <span class="menu-label">Third-Party Licenses…</span>
        </button>
        <button role="menuitem" onclick={() => fire("editor-about")}>
          <span class="menu-label">About jEngine</span>
        </button>
      </div>
    {/if}
  </div>

  <!-- Package menus (dynamic from packages) -->
  {#each [...packageMenuGroups] as [menuName, items]}
    <div class="menu-wrapper">
      <button
        class="menu-trigger"
        class:active={openMenu === `pkg_${menuName}`}
        onclick={() => toggleMenu(`pkg_${menuName}`)}
        aria-haspopup="true"
        aria-expanded={openMenu === `pkg_${menuName}`}>{menuName}</button
      >
      {#if openMenu === `pkg_${menuName}`}
        <div class="menu-dropdown" role="menu">
          {#each items as item}
            <button
              role="menuitem"
              onclick={() => {
                closeMenus();
                invoke(item.command).catch(() => {});
              }}
            >
              <span class="menu-label"
                >{item.path.includes("/")
                  ? item.path.slice(item.path.indexOf("/") + 1)
                  : item.path}</span
              >
              {#if item.shortcut}
                <span class="menu-shortcut">{item.shortcut}</span>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/each}

  <!-- Package toolbar actions -->
  {#if packageToolbarActions.length > 0}
    <div class="separator"></div>
    {#each packageToolbarActions as action}
      <button
        class="toolbar-btn"
        title={action.label}
        onclick={() => invoke(action.command).catch(() => {})}
        >{action.icon}</button
      >
    {/each}
  {/if}

  <!-- Spacer pushes playback controls to center -->
  <div class="spacer" data-tauri-drag-region></div>

  <!-- View mode toggle -->
  <div class="view-mode-group">
    <button
      class="view-mode-btn"
      class:active={$viewMode === "scene"}
      onclick={() => viewMode.set("scene")}
      title="Scene View — editor camera with gizmos">Scene</button
    >
    <button
      class="view-mode-btn"
      class:active={$viewMode === "game"}
      onclick={() => viewMode.set("game")}
      title="Game View — render from active game camera">Game</button
    >
  </div>

  <div class="separator"></div>

  <!-- Play / Pause / Step / Stop (always visible) -->
  <div class="play-group">
  {#if $isPlaying}
    <button
      onclick={handlePauseToggle}
      title={$isPaused
        ? "Resume play mode (Ctrl+Shift+P)"
        : "Pause play mode — the world freezes, editing stays live (Ctrl+Shift+P)"}
      class="pause-btn"
      class:active={$isPaused}
      >{#if $isPaused}<svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M2 1 L9 5 L2 9 Z" fill="currentColor"/></svg>Resume{:else}<svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><rect x="2" y="1.5" width="2" height="7" fill="currentColor"/><rect x="6" y="1.5" width="2" height="7" fill="currentColor"/></svg>Pause{/if}</button
    >
    <button
      onclick={handleStep}
      title="Advance one frame (Ctrl+Alt+P)"
      class="step-btn"
      aria-label="Step one frame"
      ><svg width="11" height="10" viewBox="0 0 11 10" aria-hidden="true"><path d="M1.5 1.5 L7 5 L1.5 8.5 Z" fill="currentColor"/><rect x="8" y="1.5" width="1.6" height="7" fill="currentColor"/></svg></button
    >
    <button onclick={handleStop} title="Stop play mode (Shift+F5)" class="stop-btn"
      ><svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><rect x="1.5" y="1.5" width="7" height="7" rx="1" fill="currentColor"/></svg>Stop</button
    >
  {:else}
    <button
      onclick={handlePlay}
      title="Run scripts in viewport"
      class="play-btn"
      ><svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M2 1 L9 5 L2 9 Z" fill="currentColor"/></svg>Play</button
    >
  {/if}
  </div>

  <div class="spacer" data-tauri-drag-region></div>

  <WindowControls />
</div>

{#if showBuildTools}
  <BuildToolsDialog onclose={() => (showBuildTools = false)} />
{/if}

{#if pendingAction}
  <UnsavedDialog
    title={pendingAction.title}
    items={pendingAction.items}
    onchoose={resolvePending}
  />
{/if}

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: 1px;
    padding: 0 10px 0 12px;
    background: var(--ed-crust);
    height: 40px;
    flex-shrink: 0;
    position: relative;
    /* A title bar: dragging it moves the window, it never selects text. */
    user-select: none;
    -webkit-user-select: none;
  }

  /* Room for the macOS traffic lights, overlaid on the bar's left. */
  .toolbar.mac {
    padding-left: 80px;
  }

  /* The website's header rule: accent into a3 into a2. */
  .toolbar::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 2px;
    background: linear-gradient(90deg, var(--ed-accent), var(--ed-a3) 55%, var(--ed-a2));
    opacity: 0.85;
    pointer-events: none;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-right: 12px;
    margin-right: 4px;
  }

  /* A drag region drags only where it is the element under the pointer. */
  .brand > * {
    pointer-events: none;
  }

  /* The striped setting sun, as on the website. */
  .brand-sun {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: linear-gradient(180deg, var(--ed-sun1), var(--ed-sun2));
    -webkit-mask: linear-gradient(180deg, #000 52%, transparent 52% 59%, #000 59% 70%, transparent 70% 77%, #000 77% 85%, transparent 85% 90%, #000 90%);
    mask: linear-gradient(180deg, #000 52%, transparent 52% 59%, #000 59% 70%, transparent 70% 77%, #000 77% 85%, transparent 85% 90%, #000 90%);
  }

  .brand-word {
    font-family: var(--ed-display);
    font-size: 15px;
    line-height: 1;
    color: var(--ed-ink);
    letter-spacing: 0.01em;
  }

  .brand-word em {
    font-style: normal;
    color: var(--ed-accent);
  }

  .theme-swatch {
    width: 28px;
    height: 10px;
    border-radius: 2px;
    flex-shrink: 0;
  }
  .theme-swatch[data-swatch="dusk"] {
    background: linear-gradient(90deg, #1b0f2e 0 33%, #ff5d73 33% 66%, #3ee0c5 66%);
  }
  .theme-swatch[data-swatch="sabers"] {
    background: linear-gradient(90deg, #0c0a1f 0 33%, #ff2e88 33% 66%, #29e3f0 66%);
  }
  .theme-swatch[data-swatch="genom"] {
    background: linear-gradient(90deg, #070b12 0 33%, #00d0ff 33% 66%, #ff3b3b 66%);
  }

  .spacer {
    flex: 1;
  }

  /* ---- Menu triggers (File, Edit, Build) ---- */
  .menu-trigger {
    background: transparent;
    color: var(--ed-muted);
    border: none;
    border-radius: 4px;
    padding: 0 9px;
    height: 28px;
    cursor: pointer;
    font-size: 12.5px;
    font-family: inherit;
  }

  .menu-trigger:hover,
  .menu-trigger.active {
    background: var(--ed-hover);
    color: var(--ed-ink);
  }

  /* ---- Dropdown menus ---- */
  .menu-wrapper {
    position: relative;
  }

  .menu-backdrop {
    position: fixed;
    inset: 0;
    z-index: 998;
  }

  .menu-dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: 999;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 7px;
    padding: 5px;
    min-width: 220px;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
    margin-top: 4px;
  }

  .menu-dropdown button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    text-align: left;
    padding: 5px 10px;
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 12.5px;
    font-family: inherit;
    cursor: pointer;
    gap: 16px;
  }

  .menu-dropdown button:hover:not(:disabled) {
    background: var(--ed-accent-low);
    color: var(--ed-ink);
  }

  .menu-dropdown button:hover:not(:disabled) .menu-shortcut {
    color: var(--ed-accent-high);
  }

  .menu-dropdown button:disabled {
    color: var(--ed-faint);
    cursor: default;
  }

  .menu-label {
    flex: 1;
    white-space: nowrap;
  }

  .menu-shortcut {
    color: var(--ed-faint);
    font-family: var(--ed-mono);
    font-size: 10.5px;
  }

  .menu-heading {
    padding: 8px 10px 3px;
    font-family: var(--ed-mono);
    font-size: 10px;
    color: var(--ed-a3);
    text-transform: uppercase;
    letter-spacing: 0.14em;
    border-top: 1px solid var(--ed-line);
    margin-top: 4px;
  }

  .menu-check {
    width: 16px;
    font-size: 12px;
    color: var(--ed-ok);
    text-align: center;
    flex-shrink: 0;
  }

  .menu-divider {
    height: 1px;
    background: var(--ed-line);
    margin: 4px 6px;
  }

  .menu-sub {
    position: relative;
  }

  .menu-sub > button.active {
    background: var(--ed-accent-low);
    color: var(--ed-ink);
  }

  /* A flyout beside its row, flush with the menu's own padding. */
  .menu-flyout {
    top: -5px;
    left: 100%;
    margin-top: 0;
    min-width: 180px;
  }

  /* ---- Action buttons (Play/Stop) ---- */
  button {
    background: var(--ed-raised);
    color: var(--ed-text);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    padding: 4px 14px;
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
  }

  button:hover {
    background: var(--ed-line);
  }

  button:active {
    background: var(--ed-line-strong);
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .play-group {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px;
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-radius: 6px;
  }

  .play-group button {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 12px 0 10px;
    border: none;
    border-radius: 4px;
    background: none;
    color: var(--ed-muted);
    font-size: 11.5px;
    font-weight: 500;
  }

  .play-group button:hover {
    background: var(--ed-hover);
    color: var(--ed-ink);
  }

  .play-group .play-btn {
    background: var(--ed-ok);
    color: var(--ed-on-accent);
    font-weight: 600;
  }

  .play-group .play-btn:hover {
    background: var(--ed-ok);
    color: var(--ed-on-accent);
    filter: brightness(1.12);
  }

  .play-group .pause-btn.active {
    background: color-mix(in srgb, var(--ed-warn) 22%, transparent);
    color: var(--ed-warn);
  }

  .play-group .step-btn {
    padding: 0;
    width: 28px;
    justify-content: center;
  }

  .play-group .stop-btn {
    color: var(--ed-err);
  }

  .play-group .stop-btn:hover {
    background: color-mix(in srgb, var(--ed-err) 16%, transparent);
    color: var(--ed-err);
  }

  /* ---- View mode toggle ---- */
  .view-mode-group {
    display: flex;
    gap: 2px;
    padding: 2px;
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-radius: 6px;
  }

  .view-mode-btn {
    padding: 0 10px;
    height: 26px;
    font-size: 11.5px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--ed-muted);
    cursor: pointer;
    transition:
      background 0.15s,
      color 0.15s;
  }

  .view-mode-btn:hover {
    background: var(--ed-raised);
    color: var(--ed-text);
  }

  .view-mode-btn.active {
    background: var(--ed-raised);
    color: var(--ed-ink);
    font-weight: 500;
    box-shadow: inset 0 -2px 0 var(--ed-accent);
  }

  .separator {
    width: 1px;
    height: 18px;
    background: var(--ed-line);
    margin: 0 8px;
    align-self: center;
  }
</style>
