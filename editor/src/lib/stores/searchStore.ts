import { writable } from "svelte/store";

/**
 * Seed text for the find-in-files box.
 *
 * Set before bumping [`searchFocusRequest`] to start a search from elsewhere
 * in the editor; leave it empty to focus the box without disturbing what the
 * user already typed.
 */
export const searchQuery = writable("");

/**
 * Bumped to focus and select the find-in-files input.
 *
 * A counter rather than a boolean because the panel has to react to *every*
 * request: pressing Ctrl+Shift+F twice with the panel already open must
 * re-select the text both times, and a boolean that is already `true` fires no
 * change. Starts at 0, which the panel treats as "never asked" so it does not
 * steal focus on first render.
 */
export const searchFocusRequest = writable(0);

/**
 * Whether the pending focus request also wants the replace row open.
 *
 * Read by the panel when it services a request, not watched on its own: a
 * plain Ctrl+Shift+F must leave the row exactly as the user had it, so this
 * only ever *opens* the row, and only when someone asked for replace.
 */
export const searchOpenReplace = writable(false);

/** Focus the find-in-files box, optionally replacing its contents. */
export function requestSearchFocus(seed?: string) {
  if (seed !== undefined) searchQuery.set(seed);
  searchOpenReplace.set(false);
  searchFocusRequest.update((n) => n + 1);
}

/** Focus find-in-files with the replace row open. */
export function requestReplaceFocus(seed?: string) {
  if (seed !== undefined) searchQuery.set(seed);
  searchOpenReplace.set(true);
  searchFocusRequest.update((n) => n + 1);
}
