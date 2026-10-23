import { invoke } from "@tauri-apps/api/core";

/**
 * Open or close the webview devtools.
 *
 * The editor disables WebView2's browser accelerator keys at startup (so
 * F3 / Ctrl+F / Ctrl+R stop shadowing editor shortcuts), which takes F12 and
 * Ctrl+Shift+I with them — this command is how the Debug menu and those key
 * bindings get devtools back.
 */
export function toggleDevtools(): Promise<void> {
  return invoke("toggle_devtools");
}

/**
 * The editor draws its own title bar (the toolbar), so its window buttons
 * call these. Closing goes through `close()`, which raises the same
 * `onCloseRequested` the unsaved-work guard below listens to.
 */
export async function minimizeWindow(): Promise<void> {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().minimize();
}

export async function toggleMaximizeWindow(): Promise<void> {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().toggleMaximize();
}

export async function closeWindow(): Promise<void> {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().close();
}

/** Call `fn` with whether the window is maximized now and after every
 *  resize. Returns the unlisten function. */
export async function watchMaximized(fn: (maximized: boolean) => void): Promise<() => void> {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const appWindow = getCurrentWindow();
  fn(await appWindow.isMaximized());
  return appWindow.onResized(async () => fn(await appWindow.isMaximized()));
}

/** On macOS the native traffic lights stay (overlaid on the toolbar's left),
 *  so the editor draws no window buttons of its own there. */
export const isMac = typeof navigator !== "undefined" && /Mac/i.test(navigator.userAgent);

/** Reload the editor window — the replacement for the browser's Ctrl+R. */
export function reloadEditorWindow() {
  window.location.reload();
}

/**
 * Intercept the window close so unsaved work can be dealt with first.
 *
 * `ask` is handed nothing and returns a promise: `true` to go ahead with the
 * close, `false` to stay open. It may take as long as it likes — show a
 * dialog, save files — because the close is **always** cancelled first and
 * re-issued afterwards. A synchronous guard could not do that, and a native
 * `confirm` inside a WebView is at the host's discretion: if it does not
 * render, the caller silently gets the default and the work is gone.
 *
 * Tauri fires `onCloseRequested` before destroying the window and honours
 * `preventDefault`, which is the only hook that can stop unsaved work going
 * out with it. `beforeunload` does not fire for a native window close.
 *
 * **This makes closing depend on `core:window:allow-destroy`.** Tauri stops
 * handling the close itself once a handler exists, so the frontend finishes
 * the job with `destroy()`. Without that permission in
 * `capabilities/default.json` the editor cannot be closed at all — the X does
 * nothing and the console shows a permission error.
 *
 * An `ask` that throws lets the window close. Being unable to shut the editor
 * is worse than losing the prompt.
 *
 * Returns the unlisten function.
 */
export async function guardWindowClose(
  ask: () => Promise<boolean>,
): Promise<() => void> {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const appWindow = getCurrentWindow();
  let closing = false;

  return appWindow.onCloseRequested(async (event) => {
    // The close we issue ourselves below comes back through here; letting it
    // through unchallenged is what actually shuts the window.
    if (closing) return;
    event.preventDefault();

    let proceed = true;
    try {
      proceed = await ask();
    } catch {
      proceed = true;
    }
    if (!proceed) return;

    closing = true;
    await appWindow.destroy();
  });
}
