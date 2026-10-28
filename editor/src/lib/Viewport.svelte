<script lang="ts">
  import { profilerFrame } from "./stores/profilerStore";
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
  import { scene, dirty, scenePath } from "./stores/sceneStore";
  import { readTextFile } from "@tauri-apps/plugin-fs";
  import {
    selectedEntity,
    selectEntity,
    toggleEntity,
    selectEntities,
    selectedEntities,
    clearSelection,
  } from "./stores/selectionStore";
  import { project } from "./stores/projectStore";
  import { spriteLibraryTextures } from "./spriteLibrary";
  import { prefabTextureIds, isFileTexture } from "./prefabTextures";
  import { logInfo, logWarn, logError } from "./stores/consoleStore";
  import { isPlaying, isPaused } from "./stores/playStore";
  import {
    gizmoMode,
    gizmoSpace,
    showColliders,
    showJoints,
    showLighting,
    showProfiler,
    polygonEditEntity,
    polygonEditPath,
    snapSettings,
    scenePreview,
    viewMode,
    type GizmoMode,
    type GizmoSpace,
    type ViewMode,
  } from "./stores/viewportStore";
  import { runtimeEntity, runtimeEntities } from "./stores/runtimeStore";
  import { devicePreview } from "./stores/devicePreviewStore";
  import {
    fitRect,
    profileById,
    safeFractions,
    screenAspect,
    type Rect,
  } from "./devicePreview";
  import DevicePicker from "./DevicePicker.svelte";
  import DeviceOverlay from "./DeviceOverlay.svelte";
  import { execute, pushCommand } from "./stores/commandStore";
  import { readAssetBinary, listPrefabs, listAssets } from "./api/project";
  import { editThroughInstance, prefabKey, viewOf } from "./prefabInstance";
  import { gizmosFor, type Gizmo } from "./gizmoShapes";
  import { languageFor, localized } from "./localeEdit";
  import { localeTables, previewLanguage } from "./stores/localeStore";
  import { entitiesInBox, snapAngle, snapping, snapTo, topLevelSelection } from "./sceneEditing";
  import {
    colliderPointsPath,
    insertionIndex,
    pointListInfo,
    pointsToEdit,
    readPoints,
  } from "./pointFields";
  import { prefabLibrary, prefabsChanged, scenePrefabs } from "./stores/prefabLibraryStore";
  import { openEntryScene } from "./api/openAsset";
  import { isGameKey } from "./api/playKeys";
  import { loadScene, loadScriptSources } from "./api/scene";
  import type { SceneData } from "./api/scene";
  import { resolveAnimSourcesInScene } from "./api/animation";
  import { resolveParticleSourcesInScene } from "./api/particle";
  import { resolveTilemapSourcesInScene } from "./api/tilemap";
  import { ASSET_DRAG_MIME, createEntityFromAsset } from "./api/assetEntity";
  import init, {
    viewport_init,
    viewport_update_scene,
    viewport_set_camera,
    viewport_set_selected,
    viewport_set_clear_color,
    viewport_resize,
    viewport_render,
    viewport_get_logs,
    viewport_is_playing,
    viewport_get_entity_bounds,
    viewport_get_debug_draws,
    viewport_get_collider_bounds,
    viewport_load_texture,
    viewport_take_texture_requests,
    viewport_take_video_requests,
    viewport_load_video,
    viewport_load_gui,
    viewport_load_audio,
    viewport_load_atlas,
    viewport_load_font,
    viewport_load_font_file,
    viewport_font_texture_id,
    viewport_load_prefab,
    viewport_load_material,
    viewport_take_material_requests,
    viewport_load_shader_graph,
    viewport_load_sort_layers,
    viewport_set_active_sort_layers,
    viewport_get_frame_stats,
    viewport_get_frame_history,
    viewport_get_runtime_entity,
    viewport_get_runtime_entities,
    viewport_set_entity_transform,
    viewport_get_entity_world_transform,
    viewport_set_runtime_entity,
    viewport_render_pip,
    viewport_get_game_camera,
    viewport_set_camera_size,
    viewport_set_ui_scale,
    viewport_set_texture_filters,
    viewport_set_simulation,
    viewport_set_audio_mixer,
    viewport_set_asset_packs,
    viewport_set_liveops,
    viewport_set_safe_area,
    viewport_set_touch_preview,
    viewport_clear_game_viewport,
    viewport_get_pending_scene,
    viewport_apply_scene_transition,
    viewport_take_stream_requests,
    viewport_add_scene_chunk,
    viewport_set_stream_status,
    viewport_pop_play_scene,
    viewport_set_show_lighting,
  } from "./viewport-wasm/jengine_editor_viewport.js";
  // Read by name, not imported: a viewport built before a function existed
  // must still load (a missing named import fails the whole module).
  import * as viewportModule from "./viewport-wasm/jengine_editor_viewport.js";
  import { themeColors, type ThemeColors } from "./stores/themeColors";
  import { classifyViewportLog } from "./viewportLog";

  let canvasEl = $state<HTMLCanvasElement>(null!);

  // ── Device preview ────────────────────────────────────────────────────
  // In the Game view, a chosen phone letterboxes the canvas to its shape
  // and hands the engine its safe area, so the UI and the touch controls sit
  // where they would on the device. The Scene view always fills the panel.
  let deviceRect = $state<Rect | null>(null);
  const previewProfile = $derived(
    currentViewMode === "game" ? profileById($devicePreview.id) : null,
  );

  /** Size the canvas: the device's shape in the panel, or all of it. */
  function applyCanvasSize() {
    if (!canvasEl?.parentElement) return;
    const panel = canvasEl.parentElement.getBoundingClientRect();
    const pw = Math.floor(panel.width);
    const ph = Math.floor(panel.height);
    if (pw <= 0 || ph <= 0) return;
    const profile = currentViewMode === "game" ? profileById(get(devicePreview).id) : null;
    const portrait = get(devicePreview).portrait;
    let rect: Rect = { x: 0, y: 0, width: pw, height: ph };
    if (profile) {
      rect = fitRect(pw, ph, screenAspect(profile, portrait));
      canvasEl.style.left = `${rect.x}px`;
      canvasEl.style.top = `${rect.y}px`;
      canvasEl.style.width = `${rect.width}px`;
      canvasEl.style.height = `${rect.height}px`;
    } else {
      canvasEl.style.left = "";
      canvasEl.style.top = "";
      canvasEl.style.width = "";
      canvasEl.style.height = "";
    }
    deviceRect = profile ? rect : null;
    if (canvasEl.width !== rect.width || canvasEl.height !== rect.height) {
      canvasEl.width = rect.width;
      canvasEl.height = rect.height;
      if (wasmReady) viewport_resize(rect.width, rect.height);
    }
    if (wasmReady) {
      try {
        const f = profile
          ? safeFractions(profile, portrait)
          : { left: 0, top: 0, right: 0, bottom: 0 };
        viewport_set_safe_area(f.left, f.top, f.right, f.bottom);
        // A phone is a touch screen: show `show: "touch"` controls, and let
        // the mouse work them.
        viewport_set_touch_preview(profile !== null);
      } catch {
        // A viewport built before these existed - rebuild it.
      }
    }
  }
  let animFrameId = 0;
  let onWindowFKey: ((e: KeyboardEvent) => void) | null = null;
  let cameraX = $state(0);
  let cameraY = $state(0);
  let cameraZoom = $state(1);
  // Effective game-camera projection while in Game view (position + CSS px per
  // world unit), so overlays (debug draws) map world→screen with the same
  // square-pixel scale the renderer uses for the game's virtual resolution.
  let gamePlayCam: { x: number; y: number; scale: number } | null = null;
  let isPanning = false;
  let lastMouseX = 0;
  let lastMouseY = 0;
  let loaded = $state(false);
  let error = $state("");
  let wasmReady = $state(false);

  /** The palette's colour as the linear RGBA the renderer draws in. */
  function linearRgba(hex: string, alpha: number): number[] {
    const h = hex.trim().replace("#", "");
    const lin = (i: number) => {
      const c = parseInt(h.slice(i, i + 2), 16) / 255;
      return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    };
    return [lin(0), lin(2), lin(4), alpha];
  }

  /** The scene grid in the editor's palette: lines in `line-strong`, the X
   *  axis in the accent and Y in the second accent, as the gizmo draws them. */
  function sendGridColors(t: ThemeColors) {
    const set = (viewportModule as Record<string, unknown>).viewport_set_grid_colors;
    if (typeof set !== "function") return;
    try {
      set(
        new Float32Array([
          ...linearRgba(t["line-strong"], 0.55),
          ...linearRgba(t.accent, 0.8),
          ...linearRgba(t.a2, 0.8),
        ]),
      );
    } catch {
      /* a viewport from before this function: its own colours stand */
    }
  }

  $effect(() => {
    const t = $themeColors;
    if (wasmReady) sendGridColors(t);
  });
  const loadedTextures = new Set<string>();
  const loadedGuis = new Set<string>();
  // Sprite atlases pushed into the viewport for `set_sprite_region`.
  // Keyed by the sibling-PNG atlas id (e.g. `atlases/dungeon.png`).
  const loadedAtlases = new Set<string>();
  /** `.font` ids already forwarded to the viewport, so an edit that
   *  re-fires the scene store doesn't re-read them every keystroke. */
  const loadedFonts = new Set<string>();
  const loadedAudio = new Set<string>();
  // Slice 13d — track which .material / .shader assets we've pushed to
  // the viewport so we don't re-fetch on every scene-store update.
  const loadedMaterials = new Set<string>();
  const loadedShaderGraphs = new Set<string>();
  // Sort layers 1.4 — track which `.sortlayers` assets we've pushed
  // to the viewport so we don't re-fetch on every scene-store update.
  const loadedSortLayers = new Set<string>();
  let activeSortLayersId: string | null = null;
  /** Built-in material IDs handled inside the engine; never have a
   *  `.material` file on disk so we skip them in `loadSceneMaterials`. */
  // Material ids the ENGINE resolves itself — they name a built-in pipeline,
  // not a `.material` file, so trying to read one off disk is an error every
  // time. `lit_sprite` is the engine's own `LIT_SPRITE_MATERIAL_ID`
  // (ecs/wgpu_systems.rs): sprites tagged with it are routed to the dedicated
  // lit pipeline and never touch the asset manager. The entry here used to say
  // "lit", which matched nothing, so every scene using lit sprites logged
  // `Failed to load material "lit_sprite"` on load.
  const BUILTIN_MATERIAL_IDS = new Set([
    "default_sprite",
    "lit_sprite",
    "additive",
    "multiply",
    "screen",
    "emission",
    "outline",
    "scroll",
    "flash",
    "lit",
  ]);

  // --- Gizmo state ---
  let currentGizmoMode = $state<GizmoMode>("move");
  let currentGizmoSpace = $state<GizmoSpace>("global");
  let draggingGizmo = $state<"x" | "y" | "center" | "arc" | null>(null);
  let gizmoWorldRotation = $state(0); // world rotation of selected entity (for local space gizmo)
  let dragStartWorld: [number, number] = [0, 0];
  let dragStartParentWorldRot = 0; // parent's accumulated world rotation at drag start
  /** The other selected entities a gizmo drag carries along, each with its
   *  transform and its parent's world rotation when the drag began. Children
   *  of a selected entity are left out - they move with their parent. */
  let dragGroup: {
    idx: number;
    x: number;
    y: number;
    rotation: number;
    scaleX: number;
    scaleY: number;
    parentRot: number;
  }[] = [];
  /** A box-select drag: where it started (screen, canvas-relative) and where
   *  the pointer is. */
  let marquee = $state<{ x0: number; y0: number; x1: number; y1: number; add: boolean } | null>(null);
  let dragStartWorldRot = 0; // entity's world rotation at drag start
  let dragStartValue: {
    x: number;
    y: number;
    rotation: number;
    scaleX: number;
    scaleY: number;
  } = { x: 0, y: 0, rotation: 0, scaleX: 1, scaleY: 1 };
  let gizmoHover = $state<string | null>(null);

  // Gizmo screen positions (updated each frame)
  let gizmoScreenX = $state(0);
  let gizmoScreenY = $state(0);
  let gizmoVisible = $state(false);

  // --- Stats bar state ---
  let statsFps = $state(0);
  let statsFrameTime = $state(0);
  /// Wall time actually spent inside the render callback, averaged over the
  /// profiler interval.
  ///
  /// `statsFrameTime` is the gap between successive `requestAnimationFrame`
  /// callbacks, so it can never read below the display's refresh interval — on
  /// a 165 Hz monitor it sits at 6.00 ms no matter how little work is done, and
  /// the rest is the browser waiting for vsync. That is a frame *rate*, and it
  /// is easy to mistake for a frame *cost*: subtracting the engine's phases
  /// from it appears to show milliseconds of editor overhead that do not exist.
  ///
  /// This is the honest number for "what the editor costs". Subtracting the
  /// engine phase total from it leaves the editor's own per-frame work — the
  /// `viewport_get_*` calls, the JSON, the store writes — which is what
  /// `docs/architecture-debt.md` #7 is about and what nothing could previously
  /// measure.
  let statsCpuMs = $state(0);
  let cpuAccum = 0;
  let cpuFrames = 0;
  let statsDrawCalls = $state(0);
  // Reserved wasm linear memory of the viewport module, and its live-heap
  // high-water mark. Play mode in the editor, not the shipped game.
  let statsMemoryBytes = $state(0);
  let statsMemoryPeak = $state(0);
  // Live heap. Differs from `statsMemoryBytes` (reserved wasm linear
  // memory, which only grows) and is the one to watch for a leak.
  let statsMemoryLive = $state(0);
  let statsEntityCount = $state(0);
  let statsSpriteCount = $state(0);
  let statsPhysicsTime = $state(0);
  let statsScriptTime = $state(0);
  let statsRenderTime = $state(0);
  let statsUpdateTime = $state(0);
  let frameHistory = $state<number[]>([]);

  // Debug draw overlay state
  let debugDrawCommands = $state<
    Array<{
      t: string;
      x1?: number;
      y1?: number;
      x2?: number;
      y2?: number;
      x?: number;
      y?: number;
      w?: number;
      h?: number;
      rad?: number;
      r: number;
      g: number;
      b: number;
    }>
  >([]);

  // Collider wireframe overlay state
  type ColliderInfo = {
    index: number;
    shape: string;
    x: number;
    y: number;
    rotation: number;
    w?: number;
    h?: number;
    radius?: number;
    half_height?: number;
    a?: [number, number];
    b?: [number, number];
    c?: [number, number];
    vertices?: [number, number][];
    heights?: number[];
    scale?: [number, number];
    sample_count?: number;
  };
  let colliderBounds = $state<ColliderInfo[]>([]);

  // --- Polygon edit mode state ---
  let polyEditIdx = $state(-1); // entity index being edited (-1 = off)
  /** The scene's entities as they are - a linked instance through its prefab
   *  (`prefabInstance.ts`) - for everything here that reads a component. The
   *  writes go through `editEntityAt`, which keeps an instance an instance. */
  let viewEntities = $derived($scene.entities.map((e) => viewOf(e, $scenePrefabs)));
  /** The point list being edited, as a dotted path into the entity's
   *  components (`pointFields.ts`): the one the Inspector named, or the
   *  edited entity's collider polygon. Null when nothing is edited. */
  let polyPath: string | null = $derived(
    polyEditIdx < 0 || !viewEntities[polyEditIdx]
      ? null
      : ($polygonEditPath ??
          colliderPointsPath(viewEntities[polyEditIdx].components.collider?.shape)),
  );
  /** The selected entity's ranges and outlines (`gizmoShapes.ts`), in world
   *  units: light reach, spot cones, sound ranges, area boxes, nav regions,
   *  occluders, a camera's confine area. Edit mode only - in play the
   *  selection is a live entity. */
  let selectionGizmos: Gizmo[] = $derived.by(() => {
    const sel = $selectedEntity;
    if ($isPlaying || sel < 0 || !viewEntities[sel]) return [];
    const xf = worldTransformOf(sel);
    return xf ? gizmosFor(viewEntities[sel].components as Record<string, unknown>, xf) : [];
  });
  let polyInfo = $derived(
    polyPath ? pointListInfo(viewEntities[polyEditIdx].components as Record<string, unknown>, polyPath) : null,
  );
  let polyDragVertex = $state<number | null>(null); // index of vertex being dragged
  let polyHoverVertex = $state<number | null>(null); // index of hovered vertex
  let polyDragStartWorld: [number, number] = [0, 0];
  let polyDragStartVerts: [number, number][] = [];

  // --- Joint anchor drag state ---
  let jointDragAnchor = $state<{
    entityIdx: number;
    anchor: "a" | "b";
  } | null>(null);

  // --- Camera preview state ---
  // When the selected entity has a camera component, we show a preview rectangle.
  // Mirror scene + selection into $state-backed locals so the derived preview
  // re-evaluates whenever either changes (svelte/store subscribers do the
  // mirroring; we used to manually re-derive in `updateCameraPreview()` but
  // that didn't track editor-camera zoom changes, so the rect's screen
  // size scaled wrong relative to the rest of the scene).
  let sceneEntities = $state<any[]>([]);
  let selectedIdx = $state(-1);

  /**
   * Editor-side mirror of the viewport's play-mode scene stack.
   *
   * Scripts can `load_scene` / `push_scene` / `pop_scene` while playing.
   * The viewport swaps its world, but the Scene panel and Inspector read
   * the `scene` store — so without this they keep listing the entities of
   * whichever scene you pressed Play in. Each transition pushes the
   * outgoing `(scene, path)` here and points the store at the incoming
   * one; a pop walks it back, mirroring the WASM `scene_stack`.
   */
  let playSceneStack: { scene: SceneData; path: string | null }[] = [];

  /** Frames between live-entity-list polls (~6 = 10 Hz at 60 fps). */
  const RUNTIME_LIST_INTERVAL = 6;
  /// Milliseconds between profiler refreshes.
  ///
  /// The profiler used to re-read its stats and its 120-sample history on
  /// *every* frame, which at 167 fps meant parsing a fresh JSON array, writing
  /// two reactive stores, spreading 120 numbers into `Math.max`, and rebuilding
  /// a 120-point SVG polyline 167 times a second — so that a human could read
  /// numbers that change faster than they can be focused on. The engine keeps
  /// the history itself, so a slower poll loses no samples; it only draws the
  /// same data less often. Twelve times a second is past the point where the
  /// graph looks continuous and the numbers become legible rather than a blur.
  const PROFILER_INTERVAL_MS = 80;
  /** The stats chip's poll when the full profiler is off: fps, entities and
   *  draws read fine at 2 Hz, and the JSON round trip stays off most frames. */
  const STATS_CHIP_INTERVAL_MS = 500;
  let lastProfilerPoll = 0;
  let lastInspectorPoll = 0;
  let lastInspectorSel = -1;
  let runtimeListTick = 0;

  let cameraPreview = $derived.by(() => {
    if (selectedIdx < 0 || selectedIdx >= sceneEntities.length) return null;
    const ent = sceneEntities[selectedIdx];
    const cam = ent?.components?.camera;
    if (!cam) return null;
    const t = ent.components.transform ?? {
      position: [0, 0],
      rotation: 0,
      scale: [1, 1],
    };
    const cx = (t.position as [number, number])[0];
    const cy = (t.position as [number, number])[1];
    const zoom = cam.zoom || 1;
    // Camera2D::view_projection in core uses
    //   half_w = viewport_width / (2 * zoom)
    // so the world-space extent the camera covers is viewport_w / zoom.
    const halfW = cam.viewport_width / (2 * zoom);
    const halfH = cam.viewport_height / (2 * zoom);
    // Debug: log so we can verify scene values reach the overlay.
    // eslint-disable-next-line no-console
    console.debug(
      "[camera-preview] vw=%o vh=%o zoom=%o halfW=%o halfH=%o cx=%o cy=%o",
      cam.viewport_width,
      cam.viewport_height,
      zoom,
      halfW,
      halfH,
      cx,
      cy,
    );
    return { cx, cy, halfW, halfH, isActive: !!cam.is_active, zoom };
  });

  let unsubScene: (() => void) | null = null;
  let unsubPrefabs: (() => void) | null = null;
  let unsubPreview: (() => void) | null = null;
  let unsubLanguage: (() => void) | null = null;
  let unsubPlayData: (() => void) | undefined;
  let unsubSelection: (() => void) | null = null;
  let unsubProject: (() => void) | null = null;
  let unsubGizmoMode: (() => void) | null = null;
  let unsubGizmoSpace: (() => void) | null = null;
  let unsubPolyEdit: (() => void) | null = null;
  let unsubViewMode: (() => void) | null = null;
  let unsubDevicePreview: (() => void) | null = null;
  let unsubPlaying: (() => void) | null = null;
  let currentViewMode = $state<ViewMode>("scene");
  let resizeObserver: ResizeObserver | null = null;

  // --- Coordinate conversion ---
  /** The world point under the cursor, for the readout; null off the canvas. */
  let cursorWorld = $state<[number, number] | null>(null);

  function trackCursor(e: MouseEvent) {
    if (canvasEl) cursorWorld = screenToWorld(e.clientX, e.clientY);
  }

  function screenToWorld(screenX: number, screenY: number): [number, number] {
    const rect = canvasEl.getBoundingClientRect();
    // Game view projects through the game camera's virtual resolution (see
    // worldToLocal) — invert that mapping instead of the editor camera.
    if (currentViewMode === "game" && gamePlayCam) {
      const worldX =
        gamePlayCam.x +
        (screenX - rect.left - rect.width / 2) / gamePlayCam.scale;
      const worldY =
        gamePlayCam.y -
        (screenY - rect.top - rect.height / 2) / gamePlayCam.scale;
      return [worldX, worldY];
    }
    const ndcX = ((screenX - rect.left) / rect.width) * 2 - 1;
    const ndcY = -(((screenY - rect.top) / rect.height) * 2 - 1);
    const worldX = cameraX + (ndcX * (rect.width / 2)) / cameraZoom;
    const worldY = cameraY + (ndcY * (rect.height / 2)) / cameraZoom;
    return [worldX, worldY];
  }

  function worldToScreen(worldX: number, worldY: number): [number, number] {
    const rect = canvasEl.getBoundingClientRect();
    if (currentViewMode === "game" && gamePlayCam) {
      const sx =
        rect.left +
        rect.width / 2 +
        (worldX - gamePlayCam.x) * gamePlayCam.scale;
      const sy =
        rect.top +
        rect.height / 2 -
        (worldY - gamePlayCam.y) * gamePlayCam.scale;
      return [sx, sy];
    }
    const ndcX = ((worldX - cameraX) * cameraZoom) / (rect.width / 2);
    const ndcY = ((worldY - cameraY) * cameraZoom) / (rect.height / 2);
    const sx = rect.left + ((ndcX + 1) / 2) * rect.width;
    const sy = rect.top + ((1 - ndcY) / 2) * rect.height;
    return [sx, sy];
  }

  /** World coords → SVG overlay coords (container-relative, subtracts canvas offset). */
  function worldToLocal(worldX: number, worldY: number): [number, number] {
    const rect = canvasEl.getBoundingClientRect();
    // In Game view the renderer projects through the game camera's virtual
    // resolution (square-pixel expanded to fill the canvas), not the editor
    // camera — mirror that here so overlays track the actual pixels.
    if (currentViewMode === "game" && gamePlayCam) {
      const lx = rect.width / 2 + (worldX - gamePlayCam.x) * gamePlayCam.scale;
      const ly = rect.height / 2 - (worldY - gamePlayCam.y) * gamePlayCam.scale;
      return [lx, ly];
    }
    const ndcX = ((worldX - cameraX) * cameraZoom) / (rect.width / 2);
    const ndcY = ((worldY - cameraY) * cameraZoom) / (rect.height / 2);
    const lx = ((ndcX + 1) / 2) * rect.width;
    const ly = ((1 - ndcY) / 2) * rect.height;
    return [lx, ly];
  }

  // --- Entity picking (AABB hit test) ---
  function pickEntity(worldX: number, worldY: number): number {
    if (!wasmReady) return -1;
    try {
      const boundsJson = viewport_get_entity_bounds();
      const bounds: Array<{
        index: number;
        x: number;
        y: number;
        width: number;
        height: number;
        rotation: number;
        z_order: number;
      }> = JSON.parse(boundsJson);

      let best = -1;
      let bestZ = -Infinity;

      for (const b of bounds) {
        // For rotated entities, transform the click point into the entity's local space
        const dx = worldX - b.x;
        const dy = worldY - b.y;
        const cos = Math.cos(-b.rotation);
        const sin = Math.sin(-b.rotation);
        const localX = dx * cos - dy * sin;
        const localY = dx * sin + dy * cos;

        const hw = b.width / 2;
        const hh = b.height / 2;
        if (localX >= -hw && localX <= hw && localY >= -hh && localY <= hh) {
          if (b.z_order > bestZ || (b.z_order === bestZ && b.index > best)) {
            best = b.index;
            bestZ = b.z_order;
          }
        }
      }
      return best;
    } catch {
      return -1;
    }
  }

  // --- Polygon edit helpers ---

  /** Convert world coordinates to entity-local coordinates (inverse entity transform). */
  function worldToEntity(
    wx: number,
    wy: number,
    t: {
      position: [number, number];
      rotation: number;
      scale: [number, number];
    },
  ): [number, number] {
    const dx = wx - t.position[0];
    const dy = wy - t.position[1];
    const cos = Math.cos(-t.rotation);
    const sin = Math.sin(-t.rotation);
    const lx = (dx * cos - dy * sin) / t.scale[0];
    const ly = (dx * sin + dy * cos) / t.scale[1];
    return [lx, ly];
  }

  /** Convert entity-local coordinates to world coordinates. */
  function entityToWorld(
    lx: number,
    ly: number,
    t: {
      position: [number, number];
      rotation: number;
      scale: [number, number];
    },
  ): [number, number] {
    const sx = lx * t.scale[0];
    const sy = ly * t.scale[1];
    const cos = Math.cos(t.rotation);
    const sin = Math.sin(t.rotation);
    const wx = sx * cos - sy * sin + t.position[0];
    const wy = sx * sin + sy * cos + t.position[1];
    return [wx, wy];
  }

  /** The points being edited, or null when nothing is. */
  function getPolyVertices(): [number, number][] | null {
    if (!polyPath) return null;
    return readPoints(viewEntities[polyEditIdx].components as Record<string, unknown>, polyPath);
  }

  /** The frame the edited points are in: the entity's own transform, or the
   *  world for a list in world units (a camera's confine area). */
  function polyTransform(): {
    position: [number, number];
    rotation: number;
    scale: [number, number];
  } | null {
    if (polyInfo?.world) return { position: [0, 0], rotation: 0, scale: [1, 1] };
    const t = viewEntities[polyEditIdx]?.components.transform;
    return t
      ? { position: [t.position[0], t.position[1]], rotation: t.rotation, scale: [t.scale[0], t.scale[1]] }
      : null;
  }

  /** Find the vertex index near a screen position, or -1. */
  function findNearVertex(
    screenX: number,
    screenY: number,
    threshold: number = 8,
  ): number {
    const verts = getPolyVertices();
    if (!verts) return -1;
    const t = polyTransform();
    if (!t) return -1;

    let bestIdx = -1;
    let bestDist = threshold;
    for (let i = 0; i < verts.length; i++) {
      const [wx, wy] = entityToWorld(verts[i][0], verts[i][1], t);
      const [sx, sy] = worldToLocal(wx, wy);
      const dist = Math.hypot(screenX - sx, screenY - sy);
      if (dist < bestDist) {
        bestDist = dist;
        bestIdx = i;
      }
    }
    return bestIdx;
  }

  /** A hint for an empty list, which has nothing to drag yet. */
  function editVertsHint(): string {
    const v = getPolyVertices();
    return v && v.length === 0 ? " - click to add points" : "";
  }

  /** The fewest points the edited list may be left with. */
  function minVertexCount(): number {
    return polyInfo?.min ?? 3;
  }

  // --- Resolve world transform for an entity (walks parent chain) ---
  function resolveWorldTransform(
    entityIdx: number,
    entities: typeof $scene.entities,
  ): {
    x: number;
    y: number;
    rotation: number;
    scaleX: number;
    scaleY: number;
  } {
    const ent = entities[entityIdx];
    const t = ent?.components.transform;
    let x = t?.position[0] ?? 0;
    let y = t?.position[1] ?? 0;
    let rot = t?.rotation ?? 0;
    let sx = t?.scale[0] ?? 1;
    let sy = t?.scale[1] ?? 1;

    let curIdx = entityIdx;
    let depth = 0;
    while (depth < 32) {
      const parentName = entities[curIdx]?.parent;
      if (!parentName) break;
      const parentIdx = entities.findIndex((e) => e.name === parentName);
      if (parentIdx < 0 || parentIdx === curIdx) break;

      const pt = entities[parentIdx].components.transform;
      const px = pt?.position[0] ?? 0;
      const py = pt?.position[1] ?? 0;
      const pr = pt?.rotation ?? 0;
      const psx = pt?.scale[0] ?? 1;
      const psy = pt?.scale[1] ?? 1;

      const cos = Math.cos(pr);
      const sin = Math.sin(pr);
      const rx = x * cos - y * sin;
      const ry = x * sin + y * cos;
      x = px + rx * psx;
      y = py + ry * psy;
      rot += pr;
      sx *= psx;
      sy *= psy;

      curIdx = parentIdx;
      depth++;
    }
    return { x, y, rotation: rot, scaleX: sx, scaleY: sy };
  }

  // --- Transform access: live world during play, scene file in edit mode ---
  //
  // Play-mode edits go straight into the running world through
  // `viewport_set_entity_transform` (no undo entry, no dirty flag) and are
  // discarded when Stop restores the play-start scene — the same bargain
  // Unity makes for Scene-view edits during play.

  /** Local transform of `sel`, or null when it has none. */
  function localTransform(sel: number): {
    position: [number, number];
    rotation: number;
    scale: [number, number];
  } | null {
    if ($isPlaying) {
      const t = get(runtimeEntity)?.components?.transform;
      return t
        ? {
            position: [t.position[0], t.position[1]],
            rotation: t.rotation,
            scale: [t.scale[0], t.scale[1]],
          }
        : null;
    }
    const stored = $scene.entities[sel];
    const ent = stored ? viewOf(stored, $scenePrefabs) : stored;
    const t = ent?.components.transform;
    return t
      ? {
          position: [t.position[0], t.position[1]],
          rotation: t.rotation,
          scale: [t.scale[0], t.scale[1]],
        }
      : null;
  }

  /** Resolved world transform of `sel` — where the gizmo goes. */
  function worldTransformOf(sel: number): {
    x: number;
    y: number;
    rotation: number;
    scaleX: number;
    scaleY: number;
  } | null {
    if ($isPlaying) {
      try {
        const w = JSON.parse(viewport_get_entity_world_transform(sel));
        if (!w) return null;
        return {
          x: w.x,
          y: w.y,
          rotation: w.rotation,
          scaleX: w.scale_x,
          scaleY: w.scale_y,
        };
      } catch {
        return null;
      }
    }
    if (sel < 0 || sel >= $scene.entities.length) return null;
    // Instances as they are: one whose position is a prefab override has no
    // transform of its own in the scene store.
    return resolveWorldTransform(sel, viewEntities);
  }

  /**
   * Apply a transform mutation to whichever world is authoritative.
   *
   * In edit mode this is an undoable scene edit; during play it is
   * written to the live entity, rebuilt from `dragStartValue` so a
   * mid-drag poll of the runtime can't feed stale values back in.
   */
  function applyGizmoTransform(
    sel: number,
    label: string,
    mutate: (t: {
      position: [number, number];
      rotation: number;
      scale: [number, number];
    }) => void,
  ) {
    if ($isPlaying) {
      const t = {
        position: [dragStartValue.x, dragStartValue.y] as [number, number],
        rotation: dragStartValue.rotation,
        scale: [dragStartValue.scaleX, dragStartValue.scaleY] as [
          number,
          number,
        ],
      };
      mutate(t);
      viewport_set_entity_transform(
        sel,
        t.position[0],
        t.position[1],
        t.rotation,
        t.scale[0],
        t.scale[1],
      );
      return;
    }
    const prefabs = $scenePrefabs;
    const apply = (sc: SceneData) => {
      const ent = sc.entities[sel];
      if (!ent) return sc;
      // Through the instance: a move of a linked instance is kept as the
      // override (or own transform) it already has, not a copy over it.
      editThroughInstance(ent, prefabs, (view) => {
        if (view.components.transform) {
          mutate(
            view.components.transform as {
              position: [number, number];
              rotation: number;
              scale: [number, number];
            },
          );
        }
      });
      return sc;
    };

    editScene(label, apply);
  }

  // --- Coalescing a drag into one undo step ---------------------------------
  //
  // Every drag here — the transform gizmo, a polygon vertex, a joint anchor —
  // emits an edit on each mousemove. Recording each as its own undo step meant
  // one drag filled the history with dozens of entries and Ctrl+Z rewound it a
  // mouse-tick at a time. The gesture is one intent, so it gets one command:
  // snapshot on press, mutate silently while moving, push once on release.

  /** The scene as it was when the current drag began, if one is active. */
  let dragUndoBefore: SceneData | null = null;
  /** Label for the command the active drag will produce. */
  let dragUndoLabel = "";

  /** Snapshot the scene so the drag about to start undoes as one step. */
  function beginDragUndo() {
    dragUndoBefore = structuredClone(get(scene));
  }

  /** Abandon a drag without recording anything. */
  function cancelDragUndo() {
    dragUndoBefore = null;
  }

  /** Edit entity `i` as it is: through its prefab when it is a linked
   *  instance, so a vertex or anchor the prefab supplies becomes an override
   *  instead of a write to a component the entity does not store. */
  function editEntityAt(label: string, i: number, mutate: (ent: SceneData["entities"][number]) => void) {
    const prefabs = $scenePrefabs;
    editScene(label, (sc) => {
      const ent = sc.entities[i];
      if (ent) editThroughInstance(ent, prefabs, mutate);
      return sc;
    });
  }

  /** Close the active drag, recording the whole gesture as one edit. */
  function commitDragUndo() {
    const before = dragUndoBefore;
    dragUndoBefore = null;
    if (!before) return;
    const after = get(scene);
    // A press that moved nothing must not leave an undo entry — a gizmo is
    // easy to nudge by a pixel that rounds away.
    if (JSON.stringify(before) === JSON.stringify(after)) return;
    pushCommand(dragUndoLabel, before, after);
  }

  /**
   * Apply a scene edit, coalescing it into the active drag if there is one.
   *
   * A drop-in for `execute` at any call site that can fire repeatedly during a
   * drag. Outside a drag it behaves exactly like `execute`.
   */
  function editScene(label: string, mutate: (s: SceneData) => SceneData) {
    if (dragUndoBefore !== null) {
      dragUndoLabel = label;
      scene.update((sc) => mutate(structuredClone(sc)));
      dirty.set(true);
      return;
    }
    execute(label, mutate);
  }

  // --- Update gizmo screen position ---
  function updateGizmoPosition() {
    if (!wasmReady || !canvasEl) return;
    const sel = $selectedEntity;
    if (sel < 0) {
      gizmoVisible = false;
      return;
    }
    // Editing the running world is scene-view only — the game view is
    // the player's camera, not a workspace.
    if ($isPlaying && currentViewMode !== "scene") {
      gizmoVisible = false;
      return;
    }
    if (!localTransform(sel)) {
      gizmoVisible = false;
      return;
    }
    const world = worldTransformOf(sel);
    if (!world) {
      gizmoVisible = false;
      return;
    }
    const [sx, sy] = worldToScreen(world.x, world.y);
    const rect = canvasEl.getBoundingClientRect();
    gizmoScreenX = sx - rect.left;
    gizmoScreenY = sy - rect.top;
    gizmoWorldRotation = world.rotation;
    gizmoVisible = true;
  }

  /** Scan scene for texture_ids not yet loaded and fetch their bytes via Tauri. */
  /** Read a scene for Play as a transition or a streamed chunk needs it:
   *  its `.anim` / `.particle` / `.tilemap` sources inlined (the viewport has
   *  no file access), its scripts read, and every asset it uses handed to
   *  the viewport first. `authored` is the scene as written, for the editor's
   *  own store. */
  async function prepareSceneForPlay(projectPath: string, fullPath: string, chunk = false) {
    const loadedScene = await loadScene(fullPath);
    const authored = structuredClone(loadedScene);
    const sceneData = loadedScene;
    // Inline `.anim` clip_source references before the
    // viewport sees the scene (WASM has no fs access).
    const resolved = await resolveAnimSourcesInScene(
      projectPath,
      sceneData,
      (entName, err) =>
        logError(
          `[anim] failed to resolve clip_source on '${entName}': ${err}`,
        ),
    );
    if (resolved > 0)
      logInfo(
        `[anim] resolved ${resolved} .anim reference(s)`,
      );
    // Same for `.particle` particle_source refs.
    const pResolved = await resolveParticleSourcesInScene(
      projectPath,
      sceneData,
      (entName, err) =>
        logError(
          `[particle] failed to resolve particle_source on '${entName}': ${err}`,
        ),
    );
    if (pResolved > 0)
      logInfo(
        `[particle] resolved ${pResolved} .particle reference(s)`,
      );
    // Same for `.tilemap` source refs.
    const tResolved = await resolveTilemapSourcesInScene(
      projectPath,
      sceneData,
      (entName, err) =>
        logError(
          `[tilemap] failed to resolve source on '${entName}': ${err}`,
        ),
    );
    if (tResolved > 0)
      logInfo(
        `[tilemap] resolved ${tResolved} .tilemap reference(s)`,
      );
    // Load script sources for the new scene
    const result = await loadScriptSources(
      projectPath,
      sceneData,
    );
    // Load textures for the new scene
    loadSceneTextures(sceneData);
    // Awaited like the .gui documents below: a scene
    // transition that beat its fonts in would show a frame
    // of built-in-font text before snapping to the real one.
    await loadSceneFonts(sceneData);
    // Awaited: scripts touch these in `init()`, which runs
    // inside `viewport_apply_scene_transition` (or as a chunk lands).
    // Textures and materials are not, because a missing one
    // renders a fallback and uploads lazily, whereas a
    // missing .gui or audio clip makes the script's call
    // fail outright.
    await loadSceneGuiDocuments(sceneData);
    await loadSceneAudio(fullPath);
    await loadSceneBehaviors(sceneData);
    // Prefabs are project-wide and cached per project, so
    // this is a no-op after the first scene — but the
    // transition path can be the first thing that runs, and
    // scene load now expands `prefab_instance` entries.
    await loadProjectPrefabs(projectPath);
    await loadProjectDataTables();
    await loadProjectSkeletalClips();
    // Awaited too. These three are not read by a script's
    // init(), so a late arrival is a one-frame flicker
    // rather than a failure — but "the transition awaits
    // every asset it loads" is a rule worth being able to
    // state without exceptions, and a scene change is not
    // a hot path.
    await loadSceneAtlases(fullPath);
    await loadSceneMaterials(sceneData);
    // A chunk joins a scene; the sort layers are the scene's.
    if (!chunk) await loadSceneSortLayers(sceneData);
    return { authored, sceneData, entries: result.entries };
  }

  type StreamRequest = { kind: "additive" | "preload"; handle?: number; path: string; x?: number; y?: number };

  function loadSceneTextures(s: {
    entities: Array<{
      components: {
        sprite?: { texture_id: string };
        sprite_animator?: {
          clips: Record<string, { frames: { texture_id?: string }[] }>;
        };
        particle_emitter?: { texture_id?: string };
        tilemap?: {
          tilesets: { texture_id: string }[];
        };
        material?: { normal_map_id?: string };
        lightmap?: { texture_id?: string };
      };
    }>;
  }) {
    const proj = $project;
    if (!proj?.path) return;

    const toLoad = new Set<string>();

    for (const entity of s.entities) {
      // Sprite texture
      const texId = entity.components.sprite?.texture_id;
      if (texId) toLoad.add(texId);

      // Animation frame textures (multi-image animations)
      const clips = entity.components.sprite_animator?.clips;
      if (clips) {
        for (const clip of Object.values(clips)) {
          for (const frame of clip.frames) {
            if (frame.texture_id) toLoad.add(frame.texture_id);
          }
        }
      }

      // Particle emitter texture
      const particleTex = entity.components.particle_emitter?.texture_id;
      if (particleTex) toLoad.add(particleTex);

      // Tilemap tileset textures
      const tilesets = entity.components.tilemap?.tilesets;
      if (tilesets) {
        for (const ts of tilesets) {
          if (ts.texture_id) toLoad.add(ts.texture_id);
        }
      }

      // A lit material's normal map. The viewport needs its bytes like any
      // other texture, and nothing loaded them: the lit pass then shaded
      // against the flat fallback, so a normal-mapped sprite looked exactly
      // like an un-normal-mapped one - scene 12's whole demo, erased. The
      // viewport uploads these linear itself; only the bytes come from here.
      const normalMap = entity.components.material?.normal_map_id;
      if (normalMap) toLoad.add(normalMap);
      // A lightmap's baked texture, drawn into the light buffer and named by
      // nothing else in the scene.
      const lightmap = entity.components.lightmap?.texture_id;
      if (lightmap) toLoad.add(lightmap);
      // A light's cookie, which the light draws through.
      const pointCookie = entity.components.point_light?.cookie;
      if (pointCookie) toLoad.add(pointCookie);
      const spotCookie = entity.components.spot_light?.cookie;
      if (spotCookie) toLoad.add(spotCookie);
      // A colour grade's strip (`color_lut`), which only the output pass reads.
      const lut = (entity.components as Record<string, { texture_id?: string } | undefined>).color_lut?.texture_id;
      if (lut) toLoad.add(lut);
      // A falling-sand grid's seed picture (`pixel_sim`), read for its pixels.
      const seed = (entity.components as Record<string, { seed_texture?: string } | undefined>).pixel_sim?.seed_texture;
      if (seed) toLoad.add(seed);
      // Every part a sprite library can swap in (`sprite_library`).
      for (const id of spriteLibraryTextures(entity.components)) toLoad.add(id);
    }

    for (const texId of toLoad) {
      if (!isFileTexture(texId) || loadedTextures.has(texId)) continue;
      loadedTextures.add(texId);
      readAssetBinary(proj.path, texId)
        .then((bytes) => {
          const data = new Uint8Array(bytes);
          viewport_load_texture(texId, data);
        })
        .catch((err) => {
          loadedTextures.delete(texId);
          logError(`Failed to load texture "${texId}": ${err}`);
        });
    }
  }

  /** Load the textures the viewport found missing while drawing.
   *
   *  `loadSceneTextures` covers what the scene file names, but a sprite can
   *  arrive with a texture nothing named - a package creating its own sprites
   *  (a match-3 package's gems), a texture swapped in at runtime. The viewport
   *  notices when it cannot draw one and asks, each id once; the editor has
   *  the filesystem, so it answers. */
  function loadRequestedTextures() {
    const proj = $project;
    if (!proj?.path) return;
    let ids: string[];
    try {
      ids = JSON.parse(viewport_take_texture_requests());
    } catch {
      return;
    }
    // No `loadedTextures` check: the viewport is the authority on what it is
    // missing, and it asks for each id only once.
    for (const texId of ids) {
      if (!isFileTexture(texId)) continue;
      loadedTextures.add(texId);
      readAssetBinary(proj.path, texId)
        .then((bytes) => viewport_load_texture(texId, new Uint8Array(bytes)))
        .catch((err) => {
          loadedTextures.delete(texId);
          logError(`Failed to load texture "${texId}": ${err}`);
        });
    }
  }

  /** Hand over the video files the viewport's players are waiting for.
   *
   *  A video plays in a `<video>` element, which needs a URL; the viewport
   *  has no filesystem, so the editor reads each file and passes the bytes,
   *  which the viewport keeps as a Blob URL. Asked for each file once. */
  function loadRequestedVideos() {
    const proj = $project;
    if (!proj?.path) return;
    let ids: string[];
    try {
      ids = JSON.parse(viewport_take_video_requests());
    } catch {
      return;
    }
    for (const source of ids) {
      readAssetBinary(proj.path, source)
        .then((bytes) => viewport_load_video(source, new Uint8Array(bytes)))
        .catch((err) => logError(`Failed to load video "${source}": ${err}`));
    }
  }

  /** The parts of a `.gui` node this scan cares about. */
  type GuiNodeLike = {
    node_type?: { font_id?: string };
    children?: GuiNodeLike[];
  };

  /** Collect `font_id` from Text / Button / TextInput nodes, recursively.
   *  Mirrors `collect_gui_font_ids` in `renderer/font_asset.rs`. */
  function collectGuiFontIds(node: GuiNodeLike, out: Set<string>) {
    const fontId = node.node_type?.font_id;
    if (fontId) out.add(fontId);
    for (const child of node.children ?? []) collectGuiFontIds(child, out);
  }

  /** Push every custom bitmap font the scene names into the viewport.
   *
   *  The native runtime scans the world for `TextComponent.font_id` and reads
   *  each `.font` off disk; the WASM viewport has no filesystem, so the editor
   *  does the reading. Scanning the live scene rather than the saved file
   *  matters here: a font picked in the inspector should render before the
   *  scene is saved.
   *
   *  A font is two files. The descriptor goes in under the `font_id` itself;
   *  the atlas image goes in under the sibling id, derived by the **engine's**
   *  `font_texture_id` rather than by a regex here, so the pairing cannot
   *  drift from what the render pass looks up. */
  async function loadSceneFonts(s: {
    entities: Array<{
      components: { text?: { font_id?: string }; ui_text?: { font_id?: string }; gui?: { gui_id?: string } };
    }>;
  }) {
    const proj = $project;
    if (!proj?.path) return;

    const toLoad = new Set<string>();
    for (const entity of s.entities) {
      // An empty id is the built-in font and needs no asset.
      for (const fontId of [entity.components.text?.font_id, entity.components.ui_text?.font_id]) {
        if (fontId && fontId !== "default") toLoad.add(fontId);
      }
    }

    // `.gui` Text / Button / TextInput widgets carry their own `font_id`, so a
    // document can name a font no `TextComponent` uses. The native and web
    // runtimes union these in via `collect_gui_font_ids`; without the same
    // union here the font renders in the game and falls back to the built-in
    // face in the editor - the worst way round to find out.
    for (const entity of s.entities) {
      const guiId = entity.components.gui?.gui_id;
      if (!guiId) continue;
      try {
        const bytes = await readAssetBinary(proj.path, guiId);
        const doc = JSON.parse(
          new TextDecoder().decode(new Uint8Array(bytes)),
        ) as { root?: GuiNodeLike };
        if (doc.root) collectGuiFontIds(doc.root, toLoad);
      } catch {
        // Unreadable or not yet saved - loadSceneGuiDocuments reports it.
      }
    }

    for (const fontId of toLoad) {
      if (loadedFonts.has(fontId)) continue;
      loadedFonts.add(fontId);

      // A TTF / OTF is the font itself: its atlas is made in the viewport
      // as text asks for glyphs (`renderer::dynamic_font`).
      if (/\.(ttf|otf)$/i.test(fontId)) {
        try {
          const bytes = await readAssetBinary(proj.path, fontId);
          viewport_load_font_file(fontId, new Uint8Array(bytes));
        } catch (err) {
          loadedFonts.delete(fontId);
          logError(`Failed to load font "${fontId}": ${err}`);
        }
        continue;
      }
      const textureId = viewport_font_texture_id(fontId);
      try {
        // The atlas goes first: the viewport drops a text draw whose texture
        // is missing, so pushing the descriptor first would spend a frame
        // warning about a texture that is already on its way.
        const pngBytes = await readAssetBinary(proj.path, textureId);
        viewport_load_texture(textureId, new Uint8Array(pngBytes));

        const bytes = await readAssetBinary(proj.path, fontId);
        viewport_load_font(fontId, new TextDecoder().decode(new Uint8Array(bytes)));
      } catch (err) {
        loadedFonts.delete(fontId);
        logError(
          `Failed to load font "${fontId}" (expects its atlas at "${textureId}"): ${err}`,
        );
      }
    }
  }

  /** Convert a `.atlas` reference path into the sibling-PNG-based atlas
   *  id (forward slashes, `.png` extension). Mirrors the native
   *  `derive_atlas_id` in `renderer/atlas_asset.rs` so the editor pushes
   *  atlas data under the same id scripts pass to `set_sprite_region`. */
  function deriveAtlasId(rel: string): string {
    const normalized = rel.replace(/\\/g, "/");
    const dot = normalized.lastIndexOf(".");
    return dot >= 0 ? `${normalized.slice(0, dot)}.png` : `${normalized}.png`;
  }

  /** Preload the scene's sprite atlases into the viewport so play-mode
   *  scripts can call `set_sprite_region(entity, atlas_id, region)`.
   *
   *  The WASM viewport has no filesystem, so — like the native runtime
   *  resolving `atlas_refs` from disk — the editor reads each `.atlas`
   *  descriptor and forwards it via `viewport_load_atlas`. We also push
   *  the sibling PNG under the atlas id so the swapped `texture_id`
   *  resolves. `atlas_refs` is a top-level scene field that the typed
   *  Tauri `load_scene` deserializer drops, so we read the raw `.scene`
   *  JSON from disk to recover it. */
  /** Upload a scene's top-level `audio_refs` clips into the viewport.
   *
   *  Toolbar does this once for the scene you pressed Play on. That was enough
   *  while the game was a single scene, but a script-driven `load_scene` never
   *  triggered it — so entering the arena from a hub left every clip missing
   *  and each `play_sound` logged a "clip not ready" line, per call, forever.
   *
   *  Reads the RAW scene file: `audio_refs` is a top-level field the typed
   *  scene loader drops, the same gotcha `atlas_refs` has. */
  async function loadSceneAudio(scenePathArg?: string) {
    const proj = $project;
    if (!proj?.path) return;
    const sp = scenePathArg ?? get(scenePath);
    if (!sp) return;
    let refs: string[] = [];
    try {
      const raw = await readTextFile(sp);
      const parsed = JSON.parse(raw) as { audio_refs?: unknown };
      if (Array.isArray(parsed.audio_refs)) {
        refs = parsed.audio_refs.filter((r): r is string => typeof r === "string");
      }
    } catch {
      return; // scene unreadable / not saved yet
    }
    for (const clipPath of refs) {
      if (loadedAudio.has(clipPath)) continue;
      loadedAudio.add(clipPath);
      try {
        const bytes = await readAssetBinary(proj.path, clipPath);
        viewport_load_audio(clipPath, new Uint8Array(bytes));
      } catch (e) {
        loadedAudio.delete(clipPath);
        logError(`Failed to load audio clip "${clipPath}": ${e}`);
      }
    }
  }

  async function loadSceneAtlases(scenePathArg?: string) {
    const proj = $project;
    if (!proj?.path) return;
    const sp = scenePathArg ?? get(scenePath);
    if (!sp) return;

    let refs: string[] = [];
    try {
      const raw = await readTextFile(sp);
      const parsed = JSON.parse(raw) as { atlas_refs?: unknown };
      if (Array.isArray(parsed.atlas_refs)) {
        refs = parsed.atlas_refs.filter(
          (r): r is string => typeof r === "string",
        );
      }
    } catch {
      // Scene file unreadable / not yet saved — nothing to preload.
      return;
    }

    for (const ref of refs) {
      const atlasId = deriveAtlasId(ref);
      if (loadedAtlases.has(atlasId)) continue;
      loadedAtlases.add(atlasId);

      // 1) Atlas descriptor JSON → viewport AssetManager.
      try {
        const bytes = await readAssetBinary(proj.path, ref);
        const json = new TextDecoder().decode(new Uint8Array(bytes));
        viewport_load_atlas(atlasId, json);
      } catch (err) {
        loadedAtlases.delete(atlasId);
        logError(`Failed to load atlas "${ref}": ${err}`);
        continue;
      }

      // 2) Sibling PNG → texture under the atlas id, so sprites that
      //    swap their texture_id to the atlas resolve a GPU texture.
      if (!loadedTextures.has(atlasId)) {
        loadedTextures.add(atlasId);
        try {
          const pngBytes = await readAssetBinary(proj.path, atlasId);
          viewport_load_texture(atlasId, new Uint8Array(pngBytes));
        } catch (err) {
          loadedTextures.delete(atlasId);
          logError(`Failed to load atlas texture "${atlasId}": ${err}`);
        }
      }
    }
  }

  /** Track which prefabs have already been forwarded into the viewport
   *  for the current project, so we don't re-install them on every
   *  project-store fire. Cleared when the project path changes. */
  let loadedPrefabsForProject: string | null = null;

  /** Fetch all `.prefab` files in the project (via the Tauri
   *  `list_prefabs` command) and forward them into the viewport so the
   *  browser bridge's `spawn_prefab(name)` can find them. The native
   *  runtime does the equivalent in `wgpu_scene_runner.rs`; the WASM
   *  viewport has no filesystem so we drive this from the editor. */
  async function loadProjectPrefabs(projectPath: string) {
    if (loadedPrefabsForProject === projectPath) return;
    loadedPrefabsForProject = projectPath;
    try {
      const list = await listPrefabs(projectPath);
      const prefabTextures = new Set<string>();
      for (const p of list) {
        try {
          // Keyed as the runtimes key it: its `name`, else its file's stem.
          viewport_load_prefab(prefabKey(p), JSON.stringify(p.data));
        } catch (e) {
          logError(`Failed to install prefab "${p.name}": ${e}`);
        }
        // Its textures and its parts' (`prefabTextures.ts`): the scene scan
        // only walks scene entities.
        for (const id of prefabTextureIds(p.data)) prefabTextures.add(id);
      }
      for (const texId of prefabTextures) {
        if (loadedTextures.has(texId)) continue;
        loadedTextures.add(texId);
        readAssetBinary(projectPath, texId)
          .then((bytes) => viewport_load_texture(texId, new Uint8Array(bytes)))
          .catch((err) => {
            loadedTextures.delete(texId);
            logError(`Failed to load prefab texture "${texId}": ${err}`);
          });
      }
      if (list.length > 0) {
        logInfo(`Installed ${list.length} prefab(s) into viewport`);
      }
    } catch (err) {
      logError(`Failed to list prefabs: ${err}`);
    }
  }

  /** Walk a parsed .gui document and upload every texture its Image nodes
   *  reference. The scene-component scan in loadSceneTextures cannot find
   *  these — they live inside the GUI document tree. */
  function loadGuiTextures(doc: { root?: unknown }) {
    const proj = $project;
    if (!proj?.path) return;
    const toLoad = new Set<string>();
    const walk = (node: any) => {
      if (!node || typeof node !== "object") return;
      const texId = node.node_type?.texture_id;
      if (typeof texId === "string" && texId.length > 0) toLoad.add(texId);
      for (const child of node.children ?? []) walk(child);
    };
    walk(doc.root);
    for (const texId of toLoad) {
      if (!isFileTexture(texId) || loadedTextures.has(texId)) continue;
      loadedTextures.add(texId);
      readAssetBinary(proj.path, texId)
        .then((bytes) => viewport_load_texture(texId, new Uint8Array(bytes)))
        .catch((err) => {
          loadedTextures.delete(texId);
          logError(`Failed to load GUI texture "${texId}": ${err}`);
        });
    }
  }

  /** Seed the viewport's `get_data` tables from the project's `.data` files.
   *
   *  Data tables are project-wide rather than per-scene — a script can read
   *  any table by path — so this loads all of them once per play session
   *  rather than per scene.
   *
   *  Native walks `assets/` itself and the web build gets a list baked in by
   *  the builder. The editor viewport has no filesystem, so nothing populated
   *  its tables at all and `get_data` returned nil for every lookup, in the
   *  editor only. */
  let seededDataTables = false;
  async function loadProjectDataTables() {
    const proj = $project;
    if (!proj?.path || seededDataTables) return;

    try {
      const all = await listAssets(proj.path);
      const dataFiles = all.filter((f) => f.endsWith(".data"));
      if (dataFiles.length === 0) {
        seededDataTables = true;
        return;
      }
      const decoder = new TextDecoder();
      const tables: Record<string, unknown> = {};
      for (const rel of dataFiles) {
        try {
          const bytes = await readAssetBinary(proj.path, rel);
          const parsed = JSON.parse(decoder.decode(new Uint8Array(bytes)));
          // Keyed by the assets-relative path without the extension, which is
          // what `get_data("data/enemies", ...)` looks up. The preamble also
          // strips a trailing `.data`, so both spellings resolve.
          tables[rel.replace(/\.data$/, "")] = { rows: parsed?.rows ?? [] };
        } catch (err) {
          logError(`Failed to load data table "${rel}": ${err}`);
        }
      }
      const wasm = await import("./viewport-wasm/jengine_editor_viewport.js");
      (wasm as any).viewport_seed_data_tables(JSON.stringify(tables));
      seededDataTables = true;
      logInfo(`[data] seeded ${Object.keys(tables).length} data table(s)`);
    } catch (err) {
      logError(`Failed to seed data tables: ${err}`);
    }
  }

  /** Push every `.skelanim` clip in the project into the viewport.
   *
   *  Project-wide rather than per-scene, like data tables: a `SkeletalPlayer`
   *  names a clip by path and scripts can switch clips at runtime. Native
   *  walks `assets/` for these itself; the viewport has no filesystem, so
   *  nothing populated its clip map and every `SkeletalPlayer` held its pose
   *  forever. */
  let seededSkeletalClips = false;
  async function loadProjectSkeletalClips() {
    const proj = $project;
    if (!proj?.path || seededSkeletalClips) return;
    try {
      const all = await listAssets(proj.path);
      const clips = all.filter((f) => f.endsWith(".skelanim"));
      // `.rig` files too, for `Skeleton.spawn_rig`: named by path like clips.
      const rigs = all.filter((f) => f.endsWith(".rig"));
      seededSkeletalClips = true;
      if (clips.length === 0 && rigs.length === 0) return;
      const wasm = await import("./viewport-wasm/jengine_editor_viewport.js");
      const decoder = new TextDecoder();
      for (const rel of rigs) {
        try {
          const bytes = await readAssetBinary(proj.path, rel);
          (wasm as any).viewport_load_skeleton_rig(rel, decoder.decode(new Uint8Array(bytes)));
        } catch (err) {
          logError(`Failed to load skeleton rig "${rel}": ${err}`);
        }
      }
      if (rigs.length > 0) logInfo(`[skeletal] loaded ${rigs.length} rig(s)`);
      for (const rel of clips) {
        try {
          const bytes = await readAssetBinary(proj.path, rel);
          (wasm as any).viewport_load_skeletal_clip(
            rel,
            decoder.decode(new Uint8Array(bytes)),
          );
        } catch (err) {
          logError(`Failed to load skeletal clip "${rel}": ${err}`);
        }
      }
      logInfo(`[skeletal] loaded ${clips.length} clip(s)`);
    } catch (err) {
      logError(`Failed to scan skeletal clips: ${err}`);
    }
  }

  /** Upload every `.behavior` graph the scene references into the viewport.
   *
   *  **Await this before applying a scene transition.** The viewport binds
   *  graph instances from its `pending_behaviors` cache; a graph that is not
   *  in the cache when the entities appear simply never attaches, and a
   *  graph-driven entity then does nothing at all — no error, no movement.
   *
   *  The Toolbar does the same thing (plus prefab graphs) when Play starts.
   *  The cache is never cleared, so entries from that upload survive; this
   *  only has to cover graphs introduced by the scene being entered. */
  async function loadSceneBehaviors(s: {
    entities: Array<{ components: Record<string, unknown> }>;
  }) {
    const proj = $project;
    if (!proj?.path) return;

    const paths = new Set<string>();
    for (const entity of s.entities ?? []) {
      const path = (entity.components as any)?.behavior_graph?.graph_path;
      if (path) paths.add(path);
    }
    if (paths.size === 0) return;

    const wasm = await import("./viewport-wasm/jengine_editor_viewport.js");
    const decoder = new TextDecoder();
    await Promise.all(
      [...paths].map(async (path) => {
        try {
          const bytes = await readAssetBinary(proj.path!, path);
          (wasm as any).viewport_load_behavior(
            path,
            decoder.decode(new Uint8Array(bytes)),
          );
        } catch (err) {
          logError(`Failed to load behavior "${path}": ${err}`);
        }
      }),
    );
  }

  /** Scan scene for GUI components and load their .gui documents into the
   *  viewport.
   *
   *  **Await this before running the scene's scripts.** A script's `init()`
   *  routinely calls `gui_set` / `gui_on_click` against its document, and
   *  `viewport_apply_scene_transition` installs and runs scripts
   *  synchronously — so leaving these reads in flight means the first frame of
   *  script work addresses a document the viewport does not have yet, which
   *  surfaces as `gui_set: gui '...' not loaded` once per call and silently
   *  loses whatever those calls were setting up. */
  async function loadSceneGuiDocuments(s: {
    entities: Array<{
      components: {
        gui?: { gui_id: string };
      };
    }>;
  }) {
    const proj = $project;
    if (!proj?.path) return;

    const pending: Promise<void>[] = [];
    for (const entity of s.entities) {
      const guiId = entity.components.gui?.gui_id;
      if (!guiId) continue;
      if (loadedGuis.has(guiId)) continue;
      loadedGuis.add(guiId);
      pending.push(readAssetBinary(proj.path, guiId)
        .then((bytes) => {
          const json = new TextDecoder().decode(new Uint8Array(bytes));
          console.log(
            `[GUI LOAD] proj="${proj.path}" guiId="${guiId}" bytes=${bytes.length} json_len=${json.length} preview=${json.substring(0, 200)}`,
          );
          viewport_load_gui(guiId, json);
          // A .gui Image node's texture is referenced from the document, not
          // from any scene component, so loadSceneTextures never sees it —
          // without this the icon is simply missing, in the editor only.
          try {
            loadGuiTextures(JSON.parse(json));
          } catch (err) {
            logError(`Failed to scan GUI "${guiId}" for textures: ${err}`);
          }
        })
        .catch((err) => {
          loadedGuis.delete(guiId);
          logError(`Failed to load GUI "${guiId}": ${err}`);
        }));
    }
    await Promise.all(pending);
  }

  /** Slice 13d — push every `.material` (and its referenced `.shader`
   *  graph, if any) into the WASM viewport so graph-driven materials
   *  render correctly in edit + play modes. Mirror of the native
   *  runtime's lazy `AssetManager::load_material` path; the WASM
   *  viewport has no filesystem so the editor pushes the JSON over
   *  IPC. Idempotent via the `loadedMaterials` / `loadedShaderGraphs`
   *  caches. */
  async function loadSceneMaterials(s: {
    entities: Array<{
      components: {
        material?: { material_id: string };
      };
    }>;
  }) {
    const proj = $project;
    if (!proj?.path) return;

    for (const entity of s.entities) {
      const mid = entity.components.material?.material_id;
      if (!mid) continue;
      if (BUILTIN_MATERIAL_IDS.has(mid)) continue;
      if (loadedMaterials.has(mid)) continue;
      await pushMaterial(proj.path, mid, false);
    }
  }

  /** Read one `.material`, push it, and follow it to its shader graph.
   *  `always` pushes the graph even if this editor pushed it before - the
   *  viewport asked, so it is the authority on what it is missing. */
  async function pushMaterial(projPath: string, mid: string, always: boolean) {
    loadedMaterials.add(mid);
    try {
      const bytes = await readAssetBinary(projPath, `${mid}.material`);
      const json = new TextDecoder().decode(new Uint8Array(bytes));
      viewport_load_material(mid, json);
      // Follow the chain to the shader graph (if any).
      let parsed: { shader_graph_path?: string } | undefined;
      try {
        parsed = JSON.parse(json);
      } catch {
        return;
      }
      const graphPath = parsed?.shader_graph_path;
      if (!graphPath) return;
      if (!always && loadedShaderGraphs.has(graphPath)) return;
      loadedShaderGraphs.add(graphPath);
      // `shader_graph_path` is stored relative to the project root
      // (e.g. "assets/shaders/foo.shader"); `read_asset_binary`
      // prepends `assets/`, so strip the leading segment.
      const rel = graphPath.startsWith("assets/")
        ? graphPath.slice("assets/".length)
        : graphPath;
      try {
        const gbytes = await readAssetBinary(projPath, rel);
        const gjson = new TextDecoder().decode(new Uint8Array(gbytes));
        viewport_load_shader_graph(graphPath, gjson);
      } catch (err) {
        loadedShaderGraphs.delete(graphPath);
        logError(`Failed to load shader graph "${graphPath}": ${err}`);
      }
    } catch (err) {
      loadedMaterials.delete(mid);
      logError(`Failed to load material "${mid}": ${err}`);
    }
  }

  /** Load the materials the viewport asks for: every prefab's, queued as
   *  the prefabs arrive, and any a draw wanted and did not have (one a
   *  script switched to). `loadSceneMaterials` only sees what the scene's
   *  entities name, so a graph material only a spawned prefab used was
   *  never pushed and drew with the default shader - Spellcraft's
   *  shockwaves, a graph over a white quad, flashed as white boxes. */
  function loadRequestedMaterials() {
    const proj = $project;
    if (!proj?.path) return;
    let ids: string[];
    try {
      ids = JSON.parse(viewport_take_material_requests());
    } catch {
      return;
    }
    for (const mid of ids) void pushMaterial(proj.path, mid, true);
  }

  /** Sort layers 1.4 — read the scene's `sort_layers` field (a
   *  project-relative path to a `.sortlayers` asset), fetch the JSON
   *  on first use, push it into the viewport, then activate it. When
   *  the scene clears the field we reset the active set to `None`
   *  (passthrough z_order). Idempotent via the `loadedSortLayers`
   *  cache. */
  async function loadSceneSortLayers(s: { sort_layers?: string }) {
    const proj = $project;
    const id = s.sort_layers;
    if (!proj?.path || !id) {
      if (activeSortLayersId !== null) {
        activeSortLayersId = null;
        viewport_set_active_sort_layers(undefined);
      }
      return;
    }
    if (!loadedSortLayers.has(id)) {
      loadedSortLayers.add(id);
      try {
        const bytes = await readAssetBinary(proj.path, id);
        const json = new TextDecoder().decode(new Uint8Array(bytes));
        viewport_load_sort_layers(id, json);
      } catch (err) {
        loadedSortLayers.delete(id);
        logError(`Failed to load sort_layers "${id}": ${err}`);
        return;
      }
    }
    if (activeSortLayersId !== id) {
      activeSortLayersId = id;
      viewport_set_active_sort_layers(id);
    }
  }

  onMount(async () => {
    try {
      // Initialize the WASM module (loads the .wasm binary)
      await init();
      wasmReady = true;

      // Set canvas size to fill container before init
      const rect = canvasEl.parentElement!.getBoundingClientRect();
      canvasEl.width = Math.floor(rect.width);
      canvasEl.height = Math.floor(rect.height);

      viewport_init("editor-viewport-canvas");
      // `viewport_init` starts the engine with no safe area; a device chosen
      // before it (or remembered) applies now.
      applyCanvasSize();
      loaded = true;
      logInfo("Viewport initialized");

      // `viewport_init` reinitialises the WASM `APP` cell, which
      // wipes any prior `play_state`. If the Svelte `isPlaying` store
      // is still `true` (e.g. because the viewport remounted while
      // play mode was active), reset it so the toolbar doesn't get
      // wedged showing Stop without a backing playing session.
      try {
        const { viewport_is_playing: vip } = await import(
          "./viewport-wasm/jengine_editor_viewport.js"
        );
        if (!vip()) {
          isPlaying.set(false);
        }
      } catch {
        // Non-fatal — store stays as-is.
      }

      // --- Resize observer ---
      // Sized by `applyCanvasSize`, which letterboxes to a previewed device.
      resizeObserver = new ResizeObserver(() => applyCanvasSize());
      unsubDevicePreview = devicePreview.subscribe(() => applyCanvasSize());
      resizeObserver.observe(canvasEl.parentElement!);

      // --- Subscribe to stores ---
      // A saved prefab reaches the instances in view: reinstall the prefab
      // map, then push the scene again so `viewport_update_scene` expands
      // its `prefab_instance` entries against the new one.
      let prefabsSeen: number | null = null;
      unsubPrefabs = prefabsChanged.subscribe((n) => {
        if (prefabsSeen === null) {
          prefabsSeen = n;
          return;
        }
        prefabsSeen = n;
        const path = get(project)?.path;
        if (!wasmReady || !path) return;
        loadedPrefabsForProject = null;
        void loadProjectPrefabs(path).then(() => scene.update((sc) => sc));
      });
      // What the edit-mode viewport shows: the scene, or while a timeline is
      // scrubbed, the scene posed at the playhead (`scenePreview`), which is
      // never written to the scene store.
      const showScene = (s: SceneData) => {
        if (wasmReady && !viewport_is_playing()) {
          // Resolve `.anim` clip_source and `.particle` particle_source
          // refs on a clone so the edit-mode viewport sees fully-inlined
          // data and any backfilled `sprite.texture_id` / emitter field
          // values. We must not mutate the scene store — the on-disk
          // format keeps the source references, not the inlined data.
          const proj = get(project);
          const applyResolved = async () => {
            let sceneForView: typeof s = s;
            if (proj?.path) {
              try {
                const clone = JSON.parse(JSON.stringify(s));
                const resolved = await resolveAnimSourcesInScene(
                  proj.path,
                  clone,
                  (entName, err) =>
                    logError(
                      `[anim] failed to resolve clip_source on '${entName}': ${err}`,
                    ),
                );
                const pResolved = await resolveParticleSourcesInScene(
                  proj.path,
                  clone,
                  (entName, err) =>
                    logError(
                      `[particle] failed to resolve particle_source on '${entName}': ${err}`,
                    ),
                );
                const tResolved = await resolveTilemapSourcesInScene(
                  proj.path,
                  clone,
                  (entName, err) =>
                    logError(
                      `[tilemap] failed to resolve source on '${entName}': ${err}`,
                    ),
                );
                if (resolved > 0 || pResolved > 0 || tResolved > 0)
                  sceneForView = clone;
              } catch {
                // fall back to original scene
              }
            }
            try {
              viewport_update_scene(JSON.stringify(sceneForView));
            } catch {
              // Ignore serialization errors during rapid updates
            }
            // Load any new textures referenced by the scene (use
            // the resolved clone so backfilled texture_ids upload).
            loadSceneTextures(sceneForView);
            // Preload sprite atlases for `set_sprite_region`.
            void loadSceneAtlases();
            // Custom bitmap fonts named by `TextComponent.font_id`.
            void loadSceneFonts(sceneForView);
            // Load any new GUI documents referenced by the scene. Awaited so
            // pressing Play immediately cannot beat the document in.
            await loadSceneGuiDocuments(sceneForView);
            // Slice 13d — push graph-driven materials + shader graphs.
            void loadSceneMaterials(sceneForView);
            void loadSceneSortLayers(sceneForView);
          };
          void applyResolved();
        }
        // As they are: a linked instance through its prefab, so its camera
        // and transform are where the instance has them.
        sceneEntities = (s.entities ?? []).map((e) => viewOf(e, get(scenePrefabs)));
      };
      // The scene shown, with every `text_key` in the previewed language
      // when the Localisation panel previews one (`localeEdit.localized`).
      const withLanguage = (s: SceneData): SceneData => {
        const lang = get(previewLanguage);
        if (!lang) return s;
        const tables = get(localeTables);
        return { ...s, entities: localized(s.entities ?? [], tables, languageFor(tables, lang)) };
      };
      const reshow = () => showScene(withLanguage(get(scenePreview) ?? get(scene)));
      unsubScene = scene.subscribe((s) => showScene(withLanguage(get(scenePreview) ?? s)));
      let previewSeen = false;
      unsubPreview = scenePreview.subscribe(() => {
        if (!previewSeen) {
          previewSeen = true;
          return;
        }
        reshow();
      });
      let languageSeen = false;
      unsubLanguage = previewLanguage.subscribe(() => {
        if (!languageSeen) {
          languageSeen = true;
          return;
        }
        reshow();
      });

      // Seed `get_data` tables as soon as a play session exists. The scene
      // transition path also does this, but pressing Play directly on a scene
      // never goes through a transition — without this, `get_data` would work
      // only in scenes you walked into from another one.
      unsubPlayData = isPlaying.subscribe((playing) => {
        if (playing && wasmReady) {
          void loadProjectDataTables();
          void loadProjectSkeletalClips();
          // Each session starts from the project's mix, not the last
          // session's script changes.
          const settings = get(project)?.settings;
          if (settings) viewport_set_audio_mixer(JSON.stringify(settings));
          // Live ops for the session: the editor's channel, asked afresh.
          if (settings) viewport_set_liveops(JSON.stringify(settings));
        } else if (!playing) {
          // A fresh session rebuilds the play namespace, so the seed has to
          // happen again.
          seededDataTables = false;
          seededSkeletalClips = false;
        }
      });

      unsubSelection = selectedEntity.subscribe((idx) => {
        if (wasmReady) viewport_set_selected(idx);
        selectedIdx = idx;
      });

      // Sync background color from project settings
      unsubProject = project.subscribe((proj) => {
        if (wasmReady && proj?.settings) {
          const [r, g, b, a] = proj.settings.background_color;
          viewport_set_clear_color(r, g, b, a);
          // How the UI is scaled when the viewport panel is not the size the
          // project's UI was authored for. The viewport defaults to
          // 1280x720 scale-to-fit; this is what makes the editor agree with
          // the project's own width/height.
          viewport_set_ui_scale(
            proj.settings.width,
            proj.settings.height,
            proj.settings.ui_scale_mode ?? "ScaleWithScreen",
          );
          // Nearest or filtered, per texture (`settings.texture_filter`);
          // a change re-uploads what is drawn next.
          viewport_set_texture_filters(JSON.stringify(proj.settings));
          // Fixed-step gameplay (`settings.simulation_hz`); off when absent.
          viewport_set_simulation(JSON.stringify(proj.settings));
          // The project's mixer (`settings.audio_mixer`): live while playing,
          // so Project Settings -> Audio is heard as it is changed.
          viewport_set_audio_mixer(JSON.stringify(proj.settings));
          // The project's asset packs (`settings.asset_packs`): every one
          // local here, so `Packs.status` answers "installed".
          viewport_set_asset_packs(JSON.stringify(proj.settings));
        }
        // When a project finishes loading after the viewport has
        // already mounted (the common case — viewport mounts on a
        // blank scene, then the user opens a project), the scene
        // store fired its initial subscriber callback before
        // `proj.path` was set, so `loadSceneTextures` early-returned
        // and no texture upload happened. Re-trigger here now that
        // the project path is known.
        if (wasmReady && proj?.path) {
          // Re-resolve `.anim` and `.particle` refs on a clone so
          // backfilled texture_ids / emitter fields are picked up
          // by loadSceneTextures.
          const s = get(scene);
          (async () => {
            // Prefabs first: `viewport_update_scene` now expands
            // `prefab_instance` entries at load, so a scene pushed before its
            // prefabs are installed comes up with those entities empty.
            await loadProjectPrefabs(proj.path!);
            let sceneForView: typeof s = s;
            try {
              const clone = JSON.parse(JSON.stringify(s));
              const resolved = await resolveAnimSourcesInScene(
                proj.path!,
                clone,
                (entName, err) =>
                  logError(
                    `[anim] failed to resolve clip_source on '${entName}': ${err}`,
                  ),
              );
              const pResolved = await resolveParticleSourcesInScene(
                proj.path!,
                clone,
                (entName, err) =>
                  logError(
                    `[particle] failed to resolve particle_source on '${entName}': ${err}`,
                  ),
              );
              const tResolved = await resolveTilemapSourcesInScene(
                proj.path!,
                clone,
                (entName, err) =>
                  logError(
                    `[tilemap] failed to resolve source on '${entName}': ${err}`,
                  ),
              );
              if (resolved > 0 || pResolved > 0 || tResolved > 0) {
                sceneForView = clone;
                try {
                  viewport_update_scene(JSON.stringify(sceneForView));
                } catch {
                  // ignore
                }
              }
            } catch {
              // fall back to original scene
            }
            loadSceneTextures(sceneForView);
            loadSceneGuiDocuments(sceneForView);
            void loadSceneAtlases();
            void loadSceneFonts(sceneForView);
            void loadSceneMaterials(sceneForView);
            void loadSceneSortLayers(sceneForView);
          })();
        }
      });

      // Sync gizmo mode from store
      unsubGizmoMode = gizmoMode.subscribe((mode) => {
        currentGizmoMode = mode;
      });

      // Sync gizmo space from store
      unsubGizmoSpace = gizmoSpace.subscribe((space) => {
        currentGizmoSpace = space;
      });

      // Clear GUI cache when entering play mode so files are re-read from disk
      unsubPlaying = isPlaying.subscribe((playing) => {
        if (playing) {
          loadedGuis.clear();
          // Force re-read GUI files from disk into WASM gui_cache
          loadSceneGuiDocuments(get(scene));
          // Slice 13d — re-push materials so play mode picks up edits.
          loadedMaterials.clear();
          loadedShaderGraphs.clear();
          void loadSceneMaterials(get(scene));
          loadedSortLayers.clear();
          activeSortLayersId = null;
          void loadSceneSortLayers(get(scene));
        } else {
          // Stop restores the play-start scene from `playReturn`
          // (Toolbar.handleStop), so the mirror stack is spent.
          playSceneStack = [];
        }
      });

      // Sync polygon edit mode from store
      unsubPolyEdit = polygonEditEntity.subscribe((idx) => {
        polyEditIdx = idx;
        if (polyDragVertex !== null) cancelDragUndo();
        polyDragVertex = null;
        polyHoverVertex = null;
      });

      // Sync view mode from store
      unsubViewMode = viewMode.subscribe((mode) => {
        currentViewMode = mode;
        // The device preview is the Game view's alone.
        applyCanvasSize();
        // Restore editor camera when switching back to Scene view
        if (mode === "scene" && wasmReady) {
          viewport_set_camera(cameraX, cameraY, cameraZoom);
          // Restore camera virtual size to canvas dimensions and clear game letterboxing
          viewport_clear_game_viewport();
        }
      });

      // --- Render loop ---
      // A Rust panic inside the viewport traps the WASM module: every later
      // call throws, the loop dies, and to the user the editor has simply
      // frozen. The panic hook writes the message onto `window` in plain JS
      // precisely so it survives that, because anything routed back through
      // WASM would be reporting from a corpse.
      let panicReported = false;
      function checkViewportPanic(): boolean {
        const msg = (window as any).__jengine_viewport_panic;
        if (!msg || panicReported) return panicReported;
        panicReported = true;
        logError(
          `[viewport] the engine panicked and has stopped. Press Stop, then Play ` +
            `to restart it. Details:
${msg}`,
        );
        return true;
      }

      function renderLoop() {
        // Checked first: once the module has trapped, every call below throws
        // and there is nothing useful left to do this frame.
        if (checkViewportPanic()) {
          return;
        }
        // One clock read per frame, shared by every throttled poll below, and
        // the start of the CPU-time measurement.
        const nowMs = performance.now();
        if (wasmReady) {
          // In Game view mode, sync the editor camera with the active
          // game camera. While Play is running, the WASM-side
          // `tick_play_world` calls `update_camera` so the camera can
          // follow the player smoothly — don't clobber it from JS.
          if (currentViewMode === "game") {
            try {
              const camJson = viewport_get_game_camera();
              if (camJson) {
                const gc = JSON.parse(camJson);
                if (!viewport_is_playing()) {
                  viewport_set_camera(gc.x, gc.y, gc.zoom);
                }
                // Override camera virtual size to match game resolution.
                // WASM `viewport_get_game_camera` returns `viewport_w` /
                // `viewport_h` (not `_width` / `_height`) — using the
                // wrong keys silently fell back to 512/448, which is
                // why the Game view never matched the PiP preview.
                const gameW = gc.viewport_w || gc.viewport_width || 512;
                const gameH = gc.viewport_h || gc.viewport_height || 448;
                viewport_set_camera_size(gameW, gameH);
                // Mirror the renderer's square-pixel camera fit so the debug
                // overlay maps world→screen at the same scale. render_editor
                // expands the camera along the canvas's longer axis to keep
                // pixels square; the world→CSS scale is zoom * rect.width/cam_w.
                const rect = canvasEl.getBoundingClientRect();
                const canvasAspect = rect.width / rect.height;
                const gameAspect = gameW / gameH;
                const camW =
                  canvasAspect > gameAspect ? gameH * canvasAspect : gameW;
                gamePlayCam = {
                  x: gc.x,
                  y: gc.y,
                  scale: ((gc.zoom || 1) * rect.width) / camW,
                };
              }
            } catch {
              /* no active camera — fall back to the editor camera */
              gamePlayCam = null;
            }
          }

          viewport_render();
          // The Profiler panel's recording, when one runs (`profilerStore`).
          profilerFrame();
          loadRequestedTextures();
          loadRequestedVideos();
          loadRequestedMaterials();

          // Render PiP camera preview only in Scene view when a camera entity is selected
          if (currentViewMode === "scene") {
            const cp = cameraPreview;
            if (cp && canvasEl) {
              const pipW = Math.min(320, Math.floor(canvasEl.width * 0.3));
              const pipH = Math.round((pipW * (cp.halfH * 2)) / (cp.halfW * 2));
              const pipMargin = 8;
              const pipX = canvasEl.width - pipW - pipMargin;
              const pipY = pipMargin; // GL y=0 is bottom
              viewport_render_pip(
                cp.cx,
                cp.cy,
                cp.zoom || 1,
                cp.halfW * 2,
                cp.halfH * 2,
                pipX,
                pipY,
                pipW,
                pipH,
              );
            }
          }

          updateGizmoPosition();

          // Read debug draw commands from play state
          if (viewport_is_playing()) {
            try {
              // Empty string means "nothing new" — no parse, no loop.
              const logsJson = viewport_get_logs();
              const msgs: string[] = logsJson ? JSON.parse(logsJson) : [];
              for (const msg of msgs) {
                // Its level and the script line it is about (viewportLog.ts).
                const { level, text, scriptPath, scriptLine, source } = classifyViewportLog(msg);
                const log = level === "error" ? logError : level === "warn" ? logWarn : logInfo;
                log(text, scriptPath, scriptLine, source);
              }
            } catch {
              // Ignore JSON parse errors
            }
            try {
              const drawsJson = viewport_get_debug_draws();
              if (drawsJson) {
                debugDrawCommands = JSON.parse(drawsJson);
              } else if (debugDrawCommands.length > 0) {
                // Only when it actually changes: assigning a fresh [] every
                // frame is a reactive write and a re-render for nothing.
                debugDrawCommands = [];
              }
            } catch {
              debugDrawCommands = [];
            }

            // Check for pending scene transitions from scripts
            try {
              // Empty string is "nothing pending". This used to receive the
              // string "null", which is truthy, so every frame parsed it to
              // `null` and threw a TypeError on `.action` into the catch below.
              const pendingJson = viewport_get_pending_scene();
              const pending = pendingJson
                ? (JSON.parse(pendingJson) as { action: string; path?: string } | null)
                : null;
              if (pending) {
                const proj = $project;
                if (pending.action === "pop") {
                  viewport_pop_play_scene();
                  // Walk the editor's mirror back in step with the WASM stack.
                  const prev = playSceneStack.pop();
                  if (prev) {
                    clearSelection();
                    scene.set(prev.scene);
                    scenePath.set(prev.path);
                  }
                } else if (pending.path && proj?.path) {
                  // Resolve full path: project/assets/path
                  const fullPath = `${proj.path}/assets/${pending.path}`;
                  prepareSceneForPlay(proj.path!, fullPath)
                    .then(({ authored, sceneData, entries }) => {
                      // Apply the transition
                      viewport_apply_scene_transition(
                        JSON.stringify(sceneData),
                        JSON.stringify(entries),
                      );
                      // Point the Scene panel / Inspector at the scene the
                      // game just moved into. Entity indices are per-scene,
                      // so the old selection is meaningless now.
                      playSceneStack.push({
                        scene: get(scene),
                        path: get(scenePath),
                      });
                      clearSelection();
                      scene.set(authored);
                      scenePath.set(fullPath);
                      logInfo(`[scene] now playing ${pending.path}`);
                    })
                    .catch((err) => {
                      logError(`Scene transition failed: ${err}`);
                    });
                }
              }
            } catch {
              // Ignore errors
            }
            // Scenes a script streams in (`Scene.load_additive` /
            // `Scene.preload`): read like a transition's, then laid into
            // the running world (`viewport_add_scene_chunk`) or just left
            // loaded. Unloads the viewport does itself.
            try {
              const streamJson = viewport_take_stream_requests();
              if (streamJson && $project?.path) {
                const projectPath = $project.path;
                for (const req of JSON.parse(streamJson) as StreamRequest[]) {
                  const fullPath = `${projectPath}/assets/${req.path}`;
                  prepareSceneForPlay(projectPath, fullPath, req.kind === "additive")
                    .then(({ sceneData, entries }) => {
                      if (req.kind === "additive") {
                        viewport_add_scene_chunk(
                          req.handle ?? 0,
                          JSON.stringify(sceneData),
                          JSON.stringify(entries),
                          req.x ?? 0,
                          req.y ?? 0,
                        );
                      } else {
                        viewport_set_stream_status(-1, req.path, "loaded");
                      }
                    })
                    .catch((err) => {
                      logError(`[stream] '${req.path}' failed: ${err}`);
                      viewport_set_stream_status(req.kind === "additive" ? (req.handle ?? 0) : -1, req.path, "failed");
                    });
                }
              }
            } catch (err) {
              logError(`[stream] ${err}`);
            }
            // Poll the live entity list for the Scene panel. Cheaper than
            // per-frame: the list only changes when something spawns or dies,
            // and it walks the whole world.
            //
            // An empty string means "unchanged since the last poll" - skip the
            // parse and the store write entirely. Setting the store pushes a
            // new array into the Scene panel and re-renders every row, which
            // at thousands of entities was the spike that made the frame graph
            // a sawtooth once every RUNTIME_LIST_INTERVAL frames.
            if (++runtimeListTick >= RUNTIME_LIST_INTERVAL) {
              runtimeListTick = 0;
              try {
                const listJson = viewport_get_runtime_entities();
                if (listJson) {
                  runtimeEntities.set(JSON.parse(listJson));
                }
              } catch {
                // Keep the previous list rather than blanking the panel.
              }
            }

            // Poll selected entity's live state for Inspector, on the same
            // human-reading cadence as the profiler: this serializes every
            // component of the entity through the registry, and the Inspector
            // is a panel someone is looking at, not something the frame
            // depends on. At 167 fps it was rebuilding that JSON — and writing
            // the store the Inspector re-renders from — 167 times a second so
            // a person could watch numbers blur.
            const sel = get(selectedEntity);
            if (sel < 0) {
              if (get(runtimeEntity) !== null) runtimeEntity.set(null);
            } else if (
              // A newly selected entity fills the panel at once; only the
              // live refresh of an already-shown one is on the timer.
              sel !== lastInspectorSel ||
              nowMs - lastInspectorPoll >= PROFILER_INTERVAL_MS
            ) {
              lastInspectorPoll = nowMs;
              lastInspectorSel = sel;
              try {
                const json = viewport_get_runtime_entity(sel);
                runtimeEntity.set(json ? JSON.parse(json) : null);
              } catch {
                runtimeEntity.set(null);
              }
            }
          } else {
            debugDrawCommands = [];
            // Clear runtime entity when not playing
            if (get(runtimeEntity) !== null) {
              runtimeEntity.set(null);
            }
            if (get(runtimeEntities).length > 0) {
              runtimeEntities.set([]);
            }
          }

          // Poll collider bounds for wireframe overlay
          if (get(showColliders)) {
            try {
              const json = viewport_get_collider_bounds();
              colliderBounds = JSON.parse(json);
            } catch {
              colliderBounds = [];
            }
          } else {
            colliderBounds = [];
          }

          // Poll frame stats for the profiler, on a timer rather than every
          // frame — see PROFILER_INTERVAL_MS.
          const profiling = get(showProfiler);
          if (nowMs - lastProfilerPoll >= (profiling ? PROFILER_INTERVAL_MS : STATS_CHIP_INTERVAL_MS)) {
            lastProfilerPoll = nowMs;
            statsCpuMs = cpuFrames > 0 ? cpuAccum / cpuFrames : 0;
            cpuAccum = 0;
            cpuFrames = 0;
            try {
              const statsJson = viewport_get_frame_stats();
              const s = JSON.parse(statsJson);
              statsFps = s.fps ?? 0;
              statsFrameTime = s.frame_time_ms ?? 0;
              statsDrawCalls = s.draw_calls ?? 0;
              statsMemoryBytes = s.memory_bytes ?? 0;
              statsMemoryPeak = s.memory_peak_bytes ?? 0;
              statsMemoryLive = s.memory_live_bytes ?? 0;
              statsEntityCount = s.entity_count ?? 0;
              statsSpriteCount = s.sprite_count ?? 0;
              statsPhysicsTime = s.physics_time_ms ?? 0;
              statsScriptTime = s.script_time_ms ?? 0;
              statsRenderTime = s.render_time_ms ?? 0;
              statsUpdateTime = s.update_time_ms ?? 0;
            } catch {
              /* ignore */
            }
            if (profiling) {
              try {
                const histJson = viewport_get_frame_history();
                frameHistory = JSON.parse(histJson);
              } catch {
                frameHistory = [];
              }
            }
          }
        }
        // Everything above is this frame's work; the browser then idles until
        // the next vsync. Accumulated rather than sampled, because one frame at
        // 167 Hz is noise and the panel only reads it every PROFILER_INTERVAL_MS.
        cpuAccum += performance.now() - nowMs;
        cpuFrames += 1;
        animFrameId = requestAnimationFrame(renderLoop);
      }
      animFrameId = requestAnimationFrame(renderLoop);
    } catch (e) {
      error = `Failed to load viewport: ${e}`;
      logError(error);
    }

    // Focus (F) shortcut needs to work even when the viewport canvas doesn't
    // have keyboard focus (e.g. user clicked the inspector or scene tree).
    onWindowFKey = (e: KeyboardEvent) => {
      if (e.key !== "f" && e.key !== "F") return;
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      // No play check: during running play `playKeys` hands this handler F
      // only in Scene view, and stops it before `window` in Game view.
      if (
        e.target instanceof HTMLInputElement ||
        e.target instanceof HTMLSelectElement ||
        e.target instanceof HTMLTextAreaElement
      )
        return;
      const sel = $selectedEntity;
      const world = sel >= 0 ? worldTransformOf(sel) : null;
      if (world) {
        cameraX = world.x;
        cameraY = world.y;
        if (wasmReady) viewport_set_camera(cameraX, cameraY, cameraZoom);
        e.preventDefault();
      }
    };
    window.addEventListener("keydown", onWindowFKey);
  });

  onDestroy(() => {
    if (animFrameId) cancelAnimationFrame(animFrameId);
    resizeObserver?.disconnect();
    unsubScene?.();
    unsubPrefabs?.();
    unsubPreview?.();
    unsubLanguage?.();
    unsubPlayData?.();
    unsubSelection?.();
    unsubProject?.();
    unsubGizmoMode?.();
    unsubGizmoSpace?.();
    unsubPolyEdit?.();
    unsubViewMode?.();
    unsubDevicePreview?.();
    unsubPlaying?.();
    // Clean up any dangling drag listeners
    window.removeEventListener("mousemove", onPanMove);
    window.removeEventListener("mouseup", onPanUp);
    window.removeEventListener("mousemove", onWindowMouseMove);
    window.removeEventListener("mouseup", onWindowMouseUp);
    if (onWindowFKey) window.removeEventListener("keydown", onWindowFKey);
  });

  function handleWheel(e: WheelEvent) {
    if (currentViewMode === "game") return;
    e.preventDefault();
    const factor = e.deltaY > 0 ? 0.9 : 1.1;
    cameraZoom = Math.max(0.05, Math.min(20, cameraZoom * factor));
    if (wasmReady) viewport_set_camera(cameraX, cameraY, cameraZoom);
  }

  // ── Asset drag-and-drop into viewport ──
  function handleViewportDragOver(e: DragEvent) {
    if (e.dataTransfer?.types.includes(ASSET_DRAG_MIME)) {
      e.preventDefault();
      e.dataTransfer.dropEffect = "copy";
    }
  }

  async function handleViewportDrop(e: DragEvent) {
    e.preventDefault();
    const raw = e.dataTransfer?.getData(ASSET_DRAG_MIME);
    if (!raw) return;
    const proj = get(project);
    if (!proj) return;

    const { path, name } = JSON.parse(raw) as { path: string; name: string };
    try {
      const entity = await createEntityFromAsset(proj.path, path, name);

      // Place entity at drop position in world coords
      if (canvasEl && entity.components.transform) {
        const rect = canvasEl.getBoundingClientRect();
        const canvasW = canvasEl.width;
        const canvasH = canvasEl.height;
        const ndcX = (e.clientX - rect.left) / rect.width;
        const ndcY = (e.clientY - rect.top) / rect.height;
        const worldX = cameraX + ((ndcX - 0.5) * canvasW) / cameraZoom;
        const worldY = cameraY + ((ndcY - 0.5) * canvasH) / cameraZoom;
        entity.components.transform.position = [
          Math.round(worldX),
          Math.round(worldY),
        ];
      }

      execute(`Drop asset "${name}"`, (s) => ({
        ...s,
        entities: [...s.entities, entity],
      }));
      selectEntity(get(scene).entities.length - 1);
      logInfo(`Created entity "${entity.name}" from ${name}`);
    } catch (err) {
      logError(`Failed to create entity from ${name}: ${err}`);
    }
  }

  function handleMouseDown(e: MouseEvent) {
    // Middle or right mouse button to pan
    if (e.button === 1 || e.button === 2) {
      if (currentViewMode === "game") return;
      isPanning = true;
      lastMouseX = e.clientX;
      lastMouseY = e.clientY;
      window.addEventListener("mousemove", onPanMove);
      window.addEventListener("mouseup", onPanUp);
      e.preventDefault();
      return;
    }

    // Polygon edit mode: left click for vertex interactions
    if (e.button === 0 && polyEditIdx >= 0 && !$isPlaying) {
      const rect = canvasEl.getBoundingClientRect();
      const localX = e.clientX - rect.left;
      const localY = e.clientY - rect.top;
      const hitIdx = findNearVertex(localX, localY);
      if (hitIdx >= 0) {
        // Start dragging a vertex
        polyDragVertex = hitIdx;
        beginDragUndo();
        polyDragStartWorld = screenToWorld(e.clientX, e.clientY);
        const verts = getPolyVertices();
        polyDragStartVerts = verts
          ? verts.map((v) => [v[0], v[1]] as [number, number])
          : [];
        window.addEventListener("mousemove", onPolyDragMove);
        window.addEventListener("mouseup", onPolyDragUp);
      } else {
        // Click on empty space: add a point into the edge it lands on
        // (or onto the end of an open line it is beyond).
        const [wx, wy] = screenToWorld(e.clientX, e.clientY);
        const t = polyTransform();
        const path = polyPath;
        const closed = polyInfo?.closed ?? true;
        if (t && path) {
          const [lx, ly] = worldToEntity(wx, wy, t);
          editEntityAt("Add point", polyEditIdx, (ent) => {
            const pts = pointsToEdit(ent.components as Record<string, unknown>, path);
            pts.splice(insertionIndex(pts, [lx, ly], closed), 0, [lx, ly]);
          });
        }
      }
      e.preventDefault();
      return;
    }

    // Left click: entity picking. Allowed during play in scene view so you
    // can click into the running world (including spawned entities, which
    // `viewport_get_entity_bounds` now covers).
    if (
      e.button === 0 &&
      !draggingGizmo &&
      (!$isPlaying || currentViewMode === "scene")
    ) {
      const [wx, wy] = screenToWorld(e.clientX, e.clientY);
      const picked = pickEntity(wx, wy);
      if (picked < 0 && !$isPlaying && currentViewMode === "scene") {
        // Nothing under the pointer: a drag selects what it boxes, a click
        // clears the selection (on release, so the two can be told apart).
        const rect = canvasEl.getBoundingClientRect();
        const x = e.clientX - rect.left;
        const y = e.clientY - rect.top;
        marquee = { x0: x, y0: y, x1: x, y1: y, add: e.ctrlKey || e.metaKey };
        window.addEventListener("mousemove", onMarqueeMove);
        window.addEventListener("mouseup", onMarqueeUp);
        return;
      }
      if (e.ctrlKey || e.metaKey) {
        if (picked >= 0) toggleEntity(picked);
      } else {
        selectEntity(picked);
      }
    }
  }

  function onMarqueeMove(e: MouseEvent) {
    if (!marquee) return;
    const rect = canvasEl.getBoundingClientRect();
    marquee = { ...marquee, x1: e.clientX - rect.left, y1: e.clientY - rect.top };
  }

  function onMarqueeUp(e: MouseEvent) {
    window.removeEventListener("mousemove", onMarqueeMove);
    window.removeEventListener("mouseup", onMarqueeUp);
    const m = marquee;
    marquee = null;
    if (!m) return;
    const rect = canvasEl.getBoundingClientRect();
    if (Math.hypot(m.x1 - m.x0, m.y1 - m.y0) < 4) {
      if (!m.add) selectEntity(-1);
      return;
    }
    const a = screenToWorld(rect.left + m.x0, rect.top + m.y0);
    const b = screenToWorld(rect.left + m.x1, rect.top + m.y1);
    try {
      const bounds = JSON.parse(viewport_get_entity_bounds());
      selectEntities(entitiesInBox(bounds, a, b), m.add);
    } catch {
      // No bounds yet: nothing selected.
    }
    void e;
  }

  function handleMouseMove(e: MouseEvent) {
    // Polygon edit mode: update hover vertex
    if (polyEditIdx >= 0 && polyDragVertex === null) {
      const rect = canvasEl.getBoundingClientRect();
      const localX = e.clientX - rect.left;
      const localY = e.clientY - rect.top;
      polyHoverVertex = findNearVertex(localX, localY);
    }
    // Gizmo drag is handled by window-level listener (onWindowMouseMove)
  }

  function handleMouseUp(_e: MouseEvent) {
    // Pan and gizmo drag release are handled by window-level listeners
  }

  // --- Pan (window-level to catch release outside viewport) ---
  function onPanMove(e: MouseEvent) {
    const dx = e.clientX - lastMouseX;
    const dy = e.clientY - lastMouseY;
    cameraX -= dx / cameraZoom;
    cameraY += dy / cameraZoom; // +Y up in world space
    lastMouseX = e.clientX;
    lastMouseY = e.clientY;
    if (wasmReady) viewport_set_camera(cameraX, cameraY, cameraZoom);
  }

  function onPanUp() {
    isPanning = false;
    window.removeEventListener("mousemove", onPanMove);
    window.removeEventListener("mouseup", onPanUp);
  }

  // --- Polygon vertex drag (window-level) ---
  function onPolyDragMove(e: MouseEvent) {
    if (polyDragVertex === null || polyEditIdx < 0) return;
    const [wx, wy] = screenToWorld(e.clientX, e.clientY);
    const t = polyTransform();
    const path = polyPath;
    if (!t || !path) return;
    const [lx, ly] = worldToEntity(wx, wy, t);
    const vi = polyDragVertex;
    editEntityAt("Move point", polyEditIdx, (ent) => {
      const pts = pointsToEdit(ent.components as Record<string, unknown>, path);
      if (vi < pts.length) pts[vi] = [lx, ly];
    });
  }

  function onPolyDragUp(_e: MouseEvent) {
    polyDragVertex = null;
    commitDragUndo();
    window.removeEventListener("mousemove", onPolyDragMove);
    window.removeEventListener("mouseup", onPolyDragUp);
  }

  // --- Joint anchor drag (window-level) ---
  function startJointAnchorDrag(
    entityIdx: number,
    anchor: "a" | "b",
    e: MouseEvent,
  ) {
    e.stopPropagation();
    e.preventDefault();
    jointDragAnchor = { entityIdx, anchor };
    beginDragUndo();
    window.addEventListener("mousemove", onJointAnchorDragMove);
    window.addEventListener("mouseup", onJointAnchorDragUp);
  }

  function onJointAnchorDragMove(e: MouseEvent) {
    if (!jointDragAnchor) return;
    const { entityIdx, anchor } = jointDragAnchor;
    const [wx, wy] = screenToWorld(e.clientX, e.clientY);
    const ent = viewEntities[entityIdx];
    if (!ent?.components.joint) return;
    const jt = ent.components.joint.joint_type;

    // Compute offset relative to the entity (anchor a) or target entity (anchor b)
    let refX = 0;
    let refY = 0;
    if (anchor === "a") {
      const t = ent.components.transform;
      if (t) {
        refX = t.position[0];
        refY = t.position[1];
      }
    } else {
      const tgtName = ent.components.joint.target_entity;
      const tgt = viewEntities.find((e2) => e2.name === tgtName);
      const t = tgt?.components.transform;
      if (t) {
        refX = t.position[0];
        refY = t.position[1];
      }
    }
    const offX = wx - refX;
    const offY = wy - refY;

    editEntityAt("Move joint anchor", entityIdx, (ent) => {
      const joint = ent.components.joint;
      if (!joint) return;
      const jtype = joint.joint_type;
      if ("Revolute" in jtype && anchor === "a") {
        jtype.Revolute.anchor = [offX, offY];
      } else if ("Fixed" in jtype) {
        if (anchor === "a") jtype.Fixed.anchor_a = [offX, offY];
        else jtype.Fixed.anchor_b = [offX, offY];
      } else if ("Prismatic" in jtype && anchor === "a") {
        jtype.Prismatic.anchor = [offX, offY];
      } else if ("Rope" in jtype) {
        if (anchor === "a") jtype.Rope.anchor_a = [offX, offY];
        else jtype.Rope.anchor_b = [offX, offY];
      } else if ("Spring" in jtype) {
        if (anchor === "a") jtype.Spring.anchor_a = [offX, offY];
        else jtype.Spring.anchor_b = [offX, offY];
      }
    });
  }

  function onJointAnchorDragUp(_e: MouseEvent) {
    jointDragAnchor = null;
    commitDragUndo();
    window.removeEventListener("mousemove", onJointAnchorDragMove);
    window.removeEventListener("mouseup", onJointAnchorDragUp);
  }

  // --- Gizmo interaction ---
  function onWindowMouseMove(e: MouseEvent) {
    handleGizmoDrag(e);
  }

  function onWindowMouseUp(_e: MouseEvent) {
    draggingGizmo = null;
    dragGroup = [];
    commitDragUndo();
    window.removeEventListener("mousemove", onWindowMouseMove);
    window.removeEventListener("mouseup", onWindowMouseUp);
  }

  function startGizmoDrag(axis: "x" | "y" | "center" | "arc", e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    const sel = $selectedEntity;
    if (sel < 0) return;
    const t = localTransform(sel);
    if (!t) return;

    draggingGizmo = axis;
    beginDragUndo();
    dragStartWorld = screenToWorld(e.clientX, e.clientY);
    dragStartValue = {
      x: t.position[0],
      y: t.position[1],
      rotation: t.rotation,
      scaleX: t.scale[0],
      scaleY: t.scale[1],
    };

    // Compute parent's world rotation for local↔world conversion during drag
    const world = worldTransformOf(sel);
    if (!world) {
      draggingGizmo = null;
      cancelDragUndo();
      return;
    }
    dragStartWorldRot = world.rotation;
    dragStartParentWorldRot = world.rotation - (t.rotation ?? 0);

    // The rest of the selection moves with it (edit mode: in play a drag
    // writes one live entity).
    dragGroup = [];
    if (!$isPlaying && $selectedEntities.size > 1) {
      const entities = viewEntities;
      const indexOf = new Map(entities.map((en, i) => [en.name, i]));
      const parentOf = (i: number) => {
        const p = entities[i]?.parent;
        return p ? (indexOf.get(p) ?? -1) : -1;
      };
      for (const i of topLevelSelection($selectedEntities, parentOf)) {
        if (i === sel) continue;
        const lt = localTransform(i);
        const wt = worldTransformOf(i);
        if (!lt || !wt) continue;
        dragGroup.push({
          idx: i,
          x: lt.position[0],
          y: lt.position[1],
          rotation: lt.rotation,
          scaleX: lt.scale[0],
          scaleY: lt.scale[1],
          parentRot: wt.rotation - lt.rotation,
        });
      }
    }

    // Attach window-level listeners so drag always ends, even outside viewport
    window.addEventListener("mousemove", onWindowMouseMove);
    window.addEventListener("mouseup", onWindowMouseUp);
  }

  function handleGizmoDrag(e: MouseEvent) {
    const sel = $selectedEntity;
    if (sel < 0 || !draggingGizmo) return;
    if (!localTransform(sel)) return;

    const [wx, wy] = screenToWorld(e.clientX, e.clientY);
    const dwx = wx - dragStartWorld[0];
    const dwy = wy - dragStartWorld[1];

    if (currentGizmoMode === "move") {
      // Convert world-space delta to local-space delta
      let localDx: number;
      let localDy: number;

      if (currentGizmoSpace === "local") {
        // Project world delta onto entity's world axes, then un-rotate by parent
        const entityWorldRot = dragStartWorldRot;
        const cosE = Math.cos(entityWorldRot);
        const sinE = Math.sin(entityWorldRot);
        // Project onto entity's local axes
        const alongX = dwx * cosE + dwy * sinE;
        const alongY = -dwx * sinE + dwy * cosE;
        // Apply axis constraint
        const ax = draggingGizmo === "y" ? 0 : alongX;
        const ay = draggingGizmo === "x" ? 0 : alongY;
        // Convert back from entity-local to parent-local
        const entityLocalRot = dragStartValue.rotation;
        const cosL = Math.cos(entityLocalRot);
        const sinL = Math.sin(entityLocalRot);
        localDx = ax * cosL - ay * sinL;
        localDy = ax * sinL + ay * cosL;
      } else {
        // Global: project world delta onto world axes, then un-rotate by parent
        const parentRot = dragStartParentWorldRot;
        const cosP = Math.cos(-parentRot);
        const sinP = Math.sin(-parentRot);
        // Apply axis constraint in world space first
        const constrainedDwx = draggingGizmo === "y" ? 0 : dwx;
        const constrainedDwy = draggingGizmo === "x" ? 0 : dwy;
        // Rotate world delta into parent's local frame
        localDx = constrainedDwx * cosP - constrainedDwy * sinP;
        localDy = constrainedDwx * sinP + constrainedDwy * cosP;
      }

      let newX = dragStartValue.x + localDx;
      let newY = dragStartValue.y + localDy;
      // Snap only the axes being dragged: an axis drag must not pull the
      // other coordinate onto the grid.
      if (snapping($snapSettings, e.ctrlKey)) {
        if (draggingGizmo !== "y") newX = snapTo(newX, $snapSettings.grid);
        if (draggingGizmo !== "x") newY = snapTo(newY, $snapSettings.grid);
      }
      applyGizmoTransform(sel, `Move entity`, (t) => {
        t.position = [newX, newY];
      });
      // The rest of the selection by the same world distance, each turned
      // into its own parent's frame.
      if (dragGroup.length > 0) {
        const pc = Math.cos(dragStartParentWorldRot);
        const ps = Math.sin(dragStartParentWorldRot);
        const ddx = newX - dragStartValue.x;
        const ddy = newY - dragStartValue.y;
        const worldDx = ddx * pc - ddy * ps;
        const worldDy = ddx * ps + ddy * pc;
        for (const g of dragGroup) {
          const c = Math.cos(-g.parentRot);
          const sn = Math.sin(-g.parentRot);
          const gx = g.x + worldDx * c - worldDy * sn;
          const gy = g.y + worldDx * sn + worldDy * c;
          applyGizmoTransform(g.idx, `Move entity`, (t) => {
            t.position = [gx, gy];
          });
        }
      }
    } else if (currentGizmoMode === "rotate") {
      // Rotation always works the same — compute angle from entity center to mouse
      const world = worldTransformOf(sel);
      if (!world) return;
      const angle = Math.atan2(wy - world.y, wx - world.x);
      const startAngle = Math.atan2(
        dragStartWorld[1] - world.y,
        dragStartWorld[0] - world.x,
      );
      let newRotation = dragStartValue.rotation + (angle - startAngle);
      if (snapping($snapSettings, e.ctrlKey)) newRotation = snapAngle(newRotation, $snapSettings.angle);
      applyGizmoTransform(sel, `Rotate entity`, (t) => {
        t.rotation = newRotation;
      });
      // Each of the rest turns by the same amount about its own pivot.
      const turned = newRotation - dragStartValue.rotation;
      for (const g of dragGroup) {
        applyGizmoTransform(g.idx, `Rotate entity`, (t) => {
          t.rotation = g.rotation + turned;
        });
      }
    } else if (currentGizmoMode === "scale") {
      let projX: number;
      let projY: number;

      if (currentGizmoSpace === "local") {
        // Project world delta onto entity's world axes
        const cosE = Math.cos(dragStartWorldRot);
        const sinE = Math.sin(dragStartWorldRot);
        projX = dwx * cosE + dwy * sinE;
        projY = -dwx * sinE + dwy * cosE;
      } else {
        projX = dwx;
        projY = dwy;
      }

      const sensitivity = 0.01;
      let newSX = dragStartValue.scaleX;
      let newSY = dragStartValue.scaleY;
      if (draggingGizmo === "x" || draggingGizmo === "center")
        newSX = Math.max(
          0.01,
          dragStartValue.scaleX + projX * sensitivity * cameraZoom,
        );
      if (draggingGizmo === "y" || draggingGizmo === "center")
        newSY = Math.max(
          0.01,
          dragStartValue.scaleY + projY * sensitivity * cameraZoom,
        );
      if (snapping($snapSettings, e.ctrlKey)) {
        newSX = Math.max(0.01, snapTo(newSX, $snapSettings.scale));
        newSY = Math.max(0.01, snapTo(newSY, $snapSettings.scale));
      }
      applyGizmoTransform(sel, `Scale entity`, (t) => {
        t.scale = [newSX, newSY];
      });
      // Each of the rest by the same factor.
      const fx = dragStartValue.scaleX !== 0 ? newSX / dragStartValue.scaleX : 1;
      const fy = dragStartValue.scaleY !== 0 ? newSY / dragStartValue.scaleY : 1;
      for (const g of dragGroup) {
        applyGizmoTransform(g.idx, `Scale entity`, (t) => {
          t.scale = [g.scaleX * fx, g.scaleY * fy];
        });
      }
    }
  }

  // --- Keyboard shortcuts for gizmo modes ---
  function handleKeyDown(e: KeyboardEvent) {
    // Every shortcut below is an unmodified key. Bailing out on a modifier
    // keeps Ctrl+R (reload) from also switching to the scale tool and
    // Ctrl+C (copy entity) from toggling collider outlines.
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    // This handler is on an element, so it runs before `playKeys` can fence
    // a game key off at `document` - it has to ask. In Scene view during
    // running play the keys below are the editor's; in Game view none are.
    if (isGameKey(e)) return;
    if (
      e.target instanceof HTMLInputElement ||
      e.target instanceof HTMLSelectElement ||
      e.target instanceof HTMLTextAreaElement
    )
      return;
    if (e.key === "F3") {
      showProfiler.update((v) => !v);
      e.preventDefault();
      return;
    }
    if (e.key === "c" || e.key === "C") {
      showColliders.update((v) => !v);
      e.preventDefault();
      return;
    }
    if (e.key === "l" || e.key === "L") {
      const next = !get(showLighting);
      showLighting.set(next);
      try {
        viewport_set_show_lighting(next);
      } catch {}
      e.preventDefault();
      return;
    }
    if (e.key === "j" || e.key === "J") {
      showJoints.update((v) => !v);
      e.preventDefault();
      return;
    }
    // Polygon edit mode keyboard shortcuts. Collider vertices are scene
    // data, so this stays edit-mode only even while paused.
    if (polyEditIdx >= 0 && !$isPlaying) {
      if (e.key === "Escape") {
        (polygonEditEntity.set(-1), polygonEditPath.set(null));
        e.preventDefault();
        return;
      }
      if (e.key === "Delete" || e.key === "Backspace") {
        // Delete hovered vertex
        if (polyHoverVertex !== null && polyHoverVertex >= 0) {
          const verts = getPolyVertices();
          const min = minVertexCount();
          if (verts && verts.length > min) {
            const vi = polyHoverVertex;
            const path = polyPath;
            if (path)
              editEntityAt("Delete point", polyEditIdx, (ent) => {
                const pts = pointsToEdit(ent.components as Record<string, unknown>, path);
                if (vi < pts.length) pts.splice(vi, 1);
              });
            polyHoverVertex = null;
          }
        }
        e.preventDefault();
        return;
      }
      return; // Swallow other keys in polygon edit mode
    }

    if (e.key === "w" || e.key === "W") {
      gizmoMode.set("move");
      e.preventDefault();
    } else if (e.key === "e" || e.key === "E") {
      gizmoMode.set("rotate");
      e.preventDefault();
    } else if (e.key === "r" || e.key === "R") {
      gizmoMode.set("scale");
      e.preventDefault();
    } else if (e.key === "t" || e.key === "T") {
      gizmoSpace.update((s) => (s === "global" ? "local" : "global"));
      e.preventDefault();
    } else if (e.key === "f" || e.key === "F") {
      // Focus camera on selected entity
      // The live transform during play, not where the scene file put it.
      const world = $selectedEntity >= 0 ? worldTransformOf($selectedEntity) : null;
      if (world) {
        cameraX = world.x;
        cameraY = world.y;
        if (wasmReady) viewport_set_camera(cameraX, cameraY, cameraZoom);
        e.preventDefault();
      }
    } else if (e.key === "Escape") {
      clearSelection();
    } else if (e.key === "Delete" || e.key === "Backspace") {
      // Scene-file edit — not something to do to a running world.
      if ($isPlaying) return;
      // Delete selected entity
      if ($selectedEntity >= 0) {
        const idx = $selectedEntity;
        execute(`Delete entity`, (sc) => {
          sc.entities.splice(idx, 1);
          return sc;
        });
        clearSelection();
      }
    }
  }

  // --- Cursor state ---
  let viewportCursor = $derived.by(() => {
    if (isPanning) return "grabbing";
    if (polyEditIdx >= 0) {
      if (polyDragVertex !== null) return "grabbing";
      if (polyHoverVertex !== null) return "pointer";
      return "crosshair";
    }
    if (draggingGizmo) return "none"; // Hide cursor while dragging — gizmo provides feedback
    if (gizmoHover) return "none"; // Hide on hover too — highlight is the feedback
    return "crosshair";
  });

  // Gizmo arm length (constant screen pixels)
  const GIZMO_ARM = 60;

  // Gizmo rotation in degrees for SVG transform (local space mode)
  let gizmoRotDeg = $derived(
    currentGizmoSpace === "local" ? (-gizmoWorldRotation * 180) / Math.PI : 0,
  );
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="viewport" onkeydown={handleKeyDown} tabindex="-1">
  <div
    class="viewport-canvas"
    ondragover={handleViewportDragOver}
    ondrop={handleViewportDrop}
  >
    {#if error}
      <div class="error-message">{error}</div>
    {:else if !loaded}
      <div class="placeholder"><p>Loading viewport...</p></div>
    {/if}
    {#if previewProfile && deviceRect}
      <DeviceOverlay
        profile={previewProfile}
        portrait={$devicePreview.portrait}
        x={deviceRect.x}
        y={deviceRect.y}
        width={deviceRect.width}
        height={deviceRect.height}
      />
    {/if}
    <canvas
      id="editor-viewport-canvas"
      bind:this={canvasEl}
      onwheel={handleWheel}
      onmousedown={handleMouseDown}
      onmousemove={(e) => {
        trackCursor(e);
        handleMouseMove(e);
      }}
      onmouseleave={() => (cursorWorld = null)}
      onmouseup={handleMouseUp}
      oncontextmenu={(e) => e.preventDefault()}
    ></canvas>

    <!-- The tools float over the picture, as in the design: a pill at the
         top left, the cursor's world point and the zoom at the top right,
         a light frame readout at the bottom left. Each takes the pointer
         only on itself, so the canvas under them still pans and picks. -->
    <div class="vp-tools" role="toolbar" aria-label="Viewport tools">
      {#if !$isPlaying || currentViewMode === "scene"}
        <button
          class="vp-btn"
          class:active={currentGizmoMode === "move"}
          onclick={() => gizmoMode.set("move")}
          title="Move (W)"
          aria-label="Move tool"><svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><path d="M7 1v12M1 7h12M7 1 5 3M7 1l2 2M7 13l-2-2M7 13l2-2M1 7l2-2M1 7l2 2M13 7l-2-2M13 7l-2 2" stroke="currentColor" stroke-width="1.3" fill="none"/></svg></button
        >
        <button
          class="vp-btn"
          class:active={currentGizmoMode === "rotate"}
          onclick={() => gizmoMode.set("rotate")}
          title="Rotate (E)"
          aria-label="Rotate tool"><svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><path d="M11.5 7A4.5 4.5 0 1 1 9.8 3.5M9.8 1v2.6h2.6" stroke="currentColor" stroke-width="1.3" fill="none"/></svg></button
        >
        <button
          class="vp-btn"
          class:active={currentGizmoMode === "scale"}
          onclick={() => gizmoMode.set("scale")}
          title="Scale (R)"
          aria-label="Scale tool"><svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><rect x="1.5" y="6.5" width="6" height="6" stroke="currentColor" stroke-width="1.3" fill="none"/><path d="M7 7l5.5-5.5M8.5 1.5h4v4" stroke="currentColor" stroke-width="1.3" fill="none"/></svg></button
        >
        <button
          class="vp-btn text"
          class:active={currentGizmoSpace === "local"}
          onclick={() =>
            gizmoSpace.update((s) => (s === "global" ? "local" : "global"))}
          title="Toggle Local/Global space (T)"
          >{currentGizmoSpace === "local" ? "Local" : "Global"}</button
        >
        <span class="vp-sep" aria-hidden="true"></span>
        <button
          class="vp-btn"
          onclick={() => {
            const proj = $project;
            if (proj) void openEntryScene(proj);
          }}
          title="Open the entry scene (Ctrl+Home)"
          aria-label="Open the entry scene"><svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><path d="M2 6.5 7 2.5l5 4V12H8.5V9h-3v3H2z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/></svg></button
        >
      {/if}
      <button
        class="vp-btn"
        class:active={$snapSettings.enabled}
        onclick={() => snapSettings.update((v) => ({ ...v, enabled: !v.enabled }))}
        title="Snap while dragging (hold Ctrl to do the opposite for one drag)"
        aria-label="Snap"><svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><path d="M3 2v5a4 4 0 0 0 8 0V2H8.5v5a1.5 1.5 0 0 1-3 0V2z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/><path d="M3 4.5h2.5M8.5 4.5H11" stroke="currentColor" stroke-width="1.2"/></svg></button
      >
      {#if $snapSettings.enabled}
        <input
          class="snap-input"
          type="number"
          min="1"
          step="1"
          value={$snapSettings.grid}
          title="Grid: world units a move snaps to"
          aria-label="Snap grid"
          onchange={(e) => {
            const v = parseFloat((e.target as HTMLInputElement).value);
            if (v > 0) snapSettings.update((s) => ({ ...s, grid: v }));
          }}
        />
        <input
          class="snap-input"
          type="number"
          min="1"
          step="5"
          value={$snapSettings.angle}
          title="Degrees a turn snaps to"
          aria-label="Snap angle"
          onchange={(e) => {
            const v = parseFloat((e.target as HTMLInputElement).value);
            if (v > 0) snapSettings.update((s) => ({ ...s, angle: v }));
          }}
        />
      {/if}
      <span class="vp-sep" aria-hidden="true"></span>
      <button
        class="vp-btn"
        class:active={$showColliders}
        onclick={() => showColliders.update((v) => !v)}
        title="Toggle collider wireframes (C)"
        aria-label="Collider wireframes"><svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><path d="M7 1.5 12 4.25v5.5L7 12.5 2 9.75v-5.5z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/></svg></button
      >
      <button
        class="vp-btn"
        class:active={$showLighting}
        onclick={() => {
          const next = !$showLighting;
          showLighting.set(next);
          try {
            viewport_set_show_lighting(next);
          } catch {}
        }}
        title="Toggle lighting preview (L)"
        aria-label="Lighting preview"><svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><path d="M7 1.5a3.6 3.6 0 0 0-2.2 6.4c.5.4.7.9.7 1.5h3c0-.6.2-1.1.7-1.5A3.6 3.6 0 0 0 7 1.5zM5.5 11h3M6 12.7h2" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" fill="none"/></svg></button
      >
      <button
        class="vp-btn"
        class:active={$showJoints}
        onclick={() => showJoints.update((v) => !v)}
        title="Toggle joint connections (J)"
        aria-label="Joint connections"><svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><path d="M5.8 8.2 8.2 5.8M6.5 3.8l1.2-1.2a2.3 2.3 0 0 1 3.3 3.3L9.8 7.1M7.5 10.2l-1.2 1.2a2.3 2.3 0 0 1-3.3-3.3l1.2-1.2" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" fill="none"/></svg></button
      >
      {#if currentViewMode === "game"}
        <span class="vp-sep" aria-hidden="true"></span>
        <DevicePicker />
      {/if}
    </div>

    <div class="vp-readout">
      {#if $isPlaying}
        <span class="playing-badge" class:paused={$isPaused}
          >{$isPaused ? "PAUSED" : "PLAYING"}</span
        >
        {#if currentViewMode === "scene"}
          <span class="live-edit-hint">edits are discarded on stop</span>
        {/if}
        <span class="vp-readout-sep" aria-hidden="true">|</span>
      {/if}
      <span title={cursorWorld ? "The cursor's world point" : "The camera's centre"}
        >x {Math.round(cursorWorld?.[0] ?? cameraX)}&nbsp; y {Math.round(cursorWorld?.[1] ?? cameraY)}</span
      >
      <span class="vp-readout-sep" aria-hidden="true">|</span>
      <span class="vp-zoom" title="Zoom">{Math.round(cameraZoom * 100)}%</span>
    </div>

    {#if !$showProfiler && loaded}
      <div class="vp-stats" title="F3 shows the full profiler">
        <span><span class:fps-good={statsFps >= 55} class:fps-warn={statsFps < 30}>{statsFps.toFixed(0)}</span> fps</span>
        <span>{statsEntityCount} entities</span>
        <span>{statsDrawCalls} draws</span>
      </div>
    {/if}


    <!-- Gizmo SVG Overlay -->
    {#if gizmoVisible && currentViewMode === "scene"}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <svg class="gizmo-overlay" onmousedown={(e) => e.stopPropagation()}>
        {#if currentGizmoMode === "move"}
          <!-- Move gizmo: X-axis (accent arrow), Y-axis (a2 arrow), center (white square) -->
          <g transform="rotate({gizmoRotDeg},{gizmoScreenX},{gizmoScreenY})">
            <!-- X axis line + arrowhead -->
            <line
              x1={gizmoScreenX}
              y1={gizmoScreenY}
              x2={gizmoScreenX + GIZMO_ARM}
              y2={gizmoScreenY}
              stroke="var(--ed-accent)"
              stroke-width={gizmoHover === "x" ? 4 : 2.5}
              opacity={gizmoHover === "x" ? 1 : 0.85}
              onmousedown={(e) => startGizmoDrag("x", e)}
              onmouseenter={() => (gizmoHover = "x")}
              onmouseleave={() => (gizmoHover = null)}
            />
            <polygon
              points="{gizmoScreenX + GIZMO_ARM},{gizmoScreenY -
                5} {gizmoScreenX +
                GIZMO_ARM +
                10},{gizmoScreenY} {gizmoScreenX + GIZMO_ARM},{gizmoScreenY +
                5}"
              fill="var(--ed-accent)"
              opacity={gizmoHover === "x" ? 1 : 0.85}
              onmousedown={(e) => startGizmoDrag("x", e)}
              onmouseenter={() => (gizmoHover = "x")}
              onmouseleave={() => (gizmoHover = null)}
            />
            <!-- Y axis line + arrowhead (screen Y is inverted: up is negative) -->
            <line
              x1={gizmoScreenX}
              y1={gizmoScreenY}
              x2={gizmoScreenX}
              y2={gizmoScreenY - GIZMO_ARM}
              stroke="var(--ed-a2)"
              stroke-width={gizmoHover === "y" ? 4 : 2.5}
              opacity={gizmoHover === "y" ? 1 : 0.85}
              onmousedown={(e) => startGizmoDrag("y", e)}
              onmouseenter={() => (gizmoHover = "y")}
              onmouseleave={() => (gizmoHover = null)}
            />
            <polygon
              points="{gizmoScreenX - 5},{gizmoScreenY -
                GIZMO_ARM} {gizmoScreenX},{gizmoScreenY -
                GIZMO_ARM -
                10} {gizmoScreenX + 5},{gizmoScreenY - GIZMO_ARM}"
              fill="var(--ed-a2)"
              opacity={gizmoHover === "y" ? 1 : 0.85}
              onmousedown={(e) => startGizmoDrag("y", e)}
              onmouseenter={() => (gizmoHover = "y")}
              onmouseleave={() => (gizmoHover = null)}
            />
            <!-- Center square -->
            <rect
              x={gizmoScreenX - 5}
              y={gizmoScreenY - 5}
              width="10"
              height="10"
              fill="white"
              fill-opacity="0.9"
              onmousedown={(e) => startGizmoDrag("center", e)}
              onmouseenter={() => (gizmoHover = "center")}
              onmouseleave={() => (gizmoHover = null)}
            />
          </g>
        {:else if currentGizmoMode === "rotate"}
          <!-- Rotate gizmo: circle arc -->
          <circle
            cx={gizmoScreenX}
            cy={gizmoScreenY}
            r={GIZMO_ARM * 0.7}
            fill="none"
            stroke="var(--ed-a3)"
            stroke-width={gizmoHover === "arc" ? 4 : 2.5}
            opacity={gizmoHover === "arc" ? 1 : 0.85}
            onmousedown={(e) => startGizmoDrag("arc", e)}
            onmouseenter={() => (gizmoHover = "arc")}
            onmouseleave={() => (gizmoHover = null)}
          />
          <!-- Center dot -->
          <circle cx={gizmoScreenX} cy={gizmoScreenY} r="3" fill="var(--ed-a3)" />
        {:else if currentGizmoMode === "scale"}
          <!-- Scale gizmo: X-axis (accent line + square), Y-axis (a2 line + square), center (white square) -->
          <g transform="rotate({gizmoRotDeg},{gizmoScreenX},{gizmoScreenY})">
            <line
              x1={gizmoScreenX}
              y1={gizmoScreenY}
              x2={gizmoScreenX + GIZMO_ARM}
              y2={gizmoScreenY}
              stroke="var(--ed-accent)"
              stroke-width={gizmoHover === "x" ? 3.5 : 2}
              opacity={gizmoHover === "x" ? 1 : 0.85}
              onmousedown={(e) => startGizmoDrag("x", e)}
              onmouseenter={() => (gizmoHover = "x")}
              onmouseleave={() => (gizmoHover = null)}
            />
            <rect
              x={gizmoScreenX + GIZMO_ARM - 4}
              y={gizmoScreenY - 4}
              width="8"
              height="8"
              fill="var(--ed-accent)"
              opacity={gizmoHover === "x" ? 1 : 0.85}
              onmousedown={(e) => startGizmoDrag("x", e)}
              onmouseenter={() => (gizmoHover = "x")}
              onmouseleave={() => (gizmoHover = null)}
            />
            <line
              x1={gizmoScreenX}
              y1={gizmoScreenY}
              x2={gizmoScreenX}
              y2={gizmoScreenY - GIZMO_ARM}
              stroke="var(--ed-a2)"
              stroke-width={gizmoHover === "y" ? 3.5 : 2}
              opacity={gizmoHover === "y" ? 1 : 0.85}
              onmousedown={(e) => startGizmoDrag("y", e)}
              onmouseenter={() => (gizmoHover = "y")}
              onmouseleave={() => (gizmoHover = null)}
            />
            <rect
              x={gizmoScreenX - 4}
              y={gizmoScreenY - GIZMO_ARM - 4}
              width="8"
              height="8"
              fill="var(--ed-a2)"
              opacity={gizmoHover === "y" ? 1 : 0.85}
              onmousedown={(e) => startGizmoDrag("y", e)}
              onmouseenter={() => (gizmoHover = "y")}
              onmouseleave={() => (gizmoHover = null)}
            />
            <!-- Center square for uniform scale -->
            <rect
              x={gizmoScreenX - 5}
              y={gizmoScreenY - 5}
              width="10"
              height="10"
              fill="white"
              fill-opacity="0.9"
              onmousedown={(e) => startGizmoDrag("center", e)}
              onmouseenter={() => (gizmoHover = "center")}
              onmouseleave={() => (gizmoHover = null)}
            />
          </g>
        {/if}
      </svg>
    {/if}

    <!-- Collider Wireframe Overlay -->
    {#if $showColliders && colliderBounds.length > 0 && currentViewMode === "scene"}
      <svg class="collider-overlay">
        {#each colliderBounds as col}
          {#if col.shape === "box"}
            {@const hw = (col.w ?? 0) / 2}
            {@const hh = (col.h ?? 0) / 2}
            {@const [cx, cy] = worldToLocal(col.x, col.y)}
            {@const [ex, _] = worldToLocal(col.x + hw, col.y)}
            {@const [__, ey] = worldToLocal(col.x, col.y + hh)}
            {@const sw = Math.abs(ex - cx) * 2}
            {@const sh = Math.abs(ey - cy) * 2}
            <rect
              x={cx - sw / 2}
              y={cy - sh / 2}
              width={sw}
              height={sh}
              fill="none"
              stroke="var(--ed-ok)"
              stroke-width="1.5"
              stroke-dasharray="4,3"
              opacity="0.7"
              transform="rotate({(-col.rotation * 180) / Math.PI},{cx},{cy})"
            />
          {:else if col.shape === "circle"}
            {@const [cx, cy] = worldToLocal(col.x, col.y)}
            {@const [ex, _] = worldToLocal(col.x + (col.radius ?? 0), col.y)}
            <circle
              {cx}
              {cy}
              r={Math.abs(ex - cx)}
              fill="none"
              stroke="var(--ed-ok)"
              stroke-width="1.5"
              stroke-dasharray="4,3"
              opacity="0.7"
            />
          {:else if col.shape === "capsule"}
            {@const [cx, cy] = worldToLocal(col.x, col.y)}
            {@const [ex, _] = worldToLocal(col.x + (col.radius ?? 0), col.y)}
            {@const [__, ey] = worldToLocal(
              col.x,
              col.y + (col.half_height ?? 0),
            )}
            {@const r = Math.abs(ex - cx)}
            {@const hh = Math.abs(ey - cy)}
            <rect
              x={cx - r}
              y={cy - hh}
              width={r * 2}
              height={hh * 2}
              rx={r}
              ry={r}
              fill="none"
              stroke="var(--ed-ok)"
              stroke-width="1.5"
              stroke-dasharray="4,3"
              opacity="0.7"
              transform="rotate({(-col.rotation * 180) / Math.PI},{cx},{cy})"
            />
          {:else if col.shape === "segment"}
            {@const ax = col.x + (col.a?.[0] ?? 0)}
            {@const ay = col.y + (col.a?.[1] ?? 0)}
            {@const bx = col.x + (col.b?.[0] ?? 0)}
            {@const by = col.y + (col.b?.[1] ?? 0)}
            {@const [lax, lay] = worldToLocal(ax, ay)}
            {@const [lbx, lby] = worldToLocal(bx, by)}
            <line
              x1={lax}
              y1={lay}
              x2={lbx}
              y2={lby}
              stroke="var(--ed-ok)"
              stroke-width="1.5"
              stroke-dasharray="4,3"
              opacity="0.7"
            />
          {:else if col.shape === "triangle"}
            {@const verts = [
              col.a ?? [0, 0],
              col.b ?? [0, 0],
              col.c ?? [0, 0],
            ] as [number, number][]}
            {@const pts = verts
              .map((v) => worldToLocal(col.x + v[0], col.y + v[1]))
              .map((p) => `${p[0]},${p[1]}`)
              .join(" ")}
            <polygon
              points={pts}
              fill="none"
              stroke="var(--ed-ok)"
              stroke-width="1.5"
              stroke-dasharray="4,3"
              opacity="0.7"
            />
          {:else if col.shape === "convex_polygon" || col.shape === "convex_decomposition"}
            {@const verts = col.vertices ?? []}
            {#if verts.length >= 3}
              {@const pts = verts
                .map((v) => worldToLocal(col.x + v[0], col.y + v[1]))
                .map((p) => `${p[0]},${p[1]}`)
                .join(" ")}
              <polygon
                points={pts}
                fill="none"
                stroke={col.shape === "convex_decomposition"
                  ? "var(--ed-a3)"
                  : "var(--ed-ok)"}
                stroke-width="1.5"
                stroke-dasharray="4,3"
                opacity="0.7"
              />
            {/if}
          {:else if col.shape === "polyline"}
            {@const verts = col.vertices ?? []}
            {#if verts.length >= 2}
              {@const pts = verts
                .map((v) => worldToLocal(col.x + v[0], col.y + v[1]))
                .map((p) => `${p[0]},${p[1]}`)
                .join(" ")}
              <polyline
                points={pts}
                fill="none"
                stroke="var(--ed-ok)"
                stroke-width="1.5"
                stroke-dasharray="4,3"
                opacity="0.7"
              />
            {/if}
          {:else if col.shape === "heightfield"}
            {@const heights = col.heights ?? []}
            {@const scaleX = col.scale?.[0] ?? 1}
            {@const scaleY = col.scale?.[1] ?? 1}
            {#if heights.length >= 2}
              {@const totalW = scaleX}
              {@const step = totalW / (heights.length - 1)}
              {@const pts = heights
                .map((h, i) =>
                  worldToLocal(
                    col.x - totalW / 2 + i * step,
                    col.y + h * scaleY,
                  ),
                )
                .map((p) => `${p[0]},${p[1]}`)
                .join(" ")}
              <polyline
                points={pts}
                fill="none"
                stroke="var(--ed-ok)"
                stroke-width="1.5"
                stroke-dasharray="4,3"
                opacity="0.7"
              />
            {/if}
          {/if}
        {/each}
      </svg>
    {/if}

    <!-- Joint Connection Overlay -->
    {#if ($showJoints || $selectedEntity >= 0) && currentViewMode === "scene"}
      {@const jointDraws = (() => {
        const draws: Array<{
          entityIdx: number;
          srcX: number;
          srcY: number;
          tgtX: number;
          tgtY: number;
          anchorAx: number;
          anchorAy: number;
          anchorBx: number;
          anchorBy: number;
          type: string;
          color: string;
          selected: boolean;
          axisX?: number;
          axisY?: number;
          limits?: [number, number];
          maxDist?: number;
          hasAnchorB: boolean;
        }> = [];
        const entities = viewEntities;
        for (let i = 0; i < entities.length; i++) {
          const ent = entities[i];
          const joint = ent.components.joint;
          if (!joint) continue;
          // Only show if showJoints is on, or this entity is selected
          if (!$showJoints && $selectedEntity !== i) continue;
          const srcT = ent.components.transform;
          if (!srcT) continue;
          const srcX = srcT.position[0];
          const srcY = srcT.position[1];
          // Find target entity
          const tgtIdx = entities.findIndex(
            (e) => e.name === joint.target_entity,
          );
          const tgtT =
            tgtIdx >= 0 ? entities[tgtIdx].components.transform : null;
          const tgtX = tgtT ? tgtT.position[0] : srcX;
          const tgtY = tgtT ? tgtT.position[1] : srcY;
          const jt = joint.joint_type;
          let type = "unknown";
          let color = "var(--ed-faint)";
          let anchorAx = srcX,
            anchorAy = srcY;
          let anchorBx = tgtX,
            anchorBy = tgtY;
          let axisX: number | undefined;
          let axisY: number | undefined;
          let limits: [number, number] | undefined;
          let maxDist: number | undefined;
          let hasAnchorB = false;
          if ("Revolute" in jt) {
            type = "revolute";
            color = "var(--ed-a2)";
            anchorAx = srcX + jt.Revolute.anchor[0];
            anchorAy = srcY + jt.Revolute.anchor[1];
            anchorBx = anchorAx;
            anchorBy = anchorAy;
            if (jt.Revolute.limits) limits = jt.Revolute.limits;
          } else if ("Fixed" in jt) {
            type = "fixed";
            color = "var(--ed-muted)";
            hasAnchorB = true;
            anchorAx = srcX + jt.Fixed.anchor_a[0];
            anchorAy = srcY + jt.Fixed.anchor_a[1];
            anchorBx = tgtX + jt.Fixed.anchor_b[0];
            anchorBy = tgtY + jt.Fixed.anchor_b[1];
          } else if ("Prismatic" in jt) {
            type = "prismatic";
            color = "var(--ed-a3)";
            anchorAx = srcX + jt.Prismatic.anchor[0];
            anchorAy = srcY + jt.Prismatic.anchor[1];
            anchorBx = anchorAx;
            anchorBy = anchorAy;
            axisX = jt.Prismatic.axis[0];
            axisY = jt.Prismatic.axis[1];
            if (jt.Prismatic.limits) limits = jt.Prismatic.limits;
          } else if ("Rope" in jt) {
            type = "rope";
            color = "var(--ed-sun1)";
            hasAnchorB = true;
            anchorAx = srcX + jt.Rope.anchor_a[0];
            anchorAy = srcY + jt.Rope.anchor_a[1];
            anchorBx = tgtX + jt.Rope.anchor_b[0];
            anchorBy = tgtY + jt.Rope.anchor_b[1];
            maxDist = jt.Rope.max_distance;
          } else if ("Spring" in jt) {
            type = "spring";
            color = "var(--ed-accent)";
            hasAnchorB = true;
            anchorAx = srcX + jt.Spring.anchor_a[0];
            anchorAy = srcY + jt.Spring.anchor_a[1];
            anchorBx = tgtX + jt.Spring.anchor_b[0];
            anchorBy = tgtY + jt.Spring.anchor_b[1];
          }
          draws.push({
            entityIdx: i,
            srcX,
            srcY,
            tgtX,
            tgtY,
            anchorAx,
            anchorAy,
            anchorBx,
            anchorBy,
            type,
            color,
            selected: $selectedEntity === i,
            axisX,
            axisY,
            limits,
            maxDist,
            hasAnchorB,
          });
        }
        return draws;
      })()}
      {#if jointDraws.length > 0}
        <svg class="joint-overlay">
          {#each jointDraws as jd}
            {@const [ax, ay] = worldToLocal(jd.anchorAx, jd.anchorAy)}
            {@const [bx, by] = worldToLocal(jd.anchorBx, jd.anchorBy)}
            {@const [sx, sy] = worldToLocal(jd.srcX, jd.srcY)}
            {@const [tx, ty] = worldToLocal(jd.tgtX, jd.tgtY)}
            <!-- Connection line from source entity to anchor A -->
            <line
              x1={sx}
              y1={sy}
              x2={ax}
              y2={ay}
              stroke={jd.color}
              stroke-width="1"
              stroke-dasharray="3,3"
              opacity="0.5"
            />
            <!-- Connection line from target entity to anchor B -->
            <line
              x1={tx}
              y1={ty}
              x2={bx}
              y2={by}
              stroke={jd.color}
              stroke-width="1"
              stroke-dasharray="3,3"
              opacity="0.5"
            />

            {#if jd.type === "revolute"}
              <!-- Revolute: circle at anchor point -->
              <circle
                cx={ax}
                cy={ay}
                r="8"
                fill="none"
                stroke={jd.color}
                stroke-width={jd.selected ? 2.5 : 1.5}
                opacity="0.9"
              />
              <circle cx={ax} cy={ay} r="3" fill={jd.color} opacity="0.8" />
              {#if jd.limits}
                <!-- Limit arc indicators -->
                {@const r = 14}
                {@const startAngle = -jd.limits[0]}
                {@const endAngle = -jd.limits[1]}
                {@const x1 = ax + r * Math.cos(startAngle)}
                {@const y1 = ay + r * Math.sin(startAngle)}
                {@const x2 = ax + r * Math.cos(endAngle)}
                {@const y2 = ay + r * Math.sin(endAngle)}
                <path
                  d="M {x1} {y1} A {r} {r} 0 0 0 {x2} {y2}"
                  fill="none"
                  stroke={jd.color}
                  stroke-width="1"
                  stroke-dasharray="2,2"
                  opacity="0.6"
                />
              {/if}
            {:else if jd.type === "fixed"}
              <!-- Fixed: solid line with lock squares -->
              <line
                x1={ax}
                y1={ay}
                x2={bx}
                y2={by}
                stroke={jd.color}
                stroke-width={jd.selected ? 2.5 : 1.5}
                opacity="0.8"
              />
              <rect
                x={ax - 4}
                y={ay - 4}
                width="8"
                height="8"
                fill={jd.color}
                opacity="0.8"
              />
              <rect
                x={bx - 4}
                y={by - 4}
                width="8"
                height="8"
                fill={jd.color}
                opacity="0.8"
              />
            {:else if jd.type === "prismatic"}
              <!-- Prismatic: axis line with arrows -->
              {@const axLen = 60}
              {@const normLen =
                Math.sqrt((jd.axisX ?? 1) ** 2 + (jd.axisY ?? 0) ** 2) || 1}
              {@const nx = (jd.axisX ?? 1) / normLen}
              {@const ny = (jd.axisY ?? 0) / normLen}
              {@const [p1x, p1y] = worldToLocal(
                jd.anchorAx - nx * axLen,
                jd.anchorAy - ny * axLen,
              )}
              {@const [p2x, p2y] = worldToLocal(
                jd.anchorAx + nx * axLen,
                jd.anchorAy + ny * axLen,
              )}
              <line
                x1={p1x}
                y1={p1y}
                x2={p2x}
                y2={p2y}
                stroke={jd.color}
                stroke-width={jd.selected ? 2.5 : 1.5}
                stroke-dasharray="6,3"
                opacity="0.8"
              />
              <!-- Arrow at end -->
              {@const angle = Math.atan2(p2y - p1y, p2x - p1x)}
              <polygon
                points="{p2x},{p2y} {p2x - 8 * Math.cos(angle - 0.4)},{p2y -
                  8 * Math.sin(angle - 0.4)} {p2x -
                  8 * Math.cos(angle + 0.4)},{p2y - 8 * Math.sin(angle + 0.4)}"
                fill={jd.color}
                opacity="0.8"
              />
              <circle cx={ax} cy={ay} r="4" fill={jd.color} opacity="0.8" />
            {:else if jd.type === "rope"}
              <!-- Rope: dashed line with length indicator -->
              <line
                x1={ax}
                y1={ay}
                x2={bx}
                y2={by}
                stroke={jd.color}
                stroke-width={jd.selected ? 2.5 : 1.5}
                stroke-dasharray="8,4"
                opacity="0.8"
              />
              <circle cx={ax} cy={ay} r="4" fill={jd.color} opacity="0.8" />
              <circle
                cx={bx}
                cy={by}
                r="4"
                fill="none"
                stroke={jd.color}
                stroke-width="2"
                opacity="0.8"
              />
              {#if jd.maxDist}
                {@const midX = (ax + bx) / 2}
                {@const midY = (ay + by) / 2}
                <text
                  x={midX}
                  y={midY - 6}
                  fill={jd.color}
                  font-size="10"
                  text-anchor="middle"
                  opacity="0.7">{jd.maxDist.toFixed(0)}px</text
                >
              {/if}
            {:else if jd.type === "spring"}
              <!-- Spring: zigzag line between anchors -->
              {@const dx = bx - ax}
              {@const dy = by - ay}
              {@const len = Math.sqrt(dx * dx + dy * dy) || 1}
              {@const segments = 8}
              {@const amplitude = 6}
              {@const perpX = -dy / len}
              {@const perpY = dx / len}
              {@const pts = Array.from({ length: segments + 1 }, (_, k) => {
                const t = k / segments;
                const baseX = ax + dx * t;
                const baseY = ay + dy * t;
                const offset =
                  k === 0 || k === segments
                    ? 0
                    : k % 2 === 0
                      ? -amplitude
                      : amplitude;
                return `${baseX + perpX * offset},${baseY + perpY * offset}`;
              }).join(" ")}
              <polyline
                points={pts}
                fill="none"
                stroke={jd.color}
                stroke-width={jd.selected ? 2.5 : 1.5}
                opacity="0.8"
              />
              <circle cx={ax} cy={ay} r="4" fill={jd.color} opacity="0.8" />
              <circle
                cx={bx}
                cy={by}
                r="4"
                fill="none"
                stroke={jd.color}
                stroke-width="2"
                opacity="0.8"
              />
            {/if}

            <!-- Draggable anchor handles (only when selected) -->
            {#if jd.selected}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <circle
                cx={ax}
                cy={ay}
                r="8"
                fill="transparent"
                style="pointer-events: all; cursor: grab;"
                onmousedown={(e) => startJointAnchorDrag(jd.entityIdx, "a", e)}
              />
              {#if jd.hasAnchorB}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <circle
                  cx={bx}
                  cy={by}
                  r="8"
                  fill="transparent"
                  style="pointer-events: all; cursor: grab;"
                  onmousedown={(e) =>
                    startJointAnchorDrag(jd.entityIdx, "b", e)}
                />
              {/if}
            {/if}
          {/each}
        </svg>
      {/if}
    {/if}

    <!-- Polygon Edit Mode Overlay -->
    {#if polyEditIdx >= 0 && !$isPlaying}
      {@const editT = polyTransform()}
      {@const editVerts = getPolyVertices()}
      {#if editT && editVerts && editVerts.length > 0}
        {@const isClosed = polyInfo?.closed ?? true}
        <svg class="poly-edit-overlay">
          <!-- Edges -->
          {#each editVerts as v, i}
            {#if i < editVerts.length - 1}
              {@const [ax, ay] = worldToLocal(
                ...entityToWorld(v[0], v[1], editT),
              )}
              {@const [bx, by] = worldToLocal(
                ...entityToWorld(
                  editVerts[i + 1][0],
                  editVerts[i + 1][1],
                  editT,
                ),
              )}
              <line
                x1={ax}
                y1={ay}
                x2={bx}
                y2={by}
                stroke="var(--ed-ok)"
                stroke-width="2"
                opacity="0.9"
              />
            {/if}
          {/each}
          <!-- Closing edge for closed shapes -->
          {#if isClosed && editVerts.length >= 3}
            {@const [ax, ay] = worldToLocal(
              ...entityToWorld(
                editVerts[editVerts.length - 1][0],
                editVerts[editVerts.length - 1][1],
                editT,
              ),
            )}
            {@const [bx, by] = worldToLocal(
              ...entityToWorld(editVerts[0][0], editVerts[0][1], editT),
            )}
            <line
              x1={ax}
              y1={ay}
              x2={bx}
              y2={by}
              stroke="var(--ed-ok)"
              stroke-width="2"
              opacity="0.9"
            />
          {/if}
          <!-- Vertex handles -->
          {#each editVerts as v, i}
            {@const [sx, sy] = worldToLocal(
              ...entityToWorld(v[0], v[1], editT),
            )}
            <rect
              x={sx - 4}
              y={sy - 4}
              width={8}
              height={8}
              fill={polyDragVertex === i
                ? "var(--ed-accent)"
                : polyHoverVertex === i
                  ? "var(--ed-ink)"
                  : "var(--ed-ok)"}
              stroke="#000"
              stroke-width="1"
              rx="1"
            />
          {/each}
        </svg>
      {/if}
      <!-- Floating toolbar -->
      <div class="poly-edit-toolbar">
        <span class="poly-edit-label"
          >Editing {polyPath ?? "points"}{editVertsHint()}</span
        >
        <button class="poly-edit-done" onclick={() => (polygonEditEntity.set(-1), polygonEditPath.set(null))}
          >Done</button
        >
        <span class="poly-edit-hint"
          >Click to add • Drag to move • Del to remove • Esc to exit</span
        >
      </div>
    {/if}

    {#if marquee}
      <svg class="camera-preview-overlay">
        <rect
          x={Math.min(marquee.x0, marquee.x1)}
          y={Math.min(marquee.y0, marquee.y1)}
          width={Math.abs(marquee.x1 - marquee.x0)}
          height={Math.abs(marquee.y1 - marquee.y0)}
          fill="var(--ed-accent)"
          fill-opacity="0.08"
          stroke="var(--ed-accent)"
          stroke-width="1"
          stroke-dasharray="4,3"
        />
      </svg>
    {/if}

    <!-- Selection gizmos: what the selected entity reaches -->
    {#if selectionGizmos.length > 0 && canvasEl && currentViewMode === "scene"}
      <svg class="camera-preview-overlay">
        {#each selectionGizmos as g}
          {#if g.kind === "circle"}
            {@const c = worldToLocal(g.center[0], g.center[1])}
            {@const edge = worldToLocal(g.center[0] + g.radius, g.center[1])}
            {@const r = Math.abs(edge[0] - c[0])}
            <circle
              cx={c[0]}
              cy={c[1]}
              {r}
              fill="none"
              stroke={g.color}
              stroke-width="1.5"
              stroke-dasharray={g.dashed ? "6,4" : "none"}
              opacity="0.75"
            />
            {#if g.label}
              <text x={c[0] + r * 0.71 + 3} y={c[1] - r * 0.71 - 3} fill={g.color} font-size="10" font-family="monospace" opacity="0.8">{g.label}</text>
            {/if}
          {:else}
            {@const pts = g.points.map((p) => worldToLocal(p[0], p[1]))}
            {#if g.closed}
              <polygon
                points={pts.map((p) => `${p[0]},${p[1]}`).join(" ")}
                fill={g.color}
                fill-opacity="0.06"
                stroke={g.color}
                stroke-width="1.5"
                stroke-dasharray={g.dashed ? "6,4" : "none"}
                opacity="0.8"
              />
            {:else}
              <polyline
                points={pts.map((p) => `${p[0]},${p[1]}`).join(" ")}
                fill="none"
                stroke={g.color}
                stroke-width="1.5"
                stroke-dasharray={g.dashed ? "6,4" : "none"}
                opacity="0.8"
              />
            {/if}
            {#if g.label && pts.length > 0}
              <text x={pts[0][0] + 4} y={pts[0][1] - 4} fill={g.color} font-size="10" font-family="monospace" opacity="0.8">{g.label}</text>
            {/if}
          {/if}
        {/each}
      </svg>
    {/if}

    <!-- Camera Preview Overlay -->
    {#if cameraPreview && canvasEl && currentViewMode === "scene"}
      {@const tl = worldToLocal(
        cameraPreview.cx - cameraPreview.halfW,
        cameraPreview.cy + cameraPreview.halfH,
      )}
      {@const br = worldToLocal(
        cameraPreview.cx + cameraPreview.halfW,
        cameraPreview.cy - cameraPreview.halfH,
      )}
      {@const pw = br[0] - tl[0]}
      {@const ph = br[1] - tl[1]}
      {@const cc = worldToLocal(cameraPreview.cx, cameraPreview.cy)}
      <svg class="camera-preview-overlay">
        <!-- Viewport rectangle -->
        <rect
          x={tl[0]}
          y={tl[1]}
          width={pw}
          height={ph}
          fill="none"
          stroke={cameraPreview.isActive ? "var(--ed-accent)" : "var(--ed-faint)"}
          stroke-width="2"
          stroke-dasharray={cameraPreview.isActive ? "none" : "6,4"}
          opacity="0.8"
        />
        <!-- Label -->
        <text
          x={tl[0] + 4}
          y={tl[1] - 5}
          fill={cameraPreview.isActive ? "var(--ed-accent)" : "var(--ed-faint)"}
          font-size="11"
          font-family="monospace"
          opacity="0.9"
          >{cameraPreview.isActive
            ? "🎥 Active Camera"
            : "🎥 Camera (inactive)"}</text
        >
        <!-- Crosshair at camera center -->
        <line
          x1={cc[0] - 8}
          y1={cc[1]}
          x2={cc[0] + 8}
          y2={cc[1]}
          stroke={cameraPreview.isActive ? "var(--ed-accent)" : "var(--ed-faint)"}
          stroke-width="1"
          opacity="0.5"
        />
        <line
          x1={cc[0]}
          y1={cc[1] - 8}
          x2={cc[0]}
          y2={cc[1] + 8}
          stroke={cameraPreview.isActive ? "var(--ed-accent)" : "var(--ed-faint)"}
          stroke-width="1"
          opacity="0.5"
        />
      </svg>
      <!-- PiP label -->
      {@const pipW = Math.min(320, Math.floor(canvasEl.width * 0.3))}
      <div
        class="pip-label"
        style="right: {8 +
          pipW / 2}px; bottom: 8px; transform: translateX(50%);"
      >
        🎥 Camera Preview
      </div>
    {/if}

    <!-- Debug Draw Overlay -->
    {#if debugDrawCommands.length > 0}
      <svg class="debug-draw-overlay">
        {#each debugDrawCommands as dd}
          {#if dd.t === "ddl"}
            {@const [lx1, ly1] = worldToLocal(dd.x1 ?? 0, dd.y1 ?? 0)}
            {@const [lx2, ly2] = worldToLocal(dd.x2 ?? 0, dd.y2 ?? 0)}
            <line
              x1={lx1}
              y1={ly1}
              x2={lx2}
              y2={ly2}
              stroke="rgb({Math.round(dd.r * 255)},{Math.round(
                dd.g * 255,
              )},{Math.round(dd.b * 255)})"
              stroke-width="1.5"
            />
          {:else if dd.t === "ddr"}
            {@const rx = dd.x ?? 0}
            {@const ry = dd.y ?? 0}
            {@const rw = dd.w ?? 0}
            {@const rh = dd.h ?? 0}
            {@const [rx1, ry1] = worldToLocal(rx, ry)}
            {@const [rx2, ry2] = worldToLocal(rx + rw, ry + rh)}
            <rect
              x={Math.min(rx1, rx2)}
              y={Math.min(ry1, ry2)}
              width={Math.abs(rx2 - rx1)}
              height={Math.abs(ry2 - ry1)}
              fill="none"
              stroke="rgb({Math.round(dd.r * 255)},{Math.round(
                dd.g * 255,
              )},{Math.round(dd.b * 255)})"
              stroke-width="1.5"
            />
          {:else if dd.t === "ddc"}
            {@const cx0 = dd.x ?? 0}
            {@const cy0 = dd.y ?? 0}
            {@const crad = dd.rad ?? 0}
            {@const [cx, cy] = worldToLocal(cx0, cy0)}
            {@const [ex, _ey] = worldToLocal(cx0 + crad, cy0)}
            <circle
              {cx}
              {cy}
              r={Math.abs(ex - cx)}
              fill="none"
              stroke="rgb({Math.round(dd.r * 255)},{Math.round(
                dd.g * 255,
              )},{Math.round(dd.b * 255)})"
              stroke-width="1.5"
            />
          {/if}
        {/each}
      </svg>
    {/if}

    <!-- Game View badge -->
    {#if currentViewMode === "game"}
      <div class="game-view-badge">GAME</div>
    {/if}

    <!-- Profiler overlay (absolute, no layout impact) -->
    {#if $showProfiler}
      <div class="profiler-panel">
        <div class="profiler-row">
          <div class="profiler-section">
            <span class="profiler-label">FPS</span>
            <span
              class="profiler-value"
              class:fps-warn={statsFps < 30}
              class:fps-good={statsFps >= 55}>{statsFps.toFixed(0)}</span
            >
          </div>
          <div class="profiler-section">
            <span class="profiler-label">Frame</span>
            <span class="profiler-value" title="Time between frames. Bounded below by the display refresh rate, so on a high-refresh monitor this is the vsync interval, not the cost of the frame.">{statsFrameTime.toFixed(2)} ms</span>
          </div>
          <div class="profiler-section">
            <span class="profiler-label">CPU</span>
            <span class="profiler-value" title="Wall time actually spent in the render callback: engine work plus the editor's own per-frame cost. This is the number to watch — Frame includes waiting for vsync.">{statsCpuMs.toFixed(2)} ms</span>
          </div>
          <div class="profiler-sep"></div>
          <div class="profiler-section">
            <span class="profiler-label">Scripts</span>
            <span class="profiler-value script-color"
              >{statsScriptTime.toFixed(2)} ms</span
            >
          </div>
          <div class="profiler-section">
            <span class="profiler-label">Physics</span>
            <span class="profiler-value physics-color"
              >{statsPhysicsTime.toFixed(2)} ms</span
            >
          </div>
          <div class="profiler-section">
            <span class="profiler-label">Memory</span>
            <span
              class="profiler-value"
              title="Viewport WASM linear memory, then live heap and its peak. Reserved memory never shrinks, so watch the live figure to tell a leak from allocator high-water. Editor play mode, not the shipped build."
              >{(statsMemoryBytes / 1048576).toFixed(1)} MB
              {#if statsMemoryLive > 0}
                <span style="opacity:0.6"
                  >({(statsMemoryLive / 1048576).toFixed(1)} live{statsMemoryPeak >
                  0
                    ? `, ${(statsMemoryPeak / 1048576).toFixed(1)} peak`
                    : ""})</span
                >
              {:else if statsMemoryPeak > 0}
                <span style="opacity:0.6"
                  >({(statsMemoryPeak / 1048576).toFixed(1)} peak)</span
                >
              {/if}</span
            >
          </div>
          <div class="profiler-section">
            <span class="profiler-label">Update</span>
            <span class="profiler-value"
              >{statsUpdateTime.toFixed(2)} ms</span
            >
          </div>
          <div class="profiler-section">
            <span class="profiler-label">Render</span>
            <span class="profiler-value render-color"
              >{statsRenderTime.toFixed(2)} ms</span
            >
          </div>
          <div class="profiler-sep"></div>
          <div class="profiler-section">
            <span class="profiler-label">Draws</span>
            <span class="profiler-value">{statsDrawCalls}</span>
          </div>
          <div class="profiler-section">
            <span class="profiler-label">Sprites</span>
            <span class="profiler-value">{statsSpriteCount}</span>
          </div>
          <div class="profiler-section">
            <span class="profiler-label">Entities</span>
            <span class="profiler-value">{statsEntityCount}</span>
          </div>
        </div>
        {#if true}
          <!--
            Proportioned against CPU time, not frame time. Frame time is
            bounded below by the display refresh, so on a high-refresh monitor
            most of this bar used to be "other" — and that "other" was the
            browser waiting for vsync, not anything the engine or editor did.
            Against CPU time the leftover segment means what a reader assumes
            it means: the editor's own per-frame work, outside the engine's
            phases. `statsCpuMs` can lag the phases by a poll, hence the floor.
          -->
          {@const barTotal = Math.max(
            statsCpuMs,
            statsScriptTime + statsPhysicsTime + statsRenderTime,
            0.01,
          )}
          {@const scriptPct = (statsScriptTime / barTotal) * 100}
          {@const physicsPct = (statsPhysicsTime / barTotal) * 100}
          {@const renderPct = (statsRenderTime / barTotal) * 100}
          {@const otherPct = Math.max(
            0,
            100 - scriptPct - physicsPct - renderPct,
          )}
          <div class="profiler-bar">
            <div
              class="bar-segment bar-script"
              style="width:{scriptPct}%"
            ></div>
            <div
              class="bar-segment bar-physics"
              style="width:{physicsPct}%"
            ></div>
            <div
              class="bar-segment bar-render"
              style="width:{renderPct}%"
            ></div>
            <div class="bar-segment bar-other" style="width:{otherPct}%"></div>
          </div>
          {@const maxTime = Math.max(16.7, ...frameHistory, 1)}
          <svg
            class="frame-graph"
            viewBox="0 0 120 50"
            preserveAspectRatio="none"
          >
            <line
              x1="0"
              y1={50 - (16.7 / maxTime) * 50}
              x2="120"
              y2={50 - (16.7 / maxTime) * 50}
              stroke="var(--ed-ok)"
              stroke-width="0.5"
              stroke-dasharray="2,2"
              opacity="0.5"
            />
            <line
              x1="0"
              y1={50 - (33.3 / maxTime) * 50}
              x2="120"
              y2={50 - (33.3 / maxTime) * 50}
              stroke="var(--ed-err)"
              stroke-width="0.5"
              stroke-dasharray="2,2"
              opacity="0.4"
            />
            {#if frameHistory.length > 1}
              <polyline
                fill="none"
                stroke="var(--ed-accent)"
                stroke-width="1"
                points={frameHistory
                  .map(
                    (t, i) =>
                      `${(i / (frameHistory.length - 1)) * 120},${50 - (t / maxTime) * 50}`,
                  )
                  .join(" ")}
              />
            {/if}
          </svg>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .viewport {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--ed-crust);
    outline: none;
  }

  /* Overlays over the picture: a translucent crust pill, as in the design. */
  .vp-tools,
  .vp-readout,
  .vp-stats {
    position: absolute;
    z-index: 15;
    display: flex;
    align-items: center;
    background: color-mix(in srgb, var(--ed-crust) 82%, transparent);
    border: 1px solid var(--ed-line);
    border-radius: 7px;
    backdrop-filter: blur(8px);
  }

  .vp-tools {
    top: 10px;
    left: 10px;
    gap: 2px;
    padding: 3px;
    max-width: calc(100% - 260px);
    flex-wrap: wrap;
  }

  .vp-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: 5px;
    background: none;
    color: var(--ed-muted);
    cursor: pointer;
  }

  .vp-btn.text {
    width: auto;
    padding: 0 8px;
    font-family: var(--ed-sans);
    font-size: 11px;
  }

  .vp-btn:hover {
    background: var(--ed-hover);
    color: var(--ed-ink);
  }

  .vp-btn.active {
    background: var(--ed-accent-low);
    color: var(--ed-accent-high);
  }

  .vp-sep {
    width: 1px;
    height: 18px;
    margin: 0 3px;
    background: var(--ed-line);
  }

  .vp-readout {
    top: 10px;
    right: 10px;
    gap: 10px;
    padding: 5px 10px;
    font-family: var(--ed-mono);
    font-size: 11px;
    color: var(--ed-muted);
    white-space: nowrap;
  }

  .vp-readout-sep {
    color: var(--ed-line-strong);
  }

  .vp-zoom {
    color: var(--ed-text);
  }

  .vp-stats {
    left: 10px;
    bottom: 10px;
    gap: 12px;
    padding: 5px 10px;
    font-family: var(--ed-mono);
    font-size: 11px;
    color: var(--ed-muted);
    pointer-events: none;
  }

  .vp-stats .fps-good {
    color: var(--ed-ok);
  }

  .vp-stats .fps-warn {
    color: var(--ed-err);
  }

  .playing-badge {
    background: var(--ed-ok);
    color: var(--ed-on-accent);
    font-size: 9px;
    font-weight: 700;
    padding: 1px 6px;
    border-radius: 3px;
    letter-spacing: 0.5px;
  }

  .playing-badge.paused {
    background: var(--ed-warn);
  }

  .live-edit-hint {
    font-family: var(--ed-sans);
    font-size: 10px;
    color: var(--ed-faint);
  }

  .viewport-canvas {
    flex: 1;
    position: relative;
    overflow: hidden;
    /* So the overlays can tell a short viewport (container query below). */
    container-type: size;
  }

  /* Too short for the stats chip to clear the tool pill above it. */
  @container (max-height: 160px) {
    .vp-stats {
      display: none;
    }
  }

  canvas {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    display: block;
  }

  .gizmo-overlay {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 2;
  }

  .gizmo-overlay line,
  .gizmo-overlay polygon,
  .gizmo-overlay rect,
  .gizmo-overlay circle {
    pointer-events: stroke;
  }

  .collider-overlay {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 1;
  }

  .joint-overlay {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 1;
  }

  .debug-draw-overlay {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 1;
  }

  .snap-input {
    width: 44px;
    height: 22px;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    font-family: var(--ed-mono);
    font-size: 11px;
    padding: 0 4px;
  }
  .camera-preview-overlay {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 1;
  }

  .pip-label {
    position: absolute;
    color: var(--ed-accent);
    font-size: 10px;
    font-family: monospace;
    background: color-mix(in srgb, var(--ed-base) 80%, transparent);
    padding: 2px 6px;
    border-radius: 3px;
    pointer-events: none;
    z-index: 2;
    white-space: nowrap;
  }

  .game-view-badge {
    position: absolute;
    top: 12px;
    left: 50%;
    transform: translateX(-50%);
    background: color-mix(in srgb, var(--ed-accent) 15%, transparent);
    border: 1px solid var(--ed-accent);
    color: var(--ed-accent);
    font-size: 11px;
    font-weight: 600;
    font-family: monospace;
    padding: 2px 12px;
    border-radius: 3px;
    pointer-events: none;
    z-index: 3;
    letter-spacing: 2px;
  }

  .poly-edit-overlay {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 3;
  }

  .poly-edit-toolbar {
    position: absolute;
    top: 40px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 8px;
    background: color-mix(in srgb, var(--ed-base) 95%, transparent);
    border: 1px solid var(--ed-ok);
    border-radius: 6px;
    padding: 4px 12px;
    z-index: 10;
    pointer-events: auto;
  }

  .poly-edit-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--ed-ok);
  }

  .poly-edit-done {
    background: var(--ed-ok);
    color: var(--ed-on-accent);
    border: none;
    border-radius: 4px;
    padding: 3px 10px;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }
  .poly-edit-done:hover {
    background: var(--ed-ok);
    filter: brightness(1.12);
  }

  .poly-edit-hint {
    font-size: 10px;
    color: var(--ed-muted);
  }

  .gizmo-overlay rect,
  .gizmo-overlay polygon {
    pointer-events: fill;
  }

  .placeholder,
  .error-message {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    text-align: center;
    color: var(--ed-faint);
    font-size: 14px;
    z-index: 1;
    pointer-events: none;
  }

  .error-message {
    color: var(--ed-err);
  }

  .profiler-panel {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    padding: 4px 10px;
    font-size: 10px;
    font-family: monospace;
    color: var(--ed-muted);
    background: color-mix(in srgb, var(--ed-mantle) 90%, transparent);
    border-top: 1px solid var(--ed-line);
    pointer-events: none;
    z-index: 20;
  }

  .profiler-row {
    display: flex;
    gap: 10px;
    align-items: baseline;
    /* Wrap between stats, never inside one. `nowrap` plus `overflow: hidden`
       meant a narrow panel broke the memory readout into a one-word-per-line
       column and clipped the stats after it off the right edge entirely -
       a stats bar that hides stats. */
    flex-wrap: wrap;
    row-gap: 2px;
  }

  .profiler-section {
    display: flex;
    gap: 4px;
    align-items: baseline;
  }

  .profiler-label {
    color: var(--ed-faint);
  }

  .profiler-value {
    color: var(--ed-text);
    display: inline-block;
    min-width: 48px;
    text-align: right;
    font-variant-numeric: tabular-nums;
    /* "4.4 MB (1.9 live, 2.4 peak)" is one value, not five words. */
    white-space: nowrap;
  }

  .profiler-value.fps-good {
    color: var(--ed-ok);
  }
  .profiler-value.fps-warn {
    color: var(--ed-err);
  }
  .profiler-value.script-color {
    color: var(--ed-warn);
  }
  .profiler-value.physics-color {
    color: var(--ed-accent);
  }
  .profiler-value.render-color {
    color: var(--ed-a2);
  }

  .profiler-sep {
    width: 1px;
    height: 10px;
    background: var(--ed-raised);
  }

  .profiler-bar {
    display: flex;
    height: 4px;
    margin: 3px 0 2px;
    border-radius: 2px;
    overflow: hidden;
    background: var(--ed-crust);
  }

  .bar-segment {
    height: 100%;
    min-width: 1px;
  }

  .bar-script {
    background: var(--ed-warn);
  }
  .bar-physics {
    background: var(--ed-accent);
  }
  .bar-render {
    background: var(--ed-a2);
  }
  .bar-other {
    background: var(--ed-line);
  }

  .frame-graph {
    width: 100%;
    height: 32px;
    display: block;
    margin-top: 2px;
  }
</style>
