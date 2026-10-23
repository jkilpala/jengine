<script lang="ts">
  import { logs, clearLogs, openScriptAt, type LogEntry } from "./stores/consoleStore";

  type Level = LogEntry["level"];
  const LEVELS: Level[] = ["info", "warn", "error"];

  let logListEl: HTMLDivElement | undefined = $state();
  /** Which levels are listed: the chips in the bar switch them. */
  let shown = $state<Record<Level, boolean>>({ info: true, warn: true, error: true });
  let filter = $state("");

  let counts = $derived.by(() => {
    const c: Record<Level, number> = { info: 0, warn: 0, error: 0 };
    for (const e of $logs) c[e.level]++;
    return c;
  });

  let visible = $derived.by(() => {
    const needle = filter.trim().toLowerCase();
    return $logs.filter(
      (e) =>
        shown[e.level] &&
        (!needle ||
          e.message.toLowerCase().includes(needle) ||
          (e.scriptPath ?? "").toLowerCase().includes(needle)),
    );
  });

  // Auto-scroll to bottom when new logs arrive
  $effect(() => {
    const count = visible.length;
    if (count > 0 && logListEl) {
      // Use tick to scroll after DOM update
      requestAnimationFrame(() => {
        if (logListEl) logListEl.scrollTop = logListEl.scrollHeight;
      });
    }
  });

  /** HH:MM:SS whatever the locale (Finnish writes 11.23.47). */
  function formatTime(ts: number): string {
    const d = new Date(ts);
    const two = (n: number) => String(n).padStart(2, "0");
    return `${two(d.getHours())}:${two(d.getMinutes())}:${two(d.getSeconds())}`;
  }

  function sourceLabel(entry: LogEntry): string {
    const file = entry.scriptPath?.split(/[\\/]/).pop() ?? "";
    return entry.scriptLine ? `${file}:${entry.scriptLine}` : file;
  }

  function openSource(entry: LogEntry) {
    if (entry.scriptPath) openScriptAt(entry.scriptPath, entry.scriptLine);
  }
</script>

<div class="console">
  <div class="console-bar">
    <label class="filter">
      <span class="sr-only">Filter messages</span>
      <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden="true">
        <circle cx="5" cy="5" r="3.6" stroke="currentColor" stroke-width="1.3" fill="none" />
        <path d="M7.8 7.8 L11 11" stroke="currentColor" stroke-width="1.3" />
      </svg>
      <input type="text" placeholder="Filter messages" bind:value={filter} />
    </label>
    <div class="levels" role="group" aria-label="Levels shown">
      {#each LEVELS as level (level)}
        <button
          class="chip {level}"
          class:off={!shown[level]}
          aria-pressed={shown[level]}
          title={shown[level] ? `Hide ${level} messages` : `Show ${level} messages`}
          onclick={() => (shown[level] = !shown[level])}>{level} {counts[level]}</button
        >
      {/each}
    </div>
    <button class="clear" onclick={clearLogs} title="Clear console">Clear</button>
  </div>
  <div class="log-list" bind:this={logListEl}>
    {#each visible as entry}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="log-entry {entry.level}"
        ondblclick={() => openSource(entry)}
      >
        <span class="time">{formatTime(entry.timestamp)}</span>
        <span class="level">{entry.level}</span>
        <span class="message">{entry.message}</span>
        {#if entry.scriptPath}
          <button class="source" title={`Open ${entry.scriptPath}`} onclick={() => openSource(entry)}
            >{sourceLabel(entry)}</button
          >
        {:else}
          <span class="origin">{entry.source ?? "editor"}</span>
        {/if}
      </div>
    {/each}
    {#if $logs.length === 0}
      <div class="empty">No messages</div>
    {:else if visible.length === 0}
      <div class="empty">No messages match</div>
    {/if}
  </div>
</div>

<style>
  .console {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--ed-base);
  }

  /* Filter, level chips and Clear: one slim strip under the dock's tabs. */
  .console-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 8px 0 10px;
    border-bottom: 1px solid var(--ed-line);
    flex-shrink: 0;
  }

  .filter {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 0;
    max-width: 280px;
    height: 22px;
    padding: 0 8px;
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    background: var(--ed-raised);
    color: var(--ed-faint);
  }

  .filter:focus-within {
    border-color: var(--ed-accent);
    box-shadow: 0 0 0 2px var(--ed-accent-low);
  }

  .filter input {
    flex: 1;
    min-width: 0;
    border: none;
    background: none;
    color: var(--ed-text);
    font: inherit;
    font-size: 11.5px;
    outline: none;
  }

  .filter input::placeholder {
    color: var(--ed-faint);
  }

  .levels {
    display: flex;
    gap: 4px;
    margin-left: auto;
  }

  .chip {
    height: 20px;
    padding: 0 7px;
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    background: none;
    font-family: var(--ed-mono);
    font-size: 10.5px;
    cursor: pointer;
  }

  .chip.info {
    color: var(--ed-muted);
  }

  .chip.warn {
    color: var(--ed-warn);
  }

  .chip.error {
    color: var(--ed-err);
  }

  .chip:hover {
    background: var(--ed-hover);
  }

  .chip.off {
    color: var(--ed-faint);
    border-style: dashed;
    text-decoration: line-through;
    opacity: 0.7;
  }

  .clear {
    height: 20px;
    padding: 0 8px;
    border: none;
    border-radius: 4px;
    background: none;
    color: var(--ed-faint);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }

  .clear:hover {
    background: var(--ed-hover);
    color: var(--ed-ink);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }

  .log-list {
    flex: 1;
    overflow-y: auto;
    padding: 4px 0;
    font-family: var(--ed-mono);
    font-size: 11.5px;
    user-select: text;
    -webkit-user-select: text;
  }

  .log-entry {
    display: flex;
    align-items: baseline;
    gap: 12px;
    padding: 2px 12px;
    line-height: 1.55;
    user-select: text;
    -webkit-user-select: text;
  }

  .log-entry:hover {
    background: var(--ed-hover);
  }

  .log-entry.error {
    background: color-mix(in srgb, var(--ed-err) 9%, transparent);
  }

  .log-entry.error:hover {
    background: color-mix(in srgb, var(--ed-err) 15%, transparent);
  }

  .time {
    color: var(--ed-faint);
    flex-shrink: 0;
  }

  .level {
    flex: 0 0 40px;
    color: var(--ed-faint);
  }

  .warn .level {
    color: var(--ed-warn);
  }

  .error .level,
  .error .message {
    color: var(--ed-err);
  }

  .message {
    flex: 1;
    min-width: 0;
    color: var(--ed-text);
    word-break: break-word;
    user-select: text;
    -webkit-user-select: text;
    cursor: text;
  }

  .source {
    flex-shrink: 0;
    padding: 0;
    border: none;
    background: none;
    color: var(--ed-a2);
    font: inherit;
    text-decoration: underline;
    text-decoration-color: var(--ed-line-strong);
    text-underline-offset: 2px;
    cursor: pointer;
  }

  .source:hover {
    text-decoration-color: var(--ed-a2);
  }

  /* Who said it, when it was no script: the viewport, the editor. */
  .origin {
    flex-shrink: 0;
    color: var(--ed-faint);
  }

  .empty {
    padding: 8px 12px;
    color: var(--ed-faint);
    font-family: var(--ed-sans);
    font-size: 12px;
    font-style: italic;
  }
</style>
