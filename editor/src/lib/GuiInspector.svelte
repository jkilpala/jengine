<script lang="ts">
  import {
    guiDocument,
    selectedNode,
    executeGui,
    updateNodeStyle,
    updateNodeType,
    renameNode,
  } from "./stores/guiEditorStore";
  import {
    projectFontIds,
    isBuiltinFontId,
  } from "./stores/projectFontsStore";
  import {
    getNodeTypeName,
    constraintsIsEmpty,
    type GuiNode,
    type GuiAnchor,
    type SizeValue,
    type GridTrack,
    type Constraints,
    type ConstraintValue,
  } from "./api/gui";

  const ANCHOR_OPTIONS: GuiAnchor[] = [
    "TopLeft",
    "TopCenter",
    "TopRight",
    "CenterLeft",
    "Center",
    "CenterRight",
    "BottomLeft",
    "BottomCenter",
    "BottomRight",
  ];

  // ---------------------------------------------------------------------------
  // Helpers
  // ---------------------------------------------------------------------------
  function sizeLabel(sv: SizeValue): string {
    if (sv.type === "Fixed") return `${sv.value}px`;
    if (sv.type === "Percent") return `${sv.value}%`;
    return "Auto";
  }

  function parseSizeValue(str: string): SizeValue {
    const trimmed = str.trim().toLowerCase();
    if (trimmed === "auto") return { type: "Auto" };
    if (trimmed.endsWith("%"))
      return { type: "Percent", value: parseFloat(trimmed) || 0 };
    return { type: "Fixed", value: parseFloat(trimmed) || 0 };
  }

  function trackLabel(t: GridTrack): string {
    if (t.type === "Fixed") return `${t.value}px`;
    if (t.type === "Fraction") return `${t.value}fr`;
    return "auto";
  }

  function parseTrack(str: string): GridTrack {
    const trimmed = str.trim().toLowerCase();
    if (trimmed === "auto") return { type: "Auto" };
    if (trimmed.endsWith("fr"))
      return { type: "Fraction", value: parseFloat(trimmed) || 1 };
    return { type: "Fixed", value: parseFloat(trimmed) || 0 };
  }

  function tracksLabel(tracks: GridTrack[]): string {
    return tracks.map(trackLabel).join(" ");
  }

  function parseTracks(str: string): GridTrack[] {
    return str.trim().split(/\s+/).filter(Boolean).map(parseTrack);
  }

  function applyAnchorPreset(nodeId: string, preset: string) {
    const px = (v: number): ConstraintValue => ({ type: "Px", value: v });
    let c: Constraints = {};

    switch (preset) {
      case "top-left":
        c = { left: px(0), top: px(0) };
        break;
      case "top-center":
        c = { left: { type: "Percent", value: 50 }, top: px(0) };
        break;
      case "top-right":
        c = { right: px(0), top: px(0) };
        break;
      case "center-left":
        c = { left: px(0), top: { type: "Percent", value: 50 } };
        break;
      case "center":
        c = {
          left: { type: "Percent", value: 50 },
          top: { type: "Percent", value: 50 },
        };
        break;
      case "center-right":
        c = { right: px(0), top: { type: "Percent", value: 50 } };
        break;
      case "bottom-left":
        c = { left: px(0), bottom: px(0) };
        break;
      case "bottom-center":
        c = { left: { type: "Percent", value: 50 }, bottom: px(0) };
        break;
      case "bottom-right":
        c = { right: px(0), bottom: px(0) };
        break;
      case "stretch-h":
        c = { left: px(0), right: px(0), top: px(0) };
        break;
      case "stretch-v":
        c = { left: px(0), top: px(0), bottom: px(0) };
        break;
      case "fill":
        c = { left: px(0), right: px(0), top: px(0), bottom: px(0) };
        break;
    }

    updateNodeStyle(nodeId, "constraints", c);
  }

  function getLayoutMode(node: GuiNode): "Flex" | "Grid" {
    return node.style.layout?.mode === "Grid" ? "Grid" : "Flex";
  }
</script>

