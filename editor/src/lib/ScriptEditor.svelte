<script lang="ts">
    import { onMount, onDestroy } from "svelte";
    import { get } from "svelte/store";
    import {
        scriptContent,
        scriptFilePath,
        scriptLanguage,
        scriptDirty,
        saveCurrentScript,
        updateScriptContent,
        requestedScriptLine,
    } from "./stores/scriptEditorStore";
    import { openInCodeEditor } from "./api/project";
    import { project } from "./stores/projectStore";

    import { EditorView, basicSetup } from "codemirror";
    import { EditorState, Compartment } from "@codemirror/state";
    import { javascript } from "@codemirror/lang-javascript";
    import { StreamLanguage } from "@codemirror/language";
    import { lua } from "@codemirror/legacy-modes/mode/lua";
    import { codeTheme } from "./codeTheme";
    import { themeMode } from "./stores/themeStore";
    import { keymap } from "@codemirror/view";
    import { indentWithTab } from "@codemirror/commands";
    import { autocompletion } from "@codemirror/autocomplete";
    import {
        jengineCompletionSource,
        jengineJsCompletionSource,
    } from "./scriptCompletions";
    import { debugGutter, setBreakpoints, setExecutionLine } from "./debugGutter";
    import { luaDebugger, scriptKey } from "./luaDebugger";
    import { debuggerState } from "./stores/debuggerStore";

    // ---------------------------------------------------------------------------
    // State
    // ---------------------------------------------------------------------------
    let editorContainer: HTMLDivElement | undefined = $state();
    let editorView: EditorView | undefined = $state();
    let cursorLine = $state(1);
    let cursorCol = $state(1);

    /** The code theme, swapped when the editor goes light or dark. */
    const themeSlot = new Compartment();

    /** Suppress store → editor feedback when the editor itself dispatches changes. */
    let suppressStoreSync = false;

    const fileName = $derived(
        $scriptFilePath?.split(/[\\/]/).pop() ?? "Script",
    );

    // ---------------------------------------------------------------------------
    // CodeMirror setup
    // ---------------------------------------------------------------------------

    function buildExtensions(lang: "lua" | "javascript") {
        const langExt =
            lang === "lua" ? StreamLanguage.define(lua) : javascript();

        return [
            // The debugger's breakpoints (click the margin, or F9) and the
            // line Play is stopped on.
            debugGutter((line) => {
                const path = get(scriptFilePath);
                if (path) luaDebugger.toggleBreakpoint(path, line);
            }),
            basicSetup,
            langExt,
            themeSlot.of(codeTheme(get(themeMode) === "dark")),
            autocompletion({
                // Lua calls namespaces; JavaScript keeps the flat globals.
                override: [
                    lang === "lua"
                        ? jengineCompletionSource
                        : jengineJsCompletionSource,
                ],
                defaultKeymap: true,
            }),
            EditorView.updateListener.of((update) => {
                if (update.docChanged) {
                    suppressStoreSync = true;
                    updateScriptContent(update.state.doc.toString());
                    suppressStoreSync = false;
                }
                if (update.selectionSet) {
                    const pos = update.state.selection.main.head;
                    const line = update.state.doc.lineAt(pos);
                    cursorLine = line.number;
                    cursorCol = pos - line.from + 1;
                }
            }),
            keymap.of([
                indentWithTab,
                {
                    key: "F9",
                    run: (view) => {
                        const path = get(scriptFilePath);
                        if (!path) return false;
                        const line = view.state.doc.lineAt(view.state.selection.main.head).number;
                        luaDebugger.toggleBreakpoint(path, line);
                        return true;
                    },
                },
                {
                    key: "Mod-s",
                    run: () => {
                        saveCurrentScript();
                        return true;
                    },
                },
            ]),
            EditorView.theme({
                "&": {
                    height: "100%",
                    fontSize: "13px",
                },
                ".cm-scroller": {
                    overflow: "auto",
                },
            }),
        ];
    }

    function createEditor() {
        if (!editorContainer) return;
        destroyEditor();

        const lang = get(scriptLanguage);
        const content = get(scriptContent) ?? "";

        const state = EditorState.create({
            doc: content,
            extensions: buildExtensions(lang),
        });

        editorView = new EditorView({
            state,
            parent: editorContainer,
        });
    }

    // The colours are CSS variables and follow the palette by themselves;
    // only CodeMirror's light / dark flag has to be swapped in.
    $effect(() => {
        const dark = $themeMode === "dark";
        editorView?.dispatch({ effects: themeSlot.reconfigure(codeTheme(dark)) });
    });

    function destroyEditor() {
        if (editorView) {
            editorView.destroy();
            editorView = undefined;
        }
    }

    // Recreate editor when file or language changes
    // NOTE: Do NOT track $scriptContent here — it changes on every keystroke
    // and would cause the editor to be destroyed and recreated continuously.
    let prevPath: string | null = null;
    let prevLang: "lua" | "javascript" | null = null;
    $effect(() => {
        const path = $scriptFilePath;
        const lang = $scriptLanguage;
        const container = editorContainer;
        if (container && path !== null) {
            // Only recreate when file or language actually changes, or first mount
            if (path !== prevPath || lang !== prevLang || !editorView) {
                prevPath = path;
                prevLang = lang;
                createEditor();
            }
        }
    });

    // Sync store content → editor when content changes externally
    $effect(() => {
        const content = $scriptContent;
        if (suppressStoreSync || !editorView || content === null) return;
        const current = editorView.state.doc.toString();
        if (content !== current) {
            editorView.dispatch({
                changes: {
                    from: 0,
                    to: editorView.state.doc.length,
                    insert: content,
                },
            });
        }
    });

    // The debugger's marks for this file: its breakpoints, and the line Play
    // is stopped on when it is stopped here.
    $effect(() => {
        const state = $debuggerState;
        const path = $scriptFilePath;
        const view = editorView;
        if (!view || !path) return;
        const key = scriptKey(path);
        const lines = state.breakpoints.filter((b) => b.source === key).map((b) => b.line);
        const exec = state.pause && state.pause.source === key ? state.pause.line : null;
        view.dispatch({ effects: [setBreakpoints.of(lines), setExecutionLine.of(exec)] });
    });

    // Jump to a requested line (set by console double-click).
    // Tracks editorView so it re-runs after the editor is (re)created.
    $effect(() => {
        const line = $requestedScriptLine;
        const view = editorView;
        if (line === null || !view) return;
        requestedScriptLine.set(null);
        const doc = view.state.doc;
        const targetLine = Math.max(1, Math.min(line, doc.lines));
        const pos = doc.line(targetLine).from;
        view.dispatch({
            selection: { anchor: pos, head: pos },
            effects: EditorView.scrollIntoView(pos, { y: "center" }),
        });
        view.focus();
    });

    onDestroy(() => {
        destroyEditor();
    });

    // ---------------------------------------------------------------------------
    // Actions
    // ---------------------------------------------------------------------------
    async function handleOpenExternal() {
        const proj = get(project);
        const path = get(scriptFilePath);
        if (!proj || !path) return;
        // Build relative path from project root
        const normalized = path.replace(/\\/g, "/");
        const projNorm = proj.path.replace(/\\/g, "/");
        const rel = normalized.startsWith(projNorm)
            ? normalized.slice(projNorm.length + 1)
            : normalized;
        try {
            await openInCodeEditor(
                proj.path,
                rel,
                proj.settings.code_editor ?? "",
            );
        } catch (e) {
            console.error("Failed to open external editor:", e);
        }
    }
