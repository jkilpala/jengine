<script lang="ts">
  // The Profiler panel: record a capture of the running viewport - or open
  // one the native runtime wrote (`JENGINE_PROFILE_CAPTURE`) - and read
  // where the time went. A bar per frame, stacked by what its spans were
  // (render, scripts, physics, motion, ...); click a bar for that frame,
  // Shift-click for a range, *All* for the whole capture. Below: the spans
  // as a tree (time per frame, its own time, the worst frame's, draws), the
  // scripts by cost, and the render passes with their draws. Times are CPU
  // time - for a render pass, building and encoding it, not the GPU's.
  // The logic is `profileCapture.ts`.
  import { save, open } from "@tauri-apps/plugin-dialog";
  import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
  import {
    profilerCapture,
    profilerRecording,
    startRecording,
    stopRecording,
    showCapture,
    clearCapture,
  } from "./stores/profilerStore";
  import { logError, logInfo } from "./stores/consoleStore";
  import { themeColors, withAlpha } from "./stores/themeColors";
  import {
    flatten,
    formatCapture,
    frameStack,
    parseCapture,
    passTable,
    scriptTable,
    spanTree,
    tracedMs,
    worstFrame,
  } from "./profileCapture";

  let capture = $derived($profilerCapture);
  let frames = $derived(capture.frames);
  /** The frames read below: a range of indices into `frames`, or all. */
  let sel = $state<{ from: number; to: number } | null>(null);
  let chosen = $derived(
    sel && frames.length > 0
      ? frames.slice(Math.min(sel.from, frames.length - 1), Math.min(sel.to, frames.length - 1) + 1)
      : frames,
  );
  let tree = $derived(spanTree(chosen));
  let closed = $state<Set<string>>(new Set());
  let rows = $derived(flatten(tree, (k) => !closed.has(k)));
  let scripts = $derived(scriptTable(chosen));
  let passes = $derived(passTable(tree));
  let avgTraced = $derived(chosen.length ? chosen.reduce((s, f) => s + tracedMs(f), 0) / chosen.length : 0);
  let avgWall = $derived(chosen.length ? chosen.reduce((s, f) => s + f.ms, 0) / chosen.length : 0);

  // Recording drops the selection, which would point at old frames.
  $effect(() => {
    if ($profilerRecording) sel = null;
  });

  async function toggle() {
    try {
      if ($profilerRecording) stopRecording();
      else await startRecording();
    } catch (e) {
      logError(`${e}`);
    }
  }

  async function saveCapture() {
    const path = await save({ filters: [{ name: "Profiler capture", extensions: ["jprof"] }], defaultPath: "capture.jprof" });
    if (!path) return;
    try {
      await writeTextFile(path, formatCapture(capture));
      logInfo(`Saved a capture of ${frames.length} frames to ${path}`);
    } catch (e) {
      logError(`Could not save the capture: ${e}`);
    }
  }

  async function openCapture() {
    const path = await open({ filters: [{ name: "Profiler capture", extensions: ["jprof", "json"] }] });
    if (!path || Array.isArray(path)) return;
    try {
      showCapture(parseCapture(await readTextFile(path)));
      sel = null;
    } catch (e) {
      logError(`Could not open ${path}: ${e}`);
    }
  }

  // ── The frame strip ──

  // Resolved values rather than var(): the strip is a canvas.
  const COLORS: Record<string, string> = $derived({
    render: $themeColors["cat-5"],
    scripts: $themeColors["cat-4"],
    physics: $themeColors["cat-7"],
    motion: $themeColors["cat-6"],
    animation: $themeColors["cat-3"],
    behavior_graphs: $themeColors["cat-8"],
    ui: $themeColors["cat-2"],
    navigation: $themeColors["cat-1"],
    particles: $themeColors.sun2,
  });
  const colorOf = (c: string) => COLORS[c] ?? $themeColors.faint;

  let canvas = $state<HTMLCanvasElement | null>(null);
  let cw = $state(600);
  const CH = 90;
  $effect(() => {
    const el = canvas;
    if (!el) return;
    const ro = new ResizeObserver(() => (cw = el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  });

  // The frames shown: the last that fit at 2 px a bar or more.
  let shownFrom = $derived(Math.max(0, frames.length - Math.floor(cw / 2)));
  let barW = $derived(frames.length - shownFrom > 0 ? cw / (frames.length - shownFrom) : cw);
  let top = $derived(Math.max(16.7, ...frames.slice(shownFrom).map(tracedMs)) * 1.1);

  $effect(() => {
    const el = canvas;
    if (!el) return;
    const dpr = window.devicePixelRatio || 1;
    el.width = Math.round(cw * dpr);
    el.height = Math.round(CH * dpr);
    const g = el.getContext("2d");
    if (!g) return;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.fillStyle = $themeColors.mantle;
    g.fillRect(0, 0, cw, CH);
    const y = (ms: number) => CH - (ms / top) * CH;
    for (let i = shownFrom; i < frames.length; i++) {
      const x = (i - shownFrom) * barW;
      let acc = 0;
      for (const part of frameStack(frames[i])) {
        g.fillStyle = colorOf(part.category);
        g.fillRect(x, y(acc + part.ms), Math.max(1, barW - (barW > 3 ? 1 : 0)), y(acc) - y(acc + part.ms));
        acc += part.ms;
      }
      if (sel && i >= sel.from && i <= sel.to) {
        g.fillStyle = withAlpha($themeColors.warn, 0.25);
        g.fillRect(x, 0, barW, CH);
      }
    }
    // A 60 Hz frame and a 30 Hz one.
    g.setLineDash([3, 3]);
    for (const [ms, c] of [
      [16.7, $themeColors.ok],
      [33.3, $themeColors.err],
    ] as const) {
      if (ms > top) continue;
      g.strokeStyle = c;
      g.beginPath();
      g.moveTo(0, y(ms));
      g.lineTo(cw, y(ms));
      g.stroke();
    }
    g.setLineDash([]);
  });

  function pick(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const i = shownFrom + Math.floor((e.clientX - r.left) / barW);
    if (i < 0 || i >= frames.length) return;
    sel = e.shiftKey && sel ? { from: Math.min(sel.from, i), to: Math.max(sel.to, i) } : { from: i, to: i };
  }

  function worst() {
    const i = worstFrame(frames);
    if (i >= 0) sel = { from: i, to: i };
  }

  const ms = (v: number) => (v >= 10 ? v.toFixed(1) : v.toFixed(2));
  const pct = (v: number) => (avgTraced > 0 ? `${Math.round((v / avgTraced) * 100)}%` : "");
  const toggleNode = (key: string) => {
    const next = new Set(closed);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    closed = next;
  };
</script>

<div class="profiler">
  <div class="bar">
    <button class:rec={$profilerRecording} onclick={toggle} title="Record the running viewport's frames">
      {$profilerRecording ? "■ Stop" : "● Record"}
    </button>
    <button onclick={() => (clearCapture(), (sel = null))} disabled={frames.length === 0}>Clear</button>
    <button onclick={saveCapture} disabled={frames.length === 0}>Save…</button>
    <button onclick={openCapture} title="Open a capture - this editor's, or one the native runtime wrote with JENGINE_PROFILE_CAPTURE">Open…</button>
    <span class="sep"></span>
    <button onclick={() => (sel = null)} disabled={!sel}>All</button>
    <button onclick={worst} disabled={frames.length === 0}>Worst frame</button>
    <span class="info">
      {#if frames.length === 0}
        {$profilerRecording ? "Recording - waiting for frames…" : "Record while the scene plays, or open a capture."}
      {:else}
        {capture.runtime} · {frames.length} frames ·
        {sel ? (sel.from === sel.to ? `frame ${frames[sel.from]?.index}` : `${sel.to - sel.from + 1} frames`) : "all"}:
        <b>{ms(avgTraced)} ms</b> traced CPU a frame, {ms(avgWall)} ms between frames
      {/if}
    </span>
  </div>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <canvas bind:this={canvas} class="strip" onclick={pick}></canvas>
  <div class="legend">
    {#each Object.entries(COLORS) as [c, col]}<span><i style="background:{col}"></i>{c}</span>{/each}
    <span><i style="background:var(--ed-line-strong)"></i>other</span>
  </div>
  <div class="cols">
    <div class="col wide">
      <table>
        <thead><tr><th class="name">Span</th><th>ms</th><th title="Its own time: less its children's">self</th><th title="The worst frame's">max</th><th>%</th><th>draws</th></tr></thead>
        <tbody>
          {#each rows as n (n.key)}
            <tr>
              <td class="name" style="padding-left:{4 + n.depth * 14}px">
                {#if n.children.length > 0}
                  <button class="tw" onclick={() => toggleNode(n.key)}>{closed.has(n.key) ? "▸" : "▾"}</button>
                {:else}<span class="tw"></span>{/if}
                <i class="dot" style="background:{colorOf(n.name.split('.')[0])}"></i>{n.name}{n.calls > 1.05 ? ` ×${n.calls.toFixed(1)}` : ""}
              </td>
              <td>{ms(n.ms)}</td>
              <td class="dim">{ms(n.selfMs)}</td>
              <td class="dim">{ms(n.maxMs)}</td>
              <td class="dim">{n.depth === 0 ? pct(n.ms) : ""}</td>
              <td class="dim">{n.draws > 0 ? n.draws.toFixed(n.draws % 1 ? 1 : 0) : ""}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <div class="col">
      <table>
        <thead><tr><th class="name">Script</th><th>ms</th><th>max</th><th>calls</th></tr></thead>
        <tbody>
          {#each scripts as s (s.path)}
            <tr><td class="name" title={s.path}>{s.path.split("/").pop()}</td><td>{ms(s.ms)}</td><td class="dim">{ms(s.maxMs)}</td><td class="dim">{s.calls % 1 === 0 ? s.calls : s.calls.toFixed(s.calls < 1 ? 2 : 1)}</td></tr>
          {/each}
          {#if scripts.length === 0}<tr><td class="dim" colspan="4">none timed</td></tr>{/if}
        </tbody>
      </table>
      <table>
        <thead><tr><th class="name">Pass</th><th>ms</th><th>draws</th></tr></thead>
        <tbody>
          {#each passes as p (p.name)}
            <tr><td class="name">{p.name.replace(/render\./g, "")}</td><td>{ms(p.ms)}</td><td>{p.draws.toFixed(p.draws % 1 ? 1 : 0)}</td></tr>
          {/each}
          {#if passes.length === 0}<tr><td class="dim" colspan="3">no render spans</td></tr>{/if}
        </tbody>
      </table>
    </div>
  </div>
</div>

<style>
  .profiler {
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
    flex-wrap: wrap;
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
  .bar button.rec {
    border-color: var(--ed-err);
    color: var(--ed-err);
  }
  .sep {
    width: 1px;
    height: 14px;
    background: var(--ed-line);
  }
  .info {
    color: var(--ed-muted);
  }
  .strip {
    width: 100%;
    height: 90px;
    flex-shrink: 0;
    cursor: pointer;
    border-bottom: 1px solid var(--ed-line);
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    padding: 2px 8px;
    color: var(--ed-muted);
  }
  .legend i,
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    border-radius: 2px;
    margin-right: 4px;
  }
  .cols {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 10px;
    padding: 4px 8px;
    overflow: auto;
  }
  .col {
    width: 300px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .col.wide {
    flex: 1;
    width: auto;
    min-width: 320px;
  }
  table {
    border-collapse: collapse;
    width: 100%;
  }
  th {
    text-align: right;
    color: var(--ed-muted);
    font-weight: 600;
    padding: 1px 6px;
    border-bottom: 1px solid var(--ed-line);
  }
  td {
    text-align: right;
    padding: 1px 6px;
    font-family: var(--ed-mono);
    white-space: nowrap;
  }
  .name {
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 260px;
  }
  .dim {
    color: var(--ed-faint);
  }
  .tw {
    display: inline-block;
    width: 12px;
    background: none;
    border: none;
    color: var(--ed-muted);
    cursor: pointer;
    padding: 0;
    font-size: 10px;
  }
  tbody tr:hover {
    background: var(--ed-raised);
  }
</style>
