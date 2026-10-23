<script lang="ts">
  // The sprite-sheet slicer: open an image, cut it into regions (a grid of a
  // cell size, so many columns and rows, or every island of opaque pixels),
  // name them, make animation clips from rows, and write an `.atlas` of
  // packed metadata beside the image (`sheetSlicing.ts`). A scene lists that
  // `.atlas` in its `atlas_refs` and a sprite names a region - the slicer
  // can add the ref to the open scene. An `.atlas` already beside the image
  // is read back, so a sheet can be sliced again and its names kept.
  import { get } from "svelte/store";
  import { writeTextFile } from "@tauri-apps/plugin-fs";
  import { project, requestAssetRefresh } from "./stores/projectStore";
  import { scene } from "./stores/sceneStore";
  import { execute } from "./stores/commandStore";
  import { readAssetBinary } from "./api/project";
  import { logError, logInfo } from "./stores/consoleStore";
  import {
    atlasJson,
    atlasPathFor,
    clipFrom,
    duplicateNames,
    hasContent,
    nameRegions,
    rowsOf,
    sliceAuto,
    sliceCount,
    sliceGrid,
    type AtlasClip,
    type Rect,
    type Region,
  } from "./sheetSlicing";

  let { imagePath, onclose }: { imagePath: string; onclose: () => void } = $props();

  /** `assets/art/hero.png` -> `art/hero.png`: what reads and references use. */
  const assetRel = (p: string) => p.replace(/^assets\//, "");
  let atlasPath = $derived(atlasPathFor(imagePath));
  let fileName = $derived(imagePath.split("/").pop() ?? imagePath);

  let src = $state<string | null>(null);
  let width = $state(0);
  let height = $state(0);
  let pixels: Uint8ClampedArray | null = null;
  let error = $state("");

  let mode = $state<"grid" | "count" | "auto" | "file">("grid");
  let cellW = $state(32);
  let cellH = $state(32);
  let offX = $state(0);
  let offY = $state(0);
  let gapX = $state(0);
  let gapY = $state(0);
  let cols = $state(4);
  let rows = $state(4);
  let alphaMin = $state(0);
  let mergeGap = $state(1);
  let skipEmpty = $state(true);
  let base = $state("");
  let zoom = $state(2);
  let selected = $state<number | null>(null);
  let addRef = $state(true);
  let saving = $state(false);

  /** Names typed over the generated ones, by region index. */
  let names = $state<Record<number, string>>({});
  /** Regions read back from an `.atlas` already beside the image. */
  let fileRegions = $state<Region[]>([]);
  let clips = $state<AtlasClip[]>([]);
  let clipRow = $state(0);
  let clipName = $state("idle");
  let clipFps = $state(12);

  $effect(() => {
    void load(imagePath);
  });

  async function load(path: string) {
    const proj = get(project);
    if (!proj) return;
    base = (path.split("/").pop() ?? "region").replace(/\.[^.]+$/, "");
    try {
      const bytes = new Uint8Array(await readAssetBinary(proj.path, assetRel(path)));
      const blob = new Blob([bytes], { type: "image/png" });
      const url = URL.createObjectURL(blob);
      const img = new Image();
      img.src = url;
      await img.decode();
      width = img.naturalWidth;
      height = img.naturalHeight;
      const canvas = document.createElement("canvas");
      canvas.width = width;
      canvas.height = height;
      const ctx = canvas.getContext("2d")!;
      ctx.drawImage(img, 0, 0);
      pixels = ctx.getImageData(0, 0, width, height).data;
      src = url;
      zoom = Math.max(1, Math.min(4, Math.floor(560 / Math.max(width, height)) || 1));
    } catch (e) {
      error = `Could not read ${path}: ${e}`;
      return;
    }
    // An atlas already beside the image: start from its regions and clips.
    try {
      const text = new TextDecoder().decode(new Uint8Array(await readAssetBinary(proj.path, assetRel(atlasPath))));
      const data = JSON.parse(text) as { regions?: Region[]; animation_clips?: AtlasClip[] };
      if (Array.isArray(data.regions) && data.regions.length > 0) {
        fileRegions = data.regions.map((r) => ({ name: r.name, x: r.x, y: r.y, width: r.width, height: r.height }));
        clips = data.animation_clips ?? [];
        mode = "file";
      }
    } catch {
      // No atlas yet.
    }
  }

  let rects: Rect[] = $derived.by(() => {
    if (!width || !height) return [];
    let out: Rect[];
    if (mode === "file") return fileRegions;
    if (mode === "grid") {
      out = sliceGrid(width, height, { cellWidth: cellW, cellHeight: cellH, offsetX: offX, offsetY: offY, spacingX: gapX, spacingY: gapY });
    } else if (mode === "count") {
      out = sliceCount(width, height, cols, rows);
    } else {
      return pixels ? sliceAuto(pixels, width, height, { alphaMin, gap: mergeGap }) : [];
    }
    return skipEmpty && pixels ? out.filter((r) => hasContent(pixels!, width, r, alphaMin)) : out;
  });

  let regions: Region[] = $derived(
    mode === "file"
      ? fileRegions.map((r, i) => ({ ...r, name: names[i] ?? r.name }))
      : nameRegions(rects, base).map((r, i) => ({ ...r, name: names[i] ?? r.name })),
  );
  let rowOf = $derived(rowsOf(regions));
  let rowCount = $derived(rowOf.length ? Math.max(...rowOf) + 1 : 0);
  let duplicates = $derived(new Set(duplicateNames(regions)));

  // A new cut renames from scratch.
  $effect(() => {
    void [mode, cellW, cellH, offX, offY, gapX, gapY, cols, rows, alphaMin, mergeGap, skipEmpty];
    names = {};
    selected = null;
  });

  function addClip() {
    const frames = regions.filter((_, i) => rowOf[i] === clipRow);
    if (frames.length === 0 || !clipName.trim()) return;
    clips = [...clips.filter((c) => c.name !== clipName.trim()), clipFrom(clipName.trim(), frames, clipFps)];
  }

  async function save() {
    const proj = get(project);
    if (!proj || regions.length === 0) return;
    if (duplicates.size > 0) {
      error = `Region names must differ: ${[...duplicates].join(", ")}`;
      return;
    }
    saving = true;
    error = "";
    try {
      // Clips may name regions that were renamed or cut away since.
      const known = new Set(regions.map((r) => r.name));
      const keptClips = clips.filter((c) => c.frames.every((f) => known.has(f.region_name)));
      await writeTextFile(`${proj.path}/${atlasPath}`, JSON.stringify(atlasJson(width, height, regions, keptClips), null, 2) + "\n");
      logInfo(`Wrote ${atlasPath}: ${regions.length} region(s)${keptClips.length ? `, ${keptClips.length} clip(s)` : ""}`);
      const ref = assetRel(atlasPath);
      const refs = ((get(scene) as unknown as Record<string, unknown>).atlas_refs as string[] | undefined) ?? [];
      if (addRef && !refs.includes(ref)) {
        execute(`Add ${ref} to atlas_refs`, (s) => ({ ...s, atlas_refs: [...refs, ref] }));
        logInfo(`Added ${ref} to the scene's atlas_refs - save the scene for the viewport to load it`);
      }
      requestAssetRefresh();
    } catch (e) {
      error = `Could not write ${atlasPath}: ${e}`;
      logError(error);
    } finally {
      saving = false;
    }
  }

  const num = (e: Event, d: number) => {
    const v = parseInt((e.target as HTMLInputElement).value, 10);
    return Number.isFinite(v) ? v : d;
  };
</script>

<div class="overlay" role="dialog" aria-label="Slice sprite sheet">
  <div class="window">
    <div class="head">
      <span class="title">Slice {fileName}</span>
      <span class="dim">{width} x {height} -> {atlasPath}</span>
      <button class="close" onclick={onclose} title="Close">×</button>
    </div>
    <div class="body">
      <div class="side">
        <div class="group">
          <label><input type="radio" bind:group={mode} value="grid" /> Cell size</label>
          <label><input type="radio" bind:group={mode} value="count" /> Columns x rows</label>
          <label><input type="radio" bind:group={mode} value="auto" /> Automatic (opaque islands)</label>
          {#if fileRegions.length > 0}
            <label><input type="radio" bind:group={mode} value="file" /> As saved ({fileRegions.length})</label>
          {/if}
        </div>
        {#if mode === "grid"}
          <div class="pair"><span>cell</span><input type="number" min="1" value={cellW} onchange={(e) => (cellW = num(e, 32))} /><input type="number" min="1" value={cellH} onchange={(e) => (cellH = num(e, 32))} /></div>
          <div class="pair"><span>offset</span><input type="number" min="0" value={offX} onchange={(e) => (offX = num(e, 0))} /><input type="number" min="0" value={offY} onchange={(e) => (offY = num(e, 0))} /></div>
          <div class="pair"><span>spacing</span><input type="number" min="0" value={gapX} onchange={(e) => (gapX = num(e, 0))} /><input type="number" min="0" value={gapY} onchange={(e) => (gapY = num(e, 0))} /></div>
        {:else if mode === "count"}
          <div class="pair"><span>cols x rows</span><input type="number" min="1" value={cols} onchange={(e) => (cols = num(e, 1))} /><input type="number" min="1" value={rows} onchange={(e) => (rows = num(e, 1))} /></div>
        {:else if mode === "auto"}
          <div class="pair" title="Pixels this opaque or less are background"><span>alpha &gt;</span><input type="number" min="0" max="254" value={alphaMin} onchange={(e) => (alphaMin = num(e, 0))} /></div>
          <div class="pair" title="Islands nearer than this many pixels are one sprite"><span>join within</span><input type="number" min="0" value={mergeGap} onchange={(e) => (mergeGap = num(e, 1))} /></div>
        {/if}
        {#if mode === "grid" || mode === "count"}
          <label title="Leave out cells with no opaque pixel"><input type="checkbox" bind:checked={skipEmpty} /> skip empty cells</label>
        {/if}
        {#if mode !== "file"}
          <div class="pair"><span>names</span><input class="wide" bind:value={base} title="Regions are named <this>_0, <this>_1, ..." /></div>
        {/if}

        <div class="list">
          {#each regions as r, i}
            <div class="region" class:sel={selected === i} class:dup={duplicates.has(r.name)}>
              <button class="idx" onclick={() => (selected = i)} title="{r.x},{r.y} {r.width}x{r.height}, row {rowOf[i]}">{i}</button>
              <input value={r.name} onchange={(e) => (names = { ...names, [i]: (e.target as HTMLInputElement).value.trim() || r.name })} />
            </div>
          {/each}
          {#if regions.length === 0}<div class="dim">No regions.</div>{/if}
        </div>

        <div class="group">
          <span class="dim">Animation clip from a row</span>
          <div class="pair">
            <span>row</span>
            <input type="number" min="0" max={Math.max(0, rowCount - 1)} value={clipRow} onchange={(e) => (clipRow = num(e, 0))} />
            <input class="wide" bind:value={clipName} placeholder="name" />
          </div>
          <div class="pair"><span>fps</span><input type="number" min="1" value={clipFps} onchange={(e) => (clipFps = num(e, 12))} /><button onclick={addClip}>+ clip</button></div>
          {#each clips as c}
            <div class="clip">
              <span>{c.name}: {c.frames.length} frames @ {c.fps}</span>
              <button onclick={() => (clips = clips.filter((x) => x !== c))}>×</button>
            </div>
          {/each}
        </div>

        <label title="Add the .atlas to the open scene's atlas_refs, so its sprites can name these regions"><input type="checkbox" bind:checked={addRef} /> add to the open scene</label>
        <button class="save" disabled={saving || regions.length === 0} onclick={save}>Save {atlasPath.split("/").pop()}</button>
        {#if error}<div class="error">{error}</div>{/if}
      </div>

      <div class="view">
        <div class="zoom">
          <span class="dim">zoom</span>
          <input type="range" min="1" max="8" bind:value={zoom} />
          <span class="dim">{regions.length} region(s), {rowCount} row(s)</span>
        </div>
        {#if src}
          <div class="canvas" style:width="{width * zoom}px" style:height="{height * zoom}px">
            <img {src} alt={fileName} style:width="{width * zoom}px" style:height="{height * zoom}px" />
            <svg width={width * zoom} height={height * zoom}>
              {#each regions as r, i}
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                <rect
                  x={r.x * zoom}
                  y={r.y * zoom}
                  width={r.width * zoom}
                  height={r.height * zoom}
                  class:sel={selected === i}
                  onclick={() => (selected = i)}
                />
                {#if r.width * zoom > 14}
                  <text x={r.x * zoom + 2} y={r.y * zoom + 10}>{i}</text>
                {/if}
              {/each}
            </svg>
          </div>
        {:else if !error}
          <div class="dim">Loading...</div>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .window {
    width: min(1100px, 94vw);
    height: min(760px, 90vh);
    display: flex;
    flex-direction: column;
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    border-radius: 6px;
    color: var(--ed-text);
    font-size: 12px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--ed-line);
  }
  .title {
    font-weight: 600;
  }
  .close {
    margin-left: auto;
  }
  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .side {
    width: 280px;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow-y: auto;
    border-right: 1px solid var(--ed-line);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .pair {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .pair span {
    min-width: 72px;
    color: var(--ed-muted);
  }
  input,
  button {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    font-size: 11.5px;
    padding: 3px 4px;
  }
  input[type="number"] {
    width: 56px;
  }
  .wide {
    flex: 1;
    min-width: 0;
  }
  button {
    cursor: pointer;
  }
  .list {
    flex: 1;
    min-height: 120px;
    overflow-y: auto;
    border: 1px solid var(--ed-line);
    border-radius: 3px;
  }
  .region {
    display: flex;
    gap: 3px;
    padding: 1px 3px;
  }
  .region input {
    flex: 1;
  }
  .region.sel {
    background: var(--ed-line);
  }
  .region.dup input {
    border-color: var(--ed-err);
  }
  .idx {
    width: 30px;
  }
  .clip {
    display: flex;
    justify-content: space-between;
    color: var(--ed-ok);
  }
  .save {
    border-color: var(--ed-accent);
    padding: 4px;
  }
  .dim {
    color: var(--ed-faint);
  }
  .error {
    color: var(--ed-err);
  }
  .view {
    flex: 1;
    overflow: auto;
    padding: 8px;
  }
  .zoom {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
  }
  .canvas {
    position: relative;
    background-color: var(--ed-line);
    background-image:
      linear-gradient(45deg, var(--ed-line-strong) 25%, transparent 25%),
      linear-gradient(-45deg, var(--ed-line-strong) 25%, transparent 25%),
      linear-gradient(45deg, transparent 75%, var(--ed-line-strong) 75%),
      linear-gradient(-45deg, transparent 75%, var(--ed-line-strong) 75%);
    background-size: 16px 16px;
    background-position:
      0 0,
      0 8px,
      8px -8px,
      -8px 0;
  }
  .canvas img {
    position: absolute;
    inset: 0;
    image-rendering: pixelated;
  }
  .canvas svg {
    position: absolute;
    inset: 0;
  }
  rect {
    fill: color-mix(in srgb, var(--ed-accent) 8%, transparent);
    stroke: var(--ed-accent);
    stroke-width: 1;
    cursor: pointer;
  }
  rect.sel {
    fill: color-mix(in srgb, var(--ed-accent-high) 25%, transparent);
    stroke: var(--ed-accent-high);
    stroke-width: 2;
  }
  text {
    fill: var(--ed-warn);
    font-size: 9px;
    pointer-events: none;
  }
</style>
