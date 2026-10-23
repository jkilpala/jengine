<script lang="ts">
    import { get } from "svelte/store";
    import { scene, dirty, scenePath } from "./stores/sceneStore";
    import { selectedEntity, clearSelection } from "./stores/selectionStore";
    import { project } from "./stores/projectStore";
    import { clearHistory } from "./stores/commandStore";
    import { logInfo, logError } from "./stores/consoleStore";
    import { loadScene } from "./api/scene";
    import {
        focusOrCreateSceneTab,
        findSceneTab,
        isSceneTabDirty,
        sceneTabs,
    } from "./stores/sceneTabsStore";
    import { refreshProjectFiles } from "./api/project";

    interface Props {
        onclose: () => void;
    }
    let { onclose }: Props = $props();

    let sceneFiles = $state<string[]>([]);
    let searchQuery = $state("");
    let loading = $state(true);

    const currentPath = $derived($scenePath);

    /**
     * Whether this entry has unsaved edits.
     *
     * Reads the tab strip, not just the live `dirty` flag: with tabs, a scene
     * with unsaved work is usually *not* the one in front, and a dot that only
     * lit for the front scene said "saved" about work that was not.
     * `$sceneTabs` and `$dirty` are touched so the dot re-renders when either
     * moves.
     */
    function isDirty(relativePath: string): boolean {
        void $sceneTabs;
        void $dirty;
        const proj = get(project);
        if (!proj) return false;
        const index = findSceneTab(`${proj.path}/assets/${relativePath}`);
        return index >= 0 && isSceneTabDirty(index);
    }

    let filtered = $derived(
        sceneFiles.filter((f) =>
            f.toLowerCase().includes(searchQuery.toLowerCase()),
        ),
    );

    $effect(() => {
        const proj = get(project);
        if (proj) {
            refreshProjectFiles(proj.path)
                .then((files) => {
                    sceneFiles = files.scene_files;
                    loading = false;
                })
                .catch((e) => {
                    logError(`Failed to list scenes: ${e}`);
                    loading = false;
                });
        }
    });

    async function openScene(relativePath: string) {
        const proj = get(project);
        if (!proj) return;

        // `scene_files` entries are relative to `assets/` (see
        // `list_files_with_ext` in the Tauri `project` commands), not to
        // the project root.
        const fullPath = `${proj.path}/assets/${relativePath}`;

        // Go through the tab system instead of replacing the live stores
        // behind its back. It parks the scene you were on - so there is
        // nothing to discard and nothing to ask about - and if this scene is
        // already open it switches to that tab rather than reloading over
        // the edits it holds.
        if (!focusOrCreateSceneTab(fullPath)) {
            logInfo(`Switched to scene: ${relativePath}`);
            onclose();
            return;
        }

        try {
            const s = await loadScene(fullPath);
            scene.set(s);
            scenePath.set(fullPath);
            clearSelection();
            dirty.set(false);
            clearHistory();
            logInfo(`Opened scene: ${relativePath}`);
            onclose();
        } catch (e) {
            logError(`Failed to open scene: ${e}`);
        }
    }

    function handleOverlayClick(e: MouseEvent) {
        if (e.target === e.currentTarget) onclose();
    }

    let inputEl: HTMLInputElement | undefined = $state();

    $effect(() => {
        inputEl?.focus();
    });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="scene-overlay" onclick={handleOverlayClick}>
    <div class="scene-dialog">
        <div class="dialog-header">
            <h2>Open Scene</h2>
            <button class="close-btn" onclick={onclose}>×</button>
        </div>
        <div class="search-bar">
            <input
                type="text"
                bind:this={inputEl}
                bind:value={searchQuery}
                placeholder="Search scenes…"
                class="search-input"
                onkeydown={(e) => {
                    if (e.key === "Escape") onclose();
                    if (e.key === "Enter" && filtered.length === 1) {
                        openScene(filtered[0]);
                    }
                }}
            />
        </div>
        <div class="scene-list">
            {#if loading}
                <div class="empty">Loading…</div>
            {:else if filtered.length === 0}
                <div class="empty">
                    {searchQuery ? "No matching scenes" : "No scenes found"}
                </div>
            {:else}
                {#each filtered as scenFile}
                    <button
                        class="scene-item"
                        class:current={currentPath?.endsWith(scenFile)}
                        onclick={() => openScene(scenFile)}
                    >
                        <span class="scene-icon">🎬</span>
                        <span class="scene-name">{scenFile}</span>
                        {#if currentPath?.endsWith(scenFile)}
                            <span class="current-badge">current</span>
                        {/if}
                        {#if isDirty(scenFile)}
                            <span class="dirty-dot">●</span>
                        {/if}
                    </button>
                {/each}
            {/if}
        </div>
    </div>
</div>

<style>
    .scene-overlay {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.6);
        display: flex;
        align-items: flex-start;
        justify-content: center;
        padding-top: 15vh;
        z-index: 9999;
    }

    .scene-dialog {
        background: var(--ed-base);
        border: 1px solid var(--ed-line);
        border-radius: 8px;
        width: 420px;
        max-height: 60vh;
        display: flex;
        flex-direction: column;
        box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
    }

    .dialog-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 10px 16px;
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

    .search-bar {
        padding: 8px 12px;
        border-bottom: 1px solid var(--ed-line);
    }

    .search-input {
        width: 100%;
        box-sizing: border-box;
        padding: 6px 8px;
        font-size: 13px;
        color: var(--ed-text);
        background: var(--ed-mantle);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        outline: none;
    }

    .search-input:focus {
        border-color: var(--ed-accent);
    }

    .scene-list {
        flex: 1;
        overflow-y: auto;
        padding: 4px 0;
    }

    .scene-item {
        display: flex;
        align-items: center;
        gap: 8px;
        width: 100%;
        padding: 8px 16px;
        border: none;
        background: none;
        color: var(--ed-text);
        font-size: 13px;
        text-align: left;
        cursor: pointer;
    }

    .scene-item:hover {
        background: var(--ed-raised);
    }

    .scene-item.current {
        background: var(--ed-accent-low);
    }

    .scene-icon {
        flex-shrink: 0;
    }

    .scene-name {
        flex: 1;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .current-badge {
        font-size: 10px;
        color: var(--ed-accent);
        padding: 1px 6px;
        border: 1px solid var(--ed-accent);
        border-radius: 4px;
    }

    .dirty-dot {
        color: var(--ed-warn);
        font-size: 14px;
    }

    .empty {
        padding: 16px;
        color: var(--ed-faint);
        font-size: 12px;
        text-align: center;
    }
</style>
