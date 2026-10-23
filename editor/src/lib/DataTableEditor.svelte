<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import {
    dataTable,
    dataTableSchema,
    dataTablePath,
    dataTableDirty,
    selectedRow,
    canUndoTable,
    canRedoTable,
    undoTable,
    redoTable,
    executeTable,
    saveCurrentDataTable,
    makeDefaultRow,
  } from "./stores/dataTableEditorStore";
  import { project } from "./stores/projectStore";
  import DataValueEditor from "./DataValueEditor.svelte";
  import {
    listDataTablesForSchema,
    type DataFieldDef,
    type TableRowsEntry,
  } from "./api/dataEditor";

  const fileName = $derived(
    $dataTablePath?.split(/[\\/]/).pop()?.replace(".data", "") ?? "Table",
  );

  const fields = $derived($dataTableSchema?.fields ?? []);

  /** Cached reference options, keyed by target schema name. */
  let refOptions = $state<Record<string, TableRowsEntry[]>>({});

  onMount(loadRefOptions);

  // Reload reference options whenever the schema changes.
  $effect(() => {
    void $dataTableSchema;
    loadRefOptions();
  });

  async function loadRefOptions() {
    const proj = get(project);
    const schema = $dataTableSchema;
    if (!proj || !schema) return;
    const targets = new Set<string>();
    for (const f of schema.fields) {
      if (f.type.kind === "ref") targets.add(f.type.schema);
      if (f.type.kind === "array" && f.type.element.kind === "ref")
        targets.add(f.type.element.schema);
    }
    const next: Record<string, TableRowsEntry[]> = {};
    for (const target of targets) {
      if (!target) continue;
      try {
        next[target] = await listDataTablesForSchema(proj.path, target);
      } catch {
        next[target] = [];
      }
    }
    refOptions = next;
  }

  // -------------------------------------------------------------------------
  // Row operations
  // -------------------------------------------------------------------------
  function addRow() {
    const schema = $dataTableSchema;
    if (!schema) return;
    executeTable("Add row", (t) => {
      t.rows.push(
        makeDefaultRow(
          schema,
          t.rows.map((r) => r.id),
        ),
      );
    });
    selectedRow.set(($dataTable?.rows.length ?? 1) - 1);
  }

  function duplicateRow(index: number) {
    executeTable("Duplicate row", (t) => {
      const src = t.rows[index];
      const clone = structuredClone(src);
      let n = 1;
      let id = `${src.id}_copy`;
      const ids = t.rows.map((r) => r.id);
      while (ids.includes(id)) id = `${src.id}_copy_${n++}`;
      clone.id = id;
      t.rows.splice(index + 1, 0, clone);
    });
  }

  function deleteRow(index: number) {
    executeTable("Delete row", (t) => {
      t.rows.splice(index, 1);
    });
  }

  function setId(index: number, value: string) {
    executeTable("Edit id", (t) => {
      t.rows[index].id = value;
    });
  }

  function setCell(index: number, field: string, value: unknown) {
    executeTable("Edit cell", (t) => {
      t.rows[index][field] = value;
    });
  }

  function fieldTitle(f: DataFieldDef): string {
    if (f.type.kind === "ref") return `${f.name} → ${f.type.schema}`;
    if (f.type.kind === "array") return `${f.name} [${f.type.element.kind}]`;
    return `${f.name} (${f.type.kind})`;
  }
</script>

<div class="data-editor">
  <header class="toolbar">
    <span class="title">🗃 {fileName}</span>
    <span class="schema-badge">
      schema: <strong>{$dataTable?.schema ?? "?"}</strong>
    </span>
    {#if !$dataTableSchema}
      <span class="warn">⚠ schema not found — cannot edit rows</span>
    {/if}
    <div class="spacer"></div>
    <button onclick={undoTable} disabled={!$canUndoTable} title="Undo">↶</button
    >
    <button onclick={redoTable} disabled={!$canRedoTable} title="Redo">↷</button
    >
    <button class="add" onclick={addRow} disabled={!$dataTableSchema}
      >+ Add Row</button
    >
    <button
      class="primary"
      onclick={saveCurrentDataTable}
      disabled={!$dataTableDirty}
    >
      Save{$dataTableDirty ? " •" : ""}
    </button>
  </header>

  {#if $dataTable && $dataTableSchema}
    <div class="grid-wrap">
      <table class="grid">
        <thead>
          <tr>
            <th class="id-col">id</th>
            {#each fields as f}
              <th title={fieldTitle(f)}>{f.name}</th>
            {/each}
            <th class="actions-col"></th>
          </tr>
        </thead>
        <tbody>
          {#each $dataTable.rows as row, ri (ri)}
            <tr
              class:selected={$selectedRow === ri}
              onclick={() => selectedRow.set(ri)}
            >
              <td class="id-col">
                <input
                  type="text"
                  value={row.id}
                  oninput={(e) =>
                    setId(ri, (e.target as HTMLInputElement).value)}
                />
              </td>
              {#each fields as f}
                <td>
                  <DataValueEditor
                    type={f.type}
                    value={row[f.name]}
                    {refOptions}
                    onchange={(v) => setCell(ri, f.name, v)}
                  />
                </td>
              {/each}
              <td class="actions-col">
                <button
                  class="tiny"
                  title="Duplicate"
                  onclick={() => duplicateRow(ri)}>⧉</button
                >
                <button
                  class="tiny danger"
                  title="Delete"
                  onclick={() => deleteRow(ri)}>✕</button
                >
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if $dataTable.rows.length === 0}
        <div class="empty">
          No rows yet. Click “+ Add Row” to create data objects.
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .data-editor {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--ed-base);
    color: var(--ed-text);
    font-size: 13px;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--ed-line);
    background: var(--ed-mantle);
  }
  .title {
    font-weight: 600;
  }
  .schema-badge {
    color: var(--ed-muted);
  }
  .warn {
    color: var(--ed-warn);
  }
  .spacer {
    flex: 1;
  }
  .grid-wrap {
    flex: 1;
    overflow: auto;
    padding: 8px;
  }
  table.grid {
    border-collapse: collapse;
    width: 100%;
  }
  th,
  td {
    border: 1px solid var(--ed-line);
    padding: 2px 4px;
    text-align: left;
    vertical-align: top;
  }
  th {
    position: sticky;
    top: 0;
    background: var(--ed-mantle);
    color: var(--ed-muted);
    white-space: nowrap;
  }
  tr.selected td {
    background: var(--ed-accent-low);
  }
  .id-col input {
    font-weight: 600;
    width: 110px;
  }
  .actions-col {
    white-space: nowrap;
  }
  input,
  button {
    background: var(--ed-raised);
    color: var(--ed-text);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    padding: 3px 5px;
    font-size: 12px;
    box-sizing: border-box;
  }
  input[type="text"] {
    width: 100%;
  }
  button {
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    background: var(--ed-line);
  }
  button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  button.primary {
    background: var(--ed-accent);
    color: var(--ed-on-accent);
    border-color: var(--ed-accent);
    font-weight: 600;
  }
  button.add {
    background: var(--ed-ok);
    color: var(--ed-on-accent);
    border-color: var(--ed-ok);
  }
  button.danger {
    color: var(--ed-err);
    background: color-mix(in srgb, var(--ed-err) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--ed-err) 50%, transparent);
  }
  button.tiny {
    padding: 1px 5px;
  }
  .empty {
    color: var(--ed-faint);
    padding: 24px;
    text-align: center;
  }
</style>
