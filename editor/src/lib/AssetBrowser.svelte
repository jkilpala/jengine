<script lang="ts">
  import { get } from "svelte/store";
  import { project, assetRefreshRequest } from "./stores/projectStore";
  import { scene, scenePath, dirty } from "./stores/sceneStore";
  import { sceneTabs } from "./stores/sceneTabsStore";
  import { selectedEntity, clearSelection } from "./stores/selectionStore";
  import { clearHistory } from "./stores/commandStore";
  import { logInfo, logError } from "./stores/consoleStore";
  import { isTextEditingTarget } from "./api/textTarget";
  import { registerActions } from "./stores/actionStore";
  import {
    claimRenameTarget,
    lastRenameTarget,
  } from "./stores/renameTargetStore";
  import {
    listAssetTree,
    importPsdLayers,
    createAssetFolder,
    createAssetFile,
    deleteAsset,
    findAssetReferences,
    renameAssetWithReferences,
    rewriteAssetReferencesInText,
    importAsset,
    duplicateAsset,
    refreshProjectFiles,
    openInCodeEditor,
    showInFileManager,
    type AssetNode,
  } from "./api/project";
  import { loadScene } from "./api/scene";
  import { openAsset } from "./api/openAsset";
  import { isDroppableAsset, ASSET_DRAG_MIME } from "./api/assetEntity";
  import type { EntityData } from "./api/scene";
  import { open } from "@tauri-apps/plugin-dialog";
  import { createGuiFile } from "./api/gui";
  import { createAtlas } from "./api/atlas";
  import { shaderCreateNew, shaderSave } from "./api/shaderEditor";
  import { defaultParticleEmitter } from "./api/particle";
  import { defaultAnimationClip } from "./api/animation";
  import { defaultFontFile } from "./api/font";
  import { defaultTilemap } from "./api/tilemap";
  import { defaultBehaviorTemplate } from "./api/behavior";
  import { defaultSortLayersFile } from "./api/sortLayers";
  import {
    defaultDataSchema,
    defaultDataTable,
    listDataSchemas,
    type SchemaEntry,
  } from "./api/dataEditor";

  let tree = $state<AssetNode[]>([]);
  let expanded = $state<Set<string>>(new Set());
  let contextMenu = $state<{
    x: number;
    y: number;
    node: AssetNode | null;
  } | null>(null);
  let renamingPath = $state<string | null>(null);
  /** Whether the row being renamed is a folder - its references are the
   *  paths inside it. */
  let renamingWasDir = false;
  /**
   * The row F2 and the Rename command act on.
   *
   * A real selection, not DOM focus. Focus was the selection before, and it
   * did not work: rows are `draggable`, and a browser does not focus a
   * draggable element on mousedown - so F2 had no target, nothing on screen
   * said so, and the key looked broken. A selection also survives clicking
   * away into a menu or the palette, which focus does not.
   */
  let selectedPath = $state<string | null>(null);
  let renameValue = $state("");

  function clampToViewport(el: HTMLElement) {
    const rect = el.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    if (rect.bottom > vh) {
      el.style.top = `${Math.max(0, vh - rect.height)}px`;
    }
    if (rect.right > vw) {
      el.style.left = `${Math.max(0, vw - rect.width)}px`;
    }
  }

  // New script dialog state
  interface ScriptType {
    label: string;
    ext: string;
    template: string;
  }
  const SCRIPT_TYPES: ScriptType[] = [
    {
      label: "Lua",
      ext: ".lua",
      template: `-- New scripts default to 10 Hz — raise tick_rate to 60 only if you
-- need per-frame logic (and consider a native behavior component instead).
script = { tick_rate = 10 }

function init(entity)
  -- Called once when play starts
end

function update(entity, dt)
  -- Called every tick (~10/s by default)
end
`,
    },
    {
      label: "JavaScript",
      ext: ".js",
      template: `// New scripts default to 10 Hz — raise tickHz to 60 only if you
// need per-frame logic (and consider a native behavior component instead).
export const script = { tickHz: 10 };

function init(entity) {
  // Called once when play starts
}

function update(entity, dt) {
  // Called every tick (~10/s by default)
}
`,
    },
  ];
  let newScriptDialog = $state<{ parentPath: string } | null>(null);
  let newScriptType = $state<ScriptType>(SCRIPT_TYPES[0]);
  let newScriptName = $state("");

  // New Data Table dialog (schema chosen from a dropdown).
  let newDataTableDialog = $state<{ parentPath: string } | null>(null);
  let newDataTableSchemas = $state<SchemaEntry[]>([]);
  let newDataTableSchemaName = $state("");
  let newDataTableName = $state("");

  async function refresh() {
    const proj = get(project);
    if (!proj) {
      tree = [];
      return;
    }
    try {
      const oldPaths = new Set(collectPaths(tree));
      tree = await listAssetTree(proj.path);
      // Auto-expand any new directories so they're visible immediately
      for (const p of collectPaths(tree)) {
        if (!oldPaths.has(p)) expanded.add(p);
      }
      // Also refresh file lists so pickers stay in sync
      const lists = await refreshProjectFiles(proj.path);
      project.update((p) => {
        if (!p) return p;
        return {
          ...p,
          script_files: lists.script_files,
          sprite_files: lists.sprite_files,
          scene_files: lists.scene_files,
          prefab_files: lists.prefab_files,
          gui_files: lists.gui_files,
          anim_files: lists.anim_files,
        };
      });
    } catch (e) {
      logError(`Failed to load assets: ${e}`);
      tree = [];
    }
  }

  /** Collect all directory paths from a tree for diff detection. */
  function collectPaths(nodes: AssetNode[]): string[] {
    const result: string[] = [];
    for (const n of nodes) {
      result.push(n.path);
      if (n.is_dir && n.children.length > 0) {
        result.push(...collectPaths(n.children));
      }
    }
    return result;
  }

  // Refresh when project changes (track only the path to avoid re-triggering
  // when refresh() updates file lists on the same project)
  let lastProjectPath = "";
  $effect(() => {
    const path = $project?.path ?? "";
    if (path !== lastProjectPath) {
      lastProjectPath = path;
      refresh();
    }
  });

  // Refresh when other components request it (e.g. spritesheet editor after save)
  let lastRefreshReq = 0;
  $effect(() => {
    const req = $assetRefreshRequest;
    if (req !== lastRefreshReq) {
      lastRefreshReq = req;
      if ($project) refresh();
    }
  });

  function toggleExpand(path: string) {
    const next = new Set(expanded);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    expanded = next;
  }

  function fileIcon(name: string): string {
    if (name.endsWith(".prefab")) return "🧩";
    if (name.endsWith(".atlas")) return "🎴";
    if (name.endsWith(".behavior")) return "🌳";
    if (name.endsWith(".sortlayers")) return "🪜";
    if (name.endsWith(".dataschema")) return "🧱";
    if (name.endsWith(".data")) return "🗃";
    const ext = name.split(".").pop()?.toLowerCase() ?? "";
    switch (ext) {
      case "png":
      case "jpg":
      case "jpeg":
        return "🖼";
      case "scene":
        return "📄";
      case "lua":
      case "luau":
        return "🌙";
      case "mp4":
      case "webm":
      case "mov":
        return "🎞";
      case "ttf":
      case "otf":
        return "🔤";
      case "js":
        return "📜";
      case "rs":
        return "🦀";
      case "toml":
        return "⚙";
      case "md":
        return "📝";
      case "gui":
        return "🖥";
      case "shader":
        return "🎨";
      case "material":
        return "🎭";
      case "particle":
        return "✨";
      case "anim":
        return "🎬";
      case "font":
        return "🔤";
      case "tilemap":
        return "🗺";
      case "skelanim":
        return "🦴";
      case "psd":
        return "🗂";
      case "rig":
        return "🩻";
      case "ogg":
      case "wav":
      case "mp3":
      case "flac":
        return "🔊";
      default:
        return "📁";
    }
  }

  async function handleDoubleClick(node: AssetNode) {
    if (node.is_dir) return;
    const proj = get(project);
    if (!proj) return;
    // The extension switch lives in `api/openAsset.ts` so Ctrl+P and
    // find-in-files results open the same editors this does.
    await openAsset(proj, node.path);
  }

  function handleContextMenu(event: MouseEvent, node: AssetNode | null) {
    event.preventDefault();
    contextMenu = { x: event.clientX, y: event.clientY, node };
  }

  function closeContextMenu() {
    contextMenu = null;
  }

  async function handleNewFolder() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets";
    closeContextMenu();

    const folderName = prompt("Folder name:");
    if (!folderName) return;

    const relativePath = parentPath
      ? `${parentPath}/${folderName}`
      : folderName;
    try {
      await createAssetFolder(proj.path, relativePath);
      logInfo(`Created folder: ${relativePath}`);
      await refresh();
      // Auto-expand parent
      if (parentPath) {
        const next = new Set(expanded);
        next.add(parentPath);
        expanded = next;
      }
    } catch (e) {
      logError(`Failed to create folder: ${e}`);
    }
  }

  async function handleDelete() {
    const node = contextMenu?.node;
    closeContextMenu();
    if (!node) return;

    const proj = get(project);
    if (!proj) return;

    // Say who still names it: a deleted texture is a sprite that silently
    // draws nothing.
    let users = "";
    try {
      const refs = await findAssetReferences(proj.path, node.path);
      if (refs.length > 0) {
        const files = [...new Set(refs.map((r) => r.path))];
        users =
          `\n\nStill used in ${refs.length} place(s) in ${files.length} file(s):\n` +
          files.slice(0, 8).join("\n") +
          (files.length > 8 ? `\n... and ${files.length - 8} more` : "");
      }
    } catch {
      // A failed scan should not block a delete.
    }
    const confirmed = confirm(
      `Delete "${node.name}"${node.is_dir ? " and all contents" : ""}?${users}`,
    );
    if (!confirmed) return;

    try {
      await deleteAsset(proj.path, node.path);
      logInfo(`Deleted: ${node.path}`);
      await refresh();
    } catch (e) {
      logError(`Failed to delete: ${e}`);
    }
  }

  function handleStartRename() {
    const node = contextMenu?.node;
    closeContextMenu();
    if (!node) return;
    beginRename(node);
  }

  // F2 anywhere outside a text field, plus the Rename command and the Edit
  // menu, all arrive here. Bound on the window rather than the row because
  // the row-level binding was the bug: it needed a focus the row never got.
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "F2" || isTextEditingTarget(e)) return;
      // The scene tree renames too; F2 belongs to whichever was clicked last.
      if ($lastRenameTarget !== "asset" || !selectedPath) return;
      e.preventDefault();
      renameSelected();
    };
    const onCommand = () => {
      if ($lastRenameTarget === "asset") renameSelected();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("editor-rename", onCommand);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("editor-rename", onCommand);
    };
  });

  // Registered while the panel is mounted, so the palette lists it only when
  // there is an asset browser to act on.
  $effect(() =>
    registerActions([
      {
        id: "asset.rename",
        title: "Rename Asset",
        category: "Assets",
        keys: "F2",
        enabled: () => selectedPath !== null,
        run: renameSelected,
      },
    ]),
  );

  /** Put a row into inline-rename mode. Shared by the context menu and F2. */
  function beginRename(node: AssetNode) {
    selectedPath = node.path;
    renamingPath = node.path;
    renamingWasDir = node.is_dir;
    renameValue = node.name;
  }

  /** Find a node by path anywhere in the tree. */
  function findNode(nodes: AssetNode[], path: string): AssetNode | null {
    for (const n of nodes) {
      if (n.path === path) return n;
      const hit = n.children ? findNode(n.children, path) : null;
      if (hit) return hit;
    }
    return null;
  }

  /** Rename whatever is selected - F2, the Rename command, Edit > Rename. */
  function renameSelected() {
    if (!selectedPath) {
      logError("Select an asset first, then rename it");
      return;
    }
    const node = findNode(tree, selectedPath);
    if (node) beginRename(node);
  }

  /**
   * Keys on a focused asset row.
   *
   * The tree has no selection model — rows are plain focusable elements — so
   * DOM focus stands in for selection, which is what a tree widget is supposed
   * to use anyway. F2 renames and Enter opens, the two bindings every file
   * tree has.
   */
  function handleRowKeydown(e: KeyboardEvent, node: AssetNode) {
    selectedPath = node.path;
    claimRenameTarget("asset");
    if (e.key === "F2") {
      e.preventDefault();
      beginRename(node);
    } else if (e.key === "Enter") {
      e.preventDefault();
      void handleDoubleClick(node);
    }
  }

  async function handleDuplicate() {
    const node = contextMenu?.node;
    closeContextMenu();
    if (!node || node.is_dir) return;

    const proj = get(project);
    if (!proj) return;

    // Build a unique destination name: foo.tilemap -> foo copy.tilemap,
    // then foo copy 2.tilemap, ... checking siblings in the same folder.
    const slash = node.path.lastIndexOf("/");
    const dir = slash >= 0 ? node.path.slice(0, slash) : "";
    const fileName = slash >= 0 ? node.path.slice(slash + 1) : node.path;
    const dot = fileName.lastIndexOf(".");
    const base = dot > 0 ? fileName.slice(0, dot) : fileName;
    const ext = dot > 0 ? fileName.slice(dot) : "";

    const siblings = new Set<string>();
    const collect = (nodes: AssetNode[]) => {
      for (const n of nodes) {
        const nslash = n.path.lastIndexOf("/");
        const ndir = nslash >= 0 ? n.path.slice(0, nslash) : "";
        if (ndir === dir) siblings.add(n.name);
        if (n.children) collect(n.children);
      }
    };
    collect(tree);

    let candidate = `${base} copy${ext}`;
    let i = 2;
    while (siblings.has(candidate)) {
      candidate = `${base} copy ${i}${ext}`;
      i += 1;
    }
    const newPath = dir ? `${dir}/${candidate}` : candidate;

    try {
      await duplicateAsset(proj.path, node.path, newPath);
      logInfo(`Duplicated: ${node.path} → ${newPath}`);
      await refresh();
    } catch (e) {
      logError(`Failed to duplicate: ${e}`);
    }
  }

  async function handleShowInExplorer() {
    const node = contextMenu?.node;
    closeContextMenu();
    if (!node) return;
    const proj = get(project);
    if (!proj) return;
    try {
      await showInFileManager(proj.path, node.path);
    } catch (e) {
      logError(`Failed to open file manager: ${e}`);
    }
  }

  async function handleRenameSubmit(oldPath: string) {
    const proj = get(project);
    if (!proj || !renamingPath) return;

    const parentDir = oldPath.includes("/")
      ? oldPath.substring(0, oldPath.lastIndexOf("/"))
      : "";
    const newPath = parentDir ? `${parentDir}/${renameValue}` : renameValue;

    renamingPath = null;
    if (newPath === oldPath) return;

    const folder = renamingWasDir;
    try {
      const summary = await renameAssetWithReferences(proj.path, oldPath, newPath);
      // The scenes open in the editor hold their own copies: rewrite them the
      // same way, or saving one would put the old path back.
      const memory = await rewriteOpenScenes(oldPath, newPath, folder);
      logInfo(
        `Renamed: ${oldPath} → ${newPath}` +
          (summary.references > 0
            ? ` - ${summary.references} reference(s) updated in ${summary.files_changed.length} file(s)`
            : "") +
          (memory > 0 ? `, ${memory} in open scenes` : ""),
      );
      for (const f of summary.failures) logError(`Could not update a reference: ${f}`);
      await refresh();
    } catch (e) {
      logError(`Failed to rename: ${e}`);
    }
  }

  /** Rewrite references in the open scene and every background scene tab. */
  async function rewriteOpenScenes(oldPath: string, newPath: string, folder: boolean): Promise<number> {
    let total = 0;
    const [text, n] = await rewriteAssetReferencesInText(JSON.stringify(get(scene)), oldPath, newPath, folder);
    if (n > 0) {
      scene.set(JSON.parse(text));
      total += n;
    }
    const tabs = get(sceneTabs);
    const rewritten = await Promise.all(
      tabs.map((t) => rewriteAssetReferencesInText(JSON.stringify(t.scene), oldPath, newPath, folder)),
    );
    if (rewritten.some(([, k]) => k > 0)) {
      sceneTabs.set(tabs.map((t, i) => (rewritten[i][1] > 0 ? { ...t, scene: JSON.parse(rewritten[i][0]) } : t)));
      total += rewritten.reduce((a, [, k]) => a + k, 0);
    }
    return total;
  }

  /** List every line naming the asset in the Console. */
  async function handleFindUsages() {
    const node = contextMenu?.node;
    closeContextMenu();
    const proj = get(project);
    if (!node || !proj) return;
    try {
      const refs = await findAssetReferences(proj.path, node.path);
      if (refs.length === 0) {
        logInfo(`Nothing in the project names ${node.path}`);
        return;
      }
      logInfo(`${refs.length} reference(s) to ${node.path}:`);
      for (const r of refs) logInfo(`  ${r.path}:${r.line}  ${r.text}`);
    } catch (e) {
      logError(`Failed to find usages: ${e}`);
    }
  }

  function handleRenameKeydown(event: KeyboardEvent, path: string) {
    if (event.key === "Enter") {
      handleRenameSubmit(path);
    } else if (event.key === "Escape") {
      renamingPath = null;
    }
  }

  async function handleImport() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets";
    closeContextMenu();

    const files = await open({
      multiple: true,
      title: "Import assets",
    });
    if (!files) return;

    const paths = Array.isArray(files) ? files : [files];
    for (const sourcePath of paths) {
      const fileName = (sourcePath as string).replace(/^.*[\\/]/, "");
      const destPath = parentPath ? `${parentPath}/${fileName}` : fileName;
      try {
        await importAsset(proj.path, sourcePath as string, destPath);
        logInfo(`Imported: ${fileName}`);
      } catch (e) {
        logError(`Failed to import ${fileName}: ${e}`);
      }
    }
    await refresh();
  }

  async function handleNewScene() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets";
    closeContextMenu();

    const sceneName = prompt("Scene name:");
    if (!sceneName) return;

    const fileName = sceneName.endsWith(".scene")
      ? sceneName
      : `${sceneName}.scene`;
    const relativePath = `${parentPath}/${fileName}`;
    const displayName = fileName.replace(/\.scene$/, "");

    const defaultScene = {
      name: displayName,
      entities: [],
    };

    try {
      await createAssetFile(
        proj.path,
        relativePath,
        JSON.stringify(defaultScene, null, 2),
      );
      logInfo(`Created scene: ${relativePath}`);
      await refresh();
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
    } catch (e) {
      logError(`Failed to create scene: ${e}`);
    }
  }

  async function handleNewScript() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets";
    closeContextMenu();

    newScriptDialog = { parentPath };
    newScriptType = SCRIPT_TYPES[0];
    newScriptName = "";
  }

  async function handleNewPrefab() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets/prefabs";
    closeContextMenu();

    const prefabName = prompt("Prefab name:");
    if (!prefabName) return;

    const fileName = prefabName.endsWith(".prefab")
      ? prefabName
      : `${prefabName}.prefab`;
    const relativePath = `${parentPath}/${fileName}`;

    const defaultEntity: EntityData = {
      name: prefabName.replace(".prefab", ""),
      components: {
        transform: { position: [0, 0], rotation: 0, scale: [1, 1] },
      },
    };

    try {
      // Ensure prefabs/ directory exists
      if (parentPath === "assets/prefabs") {
        await createAssetFolder(proj.path, "assets/prefabs").catch(() => {});
      }
      await createAssetFile(
        proj.path,
        relativePath,
        JSON.stringify(defaultEntity, null, 2),
      );
      logInfo(`Created prefab: ${relativePath}`);
      await refresh();
      // Auto-expand parent
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
    } catch (e) {
      logError(`Failed to create prefab: ${e}`);
    }
  }

  async function handleNewBehavior() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets/behaviors";
    closeContextMenu();

    const behaviorName = prompt("Behavior graph name:");
    if (!behaviorName) return;

    const fileName = behaviorName.endsWith(".behavior")
      ? behaviorName
      : `${behaviorName}.behavior`;
    const relativePath = `${parentPath}/${fileName}`;
    const displayName = fileName.replace(/\.behavior$/, "");

    try {
      // Ensure behaviors/ directory exists when using the default location
      if (parentPath === "assets/behaviors") {
        await createAssetFolder(proj.path, "assets/behaviors").catch(() => {});
      }
      await createAssetFile(
        proj.path,
        relativePath,
        defaultBehaviorTemplate(displayName),
      );
      logInfo(`Created behavior graph: ${relativePath}`);
      await refresh();
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
      // Open the new graph in the behavior editor immediately
      window.dispatchEvent(
        new CustomEvent("open-behavior-editor", {
          detail: { fullPath: `${proj.path}/${relativePath}` },
        }),
      );
    } catch (e) {
      logError(`Failed to create behavior graph: ${e}`);
    }
  }

  async function handleNewDataSchema() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets/data";
    closeContextMenu();

    const schemaName = prompt("Schema (data type) name:");
    if (!schemaName) return;

    const fileName = schemaName.endsWith(".dataschema")
      ? schemaName
      : `${schemaName}.dataschema`;
    const relativePath = `${parentPath}/${fileName}`;
    const displayName = fileName.replace(/\.dataschema$/, "");

    try {
      if (parentPath === "assets/data") {
        await createAssetFolder(proj.path, "assets/data").catch(() => {});
      }
      await createAssetFile(
        proj.path,
        relativePath,
        JSON.stringify(defaultDataSchema(displayName), null, 2),
      );
      logInfo(`Created data schema: ${relativePath}`);
      await refresh();
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
      window.dispatchEvent(
        new CustomEvent("open-data-schema-editor", {
          detail: { fullPath: `${proj.path}/${relativePath}` },
        }),
      );
    } catch (e) {
      logError(`Failed to create data schema: ${e}`);
    }
  }

  async function handleNewDataTable() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets/data";
    closeContextMenu();

    let schemas: SchemaEntry[] = [];
    try {
      schemas = await listDataSchemas(proj.path);
    } catch (e) {
      logError(`Failed to list schemas: ${e}`);
    }
    if (schemas.length === 0) {
      logError("No data schemas found. Create a schema first.");
      return;
    }
    newDataTableSchemas = schemas;
    newDataTableSchemaName = schemas[0].name;
    newDataTableName = "";
    newDataTableDialog = { parentPath };
  }

  async function submitNewDataTable() {
    const proj = get(project);
    if (!proj || !newDataTableDialog) return;
    const name = newDataTableName.trim();
    if (!name || !newDataTableSchemaName) return;

    const parentPath = newDataTableDialog.parentPath;
    const fileName = name.endsWith(".data") ? name : `${name}.data`;
    const relativePath = `${parentPath}/${fileName}`;
    const schemaName = newDataTableSchemaName;
    newDataTableDialog = null;

    try {
      if (parentPath === "assets/data") {
        await createAssetFolder(proj.path, "assets/data").catch(() => {});
      }
      await createAssetFile(
        proj.path,
        relativePath,
        JSON.stringify(defaultDataTable(schemaName), null, 2),
      );
      logInfo(`Created data table: ${relativePath}`);
      await refresh();
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
      window.dispatchEvent(
        new CustomEvent("open-data-table-editor", {
          detail: { fullPath: `${proj.path}/${relativePath}` },
        }),
      );
    } catch (e) {
      logError(`Failed to create data table: ${e}`);
    }
  }

  function cancelNewDataTable() {
    newDataTableDialog = null;
  }

  async function handleNewGui() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets/ui";
    closeContextMenu();

    const guiName = prompt("GUI layout name:");
    if (!guiName) return;

    const fileName = guiName.endsWith(".gui") ? guiName : `${guiName}.gui`;
    const relativePath = parentPath ? `${parentPath}/${fileName}` : fileName;

    try {
      if (parentPath) {
        await createAssetFolder(proj.path, parentPath).catch(() => {});
      }
      await createGuiFile(proj.path, relativePath, guiName.replace(".gui", ""));
      logInfo(`Created GUI layout: ${relativePath}`);
      await refresh();
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
    } catch (e) {
      logError(`Failed to create GUI layout: ${e}`);
    }
  }

  async function handleNewAtlas() {
    const proj = get(project);
    if (!proj) return;
    closeContextMenu();

    const atlasName = prompt("Atlas name:");
    if (!atlasName) return;

    try {
      const relPath = await createAtlas(proj.path, atlasName);
      logInfo(`Created atlas: ${relPath}`);
      await refresh();
      const next = new Set(expanded);
      next.add("assets/sprites");
      expanded = next;
      // Open the atlas editor immediately
      window.dispatchEvent(
        new CustomEvent("open-atlas-editor", {
          detail: { projectPath: proj.path, atlasPath: relPath },
        }),
      );
    } catch (e) {
      logError(`Failed to create atlas: ${e}`);
    }
  }

  async function handleNewMaterial() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets";
    closeContextMenu();

    const materialName = prompt("Material name:");
    if (!materialName) return;

    const fileName = materialName.endsWith(".material")
      ? materialName
      : `${materialName}.material`;
    const relativePath = `${parentPath}/${fileName}`;

    const defaultMaterial = {
      shader: "default_sprite",
      blend_mode: "Alpha",
    };

    try {
      await createAssetFile(
        proj.path,
        relativePath,
        JSON.stringify(defaultMaterial, null, 2),
      );
      logInfo(`Created material: ${relativePath}`);
      await refresh();
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
    } catch (e) {
      logError(`Failed to create material: ${e}`);
    }
  }

  async function handleNewShader() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets";
    closeContextMenu();

    const shaderName = prompt("Shader name:");
    if (!shaderName) return;

    const fileName = shaderName.endsWith(".shader")
      ? shaderName
      : `${shaderName}.shader`;
    const relativePath = `${parentPath}/${fileName}`;

    try {
      const graphJson = await shaderCreateNew(
        shaderName.replace(".shader", ""),
      );
      // shaderSave prefixes with assets/ internally, so strip it
      const savePath = relativePath.startsWith("assets/")
        ? relativePath.slice(7)
        : relativePath;
      await shaderSave(proj.path, savePath, graphJson);
      logInfo(`Created shader: ${relativePath}`);
      await refresh();
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
      // Open the shader editor immediately
      window.dispatchEvent(
        new CustomEvent("open-shader-editor", {
          detail: { projectPath: proj.path, relativePath: savePath },
        }),
      );
    } catch (e) {
      logError(`Failed to create shader: ${e}`);
    }
  }

  async function handleNewParticle() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets";
    closeContextMenu();

    const emitterName = prompt("Particle emitter name:");
    if (!emitterName) return;

    const fileName = emitterName.endsWith(".particle")
      ? emitterName
      : `${emitterName}.particle`;
    const relativePath = `${parentPath}/${fileName}`;

    try {
      const content = JSON.stringify(defaultParticleEmitter(), null, 2);
      await createAssetFile(proj.path, relativePath, content);
      logInfo(`Created particle emitter: ${relativePath}`);
      await refresh();
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
      // Open the particle editor immediately
      const fullPath = `${proj.path}/${relativePath}`;
      window.dispatchEvent(
        new CustomEvent("open-particle-editor", {
          detail: { fullPath },
        }),
      );
    } catch (e) {
      logError(`Failed to create particle emitter: ${e}`);
    }
  }

  /** A layered `.psd` as a sprite per layer and a prefab of them, opened
   *  in the prefab editor when done. `bottom` puts the prefab's root at the
   *  canvas's bottom middle - a character's feet. */
  async function handleImportPsd(bottom: boolean) {
    const proj = get(project);
    const node = contextMenu?.node;
    closeContextMenu();
    if (!proj || !node) return;
    try {
      const report = await importPsdLayers(proj.path, node.path, bottom ? "bottom" : "centre");
      logInfo(`Imported ${node.path}: ${report.prefab} with ${report.sprites.length} layer sprite(s)`);
      for (const s of report.skipped) logInfo(`  left out: ${s}`);
      await refresh();
      window.dispatchEvent(new CustomEvent("open-prefab-editor", { detail: { fullPath: `${proj.path}/assets/${report.prefab}` } }));
    } catch (e) {
      logError(`Could not import ${node.path}: ${e}`);
    }
  }

  /** A new `.skelanim` clip or `.rig`, opened in the skeletal editor. */
  async function handleNewSkeletal(kind: "skelanim" | "rig") {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir ? contextMenu.node.path : kind === "rig" ? "assets/rigs" : "assets/animations";
    closeContextMenu();
    const name = prompt(kind === "rig" ? "Rig name:" : "Skeletal clip name:");
    if (!name) return;
    const base = name.replace(new RegExp(`\\.${kind}$`), "");
    const relativePath = `${parentPath}/${base}.${kind}`;
    const content =
      kind === "rig"
        ? JSON.stringify({ name: base, bones: [{ name: "root", position: [0, 0], rotation: 0, length: 50 }] }, null, 2)
        : JSON.stringify({ name: base, duration: 1, tracks: [] }, null, 2);
    try {
      await createAssetFolder(proj.path, parentPath).catch(() => {});
      await createAssetFile(proj.path, relativePath, content + "\n");
      logInfo(`Created ${relativePath}`);
      await refresh();
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
      window.dispatchEvent(new CustomEvent("open-skeletal-editor", { detail: { projectPath: proj.path, relativePath } }));
    } catch (e) {
      logError(`Failed to create ${relativePath}: ${e}`);
    }
  }

  async function handleNewAnim() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets";
    closeContextMenu();

    const animName = prompt("Animation name:");
    if (!animName) return;

    const fileName = animName.endsWith(".anim") ? animName : `${animName}.anim`;
    const relativePath = `${parentPath}/${fileName}`;

    try {
      const content = JSON.stringify(defaultAnimationClip(), null, 2);
      await createAssetFile(proj.path, relativePath, content);
      logInfo(`Created animation: ${relativePath}`);
      await refresh();
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
      // Open the animation editor immediately
      const fullPath = `${proj.path}/${relativePath}`;
      window.dispatchEvent(
        new CustomEvent("open-animation-editor", {
          detail: { fullPath },
        }),
      );
    } catch (e) {
      logError(`Failed to create animation: ${e}`);
    }
  }

  async function handleNewFont() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets";
    closeContextMenu();

    const fontName = prompt("Font name:");
    if (!fontName) return;

    const fileName = fontName.endsWith(".font") ? fontName : `${fontName}.font`;
    const relativePath = `${parentPath}/${fileName}`;

    try {
      const content = JSON.stringify(defaultFontFile(), null, 2);
      await createAssetFile(proj.path, relativePath, content);
      logInfo(`Created font: ${relativePath}`);
      await refresh();
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
      // Open the font editor immediately
      const fullPath = `${proj.path}/${relativePath}`;
      window.dispatchEvent(
        new CustomEvent("open-font-editor", {
          detail: { fullPath },
        }),
      );
    } catch (e) {
      logError(`Failed to create font: ${e}`);
    }
  }

  async function handleNewTilemap() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets";
    closeContextMenu();

    const name = prompt("Tilemap name:");
    if (!name) return;

    const fileName = name.endsWith(".tilemap") ? name : `${name}.tilemap`;
    const relativePath = `${parentPath}/${fileName}`;

    try {
      const content = JSON.stringify(defaultTilemap(), null, 2);
      await createAssetFile(proj.path, relativePath, content);
      logInfo(`Created tilemap: ${relativePath}`);
      await refresh();
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
      const fullPath = `${proj.path}/${relativePath}`;
      window.dispatchEvent(
        new CustomEvent("open-tilemap-editor", {
          detail: { fullPath },
        }),
      );
    } catch (e) {
      logError(`Failed to create tilemap: ${e}`);
    }
  }

  async function handleNewSortLayers() {
    const proj = get(project);
    if (!proj) return;
    const parentPath = contextMenu?.node?.is_dir
      ? contextMenu.node.path
      : "assets";
    closeContextMenu();

    const name = prompt("Sort layers asset name:");
    if (!name) return;

    const fileName = name.endsWith(".sortlayers") ? name : `${name}.sortlayers`;
    const relativePath = `${parentPath}/${fileName}`;

    try {
      const content = JSON.stringify(defaultSortLayersFile(), null, 2);
      await createAssetFile(proj.path, relativePath, content);
      logInfo(`Created sort layers: ${relativePath}`);
      await refresh();
      const next = new Set(expanded);
      next.add(parentPath);
      expanded = next;
      const fullPath = `${proj.path}/${relativePath}`;
      window.dispatchEvent(
        new CustomEvent("open-sortlayers-editor", {
          detail: { fullPath },
        }),
      );
    } catch (e) {
      logError(`Failed to create sort layers asset: ${e}`);
    }
  }

  async function submitNewScript() {
    const proj = get(project);
    if (!proj || !newScriptDialog) return;
    const name = newScriptName.trim();
    if (!name) return;

    const fileName = name.endsWith(newScriptType.ext)
      ? name
      : name + newScriptType.ext;
    const relativePath = newScriptDialog.parentPath
      ? `${newScriptDialog.parentPath}/${fileName}`
      : fileName;

    newScriptDialog = null;

    try {
      await createAssetFile(proj.path, relativePath, newScriptType.template);
      logInfo(`Created script: ${relativePath}`);
      await refresh();
    } catch (e) {
      logError(`Failed to create script: ${e}`);
    }
  }

  function cancelNewScript() {
    newScriptDialog = null;
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="asset-browser"
  onclick={closeContextMenu}
  oncontextmenu={(e) => handleContextMenu(e, null)}
>
  <div class="header">
    <span>Assets</span>
    <button onclick={() => refresh()} title="Refresh">↻</button>
  </div>

  {#if !$project}
    <div class="empty">Open a project to browse assets</div>
  {:else if tree.length === 0}
    <div class="empty">
      No assets yet
      <button
        class="link-btn"
        onclick={(e) => {
          e.stopPropagation();
          handleContextMenu(
            new MouseEvent("contextmenu", {
              clientX: e.clientX || 100,
              clientY: e.clientY || 100,
            }),
            null,
          );
        }}
      >
        Right-click to add
      </button>
    </div>
  {:else}
    <div class="tree">
      {#snippet renderNode(node: AssetNode, depth: number)}
        <div
          class="tree-item"
          style="padding-left: {8 + depth * 16}px"
          draggable={!node.is_dir && isDroppableAsset(node.name)}
          ondragstart={(e) => {
            if (node.is_dir || !isDroppableAsset(node.name)) return;
            e.dataTransfer?.setData(
              ASSET_DRAG_MIME,
              JSON.stringify({ path: node.path, name: node.name }),
            );
            e.dataTransfer!.effectAllowed = "copy";
          }}
          ondblclick={() => handleDoubleClick(node)}
          onpointerdown={(e) => {
            selectedPath = node.path;
            claimRenameTarget("asset");
            (e.currentTarget as HTMLElement).focus();
          }}
          onkeydown={(e) => handleRowKeydown(e, node)}
          oncontextmenu={(e) => {
            e.stopPropagation();
            handleContextMenu(e, node);
          }}
          class:selected={selectedPath === node.path}
          role="treeitem"
          tabindex="0"
          aria-selected={selectedPath === node.path}
        >
          {#if node.is_dir}
            <button class="expand-btn" onclick={() => toggleExpand(node.path)}>
              {expanded.has(node.path) ? "▾" : "▸"}
            </button>
            <!-- The project's Rust packages, listed after the assets. -->
            <span class="icon">{node.path === "packages" ? "📦" : "📁"}</span>
          {:else}
            <span class="expand-spacer"></span>
            <span class="icon">{fileIcon(node.name)}</span>
          {/if}

          {#if renamingPath === node.path}
            <input
              class="rename-input"
              type="text"
              bind:value={renameValue}
              onblur={() => handleRenameSubmit(node.path)}
              onkeydown={(e) => handleRenameKeydown(e, node.path)}
            />
          {:else}
            <span class="node-name" class:is-dir={node.is_dir}>{node.name}</span
            >
          {/if}
        </div>

        {#if node.is_dir && expanded.has(node.path)}
          {#each node.children as child}
            {@render renderNode(child, depth + 1)}
          {/each}
          {#if node.children.length === 0}
            <div
              class="empty-folder"
              style="padding-left: {8 + (depth + 1) * 16}px"
            >
              (empty)
            </div>
          {/if}
        {/if}
      {/snippet}

      {#each tree as node}
        {@render renderNode(node, 0)}
      {/each}
    </div>
  {/if}
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
      use:clampToViewport
    >
      <button class="menu-item" onclick={handleNewFolder}>📁 New Folder</button>
      <button class="menu-item" onclick={handleNewScene}>📄 New Scene</button>
      <button class="menu-item" onclick={handleNewScript}>📜 New Script</button>
      <button class="menu-item" onclick={handleNewPrefab}>🧩 New Prefab</button>
      <button class="menu-item" onclick={handleNewBehavior}
        >🌳 New Behavior</button
      >
      <button class="menu-item" onclick={handleNewGui}>🖥 New GUI Layout</button
      >
      <button class="menu-item" onclick={handleNewAtlas}>🎴 New Atlas</button>
      <button class="menu-item" onclick={handleNewMaterial}
        >🎭 New Material</button
      >
      <button class="menu-item" onclick={handleNewShader}>🎨 New Shader</button>
      <button class="menu-item" onclick={handleNewParticle}
        >✨ New Particle Emitter</button
      >
      <button class="menu-item" onclick={handleNewAnim}>🎬 New Animation</button
      >
      <button class="menu-item" onclick={() => handleNewSkeletal("skelanim")}>🦴 New Skeletal Clip</button>
      <button class="menu-item" onclick={() => handleNewSkeletal("rig")}>🩻 New Rig</button>
      <button class="menu-item" onclick={handleNewFont}>🔤 New Font</button>
      <button class="menu-item" onclick={handleNewTilemap}
        >🗺 New Tilemap</button
      >
      <button class="menu-item" onclick={handleNewSortLayers}
        >🪜 New Sort Layers</button
      >
      <button class="menu-item" onclick={handleNewDataSchema}
        >🧱 New Data Schema</button
      >
      <button class="menu-item" onclick={handleNewDataTable}
        >🗃 New Data Table</button
      >
      <button class="menu-item" onclick={handleImport}
        >📥 Import Files...</button
      >
      {#if contextMenu.node}
        <div class="menu-divider"></div>
        {#if ["ogg", "wav", "mp3", "flac"].includes(contextMenu.node.path
            .split(".")
            .pop()
            ?.toLowerCase() ?? "")}
          <button
            class="menu-item"
            onclick={() => {
              const node = contextMenu?.node;
              if (!node) return;
              const proj = get(project);
              if (!proj) return;
              const relPath = node.path.startsWith("assets/")
                ? node.path.slice("assets/".length)
                : node.path;
              closeContextMenu();
              window.dispatchEvent(
                new CustomEvent("open-audio-preview", {
                  detail: { projectPath: proj.path, relativePath: relPath },
                }),
              );
            }}>▶ Play Audio</button
          >
        {/if}
        {#if ["png", "jpg", "jpeg", "webp"].includes(contextMenu.node.path.split(".").pop()?.toLowerCase() ?? "")}
          <button
            class="menu-item"
            onclick={() => {
              const node = contextMenu?.node;
              closeContextMenu();
              if (node)
                window.dispatchEvent(new CustomEvent("open-sheet-slicer", { detail: { imagePath: node.path } }));
            }}>✂ Slice sprite sheet…</button
          >
          <button
            class="menu-item"
            onclick={() => {
              const node = contextMenu?.node;
              closeContextMenu();
              // A new character in the open scene, rigged from this picture
              // (the skin editor makes its root on Apply).
              if (node)
                window.dispatchEvent(
                  new CustomEvent("open-skin-editor", { detail: { index: -1, texture: node.path.replace(/^assets\//, "") } }),
                );
            }}>🦴 Rig as a character…</button
          >
        {/if}
        {#if contextMenu.node.path.toLowerCase().endsWith(".psd")}
          <button class="menu-item" onclick={() => handleImportPsd(false)}>🗂 Import layers as a prefab</button>
          <button class="menu-item" onclick={() => handleImportPsd(true)}>🧍 Import layers as a character (origin at the feet)</button>
        {/if}
        <button class="menu-item" onclick={handleShowInExplorer}
          >📂 Show in Explorer</button
        >
        <button class="menu-item" onclick={handleStartRename}>✏️ Rename</button>
        <button class="menu-item" onclick={handleFindUsages}>🔗 Find usages</button>
        {#if !contextMenu?.node?.is_dir}
          <button class="menu-item" onclick={handleDuplicate}
            >⧉ Duplicate</button
          >
        {/if}
        <button class="menu-item danger" onclick={handleDelete}
          >🗑️ Delete</button
        >
      {/if}
    </div>
  </div>
{/if}

{#if newScriptDialog}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog-overlay" onclick={cancelNewScript}>
    <div class="dialog" onclick={(e) => e.stopPropagation()}>
      <div class="dialog-title">New Script</div>

      <div class="dialog-field">
        <label class="dialog-label">Type</label>
        <div class="script-type-row">
          {#each SCRIPT_TYPES as st}
            <button
              class="script-type-btn"
              class:selected={newScriptType === st}
              onclick={() => {
                newScriptType = st;
              }}
            >
              {st.label}<span class="ext-hint">{st.ext}</span>
            </button>
          {/each}
        </div>
      </div>

      <div class="dialog-field">
        <label class="dialog-label" for="script-name-input">Name</label>
        <div class="name-input-row">
          <input
            id="script-name-input"
            class="dialog-input"
            type="text"
            placeholder="my_script"
            bind:value={newScriptName}
            onkeydown={(e) => {
              if (e.key === "Enter") submitNewScript();
              else if (e.key === "Escape") cancelNewScript();
            }}
          />
          <span class="ext-suffix">{newScriptType.ext}</span>
        </div>
      </div>

      <div class="dialog-actions">
        <button class="btn btn-secondary" onclick={cancelNewScript}
          >Cancel</button
        >
        <button
          class="btn btn-primary"
          onclick={submitNewScript}
          disabled={!newScriptName.trim()}>Create</button
        >
      </div>
    </div>
  </div>
{/if}

{#if newDataTableDialog}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog-overlay" onclick={cancelNewDataTable}>
    <div class="dialog" onclick={(e) => e.stopPropagation()}>
      <div class="dialog-title">New Data Table</div>

      <div class="dialog-field">
        <label class="dialog-label" for="data-table-schema">Schema</label>
        <select
          id="data-table-schema"
          class="dialog-input"
          bind:value={newDataTableSchemaName}
        >
          {#each newDataTableSchemas as s}
            <option value={s.name}>{s.name}</option>
          {/each}
        </select>
      </div>

      <div class="dialog-field">
        <label class="dialog-label" for="data-table-name">Name</label>
        <div class="name-input-row">
          <input
            id="data-table-name"
            class="dialog-input"
            type="text"
            placeholder="monsters"
            bind:value={newDataTableName}
            onkeydown={(e) => {
              if (e.key === "Enter") submitNewDataTable();
              else if (e.key === "Escape") cancelNewDataTable();
            }}
          />
          <span class="ext-suffix">.data</span>
        </div>
      </div>

      <div class="dialog-actions">
        <button class="btn btn-secondary" onclick={cancelNewDataTable}
          >Cancel</button
        >
        <button
          class="btn btn-primary"
          onclick={submitNewDataTable}
          disabled={!newDataTableName.trim() || !newDataTableSchemaName}
          >Create</button
        >
      </div>
    </div>
  </div>
{/if}

<style>
  .asset-browser {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--ed-base);
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
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

  .header button {
    background: none;
    border: none;
    color: var(--ed-faint);
    cursor: pointer;
    font-size: 12.5px;
    padding: 0 4px;
  }

  .header button:hover {
    color: var(--ed-text);
  }

  .tree {
    flex: 1;
    overflow-y: auto;
    padding: 4px 0;
  }

  .tree-item {
    display: flex;
    align-items: center;
    gap: 4px;
    padding-right: 8px;
    height: 24px;
    cursor: pointer;
    font-size: 13px;
    color: var(--ed-text);
    user-select: none;
  }

  /* The selected row is what F2 and the Rename command act on, so it has to
     be visible - the absence of any such marker is why F2 read as broken
     rather than as "nothing is selected". */
  .tree-item.selected {
    background: var(--ed-raised);
    box-shadow: inset 2px 0 0 var(--ed-accent);
  }

  .tree-item:focus {
    outline: none;
  }

  .tree-item:hover {
    background: var(--ed-raised);
  }

  .expand-btn {
    background: none;
    border: none;
    color: var(--ed-faint);
    cursor: pointer;
    font-size: 10px;
    padding: 0;
    width: 14px;
    text-align: center;
    flex-shrink: 0;
  }

  .expand-spacer {
    width: 14px;
    flex-shrink: 0;
  }

  .icon {
    font-size: 12px;
    flex-shrink: 0;
  }

  .node-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .node-name.is-dir {
    font-weight: 500;
  }

  .rename-input {
    background: var(--ed-raised);
    border: 1px solid var(--ed-accent);
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 12px;
    padding: 1px 4px;
    flex: 1;
    min-width: 0;
    font-family: inherit;
  }

  .empty-folder {
    font-size: 11px;
    color: var(--ed-faint);
    font-style: italic;
    height: 20px;
    display: flex;
    align-items: center;
  }

  .empty {
    padding: 12px 10px;
    color: var(--ed-faint);
    font-size: 12px;
    font-style: italic;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .link-btn {
    background: none;
    border: none;
    color: var(--ed-accent);
    cursor: pointer;
    font-size: 12px;
    padding: 0;
    text-align: left;
  }

  .link-btn:hover {
    text-decoration: underline;
  }

  /* Context menu */
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
    background: none;
    border: none;
    color: var(--ed-text);
    padding: 6px 12px;
    font-size: 12px;
    cursor: pointer;
    font-family: inherit;
    border-radius: 4px;
  }

  .menu-item:hover {
    background: var(--ed-accent-low);
    color: var(--ed-ink);
  }

  .menu-item.danger:hover {
    background: var(--ed-accent-low);
    color: var(--ed-err);
  }

  .menu-divider {
    height: 1px;
    background: var(--ed-line);
    margin: 4px 0;
  }

  /* New script dialog */
  .dialog-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 2000;
  }

  .dialog {
    background: var(--ed-base);
    border: 1px solid var(--ed-line-strong);
    border-radius: 8px;
    padding: 16px 20px;
    min-width: 300px;
    max-width: 400px;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
  }

  .dialog-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--ed-text);
    margin-bottom: 14px;
  }

  .dialog-field {
    margin-bottom: 12px;
  }

  .dialog-label {
    display: block;
    font-family: var(--ed-mono);
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ed-muted);
    margin-bottom: 4px;
  }

  .script-type-row {
    display: flex;
    gap: 6px;
  }

  .script-type-btn {
    flex: 1;
    padding: 6px 10px;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-muted);
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 6px;
    justify-content: center;
    transition:
      border-color 0.1s,
      color 0.1s;
  }

  .script-type-btn:hover {
    border-color: var(--ed-line-strong);
    color: var(--ed-text);
  }

  .script-type-btn.selected {
    border-color: var(--ed-accent);
    color: var(--ed-text);
    background: color-mix(in srgb, var(--ed-accent) 10%, transparent);
  }

  .ext-hint {
    font-size: 10px;
    color: var(--ed-faint);
  }

  .script-type-btn.selected .ext-hint {
    color: var(--ed-accent);
  }

  .name-input-row {
    display: flex;
    align-items: center;
    gap: 0;
  }

  .dialog-input {
    flex: 1;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px 0 0 4px;
    color: var(--ed-text);
    font-size: 13px;
    padding: 6px 8px;
    font-family: inherit;
    outline: none;
  }

  .dialog-input:focus {
    border-color: var(--ed-accent);
  }

  .ext-suffix {
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line-strong);
    border-left: none;
    border-radius: 0 4px 4px 0;
    padding: 6px 8px;
    font-size: 13px;
    color: var(--ed-faint);
    user-select: none;
  }

  .dialog-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }

  .btn {
    padding: 6px 14px;
    font-size: 12px;
    font-family: inherit;
    border-radius: 4px;
    border: none;
    cursor: pointer;
  }

  .btn-secondary {
    background: var(--ed-raised);
    color: var(--ed-muted);
  }

  .btn-secondary:hover {
    background: var(--ed-line);
    color: var(--ed-text);
  }

  .btn-primary {
    background: var(--ed-accent);
    color: var(--ed-on-accent);
    font-weight: 600;
  }

  .btn-primary:hover {
    background: var(--ed-accent-high);
  }

  .btn-primary:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
