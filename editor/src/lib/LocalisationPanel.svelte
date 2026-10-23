<script lang="ts">
  // The localisation panel: preview the scene in a language (every
  // `text_key` written in it, in the scene view only), and the report a
  // translator works from - what each language still lacks, and which keys
  // the open scene and the project's scripts use that no string table has.
  // String tables are `.data` tables under `locale/`, edited in the data
  // table editor; this panel reads them (`localeEdit.ts`).
  import { get } from "svelte/store";
  import { scene } from "./stores/sceneStore";
  import { project } from "./stores/projectStore";
  import { projectAssets } from "./stores/projectAssetsStore";
  import { localeTables, previewLanguage } from "./stores/localeStore";
  import { readAssetBinary } from "./api/project";
  import { openAsset } from "./api/openAsset";
  import { hasKey, languages, missingByLanguage, scriptKeys, textKeysIn } from "./localeEdit";

  let langs = $derived(languages($localeTables));
  let missing = $derived(missingByLanguage($localeTables));
  let tableIds = $derived(Object.keys($localeTables).filter((id) => id.startsWith("locale/")).sort());
  let sceneKeys = $derived(textKeysIn($scene.entities ?? []));
  let unknownSceneKeys = $derived(sceneKeys.filter((k) => !hasKey($localeTables, k)));
  let scriptMissing = $state<{ key: string; file: string }[] | null>(null);
  let scanning = $state(false);
  let open = $state<string | null>(null);

  async function scanScripts() {
    const proj = get(project);
    if (!proj) return;
    scanning = true;
    const decoder = new TextDecoder();
    const found: { key: string; file: string }[] = [];
    for (const rel of $projectAssets.filter((f) => f.endsWith(".lua") || f.endsWith(".luau") || f.endsWith(".js"))) {
      try {
        const src = decoder.decode(new Uint8Array(await readAssetBinary(proj.path, rel)));
        for (const key of scriptKeys(src)) if (!hasKey($localeTables, key)) found.push({ key, file: rel });
      } catch {
        // unreadable: skip
      }
    }
    scriptMissing = found;
    scanning = false;
  }

  function openTable(id: string) {
    const proj = get(project);
    if (proj) void openAsset(proj, `assets/${id}.data`);
  }
</script>

<div class="panel">
  {#if tableIds.length === 0}
    <div class="empty">
      No string tables. A string table is a data table under <code>locale/</code>: a row per key, a column per
      language (<code>en</code>, <code>fi</code>, ...). Text with a <code>text_key</code> shows the key's text.
    </div>
  {:else}
    <div class="bar">
      <label title="Show every text_key in this language in the scene view (nothing is written)">
        Preview
        <select bind:value={$previewLanguage}>
          <option value="">(as authored)</option>
          {#each langs as l}<option value={l}>{l}</option>{/each}
        </select>
      </label>
      <span class="dim">{tableIds.length} table(s), {langs.length} language(s)</span>
      {#each tableIds as id}
        <button class="link" title="Open in the data table editor" onclick={() => openTable(id)}>{id}</button>
      {/each}
    </div>
    <div class="cols">
      <div class="col">
        <div class="title">Untranslated</div>
        {#each langs as l}
          <button class="row" class:ok={missing[l].length === 0} onclick={() => (open = open === l ? null : l)}>
            <span>{l}</span>
            <span>{missing[l].length === 0 ? "complete" : `${missing[l].length} missing`}</span>
          </button>
          {#if open === l && missing[l].length > 0}
            <div class="keys">{missing[l].join(", ")}</div>
          {/if}
        {/each}
      </div>
      <div class="col">
        <div class="title">Keys no table has</div>
        {#if unknownSceneKeys.length > 0}
          <div class="keys bad">This scene: {unknownSceneKeys.join(", ")}</div>
        {:else}
          <div class="dim">This scene's {sceneKeys.length} text_key(s) all resolve.</div>
        {/if}
        <button onclick={scanScripts} disabled={scanning}>{scanning ? "Scanning..." : "Scan scripts for Locale.get keys"}</button>
        {#if scriptMissing}
          {#if scriptMissing.length === 0}
            <div class="dim">Every literal Locale.get key resolves.</div>
          {:else}
            {#each scriptMissing as m}
              <div class="keys bad">{m.key} <span class="dim">({m.file})</span></div>
            {/each}
          {/if}
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .panel {
    height: 100%;
    overflow: auto;
    font-size: 11px;
    color: var(--ed-text);
    background: var(--ed-base);
  }
  .empty {
    padding: 10px;
    color: var(--ed-muted);
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--ed-line);
  }
  .cols {
    display: flex;
    gap: 16px;
    padding: 6px 8px;
  }
  .col {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .title {
    font-weight: 600;
  }
  .row {
    display: flex;
    justify-content: space-between;
    background: none;
    border: none;
    color: var(--ed-warn);
    padding: 1px 0;
    cursor: pointer;
    font-size: 11px;
  }
  .row.ok {
    color: var(--ed-ok);
  }
  .keys {
    font-family: var(--ed-mono);
    color: var(--ed-muted);
    padding-left: 10px;
  }
  .keys.bad {
    color: var(--ed-err);
  }
  .dim {
    color: var(--ed-faint);
  }
  .link {
    background: none;
    border: none;
    color: var(--ed-accent);
    cursor: pointer;
    font-size: 11px;
    padding: 0;
  }
  select,
  button:not(.row):not(.link) {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    font-size: 11px;
    padding: 1px 5px;
  }
</style>
