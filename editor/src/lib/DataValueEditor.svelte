<script lang="ts">
    import Self from "./DataValueEditor.svelte";
    import type { DataFieldType, TableRowsEntry } from "./api/dataEditor";

    interface Props {
        type: DataFieldType;
        value: unknown;
        refOptions: Record<string, TableRowsEntry[]>;
        onchange: (v: unknown) => void;
    }

    let { type, value, refOptions, onchange }: Props = $props();

    // --- helpers -------------------------------------------------------------
    function refKey(v: unknown): string {
        if (v && typeof v === "object" && "table" in v && "id" in v) {
            const r = v as { table: string; id: string };
            return `${r.table}||${r.id}`;
        }
        return "";
    }
    function decodeRef(key: string): { table: string; id: string } | null {
        if (!key) return null;
        const [table, id] = key.split("||");
        return { table, id };
    }
    function refChoices(schemaName: string): { key: string; label: string }[] {
        const tables = refOptions[schemaName] ?? [];
        const out: { key: string; label: string }[] = [];
        for (const t of tables) {
            for (const id of t.row_ids) {
                out.push({
                    key: `${t.relative_path}||${id}`,
                    label: `${id}  (${t.relative_path})`,
                });
            }
        }
        return out;
    }
    function rgbaToHex(c: unknown): string {
        const arr = Array.isArray(c) ? c : [1, 1, 1, 1];
        const to = (v: number) =>
            Math.round(Math.max(0, Math.min(1, v ?? 0)) * 255)
                .toString(16)
                .padStart(2, "0");
        return `#${to(arr[0])}${to(arr[1])}${to(arr[2])}`;
    }
    function hexToRgb(hex: string): [number, number, number] {
        const n = parseInt(hex.slice(1), 16);
        return [
            ((n >> 16) & 255) / 255,
            ((n >> 8) & 255) / 255,
            (n & 255) / 255,
        ];
    }
    function asVec2(v: unknown): number[] {
        return Array.isArray(v) ? v : [0, 0];
    }
    function asRgba(v: unknown): number[] {
        return Array.isArray(v) ? v : [1, 1, 1, 1];
    }
    function asArray(v: unknown): unknown[] {
        return Array.isArray(v) ? v : [];
    }
    function asObject(v: unknown): Record<string, unknown> {
        return v && typeof v === "object" && !Array.isArray(v)
            ? (v as Record<string, unknown>)
            : {};
    }
    function defaultElement(ty: DataFieldType): unknown {
        switch (ty.kind) {
            case "string":
                return "";
            case "int":
            case "float":
                return 0;
            case "bool":
                return false;
            case "ref":
                return null;
            case "asset":
                return "";
            case "enum":
                return ty.options[0] ?? "";
            case "color":
                return [1, 1, 1, 1];
            case "vec2":
                return [0, 0];
            case "object": {
                const o: Record<string, unknown> = {};
                for (const f of ty.fields) o[f.name] = defaultElement(f.type);
                return o;
            }
            default:
                return null;
        }
    }
</script>

