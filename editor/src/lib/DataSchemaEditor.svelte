<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import {
    dataSchema,
    dataSchemaPath,
    dataSchemaDirty,
    selectedField,
    canUndoSchema,
    canRedoSchema,
    undoSchema,
    redoSchema,
    executeSchema,
    saveCurrentDataSchema,
  } from "./stores/dataSchemaEditorStore";
  import { project } from "./stores/projectStore";
  import SchemaFieldTypeEditor from "./SchemaFieldTypeEditor.svelte";
  import {
    defaultValueForType,
    listDataSchemas,
    type DataFieldType,
    type SchemaEntry,
  } from "./api/dataEditor";

  let availableSchemas = $state<SchemaEntry[]>([]);

  const fileName = $derived(
    $dataSchemaPath?.split(/[\\/]/).pop()?.replace(".dataschema", "") ??
      "Schema",
  );

  onMount(async () => {
    const proj = get(project);
    if (proj) {
      try {
        availableSchemas = await listDataSchemas(proj.path);
      } catch {
        availableSchemas = [];
      }
    }
  });

  function addField() {
    executeSchema("Add field", (s) => {
      const name = uniqueFieldName(s.fields.map((f) => f.name));
      s.fields.push({ name, type: { kind: "string" }, default: "" });
    });
    selectedField.set(($dataSchema?.fields.length ?? 1) - 1);
  }

  function uniqueFieldName(existing: string[]): string {
    let n = 1;
    let name = "field";
    while (existing.includes(name)) name = `field_${n++}`;
    return name;
  }

  function removeField(index: number) {
    executeSchema("Remove field", (s) => {
      s.fields.splice(index, 1);
    });
  }

  function moveField(index: number, dir: -1 | 1) {
    const target = index + dir;
    if (!$dataSchema || target < 0 || target >= $dataSchema.fields.length)
      return;
    executeSchema("Reorder field", (s) => {
      const [f] = s.fields.splice(index, 1);
      s.fields.splice(target, 0, f);
    });
  }

  function setFieldName(index: number, name: string) {
    executeSchema("Rename field", (s) => {
      s.fields[index].name = name;
    });
  }

  function setFieldType(index: number, ty: DataFieldType) {
    executeSchema("Edit field type", (s) => {
      s.fields[index].type = ty;
      s.fields[index].default = defaultValueForType(ty) ?? null;
    });
  }

  function setDefault(index: number, value: unknown) {
    executeSchema("Set default", (s) => {
      s.fields[index].default = value;
    });
  }
</script>

<div class="data-editor">
  <header class="toolbar">
    <span class="title">🧱 {fileName}</span>
    <span class="schema-name">
      Schema name:
      <input
        type="text"
        value={$dataSchema?.name ?? ""}
        oninput={(e) =>
          executeSchema("Rename schema", (s) => {
            s.name = (e.target as HTMLInputElement).value;
          })}
      />
    </span>
    <div class="spacer"></div>
    <button onclick={undoSchema} disabled={!$canUndoSchema} title="Undo"
      >↶</button
    >
    <button onclick={redoSchema} disabled={!$canRedoSchema} title="Redo"
      >↷</button
    >
    <button
      class="primary"
      onclick={saveCurrentDataSchema}
      disabled={!$dataSchemaDirty}
    >
      Save{$dataSchemaDirty ? " •" : ""}
    </button>
  </header>

  <div class="fields">
    <div class="fields-head">
      <span>Fields</span>
      <button class="add" onclick={addField}>+ Add Field</button>
    </div>

    {#if $dataSchema && $dataSchema.fields.length > 0}
      {#each $dataSchema.fields as field, i (i)}
        <div class="field-row">
          <div class="field-main">
            <input
              class="field-name"
              type="text"
              value={field.name}
              oninput={(e) =>
                setFieldName(i, (e.target as HTMLInputElement).value)}
            />
            <div class="field-actions">
              <button title="Move up" onclick={() => moveField(i, -1)}>▲</button
              >
              <button title="Move down" onclick={() => moveField(i, 1)}
                >▼</button
              >
              <button
                class="danger"
                title="Remove"
                onclick={() => removeField(i)}>✕</button
              >
            </div>
          </div>

          <SchemaFieldTypeEditor
            type={field.type}
            {availableSchemas}
            onchange={(t) => setFieldType(i, t)}
          />

          <!-- Simple default-value editors -->
          {#if field.type.kind === "string"}
            <label class="config">
              Default
              <input
                type="text"
                value={(field.default as string) ?? ""}
                oninput={(e) =>
                  setDefault(i, (e.target as HTMLInputElement).value)}
              />
            </label>
          {:else if field.type.kind === "int" || field.type.kind === "float"}
            <label class="config">
              Default
              <input
                type="number"
                step={field.type.kind === "int" ? "1" : "any"}
                value={(field.default as number) ?? 0}
                oninput={(e) =>
                  setDefault(i, Number((e.target as HTMLInputElement).value))}
              />
            </label>
          {:else if field.type.kind === "bool"}
            <label class="config checkbox">
              <input
                type="checkbox"
                checked={Boolean(field.default)}
                onchange={(e) =>
                  setDefault(i, (e.target as HTMLInputElement).checked)}
              />
              Default true
            </label>
          {/if}
        </div>
      {/each}
    {:else}
      <div class="empty">
        No fields yet. Click “+ Add Field” to define this data type.
      </div>
    {/if}
  </div>
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
  .schema-name {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--ed-muted);
  }
  .schema-name input {
    width: 180px;
  }
  .spacer {
    flex: 1;
  }
  .fields {
    flex: 1;
    overflow-y: auto;
    padding: 12px;
  }
  .fields-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
    font-weight: 600;
    color: var(--ed-muted);
  }
  .field-row {
    border: 1px solid var(--ed-line);
    border-radius: 6px;
    padding: 8px;
    margin-bottom: 8px;
    background: var(--ed-raised);
  }
  .field-main {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .field-name {
    flex: 1;
  }
  .field-actions {
    display: flex;
    gap: 4px;
  }
  .config {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 6px;
    color: var(--ed-muted);
  }
  .config.checkbox {
    gap: 6px;
  }
  .config input[type="text"],
  .config input[type="number"] {
    flex: 1;
  }
  input,
  button {
    background: var(--ed-raised);
    color: var(--ed-text);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    padding: 4px 6px;
    font-size: 12px;
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
  .empty {
    color: var(--ed-faint);
    padding: 24px;
    text-align: center;
  }
</style>
