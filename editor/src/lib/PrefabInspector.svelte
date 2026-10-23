<script lang="ts">
  import { get } from "svelte/store";
  import {
    prefabData,
    prefabDirty,
    executePrefab as executePrefabRaw,
  } from "./stores/prefabEditorStore";
  import {
    editThroughInstance,
    instanceChanges,
    revertPath,
    unpackInstance,
    viewOf,
    type Components,
  } from "./prefabInstance";
  import { prefabLibrary } from "./stores/prefabLibraryStore";
  import type { EntityData } from "./api/scene";
  import { project } from "./stores/projectStore";
  import { listAssets } from "./api/project";
  import type { ComponentDef } from "./api/project";
  import { logError, logInfo } from "./stores/consoleStore";
  import { readAnimFile } from "./api/animation";
  import {
    KNOWN_COMPONENTS,
    BUILTIN_KEYS,
    COMPONENT_DEFAULTS,
    COMPONENT_DEPS,
    categorizeComponents,
    formatComponentName,
    PREFAB_UNDRAWN_FIELDS,
  } from "./componentDefs";
  import PackageInspector from "./PackageInspector.svelte";
  import { packageSchema } from "./stores/packageSchemaStore";

  // ---------------------------------------------------------------------------
  // Component collapse state
  // ---------------------------------------------------------------------------
  let collapsed: Record<string, boolean> = $state({});

  function toggleSection(key: string) {
    collapsed = { ...collapsed, [key]: !collapsed[key] };
  }

  // ---------------------------------------------------------------------------
  // Helpers
  // ---------------------------------------------------------------------------
  let showAddMenu = $state(false);
  let expandedCategories: Record<string, boolean> = $state({});

  /** A variant (a prefab whose components carry a `prefab_instance`) is
   *  shown as it is - its base merged in - and edited through it, so what
   *  it changes stays its overrides and its base's later changes reach it.
   *  The base comes from the prefabs on disk (`prefabLibrary`). */
  let data = $derived($prefabData ? viewOf($prefabData, $prefabLibrary.resolved) : null);

  /** The prefab a variant extends, or null. */
  let baseId: string | null = $derived(
    (($prefabData?.components as Components | undefined)?.prefab_instance as { prefab_id?: string } | undefined)
      ?.prefab_id ?? null,
  );
  let baseTemplate: Components | null = $derived(
    baseId ? ($prefabLibrary.resolved.get(baseId)?.components ?? null) : null,
  );
  let variantRows = $derived(
    baseTemplate && $prefabData ? instanceChanges($prefabData.components as Components) : [],
  );
  /** Prefabs this one could extend: every other one. */
  let baseChoices = $derived(
    [...$prefabLibrary.raw.keys()].filter((k) => k !== $prefabData?.name).sort(),
  );

  /** Every edit of the prefab, through its base when it is a variant. */
  function executePrefab(label: string, mutate: (d: EntityData) => EntityData | void) {
    const prefabs = $prefabLibrary.resolved;
    executePrefabRaw(label, (d) => {
      editThroughInstance(d, prefabs, (view) => {
        const out = mutate(view);
        if (out && out !== view) Object.assign(view, out);
      });
      return d;
    });
  }

  function setBase(id: string) {
    if (id === (baseId ?? "")) return;
    if (id === "") {
      // No longer a variant: the base's components copied in.
      const template = baseTemplate;
      executePrefabRaw("Stop extending a prefab", (d) => {
        d.components = (template
          ? unpackInstance(template, d.components as Components)
          : (() => {
              const c = { ...(d.components as Components) };
              delete c.prefab_instance;
              return c;
            })()) as EntityData["components"];
        return d;
      });
      return;
    }
    executePrefabRaw(`Extend prefab "${id}"`, (d) => {
      const c = d.components as Components;
      const old = c.prefab_instance as { overrides?: Record<string, unknown> } | undefined;
      c.prefab_instance = { prefab_id: id, ...(old?.overrides ? { overrides: old.overrides } : {}) };
      return d;
    });
  }

  function revertVariant(paths: string[]) {
    const template = baseTemplate;
    if (!template) return;
    executePrefabRaw(paths.length === 1 ? `Revert ${paths[0]}` : "Revert to base", (d) => {
      let c = d.components as Components;
      for (const path of paths) c = revertPath(c, template, path);
      d.components = c as EntityData["components"];
      return d;
    });
  }

  let missingComponents = $derived.by(() => {
    if (!data) return [] as string[];
    const result = (KNOWN_COMPONENTS as readonly string[])
      .filter((c) => !(c in data.components))
      .slice() as string[];
    // Include custom component definitions from project
    const defs = get(project)?.component_definitions ?? [];
    for (const def of defs) {
      if (!(def.name in data.components) && !result.includes(def.name)) {
        result.push(def.name);
      }
    }
    return result;
  });

  let categorizedMissing = $derived.by(() => {
    return categorizeComponents(missingComponents);
  });

  // All custom (unknown) component keys
  let customKeys = $derived(() => {
    if (!data) return [];
    return Object.keys(data.components).filter((k) => !BUILTIN_KEYS.has(k));
  });

  function handleNameChange(e: Event) {
    const val = (e.target as HTMLInputElement).value;
    executePrefab("Rename prefab", (d) => {
      d.name = val;
      return d;
    });
  }

  function handleRemoveComponent(key: string) {
    executePrefab(`Remove ${key}`, (d) => {
      delete d.components[key];
      return d;
    });
  }

  function handleAddComponent(key: string) {
    showAddMenu = false;
    const builtin = COMPONENT_DEFAULTS[key];
    if (builtin !== undefined) {
      executePrefab(`Add ${key}`, (d) => {
        // Auto-add missing dependencies
        const deps = COMPONENT_DEPS[key] ?? [];
        for (const dep of deps) {
          if (!(dep in d.components) && COMPONENT_DEFAULTS[dep] !== undefined) {
            (d.components as Record<string, unknown>)[dep] = structuredClone(
              COMPONENT_DEFAULTS[dep],
            );
          }
        }
        const defaultValue = structuredClone(builtin);
        if (key === "camera") {
          const proj = get(project);
          if (proj?.settings) {
            (defaultValue as Record<string, unknown>).viewport_width =
              proj.settings.width;
            (defaultValue as Record<string, unknown>).viewport_height =
              proj.settings.height;
          }
        }
        (d.components as Record<string, unknown>)[key] = defaultValue;
        return d;
      });
      return;
    }
    // Custom component: build default object from definition
    const defs: ComponentDef[] = get(project)?.component_definitions ?? [];
    const def = defs.find((d) => d.name === key);
    if (def) {
      executePrefab(`Add ${key}`, (d) => {
        const obj: Record<string, unknown> = {};
        for (const f of def.fields) {
          obj[f.name] = structuredClone(f.default_value);
        }
        (d.components as Record<string, unknown>)[key] = obj;
        return d;
      });
    }
  }

  // ---------------------------------------------------------------------------
  // Sprite file picker
  // ---------------------------------------------------------------------------
  let spriteFiles: string[] = $state([]);

  async function loadSpriteFiles() {
    const proj = get(project);
    if (!proj) return;
    try {
      const all = await listAssets(proj.path);
      spriteFiles = all.filter((f: string) => /\.(png|jpg|jpeg)$/i.test(f));
    } catch (e) {
      logError(`Failed to list assets: ${e}`);
    }
  }

  $effect(() => {
    loadSpriteFiles();
  });

  // ---------------------------------------------------------------------------
  // Script file picker
  // ---------------------------------------------------------------------------
  let scriptFiles: string[] = $state([]);
  let scriptPickerOpen = $state(false);

  async function loadScriptFiles() {
    const proj = get(project);
    if (!proj) return;
    try {
      const all = await listAssets(proj.path);
      scriptFiles = all.filter((f: string) => /\.(lua|js)$/i.test(f));
    } catch (e) {
      logError(`Failed to list script files: ${e}`);
    }
  }

  $effect(() => {
    loadScriptFiles();
  });

  // ---------------------------------------------------------------------------
  // Anim file picker
  // ---------------------------------------------------------------------------
  let animFiles: string[] = $state([]);
  let animPickerOpen = $state(false);

  async function loadAnimFiles() {
    const proj = get(project);
    if (!proj) return;
    try {
      const all = await listAssets(proj.path);
      animFiles = all.filter((f: string) => /\.anim$/i.test(f));
    } catch (e) {
      logError(`Failed to list anim files: ${e}`);
    }
  }

  $effect(() => {
    loadAnimFiles();
  });

  async function loadAnimFileToPrefab(animPath: string) {
    const proj = get(project);
    if (!proj) return;
    try {
      const data = await readAnimFile(proj.path, animPath);
      const clips: Record<
        string,
        {
          name: string;
          frames: {
            uv_rect: [number, number, number, number];
            texture_id?: string;
            triggers?: unknown[];
          }[];
          frame_duration: number;
          looping: boolean;
        }
      > = {};
      for (const [name, clip] of Object.entries(data.clips)) {
        clips[name] = {
          name: clip.name,
          frames: clip.frames.map((f) => ({
            uv_rect: [...f.uv_rect] as [number, number, number, number],
            texture_id: f.texture_id ?? undefined,
            triggers: f.triggers ?? [],
          })),
          frame_duration: clip.frame_duration,
          looping: clip.looping,
        };
      }
      const firstClip = Object.keys(clips)[0] ?? undefined;
      executePrefab(`Load animation "${animPath}"`, (d) => {
        const sa = d.components.sprite_animator as
          | Record<string, unknown>
          | undefined;
        if (!sa) return d;
        sa.clips = clips;
        sa.current_clip = firstClip;
        sa.current_frame = 0;
        sa.elapsed = 0;
        if (data.texture_id && d.components.sprite) {
          (d.components.sprite as Record<string, unknown>).texture_id =
            data.texture_id;
        }
        return d;
      });
      logInfo(`Loaded ${Object.keys(clips).length} clip(s) from ${animPath}`);
      animPickerOpen = false;
    } catch (e) {
      logError(`Failed to load .anim file: ${e}`);
    }
  }

  // ---------------------------------------------------------------------------
  // Generic field updaters
  // ---------------------------------------------------------------------------
  function updateField(comp: string, field: string, value: unknown) {
    executePrefab(`Change ${comp}.${field}`, (d) => {
      const c = (d.components as Record<string, Record<string, unknown>>)[comp];
      if (!c) return d;
      // `undefined` unsets an optional field, as the engine leaves it out.
      if (value === undefined) delete c[field];
      else c[field] = value;
      return d;
    });
  }

  /** Every component's schema, derived from the component itself or written
   *  by its package - what draws a component with no section here. */
  let schemaMap = $derived(
    new Map(($packageSchema?.components ?? []).map((info) => [info.component_name, info])),
  );

  function updateArrayField(
    comp: string,
    field: string,
    index: number,
    value: number,
  ) {
    executePrefab(`Change ${comp}.${field}[${index}]`, (d) => {
      const c = (d.components as Record<string, Record<string, unknown>>)[comp];
      if (!c) return d;
      const arr = c[field] as number[];
      arr[index] = value;
      return d;
    });
  }

  // ---------------------------------------------------------------------------
  // Color helpers
  // ---------------------------------------------------------------------------
  function colorToHex(rgba: [number, number, number, number]): string {
    const r = Math.round(rgba[0] * 255)
      .toString(16)
      .padStart(2, "0");
    const g = Math.round(rgba[1] * 255)
      .toString(16)
      .padStart(2, "0");
    const b = Math.round(rgba[2] * 255)
      .toString(16)
      .padStart(2, "0");
    return `#${r}${g}${b}`;
  }

  function hexToColor(
    hex: string,
    alpha: number,
  ): [number, number, number, number] {
    const r = parseInt(hex.slice(1, 3), 16) / 255;
    const g = parseInt(hex.slice(3, 5), 16) / 255;
    const b = parseInt(hex.slice(5, 7), 16) / 255;
    return [r, g, b, alpha];
  }

  // Tag handling
  let newTag = $state("");

  function handleAddTag() {
    if (!newTag.trim()) return;
    executePrefab("Add tag", (d) => {
      const tags = (d.components.tags as string[]) ?? [];
      if (!tags.includes(newTag.trim())) {
        tags.push(newTag.trim());
      }
      d.components.tags = tags;
      return d;
    });
    newTag = "";
  }

  function handleRemoveTag(tag: string) {
    executePrefab("Remove tag", (d) => {
      d.components.tags = ((d.components.tags as string[]) ?? []).filter(
        (t: string) => t !== tag,
      );
      return d;
    });
  }
