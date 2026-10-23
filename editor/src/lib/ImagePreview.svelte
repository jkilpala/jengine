<script lang="ts">
    import { readFile } from "@tauri-apps/plugin-fs";
    import { project } from "./stores/projectStore";
    import { saveProject } from "./api/project";
    import { logError, logInfo } from "./stores/consoleStore";
    import { filterFor, withTextureFilter, type FilterMode } from "./textureFilter";

    let {
        projectPath,
        relativePath,
        onclose,
    }: {
        projectPath: string;
        relativePath: string;
        onclose: () => void;
    } = $props();

    let imageUrl = $state<string | null>(null);
    let error = $state("");
    let naturalWidth = $state(0);
    let naturalHeight = $state(0);
    let zoom = $state(1);

    // Track URL outside reactive state for cleanup to avoid $effect loop
    let currentBlobUrl: string | null = null;

    const fileName = $derived(relativePath.split("/").pop() ?? relativePath);

    // This picture's own import setting: its filter (`settings.texture_filter`,
    // an override of its whole id - the longest prefix, so it beats a folder
    // rule). Saved into the project at once; the viewport re-reads it.
    const textureId = $derived(relativePath.replace(/^assets\//, ""));
    let filter = $derived(filterFor($project?.settings.texture_filter as any, textureId));
    let own = $derived(filter.rule === textureId);

    async function setFilter(choice: string) {
        const proj = $project;
        if (!proj) return;
        const mode = choice === "" ? null : (choice as FilterMode);
        const settings = { ...proj.settings, texture_filter: withTextureFilter(proj.settings.texture_filter as any, textureId, mode) as any };
        try {
            await saveProject(proj.path, proj.name, settings.entry_scene ?? proj.default_scene ?? null, settings, proj.component_definitions ?? []);
            project.update((p) => (p ? { ...p, settings } : p));
            logInfo(`${textureId}: ${mode ? `${mode}, its own setting` : "back to the project's rules"}`);
        } catch (e) {
            logError(`Could not save the filter: ${e}`);
        }
    }

    $effect(() => {
        loadImage(projectPath, relativePath);
        return () => {
            if (currentBlobUrl) {
                URL.revokeObjectURL(currentBlobUrl);
                currentBlobUrl = null;
            }
        };
    });

    async function loadImage(projPath: string, relPath: string) {
        try {
            if (currentBlobUrl) URL.revokeObjectURL(currentBlobUrl);
            const fullPath = `${projPath}/${relPath}`;
            const bytes = await readFile(fullPath);
            const ext = relPath.split(".").pop()?.toLowerCase();
            const mime =
                ext === "png"
                    ? "image/png"
                    : ext === "webp"
                      ? "image/webp"
                      : "image/jpeg";
            const blob = new Blob([bytes], { type: mime });
            currentBlobUrl = URL.createObjectURL(blob);
            imageUrl = currentBlobUrl;
            error = "";
        } catch (e) {
            error = `Failed to load image: ${e}`;
            imageUrl = null;
            currentBlobUrl = null;
        }
    }

    function handleWheel(e: WheelEvent) {
        e.preventDefault();
        const factor = e.deltaY > 0 ? 0.9 : 1.1;
        zoom = Math.max(0.1, Math.min(20, zoom * factor));
    }

    function fitToView() {
        zoom = 1;
    }

    function handleImageLoad(e: Event) {
        const img = e.target as HTMLImageElement;
        naturalWidth = img.naturalWidth;
        naturalHeight = img.naturalHeight;
    }
</script>

<div class="image-preview" onwheel={handleWheel}>
    <div class="toolbar">
        <span class="filename">{fileName}</span>
        {#if naturalWidth > 0}
            <span class="dimensions">{naturalWidth} × {naturalHeight}px</span>
        {/if}
        <label
            class="filter"
            title={own
                ? "This picture's own filter"
                : filter.rule
                  ? `From the project's rule for "${filter.rule}"`
                  : "The project's default (Project Settings - Graphics)"}
        >
            Filter
            <select value={own ? filter.mode : ""} onchange={(e) => setFilter((e.target as HTMLSelectElement).value)}>
                <option value="">as the project says ({filter.mode})</option>
                <option value="nearest">nearest (pixel art)</option>
                <option value="linear">linear, with mipmaps</option>
            </select>
        </label>
        <span class="zoom-label">{Math.round(zoom * 100)}%</span>
        <button class="tool-btn" onclick={fitToView} title="Reset zoom"
            >1:1</button
        >
        <button class="tool-btn close" onclick={onclose} title="Close preview"
            >✕</button
        >
    </div>
    <div class="canvas-area">
        {#if error}
            <div class="error">{error}</div>
        {:else if imageUrl}
            <img
                src={imageUrl}
                alt={fileName}
                style="transform: scale({zoom}); image-rendering: {zoom >= 2
                    ? 'pixelated'
                    : 'auto'};"
                onload={handleImageLoad}
                draggable="false"
            />
        {:else}
            <div class="loading">Loading...</div>
        {/if}
    </div>
</div>

<style>
    .image-preview {
        display: flex;
        flex-direction: column;
        height: 100%;
        background: var(--ed-base);
    }

    .toolbar {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 4px 10px;
        background: var(--ed-mantle);
        border-bottom: 1px solid var(--ed-line);
        font-size: 12px;
        color: var(--ed-muted);
        flex-shrink: 0;
    }

    .filename {
        font-weight: 600;
        color: var(--ed-text);
    }

    .dimensions {
        color: var(--ed-faint);
    }

    .zoom-label {
        color: var(--ed-faint);
        min-width: 40px;
        text-align: right;
    }

    .tool-btn {
        background: none;
        border: 1px solid var(--ed-line);
        color: var(--ed-muted);
        cursor: pointer;
        padding: 2px 8px;
        border-radius: 4px;
        font-size: 11px;
    }

    .tool-btn:hover {
        background: var(--ed-raised);
        color: var(--ed-text);
    }

    .tool-btn.close {
        margin-left: auto;
        border: none;
        font-size: 14px;
    }

    .canvas-area {
        flex: 1;
        overflow: auto;
        display: flex;
        align-items: center;
        justify-content: center;
        /* Checkerboard background for transparency */
        background-image: linear-gradient(45deg, var(--ed-hover) 25%, transparent 25%),
            linear-gradient(-45deg, var(--ed-hover) 25%, transparent 25%),
            linear-gradient(45deg, transparent 75%, var(--ed-hover) 75%),
            linear-gradient(-45deg, transparent 75%, var(--ed-hover) 75%);
        background-size: 16px 16px;
        background-position:
            0 0,
            0 8px,
            8px -8px,
            -8px 0px;
        background-color: var(--ed-base);
    }

    .canvas-area img {
        max-width: none;
        transform-origin: center;
        user-select: none;
        -webkit-user-select: none;
    }

    .error {
        color: var(--ed-err);
        padding: 20px;
    }

    .loading {
        color: var(--ed-faint);
        font-style: italic;
    }
    .filter {
        display: flex;
        align-items: center;
        gap: 4px;
        font-size: 11px;
        color: var(--ed-muted);
    }
    .filter select {
        background: var(--ed-base);
        border: 1px solid var(--ed-line);
        color: var(--ed-text);
        border-radius: 4px;
        font-size: 11px;
    }
</style>
