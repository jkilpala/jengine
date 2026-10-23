<script lang="ts">
    import {
        undoStack,
        redoStack,
        undo,
        redo,
        type Command,
    } from "./stores/commandStore";
    import { get } from "svelte/store";

    function jumpTo(index: number) {
        const stack = get(undoStack);
        const diff = stack.length - 1 - index;
        if (diff > 0) {
            for (let i = 0; i < diff; i++) undo();
        }
    }

    function redoTo(index: number) {
        const stack = get(redoStack);
        const count = stack.length - index;
        for (let i = 0; i < count; i++) redo();
    }
</script>

<div class="undo-history">
    <div class="header">History</div>
    <div class="history-list">
        {#each $undoStack as cmd, i}
            <button
                class="history-item"
                class:current={i === $undoStack.length - 1}
                onclick={() => jumpTo(i)}
                title={cmd.label}
            >
                <span class="history-index">{i + 1}</span>
                <span class="history-label">{cmd.label}</span>
            </button>
        {/each}

        {#if $undoStack.length === 0 && $redoStack.length === 0}
            <div class="empty">No history</div>
        {/if}

        {#if $redoStack.length > 0}
            <div class="redo-divider">
                <span>— redo —</span>
            </div>
            {#each [...$redoStack].reverse() as cmd, i}
                <button
                    class="history-item redo"
                    onclick={() => redoTo(i)}
                    title={cmd.label}
                >
                    <span class="history-index"
                        >{$undoStack.length + i + 1}</span
                    >
                    <span class="history-label">{cmd.label}</span>
                </button>
            {/each}
        {/if}
    </div>
</div>

<style>
    .undo-history {
        display: flex;
        flex-direction: column;
        height: 100%;
        background: var(--ed-base);
    }

    .header {
        padding: 6px 10px;
        font-size: 11px;
        font-weight: 600;
        color: var(--ed-text);
        border-bottom: 1px solid var(--ed-line);
        text-transform: uppercase;
        letter-spacing: 0.05em;
    }

    .history-list {
        flex: 1;
        overflow-y: auto;
        padding: 4px 0;
    }

    .history-item {
        display: flex;
        align-items: center;
        gap: 8px;
        width: 100%;
        padding: 4px 10px;
        border: none;
        background: none;
        color: var(--ed-muted);
        font-size: 12px;
        text-align: left;
        cursor: pointer;
    }

    .history-item:hover {
        background: var(--ed-raised);
        color: var(--ed-text);
    }

    .history-item.current {
        background: var(--ed-raised);
        color: var(--ed-accent);
        font-weight: 600;
    }

    .history-item.redo {
        opacity: 0.5;
    }

    .history-item.redo:hover {
        opacity: 0.8;
    }

    .history-index {
        min-width: 20px;
        font-size: 10px;
        color: var(--ed-faint);
        text-align: right;
    }

    .history-label {
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .empty {
        padding: 12px;
        color: var(--ed-faint);
        font-size: 12px;
        text-align: center;
    }

    .redo-divider {
        padding: 4px 10px;
        text-align: center;
    }

    .redo-divider span {
        font-size: 10px;
        color: var(--ed-faint);
    }
</style>
