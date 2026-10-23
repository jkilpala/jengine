<script lang="ts">
  // What covers a device's screen, drawn over a view letterboxed to it: the
  // rounded corners, the cut-out, and a dashed outline of the safe area the
  // UI is laid out in. Never takes the mouse.
  import {
    cornerRadius,
    cutoutShapes,
    safeRect,
    type DeviceProfile,
  } from "./devicePreview";

  let {
    profile,
    portrait,
    x,
    y,
    width,
    height,
  }: {
    profile: DeviceProfile;
    portrait: boolean;
    x: number;
    y: number;
    width: number;
    height: number;
  } = $props();

  const r = $derived(cornerRadius(profile, width, height));
  const shapes = $derived(cutoutShapes(profile, portrait, width, height));
  const safe = $derived(safeRect(profile, portrait, width, height));
  // The screen with its corners rounded, cut out of a rect that covers
  // everything: filled even-odd, what is left is the four corners.
  const cornerMask = $derived(
    `M0 0H${width}V${height}H0Z ` +
      `M${r} 0H${width - r}A${r} ${r} 0 0 1 ${width} ${r}V${height - r}` +
      `A${r} ${r} 0 0 1 ${width - r} ${height}H${r}A${r} ${r} 0 0 1 0 ${height - r}` +
      `V${r}A${r} ${r} 0 0 1 ${r} 0Z`,
  );
</script>

<svg
  class="device-overlay"
  style="left: {x}px; top: {y}px; width: {width}px; height: {height}px;"
  viewBox="0 0 {width} {height}"
  aria-hidden="true"
>
  {#if r > 0}
    <path d={cornerMask} fill="#000" fill-rule="evenodd" />
  {/if}
  {#each shapes as s}
    <rect x={s.x} y={s.y} width={s.width} height={s.height} rx={s.radius} fill="#000" />
  {/each}
  {#if safe.x > 0 || safe.y > 0 || safe.width < width || safe.height < height}
    <rect
      x={safe.x + 0.5}
      y={safe.y + 0.5}
      width={Math.max(0, safe.width - 1)}
      height={Math.max(0, safe.height - 1)}
      fill="none"
      stroke="var(--ed-a2)"
      stroke-width="1"
      stroke-dasharray="6 4"
    />
    <text x={safe.x + 6} y={safe.y + 14} class="label">safe area</text>
  {/if}
</svg>

<style>
  .device-overlay {
    position: absolute;
    pointer-events: none;
    overflow: visible;
    z-index: 5;
  }

  .label {
    fill: var(--ed-a2);
    font-size: 10px;
    font-family: inherit;
  }
</style>
