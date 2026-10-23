<script lang="ts">
    import Self from "./SchemaFieldTypeEditor.svelte";
    import {
        FIELD_TYPE_KINDS,
        fieldTypeLabel,
        defaultFieldType,
        type FieldTypeKind,
        type DataFieldType,
        type DataFieldDef,
        type SchemaEntry,
    } from "./api/dataEditor";

    interface Props {
        type: DataFieldType;
        availableSchemas: SchemaEntry[];
        /** Exclude these kinds from the dropdown (e.g. nested arrays). */
        exclude?: FieldTypeKind[];
        onchange: (t: DataFieldType) => void;
    }

    let { type, availableSchemas, exclude = [], onchange }: Props = $props();

    const ASSET_KINDS = [
        "any",
        "texture",
        "atlas",
        "prefab",
        "scene",
        "audio",
    ] as const;

    const kinds = $derived(
        FIELD_TYPE_KINDS.filter((k) => !exclude.includes(k)),
    );

    function edit(mutate: (t: DataFieldType) => void) {
        const clone = structuredClone(type);
        mutate(clone);
        onchange(clone);
    }

    function changeKind(kind: FieldTypeKind) {
        onchange(defaultFieldType(kind));
    }

    function uniqueSubName(existing: string[]): string {
        let n = 1;
        let name = "field";
        while (existing.includes(name)) name = `field_${n++}`;
        return name;
    }
</script>

<div class="type-editor">
    <select
        class="kind"
        value={type.kind}
        onchange={(e) =>
            changeKind((e.target as HTMLSelectElement).value as FieldTypeKind)}
    >
        {#each kinds as k}
            <option value={k}>{fieldTypeLabel(k)}</option>
        {/each}
    </select>

    {#if type.kind === "enum"}
        <input
            class="cfg"
            type="text"
            placeholder="option1, option2"
            value={type.options.join(", ")}
            oninput={(e) =>
                edit((t) => {
                    if (t.kind === "enum")
                        t.options = (e.target as HTMLInputElement).value
                            .split(",")
                            .map((o) => o.trim())
                            .filter((o) => o.length > 0);
                })}
        />
    {:else if type.kind === "ref"}
        <select
            class="cfg"
            value={type.schema}
            onchange={(e) =>
                edit((t) => {
                    if (t.kind === "ref")
                        t.schema = (e.target as HTMLSelectElement).value;
                })}
        >
            <option value="">— schema —</option>
            {#each availableSchemas as s}
                <option value={s.name}>{s.name}</option>
            {/each}
        </select>
    {:else if type.kind === "asset"}
        <select
            class="cfg"
            value={type.asset_kind}
            onchange={(e) =>
                edit((t) => {
                    if (t.kind === "asset")
                        t.asset_kind = (e.target as HTMLSelectElement)
                            .value as any;
                })}
        >
            {#each ASSET_KINDS as k}
                <option value={k}>{k}</option>
            {/each}
        </select>
    {:else if type.kind === "array"}
        <div class="nested">
            <span class="nested-label">of</span>
            <Self
                type={type.element}
                {availableSchemas}
                exclude={["array"]}
                onchange={(el) =>
                    edit((t) => {
                        if (t.kind === "array") t.element = el;
                    })}
            />
        </div>
    {:else if type.kind === "object"}
        <div class="object-fields">
            {#each type.fields as sub, si}
                <div class="sub-field">
                    <input
                        class="sub-name"
                        type="text"
                        value={sub.name}
                        oninput={(e) =>
                            edit((t) => {
                                if (t.kind === "object")
                                    t.fields[si].name = (
                                        e.target as HTMLInputElement
                                    ).value;
                            })}
                    />
                    <Self
                        type={sub.type}
                        {availableSchemas}
                        onchange={(st) =>
                            edit((t) => {
                                if (t.kind === "object") t.fields[si].type = st;
                            })}
                    />
                    <button
                        class="tiny danger"
                        title="Remove sub-field"
                        onclick={() =>
                            edit((t) => {
                                if (t.kind === "object") t.fields.splice(si, 1);
                            })}>✕</button
                    >
                </div>
            {/each}
            <button
                class="tiny add"
                onclick={() =>
                    edit((t) => {
                        if (t.kind === "object") {
                            const name = uniqueSubName(
                                t.fields.map((f: DataFieldDef) => f.name),
                            );
                            t.fields.push({
                                name,
                                type: { kind: "string" },
                                default: "",
                            });
                        }
                    })}>+ sub-field</button
            >
        </div>
    {/if}
</div>

<style>
    .type-editor {
        display: flex;
        align-items: flex-start;
        gap: 6px;
        flex-wrap: wrap;
    }
    .kind {
        width: 110px;
    }
    .cfg {
        flex: 1;
        min-width: 120px;
    }
    .nested {
        display: flex;
        align-items: center;
        gap: 6px;
    }
    .nested-label {
        color: var(--ed-faint);
        font-size: 11px;
    }
    .object-fields {
        display: flex;
        flex-direction: column;
        gap: 4px;
        width: 100%;
        border-left: 2px solid var(--ed-line-strong);
        padding-left: 8px;
        margin-top: 4px;
    }
    .sub-field {
        display: flex;
        align-items: flex-start;
        gap: 6px;
    }
    .sub-name {
        width: 120px;
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
