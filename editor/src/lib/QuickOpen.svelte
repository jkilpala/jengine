<script lang="ts">
  import { get } from "svelte/store";
  import { project, assetRefreshRequest } from "./stores/projectStore";
  import { listAssets } from "./api/project";
  import { openAsset, canOpenAsset } from "./api/openAsset";
  import { logError } from "./stores/consoleStore";
  import { fuzzyMatch, type FuzzyResult } from "./api/fuzzy";

  interface Props {
    onclose: () => void;
  }
  let { onclose }: Props = $props();

  /** Project-relative asset paths, e.g. `scripts/lua/player.lua`. */
  let files = $state<string[]>([]);
  let query = $state("");
  let loading = $state(true);
  /** Index into `results` of the highlighted row. */
  let cursor = $state(0);
  let inputEl: HTMLInputElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();

  /**
   * Cap on rendered rows. A project with thousands of assets would otherwise
   * build thousands of DOM nodes on the first keystroke, which is exactly when
   * the palette needs to feel instant.
   */
  const MAX_ROWS = 200;

  $effect(() => {
    // `assetRefreshRequest` so a file created since the palette last opened is
    // findable without restarting the editor.
    void $assetRefreshRequest;
    const proj = get(project);
    if (!proj) return;
    listAssets(proj.path)
      .then((f) => {
        files = f;
        loading = false;
      })
      .catch((e) => {
        logError(`Quick open: ${e}`);
        loading = false;
      });
  });

  const results = $derived.by((): FuzzyResult[] => {
    const q = query.trim();
    if (!q) {
      // No query: the plain list, so Ctrl+P is also a way to browse.
      return files.slice(0, MAX_ROWS).map((path) => ({ path, score: 0, hit: [] }));
    }
    const scored: FuzzyResult[] = [];
    for (const path of files) {
      const m = fuzzyMatch(q, path);
      if (m) scored.push(m);
    }
    // Best first; ties broken by path so the order never flickers between
    // identical queries.
    scored.sort((a, b) => b.score - a.score || a.path.localeCompare(b.path));
    return scored.slice(0, MAX_ROWS);
  });

  // Any change to the result set invalidates the old cursor position.
  $effect(() => {
    void results;
    cursor = 0;
  });

  $effect(() => {
    inputEl?.focus();
  });

  // Keep the highlighted row in view when navigating by keyboard.
  $effect(() => {
    void cursor;
    listEl
      ?.querySelector<HTMLElement>(".row.active")
      ?.scrollIntoView({ block: "nearest" });
  });

  /** Split a path into `dir/` + `name` so the name can be emphasised. */
  function splitPath(path: string): { dir: string; name: string } {
    const i = path.lastIndexOf("/");
    return i < 0
      ? { dir: "", name: path }
      : { dir: path.slice(0, i + 1), name: path.slice(i + 1) };
  }

  /** Character indices of `hit`, for highlighting. */
  function isHit(hits: number[], i: number): boolean {
    return hits.includes(i);
  }

  async function choose(path: string) {
    const proj = get(project);
    if (!proj) return;
    if (!canOpenAsset(path)) {
      logError(`No editor for ${path}`);
      return;
    }
    // Close first: opening a scene may raise a confirm dialog, and leaving the
    // palette on screen behind it looks like the click did nothing.
    onclose();
    await openAsset(proj, `assets/${path}`);
  }

  function onKeydown(e: KeyboardEvent) {
    // Every chord below is also a global editor shortcut. `preventDefault`
    // alone does not stop the event bubbling to the window handler, so
    // Ctrl+P inside the palette would move the cursor *and* toggle the
    // palette shut on the same keypress.
    const handled = ["Escape", "ArrowDown", "ArrowUp", "Enter"].includes(e.key)
      || (e.ctrlKey && (e.key === "n" || e.key === "p"));
    if (handled) e.stopPropagation();

    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "ArrowDown" || (e.ctrlKey && e.key === "n")) {
      e.preventDefault();
      if (results.length) cursor = (cursor + 1) % results.length;
    } else if (e.key === "ArrowUp" || (e.ctrlKey && e.key === "p")) {
      e.preventDefault();
      if (results.length)
        cursor = (cursor - 1 + results.length) % results.length;
    } else if (e.key === "Enter") {
      e.preventDefault();
      const pick = results[cursor];
      if (pick) void choose(pick.path);
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
      placeholder="Go to file…"
      spellcheck="false"
    />
    <div class="list" bind:this={listEl}>
      {#if loading}
        <div class="empty">Loading…</div>
      {:else if results.length === 0}
        <div class="empty">No matching files</div>
      {:else}
        {#each results as r, i (r.path)}
          {@const parts = splitPath(r.path)}
          {@const openable = canOpenAsset(r.path)}
          <button
            class="row"
            class:active={i === cursor}
            class:unopenable={!openable}
            onclick={() => choose(r.path)}
            onmousemove={() => (cursor = i)}
            title={openable ? r.path : `${r.path} — no editor for this type`}
          >
            <span class="name">
              {#each parts.name.split("") as ch, ci}
                <span
                  class:hit={isHit(r.hit, parts.dir.length + ci)}>{ch}</span
                >
              {/each}
            </span>
            <span class="dir">
              {#each parts.dir.split("") as ch, ci}
                <span class:hit={isHit(r.hit, ci)}>{ch}</span>
              {/each}
            </span>
          </button>
        {/each}
      {/if}
    </div>
    <div class="footer">
      <span>↑↓ navigate · ↵ open · Esc close</span>
      <span
        >{results.length}{results.length === MAX_ROWS ? "+" : ""} of {files.length}</span
      >
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
    width: min(680px, 88vw);
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
    padding: 4px 12px;
    cursor: pointer;
    font-family: "Cascadia Code", "Fira Code", monospace;
    font-size: 12px;
    color: var(--ed-text);
  }

  .row.active {
    background: var(--ed-raised);
  }

  .row.unopenable {
    opacity: 0.45;
  }

  .name {
    color: var(--ed-text);
    white-space: nowrap;
  }

  .dir {
    color: var(--ed-faint);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hit {
    color: var(--ed-warn);
    font-weight: 600;
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
