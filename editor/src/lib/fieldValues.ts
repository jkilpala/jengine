/**
 * Value helpers for the derived inspector (`PackageInspector.svelte`): the
 * text form of a field with no widget of its own (`Json`), and the project
 * files an asset field can name (`AssetRef`).
 */

/** A field's value as editable text: pretty JSON, empty when unset. */
export function jsonText(value: unknown): string {
  if (value === undefined) return "";
  const flat = JSON.stringify(value);
  return flat.length <= 48 ? flat : JSON.stringify(value, null, 2);
}

/** Parse a `Json` field's text. Empty text unsets the field (`undefined`),
 *  which leaves it out of the component as the engine's default does. */
export function parseJsonText(text: string): { value: unknown } | { error: string } {
  const trimmed = text.trim();
  if (trimmed === "") return { value: undefined };
  try {
    return { value: JSON.parse(trimmed) };
  } catch (err) {
    return { error: String((err as Error).message ?? err) };
  }
}

/** The files among `files` with one of `extensions` (no dot, any case),
 *  sorted, with `/` separators - what an asset field offers to pick. */
export function assetsWithExtensions(files: readonly string[], extensions: readonly string[]): string[] {
  const wanted = new Set(extensions.map((e) => e.toLowerCase().replace(/^\./, "")));
  return files
    .map((f) => f.replace(/\\/g, "/"))
    .filter((f) => {
      const dot = f.lastIndexOf(".");
      return dot >= 0 && wanted.has(f.slice(dot + 1).toLowerCase());
    })
    .sort();
}
