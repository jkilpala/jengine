/**
 * Reloading the editor to pick up another viewport module, without dropping
 * the user back on the start screen.
 *
 * The viewport WASM is instantiated once per page, so loading a project's own
 * build (or a fresh rebuild) needs a reload - and a reload starts at the start
 * screen, with no project open. The project to reopen is left in
 * `sessionStorage`, which survives a reload of the same window and nothing
 * else, and the start screen takes it on mount.
 */

const KEY = "jengine.reopenProjectAfterViewportReload";

/**
 * Ask for `projectPath` to be reopened after the next reload. Call it before
 * the module files change, not after: Vite watches `src/`, and may reload the
 * page itself the moment they do.
 */
export function markReopen(projectPath: string): void {
  try {
    sessionStorage.setItem(KEY, projectPath);
  } catch {
    // No storage: the reload still loads the module, the user reopens by hand.
  }
}

/** Withdraw `markReopen`, when the files did not change after all. */
export function clearReopen(): void {
  try {
    sessionStorage.removeItem(KEY);
  } catch {
    // Nothing to clear.
  }
}

/** Reload the page and reopen `projectPath` once it is back. */
export function reloadIntoProject(projectPath: string): void {
  markReopen(projectPath);
  location.reload();
}

/** The project a viewport reload asked to reopen, cleared as it is read. */
export function takeReopenProject(): string | null {
  try {
    const path = sessionStorage.getItem(KEY);
    sessionStorage.removeItem(KEY);
    return path;
  } catch {
    return null;
  }
}
