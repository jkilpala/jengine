<script lang="ts">
  import { toasts, dismissToast, type Toast } from "./stores/toastStore";

  function activate(t: Toast) {
    t.action?.();
    dismissToast(t.id);
  }
</script>

<!-- The design kit's toasts: a stack in the bottom-right corner, newest at
     the bottom, each a card with its state's bar. Clicking does its action
     (opens the script, shows the Build panel); x just closes it. -->
<div class="toasts" role="status" aria-live="polite">
  {#each $toasts as t (t.id)}
    <div class="toast {t.kind}">
      <span class="bar" aria-hidden="true"></span>
      <button
        class="body"
        class:actionable={!!t.action}
        onclick={() => activate(t)}
        title={t.action ? "Open" : undefined}
      >
        <span class="title">{t.title}</span>
        {#if t.detail}
          <span class="detail" class:mono={t.mono}>{t.detail}</span>
        {/if}
      </button>
      <button class="close" aria-label="Dismiss" onclick={() => dismissToast(t.id)}>×</button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 32px;
    z-index: 2000;
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 300px;
    pointer-events: none;
  }

  .toast {
    display: flex;
    align-items: stretch;
    gap: 10px;
    padding: 10px 8px 10px 12px;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 7px;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
    pointer-events: auto;
    animation: rise 0.15s ease-out;
  }

  @keyframes rise {
    from {
      transform: translateY(6px);
      opacity: 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .toast {
      animation: none;
    }
  }

  .bar {
    width: 3px;
    flex-shrink: 0;
    border-radius: 2px;
    background: var(--ed-muted);
  }

  .ok .bar {
    background: var(--ed-ok);
  }

  .warn .bar {
    background: var(--ed-warn);
  }

  .err .bar {
    background: var(--ed-err);
  }

  .body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: default;
  }

  .body.actionable {
    cursor: pointer;
  }

  .title {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--ed-ink);
  }

  .detail {
    font-size: 12px;
    color: var(--ed-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail.mono {
    font-family: var(--ed-mono);
    font-size: 11px;
  }

  .body.actionable:hover .detail {
    color: var(--ed-a2);
  }

  .close {
    align-self: flex-start;
    width: 20px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: 4px;
    background: none;
    color: var(--ed-faint);
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
  }

  .close:hover {
    background: var(--ed-hover);
    color: var(--ed-ink);
  }
</style>
