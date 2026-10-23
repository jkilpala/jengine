import { writable, derived } from "svelte/store";

/** Set of all selected entity indices. */
export const selectedEntities = writable<Set<number>>(new Set());

/**
 * Whether the scene itself is the current inspection target (the scene row
 * at the top of the Scene panel). Mutually exclusive with an entity
 * selection: the Inspector shows scene-level settings — sort layers,
 * post-processing — while this is set.
 */
export const sceneSelected = writable(false);

/** Index of the primary (last-clicked) selected entity, or -1 if none selected. */
export const selectedEntity = derived(selectedEntities, ($set) => {
  if ($set.size === 0) return -1;
  return Math.max(...$set);
});

/** Select the scene itself — shows scene settings in the Inspector. */
export function selectScene() {
  selectedEntities.set(new Set());
  sceneSelected.set(true);
}

/** Replace selection with a single entity (or -1 to clear). */
export function selectEntity(index: number) {
  sceneSelected.set(false);
  selectedEntities.set(index < 0 ? new Set() : new Set([index]));
}

/** Toggle an entity in/out of the selection (Ctrl+Click). */
export function toggleEntity(index: number) {
  sceneSelected.set(false);
  selectedEntities.update((s) => {
    const next = new Set(s);
    if (next.has(index)) next.delete(index);
    else next.add(index);
    return next;
  });
}

/** Select a range of entities (Shift+Click). */
export function selectRange(from: number, to: number) {
  sceneSelected.set(false);
  const min = Math.min(from, to);
  const max = Math.max(from, to);
  const set = new Set<number>();
  for (let i = min; i <= max; i++) set.add(i);
  selectedEntities.set(set);
}

/** Clear all selection, including the scene row. */
export function clearSelection() {
  sceneSelected.set(false);
  selectedEntities.set(new Set());
}

/** Select a list of entities at once (a box drag), or add them to the
 *  selection (Ctrl held). */
export function selectEntities(indices: number[], add = false) {
  sceneSelected.set(false);
  selectedEntities.update((s) => {
    const next = add ? new Set(s) : new Set<number>();
    for (const i of indices) next.add(i);
    return next;
  });
}
