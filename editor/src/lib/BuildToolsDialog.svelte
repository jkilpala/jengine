<script lang="ts">
  /**
   * Help > Check Build Tools: what building a game needs on this machine
   * (`commands/build_tools.rs`), what is missing, and the way to get it - an
   * install the editor runs, or a download page in the browser.
   */
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { checkBuildTools, installBuildTool } from "./api/buildTools";
  import { openExternalUrl } from "./api/help";
  import { statusGlyph, summarize, type BuildToolsReport, type ToolRow } from "./buildTools";
  import { beginBuild, buildRunning, endBuild } from "./stores/buildStore";
  import { logError } from "./stores/consoleStore";

  interface Props {
    onclose: () => void;
  }
  let { onclose }: Props = $props();

  let report = $state<BuildToolsReport | null>(null);
  let checking = $state(false);
  let failed = $state<string | null>(null);
  /** The install id running, if any. */
  let installing = $state<string | null>(null);
  let closeEl: HTMLButtonElement | undefined = $state();

  const summary = $derived(report ? summarize(report) : null);
  const required = $derived(report?.rows.filter((r) => r.group !== "optional") ?? []);
  const optional = $derived(report?.rows.filter((r) => r.group === "optional") ?? []);

  async function recheck() {
    if (checking) return;
    checking = true;
    failed = null;
    try {
      report = await checkBuildTools();
    } catch (e) {
      failed = `${e}`;
    } finally {
      checking = false;
    }
  }

  onMount(() => {
    closeEl?.focus();
    void recheck();
  });

  function open(url: string) {
    openExternalUrl(url).catch((e) => logError(`Could not open ${url}: ${e}`));
  }

  async function install(row: ToolRow, id: string) {
    if (installing) return;
    // The builder streams one process at a time into the Build panel.
    if (get(buildRunning)) {
      logError("A build is running - install once it has finished.");
      return;
    }
    installing = id;
    beginBuild(`Install ${row.name}`);
    try {
      endBuild(`${await installBuildTool(id)} - ${row.name}`);
    } catch (e) {
      endBuild(`Install of ${row.name} failed: ${e}`);
      logError(`Install of ${row.name} failed: ${e}`);
    } finally {
      installing = null;
      await recheck();
    }
  }

  function onKeydown(e: KeyboardEvent) {
    // Keep the editor's shortcuts from firing behind the dialog.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }
</script>

{#snippet toolRow(row: ToolRow)}
  {@const s = statusGlyph(row.status)}
  <li class="tool {row.status}">
    <span class="icon" role="img" aria-label={s.label} title={s.label}>{s.glyph}</span>
    <div class="what">
      <div class="line">
        <span class="name">{row.name}</span>
        {#if row.version}<span class="version">{row.version}</span>{/if}
      </div>
      <div class="for">{row.needed_for}</div>
      {#if row.detail}<div class="detail">{row.detail}</div>{/if}
    </div>
    <div class="act">
      {#if row.status !== "ok" && row.action?.kind === "install"}
        {@const id = row.action.id}
        <button class="btn primary" disabled={!!installing || checking} onclick={() => install(row, id)}>
          {installing === id ? "Installing…" : row.action.label}
        </button>
        {#if row.help_url}
          {@const url = row.help_url}
          <button class="link" onclick={() => open(url)}>or download</button>
        {/if}
      {:else if row.status !== "ok" && row.action?.kind === "download"}
        {@const url = row.action.url}
        <button class="btn" onclick={() => open(url)} title={url}>{row.action.label} ↗</button>
      {/if}
    </div>
  </li>
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()} onkeydown={onKeydown}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="build-tools-title">
    <header>
      <h2 id="build-tools-title">Build Tools</h2>
      <button class="close-btn" bind:this={closeEl} onclick={onclose} aria-label="Close">×</button>
    </header>
    <div class="rule" aria-hidden="true"></div>

    <div class="body">
      <p class="summary {summary?.level ?? ''}" aria-live="polite">
        {#if failed}
          The check could not run: {failed}
        {:else if summary && !checking}
          {summary.text}
        {:else if summary}
          {summary.text} <span class="muted">(checking again…)</span>
        {:else}
          Checking what is installed…
        {/if}
      </p>

      {#if report}
        <ul class="tools">
          {#each required as row (row.id)}
            {@render toolRow(row)}
          {/each}
        </ul>
        {#if optional.length}
          <h3 class="eyebrow">Optional</h3>
          <ul class="tools">
            {#each optional as row (row.id)}
              {@render toolRow(row)}
            {/each}
          </ul>
        {/if}
        <p class="note">
          Building a game compiles the engine from source, so these are installed beside the editor. Installers that
          change PATH take effect after the editor is restarted.
        </p>
      {/if}
    </div>

    <footer>
      <button class="btn" onclick={recheck} disabled={checking || !!installing}>
        {checking ? "Checking…" : "Re-check"}
      </button>
      <button class="btn primary" onclick={onclose}>Close</button>
    </footer>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 4000;
  }

  .dialog {
    width: min(640px, 92vw);
    max-height: 86vh;
    display: flex;
    flex-direction: column;
    background: var(--ed-base);
    border: 1px solid var(--ed-line-strong);
    border-radius: 8px;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
    color: var(--ed-text);
    overflow: hidden;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px 10px;
    background: var(--ed-crust);
  }

  h2 {
    margin: 0;
    font-family: var(--ed-display);
    font-weight: 400;
    font-size: 22px;
    line-height: 1;
    color: var(--ed-ink);
  }

  .rule {
    height: 2px;
    opacity: 0.85;
    background: linear-gradient(90deg, var(--ed-accent), var(--ed-a3) 55%, var(--ed-a2));
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--ed-faint);
    font-size: 18px;
    cursor: pointer;
    padding: 2px 6px;
    border-radius: 4px;
  }

  .close-btn:hover {
    color: var(--ed-text);
    background: var(--ed-hover);
  }

  .body {
    padding: 14px 16px;
    overflow-y: auto;
    user-select: text;
  }

  .summary {
    margin: 0 0 12px;
    padding: 8px 10px;
    border-radius: 5px;
    border-left: 3px solid var(--ed-line-strong);
    background: var(--ed-mantle);
    font-size: 13px;
    font-weight: 600;
  }

  .summary.ok {
    border-left-color: var(--ed-ok);
  }

  .summary.warn {
    border-left-color: var(--ed-warn);
  }

  .summary.err {
    border-left-color: var(--ed-err);
  }

  .muted {
    font-weight: 400;
    color: var(--ed-muted);
  }

  .eyebrow {
    margin: 14px 0 4px;
    font-family: var(--ed-mono);
    font-size: 11px;
    font-weight: 400;
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: var(--ed-a3);
  }

  .tools {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .tool {
    display: grid;
    grid-template-columns: 22px 1fr auto;
    gap: 10px;
    align-items: start;
    padding: 7px 0;
    border-bottom: 1px solid var(--ed-line);
    font-size: 12px;
  }

  .tool:last-child {
    border-bottom: none;
  }

  /* A shape per status (check, cross, bang, dash) as well as a colour. */
  .icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    border-radius: 50%;
    font-size: 11px;
    font-weight: 700;
    line-height: 1;
  }

  .ok .icon {
    background: color-mix(in srgb, var(--ed-ok) 22%, transparent);
    color: var(--ed-ok);
  }

  .missing .icon {
    background: color-mix(in srgb, var(--ed-err) 22%, transparent);
    color: var(--ed-err);
  }

  .outdated .icon {
    border-radius: 3px;
    background: color-mix(in srgb, var(--ed-warn) 22%, transparent);
    color: var(--ed-warn);
  }

  .optional_missing .icon {
    border: 1px dashed var(--ed-faint);
    color: var(--ed-faint);
  }

  .line {
    display: flex;
    gap: 8px;
    align-items: baseline;
    flex-wrap: wrap;
  }

  .name {
    font-weight: 600;
    color: var(--ed-text);
  }

  .version {
    font-family: var(--ed-mono);
    font-size: 11px;
    color: var(--ed-muted);
  }

  .for {
    color: var(--ed-muted);
  }

  .detail {
    margin-top: 2px;
    font-family: var(--ed-mono);
    font-size: 11px;
    color: var(--ed-faint);
    overflow-wrap: anywhere;
  }

  .missing .detail,
  .outdated .detail {
    color: var(--ed-text);
    font-family: var(--ed-sans);
    font-size: 12px;
  }

  .act {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 3px;
  }

  .note {
    margin: 12px 0 0;
    font-size: 11.5px;
    color: var(--ed-muted);
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    font-size: 11px;
    color: var(--ed-a2);
    cursor: pointer;
  }

  .link:hover {
    text-decoration: underline;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    padding: 10px 16px;
    border-top: 1px solid var(--ed-line);
    background: var(--ed-mantle);
  }

  .btn {
    border: 1px solid var(--ed-line-strong);
    background: var(--ed-raised);
    color: var(--ed-text);
    border-radius: 4px;
    cursor: pointer;
    font-size: 12.5px;
    padding: 5px 12px;
    white-space: nowrap;
  }

  .btn:hover:not(:disabled) {
    background: var(--ed-hover);
  }

  .btn:disabled {
    color: var(--ed-faint);
    cursor: default;
  }

  .btn.primary {
    background: var(--ed-accent);
    border-color: var(--ed-accent);
    color: var(--ed-on-accent);
  }

  .btn.primary:hover:not(:disabled) {
    background: var(--ed-accent);
    box-shadow: 0 0 12px color-mix(in srgb, var(--ed-accent) 45%, transparent);
  }

  .btn.primary:disabled {
    opacity: 0.55;
  }
</style>
