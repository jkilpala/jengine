/**
 * The Lua debugger: breakpoints, stepping and locals for scripts running in
 * the editor's Play mode (Fengari, `lua-runtime.ts`).
 *
 * How a breakpoint stops a game that runs in a synchronous frame: while the
 * debugger is **armed** (a breakpoint set, or a step under way) the runtime
 * calls each script callback as a coroutine, and a line hook on every script
 * state yields when it reaches a breakpoint. The callback returns to the
 * engine early; the debugger keeps the suspended coroutine, takes the call
 * stack and its locals, and says it is paused - the viewport stops ticking
 * and no other script runs. Continue (or a step) resumes the coroutine where
 * it stopped; when it finishes, the game runs again. Disarmed, scripts run as
 * before: no hook, no coroutine, no cost.
 *
 * Lua 5.3 lets a line hook yield (`luaG_traceexec`), which Fengari keeps.
 *
 * The runtime finds this object at `globalThis.__jengine_lua_debugger`; it is
 * installed by the editor only, so a web build's copy of the runtime never
 * sees one.
 */
import { lua, to_jsstring } from "fengari-web";

export interface Local {
  name: string;
  value: string;
}

export interface Frame {
  /** The script, as it was compiled (`scripts/player.lua`). */
  source: string;
  line: number;
  /** The function's name, as Lua knows it (`update`, `?`). */
  name: string;
  locals: Local[];
}

export interface Pause {
  source: string;
  line: number;
  reason: "breakpoint" | "step";
  /** Innermost first. */
  frames: Frame[];
}

type StepMode = "into" | "over" | "out";

interface Suspended {
  co: any;
  L: any;
  /** The callback's own error handling and cleanup, for when it ends. */
  finish: (status: number) => void;
}

/** A script label (`scripts/a.lua`, `C:\p\assets\scripts\a.lua`) as the key
 *  breakpoints are kept under: forward slashes, from `scripts/`-ish onwards -
 *  the part after `assets/` when there is one. */
