<script lang="ts">
  import { themeColors, withAlpha } from "./stores/themeColors";
  import { SettledHistory } from "./settledHistory";
  import { claimUndo } from "./stores/undoTargetStore";
  import { get } from "svelte/store";
  import { project, requestAssetRefresh } from "./stores/projectStore";
  import { listProjectImages } from "./api/project";
  import {
    packSpritesheet,
    saveSpritesheet,
    previewAtlas,
    type EditorPackConfig,
    type SpritesheetPackResult,
    type AtlasRegion,
    type SpritePoint,
  } from "./api/spritesheet";

  let { onclose, atlasPath }: { onclose: () => void; atlasPath?: string } =
    $props();

  // ── State ──────────────────────────────────────────────────
  let availableSprites = $state<string[]>([]);
  let selectedSprites = $state<string[]>([]);
  let outputName = $state("atlas");

  // Pack config
  let maxSize = $state(2048);
  let padding = $state(1);
  let powerOfTwo = $state(true);
  let packMode = $state<"auto" | "grid" | "rectpack">("auto");
  let cellWidth = $state(32);
  let cellHeight = $state(32);
  let columns = $state<number | undefined>(undefined);

  // Undo over the sheet's makings - which sprites, and how they are packed -
  // a step each time they settle after a change (`settledHistory.ts`).
  type Makings = {
    selectedSprites: string[];
    outputName: string;
    maxSize: number;
    padding: number;
    powerOfTwo: boolean;
    packMode: "auto" | "grid" | "rectpack";
    cellWidth: number;
    cellHeight: number;
    columns: number | null;
  };
  const history = new SettledHistory<Makings>(
    () => ({
      selectedSprites: [...selectedSprites],
      outputName,
      maxSize,
      padding,
      powerOfTwo,
      packMode,
      cellWidth,
      cellHeight,
      columns: columns ?? null,
    }),
    (v) => {
      selectedSprites = v.selectedSprites;
      outputName = v.outputName;
      maxSize = v.maxSize;
      padding = v.padding;
      powerOfTwo = v.powerOfTwo;
      packMode = v.packMode;
      cellWidth = v.cellWidth;
      cellHeight = v.cellHeight;
      columns = v.columns ?? undefined;
    },
  );
  $effect(() => {
    JSON.stringify([selectedSprites, outputName, maxSize, padding, powerOfTwo, packMode, cellWidth, cellHeight, columns]);
    history.observe();
  });
  $effect(() => claimUndo({ undo: () => history.undo(), redo: () => history.redo() }));

  // Result
  let packResult = $state<SpritesheetPackResult | null>(null);
  let packing = $state(false);
  let saving = $state(false);
  let error = $state("");
  let hoveredRegion = $state<AtlasRegion | null>(null);
  let selectedRegion = $state<AtlasRegion | null>(null);

  // Canvas
  let canvasRef = $state<HTMLCanvasElement | null>(null);
  let atlasImage = $state<HTMLImageElement | null>(null);

  // Load available sprites on mount, and load existing atlas if provided
  $effect(() => {
    const proj = get(project);
    if (proj?.path) {
      loadSprites(proj.path).then(() => {
        if (atlasPath) {
          loadExistingAtlas(proj.path, atlasPath);
        }
      });
    }
  });

  // Redraw canvas when result, hover, or selection changes
  $effect(() => {
    if (canvasRef && atlasImage && packResult) {
      // Track reactive dependencies
      void hoveredRegion;
      void selectedRegion;
      drawPreview();
    }
  });

  async function loadExistingAtlas(projectPath: string, path: string) {
    try {
      const preview = await previewAtlas(projectPath, path);
      // Derive output name from the atlas filename (e.g. "sprites/_atlases/player.atlas" → "player")
      const basename = path.split("/").pop() ?? path;
      outputName = basename.replace(/\.atlas$/, "");

      // Pre-select sprites whose filename stems match atlas region names
      const regionNames = new Set(
        preview.atlas_data.regions.map((r) => r.name),
      );
      selectedSprites = availableSprites.filter((s) => {
        const stem =
          s
            .split("/")
            .pop()
            ?.replace(/\.[^.]+$/, "") ?? "";
        return regionNames.has(stem);
      });

      // Build a pack result from the preview data so the canvas renders
      packResult = {
        image_base64: preview.image_base64,
        atlas_data: preview.atlas_data,
        width: preview.atlas_data.width,
        height: preview.atlas_data.height,
        efficiency: 0,
      };

      const img = new Image();
      img.onload = () => {
        atlasImage = img;
      };
      img.src = `data:image/png;base64,${preview.image_base64}`;
    } catch (e) {
      error = `Failed to load atlas: ${e}`;
    }
    // A loaded sheet is where undo starts.
    history.reset();
  }

  async function loadSprites(projectPath: string) {
    try {
      const all = await listProjectImages(projectPath);
      availableSprites = all.filter((f) => !f.includes("_atlases/"));
    } catch (e) {
      error = `Failed to list assets: ${e}`;
    }
  }

  function toggleSprite(path: string) {
    if (selectedSprites.includes(path)) {
      selectedSprites = selectedSprites.filter((s) => s !== path);
    } else {
      selectedSprites = [...selectedSprites, path];
    }
  }

  function selectAll() {
    selectedSprites = [...availableSprites];
  }

  function selectNone() {
    selectedSprites = [];
  }

  async function doPack() {
    const proj = get(project);
    if (!proj?.path || selectedSprites.length === 0) return;

    packing = true;
    error = "";
    packResult = null;

    const config: EditorPackConfig = {
      max_size: maxSize,
      padding,
      power_of_two: powerOfTwo,
      mode: packMode === "auto" ? "rectpack" : packMode,
      cell_width: packMode === "grid" ? cellWidth : undefined,
      cell_height: packMode === "grid" ? cellHeight : undefined,
      columns: packMode === "grid" ? columns : undefined,
    };

    try {
      packResult = await packSpritesheet(proj.path, selectedSprites, config);
      // Load image for canvas preview
      const img = new Image();
      img.onload = () => {
        atlasImage = img;
      };
      img.src = `data:image/png;base64,${packResult.image_base64}`;
    } catch (e) {
      error = `Packing failed: ${e}`;
    } finally {
      packing = false;
    }
  }

  async function doSave() {
    const proj = get(project);
    if (!proj?.path || !packResult) return;

    saving = true;
    error = "";

    try {
      await saveSpritesheet(
        proj.path,
        outputName,
        packResult.image_base64,
        JSON.stringify(packResult.atlas_data),
      );
    } catch (e) {
      error = `Save failed: ${e}`;
      saving = false;
      return;
    }

    saving = false;
    requestAssetRefresh();
    onclose();
  }

  function getCanvasTransform(): {
    scale: number;
    ox: number;
    oy: number;
  } | null {
    if (!canvasRef || !packResult) return null;
    const cw = canvasRef.clientWidth;
    const ch = canvasRef.clientHeight;
    const scaleX = cw / packResult.width;
    const scaleY = ch / packResult.height;
    const scale = Math.min(scaleX, scaleY, 1);
    const ox = (cw - packResult.width * scale) / 2;
    const oy = (ch - packResult.height * scale) / 2;
    return { scale, ox, oy };
  }

  function handleCanvasClick(e: MouseEvent) {
    if (!canvasRef || !packResult) return;
    const t = getCanvasTransform();
    if (!t) return;
    const rect = canvasRef.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    // Convert to atlas pixel coords
    const ax = (mx - t.ox) / t.scale;
    const ay = (my - t.oy) / t.scale;

    // Find region under click
    for (const r of packResult.atlas_data.regions) {
      if (
        ax >= r.x &&
        ax <= r.x + r.width &&
        ay >= r.y &&
        ay <= r.y + r.height
      ) {
        selectedRegion = r;
        return;
      }
    }
    selectedRegion = null;
  }

  function handleCanvasMouseMove(e: MouseEvent) {
    if (!canvasRef || !packResult) return;
    const t = getCanvasTransform();
    if (!t) return;
    const rect = canvasRef.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const ax = (mx - t.ox) / t.scale;
    const ay = (my - t.oy) / t.scale;

    for (const r of packResult.atlas_data.regions) {
      if (
        ax >= r.x &&
        ax <= r.x + r.width &&
        ay >= r.y &&
        ay <= r.y + r.height
      ) {
        hoveredRegion = r;
        return;
      }
    }
    hoveredRegion = null;
  }

  // ── Anchor presets ──
  function setAnchorPreset(x: number, y: number) {
    if (!selectedRegion) return;
    selectedRegion.anchor = [x, y];
    selectedRegion = selectedRegion; // trigger reactivity
  }

  // ── Points management ──
  function addPoint() {
    if (!selectedRegion) return;
    const name = `point_${selectedRegion.points.length}`;
    selectedRegion.points = [
      ...selectedRegion.points,
      { name, x: selectedRegion.width / 2, y: selectedRegion.height / 2 },
    ];
    selectedRegion = selectedRegion;
  }

  function removePoint(index: number) {
    if (!selectedRegion) return;
    selectedRegion.points = selectedRegion.points.filter((_, i) => i !== index);
    selectedRegion = selectedRegion;
  }

  function drawPreview() {
    if (!canvasRef || !atlasImage || !packResult) return;
    const ctx = canvasRef.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const cw = canvasRef.clientWidth;
    const ch = canvasRef.clientHeight;
    canvasRef.width = cw * dpr;
    canvasRef.height = ch * dpr;
    ctx.scale(dpr, dpr);

    ctx.clearRect(0, 0, cw, ch);

    // Checkerboard background
    const size = 8;
    for (let y = 0; y < ch; y += size) {
      for (let x = 0; x < cw; x += size) {
        ctx.fillStyle =
          (Math.floor(x / size) + Math.floor(y / size)) % 2 === 0
            ? $themeColors.hover
            : $themeColors.raised;
        ctx.fillRect(x, y, size, size);
      }
    }

    // Scale to fit
    const scaleX = cw / packResult.width;
    const scaleY = ch / packResult.height;
    const scale = Math.min(scaleX, scaleY, 1);
    const ox = (cw - packResult.width * scale) / 2;
    const oy = (ch - packResult.height * scale) / 2;

    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(
      atlasImage,
      ox,
      oy,
      packResult.width * scale,
      packResult.height * scale,
    );

    // Draw region outlines
    ctx.strokeStyle = "#89b4fa44";
    ctx.lineWidth = 1;
    for (const r of packResult.atlas_data.regions) {
      const rx = ox + r.x * scale;
      const ry = oy + r.y * scale;
      const rw = r.width * scale;
      const rh = r.height * scale;
      ctx.strokeRect(rx, ry, rw, rh);
    }

    // Highlight hovered region
    if (hoveredRegion) {
      ctx.strokeStyle = $themeColors.accent;
      ctx.lineWidth = 2;
      const rx = ox + hoveredRegion.x * scale;
      const ry = oy + hoveredRegion.y * scale;
      const rw = hoveredRegion.width * scale;
      const rh = hoveredRegion.height * scale;
      ctx.strokeRect(rx, ry, rw, rh);

      ctx.fillStyle = withAlpha($themeColors.accent, 0.15);
      ctx.fillRect(rx, ry, rw, rh);
    }

    // Highlight selected region
    if (selectedRegion) {
      ctx.strokeStyle = $themeColors.warn;
      ctx.lineWidth = 2;
      const rx = ox + selectedRegion.x * scale;
      const ry = oy + selectedRegion.y * scale;
      const rw = selectedRegion.width * scale;
      const rh = selectedRegion.height * scale;
      ctx.strokeRect(rx, ry, rw, rh);

      // Draw anchor crosshair
      const ax = rx + selectedRegion.anchor[0] * rw;
      const ay = ry + selectedRegion.anchor[1] * rh;
      const crossSize = 8;
      ctx.strokeStyle = $themeColors.err;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(ax - crossSize, ay);
      ctx.lineTo(ax + crossSize, ay);
      ctx.moveTo(ax, ay - crossSize);
      ctx.lineTo(ax, ay + crossSize);
      ctx.stroke();
      // Anchor dot
      ctx.fillStyle = $themeColors.err;
      ctx.beginPath();
      ctx.arc(ax, ay, 3, 0, Math.PI * 2);
      ctx.fill();

      // Draw named points
      for (const pt of selectedRegion.points) {
        const px = rx + (pt.x / selectedRegion.width) * rw;
        const py = ry + (pt.y / selectedRegion.height) * rh;
        // Diamond marker
        ctx.fillStyle = $themeColors.ok;
        ctx.beginPath();
        ctx.moveTo(px, py - 4);
        ctx.lineTo(px + 4, py);
        ctx.lineTo(px, py + 4);
        ctx.lineTo(px - 4, py);
        ctx.closePath();
        ctx.fill();
        // Label
        ctx.fillStyle = $themeColors.ok;
        ctx.font = `${10}px monospace`;
        ctx.textAlign = "left";
        ctx.textBaseline = "bottom";
        ctx.fillText(pt.name, px + 6, py - 2);
      }
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="overlay"
  onclick={onclose}
  onkeydown={(e) => {
    if (e.key === "Escape") onclose();
  }}
>
  <div
    class="dialog"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
    role="dialog"
    tabindex="-1"
    aria-label="Spritesheet Editor"
  >
    <div class="header">
      <h2>Spritesheet Editor</h2>
      <button class="close-btn" onclick={onclose} aria-label="Close"
        >&times;</button
      >
    </div>

    <div class="body">
      <!-- Left: sprite list -->
      <div class="sprite-list">
        <div class="section-title">
          <span>Sprites</span>
          <div class="actions">
            <button class="small-btn" onclick={selectAll}>All</button>
            <button class="small-btn" onclick={selectNone}>None</button>
          </div>
        </div>
        <div class="list-scroll">
          {#each availableSprites as sprite}
            <label
              class="sprite-item"
              class:selected={selectedSprites.includes(sprite)}
            >
              <input
                type="checkbox"
                checked={selectedSprites.includes(sprite)}
                onchange={() => toggleSprite(sprite)}
              />
              <span class="sprite-name" title={sprite}
                >{sprite.split("/").pop()}</span
              >
            </label>
          {/each}
          {#if availableSprites.length === 0}
            <div class="empty">No image assets found</div>
          {/if}
        </div>
        <div class="list-footer">{selectedSprites.length} selected</div>
      </div>

      <!-- Center: preview canvas -->
      <div class="preview-area">
        {#if packResult && atlasImage}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <canvas
            bind:this={canvasRef}
            onclick={handleCanvasClick}
            onmousemove={handleCanvasMouseMove}
            onmouseleave={() => {
              hoveredRegion = null;
            }}
          ></canvas>
          <div class="preview-info">
            {packResult.width}&times;{packResult.height}px &middot; {packResult
              .atlas_data.regions.length} regions &middot; {packResult.efficiency.toFixed(
              1,
            )}% packed
          </div>
        {:else if packing}
          <div class="placeholder">Packing…</div>
        {:else}
          <div class="placeholder">
            Select sprites and click "Pack" to preview
          </div>
        {/if}
      </div>

      <!-- Right: config + regions -->
      <div class="config-panel">
        <div class="section-title">Packing Config</div>

        <label class="field">
          <span>Output Name</span>
          <input type="text" bind:value={outputName} />
        </label>

        <label class="field">
          <span>Max Atlas Size</span>
          <select bind:value={maxSize}>
            <option value={512}>512</option>
            <option value={1024}>1024</option>
            <option value={2048}>2048</option>
            <option value={4096}>4096</option>
          </select>
        </label>

        <label class="field">
          <span>Padding</span>
          <input type="number" bind:value={padding} min="0" max="16" />
        </label>

        <label class="field checkbox">
          <input type="checkbox" bind:checked={powerOfTwo} />
          <span>Power of Two</span>
        </label>

        <label class="field">
          <span>Pack Mode</span>
          <select bind:value={packMode}>
            <option value="auto">Auto (best fit)</option>
            <option value="grid">Uniform Grid</option>
            <option value="rectpack">Rectangle Pack</option>
          </select>
        </label>

        {#if packMode === "grid"}
          <label class="field">
            <span>Cell Width</span>
            <input type="number" bind:value={cellWidth} min="1" />
          </label>
          <label class="field">
            <span>Cell Height</span>
            <input type="number" bind:value={cellHeight} min="1" />
          </label>
        {/if}

        {#if packResult}
          <div class="section-title" style="margin-top: 12px">Regions</div>
          <div class="region-list">
            {#each packResult.atlas_data.regions as region}
              <div
                class="region-item"
                class:hovered={hoveredRegion === region}
                class:selected={selectedRegion === region}
                role="button"
                tabindex="0"
                onclick={() => {
                  selectedRegion = region;
                }}
                onkeydown={(e) => {
                  if (e.key === "Enter" || e.key === " ")
                    selectedRegion = region;
                }}
                onmouseenter={() => {
                  hoveredRegion = region;
                }}
                onmouseleave={() => {
                  hoveredRegion = null;
                }}
              >
                <span class="region-name">{region.name}</span>
                <span class="region-size"
                  >{region.width}&times;{region.height}</span
                >
              </div>
            {/each}
          </div>

          {#if selectedRegion}
            <div class="section-title" style="margin-top: 12px">
              Anchor
              <span class="region-size">{selectedRegion.name}</span>
            </div>
            <div class="anchor-presets">
              <button
                class="preset-btn"
                class:active={selectedRegion.anchor[0] === 0 &&
                  selectedRegion.anchor[1] === 0}
                onclick={() => setAnchorPreset(0, 0)}
                title="Top-Left">&#8598;</button
              >
              <button
                class="preset-btn"
                class:active={selectedRegion.anchor[0] === 0.5 &&
                  selectedRegion.anchor[1] === 0}
                onclick={() => setAnchorPreset(0.5, 0)}
                title="Top-Center">&#8593;</button
              >
              <button
                class="preset-btn"
                class:active={selectedRegion.anchor[0] === 1 &&
                  selectedRegion.anchor[1] === 0}
                onclick={() => setAnchorPreset(1, 0)}
                title="Top-Right">&#8599;</button
              >
              <button
                class="preset-btn"
                class:active={selectedRegion.anchor[0] === 0 &&
                  selectedRegion.anchor[1] === 0.5}
                onclick={() => setAnchorPreset(0, 0.5)}
                title="Center-Left">&#8592;</button
              >
              <button
                class="preset-btn"
                class:active={selectedRegion.anchor[0] === 0.5 &&
                  selectedRegion.anchor[1] === 0.5}
                onclick={() => setAnchorPreset(0.5, 0.5)}
                title="Center">&#9679;</button
              >
              <button
                class="preset-btn"
                class:active={selectedRegion.anchor[0] === 1 &&
                  selectedRegion.anchor[1] === 0.5}
                onclick={() => setAnchorPreset(1, 0.5)}
                title="Center-Right">&#8594;</button
              >
              <button
                class="preset-btn"
                class:active={selectedRegion.anchor[0] === 0 &&
                  selectedRegion.anchor[1] === 1}
                onclick={() => setAnchorPreset(0, 1)}
                title="Bottom-Left">&#8601;</button
              >
              <button
                class="preset-btn"
                class:active={selectedRegion.anchor[0] === 0.5 &&
                  selectedRegion.anchor[1] === 1}
                onclick={() => setAnchorPreset(0.5, 1)}
                title="Bottom-Center">&#8595;</button
              >
              <button
                class="preset-btn"
                class:active={selectedRegion.anchor[0] === 1 &&
                  selectedRegion.anchor[1] === 1}
                onclick={() => setAnchorPreset(1, 1)}
                title="Bottom-Right">&#8600;</button
              >
            </div>
            <div class="anchor-custom">
              <label class="field inline">
                <span>X</span>
                <input
                  type="number"
                  step="0.05"
                  min="0"
                  max="1"
                  bind:value={selectedRegion.anchor[0]}
                  oninput={() => {
                    selectedRegion = selectedRegion;
                  }}
                />
              </label>
              <label class="field inline">
                <span>Y</span>
                <input
                  type="number"
                  step="0.05"
                  min="0"
                  max="1"
                  bind:value={selectedRegion.anchor[1]}
                  oninput={() => {
                    selectedRegion = selectedRegion;
                  }}
                />
              </label>
            </div>

            <div class="section-title" style="margin-top: 12px">
              Points
              <button class="small-btn" onclick={addPoint}>+ Add</button>
            </div>
            <div class="points-list">
              {#each selectedRegion.points as point, i}
                <div class="point-item">
                  <input
                    class="point-name"
                    type="text"
                    bind:value={point.name}
                    placeholder="name"
                    oninput={() => {
                      selectedRegion = selectedRegion;
                    }}
                  />
                  <input
                    class="point-coord"
                    type="number"
                    step="1"
                    min="0"
                    max={selectedRegion.width}
                    bind:value={point.x}
                    title="X (pixels)"
                    oninput={() => {
                      selectedRegion = selectedRegion;
                    }}
                  />
                  <input
                    class="point-coord"
                    type="number"
                    step="1"
                    min="0"
                    max={selectedRegion.height}
                    bind:value={point.y}
                    title="Y (pixels)"
                    oninput={() => {
                      selectedRegion = selectedRegion;
                    }}
                  />
                  <button
                    class="remove-btn"
                    onclick={() => removePoint(i)}
                    title="Remove">&times;</button
                  >
                </div>
              {/each}
              {#if selectedRegion.points.length === 0}
                <div class="empty">No points defined</div>
              {/if}
            </div>
          {/if}
        {/if}
      </div>
    </div>

    {#if error}
      <div class="error">{error}</div>
    {/if}

    <div class="footer">
      <button class="btn secondary" onclick={onclose}>Cancel</button>
      <button
        class="btn primary"
        onclick={doPack}
        disabled={packing || selectedSprites.length === 0}
      >
        {packing ? "Packing…" : "Pack"}
      </button>
      <button
        class="btn primary"
        onclick={doSave}
        disabled={saving || !packResult}
      >
        {saving ? "Saving…" : "Save Atlas"}
      </button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .dialog {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    border-radius: 8px;
    width: 900px;
    max-width: 95vw;
    max-height: 85vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
  }

  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 16px;
    border-bottom: 1px solid var(--ed-line);
  }

  .header h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--ed-text);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--ed-text);
    font-size: 18px;
    cursor: pointer;
    padding: 2px 6px;
    border-radius: 4px;
  }

  .close-btn:hover {
    background: var(--ed-raised);
  }

  .body {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  /* Sprite list panel */
  .sprite-list {
    width: 200px;
    border-right: 1px solid var(--ed-line);
    display: flex;
    flex-direction: column;
  }

  .section-title {
    font-size: 11px;
    text-transform: uppercase;
    color: var(--ed-muted);
    padding: 8px 8px 4px;
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .actions {
    display: flex;
    gap: 4px;
  }

  .small-btn {
    background: none;
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    font-size: 11.5px;
    padding: 3px 6px;
    border-radius: 4px;
    cursor: pointer;
  }

  .small-btn:hover {
    background: var(--ed-raised);
  }

  .list-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 4px 0;
  }

  .sprite-item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    cursor: pointer;
    font-size: 12px;
    color: var(--ed-text);
  }

  .sprite-item:hover {
    background: var(--ed-raised);
  }

  .sprite-item.selected {
    background: color-mix(in srgb, var(--ed-accent) 8%, transparent);
  }

  .sprite-item input[type="checkbox"] {
    accent-color: var(--ed-accent);
  }

  .sprite-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .list-footer {
    font-size: 11px;
    color: var(--ed-muted);
    padding: 4px 8px;
    border-top: 1px solid var(--ed-line);
  }

  .empty {
    padding: 12px 8px;
    font-size: 12px;
    color: var(--ed-muted);
    text-align: center;
  }

  /* Preview canvas */
  .preview-area {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-width: 0;
    position: relative;
    background: var(--ed-mantle);
  }

  .preview-area canvas {
    width: 100%;
    height: 100%;
    display: block;
  }

  .preview-info {
    position: absolute;
    bottom: 8px;
    left: 50%;
    transform: translateX(-50%);
    font-size: 11px;
    color: var(--ed-muted);
    background: color-mix(in srgb, var(--ed-base) 85%, transparent);
    padding: 3px 10px;
    border-radius: 4px;
  }

  .placeholder {
    color: var(--ed-muted);
    font-size: 13px;
  }

  /* Config panel */
  .config-panel {
    width: 200px;
    border-left: 1px solid var(--ed-line);
    overflow-y: auto;
    padding: 0 0 8px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px 8px;
    font-size: 12px;
    color: var(--ed-text);
  }

  .field.checkbox {
    flex-direction: row;
    align-items: center;
    gap: 6px;
  }

  .field span {
    font-size: 11px;
    color: var(--ed-muted);
  }

  .field input[type="text"],
  .field input[type="number"],
  .field select {
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    color: var(--ed-text);
    padding: 4px 6px;
    font-size: 12px;
  }

  .field input:focus,
  .field select:focus {
    outline: none;
    border-color: var(--ed-accent);
  }

  .field input[type="checkbox"] {
    accent-color: var(--ed-accent);
  }

  .region-list {
    max-height: 200px;
    overflow-y: auto;
  }

  .region-item {
    display: flex;
    justify-content: space-between;
    padding: 3px 8px;
    font-size: 11px;
    color: var(--ed-text);
    cursor: default;
  }

  .region-item:hover,
  .region-item.hovered {
    background: color-mix(in srgb, var(--ed-accent) 10%, transparent);
  }

  .region-item.selected {
    background: color-mix(in srgb, var(--ed-warn) 15%, transparent);
    border-left: 2px solid var(--ed-warn);
    padding-left: 6px;
  }

  .region-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  .region-size {
    color: var(--ed-muted);
    margin-left: 6px;
    flex-shrink: 0;
  }

  .error {
    padding: 6px 16px;
    font-size: 12px;
    color: var(--ed-err);
    background: color-mix(in srgb, var(--ed-err) 8%, transparent);
    border-top: 1px solid var(--ed-line);
  }

  .footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 16px;
    border-top: 1px solid var(--ed-line);
  }

  .btn {
    padding: 6px 16px;
    border-radius: 6px;
    font-size: 12px;
    cursor: pointer;
    border: none;
  }

  .btn.primary {
    background: var(--ed-accent);
    color: var(--ed-on-accent);
    font-weight: 600;
  }

  .btn.primary:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .btn.primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn.secondary {
    background: var(--ed-raised);
    color: var(--ed-text);
  }

  .btn.secondary:hover {
    background: var(--ed-line);
  }

  /* Anchor presets grid */
  .anchor-presets {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 2px;
    padding: 4px 8px;
  }

  .preset-btn {
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    font-size: 14px;
    padding: 4px;
    border-radius: 4px;
    cursor: pointer;
    text-align: center;
    line-height: 1;
  }

  .preset-btn:hover {
    background: var(--ed-raised);
  }

  .preset-btn.active {
    background: color-mix(in srgb, var(--ed-err) 20%, transparent);
    border-color: var(--ed-err);
    color: var(--ed-err);
  }

  .anchor-custom {
    display: flex;
    gap: 4px;
    padding: 4px 8px;
  }

  .field.inline {
    flex-direction: row;
    align-items: center;
    gap: 4px;
    flex: 1;
    padding: 0;
  }

  .field.inline input {
    width: 100%;
    min-width: 0;
  }

  /* Points list */
  .points-list {
    max-height: 160px;
    overflow-y: auto;
    padding: 0 4px;
  }

  .point-item {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px 4px;
    font-size: 11px;
  }

  .point-name {
    flex: 1;
    min-width: 0;
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-radius: 3px;
    color: var(--ed-text);
    padding: 2px 4px;
    font-size: 11px;
  }

  .point-coord {
    width: 44px;
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-radius: 3px;
    color: var(--ed-text);
    padding: 2px 4px;
    font-size: 11px;
    text-align: right;
  }

  .point-name:focus,
  .point-coord:focus {
    outline: none;
    border-color: var(--ed-accent);
  }

  .remove-btn {
    background: none;
    border: none;
    color: var(--ed-err);
    cursor: pointer;
    font-size: 14px;
    padding: 0 2px;
    line-height: 1;
  }

  .remove-btn:hover {
    color: var(--ed-err);
  }
</style>
