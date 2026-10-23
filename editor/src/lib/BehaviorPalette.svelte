<script lang="ts">
    import { onMount } from "svelte";
    import {
        ensureOpCatalog,
        opCatalog,
        requestCenterDrop,
    } from "./stores/behaviorEditorStore";
    import { categoryColor, PALETTE_DND_MIME } from "./behavior/layout";
    import type { EffectKind, OpDescriptor } from "./api/behavior";

    let search = $state("");

    onMount(() => {
        void ensureOpCatalog();
    });

    /** Display order. Mirrors the spec's category list. */
    const CATEGORY_ORDER: EffectKind[] = [
        "Trigger",
        "Read",
        "Pure",
        "Gate",
        "DirectWrite",
        "SetupComponent",
        "StructuralCommand",
        "SideEffect",
    ];

    let filtered: OpDescriptor[] = $derived.by(() => {
        const q = search.trim().toLowerCase();
        const all = $opCatalog ?? [];
        if (!q) return all;
        return all.filter(
            (op) =>
                op.op.toLowerCase().includes(q) ||
                op.summary.toLowerCase().includes(q),
        );
    });

    let grouped: Map<EffectKind, OpDescriptor[]> = $derived.by(() => {
        const map = new Map<EffectKind, OpDescriptor[]>();
        for (const cat of CATEGORY_ORDER) map.set(cat, []);
        for (const op of filtered) {
            const list = map.get(op.category) ?? [];
            list.push(op);
            map.set(op.category, list);
        }
        // Sort each category by op name for deterministic display.
        for (const list of map.values())
            list.sort((a, b) => a.op.localeCompare(b.op));
        return map;
    });

    function handleDragStart(e: DragEvent, op: OpDescriptor) {
        if (!e.dataTransfer) return;
        e.dataTransfer.setData(PALETTE_DND_MIME, op.op);
        e.dataTransfer.setData("text/plain", op.op);
        e.dataTransfer.effectAllowed = "copy";
    }

    function handleDoubleClick(op: OpDescriptor) {
        requestCenterDrop.set(op.op);
    }
</script>

<aside class="palette" aria-label="Op palette">
    <input
        class="search"
        type="search"
        placeholder="Search ops…"
        bind:value={search}
        aria-label="Filter ops"
    />
    <div class="groups">
        {#each [...grouped] as [category, ops] (category)}
            {#if ops.length > 0}
                <section class="group">
                    <header class="group-header">
                        <span
                            class="swatch"
                            style="background:{categoryColor(category)}"
                            aria-hidden="true"
                        ></span>
                        <span class="group-name">{category}</span>
                        <span class="group-count">{ops.length}</span>
                    </header>
                    <ul class="op-list">
                        {#each ops as op (op.op)}
                            <li>
                                <button
                                    type="button"
                                    class="op-row"
                                    draggable="true"
                                    ondragstart={(e) => handleDragStart(e, op)}
                                    ondblclick={() => handleDoubleClick(op)}
                                    title={op.summary}
                                >
                                    <span class="op-name">{op.op}</span>
                                    {#if op.summary}
                                        <span class="op-summary"
                                            >{op.summary}</span
                                        >
                                    {/if}
                                </button>
                            </li>
                        {/each}
                    </ul>
                </section>
            {/if}
        {/each}
        {#if ($opCatalog?.length ?? 0) === 0}
            <p class="empty">Loading catalog…</p>
        {:else if filtered.length === 0}
            <p class="empty">No matches for “{search}”.</p>
        {/if}
    </div>
</aside>

<style>
    .palette {
        background: var(--ctp-mantle);
        border-right: 1px solid var(--ctp-surface0);
        display: flex;
        flex-direction: column;
        overflow: hidden;
        min-width: 0;
    }
    .search {
        margin: 8px;
        padding: 4px 8px;
        background: var(--ctp-base);
        border: 1px solid var(--ctp-surface1);
        border-radius: 4px;
        color: var(--ctp-text);
        font-size: 12px;
        flex-shrink: 0;
    }
    .search:focus {
        outline: 1px solid var(--ctp-blue);
        outline-offset: -1px;
    }
    .groups {
        flex: 1;
        overflow-y: auto;
        padding: 0 4px 8px 4px;
    }
    .group {
        margin-bottom: 4px;
    }
    .group-header {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 4px 6px;
        font-size: 10px;
        text-transform: uppercase;
        letter-spacing: 0.5px;
        color: var(--ctp-subtext0);
    }
    .swatch {
        width: 8px;
        height: 8px;
        border-radius: 2px;
        flex-shrink: 0;
    }
    .group-name {
        font-weight: 600;
    }
    .group-count {
        margin-left: auto;
        color: var(--ctp-overlay1);
    }
    .op-list {
        list-style: none;
        margin: 0;
        padding: 0;
    }
    .op-row {
        width: 100%;
        text-align: left;
        background: transparent;
        border: 1px solid transparent;
        border-radius: 4px;
        padding: 4px 8px;
        color: var(--ctp-text);
        cursor: grab;
        display: flex;
        flex-direction: column;
        gap: 2px;
        font-size: 12px;
        line-height: 1.3;
    }
    .op-row:hover {
        background: var(--ctp-surface0);
        border-color: var(--ctp-surface1);
    }
    .op-row:active {
        cursor: grabbing;
    }
    .op-row:focus-visible {
        outline: 1px solid var(--ctp-blue);
        outline-offset: -1px;
    }
    .op-name {
        font-weight: 600;
    }
    .op-summary {
        font-size: 10px;
        color: var(--ctp-subtext0);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .empty {
        color: var(--ctp-subtext0);
        font-style: italic;
        padding: 6px 8px;
        margin: 0;
        font-size: 11px;
    }
</style>
