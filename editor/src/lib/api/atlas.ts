import { invoke } from "@tauri-apps/api/core";

// ── Types ────────────────────────────────────────────────────────────

export interface SpritePoint {
  name: string;
  x: number;
  y: number;
}

export interface ImageEntry {
  name: string;
  path: string;
  anchor: [number, number];
  points: SpritePoint[];
}

export interface AnimationEntry {
  name: string;
  fps: number;
  looping: boolean;
  frames: string[];
  points: SpritePoint[];
  frame_points: Record<number, SpritePoint[]>;
}

export interface AtlasDefinition {
  name: string;
  images: ImageEntry[];
  animations: AnimationEntry[];
  shared_points: SpritePoint[];
}

export interface ImageDimensions {
  width: number;
  height: number;
}

export interface AtlasImagePreview {
  image_base64: string;
  width: number;
  height: number;
}

// ── Commands ─────────────────────────────────────────────────────────

/** Create a new empty atlas definition file. Returns relative path. */
export function createAtlas(
  projectPath: string,
  name: string
): Promise<string> {
  return invoke<string>("create_atlas", {
    projectPath,
    name,
  });
}

/** Load an atlas definition from disk. */
export function loadAtlas(
  projectPath: string,
  atlasPath: string
): Promise<AtlasDefinition> {
  return invoke<AtlasDefinition>("load_atlas", {
    projectPath,
    atlasPath,
  });
}

/** Save an atlas definition to disk. */
export function saveAtlas(
  projectPath: string,
  atlasPath: string,
  definition: AtlasDefinition
): Promise<void> {
  return invoke<void>("save_atlas", {
    projectPath,
    atlasPath,
    definition,
  });
}

/** Get pixel dimensions of an image file. */
export function getImageDimensions(
  projectPath: string,
  imagePath: string
): Promise<ImageDimensions> {
  return invoke<ImageDimensions>("get_image_dimensions", {
    projectPath,
    imagePath,
  });
}

/** Load an image as base64 PNG for canvas rendering. */
export function loadAtlasImage(
  projectPath: string,
  imagePath: string
): Promise<AtlasImagePreview> {
  return invoke<AtlasImagePreview>("load_atlas_image", {
    projectPath,
    imagePath,
  });
}

// ── Atlas definition browsing (for Inspector pickers) ────────────────

export interface AtlasDefImageEntry {
  atlas_name: string;
  atlas_path: string;
  image_name: string;
  image_path: string;
  anchor: [number, number];
}

export interface AtlasDefAnimationEntry {
  atlas_name: string;
  atlas_path: string;
  animation_name: string;
  fps: number;
  looping: boolean;
  frame_count: number;
  frame_paths: string[];
}

/** List all images across all atlas definitions (for sprite picker). */
export function listAtlasDefImages(
  projectPath: string
): Promise<AtlasDefImageEntry[]> {
  return invoke<AtlasDefImageEntry[]>("list_atlas_def_images", {
    projectPath,
  });
}

/** List all animations across all atlas definitions (for animator import). */
export function listAtlasDefAnimations(
  projectPath: string
): Promise<AtlasDefAnimationEntry[]> {
  return invoke<AtlasDefAnimationEntry[]>("list_atlas_def_animations", {
    projectPath,
  });
}
