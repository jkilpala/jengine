<script lang="ts">
    import { claimUndo } from "./stores/undoTargetStore";
    import { requestEditorClose } from "./stores/editorClose";
  import {
    sortLayersData,
    sortLayersFilePath,
    sortLayersDirty,
    canUndoSortLayers,
    canRedoSortLayers,
    undoSortLayers,
    redoSortLayers,
    addSortLayer,
    removeSortLayer,
    moveSortLayer,
    updateSortLayer,
    saveCurrentSortLayers,
    closeSortLayersEditor,
  } from "./stores/sortLayersEditorStore";

  const fileName = $derived(
    $sortLayersFilePath?.split(/[\\/]/).pop() ?? "Sort Layers",
  );

  // Layers sorted by base_z (ascending = back-to-front) for the
  // "Sorted preview" pane on the right. We keep the editable list in
  // authoring order so reordering is intentional, not a side-effect of
  // editing a number.
  const sortedPreview = $derived(
    [...($sortLayersData?.layers ?? [])].sort((a, b) => a.base_z - b.base_z),
  );

  function handleKey(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      saveCurrentSortLayers();
    }
  }

  function isDuplicate(index: number, name: string): boolean {
    const layers = $sortLayersData?.layers ?? [];
    return layers.some((l, i) => i !== index && l.name === name);
  }

    // Ctrl+Z / Ctrl+Y are this editor's while it is open; the app's key
    // handler asks `undoTarget()` before undoing the scene, so one press
    // no longer undoes in both.
    $effect(() => claimUndo({ undo: undoSortLayers, redo: redoSortLayers }));
</script>

<svelte:window onkeydown={handleKey} />