</script>

<div class="script-editor">
    <!-- Toolbar -->
    <div class="script-toolbar">
        <div class="toolbar-left">
            <span class="file-name" title={$scriptFilePath ?? ""}>
                {fileName}
            </span>
            <span class="lang-badge" class:lua={$scriptLanguage === "lua"}>
                {$scriptLanguage === "lua" ? "Lua" : "JavaScript"}
            </span>
            {#if $scriptDirty}
                <span class="dirty-dot" title="Unsaved changes">●</span>
            {/if}
        </div>
        <div class="toolbar-right">
            <button
                class="tool-btn"
                title="Save (Ctrl+S)"
                disabled={!$scriptDirty}
                onclick={() => saveCurrentScript()}>💾 Save</button
            >
            <button
                class="tool-btn"
                title="Open in external editor"
                onclick={handleOpenExternal}>🔗 External</button
            >
            <button
                class="tool-btn close-btn"
                title="Close script editor"
                onclick={() =>
                    window.dispatchEvent(
                        new CustomEvent("request-close-script"),
                    )}>✕</button
            >
        </div>
    </div>

    <!-- CodeMirror container -->
    <div class="editor-container" bind:this={editorContainer}></div>

    <!-- Status bar -->
    <div class="script-statusbar">
        <span>Ln {cursorLine}, Col {cursorCol}</span>
        <span>{$scriptLanguage === "lua" ? "Lua" : "JavaScript"}</span>
    </div>
</div>

<style>
    .script-editor {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        background: var(--crust);
        color: var(--ed-text);
        overflow: hidden;
    }

    .script-toolbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 4px 8px;
        background: var(--mantle);
        border-bottom: 1px solid var(--surface0);
        gap: 8px;
        min-height: 32px;
    }

    .toolbar-left {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }

    .toolbar-right {
        display: flex;
        align-items: center;
        gap: 4px;
        flex-shrink: 0;
    }

    .file-name {
        font-weight: 600;
        font-size: 13px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .lang-badge {
        font-size: 11px;
        padding: 1px 6px;
        border-radius: 4px;
        background: var(--yellow);
        color: var(--crust);
        font-weight: 600;
    }

    .lang-badge.lua {
        background: var(--blue);
    }

    .dirty-dot {
        /* Same yellow, same size as the tab strips. */
        color: var(--ed-warn);
        font-size: 9px;
    }

    .tool-btn {
        padding: 2px 8px;
        font-size: 12px;
        background: var(--surface0);
        color: var(--ed-text);
        border: 1px solid var(--surface1);
        border-radius: 4px;
        cursor: pointer;
    }

    .tool-btn:hover:not(:disabled) {
        background: var(--surface1);
    }

    .tool-btn:disabled {
        opacity: 0.4;
        cursor: default;
    }

    .close-btn {
        margin-left: 4px;
    }

    .editor-container {
        flex: 1;
        overflow: hidden;
    }

    .editor-container :global(.cm-editor) {
        height: 100%;
    }

    .script-statusbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 2px 10px;
        font-size: 11px;
        color: var(--subtext0);
        background: var(--mantle);
        border-top: 1px solid var(--surface0);
        min-height: 20px;
    }
</style>
