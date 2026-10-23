<script lang="ts">
  import { themeColors } from "./stores/themeColors";
  // A sprite's `fill`: stretched (the default), nine-sliced or tiled. For a
  // nine-slice the sprite's own picture - its `uv_rect` region of the
  // texture - is drawn with the four borders as guide lines to drag, in the
  // texture's pixels, as the engine measures them; the corners keep their
  // size and the edges and middle stretch. The data is `spriteFill.ts`.
  import { readAssetBinary } from "./api/project";
  import { project } from "./stores/projectStore";
  import { clampBorders, readFill, writeFill, type Borders, type Fill } from "./spriteFill";

  let {
    fill: raw,
    textureId,
    uvRect,
    disabled = false,
    onchange,
  }: {
    fill: unknown;
    textureId: string;
    uvRect?: [number, number, number, number] | null;
    disabled?: boolean;
    onchange: (fill: unknown) => void;
  } = $props();

  let fill = $derived<Fill>(readFill(raw));
  let draft = $state<Borders | null>(null);
  let borders = $derived<Borders>(draft ?? (fill.kind === "nine" ? fill.borders : { left: 0, right: 0, top: 0, bottom: 0 }));

  // The picture: the texture's pixels in the sprite's region.
  let img = $state<HTMLImageElement | null>(null);
  $effect(() => {
    const id = textureId;
    const proj = $project;
    img = null;
    if (!id || !proj || id === "__white") return;
    let url: string | null = null;
    readAssetBinary(proj.path, id)
      .then((bytes) => {
        url = URL.createObjectURL(new Blob([new Uint8Array(bytes)]));
        const el = new Image();
        el.onload = () => (img = el);
        el.src = url;
      })
      .catch(() => {});
    return () => {
      if (url) URL.revokeObjectURL(url);
    };
  });
  let region = $derived.by(() => {
    if (!img) return null;
    const [u0, v0, u1, v1] = uvRect ?? [0, 0, 1, 1];
    return {
      x: u0 * img.width,
      y: v0 * img.height,
      w: Math.max(1, Math.abs(u1 - u0) * img.width),
      h: Math.max(1, Math.abs(v1 - v0) * img.height),
    };
  });

  const BOX = 200;
  let scale = $derived(region ? Math.min(BOX / region.w, 140 / region.h) : 1);
  let canvas = $state<HTMLCanvasElement | null>(null);

  $effect(() => {
    const el = canvas;
    const r = region;
    if (!el || !r || !img) return;
    const w = Math.round(r.w * scale);
    const h = Math.round(r.h * scale);
    el.width = w;
    el.height = h;
    const g = el.getContext("2d");
    if (!g) return;
    g.imageSmoothingEnabled = scale < 2;
    g.clearRect(0, 0, w, h);
    g.drawImage(img, r.x, r.y, r.w, r.h, 0, 0, w, h);
    const b = borders;
    g.strokeStyle = $themeColors.warn;
    g.setLineDash([4, 3]);
    g.beginPath();
    for (const x of [b.left, r.w - b.right]) {
      g.moveTo(x * scale + 0.5, 0);
      g.lineTo(x * scale + 0.5, h);
    }
    for (const y of [b.top, r.h - b.bottom]) {
      g.moveTo(0, y * scale + 0.5);
      g.lineTo(w, y * scale + 0.5);
    }
    g.stroke();
  });

  // Dragging the nearest guide line.
  let dragging: keyof Borders | null = null;
  function down(e: PointerEvent) {
    if (disabled || !region) return;
    const r = canvas!.getBoundingClientRect();
    const px = (e.clientX - r.left) / scale;
    const py = (e.clientY - r.top) / scale;
    const b = borders;
    const d: [keyof Borders, number][] = [
      ["left", Math.abs(px - b.left)],
      ["right", Math.abs(px - (region.w - b.right))],
      ["top", Math.abs(py - b.top)],
      ["bottom", Math.abs(py - (region.h - b.bottom))],
    ];
    d.sort((a, z) => a[1] - z[1]);
    dragging = d[0][0];
    draft = { ...b };
    canvas!.setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (!dragging || !region || !draft) return;
    const r = canvas!.getBoundingClientRect();
    const px = (e.clientX - r.left) / scale;
    const py = (e.clientY - r.top) / scale;
    const next = { ...draft };
    if (dragging === "left") next.left = px;
    else if (dragging === "right") next.right = region.w - px;
    else if (dragging === "top") next.top = py;
    else next.bottom = region.h - py;
    draft = clampBorders(next, region.w, region.h);
  }
  function up() {
    if (!dragging) return;
    dragging = null;
    if (draft) onchange(writeFill({ kind: "nine", borders: draft }));
    draft = null;
  }

  function setKind(kind: Fill["kind"]) {
    if (kind === "stretch") onchange(undefined);
    else if (kind === "nine") {
      // A start: a quarter of the picture each side.
      const w = region?.w ?? 32;
      const h = region?.h ?? 32;
      onchange(writeFill({ kind: "nine", borders: clampBorders({ left: w / 4, right: w / 4, top: h / 4, bottom: h / 4 }, w, h) }));
    } else onchange(writeFill({ kind: "tile", tileSize: [region?.w ?? 32, region?.h ?? 32] }));
  }
  function setBorder(side: keyof Borders, value: number) {
    const b = { ...borders, [side]: value };
    onchange(writeFill({ kind: "nine", borders: region ? clampBorders(b, region.w, region.h) : clampBorders(b, 0, 0) }));
  }
  const num = (e: Event) => Number((e.target as HTMLInputElement).value);
