/**
 * Every texture a `sprite_library` can swap in (`ecs::sprite_library`):
 * nothing names a part until a resolver wears it, so the viewport has to
 * load them all with the scene, as `scene::format::world_texture_ids` does
 * natively.
 */
export function spriteLibraryTextures(components: unknown): string[] {
  const lib = (components as { sprite_library?: { categories?: unknown } } | null)?.sprite_library;
  const categories = lib?.categories;
  if (!categories || typeof categories !== "object") return [];
  const out: string[] = [];
  for (const labels of Object.values(categories as Record<string, unknown>)) {
    if (!labels || typeof labels !== "object") continue;
    for (const entry of Object.values(labels as Record<string, unknown>)) {
      const id = (entry as { texture_id?: unknown } | null)?.texture_id;
      if (typeof id === "string" && id) out.push(id);
    }
  }
  return out;
}
