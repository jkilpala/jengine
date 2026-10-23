<script lang="ts">
    /**
     * Slice E.7 — Inspector pane for the selected behavior node.
     *
     * Edits literal pin values, reference-binding `× scale` /
     * `+ offset` modifiers, and node `params`. All mutations route
     * through `behaviorEditorStore` (single undo per commit).
     *
     * Inputs follow a "local state + commit on blur or Enter" pattern
     * so a typed value records exactly one undo entry, not one per
     * keystroke.
     */
    import {
        behaviorJson,
        opCatalog,
        selectedNodeIds,
        setPinLiteral,
        setPinModifier,
        setNodeParam,
        removeDataBinding,
    } from "./stores/behaviorEditorStore";
    import {
        parseBehaviorDoc,
        isReferenceBinding,
        type BehaviorNode,
        type BehaviorPinBinding,
        type BehaviorPinBindingReference,
    } from "./behavior/layout";
    import type {
        OpDescriptor,
        PinDescriptor,
        ParamDescriptor,
        DefaultValue,
    } from "./api/behavior";

    /** The single selected node (or null if 0 / >1 selected). */
    let selectedNode = $derived.by<BehaviorNode | null>(() => {
        const ids = $selectedNodeIds as Set<string>;
        if (ids.size !== 1) return null;
        const id = ids.values().next().value as string;
        const json = $behaviorJson;
        if (json == null) return null;
        const doc = parseBehaviorDoc(json);
        if (!doc) return null;
        return doc.nodes.find((n) => n.id === id) ?? null;
    });

    let descriptor = $derived.by<OpDescriptor | null>(() => {
        const node = selectedNode;
        if (!node) return null;
        const cat = $opCatalog ?? [];
        return cat.find((o) => o.op === node.op) ?? null;
    });

    function bindingFor(
        node: BehaviorNode,
        pinName: string,
    ): BehaviorPinBinding | null {
        return node.in?.[pinName] ?? null;
    }

    /** True when the binding is the literal-shape `{ value }`. */
    function isLiteral(b: BehaviorPinBinding | null): b is { value: unknown } {
        return b != null && !isReferenceBinding(b);
    }

    function formatRef(b: BehaviorPinBindingReference): string {
        return b.out != null ? `${b.from}.${b.out}` : b.from;
    }

    function defaultLabel(d: DefaultValue | null): string {
        if (d == null) return "—";
        if (typeof d === "number") return String(d);
        if (typeof d === "boolean") return d ? "true" : "false";
        if (typeof d === "string") return d;
        if (Array.isArray(d)) return `[${d[0]}, ${d[1]}]`;
        return String(d);
    }

    /** Coerce string → number; returns null when invalid/empty. */
    function tryNumber(s: string): number | null {
        const trimmed = s.trim();
        if (trimmed === "") return null;
        const n = Number(trimmed);
        return Number.isFinite(n) ? n : null;
    }
</script>

{#snippet floatEditor(
    value: number,
    onCommit: (v: number) => void,
    placeholder: string,
)}
    {@const initial = String(value)}
    <input
        type="number"
        class="num"
        value={initial}
        {placeholder}
        onblur={(e) => {
            const n = tryNumber((e.currentTarget as HTMLInputElement).value);
            if (n != null && n !== value) onCommit(n);
            else (e.currentTarget as HTMLInputElement).value = String(value);
        }}
        onkeydown={(e) => {
            if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
            else if (e.key === "Escape") {
                (e.currentTarget as HTMLInputElement).value = String(value);
                (e.currentTarget as HTMLInputElement).blur();
            }
        }}
    />
{/snippet}