<!-- Document Anchor -->
{#if $guiDocument}
  <div class="prop-section">
    <div class="prop-section-title">Document</div>
    <div class="prop-row">
      <label>Anchor</label>
      <select
        value={$guiDocument.anchor ?? "TopLeft"}
        onchange={(e) => {
          const val = (e.target as HTMLSelectElement).value as GuiAnchor;
          executeGui("Change anchor", (doc) => {
            doc.anchor = val;
          });
        }}
      >
        {#each ANCHOR_OPTIONS as a}
          <option value={a}>{a.replace(/([A-Z])/g, " $1").trim()}</option>
        {/each}
      </select>
    </div>
  </div>
{/if}

{#if $selectedNode}
  {@const node = $selectedNode}
  {@const typeName = getNodeTypeName(node.node_type)}
  {@const data = node.node_type as Record<string, unknown>}

  <!-- Node ID -->
  <div class="prop-section">
    <div class="prop-section-title">Node</div>
    <div class="prop-row">
      <label>ID</label>
      {#if node.id === "root"}
        <span class="prop-value">{node.id}</span>
      {:else}
        <input
          type="text"
          value={node.id}
          onchange={(e) => {
            const newId = (e.target as HTMLInputElement).value.trim();
            if (newId && newId !== node.id) renameNode(node.id, newId);
          }}
        />
      {/if}
    </div>
    <div class="prop-row">
      <label>Type</label><span class="prop-value">{typeName}</span>
    </div>
  </div>

  <!-- Size -->
  <div class="prop-section">
    <div class="prop-section-title">Size</div>
    <div class="prop-row">
      <label>Width</label>
      {#if node.style.width.type === "Auto"}
        <span class="prop-value">Auto</span>
      {:else}
        <input
          type="number"
          step="1"
          class="constraint-value"
          value={node.style.width.value}
          onchange={(e) =>
            updateNodeStyle(node.id, "width", {
              type: node.style.width.type,
              value: parseFloat((e.target as HTMLInputElement).value) || 0,
            } as SizeValue)}
        />
      {/if}
      <select
        class="constraint-unit"
        value={node.style.width.type}
        onchange={(e) => {
          const t = (e.target as HTMLSelectElement).value as
            | "Fixed"
            | "Percent"
            | "Auto";
          if (t === "Auto") updateNodeStyle(node.id, "width", { type: "Auto" });
          else
            updateNodeStyle(node.id, "width", {
              type: t,
              value:
                node.style.width.type === "Auto"
                  ? 0
                  : ((node.style.width as any).value ?? 0),
            });
        }}
      >
        <option value="Fixed">px</option>
        <option value="Percent">%</option>
        <option value="Auto">auto</option>
      </select>
    </div>
    <div class="prop-row">
      <label>Height</label>
      {#if node.style.height.type === "Auto"}
        <span class="prop-value">Auto</span>
      {:else}
        <input
          type="number"
          step="1"
          class="constraint-value"
          value={node.style.height.value}
          onchange={(e) =>
            updateNodeStyle(node.id, "height", {
              type: node.style.height.type,
              value: parseFloat((e.target as HTMLInputElement).value) || 0,
            } as SizeValue)}
        />
      {/if}
      <select
        class="constraint-unit"
        value={node.style.height.type}
        onchange={(e) => {
          const t = (e.target as HTMLSelectElement).value as
            | "Fixed"
            | "Percent"
            | "Auto";
          if (t === "Auto")
            updateNodeStyle(node.id, "height", { type: "Auto" });
          else
            updateNodeStyle(node.id, "height", {
              type: t,
              value:
                node.style.height.type === "Auto"
                  ? 0
                  : ((node.style.height as any).value ?? 0),
            });
        }}
      >
        <option value="Fixed">px</option>
        <option value="Percent">%</option>
        <option value="Auto">auto</option>
      </select>
    </div>
  </div>

  <!-- Padding -->
  <div class="prop-section">
    <div class="prop-section-title">Padding</div>
    <div class="prop-row-4">
      <input
        type="number"
        step="1"
        value={node.style.padding.top}
        title="Top"
        onchange={(e) =>
          updateNodeStyle(node.id, "padding", {
            ...node.style.padding,
            top: parseFloat((e.target as HTMLInputElement).value) || 0,
          })}
      />
      <input
        type="number"
        step="1"
        value={node.style.padding.right}
        title="Right"
        onchange={(e) =>
          updateNodeStyle(node.id, "padding", {
            ...node.style.padding,
            right: parseFloat((e.target as HTMLInputElement).value) || 0,
          })}
      />
      <input
        type="number"
        step="1"
        value={node.style.padding.bottom}
        title="Bottom"
        onchange={(e) =>
          updateNodeStyle(node.id, "padding", {
            ...node.style.padding,
            bottom: parseFloat((e.target as HTMLInputElement).value) || 0,
          })}
      />
      <input
        type="number"
        step="1"
        value={node.style.padding.left}
        title="Left"
        onchange={(e) =>
          updateNodeStyle(node.id, "padding", {
            ...node.style.padding,
            left: parseFloat((e.target as HTMLInputElement).value) || 0,
          })}
      />
    </div>
  </div>

  <!-- Margin -->
  <div class="prop-section">
    <div class="prop-section-title">Margin</div>
    <div class="prop-row-4">
      <input
        type="number"
        step="1"
        value={node.style.margin.top}
        title="Top"
        onchange={(e) =>
          updateNodeStyle(node.id, "margin", {
            ...node.style.margin,
            top: parseFloat((e.target as HTMLInputElement).value) || 0,
          })}
      />
      <input
        type="number"
        step="1"
        value={node.style.margin.right}
        title="Right"
        onchange={(e) =>
          updateNodeStyle(node.id, "margin", {
            ...node.style.margin,
            right: parseFloat((e.target as HTMLInputElement).value) || 0,
          })}
      />
      <input
        type="number"
        step="1"
        value={node.style.margin.bottom}
        title="Bottom"
        onchange={(e) =>
          updateNodeStyle(node.id, "margin", {
            ...node.style.margin,
            bottom: parseFloat((e.target as HTMLInputElement).value) || 0,
          })}
      />
      <input
        type="number"
        step="1"
        value={node.style.margin.left}
        title="Left"
        onchange={(e) =>
          updateNodeStyle(node.id, "margin", {
            ...node.style.margin,
            left: parseFloat((e.target as HTMLInputElement).value) || 0,
          })}
      />
    </div>
  </div>

  <!-- Constraints / Offset -->
  {#if node.id !== "root"}
    {@const hasConstraints = !constraintsIsEmpty(node.style.constraints)}
    <div class="prop-section">
      <div class="prop-section-title">Constraints</div>

      <!-- Anchor preset grid (3×3 + 3 stretch presets) -->
      <div class="anchor-presets">
        <button
          class="anchor-btn"
          title="Top-Left"
          onclick={() => applyAnchorPreset(node.id, "top-left")}>⬉</button
        >
        <button
          class="anchor-btn"
          title="Top-Center"
          onclick={() => applyAnchorPreset(node.id, "top-center")}>⬆</button
        >
        <button
          class="anchor-btn"
          title="Top-Right"
          onclick={() => applyAnchorPreset(node.id, "top-right")}>⬈</button
        >
        <button
          class="anchor-btn"
          title="Center-Left"
          onclick={() => applyAnchorPreset(node.id, "center-left")}>⬅</button
        >
        <button
          class="anchor-btn"
          title="Center"
          onclick={() => applyAnchorPreset(node.id, "center")}>⊙</button
        >
        <button
          class="anchor-btn"
          title="Center-Right"
          onclick={() => applyAnchorPreset(node.id, "center-right")}>➡</button
        >
        <button
          class="anchor-btn"
          title="Bottom-Left"
          onclick={() => applyAnchorPreset(node.id, "bottom-left")}>⬋</button
        >
        <button
          class="anchor-btn"
          title="Bottom-Center"
          onclick={() => applyAnchorPreset(node.id, "bottom-center")}>⬇</button
        >
        <button
          class="anchor-btn"
          title="Bottom-Right"
          onclick={() => applyAnchorPreset(node.id, "bottom-right")}>⬊</button
        >
        <button
          class="anchor-btn stretch-btn"
          title="Stretch Horizontal"
          onclick={() => applyAnchorPreset(node.id, "stretch-h")}>↔</button
        >
        <button
          class="anchor-btn stretch-btn"
          title="Stretch Vertical"
          onclick={() => applyAnchorPreset(node.id, "stretch-v")}>↕</button
        >
        <button
          class="anchor-btn stretch-btn"
          title="Fill"
          onclick={() => applyAnchorPreset(node.id, "fill")}>⛶</button
        >
      </div>

      <!-- Per-edge constraint editors -->
      {#each ["left", "right", "top", "bottom"] as edge}
        {@const cv = ((node.style.constraints ?? {}) as any)[edge] as
          | import("./api/gui").ConstraintValue
          | undefined}
        <div class="prop-row constraint-row">
          <input
            type="checkbox"
            checked={!!cv}
            onchange={(e) => {
              const checked = (e.target as HTMLInputElement).checked;
              const newCons = { ...(node.style.constraints ?? {}) };
              if (checked) {
                (newCons as any)[edge] = { type: "Px", value: 0 };
              } else {
                (newCons as any)[edge] = undefined;
              }
              updateNodeStyle(node.id, "constraints", newCons);
            }}
          />
          <label class="constraint-label"
            >{edge.charAt(0).toUpperCase() + edge.slice(1)}</label
          >
          {#if cv}
            <input
              type="number"
              step="1"
              class="constraint-value"
              value={cv.value}
              onchange={(e) => {
                const val =
                  parseFloat((e.target as HTMLInputElement).value) || 0;
                const newCons = { ...(node.style.constraints ?? {}) };
                (newCons as any)[edge] = { type: cv.type, value: val };
                updateNodeStyle(node.id, "constraints", newCons);
              }}
            />
            <select
              class="constraint-unit"
              value={cv.type}
              onchange={(e) => {
                const type = (e.target as HTMLSelectElement).value as
                  | "Px"
                  | "Percent";
                const newCons = { ...(node.style.constraints ?? {}) };
                (newCons as any)[edge] = { type, value: cv.value };
                updateNodeStyle(node.id, "constraints", newCons);
              }}
            >
              <option value="Px">px</option>
              <option value="Percent">%</option>
            </select>
          {/if}
        </div>
      {/each}

      <!-- Derived size hints -->
      {#if (node.style.constraints ?? {}).left && (node.style.constraints ?? {}).right}
        <div class="constraint-hint">↔ Width derived from left + right</div>
      {/if}
      {#if (node.style.constraints ?? {}).top && (node.style.constraints ?? {}).bottom}
        <div class="constraint-hint">↕ Height derived from top + bottom</div>
      {/if}

      <!-- Clear all constraints button -->
      {#if hasConstraints}
        <button
          class="constraint-clear"
          onclick={() => updateNodeStyle(node.id, "constraints", {})}
        >
          Clear Constraints
        </button>
      {/if}
    </div>

    <!-- Offset (only when no constraints) -->
    {#if !hasConstraints}
      <div class="prop-section">
        <div class="prop-section-title">Offset</div>
        <div class="prop-row">
          <label>X</label>
          <input
            type="number"
            step="1"
            value={node.style.offset_x}
            onchange={(e) =>
              updateNodeStyle(
                node.id,
                "offset_x",
                parseFloat((e.target as HTMLInputElement).value) || 0,
              )}
          />
        </div>
        <div class="prop-row">
          <label>Y</label>
          <input
            type="number"
            step="1"
            value={node.style.offset_y}
            onchange={(e) =>
              updateNodeStyle(
                node.id,
                "offset_y",
                parseFloat((e.target as HTMLInputElement).value) || 0,
              )}
          />
        </div>
      </div>
    {/if}
  {/if}

  <!-- Layout -->
  <div class="prop-section">
    <div class="prop-section-title">Layout</div>
    <div class="prop-row">
      <label>Mode</label>
      <select
        value={getLayoutMode(node)}
        onchange={(e) => {
          const mode = (e.target as HTMLSelectElement).value;
          if (mode === "Grid") {
            updateNodeStyle(node.id, "layout", {
              mode: "Grid",
              columns: [
                { type: "Fraction", value: 1 },
                { type: "Fraction", value: 1 },
              ],
              rows: [{ type: "Fraction", value: 1 }],
              column_gap: 0,
              row_gap: 0,
            });
          } else {
            updateNodeStyle(node.id, "layout", undefined);
          }
        }}
      >
        <option value="Flex">Flex</option>
        <option value="Grid">Grid</option>
      </select>
    </div>
    {#if getLayoutMode(node) === "Grid"}
      {@const gl = node.style.layout as any}
      <div class="prop-row">
        <label>Columns</label>
        <input
          type="text"
          value={tracksLabel(gl.columns ?? [{ type: "Fraction", value: 1 }])}
          onchange={(e) =>
            updateNodeStyle(node.id, "layout", {
              ...gl,
              columns: parseTracks((e.target as HTMLInputElement).value),
            })}
        />
      </div>
      <div class="prop-row">
        <label>Rows</label>
        <input
          type="text"
          value={tracksLabel(gl.rows ?? [{ type: "Fraction", value: 1 }])}
          onchange={(e) =>
            updateNodeStyle(node.id, "layout", {
              ...gl,
              rows: parseTracks((e.target as HTMLInputElement).value),
            })}
        />
      </div>
      <div class="prop-row">
        <label>Col Gap</label>
        <input
          type="number"
          step="1"
          value={gl.column_gap ?? 0}
          onchange={(e) =>
            updateNodeStyle(node.id, "layout", {
              ...gl,
              column_gap: parseFloat((e.target as HTMLInputElement).value) || 0,
            })}
        />
      </div>
      <div class="prop-row">
        <label>Row Gap</label>
        <input
          type="number"
          step="1"
          value={gl.row_gap ?? 0}
          onchange={(e) =>
            updateNodeStyle(node.id, "layout", {
              ...gl,
              row_gap: parseFloat((e.target as HTMLInputElement).value) || 0,
            })}
        />
      </div>
      <!-- Grid child placement (only show for non-root nodes) -->
      {#if node.id !== "root"}
        <div class="prop-row">
          <label>Grid Col</label>
          <input
            type="number"
            step="1"
            min="0"
            value={node.style.grid_column ?? ""}
            onchange={(e) => {
              const v = (e.target as HTMLInputElement).value;
              updateNodeStyle(
                node.id,
                "grid_column",
                v ? parseInt(v) : undefined,
              );
            }}
          />
        </div>
        <div class="prop-row">
          <label>Grid Row</label>
          <input
            type="number"
            step="1"
            min="0"
            value={node.style.grid_row ?? ""}
            onchange={(e) => {
              const v = (e.target as HTMLInputElement).value;
              updateNodeStyle(node.id, "grid_row", v ? parseInt(v) : undefined);
            }}
          />
        </div>
      {/if}
    {:else}
      <div class="prop-row">
        <label>Direction</label>
        <select
          value={node.style.flex.direction}
          onchange={(e) =>
            updateNodeStyle(node.id, "flex", {
              ...node.style.flex,
              direction: (e.target as HTMLSelectElement).value,
            })}
        >
          <option value="Row">Row</option>
          <option value="Column">Column</option>
        </select>
      </div>
      <div class="prop-row">
        <label>Gap</label>
        <input
          type="number"
          step="1"
          value={node.style.flex.gap}
          onchange={(e) =>
            updateNodeStyle(node.id, "flex", {
              ...node.style.flex,
              gap: parseFloat((e.target as HTMLInputElement).value) || 0,
            })}
        />
      </div>
      <div class="prop-row">
        <label>Align</label>
        <select
          value={node.style.flex.align_items}
          onchange={(e) =>
            updateNodeStyle(node.id, "flex", {
              ...node.style.flex,
              align_items: (e.target as HTMLSelectElement).value,
            })}
        >
          <option value="Start">Start</option>
          <option value="Center">Center</option>
          <option value="End">End</option>
          <option value="Stretch">Stretch</option>
        </select>
      </div>
      <div class="prop-row">
        <label>Justify</label>
        <select
          value={node.style.flex.justify_content}
          onchange={(e) =>
            updateNodeStyle(node.id, "flex", {
              ...node.style.flex,
              justify_content: (e.target as HTMLSelectElement).value,
            })}
        >
          <option value="Start">Start</option>
          <option value="Center">Center</option>
          <option value="End">End</option>
          <option value="SpaceBetween">Space Between</option>
          <option value="SpaceAround">Space Around</option>
        </select>
      </div>
      <!-- Grid child placement (show for all non-root nodes in case parent is grid) -->
      {#if node.id !== "root"}
        <div class="prop-row">
          <label>Grid Col</label>
          <input
            type="number"
            step="1"
            min="0"
            value={node.style.grid_column ?? ""}
            onchange={(e) => {
              const v = (e.target as HTMLInputElement).value;
              updateNodeStyle(
                node.id,
                "grid_column",
                v ? parseInt(v) : undefined,
              );
            }}
          />
        </div>
        <div class="prop-row">
          <label>Grid Row</label>
          <input
            type="number"
            step="1"
            min="0"
            value={node.style.grid_row ?? ""}
            onchange={(e) => {
              const v = (e.target as HTMLInputElement).value;
              updateNodeStyle(node.id, "grid_row", v ? parseInt(v) : undefined);
            }}
          />
        </div>
      {/if}
    {/if}
  </div>

  <!-- Appearance -->
  <div class="prop-section">
    <div class="prop-section-title">Appearance</div>
    <div class="prop-row">
      <label>BG Color</label>
      <input
        type="color"
        value={`#${Math.round(node.style.bg_color[0] * 255)
          .toString(16)
          .padStart(2, "0")}${Math.round(node.style.bg_color[1] * 255)
          .toString(16)
          .padStart(2, "0")}${Math.round(node.style.bg_color[2] * 255)
          .toString(16)
          .padStart(2, "0")}`}
        onchange={(e) => {
          const h = (e.target as HTMLInputElement).value;
          updateNodeStyle(node.id, "bg_color", [
            parseInt(h.slice(1, 3), 16) / 255,
            parseInt(h.slice(3, 5), 16) / 255,
            parseInt(h.slice(5, 7), 16) / 255,
            node.style.bg_color[3],
          ]);
        }}
      />
      <input
        type="number"
        step="0.05"
        min="0"
        max="1"
        value={node.style.bg_color[3]}
        style="width:50px"
        onchange={(e) =>
          updateNodeStyle(node.id, "bg_color", [
            node.style.bg_color[0],
            node.style.bg_color[1],
            node.style.bg_color[2],
            parseFloat((e.target as HTMLInputElement).value) || 0,
          ])}
      />
    </div>
    <div class="prop-row">
      <label>Border</label>
      <input
        type="number"
        step="1"
        value={node.style.border_width}
        style="width:50px"
        onchange={(e) =>
          updateNodeStyle(
            node.id,
            "border_width",
            parseFloat((e.target as HTMLInputElement).value) || 0,
          )}
      />
      <input
        type="color"
        value={`#${Math.round(node.style.border_color[0] * 255)
          .toString(16)
          .padStart(2, "0")}${Math.round(node.style.border_color[1] * 255)
          .toString(16)
          .padStart(2, "0")}${Math.round(node.style.border_color[2] * 255)
          .toString(16)
          .padStart(2, "0")}`}
        onchange={(e) => {
          const h = (e.target as HTMLInputElement).value;
          updateNodeStyle(node.id, "border_color", [
            parseInt(h.slice(1, 3), 16) / 255,
            parseInt(h.slice(3, 5), 16) / 255,
            parseInt(h.slice(5, 7), 16) / 255,
            node.style.border_color[3],
          ]);
        }}
      />
    </div>
    <div class="prop-row">
      <label title="Rounded corners, in pixels - background, border, button, bar and shadow follow them">Corners</label>
      <input
        type="number"
        step="1"
        min="0"
        value={node.style.corner_radius ?? 0}
        style="width:50px"
        onchange={(e) =>
          updateNodeStyle(node.id, "corner_radius", Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0))}
      />
    </div>
    <div class="prop-row">
      <label title="0..1, inherited: everything in this node draws at it">Opacity</label>
      <input
        type="number"
        step="0.05"
        min="0"
        max="1"
        value={node.style.opacity ?? 1}
        style="width:50px"
        onchange={(e) => {
          const v = parseFloat((e.target as HTMLInputElement).value);
          updateNodeStyle(node.id, "opacity", Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 1);
        }}
      />
      <label title="Scale about the node's centre, inherited - applied by layout, so it is clicked where it is drawn">Scale</label>
      <input
        type="number"
        step="0.05"
        min="0"
        value={node.style.scale ?? 1}
        style="width:50px"
        onchange={(e) => {
          const v = parseFloat((e.target as HTMLInputElement).value);
          updateNodeStyle(node.id, "scale", Number.isFinite(v) ? Math.max(0, v) : 1);
        }}
      />
    </div>
    <div class="prop-row">
      <label title="A soft drop shadow: offset y down, blur in pixels">Shadow</label>
      <input
        type="checkbox"
        checked={!!node.style.shadow}
        onchange={(e) =>
          updateNodeStyle(
            node.id,
            "shadow",
            (e.target as HTMLInputElement).checked ? { color: [0, 0, 0, 0.45], offset: [0, 6], blur: 14 } : null,
          )}
      />
      {#if node.style.shadow}
        {@const sh = node.style.shadow}
        <input
          type="number"
          step="1"
          title="Offset down, px"
          value={sh.offset[1]}
          style="width:44px"
          onchange={(e) =>
            updateNodeStyle(node.id, "shadow", { ...sh, offset: [sh.offset[0], parseFloat((e.target as HTMLInputElement).value) || 0] })}
        />
        <input
          type="number"
          step="1"
          min="0"
          title="Blur, px"
          value={sh.blur}
          style="width:44px"
          onchange={(e) =>
            updateNodeStyle(node.id, "shadow", { ...sh, blur: Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0) })}
        />
        <input
          type="number"
          step="0.05"
          min="0"
          max="1"
          title="Darkness (alpha)"
          value={sh.color[3]}
          style="width:44px"
          onchange={(e) =>
            updateNodeStyle(node.id, "shadow", {
              ...sh,
              color: [sh.color[0], sh.color[1], sh.color[2], Math.min(1, Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0))],
            })}
        />
      {/if}
    </div>
    <div class="prop-row">
      <label>Visible</label>
      <input
        type="checkbox"
        checked={node.style.visible}
        onchange={(e) =>
          updateNodeStyle(
            node.id,
            "visible",
            (e.target as HTMLInputElement).checked,
          )}
      />
    </div>
    <div class="prop-row">
      <label>Z-Order</label>
      <input
        type="number"
        step="1"
        value={node.style.z_order}
        onchange={(e) =>
          updateNodeStyle(
            node.id,
            "z_order",
            parseInt((e.target as HTMLInputElement).value) || 0,
          )}
      />
    </div>
  </div>

  <!-- Overflow / scrolling -->
  <div class="prop-section">
    <div class="prop-section-title">Overflow</div>
    <div class="prop-row">
      <label>Children</label>
      <select
        value={node.style.overflow ?? "Visible"}
        onchange={(e) =>
          updateNodeStyle(
            node.id,
            "overflow",
            (e.target as HTMLSelectElement).value,
          )}
      >
        <option value="Visible">Visible (no clip)</option>
        <option value="Hidden">Hidden (clip)</option>
        <option value="Scroll">Scroll (clip + wheel)</option>
      </select>
    </div>
    {#if (node.style.overflow ?? "Visible") === "Scroll"}
      <!-- Starting offsets. At runtime the engine drives these from the mouse
           wheel and clamps them to the content; scripts write them with
           gui_set(gui, node, "scroll_y", n). -->
      <div class="prop-row">
        <label>Scroll X</label>
        <input
          type="number"
          step="1"
          value={node.style.scroll_x ?? 0}
          onchange={(e) =>
            updateNodeStyle(
              node.id,
              "scroll_x",
              parseFloat((e.target as HTMLInputElement).value) || 0,
            )}
        />
      </div>
      <div class="prop-row">
        <label>Scroll Y</label>
        <input
          type="number"
          step="1"
          value={node.style.scroll_y ?? 0}
          onchange={(e) =>
            updateNodeStyle(
              node.id,
              "scroll_y",
              parseFloat((e.target as HTMLInputElement).value) || 0,
            )}
        />
      </div>
    {/if}
  </div>

  <!-- Focus / Navigation -->
  <div class="prop-section">
    <div class="prop-section-title">Navigation</div>
    <div class="prop-row">
      <label>Focusable</label>
      <input
        type="checkbox"
        checked={node.style.focusable ?? false}
        onchange={(e) =>
          updateNodeStyle(
            node.id,
            "focusable",
            (e.target as HTMLInputElement).checked,
          )}
      />
    </div>
    <div class="prop-row">
      <label title="What a screen reader calls this node (empty: its own words)">Reader Name</label>
      <input
        type="text"
        value={node.style.accessible?.label ?? ""}
        onchange={(e) =>
          updateNodeStyle(node.id, "accessible", {
            ...(node.style.accessible ?? {}),
            label: (e.target as HTMLInputElement).value,
          })}
      />
    </div>
    <div class="prop-row">
      <label title="Read after the name">Reader Description</label>
      <input
        type="text"
        value={node.style.accessible?.description ?? ""}
        onchange={(e) =>
          updateNodeStyle(node.id, "accessible", {
            ...(node.style.accessible ?? {}),
            description: (e.target as HTMLInputElement).value,
          })}
      />
    </div>
    <div class="prop-row">
      <label title="Decoration: a screen reader skips it">Reader Skips</label>
      <input
        type="checkbox"
        checked={node.style.accessible?.hidden ?? false}
        onchange={(e) =>
          updateNodeStyle(node.id, "accessible", {
            ...(node.style.accessible ?? {}),
            hidden: (e.target as HTMLInputElement).checked,
          })}
      />
    </div>
    {#if node.style.focusable}
      <div class="prop-row">
        <label>Tab Order</label>
        <input
          type="number"
          step="1"
          min="0"
          value={node.style.tab_order ?? 0}
          onchange={(e) =>
            updateNodeStyle(
              node.id,
              "tab_order",
              parseInt((e.target as HTMLInputElement).value) || 0,
            )}
        />
      </div>
    {/if}
  </div>

  <!-- Widget-specific properties -->
  <div class="prop-section">
    <div class="prop-section-title">{typeName} Properties</div>
    {#if typeName === "Panel"}
      <div class="prop-row">
        <label>Panel BG</label>
        <input
          type="color"
          value={`#${Math.round(node.style.bg_color[0] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round(node.style.bg_color[1] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round(node.style.bg_color[2] * 255)
            .toString(16)
            .padStart(2, "0")}`}
          onchange={(e) => {
            const h = (e.target as HTMLInputElement).value;
            const a = node.style.bg_color[3];
            updateNodeStyle(node.id, "bg_color", [
              parseInt(h.slice(1, 3), 16) / 255,
              parseInt(h.slice(3, 5), 16) / 255,
              parseInt(h.slice(5, 7), 16) / 255,
              a,
            ]);
          }}
        />
      </div>
    {:else if typeName === "Text"}
      <div class="prop-row">
        <label>Text</label><input
          type="text"
          value={data.text}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Text")
                (nt as any).text = (e.target as HTMLInputElement).value;
            })}
        />
      </div>
      <div class="prop-row">
        <label>Font</label><select
          value={data.font_id}
          onchange={(e) => {
            const v = (e.target as HTMLSelectElement).value;
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Text") (nt as any).font_id = v;
            });
          }}
        >
          <option value={isBuiltinFontId(data.font_id as string) ? (data.font_id as string) : ""}>(none - built-in font)</option>
          {#each $projectFontIds as id (id)}
            <option value={id}>{id}</option>
          {/each}
          {#if data.font_id && !isBuiltinFontId(data.font_id as string) && !$projectFontIds.includes(data.font_id as string)}
            <option value={data.font_id as string}
              >{data.font_id} (missing)</option
            >
          {/if}
        </select>
      </div>
      <div class="prop-row">
        <label>Font Size</label><input
          type="number"
          step="0.1"
          value={data.font_size}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Text")
                (nt as any).font_size =
                  parseFloat((e.target as HTMLInputElement).value) || 1;
            })}
        />
      </div>
      <div class="prop-row">
        <label title="Read the text as markup, after {{variables}} are filled in: [color=#e05040] [b] [wave] [shake] [rainbow] [icon=sprites/key.png], each closed like [/color].">Rich text</label>
        <input
          type="checkbox"
          checked={data.rich ?? false}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Text") {
                if ((e.target as HTMLInputElement).checked) (nt as any).rich = true;
                else delete (nt as any).rich;
              }
            })}
        />
      </div>
      <div class="prop-row">
        <label>Alignment</label>
        <select
          value={data.alignment}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Text")
                (nt as any).alignment = (e.target as HTMLSelectElement).value;
            })}
        >
          <option value="Left">Left</option>
          <option value="Center">Center</option>
          <option value="Right">Right</option>
        </select>
      </div>
      <div class="prop-row">
        <label>Color</label>
        <input
          type="color"
          value={`#${Math.round((data.color as number[])[0] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round((data.color as number[])[1] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round((data.color as number[])[2] * 255)
            .toString(16)
            .padStart(2, "0")}`}
          onchange={(e) => {
            const h = (e.target as HTMLInputElement).value;
            const c = data.color as number[];
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Text")
                (nt as any).color = [
                  parseInt(h.slice(1, 3), 16) / 255,
                  parseInt(h.slice(3, 5), 16) / 255,
                  parseInt(h.slice(5, 7), 16) / 255,
                  c[3],
                ];
            });
          }}
        />
      </div>
    {:else if typeName === "Image"}
      <div class="prop-row">
        <label>Texture</label><input
          type="text"
          value={data.texture_id}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Image")
                (nt as any).texture_id = (e.target as HTMLInputElement).value;
            })}
        />
      </div>
    {:else if typeName === "Button"}
      <div class="prop-row">
        <label>Label</label><input
          type="text"
          value={data.label}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Button")
                (nt as any).label = (e.target as HTMLInputElement).value;
            })}
        />
      </div>
      <div class="prop-row">
        <label>Font</label><select
          value={data.font_id}
          onchange={(e) => {
            const v = (e.target as HTMLSelectElement).value;
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Button") (nt as any).font_id = v;
            });
          }}
        >
          <option value={isBuiltinFontId(data.font_id as string) ? (data.font_id as string) : ""}>(none - built-in font)</option>
          {#each $projectFontIds as id (id)}
            <option value={id}>{id}</option>
          {/each}
          {#if data.font_id && !isBuiltinFontId(data.font_id as string) && !$projectFontIds.includes(data.font_id as string)}
            <option value={data.font_id as string}
              >{data.font_id} (missing)</option
            >
          {/if}
        </select>
      </div>
      <div class="prop-row">
        <label>Font Size</label><input
          type="number"
          step="0.1"
          value={data.font_size}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Button")
                (nt as any).font_size =
                  parseFloat((e.target as HTMLInputElement).value) || 1;
            })}
        />
      </div>
      <div class="prop-row">
        <label>Normal</label>
        <input
          type="color"
          value={`#${Math.round((data.normal_color as number[])[0] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.normal_color as number[])[1] * 255,
          )
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.normal_color as number[])[2] * 255,
          )
            .toString(16)
            .padStart(2, "0")}`}
          onchange={(e) => {
            const h = (e.target as HTMLInputElement).value;
            const c = data.normal_color as number[];
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Button")
                (nt as any).normal_color = [
                  parseInt(h.slice(1, 3), 16) / 255,
                  parseInt(h.slice(3, 5), 16) / 255,
                  parseInt(h.slice(5, 7), 16) / 255,
                  c[3],
                ];
            });
          }}
        />
      </div>
      <div class="prop-row">
        <label>Hover</label>
        <input
          type="color"
          value={`#${Math.round((data.hover_color as number[])[0] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.hover_color as number[])[1] * 255,
          )
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.hover_color as number[])[2] * 255,
          )
            .toString(16)
            .padStart(2, "0")}`}
          onchange={(e) => {
            const h = (e.target as HTMLInputElement).value;
            const c = data.hover_color as number[];
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Button")
                (nt as any).hover_color = [
                  parseInt(h.slice(1, 3), 16) / 255,
                  parseInt(h.slice(3, 5), 16) / 255,
                  parseInt(h.slice(5, 7), 16) / 255,
                  c[3],
                ];
            });
          }}
        />
      </div>
      <div class="prop-row">
        <label>Pressed</label>
        <input
          type="color"
          value={`#${Math.round((data.pressed_color as number[])[0] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.pressed_color as number[])[1] * 255,
          )
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.pressed_color as number[])[2] * 255,
          )
            .toString(16)
            .padStart(2, "0")}`}
          onchange={(e) => {
            const h = (e.target as HTMLInputElement).value;
            const c = data.pressed_color as number[];
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Button")
                (nt as any).pressed_color = [
                  parseInt(h.slice(1, 3), 16) / 255,
                  parseInt(h.slice(3, 5), 16) / 255,
                  parseInt(h.slice(5, 7), 16) / 255,
                  c[3],
                ];
            });
          }}
        />
      </div>
      <div class="prop-row">
        <label>Text Color</label>
        <input
          type="color"
          value={`#${Math.round((data.text_color as number[])[0] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.text_color as number[])[1] * 255,
          )
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.text_color as number[])[2] * 255,
          )
            .toString(16)
            .padStart(2, "0")}`}
          onchange={(e) => {
            const h = (e.target as HTMLInputElement).value;
            const c = data.text_color as number[];
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Button")
                (nt as any).text_color = [
                  parseInt(h.slice(1, 3), 16) / 255,
                  parseInt(h.slice(3, 5), 16) / 255,
                  parseInt(h.slice(5, 7), 16) / 255,
                  c[3],
                ];
            });
          }}
        />
      </div>
    {:else if typeName === "ProgressBar"}
      <div class="prop-row">
        <label>Value</label><input
          type="number"
          step="0.01"
          value={data.value}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "ProgressBar")
                (nt as any).value =
                  parseFloat((e.target as HTMLInputElement).value) || 0;
            })}
        />
      </div>
      <div class="prop-row">
        <label>Min</label><input
          type="number"
          step="0.1"
          value={data.min}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "ProgressBar")
                (nt as any).min =
                  parseFloat((e.target as HTMLInputElement).value) || 0;
            })}
        />
      </div>
      <div class="prop-row">
        <label>Max</label><input
          type="number"
          step="0.1"
          value={data.max}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "ProgressBar")
                (nt as any).max =
                  parseFloat((e.target as HTMLInputElement).value) || 1;
            })}
        />
      </div>
      <div class="prop-row">
        <label>FG</label>
        <input
          type="color"
          value={`#${Math.round((data.fg_color as number[])[0] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round((data.fg_color as number[])[1] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round((data.fg_color as number[])[2] * 255)
            .toString(16)
            .padStart(2, "0")}`}
          onchange={(e) => {
            const h = (e.target as HTMLInputElement).value;
            const c = data.fg_color as number[];
            updateNodeType(node.id, (nt) => {
              if (nt.type === "ProgressBar")
                (nt as any).fg_color = [
                  parseInt(h.slice(1, 3), 16) / 255,
                  parseInt(h.slice(3, 5), 16) / 255,
                  parseInt(h.slice(5, 7), 16) / 255,
                  c[3],
                ];
            });
          }}
        />
      </div>
      <div class="prop-row">
        <label>BG</label>
        <input
          type="color"
          value={`#${Math.round((data.bg_color as number[])[0] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round((data.bg_color as number[])[1] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round((data.bg_color as number[])[2] * 255)
            .toString(16)
            .padStart(2, "0")}`}
          onchange={(e) => {
            const h = (e.target as HTMLInputElement).value;
            const c = data.bg_color as number[];
            updateNodeType(node.id, (nt) => {
              if (nt.type === "ProgressBar")
                (nt as any).bg_color = [
                  parseInt(h.slice(1, 3), 16) / 255,
                  parseInt(h.slice(3, 5), 16) / 255,
                  parseInt(h.slice(5, 7), 16) / 255,
                  c[3],
                ];
            });
          }}
        />
      </div>
    {:else if typeName === "Slider"}
      <div class="prop-row">
        <label>Value</label><input
          type="number"
          step="0.01"
          value={data.value}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Slider")
                (nt as any).value =
                  parseFloat((e.target as HTMLInputElement).value) || 0;
            })}
        />
      </div>
      <div class="prop-row">
        <label>Min</label><input
          type="number"
          step="0.1"
          value={data.min}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Slider")
                (nt as any).min =
                  parseFloat((e.target as HTMLInputElement).value) || 0;
            })}
        />
      </div>
      <div class="prop-row">
        <label>Max</label><input
          type="number"
          step="0.1"
          value={data.max}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Slider")
                (nt as any).max =
                  parseFloat((e.target as HTMLInputElement).value) || 1;
            })}
        />
      </div>
      <div class="prop-row">
        <label>Thumb Size</label><input
          type="number"
          step="1"
          value={data.thumb_size}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Slider")
                (nt as any).thumb_size =
                  parseFloat((e.target as HTMLInputElement).value) || 12;
            })}
        />
      </div>
    {:else if typeName === "Toggle"}
      <div class="prop-row">
        <label>Checked</label><input
          type="checkbox"
          checked={data.checked}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Toggle")
                (nt as any).checked = (e.target as HTMLInputElement).checked;
            })}
        />
      </div>
      <div class="prop-row">
        <label>On Color</label>
        <input
          type="color"
          value={`#${Math.round((data.on_color as number[])[0] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round((data.on_color as number[])[1] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round((data.on_color as number[])[2] * 255)
            .toString(16)
            .padStart(2, "0")}`}
          onchange={(e) => {
            const h = (e.target as HTMLInputElement).value;
            const c = data.on_color as number[];
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Toggle")
                (nt as any).on_color = [
                  parseInt(h.slice(1, 3), 16) / 255,
                  parseInt(h.slice(3, 5), 16) / 255,
                  parseInt(h.slice(5, 7), 16) / 255,
                  c[3],
                ];
            });
          }}
        />
      </div>
      <div class="prop-row">
        <label>Off Color</label>
        <input
          type="color"
          value={`#${Math.round((data.off_color as number[])[0] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.off_color as number[])[1] * 255,
          )
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.off_color as number[])[2] * 255,
          )
            .toString(16)
            .padStart(2, "0")}`}
          onchange={(e) => {
            const h = (e.target as HTMLInputElement).value;
            const c = data.off_color as number[];
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Toggle")
                (nt as any).off_color = [
                  parseInt(h.slice(1, 3), 16) / 255,
                  parseInt(h.slice(3, 5), 16) / 255,
                  parseInt(h.slice(5, 7), 16) / 255,
                  c[3],
                ];
            });
          }}
        />
      </div>
    {:else if typeName === "TextInput"}
      <div class="prop-row">
        <label>Text</label><input
          type="text"
          value={data.text}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "TextInput")
                (nt as any).text = (e.target as HTMLInputElement).value;
            })}
        />
      </div>
      <div class="prop-row">
        <label>Placeholder</label><input
          type="text"
          value={data.placeholder}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "TextInput")
                (nt as any).placeholder = (e.target as HTMLInputElement).value;
            })}
        />
      </div>
      <div class="prop-row">
        <label>Font</label><select
          value={data.font_id}
          onchange={(e) => {
            const v = (e.target as HTMLSelectElement).value;
            updateNodeType(node.id, (nt) => {
              if (nt.type === "TextInput") (nt as any).font_id = v;
            });
          }}
        >
          <option value={isBuiltinFontId(data.font_id as string) ? (data.font_id as string) : ""}>(none - built-in font)</option>
          {#each $projectFontIds as id (id)}
            <option value={id}>{id}</option>
          {/each}
          {#if data.font_id && !isBuiltinFontId(data.font_id as string) && !$projectFontIds.includes(data.font_id as string)}
            <option value={data.font_id as string}
              >{data.font_id} (missing)</option
            >
          {/if}
        </select>
      </div>
      <div class="prop-row">
        <label>Font Size</label><input
          type="number"
          step="0.1"
          value={data.font_size}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "TextInput")
                (nt as any).font_size =
                  parseFloat((e.target as HTMLInputElement).value) || 1;
            })}
        />
      </div>
    {:else if typeName === "Circle"}
      <div class="prop-row">
        <label>Fill</label>
        <input
          type="color"
          value={`#${Math.round((data.fill_color as number[])[0] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.fill_color as number[])[1] * 255,
          )
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.fill_color as number[])[2] * 255,
          )
            .toString(16)
            .padStart(2, "0")}`}
          onchange={(e) => {
            const h = (e.target as HTMLInputElement).value;
            const c = data.fill_color as number[];
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Circle")
                (nt as any).fill_color = [
                  parseInt(h.slice(1, 3), 16) / 255,
                  parseInt(h.slice(3, 5), 16) / 255,
                  parseInt(h.slice(5, 7), 16) / 255,
                  c[3],
                ];
            });
          }}
        />
      </div>
      <div class="prop-row">
        <label>Fill Alpha</label><input
          type="number"
          step="0.05"
          min="0"
          max="1"
          value={(data.fill_color as number[])[3]}
          onchange={(e) => {
            const a = parseFloat((e.target as HTMLInputElement).value) || 0;
            const c = data.fill_color as number[];
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Circle")
                (nt as any).fill_color = [c[0], c[1], c[2], a];
            });
          }}
        />
      </div>
      <div class="prop-row">
        <label>Stroke</label>
        <input
          type="color"
          value={`#${Math.round((data.stroke_color as number[])[0] * 255)
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.stroke_color as number[])[1] * 255,
          )
            .toString(16)
            .padStart(2, "0")}${Math.round(
            (data.stroke_color as number[])[2] * 255,
          )
            .toString(16)
            .padStart(2, "0")}`}
          onchange={(e) => {
            const h = (e.target as HTMLInputElement).value;
            const c = data.stroke_color as number[];
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Circle")
                (nt as any).stroke_color = [
                  parseInt(h.slice(1, 3), 16) / 255,
                  parseInt(h.slice(3, 5), 16) / 255,
                  parseInt(h.slice(5, 7), 16) / 255,
                  c[3],
                ];
            });
          }}
        />
      </div>
      <div class="prop-row">
        <label>Stroke Alpha</label><input
          type="number"
          step="0.05"
          min="0"
          max="1"
          value={(data.stroke_color as number[])[3]}
          onchange={(e) => {
            const a = parseFloat((e.target as HTMLInputElement).value) || 0;
            const c = data.stroke_color as number[];
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Circle")
                (nt as any).stroke_color = [c[0], c[1], c[2], a];
            });
          }}
        />
      </div>
      <div class="prop-row">
        <label>Stroke Width</label><input
          type="number"
          step="0.5"
          min="0"
          value={data.stroke_width}
          onchange={(e) =>
            updateNodeType(node.id, (nt) => {
              if (nt.type === "Circle")
                (nt as any).stroke_width =
                  parseFloat((e.target as HTMLInputElement).value) || 0;
            })}
        />
      </div>
    {/if}
  </div>
{:else}
  <div class="empty-props">Select a node to edit properties</div>
{/if}

