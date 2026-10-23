<script lang="ts">
    import { requestEditorClose } from "./stores/editorClose";
  import {
    prefabData,
    prefabFilePath,
    prefabDirty,
    canUndo,
    canRedo,
    saveCurrentPrefab,
    closePrefabEditor,
    undoPrefab,
    redoPrefab,
  } from "./stores/prefabEditorStore";
  import PrefabStage from "./PrefabStage.svelte";
  import PrefabPartTree from "./PrefabPartTree.svelte";

  let fileName = $derived(
    $prefabFilePath?.split(/[\\/]/).pop()?.replace(".prefab", "") ?? "Prefab",
  );
</script>

<div class="prefab-editor">
  <!-- Toolbar -->
  <div class="pe-toolbar">
    <span class="pe-icon">🧩</span>
    <span class="pe-title">{fileName}</span>
    <div class="pe-spacer"></div>
    <button
      class="pe-btn"
      title="Undo (Ctrl+Z)"
      disabled={!$canUndo}
      onclick={undoPrefab}>↩</button
    >
    <button
      class="pe-btn"
      title="Redo (Ctrl+Y)"
      disabled={!$canRedo}
      onclick={redoPrefab}>↪</button
    >
    <button
      class="pe-btn pe-save"
      title="Save prefab"
      disabled={!$prefabDirty}
      onclick={saveCurrentPrefab}>💾 Save</button
    >
    <button
      class="pe-btn pe-close"
      title="Close prefab editor"
      onclick={() => requestEditorClose("prefab")}>✕</button
    >
  </div>

  <!-- Content area -->
  <div class="pe-content">
    <div class="pe-body">
      <PrefabPartTree />
      <PrefabStage />
    </div>
    {#if $prefabData}
      <div class="pe-components">
        {#each Object.keys($prefabData.components) as comp}
          <span class="pe-comp-badge">{comp}</span>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .prefab-editor {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--ed-base);
    color: var(--ed-text);
  }

  .pe-toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 8px;
    height: 36px;
    background: var(--ed-base);
    border-bottom: 1px solid var(--ed-line);
    flex-shrink: 0;
  }

  .pe-icon {
    font-size: 16px;
  }

  .pe-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--ed-text);
  }

  .pe-spacer {
    flex: 1;
  }

  .pe-btn {
    background: none;
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    padding: 4px 8px;
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
    transition: background 0.15s;
  }

  .pe-btn:hover:not(:disabled) {
    background: var(--ed-raised);
  }

  .pe-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .pe-save {
    color: var(--ed-accent);
    border-color: var(--ed-accent);
  }

  .pe-close {
    color: var(--ed-err);
  }

  .pe-content {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .pe-components {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding: 6px 8px;
    border-top: 1px solid var(--ed-line);
    max-height: 64px;
    overflow-y: auto;
    flex-shrink: 0;
  }

  .pe-comp-badge {
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--ed-accent) 12%, transparent);
    color: var(--ed-accent);
    text-transform: capitalize;
  }

  .pe-body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
</style>
