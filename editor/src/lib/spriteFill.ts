/**
 * A sprite's `fill` (Rust `SpriteFill`, an externally tagged enum):
 * `"Stretch"` (or absent - the default), `{ NineSlice: { left, right, top,
 * bottom } }` with the borders in the texture's **pixels**, or `{ Tile:
 * { tile_size } }` in world units. The Inspector's fill editor
 * (`SpriteFillEditor.svelte`) reads and writes it through these.
 */
export type Borders = { left: number; right: number; top: number; bottom: number };
export type Fill =
  | { kind: "stretch" }
  | { kind: "nine"; borders: Borders }
  | { kind: "tile"; tileSize: [number, number] };

/** Read a component's `fill`, whatever spelling it was authored in. */
export function readFill(raw: unknown): Fill {
  if (raw && typeof raw === "object") {
    const o = raw as Record<string, any>;
    if (o.NineSlice) {
      const n = o.NineSlice;
      return { kind: "nine", borders: { left: +n.left || 0, right: +n.right || 0, top: +n.top || 0, bottom: +n.bottom || 0 } };
    }
    if (o.Tile) {
      const t = o.Tile.tile_size ?? [32, 32];
      return { kind: "tile", tileSize: [+t[0] || 32, +t[1] || 32] };
    }
  }
  return { kind: "stretch" };
}

/** The component's `fill` for `f`: absent for a plain stretch. */
export function writeFill(f: Fill): unknown {
  switch (f.kind) {
    case "nine":
      return { NineSlice: { ...f.borders } };
    case "tile":
      return { Tile: { tile_size: [...f.tileSize] } };
    default:
      return undefined;
  }
}

/** Borders kept inside a `w` x `h` source: each at least 0, and opposite
 *  borders together no wider than the source (the engine clamps too, but
 *  then the editor would show what does not draw). */
export function clampBorders(b: Borders, w: number, h: number): Borders {
  const c = (v: number) => Math.max(0, Math.round(v));
  let { left, right, top, bottom } = { left: c(b.left), right: c(b.right), top: c(b.top), bottom: c(b.bottom) };
  if (w > 0 && left + right > w) right = Math.max(0, w - left);
  if (h > 0 && top + bottom > h) bottom = Math.max(0, h - top);
  return { left, right, top, bottom };
}
