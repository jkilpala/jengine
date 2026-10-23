<script lang="ts">
  import type { SpritePoint } from "./api/atlas";

  let {
    points = [],
    sharedPoints = [],
    animationPoints = [],
    imageWidth = 0,
    imageHeight = 0,
    shareAllAnimations = false,
    shareThisAnimation = false,
    showShareToggles = false,
    onchange,
    onshareAllChange,
    onshareAnimChange,
  }: {
    points: SpritePoint[];
    sharedPoints?: SpritePoint[];
    animationPoints?: SpritePoint[];
    imageWidth?: number;
    imageHeight?: number;
    shareAllAnimations?: boolean;
    shareThisAnimation?: boolean;
    showShareToggles?: boolean;
    onchange: (points: SpritePoint[]) => void;
    onshareAllChange?: (v: boolean) => void;
    onshareAnimChange?: (v: boolean) => void;
  } = $props();

  let editingName = $state<number | null>(null);
  let editNameValue = $state("");

  /** Origin is always at index 0 (auto-inserted if missing). */
  let resolvedPoints = $derived.by(() => {
    const map = new Map<string, SpritePoint>();
    // Auto defaults
    map.set("Origin", { name: "Origin", x: 0, y: 0 });
    map.set("Center", {
      name: "Center",
      x: Math.round(imageWidth / 2),
      y: Math.round(imageHeight / 2),
    });
    // Shared base
    for (const p of sharedPoints) map.set(p.name, { ...p });
    // Animation-level
    for (const p of animationPoints) map.set(p.name, { ...p });
    // Current layer
    for (const p of points) map.set(p.name, { ...p });

    // Sort: Origin, Center, then custom alphabetical
    const arr = [...map.values()];
    arr.sort((a, b) => {
      const rank = (n: string) => (n === "Origin" ? 0 : n === "Center" ? 1 : 2);
      return rank(a.name) - rank(b.name) || a.name.localeCompare(b.name);
    });
    return arr;
  });

  function isDefault(name: string): boolean {
    return name === "Origin" || name === "Center";
  }

  /** Whether this point exists in the current layer's points array. */
  function isInCurrentLayer(name: string): boolean {
    return points.some((p) => p.name === name);
  }

  /** What the auto value for Center would be. */
  function isAutoCenter(p: SpritePoint): boolean {
    return (
      p.name === "Center" &&
      !isInCurrentLayer("Center") &&
      !animationPoints.some((ap) => ap.name === "Center") &&
      !sharedPoints.some((sp) => sp.name === "Center")
    );
  }

  function updatePoint(index: number, field: "x" | "y", value: string) {
    const p = resolvedPoints[index];
    if (!p) return;
    const num = parseFloat(value);
    if (isNaN(num)) return;

    // Ensure point exists in current layer
    const existing = points.findIndex((pt) => pt.name === p.name);
    const updated = [...points];
    if (existing >= 0) {
      updated[existing] = { ...updated[existing], [field]: num };
    } else {
      updated.push({ name: p.name, x: p.x, y: p.y, [field]: num });
    }
    onchange(updated);
  }

  function removePoint(name: string) {
    onchange(points.filter((p) => p.name !== name));
  }

  function addPoint() {
    let name = "Point";
    let n = 1;
    const existingNames = new Set(resolvedPoints.map((p) => p.name));
    while (existingNames.has(name)) {
      name = `Point${n++}`;
    }
    onchange([...points, { name, x: 0, y: 0 }]);
  }

  function startEditName(index: number) {
    const p = resolvedPoints[index];
    if (isDefault(p.name)) return;
    editingName = index;
    editNameValue = p.name;
  }

  function commitEditName() {
    if (editingName === null) return;
    const p = resolvedPoints[editingName];
    const newName = editNameValue.trim();
    if (!newName || newName === p.name) {
      editingName = null;
      return;
    }
    // Check for duplicates
    if (resolvedPoints.some((pt) => pt.name === newName)) {
      editingName = null;
      return;
    }
    const updated = points.map((pt) =>
      pt.name === p.name ? { ...pt, name: newName } : pt,
    );
    onchange(updated);
    editingName = null;
  }
</script>

