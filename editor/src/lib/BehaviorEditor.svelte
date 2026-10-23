<script lang="ts">
    import { claimUndo } from "./stores/undoTargetStore";
    import { requestEditorClose } from "./stores/editorClose";
    import { onMount } from "svelte";
    import {
        behaviorJson,
        behaviorFilePath,
        behaviorReport,
        behaviorDirty,
        closeBehaviorEditor,
        compileStatus,
        copySelectedNodes,
        cutSelectedNodes,
        deleteSelectedNodes,
        pasteClipboard,
        redoBehavior,
        redoDepth,
        requestFocusNode,
        saveBehavior,
        selectedNodeIds,
        undoBehavior,
        undoDepth,
    } from "./stores/behaviorEditorStore";
    import BehaviorCanvas from "./BehaviorCanvas.svelte";
    import BehaviorPalette from "./BehaviorPalette.svelte";
    import BehaviorInspector from "./BehaviorInspector.svelte";

    function fileName(p: string | null): string {
        if (!p) return "";
        return p.split(/[\\/]/).pop() ?? p;
    }

    function isTypingTarget(target: EventTarget | null): boolean {
        const el = target as HTMLElement | null;
        if (!el) return false;
        const tag = el.tagName;
        return (
            tag === "INPUT" ||
            tag === "TEXTAREA" ||
            el.isContentEditable === true
        );
    }

    function handleKeydown(e: KeyboardEvent) {
        if ($behaviorJson == null) return;
        if (isTypingTarget(e.target)) return;
        const ctrl = e.ctrlKey || e.metaKey;

        if (ctrl && e.key.toLowerCase() === "s") {
            e.preventDefault();
            void saveBehavior();
            return;
        }
        if (e.key === "Delete" || e.key === "Backspace") {
            if (($selectedNodeIds as Set<string>).size > 0) {
                e.preventDefault();
                deleteSelectedNodes();
            }
            return;
        }
        // Slice E.10d: copy / cut / paste. Selection-scoped — paste
        // works even with no selection (uses last clipboard payload).
        if (ctrl && e.key.toLowerCase() === "c") {
            if (($selectedNodeIds as Set<string>).size > 0) {
                e.preventDefault();
                copySelectedNodes();
            }
            return;
        }
        if (ctrl && e.key.toLowerCase() === "x") {
            if (($selectedNodeIds as Set<string>).size > 0) {
                e.preventDefault();
                cutSelectedNodes();
            }
            return;
        }
        if (ctrl && e.key.toLowerCase() === "v") {
            e.preventDefault();
            pasteClipboard();
        }
    }

    onMount(() => {
        window.addEventListener("keydown", handleKeydown);
        return () => window.removeEventListener("keydown", handleKeydown);
    });

    // Ctrl+Z / Ctrl+Y are this editor's while it is open; the app's key
    // handler asks `undoTarget()` before undoing the scene, so one press
    // no longer undoes in both.
    $effect(() => claimUndo({ undo: undoBehavior, redo: redoBehavior, ignoreTyping: true }));
</script>

