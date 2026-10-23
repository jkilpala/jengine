import { invoke } from "@tauri-apps/api/core";
import { writeTextFile } from "@tauri-apps/plugin-fs";

/** Metadata for a single glyph. Matches Rust `GlyphInfo`. */
export interface GlyphInfoData {
  ch: string;
  x: number;
  y: number;
  width: number;
  height: number;
  advance: number;
  /** Horizontal bearing. Optional in the file; Rust defaults it to 0. */
  x_offset?: number;
  /** Pixels from the line top down to the glyph bitmap top. Optional in the
   *  file; Rust defaults it to 0. TTF-derived fonts need it or every
   *  descender sits on the baseline. */
  y_offset?: number;
}

/** Font file data. Matches Rust `FontData`, plus the editor-only `texture_id`.
 *
 *  Every field Rust reads must appear here even if the editor never shows it:
 *  the editor round-trips this object through `JSON.parse` → save, so a field
 *  missing from the interface is a field silently deleted from the file. That
 *  cost `pixel_size`, `x_offset` and `y_offset` until Sep 2026 — opening a
 *  TTF-derived font and saving it flattened every descender.
 *
 *  `texture_id` is **not** read by the engine, which pairs a font with the
 *  sibling image by stem (`ui.font` → `ui.png`). It is kept for the editor's
 *  atlas preview and defaults to that sibling. */
export interface FontFileData {
  texture_id: string;
  atlas_width: number;
  atlas_height: number;
  line_height: number;
  /** Native rasterization size. Absent means 1.0 — legacy hand-authored
   *  bitmap fonts, where `scale` was a raw multiplier. Changing that default
   *  resizes every one of them. */
  pixel_size?: number;
  /** Atlas pixels per layout pixel: glyph rects and offsets are atlas
   *  pixels, advances and the line height layout ones. Absent means 1. */
  atlas_scale?: number;
  /** The atlas is a distance field (`jengine font-sdf`), drawn sharp at any
   *  size. Its alpha is a distance, not coverage, so it looks blurred in the
   *  atlas preview. */
  sdf?: boolean;
  /** How far a distance field's glyph rects reach past the ink, in atlas
   *  pixels. */
  sdf_spread?: number;
  glyphs: GlyphInfoData[];
}

/** Default font file with no glyphs. */
export function defaultFontFile(): FontFileData {
  return {
    texture_id: "",
    atlas_width: 256,
    atlas_height: 128,
    line_height: 16,
    pixel_size: 1,
    glyphs: [],
  };
}

/** The atlas image a `.font` pairs with: the sibling file with the same stem.
 *
 *  Mirrors `font_texture_id` in `renderer/font_asset.rs`, which is what the
 *  runtime actually resolves. Paths here are project-relative asset paths. */
export function siblingAtlasPath(fontPath: string): string {
  const normalized = fontPath.replace(/\\/g, "/");
  const slash = normalized.lastIndexOf("/");
  const dot = normalized.lastIndexOf(".");
  // A dot in a *directory* is not an extension.
  return dot > slash ? `${normalized.slice(0, dot)}.png` : `${normalized}.png`;
}

/**
 * Generate glyph entries for a monospace grid atlas.
 * Assumes characters are laid out left-to-right, top-to-bottom in a grid.
 */
export function generateGridFont(
  atlasWidth: number,
  atlasHeight: number,
  cols: number,
  rows: number,
  startChar: number = 32,
  endChar: number = 126,
): GlyphInfoData[] {
  const cellW = Math.floor(atlasWidth / cols);
  const cellH = Math.floor(atlasHeight / rows);
  const glyphs: GlyphInfoData[] = [];
  const total = cols * rows;
  for (let i = 0; i <= endChar - startChar && i < total; i++) {
    const code = startChar + i;
    const col = i % cols;
    const row = Math.floor(i / cols);
    glyphs.push({
      ch: String.fromCharCode(code),
      x: col * cellW,
      y: row * cellH,
      width: cellW,
      height: cellH,
      advance: cellW,
    });
  }
  return glyphs;
}

/** Read a `.font` file from the project assets. */
export async function readFontFile(
  projectPath: string,
  relativePath: string,
): Promise<FontFileData> {
  const bytes = await invoke<number[]>("read_asset_binary", {
    projectPath,
    relativePath,
  });
  const text = new TextDecoder().decode(new Uint8Array(bytes));
  return JSON.parse(text) as FontFileData;
}

/** Save a `.font` file to disk (overwrites existing). */
export async function saveFontFile(
  fullPath: string,
  data: FontFileData,
): Promise<void> {
  const json = JSON.stringify(data, null, 2);
  await writeTextFile(fullPath, json);
}
