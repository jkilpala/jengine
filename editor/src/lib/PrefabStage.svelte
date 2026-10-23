<script lang="ts">
  /**
   * Visual stage for the prefab editor — renders the prefab the way the
   * engine would draw it (sprite quad with anchor/uv/tint, animation
   * clips, text, collider + trigger wireframes) and lets you edit the
   * transform and shape sizes by dragging.
   *
   * This is a 2D-canvas re-implementation of the sprite quad math in
   * `crates/jengine-core/src/ecs/wgpu_systems.rs` (`for_each_fill_quad` /
   * `draw_sprite_anchored`), not the wgpu viewport: the viewport WASM
   * module owns a single global `APP` bound to one canvas, so it cannot
   * host a second surface without Rust-side work. Keep the math here in
   * sync with those functions — notably the size of a sprite with no
   * `sprite.size`: its texture's own size times its uv span
   * (`SpriteComponent::drawn_size`), falling back to 64x64 for the built-in
   * white texture or one that has not loaded yet.
   */
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
  import {
    prefabData,
    prefabDoc,
    prefabFocus,
    prefabFilePath,
    beginPrefabEdit,
    updatePrefabLive,
  } from "./stores/prefabEditorStore";
  import { framesIn } from "./prefabParts";
  import { viewOf, type PrefabEntity } from "./prefabInstance";
  import { prefabLibrary } from "./stores/prefabLibraryStore";
  import { project } from "./stores/projectStore";
  import { readAssetBinary } from "./api/project";
  import { readAnimFile } from "./api/animation";
  import { gizmoMode } from "./stores/viewportStore";
  import { logError } from "./stores/consoleStore";
  import { themeColors, withAlpha } from "./stores/themeColors";
  import type { EntityData } from "./api/scene";

  type Vec2 = [number, number];
  type Rgba = [number, number, number, number];

  /** Size of a sprite with no `sprite.size` whose texture size is unknown
   *  (built-in white, or not loaded yet) — `SpriteComponent::FALLBACK_SIZE`. */
  const DEFAULT_SPRITE_SIZE: Vec2 = [64, 64];
  /** Gizmo arm length in screen px (matches the scene viewport). */
  const ARM = 60;
  const HANDLE_HIT = 9;

  // The scene viewport's gizmo and overlay colours, from the palette.
  const COL_X = $derived($themeColors.accent);
  const COL_Y = $derived($themeColors.a2);
  const COL_FREE = $derived($themeColors.sun1);
  const COL_COLLIDER = $derived($themeColors.ok);
  const COL_TRIGGER = $derived($themeColors.warn);
  const COL_LIGHT = $derived($themeColors.a3);

  interface DragStart {
    world: Vec2;
    data: EntityData;
  }

  interface Handle {
    id: string;
    /** Screen-space anchor of the handle (CSS px). */
    sx: number;
    sy: number;
    cursor: string;
    draw: "square" | "arrowX" | "arrowY" | "dot" | "ring";
    color: string;
    hint: string;
    apply: (world: Vec2, start: DragStart, snap: boolean) => void;
  }

  // ── View state ─────────────────────────────────────────────────────
  let hostEl: HTMLDivElement | undefined = $state();
  let canvasEl: HTMLCanvasElement | undefined = $state();
  let canvasW = 0;
  let canvasH = 0;

  let camX = $state(0);
  let camY = $state(0);
  let zoom = $state(4);

  let showGrid = $state(true);
  let showOverlays = $state(true);
  let showChecker = $state(true);

  let animPlaying = $state(true);
  let animClipName = $state<string | null>(null);
  let animTime = $state(0);

  let hoverHandleId = $state<string | null>(null);
  let dragHandleId = $state<string | null>(null);
  let isPanning = $state(false);
  let spaceHeld = false;
  let statusHint = $state("");

  let handles: Handle[] = [];
  /** `opened` flips on the first mousemove so a click that never moves
   *  doesn't push a no-op undo entry. */
  let drag: { handle: Handle; start: DragStart; opened: boolean } | null = null;
  let panStart: { mx: number; my: number; camX: number; camY: number } | null =
    null;

  let rafId = 0;
  let lastT = 0;
  let resizeObs: ResizeObserver | null = null;

  // ── Texture cache ──────────────────────────────────────────────────
  const texCache = new Map<string, HTMLImageElement | null>();
  const texPending = new Set<string>();
  const scratch = document.createElement("canvas");

  function getTexture(id: string): HTMLImageElement | null {
    if (!id) return null;
    const hit = texCache.get(id);
    if (hit !== undefined) return hit;
    if (texPending.has(id)) return null;
    const proj = get(project);
    if (!proj?.path) return null;
    texPending.add(id);
    void (async () => {
      try {
        const bytes = await readAssetBinary(proj.path, id);
        const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)]));
        const img = new Image();
        img.onload = () => {
          texCache.set(id, img);
          texPending.delete(id);
          URL.revokeObjectURL(url);
        };
        img.onerror = () => {
          texCache.set(id, null);
          texPending.delete(id);
          URL.revokeObjectURL(url);
          logError(`[prefab] failed to decode texture "${id}"`);
        };
        img.src = url;
      } catch (e) {
        texCache.set(id, null);
        texPending.delete(id);
        logError(`[prefab] failed to read texture "${id}": ${e}`);
      }
    })();
    return null;
  }

  // ── Component accessors ────────────────────────────────────────────
  type AnyRec = Record<string, any>;

  function comp(d: EntityData | null, key: string): AnyRec | null {
    if (!d) return null;
    const c = (d.components as Record<string, unknown>)[key];
    return c && typeof c === "object" ? (c as AnyRec) : null;
  }

  function xf(d: EntityData | null) {
    const t = comp(d, "transform");
    return {
      pos: ((t?.position as Vec2) ?? [0, 0]) as Vec2,
      rot: (t?.rotation as number) ?? 0,
      scale: ((t?.scale as Vec2) ?? [1, 1]) as Vec2,
    };
  }

  // ── Animation ──────────────────────────────────────────────────────
  interface ClipLike {
    frames?: {
      uv_rect?: [number, number, number, number];
      texture_id?: string;
    }[];
    frame_duration?: number;
    looping?: boolean;
  }

  let externalClips = $state<Record<string, ClipLike> | null>(null);
  let externalClipSource: string | null = null;

  function clipsOf(d: EntityData | null): Record<string, ClipLike> | null {
    const sa = comp(d, "sprite_animator");
    if (!sa) return null;
    const inline = sa.clips as Record<string, ClipLike> | undefined;
    if (inline && Object.keys(inline).length > 0) return inline;
    return externalClips;
  }

  let clipNames = $derived(Object.keys(clipsOf($prefabData) ?? {}));

  let activeClip = $derived.by(() => {
    const clips = clipsOf($prefabData);
    if (!clips) return null;
    const sa = comp($prefabData, "sprite_animator");
    const preferred = sa?.current_clip as string | undefined;
    const name =
      (animClipName && clips[animClipName] ? animClipName : null) ??
      (preferred && clips[preferred] ? preferred : null) ??
      Object.keys(clips)[0] ??
      null;
    return name ? { name, clip: clips[name] } : null;
  });

  /** The frame the preview is currently showing, or null when not animated. */
  function currentFrame(d: EntityData | null) {
    const ac = activeClip;
    const frames = ac?.clip?.frames;
    if (!ac || !frames || frames.length === 0) return null;
    const sa = comp(d, "sprite_animator");
    const speed = Math.abs((sa?.speed as number) ?? 1) || 1;
    const dur = Math.max(0.01, (ac.clip.frame_duration as number) ?? 0.1);
    const looping = ac.clip.looping !== false;
    const raw = Math.floor(animTime / (dur / speed));
    const idx = looping
      ? ((raw % frames.length) + frames.length) % frames.length
      : Math.min(frames.length - 1, Math.max(0, raw));
    return { frame: frames[idx], index: idx, count: frames.length };
  }

  // Resolve `clip_source` (an .anim asset) so prefabs that reference a
  // shared clip file animate in the preview too.
  $effect(() => {
    const sa = comp($prefabData, "sprite_animator");
    const src = (sa?.clip_source as string) ?? null;
    const inline = sa?.clips as Record<string, ClipLike> | undefined;
    if (!src) {
      externalClips = null;
      externalClipSource = null;
      return;
    }
    if (inline && Object.keys(inline).length > 0) return;
    if (src === externalClipSource) return;
    externalClipSource = src;
    const proj = get(project);
    if (!proj?.path) return;
    void (async () => {
      try {
        const anim = await readAnimFile(proj.path, src);
        externalClips = anim.clips as unknown as Record<string, ClipLike>;
      } catch (e) {
        externalClips = null;
        logError(`[prefab] failed to load clip_source "${src}": ${e}`);
      }
    })();
  });

  // ── Coordinate helpers ─────────────────────────────────────────────
  function toScreen(x: number, y: number): Vec2 {
    return [canvasW / 2 + (x - camX) * zoom, canvasH / 2 - (y - camY) * zoom];
  }

  function toWorld(sx: number, sy: number): Vec2 {
    return [camX + (sx - canvasW / 2) / zoom, camY - (sy - canvasH / 2) / zoom];
  }

  function snapVal(v: number, snap: boolean): number {
    return snap ? Math.round(v) : Math.round(v * 1000) / 1000;
  }

  // ── Sprite geometry (mirrors `for_each_fill_quad`) ─────────────────
  interface Quad {
    /** Sprite size in world units, already multiplied by transform scale. */
    w: number;
    h: number;
    anchor: Vec2;
    uv: [number, number, number, number];
    textureId: string;
    color: Rgba;
  }

  function spriteQuad(d: EntityData | null): Quad | null {
    const sp = comp(d, "sprite");
    if (!sp) return null;
    const { scale } = xf(d);
    const frame = currentFrame(d)?.frame;
    const uv: [number, number, number, number] =
      frame?.uv_rect ??
      (sp.uv_rect as [number, number, number, number]) ??
      ([0, 0, 1, 1] as [number, number, number, number]);
    const textureId: string =
      (frame?.texture_id as string) ?? (sp.texture_id as string) ?? "";
    // No explicit size: the texture's own size over the part it shows, as
    // the runtime draws it. The built-in white and an unloaded texture fall
    // back to 64x64.
    let base = sp.size as Vec2 | undefined;
    if (!base) {
      const img = textureId && textureId !== "__white" ? getTexture(textureId) : null;
      base = img
        ? [img.width * Math.abs(uv[2] - uv[0]), img.height * Math.abs(uv[3] - uv[1])]
        : DEFAULT_SPRITE_SIZE;
    }
    return {
      w: base[0] * scale[0],
      h: base[1] * scale[1],
      anchor: ((sp.anchor as Vec2) ?? [0.5, 0.5]) as Vec2,
      uv,
      textureId,
      color: ((sp.color as Rgba) ?? [1, 1, 1, 1]) as Rgba,
    };
  }

  /** World-space quad corners (+Y up), in TL/TR/BR/BL order. */
  function quadCorners(q: Quad, rot: number, pos: Vec2): Vec2[] {
    const left = -q.anchor[0] * q.w;
    const right = (1 - q.anchor[0]) * q.w;
    const bottom = -q.anchor[1] * q.h;
    const top = (1 - q.anchor[1]) * q.h;
    const cos = Math.cos(rot);
    const sin = Math.sin(rot);
    const local: Vec2[] = [
      [left, top],
      [right, top],
      [right, bottom],
      [left, bottom],
    ];
    return local.map(
      ([x, y]) =>
        [pos[0] + x * cos - y * sin, pos[1] + x * sin + y * cos] as Vec2,
    );
  }

  /** The focused part as it draws: a part that is a prefab, through that
   *  prefab. Drawn and hit-tested; edits still go to the stored data. */
  let shown = $derived($prefabData ? viewOf($prefabData, $prefabLibrary.resolved) : null);

  // ── Rendering ──────────────────────────────────────────────────────
  function draw() {
    const canvas = canvasEl;
    if (!canvas || !hostEl) return;
    const dpr = window.devicePixelRatio || 1;
    const rect = hostEl.getBoundingClientRect();
    canvasW = Math.max(1, Math.floor(rect.width));
    canvasH = Math.max(1, Math.floor(rect.height));
    const pxW = Math.floor(canvasW * dpr);
    const pxH = Math.floor(canvasH * dpr);
    if (canvas.width !== pxW || canvas.height !== pxH) {
      canvas.width = pxW;
      canvas.height = pxH;
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.imageSmoothingEnabled = false;

    ctx.fillStyle = $themeColors.crust;
    ctx.fillRect(0, 0, canvasW, canvasH);

    const d = shown;
    if (showChecker) drawChecker(ctx);
    if (showGrid) drawGrid(ctx);
    drawAxes(ctx);

    if (!d) {
      handles = [];
      return;
    }

    // The prefab's other parts, where they sit around the focused one.
    drawParts(ctx);
    drawSprite(ctx, d);
    drawText(ctx, d);
    if (showOverlays) drawOverlays(ctx, d);

    handles = buildHandles(d);
    drawGizmo(ctx, d);
  }

  let checkerPattern: CanvasPattern | null = null;
  // The checker is drawn in the palette's colours, so a new palette remakes it.
  $effect(() => {
    void $themeColors;
    checkerPattern = null;
  });

  function drawChecker(ctx: CanvasRenderingContext2D) {
    if (!checkerPattern) {
      const tile = document.createElement("canvas");
      tile.width = 32;
      tile.height = 32;
      const tctx = tile.getContext("2d");
      if (!tctx) return;
      tctx.fillStyle = $themeColors.mantle;
      tctx.fillRect(0, 0, 16, 16);
      tctx.fillRect(16, 16, 16, 16);
      checkerPattern = ctx.createPattern(tile, "repeat");
    }
    if (!checkerPattern) return;
    ctx.fillStyle = checkerPattern;
    ctx.fillRect(0, 0, canvasW, canvasH);
  }

  function gridStep(): number {
    let step = 8;
    while (step * zoom < 24) step *= 2;
    while (step * zoom > 96) step /= 2;
    return step;
  }

  function drawGrid(ctx: CanvasRenderingContext2D) {
    const step = gridStep();
    const [wx0, wy1] = toWorld(0, 0);
    const [wx1, wy0] = toWorld(canvasW, canvasH);
    ctx.lineWidth = 1;
    ctx.strokeStyle = withAlpha($themeColors["line-strong"], 0.45);
    ctx.beginPath();
    for (let x = Math.floor(wx0 / step) * step; x <= wx1; x += step) {
      const sx = Math.round(toScreen(x, 0)[0]) + 0.5;
      ctx.moveTo(sx, 0);
      ctx.lineTo(sx, canvasH);
    }
    for (let y = Math.floor(wy0 / step) * step; y <= wy1; y += step) {
      const sy = Math.round(toScreen(0, y)[1]) + 0.5;
      ctx.moveTo(0, sy);
      ctx.lineTo(canvasW, sy);
    }
    ctx.stroke();
  }

  /** Prefab origin — the point a spawner places at the spawn position. */
  function drawAxes(ctx: CanvasRenderingContext2D) {
    const [ox, oy] = toScreen(0, 0);
    ctx.lineWidth = 1;
    ctx.strokeStyle = withAlpha(COL_X, 0.45);
    ctx.beginPath();
    ctx.moveTo(0, Math.round(oy) + 0.5);
    ctx.lineTo(canvasW, Math.round(oy) + 0.5);
    ctx.stroke();
    ctx.strokeStyle = withAlpha(COL_Y, 0.45);
    ctx.beginPath();
    ctx.moveTo(Math.round(ox) + 0.5, 0);
    ctx.lineTo(Math.round(ox) + 0.5, canvasH);
    ctx.stroke();
  }

  function cssColor(c: Rgba, alphaMul = 1): string {
    return `rgba(${Math.round(c[0] * 255)}, ${Math.round(c[1] * 255)}, ${Math.round(
      c[2] * 255,
    )}, ${c[3] * alphaMul})`;
  }

  /**
   * Every part but the focused one, drawn in the focused part's parent's
   * space (`prefabParts.framesIn`) - the space the gizmo edits in - so the
   * focused part sits among the rest, and its own parts follow it while it
   * is dragged. Drawn a little faded, in part order.
   */
  function drawParts(ctx: CanvasRenderingContext2D) {
    const doc = $prefabDoc;
    if (!doc || (!doc.children?.length && $prefabFocus.length === 0)) return;
    const focus = $prefabFocus.join("/");
    const screen = [zoom, 0, 0, -zoom, canvasW / 2 - camX * zoom, canvasH / 2 + camY * zoom];
    for (const { path, frame, part } of framesIn(doc as unknown as PrefabEntity, $prefabFocus)) {
      if (path.join("/") === focus) continue;
      // A part that is a prefab draws as that prefab (its sprite is there).
      const e = viewOf(part as unknown as EntityData, $prefabLibrary.resolved);
      const t = comp(e, "transform");
      // Its scale is in its frame; measure the quad without it.
      const unscaled = { ...e, components: { ...e.components, transform: { ...(t ?? {}), scale: [1, 1] } } } as EntityData;
      const q = spriteQuad(unscaled);
      if (!q) continue;
      const img = q.textureId ? getTexture(q.textureId) : null;
      ctx.save();
      ctx.globalAlpha = 0.75;
      ctx.transform(
        screen[0] * frame[0] + screen[2] * frame[1],
        screen[1] * frame[0] + screen[3] * frame[1],
        screen[0] * frame[2] + screen[2] * frame[3],
        screen[1] * frame[2] + screen[3] * frame[3],
        screen[0] * frame[4] + screen[2] * frame[5] + screen[4],
        screen[1] * frame[4] + screen[3] * frame[5] + screen[5],
      );
      // Local y runs up; the image draws y-down.
      ctx.scale(1, -1);
      const dx = -q.anchor[0] * q.w;
      const dy = -(1 - q.anchor[1]) * q.h;
      if (img) {
        const [u0, v0, u1, v1] = q.uv;
        drawTinted(
          ctx,
          img,
          [u0 * img.width, v0 * img.height, Math.max(1, (u1 - u0) * img.width), Math.max(1, (v1 - v0) * img.height)],
          dx,
          dy,
          q.w,
          q.h,
          q.color,
        );
      } else {
        ctx.fillStyle = cssColor(q.color, 0.25);
        ctx.fillRect(dx, dy, q.w, q.h);
      }
      ctx.restore();
    }
  }

  function drawSprite(ctx: CanvasRenderingContext2D, d: EntityData) {
    const q = spriteQuad(d);
    if (!q) return;
    const { pos, rot } = xf(d);
    const [sx, sy] = toScreen(pos[0], pos[1]);
    const img = q.textureId ? getTexture(q.textureId) : null;

    // Negative scale mirrors the quad about the anchor — express that as a
    // sign flip so `drawImage` always gets positive extents.
    const fx = q.w < 0 ? -1 : 1;
    const fy = q.h < 0 ? -1 : 1;
    const aw = Math.abs(q.w);
    const ah = Math.abs(q.h);
    const dx = -q.anchor[0] * aw;
    const dy = -(1 - q.anchor[1]) * ah;

    ctx.save();
    ctx.translate(sx, sy);
    ctx.rotate(-rot);
    ctx.scale(zoom * fx, zoom * fy);
    if (img) {
      const [u0, v0, u1, v1] = q.uv;
      const src: [number, number, number, number] = [
        u0 * img.width,
        v0 * img.height,
        Math.max(1, (u1 - u0) * img.width),
        Math.max(1, (v1 - v0) * img.height),
      ];
      drawTinted(ctx, img, src, dx, dy, aw, ah, q.color);
    } else {
      // Missing / still-loading texture: show the quad the engine would fill.
      ctx.fillStyle = cssColor(q.color, 0.25);
      ctx.fillRect(dx, dy, aw, ah);
      ctx.strokeStyle = cssColor(q.color, 0.9);
      ctx.lineWidth = 1 / zoom;
      ctx.strokeRect(dx, dy, aw, ah);
    }
    ctx.restore();
  }

  function drawTinted(
    ctx: CanvasRenderingContext2D,
    img: HTMLImageElement,
    src: [number, number, number, number],
    dx: number,
    dy: number,
    dw: number,
    dh: number,
    color: Rgba,
  ) {
    const [r, g, b, a] = color;
    ctx.globalAlpha = Math.max(0, Math.min(1, a));
    if (r > 0.999 && g > 0.999 && b > 0.999) {
      ctx.drawImage(img, src[0], src[1], src[2], src[3], dx, dy, dw, dh);
      ctx.globalAlpha = 1;
      return;
    }
    // Multiply tint on a scratch canvas, then mask back to the source alpha.
    const w = Math.max(1, Math.round(src[2]));
    const h = Math.max(1, Math.round(src[3]));
    scratch.width = w;
    scratch.height = h;
    const sctx = scratch.getContext("2d");
    if (!sctx) return;
    sctx.imageSmoothingEnabled = false;
    sctx.clearRect(0, 0, w, h);
    sctx.drawImage(img, src[0], src[1], src[2], src[3], 0, 0, w, h);
    sctx.globalCompositeOperation = "multiply";
    sctx.fillStyle = `rgb(${Math.round(r * 255)}, ${Math.round(g * 255)}, ${Math.round(
      b * 255,
    )})`;
    sctx.fillRect(0, 0, w, h);
    sctx.globalCompositeOperation = "destination-in";
    sctx.drawImage(img, src[0], src[1], src[2], src[3], 0, 0, w, h);
    sctx.globalCompositeOperation = "source-over";
    ctx.drawImage(scratch, 0, 0, w, h, dx, dy, dw, dh);
    ctx.globalAlpha = 1;
  }

  function drawText(ctx: CanvasRenderingContext2D, d: EntityData) {
    const tc = comp(d, "text");
    if (!tc || !tc.text) return;
    const { pos, scale } = xf(d);
    const [sx, sy] = toScreen(pos[0], pos[1]);
    // The built-in font rasterizes Noto Sans at 16px and scales glyphs by
    // `font_scale * transform.scale.x`, from a top-left origin.
    const px = 16 * ((tc.font_scale as number) ?? 1) * scale[0] * zoom;
    ctx.save();
    ctx.fillStyle = cssColor((tc.color as Rgba) ?? [1, 1, 1, 1]);
    ctx.font = `${Math.max(1, px)}px jEngineDefault`;
    ctx.textBaseline = "top";
    ctx.textAlign = "left";
    ctx.fillText(String(tc.text), sx, sy);
    ctx.restore();
  }

  function strokeWorldRect(
    ctx: CanvasRenderingContext2D,
    cx: number,
    cy: number,
    w: number,
    h: number,
    color: string,
    dashed = false,
  ) {
    const [sx, sy] = toScreen(cx - w / 2, cy + h / 2);
    ctx.save();
    ctx.strokeStyle = color;
    ctx.lineWidth = 1;
    if (dashed) ctx.setLineDash([4, 3]);
    ctx.strokeRect(sx, sy, w * zoom, h * zoom);
    ctx.restore();
  }

  function strokeWorldCircle(
    ctx: CanvasRenderingContext2D,
    cx: number,
    cy: number,
    r: number,
    color: string,
    dashed = false,
  ) {
    const [sx, sy] = toScreen(cx, cy);
    ctx.save();
    ctx.strokeStyle = color;
    ctx.lineWidth = 1;
    if (dashed) ctx.setLineDash([4, 3]);
    ctx.beginPath();
    ctx.arc(sx, sy, Math.abs(r) * zoom, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
  }

  function drawOverlays(ctx: CanvasRenderingContext2D, d: EntityData) {
    const { pos, rot } = xf(d);

    // Sprite bounds
    const q = spriteQuad(d);
    if (q) {
      const pts = quadCorners(q, rot, pos).map(([x, y]) => toScreen(x, y));
      ctx.save();
      ctx.strokeStyle = withAlpha($themeColors.accent, 0.55);
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 3]);
      ctx.beginPath();
      pts.forEach(([x, y], i) =>
        i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y),
      );
      ctx.closePath();
      ctx.stroke();
      ctx.restore();
    }

    // Physics collider — shape enum is externally tagged in JSON.
    const col = comp(d, "collider");
    if (col) {
      const off = (col.offset as Vec2) ?? [0, 0];
      const cx = pos[0] + off[0];
      const cy = pos[1] + off[1];
      const shape = col.shape as AnyRec | undefined;
      if (shape?.Box) {
        strokeWorldRect(
          ctx,
          cx,
          cy,
          shape.Box.width,
          shape.Box.height,
          COL_COLLIDER,
        );
      } else if (shape?.Circle) {
        strokeWorldCircle(ctx, cx, cy, shape.Circle.radius, COL_COLLIDER);
      } else if (shape?.Capsule) {
        const r = shape.Capsule.radius;
        const hh = shape.Capsule.half_height;
        strokeWorldRect(ctx, cx, cy, r * 2, hh * 2, COL_COLLIDER);
        strokeWorldCircle(ctx, cx, cy + hh, r, COL_COLLIDER);
        strokeWorldCircle(ctx, cx, cy - hh, r, COL_COLLIDER);
      } else if (Array.isArray(shape?.ConvexPolygon?.vertices)) {
        const verts = shape.ConvexPolygon.vertices as Vec2[];
        ctx.save();
        ctx.strokeStyle = COL_COLLIDER;
        ctx.lineWidth = 1;
        ctx.beginPath();
        verts.forEach((v, i) => {
          const [x, y] = toScreen(cx + v[0], cy + v[1]);
          if (i === 0) ctx.moveTo(x, y);
          else ctx.lineTo(x, y);
        });
        ctx.closePath();
        ctx.stroke();
        ctx.restore();
      }
    }

    // Trigger area — internally tagged (`{ "kind": "circle" | "aabb" }`).
    const trig = comp(d, "trigger_area");
    const tshape = trig?.shape as AnyRec | undefined;
    if (tshape?.kind === "circle") {
      strokeWorldCircle(
        ctx,
        pos[0],
        pos[1],
        tshape.radius ?? 0,
        COL_TRIGGER,
        true,
      );
    } else if (tshape?.kind === "aabb") {
      const he = (tshape.half_extents as Vec2) ?? [16, 16];
      strokeWorldRect(ctx, pos[0], pos[1], he[0] * 2, he[1] * 2, COL_TRIGGER, true);
    }

    // Health bar preview (drawn at 70% fill so both halves are visible).
    const hb = comp(d, "health_bar");
    if (hb) {
      const w = (hb.width as number) ?? 34;
      const h = (hb.height as number) ?? 5;
      const yo = (hb.y_offset as number) ?? 26;
      const [sx, sy] = toScreen(pos[0] - w / 2, pos[1] + yo + h / 2);
      ctx.save();
      ctx.fillStyle = withAlpha($themeColors.mantle, 0.9);
      ctx.fillRect(sx, sy, w * zoom, h * zoom);
      ctx.fillStyle = $themeColors.ok;
      ctx.fillRect(sx, sy, w * zoom * 0.7, h * zoom);
      ctx.strokeStyle = withAlpha($themeColors.text, 0.5);
      ctx.lineWidth = 1;
      ctx.strokeRect(sx, sy, w * zoom, h * zoom);
      ctx.restore();
    }

    // Point light radius
    const pl = comp(d, "point_light");
    if (pl) {
      strokeWorldCircle(
        ctx,
        pos[0],
        pos[1],
        (pl.radius as number) ?? 0,
        COL_LIGHT,
        true,
      );
    }

    // Particle emitter marker
    if (comp(d, "particle_emitter")) {
      const [sx, sy] = toScreen(pos[0], pos[1]);
      ctx.save();
      ctx.strokeStyle = $themeColors["accent-high"];
      ctx.lineWidth = 1;
      for (let i = 0; i < 8; i++) {
        const a = (i / 8) * Math.PI * 2;
        ctx.beginPath();
        ctx.moveTo(sx + Math.cos(a) * 6, sy + Math.sin(a) * 6);
        ctx.lineTo(sx + Math.cos(a) * 12, sy + Math.sin(a) * 12);
        ctx.stroke();
      }
      ctx.restore();
    }
  }

  // ── Editing ────────────────────────────────────────────────────────
  function setTransform(
    d: EntityData,
    patch: Partial<{ position: Vec2; rotation: number; scale: Vec2 }>,
  ): EntityData {
    const comps = d.components as AnyRec;
    const t = (comps.transform as AnyRec) ?? {};
    comps.transform = {
      ...t,
      position: patch.position ?? (t.position as Vec2) ?? [0, 0],
      rotation: patch.rotation ?? (t.rotation as number) ?? 0,
      scale: patch.scale ?? (t.scale as Vec2) ?? [1, 1],
    };
    return d;
  }

  /** Write `value` at `components.<path>`, leaving the rest untouched. */
  function setField(path: string[], value: unknown) {
    updatePrefabLive((dd) => {
      let node = dd.components as AnyRec;
      for (const key of path.slice(0, -1)) {
        if (!node[key] || typeof node[key] !== "object") return dd;
        node = node[key] as AnyRec;
      }
      node[path[path.length - 1]] = value;
      return dd;
    });
  }

  function buildHandles(d: EntityData): Handle[] {
    const out: Handle[] = [];
    const { pos, rot } = xf(d);
    const [ox, oy] = toScreen(pos[0], pos[1]);
    const mode = $gizmoMode;

    if (mode === "move") {
      const moveTo = (w: Vec2, s: DragStart, snap: boolean, axis: 0 | 1 | 2) => {
        const st = xf(s.data);
        const nx =
          axis === 1 ? st.pos[0] : snapVal(st.pos[0] + (w[0] - s.world[0]), snap);
        const ny =
          axis === 0 ? st.pos[1] : snapVal(st.pos[1] + (w[1] - s.world[1]), snap);
        updatePrefabLive((dd) => setTransform(dd, { position: [nx, ny] }));
      };
      out.push({
        id: "move-x",
        sx: ox + ARM,
        sy: oy,
        cursor: "ew-resize",
        draw: "arrowX",
        color: COL_X,
        hint: "Move along X",
        apply: (w, s, snap) => moveTo(w, s, snap, 0),
      });
      out.push({
        id: "move-y",
        sx: ox,
        sy: oy - ARM,
        cursor: "ns-resize",
        draw: "arrowY",
        color: COL_Y,
        hint: "Move along Y",
        apply: (w, s, snap) => moveTo(w, s, snap, 1),
      });
      out.push({
        id: "move-free",
        sx: ox,
        sy: oy,
        cursor: "move",
        draw: "square",
        color: COL_FREE,
        hint: "Move (drag the sprite too)",
        apply: (w, s, snap) => moveTo(w, s, snap, 2),
      });
    } else if (mode === "rotate") {
      out.push({
        id: "rotate",
        sx: ox + Math.cos(-rot) * ARM,
        sy: oy + Math.sin(-rot) * ARM,
        cursor: "grab",
        draw: "ring",
        color: COL_FREE,
        hint: "Rotate (Ctrl snaps to 15°)",
        apply: (w, s, snap) => {
          const st = xf(s.data);
          const a0 = Math.atan2(s.world[1] - st.pos[1], s.world[0] - st.pos[0]);
          const a1 = Math.atan2(w[1] - st.pos[1], w[0] - st.pos[0]);
          let rotation = st.rot + (a1 - a0);
          if (snap) {
            const step = Math.PI / 12; // 15°
            rotation = Math.round(rotation / step) * step;
          }
          updatePrefabLive((dd) =>
            setTransform(dd, { rotation: Math.round(rotation * 10000) / 10000 }),
          );
        },
      });
    } else {
      // Scale handles follow the entity's local axes — the engine multiplies
      // the sprite size by `scale` before applying rotation.
      const ax: Vec2 = [Math.cos(rot), Math.sin(rot)];
      const ay: Vec2 = [-Math.sin(rot), Math.cos(rot)];
      const round = (v: number, snap: boolean) =>
        snap ? Math.round(v * 10) / 10 : Math.round(v * 1000) / 1000;
      const axisScale = (
        axis: Vec2,
        index: 0 | 1,
        w: Vec2,
        s: DragStart,
        snap: boolean,
      ) => {
        const st = xf(s.data);
        const d0 =
          (s.world[0] - st.pos[0]) * axis[0] +
          (s.world[1] - st.pos[1]) * axis[1];
        const d1 = (w[0] - st.pos[0]) * axis[0] + (w[1] - st.pos[1]) * axis[1];
        if (Math.abs(d0) < 1e-4) return;
        const v = round(st.scale[index] * (d1 / d0), snap);
        const next: Vec2 = index === 0 ? [v, st.scale[1]] : [st.scale[0], v];
        updatePrefabLive((dd) => setTransform(dd, { scale: next }));
      };
      out.push({
        id: "scale-x",
        sx: ox + ax[0] * ARM,
        sy: oy - ax[1] * ARM,
        cursor: "ew-resize",
        draw: "square",
        color: COL_X,
        hint: "Scale X",
        apply: (w, s, snap) => axisScale(ax, 0, w, s, snap),
      });
      out.push({
        id: "scale-y",
        sx: ox + ay[0] * ARM,
        sy: oy - ay[1] * ARM,
        cursor: "ns-resize",
        draw: "square",
        color: COL_Y,
        hint: "Scale Y",
        apply: (w, s, snap) => axisScale(ay, 1, w, s, snap),
      });
      out.push({
        id: "scale-uniform",
        sx: ox,
        sy: oy,
        cursor: "nwse-resize",
        draw: "square",
        color: COL_FREE,
        hint: "Scale uniformly",
        apply: (w, s, snap) => {
          const st = xf(s.data);
          const d0 = Math.hypot(s.world[0] - st.pos[0], s.world[1] - st.pos[1]);
          const d1 = Math.hypot(w[0] - st.pos[0], w[1] - st.pos[1]);
          if (d0 < 1e-4) return;
          const f = d1 / d0;
          const next: Vec2 = [
            round(st.scale[0] * f, snap),
            round(st.scale[1] * f, snap),
          ];
          updatePrefabLive((dd) => setTransform(dd, { scale: next }));
        },
      });
    }

    if (showOverlays) out.push(...shapeHandles(d, pos));
    return out;
  }

  /** Draggable size handles for the shape-bearing components. */
  function shapeHandles(d: EntityData, pos: Vec2): Handle[] {
    const out: Handle[] = [];

    const trig = comp(d, "trigger_area");
    const tshape = trig?.shape as AnyRec | undefined;
    if (tshape?.kind === "circle") {
      const r = (tshape.radius as number) ?? 0;
      const [hx, hy] = toScreen(pos[0] + r, pos[1]);
      out.push({
        id: "trigger-radius",
        sx: hx,
        sy: hy,
        cursor: "ew-resize",
        draw: "dot",
        color: COL_TRIGGER,
        hint: "Trigger radius",
        apply: (w, s, snap) => {
          const p = xf(s.data).pos;
          setField(
            ["trigger_area", "shape", "radius"],
            Math.max(0, snapVal(Math.hypot(w[0] - p[0], w[1] - p[1]), snap)),
          );
        },
      });
    } else if (tshape?.kind === "aabb") {
      const he = (tshape.half_extents as Vec2) ?? [16, 16];
      const [hx, hy] = toScreen(pos[0] + he[0], pos[1] + he[1]);
      out.push({
        id: "trigger-extents",
        sx: hx,
        sy: hy,
        cursor: "nwse-resize",
        draw: "dot",
        color: COL_TRIGGER,
        hint: "Trigger half-extents",
        apply: (w, s, snap) => {
          const p = xf(s.data).pos;
          setField(
            ["trigger_area", "shape", "half_extents"],
            [
              Math.max(0, snapVal(Math.abs(w[0] - p[0]), snap)),
              Math.max(0, snapVal(Math.abs(w[1] - p[1]), snap)),
            ],
          );
        },
      });
    }

    const col = comp(d, "collider");
    const cshape = col?.shape as AnyRec | undefined;
    if (col && cshape) {
      const off = (col.offset as Vec2) ?? [0, 0];
      const cx = pos[0] + off[0];
      const cy = pos[1] + off[1];
      if (cshape.Circle) {
        const [hx, hy] = toScreen(cx + (cshape.Circle.radius ?? 0), cy);
        out.push({
          id: "collider-radius",
          sx: hx,
          sy: hy,
          cursor: "ew-resize",
          draw: "dot",
          color: COL_COLLIDER,
          hint: "Collider radius",
          apply: (w, s, snap) => {
            const p = xf(s.data).pos;
            const o = ((comp(s.data, "collider")?.offset as Vec2) ??
              [0, 0]) as Vec2;
            const r = Math.hypot(w[0] - p[0] - o[0], w[1] - p[1] - o[1]);
            setField(
              ["collider", "shape", "Circle", "radius"],
              Math.max(0, snapVal(r, snap)),
            );
          },
        });
      } else if (cshape.Box) {
        const [hx, hy] = toScreen(
          cx + (cshape.Box.width ?? 0) / 2,
          cy + (cshape.Box.height ?? 0) / 2,
        );
        out.push({
          id: "collider-box",
          sx: hx,
          sy: hy,
          cursor: "nwse-resize",
          draw: "dot",
          color: COL_COLLIDER,
          hint: "Collider size",
          apply: (w, s, snap) => {
            const p = xf(s.data).pos;
            const o = ((comp(s.data, "collider")?.offset as Vec2) ??
              [0, 0]) as Vec2;
            setField(
              ["collider", "shape", "Box", "width"],
              Math.max(0, snapVal(Math.abs(w[0] - p[0] - o[0]) * 2, snap)),
            );
            setField(
              ["collider", "shape", "Box", "height"],
              Math.max(0, snapVal(Math.abs(w[1] - p[1] - o[1]) * 2, snap)),
            );
          },
        });
      }
    }

    const hb = comp(d, "health_bar");
    if (hb) {
      const w = (hb.width as number) ?? 34;
      const yo = (hb.y_offset as number) ?? 26;
      const [hx, hy] = toScreen(pos[0] + w / 2, pos[1] + yo);
      out.push({
        id: "healthbar",
        sx: hx,
        sy: hy,
        cursor: "move",
        draw: "dot",
        color: $themeColors.ok,
        hint: "Health bar width / offset",
        apply: (wpt, s, snap) => {
          const p = xf(s.data).pos;
          setField(
            ["health_bar", "width"],
            Math.max(0, snapVal(Math.abs(wpt[0] - p[0]) * 2, snap)),
          );
          setField(["health_bar", "y_offset"], snapVal(wpt[1] - p[1], snap));
        },
      });
    }

    return out;
  }

  function drawGizmo(ctx: CanvasRenderingContext2D, d: EntityData) {
    const { pos, rot } = xf(d);
    const [ox, oy] = toScreen(pos[0], pos[1]);

    ctx.save();
    ctx.lineWidth = 2;

    if ($gizmoMode === "rotate") {
      ctx.strokeStyle = withAlpha(COL_FREE, 0.8);
      ctx.beginPath();
      ctx.arc(ox, oy, ARM, 0, Math.PI * 2);
      ctx.stroke();
      ctx.strokeStyle = withAlpha($themeColors.text, 0.5);
      ctx.beginPath();
      ctx.moveTo(ox, oy);
      ctx.lineTo(ox + Math.cos(-rot) * ARM, oy + Math.sin(-rot) * ARM);
      ctx.stroke();
    } else {
      for (const h of handles) {
        if (h.id === "move-free" || h.id === "scale-uniform") continue;
        if (!h.id.startsWith("move-") && !h.id.startsWith("scale-")) continue;
        ctx.strokeStyle = h.color;
        ctx.beginPath();
        ctx.moveTo(ox, oy);
        ctx.lineTo(h.sx, h.sy);
        ctx.stroke();
      }
    }

    for (const h of handles) {
      const active = h.id === dragHandleId || h.id === hoverHandleId;
      ctx.fillStyle = h.color;
      ctx.strokeStyle = active ? $themeColors.ink : withAlpha($themeColors.crust, 0.9);
      ctx.lineWidth = active ? 2 : 1;
      const s = active ? 6 : 5;
      if (h.draw === "arrowX") {
        ctx.beginPath();
        ctx.moveTo(h.sx + 8, h.sy);
        ctx.lineTo(h.sx - 4, h.sy - 5);
        ctx.lineTo(h.sx - 4, h.sy + 5);
        ctx.closePath();
        ctx.fill();
        ctx.stroke();
      } else if (h.draw === "arrowY") {
        ctx.beginPath();
        ctx.moveTo(h.sx, h.sy - 8);
        ctx.lineTo(h.sx - 5, h.sy + 4);
        ctx.lineTo(h.sx + 5, h.sy + 4);
        ctx.closePath();
        ctx.fill();
        ctx.stroke();
      } else if (h.draw === "dot" || h.draw === "ring") {
        ctx.beginPath();
        ctx.arc(h.sx, h.sy, s, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
      } else {
        ctx.fillRect(h.sx - s, h.sy - s, s * 2, s * 2);
        ctx.strokeRect(h.sx - s, h.sy - s, s * 2, s * 2);
      }
    }
    ctx.restore();
  }

  // ── Hit testing + interaction ──────────────────────────────────────
  function hitHandle(mx: number, my: number): Handle | null {
    let best: Handle | null = null;
    let bestD = HANDLE_HIT;
    for (const h of handles) {
      const dist = Math.hypot(h.sx - mx, h.sy - my);
      if (dist <= bestD) {
        bestD = dist;
        best = h;
      }
    }
    return best;
  }

  function pointInSprite(d: EntityData, wx: number, wy: number): boolean {
    const q = spriteQuad(d);
    if (!q) return false;
    const { pos, rot } = xf(d);
    // Rotate the point into the sprite's local frame.
    const dx = wx - pos[0];
    const dy = wy - pos[1];
    const cos = Math.cos(-rot);
    const sin = Math.sin(-rot);
    const lx = dx * cos - dy * sin;
    const ly = dx * sin + dy * cos;
    const left = Math.min(-q.anchor[0] * q.w, (1 - q.anchor[0]) * q.w);
    const right = Math.max(-q.anchor[0] * q.w, (1 - q.anchor[0]) * q.w);
    const bottom = Math.min(-q.anchor[1] * q.h, (1 - q.anchor[1]) * q.h);
    const top = Math.max(-q.anchor[1] * q.h, (1 - q.anchor[1]) * q.h);
    return lx >= left && lx <= right && ly >= bottom && ly <= top;
  }

  function localMouse(e: MouseEvent): Vec2 {
    const rect = canvasEl!.getBoundingClientRect();
    return [e.clientX - rect.left, e.clientY - rect.top];
  }

  function onMouseDown(e: MouseEvent) {
    if (!canvasEl) return;
    hostEl?.focus();
    const [mx, my] = localMouse(e);
    if (e.button === 1 || e.button === 2 || (e.button === 0 && spaceHeld)) {
      isPanning = true;
      panStart = { mx, my, camX, camY };
      window.addEventListener("mousemove", onWindowMove);
      window.addEventListener("mouseup", onWindowUp);
      e.preventDefault();
      return;
    }
    if (e.button !== 0) return;

    const d = shown;
    if (!d) return;
    let handle = hitHandle(mx, my);
    if (!handle && $gizmoMode === "move") {
      // Dragging the sprite body itself is a free move.
      const [wx, wy] = toWorld(mx, my);
      if (pointInSprite(d, wx, wy)) {
        handle = handles.find((h) => h.id === "move-free") ?? null;
      }
    }
    if (!handle) return;

    drag = {
      handle,
      start: { world: toWorld(mx, my), data: structuredClone(d) },
      opened: false,
    };
    dragHandleId = handle.id;
    statusHint = handle.hint;
    window.addEventListener("mousemove", onWindowMove);
    window.addEventListener("mouseup", onWindowUp);
    e.preventDefault();
  }

  function onWindowMove(e: MouseEvent) {
    if (!canvasEl) return;
    const [mx, my] = localMouse(e);
    if (isPanning && panStart) {
      camX = panStart.camX - (mx - panStart.mx) / zoom;
      camY = panStart.camY + (my - panStart.my) / zoom;
      return;
    }
    if (drag) {
      if (!drag.opened) {
        beginPrefabEdit();
        drag.opened = true;
      }
      drag.handle.apply(toWorld(mx, my), drag.start, e.ctrlKey || e.metaKey);
    }
  }

  function onWindowUp() {
    isPanning = false;
    panStart = null;
    drag = null;
    dragHandleId = null;
    statusHint = "";
    window.removeEventListener("mousemove", onWindowMove);
    window.removeEventListener("mouseup", onWindowUp);
  }

  function onCanvasMove(e: MouseEvent) {
    if (drag || isPanning) return;
    const [mx, my] = localMouse(e);
    const h = hitHandle(mx, my);
    hoverHandleId = h?.id ?? null;
    statusHint = h?.hint ?? "";
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    if (!canvasEl) return;
    const [mx, my] = localMouse(e);
    const [wx, wy] = toWorld(mx, my);
    const factor = e.deltaY > 0 ? 1 / 1.15 : 1.15;
    zoom = Math.max(0.1, Math.min(200, zoom * factor));
    camX = wx - (mx - canvasW / 2) / zoom;
    camY = wy + (my - canvasH / 2) / zoom;
  }

  function onKeyDown(e: KeyboardEvent) {
    // All stage shortcuts are unmodified keys — let Ctrl/Alt/Cmd combos
    // (reload, save, undo …) through untouched.
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (
      e.target instanceof HTMLInputElement ||
      e.target instanceof HTMLSelectElement ||
      e.target instanceof HTMLTextAreaElement
    )
      return;
    if (e.key === " ") {
      spaceHeld = true;
      e.preventDefault();
    } else if (e.key === "w" || e.key === "W") {
      gizmoMode.set("move");
    } else if (e.key === "e" || e.key === "E") {
      gizmoMode.set("rotate");
    } else if (e.key === "r" || e.key === "R") {
      gizmoMode.set("scale");
    } else if (e.key === "f" || e.key === "F") {
      fitView();
    } else if (e.key === "g" || e.key === "G") {
      showGrid = !showGrid;
    }
  }

  function onKeyUp(e: KeyboardEvent) {
    if (e.key === " ") spaceHeld = false;
  }

  // ── Framing ────────────────────────────────────────────────────────
  function contentBounds(d: EntityData | null) {
    let minX = -16;
    let minY = -16;
    let maxX = 16;
    let maxY = 16;
    const grow = (x: number, y: number) => {
      minX = Math.min(minX, x);
      minY = Math.min(minY, y);
      maxX = Math.max(maxX, x);
      maxY = Math.max(maxY, y);
    };
    if (d) {
      const { pos, rot } = xf(d);
      grow(pos[0], pos[1]);
      const q = spriteQuad(d);
      if (q) for (const [x, y] of quadCorners(q, rot, pos)) grow(x, y);
      const trig = comp(d, "trigger_area")?.shape as AnyRec | undefined;
      if (trig?.kind === "circle") {
        grow(pos[0] - trig.radius, pos[1] - trig.radius);
        grow(pos[0] + trig.radius, pos[1] + trig.radius);
      } else if (trig?.kind === "aabb") {
        const he = (trig.half_extents as Vec2) ?? [16, 16];
        grow(pos[0] - he[0], pos[1] - he[1]);
        grow(pos[0] + he[0], pos[1] + he[1]);
      }
      const hb = comp(d, "health_bar");
      if (hb) {
        const w = (hb.width as number) ?? 34;
        const yo = (hb.y_offset as number) ?? 26;
        grow(pos[0] - w / 2, pos[1] + yo);
        grow(pos[0] + w / 2, pos[1] + yo + ((hb.height as number) ?? 5));
      }
    }
    return { minX, minY, maxX, maxY };
  }

  function fitView() {
    if (!hostEl) return;
    const rect = hostEl.getBoundingClientRect();
    const w = Math.max(1, rect.width);
    const h = Math.max(1, rect.height);
    const b = contentBounds($prefabData);
    const bw = Math.max(1, b.maxX - b.minX);
    const bh = Math.max(1, b.maxY - b.minY);
    zoom = Math.max(0.1, Math.min(200, Math.min(w / bw, h / bh) * 0.75));
    camX = (b.minX + b.maxX) / 2;
    camY = (b.minY + b.maxY) / 2;
  }

  // Frame the prefab whenever a different file is opened.
  let framedPath: string | null = null;
  $effect(() => {
    const p = $prefabFilePath;
    if (p && p !== framedPath) {
      framedPath = p;
      animTime = 0;
      animClipName = null;
      animPlaying = (comp($prefabData, "sprite_animator")?.playing ?? true) !== false;
      queueMicrotask(fitView);
    }
  });

  // ── Lifecycle ──────────────────────────────────────────────────────
  onMount(() => {
    lastT = performance.now();
    const loop = (now: number) => {
      const dt = Math.min(0.1, (now - lastT) / 1000);
      lastT = now;
      if (animPlaying) animTime += dt;
      draw();
      rafId = requestAnimationFrame(loop);
    };
    rafId = requestAnimationFrame(loop);

    if (hostEl) {
      resizeObs = new ResizeObserver(() => draw());
      resizeObs.observe(hostEl);
      hostEl.focus();
    }
    fitView();
  });

  onDestroy(() => {
    if (rafId) cancelAnimationFrame(rafId);
    resizeObs?.disconnect();
    window.removeEventListener("mousemove", onWindowMove);
    window.removeEventListener("mouseup", onWindowUp);
  });

  let cursorStyle = $derived(
    isPanning || dragHandleId
      ? "grabbing"
      : hoverHandleId
        ? (handles.find((h) => h.id === hoverHandleId)?.cursor ?? "pointer")
        : "crosshair",
  );

  let frameLabel = $derived.by(() => {
    const ac = activeClip;
    const frames = ac?.clip?.frames;
    if (!ac || !frames?.length) return "";
    const cur = currentFrame($prefabData);
    return `frame ${(cur?.index ?? 0) + 1}/${frames.length}`;
  });
</script>

<div class="stage">
  <div class="stage-toolbar">
    <div class="tool-group">
      <button
        class="tool-btn"
        class:active={$gizmoMode === "move"}
        title="Move (W)"
        onclick={() => gizmoMode.set("move")}>✥</button
      >
      <button
        class="tool-btn"
        class:active={$gizmoMode === "rotate"}
        title="Rotate (E)"
        onclick={() => gizmoMode.set("rotate")}>⟳</button
      >
      <button
        class="tool-btn"
        class:active={$gizmoMode === "scale"}
        title="Scale (R)"
        onclick={() => gizmoMode.set("scale")}>⤢</button
      >
    </div>
    <span class="sep"></span>
    <div class="tool-group">
      <button
        class="tool-btn"
        class:active={showGrid}
        title="Toggle grid (G)"
        onclick={() => (showGrid = !showGrid)}>▦</button
      >
      <button
        class="tool-btn"
        class:active={showOverlays}
        title="Toggle collider / trigger / health-bar overlays"
        onclick={() => (showOverlays = !showOverlays)}>⬡</button
      >
      <button
        class="tool-btn"
        class:active={showChecker}
        title="Toggle checkerboard backdrop"
        onclick={() => (showChecker = !showChecker)}>◫</button
      >
    </div>
    {#if clipNames.length > 0}
      <span class="sep"></span>
      <div class="tool-group anim-group">
        <button
          class="tool-btn"
          class:active={animPlaying}
          title={animPlaying ? "Pause animation" : "Play animation"}
          onclick={() => (animPlaying = !animPlaying)}
          >{animPlaying ? "⏸" : "▶"}</button
        >
        <select
          class="clip-select"
          value={activeClip?.name ?? ""}
          onchange={(e) => {
            animClipName = (e.target as HTMLSelectElement).value;
            animTime = 0;
          }}
        >
          {#each clipNames as name}
            <option value={name}>{name}</option>
          {/each}
        </select>
        <span class="frame-label">{frameLabel}</span>
      </div>
    {/if}
    <div class="spacer"></div>
    <span class="hint">{statusHint}</span>
    <button class="tool-btn" title="Frame prefab (F)" onclick={fitView}>⛶</button
    >
    <span class="zoom">{Math.round(zoom * 100)}%</span>
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="stage-canvas"
    bind:this={hostEl}
    tabindex="-1"
    role="application"
    aria-label="Prefab preview"
    onkeydown={onKeyDown}
    onkeyup={onKeyUp}
    oncontextmenu={(e) => e.preventDefault()}
  >
    <canvas
      bind:this={canvasEl}
      style:cursor={cursorStyle}
      onmousedown={onMouseDown}
      onmousemove={onCanvasMove}
      onwheel={onWheel}
    ></canvas>
    {#if !$prefabData}
      <div class="stage-empty">No prefab loaded</div>
    {/if}
  </div>

  <div class="stage-footer">
    {#if $prefabData}
      {@const t = xf($prefabData)}
      <span class="stat">pos {t.pos[0].toFixed(2)}, {t.pos[1].toFixed(2)}</span>
      <span class="stat">rot {t.rot.toFixed(3)} rad</span>
      <span class="stat"
        >scale {t.scale[0].toFixed(2)}, {t.scale[1].toFixed(2)}</span
      >
      <div class="spacer"></div>
      <span class="stat dim"
        >drag to edit · Ctrl snaps · middle-drag pans · wheel zooms</span
      >
    {/if}
  </div>
</div>

<style>
  .stage {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    background: var(--ed-base);
  }

  .stage-toolbar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--ed-line);
    flex-shrink: 0;
  }

  .tool-group {
    display: flex;
    gap: 2px;
  }

  .anim-group {
    align-items: center;
    gap: 4px;
  }

  .tool-btn {
    background: none;
    border: 1px solid transparent;
    color: var(--ed-muted);
    min-width: 24px;
    height: 22px;
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
    line-height: 1;
    padding: 0 4px;
  }

  .tool-btn:hover {
    background: var(--ed-raised);
    color: var(--ed-text);
  }

  .tool-btn.active {
    background: color-mix(in srgb, var(--ed-accent) 16%, transparent);
    border-color: var(--ed-accent);
    color: var(--ed-accent);
  }

  .clip-select {
    background: var(--ed-mantle);
    color: var(--ed-text);
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    font-size: 11px;
    padding: 1px 4px;
    max-width: 120px;
  }

  .frame-label,
  .zoom,
  .hint {
    font-size: 11px;
    color: var(--ed-faint);
  }

  .hint {
    color: var(--ed-muted);
  }

  .sep {
    width: 1px;
    height: 16px;
    background: var(--ed-line);
  }

  .spacer {
    flex: 1;
  }

  .stage-canvas {
    position: relative;
    flex: 1;
    min-height: 0;
    outline: none;
    overflow: hidden;
  }

  .stage-canvas canvas {
    display: block;
    width: 100%;
    height: 100%;
  }

  .stage-empty {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 13px;
    color: var(--ed-faint);
    pointer-events: none;
  }

  .stage-footer {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 3px 8px;
    border-top: 1px solid var(--ed-line);
    font-size: 11px;
    color: var(--ed-muted);
    flex-shrink: 0;
  }

  .stat.dim {
    color: var(--ed-faint);
  }
</style>
