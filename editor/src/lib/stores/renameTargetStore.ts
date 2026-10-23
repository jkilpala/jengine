import { writable } from "svelte/store";

/**
 * Which tree F2 renames in.
 *
 * The asset browser and the scene tree can both hold a selection at the same
 * time, so a single window-level F2 needs to know which one the user means.
 * The answer is the one they last clicked in - the same rule every editor
 * with two trees uses, and the only one that needs no extra UI to explain.
 *
 * Null until the user has clicked either tree, so F2 before any click does
 * nothing rather than renaming something off-screen.
 */
export type RenameTarget = "asset" | "entity";

export const lastRenameTarget = writable<RenameTarget | null>(null);

/** Called by a tree when one of its rows is clicked. */
export function claimRenameTarget(kind: RenameTarget) {
  lastRenameTarget.set(kind);
}
