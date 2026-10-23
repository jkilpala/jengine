import { spriteLibraryTextures } from "./spriteLibrary";

/**
 * Whether a texture id names a file the editor should read. An id starting
 * `__` (`__white`, a view's `__screen/..`, `__pixel_sim/..`) is made by the
 * engine and never on disk - the rule `asset::textures_to_request` and the
 * native and web loaders follow. Reading one logged "Asset not found:
 * __white" for the very first scene of a new project.
 */
export function isFileTexture(id: unknown): id is string {
  return typeof id === "string" && id.length > 0 && !id.startsWith("__");
}

/**
 * Every texture one prefab will need once something spawns it - its own and
 * its parts' (`children`, a prefab that is a hierarchy), as
 * `scene::format::prefab_texture_ids` does natively. A prefab's textures are
 * invisible to the scene scan, which only walks scene entities: a prefab
 * with art of its own spawned untextured, and a wagon's barrels are named by
 * nothing but the wagon.
 */
export function prefabTextureIds(prefab: unknown): string[] {
  const out = new Set<string>();
  const todo: unknown[] = [prefab];
  while (todo.length) {
    const node = todo.pop() as { components?: Record<string, any>; children?: unknown } | null;
    if (!node || typeof node !== "object") continue;
    if (Array.isArray(node.children)) todo.push(...node.children);
    const comps = node.components ?? {};
    const add = (id: unknown) => {
      if (isFileTexture(id)) out.add(id);
    };
    add(comps.sprite?.texture_id);
    for (const clip of Object.values((comps.sprite_animator?.clips ?? {}) as Record<string, any>)) {
      for (const frame of clip?.frames ?? []) add(frame?.texture_id);
    }
    // A spawned lit sprite's normal map, a lightmap and a light's cookie,
    // for the same reason the scene scan collects them.
    add(comps.material?.normal_map_id);
    add(comps.lightmap?.texture_id);
    add(comps.point_light?.cookie);
    add(comps.spot_light?.cookie);
    for (const id of spriteLibraryTextures(comps)) add(id);
  }
  return [...out];
}
