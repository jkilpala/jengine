<script lang="ts">
    interface Props {
        onclose: () => void;
    }
    let { onclose }: Props = $props();

    const sections = [
        {
            title: "Gizmo Tools",
            shortcuts: [
                { key: "W", desc: "Move tool" },
                { key: "E", desc: "Rotate tool" },
                { key: "R", desc: "Scale tool" },
                { key: "T", desc: "Toggle local / global space" },
                { key: "F", desc: "Focus camera on selected entity" },
            ],
        },
        {
            title: "Viewport Overlays",
            shortcuts: [
                { key: "C", desc: "Toggle collider outlines" },
                { key: "L", desc: "Toggle lighting preview" },
                { key: "J", desc: "Toggle joint visualization" },
                { key: "F3", desc: "Toggle profiler overlay" },
            ],
        },
        {
            title: "Entity Actions",
            shortcuts: [
                { key: "Delete / Backspace", desc: "Delete selected entity" },
                { key: "Escape", desc: "Deselect entity" },
                { key: "Ctrl+C", desc: "Copy entity" },
                { key: "Ctrl+V", desc: "Paste entity" },
                { key: "Ctrl+D", desc: "Duplicate entity" },
            ],
        },
        {
            title: "General",
            shortcuts: [
                { key: "Ctrl+S", desc: "Save scene / active editor" },
                { key: "Ctrl+Shift+A", desc: "Command palette" },
                { key: "Ctrl+P", desc: "Go to file (quick open)" },
                { key: "Ctrl+Tab", desc: "Switch to last script" },
                { key: "Ctrl+Shift+F", desc: "Find in files" },
                { key: "Ctrl+Shift+H", desc: "Replace in files" },
                {
                    key: "F2",
                    desc: "Rename the selected entity or asset (whichever tree you clicked last)",
                },
                { key: "Ctrl+O", desc: "Open scene switcher" },
                { key: "Ctrl+Home", desc: "Open entry scene" },
                { key: "Ctrl+Z", desc: "Undo" },
                { key: "Ctrl+Shift+Z / Ctrl+Y", desc: "Redo" },
                { key: "F1", desc: "Show this shortcut reference" },
            ],
        },
        {
            title: "Play Mode",
            shortcuts: [
                { key: "Ctrl+Shift+P", desc: "Pause / resume play mode" },
                { key: "Ctrl+Alt+P", desc: "Step one frame (pauses first)" },
                { key: "Shift+F5", desc: "Stop play mode" },
                {
                    key: "Game view",
                    desc: "Running play owns the keyboard - only the three keys above reach the editor",
                },
                {
                    key: "Scene view",
                    desc: "Also keeps W E R T F, C L J, F3, Esc, palette, quick open and find; other keys drive the game",
                },
                {
                    key: "Paused",
                    desc: "Every editor shortcut works",
                },
            ],
        },
        {
            title: "Window (Debug menu)",
            shortcuts: [
                { key: "Ctrl+R / F5", desc: "Reload the editor window" },
                { key: "F12", desc: "Toggle developer tools" },
            ],
        },
    ];
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="shortcut-overlay">
    <div class="shortcut-dialog">
        <div class="dialog-header">
            <h2>Keyboard Shortcuts</h2>
            <button class="close-btn" onclick={onclose}>×</button>
        </div>
        <div class="dialog-body">
            {#each sections as section}
                <div class="shortcut-section">
                    <h3>{section.title}</h3>
                    {#each section.shortcuts as sc}
                        <div class="shortcut-row">
                            <kbd>{sc.key}</kbd>
                            <span>{sc.desc}</span>
                        </div>
                    {/each}
                </div>
            {/each}
        </div>
    </div>
</div>

<style>
    .shortcut-overlay {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.6);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 9999;
    }

    .shortcut-dialog {
        background: var(--ed-base);
        border: 1px solid var(--ed-line);
        border-radius: 8px;
        width: 480px;
        max-height: 80vh;
        display: flex;
        flex-direction: column;
        box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
    }

    .dialog-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 12px 16px;
        border-bottom: 1px solid var(--ed-line);
    }

    .dialog-header h2 {
        margin: 0;
        font-size: 14px;
        color: var(--ed-text);
    }

    .close-btn {
        background: none;
        border: none;
        color: var(--ed-faint);
        font-size: 18px;
        cursor: pointer;
        padding: 2px 6px;
        border-radius: 4px;
    }

    .close-btn:hover {
        color: var(--ed-text);
        background: var(--ed-raised);
    }

    .dialog-body {
        padding: 12px 16px;
        overflow-y: auto;
    }

    .shortcut-section {
        margin-bottom: 16px;
    }

    .shortcut-section:last-child {
        margin-bottom: 0;
    }

    .shortcut-section h3 {
        margin: 0 0 8px;
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.05em;
        color: var(--ed-accent);
    }

    .shortcut-row {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 4px 0;
    }

    kbd {
        display: inline-block;
        min-width: 80px;
        padding: 2px 8px;
        font-family: "JetBrains Mono", monospace;
        font-size: 11px;
        color: var(--ed-text);
        background: var(--ed-mantle);
        border: 1px solid var(--ed-line-strong);
        border-radius: 4px;
        text-align: center;
    }

    .shortcut-row span {
        font-size: 12px;
        color: var(--ed-muted);
    }
</style>
