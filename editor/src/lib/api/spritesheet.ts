import { invoke } from "@tauri-apps/api/core";

// ── Types ────────────────────────────────────────────────────────────

export interface SpritePoint {
  name: string;
  x: number;
  y: number;
}

export interface AtlasRegion {
  name: string;
  x: number;
  y: number;
  width: number;
  height: number;
  /** Anchor/pivot as fraction [0-1, 0-1]. Default [0.5, 0.5] = center. */
  anchor: [number, number];
  /** Custom named points on this sprite. */
  points: SpritePoint[];
}

export interface AtlasData {
  width: number;
  height: number;
  regions: AtlasRegion[];
}

export interface EditorPackConfig {
  max_size: number;
  padding: number;
  power_of_two: boolean;
  /** "grid" or "rectpack" */
  mode: string;
  cell_width?: number;
  cell_height?: number;
  columns?: number;
}

export interface SpritesheetPackResult {
  image_base64: string;
  atlas_data: AtlasData;
  width: number;
  height: number;
  /** 0-100 */
  efficiency: number;
}

export interface CompressionResult {
  original_bytes: number;
  compressed_bytes: number;
  savings_percent: number;
  format: string;
}

export interface AtlasPreviewData {
  atlas_data: AtlasData;
  image_base64: string;
}

export interface FileSavingsEstimate {
  path: string;
  original_bytes: number;
  compressed_bytes: number;
  savings_percent: number;
}

export interface SavingsEstimate {
  total_original_bytes: number;
  total_compressed_bytes: number;
  total_savings_percent: number;
  file_count: number;
  files: FileSavingsEstimate[];
}

// ── Commands ─────────────────────────────────────────────────────────

export function packSpritesheet(
  projectPath: string,
  spritePaths: string[],
  config: EditorPackConfig
): Promise<SpritesheetPackResult> {
  return invoke<SpritesheetPackResult>("pack_spritesheet", {
    projectPath,
    spritePaths,
    config,
  });
}

export function saveSpritesheet(
  projectPath: string,
  outputName: string,
  imageBase64: string,
  atlasDataJson: string
): Promise<void> {
  return invoke<void>("save_spritesheet", {
    projectPath,
    outputName,
    imageBase64,
    atlasDataJson,
  });
}

export function compressTexture(
  projectPath: string,
  relativePath: string,
  format: string,
  quality?: number
): Promise<CompressionResult> {
  return invoke<CompressionResult>("compress_texture", {
    projectPath,
    relativePath,
    format,
    quality,
  });
}

export function previewAtlas(
  projectPath: string,
  atlasPath: string
): Promise<AtlasPreviewData> {
  return invoke<AtlasPreviewData>("preview_atlas", { projectPath, atlasPath });
}

export function estimateSavings(
  projectPath: string,
  format: string,
  quality?: number
): Promise<SavingsEstimate> {
  return invoke<SavingsEstimate>("estimate_savings", {
    projectPath,
    format,
    quality,
  });
}

export interface AtlasPickerEntry {
  name: string;
  atlas_image: string;
  uv_rect: [number, number, number, number];
  size: [number, number];
  anchor: [number, number];
}

export function listAtlasEntries(
  projectPath: string
): Promise<AtlasPickerEntry[]> {
  return invoke<AtlasPickerEntry[]>("list_atlas_entries", { projectPath });
}
