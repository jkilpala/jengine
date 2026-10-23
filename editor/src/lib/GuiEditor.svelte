<script lang="ts">
    import { claimUndo } from "./stores/undoTargetStore";
    import { requestEditorClose } from "./stores/editorClose";
  import {
    guiDocument,
    guiFilePath,
    guiDirty,
    selectedNodeId,
    selectedNode,
    canUndo,
    canRedo,
    saveCurrentGui,
    closeGuiEditor,
    addChild,
    deleteNode,
    duplicateNode,
    moveNode,
    renameNode,
    executeGui,
    undo,
    redo,
  } from "./stores/guiEditorStore";
  import {
    getNodeTypeName,
    findNode,
    constraintsIsEmpty,
    resolveConstraintValue,
    type GuiNode,
    type GuiDocument as GuiDocType,
    type SizeValue,
    type Constraints,
  } from "./api/gui";
  import { devicePreview } from "./stores/devicePreviewStore";
  import { themeColors, withAlpha } from "./stores/themeColors";
  import {
    fitRect,
    profileById,
    safeRect,
    screenAspect,
    type Rect,
  } from "./devicePreview";
  import DevicePicker from "./DevicePicker.svelte";
  import DeviceOverlay from "./DeviceOverlay.svelte";

  // ---------------------------------------------------------------------------
  // State
  // ---------------------------------------------------------------------------
  let canvasEl: HTMLCanvasElement | undefined = $state();
  let canvasW = $state(800);
  let canvasH = $state(600);
  let renamingId = $state<string | null>(null);
  let renameValue = $state("");
  let contextMenu = $state<{ x: number; y: number; nodeId: string } | null>(
    null,
  );

  // Drag-to-move state (canvas)
  let dragging = $state(false);
  let dragNodeId = $state<string | null>(null);
  let dragStartMouse = $state({ x: 0, y: 0 });
  let dragStartOffset = $state({ x: 0, y: 0 });

  // Tree drag-and-drop state (reparent)
  let treeDragId = $state<string | null>(null);
  let treeDropTarget = $state<string | null>(null);

  // Resize handle state
  let resizing = $state(false);
  let resizeHandle = $state<string | null>(null);
  let resizeStartRect = $state<LayoutRect>({ x: 0, y: 0, width: 0, height: 0 });
  let resizeStartMouse = $state({ x: 0, y: 0 });

  // Grid & snap state
  let showGrid = $state(true);
  let snapEnabled = $state(true);
  let gridSize = $state(16);
  let activeGuides = $state<{ axis: "x" | "y"; pos: number }[]>([]);
  let dragStartLayoutRect = $state<LayoutRect>({
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  });

  interface LayoutRect {
    x: number;
    y: number;
    width: number;
    height: number;
  }

  type HandleInfo = { name: string; x: number; y: number };

  function getResizeHandlePositions(r: LayoutRect): HandleInfo[] {
    const mx = r.x + r.width / 2;
    const my = r.y + r.height / 2;
    return [
      { name: "nw", x: r.x, y: r.y },
      { name: "n", x: mx, y: r.y },
      { name: "ne", x: r.x + r.width, y: r.y },
      { name: "w", x: r.x, y: my },
      { name: "e", x: r.x + r.width, y: my },
      { name: "sw", x: r.x, y: r.y + r.height },
      { name: "s", x: mx, y: r.y + r.height },
      { name: "se", x: r.x + r.width, y: r.y + r.height },
    ];
  }

  const WIDGET_TYPES = [
    "Panel",
    "Text",
    "Image",
    "Button",
    "ProgressBar",
    "Slider",
    "Toggle",
    "TextInput",
    "Circle",
  ];

  // ---------------------------------------------------------------------------
  // Grid & alignment helpers
  // ---------------------------------------------------------------------------
  function gridSnap(value: number): number {
    return Math.round(value / gridSize) * gridSize;
  }

  function findParentNode(root: GuiNode, targetId: string): GuiNode | null {
    for (const child of root.children) {
      if (child.id === targetId) return root;
      const found = findParentNode(child, targetId);
      if (found) return found;
    }
    return null;
  }

  function computeAlignmentGuides(
    candidateRect: LayoutRect,
    otherRects: LayoutRect[],
    threshold: number,
  ): {
    adjustX: number;
    adjustY: number;
    guides: { axis: "x" | "y"; pos: number }[];
  } {
    let adjustX = 0;
    let adjustY = 0;
    let bestDistX = threshold + 1;
    let bestDistY = threshold + 1;
    let guideX = 0;
    let guideY = 0;

    const myLeft = candidateRect.x;
    const myRight = candidateRect.x + candidateRect.width;
    const myCenterX = candidateRect.x + candidateRect.width / 2;
    const myTop = candidateRect.y;
    const myBottom = candidateRect.y + candidateRect.height;
    const myCenterY = candidateRect.y + candidateRect.height / 2;
    const myEdgesX = [myLeft, myRight, myCenterX];
    const myEdgesY = [myTop, myBottom, myCenterY];

    for (const r of otherRects) {
      const otherEdgesX = [r.x, r.x + r.width, r.x + r.width / 2];
      const otherEdgesY = [r.y, r.y + r.height, r.y + r.height / 2];
      for (const myE of myEdgesX) {
        for (const otherE of otherEdgesX) {
          const dist = Math.abs(myE - otherE);
          if (dist < bestDistX) {
            bestDistX = dist;
            adjustX = otherE - myE;
            guideX = otherE;
          }
        }
      }
      for (const myE of myEdgesY) {
        for (const otherE of otherEdgesY) {
          const dist = Math.abs(myE - otherE);
          if (dist < bestDistY) {
            bestDistY = dist;
            adjustY = otherE - myE;
            guideY = otherE;
          }
        }
      }
    }

    const guides: { axis: "x" | "y"; pos: number }[] = [];
    if (bestDistX <= threshold) guides.push({ axis: "x", pos: guideX });
    else adjustX = 0;
    if (bestDistY <= threshold) guides.push({ axis: "y", pos: guideY });
    else adjustY = 0;
    return { adjustX, adjustY, guides };
  }

  /** Substitute {{key}} placeholders with values from a variables map. */
  function substituteTemplate(
    text: string,
    vars: Record<string, unknown>,
  ): string {
    if (!text.includes("{{") || !vars) return text;
    return text.replace(/\{\{(\w+)\}\}/g, (_, key) => {
      const v = vars[key];
      if (v === undefined || v === null) return `{{${key}}}`;
      return String(v);
    });
  }

  // ---------------------------------------------------------------------------
  // Layout engine (TypeScript port of Rust compute_layout)
  // ---------------------------------------------------------------------------

  function resolveSize(sv: SizeValue, parentSize: number): number {
    if (sv.type === "Fixed") return sv.value;
    if (sv.type === "Percent") return (sv.value / 100) * parentSize;
    return 0;
  }

  // Content box of every Hidden/Scroll container, filled during layout and
  // consumed by `renderNode` to clip that subtree.
  let clipRects = new Map<string, LayoutRect>();
  // Scrollable range of every Overflow: Scroll container, also filled during
  // layout - the wheel handler clamps against it.
  let scrollRanges = new Map<string, { x: number; y: number }>();
  // Wheel-driven preview offset, ADDED to the node's authored scroll_x/_y.
  // Canvas-only: never written to the document, so scrolling around while
  // authoring neither dirties the file nor fills the undo stack.
  let previewScroll = new Map<string, { x: number; y: number }>();

  // Matches WHEEL_SCROLL_STEP in crates/jengine-core/src/ui/layout.rs, so a
  // notch here moves the list by the same amount it will at runtime.
  const WHEEL_SCROLL_STEP = 58;

  function clamp(v: number, lo: number, hi: number): number {
    return Math.min(hi, Math.max(lo, v));
  }

  function flexChildrenOf(node: GuiNode): GuiNode[] {
    return node.children.filter(
      (c) => c.style.visible && constraintsIsEmpty(c.style.constraints),
    );
  }

  /// Natural size of a container's children - the scrollable range is this
  /// minus the content box. Mirrors `measure_content` in layout.rs.
  function measureContent(
    node: GuiNode,
    children: GuiNode[],
    cw: number,
    ch: number,
  ): { w: number; h: number } {
    if (children.length === 0) return { w: 0, h: 0 };
    const isRow = node.style.flex.direction === "Row";
    const gapTotal = node.style.flex.gap * Math.max(0, children.length - 1);
    let main = 0;
    let cross = 0;
    for (const c of children) {
      const m = c.style.margin;
      const w = (resolveSize(c.style.width, cw) || 0) + m.left + m.right;
      const h = (resolveSize(c.style.height, ch) || 0) + m.top + m.bottom;
      main += isRow ? w : h;
      cross = Math.max(cross, isRow ? h : w);
    }
    return isRow
      ? { w: main + gapTotal, h: cross }
      : { w: cross, h: main + gapTotal };
  }

  // ── Device preview ────────────────────────────────────────────────────
  // A chosen phone letterboxes the canvas to its shape and lays the document
  // out in its safe area, as the engine does on the device - so a panel the
  // notch would cover is seen to move, not discovered on a phone.
  let deviceRect = $state<Rect | null>(null);
  /** The safe area the document is laid out in, or null for the canvas. */
  let layoutArea: Rect | null = null;
  const previewProfile = $derived(profileById($devicePreview.id));

  function computeLayout(
    doc: GuiDocType,
    screenW: number,
    screenH: number,
  ): Map<string, LayoutRect> {
    const result = new Map<string, LayoutRect>();
    clipRects = new Map<string, LayoutRect>();
    scrollRanges = new Map<string, { x: number; y: number }>();
    const a = layoutArea;
    layoutNode(doc.root, a?.x ?? 0, a?.y ?? 0, a?.width ?? screenW, a?.height ?? screenH, result);
    return result;
  }

  function layoutNode(
    node: GuiNode,
    px: number,
    py: number,
    pw: number,
    ph: number,
    out: Map<string, LayoutRect>,
  ) {
    if (!node.style.visible) return;

    const w = resolveSize(node.style.width, pw) || pw;
    const h = resolveSize(node.style.height, ph) || ph;
    const m = node.style.margin;
    const ox = node.style.offset_x || 0;
    const oy = node.style.offset_y || 0;
    const x = px + m.left + ox;
    const y = py + m.top + oy;
    const aw = w - m.left - m.right;
    const ah = h - m.top - m.bottom;

    out.set(node.id, { x, y, width: Math.max(0, aw), height: Math.max(0, ah) });

    // Layout children
    const pad = node.style.padding;
    const cx = x + pad.left;
    const cy = y + pad.top;
    const cw = aw - pad.left - pad.right;
    const ch = ah - pad.top - pad.bottom;

    const visibleChildren = node.children.filter((c) => c.style.visible);
    if (visibleChildren.length === 0) return;

    // Overflow: children of a Hidden/Scroll container are clipped to the
    // content box, and a Scroll container shifts them by its offset. Mirrors
    // `layout_node` in crates/jengine-core/src/ui/layout.rs - the canvas here
    // is a preview of that, so a divergence shows up as "looks fine in the
    // GUI editor, clipped at runtime".
    const overflow = node.style.overflow ?? "Visible";
    let sx = 0;
    let sy = 0;
    if (overflow === "Scroll") {
      const natural = measureContent(node, flexChildrenOf(node), cw, ch);
      const maxX = Math.max(0, natural.w - cw);
      const maxY = Math.max(0, natural.h - ch);
      scrollRanges.set(node.id, { x: maxX, y: maxY });
      const preview = previewScroll.get(node.id);
      sx = clamp((node.style.scroll_x ?? 0) + (preview?.x ?? 0), 0, maxX);
      sy = clamp((node.style.scroll_y ?? 0) + (preview?.y ?? 0), 0, maxY);
    }
    if (overflow !== "Visible") {
      clipRects.set(node.id, { x: cx, y: cy, width: cw, height: ch });
    }

    // Separate constrained children (positioned absolutely) from flex/grid children.
    const flexChildren = visibleChildren.filter((c) =>
      constraintsIsEmpty(c.style.constraints),
    );
    const constrainedChildren = visibleChildren.filter(
      (c) => !constraintsIsEmpty(c.style.constraints),
    );

    // Dispatch unconstrained children to grid or flex layout.
    if (flexChildren.length > 0) {
      if (node.style.layout && node.style.layout.mode === "Grid") {
        layoutGrid(node, flexChildren, cx - sx, cy - sy, cw, ch, out);
      } else {
        layoutFlex(node, flexChildren, cx - sx, cy - sy, cw, ch, out);
      }
    }

    // Position constrained children absolutely within content area.
    for (const child of constrainedChildren) {
      const cr = resolveConstraintsRect(child, cx - sx, cy - sy, cw, ch);
      layoutNode(child, cr.x, cr.y, cr.width, cr.height, out);
    }
  }

  function resolveGridTracks(
    defs: { type: string; value?: number }[],
    available: number,
    count: number,
  ): number[] {
    const sizes = new Array(count).fill(0);
    let remaining = available;
    let totalFr = 0;

    for (let i = 0; i < Math.min(defs.length, count); i++) {
      const def = defs[i];
      if (def.type === "Fixed") {
        sizes[i] = def.value ?? 0;
        remaining -= sizes[i];
      } else if (def.type === "Auto") {
        const autoSize = Math.min(40, available / count);
        sizes[i] = autoSize;
        remaining -= autoSize;
      } else if (def.type === "Fraction") {
        totalFr += def.value ?? 1;
      }
    }
    for (let i = defs.length; i < count; i++) {
      totalFr += 1;
    }
    remaining = Math.max(0, remaining);
    if (totalFr > 0) {
      for (let i = 0; i < Math.min(defs.length, count); i++) {
        if (defs[i].type === "Fraction") {
          sizes[i] = remaining * ((defs[i].value ?? 1) / totalFr);
        }
      }
      for (let i = defs.length; i < count; i++) {
        sizes[i] = remaining * (1 / totalFr);
      }
    }
    return sizes;
  }

  function layoutGrid(
    node: GuiNode,
    visibleChildren: GuiNode[],
    cx: number,
    cy: number,
    cw: number,
    ch: number,
    out: Map<string, LayoutRect>,
  ) {
    const layout = node.style.layout!;
    const columns = (layout as any).columns ?? [{ type: "Fraction", value: 1 }];
    const rows = (layout as any).rows ?? [{ type: "Fraction", value: 1 }];
    const colGap = (layout as any).column_gap ?? 0;
    const rowGap = (layout as any).row_gap ?? 0;

    const numCols = Math.max(1, columns.length);
    const neededRows = Math.ceil(visibleChildren.length / numCols);
    const numRows = Math.max(rows.length, neededRows, 1);

    const colGapTotal = numCols > 1 ? colGap * (numCols - 1) : 0;
    const colWidths = resolveGridTracks(columns, cw - colGapTotal, numCols);

    const rowDefs = [...rows];
    while (rowDefs.length < numRows)
      rowDefs.push({ type: "Fraction", value: 1 });
    const rowGapTotal = numRows > 1 ? rowGap * (numRows - 1) : 0;
    const rowHeights = resolveGridTracks(rowDefs, ch - rowGapTotal, numRows);

    // Compute cumulative offsets
    const colOffsets = [cx];
    for (let i = 0; i < colWidths.length; i++) {
      const gap = i < numCols - 1 ? colGap : 0;
      colOffsets.push(colOffsets[i] + colWidths[i] + gap);
    }
    const rowOffsets = [cy];
    for (let i = 0; i < rowHeights.length; i++) {
      const gap = i < numRows - 1 ? rowGap : 0;
      rowOffsets.push(rowOffsets[i] + rowHeights[i] + gap);
    }

    let autoCursor = 0;
    for (const child of visibleChildren) {
      let col: number, row: number;
      if (
        child.style.grid_column !== undefined &&
        child.style.grid_row !== undefined
      ) {
        col = Math.min(child.style.grid_column, numCols - 1);
        row = Math.min(child.style.grid_row, numRows - 1);
      } else {
        col = autoCursor % numCols;
        row = Math.min(Math.floor(autoCursor / numCols), numRows - 1);
        autoCursor++;
      }

      const cellX = colOffsets[col];
      const cellY = rowOffsets[row];
      const cellW = colWidths[col] ?? 0;
      const cellH = rowHeights[row] ?? 0;

      const cm = child.style.margin;
      layoutNode(
        child,
        cellX + cm.left,
        cellY + cm.top,
        cellW - cm.left - cm.right,
        cellH - cm.top - cm.bottom,
        out,
      );
    }
  }

  function layoutFlex(
    node: GuiNode,
    visibleChildren: GuiNode[],
    cx: number,
    cy: number,
    cw: number,
    ch: number,
    out: Map<string, LayoutRect>,
  ) {
    const flex = node.style.flex;
    const isRow = flex.direction === "Row";
    const gap = flex.gap;

    // Calculate children sizes for main axis
    let totalMain = 0;
    const childSizes: { w: number; h: number }[] = [];
    for (const child of visibleChildren) {
      const childW = resolveSize(child.style.width, cw) || (isRow ? 0 : cw);
      const childH = resolveSize(child.style.height, ch) || (isRow ? ch : 0);
      childSizes.push({ w: childW, h: childH });
      totalMain += isRow ? childW : childH;
    }
    totalMain += gap * (visibleChildren.length - 1);

    // Justify content
    const mainSpace = isRow ? cw : ch;
    let mainOffset = 0;
    let mainGap = gap;
    if (flex.justify_content === "Center") {
      mainOffset = (mainSpace - totalMain) / 2;
    } else if (flex.justify_content === "End") {
      mainOffset = mainSpace - totalMain;
    } else if (
      flex.justify_content === "SpaceBetween" &&
      visibleChildren.length > 1
    ) {
      mainGap =
        (mainSpace - (totalMain - gap * (visibleChildren.length - 1))) /
        (visibleChildren.length - 1);
    } else if (
      flex.justify_content === "SpaceAround" &&
      visibleChildren.length > 0
    ) {
      const spacing =
        (mainSpace - (totalMain - gap * (visibleChildren.length - 1))) /
        visibleChildren.length;
      mainOffset = spacing / 2;
      mainGap = spacing;
    }

    let cursor = mainOffset;
    for (let i = 0; i < visibleChildren.length; i++) {
      const child = visibleChildren[i];
      const cs = childSizes[i];

      // Cross axis alignment
      const crossSize = isRow ? ch : cw;
      const childCross = isRow ? cs.h : cs.w;
      let crossOffset = 0;
      if (flex.align_items === "Center") {
        crossOffset = (crossSize - childCross) / 2;
      } else if (flex.align_items === "End") {
        crossOffset = crossSize - childCross;
      }

      const childX = isRow ? cx + cursor : cx + crossOffset;
      const childY = isRow ? cy + crossOffset : cy + cursor;
      const childW = isRow ? cs.w : flex.align_items === "Stretch" ? cw : cs.w;
      const childH = isRow
        ? flex.align_items === "Stretch"
          ? ch
          : cs.h
        : cs.h;

      layoutNode(child, childX, childY, childW, childH, out);
      cursor += (isRow ? cs.w : cs.h) + mainGap;
    }
  }

  function resolveConstraintsRect(
    child: GuiNode,
    px: number,
    py: number,
    pw: number,
    ph: number,
  ): LayoutRect {
    const c = child.style.constraints!;

    // Horizontal axis
    let x: number, width: number;
    if (c.left && c.right) {
      const lv = resolveConstraintValue(c.left, pw);
      const rv = resolveConstraintValue(c.right, pw);
      x = px + lv;
      width = Math.max(0, pw - lv - rv);
    } else if (c.left) {
      const lv = resolveConstraintValue(c.left, pw);
      width = resolveSize(child.style.width, pw) || pw;
      x = px + lv;
    } else if (c.right) {
      const rv = resolveConstraintValue(c.right, pw);
      width = resolveSize(child.style.width, pw) || pw;
      x = px + pw - rv - width;
    } else {
      width = resolveSize(child.style.width, pw) || pw;
      x = px;
    }

    // Vertical axis
    let y: number, height: number;
    if (c.top && c.bottom) {
      const tv = resolveConstraintValue(c.top, ph);
      const bv = resolveConstraintValue(c.bottom, ph);
      y = py + tv;
      height = Math.max(0, ph - tv - bv);
    } else if (c.top) {
      const tv = resolveConstraintValue(c.top, ph);
      height = resolveSize(child.style.height, ph) || ph;
      y = py + tv;
    } else if (c.bottom) {
      const bv = resolveConstraintValue(c.bottom, ph);
      height = resolveSize(child.style.height, ph) || ph;
      y = py + ph - bv - height;
    } else {
      height = resolveSize(child.style.height, ph) || ph;
      y = py;
    }

    return { x, y, width, height };
  }

  // ---------------------------------------------------------------------------
  // Canvas rendering
  // ---------------------------------------------------------------------------
  function renderCanvas() {
    if (!canvasEl || !$guiDocument) return;
    const ctx = canvasEl.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    // Letterbox to a previewed device, or fill the area.
    const profile = profileById($devicePreview.id);
    const portrait = $devicePreview.portrait;
    const area = canvasEl.parentElement!.getBoundingClientRect();
    if (profile) {
      const f = fitRect(area.width, area.height, screenAspect(profile, portrait));
      canvasEl.style.left = `${f.x}px`;
      canvasEl.style.top = `${f.y}px`;
      canvasEl.style.width = `${f.width}px`;
      canvasEl.style.height = `${f.height}px`;
      canvasEl.style.right = "auto";
      canvasEl.style.bottom = "auto";
      deviceRect = f;
    } else {
      for (const k of ["left", "top", "width", "height", "right", "bottom"] as const) {
        canvasEl.style[k] = "";
      }
      deviceRect = null;
    }
    const rect = canvasEl.getBoundingClientRect();
    canvasW = rect.width;
    canvasH = rect.height;
    layoutArea = profile ? safeRect(profile, portrait, canvasW, canvasH) : null;
    canvasEl.width = canvasW * dpr;
    canvasEl.height = canvasH * dpr;
    ctx.scale(dpr, dpr);

    const t = $themeColors;
    // Clear
    ctx.fillStyle = t.mantle;
    ctx.fillRect(0, 0, canvasW, canvasH);

    // Draw screen boundary
    ctx.strokeStyle = t.line;
    ctx.lineWidth = 1;
    ctx.strokeRect(0.5, 0.5, canvasW - 1, canvasH - 1);

    // Draw grid overlay
    if (showGrid) {
      ctx.strokeStyle = withAlpha(t["line-strong"], 0.35);
      ctx.lineWidth = 0.5;
      for (let gx = gridSize; gx < canvasW; gx += gridSize) {
        ctx.beginPath();
        ctx.moveTo(gx + 0.5, 0);
        ctx.lineTo(gx + 0.5, canvasH);
        ctx.stroke();
      }
      for (let gy = gridSize; gy < canvasH; gy += gridSize) {
        ctx.beginPath();
        ctx.moveTo(0, gy + 0.5);
        ctx.lineTo(canvasW, gy + 0.5);
        ctx.stroke();
      }
    }

    const layoutMap = computeLayout($guiDocument, canvasW, canvasH);
    renderNode(ctx, $guiDocument.root, layoutMap, null);

    // Draw alignment guides
    if (activeGuides.length > 0) {
      ctx.strokeStyle = withAlpha(t.a3, 0.7);
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 4]);
      for (const guide of activeGuides) {
        ctx.beginPath();
        if (guide.axis === "x") {
          ctx.moveTo(guide.pos + 0.5, 0);
          ctx.lineTo(guide.pos + 0.5, canvasH);
        } else {
          ctx.moveTo(0, guide.pos + 0.5);
          ctx.lineTo(canvasW, guide.pos + 0.5);
        }
        ctx.stroke();
      }
      ctx.setLineDash([]);
    }
  }

  function colorToCSS(c: [number, number, number, number]): string {
    return `rgba(${Math.round(c[0] * 255)}, ${Math.round(c[1] * 255)}, ${Math.round(c[2] * 255)}, ${c[3]})`;
  }

  function renderNode(
    ctx: CanvasRenderingContext2D,
    node: GuiNode,
    layoutMap: Map<string, LayoutRect>,
    parentId: string | null,
  ) {
    const r = layoutMap.get(node.id);
    if (!r) return;

    const typeName = getNodeTypeName(node.node_type);
    const data = node.node_type as Record<string, unknown>;
    const isSelected = $selectedNodeId === node.id;

    // Background
    const bgColor = node.style.bg_color;
    if (bgColor[3] > 0) {
      ctx.fillStyle = colorToCSS(bgColor);
      ctx.fillRect(r.x, r.y, r.width, r.height);
    }

    // Widget-specific rendering
    if (typeName === "Panel") {
      // Panel uses style.bg_color (already rendered above)
    } else if (typeName === "Text") {
      const tc = data.color as [number, number, number, number];
      ctx.fillStyle = colorToCSS(tc);
      ctx.font = `${Math.max(8, data.font_size as number)}px jEngineDefault`;
      ctx.textBaseline = "middle";
      const align = data.alignment as string;
      ctx.textAlign =
        align === "Center" ? "center" : align === "Right" ? "right" : "left";
      const tx =
        align === "Center"
          ? r.x + r.width / 2
          : align === "Right"
            ? r.x + r.width - 4
            : r.x + 4;
      ctx.fillText(
        substituteTemplate(data.text as string, {}),
        tx,
        r.y + r.height / 2,
        r.width - 8,
      );
    } else if (typeName === "Image") {
      ctx.strokeStyle = $themeColors["line-strong"];
      ctx.lineWidth = 1;
      ctx.strokeRect(r.x + 0.5, r.y + 0.5, r.width - 1, r.height - 1);
      ctx.fillStyle = $themeColors.faint;
      ctx.font = "10px jEngineDefault";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText("🖼 Image", r.x + r.width / 2, r.y + r.height / 2);
    } else if (typeName === "Button") {
      const nc = data.normal_color as [number, number, number, number];
      ctx.fillStyle = colorToCSS(nc);
      ctx.beginPath();
      ctx.roundRect(r.x, r.y, r.width, r.height, 4);
      ctx.fill();
      const txtC = data.text_color as [number, number, number, number];
      ctx.fillStyle = colorToCSS(txtC);
      ctx.font = `${Math.max(8, data.font_size as number)}px jEngineDefault`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(
        substituteTemplate(data.label as string, {}),
        r.x + r.width / 2,
        r.y + r.height / 2,
        r.width - 8,
      );
    } else if (typeName === "ProgressBar") {
      const bgc = data.bg_color as [number, number, number, number];
      const fgc = data.fg_color as [number, number, number, number];
      ctx.fillStyle = colorToCSS(bgc);
      ctx.fillRect(r.x, r.y, r.width, r.height);
      const ratio = Math.max(
        0,
        Math.min(
          1,
          ((data.value as number) - (data.min as number)) /
            ((data.max as number) - (data.min as number) || 1),
        ),
      );
      ctx.fillStyle = colorToCSS(fgc);
      ctx.fillRect(r.x, r.y, r.width * ratio, r.height);
    } else if (typeName === "Slider") {
      const tc = data.track_color as [number, number, number, number];
      const thc = data.thumb_color as [number, number, number, number];
      const trackH = Math.max(4, r.height * 0.3);
      const trackY = r.y + (r.height - trackH) / 2;
      ctx.fillStyle = colorToCSS(tc);
      ctx.fillRect(r.x, trackY, r.width, trackH);
      const ratio = Math.max(
        0,
        Math.min(
          1,
          ((data.value as number) - (data.min as number)) /
            ((data.max as number) - (data.min as number) || 1),
        ),
      );
      const thumbX = r.x + ratio * (r.width - (data.thumb_size as number));
      ctx.fillStyle = colorToCSS(thc);
      ctx.beginPath();
      ctx.arc(
        thumbX + (data.thumb_size as number) / 2,
        r.y + r.height / 2,
        (data.thumb_size as number) / 2,
        0,
        Math.PI * 2,
      );
      ctx.fill();
    } else if (typeName === "Toggle") {
      const isOn = data.checked as boolean;
      const bgc = isOn
        ? (data.on_color as [number, number, number, number])
        : (data.off_color as [number, number, number, number]);
      ctx.fillStyle = colorToCSS(bgc);
      ctx.beginPath();
      ctx.roundRect(r.x, r.y, r.width, r.height, r.height / 2);
      ctx.fill();
      const thc = data.thumb_color as [number, number, number, number];
      ctx.fillStyle = colorToCSS(thc);
      const thumbR = r.height * 0.4;
      const thumbX = isOn ? r.x + r.width - thumbR - 4 : r.x + thumbR + 4;
      ctx.beginPath();
      ctx.arc(thumbX, r.y + r.height / 2, thumbR, 0, Math.PI * 2);
      ctx.fill();
    } else if (typeName === "TextInput") {
      const bgc = data.bg_color as [number, number, number, number];
      ctx.fillStyle = colorToCSS(bgc);
      ctx.fillRect(r.x, r.y, r.width, r.height);
      ctx.strokeStyle = $themeColors["line-strong"];
      ctx.lineWidth = 1;
      ctx.strokeRect(r.x + 0.5, r.y + 0.5, r.width - 1, r.height - 1);
      const text = (data.text as string) || (data.placeholder as string);
      const tc = (data.text as string)
        ? (data.color as [number, number, number, number])
        : ([0.4, 0.4, 0.5, 1] as [number, number, number, number]);
      ctx.fillStyle = colorToCSS(tc);
      ctx.font = `${Math.max(8, data.font_size as number)}px jEngineDefault`;
      ctx.textAlign = "left";
      ctx.textBaseline = "middle";
      ctx.fillText(text, r.x + 4, r.y + r.height / 2, r.width - 8);
    } else if (typeName === "Circle") {
      const fc = data.fill_color as [number, number, number, number];
      const sc = data.stroke_color as [number, number, number, number];
      const sw = data.stroke_width as number;
      const cx = r.x + r.width / 2;
      const cy = r.y + r.height / 2;
      const rx = r.width / 2;
      const ry = r.height / 2;
      ctx.beginPath();
      ctx.ellipse(cx, cy, Math.max(0, rx), Math.max(0, ry), 0, 0, Math.PI * 2);
      if (fc[3] > 0) {
        ctx.fillStyle = colorToCSS(fc);
        ctx.fill();
      }
      if (sw > 0 && sc[3] > 0) {
        ctx.strokeStyle = colorToCSS(sc);
        ctx.lineWidth = sw;
        ctx.stroke();
      }
    }

    // Border
    if (node.style.border_width > 0 && node.style.border_color[3] > 0) {
      ctx.strokeStyle = colorToCSS(node.style.border_color);
      ctx.lineWidth = node.style.border_width;
      ctx.strokeRect(r.x, r.y, r.width, r.height);
    }

    // Selection highlight + resize handles
    if (isSelected) {
      ctx.strokeStyle = $themeColors.accent;
      ctx.lineWidth = 2;
      ctx.setLineDash([4, 4]);
      ctx.strokeRect(r.x - 1, r.y - 1, r.width + 2, r.height + 2);
      ctx.setLineDash([]);

      // Draw 8 resize handles
      const hs = 6; // handle size
      const hh = hs / 2;
      ctx.fillStyle = $themeColors.accent;
      const handles = getResizeHandlePositions(r);
      for (const h of handles) {
        ctx.fillRect(h.x - hh, h.y - hh, hs, hs);
      }

      // Draw constraint pin lines to parent
      if (parentId && !constraintsIsEmpty(node.style.constraints)) {
        const pr = layoutMap.get(parentId);
        if (pr) {
          const cons = node.style.constraints!;
          ctx.strokeStyle = withAlpha($themeColors.accent, 0.4);
          ctx.lineWidth = 1;
          ctx.setLineDash([3, 3]);
          ctx.font = "9px jEngineDefault";
          ctx.fillStyle = withAlpha($themeColors.accent, 0.7);
          ctx.textAlign = "center";
          ctx.textBaseline = "middle";
          // Left pin
          if (cons.left) {
            const lx1 = pr.x;
            const lx2 = r.x;
            const ly = r.y + r.height / 2;
            ctx.beginPath();
            ctx.moveTo(lx1, ly);
            ctx.lineTo(lx2, ly);
            ctx.stroke();
            ctx.fillText(
              cons.left.type === "Px"
                ? `${cons.left.value}px`
                : `${cons.left.value}%`,
              (lx1 + lx2) / 2,
              ly - 8,
            );
          }
          // Right pin
          if (cons.right) {
            const rx1 = r.x + r.width;
            const rx2 = pr.x + pr.width;
            const ry = r.y + r.height / 2;
            ctx.beginPath();
            ctx.moveTo(rx1, ry);
            ctx.lineTo(rx2, ry);
            ctx.stroke();
            ctx.fillText(
              cons.right.type === "Px"
                ? `${cons.right.value}px`
                : `${cons.right.value}%`,
              (rx1 + rx2) / 2,
              ry - 8,
            );
          }
          // Top pin
          if (cons.top) {
            const tx = r.x + r.width / 2;
            const ty1 = pr.y;
            const ty2 = r.y;
            ctx.beginPath();
            ctx.moveTo(tx, ty1);
            ctx.lineTo(tx, ty2);
            ctx.stroke();
            ctx.fillText(
              cons.top.type === "Px"
                ? `${cons.top.value}px`
                : `${cons.top.value}%`,
              tx + 14,
              (ty1 + ty2) / 2,
            );
          }
          // Bottom pin
          if (cons.bottom) {
            const bx = r.x + r.width / 2;
            const by1 = r.y + r.height;
            const by2 = pr.y + pr.height;
            ctx.beginPath();
            ctx.moveTo(bx, by1);
            ctx.lineTo(bx, by2);
            ctx.stroke();
            ctx.fillText(
              cons.bottom.type === "Px"
                ? `${cons.bottom.value}px`
                : `${cons.bottom.value}%`,
              bx + 14,
              (by1 + by2) / 2,
            );
          }
          ctx.setLineDash([]);
        }
      }
    }

    // Tab-order badge for focusable nodes
    if (node.style.focusable) {
      const badgeText = String(node.style.tab_order ?? 0);
      ctx.font = "bold 9px jEngineDefault";
      const tw = ctx.measureText(badgeText).width;
      const bw = tw + 6;
      const bh = 14;
      const bx = r.x + r.width - bw;
      const by = r.y - bh + 2;
      ctx.fillStyle = $themeColors.a3;
      ctx.beginPath();
      ctx.roundRect(bx, by, bw, bh, 3);
      ctx.fill();
      ctx.fillStyle = $themeColors["on-accent"];
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(badgeText, bx + bw / 2, by + bh / 2);
    }

    // Recurse children, clipped to this node's content box when it is a
    // Hidden/Scroll container so the preview matches what the runtime draws.
    const clip = clipRects.get(node.id);
    if (clip) {
      ctx.save();
      ctx.beginPath();
      ctx.rect(clip.x, clip.y, clip.width, clip.height);
      ctx.clip();
    }
    for (const child of node.children) {
      renderNode(ctx, child, layoutMap, node.id);
    }
    if (clip) {
      ctx.restore();
    }
  }

  // Redraw when document or selection changes
  $effect(() => {
    void $guiDocument;
    void $selectedNodeId;
    void $devicePreview;
    void $themeColors;
    requestAnimationFrame(renderCanvas);
  });

  // ResizeObserver for canvas
  $effect(() => {
    if (!canvasEl) return;
    const obs = new ResizeObserver(() => requestAnimationFrame(renderCanvas));
    obs.observe(canvasEl.parentElement!);
    return () => obs.disconnect();
  });

  // ---------------------------------------------------------------------------
  // Canvas interaction — click-to-select, drag-to-move, resize handles
  // ---------------------------------------------------------------------------
  /// Deepest node whose *visible* rect contains the point. A node clipped away
  /// by an Overflow container is not pickable, matching the runtime hit-test
  /// (`LayoutEntry::contains` in crates/jengine-core/src/ui/layout.rs).
  function pickNodeAt(
    root: GuiNode,
    layoutMap: Map<string, LayoutRect>,
    mx: number,
    my: number,
  ): string | null {
    let hit: string | null = null;
    const walk = (node: GuiNode, clip: LayoutRect | null) => {
      const r = layoutMap.get(node.id);
      if (!r) return;
      const inRect =
        mx >= r.x && mx <= r.x + r.width && my >= r.y && my <= r.y + r.height;
      const inClip =
        !clip ||
        (mx >= clip.x &&
          mx <= clip.x + clip.width &&
          my >= clip.y &&
          my <= clip.y + clip.height);
      if (inRect && inClip) hit = node.id;
      const own = clipRects.get(node.id);
      const childClip = own
        ? clip
          ? {
              x: Math.max(clip.x, own.x),
              y: Math.max(clip.y, own.y),
              width:
                Math.min(clip.x + clip.width, own.x + own.width) -
                Math.max(clip.x, own.x),
              height:
                Math.min(clip.y + clip.height, own.y + own.height) -
                Math.max(clip.y, own.y),
            }
          : own
        : clip;
      for (const child of node.children) walk(child, childClip);
    };
    walk(root, null);
    return hit;
  }

  /// Wheel over an Overflow: Scroll container scrolls it, the same way the
  /// engine does at runtime. This is a preview: it moves `previewScroll`, not
  /// the document, so the file stays clean - the authored starting offset is
  /// the inspector's Scroll Y field.
  function handleCanvasWheel(e: WheelEvent) {
    if (!canvasEl || !$guiDocument) return;
    const rect = canvasEl.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const layoutMap = computeLayout($guiDocument, canvasW, canvasH);

    // Deepest scroll container under the cursor wins, matching the runtime.
    let target: string | null = null;
    for (const [id, range] of scrollRanges) {
      if (range.x <= 0 && range.y <= 0) continue;
      const r = layoutMap.get(id);
      if (!r) continue;
      if (mx >= r.x && mx <= r.x + r.width && my >= r.y && my <= r.y + r.height) {
        target = id;
      }
    }
    if (!target) return;

    e.preventDefault();
    const range = scrollRanges.get(target)!;
    const node = findNode($guiDocument.root, target);
    const step = (e.deltaY > 0 ? 1 : -1) * WHEEL_SCROLL_STEP;
    const prev = previewScroll.get(target) ?? { x: 0, y: 0 };
    // The preview is an offset on top of the authored value, so it is clamped
    // against the range minus that value.
    const next = { ...prev };
    if (range.y > 0) {
      const base = node?.style.scroll_y ?? 0;
      next.y = clamp(base + prev.y + step, 0, range.y) - base;
    } else {
      const base = node?.style.scroll_x ?? 0;
      next.x = clamp(base + prev.x + step, 0, range.x) - base;
    }
    previewScroll.set(target, next);
    requestAnimationFrame(renderCanvas);
  }

  function handleCanvasMouseDown(e: MouseEvent) {
    if (!canvasEl || !$guiDocument) return;
    const rect = canvasEl.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const layoutMap = computeLayout($guiDocument, canvasW, canvasH);

    // Check resize handles first (if a node is selected)
    if ($selectedNodeId) {
      const selRect = layoutMap.get($selectedNodeId);
      if (selRect) {
        const handles = getResizeHandlePositions(selRect);
        const hitMargin = 5;
        for (const h of handles) {
          if (
            Math.abs(mx - h.x) <= hitMargin &&
            Math.abs(my - h.y) <= hitMargin
          ) {
            resizing = true;
            resizeHandle = h.name;
            resizeStartRect = { ...selRect };
            resizeStartMouse = { x: mx, y: my };
            return;
          }
        }
      }
    }

    // Find deepest node containing click point (clipped nodes are not pickable)
    const hit = pickNodeAt($guiDocument.root, layoutMap, mx, my);
    selectedNodeId.set(hit);

    // Start drag if we hit a non-root node
    if (hit && hit !== "root") {
      const hitNode = findNode($guiDocument.root, hit);
      if (hitNode) {
        dragging = true;
        dragNodeId = hit;
        dragStartMouse = { x: mx, y: my };
        dragStartOffset = {
          x: hitNode.style.offset_x || 0,
          y: hitNode.style.offset_y || 0,
        };
        const nodeRect = layoutMap.get(hit);
        dragStartLayoutRect = nodeRect
          ? { ...nodeRect }
          : { x: 0, y: 0, width: 0, height: 0 };
      }
    }
  }

  function handleCanvasMouseMove(e: MouseEvent) {
    if (!canvasEl) return;
    const rect = canvasEl.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;

    // Resize in progress
    if (resizing && resizeHandle && $selectedNodeId) {
      const dx = mx - resizeStartMouse.x;
      const dy = my - resizeStartMouse.y;
      const doc = structuredClone($guiDocument);
      if (!doc) return;
      const node = findNode(doc.root, $selectedNodeId);
      if (!node) return;

      const isConstrained = !constraintsIsEmpty(node.style.constraints);
      const minSize = 10;

      if (isConstrained) {
        // For constrained nodes, adjust constraint values
        const c = node.style.constraints ?? {};
        if (resizeHandle.includes("e") && c.right) {
          c.right = {
            type: c.right.type,
            value: Math.max(
              0,
              c.right.type === "Px" ? c.right.value - dx : c.right.value,
            ),
          };
        }
        if (resizeHandle.includes("w") && c.left) {
          c.left = {
            type: c.left.type,
            value: Math.max(
              0,
              c.left.type === "Px" ? c.left.value + dx : c.left.value,
            ),
          };
        }
        if (resizeHandle.includes("s") && c.bottom) {
          c.bottom = {
            type: c.bottom.type,
            value: Math.max(
              0,
              c.bottom.type === "Px" ? c.bottom.value - dy : c.bottom.value,
            ),
          };
        }
        if (resizeHandle.includes("n") && c.top) {
          c.top = {
            type: c.top.type,
            value: Math.max(
              0,
              c.top.type === "Px" ? c.top.value + dy : c.top.value,
            ),
          };
        }
        // If only one edge is pinned, adjust width/height
        if (resizeHandle.includes("e") && !c.right) {
          const newW = Math.max(minSize, resizeStartRect.width + dx);
          node.style.width = { type: "Fixed", value: Math.round(newW) };
        }
        if (resizeHandle.includes("w") && !c.left) {
          const newW = Math.max(minSize, resizeStartRect.width - dx);
          node.style.width = { type: "Fixed", value: Math.round(newW) };
        }
        if (resizeHandle.includes("s") && !c.bottom) {
          const newH = Math.max(minSize, resizeStartRect.height + dy);
          node.style.height = { type: "Fixed", value: Math.round(newH) };
        }
        if (resizeHandle.includes("n") && !c.top) {
          const newH = Math.max(minSize, resizeStartRect.height - dy);
          node.style.height = { type: "Fixed", value: Math.round(newH) };
        }
        node.style.constraints = c;
      } else {
        // For unconstrained nodes, adjust width/height + offset (with grid snap)
        if (resizeHandle.includes("e")) {
          let newW = Math.max(minSize, resizeStartRect.width + dx);
          if (snapEnabled) newW = Math.max(minSize, gridSnap(newW));
          node.style.width = { type: "Fixed", value: Math.round(newW) };
        }
        if (resizeHandle.includes("w")) {
          let newW = Math.max(minSize, resizeStartRect.width - dx);
          if (snapEnabled) newW = Math.max(minSize, gridSnap(newW));
          node.style.width = { type: "Fixed", value: Math.round(newW) };
          node.style.offset_x =
            (node.style.offset_x || 0) +
            (resizeStartRect.width - Math.round(newW));
        }
        if (resizeHandle.includes("s")) {
          let newH = Math.max(minSize, resizeStartRect.height + dy);
          if (snapEnabled) newH = Math.max(minSize, gridSnap(newH));
          node.style.height = { type: "Fixed", value: Math.round(newH) };
        }
        if (resizeHandle.includes("n")) {
          let newH = Math.max(minSize, resizeStartRect.height - dy);
          if (snapEnabled) newH = Math.max(minSize, gridSnap(newH));
          node.style.height = { type: "Fixed", value: Math.round(newH) };
          node.style.offset_y =
            (node.style.offset_y || 0) +
            (resizeStartRect.height - Math.round(newH));
        }
      }
      guiDocument.set(doc);
      return;
    }

    // Drag-to-move in progress
    if (!dragging || !dragNodeId) return;
    const dx = mx - dragStartMouse.x;
    const dy = my - dragStartMouse.y;

    const doc = structuredClone($guiDocument);
    if (!doc) return;
    const node = findNode(doc.root, dragNodeId);
    if (!node) return;

    let newOffsetX = dragStartOffset.x + dx;
    let newOffsetY = dragStartOffset.y + dy;

    // Grid snap
    if (snapEnabled) {
      newOffsetX = gridSnap(newOffsetX);
      newOffsetY = gridSnap(newOffsetY);
    }

    // Alignment guides — check against siblings + parent rect
    const candidateRect: LayoutRect = {
      x: dragStartLayoutRect.x + (newOffsetX - dragStartOffset.x),
      y: dragStartLayoutRect.y + (newOffsetY - dragStartOffset.y),
      width: dragStartLayoutRect.width,
      height: dragStartLayoutRect.height,
    };
    const draftLayoutMap = computeLayout(doc, canvasW, canvasH);
    const siblingRects: LayoutRect[] = [];
    const parentNode = findParentNode(doc.root, dragNodeId);
    if (parentNode) {
      for (const child of parentNode.children) {
        if (child.id !== dragNodeId) {
          const sr = draftLayoutMap.get(child.id);
          if (sr) siblingRects.push(sr);
        }
      }
      const pr = draftLayoutMap.get(parentNode.id);
      if (pr) siblingRects.push(pr);
    }
    const alignment = computeAlignmentGuides(candidateRect, siblingRects, 5);
    newOffsetX += alignment.adjustX;
    newOffsetY += alignment.adjustY;
    activeGuides = alignment.guides;

    node.style.offset_x = Math.round(newOffsetX);
    node.style.offset_y = Math.round(newOffsetY);
    guiDocument.set(doc);
  }

  function handleCanvasMouseUp(_e: MouseEvent) {
    // Handle resize commit
    if (resizing && resizeHandle && $selectedNodeId) {
      const nodeId = $selectedNodeId;
      const doc = $guiDocument;
      if (doc) {
        const currentNode = findNode(doc.root, nodeId);
        if (currentNode) {
          // Capture final state
          const finalStyle = structuredClone(currentNode.style);
          // Restore to pre-resize state and use executeGui for undo
          const restored = structuredClone(doc);
          const rn = findNode(restored.root, nodeId);
          if (rn) {
            // We don't have the pre-resize style stored perfectly, but resizeStartRect
            // gives us the original dimensions. For simplicity, use executeGui with the final.
            guiDocument.set(restored);
          }
          executeGui("Resize node", (d) => {
            const n = findNode(d.root, nodeId);
            if (n) {
              n.style.width = finalStyle.width;
              n.style.height = finalStyle.height;
              n.style.offset_x = finalStyle.offset_x;
              n.style.offset_y = finalStyle.offset_y;
              n.style.constraints = finalStyle.constraints;
            }
          });
        }
      }
      resizing = false;
      resizeHandle = null;
      activeGuides = [];
      return;
    }

    if (!dragging || !dragNodeId) {
      dragging = false;
      activeGuides = [];
      return;
    }
    // Commit the final offset via executeGui so it's undoable
    const doc = $guiDocument;
    const finalNode = doc ? findNode(doc.root, dragNodeId) : null;
    if (finalNode) {
      const finalX = finalNode.style.offset_x;
      const finalY = finalNode.style.offset_y;
      // Only push undo if the offset actually changed
      if (finalX !== dragStartOffset.x || finalY !== dragStartOffset.y) {
        // Restore original offset so executeGui snapshots the pre-drag state
        const nodeId = dragNodeId;
        const origX = dragStartOffset.x;
        const origY = dragStartOffset.y;
        const restored = structuredClone(doc);
        const rn = findNode(restored.root, nodeId);
        if (rn) {
          rn.style.offset_x = origX;
          rn.style.offset_y = origY;
        }
        guiDocument.set(restored);
        executeGui("Move node", (d) => {
          const n = findNode(d.root, nodeId);
          if (n) {
            n.style.offset_x = finalX;
            n.style.offset_y = finalY;
          }
        });
      }
    }
    dragging = false;
    dragNodeId = null;
    activeGuides = [];
  }

  // ---------------------------------------------------------------------------
  // Widget tree helpers
  // ---------------------------------------------------------------------------
  function nodeIcon(typeName: string): string {
    switch (typeName) {
      case "Panel":
        return "◻";
      case "Text":
        return "T";
      case "Image":
        return "🖼";
      case "Button":
        return "▣";
      case "ProgressBar":
        return "▰";
      case "Slider":
        return "⊶";
      case "Toggle":
        return "⊘";
      case "TextInput":
        return "⌨";
      case "Circle":
        return "○";
      default:
        return "?";
    }
  }

  function startRename(nodeId: string) {
    renamingId = nodeId;
    renameValue = nodeId;
  }

  function submitRename() {
    if (renamingId && renameValue.trim()) {
      renameNode(renamingId, renameValue.trim());
    }
    renamingId = null;
  }

  function handleNodeContext(e: MouseEvent, nodeId: string) {
    e.preventDefault();
    e.stopPropagation();
    contextMenu = { x: e.clientX, y: e.clientY, nodeId };
  }

  function handleCanvasContext(e: MouseEvent) {
    e.preventDefault();
    if (!canvasEl || !$guiDocument) return;
    const rect = canvasEl.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const layoutMap = computeLayout($guiDocument, canvasW, canvasH);
    const hit = pickNodeAt($guiDocument.root, layoutMap, mx, my) ?? "root";
    contextMenu = { x: e.clientX, y: e.clientY, nodeId: hit };
  }

  function closeContextMenu() {
    contextMenu = null;
  }

  // ---------------------------------------------------------------------------
  // Tree drag-and-drop (reparent nodes)
  // ---------------------------------------------------------------------------
  function handleTreeDragStart(e: DragEvent, nodeId: string) {
    if (nodeId === "root") {
      e.preventDefault();
      return;
    }
    treeDragId = nodeId;
    e.dataTransfer!.effectAllowed = "move";
    e.dataTransfer!.setData("text/plain", nodeId);
  }

  function handleTreeDragOver(e: DragEvent, nodeId: string) {
    if (!treeDragId || treeDragId === nodeId) return;
    // Don't allow dropping onto a descendant of the dragged node
    if ($guiDocument) {
      const draggedNode = findNode($guiDocument.root, treeDragId);
      if (draggedNode && findNode(draggedNode, nodeId)) return;
    }
    e.preventDefault();
    e.dataTransfer!.dropEffect = "move";
    treeDropTarget = nodeId;
  }

  function handleTreeDragLeave(_e: DragEvent, nodeId: string) {
    if (treeDropTarget === nodeId) treeDropTarget = null;
  }

  function handleTreeDrop(e: DragEvent, nodeId: string) {
    e.preventDefault();
    if (treeDragId && treeDragId !== nodeId) {
      moveNode(treeDragId, nodeId, 0);
    }
    treeDragId = null;
    treeDropTarget = null;
  }

  function handleTreeDragEnd() {
    treeDragId = null;
    treeDropTarget = null;
  }

  // ---------------------------------------------------------------------------
  // Keyboard shortcuts
  // ---------------------------------------------------------------------------
  function handleKeydown(e: KeyboardEvent) {
    if (!$guiDocument) return;
    if (e.ctrlKey && e.key === "s") {
      e.preventDefault();
      saveCurrentGui();
    } else if (
      e.key === "Delete" &&
      $selectedNodeId &&
      $selectedNodeId !== "root"
    ) {
      deleteNode($selectedNodeId);
    } else if (e.ctrlKey && e.key === "d" && $selectedNodeId) {
      e.preventDefault();
      duplicateNode($selectedNodeId);
    }
  }

  // ---------------------------------------------------------------------------
  // Widget types (for add‐child menu)
  // ---------------------------------------------------------------------------

    // Ctrl+Z / Ctrl+Y are this editor's while it is open; the app's key
    // handler asks `undoTarget()` before undoing the scene, so one press
    // no longer undoes in both.
    $effect(() => claimUndo({ undo, redo }));
