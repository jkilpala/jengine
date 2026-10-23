<script lang="ts">
    // Eight light layers as toggles (`lightLayers.ts`): a light's
    // `light_mask` or a sprite's `light_layers`. Absent is every layer.
    import { LAYER_COUNT, layerOn, toggleLayer } from "./lightLayers";

    let { mask, onchange }: { mask: number | undefined; onchange: (mask: number | undefined) => void } = $props();
</script>

<div class="layer-mask">
    {#each Array.from({ length: LAYER_COUNT }, (_, i) => i) as i}
        <button
            type="button"
            class:on={layerOn(mask, i)}
            title={`Light layer ${i + 1}`}
            onclick={() => onchange(toggleLayer(mask, i))}>{i + 1}</button
        >
    {/each}
</div>

<style>
    .layer-mask {
        display: flex;
        gap: 2px;
    }
    button {
        width: 20px;
        height: 18px;
        padding: 0;
        font-size: 10px;
        border: 1px solid var(--ed-line-strong);
        background: var(--ed-base);
        color: var(--ed-faint);
        border-radius: 4px;
        cursor: pointer;
    }
    button.on {
        background: var(--ed-accent);
        color: var(--ed-on-accent);
    }
</style>
