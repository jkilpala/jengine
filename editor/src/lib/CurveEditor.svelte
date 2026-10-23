<script lang="ts">
  // A curve over 0..1 as a graph: `[[t, value], ...]`, exactly the list the
  // engine stores (`curves.ts`). Drag a key to move it, double-click the
  // graph to add one where you clicked, double-click or right-click a key
  // (or Delete) to remove it. The selected key's numbers are typed below,
  // and `{ }` swaps to the JSON text for pasting. A drag reports one change
  // when it ends, so it is one undo step.
  import {
    formatKeys,
    insertKey,
    moveKey,
    parseKeys,
    removeKey,
    sample,
    sortKeys,
    valueRange,
    type Key,
  } from "./curves";

  let {
    keys,
    fallback = 1,
    disabled = false,
    onchange,
  }: {
    keys: Key[] | undefined | null;
    /** The value the engine uses when there are no keys. */
    fallback?: number;
    disabled?: boolean;
    onchange: (keys: Key[]) => void;
  } = $props();

  const H = 84;
  const PAD = 7;

  let width = $state(220);
  let draft = $state<Key[] | null>(null);
  let selected = $state<number | null>(null);
  let dragRange = $state<{ min: number; max: number } | null>(null);
  let moved = false;
  let showText = $state(false);
  let textError = $state("");
  let svg: SVGSVGElement | undefined = $state();

  let shown = $derived(draft ?? sortKeys(keys ?? []));
  let range = $derived(dragRange ?? valueRange(shown));
  let sel = $derived(selected != null && selected < shown.length ? shown[selected] : null);

  const x = (t: number) => PAD + t * (width - 2 * PAD);
  const y = (v: number) => PAD + (1 - (v - range.min) / (range.max - range.min)) * (H - 2 * PAD);

  function pointerToKey(e: PointerEvent | MouseEvent): [number, number] {
    const r = svg!.getBoundingClientRect();
    const t = (e.clientX - r.left - PAD) / (r.width - 2 * PAD);
    const fy = (e.clientY - r.top - PAD) / (r.height - 2 * PAD);
    const v = range.min + (1 - fy) * (range.max - range.min);
    return [Math.min(1, Math.max(0, t)), Math.round(v * 1000) / 1000];
  }

  let path = $derived.by(() => {
    if (shown.length === 0) return `M ${x(0)} ${y(fallback)} L ${x(1)} ${y(fallback)}`;
    const pts = [[0, shown[0][1]], ...shown, [1, shown[shown.length - 1][1]]];
    return pts.map((k, i) => `${i === 0 ? "M" : "L"} ${x(k[0])} ${y(k[1])}`).join(" ");
  });

  let gridLines = $derived.by(() => {
    const lines: { v: number; label: string }[] = [];
    for (const v of [range.min, 0, 1, range.max]) {
      if (v >= range.min && v <= range.max && !lines.some((l) => Math.abs(l.v - v) < 1e-6)) {
        lines.push({ v, label: String(Math.round(v * 100) / 100) });
      }
    }
    return lines;
  });

  function commit(next: Key[]) {
    draft = null;
    dragRange = null;
    onchange(next);
  }

  function startDrag(e: PointerEvent, index: number) {
    if (disabled || e.button !== 0) return;
    e.stopPropagation();
    selected = index;
    draft = shown.map((k) => [...k]);
    dragRange = range;
    moved = false;
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  }

  function drag(e: PointerEvent) {
    if (!draft || selected == null) return;
    const [t, v] = pointerToKey(e);
    const next = moveKey(draft, selected, t, [v]);
    draft = next.keys;
    selected = next.index;
    moved = true;
  }

  function endDrag() {
    if (!draft) return;
    if (moved) commit(draft);
    else {
      draft = null;
      dragRange = null;
    }
  }

  function addAt(e: MouseEvent) {
    if (disabled) return;
    const [t, v] = pointerToKey(e);
    const added = insertKey(shown, t, [fallback], [v]);
    selected = added.index;
    commit(added.keys);
  }

  function remove(index: number) {
    if (disabled) return;
    selected = null;
    commit(removeKey(shown, index));
  }

  function setSelected(field: 0 | 1, value: number) {
    if (selected == null || !sel || !Number.isFinite(value)) return;
    const next =
      field === 0 ? moveKey(shown, selected, value) : moveKey(shown, selected, sel[0], [value]);
    selected = next.index;
    commit(next.keys);
  }

  function setText(text: string) {
    const parsed = parseKeys(text, 2);
    if ("error" in parsed) {
      textError = parsed.error;
      return;
    }
    textError = "";
    selected = null;
    commit(parsed.keys);
  }

  function onKey(e: KeyboardEvent) {
    if ((e.key === "Delete" || e.key === "Backspace") && selected != null) {
      e.preventDefault();
      remove(selected);
    }
  }