<style>
  .empty-props {
    padding: 16px;
    color: var(--ed-faint);
    font-size: 12px;
    font-style: italic;
    text-align: center;
  }

  .prop-section {
    padding: 8px 10px;
    border-bottom: 1px solid var(--ed-line);
  }

  .prop-section-title {
    font-family: var(--ed-mono);
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ed-muted);
    margin-bottom: 6px;
  }

  .prop-row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
  }

  .prop-row label {
    font-size: 11px;
    color: var(--ed-muted);
    width: 60px;
    flex-shrink: 0;
    text-align: right;
  }

  .prop-row input[type="text"],
  .prop-row input[type="number"],
  .prop-row select {
    flex: 1;
    background: var(--ed-raised);
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 12px;
    padding: 2px 6px;
    font-family: inherit;
    min-width: 0;
  }

  .prop-row input:focus,
  .prop-row select:focus {
    border-color: var(--ed-accent);
    outline: none;
  }

  .prop-row input[type="color"] {
    width: 28px;
    height: 24px;
    padding: 1px;
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .prop-row input[type="checkbox"] {
    accent-color: var(--ed-accent);
  }

  .prop-value {
    font-size: 12px;
    color: var(--ed-faint);
  }

  .prop-row-4 {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr 1fr;
    gap: 4px;
    margin-bottom: 4px;
  }

  .prop-row-4 input {
    background: var(--ed-raised);
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 11px;
    padding: 2px 4px;
    text-align: center;
    font-family: var(--ed-mono);
    width: 100%;
  }

  .prop-row-4 input:focus {
    border-color: var(--ed-accent);
    outline: none;
  }

  /* Constraint editor */
  .anchor-presets {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 2px;
    margin-bottom: 8px;
  }

  .anchor-btn {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    color: var(--ed-text);
    padding: 4px;
    font-size: 12px;
    cursor: pointer;
    border-radius: 4px;
    line-height: 1;
    text-align: center;
  }

  .anchor-btn:hover {
    background: var(--ed-line);
    border-color: var(--ed-accent);
  }

  .stretch-btn {
    grid-column: span 1;
  }

  .constraint-row {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .constraint-row input[type="checkbox"] {
    width: 14px;
    height: 14px;
    margin: 0;
    flex-shrink: 0;
  }

  .constraint-label {
    width: 44px;
    font-size: 11px;
    flex-shrink: 0;
  }

  .constraint-value {
    width: 60px !important;
    flex: 1;
  }

  .constraint-unit {
    width: 44px;
    flex-shrink: 0;
    font-size: 11px;
    background: var(--ed-raised);
    color: var(--ed-text);
    border: 1px solid var(--ed-line-strong);
    border-radius: 3px;
    padding: 2px;
  }

  .constraint-hint {
    font-size: 10px;
    color: var(--ed-muted);
    padding: 2px 0;
    font-style: italic;
  }

  .constraint-clear {
    background: none;
    border: 1px solid var(--ed-line-strong);
    color: var(--ed-muted);
    padding: 4px 8px;
    font-size: 11px;
    cursor: pointer;
    border-radius: 3px;
    margin-top: 4px;
    width: 100%;
  }

  .constraint-clear:hover {
    color: var(--ed-err);
    border-color: var(--ed-err);
  }
</style>
