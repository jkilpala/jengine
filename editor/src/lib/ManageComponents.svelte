<script lang="ts">
  import { get } from "svelte/store";
  import { project } from "./stores/projectStore";
  import { saveProject } from "./api/project";
  import { logInfo, logError } from "./stores/consoleStore";
  import type {
    ComponentDef,
    ComponentFieldDef,
    FieldType,
  } from "./api/project";

  let { onclose }: { onclose: () => void } = $props();

  const proj = get(project);
  let definitions: ComponentDef[] = $state(
    structuredClone(proj?.component_definitions ?? []),
  );

  let selectedIndex = $state<number>(-1);

  const FIELD_TYPES: FieldType[] = [
    "Float",
    "Int",
    "String",
    "Vec2",
    "Bool",
    "Color",
  ];

  function defaultValueForType(ft: FieldType): unknown {
    switch (ft) {
      case "Float":
        return 0.0;
      case "Int":
        return 0;
      case "String":
        return "";
      case "Vec2":
        return [0, 0];
      case "Bool":
        return false;
      case "Color":
        return [1, 1, 1, 1];
    }
  }

  function handleAddComponent() {
    let name = "NewComponent";
    let i = 1;
    while (definitions.some((d) => d.name === name)) {
      name = `NewComponent${i++}`;
    }
    definitions.push({ name, fields: [] });
    definitions = definitions;
    selectedIndex = definitions.length - 1;
  }

  function handleRemoveComponent(idx: number) {
    definitions.splice(idx, 1);
    definitions = definitions;
    if (selectedIndex >= definitions.length) {
      selectedIndex = definitions.length - 1;
    }
  }

  function handleAddField() {
    if (selectedIndex < 0 || selectedIndex >= definitions.length) return;
    const comp = definitions[selectedIndex];
    let name = "field";
    let i = 1;
    while (comp.fields.some((f) => f.name === name)) {
      name = `field${i++}`;
    }
    comp.fields.push({
      name,
      field_type: "Float",
      default_value: 0.0,
    });
    definitions = definitions;
  }

  function handleRemoveField(fieldIdx: number) {
    if (selectedIndex < 0 || selectedIndex >= definitions.length) return;
    definitions[selectedIndex].fields.splice(fieldIdx, 1);
    definitions = definitions;
  }

  function handleFieldTypeChange(fieldIdx: number, newType: FieldType) {
    if (selectedIndex < 0) return;
    const field = definitions[selectedIndex].fields[fieldIdx];
    field.field_type = newType;
    field.default_value = defaultValueForType(newType);
    definitions = definitions;
  }

  async function handleSave() {
    const p = get(project);
    if (!p) return;
    // Validate: no duplicate names
    const names = new Set<string>();
    for (const def of definitions) {
      if (!def.name.trim()) {
        logError("Component name cannot be empty");
        return;
      }
      if (names.has(def.name)) {
        logError(`Duplicate component name: "${def.name}"`);
        return;
      }
      names.add(def.name);
      // Validate field names
      const fieldNames = new Set<string>();
      for (const f of def.fields) {
        if (!f.name.trim()) {
          logError(`Field name cannot be empty in "${def.name}"`);
          return;
        }
        if (fieldNames.has(f.name)) {
          logError(`Duplicate field "${f.name}" in "${def.name}"`);
          return;
        }
        fieldNames.add(f.name);
      }
    }
    try {
      const snapshot = $state.snapshot(definitions) as ComponentDef[];
      await saveProject(p.path, p.name, p.default_scene, p.settings, snapshot);
      project.update((pp) => {
        if (!pp) return pp;
        return { ...pp, component_definitions: snapshot };
      });
      logInfo("Component definitions saved");
      onclose();
    } catch (e) {
      logError(`Failed to save component definitions: ${e}`);
    }
  }

  let selectedDef: ComponentDef | null = $derived(
    selectedIndex >= 0 && selectedIndex < definitions.length
      ? definitions[selectedIndex]
      : null,
  );
</script>

