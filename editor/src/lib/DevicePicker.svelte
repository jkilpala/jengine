<script lang="ts">
  // Choose a phone to preview: the view letterboxes to its shape, draws its
  // notch and rounded corners, and lays the UI out clear of them.
  import { DEVICE_PROFILES } from "./devicePreview";
  import { devicePreview } from "./stores/devicePreviewStore";
</script>

<div class="device-picker" title="Preview on a device: its shape, its notch, and its safe area">
  <select
    value={$devicePreview.id ?? ""}
    onchange={(e) => {
      const v = (e.target as HTMLSelectElement).value;
      devicePreview.update((s) => ({ ...s, id: v || null }));
    }}
  >
    <option value="">No device</option>
    {#each DEVICE_PROFILES as p}
      <option value={p.id}>{p.label}</option>
    {/each}
  </select>
  {#if $devicePreview.id}
    <button
      type="button"
      class="orient"
      title={$devicePreview.portrait ? "Portrait - turn to landscape" : "Landscape - turn to portrait"}
      onclick={() => devicePreview.update((s) => ({ ...s, portrait: !s.portrait }))}
    >
      {$devicePreview.portrait ? "▯" : "▭"}
    </button>
  {/if}
</div>

<style>
  .device-picker {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  select {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 11px;
    padding: 2px 4px;
  }

  .orient {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 3px;
    color: var(--ed-text);
    font-size: 13px;
    line-height: 1;
    padding: 2px 6px;
    cursor: pointer;
  }
</style>
