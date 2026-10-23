<script lang="ts">
  // Colour stops over 0..1: `[[t, r, g, b, a], ...]`, linear colours, exactly
  // the list the engine stores (`curves.ts`). The bar shows the gradient over
  // a checkerboard; drag a stop's marker to move it, double-click the bar to
  // add a stop in the colour already there, and edit the selected stop's
  // colour, alpha and intensity below (intensity keeps an HDR colour brighter
  // than 1, which a colour picker cannot hold). `{ }` swaps to the JSON text.
  // A drag reports one change when it ends, so it is one undo step.
  import {
    cssGradient,
    formatKeys,
    hexOf,
    insertKey,
    moveKey,
    parseKeys,
    removeKey,
    rgbOf,
    sortKeys,
    splitIntensity,
    type Key,
  } from "./curves";

  let {
    keys,
    fallback = [1, 1, 1, 1],
    disabled = false,
    onchange,
  }: {
    keys: Key[] | undefined | null;
    /** The colour the engine uses when there are no stops. */
    fallback?: number[];
    disabled?: boolean;
    onchange: (keys: Key[]) => void;
  } = $props();

  let draft = $state<Key[] | null>(null);
  let selected = $state<number | null>(null);
  let moved = false;
  let showText = $state(false);
  let textError = $state("");
  let bar: HTMLDivElement | undefined = $state();

  let shown = $derived(draft ?? sortKeys(keys ?? []));
  let sel = $derived(selected != null && selected < shown.length ? shown[selected] : null);
  let selColour = $derived(sel ? splitIntensity(sel.slice(1, 4)) : null);
  let background = $derived(cssGradient(shown, fallback));

  function tAt(clientX: number): number {
    const r = bar!.getBoundingClientRect();
    return Math.min(1, Math.max(0, (clientX - r.left) / r.width));
  }

  function commit(next: Key[]) {
    draft = null;
    onchange(next);
  }

  function startDrag(e: PointerEvent, index: number) {
    if (disabled || e.button !== 0) return;
    e.stopPropagation();
    selected = index;
    draft = shown.map((k) => [...k]);
    moved = false;
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  }

  function drag(e: PointerEvent) {
    if (!draft || selected == null) return;
    const next = moveKey(draft, selected, Math.round(tAt(e.clientX) * 1000) / 1000);
    draft = next.keys;
    selected = next.index;
    moved = true;
  }

  function endDrag() {
    if (!draft) return;
    if (moved) commit(draft);
    else draft = null;
  }

  function addAt(e: MouseEvent) {
    if (disabled) return;
    const added = insertKey(shown, Math.round(tAt(e.clientX) * 1000) / 1000, fallback.slice(0, 4));
    selected = added.index;
    commit(added.keys);
  }

  function remove(index: number) {
    if (disabled) return;
    selected = null;
    commit(removeKey(shown, index));
  }

  /** Replace the selected stop's colour with `rgb * intensity` and `alpha`. */
  function setColour(rgb: number[], intensity: number, alpha: number) {
    if (selected == null || !sel) return;
    const values = [...rgb.map((c) => c * intensity), Math.min(1, Math.max(0, alpha))];
    const next = moveKey(shown, selected, sel[0], values);
    selected = next.index;
    commit(next.keys);
  }

  function setTime(t: number) {
    if (selected == null || !Number.isFinite(t)) return;
    const next = moveKey(shown, selected, t);
    selected = next.index;
    commit(next.keys);
  }

  function setText(text: string) {
    const parsed = parseKeys(text, 5);
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

  const num = (e: Event, fallbackValue: number) => {
    const v = parseFloat((e.target as HTMLInputElement).value);
    return Number.isFinite(v) ? v : fallbackValue;
  };
</script>

<div class="gradient" class:disabled>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="track"
    tabindex="0"
    role="application"
    aria-label="Gradient: double-click to add a stop"
    onkeydown={onKey}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="bar" bind:this={bar} ondblclick={addAt} title="Double-click to add a stop">
      <div class="fill" style:background-image={background}></div>
    </div>
    <div class="markers">
      {#each shown as k, i}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="marker"
          class:selected={i === selected}
          style:left="{k[0] * 100}%"
          style:--swatch={hexOf(splitIntensity(k.slice(1, 4)).rgb)}
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
          title="t {k[0].toFixed(3)} - drag to move, double-click to remove"
        ></div>
      {/each}
    </div>
  </div>
  <div class="row">
    {#if sel && selColour}
      <input
        type="color"
        title="Colour (shown in sRGB, stored linear)"
        value={hexOf(selColour.rgb)}
        {disabled}
        onchange={(e) => setColour(rgbOf((e.target as HTMLInputElement).value), selColour.intensity, sel[4])}
      />
      <span class="dim" title="Alpha">a</span>
      <input
        type="number"
        step="0.05"
        min="0"
        max="1"
        value={sel[4]}
        {disabled}
        onchange={(e) => setColour(selColour.rgb, selColour.intensity, num(e, sel[4]))}
      />
      <span class="dim" title="Intensity: the colour times this - above 1 for an HDR glow">×</span>
      <input
        type="number"
        step="0.1"
        min="0"
        value={Math.round(selColour.intensity * 1000) / 1000}
        {disabled}
        onchange={(e) => setColour(selColour.rgb, Math.max(0, num(e, 1)), sel[4])}
      />
      <span class="dim">t</span>
      <input
        type="number"
        step="0.01"
        min="0"
        max="1"
        value={sel[0]}
        {disabled}
        onchange={(e) => setTime(num(e, sel[0]))}
      />
      <button title="Remove this stop" {disabled} onclick={() => selected != null && remove(selected)}>×</button>
    {:else}
      <span class="hint"
        >{shown.length === 0 ? "No stops. Double-click the bar to add one." : `${shown.length} stops`}</span
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
      placeholder="[[0, 1, 1, 1, 0], [0.2, 1, 1, 1, 1], [1, 1, 1, 1, 0]]"
      value={formatKeys(shown)}
      {disabled}
      onchange={(e) => setText((e.target as HTMLTextAreaElement).value)}
    ></textarea>
    {#if textError}<span class="error">{textError}</span>{/if}
  {/if}
</div>

<style>
  .gradient {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
    flex: 1;
  }
  .gradient.disabled {
    opacity: 0.6;
    pointer-events: none;
  }
  .track {
    outline: none;
    padding: 0 6px;
  }
  .track:focus-visible .bar {
    border-color: var(--ed-accent);
  }
  .bar {
    position: relative;
    height: 20px;
    border: 1px solid var(--ed-line);
    border-radius: 3px;
    cursor: copy;
    background-color: var(--ed-line);
    background-image:
      linear-gradient(45deg, var(--ed-line-strong) 25%, transparent 25%),
      linear-gradient(-45deg, var(--ed-line-strong) 25%, transparent 25%),
      linear-gradient(45deg, transparent 75%, var(--ed-line-strong) 75%),
      linear-gradient(-45deg, transparent 75%, var(--ed-line-strong) 75%);
    background-size: 10px 10px;
    background-position:
      0 0,
      0 5px,
      5px -5px,
      -5px 0;
  }
  .fill {
    position: absolute;
    inset: 0;
    border-radius: 2px;
  }
  .markers {
    position: relative;
    height: 12px;
  }
  .marker {
    position: absolute;
    top: 1px;
    width: 9px;
    height: 10px;
    margin-left: -5px;
    background: var(--swatch);
    border: 1px solid var(--ed-muted);
    border-radius: 2px;
    cursor: ew-resize;
  }
  .marker.selected {
    border: 2px solid var(--ed-accent);
    top: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    min-height: 22px;
  }
  .row input[type="number"] {
    width: 46px;
    min-width: 0;
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    padding: 2px 4px;
    border-radius: 4px;
    font-size: 12px;
  }
  .row input[type="color"] {
    width: 26px;
    height: 20px;
    padding: 0;
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    cursor: pointer;
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
