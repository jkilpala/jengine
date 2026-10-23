<script lang="ts">
  import { newInstance, prefabKey } from "./prefabInstance";
  import { notifyPrefabsChanged, prefabLibrary } from "./stores/prefabLibraryStore";
  import { unclaimedParts, type ScenePart } from "./prefabParts";
  import { get } from "svelte/store";
  import { scene, dirty } from "./stores/sceneStore";
  import {
    selectedEntity,
    selectedEntities,
    selectEntity,
    toggleEntity,
    selectRange,
    clearSelection,
    selectScene,
    sceneSelected,
  } from "./stores/selectionStore";
  import { project } from "./stores/projectStore";
  import { isPlaying } from "./stores/playStore";
  import { runtimeEntities } from "./stores/runtimeStore";
  import type { RuntimeEntityRow } from "./stores/runtimeStore";
  import { logInfo, logError } from "./stores/consoleStore";
  import { execute, pushCommand } from "./stores/commandStore";
  import { removeEntity, addEntity } from "./api/scene";
  import {
    createAssetFile,
    listPrefabs,
    type PrefabEntry,
  } from "./api/project";
  import { ASSET_DRAG_MIME, createEntityFromAsset } from "./api/assetEntity";
  import { isTextEditingTarget } from "./api/textTarget";
  import { registerActions } from "./stores/actionStore";
  import {
    claimRenameTarget,
    lastRenameTarget,
  } from "./stores/renameTargetStore";

  import type { EntityData } from "./api/scene";

  let searchQuery = $state("");

  // ---- inline rename -------------------------------------------------------
  //
  // An entity could only be renamed through the Inspector's Name field, which
  // means finding it in a different panel to change a label you are looking
  // straight at. F2 does it in place, the same key as in the asset tree.

  /** Entity index being renamed inline, or null. */
  let renamingIndex = $state<number | null>(null);
  let renameValue = $state("");
  let renameInput = $state<HTMLInputElement | undefined>();

  function beginRename(index: number) {
    const ent = get(scene).entities[index];
    if (!ent) return;
    renamingIndex = index;
    renameValue = ent.name;
  }

  /**
   * Commit the rename.
   *
   * Renaming is not a local edit: `parent` refers to an entity **by name**, so
   * a rename that missed the children would silently orphan them. Undo takes
   * the whole scene before and after, like every other structural change here.
   */
  function commitRename() {
    const index = renamingIndex;
    renamingIndex = null;
    if (index === null) return;

    const before = get(scene);
    const oldName = before.entities[index]?.name;
    const newName = renameValue.trim();
    if (!oldName || !newName || newName === oldName) return;

    if (before.entities.some((e, i) => i !== index && e.name === newName)) {
      logError(`An entity named "${newName}" already exists`);
      return;
    }

    const after = structuredClone(before);
    after.entities[index].name = newName;
    for (const e of after.entities) {
      if (e.parent === oldName) e.parent = newName;
    }
    pushCommand(`Rename "${oldName}" to "${newName}"`, before, after);
  }

  function handleRenameKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      commitRename();
    } else if (e.key === "Escape") {
      e.preventDefault();
      renamingIndex = null;
    }
  }

  /** Rename whatever is selected - F2, Edit > Rename, the palette. */
  function renameSelected() {
    if (liveMode) {
      logError("Stop play mode before renaming an entity");
      return;
    }
    const index = get(selectedEntity);
    if (index < 0) {
      logError("Select an entity first, then rename it");
      return;
    }
    beginRename(index);
  }

  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "F2" || isTextEditingTarget(e)) return;
      // The asset browser renames too; F2 goes to whichever was clicked last.
      if ($lastRenameTarget !== "entity") return;
      e.preventDefault();
      renameSelected();
    };
    const onCommand = () => {
      if ($lastRenameTarget === "entity") renameSelected();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("editor-rename", onCommand);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("editor-rename", onCommand);
    };
  });

  $effect(() =>
    registerActions([
      {
        id: "entity.rename",
        title: "Rename Entity",
        category: "Scene",
        keys: "F2",
        enabled: () => $selectedEntity >= 0 && !liveMode,
        run: renameSelected,
      },
    ]),
  );

  $effect(() => {
    if (renamingIndex !== null) renameInput?.select();
  });
  let prefabList = $state<PrefabEntry[]>([]);
  let showPrefabSubmenu = $state(false);
  let collapsedNodes = $state<Set<string>>(new Set());
  let dragOverIndex = $state<number | null>(null);

  interface TreeNode {
    entity: EntityData;
    index: number;
    children: TreeNode[];
    depth: number;
    /** Collapse/identity key. Authored rows key by name (stable across
     *  edits); live rows key by runtime index, because names repeat
     *  freely at runtime — a wave of "Projectile" entities would
     *  otherwise share one collapse state and one parent lookup. */
    key: string;
    /** Set for rows sourced from the running world. */
    live?: RuntimeEntityRow;
  }

  /** Build tree from flat entity list using parent field. A placed
   *  prefab's parts the scene does not lay over yet (`parts`) are rows too,
   *  at negative indices (`-1 - k`): shown under their instance, and turned
   *  into a scene entity when clicked (`overridePart`). */
  function buildTree(sceneEntities: EntityData[], parts: ScenePart[] = []): TreeNode[] {
    const entities: EntityData[] = [
      ...sceneEntities,
      ...parts.map((p) => ({ name: p.name, parent: p.parent, components: p.components as EntityData["components"] })),
    ];
    const indexOf = (i: number) => (i < sceneEntities.length ? i : -1 - (i - sceneEntities.length));
    const nameToIndex = new Map<string, number>();
    entities.forEach((e, i) => nameToIndex.set(e.name, i));

    const childrenMap = new Map<number, number[]>();
    const roots: number[] = [];

    entities.forEach((e, i) => {
      const parentName = e.parent;
      if (parentName && nameToIndex.has(parentName)) {
        const parentIdx = nameToIndex.get(parentName)!;
        if (!childrenMap.has(parentIdx)) childrenMap.set(parentIdx, []);
        childrenMap.get(parentIdx)!.push(i);
      } else {
        roots.push(i);
      }
    });

    function buildNode(idx: number, depth: number): TreeNode {
      const kids = childrenMap.get(idx) ?? [];
      return {
        entity: entities[idx],
        index: indexOf(idx),
        depth,
        key: entities[idx].name,
        children: kids.map((c) => buildNode(c, depth + 1)),
      };
    }

    return roots.map((r) => buildNode(r, 0));
  }

  /** Components stub so `entityKind` can pick a row's dot and tag for live rows. */
  function kindComponents(kind: string): EntityData["components"] {
    switch (kind) {
      case "particle":
        return { particle_emitter: {} } as EntityData["components"];
      case "text":
        return { text: {} } as EntityData["components"];
      case "camera":
        return { camera: {} } as EntityData["components"];
      case "sprite":
        return { sprite: {} } as EntityData["components"];
      default:
        return {} as EntityData["components"];
    }
  }

  /**
   * Build the tree from the running world. Parents come through as
   * runtime indices (names are not unique at runtime), and rows the
   * engine manages itself — health-bar quads, name tags — are dropped
   * unless `showInternal` is on, since they can outnumber the entities
   * you actually want to look at.
   */
  function buildLiveTree(rows: RuntimeEntityRow[]): TreeNode[] {
    const visible = rows.filter((r) => showInternal || !r.internal);
    const byIndex = new Map<number, RuntimeEntityRow>();
    for (const r of visible) byIndex.set(r.index, r);

    const childrenMap = new Map<number, RuntimeEntityRow[]>();
    const roots: RuntimeEntityRow[] = [];
    for (const r of visible) {
      // A parent that was filtered out (or already despawned) promotes
      // its children to roots rather than hiding them.
      if (r.parent !== null && byIndex.has(r.parent)) {
        const list = childrenMap.get(r.parent) ?? [];
        list.push(r);
        childrenMap.set(r.parent, list);
      } else {
        roots.push(r);
      }
    }

    function buildNode(row: RuntimeEntityRow, depth: number): TreeNode {
      return {
        entity: {
          name: row.name,
          components: kindComponents(row.kind),
        } as EntityData,
        index: row.index,
        depth,
        key: `live:${row.index}`,
        live: row,
        children: (childrenMap.get(row.index) ?? []).map((c) =>
          buildNode(c, depth + 1),
        ),
      };
    }

    return roots.map((r) => buildNode(r, 0));
  }

  /** Show engine-managed view entities (health bars, name tags). */
  let showInternal = $state(false);

  /** True while the panel is mirroring the running world. */
  let liveMode = $derived($isPlaying && $runtimeEntities.length > 0);

  /** Parts of placed prefabs the scene does not lay over yet. */
  let looseParts = $derived(liveMode ? [] : unclaimedParts($prefabLibrary.resolved, $scene.entities));

  let tree = $derived(
    liveMode ? buildLiveTree($runtimeEntities) : buildTree($scene.entities, looseParts),
  );

  /** A part's row clicked: an entity of its name and parent, laid over the
   *  part - empty, so nothing changes until it is edited - then selected. */
  function overridePart(node: TreeNode) {
    const name = node.entity.name;
    const parent = node.entity.parent ?? null;
    execute(`Edit part ${name}`, (s) => {
      s.entities.push({ name, parent, components: {} as EntityData["components"] });
      return s;
    });
    const at = get(scene).entities.findIndex((e) => e.name === name && (e.parent ?? null) === parent);
    if (at >= 0) selectEntity(at);
  }

  /** What a row is called: a part (`Cart/Wheel`) by its own name. */
  function rowName(e: EntityData): string {
    return e.parent && e.name.startsWith(`${e.parent}/`) ? e.name.slice(e.parent.length + 1) : e.name;
  }

  /** Flatten tree into visible list (respecting collapsed state and search). */
  function flattenTree(nodes: TreeNode[], filter: string): TreeNode[] {
    const result: TreeNode[] = [];
    const lowerFilter = filter.toLowerCase();

    function walk(node: TreeNode) {
      const matches =
        !filter || node.entity.name.toLowerCase().includes(lowerFilter);
      if (matches) result.push(node);
      if (!collapsedNodes.has(node.key)) {
        for (const child of node.children) {
          walk(child);
        }
      }
    }

    for (const root of nodes) walk(root);
    return result;
  }

  let visibleEntities = $derived(flattenTree(tree, searchQuery.trim()));

  // ── Row windowing ────────────────────────────────────────────────────────
  //
  // The panel used to render every entity as a `<div>` with six event
  // handlers. At 16,000 entities that was ~68 ms a frame of browser layout
  // and style — three times the engine's own work — and it is invisible to
  // the engine's profiler because it is the DOM, not the frame loop.
  //
  // Only the rows in view are rendered; two spacer divs stand in for the rest
  // so the scrollbar still reflects the true length.

  /** Row height in px. Must match `.entity-item`'s fixed height below —
   *  windowing needs a height it can compute with rather than measure. */
  const ROW_H = 24;

  /** Rows rendered beyond each edge, so a fast scroll does not show gaps. */
  const OVERSCAN = 8;

  /** Below this, render everything. Small scenes are the common case and
   *  behave exactly as they always did — no spacers, no scroll maths. */
  const WINDOW_THRESHOLD = 150;

  let listEl: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewportH = $state(0);

  let windowed = $derived(visibleEntities.length > WINDOW_THRESHOLD);

  let firstRow = $derived(
    windowed ? Math.max(0, Math.floor(scrollTop / ROW_H) - OVERSCAN) : 0,
  );
  let lastRow = $derived(
    windowed
      ? Math.min(
          visibleEntities.length,
          Math.ceil((scrollTop + (viewportH || 400)) / ROW_H) + OVERSCAN,
        )
      : visibleEntities.length,
  );
  let renderedEntities = $derived(visibleEntities.slice(firstRow, lastRow));
  let padTop = $derived(firstRow * ROW_H);
  let padBottom = $derived(
    Math.max(0, (visibleEntities.length - lastRow) * ROW_H),
  );

  function onListScroll() {
    if (!listEl) return;
    scrollTop = listEl.scrollTop;
    viewportH = listEl.clientHeight;
  }

  // The list can change length without a scroll event (play starts, a search
  // is typed), and the panel can be resized. Re-measure when the set changes.
  $effect(() => {
    void visibleEntities.length;
    if (listEl) viewportH = listEl.clientHeight;
  });

  /**
   * What a row is, for its dot and its tag: the dot in the palette's
   * category hues, the tag the kind in a word. A plain sprite has no tag -
   * it is most of a scene, and a tag on every row says nothing.
   */
  function entityKind(entity: EntityData): { tag: string; hue: string } {
    const c = entity.components as Record<string, unknown>;
    const has = (k: string) => c[k] !== undefined && c[k] !== null;
    if (has("prefab_instance")) return { tag: "prefab", hue: "var(--ed-cat-1)" };
    if (has("camera")) return { tag: "camera", hue: "var(--ed-cat-3)" };
    if (["point_light", "spot_light", "directional_light", "ambient_light", "freeform_light"].some(has))
      return { tag: "light", hue: "var(--ed-cat-4)" };
    if (has("gui")) return { tag: "gui", hue: "var(--ed-cat-6)" };
    if (Object.keys(c).some((k) => k.startsWith("ui_"))) return { tag: "ui", hue: "var(--ed-cat-6)" };
    if (has("tilemap")) return { tag: "tilemap", hue: "var(--ed-muted)" };
    if (has("particle_emitter") || has("gpu_particles")) return { tag: "particles", hue: "var(--ed-cat-8)" };
    if (has("trail")) return { tag: "trail", hue: "var(--ed-cat-8)" };
    if (has("audio_source")) return { tag: "audio", hue: "var(--ed-cat-5)" };
    if ((has("rigid_body") || has("collider")) && !has("sprite")) return { tag: "body", hue: "var(--ed-cat-7)" };
    if (has("sprite") || has("mesh") || has("skinned_mesh")) return { tag: "", hue: "var(--ed-cat-2)" };
    if (has("text")) return { tag: "text", hue: "var(--ed-text)" };
    if (has("script")) return { tag: "script", hue: "var(--ed-cat-8)" };
    return { tag: "", hue: "var(--ed-line-strong)" };
  }

  function hasChildren(node: TreeNode): boolean {
    return node.children.length > 0;
  }

  function toggleCollapse(name: string) {
    const next = new Set(collapsedNodes);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    collapsedNodes = next;
  }

  let contextMenu = $state<{
    x: number;
    y: number;
    index: number | null;
  } | null>(null);

  function handleSelect(index: number, event?: MouseEvent) {
    if (event?.ctrlKey || event?.metaKey) {
      toggleEntity(index);
    } else if (event?.shiftKey && $selectedEntity >= 0) {
      selectRange($selectedEntity, index);
    } else {
      selectEntity(index);
    }
  }

  async function handleDelete(index: number, event: MouseEvent) {
    event.stopPropagation();
    const currentScene = get(scene);
    const deletedName = currentScene.entities[index]?.name;
    try {
      const before = structuredClone(currentScene);
      // Also unparent children of deleted entity
      const updated = structuredClone(currentScene);
      updated.entities.splice(index, 1);
      for (const e of updated.entities) {
        if (e.parent === deletedName) e.parent = null;
      }
      pushCommand(`Remove entity "${deletedName}"`, before, updated);
      const sel = get(selectedEntity);
      if (sel === index) {
        clearSelection();
      } else if (sel > index) {
        selectEntity(sel - 1);
      }
      logInfo(`Removed entity at index ${index}`);
    } catch (e) {
      console.error("Failed to remove entity:", e);
    }
  }

  function handleContextMenu(event: MouseEvent, index: number) {
    event.preventDefault();
    event.stopPropagation();
    // The menu's actions (add / duplicate / delete / reparent / save as
    // prefab) all rewrite the scene file by index — nothing you can
    // meaningfully do to a world that is running.
    if (liveMode) return;
    contextMenu = { x: event.clientX, y: event.clientY, index };
  }

  function closeContextMenu() {
    contextMenu = null;
    showPrefabSubmenu = false;
  }

  async function handleSaveAsPrefab() {
    if (contextMenu === null || contextMenu.index === null) return;
    const idx = contextMenu.index;
    closeContextMenu();

    const proj = get(project);
    if (!proj) {
      logError("No project open");
      return;
    }

    const currentScene = get(scene);
    const entity = currentScene.entities[idx];
    if (!entity) return;

    const fileName = `${entity.name.replace(/[^a-zA-Z0-9_-]/g, "_")}.prefab`;
    const prefabData = {
      name: entity.name,
      components: entity.components,
    };

    try {
      await createAssetFile(
        proj.path,
        `prefabs/${fileName}`,
        JSON.stringify(prefabData, null, 2),
      );
      logInfo(`Saved prefab: prefabs/${fileName}`);
      notifyPrefabsChanged();
    } catch (e) {
      logError(`Failed to save prefab: ${e}`);
    }
  }

  async function togglePrefabSubmenu() {
    const proj = get(project);
    if (!proj) return;
    if (showPrefabSubmenu) {
      showPrefabSubmenu = false;
      return;
    }
    try {
      prefabList = await listPrefabs(proj.path);
      showPrefabSubmenu = true;
    } catch (e) {
      logError(`Failed to list prefabs: ${e}`);
    }
  }

  function instantiatePrefab(entry: PrefabEntry) {
    closeContextMenu();
    const currentScene = get(scene);
    if (!currentScene) return;

    // A linked instance, not a copy: the prefab's id and the entity's own
    // transform, so editing the prefab later reaches it.
    const source = entry.data as EntityData;
    const cloned = newInstance(prefabKey(entry), {
      name: source.name ?? entry.name,
      components: source.components ?? {},
    }) as EntityData;

    // Ensure unique name to keep the parent-by-name tree map stable.
    const existingNames = new Set(currentScene.entities.map((e) => e.name));
    let finalName = cloned.name;
    if (existingNames.has(finalName)) {
      let n = 2;
      while (existingNames.has(`${cloned.name}_${n}`)) n++;
      finalName = `${cloned.name}_${n}`;
    }
    cloned.name = finalName;

    const before = structuredClone(currentScene);
    const updated = {
      ...currentScene,
      entities: [...currentScene.entities, cloned],
    };
    pushCommand(`Instantiate prefab "${entry.name}"`, before, updated);
    selectEntity(currentScene.entities.length);
    logInfo(`Instantiated prefab: ${entry.name} as "${finalName}"`);
  }

  async function handleAddEntity() {
    closeContextMenu();
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

  async function handleAddChild() {
    if (contextMenu === null || contextMenu.index === null) return;
    const parentIdx = contextMenu.index;
    const parentName = get(scene).entities[parentIdx]?.name;
    closeContextMenu();
    if (!parentName) return;

    const currentScene = get(scene);
    const entityNum = currentScene.entities.length + 1;
    const name = `Entity_${entityNum}`;
    try {
      const before = structuredClone(currentScene);
      const s = await addEntity(currentScene, name);
      // Set parent on the newly added entity
      s.entities[s.entities.length - 1].parent = parentName;
      pushCommand(`Add child "${name}" to "${parentName}"`, before, s);
      selectEntity(s.entities.length - 1);
      logInfo(`Added child entity: ${name}`);
    } catch (e) {
      logError(`Failed to add child entity: ${e}`);
    }
  }

  function handleUnparent() {
    if (contextMenu === null || contextMenu.index === null) return;
    const idx = contextMenu.index;
    closeContextMenu();

    const currentScene = get(scene);
    const entity = currentScene.entities[idx];
    if (!entity || !entity.parent) return;

    const before = structuredClone(currentScene);
    const updated = structuredClone(currentScene);
    updated.entities[idx].parent = null;
    pushCommand(`Unparent "${entity.name}"`, before, updated);
  }

  async function handleDuplicate(index: number) {
    closeContextMenu();
    const currentScene = get(scene);
    const entity = currentScene.entities[index];
    if (!entity) return;
    const before = structuredClone(currentScene);
    const clone: EntityData = structuredClone(entity);
    clone.name = `${entity.name}_copy`;
    const updated = {
      ...currentScene,
      entities: [...currentScene.entities, clone],
    };
    pushCommand(`Duplicate entity "${entity.name}"`, before, updated);
    selectEntity(updated.entities.length - 1);
    logInfo(`Duplicated entity: ${entity.name}`);
  }

  function handlePanelContextMenu(event: MouseEvent) {
    event.preventDefault();
    contextMenu = { x: event.clientX, y: event.clientY, index: null };
  }

  // ── Drag & Drop reparenting ──
  function handleDragStart(event: DragEvent, index: number) {
    event.dataTransfer?.setData("text/plain", String(index));
    event.dataTransfer!.effectAllowed = "move";
  }

  function handleDragOver(event: DragEvent, index: number) {
    event.preventDefault();
    event.dataTransfer!.dropEffect = event.dataTransfer?.types.includes(
      ASSET_DRAG_MIME,
    )
      ? "copy"
      : "move";
    dragOverIndex = index;
  }

  function handleDragLeave() {
    dragOverIndex = null;
  }

  function handleDrop(event: DragEvent, targetIndex: number) {
    event.preventDefault();
    dragOverIndex = null;

    // Asset drop → create entity as child of target
    if (event.dataTransfer?.types.includes(ASSET_DRAG_MIME)) {
      handleAssetDrop(event, targetIndex);
      return;
    }

    const sourceIndexStr = event.dataTransfer?.getData("text/plain");
    if (sourceIndexStr === undefined) return;
    const sourceIndex = parseInt(sourceIndexStr, 10);
    if (isNaN(sourceIndex) || sourceIndex === targetIndex) return;

    const currentScene = get(scene);
    const source = currentScene.entities[sourceIndex];
    const target = currentScene.entities[targetIndex];
    if (!source || !target) return;

    // Prevent parenting to own descendant (cycle check)
    if (isDescendant(currentScene.entities, target.name, source.name)) return;

    const before = structuredClone(currentScene);
    const updated = structuredClone(currentScene);
    updated.entities[sourceIndex].parent = target.name;
    pushCommand(
      `Parent "${source.name}" under "${target.name}"`,
      before,
      updated,
    );
    logInfo(`Parented "${source.name}" under "${target.name}"`);
  }

  function handleDropOnPanel(event: DragEvent) {
    event.preventDefault();
    dragOverIndex = null;

    // Asset drop → create entity at root
    if (event.dataTransfer?.types.includes(ASSET_DRAG_MIME)) {
      handleAssetDrop(event, null);
      return;
    }

    const sourceIndexStr = event.dataTransfer?.getData("text/plain");
    if (sourceIndexStr === undefined) return;
    const sourceIndex = parseInt(sourceIndexStr, 10);
    if (isNaN(sourceIndex)) return;

    const currentScene = get(scene);
    const source = currentScene.entities[sourceIndex];
    if (!source || !source.parent) return;

    const before = structuredClone(currentScene);
    const updated = structuredClone(currentScene);
    updated.entities[sourceIndex].parent = null;
    pushCommand(`Unparent "${source.name}"`, before, updated);
    logInfo(`Unparented "${source.name}"`);
  }

  /** Handle an asset drop — create a new entity from the dropped file. */
  async function handleAssetDrop(event: DragEvent, parentIndex: number | null) {
    const raw = event.dataTransfer?.getData(ASSET_DRAG_MIME);
    if (!raw) return;
    const proj = get(project);
    if (!proj) return;

    const { path, name } = JSON.parse(raw) as { path: string; name: string };
    try {
      const entity = await createEntityFromAsset(proj.path, path, name);
      if (parentIndex !== null) {
        const currentScene = get(scene);
        const parent = currentScene.entities[parentIndex];
        if (parent) entity.parent = parent.name;
      }

      const before = structuredClone(get(scene));
      const updated = structuredClone(get(scene));
      updated.entities.push(entity);
      pushCommand(`Drop asset "${name}"`, before, updated);
      selectEntity(updated.entities.length - 1);
      logInfo(`Created entity "${entity.name}" from ${name}`);
    } catch (err) {
      logError(`Failed to create entity from ${name}: ${err}`);
    }
  }

  /** Check if `name` is a descendant of `ancestorName`. */
  function isDescendant(
    entities: EntityData[],
    name: string,
    ancestorName: string,
  ): boolean {
    let current = name;
    let depth = 0;
    while (depth < 32) {
      const entity = entities.find((e) => e.name === current);
      if (!entity?.parent) return false;
      if (entity.parent === ancestorName) return true;
      current = entity.parent;
      depth++;
    }
    return false;
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scene-tree" onclick={closeContextMenu}>
  <!-- The dock tab names the panel; this strip is for Play's live view. -->
  {#if liveMode}
    <div class="header">
      <span class="live-badge" title="Showing the running world">LIVE</span>
      <label class="internal-toggle" title="Show engine-managed entities (health bars, name tags)">
        <input type="checkbox" bind:checked={showInternal} />
        internal
      </label>
    </div>
  {/if}
  <!-- Scene row — selecting it puts scene-level settings (sort layers,
       post-processing) in the Inspector, the way entities put their
       components there. -->
  <button
    class="scene-row"
    class:selected={$sceneSelected}
    onclick={() => selectScene()}
    title="Scene settings — sort layers, post-processing"
  >
    <svg class="scene-icon" width="13" height="13" viewBox="0 0 14 14" aria-hidden="true">
      <path d="M7 1.5 L12.5 4.5 L7 7.5 L1.5 4.5 Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" />
      <path d="M1.5 7.2 L7 10.2 L12.5 7.2 M1.5 9.9 L7 12.9 L12.5 9.9" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" />
    </svg>
    <span class="scene-label">{$scene.name}</span>
    {#if $dirty}<span class="scene-dirty" title="Unsaved changes"></span>{/if}
  </button>
  <div class="search-bar">
    <label class="search-field">
      <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden="true">
        <circle cx="5" cy="5" r="3.6" stroke="currentColor" stroke-width="1.3" fill="none" />
        <path d="M7.8 7.8 L11 11" stroke="currentColor" stroke-width="1.3" />
      </svg>
      <span class="sr-only">Filter entities</span>
      <input
        type="text"
        placeholder="Filter entities"
        bind:value={searchQuery}
        class="search-input"
      />
    </label>
  </div>
  <div
    class="entity-list"
    bind:this={listEl}
    onscroll={onListScroll}
    oncontextmenu={handlePanelContextMenu}
    ondragover={(e) => {
      e.preventDefault();
      e.dataTransfer!.dropEffect = e.dataTransfer?.types.includes(
        ASSET_DRAG_MIME,
      )
        ? "copy"
        : "move";
    }}
    ondrop={handleDropOnPanel}
  >
    {#if padTop > 0}
      <div style="height: {padTop}px" aria-hidden="true"></div>
    {/if}
    {#each renderedEntities as node}
      {#if node.index < 0}
        <!-- A part of a placed prefab, not laid over in this scene yet. -->
        <div
          class="entity-item part-row"
          style="padding-left: {10 + node.depth * 16}px"
          title="A part of the prefab placed above. Click to edit it in this scene (what you change is kept here)."
          onclick={() => overridePart(node)}
          onkeydown={(e) => e.key === "Enter" && overridePart(node)}
          role="treeitem"
          aria-selected="false"
          tabindex="0"
        >
          {#if hasChildren(node)}
            <button
              class="collapse-btn"
              onclick={(e) => {
                e.stopPropagation();
                toggleCollapse(node.key);
              }}>{collapsedNodes.has(node.key) ? "▸" : "▾"}</button
            >
          {:else}
            <span class="collapse-spacer"></span>
          {/if}
          <span class="entity-dot part" aria-hidden="true"></span>
          <span class="entity-name">{rowName(node.entity)}</span>
          <span class="entity-tag">part</span>
        </div>
      {:else}
      {@const kind = entityKind(node.entity)}
      <div
        class="entity-item"
        class:selected={$selectedEntities.has(node.index)}
        class:drag-over={dragOverIndex === node.index}
        style="padding-left: {10 + node.depth * 16}px"
        onclick={(e) => {
          claimRenameTarget("entity");
          handleSelect(node.index, e);
        }}
        ondblclick={() => !liveMode && beginRename(node.index)}
        oncontextmenu={(e) => handleContextMenu(e, node.index)}
        ondragstart={(e) => handleDragStart(e, node.index)}
        ondragover={(e) => handleDragOver(e, node.index)}
        ondragleave={handleDragLeave}
        ondrop={(e) => {
          e.stopPropagation();
          handleDrop(e, node.index);
        }}
        draggable={!liveMode}
        role="treeitem"
        aria-selected={$selectedEntities.has(node.index)}
        tabindex="0"
        onkeydown={(e) => e.key === "Enter" && handleSelect(node.index)}
      >
        {#if hasChildren(node)}
          <button
            class="collapse-btn"
            onclick={(e) => {
              e.stopPropagation();
              toggleCollapse(node.key);
            }}>{collapsedNodes.has(node.key) ? "▸" : "▾"}</button
          >
        {:else}
          <span class="collapse-spacer"></span>
        {/if}
        <span class="entity-dot" style:background={kind.hue} aria-hidden="true"></span>
        {#if renamingIndex === node.index}
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="rename-input"
            bind:this={renameInput}
            bind:value={renameValue}
            autofocus
            onclick={(e) => e.stopPropagation()}
            onkeydown={handleRenameKeydown}
            onblur={commitRename}
          />
        {:else}
          {@const linked = (node.entity.components as Record<string, unknown>)?.prefab_instance as { prefab_id?: string } | undefined}
          <span class="entity-name">{rowName(node.entity)}</span>
          {#if kind.tag}
            <span
              class="entity-tag"
              title={linked ? `Instance of prefab ${linked.prefab_id}` : undefined}>{kind.tag}</span
            >
          {/if}
        {/if}
        {#if node.live?.spawned}
          <span class="spawned-badge" title="Spawned during play">+</span>
        {/if}
        {#if !liveMode}
          <button
            class="delete-btn"
            onclick={(e) => handleDelete(node.index, e)}
            title="Delete entity">×</button
          >
        {/if}
      </div>
      {/if}
    {/each}
    {#if padBottom > 0}
      <div style="height: {padBottom}px" aria-hidden="true"></div>
    {/if}
    {#if visibleEntities.length === 0}
      <div class="empty">{searchQuery ? "No matches" : "No entities"}</div>
    {/if}
  </div>
</div>

{#if contextMenu}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="context-overlay"
    onclick={closeContextMenu}
    oncontextmenu={(e) => {
      e.preventDefault();
      closeContextMenu();
    }}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      class="context-menu"
      style="left: {contextMenu.x}px; top: {contextMenu.y}px"
      onclick={(e) => e.stopPropagation()}
    >
      <button class="menu-item" onclick={handleAddEntity}>➕ Add Entity</button>
      {#if contextMenu.index !== null}
        <button class="menu-item" onclick={handleAddChild}>➕ Add Child</button>
        <button
          class="menu-item"
          onclick={() => handleDuplicate(contextMenu!.index!)}
          >📋 Duplicate</button
        >
        <button
          class="menu-item delete"
          onclick={(e) => {
            handleDelete(contextMenu!.index!, e);
            closeContextMenu();
          }}>🗑 Delete</button
        >
        <div class="menu-divider"></div>
        {#if $scene.entities[contextMenu.index]?.parent}
          <button class="menu-item" onclick={handleUnparent}>⬆ Unparent</button
          >
        {/if}
        <button class="menu-item" onclick={handleSaveAsPrefab}
          >🧩 Save as Prefab</button
        >
      {/if}
      <div class="menu-divider"></div>
      <button class="menu-item submenu-trigger" onclick={togglePrefabSubmenu}>
        🧩 Instantiate Prefab {showPrefabSubmenu ? "▾" : "▸"}
      </button>
      {#if showPrefabSubmenu}
        {#if prefabList.length === 0}
          <div class="submenu-empty">No prefabs found</div>
        {:else}
          {#each prefabList as entry}
            <button
              class="menu-item submenu-item"
              onclick={() => instantiatePrefab(entry)}
            >
              {entry.name}
            </button>
          {/each}
        {/if}
      {/if}
    </div>
  </div>
{/if}

<style>
  .scene-tree {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--ed-base);
  }

  .search-bar {
    padding: 8px 8px 6px;
  }

  .search-field {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 26px;
    padding: 0 8px;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line);
    border-radius: 5px;
    color: var(--ed-faint);
  }

  .search-field:focus-within {
    border-color: var(--ed-accent);
    box-shadow: 0 0 0 2px var(--ed-accent-low);
  }

  .search-input {
    flex: 1;
    min-width: 0;
    padding: 0;
    font-family: inherit;
    font-size: 12px;
    color: var(--ed-text);
    background: none;
    border: none;
    outline: none;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  .search-input::placeholder {
    color: var(--ed-faint);
  }

  .header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    font-family: var(--ed-mono);
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ed-muted);
    background: var(--ed-mantle);
    border-bottom: 1px solid var(--ed-line);
  }

  .live-badge {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.5px;
    color: var(--ed-ok);
    border: 1px solid color-mix(in srgb, var(--ed-ok) 50%, transparent);
    border-radius: 3px;
    padding: 0 4px;
  }

  .internal-toggle {
    display: flex;
    align-items: center;
    gap: 3px;
    margin-left: auto;
    font-size: 10px;
    font-weight: 400;
    text-transform: none;
    color: var(--ed-faint);
    cursor: pointer;
  }

  .internal-toggle input {
    margin: 0;
  }

  .spawned-badge {
    font-size: 10px;
    color: var(--ed-ok);
    margin-left: 4px;
  }

  .scene-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 30px;
    padding: 0 12px;
    background: none;
    border: none;
    border-bottom: 1px solid var(--ed-line);
    color: var(--ed-ink);
    font-size: 12.5px;
    font-weight: 600;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
  }

  .scene-row:hover {
    background: var(--ed-hover);
  }

  .scene-row.selected {
    background: var(--ed-accent-low);
  }

  .scene-icon {
    flex-shrink: 0;
    color: var(--ed-faint);
  }

  .scene-dirty {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ed-a3);
    flex-shrink: 0;
  }

  .scene-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .entity-list {
    flex: 1;
    overflow-y: auto;
    padding: 0 4px 4px;
  }

  .entity-item {
    display: flex;
    align-items: center;
    gap: 2px;
    /* Fixed height, not padding-derived: the windowing maths above needs a
       row height it can compute positions from. Keep `ROW_H` in step. */
    height: 24px;
    box-sizing: border-box;
    padding: 0 8px;
    border-radius: 4px;
    cursor: grab;
    font-size: 12.5px;
    color: var(--ed-text);
  }

  .entity-item:hover {
    background: var(--ed-hover);
  }

  .entity-item.selected {
    background: var(--ed-accent-low);
    color: var(--ed-ink);
    font-weight: 600;
  }

  .entity-item.drag-over {
    border-top: 2px solid var(--ed-accent);
    background: color-mix(in srgb, var(--ed-accent) 10%, transparent);
  }

  .collapse-btn {
    background: none;
    border: none;
    color: var(--ed-faint);
    cursor: pointer;
    font-size: 10px;
    padding: 0;
    width: 14px;
    flex-shrink: 0;
    text-align: center;
    line-height: 1;
  }

  .collapse-btn:hover {
    color: var(--ed-text);
  }

  .collapse-spacer {
    width: 14px;
    flex-shrink: 0;
  }

  /* The kind of thing a row is, as a dot in its category hue. */
  .entity-dot {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    margin: 0 7px 0 3px;
  }

  .entity-dot.part {
    background: none;
    border: 1px dashed var(--ed-line-strong);
  }

  .entity-tag {
    margin-left: auto;
    padding-left: 8px;
    flex-shrink: 0;
    font-family: var(--ed-mono);
    font-size: 10px;
    font-weight: 400;
    color: var(--ed-faint);
  }

  .rename-input {
    flex: 1;
    min-width: 0;
    background: var(--ed-base);
    border: 1px solid var(--ed-accent);
    border-radius: 4px;
    color: var(--ed-text);
    font-family: inherit;
    font-size: inherit;
    padding: 0 2px;
    outline: none;
  }

  .entity-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .delete-btn {
    background: none;
    border: none;
    color: var(--ed-faint);
    cursor: pointer;
    font-size: 14px;
    padding: 0 4px;
    line-height: 1;
    /* At the row's right edge, in the tag's place while the row is hovered. */
    margin-left: auto;
    flex-shrink: 0;
    display: none;
  }

  .entity-item:hover .delete-btn {
    display: block;
  }

  .entity-item:hover .entity-tag {
    display: none;
  }

  .delete-btn:hover {
    color: var(--ed-err);
  }

  .empty {
    padding: 12px 10px;
    color: var(--ed-faint);
    font-size: 12px;
    font-style: italic;
  }

  .context-overlay {
    position: fixed;
    inset: 0;
    z-index: 1000;
  }

  .context-menu {
    position: fixed;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 7px;
    padding: 5px;
    min-width: 160px;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
    z-index: 1001;
  }

  .menu-item {
    display: block;
    width: 100%;
    text-align: left;
    padding: 6px 12px;
    background: none;
    border: none;
    color: var(--ed-text);
    font-size: 12px;
    cursor: pointer;
    font-family: inherit;
    border-radius: 4px;
  }

  .menu-item:hover {
    background: var(--ed-accent-low);
    color: var(--ed-ink);
  }

  .menu-divider {
    height: 1px;
    background: var(--ed-line);
    margin: 4px 0;
  }

  .submenu-trigger {
    font-weight: 500;
  }

  .submenu-item {
    padding-left: 24px;
    border-radius: 4px;
  }

  .submenu-empty {
    padding: 6px 24px;
    color: var(--ed-faint);
    font-size: 11px;
    font-style: italic;
  }

  .menu-item.delete:hover {
    color: var(--ed-err);
  }
  .part-row {
    opacity: 0.6;
    font-style: italic;
  }
</style>
