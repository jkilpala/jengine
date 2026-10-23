<script lang="ts">
  import { get } from "svelte/store";
  import { project } from "./stores/projectStore";
  import {
    searchInFiles,
    previewReplace,
    applyReplace,
    type SearchHit,
    type SearchResults,
    type ReplacePreview,
  } from "./api/search";
  import { logInfo, logError } from "./stores/consoleStore";
  import { requestAssetRefresh } from "./stores/projectStore";
  import { openAsset, canOpenAsset } from "./api/openAsset";
  import {
    searchQuery,
    searchFocusRequest,
    searchOpenReplace,
  } from "./stores/searchStore";
  import { reloadScriptTabsFromDisk } from "./stores/scriptEditorStore";
  import { sceneTabs } from "./stores/sceneTabsStore";

  let query = $state("");
  let caseSensitive = $state(false);
  let wholeWord = $state(false);
  let isRegex = $state(false);
  let results = $state<SearchResults | null>(null);
  let error = $state<string | null>(null);
  let running = $state(false);
  let inputEl: HTMLInputElement | undefined = $state();
  /**
   * The query the visible results were produced by.
   *
   * The list only refreshes on Enter, so after typing it describes a search
   * the box no longer says - and it looks perfectly current. Keeping the
   * query that produced it is what lets the panel admit that.
   */
  let resultsQuery = $state("");
  /** Files the user has collapsed, by path. */
  let collapsed = $state<Set<string>>(new Set());

  // ---- replace ------------------------------------------------------------
  let showReplace = $state(false);
  let replacement = $state("");
  /** Non-null once a preview has been taken; cleared by any edit. */
  let preview = $state<ReplacePreview | null>(null);
  /** Files the user unticked in the preview. */
  let excluded = $state<Set<string>>(new Set());
  let applying = $state(false);

  /**
   * Discard a stale preview.
   *
   * A preview describes one exact query, replacement and option set. Letting
   * it survive an edit to any of those is how someone ends up applying a
   * change they never looked at, and there is no undo for this.
   */
  function invalidatePreview() {
    preview = null;
    excluded = new Set();
  }

  const previewFiles = $derived(
    (preview?.files ?? []).filter((f) => !excluded.has(f)),
  );

  const previewHitCount = $derived(
    (preview?.hits ?? []).filter((h) => !excluded.has(h.path)).length,
  );

  async function runPreview() {
    const proj = get(project);
    const q = query.trim();
    if (!proj || !q) return;
    error = null;
    try {
      preview = await previewReplace(proj.path, q, replacement, {
        caseSensitive,
        wholeWord,
        isRegex,
      });
      excluded = new Set();
    } catch (e) {
      error = `${e}`;
      preview = null;
    }
  }

  async function applyPreview() {
    const proj = get(project);
    const q = query.trim();
    if (!proj || !q || !preview) return;
    const files = previewFiles;
    if (files.length === 0) return;
    // Last gate. Everything above this line is reversible; this is not.
    const ok = confirm(
      `Replace ${previewHitCount} occurrence${previewHitCount === 1 ? "" : "s"} ` +
        `across ${files.length} file${files.length === 1 ? "" : "s"}?\n\n` +
        `This cannot be undone from the editor.`,
    );
    if (!ok) return;
    applying = true;
    try {
      const summary = await applyReplace(
        proj.path,
        q,
        replacement,
        { caseSensitive, wholeWord, isRegex },
        files,
      );
      logInfo(
        `Replaced ${summary.replacements} occurrence(s) in ${summary.files_changed} file(s)`,
      );
      for (const f of summary.failures) logError(`Replace: ${f}`);
      // Files changed on disk behind every open editor and the asset list.
      // A tab still holding what it read at open time is not just showing
      // stale text - that copy is what its next save writes, which would
      // quietly undo the replace.
      requestAssetRefresh();
      await refreshOpenEditors(proj.path, files);
      invalidatePreview();
      await run();
    } catch (e) {
      logError(`Replace failed: ${e}`);
    } finally {
      applying = false;
    }
  }

  function toggleExcluded(path: string) {
    const next = new Set(excluded);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    excluded = next;
  }

  /**
   * Bumped for every search started. A slow earlier search resolving after a
   * later one would otherwise overwrite the newer results — the classic
   * search-as-you-type race.
   */
  let generation = 0;

  /** Ctrl+Shift+F focuses the box and selects what is there, like an IDE. */
  $effect(() => {
    void $searchFocusRequest;
    if ($searchFocusRequest === 0) return;
    // A selection seeded from elsewhere (a future "search selection") arrives
    // through the store; an empty one leaves the box as the user left it.
    const seeded = get(searchQuery);
    if (seeded) query = seeded;
    // Only ever opens the row. Closing it here would fight the user, who may
    // have opened it by hand before pressing Ctrl+Shift+F again.
    if (get(searchOpenReplace)) showReplace = true;
    inputEl?.focus();
    inputEl?.select();
  });

  /**
   * Bring open editors back in step with what was just written.
   *
   * Scripts reload themselves; a script with unsaved edits is left alone and
   * named, because reloading would discard them. Scenes are only reported:
   * a scene tab carries an undo history built against the old text, so
   * swapping the document under it silently would leave Ctrl+Z replaying
   * edits from a file that no longer exists that way.
   */
  async function refreshOpenEditors(projectPath: string, changed: string[]) {
    const conflicts = await reloadScriptTabsFromDisk(projectPath, changed);
    if (conflicts.length > 0) {
      logError(
        `Changed on disk but left open with your unsaved edits - reconcile these yourself: ${conflicts.join(", ")}`,
      );
    }

    // Same shape problem as above: search reports project-relative paths,
    // a scene tab holds an absolute one.
    const norm = (v: string) =>
      v.replace(/\\/g, "/").replace(/\/+/g, "/").toLowerCase();
    const root = norm(projectPath).replace(/\/$/, "");
    const changedSet = new Set(changed.map((rel) => norm(`${root}/${rel}`)));
    const staleScenes = get(sceneTabs)
      .filter((t) => t.path && changedSet.has(norm(t.path)))
      .map((t) => t.path!.split(/[\\\\/]/).pop());
    if (staleScenes.length > 0) {
      logError(
        `Scene(s) changed on disk and still open from before: ${staleScenes.join(", ")} — close and reopen them, or saving will write the old version back.`,
      );
    }
  }

  async function run() {
    invalidatePreview();
    resultsQuery = query.trim();
    const proj = get(project);
    if (!proj) return;
    const q = query.trim();
    if (!q) {
      results = null;
      error = null;
      return;
    }
    const mine = ++generation;
    running = true;
    error = null;
    try {
      const r = await searchInFiles(proj.path, q, {
        caseSensitive,
        wholeWord,
        isRegex,
      });
      if (mine !== generation) return;
      results = r;
      collapsed = new Set();
    } catch (e) {
      if (mine !== generation) return;
      // An in-progress regex is invalid far more often than it is valid, so
      // this is an ordinary state, not a failure worth a console entry.
      error = `${e}`;
      results = null;
    } finally {
      if (mine === generation) running = false;
    }
  }

  /** Hits bucketed by file, preserving the backend's stable ordering. */
  const grouped = $derived.by(() => {
    const out: { path: string; hits: SearchHit[] }[] = [];
    for (const hit of results?.hits ?? []) {
      const last = out[out.length - 1];
      if (last && last.path === hit.path) last.hits.push(hit);
      else out.push({ path: hit.path, hits: [hit] });
    }
    return out;
  });

  function toggle(path: string) {
    const next = new Set(collapsed);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    collapsed = next;
  }

  async function openHit(hit: SearchHit) {
    const proj = get(project);
    if (!proj) return;
    await openAsset(proj, hit.path, { line: hit.line });
  }

  /** Split a line into alternating plain / matched runs for highlighting. */
  function segments(hit: SearchHit): { text: string; hit: boolean }[] {
    const out: { text: string; hit: boolean }[] = [];
    let at = 0;
    for (const [s, e] of hit.matches) {
      if (s > at) out.push({ text: hit.text.slice(at, s), hit: false });
      out.push({ text: hit.text.slice(s, e), hit: true });
      at = e;
    }
    if (at < hit.text.length) out.push({ text: hit.text.slice(at), hit: false });
    return out;
  }

  function fileName(path: string): string {
    return path.split("/").pop() ?? path;
  }

  function fileDir(path: string): string {
    const i = path.lastIndexOf("/");
    return i < 0 ? "" : path.slice(0, i);
  }
