<script lang="ts">
  let {
    frames = [],
    selectedFrame = 0,
    fps = 10,
    looping = true,
    playing = false,
    projectPath = "",
    onselect,
    onreorder,
    onaddframes,
    onremoveframe,
    onfpschange,
    onloopingchange,
    onplaytoggle,
  }: {
    frames: string[];
    selectedFrame: number;
    fps: number;
    looping: boolean;
    playing: boolean;
    projectPath: string;
    onselect: (index: number) => void;
    onreorder: (from: number, to: number) => void;
    onaddframes: () => void;
    onremoveframe: (index: number) => void;
    onfpschange: (fps: number) => void;
    onloopingchange: (looping: boolean) => void;
    onplaytoggle: () => void;
  } = $props();

  let dragIdx = $state(-1);
  let dropIdx = $state(-1);

  function getFilename(path: string): string {
    const parts = path.split("/");
    return parts[parts.length - 1] ?? path;
  }

  function handleDragStart(e: DragEvent, idx: number) {
    if (!e.dataTransfer) return;
    e.dataTransfer.effectAllowed = "move";
    dragIdx = idx;
  }

  function handleDragOver(e: DragEvent, idx: number) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    dropIdx = idx;
  }

  function handleDrop(e: DragEvent, idx: number) {
    e.preventDefault();
    if (dragIdx >= 0 && dragIdx !== idx) {
      onreorder(dragIdx, idx);
    }
    dragIdx = -1;
    dropIdx = -1;
  }

  function handleDragEnd() {
    dragIdx = -1;
    dropIdx = -1;
  }

  function stepFrame(delta: number) {
    if (frames.length === 0) return;
    let next = selectedFrame + delta;
    if (next < 0) next = looping ? frames.length - 1 : 0;
    if (next >= frames.length) next = looping ? 0 : frames.length - 1;
    onselect(next);
  }
</script>

<div class="timeline">
  <!-- Playback controls bar -->
  <div class="controls">
    <button
      class="ctrl-btn"
      title="Previous frame"
      onclick={() => stepFrame(-1)}>⏮</button
    >
    <button
      class="ctrl-btn play-btn"
      title={playing ? "Pause" : "Play"}
      onclick={onplaytoggle}
    >
      {playing ? "⏸" : "▶"}
    </button>
    <button class="ctrl-btn" title="Next frame" onclick={() => stepFrame(1)}
      >⏭</button
    >

    <span class="frame-info">
      {#if frames.length > 0}
        Frame {selectedFrame + 1}/{frames.length}
      {:else}
        No frames
      {/if}
    </span>

    <div class="spacer"></div>

    <label class="fps-label" title="Frames per second">
      FPS
      <input
        type="number"
        class="fps-input"
        min="1"
        max="60"
        step="1"
        value={fps}
        onchange={(e) =>
          onfpschange(
            parseInt((e.currentTarget as HTMLInputElement).value) || 10,
          )}
      />
    </label>

    <label class="loop-label" title="Loop animation">
      <input
        type="checkbox"
        checked={looping}
        onchange={(e) =>
          onloopingchange((e.currentTarget as HTMLInputElement).checked)}
      />
      Loop
    </label>
  </div>

  <!-- Frame strip -->
  <div class="frame-strip">
    {#each frames as frame, i}
      <div
        class="frame-thumb"
        class:selected={i === selectedFrame}
        class:drag-over={i === dropIdx && dragIdx !== i}
        draggable="true"
        onclick={() => onselect(i)}
        onkeydown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onselect(i);
          }
        }}
        ondragstart={(e) => handleDragStart(e, i)}
        ondragover={(e) => handleDragOver(e, i)}
        ondrop={(e) => handleDrop(e, i)}
        ondragend={handleDragEnd}
        ondragleave={() => {
          if (dropIdx === i) dropIdx = -1;
        }}
        title={getFilename(frame)}
        role="button"
        tabindex={0}
      >
        <span class="frame-number">{i + 1}</span>
        <span class="frame-name"
          >{getFilename(frame).replace(/\.\w+$/, "")}</span
        >
        <button
          class="remove-frame-btn"
          title="Remove frame"
          onclick={(e) => {
            e.stopPropagation();
            onremoveframe(i);
          }}>×</button
        >
      </div>
    {/each}

    <button class="add-frame-btn" title="Add frames" onclick={onaddframes}>
      +
    </button>
  </div>
</div>

<style>
  .timeline {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--ed-line);
    background: var(--ed-base);
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--ed-line);
  }

  .ctrl-btn {
    background: none;
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 12px;
    padding: 3px 6px;
    cursor: pointer;
    min-width: 28px;
  }

  .ctrl-btn:hover {
    background: var(--ed-raised);
  }

  .play-btn {
    color: var(--ed-ok);
  }

  .frame-info {
    font-size: 11px;
    color: var(--ed-muted);
    margin-left: 4px;
  }

  .spacer {
    flex: 1;
  }

  .fps-label {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--ed-muted);
  }

  .fps-input {
    width: 40px;
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 11px;
    padding: 2px 4px;
    text-align: center;
  }

  .fps-input:focus {
    border-color: var(--ed-accent);
    outline: none;
  }

  .loop-label {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--ed-muted);
    cursor: pointer;
  }

  .loop-label input {
    accent-color: var(--ed-accent);
  }

  .frame-strip {
    display: flex;
    gap: 4px;
    padding: 6px 8px;
    overflow-x: auto;
    min-height: 50px;
  }

  .frame-thumb {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-width: 60px;
    max-width: 80px;
    padding: 4px 6px;
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    cursor: pointer;
    position: relative;
    user-select: none;
    background: var(--ed-mantle);
  }

  .frame-thumb:hover {
    border-color: var(--ed-line-strong);
    background: var(--ed-raised);
  }

  .frame-thumb.selected {
    border-color: var(--ed-accent);
    background: color-mix(in srgb, var(--ed-accent) 10%, transparent);
  }

  .frame-thumb.drag-over {
    border-color: var(--ed-accent);
    border-style: dashed;
  }

  .frame-number {
    font-size: 10px;
    color: var(--ed-muted);
  }

  .frame-name {
    font-size: 10px;
    color: var(--ed-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 70px;
  }

  .remove-frame-btn {
    position: absolute;
    top: 1px;
    right: 2px;
    background: none;
    border: none;
    color: var(--ed-err);
    font-size: 12px;
    cursor: pointer;
    opacity: 0;
    padding: 0 2px;
    line-height: 1;
  }

  .frame-thumb:hover .remove-frame-btn {
    opacity: 0.7;
  }

  .remove-frame-btn:hover {
    opacity: 1 !important;
  }

  .add-frame-btn {
    min-width: 40px;
    border: 1px dashed var(--ed-line);
    border-radius: 4px;
    background: none;
    color: var(--ed-accent);
    font-size: 18px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .add-frame-btn:hover {
    border-color: var(--ed-accent);
    background: color-mix(in srgb, var(--ed-accent) 5%, transparent);
  }
</style>