{#if type.kind === "string"}
    <input
        type="text"
        value={(value as string) ?? ""}
        oninput={(e) => onchange((e.target as HTMLInputElement).value)}
    />
{:else if type.kind === "int" || type.kind === "float"}
    <input
        type="number"
        step={type.kind === "int" ? "1" : "any"}
        value={(value as number) ?? 0}
        oninput={(e) => onchange(Number((e.target as HTMLInputElement).value))}
    />
{:else if type.kind === "bool"}
    <input
        type="checkbox"
        checked={Boolean(value)}
        onchange={(e) => onchange((e.target as HTMLInputElement).checked)}
    />
{:else if type.kind === "enum"}
    <select
        value={(value as string) ?? ""}
        onchange={(e) => onchange((e.target as HTMLSelectElement).value)}
    >
        {#each type.options as opt}
            <option value={opt}>{opt}</option>
        {/each}
    </select>
{:else if type.kind === "color"}
    <div class="row">
        <input
            type="color"
            value={rgbaToHex(value)}
            oninput={(e) => {
                const rgb = hexToRgb((e.target as HTMLInputElement).value);
                onchange([...rgb, asRgba(value)[3] ?? 1]);
            }}
        />
        <input
            class="alpha"
            type="number"
            min="0"
            max="1"
            step="0.05"
            value={asRgba(value)[3] ?? 1}
            oninput={(e) => {
                const c = asRgba(value);
                onchange([
                    c[0],
                    c[1],
                    c[2],
                    Number((e.target as HTMLInputElement).value),
                ]);
            }}
        />
    </div>
{:else if type.kind === "vec2"}
    <div class="row">
        <input
            type="number"
            step="any"
            value={asVec2(value)[0] ?? 0}
            oninput={(e) =>
                onchange([
                    Number((e.target as HTMLInputElement).value),
                    asVec2(value)[1] ?? 0,
                ])}
        />
        <input
            type="number"
            step="any"
            value={asVec2(value)[1] ?? 0}
            oninput={(e) =>
                onchange([
                    asVec2(value)[0] ?? 0,
                    Number((e.target as HTMLInputElement).value),
                ])}
        />
    </div>
{:else if type.kind === "asset"}
    <input
        type="text"
        placeholder={type.asset_kind}
        value={(value as string) ?? ""}
        oninput={(e) => onchange((e.target as HTMLInputElement).value)}
    />
{:else if type.kind === "ref"}
    <select
        value={refKey(value)}
        onchange={(e) =>
            onchange(decodeRef((e.target as HTMLSelectElement).value))}
    >
        <option value="">— none —</option>
        {#each refChoices(type.schema) as choice}
            <option value={choice.key}>{choice.label}</option>
        {/each}
    </select>
{:else if type.kind === "array"}
    <div class="array">
        {#each asArray(value) as el, ei}
            <div class="array-item">
                <Self
                    type={type.element}
                    value={el}
                    {refOptions}
                    onchange={(nv) => {
                        const arr = [...asArray(value)];
                        arr[ei] = nv;
                        onchange(arr);
                    }}
                />
                <button
                    class="tiny danger"
                    title="Remove item"
                    onclick={() => {
                        const arr = [...asArray(value)];
                        arr.splice(ei, 1);
                        onchange(arr);
                    }}>✕</button
                >
            </div>
        {/each}
        <button
            class="tiny add"
            onclick={() => {
                if (type.kind !== "array") return;
                onchange([...asArray(value), defaultElement(type.element)]);
            }}>+ item</button
        >
    </div>
{:else if type.kind === "object"}
    <div class="object">
        {#each type.fields as sub}
            <div class="object-field">
                <span class="sub-label">{sub.name}</span>
                <Self
                    type={sub.type}
                    value={asObject(value)[sub.name]}
                    {refOptions}
                    onchange={(nv) => {
                        const obj = { ...asObject(value) };
                        obj[sub.name] = nv;
                        onchange(obj);
                    }}
                />
            </div>
        {/each}
    </div>
{/if}

<style>
    .row {
        display: flex;
        gap: 4px;
        align-items: center;
    }
    .row input[type="number"] {
        width: 60px;
    }
    .alpha {
        width: 50px;
    }
    .array,
    .object {
        display: flex;
        flex-direction: column;
        gap: 3px;
        min-width: 160px;
    }
    .array-item {
        display: flex;
        gap: 3px;
        align-items: flex-start;
    }
    .object-field {
        display: flex;
        gap: 6px;
        align-items: flex-start;
    }
    .sub-label {
        color: var(--ed-muted);
        font-size: 11px;
        min-width: 60px;
    }
    input,
    select,
    button {
        background: var(--ed-raised);
        color: var(--ed-text);
        border: 1px solid var(--ed-line-strong);
        border-radius: 4px;
        padding: 3px 5px;
        font-size: 12px;
        box-sizing: border-box;
    }
    input[type="text"],
    select {
        width: 100%;
    }
    button {
        cursor: pointer;
    }
    button:hover {
        background: var(--ed-line);
    }
    button.tiny {
        padding: 1px 6px;
    }
    button.add {
        background: var(--ed-ok);
        color: var(--ed-on-accent);
        border-color: var(--ed-ok);
        align-self: flex-start;
    }
    button.danger {
        color: var(--ed-err);
    background: color-mix(in srgb, var(--ed-err) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--ed-err) 50%, transparent);
  }
</style>
