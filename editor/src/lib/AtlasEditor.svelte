<script lang="ts">
  import { themeColors } from "./stores/themeColors";
  import { SettledHistory } from "./settledHistory";
  import { claimUndo } from "./stores/undoTargetStore";
  import { get } from "svelte/store";
  import { project } from "./stores/projectStore";
  import { listProjectImages } from "./api/project";
  import {
    loadAtlas,
    saveAtlas,
    loadAtlasImage,
    type AtlasDefinition,
    type ImageEntry,
    type AnimationEntry,
    type SpritePoint,
  } from "./api/atlas";
  import AtlasPointsEditor from "./AtlasPointsEditor.svelte";
  import AtlasAnimationTimeline from "./AtlasAnimationTimeline.svelte";

  let {
    atlasPath,
    onclose,
  }: {
    atlasPath: string;
    onclose: () => void;
  } = $props();

  // ── Atlas state ────────────────────────────────────────────
  let definition = $state<AtlasDefinition>({
    name: "",
    images: [],
    animations: [],
    shared_points: [],
  });
  let dirty = $state(false);

  // Undo: the definition, a step each time it settles after a change
  // (`settledHistory.ts`); Ctrl+Z / Ctrl+Y are this editor's while it is open.
  const history = new SettledHistory<AtlasDefinition>(
    () => $state.snapshot(definition) as AtlasDefinition,
    (v) => {
      definition = v;
      dirty = true;
    },
  );
  $effect(() => {
    JSON.stringify(definition);
    history.observe();
  });
  $effect(() => claimUndo({ undo: () => history.undo(), redo: () => history.redo() }));
  let saving = $state(false);
  let error = $state("");

  // ── Selection ──────────────────────────────────────────────
  type Selection =
    | { kind: "image"; index: number }
    | { kind: "animation"; index: number }
    | { kind: "frame"; animIndex: number; frameIndex: number }
    | null;
  let selection = $state<Selection>(null);

  // ── Image picker ───────────────────────────────────────────
  let availableImages = $state<string[]>([]);
  let showImagePicker = $state(false);
  let imagePickerTarget = $state<"images" | "frames">("images");

  // ── Canvas ─────────────────────────────────────────────────
  let canvasRef = $state<HTMLCanvasElement | null>(null);
  let previewImage = $state<HTMLImageElement | null>(null);
  let previewWidth = $state(0);
  let previewHeight = $state(0);

  // ── Animation playback ─────────────────────────────────────
  let playing = $state(false);
  let playbackFrame = $state(0);
  let playbackTimer = $state<ReturnType<typeof setInterval> | null>(null);

  // ── Derived helpers ────────────────────────────────────────
  let selectedImage = $derived(
    selection?.kind === "image" ? definition.images[selection.index] : null,
  );
  let selectedAnim = $derived(
    selection?.kind === "animation"
      ? definition.animations[selection.index]
      : selection?.kind === "frame"
        ? definition.animations[selection.animIndex]
        : null,
  );
  let selectedFrameIndex = $derived(
    selection?.kind === "frame" ? selection.frameIndex : -1,
  );

  // Current image path to preview (for both images and animation frames)
  let currentPreviewPath = $derived.by(() => {
    if (selection?.kind === "image") {
      return definition.images[selection.index]?.path ?? null;
    }
    if (selection?.kind === "animation" || selection?.kind === "frame") {
      const anim =
        selection.kind === "animation"
          ? definition.animations[selection.index]
          : definition.animations[selection.animIndex];
      if (!anim || anim.frames.length === 0) return null;
      const fi =
        selection.kind === "frame"
          ? selection.frameIndex
          : playing
            ? playbackFrame
            : 0;
      return anim.frames[fi] ?? null;
    }
    return null;
  });

  // ── Load atlas on mount ────────────────────────────────────
  let loaded = false;
  $effect(() => {
    if (loaded) return;
    const proj = get(project);
    if (proj?.path && atlasPath) {
      loaded = true;
      loadAtlasData(proj.path, atlasPath);
      loadAvailableImages(proj.path);
    }
  });

  // ── Load preview image when selection changes ──────────────
  $effect(() => {
    const path = currentPreviewPath;
    const proj = get(project);
    if (path && proj?.path) {
      loadPreview(proj.path, path);
    } else {
      previewImage = null;
    }
  });

  // ── Redraw canvas ──────────────────────────────────────────
  $effect(() => {
    if (canvasRef && previewImage) {
      // Track reactive deps
      void selection;
      void definition.shared_points;
      void playbackFrame;
      drawCanvas();
    }
  });

  // ── Stop playback when switching away from animation ───────
  $effect(() => {
    if (selection?.kind !== "animation" && selection?.kind !== "frame") {
      stopPlayback();
    }
  });

  async function loadAtlasData(projectPath: string, path: string) {
    try {
      const raw = await loadAtlas(projectPath, path);
      // Rust's skip_serializing_if omits empty vecs — ensure arrays exist on JS side
      definition = {
        name: raw.name ?? "",
        images: raw.images ?? [],
        animations: raw.animations ?? [],
        shared_points: raw.shared_points ?? [],
      };
      dirty = false;
      error = "";
    } catch {
      // File doesn't exist yet — keep default empty definition
      definition.name = path.split("/").pop()?.replace(".atlas", "") ?? "";
      dirty = true;
      error = "";
    }
    // A loaded file is where undo starts.
    history.reset();
  }

  async function loadAvailableImages(projectPath: string) {
    try {
      const all = await listProjectImages(projectPath);
      availableImages = all.filter(
        (f) => !f.includes("_atlases/") && !f.endsWith(".atlas"),
      );
    } catch (e) {
      error = `Failed to list images: ${e}`;
    }
  }

  async function loadPreview(projectPath: string, imagePath: string) {
    try {
      const data = await loadAtlasImage(projectPath, imagePath);
      const img = new Image();
      img.onload = () => {
        previewImage = img;
        previewWidth = data.width;
        previewHeight = data.height;
      };
      img.src = `data:image/png;base64,${data.image_base64}`;
    } catch {
      previewImage = null;
    }
  }

  async function handleSave() {
    const proj = get(project);
    if (!proj?.path) return;
    saving = true;
    error = "";
    try {
      await saveAtlas(proj.path, atlasPath, definition);
      dirty = false;
    } catch (e) {
      error = `Failed to save: ${e}`;
    } finally {
      saving = false;
    }
  }

  function markDirty() {
    dirty = true;
  }

  // ── Image management ───────────────────────────────────────
  function addImages(paths: string[]) {
    for (const p of paths) {
      if (definition.images.some((img) => img.path === p)) continue;
      const name =
        p
          .split("/")
          .pop()
          ?.replace(/\.[^.]+$/, "") ?? p;
      definition.images = [
        ...definition.images,
        { name, path: p, anchor: [0.5, 0.5], points: [] },
      ];
    }
    definition = { ...definition };
    markDirty();
  }

  function removeImage(index: number) {
    definition.images = definition.images.filter((_, i) => i !== index);
    definition = { ...definition };
    if (selection?.kind === "image" && selection.index === index) {
      selection = null;
    }
    markDirty();
  }

  // ── Animation management ───────────────────────────────────
  function addAnimation() {
    const name = `anim_${definition.animations.length}`;
    definition.animations = [
      ...definition.animations,
      {
        name,
        fps: 10,
        looping: true,
        frames: [],
        points: [],
        frame_points: {},
      },
    ];
    definition = { ...definition };
    selection = { kind: "animation", index: definition.animations.length - 1 };
    markDirty();
  }

  function removeAnimation(index: number) {
    definition.animations = definition.animations.filter((_, i) => i !== index);
    definition = { ...definition };
    if (selection?.kind === "animation" && selection.index === index) {
      selection = null;
    }
    markDirty();
  }

  function addFramesToAnimation(paths: string[]) {
    if (selection?.kind !== "animation" && selection?.kind !== "frame") return;
    const ai =
      selection.kind === "animation" ? selection.index : selection.animIndex;
    const anim = definition.animations[ai];
    if (!anim) return;
    anim.frames = [...anim.frames, ...paths];
    definition.animations = [...definition.animations];
    definition = { ...definition };
    markDirty();
  }

  function removeFrame(animIndex: number, frameIndex: number) {
    const anim = definition.animations[animIndex];
    if (!anim) return;
    anim.frames = anim.frames.filter((_, i) => i !== frameIndex);
    // Clean up frame_points for removed index
    const newFp: Record<number, SpritePoint[]> = {};
    for (const [k, v] of Object.entries(anim.frame_points)) {
      const ki = Number(k);
      if (ki < frameIndex) newFp[ki] = v;
      else if (ki > frameIndex) newFp[ki - 1] = v;
    }
    anim.frame_points = newFp;
    definition.animations = [...definition.animations];
    definition = { ...definition };
    if (
      selection?.kind === "frame" &&
      selection.animIndex === animIndex &&
      selection.frameIndex === frameIndex
    ) {
      selection = { kind: "animation", index: animIndex };
    }
    markDirty();
  }

  function reorderFrame(animIndex: number, from: number, to: number) {
    const anim = definition.animations[animIndex];
    if (!anim) return;
    const frames = [...anim.frames];
    const [moved] = frames.splice(from, 1);
    frames.splice(to, 0, moved);
    anim.frames = frames;
    // Remap frame_points
    const newFp: Record<number, SpritePoint[]> = {};
    for (const [k, v] of Object.entries(anim.frame_points)) {
      let ki = Number(k);
      if (ki === from) ki = to;
      else if (from < to && ki > from && ki <= to) ki--;
      else if (from > to && ki >= to && ki < from) ki++;
      newFp[ki] = v;
    }
    anim.frame_points = newFp;
    definition.animations = [...definition.animations];
    definition = { ...definition };
    markDirty();
  }

  // ── Playback ───────────────────────────────────────────────
  function togglePlayback() {
    if (playing) {
      stopPlayback();
    } else {
      startPlayback();
    }
  }

  function startPlayback() {
    const anim = selectedAnim;
    if (!anim || anim.frames.length === 0) return;
    playing = true;
    playbackFrame = 0;
    const interval = Math.max(16, Math.round(1000 / anim.fps));
    playbackTimer = setInterval(() => {
      const a = selectedAnim;
      if (!a) {
        stopPlayback();
        return;
      }
      playbackFrame++;
      if (playbackFrame >= a.frames.length) {
        if (a.looping) {
          playbackFrame = 0;
        } else {
          stopPlayback();
        }
      }
    }, interval);
  }

  function stopPlayback() {
    playing = false;
    if (playbackTimer) {
      clearInterval(playbackTimer);
      playbackTimer = null;
    }
  }

  // ── Canvas drawing ─────────────────────────────────────────
  function drawCanvas() {
    if (!canvasRef || !previewImage) return;
    const ctx = canvasRef.getContext("2d");
    if (!ctx) return;

    const cw = canvasRef.width;
    const ch = canvasRef.height;

    // Clear with checkerboard
    ctx.clearRect(0, 0, cw, ch);
    const checkSize = 16;
    for (let y = 0; y < ch; y += checkSize) {
      for (let x = 0; x < cw; x += checkSize) {
        const dark =
          (Math.floor(x / checkSize) + Math.floor(y / checkSize)) % 2 === 0;
        ctx.fillStyle = dark ? $themeColors.base : $themeColors.hover;
        ctx.fillRect(x, y, checkSize, checkSize);
      }
    }

    // Scale image to fit
    const scale = Math.min(cw / previewWidth, ch / previewHeight, 2);
    const dw = previewWidth * scale;
    const dh = previewHeight * scale;
    const dx = (cw - dw) / 2;
    const dy = (ch - dh) / 2;

    ctx.drawImage(previewImage, dx, dy, dw, dh);

    // Draw points
    const points = getCurrentPoints();
    for (const pt of points) {
      const px = dx + pt.x * scale;
      const py = dy + pt.y * scale;

      ctx.beginPath();
      ctx.arc(px, py, 5, 0, Math.PI * 2);
      ctx.fillStyle =
        pt.name === "Origin"
          ? $themeColors.err
          : pt.name === "Center"
            ? $themeColors.ok
            : $themeColors.accent;
      ctx.fill();
      ctx.strokeStyle = $themeColors.base;
      ctx.lineWidth = 1.5;
      ctx.stroke();

      ctx.fillStyle = $themeColors.text;
      ctx.font = "11px sans-serif";
      ctx.fillText(pt.name, px + 8, py + 4);
    }
  }

  function getCurrentPoints(): SpritePoint[] {
    const shared = definition.shared_points;
    if (selectedImage) {
      return mergePoints(shared, [], selectedImage.points);
    }
    if (selectedAnim) {
      const animPts = selectedAnim.points;
      if (selection?.kind === "frame") {
        const framePts = selectedAnim.frame_points[selection.frameIndex] ?? [];
        return mergePoints(shared, animPts, framePts);
      }
      return mergePoints(shared, [], animPts);
    }
    return mergePoints(shared, [], []);
  }

  function mergePoints(
    shared: SpritePoint[],
    anim: SpritePoint[],
    local: SpritePoint[],
  ): SpritePoint[] {
    const map = new Map<string, SpritePoint>();
    map.set("Origin", { name: "Origin", x: 0, y: 0 });
    map.set("Center", {
      name: "Center",
      x: Math.round(previewWidth / 2),
      y: Math.round(previewHeight / 2),
    });
    for (const p of shared) map.set(p.name, { ...p });
    for (const p of anim) map.set(p.name, { ...p });
    for (const p of local) map.set(p.name, { ...p });
    return [...map.values()];
  }

  // ── Image picker ───────────────────────────────────────────
  let pickerSearch = $state("");
  let filteredImages = $derived(
    availableImages.filter((p) =>
      p.toLowerCase().includes(pickerSearch.toLowerCase()),
    ),
  );

  function openImagePicker(target: "images" | "frames") {
    imagePickerTarget = target;
    pickerSearch = "";
    showImagePicker = true;
  }

  function confirmImagePicker(selected: string[]) {
    if (imagePickerTarget === "images") {
      addImages(selected);
    } else {
      addFramesToAnimation(selected);
    }
    showImagePicker = false;
  }

  // ── Canvas resize observer ─────────────────────────────────
  let canvasContainer = $state<HTMLDivElement | null>(null);
  $effect(() => {
    if (!canvasContainer || !canvasRef) return;
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        if (canvasRef) {
          canvasRef.width = width;
          canvasRef.height = height;
          drawCanvas();
        }
      }
    });
    ro.observe(canvasContainer);
    return () => ro.disconnect();
  });

  function getFilename(path: string): string {
    return path.split("/").pop() ?? path;
  }