<div class="overlay" role="presentation">
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="dialog"
    role="dialog"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
  >
    <div class="dialog-header">
      <span>Manage Custom Components</span>
      <button class="close-btn" onclick={onclose}>×</button>
    </div>
    <div class="dialog-body">
      <div class="left-list">
        <div class="list-header">
          <span>Components</span>
          <button class="small-btn" onclick={handleAddComponent}>+ New</button>
        </div>
        <div class="list-items">
          {#each definitions as def, i}
            <button
              class="list-item"
              class:selected={i === selectedIndex}
              onclick={() => (selectedIndex = i)}
            >
              {def.name}
              <span
                class="remove-inline"
                role="button"
                tabindex="0"
                onclick={(e) => {
                  e.stopPropagation();
                  handleRemoveComponent(i);
                }}
                onkeydown={(e) => {
                  e.stopPropagation();
                  if (e.key === "Enter") handleRemoveComponent(i);
                }}>×</span
              >
            </button>
          {/each}
          {#if definitions.length === 0}
            <div class="empty-hint">No custom components yet</div>
          {/if}
        </div>
      </div>
      <div class="right-editor">
        {#if selectedDef}
          <div class="field-row">
            <label for="comp-def-name">Name</label>
            <input
              id="comp-def-name"
              type="text"
              bind:value={selectedDef.name}
            />
          </div>
          <div class="fields-header">
            <span>Fields</span>
            <button class="small-btn" onclick={handleAddField}
              >+ Add Field</button
            >
          </div>
          <div class="fields-list">
            {#each selectedDef.fields as field, fi}
              <div class="field-entry">
                <div class="field-main-row">
                  <input
                    type="text"
                    class="field-name"
                    bind:value={field.name}
                    placeholder="field name"
                  />
                  <select
                    value={field.field_type}
                    onchange={(e) =>
                      handleFieldTypeChange(
                        fi,
                        (e.target as HTMLSelectElement).value as FieldType,
                      )}
                  >
                    {#each FIELD_TYPES as ft}
                      <option value={ft}>{ft}</option>
                    {/each}
                  </select>
                  <button
                    class="remove-field-btn"
                    onclick={() => handleRemoveField(fi)}>×</button
                  >
                </div>
                <div class="field-default-row">
                  <span class="default-label">default</span>
                  {#if field.field_type === "Float"}
                    <input
                      type="number"
                      step="any"
                      value={field.default_value as number}
                      onchange={(e) => {
                        field.default_value =
                          parseFloat((e.target as HTMLInputElement).value) || 0;
                        definitions = definitions;
                      }}
                    />
                  {:else if field.field_type === "Int"}
                    <input
                      type="number"
                      step="1"
                      value={field.default_value as number}
                      onchange={(e) => {
                        field.default_value =
                          parseInt((e.target as HTMLInputElement).value) || 0;
                        definitions = definitions;
                      }}
                    />
                  {:else if field.field_type === "String"}
                    <input
                      type="text"
                      value={field.default_value as string}
                      oninput={(e) => {
                        field.default_value = (
                          e.target as HTMLInputElement
                        ).value;
                        definitions = definitions;
                      }}
                    />
                  {:else if field.field_type === "Vec2"}
                    <input
                      type="number"
                      step="any"
                      class="vec-input"
                      placeholder="x"
                      value={(field.default_value as number[])[0]}
                      onchange={(e) => {
                        (field.default_value as number[])[0] =
                          parseFloat((e.target as HTMLInputElement).value) || 0;
                        definitions = definitions;
                      }}
                    />
                    <input
                      type="number"
                      step="any"
                      class="vec-input"
                      placeholder="y"
                      value={(field.default_value as number[])[1]}
                      onchange={(e) => {
                        (field.default_value as number[])[1] =
                          parseFloat((e.target as HTMLInputElement).value) || 0;
                        definitions = definitions;
                      }}
                    />
                  {:else if field.field_type === "Bool"}
                    <input
                      type="checkbox"
                      class="bool-input"
                      checked={field.default_value as boolean}
                      onchange={(e) => {
                        field.default_value = (
                          e.target as HTMLInputElement
                        ).checked;
                        definitions = definitions;
                      }}
                    />
                  {:else if field.field_type === "Color"}
                    {#each ["r", "g", "b", "a"] as ch, ci}
                      <input
                        type="number"
                        step="0.01"
                        min="0"
                        max="1"
                        class="color-input"
                        placeholder={ch}
                        value={(field.default_value as number[])[ci]}
                        onchange={(e) => {
                          (field.default_value as number[])[ci] = Math.min(
                            1,
                            Math.max(
                              0,
                              parseFloat(
                                (e.target as HTMLInputElement).value,
                              ) || 0,
                            ),
                          );
                          definitions = definitions;
                        }}
                      />
                    {/each}
                  {/if}
                </div>
              </div>
            {/each}
            {#if selectedDef.fields.length === 0}
              <div class="empty-hint">No fields — click "+ Add Field"</div>
            {/if}
          </div>
        {:else}
          <div class="empty-hint center">
            Select a component or create a new one
          </div>
        {/if}
      </div>
    </div>
    <div class="dialog-footer">
      <button class="btn cancel" onclick={onclose}>Cancel</button>
      <button class="btn save" onclick={handleSave}>Save</button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .dialog {
    background: var(--ed-base);
    border: 1px solid var(--ed-line-strong);
    border-radius: 8px;
    width: 620px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
  }
  .dialog-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 14px;
    border-bottom: 1px solid var(--ed-line);
    font-weight: 600;
    font-size: 13px;
    color: var(--ed-text);
  }
  .close-btn {
    background: none;
    border: none;
    color: var(--ed-faint);
    font-size: 18px;
    cursor: pointer;
  }
  .close-btn:hover {
    color: var(--ed-err);
  }
  .dialog-body {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .left-list {
    width: 180px;
    border-right: 1px solid var(--ed-line);
    display: flex;
    flex-direction: column;
  }
  .list-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 10px;
    font-size: 11px;
    font-weight: 600;
    color: var(--ed-muted);
    border-bottom: 1px solid var(--ed-line);
  }
  .list-items {
    flex: 1;
    overflow-y: auto;
  }
  .list-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: var(--ed-text);
    padding: 6px 10px;
    font-size: 12px;
    cursor: pointer;
    font-family: inherit;
  }
  .list-item:hover {
    background: var(--ed-raised);
  }
  .list-item.selected {
    background: var(--ed-line);
  }
  .remove-inline {
    color: var(--ed-faint);
    font-size: 14px;
    cursor: pointer;
  }
  .remove-inline:hover {
    color: var(--ed-err);
  }
  .right-editor {
    flex: 1;
    padding: 10px 14px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .field-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .field-row label {
    font-size: 11px;
    color: var(--ed-faint);
    min-width: 40px;
  }
  .fields-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 11px;
    font-weight: 600;
    color: var(--ed-muted);
    padding-top: 6px;
    border-top: 1px solid var(--ed-line);
  }
  .fields-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .field-entry {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--ed-line);
  }
  .field-entry:last-child {
    border-bottom: none;
  }
  .field-main-row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .field-name {
    flex: 1;
  }
  .field-default-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding-left: 4px;
  }
  .default-label {
    font-size: 10px;
    color: var(--ed-faint);
    min-width: 38px;
    flex-shrink: 0;
  }
  .vec-input {
    flex: 1;
    min-width: 0;
  }
  .color-input {
    flex: 1;
    min-width: 0;
  }
  .bool-input {
    flex: none;
    width: auto;
    cursor: pointer;
  }
  .remove-field-btn {
    background: none;
    border: none;
    color: var(--ed-faint);
    cursor: pointer;
    font-size: 14px;
    padding: 0 4px;
  }
  .remove-field-btn:hover {
    color: var(--ed-err);
  }
  input,
  select {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-text);
    padding: 3px 6px;
    font-size: 12px;
    font-family: inherit;
    flex: 1;
    min-width: 0;
  }
  input:focus,
  select:focus {
    outline: none;
    border-color: var(--ed-accent);
  }
  .dialog-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 10px 14px;
    border-top: 1px solid var(--ed-line);
  }
  .btn {
    padding: 5px 16px;
    border-radius: 4px;
    font-size: 12px;
    cursor: pointer;
    font-family: inherit;
    border: 1px solid var(--ed-line-strong);
  }
  .btn.cancel {
    background: var(--ed-raised);
    color: var(--ed-text);
  }
  .btn.cancel:hover {
    background: var(--ed-line);
  }
  .btn.save {
    background: var(--ed-accent);
    color: var(--ed-on-accent);
    border-color: var(--ed-accent);
    font-weight: 600;
  }
  .btn.save:hover {
    background: var(--ed-a2);
  }
  .small-btn {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-ok);
    cursor: pointer;
    font-size: 11.5px;
    padding: 3px 8px;
    font-family: inherit;
  }
  .small-btn:hover {
    background: var(--ed-line);
  }
  .empty-hint {
    padding: 12px 10px;
    color: var(--ed-faint);
    font-size: 11px;
    font-style: italic;
  }
  .empty-hint.center {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
  }

  /* A number is a value: mono, as the design kit sets values. */
  input[type="number"] {
    font-family: var(--ed-mono);
  }
</style>
