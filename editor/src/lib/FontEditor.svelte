<script lang="ts">
    import { themeColors, withAlpha } from "./stores/themeColors";
    import { requestEditorClose } from "./stores/editorClose";
    import { onMount, onDestroy } from "svelte";
    import { get } from "svelte/store";
    import {
        fontData,
        fontFilePath,
        fontDirty,
        selectedGlyph,
        canUndoFont,
        canRedoFont,
        undoFont,
        redoFont,
        executeFont,
        saveCurrentFont,
        closeFontEditor,
    } from "./stores/fontEditorStore";
    import type { FontFileData, GlyphInfoData } from "./api/font";
    import { generateGridFont } from "./api/font";
    import { listProjectImages, readAssetBinary } from "./api/project";
    import { project } from "./stores/projectStore";
    import { siblingAtlasPath } from "./api/font";

    // ---------------------------------------------------------------------------
    // State
    // ---------------------------------------------------------------------------
    let atlasCanvas: HTMLCanvasElement | undefined = $state();
    let previewCanvas: HTMLCanvasElement | undefined = $state();
    let atlasImage: HTMLImageElement | null = $state(null);
    let loadedTextureId = $state<string | null>(null);

    // Collapsible sections
    let showAtlasSettings = $state(true);
    let showGridGen = $state(true);
    let showGlyphList = $state(true);
    let showGlyphInspector = $state(true);
    let showPreview = $state(true);

    // Project images
    let projectImages = $state<string[]>([]);

    // Grid generator inputs
    let gridCols = $state(16);
    let gridRows = $state(8);
    let gridStartChar = $state(32);
    let gridEndChar = $state(126);

    // Preview text
    let previewText = $state("The quick brown fox jumps over the lazy dog.");
    let previewScale = $state(2);

    const fileName = $derived(
        $fontFilePath?.split(/[\\/]/).pop()?.replace(".font", "") ?? "Font",
    );

    const glyphCount = $derived($fontData?.glyphs.length ?? 0);

    /** The open font's project-relative asset path (what a `font_id` spells). */
    const relativeFontPath = $derived.by(() => {
        const full = $fontFilePath?.replace(/\\/g, "/") ?? "";
        const idx = full.indexOf("/assets/");
        if (idx >= 0) return full.slice(idx + "/assets/".length);
        return full.split("/").pop() ?? "";
    });

    /** The atlas the *engine* will pair with this font, regardless of what
     *  `texture_id` says - it resolves the sibling by stem and never reads
     *  `texture_id` at all. */
    const expectedAtlas = $derived(
        relativeFontPath ? siblingAtlasPath(relativeFontPath) : "",
    );

    /** `texture_id` points somewhere the runtime will not look. The preview
     *  here would be right and the game wrong, which is the worst way round. */
    const atlasMismatch = $derived(
        !!$fontData?.texture_id &&
            !!expectedAtlas &&
            $fontData.texture_id !== expectedAtlas,
    );

    // ---------------------------------------------------------------------------
    // Atlas image loading
    // ---------------------------------------------------------------------------
    $effect(() => {
        const data = $fontData;
        if (!data || !data.texture_id) {
            atlasImage = null;
            loadedTextureId = null;
            return;
        }
        if (data.texture_id === loadedTextureId) return;
        const proj = get(project);
        if (!proj) return;
        const texId = data.texture_id;
        (async () => {
            try {
                const bytes = await readAssetBinary(proj.path, texId);
                const blob = new Blob([new Uint8Array(bytes)]);
                const url = URL.createObjectURL(blob);
                const img = new Image();
                img.onload = () => {
                    atlasImage = img;
                    loadedTextureId = texId;
                    // Auto-detect atlas dimensions from image
                    if (
                        data.atlas_width !== img.naturalWidth ||
                        data.atlas_height !== img.naturalHeight
                    ) {
                        executeFont("Auto-detect atlas size", (fd) => {
                            fd.atlas_width = img.naturalWidth;
                            fd.atlas_height = img.naturalHeight;
                        });
                    }
                    drawAtlas();
                };
                img.src = url;
            } catch (e) {
                console.error("Failed to load font atlas:", e);
            }
        })();
    });

    // Redraw atlas when selection or data changes
    $effect(() => {
        void $fontData;
        void $selectedGlyph;
        void atlasImage;
        drawAtlas();
    });

    // Redraw preview when data or preview text changes
    $effect(() => {
        void $fontData;
        void previewText;
        void previewScale;
        void atlasImage;
        drawPreview();
    });

    // ---------------------------------------------------------------------------
    // Lifecycle
    // ---------------------------------------------------------------------------
    onMount(async () => {
        const proj = get(project);
        if (proj) {
            projectImages = await listProjectImages(proj.path);
        }
    });

    // ---------------------------------------------------------------------------
    // Canvas drawing: Atlas
    // ---------------------------------------------------------------------------
    function drawAtlas() {
        if (!atlasCanvas) return;
        const ctx = atlasCanvas.getContext("2d");
        if (!ctx) return;
        const data = get(fontData);
        if (!data) return;

        const cw = atlasCanvas.parentElement?.clientWidth ?? 400;
        const ch = atlasCanvas.parentElement?.clientHeight ?? 300;
        atlasCanvas.width = cw;
        atlasCanvas.height = ch;

        // Clear
        ctx.fillStyle = $themeColors.crust;
        ctx.fillRect(0, 0, cw, ch);

        if (!atlasImage) {
            ctx.fillStyle = $themeColors.faint;
            ctx.font = "14px sans-serif";
            ctx.textAlign = "center";
            ctx.fillText("Select a texture to view atlas", cw / 2, ch / 2);
            return;
        }

        // Compute scale to fit atlas in canvas
        const scaleX = cw / data.atlas_width;
        const scaleY = ch / data.atlas_height;
        const scale = Math.min(scaleX, scaleY, 4);
        const ox = (cw - data.atlas_width * scale) / 2;
        const oy = (ch - data.atlas_height * scale) / 2;

        // Draw checkerboard background
        const checkSize = 8;
        for (let y = 0; y < data.atlas_height * scale; y += checkSize) {
            for (let x = 0; x < data.atlas_width * scale; x += checkSize) {
                const dark =
                    (Math.floor(x / checkSize) + Math.floor(y / checkSize)) %
                        2 ===
                    0;
                ctx.fillStyle = dark ? $themeColors.base : $themeColors.hover;
                ctx.fillRect(
                    ox + x,
                    oy + y,
                    Math.min(checkSize, data.atlas_width * scale - x),
                    Math.min(checkSize, data.atlas_height * scale - y),
                );
            }
        }

        // Draw atlas image
        ctx.imageSmoothingEnabled = false;
        ctx.drawImage(
            atlasImage,
            ox,
            oy,
            data.atlas_width * scale,
            data.atlas_height * scale,
        );

        // Draw glyph rectangles
        const sel = get(selectedGlyph);
        for (let i = 0; i < data.glyphs.length; i++) {
            const g = data.glyphs[i];
            const gx = ox + g.x * scale;
            const gy = oy + g.y * scale;
            const gw = g.width * scale;
            const gh = g.height * scale;

            if (i === sel) {
                ctx.strokeStyle = $themeColors.accent;
                ctx.lineWidth = 2;
                ctx.fillStyle = withAlpha($themeColors.accent, 0.15);
                ctx.fillRect(gx, gy, gw, gh);
            } else {
                ctx.strokeStyle = withAlpha($themeColors.text, 0.2);
                ctx.lineWidth = 1;
            }
            ctx.strokeRect(gx, gy, gw, gh);
        }

        // Store scale info for click handling
        atlasCanvas.dataset.scale = String(scale);
        atlasCanvas.dataset.ox = String(ox);
        atlasCanvas.dataset.oy = String(oy);
    }

    function handleAtlasClick(e: MouseEvent) {
        if (!atlasCanvas || !$fontData) return;
        const rect = atlasCanvas.getBoundingClientRect();
        const mx = e.clientX - rect.left;
        const my = e.clientY - rect.top;
        const scale = parseFloat(atlasCanvas.dataset.scale ?? "1");
        const ox = parseFloat(atlasCanvas.dataset.ox ?? "0");
        const oy = parseFloat(atlasCanvas.dataset.oy ?? "0");

        // Convert to atlas pixel coords
        const ax = (mx - ox) / scale;
        const ay = (my - oy) / scale;

        // Find glyph under cursor
        for (let i = 0; i < $fontData.glyphs.length; i++) {
            const g = $fontData.glyphs[i];
            if (
                ax >= g.x &&
                ax < g.x + g.width &&
                ay >= g.y &&
                ay < g.y + g.height
            ) {
                selectedGlyph.set(i);
                return;
            }
        }
        selectedGlyph.set(-1);
    }

    // ---------------------------------------------------------------------------
    // Canvas drawing: Text Preview
    // ---------------------------------------------------------------------------
    function drawPreview() {
        if (!previewCanvas) return;
        const ctx = previewCanvas.getContext("2d");
        if (!ctx) return;
        const data = get(fontData);

        const cw = previewCanvas.parentElement?.clientWidth ?? 400;
        const ch = 80;
        previewCanvas.width = cw;
        previewCanvas.height = ch;

        ctx.fillStyle = $themeColors.mantle;
        ctx.fillRect(0, 0, cw, ch);

        if (!data || !atlasImage || data.glyphs.length === 0) {
            ctx.fillStyle = $themeColors.faint;
            ctx.font = "12px sans-serif";
            ctx.textAlign = "center";
            ctx.fillText(
                "Add glyphs and a texture to preview text",
                cw / 2,
                ch / 2,
            );
            return;
        }

        // Build lookup
        const lookup = new Map<string, GlyphInfoData>();
        for (const g of data.glyphs) {
            lookup.set(g.ch, g);
        }

        ctx.imageSmoothingEnabled = false;
        let cursorX = 8;
        let cursorY = 8;
        const s = previewScale;

        for (const ch of previewText) {
            if (ch === "\n") {
                cursorX = 8;
                cursorY += data.line_height * s;
                continue;
            }
            const g = lookup.get(ch);
            if (!g) {
                cursorX += data.line_height * 0.5 * s;
                continue;
            }
            ctx.drawImage(
                atlasImage,
                g.x,
                g.y,
                g.width,
                g.height,
                cursorX,
                cursorY,
                g.width * s,
                g.height * s,
            );
            cursorX += g.advance * s;
        }
    }

    // ---------------------------------------------------------------------------
    // Actions
    // ---------------------------------------------------------------------------
    function applyGridGenerator() {
        if (!$fontData) return;
        const glyphs = generateGridFont(
            $fontData.atlas_width,
            $fontData.atlas_height,
            gridCols,
            gridRows,
            gridStartChar,
            gridEndChar,
        );
        executeFont("Generate grid glyphs", (fd) => {
            fd.glyphs = glyphs;
            fd.line_height = Math.floor(fd.atlas_height / gridRows);
        });
        selectedGlyph.set(-1);
    }

    function addGlyph() {
        executeFont("Add glyph", (fd) => {
            const nextCode =
                fd.glyphs.length > 0
                    ? fd.glyphs[fd.glyphs.length - 1].ch.charCodeAt(0) + 1
                    : 32;
            fd.glyphs.push({
                ch: String.fromCharCode(nextCode),
                x: 0,
                y: 0,
                width: 8,
                height: fd.line_height,
                advance: 8,
            });
        });
        selectedGlyph.set(($fontData?.glyphs.length ?? 1) - 1);
    }

    function removeGlyph(idx: number) {
        executeFont("Remove glyph", (fd) => {
            fd.glyphs.splice(idx, 1);
        });
        if ($selectedGlyph >= ($fontData?.glyphs.length ?? 0)) {
            selectedGlyph.set(
                Math.max(-1, ($fontData?.glyphs.length ?? 1) - 1),
            );
        }
    }

    function handleKeydown(e: KeyboardEvent) {
        if (e.ctrlKey && e.key === "z" && !e.shiftKey) {
            e.preventDefault();
            e.stopPropagation();
            undoFont();
        } else if (
            e.ctrlKey &&
            (e.key === "y" || (e.key === "z" && e.shiftKey))
        ) {
            e.preventDefault();
            e.stopPropagation();
            redoFont();
        }
    }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="font-editor" onkeydown={handleKeydown} tabindex="-1">
    <!-- Left panel: properties -->
    <div class="properties-panel">
        <!-- Toolbar -->
        <div class="panel-toolbar">
            <button
                class="tool-btn"
                title="Undo (Ctrl+Z)"
                disabled={!$canUndoFont}
                onclick={undoFont}>↩</button
            >
            <button
                class="tool-btn"
                title="Redo (Ctrl+Y)"
                disabled={!$canRedoFont}
                onclick={redoFont}>↪</button
            >
            <div class="toolbar-spacer"></div>
            <button
                class="tool-btn"
                title="Save (Ctrl+S)"
                onclick={saveCurrentFont}>💾</button
            >
            <button class="tool-btn" title="Close" onclick={() => requestEditorClose("font")}
                >✕</button
            >
        </div>

        <div class="panel-scroll">
            <!-- Atlas Settings -->
            <button
                class="section-toggle"
                onclick={() => (showAtlasSettings = !showAtlasSettings)}
            >
                {showAtlasSettings ? "▼" : "▶"} Atlas Settings
            </button>
            {#if showAtlasSettings && $fontData}
                <div class="section">
                    <label>
                        Texture
                        <select
                            value={$fontData.texture_id}
                            onchange={(e) => {
                                const val = (e.target as HTMLSelectElement)
                                    .value;
                                executeFont("Set texture", (fd) => {
                                    fd.texture_id = val;
                                });
                            }}
                        >
                            <option value="">-- None --</option>
                            {#each projectImages as img}
                                <option value={img}>{img}</option>
                            {/each}
                        </select>
                    </label>
                    {#if atlasMismatch}
                        <p class="atlas-warning">
                            The runtime loads <code>{expectedAtlas}</code> — the
                            sibling of this <code>.font</code> — and ignores the
                            texture picked here. This preview will not match the
                            game.
                        </p>
                    {/if}
                    <div class="row-2">
                        <label>
                            Atlas W
                            <input
                                type="number"
                                min="1"
                                value={$fontData.atlas_width}
                                onchange={(e) =>
                                    executeFont("Set atlas width", (fd) => {
                                        fd.atlas_width =
                                            parseInt(
                                                (e.target as HTMLInputElement)
                                                    .value,
                                            ) || 256;
                                    })}
                            />
                        </label>
                        <label>
                            Atlas H
                            <input
                                type="number"
                                min="1"
                                value={$fontData.atlas_height}
                                onchange={(e) =>
                                    executeFont("Set atlas height", (fd) => {
                                        fd.atlas_height =
                                            parseInt(
                                                (e.target as HTMLInputElement)
                                                    .value,
                                            ) || 128;
                                    })}
                            />
                        </label>
                    </div>
                    <label>
                        Line Height
                        <input
                            type="number"
                            min="1"
                            step="1"
                            value={$fontData.line_height}
                            onchange={(e) =>
                                executeFont("Set line height", (fd) => {
                                    fd.line_height =
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 16;
                                })}
                        />
                    </label>
                </div>
            {/if}

            <!-- Grid Generator -->
            <button
                class="section-toggle"
                onclick={() => (showGridGen = !showGridGen)}
            >
                {showGridGen ? "▼" : "▶"} Grid Generator
            </button>
            {#if showGridGen}
                <div class="section">
                    <div class="row-2">
                        <label>
                            Columns
                            <input
                                type="number"
                                min="1"
                                max="64"
                                bind:value={gridCols}
                            />
                        </label>
                        <label>
                            Rows
                            <input
                                type="number"
                                min="1"
                                max="64"
                                bind:value={gridRows}
                            />
                        </label>
                    </div>
                    <div class="row-2">
                        <label>
                            Start Char
                            <input
                                type="number"
                                min="0"
                                max="255"
                                bind:value={gridStartChar}
                            />
                            <span class="char-preview"
                                >{String.fromCharCode(gridStartChar)} (#{gridStartChar})</span
                            >
                        </label>
                        <label>
                            End Char
                            <input
                                type="number"
                                min="0"
                                max="255"
                                bind:value={gridEndChar}
                            />
                            <span class="char-preview"
                                >{String.fromCharCode(gridEndChar)} (#{gridEndChar})</span
                            >
                        </label>
                    </div>
                    <button class="action-btn" onclick={applyGridGenerator}>
                        🔲 Generate {gridEndChar - gridStartChar + 1} Glyphs
                    </button>
                </div>
            {/if}

            <!-- Glyph List -->
            <button
                class="section-toggle"
                onclick={() => (showGlyphList = !showGlyphList)}
            >
                {showGlyphList ? "▼" : "▶"} Glyphs ({glyphCount})
            </button>
            {#if showGlyphList && $fontData}
                <div class="section">
                    <button class="action-btn" onclick={addGlyph}
                        >+ Add Glyph</button
                    >
                    <div class="glyph-list">
                        {#each $fontData.glyphs as glyph, i}
                            <!-- svelte-ignore a11y_click_events_have_key_events -->
                            <div
                                class="glyph-item"
                                class:selected={$selectedGlyph === i}
                                onclick={() => selectedGlyph.set(i)}
                                role="button"
                                tabindex="0"
                            >
                                <span class="glyph-char"
                                    >{glyph.ch === " " ? "⎵" : glyph.ch}</span
                                >
                                <span class="glyph-code"
                                    >#{glyph.ch.charCodeAt(0)}</span
                                >
                                <span class="glyph-dims"
                                    >{glyph.width}×{glyph.height}</span
                                >
                                <button
                                    class="glyph-btn danger"
                                    title="Remove"
                                    onclick={(e) => {
                                        e.stopPropagation();
                                        removeGlyph(i);
                                    }}>✕</button
                                >
                            </div>
                        {/each}
                    </div>
                </div>
            {/if}

            <!-- Selected Glyph Inspector -->
            <button
                class="section-toggle"
                onclick={() => (showGlyphInspector = !showGlyphInspector)}
            >
                {showGlyphInspector ? "▼" : "▶"} Selected Glyph
            </button>
            {#if showGlyphInspector && $fontData && $selectedGlyph >= 0 && $fontData.glyphs[$selectedGlyph]}
                {@const glyph = $fontData.glyphs[$selectedGlyph]}
                <div class="section">
                    <label>
                        Character
                        <input
                            type="text"
                            maxlength="1"
                            value={glyph.ch}
                            onchange={(e) => {
                                const idx = $selectedGlyph;
                                const val = (e.target as HTMLInputElement)
                                    .value;
                                if (val.length === 1) {
                                    executeFont("Set char", (fd) => {
                                        fd.glyphs[idx].ch = val;
                                    });
                                }
                            }}
                        />
                    </label>
                    <div class="row-2">
                        <label>
                            X
                            <input
                                type="number"
                                min="0"
                                value={glyph.x}
                                onchange={(e) => {
                                    const idx = $selectedGlyph;
                                    executeFont("Set glyph X", (fd) => {
                                        fd.glyphs[idx].x =
                                            parseInt(
                                                (e.target as HTMLInputElement)
                                                    .value,
                                            ) || 0;
                                    });
                                }}
                            />
                        </label>
                        <label>
                            Y
                            <input
                                type="number"
                                min="0"
                                value={glyph.y}
                                onchange={(e) => {
                                    const idx = $selectedGlyph;
                                    executeFont("Set glyph Y", (fd) => {
                                        fd.glyphs[idx].y =
                                            parseInt(
                                                (e.target as HTMLInputElement)
                                                    .value,
                                            ) || 0;
                                    });
                                }}
                            />
                        </label>
                    </div>
                    <div class="row-2">
                        <label>
                            Width
                            <input
                                type="number"
                                min="1"
                                value={glyph.width}
                                onchange={(e) => {
                                    const idx = $selectedGlyph;
                                    executeFont("Set glyph width", (fd) => {
                                        fd.glyphs[idx].width =
                                            parseInt(
                                                (e.target as HTMLInputElement)
                                                    .value,
                                            ) || 1;
                                    });
                                }}
                            />
                        </label>
                        <label>
                            Height
                            <input
                                type="number"
                                min="1"
                                value={glyph.height}
                                onchange={(e) => {
                                    const idx = $selectedGlyph;
                                    executeFont("Set glyph height", (fd) => {
                                        fd.glyphs[idx].height =
                                            parseInt(
                                                (e.target as HTMLInputElement)
                                                    .value,
                                            ) || 1;
                                    });
                                }}
                            />
                        </label>
                    </div>
                    <label>
                        Advance
                        <input
                            type="number"
                            min="0"
                            step="0.5"
                            value={glyph.advance}
                            onchange={(e) => {
                                const idx = $selectedGlyph;
                                executeFont("Set advance", (fd) => {
                                    fd.glyphs[idx].advance =
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0;
                                });
                            }}
                        />
                    </label>
                </div>
            {/if}
        </div>
    </div>

    <!-- Center: Atlas view + Preview -->
    <div class="center-area">
        <div class="center-toolbar">
            <span class="file-name">🔤 {fileName}</span>
            <span class="info-badge">{glyphCount} glyphs</span>
        </div>

        <!-- Atlas canvas -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="atlas-container" role="img" tabindex="-1">
            <canvas
                id="font-atlas-canvas"
                bind:this={atlasCanvas}
                onclick={handleAtlasClick}
            ></canvas>
        </div>

        <!-- Text preview -->
        <button
            class="section-toggle preview-toggle"
            onclick={() => (showPreview = !showPreview)}
        >
            {showPreview ? "▼" : "▶"} Text Preview
        </button>
        {#if showPreview}
            <div class="preview-section">
                <div class="preview-controls">
                    <input
                        type="text"
                        class="preview-input"
                        placeholder="Type preview text..."
                        bind:value={previewText}
                    />
                    <label class="scale-label">
                        {previewScale}×
                        <input
                            type="range"
                            min="1"
                            max="6"
                            step="1"
                            bind:value={previewScale}
                        />
                    </label>
                </div>
                <div class="preview-canvas-container">
                    <canvas id="font-preview-canvas" bind:this={previewCanvas}
                    ></canvas>
                </div>
            </div>
        {/if}
    </div>
</div>

<style>
    .atlas-warning {
        margin: 4px 0 0;
        padding: 4px 6px;
        border-left: 2px solid var(--ed-warn);
        background: color-mix(in srgb, var(--ed-warn) 10%, var(--ed-base));
        color: var(--ed-warn);
        font-size: 11px;
        line-height: 1.4;
    }

    .atlas-warning code {
        color: var(--ed-a3);
    }

    .font-editor {
        display: flex;
        width: 100%;
        height: 100%;
        background: var(--ed-base);
        color: var(--ed-text);
        font-size: 12px;
        outline: none;
    }

    /* Left panel */
    .properties-panel {
        width: 280px;
        min-width: 280px;
        border-right: 1px solid var(--ed-line);
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .panel-toolbar {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 6px 8px;
        border-bottom: 1px solid var(--ed-line);
        background: var(--ed-mantle);
    }

    .toolbar-spacer {
        flex: 1;
    }

    .panel-scroll {
        flex: 1;
        overflow-y: auto;
        padding-bottom: 16px;
    }

    .section-toggle {
        display: block;
        width: 100%;
        text-align: left;
        padding: 6px 8px;
        background: var(--ed-mantle);
        border: none;
        border-bottom: 1px solid var(--ed-line);
        color: var(--ed-text);
        cursor: pointer;
        font-size: 11px;
        font-weight: 600;
    }

    .section-toggle:hover {
        background: var(--ed-hover);
    }

    .section {
        padding: 8px;
        border-bottom: 1px solid var(--ed-line);
    }

    .section label {
        display: flex;
        flex-direction: column;
        gap: 2px;
        margin-bottom: 6px;
        font-size: 11px;
        color: var(--ed-muted);
    }

    .section input[type="number"],
    .section input[type="text"],
    .section select {
        width: 100%;
        padding: 4px 6px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 12px;
        box-sizing: border-box;
    }

    .row-2 {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 6px;
    }

    .char-preview {
        font-size: 10px;
        color: var(--ed-muted);
        font-family: monospace;
    }

    .action-btn {
        width: 100%;
        padding: 6px 8px;
        background: var(--ed-mantle);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        cursor: pointer;
        font-size: 11.5px;
        margin-top: 4px;
    }

    .action-btn:hover {
        background: var(--ed-hover);
    }

    /* Glyph list */
    .glyph-list {
        display: flex;
        flex-direction: column;
        gap: 1px;
        max-height: 280px;
        overflow-y: auto;
        margin-top: 6px;
    }

    .glyph-item {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 3px 6px;
        border-radius: 3px;
        cursor: pointer;
        background: var(--ed-raised);
        border: 1px solid transparent;
        font-size: 11px;
    }

    .glyph-item:hover {
        background: var(--ed-hover);
    }

    .glyph-item.selected {
        border-color: var(--ed-accent);
        background: color-mix(in srgb, var(--ed-accent) 10%, transparent);
    }

    .glyph-char {
        font-weight: 700;
        font-size: 14px;
        min-width: 18px;
        text-align: center;
    }

    .glyph-code {
        color: var(--ed-muted);
        font-family: monospace;
        font-size: 10px;
        min-width: 32px;
    }

    .glyph-dims {
        flex: 1;
        color: var(--ed-muted);
        font-family: monospace;
        font-size: 10px;
    }

    .glyph-btn {
        padding: 3px 4px;
        font-size: 11.5px;
        border: none;
        border-radius: 4px;
        background: transparent;
        color: var(--ed-muted);
        cursor: pointer;
    }

    .glyph-btn.danger:hover {
        color: var(--ed-err);
    }

    /* Center area */
    .center-area {
        flex: 1;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .center-toolbar {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 6px 8px;
        border-bottom: 1px solid var(--ed-line);
        background: var(--ed-mantle);
    }

    .file-name {
        font-size: 12px;
        font-weight: 600;
    }

    .info-badge {
        padding: 2px 8px;
        border-radius: 8px;
        background: var(--ed-raised);
        font-size: 11px;
        font-family: monospace;
        color: var(--ed-muted);
    }

    .atlas-container {
        flex: 1;
        position: relative;
        overflow: hidden;
        min-height: 120px;
    }

    .atlas-container canvas {
        position: absolute;
        inset: 0;
        width: 100%;
        height: 100%;
    }

    /* Preview */
    .preview-toggle {
        flex-shrink: 0;
    }

    .preview-section {
        border-top: 1px solid var(--ed-line);
        flex-shrink: 0;
    }

    .preview-controls {
        display: flex;
        gap: 8px;
        padding: 6px 8px;
        align-items: center;
    }

    .preview-input {
        flex: 1;
        padding: 4px 6px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 12px;
    }

    .scale-label {
        display: flex;
        align-items: center;
        gap: 4px;
        font-size: 11px;
        color: var(--ed-muted);
        white-space: nowrap;
    }

    .scale-label input[type="range"] {
        width: 60px;
    }

    .preview-canvas-container {
        height: 80px;
        position: relative;
        overflow: hidden;
    }

    .preview-canvas-container canvas {
        position: absolute;
        inset: 0;
        width: 100%;
        height: 100%;
    }

    /* Shared buttons */
    .tool-btn {
        padding: 4px 8px;
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        background: var(--ed-mantle);
        color: var(--ed-text);
        cursor: pointer;
        font-size: 12px;
        line-height: 1;
    }

    .tool-btn:hover {
        background: var(--ed-hover);
    }

    .tool-btn:disabled {
        opacity: 0.4;
        cursor: default;
    }
</style>
