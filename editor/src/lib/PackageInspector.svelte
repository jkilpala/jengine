<script lang="ts">
  import type { ComponentEditorInfo, FieldType } from "./api/packages";
  import CurveEditor from "./CurveEditor.svelte";
  import GradientEditor from "./GradientEditor.svelte";
  import { assetsWithExtensions, jsonText, parseJsonText } from "./fieldValues";
  import { projectAssets } from "./stores/projectAssetsStore";
  import { localeTables } from "./stores/localeStore";
  import { keys as localeKeys } from "./localeEdit";

  let {
    info,
    data,
    disabled = false,
    onchange,
    oneditpoints,
    overridden,
    mixed,
  }: {
    info: ComponentEditorInfo;
    data: Record<string, unknown>;
    disabled?: boolean;
    onchange: (field: string, value: unknown) => void;
    /** Edit a `Points` field by dragging in the scene view; no button when
     *  absent (the prefab inspector has no scene view). */
    oneditpoints?: (field: string) => void;
    /** Whether a field is changed from the prefab (or part) the entity is
     *  built from - marked beside its name. */
    overridden?: (field: string) => boolean;
    /** Whether a field differs among the selected entities: the value shown
     *  is the first one's. */
    mixed?: (field: string) => boolean;
  } = $props();

  // Colours are linear in the engine and a colour picker speaks sRGB, so
  // convert both ways - showing a linear value as sRGB draws every colour
  // darker in the picker than on screen.
  function toSrgb(c: number): number {
    const v = Math.max(0, Math.min(1, c));
    return v <= 0.0031308 ? v * 12.92 : 1.055 * Math.pow(v, 1 / 2.4) - 0.055;
  }
  function toLinear(c: number): number {
    return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  }
  function hexOf(col: number[]): string {
    return (
      "#" +
      [col[0], col[1], col[2]]
        .map((c) =>
          Math.round(toSrgb(c ?? 1) * 255)
            .toString(16)
            .padStart(2, "0"),
        )
        .join("")
    );
  }
  function rgbOf(hex: string): number[] {
    return [1, 3, 5].map((i) => toLinear(parseInt(hex.slice(i, i + 2), 16) / 255));
  }

  function fieldTypeKey(ft: FieldType): string {
    if (typeof ft === "string") return ft;
    if ("Float" in ft) return "Float";
    if ("Int" in ft) return "Int";
    if ("Enum" in ft) return "Enum";
    if ("AssetRef" in ft) return "AssetRef";
    return "String";
  }

  let jsonErrors = $state<Record<string, string>>({});

  function setJson(field: string, text: string) {
    const parsed = parseJsonText(text);
    if ("error" in parsed) {
      jsonErrors = { ...jsonErrors, [field]: parsed.error };
      return;
    }
    jsonErrors = { ...jsonErrors, [field]: "" };
    onchange(field, parsed.value);
  }

  /** A field the engine leaves out by default (`default_value` null, as an
   *  optional or skipped-when-empty field derives) and that is set now can
   *  be unset again - `undefined` drops it from the component. */
  function unsettable(field: { name: string; default_value: unknown }): boolean {
    return field.default_value == null && data[field.name] !== undefined && data[field.name] !== null;
  }
</script>