<div class="sortlayers-editor">
  <header>
    <h2>🪜 {fileName}{$sortLayersDirty ? " •" : ""}</h2>
    <div class="actions">
      <button
        onclick={undoSortLayers}
        disabled={!$canUndoSortLayers}
        title="Undo (Ctrl+Z)">↶ Undo</button
      >
      <button
        onclick={redoSortLayers}
        disabled={!$canRedoSortLayers}
        title="Redo (Ctrl+Shift+Z)">↷ Redo</button
      >
      <button
        onclick={saveCurrentSortLayers}
        disabled={!$sortLayersDirty}
        title="Save (Ctrl+S)">💾 Save</button
      >
      <button onclick={() => requestEditorClose("sortlayers")} title="Close">✕ Close</button>
    </div>
  </header>

  {#if !$sortLayersData}
    <div class="empty">No sort layers file open.</div>
  {:else}
    <div class="body">
      <section class="editor-pane">
        <div class="pane-header">
          <h3>Layers</h3>
          <button class="primary" onclick={() => addSortLayer()}
            >＋ Add layer</button
          >
        </div>

        {#if $sortLayersData.layers.length === 0}
          <div class="empty-list">
            No layers yet. Click <strong>Add layer</strong> to begin.
          </div>
        {:else}
          <div class="layer-table">
            <div class="row header">
              <div class="col-order">Order</div>
              <div class="col-name">Name</div>
              <div class="col-z">Base Z</div>
              <div class="col-actions"></div>
            </div>
            {#each $sortLayersData.layers as layer, i (i)}
              <div class="row" class:dup={isDuplicate(i, layer.name)}>
                <div class="col-order">
                  <button
                    class="icon"
                    disabled={i === 0}
                    title="Move up"
                    onclick={() => moveSortLayer(i, -1)}>▲</button
                  >
                  <button
                    class="icon"
                    disabled={i === $sortLayersData.layers.length - 1}
                    title="Move down"
                    onclick={() => moveSortLayer(i, 1)}>▼</button
                  >
                </div>
                <div class="col-name">
                  <input
                    type="text"
                    value={layer.name}
                    placeholder="layer_name"
                    oninput={(e) =>
                      updateSortLayer(i, {
                        name: (e.currentTarget as HTMLInputElement).value,
                      })}
                  />
                  {#if isDuplicate(i, layer.name)}
                    <span class="warn" title="Duplicate name">⚠</span>
                  {/if}
                </div>
                <div class="col-z">
                  <input
                    type="number"
                    value={layer.base_z}
                    step="100"
                    oninput={(e) => {
                      const v = parseInt(
                        (e.currentTarget as HTMLInputElement).value,
                        10,
                      );
                      if (!isNaN(v)) updateSortLayer(i, { base_z: v });
                    }}
                  />
                </div>
                <div class="col-actions">
                  <button
                    class="icon danger"
                    title="Remove layer"
                    onclick={() => removeSortLayer(i)}>🗑</button
                  >
                </div>
              </div>
            {/each}
          </div>
        {/if}

        <p class="hint">
          Render order is determined by <code>base_z + sprite.z_order</code>.
          Lower values draw first (behind), higher draw last (in front).
          Reference this file from a scene's <code>sort_layers</code> field,
          then tag sprites by name in the Inspector.
        </p>
      </section>

      <section class="preview-pane">
        <h3>Sorted preview (back → front)</h3>
        {#if sortedPreview.length === 0}
          <div class="empty-list">Add layers to see the order.</div>
        {:else}
          <ol>
            {#each sortedPreview as layer (layer.name + "@" + layer.base_z)}
              <li>
                <span class="lname">{layer.name || "(unnamed)"}</span>
                <span class="lz">{layer.base_z}</span>
              </li>
            {/each}
          </ol>
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .sortlayers-editor {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    background: var(--ed-base);
    color: var(--ed-text);
    font-family: var(--ed-sans);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--ed-line);
    background: var(--ed-mantle);
  }
  header h2 {
    font-size: 14px;
    margin: 0;
    font-weight: 600;
  }
  .actions {
    display: flex;
    gap: 6px;
  }
  .actions button {
    background: var(--ed-raised);
    color: var(--ed-text);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    padding: 4px 10px;
    font-size: 12px;
    cursor: pointer;
  }
  .actions button:hover:not(:disabled) {
    background: var(--ed-hover);
  }
  .actions button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .empty,
  .empty-list {
    padding: 24px;
    color: var(--ed-faint);
    text-align: center;
    font-size: 13px;
  }
  .body {
    display: grid;
    grid-template-columns: 1fr 280px;
    gap: 12px;
    padding: 12px;
    flex: 1;
    overflow: auto;
  }
  section {
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    padding: 12px;
    overflow: auto;
  }
  .pane-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 10px;
  }
  h3 {
    margin: 0;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--ed-muted);
    font-weight: 600;
  }
  .primary {
    background: var(--ed-accent);
    color: var(--ed-on-accent);
    border: none;
    border-radius: 3px;
    padding: 4px 10px;
    font-size: 12px;
    cursor: pointer;
  }
  .primary:hover {
    filter: brightness(1.1);
  }
  .layer-table {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .row {
    display: grid;
    grid-template-columns: 70px 1fr 110px 40px;
    align-items: center;
    gap: 8px;
    padding: 4px 6px;
    border-radius: 3px;
  }
  .row.header {
    color: var(--ed-faint);
    font-size: 11px;
    text-transform: uppercase;
    border-bottom: 1px solid var(--ed-line);
    padding-bottom: 6px;
    margin-bottom: 4px;
  }
  .row.dup {
    background: color-mix(in srgb, var(--ed-err) 8%, transparent);
  }
  .col-order {
    display: flex;
    gap: 2px;
  }
  .col-name {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .col-name input,
  .col-z input {
    width: 100%;
    background: var(--ed-raised);
    color: var(--ed-text);
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    padding: 3px 6px;
    font-size: 12px;
    font-family: var(--ed-mono);
  }
  .col-name input:focus,
  .col-z input:focus {
    outline: none;
    border-color: var(--ed-accent);
  }
  .col-actions {
    display: flex;
    justify-content: flex-end;
  }
  .icon {
    background: transparent;
    border: 1px solid var(--ed-line);
    color: var(--ed-muted);
    border-radius: 3px;
    padding: 2px 6px;
    font-size: 10px;
    cursor: pointer;
    line-height: 1;
  }
  .icon:hover:not(:disabled) {
    background: var(--ed-hover);
    color: var(--ed-ink);
  }
  .icon:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }
  .icon.danger:hover:not(:disabled) {
    background: var(--ed-err);
    color: var(--ed-on-accent);
  }
  .warn {
    color: var(--ed-warn);
    font-size: 12px;
  }
  .hint {
    margin-top: 14px;
    padding: 8px 10px;
    background: var(--ed-raised);
    border-left: 3px solid var(--ed-accent);
    border-radius: 2px;
    font-size: 11px;
    color: var(--ed-muted);
    line-height: 1.5;
  }
  .hint code {
    background: var(--ed-crust);
    padding: 1px 4px;
    border-radius: 2px;
    color: var(--ed-a2);
    font-size: 11px;
  }
  .preview-pane ol {
    margin: 0;
    padding-left: 18px;
    font-family: var(--ed-mono);
    font-size: 12px;
  }
  .preview-pane li {
    display: flex;
    justify-content: space-between;
    padding: 2px 0;
    border-bottom: 1px dotted var(--ed-line);
  }
  .lname {
    color: var(--ed-text);
  }
  .lz {
    color: var(--ed-faint);
  }
</style>