</script>

<svelte:window onkeydown={handleKeydown} />

{#if !$guiDocument}
  <div class="gui-editor empty-state">
    <div class="empty-msg">No GUI file open</div>
    <div class="empty-hint">Double-click a .gui file in Assets to edit it</div>
  </div>
{:else}
  <div class="gui-editor">
    <!-- Toolbar -->
    <div class="ge-toolbar">
      <span class="ge-title">{$guiDocument.name}{$guiDirty ? " •" : ""}</span>
      <div class="ge-toolbar-actions">
        <DevicePicker />
        <button onclick={undo} disabled={!$canUndo} title="Undo (Ctrl+Z)"
          >↩</button
        >
        <button onclick={redo} disabled={!$canRedo} title="Redo (Ctrl+Y)"
          >↪</button
        >
        <span class="separator"></span>
        {#each WIDGET_TYPES as wt}
          <button
            class="widget-add-btn"
            title="Add {wt}"
            onclick={() => addChild($selectedNodeId || "root", wt)}
            >{nodeIcon(wt)}</button
          >
        {/each}
        <span class="separator"></span>
        <button
          class:active={showGrid}
          onclick={() => (showGrid = !showGrid)}
          title="Toggle Grid">⊞</button
        >
        <button
          class:active={snapEnabled}
          onclick={() => (snapEnabled = !snapEnabled)}
          title="Toggle Snap">⊟</button
        >
        <span class="separator"></span>
        <button onclick={saveCurrentGui} title="Save (Ctrl+S)">💾</button>
        <button onclick={() => requestEditorClose("gui")} title="Close">✕</button>
      </div>
    </div>

    <div class="ge-body">
      <!-- Left: Widget Tree -->
      <div class="ge-tree-panel">
        <div class="ge-panel-header">Widget Tree</div>
        <div class="ge-tree-scroll">
          {#snippet renderTree(node: GuiNode, depth: number)}
            <div
              class="tree-node"
              class:selected={$selectedNodeId === node.id}
              class:drop-target={treeDropTarget === node.id}
              style="padding-left: {8 + depth * 16}px"
              draggable={node.id !== "root"}
              role="treeitem"
              tabindex="0"
              aria-selected={$selectedNodeId === node.id}
              onclick={() => selectedNodeId.set(node.id)}
              onkeydown={(e) => {
                if (e.key === "Enter" || e.key === " ")
                  selectedNodeId.set(node.id);
              }}
              ondblclick={() => startRename(node.id)}
              oncontextmenu={(e) => handleNodeContext(e, node.id)}
              ondragstart={(e) => handleTreeDragStart(e, node.id)}
              ondragover={(e) => handleTreeDragOver(e, node.id)}
              ondragleave={(e) => handleTreeDragLeave(e, node.id)}
              ondrop={(e) => handleTreeDrop(e, node.id)}
              ondragend={handleTreeDragEnd}
            >
              <span class="node-icon"
                >{nodeIcon(getNodeTypeName(node.node_type))}</span
              >
              {#if renamingId === node.id}
                <input
                  class="rename-input"
                  type="text"
                  bind:value={renameValue}
                  onblur={submitRename}
                  onkeydown={(e) => {
                    if (e.key === "Enter") submitRename();
                    else if (e.key === "Escape") {
                      renamingId = null;
                    }
                  }}
                />
              {:else}
                <span class="node-label">{node.id}</span>
              {/if}
              <span class="node-type-hint"
                >{getNodeTypeName(node.node_type)}</span
              >
            </div>
            {#each node.children as child}
              {@render renderTree(child, depth + 1)}
            {/each}
          {/snippet}
          {@render renderTree($guiDocument.root, 0)}
        </div>
      </div>

      <!-- Center: Canvas Preview -->
      <div class="ge-canvas-area">
        {#if previewProfile && deviceRect}
          <DeviceOverlay
            profile={previewProfile}
            portrait={$devicePreview.portrait}
            x={deviceRect.x}
            y={deviceRect.y}
            width={deviceRect.width}
            height={deviceRect.height}
          />
        {/if}
        <canvas
          bind:this={canvasEl}
          class="ge-canvas"
          class:dragging
          class:resizing-nw={resizing && resizeHandle === "nw"}
          class:resizing-ne={resizing && resizeHandle === "ne"}
          class:resizing-sw={resizing && resizeHandle === "sw"}
          class:resizing-se={resizing && resizeHandle === "se"}
          class:resizing-n={resizing && resizeHandle === "n"}
          class:resizing-s={resizing && resizeHandle === "s"}
          class:resizing-e={resizing && resizeHandle === "e"}
          class:resizing-w={resizing && resizeHandle === "w"}
          onmousedown={handleCanvasMouseDown}
          onmousemove={handleCanvasMouseMove}
          onmouseup={handleCanvasMouseUp}
          onmouseleave={handleCanvasMouseUp}
          oncontextmenu={handleCanvasContext}
          onwheel={handleCanvasWheel}
        ></canvas>
      </div>
    </div>
  </div>
{/if}

<!-- Context Menu -->
{#if contextMenu}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="context-overlay"
    onclick={closeContextMenu}
    oncontextmenu={(e) => {
      e.preventDefault();
      closeContextMenu();
    }}
  >
    <div
      class="context-menu"
      style="left: {contextMenu.x}px; top: {contextMenu.y}px"
      onclick={(e) => e.stopPropagation()}
    >
      {#each WIDGET_TYPES as wt}
        <button
          class="menu-item"
          onclick={() => {
            addChild(contextMenu?.nodeId ?? "root", wt);
            closeContextMenu();
          }}>Add {wt}</button
        >
      {/each}
      {#if contextMenu.nodeId !== "root"}
        <div class="menu-divider"></div>
        <button
          class="menu-item"
          onclick={() => {
            duplicateNode(contextMenu?.nodeId ?? "");
            closeContextMenu();
          }}>Duplicate</button
        >
        <button
          class="menu-item danger"
          onclick={() => {
            deleteNode(contextMenu?.nodeId ?? "");
            closeContextMenu();
          }}>Delete</button
        >
      {/if}
    </div>
  </div>
{/if}

<style>
  .gui-editor {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--ed-base);
    color: var(--ed-text);
    font-size: 13px;
  }

  .gui-editor.empty-state {
    align-items: center;
    justify-content: center;
  }

  .empty-msg {
    font-size: 14px;
    color: var(--ed-muted);
  }

  .empty-hint {
    font-size: 12px;
    color: var(--ed-faint);
    margin-top: 4px;
  }

  /* Toolbar */
  .ge-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 8px;
    background: var(--ed-mantle);
    border-bottom: 1px solid var(--ed-line);
    flex-shrink: 0;
    height: 36px;
  }

  .ge-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--ed-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ge-toolbar-actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .ge-toolbar-actions button {
    background: none;
    border: 1px solid transparent;
    color: var(--ed-text);
    cursor: pointer;
    font-size: 12.5px;
    padding: 3px 6px;
    border-radius: 4px;
    line-height: 1;
  }

  .ge-toolbar-actions button:hover:not(:disabled) {
    background: var(--ed-raised);
  }

  .ge-toolbar-actions button:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .ge-toolbar-actions button.active {
    background: var(--ed-raised);
    border-color: var(--ed-accent);
    color: var(--ed-accent);
  }

  .widget-add-btn {
    font-size: 14px !important;
    width: 28px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .separator {
    width: 1px;
    height: 20px;
    background: var(--ed-raised);
    margin: 0 4px;
  }

  /* Body layout */
  .ge-body {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  /* Tree panel (left) */
  .ge-tree-panel {
    width: 200px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--ed-line);
  }

  .ge-panel-header {
    padding: 6px 10px;
    font-family: var(--ed-mono);
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ed-muted);
    background: var(--ed-mantle);
    border-bottom: 1px solid var(--ed-line);
  }

  .ge-tree-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 4px 0;
  }

  .tree-node {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    cursor: pointer;
    user-select: none;
    padding-right: 8px;
    color: var(--ed-text);
  }

  .tree-node:hover {
    background: var(--ed-raised);
  }

  .tree-node.selected {
    background: var(--ed-line);
  }

  .tree-node.drop-target {
    background: var(--ed-raised);
    outline: 1px dashed var(--ed-accent);
    outline-offset: -1px;
  }

  .node-icon {
    font-size: 11px;
    flex-shrink: 0;
    width: 16px;
    text-align: center;
    color: var(--ed-accent);
  }

  .node-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }

  .node-type-hint {
    margin-left: auto;
    font-size: 10px;
    color: var(--ed-faint);
    flex-shrink: 0;
  }

  .rename-input {
    background: var(--ed-raised);
    border: 1px solid var(--ed-accent);
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 12px;
    padding: 1px 4px;
    flex: 1;
    min-width: 0;
    font-family: inherit;
  }

  /* Canvas area (center) */
  .ge-canvas-area {
    flex: 1;
    min-width: 0;
    position: relative;
    background: var(--ed-crust);
  }

  .ge-canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    cursor: crosshair;
  }

  .ge-canvas.dragging {
    cursor: grabbing;
  }

  .ge-canvas.resizing-nw,
  .ge-canvas.resizing-se {
    cursor: nwse-resize;
  }
  .ge-canvas.resizing-ne,
  .ge-canvas.resizing-sw {
    cursor: nesw-resize;
  }
  .ge-canvas.resizing-n,
  .ge-canvas.resizing-s {
    cursor: ns-resize;
  }
  .ge-canvas.resizing-e,
  .ge-canvas.resizing-w {
    cursor: ew-resize;
  }

  /* Context menu */
  .context-overlay {
    position: fixed;
    inset: 0;
    z-index: 999;
  }

  .context-menu {
    position: fixed;
    z-index: 1000;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 7px;
    padding: 5px;
    min-width: 160px;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
  }

  .menu-item {
    display: block;
    width: 100%;
    text-align: left;
    padding: 4px 12px;
    background: none;
    border: none;
    color: var(--ed-text);
    font-size: 12px;
    cursor: pointer;
    font-family: inherit;
    border-radius: 4px;
  }

  .menu-item:hover {
    background: var(--ed-accent-low);
    color: var(--ed-ink);
  }

  .menu-item.danger {
    color: var(--ed-err);
    border-radius: 4px;
  }

  .menu-divider {
    height: 1px;
    background: var(--ed-line);
    margin: 4px 0;
  }
</style>
