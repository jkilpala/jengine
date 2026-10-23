/**
 * Light layers (Rust `LightLayers` on a sprite, `light_mask` on a light): a
 * light lights a sprite when the two share a bit. Eight layers, the low eight
 * bits. A light's mask is absent for every layer, which is the default and
 * what a file should keep saying.
 */
export const ALL_LAYERS = 0xffffffff;
export const LAYER_COUNT = 8;

/** Whether layer `i` (0-based) is in `mask`; absent is every layer. */
export function layerOn(mask: number | undefined, i: number): boolean {
  return (((mask ?? ALL_LAYERS) >>> i) & 1) === 1;
}

/** `mask` with layer `i` flipped - absent again once all eight are on. */
export function toggleLayer(mask: number | undefined, i: number): number | undefined {
  const flipped = (((mask ?? ALL_LAYERS) ^ (1 << i)) & 0xff) >>> 0;
  return flipped === 0xff ? undefined : flipped;
}
