<script lang="ts">
    import { themeColors } from "./stores/themeColors";
    import { prefabLibrary, scenePrefabs } from "./stores/prefabLibraryStore";
    import { editThroughInstance } from "./prefabInstance";
    import { requestEditorClose } from "./stores/editorClose";
    import {
        vectorPathData,
        vectorPathDirty,
        vpSelectedCommand,
        vpTool,
        executeVP,
        undoVP,
        redoVP,
        canUndoVP,
        canRedoVP,
        closeVectorPathEditor,
        vectorPathEntityId,
    } from "./stores/vectorPathEditorStore";
    import type { PathCommand } from "./api/scene";
    import { execute } from "./stores/commandStore";

    let canvas: HTMLCanvasElement;
    let ctx: CanvasRenderingContext2D | null = null;
    let width = $state(800);
    let height = $state(600);

    // Canvas panning + zoom
    let panX = $state(0);
    let panY = $state(0);
    let zoom = $state(1);
    let isPanning = $state(false);
    let lastMouse = $state({ x: 0, y: 0 });

    // Drag state
    let dragging = $state<{
        cmdIdx: number;
        handle: "pos" | "c1" | "c2" | "ctrl";
    } | null>(null);

    const POINT_RADIUS = 6;
    const HANDLE_RADIUS = 4;
    const GRID_SIZE = 20;

    /** Convert world coords to screen coords. */
    function toScreen(wx: number, wy: number): [number, number] {
        return [wx * zoom + panX + width / 2, -wy * zoom + panY + height / 2];
    }

    /** Convert screen coords to world coords. */
    function toWorld(sx: number, sy: number): [number, number] {
        return [
            (sx - panX - width / 2) / zoom,
            -(sy - panY - height / 2) / zoom,
        ];
    }

    function draw() {
        if (!ctx || !$vectorPathData) return;
        const data = $vectorPathData;
        const commands = data.commands;

        ctx.clearRect(0, 0, width, height);

        // Draw grid
        ctx.strokeStyle = $themeColors.line;
        ctx.lineWidth = 1;
        const gridStep = GRID_SIZE * zoom;
        if (gridStep > 4) {
            const startX =
                (((panX + width / 2) % gridStep) + gridStep) % gridStep;
            const startY =
                (((panY + height / 2) % gridStep) + gridStep) % gridStep;
            ctx.beginPath();
            for (let x = startX; x < width; x += gridStep) {
                ctx.moveTo(Math.round(x) + 0.5, 0);
                ctx.lineTo(Math.round(x) + 0.5, height);
            }
            for (let y = startY; y < height; y += gridStep) {
                ctx.moveTo(0, Math.round(y) + 0.5);
                ctx.lineTo(width, Math.round(y) + 0.5);
            }
            ctx.stroke();
        }

        // Draw origin crosshair
        const [ox, oy] = toScreen(0, 0);
        ctx.strokeStyle = $themeColors["line-strong"];
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(ox, 0);
        ctx.lineTo(ox, height);
        ctx.moveTo(0, oy);
        ctx.lineTo(width, oy);
        ctx.stroke();

        if (commands.length === 0) {
            ctx.fillStyle = $themeColors.faint;
            ctx.font = "14px sans-serif";
            ctx.textAlign = "center";
            ctx.fillText(
                "Select the Pen tool and click to add points",
                width / 2,
                height / 2,
            );
            return;
        }

        // Build and draw the path
        ctx.beginPath();
        let firstPoint = true;
        for (const cmd of commands) {
            if (cmd.type === "MoveTo") {
                const [sx, sy] = toScreen(cmd.x, cmd.y);
                ctx.moveTo(sx, sy);
                firstPoint = false;
            } else if (cmd.type === "LineTo") {
                const [sx, sy] = toScreen(cmd.x, cmd.y);
                if (firstPoint) {
                    ctx.moveTo(sx, sy);
                    firstPoint = false;
                } else {
                    ctx.lineTo(sx, sy);
                }
            } else if (cmd.type === "QuadTo") {
                const [cx, cy] = toScreen(cmd.cx, cmd.cy);
                const [sx, sy] = toScreen(cmd.x, cmd.y);
                ctx.quadraticCurveTo(cx, cy, sx, sy);
            } else if (cmd.type === "CubicTo") {
                const [c1x, c1y] = toScreen(cmd.c1x, cmd.c1y);
                const [c2x, c2y] = toScreen(cmd.c2x, cmd.c2y);
                const [sx, sy] = toScreen(cmd.x, cmd.y);
                ctx.bezierCurveTo(c1x, c1y, c2x, c2y, sx, sy);
            } else if (cmd.type === "Close") {
                ctx.closePath();
            }
        }

        // Fill
        if (data.fill_color) {
            const [r, g, b, a] = data.fill_color;
            ctx.fillStyle = `rgba(${Math.round(r * 255)},${Math.round(g * 255)},${Math.round(b * 255)},${a})`;
            ctx.fill();
        }

        // Stroke
        if (data.stroke_color) {
            const [r, g, b, a] = data.stroke_color;
            ctx.strokeStyle = `rgba(${Math.round(r * 255)},${Math.round(g * 255)},${Math.round(b * 255)},${a})`;
            ctx.lineWidth = data.stroke_width * zoom;
            ctx.stroke();
        } else {
            // Always show an outline in the editor for visibility
            ctx.strokeStyle = $themeColors.text;
            ctx.lineWidth = 1;
            ctx.stroke();
        }

        // Draw control points and handles
        for (let i = 0; i < commands.length; i++) {
            const cmd = commands[i];
            const selected = $vpSelectedCommand === i;

            if (cmd.type === "MoveTo" || cmd.type === "LineTo") {
                const [sx, sy] = toScreen(cmd.x, cmd.y);
                drawPoint(sx, sy, selected);
            } else if (cmd.type === "QuadTo") {
                const [sx, sy] = toScreen(cmd.x, cmd.y);
                const [cx, cy] = toScreen(cmd.cx, cmd.cy);
                // Handle line
                ctx.strokeStyle = $themeColors.a2;
                ctx.lineWidth = 1;
                ctx.setLineDash([3, 3]);
                ctx.beginPath();
                ctx.moveTo(cx, cy);
                ctx.lineTo(sx, sy);
                ctx.stroke();
                ctx.setLineDash([]);
                drawHandle(cx, cy, selected);
                drawPoint(sx, sy, selected);
            } else if (cmd.type === "CubicTo") {
                const [sx, sy] = toScreen(cmd.x, cmd.y);
                const [c1x, c1y] = toScreen(cmd.c1x, cmd.c1y);
                const [c2x, c2y] = toScreen(cmd.c2x, cmd.c2y);
                // Handle lines
                ctx.strokeStyle = $themeColors.a2;
                ctx.lineWidth = 1;
                ctx.setLineDash([3, 3]);
                ctx.beginPath();
                // Connect c1 to previous point if possible
                const prev = findPreviousEndpoint(commands, i);
                if (prev) {
                    const [px, py] = toScreen(prev[0], prev[1]);
                    ctx.moveTo(px, py);
                    ctx.lineTo(c1x, c1y);
                }
                ctx.moveTo(c2x, c2y);
                ctx.lineTo(sx, sy);
                ctx.stroke();
                ctx.setLineDash([]);
                drawHandle(c1x, c1y, selected);
                drawHandle(c2x, c2y, selected);
                drawPoint(sx, sy, selected);
            }
        }
    }

    function findPreviousEndpoint(
        commands: PathCommand[],
        idx: number,
    ): [number, number] | null {
        for (let i = idx - 1; i >= 0; i--) {
            const c = commands[i];
            if (c.type === "MoveTo" || c.type === "LineTo") return [c.x, c.y];
            if (c.type === "QuadTo" || c.type === "CubicTo") return [c.x, c.y];
        }
        return null;
    }

    function drawPoint(x: number, y: number, selected: boolean) {
        if (!ctx) return;
        ctx.fillStyle = selected ? $themeColors.err : $themeColors.text;
        ctx.strokeStyle = $themeColors.base;
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.arc(x, y, POINT_RADIUS, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
    }

    function drawHandle(x: number, y: number, selected: boolean) {
        if (!ctx) return;
        ctx.fillStyle = selected ? $themeColors.a3 : $themeColors.a2;
        ctx.strokeStyle = $themeColors.base;
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.rect(
            x - HANDLE_RADIUS,
            y - HANDLE_RADIUS,
            HANDLE_RADIUS * 2,
            HANDLE_RADIUS * 2,
        );
        ctx.fill();
        ctx.stroke();
    }

    function hitTest(
        sx: number,
        sy: number,
    ): { cmdIdx: number; handle: "pos" | "c1" | "c2" | "ctrl" } | null {
        const commands = $vectorPathData?.commands;
        if (!commands) return null;

        for (let i = commands.length - 1; i >= 0; i--) {
            const cmd = commands[i];
            if (cmd.type === "MoveTo" || cmd.type === "LineTo") {
                const [px, py] = toScreen(cmd.x, cmd.y);
                if (dist(sx, sy, px, py) <= POINT_RADIUS + 2)
                    return { cmdIdx: i, handle: "pos" };
            } else if (cmd.type === "QuadTo") {
                const [px, py] = toScreen(cmd.x, cmd.y);
                const [cx, cy] = toScreen(cmd.cx, cmd.cy);
                if (dist(sx, sy, cx, cy) <= HANDLE_RADIUS + 2)
                    return { cmdIdx: i, handle: "ctrl" };
                if (dist(sx, sy, px, py) <= POINT_RADIUS + 2)
                    return { cmdIdx: i, handle: "pos" };
            } else if (cmd.type === "CubicTo") {
                const [px, py] = toScreen(cmd.x, cmd.y);
                const [c1x, c1y] = toScreen(cmd.c1x, cmd.c1y);
                const [c2x, c2y] = toScreen(cmd.c2x, cmd.c2y);
                if (dist(sx, sy, c1x, c1y) <= HANDLE_RADIUS + 2)
                    return { cmdIdx: i, handle: "c1" };
                if (dist(sx, sy, c2x, c2y) <= HANDLE_RADIUS + 2)
                    return { cmdIdx: i, handle: "c2" };
                if (dist(sx, sy, px, py) <= POINT_RADIUS + 2)
                    return { cmdIdx: i, handle: "pos" };
            }
        }
        return null;
    }

    function dist(x1: number, y1: number, x2: number, y2: number): number {
        return Math.hypot(x2 - x1, y2 - y1);
    }

    function handleMouseDown(e: MouseEvent) {
        const rect = canvas.getBoundingClientRect();
        const mx = e.clientX - rect.left;
        const my = e.clientY - rect.top;

        // Middle button or space+left = pan
        if (e.button === 1) {
            isPanning = true;
            lastMouse = { x: mx, y: my };
            e.preventDefault();
            return;
        }

        if (e.button !== 0) return;

        if ($vpTool === "select") {
            const hit = hitTest(mx, my);
            if (hit) {
                vpSelectedCommand.set(hit.cmdIdx);
                dragging = hit;
            } else {
                vpSelectedCommand.set(null);
            }
        } else if ($vpTool === "pen") {
            const [wx, wy] = toWorld(mx, my);
            executeVP("Add point", (data) => {
                if (data.commands.length === 0) {
                    data.commands.push({ type: "MoveTo", x: wx, y: wy });
                } else {
                    data.commands.push({ type: "LineTo", x: wx, y: wy });
                }
            });
            vpSelectedCommand.set(($vectorPathData?.commands.length ?? 1) - 1);
        }

        draw();
    }

    function handleMouseMove(e: MouseEvent) {
        const rect = canvas.getBoundingClientRect();
        const mx = e.clientX - rect.left;
        const my = e.clientY - rect.top;

        if (isPanning) {
            panX += mx - lastMouse.x;
            panY += my - lastMouse.y;
            lastMouse = { x: mx, y: my };
            draw();
            return;
        }

        if (dragging && $vectorPathData) {
            const [wx, wy] = toWorld(mx, my);
            executeVP("Move point", (data) => {
                const cmd = data.commands[dragging!.cmdIdx];
                if (!cmd) return;

                if (dragging!.handle === "pos") {
                    if (
                        cmd.type === "MoveTo" ||
                        cmd.type === "LineTo" ||
                        cmd.type === "QuadTo" ||
                        cmd.type === "CubicTo"
                    ) {
                        cmd.x = wx;
                        cmd.y = wy;
                    }
                } else if (
                    dragging!.handle === "ctrl" &&
                    cmd.type === "QuadTo"
                ) {
                    cmd.cx = wx;
                    cmd.cy = wy;
                } else if (
                    dragging!.handle === "c1" &&
                    cmd.type === "CubicTo"
                ) {
                    cmd.c1x = wx;
                    cmd.c1y = wy;
                } else if (
                    dragging!.handle === "c2" &&
                    cmd.type === "CubicTo"
                ) {
                    cmd.c2x = wx;
                    cmd.c2y = wy;
                }
            });
            draw();
        }
    }

    function handleMouseUp() {
        isPanning = false;
        dragging = null;
    }

    function handleWheel(e: WheelEvent) {
        e.preventDefault();
        const rect = canvas.getBoundingClientRect();
        const mx = e.clientX - rect.left;
        const my = e.clientY - rect.top;

        const oldZoom = zoom;
        const factor = e.deltaY > 0 ? 0.9 : 1.1;
        zoom = Math.max(0.1, Math.min(20, zoom * factor));

        // Zoom towards cursor
        panX = mx - (mx - panX) * (zoom / oldZoom);
        panY = my - (my - panY) * (zoom / oldZoom);

        draw();
    }

    function handleKeyDown(e: KeyboardEvent) {
        if (e.key === "Delete" || e.key === "Backspace") {
            if ($vpSelectedCommand !== null) {
                const idx = $vpSelectedCommand;
                executeVP("Delete point", (data) => {
                    data.commands.splice(idx, 1);
                });
                vpSelectedCommand.set(null);
                draw();
            }
        } else if (e.key === "z" && (e.ctrlKey || e.metaKey) && !e.shiftKey) {
            e.preventDefault();
            undoVP();
            draw();
        } else if (
            (e.key === "z" && (e.ctrlKey || e.metaKey) && e.shiftKey) ||
            (e.key === "y" && (e.ctrlKey || e.metaKey))
        ) {
            e.preventDefault();
            redoVP();
            draw();
        } else if (e.key === "v" && !e.ctrlKey && !e.metaKey) {
            vpTool.set("select");
        } else if (e.key === "p" && !e.ctrlKey && !e.metaKey) {
            vpTool.set("pen");
        }
    }

    /** Convert selected LineTo to QuadTo with midpoint control. */
    function convertToQuad() {
        if ($vpSelectedCommand === null || !$vectorPathData) return;
        const cmd = $vectorPathData.commands[$vpSelectedCommand];
        if (cmd.type !== "LineTo") return;
        const prev = findPreviousEndpoint(
            $vectorPathData.commands,
            $vpSelectedCommand,
        );
        const px = prev ? prev[0] : 0;
        const py = prev ? prev[1] : 0;
        executeVP("Convert to quad", (data) => {
            data.commands[$vpSelectedCommand!] = {
                type: "QuadTo",
                cx: (px + cmd.x) / 2,
                cy: (py + cmd.y) / 2 + 50,
                x: cmd.x,
                y: cmd.y,
            };
        });
        draw();
    }

    /** Convert selected LineTo/QuadTo to CubicTo. */
    function convertToCubic() {
        if ($vpSelectedCommand === null || !$vectorPathData) return;
        const cmd = $vectorPathData.commands[$vpSelectedCommand];
        const prev = findPreviousEndpoint(
            $vectorPathData.commands,
            $vpSelectedCommand,
        );
        const px = prev ? prev[0] : 0;
        const py = prev ? prev[1] : 0;
        if (cmd.type === "LineTo") {
            executeVP("Convert to cubic", (data) => {
                const t1 = 1 / 3;
                const t2 = 2 / 3;
                data.commands[$vpSelectedCommand!] = {
                    type: "CubicTo",
                    c1x: px + (cmd.x - px) * t1,
                    c1y: py + (cmd.y - py) * t1 + 30,
                    c2x: px + (cmd.x - px) * t2,
                    c2y: py + (cmd.y - py) * t2 + 30,
                    x: cmd.x,
                    y: cmd.y,
                };
            });
            draw();
        } else if (cmd.type === "QuadTo") {
            executeVP("Convert to cubic", (data) => {
                data.commands[$vpSelectedCommand!] = {
                    type: "CubicTo",
                    c1x: px + (2 / 3) * (cmd.cx - px),
                    c1y: py + (2 / 3) * (cmd.cy - py),
                    c2x: cmd.x + (2 / 3) * (cmd.cx - cmd.x),
                    c2y: cmd.y + (2 / 3) * (cmd.cy - cmd.y),
                    x: cmd.x,
                    y: cmd.y,
                };
            });
            draw();
        }
    }

    /** Apply changes back to the scene. */
    function applyToScene() {
        const data = $vectorPathData;
        const entityId = $vectorPathEntityId;
        if (!data || entityId === null) return;
        // Through the instance: a path the entity's prefab supplies becomes
        // an override rather than a write to a component it does not store.
        const prefabs = $scenePrefabs;
        execute("Update vector path", (s) => {
            const ent = s.entities[entityId];
            if (!ent) return s;
            editThroughInstance(ent, prefabs, (view) => {
                if (view.components.vector_path) {
                    view.components.vector_path.commands = structuredClone(
                        data.commands,
                    );
                }
            });
            return s;
        });
        vectorPathDirty.set(false);
    }

    function setupCanvas(node: HTMLCanvasElement) {
        canvas = node;
        ctx = node.getContext("2d");
        const observer = new ResizeObserver(() => {
            width = node.clientWidth;
            height = node.clientHeight;
            node.width = width;
            node.height = height;
            draw();
        });
        observer.observe(node);
        // Initial size
        width = node.clientWidth;
        height = node.clientHeight;
        node.width = width;
        node.height = height;
        // Center pan on existing path
        panX = 0;
        panY = 0;
        zoom = 1;
        draw();
        return {
            destroy() {
                observer.disconnect();
            },
        };
    }

    // Redraw when data changes
    $effect(() => {
        if ($vectorPathData) draw();
    });
</script>

{#if $vectorPathData}
    <div class="vp-editor" role="application" aria-label="Vector Path Editor">
        <div class="vp-toolbar">
            <button
                class="tool-btn"
                class:active={$vpTool === "select"}
                onclick={() => vpTool.set("select")}
                title="Select tool (V)"
            >
                ◇ Select
            </button>
            <button
                class="tool-btn"
                class:active={$vpTool === "pen"}
                onclick={() => vpTool.set("pen")}
                title="Pen tool (P)"
            >
                ✎ Pen
            </button>
            <span class="separator"></span>
            <button
                class="tool-btn"
                disabled={!$canUndoVP}
                onclick={() => {
                    undoVP();
                    draw();
                }}
                title="Undo (Ctrl+Z)"
            >
                ↶
            </button>
            <button
                class="tool-btn"
                disabled={!$canRedoVP}
                onclick={() => {
                    redoVP();
                    draw();
                }}
                title="Redo (Ctrl+Y)"
            >
                ↷
            </button>
            <span class="separator"></span>
            {#if $vpSelectedCommand !== null}
                {@const cmd = $vectorPathData.commands[$vpSelectedCommand]}
                {#if cmd && cmd.type === "LineTo"}
                    <button class="tool-btn" onclick={convertToQuad}
                        >⟿ Quad</button
                    >
                    <button class="tool-btn" onclick={convertToCubic}
                        >∿ Cubic</button
                    >
                {:else if cmd && cmd.type === "QuadTo"}
                    <button class="tool-btn" onclick={convertToCubic}
                        >∿ Cubic</button
                    >
                {/if}
                <button
                    class="tool-btn danger"
                    onclick={() => {
                        const idx = $vpSelectedCommand!;
                        executeVP("Delete point", (data) => {
                            data.commands.splice(idx, 1);
                        });
                        vpSelectedCommand.set(null);
                        draw();
                    }}
                    title="Delete point"
                >
                    🗑 Delete
                </button>
            {/if}
            <span style="flex: 1;"></span>
            <button
                class="tool-btn apply"
                disabled={!$vectorPathDirty}
                onclick={applyToScene}
            >
                ✓ Apply
            </button>
            <button class="tool-btn" onclick={() => requestEditorClose("vectorpath")}>
                ✕ Close
            </button>
        </div>
        <!-- svelte-ignore a11y_positive_tabindex -->
        <canvas
            use:setupCanvas
            class="vp-canvas"
            tabindex="1"
            onmousedown={handleMouseDown}
            onmousemove={handleMouseMove}
            onmouseup={handleMouseUp}
            onmouseleave={handleMouseUp}
            onwheel={handleWheel}
            onkeydown={handleKeyDown}
        ></canvas>
        <div class="vp-status">
            {$vectorPathData.commands.length} commands
            {#if $vpSelectedCommand !== null}
                · Selected: {$vectorPathData.commands[$vpSelectedCommand]
                    ?.type ?? "—"} #{$vpSelectedCommand}
            {/if}
            · Zoom: {(zoom * 100).toFixed(0)}%
        </div>
    </div>
{/if}

<style>
    .vp-editor {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        background: var(--ctp-base, var(--ed-base));
    }

    .vp-toolbar {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 4px 8px;
        background: var(--ctp-mantle, var(--ed-mantle));
        border-bottom: 1px solid var(--ctp-surface0, var(--ed-line));
        flex-shrink: 0;
    }

    .tool-btn {
        padding: 3px 8px;
        border: 1px solid var(--ctp-surface1, var(--ed-line-strong));
        border-radius: 4px;
        background: var(--ctp-surface0, var(--ed-raised));
        color: var(--ctp-text, var(--ed-text));
        cursor: pointer;
        font-size: 12px;
        white-space: nowrap;
    }
    .tool-btn:hover {
        background: var(--ctp-surface1, var(--ed-line));
    }
    .tool-btn.active {
        background: var(--ctp-blue, var(--ed-accent));
        color: var(--ctp-base, var(--ed-on-accent));
        border-color: var(--ctp-blue, var(--ed-accent));
    }
    .tool-btn:disabled {
        opacity: 0.4;
        cursor: default;
    }
    .tool-btn.danger {
        color: var(--ctp-red, var(--ed-err));
    }
    .tool-btn.apply {
        color: var(--ctp-green, var(--ed-ok));
        border-color: var(--ctp-green, var(--ed-ok));
    }

    .separator {
        width: 1px;
        height: 20px;
        background: var(--ctp-surface1, var(--ed-line));
        margin: 0 4px;
    }

    .vp-canvas {
        flex: 1;
        cursor: crosshair;
        outline: none;
    }

    .vp-status {
        padding: 2px 8px;
        font-size: 11px;
        color: var(--ctp-subtext0, var(--ed-muted));
        background: var(--ctp-mantle, var(--ed-mantle));
        border-top: 1px solid var(--ctp-surface0, var(--ed-line));
        flex-shrink: 0;
    }
</style>
