<script lang="ts">
  // The prefab editor's part tree: the prefab and its parts
  // (`EntityData::children`), each in its parent's frame. Click a part to
  // edit it - the stage and the inspector follow (`prefabFocus`); add a
  // part under the selected one, duplicate, remove, move it among its
  // siblings (a later part is spawned after, so drawn over an equal z), or
  // double-click to rename. A scene finds a part by name, so siblings keep
  // different names. The tree logic is `prefabParts.ts`.
  import { prefabDoc, prefabFocus, executePrefabTree } from "./stores/prefabEditorStore";
  import {
    addPart,
    duplicatePart,
    movePart,
    partRows,
    removePart,
    renamePart,
    type PartPath,
  } from "./prefabParts";
  import type { PrefabEntity } from "./prefabInstance";
  import type { EntityData } from "./api/scene";

  const asPrefab = (d: EntityData) => d as unknown as PrefabEntity;
  const asEntity = (p: PrefabEntity) => p as unknown as EntityData;
  const same = (a: PartPath, b: PartPath) => a.length === b.length && a.every((x, i) => x === b[i]);

  let rows = $derived($prefabDoc ? partRows(asPrefab($prefabDoc)) : []);
  let renaming = $state<string | null>(null);
  let renameValue = $state("");

  function add() {
    const doc = $prefabDoc;
    if (!doc) return;
    const r = addPart(asPrefab(doc), $prefabFocus);
    executePrefabTree(() => asEntity(r.root), r.path);
  }
  function duplicate() {
    const doc = $prefabDoc;
    if (!doc || $prefabFocus.length === 0) return;
    const r = duplicatePart(asPrefab(doc), $prefabFocus);
    executePrefabTree(() => asEntity(r.root), r.path);
  }
  function remove() {
    const doc = $prefabDoc;
    if (!doc || $prefabFocus.length === 0) return;
    executePrefabTree(() => asEntity(removePart(asPrefab(doc), $prefabFocus)), $prefabFocus.slice(0, -1));
  }
  function move(by: number) {
    const doc = $prefabDoc;
    if (!doc || $prefabFocus.length === 0) return;
    const r = movePart(asPrefab(doc), $prefabFocus, by);
    executePrefabTree(() => asEntity(r.root), r.path);
  }
  function commitRename(path: PartPath) {
    const doc = $prefabDoc;
    renaming = null;
    if (!doc) return;
    const next = renamePart(asPrefab(doc), path, renameValue);
    if (next !== asPrefab(doc)) executePrefabTree(() => asEntity(next));
  }
</script>

<div class="parts">
  <div class="head">
    <span class="title">Parts</span>
    <button title="Add a part under the selected one" onclick={add}>＋</button>
    <button title="Duplicate the selected part" disabled={$prefabFocus.length === 0} onclick={duplicate}>⧉</button>
    <button title="Move up (drawn earlier)" disabled={$prefabFocus.length === 0} onclick={() => move(-1)}>↑</button>
    <button title="Move down (drawn later)" disabled={$prefabFocus.length === 0} onclick={() => move(1)}>↓</button>
    <button title="Remove the selected part and its parts" disabled={$prefabFocus.length === 0} onclick={remove}>×</button>
  </div>
  <div class="list" role="tree">
    {#each rows as row (row.path.join("/"))}
      {@const key = row.path.join("/")}
      {@const linked = (row.part.components as Record<string, unknown>)?.prefab_instance as { prefab_id?: string } | undefined}
      <div
        class="row"
        class:sel={same(row.path, $prefabFocus)}
        style="padding-left:{6 + row.depth * 14}px"
        role="treeitem"
        aria-selected={same(row.path, $prefabFocus)}
        tabindex="0"
        onclick={() => prefabFocus.set(row.path)}
        onkeydown={(e) => e.key === "Enter" && prefabFocus.set(row.path)}
        ondblclick={() => {
          if (row.path.length > 0) {
            renaming = key;
            renameValue = row.part.name;
          }
        }}
      >
        {#if renaming === key}
          <!-- svelte-ignore a11y_autofocus -->
          <input
            autofocus
            bind:value={renameValue}
            onclick={(e) => e.stopPropagation()}
            onkeydown={(e) => {
              if (e.key === "Enter") commitRename(row.path);
              else if (e.key === "Escape") renaming = null;
            }}
            onblur={() => commitRename(row.path)}
          />
        {:else}
          <span class="icon">{row.depth === 0 ? "🧩" : linked ? "⧉" : "◻"}</span>
          <span class="name">{row.part.name || "(unnamed)"}</span>
          {#if linked}<span class="dim" title="This part is an instance of a prefab">{linked.prefab_id}</span>{/if}
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .parts {
    width: 200px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--ed-line);
    font-size: 12px;
    background: var(--ed-base);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px 6px;
    border-bottom: 1px solid var(--ed-line);
  }
  .title {
    flex: 1;
    font-weight: 600;
  }
  .head button {
    background: none;
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    font-size: 11.5px;
    padding: 0 5px;
    cursor: pointer;
  }
  .head button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .list {
    flex: 1;
    overflow-y: auto;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 3px 6px;
    cursor: pointer;
    white-space: nowrap;
  }
  .row:hover {
    background: var(--ed-raised);
  }
  .row.sel {
    background: var(--ed-line);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dim {
    color: var(--ed-faint);
    font-size: 10px;
  }
  input {
    width: 100%;
    background: var(--ed-base);
    border: 1px solid var(--ed-accent);
    color: var(--ed-text);
    font-size: 12px;
  }
</style>
