<script lang="ts">
  /**
   * Help > About jEngine: the build (version, commit, date - from the
   * editor's Rust side, so the version is the workspace's), the licence, and
   * the projects the engine is built on.
   */
  import { onMount } from "svelte";
  import { aboutInfo, openExternalUrl, type AboutInfo } from "./api/help";
  import { CREDITS, creditDetails } from "./credits";
  import { COPYRIGHT, LICENSE_NAME, LINKS, REPO, SITE } from "./links";
  import { logError } from "./stores/consoleStore";

  interface Props {
    onclose: () => void;
    /** Opens the full third-party list. */
    onlicenses: () => void;
  }
  let { onclose, onlicenses }: Props = $props();

  let info = $state<AboutInfo | null>(null);
  let failed = $state(false);
  let copied = $state(false);
  let closeEl: HTMLButtonElement | undefined = $state();

  onMount(() => {
    closeEl?.focus();
    aboutInfo()
      .then((i) => (info = i))
      .catch(() => (failed = true));
  });

  const credits = CREDITS.map((c) => ({ ...c, details: creditDetails(c) }));

  function open(url: string) {
    openExternalUrl(url).catch((e) => logError(`Could not open ${url}: ${e}`));
  }

  /** The lines a bug report wants, on the clipboard. */
  async function copyVersion() {
    if (!info) return;
    const text = `jEngine ${info.version} (${info.commit}, built ${info.build_date}, ${info.profile}) on ${info.os} ${info.arch}, Tauri ${info.tauri}`;
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      logError(`Could not copy: ${e}`);
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

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()} onkeydown={onKeydown}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="about-title">
    <header>
      <h2 id="about-title" class="wordmark">j<em>Engine</em></h2>
      <button class="close-btn" bind:this={closeEl} onclick={onclose} aria-label="Close">×</button>
    </header>
    <div class="rule" aria-hidden="true"></div>

    <div class="body">
      <dl class="build">
        <dt>Version</dt>
        <dd>{info?.version ?? (failed ? "unknown" : "…")}</dd>
        <dt>Commit</dt>
        <dd>{info?.commit ?? "…"}</dd>
        <dt>Built</dt>
        <dd>{info ? `${info.build_date} · ${info.profile}` : "…"}</dd>
        <dt>Platform</dt>
        <dd>{info ? `${info.os} ${info.arch} · Tauri ${info.tauri}` : "…"}</dd>
      </dl>
      <div class="row">
        <button class="btn" onclick={copyVersion} disabled={!info}>{copied ? "Copied" : "Copy version info"}</button>
      </div>

      <p class="legal">
        {COPYRIGHT}.<br />
        Released under the {LICENSE_NAME} licence.
        <button class="link" onclick={() => open(LINKS.license)}>Read the licence</button>
      </p>
      <p class="links">
        <button class="link" onclick={() => open(LINKS.site)}>{SITE.replace("https://", "")}</button>
        <span aria-hidden="true">·</span>
        <button class="link" onclick={() => open(REPO)}>{REPO.replace("https://", "")}</button>
      </p>

      <h3 class="eyebrow">Credits</h3>
      <p class="lead">jEngine is built on the work of these projects, and many more.</p>
      <ul class="credits">
        {#each credits as c (c.name)}
          <li>
            <button class="link name" onclick={() => open(c.url)}>{c.name}</button>
            <span class="role">{c.role}</span>
            {#if c.details}
              <span class="meta">{c.details.version ? `${c.details.version} · ` : ""}{c.details.license}</span>
            {/if}
          </li>
        {/each}
      </ul>
    </div>

    <footer>
      <button class="btn" onclick={onlicenses}>Third-party licences…</button>
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
    width: min(560px, 92vw);
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
    padding: 14px 16px 10px;
    background: var(--ed-crust);
  }

  .wordmark {
    margin: 0;
    font-family: var(--ed-display);
    font-weight: 400;
    font-size: 30px;
    line-height: 1;
    color: var(--ed-ink);
  }

  .wordmark em {
    font-style: normal;
    color: var(--ed-accent);
    text-shadow: 0 0 18px color-mix(in srgb, var(--ed-accent) 55%, transparent);
  }

  /* The brand's rule under a header bar (docs/brand, "The rule"). */
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

  .build {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 16px;
    margin: 0 0 10px;
    font-size: 12px;
  }

  dt {
    font-family: var(--ed-mono);
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--ed-muted);
  }

  dd {
    margin: 0;
    font-family: var(--ed-mono);
    color: var(--ed-text);
  }

  .row {
    margin-bottom: 14px;
  }

  .legal,
  .links,
  .lead {
    margin: 0 0 8px;
    font-size: 12px;
    color: var(--ed-muted);
  }

  .links {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-bottom: 16px;
  }

  .eyebrow {
    margin: 0 0 4px;
    font-family: var(--ed-mono);
    font-size: 11px;
    font-weight: 400;
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: var(--ed-a3);
  }

  .credits {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 2px;
  }

  .credits li {
    display: grid;
    grid-template-columns: 150px 1fr auto;
    gap: 10px;
    align-items: baseline;
    padding: 3px 0;
    border-bottom: 1px solid var(--ed-line);
    font-size: 12px;
  }

  .credits li:last-child {
    border-bottom: none;
  }

  .role {
    color: var(--ed-muted);
  }

  .meta {
    font-family: var(--ed-mono);
    font-size: 11px;
    color: var(--ed-faint);
    white-space: nowrap;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    color: var(--ed-a2);
    cursor: pointer;
    text-align: left;
  }

  .link:hover {
    text-decoration: underline;
  }

  .name {
    color: var(--ed-text);
    font-weight: 600;
  }

  .name:hover {
    color: var(--ed-a2);
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

  .btn.primary:hover {
    background: var(--ed-accent);
    box-shadow: 0 0 12px color-mix(in srgb, var(--ed-accent) 45%, transparent);
  }
</style>