{#snippet vec2Editor(
    value: [number, number],
    onCommit: (v: [number, number]) => void,
)}
    <span class="vec2">
        <input
            type="number"
            class="num"
            value={String(value[0])}
            onblur={(e) => {
                const n = tryNumber(
                    (e.currentTarget as HTMLInputElement).value,
                );
                if (n != null && n !== value[0]) onCommit([n, value[1]]);
                else
                    (e.currentTarget as HTMLInputElement).value = String(
                        value[0],
                    );
            }}
            onkeydown={(e) => {
                if (e.key === "Enter")
                    (e.currentTarget as HTMLInputElement).blur();
            }}
        />
        <input
            type="number"
            class="num"
            value={String(value[1])}
            onblur={(e) => {
                const n = tryNumber(
                    (e.currentTarget as HTMLInputElement).value,
                );
                if (n != null && n !== value[1]) onCommit([value[0], n]);
                else
                    (e.currentTarget as HTMLInputElement).value = String(
                        value[1],
                    );
            }}
            onkeydown={(e) => {
                if (e.key === "Enter")
                    (e.currentTarget as HTMLInputElement).blur();
            }}
        />
    </span>
{/snippet}

{#snippet boolEditor(value: boolean, onCommit: (v: boolean) => void)}
    <input
        type="checkbox"
        checked={value}
        onchange={(e) =>
            onCommit((e.currentTarget as HTMLInputElement).checked)}
    />
{/snippet}

{#snippet enumEditor(
    value: string,
    choices: string[],
    onCommit: (v: string) => void,
)}
    <select
        class="select"
        {value}
        onchange={(e) => onCommit((e.currentTarget as HTMLSelectElement).value)}
    >
        {#each choices as choice (choice)}
            <option value={choice}>{choice}</option>
        {/each}
    </select>
{/snippet}

{#snippet stringEditor(value: string, onCommit: (v: string) => void)}
    {@const initial = value}
    <input
        type="text"
        class="text"
        value={initial}
        onblur={(e) => {
            const v = (e.currentTarget as HTMLInputElement).value;
            if (v !== value) onCommit(v);
        }}
        onkeydown={(e) => {
            if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
            else if (e.key === "Escape") {
                (e.currentTarget as HTMLInputElement).value = value;
                (e.currentTarget as HTMLInputElement).blur();
            }
        }}
    />
{/snippet}

<!-- Param row: edits node.params[name]. Enum dropdown when applicable. -->
{#snippet paramRow(node: BehaviorNode, param: ParamDescriptor)}
    {@const current = node.params?.[param.name] ?? param.default}
    <div class="row" class:required={param.required && current == null}>
        <span class="row-name">
            {param.name}
            {#if param.required}<span class="req" title="required">*</span>{/if}
        </span>
        {#if param.ty === "Float"}
            {@render floatEditor(
                typeof current === "number" ? current : 0,
                (v) => setNodeParam(node.id, param.name, v),
                param.default == null ? "" : String(param.default),
            )}
        {:else if param.ty === "Vec2"}
            {@render vec2Editor(
                Array.isArray(current) ? (current as [number, number]) : [0, 0],
                (v) => setNodeParam(node.id, param.name, v),
            )}
        {:else if param.ty === "Bool"}
            {@render boolEditor(
                typeof current === "boolean" ? current : false,
                (v) => setNodeParam(node.id, param.name, v),
            )}
        {:else if param.ty === "Enum"}
            {@render enumEditor(
                typeof current === "string"
                    ? current
                    : (param.enum_variants[0] ?? ""),
                param.enum_variants,
                (v) => setNodeParam(node.id, param.name, v),
            )}
        {:else if param.ty === "String"}
            {@render stringEditor(
                typeof current === "string" ? current : "",
                (v) => setNodeParam(node.id, param.name, v),
            )}
        {:else}
            <span class="muted">unsupported: {param.ty}</span>
        {/if}
    </div>
{/snippet}

<!-- Pin row: literal editor when unbound or literal-bound; reference
     summary + scale/offset when reference-bound. -->
{#snippet pinRow(node: BehaviorNode, pin: PinDescriptor)}
    {@const binding = bindingFor(node, pin.name)}
    {@const isRef = binding != null && isReferenceBinding(binding)}
    {@const lit = isLiteral(binding) ? binding.value : pin.default}
    <div class="row pin-row">
        <div class="row-head">
            <span class="row-name">{pin.name}</span>
            <span class="row-ty">{pin.ty}</span>
            {#if !isRef && binding == null}
                <span
                    class="muted small"
                    title="default applied at compile time"
                    >default {defaultLabel(pin.default)}</span
                >
            {/if}
        </div>

        {#if isRef}
            {@const ref = binding as BehaviorPinBindingReference}
            <div class="ref-line">
                <span class="ref">← {formatRef(ref)}</span>
                <button
                    type="button"
                    class="mini-btn danger"
                    onclick={() => removeDataBinding(node.id, pin.name)}
                    title="Disconnect">⨯</button
                >
            </div>
            {#if pin.accepts_scale_offset}
                <div class="modifiers">
                    <label class="mod-row">
                        <span class="mod-name">× scale</span>
                        {@render floatEditor(
                            typeof ref.scale === "number" ? ref.scale : 1,
                            (v) =>
                                setPinModifier(
                                    node.id,
                                    pin.name,
                                    "scale",
                                    v === 1 ? null : v,
                                ),
                            "1",
                        )}
                    </label>
                    <label class="mod-row">
                        <span class="mod-name">+ offset</span>
                        {@render floatEditor(
                            typeof ref.offset === "number" ? ref.offset : 0,
                            (v) =>
                                setPinModifier(
                                    node.id,
                                    pin.name,
                                    "offset",
                                    v === 0 ? null : v,
                                ),
                            "0",
                        )}
                    </label>
                </div>
            {/if}
        {:else}
            <!-- Literal editor (covers both literal-bound and unbound cases). -->
            {#if pin.ty === "Float"}
                {@render floatEditor(
                    typeof lit === "number" ? lit : 0,
                    (v) => setPinLiteral(node.id, pin.name, v),
                    defaultLabel(pin.default),
                )}
            {:else if pin.ty === "Vec2"}
                {@render vec2Editor(
                    Array.isArray(lit) ? (lit as [number, number]) : [0, 0],
                    (v) => setPinLiteral(node.id, pin.name, v),
                )}
            {:else if pin.ty === "Bool"}
                {@render boolEditor(
                    typeof lit === "boolean" ? lit : false,
                    (v) => setPinLiteral(node.id, pin.name, v),
                )}
            {:else if pin.ty === "String"}
                {@render stringEditor(typeof lit === "string" ? lit : "", (v) =>
                    setPinLiteral(node.id, pin.name, v),
                )}
            {:else}
                <span class="muted">no inline editor for {pin.ty}</span>
            {/if}
            {#if binding != null && !isRef}
                <button
                    type="button"
                    class="mini-btn"
                    onclick={() => removeDataBinding(node.id, pin.name)}
                    title="Reset to default">reset</button
                >
            {/if}
        {/if}
    </div>
{/snippet}

{#if !selectedNode}
    <div class="inspector empty">
        <p class="hint">
            Select a single node to edit its inputs and parameters.
        </p>
    </div>
{:else}
    {@const node = selectedNode}
    {@const desc = descriptor}
    <div class="inspector">
        <header class="ins-head">
            <span class="ins-op">{node.op}</span>
            <span class="ins-id">{node.id}</span>
        </header>

        {#if !desc}
            <p class="warn">Unknown op — no schema available.</p>
        {:else}
            {#if desc.params.length > 0}
                <section class="ins-section">
                    <h3>Params</h3>
                    {#each desc.params as param (param.name)}
                        {@render paramRow(node, param)}
                    {/each}
                </section>
            {/if}

            {#if desc.inputs.length > 0}
                <section class="ins-section">
                    <h3>Inputs</h3>
                    {#each desc.inputs as pin (pin.name)}
                        {@render pinRow(node, pin)}
                    {/each}
                </section>
            {/if}

            {#if desc.outputs.length > 0}
                <section class="ins-section">
                    <h3>Outputs</h3>
                    {#each desc.outputs as out (out.name)}
                        <div class="row read-only">
                            <span class="row-name">{out.name}</span>
                            <span class="row-ty">{out.ty}</span>
                        </div>
                    {/each}
                </section>
            {/if}
        {/if}
    </div>
{/if}

<style>
    .inspector {
        display: flex;
        flex-direction: column;
        gap: 8px;
        padding: 8px;
        overflow: auto;
    }
    .inspector.empty {
        align-items: center;
        justify-content: center;
        padding: 16px;
    }
    .hint {
        color: var(--ctp-subtext0);
        font-style: italic;
        font-size: 11px;
        text-align: center;
        margin: 0;
    }
    .ins-head {
        display: flex;
        align-items: baseline;
        gap: 8px;
        padding-bottom: 6px;
        border-bottom: 1px solid var(--ctp-surface0);
    }
    .ins-op {
        font-weight: 700;
        color: var(--ctp-mauve);
    }
    .ins-id {
        font-family: var(--ed-mono);
        font-size: 10px;
        color: var(--ctp-subtext0);
    }
    .ins-section {
        display: flex;
        flex-direction: column;
        gap: 4px;
    }
    .ins-section h3 {
        margin: 0;
        font-size: 10px;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.5px;
        color: var(--ctp-subtext1);
    }
    .row {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 6px;
        border-radius: 4px;
        background: var(--ctp-surface0);
    }
    .row.read-only {
        flex-direction: row;
        align-items: baseline;
        justify-content: space-between;
        opacity: 0.85;
    }
    .row.required {
        box-shadow: inset 0 0 0 1px var(--ctp-yellow);
    }
    .row-head {
        display: flex;
        align-items: baseline;
        gap: 6px;
    }
    .row-name {
        font-weight: 600;
        font-size: 11px;
    }
    .row-ty {
        font-family: var(--ed-mono);
        font-size: 9px;
        color: var(--ctp-subtext0);
        padding: 1px 4px;
        border-radius: 3px;
        background: var(--ctp-surface1);
    }
    .req {
        color: var(--ctp-red);
        font-weight: 700;
    }
    .num {
        width: 100%;
        background: var(--ctp-base);
        color: var(--ctp-text);
        border: 1px solid var(--ctp-surface1);
        border-radius: 3px;
        padding: 2px 4px;
        font-size: 11px;
        font-family: var(--ed-mono);
    }
    .vec2 {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 4px;
    }
    .text {
        width: 100%;
        background: var(--ctp-base);
        color: var(--ctp-text);
        border: 1px solid var(--ctp-surface1);
        border-radius: 3px;
        padding: 2px 4px;
        font-size: 11px;
    }
    .select {
        width: 100%;
        background: var(--ctp-base);
        color: var(--ctp-text);
        border: 1px solid var(--ctp-surface1);
        border-radius: 4px;
        padding: 2px 4px;
        font-size: 11px;
    }
    .ref-line {
        display: flex;
        align-items: center;
        gap: 6px;
        justify-content: space-between;
    }
    .ref {
        font-family: var(--ed-mono);
        font-size: 11px;
        color: var(--ctp-teal);
    }
    .mini-btn {
        background: var(--ctp-surface1);
        border: none;
        color: var(--ctp-text);
        border-radius: 4px;
        padding: 1px 6px;
        font-size: 10px;
        cursor: pointer;
    }
    .mini-btn:hover {
        background: var(--ctp-surface2);
    }
    .mini-btn.danger:hover {
        background: var(--ctp-red);
        color: var(--ctp-base);
    }
    .modifiers {
        display: flex;
        flex-direction: column;
        gap: 3px;
        padding-top: 4px;
        border-top: 1px dashed var(--ctp-surface1);
    }
    .mod-row {
        display: grid;
        grid-template-columns: 70px 1fr;
        align-items: center;
        gap: 6px;
    }
    .mod-name {
        font-size: 10px;
        color: var(--ctp-subtext0);
    }
    .muted {
        color: var(--ctp-subtext0);
        font-size: 10px;
    }
    .small {
        font-size: 10px;
    }
    .warn {
        color: var(--ctp-yellow);
        font-size: 11px;
        font-style: italic;
    }
</style>
