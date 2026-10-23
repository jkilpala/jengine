/**
 * The Lua debugger (`luaDebugger.ts`) for the editor: its state as a Svelte
 * store, its pause tied to the viewport's (a breakpoint pauses Play as the
 * toolbar's Pause does, and going on releases it), its breakpoints kept for
 * the next session (per person, in this browser's storage), and its keys -
 * F8 continue, F10 step over, F11 step into, Shift+F11 step out - while it
 * is stopped.
 */
import { readable } from "svelte/store";
import { luaDebugger, type Pause } from "../luaDebugger";
import { isPaused } from "./playStore";

export interface DebuggerState {
  pause: Pause | null;
  breakpoints: { source: string; line: number }[];
}

const snapshot = (): DebuggerState => ({ pause: luaDebugger.pause, breakpoints: luaDebugger.allBreakpoints() });

export const debuggerState = readable<DebuggerState>(snapshot(), (set) => luaDebugger.subscribe(() => set(snapshot())));

// ── Breakpoints kept between sessions ──

const KEY = "jengine.editor.breakpoints";
try {
  const saved = JSON.parse(localStorage.getItem(KEY) ?? "[]") as { source: string; line: number }[];
  const bySource = new Map<string, number[]>();
  for (const b of saved) bySource.set(b.source, [...(bySource.get(b.source) ?? []), b.line]);
  for (const [source, lines] of bySource) luaDebugger.setBreakpoints(source, lines);
} catch {
  // Unavailable or unreadable storage: start with none.
}

// ── The viewport's pause follows the debugger's ──

let heldPause = false;
luaDebugger.subscribe(() => {
  try {
    localStorage.setItem(KEY, JSON.stringify(luaDebugger.allBreakpoints()));
  } catch {
    // Not persisted; the breakpoints still hold for this session.
  }
  const stopped = luaDebugger.paused;
  if (stopped === heldPause) return;
  heldPause = stopped;
  void import("../viewport-wasm/jengine_editor_viewport.js")
    .then((wasm) => {
      if (!wasm.viewport_is_playing()) return;
      wasm.viewport_set_paused(stopped);
      isPaused.set(wasm.viewport_is_paused());
    })
    .catch(() => {});
});

/** Whether the debugger is what holds Play paused - the toolbar's resume
 *  then goes on through it, so the stopped script finishes first. */
export function debuggerHoldsPause(): boolean {
  return luaDebugger.paused;
}

// ── Keys while stopped ──

if (typeof window !== "undefined") {
  window.addEventListener(
    "keydown",
    (e) => {
      if (!luaDebugger.paused) return;
      let act: (() => void) | null = null;
      if (e.key === "F8") act = () => luaDebugger.continue();
      else if (e.key === "F10") act = () => luaDebugger.stepOver();
      else if (e.key === "F11" && e.shiftKey) act = () => luaDebugger.stepOut();
      else if (e.key === "F11") act = () => luaDebugger.stepInto();
      if (!act) return;
      e.preventDefault();
      e.stopPropagation();
      act();
    },
    true,
  );
}

