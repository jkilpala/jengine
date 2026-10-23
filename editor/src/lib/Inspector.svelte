<script lang="ts">
  import LayerMaskField from "./LayerMaskField.svelte";
  import { get } from "svelte/store";
  import { scene, dirty, scenePath } from "./stores/sceneStore";
  import {
    selectedEntity,
    selectedEntities,
    sceneSelected,
  } from "./stores/selectionStore";
  import { isPlaying } from "./stores/playStore";
  import { runtimeEntity } from "./stores/runtimeStore";
  import { execute } from "./stores/commandStore";
  import { updateEntityName } from "./api/scene";
  import {
    createAssetFile,
    refreshProjectFiles,
    listAssets,
    listProjectImages,
    readAssetBinary,
  } from "./api/project";
  import { project, requestAssetRefresh } from "./stores/projectStore";
  import {
    projectFontIds,
    isBuiltinFontId,
  } from "./stores/projectFontsStore";
  import { logInfo, logError } from "./stores/consoleStore";
  import { previewAtlas, listAtlasEntries } from "./api/spritesheet";
  import type { AtlasRegion, AtlasPickerEntry } from "./api/spritesheet";
  import { listAtlasDefImages, listAtlasDefAnimations } from "./api/atlas";
  import type { AtlasDefImageEntry, AtlasDefAnimationEntry } from "./api/atlas";
  import { readAnimFile } from "./api/animation";
  import type { AnimationClipData } from "./api/animation";
  import type { EntityData, PostProcessEffectSettings } from "./api/scene";
  import * as pp from "./postProcess";
  import type { ComponentDef, ComponentFieldDef } from "./api/project";
  import { PARTICLE_PRESETS } from "./api/particle-presets";
  import { readParticleFile } from "./api/particle";
  import { readTilemapFile } from "./api/tilemap";
  import { ASSET_DRAG_MIME } from "./api/assetEntity";
  import { defaultBehaviorTemplate } from "./api/behavior";
  import { openTilemapFile } from "./stores/tilemapEditorStore";
  import { viewport_set_runtime_entity } from "./viewport-wasm/jengine_editor_viewport.js";
  import { guiDocument } from "./stores/guiEditorStore";
  import { polygonEditEntity, polygonEditPath } from "./stores/viewportStore";
  import GuiInspector from "./GuiInspector.svelte";
  import PrefabInspector from "./PrefabInspector.svelte";

  let {
    showGuiInspector = false,
    showPrefabInspector = false,
    onmanagecomponents,
  }: {
    showGuiInspector?: boolean;
    showPrefabInspector?: boolean;
    onmanagecomponents?: () => void;
  } = $props();
  import { prefabData } from "./stores/prefabEditorStore";
  import PackageInspector from "./PackageInspector.svelte";
  import {
    applyToPrefab,
    editThroughInstance,
    instanceChanges,
    resolveInstance,
    revertPath,
    unpackInstance,
    viewOf,
    partOf,
    changedByComponent,
    fieldChanged,
    type Components,
  } from "./prefabInstance";
  import { notifyPrefabsChanged, prefabLibrary, scenePrefabs } from "./stores/prefabLibraryStore";
  import { carryEdit, mixedFields } from "./sceneEditing";
  import BehaviorTreeEditor from "./BehaviorTreeEditor.svelte";
  import SkeletalPlayerExtras from "./SkeletalPlayerExtras.svelte";
  import SpriteFillEditor from "./SpriteFillEditor.svelte";
  import { clipId, skeletonFromScene } from "./skelEdit";
  import { projectAssets } from "./stores/projectAssetsStore";
  import type { BtNode } from "./behaviorTree";
  import { writeTextFile } from "@tauri-apps/plugin-fs";
  import { openAsset } from "./api/openAsset";
  import ParticleExtras from "./ParticleExtras.svelte";
  import { packageSchema } from "./stores/packageSchemaStore";
  import type { ComponentEditorInfo } from "./api/packages";
  import {
    shaderEditorGraph,
    shaderSelectedNodeId,
    shaderRequestDraw,
    shaderMarkDirty,
  } from "./stores/shaderEditorStore";
  import type {
    NodeType,
    ShaderNode,
    PinType,
    CustomPin,
  } from "./api/shaderEditor";
  import { shaderGeneratePreviewGlsl } from "./api/shaderEditor";
  import {
    KNOWN_COMPONENTS,
    BUILTIN_KEYS,
    COMPONENT_DEFAULTS,
    COMPONENT_DEPS,
    categorizeComponents,
    UNDRAWN_FIELDS,
  } from "./componentDefs";

  let sceneEntity: EntityData | null = $derived(
    $selectedEntity >= 0 && $selectedEntity < $scene.entities.length
      ? $scene.entities[$selectedEntity]
      : null,
  );

  /** The id of the prefab the selected entity is an instance of, if any. */
  let instanceId: string | null = $derived(
    ((sceneEntity?.components as Components | undefined)?.prefab_instance as { prefab_id?: string } | undefined)
      ?.prefab_id ?? null,
  );

  /** The prefab an instance is built from (its variant base merged in), or
   *  null - for a plain entity, or an instance whose prefab is not found. */
  let instanceTemplate: Components | null = $derived(
    instanceId ? ($prefabLibrary.resolved.get(instanceId)?.components ?? null) : null,
  );

  /** What the Inspector shows: a linked instance resolved through its prefab
   *  (`prefabInstance.ts`), anything else as stored. Edits to it are turned
   *  back into overrides by `editEntity`. */
  let viewedEntity: EntityData | null = $derived(sceneEntity ? viewOf(sceneEntity, $scenePrefabs) : null);

  /** The prefab part the selected entity is laid over in this scene
   *  (`prefabParts`), if it is one. */
  let partTemplate = $derived(sceneEntity && !instanceId ? (partOf(sceneEntity, $scenePrefabs) ?? null) : null);

  // During play mode, show live data from WASM; otherwise show scene store data
  let entity: EntityData | null = $derived(
    $isPlaying ? $runtimeEntity : viewedEntity,
  );

  let playing = $derived($isPlaying);

  /** Map of package component name → ComponentEditorInfo for non-builtin package components. */
  let packageComponentMap = $derived.by(() => {
    const map = new Map<string, ComponentEditorInfo>();
    if (!$packageSchema) return map;
    for (const info of $packageSchema.components) {
      if (!BUILTIN_KEYS.has(info.component_name)) {
        map.set(info.component_name, info);
      }
    }
    return map;
  });

  /** Derived schemas of the components with hand-written sections - what
   *  draws the fields a section leaves out (`undrawnFields`). */
  let builtinSchemaMap = $derived.by(() => {
    const map = new Map<string, ComponentEditorInfo>();
    for (const info of $packageSchema?.components ?? []) {
      if (BUILTIN_KEYS.has(info.component_name)) map.set(info.component_name, info);
    }
    return map;
  });

  /** Set one field of a component through the undo history; `undefined`
   *  removes the field, as an unset optional is left out of the scene. */
  /** Drag a component's point list in the scene view (`pointFields.ts`). */
  function editPointsInViewport(comp: string, field: string) {
    polygonEditPath.set(`${comp}.${field}`);
    polygonEditEntity.set($selectedEntity);
  }

  function setComponentField(comp: string, field: string, value: unknown) {
    editEntity(`Change ${comp}.${field}`, (ent) => {
      const c = (ent.components as Record<string, unknown>)[comp];
      if (c && typeof c === "object") {
        if (value === undefined) delete (c as Record<string, unknown>)[field];
        else (c as Record<string, unknown>)[field] = value;
      }
    });
  }

  /** Check if a component key is present on the entity. */
  function hasComponent(ent: EntityData, key: string): boolean {
    return (ent.components as Record<string, unknown>)[key] != null;
  }

  /** For each component present on the entity, list its missing required dependencies. */
  let missingDeps: Record<string, string[]> = $derived.by(() => {
    const result: Record<string, string[]> = {};
    if (!entity) return result;
    for (const [comp, deps] of Object.entries(COMPONENT_DEPS)) {
      if (!hasComponent(entity, comp)) continue;
      const missing = deps.filter((d) => !hasComponent(entity, d));
      if (missing.length > 0) result[comp] = missing;
    }
    return result;
  });

  /**
   * Components that cannot be removed because other present components depend on them.
   * Returns a map of component → list of dependents that need it.
   */
  let removeDeps: Record<string, string[]> = $derived.by(() => {
    const result: Record<string, string[]> = {};
    if (!entity) return result;
    for (const [comp, deps] of Object.entries(COMPONENT_DEPS)) {
      if (!hasComponent(entity, comp)) continue;
      for (const dep of deps) {
        if (hasComponent(entity, dep)) {
          if (!result[dep]) result[dep] = [];
          result[dep].push(comp);
        }
      }
    }
    return result;
  });

  /** Send a temporary edit to the WASM runtime during play mode (not saved to scene). */
  function runtimeEdit(mutate: (ent: EntityData) => void) {
    const idx = get(selectedEntity);
    const ent = get(runtimeEntity);
    if (idx < 0 || !ent) return;
    const clone = structuredClone(ent);
    mutate(clone);
    try {
      viewport_set_runtime_entity(idx, JSON.stringify(clone));
    } catch (e) {
      logError(`Runtime edit failed: ${e}`);
    }
  }

  /** Edit helper: during play mode, edits are temporary (WASM only); during edit mode, uses undo-able execute(). */
  function editEntity(label: string, mutate: (ent: EntityData) => void) {
    if (playing) {
      runtimeEdit(mutate);
    } else {
      const idx = $selectedEntity;
      const prefabs = $scenePrefabs;
      const others = [...$selectedEntities].filter((i) => i !== idx);
      execute(others.length > 0 ? `${label} (${others.length + 1} entities)` : label, (s) => {
        const ent = s.entities[idx];
        if (!ent) return s;
        // A linked instance is edited as it is and keeps only what then
        // differs from its prefab (`prefabInstance.ts`).
        let before: Components = {};
        let after: Components = {};
        editThroughInstance(ent, prefabs, (view) => {
          before = structuredClone(view.components) as Components;
          mutate(view);
          after = structuredClone(view.components) as Components;
        });
        // Several selected: the same fields change on each of the others
        // that has the component (`sceneEditing.carryEdit`).
        for (const i of others) {
          const other = s.entities[i];
          if (other) editThroughInstance(other, prefabs, (view) => carryEdit(view.components as Components, before, after));
        }
        return s;
      });
    }
  }

  // ── Prefab instances ──

  /** The behaviour-tree leaves the selected entity is running, from the
   *  tree's `_running` blackboard readout (labels joined by " + "); only
   *  while playing, when the Inspector shows the live entity. */
  let runningLeaves: string[] = $derived.by(() => {
    if (!playing || !entity) return [];
    const bb = (entity.components as Record<string, unknown>).Blackboard as Record<string, unknown> | undefined;
    const r = bb?._running;
    return typeof r === "string" && r ? r.split(" + ") : [];
  });

  /** The bones under the selected skeleton, for a `skeletal_player`'s
   *  layer masks; and every clip a player can name. */
  let skeletonBones = $derived(
    $selectedEntity >= 0 ? skeletonFromScene($scene.entities as any, $selectedEntity).map((b) => b.name) : [],
  );
  let skeletalClips = $derived(
    $projectAssets.filter((f) => f.toLowerCase().endsWith(".skelanim")).map((f) => clipId(f)).sort(),
  );

  /** What the selected entity changes from the prefab - or the part - it is
   *  built from, by component: marked on each section and field. */
  let entityChanges = $derived(
    sceneEntity && !playing ? changedByComponent(sceneEntity.components as Components, !!partTemplate) : new Map<string, string[]>(),
  );

  /** The selected entities as they are (instances resolved), when there are
   *  several - to say which values differ among them. */
  let selectionViews = $derived(
    $selectedEntities.size > 1 && !playing
      ? [...$selectedEntities].map((i) => $scene.entities[i]).filter(Boolean).map((e) => viewOf(e, $scenePrefabs))
      : [],
  );
  const mixedOf = (comp: string) => (selectionViews.length > 1 ? mixedFields(selectionViews, comp) : []);

  /** What the selected instance changes from its prefab. */
  let instanceRows = $derived(
    instanceTemplate && sceneEntity ? instanceChanges(sceneEntity.components as Components) : [],
  );

  /** A part overlay's components (each one it changes, whole). */
  let partRows = $derived(partTemplate && sceneEntity ? Object.keys(sceneEntity.components as Components).sort() : []);

  /** Put a part overlay's `keys` back to the part's: drop them from the
   *  overlay - all of them drops the overlay entity too. */
  function revertPartComponents(keys: string[]) {
    const idx = $selectedEntity;
    execute(keys.length === 1 ? `Revert ${keys[0]} to the part` : "Revert to the part", (s) => {
      const ent = s.entities[idx];
      if (!ent) return s;
      const comps = { ...(ent.components as Components) };
      for (const k of keys) delete comps[k];
      ent.components = comps as EntityData["components"];
      if (Object.keys(comps).length === 0) s.entities.splice(idx, 1);
      return s;
    });
  }

  /** Put the instance's `path` back to the prefab's (one row, or every row). */
  function revertInstance(paths: string[]) {
    const template = instanceTemplate;
    if (!template) return;
    const idx = $selectedEntity;
    execute(paths.length === 1 ? `Revert ${paths[0]}` : "Revert to prefab", (s) => {
      const ent = s.entities[idx];
      if (!ent) return s;
      let comps = ent.components as Components;
      for (const path of paths) comps = revertPath(comps, template, path);
      ent.components = comps as EntityData["components"];
      return s;
    });
  }

  /** Make the instance's values at `paths` the prefab's own: write them into
   *  the `.prefab` file, then revert them here, so every instance shares them. */
  async function applyInstance(paths: string[]) {
    const proj = $project;
    const id = instanceId;
    if (!proj || !id || !sceneEntity) return;
    const entry = $prefabLibrary.entries.find((e) => $prefabLibrary.raw.get(id) === e.data);
    const raw = $prefabLibrary.raw.get(id);
    if (!entry || !raw) {
      logError(`Prefab "${id}" is not in the project`);
      return;
    }
    const rows = instanceRows.filter((r) => paths.includes(r.path));
    let next = raw;
    for (const row of rows) next = applyToPrefab(next, row.path, row.value);
    try {
      await writeTextFile(`${proj.path}/assets/${entry.file}`, JSON.stringify(next, null, 2));
      logInfo(`Applied ${rows.map((r) => r.path).join(", ")} to prefab "${id}"`);
    } catch (e) {
      logError(`Failed to write prefab "${id}": ${e}`);
      return;
    }
    revertInstance(rows.map((r) => r.path));
    notifyPrefabsChanged();
  }

  /** Break the link: the prefab's components copied in, the marker gone. */
  function unpackSelected() {
    const template = instanceTemplate;
    if (!template) return;
    const idx = $selectedEntity;
    execute("Unpack prefab", (s) => {
      const ent = s.entities[idx];
      if (!ent) return s;
      ent.components = unpackInstance(template, ent.components as Components) as EntityData["components"];
      return s;
    });
  }

  function openInstancePrefab() {
    const proj = $project;
    const id = instanceId;
    if (!proj || !id) return;
    const entry = $prefabLibrary.entries.find((e) => $prefabLibrary.raw.get(id) === e.data);
    if (entry) void openAsset(proj, `assets/${entry.file}`);
  }

  /** The joint types whose fields the Inspector draws generically - every
   * number and [x, y] pair of the variant - rather than by hand. */
  const GENERIC_JOINTS: Record<string, Record<string, number | [number, number]>> = {
    Wheel: { anchor: [0, 0], anchor_b: [0, 0], axis: [0, 1], frequency: 3, damping_ratio: 0.7, motor_speed: 0, motor_max_torque: 0 },
    Motor: { linear_offset: [0, 0], angular_offset: 0, max_force: 0, max_torque: 0, frequency: 5, damping_ratio: 1 },
    Friction: { anchor_a: [0, 0], anchor_b: [0, 0], max_force: 100, max_torque: 100 },
    Target: { anchor: [0, 0], target: [0, 0], frequency: 5, damping_ratio: 0.7, max_force: 0 },
  };

  function jointFieldLabel(key: string): string {
    const s = key.replace(/_/g, " ");
    return s.charAt(0).toUpperCase() + s.slice(1);
  }

  function setJointParam(kind: string, key: string, value: number | [number, number]) {
    editEntity(`Change joint ${key}`, (ent) => {
      const jt = ent.components.joint?.joint_type as Record<string, Record<string, unknown>> | undefined;
      if (jt && jt[kind]) jt[kind][key] = value;
    });
  }

  async function handleNameChange(event: Event) {
    const input = event.target as HTMLInputElement;
    const newName = input.value;
    editEntity(`Rename to "${newName}"`, (ent) => {
      ent.name = newName;
    });
  }

  function handleTransformChange(
    field: string,
    subfield: string | null,
    value: string,
  ) {
    const num = parseFloat(value) || 0;
    editEntity(`Change ${field}${subfield ? "." + subfield : ""}`, (ent) => {
      if (!ent.components.transform) {
        ent.components.transform = {
          position: [0, 0],
          rotation: 0,
          scale: [1, 1],
        };
      }
      const t = ent.components.transform;
      if (field === "position" && subfield === "x") t.position[0] = num;
      else if (field === "position" && subfield === "y") t.position[1] = num;
      else if (field === "rotation") t.rotation = num;
      else if (field === "scale" && subfield === "x") t.scale[0] = num;
      else if (field === "scale" && subfield === "y") t.scale[1] = num;
    });
  }

  function handleSpriteChange(field: string, value: string) {
    editEntity(`Change sprite ${field}`, (ent) => {
      if (!ent.components.sprite) {
        ent.components.sprite = {
          texture_id: "",
          color: [1, 1, 1, 1],
          z_order: 0,
        };
      }
      const sp = ent.components.sprite;
      if (field === "texture_id") {
        sp.texture_id = value;
        // Clear atlas-specific fields when selecting a standalone texture.
        // `region` too: it names a region of the OLD atlas, and the engine
        // resolves it against whatever atlas the new texture has.
        sp.uv_rect = undefined;
        sp.region = undefined;
      } else if (field === "z_order") sp.z_order = parseInt(value) || 0;
    });
  }

  function handleAtlasRegionSelect(entry: AtlasPickerEntry) {
    editEntity(`Set sprite to atlas region "${entry.name}"`, (ent) => {
      if (!ent.components.sprite) {
        ent.components.sprite = {
          texture_id: "",
          color: [1, 1, 1, 1],
          z_order: 0,
        };
      }
      const sp = ent.components.sprite;
      sp.texture_id = entry.atlas_image;
      // Name the region as well as its coordinates: the engine resolves
      // `region` against the atlas at load, so the sprite survives the sheet
      // being re-packed. The uv_rect stays as the value to draw until then.
      sp.region = entry.name;
      sp.uv_rect = [...entry.uv_rect];
      sp.size = [...entry.size];
      sp.anchor = [...entry.anchor];
    });
  }

  function handleAtlasDefImageSelect(entry: AtlasDefImageEntry) {
    // Find animations from the same atlas
    const atlasAnims = atlasDefAnimations.filter(
      (a) => a.atlas_name === entry.atlas_name,
    );

    editEntity(`Set sprite to atlas image "${entry.image_name}"`, (ent) => {
      if (!ent.components.sprite) {
        ent.components.sprite = {
          texture_id: "",
          color: [1, 1, 1, 1],
          z_order: 0,
        };
      }
      const sp = ent.components.sprite;
      sp.texture_id = entry.image_path;
      sp.uv_rect = undefined;
      sp.region = undefined;
      sp.anchor = [...entry.anchor];

      // Auto-add SpriteAnimator if atlas has animations
      if (atlasAnims.length > 0 && !ent.components.sprite_animator) {
        const clips: Record<string, any> = {};
        for (const anim of atlasAnims) {
          const frameDuration = anim.fps > 0 ? 1.0 / anim.fps : 0.1;
          clips[anim.animation_name] = {
            name: anim.animation_name,
            frames: anim.frame_paths.map((p) => ({
              uv_rect: [0, 0, 1, 1],
              texture_id: p,
            })),
            frame_duration: frameDuration,
            looping: anim.looping,
          };
        }
        ent.components.sprite_animator = {
          clips,
          current_clip: atlasAnims[0].animation_name,
          current_frame: 0,
          elapsed: 0,
          playing: false,
          speed: 1,
          random_start: false,
        };
      }
    });
    if (atlasAnims.length > 0) {
      logInfo(
        `Set sprite to "${entry.image_name}" and added ${atlasAnims.length} animation clip(s) from atlas "${entry.atlas_name}".`,
      );
    }
  }

  function handleColorChange(index: number, value: string) {
    const num = Math.max(0, Math.min(1, parseFloat(value) || 0));
    editEntity(`Change sprite color[${index}]`, (ent) => {
      if (!ent.components.sprite) return;
      ent.components.sprite.color[index] = num;
    });
  }

  function handleColorPicker(value: string) {
    const r = parseInt(value.slice(1, 3), 16) / 255;
    const g = parseInt(value.slice(3, 5), 16) / 255;
    const b = parseInt(value.slice(5, 7), 16) / 255;
    editEntity("Change sprite color", (ent) => {
      if (!ent.components.sprite) return;
      ent.components.sprite.color[0] = Math.round(r * 100) / 100;
      ent.components.sprite.color[1] = Math.round(g * 100) / 100;
      ent.components.sprite.color[2] = Math.round(b * 100) / 100;
    });
  }

  function handleSizeChange(axis: number, value: string) {
    const num = parseFloat(value);
    editEntity(`Change sprite size`, (ent) => {
      if (!ent.components.sprite) return;
      if (!ent.components.sprite.size) {
        ent.components.sprite.size = [64, 64];
      }
      ent.components.sprite.size[axis] = num || 0;
    });
  }

  function handleClearSize() {
    editEntity("Clear sprite size", (ent) => {
      if (!ent.components.sprite) return;
      ent.components.sprite.size = undefined;
    });
  }

  function handleAnchorChange(axis: number, value: string) {
    const num = Math.max(0, Math.min(1, parseFloat(value) || 0));
    editEntity(`Change sprite anchor`, (ent) => {
      if (!ent.components.sprite) return;
      if (!ent.components.sprite.anchor) {
        ent.components.sprite.anchor = [0.5, 0.5];
      }
      ent.components.sprite.anchor[axis] = num;
    });
  }

  function handleScriptChange(field: string, value: string) {
    editEntity(`Change script ${field}`, (ent) => {
      if (!ent.components.script) return;
      if (field === "source_path") ent.components.script.source_path = value;
      else if (field === "language") ent.components.script.language = value;
    });
  }

  async function handleNewScript() {
    const proj = get(project);
    if (!proj) return;

    const lang = entity?.components.script?.language ?? "Lua";
    const ext = lang === "JavaScript" ? "js" : "lua";
    const defaultName = `scripts/${(entity?.name ?? "entity").toLowerCase().replace(/\s+/g, "_")}.${ext}`;
    const scriptName = prompt(
      "Script path (relative to assets/):",
      defaultName,
    );
    if (!scriptName) return;

    const actualExt = scriptName.split(".").pop()?.toLowerCase();
    let template = "";
    if (actualExt === "lua") {
      template = `function init(entity)\n  -- Called once when play starts\nend\n\nfunction update(entity, dt)\n  -- Called every frame\nend\n`;
    } else if (actualExt === "js") {
      template = `function init(entity) {\n  // Called once when play starts\n}\n\nfunction update(entity, dt) {\n  // Called every frame\n}\n`;
    } else {
      logError("Script must be .lua or .js");
      return;
    }

    try {
      await createAssetFile(proj.path, scriptName, template);
      logInfo(`Created script: ${scriptName}`);
      requestAssetRefresh();
      const newLang = actualExt === "js" ? "JavaScript" : "Lua";
      editEntity("Set script path", (ent) => {
        if (!ent.components.script) return;
        ent.components.script.source_path = scriptName;
        ent.components.script.language = newLang;
      });
      // Refresh project file lists so pickers see the new file
      try {
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
            behavior_files: lists.behavior_files,
          };
        });
      } catch {
        /* ignore refresh errors */
      }
    } catch (e) {
      logError(`Failed to create script: ${e}`);
    }
  }

  // ─── Step 7 / Slice 7.7 — BehaviorGraph helpers ───

  /** Update one field on the BehaviorGraphComponent with undo. */
  function handleBehaviorChange(field: "graph_path", value: string) {
    editEntity(`Change behavior_graph ${field}`, (ent) => {
      if (!ent.components.behavior_graph) return;
      if (field === "graph_path")
        ent.components.behavior_graph.graph_path = value;
    });
  }

  /**
   * Replace the entire `overrides` map. Called by the textarea
   * onchange after JSON.parse succeeds. Invalid JSON is silently
   * ignored at the call site (the textarea keeps the user's draft).
   */
  function handleBehaviorOverridesReplace(next: Record<string, unknown>) {
    editEntity("Edit behavior_graph overrides", (ent) => {
      if (!ent.components.behavior_graph) return;
      ent.components.behavior_graph.overrides = next;
    });
  }

  /**
   * Create a new minimal `.behavior` asset and wire it into the
   * currently-selected entity. The template is a one-node OnTick
   * graph so the file loads cleanly through the runtime compiler.
   */
  async function handleNewBehavior() {
    const proj = get(project);
    if (!proj) return;

    const defaultName = `behaviors/${(entity?.name ?? "entity").toLowerCase().replace(/\s+/g, "_")}.behavior`;
    const path = prompt(
      "Behavior graph path (relative to assets/):",
      defaultName,
    );
    if (!path) return;

    if (!path.endsWith(".behavior")) {
      logError("Behavior graph must end in .behavior");
      return;
    }

    const template = defaultBehaviorTemplate(
      path
        .split("/")
        .pop()
        ?.replace(/\.behavior$/, "") ?? "Graph",
    );

    try {
      await createAssetFile(proj.path, path, template);
      logInfo(`Created behavior graph: ${path}`);
      editEntity("Set behavior_graph path", (ent) => {
        if (!ent.components.behavior_graph) return;
        ent.components.behavior_graph.graph_path = path;
      });
      try {
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
            behavior_files: lists.behavior_files,
          };
        });
      } catch {
        /* ignore refresh errors */
      }
    } catch (e) {
      logError(`Failed to create behavior graph: ${e}`);
    }
  }

  /** Add a component with defaults to an entity if not already present. Returns true if added. */
  function ensureComponent(ent: EntityData, type: string): boolean {
    if (hasComponent(ent, type)) return false;
    const defaults = COMPONENT_DEFAULTS[type];
    if (defaults !== undefined) {
      (ent.components as Record<string, unknown>)[type] =
        structuredClone(defaults);
      return true;
    }
    return false;
  }

  function handleAddComponent(type: string) {
    if (playing) return;
    const idx = $selectedEntity;
    const defs = $project?.component_definitions ?? [];

    // Collect dependencies that need to be auto-added
    const deps = COMPONENT_DEPS[type] ?? [];

    execute(`Add ${type} component`, (s) => {
      const ent = s.entities[idx];
      if (!ent) return s;

      // Auto-add missing dependencies first
      const added: string[] = [];
      for (const dep of deps) {
        if (ensureComponent(ent, dep)) {
          added.push(dep);
        }
      }
      if (added.length > 0) {
        logInfo(`Auto-added required: ${added.join(", ")}`);
      }

      if (type === "transform" && !ent.components.transform) {
        ent.components.transform = {
          position: [0, 0],
          rotation: 0,
          scale: [1, 1],
        };
      } else if (type in COMPONENT_DEFAULTS && !(type in ent.components)) {
        const defaultValue = structuredClone(COMPONENT_DEFAULTS[type]);
        if (type === "camera") {
          const proj = get(project);
          if (proj?.settings) {
            (defaultValue as Record<string, unknown>).viewport_width =
              proj.settings.width;
            (defaultValue as Record<string, unknown>).viewport_height =
              proj.settings.height;
          }
        }
        (ent.components as Record<string, unknown>)[type] = defaultValue;
      } else {
        // Package component: build default object from schema
        const pkgInfo = packageComponentMap.get(type);
        if (pkgInfo && pkgInfo.default != null && !(type in ent.components)) {
          // The component's own default, whole.
          (ent.components as Record<string, unknown>)[type] =
            structuredClone(pkgInfo.default);
        } else if (pkgInfo && !(type in ent.components)) {
          const obj: Record<string, unknown> = {};
          for (const f of pkgInfo.fields) {
            if (f.default_value != null) {
              obj[f.name] = structuredClone(f.default_value);
            } else {
              // Sensible defaults by field type
              const ftk =
                typeof f.field_type === "string"
                  ? f.field_type
                  : Object.keys(f.field_type)[0];
              if (ftk === "Float") obj[f.name] = 0;
              else if (ftk === "Int") obj[f.name] = 0;
              else if (ftk === "String") obj[f.name] = "";
              else if (ftk === "Bool") obj[f.name] = false;
              else if (ftk === "Vec2") obj[f.name] = [0, 0];
              else if (ftk === "Color") obj[f.name] = [1, 1, 1, 1];
              else if (ftk === "Enum") {
                const opts = (f.field_type as { Enum: { options: string[] } })
                  .Enum.options;
                obj[f.name] = opts[0] ?? "";
              } else if (ftk === "AssetRef") obj[f.name] = "";
              else obj[f.name] = null;
            }
          }
          (ent.components as Record<string, unknown>)[type] = obj;
        } else {
          // Custom component: build default object from definition
          const def = defs.find((d) => d.name === type);
          if (def && !(type in ent.components)) {
            const obj: Record<string, unknown> = {};
            for (const f of def.fields) {
              obj[f.name] = structuredClone(f.default_value);
            }
            (ent.components as Record<string, unknown>)[type] = obj;
          }
        }
      }
      return s;
    });
  }

  function handleRemoveComponent(type: string) {
    if (playing) return;
    // Block removal if other components depend on this one
    const dependents = removeDeps[type];
    if (dependents && dependents.length > 0) {
      logError(`Cannot remove ${type}: required by ${dependents.join(", ")}`);
      return;
    }
    if (instanceTemplate) {
      // The prefab's component is hidden with a `null` override, the
      // entity's own copy dropped (`diffEdit`).
      editEntity(`Remove ${type} component`, (ent) => {
        delete (ent.components as Record<string, unknown>)[type];
      });
      return;
    }
    const idx = $selectedEntity;
    execute(`Remove ${type} component`, (s) => {
      const ent = s.entities[idx];
      if (!ent) return s;
      if (type === "transform") ent.components.transform = undefined;
      else if (type === "sprite") ent.components.sprite = undefined;
      else if (type === "script") ent.components.script = undefined;
      else if (type === "text") ent.components.text = undefined;
      else if (type === "camera_follow")
        ent.components.camera_follow = undefined;
      else if (type === "camera") ent.components.camera = undefined;
      else if (type === "rigid_body") ent.components.rigid_body = undefined;
      else if (type === "collider") ent.components.collider = undefined;
      else if (type === "joint") ent.components.joint = undefined;
      else if (type === "velocity") ent.components.velocity = undefined;
      else if (type === "bounce_area") ent.components.bounce_area = undefined;
      else if (type === "audio_source") ent.components.audio_source = undefined;
      else if (type === "sprite_animator")
        ent.components.sprite_animator = undefined;
      else if (type === "anim_state_machine")
        ent.components.anim_state_machine = undefined;
      else if (type === "tilemap") ent.components.tilemap = undefined;
      else if (type === "particle_emitter")
        ent.components.particle_emitter = undefined;
      else if (type === "ui_element") ent.components.ui_element = undefined;
      else if (type === "ui_text") ent.components.ui_text = undefined;
      else if (type === "ui_image") ent.components.ui_image = undefined;
      else if (type === "ui_panel") ent.components.ui_panel = undefined;
      else if (type === "ui_button") ent.components.ui_button = undefined;
      else if (type === "ui_progress_bar")
        ent.components.ui_progress_bar = undefined;
      else if (type === "ui_slider") ent.components.ui_slider = undefined;
      else if (type === "ui_toggle") ent.components.ui_toggle = undefined;
      else if (type === "ui_text_input")
        ent.components.ui_text_input = undefined;
      else if (type === "gui") ent.components.gui = undefined;
      else if (type === "point_light") ent.components.point_light = undefined;
      else if (type === "spot_light") ent.components.spot_light = undefined;
      else if (type === "ambient_light")
        ent.components.ambient_light = undefined;
      else if (type === "lightmap") ent.components.lightmap = undefined;
      else if (type === "shadow_caster")
        ent.components.shadow_caster = undefined;
      else if (type === "vector_path") ent.components.vector_path = undefined;
      else delete (ent.components as Record<string, unknown>)[type];
      return s;
    });
  }

  function handleCustomFieldChange(
    compName: string,
    fieldName: string,
    value: unknown,
  ) {
    editEntity(`Change ${compName}.${fieldName}`, (ent) => {
      const comp = (ent.components as Record<string, Record<string, unknown>>)[
        compName
      ];
      if (comp) comp[fieldName] = value;
    });
  }

  // --- Tags ---
  let newTagInput = $state("");

  // Helper to get the collider shape type
  function getShapeType(shape: Record<string, unknown>): string {
    if ("Box" in shape) return "Box";
    if ("Circle" in shape) return "Circle";
    if ("Capsule" in shape) return "Capsule";
    if ("Segment" in shape) return "Segment";
    if ("Triangle" in shape) return "Triangle";
    if ("ConvexPolygon" in shape) return "ConvexPolygon";
    if ("Polyline" in shape) return "Polyline";
    if ("HeightField" in shape) return "HeightField";
    if ("ConvexDecomposition" in shape) return "ConvexDecomposition";
    return "Box";
  }

  function handleAddTag() {
    const tag = newTagInput.trim();
    if (!tag) return;
    editEntity(`Add tag "${tag}"`, (ent) => {
      if (!ent.components.tags) ent.components.tags = [];
      if (!ent.components.tags.includes(tag)) {
        ent.components.tags.push(tag);
      }
    });
    newTagInput = "";
  }

  function handleRemoveTag(tag: string) {
    editEntity(`Remove tag "${tag}"`, (ent) => {
      if (!ent.components.tags) return;
      ent.components.tags = ent.components.tags.filter(
        (t: string) => t !== tag,
      );
      if (ent.components.tags.length === 0) ent.components.tags = undefined;
    });
  }

  function colorToHex(color: [number, number, number, number]): string {
    const r = Math.round(color[0] * 255)
      .toString(16)
      .padStart(2, "0");
    const g = Math.round(color[1] * 255)
      .toString(16)
      .padStart(2, "0");
    const b = Math.round(color[2] * 255)
      .toString(16)
      .padStart(2, "0");
    return `#${r}${g}${b}`;
  }

  /** Which component types can still be added to the selected entity. */
  let addableComponents: string[] = $derived.by(() => {
    if (!entity) return [];
    const comps = entity.components as Record<string, unknown>;
    const result: string[] = [];
    // Builtin components not yet on the entity
    for (const name of KNOWN_COMPONENTS) {
      if (!(name in comps) || comps[name] == null) result.push(name);
    }
    // Custom component definitions from project
    const defs = $project?.component_definitions ?? [];
    for (const def of defs) {
      if (!(def.name in comps)) result.push(def.name);
    }
    // Package components (only those not already builtin)
    for (const name of packageComponentMap.keys()) {
      if (!(name in comps) && !result.includes(name)) {
        result.push(name);
      }
    }
    return result;
  });

  let expandedCategories: Record<string, boolean> = $state({});

  let categorizedComponents = $derived.by(() => {
    const base = categorizeComponents(addableComponents);
    // Package components grouped by category
    const addable = new Set(addableComponents);
    const pkgCats = new Map<string, string[]>();
    for (const [name, info] of packageComponentMap) {
      if (!addable.has(name)) continue;
      const cat = info.category || "Packages";
      if (!pkgCats.has(cat)) pkgCats.set(cat, []);
      pkgCats.get(cat)!.push(name);
    }
    for (const [cat, items] of pkgCats) {
      base.push({
        key: `pkg_${cat.toLowerCase()}`,
        label: cat,
        icon: "📦",
        items,
      });
    }
    return base;
  });

  /** Custom components currently attached to the entity. */
  let customComps: {
    name: string;
    data: Record<string, unknown>;
    def: ComponentDef | undefined;
  }[] = $derived.by(() => {
    if (!entity) return [];
    const defs = $project?.component_definitions ?? [];
    const result: {
      name: string;
      data: Record<string, unknown>;
      def: ComponentDef | undefined;
    }[] = [];
    for (const [key, val] of Object.entries(entity.components)) {
      if (BUILTIN_KEYS.has(key) || val == null || typeof val !== "object")
        continue;
      // Skip components that have package schema — they render via PackageInspector
      if (packageComponentMap.has(key)) continue;
      result.push({
        name: key,
        data: val as Record<string, unknown>,
        def: defs.find((d) => d.name === key),
      });
    }
    return result;
  });

  /** Package components currently attached to the entity (rendered via schema). */
  let packageComps: {
    name: string;
    data: Record<string, unknown>;
    info: ComponentEditorInfo;
  }[] = $derived.by(() => {
    if (!entity) return [];
    const result: {
      name: string;
      data: Record<string, unknown>;
      info: ComponentEditorInfo;
    }[] = [];
    for (const [key, val] of Object.entries(entity.components)) {
      if (BUILTIN_KEYS.has(key) || val == null || typeof val !== "object")
        continue;
      const info = packageComponentMap.get(key);
      if (!info) continue;
      result.push({
        name: key,
        data: val as Record<string, unknown>,
        info,
      });
    }
    return result;
  });

  let showAddMenu = $state(false);
  let componentSearch = $state("");

  let filteredCategories = $derived.by(() => {
    const q = componentSearch.trim().toLowerCase();
    if (!q) return categorizedComponents;
    return categorizedComponents
      .map((cat) => ({
        ...cat,
        items: cat.items.filter((c) => c.toLowerCase().includes(q)),
      }))
      .filter((cat) => cat.key === "custom" || cat.items.length > 0);
  });
  let spritePickerOpen = $state(false);
  let scriptPickerOpen = $state(false);
  let fontPickerOpen = $state(false);
  let guiPickerOpen = $state(false);
  let animPickerOpen = $state(false);
  // Step 7 / Slice 7.7 — picker state for the BehaviorGraph component.
  let behaviorPickerOpen = $state(false);

  let spriteFiles: string[] = $derived($project?.sprite_files ?? []);
  let scriptFiles: string[] = $derived($project?.script_files ?? []);
  let guiFiles: string[] = $derived($project?.gui_files ?? []);
  let animFiles: string[] = $derived($project?.anim_files ?? []);
  let behaviorFiles: string[] = $derived($project?.behavior_files ?? []);

  // Atlas region entries for the sprite picker
  let atlasEntries = $state<AtlasPickerEntry[]>([]);
  $effect(() => {
    const path = $project?.path;
    if (path) {
      listAtlasEntries(path)
        .then((e) => (atlasEntries = e))
        .catch(() => (atlasEntries = []));
    } else {
      atlasEntries = [];
    }
  });

  // Project-authored .material files (relative ids without extension, e.g.
  // "materials/pulse_emission"). Merged into the Material dropdown so
  // user-created materials show up alongside the built-ins.
  let projectMaterialIds: string[] = $state([]);
  $effect(() => {
    const path = $project?.path;
    if (path) {
      listAssets(path)
        .then((files) => {
          projectMaterialIds = files
            .filter((f) => f.endsWith(".material"))
            .map((f) => f.replace(/\.material$/, ""))
            .sort();
        })
        .catch(() => (projectMaterialIds = []));
    } else {
      projectMaterialIds = [];
    }
  });

  // ── Sort Layers 1.4 — project `.sortlayers` assets + active set ──
  // Discover all `.sortlayers` files in the project so the scene-level
  // picker and per-component dropdowns can offer them. Ids retain the
  // extension so they match what's written to `SceneData.sort_layers`.
  let projectSortLayerIds: string[] = $state([]);
  $effect(() => {
    const path = $project?.path;
    if (path) {
      listAssets(path)
        .then((files) => {
          projectSortLayerIds = files
            .filter((f) => f.endsWith(".sortlayers"))
            .sort();
        })
        .catch(() => (projectSortLayerIds = []));
    } else {
      projectSortLayerIds = [];
    }
  });
  // Parsed contents of the currently active `.sortlayers` asset so the
  // per-component dropdowns can list layer names. Refetched whenever
  // the scene's `sort_layers` field changes.
  let activeSortLayerNames: string[] = $state([]);
  $effect(() => {
    const path = $project?.path;
    const id = $scene?.sort_layers;
    if (!path || !id) {
      activeSortLayerNames = [];
      return;
    }
    readAssetBinary(path, id)
      .then((bytes) => {
        try {
          const json = new TextDecoder().decode(new Uint8Array(bytes));
          const parsed = JSON.parse(json) as {
            layers?: { name: string }[];
          };
          activeSortLayerNames = (parsed.layers ?? [])
            .map((l) => l.name)
            .filter((n) => typeof n === "string" && n.length > 0);
        } catch {
          activeSortLayerNames = [];
        }
      })
      .catch(() => (activeSortLayerNames = []));
  });

  // Atlas definition images (from .atlas files) for the sprite picker
  let atlasDefImages = $state<AtlasDefImageEntry[]>([]);
  $effect(() => {
    const path = $project?.path;
    if (path) {
      listAtlasDefImages(path)
        .then((e) => (atlasDefImages = e))
        .catch(() => (atlasDefImages = []));
    } else {
      atlasDefImages = [];
    }
  });

  // Atlas definition animations (from .atlas files) for the animator importer
  let atlasDefAnimations = $state<AtlasDefAnimationEntry[]>([]);
  $effect(() => {
    const path = $project?.path;
    if (path) {
      listAtlasDefAnimations(path)
        .then((e) => (atlasDefAnimations = e))
        .catch(() => (atlasDefAnimations = []));
    } else {
      atlasDefAnimations = [];
    }
  });

  // Group atlas def images by atlas name for the picker
  let atlasDefImagesByAtlas = $derived.by(() => {
    const groups = new Map<string, AtlasDefImageEntry[]>();
    for (const img of atlasDefImages) {
      if (!groups.has(img.atlas_name)) {
        groups.set(img.atlas_name, []);
      }
      groups.get(img.atlas_name)!.push(img);
    }
    return groups;
  });

  // Close pickers when entity selection changes
  $effect(() => {
    void $selectedEntity;
    spritePickerOpen = false;
    scriptPickerOpen = false;
    fontPickerOpen = false;
    importAtlasOpen = false;
    importAtlasDefOpen = false;
  });

  // --- Tilemap file picker state ---
  let tilemapFiles: string[] = $state([]);

  // --- Generate from Grid state ---
  let gridImportOpen = $state(false);
  let gridImportImages: string[] = $state([]);
  let gridImportTexture = $state("");
  let gridImportCols = $state(4);
  let gridImportRows = $state(4);
  let gridImportFrameDuration = $state(0.15);
  let gridImportLooping = $state(true);
  let gridImportIdleClips = $state(true);
  /** One clip name per row. Empty string = skip that row. */
  let gridImportRowNames: string[] = $state([]);

  async function openGridImport() {
    const projPath = $project?.path;
    if (!projPath) return;
    try {
      gridImportImages = await listProjectImages(projPath);
      gridImportTexture = gridImportImages[0] ?? "";
      gridImportCols = 4;
      gridImportRows = 4;
      gridImportFrameDuration = 0.15;
      gridImportLooping = true;
      gridImportIdleClips = true;
      gridImportRowNames = Array.from({ length: 4 }, (_, i) => `row_${i}`);
      gridImportOpen = true;
    } catch (e) {
      logError(`Failed to list images: ${e}`);
    }
  }

  function gridImportUpdateRowNames() {
    const prev = gridImportRowNames;
    gridImportRowNames = Array.from({ length: gridImportRows }, (_, i) =>
      i < prev.length ? prev[i] : `row_${i}`,
    );
  }

  function gridImportApply() {
    if (!entity) return;
    const cols = gridImportCols;
    const rows = gridImportRows;
    if (cols < 1 || rows < 1) return;

    const fw = 1 / cols;
    const fh = 1 / rows;

    editEntity("Generate clips from grid", (ent) => {
      if (!ent.components.sprite_animator) {
        ent.components.sprite_animator = {
          clips: {},
          current_clip: "",
          current_frame: 0,
          elapsed: 0,
          playing: true,
          speed: 1.0,
        };
      }
      const sa = ent.components.sprite_animator;

      for (let row = 0; row < rows; row++) {
        const name = gridImportRowNames[row]?.trim();
        if (!name) continue; // skip unnamed rows

        // Walk clip — all frames in the row
        const frames = [];
        for (let col = 0; col < cols; col++) {
          frames.push({
            uv_rect: [col * fw, row * fh, (col + 1) * fw, (row + 1) * fh] as [
              number,
              number,
              number,
              number,
            ],
          });
        }
        sa.clips[name] = {
          name,
          frames,
          frame_duration: gridImportFrameDuration,
          looping: gridImportLooping,
        };

        // Idle clip — first frame only (e.g. "walk_down" → "idle_down")
        if (gridImportIdleClips) {
          const idleName = name.replace(/^walk_/, "idle_");
          if (idleName !== name) {
            sa.clips[idleName] = {
              name: idleName,
              frames: [
                {
                  uv_rect: [0, row * fh, fw, (row + 1) * fh] as [
                    number,
                    number,
                    number,
                    number,
                  ],
                },
              ],
              frame_duration: 1.0,
              looping: true,
            };
          }
        }
      }

      // Set current clip to the first non-empty row
      const firstName = gridImportRowNames.find((n) => n?.trim());
      if (firstName && !sa.current_clip) {
        sa.current_clip = firstName.trim();
      }

      // Also set the sprite texture if not already set
      if (ent.components.sprite && gridImportTexture) {
        ent.components.sprite.texture_id = gridImportTexture;
      }
    });

    const namedRows = gridImportRowNames.filter((n) => n?.trim()).length;
    const idleCount = gridImportIdleClips
      ? gridImportRowNames.filter((n) => n?.trim().startsWith("walk_")).length
      : 0;
    logInfo(
      `Generated ${namedRows + idleCount} clips (${namedRows} walk + ${idleCount} idle) from ${gridImportCols}×${gridImportRows} grid.`,
    );
    gridImportOpen = false;
  }

  // --- Load .anim file into entity ---
  async function loadAnimFileToEntity(animPath: string) {
    const proj = get(project);
    if (!proj) return;
    try {
      const data = await readAnimFile(proj.path, animPath);
      // The clip type lives in `api/animation.ts`; this redeclared it with a
      // required `uv_rect`, which a frame naming a region does not have.
      const clips: Record<string, AnimationClipData> = {};
      for (const [name, clip] of Object.entries(data.clips)) {
        clips[name] = {
          name: clip.name,
          frames: clip.frames.map((f) => ({
            uv_rect: f.uv_rect
              ? ([...f.uv_rect] as [number, number, number, number])
              : undefined,
            region: f.region,
            texture_id: f.texture_id ?? undefined,
            triggers: f.triggers ?? [],
          })),
          frame_duration: clip.frame_duration,
          looping: clip.looping,
        };
      }
      const firstClip = Object.keys(clips)[0] ?? undefined;
      editEntity(`Load animation "${animPath}"`, (ent) => {
        if (!ent.components.sprite_animator) return;
        ent.components.sprite_animator.clips = clips;
        ent.components.sprite_animator.current_clip = firstClip;
        ent.components.sprite_animator.current_frame = 0;
        ent.components.sprite_animator.elapsed = 0;
        if (data.texture_id && ent.components.sprite) {
          ent.components.sprite.texture_id = data.texture_id;
        }
      });
      logInfo(`Loaded ${Object.keys(clips).length} clip(s) from ${animPath}`);
      animPickerOpen = false;
    } catch (e) {
      logError(`Failed to load .anim file: ${e}`);
    }
  }

  // --- Import from Spritesheet state ---
  let importAtlasOpen = $state(false);
  let importAtlasFiles: string[] = $state([]);
  let importAtlasSelected = $state("");
  let importAtlasRegions: AtlasRegion[] = $state([]);
  let importAtlasWidth = $state(0);
  let importAtlasHeight = $state(0);
  let importRegionChecked: boolean[] = $state([]);
  let importClipName = $state("default");
  let importFrameDuration = $state(0.1);
  let importLooping = $state(true);
  let importLoading = $state(false);

  async function openAtlasImport() {
    const projPath = $project?.path;
    if (!projPath) return;
    try {
      const allAssets = await listAssets(projPath);
      importAtlasFiles = allAssets.filter((f: string) => f.endsWith(".atlas"));
      if (importAtlasFiles.length === 0) {
        logError(
          "No atlas files found. Pack a spritesheet first via Build → Spritesheet Editor.",
        );
        return;
      }
      importAtlasSelected = importAtlasFiles[0];
      importAtlasRegions = [];
      importRegionChecked = [];
      importClipName = "default";
      importFrameDuration = 0.1;
      importLooping = true;
      importAtlasOpen = true;
      await loadAtlasPreview();
    } catch (e) {
      logError(`Failed to list atlas files: ${e}`);
    }
  }

  async function loadAtlasPreview() {
    const projPath = $project?.path;
    if (!projPath || !importAtlasSelected) return;
    importLoading = true;
    try {
      const preview = await previewAtlas(
        projPath,
        `assets/${importAtlasSelected}`,
      );
      importAtlasRegions = preview.atlas_data.regions;
      importAtlasWidth = preview.atlas_data.width;
      importAtlasHeight = preview.atlas_data.height;
      importRegionChecked = importAtlasRegions.map(() => true);
    } catch (e) {
      logError(`Failed to load atlas: ${e}`);
      importAtlasRegions = [];
    } finally {
      importLoading = false;
    }
  }

  function importAtlasAsClip() {
    const selectedRegions = importAtlasRegions.filter(
      (_, i) => importRegionChecked[i],
    );
    if (selectedRegions.length === 0) {
      logError("Select at least one region to import.");
      return;
    }
    const w = importAtlasWidth;
    const h = importAtlasHeight;
    if (w === 0 || h === 0) return;

    // [u_min, v_min, u_max, v_max] - the engine's uv_rect everywhere. This
    // built [x, y, width, height], so every imported frame whose region was
    // not at the sheet's origin drew a mirrored or empty slice.
    const frames = selectedRegions.map((r) => ({
      uv_rect: [r.x / w, r.y / h, (r.x + r.width) / w, (r.y + r.height) / h] as [
        number,
        number,
        number,
        number,
      ],
    }));

    const clipName = importClipName.trim() || "default";
    editEntity("Import clip from spritesheet", (ent) => {
      if (!ent.components.sprite_animator) return;
      if (!ent.components.sprite_animator.clips) {
        ent.components.sprite_animator.clips = {};
      }
      ent.components.sprite_animator.clips[clipName] = {
        name: clipName,
        frames,
        frame_duration: importFrameDuration,
        looping: importLooping,
      };
      // Auto-select if it's the first clip
      if (Object.keys(ent.components.sprite_animator.clips).length === 1) {
        ent.components.sprite_animator.current_clip = clipName;
      }
    });
    logInfo(
      `Imported clip "${clipName}" with ${frames.length} frame(s) from atlas.`,
    );
    importAtlasOpen = false;
  }

  // --- Import from Atlas Definition state ---
  let importAtlasDefOpen = $state(false);
  let importAtlasDefTab = $state<"animations" | "images">("animations");
  let importAtlasDefSelectedImages = $state<AtlasDefImageEntry[]>([]);
  let importAtlasDefClipName = $state("default");
  let importAtlasDefFps = $state(10);
  let importAtlasDefLooping = $state(true);

  function openAtlasDefImport() {
    if (atlasDefAnimations.length === 0 && atlasDefImages.length === 0) {
      logError(
        "No atlas content found. Add images or animations in an Atlas editor first.",
      );
      return;
    }
    importAtlasDefTab = atlasDefAnimations.length > 0 ? "animations" : "images";
    importAtlasDefSelectedImages = [];
    importAtlasDefClipName = "default";
    importAtlasDefFps = 10;
    importAtlasDefLooping = true;
    importAtlasDefOpen = true;
  }

  function toggleAtlasDefImage(img: AtlasDefImageEntry) {
    const idx = importAtlasDefSelectedImages.findIndex(
      (s) => s.image_path === img.image_path && s.atlas_name === img.atlas_name,
    );
    if (idx >= 0) {
      importAtlasDefSelectedImages = importAtlasDefSelectedImages.filter(
        (_, i) => i !== idx,
      );
    } else {
      importAtlasDefSelectedImages = [...importAtlasDefSelectedImages, img];
    }
  }

  function importAtlasDefFromImages() {
    if (importAtlasDefSelectedImages.length === 0) return;
    const clipName = importAtlasDefClipName.trim() || "default";
    const frameDuration = importAtlasDefFps > 0 ? 1.0 / importAtlasDefFps : 0.1;
    const frames = importAtlasDefSelectedImages.map((img) => ({
      uv_rect: [0, 0, 1, 1] as [number, number, number, number],
      texture_id: img.image_path,
    }));
    const framePaths = importAtlasDefSelectedImages.map(
      (img) => img.image_path,
    );

    editEntity(`Import clip "${clipName}" from atlas images`, (ent) => {
      if (!ent.components.sprite_animator) return;
      if (!ent.components.sprite_animator.clips) {
        ent.components.sprite_animator.clips = {};
      }
      ent.components.sprite_animator.clips[clipName] = {
        name: clipName,
        frames,
        frame_duration: frameDuration,
        looping: importAtlasDefLooping,
      };
      if (Object.keys(ent.components.sprite_animator.clips).length === 1) {
        ent.components.sprite_animator.current_clip = clipName;
      }
      // Ensure sprite component exists and set first frame texture
      if (framePaths.length > 0) {
        if (!ent.components.sprite) {
          ent.components.sprite = {
            texture_id: framePaths[0],
            color: [1, 1, 1, 1],
            z_order: 0,
            size: null,
            anchor: [0.5, 0.5],
          };
        } else {
          ent.components.sprite.texture_id = framePaths[0];
          ent.components.sprite.uv_rect = undefined;
        }
      }
    });
    logInfo(
      `Imported clip "${clipName}" (${importAtlasDefSelectedImages.length} frames, ${importAtlasDefFps} FPS) from atlas images.`,
    );
    importAtlasDefOpen = false;
  }

  function importAtlasDefAnimation(anim: AtlasDefAnimationEntry) {
    const frameDuration = anim.fps > 0 ? 1.0 / anim.fps : 0.1;
    // Each frame is a full individual image with its own texture_id
    const frames = anim.frame_paths.map((p) => ({
      uv_rect: [0, 0, 1, 1] as [number, number, number, number],
      texture_id: p,
    }));

    editEntity(
      `Import animation "${anim.animation_name}" from atlas`,
      (ent) => {
        if (!ent.components.sprite_animator) return;
        if (!ent.components.sprite_animator.clips) {
          ent.components.sprite_animator.clips = {};
        }
        ent.components.sprite_animator.clips[anim.animation_name] = {
          name: anim.animation_name,
          frames,
          frame_duration: frameDuration,
          looping: anim.looping,
        };
        // Auto-select if it's the first clip
        if (Object.keys(ent.components.sprite_animator.clips).length === 1) {
          ent.components.sprite_animator.current_clip = anim.animation_name;
        }
        // Ensure sprite component exists and set first frame texture
        if (anim.frame_paths.length > 0) {
          if (!ent.components.sprite) {
            ent.components.sprite = {
              texture_id: anim.frame_paths[0],
              color: [1, 1, 1, 1],
              z_order: 0,
              size: null,
              anchor: [0.5, 0.5],
            };
          } else {
            ent.components.sprite.texture_id = anim.frame_paths[0];
            ent.components.sprite.uv_rect = undefined;
          }
        }
      },
    );
    logInfo(
      `Imported clip "${anim.animation_name}" (${anim.frame_count} frames, ${anim.fps} FPS) from atlas "${anim.atlas_name}".`,
    );
    importAtlasDefOpen = false;
  }

  // Collapsible section state — Maps section key to collapsed boolean
  let collapsed: Record<string, boolean> = $state({});
  function toggleSection(key: string) {
    collapsed[key] = !collapsed[key];
  }

  // The bar beside a section's title says what kind of thing it is, in the
  // palette's category hues (theme.css): placement, drawing, physics,
  // animation, sound, UI, logic. Anything else gets a neutral bar.
  function sectionHue(key: string): string {
    if (key === "transform") return "var(--ed-cat-1)";
    if (/rigid|collider|joint|velocity|accel|speed|bounce|character|area|physics/.test(key)) return "var(--ed-cat-3)";
    if (/anim|tween|oscillat|patrol|follow|seek|timeline|skelet/.test(key)) return "var(--ed-cat-6)";
    if (/audio|sound|music/.test(key)) return "var(--ed-cat-5)";
    if (/^ui_|^gui/.test(key)) return "var(--ed-cat-4)";
    if (/script|behavior|state_machine|tags/.test(key)) return "var(--ed-cat-8)";
    if (/sprite|material|light|text|tilemap|particle|camera|mesh|trail|line/.test(key)) return "var(--ed-cat-2)";
    return "var(--ed-line-strong)";
  }

  // ── Post-Processing helpers ──
  // The parameter table, and every rule about editing it, live in
  // `postProcess.ts` - see its header for why.

  let ppEffects: PostProcessEffectSettings[] = $derived(pp.effectsFor($scene.post_process));

  function ppUpdateScene(effects: PostProcessEffectSettings[]) {
    execute("Update post-processing", (s) => {
      s.post_process = pp.withEffects(s.post_process, effects);
      return s;
    });
  }

  function ppToggleEffect(index: number, enabled: boolean) {
    ppUpdateScene(ppEffects.map((e, i) => (i === index ? { ...e, enabled } : e)));
  }

  function ppSetParam(index: number, key: string, value: number) {
    ppUpdateScene(pp.withParam(ppEffects, index, key, { Float: value }));
  }

  function ppSetColor(index: number, key: string, value: number[]) {
    ppUpdateScene(pp.withParam(ppEffects, index, key, { Vec4: value }));
  }

  function ppSetHdr(change: { hdr: boolean; tonemap?: pp.TonemapName; exposure?: number }) {
    execute("Update post-processing", (s) => {
      s.post_process = pp.withHdr(s.post_process, change);
      return s;
    });
  }

  // Raw 0..1 <-> #rrggbb, for the shader-node colour pickers. (The post
  // panel's pickers convert linear <-> sRGB instead - `postProcess.ts`.)
  function ppVec4ToHex(v: number[]): string {
    const r = Math.round(v[0] * 255)
      .toString(16)
      .padStart(2, "0");
    const g = Math.round(v[1] * 255)
      .toString(16)
      .padStart(2, "0");
    const b = Math.round(v[2] * 255)
      .toString(16)
      .padStart(2, "0");
    return `#${r}${g}${b}`;
  }

  function ppHexToVec4(hex: string): number[] {
    const r = parseInt(hex.slice(1, 3), 16) / 255;
    const g = parseInt(hex.slice(3, 5), 16) / 255;
    const b = parseInt(hex.slice(5, 7), 16) / 255;
    return [r, g, b, 1.0];
  }

  // ─── Shader Node Helpers ─────────────────────────────────────────

  let shaderNode: ShaderNode | undefined = $derived(
    $shaderEditorGraph && $shaderSelectedNodeId !== null
      ? $shaderEditorGraph.nodes.find((n) => n.id === $shaderSelectedNodeId)
      : undefined,
  );

  function shaderNodeTypeLabel(nt: NodeType): string {
    switch (nt.type) {
      case "FloatConstant":
        return `Float: ${nt.value}`;
      case "Vec2Constant":
        return "Vec2";
      case "Vec3Constant":
        return "Vec3";
      case "Vec4Constant":
        return "Vec4";
      case "Combine":
        return `Combine (${nt.components})`;
      case "CustomGlsl":
        return "Custom GLSL";
      default:
        return nt.type.replace(/([A-Z])/g, " $1").trim();
    }
  }

  function shaderNodeEdit() {
    shaderEditorGraph.update((g) => g);
    const draw = get(shaderRequestDraw);
    const markDirty = get(shaderMarkDirty);
    if (draw) draw();
    if (markDirty) markDirty();
  }

  // ─── Shader Preview ──────────────────────────────────────────────
  //
  // Live preview pipeline (Slice 13b + 13c hardening):
  //   graph → shaderGeneratePreviewGlsl (Tauri) → WGSL → naga (GLSL ES 300)
  //   → compile + link → introspect uniforms → animate.
  //
  // We compile the same WGSL the runtime uses, just transpiled to GLSL
  // for WebGL2. Uniform/sampler bindings come from naga's auto-generated
  // names so we discover them at runtime instead of hard-coding.

  let previewCanvas: HTMLCanvasElement | null = $state(null);
  let previewGl: WebGL2RenderingContext | null = null;
  let previewProgram: WebGLProgram | null = null;
  let previewVao: WebGLVertexArrayObject | null = null;
  let previewError: string | null = $state(null);
  let previewCompiling = $state(false);
  let previewStartTime = 0;
  let previewAnimId = 0;

  /** Dummy 1×1 white texture bound to every sampler2D the shader uses. */
  let previewDummyTex: WebGLTexture | null = null;
  /** UBO + size for the (single) Globals uniform block, if any. */
  let previewUbo: WebGLBuffer | null = null;
  let previewUboSize = 0;
  /** Offsets discovered from the linked program. -1 if missing. */
  let previewTimeOffset = -1;
  let previewVpOffset = -1;

  function initPreviewGl() {
    if (!previewCanvas || previewGl) return;
    const gl = previewCanvas.getContext("webgl2", {
      alpha: false,
      antialias: true,
    });
    if (!gl) return;
    previewGl = gl;

    // Fullscreen quad with UVs in [0,1] and white vertex color.
    // Layout: (vec2 pos, vec2 uv, vec4 color) — matches sprite.wgsl VsIn.
    // For preview we draw at clip space [-1,1]² so vs_main's
    // view_projection = identity works out of the box.
    const verts = new Float32Array([
      // x, y,     u, v,    r, g, b, a
      -1, -1, 0, 0, 1, 1, 1, 1, 1, -1, 1, 0, 1, 1, 1, 1, -1, 1, 0, 1, 1, 1, 1,
      1, 1, 1, 1, 1, 1, 1, 1, 1,
    ]);
    const vao = gl.createVertexArray();
    const buf = gl.createBuffer();
    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, verts, gl.STATIC_DRAW);
    const stride = 8 * 4;
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, stride, 0);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 2, gl.FLOAT, false, stride, 2 * 4);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribPointer(2, 4, gl.FLOAT, false, stride, 4 * 4);
    gl.bindVertexArray(null);
    previewVao = vao;
    previewStartTime = performance.now();

    // 1×1 white RGBA texture stand-in for any sampler the shader uses.
    const tex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texImage2D(
      gl.TEXTURE_2D,
      0,
      gl.RGBA,
      1,
      1,
      0,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      new Uint8Array([255, 255, 255, 255]),
    );
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.bindTexture(gl.TEXTURE_2D, null);
    previewDummyTex = tex;
  }

  function compilePreviewShader(fragSrc: string) {
    const gl = previewGl;
    if (!gl || !previewVao) return;

    // Clean up old program
    if (previewProgram) {
      gl.deleteProgram(previewProgram);
      previewProgram = null;
    }

    // Vertex shader feeds the *exact* varyings naga emits when it
    // transpiles `vs_main` (sprite.wgsl): location 0 = uv, location 1 = color.
    // We don't actually share the WGSL vertex stage because the runtime's
    // VsIn requires a buffer layout we don't want to construct in the editor;
    // instead we mimic the same VsOut interface block so the GLSL fragment
    // compiles unchanged.
    const vertSrc = `#version 300 es
precision mediump float;
layout(location=0) in vec2 a_pos;
layout(location=1) in vec2 a_uv;
layout(location=2) in vec4 a_color;
out vec2 _vs2fs_location0;
out vec4 _vs2fs_location1;
void main() {
  _vs2fs_location0 = a_uv;
  _vs2fs_location1 = a_color;
  gl_Position = vec4(a_pos, 0.0, 1.0);
}`;

    const vs = gl.createShader(gl.VERTEX_SHADER)!;
    gl.shaderSource(vs, vertSrc);
    gl.compileShader(vs);
    if (!gl.getShaderParameter(vs, gl.COMPILE_STATUS)) {
      previewError = "Vertex: " + gl.getShaderInfoLog(vs);
      gl.deleteShader(vs);
      return;
    }

    const fs = gl.createShader(gl.FRAGMENT_SHADER)!;
    gl.shaderSource(fs, fragSrc);
    gl.compileShader(fs);
    if (!gl.getShaderParameter(fs, gl.COMPILE_STATUS)) {
      previewError = gl.getShaderInfoLog(fs) || "Fragment compile failed";
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      return;
    }

    const prog = gl.createProgram()!;
    gl.attachShader(prog, vs);
    gl.attachShader(prog, fs);
    gl.linkProgram(prog);
    gl.deleteShader(vs);
    gl.deleteShader(fs);

    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
      previewError = gl.getProgramInfoLog(prog) || "Link failed";
      gl.deleteProgram(prog);
      return;
    }

    previewProgram = prog;
    previewError = null;

    // ── Discover Globals UBO layout (naga emits a uniform block) ──
    previewUbo = null;
    previewUboSize = 0;
    previewTimeOffset = -1;
    previewVpOffset = -1;
    const numBlocks = gl.getProgramParameter(
      prog,
      gl.ACTIVE_UNIFORM_BLOCKS,
    ) as number;
    if (numBlocks > 0) {
      // Slice 13: graph shaders only ever bind the one Globals UBO at
      // group(0) binding(0), so it's safe to grab block 0.
      const blockIdx = 0;
      gl.uniformBlockBinding(prog, blockIdx, 0);
      const size = gl.getActiveUniformBlockParameter(
        prog,
        blockIdx,
        gl.UNIFORM_BLOCK_DATA_SIZE,
      ) as number;
      previewUboSize = size;
      previewUbo = gl.createBuffer();
      gl.bindBuffer(gl.UNIFORM_BUFFER, previewUbo);
      gl.bufferData(gl.UNIFORM_BUFFER, size, gl.DYNAMIC_DRAW);
      gl.bindBuffer(gl.UNIFORM_BUFFER, null);
      gl.bindBufferBase(gl.UNIFORM_BUFFER, 0, previewUbo);

      // naga decorates each Globals field with `Globals.<name>` so we
      // can resolve `time` and `view_projection` by suffix-match.
      const memberIndices = gl.getActiveUniformBlockParameter(
        prog,
        blockIdx,
        gl.UNIFORM_BLOCK_ACTIVE_UNIFORM_INDICES,
      ) as Uint32Array;
      const indexArr = Array.from(memberIndices);
      const offsets = gl.getActiveUniforms(
        prog,
        indexArr,
        gl.UNIFORM_OFFSET,
      ) as number[];
      for (let i = 0; i < indexArr.length; i++) {
        const info = gl.getActiveUniform(prog, indexArr[i]);
        if (!info) continue;
        const name = info.name;
        // Match on suffix so we tolerate naga's `Globals.time`,
        // `_group_0_binding_0_fs.time`, etc.
        if (name.endsWith(".time") || name === "time") {
          previewTimeOffset = offsets[i];
        } else if (
          name.endsWith(".view_projection") ||
          name === "view_projection"
        ) {
          previewVpOffset = offsets[i];
        }
      }
    }

    // Bind every active sampler2D to TEXTURE0 (our 1×1 white dummy).
    gl.useProgram(prog);
    const numUniforms = gl.getProgramParameter(
      prog,
      gl.ACTIVE_UNIFORMS,
    ) as number;
    for (let i = 0; i < numUniforms; i++) {
      const info = gl.getActiveUniform(prog, i);
      if (!info) continue;
      if (info.type === gl.SAMPLER_2D) {
        const loc = gl.getUniformLocation(prog, info.name);
        if (loc) gl.uniform1i(loc, 0);
      }
    }
    gl.useProgram(null);

    renderPreview();
  }

  function renderPreview() {
    const gl = previewGl;
    if (!gl || !previewProgram || !previewVao) return;

    gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight);
    gl.clearColor(0.07, 0.07, 0.11, 1.0);
    gl.clear(gl.COLOR_BUFFER_BIT);

    gl.useProgram(previewProgram);

    // Refresh the Globals UBO (time + identity view_projection).
    if (previewUbo && previewUboSize > 0) {
      const buf = new ArrayBuffer(previewUboSize);
      const view = new DataView(buf);
      if (previewVpOffset >= 0) {
        // Column-major identity mat4.
        const id = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
        for (let i = 0; i < 16; i++) {
          view.setFloat32(previewVpOffset + i * 4, id[i], true);
        }
      }
      if (previewTimeOffset >= 0) {
        const t = (performance.now() - previewStartTime) / 1000.0;
        view.setFloat32(previewTimeOffset, t, true);
      }
      gl.bindBuffer(gl.UNIFORM_BUFFER, previewUbo);
      gl.bufferSubData(gl.UNIFORM_BUFFER, 0, new Uint8Array(buf));
      gl.bindBuffer(gl.UNIFORM_BUFFER, null);
    }

    if (previewDummyTex) {
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, previewDummyTex);
    }

    gl.bindVertexArray(previewVao);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    gl.bindVertexArray(null);
  }

  function previewAnimLoop() {
    if (!previewGl || !previewProgram) return;
    renderPreview();
    previewAnimId = requestAnimationFrame(previewAnimLoop);
  }

  async function updatePreview() {
    const g = get(shaderEditorGraph);
    if (!g) return;
    // Slice 13c: codegen still emits canonical WGSL for the runtime, but
    // for the editor preview we pipe that WGSL through naga's GLSL ES 300
    // backend on the Rust side so WebGL2 can compile it directly. Keeps
    // the preview pixel-accurate against what the runtime ships.
    previewCompiling = true;
    try {
      const result = await shaderGeneratePreviewGlsl(JSON.stringify(g));
      cancelAnimationFrame(previewAnimId);
      compilePreviewShader(result.glsl);
      if (previewProgram) {
        previewAnimLoop();
      }
    } catch (err) {
      cancelAnimationFrame(previewAnimId);
      previewError = `${err}`;
    } finally {
      previewCompiling = false;
    }
  }

  // Initialize preview when canvas mounts
  $effect(() => {
    if (previewCanvas && $shaderEditorGraph) {
      initPreviewGl();
      updatePreview();
      return () => {
        cancelAnimationFrame(previewAnimId);
        // Clean up GL
        if (previewGl) {
          if (previewProgram) previewGl.deleteProgram(previewProgram);
          if (previewDummyTex) previewGl.deleteTexture(previewDummyTex);
          if (previewUbo) previewGl.deleteBuffer(previewUbo);
        }
        previewProgram = null;
        previewDummyTex = null;
        previewUbo = null;
        previewGl = null;
      };
    }
  });

  // Recompile preview when graph changes (debounced)
  let previewDebounce: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    // Track graph changes by touching the nodes array
    if ($shaderEditorGraph?.nodes) {
      if (previewDebounce) clearTimeout(previewDebounce);
      previewDebounce = setTimeout(() => {
        if (previewGl) updatePreview();
      }, 500);
    }
  });
</script>

<div class="inspector" class:playing>
  {#if showGuiInspector && $guiDocument}
    <div class="header">GUI Inspector</div>
    <div class="gui-inspector-scroll">
      <GuiInspector />
    </div>
  {:else if showPrefabInspector && $prefabData}
    <div class="header">Prefab Inspector</div>
    <div class="gui-inspector-scroll">
      <PrefabInspector />
    </div>
  {:else if entity}
    {#snippet depWarning(comp: string)}
      {#if missingDeps[comp]}
        <div class="dep-warning">
          ⚠ Requires: {missingDeps[comp].join(", ")}
          <button
            class="dep-fix-btn"
            onclick={() => {
              for (const dep of missingDeps[comp]) {
                handleAddComponent(dep);
              }
            }}>Add</button
          >
        </div>
      {/if}
    {/snippet}
    {#snippet sectionBar(comp: string)}
      <span class="section-bar" style:background={sectionHue(comp)} aria-hidden="true"></span>
    {/snippet}
    {#snippet overrideMark(comp: string)}
      {@const mixedHere = mixedOf(comp)}
      {#if mixedHere.length > 0}
        <span class="mixed-mark" title={`Differs among the selected entities (the values shown are the first one's): ${mixedHere.join(", ")}`}>≠</span>
      {/if}
      {#if entityChanges.has(comp)}
        <span
          class="override-mark"
          title={`Changed from the ${partTemplate ? "part" : "prefab"}: ${(entityChanges.get(comp) ?? []).join(", ")}`}>●</span
        >
      {/if}
    {/snippet}

    {#snippet undrawnFields(comp: string)}
      <!-- The component's fields its section does not draw (componentDefs.ts
           UNDRAWN_FIELDS), with the widgets derived from the component. -->
      {@const info = builtinSchemaMap.get(comp)}
      {@const names = UNDRAWN_FIELDS[comp] ?? []}
      {@const data = (entity.components as Record<string, unknown>)[comp]}
      {#if info && names.length > 0 && data && typeof data === "object"}
        <div class="undrawn-fields">
          <PackageInspector
            overridden={(f) => fieldChanged(entityChanges, comp, f)}
            mixed={(f) => mixedOf(comp).includes(f)}
            info={{ ...info, fields: info.fields.filter((f) => names.includes(f.name)) }}
            data={data as Record<string, unknown>}
            disabled={playing}
            onchange={(field, value) => setComponentField(comp, field, value)}
            oneditpoints={(field) => editPointsInViewport(comp, field)}
          />
        </div>
      {/if}
    {/snippet}
    {#snippet sortLayerPicker(
      _compName: string,
      current: string | undefined,
      setValue: (v: string | undefined) => void,
    )}
      <div class="field-row">
        <label>Sort Layer</label>
        <select
          value={current ?? ""}
          disabled={!$scene?.sort_layers}
          title={$scene?.sort_layers
            ? "Named layer from the scene's .sortlayers asset (effective z = base + z_order)"
            : "Set a scene Sort Layers asset to enable named layers"}
          onchange={(e) => {
            const v = (e.target as HTMLSelectElement).value;
            setValue(v || undefined);
          }}
        >
          <option value="">(none — z_order only)</option>
          {#each activeSortLayerNames as ln (ln)}
            <option value={ln}>{ln}</option>
          {/each}
          {#if current && !activeSortLayerNames.includes(current)}
            <option value={current}>{current} (missing)</option>
          {/if}
        </select>
      </div>
    {/snippet}
    {#if playing}<div class="header">Inspector (Playing)</div>{/if}
    <div class="section entity-head">
      <span class="entity-dot" aria-hidden="true"></span>
      <label class="sr-only" for="inspector-entity-name">Name</label>
      <input
        id="inspector-entity-name"
        class="entity-name"
        type="text"
        value={entity.name}
        onchange={handleNameChange}
      />
    </div>

    {#if !playing && $selectedEntities.size > 1}
      <div class="section multi-edit">
        Editing {$selectedEntities.size} entities: a change here is made on every selected entity
        that has the component. The values shown are {entity.name}'s.
      </div>
    {/if}

    {#if !playing && instanceId}
      <div class="section prefab-instance">
        <div class="prefab-row">
          <span class="prefab-label">Prefab</span>
          <button class="prefab-link" title="Open the prefab" onclick={openInstancePrefab}>{instanceId}</button>
          {#if instanceTemplate}
            <button
              class="prefab-btn"
              title="Unpack: copy the prefab's components into this entity and drop the link"
              onclick={unpackSelected}>Unpack</button
            >
          {/if}
        </div>
        {#if !instanceTemplate}
          <div class="dep-warning">
            ⚠ No prefab "{instanceId}" in the project: the entity shows only its own components.
          </div>
        {:else if instanceRows.length === 0}
          <div class="prefab-hint">Everything comes from the prefab. An edit here becomes an override.</div>
        {:else}
          {#each instanceRows as row (row.path)}
            <div class="override-row" title={JSON.stringify(row.value)}>
              <code>{row.path}</code>
              {#if row.own}<span class="prefab-hint">own</span>{/if}
              <button class="prefab-btn" title="Revert to the prefab's" onclick={() => revertInstance([row.path])}>⟲</button>
              <button class="prefab-btn" title="Apply to the prefab: every instance gets it" onclick={() => applyInstance([row.path])}>⇪</button>
            </div>
          {/each}
          <div class="prefab-row">
            <button class="prefab-btn" onclick={() => revertInstance(instanceRows.map((r) => r.path))}>Revert all</button>
            <button class="prefab-btn" onclick={() => applyInstance(instanceRows.map((r) => r.path))}>Apply all</button>
          </div>
        {/if}
      </div>
    {/if}

    {#if !playing && partTemplate}
      <div class="section prefab-instance">
        <div class="prefab-row">
          <span class="prefab-label">Part</span>
          <span title="A part of a placed prefab, laid over in this scene: what it changes is kept here, each component whole">
            {(sceneEntity?.name ?? "").split("/").pop()} of {sceneEntity?.parent}
          </span>
        </div>
        {#if partRows.length === 0}
          <div class="prefab-hint">Everything comes from the prefab. An edit here is kept in this scene.</div>
        {:else}
          {#each partRows as key (key)}
            <div class="override-row">
              <code>{key}</code>
              <button class="prefab-btn" title="Revert to the part's" onclick={() => revertPartComponents([key])}>⟲</button>
            </div>
          {/each}
        {/if}
        <div class="prefab-row">
          <button class="prefab-btn" title="Remove this scene's changes to the part" onclick={() => revertPartComponents(partRows.length ? partRows : ["__none__"])}>Revert part</button>
        </div>
      </div>
    {/if}

    {#if entity.components.transform}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("transform")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("transform");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["transform"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("transform")}<span class="section-title">Transform</span>{@render overrideMark("transform")}<span class="section-key">transform</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("transform");
            }}
            title="Remove Transform">×</button
          >
        </div>
        {#if !collapsed["transform"]}
          <div class="field-row">
            <label>Position</label>
            <label class="axis x">X</label>
            <input
              type="number"
              step="0.1"
              value={entity.components.transform.position[0]}
              onchange={(e) =>
                handleTransformChange(
                  "position",
                  "x",
                  (e.target as HTMLInputElement).value,
                )}
            />
            <label class="axis y">Y</label>
            <input
              type="number"
              step="0.1"
              value={entity.components.transform.position[1]}
              onchange={(e) =>
                handleTransformChange(
                  "position",
                  "y",
                  (e.target as HTMLInputElement).value,
                )}
            />
          </div>
          <div class="field-row">
            <label>Rotation</label>
            <input
              type="number"
              step="0.01"
              value={entity.components.transform.rotation}
              onchange={(e) =>
                handleTransformChange(
                  "rotation",
                  null,
                  (e.target as HTMLInputElement).value,
                )}
            />
          </div>
          <div class="field-row">
            <label>Scale</label>
            <label class="axis x">X</label>
            <input
              type="number"
              step="0.1"
              value={entity.components.transform.scale[0]}
              onchange={(e) =>
                handleTransformChange(
                  "scale",
                  "x",
                  (e.target as HTMLInputElement).value,
                )}
            />
            <label class="axis y">Y</label>
            <input
              type="number"
              step="0.1"
              value={entity.components.transform.scale[1]}
              onchange={(e) =>
                handleTransformChange(
                  "scale",
                  "y",
                  (e.target as HTMLInputElement).value,
                )}
            />
          </div>
        {/if}
      </div>
    {/if}

    {#if entity.components.sprite}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("sprite")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("sprite");
          }}
        >
          <span class="collapse-arrow">{collapsed["sprite"] ? "▸" : "▾"}</span>
          {@render sectionBar("sprite")}<span class="section-title">Sprite</span>{@render overrideMark("sprite")}<span class="section-key">sprite</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("sprite");
            }}
            title="Remove Sprite">×</button
          >
        </div>
        {@render depWarning("sprite")}
        {#if !collapsed["sprite"]}
          <div class="field-row">
            <label>Texture</label>
            <div class="asset-picker-wrapper">
              <input
                type="text"
                value={entity.components.sprite.texture_id}
                onchange={(e) =>
                  handleSpriteChange(
                    "texture_id",
                    (e.target as HTMLInputElement).value,
                  )}
                placeholder="sprites/image.png"
              />
              <button
                class="picker-btn"
                onclick={() => (spritePickerOpen = !spritePickerOpen)}
                title="Browse sprites">⊞</button
              >
              {#if spritePickerOpen && (spriteFiles.length > 0 || atlasEntries.length > 0 || atlasDefImages.length > 0)}
                <button
                  class="picker-backdrop"
                  onclick={() => (spritePickerOpen = false)}
                  aria-label="Close picker"
                ></button>
                <div class="picker-dropdown" role="listbox">
                  {#if atlasDefImages.length > 0}
                    {#each [...atlasDefImagesByAtlas] as [atlasName, images]}
                      <div class="picker-group-label">🎴 {atlasName}</div>
                      {#each images as img}
                        <button
                          class="picker-item"
                          class:active={entity.components.sprite?.texture_id ===
                            img.image_path &&
                            !entity.components.sprite?.uv_rect}
                          role="option"
                          aria-selected={entity.components.sprite
                            ?.texture_id === img.image_path &&
                            !entity.components.sprite?.uv_rect}
                          onclick={() => {
                            handleAtlasDefImageSelect(img);
                            spritePickerOpen = false;
                          }}
                          >{img.image_name}
                          <span class="picker-item-dim">({img.image_path})</span
                          ></button
                        >
                      {/each}
                    {/each}
                  {/if}
                  {#if spriteFiles.length > 0}
                    <div class="picker-group-label">Images</div>
                    {#each spriteFiles as file}
                      <button
                        class="picker-item"
                        class:active={entity.components.sprite?.texture_id ===
                          file && !entity.components.sprite?.uv_rect}
                        role="option"
                        aria-selected={entity.components.sprite?.texture_id ===
                          file && !entity.components.sprite?.uv_rect}
                        onclick={() => {
                          handleSpriteChange("texture_id", file);
                          spritePickerOpen = false;
                        }}>{file}</button
                      >
                    {/each}
                  {/if}
                  {#if atlasEntries.length > 0}
                    <div class="picker-group-label">Atlas Regions</div>
                    {#each atlasEntries as entry}
                      <button
                        class="picker-item"
                        class:active={entity.components.sprite?.texture_id ===
                          entry.atlas_image &&
                          entity.components.sprite?.uv_rect?.[0] ===
                            entry.uv_rect[0] &&
                          entity.components.sprite?.uv_rect?.[1] ===
                            entry.uv_rect[1]}
                        role="option"
                        aria-selected={entity.components.sprite?.texture_id ===
                          entry.atlas_image &&
                          entity.components.sprite?.uv_rect?.[0] ===
                            entry.uv_rect[0] &&
                          entity.components.sprite?.uv_rect?.[1] ===
                            entry.uv_rect[1]}
                        onclick={() => {
                          handleAtlasRegionSelect(entry);
                          spritePickerOpen = false;
                        }}
                        >{entry.name}
                        <span class="picker-item-dim"
                          >({entry.size[0]}×{entry.size[1]})</span
                        ></button
                      >
                    {/each}
                  {/if}
                </div>
              {:else if spritePickerOpen}
                <button
                  class="picker-backdrop"
                  onclick={() => (spritePickerOpen = false)}
                  aria-label="Close picker"
                ></button>
                <div class="picker-dropdown">
                  <span class="picker-empty">No sprite files in project</span>
                </div>
              {/if}
            </div>
          </div>
          <div class="field-row">
            <label>Color</label>
            <input
              type="color"
              value={colorToHex(entity.components.sprite.color)}
              onchange={(e) =>
                handleColorPicker((e.target as HTMLInputElement).value)}
            />
            <label>A</label>
            <input
              type="number"
              step="0.05"
              min="0"
              max="1"
              value={entity.components.sprite.color[3]}
              onchange={(e) =>
                handleColorChange(3, (e.target as HTMLInputElement).value)}
            />
          </div>
          <div class="field-row">
            <label>Z</label>
            <input
              type="number"
              value={entity.components.sprite.z_order}
              onchange={(e) =>
                handleSpriteChange(
                  "z_order",
                  (e.target as HTMLInputElement).value,
                )}
            />
          </div>
          {@render sortLayerPicker(
            "sprite",
            entity.components.sprite.sort_layer,
            (v) =>
              editEntity("Change sprite sort_layer", (ent) => {
                if (ent.components.sprite) ent.components.sprite.sort_layer = v;
              }),
          )}
          <div class="field-row">
            <label>Y Sort</label>
            <input
              type="checkbox"
              checked={entity.components.sprite.y_sort ?? false}
              title="Sort against the other sprites at this Z by position — lower on screen draws in front. The entity's origin is what sorts, so anchor a character at its feet."
              onchange={(e) => {
                const on = (e.target as HTMLInputElement).checked;
                editEntity("Change sprite y_sort", (ent) => {
                  if (ent.components.sprite)
                    ent.components.sprite.y_sort = on || undefined;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Size</label>
            {#if entity.components.sprite.size}
              <input
                type="number"
                step="1"
                value={entity.components.sprite.size[0]}
                onchange={(e) =>
                  handleSizeChange(0, (e.target as HTMLInputElement).value)}
              />
              <span class="dim">×</span>
              <input
                type="number"
                step="1"
                value={entity.components.sprite.size[1]}
                onchange={(e) =>
                  handleSizeChange(1, (e.target as HTMLInputElement).value)}
              />
              <button
                class="remove-btn small"
                onclick={handleClearSize}
                title="Use texture size">×</button
              >
            {:else}
              <span class="dim">auto</span>
              <button
                class="add-size-btn"
                onclick={() => handleSizeChange(0, "64")}>Set</button
              >
            {/if}
          </div>
          <div class="field-row">
            <label>Anchor</label>
            <input
              type="number"
              step="0.05"
              min="0"
              max="1"
              value={entity.components.sprite.anchor?.[0] ?? 0.5}
              onchange={(e) =>
                handleAnchorChange(0, (e.target as HTMLInputElement).value)}
            />
            <input
              type="number"
              step="0.05"
              min="0"
              max="1"
              value={entity.components.sprite.anchor?.[1] ?? 0.5}
              onchange={(e) =>
                handleAnchorChange(1, (e.target as HTMLInputElement).value)}
            />
          </div>
          <!-- Stretched, nine-sliced (borders dragged on its picture) or tiled. -->
          <SpriteFillEditor
            fill={entity.components.sprite.fill}
            textureId={entity.components.sprite.texture_id ?? ""}
            uvRect={entity.components.sprite.uv_rect ?? null}
            disabled={playing}
            onchange={(fill) => setComponentField("sprite", "fill", fill)}
          />
          {@render undrawnFields("sprite")}
        {/if}
      </div>
    {/if}

    {#if entity.components.material}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("material")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("material");
          }}
        >
          <span class="collapse-arrow">{collapsed["material"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("material")}<span class="section-title">Material</span>{@render overrideMark("material")}<span class="section-key">material</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("material");
            }}
            title="Remove Material">×</button
          >
        </div>
        {@render depWarning("material")}
        {#if !collapsed["material"]}
          <div class="field-row">
            <label>Material</label>
            <select
              value={entity.components.material.material_id}
              onchange={(e) => {
                const newId = (e.target as HTMLSelectElement).value;
                editEntity("Change material", (ent) => {
                  if (ent.components.material) {
                    ent.components.material.material_id = newId;
                    // Auto-populate default uniforms for effect shaders
                    const defaults: Record<string, Record<string, unknown>> = {
                      emission: {
                        u_emission_color: { Vec4: [1, 0.5, 0, 1] },
                        u_emission_strength: { Float: 0 },
                      },
                      outline: {
                        u_outline_color: { Vec4: [1, 1, 1, 1] },
                        u_outline_width: { Float: 1 },
                        u_texture_size: { Vec2: [64, 64] },
                      },
                      scroll: { u_scroll_speed: { Vec2: [0.5, 0] } },
                      flash: {
                        u_flash_color: { Vec4: [1, 1, 1, 1] },
                        u_flash_amount: { Float: 0 },
                      },
                      lit: {
                        u_has_normal_map: { Int: 0 },
                        u_ambient_color: { Vec3: [1, 1, 1] },
                        u_ambient_intensity: { Float: 0.3 },
                        u_num_lights: { Int: 0 },
                      },
                    };
                    if (defaults[newId]) {
                      ent.components.material.uniforms = { ...defaults[newId] };
                    }
                  }
                });
              }}
            >
              <option value="default_sprite">Default Sprite</option>
              <option value="additive">Additive</option>
              <option value="multiply">Multiply</option>
              <option value="screen">Screen</option>
              <option value="emission">Emission / Glow</option>
              <option value="outline">Outline / Stroke</option>
              <option value="scroll">UV Scroll</option>
              <option value="flash">Flash / Hit</option>
              <option value="lit">Lit (Normal Map)</option>
              {#if projectMaterialIds.length > 0}
                <optgroup label="Project materials">
                  {#each projectMaterialIds as mid (mid)}
                    <option value={mid}>{mid}</option>
                  {/each}
                </optgroup>
              {/if}
              {#if entity.components.material.material_id && !projectMaterialIds.includes(entity.components.material.material_id) && !["default_sprite", "additive", "multiply", "screen", "emission", "outline", "scroll", "flash", "lit"].includes(entity.components.material.material_id)}
                <option value={entity.components.material.material_id}>
                  {entity.components.material.material_id} (missing)
                </option>
              {/if}
            </select>
          </div>
          <div class="field-row">
            <label>Blend Mode</label>
            <select
              value={entity.components.material.blend_mode ?? ""}
              onchange={(e) => {
                editEntity("Change blend mode override", (ent) => {
                  if (ent.components.material) {
                    const val = (e.target as HTMLSelectElement).value;
                    ent.components.material.blend_mode = val || undefined;
                  }
                });
              }}
            >
              <option value="">Default (from material)</option>
              <option value="Alpha">Alpha</option>
              <option value="Additive">Additive</option>
              <option value="Multiply">Multiply</option>
              <option value="Screen">Screen</option>
              <option value="PremultipliedAlpha">Premultiplied Alpha</option>
            </select>
          </div>

          <!-- Normal map: any material can have one (it was shown only for a
               material named "lit", which no scene uses). -->
          <div class="field-row">
            <label>Normal Map</label>
            <input
              type="text"
              placeholder="e.g. sprites/player_n.png"
              value={entity.components.material.normal_map_id ?? ""}
              onchange={(e) => {
                const val = (e.target as HTMLInputElement).value.trim();
                editEntity("Set normal map", (ent) => {
                  if (ent.components.material) {
                    ent.components.material.normal_map_id = val || undefined;
                  }
                });
              }}
            />
          </div>
          <!-- Specular highlights from point and spot lights (the normal map's
               alpha masks them). -->
          <div class="field-row">
            <label>Specular</label>
            <input
              type="number"
              min="0"
              max="1"
              step="0.05"
              value={entity.components.material.specular ?? 0}
              onchange={(e) => {
                const val = parseFloat((e.target as HTMLInputElement).value);
                editEntity("Set specular", (ent) => {
                  if (ent.components.material) {
                    ent.components.material.specular = Number.isFinite(val) && val > 0 ? val : undefined;
                  }
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Shininess</label>
            <input
              type="number"
              min="1"
              max="128"
              step="1"
              value={entity.components.material.shininess ?? 24}
              onchange={(e) => {
                const val = parseFloat((e.target as HTMLInputElement).value);
                editEntity("Set shininess", (ent) => {
                  if (ent.components.material) {
                    ent.components.material.shininess = Number.isFinite(val) ? val : undefined;
                  }
                });
              }}
            />
          </div>

          <!-- Emission effect controls -->
          {#if entity.components.material.material_id === "emission"}
            <div class="field-row">
              <label>Glow Color</label>
              <input
                type="color"
                value={(() => {
                  const c = (
                    entity.components.material.uniforms
                      ?.u_emission_color as Record<string, number[]>
                  )?.Vec4 ?? [1, 0.5, 0, 1];
                  return (
                    "#" +
                    [c[0], c[1], c[2]]
                      .map((v) =>
                        Math.round(v * 255)
                          .toString(16)
                          .padStart(2, "0"),
                      )
                      .join("")
                  );
                })()}
                onchange={(e) => {
                  const hex = (e.target as HTMLInputElement).value;
                  const r = parseInt(hex.slice(1, 3), 16) / 255;
                  const g = parseInt(hex.slice(3, 5), 16) / 255;
                  const b = parseInt(hex.slice(5, 7), 16) / 255;
                  editEntity("Change emission color", (ent) => {
                    if (ent.components.material) {
                      const u = ent.components.material.uniforms ?? {};
                      const old = (
                        u.u_emission_color as Record<string, number[]>
                      )?.Vec4 ?? [1, 0.5, 0, 1];
                      u.u_emission_color = { Vec4: [r, g, b, old[3]] };
                      ent.components.material.uniforms = u;
                    }
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Strength</label>
              <input
                type="range"
                min="0"
                max="3"
                step="0.05"
                value={(
                  entity.components.material.uniforms
                    ?.u_emission_strength as Record<string, number>
                )?.Float ?? 0}
                oninput={(e) => {
                  editEntity("Change emission strength", (ent) => {
                    if (ent.components.material) {
                      const u = ent.components.material.uniforms ?? {};
                      u.u_emission_strength = {
                        Float: parseFloat((e.target as HTMLInputElement).value),
                      };
                      ent.components.material.uniforms = u;
                    }
                  });
                }}
              />
              <span style="width: 30px; text-align: right;"
                >{(
                  (
                    entity.components.material.uniforms
                      ?.u_emission_strength as Record<string, number>
                  )?.Float ?? 0
                ).toFixed(1)}</span
              >
            </div>
          {/if}

          <!-- Outline effect controls -->
          {#if entity.components.material.material_id === "outline"}
            <div class="field-row">
              <label>Color</label>
              <input
                type="color"
                value={(() => {
                  const c = (
                    entity.components.material.uniforms
                      ?.u_outline_color as Record<string, number[]>
                  )?.Vec4 ?? [1, 1, 1, 1];
                  return (
                    "#" +
                    [c[0], c[1], c[2]]
                      .map((v) =>
                        Math.round(v * 255)
                          .toString(16)
                          .padStart(2, "0"),
                      )
                      .join("")
                  );
                })()}
                onchange={(e) => {
                  const hex = (e.target as HTMLInputElement).value;
                  const r = parseInt(hex.slice(1, 3), 16) / 255;
                  const g = parseInt(hex.slice(3, 5), 16) / 255;
                  const b = parseInt(hex.slice(5, 7), 16) / 255;
                  editEntity("Change outline color", (ent) => {
                    if (ent.components.material) {
                      const u = ent.components.material.uniforms ?? {};
                      u.u_outline_color = { Vec4: [r, g, b, 1] };
                      ent.components.material.uniforms = u;
                    }
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Width</label>
              <input
                type="range"
                min="0"
                max="5"
                step="0.5"
                value={(
                  entity.components.material.uniforms
                    ?.u_outline_width as Record<string, number>
                )?.Float ?? 1}
                oninput={(e) => {
                  editEntity("Change outline width", (ent) => {
                    if (ent.components.material) {
                      const u = ent.components.material.uniforms ?? {};
                      u.u_outline_width = {
                        Float: parseFloat((e.target as HTMLInputElement).value),
                      };
                      ent.components.material.uniforms = u;
                    }
                  });
                }}
              />
              <span style="width: 30px; text-align: right;"
                >{(
                  (
                    entity.components.material.uniforms
                      ?.u_outline_width as Record<string, number>
                  )?.Float ?? 1
                ).toFixed(1)}</span
              >
            </div>
          {/if}

          <!-- Scroll effect controls -->
          {#if entity.components.material.material_id === "scroll"}
            <div class="field-row">
              <label>Speed X</label>
              <input
                type="number"
                step="0.1"
                value={((
                  entity.components.material.uniforms?.u_scroll_speed as Record<
                    string,
                    number[]
                  >
                )?.Vec2 ?? [0.5, 0])[0]}
                onchange={(e) => {
                  editEntity("Change scroll speed", (ent) => {
                    if (ent.components.material) {
                      const u = ent.components.material.uniforms ?? {};
                      const old = (u.u_scroll_speed as Record<string, number[]>)
                        ?.Vec2 ?? [0.5, 0];
                      u.u_scroll_speed = {
                        Vec2: [
                          parseFloat((e.target as HTMLInputElement).value) || 0,
                          old[1],
                        ],
                      };
                      ent.components.material.uniforms = u;
                    }
                  });
                }}
              />
              <label>Y</label>
              <input
                type="number"
                step="0.1"
                value={((
                  entity.components.material.uniforms?.u_scroll_speed as Record<
                    string,
                    number[]
                  >
                )?.Vec2 ?? [0.5, 0])[1]}
                onchange={(e) => {
                  editEntity("Change scroll speed", (ent) => {
                    if (ent.components.material) {
                      const u = ent.components.material.uniforms ?? {};
                      const old = (u.u_scroll_speed as Record<string, number[]>)
                        ?.Vec2 ?? [0.5, 0];
                      u.u_scroll_speed = {
                        Vec2: [
                          old[0],
                          parseFloat((e.target as HTMLInputElement).value) || 0,
                        ],
                      };
                      ent.components.material.uniforms = u;
                    }
                  });
                }}
              />
            </div>
          {/if}

          <!-- Flash effect controls -->
          {#if entity.components.material.material_id === "flash"}
            <div class="field-row">
              <label>Color</label>
              <input
                type="color"
                value={(() => {
                  const c = (
                    entity.components.material.uniforms
                      ?.u_flash_color as Record<string, number[]>
                  )?.Vec4 ?? [1, 1, 1, 1];
                  return (
                    "#" +
                    [c[0], c[1], c[2]]
                      .map((v) =>
                        Math.round(v * 255)
                          .toString(16)
                          .padStart(2, "0"),
                      )
                      .join("")
                  );
                })()}
                onchange={(e) => {
                  const hex = (e.target as HTMLInputElement).value;
                  const r = parseInt(hex.slice(1, 3), 16) / 255;
                  const g = parseInt(hex.slice(3, 5), 16) / 255;
                  const b = parseInt(hex.slice(5, 7), 16) / 255;
                  editEntity("Change flash color", (ent) => {
                    if (ent.components.material) {
                      const u = ent.components.material.uniforms ?? {};
                      u.u_flash_color = { Vec4: [r, g, b, 1] };
                      ent.components.material.uniforms = u;
                    }
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Amount</label>
              <input
                type="range"
                min="0"
                max="1"
                step="0.05"
                value={(
                  entity.components.material.uniforms?.u_flash_amount as Record<
                    string,
                    number
                  >
                )?.Float ?? 0}
                oninput={(e) => {
                  editEntity("Change flash amount", (ent) => {
                    if (ent.components.material) {
                      const u = ent.components.material.uniforms ?? {};
                      u.u_flash_amount = {
                        Float: parseFloat((e.target as HTMLInputElement).value),
                      };
                      ent.components.material.uniforms = u;
                    }
                  });
                }}
              />
              <span style="width: 30px; text-align: right;"
                >{(
                  (
                    entity.components.material.uniforms
                      ?.u_flash_amount as Record<string, number>
                  )?.Float ?? 0
                ).toFixed(2)}</span
              >
            </div>
          {/if}

          <div
            class="field-row"
            style="flex-direction: column; align-items: stretch;"
          >
            <div
              style="display: flex; align-items: center; justify-content: space-between;"
            >
              <label>Uniforms</label>
              <button
                class="small-btn"
                onclick={() => {
                  editEntity("Add uniform", (ent) => {
                    if (ent.components.material) {
                      const uniforms = ent.components.material.uniforms ?? {};
                      let name = "u_custom";
                      let i = 1;
                      while (name in uniforms) {
                        name = `u_custom_${i++}`;
                      }
                      uniforms[name] = { Float: 0 };
                      ent.components.material.uniforms = uniforms;
                    }
                  });
                }}>+ Add</button
              >
            </div>
            {#each Object.entries(entity.components.material.uniforms ?? {}) as [uName, uVal]}
              <div class="field-row" style="padding-left: 8px;">
                <input
                  type="text"
                  value={uName}
                  style="width: 80px;"
                  onchange={(e) => {
                    const newName = (e.target as HTMLInputElement).value.trim();
                    if (!newName || newName === uName) return;
                    editEntity("Rename uniform", (ent) => {
                      if (ent.components.material) {
                        const u = ent.components.material.uniforms ?? {};
                        const val = u[uName];
                        delete u[uName];
                        u[newName] = val;
                        ent.components.material.uniforms = u;
                      }
                    });
                  }}
                />
                <select
                  value={typeof uVal === "object" && uVal !== null
                    ? Object.keys(uVal)[0]
                    : "Float"}
                  style="width: 60px;"
                  onchange={(e) => {
                    const kind = (e.target as HTMLSelectElement).value;
                    editEntity("Change uniform type", (ent) => {
                      if (ent.components.material) {
                        const u = ent.components.material.uniforms ?? {};
                        if (kind === "Float") u[uName] = { Float: 0 };
                        else if (kind === "Int") u[uName] = { Int: 0 };
                        else if (kind === "Vec2") u[uName] = { Vec2: [0, 0] };
                        else if (kind === "Vec3")
                          u[uName] = { Vec3: [0, 0, 0] };
                        else if (kind === "Vec4")
                          u[uName] = { Vec4: [0, 0, 0, 1] };
                        ent.components.material.uniforms = u;
                      }
                    });
                  }}
                >
                  <option value="Float">Float</option>
                  <option value="Int">Int</option>
                  <option value="Vec2">Vec2</option>
                  <option value="Vec3">Vec3</option>
                  <option value="Vec4">Vec4</option>
                </select>
                {#if typeof uVal === "object" && uVal !== null}
                  {@const kind = Object.keys(uVal)[0]}
                  {#if kind === "Float"}
                    <input
                      type="number"
                      step="0.01"
                      value={(uVal as Record<string, number>).Float}
                      onchange={(e) => {
                        editEntity("Change uniform value", (ent) => {
                          if (ent.components.material) {
                            const u = ent.components.material.uniforms ?? {};
                            u[uName] = {
                              Float:
                                parseFloat(
                                  (e.target as HTMLInputElement).value,
                                ) || 0,
                            };
                            ent.components.material.uniforms = u;
                          }
                        });
                      }}
                    />
                  {:else if kind === "Int"}
                    <input
                      type="number"
                      step="1"
                      value={(uVal as Record<string, number>).Int}
                      onchange={(e) => {
                        editEntity("Change uniform value", (ent) => {
                          if (ent.components.material) {
                            const u = ent.components.material.uniforms ?? {};
                            u[uName] = {
                              Int:
                                parseInt(
                                  (e.target as HTMLInputElement).value,
                                ) || 0,
                            };
                            ent.components.material.uniforms = u;
                          }
                        });
                      }}
                    />
                  {:else if kind === "Vec2"}
                    {#each [0, 1] as vi}
                      <input
                        type="number"
                        step="0.01"
                        value={(uVal as Record<string, number[]>).Vec2[vi]}
                        style="width: 50px;"
                        onchange={(e) => {
                          editEntity("Change uniform value", (ent) => {
                            if (ent.components.material) {
                              const u = ent.components.material.uniforms ?? {};
                              const arr = [
                                ...((u[uName] as Record<string, number[]>)
                                  ?.Vec2 ?? [0, 0]),
                              ];
                              arr[vi] =
                                parseFloat(
                                  (e.target as HTMLInputElement).value,
                                ) || 0;
                              u[uName] = { Vec2: arr };
                              ent.components.material.uniforms = u;
                            }
                          });
                        }}
                      />
                    {/each}
                  {:else if kind === "Vec3"}
                    {#each [0, 1, 2] as vi}
                      <input
                        type="number"
                        step="0.01"
                        value={(uVal as Record<string, number[]>).Vec3[vi]}
                        style="width: 45px;"
                        onchange={(e) => {
                          editEntity("Change uniform value", (ent) => {
                            if (ent.components.material) {
                              const u = ent.components.material.uniforms ?? {};
                              const arr = [
                                ...((u[uName] as Record<string, number[]>)
                                  ?.Vec3 ?? [0, 0, 0]),
                              ];
                              arr[vi] =
                                parseFloat(
                                  (e.target as HTMLInputElement).value,
                                ) || 0;
                              u[uName] = { Vec3: arr };
                              ent.components.material.uniforms = u;
                            }
                          });
                        }}
                      />
                    {/each}
                  {:else if kind === "Vec4"}
                    {#each [0, 1, 2, 3] as vi}
                      <input
                        type="number"
                        step="0.01"
                        value={(uVal as Record<string, number[]>).Vec4[vi]}
                        style="width: 40px;"
                        onchange={(e) => {
                          editEntity("Change uniform value", (ent) => {
                            if (ent.components.material) {
                              const u = ent.components.material.uniforms ?? {};
                              const arr = [
                                ...((u[uName] as Record<string, number[]>)
                                  ?.Vec4 ?? [0, 0, 0, 1]),
                              ];
                              arr[vi] =
                                parseFloat(
                                  (e.target as HTMLInputElement).value,
                                ) || 0;
                              u[uName] = { Vec4: arr };
                              ent.components.material.uniforms = u;
                            }
                          });
                        }}
                      />
                    {/each}
                  {/if}
                {/if}
                <button
                  class="remove-btn"
                  onclick={() => {
                    editEntity("Remove uniform", (ent) => {
                      if (ent.components.material) {
                        const u = { ...ent.components.material.uniforms };
                        delete u[uName];
                        ent.components.material.uniforms = u;
                      }
                    });
                  }}
                  title="Remove uniform">×</button
                >
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    <!-- Point Light 2D -->
    {#if entity.components.point_light}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("point_light")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("point_light");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["point_light"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("point_light")}<span class="section-title">Point Light</span>{@render overrideMark("point_light")}<span class="section-key">point_light</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              editEntity("Remove Point Light", (ent) => {
                ent.components.point_light = undefined;
              });
            }}
            title="Remove">×</button
          >
        </div>
        {#if !collapsed["point_light"]}
          <div class="section-body">
            <div class="field-row">
              <label>Color</label>
              <input
                type="color"
                value={colorToHex([
                  ...entity.components.point_light.color,
                  1,
                ] as [number, number, number, number])}
                onchange={(e) => {
                  const hex = (e.target as HTMLInputElement).value;
                  const r = parseInt(hex.slice(1, 3), 16) / 255;
                  const g = parseInt(hex.slice(3, 5), 16) / 255;
                  const b = parseInt(hex.slice(5, 7), 16) / 255;
                  editEntity("Set point light color", (ent) => {
                    if (ent.components.point_light)
                      ent.components.point_light.color = [r, g, b];
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Intensity</label>
              <input
                type="range"
                min="0"
                max="5"
                step="0.1"
                value={entity.components.point_light.intensity}
                oninput={(e) => {
                  editEntity("Set point light intensity", (ent) => {
                    if (ent.components.point_light)
                      ent.components.point_light.intensity = parseFloat(
                        (e.target as HTMLInputElement).value,
                      );
                  });
                }}
              />
              <span style="width: 30px; text-align: right;"
                >{entity.components.point_light.intensity.toFixed(1)}</span
              >
            </div>
            <div class="field-row">
              <label>Radius</label>
              <input
                type="number"
                step="10"
                min="0"
                value={entity.components.point_light.radius}
                onchange={(e) => {
                  editEntity("Set point light radius", (ent) => {
                    if (ent.components.point_light)
                      ent.components.point_light.radius =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Falloff</label>
              <input
                type="range"
                min="0.5"
                max="5"
                step="0.1"
                value={entity.components.point_light.falloff}
                oninput={(e) => {
                  editEntity("Set point light falloff", (ent) => {
                    if (ent.components.point_light)
                      ent.components.point_light.falloff = parseFloat(
                        (e.target as HTMLInputElement).value,
                      );
                  });
                }}
              />
              <span style="width: 30px; text-align: right;"
                >{entity.components.point_light.falloff.toFixed(1)}</span
              >
            </div>
            <div class="field-row">
              <label title="A texture the light shines through, spanning its reach and turning with it - white passes, black stops">Cookie</label>
              <input
                type="text"
                placeholder="none, e.g. sprites/window.png"
                value={entity.components.point_light.cookie ?? ""}
                onchange={(e) => {
                  const val = (e.target as HTMLInputElement).value.trim();
                  editEntity("Set point light cookie", (ent) => {
                    if (ent.components.point_light) ent.components.point_light.cookie = val || undefined;
                  });
                }}
              />
            </div>
            {#if entity.components.point_light.cookie}
              <div class="field-row">
                <label title="How far the cookie reaches, as a share of the radius">Cookie scale</label>
                <input
                  type="number"
                  step="0.05"
                  min="0.05"
                  value={entity.components.point_light.cookie_scale ?? 1}
                  onchange={(e) => {
                    const v = parseFloat((e.target as HTMLInputElement).value);
                    editEntity("Set point light cookie scale", (ent) => {
                      if (ent.components.point_light) ent.components.point_light.cookie_scale = isNaN(v) ? 1 : v;
                    });
                  }}
                />
              </div>
            {/if}
            <div class="field-row">
              <label title="The light layers this light lights: a sprite is lit by it when its Light Layers share one (all, by default)">Lights layers</label>
              <LayerMaskField
                mask={entity.components.point_light.light_mask}
                onchange={(m) =>
                  editEntity("Set point light layers", (ent) => {
                    if (ent.components.point_light) ent.components.point_light.light_mask = m;
                  })}
              />
            </div>
            <div class="field-row">
              <label title="Take light away instead of adding it - darkness cast by a cursed lantern">Subtract</label>
              <input
                type="checkbox"
                checked={!!entity.components.point_light.subtract}
                onchange={(e) =>
                  editEntity("Toggle point light subtract", (ent) => {
                    if (ent.components.point_light) ent.components.point_light.subtract = (e.target as HTMLInputElement).checked || undefined;
                  })}
              />
            </div>
          </div>
          {@render undrawnFields("point_light")}
        {/if}
      </div>
    {/if}

    <!-- Spot Light 2D -->
    {#if entity.components.spot_light}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("spot_light")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("spot_light");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["spot_light"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("spot_light")}<span class="section-title">Spot Light</span>{@render overrideMark("spot_light")}<span class="section-key">spot_light</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              editEntity("Remove Spot Light", (ent) => {
                ent.components.spot_light = undefined;
              });
            }}
            title="Remove">×</button
          >
        </div>
        {#if !collapsed["spot_light"]}
          <div class="section-body">
            <div class="field-row">
              <label>Color</label>
              <input
                type="color"
                value={colorToHex([
                  ...entity.components.spot_light.color,
                  1,
                ] as [number, number, number, number])}
                onchange={(e) => {
                  const hex = (e.target as HTMLInputElement).value;
                  const r = parseInt(hex.slice(1, 3), 16) / 255;
                  const g = parseInt(hex.slice(3, 5), 16) / 255;
                  const b = parseInt(hex.slice(5, 7), 16) / 255;
                  editEntity("Set spot light color", (ent) => {
                    if (ent.components.spot_light)
                      ent.components.spot_light.color = [r, g, b];
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Intensity</label>
              <input
                type="range"
                min="0"
                max="5"
                step="0.1"
                value={entity.components.spot_light.intensity}
                oninput={(e) => {
                  editEntity("Set spot light intensity", (ent) => {
                    if (ent.components.spot_light)
                      ent.components.spot_light.intensity = parseFloat(
                        (e.target as HTMLInputElement).value,
                      );
                  });
                }}
              />
              <span style="width: 30px; text-align: right;"
                >{entity.components.spot_light.intensity.toFixed(1)}</span
              >
            </div>
            <div class="field-row">
              <label>Radius</label>
              <input
                type="number"
                step="10"
                min="0"
                value={entity.components.spot_light.radius}
                onchange={(e) => {
                  editEntity("Set spot light radius", (ent) => {
                    if (ent.components.spot_light)
                      ent.components.spot_light.radius =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Angle (°)</label>
              <input
                type="range"
                min="5"
                max="180"
                step="1"
                value={Math.round(
                  (entity.components.spot_light.angle * 180) / Math.PI,
                )}
                oninput={(e) => {
                  editEntity("Set spot light angle", (ent) => {
                    if (ent.components.spot_light)
                      ent.components.spot_light.angle =
                        (parseFloat((e.target as HTMLInputElement).value) *
                          Math.PI) /
                        180;
                  });
                }}
              />
              <span style="width: 30px; text-align: right;"
                >{Math.round(
                  (entity.components.spot_light.angle * 180) / Math.PI,
                )}°</span
              >
            </div>
            <div class="field-row">
              <label>Direction (°)</label>
              <input
                type="range"
                min="0"
                max="360"
                step="1"
                value={Math.round(
                  (entity.components.spot_light.direction * 180) / Math.PI,
                )}
                oninput={(e) => {
                  editEntity("Set spot light direction", (ent) => {
                    if (ent.components.spot_light)
                      ent.components.spot_light.direction =
                        (parseFloat((e.target as HTMLInputElement).value) *
                          Math.PI) /
                        180;
                  });
                }}
              />
              <span style="width: 30px; text-align: right;"
                >{Math.round(
                  (entity.components.spot_light.direction * 180) / Math.PI,
                )}°</span
              >
            </div>
            <div class="field-row">
              <label>Falloff</label>
              <input
                type="range"
                min="0.5"
                max="5"
                step="0.1"
                value={entity.components.spot_light.falloff}
                oninput={(e) => {
                  editEntity("Set spot light falloff", (ent) => {
                    if (ent.components.spot_light)
                      ent.components.spot_light.falloff = parseFloat(
                        (e.target as HTMLInputElement).value,
                      );
                  });
                }}
              />
              <span style="width: 30px; text-align: right;"
                >{entity.components.spot_light.falloff.toFixed(1)}</span
              >
            </div>
            <div class="field-row">
              <label title="A texture the light shines through, spanning its reach and turning with it - white passes, black stops">Cookie</label>
              <input
                type="text"
                placeholder="none, e.g. sprites/window.png"
                value={entity.components.spot_light.cookie ?? ""}
                onchange={(e) => {
                  const val = (e.target as HTMLInputElement).value.trim();
                  editEntity("Set spot light cookie", (ent) => {
                    if (ent.components.spot_light) ent.components.spot_light.cookie = val || undefined;
                  });
                }}
              />
            </div>
            {#if entity.components.spot_light.cookie}
              <div class="field-row">
                <label title="How far the cookie reaches, as a share of the radius">Cookie scale</label>
                <input
                  type="number"
                  step="0.05"
                  min="0.05"
                  value={entity.components.spot_light.cookie_scale ?? 1}
                  onchange={(e) => {
                    const v = parseFloat((e.target as HTMLInputElement).value);
                    editEntity("Set spot light cookie scale", (ent) => {
                      if (ent.components.spot_light) ent.components.spot_light.cookie_scale = isNaN(v) ? 1 : v;
                    });
                  }}
                />
              </div>
            {/if}
            <div class="field-row">
              <label title="The light layers this light lights: a sprite is lit by it when its Light Layers share one (all, by default)">Lights layers</label>
              <LayerMaskField
                mask={entity.components.spot_light.light_mask}
                onchange={(m) =>
                  editEntity("Set spot light layers", (ent) => {
                    if (ent.components.spot_light) ent.components.spot_light.light_mask = m;
                  })}
              />
            </div>
            <div class="field-row">
              <label title="Take light away instead of adding it - darkness cast by a cursed lantern">Subtract</label>
              <input
                type="checkbox"
                checked={!!entity.components.spot_light.subtract}
                onchange={(e) =>
                  editEntity("Toggle spot light subtract", (ent) => {
                    if (ent.components.spot_light) ent.components.spot_light.subtract = (e.target as HTMLInputElement).checked || undefined;
                  })}
              />
            </div>
          </div>
          {@render undrawnFields("spot_light")}
        {/if}
      </div>
    {/if}

    <!-- Ambient Light -->
    {#if entity.components.ambient_light}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("ambient_light")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("ambient_light");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["ambient_light"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("ambient_light")}<span class="section-title">Ambient Light</span>{@render overrideMark("ambient_light")}<span class="section-key">ambient_light</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              editEntity("Remove Ambient Light", (ent) => {
                ent.components.ambient_light = undefined;
              });
            }}
            title="Remove">×</button
          >
        </div>
        {#if !collapsed["ambient_light"]}
          <div class="section-body">
            <div class="field-row">
              <label>Color</label>
              <input
                type="color"
                value={colorToHex([
                  ...entity.components.ambient_light.color,
                  1,
                ] as [number, number, number, number])}
                onchange={(e) => {
                  const hex = (e.target as HTMLInputElement).value;
                  const r = parseInt(hex.slice(1, 3), 16) / 255;
                  const g = parseInt(hex.slice(3, 5), 16) / 255;
                  const b = parseInt(hex.slice(5, 7), 16) / 255;
                  editEntity("Set ambient light color", (ent) => {
                    if (ent.components.ambient_light)
                      ent.components.ambient_light.color = [r, g, b];
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Intensity</label>
              <input
                type="range"
                min="0"
                max="2"
                step="0.05"
                value={entity.components.ambient_light.intensity}
                oninput={(e) => {
                  editEntity("Set ambient light intensity", (ent) => {
                    if (ent.components.ambient_light)
                      ent.components.ambient_light.intensity = parseFloat(
                        (e.target as HTMLInputElement).value,
                      );
                  });
                }}
              />
              <span style="width: 30px; text-align: right;"
                >{entity.components.ambient_light.intensity.toFixed(2)}</span
              >
            </div>
          </div>
        {/if}
      </div>
    {/if}

    {#if entity.components.lightmap}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          onclick={() => toggleSection("lightmap")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("lightmap");
          }}
        >
          <span class="collapse-arrow">{collapsed["lightmap"] ? "▸" : "▾"}</span
          >
          Lightmap
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              editEntity("Remove Lightmap", (ent) => {
                ent.components.lightmap = undefined;
              });
            }}
            title="Remove">×</button
          >
        </div>
        {#if !collapsed["lightmap"]}
          <div class="section-body">
            <div class="field-row">
              <label>Texture</label>
              <input
                type="text"
                placeholder="e.g. lightmap.png"
                value={entity.components.lightmap.texture_id}
                onchange={(e) => {
                  const val = (e.target as HTMLInputElement).value.trim();
                  editEntity("Set lightmap texture", (ent) => {
                    if (ent.components.lightmap)
                      ent.components.lightmap.texture_id = val;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Intensity</label>
              <input
                type="range"
                min="0"
                max="3"
                step="0.05"
                value={entity.components.lightmap.intensity}
                oninput={(e) => {
                  editEntity("Set lightmap intensity", (ent) => {
                    if (ent.components.lightmap)
                      ent.components.lightmap.intensity = parseFloat(
                        (e.target as HTMLInputElement).value,
                      );
                  });
                }}
              />
              <span style="width: 30px; text-align: right;"
                >{entity.components.lightmap.intensity.toFixed(2)}</span
              >
            </div>
          </div>
          {@render undrawnFields("lightmap")}
        {/if}
      </div>
    {/if}

    {#if entity.components.shadow_caster}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          onclick={() => toggleSection("shadow_caster")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("shadow_caster");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["shadow_caster"] ? "▸" : "▾"}</span
          >
          Shadow Caster
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("shadow_caster");
            }}
            title="Remove">×</button
          >
        </div>
        {#if !collapsed["shadow_caster"]}
          <div class="section-body">
            <div class="field-row">
              <label>Enabled</label>
              <input
                type="checkbox"
                checked={entity.components.shadow_caster.enabled}
                onchange={(e) => {
                  const val = (e.target as HTMLInputElement).checked;
                  editEntity("Toggle shadow caster", (ent) => {
                    if (ent.components.shadow_caster)
                      ent.components.shadow_caster.enabled = val;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Color</label>
              <input
                type="color"
                value={`#${entity.components.shadow_caster.color
                  .slice(0, 3)
                  .map((c: number) =>
                    Math.round(c * 255)
                      .toString(16)
                      .padStart(2, "0"),
                  )
                  .join("")}`}
                onchange={(e) => {
                  const hex = (e.target as HTMLInputElement).value;
                  const r = parseInt(hex.slice(1, 3), 16) / 255;
                  const g = parseInt(hex.slice(3, 5), 16) / 255;
                  const b = parseInt(hex.slice(5, 7), 16) / 255;
                  const a = entity!.components.shadow_caster!.color[3];
                  editEntity("Set shadow color", (ent) => {
                    if (ent.components.shadow_caster)
                      ent.components.shadow_caster.color = [r, g, b, a];
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Opacity</label>
              <input
                type="range"
                min="0"
                max="1"
                step="0.05"
                value={entity.components.shadow_caster.color[3]}
                oninput={(e) => {
                  const a = parseFloat((e.target as HTMLInputElement).value);
                  const c = entity!.components.shadow_caster!.color;
                  editEntity("Set shadow opacity", (ent) => {
                    if (ent.components.shadow_caster)
                      ent.components.shadow_caster.color = [
                        c[0],
                        c[1],
                        c[2],
                        a,
                      ];
                  });
                }}
              />
              <span style="width: 30px; text-align: right;"
                >{entity.components.shadow_caster.color[3].toFixed(2)}</span
              >
            </div>
            <div class="field-row">
              <label>Offset X</label>
              <input
                type="number"
                step="0.5"
                value={entity.components.shadow_caster.offset[0]}
                onchange={(e) => {
                  const val = parseFloat((e.target as HTMLInputElement).value);
                  editEntity("Set shadow offset X", (ent) => {
                    if (ent.components.shadow_caster)
                      ent.components.shadow_caster.offset[0] = val;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Offset Y</label>
              <input
                type="number"
                step="0.5"
                value={entity.components.shadow_caster.offset[1]}
                onchange={(e) => {
                  const val = parseFloat((e.target as HTMLInputElement).value);
                  editEntity("Set shadow offset Y", (ent) => {
                    if (ent.components.shadow_caster)
                      ent.components.shadow_caster.offset[1] = val;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Height</label>
              <input
                type="number"
                step="0.1"
                min="0"
                value={entity.components.shadow_caster.height}
                onchange={(e) => {
                  const val = parseFloat((e.target as HTMLInputElement).value);
                  editEntity("Set shadow height", (ent) => {
                    if (ent.components.shadow_caster)
                      ent.components.shadow_caster.height = val;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Scale X</label>
              <input
                type="number"
                step="0.1"
                min="0"
                value={entity.components.shadow_caster.scale[0]}
                onchange={(e) => {
                  const val = parseFloat((e.target as HTMLInputElement).value);
                  editEntity("Set shadow scale X", (ent) => {
                    if (ent.components.shadow_caster)
                      ent.components.shadow_caster.scale[0] = val;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Scale Y</label>
              <input
                type="number"
                step="0.1"
                min="0"
                value={entity.components.shadow_caster.scale[1]}
                onchange={(e) => {
                  const val = parseFloat((e.target as HTMLInputElement).value);
                  editEntity("Set shadow scale Y", (ent) => {
                    if (ent.components.shadow_caster)
                      ent.components.shadow_caster.scale[1] = val;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Skew</label>
              <input
                type="number"
                step="0.05"
                value={entity.components.shadow_caster.skew}
                onchange={(e) => {
                  const val = parseFloat((e.target as HTMLInputElement).value);
                  editEntity("Set shadow skew", (ent) => {
                    if (ent.components.shadow_caster)
                      ent.components.shadow_caster.skew = val;
                  });
                }}
              />
            </div>
          </div>
        {/if}
      </div>
    {/if}

    {#if entity.components.vector_path}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          onclick={() => toggleSection("vector_path")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("vector_path");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["vector_path"] ? "▸" : "▾"}</span
          >
          Vector Path
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("vector_path");
            }}
            title="Remove">×</button
          >
        </div>
        {#if !collapsed["vector_path"]}
          <div class="section-body">
            <div class="field-row">
              <label>Fill</label>
              {#if entity.components.vector_path.fill_color}
                <input
                  type="color"
                  value={`#${entity.components.vector_path.fill_color
                    .slice(0, 3)
                    .map((c: number) =>
                      Math.round(c * 255)
                        .toString(16)
                        .padStart(2, "0"),
                    )
                    .join("")}`}
                  onchange={(e) => {
                    const hex = (e.target as HTMLInputElement).value;
                    const r = parseInt(hex.slice(1, 3), 16) / 255;
                    const g = parseInt(hex.slice(3, 5), 16) / 255;
                    const b = parseInt(hex.slice(5, 7), 16) / 255;
                    const a = entity!.components.vector_path!.fill_color![3];
                    editEntity("Set fill color", (ent) => {
                      if (ent.components.vector_path)
                        ent.components.vector_path.fill_color = [r, g, b, a];
                    });
                  }}
                />
                <button
                  class="remove-btn"
                  onclick={() => {
                    editEntity("Remove fill", (ent) => {
                      if (ent.components.vector_path)
                        ent.components.vector_path.fill_color = null;
                    });
                  }}
                  title="Remove fill">×</button
                >
              {:else}
                <button
                  class="add-btn"
                  onclick={() => {
                    editEntity("Add fill", (ent) => {
                      if (ent.components.vector_path)
                        ent.components.vector_path.fill_color = [1, 1, 1, 1];
                    });
                  }}>+ Fill</button
                >
              {/if}
            </div>
            {#if entity.components.vector_path.fill_color}
              <div class="field-row">
                <label>Fill Alpha</label>
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  value={entity.components.vector_path.fill_color[3]}
                  oninput={(e) => {
                    const a = parseFloat((e.target as HTMLInputElement).value);
                    const c = entity!.components.vector_path!.fill_color!;
                    editEntity("Set fill alpha", (ent) => {
                      if (ent.components.vector_path)
                        ent.components.vector_path.fill_color = [
                          c[0],
                          c[1],
                          c[2],
                          a,
                        ];
                    });
                  }}
                />
                <span style="width: 30px; text-align: right;"
                  >{entity.components.vector_path.fill_color[3].toFixed(
                    2,
                  )}</span
                >
              </div>
            {/if}
            <div class="field-row">
              <label>Stroke</label>
              {#if entity.components.vector_path.stroke_color}
                <input
                  type="color"
                  value={`#${entity.components.vector_path.stroke_color
                    .slice(0, 3)
                    .map((c: number) =>
                      Math.round(c * 255)
                        .toString(16)
                        .padStart(2, "0"),
                    )
                    .join("")}`}
                  onchange={(e) => {
                    const hex = (e.target as HTMLInputElement).value;
                    const r = parseInt(hex.slice(1, 3), 16) / 255;
                    const g = parseInt(hex.slice(3, 5), 16) / 255;
                    const b = parseInt(hex.slice(5, 7), 16) / 255;
                    const a = entity!.components.vector_path!.stroke_color![3];
                    editEntity("Set stroke color", (ent) => {
                      if (ent.components.vector_path)
                        ent.components.vector_path.stroke_color = [r, g, b, a];
                    });
                  }}
                />
                <button
                  class="remove-btn"
                  onclick={() => {
                    editEntity("Remove stroke", (ent) => {
                      if (ent.components.vector_path)
                        ent.components.vector_path.stroke_color = null;
                    });
                  }}
                  title="Remove stroke">×</button
                >
              {:else}
                <button
                  class="add-btn"
                  onclick={() => {
                    editEntity("Add stroke", (ent) => {
                      if (ent.components.vector_path)
                        ent.components.vector_path.stroke_color = [0, 0, 0, 1];
                    });
                  }}>+ Stroke</button
                >
              {/if}
            </div>
            {#if entity.components.vector_path.stroke_color}
              <div class="field-row">
                <label>Stroke Alpha</label>
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  value={entity.components.vector_path.stroke_color[3]}
                  oninput={(e) => {
                    const a = parseFloat((e.target as HTMLInputElement).value);
                    const c = entity!.components.vector_path!.stroke_color!;
                    editEntity("Set stroke alpha", (ent) => {
                      if (ent.components.vector_path)
                        ent.components.vector_path.stroke_color = [
                          c[0],
                          c[1],
                          c[2],
                          a,
                        ];
                    });
                  }}
                />
                <span style="width: 30px; text-align: right;"
                  >{entity.components.vector_path.stroke_color[3].toFixed(
                    2,
                  )}</span
                >
              </div>
              <div class="field-row">
                <label>Stroke Width</label>
                <input
                  type="number"
                  step="0.5"
                  min="0.1"
                  value={entity.components.vector_path.stroke_width}
                  onchange={(e) => {
                    const val = parseFloat(
                      (e.target as HTMLInputElement).value,
                    );
                    editEntity("Set stroke width", (ent) => {
                      if (ent.components.vector_path)
                        ent.components.vector_path.stroke_width = val || 1;
                    });
                  }}
                />
              </div>
            {/if}
            <div class="field-row">
              <label>Z Order</label>
              <input
                type="number"
                step="1"
                value={entity.components.vector_path.z_order}
                onchange={(e) => {
                  const val = parseInt(
                    (e.target as HTMLInputElement).value,
                    10,
                  );
                  editEntity("Set vector path z_order", (ent) => {
                    if (ent.components.vector_path)
                      ent.components.vector_path.z_order = val || 0;
                  });
                }}
              />
            </div>
            {@render sortLayerPicker(
              "vector_path",
              entity.components.vector_path.sort_layer,
              (v) =>
                editEntity("Change vector_path sort_layer", (ent) => {
                  if (ent.components.vector_path)
                    ent.components.vector_path.sort_layer = v;
                }),
            )}
            <div class="field-row">
              <label>Closed</label>
              <input
                type="checkbox"
                checked={entity.components.vector_path.closed}
                onchange={(e) => {
                  const val = (e.target as HTMLInputElement).checked;
                  editEntity("Toggle path closed", (ent) => {
                    if (ent.components.vector_path)
                      ent.components.vector_path.closed = val;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Points</label>
              <span
                >{entity.components.vector_path.commands.length} commands</span
              >
            </div>
            <button
              class="add-btn"
              style="width: 100%; margin-top: 4px;"
              onclick={() => {
                window.dispatchEvent(
                  new CustomEvent("open-vectorpath-editor", {
                    detail: {
                      entityId: $selectedEntity,
                      data: structuredClone(entity.components.vector_path),
                    },
                  }),
                );
              }}>✎ Edit Path</button
            >
          </div>
        {/if}
      </div>
    {/if}

    {#if entity.components.script}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("script")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("script");
          }}
        >
          <span class="collapse-arrow">{collapsed["script"] ? "▸" : "▾"}</span>
          {@render sectionBar("script")}<span class="section-title">Script</span>{@render overrideMark("script")}<span class="section-key">script</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("script");
            }}
            title="Remove Script">×</button
          >
        </div>
        {#if !collapsed["script"]}
          <div class="field-row">
            <label>Path</label>
            <div class="asset-picker-wrapper">
              <input
                type="text"
                value={entity.components.script.source_path}
                onchange={(e) =>
                  handleScriptChange(
                    "source_path",
                    (e.target as HTMLInputElement).value,
                  )}
                placeholder="scripts/script.lua"
              />
              <button
                class="picker-btn"
                onclick={() => (scriptPickerOpen = !scriptPickerOpen)}
                title="Browse scripts">⊞</button
              >
              {#if scriptPickerOpen && scriptFiles.length > 0}
                <button
                  class="picker-backdrop"
                  onclick={() => (scriptPickerOpen = false)}
                  aria-label="Close picker"
                ></button>
                <div class="picker-dropdown" role="listbox">
                  {#each scriptFiles as file}
                    <button
                      class="picker-item"
                      class:active={entity.components.script?.source_path ===
                        file}
                      role="option"
                      aria-selected={entity.components.script?.source_path ===
                        file}
                      onclick={() => {
                        const ext = file.split(".").pop()?.toLowerCase();
                        const lang = ext === "js" ? "JavaScript" : "Lua";
                        handleScriptChange("source_path", file);
                        handleScriptChange("language", lang);
                        scriptPickerOpen = false;
                      }}>{file}</button
                    >
                  {/each}
                </div>
              {:else if scriptPickerOpen}
                <button
                  class="picker-backdrop"
                  onclick={() => (scriptPickerOpen = false)}
                  aria-label="Close picker"
                ></button>
                <div class="picker-dropdown">
                  <span class="picker-empty">No script files in project</span>
                </div>
              {/if}
            </div>
            <button
              class="new-script-btn"
              onclick={handleNewScript}
              title="Create new script file">+ New</button
            >
          </div>
          <div class="field-row">
            <label>Lang</label>
            <select
              value={entity.components.script.language}
              onchange={(e) =>
                handleScriptChange(
                  "language",
                  (e.target as HTMLSelectElement).value,
                )}
            >
              <option value="Lua">Lua</option>
              <option value="JavaScript">JavaScript</option>
            </select>
          </div>
          {@render undrawnFields("script")}
        {/if}
      </div>
    {/if}

    {#if entity.components.behavior_graph}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("behavior_graph")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("behavior_graph");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["behavior_graph"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("behavior_graph")}<span class="section-title">Behavior Graph</span>{@render overrideMark("behavior_graph")}<span class="section-key">behavior_graph</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("behavior_graph");
            }}
            title="Remove Behavior Graph">×</button
          >
        </div>
        {#if !collapsed["behavior_graph"]}
          <div class="field-row">
            <label>Path</label>
            <div class="asset-picker-wrapper">
              <input
                type="text"
                value={entity.components.behavior_graph.graph_path}
                onchange={(e) =>
                  handleBehaviorChange(
                    "graph_path",
                    (e.target as HTMLInputElement).value,
                  )}
                placeholder="behaviors/player.behavior"
              />
              <button
                class="picker-btn"
                onclick={() => (behaviorPickerOpen = !behaviorPickerOpen)}
                title="Browse behavior graphs">⊞</button
              >
              {#if behaviorPickerOpen && behaviorFiles.length > 0}
                <button
                  class="picker-backdrop"
                  onclick={() => (behaviorPickerOpen = false)}
                  aria-label="Close picker"
                ></button>
                <div class="picker-dropdown" role="listbox">
                  {#each behaviorFiles as file}
                    <button
                      class="picker-item"
                      class:active={entity.components.behavior_graph
                        ?.graph_path === file}
                      role="option"
                      aria-selected={entity.components.behavior_graph
                        ?.graph_path === file}
                      onclick={() => {
                        handleBehaviorChange("graph_path", file);
                        behaviorPickerOpen = false;
                      }}>{file}</button
                    >
                  {/each}
                </div>
              {:else if behaviorPickerOpen}
                <button
                  class="picker-backdrop"
                  onclick={() => (behaviorPickerOpen = false)}
                  aria-label="Close picker"
                ></button>
                <div class="picker-dropdown">
                  <span class="picker-empty">No .behavior files in project</span
                  >
                </div>
              {/if}
            </div>
            <button
              class="new-script-btn"
              onclick={handleNewBehavior}
              title="Create new behavior graph">+ New</button
            >
          </div>
          <div class="field-row" style="align-items: flex-start;">
            <label style="padding-top: 4px;">Overrides</label>
            <textarea
              rows="4"
              style="flex: 1; font-family: var(--ed-mono); font-size: 11px;"
              placeholder={"{}"}
              value={JSON.stringify(
                entity.components.behavior_graph.overrides ?? {},
                null,
                2,
              )}
              onchange={(e) => {
                const raw = (e.target as HTMLTextAreaElement).value.trim();
                if (raw === "") {
                  handleBehaviorOverridesReplace({});
                  return;
                }
                try {
                  const parsed = JSON.parse(raw);
                  if (
                    parsed === null ||
                    typeof parsed !== "object" ||
                    Array.isArray(parsed)
                  ) {
                    logError("behavior_graph overrides must be a JSON object");
                    return;
                  }
                  handleBehaviorOverridesReplace(
                    parsed as Record<string, unknown>,
                  );
                } catch (err) {
                  logError(`Invalid overrides JSON: ${err}`);
                }
              }}
            ></textarea>
          </div>
        {/if}
      </div>
    {/if}

    {#if entity.components.text}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("text")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("text");
          }}
        >
          <span class="collapse-arrow">{collapsed["text"] ? "▸" : "▾"}</span>
          {@render sectionBar("text")}<span class="section-title">Text</span>{@render overrideMark("text")}<span class="section-key">text</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("text");
            }}
            title="Remove Text">×</button
          >
        </div>
        {@render depWarning("text")}
        {#if !collapsed["text"]}
          <div class="field-row">
            <label>Text</label>
            <input
              type="text"
              value={entity.components.text.text}
              onchange={(e) => {
                editEntity("Change text", (ent) => {
                  if (ent.components.text)
                    ent.components.text.text = (
                      e.target as HTMLInputElement
                    ).value;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Font</label>
            <select
              value={entity.components.text.font_id}
              onchange={(e) => {
                const v = (e.target as HTMLSelectElement).value;
                editEntity("Change text font", (ent) => {
                  if (ent.components.text) ent.components.text.font_id = v;
                });
              }}
            >
              <option value={isBuiltinFontId(entity.components.text.font_id) ? entity.components.text.font_id : ""}>(none - built-in font)</option>
              {#each $projectFontIds as id (id)}
                <option value={id}>{id}</option>
              {/each}
              {#if entity.components.text.font_id && !isBuiltinFontId(entity.components.text.font_id) && !$projectFontIds.includes(entity.components.text.font_id)}
                <option value={entity.components.text.font_id}>{entity.components.text.font_id} (missing)</option>
              {/if}
            </select>
          </div>
          <div class="field-row">
            <label>Scale</label>
            <input
              type="number"
              step="0.1"
              value={entity.components.text.font_scale}
              onchange={(e) => {
                editEntity("Change text scale", (ent) => {
                  if (ent.components.text)
                    ent.components.text.font_scale =
                      parseFloat((e.target as HTMLInputElement).value) || 1;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Color</label>
            <input
              type="color"
              value={colorToHex(entity.components.text.color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change text color", (ent) => {
                  if (ent.components.text) {
                    ent.components.text.color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      ent.components.text.color[3],
                    ];
                  }
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Z Order</label>
            <input
              type="number"
              step="1"
              value={entity.components.text.z_order}
              onchange={(e) => {
                editEntity("Change text z_order", (ent) => {
                  if (ent.components.text)
                    ent.components.text.z_order =
                      parseInt((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          {@render sortLayerPicker(
            "text",
            entity.components.text.sort_layer,
            (v) =>
              editEntity("Change text sort_layer", (ent) => {
                if (ent.components.text) ent.components.text.sort_layer = v;
              }),
          )}
          <div class="field-row">
            <label title="Read the text as markup: [color=#e05040] [b] [wave] [shake] [rainbow] [pause=0.5] [icon=sprites/key.png], each closed like [/color]. Off: brackets are just text.">Rich text</label>
            <input
              type="checkbox"
              checked={entity.components.text.rich ?? false}
              onchange={(e) => {
                editEntity("Change rich text", (ent) => {
                  if (!ent.components.text) return;
                  if ((e.target as HTMLInputElement).checked) ent.components.text.rich = true;
                  else delete ent.components.text.rich;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label title="A typewriter: characters a second, waiting at each [pause], from whenever the text changes. 0 shows it all at once.">Reveal speed</label>
            <input
              type="number"
              step="1"
              min="0"
              value={entity.components.text.reveal_speed ?? 0}
              onchange={(e) => {
                editEntity("Change text reveal speed", (ent) => {
                  if (!ent.components.text) return;
                  const v = Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0);
                  if (v > 0) ent.components.text.reveal_speed = v;
                  else delete ent.components.text.reveal_speed;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label title="An outline round every glyph, in world units (0: none). On the built-in font it reaches at most two of its pixels times its scale.">Outline</label>
            <input
              type="number"
              step="0.5"
              min="0"
              value={entity.components.text.outline?.width ?? 0}
              onchange={(e) => {
                editEntity("Change text outline", (ent) => {
                  const t = ent.components.text;
                  if (!t) return;
                  const w = Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0);
                  if (w > 0) t.outline = { width: w, color: t.outline?.color ?? [0, 0, 0, 1] };
                  else delete t.outline;
                });
              }}
            />
            {#if entity.components.text.outline}
              <input
                type="color"
                value={colorToHex(entity.components.text.outline.color)}
                onchange={(e) => {
                  const hex = (e.target as HTMLInputElement).value;
                  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
                  editEntity("Change text outline colour", (ent) => {
                    const o = ent.components.text?.outline;
                    if (o) o.color = [c[0], c[1], c[2], o.color[3]];
                  });
                }}
              />
            {/if}
          </div>
          <div class="field-row">
            <label title="A shadow under the text, moved by an offset (world units, y up) and softened.">Shadow</label>
            <input
              type="checkbox"
              checked={!!entity.components.text.shadow}
              onchange={(e) => {
                editEntity("Toggle text shadow", (ent) => {
                  const t = ent.components.text;
                  if (!t) return;
                  if ((e.target as HTMLInputElement).checked)
                    t.shadow = { offset: [2, -2], color: [0, 0, 0, 0.6], softness: 0 };
                  else delete t.shadow;
                });
              }}
            />
            {#if entity.components.text.shadow}
              <input
                type="number"
                step="1"
                title="Offset x"
                value={entity.components.text.shadow.offset[0]}
                onchange={(e) => {
                  editEntity("Change text shadow offset", (ent) => {
                    const s = ent.components.text?.shadow;
                    if (s) s.offset = [parseFloat((e.target as HTMLInputElement).value) || 0, s.offset[1]];
                  });
                }}
              />
              <input
                type="number"
                step="1"
                title="Offset y (up)"
                value={entity.components.text.shadow.offset[1]}
                onchange={(e) => {
                  editEntity("Change text shadow offset", (ent) => {
                    const s = ent.components.text?.shadow;
                    if (s) s.offset = [s.offset[0], parseFloat((e.target as HTMLInputElement).value) || 0];
                  });
                }}
              />
              <input
                type="number"
                step="0.5"
                min="0"
                title="Softness"
                value={entity.components.text.shadow.softness}
                onchange={(e) => {
                  editEntity("Change text shadow softness", (ent) => {
                    const s = ent.components.text?.shadow;
                    if (s) s.softness = Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0);
                  });
                }}
              />
              <input
                type="color"
                value={colorToHex(entity.components.text.shadow.color)}
                onchange={(e) => {
                  const hex = (e.target as HTMLInputElement).value;
                  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
                  editEntity("Change text shadow colour", (ent) => {
                    const s = ent.components.text?.shadow;
                    if (s) s.color = [c[0], c[1], c[2], s.color[3]];
                  });
                }}
              />
            {/if}
          </div>
          {@render undrawnFields("text")}
        {/if}
      </div>
    {/if}

    {#if entity.components.camera_follow}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("camera_follow")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("camera_follow");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["camera_follow"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("camera_follow")}<span class="section-title">Camera Follow</span>{@render overrideMark("camera_follow")}<span class="section-key">camera_follow</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("camera_follow");
            }}
            title="Remove Camera Follow">×</button
          >
        </div>
        {@render depWarning("camera_follow")}
        {#if !collapsed["camera_follow"]}
          <div class="field-row">
            <label>Smoothing</label>
            <input
              type="number"
              step="0.05"
              min="0"
              max="1"
              value={entity.components.camera_follow.smoothing}
              onchange={(e) => {
                editEntity("Change camera smoothing", (ent) => {
                  if (ent.components.camera_follow)
                    ent.components.camera_follow.smoothing =
                      parseFloat((e.target as HTMLInputElement).value) || 0.1;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Offset X</label>
            <input
              type="number"
              step="1"
              value={entity.components.camera_follow.offset[0]}
              onchange={(e) => {
                editEntity("Change camera offset", (ent) => {
                  if (ent.components.camera_follow)
                    ent.components.camera_follow.offset[0] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
            <label>Y</label>
            <input
              type="number"
              step="1"
              value={entity.components.camera_follow.offset[1]}
              onchange={(e) => {
                editEntity("Change camera offset", (ent) => {
                  if (ent.components.camera_follow)
                    ent.components.camera_follow.offset[1] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          {@render undrawnFields("camera_follow")}
        {/if}
      </div>
    {/if}

    {#if entity.components.camera}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("camera")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("camera");
          }}
        >
          <span class="collapse-arrow">{collapsed["camera"] ? "▸" : "▾"}</span>
          {@render sectionBar("camera")}<span class="section-title">Camera 2D</span>{@render overrideMark("camera")}<span class="section-key">camera</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("camera");
            }}
            title="Remove Camera">×</button
          >
        </div>
        {@render depWarning("camera")}
        {#if !collapsed["camera"]}
          <div class="field-row">
            <label>Active</label>
            <input
              type="checkbox"
              checked={entity.components.camera.is_active}
              onchange={(e) => {
                editEntity("Change camera active", (ent) => {
                  if (ent.components.camera)
                    ent.components.camera.is_active = (
                      e.target as HTMLInputElement
                    ).checked;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label title="Of the active cameras, the highest priority draws the screen (the first on a tie). Raise one from a script to hand it the view.">Priority</label>
            <input
              type="number"
              step="1"
              value={entity.components.camera.priority ?? 0}
              onchange={(e) => {
                editEntity("Change camera priority", (ent) => {
                  if (ent.components.camera) {
                    const v = Math.round(parseFloat((e.target as HTMLInputElement).value) || 0);
                    if (v !== 0) ent.components.camera.priority = v;
                    else delete ent.components.camera.priority;
                  }
                });
              }}
            />
            <label title="Seconds the view eases over to this camera when it takes the screen from another. 0 cuts.">Blend</label>
            <input
              type="number"
              step="0.1"
              min="0"
              value={entity.components.camera.blend ?? 0}
              onchange={(e) => {
                editEntity("Change camera blend", (ent) => {
                  if (ent.components.camera) {
                    const v = Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0);
                    if (v > 0) ent.components.camera.blend = v;
                    else delete ent.components.camera.blend;
                  }
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Zoom</label>
            <input
              type="number"
              step="0.1"
              min="0.05"
              max="20"
              value={entity.components.camera.zoom}
              onchange={(e) => {
                editEntity("Change camera zoom", (ent) => {
                  if (ent.components.camera)
                    ent.components.camera.zoom =
                      parseFloat((e.target as HTMLInputElement).value) || 1;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Viewport W</label>
            <input
              type="number"
              step="1"
              min="1"
              value={entity.components.camera.viewport_width}
              onchange={(e) => {
                editEntity("Change camera viewport width", (ent) => {
                  if (ent.components.camera)
                    ent.components.camera.viewport_width =
                      parseFloat((e.target as HTMLInputElement).value) || 1280;
                });
              }}
            />
            <label>H</label>
            <input
              type="number"
              step="1"
              min="1"
              value={entity.components.camera.viewport_height}
              onchange={(e) => {
                editEntity("Change camera viewport height", (ent) => {
                  if (ent.components.camera)
                    ent.components.camera.viewport_height =
                      parseFloat((e.target as HTMLInputElement).value) || 720;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label title="World units per art pixel: the world is drawn at the art's resolution and scaled up. 0 draws at the screen's.">Pixel size</label>
            <input
              type="number"
              step="1"
              min="0"
              value={entity.components.camera.pixel_size ?? 0}
              onchange={(e) => {
                editEntity("Change camera pixel size", (ent) => {
                  if (ent.components.camera)
                    ent.components.camera.pixel_size = Math.max(
                      0,
                      parseFloat((e.target as HTMLInputElement).value) || 0,
                    );
                });
              }}
            />
          </div>
          {#if (entity.components.camera.pixel_size ?? 0) > 0}
            <div class="field-row">
              <label title="Draw every art pixel as a whole number of screen pixels, showing a little more world to fill the screen. Off: the authored area fills the screen exactly and an art pixel may be 4.27 pixels wide.">Whole pixels</label>
              <input
                type="checkbox"
                checked={entity.components.camera.integer_scale ?? false}
                onchange={(e) => {
                  editEntity("Change camera integer scaling", (ent) => {
                    if (ent.components.camera) {
                      if ((e.target as HTMLInputElement).checked) ent.components.camera.integer_scale = true;
                      else delete ent.components.camera.integer_scale;
                    }
                  });
                }}
              />
            </div>
          {/if}
          <div class="field-row">
            <label title="Draw into a texture of this name instead of the screen - a view any sprite or UI image can show by naming it. Empty is the main camera.">Target</label>
            <input
              type="text"
              placeholder="(the screen)"
              value={entity.components.camera.target ?? ""}
              onchange={(e) => {
                editEntity("Change camera target", (ent) => {
                  if (ent.components.camera)
                    ent.components.camera.target = (e.target as HTMLInputElement).value.trim();
                });
              }}
            />
          </div>
          <div class="field-row">
            <label title="Which render layers this camera draws: tick a layer to draw the entities whose render_layers include it. Every layer by default.">Layers</label>
            <span class="layer-bits">
              {#each [0, 1, 2, 3, 4, 5, 6, 7] as bit}
                <label class="layer-bit" title={`layer ${bit + 1}`}>
                  <input
                    type="checkbox"
                    checked={((entity.components.camera.cull_mask ?? 0xffffffff) >>> bit) & 1 ? true : false}
                    onchange={(e) => {
                      editEntity("Change camera layers", (ent) => {
                        if (ent.components.camera) {
                          const on = (e.target as HTMLInputElement).checked;
                          const mask = (ent.components.camera.cull_mask ?? 0xffffffff) >>> 0;
                          ent.components.camera.cull_mask = (on ? mask | (1 << bit) : mask & ~(1 << bit)) >>> 0;
                        }
                      });
                    }}
                  />{bit + 1}
                </label>
              {/each}
            </span>
          </div>
          {#if entity.components.camera.target}
            <div class="field-row">
              <label title="The target texture's size in pixels; 0 takes the screen's.">Target W</label>
              <input
                type="number"
                step="1"
                min="0"
                value={entity.components.camera.target_size?.[0] ?? 0}
                onchange={(e) => {
                  editEntity("Change camera target width", (ent) => {
                    if (ent.components.camera) {
                      const size = ent.components.camera.target_size ?? [0, 0];
                      ent.components.camera.target_size = [
                        Math.max(0, Math.round(parseFloat((e.target as HTMLInputElement).value) || 0)),
                        size[1],
                      ];
                    }
                  });
                }}
              />
              <label>H</label>
              <input
                type="number"
                step="1"
                min="0"
                value={entity.components.camera.target_size?.[1] ?? 0}
                onchange={(e) => {
                  editEntity("Change camera target height", (ent) => {
                    if (ent.components.camera) {
                      const size = ent.components.camera.target_size ?? [0, 0];
                      ent.components.camera.target_size = [
                        size[0],
                        Math.max(0, Math.round(parseFloat((e.target as HTMLInputElement).value) || 0)),
                      ];
                    }
                  });
                }}
              />
            </div>
          {/if}
          <div class="field-row">
            <label
              title="Draw onto this part of the screen - x, y, w, h as fractions from the top-left, y down - for split screen. All 0 is the whole screen."
              >Screen Rect</label
            >
            {#each [0, 1, 2, 3] as i (i)}
              <input
                type="number"
                step="0.05"
                min="0"
                max="1"
                style="width: 3.6em"
                value={entity.components.camera.screen_rect?.[i] ?? 0}
                onchange={(e) => {
                  editEntity("Change camera screen rect", (ent) => {
                    if (ent.components.camera) {
                      const r: [number, number, number, number] = [...(ent.components.camera.screen_rect ?? [0, 0, 0, 0])] as [
                        number,
                        number,
                        number,
                        number,
                      ];
                      r[i] = Math.min(1, Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0));
                      ent.components.camera.screen_rect = r;
                    }
                  });
                }}
              />
            {/each}
          </div>
          {@render undrawnFields("camera")}
        {/if}
      </div>
    {/if}

    <!-- Rigid Body 2D -->
    {#if entity.components.rigid_body}
      {@const rb = entity.components.rigid_body}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("rigid_body")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("rigid_body");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["rigid_body"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("rigid_body")}<span class="section-title">Rigid Body 2D</span>{@render overrideMark("rigid_body")}<span class="section-key">rigid_body</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("rigid_body");
            }}
            title="Remove Rigid Body">×</button
          >
        </div>
        {#if !collapsed["rigid_body"]}
          <div class="field-row">
            <label>Type</label>
            <select
              value={rb.body_type}
              onchange={(e) => {
                const val = (e.target as HTMLSelectElement).value;
                editEntity("Change body type", (ent) => {
                  if (ent.components.rigid_body)
                    ent.components.rigid_body.body_type = val as
                      | "Dynamic"
                      | "Static"
                      | "Kinematic";
                });
              }}
            >
              <option value="Dynamic">Dynamic</option>
              <option value="Static">Static</option>
              <option value="Kinematic">Kinematic</option>
            </select>
          </div>
          <div class="field-row">
            <label>Mass</label>
            <input
              type="number"
              step="0.1"
              min="0"
              value={rb.mass}
              onchange={(e) => {
                editEntity("Change mass", (ent) => {
                  if (ent.components.rigid_body)
                    ent.components.rigid_body.mass =
                      parseFloat((e.target as HTMLInputElement).value) || 1;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Gravity</label>
            <input
              type="number"
              step="0.1"
              value={rb.gravity_scale}
              onchange={(e) => {
                editEntity("Change gravity scale", (ent) => {
                  if (ent.components.rigid_body)
                    ent.components.rigid_body.gravity_scale =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Lin Damp</label>
            <input
              type="number"
              step="0.1"
              min="0"
              value={rb.linear_damping}
              onchange={(e) => {
                editEntity("Change linear damping", (ent) => {
                  if (ent.components.rigid_body)
                    ent.components.rigid_body.linear_damping =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Ang Damp</label>
            <input
              type="number"
              step="0.1"
              min="0"
              value={rb.angular_damping}
              onchange={(e) => {
                editEntity("Change angular damping", (ent) => {
                  if (ent.components.rigid_body)
                    ent.components.rigid_body.angular_damping =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Fixed Rot</label>
            <input
              type="checkbox"
              checked={rb.fixed_rotation}
              onchange={(e) => {
                editEntity("Toggle fixed rotation", (ent) => {
                  if (ent.components.rigid_body)
                    ent.components.rigid_body.fixed_rotation = (
                      e.target as HTMLInputElement
                    ).checked;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label
              title="Sweep the shape along its path instead of only testing where it landed. Needed for anything that crosses more than its own thickness in a frame - a pinball, a bullet. Not free; leave it off for crates and characters."
              >CCD</label
            >
            <input
              type="checkbox"
              checked={rb.ccd ?? false}
              onchange={(e) => {
                editEntity("Toggle CCD", (ent) => {
                  if (ent.components.rigid_body)
                    ent.components.rigid_body.ccd = (
                      e.target as HTMLInputElement
                    ).checked;
                });
              }}
            />
          </div>
        {/if}
      </div>
    {/if}

    <!-- Collider 2D -->
    {#if entity.components.collider}
      {@const col = entity.components.collider}
      {@const shapeType = getShapeType(col.shape as Record<string, unknown>)}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("collider")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("collider");
          }}
        >
          <span class="collapse-arrow">{collapsed["collider"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("collider")}<span class="section-title">Collider 2D</span>{@render overrideMark("collider")}<span class="section-key">collider</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("collider");
            }}
            title="Remove Collider">×</button
          >
        </div>
        {@render depWarning("collider")}
        {#if !collapsed["collider"]}
          <div class="field-row">
            <label>Shape</label>
            <select
              value={shapeType}
              onchange={(e) => {
                const val = (e.target as HTMLSelectElement).value;
                editEntity("Change collider shape", (ent) => {
                  if (!ent.components.collider) return;
                  if (val === "Box")
                    ent.components.collider.shape = {
                      Box: { width: 1, height: 1 },
                    };
                  else if (val === "Circle")
                    ent.components.collider.shape = { Circle: { radius: 0.5 } };
                  else if (val === "Capsule")
                    ent.components.collider.shape = {
                      Capsule: { half_height: 0.5, radius: 0.25 },
                    };
                  else if (val === "Segment")
                    ent.components.collider.shape = {
                      Segment: { a: [-0.5, 0], b: [0.5, 0] },
                    };
                  else if (val === "Triangle")
                    ent.components.collider.shape = {
                      Triangle: {
                        a: [0, 0.5],
                        b: [-0.5, -0.5],
                        c: [0.5, -0.5],
                      },
                    };
                  else if (val === "ConvexPolygon")
                    ent.components.collider.shape = {
                      ConvexPolygon: {
                        vertices: [
                          [-0.5, -0.5],
                          [0.5, -0.5],
                          [0.5, 0.5],
                          [-0.5, 0.5],
                        ],
                      },
                    };
                  else if (val === "Polyline")
                    ent.components.collider.shape = {
                      Polyline: {
                        vertices: [
                          [-0.5, 0],
                          [0, 0.5],
                          [0.5, 0],
                        ],
                      },
                    };
                  else if (val === "HeightField")
                    ent.components.collider.shape = {
                      HeightField: {
                        heights: [0, 0.5, 0.2, 0.8, 0],
                        scale: [4, 1],
                      },
                    };
                  else if (val === "ConvexDecomposition")
                    ent.components.collider.shape = {
                      ConvexDecomposition: {
                        vertices: [
                          [-0.5, -0.5],
                          [0.5, -0.5],
                          [0.5, 0.5],
                          [0, 0],
                          [-0.5, 0.5],
                        ],
                      },
                    };
                });
              }}
            >
              <option value="Box">Box</option>
              <option value="Circle">Circle</option>
              <option value="Capsule">Capsule</option>
              <option value="Segment">Segment</option>
              <option value="Triangle">Triangle</option>
              <option value="ConvexPolygon">Convex Polygon</option>
              <option value="Polyline">Polyline</option>
              <option value="HeightField">Height Field</option>
              <option value="ConvexDecomposition">Convex Decomposition</option>
            </select>
          </div>
          {#if shapeType === "Box"}
            {@const box = (
              col.shape as { Box: { width: number; height: number } }
            ).Box}
            <div class="field-row">
              <label>Size</label>
              <label class="axis">W</label>
              <input
                type="number"
                step="0.1"
                min="0"
                value={box.width}
                onchange={(e) => {
                  editEntity("Change box width", (ent) => {
                    if (
                      ent.components.collider &&
                      "Box" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Box: { width: number; height: number };
                        }
                      ).Box.width =
                        parseFloat((e.target as HTMLInputElement).value) || 1;
                  });
                }}
              />
              <label>H</label>
              <input
                type="number"
                step="0.1"
                min="0"
                value={box.height}
                onchange={(e) => {
                  editEntity("Change box height", (ent) => {
                    if (
                      ent.components.collider &&
                      "Box" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Box: { width: number; height: number };
                        }
                      ).Box.height =
                        parseFloat((e.target as HTMLInputElement).value) || 1;
                  });
                }}
              />
            </div>
          {:else if shapeType === "Circle"}
            {@const circle = (col.shape as { Circle: { radius: number } })
              .Circle}
            <div class="field-row">
              <label>Radius</label>
              <input
                type="number"
                step="0.1"
                min="0"
                value={circle.radius}
                onchange={(e) => {
                  editEntity("Change circle radius", (ent) => {
                    if (
                      ent.components.collider &&
                      "Circle" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Circle: { radius: number };
                        }
                      ).Circle.radius =
                        parseFloat((e.target as HTMLInputElement).value) || 0.5;
                  });
                }}
              />
            </div>
          {:else if shapeType === "Capsule"}
            {@const capsule = (
              col.shape as { Capsule: { half_height: number; radius: number } }
            ).Capsule}
            <div class="field-row">
              <label>Half H</label>
              <input
                type="number"
                step="0.1"
                min="0"
                value={capsule.half_height}
                onchange={(e) => {
                  editEntity("Change capsule half height", (ent) => {
                    if (
                      ent.components.collider &&
                      "Capsule" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Capsule: { half_height: number; radius: number };
                        }
                      ).Capsule.half_height =
                        parseFloat((e.target as HTMLInputElement).value) || 0.5;
                  });
                }}
              />
              <label>R</label>
              <input
                type="number"
                step="0.1"
                min="0"
                value={capsule.radius}
                onchange={(e) => {
                  editEntity("Change capsule radius", (ent) => {
                    if (
                      ent.components.collider &&
                      "Capsule" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Capsule: { half_height: number; radius: number };
                        }
                      ).Capsule.radius =
                        parseFloat((e.target as HTMLInputElement).value) ||
                        0.25;
                  });
                }}
              />
            </div>
          {:else if shapeType === "Segment"}
            {@const seg = (
              col.shape as {
                Segment: { a: [number, number]; b: [number, number] };
              }
            ).Segment}
            <div class="field-row">
              <label>A X</label>
              <input
                type="number"
                step="0.1"
                value={seg.a[0]}
                onchange={(e) => {
                  editEntity("Change segment A", (ent) => {
                    if (
                      ent.components.collider &&
                      "Segment" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Segment: { a: [number, number]; b: [number, number] };
                        }
                      ).Segment.a[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <label>Y</label>
              <input
                type="number"
                step="0.1"
                value={seg.a[1]}
                onchange={(e) => {
                  editEntity("Change segment A", (ent) => {
                    if (
                      ent.components.collider &&
                      "Segment" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Segment: { a: [number, number]; b: [number, number] };
                        }
                      ).Segment.a[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>B X</label>
              <input
                type="number"
                step="0.1"
                value={seg.b[0]}
                onchange={(e) => {
                  editEntity("Change segment B", (ent) => {
                    if (
                      ent.components.collider &&
                      "Segment" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Segment: { a: [number, number]; b: [number, number] };
                        }
                      ).Segment.b[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <label>Y</label>
              <input
                type="number"
                step="0.1"
                value={seg.b[1]}
                onchange={(e) => {
                  editEntity("Change segment B", (ent) => {
                    if (
                      ent.components.collider &&
                      "Segment" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Segment: { a: [number, number]; b: [number, number] };
                        }
                      ).Segment.b[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
          {:else if shapeType === "Triangle"}
            {@const tri = (
              col.shape as {
                Triangle: {
                  a: [number, number];
                  b: [number, number];
                  c: [number, number];
                };
              }
            ).Triangle}
            <div class="field-row">
              <label>A</label>
              <input
                type="number"
                step="0.1"
                value={tri.a[0]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change triangle A", (ent) => {
                    if (
                      ent.components.collider &&
                      "Triangle" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Triangle: {
                            a: [number, number];
                            b: [number, number];
                            c: [number, number];
                          };
                        }
                      ).Triangle.a[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <input
                type="number"
                step="0.1"
                value={tri.a[1]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change triangle A", (ent) => {
                    if (
                      ent.components.collider &&
                      "Triangle" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Triangle: {
                            a: [number, number];
                            b: [number, number];
                            c: [number, number];
                          };
                        }
                      ).Triangle.a[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>B</label>
              <input
                type="number"
                step="0.1"
                value={tri.b[0]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change triangle B", (ent) => {
                    if (
                      ent.components.collider &&
                      "Triangle" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Triangle: {
                            a: [number, number];
                            b: [number, number];
                            c: [number, number];
                          };
                        }
                      ).Triangle.b[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <input
                type="number"
                step="0.1"
                value={tri.b[1]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change triangle B", (ent) => {
                    if (
                      ent.components.collider &&
                      "Triangle" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Triangle: {
                            a: [number, number];
                            b: [number, number];
                            c: [number, number];
                          };
                        }
                      ).Triangle.b[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>C</label>
              <input
                type="number"
                step="0.1"
                value={tri.c[0]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change triangle C", (ent) => {
                    if (
                      ent.components.collider &&
                      "Triangle" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Triangle: {
                            a: [number, number];
                            b: [number, number];
                            c: [number, number];
                          };
                        }
                      ).Triangle.c[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <input
                type="number"
                step="0.1"
                value={tri.c[1]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change triangle C", (ent) => {
                    if (
                      ent.components.collider &&
                      "Triangle" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          Triangle: {
                            a: [number, number];
                            b: [number, number];
                            c: [number, number];
                          };
                        }
                      ).Triangle.c[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
          {:else if shapeType === "ConvexPolygon" || shapeType === "Polyline" || shapeType === "ConvexDecomposition"}
            {@const key = shapeType as
              | "ConvexPolygon"
              | "Polyline"
              | "ConvexDecomposition"}
            {@const verts =
              (col.shape as Record<string, { vertices: [number, number][] }>)[
                key
              ]?.vertices ?? []}
            <div class="field-row">
              <label>Vertices</label>
              <span style="opacity:0.6">{verts.length} points</span>
            </div>
            {#each verts as v, vi}
              <div class="field-row" style="padding-left:8px">
                <label>#{vi}</label>
                <input
                  type="number"
                  step="0.1"
                  value={v[0]}
                  style="width:50px"
                  onchange={(e) => {
                    editEntity(`Change vertex ${vi}`, (ent) => {
                      if (
                        ent.components.collider &&
                        key in ent.components.collider.shape
                      )
                        (
                          ent.components.collider.shape as Record<
                            string,
                            { vertices: [number, number][] }
                          >
                        )[key].vertices[vi][0] =
                          parseFloat((e.target as HTMLInputElement).value) || 0;
                    });
                  }}
                />
                <input
                  type="number"
                  step="0.1"
                  value={v[1]}
                  style="width:50px"
                  onchange={(e) => {
                    editEntity(`Change vertex ${vi}`, (ent) => {
                      if (
                        ent.components.collider &&
                        key in ent.components.collider.shape
                      )
                        (
                          ent.components.collider.shape as Record<
                            string,
                            { vertices: [number, number][] }
                          >
                        )[key].vertices[vi][1] =
                          parseFloat((e.target as HTMLInputElement).value) || 0;
                    });
                  }}
                />
                <button
                  class="remove-btn"
                  title="Remove vertex"
                  onclick={() => {
                    editEntity(`Remove vertex ${vi}`, (ent) => {
                      if (
                        ent.components.collider &&
                        key in ent.components.collider.shape
                      )
                        (
                          ent.components.collider.shape as Record<
                            string,
                            { vertices: [number, number][] }
                          >
                        )[key].vertices.splice(vi, 1);
                    });
                  }}>×</button
                >
              </div>
            {/each}
            <div class="field-row">
              <button
                class="add-btn"
                onclick={() => {
                  editEntity("Add vertex", (ent) => {
                    if (
                      ent.components.collider &&
                      key in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as Record<
                          string,
                          { vertices: [number, number][] }
                        >
                      )[key].vertices.push([0, 0]);
                  });
                }}>+ Add Vertex</button
              >
              <button
                class="add-btn"
                onclick={() => {
                  polygonEditPath.set(null);
                  polygonEditEntity.set($selectedEntity);
                }}>Edit in Viewport</button
              >
            </div>
          {:else if shapeType === "HeightField"}
            {@const hf = (
              col.shape as {
                HeightField: { heights: number[]; scale: [number, number] };
              }
            ).HeightField}
            <div class="field-row">
              <label>Scale X</label>
              <input
                type="number"
                step="0.1"
                value={hf.scale[0]}
                onchange={(e) => {
                  editEntity("Change heightfield scale", (ent) => {
                    if (
                      ent.components.collider &&
                      "HeightField" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          HeightField: {
                            heights: number[];
                            scale: [number, number];
                          };
                        }
                      ).HeightField.scale[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 1;
                  });
                }}
              />
              <label>Y</label>
              <input
                type="number"
                step="0.1"
                value={hf.scale[1]}
                onchange={(e) => {
                  editEntity("Change heightfield scale", (ent) => {
                    if (
                      ent.components.collider &&
                      "HeightField" in ent.components.collider.shape
                    )
                      (
                        ent.components.collider.shape as {
                          HeightField: {
                            heights: number[];
                            scale: [number, number];
                          };
                        }
                      ).HeightField.scale[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 1;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Samples</label>
              <span style="opacity:0.6">{hf.heights.length} heights</span>
            </div>
          {/if}
          <div class="field-row">
            <label>Offset X</label>
            <input
              type="number"
              step="0.1"
              value={col.offset[0]}
              onchange={(e) => {
                editEntity("Change collider offset", (ent) => {
                  if (ent.components.collider)
                    ent.components.collider.offset[0] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
            <label>Y</label>
            <input
              type="number"
              step="0.1"
              value={col.offset[1]}
              onchange={(e) => {
                editEntity("Change collider offset", (ent) => {
                  if (ent.components.collider)
                    ent.components.collider.offset[1] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Sensor</label>
            <input
              type="checkbox"
              checked={col.is_sensor}
              onchange={(e) => {
                editEntity("Toggle sensor", (ent) => {
                  if (ent.components.collider)
                    ent.components.collider.is_sensor = (
                      e.target as HTMLInputElement
                    ).checked;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Friction</label>
            <input
              type="number"
              step="0.1"
              min="0"
              max="1"
              value={col.friction}
              onchange={(e) => {
                editEntity("Change friction", (ent) => {
                  if (ent.components.collider)
                    ent.components.collider.friction =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Bounce</label>
            <input
              type="number"
              step="0.1"
              min="0"
              max="1"
              value={col.restitution}
              onchange={(e) => {
                editEntity("Change restitution", (ent) => {
                  if (ent.components.collider)
                    ent.components.collider.restitution =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Density</label>
            <input
              type="number"
              step="0.1"
              min="0"
              value={col.density}
              onchange={(e) => {
                editEntity("Change density", (ent) => {
                  if (ent.components.collider)
                    ent.components.collider.density =
                      parseFloat((e.target as HTMLInputElement).value) || 1;
                });
              }}
            />
          </div>
          <!-- Collision Layers -->
          {@const layerNames = $project?.physics?.collision_layer_names ?? []}
          <div
            class="field-row"
            style="flex-direction:column;align-items:flex-start;gap:4px"
          >
            <label style="margin-bottom:2px">Membership</label>
            <div
              style="display:grid;grid-template-columns:repeat(4,1fr);gap:2px;width:100%"
            >
              {#each Array(16) as _, li}
                {@const bit = 1 << li}
                {@const active =
                  ((col.collision_membership ?? 0xffff) & bit) !== 0}
                <button
                  class="layer-toggle"
                  class:active
                  title={layerNames[li] || `Layer ${li + 1}`}
                  onclick={() => {
                    editEntity("Toggle collision membership", (ent) => {
                      if (ent.components.collider)
                        ent.components.collider.collision_membership =
                          (ent.components.collider.collision_membership ??
                            0xffff) ^ bit;
                    });
                  }}>{li + 1}</button
                >
              {/each}
            </div>
          </div>
          <div
            class="field-row"
            style="flex-direction:column;align-items:flex-start;gap:4px"
          >
            <label style="margin-bottom:2px">Filter</label>
            <div
              style="display:grid;grid-template-columns:repeat(4,1fr);gap:2px;width:100%"
            >
              {#each Array(16) as _, li}
                {@const bit = 1 << li}
                {@const active = ((col.collision_filter ?? 0xffff) & bit) !== 0}
                <button
                  class="layer-toggle"
                  class:active
                  title={layerNames[li] || `Layer ${li + 1}`}
                  onclick={() => {
                    editEntity("Toggle collision filter", (ent) => {
                      if (ent.components.collider)
                        ent.components.collider.collision_filter =
                          (ent.components.collider.collision_filter ?? 0xffff) ^
                          bit;
                    });
                  }}>{li + 1}</button
                >
              {/each}
            </div>
          </div>
          {@render undrawnFields("collider")}
        {/if}
      </div>
    {/if}

    <!-- Joint 2D -->
    {#if entity.components.joint}
      {@const jnt = entity.components.joint}
      {@const jtype = (() => {
        const jt = jnt.joint_type as Record<string, unknown>;
        if ("Revolute" in jt) return "Revolute";
        if ("Fixed" in jt) return "Fixed";
        if ("Prismatic" in jt) return "Prismatic";
        if ("Rope" in jt) return "Rope";
        if ("Spring" in jt) return "Spring";
        for (const k of Object.keys(GENERIC_JOINTS)) if (k in jt) return k;
        return "Revolute";
      })()}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("joint")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("joint");
          }}
        >
          <span class="collapse-arrow">{collapsed["joint"] ? "▸" : "▾"}</span>
          {@render sectionBar("joint")}<span class="section-title">Joint 2D</span>{@render overrideMark("joint")}<span class="section-key">joint</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("joint");
            }}
            title="Remove Joint">×</button
          >
        </div>
        {@render depWarning("joint")}
        {#if !collapsed["joint"]}
          <div class="field-row">
            <label>Target</label>
            <select
              value={jnt.target_entity}
              onchange={(e) => {
                editEntity("Change joint target", (ent) => {
                  if (ent.components.joint)
                    ent.components.joint.target_entity = (
                      e.target as HTMLSelectElement
                    ).value;
                });
              }}
            >
              <option value="">-- Select --</option>
              {#each $scene.entities as ent, ei}
                {#if ent.name !== entity.name && (ent.components.rigid_body || jtype === "Target")}
                  <option value={ent.name}>{ent.name}</option>
                {/if}
              {/each}
            </select>
          </div>
          <div class="field-row">
            <label>Type</label>
            <select
              value={jtype}
              onchange={(e) => {
                const val = (e.target as HTMLSelectElement).value;
                editEntity("Change joint type", (ent) => {
                  if (!ent.components.joint) return;
                  if (val === "Revolute")
                    ent.components.joint.joint_type = {
                      Revolute: {
                        anchor: [0, 0],
                        motor_speed: 0,
                        motor_max_torque: 0,
                      },
                    };
                  else if (val === "Fixed")
                    ent.components.joint.joint_type = {
                      Fixed: { anchor_a: [0, 0], anchor_b: [0, 0] },
                    };
                  else if (val === "Prismatic")
                    ent.components.joint.joint_type = {
                      Prismatic: {
                        anchor: [0, 0],
                        axis: [1, 0],
                        motor_speed: 0,
                        motor_max_force: 0,
                      },
                    };
                  else if (val === "Rope")
                    ent.components.joint.joint_type = {
                      Rope: {
                        anchor_a: [0, 0],
                        anchor_b: [0, 0],
                        max_distance: 2,
                      },
                    };
                  else if (val in GENERIC_JOINTS)
                    ent.components.joint.joint_type = {
                      [val]: structuredClone(GENERIC_JOINTS[val]),
                    } as never;
                  else if (val === "Spring")
                    ent.components.joint.joint_type = {
                      Spring: {
                        anchor_a: [0, 0],
                        anchor_b: [0, 0],
                        rest_length: 1,
                        stiffness: 10,
                        damping: 0.5,
                      },
                    };
                });
              }}
            >
              <option value="Revolute">Revolute</option>
              <option value="Fixed">Fixed</option>
              <option value="Prismatic">Prismatic</option>
              <option value="Rope">Rope</option>
              <option value="Spring">Spring</option>
              <option value="Wheel">Wheel</option>
              <option value="Motor">Motor</option>
              <option value="Friction">Friction</option>
              <option value="Target">Target</option>
            </select>
          </div>
          {#if jtype === "Revolute"}
            {@const rev = (
              jnt.joint_type as {
                Revolute: {
                  anchor: [number, number];
                  limits?: [number, number];
                  motor_speed: number;
                  motor_max_torque: number;
                };
              }
            ).Revolute}
            <div class="field-row">
              <label>Anchor X</label>
              <input
                type="number"
                step="0.1"
                value={rev.anchor[0]}
                onchange={(e) => {
                  editEntity("Change joint anchor", (ent) => {
                    if (
                      ent.components.joint &&
                      "Revolute" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Revolute: { anchor: [number, number] };
                        }
                      ).Revolute.anchor[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <label>Y</label>
              <input
                type="number"
                step="0.1"
                value={rev.anchor[1]}
                onchange={(e) => {
                  editEntity("Change joint anchor", (ent) => {
                    if (
                      ent.components.joint &&
                      "Revolute" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Revolute: { anchor: [number, number] };
                        }
                      ).Revolute.anchor[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Motor Spd</label>
              <input
                type="number"
                step="0.1"
                value={rev.motor_speed}
                onchange={(e) => {
                  editEntity("Change motor speed", (ent) => {
                    if (
                      ent.components.joint &&
                      "Revolute" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Revolute: { motor_speed: number };
                        }
                      ).Revolute.motor_speed =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Max Torque</label>
              <input
                type="number"
                step="0.1"
                value={rev.motor_max_torque}
                onchange={(e) => {
                  editEntity("Change motor torque", (ent) => {
                    if (
                      ent.components.joint &&
                      "Revolute" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Revolute: { motor_max_torque: number };
                        }
                      ).Revolute.motor_max_torque =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
          {:else if jtype === "Fixed"}
            {@const fix = (
              jnt.joint_type as {
                Fixed: {
                  anchor_a: [number, number];
                  anchor_b: [number, number];
                };
              }
            ).Fixed}
            <div class="field-row">
              <label>Anchor A</label>
              <input
                type="number"
                step="0.1"
                value={fix.anchor_a[0]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change fixed anchor A", (ent) => {
                    if (
                      ent.components.joint &&
                      "Fixed" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Fixed: { anchor_a: [number, number] };
                        }
                      ).Fixed.anchor_a[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <input
                type="number"
                step="0.1"
                value={fix.anchor_a[1]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change fixed anchor A", (ent) => {
                    if (
                      ent.components.joint &&
                      "Fixed" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Fixed: { anchor_a: [number, number] };
                        }
                      ).Fixed.anchor_a[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Anchor B</label>
              <input
                type="number"
                step="0.1"
                value={fix.anchor_b[0]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change fixed anchor B", (ent) => {
                    if (
                      ent.components.joint &&
                      "Fixed" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Fixed: { anchor_b: [number, number] };
                        }
                      ).Fixed.anchor_b[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <input
                type="number"
                step="0.1"
                value={fix.anchor_b[1]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change fixed anchor B", (ent) => {
                    if (
                      ent.components.joint &&
                      "Fixed" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Fixed: { anchor_b: [number, number] };
                        }
                      ).Fixed.anchor_b[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
          {:else if jtype === "Prismatic"}
            {@const pris = (
              jnt.joint_type as {
                Prismatic: {
                  anchor: [number, number];
                  axis: [number, number];
                  motor_speed: number;
                  motor_max_force: number;
                };
              }
            ).Prismatic}
            <div class="field-row">
              <label>Anchor X</label>
              <input
                type="number"
                step="0.1"
                value={pris.anchor[0]}
                onchange={(e) => {
                  editEntity("Change prismatic anchor", (ent) => {
                    if (
                      ent.components.joint &&
                      "Prismatic" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Prismatic: { anchor: [number, number] };
                        }
                      ).Prismatic.anchor[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <label>Y</label>
              <input
                type="number"
                step="0.1"
                value={pris.anchor[1]}
                onchange={(e) => {
                  editEntity("Change prismatic anchor", (ent) => {
                    if (
                      ent.components.joint &&
                      "Prismatic" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Prismatic: { anchor: [number, number] };
                        }
                      ).Prismatic.anchor[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Axis X</label>
              <input
                type="number"
                step="0.1"
                value={pris.axis[0]}
                onchange={(e) => {
                  editEntity("Change prismatic axis", (ent) => {
                    if (
                      ent.components.joint &&
                      "Prismatic" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Prismatic: { axis: [number, number] };
                        }
                      ).Prismatic.axis[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <label>Y</label>
              <input
                type="number"
                step="0.1"
                value={pris.axis[1]}
                onchange={(e) => {
                  editEntity("Change prismatic axis", (ent) => {
                    if (
                      ent.components.joint &&
                      "Prismatic" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Prismatic: { axis: [number, number] };
                        }
                      ).Prismatic.axis[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Motor Spd</label>
              <input
                type="number"
                step="0.1"
                value={pris.motor_speed}
                onchange={(e) => {
                  editEntity("Change motor speed", (ent) => {
                    if (
                      ent.components.joint &&
                      "Prismatic" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Prismatic: { motor_speed: number };
                        }
                      ).Prismatic.motor_speed =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Max Force</label>
              <input
                type="number"
                step="0.1"
                value={pris.motor_max_force}
                onchange={(e) => {
                  editEntity("Change motor force", (ent) => {
                    if (
                      ent.components.joint &&
                      "Prismatic" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Prismatic: { motor_max_force: number };
                        }
                      ).Prismatic.motor_max_force =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
          {:else if jtype === "Rope"}
            {@const rope = (
              jnt.joint_type as {
                Rope: {
                  anchor_a: [number, number];
                  anchor_b: [number, number];
                  max_distance: number;
                };
              }
            ).Rope}
            <div class="field-row">
              <label>Anchor A</label>
              <input
                type="number"
                step="0.1"
                value={rope.anchor_a[0]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change rope anchor A", (ent) => {
                    if (
                      ent.components.joint &&
                      "Rope" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Rope: { anchor_a: [number, number] };
                        }
                      ).Rope.anchor_a[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <input
                type="number"
                step="0.1"
                value={rope.anchor_a[1]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change rope anchor A", (ent) => {
                    if (
                      ent.components.joint &&
                      "Rope" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Rope: { anchor_a: [number, number] };
                        }
                      ).Rope.anchor_a[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Anchor B</label>
              <input
                type="number"
                step="0.1"
                value={rope.anchor_b[0]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change rope anchor B", (ent) => {
                    if (
                      ent.components.joint &&
                      "Rope" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Rope: { anchor_b: [number, number] };
                        }
                      ).Rope.anchor_b[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <input
                type="number"
                step="0.1"
                value={rope.anchor_b[1]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change rope anchor B", (ent) => {
                    if (
                      ent.components.joint &&
                      "Rope" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Rope: { anchor_b: [number, number] };
                        }
                      ).Rope.anchor_b[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Max Dist</label>
              <input
                type="number"
                step="0.1"
                min="0"
                value={rope.max_distance}
                onchange={(e) => {
                  editEntity("Change rope max distance", (ent) => {
                    if (
                      ent.components.joint &&
                      "Rope" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Rope: { max_distance: number };
                        }
                      ).Rope.max_distance =
                        parseFloat((e.target as HTMLInputElement).value) || 1;
                  });
                }}
              />
            </div>
          {:else if jtype === "Spring"}
            {@const spr = (
              jnt.joint_type as {
                Spring: {
                  anchor_a: [number, number];
                  anchor_b: [number, number];
                  rest_length: number;
                  stiffness: number;
                  damping: number;
                };
              }
            ).Spring}
            <div class="field-row">
              <label>Anchor A</label>
              <input
                type="number"
                step="0.1"
                value={spr.anchor_a[0]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change spring anchor A", (ent) => {
                    if (
                      ent.components.joint &&
                      "Spring" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Spring: { anchor_a: [number, number] };
                        }
                      ).Spring.anchor_a[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <input
                type="number"
                step="0.1"
                value={spr.anchor_a[1]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change spring anchor A", (ent) => {
                    if (
                      ent.components.joint &&
                      "Spring" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Spring: { anchor_a: [number, number] };
                        }
                      ).Spring.anchor_a[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Anchor B</label>
              <input
                type="number"
                step="0.1"
                value={spr.anchor_b[0]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change spring anchor B", (ent) => {
                    if (
                      ent.components.joint &&
                      "Spring" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Spring: { anchor_b: [number, number] };
                        }
                      ).Spring.anchor_b[0] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
              <input
                type="number"
                step="0.1"
                value={spr.anchor_b[1]}
                style="width:50px"
                onchange={(e) => {
                  editEntity("Change spring anchor B", (ent) => {
                    if (
                      ent.components.joint &&
                      "Spring" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Spring: { anchor_b: [number, number] };
                        }
                      ).Spring.anchor_b[1] =
                        parseFloat((e.target as HTMLInputElement).value) || 0;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Rest Len</label>
              <input
                type="number"
                step="0.1"
                min="0"
                value={spr.rest_length}
                onchange={(e) => {
                  editEntity("Change spring rest length", (ent) => {
                    if (
                      ent.components.joint &&
                      "Spring" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Spring: { rest_length: number };
                        }
                      ).Spring.rest_length =
                        parseFloat((e.target as HTMLInputElement).value) || 1;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Stiffness</label>
              <input
                type="number"
                step="1"
                min="0"
                value={spr.stiffness}
                onchange={(e) => {
                  editEntity("Change spring stiffness", (ent) => {
                    if (
                      ent.components.joint &&
                      "Spring" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Spring: { stiffness: number };
                        }
                      ).Spring.stiffness =
                        parseFloat((e.target as HTMLInputElement).value) || 10;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label>Damping</label>
              <input
                type="number"
                step="0.1"
                min="0"
                value={spr.damping}
                onchange={(e) => {
                  editEntity("Change spring damping", (ent) => {
                    if (
                      ent.components.joint &&
                      "Spring" in ent.components.joint.joint_type
                    )
                      (
                        ent.components.joint.joint_type as {
                          Spring: { damping: number };
                        }
                      ).Spring.damping =
                        parseFloat((e.target as HTMLInputElement).value) || 0.5;
                  });
                }}
              />
            </div>
          {:else if jtype in GENERIC_JOINTS}
            {@const params = (jnt.joint_type as Record<string, Record<string, unknown>>)[jtype] ?? {}}
            {#each Object.entries({ ...GENERIC_JOINTS[jtype], ...params }) as [key, value] (key)}
              {#if typeof value === "number"}
                <div class="field-row">
                  <label>{jointFieldLabel(key)}</label>
                  <input
                    type="number"
                    step="0.1"
                    {value}
                    onchange={(e) =>
                      setJointParam(jtype, key, parseFloat((e.target as HTMLInputElement).value) || 0)}
                  />
                </div>
              {:else if Array.isArray(value) && value.length === 2}
                <div class="field-row">
                  <label>{jointFieldLabel(key)} X</label>
                  <input
                    type="number"
                    step="0.1"
                    value={value[0]}
                    onchange={(e) =>
                      setJointParam(jtype, key, [parseFloat((e.target as HTMLInputElement).value) || 0, Number(value[1])])}
                  />
                  <label>Y</label>
                  <input
                    type="number"
                    step="0.1"
                    value={value[1]}
                    onchange={(e) =>
                      setJointParam(jtype, key, [Number(value[0]), parseFloat((e.target as HTMLInputElement).value) || 0])}
                  />
                </div>
              {/if}
            {/each}
          {/if}
          <div class="field-row">
            <label>Collide Connected</label>
            <input
              type="checkbox"
              checked={jnt.collide_connected}
              onchange={(e) => {
                editEntity("Toggle collide connected", (ent) => {
                  if (ent.components.joint)
                    ent.components.joint.collide_connected = (
                      e.target as HTMLInputElement
                    ).checked;
                });
              }}
            />
          </div>
          {@render undrawnFields("joint")}
        {/if}
      </div>
    {/if}

    <!-- Audio Source -->
    {#if entity.components.audio_source}
      {@const audio = entity.components.audio_source}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("audio_source")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("audio_source");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["audio_source"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("audio_source")}<span class="section-title">Audio Source</span>{@render overrideMark("audio_source")}<span class="section-key">audio_source</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("audio_source");
            }}
            title="Remove Audio Source">×</button
          >
        </div>
        {#if !collapsed["audio_source"]}
          <div class="field-row">
            <label>Clip</label>
            <input
              type="text"
              value={audio.clip_id}
              placeholder="sounds/clip.ogg"
              onchange={(e) => {
                editEntity("Change audio clip", (ent) => {
                  if (ent.components.audio_source)
                    ent.components.audio_source.clip_id = (
                      e.target as HTMLInputElement
                    ).value;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Volume</label>
            <input
              type="number"
              step="0.05"
              min="0"
              max="1"
              value={audio.volume}
              onchange={(e) => {
                editEntity("Change audio volume", (ent) => {
                  if (ent.components.audio_source)
                    ent.components.audio_source.volume =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Pitch</label>
            <input
              type="number"
              step="0.1"
              min="0.1"
              max="4"
              value={audio.pitch}
              onchange={(e) => {
                editEntity("Change audio pitch", (ent) => {
                  if (ent.components.audio_source)
                    ent.components.audio_source.pitch =
                      parseFloat((e.target as HTMLInputElement).value) || 1;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Loop</label>
            <input
              type="checkbox"
              checked={audio.looping}
              onchange={(e) => {
                editEntity("Toggle audio loop", (ent) => {
                  if (ent.components.audio_source)
                    ent.components.audio_source.looping = (
                      e.target as HTMLInputElement
                    ).checked;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Auto Play</label>
            <input
              type="checkbox"
              checked={audio.auto_play}
              onchange={(e) => {
                editEntity("Toggle audio auto play", (ent) => {
                  if (ent.components.audio_source)
                    ent.components.audio_source.auto_play = (
                      e.target as HTMLInputElement
                    ).checked;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label title="Heard from where it is: louder near the listener (an Audio Listener, or the active camera) and panned to its side">Spatial</label>
            <input
              type="checkbox"
              checked={audio.spatial ?? false}
              onchange={(e) => {
                const on = (e.target as HTMLInputElement).checked;
                editEntity("Toggle spatial audio", (ent) => {
                  if (ent.components.audio_source) ent.components.audio_source.spatial = on || undefined;
                });
              }}
            />
          </div>
          {#if audio.spatial}
            <div class="field-row">
              <label title="Full volume within this distance of the listener">Full within</label>
              <input
                type="number"
                min="0"
                value={audio.min_distance ?? 120}
                onchange={(e) => {
                  const v = parseFloat((e.target as HTMLInputElement).value);
                  editEntity("Set audio full within", (ent) => {
                    if (ent.components.audio_source) ent.components.audio_source.min_distance = isNaN(v) ? 120 : v;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label title="Silent from this distance">Silent from</label>
              <input
                type="number"
                min="0"
                value={audio.max_distance ?? 1000}
                onchange={(e) => {
                  const v = parseFloat((e.target as HTMLInputElement).value);
                  editEntity("Set audio silent from", (ent) => {
                    if (ent.components.audio_source) ent.components.audio_source.max_distance = isNaN(v) ? 1000 : v;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label title="A source this far to one side is panned all the way to it">Pan width</label>
              <input
                type="number"
                min="0"
                value={audio.pan_width ?? 600}
                onchange={(e) => {
                  const v = parseFloat((e.target as HTMLInputElement).value);
                  editEntity("Set audio pan width", (ent) => {
                    if (ent.components.audio_source) ent.components.audio_source.pan_width = isNaN(v) ? 600 : v;
                  });
                }}
              />
            </div>
          {/if}
          {@render undrawnFields("audio_source")}
        {/if}
      </div>
    {/if}

    <!-- Velocity -->
    {#if entity.components.velocity}
      {@const vel = entity.components.velocity}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("velocity")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("velocity");
          }}
        >
          <span class="collapse-arrow">{collapsed["velocity"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("velocity")}<span class="section-title">Velocity</span>{@render overrideMark("velocity")}<span class="section-key">velocity</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("velocity");
            }}
            title="Remove Velocity">×</button
          >
        </div>
        {#if !collapsed["velocity"]}
          <div class="field-row">
            <label>X</label>
            <input
              type="number"
              step="1"
              value={vel.x}
              onchange={(e) => {
                editEntity("Change velocity X", (ent) => {
                  if (ent.components.velocity)
                    ent.components.velocity.x =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Y</label>
            <input
              type="number"
              step="1"
              value={vel.y}
              onchange={(e) => {
                editEntity("Change velocity Y", (ent) => {
                  if (ent.components.velocity)
                    ent.components.velocity.y =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Gravity</label>
            <input
              type="number"
              step="0.1"
              value={vel.gravity}
              onchange={(e) => {
                editEntity("Change velocity gravity", (ent) => {
                  if (ent.components.velocity)
                    ent.components.velocity.gravity =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
        {/if}
      </div>
    {/if}

    <!-- Bounce Area -->
    {#if entity.components.bounce_area}
      {@const bounce = entity.components.bounce_area}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("bounce_area")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("bounce_area");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["bounce_area"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("bounce_area")}<span class="section-title">Bounce Area</span>{@render overrideMark("bounce_area")}<span class="section-key">bounce_area</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("bounce_area");
            }}
            title="Remove Bounce Area">×</button
          >
        </div>
        {#if !collapsed["bounce_area"]}
          <div class="field-row">
            <label>Min X</label>
            <input
              type="number"
              step="1"
              value={bounce.min_x}
              onchange={(e) => {
                editEntity("Change bounce min_x", (ent) => {
                  if (ent.components.bounce_area)
                    ent.components.bounce_area.min_x =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Min Y</label>
            <input
              type="number"
              step="1"
              value={bounce.min_y}
              onchange={(e) => {
                editEntity("Change bounce min_y", (ent) => {
                  if (ent.components.bounce_area)
                    ent.components.bounce_area.min_y =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Max X</label>
            <input
              type="number"
              step="1"
              value={bounce.max_x}
              onchange={(e) => {
                editEntity("Change bounce max_x", (ent) => {
                  if (ent.components.bounce_area)
                    ent.components.bounce_area.max_x =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Max Y</label>
            <input
              type="number"
              step="1"
              value={bounce.max_y}
              onchange={(e) => {
                editEntity("Change bounce max_y", (ent) => {
                  if (ent.components.bounce_area)
                    ent.components.bounce_area.max_y =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Damping</label>
            <input
              type="number"
              step="0.01"
              min="0"
              max="1"
              value={bounce.damping}
              onchange={(e) => {
                editEntity("Change bounce damping", (ent) => {
                  if (ent.components.bounce_area)
                    ent.components.bounce_area.damping =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
        {/if}
      </div>
    {/if}

    <!-- Acceleration 2D -->
    {#if entity.components.acceleration}
      {@const acc = entity.components.acceleration}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("acceleration")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("acceleration");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["acceleration"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("acceleration")}<span class="section-title">Acceleration 2D</span>{@render overrideMark("acceleration")}<span class="section-key">acceleration</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("acceleration");
            }}
            title="Remove Acceleration 2D">×</button
          >
        </div>
        {@render depWarning("acceleration")}
        {#if !collapsed["acceleration"]}
          <div class="field-row">
            <label>X</label>
            <input
              type="number"
              step="1"
              value={acc.x}
              onchange={(e) =>
                editEntity("Change acceleration X", (ent) => {
                  if (ent.components.acceleration)
                    ent.components.acceleration.x =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Y</label>
            <input
              type="number"
              step="1"
              value={acc.y}
              onchange={(e) =>
                editEntity("Change acceleration Y", (ent) => {
                  if (ent.components.acceleration)
                    ent.components.acceleration.y =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
        {/if}
      </div>
    {/if}

    <!-- Max Speed -->
    {#if entity.components.max_speed}
      {@const ms = entity.components.max_speed}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("max_speed")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("max_speed");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["max_speed"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("max_speed")}<span class="section-title">Max Speed</span>{@render overrideMark("max_speed")}<span class="section-key">max_speed</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("max_speed");
            }}
            title="Remove Max Speed">×</button
          >
        </div>
        {@render depWarning("max_speed")}
        {#if !collapsed["max_speed"]}
          <div class="field-row">
            <label>Value (px/s)</label>
            <input
              type="number"
              step="10"
              min="0"
              value={ms.value}
              onchange={(e) =>
                editEntity("Change max_speed", (ent) => {
                  if (ent.components.max_speed)
                    ent.components.max_speed.value =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
        {/if}
      </div>
    {/if}

    <!-- Oscillator -->
    {#if entity.components.oscillator}
      {@const osc = entity.components.oscillator}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("oscillator")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("oscillator");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["oscillator"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("oscillator")}<span class="section-title">Oscillator</span>{@render overrideMark("oscillator")}<span class="section-key">oscillator</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("oscillator");
            }}
            title="Remove Oscillator">×</button
          >
        </div>
        {#if !collapsed["oscillator"]}
          <div class="field-row">
            <label>Amplitude X</label>
            <input
              type="number"
              step="1"
              value={osc.amplitude[0]}
              onchange={(e) =>
                editEntity("Change oscillator amp X", (ent) => {
                  if (ent.components.oscillator)
                    ent.components.oscillator.amplitude[0] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Amplitude Y</label>
            <input
              type="number"
              step="1"
              value={osc.amplitude[1]}
              onchange={(e) =>
                editEntity("Change oscillator amp Y", (ent) => {
                  if (ent.components.oscillator)
                    ent.components.oscillator.amplitude[1] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Frequency X (Hz)</label>
            <input
              type="number"
              step="0.1"
              value={osc.frequency[0]}
              onchange={(e) =>
                editEntity("Change oscillator freq X", (ent) => {
                  if (ent.components.oscillator)
                    ent.components.oscillator.frequency[0] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Frequency Y (Hz)</label>
            <input
              type="number"
              step="0.1"
              value={osc.frequency[1]}
              onchange={(e) =>
                editEntity("Change oscillator freq Y", (ent) => {
                  if (ent.components.oscillator)
                    ent.components.oscillator.frequency[1] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Phase X (rad)</label>
            <input
              type="number"
              step="0.1"
              value={osc.phase[0]}
              onchange={(e) =>
                editEntity("Change oscillator phase X", (ent) => {
                  if (ent.components.oscillator)
                    ent.components.oscillator.phase[0] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Phase Y (rad)</label>
            <input
              type="number"
              step="0.1"
              value={osc.phase[1]}
              onchange={(e) =>
                editEntity("Change oscillator phase Y", (ent) => {
                  if (ent.components.oscillator)
                    ent.components.oscillator.phase[1] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          {@render undrawnFields("oscillator")}
        {/if}
      </div>
    {/if}

    <!-- Follow Target -->
    {#if entity.components.follow_target}
      {@const ft = entity.components.follow_target}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("follow_target")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("follow_target");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["follow_target"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("follow_target")}<span class="section-title">Follow Target</span>{@render overrideMark("follow_target")}<span class="section-key">follow_target</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("follow_target");
            }}
            title="Remove Follow Target">×</button
          >
        </div>
        {#if !collapsed["follow_target"]}
          <div class="field-row">
            <label>Target Kind</label>
            <select
              value={ft.target.kind}
              onchange={(e) =>
                editEntity("Change follow target kind", (ent) => {
                  if (!ent.components.follow_target) return;
                  const k = (e.target as HTMLSelectElement).value;
                  ent.components.follow_target.target =
                    k === "Named"
                      ? {
                          kind: "Named",
                          value:
                            ent.components.follow_target.target.value ?? "",
                        }
                      : { kind: k };
                })}
            >
              <option value="SelfEntity">Self</option>
              <option value="Named">Named entity</option>
              <option value="Player">Player (tagged "player")</option>
            </select>
          </div>
          {#if ft.target.kind === "Named"}
            <div class="field-row">
              <label>Target Name</label>
              <input
                type="text"
                value={ft.target.value ?? ""}
                onchange={(e) =>
                  editEntity("Change follow target name", (ent) => {
                    if (ent.components.follow_target)
                      ent.components.follow_target.target = {
                        kind: "Named",
                        value: (e.target as HTMLInputElement).value,
                      };
                  })}
              />
            </div>
          {/if}
          <div class="field-row">
            <label>Offset X</label>
            <input
              type="number"
              step="1"
              value={ft.offset[0]}
              onchange={(e) =>
                editEntity("Change follow offset X", (ent) => {
                  if (ent.components.follow_target)
                    ent.components.follow_target.offset[0] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Offset Y</label>
            <input
              type="number"
              step="1"
              value={ft.offset[1]}
              onchange={(e) =>
                editEntity("Change follow offset Y", (ent) => {
                  if (ent.components.follow_target)
                    ent.components.follow_target.offset[1] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Max Speed</label>
            <input
              type="number"
              step="10"
              min="0"
              value={ft.max_speed}
              onchange={(e) =>
                editEntity("Change follow max_speed", (ent) => {
                  if (ent.components.follow_target)
                    ent.components.follow_target.max_speed =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Stop Distance</label>
            <input
              type="number"
              step="1"
              min="0"
              value={ft.stop_distance}
              onchange={(e) =>
                editEntity("Change follow stop_distance", (ent) => {
                  if (ent.components.follow_target)
                    ent.components.follow_target.stop_distance =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
        {/if}
      </div>
    {/if}

    <!-- Seek Target -->
    {#if entity.components.seek_target}
      {@const st = entity.components.seek_target}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("seek_target")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("seek_target");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["seek_target"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("seek_target")}<span class="section-title">Seek Target</span>{@render overrideMark("seek_target")}<span class="section-key">seek_target</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("seek_target");
            }}
            title="Remove Seek Target">×</button
          >
        </div>
        {@render depWarning("seek_target")}
        {#if !collapsed["seek_target"]}
          <div class="field-row">
            <label>Target Kind</label>
            <select
              value={st.target.kind}
              onchange={(e) =>
                editEntity("Change seek target kind", (ent) => {
                  if (!ent.components.seek_target) return;
                  const k = (e.target as HTMLSelectElement).value;
                  ent.components.seek_target.target =
                    k === "Named"
                      ? {
                          kind: "Named",
                          value: ent.components.seek_target.target.value ?? "",
                        }
                      : { kind: k };
                })}
            >
              <option value="SelfEntity">Self</option>
              <option value="Named">Named entity</option>
              <option value="Player">Player (tagged "player")</option>
            </select>
          </div>
          {#if st.target.kind === "Named"}
            <div class="field-row">
              <label>Target Name</label>
              <input
                type="text"
                value={st.target.value ?? ""}
                onchange={(e) =>
                  editEntity("Change seek target name", (ent) => {
                    if (ent.components.seek_target)
                      ent.components.seek_target.target = {
                        kind: "Named",
                        value: (e.target as HTMLInputElement).value,
                      };
                  })}
              />
            </div>
          {/if}
          <div class="field-row">
            <label>Accel (px/s²)</label>
            <input
              type="number"
              step="10"
              min="0"
              value={st.accel}
              onchange={(e) =>
                editEntity("Change seek accel", (ent) => {
                  if (ent.components.seek_target)
                    ent.components.seek_target.accel =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Max Speed</label>
            <input
              type="number"
              step="10"
              min="0"
              value={st.max_speed}
              onchange={(e) =>
                editEntity("Change seek max_speed", (ent) => {
                  if (ent.components.seek_target)
                    ent.components.seek_target.max_speed =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          {@render undrawnFields("seek_target")}
        {/if}
      </div>
    {/if}

    <!-- Tween -->
    {#if entity.components.tween}
      {@const tw = entity.components.tween}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("tween")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("tween");
          }}
        >
          <span class="collapse-arrow">{collapsed["tween"] ? "▸" : "▾"}</span>
          {@render sectionBar("tween")}<span class="section-title">Tween</span>{@render overrideMark("tween")}<span class="section-key">tween</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("tween");
            }}
            title="Remove Tween">×</button
          >
        </div>
        {#if !collapsed["tween"]}
          <div class="field-row">
            <label>Target</label>
            <select
              value={tw.target}
              onchange={(e) =>
                editEntity("Change tween target", (ent) => {
                  if (ent.components.tween)
                    ent.components.tween.target = (
                      e.target as HTMLSelectElement
                    ).value as typeof tw.target;
                })}
            >
              <option value="PositionX">PositionX</option>
              <option value="PositionY">PositionY</option>
              <option value="Rotation">Rotation</option>
              <option value="ScaleX">ScaleX</option>
              <option value="ScaleY">ScaleY</option>
              <option value="Opacity">Opacity</option>
            </select>
          </div>
          <div class="field-row">
            <label>From</label>
            <input
              type="number"
              step="0.1"
              value={tw.from}
              onchange={(e) =>
                editEntity("Change tween from", (ent) => {
                  if (ent.components.tween)
                    ent.components.tween.from =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>To</label>
            <input
              type="number"
              step="0.1"
              value={tw.to}
              onchange={(e) =>
                editEntity("Change tween to", (ent) => {
                  if (ent.components.tween)
                    ent.components.tween.to =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Duration (s)</label>
            <input
              type="number"
              step="0.1"
              min="0"
              value={tw.duration}
              onchange={(e) =>
                editEntity("Change tween duration", (ent) => {
                  if (ent.components.tween)
                    ent.components.tween.duration =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Ease</label>
            <select
              value={tw.ease}
              onchange={(e) =>
                editEntity("Change tween ease", (ent) => {
                  if (ent.components.tween)
                    ent.components.tween.ease = (e.target as HTMLSelectElement)
                      .value as typeof tw.ease;
                })}
            >
              <option value="Linear">Linear</option>
              <option value="EaseInQuad">EaseInQuad</option>
              <option value="EaseOutQuad">EaseOutQuad</option>
              <option value="EaseInOutQuad">EaseInOutQuad</option>
            </select>
          </div>
          <div class="field-row">
            <label>On Complete</label>
            <select
              value={tw.on_complete}
              onchange={(e) =>
                editEntity("Change tween on_complete", (ent) => {
                  if (ent.components.tween)
                    ent.components.tween.on_complete = (
                      e.target as HTMLSelectElement
                    ).value as typeof tw.on_complete;
                })}
            >
              <option value="Stop">Stop</option>
              <option value="Loop">Loop</option>
              <option value="PingPong">PingPong</option>
            </select>
          </div>
        {/if}
      </div>
    {/if}

    <!-- Patrol Path -->
    {#if entity.components.patrol_path}
      {@const pp = entity.components.patrol_path}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("patrol_path")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("patrol_path");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["patrol_path"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("patrol_path")}<span class="section-title">Patrol Path</span>{@render overrideMark("patrol_path")}<span class="section-key">patrol_path</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("patrol_path");
            }}
            title="Remove Patrol Path">×</button
          >
        </div>
        {#if !collapsed["patrol_path"]}
          <div class="field-row">
            <label>Path ID</label>
            <input
              type="text"
              value={pp.path_id}
              onchange={(e) =>
                editEntity("Change patrol path_id", (ent) => {
                  if (ent.components.patrol_path)
                    ent.components.patrol_path.path_id = (
                      e.target as HTMLInputElement
                    ).value;
                })}
            />
          </div>
          <div class="field-row">
            <label>Speed (px/s)</label>
            <input
              type="number"
              step="10"
              min="0"
              value={pp.speed}
              onchange={(e) =>
                editEntity("Change patrol speed", (ent) => {
                  if (ent.components.patrol_path)
                    ent.components.patrol_path.speed =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Mode</label>
            <select
              value={pp.mode}
              onchange={(e) =>
                editEntity("Change patrol mode", (ent) => {
                  if (ent.components.patrol_path)
                    ent.components.patrol_path.mode = (
                      e.target as HTMLSelectElement
                    ).value as typeof pp.mode;
                })}
            >
              <option value="Loop">Loop</option>
              <option value="PingPong">PingPong</option>
              <option value="Once">Once</option>
            </select>
          </div>
        {/if}
      </div>
    {/if}

    <!-- Sprite Animator -->
    {#if entity.components.sprite_animator}
      {@const anim = entity.components.sprite_animator}
      {@const clipNames = Object.keys(anim.clips ?? {})}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("sprite_animator")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("sprite_animator");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["sprite_animator"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("sprite_animator")}<span class="section-title">Sprite Animator</span>{@render overrideMark("sprite_animator")}<span class="section-key">sprite_animator</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("sprite_animator");
            }}
            title="Remove Sprite Animator">×</button
          >
        </div>
        {@render depWarning("sprite_animator")}
        {#if !collapsed["sprite_animator"]}
          <div class="field-row">
            <label>Speed</label>
            <input
              type="number"
              step="0.1"
              min="0"
              value={anim.speed}
              onchange={(e) => {
                editEntity("Change anim speed", (ent) => {
                  if (ent.components.sprite_animator)
                    ent.components.sprite_animator.speed =
                      parseFloat((e.target as HTMLInputElement).value) || 1;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Playing</label>
            <input
              type="checkbox"
              checked={anim.playing}
              onchange={(e) => {
                editEntity("Toggle anim playing", (ent) => {
                  if (ent.components.sprite_animator)
                    ent.components.sprite_animator.playing = (
                      e.target as HTMLInputElement
                    ).checked;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Random Start</label>
            <input
              type="checkbox"
              checked={anim.random_start ?? false}
              onchange={(e) => {
                editEntity("Toggle random start", (ent) => {
                  if (ent.components.sprite_animator)
                    ent.components.sprite_animator.random_start = (
                      e.target as HTMLInputElement
                    ).checked;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Clip</label>
            {#if clipNames.length > 0}
              <select
                value={anim.current_clip ?? ""}
                onchange={(e) => {
                  const val =
                    (e.target as HTMLSelectElement).value || undefined;
                  editEntity("Change current clip", (ent) => {
                    if (ent.components.sprite_animator)
                      ent.components.sprite_animator.current_clip = val;
                  });
                }}
              >
                <option value="">None</option>
                {#each clipNames as name}
                  <option value={name}>{name}</option>
                {/each}
              </select>
            {:else}
              <span class="dim">No clips defined</span>
            {/if}
          </div>
          <div class="field-row">
            <label>Anim File</label>
            <div class="asset-picker-wrapper">
              <button
                class="compact-btn"
                style="width: 100%"
                onclick={() => (animPickerOpen = !animPickerOpen)}
                >📂 Load .anim file</button
              >
              {#if animPickerOpen && animFiles.length > 0}
                <button
                  class="picker-backdrop"
                  onclick={() => (animPickerOpen = false)}
                  aria-label="Close picker"
                ></button>
                <div class="picker-dropdown" role="listbox">
                  {#each animFiles as file}
                    <button
                      class="picker-item"
                      role="option"
                      aria-selected={false}
                      onclick={() => loadAnimFileToEntity(file)}>{file}</button
                    >
                  {/each}
                </div>
              {:else if animPickerOpen}
                <button
                  class="picker-backdrop"
                  onclick={() => (animPickerOpen = false)}
                  aria-label="Close picker"
                ></button>
                <div class="picker-dropdown">
                  <span class="dim" style="padding: 8px;"
                    >No .anim files found</span
                  >
                </div>
              {/if}
            </div>
          </div>
          {#if clipNames.length > 0}
            <div class="dim" style="padding: 2px 0; font-size: 11px;">
              {clipNames.length} clip{clipNames.length === 1 ? "" : "s"}: {clipNames.join(
                ", ",
              )}
            </div>
            <!-- Per-clip details -->
            {#each clipNames as clipName}
              {@const clip = anim.clips[clipName]}
              {#if clip}
                <div class="clip-detail">
                  <div class="clip-detail-header">
                    <span class="clip-name">{clipName}</span>
                    <span class="dim" style="font-size: 10px;"
                      >{clip.frames?.length ?? 0} frames</span
                    >
                    <button
                      class="remove-btn"
                      title="Remove clip"
                      onclick={() => {
                        editEntity(`Remove clip "${clipName}"`, (ent) => {
                          if (ent.components.sprite_animator?.clips) {
                            delete ent.components.sprite_animator.clips[
                              clipName
                            ];
                            if (
                              ent.components.sprite_animator.current_clip ===
                              clipName
                            ) {
                              const remaining = Object.keys(
                                ent.components.sprite_animator.clips,
                              );
                              ent.components.sprite_animator.current_clip =
                                remaining[0] ?? undefined;
                            }
                          }
                        });
                      }}>×</button
                    >
                  </div>
                  <div class="clip-detail-fields">
                    <label>FPS</label>
                    <input
                      type="number"
                      step="1"
                      min="1"
                      max="120"
                      value={clip.frame_duration > 0
                        ? Math.round(1.0 / clip.frame_duration)
                        : 10}
                      style="width: 48px;"
                      onchange={(e) => {
                        const fps =
                          parseFloat((e.target as HTMLInputElement).value) ||
                          10;
                        editEntity(`Set clip "${clipName}" FPS`, (ent) => {
                          const c =
                            ent.components.sprite_animator?.clips?.[clipName];
                          if (c) c.frame_duration = fps > 0 ? 1.0 / fps : 0.1;
                        });
                      }}
                    />
                    <label>Loop</label>
                    <input
                      type="checkbox"
                      checked={clip.looping}
                      onchange={(e) => {
                        editEntity(
                          `Toggle clip "${clipName}" looping`,
                          (ent) => {
                            const c =
                              ent.components.sprite_animator?.clips?.[clipName];
                            if (c)
                              c.looping = (
                                e.target as HTMLInputElement
                              ).checked;
                          },
                        );
                      }}
                    />
                  </div>
                  <!-- Frame Triggers -->
                  {#if (clip.frames?.length ?? 0) > 0}
                    <div style="padding: 2px 0;">
                      <span class="dim" style="font-size: 10px;"
                        >Frame Triggers</span
                      >
                      {#each clip.frames as frame, fi}
                        {#if frame.triggers && frame.triggers.length > 0}
                          {#each frame.triggers as trigger, ti}
                            <div class="field-row" style="font-size: 10px;">
                              <span
                                style="min-width: 18px; color: var(--mauve);"
                                >F{fi}</span
                              >
                              <span class="dim"
                                >{trigger.type === "PlaySound"
                                  ? `🔊 ${trigger.clip_id}`
                                  : `📢 ${trigger.event_name}`}</span
                              >
                              <button
                                class="remove-btn"
                                title="Remove trigger"
                                onclick={() => {
                                  editEntity(
                                    `Remove trigger from frame ${fi}`,
                                    (ent) => {
                                      const c =
                                        ent.components.sprite_animator?.clips?.[
                                          clipName
                                        ];
                                      if (c?.frames?.[fi]?.triggers) {
                                        c.frames[fi].triggers.splice(ti, 1);
                                      }
                                    },
                                  );
                                }}>×</button
                              >
                            </div>
                          {/each}
                        {/if}
                      {/each}
                      <div class="field-row" style="font-size: 10px; gap: 2px;">
                        <label>Frame</label>
                        <input
                          type="number"
                          min="0"
                          max={(clip.frames?.length ?? 1) - 1}
                          value="0"
                          style="width: 36px;"
                          class="trigger-frame-input"
                          data-clip={clipName}
                        />
                        <select
                          style="width: 70px;"
                          class="trigger-type-select"
                          data-clip={clipName}
                        >
                          <option value="PlaySound">Sound</option>
                          <option value="EmitEvent">Event</option>
                        </select>
                        <input
                          type="text"
                          placeholder="clip_id / event"
                          style="width: 80px;"
                          class="trigger-value-input"
                          data-clip={clipName}
                        />
                        <button
                          class="compact-btn"
                          style="padding: 1px 4px;"
                          onclick={(e) => {
                            const row = (e.target as HTMLElement)
                              .parentElement!;
                            const frameIdx = parseInt(
                              (
                                row.querySelector(
                                  ".trigger-frame-input",
                                ) as HTMLInputElement
                              )?.value ?? "0",
                            );
                            const trigType =
                              (
                                row.querySelector(
                                  ".trigger-type-select",
                                ) as HTMLSelectElement
                              )?.value ?? "PlaySound";
                            const trigVal =
                              (
                                row.querySelector(
                                  ".trigger-value-input",
                                ) as HTMLInputElement
                              )?.value ?? "";
                            if (!trigVal.trim()) return;
                            editEntity(
                              `Add ${trigType} trigger to frame ${frameIdx}`,
                              (ent) => {
                                const c =
                                  ent.components.sprite_animator?.clips?.[
                                    clipName
                                  ];
                                if (!c?.frames?.[frameIdx]) return;
                                if (!c.frames[frameIdx].triggers)
                                  c.frames[frameIdx].triggers = [];
                                if (trigType === "PlaySound") {
                                  c.frames[frameIdx].triggers.push({
                                    type: "PlaySound",
                                    clip_id: trigVal.trim(),
                                    volume: 1.0,
                                  });
                                } else {
                                  c.frames[frameIdx].triggers.push({
                                    type: "EmitEvent",
                                    event_name: trigVal.trim(),
                                  });
                                }
                              },
                            );
                          }}>+</button
                        >
                      </div>
                    </div>
                  {/if}
                </div>
              {/if}
            {/each}
          {/if}
          {#if !playing}
            <button
              class="compact-btn"
              style="margin-top: 4px; width: 100%;"
              onclick={openGridImport}>Generate from Grid…</button
            >
            <button
              class="compact-btn"
              style="margin-top: 4px; width: 100%;"
              onclick={openAtlasImport}>Import from Spritesheet…</button
            >
            <button
              class="compact-btn"
              style="margin-top: 4px; width: 100%;"
              onclick={openAtlasDefImport}>Import from Atlas…</button
            >
            {#if importAtlasDefOpen}
              <button
                class="picker-backdrop"
                onclick={() => (importAtlasDefOpen = false)}
                aria-label="Close picker"
              ></button>
              <div class="picker-dropdown atlas-anim-picker" role="listbox">
                <!-- Tabs -->
                <div class="atlas-import-tabs">
                  <button
                    class="atlas-import-tab"
                    class:active={importAtlasDefTab === "animations"}
                    disabled={atlasDefAnimations.length === 0}
                    onclick={() => (importAtlasDefTab = "animations")}
                    >Animations ({atlasDefAnimations.length})</button
                  >
                  <button
                    class="atlas-import-tab"
                    class:active={importAtlasDefTab === "images"}
                    disabled={atlasDefImages.length === 0}
                    onclick={() => (importAtlasDefTab = "images")}
                    >Images ({atlasDefImages.length})</button
                  >
                </div>

                {#if importAtlasDefTab === "animations" && atlasDefAnimations.length > 0}
                  <div class="picker-group-label">Click to import a clip</div>
                  {#each atlasDefAnimations as anim}
                    <button
                      class="picker-item"
                      role="option"
                      aria-selected={false}
                      onclick={() => importAtlasDefAnimation(anim)}
                      >{anim.animation_name}
                      <span class="picker-item-dim"
                        >({anim.atlas_name} · {anim.frame_count} frames · {anim.fps}
                        FPS{anim.looping ? " · loop" : ""})</span
                      ></button
                    >
                  {/each}
                {:else if importAtlasDefTab === "images"}
                  <div class="atlas-import-config">
                    <div class="field-row">
                      <label>Clip name</label>
                      <input
                        type="text"
                        bind:value={importAtlasDefClipName}
                        placeholder="default"
                      />
                    </div>
                    <div class="field-row">
                      <label>FPS</label>
                      <input
                        type="number"
                        bind:value={importAtlasDefFps}
                        min="1"
                        max="120"
                        step="1"
                        style="width: 50px;"
                      />
                    </div>
                    <div class="field-row">
                      <label>Loop</label>
                      <input
                        type="checkbox"
                        bind:checked={importAtlasDefLooping}
                      />
                    </div>
                  </div>
                  <div class="picker-group-label">
                    Select frames (click in order)
                  </div>
                  {#each [...atlasDefImagesByAtlas] as [atlasName, images]}
                    <div class="picker-group-label">🎴 {atlasName}</div>
                    {#each images as img}
                      {@const idx = importAtlasDefSelectedImages.findIndex(
                        (s) =>
                          s.image_path === img.image_path &&
                          s.atlas_name === img.atlas_name,
                      )}
                      <button
                        class="picker-item"
                        class:active={idx >= 0}
                        role="option"
                        aria-selected={idx >= 0}
                        onclick={() => toggleAtlasDefImage(img)}
                      >
                        {#if idx >= 0}<span class="frame-badge">{idx + 1}</span
                          >{/if}
                        {img.image_name}
                        <span class="picker-item-dim">({img.image_path})</span>
                      </button>
                    {/each}
                  {/each}
                  {#if importAtlasDefSelectedImages.length > 0}
                    <div class="atlas-import-actions">
                      <button
                        class="compact-btn"
                        onclick={importAtlasDefFromImages}
                        >Import {importAtlasDefSelectedImages.length} frame{importAtlasDefSelectedImages.length ===
                        1
                          ? ""
                          : "s"} as clip</button
                      >
                    </div>
                  {/if}
                {/if}
              </div>
            {/if}
          {/if}
          {@render undrawnFields("sprite_animator")}
        {/if}
      </div>
    {/if}

    <!-- Animation State Machine -->
    {#if entity.components.anim_state_machine}
      {@const fsm = entity.components.anim_state_machine}
      {@const stateNames = Object.keys(fsm.states ?? {})}
      {@const paramNames = Object.keys(fsm.parameters ?? {})}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("anim_state_machine")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("anim_state_machine");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["anim_state_machine"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("anim_state_machine")}<span class="section-title">Animation State Machine</span>{@render overrideMark("anim_state_machine")}<span class="section-key">anim_state_machine</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("anim_state_machine");
            }}
            title="Remove Anim State Machine">×</button
          >
        </div>
        {@render depWarning("anim_state_machine")}
        {#if !collapsed["anim_state_machine"]}
          <!-- Open visual editor button -->
          <div class="field-row" style="margin-bottom: 6px;">
            <button
              class="compact-btn"
              style="width: 100%; padding: 4px 8px; font-size: 11px;"
              onclick={() => {
                window.dispatchEvent(
                  new CustomEvent("open-statemachine-editor", {
                    detail: {
                      entityId: $selectedEntity,
                      data: structuredClone(fsm),
                    },
                  }),
                );
              }}>🔀 Edit Visual State Machine</button
            >
          </div>
          <!-- Current State -->
          <div class="field-row">
            <label>Current</label>
            {#if stateNames.length > 0}
              <select
                value={fsm.current_state ?? ""}
                onchange={(e) => {
                  const val =
                    (e.target as HTMLSelectElement).value || undefined;
                  editEntity("Set FSM current state", (ent) => {
                    if (ent.components.anim_state_machine)
                      ent.components.anim_state_machine.current_state = val;
                  });
                }}
              >
                <option value="">None</option>
                {#each stateNames as name}
                  <option value={name}>{name}</option>
                {/each}
              </select>
            {:else}
              <span class="dim">No states defined</span>
            {/if}
          </div>

          <!-- Parameters -->
          <div style="padding: 2px 0;">
            <span class="dim" style="font-size: 11px; font-weight: bold;"
              >Parameters</span
            >
            {#each paramNames as pname}
              {@const param = fsm.parameters[pname]}
              <div class="field-row" style="font-size: 10px;">
                <span style="min-width: 50px;">{pname}</span>
                <span class="dim">{param?.type ?? "?"}</span>
                <button
                  class="remove-btn"
                  title="Remove parameter"
                  onclick={() => {
                    editEntity(`Remove param "${pname}"`, (ent) => {
                      if (ent.components.anim_state_machine?.parameters) {
                        delete ent.components.anim_state_machine.parameters[
                          pname
                        ];
                      }
                    });
                  }}>×</button
                >
              </div>
            {/each}
            <div class="field-row" style="font-size: 10px; gap: 2px;">
              <input
                type="text"
                placeholder="name"
                style="width: 60px;"
                class="fsm-param-name"
              />
              <select style="width: 58px;" class="fsm-param-type">
                <option value="Bool">Bool</option>
                <option value="Float">Float</option>
                <option value="Trigger">Trigger</option>
              </select>
              <button
                class="compact-btn"
                style="padding: 1px 4px;"
                onclick={(e) => {
                  const row = (e.target as HTMLElement).parentElement!;
                  const nameInput = row.querySelector(
                    ".fsm-param-name",
                  ) as HTMLInputElement;
                  const typeSelect = row.querySelector(
                    ".fsm-param-type",
                  ) as HTMLSelectElement;
                  const pName = nameInput?.value?.trim();
                  const pType = typeSelect?.value ?? "Bool";
                  if (!pName) return;
                  editEntity(`Add FSM param "${pName}"`, (ent) => {
                    if (!ent.components.anim_state_machine) return;
                    if (!ent.components.anim_state_machine.parameters)
                      ent.components.anim_state_machine.parameters = {};
                    if (pType === "Bool") {
                      ent.components.anim_state_machine.parameters[pName] = {
                        type: "Bool",
                        value: false,
                      };
                    } else if (pType === "Float") {
                      ent.components.anim_state_machine.parameters[pName] = {
                        type: "Float",
                        value: 0,
                      };
                    } else {
                      ent.components.anim_state_machine.parameters[pName] = {
                        type: "Trigger",
                      };
                    }
                  });
                }}>+</button
              >
            </div>
          </div>

          <!-- States -->
          <div style="padding: 2px 0;">
            <span class="dim" style="font-size: 11px; font-weight: bold;"
              >States</span
            >
            {#each stateNames as sname}
              {@const st = fsm.states[sname]}
              {#if st}
                <div class="clip-detail">
                  <div class="clip-detail-header">
                    <span class="clip-name">{sname}</span>
                    <span class="dim" style="font-size: 10px;"
                      >→ {st.clip_name}</span
                    >
                    <button
                      class="remove-btn"
                      title="Remove state"
                      onclick={() => {
                        editEntity(`Remove state "${sname}"`, (ent) => {
                          if (ent.components.anim_state_machine?.states) {
                            delete ent.components.anim_state_machine.states[
                              sname
                            ];
                            if (
                              ent.components.anim_state_machine
                                .current_state === sname
                            ) {
                              ent.components.anim_state_machine.current_state =
                                undefined;
                            }
                          }
                        });
                      }}>×</button
                    >
                  </div>
                  <!-- Transitions -->
                  {#if st.transitions && st.transitions.length > 0}
                    {#each st.transitions as tr, tri}
                      <div class="field-row" style="font-size: 10px;">
                        <span class="dim">→ {tr.target}</span>
                        <span class="dim" style="font-size: 9px;">
                          {tr.conditions
                            ?.map((c: any) => {
                              if (c.type === "BoolEquals")
                                return `${c.param}=${c.value}`;
                              if (c.type === "FloatGreaterThan")
                                return `${c.param}>${c.threshold}`;
                              if (c.type === "FloatLessThan")
                                return `${c.param}<${c.threshold}`;
                              if (c.type === "ClipFinished") return "clip_done";
                              if (c.type === "TriggerSet")
                                return `⚡${c.param}`;
                              return "?";
                            })
                            .join(" & ") || "always"}
                        </span>
                        <button
                          class="remove-btn"
                          title="Remove transition"
                          onclick={() => {
                            editEntity(`Remove transition`, (ent) => {
                              const s =
                                ent.components.anim_state_machine?.states?.[
                                  sname
                                ];
                              if (s?.transitions) s.transitions.splice(tri, 1);
                            });
                          }}>×</button
                        >
                      </div>
                    {/each}
                  {/if}
                  <!-- Add transition -->
                  <div class="field-row" style="font-size: 10px; gap: 2px;">
                    <select
                      style="width: 55px;"
                      class="fsm-tr-target"
                      data-state={sname}
                    >
                      {#each stateNames.filter((n) => n !== sname) as tgt}
                        <option value={tgt}>{tgt}</option>
                      {/each}
                    </select>
                    <select
                      style="width: 58px;"
                      class="fsm-tr-condtype"
                      data-state={sname}
                    >
                      <option value="BoolEquals">Bool=</option>
                      <option value="FloatGreaterThan">Float&gt;</option>
                      <option value="FloatLessThan">Float&lt;</option>
                      <option value="ClipFinished">Clip Done</option>
                      <option value="TriggerSet">Trigger</option>
                    </select>
                    <input
                      type="text"
                      placeholder="param"
                      style="width: 50px;"
                      class="fsm-tr-param"
                      data-state={sname}
                    />
                    <input
                      type="text"
                      placeholder="val"
                      style="width: 30px;"
                      class="fsm-tr-val"
                      data-state={sname}
                    />
                    <button
                      class="compact-btn"
                      style="padding: 1px 4px;"
                      onclick={(e) => {
                        const row = (e.target as HTMLElement).parentElement!;
                        const target = (
                          row.querySelector(
                            ".fsm-tr-target",
                          ) as HTMLSelectElement
                        )?.value;
                        const condType = (
                          row.querySelector(
                            ".fsm-tr-condtype",
                          ) as HTMLSelectElement
                        )?.value;
                        const param = (
                          row.querySelector(".fsm-tr-param") as HTMLInputElement
                        )?.value?.trim();
                        const val = (
                          row.querySelector(".fsm-tr-val") as HTMLInputElement
                        )?.value?.trim();
                        if (!target) return;
                        let condition: any;
                        if (condType === "BoolEquals")
                          condition = {
                            type: "BoolEquals",
                            param: param || "param",
                            value: val === "true",
                          };
                        else if (condType === "FloatGreaterThan")
                          condition = {
                            type: "FloatGreaterThan",
                            param: param || "param",
                            threshold: parseFloat(val) || 0,
                          };
                        else if (condType === "FloatLessThan")
                          condition = {
                            type: "FloatLessThan",
                            param: param || "param",
                            threshold: parseFloat(val) || 0,
                          };
                        else if (condType === "ClipFinished")
                          condition = { type: "ClipFinished" };
                        else if (condType === "TriggerSet")
                          condition = {
                            type: "TriggerSet",
                            param: param || "param",
                          };
                        editEntity(
                          `Add transition ${sname}→${target}`,
                          (ent) => {
                            const s =
                              ent.components.anim_state_machine?.states?.[
                                sname
                              ];
                            if (!s) return;
                            if (!s.transitions) s.transitions = [];
                            s.transitions.push({
                              target,
                              conditions: condition ? [condition] : [],
                            });
                          },
                        );
                      }}>+</button
                    >
                  </div>
                </div>
              {/if}
            {/each}
          </div>

          <!-- Add State -->
          {@const animClipNames = Object.keys(
            entity.components.sprite_animator?.clips ?? {},
          )}
          <div class="field-row" style="gap: 2px;">
            <input
              type="text"
              placeholder="state name"
              style="width: 70px;"
              class="fsm-new-state-name"
            />
            <select style="width: 70px;" class="fsm-new-state-clip">
              {#each animClipNames as cn}
                <option value={cn}>{cn}</option>
              {/each}
            </select>
            <button
              class="compact-btn"
              style="padding: 1px 6px;"
              onclick={(e) => {
                const row = (e.target as HTMLElement).parentElement!;
                const sName = (
                  row.querySelector(".fsm-new-state-name") as HTMLInputElement
                )?.value?.trim();
                const clipName = (
                  row.querySelector(".fsm-new-state-clip") as HTMLSelectElement
                )?.value;
                if (!sName || !clipName) return;
                editEntity(`Add FSM state "${sName}"`, (ent) => {
                  if (!ent.components.anim_state_machine) return;
                  if (!ent.components.anim_state_machine.states)
                    ent.components.anim_state_machine.states = {};
                  ent.components.anim_state_machine.states[sName] = {
                    clip_name: clipName,
                    transitions: [],
                  };
                });
              }}>Add State</button
            >
          </div>
        {/if}
      </div>
    {/if}

    <!-- Tilemap -->
    {#if entity.components.tilemap}
      {@const tm = entity.components.tilemap}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("tilemap")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("tilemap");
          }}
        >
          <span class="collapse-arrow">{collapsed["tilemap"] ? "▸" : "▾"}</span>
          {@render sectionBar("tilemap")}<span class="section-title">Tilemap</span>{@render overrideMark("tilemap")}<span class="section-key">tilemap</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("tilemap");
            }}
            title="Remove Tilemap">×</button
          >
        </div>
        {@render depWarning("tilemap")}
        {#if !collapsed["tilemap"]}
          <div class="field-row">
            <label>Source</label>
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              style="display:flex;gap:4px;flex:1;min-width:0;"
              role="group"
              ondragover={(e) => {
                if (e.dataTransfer?.types.includes(ASSET_DRAG_MIME)) {
                  e.preventDefault();
                  e.dataTransfer.dropEffect = "copy";
                }
              }}
              ondrop={async (e) => {
                const raw = e.dataTransfer?.getData(ASSET_DRAG_MIME);
                if (!raw) return;
                e.preventDefault();
                let path: string;
                try {
                  path = (JSON.parse(raw) as { path: string }).path;
                } catch {
                  return;
                }
                if (!path.endsWith(".tilemap")) {
                  logError("Drop a .tilemap asset onto the tilemap source");
                  return;
                }
                const file = path.startsWith("assets/")
                  ? path.slice("assets/".length)
                  : path;
                const proj = get(project);
                if (!proj) return;
                try {
                  const data = await readTilemapFile(proj.path, file);
                  editEntity("Load tilemap from file", (ent) => {
                    if (ent.components.tilemap) {
                      ent.components.tilemap.tile_size = data.tile_size;
                      ent.components.tilemap.layers = data.layers;
                      ent.components.tilemap.tilesets = data.tilesets;
                      ent.components.tilemap.source = file;
                    }
                  });
                  logInfo(`Loaded tilemap: ${file}`);
                } catch (err) {
                  logError(`Failed to load tilemap: ${err}`);
                }
              }}
            >
              <select
                style="flex:1;min-width:0;"
                value={tm.source ?? ""}
                onchange={async (e) => {
                  const file = (e.target as HTMLSelectElement).value;
                  if (!file) return;
                  const proj = get(project);
                  if (!proj) return;
                  try {
                    const data = await readTilemapFile(proj.path, file);
                    editEntity("Load tilemap from file", (ent) => {
                      if (ent.components.tilemap) {
                        ent.components.tilemap.tile_size = data.tile_size;
                        ent.components.tilemap.layers = data.layers;
                        ent.components.tilemap.tilesets = data.tilesets;
                        ent.components.tilemap.source = file;
                      }
                    });
                    logInfo(`Loaded tilemap: ${file}`);
                  } catch (err) {
                    logError(`Failed to load tilemap: ${err}`);
                  }
                }}
                onfocus={async () => {
                  const proj = get(project);
                  if (!proj) return;
                  try {
                    const all = await listAssets(proj.path);
                    tilemapFiles = all.filter((f: string) =>
                      f.endsWith(".tilemap"),
                    );
                  } catch {}
                }}
              >
                <option value="">-- Select .tilemap --</option>
                {#each tilemapFiles as f}
                  <option value={f}>{f}</option>
                {/each}
              </select>
              <!-- A map in a file, or the entity's own: either opens in the
                   tilemap editor with this entity, whose `autotile` says
                   which layers are painted in terrains. -->
              <button
                class="small-btn"
                title={tm.source ? "Edit in Tilemap Editor" : "Edit this entity's map in the Tilemap Editor"}
                disabled={playing}
                onclick={() => {
                  const proj = get(project);
                  if (!proj || !entity) return;
                  window.dispatchEvent(
                    new CustomEvent("open-tilemap-editor", {
                      detail: tm.source
                        ? { fullPath: `${proj.path}/assets/${tm.source}`, entity: entity.name }
                        : { entity: entity.name },
                    }),
                  );
                }}>✏</button
              >
            </div>
          </div>
          <div class="field-row">
            <label>Tile W</label>
            <input
              type="number"
              step="1"
              min="1"
              value={tm.tile_size[0]}
              onchange={(e) => {
                editEntity("Change tile width", (ent) => {
                  if (ent.components.tilemap)
                    ent.components.tilemap.tile_size[0] =
                      parseFloat((e.target as HTMLInputElement).value) || 16;
                });
              }}
            />
            <label>H</label>
            <input
              type="number"
              step="1"
              min="1"
              value={tm.tile_size[1]}
              onchange={(e) => {
                editEntity("Change tile height", (ent) => {
                  if (ent.components.tilemap)
                    ent.components.tilemap.tile_size[1] =
                      parseFloat((e.target as HTMLInputElement).value) || 16;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label title="Square cells, diamonds hung from the top vertex of cell (0, 0), or hexagons">Grid</label>
            <select
              value={tm.orientation ?? "orthogonal"}
              onchange={(e) => {
                const v = (e.target as HTMLSelectElement).value;
                editEntity("Change tilemap grid", (ent) => {
                  const t = ent.components.tilemap;
                  if (!t) return;
                  if (v === "isometric" || v === "hexagonal") t.orientation = v;
                  else delete t.orientation;
                  // A regular pointy-topped hexagon's side is half its height.
                  if (v === "hexagonal" && !t.hex_side) t.hex_side = t.tile_size[1] / 2;
                });
              }}
            >
              <option value="orthogonal">Square</option>
              <option value="isometric">Isometric</option>
              <option value="hexagonal">Hexagonal</option>
            </select>
          </div>
          {#if tm.orientation === "hexagonal"}
            <div class="field-row">
              <label title="The hexagon's flat sides: the upright ones when pointy-topped, the level ones when flat-topped. Half the height (or width) is a regular hexagon.">Hex side</label>
              <input
                type="number"
                min="0"
                value={tm.hex_side ?? 0}
                onchange={(e) => {
                  const v = parseFloat((e.target as HTMLInputElement).value);
                  editEntity("Change hex side", (ent) => {
                    if (ent.components.tilemap) ent.components.tilemap.hex_side = isNaN(v) ? 0 : v;
                  });
                }}
              />
            </div>
            <div class="field-row">
              <label title="Pointy-topped hexagons in rows (every other row pushed right), or flat-topped in columns (every other column pushed down)">Stagger</label>
              <select
                value={`${tm.stagger_axis ?? "y"}-${tm.stagger_index ?? "odd"}`}
                onchange={(e) => {
                  const [axis, index] = (e.target as HTMLSelectElement).value.split("-");
                  editEntity("Change hex stagger", (ent) => {
                    const t = ent.components.tilemap;
                    if (!t) return;
                    if (axis === "x") t.stagger_axis = "x";
                    else delete t.stagger_axis;
                    if (index === "even") t.stagger_index = "even";
                    else delete t.stagger_index;
                  });
                }}
              >
                <option value="y-odd">Pointy, odd rows pushed</option>
                <option value="y-even">Pointy, even rows pushed</option>
                <option value="x-odd">Flat, odd columns pushed</option>
                <option value="x-even">Flat, even columns pushed</option>
              </select>
            </div>
          {/if}
          <div class="dim" style="padding: 2px 0; font-size: 11px;">
            {tm.layers.length} layer{tm.layers.length === 1 ? "" : "s"}, {tm
              .tilesets.length} tileset{tm.tilesets.length === 1 ? "" : "s"}
          </div>
          {#each tm.layers as layer, li}
            <div class="field-row">
              <label>{layer.name}</label>
              <span class="dim">{layer.width}×{layer.height}</span>
              <input
                type="checkbox"
                checked={layer.visible}
                title="Visible"
                onchange={(e) => {
                  editEntity(`Toggle layer ${layer.name}`, (ent) => {
                    if (ent.components.tilemap?.layers[li])
                      ent.components.tilemap.layers[li].visible = (
                        e.target as HTMLInputElement
                      ).checked;
                  });
                }}
              />
              <label class="dim" title="Sort this layer's tiles with y-sorted sprites, by cell">Y-sort</label>
              <input
                type="checkbox"
                checked={layer.y_sort ?? false}
                title="Y-sort"
                onchange={(e) => {
                  const on = (e.target as HTMLInputElement).checked;
                  editEntity(`Y-sort layer ${layer.name}`, (ent) => {
                    const l = ent.components.tilemap?.layers[li];
                    if (!l) return;
                    if (on) l.y_sort = true;
                    else delete l.y_sort;
                  });
                }}
              />
            </div>
          {/each}
          <div class="field-row">
            <label>Z Order</label>
            <input
              type="number"
              step="1"
              value={tm.z_order ?? 0}
              onchange={(e) => {
                const val =
                  parseInt((e.target as HTMLInputElement).value, 10) || 0;
                editEntity("Change tilemap z_order", (ent) => {
                  if (ent.components.tilemap)
                    ent.components.tilemap.z_order = val;
                });
              }}
            />
          </div>
          {@render sortLayerPicker("tilemap", tm.sort_layer, (v) =>
            editEntity("Change tilemap sort_layer", (ent) => {
              if (ent.components.tilemap) ent.components.tilemap.sort_layer = v;
            }),
          )}
        {/if}
      </div>
    {/if}

    <!-- Particle Emitter -->
    {#if entity.components.particle_emitter}
      {@const pe = entity.components.particle_emitter}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("particle_emitter")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("particle_emitter");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["particle_emitter"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("particle_emitter")}<span class="section-title">Particle Emitter</span>{@render overrideMark("particle_emitter")}<span class="section-key">particle_emitter</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("particle_emitter");
            }}
            title="Remove Particle Emitter">×</button
          >
        </div>
        {@render depWarning("particle_emitter")}
        {#if !collapsed["particle_emitter"]}
          <div class="field-row">
            <label>Preset</label>
            <select
              onchange={(e) => {
                const name = (e.target as HTMLSelectElement).value;
                if (!name) return;
                const preset = PARTICLE_PRESETS.find((p) => p.label === name);
                if (!preset) return;
                editEntity(`Apply ${name} preset`, (ent) => {
                  if (ent.components.particle_emitter) {
                    Object.assign(
                      ent.components.particle_emitter,
                      JSON.parse(JSON.stringify(preset.values)),
                    );
                  }
                });
                (e.target as HTMLSelectElement).value = "";
              }}
            >
              <option value="">Apply preset…</option>
              {#each PARTICLE_PRESETS as preset}
                <option value={preset.label}>{preset.label}</option>
              {/each}
            </select>
          </div>
          <div class="field-row">
            <label>File</label>
            <select
              onchange={async (e) => {
                const relPath = (e.target as HTMLSelectElement).value;
                if (!relPath) return;
                const proj = get(project);
                if (!proj) return;
                try {
                  const data = await readParticleFile(proj.path, relPath);
                  editEntity(`Load ${relPath}`, (ent) => {
                    if (ent.components.particle_emitter) {
                      Object.assign(ent.components.particle_emitter, data);
                    }
                  });
                } catch (err) {
                  logError(`Failed to load particle file: ${err}`);
                }
                (e.target as HTMLSelectElement).value = "";
              }}
              onfocus={async (e) => {
                const proj = get(project);
                if (!proj) return;
                const sel = e.target as HTMLSelectElement;
                // Clear old options except the placeholder
                while (sel.options.length > 1) sel.remove(1);
                try {
                  const files = await listAssets(proj.path);
                  for (const f of files) {
                    if (f.endsWith(".particle")) {
                      const opt = document.createElement("option");
                      const label = f.startsWith("assets/") ? f.slice(7) : f;
                      opt.value = label;
                      opt.textContent = label;
                      sel.appendChild(opt);
                    }
                  }
                } catch {
                  /* ignore */
                }
              }}
            >
              <option value="">Load from file…</option>
            </select>
          </div>
          <div class="field-row">
            <label>Active</label>
            <input
              type="checkbox"
              checked={pe.active}
              onchange={(e) => {
                editEntity("Toggle emitter active", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.active = (
                      e.target as HTMLInputElement
                    ).checked;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Texture</label>
            <input
              type="text"
              value={pe.texture_id ?? ""}
              placeholder="(white quad)"
              onchange={(e) => {
                const val = (e.target as HTMLInputElement).value || undefined;
                editEntity("Change particle texture", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.texture_id = val;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Rate</label>
            <input
              type="number"
              step="1"
              min="0"
              value={pe.emission_rate}
              onchange={(e) => {
                editEntity("Change emission rate", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.emission_rate =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Max</label>
            <input
              type="number"
              step="1"
              min="1"
              value={pe.max_particles}
              onchange={(e) => {
                editEntity("Change max particles", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.max_particles =
                      parseInt((e.target as HTMLInputElement).value) || 256;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Life</label>
            <input
              type="number"
              step="0.1"
              min="0"
              value={pe.lifetime[0]}
              onchange={(e) => {
                editEntity("Change lifetime min", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.lifetime[0] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
            <span class="dim">–</span>
            <input
              type="number"
              step="0.1"
              min="0"
              value={pe.lifetime[1]}
              onchange={(e) => {
                editEntity("Change lifetime max", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.lifetime[1] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Speed</label>
            <input
              type="number"
              step="1"
              min="0"
              value={pe.speed[0]}
              onchange={(e) => {
                editEntity("Change speed min", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.speed[0] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
            <span class="dim">–</span>
            <input
              type="number"
              step="1"
              min="0"
              value={pe.speed[1]}
              onchange={(e) => {
                editEntity("Change speed max", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.speed[1] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Size Start</label>
            <input
              type="number"
              step="0.5"
              min="0"
              value={pe.size_start[0]}
              onchange={(e) => {
                editEntity("Change size start min", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.size_start[0] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
            <span class="dim">–</span>
            <input
              type="number"
              step="0.5"
              min="0"
              value={pe.size_start[1]}
              onchange={(e) => {
                editEntity("Change size start max", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.size_start[1] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Size End</label>
            <input
              type="number"
              step="0.5"
              min="0"
              value={pe.size_end[0]}
              onchange={(e) => {
                editEntity("Change size end min", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.size_end[0] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
            <span class="dim">–</span>
            <input
              type="number"
              step="0.5"
              min="0"
              value={pe.size_end[1]}
              onchange={(e) => {
                editEntity("Change size end max", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.size_end[1] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Color Start</label>
            <input
              type="color"
              value={colorToHex(pe.color_start)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change particle color start", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.color_start = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      ent.components.particle_emitter.color_start[3],
                    ];
                });
              }}
            />
            <label>A</label>
            <input
              type="number"
              step="0.05"
              min="0"
              max="1"
              value={pe.color_start[3]}
              onchange={(e) => {
                editEntity("Change color start alpha", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.color_start[3] = Math.max(
                      0,
                      Math.min(
                        1,
                        parseFloat((e.target as HTMLInputElement).value) || 0,
                      ),
                    );
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Color End</label>
            <input
              type="color"
              value={colorToHex(pe.color_end)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change particle color end", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.color_end = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      ent.components.particle_emitter.color_end[3],
                    ];
                });
              }}
            />
            <label>A</label>
            <input
              type="number"
              step="0.05"
              min="0"
              max="1"
              value={pe.color_end[3]}
              onchange={(e) => {
                editEntity("Change color end alpha", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.color_end[3] = Math.max(
                      0,
                      Math.min(
                        1,
                        parseFloat((e.target as HTMLInputElement).value) || 0,
                      ),
                    );
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Gravity</label>
            <input
              type="number"
              step="0.1"
              value={pe.gravity_modifier}
              onchange={(e) => {
                editEntity("Change particle gravity", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.gravity_modifier =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Burst</label>
            <input
              type="number"
              step="1"
              min="0"
              value={pe.burst_count}
              onchange={(e) => {
                editEntity("Change burst count", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.burst_count =
                      parseInt((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>World Space</label>
            <input
              type="checkbox"
              checked={pe.world_space}
              onchange={(e) => {
                editEntity("Toggle world space", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.world_space = (
                      e.target as HTMLInputElement
                    ).checked;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Z Order</label>
            <input
              type="number"
              step="1"
              value={pe.z_order}
              onchange={(e) => {
                editEntity("Change particle z_order", (ent) => {
                  if (ent.components.particle_emitter)
                    ent.components.particle_emitter.z_order =
                      parseInt((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          {@render sortLayerPicker("particle_emitter", pe.sort_layer, (v) =>
            editEntity("Change particle sort_layer", (ent) => {
              if (ent.components.particle_emitter)
                ent.components.particle_emitter.sort_layer = v;
            }),
          )}
          <ParticleExtras
            emitter={pe}
            onchange={(field, value) =>
              editEntity(`Change particle ${field}`, (ent) => {
                if (ent.components.particle_emitter)
                  (ent.components.particle_emitter as Record<string, unknown>)[field] = value;
              })}
          />
          {@render undrawnFields("particle_emitter")}
        {/if}
      </div>
    {/if}

    <!-- UI Element -->
    {#if entity.components.ui_element}
      {@const ui = entity.components.ui_element}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("ui_element")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("ui_element");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["ui_element"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("ui_element")}<span class="section-title">UI Element</span>{@render overrideMark("ui_element")}<span class="section-key">ui_element</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("ui_element");
            }}
            title="Remove UI Element">×</button
          >
        </div>
        {#if !collapsed["ui_element"]}
          <div class="field-row">
            <label>Anchor</label>
            <select
              value={ui.anchor}
              onchange={(e) => {
                editEntity("Change UI anchor", (ent) => {
                  if (ent.components.ui_element)
                    ent.components.ui_element.anchor = (
                      e.target as HTMLSelectElement
                    ).value as typeof ui.anchor;
                });
              }}
            >
              <option value="TopLeft">Top Left</option>
              <option value="TopCenter">Top Center</option>
              <option value="TopRight">Top Right</option>
              <option value="CenterLeft">Center Left</option>
              <option value="Center">Center</option>
              <option value="CenterRight">Center Right</option>
              <option value="BottomLeft">Bottom Left</option>
              <option value="BottomCenter">Bottom Center</option>
              <option value="BottomRight">Bottom Right</option>
            </select>
          </div>
          <div class="field-row">
            <label>Offset X</label>
            <input
              type="number"
              step="1"
              value={ui.offset[0]}
              onchange={(e) => {
                editEntity("Change UI offset", (ent) => {
                  if (ent.components.ui_element)
                    ent.components.ui_element.offset[0] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
            <label>Y</label>
            <input
              type="number"
              step="1"
              value={ui.offset[1]}
              onchange={(e) => {
                editEntity("Change UI offset", (ent) => {
                  if (ent.components.ui_element)
                    ent.components.ui_element.offset[1] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Size W</label>
            <input
              type="number"
              step="1"
              min="0"
              value={ui.size[0]}
              onchange={(e) => {
                editEntity("Change UI size", (ent) => {
                  if (ent.components.ui_element)
                    ent.components.ui_element.size[0] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
            <label>H</label>
            <input
              type="number"
              step="1"
              min="0"
              value={ui.size[1]}
              onchange={(e) => {
                editEntity("Change UI size", (ent) => {
                  if (ent.components.ui_element)
                    ent.components.ui_element.size[1] =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Visible</label>
            <input
              type="checkbox"
              checked={ui.visible}
              onchange={(e) => {
                editEntity("Toggle UI visible", (ent) => {
                  if (ent.components.ui_element)
                    ent.components.ui_element.visible = (
                      e.target as HTMLInputElement
                    ).checked;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Z Order</label>
            <input
              type="number"
              step="1"
              value={ui.z_order}
              onchange={(e) => {
                editEntity("Change UI z_order", (ent) => {
                  if (ent.components.ui_element)
                    ent.components.ui_element.z_order =
                      parseInt((e.target as HTMLInputElement).value) || 0;
                });
              }}
            />
          </div>
          {@render undrawnFields("ui_element")}
        {/if}
      </div>
    {/if}

    <!-- UI Text -->
    {#if entity.components.ui_text}
      {@const uit = entity.components.ui_text}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("ui_text")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("ui_text");
          }}
        >
          <span class="collapse-arrow">{collapsed["ui_text"] ? "▸" : "▾"}</span>
          {@render sectionBar("ui_text")}<span class="section-title">UI Text</span>{@render overrideMark("ui_text")}<span class="section-key">ui_text</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("ui_text");
            }}
            title="Remove UI Text">×</button
          >
        </div>
        {@render depWarning("ui_text")}
        {#if !collapsed["ui_text"]}
          <div class="field-row">
            <label>Text</label>
            <input
              type="text"
              value={uit.text}
              onchange={(e) => {
                editEntity("Change UI text", (ent) => {
                  if (ent.components.ui_text)
                    ent.components.ui_text.text = (
                      e.target as HTMLInputElement
                    ).value;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Font</label>
            <select
              value={uit.font_id}
              onchange={(e) => {
                const v = (e.target as HTMLSelectElement).value;
                editEntity("Change UI font", (ent) => {
                  if (ent.components.ui_text) ent.components.ui_text.font_id = v;
                });
              }}
            >
              <option value={isBuiltinFontId(uit.font_id) ? uit.font_id : ""}>(none - built-in font)</option>
              {#each $projectFontIds as id (id)}
                <option value={id}>{id}</option>
              {/each}
              {#if uit.font_id && !isBuiltinFontId(uit.font_id) && !$projectFontIds.includes(uit.font_id)}
                <option value={uit.font_id}>{uit.font_id} (missing)</option>
              {/if}
            </select>
          </div>
          <div class="field-row">
            <label>Size</label>
            <input
              type="number"
              step="0.1"
              min="0.1"
              value={uit.font_size}
              onchange={(e) => {
                editEntity("Change UI font size", (ent) => {
                  if (ent.components.ui_text)
                    ent.components.ui_text.font_size =
                      parseFloat((e.target as HTMLInputElement).value) || 1;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Color</label>
            <input
              type="color"
              value={colorToHex(uit.color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change UI text color", (ent) => {
                  if (ent.components.ui_text)
                    ent.components.ui_text.color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      ent.components.ui_text.color[3],
                    ];
                });
              }}
            />
            <label>A</label>
            <input
              type="number"
              step="0.05"
              min="0"
              max="1"
              value={uit.color[3]}
              onchange={(e) => {
                editEntity("Change UI text alpha", (ent) => {
                  if (ent.components.ui_text)
                    ent.components.ui_text.color[3] = Math.max(
                      0,
                      Math.min(
                        1,
                        parseFloat((e.target as HTMLInputElement).value) || 0,
                      ),
                    );
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Align</label>
            <select
              value={uit.alignment}
              onchange={(e) => {
                editEntity("Change UI text align", (ent) => {
                  if (ent.components.ui_text)
                    ent.components.ui_text.alignment = (
                      e.target as HTMLSelectElement
                    ).value as "Left" | "Center" | "Right";
                });
              }}
            >
              <option value="Left">Left</option>
              <option value="Center">Center</option>
              <option value="Right">Right</option>
            </select>
          </div>
          <div class="field-row">
            <label title="Read the text as markup, as world text does: [color=#e05040] [b] [wave] [shake] [rainbow] [pause=0.5] [icon=sprites/key.png], each closed like [/color]. Off: brackets are just text.">Rich text</label>
            <input
              type="checkbox"
              checked={uit.rich ?? false}
              onchange={(e) => {
                editEntity("Change UI rich text", (ent) => {
                  if (!ent.components.ui_text) return;
                  if ((e.target as HTMLInputElement).checked) ent.components.ui_text.rich = true;
                  else delete ent.components.ui_text.rich;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label title="A typewriter: characters a second, waiting at each [pause], from whenever the text changes. 0 shows it all at once.">Reveal speed</label>
            <input
              type="number"
              step="1"
              min="0"
              value={uit.reveal_speed ?? 0}
              onchange={(e) => {
                editEntity("Change UI text reveal speed", (ent) => {
                  if (!ent.components.ui_text) return;
                  const v = Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0);
                  if (v > 0) ent.components.ui_text.reveal_speed = v;
                  else delete ent.components.ui_text.reveal_speed;
                });
              }}
            />
          </div>
          {@render undrawnFields("ui_text")}
        {/if}
      </div>
    {/if}

    <!-- UI Image -->
    {#if entity.components.ui_image}
      {@const uii = entity.components.ui_image}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("ui_image")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("ui_image");
          }}
        >
          <span class="collapse-arrow">{collapsed["ui_image"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("ui_image")}<span class="section-title">UI Image</span>{@render overrideMark("ui_image")}<span class="section-key">ui_image</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("ui_image");
            }}
            title="Remove UI Image">×</button
          >
        </div>
        {@render depWarning("ui_image")}
        {#if !collapsed["ui_image"]}
          <div class="field-row">
            <label>Texture</label>
            <input
              type="text"
              value={uii.texture_id}
              placeholder="sprites/image.png"
              onchange={(e) => {
                editEntity("Change UI image texture", (ent) => {
                  if (ent.components.ui_image)
                    ent.components.ui_image.texture_id = (
                      e.target as HTMLInputElement
                    ).value;
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Color</label>
            <input
              type="color"
              value={colorToHex(uii.color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change UI image color", (ent) => {
                  if (ent.components.ui_image)
                    ent.components.ui_image.color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      ent.components.ui_image.color[3],
                    ];
                });
              }}
            />
            <label>A</label>
            <input
              type="number"
              step="0.05"
              min="0"
              max="1"
              value={uii.color[3]}
              onchange={(e) => {
                editEntity("Change UI image alpha", (ent) => {
                  if (ent.components.ui_image)
                    ent.components.ui_image.color[3] = Math.max(
                      0,
                      Math.min(
                        1,
                        parseFloat((e.target as HTMLInputElement).value) || 0,
                      ),
                    );
                });
              }}
            />
          </div>
          {@render undrawnFields("ui_image")}
        {/if}
      </div>
    {/if}

    <!-- UI Panel -->
    {#if entity.components.ui_panel}
      {@const uip = entity.components.ui_panel}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("ui_panel")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("ui_panel");
          }}
        >
          <span class="collapse-arrow">{collapsed["ui_panel"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("ui_panel")}<span class="section-title">UI Panel</span>{@render overrideMark("ui_panel")}<span class="section-key">ui_panel</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("ui_panel");
            }}
            title="Remove UI Panel">×</button
          >
        </div>
        {@render depWarning("ui_panel")}
        {#if !collapsed["ui_panel"]}
          <div class="field-row">
            <label>BG Color</label>
            <input
              type="color"
              value={colorToHex(uip.bg_color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change panel bg color", (ent) => {
                  if (ent.components.ui_panel)
                    ent.components.ui_panel.bg_color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      ent.components.ui_panel.bg_color[3],
                    ];
                });
              }}
            />
            <label>A</label>
            <input
              type="number"
              step="0.05"
              min="0"
              max="1"
              value={uip.bg_color[3]}
              onchange={(e) => {
                editEntity("Change panel bg alpha", (ent) => {
                  if (ent.components.ui_panel)
                    ent.components.ui_panel.bg_color[3] = Math.max(
                      0,
                      Math.min(
                        1,
                        parseFloat((e.target as HTMLInputElement).value) || 0,
                      ),
                    );
                });
              }}
            />
          </div>
        {/if}
      </div>
    {/if}

    <!-- UI Button -->
    {#if entity.components.ui_button}
      {@const uib = entity.components.ui_button}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("ui_button")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("ui_button");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["ui_button"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("ui_button")}<span class="section-title">UI Button</span>{@render overrideMark("ui_button")}<span class="section-key">ui_button</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("ui_button");
            }}
            title="Remove UI Button">×</button
          >
        </div>
        {@render depWarning("ui_button")}
        {#if !collapsed["ui_button"]}
          <div class="field-row">
            <label>Normal</label>
            <input
              type="color"
              value={colorToHex(uib.normal_color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change button normal color", (ent) => {
                  if (ent.components.ui_button)
                    ent.components.ui_button.normal_color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      ent.components.ui_button.normal_color[3],
                    ];
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Hover</label>
            <input
              type="color"
              value={colorToHex(uib.hover_color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change button hover color", (ent) => {
                  if (ent.components.ui_button)
                    ent.components.ui_button.hover_color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      ent.components.ui_button.hover_color[3],
                    ];
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Pressed</label>
            <input
              type="color"
              value={colorToHex(uib.pressed_color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change button pressed color", (ent) => {
                  if (ent.components.ui_button)
                    ent.components.ui_button.pressed_color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      ent.components.ui_button.pressed_color[3],
                    ];
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Disabled</label>
            <input
              type="checkbox"
              checked={uib.disabled}
              onchange={(e) => {
                editEntity("Toggle button disabled", (ent) => {
                  if (ent.components.ui_button)
                    ent.components.ui_button.disabled = (
                      e.target as HTMLInputElement
                    ).checked;
                });
              }}
            />
          </div>
        {/if}
      </div>
    {/if}

    {#if entity.components.ui_progress_bar}
      {@const pb = entity.components.ui_progress_bar}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("ui_progress_bar")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("ui_progress_bar");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["ui_progress_bar"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("ui_progress_bar")}<span class="section-title">UI Progress Bar</span>{@render overrideMark("ui_progress_bar")}<span class="section-key">ui_progress_bar</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("ui_progress_bar");
            }}
            title="Remove">×</button
          >
        </div>
        {@render depWarning("ui_progress_bar")}
        {#if !collapsed["ui_progress_bar"]}
          <div class="field-row">
            <label>Value</label><input
              type="number"
              step="0.01"
              value={pb.value}
              onchange={(e) =>
                editEntity("Change progress value", (ent) => {
                  if (ent.components.ui_progress_bar)
                    ent.components.ui_progress_bar.value =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Min</label><input
              type="number"
              step="0.1"
              value={pb.min}
              onchange={(e) =>
                editEntity("Change progress min", (ent) => {
                  if (ent.components.ui_progress_bar)
                    ent.components.ui_progress_bar.min =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Max</label><input
              type="number"
              step="0.1"
              value={pb.max}
              onchange={(e) =>
                editEntity("Change progress max", (ent) => {
                  if (ent.components.ui_progress_bar)
                    ent.components.ui_progress_bar.max =
                      parseFloat((e.target as HTMLInputElement).value) || 1;
                })}
            />
          </div>
          <div class="field-row">
            <label>FG Color</label><input
              type="color"
              value={colorToHex(pb.fg_color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change progress fg", (ent) => {
                  if (ent.components.ui_progress_bar)
                    ent.components.ui_progress_bar.fg_color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      pb.fg_color[3],
                    ];
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>BG Color</label><input
              type="color"
              value={colorToHex(pb.bg_color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change progress bg", (ent) => {
                  if (ent.components.ui_progress_bar)
                    ent.components.ui_progress_bar.bg_color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      pb.bg_color[3],
                    ];
                });
              }}
            />
          </div>
          {@render undrawnFields("ui_progress_bar")}
        {/if}
      </div>
    {/if}

    {#if entity.components.ui_slider}
      {@const sl = entity.components.ui_slider}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("ui_slider")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("ui_slider");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["ui_slider"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("ui_slider")}<span class="section-title">UI Slider</span>{@render overrideMark("ui_slider")}<span class="section-key">ui_slider</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("ui_slider");
            }}
            title="Remove">×</button
          >
        </div>
        {@render depWarning("ui_slider")}
        {#if !collapsed["ui_slider"]}
          <div class="field-row">
            <label>Value</label><input
              type="number"
              step="0.01"
              value={sl.value}
              onchange={(e) =>
                editEntity("Change slider value", (ent) => {
                  if (ent.components.ui_slider)
                    ent.components.ui_slider.value =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Min</label><input
              type="number"
              step="0.1"
              value={sl.min}
              onchange={(e) =>
                editEntity("Change slider min", (ent) => {
                  if (ent.components.ui_slider)
                    ent.components.ui_slider.min =
                      parseFloat((e.target as HTMLInputElement).value) || 0;
                })}
            />
          </div>
          <div class="field-row">
            <label>Max</label><input
              type="number"
              step="0.1"
              value={sl.max}
              onchange={(e) =>
                editEntity("Change slider max", (ent) => {
                  if (ent.components.ui_slider)
                    ent.components.ui_slider.max =
                      parseFloat((e.target as HTMLInputElement).value) || 1;
                })}
            />
          </div>
          <div class="field-row">
            <label>Thumb Size</label><input
              type="number"
              step="1"
              value={sl.thumb_size}
              onchange={(e) =>
                editEntity("Change slider thumb", (ent) => {
                  if (ent.components.ui_slider)
                    ent.components.ui_slider.thumb_size =
                      parseFloat((e.target as HTMLInputElement).value) || 12;
                })}
            />
          </div>
          <div class="field-row">
            <label>Track Color</label><input
              type="color"
              value={colorToHex(sl.track_color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change slider track", (ent) => {
                  if (ent.components.ui_slider)
                    ent.components.ui_slider.track_color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      sl.track_color[3],
                    ];
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Thumb Color</label><input
              type="color"
              value={colorToHex(sl.thumb_color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change slider thumb color", (ent) => {
                  if (ent.components.ui_slider)
                    ent.components.ui_slider.thumb_color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      sl.thumb_color[3],
                    ];
                });
              }}
            />
          </div>
        {/if}
      </div>
    {/if}

    {#if entity.components.ui_toggle}
      {@const tg = entity.components.ui_toggle}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("ui_toggle")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("ui_toggle");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["ui_toggle"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("ui_toggle")}<span class="section-title">UI Toggle</span>{@render overrideMark("ui_toggle")}<span class="section-key">ui_toggle</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("ui_toggle");
            }}
            title="Remove">×</button
          >
        </div>
        {@render depWarning("ui_toggle")}
        {#if !collapsed["ui_toggle"]}
          <div class="field-row">
            <label>Checked</label><input
              type="checkbox"
              checked={tg.checked}
              onchange={(e) =>
                editEntity("Toggle checked", (ent) => {
                  if (ent.components.ui_toggle)
                    ent.components.ui_toggle.checked = (
                      e.target as HTMLInputElement
                    ).checked;
                })}
            />
          </div>
          <div class="field-row">
            <label>On Color</label><input
              type="color"
              value={colorToHex(tg.on_color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change toggle on color", (ent) => {
                  if (ent.components.ui_toggle)
                    ent.components.ui_toggle.on_color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      tg.on_color[3],
                    ];
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>Off Color</label><input
              type="color"
              value={colorToHex(tg.off_color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change toggle off color", (ent) => {
                  if (ent.components.ui_toggle)
                    ent.components.ui_toggle.off_color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      tg.off_color[3],
                    ];
                });
              }}
            />
          </div>
          {@render undrawnFields("ui_toggle")}
        {/if}
      </div>
    {/if}

    {#if entity.components.ui_text_input}
      {@const ti = entity.components.ui_text_input}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("ui_text_input")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ")
              toggleSection("ui_text_input");
          }}
        >
          <span class="collapse-arrow"
            >{collapsed["ui_text_input"] ? "▸" : "▾"}</span
          >
          {@render sectionBar("ui_text_input")}<span class="section-title">UI Text Input</span>{@render overrideMark("ui_text_input")}<span class="section-key">ui_text_input</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("ui_text_input");
            }}
            title="Remove">×</button
          >
        </div>
        {@render depWarning("ui_text_input")}
        {#if !collapsed["ui_text_input"]}
          <div class="field-row">
            <label>Text</label><input
              type="text"
              value={ti.text}
              onchange={(e) =>
                editEntity("Change text input text", (ent) => {
                  if (ent.components.ui_text_input)
                    ent.components.ui_text_input.text = (
                      e.target as HTMLInputElement
                    ).value;
                })}
            />
          </div>
          <div class="field-row">
            <label>Placeholder</label><input
              type="text"
              value={ti.placeholder}
              onchange={(e) =>
                editEntity("Change text input placeholder", (ent) => {
                  if (ent.components.ui_text_input)
                    ent.components.ui_text_input.placeholder = (
                      e.target as HTMLInputElement
                    ).value;
                })}
            />
          </div>
          <div class="field-row">
            <label>Font</label><select
              value={ti.font_id}
              onchange={(e) => {
                const v = (e.target as HTMLSelectElement).value;
                editEntity("Change text input font", (ent) => {
                  if (ent.components.ui_text_input)
                    ent.components.ui_text_input.font_id = v;
                });
              }}
            >
              <option value={isBuiltinFontId(ti.font_id) ? ti.font_id : ""}>(none - built-in font)</option>
              {#each $projectFontIds as id (id)}
                <option value={id}>{id}</option>
              {/each}
              {#if ti.font_id && !isBuiltinFontId(ti.font_id) && !$projectFontIds.includes(ti.font_id)}
                <option value={ti.font_id}>{ti.font_id} (missing)</option>
              {/if}
            </select>
          </div>
          <div class="field-row">
            <label>Font Size</label><input
              type="number"
              step="0.1"
              value={ti.font_size}
              onchange={(e) =>
                editEntity("Change text input font size", (ent) => {
                  if (ent.components.ui_text_input)
                    ent.components.ui_text_input.font_size =
                      parseFloat((e.target as HTMLInputElement).value) || 1;
                })}
            />
          </div>
          <div class="field-row">
            <label>Color</label><input
              type="color"
              value={colorToHex(ti.color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change text input color", (ent) => {
                  if (ent.components.ui_text_input)
                    ent.components.ui_text_input.color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      ti.color[3],
                    ];
                });
              }}
            />
          </div>
          <div class="field-row">
            <label>BG Color</label><input
              type="color"
              value={colorToHex(ti.bg_color)}
              onchange={(e) => {
                const hex = (e.target as HTMLInputElement).value;
                const r = parseInt(hex.slice(1, 3), 16) / 255;
                const g = parseInt(hex.slice(3, 5), 16) / 255;
                const b = parseInt(hex.slice(5, 7), 16) / 255;
                editEntity("Change text input bg", (ent) => {
                  if (ent.components.ui_text_input)
                    ent.components.ui_text_input.bg_color = [
                      Math.round(r * 100) / 100,
                      Math.round(g * 100) / 100,
                      Math.round(b * 100) / 100,
                      ti.bg_color[3],
                    ];
                });
              }}
            />
          </div>
          {@render undrawnFields("ui_text_input")}
        {/if}
      </div>
    {/if}

    {#if entity.components.gui}
      {@const gc = entity.components.gui}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection("gui")}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection("gui");
          }}
        >
          <span class="collapse-arrow">{collapsed["gui"] ? "▸" : "▾"}</span>
          {@render sectionBar("gui")}<span class="section-title">GUI Layout</span>{@render overrideMark("gui")}<span class="section-key">gui</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("gui");
            }}
            title="Remove">×</button
          >
        </div>
        {#if !collapsed["gui"]}
          <div class="field-row">
            <label>GUI ID</label>
            <div class="asset-picker-wrapper">
              <input
                type="text"
                value={gc.gui_id}
                placeholder="ui/file.gui"
                onchange={(e) =>
                  editEntity("Change GUI ID", (ent) => {
                    if (ent.components.gui)
                      ent.components.gui.gui_id = (
                        e.target as HTMLInputElement
                      ).value;
                  })}
              />
              <button
                class="picker-btn"
                onclick={() => (guiPickerOpen = !guiPickerOpen)}
                title="Browse GUI files">⊞</button
              >
              {#if guiPickerOpen && guiFiles.length > 0}
                <button
                  class="picker-backdrop"
                  onclick={() => (guiPickerOpen = false)}
                  aria-label="Close picker"
                ></button>
                <div class="picker-dropdown" role="listbox">
                  {#each guiFiles as file}
                    <button
                      class="picker-item"
                      class:active={gc.gui_id === file}
                      onclick={() => {
                        editEntity("Change GUI ID", (ent) => {
                          if (ent.components.gui)
                            ent.components.gui.gui_id = file;
                        });
                        guiPickerOpen = false;
                      }}>{file}</button
                    >
                  {/each}
                </div>
              {:else if guiPickerOpen}
                <button
                  class="picker-backdrop"
                  onclick={() => (guiPickerOpen = false)}
                  aria-label="Close picker"
                ></button>
                <div class="picker-dropdown">
                  <span class="picker-empty">No .gui files in project</span>
                </div>
              {/if}
            </div>
          </div>
          <div class="field-row">
            <label>Visible</label><input
              type="checkbox"
              checked={gc.visible}
              onchange={(e) =>
                editEntity("Toggle GUI visible", (ent) => {
                  if (ent.components.gui)
                    ent.components.gui.visible = (
                      e.target as HTMLInputElement
                    ).checked;
                })}
            />
          </div>
          <!-- Variables -->
          <div class="sub-section-header">Variables</div>
          {#if gc.variables && Object.keys(gc.variables).length > 0}
            {#each Object.entries(gc.variables) as [varKey, varVal]}
              <div class="field-row" style="gap:4px">
                <input
                  type="text"
                  value={varKey}
                  style="flex:1;min-width:60px"
                  placeholder="key"
                  onchange={(e) => {
                    const newKey = (e.target as HTMLInputElement).value.trim();
                    if (!newKey || newKey === varKey) return;
                    editEntity("Rename GUI variable", (ent) => {
                      if (!ent.components.gui?.variables) return;
                      const old = ent.components.gui.variables[varKey];
                      delete ent.components.gui.variables[varKey];
                      ent.components.gui.variables[newKey] = old;
                    });
                  }}
                />
                <input
                  type="text"
                  value={String(varVal ?? "")}
                  style="flex:1;min-width:60px"
                  placeholder="value"
                  onchange={(e) => {
                    const raw = (e.target as HTMLInputElement).value;
                    let parsed: unknown = raw;
                    if (raw === "true") parsed = true;
                    else if (raw === "false") parsed = false;
                    else if (raw !== "" && !isNaN(Number(raw)))
                      parsed = Number(raw);
                    editEntity("Change GUI variable", (ent) => {
                      if (ent.components.gui?.variables)
                        ent.components.gui.variables[varKey] = parsed;
                    });
                  }}
                />
                <button
                  class="remove-btn"
                  title="Remove variable"
                  onclick={() =>
                    editEntity("Remove GUI variable", (ent) => {
                      if (ent.components.gui?.variables)
                        delete ent.components.gui.variables[varKey];
                    })}>×</button
                >
              </div>
            {/each}
          {/if}
          <button
            class="add-btn"
            onclick={() =>
              editEntity("Add GUI variable", (ent) => {
                if (!ent.components.gui) return;
                if (!ent.components.gui.variables)
                  ent.components.gui.variables = {};
                let key = "var0";
                let i = 1;
                while (key in ent.components.gui.variables) {
                  key = `var${i++}`;
                }
                ent.components.gui.variables[key] = "";
              })}>+ Add Variable</button
          >
        {/if}
      </div>
    {/if}

    <!-- Tags Section -->
    <div class="section">
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="section-header"
        role="button"
        tabindex="0"
        onclick={() => toggleSection("tags")}
        onkeydown={(e) => {
          if (e.key === "Enter" || e.key === " ") toggleSection("tags");
        }}
      >
        <span class="collapse-arrow">{collapsed["tags"] ? "▸" : "▾"}</span>
        {@render sectionBar("tags")}<span class="section-title">Tags</span>{@render overrideMark("tags")}<span class="section-key">tags</span>
      </div>
      {#if !collapsed["tags"]}
        <div class="tags-container">
          {#if entity.components.tags && entity.components.tags.length > 0}
            {#each entity.components.tags as tag}
              <span class="tag-chip">
                {tag}
                <button
                  class="tag-remove"
                  onclick={() => handleRemoveTag(tag)}
                  title="Remove tag">×</button
                >
              </span>
            {/each}
          {/if}
          <div class="tag-add-row">
            <input
              type="text"
              class="tag-input"
              placeholder="Add tag..."
              bind:value={newTagInput}
              onkeydown={(e) => {
                if (e.key === "Enter") handleAddTag();
              }}
            />
            <button
              class="tag-add-btn"
              onclick={handleAddTag}
              disabled={!newTagInput.trim()}>+</button
            >
          </div>
        </div>
      {/if}
    </div>

    {#each packageComps as pc (pc.name)}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection(pc.name)}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection(pc.name);
          }}
        >
          <span class="collapse-arrow">{collapsed[pc.name] ? "▸" : "▾"}</span>
          {@render sectionBar(pc.name)}<span class="section-title">{pc.info.component_name}</span>{@render overrideMark(pc.name)}<span class="section-key">{pc.name}</span>
          <span style="font-size:10px;color:var(--ed-muted);margin-left:auto;"
            >📦 {pc.info.category}</span
          >
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent(pc.name);
            }}
            title="Remove {pc.name}">×</button
          >
        </div>
        {#if !collapsed[pc.name] && pc.name === "behavior_tree"}
          <!-- The tree has its own editor; the rest of the component (enabled,
               loop) the derived widgets. -->
          <PackageInspector
            overridden={(f) => fieldChanged(entityChanges, pc.name, f)}
            mixed={(f) => mixedOf(pc.name).includes(f)}
            info={{ ...pc.info, fields: pc.info.fields.filter((f) => f.name !== "root") }}
            data={pc.data}
            disabled={playing}
            onchange={(field, value) => setComponentField(pc.name, field, value)}
          />
          <BehaviorTreeEditor
            root={pc.data.root as BtNode | undefined}
            running={runningLeaves}
            disabled={playing}
            onchange={(root) => setComponentField(pc.name, "root", root)}
          />
        {:else if !collapsed[pc.name] && (pc.name === "skeleton_root" || pc.name === "skinned_mesh")}
          <!-- A skin's vertices are thousands of numbers: summed up here,
               edited in the skin editor. -->
          <PackageInspector
            overridden={(f) => fieldChanged(entityChanges, pc.name, f)}
            mixed={(f) => mixedOf(pc.name).includes(f)}
            info={{ ...pc.info, fields: pc.info.fields.filter((f) => !["bones", "vertices", "indices"].includes(f.name)) }}
            data={pc.data}
            disabled={playing}
            onchange={(field, value) => setComponentField(pc.name, field, value)}
          />
          <div class="skin-row">
            {#if pc.name === "skinned_mesh"}
              <span class="skin-sum"
                >{(pc.data.vertices as unknown[] | undefined)?.length ?? 0} vertices, {((pc.data.indices as unknown[] | undefined)?.length ?? 0) / 3}
                triangles, {(pc.data.bones as unknown[] | undefined)?.length ?? 0} bones</span
              >
            {/if}
            <button
              disabled={playing || !(entity?.components as Record<string, unknown>)?.skeleton_root}
              title="Draw bones over a picture, build its mesh and paint its weights"
              onclick={() =>
                window.dispatchEvent(new CustomEvent("open-skin-editor", { detail: { index: $selectedEntity } }))}
              >🦴 Skin & rig…</button
            >
          </div>
        {:else if !collapsed[pc.name] && pc.name === "skeletal_player"}
          <!-- The blend space and layers have their own editors (a plot, a
               list with bone masks); the rest the derived widgets. -->
          <PackageInspector
            overridden={(f) => fieldChanged(entityChanges, pc.name, f)}
            mixed={(f) => mixedOf(pc.name).includes(f)}
            info={{ ...pc.info, fields: pc.info.fields.filter((f) => f.name !== "blend_space" && f.name !== "layers" && f.name !== "fade") }}
            data={pc.data}
            disabled={playing}
            onchange={(field, value) => setComponentField(pc.name, field, value)}
          />
          <SkeletalPlayerExtras
            data={pc.data}
            bones={skeletonBones}
            clips={skeletalClips}
            disabled={playing}
            onchange={(field, value) => setComponentField(pc.name, field, value)}
          />
        {:else if !collapsed[pc.name]}
          <PackageInspector
            overridden={(f) => fieldChanged(entityChanges, pc.name, f)}
            mixed={(f) => mixedOf(pc.name).includes(f)}
            info={pc.info}
            data={pc.data}
            disabled={playing}
            onchange={(field, value) => setComponentField(pc.name, field, value)}
            oneditpoints={(field) => editPointsInViewport(pc.name, field)}
          />
        {/if}
      </div>
    {/each}

    {#each customComps as cc (cc.name)}
      <div class="section">
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="section-header"
          role="button"
          tabindex="0"
          onclick={() => toggleSection(cc.name)}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") toggleSection(cc.name);
          }}
        >
          <span class="collapse-arrow">{collapsed[cc.name] ? "▸" : "▾"}</span>
          {@render sectionBar("custom")}<span class="section-title">{cc.name}</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent(cc.name);
            }}
            title="Remove {cc.name}">×</button
          >
        </div>
        {#if !collapsed[cc.name]}
          {#if cc.def}
            {#each cc.def.fields as fieldDef}
              <div class="field-row">
                <label>{fieldDef.name}</label>
                {#if fieldDef.field_type === "Float"}
                  <input
                    type="number"
                    step="0.1"
                    value={cc.data[fieldDef.name] ?? 0}
                    onchange={(e) =>
                      handleCustomFieldChange(
                        cc.name,
                        fieldDef.name,
                        parseFloat((e.target as HTMLInputElement).value) || 0,
                      )}
                  />
                {:else if fieldDef.field_type === "Int"}
                  <input
                    type="number"
                    step="1"
                    value={cc.data[fieldDef.name] ?? 0}
                    onchange={(e) =>
                      handleCustomFieldChange(
                        cc.name,
                        fieldDef.name,
                        parseInt((e.target as HTMLInputElement).value) || 0,
                      )}
                  />
                {:else if fieldDef.field_type === "String"}
                  <input
                    type="text"
                    value={cc.data[fieldDef.name] ?? ""}
                    onchange={(e) =>
                      handleCustomFieldChange(
                        cc.name,
                        fieldDef.name,
                        (e.target as HTMLInputElement).value,
                      )}
                  />
                {:else if fieldDef.field_type === "Bool"}
                  <input
                    type="checkbox"
                    checked={!!cc.data[fieldDef.name]}
                    onchange={(e) =>
                      handleCustomFieldChange(
                        cc.name,
                        fieldDef.name,
                        (e.target as HTMLInputElement).checked,
                      )}
                  />
                {:else if fieldDef.field_type === "Vec2"}
                  {@const vec = Array.isArray(cc.data[fieldDef.name])
                    ? (cc.data[fieldDef.name] as number[])
                    : [0, 0]}
                  <input
                    type="number"
                    step="0.1"
                    value={vec[0] ?? 0}
                    onchange={(e) =>
                      handleCustomFieldChange(cc.name, fieldDef.name, [
                        parseFloat((e.target as HTMLInputElement).value) || 0,
                        vec[1] ?? 0,
                      ])}
                  />
                  <input
                    type="number"
                    step="0.1"
                    value={vec[1] ?? 0}
                    onchange={(e) =>
                      handleCustomFieldChange(cc.name, fieldDef.name, [
                        vec[0] ?? 0,
                        parseFloat((e.target as HTMLInputElement).value) || 0,
                      ])}
                  />
                {:else if fieldDef.field_type === "Color"}
                  {@const col = Array.isArray(cc.data[fieldDef.name])
                    ? (cc.data[fieldDef.name] as number[])
                    : [1, 1, 1, 1]}
                  <input
                    type="color"
                    value={colorToHex(
                      col.length >= 3
                        ? [col[0], col[1], col[2], col[3] ?? 1]
                        : [1, 1, 1, 1],
                    )}
                    onchange={(e) => {
                      const hex = (e.target as HTMLInputElement).value;
                      const cr = parseInt(hex.slice(1, 3), 16) / 255;
                      const cg = parseInt(hex.slice(3, 5), 16) / 255;
                      const cb = parseInt(hex.slice(5, 7), 16) / 255;
                      handleCustomFieldChange(cc.name, fieldDef.name, [
                        Math.round(cr * 100) / 100,
                        Math.round(cg * 100) / 100,
                        Math.round(cb * 100) / 100,
                        col[3] ?? 1,
                      ]);
                    }}
                  />
                  <label for="custom-color-alpha-{cc.name}-{fieldDef.name}"
                    >A</label
                  >
                  <input
                    id="custom-color-alpha-{cc.name}-{fieldDef.name}"
                    type="number"
                    step="0.05"
                    min="0"
                    max="1"
                    value={col[3] ?? 1}
                    onchange={(e) => {
                      const a = Math.max(
                        0,
                        Math.min(
                          1,
                          parseFloat((e.target as HTMLInputElement).value) || 0,
                        ),
                      );
                      handleCustomFieldChange(cc.name, fieldDef.name, [
                        col[0] ?? 1,
                        col[1] ?? 1,
                        col[2] ?? 1,
                        a,
                      ]);
                    }}
                  />
                {/if}
              </div>
            {/each}
          {:else}
            <div class="dim" style="padding: 4px 0; font-size: 11px;">
              No schema — raw data attached
            </div>
          {/if}
        {/if}
      </div>
    {/each}

    {#if addableComponents.length > 0}
      <div class="section add-section">
        <div class="add-component-wrapper">
          <button
            class="add-component-btn"
            onclick={() => {
              showAddMenu = !showAddMenu;
              if (!showAddMenu) componentSearch = "";
            }}
            disabled={playing}
          >
            + Add Component
          </button>
          {#if showAddMenu}
            <div class="add-menu">
              <div class="add-menu-search">
                <!-- svelte-ignore a11y_autofocus -->
                <input
                  class="add-menu-search-input"
                  type="text"
                  placeholder="Search components…"
                  autofocus
                  bind:value={componentSearch}
                  onclick={(e) => e.stopPropagation()}
                />
              </div>
              {#each filteredCategories as cat}
                {@const isSearching = componentSearch.trim().length > 0}
                <button
                  type="button"
                  class="add-menu-category"
                  onclick={() =>
                    (expandedCategories[cat.key] =
                      !expandedCategories[cat.key])}
                >
                  <span class="cat-arrow"
                    >{isSearching || expandedCategories[cat.key]
                      ? "▾"
                      : "▸"}</span
                  >
                  <span class="cat-icon">{cat.icon}</span>
                  {cat.label}
                  <span class="cat-count">{cat.items.length}</span>
                </button>
                {#if isSearching || expandedCategories[cat.key]}
                  {#each cat.items as comp}
                    <button
                      class="add-menu-item"
                      onclick={() => {
                        handleAddComponent(comp);
                        showAddMenu = false;
                        componentSearch = "";
                      }}
                    >
                      {comp.charAt(0).toUpperCase() +
                        comp.slice(1).replace(/_/g, " ")}
                    </button>
                  {/each}
                  {#if cat.key === "custom"}
                    <button
                      class="add-menu-new-custom"
                      onclick={() => {
                        showAddMenu = false;
                        componentSearch = "";
                        onmanagecomponents?.();
                      }}
                    >
                      ✚ New Custom Component…
                    </button>
                  {/if}
                {/if}
              {/each}
              {#if filteredCategories.length === 0 || (filteredCategories.length === 1 && filteredCategories[0].key === "custom" && filteredCategories[0].items.length === 0)}
                <div class="add-menu-no-results">No components found</div>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    {/if}
  {:else if $shaderEditorGraph}
    <div class="header">{shaderNode ? "Shader Node" : "Shader Editor"}</div>

    <!-- Main Shader Preview -->
    <div class="section" style="padding: 8px;">
      <div
        class="section-title"
        style="cursor:pointer; display:flex; align-items:center; gap:4px; user-select:none;"
        onclick={() => toggleSection("shader-preview")}
        onkeydown={() => {}}
        role="button"
        tabindex="0"
      >
        <span
          style="font-size:9px; display:inline-block; transform: rotate({collapsed[
            'shader-preview'
          ]
            ? '0'
            : '90'}deg); transition: transform 0.1s;">▶</span
        >
        MAIN PREVIEW
      </div>
      {#if !collapsed["shader-preview"]}
        <div style="margin-top: 6px; position: relative;">
          <canvas
            bind:this={previewCanvas}
            width={240}
            height={240}
            style="width: 100%; aspect-ratio: 1; border-radius: 4px; border: 1px solid var(--ed-line); background: var(--ed-crust); display: block;"
          ></canvas>
          {#if previewCompiling}
            <div
              style="position: absolute; top: 50%; left: 50%; transform: translate(-50%, -50%); color: var(--ed-muted); font-size: 11px;"
            >
              Compiling...
            </div>
          {/if}
          {#if previewError}
            <div
              style="margin-top: 4px; font-size: 10px; color: var(--ed-err); max-height: 60px; overflow: auto; word-break: break-all;"
            >
              {previewError}
            </div>
          {/if}
          <button
            style="margin-top: 4px; width: 100%; padding: 3px 6px; font-size: 11px; background: var(--ed-raised); color: var(--ed-text); border: 1px solid var(--ed-line-strong); border-radius: 3px; cursor: pointer;"
            onclick={updatePreview}>Refresh Preview</button
          >
        </div>
      {/if}
    </div>

    {#if shaderNode}
      <div class="section">
        <div class="section-title">
          {shaderNodeTypeLabel(shaderNode.node_type)}
        </div>
        <div class="section-body" style="padding: 6px 8px;">
          <div class="field-row">
            <span class="field-label">Label</span>
            <input
              class="field-input"
              type="text"
              value={shaderNode.label || ""}
              oninput={(e) => {
                if (!shaderNode) return;
                shaderNode.label =
                  (e.currentTarget as HTMLInputElement).value || undefined;
                shaderNodeEdit();
              }}
              placeholder={shaderNodeTypeLabel(shaderNode.node_type)}
            />
          </div>

          {#if shaderNode.node_type.type === "FloatConstant"}
            <div class="field-row">
              <span class="field-label">Value</span>
              <input
                class="field-input"
                type="number"
                step="0.01"
                value={shaderNode.node_type.value}
                oninput={(e) => {
                  if (shaderNode?.node_type.type === "FloatConstant") {
                    shaderNode.node_type.value =
                      parseFloat((e.currentTarget as HTMLInputElement).value) ||
                      0;
                    shaderNodeEdit();
                  }
                }}
              />
            </div>
          {/if}

          {#if shaderNode.node_type.type === "ColorInput"}
            <div class="field-row">
              <span class="field-label">Color</span>
              <input
                class="field-input"
                type="color"
                value={ppVec4ToHex(shaderNode.node_type.color ?? [1, 1, 1, 1])}
                oninput={(e) => {
                  if (shaderNode?.node_type.type === "ColorInput") {
                    const rgb = ppHexToVec4(
                      (e.currentTarget as HTMLInputElement).value,
                    );
                    shaderNode.node_type.color = [
                      rgb[0],
                      rgb[1],
                      rgb[2],
                      (shaderNode.node_type.color ?? [1, 1, 1, 1])[3],
                    ];
                    shaderNodeEdit();
                  }
                }}
              />
            </div>
            <div class="field-row">
              <span class="field-label">Alpha</span>
              <input
                class="field-input"
                type="number"
                step="0.01"
                min="0"
                max="1"
                value={(shaderNode.node_type.color ?? [1, 1, 1, 1])[3]}
                oninput={(e) => {
                  if (shaderNode?.node_type.type === "ColorInput") {
                    if (!shaderNode.node_type.color)
                      shaderNode.node_type.color = [1, 1, 1, 1];
                    shaderNode.node_type.color[3] =
                      parseFloat((e.currentTarget as HTMLInputElement).value) ||
                      0;
                    shaderNodeEdit();
                  }
                }}
              />
            </div>
          {/if}

          {#if shaderNode.node_type.type === "Vec2Constant"}
            <div class="field-row">
              <span class="field-label">X</span>
              <input
                class="field-input"
                type="number"
                step="0.01"
                value={shaderNode.node_type.value[0]}
                oninput={(e) => {
                  if (shaderNode?.node_type.type === "Vec2Constant") {
                    shaderNode.node_type.value[0] =
                      parseFloat((e.currentTarget as HTMLInputElement).value) ||
                      0;
                    shaderNodeEdit();
                  }
                }}
              />
            </div>
            <div class="field-row">
              <span class="field-label">Y</span>
              <input
                class="field-input"
                type="number"
                step="0.01"
                value={shaderNode.node_type.value[1]}
                oninput={(e) => {
                  if (shaderNode?.node_type.type === "Vec2Constant") {
                    shaderNode.node_type.value[1] =
                      parseFloat((e.currentTarget as HTMLInputElement).value) ||
                      0;
                    shaderNodeEdit();
                  }
                }}
              />
            </div>
          {/if}

          {#if shaderNode.node_type.type === "Vec3Constant"}
            <div class="field-row">
              <span class="field-label">Color</span>
              <input
                class="field-input"
                type="color"
                value={ppVec4ToHex([...shaderNode.node_type.value, 1])}
                oninput={(e) => {
                  if (shaderNode?.node_type.type === "Vec3Constant") {
                    const rgb = ppHexToVec4(
                      (e.currentTarget as HTMLInputElement).value,
                    );
                    shaderNode.node_type.value = [rgb[0], rgb[1], rgb[2]];
                    shaderNodeEdit();
                  }
                }}
              />
            </div>
            <div class="field-row">
              <span class="field-label">R</span>
              <input
                class="field-input"
                type="number"
                step="0.01"
                min="0"
                max="1"
                value={shaderNode.node_type.value[0]}
                oninput={(e) => {
                  if (shaderNode?.node_type.type === "Vec3Constant") {
                    shaderNode.node_type.value[0] =
                      parseFloat((e.currentTarget as HTMLInputElement).value) ||
                      0;
                    shaderNodeEdit();
                  }
                }}
              />
            </div>
            <div class="field-row">
              <span class="field-label">G</span>
              <input
                class="field-input"
                type="number"
                step="0.01"
                min="0"
                max="1"
                value={shaderNode.node_type.value[1]}
                oninput={(e) => {
                  if (shaderNode?.node_type.type === "Vec3Constant") {
                    shaderNode.node_type.value[1] =
                      parseFloat((e.currentTarget as HTMLInputElement).value) ||
                      0;
                    shaderNodeEdit();
                  }
                }}
              />
            </div>
            <div class="field-row">
              <span class="field-label">B</span>
              <input
                class="field-input"
                type="number"
                step="0.01"
                min="0"
                max="1"
                value={shaderNode.node_type.value[2]}
                oninput={(e) => {
                  if (shaderNode?.node_type.type === "Vec3Constant") {
                    shaderNode.node_type.value[2] =
                      parseFloat((e.currentTarget as HTMLInputElement).value) ||
                      0;
                    shaderNodeEdit();
                  }
                }}
              />
            </div>
          {/if}

          {#if shaderNode.node_type.type === "Vec4Constant"}
            <div class="field-row">
              <span class="field-label">Color</span>
              <input
                class="field-input"
                type="color"
                value={ppVec4ToHex(shaderNode.node_type.value)}
                oninput={(e) => {
                  if (shaderNode?.node_type.type === "Vec4Constant") {
                    const rgb = ppHexToVec4(
                      (e.currentTarget as HTMLInputElement).value,
                    );
                    shaderNode.node_type.value = [
                      rgb[0],
                      rgb[1],
                      rgb[2],
                      shaderNode.node_type.value[3],
                    ];
                    shaderNodeEdit();
                  }
                }}
              />
            </div>
            <div class="field-row">
              <span class="field-label">Alpha</span>
              <input
                class="field-input"
                type="number"
                step="0.01"
                min="0"
                max="1"
                value={shaderNode.node_type.value[3]}
                oninput={(e) => {
                  if (shaderNode?.node_type.type === "Vec4Constant") {
                    shaderNode.node_type.value[3] =
                      parseFloat((e.currentTarget as HTMLInputElement).value) ||
                      0;
                    shaderNodeEdit();
                  }
                }}
              />
            </div>
          {/if}

          {#if shaderNode.node_type.type === "CustomGlsl"}
            <div
              class="field-row"
              style="flex-direction: column; align-items: stretch;"
            >
              <span class="field-label" style="margin-bottom: 4px;"
                >WGSL code - read inputs and assign outputs by pin name</span
              >
              <textarea
                style="width: 100%; padding: 4px 6px; font-size: 11px; font-family: 'Cascadia Code', 'Fira Code', monospace; background: var(--ed-base); color: var(--ed-ok); border: 1px solid var(--ed-line); border-radius: 3px; outline: none; resize: vertical;"
                value={shaderNode.node_type.code}
                oninput={(e) => {
                  if (shaderNode?.node_type.type === "CustomGlsl") {
                    shaderNode.node_type.code = (
                      e.currentTarget as HTMLTextAreaElement
                    ).value;
                    shaderNodeEdit();
                  }
                }}
                rows={8}
              ></textarea>
            </div>

            <!-- Custom Inputs -->
            <div style="margin-top: 8px; margin-bottom: 4px;">
              <div
                style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 4px;"
              >
                <span
                  class="field-label"
                  style="font-size: 11px; text-transform: uppercase; color: var(--ed-muted);"
                  >Inputs</span
                >
                <button
                  style="background: none; border: 1px solid var(--ed-line-strong); color: var(--ed-ok); font-size: 11px; padding: 1px 6px; border-radius: 3px; cursor: pointer;"
                  onclick={() => {
                    if (shaderNode?.node_type.type === "CustomGlsl") {
                      shaderNode.node_type.inputs = [
                        ...shaderNode.node_type.inputs,
                        {
                          name: `in${shaderNode.node_type.inputs.length}`,
                          pin_type: "Float" as PinType,
                        },
                      ];
                      shaderNodeEdit();
                    }
                  }}>+ Add</button
                >
              </div>
              {#each shaderNode.node_type.inputs as pin, i}
                <div
                  style="display: flex; align-items: center; gap: 4px; margin-bottom: 3px;"
                >
                  <input
                    style="flex: 1; min-width: 0; background: var(--ed-raised); border: 1px solid var(--ed-line-strong); border-radius: 3px; color: var(--ed-text); padding: 2px 4px; font-size: 11px;"
                    type="text"
                    value={pin.name}
                    oninput={(e) => {
                      if (shaderNode?.node_type.type === "CustomGlsl") {
                        shaderNode.node_type.inputs[i].name = (
                          e.currentTarget as HTMLInputElement
                        ).value;
                        shaderNodeEdit();
                      }
                    }}
                    placeholder="name"
                  />
                  <select
                    style="background: var(--ed-raised); border: 1px solid var(--ed-line-strong); border-radius: 3px; color: var(--ed-text); padding: 2px 4px; font-size: 11px; width: 70px;"
                    value={pin.pin_type}
                    onchange={(e) => {
                      if (shaderNode?.node_type.type === "CustomGlsl") {
                        shaderNode.node_type.inputs[i].pin_type = (
                          e.currentTarget as HTMLSelectElement
                        ).value as PinType;
                        shaderNodeEdit();
                      }
                    }}
                  >
                    <option value="Float">Float</option>
                    <option value="Vec2">Vec2</option>
                    <option value="Vec3">Vec3</option>
                    <option value="Vec4">Vec4</option>
                  </select>
                  <button
                    style="background: none; border: none; color: var(--ed-err); font-size: 14px; cursor: pointer; padding: 0 2px; line-height: 1;"
                    title="Remove input"
                    onclick={() => {
                      if (shaderNode?.node_type.type === "CustomGlsl") {
                        shaderNode.node_type.inputs =
                          shaderNode.node_type.inputs.filter(
                            (_, idx) => idx !== i,
                          );
                        shaderNodeEdit();
                      }
                    }}>&times;</button
                  >
                </div>
              {/each}
            </div>

            <!-- Custom Outputs -->
            <div style="margin-top: 8px; margin-bottom: 4px;">
              <div
                style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 4px;"
              >
                <span
                  class="field-label"
                  style="font-size: 11px; text-transform: uppercase; color: var(--ed-muted);"
                  >Outputs</span
                >
                <button
                  style="background: none; border: 1px solid var(--ed-line-strong); color: var(--ed-ok); font-size: 11px; padding: 1px 6px; border-radius: 3px; cursor: pointer;"
                  onclick={() => {
                    if (shaderNode?.node_type.type === "CustomGlsl") {
                      shaderNode.node_type.outputs = [
                        ...shaderNode.node_type.outputs,
                        {
                          name: `out${shaderNode.node_type.outputs.length}`,
                          pin_type: "Float" as PinType,
                        },
                      ];
                      shaderNodeEdit();
                    }
                  }}>+ Add</button
                >
              </div>
              {#each shaderNode.node_type.outputs as pin, i}
                <div
                  style="display: flex; align-items: center; gap: 4px; margin-bottom: 3px;"
                >
                  <input
                    style="flex: 1; min-width: 0; background: var(--ed-raised); border: 1px solid var(--ed-line-strong); border-radius: 3px; color: var(--ed-text); padding: 2px 4px; font-size: 11px;"
                    type="text"
                    value={pin.name}
                    oninput={(e) => {
                      if (shaderNode?.node_type.type === "CustomGlsl") {
                        shaderNode.node_type.outputs[i].name = (
                          e.currentTarget as HTMLInputElement
                        ).value;
                        shaderNodeEdit();
                      }
                    }}
                    placeholder="name"
                  />
                  <select
                    style="background: var(--ed-raised); border: 1px solid var(--ed-line-strong); border-radius: 3px; color: var(--ed-text); padding: 2px 4px; font-size: 11px; width: 70px;"
                    value={pin.pin_type}
                    onchange={(e) => {
                      if (shaderNode?.node_type.type === "CustomGlsl") {
                        shaderNode.node_type.outputs[i].pin_type = (
                          e.currentTarget as HTMLSelectElement
                        ).value as PinType;
                        shaderNodeEdit();
                      }
                    }}
                  >
                    <option value="Float">Float</option>
                    <option value="Vec2">Vec2</option>
                    <option value="Vec3">Vec3</option>
                    <option value="Vec4">Vec4</option>
                  </select>
                  <button
                    style="background: none; border: none; color: var(--ed-err); font-size: 14px; cursor: pointer; padding: 0 2px; line-height: 1;"
                    title="Remove output"
                    onclick={() => {
                      if (shaderNode?.node_type.type === "CustomGlsl") {
                        shaderNode.node_type.outputs =
                          shaderNode.node_type.outputs.filter(
                            (_, idx) => idx !== i,
                          );
                        shaderNodeEdit();
                      }
                    }}>&times;</button
                  >
                </div>
              {/each}
            </div>
          {/if}

          {#if shaderNode.node_type.type === "Combine"}
            <div class="field-row">
              <span class="field-label">Components</span>
              <select
                class="field-input"
                value={shaderNode.node_type.components}
                onchange={(e) => {
                  if (shaderNode?.node_type.type === "Combine") {
                    shaderNode.node_type.components = parseInt(
                      (e.currentTarget as HTMLSelectElement).value,
                    );
                    shaderNodeEdit();
                  }
                }}
              >
                <option value={2}>Vec2 (2)</option>
                <option value={3}>Vec3 (3)</option>
                <option value={4}>Vec4 (4)</option>
              </select>
            </div>
          {/if}
        </div>
      </div>
    {/if}
  {:else if $selectedEntities.size > 1}
    <div class="empty">{$selectedEntities.size} entities selected</div>
  {:else if $sceneSelected}
    <!-- Scene-level settings, shown when the Scene panel's scene row is
         selected. Entities put their components here the same way. -->
    <div class="header">Scene Settings</div>
    <div class="scene-subtitle">
      {$scenePath?.split(/[\\/]/).pop() ?? $scene?.name ?? "Untitled Scene"}
    </div>

    <!-- ── Sort Layers (scene-level) ── -->
    <div class="section" style="margin-top: 8px;">
      <div
        class="section-title"
        style="cursor:pointer; display:flex; align-items:center; gap:4px; user-select:none;"
        onclick={() => toggleSection("sortlayers")}
        onkeydown={() => {}}
        role="button"
        tabindex="0"
      >
        <span
          style="font-size:9px; display:inline-block; transform: rotate({collapsed[
            'sortlayers'
          ]
            ? '0'
            : '90'}deg); transition: transform 0.1s;">▶</span
        >
        SORT LAYERS
      </div>
      {#if !collapsed["sortlayers"]}
        <div class="section-body" style="padding: 6px 8px;">
          <div class="field-row">
            <label style="font-size:12px;">Asset</label>
            <select
              value={$scene?.sort_layers ?? ""}
              disabled={playing}
              onchange={(e) => {
                const v = (e.target as HTMLSelectElement).value;
                execute("Set scene sort_layers", (s) => {
                  s.sort_layers = v || undefined;
                });
              }}
            >
              <option value="">(none — z_order only)</option>
              {#each projectSortLayerIds as id (id)}
                <option value={id}>{id}</option>
              {/each}
              {#if $scene?.sort_layers && !projectSortLayerIds.includes($scene.sort_layers)}
                <option value={$scene.sort_layers}
                  >{$scene.sort_layers} (missing)</option
                >
              {/if}
            </select>
          </div>
          <div
            class="dim"
            style="padding: 4px 0 0; font-size: 11px; line-height: 1.4;"
          >
            {#if $scene?.sort_layers}
              {activeSortLayerNames.length} layer{activeSortLayerNames.length ===
              1
                ? ""
                : "s"} — components can pick named layers to add a base z offset.
            {:else}
              Pick a <code>.sortlayers</code> asset to enable named layers (effective
              z = base + z_order). Without one, only the per-component z_order is
              used.
            {/if}
          </div>
        </div>
      {/if}
    </div>

    <!-- ── Post-Processing Settings (scene-level) ── -->
    <div class="section" style="margin-top: 8px;">
      <div
        class="section-title"
        style="cursor:pointer; display:flex; align-items:center; gap:4px; user-select:none;"
        onclick={() => toggleSection("postprocess")}
        onkeydown={() => {}}
        role="button"
        tabindex="0"
      >
        <span
          style="font-size:9px; display:inline-block; transform: rotate({collapsed[
            'postprocess'
          ]
            ? '0'
            : '90'}deg); transition: transform 0.1s;">▶</span
        >
        POST-PROCESSING
      </div>
      {#if !collapsed["postprocess"]}
        <div class="section-body" style="padding: 6px 8px;">
          <div
            style="margin-bottom: 8px; padding: 6px; background: var(--ed-mantle); border-radius: 4px; border: 1px solid var(--ed-line);"
          >
            <label
              style="font-size:12px; display:flex; align-items:center; gap:4px; cursor:pointer;"
              title="Light and glows may exceed white; the tonemap brings them back into range. Off: light stops at 1.0, as it always has."
            >
              <input
                type="checkbox"
                checked={!!$scene.post_process?.hdr}
                disabled={playing}
                onchange={(e) => ppSetHdr({ hdr: (e.target as HTMLInputElement).checked })}
              />
              <strong>HDR</strong>
            </label>
            {#if $scene.post_process?.hdr}
              <div class="field-row">
                <span class="field-label">Tonemap</span>
                <select
                  class="field-input"
                  value={$scene.post_process?.tonemap ?? "aces"}
                  disabled={playing}
                  onchange={(e) =>
                    ppSetHdr({
                      hdr: true,
                      tonemap: (e.target as HTMLSelectElement).value as pp.TonemapName,
                    })}
                >
                  {#each pp.TONEMAPS as t}
                    <option value={t}>{t}</option>
                  {/each}
                </select>
              </div>
              <div class="field-row">
                <span class="field-label">Exposure</span>
                <input
                  type="range"
                  min="0.1"
                  max="4"
                  step="0.05"
                  class="field-input"
                  value={$scene.post_process?.exposure ?? 1}
                  disabled={playing}
                  oninput={(e) =>
                    ppSetHdr({
                      hdr: true,
                      exposure: parseFloat((e.target as HTMLInputElement).value),
                    })}
                />
                <span style="width:32px; text-align:right; font-size:11px;"
                  >{($scene.post_process?.exposure ?? 1).toFixed(2)}</span
                >
              </div>
            {/if}
          </div>
          {#each ppEffects as effect, i}
            {@const spec = pp.effectSpec(effect.name)}
            <div
              style="margin-bottom: 8px; padding: 6px; background: var(--ed-mantle); border-radius: 4px; border: 1px solid var(--ed-line);"
            >
              <div
                style="display:flex; align-items:center; gap:6px; margin-bottom: 4px;"
              >
                <label
                  style="font-size:12px; display:flex; align-items:center; gap:4px; cursor:pointer;"
                >
                  <input
                    type="checkbox"
                    checked={effect.enabled}
                    disabled={playing}
                    onchange={(e) =>
                      ppToggleEffect(i, (e.target as HTMLInputElement).checked)}
                  />
                  <strong>{spec?.label ?? effect.name}</strong>
                </label>
              </div>
              {#if effect.enabled && spec}
                {#each spec.params as param}
                  <div class="field-row">
                    <span class="field-label">{param.label}</span>
                    <input
                      type="range"
                      min={param.min}
                      max={param.max}
                      step={param.step}
                      class="field-input"
                      value={pp.getParam(effect, param.key)}
                      disabled={playing}
                      oninput={(e) =>
                        ppSetParam(
                          i,
                          param.key,
                          parseFloat((e.target as HTMLInputElement).value),
                        )}
                    />
                    <span style="width:40px; text-align:right; font-size:11px;"
                      >{pp.getParam(effect, param.key).toFixed(param.decimals)}</span
                    >
                  </div>
                {/each}
                {#if spec.color}
                  {@const colorKey = spec.color.key}
                  <div class="field-row">
                    <span class="field-label">{spec.color.label}</span>
                    <input
                      type="color"
                      class="field-input"
                      style="width:40px; height:22px; padding:0; border:none;"
                      value={pp.linearToHex(pp.getColor(effect, colorKey))}
                      disabled={playing}
                      oninput={(e) =>
                        ppSetColor(
                          i,
                          colorKey,
                          pp.hexToLinear(
                            (e.target as HTMLInputElement).value,
                            pp.getColor(effect, colorKey)[3] ?? 1,
                          ),
                        )}
                    />
                  </div>
                {/if}
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {:else}
    <div class="empty">Select an entity to inspect</div>
    <div class="empty-hint">
      Sort layers and post-processing moved to the scene row at the top of the
      Scene panel.
    </div>
  {/if}
</div>

{#if importAtlasOpen}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="modal-overlay"
    onclick={() => (importAtlasOpen = false)}
    onkeydown={(e) => {
      if (e.key === "Escape") importAtlasOpen = false;
    }}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="modal-content"
      onclick={(e) => e.stopPropagation()}
      onkeydown={() => {}}
    >
      <div class="modal-header">
        <span>Import Animation Clip from Spritesheet</span>
        <button class="modal-close" onclick={() => (importAtlasOpen = false)}
          >×</button
        >
      </div>
      <div class="modal-body">
        <div class="field-row">
          <label>Atlas</label>
          <select
            value={importAtlasSelected}
            onchange={async (e) => {
              importAtlasSelected = (e.target as HTMLSelectElement).value;
              await loadAtlasPreview();
            }}
          >
            {#each importAtlasFiles as f}
              <option value={f}>{f}</option>
            {/each}
          </select>
        </div>
        {#if importLoading}
          <div class="dim" style="padding: 8px 0;">Loading atlas…</div>
        {:else if importAtlasRegions.length > 0}
          <div class="field-row">
            <label>Clip Name</label>
            <input type="text" bind:value={importClipName} />
          </div>
          <div class="field-row">
            <label>Frame Duration</label>
            <input
              type="number"
              step="0.01"
              min="0.01"
              bind:value={importFrameDuration}
            />
          </div>
          <div class="field-row">
            <label>Looping</label>
            <input type="checkbox" bind:checked={importLooping} />
          </div>
          <div
            style="margin-top: 8px; margin-bottom: 4px; display: flex; justify-content: space-between; align-items: center;"
          >
            <span class="dim"
              >{importAtlasRegions.length} regions ({importRegionChecked.filter(
                Boolean,
              ).length} selected)</span
            >
            <div>
              <button
                class="compact-btn"
                onclick={() =>
                  (importRegionChecked = importAtlasRegions.map(() => true))}
                >All</button
              >
              <button
                class="compact-btn"
                onclick={() =>
                  (importRegionChecked = importAtlasRegions.map(() => false))}
                >None</button
              >
            </div>
          </div>
          <div class="import-region-list">
            {#each importAtlasRegions as region, i}
              <label class="import-region-item">
                <input type="checkbox" bind:checked={importRegionChecked[i]} />
                <span class="region-name">{region.name}</span>
                <span class="dim">{region.width}×{region.height}</span>
              </label>
            {/each}
          </div>
        {:else}
          <div class="dim" style="padding: 8px 0;">
            No regions found in atlas.
          </div>
        {/if}
      </div>
      <div class="modal-footer">
        <button class="compact-btn" onclick={() => (importAtlasOpen = false)}
          >Cancel</button
        >
        <button
          class="compact-btn primary"
          onclick={importAtlasAsClip}
          disabled={importAtlasRegions.length === 0 ||
            importRegionChecked.filter(Boolean).length === 0}>Import</button
        >
      </div>
    </div>
  </div>
{/if}

{#if gridImportOpen}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="modal-overlay"
    onclick={() => (gridImportOpen = false)}
    onkeydown={(e) => {
      if (e.key === "Escape") gridImportOpen = false;
    }}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="modal-content"
      style="width: 420px;"
      onclick={(e) => e.stopPropagation()}
      onkeydown={() => {}}
    >
      <div class="modal-header">
        <span>Generate Clips from Grid Spritesheet</span>
        <button class="modal-close" onclick={() => (gridImportOpen = false)}
          >×</button
        >
      </div>
      <div class="modal-body">
        <div class="field-row">
          <label>Texture</label>
          <select
            value={gridImportTexture}
            onchange={(e) => {
              gridImportTexture = (e.target as HTMLSelectElement).value;
            }}
          >
            <option value="">-- None --</option>
            {#each gridImportImages as img}
              <option value={img}>{img}</option>
            {/each}
          </select>
        </div>
        <div style="display: flex; gap: 8px;">
          <div class="field-row" style="flex: 1;">
            <label>Columns</label>
            <input type="number" min="1" max="64" bind:value={gridImportCols} />
          </div>
          <div class="field-row" style="flex: 1;">
            <label>Rows</label>
            <input
              type="number"
              min="1"
              max="64"
              bind:value={gridImportRows}
              onchange={gridImportUpdateRowNames}
            />
          </div>
        </div>
        <div style="display: flex; gap: 8px;">
          <div class="field-row" style="flex: 1;">
            <label>Frame Duration (s)</label>
            <input
              type="number"
              step="0.01"
              min="0.01"
              bind:value={gridImportFrameDuration}
            />
          </div>
          <div class="field-row" style="flex: 1;">
            <label style="display: flex; align-items: center; gap: 6px;">
              <input type="checkbox" bind:checked={gridImportLooping} />
              Looping
            </label>
          </div>
        </div>
        <div style="display: flex; gap: 12px; margin-top: 4px;">
          <label
            style="display: flex; align-items: center; gap: 6px; font-size: 12px;"
          >
            <input type="checkbox" bind:checked={gridImportIdleClips} />
            Also generate idle clips (first frame of each "walk_*" row)
          </label>
        </div>
        <div style="margin-top: 8px; margin-bottom: 4px;">
          <span class="dim">Clip name per row (leave empty to skip):</span>
        </div>
        <div class="grid-row-names">
          {#each gridImportRowNames as _name, i}
            <div class="grid-row-name-item">
              <span class="dim" style="min-width: 48px;">Row {i}:</span>
              <input
                type="text"
                bind:value={gridImportRowNames[i]}
                placeholder={`row_${i}`}
                style="flex: 1;"
              />
            </div>
          {/each}
        </div>
      </div>
      <div class="modal-footer">
        <button class="compact-btn" onclick={() => (gridImportOpen = false)}
          >Cancel</button
        >
        <button
          class="compact-btn primary"
          onclick={gridImportApply}
          disabled={gridImportCols < 1 ||
            gridImportRows < 1 ||
            gridImportRowNames.every((n) => !n?.trim())}
          >Generate {gridImportRowNames.filter((n) => n?.trim()).length} Clips</button
        >
      </div>
    </div>
  </div>
{/if}

<style>
  .inspector {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--ed-base);
    overflow-y: auto;
    /* The field rows narrow their label column when the dock is narrow. */
    container-type: inline-size;
  }

  @container (max-width: 300px) {
    .field-row {
      padding-left: 6px;
    }

    .field-row > label {
      flex-basis: 62px;
      font-size: 11.5px;
    }

    .field-row > label:not(:first-child) {
      min-width: 18px;
      padding: 0 4px;
    }

    .tags-container {
      padding-left: 6px;
    }
  }

  .header {
    padding: 6px 10px;
    font-family: var(--ed-mono);
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ed-muted);
    background: var(--ed-mantle);
    border-bottom: 1px solid var(--ed-line);
    flex-shrink: 0;
  }

  .gui-inspector-scroll {
    flex: 1;
    overflow-y: auto;
  }

  .inspector.playing .header {
    background: var(--ed-accent-low);
    color: var(--ed-accent-high);
    border-bottom-color: var(--ed-accent);
  }

  .section {
    padding: 0 12px 10px;
    border-bottom: 1px solid var(--ed-line);
  }

  /* The entity's name, as the design's head of the panel. */
  .entity-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px;
  }

  .entity-dot {
    width: 10px;
    height: 10px;
    border-radius: 3px;
    background: var(--ed-a2);
    flex-shrink: 0;
  }

  .entity-head input.entity-name {
    height: 28px;
    padding: 0 8px;
    border-radius: 5px;
    font-family: var(--ed-sans);
    font-size: 13.5px;
    font-weight: 600;
    color: var(--ed-ink);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  .multi-edit {
    font-size: 11px;
    color: var(--ed-warn);
    border-left: 2px solid var(--ed-warn);
  }
  .prefab-instance {
    border-left: 2px solid var(--ed-accent);
  }
  .prefab-row,
  .override-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
  }
  .override-row code {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 11px;
    color: var(--ed-accent);
  }
  .prefab-label {
    font-size: 11px;
    color: var(--ed-muted);
  }
  .prefab-link {
    background: none;
    border: none;
    color: var(--ed-accent);
    cursor: pointer;
    font-weight: 600;
    padding: 0;
    flex: 1;
    text-align: left;
  }
  .prefab-btn {
    background: none;
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    font-size: 11.5px;
    padding: 0 6px;
    cursor: pointer;
  }
  .prefab-hint {
    font-size: 10px;
    color: var(--ed-faint);
  }
  .undrawn-fields {
    border-top: 1px dashed var(--ed-line);
    margin-top: 4px;
    padding-top: 4px;
  }
  .dep-warning {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    margin-bottom: 6px;
    background: color-mix(in srgb, var(--ed-warn) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--ed-warn) 30%, transparent);
    border-radius: 4px;
    font-size: 11px;
    color: var(--ed-warn);
  }

  .dep-fix-btn {
    margin-left: auto;
    padding: 1px 8px;
    background: color-mix(in srgb, var(--ed-warn) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--ed-warn) 40%, transparent);
    border-radius: 4px;
    color: var(--ed-warn);
    font-size: 10px;
    cursor: pointer;
  }

  .dep-fix-btn:hover {
    background: color-mix(in srgb, var(--ed-warn) 25%, transparent);
  }

  /* A component's header: arrow, a bar in its kind's hue, the title, its
     key in mono, and the remove button - a full-width row of its own. */
  .section-header {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    margin: 0 -12px 4px;
    padding: 0 12px;
    background: none;
    border: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
    user-select: none;
    text-align: left;
  }

  .section-header:hover {
    background: var(--ed-hover);
  }

  .collapse-arrow {
    font-size: 9px;
    color: var(--ed-faint);
    width: 10px;
    flex-shrink: 0;
    text-align: center;
  }

  .section-bar {
    width: 3px;
    height: 14px;
    border-radius: 2px;
    flex-shrink: 0;
  }

  .section-header .section-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--ed-ink);
  }

  .section-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--ed-muted);
    display: block;
  }

  .section-key {
    font-family: var(--ed-mono);
    font-size: 10px;
    color: var(--ed-faint);
    white-space: nowrap;
  }

  .remove-btn {
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--ed-faint);
    cursor: pointer;
    font-size: 14px;
    width: 20px;
    height: 20px;
    padding: 0;
    line-height: 1;
    flex-shrink: 0;
  }

  .section-header .remove-btn {
    opacity: 0.55;
  }

  .section-header:hover .remove-btn,
  .section-header .remove-btn:focus-visible {
    opacity: 1;
  }

  .remove-btn:hover {
    color: var(--ed-err);
    background: color-mix(in srgb, var(--ed-err) 12%, transparent);
  }

  .remove-btn.small {
    font-size: 12px;
    padding: 0 2px;
  }

  .sub-section-header {
    font-family: var(--ed-mono);
    font-size: 10px;
    font-weight: 500;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ed-faint);
    margin: 10px 0 6px;
  }

  .add-btn {
    display: block;
    width: 100%;
    background: none;
    border: 1px dashed var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-accent);
    cursor: pointer;
    font-size: 11.5px;
    padding: 3px 0;
    margin-top: 2px;
    text-align: center;
  }

  .add-btn:hover {
    background: color-mix(in srgb, var(--ed-accent) 8%, transparent);
    border-color: var(--ed-accent);
  }

  /* A field: its name in a fixed column, then the control. */
  .field-row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 24px;
    margin-bottom: 6px;
    padding-left: 18px;
  }

  .field-row > label {
    flex: 0 0 82px;
    font-size: 12px;
    color: var(--ed-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* A label after the first is an axis chip fastened to its input:
     Position [X 412] [Y 288], Size [W] [H], Color [#] [A]. */
  .field-row > label:not(:first-child) {
    flex: 0 0 auto;
    min-width: 22px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0 6px;
    margin-right: -6px;
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-right: none;
    border-radius: 4px 0 0 4px;
    font-family: var(--ed-mono);
    font-size: 10.5px;
    font-weight: 500;
    color: var(--ed-faint);
  }

  .field-row > label:not(:first-child) + input {
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
  }

  .field-row > label.axis.x {
    color: var(--ed-accent);
  }

  .field-row > label.axis.y {
    color: var(--ed-a2);
  }

  input,
  select {
    height: 24px;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    color: var(--ed-ink);
    padding: 0 7px;
    font-size: 11.5px;
    font-family: var(--ed-mono);
    flex: 1;
    min-width: 0;
  }

  input::placeholder {
    color: var(--ed-faint);
  }

  /* An asset path reads in the second accent, as in the design: a field
     with an asset picker, or one whose example is a file. Numbers and
     plain text stay in ink. */
  .asset-picker-wrapper input,
  input[placeholder*=".png"],
  input[placeholder*=".lua"],
  input[placeholder*=".js"],
  input[placeholder*=".ogg"],
  input[placeholder*=".wav"],
  input[placeholder*=".gui"],
  input[placeholder*=".behavior"],
  input[placeholder*=".prefab"],
  input[placeholder*=".material"],
  input[placeholder*=".font"] {
    color: var(--ed-a2);
  }

  input:hover,
  select:hover {
    border-color: var(--ed-line-strong);
  }

  input:focus,
  select:focus {
    outline: none;
    border-color: var(--ed-accent);
    box-shadow: 0 0 0 2px var(--ed-accent-low);
  }

  select {
    appearance: none;
    padding-right: 22px;
    cursor: pointer;
    background-image:
      linear-gradient(45deg, transparent 50%, var(--ed-muted) 50%),
      linear-gradient(-45deg, transparent 50%, var(--ed-muted) 50%);
    background-position:
      calc(100% - 12px) 55%,
      calc(100% - 8px) 55%;
    background-size: 4px 4px;
    background-repeat: no-repeat;
  }

  select option {
    background: var(--ed-raised);
    color: var(--ed-ink);
  }

  input[type="number"] {
    width: 60px;
    flex: 1;
  }

  input[type="color"] {
    width: 32px;
    height: 24px;
    padding: 2px;
    flex: 0 0 32px;
    cursor: pointer;
    border-radius: 4px;
  }

  /* A checkbox is a switch, as in the design. */
  input[type="checkbox"] {
    appearance: none;
    flex: 0 0 26px;
    width: 26px;
    height: 15px;
    padding: 0;
    border: none;
    border-radius: 8px;
    background: var(--ed-line-strong);
    position: relative;
    cursor: pointer;
    transition: background 0.12s;
  }

  input[type="checkbox"]::after {
    content: "";
    position: absolute;
    left: 2px;
    top: 2px;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: var(--ed-muted);
    transition: transform 0.12s, background 0.12s;
  }

  input[type="checkbox"]:checked {
    background: var(--ed-ok);
  }

  input[type="checkbox"]:checked::after {
    transform: translateX(11px);
    background: var(--ed-on-accent);
  }

  input[type="checkbox"]:focus-visible {
    outline: 2px solid var(--ed-a2);
    outline-offset: 2px;
    box-shadow: none;
  }

  input[type="range"] {
    height: 18px;
    padding: 0;
    background: none;
    border: none;
    accent-color: var(--ed-accent);
    box-shadow: none;
  }

  .dim {
    font-size: 11px;
    color: var(--ed-faint);
  }

  .add-size-btn {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-accent);
    cursor: pointer;
    font-size: 11.5px;
    padding: 3px 8px;
  }

  .add-size-btn:hover {
    background: var(--ed-line);
  }

  .new-script-btn {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-ok);
    cursor: pointer;
    font-size: 11.5px;
    padding: 3px 8px;
    flex-shrink: 0;
  }

  .new-script-btn:hover {
    background: var(--ed-line);
  }

  .add-section {
    padding: 8px 10px;
  }

  .add-component-wrapper {
    position: relative;
  }

  .add-component-btn {
    width: 100%;
    height: 30px;
    background: none;
    border: 1px dashed var(--ed-line-strong);
    border-radius: 5px;
    color: var(--ed-muted);
    cursor: pointer;
    font-size: 12px;
    font-family: var(--ed-sans);
  }

  .add-component-btn:hover {
    background: var(--ed-hover);
    border-color: var(--ed-accent);
    color: var(--ed-ink);
  }

  .add-menu {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 7px;
    margin-top: 2px;
    z-index: 10;
    overflow: hidden;
    max-height: 320px;
    overflow-y: auto;
  }

  .add-menu-search {
    padding: 6px 6px 4px;
    position: sticky;
    top: 0;
    background: var(--ed-raised);
    border-bottom: 1px solid var(--ed-line-strong);
    z-index: 1;
  }

  .add-menu-search-input {
    width: 100%;
    box-sizing: border-box;
    background: var(--ed-base);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 11px;
    padding: 4px 7px;
    font-family: inherit;
    outline: none;
  }

  .add-menu-search-input:focus {
    border-color: var(--ed-accent);
  }

  .add-menu-no-results {
    padding: 8px 10px;
    font-size: 11px;
    color: var(--ed-faint);
    font-style: italic;
  }

  .add-menu-category {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    text-align: left;
    background: var(--ed-base);
    border: none;
    border-bottom: 1px solid var(--ed-line);
    color: var(--ed-muted);
    padding: 5px 8px;
    font-size: 11px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
    user-select: none;
  }

  .add-menu-category:hover {
    background: var(--ed-mantle);
  }

  .cat-arrow {
    font-size: 9px;
    width: 10px;
    color: var(--ed-faint);
  }

  .cat-icon {
    font-size: 11px;
  }

  .cat-count {
    margin-left: auto;
    font-size: 10px;
    color: var(--ed-faint);
    font-weight: 400;
  }

  .add-menu-item {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: var(--ed-text);
    padding: 5px 10px 5px 26px;
    font-size: 12px;
    cursor: pointer;
    font-family: inherit;
    border-radius: 4px;
  }

  .add-menu-item:hover {
    background: var(--ed-accent-low);
    color: var(--ed-ink);
  }

  .add-menu-new-custom {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: var(--ed-accent);
    padding: 5px 10px;
    font-size: 12px;
    cursor: pointer;
    font-family: inherit;
  }

  .add-menu-new-custom:hover {
    background: var(--ed-line);
  }

  .empty {
    padding: 16px 10px;
    color: var(--ed-faint);
    font-size: 12px;
    font-style: italic;
  }

  .empty-hint {
    padding: 0 10px 12px;
    color: var(--ed-faint);
    font-size: 11px;
    line-height: 1.4;
  }

  .scene-subtitle {
    padding: 6px 10px 0;
    color: var(--ed-muted);
    font-size: 12px;
  }

  /* Tags */
  .tags-container {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    padding-left: 18px;
  }

  .tag-chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: var(--ed-accent-low);
    border: none;
    border-radius: 10px;
    padding: 1px 4px 1px 8px;
    font-family: var(--ed-mono);
    font-size: 10.5px;
    color: var(--ed-accent-high);
  }

  .tag-remove {
    background: none;
    border: none;
    color: var(--ed-faint);
    cursor: pointer;
    font-size: 12px;
    padding: 0 2px;
    line-height: 1;
  }

  .tag-remove:hover {
    color: var(--ed-err);
  }

  .tag-add-row {
    display: flex;
    gap: 4px;
    width: 100%;
    margin-top: 4px;
  }

  .tag-input {
    flex: 1;
    min-width: 0;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 3px;
    color: var(--ed-text);
    padding: 2px 6px;
    font-size: 11px;
    font-family: inherit;
  }

  .tag-input:focus {
    outline: none;
    border-color: var(--ed-accent);
  }

  .tag-add-btn {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 3px;
    color: var(--ed-accent);
    cursor: pointer;
    font-size: 12px;
    padding: 2px 8px;
    font-family: inherit;
    flex-shrink: 0;
  }

  .tag-add-btn:hover:not(:disabled) {
    background: var(--ed-line);
  }

  .tag-add-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  /* Import from Spritesheet modal */
  .compact-btn {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-text);
    cursor: pointer;
    font-size: 12px;
    padding: 4px 10px;
    font-family: inherit;
  }
  .compact-btn:hover:not(:disabled) {
    background: var(--ed-line);
  }

  .modal-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal-content {
    background: var(--ed-base);
    border: 1px solid var(--ed-line-strong);
    border-radius: 8px;
    width: 380px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 14px;
    border-bottom: 1px solid var(--ed-line);
    font-weight: 600;
    font-size: 13px;
    color: var(--ed-text);
  }

  .modal-close {
    background: none;
    border: none;
    color: var(--ed-faint);
    cursor: pointer;
    font-size: 18px;
    padding: 0 4px;
  }
  .modal-close:hover {
    color: var(--ed-err);
  }

  .modal-body {
    padding: 12px 14px;
    overflow-y: auto;
    flex: 1;
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 10px 14px;
    border-top: 1px solid var(--ed-line);
  }

  .compact-btn.primary {
    background: var(--ed-accent);
    color: var(--ed-on-accent);
    border-color: var(--ed-accent);
  }
  .compact-btn.primary:hover:not(:disabled) {
    background: var(--ed-a2);
  }
  .compact-btn.primary:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .import-region-list {
    max-height: 200px;
    overflow-y: auto;
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    padding: 4px;
  }

  .grid-row-names {
    max-height: 180px;
    overflow-y: auto;
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .grid-row-name-item {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--ed-text);
  }

  .grid-row-name-item input {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-text);
    padding: 3px 6px;
    font-size: 12px;
    font-family: inherit;
  }

  .import-region-item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 4px;
    font-size: 12px;
    color: var(--ed-text);
    cursor: pointer;
  }
  .import-region-item:hover {
    background: var(--ed-raised);
    border-radius: 3px;
  }

  .region-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Asset picker */
  .asset-picker-wrapper {
    position: relative;
    flex: 1;
    display: flex;
    gap: 0;
    min-width: 0;
  }

  .asset-picker-wrapper input {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
    border-right: none;
  }

  .picker-btn {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-top-right-radius: 3px;
    border-bottom-right-radius: 3px;
    color: var(--ed-accent);
    cursor: pointer;
    font-size: 12px;
    padding: 0 6px;
    flex-shrink: 0;
    line-height: 1;
  }

  .picker-btn:hover {
    background: var(--ed-line);
  }

  .picker-backdrop {
    position: fixed;
    inset: 0;
    z-index: 99;
    background: transparent;
    border: none;
    padding: 0;
    cursor: default;
  }

  .picker-dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 7px;
    margin-top: 2px;
    z-index: 100;
    max-height: 180px;
    overflow-y: auto;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
  }

  .atlas-anim-picker {
    position: relative;
    top: auto;
    left: auto;
    right: auto;
    margin-top: 4px;
    max-height: 320px;
  }

  .atlas-import-tabs {
    display: flex;
    border-bottom: 1px solid var(--ed-line);
  }

  .clip-detail {
    margin: 3px 0;
    padding: 4px 6px;
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    border-radius: 4px;
  }

  .clip-detail-header {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .clip-name {
    font-size: 11px;
    font-weight: 600;
    color: var(--ed-text);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .clip-detail-fields {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 3px;
    font-size: 11px;
  }

  .clip-detail-fields label {
    color: var(--ed-faint);
    min-width: unset;
  }

  .clip-detail-fields input[type="number"] {
    width: 48px;
  }

  .atlas-import-tab {
    flex: 1;
    padding: 5px 8px;
    font-size: 11px;
    background: none;
    border: none;
    color: var(--ed-faint);
    cursor: pointer;
  }

  .atlas-import-tab.active {
    color: var(--ed-text);
    border-bottom: 2px solid var(--ed-accent);
  }

  .atlas-import-tab:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .atlas-import-config {
    padding: 6px 8px;
    border-bottom: 1px solid var(--ed-line);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .atlas-import-config .field-row {
    margin: 0;
  }

  .atlas-import-actions {
    padding: 6px 8px;
    border-top: 1px solid var(--ed-line);
    position: sticky;
    bottom: 0;
    background: var(--ed-base);
  }

  .frame-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--ed-accent);
    color: var(--ed-on-accent);
    font-size: 10px;
    font-weight: 700;
    margin-right: 4px;
    flex-shrink: 0;
  }

  .picker-item {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: var(--ed-text);
    padding: 5px 8px;
    font-size: 11px;
    cursor: pointer;
    font-family: monospace;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    border-radius: 4px;
  }

  .picker-item:hover {
    background: var(--ed-accent-low);
    color: var(--ed-ink);
  }

  .picker-item.active {
    background: color-mix(in srgb, var(--ed-accent) 15%, transparent);
    color: var(--ed-accent);
    border-radius: 4px;
  }

  .picker-empty {
    display: block;
    padding: 8px;
    font-size: 11px;
    color: var(--ed-faint);
    font-style: italic;
  }

  .picker-group-label {
    padding: 4px 8px 2px;
    font-size: 10px;
    color: var(--ed-faint);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    border-top: 1px solid var(--ed-line);
  }

  .picker-group-label:first-child {
    border-top: none;
  }

  .picker-item-dim {
    color: var(--ed-faint);
    font-size: 10px;
    border-radius: 4px;
  }

  .layer-toggle {
    width: 100%;
    padding: 2px 0;
    font-size: 10px;
    border: 1px solid var(--ed-line);
    background: var(--ed-base);
    color: var(--ed-faint);
    cursor: pointer;
    border-radius: 2px;
    text-align: center;
  }
  .layer-toggle.active {
    background: var(--ed-ok);
    color: var(--ed-on-accent);
    border-color: var(--ed-ok);
  }
  .skin-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 2px 8px 6px;
    font-size: 11px;
  }
  .skin-sum {
    color: var(--ed-muted);
    flex: 1;
  }
  .override-mark {
    color: var(--ed-accent);
    font-size: 9px;
    margin-left: 5px;
    cursor: help;
  }
  .mixed-mark {
    color: var(--ed-warn);
    font-size: 10px;
    margin-left: 5px;
    cursor: help;
  }
</style>
