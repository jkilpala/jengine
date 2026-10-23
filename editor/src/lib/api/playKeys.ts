/**
 * Who a keystroke belongs to while play mode runs: the editor or the game.
 *
 * The game reads the keyboard from a `keydown` listener on `document`, which
 * the viewport WASM installs, and every editor shortcut listens on `window` -
 * so before this, one key press did both: W walked the player *and* picked
 * the move tool, Ctrl+Z moved the player and undid a scene edit, C toggled
 * collider outlines under a game that also used C. The rule is decided once
 * here and enforced by `installPlayKeyRouter`, rather than by every handler
 * checking play state its own way (several checked it, several did not).
 *
 * The rule:
 * - Not playing, or paused: the editor owns the keyboard, as always. (The
 *   WASM already ignores presses while paused.)
 * - Typing into a text field or the script editor: the field gets it, never
 *   the game - otherwise Ctrl+S in a script tab moves the player too. Ctrl+C
 *   over selected text (the console, say) is a copy, likewise.
 * - Running, Game view: only the play controls reach the editor (pause,
 *   step, stop). Everything else - Escape, F3, F12, Ctrl+anything - is the
 *   game's, because a game binds those too.
 * - Running, Scene view: the play controls plus the scene-view tools that do
 *   not edit the scene file (gizmo mode, focus, overlays, deselect) and the
 *   editor's navigation (palette, quick open, find, F1). Those keys are held
 *   back from the game; every other key still drives it.
 *
 * A game key also loses its browser default: Space or Enter would otherwise
 * press whichever toolbar button last had focus - Pause, typically - and
 * arrows and Tab would scroll a panel or move focus under the game.
 */

export type KeyRoute = "editor" | "game";

/** The parts of a `KeyboardEvent` the rule reads - plain data, so it tests. */
export interface KeyLike {
  key: string;
  code: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}

export interface PlayKeyState {
  playing: boolean;
  paused: boolean;
  view: "scene" | "game";
  /** The key is going into a text field or code editor. */
  editable: boolean;
  /** Some text is selected, so Ctrl+C is a copy. */
  textSelected: boolean;
}

/** Ctrl+Shift+P - pause / resume. */
export function isPauseKey(e: KeyLike): boolean {
  return e.ctrlKey && e.shiftKey && !e.altKey && e.code === "KeyP";
}

/** Ctrl+Alt+P - advance one frame. */
export function isStepKey(e: KeyLike): boolean {
  return e.ctrlKey && e.altKey && !e.shiftKey && e.code === "KeyP";
}

/**
 * Shift+F5 - stop. Visual Studio's "stop debugging": F5 alone is the
 * editor's reload, and Escape is the one key nearly every game binds.
 */
export function isStopKey(e: KeyLike): boolean {
  return e.shiftKey && !e.ctrlKey && !e.altKey && e.key === "F5";
}

/** The keys that reach the editor however play is going. */
export function isPlayControl(e: KeyLike): boolean {
  return isPauseKey(e) || isStepKey(e) || isStopKey(e);
}

/** Unmodified keys the Scene view keeps during running play. */
const SCENE_VIEW_KEYS = new Set([
  "KeyW", // move tool
  "KeyE", // rotate tool
  "KeyR", // scale tool
  "KeyT", // local / global
  "KeyF", // focus selection
  "KeyC", // collider outlines
  "KeyL", // lighting preview
  "KeyJ", // joint outlines
  "Escape", // deselect
  "F1", // shortcut reference
  "F3", // profiler overlay
]);

/** Navigation chords the Scene view keeps during running play. */
function isSceneViewChord(e: KeyLike): boolean {
  if (!e.ctrlKey || e.altKey || e.metaKey) return false;
  if (e.shiftKey) {
    // Command palette, find in files, replace in files.
    return e.code === "KeyA" || e.code === "KeyF" || e.code === "KeyH";
  }
  // Quick open, next document.
  return e.code === "KeyP" || e.key === "Tab";
}

export function routeKey(e: KeyLike, s: PlayKeyState): KeyRoute {
  if (!s.playing || s.paused) return "editor";
  if (s.editable) return "editor";
  if (s.textSelected && e.ctrlKey && !e.shiftKey && !e.altKey && e.code === "KeyC") {
    return "editor";
  }
  if (isPlayControl(e)) return "editor";
  if (s.view === "scene") {
    const plain = !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey;
    if (plain && SCENE_VIEW_KEYS.has(e.code)) return "editor";
    if (isSceneViewChord(e)) return "editor";
  }
  return "game";
}

// --- Enforcement ------------------------------------------------------------

/**
 * Property the WASM keydown listener reads: set on a press the editor kept,
 * so the game does not see it. Must match `EDITOR_KEY_FLAG` in
 * `jengine-editor-viewport`.
 */
export const EDITOR_KEY_FLAG = "__jengineEditorKey";

const routes = new WeakMap<Event, KeyRoute>();

/**
 * True when the running game owns this press. A handler that fires before
 * `document` - one bound on an element, which Svelte delegates to the mount
 * point - checks this; handlers on `window` never see a game key at all.
 */
export function isGameKey(e: Event): boolean {
  return routes.get(e) === "game";
}

/**
 * Install the router. `state` is read per keystroke.
 *
 * A capture listener on `window` runs before anything else on the path and
 * records the decision. A key the editor keeps during play is flagged for
 * the WASM listener to skip. A game key is stopped at `document`, after the
 * WASM listener there has read it and before any `window` handler does - so
 * stopping it earlier would starve the game, and later would be too late.
 */
export function installPlayKeyRouter(
  state: (e: KeyboardEvent) => PlayKeyState,
): () => void {
  const decide = (e: KeyboardEvent) => {
    const s = state(e);
    const route = routeKey(e, s);
    routes.set(e, route);
    if (route === "editor" && s.playing) {
      (e as unknown as Record<string, unknown>)[EDITOR_KEY_FLAG] = true;
    }
  };
  const fence = (e: KeyboardEvent) => {
    if (routes.get(e) !== "game") return;
    e.stopPropagation();
    e.preventDefault();
  };
  window.addEventListener("keydown", decide, { capture: true });
  document.addEventListener("keydown", fence);
  return () => {
    window.removeEventListener("keydown", decide, { capture: true });
    document.removeEventListener("keydown", fence);
  };
}
