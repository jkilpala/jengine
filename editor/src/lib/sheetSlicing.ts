/**
 * Cutting a sprite sheet into named regions - the maths behind the sheet
 * slicer (`SheetSlicer.svelte`), which writes them as an `.atlas` of packed
 * metadata beside the image (Rust `renderer::atlas::AtlasData`): a scene
 * lists that `.atlas` in its `atlas_refs` and a sprite names a region.
 *
 * Three ways to cut: a grid of a cell size (with an offset and spacing, as
 * sheets exported with margins have), a grid of so many columns and rows,
 * and **automatic** - every island of opaque pixels, for a sheet whose
 * sprites are packed loosely.
 */

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Region extends Rect {
  name: string;
}

export interface GridOptions {
  cellWidth: number;
  cellHeight: number;
  offsetX?: number;
  offsetY?: number;
  spacingX?: number;
  spacingY?: number;
}

/** Whole cells of `cellWidth` x `cellHeight` across the image, row by row
 *  (a cell running off the edge is left out). */
export function sliceGrid(imageWidth: number, imageHeight: number, o: GridOptions): Rect[] {
  const out: Rect[] = [];
  const cw = Math.floor(o.cellWidth);
  const ch = Math.floor(o.cellHeight);
  if (cw <= 0 || ch <= 0) return out;
  const sx = o.spacingX ?? 0;
  const sy = o.spacingY ?? 0;
  for (let y = o.offsetY ?? 0; y + ch <= imageHeight; y += ch + sy) {
    for (let x = o.offsetX ?? 0; x + cw <= imageWidth; x += cw + sx) {
      out.push({ x, y, width: cw, height: ch });
    }
  }
  return out;
}

/** `columns` x `rows` cells filling the image (the remainder of an uneven
 *  division is left at the right and bottom). */
export function sliceCount(imageWidth: number, imageHeight: number, columns: number, rows: number): Rect[] {
  if (columns < 1 || rows < 1) return [];
  return sliceGrid(imageWidth, imageHeight, {
    cellWidth: Math.floor(imageWidth / columns),
    cellHeight: Math.floor(imageHeight / rows),
  });
}

/** Whether any pixel in `r` is more opaque than `alphaMin` (RGBA bytes,
 *  `width` pixels a row). */
export function hasContent(rgba: ArrayLike<number>, width: number, r: Rect, alphaMin = 0): boolean {
  for (let y = r.y; y < r.y + r.height; y++) {
    for (let x = r.x; x < r.x + r.width; x++) {
      if (rgba[(y * width + x) * 4 + 3] > alphaMin) return true;
    }
  }
  return false;
}

/** Every island of pixels more opaque than `alphaMin` (8-connected) as its
 *  bounding box; boxes nearer than `gap` pixels are merged, so a sprite
 *  drawn in separate strokes (an eye apart from a face) stays one region.
 *  Boxes smaller than `minSize` on both sides (specks) are dropped. Ordered
 *  in reading order: by rows of boxes that overlap vertically, then by x. */
export function sliceAuto(
  rgba: ArrayLike<number>,
  width: number,
  height: number,
  { alphaMin = 0, gap = 1, minSize = 2 }: { alphaMin?: number; gap?: number; minSize?: number } = {},
): Rect[] {
  const seen = new Uint8Array(width * height);
  const boxes: Rect[] = [];
  const stack: number[] = [];
  for (let start = 0; start < width * height; start++) {
    if (seen[start] || rgba[start * 4 + 3] <= alphaMin) continue;
    let minX = width;
    let minY = height;
    let maxX = -1;
    let maxY = -1;
    stack.push(start);
    seen[start] = 1;
    while (stack.length > 0) {
      const p = stack.pop()!;
      const x = p % width;
      const y = (p - x) / width;
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          const nx = x + dx;
          const ny = y + dy;
          if (nx < 0 || ny < 0 || nx >= width || ny >= height) continue;
          const q = ny * width + nx;
          if (!seen[q] && rgba[q * 4 + 3] > alphaMin) {
            seen[q] = 1;
            stack.push(q);
          }
        }
      }
    }
    boxes.push({ x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1 });
  }
  return readingOrder(mergeNear(boxes, gap).filter((b) => b.width >= minSize || b.height >= minSize));
}