<div class="points-editor">
  {#if showShareToggles}
    <label class="share-toggle">
      <input
        type="checkbox"
        checked={shareAllAnimations}
        onchange={(e) =>
          onshareAllChange?.((e.currentTarget as HTMLInputElement).checked)}
      />
      Share same points for all animations
    </label>
    <label class="share-toggle">
      <input
        type="checkbox"
        checked={shareThisAnimation}
        onchange={(e) =>
          onshareAnimChange?.((e.currentTarget as HTMLInputElement).checked)}
      />
      Share same points for all sprites of this animation
    </label>
  {/if}

  <div class="points-table">
    <div class="table-header">
      <span class="col-name">Point name</span>
      <span class="col-val">X</span>
      <span class="col-val">Y</span>
      <span class="col-action"></span>
    </div>

    {#each resolvedPoints as point, i}
      <div class="table-row" class:auto={isAutoCenter(point)}>
        <span class="col-name">
          {#if editingName === i}
            <input
              class="name-input"
              bind:value={editNameValue}
              onblur={commitEditName}
              onkeydown={(e) => {
                if (e.key === "Enter") commitEditName();
                if (e.key === "Escape") editingName = null;
              }}
            />
          {:else}
            <span
              class="point-name"
              class:editable={!isDefault(point.name)}
              ondblclick={() => startEditName(i)}
              role="button"
              tabindex={isDefault(point.name) ? -1 : 0}
            >
              {point.name}
            </span>
          {/if}
        </span>
        <span class="col-val">
          {#if isAutoCenter(point)}
            <span class="auto-label">(auto)</span>
          {:else}
            <input
              type="number"
              class="coord-input"
              value={Math.round(point.x)}
              onchange={(e) =>
                updatePoint(
                  i,
                  "x",
                  (e.currentTarget as HTMLInputElement).value,
                )}
            />
          {/if}
        </span>
        <span class="col-val">
          {#if isAutoCenter(point)}
            <span class="auto-label">(auto)</span>
          {:else}
            <input
              type="number"
              class="coord-input"
              value={Math.round(point.y)}
              onchange={(e) =>
                updatePoint(
                  i,
                  "y",
                  (e.currentTarget as HTMLInputElement).value,
                )}
            />
          {/if}
        </span>
        <span class="col-action">
          {#if isAutoCenter(point) && !isInCurrentLayer(point.name)}
            <button
              class="edit-btn"
              title="Override auto Center"
              onclick={() =>
                onchange([
                  ...points,
                  {
                    name: "Center",
                    x: Math.round(imageWidth / 2),
                    y: Math.round(imageHeight / 2),
                  },
                ])}
            >
              ✏️
            </button>
          {:else if !isDefault(point.name) && isInCurrentLayer(point.name)}
            <button
              class="delete-btn"
              title="Remove point"
              onclick={() => removePoint(point.name)}
            >
              🗑️
            </button>
          {/if}
        </span>
      </div>
    {/each}
  </div>

  <button class="add-btn" onclick={addPoint}>+ Add a point</button>
</div>

<style>
  .points-editor {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .share-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--ed-text);
    cursor: pointer;
  }

  .share-toggle input[type="checkbox"] {
    accent-color: var(--ed-accent);
  }

  .points-table {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    overflow: hidden;
  }

  .table-header {
    display: flex;
    padding: 4px 8px;
    font-size: 11px;
    text-transform: uppercase;
    color: var(--ed-muted);
    background: rgba(0, 0, 0, 0.2);
    border-bottom: 1px solid var(--ed-line);
  }

  .table-row {
    display: flex;
    padding: 4px 8px;
    align-items: center;
    border-bottom: 1px solid var(--ed-line);
  }

  .table-row:last-child {
    border-bottom: none;
  }

  .table-row.auto {
    opacity: 0.7;
  }

  .col-name {
    flex: 1;
    min-width: 80px;
  }

  .col-val {
    width: 60px;
    text-align: center;
  }

  .col-action {
    width: 30px;
    text-align: center;
  }

  .point-name {
    font-size: 12px;
    color: var(--ed-text);
  }

  .point-name.editable {
    cursor: pointer;
  }

  .point-name.editable:hover {
    color: var(--ed-accent);
  }

  .name-input {
    width: 100%;
    background: var(--ed-mantle);
    border: 1px solid var(--ed-accent);
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 12px;
    padding: 2px 4px;
  }

  .coord-input {
    width: 50px;
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 12px;
    padding: 2px 4px;
    text-align: center;
  }

  .coord-input:focus {
    border-color: var(--ed-accent);
    outline: none;
  }

  .auto-label {
    font-size: 11px;
    color: var(--ed-muted);
    font-style: italic;
  }

  .edit-btn,
  .delete-btn {
    background: none;
    border: none;
    cursor: pointer;
    padding: 2px;
    font-size: 12px;
    opacity: 0.6;
  }

  .edit-btn:hover,
  .delete-btn:hover {
    opacity: 1;
  }

  .add-btn {
    background: none;
    border: 1px dashed var(--ed-line);
    border-radius: 4px;
    color: var(--ed-accent);
    font-size: 12px;
    padding: 6px 12px;
    cursor: pointer;
    text-align: center;
  }

  .add-btn:hover {
    border-color: var(--ed-accent);
    background: color-mix(in srgb, var(--ed-accent) 5%, transparent);
  }
</style>
