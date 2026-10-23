<script lang="ts">
  /**
   * The window's minimize / maximize / close buttons, drawn by the editor
   * because it draws its own title bar - the toolbar (and the start screen's
   * top strip) is it. Shaped like Windows' own: full-height 46 px buttons at
   * the bar's right end, close turning red on hover. On macOS there are none:
   * the native traffic lights stay, overlaid on the bar's left.
   */
  import {
    closeWindow,
    isMac,
    minimizeWindow,
    toggleMaximizeWindow,
    watchMaximized,
  } from "./api/editorWindow";

  let maximized = $state(false);

  $effect(() => {
    if (isMac) return;
    let stop: (() => void) | undefined;
    let gone = false;
    watchMaximized((m) => (maximized = m))
      .then((unlisten) => (gone ? unlisten() : (stop = unlisten)))
      .catch(() => {});
    return () => {
      gone = true;
      stop?.();
    };
  });
</script>

{#if !isMac}
  <div class="window-controls">
    <button type="button" class="wc" aria-label="Minimize" title="Minimize" onclick={minimizeWindow}>
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0 5.5 H10" stroke="currentColor" stroke-width="1" />
      </svg>
    </button>
    <button
      type="button"
      class="wc"
      aria-label={maximized ? "Restore" : "Maximize"}
      title={maximized ? "Restore" : "Maximize"}
      onclick={toggleMaximizeWindow}
    >
      {#if maximized}
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <rect x="0.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1" />
          <path d="M2.5 2.5 V0.5 H9.5 V7.5 H7.5" fill="none" stroke="currentColor" stroke-width="1" />
        </svg>
      {:else}
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1" />
        </svg>
      {/if}
    </button>
    <button type="button" class="wc close" aria-label="Close" title="Close" onclick={closeWindow}>
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0.5 0.5 L9.5 9.5 M9.5 0.5 L0.5 9.5" stroke="currentColor" stroke-width="1" />
      </svg>
    </button>
  </div>
{/if}

<style>
  .window-controls {
    display: flex;
    align-self: stretch;
    margin-right: -10px; /* flush with the window's edge, past the bar's padding */
    flex-shrink: 0;
  }

  .wc {
    width: 46px;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 0;
    background: none;
    color: var(--ed-muted);
    cursor: default;
    transition: background 0.08s, color 0.08s;
  }

  .wc:hover {
    background: var(--ed-hover);
    color: var(--ed-ink);
  }

  .wc:active {
    background: var(--ed-line);
  }

  .wc:focus-visible {
    outline: 1px solid var(--ed-accent);
    outline-offset: -2px;
  }

  /* Windows' own close red, whatever the palette: it is the one place every
     app agrees on, and a palette-coloured close reads as some other button. */
  .wc.close:hover {
    background: #c42b1c;
    color: #ffffff;
  }

  .wc.close:active {
    background: #b12a1c;
    color: #ffffff;
  }
</style>