export function scriptKey(path: string): string {
  const p = path.replace(/\\/g, "/").replace(/^=/, "");
  const i = p.lastIndexOf("/assets/");
  return i >= 0 ? p.slice(i + "/assets/".length) : p.replace(/^assets\//, "");
}

/** A Lua value as one line of text: strings quoted, a table's first few
 *  fields. */
function describe(L: any, idx: number, depth = 0): string {
  const t = lua.lua_type(L, idx);
  switch (t) {
    case lua.LUA_TNIL:
      return "nil";
    case lua.LUA_TBOOLEAN:
      return lua.lua_toboolean(L, idx) ? "true" : "false";
    case lua.LUA_TNUMBER: {
      const n = lua.lua_tonumber(L, idx);
      return Number.isInteger(n) ? String(n) : String(Math.round(n * 1e6) / 1e6);
    }
    case lua.LUA_TSTRING:
      return JSON.stringify(lua.lua_tojsstring(L, idx));
    case lua.LUA_TTABLE: {
      if (depth > 0) return "{...}";
      const abs = lua.lua_absindex(L, idx);
      const parts: string[] = [];
      let count = 0;
      lua.lua_pushnil(L);
      while (lua.lua_next(L, abs) !== 0) {
        if (count < 8) {
          const key = lua.lua_type(L, -2) === lua.LUA_TSTRING ? lua.lua_tojsstring(L, -2) : `[${describe(L, -2, 1)}]`;
          parts.push(`${key} = ${describe(L, -1, depth + 1)}`);
        }
        count++;
        lua.lua_pop(L, 1);
      }
      return `{ ${parts.join(", ")}${count > 8 ? `, ... (${count})` : ""} }`;
    }
    case lua.LUA_TFUNCTION:
      return "function";
    default:
      return to_jsstring(lua.lua_typename(L, t));
  }
}

/** How many Lua frames are on `L`'s stack. */
function depthOf(L: any): number {
  const ar = new lua.lua_Debug();
  let d = 0;
  while (lua.lua_getstack(L, d, ar)) d++;
  return d;
}

/** The call stack of `L`, innermost first, with each frame's locals. The
 *  outermost frame, which Lua cannot name, takes `outer`. */
function framesOf(L: any, outer: string): Frame[] {
  const frames: Frame[] = [];
  const ar = new lua.lua_Debug();
  for (let level = 0; lua.lua_getstack(L, level, ar) && level < 32; level++) {
    lua.lua_getinfo(L, "Sln", ar);
    const source = ar.source ? scriptKey(to_jsstring(ar.source)) : "?";
    const locals: Local[] = [];
    for (let i = 1; i < 200; i++) {
      const name = lua.lua_getlocal(L, ar, i);
      if (name === null || name === undefined) break;
      const n = to_jsstring(name);
      if (!n.startsWith("(")) locals.push({ name: n, value: describe(L, -1) });
      lua.lua_pop(L, 1);
    }
    frames.push({ source, line: ar.currentline, name: ar.name ? to_jsstring(ar.name) : "?", locals });
  }
  const last = frames[frames.length - 1];
  if (last && last.name === "?") last.name = outer;
  return frames;
}

export class LuaDebugger {
  /** Breakpoint lines by script key. */
  private breakpoints = new Map<string, Set<number>>();
  private step: { mode: StepMode; depth: number; thread: any } | null = null;
  private suspended: Suspended | null = null;
  private listeners = new Set<() => void>();
  /** The callback the runtime is calling (`update`, `on_hit`), set by it. */
  calling = "?";
  /** Where it is stopped, or null while the game runs. */
  pause: Pause | null = null;

  /** Whether scripts must run hooked, as coroutines. */
  get armed(): boolean {
    return this.breakpoints.size > 0 || this.step !== null;
  }

  get paused(): boolean {
    return this.pause !== null;
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  private changed() {
    for (const fn of this.listeners) fn();
  }

  // ── Breakpoints ──

  breakpointsOf(path: string): number[] {
    return [...(this.breakpoints.get(scriptKey(path)) ?? [])].sort((a, b) => a - b);
  }

  allBreakpoints(): { source: string; line: number }[] {
    const out: { source: string; line: number }[] = [];
    for (const [source, lines] of this.breakpoints) for (const line of lines) out.push({ source, line });
    return out.sort((a, b) => a.source.localeCompare(b.source) || a.line - b.line);
  }

  setBreakpoints(path: string, lines: number[]): void {
    const key = scriptKey(path);
    if (lines.length === 0) this.breakpoints.delete(key);
    else this.breakpoints.set(key, new Set(lines));
    this.rehook();
    this.changed();
  }

  toggleBreakpoint(path: string, line: number): void {
    const cur = new Set(this.breakpointsOf(path));
    if (cur.has(line)) cur.delete(line);
    else cur.add(line);
    this.setBreakpoints(path, [...cur]);
  }

  clearBreakpoints(): void {
    this.breakpoints.clear();
    this.rehook();
    this.changed();
  }

  /** Hook (or unhook) every script state as the debugger arms or disarms. */
  private rehook() {
    const states: { L: any }[] = (globalThis as any).__jengine_lua_states ?? [];
    for (const s of states) this.applyHook(s.L);
  }

  /** Set the line hook on a state if armed, clear it if not. New coroutines
   *  take the hook of the state they are made from. */
  applyHook(L: any): void {
    if (this.armed) lua.lua_sethook(L, this.hook, lua.LUA_MASKLINE, 0);
    else lua.lua_sethook(L, null, 0, 0);
  }

  // ── The hook ──

  private hook = (L: any, ar: any) => {
    if (this.pause) return;
    lua.lua_getinfo(L, "Sl", ar);
    const source = ar.source ? scriptKey(to_jsstring(ar.source)) : "";
    const line = ar.currentline;
    let reason: Pause["reason"] | null = null;
    if (this.breakpoints.get(source)?.has(line)) reason = "breakpoint";
    else if (this.step) {
      const s = this.step;
      if (s.mode === "into") reason = "step";
      else if (L === s.thread) {
        const d = depthOf(L);
        if ((s.mode === "over" && d <= s.depth) || (s.mode === "out" && d < s.depth)) reason = "step";
      }
    }
    if (!reason) return;
    if (!lua.lua_isyieldable(L)) {
      // A script's top level, run as it loads: there is no coroutine to stop.
      return;
    }
    this.step = null;
    this.pause = { source, line, reason, frames: framesOf(L, this.calling) };
    this.stepDepth = depthOf(L);
    this.stepThread = L;
    lua.lua_yield(L, 0);
  };

  private stepDepth = 0;
  private stepThread: any = null;

  // ── Being paused, and going on ──

  /** The runtime, after a callback's coroutine yielded at a pause. */
  suspendedAt(s: Suspended): void {
    this.suspended = s;
    this.changed();
  }

  /** Run on: the game, until the next breakpoint. */
  continue(): void {
    this.resume(null);
  }

  stepOver(): void {
    this.resume("over");
  }

  stepInto(): void {
    this.resume("into");
  }

  stepOut(): void {
    this.resume("out");
  }

  private resume(mode: StepMode | null) {
    const s = this.suspended;
    if (!s) return;
    this.step = mode ? { mode, depth: this.stepDepth, thread: this.stepThread } : null;
    this.suspended = null;
    this.pause = null;
    this.rehook();
    const status = lua.lua_resume(s.co, s.L, 0);
    if (status === lua.LUA_YIELD && this.pause) {
      // Stopped again inside the same callback.
      this.suspended = s;
    } else {
      // The callback is done; a step that has not stopped yet goes on into
      // whatever script runs next ("into"), or ends with the callback.
      if (this.step && this.step.mode !== "into") this.step = null;
      this.rehook();
      s.finish(status);
    }
    this.changed();
  }

  /** Drop a pause without running the rest of the callback - Play stopped.
   *  The session's script states are forgotten with it, so they are not
   *  kept alive (and re-hooked) after it. */
  reset(): void {
    if (this.suspended) this.suspended.finish(lua.LUA_OK);
    this.suspended = null;
    this.pause = null;
    this.step = null;
    const states: unknown[] | undefined = (globalThis as any).__jengine_lua_states;
    if (states) states.length = 0;
    this.changed();
  }
}

/** The editor's one debugger, installed where the runtime looks for it. */
export const luaDebugger = new LuaDebugger();
(globalThis as any).__jengine_lua_debugger = luaDebugger;
