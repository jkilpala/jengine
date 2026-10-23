<script lang="ts">
  import { actions, type EditorAction } from "./stores/actionStore";
  import { fuzzyMatch } from "./api/fuzzy";

  interface Props {
    onclose: () => void;
  }
  let { onclose }: Props = $props();

  let query = $state("");
  let cursor = $state(0);
  let inputEl: HTMLInputElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();

  interface Row {
    action: EditorAction;
    score: number;
    enabled: boolean;
  }

  const rows = $derived.by((): Row[] => {
    const q = query.trim();
    const all = $actions;
    const scored: Row[] = [];
    for (const action of all) {
      const enabled = action.enabled?.() !== false;
      if (!q) {
        scored.push({ action, score: 0, enabled });
        continue;
      }
      // Match against "Category Title" so either half finds the command —
      // typing "play" should reach Play Mode entries whatever they are called.
      const m = fuzzyMatch(q, `${action.category} ${action.title}`);
      if (m) scored.push({ action, score: m.score, enabled });
    }
    // Runnable commands first: a disabled one is never what was wanted, but
    // hiding it entirely makes the palette look like the command is missing.
    scored.sort(
      (a, b) =>
        Number(b.enabled) - Number(a.enabled) ||
        b.score - a.score ||
        a.action.title.localeCompare(b.action.title),
    );
    return scored;
  });

  $effect(() => {
    void rows;
    cursor = 0;
  });

  $effect(() => {
    inputEl?.focus();
  });

  $effect(() => {
    void cursor;
    listEl
      ?.querySelector<HTMLElement>(".row.active")
      ?.scrollIntoView({ block: "nearest" });
  });

  function choose(row: Row) {
    if (!row.enabled) return;
    // Close first so an action that opens a dialog is not stacked behind this.
    onclose();
    void row.action.run();
  }

  function onKeydown(e: KeyboardEvent) {
    const handled =
      ["Escape", "ArrowDown", "ArrowUp", "Enter"].includes(e.key) ||
      (e.ctrlKey && (e.key === "n" || e.key === "p"));
    if (handled) e.stopPropagation();

    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "ArrowDown" || (e.ctrlKey && e.key === "n")) {
      e.preventDefault();
      if (rows.length) cursor = (cursor + 1) % rows.length;
    } else if (e.key === "ArrowUp" || (e.ctrlKey && e.key === "p")) {
      e.preventDefault();
      if (rows.length) cursor = (cursor - 1 + rows.length) % rows.length;
    } else if (e.key === "Enter") {
      e.preventDefault();
      const pick = rows[cursor];
      if (pick) choose(pick);
    }
  }

  function onOverlayClick(e: MouseEvent) {
    if (e.target === e.currentTarget) onclose();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="overlay" onclick={onOverlayClick}>
  <div class="palette">
    <input
      type="text"
      bind:this={inputEl}
      bind:value={query}
      onkeydown={onKeydown}
      placeholder="Run a command…"
      spellcheck="false"
    />
    <div class="list" bind:this={listEl}>
      {#if rows.length === 0}
        <div class="empty">No matching commands</div>
      {:else}
        {#each rows as row, i (row.action.id)}
          <button
            class="row"
            class:active={i === cursor}
            class:disabled={!row.enabled}
            onclick={() => choose(row)}
            onmousemove={() => (cursor = i)}
          >
            <span class="cat">{row.action.category}</span>
            <span class="title">{row.action.title}</span>
            {#if row.action.keys}
              <span class="keys">{row.action.keys}</span>
            {/if}
          </button>
        {/each}
      {/if}
    </div>
    <div class="footer">
      <span>↑↓ navigate · ↵ run · Esc close</span>
      <span>{rows.length} of {$actions.length}</span>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 90px;
    z-index: 3000;
  }

  .palette {
    width: min(620px, 88vw);
    max-height: 62vh;
    display: flex;
    flex-direction: column;
    background: var(--ed-base);
    border: 1px solid var(--ed-line-strong);
    border-radius: 6px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.55);
    overflow: hidden;
  }

  .palette input {
    border: none;
    border-bottom: 1px solid var(--ed-line);
    background: var(--ed-mantle);
    color: var(--ed-text);
    font-size: 14px;
    padding: 10px 12px;
    outline: none;
  }

  .list {
    overflow-y: auto;
    padding: 4px 0;
  }

  .row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    width: 100%;
    border: none;
    background: none;
    text-align: left;
    padding: 5px 12px;
    cursor: pointer;
    font-size: 12px;
    color: var(--ed-text);
  }

  .row.active {
    background: var(--ed-raised);
  }

  .row.disabled {
    opacity: 0.4;
    cursor: default;
  }

  .cat {
    color: var(--ed-faint);
    font-size: 11px;
    min-width: 62px;
  }

  .title {
    flex: 1;
  }

  .keys {
    color: var(--ed-faint);
    font-family: "Cascadia Code", monospace;
    font-size: 11px;
  }

  .empty {
    padding: 12px;
    color: var(--ed-faint);
    font-size: 12px;
    font-style: italic;
  }

  .footer {
    display: flex;
    justify-content: space-between;
    border-top: 1px solid var(--ed-line);
    background: var(--ed-mantle);
    padding: 4px 12px;
    font-size: 11px;
    color: var(--ed-faint);
  }
</style>
