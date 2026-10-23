<script lang="ts">
  // The Lua debugger's panel: where Play stopped, the call stack, the
  // selected frame's locals, and every breakpoint. Breakpoints are set in the
  // script editor's margin (or F9 on a line); while stopped, F8 continues,
  // F10 steps over, F11 steps into and Shift+F11 out. The debugger stops
  // scripts in the editor's Play mode (Fengari); see `luaDebugger.ts`.
  import { get } from "svelte/store";
  import { luaDebugger } from "./luaDebugger";
  import { debuggerState } from "./stores/debuggerStore";
  import { project } from "./stores/projectStore";
  import { openAsset } from "./api/openAsset";

  let frame = $state(0);
  let pause = $derived($debuggerState.pause);
  let frames = $derived(pause?.frames ?? []);
  let locals = $derived(frames[Math.min(frame, frames.length - 1)]?.locals ?? []);

  // A new stop shows its own top frame.
  $effect(() => {
    void pause;
    frame = 0;
  });

  function open(source: string, line: number) {
    const proj = get(project);
    if (proj) void openAsset(proj, `assets/${source}`, { line });
  }

  // Show where it stopped.
  $effect(() => {
    if (pause) open(pause.source, pause.line);
  });
</script>

<div class="debugger">
  <div class="bar">
    {#if pause}
      <span class="where">⏸ {pause.source}:{pause.line} <span class="dim">({pause.reason})</span></span>
    {:else}
      <span class="dim">Running - Play stops at a breakpoint set in a script's margin (or F9).</span>
    {/if}
    <span class="spacer"></span>
    <button disabled={!pause} title="Continue (F8)" onclick={() => luaDebugger.continue()}>▶ Continue</button>
    <button disabled={!pause} title="Step over (F10)" onclick={() => luaDebugger.stepOver()}>⤼ Over</button>
    <button disabled={!pause} title="Step into (F11)" onclick={() => luaDebugger.stepInto()}>↓ Into</button>
    <button disabled={!pause} title="Step out (Shift+F11)" onclick={() => luaDebugger.stepOut()}>↑ Out</button>
  </div>
  <div class="cols">
    <div class="col">
      <div class="title">Call stack</div>
      {#each frames as f, i}
        <button class="row" class:sel={i === frame} onclick={() => ((frame = i), open(f.source, f.line))}>
          <span class="fn">{f.name}</span>
          <span class="dim">{f.source}:{f.line}</span>
        </button>
      {/each}
      {#if frames.length === 0}<div class="dim">-</div>{/if}
    </div>
    <div class="col wide">
      <div class="title">Locals</div>
      {#each locals as l}
        <div class="local"><span class="name">{l.name}</span><span class="value">{l.value}</span></div>
      {/each}
      {#if pause && locals.length === 0}<div class="dim">none</div>{/if}
    </div>
    <div class="col">
      <div class="title">
        Breakpoints
        {#if $debuggerState.breakpoints.length > 0}
          <button class="link" onclick={() => luaDebugger.clearBreakpoints()}>clear all</button>
        {/if}
      </div>
      {#each $debuggerState.breakpoints as b}
        <div class="bp">
          <button class="link" onclick={() => open(b.source, b.line)}>{b.source}:{b.line}</button>
          <button class="x" title="Remove" onclick={() => luaDebugger.toggleBreakpoint(b.source, b.line)}>×</button>
        </div>
      {/each}
      {#if $debuggerState.breakpoints.length === 0}<div class="dim">none</div>{/if}
    </div>
  </div>
</div>

<style>
  .debugger {
    height: 100%;
    display: flex;
    flex-direction: column;
    font-size: 11px;
    color: var(--ed-text);
    background: var(--ed-base);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    border-bottom: 1px solid var(--ed-line);
  }
  .where {
    color: var(--ed-warn);
    font-family: var(--ed-mono);
  }
  .spacer {
    flex: 1;
  }
  .cols {
    flex: 1;
    display: flex;
    gap: 12px;
    padding: 6px 8px;
    overflow: auto;
  }
  .col {
    width: 220px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .col.wide {
    flex: 1;
    width: auto;
    min-width: 200px;
  }
  .title {
    font-weight: 600;
    display: flex;
    gap: 8px;
  }
  .row {
    display: flex;
    justify-content: space-between;
    gap: 6px;
    background: none;
    border: none;
    color: var(--ed-text);
    text-align: left;
    padding: 1px 3px;
    cursor: pointer;
    font-size: 11px;
  }
  .row.sel {
    background: var(--ed-line);
  }
  .fn {
    font-family: var(--ed-mono);
  }
  .local {
    display: flex;
    gap: 8px;
    font-family: var(--ed-mono);
  }
  .local .name {
    color: var(--ed-accent);
    min-width: 80px;
  }
  .local .value {
    color: var(--ed-ok);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bp {
    display: flex;
    justify-content: space-between;
  }
  .dim {
    color: var(--ed-faint);
  }
  .link {
    background: none;
    border: none;
    color: var(--ed-accent);
    cursor: pointer;
    font-size: 11px;
    padding: 0;
  }
  .x {
    background: none;
    border: none;
    color: var(--ed-muted);
    cursor: pointer;
  }
  .bar button {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 3px;
    font-size: 11.5px;
    padding: 3px 6px;
    cursor: pointer;
  }
  .bar button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