</script>

<div class="fill">
  <div class="row">
    <span class="lbl">Fill</span>
    <select value={fill.kind} {disabled} onchange={(e) => setKind((e.target as HTMLSelectElement).value as Fill["kind"])}>
      <option value="stretch">stretch</option>
      <option value="nine">nine-slice</option>
      <option value="tile">tile</option>
    </select>
  </div>
  {#if fill.kind === "nine"}
    {#if region}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <canvas
        bind:this={canvas}
        class="pic"
        onpointerdown={down}
        onpointermove={move}
        onpointerup={up}
        title="Drag a guide line: the corners outside them keep their size"
      ></canvas>
    {:else}
      <div class="hint">No picture to show (the borders still apply).</div>
    {/if}
    <div class="row borders">
      {#each ["left", "right", "top", "bottom"] as side}
        <label>{side}<input type="number" min="0" {disabled} value={borders[side as keyof Borders]} onchange={(e) => setBorder(side as keyof Borders, num(e))} /></label>
      {/each}
    </div>
    <div class="hint">Borders in the texture's pixels{region ? ` (the picture is ${Math.round(region.w)} x ${Math.round(region.h)})` : ""}. Give the sprite a size to see it stretch.</div>
  {:else if fill.kind === "tile"}
    <div class="row">
      <span class="lbl">Tile size</span>
      <input type="number" min="1" {disabled} value={fill.tileSize[0]} onchange={(e) => onchange(writeFill({ kind: "tile", tileSize: [Math.max(1, num(e)), fill.kind === "tile" ? fill.tileSize[1] : 32] }))} />
      <input type="number" min="1" {disabled} value={fill.tileSize[1]} onchange={(e) => onchange(writeFill({ kind: "tile", tileSize: [fill.kind === "tile" ? fill.tileSize[0] : 32, Math.max(1, num(e))] }))} />
    </div>
  {/if}
</div>

<style>
  /* Laid out as the Inspector's own rows: indented, a fixed label column. */
  .fill {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 0 0 6px 18px;
    font-size: 12px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .lbl {
    flex: 0 0 82px;
    color: var(--ed-muted);
  }
  .pic {
    align-self: flex-start;
    border: 1px solid var(--ed-line);
    background: repeating-conic-gradient(var(--ed-raised) 0% 25%, var(--ed-base) 0% 50%) 50% / 12px 12px;
    cursor: crosshair;
    touch-action: none;
  }
  .borders label {
    display: flex;
    flex-direction: column;
    color: var(--ed-muted);
  }
  .borders input,
  .row input {
    width: 52px;
  }
  .hint {
    color: var(--ed-faint);
  }
  select,
  input {
    height: 24px;
    padding: 0 7px;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line);
    color: var(--ed-ink);
    border-radius: 4px;
    font-family: var(--ed-mono);
    font-size: 11.5px;
  }
  select {
    flex: 1;
    min-width: 0;
    appearance: none;
    padding-right: 22px;
    cursor: pointer;
    background-image:
      linear-gradient(45deg, transparent 50%, var(--ed-muted) 50%),
      linear-gradient(-45deg, transparent 50%, var(--ed-muted) 50%);
    background-position:
      calc(100% - 12px) 55%,
      calc(100% - 8px) 55%;
    background-size: 4px 4px;
    background-repeat: no-repeat;
  }
  select:focus,
  input:focus {
    outline: none;
    border-color: var(--ed-accent);
    box-shadow: 0 0 0 2px var(--ed-accent-low);
  }
</style>
