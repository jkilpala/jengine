<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import {
    addCommentAt,
    addControlEdge,
    addDataBinding,
    addNodeAt,
    behaviorFilePath,
    behaviorReport,
    commitCommentRect,
    commitSetNodePositions,
    deleteComment,
    ensureOpCatalog,
    focusNodeRequest,
    opCatalog,
    removeControlEdge,
    removeDataBinding,
    requestCenterDrop,
    selectedNodeIds,
    updateCommentText,
  } from "./stores/behaviorEditorStore";
  import { project } from "./stores/projectStore";
  import { openInCodeEditor } from "./api/project";
  import { logError, logInfo } from "./stores/consoleStore";
  import {
    buildCatalogIndex,
    categoryColor,
    CONTROL_EDGE_COLOR,
    EXEC_PIN_RADIUS,
    execInPinY,
    execOutIndex,
    execOutPinY,
    findOutputPin,
    inputPinIndex,
    inputPinY,
    isReferenceBinding,
    layoutNodes,
    NODE_HEADER_HEIGHT,
    NODE_PIN_HEIGHT,
    NODE_WIDTH,
    nodeHeight,
    outputPinY,
    PALETTE_DND_MIME,
    parseBehaviorDoc,
    pinsCompatible,
    pinTypeColor,
    type BehaviorDoc,
    type BehaviorComment,
    type BehaviorPosition,
  } from "./behavior/layout";
  import type { OpDescriptor } from "./api/behavior";

  interface Props {
    json: string;
  }
  let { json }: Props = $props();

  // Pan/zoom view state.
  let panX = $state(0);
  let panY = $state(0);
  let zoom = $state(1);
  let svgEl: SVGSVGElement | undefined = $state();

  // ─── Slice E.10a: snap-to-grid ───────────────────────────────
  // 10 world-unit grid per spec §1.6. Default-on, per-tab toggle.
  // Snap is applied at write boundaries (drop, drag commit) — the
  // visual grid is just a hint; node positions stored on disk are
  // already snapped when the toggle is active.
  const GRID_SIZE = 10;
  let snapEnabled = $state(true);
  function snap(v: number): number {
    return snapEnabled ? Math.round(v / GRID_SIZE) * GRID_SIZE : v;
  }

  onMount(() => {
    void ensureOpCatalog();
  });

  let doc = $derived<BehaviorDoc | null>(parseBehaviorDoc(json));
  let catalog = $derived<OpDescriptor[]>($opCatalog ?? []);
  let catIndex = $derived(buildCatalogIndex(catalog));
  let positions = $derived(doc ? layoutNodes(doc) : {});

  interface DrawnNode {
    id: string;
    op: string;
    descriptor: OpDescriptor | undefined;
    x: number;
    y: number;
    width: number;
    height: number;
    headerColor: string;
  }

  let drawnNodes = $derived<DrawnNode[]>(
    (doc?.nodes ?? []).map((n) => {
      const desc = catIndex.byOp.get(n.op);
      const inCount = desc?.inputs.length ?? 0;
      const outCount = desc?.outputs.length ?? 0;
      const pos = positions[n.id] ?? { x: 0, y: 0 };
      return {
        id: n.id,
        op: n.op,
        descriptor: desc,
        x: pos.x,
        y: pos.y,
        width: NODE_WIDTH,
        height: nodeHeight(inCount, outCount),
        headerColor: desc ? categoryColor(desc.category) : "var(--ed-faint)",
      };
    }),
  );

  function nodeById(id: string): DrawnNode | undefined {
    return drawnNodes.find((n) => n.id === id);
  }

  // ─── Slice E.10e: comment groups ─────────────────────────────
  // Comments live in `editor.comments` and are rendered as an HTML
  // overlay above the SVG canvas. Keeping them out of SVG avoids
  // foreignObject quirks (text-edit input lag, mobile keyboard
  // weirdness) and lets the existing pan/zoom math just feed
  // CSS transforms.
  let docComments = $derived<BehaviorComment[]>(
    (doc?.editor?.comments as BehaviorComment[] | undefined) ?? [],
  );
  // Live drag/resize delta for the comment currently under the
  // pointer. `null` when no interaction is active. Keeps the
  // disk-stored rect untouched until pointerup so undo gets one
  // entry per gesture.
  let commentDrag = $state<
    | null
    | {
        id: string;
        kind: "move" | "resize";
        startClientX: number;
        startClientY: number;
        origin: { x: number; y: number; w: number; h: number };
      }
  >(null);
  let commentDelta = $state<{ dx: number; dy: number; dw: number; dh: number }>({
    dx: 0,
    dy: 0,
    dw: 0,
    dh: 0,
  });

  function liveCommentRect(c: BehaviorComment): {
    x: number;
    y: number;
    w: number;
    h: number;
  } {
    if (!commentDrag || commentDrag.id !== c.id) {
      return { x: c.x, y: c.y, w: c.w, h: c.h };
    }
    if (commentDrag.kind === "move") {
      return {
        x: c.x + commentDelta.dx,
        y: c.y + commentDelta.dy,
        w: c.w,
        h: c.h,
      };
    }
    return {
      x: c.x,
      y: c.y,
      w: Math.max(80, c.w + commentDelta.dw),
      h: Math.max(60, c.h + commentDelta.dh),
    };
  }

  function startCommentDrag(
    e: PointerEvent,
    c: BehaviorComment,
    kind: "move" | "resize",
  ) {
    e.stopPropagation();
    e.preventDefault();
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    commentDrag = {
      id: c.id,
      kind,
      startClientX: e.clientX,
      startClientY: e.clientY,
      origin: { x: c.x, y: c.y, w: c.w, h: c.h },
    };
    commentDelta = { dx: 0, dy: 0, dw: 0, dh: 0 };
  }

  function onCommentPointerMove(e: PointerEvent) {
    if (!commentDrag) return;
    const dxScreen = e.clientX - commentDrag.startClientX;
    const dyScreen = e.clientY - commentDrag.startClientY;
    const dxWorld = dxScreen / zoom;
    const dyWorld = dyScreen / zoom;
    if (commentDrag.kind === "move") {
      commentDelta = {
        dx: snap(commentDrag.origin.x + dxWorld) - commentDrag.origin.x,
        dy: snap(commentDrag.origin.y + dyWorld) - commentDrag.origin.y,
        dw: 0,
        dh: 0,
      };
    } else {
      commentDelta = {
        dx: 0,
        dy: 0,
        dw: snap(commentDrag.origin.w + dxWorld) - commentDrag.origin.w,
        dh: snap(commentDrag.origin.h + dyWorld) - commentDrag.origin.h,
      };
    }
  }

  function onCommentPointerUp(e: PointerEvent) {
    if (!commentDrag) return;
    (e.currentTarget as Element).releasePointerCapture?.(e.pointerId);
    const drag = commentDrag;
    const delta = commentDelta;
    commentDrag = null;
    commentDelta = { dx: 0, dy: 0, dw: 0, dh: 0 };
    const moved = delta.dx !== 0 || delta.dy !== 0;
    const resized = delta.dw !== 0 || delta.dh !== 0;
    if (!moved && !resized) return;
    commitCommentRect(drag.id, {
      x: drag.origin.x + delta.dx,
      y: drag.origin.y + delta.dy,
      w: drag.origin.w + delta.dw,
      h: drag.origin.h + delta.dh,
    });
  }

  function onCommentTextInput(id: string, ev: Event) {
    const target = ev.currentTarget as HTMLTextAreaElement;
    updateCommentText(id, target.value);
  }

  function addCommentAtCenter() {
    if (!svgEl) return;
    const rect = svgEl.getBoundingClientRect();
    const cx = (rect.width / 2 - panX) / zoom;
    const cy = (rect.height / 2 - panY) / zoom;
    addCommentAt(snap(cx - 120), snap(cy - 70));
  }

  // ─── Slice E.8: per-node diagnostic aggregation ──────────────
  interface NodeDiag {
    severity: "error" | "warning";
    messages: string[];
  }
  let nodeDiags = $derived.by<Map<string, NodeDiag>>(() => {
    const out = new Map<string, NodeDiag>();
    const r = $behaviorReport;
    if (!r) return out;
    const push = (
      id: string | null | undefined,
      sev: "error" | "warning",
      msg: string,
    ) => {
      if (!id) return;
      const cur = out.get(id);
      if (!cur) {
        out.set(id, { severity: sev, messages: [msg] });
      } else {
        // Errors override warnings on the same node.
        if (sev === "error" && cur.severity === "warning") cur.severity = "error";
        cur.messages.push(msg);
      }
    };
    for (const d of r.errors) push(d.node_id, "error", `${d.code}: ${d.message}`);
    for (const d of r.warnings) push(d.node_id, "warning", `${d.code}: ${d.message}`);
    return out;
  });

  /** Cubic Bezier path between two points biased horizontally for
   *  left-to-right reading. */
  function bezierPath(
    x1: number,
    y1: number,
    x2: number,
    y2: number,
  ): string {
    const dx = Math.max(40, Math.abs(x2 - x1) * 0.5);
    return `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
  }

  // Control (exec) edges: from a specific exec_out pin on the
  // source header → exec_in pin on the dest header.
  interface DrawnControlEdge {
    key: string;
    d: string;
    fromId: string;
    out: string;
    toId: string;
  }
  let controlEdges = $derived<DrawnControlEdge[]>(
    (doc?.edges ?? [])
      .map((e, i): DrawnControlEdge | null => {
        const a = nodeById(e.from);
        const b = nodeById(e.to);
        if (!a || !b) return null;
        const out = e.out ?? "exec";
        const outCount = a.descriptor?.exec_out.length ?? 1;
        const outIdx = a.descriptor
          ? execOutIndex(a.descriptor, out)
          : 0;
        const x1 = a.x + a.width;
        const y1 = a.y + execOutPinY(Math.max(outIdx, 0), Math.max(outCount, 1));
        const x2 = b.x;
        const y2 = b.y + execInPinY();
        return {
          key: `${i}:${e.from}.${out}->${e.to}`,
          d: bezierPath(x1, y1, x2, y2),
          fromId: e.from,
          out,
          toId: e.to,
        };
      })
      .filter((e): e is DrawnControlEdge => e !== null),
  );

  // Data bindings: from each input pin's binding (if reference)
  // back to the source node's matching output pin.
  interface DrawnDataEdge {
    key: string;
    d: string;
    color: string;
    toId: string;
    toPin: string;
  }
  let dataEdges = $derived<DrawnDataEdge[]>(
    (doc?.nodes ?? []).flatMap((n) => {
      const dest = nodeById(n.id);
      if (!dest) return [];
      const destDesc = dest.descriptor;
      const inputs = n.in ?? {};
      const out: DrawnDataEdge[] = [];
      for (const [pinName, binding] of Object.entries(inputs)) {
        if (!isReferenceBinding(binding)) continue;
        const src = nodeById(binding.from);
        if (!src) continue;
        const srcOut = findOutputPin(src.descriptor, binding.out);
        const destIdx = inputPinIndex(destDesc, pinName);
        if (!srcOut || destIdx < 0) continue;
        const x1 = src.x + src.width;
        const y1 = src.y + outputPinY(srcOut.index);
        const x2 = dest.x;
        const y2 = dest.y + inputPinY(destIdx);
        out.push({
          key: `${binding.from}.${binding.out ?? "*"}->${n.id}.${pinName}`,
          d: bezierPath(x1, y1, x2, y2),
          color: pinTypeColor(srcOut.pin.ty),
          toId: n.id,
          toPin: pinName,
        });
      }
      return out;
    }),
  );

  // ─── Pan / zoom interactions ─────────────────────────────────

  type PointerMode =
    | { kind: "idle" }
    | {
        kind: "pan";
        startClientX: number;
        startClientY: number;
        originPanX: number;
        originPanY: number;
      }
    | {
        kind: "drag-nodes";
        startWorldX: number;
        startWorldY: number;
        ids: string[];
      }
    | {
        kind: "marquee";
        startWorldX: number;
        startWorldY: number;
        currentWorldX: number;
        currentWorldY: number;
        additive: boolean;
        baseSelection: Set<string>;
      }
    | {
        kind: "wire";
        // Source pin (the pin the user grabbed first). May be an
        // output OR an input — for input-side drags the wire is
        // semantically reversed when committed.
        fromNodeId: string;
        fromPinName: string;
        fromPinKind: "exec" | "data";
        fromPinType: string | null;
        fromSide: "out" | "in";
        // Anchor in world coords (start of the rubber-band path).
        anchorX: number;
        anchorY: number;
        // Live cursor in world coords.
        cursorX: number;
        cursorY: number;
        // Hovered candidate pin on the *opposite* side, if any.
        hover: {
          nodeId: string;
          pinName: string;
          kind: "exec" | "data";
          type: string | null;
          x: number;
          y: number;
          compatible: boolean;
        } | null;
      };
  let mode: PointerMode = $state({ kind: "idle" });

  /** Per-node visual delta during a live drag (world coords). */
  let dragDelta = $state<{ x: number; y: number }>({ x: 0, y: 0 });

  function clientToWorld(e: PointerEvent): { x: number; y: number } {
    if (!svgEl) return { x: 0, y: 0 };
    const rect = svgEl.getBoundingClientRect();
    return {
      x: (e.clientX - rect.left - panX) / zoom,
      y: (e.clientY - rect.top - panY) / zoom,
    };
  }

  function nodeAtTarget(target: EventTarget | null): string | null {
    let el = target as Element | null;
    while (el) {
      const id = el.getAttribute?.("data-node-id");
      if (id) return id;
      el = el.parentElement;
    }
    return null;
  }

  interface PinHit {
    nodeId: string;
    pinName: string;
    side: "in" | "out";
    kind: "exec" | "data";
    type: string | null;
  }

  function pinAtTarget(target: EventTarget | null): PinHit | null {
    let el = target as Element | null;
    while (el) {
      const nodeId = el.getAttribute?.("data-pin-node");
      if (nodeId) {
        const pinName = el.getAttribute("data-pin-name") ?? "";
        const side = el.getAttribute("data-pin-side") as "in" | "out";
        const kind = el.getAttribute("data-pin-kind") as "exec" | "data";
        const type = el.getAttribute("data-pin-type");
        return { nodeId, pinName, side, kind, type };
      }
      el = el.parentElement;
    }
    return null;
  }

  /** World-space coords of a pin's anchor (centre of the circle/diamond). */
  function pinWorldPos(pin: PinHit): { x: number; y: number } | null {
    const node = drawnNodes.find((n) => n.id === pin.nodeId);
    if (!node) return null;
    if (pin.kind === "exec") {
      if (pin.side === "in") {
        return { x: node.x, y: node.y + execInPinY() };
      }
      const count = node.descriptor?.exec_out.length ?? 1;
      const idx = node.descriptor
        ? execOutIndex(node.descriptor, pin.pinName)
        : 0;
      return {
        x: node.x + node.width,
        y: node.y + execOutPinY(Math.max(idx, 0), Math.max(count, 1)),
      };
    }
    // Data pin
    if (pin.side === "in") {
      const idx = inputPinIndex(node.descriptor, pin.pinName);
      if (idx < 0) return null;
      return { x: node.x, y: node.y + inputPinY(idx) };
    }
    const out = findOutputPin(node.descriptor, pin.pinName);
    if (!out) return null;
    return { x: node.x + node.width, y: node.y + outputPinY(out.index) };
  }

  function clientToWorldXY(clientX: number, clientY: number): {
    x: number;
    y: number;
  } {
    if (!svgEl) return { x: 0, y: 0 };
    const rect = svgEl.getBoundingClientRect();
    return {
      x: (clientX - rect.left - panX) / zoom,
      y: (clientY - rect.top - panY) / zoom,
    };
  }

  function handlePointerDown(e: PointerEvent) {
    // Pin interaction takes priority over node-body drag — start a wire.
    const pin = pinAtTarget(e.target);
    if (pin && e.button === 0) {
      // Wires can be dragged from either side. Output-side drags
      // commit forward; input-side drags commit reversed (we still
      // store edges as out→in / data binding to←from).
      const anchor = pinWorldPos(pin);
      if (!anchor) return;
      const w = clientToWorld(e);
      mode = {
        kind: "wire",
        fromNodeId: pin.nodeId,
        fromPinName: pin.pinName,
        fromPinKind: pin.kind,
        fromPinType: pin.type,
        fromSide: pin.side,
        anchorX: anchor.x,
        anchorY: anchor.y,
        cursorX: w.x,
        cursorY: w.y,
        hover: null,
      };
      svgEl?.setPointerCapture(e.pointerId);
      e.preventDefault();
      e.stopPropagation();
      return;
    }

    const nodeId = nodeAtTarget(e.target);

    // Middle-mouse always pans, regardless of target.
    if (e.button === 1) {
      mode = {
        kind: "pan",
        startClientX: e.clientX,
        startClientY: e.clientY,
        originPanX: panX,
        originPanY: panY,
      };
      svgEl?.setPointerCapture(e.pointerId);
      e.preventDefault();
      return;
    }

    if (e.button !== 0) return;

    if (nodeId != null) {
      // Node interaction: select + start drag.
      const cur = $state.snapshot($selectedNodeIds) as Set<string>;
      let next: Set<string>;
      if (e.shiftKey) {
        next = new Set(cur);
        if (next.has(nodeId)) next.delete(nodeId);
        else next.add(nodeId);
      } else if (cur.has(nodeId)) {
        next = cur;
      } else {
        next = new Set([nodeId]);
      }
      selectedNodeIds.set(next);

      if (next.has(nodeId)) {
        const w = clientToWorld(e);
        mode = {
          kind: "drag-nodes",
          startWorldX: w.x,
          startWorldY: w.y,
          ids: [...next],
        };
        dragDelta = { x: 0, y: 0 };
        svgEl?.setPointerCapture(e.pointerId);
      }
      e.preventDefault();
      return;
    }

    // Empty canvas: marquee select. Clear selection unless shift held.
    const w = clientToWorld(e);
    const base = e.shiftKey
      ? new Set($state.snapshot($selectedNodeIds) as Set<string>)
      : new Set<string>();
    if (!e.shiftKey) selectedNodeIds.set(new Set());
    mode = {
      kind: "marquee",
      startWorldX: w.x,
      startWorldY: w.y,
      currentWorldX: w.x,
      currentWorldY: w.y,
      additive: e.shiftKey,
      baseSelection: base,
    };
    svgEl?.setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function handlePointerMove(e: PointerEvent) {
    if (mode.kind === "pan") {
      panX = mode.originPanX + (e.clientX - mode.startClientX);
      panY = mode.originPanY + (e.clientY - mode.startClientY);
    } else if (mode.kind === "drag-nodes") {
      const w = clientToWorld(e);
      dragDelta = {
        x: w.x - mode.startWorldX,
        y: w.y - mode.startWorldY,
      };
    } else if (mode.kind === "marquee") {
      const w = clientToWorld(e);
      mode = { ...mode, currentWorldX: w.x, currentWorldY: w.y };
      // Live preview: recompute selection.
      const minX = Math.min(mode.startWorldX, w.x);
      const minY = Math.min(mode.startWorldY, w.y);
      const maxX = Math.max(mode.startWorldX, w.x);
      const maxY = Math.max(mode.startWorldY, w.y);
      const hits = new Set(mode.baseSelection);
      for (const n of drawnNodes) {
        const intersects =
          n.x < maxX && n.x + n.width > minX && n.y < maxY && n.y + n.height > minY;
        if (intersects) hits.add(n.id);
      }
      selectedNodeIds.set(hits);
    } else if (mode.kind === "wire") {
      const w = clientToWorld(e);
      // Hit-test under the cursor for an opposite-side pin
      // candidate: output drags look for inputs, input drags look
      // for outputs.
      const wantSide: "in" | "out" = mode.fromSide === "out" ? "in" : "out";
      const el = document.elementFromPoint(e.clientX, e.clientY);
      const pin = pinAtTarget(el);
      let hover: typeof mode.hover = null;
      if (
        pin &&
        pin.side === wantSide &&
        // Can't connect a node to itself.
        pin.nodeId !== mode.fromNodeId
      ) {
        const anchor = pinWorldPos(pin);
        if (anchor) {
          // Always evaluate compatibility in src(out)→dst(in)
          // direction, regardless of which side the user grabbed.
          const srcKind = mode.fromSide === "out" ? mode.fromPinKind : pin.kind;
          const srcType = mode.fromSide === "out" ? mode.fromPinType : pin.type;
          const dstKind = mode.fromSide === "out" ? pin.kind : mode.fromPinKind;
          const dstType = mode.fromSide === "out" ? pin.type : mode.fromPinType;
          hover = {
            nodeId: pin.nodeId,
            pinName: pin.pinName,
            kind: pin.kind,
            type: pin.type,
            x: anchor.x,
            y: anchor.y,
            compatible: pinsCompatible(srcKind, srcType, dstKind, dstType),
          };
        }
      }
      mode = { ...mode, cursorX: w.x, cursorY: w.y, hover };
    }
  }

  function handlePointerUp(e: PointerEvent) {
    if (mode.kind === "drag-nodes") {
      const dx = dragDelta.x;
      const dy = dragDelta.y;
      if (Math.abs(dx) > 0.5 || Math.abs(dy) > 0.5) {
        // Resolve current displayed position for each dragged node
        // (may be from saved positions or from layered fallback)
        // and write the absolute new position.
        const next = new Map<string, BehaviorPosition>();
        for (const id of mode.ids) {
          const n = drawnNodes.find((d) => d.id === id);
          if (!n) continue;
          next.set(id, { x: snap(n.x + dx), y: snap(n.y + dy) });
        }
        commitSetNodePositions(next);
      }
      dragDelta = { x: 0, y: 0 };
    } else if (mode.kind === "wire") {
      const hover = mode.hover;
      if (hover && hover.compatible) {
        // Normalise into out→in direction before committing.
        const srcNodeId = mode.fromSide === "out" ? mode.fromNodeId : hover.nodeId;
        const srcPinName = mode.fromSide === "out" ? mode.fromPinName : hover.pinName;
        const dstNodeId = mode.fromSide === "out" ? hover.nodeId : mode.fromNodeId;
        const dstPinName = mode.fromSide === "out" ? hover.pinName : mode.fromPinName;
        const kind = mode.fromPinKind;
        if (kind === "exec") {
          addControlEdge(srcNodeId, srcPinName, dstNodeId);
        } else {
          addDataBinding(dstNodeId, dstPinName, srcNodeId, srcPinName);
        }
      } else {
        // No compatible target. Two cases:
        //  • Cursor barely moved → treat as a click on the source
        //    pin and disconnect any existing wire (preserves the
        //    old click-to-disconnect behaviour for input pins).
        //  • Otherwise → open the quick-add menu so the user can
        //    spawn a matching node and auto-connect to it.
        const dx = mode.cursorX - mode.anchorX;
        const dy = mode.cursorY - mode.anchorY;
        const movedFar = dx * dx + dy * dy > 4 * 4; // 4 world units
        if (!movedFar && mode.fromSide === "in" && mode.fromPinKind === "data") {
          removeDataBinding(mode.fromNodeId, mode.fromPinName);
        } else if (movedFar) {
          openQuickAddMenu(e.clientX, e.clientY, {
            fromNodeId: mode.fromNodeId,
            fromPinName: mode.fromPinName,
            fromPinKind: mode.fromPinKind,
            fromPinType: mode.fromPinType,
            fromSide: mode.fromSide,
          });
        }
      }
    }
    if (mode.kind !== "idle") {
      svgEl?.releasePointerCapture(e.pointerId);
      mode = { kind: "idle" };
    }
  }
  function handleWheel(e: WheelEvent) {
    e.preventDefault();
    if (!svgEl) return;
    const rect = svgEl.getBoundingClientRect();
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;
    const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
    const newZoom = Math.max(0.2, Math.min(3, zoom * factor));
    // Keep cursor-anchored zoom: world point under cursor stays put.
    const wx = (cx - panX) / zoom;
    const wy = (cy - panY) / zoom;
    zoom = newZoom;
    panX = cx - wx * zoom;
    panY = cy - wy * zoom;
  }

  // ─── Drag-and-drop from palette (Slice E.5) ──────────────────

  function viewportCenterWorld(): { x: number; y: number } {
    if (!svgEl) return { x: 0, y: 0 };
    const rect = svgEl.getBoundingClientRect();
    return {
      x: (rect.width / 2 - panX) / zoom,
      y: (rect.height / 2 - panY) / zoom,
    };
  }

  function handleDragOver(e: DragEvent) {
    if (!e.dataTransfer) return;
    if (!Array.from(e.dataTransfer.types).includes(PALETTE_DND_MIME)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
  }

  function handleDrop(e: DragEvent) {
    if (!e.dataTransfer) return;
    const op = e.dataTransfer.getData(PALETTE_DND_MIME);
    if (!op) return;
    e.preventDefault();
    if (!svgEl) return;
    const rect = svgEl.getBoundingClientRect();
    const worldX = (e.clientX - rect.left - panX) / zoom;
    const worldY = (e.clientY - rect.top - panY) / zoom;
    addNodeAt(op, snap(worldX), snap(worldY));
  }

  // Palette double-click: drop the op at the viewport centre.
  $effect(() => {
    const op = $requestCenterDrop;
    if (op == null) return;
    const c = viewportCenterWorld();
    addNodeAt(op, snap(c.x), snap(c.y));
    requestCenterDrop.set(null);
  });

  // Slice E.8: focus on a node when the diagnostics list asks.
  $effect(() => {
    const req = $focusNodeRequest;
    if (req == null) return;
    const node = drawnNodes.find((n) => n.id === req.nodeId);
    if (!node || !svgEl) {
      focusNodeRequest.set(null);
      return;
    }
    const rect = svgEl.getBoundingClientRect();
    const cx = node.x + node.width / 2;
    const cy = node.y + node.height / 2;
    panX = rect.width / 2 - cx * zoom;
    panY = rect.height / 2 - cy * zoom;
    selectedNodeIds.set(new Set([req.nodeId]));
    focusNodeRequest.set(null);
  });
  function resetView() {
    panX = 0;
    panY = 0;
    zoom = 1;
  }

  // ─── Slice E.10b: auto-layout button ───────────────────────────
  // Re-runs the same layered placement that ungrouped graphs get
  // automatically (see `layoutNodes` in behavior/layout.ts) on
  // demand. Implementation strategy: build a synthetic doc whose
  // `editor.positions` is empty so `layoutNodes` is forced to
  // assign fresh coordinates to every node, then push the result
  // through the same `commitSetNodePositions` write path the drag
  // handler uses — one undo entry, snap-aware. Doing it in the
  // canvas (rather than the store) avoids leaking the layout
  // algorithm into the store layer; the store stays a thin
  // mutator over the JSON document.
  function autoLayout() {
    if (!doc) return;
    const synthetic: BehaviorDoc = {
      ...doc,
      editor: { ...(doc.editor ?? {}), positions: {} },
    };
    const fresh = layoutNodes(synthetic);
    const next = new Map<string, BehaviorPosition>();
    for (const n of doc.nodes) {
      const p = fresh[n.id];
      if (!p) continue;
      next.set(n.id, { x: snap(p.x), y: snap(p.y) });
    }
    if (next.size > 0) commitSetNodePositions(next);
  }

  // Visual offset applied to a node during a live drag (only the
  // selected nodes shift; everything else stays put).
  function liveOffset(id: string): { x: number; y: number } {
    if (mode.kind === "drag-nodes" && ($selectedNodeIds as Set<string>).has(id)) {
      return dragDelta;
    }
    return { x: 0, y: 0 };
  }

  // Marquee rect in world coords.
  let marqueeRect = $derived(
    mode.kind === "marquee"
      ? {
          x: Math.min(mode.startWorldX, mode.currentWorldX),
          y: Math.min(mode.startWorldY, mode.currentWorldY),
          w: Math.abs(mode.currentWorldX - mode.startWorldX),
          h: Math.abs(mode.currentWorldY - mode.startWorldY),
        }
      : null,
  );

  // ─── Wire ghost + edge context menu (Slice E.6) ──────────────

  /** SVG path of the rubber-band wire, or null. */
  let wireGhostPath = $derived<string | null>(
    mode.kind === "wire"
      ? bezierPath(mode.anchorX, mode.anchorY, mode.cursorX, mode.cursorY)
      : null,
  );

  /** Stroke colour for the rubber-band wire — green when over a
   *  compatible target, red when over an incompatible one, grey
   *  otherwise. */
  let wireGhostColor: string = $derived.by(() => {
    if (mode.kind !== "wire") return "var(--ed-faint)";
    if (!mode.hover) return "var(--ed-muted)";
    return mode.hover.compatible ? "var(--ed-ok)" : "var(--ed-err)";
  });

  /** Per-pin highlight class while wiring. Returns "" when not
   *  wiring or for pins that aren't relevant. */
  function pinHighlight(
    nodeId: string,
    pinName: string,
    side: "in" | "out",
    kind: "exec" | "data",
    type: string | null,
  ): string {
    if (mode.kind !== "wire") return "";
    if (side !== "in") return "";
    if (nodeId === mode.fromNodeId) return "";
    const compatible = pinsCompatible(
      mode.fromPinKind,
      mode.fromPinType,
      kind,
      type,
    );
    return compatible ? "compat" : "incompat";
  }

  type EdgeMenu =
    | { kind: "control"; from: string; out: string; to: string; x: number; y: number }
    | { kind: "data"; toId: string; toPin: string; x: number; y: number };
  let edgeMenu = $state<EdgeMenu | null>(null);

  // ─── Slice E.10c: minimap ─────────────────────────────────────────
  // Bottom-right overview that scales the entire graph plus the
  // current viewport rect into a fixed-size SVG. Click anywhere
  // on it to recentre the main canvas on that world point. Hidden
  // when the graph is empty (nothing to overview).
  const MINIMAP_W = 180;
  const MINIMAP_H = 110;
  const MINIMAP_PAD = 24; // world units of breathing room around bbox
  let viewportSize = $state<{ w: number; h: number }>({ w: 0, h: 0 });

  // Track the SVG client size so the viewport rect on the minimap
  // stays accurate when the panel is resized.
  $effect(() => {
    if (!svgEl) return;
    const update = () => {
      const r = svgEl!.getBoundingClientRect();
      viewportSize = { w: r.width, h: r.height };
    };
    update();
    const ro = new ResizeObserver(update);
    ro.observe(svgEl);
    return () => ro.disconnect();
  });

  interface MinimapProj {
    bbox: { x: number; y: number; w: number; h: number };
    scale: number;
    offsetX: number;
    offsetY: number;
  }

  let minimap = $derived.by<MinimapProj | null>(() => {
    if (drawnNodes.length === 0) return null;
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const n of drawnNodes) {
      if (n.x < minX) minX = n.x;
      if (n.y < minY) minY = n.y;
      if (n.x + n.width > maxX) maxX = n.x + n.width;
      if (n.y + n.height > maxY) maxY = n.y + n.height;
    }
    // Always include the current viewport so the user sees where
    // they are even after panning far off the node cloud.
    if (viewportSize.w > 0 && viewportSize.h > 0) {
      const vx = -panX / zoom;
      const vy = -panY / zoom;
      const vw = viewportSize.w / zoom;
      const vh = viewportSize.h / zoom;
      if (vx < minX) minX = vx;
      if (vy < minY) minY = vy;
      if (vx + vw > maxX) maxX = vx + vw;
      if (vy + vh > maxY) maxY = vy + vh;
    }
    minX -= MINIMAP_PAD;
    minY -= MINIMAP_PAD;
    maxX += MINIMAP_PAD;
    maxY += MINIMAP_PAD;
    const bw = Math.max(1, maxX - minX);
    const bh = Math.max(1, maxY - minY);
    const scale = Math.min(MINIMAP_W / bw, MINIMAP_H / bh);
    // Centre the projected bbox in the minimap rect.
    const offsetX = (MINIMAP_W - bw * scale) / 2;
    const offsetY = (MINIMAP_H - bh * scale) / 2;
    return {
      bbox: { x: minX, y: minY, w: bw, h: bh },
      scale,
      offsetX,
      offsetY,
    };
  });

  function projectToMinimap(
    m: MinimapProj,
    worldX: number,
    worldY: number,
  ): { x: number; y: number } {
    return {
      x: m.offsetX + (worldX - m.bbox.x) * m.scale,
      y: m.offsetY + (worldY - m.bbox.y) * m.scale,
    };
  }

  function handleMinimapClick(e: MouseEvent) {
    if (!minimap || !svgEl) return;
    const target = e.currentTarget as SVGSVGElement;
    const rect = target.getBoundingClientRect();
    const localX = e.clientX - rect.left;
    const localY = e.clientY - rect.top;
    // Inverse projection: minimap pixel → world coord.
    const worldX = (localX - minimap.offsetX) / minimap.scale + minimap.bbox.x;
    const worldY = (localY - minimap.offsetY) / minimap.scale + minimap.bbox.y;
    const canvasRect = svgEl.getBoundingClientRect();
    panX = canvasRect.width / 2 - worldX * zoom;
    panY = canvasRect.height / 2 - worldY * zoom;
  }

  function openControlEdgeMenu(
    e: MouseEvent,
    edge: DrawnControlEdge,
  ) {
    e.preventDefault();
    e.stopPropagation();
    edgeMenu = {
      kind: "control",
      from: edge.fromId,
      out: edge.out,
      to: edge.toId,
      x: e.clientX,
      y: e.clientY,
    };
  }

  function openDataEdgeMenu(e: MouseEvent, edge: DrawnDataEdge) {
    e.preventDefault();
    e.stopPropagation();
    edgeMenu = {
      kind: "data",
      toId: edge.toId,
      toPin: edge.toPin,
      x: e.clientX,
      y: e.clientY,
    };
  }

  function dismissEdgeMenu() {
    edgeMenu = null;
  }

  // ─── Slice E.10f: "Open in code editor" escape hatch ─────────
  let canvasMenu = $state<{ x: number; y: number } | null>(null);
  function openCanvasMenu(e: MouseEvent) {
    e.preventDefault();
    canvasMenu = { x: e.clientX, y: e.clientY };
  }
  function dismissCanvasMenu() {
    canvasMenu = null;
  }

  // ─── Quick-add menu: drop a wire onto empty canvas to spawn ──
  // a node whose first compatible input pin matches the source.
  interface QuickAddMenu {
    clientX: number;
    clientY: number;
    worldX: number;
    worldY: number;
    fromNodeId: string;
    fromPinName: string;
    fromPinKind: "exec" | "data";
    fromPinType: string | null;
    fromSide: "out" | "in";
    candidates: { op: OpDescriptor; targetPin: string | null }[];
    filter: string;
  }
  let quickAddMenu = $state<QuickAddMenu | null>(null);
  let quickAddSearchEl: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (quickAddMenu) {
      // Focus the search box once the input has been mounted.
      const el = quickAddSearchEl;
      if (el) queueMicrotask(() => el.focus());
    }
  });

  /** First input pin on `op` that would accept a wire from
   *  (`srcKind`, `srcType`). Returns the pin name, or `null` if
   *  exec (which uses the implicit `exec_in`), or `undefined` if
   *  the op has no compatible input. */
  function firstCompatibleInputPin(
    op: OpDescriptor,
    srcKind: "exec" | "data",
    srcType: string | null,
  ): string | null | undefined {
    if (srcKind === "exec") {
      return op.exec_in ? null : undefined;
    }
    for (const p of op.inputs) {
      if (pinsCompatible(srcKind, srcType, "data", p.ty)) return p.name;
    }
    return undefined;
  }

  /** First output pin on `op` that would feed a wire into
   *  (`dstKind`, `dstType`). Returns the pin name, or `null` for
   *  the implicit `exec` output (when only one is present), or
   *  `undefined` if the op has no compatible output. */
  function firstCompatibleOutputPin(
    op: OpDescriptor,
    dstKind: "exec" | "data",
    dstType: string | null,
  ): string | null | undefined {
    if (dstKind === "exec") {
      if (op.exec_out.length === 0) return undefined;
      return op.exec_out[0];
    }
    for (const p of op.outputs) {
      if (pinsCompatible("data", p.ty, dstKind, dstType)) return p.name;
    }
    return undefined;
  }

  function openQuickAddMenu(
    clientX: number,
    clientY: number,
    src: {
      fromNodeId: string;
      fromPinName: string;
      fromPinKind: "exec" | "data";
      fromPinType: string | null;
      fromSide: "out" | "in";
    },
  ) {
    const w = clientToWorldXY(clientX, clientY);
    const candidates: QuickAddMenu["candidates"] = [];
    for (const op of catalog) {
      const pin =
        src.fromSide === "out"
          ? firstCompatibleInputPin(op, src.fromPinKind, src.fromPinType)
          : firstCompatibleOutputPin(op, src.fromPinKind, src.fromPinType);
      if (pin === undefined) continue;
      candidates.push({ op, targetPin: pin });
    }
    candidates.sort((a, b) => a.op.op.localeCompare(b.op.op));
    quickAddMenu = {
      clientX,
      clientY,
      worldX: snap(w.x),
      worldY: snap(w.y),
      fromNodeId: src.fromNodeId,
      fromPinName: src.fromPinName,
      fromPinKind: src.fromPinKind,
      fromPinType: src.fromPinType,
      fromSide: src.fromSide,
      candidates,
      filter: "",
    };
  }

  function dismissQuickAddMenu() {
    quickAddMenu = null;
  }

  function commitQuickAdd(opName: string, targetPin: string | null) {
    const m = quickAddMenu;
    if (!m) return;
    const newId = addNodeAt(opName, m.worldX, m.worldY);
    if (newId == null) {
      quickAddMenu = null;
      return;
    }
    if (m.fromSide === "out") {
      // Forward wire: source(out) → newNode(in)
      if (m.fromPinKind === "exec") {
        addControlEdge(m.fromNodeId, m.fromPinName, newId);
      } else if (targetPin != null) {
        addDataBinding(newId, targetPin, m.fromNodeId, m.fromPinName);
      }
    } else {
      // Reverse wire: newNode(out) → source(in)
      if (m.fromPinKind === "exec") {
        // source pin is exec_in on m.fromNodeId; new node has the
        // exec_out (`targetPin` is its name, defaults to "exec").
        addControlEdge(newId, targetPin ?? "exec", m.fromNodeId);
      } else if (targetPin != null) {
        addDataBinding(m.fromNodeId, m.fromPinName, newId, targetPin);
      }
    }
    quickAddMenu = null;
  }

  let quickAddFiltered = $derived.by(() => {
    if (!quickAddMenu) return [] as QuickAddMenu["candidates"];
    const q = quickAddMenu.filter.trim().toLowerCase();
    if (!q) return quickAddMenu.candidates;
    return quickAddMenu.candidates.filter((c) =>
      c.op.op.toLowerCase().includes(q) ||
      c.op.summary.toLowerCase().includes(q) ||
      c.op.category.toLowerCase().includes(q),
    );
  });
  async function openBehaviorInCodeEditor() {
    canvasMenu = null;
    const proj = get(project);
    const fullPath = get(behaviorFilePath);
    if (!proj || !fullPath) return;
    // Build a project-relative path (`assets/...`) from the full path.
    const normalized = fullPath.replace(/\\/g, "/");
    const projNorm = proj.path.replace(/\\/g, "/");
    const relative = normalized.startsWith(projNorm)
      ? normalized.slice(projNorm.length).replace(/^\/+/, "")
      : normalized;
    try {
      await openInCodeEditor(
        proj.path,
        relative,
        proj.settings.code_editor ?? "",
      );
      logInfo(`Opened in code editor: ${relative}`);
    } catch (err) {
      logError(`${err}`);
    }
  }

  function performEdgeRemove() {
    if (!edgeMenu) return;
    if (edgeMenu.kind === "control") {
      removeControlEdge(edgeMenu.from, edgeMenu.out, edgeMenu.to);
    } else {
      removeDataBinding(edgeMenu.toId, edgeMenu.toPin);
    }
    edgeMenu = null;
  }
</script>

<div class="canvas-wrap">
  <div class="overlay">
    <button class="pill" onclick={resetView} title="Reset pan + zoom"
      >reset</button
    >
    <button
      class="pill"
      onclick={autoLayout}
      disabled={!doc || doc.nodes.length === 0}
      title="Re-run layered auto-layout on every node"
    >auto-layout</button>
    <button
      class="pill"
      class:active={snapEnabled}
      onclick={() => (snapEnabled = !snapEnabled)}
      title="Snap node positions to a {GRID_SIZE}-unit grid"
      aria-pressed={snapEnabled}
    >snap {snapEnabled ? "on" : "off"}</button>
    <button
      class="pill"
      onclick={addCommentAtCenter}
      title="Add a free-floating note at the centre of the viewport"
    >+ note</button>
    <span class="pill ro">{Math.round(zoom * 100)}%</span>
  </div>
  <svg
    bind:this={svgEl}
    class="canvas"
    onpointerdown={handlePointerDown}
    onpointermove={handlePointerMove}
    onpointerup={handlePointerUp}
    onwheel={handleWheel}
    ondragover={handleDragOver}
    ondrop={handleDrop}
    oncontextmenu={(e) => {
      // Right-click on bare canvas opens the escape-hatch menu
      // (“Open in code editor”). Edge / node oncontextmenu handlers
      // stopPropagation themselves so they keep their own menus.
      openCanvasMenu(e);
    }}
    role="application"
    aria-label="Behavior graph canvas"
  >
    <!-- background captures pan drags -->
    <rect class="bg" x="0" y="0" width="100%" height="100%" />

    <!-- Slice E.10a: grid overlay. Pattern is in world units so it
         pans + zooms with the canvas. Hidden when snap is off so
         the canvas stays clean for users who want freeform layout.
         Also hidden when zoomed out so far the dots become noise. -->
    {#if snapEnabled && zoom >= 0.5}
      <defs>
        <pattern
          id="behavior-grid"
          width={GRID_SIZE * zoom}
          height={GRID_SIZE * zoom}
          patternUnits="userSpaceOnUse"
          x={panX}
          y={panY}
        >
          <circle
            cx={(GRID_SIZE * zoom) / 2}
            cy={(GRID_SIZE * zoom) / 2}
            r={Math.max(0.5, zoom * 0.6)}
            class="grid-dot"
          />
        </pattern>
      </defs>
      <rect
        class="grid"
        x="0"
        y="0"
        width="100%"
        height="100%"
        fill="url(#behavior-grid)"
        pointer-events="none"
      />
    {/if}

    <g transform="translate({panX} {panY}) scale({zoom})">
      <!-- Data bindings underneath control edges -->
      {#each dataEdges as edge (edge.key)}
        <path
          class="data-edge"
          d={edge.d}
          stroke={edge.color}
          stroke-dasharray="6 4"
          fill="none"
          stroke-width="1.5"
          oncontextmenu={(e) => openDataEdgeMenu(e, edge)}
          role="presentation"
        />
      {/each}

      <!-- Control flow edges -->
      {#each controlEdges as edge (edge.key)}
        <path
          class="control-edge"
          d={edge.d}
          stroke={CONTROL_EDGE_COLOR}
          fill="none"
          stroke-width="2"
          oncontextmenu={(e) => openControlEdgeMenu(e, edge)}
          role="presentation"
        />
      {/each}

      <!-- Nodes -->
      {#each drawnNodes as node (node.id)}
        {@const off = liveOffset(node.id)}
        {@const selected = ($selectedNodeIds as Set<string>).has(node.id)}
        <g
          class="node"
          class:selected
          data-node-id={node.id}
          transform="translate({node.x + off.x} {node.y + off.y})"
        >
          <rect
            class="node-bg"
            width={node.width}
            height={node.height}
            rx="6"
            ry="6"
          />
          {#if selected}
            <rect
              class="node-selection"
              x="-3"
              y="-3"
              width={node.width + 6}
              height={node.height + 6}
              rx="8"
              ry="8"
            />
          {/if}
          <rect
            class="node-header"
            width={node.width}
            height={NODE_HEADER_HEIGHT}
            rx="6"
            ry="6"
            fill={node.headerColor}
          />
          <!-- mask the bottom corners of the header -->
          <rect
            x="0"
            y={NODE_HEADER_HEIGHT - 6}
            width={node.width}
            height="6"
            fill={node.headerColor}
          />
          <text
            class="node-title"
            x="10"
            y={NODE_HEADER_HEIGHT / 2 + 4}>{node.op}</text
          >
          {#if !node.descriptor}
            <text
              class="node-unknown"
              x={node.width - 8}
              y={NODE_HEADER_HEIGHT / 2 + 4}
              text-anchor="end">?</text
            >
          {/if}
          <text
            class="node-id"
            x={node.width - 8}
            y={NODE_HEADER_HEIGHT - 6}
            text-anchor="end">{node.id}</text
          >

          {#if nodeDiags.get(node.id)}
            {@const diag = nodeDiags.get(node.id)!}
            <g class="node-diag-chip" transform="translate({node.width - 10} {NODE_HEADER_HEIGHT + 10})">
              <circle
                r="6"
                fill={diag.severity === "error" ? "var(--ed-err)" : "var(--ed-warn)"}
                stroke="var(--ed-crust)"
                stroke-width="1"
              />
              <text
                class="node-diag-count"
                x="0"
                y="3"
                text-anchor="middle">{diag.messages.length}</text
              >
              <title>{diag.messages.join("\n")}</title>
            </g>
          {/if}
          {#if node.descriptor}
            <!-- Exec input pin (diamond on left of header) -->
            {#if node.descriptor.exec_in}
              <g
                class="pin exec in {pinHighlight(node.id, 'exec', 'in', 'exec', null)}"
                data-pin-node={node.id}
                data-pin-name="exec"
                data-pin-side="in"
                data-pin-kind="exec"
                data-pin-type=""
                transform="translate(0 {execInPinY()})"
              >
                <polygon
                  class="exec-pin-shape"
                  points="-{EXEC_PIN_RADIUS},0 0,-{EXEC_PIN_RADIUS} {EXEC_PIN_RADIUS},0 0,{EXEC_PIN_RADIUS}"
                  fill={CONTROL_EDGE_COLOR}
                  stroke="var(--ed-crust)"
                  stroke-width="1"
                />
              </g>
            {/if}
            <!-- Exec output pins (diamonds on right of header) -->
            {#each node.descriptor.exec_out as outName, oi (outName)}
              {@const ey = execOutPinY(oi, node.descriptor.exec_out.length)}
              <g
                class="pin exec out"
                data-pin-node={node.id}
                data-pin-name={outName}
                data-pin-side="out"
                data-pin-kind="exec"
                data-pin-type=""
                transform="translate({node.width} {ey})"
              >
                <polygon
                  class="exec-pin-shape"
                  points="-{EXEC_PIN_RADIUS},0 0,-{EXEC_PIN_RADIUS} {EXEC_PIN_RADIUS},0 0,{EXEC_PIN_RADIUS}"
                  fill={CONTROL_EDGE_COLOR}
                  stroke="var(--ed-crust)"
                  stroke-width="1"
                />
                {#if node.descriptor.exec_out.length > 1}
                  <text
                    class="exec-label"
                    x={-EXEC_PIN_RADIUS - 4}
                    y="3"
                    text-anchor="end">{outName}</text
                  >
                {/if}
              </g>
            {/each}

            {#each node.descriptor.inputs as pin, i (pin.name)}
              <g
                class="pin in {pinHighlight(node.id, pin.name, 'in', 'data', pin.ty)}"
                data-pin-node={node.id}
                data-pin-name={pin.name}
                data-pin-side="in"
                data-pin-kind="data"
                data-pin-type={pin.ty}
                transform="translate(0 {NODE_HEADER_HEIGHT +
                  i * NODE_PIN_HEIGHT})"
              >
                <circle
                  class="data-pin-shape"
                  cx="0"
                  cy={NODE_PIN_HEIGHT / 2}
                  r="4"
                  fill={pinTypeColor(pin.ty)}
                  stroke="var(--ed-crust)"
                  stroke-width="1"
                />
                <text
                  class="pin-label"
                  x="10"
                  y={NODE_PIN_HEIGHT / 2 + 4}>{pin.name}</text
                >
              </g>
            {/each}
            {#each node.descriptor.outputs as pin, i (pin.name)}
              <g
                class="pin out"
                data-pin-node={node.id}
                data-pin-name={pin.name}
                data-pin-side="out"
                data-pin-kind="data"
                data-pin-type={pin.ty}
                transform="translate({node.width} {NODE_HEADER_HEIGHT +
                  i * NODE_PIN_HEIGHT})"
              >
                <circle
                  class="data-pin-shape"
                  cx="0"
                  cy={NODE_PIN_HEIGHT / 2}
                  r="4"
                  fill={pinTypeColor(pin.ty)}
                  stroke="var(--ed-crust)"
                  stroke-width="1"
                />
                <text
                  class="pin-label out"
                  x="-10"
                  y={NODE_PIN_HEIGHT / 2 + 4}
                  text-anchor="end">{pin.name}</text
                >
              </g>
            {/each}
          {/if}
        </g>
      {/each}

      {#if marqueeRect}
        <rect
          class="marquee"
          x={marqueeRect.x}
          y={marqueeRect.y}
          width={marqueeRect.w}
          height={marqueeRect.h}
        />
      {/if}

      {#if wireGhostPath}
        <path
          class="wire-ghost"
          d={wireGhostPath}
          stroke={wireGhostColor}
          fill="none"
          stroke-width="2"
          stroke-dasharray="4 3"
          pointer-events="none"
        />
      {/if}
    </g>
  </svg>

  {#if docComments.length > 0}
    <!-- Slice E.10e: comment overlay. HTML rather than SVG so the
         in-place textarea behaves like a normal input. Positioned
         in screen pixels via the same pan/zoom math the SVG uses;
         pointerdown handlers stopPropagation so canvas pan/marquee
         doesn't fire when the user grabs a comment. -->
    <div class="comments-layer">
      {#each docComments as comment (comment.id)}
        {@const r = liveCommentRect(comment)}
        {@const fill = comment.color ?? "var(--ctp-yellow)"}
        <div
          class="comment"
          style="left: {panX + r.x * zoom}px; top: {panY + r.y * zoom}px;
                 width: {r.w * zoom}px; height: {r.h * zoom}px;
                 --comment-color: {fill};"
        >
          <div
            class="comment-header"
            onpointerdown={(e) => startCommentDrag(e, comment, "move")}
            onpointermove={onCommentPointerMove}
            onpointerup={onCommentPointerUp}
            oncontextmenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
            }}
            role="presentation"
          >
            <span class="comment-grip" aria-hidden="true">⋮⋮</span>
            <button
              type="button"
              class="comment-close"
              title="Remove note"
              onclick={(e) => {
                e.stopPropagation();
                deleteComment(comment.id);
              }}
              onpointerdown={(e) => e.stopPropagation()}
              aria-label="Remove note"
            >✕</button>
          </div>
          <textarea
            class="comment-text"
            value={comment.text}
            oninput={(e) => onCommentTextInput(comment.id, e)}
            onpointerdown={(e) => e.stopPropagation()}
            spellcheck="false"
            placeholder="Note…"
            style="font-size: {Math.max(10, 13 * zoom)}px;"
          ></textarea>
          <div
            class="comment-resize"
            onpointerdown={(e) => startCommentDrag(e, comment, "resize")}
            onpointermove={onCommentPointerMove}
            onpointerup={onCommentPointerUp}
            role="presentation"
            aria-hidden="true"
          ></div>
        </div>
      {/each}
    </div>
  {/if}

  {#if minimap}
    {@const m = minimap}
    <svg
      class="minimap"
      width={MINIMAP_W}
      height={MINIMAP_H}
      viewBox="0 0 {MINIMAP_W} {MINIMAP_H}"
      onclick={handleMinimapClick}
      onpointerdown={(e) => e.stopPropagation()}
      role="presentation"
    >
      <rect class="minimap-bg" x="0" y="0" width={MINIMAP_W} height={MINIMAP_H} />
      {#each drawnNodes as node (node.id)}
        {@const p = projectToMinimap(m, node.x, node.y)}
        <rect
          class="minimap-node"
          x={p.x}
          y={p.y}
          width={Math.max(1, node.width * m.scale)}
          height={Math.max(1, node.height * m.scale)}
          fill={node.headerColor}
        />
      {/each}
      {#if viewportSize.w > 0 && viewportSize.h > 0}
        {@const v = projectToMinimap(m, -panX / zoom, -panY / zoom)}
        <rect
          class="minimap-viewport"
          x={v.x}
          y={v.y}
          width={Math.max(2, (viewportSize.w / zoom) * m.scale)}
          height={Math.max(2, (viewportSize.h / zoom) * m.scale)}
        />
      {/if}
    </svg>
  {/if}

  {#if edgeMenu}
    <!-- Click-outside catcher -->
    <button
      type="button"
      class="menu-scrim"
      onclick={dismissEdgeMenu}
      oncontextmenu={(e) => {
        e.preventDefault();
        dismissEdgeMenu();
      }}
      aria-label="Dismiss menu"
    ></button>
    <div
      class="ctx-menu"
      role="menu"
      style="left: {edgeMenu.x}px; top: {edgeMenu.y}px"
    >
      <button
        type="button"
        class="ctx-item"
        role="menuitem"
        onclick={performEdgeRemove}>Remove</button
      >
    </div>
  {/if}

  {#if canvasMenu}
    <button
      type="button"
      class="menu-scrim"
      onclick={dismissCanvasMenu}
      oncontextmenu={(e) => {
        e.preventDefault();
        dismissCanvasMenu();
      }}
      aria-label="Dismiss menu"
    ></button>
    <div
      class="ctx-menu"
      role="menu"
      style="left: {canvasMenu.x}px; top: {canvasMenu.y}px"
    >
      <button
        type="button"
        class="ctx-item"
        role="menuitem"
        onclick={openBehaviorInCodeEditor}
        >Open in code editor</button
      >
    </div>
  {/if}

  {#if quickAddMenu}
    <button
      type="button"
      class="menu-scrim"
      onclick={dismissQuickAddMenu}
      oncontextmenu={(e) => {
        e.preventDefault();
        dismissQuickAddMenu();
      }}
      aria-label="Dismiss menu"
    ></button>
    <div
      class="quick-add"
      role="menu"
      style="left: {quickAddMenu.clientX}px; top: {quickAddMenu.clientY}px"
    >
      <div class="quick-add-header">
        Add node compatible with
        <strong>
          {quickAddMenu.fromPinKind === "exec"
            ? "exec"
            : (quickAddMenu.fromPinType ?? "?")}
        </strong>
      </div>
      <input
        type="text"
        class="quick-add-search"
        placeholder="Search ops…"
        bind:value={quickAddMenu.filter}
        bind:this={quickAddSearchEl}
        onkeydown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            dismissQuickAddMenu();
          } else if (e.key === "Enter" && quickAddFiltered.length > 0) {
            e.preventDefault();
            const first = quickAddFiltered[0];
            commitQuickAdd(first.op.op, first.targetPin);
          }
        }}
      />
      <div class="quick-add-list">
        {#if quickAddFiltered.length === 0}
          <div class="quick-add-empty">No compatible ops</div>
        {:else}
          {#each quickAddFiltered as entry (entry.op.op)}
            <button
              type="button"
              class="quick-add-item"
              role="menuitem"
              onclick={() => commitQuickAdd(entry.op.op, entry.targetPin)}
              title={entry.op.summary}
            >
              <span
                class="quick-add-cat"
                style="background: {categoryColor(entry.op.category)}"
              ></span>
              <span class="quick-add-op">{entry.op.op}</span>
              {#if entry.targetPin}
                <span class="quick-add-pin">→ {entry.targetPin}</span>
              {/if}
            </button>
          {/each}
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .canvas-wrap {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: var(--ctp-base);
  }
  .canvas {
    width: 100%;
    height: 100%;
    display: block;
    cursor: grab;
    user-select: none;
  }
  .canvas:active {
    cursor: grabbing;
  }
  .bg {
    fill: var(--ctp-base);
  }
  .node-bg {
    fill: var(--ctp-mantle);
    stroke: var(--ctp-surface1);
    stroke-width: 1;
  }
  .node-selection {
    fill: none;
    stroke: var(--ctp-yellow);
    stroke-width: 2;
    pointer-events: none;
  }
  .node {
    cursor: move;
  }
  .marquee {
    fill: var(--ctp-yellow);
    fill-opacity: 0.08;
    stroke: var(--ctp-yellow);
    stroke-width: 1;
    stroke-dasharray: 4 3;
    pointer-events: none;
  }
  .node-title {
    fill: var(--ctp-base);
    font-family: var(--ed-mono);
    font-size: 12px;
    font-weight: 600;
    pointer-events: none;
  }
  .node-unknown {
    fill: var(--ctp-base);
    font-size: 14px;
    font-weight: 700;
    pointer-events: none;
  }
  .node-id {
    fill: var(--ctp-base);
    font-family: var(--ed-mono);
    font-size: 9px;
    opacity: 0.7;
    pointer-events: none;
  }
  .node-diag-chip {
    pointer-events: all;
    cursor: help;
  }
  .node-diag-count {
    fill: var(--ed-on-accent);
    font-family: var(--ed-mono);
    font-size: 9px;
    font-weight: 700;
    pointer-events: none;
  }
  .pin-label {
    fill: var(--ctp-text);
    font-family: var(--ed-mono);
    font-size: 10px;
    pointer-events: none;
  }
  .overlay {
    position: absolute;
    top: 8px;
    right: 8px;
    z-index: 1;
    display: flex;
    gap: 6px;
    align-items: center;
    pointer-events: none;
  }
  .pill {
    background: var(--ctp-surface0);
    color: var(--ctp-text);
    border: 1px solid var(--ctp-surface1);
    border-radius: 10px;
    padding: 2px 10px;
    font-size: 10px;
    font-family: var(--ed-mono);
    pointer-events: auto;
    cursor: pointer;
  }
  .pill.ro {
    cursor: default;
    opacity: 0.85;
  }
  .pill:hover:not(.ro) {
    background: var(--ctp-surface1);
  }
  .pill.active {
    background: var(--ctp-surface2);
    border-color: var(--ctp-yellow);
    color: var(--ctp-yellow);
  }
  .pill:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .grid-dot {
    fill: var(--ctp-surface1);
  }
  .minimap {
    position: absolute;
    right: 8px;
    bottom: 8px;
    z-index: 1;
    background: var(--ctp-mantle);
    border: 1px solid var(--ctp-surface1);
    border-radius: 4px;
    cursor: crosshair;
    pointer-events: auto;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
  }
  .minimap-bg {
    fill: var(--ctp-mantle);
  }
  .minimap-node {
    opacity: 0.85;
  }
  .minimap-viewport {
    fill: var(--ctp-yellow);
    fill-opacity: 0.08;
    stroke: var(--ctp-yellow);
    stroke-width: 1;
    pointer-events: none;
  }  .pin {
    cursor: crosshair;
  }
  .pin .data-pin-shape,
  .pin .exec-pin-shape {
    transition: stroke 60ms ease, stroke-width 60ms ease;
  }
  .pin.compat .data-pin-shape,
  .pin.compat .exec-pin-shape {
    stroke: var(--ctp-green);
    stroke-width: 2;
  }
  .pin.incompat .data-pin-shape,
  .pin.incompat .exec-pin-shape {
    stroke: var(--ctp-red);
    stroke-width: 2;
  }
  .exec-label {
    fill: var(--ctp-base);
    font-family: var(--ed-mono);
    font-size: 9px;
    pointer-events: none;
  }
  .control-edge,
  .data-edge {
    cursor: context-menu;
  }
  .wire-ghost {
    pointer-events: none;
  }
  .menu-scrim {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    background: transparent;
    border: none;
    padding: 0;
    margin: 0;
    z-index: 2;
    cursor: default;
  }
  .ctx-menu {
    position: fixed;
    z-index: 3;
    background: var(--ctp-surface0);
    border: 1px solid var(--ctp-surface1);
    border-radius: 7px;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
    padding: 4px;
    min-width: 100px;
  }
  .ctx-item {
    display: block;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    color: var(--ctp-text);
    padding: 4px 10px;
    border-radius: 3px;
    cursor: pointer;
    font-size: 12px;
  }
  .ctx-item:hover {
    background: var(--ctp-red);
    color: var(--ctp-base);
  }

  .quick-add {
    position: fixed;
    z-index: 3;
    background: var(--ctp-surface0);
    border: 1px solid var(--ctp-surface1);
    border-radius: 4px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
    padding: 6px;
    width: 240px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .quick-add-header {
    font-size: 11px;
    color: var(--ctp-subtext0);
    padding: 0 2px;
  }
  .quick-add-header strong {
    color: var(--ctp-text);
  }
  .quick-add-search {
    background: var(--ctp-base);
    border: 1px solid var(--ctp-surface1);
    border-radius: 3px;
    padding: 4px 6px;
    color: var(--ctp-text);
    font-size: 12px;
    outline: none;
  }
  .quick-add-search:focus {
    border-color: var(--ctp-blue);
  }
  .quick-add-list {
    max-height: 280px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .quick-add-empty {
    padding: 8px;
    text-align: center;
    color: var(--ctp-subtext0);
    font-size: 12px;
  }
  .quick-add-item {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    color: var(--ctp-text);
    padding: 4px 6px;
    border-radius: 3px;
    cursor: pointer;
    font-size: 12px;
  }
  .quick-add-item:hover {
    background: var(--ctp-surface1);
  }
  .quick-add-cat {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    flex: 0 0 auto;
  }
  .quick-add-op {
    flex: 1 1 auto;
  }
  .quick-add-pin {
    font-size: 10px;
    color: var(--ctp-subtext0);
  }

  /* Slice E.10e: comment overlay */
  .comments-layer {
    position: absolute;
    inset: 0;
    pointer-events: none;
    z-index: 0;
  }
  .comment {
    position: absolute;
    pointer-events: auto;
    background: color-mix(in srgb, var(--comment-color) 18%, var(--ctp-mantle));
    border: 1px solid var(--comment-color);
    border-radius: 4px;
    display: flex;
    flex-direction: column;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.35);
    overflow: hidden;
    min-width: 80px;
    min-height: 60px;
  }
  .comment-header {
    height: 18px;
    background: color-mix(in srgb, var(--comment-color) 60%, var(--ctp-mantle));
    cursor: move;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 4px;
    user-select: none;
    flex: 0 0 auto;
  }
  .comment-grip {
    color: var(--ctp-base);
    font-size: 10px;
    letter-spacing: -1px;
    opacity: 0.7;
  }
  .comment-close {
    background: transparent;
    border: none;
    color: var(--ctp-base);
    cursor: pointer;
    font-size: 11px;
    line-height: 1;
    padding: 1px 4px;
    border-radius: 2px;
    opacity: 0.6;
  }
  .comment-close:hover {
    opacity: 1;
    background: var(--ctp-red);
    color: var(--ctp-base);
  }
  .comment-text {
    flex: 1 1 auto;
    background: transparent;
    color: var(--ctp-text);
    border: none;
    outline: none;
    resize: none;
    padding: 6px 8px;
    font-family: inherit;
    line-height: 1.35;
    overflow: auto;
  }
  .comment-resize {
    position: absolute;
    right: 0;
    bottom: 0;
    width: 14px;
    height: 14px;
    cursor: nwse-resize;
    background: linear-gradient(
      135deg,
      transparent 50%,
      var(--comment-color) 50%
    );
    opacity: 0.6;
  }
  .comment-resize:hover {
    opacity: 1;
  }
</style>