<div class="behavior-editor">
    <header class="be-header">
        <span class="be-title"
            >🌳 {fileName($behaviorFilePath)}{$behaviorDirty ? " •" : ""}</span
        >
        {#if $behaviorReport}
            {#if $compileStatus === "validating"}
                <span class="badge validating">validating…</span>
            {:else if $behaviorReport.ok && $behaviorReport.warnings.length === 0}
                <span class="badge ok">compiles</span>
            {:else if $behaviorReport.ok}
                <span class="badge warn"
                    >{$behaviorReport.warnings.length} warning{$behaviorReport
                        .warnings.length === 1
                        ? ""
                        : "s"}</span
                >
            {:else}
                <span class="badge err"
                    >{$behaviorReport.errors.length} error{$behaviorReport
                        .errors.length === 1
                        ? ""
                        : "s"}</span
                >
            {/if}
        {/if}
        <span class="spacer"></span>
        <button
            class="tool-btn"
            onclick={undoBehavior}
            disabled={$undoDepth === 0}
            title="Undo (Ctrl+Z)">↶</button
        >
        <button
            class="tool-btn"
            onclick={redoBehavior}
            disabled={$redoDepth === 0}
            title="Redo (Ctrl+Shift+Z)">↷</button
        >
        <button
            class="tool-btn"
            onclick={() => void saveBehavior()}
            disabled={!$behaviorDirty}
            title="Save (Ctrl+S)">💾</button
        >
        <button class="close-btn" onclick={() => requestEditorClose("behavior")} title="Close"
            >✕</button
        >
    </header>

    <div class="be-body">
        <BehaviorPalette />
        {#if $behaviorJson != null}
            <BehaviorCanvas json={$behaviorJson} />
        {:else}
            <div class="be-empty">No behavior loaded</div>
        {/if}

        <aside class="be-right" aria-label="Inspector and diagnostics">
            <div class="be-inspector">
                <BehaviorInspector />
            </div>
            <div class="be-diagnostics">
                {#if $behaviorReport == null}
                    <p class="empty">Loading…</p>
                {:else if $behaviorReport.ok && $behaviorReport.warnings.length === 0}
                    <p class="empty ok">
                        No diagnostics. Graph compiles cleanly.
                    </p>
                {:else}
                    {#each $behaviorReport.errors as diag (diag.code + (diag.node_id ?? ""))}
                        <button
                            type="button"
                            class="diag err"
                            class:clickable={!!diag.node_id}
                            disabled={!diag.node_id}
                            onclick={() =>
                                diag.node_id && requestFocusNode(diag.node_id)}
                        >
                            <div class="diag-head">
                                <span class="diag-sev">error</span>
                                <span class="diag-code">{diag.code}</span>
                                {#if diag.node_id}
                                    <span class="diag-loc">{diag.node_id}</span>
                                {/if}
                                {#if diag.pin}
                                    <span class="diag-loc">·{diag.pin}</span>
                                {/if}
                            </div>
                            <div class="diag-msg">{diag.message}</div>
                        </button>
                    {/each}
                    {#each $behaviorReport.warnings as diag (diag.code + (diag.node_id ?? ""))}
                        <button
                            type="button"
                            class="diag warn"
                            class:clickable={!!diag.node_id}
                            disabled={!diag.node_id}
                            onclick={() =>
                                diag.node_id && requestFocusNode(diag.node_id)}
                        >
                            <div class="diag-head">
                                <span class="diag-sev">warn</span>
                                <span class="diag-code">{diag.code}</span>
                                {#if diag.node_id}
                                    <span class="diag-loc">{diag.node_id}</span>
                                {/if}
                            </div>
                            <div class="diag-msg">{diag.message}</div>
                        </button>
                    {/each}
                {/if}
            </div>
        </aside>
    </div>
</div>

<style>
    .behavior-editor {
        display: flex;
        flex-direction: column;
        height: 100%;
        background: var(--ctp-base);
        color: var(--ctp-text);
        font-size: 12px;
    }
    .be-header {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 6px 10px;
        background: var(--ctp-mantle);
        border-bottom: 1px solid var(--ctp-surface0);
        flex-shrink: 0;
    }
    .be-title {
        font-weight: 600;
    }
    .spacer {
        flex: 1;
    }
    .badge {
        font-size: 10px;
        padding: 2px 8px;
        border-radius: 10px;
        text-transform: uppercase;
        letter-spacing: 0.5px;
        font-weight: 600;
    }
    .badge.ok {
        background: var(--ctp-green);
        color: var(--ctp-base);
    }
    .badge.warn {
        background: var(--ctp-yellow);
        color: var(--ctp-base);
    }
    .badge.err {
        background: var(--ctp-red);
        color: var(--ctp-base);
    }
    .badge.validating {
        background: var(--ctp-surface1);
        color: var(--ctp-subtext1);
    }
    .close-btn {
        background: none;
        border: none;
        color: var(--ctp-subtext0);
        cursor: pointer;
        font-size: 14px;
        padding: 0 6px;
    }
    .close-btn:hover {
        color: var(--ctp-text);
    }
    .tool-btn {
        background: var(--ctp-surface0);
        border: 1px solid var(--ctp-surface1);
        border-radius: 4px;
        color: var(--ctp-text);
        cursor: pointer;
        font-size: 12px;
        padding: 2px 8px;
        line-height: 1.2;
    }
    .tool-btn:hover:not(:disabled) {
        background: var(--ctp-surface1);
    }
    .tool-btn:disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }
    .be-body {
        flex: 1;
        display: grid;
        grid-template-columns: 220px 1fr 320px;
        min-height: 0;
    }
    .be-empty {
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--ctp-subtext0);
        font-style: italic;
    }
    .be-right {
        background: var(--ctp-mantle);
        border-left: 1px solid var(--ctp-surface0);
        display: grid;
        grid-template-rows: minmax(140px, 1fr) minmax(120px, 1fr);
        min-height: 0;
    }
    .be-inspector {
        overflow: auto;
        border-bottom: 1px solid var(--ctp-surface0);
        min-height: 0;
    }
    .be-diagnostics {
        overflow: auto;
        padding: 8px;
        display: flex;
        flex-direction: column;
        gap: 6px;
        min-height: 0;
    }
    .empty {
        color: var(--ctp-subtext0);
        font-style: italic;
        margin: 8px 0;
    }
    .empty.ok {
        color: var(--ctp-green);
        font-style: normal;
    }
    .diag {
        border-radius: 4px;
        padding: 6px 8px;
        border-left: 3px solid var(--ctp-surface1);
        background: var(--ctp-surface0);
        width: 100%;
        text-align: left;
        color: inherit;
        font: inherit;
        cursor: default;
    }
    .diag.clickable {
        cursor: pointer;
    }
    .diag.clickable:hover {
        background: var(--ctp-surface1);
    }
    .diag.err {
        border-left-color: var(--ctp-red);
    }
    .diag.warn {
        border-left-color: var(--ctp-yellow);
    }
    .diag-head {
        display: flex;
        gap: 6px;
        align-items: baseline;
        font-size: 10px;
        margin-bottom: 4px;
    }
    .diag-sev {
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.5px;
    }
    .diag.err .diag-sev {
        color: var(--ctp-red);
    }
    .diag.warn .diag-sev {
        color: var(--ctp-yellow);
    }
    .diag-code {
        color: var(--ctp-subtext0);
        font-family: var(--ed-mono);
    }
    .diag-loc {
        color: var(--ctp-mauve);
        font-family: var(--ed-mono);
    }
    .diag-msg {
        font-size: 12px;
        color: var(--ctp-text);
        white-space: pre-wrap;
        word-break: break-word;
    }
</style>