</script>

<div class="curve" bind:clientWidth={width}>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <svg
    bind:this={svg}
    {width}
    height={H}
    tabindex="0"
    role="application"
    aria-label="Curve: double-click to add a key"
    class:disabled
    ondblclick={addAt}
    onkeydown={onKey}
  >
    {#each gridLines as g}
      <line class="grid" x1={x(0)} x2={x(1)} y1={y(g.v)} y2={y(g.v)} />
      <text class="axis" x={x(0) + 2} y={y(g.v) - 2}>{g.label}</text>
    {/each}
    <line class="grid" x1={x(0.5)} x2={x(0.5)} y1={PAD} y2={H - PAD} />
    <path class="line" class:empty={shown.length === 0} d={path} />
    {#each shown as k, i}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <circle
        class="key"
        class:selected={i === selected}
        cx={x(k[0])}
        cy={y(k[1])}
        r={i === selected ? 5 : 4}
        onpointerdown={(e) => startDrag(e, i)}
        onpointermove={drag}
        onpointerup={endDrag}
        ondblclick={(e) => {
          e.stopPropagation();
          remove(i);
        }}
        oncontextmenu={(e) => {
          e.preventDefault();
          remove(i);
        }}
      >
        <title>t {k[0].toFixed(3)}, {k[1].toFixed(3)} - drag to move, double-click to remove</title>
      </circle>
    {/each}
  </svg>
  <div class="row">
    {#if sel}
      <span class="dim">t</span>
      <input
        type="number"
        step="0.01"
        min="0"
        max="1"
        value={sel[0]}
        {disabled}
        onchange={(e) => setSelected(0, parseFloat((e.target as HTMLInputElement).value))}
      />
      <span class="dim">value</span>
      <input
        type="number"
        step="0.05"
        value={sel[1]}
        {disabled}
        onchange={(e) => setSelected(1, parseFloat((e.target as HTMLInputElement).value))}
      />
      <button title="Remove this key" {disabled} onclick={() => selected != null && remove(selected)}>×</button>
    {:else}
      <span class="hint"
        >{shown.length === 0
          ? `No keys: ${fallback} all along. Double-click to add one.`
          : `${shown.length} keys; at 0.5: ${sample(shown, 0.5, [fallback])[0].toFixed(2)}`}</span
      >
    {/if}
    <button class="text-toggle" class:on={showText} title="Edit as JSON text" onclick={() => (showText = !showText)}
      >{"{ }"}</button
    >
  </div>
  {#if showText}
    <textarea
      rows="3"
      spellcheck="false"
      placeholder="[[0, 0.5], [0.2, 1], [1, 1.5]]"
      value={formatKeys(shown)}
      {disabled}
      onchange={(e) => setText((e.target as HTMLTextAreaElement).value)}
    ></textarea>
    {#if textError}<span class="error">{textError}</span>{/if}
  {/if}
</div>

<style>
  .curve {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
    flex: 1;
  }
  svg {
    display: block;
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    border-radius: 3px;
    cursor: crosshair;
    outline: none;
  }
  svg:focus-visible {
    border-color: var(--ed-accent);
  }
  svg.disabled {
    opacity: 0.6;
    pointer-events: none;
  }
  .grid {
    stroke: var(--ed-line);
    stroke-width: 1;
  }
  .axis {
    fill: var(--ed-faint);
    font-size: 9px;
  }
  .line {
    fill: none;
    stroke: var(--ed-accent);
    stroke-width: 1.5;
  }
  .line.empty {
    stroke-dasharray: 3 3;
    opacity: 0.6;
  }
  .key {
    fill: var(--ed-base);
    stroke: var(--ed-accent);
    stroke-width: 1.5;
    cursor: grab;
  }
  .key.selected {
    fill: var(--ed-accent);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    min-height: 22px;
  }
  .row input {
    width: 58px;
    min-width: 0;
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    padding: 2px 4px;
    border-radius: 4px;
    font-size: 12px;
  }
  .dim,
  .hint {
    font-size: 10px;
    color: var(--ed-muted);
  }
  .hint {
    flex: 1;
  }
  button {
    background: none;
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    font-size: 11.5px;
    padding: 0 5px;
    cursor: pointer;
  }
  .text-toggle {
    margin-left: auto;
    font-family: var(--ed-mono);
  }
  .text-toggle.on {
    border-color: var(--ed-accent);
  }
  textarea {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    padding: 2px 4px;
    border-radius: 4px;
    font-family: var(--ed-mono);
    font-size: 11px;
    resize: vertical;
  }
  .error {
    color: var(--ed-err);
    font-size: 10px;
  }
</style>
