<script lang="ts">
  // The parts of a `skeletal_player` the derived inspector can only show as
  // JSON: its 2D blend space - clips placed on a plot, the position dragged
  // about it with each clip's weight shown as the engine works it out
  // (`skelEdit.blendWeights`, checked against `blend_weights`) - and its
  // layers, each with a clip, a weight, a mode and a mask picked from the
  // skeleton's bones. A drag is held here and written once on release, so
  // it is one undo step.
  import {
    blendExtent,
    blendWeights,
    freshLayerName,
    type AnimLayer,
    type BlendSpace,
  } from "./skelEdit";

  let {
    data,
    bones,
    clips,
    disabled = false,
    onchange,
  }: {
    data: Record<string, unknown>;
    /** The skeleton's bone names, for layer masks. */
    bones: string[];
    /** Every `.skelanim`, as a player names it. */
    clips: string[];
    disabled?: boolean;
    onchange: (field: string, value: unknown) => void;
  } = $props();

  // ── Blend space ──

  let draft = $state<BlendSpace | null>(null);
  let space = $derived<BlendSpace | null>(draft ?? ((data.blend_space as BlendSpace | undefined) ?? null));
  let extent = $derived(space ? blendExtent(space) : 1);
  let weights = $derived(space ? blendWeights(space.points.map((p) => p.at), space.position ?? [0, 0]) : []);

  const SIZE = 200;
  const sx = (x: number) => SIZE / 2 + (x / extent) * (SIZE / 2 - 8);
  const sy = (y: number) => SIZE / 2 - (y / extent) * (SIZE / 2 - 8);
  const wx = (px: number) => ((px - SIZE / 2) / (SIZE / 2 - 8)) * extent;
  const wy = (py: number) => (-(py - SIZE / 2) / (SIZE / 2 - 8)) * extent;

  let svg = $state<SVGSVGElement | null>(null);
  let dragging: number | "position" | null = null;

  function at(e: PointerEvent): [number, number] {
    const r = svg!.getBoundingClientRect();
    const step = e.altKey ? 0 : 0.05;
    const px = ((e.clientX - r.left) / r.width) * SIZE;
    const py = ((e.clientY - r.top) / r.height) * SIZE;
    const sn = (v: number) => (step ? Math.round(v / step) * step : v);
    return [sn(wx(px)), sn(wy(py))];
  }

  function down(e: PointerEvent, what: number | "position") {
    if (disabled || !space) return;
    e.stopPropagation();
    dragging = what;
    draft = structuredClone($state.snapshot(space)) as BlendSpace;
    svg!.setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (dragging === null || !draft) return;
    const p = at(e);
    const next = structuredClone($state.snapshot(draft)) as BlendSpace;
    if (dragging === "position") next.position = p;
    else next.points[dragging].at = p;
    draft = next;
  }
  function up() {
    if (dragging === null) return;
    dragging = null;
    if (draft) onchange("blend_space", $state.snapshot(draft));
    draft = null;
  }
  // A click on empty plot moves the position there.
  function plotDown(e: PointerEvent) {
    if (disabled || !space) return;
    const p = at(e);
    draft = { ...(structuredClone($state.snapshot(space)) as BlendSpace), position: p };
    dragging = "position";
    svg!.setPointerCapture(e.pointerId);
  }

  function setSpace(next: BlendSpace | null) {
    onchange("blend_space", next ?? undefined);
  }
  function setPoint(i: number, patch: Partial<{ clip: string; at: [number, number] }>) {
    if (!space) return;
    const next = structuredClone($state.snapshot(space)) as BlendSpace;
    next.points[i] = { ...next.points[i], ...patch };
    setSpace(next);
  }
  function addPoint() {
    if (!space) return;
    const next = structuredClone($state.snapshot(space)) as BlendSpace;
    // The first free spot round the middle.
    const spots: [number, number][] = [[0, 0], [1, 0], [-1, 0], [0, 1], [0, -1], [1, 1], [-1, 1], [1, -1], [-1, -1]];
    const free = spots.find((s) => !next.points.some((p) => p.at[0] === s[0] && p.at[1] === s[1])) ?? [0, 0];
    next.points.push({ clip: clips[0] ?? "", at: free });
    setSpace(next);
  }
  function removePoint(i: number) {
    if (!space) return;
    const next = structuredClone($state.snapshot(space)) as BlendSpace;
    next.points.splice(i, 1);
    setSpace(next);
  }

  const short = (clip: string) => (clip.split("/").pop() ?? clip).replace(/\.skelanim$/, "");
  const num = (e: Event) => Number((e.target as HTMLInputElement).value);

  // ── Layers ──

  let layers = $derived<AnimLayer[]>((data.layers as AnimLayer[] | undefined) ?? []);

  function setLayers(next: AnimLayer[]) {
    onchange("layers", next.length > 0 ? next : undefined);
  }
  function setLayer(i: number, patch: Partial<AnimLayer>) {
    const next = structuredClone($state.snapshot(layers)) as AnimLayer[];
    next[i] = { ...next[i], ...patch };
    setLayers(next);
  }
  function addLayer() {
    const next = structuredClone($state.snapshot(layers)) as AnimLayer[];
    next.push({ name: freshLayerName(next), clip: "", weight: 1, mask: [], mode: "override" });
    setLayers(next);
  }
  function removeLayer(i: number) {
    const next = structuredClone($state.snapshot(layers)) as AnimLayer[];
    next.splice(i, 1);
    setLayers(next);
  }
  function moveLayer(i: number, by: number) {
    const j = i + by;
    if (j < 0 || j >= layers.length) return;
    const next = structuredClone($state.snapshot(layers)) as AnimLayer[];
    [next[i], next[j]] = [next[j], next[i]];
    setLayers(next);
  }
  // Ticks for the plot's grid.
  let grid = $derived.by(() => {
    const out: number[] = [];
    const step = extent > 2 ? 1 : 0.5;
    for (let k = Math.ceil(-extent / step); k <= Math.floor(extent / step); k++) out.push(k * step);
    return out;
  });
