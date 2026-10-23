<script lang="ts" module>
  /**
   * Save / Discard / Cancel for unsaved work.
   *
   * An in-app modal rather than `window.confirm`, for two reasons. The obvious
   * one is that `confirm` offers exactly two answers, and "discard my work"
   * and "don't close" are not the only two a person wants — the third,
   * *save it*, is the one they usually mean. The less obvious one is that a
   * native dialog inside a WebView is at the host's discretion: if it does not
   * render, the caller silently gets the default and the work is gone, which
   * is precisely the failure this dialog exists to prevent.
   */
  export type UnsavedChoice = "save" | "discard" | "cancel";
</script>

<script lang="ts">
  interface Props {
    /** What is about to be lost, one line each. */
    items: string[];
    /** Verb for the action being confirmed, e.g. "Close scene". */
    title?: string;
    /**
     * Whether Save can actually write this document.
     *
     * The state machine and vector path editors apply to the entity from
     * inside their own components and expose nothing a caller can invoke, so
     * offering Save there would be a button that quietly does nothing - worse
     * than no button, because the user would believe the work was kept.
     */
    canSave?: boolean;
    onchoose: (choice: UnsavedChoice) => void;
  }
  let {
    items,
    title = "Unsaved changes",
    canSave = true,
    onchoose,
  }: Props = $props();

  let dialogEl: HTMLDivElement | undefined = $state();

  // Focus Cancel, not Discard: the safe option should be the one a stray
  // Enter or Space lands on.
  let cancelEl: HTMLButtonElement | undefined = $state();
  $effect(() => {
    cancelEl?.focus();
  });

  function onKeydown(e: KeyboardEvent) {
    // Nothing here may reach the editor's global shortcuts while a modal is
    // up — Ctrl+S behind this dialog would be genuinely confusing.
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onchoose("cancel");
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="overlay" onkeydown={onKeydown} role="presentation">
  <div
    class="dialog"
    bind:this={dialogEl}
    role="alertdialog"
    aria-modal="true"
    aria-label={title}
  >
    <h2>{title}</h2>
    <p class="lead">
      {items.length === 1
        ? "This has unsaved changes:"
        : "These have unsaved changes:"}
    </p>
    <ul>
      {#each items as item}
        <li>{item}</li>
      {/each}
    </ul>
    {#if !canSave}
      <p class="lead">
        This editor applies its changes to the scene rather than to a file, so
        it cannot be saved from here - cancel and apply it from its own panel.
      </p>
    {/if}
    <div class="actions">
      {#if canSave}
        <button class="primary" onclick={() => onchoose("save")}>Save</button>
      {/if}
      <button class="danger" onclick={() => onchoose("discard")}>Discard</button>
      <button bind:this={cancelEl} onclick={() => onchoose("cancel")}
        >Cancel</button
      >
    </div>
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
    /* Above every panel and both palettes. */
    z-index: 4000;
  }

  .dialog {
    width: min(440px, 88vw);
    background: var(--ed-base);
    border: 1px solid var(--ed-line-strong);
    border-radius: 6px;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
    padding: 16px 18px 14px;
    color: var(--ed-text);
  }

  h2 {
    margin: 0 0 8px;
    font-size: 15px;
    font-weight: 600;
  }

  .lead {
    margin: 0 0 6px;
    font-size: 12px;
    color: var(--ed-muted);
  }

  ul {
    margin: 0 0 14px;
    padding-left: 18px;
    max-height: 180px;
    overflow-y: auto;
    font-family: "Cascadia Code", "Fira Code", monospace;
    font-size: 12px;
  }

  li {
    padding: 1px 0;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }

  .actions button {
    border: 1px solid var(--ed-line-strong);
    background: var(--ed-raised);
    color: var(--ed-text);
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
    padding: 5px 12px;
  }

  .actions button:hover {
    background: var(--ed-line);
  }

  .actions button:focus-visible {
    outline: 2px solid var(--ed-accent);
    outline-offset: 1px;
  }

  .actions .primary {
    background: var(--ed-accent);
    border-color: var(--ed-accent);
    color: var(--ed-on-accent);
  }

  .actions .primary:hover {
    background: var(--ed-accent);
    filter: brightness(1.1);
  }

  .actions .danger {
    color: var(--ed-err);
    background: color-mix(in srgb, var(--ed-err) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--ed-err) 50%, transparent);
  }

  .actions .danger:hover {
    background: color-mix(in srgb, var(--ed-err) 18%, transparent);
  }
</style>
