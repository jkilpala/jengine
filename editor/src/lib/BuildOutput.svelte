<script lang="ts">
  import {
    buildLines,
    buildRunning,
    clearBuildOutput,
    type BuildLine,
  } from "./stores/buildStore";

  let listEl: HTMLDivElement | undefined = $state();
  /**
   * Follow the tail, but stop the moment the user scrolls up — a build
   * yanking you back to the bottom while you are reading an error is the
   * one thing that makes a build log unusable.
   */
  let pinned = $state(true);

  function onScroll() {
    if (!listEl) return;
    const slack = listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight;
    pinned = slack < 24;
  }

  $effect(() => {
    // Touch the length so this re-runs per line.
    void $buildLines.length;
    if (!pinned || !listEl) return;
    requestAnimationFrame(() => {
      if (listEl && pinned) listEl.scrollTop = listEl.scrollHeight;
    });
  });

  function lineColor(kind: BuildLine["kind"]): string {
    switch (kind) {
      case "command":
        return "var(--ed-a2)";
      case "error":
        return "var(--ed-err)";
      case "warning":
        return "var(--ed-warn)";
      case "success":
        return "var(--ed-ok)";
      default:
        return "var(--ed-text)";
    }
  }

  function copyAll() {
    const text = $buildLines.map((l) => l.text).join("\n");
    navigator.clipboard?.writeText(text);
  }
</script>

<div class="build">
  <div class="header">
    <span>
      Build
      {#if $buildRunning}
        <span class="running">● running</span>
      {/if}
    </span>
    <span class="actions">
      {#if !pinned && $buildRunning}
        <button
          onclick={() => {
            pinned = true;
            if (listEl) listEl.scrollTop = listEl.scrollHeight;
          }}
          title="Resume following the output">Follow</button
        >
      {/if}
      <button onclick={copyAll} title="Copy the whole transcript">Copy</button>
      <button onclick={clearBuildOutput} title="Clear build output">Clear</button
      >
    </span>
  </div>
  <div class="line-list" bind:this={listEl} onscroll={onScroll}>
    {#each $buildLines as line}
      <div class="line" style="color: {lineColor(line.kind)}">{line.text}</div>
    {/each}
    {#if $buildLines.length === 0}
      <div class="empty">
        No build output yet — start a build from Project → Build Settings.
      </div>
    {/if}
  </div>
</div>

<style>
  .build {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--ed-base);
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 10px;
    font-family: var(--ed-mono);
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ed-muted);
    background: var(--ed-mantle);
    border-top: 1px solid var(--ed-line);
  }

  .running {
    color: var(--ed-ok);
    font-weight: 500;
    margin-left: 6px;
    text-transform: none;
  }

  .actions {
    display: flex;
    gap: 2px;
  }

  .header button {
    background: none;
    border: none;
    color: var(--ed-faint);
    cursor: pointer;
    font-size: 11.5px;
    padding: 3px 6px;
  }

  .header button:hover {
    color: var(--ed-text);
  }

  .line-list {
    flex: 1;
    overflow-y: auto;
    padding: 4px 0;
    font-family: "Cascadia Code", "Fira Code", monospace;
    font-size: 12px;
    user-select: text;
    -webkit-user-select: text;
  }

  .line {
    padding: 0 10px;
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
    -webkit-user-select: text;
    cursor: text;
  }

  .line:hover {
    background: var(--ed-hover);
  }

  .empty {
    padding: 8px 10px;
    color: var(--ed-faint);
    font-size: 12px;
    font-style: italic;
  }
</style>

