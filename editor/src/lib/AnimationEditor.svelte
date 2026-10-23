<script lang="ts">
    import { themeColors, withAlpha } from "./stores/themeColors";
    import { requestEditorClose } from "./stores/editorClose";
    import { onMount, onDestroy } from "svelte";
    import { get } from "svelte/store";
    import {
        animClip,
        animData,
        animFilePath,
        animDirty,
        selectedFrameIndex,
        selectedClipName,
        clipNames,
        canUndoAnim,
        canRedoAnim,
        undoAnim,
        redoAnim,
        executeAnim,
        executeAnimData,
        saveCurrentAnim,
        closeAnimEditor,
        selectClip,
        addClip,
        removeClip,
        renameClip,
        duplicateClip,
        linkedEntityIndex,
        linkSelectedEntity,
        unlinkEntity,
        loadFromEntity,
    } from "./stores/animationEditorStore";
    import type {
        AnimationClipData,
        AnimatableProperty,
        PropertyTrackData,
    } from "./api/animation";
    import { listProjectImages, readAssetBinary } from "./api/project";
    import { project } from "./stores/projectStore";
    import { selectedEntity } from "./stores/selectionStore";
    import init, {
        viewport_anim_preview_init,
        viewport_anim_preview_start,
        viewport_anim_preview_update,
        viewport_anim_preview_render,
        viewport_anim_preview_stop,
        viewport_anim_preview_set_frame,
        viewport_anim_preview_set_texture,
        viewport_anim_preview_set_atlas,
        viewport_anim_preview_resize,
        viewport_anim_preview_current_frame,
        viewport_anim_preview_is_playing,
        viewport_anim_preview_play,
        viewport_anim_preview_pause,
    } from "./viewport-wasm/jengine_editor_viewport.js";

    // ---------------------------------------------------------------------------
    // State
    // ---------------------------------------------------------------------------
    let canvasEl: HTMLCanvasElement | undefined = $state();
    let wasmReady = $state(false);
    let playing = $state(true);
    let animFrameId = 0;
    let lastTime = 0;
    let resizeObserver: ResizeObserver | undefined;

    // Collapsible sections
    let showClipSettings = $state(true);
    let showFrames = $state(true);
    let showSelectedFrame = $state(true);
    let showTexture = $state(true);
    let showPropertyTracks = $state(true);

    // Texture picker state
    let projectImages = $state<string[]>([]);
    let textureId = $state<string>("");
    let loadedTextureId = $state<string | null>(null);

    // Clip management state
    let showClipList = $state(true);
    let newClipName = $state("");
    let renamingClip = $state<string | null>(null);
    let renameValue = $state("");

    // Trigger add state
    let newTriggerType = $state<"PlaySound" | "EmitEvent">("PlaySound");
    let newTriggerValue = $state("");

    // Property track state
    let newTrackProperty = $state<AnimatableProperty>("PositionX");
    let selectedTrackIndex = $state<number | null>(null);
    const allAnimProperties: AnimatableProperty[] = [
        // Transform
        "PositionX",
        "PositionY",
        "Rotation",
        "ScaleX",
        "ScaleY",
        // Sprite color
        "ColorR",
        "ColorG",
        "ColorB",
        "ColorA",
        // UI Element
        "UiOffsetX",
        "UiOffsetY",
        "UiSizeX",
        "UiSizeY",
        // UI Text
        "UiTextFontSize",
        "UiTextColorR",
        "UiTextColorG",
        "UiTextColorB",
        "UiTextColorA",
        // UI Image
        "UiImageColorR",
        "UiImageColorG",
        "UiImageColorB",
        "UiImageColorA",
        // UI Panel
        "UiPanelColorR",
        "UiPanelColorG",
        "UiPanelColorB",
        "UiPanelColorA",
        // UI Button
        "UiButtonColorR",
        "UiButtonColorG",
        "UiButtonColorB",
        "UiButtonColorA",
        // UI Progress Bar
        "UiProgressValue",
        "UiProgressFgR",
        "UiProgressFgG",
        "UiProgressFgB",
        "UiProgressFgA",
        // UI Slider
        "UiSliderValue",
    ];

    const usedProperties = $derived(
        new Set(($animClip?.property_tracks ?? []).map((t) => t.property)),
    );
    function availProps(props: AnimatableProperty[]): AnimatableProperty[] {
        return props.filter((p) => !usedProperties.has(p));
    }

    const fileName = $derived(
        $animFilePath?.split(/[\\/]/).pop()?.replace(".anim", "") ??
            "Animation",
    );

    const frameCount = $derived($animClip?.frames.length ?? 0);
    const currentWasmFrame = $derived(
        wasmReady ? viewport_anim_preview_current_frame() : 0,
    );

    // ---------------------------------------------------------------------------
    // WASM preview loop
    // ---------------------------------------------------------------------------
    function startLoop() {
        lastTime = performance.now();
        function frame(now: number) {
            const dt = Math.min((now - lastTime) / 1000, 0.05);
            lastTime = now;
            // Always call update — the WASM state.playing flag controls
            // whether frames actually advance. This avoids issues with
            // Svelte 5 $state reads inside rAF closures.
            viewport_anim_preview_update(dt);
            viewport_anim_preview_render();

            // Sync WASM current frame → UI frame counter during playback
            if (viewport_anim_preview_is_playing()) {
                const wf = viewport_anim_preview_current_frame();
                if (wf !== get(selectedFrameIndex)) {
                    selectedFrameIndex.set(wf);
                }
            }

            animFrameId = requestAnimationFrame(frame);
        }
        animFrameId = requestAnimationFrame(frame);
    }

    function pushClipToWasm() {
        if (!wasmReady || !$animClip) return;
        try {
            viewport_anim_preview_start(JSON.stringify($animClip));
            if (!playing) {
                viewport_anim_preview_pause();
                viewport_anim_preview_set_frame(get(selectedFrameIndex));
            }
        } catch (e) {
            console.error("Failed to push clip to WASM:", e);
        }
    }

    // ---------------------------------------------------------------------------
    // Lifecycle
    // ---------------------------------------------------------------------------
    onMount(async () => {
        if (!canvasEl) return;
        await init();
        const parent = canvasEl.parentElement!;
        canvasEl.width = parent.clientWidth;
        canvasEl.height = parent.clientHeight;
        viewport_anim_preview_init("anim-preview-canvas");
        wasmReady = true;
        pushClipToWasm();
        startLoop();

        // Observe canvas container resize
        resizeObserver = new ResizeObserver((entries) => {
            for (const entry of entries) {
                const { width, height } = entry.contentRect;
                const w = Math.round(width);
                const h = Math.round(height);
                if (canvasEl && w > 0 && h > 0) {
                    canvasEl.width = w;
                    canvasEl.height = h;
                    viewport_anim_preview_resize(w, h);
                }
            }
        });
        resizeObserver.observe(parent);

        // Load project images list
        const proj = get(project);
        if (proj) {
            projectImages = await listProjectImages(proj.path);
        }
    });

    onDestroy(() => {
        if (animFrameId) cancelAnimationFrame(animFrameId);
        resizeObserver?.disconnect();
        if (wasmReady) viewport_anim_preview_stop();
    });

    // Push clip to WASM whenever it changes
    let lastClipJson = "";
    $effect(() => {
        if (!$animClip || !wasmReady) return;
        const json = JSON.stringify($animClip);
        if (json !== lastClipJson) {
            lastClipJson = json;
            pushClipToWasm();
        }
    });

    // Load texture when textureId changes
    $effect(() => {
        const id = textureId;
        if (!id || !wasmReady) {
            loadedTextureId = null;
            return;
        }
        if (id === loadedTextureId) return;
        const proj = get(project);
        if (!proj) return;
        (async () => {
            try {
                const bytes = await readAssetBinary(proj.path, id);
                viewport_anim_preview_set_texture(new Uint8Array(bytes));
                loadedTextureId = id;
                // A frame may name an atlas region instead of carrying a uv
                // rect, and the preview can only resolve one against the
                // packed metadata beside the sheet (`creatures.png` ->
                // `creatures.atlas`). Most sheets have none, so a miss here
                // is the normal case and not worth logging.
                const atlasPath = id.replace(/\.[^./]+$/, "") + ".atlas";
                try {
                    const atlasBytes = await readAssetBinary(
                        proj.path,
                        atlasPath,
                    );
                    viewport_anim_preview_set_atlas(
                        new TextDecoder().decode(new Uint8Array(atlasBytes)),
                    );
                } catch {
                    // No sibling atlas: frames must carry their own uv rects.
                }
            } catch (e) {
                console.error("Failed to load texture:", e);
            }
        })();
    });

    // Sync textureId with animData.texture_id on load
    $effect(() => {
        const tid = $animData?.texture_id;
        if (tid !== undefined && tid !== null) {
            textureId = tid;
        }
    });

    // ---------------------------------------------------------------------------
    // Actions
    // ---------------------------------------------------------------------------
    /** A frame's rect, for display and editing.
     *
     *  A frame that names an atlas `region` carries no rect until the engine
     *  resolves it against the atlas, so show the whole texture rather than
     *  reading `undefined`.
     */
    function frameUv(f: {
        uv_rect?: [number, number, number, number];
    }): [number, number, number, number] {
        return f.uv_rect ?? [0, 0, 1, 1];
    }

    /** Typing a coordinate makes the frame explicit: a `region` would be
     *  resolved over it at load, so the two cannot both hold. */
    function setFrameUv(
        clip: {
            frames: {
                uv_rect?: [number, number, number, number];
                region?: string;
            }[];
        },
        idx: number,
        axis: number,
        value: number,
    ) {
        const frame = clip.frames[idx];
        if (!frame) return;
        const uv = frameUv(frame);
        uv[axis] = value;
        frame.uv_rect = uv;
        delete frame.region;
    }

    function togglePlay() {
        playing = !playing;
        if (wasmReady) {
            if (playing) viewport_anim_preview_play();
            else viewport_anim_preview_pause();
        }
    }

    function stepForward() {
        if (!$animClip || $animClip.frames.length === 0) return;
        playing = false;
        if (wasmReady) viewport_anim_preview_pause();
        const next = ($selectedFrameIndex + 1) % $animClip.frames.length;
        selectedFrameIndex.set(next);
        if (wasmReady) viewport_anim_preview_set_frame(next);
    }

    function stepBackward() {
        if (!$animClip || $animClip.frames.length === 0) return;
        playing = false;
        if (wasmReady) viewport_anim_preview_pause();
        const prev =
            ($selectedFrameIndex - 1 + $animClip.frames.length) %
            $animClip.frames.length;
        selectedFrameIndex.set(prev);
        if (wasmReady) viewport_anim_preview_set_frame(prev);
    }

    function restart() {
        selectedFrameIndex.set(0);
        if (wasmReady) {
            viewport_anim_preview_set_frame(0);
            viewport_anim_preview_play();
        }
        playing = true;
    }

    function addFrame() {
        executeAnim("Add frame", (clip) => {
            clip.frames.push({ uv_rect: [0, 0, 1, 1] });
        });
        // Select the new frame
        if ($animClip) selectedFrameIndex.set($animClip.frames.length - 1);
    }

    function removeFrame(idx: number) {
        if (!$animClip || $animClip.frames.length <= 1) return;
        executeAnim("Remove frame", (clip) => {
            clip.frames.splice(idx, 1);
        });
        if ($selectedFrameIndex >= ($animClip?.frames.length ?? 1)) {
            selectedFrameIndex.set(
                Math.max(0, ($animClip?.frames.length ?? 1) - 1),
            );
        }
    }

    function duplicateFrame(idx: number) {
        if (!$animClip) return;
        executeAnim("Duplicate frame", (clip) => {
            const copy = structuredClone(clip.frames[idx]);
            clip.frames.splice(idx + 1, 0, copy);
        });
        selectedFrameIndex.set(idx + 1);
    }

    function moveFrameUp(idx: number) {
        if (!$animClip || idx <= 0) return;
        executeAnim("Move frame up", (clip) => {
            [clip.frames[idx - 1], clip.frames[idx]] = [
                clip.frames[idx],
                clip.frames[idx - 1],
            ];
        });
        selectedFrameIndex.set(idx - 1);
    }

    function moveFrameDown(idx: number) {
        if (!$animClip || idx >= $animClip.frames.length - 1) return;
        executeAnim("Move frame down", (clip) => {
            [clip.frames[idx], clip.frames[idx + 1]] = [
                clip.frames[idx + 1],
                clip.frames[idx],
            ];
        });
        selectedFrameIndex.set(idx + 1);
    }

    function selectFrame(idx: number) {
        selectedFrameIndex.set(idx);
        if (wasmReady && !playing) {
            viewport_anim_preview_set_frame(idx);
        }
    }

    // --- Generate Grid Frames inline form state ---
    let showGridForm = $state(false);
    let gridCols = $state(4);
    let gridRows = $state(4);
    let gridTotalFrames = $state(16);

    function openGridForm() {
        gridCols = 4;
        gridRows = 4;
        gridTotalFrames = 16;
        showGridForm = true;
    }

    function applyGridFrames() {
        const cols = gridCols;
        const rows = gridRows;
        if (cols < 1 || rows < 1) return;
        const total = cols * rows;
        const count = Math.min(Math.max(1, gridTotalFrames), total);
        executeAnim("Generate grid", (clip) => {
            clip.frames = [];
            const fw = 1 / cols;
            const fh = 1 / rows;
            for (let i = 0; i < count; i++) {
                const col = i % cols;
                const row = Math.floor(i / cols);
                clip.frames.push({
                    uv_rect: [
                        col * fw,
                        row * fh,
                        (col + 1) * fw,
                        (row + 1) * fh,
                    ],
                });
            }
        });
        selectedFrameIndex.set(0);
        showGridForm = false;
    }

    // ---------------------------------------------------------------------------
    // Spritesheet visual frame picker
    // ---------------------------------------------------------------------------
    let showSpritesheetPicker = $state(false);
    let sheetCanvas: HTMLCanvasElement | undefined = $state();
    let sheetImage: HTMLImageElement | null = $state(null);
    let sheetCols = $state(4);
    let sheetRows = $state(4);
    let selectedCells = $state<Set<number>>(new Set());
    let sheetHoverCell = $state<number | null>(null);

    /** Load spritesheet image from project assets for the picker. */
    async function loadSheetImage() {
        if (!textureId) return;
        const proj = get(project);
        if (!proj) return;
        try {
            const bytes = await readAssetBinary(proj.path, textureId);
            const blob = new Blob([new Uint8Array(bytes)]);
            const url = URL.createObjectURL(blob);
            const img = new Image();
            img.onload = () => {
                sheetImage = img;
                drawSheetCanvas();
            };
            img.src = url;
        } catch (e) {
            console.error("Failed to load sheet image:", e);
        }
    }

    // The sheet is drawn in the palette's colours; redraw it when they change.
    $effect(() => {
        void $themeColors;
        drawSheetCanvas();
    });

    function drawSheetCanvas() {
        if (!sheetCanvas || !sheetImage) return;
        const ctx = sheetCanvas.getContext("2d");
        if (!ctx) return;

        const w = sheetCanvas.width;
        const h = sheetCanvas.height;

        // Draw checkerboard background
        const checkSize = 8;
        for (let y = 0; y < h; y += checkSize) {
            for (let x = 0; x < w; x += checkSize) {
                ctx.fillStyle =
                    ((x + y) / checkSize) % 2 === 0 ? $themeColors.hover : $themeColors.base;
                ctx.fillRect(x, y, checkSize, checkSize);
            }
        }

        // Draw image scaled to fit
        const scale = Math.min(w / sheetImage.width, h / sheetImage.height);
        const iw = sheetImage.width * scale;
        const ih = sheetImage.height * scale;
        const ox = (w - iw) / 2;
        const oy = (h - ih) / 2;
        ctx.drawImage(sheetImage, ox, oy, iw, ih);

        // Draw grid
        const cellW = iw / sheetCols;
        const cellH = ih / sheetRows;

        // Highlight selected cells
        for (const idx of selectedCells) {
            const col = idx % sheetCols;
            const row = Math.floor(idx / sheetCols);
            ctx.fillStyle = withAlpha($themeColors.accent, 0.3);
            ctx.fillRect(ox + col * cellW, oy + row * cellH, cellW, cellH);
            // Frame number
            ctx.fillStyle = $themeColors.accent;
            ctx.font = "bold 11px system-ui";
            ctx.textAlign = "center";
            ctx.textBaseline = "middle";
            ctx.fillText(
                String(idx),
                ox + col * cellW + cellW / 2,
                oy + row * cellH + cellH / 2,
            );
        }

        // Highlight hover cell
        if (sheetHoverCell !== null && !selectedCells.has(sheetHoverCell)) {
            const col = sheetHoverCell % sheetCols;
            const row = Math.floor(sheetHoverCell / sheetCols);
            ctx.fillStyle = withAlpha($themeColors.accent, 0.15);
            ctx.fillRect(ox + col * cellW, oy + row * cellH, cellW, cellH);
        }

        // Grid lines
        ctx.strokeStyle = withAlpha($themeColors.accent, 0.4);
        ctx.lineWidth = 1;
        for (let c = 0; c <= sheetCols; c++) {
            const x = ox + c * cellW;
            ctx.beginPath();
            ctx.moveTo(x, oy);
            ctx.lineTo(x, oy + ih);
            ctx.stroke();
        }
        for (let r = 0; r <= sheetRows; r++) {
            const y = oy + r * cellH;
            ctx.beginPath();
            ctx.moveTo(ox, y);
            ctx.lineTo(ox + iw, y);
            ctx.stroke();
        }
    }

    function getCellFromMouse(e: MouseEvent): number | null {
        if (!sheetCanvas || !sheetImage) return null;
        const rect = sheetCanvas.getBoundingClientRect();
        const mx = e.clientX - rect.left;
        const my = e.clientY - rect.top;
        const w = sheetCanvas.width;
        const h = sheetCanvas.height;
        const scale = Math.min(w / sheetImage.width, h / sheetImage.height);
        const iw = sheetImage.width * scale;
        const ih = sheetImage.height * scale;
        const ox = (w - iw) / 2;
        const oy = (h - ih) / 2;
        if (mx < ox || mx > ox + iw || my < oy || my > oy + ih) return null;
        const col = Math.floor((mx - ox) / (iw / sheetCols));
        const row = Math.floor((my - oy) / (ih / sheetRows));
        if (col < 0 || col >= sheetCols || row < 0 || row >= sheetRows)
            return null;
        return row * sheetCols + col;
    }

    function onSheetClick(e: MouseEvent) {
        const cell = getCellFromMouse(e);
        if (cell === null) return;
        const next = new Set(selectedCells);
        if (e.shiftKey) {
            // Range select: from lowest selected to clicked cell
            const min = Math.min(...selectedCells, cell);
            const max = Math.max(...selectedCells, cell);
            for (let i = min; i <= max; i++) next.add(i);
        } else if (e.ctrlKey) {
            // Toggle single cell
            if (next.has(cell)) next.delete(cell);
            else next.add(cell);
        } else {
            // Single select
            if (next.has(cell) && next.size === 1) next.clear();
            else {
                next.clear();
                next.add(cell);
            }
        }
        selectedCells = next;
        drawSheetCanvas();
    }

    function onSheetMouseMove(e: MouseEvent) {
        const cell = getCellFromMouse(e);
        if (cell !== sheetHoverCell) {
            sheetHoverCell = cell;
            drawSheetCanvas();
        }
    }

    function addSelectedCellsToClip() {
        if (selectedCells.size === 0) return;
        const sorted = [...selectedCells].sort((a, b) => a - b);
        executeAnim("Add spritesheet frames", (clip) => {
            const fw = 1 / sheetCols;
            const fh = 1 / sheetRows;
            for (const idx of sorted) {
                const col = idx % sheetCols;
                const row = Math.floor(idx / sheetCols);
                clip.frames.push({
                    uv_rect: [
                        col * fw,
                        row * fh,
                        (col + 1) * fw,
                        (row + 1) * fh,
                    ],
                });
            }
        });
        selectedCells = new Set();
        drawSheetCanvas();
    }

    function replaceClipWithSelectedCells() {
        if (selectedCells.size === 0) return;
        const sorted = [...selectedCells].sort((a, b) => a - b);
        executeAnim("Replace with spritesheet frames", (clip) => {
            clip.frames = [];
            const fw = 1 / sheetCols;
            const fh = 1 / sheetRows;
            for (const idx of sorted) {
                const col = idx % sheetCols;
                const row = Math.floor(idx / sheetCols);
                clip.frames.push({
                    uv_rect: [
                        col * fw,
                        row * fh,
                        (col + 1) * fw,
                        (row + 1) * fh,
                    ],
                });
            }
        });
        selectedFrameIndex.set(0);
        selectedCells = new Set();
        drawSheetCanvas();
    }

    // Redraw spritesheet canvas when grid params change
    $effect(() => {
        void sheetCols;
        void sheetRows;
        drawSheetCanvas();
    });

    // Load sheet image when picker opens or texture changes
    $effect(() => {
        if (showSpritesheetPicker && textureId) {
            loadSheetImage();
        }
    });

    // ---------------------------------------------------------------------------
    // Generate clips from grid (row→clip mapping)
    // ---------------------------------------------------------------------------
    let showGridClipsForm = $state(false);
    let gridClipsCols = $state(4);
    let gridClipsRows = $state(4);
    let gridClipsNames = $state<string[]>([]);

    function openGridClipsForm() {
        gridClipsCols = sheetCols || 4;
        gridClipsRows = sheetRows || 4;
        gridClipsNames = Array.from(
            { length: gridClipsRows },
            (_, i) => `row_${i}`,
        );
        showGridClipsForm = true;
    }

    function applyGridClips() {
        if (gridClipsCols < 1 || gridClipsRows < 1) return;
        const cols = gridClipsCols;
        const rows = gridClipsRows;
        const fw = 1 / cols;
        const fh = 1 / rows;
        executeAnimData("Generate clips from grid", (data) => {
            for (let r = 0; r < rows; r++) {
                const clipName = gridClipsNames[r]?.trim() || `row_${r}`;
                const frames = [];
                for (let c = 0; c < cols; c++) {
                    frames.push({
                        uv_rect: [
                            c * fw,
                            r * fh,
                            (c + 1) * fw,
                            (r + 1) * fh,
                        ] as [number, number, number, number],
                    });
                }
                data.clips[clipName] = {
                    name: clipName,
                    frame_duration: 0.1,
                    looping: true,
                    frames,
                };
            }
        });
        const first = gridClipsNames[0]?.trim() || "row_0";
        selectedClipName.set(first);
        selectedFrameIndex.set(0);
        showGridClipsForm = false;
    }

    // ---------------------------------------------------------------------------
    // Property Track actions
    // ---------------------------------------------------------------------------
    function addPropertyTrack() {
        if (!$animClip) return;
        const prop = newTrackProperty;
        // Don't add duplicate
        const existing = $animClip.property_tracks ?? [];
        if (existing.some((t) => t.property === prop)) return;
        executeAnim("Add property track", (clip) => {
            if (!clip.property_tracks) clip.property_tracks = [];
            clip.property_tracks.push({ property: prop, keyframes: [] });
        });
        selectedTrackIndex = ($animClip?.property_tracks?.length ?? 1) - 1;
    }

    function removePropertyTrack(idx: number) {
        executeAnim("Remove property track", (clip) => {
            if (clip.property_tracks) clip.property_tracks.splice(idx, 1);
        });
        if (selectedTrackIndex === idx) selectedTrackIndex = null;
        else if (selectedTrackIndex !== null && selectedTrackIndex > idx)
            selectedTrackIndex--;
    }

    function addKeyframeToTrack(trackIdx: number) {
        const frame = $selectedFrameIndex;
        executeAnim("Add keyframe", (clip) => {
            const track = clip.property_tracks?.[trackIdx];
            if (!track) return;
            // Remove existing keyframe at this frame
            track.keyframes = track.keyframes.filter((k) => k.frame !== frame);
            // Insert sorted
            const kf = { frame, value: 0 };
            const insertAt = track.keyframes.findIndex((k) => k.frame > frame);
            if (insertAt === -1) track.keyframes.push(kf);
            else track.keyframes.splice(insertAt, 0, kf);
        });
    }

    function removeKeyframe(trackIdx: number, kfIdx: number) {
        executeAnim("Remove keyframe", (clip) => {
            clip.property_tracks?.[trackIdx]?.keyframes.splice(kfIdx, 1);
        });
    }

    function setKeyframeValue(trackIdx: number, kfIdx: number, value: number) {
        executeAnim("Set keyframe value", (clip) => {
            const kf = clip.property_tracks?.[trackIdx]?.keyframes[kfIdx];
            if (kf) kf.value = value;
        });
    }

    function setKeyframeFrame(trackIdx: number, kfIdx: number, frame: number) {
        executeAnim("Set keyframe frame", (clip) => {
            const kf = clip.property_tracks?.[trackIdx]?.keyframes[kfIdx];
            if (kf) kf.frame = Math.max(0, frame);
        });
    }

    /** Get a short label for a property name. */
    function propLabel(p: AnimatableProperty): string {
        const labels: Record<AnimatableProperty, string> = {
            PositionX: "Pos X",
            PositionY: "Pos Y",
            Rotation: "Rot",
            ScaleX: "Scale X",
            ScaleY: "Scale Y",
            ColorR: "R",
            ColorG: "G",
            ColorB: "B",
            ColorA: "A",
            UiOffsetX: "UI Offset X",
            UiOffsetY: "UI Offset Y",
            UiSizeX: "UI Size X",
            UiSizeY: "UI Size Y",
            UiTextFontSize: "Text Font Size",
            UiTextColorR: "Text R",
            UiTextColorG: "Text G",
            UiTextColorB: "Text B",
            UiTextColorA: "Text A",
            UiImageColorR: "Img R",
            UiImageColorG: "Img G",
            UiImageColorB: "Img B",
            UiImageColorA: "Img A",
            UiPanelColorR: "Panel R",
            UiPanelColorG: "Panel G",
            UiPanelColorB: "Panel B",
            UiPanelColorA: "Panel A",
            UiButtonColorR: "Btn R",
            UiButtonColorG: "Btn G",
            UiButtonColorB: "Btn B",
            UiButtonColorA: "Btn A",
            UiProgressValue: "Progress Val",
            UiProgressFgR: "Progress R",
            UiProgressFgG: "Progress G",
            UiProgressFgB: "Progress B",
            UiProgressFgA: "Progress A",
            UiSliderValue: "Slider Val",
        };
        return labels[p] ?? p;
    }

    // ---------------------------------------------------------------------------
    // Entity linking
    // ---------------------------------------------------------------------------
    const canLinkEntity = $derived($selectedEntity >= 0 && $animData !== null);

    function handleKeydown(e: KeyboardEvent) {
        if (e.ctrlKey && e.key === "z" && !e.shiftKey) {
            e.preventDefault();
            e.stopPropagation();
            undoAnim();
        } else if (
            e.ctrlKey &&
            (e.key === "y" || (e.key === "z" && e.shiftKey))
        ) {
            e.preventDefault();
            e.stopPropagation();
            redoAnim();
        } else if (e.key === " ") {
            e.preventDefault();
            togglePlay();
        } else if (e.key === "ArrowRight") {
            e.preventDefault();
            stepForward();
        } else if (e.key === "ArrowLeft") {
            e.preventDefault();
            stepBackward();
        }
    }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="animation-editor" onkeydown={handleKeydown} tabindex="-1">
    <!-- Left panel: properties -->
    <div class="properties-panel">
        <!-- Toolbar -->
        <div class="panel-toolbar">
            <button
                class="tool-btn"
                title="Undo (Ctrl+Z)"
                disabled={!$canUndoAnim}
                onclick={undoAnim}>↩</button
            >
            <button
                class="tool-btn"
                title="Redo (Ctrl+Y)"
                disabled={!$canRedoAnim}
                onclick={redoAnim}>↪</button
            >
            <div class="toolbar-spacer"></div>
            {#if $linkedEntityIndex >= 0}
                <button
                    class="tool-btn linked"
                    title="Linked to entity #{$linkedEntityIndex} — click to unlink"
                    onclick={unlinkEntity}>🔗</button
                >
            {:else}
                <button
                    class="tool-btn"
                    title="Link to selected entity (push on save)"
                    disabled={!canLinkEntity}
                    onclick={linkSelectedEntity}>🔗</button
                >
            {/if}
            <button
                class="tool-btn"
                title="Load clips from selected entity"
                disabled={$selectedEntity < 0}
                onclick={loadFromEntity}>📥</button
            >
            <button
                class="tool-btn"
                title="Save (Ctrl+S)"
                onclick={saveCurrentAnim}>💾</button
            >
            <button class="tool-btn" title="Close" onclick={() => requestEditorClose("animation")}
                >✕</button
            >
        </div>

        <div class="panel-scroll">
            <!-- Clip List -->
            <button
                class="section-toggle"
                onclick={() => (showClipList = !showClipList)}
            >
                {showClipList ? "▼" : "▶"} Clips ({$clipNames.length})
            </button>
            {#if showClipList && $animData}
                <div class="section">
                    <div class="clip-list">
                        {#each $clipNames as name}
                            <!-- svelte-ignore a11y_click_events_have_key_events -->
                            <div
                                class="clip-item"
                                class:selected={$selectedClipName === name}
                                onclick={() => selectClip(name)}
                                role="button"
                                tabindex="0"
                            >
                                {#if renamingClip === name}
                                    <input
                                        type="text"
                                        class="clip-rename-input"
                                        bind:value={renameValue}
                                        onkeydown={(e) => {
                                            if (e.key === "Enter") {
                                                renameClip(
                                                    name,
                                                    renameValue.trim(),
                                                );
                                                renamingClip = null;
                                            } else if (e.key === "Escape") {
                                                renamingClip = null;
                                            }
                                        }}
                                        onblur={() => {
                                            if (
                                                renameValue.trim() &&
                                                renameValue.trim() !== name
                                            ) {
                                                renameClip(
                                                    name,
                                                    renameValue.trim(),
                                                );
                                            }
                                            renamingClip = null;
                                        }}
                                        onclick={(e) => e.stopPropagation()}
                                    />
                                {:else}
                                    <span
                                        class="clip-name"
                                        ondblclick={(e) => {
                                            e.stopPropagation();
                                            renamingClip = name;
                                            renameValue = name;
                                        }}>{name}</span
                                    >
                                {/if}
                                <div class="frame-btns">
                                    <button
                                        class="frame-btn"
                                        title="Duplicate clip"
                                        onclick={(e) => {
                                            e.stopPropagation();
                                            duplicateClip(name);
                                        }}>⧉</button
                                    >
                                    <button
                                        class="frame-btn danger"
                                        title="Remove clip"
                                        disabled={$clipNames.length <= 1}
                                        onclick={(e) => {
                                            e.stopPropagation();
                                            removeClip(name);
                                        }}>✕</button
                                    >
                                </div>
                            </div>
                        {/each}
                    </div>
                    <div class="clip-add-row">
                        <input
                            type="text"
                            placeholder="New clip name..."
                            bind:value={newClipName}
                            onkeydown={(e) => {
                                if (e.key === "Enter" && newClipName.trim()) {
                                    addClip(newClipName.trim());
                                    newClipName = "";
                                }
                            }}
                        />
                        <button
                            class="frame-btn"
                            title="Add clip"
                            disabled={!newClipName.trim() ||
                                $clipNames.includes(newClipName.trim())}
                            onclick={() => {
                                if (newClipName.trim()) {
                                    addClip(newClipName.trim());
                                    newClipName = "";
                                }
                            }}>+</button
                        >
                    </div>
                </div>
            {/if}

            <!-- Clip Settings -->
            <button
                class="section-toggle"
                onclick={() => (showClipSettings = !showClipSettings)}
            >
                {showClipSettings ? "▼" : "▶"} Clip Settings
            </button>
            {#if showClipSettings && $animClip}
                <div class="section">
                    <label>
                        Name
                        <input
                            type="text"
                            value={$animClip.name}
                            onchange={(e) => {
                                const newName = (
                                    e.target as HTMLInputElement
                                ).value.trim();
                                const oldName = $selectedClipName;
                                if (newName && oldName && newName !== oldName) {
                                    renameClip(oldName, newName);
                                }
                            }}
                        />
                    </label>
                    <label>
                        Frame Duration (s)
                        <input
                            type="number"
                            min="0.001"
                            max="10"
                            step="0.01"
                            value={$animClip.frame_duration}
                            onchange={(e) =>
                                executeAnim("Set frame duration", (c) => {
                                    c.frame_duration =
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0.1;
                                })}
                        />
                    </label>
                    <label class="checkbox-label">
                        <input
                            type="checkbox"
                            checked={$animClip.looping}
                            onchange={(e) =>
                                executeAnim("Toggle looping", (c) => {
                                    c.looping = (
                                        e.target as HTMLInputElement
                                    ).checked;
                                })}
                        />
                        Looping
                    </label>
                    <div class="info-row">
                        FPS: {(1 / ($animClip.frame_duration || 0.1)).toFixed(
                            1,
                        )} &nbsp;|&nbsp; Frames: {$animClip.frames.length} &nbsp;|&nbsp;
                        Duration: {(
                            $animClip.frames.length *
                                $animClip.frame_duration || 0
                        ).toFixed(2)}s
                    </div>
                </div>
            {/if}

            <!-- Texture -->
            <button
                class="section-toggle"
                onclick={() => (showTexture = !showTexture)}
            >
                {showTexture ? "▼" : "▶"} Texture
            </button>
            {#if showTexture}
                <div class="section">
                    <label>
                        Sprite Sheet
                        <select
                            value={textureId}
                            onchange={(e) => {
                                const val = (e.target as HTMLSelectElement)
                                    .value;
                                textureId = val;
                                executeAnimData("Set texture", (d) => {
                                    d.texture_id = val || undefined;
                                });
                            }}
                        >
                            <option value="">-- None --</option>
                            {#each projectImages as img}
                                <option value={img}>{img}</option>
                            {/each}
                        </select>
                    </label>
                    {#if !showGridForm}
                        <button class="action-btn" onclick={openGridForm}
                            >🔲 Generate Grid Frames</button
                        >
                    {:else}
                        <div class="grid-form">
                            <div class="grid-form-row">
                                <label
                                    >Cols <input
                                        type="number"
                                        min="1"
                                        max="64"
                                        bind:value={gridCols}
                                        onchange={() => {
                                            gridTotalFrames =
                                                gridCols * gridRows;
                                        }}
                                    /></label
                                >
                                <label
                                    >Rows <input
                                        type="number"
                                        min="1"
                                        max="64"
                                        bind:value={gridRows}
                                        onchange={() => {
                                            gridTotalFrames =
                                                gridCols * gridRows;
                                        }}
                                    /></label
                                >
                                <label
                                    >Frames <input
                                        type="number"
                                        min="1"
                                        max={gridCols * gridRows}
                                        bind:value={gridTotalFrames}
                                    /></label
                                >
                            </div>
                            <div class="grid-form-row">
                                <button
                                    class="action-btn"
                                    onclick={applyGridFrames}>✓ Generate</button
                                >
                                <button
                                    class="action-btn"
                                    onclick={() => (showGridForm = false)}
                                    >✕ Cancel</button
                                >
                            </div>
                        </div>
                    {/if}
                    {#if textureId}
                        <button
                            class="action-btn"
                            onclick={() => {
                                showSpritesheetPicker = !showSpritesheetPicker;
                                selectedCells = new Set();
                            }}
                        >
                            {showSpritesheetPicker
                                ? "✕ Close Spritesheet Picker"
                                : "🖼 Spritesheet Frame Picker"}
                        </button>
                    {/if}
                    {#if !showGridClipsForm}
                        <button class="action-btn" onclick={openGridClipsForm}
                            >📋 Generate Clips from Grid</button
                        >
                    {:else}
                        <div class="grid-form">
                            <div class="grid-form-row">
                                <label
                                    >Cols <input
                                        type="number"
                                        min="1"
                                        max="64"
                                        bind:value={gridClipsCols}
                                        onchange={() => {
                                            gridClipsNames = Array.from(
                                                { length: gridClipsRows },
                                                (_, i) =>
                                                    gridClipsNames[i] ||
                                                    `row_${i}`,
                                            );
                                        }}
                                    /></label
                                >
                                <label
                                    >Rows <input
                                        type="number"
                                        min="1"
                                        max="64"
                                        bind:value={gridClipsRows}
                                        onchange={() => {
                                            gridClipsNames = Array.from(
                                                { length: gridClipsRows },
                                                (_, i) =>
                                                    gridClipsNames[i] ||
                                                    `row_${i}`,
                                            );
                                        }}
                                    /></label
                                >
                            </div>
                            <div class="grid-clips-names">
                                {#each gridClipsNames as name, i}
                                    <div class="grid-form-row">
                                        <span
                                            class="dim"
                                            style="min-width: 40px"
                                            >Row {i}</span
                                        >
                                        <input
                                            type="text"
                                            value={name}
                                            onchange={(e) => {
                                                gridClipsNames[i] = (
                                                    e.target as HTMLInputElement
                                                ).value;
                                            }}
                                            style="flex: 1; padding: 3px 4px; background: var(--ed-raised); border: 1px solid var(--ed-line); border-radius: 3px; color: var(--ed-text); font-size: 11px;"
                                        />
                                    </div>
                                {/each}
                            </div>
                            <div class="grid-form-row">
                                <button
                                    class="action-btn"
                                    onclick={applyGridClips}
                                    >✓ Generate Clips</button
                                >
                                <button
                                    class="action-btn"
                                    onclick={() => (showGridClipsForm = false)}
                                    >✕ Cancel</button
                                >
                            </div>
                        </div>
                    {/if}
                </div>
            {/if}

            <!-- Frames List -->
            <button
                class="section-toggle"
                onclick={() => (showFrames = !showFrames)}
            >
                {showFrames ? "▼" : "▶"} Frames ({frameCount})
            </button>
            {#if showFrames && $animClip}
                <div class="section">
                    <div class="frame-actions">
                        <button class="action-btn" onclick={addFrame}
                            >+ Add Frame</button
                        >
                    </div>
                    <div class="frame-list">
                        {#each $animClip.frames as frame, i}
                            <!-- svelte-ignore a11y_click_events_have_key_events -->
                            <div
                                class="frame-item"
                                class:selected={$selectedFrameIndex === i}
                                onclick={() => selectFrame(i)}
                                role="button"
                                tabindex="0"
                            >
                                <span class="frame-index">#{i}</span>
                                <span class="frame-uv">
                                    {#if frame.region}
                                        {frame.region}
                                    {:else}
                                        [{frameUv(frame)
                                            .map((v) => v.toFixed(2))
                                            .join(", ")}]
                                    {/if}
                                </span>
                                <div class="frame-btns">
                                    <button
                                        class="frame-btn"
                                        title="Move up"
                                        disabled={i === 0}
                                        onclick={(e) => {
                                            e.stopPropagation();
                                            moveFrameUp(i);
                                        }}>▲</button
                                    >
                                    <button
                                        class="frame-btn"
                                        title="Move down"
                                        disabled={i ===
                                            $animClip!.frames.length - 1}
                                        onclick={(e) => {
                                            e.stopPropagation();
                                            moveFrameDown(i);
                                        }}>▼</button
                                    >
                                    <button
                                        class="frame-btn"
                                        title="Duplicate"
                                        onclick={(e) => {
                                            e.stopPropagation();
                                            duplicateFrame(i);
                                        }}>⧉</button
                                    >
                                    <button
                                        class="frame-btn danger"
                                        title="Remove"
                                        disabled={$animClip!.frames.length <= 1}
                                        onclick={(e) => {
                                            e.stopPropagation();
                                            removeFrame(i);
                                        }}>✕</button
                                    >
                                </div>
                            </div>
                        {/each}
                    </div>
                </div>
            {/if}

            <!-- Selected Frame Inspector -->
            <button
                class="section-toggle"
                onclick={() => (showSelectedFrame = !showSelectedFrame)}
            >
                {showSelectedFrame ? "▼" : "▶"} Selected Frame
            </button>
            {#if showSelectedFrame && $animClip && $animClip.frames[$selectedFrameIndex]}
                {@const frame = $animClip.frames[$selectedFrameIndex]}
                <div class="section">
                    <div class="info-row">Frame #{$selectedFrameIndex}</div>
                    <label>
                        U Min
                        <input
                            type="number"
                            min="0"
                            max="1"
                            step="0.01"
                            value={frameUv(frame)[0]}
                            onchange={(e) => {
                                const idx = $selectedFrameIndex;
                                executeAnim("Set U min", (c) => {
                                    setFrameUv(
                                        c,
                                        idx,
                                        0,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0,
                                    );
                                });
                            }}
                        />
                    </label>
                    <label>
                        V Min
                        <input
                            type="number"
                            min="0"
                            max="1"
                            step="0.01"
                            value={frameUv(frame)[1]}
                            onchange={(e) => {
                                const idx = $selectedFrameIndex;
                                executeAnim("Set V min", (c) => {
                                    setFrameUv(
                                        c,
                                        idx,
                                        1,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0,
                                    );
                                });
                            }}
                        />
                    </label>
                    <label>
                        U Max
                        <input
                            type="number"
                            min="0"
                            max="1"
                            step="0.01"
                            value={frameUv(frame)[2]}
                            onchange={(e) => {
                                const idx = $selectedFrameIndex;
                                executeAnim("Set U max", (c) => {
                                    setFrameUv(
                                        c,
                                        idx,
                                        2,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0,
                                    );
                                });
                            }}
                        />
                    </label>
                    <label>
                        V Max
                        <input
                            type="number"
                            min="0"
                            max="1"
                            step="0.01"
                            value={frameUv(frame)[3]}
                            onchange={(e) => {
                                const idx = $selectedFrameIndex;
                                executeAnim("Set V max", (c) => {
                                    setFrameUv(
                                        c,
                                        idx,
                                        3,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0,
                                    );
                                });
                            }}
                        />
                    </label>
                    <label>
                        Frame Texture Override
                        <select
                            value={frame.texture_id ?? ""}
                            onchange={(e) => {
                                const idx = $selectedFrameIndex;
                                const val =
                                    (e.target as HTMLSelectElement).value ||
                                    null;
                                executeAnim("Set frame texture", (c) => {
                                    c.frames[idx].texture_id = val;
                                });
                            }}
                        >
                            <option value="">-- Same as clip --</option>
                            {#each projectImages as img}
                                <option value={img}>{img}</option>
                            {/each}
                        </select>
                    </label>

                    <!-- Frame Triggers -->
                    <div class="triggers-section">
                        <div class="triggers-header">
                            <span class="dim">Triggers</span>
                        </div>
                        {#if frame.triggers && frame.triggers.length > 0}
                            {#each frame.triggers as trigger, ti}
                                <div class="trigger-row">
                                    <span class="trigger-icon"
                                        >{trigger.type === "PlaySound"
                                            ? "🔊"
                                            : "📢"}</span
                                    >
                                    <span class="trigger-detail">
                                        {trigger.type === "PlaySound"
                                            ? trigger.clip_id
                                            : trigger.event_name}
                                    </span>
                                    <button
                                        class="frame-btn danger"
                                        title="Remove trigger"
                                        onclick={() => {
                                            const idx = $selectedFrameIndex;
                                            executeAnim(
                                                "Remove trigger",
                                                (c) => {
                                                    if (
                                                        c.frames[idx].triggers
                                                    ) {
                                                        c.frames[
                                                            idx
                                                        ].triggers!.splice(
                                                            ti,
                                                            1,
                                                        );
                                                    }
                                                },
                                            );
                                        }}>✕</button
                                    >
                                </div>
                            {/each}
                        {:else}
                            <div class="info-row">No triggers</div>
                        {/if}
                        <div class="trigger-add-row">
                            <select
                                bind:value={newTriggerType}
                                class="trigger-select"
                            >
                                <option value="PlaySound">Sound</option>
                                <option value="EmitEvent">Event</option>
                            </select>
                            <input
                                type="text"
                                bind:value={newTriggerValue}
                                placeholder={newTriggerType === "PlaySound"
                                    ? "clip_id"
                                    : "event_name"}
                                class="trigger-input"
                            />
                            <button
                                class="frame-btn"
                                title="Add trigger"
                                disabled={!newTriggerValue.trim()}
                                onclick={() => {
                                    const idx = $selectedFrameIndex;
                                    const val = newTriggerValue.trim();
                                    if (!val) return;
                                    const ttype = newTriggerType;
                                    executeAnim("Add trigger", (c) => {
                                        if (!c.frames[idx].triggers)
                                            c.frames[idx].triggers = [];
                                        if (ttype === "PlaySound") {
                                            c.frames[idx].triggers!.push({
                                                type: "PlaySound",
                                                clip_id: val,
                                            });
                                        } else {
                                            c.frames[idx].triggers!.push({
                                                type: "EmitEvent",
                                                event_name: val,
                                            });
                                        }
                                    });
                                    newTriggerValue = "";
                                }}>+</button
                            >
                        </div>
                    </div>
                </div>
            {/if}

            <!-- Property Tracks -->
            <button
                class="section-toggle"
                onclick={() => (showPropertyTracks = !showPropertyTracks)}
            >
                {showPropertyTracks ? "▼" : "▶"} Property Tracks ({$animClip
                    ?.property_tracks?.length ?? 0})
            </button>
            {#if showPropertyTracks && $animClip}
                <div class="section">
                    <!-- Add track -->
                    <div class="track-add-row">
                        <select
                            bind:value={newTrackProperty}
                            class="trigger-select"
                            style="width:130px"
                        >
                            {#if availProps( ["PositionX", "PositionY", "Rotation", "ScaleX", "ScaleY"], ).length}
                                <optgroup label="Transform">
                                    {#each availProps( ["PositionX", "PositionY", "Rotation", "ScaleX", "ScaleY"], ) as prop}
                                        <option value={prop}
                                            >{propLabel(prop)}</option
                                        >
                                    {/each}
                                </optgroup>
                            {/if}
                            {#if availProps( ["ColorR", "ColorG", "ColorB", "ColorA"], ).length}
                                <optgroup label="Sprite Color">
                                    {#each availProps( ["ColorR", "ColorG", "ColorB", "ColorA"], ) as prop}
                                        <option value={prop}
                                            >{propLabel(prop)}</option
                                        >
                                    {/each}
                                </optgroup>
                            {/if}
                            {#if availProps( ["UiOffsetX", "UiOffsetY", "UiSizeX", "UiSizeY"], ).length}
                                <optgroup label="UI Element">
                                    {#each availProps( ["UiOffsetX", "UiOffsetY", "UiSizeX", "UiSizeY"], ) as prop}
                                        <option value={prop}
                                            >{propLabel(prop)}</option
                                        >
                                    {/each}
                                </optgroup>
                            {/if}
                            {#if availProps( ["UiTextFontSize", "UiTextColorR", "UiTextColorG", "UiTextColorB", "UiTextColorA"], ).length}
                                <optgroup label="UI Text">
                                    {#each availProps( ["UiTextFontSize", "UiTextColorR", "UiTextColorG", "UiTextColorB", "UiTextColorA"], ) as prop}
                                        <option value={prop}
                                            >{propLabel(prop)}</option
                                        >
                                    {/each}
                                </optgroup>
                            {/if}
                            {#if availProps( ["UiImageColorR", "UiImageColorG", "UiImageColorB", "UiImageColorA"], ).length}
                                <optgroup label="UI Image">
                                    {#each availProps( ["UiImageColorR", "UiImageColorG", "UiImageColorB", "UiImageColorA"], ) as prop}
                                        <option value={prop}
                                            >{propLabel(prop)}</option
                                        >
                                    {/each}
                                </optgroup>
                            {/if}
                            {#if availProps( ["UiPanelColorR", "UiPanelColorG", "UiPanelColorB", "UiPanelColorA"], ).length}
                                <optgroup label="UI Panel">
                                    {#each availProps( ["UiPanelColorR", "UiPanelColorG", "UiPanelColorB", "UiPanelColorA"], ) as prop}
                                        <option value={prop}
                                            >{propLabel(prop)}</option
                                        >
                                    {/each}
                                </optgroup>
                            {/if}
                            {#if availProps( ["UiButtonColorR", "UiButtonColorG", "UiButtonColorB", "UiButtonColorA"], ).length}
                                <optgroup label="UI Button">
                                    {#each availProps( ["UiButtonColorR", "UiButtonColorG", "UiButtonColorB", "UiButtonColorA"], ) as prop}
                                        <option value={prop}
                                            >{propLabel(prop)}</option
                                        >
                                    {/each}
                                </optgroup>
                            {/if}
                            {#if availProps( ["UiProgressValue", "UiProgressFgR", "UiProgressFgG", "UiProgressFgB", "UiProgressFgA"], ).length}
                                <optgroup label="UI Progress">
                                    {#each availProps( ["UiProgressValue", "UiProgressFgR", "UiProgressFgG", "UiProgressFgB", "UiProgressFgA"], ) as prop}
                                        <option value={prop}
                                            >{propLabel(prop)}</option
                                        >
                                    {/each}
                                </optgroup>
                            {/if}
                            {#if availProps(["UiSliderValue"]).length}
                                <optgroup label="UI Slider">
                                    {#each availProps( ["UiSliderValue"], ) as prop}
                                        <option value={prop}
                                            >{propLabel(prop)}</option
                                        >
                                    {/each}
                                </optgroup>
                            {/if}
                        </select>
                        <button
                            class="action-btn"
                            style="flex:1; margin-top:0"
                            disabled={($animClip.property_tracks ?? []).some(
                                (t) => t.property === newTrackProperty,
                            )}
                            onclick={addPropertyTrack}>+ Add Track</button
                        >
                    </div>

                    {#if $animClip.property_tracks && $animClip.property_tracks.length > 0}
                        <div class="track-list">
                            {#each $animClip.property_tracks as track, ti}
                                <div
                                    class="track-item"
                                    class:selected={selectedTrackIndex === ti}
                                >
                                    <!-- svelte-ignore a11y_click_events_have_key_events -->
                                    <div
                                        class="track-header"
                                        role="button"
                                        tabindex="0"
                                        onclick={() =>
                                            (selectedTrackIndex =
                                                selectedTrackIndex === ti
                                                    ? null
                                                    : ti)}
                                    >
                                        <span class="track-prop"
                                            >{propLabel(track.property)}</span
                                        >
                                        <span class="track-kf-count"
                                            >{track.keyframes.length} kf</span
                                        >
                                        <button
                                            class="frame-btn danger"
                                            title="Remove track"
                                            onclick={(e) => {
                                                e.stopPropagation();
                                                removePropertyTrack(ti);
                                            }}>✕</button
                                        >
                                    </div>

                                    {#if selectedTrackIndex === ti}
                                        <div class="track-keyframes">
                                            <button
                                                class="action-btn"
                                                onclick={() =>
                                                    addKeyframeToTrack(ti)}
                                                >+ Keyframe at frame {$selectedFrameIndex}</button
                                            >
                                            {#if track.keyframes.length > 0}
                                                <div class="kf-list">
                                                    {#each track.keyframes as kf, ki}
                                                        <div class="kf-row">
                                                            <label
                                                                class="kf-label"
                                                            >
                                                                F
                                                                <input
                                                                    type="number"
                                                                    min="0"
                                                                    step="1"
                                                                    class="kf-input"
                                                                    value={kf.frame}
                                                                    onchange={(
                                                                        e,
                                                                    ) =>
                                                                        setKeyframeFrame(
                                                                            ti,
                                                                            ki,
                                                                            parseInt(
                                                                                (
                                                                                    e.target as HTMLInputElement
                                                                                )
                                                                                    .value,
                                                                            ) ||
                                                                                0,
                                                                        )}
                                                                />
                                                            </label>
                                                            <label
                                                                class="kf-label"
                                                            >
                                                                V
                                                                <input
                                                                    type="number"
                                                                    step="0.01"
                                                                    class="kf-input"
                                                                    value={kf.value}
                                                                    onchange={(
                                                                        e,
                                                                    ) =>
                                                                        setKeyframeValue(
                                                                            ti,
                                                                            ki,
                                                                            parseFloat(
                                                                                (
                                                                                    e.target as HTMLInputElement
                                                                                )
                                                                                    .value,
                                                                            ) ||
                                                                                0,
                                                                        )}
                                                                />
                                                            </label>
                                                            <button
                                                                class="frame-btn danger"
                                                                title="Remove keyframe"
                                                                onclick={() =>
                                                                    removeKeyframe(
                                                                        ti,
                                                                        ki,
                                                                    )}>✕</button
                                                            >
                                                        </div>
                                                    {/each}
                                                </div>
                                            {:else}
                                                <div class="info-row">
                                                    No keyframes
                                                </div>
                                            {/if}
                                        </div>
                                    {/if}
                                </div>
                            {/each}
                        </div>
                    {:else}
                        <div class="info-row">No property tracks</div>
                    {/if}
                </div>
            {/if}
        </div>
    </div>

    <!-- Right: Preview -->
    <div class="preview-area">
        <div class="preview-toolbar">
            <button
                class="tool-btn"
                title="Step backward (←)"
                onclick={stepBackward}>⏮</button
            >
            <button
                class="tool-btn"
                title={playing ? "Pause (Space)" : "Play (Space)"}
                onclick={togglePlay}
            >
                {playing ? "⏸" : "▶"}
            </button>
            <button
                class="tool-btn"
                title="Step forward (→)"
                onclick={stepForward}>⏭</button
            >
            <button class="tool-btn" title="Restart" onclick={restart}
                >⏹</button
            >
            <span class="frame-badge">
                Frame {$selectedFrameIndex}/{frameCount}
            </span>
            {#if $selectedClipName}
                <span class="clip-badge">{$selectedClipName}</span>
            {/if}
            <span class="file-name">{fileName}</span>
        </div>
        {#if showSpritesheetPicker}
            <!-- Spritesheet picker overlay -->
            <div class="sheet-picker">
                <div class="sheet-picker-toolbar">
                    <label
                        >Cols <input
                            type="number"
                            min="1"
                            max="64"
                            bind:value={sheetCols}
                            class="sheet-input"
                        /></label
                    >
                    <label
                        >Rows <input
                            type="number"
                            min="1"
                            max="64"
                            bind:value={sheetRows}
                            class="sheet-input"
                        /></label
                    >
                    <span class="dim" style="font-size: 10px"
                        >Click: select · Ctrl+click: toggle · Shift+click: range</span
                    >
                    <div class="toolbar-spacer"></div>
                    <button
                        class="action-btn"
                        style="width: auto; margin: 0"
                        disabled={selectedCells.size === 0}
                        onclick={addSelectedCellsToClip}
                        >+ Add {selectedCells.size} frame{selectedCells.size !==
                        1
                            ? "s"
                            : ""}</button
                    >
                    <button
                        class="action-btn"
                        style="width: auto; margin: 0"
                        disabled={selectedCells.size === 0}
                        onclick={replaceClipWithSelectedCells}>⇄ Replace</button
                    >
                    <button
                        class="tool-btn"
                        title="Close spritesheet picker"
                        onclick={() => {
                            showSpritesheetPicker = false;
                            selectedCells = new Set();
                        }}>✕</button
                    >
                </div>
                <div class="sheet-canvas-container">
                    <canvas
                        class="sheet-canvas"
                        bind:this={sheetCanvas}
                        width={600}
                        height={400}
                        onclick={onSheetClick}
                        onmousemove={onSheetMouseMove}
                        onmouseleave={() => {
                            sheetHoverCell = null;
                            drawSheetCanvas();
                        }}
                    ></canvas>
                </div>
            </div>
        {:else}
            <div class="canvas-container">
                <canvas id="anim-preview-canvas" bind:this={canvasEl}></canvas>
            </div>
        {/if}
        <!-- Timeline strip -->
        {#if $animClip && $animClip.frames.length > 0}
            <div class="timeline-strip">
                {#each $animClip.frames as frame, i}
                    <button
                        class="timeline-frame"
                        class:active={$selectedFrameIndex === i}
                        class:has-trigger={frame.triggers &&
                            frame.triggers.length > 0}
                        class:has-keyframe={$animClip.property_tracks?.some(
                            (t) => t.keyframes.some((k) => k.frame === i),
                        )}
                        onclick={() => selectFrame(i)}
                        title="Frame {i}{frame.triggers?.length
                            ? ` (${frame.triggers.length} trigger${frame.triggers.length > 1 ? 's' : ''})`
                            : ''}{$animClip.property_tracks?.some((t) =>
                            t.keyframes.some((k) => k.frame === i),
                        )
                            ? ' (keyframe)'
                            : ''}"
                    >
                        {i}
                    </button>
                {/each}
            </div>
        {/if}
    </div>
</div>

<style>
    .animation-editor {
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

    /* Clip list */
    .clip-list {
        display: flex;
        flex-direction: column;
        gap: 2px;
        max-height: 200px;
        overflow-y: auto;
    }

    .clip-item {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 4px 6px;
        border-radius: 4px;
        cursor: pointer;
        background: var(--ed-raised);
        border: 1px solid transparent;
    }

    .clip-item:hover {
        background: var(--ed-hover);
    }

    .clip-item.selected {
        border-color: var(--ed-accent);
        background: color-mix(in srgb, var(--ed-accent) 10%, transparent);
    }

    .clip-name {
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 11px;
    }

    .clip-rename-input {
        flex: 1;
        padding: 2px 4px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-accent);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 11px;
        outline: none;
    }

    .clip-add-row {
        display: flex;
        gap: 4px;
        margin-top: 6px;
    }

    .clip-add-row input {
        flex: 1;
        padding: 4px 6px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 11px;
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

    .checkbox-label {
        flex-direction: row !important;
        align-items: center;
        gap: 6px !important;
    }

    .info-row {
        font-size: 11px;
        color: var(--ed-muted);
        padding: 4px 0;
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

    .grid-form {
        margin-top: 6px;
        padding: 6px;
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        background: var(--ed-mantle);
    }

    .grid-form-row {
        display: flex;
        gap: 6px;
        margin-bottom: 4px;
        align-items: center;
    }

    .grid-form-row label {
        display: flex;
        align-items: center;
        gap: 4px;
        font-size: 11px;
        color: var(--ed-faint);
    }

    .grid-form-row input[type="number"] {
        width: 48px;
        padding: 3px 4px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 11px;
        font-family: var(--ed-mono);
    }

    .grid-form-row .action-btn {
        width: auto;
        flex: 1;
    }

    /* Frame list */
    .frame-actions {
        margin-bottom: 6px;
    }

    .frame-list {
        display: flex;
        flex-direction: column;
        gap: 2px;
        max-height: 300px;
        overflow-y: auto;
    }

    .frame-item {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 4px 6px;
        border-radius: 4px;
        cursor: pointer;
        background: var(--ed-raised);
        border: 1px solid transparent;
    }

    .frame-item:hover {
        background: var(--ed-hover);
    }

    .frame-item.selected {
        border-color: var(--ed-accent);
        background: color-mix(in srgb, var(--ed-accent) 10%, transparent);
    }

    .frame-index {
        font-weight: 600;
        min-width: 24px;
        color: var(--ed-muted);
    }

    .frame-uv {
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-family: monospace;
        font-size: 10px;
    }

    .frame-btns {
        display: flex;
        gap: 2px;
    }

    .frame-btn {
        padding: 1px 4px;
        font-size: 10px;
        border: none;
        border-radius: 4px;
        background: transparent;
        color: var(--ed-muted);
        cursor: pointer;
    }

    .frame-btn:hover {
        background: var(--ed-hover);
        color: var(--ed-text);
    }

    .frame-btn.danger:hover {
        color: var(--ed-err);
    }

    .frame-btn:disabled {
        opacity: 0.3;
        cursor: default;
    }

    /* Right: preview */
    .preview-area {
        flex: 1;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .preview-toolbar {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 6px 8px;
        border-bottom: 1px solid var(--ed-line);
        background: var(--ed-mantle);
    }

    .frame-badge {
        padding: 2px 8px;
        border-radius: 8px;
        background: var(--ed-raised);
        font-size: 11px;
        font-family: monospace;
    }

    .file-name {
        margin-left: auto;
        font-size: 11px;
        color: var(--ed-muted);
    }

    .canvas-container {
        flex: 1;
        position: relative;
        overflow: hidden;
    }

    .canvas-container canvas {
        position: absolute;
        inset: 0;
        width: 100%;
        height: 100%;
    }

    /* Timeline */
    .timeline-strip {
        display: flex;
        gap: 2px;
        padding: 6px 8px;
        border-top: 1px solid var(--ed-line);
        background: var(--ed-mantle);
        overflow-x: auto;
        min-height: 30px;
    }

    .timeline-frame {
        min-width: 28px;
        height: 22px;
        padding: 0 4px;
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        background: var(--ed-raised);
        color: var(--ed-muted);
        font-size: 10px;
        cursor: pointer;
        flex-shrink: 0;
    }

    .timeline-frame:hover {
        background: var(--ed-hover);
    }

    .timeline-frame.active {
        border-color: var(--ed-accent);
        background: color-mix(in srgb, var(--ed-accent) 15%, transparent);
        color: var(--ed-accent);
        font-weight: 600;
    }

    .timeline-frame.has-trigger {
        position: relative;
    }

    .timeline-frame.has-trigger::after {
        content: "";
        position: absolute;
        top: 2px;
        right: 2px;
        width: 5px;
        height: 5px;
        border-radius: 50%;
        background: var(--ed-a3);
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

    .tool-btn.linked {
        background: var(--ed-accent);
        color: var(--ed-base);
    }

    /* Triggers */
    .triggers-section {
        margin-top: 8px;
        border-top: 1px solid var(--ed-line);
        padding-top: 6px;
    }

    .triggers-header {
        font-size: 11px;
        font-weight: 600;
        margin-bottom: 4px;
    }

    .trigger-row {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 2px 4px;
        font-size: 11px;
        background: var(--ed-raised);
        border-radius: 3px;
        margin-bottom: 2px;
    }

    .trigger-icon {
        flex-shrink: 0;
    }

    .trigger-detail {
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        color: var(--ed-muted);
    }

    .trigger-add-row {
        display: flex;
        gap: 2px;
        margin-top: 4px;
    }

    .trigger-add-row > .trigger-select {
        width: 64px;
        flex-shrink: 0;
        padding: 2px 4px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 11px;
    }

    .trigger-add-row > .trigger-input {
        flex: 1;
        width: auto;
        padding: 2px 4px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 11px;
        min-width: 0;
    }

    /* Property Tracks */
    .track-add-row {
        display: flex;
        gap: 4px;
        margin-bottom: 6px;
    }

    .track-list {
        display: flex;
        flex-direction: column;
        gap: 2px;
    }

    .track-item {
        border: 1px solid transparent;
        border-radius: 4px;
        background: var(--ed-raised);
    }

    .track-item.selected {
        border-color: var(--ed-accent);
    }

    .track-header {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 4px 6px;
        cursor: pointer;
    }

    .track-header:hover {
        background: var(--ed-hover);
        border-radius: 4px;
    }

    .track-prop {
        font-weight: 600;
        font-size: 11px;
        min-width: 44px;
    }

    .track-kf-count {
        flex: 1;
        font-size: 10px;
        color: var(--ed-muted);
    }

    .track-keyframes {
        padding: 4px 6px 6px;
        border-top: 1px solid var(--ed-line);
    }

    .kf-list {
        display: flex;
        flex-direction: column;
        gap: 2px;
        margin-top: 4px;
    }

    .kf-row {
        display: flex;
        align-items: center;
        gap: 4px;
    }

    .kf-label {
        display: flex;
        flex-direction: row !important;
        align-items: center;
        gap: 2px;
        font-size: 10px;
        color: var(--ed-muted);
        margin-bottom: 0 !important;
    }

    .kf-input {
        width: 54px;
        padding: 2px 4px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 11px;
    }

    /* Keyframe indicator on timeline */
    .timeline-frame.has-keyframe {
        position: relative;
    }

    .timeline-frame.has-keyframe::before {
        content: "";
        position: absolute;
        bottom: 2px;
        right: 2px;
        width: 5px;
        height: 5px;
        border-radius: 1px;
        background: var(--ed-ok);
        transform: rotate(45deg);
    }

    /* Clip badge in preview toolbar */
    .clip-badge {
        padding: 2px 8px;
        border-radius: 8px;
        background: color-mix(in srgb, var(--ed-accent) 15%, transparent);
        color: var(--ed-accent);
        font-size: 11px;
        font-weight: 600;
    }

    /* Spritesheet picker */
    .sheet-picker {
        flex: 1;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .sheet-picker-toolbar {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 6px 8px;
        border-bottom: 1px solid var(--ed-line);
        background: var(--ed-mantle);
        flex-wrap: wrap;
    }

    .sheet-picker-toolbar label {
        display: flex;
        align-items: center;
        gap: 4px;
        font-size: 11px;
        color: var(--ed-muted);
    }

    .sheet-input {
        width: 48px;
        padding: 3px 4px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 11px;
    }

    .sheet-canvas-container {
        flex: 1;
        display: flex;
        align-items: center;
        justify-content: center;
        overflow: hidden;
        background: var(--ed-crust);
    }

    .sheet-canvas {
        cursor: crosshair;
        max-width: 100%;
        max-height: 100%;
    }

    /* Grid clips form */
    .grid-clips-names {
        max-height: 120px;
        overflow-y: auto;
        margin: 4px 0;
    }

    .dim {
        color: var(--ed-muted);
        font-size: 10px;
    }
</style>