</script>

<div class="extras">
  <div class="sub-title">
    Blend space
    {#if space}
      <button class="small" {disabled} onclick={() => setSpace(null)} title="Remove the blend space">remove</button>
    {/if}
  </div>
  {#if !space}
    <button class="small" {disabled} onclick={() => setSpace({ points: [], position: [0, 0] })}>+ Add a blend space</button>
    <div class="hint">Clips placed on two axes - eight-way walks, an aim - mixed by where the position is.</div>
  {:else}
    {#if typeof data.clip === "string" && data.clip}
      <div class="warn">It plays only while Clip is empty (`Animation.play(e, "")` goes back to it).</div>
    {/if}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <svg
      bind:this={svg}
      class="plot"
      viewBox="0 0 {SIZE} {SIZE}"
      onpointerdown={plotDown}
      onpointermove={move}
      onpointerup={up}
    >
      {#each grid as g}
        <line x1={sx(g)} y1="0" x2={sx(g)} y2={SIZE} class:axis={Math.abs(g) < 1e-6} />
        <line x1="0" y1={sy(g)} x2={SIZE} y2={sy(g)} class:axis={Math.abs(g) < 1e-6} />
      {/each}
      {#each space.points as p, i}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <g onpointerdown={(e) => down(e, i)} class="point">
          <circle cx={sx(p.at[0])} cy={sy(p.at[1])} r={4 + 10 * (weights[i] ?? 0)} class="weight" />
          <circle cx={sx(p.at[0])} cy={sy(p.at[1])} r="4" />
          <!-- Right of centre, the label sits below and left of its point,
               inside the plot and clear of a label at the middle. -->
          <text
            x={sx(p.at[0]) + (sx(p.at[0]) > SIZE * 0.6 ? -6 : 6)}
            y={sy(p.at[1]) + (sx(p.at[0]) > SIZE * 0.6 ? 14 : -6)}
            text-anchor={sx(p.at[0]) > SIZE * 0.6 ? "end" : "start"}>{short(p.clip) || "?"}</text
          >
        </g>
      {/each}
      {#if space.position}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <g class="pos" onpointerdown={(e) => down(e, "position")}>
          <circle cx={sx(space.position[0])} cy={sy(space.position[1])} r="6" />
          <line x1={sx(space.position[0]) - 9} y1={sy(space.position[1])} x2={sx(space.position[0]) + 9} y2={sy(space.position[1])} />
          <line x1={sx(space.position[0])} y1={sy(space.position[1]) - 9} x2={sx(space.position[0])} y2={sy(space.position[1]) + 9} />
        </g>
      {/if}
    </svg>
    <div class="hint">Drag points and the cross (Alt: no snap); click the plot to move the position there.</div>
    <div class="rowline">
      <span class="lbl">Position</span>
      <input type="number" step="0.05" {disabled} value={space.position?.[0] ?? 0} onchange={(e) => setSpace({ ...space!, position: [num(e), space!.position?.[1] ?? 0] })} />
      <input type="number" step="0.05" {disabled} value={space.position?.[1] ?? 0} onchange={(e) => setSpace({ ...space!, position: [space!.position?.[0] ?? 0, num(e)] })} />
    </div>
    {#each space.points as p, i}
      <div class="rowline">
        <input class="clip" list="skel-clips" {disabled} value={p.clip} title={p.clip} placeholder="animations/x.skelanim" onchange={(e) => setPoint(i, { clip: (e.target as HTMLInputElement).value })} />
        <input type="number" step="0.05" {disabled} value={p.at[0]} onchange={(e) => setPoint(i, { at: [num(e), p.at[1]] })} />
        <input type="number" step="0.05" {disabled} value={p.at[1]} onchange={(e) => setPoint(i, { at: [p.at[0], num(e)] })} />
        <span class="pct">{Math.round((weights[i] ?? 0) * 100)}%</span>
        <button class="x" {disabled} onclick={() => removePoint(i)} title="Remove this point">×</button>
      </div>
    {/each}
    <button class="small" {disabled} onclick={addPoint}>+ Point</button>
  {/if}

  <div class="sub-title">Layers</div>
  {#each layers as layer, i}
    <div class="layer">
      <div class="rowline">
        <input class="name" {disabled} value={layer.name ?? ""} onchange={(e) => setLayer(i, { name: (e.target as HTMLInputElement).value })} />
        <select {disabled} value={layer.mode ?? "override"} onchange={(e) => setLayer(i, { mode: (e.target as HTMLSelectElement).value as AnimLayer["mode"] })}>
          <option value="override">override</option>
          <option value="additive">additive</option>
        </select>
        <button class="x" {disabled} onclick={() => moveLayer(i, -1)} title="Earlier (under)">↑</button>
        <button class="x" {disabled} onclick={() => moveLayer(i, 1)} title="Later (over)">↓</button>
        <button class="x" {disabled} onclick={() => removeLayer(i)} title="Remove this layer">×</button>
      </div>
      <div class="rowline">
        <span class="lbl">Clip</span>
        <input class="clip" list="skel-clips" {disabled} value={layer.clip ?? ""} placeholder="(none - a script plays one)" onchange={(e) => setLayer(i, { clip: (e.target as HTMLInputElement).value })} />
      </div>
      <div class="rowline">
        <span class="lbl">Weight</span>
        <input type="range" min="0" max="1" step="0.01" {disabled} value={layer.weight ?? 1} onchange={(e) => setLayer(i, { weight: num(e) })} />
        <span class="pct">{Math.round((layer.weight ?? 1) * 100)}%</span>
        <label class="check"><input type="checkbox" {disabled} checked={layer.looping ?? true} onchange={(e) => setLayer(i, { looping: (e.target as HTMLInputElement).checked })} /> loop</label>
      </div>
      <div class="rowline">
        <span class="lbl">Mask</span>
        <div class="chips">
          {#each layer.mask ?? [] as m}
            <span class="chip" class:missing={!bones.includes(m)} title={bones.includes(m) ? `${m} and every bone below it` : "No such bone in this skeleton"}>
              {m}<button class="x" {disabled} onclick={() => setLayer(i, { mask: (layer.mask ?? []).filter((x) => x !== m) })}>×</button>
            </span>
          {/each}
          <select
            {disabled}
            value=""
            onchange={(e) => {
              const v = (e.target as HTMLSelectElement).value;
              if (v) setLayer(i, { mask: [...(layer.mask ?? []), v] });
            }}
          >
            <option value="">{(layer.mask ?? []).length === 0 ? "every bone - add…" : "add…"}</option>
            {#each bones.filter((b) => !(layer.mask ?? []).includes(b)) as b}<option value={b}>{b}</option>{/each}
          </select>
        </div>
      </div>
    </div>
  {/each}
  <button class="small" {disabled} onclick={addLayer}>+ Layer</button>
  <datalist id="skel-clips">
    {#each clips as c}<option value={c}></option>{/each}
  </datalist>
</div>

<style>
  .extras {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 4px 8px 8px;
    font-size: 11px;
  }
  .sub-title {
    font-weight: 600;
    margin-top: 6px;
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .plot {
    width: 200px;
    height: 200px;
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    touch-action: none;
    cursor: crosshair;
  }
  .plot line {
    stroke: var(--ed-line);
    stroke-width: 1;
  }
  .plot line.axis {
    stroke: var(--ed-line-strong);
  }
  .point circle {
    fill: var(--ed-accent);
    cursor: grab;
  }
  .point circle.weight {
    fill: color-mix(in srgb, var(--ed-accent) 20%, transparent);
    stroke: color-mix(in srgb, var(--ed-accent) 53%, transparent);
  }
  .point text {
    fill: var(--ed-text);
    font-size: 9px;
    pointer-events: none;
  }
  .pos circle {
    fill: color-mix(in srgb, var(--ed-err) 33%, transparent);
    stroke: var(--ed-err);
    cursor: grab;
  }
  .pos line {
    stroke: var(--ed-err);
    stroke-width: 1.5;
  }
  .rowline {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .rowline input[type="number"] {
    width: 54px;
  }
  .lbl {
    min-width: 50px;
    color: var(--ed-muted);
  }
  .clip {
    flex: 1;
    min-width: 0;
  }
  .name {
    flex: 1;
    min-width: 0;
  }
  .pct {
    min-width: 32px;
    text-align: right;
    color: var(--ed-muted);
  }
  .layer {
    border: 1px solid var(--ed-line);
    border-radius: 3px;
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
    flex: 1;
  }
  .chip {
    background: var(--ed-raised);
    border-radius: 8px;
    padding: 0 2px 0 6px;
  }
  .chip.missing {
    color: var(--ed-err);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .hint {
    color: var(--ed-faint);
  }
  .warn {
    color: var(--ed-warn);
  }
  input,
  select,
  button {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    font-size: 11.5px;
    padding: 3px 4px;
  }
  .small {
    align-self: flex-start;
    cursor: pointer;
  }
  .x {
    border: none;
    background: none;
    cursor: pointer;
    padding: 0 3px;
  }
</style>
