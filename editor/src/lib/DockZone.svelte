<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { PanelId, ZoneId, PanelGroup } from './stores/layoutStore';
  import { setActivePanel, movePanel, isPanelDragging, PANEL_NAMES } from './stores/layoutStore';
  import { logs } from './stores/consoleStore';

  let {
    zone,
    groups,
    renderPanel,
  }: {
    zone: ZoneId;
    groups: PanelGroup[];
    renderPanel: Snippet<[PanelId]>;
  } = $props();

  /** The Console tab carries its error count, so an error is seen from any tab. */
  let consoleErrors = $derived($logs.filter((e) => e.level === 'error').length);

  let dropTarget = $state<{ groupIdx: number; position: 'tab' | 'before' | 'after' } | null>(null);

  const DRAG_TYPE = 'application/jengine-panel';

  function onTabDragStart(e: DragEvent, panelId: PanelId) {
    e.dataTransfer!.setData(DRAG_TYPE, panelId);
    e.dataTransfer!.effectAllowed = 'move';
    isPanelDragging.set(true);
  }

  function onTabDragEnd() {
    isPanelDragging.set(false);
    dropTarget = null;
  }

  function isPanel(e: DragEvent): boolean {
    const types = e.dataTransfer?.types;
    if (!types) return false;
    return Array.from(types).includes(DRAG_TYPE);
  }

  function handleTabBarDragOver(e: DragEvent, groupIdx: number) {
    if (!isPanel(e)) return;
    e.preventDefault();
    e.dataTransfer!.dropEffect = 'move';
    dropTarget = { groupIdx, position: 'tab' };
  }

  function handleContentDragOver(e: DragEvent, groupIdx: number) {
    if (!isPanel(e)) return;
    e.preventDefault();
    e.dataTransfer!.dropEffect = 'move';
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const ratio = (e.clientY - rect.top) / rect.height;
    if (ratio < 0.3) {
      dropTarget = { groupIdx, position: 'before' };
    } else if (ratio > 0.7) {
      dropTarget = { groupIdx, position: 'after' };
    } else {
      dropTarget = { groupIdx, position: 'tab' };
    }
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    const panelId = e.dataTransfer?.getData(DRAG_TYPE) as PanelId;
    if (panelId && dropTarget) {
      movePanel(panelId, zone, dropTarget.groupIdx, dropTarget.position);
    }
    dropTarget = null;
    isPanelDragging.set(false);
  }

  function handleEmptyDrop(e: DragEvent) {
    e.preventDefault();
    const panelId = e.dataTransfer?.getData(DRAG_TYPE) as PanelId;
    if (panelId) {
      movePanel(panelId, zone, 0, 'tab');
    }
    dropTarget = null;
    isPanelDragging.set(false);
  }

  function handleZoneDragLeave(e: DragEvent) {
    const related = e.relatedTarget as HTMLElement | null;
    if (!related || !(e.currentTarget as HTMLElement).contains(related)) {
      dropTarget = null;
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="dock-zone" ondragleave={handleZoneDragLeave}>
  {#if groups.length === 0}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="dock-empty"
      class:drop-highlight={dropTarget !== null}
      ondragover={(e) => {
        if (!isPanel(e)) return;
        e.preventDefault();
        e.dataTransfer!.dropEffect = 'move';
        dropTarget = { groupIdx: 0, position: 'tab' };
      }}
      ondrop={handleEmptyDrop}
    >
      <span class="drop-hint">+</span>
    </div>
  {:else}
    {#each groups as group, gi (gi)}
      {#if dropTarget?.groupIdx === gi && dropTarget.position === 'before'}
        <div class="drop-line"></div>
      {/if}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="dock-group">
        <div
          class="dock-tabs"
          class:drop-tab-highlight={dropTarget?.groupIdx === gi && dropTarget.position === 'tab'}
          ondragover={(e) => handleTabBarDragOver(e, gi)}
          ondrop={handleDrop}
          role="tablist"
          tabindex="0"
        >
          {#each group.panels as panel (panel)}
            <button
              class="dock-tab"
              class:active={group.active === panel}
              onclick={() => setActivePanel(zone, gi, panel)}
              draggable="true"
              ondragstart={(e) => onTabDragStart(e, panel)}
              ondragend={onTabDragEnd}
              role="tab"
              aria-selected={group.active === panel}
            >
              {PANEL_NAMES[panel]}
              {#if panel === 'console' && consoleErrors > 0}
                <span class="tab-badge" title="{consoleErrors} error{consoleErrors === 1 ? '' : 's'}">{consoleErrors}</span>
              {/if}
            </button>
          {/each}
        </div>
        <div
          class="dock-content"
          ondragover={(e) => handleContentDragOver(e, gi)}
          ondrop={handleDrop}
          role="tabpanel"
          tabindex="0"
        >
          {#if group.active}
            {@render renderPanel(group.active)}
          {/if}
        </div>
      </div>
      {#if dropTarget?.groupIdx === gi && dropTarget.position === 'after'}
        <div class="drop-line"></div>
      {/if}
    {/each}
  {/if}
</div>

<style>
  .dock-zone {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .dock-group {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 48px;
    overflow: hidden;
  }

  .dock-tabs {
    display: flex;
    background: var(--ed-mantle);
    border-bottom: 1px solid var(--ed-line);
    flex-shrink: 0;
    min-height: 30px;
    padding: 0 4px;
    transition: background 0.1s;
    /* The strip is how you reach every other panel, so it must stay on top
       of whatever the active panel does. A panel whose content overflows its
       box would otherwise paint across it and leave only the active tab's
       2px underline showing. */
    position: relative;
    z-index: 1;
  }

  .dock-tabs.drop-tab-highlight {
    background: color-mix(in srgb, var(--ed-accent) 15%, transparent);
  }

  .dock-tab {
    padding: 0 10px;
    font-family: var(--ed-mono);
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ed-faint);
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    cursor: grab;
    user-select: none;
  }

  .dock-tab:hover {
    color: var(--ed-text);
  }

  .tab-badge {
    display: inline-block;
    margin-left: 6px;
    padding: 0 5px;
    border-radius: 8px;
    background: var(--ed-accent-low);
    color: var(--ed-accent-high);
    font-size: 10px;
    letter-spacing: 0;
  }

  .dock-tab.active {
    color: var(--ed-ink);
    border-bottom-color: var(--ed-accent);
  }

  .dock-content {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .drop-line {
    height: 2px;
    background: var(--ed-accent);
    flex-shrink: 0;
  }

  .dock-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    min-width: 28px;
    min-height: 28px;
    border: 1px dashed var(--ed-line-strong);
    border-radius: 4px;
    margin: 2px;
    transition: background 0.15s, border-color 0.15s;
  }

  .dock-empty.drop-highlight {
    background: color-mix(in srgb, var(--ed-accent) 10%, transparent);
    border-color: var(--ed-accent);
  }

  .drop-hint {
    color: var(--ed-faint);
    font-size: 16px;
    font-weight: bold;
  }

  .dock-empty.drop-highlight .drop-hint {
    color: var(--ed-accent);
  }
</style>