</script>

<div class="find">
  <div class="controls">
    <input
      type="text"
      bind:this={inputEl}
      bind:value={query}
      placeholder="Search in files…"
      spellcheck="false"
      oninput={invalidatePreview}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          void run();
        }
      }}
    />
    <div class="toggles">
      <button
        class:on={caseSensitive}
        title="Match case"
        onclick={() => {
          caseSensitive = !caseSensitive;
          void run();
        }}>Aa</button
      >
      <button
        class:on={wholeWord}
        title="Whole word"
        onclick={() => {
          wholeWord = !wholeWord;
          void run();
        }}>ab|</button
      >
      <button
        class:on={isRegex}
        title="Regular expression"
        onclick={() => {
          isRegex = !isRegex;
          void run();
        }}>.*</button
      >
      <button
        class:on={showReplace}
        title="Toggle replace"
        onclick={() => {
          showReplace = !showReplace;
          if (!showReplace) invalidatePreview();
        }}>⇄</button
      >
      <button class="go" onclick={() => void run()} disabled={running}>
        {running ? "…" : "Search"}
      </button>
    </div>
  </div>

  {#if showReplace}
    <div class="controls replace-row">
      <input
        type="text"
        bind:value={replacement}
        oninput={invalidatePreview}
        placeholder="Replace with…"
        spellcheck="false"
      />
      <div class="toggles">
        <button onclick={() => void runPreview()} disabled={!query.trim()}>
          Preview
        </button>
        <button
          class="danger"
          onclick={() => void applyPreview()}
          disabled={!preview || previewFiles.length === 0 || applying}
        >
          {applying ? "…" : "Replace All"}
        </button>
      </div>
    </div>
  {/if}

  {#if preview}
    <div class="preview">
      <div class="preview-head">
        {previewHitCount} change{previewHitCount === 1 ? "" : "s"} in
        {previewFiles.length} file{previewFiles.length === 1 ? "" : "s"}
        <span class="dim">— untick a file to leave it alone</span>
      </div>
      {#each preview.files as file (file)}
        <label class="pfile">
          <input
            type="checkbox"
            checked={!excluded.has(file)}
            onchange={() => toggleExcluded(file)}
          />
          <span class:struck={excluded.has(file)}>{file}</span>
        </label>
        {#if !excluded.has(file)}
          {#each preview.hits.filter((h) => h.path === file) as h (h.line)}
            <div class="pdiff">
              <span class="lineno">{h.line}</span>
              <span class="before">{h.before}</span>
              <span class="after">{h.after}</span>
            </div>
          {/each}
        {/if}
      {/each}
    </div>
  {/if}

  {#if error}
    <div class="status error">{error}</div>
  {:else if results}
    <div class="status">
      {results.hits.length} result{results.hits.length === 1 ? "" : "s"} in
      {results.files_matched} file{results.files_matched === 1 ? "" : "s"}
      <span class="dim">({results.files_searched} searched)</span>
      {#if results.truncated}
        <span class="warn">— stopped at the result limit, narrow the search</span>
      {/if}
      {#if query.trim() !== resultsQuery}
        <span class="warn"
          >— for “{resultsQuery}”, press Enter to search again</span
        >
      {/if}
    </div>
  {/if}

  <div class="results" class:stale={query.trim() !== resultsQuery}>
    {#if !results && !error}
      <div class="empty">
        Type a query and press Enter. Ctrl+Shift+F focuses this box.
      </div>
    {:else if results && results.hits.length === 0}
      <div class="empty">No results</div>
    {/if}
    {#each grouped as group (group.path)}
      <button class="file-row" onclick={() => toggle(group.path)}>
        <span class="chev">{collapsed.has(group.path) ? "▶" : "▼"}</span>
        <span class="fname">{fileName(group.path)}</span>
        <span class="fdir">{fileDir(group.path)}</span>
        <span class="count">{group.hits.length}</span>
      </button>
      {#if !collapsed.has(group.path)}
        {#each group.hits as hit (hit.line + ":" + hit.text)}
          <button
            class="hit-row"
            class:unopenable={!canOpenAsset(group.path)}
            title={canOpenAsset(group.path)
              ? `${group.path}:${hit.line}`
              : `${group.path}:${hit.line} — no editor for this type`}
            onclick={() => void openHit(hit)}
          >
            <span class="lineno">{hit.line}</span>
            <span class="text"
              >{#each segments(hit) as seg}<span class:m={seg.hit}
                  >{seg.text}</span
                >{/each}{#if hit.clipped}<span class="dim"> …</span>{/if}</span
            >
          </button>
        {/each}
      {/if}
    {/each}
  </div>
</div>

<style>
  .find {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--ed-base);
  }

  .controls {
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 6px 8px;
    background: var(--ed-mantle);
    border-top: 1px solid var(--ed-line);
  }

  .controls input {
    flex: 1;
    min-width: 60px;
    background: var(--ed-base);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 12px;
    padding: 4px 6px;
    outline: none;
  }

  .controls input:focus {
    border-color: var(--ed-accent);
  }

  .toggles {
    display: flex;
    gap: 2px;
  }

  .toggles button {
    background: none;
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--ed-faint);
    cursor: pointer;
    font-family: "Cascadia Code", monospace;
    font-size: 11px;
    padding: 3px 6px;
  }

  .toggles button:hover {
    color: var(--ed-text);
  }

  .toggles button.on {
    background: var(--ed-raised);
    border-color: var(--ed-line-strong);
    color: var(--ed-accent);
  }

  .toggles button.go {
    font-family: inherit;
    color: var(--ed-muted);
  }

  .replace-row {
    border-top: none;
    padding-top: 0;
  }

  .toggles button.danger {
    font-family: inherit;
    color: var(--ed-err);
  }

  .toggles button.danger:disabled {
    color: var(--ed-faint);
    cursor: default;
  }

  .preview {
    max-height: 40%;
    overflow-y: auto;
    border-bottom: 1px solid var(--ed-line);
    background: var(--ed-mantle);
    font-family: "Cascadia Code", "Fira Code", monospace;
    font-size: 11px;
  }

  .preview-head {
    padding: 4px 10px;
    color: var(--ed-warn);
    font-family: inherit;
  }

  .pfile {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 10px;
    color: var(--ed-text);
    cursor: pointer;
  }

  .struck {
    text-decoration: line-through;
    color: var(--ed-faint);
  }

  .pdiff {
    display: flex;
    gap: 8px;
    padding: 1px 10px 1px 30px;
    white-space: pre;
    overflow: hidden;
  }

  .before {
    color: var(--ed-err);
    text-decoration: line-through;
  }

  .after {
    color: var(--ed-ok);
  }

  .status {
    padding: 3px 10px;
    font-size: 11px;
    color: var(--ed-muted);
    border-bottom: 1px solid var(--ed-line);
  }

  .status.error {
    color: var(--ed-err);
  }

  .warn {
    color: var(--ed-warn);
  }

  .dim {
    color: var(--ed-faint);
  }

  /* Dimmed while the list describes a query the box no longer holds - it is
     still useful, it is just not an answer to what is typed. */
  .results.stale {
    opacity: 0.55;
  }

  .results {
    flex: 1;
    /* Without this the list keeps `min-height: auto`, refuses to shrink to
       the panel, and pushes the whole panel past the bottom of the dock -
       so `overflow-y` has nothing to scroll and hundreds of hits are simply
       unreachable. */
    min-height: 0;
    overflow-y: auto;
    padding: 2px 0;
    font-family: "Cascadia Code", "Fira Code", monospace;
    font-size: 12px;
  }

  .file-row,
  .hit-row {
    display: flex;
    align-items: baseline;
    gap: 6px;
    width: 100%;
    border: none;
    background: none;
    text-align: left;
    cursor: pointer;
    padding: 2px 10px;
    font-family: inherit;
    font-size: inherit;
  }

  .file-row {
    color: var(--ed-text);
    margin-top: 2px;
  }

  .file-row:hover,
  .hit-row:hover {
    background: var(--ed-hover);
  }

  .chev {
    color: var(--ed-faint);
    font-size: 9px;
    width: 10px;
  }

  .fname {
    color: var(--ed-text);
  }

  .fdir {
    color: var(--ed-faint);
    font-size: 11px;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    color: var(--ed-faint);
    font-size: 11px;
  }

  .hit-row {
    padding-left: 26px;
    color: var(--ed-text);
  }

  .hit-row.unopenable {
    opacity: 0.5;
  }

  .lineno {
    color: var(--ed-faint);
    min-width: 34px;
    text-align: right;
    font-size: 11px;
  }

  .text {
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .m {
    background: color-mix(in srgb, var(--ed-warn) 22%, transparent);
    color: var(--ed-warn);
  }

  .empty {
    padding: 10px;
    color: var(--ed-faint);
    font-size: 12px;
    font-style: italic;
  }
</style>