</script>

<!-- ── Fullscreen modal overlay ──────────────────────────── -->
<div class="atlas-overlay">
  <div class="atlas-editor">
    <!-- Header bar -->
    <div class="header">
      <span class="title">🎴 Atlas Editor — {definition.name || atlasPath}</span
      >
      {#if dirty}
        <span class="dirty-badge">●</span>
      {/if}
      {#if error}
        <span class="error-msg">{error}</span>
      {/if}
      <div class="header-actions">
        <button class="btn" onclick={handleSave} disabled={saving || !dirty}>
          {saving ? "Saving…" : "Save"}
        </button>
        <button class="btn btn-secondary" onclick={onclose}>Close</button>
      </div>
    </div>

    <div class="body">
      <!-- ── Left panel: tree ──────────────────────────────── -->
      <div class="left-panel">
        <!-- Images section -->
        <div class="tree-section">
          <div class="tree-header">
            <span>Images</span>
            <button
              class="icon-btn"
              title="Add images"
              onclick={() => openImagePicker("images")}>+</button
            >
          </div>
          {#each definition.images as img, i}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="tree-item"
              class:selected={selection?.kind === "image" &&
                selection.index === i}
              onclick={() => (selection = { kind: "image", index: i })}
              onkeydown={(e) => {
                if (e.key === "Enter" || e.key === " ")
                  selection = { kind: "image", index: i };
              }}
              role="button"
              tabindex="0"
            >
              <span class="tree-icon">🖼</span>
              <span class="tree-label">{img.name}</span>
              <button
                class="tree-remove"
                title="Remove"
                onclick={(e) => {
                  e.stopPropagation();
                  removeImage(i);
                }}>✕</button
              >
            </div>
          {/each}
          {#if definition.images.length === 0}
            <div class="tree-empty">No images added</div>
          {/if}
        </div>

        <!-- Animations section -->
        <div class="tree-section">
          <div class="tree-header">
            <span>Animations</span>
            <button
              type="button"
              class="icon-btn"
              title="New animation"
              onclick={() => addAnimation()}
            >
              +
            </button>
          </div>
          {#each definition.animations as anim, ai}
            <div class="tree-group">
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="tree-item"
                class:selected={selection?.kind === "animation" &&
                  selection.index === ai}
                onclick={() => (selection = { kind: "animation", index: ai })}
                onkeydown={(e) => {
                  if (e.key === "Enter" || e.key === " ")
                    selection = { kind: "animation", index: ai };
                }}
                role="button"
                tabindex="0"
              >
                <span class="tree-icon">🎬</span>
                <span class="tree-label">{anim.name}</span>
                <button
                  class="tree-remove"
                  title="Remove"
                  onclick={(e) => {
                    e.stopPropagation();
                    removeAnimation(ai);
                  }}>✕</button
                >
              </div>
              {#each anim.frames as frame, fi}
                <button
                  class="tree-item frame-item"
                  class:selected={selection?.kind === "frame" &&
                    selection.animIndex === ai &&
                    selection.frameIndex === fi}
                  onclick={() =>
                    (selection = {
                      kind: "frame",
                      animIndex: ai,
                      frameIndex: fi,
                    })}
                >
                  <span class="tree-label indent">{getFilename(frame)}</span>
                </button>
              {/each}
            </div>
          {/each}
          {#if definition.animations.length === 0}
            <div class="tree-empty">No animations</div>
          {/if}
        </div>

        <!-- Shared points section -->
        <div class="tree-section">
          <div class="tree-header">
            <span>Shared Points</span>
          </div>
          <div class="shared-points-wrap">
            <AtlasPointsEditor
              points={definition.shared_points}
              imageWidth={previewWidth}
              imageHeight={previewHeight}
              onchange={(pts) => {
                definition.shared_points = pts;
                definition = { ...definition };
                markDirty();
              }}
            />
          </div>
        </div>
      </div>

      <!-- ── Center panel: canvas preview ─────────────────── -->
      <div class="center-panel">
        <div class="canvas-wrap" bind:this={canvasContainer}>
          <canvas bind:this={canvasRef}></canvas>
          {#if !currentPreviewPath}
            <div class="canvas-placeholder">
              Select an image or animation frame to preview
            </div>
          {/if}
        </div>

        <!-- Animation timeline (only when animation selected) -->
        {#if selectedAnim}
          {@const ai =
            selection?.kind === "animation"
              ? selection.index
              : selection?.kind === "frame"
                ? selection.animIndex
                : -1}
          {#if ai >= 0}
            <AtlasAnimationTimeline
              frames={selectedAnim.frames}
              selectedFrame={selectedFrameIndex >= 0 ? selectedFrameIndex : 0}
              fps={selectedAnim.fps}
              looping={selectedAnim.looping}
              {playing}
              projectPath={get(project)?.path ?? ""}
              onselect={(fi) =>
                (selection = { kind: "frame", animIndex: ai, frameIndex: fi })}
              onreorder={(from, to) => reorderFrame(ai, from, to)}
              onaddframes={() => openImagePicker("frames")}
              onremoveframe={(fi) => removeFrame(ai, fi)}
              onfpschange={(fps) => {
                definition.animations[ai].fps = fps;
                definition.animations = [...definition.animations];
                markDirty();
              }}
              onloopingchange={(loop) => {
                definition.animations[ai].looping = loop;
                definition.animations = [...definition.animations];
                markDirty();
              }}
              onplaytoggle={togglePlayback}
            />
          {/if}
        {/if}
      </div>

      <!-- ── Right panel: properties ──────────────────────── -->
      <div class="right-panel">
        {#if selectedImage}
          <div class="props-section">
            <h3>Image Properties</h3>
            <label class="prop-row">
              <span>Name</span>
              <input
                type="text"
                value={selectedImage.name}
                onchange={(e) => {
                  const idx =
                    selection?.kind === "image" ? selection.index : -1;
                  if (idx >= 0) {
                    definition.images[idx].name = (
                      e.target as HTMLInputElement
                    ).value;
                    definition.images = [...definition.images];
                    markDirty();
                  }
                }}
              />
            </label>
            <label class="prop-row">
              <span>Path</span>
              <span class="prop-value">{selectedImage.path}</span>
            </label>
            <label class="prop-row">
              <span>Anchor X</span>
              <input
                type="number"
                step="0.1"
                min="0"
                max="1"
                value={selectedImage.anchor[0]}
                onchange={(e) => {
                  const idx =
                    selection?.kind === "image" ? selection.index : -1;
                  if (idx >= 0) {
                    definition.images[idx].anchor[0] = parseFloat(
                      (e.target as HTMLInputElement).value,
                    );
                    definition.images = [...definition.images];
                    markDirty();
                  }
                }}
              />
            </label>
            <label class="prop-row">
              <span>Anchor Y</span>
              <input
                type="number"
                step="0.1"
                min="0"
                max="1"
                value={selectedImage.anchor[1]}
                onchange={(e) => {
                  const idx =
                    selection?.kind === "image" ? selection.index : -1;
                  if (idx >= 0) {
                    definition.images[idx].anchor[1] = parseFloat(
                      (e.target as HTMLInputElement).value,
                    );
                    definition.images = [...definition.images];
                    markDirty();
                  }
                }}
              />
            </label>
          </div>
          <div class="props-section">
            <h3>Points</h3>
            <AtlasPointsEditor
              points={selectedImage.points}
              sharedPoints={definition.shared_points}
              imageWidth={previewWidth}
              imageHeight={previewHeight}
              onchange={(pts) => {
                const idx = selection?.kind === "image" ? selection.index : -1;
                if (idx >= 0) {
                  definition.images[idx].points = pts;
                  definition.images = [...definition.images];
                  markDirty();
                }
              }}
            />
          </div>
        {:else if selectedAnim}
          {@const ai =
            selection?.kind === "animation"
              ? selection.index
              : selection?.kind === "frame"
                ? selection.animIndex
                : -1}
          <div class="props-section">
            <h3>Animation Properties</h3>
            <label class="prop-row">
              <span>Name</span>
              <input
                type="text"
                value={selectedAnim.name}
                onchange={(e) => {
                  if (ai >= 0) {
                    definition.animations[ai].name = (
                      e.target as HTMLInputElement
                    ).value;
                    definition.animations = [...definition.animations];
                    markDirty();
                  }
                }}
              />
            </label>
            <label class="prop-row">
              <span>Frames</span>
              <span class="prop-value">{selectedAnim.frames.length}</span>
            </label>
          </div>

          <!-- Animation-level points -->
          <div class="props-section">
            <h3>Animation Points</h3>
            <AtlasPointsEditor
              points={selectedAnim.points}
              sharedPoints={definition.shared_points}
              imageWidth={previewWidth}
              imageHeight={previewHeight}
              onchange={(pts) => {
                if (ai >= 0) {
                  definition.animations[ai].points = pts;
                  definition.animations = [...definition.animations];
                  markDirty();
                }
              }}
            />
          </div>

          <!-- Frame-level points (when a specific frame is selected) -->
          {#if selection?.kind === "frame"}
            <div class="props-section">
              <h3>Frame {selection.frameIndex + 1} Points</h3>
              <AtlasPointsEditor
                points={selectedAnim.frame_points[selection.frameIndex] ?? []}
                sharedPoints={definition.shared_points}
                animationPoints={selectedAnim.points}
                imageWidth={previewWidth}
                imageHeight={previewHeight}
                onchange={(pts) => {
                  if (ai >= 0 && selection?.kind === "frame") {
                    const fi = selection.frameIndex;
                    if (pts.length > 0) {
                      definition.animations[ai].frame_points[fi] = pts;
                    } else {
                      delete definition.animations[ai].frame_points[fi];
                    }
                    definition.animations = [...definition.animations];
                    markDirty();
                  }
                }}
              />
            </div>
          {/if}
        {:else}
          <div class="props-empty">
            Select an image or animation to edit properties
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>

<!-- ── Image picker modal (outside atlas-overlay to avoid stacking context issues) ── -->
{#if showImagePicker}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="picker-overlay"
    onclick={(e) => {
      if (e.target === e.currentTarget) showImagePicker = false;
    }}
    onkeydown={() => {}}
  >
    <div class="picker-dialog">
      <div class="picker-header">
        <span>Select Images</span>
        <button class="icon-btn" onclick={() => (showImagePicker = false)}
          >✕</button
        >
      </div>
      <input
        class="picker-search"
        type="text"
        placeholder="Search images…"
        bind:value={pickerSearch}
      />
      <div class="picker-list">
        {#each filteredImages as imgPath}
          <button
            class="picker-item"
            onclick={() => confirmImagePicker([imgPath])}
          >
            {getFilename(imgPath)}
            <span class="picker-path">{imgPath}</span>
          </button>
        {/each}
        {#if filteredImages.length === 0}
          <div class="picker-empty">No images found</div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .atlas-overlay {
    position: fixed;
    inset: 0;
    z-index: 1000;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: stretch;
    justify-content: stretch;
  }

  .atlas-editor {
    flex: 1;
    margin: 8px;
    background: var(--ed-base);
    border: 1px solid var(--ed-line-strong);
    border-radius: 8px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  /* ── Header ─────────────────────────────────────────────── */
  .header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--ed-mantle);
    border-bottom: 1px solid var(--ed-line);
    flex-shrink: 0;
  }

  .title {
    font-weight: 600;
    color: var(--ed-text);
  }

  .dirty-badge {
    color: var(--ed-a3);
    font-size: 16px;
  }

  .error-msg {
    color: var(--ed-err);
    font-size: 12px;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .header-actions {
    margin-left: auto;
    display: flex;
    gap: 6px;
  }

  .btn {
    padding: 4px 12px;
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    background: var(--ed-accent);
    color: var(--ed-on-accent);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .btn-secondary {
    background: var(--ed-raised);
    color: var(--ed-text);
  }

  .btn-secondary:hover {
    background: var(--ed-line);
  }

  /* ── Body layout ────────────────────────────────────────── */
  .body {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  /* ── Left panel ─────────────────────────────────────────── */
  .left-panel {
    width: 240px;
    flex-shrink: 0;
    background: var(--ed-mantle);
    border-right: 1px solid var(--ed-line);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }

  .tree-section {
    border-bottom: 1px solid var(--ed-line);
  }

  .tree-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 8px;
    font-size: 11px;
    font-weight: 600;
    color: var(--ed-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .icon-btn {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    background: none;
    color: var(--ed-muted);
    font-size: 14px;
    cursor: pointer;
    border-radius: 4px;
  }

  .icon-btn:hover {
    background: var(--ed-raised);
    color: var(--ed-text);
  }

  .tree-item {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 4px 8px;
    border: none;
    background: none;
    color: var(--ed-text);
    font-size: 12px;
    cursor: pointer;
    text-align: left;
  }

  .tree-item:hover {
    background: var(--ed-raised);
  }

  .tree-item.selected {
    background: var(--ed-line);
  }

  .tree-icon {
    font-size: 12px;
    flex-shrink: 0;
  }

  .tree-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tree-label.indent {
    padding-left: 20px;
  }

  .tree-remove {
    opacity: 0;
    border: none;
    background: none;
    color: var(--ed-err);
    font-size: 11px;
    cursor: pointer;
    padding: 0 2px;
  }

  .tree-item:hover .tree-remove {
    opacity: 0.7;
  }

  .tree-remove:hover {
    opacity: 1 !important;
  }

  .tree-empty {
    padding: 8px 12px;
    color: var(--ed-faint);
    font-size: 11px;
    font-style: italic;
  }

  .tree-group {
    /* nothing special, just grouping */
  }

  .shared-points-wrap {
    padding: 4px;
  }

  .frame-item {
    padding-left: 16px;
    font-size: 11px;
    color: var(--ed-muted);
  }

  /* ── Center panel ───────────────────────────────────────── */
  .center-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .canvas-wrap {
    flex: 1;
    position: relative;
    overflow: hidden;
  }

  .canvas-wrap canvas {
    display: block;
    width: 100%;
    height: 100%;
  }

  .canvas-placeholder {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--ed-faint);
    font-size: 14px;
    pointer-events: none;
  }

  /* ── Right panel ────────────────────────────────────────── */
  .right-panel {
    width: 280px;
    flex-shrink: 0;
    background: var(--ed-mantle);
    border-left: 1px solid var(--ed-line);
    overflow-y: auto;
    padding: 8px;
  }

  .props-section {
    margin-bottom: 12px;
  }

  .props-section h3 {
    font-size: 11px;
    font-weight: 600;
    color: var(--ed-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 6px;
    padding-bottom: 4px;
    border-bottom: 1px solid var(--ed-line);
  }

  .prop-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 3px 0;
    font-size: 12px;
    color: var(--ed-text);
  }

  .prop-row span:first-child {
    color: var(--ed-muted);
    flex-shrink: 0;
  }

  .prop-row input {
    width: 100px;
    padding: 2px 6px;
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    background: var(--ed-base);
    color: var(--ed-text);
    font-size: 12px;
  }

  .prop-value {
    color: var(--ed-faint);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 160px;
    text-align: right;
  }

  .props-empty {
    padding: 20px;
    text-align: center;
    color: var(--ed-faint);
    font-size: 12px;
  }

  /* ── Image picker modal ─────────────────────────────────── */
  .picker-overlay {
    position: fixed;
    inset: 0;
    z-index: 2000;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .picker-dialog {
    width: 420px;
    max-height: 500px;
    background: var(--ed-base);
    border: 1px solid var(--ed-line-strong);
    border-radius: 8px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .picker-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--ed-line);
    font-weight: 600;
    color: var(--ed-text);
  }

  .picker-search {
    margin: 8px;
    padding: 6px 8px;
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    background: var(--ed-mantle);
    color: var(--ed-text);
    font-size: 12px;
  }

  .picker-list {
    flex: 1;
    overflow-y: auto;
    padding: 4px;
  }

  .picker-item {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: 6px 8px;
    border: none;
    background: none;
    color: var(--ed-text);
    font-size: 12px;
    cursor: pointer;
    text-align: left;
    border-radius: 4px;
  }

  .picker-item:hover {
    background: var(--ed-accent-low);
    color: var(--ed-ink);
  }

  .picker-path {
    font-size: 10px;
    color: var(--ed-faint);
  }

  .picker-empty {
    padding: 16px;
    text-align: center;
    color: var(--ed-faint);
    font-size: 12px;
  }
</style>
