/**
 * A texture's filter as the project sets it (Rust
 * `renderer::texture_filter`): `settings.texture_filter` is a default and
 * texture-id **prefix** overrides, the longest matching prefix winning - so
 * an override of one texture's whole id is that texture's own setting,
 * whatever folder rule it is under. The image preview's *Filter* picker is
 * that per-texture setting.
 */
export type FilterMode = "nearest" | "linear";

export interface TextureFilterSettings {
  default?: FilterMode;
  overrides?: Record<string, FilterMode>;
}

/** How texture `id` (its path under `assets/`) is sampled, and which rule
 *  says so (`null` for the project default). */
export function filterFor(tf: TextureFilterSettings | undefined, id: string): { mode: FilterMode; rule: string | null } {
  let best: string | null = null;
  for (const prefix of Object.keys(tf?.overrides ?? {})) {
    if (id.startsWith(prefix) && (best === null || prefix.length > best.length)) best = prefix;
  }
  return best !== null ? { mode: tf!.overrides![best], rule: best } : { mode: tf?.default ?? "nearest", rule: null };
}

/** The settings with texture `id`'s own filter set (`null` drops it, so the
 *  folder rules or the default apply again). Absent when nothing is left
 *  but the engine's own default - nearest - as the settings have it. */
export function withTextureFilter(
  tf: TextureFilterSettings | undefined,
  id: string,
  mode: FilterMode | null,
): TextureFilterSettings | undefined {
  const overrides = { ...(tf?.overrides ?? {}) };
  if (mode) overrides[id] = mode;
  else delete overrides[id];
  const out: TextureFilterSettings = { ...(tf ?? {}) };
  if (Object.keys(overrides).length > 0) out.overrides = overrides;
  else delete out.overrides;
  if ((out.default ?? "nearest") === "nearest" && !out.overrides) return undefined;
  return out;
}