/** Merge boxes whose gap is under `gap` pixels, until none are. */
function mergeNear(boxes: Rect[], gap: number): Rect[] {
  const out = boxes.map((b) => ({ ...b }));
  let merged = true;
  while (merged) {
    merged = false;
    for (let i = 0; i < out.length && !merged; i++) {
      for (let j = i + 1; j < out.length; j++) {
        const a = out[i];
        const b = out[j];
        const apartX = Math.max(a.x, b.x) - Math.min(a.x + a.width, b.x + b.width);
        const apartY = Math.max(a.y, b.y) - Math.min(a.y + a.height, b.y + b.height);
        if (apartX < gap && apartY < gap) {
          const x = Math.min(a.x, b.x);
          const y = Math.min(a.y, b.y);
          out[i] = {
            x,
            y,
            width: Math.max(a.x + a.width, b.x + b.width) - x,
            height: Math.max(a.y + a.height, b.y + b.height) - y,
          };
          out.splice(j, 1);
          merged = true;
          break;
        }
      }
    }
  }
  return out;
}

/** Boxes in reading order: grouped into rows of boxes that overlap
 *  vertically, top row first, each row left to right. */
export function readingOrder(boxes: Rect[]): Rect[] {
  const sorted = [...boxes].sort((a, b) => a.y - b.y || a.x - b.x);
  const rows: Rect[][] = [];
  for (const b of sorted) {
    const row = rows.find((r) => r.some((o) => b.y < o.y + o.height && o.y < b.y + b.height));
    if (row) row.push(b);
    else rows.push([b]);
  }
  return rows.flatMap((r) => r.sort((a, b) => a.x - b.x));
}

/** Which row each rect is in (rects that overlap vertically share one),
 *  counting from 0 at the top. */
export function rowsOf(rects: Rect[]): number[] {
  const order = readingOrder(rects);
  const rowOf = new Map<Rect, number>();
  let row = -1;
  let bottom = -Infinity;
  let top = -Infinity;
  for (const r of order) {
    if (!(r.y < bottom && r.y + r.height > top)) {
      row++;
      top = r.y;
      bottom = r.y + r.height;
    } else {
      top = Math.min(top, r.y);
      bottom = Math.max(bottom, r.y + r.height);
    }
    rowOf.set(r, row);
  }
  return rects.map((r) => rowOf.get(r) ?? 0);
}

/** Name rects `<base>_0`, `<base>_1`, ... in the order given. */
export function nameRegions(rects: Rect[], base: string): Region[] {
  const stem = base.trim() || "region";
  return rects.map((r, i) => ({ name: `${stem}_${i}`, ...r }));
}

/** The names that are used more than once (a region is found by name). */
export function duplicateNames(regions: Region[]): string[] {
  const seen = new Set<string>();
  const dup = new Set<string>();
  for (const r of regions) (seen.has(r.name) ? dup : seen).add(r.name);
  return [...dup];
}

export interface AtlasClip {
  name: string;
  fps: number;
  looping: boolean;
  frames: { region_name: string }[];
}

/** An animation clip playing `regions` in order (one row of a sheet). */
export function clipFrom(name: string, regions: Region[], fps = 12, looping = true): AtlasClip {
  return { name, fps, looping, frames: regions.map((r) => ({ region_name: r.name })) };
}

/** The packed metadata an `.atlas` beside the image holds. */
export function atlasJson(width: number, height: number, regions: Region[], clips: AtlasClip[] = []) {
  const out: Record<string, unknown> = {
    width,
    height,
    regions: regions.map((r) => ({ name: r.name, x: r.x, y: r.y, width: r.width, height: r.height })),
  };
  if (clips.length > 0) out.animation_clips = clips;
  return out;
}

/** `sprites/hero.png` -> `sprites/hero.atlas`: the file the engine pairs with
 *  the image (`derive_atlas_id`, the reverse). */
export function atlasPathFor(imagePath: string): string {
  return imagePath.replace(/\.[^./]+$/, "") + ".atlas";
}
