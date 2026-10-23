import { writable, get } from "svelte/store";

/**
 * One thing the editor can do, addressable by name.
 *
 * The palette is only as good as this list, so an action is worth registering
 * whenever a menu item or shortcut exists — the point is that someone who does
 * not know the shortcut can still find the command.
 */
export interface EditorAction {
  /** Stable identifier, `category.verb-noun`. Used for dedupe, not display. */
  id: string;
  /** What the palette shows, in Title Case. */
  title: string;
  /** Grouping label shown dimmed beside the title. */
  category: string;
  /** Shortcut to display. Purely informational — binding happens elsewhere. */
  keys?: string;
  run: () => void | Promise<void>;
  /**
   * When present and false, the action is shown greyed and cannot be run.
   * Evaluated as the palette renders, so it must be cheap and side-effect
   * free.
   */
  enabled?: () => boolean;
}

/** Every registered action, in registration order. */
export const actions = writable<EditorAction[]>([]);

/**
 * Register actions, returning a function that removes exactly these again.
 *
 * Registration is additive and reversible so a panel can contribute its own
 * commands while it is mounted without leaking them once it closes. Re-adding
 * an id that is already present replaces it rather than duplicating, which is
 * what happens when a component remounts.
 */
export function registerActions(list: EditorAction[]): () => void {
  const ids = new Set(list.map((a) => a.id));
  actions.update((prev) => [...prev.filter((a) => !ids.has(a.id)), ...list]);
  return () => {
    actions.update((prev) => prev.filter((a) => !ids.has(a.id)));
  };
}

/** Run an action by id. Returns false if it is unknown or disabled. */
export function runAction(id: string): boolean {
  const action = get(actions).find((a) => a.id === id);
  if (!action || action.enabled?.() === false) return false;
  void action.run();
  return true;
}