</script>

<div class="prefab-inspector">
  {#if data}
    <!-- Name -->
    <div class="section">
      <label class="section-title">Name</label>
      <input type="text" value={data.name} onchange={handleNameChange} />
    </div>

    <!-- Variant: the prefab this one extends, and what it changes -->
    <div class="section variant">
      <div class="variant-row">
        <label class="section-title" title="A variant is its base prefab plus its own changes: a later change to the base reaches it">Extends</label>
        <select value={baseId ?? ""} onchange={(e) => setBase((e.target as HTMLSelectElement).value)}>
          <option value="">(nothing - a prefab of its own)</option>
          {#each baseChoices as id}
            <option value={id}>{id}</option>
          {/each}
          {#if baseId && !baseChoices.includes(baseId)}
            <option value={baseId}>{baseId} (missing)</option>
          {/if}
        </select>
      </div>
      {#if baseId && !baseTemplate}
        <div class="variant-hint">⚠ No prefab "{baseId}" in the project: only this prefab's own components show.</div>
      {:else if baseId && variantRows.length === 0}
        <div class="variant-hint">Everything comes from {baseId}. An edit here becomes this variant's change.</div>
      {:else}
        {#each variantRows as row (row.path)}
          <div class="variant-row" title={JSON.stringify(row.value)}>
            <code>{row.path}</code>
            {#if row.own}<span class="variant-hint">own</span>{/if}
            <button class="variant-btn" title="Revert to the base's" onclick={() => revertVariant([row.path])}>⟲</button>
          </div>
        {/each}
      {/if}
    </div>

    <!-- Transform -->
    {#if data.components.transform}
      {@const t = data.components.transform}
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
          <span class="section-title">Transform</span>
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
            <label>X</label>
            <input
              type="number"
              step="0.1"
              value={t.position[0]}
              onchange={(e) =>
                updateArrayField(
                  "transform",
                  "position",
                  0,
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
            <label>Y</label>
            <input
              type="number"
              step="0.1"
              value={t.position[1]}
              onchange={(e) =>
                updateArrayField(
                  "transform",
                  "position",
                  1,
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
          </div>
          <div class="field-row">
            <label>Rot</label>
            <input
              type="number"
              step="1"
              value={t.rotation}
              onchange={(e) =>
                updateField(
                  "transform",
                  "rotation",
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
          </div>
          <div class="field-row">
            <label>SX</label>
            <input
              type="number"
              step="0.1"
              value={t.scale[0]}
              onchange={(e) =>
                updateArrayField(
                  "transform",
                  "scale",
                  0,
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
            <label>SY</label>
            <input
              type="number"
              step="0.1"
              value={t.scale[1]}
              onchange={(e) =>
                updateArrayField(
                  "transform",
                  "scale",
                  1,
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
          </div>
          {@render undrawnFields("transform")}
        {/if}
      </div>
    {/if}

    <!-- Sprite -->
    {#if data.components.sprite}
      {@const s = data.components.sprite}
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
          <span class="section-title">Sprite</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("sprite");
            }}
            title="Remove Sprite">×</button
          >
        </div>
        {#if !collapsed["sprite"]}
          <div class="field-row">
            <label>Texture</label>
            <select
              value={s.texture_id}
              onchange={(e) =>
                updateField(
                  "sprite",
                  "texture_id",
                  (e.target as HTMLSelectElement).value,
                )}
            >
              <option value="">— none —</option>
              {#each spriteFiles as file}
                <option value={file}>{file}</option>
              {/each}
            </select>
          </div>
          <div class="field-row">
            <label>Color</label>
            <input
              type="color"
              value={colorToHex(s.color)}
              onchange={(e) =>
                updateField(
                  "sprite",
                  "color",
                  hexToColor((e.target as HTMLInputElement).value, s.color[3]),
                )}
            />
            <label>A</label>
            <input
              type="number"
              step="0.05"
              min="0"
              max="1"
              value={s.color[3]}
              onchange={(e) =>
                updateField("sprite", "color", [
                  s.color[0],
                  s.color[1],
                  s.color[2],
                  parseFloat((e.target as HTMLInputElement).value),
                ])}
            />
          </div>
          <div class="field-row">
            <label>Z</label>
            <input
              type="number"
              step="1"
              value={s.z_order}
              onchange={(e) =>
                updateField(
                  "sprite",
                  "z_order",
                  parseInt((e.target as HTMLInputElement).value),
                )}
            />
          </div>
          {#if s.size}
            <div class="field-row">
              <label>W</label>
              <input
                type="number"
                step="1"
                value={s.size[0]}
                onchange={(e) =>
                  updateField("sprite", "size", [
                    parseFloat((e.target as HTMLInputElement).value),
                    s.size![1],
                  ])}
              />
              <label>H</label>
              <input
                type="number"
                step="1"
                value={s.size[1]}
                onchange={(e) =>
                  updateField("sprite", "size", [
                    s.size![0],
                    parseFloat((e.target as HTMLInputElement).value),
                  ])}
              />
              <button
                class="remove-btn small"
                onclick={() =>
                  executePrefab("Remove size override", (d) => {
                    delete d.components.sprite!.size;
                    return d;
                  })}
                title="Remove size override">×</button
              >
            </div>
          {:else}
            <div class="field-row">
              <button
                class="add-size-btn"
                onclick={() => updateField("sprite", "size", [32, 32])}
              >
                + Size override
              </button>
            </div>
          {/if}
          {@render undrawnFields("sprite")}
        {/if}
      </div>
    {/if}

    <!-- Script -->
    {#if data.components.script}
      {@const sc = data.components.script}
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
          <span class="section-title">Script</span>
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
                value={sc.source_path}
                onchange={(e) =>
                  updateField(
                    "script",
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
                      class:active={sc.source_path === file}
                      role="option"
                      aria-selected={sc.source_path === file}
                      onclick={() => {
                        const ext = file.split(".").pop()?.toLowerCase();
                        const lang = ext === "js" ? "JavaScript" : "Lua";
                        updateField("script", "source_path", file);
                        updateField("script", "language", lang);
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
          </div>
          <div class="field-row">
            <label>Lang</label>
            <select
              value={sc.language}
              onchange={(e) =>
                updateField(
                  "script",
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

    <!-- Tags -->
    {#if data.components.tags}
      {@const tags = data.components.tags as string[]}
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
          <span class="section-title">Tags</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("tags");
            }}
            title="Remove Tags">×</button
          >
        </div>
        {#if !collapsed["tags"]}
          <div class="tag-list">
            {#each tags as tag}
              <span class="tag-chip">
                {tag}
                <button class="tag-remove" onclick={() => handleRemoveTag(tag)}
                  >×</button
                >
              </span>
            {/each}
          </div>
          <div class="field-row">
            <input
              type="text"
              placeholder="New tag…"
              bind:value={newTag}
              onkeydown={(e) => {
                if (e.key === "Enter") handleAddTag();
              }}
            />
            <button class="add-size-btn" onclick={handleAddTag}>+</button>
          </div>
          {@render undrawnFields("tags")}
        {/if}
      </div>
    {/if}

    <!-- Rigid Body -->
    {#if data.components.rigid_body}
      {@const rb = data.components.rigid_body}
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
          <span class="section-title">Rigid Body</span>
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
              onchange={(e) =>
                updateField(
                  "rigid_body",
                  "body_type",
                  (e.target as HTMLSelectElement).value,
                )}
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
              value={rb.mass}
              onchange={(e) =>
                updateField(
                  "rigid_body",
                  "mass",
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
          </div>
          <div class="field-row">
            <label>Gravity</label>
            <input
              type="number"
              step="0.1"
              value={rb.gravity_scale}
              onchange={(e) =>
                updateField(
                  "rigid_body",
                  "gravity_scale",
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
          </div>
          <div class="field-row">
            <label>Lin damp</label>
            <input
              type="number"
              step="0.1"
              value={rb.linear_damping}
              onchange={(e) =>
                updateField(
                  "rigid_body",
                  "linear_damping",
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
          </div>
          <div class="field-row">
            <label>Ang damp</label>
            <input
              type="number"
              step="0.1"
              value={rb.angular_damping}
              onchange={(e) =>
                updateField(
                  "rigid_body",
                  "angular_damping",
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
          </div>
          <div class="field-row">
            <label>Fixed rot</label>
            <input
              type="checkbox"
              checked={rb.fixed_rotation}
              onchange={(e) =>
                updateField(
                  "rigid_body",
                  "fixed_rotation",
                  (e.target as HTMLInputElement).checked,
                )}
            />
          </div>
          <div class="field-row">
            <label
              title="Sweep the shape along its path (continuous collision). For anything that crosses more than its own thickness in a frame."
              >CCD</label
            >
            <input
              type="checkbox"
              checked={rb.ccd ?? false}
              onchange={(e) =>
                updateField(
                  "rigid_body",
                  "ccd",
                  (e.target as HTMLInputElement).checked,
                )}
            />
          </div>
          {@render undrawnFields("rigid_body")}
        {/if}
      </div>
    {/if}

    <!-- Collider -->
    {#if data.components.collider}
      {@const col = data.components.collider}
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
          <span class="section-title">Collider</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("collider");
            }}
            title="Remove Collider">×</button
          >
        </div>
        {#if !collapsed["collider"]}
          <div class="field-row">
            <label>Sensor</label>
            <input
              type="checkbox"
              checked={col.is_sensor}
              onchange={(e) =>
                updateField(
                  "collider",
                  "is_sensor",
                  (e.target as HTMLInputElement).checked,
                )}
            />
          </div>
          <div class="field-row">
            <label>Friction</label>
            <input
              type="number"
              step="0.1"
              value={col.friction}
              onchange={(e) =>
                updateField(
                  "collider",
                  "friction",
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
          </div>
          <div class="field-row">
            <label>Bounce</label>
            <input
              type="number"
              step="0.1"
              value={col.restitution}
              onchange={(e) =>
                updateField(
                  "collider",
                  "restitution",
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
          </div>
          <div class="field-row">
            <label>Density</label>
            <input
              type="number"
              step="0.1"
              value={col.density}
              onchange={(e) =>
                updateField(
                  "collider",
                  "density",
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
          </div>
          <div class="field-row">
            <label>Offset X</label>
            <input
              type="number"
              step="1"
              value={col.offset[0]}
              onchange={(e) =>
                updateArrayField(
                  "collider",
                  "offset",
                  0,
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
            <label>Y</label>
            <input
              type="number"
              step="1"
              value={col.offset[1]}
              onchange={(e) =>
                updateArrayField(
                  "collider",
                  "offset",
                  1,
                  parseFloat((e.target as HTMLInputElement).value),
                )}
            />
          </div>
          {@render undrawnFields("collider")}
        {/if}
      </div>
    {/if}

    <!-- Sprite Animator -->
    {#if data.components.sprite_animator}
      {@const anim = data.components.sprite_animator as Record<string, unknown>}
      {@const clips = (anim.clips ?? {}) as Record<
        string,
        Record<string, unknown>
      >}
      {@const clipNames = Object.keys(clips)}
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
          <span class="section-title">Sprite Animator</span>
          <button
            class="remove-btn"
            onclick={(e) => {
              e.stopPropagation();
              handleRemoveComponent("sprite_animator");
            }}
            title="Remove Sprite Animator">×</button
          >
        </div>
        {#if !collapsed["sprite_animator"]}
          <div class="field-row">
            <label>Speed</label>
            <input
              type="number"
              step="0.1"
              min="0"
              value={anim.speed}
              onchange={(e) =>
                updateField(
                  "sprite_animator",
                  "speed",
                  parseFloat((e.target as HTMLInputElement).value) || 1,
                )}
            />
          </div>
          <div class="field-row">
            <label>Playing</label>
            <input
              type="checkbox"
              checked={anim.playing as boolean}
              onchange={(e) =>
                updateField(
                  "sprite_animator",
                  "playing",
                  (e.target as HTMLInputElement).checked,
                )}
            />
          </div>
          <div class="field-row">
            <label>Random Start</label>
            <input
              type="checkbox"
              checked={(anim.random_start ?? false) as boolean}
              onchange={(e) =>
                updateField(
                  "sprite_animator",
                  "random_start",
                  (e.target as HTMLInputElement).checked,
                )}
            />
          </div>
          <div class="field-row">
            <label>Clip</label>
            {#if clipNames.length > 0}
              <select
                value={(anim.current_clip ?? "") as string}
                onchange={(e) => {
                  const val =
                    (e.target as HTMLSelectElement).value || undefined;
                  updateField("sprite_animator", "current_clip", val);
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
                      onclick={() => loadAnimFileToPrefab(file)}>{file}</button
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
            {#each clipNames as clipName}
              {@const clip = clips[clipName]}
              {#if clip}
                <div class="clip-detail">
                  <div class="clip-detail-header">
                    <span class="clip-name">{clipName}</span>
                    <span class="dim" style="font-size: 10px;"
                      >{(clip.frames as unknown[])?.length ?? 0} frames</span
                    >
                    <button
                      class="remove-btn"
                      title="Remove clip"
                      onclick={() => {
                        executePrefab(`Remove clip "${clipName}"`, (d) => {
                          const sa = d.components.sprite_animator as
                            | Record<string, unknown>
                            | undefined;
                          if (sa?.clips) {
                            delete (sa.clips as Record<string, unknown>)[
                              clipName
                            ];
                            if (sa.current_clip === clipName) {
                              const remaining = Object.keys(sa.clips as object);
                              sa.current_clip = remaining[0] ?? undefined;
                            }
                          }
                          return d;
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
                      value={(clip.frame_duration as number) > 0
                        ? Math.round(1.0 / (clip.frame_duration as number))
                        : 10}
                      style="width: 48px;"
                      onchange={(e) => {
                        const fps =
                          parseFloat((e.target as HTMLInputElement).value) ||
                          10;
                        executePrefab(`Set clip "${clipName}" FPS`, (d) => {
                          const sa = d.components.sprite_animator as
                            | Record<string, unknown>
                            | undefined;
                          const c = (
                            sa?.clips as Record<string, Record<string, unknown>>
                          )?.[clipName];
                          if (c) c.frame_duration = fps > 0 ? 1.0 / fps : 0.1;
                          return d;
                        });
                      }}
                    />
                    <label>Loop</label>
                    <input
                      type="checkbox"
                      checked={clip.looping as boolean}
                      onchange={(e) => {
                        executePrefab(
                          `Toggle clip "${clipName}" looping`,
                          (d) => {
                            const sa = d.components.sprite_animator as
                              | Record<string, unknown>
                              | undefined;
                            const c = (
                              sa?.clips as Record<
                                string,
                                Record<string, unknown>
                              >
                            )?.[clipName];
                            if (c)
                              c.looping = (
                                e.target as HTMLInputElement
                              ).checked;
                            return d;
                          },
                        );
                      }}
                    />
                  </div>
                </div>
              {/if}
            {/each}
          {/if}
          {@render undrawnFields("sprite_animator")}
        {/if}
      </div>
    {/if}

    {#snippet undrawnFields(comp: string)}
      <!-- The component's fields its section here does not draw
           (componentDefs.ts PREFAB_UNDRAWN_FIELDS), with derived widgets. -->
      {@const info = schemaMap.get(comp)}
      {@const names = PREFAB_UNDRAWN_FIELDS[comp] ?? []}
      {@const compData = (data.components as Record<string, unknown>)[comp]}
      {#if info && names.length > 0 && compData && typeof compData === "object"}
        <div class="undrawn-fields">
          <PackageInspector
            info={{ ...info, fields: info.fields.filter((f) => names.includes(f.name)) }}
            data={compData as Record<string, unknown>}
            onchange={(field, value) => updateField(comp, field, value)}
          />
        </div>
      {/if}
    {/snippet}

    <!-- Other known components rendered generically -->
    {#each Object.entries(data.components) as [key, value]}
      {#if !["transform", "sprite", "script", "tags", "rigid_body", "collider", "sprite_animator"].includes(key)}
        {@const compDef = ($project?.component_definitions ?? []).find(
          (d) => d.name === key,
        )}
        {@const compData = (value ?? {}) as Record<string, unknown>}
        <div class="section">
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="section-header"
            role="button"
            tabindex="0"
            onclick={() => toggleSection(key)}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") toggleSection(key);
            }}
          >
            <span class="collapse-arrow">{collapsed[key] ? "▸" : "▾"}</span>
            <span class="section-title"
              >{key
                .replace(/_/g, " ")
                .replace(/\b\w/g, (c) => c.toUpperCase())}</span
            >
            <button
              class="remove-btn"
              onclick={(e) => {
                e.stopPropagation();
                handleRemoveComponent(key);
              }}
              title="Remove {key}">×</button
            >
          </div>
          {#if !collapsed[key]}
            {#if compDef}
              {#each compDef.fields as fieldDef (fieldDef.name)}
                <div class="field-row">
                  <label>{fieldDef.name}</label>
                  {#if fieldDef.field_type === "Float"}
                    <input
                      type="number"
                      step="0.1"
                      value={(compData[fieldDef.name] as number) ?? 0}
                      onchange={(e) =>
                        updateField(
                          key,
                          fieldDef.name,
                          parseFloat((e.target as HTMLInputElement).value) || 0,
                        )}
                    />
                  {:else if fieldDef.field_type === "Int"}
                    <input
                      type="number"
                      step="1"
                      value={(compData[fieldDef.name] as number) ?? 0}
                      onchange={(e) =>
                        updateField(
                          key,
                          fieldDef.name,
                          parseInt((e.target as HTMLInputElement).value) || 0,
                        )}
                    />
                  {:else if fieldDef.field_type === "String"}
                    <input
                      type="text"
                      value={(compData[fieldDef.name] as string) ?? ""}
                      onchange={(e) =>
                        updateField(
                          key,
                          fieldDef.name,
                          (e.target as HTMLInputElement).value,
                        )}
                    />
                  {:else if fieldDef.field_type === "Bool"}
                    <input
                      type="checkbox"
                      checked={!!compData[fieldDef.name]}
                      onchange={(e) =>
                        updateField(
                          key,
                          fieldDef.name,
                          (e.target as HTMLInputElement).checked,
                        )}
                    />
                  {:else if fieldDef.field_type === "Vec2"}
                    {@const vec = Array.isArray(compData[fieldDef.name])
                      ? (compData[fieldDef.name] as number[])
                      : [0, 0]}
                    <input
                      type="number"
                      step="0.1"
                      value={vec[0] ?? 0}
                      onchange={(e) =>
                        updateField(key, fieldDef.name, [
                          parseFloat((e.target as HTMLInputElement).value) || 0,
                          vec[1] ?? 0,
                        ])}
                    />
                    <input
                      type="number"
                      step="0.1"
                      value={vec[1] ?? 0}
                      onchange={(e) =>
                        updateField(key, fieldDef.name, [
                          vec[0] ?? 0,
                          parseFloat((e.target as HTMLInputElement).value) || 0,
                        ])}
                    />
                  {:else if fieldDef.field_type === "Color"}
                    {@const col = Array.isArray(compData[fieldDef.name])
                      ? (compData[fieldDef.name] as number[])
                      : [1, 1, 1, 1]}
                    <input
                      type="color"
                      value={colorToHex([
                        col[0] ?? 1,
                        col[1] ?? 1,
                        col[2] ?? 1,
                        col[3] ?? 1,
                      ])}
                      onchange={(e) => {
                        const hex = (e.target as HTMLInputElement).value;
                        const cr = parseInt(hex.slice(1, 3), 16) / 255;
                        const cg = parseInt(hex.slice(3, 5), 16) / 255;
                        const cb = parseInt(hex.slice(5, 7), 16) / 255;
                        updateField(key, fieldDef.name, [
                          Math.round(cr * 100) / 100,
                          Math.round(cg * 100) / 100,
                          Math.round(cb * 100) / 100,
                          col[3] ?? 1,
                        ]);
                      }}
                    />
                    <input
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
                            parseFloat((e.target as HTMLInputElement).value) ||
                              0,
                          ),
                        );
                        updateField(key, fieldDef.name, [
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
            {:else if schemaMap.get(key)}
              <PackageInspector
                info={schemaMap.get(key)!}
                data={compData}
                onchange={(field, v) => updateField(key, field, v)}
              />
            {:else}
              <pre class="json-preview">{JSON.stringify(value, null, 2)}</pre>
            {/if}
          {/if}
        </div>
      {/if}
    {/each}

    <!-- Add Component -->
    <div class="add-section">
      <div class="add-component-wrapper">
        <button
          class="add-component-btn"
          onclick={() => (showAddMenu = !showAddMenu)}
        >
          + Add Component
        </button>
        {#if showAddMenu}
          <div class="add-menu">
            {#each categorizedMissing as cat}
              <button
                type="button"
                class="add-menu-category"
                onclick={() =>
                  (expandedCategories[cat.key] = !expandedCategories[cat.key])}
              >
                <span class="cat-arrow"
                  >{expandedCategories[cat.key] ? "▾" : "▸"}</span
                >
                <span class="cat-icon">{cat.icon}</span>
                {cat.label}
                <span class="cat-count">{cat.items.length}</span>
              </button>
              {#if expandedCategories[cat.key]}
                {#each cat.items as comp}
                  <button
                    class="add-menu-item"
                    onclick={() => handleAddComponent(comp)}
                  >
                    {formatComponentName(comp)}
                  </button>
                {/each}
              {/if}
            {/each}
            {#if categorizedMissing.length === 0}
              <div class="add-menu-empty">All components added</div>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  {:else}
    <div class="empty">No prefab loaded</div>
  {/if}
</div>

<style>
  .prefab-inspector {
    padding: 0;
  }

  .variant-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
  }
  .variant-row select {
    flex: 1;
    min-width: 0;
  }
  .variant-row code {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 11px;
    color: var(--ed-accent);
  }
  .variant-hint {
    font-size: 10px;
    color: var(--ed-faint);
  }
  .variant-btn {
    background: none;
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    font-size: 11.5px;
    padding: 0 6px;
    cursor: pointer;
  }
  .undrawn-fields {
    border-top: 1px dashed var(--ed-line);
    margin-top: 4px;
    padding-top: 4px;
  }
  .section {
    padding: 8px 10px;
    border-bottom: 1px solid var(--ed-line);
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    margin-bottom: 6px;
    padding: 0;
    background: none;
    border: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
    user-select: none;
    text-align: left;
  }

  .section-header:hover {
    background: var(--ed-raised);
    margin: -2px -4px 4px;
    padding: 2px 4px;
    border-radius: 3px;
  }

  .collapse-arrow {
    font-size: 10px;
    color: var(--ed-faint);
    width: 14px;
    flex-shrink: 0;
  }

  .section-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--ed-muted);
    display: block;
  }

  .remove-btn {
    background: none;
    border: none;
    color: var(--ed-faint);
    cursor: pointer;
    font-size: 14px;
    padding: 0 4px;
    line-height: 1;
  }

  .remove-btn:hover {
    color: var(--ed-err);
  }

  .remove-btn.small {
    font-size: 12px;
    padding: 0 2px;
  }

  .field-row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
  }

  .field-row label {
    font-size: 11px;
    color: var(--ed-faint);
    min-width: 24px;
  }

  input,
  select {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-text);
    padding: 3px 6px;
    font-size: 12px;
    font-family: var(--ed-mono);
    flex: 1;
    min-width: 0;
  }

  input:focus,
  select:focus {
    outline: none;
    border-color: var(--ed-accent);
  }

  input[type="number"] {
    width: 60px;
    flex: 1;
  }

  input[type="color"] {
    width: 32px;
    height: 24px;
    padding: 1px;
    flex: 0 0 32px;
    cursor: pointer;
    border-radius: 4px;
  }

  input[type="checkbox"] {
    flex: 0 0 auto;
    width: auto;
  }

  .add-size-btn {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-accent);
    cursor: pointer;
    font-size: 11.5px;
    padding: 3px 8px;
    flex-shrink: 0;
  }

  .add-size-btn:hover {
    background: var(--ed-line);
  }

  .tag-list {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-bottom: 6px;
  }

  .tag-chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 2px 6px;
    background: color-mix(in srgb, var(--ed-accent) 12%, transparent);
    border-radius: 4px;
    font-size: 11px;
    color: var(--ed-accent);
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

  .json-preview {
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-radius: 3px;
    padding: 6px 8px;
    font-size: 11px;
    color: var(--ed-muted);
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-word;
    margin: 0;
  }

  .add-section {
    padding: 8px 10px;
  }

  .add-component-wrapper {
    position: relative;
  }

  .add-component-btn {
    width: 100%;
    background: var(--ed-raised);
    border: 1px dashed var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-accent);
    cursor: pointer;
    font-size: 12px;
    padding: 6px 0;
    font-family: inherit;
  }

  .add-component-btn:hover {
    background: var(--ed-line);
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
    max-height: 280px;
    overflow-y: auto;
    z-index: 50;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
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
    background: none;
    border: none;
    color: var(--ed-text);
    padding: 5px 10px 5px 26px;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    font-family: inherit;
    border-radius: 4px;
  }

  .add-menu-item:hover {
    background: var(--ed-accent-low);
    color: var(--ed-ink);
  }

  .add-menu-empty {
    padding: 8px 10px;
    font-size: 11px;
    color: var(--ed-faint);
    text-align: center;
  }

  .asset-picker-wrapper {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
    gap: 0;
  }

  .asset-picker-wrapper input {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }

  .picker-btn {
    background: var(--ed-line);
    border: 1px solid var(--ed-line-strong);
    border-left: none;
    border-radius: 0 3px 3px 0;
    color: var(--ed-text);
    cursor: pointer;
    font-size: 12px;
    padding: 2px 6px;
    flex-shrink: 0;
  }

  .picker-btn:hover {
    background: var(--ed-line-strong);
  }

  .picker-backdrop {
    position: fixed;
    top: 0;
    left: 0;
    width: 100vw;
    height: 100vh;
    background: transparent;
    border: none;
    z-index: 99;
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
    max-height: 180px;
    overflow-y: auto;
    z-index: 100;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
  }

  .picker-item {
    display: block;
    width: 100%;
    background: none;
    border: none;
    color: var(--ed-text);
    padding: 4px 8px;
    font-size: 11px;
    text-align: left;
    cursor: pointer;
    font-family: inherit;
    border-radius: 4px;
  }

  .picker-item:hover {
    background: var(--ed-accent-low);
    color: var(--ed-ink);
  }

  .picker-item.active {
    background: var(--ed-line);
    color: var(--ed-accent);
    border-radius: 4px;
  }

  .picker-empty {
    padding: 8px;
    font-size: 11px;
    color: var(--ed-faint);
    text-align: center;
  }

  .dim {
    color: var(--ed-faint);
    font-size: 11px;
  }

  .clip-detail {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    padding: 4px 6px;
    margin-top: 4px;
  }

  .clip-detail-header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 2px;
  }

  .clip-name {
    font-size: 11px;
    font-weight: 600;
    color: var(--ed-a2);
  }

  .clip-detail-fields {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
  }

  .clip-detail-fields label {
    color: var(--ed-faint);
    font-size: 10px;
  }

  .clip-detail-fields input[type="number"] {
    width: 40px;
    flex: 0 0 40px;
    font-size: 11px;
    padding: 1px 4px;
  }

  .clip-detail-fields input[type="checkbox"] {
    flex: 0 0 auto;
  }

  .empty {
    padding: 24px;
    text-align: center;
    color: var(--ed-faint);
    font-size: 13px;
  }
</style>
