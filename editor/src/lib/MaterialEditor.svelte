<script lang="ts">
  // The material editor: a `.material` file as a form - the shader graph it
  // draws with, its blend mode, and a row per parameter the graph reads
  // (its `MaterialParam` nodes): the file's value is every wearer's default,
  // under what an entity sets with `Material.set_uniform_*`. Undo is the
  // settle-based history; *Open as text* hands the file to the code editor.
  import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
  import { openInCodeEditor } from "./api/project";
  import { project, requestAssetRefresh } from "./stores/projectStore";
  import { projectAssets } from "./stores/projectAssetsStore";
  import { assetsWithExtensions } from "./fieldValues";
  import { logError, logInfo } from "./stores/consoleStore";
  import { SettledHistory } from "./settledHistory";
  import { claimUndo } from "./stores/undoTargetStore";
  import { hexOf, rgbOf, splitIntensity } from "./curves";
  import {
    BLEND_MODES,
    formatMaterial,
    graphParams,
    parseMaterial,
    uniformToVec4,
    withGraph,
    withUniform,
    type MaterialFile,
  } from "./materialEdit";

  let { projectPath, relativePath, onclose }: { projectPath: string; relativePath: string; onclose: () => void } =
    $props();

  let material = $state<MaterialFile>({ shader: "default_sprite" });
  let dirty = $state(false);
  let error = $state("");
  let params = $state<string[]>([]);

  const history = new SettledHistory<MaterialFile>(
    () => $state.snapshot(material) as MaterialFile,
    (v) => {
      material = v;
      dirty = true;
    },
  );
  $effect(() => {
    JSON.stringify(material);
    history.observe();
  });
  $effect(() => claimUndo({ undo: () => history.undo(), redo: () => history.redo() }));

  $effect(() => {
    void load(relativePath);
  });

  async function load(path: string) {
    try {
      material = parseMaterial(await readTextFile(`${projectPath}/${path}`));
      dirty = false;
      history.reset();
    } catch (e) {
      error = `Could not read ${path}: ${e}`;
    }
  }

  // The graph's parameters, read from the graph file whenever it changes.
  $effect(() => {
    const graph = material.shader_graph_path;
    if (!graph) {
      params = [];
      return;
    }
    const rel = graph.startsWith("assets/") ? graph : `assets/${graph}`;
    readTextFile(`${projectPath}/${rel}`)
      .then((text) => (params = graphParams(JSON.parse(text))))
      .catch(() => (params = []));
  });

  let graphs = $derived(assetsWithExtensions($projectAssets, ["shader"]).map((p) => `assets/${p}`));
  let extra = $derived(Object.keys(material.uniforms ?? {}).filter((k) => !params.includes(k)));

  function edit(next: MaterialFile) {
    material = next;
    dirty = true;
  }

  function setChannel(name: string, i: number, value: number) {
    const v = uniformToVec4(material.uniforms?.[name]);
    v[i] = value;
    edit(withUniform(material, name, v));
  }

  async function save() {
    try {
      await writeTextFile(`${projectPath}/${relativePath}`, formatMaterial($state.snapshot(material) as MaterialFile));
      dirty = false;
      logInfo(`Saved ${relativePath}`);
      requestAssetRefresh();
    } catch (e) {
      logError(`Could not save ${relativePath}: ${e}`);
    }
  }

  async function openAsText() {
    try {
      await openInCodeEditor(projectPath, relativePath, $project?.settings.code_editor ?? "");
    } catch (e) {
      logError(`${e}`);
    }
  }

  function close() {
    if (dirty && !confirm(`Discard the changes to ${relativePath}?`)) return;
    onclose();
  }
</script>

