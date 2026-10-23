<script lang="ts">
    import { requestEditorClose } from "./stores/editorClose";
    import { themeColors } from "./stores/themeColors";
    import { onMount, onDestroy } from "svelte";
    import { get } from "svelte/store";
    import {
        stateMachineData,
        stateMachineLayout,
        stateMachineDirty,
        stateMachineEntityId,
        smSelectedState,
        smSelectedTransition,
        canUndoSM,
        canRedoSM,
        undoSM,
        redoSM,
        executeSM,
        closeStateMachineEditor,
    } from "./stores/stateMachineEditorStore";
    import type {
        TransitionCondition,
        AnimParamData,
    } from "./api/stateMachine";

    // ---------------------------------------------------------------------------
    // Constants
    // ---------------------------------------------------------------------------
    const NODE_WIDTH = 180;
    const NODE_HEIGHT = 56;
    const NODE_RADIUS = 8;
    const PIN_RADIUS = 5;
    const ARROW_SIZE = 8;

    // Colors (Catppuccin Mocha)
    const COL_BG = $derived($themeColors.base);
    const COL_GRID = $derived($themeColors.hover);
    const COL_NODE = $derived($themeColors.raised);
    const COL_NODE_SELECTED = $derived($themeColors.accent);
    const COL_NODE_CURRENT = $derived($themeColors.ok);
    const COL_NODE_TEXT = $derived($themeColors.ink);
    const COL_NODE_SUB = $derived($themeColors.muted);
    const COL_EDGE = $derived($themeColors.faint);
    const COL_EDGE_SELECTED = $derived($themeColors.warn);
    const COL_EDGE_HOVER = $derived($themeColors.accent);
    const COL_PIN_RING = $derived($themeColors["line-strong"]);

    // ---------------------------------------------------------------------------
    // State
    // ---------------------------------------------------------------------------
    let canvasEl: HTMLCanvasElement | undefined = $state();
    let ctx: CanvasRenderingContext2D | null = $state(null);
    let width = $state(800);
    let height = $state(600);
    let panOffset = $state<[number, number]>([0, 0]);
    let zoom = $state(1);
    let resizeObserver: ResizeObserver | undefined;

    // Interaction state
    let draggingNode: string | null = $state(null);
    let dragStart = $state<[number, number]>([0, 0]);
    let dragNodeStart = $state<[number, number]>([0, 0]);
    let isPanning = $state(false);
    let panStart = $state<[number, number]>([0, 0]);
    let panStartOffset = $state<[number, number]>([0, 0]);

    // Connecting transitions
    let connectingFrom: string | null = $state(null);
    let connectPreview = $state<[number, number] | null>(null);

    // Context menu
    let showContextMenu = $state(false);
    let contextMenuPos = $state<[number, number]>([0, 0]);
    let contextMenuGraph = $state<[number, number]>([0, 0]);

    // Property panel
    let showProperties = $state(true);
    let showParameters = $state(true);
    let newStateName = $state("");
    let newParamName = $state("");
    let newParamType = $state<"Bool" | "Float" | "Trigger">("Bool");

    // Condition add
    let newCondType = $state<TransitionCondition["type"]>("BoolEquals");

    // ---------------------------------------------------------------------------
    // Coordinate helpers
    // ---------------------------------------------------------------------------
    function screenToGraph(sx: number, sy: number): [number, number] {
        return [(sx - panOffset[0]) / zoom, (sy - panOffset[1]) / zoom];
    }

    function graphToScreen(gx: number, gy: number): [number, number] {
        return [gx * zoom + panOffset[0], gy * zoom + panOffset[1]];
    }

    // ---------------------------------------------------------------------------
    // Hit testing
    // ---------------------------------------------------------------------------
    function hitTestNode(gx: number, gy: number): string | null {
        const layout = get(stateMachineLayout);
        // Reverse order so topmost nodes hit first
        const names = Object.keys(layout).reverse();
        for (const name of names) {
            const pos = layout[name];
            if (
                gx >= pos.x &&
                gx <= pos.x + NODE_WIDTH &&
                gy >= pos.y &&
                gy <= pos.y + NODE_HEIGHT
            ) {
                return name;
            }
        }
        return null;
    }

    /** Find which transition edge is near (gx, gy). Returns [fromState, transitionIndex] or null. */
    function hitTestEdge(gx: number, gy: number): [string, number] | null {
        const data = get(stateMachineData);
        const layout = get(stateMachineLayout);
        if (!data) return null;
        const threshold = 8 / zoom;
        for (const [fromName, state] of Object.entries(data.states)) {
            const fromPos = layout[fromName];
            if (!fromPos) continue;
            for (let ti = 0; ti < state.transitions.length; ti++) {
                const tr = state.transitions[ti];
                const toPos = layout[tr.target];
                if (!toPos) continue;
                const x1 = fromPos.x + NODE_WIDTH;
                const y1 = fromPos.y + NODE_HEIGHT / 2;
                const x2 = toPos.x;
                const y2 = toPos.y + NODE_HEIGHT / 2;
                // Simple distance-to-line-segment test
                if (pointToSegmentDist(gx, gy, x1, y1, x2, y2) < threshold) {
                    return [fromName, ti];
                }
            }
        }
        return null;
    }

    function pointToSegmentDist(
        px: number,
        py: number,
        x1: number,
        y1: number,
        x2: number,
        y2: number,
    ): number {
        const dx = x2 - x1;
        const dy = y2 - y1;
        const lenSq = dx * dx + dy * dy;
        if (lenSq === 0) return Math.hypot(px - x1, py - y1);
        let t = ((px - x1) * dx + (py - y1) * dy) / lenSq;
        t = Math.max(0, Math.min(1, t));
        return Math.hypot(px - (x1 + t * dx), py - (y1 + t * dy));
    }

    // ---------------------------------------------------------------------------
    // Drawing
    // ---------------------------------------------------------------------------
    function draw() {
        if (!ctx) return;
        const c = ctx;
        c.save();
        c.clearRect(0, 0, width, height);
        c.fillStyle = COL_BG;
        c.fillRect(0, 0, width, height);

        // Grid
        drawGrid(c);

        c.translate(panOffset[0], panOffset[1]);
        c.scale(zoom, zoom);

        const data = get(stateMachineData);
        const layout = get(stateMachineLayout);
        if (!data) {
            c.restore();
            return;
        }

        // Draw edges (transitions)
        for (const [fromName, state] of Object.entries(data.states)) {
            const fromPos = layout[fromName];
            if (!fromPos) continue;
            for (let ti = 0; ti < state.transitions.length; ti++) {
                const tr = state.transitions[ti];
                const toPos = layout[tr.target];
                if (!toPos) continue;
                const sel = $smSelectedTransition;
                const isSelected =
                    sel !== null && sel[0] === fromName && sel[1] === ti;
                drawEdge(
                    c,
                    fromPos.x + NODE_WIDTH,
                    fromPos.y + NODE_HEIGHT / 2,
                    toPos.x,
                    toPos.y + NODE_HEIGHT / 2,
                    isSelected,
                    tr.conditions.length,
                );
            }
        }

        // Draw connecting preview
        if (connectingFrom && connectPreview) {
            const fromPos = layout[connectingFrom];
            if (fromPos) {
                const [gx, gy] = screenToGraph(
                    connectPreview[0],
                    connectPreview[1],
                );
                c.strokeStyle = COL_EDGE_HOVER;
                c.lineWidth = 2;
                c.setLineDash([6, 4]);
                c.beginPath();
                c.moveTo(fromPos.x + NODE_WIDTH, fromPos.y + NODE_HEIGHT / 2);
                c.lineTo(gx, gy);
                c.stroke();
                c.setLineDash([]);
            }
        }

        // Draw nodes (states)
        for (const [name, stateData] of Object.entries(data.states)) {
            const pos = layout[name];
            if (!pos) continue;
            const isSelected = $smSelectedState === name;
            const isCurrent = data.current_state === name;
            drawNode(
                c,
                pos.x,
                pos.y,
                name,
                stateData.clip_name,
                isSelected,
                isCurrent,
            );
        }

        c.restore();
    }

    function drawGrid(c: CanvasRenderingContext2D) {
        const gridSize = 40 * zoom;
        const offX = panOffset[0] % gridSize;
        const offY = panOffset[1] % gridSize;
        c.strokeStyle = COL_GRID;
        c.lineWidth = 1;
        c.beginPath();
        for (let x = offX; x < width; x += gridSize) {
            c.moveTo(Math.round(x) + 0.5, 0);
            c.lineTo(Math.round(x) + 0.5, height);
        }
        for (let y = offY; y < height; y += gridSize) {
            c.moveTo(0, Math.round(y) + 0.5);
            c.lineTo(width, Math.round(y) + 0.5);
        }
        c.stroke();
    }

    function drawNode(
        c: CanvasRenderingContext2D,
        x: number,
        y: number,
        name: string,
        clipName: string,
        selected: boolean,
        isCurrent: boolean,
    ) {
        // Shadow
        c.fillStyle = "rgba(0,0,0,0.3)";
        roundRect(c, x + 2, y + 2, NODE_WIDTH, NODE_HEIGHT, NODE_RADIUS);
        c.fill();

        // Body
        c.fillStyle = COL_NODE;
        roundRect(c, x, y, NODE_WIDTH, NODE_HEIGHT, NODE_RADIUS);
        c.fill();

        // Border
        if (selected) {
            c.strokeStyle = COL_NODE_SELECTED;
            c.lineWidth = 2;
        } else if (isCurrent) {
            c.strokeStyle = COL_NODE_CURRENT;
            c.lineWidth = 2;
        } else {
            c.strokeStyle = COL_EDGE;
            c.lineWidth = 1;
        }
        roundRect(c, x, y, NODE_WIDTH, NODE_HEIGHT, NODE_RADIUS);
        c.stroke();

        // Current state indicator (green dot)
        if (isCurrent) {
            c.fillStyle = COL_NODE_CURRENT;
            c.beginPath();
            c.arc(x + 12, y + 14, 4, 0, Math.PI * 2);
            c.fill();
        }

        // State name
        c.fillStyle = COL_NODE_TEXT;
        c.font = "bold 12px sans-serif";
        c.textBaseline = "top";
        c.fillText(name, x + (isCurrent ? 22 : 8), y + 8, NODE_WIDTH - 16);

        // Clip name (subtitle)
        c.fillStyle = COL_NODE_SUB;
        c.font = "10px sans-serif";
        c.fillText(clipName || "(no clip)", x + 8, y + 28, NODE_WIDTH - 16);

        // Output pin (right side)
        c.fillStyle = COL_EDGE;
        c.beginPath();
        c.arc(x + NODE_WIDTH, y + NODE_HEIGHT / 2, PIN_RADIUS, 0, Math.PI * 2);
        c.fill();
        c.strokeStyle = COL_PIN_RING;
        c.lineWidth = 1;
        c.stroke();

        // Input pin (left side)
        c.fillStyle = COL_EDGE;
        c.beginPath();
        c.arc(x, y + NODE_HEIGHT / 2, PIN_RADIUS, 0, Math.PI * 2);
        c.fill();
        c.strokeStyle = COL_PIN_RING;
        c.lineWidth = 1;
        c.stroke();
    }

    function drawEdge(
        c: CanvasRenderingContext2D,
        x1: number,
        y1: number,
        x2: number,
        y2: number,
        selected: boolean,
        condCount: number,
    ) {
        const cpOffset = Math.max(40, Math.abs(x2 - x1) * 0.4);
        c.strokeStyle = selected ? COL_EDGE_SELECTED : COL_EDGE;
        c.lineWidth = selected ? 2.5 : 1.5;
        c.beginPath();
        c.moveTo(x1, y1);
        c.bezierCurveTo(x1 + cpOffset, y1, x2 - cpOffset, y2, x2, y2);
        c.stroke();

        // Arrowhead
        const t = 0.95;
        const ax = bezierX(x1, x1 + cpOffset, x2 - cpOffset, x2, t);
        const ay = bezierY(y1, y1, y2, y2, t);
        const bx = bezierX(x1, x1 + cpOffset, x2 - cpOffset, x2, t - 0.02);
        const by = bezierY(y1, y1, y2, y2, t - 0.02);
        const angle = Math.atan2(ay - by, ax - bx);
        c.fillStyle = selected ? COL_EDGE_SELECTED : COL_EDGE;
        c.beginPath();
        c.moveTo(x2, y2);
        c.lineTo(
            x2 - ARROW_SIZE * Math.cos(angle - Math.PI / 6),
            y2 - ARROW_SIZE * Math.sin(angle - Math.PI / 6),
        );
        c.lineTo(
            x2 - ARROW_SIZE * Math.cos(angle + Math.PI / 6),
            y2 - ARROW_SIZE * Math.sin(angle + Math.PI / 6),
        );
        c.closePath();
        c.fill();

        // Condition badge
        if (condCount > 0) {
            const mx = bezierX(x1, x1 + cpOffset, x2 - cpOffset, x2, 0.5);
            const my = bezierY(y1, y1, y2, y2, 0.5);
            c.fillStyle = COL_PIN_RING;
            c.beginPath();
            c.arc(mx, my - 10, 8, 0, Math.PI * 2);
            c.fill();
            c.fillStyle = COL_NODE_TEXT;
            c.font = "bold 9px sans-serif";
            c.textAlign = "center";
            c.textBaseline = "middle";
            c.fillText(String(condCount), mx, my - 10);
            c.textAlign = "start";
        }
    }

    function bezierX(
        x0: number,
        x1: number,
        x2: number,
        x3: number,
        t: number,
    ): number {
        const mt = 1 - t;
        return (
            mt * mt * mt * x0 +
            3 * mt * mt * t * x1 +
            3 * mt * t * t * x2 +
            t * t * t * x3
        );
    }
    function bezierY(
        y0: number,
        y1: number,
        y2: number,
        y3: number,
        t: number,
    ): number {
        const mt = 1 - t;
        return (
            mt * mt * mt * y0 +
            3 * mt * mt * t * y1 +
            3 * mt * t * t * y2 +
            t * t * t * y3
        );
    }

    function roundRect(
        c: CanvasRenderingContext2D,
        x: number,
        y: number,
        w: number,
        h: number,
        r: number,
    ) {
        c.beginPath();
        c.moveTo(x + r, y);
        c.lineTo(x + w - r, y);
        c.quadraticCurveTo(x + w, y, x + w, y + r);
        c.lineTo(x + w, y + h - r);
        c.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
        c.lineTo(x + r, y + h);
        c.quadraticCurveTo(x, y + h, x, y + h - r);
        c.lineTo(x, y + r);
        c.quadraticCurveTo(x, y, x + r, y);
        c.closePath();
    }

    // ---------------------------------------------------------------------------
    // Interaction handlers
    // ---------------------------------------------------------------------------
    function onPointerDown(e: PointerEvent) {
        if (!canvasEl) return;
        showContextMenu = false;
        const rect = canvasEl.getBoundingClientRect();
        const sx = e.clientX - rect.left;
        const sy = e.clientY - rect.top;
        const [gx, gy] = screenToGraph(sx, sy);

        if (e.button === 1 || (e.button === 0 && e.altKey)) {
            // Middle click or alt+click = pan
            isPanning = true;
            panStart = [e.clientX, e.clientY];
            panStartOffset = [...panOffset];
            canvasEl.setPointerCapture(e.pointerId);
            return;
        }

        if (e.button === 0) {
            // Check if clicking output pin (right side of node) to start connection
            const layout = get(stateMachineLayout);
            for (const [name, pos] of Object.entries(layout)) {
                const px = pos.x + NODE_WIDTH;
                const py = pos.y + NODE_HEIGHT / 2;
                if (Math.hypot(gx - px, gy - py) < PIN_RADIUS * 2) {
                    connectingFrom = name;
                    connectPreview = [sx, sy];
                    smSelectedState.set(null);
                    smSelectedTransition.set(null);
                    canvasEl.setPointerCapture(e.pointerId);
                    return;
                }
            }

            // Check node hit
            const hitNode = hitTestNode(gx, gy);
            if (hitNode) {
                smSelectedState.set(hitNode);
                smSelectedTransition.set(null);
                draggingNode = hitNode;
                dragStart = [sx, sy];
                const pos = layout[hitNode];
                dragNodeStart = [pos.x, pos.y];
                canvasEl.setPointerCapture(e.pointerId);
                draw();
                return;
            }

            // Check edge hit
            const hitEdge = hitTestEdge(gx, gy);
            if (hitEdge) {
                smSelectedTransition.set(hitEdge);
                smSelectedState.set(null);
                draw();
                return;
            }

            // Click on empty space — deselect
            smSelectedState.set(null);
            smSelectedTransition.set(null);
            draw();
        }
    }

    function onPointerMove(e: PointerEvent) {
        if (!canvasEl) return;
        const rect = canvasEl.getBoundingClientRect();
        const sx = e.clientX - rect.left;
        const sy = e.clientY - rect.top;

        if (isPanning) {
            panOffset = [
                panStartOffset[0] + (e.clientX - panStart[0]),
                panStartOffset[1] + (e.clientY - panStart[1]),
            ];
            draw();
            return;
        }

        if (connectingFrom) {
            connectPreview = [sx, sy];
            draw();
            return;
        }

        if (draggingNode) {
            const dx = (sx - dragStart[0]) / zoom;
            const dy = (sy - dragStart[1]) / zoom;
            stateMachineLayout.update((l) => {
                l[draggingNode!] = {
                    x: dragNodeStart[0] + dx,
                    y: dragNodeStart[1] + dy,
                };
                return l;
            });
            draw();
        }
    }

    function onPointerUp(e: PointerEvent) {
        if (!canvasEl) return;

        if (connectingFrom) {
            const rect = canvasEl.getBoundingClientRect();
            const sx = e.clientX - rect.left;
            const sy = e.clientY - rect.top;
            const [gx, gy] = screenToGraph(sx, sy);
            const targetNode = hitTestNode(gx, gy);
            if (targetNode && targetNode !== connectingFrom) {
                const from = connectingFrom;
                executeSM("Add transition", (data) => {
                    const state = data.states[from];
                    if (!state) return;
                    // Don't add duplicate
                    if (
                        !state.transitions.some((t) => t.target === targetNode)
                    ) {
                        state.transitions.push({
                            target: targetNode,
                            conditions: [],
                        });
                    }
                });
            }
            connectingFrom = null;
            connectPreview = null;
            draw();
        }

        isPanning = false;
        draggingNode = null;
        canvasEl.releasePointerCapture(e.pointerId);
    }

    function onWheel(e: WheelEvent) {
        e.preventDefault();
        if (!canvasEl) return;
        const rect = canvasEl.getBoundingClientRect();
        const sx = e.clientX - rect.left;
        const sy = e.clientY - rect.top;
        const oldZoom = zoom;
        const delta = e.deltaY > 0 ? 0.9 : 1.1;
        zoom = Math.max(0.2, Math.min(3, zoom * delta));
        // Zoom toward cursor
        panOffset = [
            sx - (sx - panOffset[0]) * (zoom / oldZoom),
            sy - (sy - panOffset[1]) * (zoom / oldZoom),
        ];
        draw();
    }

    function onContextMenu(e: MouseEvent) {
        e.preventDefault();
        if (!canvasEl) return;
        const rect = canvasEl.getBoundingClientRect();
        const sx = e.clientX - rect.left;
        const sy = e.clientY - rect.top;
        contextMenuPos = [e.clientX - rect.left, e.clientY - rect.top];
        contextMenuGraph = screenToGraph(sx, sy);
        showContextMenu = true;
    }

    function onKeyDown(e: KeyboardEvent) {
        if (e.ctrlKey && e.key === "z" && !e.shiftKey) {
            e.preventDefault();
            undoSM();
            draw();
        } else if (
            e.ctrlKey &&
            (e.key === "y" || (e.key === "z" && e.shiftKey))
        ) {
            e.preventDefault();
            redoSM();
            draw();
        } else if (e.key === "Delete" || e.key === "Backspace") {
            e.preventDefault();
            deleteSelected();
        } else if (
            (e.key === "f" || e.key === "F") &&
            !e.ctrlKey &&
            !e.metaKey &&
            !e.altKey
        ) {
            zoomToFit();
        }
    }

    // ---------------------------------------------------------------------------
    // Actions
    // ---------------------------------------------------------------------------
    function addState(gx: number, gy: number) {
        let name = newStateName.trim() || "NewState";
        const data = get(stateMachineData);
        if (!data) return;
        // Ensure unique name
        let counter = 1;
        let uniqueName = name;
        while (data.states[uniqueName]) {
            uniqueName = `${name}_${counter++}`;
        }
        name = uniqueName;
        executeSM(`Add state "${name}"`, (d) => {
            d.states[name] = { clip_name: "", transitions: [] };
        });
        stateMachineLayout.update((l) => {
            l[name] = { x: gx, y: gy };
            return l;
        });
        smSelectedState.set(name);
        smSelectedTransition.set(null);
        newStateName = "";
        showContextMenu = false;
        draw();
    }

    function deleteSelected() {
        const selState = get(smSelectedState);
        const selTr = get(smSelectedTransition);
        if (selState) {
            executeSM(`Delete state "${selState}"`, (data) => {
                delete data.states[selState];
                // Remove transitions pointing to this state
                for (const state of Object.values(data.states)) {
                    state.transitions = state.transitions.filter(
                        (t) => t.target !== selState,
                    );
                }
                if (data.current_state === selState) {
                    data.current_state = undefined;
                }
            });
            stateMachineLayout.update((l) => {
                delete l[selState];
                return l;
            });
            smSelectedState.set(null);
            draw();
        } else if (selTr) {
            const [fromState, ti] = selTr;
            executeSM("Delete transition", (data) => {
                data.states[fromState]?.transitions.splice(ti, 1);
            });
            smSelectedTransition.set(null);
            draw();
        }
    }

    function setCurrentState(name: string) {
        executeSM("Set current state", (data) => {
            data.current_state = name;
        });
        draw();
    }

    function renameState(oldName: string, newName: string) {
        if (!newName.trim() || oldName === newName) return;
        const data = get(stateMachineData);
        if (!data || data.states[newName]) return;
        executeSM(`Rename "${oldName}" → "${newName}"`, (d) => {
            d.states[newName] = d.states[oldName];
            delete d.states[oldName];
            // Update all transitions targeting old name
            for (const state of Object.values(d.states)) {
                for (const tr of state.transitions) {
                    if (tr.target === oldName) tr.target = newName;
                }
            }
            if (d.current_state === oldName) d.current_state = newName;
        });
        stateMachineLayout.update((l) => {
            l[newName] = l[oldName];
            delete l[oldName];
            return l;
        });
        smSelectedState.set(newName);
        draw();
    }

    function addParameter() {
        const name = newParamName.trim();
        if (!name) return;
        const ptype = newParamType;
        executeSM(`Add param "${name}"`, (data) => {
            if (!data.parameters) data.parameters = {};
            if (data.parameters[name]) return;
            if (ptype === "Bool")
                data.parameters[name] = { type: "Bool", value: false };
            else if (ptype === "Float")
                data.parameters[name] = { type: "Float", value: 0 };
            else data.parameters[name] = { type: "Trigger" };
        });
        newParamName = "";
    }

    function removeParameter(name: string) {
        executeSM(`Remove param "${name}"`, (data) => {
            if (data.parameters) delete data.parameters[name];
        });
    }

    function addCondition(fromState: string, ti: number) {
        const ctype = newCondType;
        executeSM("Add condition", (data) => {
            const tr = data.states[fromState]?.transitions[ti];
            if (!tr) return;
            if (ctype === "BoolEquals") {
                tr.conditions.push({
                    type: "BoolEquals",
                    param: "",
                    value: true,
                });
            } else if (ctype === "FloatGreaterThan") {
                tr.conditions.push({
                    type: "FloatGreaterThan",
                    param: "",
                    threshold: 0,
                });
            } else if (ctype === "FloatLessThan") {
                tr.conditions.push({
                    type: "FloatLessThan",
                    param: "",
                    threshold: 0,
                });
            } else if (ctype === "ClipFinished") {
                tr.conditions.push({ type: "ClipFinished" });
            } else if (ctype === "TriggerSet") {
                tr.conditions.push({ type: "TriggerSet", param: "" });
            }
        });
        draw();
    }

    function removeCondition(fromState: string, ti: number, ci: number) {
        executeSM("Remove condition", (data) => {
            data.states[fromState]?.transitions[ti]?.conditions.splice(ci, 1);
        });
        draw();
    }

    function zoomToFit() {
        const layout = get(stateMachineLayout);
        const names = Object.keys(layout);
        if (names.length === 0) return;
        let minX = Infinity,
            minY = Infinity,
            maxX = -Infinity,
            maxY = -Infinity;
        for (const pos of Object.values(layout)) {
            minX = Math.min(minX, pos.x);
            minY = Math.min(minY, pos.y);
            maxX = Math.max(maxX, pos.x + NODE_WIDTH);
            maxY = Math.max(maxY, pos.y + NODE_HEIGHT);
        }
        const padding = 60;
        const w = maxX - minX + padding * 2;
        const h = maxY - minY + padding * 2;
        zoom = Math.min(1.5, Math.min(width / w, height / h));
        panOffset = [
            (width - w * zoom) / 2 - minX * zoom + padding * zoom,
            (height - h * zoom) / 2 - minY * zoom + padding * zoom,
        ];
        draw();
    }

    function applyToEntity() {
        const data = get(stateMachineData);
        if (!data) return;
        // Dispatch event so Inspector / App can apply data to the entity
        window.dispatchEvent(
            new CustomEvent("statemachine-apply", {
                detail: {
                    entityId: get(stateMachineEntityId),
                    data: structuredClone(data),
                },
            }),
        );
        stateMachineDirty.set(false);
    }

    /** Condition type display labels. */
    function condLabel(c: TransitionCondition): string {
        switch (c.type) {
            case "BoolEquals":
                return `${c.param} == ${c.value}`;
            case "FloatGreaterThan":
                return `${c.param} > ${c.threshold}`;
            case "FloatLessThan":
                return `${c.param} < ${c.threshold}`;
            case "ClipFinished":
                return "Clip Finished";
            case "TriggerSet":
                return `trigger(${c.param})`;
        }
    }

    // ---------------------------------------------------------------------------
    // Lifecycle
    // ---------------------------------------------------------------------------
    onMount(() => {
        if (!canvasEl) return;
        ctx = canvasEl.getContext("2d");
        const parent = canvasEl.parentElement!;
        width = parent.clientWidth;
        height = parent.clientHeight;
        canvasEl.width = width;
        canvasEl.height = height;

        resizeObserver = new ResizeObserver((entries) => {
            for (const entry of entries) {
                const { width: w, height: h } = entry.contentRect;
                width = Math.round(w);
                height = Math.round(h);
                if (canvasEl && width > 0 && height > 0) {
                    canvasEl.width = width;
                    canvasEl.height = height;
                    draw();
                }
            }
        });
        resizeObserver.observe(parent);

        // Initial draw after tick
        requestAnimationFrame(() => {
            zoomToFit();
            draw();
        });
    });

    onDestroy(() => {
        resizeObserver?.disconnect();
    });

    // Redraw when store changes
    $effect(() => {
        // Subscribe to all relevant stores
        $stateMachineData;
        $stateMachineLayout;
        $smSelectedState;
        $smSelectedTransition;
        $themeColors;
        draw();
    });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="sm-editor" onkeydown={onKeyDown} tabindex="-1">
    <!-- Left panel: properties -->
    <div class="sm-properties">
        <div class="panel-toolbar">
            <button
                class="tool-btn"
                title="Undo (Ctrl+Z)"
                disabled={!$canUndoSM}
                onclick={() => {
                    undoSM();
                    draw();
                }}>↩</button
            >
            <button
                class="tool-btn"
                title="Redo (Ctrl+Y)"
                disabled={!$canRedoSM}
                onclick={() => {
                    redoSM();
                    draw();
                }}>↪</button
            >
            <div class="toolbar-spacer"></div>
            <button
                class="tool-btn"
                title="Apply to entity"
                disabled={!$stateMachineDirty}
                onclick={applyToEntity}>💾</button
            >
            <button
                class="tool-btn"
                title="Close"
                onclick={() => requestEditorClose("statemachine")}>✕</button
            >
        </div>

        <div class="panel-scroll">
            <!-- Selected State properties -->
            <button
                class="section-toggle"
                onclick={() => (showProperties = !showProperties)}
            >
                {showProperties ? "▼" : "▶"} Properties
            </button>
            {#if showProperties && $stateMachineData}
                <div class="section">
                    {#if $smSelectedState && $stateMachineData.states[$smSelectedState]}
                        {@const selName = $smSelectedState}
                        {@const selState = $stateMachineData.states[selName]}
                        <div class="info-row">
                            <strong>State: {selName}</strong>
                        </div>
                        <label>
                            Name
                            <input
                                type="text"
                                value={selName}
                                onchange={(e) =>
                                    renameState(
                                        selName,
                                        (e.target as HTMLInputElement).value,
                                    )}
                            />
                        </label>
                        <label>
                            Clip Name
                            <input
                                type="text"
                                value={selState.clip_name}
                                onchange={(e) => {
                                    const n = selName;
                                    const val = (e.target as HTMLInputElement)
                                        .value;
                                    executeSM("Set clip name", (d) => {
                                        d.states[n].clip_name = val;
                                    });
                                    draw();
                                }}
                            />
                        </label>
                        <button
                            class="action-btn"
                            onclick={() => setCurrentState(selName)}
                        >
                            ▶ Set as Initial State
                        </button>
                        <button
                            class="action-btn danger-btn"
                            onclick={deleteSelected}
                        >
                            🗑 Delete State
                        </button>

                        <!-- Transitions from this state -->
                        <div class="sub-header">
                            Transitions ({selState.transitions.length})
                        </div>
                        {#each selState.transitions as tr, ti}
                            <!-- svelte-ignore a11y_click_events_have_key_events -->
                            <div
                                class="transition-row"
                                class:selected={$smSelectedTransition?.[0] ===
                                    selName &&
                                    $smSelectedTransition?.[1] === ti}
                                role="button"
                                tabindex="0"
                                onclick={() => {
                                    smSelectedTransition.set([selName, ti]);
                                    smSelectedState.set(null);
                                    draw();
                                }}
                            >
                                <span class="tr-arrow">→</span>
                                <span class="tr-target">{tr.target}</span>
                                <span class="tr-cond-count"
                                    >{tr.conditions.length}c</span
                                >
                                <button
                                    class="frame-btn danger"
                                    title="Remove"
                                    onclick={(e) => {
                                        e.stopPropagation();
                                        executeSM("Remove transition", (d) => {
                                            d.states[
                                                selName
                                            ]?.transitions.splice(ti, 1);
                                        });
                                        draw();
                                    }}>✕</button
                                >
                            </div>
                        {/each}
                    {:else if $smSelectedTransition}
                        {@const [fromState, ti] = $smSelectedTransition}
                        {@const tr =
                            $stateMachineData.states[fromState]?.transitions[
                                ti
                            ]}
                        {#if tr}
                            <div class="info-row">
                                <strong
                                    >Transition: {fromState} → {tr.target}</strong
                                >
                            </div>

                            <!-- Conditions -->
                            <div class="sub-header">
                                Conditions ({tr.conditions.length})
                            </div>
                            {#each tr.conditions as cond, ci}
                                <div class="condition-row">
                                    <span class="cond-label"
                                        >{condLabel(cond)}</span
                                    >
                                    {#if cond.type === "BoolEquals"}
                                        <input
                                            type="text"
                                            class="cond-input"
                                            placeholder="param"
                                            value={cond.param}
                                            onchange={(e) => {
                                                const fs = fromState;
                                                const tidx = ti;
                                                const cidx = ci;
                                                executeSM(
                                                    "Set condition param",
                                                    (d) => {
                                                        const c =
                                                            d.states[fs]
                                                                ?.transitions[
                                                                tidx
                                                            ]?.conditions[cidx];
                                                        if (
                                                            c &&
                                                            c.type ===
                                                                "BoolEquals"
                                                        )
                                                            c.param = (
                                                                e.target as HTMLInputElement
                                                            ).value;
                                                    },
                                                );
                                                draw();
                                            }}
                                        />
                                        <select
                                            class="cond-select"
                                            value={String(cond.value)}
                                            onchange={(e) => {
                                                const fs = fromState;
                                                const tidx = ti;
                                                const cidx = ci;
                                                const val =
                                                    (
                                                        e.target as HTMLSelectElement
                                                    ).value === "true";
                                                executeSM(
                                                    "Set condition value",
                                                    (d) => {
                                                        const c =
                                                            d.states[fs]
                                                                ?.transitions[
                                                                tidx
                                                            ]?.conditions[cidx];
                                                        if (
                                                            c &&
                                                            c.type ===
                                                                "BoolEquals"
                                                        )
                                                            c.value = val;
                                                    },
                                                );
                                                draw();
                                            }}
                                        >
                                            <option value="true">true</option>
                                            <option value="false">false</option>
                                        </select>
                                    {:else if cond.type === "FloatGreaterThan" || cond.type === "FloatLessThan"}
                                        <input
                                            type="text"
                                            class="cond-input"
                                            placeholder="param"
                                            value={cond.param}
                                            onchange={(e) => {
                                                const fs = fromState;
                                                const tidx = ti;
                                                const cidx = ci;
                                                executeSM(
                                                    "Set condition param",
                                                    (d) => {
                                                        const c =
                                                            d.states[fs]
                                                                ?.transitions[
                                                                tidx
                                                            ]?.conditions[cidx];
                                                        if (
                                                            c &&
                                                            (c.type ===
                                                                "FloatGreaterThan" ||
                                                                c.type ===
                                                                    "FloatLessThan")
                                                        )
                                                            c.param = (
                                                                e.target as HTMLInputElement
                                                            ).value;
                                                    },
                                                );
                                                draw();
                                            }}
                                        />
                                        <input
                                            type="number"
                                            step="0.1"
                                            class="cond-input"
                                            value={cond.threshold}
                                            onchange={(e) => {
                                                const fs = fromState;
                                                const tidx = ti;
                                                const cidx = ci;
                                                executeSM(
                                                    "Set threshold",
                                                    (d) => {
                                                        const c =
                                                            d.states[fs]
                                                                ?.transitions[
                                                                tidx
                                                            ]?.conditions[cidx];
                                                        if (
                                                            c &&
                                                            (c.type ===
                                                                "FloatGreaterThan" ||
                                                                c.type ===
                                                                    "FloatLessThan")
                                                        )
                                                            c.threshold =
                                                                parseFloat(
                                                                    (
                                                                        e.target as HTMLInputElement
                                                                    ).value,
                                                                ) || 0;
                                                    },
                                                );
                                                draw();
                                            }}
                                        />
                                    {:else if cond.type === "TriggerSet"}
                                        <input
                                            type="text"
                                            class="cond-input"
                                            placeholder="param"
                                            value={cond.param}
                                            onchange={(e) => {
                                                const fs = fromState;
                                                const tidx = ti;
                                                const cidx = ci;
                                                executeSM(
                                                    "Set trigger param",
                                                    (d) => {
                                                        const c =
                                                            d.states[fs]
                                                                ?.transitions[
                                                                tidx
                                                            ]?.conditions[cidx];
                                                        if (
                                                            c &&
                                                            c.type ===
                                                                "TriggerSet"
                                                        )
                                                            c.param = (
                                                                e.target as HTMLInputElement
                                                            ).value;
                                                    },
                                                );
                                                draw();
                                            }}
                                        />
                                    {/if}
                                    <button
                                        class="frame-btn danger"
                                        title="Remove condition"
                                        onclick={() =>
                                            removeCondition(fromState, ti, ci)}
                                        >✕</button
                                    >
                                </div>
                            {/each}

                            <!-- Add condition -->
                            <div class="cond-add-row">
                                <select
                                    bind:value={newCondType}
                                    class="trigger-select"
                                    style="width:100px"
                                >
                                    <option value="BoolEquals">Bool ==</option>
                                    <option value="FloatGreaterThan"
                                        >Float &gt;</option
                                    >
                                    <option value="FloatLessThan"
                                        >Float &lt;</option
                                    >
                                    <option value="ClipFinished"
                                        >Clip Done</option
                                    >
                                    <option value="TriggerSet">Trigger</option>
                                </select>
                                <button
                                    class="action-btn"
                                    style="flex:1; margin-top:0"
                                    onclick={() => addCondition(fromState, ti)}
                                    >+ Add</button
                                >
                            </div>

                            <button
                                class="action-btn danger-btn"
                                onclick={deleteSelected}
                            >
                                🗑 Delete Transition
                            </button>
                        {/if}
                    {:else}
                        <div class="info-row">
                            Click a state or transition to edit.
                        </div>
                        <div class="info-row">
                            Drag from ● to ● to create transitions.
                        </div>
                        <div class="info-row">
                            Right-click canvas to add states.
                        </div>
                    {/if}
                </div>
            {/if}

            <!-- Parameters -->
            <button
                class="section-toggle"
                onclick={() => (showParameters = !showParameters)}
            >
                {showParameters ? "▼" : "▶"} Parameters ({Object.keys(
                    $stateMachineData?.parameters ?? {},
                ).length})
            </button>
            {#if showParameters && $stateMachineData}
                <div class="section">
                    {#each Object.entries($stateMachineData.parameters ?? {}) as [pname, pval]}
                        <div class="param-row">
                            <span class="param-name">{pname}</span>
                            <span class="param-type">{pval.type}</span>
                            <button
                                class="frame-btn danger"
                                title="Remove"
                                onclick={() => removeParameter(pname)}>✕</button
                            >
                        </div>
                    {/each}
                    <div class="param-add-row">
                        <input
                            type="text"
                            bind:value={newParamName}
                            placeholder="name"
                            class="trigger-input"
                        />
                        <select
                            bind:value={newParamType}
                            class="trigger-select"
                            style="width:60px"
                        >
                            <option value="Bool">Bool</option>
                            <option value="Float">Float</option>
                            <option value="Trigger">Trig</option>
                        </select>
                        <button
                            class="frame-btn"
                            disabled={!newParamName.trim()}
                            onclick={addParameter}>+</button
                        >
                    </div>
                </div>
            {/if}
        </div>
    </div>

    <!-- Right: Canvas -->
    <div class="sm-canvas-area">
        <div class="sm-toolbar">
            <button class="tool-btn" title="Zoom to fit (F)" onclick={zoomToFit}
                >⊞</button
            >
            <span class="zoom-badge">{Math.round(zoom * 100)}%</span>
            <span class="sm-title"
                >State Machine{$stateMachineDirty ? " •" : ""}</span
            >
        </div>
        <div class="canvas-container">
            <canvas
                bind:this={canvasEl}
                onpointerdown={onPointerDown}
                onpointermove={onPointerMove}
                onpointerup={onPointerUp}
                onwheel={onWheel}
                oncontextmenu={onContextMenu}
            ></canvas>

            <!-- Context menu -->
            {#if showContextMenu}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <div
                    class="context-menu"
                    style="left: {contextMenuPos[0]}px; top: {contextMenuPos[1]}px"
                    role="menu"
                    tabindex="-1"
                >
                    <input
                        type="text"
                        bind:value={newStateName}
                        placeholder="State name"
                        class="ctx-input"
                        onkeydown={(e) => {
                            if (e.key === "Enter")
                                addState(
                                    contextMenuGraph[0],
                                    contextMenuGraph[1],
                                );
                        }}
                    />
                    <button
                        class="ctx-item"
                        onclick={() =>
                            addState(contextMenuGraph[0], contextMenuGraph[1])}
                    >
                        + Add State
                    </button>
                    <button
                        class="ctx-item"
                        onclick={() => {
                            showContextMenu = false;
                            zoomToFit();
                        }}
                    >
                        ⊞ Zoom to Fit
                    </button>
                </div>
            {/if}
        </div>
    </div>
</div>

<style>
    .sm-editor {
        display: flex;
        width: 100%;
        height: 100%;
        background: var(--ed-base);
        color: var(--ed-text);
        font-size: 12px;
        outline: none;
    }

    /* Left panel */
    .sm-properties {
        width: 280px;
        min-width: 280px;
        border-right: 1px solid var(--ed-line);
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .panel-toolbar {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 6px 8px;
        border-bottom: 1px solid var(--ed-line);
        background: var(--ed-mantle);
    }

    .toolbar-spacer {
        flex: 1;
    }

    .panel-scroll {
        flex: 1;
        overflow-y: auto;
        padding-bottom: 16px;
    }

    .section-toggle {
        display: block;
        width: 100%;
        text-align: left;
        padding: 6px 8px;
        background: var(--ed-mantle);
        border: none;
        border-bottom: 1px solid var(--ed-line);
        color: var(--ed-text);
        cursor: pointer;
        font-size: 11px;
        font-weight: 600;
    }

    .section-toggle:hover {
        background: var(--ed-hover);
    }

    .section {
        padding: 8px;
        border-bottom: 1px solid var(--ed-line);
    }

    .section label {
        display: flex;
        flex-direction: column;
        gap: 2px;
        margin-bottom: 6px;
        font-size: 11px;
        color: var(--ed-muted);
    }

    .section input[type="number"],
    .section input[type="text"],
    .section select {
        width: 100%;
        padding: 4px 6px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 12px;
        box-sizing: border-box;
    }

    .info-row {
        font-size: 11px;
        color: var(--ed-muted);
        padding: 4px 0;
    }

    .sub-header {
        font-size: 11px;
        font-weight: 600;
        margin-top: 8px;
        margin-bottom: 4px;
        color: var(--ed-muted);
        border-top: 1px solid var(--ed-line);
        padding-top: 6px;
    }

    .action-btn {
        width: 100%;
        padding: 6px 8px;
        background: var(--ed-mantle);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        cursor: pointer;
        font-size: 11.5px;
        margin-top: 4px;
    }

    .action-btn:hover {
        background: var(--ed-hover);
    }
    .action-btn:disabled {
        opacity: 0.4;
        cursor: default;
    }

    .danger-btn {
        color: var(--ed-err);
    background: color-mix(in srgb, var(--ed-err) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--ed-err) 50%, transparent);
  }
    .danger-btn:hover {
        background: color-mix(in srgb, var(--ed-err) 10%, transparent);
    }

    /* Transition list */
    .transition-row {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 3px 6px;
        border-radius: 4px;
        background: var(--ed-raised);
        margin-bottom: 2px;
        cursor: pointer;
        font-size: 11px;
    }

    .transition-row:hover {
        background: var(--ed-hover);
    }
    .transition-row.selected {
        border: 1px solid var(--ed-accent);
        background: color-mix(in srgb, var(--ed-accent) 10%, transparent);
    }

    .tr-arrow {
        color: var(--ed-muted);
    }
    .tr-target {
        flex: 1;
    }
    .tr-cond-count {
        color: var(--ed-muted);
        font-size: 10px;
    }

    /* Conditions */
    .condition-row {
        display: flex;
        align-items: center;
        gap: 3px;
        padding: 2px 4px;
        background: var(--ed-raised);
        border-radius: 3px;
        margin-bottom: 2px;
        font-size: 10px;
        flex-wrap: wrap;
    }

    .cond-label {
        flex: 1;
        min-width: 60px;
        color: var(--ed-muted);
    }

    .cond-input {
        width: 60px;
        padding: 2px 4px;
        background: var(--ed-base);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 10px;
    }

    .cond-select {
        padding: 2px 4px;
        background: var(--ed-base);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 10px;
    }

    .cond-add-row {
        display: flex;
        gap: 2px;
        margin-top: 4px;
    }

    /* Parameters */
    .param-row {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 3px 6px;
        background: var(--ed-raised);
        border-radius: 4px;
        margin-bottom: 2px;
        font-size: 11px;
    }

    .param-name {
        flex: 1;
        font-weight: 600;
    }
    .param-type {
        color: var(--ed-muted);
        font-size: 10px;
    }

    .param-add-row {
        display: flex;
        gap: 2px;
        margin-top: 4px;
    }

    /* Shared */
    .tool-btn {
        padding: 4px 8px;
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        background: var(--ed-mantle);
        color: var(--ed-text);
        cursor: pointer;
        font-size: 12px;
        line-height: 1;
    }

    .tool-btn:hover {
        background: var(--ed-hover);
    }
    .tool-btn:disabled {
        opacity: 0.4;
        cursor: default;
    }

    .frame-btn {
        padding: 1px 4px;
        font-size: 10px;
        border: none;
        border-radius: 4px;
        background: transparent;
        color: var(--ed-muted);
        cursor: pointer;
    }

    .frame-btn:hover {
        background: var(--ed-hover);
        color: var(--ed-text);
    }
    .frame-btn.danger:hover {
        color: var(--ed-err);
    }

    .trigger-select {
        width: 64px;
        padding: 2px 4px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 11px;
    }

    .trigger-input {
        flex: 1;
        padding: 2px 4px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 11px;
        min-width: 0;
    }

    /* Canvas area */
    .sm-canvas-area {
        flex: 1;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .sm-toolbar {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 6px 8px;
        border-bottom: 1px solid var(--ed-line);
        background: var(--ed-mantle);
    }

    .zoom-badge {
        padding: 2px 8px;
        border-radius: 8px;
        background: var(--ed-raised);
        font-size: 11px;
        font-family: monospace;
    }

    .sm-title {
        margin-left: auto;
        font-size: 11px;
        color: var(--ed-muted);
    }

    .canvas-container {
        flex: 1;
        position: relative;
        overflow: hidden;
    }

    .canvas-container canvas {
        position: absolute;
        inset: 0;
        width: 100%;
        height: 100%;
        cursor: default;
    }

    /* Context menu */
    .context-menu {
        position: absolute;
        background: var(--ed-mantle);
        border: 1px solid var(--ed-line);
        border-radius: 7px;
        padding: 4px;
        min-width: 160px;
        z-index: 100;
        box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
    }

    .ctx-input {
        width: 100%;
        padding: 4px 6px;
        margin-bottom: 4px;
        background: var(--ed-raised);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        font-size: 12px;
        box-sizing: border-box;
    }

    .ctx-item {
        display: block;
        width: 100%;
        text-align: left;
        padding: 6px 8px;
        border: none;
        border-radius: 4px;
        background: transparent;
        color: var(--ed-text);
        cursor: pointer;
        font-size: 12px;
    }

    .ctx-item:hover {
        background: var(--ed-hover);
    }
</style>
