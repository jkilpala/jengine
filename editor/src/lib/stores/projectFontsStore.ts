import { derived } from "svelte/store";
import { project, assetRefreshRequest } from "./projectStore";
import { listAssets } from "../api/project";

/**
 * Every font asset in the open project, as project-relative paths: `.font`
 * descriptors and the `.ttf` / `.otf` files the engine bakes glyphs from as a
 * text asks for them (`renderer::dynamic_font`).
 *
 * These are exactly the strings a `font_id` takes — the extension is kept,
 * because the engine resolves the `.font` and derives its atlas from the
 * sibling stem, and tells a font file by its extension. Anything that trims
 * the extension here produces an id no runtime can load.
 *
 * Shared rather than re-derived per inspector: the scene `TextComponent`, the
 * two `UIElement` widgets and the `.gui` node editor all offer the same list,
 * and four copies of the filter would drift.
 *
 * Recomputes when the project changes and whenever something signals an asset
 * refresh, so a font created or deleted during the session shows up without
 * reopening the project.
 */
/**
 * Whether `fontId` means the engine's built-in face rather than a project asset.
 *
 * Empty is the documented spelling; `"default"` is an older one that scenes
 * were authored with before anything read `font_id`. The runtime treats both
 * as built-in (`renderer::font_asset::is_builtin_font_id`), so the inspector
 * must not label one of them "(missing)" - it renders fine.
 */
export function isBuiltinFontId(fontId: string): boolean {
  return fontId === "" || fontId === "default";
}

/**
 * Whether a project file is a font a `font_id` can name. The same rule as the
 * runtime's (`dynamic_font::is_font_file` for `.ttf` / `.otf`): a font file
 * left out here was labelled "(missing)" in every font picker while it
 * rendered fine.
 */
export function isFontAsset(path: string): boolean {
  const lower = path.toLowerCase();
  return lower.endsWith(".font") || lower.endsWith(".ttf") || lower.endsWith(".otf");
}

export const projectFontIds = derived<
  [typeof project, typeof assetRefreshRequest],
  string[]
>(
  [project, assetRefreshRequest],
  ([$project], set) => {
    const path = $project?.path;
    if (!path) {
      set([]);
      return;
    }
    // A refresh landing while a previous listing is in flight would otherwise
    // let the slower response overwrite the newer one.
    let cancelled = false;
    listAssets(path)
      .then((files) => {
        if (cancelled) return;
        set(files.filter(isFontAsset).sort());
      })
      .catch(() => {
        if (!cancelled) set([]);
      });
    return () => {
      cancelled = true;
    };
  },
  [],
);
