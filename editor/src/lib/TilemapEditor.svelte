<script lang="ts">
    import { themeColors, withAlpha } from "./stores/themeColors";
    import { requestEditorClose } from "./stores/editorClose";
    import { onMount, onDestroy } from "svelte";
    import { get } from "svelte/store";
    import {
        tilemapData,
        tilemapFilePath,
        tilemapDirty,
        selectedLayerIndex,
        selectedTileId,
        brushMode,
        canUndoTilemap,
        canRedoTilemap,
        undoTilemap,
        redoTilemap,
        executeTilemap,
        beginTilemapStroke,
        updateTilemapLive,
        selectedTileIds,
        saveCurrentTilemap,
        closeTilemapEditor,
        tilemapEntity,
        tilemapAutotile,
        stampBlock,
        selectedTerrain,
        setTilemapAutotile,
    } from "./stores/tilemapEditorStore";
    import { nextTerrainStart, renameRuleLayer, ruleFor, setRule, terrainSwatch } from "./autotileEdit";
    import type { TilemapData, TilesetDefData } from "./api/tilemap";
    import { resolveGid, tileAtSheetPoint, tileRect, tileUv } from "./api/tilemap";
    import { SHAPE_PRESETS, presetOf, setPreset, type ShapePreset } from "./tileShapes";
    import {
        animationOf,
        copyBlock,
        stampCells,
        dataOf,
        framesInARow,
        pickTile,
        rectCells,
        setAnimation,
        setData,
        setSpawn,
        setVariants,
        spawnOf,
        variantsOf,
        type Frame,
        type Tileset,
    } from "./tileProps";
    import { assetsWithExtensions } from "./fieldValues";
    import { projectAssets } from "./stores/projectAssetsStore";
    import { listProjectImages, readAssetBinary } from "./api/project";
    import { project } from "./stores/projectStore";
    import init, {
        viewport_tilemap_preview_init,
        viewport_tilemap_preview_load,
        viewport_tilemap_preview_render,
        viewport_tilemap_preview_resize,
        viewport_tilemap_preview_set_camera,
        viewport_tilemap_preview_set_tile,
        viewport_tilemap_preview_load_tileset_texture,
        viewport_tilemap_preview_screen_to_tile,
        viewport_tilemap_preview_set_highlight,
        viewport_tilemap_preview_set_grid,
    } from "./viewport-wasm/jengine_editor_viewport.js";

    // ---------------------------------------------------------------------------
    // State
    // ---------------------------------------------------------------------------
    let canvasEl: HTMLCanvasElement | undefined = $state();
    let paletteCanvas: HTMLCanvasElement | undefined = $state();
    let wasmReady = $state(false);
    let animFrameId = 0;

    // Camera
    let camX = $state(0);
    let camY = $state(0);
    let camZoom = $state(2);

    // Panning
    let isPanning = $state(false);
    let panStartX = 0;
    let panStartY = 0;
    let panStartCamX = 0;
    let panStartCamY = 0;

    // Painting
    let isPainting = $state(false);

    // Grid
    let showGrid = $state(true);

    // Collapsible sections
    let showLayers = $state(true);
    let showTilesets = $state(true);
    let showPalette = $state(true);
    let showNewTileset = $state(false);

    // Project images
    let projectImages = $state<string[]>([]);

    // New tileset dialog
    let newTilesetImage = $state("");
    let newTilesetTileW = $state(16);
    let newTilesetTileH = $state(16);

    // New layer dialog
    let showNewLayer = $state(false);
    let newLayerName = $state("Layer");
    let renamingLayerIndex = $state<number | null>(null);

    // Resize tilemap
    let showResize = $state(false);
    let resizeWidth = $state(20);
    let resizeHeight = $state(15);

    // Loaded tileset textures (track which tileset indices have been loaded)
    // NOTE: plain variable, not $state — must not be a reactive dependency of
    // the $effect that calls syncToPreview, or every texture load would re-
    // trigger the effect and destroy/recreate the entity in a feedback loop.
    let loadedTilesets = new Set<string>();

    // Palette image for the selected tileset
    let paletteImages = $state<Map<number, HTMLImageElement>>(new Map());

    // Hover tile coordinates
    let hoverTileX = $state(-1);
    let hoverTileY = $state(-1);

    // Resize observer
    let resizeObserver: ResizeObserver | undefined;

    const fileName = $derived(
        $tilemapFilePath?.split(/[\\/]/).pop()?.replace(".tilemap", "") ??
            ($tilemapEntity ? `${$tilemapEntity} (in the scene)` : "Tilemap"),
    );

    const currentLayer = $derived(
        $tilemapData?.layers[$selectedLayerIndex] ?? null,
    );

    // Which tileset is selected for the palette
    let selectedTilesetIdx = $state(0);

    const currentTileset = $derived(
        $tilemapData?.tilesets[selectedTilesetIdx] ?? null,
    );

    /** The selected layer's terrain rule, when it is painted in terrains
     *  (the entity's `autotile`): its cells hold terrain numbers, and the
     *  brush paints `selectedTerrain`. */
    const terrainRule = $derived(currentLayer ? ruleFor($tilemapAutotile, currentLayer.name) : undefined);

    /** The preview draws the dual grid itself, given the rule. Read through
     *  the module, guarded, so an older viewport build still opens maps. */
    let viewportModule: any = null;
    import("./viewport-wasm/jengine_editor_viewport.js").then((m) => {
        viewportModule = m;
        pushAutotileToPreview();
    });
    function pushAutotileToPreview() {
        if (!wasmReady) return;
        try {
            viewportModule?.viewport_tilemap_preview_set_autotile?.(JSON.stringify(get(tilemapAutotile)));
        } catch {
            /* an unreadable rule draws the cells as they are */
        }
    }
    $effect(() => {
        void $tilemapAutotile;
        pushAutotileToPreview();
    });

    /** Make the selected layer a terrain layer, or a plain one again. */
    function setTerrainLayer(on: boolean) {
        if (!currentLayer) return;
        const first = $tilemapData?.tilesets[0]?.first_gid ?? 1;
        setTilemapAutotile(setRule($tilemapAutotile, currentLayer.name, on ? [nextTerrainStart(undefined, first)] : null));
        selectedTerrain.set(1);
    }
    function setTerrains(terrains: number[]) {
        if (!currentLayer) return;
        setTilemapAutotile(setRule($tilemapAutotile, currentLayer.name, terrains));
        if ($selectedTerrain > terrains.length) selectedTerrain.set(Math.max(1, terrains.length));
    }

    /** Draw tile `gid` into a small canvas (a terrain swatch). Takes the
     *  palette's images too, so a swatch drawn before its tileset's image
     *  had loaded is drawn again when it has. */
    function tileSwatch(node: HTMLCanvasElement, arg: [number, Map<number, HTMLImageElement>]) {
        const paint = ([g]: [number, Map<number, HTMLImageElement>]) => {
            const ctx = node.getContext("2d");
            const data = get(tilemapData);
            if (!ctx || !data) return;
            ctx.clearRect(0, 0, node.width, node.height);
            const hit = resolveGid(data.tilesets, g);
            const img = hit ? paletteImages.get(hit.tilesetIdx) : undefined;
            const ts = hit ? data.tilesets[hit.tilesetIdx] : undefined;
            const uv = hit && ts ? tileUv(ts, hit.localId) : null;
            if (!img || !uv || !ts) {
                ctx.fillStyle = $themeColors["line-strong"];
                ctx.fillRect(0, 0, node.width, node.height);
                return;
            }
            ctx.imageSmoothingEnabled = false;
            ctx.drawImage(img, uv[0] * img.width, uv[1] * img.height, ts.tile_width, ts.tile_height, 0, 0, node.width, node.height);
        };
        paint(arg);
        return { update: paint };
    }

    /** True when the selected layer is a collision layer (paint = solid, erase = empty). */
    const isCollisionLayer = $derived(currentLayer?.collision ?? false);

    // ---------------------------------------------------------------------------
    // WASM lifecycle
    // ---------------------------------------------------------------------------
    onMount(async () => {
        if (!canvasEl) return;
        try {
            await init();
        } catch {
            // Already initialized
        }

        const rect = canvasEl.parentElement!.getBoundingClientRect();
        canvasEl.width = Math.floor(rect.width);
        canvasEl.height = Math.floor(rect.height);

        viewport_tilemap_preview_init("tilemap-preview-canvas");
        wasmReady = true;

        syncToPreview();
        startLoop();

        resizeObserver = new ResizeObserver((entries) => {
            for (const entry of entries) {
                const w = Math.floor(entry.contentRect.width);
                const h = Math.floor(entry.contentRect.height);
                if (w > 0 && h > 0 && canvasEl) {
                    canvasEl.width = w;
                    canvasEl.height = h;
                    if (wasmReady) viewport_tilemap_preview_resize(w, h);
                }
            }
        });
        resizeObserver.observe(canvasEl.parentElement!);

        const proj = get(project);
        if (proj) {
            projectImages = await listProjectImages(proj.path);
        }
    });

    onDestroy(() => {
        stopLoop();
        resizeObserver?.disconnect();
    });

    function startLoop() {
        function frame() {
            if (wasmReady) {
                viewport_tilemap_preview_render();
            }
            animFrameId = requestAnimationFrame(frame);
        }
        animFrameId = requestAnimationFrame(frame);
    }

    function stopLoop() {
        if (animFrameId) {
            cancelAnimationFrame(animFrameId);
            animFrameId = 0;
        }
    }

    // ---------------------------------------------------------------------------
    // Sync tilemap data to WASM
    // ---------------------------------------------------------------------------
    function syncToPreview() {
        if (!wasmReady) return;
        const data = get(tilemapData);
        if (!data) return;
        viewport_tilemap_preview_load(JSON.stringify(data));
        // A fresh preview entity: give it the terrain rule again.
        pushAutotileToPreview();
        viewport_tilemap_preview_set_camera(camX, camY, camZoom);
        viewport_tilemap_preview_set_grid(showGrid);
        // Clear cache so the new WASM entity always gets its tileset texture patched.
        // viewport_tilemap_preview_load recreates the entity with original texture_ids;
        // without clearing here the loadedTilesets guard would skip the re-upload.
        loadedTilesets.clear();
        loadTilesetTextures(data);
    }

    async function loadTilesetTextures(data: TilemapData) {
        const proj = get(project);
        if (!proj) return;
        for (let i = 0; i < data.tilesets.length; i++) {
            const ts = data.tilesets[i];
            const key = `${i}:${ts.texture_id}`;
            if (loadedTilesets.has(key)) continue;
            try {
                const bytes = await readAssetBinary(proj.path, ts.texture_id);
                const u8 = new Uint8Array(bytes);
                viewport_tilemap_preview_load_tileset_texture(i, u8);
                loadedTilesets.add(key);

                // Load palette image only on first encounter (palette doesn't change
                // when the WASM entity is recreated with the same tileset texture).
                if (!paletteImages.has(i)) {
                    const blob = new Blob([u8]);
                    const url = URL.createObjectURL(blob);
                    const img = new Image();
                    img.onload = () => {
                        paletteImages = new Map(paletteImages).set(i, img);
                        drawPalette();
                    };
                    img.src = url;
                }
            } catch (e) {
                console.error(
                    `Failed to load tileset texture ${ts.texture_id}:`,
                    e,
                );
            }
        }
    }

    // Re-sync when tilemap data changes
    $effect(() => {
        void $tilemapData;
        if (wasmReady) syncToPreview();
    });

    // Update camera in WASM when it changes
    $effect(() => {
        void camX;
        void camY;
        void camZoom;
        if (wasmReady) {
            viewport_tilemap_preview_set_camera(camX, camY, camZoom);
        }
    });

    // Update grid visibility
    $effect(() => {
        void showGrid;
        if (wasmReady) viewport_tilemap_preview_set_grid(showGrid);
    });

    // Redraw palette when selection or data changes
    $effect(() => {
        void currentTileset;
        void $selectedTileId;
        void paletteImages;
        drawPalette();
    });

    // ---------------------------------------------------------------------------
    // Palette drawing (2D canvas)
    // ---------------------------------------------------------------------------
    function drawPalette() {
        if (!paletteCanvas || !currentTileset) return;
        const ctx = paletteCanvas.getContext("2d");
        if (!ctx) return;

        const ts = currentTileset;
        const img = paletteImages.get(selectedTilesetIdx);
        const cw = paletteCanvas.parentElement?.clientWidth ?? 256;

        if (!img || ts.image_width === 0) {
            paletteCanvas.width = cw;
            paletteCanvas.height = 60;
            ctx.fillStyle = $themeColors.crust;
            ctx.fillRect(0, 0, cw, 60);
            ctx.fillStyle = $themeColors.faint;
            ctx.font = "11px sans-serif";
            ctx.textAlign = "center";
            ctx.fillText("Load texture to see palette", cw / 2, 30);
            return;
        }

        const scale = Math.min(cw / ts.image_width, 4);
        const ch = Math.ceil(ts.image_height * scale);
        paletteCanvas.width = cw;
        paletteCanvas.height = ch;

        // Checkerboard background
        ctx.fillStyle = $themeColors.base;
        ctx.fillRect(0, 0, cw, ch);
        const cs = 8;
        for (let y = 0; y < ch; y += cs) {
            for (let x = 0; x < cw; x += cs) {
                if ((Math.floor(x / cs) + Math.floor(y / cs)) % 2 === 0) {
                    ctx.fillStyle = $themeColors.hover;
                    ctx.fillRect(x, y, cs, cs);
                }
            }
        }

        // Draw tileset image
        ctx.imageSmoothingEnabled = false;
        ctx.drawImage(
            img,
            0,
            0,
            ts.image_width * scale,
            ts.image_height * scale,
        );

        // Outline every tile (a margin or spacing leaves gaps between them).
        ctx.strokeStyle = withAlpha($themeColors.text, 0.15);
        ctx.lineWidth = 1;
        for (let id = 0; id < ts.tile_count; id++) {
            const r = tileRect(ts, id);
            ctx.strokeRect(r.x * scale, r.y * scale, r.w * scale, r.h * scale);
        }

        // The random brush's tiles.
        const many = get(selectedTileIds);
        if (many.length > 1) {
            ctx.strokeStyle = $themeColors.warn;
            ctx.lineWidth = 2;
            for (const g of many) {
                const r = resolveGid($tilemapData?.tilesets ?? [], g);
                if (!r || r.tilesetIdx !== selectedTilesetIdx) continue;
                const t = tileRect(ts, r.localId);
                ctx.strokeRect(t.x * scale, t.y * scale, t.w * scale, t.h * scale);
            }
        }

        // Highlight selected tile
        if ($selectedTileId !== null) {
            const gid = $selectedTileId;
            const resolved = resolveGid($tilemapData?.tilesets ?? [], gid);
            if (resolved && resolved.tilesetIdx === selectedTilesetIdx) {
                const t = tileRect(ts, resolved.localId);
                ctx.strokeStyle = $themeColors.accent;
                ctx.lineWidth = 2;
                ctx.strokeRect(t.x * scale, t.y * scale, t.w * scale, t.h * scale);
            }
        }
    }

    function handlePaletteClick(e: MouseEvent) {
        if (!paletteCanvas || !currentTileset) return;
        const ts = currentTileset;
        const img = paletteImages.get(selectedTilesetIdx);
        if (!img || ts.image_width === 0) return;

        const rect = paletteCanvas.getBoundingClientRect();
        const mx = e.clientX - rect.left;
        const my = e.clientY - rect.top;

        const cw = paletteCanvas.parentElement?.clientWidth ?? 256;
        const scale = Math.min(cw / ts.image_width, 4);

        const localId = tileAtSheetPoint(ts, mx / scale, my / scale);

        if (localId !== null) {
            const gid = ts.first_gid + localId;
            if (e.ctrlKey || e.shiftKey) {
                // Several tiles: a random brush.
                const cur = get(selectedTileIds);
                const base = cur.length > 0 ? cur : $selectedTileId !== null ? [$selectedTileId] : [];
                selectedTileIds.set(base.includes(gid) ? base.filter((g) => g !== gid) : [...base, gid]);
            } else {
                selectedTileIds.set([gid]);
            }
            selectedTileId.set(gid);
            if (get(brushMode) !== "rect") brushMode.set("paint");
            drawPalette();
        }
    }

    // ---------------------------------------------------------------------------
    // Canvas mouse events (pan, paint, hover)
    // ---------------------------------------------------------------------------
    function handleCanvasMouseDown(e: MouseEvent) {
        if (!canvasEl) return;
        if (e.button === 1 || (e.button === 0 && e.altKey)) {
            // Middle-click or Alt+click: start panning
            isPanning = true;
            panStartX = e.clientX;
            panStartY = e.clientY;
            panStartCamX = camX;
            panStartCamY = camY;
            e.preventDefault();
        } else if (e.button === 0) {
            if (get(brushMode) === "rect") {
                // A rectangle: from here to where the button is let go.
                rectStart = cellAt(e);
                rectEnd = rectStart;
                return;
            }
            // Left click: paint/erase/fill - the whole drag one undo step.
            if (get(brushMode) !== "fill") beginTilemapStroke();
            isPainting = true;
            applyBrushAtScreen(e);
        }
    }

    function handleCanvasMouseMove(e: MouseEvent) {
        if (!canvasEl || !wasmReady) return;
        if (isPanning) {
            const dx = (e.clientX - panStartX) / camZoom;
            const dy = (e.clientY - panStartY) / camZoom;
            camX = panStartCamX - dx;
            camY = panStartCamY + dy; // Y inverted
            return;
        }

        // Update hover highlight + coordinates
        const rect = canvasEl.getBoundingClientRect();
        const sx = e.clientX - rect.left;
        const sy = e.clientY - rect.top;
        const tileCoords = viewport_tilemap_preview_screen_to_tile(sx, sy);
        hoverTileX = tileCoords[0];
        hoverTileY = tileCoords[1];
        viewport_tilemap_preview_set_highlight(tileCoords[0], tileCoords[1]);

        if (isPainting) {
            applyBrushAtScreen(e);
        }
        if (rectStart) rectEnd = [tileCoords[0], tileCoords[1]];
    }

    function handleCanvasMouseUp(e: MouseEvent) {
        if (isPanning && (e.button === 1 || (e.button === 0 && e.altKey))) {
            isPanning = false;
        }
        if (isPainting && e.button === 0) {
            isPainting = false;
        }
        if (rectStart && e.button === 0) {
            const a = rectStart;
            const b = cellAt(e) ?? rectEnd ?? a;
            rectStart = null;
            rectEnd = null;
            if (a && b) {
                if (e.ctrlKey || e.metaKey) copyStamp(a, b);
                else paintRect(a, b, e.shiftKey);
            }
        }
    }

    /** The cell under the pointer, or null off the canvas. */
    function cellAt(e: MouseEvent): [number, number] | null {
        if (!wasmReady || !canvasEl) return null;
        const r = canvasEl.getBoundingClientRect();
        const c = viewport_tilemap_preview_screen_to_tile(e.clientX - r.left, e.clientY - r.top);
        return [c[0], c[1]];
    }

    /** The tile a brush paints now: the selected one, or one of the
     *  selected ones at random. */
    function brushTile(): number | null {
        // A terrain layer holds terrain numbers.
        if (terrainRule) return get(selectedTerrain);
        const many = get(selectedTileIds);
        if (many.length > 1) return pickTile(many, Math.random());
        return $selectedTileId;
    }

    /** Ctrl held as a rectangle is let go: copy its cells as the stamp, and
     *  switch to the stamp brush. */
    function copyStamp(a: [number, number], b: [number, number]) {
        const layer = $tilemapData?.layers[$selectedLayerIndex];
        if (!layer) return;
        const block = copyBlock(layer, a, b);
        if (!block) return;
        stampBlock.set(block);
        brushMode.set("stamp");
    }

    /** Lay the stamp with its top-left at a cell - inside a stroke, so a
     *  drag of stamps is one undo step; empty cells of it are left alone. */
    function stampAt(tx: number, ty: number) {
        const block = get(stampBlock);
        const layerIdx = $selectedLayerIndex;
        const layer = $tilemapData?.layers[layerIdx];
        if (!block || !layer) return;
        const cells = stampCells(block, [tx, ty], layer.width, layer.height).filter(
            ([x, y, t]) => layer.tiles[y * layer.width + x] !== t,
        );
        if (cells.length === 0) return;
        updateTilemapLive((data) => {
            const l = data.layers[layerIdx];
            for (const [x, y, t] of cells) l.tiles[y * l.width + x] = t;
        });
        for (const [x, y, t] of cells) viewport_tilemap_preview_set_tile(layerIdx, x, y, t);
    }

    /** Fill (or, with Shift, erase) the rectangle between two cells - one
     *  undo step. */
    function paintRect(a: [number, number], b: [number, number], erase: boolean) {
        const layerIdx = $selectedLayerIndex;
        const layer = $tilemapData?.layers[layerIdx];
        if (!layer) return;
        const cells = rectCells(a, b, layer.width, layer.height);
        if (cells.length === 0) return;
        if (!erase && brushTile() === null) return;
        executeTilemap(erase ? "Erase rectangle" : "Paint rectangle", (data) => {
            const l = data.layers[layerIdx];
            for (const [x, y] of cells) l.tiles[y * l.width + x] = erase ? null : brushTile();
        });
        syncToPreview();
    }

    function handleCanvasWheel(e: WheelEvent) {
        e.preventDefault();
        const delta = e.deltaY > 0 ? -0.2 : 0.2;
        camZoom = Math.max(0.25, Math.min(8, camZoom + delta * camZoom));
    }

    function handleCanvasLeave() {
        isPanning = false;
        isPainting = false;
        hoverTileX = -1;
        hoverTileY = -1;
        if (wasmReady) viewport_tilemap_preview_set_highlight(-1, -1);
    }

    function applyBrushAtScreen(e: MouseEvent) {
        if (!wasmReady || !canvasEl || !$tilemapData) return;
        const rect = canvasEl.getBoundingClientRect();
        const sx = e.clientX - rect.left;
        const sy = e.clientY - rect.top;
        const tileCoords = viewport_tilemap_preview_screen_to_tile(sx, sy);
        const tx = tileCoords[0];
        const ty = tileCoords[1];

        const layerIdx = $selectedLayerIndex;
        const layer = $tilemapData.layers[layerIdx];
        if (
            !layer ||
            tx < 0 ||
            ty < 0 ||
            tx >= layer.width ||
            ty >= layer.height
        )
            return;

        const mode = get(brushMode);

        // Collision layers: paint uses first available GID (solid), erase clears
        const paintGid = terrainRule
            ? get(selectedTerrain)
            : isCollisionLayer
              ? ($selectedTileId ?? $tilemapData.tilesets[0]?.first_gid ?? 1)
              : brushTile();

        if (mode === "paint" && paintGid !== null) {
            const curIdx = ty * layer.width + tx;
            if (layer.tiles[curIdx] === paintGid) return; // No change
            updateTilemapLive((data) => {
                const l = data.layers[layerIdx];
                l.tiles[ty * l.width + tx] = paintGid;
            });
            viewport_tilemap_preview_set_tile(layerIdx, tx, ty, paintGid);
        } else if (mode === "erase") {
            const curIdx = ty * layer.width + tx;
            if (layer.tiles[curIdx] === null) return;
            updateTilemapLive((data) => {
                const l = data.layers[layerIdx];
                l.tiles[ty * l.width + tx] = null;
            });
            viewport_tilemap_preview_set_tile(layerIdx, tx, ty, -1);
        } else if (mode === "fill") {
            floodFill(layerIdx, tx, ty);
        } else if (mode === "stamp") {
            stampAt(tx, ty);
        }
    }

    function floodFill(layerIdx: number, startX: number, startY: number) {
        const data = get(tilemapData);
        const fillTile = terrainRule ? get(selectedTerrain) : $selectedTileId;
        if (!data || fillTile === null) return;
        const layer = data.layers[layerIdx];
        if (!layer) return;

        const targetTile = layer.tiles[startY * layer.width + startX] ?? null;
        if (targetTile === fillTile) return;

        const visited = new Set<number>();
        const stack: [number, number][] = [[startX, startY]];

        executeTilemap("Fill tiles", (d) => {
            const l = d.layers[layerIdx];
            while (stack.length > 0) {
                const [x, y] = stack.pop()!;
                const idx = y * l.width + x;
                if (x < 0 || y < 0 || x >= l.width || y >= l.height) continue;
                if (visited.has(idx)) continue;
                visited.add(idx);
                const current = l.tiles[idx] ?? null;
                if (current !== targetTile) continue;
                l.tiles[idx] = fillTile;
                stack.push([x - 1, y], [x + 1, y], [x, y - 1], [x, y + 1]);
            }
        });
    }

    // ---------------------------------------------------------------------------
    // Layer actions
    // ---------------------------------------------------------------------------
    function openResize() {
        if (!$tilemapData) return;
        const ref = $tilemapData.layers[0];
        resizeWidth = ref?.width ?? 20;
        resizeHeight = ref?.height ?? 15;
        showResize = true;
    }

    function applyResize() {
        if (!$tilemapData) return;
        const newW = Math.max(1, Math.floor(resizeWidth));
        const newH = Math.max(1, Math.floor(resizeHeight));
        executeTilemap("Resize tilemap", (data) => {
            for (const layer of data.layers) {
                const oldW = layer.width;
                const oldH = layer.height;
                const newTiles: (number | null)[] = new Array(newW * newH).fill(
                    null,
                );
                const copyW = Math.min(oldW, newW);
                const copyH = Math.min(oldH, newH);
                for (let y = 0; y < copyH; y++) {
                    for (let x = 0; x < copyW; x++) {
                        newTiles[y * newW + x] = layer.tiles[y * oldW + x];
                    }
                }
                layer.tiles = newTiles;
                layer.width = newW;
                layer.height = newH;
            }
        });
        showResize = false;
        syncToPreview();
    }

    function addLayer() {
        if (!$tilemapData) return;
        const name = newLayerName.trim() || "Layer";
        const refLayer = $tilemapData.layers[0];
        const w = refLayer?.width ?? 20;
        const h = refLayer?.height ?? 15;
        executeTilemap("Add layer", (data) => {
            data.layers.push({
                name,
                tiles: new Array(w * h).fill(null),
                width: w,
                height: h,
                visible: true,
                collision: false,
            });
        });
        selectedLayerIndex.set(($tilemapData?.layers.length ?? 1) - 1);
        showNewLayer = false;
        newLayerName = "Layer";
    }

    function removeLayer(idx: number) {
        if (!$tilemapData || $tilemapData.layers.length <= 1) return;
        executeTilemap("Remove layer", (data) => {
            data.layers.splice(idx, 1);
        });
        if ($selectedLayerIndex >= ($tilemapData?.layers.length ?? 1)) {
            selectedLayerIndex.set(
                Math.max(0, ($tilemapData?.layers.length ?? 1) - 1),
            );
        }
    }

    function toggleLayerVisibility(idx: number) {
        executeTilemap("Toggle visibility", (data) => {
            data.layers[idx].visible = !data.layers[idx].visible;
        });
    }

    function toggleLayerCollision(idx: number) {
        executeTilemap("Toggle collision", (data) => {
            data.layers[idx].collision = !data.layers[idx].collision;
        });
    }

    function renameLayer(idx: number, name: string) {
        const trimmed = name.trim();
        renamingLayerIndex = null;
        if (!trimmed) return;
        const old = $tilemapData?.layers[idx]?.name;
        if (old === trimmed) return;
        executeTilemap("Rename layer", (data) => {
            data.layers[idx].name = trimmed;
        });
        // Its terrain rule follows it, in the same undo step.
        if (old && ruleFor($tilemapAutotile, old)) tilemapAutotile.set(renameRuleLayer($tilemapAutotile, old, trimmed));
    }

    function moveLayerUp(idx: number) {
        if (idx <= 0) return;
        executeTilemap("Move layer up", (data) => {
            [data.layers[idx - 1], data.layers[idx]] = [
                data.layers[idx],
                data.layers[idx - 1],
            ];
        });
        selectedLayerIndex.set(idx - 1);
    }

    function moveLayerDown(idx: number) {
        if (!$tilemapData || idx >= $tilemapData.layers.length - 1) return;
        executeTilemap("Move layer down", (data) => {
            [data.layers[idx], data.layers[idx + 1]] = [
                data.layers[idx + 1],
                data.layers[idx],
            ];
        });
        selectedLayerIndex.set(idx + 1);
    }

    // ---------------------------------------------------------------------------
    // Tileset actions
    // ---------------------------------------------------------------------------
    async function addTileset() {
        if (!$tilemapData || !newTilesetImage) return;
        const proj = get(project);
        if (!proj) return;

        // Load the image to get dimensions
        try {
            const bytes = await readAssetBinary(proj.path, newTilesetImage);
            const blob = new Blob([new Uint8Array(bytes)]);
            const url = URL.createObjectURL(blob);
            const img = await loadImage(url);

            const cols = Math.floor(img.naturalWidth / newTilesetTileW);
            const rows = Math.floor(img.naturalHeight / newTilesetTileH);
            const tileCount = cols * rows;

            // Calculate first_gid (after all existing tilesets)
            let maxGid = 0;
            for (const ts of $tilemapData.tilesets) {
                const end = ts.first_gid + ts.tile_count;
                if (end > maxGid) maxGid = end;
            }
            const firstGid = maxGid > 0 ? maxGid : 1;

            executeTilemap("Add tileset", (data) => {
                data.tilesets.push({
                    texture_id: newTilesetImage,
                    tile_width: newTilesetTileW,
                    tile_height: newTilesetTileH,
                    columns: cols,
                    tile_count: tileCount,
                    first_gid: firstGid,
                    image_width: img.naturalWidth,
                    image_height: img.naturalHeight,
                });
            });

            showNewTileset = false;
            newTilesetImage = "";
            // Reset loaded tilesets to force reload
            loadedTilesets.clear();
        } catch (e) {
            console.error("Failed to add tileset:", e);
        }
    }

    function loadImage(url: string): Promise<HTMLImageElement> {
        return new Promise((resolve, reject) => {
            const img = new Image();
            img.onload = () => resolve(img);
            img.onerror = reject;
            img.src = url;
        });
    }

    /** Change the map's grid shape (orientation, hex side, stagger); the
     *  engine's defaults are left out of the file, as it saves them. */
    function setMapShape(change: Partial<Pick<TilemapData, "orientation" | "hex_side" | "stagger_axis" | "stagger_index">>) {
        executeTilemap("Change map grid", (data) => {
            Object.assign(data, change);
            if (data.orientation === "orthogonal") delete data.orientation;
            if (!data.hex_side) delete data.hex_side;
            if (data.stagger_axis === "y") delete data.stagger_axis;
            if (data.stagger_index === "odd") delete data.stagger_index;
        });
        syncToPreview();
    }

    // A rectangle being dragged (brush "rect"): its first cell and where the
    // pointer is.
    let rectStart = $state<[number, number] | null>(null);
    let rectEnd = $state<[number, number] | null>(null);

    // ── The selected tile's properties (animation, variants, data, spawn) ──

    let selectedTile = $derived.by((): { tsIdx: number; local: number; ts: Tileset } | null => {
        const gid = $selectedTileId;
        const data = $tilemapData;
        if (gid === null || !data) return null;
        const r = resolveGid(data.tilesets, gid);
        if (!r) return null;
        return { tsIdx: r.tilesetIdx, local: r.localId, ts: data.tilesets[r.tilesetIdx] as Tileset };
    });
    let tileAnim = $derived(selectedTile ? animationOf(selectedTile.ts, selectedTile.local) : { frames: [], scatter: false });
    let tileVariants = $derived(selectedTile ? variantsOf(selectedTile.ts, selectedTile.local) : []);
    let tileData = $derived(selectedTile ? dataOf(selectedTile.ts, selectedTile.local) : undefined);
    let tileSpawn = $derived(selectedTile ? spawnOf(selectedTile.ts, selectedTile.local) : null);
    let tileDataError = $state("");
    let prefabChoices = $derived(assetsWithExtensions($projectAssets, ["prefab"]));

    /** Change the selected tile's entry in its tileset, one undo step. */
    function editTile(label: string, fn: (ts: Tileset, local: number) => void) {
        const sel = selectedTile;
        if (!sel) return;
        executeTilemap(label, (data) => fn(data.tilesets[sel.tsIdx] as Tileset, sel.local));
        syncToPreview();
    }

    function setFrames(frames: Frame[], scatter = tileAnim.scatter) {
        editTile("Set tile animation", (ts, t) => setAnimation(ts, t, frames, scatter));
    }

    function setTileDataText(text: string) {
        const t = text.trim();
        if (t === "") {
            tileDataError = "";
            editTile("Clear tile data", (ts, l) => setData(ts, l, null));
            return;
        }
        try {
            const v = JSON.parse(t);
            tileDataError = "";
            editTile("Set tile data", (ts, l) => setData(ts, l, v));
        } catch {
            tileDataError = "not JSON";
        }
    }

    // The selected tile's shape (collision and shadow), as a preset.
    let selectedShape = $derived.by((): { tsIdx: number; local: number; preset: ShapePreset } | null => {
        const gid = $selectedTileId;
        const data = $tilemapData;
        if (gid === null || !data) return null;
        const r = resolveGid(data.tilesets, gid);
        if (!r) return null;
        return { tsIdx: r.tilesetIdx, local: r.localId, preset: presetOf(data.tilesets[r.tilesetIdx], r.localId) };
    });

    function setSelectedShape(preset: string) {
        const sel = selectedShape;
        if (!sel || preset === "custom") return;
        executeTilemap("Set tile shape", (data) => {
            setPreset(data.tilesets[sel.tsIdx], sel.local, preset as Exclude<ShapePreset, "custom">);
        });
    }

    function removeTileset(idx: number) {
        executeTilemap("Remove tileset", (data) => {
            data.tilesets.splice(idx, 1);
        });
        loadedTilesets.clear();
        if (selectedTilesetIdx >= ($tilemapData?.tilesets.length ?? 0)) {
            selectedTilesetIdx = Math.max(
                0,
                ($tilemapData?.tilesets.length ?? 1) - 1,
            );
        }
    }

    // ---------------------------------------------------------------------------
    // Keyboard
    // ---------------------------------------------------------------------------
    function handleKeydown(e: KeyboardEvent) {
        if (e.ctrlKey && e.key === "z" && !e.shiftKey) {
            e.preventDefault();
            e.stopPropagation();
            undoTilemap();
        } else if (
            e.ctrlKey &&
            (e.key === "y" || (e.key === "z" && e.shiftKey))
        ) {
            e.preventDefault();
            e.stopPropagation();
            redoTilemap();
        } else if (e.key === "b" && !e.ctrlKey) {
            brushMode.set("paint");
        } else if (e.key === "e" && !e.ctrlKey) {
            brushMode.set("erase");
        } else if (e.key === "g" && !e.ctrlKey) {
            showGrid = !showGrid;
        } else if (e.key === "f" && !e.ctrlKey) {
            brushMode.set("fill");
        } else if (e.key === "r" && !e.ctrlKey) {
            brushMode.set("rect");
        } else if (e.key === "s" && !e.ctrlKey && !e.metaKey && get(stampBlock)) {
            brushMode.set("stamp");
        }
    }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="tilemap-editor" onkeydown={handleKeydown} tabindex="-1">
    <!-- Left panel: layers, tilesets, palette -->
    <div class="properties-panel">
        <!-- Toolbar -->
        <div class="panel-toolbar">
            <button
                class="tool-btn"
                title="Undo (Ctrl+Z)"
                disabled={!$canUndoTilemap}
                onclick={undoTilemap}>↩</button
            >
            <button
                class="tool-btn"
                title="Redo (Ctrl+Y)"
                disabled={!$canRedoTilemap}
                onclick={redoTilemap}>↪</button
            >
            <div class="toolbar-spacer"></div>
            <button
                class="tool-btn"
                title="Save (Ctrl+S)"
                onclick={saveCurrentTilemap}>💾</button
            >
            <button class="tool-btn" title="Close" onclick={() => requestEditorClose("tilemap")}
                >✕</button
            >
        </div>

        <div class="panel-scroll">
            <!-- Map Size -->
            {#if $tilemapData}
                <div class="section map-size-section">
                    <div class="map-size-row">
                        <span class="map-size-label">Map Size</span>
                        {#if showResize}
                            <div class="map-size-inputs">
                                <label class="size-label"
                                    >W
                                    <input
                                        type="number"
                                        min="1"
                                        max="1024"
                                        bind:value={resizeWidth}
                                        class="size-input"
                                    />
                                </label>
                                <span class="size-x">×</span>
                                <label class="size-label"
                                    >H
                                    <input
                                        type="number"
                                        min="1"
                                        max="1024"
                                        bind:value={resizeHeight}
                                        class="size-input"
                                    />
                                </label>
                                <button
                                    class="action-btn small"
                                    onclick={applyResize}>Apply</button
                                >
                                <button
                                    class="action-btn small"
                                    onclick={() => (showResize = false)}
                                    >✕</button
                                >
                            </div>
                        {:else}
                            <span class="map-size-value"
                                >{$tilemapData.layers[0]?.width ??
                                    0}×{$tilemapData.layers[0]?.height ??
                                    0}</span
                            >
                            <button
                                class="layer-btn"
                                title="Resize tilemap"
                                onclick={openResize}>✏</button
                            >
                        {/if}
                    </div>
                </div>
            {/if}

            <!-- Map: the grid's shape -->
            {#if $tilemapData}
                <div class="section map-shape">
                    <label title="Square cells, diamonds (isometric) or hexagons - every tool paints whichever">
                        Grid
                        <select
                            value={$tilemapData.orientation ?? "orthogonal"}
                            onchange={(e) => setMapShape({ orientation: (e.target as HTMLSelectElement).value as "orthogonal" | "isometric" | "hexagonal" })}
                        >
                            <option value="orthogonal">square</option>
                            <option value="isometric">isometric</option>
                            <option value="hexagonal">hexagonal</option>
                        </select>
                    </label>
                    {#if $tilemapData.orientation === "hexagonal"}
                        <label title="A hexagon's flat side, in pixels: half the tile height (or width) makes a regular hexagon">
                            side
                            <input type="number" min="0" value={$tilemapData.hex_side ?? 0} onchange={(e) => setMapShape({ hex_side: Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0) })} />
                        </label>
                        <label title="Pointy-topped (y) or flat-topped (x) hexagons">
                            stagger
                            <select value={$tilemapData.stagger_axis ?? "y"} onchange={(e) => setMapShape({ stagger_axis: (e.target as HTMLSelectElement).value as "x" | "y" })}>
                                <option value="y">y</option>
                                <option value="x">x</option>
                            </select>
                            <select value={$tilemapData.stagger_index ?? "odd"} onchange={(e) => setMapShape({ stagger_index: (e.target as HTMLSelectElement).value as "odd" | "even" })}>
                                <option value="odd">odd</option>
                                <option value="even">even</option>
                            </select>
                        </label>
                    {/if}
                </div>
            {/if}

            <!-- Layers -->
            <button
                class="section-toggle"
                onclick={() => (showLayers = !showLayers)}
            >
                {showLayers ? "▼" : "▶"} Layers ({$tilemapData?.layers
                    .length ?? 0})
            </button>
            {#if showLayers && $tilemapData}
                <div class="section">
                    <div class="layer-list">
                        {#each $tilemapData.layers as layer, i}
                            <!-- svelte-ignore a11y_click_events_have_key_events -->
                            <div
                                class="layer-item"
                                class:selected={$selectedLayerIndex === i}
                                onclick={() => selectedLayerIndex.set(i)}
                                role="button"
                                tabindex="0"
                            >
                                <button
                                    class="layer-vis"
                                    title={layer.visible ? "Hide" : "Show"}
                                    onclick={(e) => {
                                        e.stopPropagation();
                                        toggleLayerVisibility(i);
                                    }}>{layer.visible ? "👁" : "⊘"}</button
                                >
                                <button
                                    class="layer-btn"
                                    class:active={layer.collision}
                                    title={layer.collision
                                        ? "Collision layer (click to unset)"
                                        : "Set as collision layer"}
                                    onclick={(e) => {
                                        e.stopPropagation();
                                        toggleLayerCollision(i);
                                    }}>{layer.collision ? "🧱" : "▫"}</button
                                >
                                {#if renamingLayerIndex === i}
                                    <!-- svelte-ignore a11y_autofocus -->
                                    <input
                                        class="layer-name-input"
                                        type="text"
                                        value={layer.name}
                                        autofocus
                                        onclick={(e) => e.stopPropagation()}
                                        onblur={(e) =>
                                            renameLayer(
                                                i,
                                                (e.target as HTMLInputElement)
                                                    .value,
                                            )}
                                        onkeydown={(e) => {
                                            if (e.key === "Enter") {
                                                renameLayer(
                                                    i,
                                                    (
                                                        e.target as HTMLInputElement
                                                    ).value,
                                                );
                                            } else if (e.key === "Escape") {
                                                renamingLayerIndex = null;
                                            }
                                        }}
                                    />
                                {:else}
                                    <span
                                        class="layer-name"
                                        title="Double-click to rename"
                                        ondblclick={(e) => {
                                            e.stopPropagation();
                                            renamingLayerIndex = i;
                                        }}>{layer.name}</span
                                    >
                                {/if}
                                <span class="layer-dims"
                                    >{layer.width}×{layer.height}</span
                                >
                                <button
                                    class="layer-btn"
                                    title="Move up"
                                    onclick={(e) => {
                                        e.stopPropagation();
                                        moveLayerUp(i);
                                    }}>↑</button
                                >
                                <button
                                    class="layer-btn"
                                    title="Move down"
                                    onclick={(e) => {
                                        e.stopPropagation();
                                        moveLayerDown(i);
                                    }}>↓</button
                                >
                                <button
                                    class="layer-btn danger"
                                    title="Remove"
                                    onclick={(e) => {
                                        e.stopPropagation();
                                        removeLayer(i);
                                    }}>✕</button
                                >
                            </div>
                        {/each}
                    </div>
                    {#if showNewLayer}
                        <div class="inline-form layer-add-form">
                            <input
                                type="text"
                                placeholder="Layer name"
                                bind:value={newLayerName}
                            />
                            <button class="action-btn small" onclick={addLayer}
                                >Add</button
                            >
                            <button
                                class="action-btn small"
                                onclick={() => (showNewLayer = false)}>✕</button
                            >
                        </div>
                    {:else}
                        <button
                            class="action-btn"
                            onclick={() => (showNewLayer = true)}
                            >+ Add Layer</button
                        >
                    {/if}
                </div>
            {/if}

            <!-- Tilesets -->
            <button
                class="section-toggle"
                onclick={() => (showTilesets = !showTilesets)}
            >
                {showTilesets ? "▼" : "▶"} Tilesets ({$tilemapData?.tilesets
                    .length ?? 0})
            </button>
            {#if showTilesets && $tilemapData}
                <div class="section">
                    {#each $tilemapData.tilesets as ts, i}
                        <!-- svelte-ignore a11y_click_events_have_key_events -->
                        <div
                            class="tileset-item"
                            class:selected={selectedTilesetIdx === i}
                            onclick={() => (selectedTilesetIdx = i)}
                            role="button"
                            tabindex="0"
                        >
                            <span class="tileset-name"
                                >{ts.texture_id.split("/").pop()}</span
                            >
                            <span class="tileset-info"
                                >{ts.tile_count} tiles</span
                            >
                            <button
                                class="layer-btn danger"
                                title="Remove"
                                onclick={(e) => {
                                    e.stopPropagation();
                                    removeTileset(i);
                                }}>✕</button
                            >
                        </div>
                    {/each}
                    {#if showNewTileset}
                        <div class="new-tileset-form">
                            <label>
                                Image
                                <select bind:value={newTilesetImage}>
                                    <option value="">-- Select --</option>
                                    {#each projectImages as img}
                                        <option value={img}>{img}</option>
                                    {/each}
                                </select>
                            </label>
                            <div class="row-2">
                                <label>
                                    Tile W
                                    <input
                                        type="number"
                                        min="1"
                                        bind:value={newTilesetTileW}
                                    />
                                </label>
                                <label>
                                    Tile H
                                    <input
                                        type="number"
                                        min="1"
                                        bind:value={newTilesetTileH}
                                    />
                                </label>
                            </div>
                            <div class="row-2">
                                <button class="action-btn" onclick={addTileset}
                                    >Add</button
                                >
                                <button
                                    class="action-btn"
                                    onclick={() => (showNewTileset = false)}
                                    >Cancel</button
                                >
                            </div>
                        </div>
                    {:else}
                        <button
                            class="action-btn"
                            onclick={() => (showNewTileset = true)}
                            >+ Add Tileset</button
                        >
                    {/if}
                </div>
            {/if}

            <!-- Tile Palette -->
            <button
                class="section-toggle"
                onclick={() => (showPalette = !showPalette)}
            >
                {showPalette ? "▼" : "▶"} Palette
            </button>
            {#if showPalette && currentLayer}
                <!-- Terrains: which layer is painted in them is the entity's
                     `autotile` rule, kept beside the map. -->
                <div class="section terrain-section">
                    <label class="terrain-toggle" title={$tilemapEntity ? "Paint this layer in terrains: each cell a terrain number, drawn with its edges and corners on the dual grid" : "The terrain rule lives on the map's entity: open the map from its entity (the Inspector's edit button) to paint terrains"}>
                        <input
                            type="checkbox"
                            checked={!!terrainRule}
                            disabled={!$tilemapEntity}
                            onchange={(e) => setTerrainLayer((e.target as HTMLInputElement).checked)}
                        />
                        Paint "{currentLayer.name}" in terrains
                    </label>
                    {#if terrainRule}
                        <div class="terrain-list">
                            {#each terrainRule.terrains as first, i (i)}
                                <div class="terrain-row" class:sel={$selectedTerrain === i + 1}>
                                    <button class="terrain-pick" title="Paint terrain {i + 1}" onclick={() => selectedTerrain.set(i + 1)}>
                                        <canvas width="28" height="28" use:tileSwatch={[terrainSwatch(terrainRule, i + 1), paletteImages]}></canvas>
                                        <span>{i + 1}</span>
                                    </button>
                                    <label title="The first of this terrain's sixteen tiles (corner-mask order: 1 top-left, 2 top-right, 4 bottom-left, 8 bottom-right)">
                                        first gid
                                        <input
                                            type="number"
                                            min="1"
                                            value={first}
                                            onchange={(e) => {
                                                const t = [...terrainRule.terrains];
                                                t[i] = Math.max(1, Number((e.target as HTMLInputElement).value) || 1);
                                                setTerrains(t);
                                            }}
                                        />
                                    </label>
                                    <button class="terrain-x" title="Remove this terrain" onclick={() => setTerrains(terrainRule.terrains.filter((_, k) => k !== i))} disabled={terrainRule.terrains.length <= 1}>×</button>
                                </div>
                            {/each}
                        </div>
                        <button class="terrain-add" onclick={() => setTerrains([...terrainRule.terrains, nextTerrainStart(terrainRule, $tilemapData?.tilesets[0]?.first_gid ?? 1)])}>+ Terrain</button>
                        <div class="terrain-hint">Higher terrains draw over lower ones. Erase clears a cell.</div>
                    {/if}
                </div>
            {/if}
            {#if showPalette && !terrainRule}
                <div class="section palette-section">
                    {#if $tilemapData && $tilemapData.tilesets.length > 1}
                        <select
                            class="palette-selector"
                            value={selectedTilesetIdx}
                            onchange={(e) =>
                                (selectedTilesetIdx = parseInt(
                                    (e.target as HTMLSelectElement).value,
                                ))}
                        >
                            {#each $tilemapData.tilesets as ts, i}
                                <option value={i}
                                    >{ts.texture_id.split("/").pop()}</option
                                >
                            {/each}
                        </select>
                    {/if}
                    <!-- svelte-ignore a11y_click_events_have_key_events -->
                    <div
                        class="palette-canvas-container"
                        role="listbox"
                        tabindex="-1"
                    >
                        <canvas
                            id="tilemap-palette-canvas"
                            bind:this={paletteCanvas}
                            onclick={handlePaletteClick}
                        ></canvas>
                    </div>
                    {#if $selectedTileId !== null}
                        <div class="selected-tile-info">
                            Selected: GID {$selectedTileId}
                        </div>
                        {#if selectedShape}
                            <label
                                class="selected-tile-info"
                                title="What part of this tile is solid and casts a shadow, wherever it is painted. A layer collides through a Tilemap Collider and casts shadows through a Tilemap Occluder."
                            >
                                Shape
                                <select
                                    value={selectedShape.preset}
                                    onchange={(e) =>
                                        setSelectedShape(
                                            (e.target as HTMLSelectElement).value,
                                        )}
                                >
                                    {#each SHAPE_PRESETS as p}
                                        <option value={p.id}>{p.label}</option>
                                    {/each}
                                    {#if selectedShape.preset === "custom"}
                                        <option value="custom">Custom</option>
                                    {/if}
                                </select>
                            </label>
                        {/if}
                        {#if selectedTile}
                            <details class="tile-props" open={tileAnim.frames.length > 0 || tileVariants.length > 0 || tileData !== undefined || tileSpawn !== null}>
                                <summary>Tile {selectedTile.local}: animation, variants, data, spawn</summary>
                                <div class="tp-head" title="Wherever a cell names this tile, it is drawn as these frames in turn (seconds each)">
                                    Animation
                                    <button onclick={() => setFrames([...tileAnim.frames, { tile: (tileAnim.frames.at(-1)?.tile ?? selectedTile!.local - 1) + 1, duration: tileAnim.frames.at(-1)?.duration ?? 0.2 }])}>+ frame</button>
                                    <button title="The next 4 tiles in a row, 0.15 s each" onclick={() => setFrames(framesInARow(selectedTile!.local, 4, 0.15))}>row x4</button>
                                    {#if tileAnim.frames.length > 0}
                                        <label title="Start each cell at its own point in the loop"><input type="checkbox" checked={tileAnim.scatter} onchange={(e) => setFrames(tileAnim.frames, (e.target as HTMLInputElement).checked)} /> scatter</label>
                                    {/if}
                                </div>
                                {#each tileAnim.frames as f, i}
                                    <div class="tp-row">
                                        <span>tile</span>
                                        <input type="number" min="0" value={f.tile} onchange={(e) => setFrames(tileAnim.frames.map((g, j) => (j === i ? { ...g, tile: parseInt((e.target as HTMLInputElement).value) || 0 } : g)))} />
                                        <span>s</span>
                                        <input type="number" min="0" step="0.05" value={f.duration} onchange={(e) => setFrames(tileAnim.frames.map((g, j) => (j === i ? { ...g, duration: Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0) } : g)))} />
                                        <button onclick={() => setFrames(tileAnim.frames.filter((_, j) => j !== i))}>×</button>
                                    </div>
                                {/each}

                                <div class="tp-head" title="Wherever a cell names this tile, it is drawn as one of these, by weight - the same cell always alike">
                                    Variants
                                    <button onclick={() => editTile("Add tile variant", (ts, t) => setVariants(ts, t, [...variantsOf(ts, t), [t, 1]]))}>+ variant</button>
                                </div>
                                {#each tileVariants as v, i}
                                    <div class="tp-row">
                                        <span>tile</span>
                                        <input type="number" min="0" value={v[0]} onchange={(e) => editTile("Set tile variant", (ts, t) => setVariants(ts, t, tileVariants.map((w, j) => (j === i ? [parseInt((e.target as HTMLInputElement).value) || 0, w[1]] : w))))} />
                                        <span>weight</span>
                                        <input type="number" min="0" step="0.5" value={v[1]} onchange={(e) => editTile("Set tile variant", (ts, t) => setVariants(ts, t, tileVariants.map((w, j) => (j === i ? [w[0], Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0)] : w))))} />
                                        <button onclick={() => editTile("Remove tile variant", (ts, t) => setVariants(ts, t, tileVariants.filter((_, j) => j !== i)))}>×</button>
                                    </div>
                                {/each}

                                <div class="tp-head" title="Any JSON a game reads with Tilemap.get_tile_data">Data</div>
                                <textarea
                                    rows="2"
                                    spellcheck="false"
                                    placeholder={'{ "kind": "lava", "damage": 5 }'}
                                    value={tileData === undefined ? "" : JSON.stringify(tileData)}
                                    onchange={(e) => setTileDataText((e.target as HTMLTextAreaElement).value)}
                                ></textarea>
                                {#if tileDataError}<span class="tp-error">{tileDataError}</span>{/if}

                                <div class="tp-head" title="Wherever a cell names this tile, the scene gets an instance of this prefab at the cell's centre when it loads">Spawn</div>
                                <div class="tp-row">
                                    <input
                                        class="wide"
                                        list="tile-prefabs"
                                        placeholder="(none) prefab path"
                                        value={tileSpawn?.prefab ?? ""}
                                        onchange={(e) => editTile("Set tile spawn", (ts, t) => setSpawn(ts, t, (e.target as HTMLInputElement).value, tileSpawn?.keep ?? false))}
                                    />
                                    <datalist id="tile-prefabs">
                                        {#each prefabChoices as p}<option value={p}></option>{/each}
                                    </datalist>
                                    {#if tileSpawn}
                                        <label title="Keep the tile drawn under the instance"><input type="checkbox" checked={tileSpawn.keep} onchange={(e) => editTile("Set tile spawn", (ts, t) => setSpawn(ts, t, tileSpawn!.prefab, (e.target as HTMLInputElement).checked))} /> keep</label>
                                    {/if}
                                </div>
                            </details>
                        {/if}
                    {/if}
                </div>
            {/if}
        </div>
    </div>

    <!-- Center: Canvas + toolbar -->
    <div class="center-area">
        <div class="center-toolbar">
            <span class="file-name">🗺 {fileName}</span>
            <div class="tool-group">
                <button
                    class="tool-btn"
                    class:active={$brushMode === "paint"}
                    title="Paint (B)"
                    onclick={() => brushMode.set("paint")}>🖌</button
                >
                <button
                    class="tool-btn"
                    class:active={$brushMode === "erase"}
                    title="Erase (E)"
                    onclick={() => brushMode.set("erase")}>🧹</button
                >
                <button
                    class="tool-btn"
                    class:active={$brushMode === "fill"}
                    title="Fill (F)"
                    onclick={() => brushMode.set("fill")}>🪣</button
                >
                <button
                    class="tool-btn"
                    class:active={$brushMode === "rect"}
                    title="Rectangle (R): drag to fill a box; with Shift held at release, erase it"
                    onclick={() => brushMode.set("rect")}>▭</button
                >
                <button
                    class="tool-btn"
                    class:active={$brushMode === "stamp"}
                    disabled={!$stampBlock}
                    title={$stampBlock ? `Stamp (S): lay the copied ${$stampBlock.width} x ${$stampBlock.height} block, its top-left at the pointer` : "Stamp: hold Ctrl as you let go of a rectangle (R) to copy its cells"}
                    onclick={() => brushMode.set("stamp")}>⧉</button
                >
            </div>
            <div class="toolbar-separator"></div>
            <button
                class="tool-btn"
                class:active={showGrid}
                title="Toggle Grid (G)"
                onclick={() => (showGrid = !showGrid)}>#</button
            >
            <div class="toolbar-spacer"></div>
            {#if rectStart && rectEnd}
                <span class="info-badge">▭ {rectStart[0]},{rectStart[1]} → {rectEnd[0]},{rectEnd[1]}</span>
            {/if}
            {#if $selectedTileIds.length > 1}
                <span class="info-badge" title="Painting picks one of these at random (Ctrl/Shift-click the palette to change)">🎲 {$selectedTileIds.length} tiles</span>
            {/if}
            <span class="info-badge">{camZoom.toFixed(1)}×</span>
            {#if currentLayer}
                <span class="info-badge"
                    >{currentLayer.name} ({currentLayer.width}×{currentLayer.height})</span
                >
            {/if}
        </div>

        <!-- WASM canvas -->
        <div
            class="canvas-container"
            role="img"
            tabindex="-1"
            oncontextmenu={(e) => e.preventDefault()}
        >
            <canvas
                id="tilemap-preview-canvas"
                bind:this={canvasEl}
                onmousedown={handleCanvasMouseDown}
                onmousemove={handleCanvasMouseMove}
                onmouseup={handleCanvasMouseUp}
                onmouseleave={handleCanvasLeave}
                onwheel={handleCanvasWheel}
            ></canvas>
        </div>

        <!-- Info bar -->
        <div class="info-bar">
            <span
                >Pan: Middle-click / Alt+drag &nbsp; Zoom: Scroll &nbsp; B:
                Paint &nbsp; E: Erase &nbsp; F: Fill &nbsp; G: Grid</span
            >
            <div class="info-bar-right">
                {#if isCollisionLayer}
                    <span class="info-tag">Collision</span>
                {/if}
                {#if hoverTileX >= 0 && hoverTileY >= 0}
                    <span class="info-coords">{hoverTileX}, {hoverTileY}</span>
                {/if}
            </div>
        </div>
    </div>
</div>

<style>
    .tilemap-editor {
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
    .action-btn.small {
        padding: 3px 6px;
        margin-top: 0;
    }

    .inline-form {
        display: flex;
        gap: 4px;
        margin-top: 6px;
        align-items: center;
    }

    .inline-form input {
        flex: 1;
        min-width: 0;
        padding: 3px 6px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 11px;
    }

    .layer-add-form {
        flex-wrap: wrap;
    }

    .layer-add-form input {
        flex: 1 1 60%;
        min-width: 100px;
    }

    /* Map Size */
    .map-size-section {
        padding: 6px 8px;
    }

    .map-size-row {
        display: flex;
        align-items: center;
        gap: 6px;
        flex-wrap: wrap;
    }

    .map-size-label {
        font-size: 11px;
        font-weight: 600;
        color: var(--ed-text);
    }

    .map-size-value {
        font-family: monospace;
        font-size: 12px;
        color: var(--ed-muted);
    }

    .map-size-inputs {
        display: flex;
        align-items: center;
        gap: 4px;
        flex: 1;
    }

    .size-label {
        display: flex;
        align-items: center;
        gap: 2px;
        font-size: 10px;
        color: var(--ed-muted);
        margin-bottom: 0;
    }

    .size-input {
        width: 64px;
        padding: 4px 6px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 12px;
        -moz-appearance: textfield;
        appearance: textfield;
    }

    .size-input::-webkit-inner-spin-button,
    .size-input::-webkit-outer-spin-button {
        -webkit-appearance: none;
        margin: 0;
    }

    .size-x {
        font-size: 11px;
        color: var(--ed-muted);
    }

    /* Layer list */
    .layer-list {
        display: flex;
        flex-direction: column;
        gap: 1px;
        margin-bottom: 6px;
    }

    .layer-item {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 3px 4px;
        border-radius: 3px;
        cursor: pointer;
        background: var(--ed-raised);
        border: 1px solid transparent;
        font-size: 11px;
    }

    .layer-item:hover {
        background: var(--ed-hover);
    }
    .layer-item.selected {
        border-color: var(--ed-accent);
        background: color-mix(in srgb, var(--ed-accent) 10%, transparent);
    }

    .layer-vis {
        border: none;
        background: transparent;
        cursor: pointer;
        font-size: 12px;
        padding: 0 2px;
    }

    .layer-name {
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .layer-name-input {
        flex: 1;
        min-width: 0;
        background: var(--ed-base);
        border: 1px solid var(--ed-accent);
        color: var(--ed-text);
        font-size: 12px;
        padding: 1px 3px;
        border-radius: 4px;
    }

    .layer-dims {
        color: var(--ed-muted);
        font-family: monospace;
        font-size: 10px;
    }

    .layer-btn {
        padding: 3px 3px;
        font-size: 11.5px;
        border: none;
        border-radius: 4px;
        background: transparent;
        color: var(--ed-muted);
        cursor: pointer;
    }

    .layer-btn.danger:hover {
        color: var(--ed-err);
    }
    .layer-btn:hover {
        color: var(--ed-text);
    }

    /* Tileset list */
    .tileset-item {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 3px 4px;
        border-radius: 3px;
        cursor: pointer;
        background: var(--ed-raised);
        border: 1px solid transparent;
        font-size: 11px;
        margin-bottom: 1px;
    }

    .tileset-item:hover {
        background: var(--ed-hover);
    }
    .tileset-item.selected {
        border-color: var(--ed-accent);
        background: color-mix(in srgb, var(--ed-accent) 10%, transparent);
    }

    .tileset-name {
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .tileset-info {
        color: var(--ed-muted);
        font-family: monospace;
        font-size: 10px;
    }

    .new-tileset-form {
        margin-top: 6px;
        display: flex;
        flex-direction: column;
        gap: 6px;
    }

    /* Palette */
    .palette-section {
        padding: 4px;
    }

    .palette-selector {
        width: 100%;
        padding: 3px 4px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 11px;
        margin-bottom: 4px;
    }

    .palette-canvas-container {
        width: 100%;
        overflow: auto;
        max-height: 300px;
        cursor: pointer;
        outline: none;
    }

    .palette-canvas-container canvas {
        display: block;
        width: 100%;
    }

    .map-shape {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
        font-size: 11px;
        padding: 4px 0;
    }
    .map-shape input {
        width: 52px;
    }
    .tile-props {
        font-size: 11px;
        margin-top: 4px;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }
    .tile-props summary {
        cursor: pointer;
        color: var(--ed-muted);
    }
    .tp-head {
        display: flex;
        align-items: center;
        gap: 4px;
        margin-top: 4px;
        font-weight: 600;
    }
    .tp-row {
        display: flex;
        align-items: center;
        gap: 3px;
    }
    .tp-row input[type="number"] {
        width: 52px;
    }
    .tp-row .wide {
        flex: 1;
        min-width: 0;
    }
    .tile-props textarea {
        font-family: var(--ed-mono);
        font-size: 11px;
    }
    .tp-error {
        color: var(--ed-err);
    }
    .selected-tile-info {
        padding: 4px 6px;
        font-size: 10px;
        color: var(--ed-muted);
        font-family: monospace;
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
        gap: 6px;
        padding: 6px 8px;
        border-bottom: 1px solid var(--ed-line);
        background: var(--ed-mantle);
    }

    .file-name {
        font-size: 12px;
        font-weight: 600;
    }

    .tool-group {
        display: flex;
        gap: 2px;
    }

    .toolbar-separator {
        width: 1px;
        height: 18px;
        background: var(--ed-line);
    }

    .info-badge {
        padding: 2px 8px;
        border-radius: 8px;
        background: var(--ed-raised);
        font-size: 11px;
        font-family: monospace;
        color: var(--ed-muted);
    }

    .canvas-container {
        flex: 1;
        position: relative;
        overflow: hidden;
        min-height: 120px;
        cursor: crosshair;
    }

    .canvas-container canvas {
        position: absolute;
        inset: 0;
        width: 100%;
        height: 100%;
    }

    .info-bar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 4px 8px;
        border-top: 1px solid var(--ed-line);
        background: var(--ed-mantle);
        font-size: 10px;
        color: var(--ed-muted);
    }

    .info-bar-right {
        display: flex;
        align-items: center;
        gap: 8px;
    }

    .info-coords {
        font-family: monospace;
        font-size: 11px;
        color: var(--ed-text);
    }

    .info-tag {
        padding: 1px 6px;
        border-radius: 4px;
        background: color-mix(in srgb, var(--ed-ok) 15%, transparent);
        color: var(--ed-ok);
        font-size: 10px;
        font-weight: 600;
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
    .tool-btn.active {
        background: color-mix(in srgb, var(--ed-accent) 15%, transparent);
        border-color: var(--ed-accent);
    }
    /* Terrains */
    .terrain-section {
        display: flex;
        flex-direction: column;
        gap: 4px;
        font-size: 11px;
    }
    .terrain-toggle {
        display: flex;
        align-items: center;
        gap: 6px;
    }
    .terrain-list {
        display: flex;
        flex-direction: column;
        gap: 2px;
    }
    .terrain-row {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 2px;
        border-radius: 3px;
    }
    .terrain-row.sel {
        background: var(--ed-line);
    }
    .terrain-pick {
        display: flex;
        align-items: center;
        gap: 4px;
        background: none;
        border: 1px solid var(--ed-line);
        color: inherit;
        border-radius: 3px;
        padding: 1px 4px;
        cursor: pointer;
    }
    .terrain-row input[type="number"] {
        width: 50px;
    }
    .terrain-x,
    .terrain-add {
        background: none;
        border: 1px solid var(--ed-line);
        color: inherit;
        border-radius: 3px;
        cursor: pointer;
        font-size: 11px;
    }
    .terrain-add {
        align-self: flex-start;
    }
    .terrain-hint {
        color: var(--ed-faint);
    }
</style>