<div class="overlay" role="dialog" aria-label="Material editor">
  <div class="window">
    <div class="head">
      <span class="title">{relativePath.split("/").pop()}{dirty ? " *" : ""}</span>
      <button onclick={openAsText} title="Open the file in the code editor">Open as text</button>
      <button class="save" disabled={!dirty} onclick={save}>Save</button>
      <button onclick={close} title="Close">×</button>
    </div>
    {#if error}<div class="error">{error}</div>{/if}
    <div class="form">
      <label class="row">
        <span>Shader graph</span>
        <input
          list="material-graphs"
          value={material.shader_graph_path ?? ""}
          placeholder="(none - the default sprite shader)"
          onchange={(e) => edit(withGraph(material, (e.target as HTMLInputElement).value))}
        />
        <datalist id="material-graphs">
          {#each graphs as g}<option value={g}></option>{/each}
        </datalist>
      </label>
      <label class="row">
        <span>Blend mode</span>
        <select
          value={material.blend_mode ?? "Alpha"}
          onchange={(e) => edit({ ...material, blend_mode: (e.target as HTMLSelectElement).value })}
        >
          {#each BLEND_MODES as b}<option value={b}>{b}</option>{/each}
        </select>
      </label>

      <div class="section">
        <div class="section-title">Parameters</div>
        {#if !material.shader_graph_path}
          <div class="hint">A material with a shader graph has the parameters its MaterialParam nodes read.</div>
        {:else if params.length === 0}
          <div class="hint">The graph reads no parameters (no MaterialParam nodes).</div>
        {/if}
        {#each [...params, ...extra] as name (name)}
          {@const set = material.uniforms?.[name] !== undefined}
          {@const v = uniformToVec4(material.uniforms?.[name])}
          {@const colour = splitIntensity([v[0], v[1], v[2]])}
          <div class="param" class:unused={!params.includes(name)}>
            <span class="pname" title={params.includes(name) ? "Read by the graph" : "Not read by this graph"}>{name}</span>
            {#each [0, 1, 2, 3] as i}
              <input
                type="number"
                step="0.05"
                value={v[i]}
                onchange={(e) => setChannel(name, i, parseFloat((e.target as HTMLInputElement).value) || 0)}
              />
            {/each}
            <input
              type="color"
              title="As a colour (linear, shown in sRGB); the fourth number is untouched"
              value={hexOf(colour.rgb)}
              onchange={(e) => {
                const [r, g, b] = rgbOf((e.target as HTMLInputElement).value);
                edit(withUniform(material, name, [r * colour.intensity, g * colour.intensity, b * colour.intensity, v[3]]));
              }}
            />
            {#if set}
              <button title="Clear: no default (0)" onclick={() => edit(withUniform(material, name, null))}>⟲</button>
            {/if}
          </div>
        {/each}
        <div class="hint">
          A value here is every wearer's default; an entity's own (Material.set_uniform_*) replaces it.
        </div>
      </div>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .window {
    width: min(620px, 94vw);
    max-height: 86vh;
    overflow-y: auto;
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    border-radius: 6px;
    color: var(--ed-text);
    font-size: 12px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--ed-line);
  }
  .title {
    flex: 1;
    font-weight: 600;
  }
  .form {
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .row span {
    min-width: 100px;
    color: var(--ed-muted);
  }
  .row input,
  .row select {
    flex: 1;
  }
  input,
  select,
  button {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    font-size: 11.5px;
    padding: 3px 5px;
  }
  button {
    cursor: pointer;
  }
  .save:not(:disabled) {
    border-color: var(--ed-accent);
  }
  .section-title {
    font-weight: 600;
    margin-bottom: 4px;
  }
  .param {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 0;
  }
  .param.unused .pname {
    color: var(--ed-faint);
    text-decoration: line-through;
  }
  .pname {
    min-width: 100px;
    font-family: var(--ed-mono);
  }
  .param input[type="number"] {
    width: 58px;
  }
  .param input[type="color"] {
    width: 26px;
    height: 20px;
    padding: 0;
  }
  .hint {
    color: var(--ed-faint);
    font-size: 11px;
  }
  .error {
    color: var(--ed-err);
    padding: 6px 10px;
  }
</style>