{#each info.fields as field}
  {@const ftk = fieldTypeKey(field.field_type)}
  <div class="field-row" class:unset={data[field.name] === undefined} title={field.description || ""}>
    <label class:overridden={overridden?.(field.name)} title={overridden?.(field.name) ? "Changed from the prefab" : undefined}>{field.display_name}{#if mixed?.(field.name)}<span class="mixed" title="Differs among the selected entities: the value shown is the first one's">mixed</span>{/if}</label>
    {#if ftk === "Json"}
      <div class="json">
        <textarea
          rows={jsonText(data[field.name]).includes("\n") ? 4 : 1}
          spellcheck="false"
          placeholder="unset - JSON"
          value={jsonText(data[field.name])}
          {disabled}
          onchange={(e) => setJson(field.name, (e.target as HTMLTextAreaElement).value)}
        ></textarea>
        {#if jsonErrors[field.name]}<span class="error">{jsonErrors[field.name]}</span>{/if}
      </div>
    {:else if ftk === "Float"}
      {@const ft = field.field_type as {
        Float: { min: number | null; max: number | null };
      }}
      {#if ft.Float.min != null && ft.Float.max != null}
        <input
          type="range"
          min={ft.Float.min}
          max={ft.Float.max}
          step="0.01"
          value={data[field.name] ?? 0}
          {disabled}
          oninput={(e) =>
            onchange(
              field.name,
              parseFloat((e.target as HTMLInputElement).value) || 0,
            )}
        />
        <span class="range-value"
          >{(data[field.name] as number)?.toFixed(2) ?? "0"}</span
        >
      {:else}
        <input
          type="number"
          step="0.1"
          value={data[field.name] ?? 0}
          {disabled}
          onchange={(e) =>
            onchange(
              field.name,
              parseFloat((e.target as HTMLInputElement).value) || 0,
            )}
        />
      {/if}
    {:else if ftk === "Int"}
      {@const ft = field.field_type as {
        Int: { min: number | null; max: number | null };
      }}
      {#if ft.Int.min != null && ft.Int.max != null}
        <input
          type="range"
          min={ft.Int.min}
          max={ft.Int.max}
          step="1"
          value={data[field.name] ?? 0}
          {disabled}
          oninput={(e) =>
            onchange(
              field.name,
              parseInt((e.target as HTMLInputElement).value) || 0,
            )}
        />
        <span class="range-value">{data[field.name] ?? 0}</span>
      {:else}
        <input
          type="number"
          step="1"
          value={data[field.name] ?? 0}
          {disabled}
          onchange={(e) =>
            onchange(
              field.name,
              parseInt((e.target as HTMLInputElement).value) || 0,
            )}
        />
      {/if}
    {:else if ftk === "String"}
      <input
        type="text"
        list={field.name === "text_key" ? "locale-keys" : undefined}
        value={data[field.name] ?? ""}
        {disabled}
        onchange={(e) =>
          onchange(field.name, (e.target as HTMLInputElement).value)}
      />
      {#if field.name === "text_key"}
        <datalist id="locale-keys">
          {#each localeKeys($localeTables) as k}<option value={k}></option>{/each}
        </datalist>
      {/if}
    {:else if ftk === "Bool"}
      <input
        type="checkbox"
        checked={!!data[field.name]}
        {disabled}
        onchange={(e) =>
          onchange(field.name, (e.target as HTMLInputElement).checked)}
      />
    {:else if ftk === "Vec2"}
      {@const vec = Array.isArray(data[field.name])
        ? (data[field.name] as number[])
        : [0, 0]}
      <input
        type="number"
        step="0.1"
        value={vec[0] ?? 0}
        {disabled}
        onchange={(e) =>
          onchange(field.name, [
            parseFloat((e.target as HTMLInputElement).value) || 0,
            vec[1] ?? 0,
          ])}
      />
      <input
        type="number"
        step="0.1"
        value={vec[1] ?? 0}
        {disabled}
        onchange={(e) =>
          onchange(field.name, [
            vec[0] ?? 0,
            parseFloat((e.target as HTMLInputElement).value) || 0,
          ])}
      />
    {:else if ftk === "Color"}
      {@const col = Array.isArray(data[field.name])
        ? (data[field.name] as number[])
        : [1, 1, 1, 1]}
      <input
        type="color"
        value={hexOf(col)}
        {disabled}
        onchange={(e) => {
          const [r, g, b] = rgbOf((e.target as HTMLInputElement).value);
          onchange(field.name, [r, g, b, col[3] ?? 1]);
        }}
      />
      <input
        type="number"
        step="0.05"
        min="0"
        max="1"
        value={col[3] ?? 1}
        {disabled}
        onchange={(e) =>
          onchange(field.name, [
            col[0] ?? 1,
            col[1] ?? 1,
            col[2] ?? 1,
            parseFloat((e.target as HTMLInputElement).value) || 0,
          ])}
      />
    {:else if ftk === "Points"}
      {@const pts = Array.isArray(data[field.name])
        ? (data[field.name] as number[][])
        : []}
      <div class="points">
        {#each pts as p, i}
          <div class="point-row">
            <span class="dim">{i + 1}</span>
            <input
              type="number"
              step="1"
              value={p[0] ?? 0}
              {disabled}
              onchange={(e) =>
                onchange(
                  field.name,
                  pts.map((q, j) =>
                    j === i ? [parseFloat((e.target as HTMLInputElement).value) || 0, q[1] ?? 0] : q,
                  ),
                )}
            />
            <input
              type="number"
              step="1"
              value={p[1] ?? 0}
              {disabled}
              onchange={(e) =>
                onchange(
                  field.name,
                  pts.map((q, j) =>
                    j === i ? [q[0] ?? 0, parseFloat((e.target as HTMLInputElement).value) || 0] : q,
                  ),
                )}
            />
            <button
              title="Remove this point"
              {disabled}
              onclick={() => onchange(field.name, pts.filter((_, j) => j !== i))}>×</button
            >
            <button
              title="Insert a point after this one, halfway to the next"
              {disabled}
              onclick={() => {
                const next = pts[i + 1] ?? [p[0] + 50, p[1]];
                const mid = [(p[0] + next[0]) / 2, (p[1] + next[1]) / 2];
                onchange(field.name, [...pts.slice(0, i + 1), mid, ...pts.slice(i + 1)]);
              }}>+</button
            >
          </div>
        {/each}
        <div class="point-row">
          {#if pts.length === 0}
            <button
              title="Add the first point"
              {disabled}
              onclick={() => onchange(field.name, [[0, 0]])}>+ point</button
            >
          {/if}
          {#if oneditpoints}
            <button
              title="Drag the points in the scene view: click an edge to add one, Delete removes the one under the cursor"
              {disabled}
              onclick={() => oneditpoints(field.name)}>Edit in viewport</button
            >
          {/if}
        </div>
      </div>
    {:else if ftk === "Curve"}
      <CurveEditor
        keys={data[field.name] as number[][] | undefined}
        {disabled}
        onchange={(keys) => onchange(field.name, keys)}
      />
    {:else if ftk === "Gradient"}
      <GradientEditor
        keys={data[field.name] as number[][] | undefined}
        {disabled}
        onchange={(keys) => onchange(field.name, keys)}
      />
    {:else if ftk === "Color3"}
      {@const col = Array.isArray(data[field.name])
        ? (data[field.name] as number[])
        : [1, 1, 1]}
      <input
        type="color"
        value={hexOf(col)}
        {disabled}
        onchange={(e) =>
          onchange(field.name, rgbOf((e.target as HTMLInputElement).value))}
      />
    {:else if ftk === "Enum"}
      {@const ft = field.field_type as { Enum: { options: string[] } }}
      <select
        value={data[field.name] ?? ft.Enum.options[0] ?? ""}
        {disabled}
        onchange={(e) =>
          onchange(field.name, (e.target as HTMLSelectElement).value)}
      >
        {#each ft.Enum.options as opt}
          <option value={opt}>{opt}</option>
        {/each}
      </select>
    {:else if ftk === "AssetRef"}
      {@const ft = field.field_type as { AssetRef: { extensions: string[] } }}
      {@const listId = `assets-${info.component_name}-${field.name}`}
      <input
        type="text"
        list={listId}
        value={data[field.name] ?? ""}
        {disabled}
        onchange={(e) =>
          onchange(field.name, (e.target as HTMLInputElement).value)}
        placeholder="Asset path…"
      />
      <datalist id={listId}>
        {#each assetsWithExtensions($projectAssets, ft.AssetRef.extensions) as file}
          <option value={file}></option>
        {/each}
      </datalist>
    {/if}
    {#if unsettable(field)}
      <button
        class="unset-btn"
        title="Unset: leave this field out, as the engine's default does"
        {disabled}
        onclick={() => onchange(field.name, undefined)}>⟲</button
      >
    {/if}
  </div>
{/each}

<style>
  .json {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .json textarea {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    padding: 2px 4px;
    border-radius: 4px;
    font-family: var(--ed-mono);
    font-size: 11px;
    resize: vertical;
  }
  .error {
    color: var(--ed-err);
    font-size: 10px;
  }
  .field-row.unset > label {
    color: var(--ed-faint);
  }
  .unset-btn {
    background: none;
    border: none;
    color: var(--ed-muted);
    cursor: pointer;
    font-size: 12px;
    padding: 0 2px;
  }
  .points {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .point-row {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .point-row input {
    width: 64px;
  }
  .field-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
  }
  .field-row label {
    min-width: 90px;
    font-size: 11px;
    color: var(--ed-text);
    flex-shrink: 0;
  }
  .field-row input[type="number"],
  .field-row input[type="text"] {
    flex: 1;
    min-width: 0;
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    padding: 2px 4px;
    border-radius: 4px;
    font-size: 12px;
  }
  .field-row input[type="checkbox"] {
    accent-color: var(--ed-accent);
  }
  .field-row input[type="color"] {
    width: 28px;
    height: 22px;
    padding: 0;
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    cursor: pointer;
  }
  .field-row input[type="range"] {
    flex: 1;
    min-width: 0;
    accent-color: var(--ed-accent);
  }
  .field-row select {
    flex: 1;
    min-width: 0;
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    padding: 2px 4px;
    border-radius: 4px;
    font-size: 12px;
  }
  .range-value {
    font-size: 11px;
    color: var(--ed-muted);
    min-width: 36px;
    text-align: right;
  }
  label.overridden::before {
    content: "●";
    color: var(--ed-accent);
    font-size: 8px;
    margin-right: 3px;
    vertical-align: middle;
  }
  .mixed {
    margin-left: 4px;
    font-size: 9px;
    font-style: italic;
    color: var(--ed-warn);
  }
</style>
