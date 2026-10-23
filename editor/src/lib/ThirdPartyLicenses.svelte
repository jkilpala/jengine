<script lang="ts">
  /**
   * Help > Third-Party Licences: every crate, npm package and bundled asset
   * that ships, from `generated/third-party.json`. The licence texts
   * themselves are in THIRD-PARTY-NOTICES.md (both written by
   * `tools/release/third_party_notices.mjs`), linked at the bottom.
   */
  import { onMount } from "svelte";
  import thirdParty from "./generated/third-party.json";
  import { openExternalUrl } from "./api/help";
  import { LINKS } from "./links";
  import { logError } from "./stores/consoleStore";

  interface Props {
    onclose: () => void;
  }
  let { onclose }: Props = $props();

  interface Row {
    name: string;
    version: string;
    license: string;
    repository: string;
    note?: string;
  }

  type Tab = "rust" | "npm" | "assets";
  const TABS: { id: Tab; label: string; rows: Row[] }[] = [
    { id: "rust", label: "Rust crates", rows: thirdParty.rust },
    { id: "npm", label: "npm packages", rows: thirdParty.npm },
    {
      id: "assets",
      label: "Bundled",
      rows: thirdParty.assets.map((a) => ({ version: "", ...a })),
    },
  ];

  let tab = $state<Tab>("rust");
  let query = $state("");
  let searchEl: HTMLInputElement | undefined = $state();

  onMount(() => searchEl?.focus());

  let rows = $derived.by(() => {
    const all = TABS.find((t) => t.id === tab)!.rows;
    const q = query.trim().toLowerCase();
    if (!q) return all;
    return all.filter(
      (r) => r.name.toLowerCase().includes(q) || r.license.toLowerCase().includes(q),
    );
  });

  function open(url: string) {
    openExternalUrl(url).catch((e) => logError(`Could not open ${url}: ${e}`));
  }

  function onKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()} onkeydown={onKeydown}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="tp-title">
    <header>
      <h2 id="tp-title">Third-party licences</h2>
      <button class="close-btn" onclick={onclose} aria-label="Close">×</button>
    </header>

    <div class="controls">
      <div class="segmented" role="tablist">
        {#each TABS as t (t.id)}
          <button
            role="tab"
            aria-selected={tab === t.id}
            class:active={tab === t.id}
            onclick={() => (tab = t.id)}
          >
            {t.label} <span class="count">{t.rows.length}</span>
          </button>
        {/each}
      </div>
      <input
        class="search"
        type="search"
        placeholder="Filter by name or licence"
        bind:value={query}
        bind:this={searchEl}
      />
    </div>

    <div class="list" role="tabpanel">
      {#each rows as r (`${r.name}@${r.version}`)}
        <div class="item">
          <div class="main">
            {#if r.repository}
              <button class="link name" onclick={() => open(r.repository)}>{r.name}</button>
            {:else}
              <span class="name">{r.name}</span>
            {/if}
            {#if r.version}<span class="version">{r.version}</span>{/if}
          </div>
          <span class="license">{r.license}</span>
          {#if r.note}<div class="note">{r.note}</div>{/if}
        </div>
      {:else}
        <p class="empty">Nothing matches “{query}”.</p>
      {/each}
    </div>

    <footer>
      <p class="hint">The full licence texts are in THIRD-PARTY-NOTICES.md.</p>
      <button class="btn" onclick={() => open(LINKS.thirdPartyNotices)}>Open notices</button>
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
    z-index: 4001;
  }

  .dialog {
    width: min(640px, 94vw);
    height: min(640px, 86vh);
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
    padding: 12px 16px;
    border-bottom: 1px solid var(--ed-line);
  }

  h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--ed-ink);
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

  .controls {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 10px 16px;
  }

  /* The kit's segmented control: a mantle well, the chosen segment raised
     with an accent underline. */
  .segmented {
    display: flex;
    padding: 2px;
    gap: 2px;
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-radius: 6px;
  }

  .segmented button {
    border: none;
    background: none;
    color: var(--ed-muted);
    font-size: 12px;
    padding: 4px 10px;
    border-radius: 4px;
    cursor: pointer;
    border-bottom: 2px solid transparent;
  }

  .segmented button:hover {
    color: var(--ed-text);
  }

  .segmented button.active {
    background: var(--ed-raised);
    color: var(--ed-ink);
    border-bottom-color: var(--ed-accent);
  }

  .count {
    font-family: var(--ed-mono);
    font-size: 10.5px;
    color: var(--ed-faint);
  }

  .search {
    flex: 1;
    min-width: 0;
    height: 26px;
    padding: 0 8px;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 12px;
  }

  .list {
    flex: 1;
    overflow-y: auto;
    padding: 0 16px 8px;
    user-select: text;
  }

  .item {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 2px 12px;
    padding: 5px 0;
    border-bottom: 1px solid var(--ed-line);
    font-size: 12px;
  }

  .main {
    display: flex;
    gap: 8px;
    align-items: baseline;
    min-width: 0;
  }

  .name {
    color: var(--ed-text);
    font-weight: 600;
  }

  .version,
  .license {
    font-family: var(--ed-mono);
    font-size: 11px;
    color: var(--ed-muted);
  }

  .note {
    grid-column: 1 / -1;
    color: var(--ed-muted);
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    cursor: pointer;
    text-align: left;
  }

  .link:hover {
    color: var(--ed-a2);
    text-decoration: underline;
  }

  .empty {
    padding: 16px 0;
    color: var(--ed-faint);
    font-size: 12px;
  }

  footer {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 10px 16px;
    border-top: 1px solid var(--ed-line);
    background: var(--ed-mantle);
  }

  .hint {
    flex: 1;
    margin: 0;
    font-size: 11.5px;
    color: var(--ed-muted);
  }

  .btn {
    border: 1px solid var(--ed-line-strong);
    background: var(--ed-raised);
    color: var(--ed-text);
    border-radius: 4px;
    cursor: pointer;
    font-size: 12.5px;
    padding: 5px 12px;
  }

  .btn:hover {
    background: var(--ed-hover);
  }

  .btn.primary {
    background: var(--ed-accent);
    border-color: var(--ed-accent);
    color: var(--ed-on-accent);
  }

  .btn.primary:hover {
    box-shadow: 0 0 12px color-mix(in srgb, var(--ed-accent) 45%, transparent);
  }
</style>
