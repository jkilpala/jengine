<script lang="ts" module>
    function rgbaToHex(rgba: [number, number, number, number]): string {
        const r = Math.round(rgba[0] * 255)
            .toString(16)
            .padStart(2, "0");
        const g = Math.round(rgba[1] * 255)
            .toString(16)
            .padStart(2, "0");
        const b = Math.round(rgba[2] * 255)
            .toString(16)
            .padStart(2, "0");
        return `#${r}${g}${b}`;
    }

    function hexToRgb(hex: string): [number, number, number] {
        const r = parseInt(hex.slice(1, 3), 16) / 255;
        const g = parseInt(hex.slice(3, 5), 16) / 255;
        const b = parseInt(hex.slice(5, 7), 16) / 255;
        return [r, g, b];
    }
</script>

<script lang="ts">
    import { SettledHistory } from "./settledHistory";
    import { claimUndo } from "./stores/undoTargetStore";
    import type {
        ShaderGraph,
        ShaderNode,
        Edge,
        NodeType,
        PaletteCategory,
        PinInfo,
    } from "./api/shaderEditor";
    import {
        shaderGetPalette,
        shaderGenerate,
        shaderValidate,
        shaderSave,
        shaderGetNodePins,
    } from "./api/shaderEditor";
    import { logInfo, logError, logWarn } from "./stores/consoleStore";
    import { themeColors } from "./stores/themeColors";
    import {
        shaderEditorGraph,
        shaderSelectedNodeId,
        shaderRequestDraw,
        shaderMarkDirty,
    } from "./stores/shaderEditorStore";

    let {
        graph = $bindable(),
        projectPath,
        relativePath,
        onclose,
    }: {
        graph: ShaderGraph;
        projectPath?: string;
        relativePath?: string;
        onclose?: () => void;
    } = $props();

    // ─── State ───────────────────────────────────────────────────────

    let canvas: HTMLCanvasElement | null = $state(null);
    let palette: PaletteCategory[] = $state([]);
    let selectedNodeId: number | null = $state(null);

    // Sync shader editor state to stores for Inspector access
    $effect(() => {
        shaderEditorGraph.set(graph);
        shaderRequestDraw.set(() => requestDraw());
        shaderMarkDirty.set(() => {
            dirty = true;
        });
        return () => {
            shaderEditorGraph.set(null);
            shaderSelectedNodeId.set(null);
            shaderRequestDraw.set(null);
            shaderMarkDirty.set(null);
        };
    });
    $effect(() => {
        shaderSelectedNodeId.set(selectedNodeId);
    });
    let draggingNode: number | null = $state(null);
    let dragOffset: [number, number] = $state([0, 0]);
    let connectingFrom: {
        nodeId: number;
        pinIdx: number;
        isOutput: boolean;
    } | null = $state(null);
    let mousePos: [number, number] = $state([0, 0]);
    let panOffset: [number, number] = $state([0, 0]);
    let zoom = $state(1);
    let isPanning = $state(false);
    let panStart: [number, number] = $state([0, 0]);
    let showPalette = $state(false);
    let paletteFilter = $state("");
    let glslPreview = $state("");
    let showGlsl = $state(false);
    let validationErrors: string[] = $state([]);
    let dirty = $state(false);

    // Undo: the graph, a step each time it settles after a change - a node
    // dragged, a pin wired, a value typed (`settledHistory.ts`). Ctrl+Z /
    // Ctrl+Y are this editor's while it is open.
    const history = new SettledHistory<ShaderGraph>(
        () => $state.snapshot(graph) as ShaderGraph,
        (v) => {
            graph = v;
            dirty = true;
            requestDraw();
        },
    );
    $effect(() => {
        JSON.stringify(graph);
        history.observe();
    });
    // Another shader opened in the same editor starts a history of its own.
    $effect(() => {
        void relativePath;
        history.reset();
    });
    $effect(() => claimUndo({ undo: () => history.undo(), redo: () => history.redo() }));
    let nodeCache = $state(
        new Map<number, { inputs: PinInfo[]; outputs: PinInfo[] }>(),
    );

    // ─── Constants ───────────────────────────────────────────────────

    const NODE_WIDTH = 180;
    const NODE_HEADER_HEIGHT = 28;
    const PIN_HEIGHT = 22;
    const PIN_RADIUS = 6;
    const PREVIEW_SIZE = 64;
    const PREVIEW_PAD = 6;
    // A pin's colour names its data type; taken from the palette, so each
    // type keeps a hue of its own in every theme.
    const PIN_COLORS: Record<string, string> = $derived({
        Float: $themeColors["cat-4"],
        Vec2: $themeColors["cat-2"],
        Vec3: $themeColors["cat-1"],
        Vec4: $themeColors["cat-8"],
        Sampler2D: $themeColors["cat-3"],
    });

    // ─── Palette Loading ─────────────────────────────────────────────

    $effect(() => {
        shaderGetPalette().then((p) => (palette = p));
    });

    // ─── Build pin cache from graph nodes ─────────────────────────────

    async function buildNodeCache() {
        const cache = new Map<
            number,
            { inputs: PinInfo[]; outputs: PinInfo[] }
        >();
        for (const node of graph.nodes) {
            try {
                const pins = await shaderGetNodePins(
                    JSON.stringify(node.node_type),
                );
                cache.set(node.id, pins);
            } catch {
                cache.set(node.id, { inputs: [], outputs: [] });
            }
        }
        nodeCache = cache;
    }

    $effect(() => {
        // Rebuild cache whenever nodes change.
        if (graph.nodes) {
            buildNodeCache();
        }
    });

    // ─── Canvas Drawing ──────────────────────────────────────────────

    function nodeHasPreview(node: ShaderNode): boolean {
        const t = node.node_type.type;
        return (
            t === "FloatConstant" ||
            t === "Vec2Constant" ||
            t === "Vec3Constant" ||
            t === "Vec4Constant" ||
            t === "UvInput" ||
            t === "ColorInput" ||
            t === "TextureSample" ||
            t === "FragmentOutput" ||
            t === "Add" ||
            t === "Subtract" ||
            t === "Multiply" ||
            t === "Divide" ||
            t === "Mix" ||
            t === "Step" ||
            t === "SmoothStep" ||
            t === "Clamp" ||
            t === "Abs" ||
            t === "Negate" ||
            t === "Floor" ||
            t === "Ceil" ||
            t === "Fract" ||
            t === "Sin" ||
            t === "Cos" ||
            t === "Normalize" ||
            t === "Length" ||
            t === "Dot" ||
            t === "Cross" ||
            t === "Pow" ||
            t === "Sqrt" ||
            t === "Min" ||
            t === "Max" ||
            t === "Mod" ||
            t === "Split" ||
            t === "Combine" ||
            t === "Swizzle" ||
            t === "CustomGlsl" ||
            t === "TimeInput" ||
            t === "PositionInput" ||
            t === "ScreenPosition" ||
            t === "WorldToScreen"
        );
    }

    function getNodeHeight(nodeId: number): number {
        const pins = nodeCache.get(nodeId);
        if (!pins) return NODE_HEADER_HEIGHT + PIN_HEIGHT;
        const maxPins = Math.max(pins.inputs.length, pins.outputs.length, 1);
        const node = graph.nodes.find((n) => n.id === nodeId);
        const preview =
            node && nodeHasPreview(node) ? PREVIEW_SIZE + PREVIEW_PAD * 2 : 0;
        return NODE_HEADER_HEIGHT + maxPins * PIN_HEIGHT + 8 + preview;
    }

    function getPinPos(
        nodeId: number,
        pinIdx: number,
        isOutput: boolean,
    ): [number, number] {
        const node = graph.nodes.find((n) => n.id === nodeId);
        if (!node) return [0, 0];
        const x = isOutput ? node.position[0] + NODE_WIDTH : node.position[0];
        const y =
            node.position[1] +
            NODE_HEADER_HEIGHT +
            pinIdx * PIN_HEIGHT +
            PIN_HEIGHT / 2;
        return [x, y];
    }

    function draw() {
        if (!canvas) return;
        const ctx = canvas.getContext("2d");
        if (!ctx) return;

        const w = canvas.width;
        const h = canvas.height;

        ctx.clearRect(0, 0, w, h);
        ctx.save();
        ctx.translate(panOffset[0], panOffset[1]);
        ctx.scale(zoom, zoom);

        // Draw grid.
        drawGrid(ctx, w, h);

        // Draw edges.
        for (const edge of graph.edges) {
            drawEdge(ctx, edge);
        }

        // Draw "connecting" preview line.
        if (connectingFrom) {
            const from = getPinPos(
                connectingFrom.nodeId,
                connectingFrom.pinIdx,
                connectingFrom.isOutput,
            );
            const toScreen = screenToGraph(mousePos[0], mousePos[1]);
            drawBezier(
                ctx,
                from[0],
                from[1],
                toScreen[0],
                toScreen[1],
                $themeColors.accent + "80",
            );
        }

        // Draw nodes.
        for (const node of graph.nodes) {
            drawNode(ctx, node);
        }

        ctx.restore();
    }

    function drawGrid(ctx: CanvasRenderingContext2D, w: number, h: number) {
        const gridSize = 40;
        ctx.strokeStyle = $themeColors.line + "80";
        ctx.lineWidth = 1 / zoom;

        const topLeft = screenToGraph(0, 0);
        const bottomRight = screenToGraph(w, h);

        const startX = Math.floor(topLeft[0] / gridSize) * gridSize;
        const startY = Math.floor(topLeft[1] / gridSize) * gridSize;

        for (let x = startX; x <= bottomRight[0]; x += gridSize) {
            ctx.beginPath();
            ctx.moveTo(x, topLeft[1]);
            ctx.lineTo(x, bottomRight[1]);
            ctx.stroke();
        }
        for (let y = startY; y <= bottomRight[1]; y += gridSize) {
            ctx.beginPath();
            ctx.moveTo(topLeft[0], y);
            ctx.lineTo(bottomRight[0], y);
            ctx.stroke();
        }
    }

    function drawNode(ctx: CanvasRenderingContext2D, node: ShaderNode) {
        const x = node.position[0];
        const y = node.position[1];
        const h = getNodeHeight(node.id);
        const isSelected = selectedNodeId === node.id;
        const isOutput =
            node.node_type.type === "FragmentOutput" ||
            node.node_type.type === "VertexOffset";

        // Node body.
        const t = $themeColors;
        ctx.fillStyle = isOutput ? t["accent-low"] : t.raised;
        ctx.strokeStyle = isSelected ? t.accent : t.line;
        ctx.lineWidth = isSelected ? 2 / zoom : 1 / zoom;

        roundRect(ctx, x, y, NODE_WIDTH, h, 6);
        ctx.fill();
        ctx.stroke();

        // Header.
        ctx.fillStyle = isOutput ? t["accent-low"] : t.line;
        roundRectTop(ctx, x, y, NODE_WIDTH, NODE_HEADER_HEIGHT, 6);
        ctx.fill();

        // Title.
        ctx.fillStyle = t.ink;
        ctx.font = `${12 / zoom < 8 ? 8 : 12}px "IBM Plex Sans", system-ui`;
        ctx.textBaseline = "middle";
        const label = node.label || nodeTypeLabel(node.node_type);
        ctx.fillText(label, x + 8, y + NODE_HEADER_HEIGHT / 2, NODE_WIDTH - 16);

        // Pins.
        const pins = nodeCache.get(node.id);
        if (!pins) return;

        for (let i = 0; i < pins.inputs.length; i++) {
            const py = y + NODE_HEADER_HEIGHT + i * PIN_HEIGHT + PIN_HEIGHT / 2;
            const color = PIN_COLORS[pins.inputs[i].pin_type] || t.muted;
            ctx.fillStyle = color;
            ctx.beginPath();
            ctx.arc(x, py, PIN_RADIUS / zoom, 0, Math.PI * 2);
            ctx.fill();

            ctx.fillStyle = t.muted;
            ctx.textAlign = "left";
            ctx.fillText(pins.inputs[i].name, x + PIN_RADIUS + 4, py);
        }

        for (let i = 0; i < pins.outputs.length; i++) {
            const py = y + NODE_HEADER_HEIGHT + i * PIN_HEIGHT + PIN_HEIGHT / 2;
            const color = PIN_COLORS[pins.outputs[i].pin_type] || t.muted;
            ctx.fillStyle = color;
            ctx.beginPath();
            ctx.arc(x + NODE_WIDTH, py, PIN_RADIUS / zoom, 0, Math.PI * 2);
            ctx.fill();

            ctx.fillStyle = t.muted;
            ctx.textAlign = "right";
            ctx.fillText(
                pins.outputs[i].name,
                x + NODE_WIDTH - PIN_RADIUS - 4,
                py,
            );
        }
        ctx.textAlign = "left";

        // Preview thumbnail.
        if (nodeHasPreview(node)) {
            const maxPins = Math.max(
                pins.inputs.length,
                pins.outputs.length,
                1,
            );
            const previewY =
                y + NODE_HEADER_HEIGHT + maxPins * PIN_HEIGHT + 8 + PREVIEW_PAD;
            const previewX = x + (NODE_WIDTH - PREVIEW_SIZE) / 2;
            drawNodePreview(ctx, node, previewX, previewY, PREVIEW_SIZE);
        }
    }

    function drawNodePreview(
        ctx: CanvasRenderingContext2D,
        node: ShaderNode,
        px: number,
        py: number,
        size: number,
    ) {
        const t = $themeColors;
        // Border
        ctx.fillStyle = t.crust;
        ctx.fillRect(px - 1, py - 1, size + 2, size + 2);

        const nt = node.node_type;

        if (nt.type === "FloatConstant") {
            // Grayscale swatch
            const v = Math.max(0, Math.min(1, nt.value));
            const c = Math.round(v * 255);
            ctx.fillStyle = `rgb(${c},${c},${c})`;
            ctx.fillRect(px, py, size, size);
        } else if (nt.type === "Vec2Constant") {
            // 2D direction visualization
            const v = nt.value;
            ctx.fillStyle = `rgb(${Math.round(Math.abs(v[0]) * 255)},${Math.round(Math.abs(v[1]) * 255)},0)`;
            ctx.fillRect(px, py, size, size);
        } else if (nt.type === "Vec3Constant") {
            const [r, g, b] = nt.value;
            ctx.fillStyle = `rgb(${Math.round(r * 255)},${Math.round(g * 255)},${Math.round(b * 255)})`;
            ctx.fillRect(px, py, size, size);
        } else if (nt.type === "Vec4Constant") {
            // Checkerboard behind alpha
            drawCheckerboard(ctx, px, py, size);
            const [r, g, b, a] = nt.value;
            ctx.fillStyle = `rgba(${Math.round(r * 255)},${Math.round(g * 255)},${Math.round(b * 255)},${a})`;
            ctx.fillRect(px, py, size, size);
        } else if (nt.type === "UvInput") {
            // Red-green UV gradient
            drawUvGradient(ctx, px, py, size);
        } else if (nt.type === "ColorInput") {
            // Show actual color
            const c = nt.color || [1, 1, 1, 1];
            ctx.fillStyle = `rgba(${Math.round(c[0] * 255)},${Math.round(c[1] * 255)},${Math.round(c[2] * 255)},${c[3]})`;
            ctx.fillRect(px, py, size, size);
        } else if (nt.type === "TimeInput") {
            // Animated-looking bar
            ctx.fillStyle = t.mantle;
            ctx.fillRect(px, py, size, size);
            ctx.fillStyle = PIN_COLORS.Float;
            ctx.font = `${10}px system-ui`;
            ctx.textAlign = "center";
            ctx.textBaseline = "middle";
            ctx.fillText("t", px + size / 2, py + size / 2);
            ctx.textAlign = "left";
        } else if (nt.type === "PositionInput") {
            drawUvGradient(ctx, px, py, size);
        } else if (nt.type === "TextureSample") {
            drawCheckerboard(ctx, px, py, size);
        } else if (nt.type === "FragmentOutput") {
            // Special output node — show output icon
            ctx.fillStyle = t["accent-low"];
            ctx.fillRect(px, py, size, size);
            ctx.fillStyle = t["accent-high"];
            ctx.font = `${10}px system-ui`;
            ctx.textAlign = "center";
            ctx.textBaseline = "middle";
            ctx.fillText("OUT", px + size / 2, py + size / 2);
            ctx.textAlign = "left";
        } else {
            // Generic node — show a type-colored gradient based on output pin type
            const pins = nodeCache.get(node.id);
            const outType = pins?.outputs[0]?.pin_type || "Float";
            const color = PIN_COLORS[outType] || t.muted;
            // Parse color and draw gradient
            ctx.fillStyle = color + "40";
            ctx.fillRect(px, py, size, size);
            ctx.fillStyle = color;
            ctx.font = `${9}px system-ui`;
            ctx.textAlign = "center";
            ctx.textBaseline = "middle";
            ctx.fillText(outType, px + size / 2, py + size / 2);
            ctx.textAlign = "left";
        }
    }

    function drawCheckerboard(
        ctx: CanvasRenderingContext2D,
        px: number,
        py: number,
        size: number,
    ) {
        const sq = 8;
        for (let cy = 0; cy < size; cy += sq) {
            for (let cx = 0; cx < size; cx += sq) {
                const dark = (cx / sq + cy / sq) % 2 === 0;
                ctx.fillStyle = dark ? $themeColors.raised : $themeColors.line;
                ctx.fillRect(
                    px + cx,
                    py + cy,
                    Math.min(sq, size - cx),
                    Math.min(sq, size - cy),
                );
            }
        }
    }

    // Cached UV gradient image
    let uvGradientCache: ImageBitmap | null = null;

    function getUvGradient(size: number): ImageBitmap | null {
        if (uvGradientCache) return uvGradientCache;
        const offscreen = new OffscreenCanvas(size, size);
        const octx = offscreen.getContext("2d");
        if (!octx) return null;
        const imgData = octx.createImageData(size, size);
        for (let y = 0; y < size; y++) {
            for (let x = 0; x < size; x++) {
                const idx = (y * size + x) * 4;
                imgData.data[idx] = Math.round((x / size) * 255); // R = U
                imgData.data[idx + 1] = Math.round((y / size) * 255); // G = V
                imgData.data[idx + 2] = 0;
                imgData.data[idx + 3] = 255;
            }
        }
        octx.putImageData(imgData, 0, 0);
        createImageBitmap(offscreen).then((bmp) => {
            uvGradientCache = bmp;
            requestDraw();
        });
        return null;
    }

    function drawUvGradient(
        ctx: CanvasRenderingContext2D,
        px: number,
        py: number,
        size: number,
    ) {
        const bmp = getUvGradient(size);
        if (bmp) {
            ctx.drawImage(bmp, px, py, size, size);
        } else {
            // Fallback while generating
            ctx.fillStyle = $themeColors.mantle;
            ctx.fillRect(px, py, size, size);
        }
    }

    function drawEdge(ctx: CanvasRenderingContext2D, edge: Edge) {
        const from = getPinPos(edge.from_node, edge.from_pin, true);
        const to = getPinPos(edge.to_node, edge.to_pin, false);

        // Color based on source pin type.
        const srcPins = nodeCache.get(edge.from_node);
        let color = $themeColors.accent;
        if (srcPins && srcPins.outputs[edge.from_pin]) {
            color =
                PIN_COLORS[srcPins.outputs[edge.from_pin].pin_type] ||
                $themeColors.accent;
        }

        drawBezier(ctx, from[0], from[1], to[0], to[1], color);
    }

    function drawBezier(
        ctx: CanvasRenderingContext2D,
        x1: number,
        y1: number,
        x2: number,
        y2: number,
        color: string,
    ) {
        const dx = Math.abs(x2 - x1) * 0.5;
        ctx.strokeStyle = color;
        ctx.lineWidth = 2 / zoom;
        ctx.beginPath();
        ctx.moveTo(x1, y1);
        ctx.bezierCurveTo(x1 + dx, y1, x2 - dx, y2, x2, y2);
        ctx.stroke();
    }

    // ─── Coordinate Helpers ──────────────────────────────────────────

    function screenToGraph(sx: number, sy: number): [number, number] {
        return [(sx - panOffset[0]) / zoom, (sy - panOffset[1]) / zoom];
    }

    function nodeAtPos(gx: number, gy: number): ShaderNode | null {
        // Iterate in reverse (top-most first).
        for (let i = graph.nodes.length - 1; i >= 0; i--) {
            const n = graph.nodes[i];
            const h = getNodeHeight(n.id);
            if (
                gx >= n.position[0] &&
                gx <= n.position[0] + NODE_WIDTH &&
                gy >= n.position[1] &&
                gy <= n.position[1] + h
            ) {
                return n;
            }
        }
        return null;
    }

    function pinAtPos(
        gx: number,
        gy: number,
    ): { nodeId: number; pinIdx: number; isOutput: boolean } | null {
        const threshold = PIN_RADIUS * 2;
        for (const node of graph.nodes) {
            const pins = nodeCache.get(node.id);
            if (!pins) continue;
            // Check input pins.
            for (let i = 0; i < pins.inputs.length; i++) {
                const [px, py] = getPinPos(node.id, i, false);
                if (Math.hypot(gx - px, gy - py) < threshold) {
                    return { nodeId: node.id, pinIdx: i, isOutput: false };
                }
            }
            // Check output pins.
            for (let i = 0; i < pins.outputs.length; i++) {
                const [px, py] = getPinPos(node.id, i, true);
                if (Math.hypot(gx - px, gy - py) < threshold) {
                    return { nodeId: node.id, pinIdx: i, isOutput: true };
                }
            }
        }
        return null;
    }

    // ─── Canvas Helpers ──────────────────────────────────────────────

    function roundRect(
        ctx: CanvasRenderingContext2D,
        x: number,
        y: number,
        w: number,
        h: number,
        r: number,
    ) {
        ctx.beginPath();
        ctx.moveTo(x + r, y);
        ctx.lineTo(x + w - r, y);
        ctx.quadraticCurveTo(x + w, y, x + w, y + r);
        ctx.lineTo(x + w, y + h - r);
        ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
        ctx.lineTo(x + r, y + h);
        ctx.quadraticCurveTo(x, y + h, x, y + h - r);
        ctx.lineTo(x, y + r);
        ctx.quadraticCurveTo(x, y, x + r, y);
        ctx.closePath();
    }

    function roundRectTop(
        ctx: CanvasRenderingContext2D,
        x: number,
        y: number,
        w: number,
        h: number,
        r: number,
    ) {
        ctx.beginPath();
        ctx.moveTo(x + r, y);
        ctx.lineTo(x + w - r, y);
        ctx.quadraticCurveTo(x + w, y, x + w, y + r);
        ctx.lineTo(x + w, y + h);
        ctx.lineTo(x, y + h);
        ctx.lineTo(x, y + r);
        ctx.quadraticCurveTo(x, y, x + r, y);
        ctx.closePath();
    }

    function nodeTypeLabel(nt: NodeType): string {
        switch (nt.type) {
            case "FloatConstant":
                return `Float: ${nt.value}`;
            case "Vec2Constant":
                return `Vec2`;
            case "Vec3Constant":
                return `Vec3`;
            case "Vec4Constant":
                return `Vec4`;
            case "Combine":
                return `Combine (${nt.components})`;
            case "CustomGlsl":
                return "Custom GLSL";
            default:
                return nt.type.replace(/([A-Z])/g, " $1").trim();
        }
    }

    // ─── Event Handlers ──────────────────────────────────────────────

    function onPointerDown(e: PointerEvent) {
        if (!canvas) return;
        const rect = canvas.getBoundingClientRect();
        const sx = e.clientX - rect.left;
        const sy = e.clientY - rect.top;
        const [gx, gy] = screenToGraph(sx, sy);

        // Middle button → pan.
        if (e.button === 1) {
            isPanning = true;
            panStart = [e.clientX - panOffset[0], e.clientY - panOffset[1]];
            canvas.setPointerCapture(e.pointerId);
            return;
        }

        if (e.button !== 0) return;

        // Check pin hit first.
        const pin = pinAtPos(gx, gy);
        if (pin) {
            if (pin.isOutput) {
                // Start connection from output pin.
                connectingFrom = pin;
            } else {
                // If clicking an input pin, disconnect existing edge.
                graph.edges = graph.edges.filter(
                    (edge) =>
                        !(
                            edge.to_node === pin.nodeId &&
                            edge.to_pin === pin.pinIdx
                        ),
                );
                dirty = true;
            }
            canvas.setPointerCapture(e.pointerId);
            return;
        }

        // Check node hit.
        const node = nodeAtPos(gx, gy);
        if (node) {
            selectedNodeId = node.id;
            draggingNode = node.id;
            dragOffset = [gx - node.position[0], gy - node.position[1]];
            canvas.setPointerCapture(e.pointerId);
            return;
        }

        // Clicked empty space.
        selectedNodeId = null;

        // Alt+click or space+click for pan.
        if (e.altKey) {
            isPanning = true;
            panStart = [e.clientX - panOffset[0], e.clientY - panOffset[1]];
            canvas.setPointerCapture(e.pointerId);
        }
    }

    function onPointerMove(e: PointerEvent) {
        if (!canvas) return;
        const rect = canvas.getBoundingClientRect();
        mousePos = [e.clientX - rect.left, e.clientY - rect.top];

        if (isPanning) {
            panOffset = [e.clientX - panStart[0], e.clientY - panStart[1]];
            requestDraw();
            return;
        }

        if (draggingNode !== null) {
            const [gx, gy] = screenToGraph(mousePos[0], mousePos[1]);
            const node = graph.nodes.find((n) => n.id === draggingNode);
            if (node) {
                node.position = [gx - dragOffset[0], gy - dragOffset[1]];
                dirty = true;
            }
            requestDraw();
            return;
        }

        if (connectingFrom) {
            requestDraw();
        }
    }

    function onPointerUp(e: PointerEvent) {
        if (isPanning) {
            isPanning = false;
            return;
        }

        if (connectingFrom && canvas) {
            const rect = canvas.getBoundingClientRect();
            const [gx, gy] = screenToGraph(
                e.clientX - rect.left,
                e.clientY - rect.top,
            );
            const pin = pinAtPos(gx, gy);

            if (pin && !pin.isOutput && pin.nodeId !== connectingFrom.nodeId) {
                // Valid connection: output → input.
                // Remove any existing connection to this input pin.
                graph.edges = graph.edges.filter(
                    (edge) =>
                        !(
                            edge.to_node === pin.nodeId &&
                            edge.to_pin === pin.pinIdx
                        ),
                );
                graph.edges = [
                    ...graph.edges,
                    {
                        from_node: connectingFrom.nodeId,
                        from_pin: connectingFrom.pinIdx,
                        to_node: pin.nodeId,
                        to_pin: pin.pinIdx,
                    },
                ];
                dirty = true;
            }
            connectingFrom = null;
            requestDraw();
        }

        draggingNode = null;
    }

    function onWheel(e: WheelEvent) {
        e.preventDefault();
        const factor = e.deltaY > 0 ? 0.9 : 1.1;
        const newZoom = Math.max(0.2, Math.min(3, zoom * factor));

        // Zoom toward mouse position.
        if (canvas) {
            const rect = canvas.getBoundingClientRect();
            const mx = e.clientX - rect.left;
            const my = e.clientY - rect.top;
            panOffset = [
                mx - ((mx - panOffset[0]) / zoom) * newZoom,
                my - ((my - panOffset[1]) / zoom) * newZoom,
            ];
        }
        zoom = newZoom;
        requestDraw();
    }

    function onKeyDown(e: KeyboardEvent) {
        if (e.key === "Delete" && selectedNodeId !== null) {
            // Don't delete the output node.
            const node = graph.nodes.find((n) => n.id === selectedNodeId);
            if (node && node.node_type.type !== "FragmentOutput") {
                graph.nodes = graph.nodes.filter(
                    (n) => n.id !== selectedNodeId,
                );
                graph.edges = graph.edges.filter(
                    (edge) =>
                        edge.from_node !== selectedNodeId &&
                        edge.to_node !== selectedNodeId,
                );
                selectedNodeId = null;
                dirty = true;
                requestDraw();
            }
        }

        if (e.key === " " && !showPalette) {
            e.preventDefault();
            showPalette = true;
            paletteFilter = "";
        }

        if (e.key === "Escape") {
            if (showPalette) {
                showPalette = false;
            } else if (connectingFrom) {
                connectingFrom = null;
                requestDraw();
            }
        }

        // Ctrl+S → Save.
        if (e.ctrlKey && e.key === "s") {
            e.preventDefault();
            handleSave();
        }

        // F → Zoom to fit (unmodified only — Ctrl+F is not ours).
        if (e.key === "f" && !e.ctrlKey && !e.metaKey && !e.altKey && !showPalette) {
            zoomToFit();
        }
    }

    // ─── Node Operations ─────────────────────────────────────────────

    function addNode(nodeType: NodeType) {
        const center = screenToGraph(
            canvas ? canvas.width / 2 : 300,
            canvas ? canvas.height / 2 : 200,
        );
        const id = graph.next_id;
        graph.next_id++;
        graph.nodes = [
            ...graph.nodes,
            { id, node_type: nodeType, position: center, label: undefined },
        ];
        selectedNodeId = id;
        showPalette = false;
        dirty = true;
        // Cache update will trigger via $effect.
        requestDraw();
    }

    function zoomToFit() {
        if (!canvas || graph.nodes.length === 0) return;
        let minX = Infinity,
            minY = Infinity,
            maxX = -Infinity,
            maxY = -Infinity;
        for (const n of graph.nodes) {
            minX = Math.min(minX, n.position[0]);
            minY = Math.min(minY, n.position[1]);
            maxX = Math.max(maxX, n.position[0] + NODE_WIDTH);
            maxY = Math.max(maxY, n.position[1] + getNodeHeight(n.id));
        }
        const pad = 60;
        const w = maxX - minX + pad * 2;
        const h = maxY - minY + pad * 2;
        zoom = Math.min(canvas.width / w, canvas.height / h, 2);
        panOffset = [
            canvas.width / 2 - ((minX + maxX) / 2) * zoom,
            canvas.height / 2 - ((minY + maxY) / 2) * zoom,
        ];
        requestDraw();
    }

    // ─── Compile & Validate ──────────────────────────────────────────

    async function handleCompile() {
        try {
            const json = JSON.stringify(graph);
            const result = await shaderGenerate(json);
            glslPreview = result.wgsl;
            showGlsl = true;
            validationErrors = [];
            logInfo(`Shader "${graph.meta.name}" compiled successfully`);
        } catch (err) {
            logError(`Shader compile failed: ${err}`);
        }
    }

    async function handleValidate() {
        try {
            const json = JSON.stringify(graph);
            const result = await shaderValidate(json);
            validationErrors = result.errors;
            if (result.valid) {
                logInfo(`Shader "${graph.meta.name}" is valid`);
            } else {
                logWarn(`Shader has ${result.errors.length} error(s)`);
            }
        } catch (err) {
            logError(`Validation failed: ${err}`);
        }
    }

    async function handleSave() {
        if (!projectPath || !relativePath) {
            logWarn("No project path set — cannot save");
            return;
        }
        try {
            const json = JSON.stringify(graph);
            await shaderSave(projectPath, relativePath, json);
            dirty = false;
            logInfo(`Saved shader to ${relativePath}`);
        } catch (err) {
            logError(`Save failed: ${err}`);
        }
    }

    // ─── Draw Loop ───────────────────────────────────────────────────

    let drawPending = false;
    function requestDraw() {
        if (drawPending) return;
        drawPending = true;
        requestAnimationFrame(() => {
            drawPending = false;
            draw();
        });
    }

    function resizeCanvas() {
        if (!canvas) return;
        const rect = canvas.parentElement?.getBoundingClientRect();
        if (rect) {
            canvas.width = rect.width;
            canvas.height = rect.height;
        }
        requestDraw();
    }

    $effect(() => {
        if (canvas) {
            resizeCanvas();
            const observer = new ResizeObserver(() => resizeCanvas());
            observer.observe(canvas.parentElement!);
            return () => observer.disconnect();
        }
    });

    // Redraw when the palette changes.
    $effect(() => {
        void $themeColors;
        requestDraw();
    });

    // Redraw when graph changes.
    $effect(() => {
        if (graph && nodeCache.size > 0) {
            requestDraw();
        }
    });

    // ─── Filtered Palette ────────────────────────────────────────────

    let filteredPalette = $derived(
        paletteFilter
            ? palette
                  .map((cat) => ({
                      ...cat,
                      nodes: cat.nodes.filter((n) =>
                          n.name
                              .toLowerCase()
                              .includes(paletteFilter.toLowerCase()),
                      ),
                  }))
                  .filter((cat) => cat.nodes.length > 0)
            : palette,
    );
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="shader-editor" onkeydown={onKeyDown} tabindex="-1">
    <!-- Toolbar -->
    <div class="shader-toolbar">
        <span class="shader-name">{graph.meta.name}{dirty ? " •" : ""}</span>
        <div class="toolbar-buttons">
            <button
                onclick={() => (showPalette = !showPalette)}
                title="Add Node (Space)">+ Add Node</button
            >
            <button onclick={handleCompile} title="Generate WGSL (Ctrl+G)"
                >Compile</button
            >
            <button onclick={handleValidate} title="Validate Graph"
                >Validate</button
            >
            <button onclick={handleSave} title="Save (Ctrl+S)">Save</button>
            <button onclick={zoomToFit} title="Zoom to Fit (F)">Fit</button>
            {#if onclose}
                <button onclick={onclose} class="close-btn">✕</button>
            {/if}
        </div>
    </div>

    <div class="shader-body">
        <!-- Node Palette Sidebar -->
        {#if showPalette}
            <div class="palette-panel">
                <input
                    class="palette-search"
                    type="text"
                    placeholder="Search nodes..."
                    bind:value={paletteFilter}
                />
                <div class="palette-list">
                    {#each filteredPalette as cat}
                        <div class="palette-category">{cat.category}</div>
                        {#each cat.nodes as node}
                            <button
                                class="palette-item"
                                onclick={() => addNode(node.node_type)}
                            >
                                {node.name}
                            </button>
                        {/each}
                    {/each}
                </div>
            </div>
        {/if}

        <!-- Canvas -->
        <div class="canvas-container">
            <canvas
                bind:this={canvas}
                onpointerdown={onPointerDown}
                onpointermove={onPointerMove}
                onpointerup={onPointerUp}
                onwheel={onWheel}
            ></canvas>

            <!-- Validation errors overlay -->
            {#if validationErrors.length > 0}
                <div class="validation-overlay">
                    {#each validationErrors as err}
                        <div class="validation-error">{err}</div>
                    {/each}
                </div>
            {/if}
        </div>

        <!-- WGSL Preview Panel -->
        {#if showGlsl}
            <div class="glsl-panel">
                <div class="glsl-header">
                    <span>Generated WGSL</span>
                    <button onclick={() => (showGlsl = false)}>✕</button>
                </div>
                <pre class="glsl-code">{glslPreview}</pre>
            </div>
        {/if}
    </div>
</div>

<style>
    .shader-editor {
        display: flex;
        flex-direction: column;
        height: 100%;
        background: var(--ed-base);
        color: var(--ed-text);
        outline: none;
    }

    .shader-toolbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        height: 36px;
        padding: 0 8px;
        background: var(--ed-mantle);
        border-bottom: 1px solid var(--ed-line);
    }

    .shader-name {
        font-size: 13px;
        font-weight: 600;
        color: var(--ed-text);
    }

    .toolbar-buttons {
        display: flex;
        gap: 4px;
    }

    .toolbar-buttons button {
        padding: 4px 10px;
        font-size: 12px;
        background: var(--ed-raised);
        color: var(--ed-text);
        border: 1px solid var(--ed-line-strong);
        border-radius: 4px;
        cursor: pointer;
    }

    .toolbar-buttons button:hover {
        background: var(--ed-line);
    }

    .close-btn {
        color: var(--ed-err) !important;
    }

    .shader-body {
        display: flex;
        flex: 1;
        overflow: hidden;
        position: relative;
    }

    /* Palette */
    .palette-panel {
        width: 200px;
        background: var(--ed-mantle);
        border-right: 1px solid var(--ed-line);
        display: flex;
        flex-direction: column;
    }

    .palette-search {
        padding: 6px 8px;
        margin: 6px;
        font-size: 12px;
        background: var(--ed-base);
        color: var(--ed-text);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        outline: none;
    }

    .palette-search:focus {
        border-color: var(--ed-accent);
    }

    .palette-list {
        overflow-y: auto;
        flex: 1;
        padding: 0 6px 6px;
    }

    .palette-category {
        font-size: 10px;
        text-transform: uppercase;
        color: var(--ed-muted);
        padding: 8px 4px 4px;
        letter-spacing: 0.5px;
    }

    .palette-item {
        display: block;
        width: 100%;
        text-align: left;
        padding: 4px 8px;
        font-size: 12px;
        background: none;
        color: var(--ed-text);
        border: none;
        border-radius: 4px;
        cursor: pointer;
    }

    .palette-item:hover {
        background: var(--ed-raised);
    }

    /* Canvas */
    .canvas-container {
        flex: 1;
        position: relative;
        overflow: hidden;
    }

    .canvas-container canvas {
        display: block;
        width: 100%;
        height: 100%;
        cursor: crosshair;
    }

    /* Validation Overlay */
    .validation-overlay {
        position: absolute;
        top: 8px;
        right: 8px;
        max-width: 300px;
    }

    .validation-error {
        background: color-mix(in srgb, var(--ed-line) 80%, transparent);
        color: var(--ed-err);
        padding: 6px 10px;
        border-radius: 4px;
        font-size: 12px;
        margin-bottom: 4px;
        border-left: 3px solid var(--ed-err);
    }

    /* GLSL Preview */
    .glsl-panel {
        width: 320px;
        background: var(--ed-mantle);
        border-left: 1px solid var(--ed-line);
        display: flex;
        flex-direction: column;
    }

    .glsl-header {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: 6px 8px;
        font-size: 11px;
        text-transform: uppercase;
        color: var(--ed-muted);
        border-bottom: 1px solid var(--ed-line);
    }

    .glsl-header button {
        background: none;
        border: none;
        color: var(--ed-muted);
        cursor: pointer;
        font-size: 12.5px;
    }

    .glsl-code {
        flex: 1;
        overflow: auto;
        padding: 8px;
        font-size: 12px;
        font-family: "Cascadia Code", "Fira Code", monospace;
        color: var(--ed-ok);
        white-space: pre;
        margin: 0;
        line-height: 1.5;
    }
</style>
