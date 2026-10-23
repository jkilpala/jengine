/**
 * Factory for creating entities from asset files dropped into the scene.
 * Maps asset file extensions to the correct component bundles.
 */

import type { EntityData, ComponentBundle } from "./scene";
import { readParticleFile } from "./particle";
import { readAnimFile } from "./animation";
import { readTilemapFile } from "./tilemap";
import { invoke } from "@tauri-apps/api/core";
import { newInstance, prefabKey } from "../prefabInstance";

/** MIME type used in drag-and-drop data transfer for engine assets. */
export const ASSET_DRAG_MIME = "application/jengine-asset";

/** Supported asset extensions that can be dropped to create entities. */
const DROPPABLE_EXTENSIONS = new Set([
  "png", "jpg", "jpeg", "webp",
  "particle",
  "anim",
  "gui",
  "lua", "js",
  "behavior",
  "ogg", "wav", "mp3", "flac",
  "prefab",
  "tilemap",
]);;

/** Check whether a file name can be dropped to create an entity. */
export function isDroppableAsset(fileName: string): boolean {
  const ext = fileName.split(".").pop()?.toLowerCase() ?? "";
  return DROPPABLE_EXTENSIONS.has(ext);
}

/**
 * Build an entity name from the asset file name (without extension).
 * e.g. "fire.particle" → "fire", "player.png" → "player"
 */
function entityNameFromFile(fileName: string): string {
  return fileName.replace(/\.[^.]+$/, "");
}

/**
 * Strip the leading "assets/" prefix if present — component fields like
 * `texture_id` and `source_path` use paths relative to the assets folder.
 */
function stripAssetsPrefix(path: string): string {
  return path.startsWith("assets/") ? path.slice("assets/".length) : path;
}

/**
 * Create an entity from an asset file.
 *
 * @param projectPath  Absolute path to the project root
 * @param relativePath Path relative to the project root (e.g. "assets/fire.particle")
 * @param fileName     Just the file name (e.g. "fire.particle")
 * @returns Fully populated EntityData ready to append to the scene
 */
export async function createEntityFromAsset(
  projectPath: string,
  relativePath: string,
  fileName: string,
): Promise<EntityData> {
  const ext = fileName.split(".").pop()?.toLowerCase() ?? "";
  const name = entityNameFromFile(fileName);
  // Component fields (texture_id, source_path, etc.) use paths relative to assets/
  const assetPath = stripAssetsPrefix(relativePath);
  // read_asset_binary expects a path relative to the project's assets/ folder
  const readPath = assetPath;

  // ── Prefabs: load full entity data from disk ──
  if (ext === "prefab") {
    const bytes = await invoke<number[]>("read_asset_binary", {
      projectPath,
      relativePath: readPath,
    });
    const text = new TextDecoder().decode(new Uint8Array(bytes));
    const parsed = JSON.parse(text) as Partial<EntityData>;
    // A linked instance, not a copy: the scene keeps the prefab's id and its
    // own transform, so editing the prefab later reaches this entity.
    const id = prefabKey({ name: fileName.replace(/\.prefab$/i, ""), data: parsed });
    const entity = newInstance(id, { name: parsed.name ?? name, components: parsed.components ?? {} });
    return entity as EntityData;
  }

  const components: ComponentBundle = {
    transform: { position: [0, 0], rotation: 0, scale: [1, 1] },
  };

  switch (ext) {
    // ── Images → Sprite ──
    case "png":
    case "jpg":
    case "jpeg":
    case "webp":
      components.sprite = {
        texture_id: assetPath,
        color: [1, 1, 1, 1],
        z_order: 0,
      };
      break;

    // ── Particle files → ParticleEmitter ──
    case "particle": {
      const data = await readParticleFile(projectPath, readPath);
      components.particle_emitter = data;
      break;
    }

    // ── Animation files → SpriteAnimator ──
    case "anim": {
      const data = await readAnimFile(projectPath, readPath);
      components.sprite = {
        texture_id: data.texture_id ?? "",
        color: [1, 1, 1, 1],
        z_order: 0,
      };
      const clips: Record<string, unknown> = {};
      for (const [name, clip] of Object.entries(data.clips)) {
        clips[name] = {
          name: clip.name,
          frames: clip.frames.map((f) => ({
            uv_rect: [...f.uv_rect],
            texture_id: f.texture_id,
            triggers: f.triggers ?? [],
          })),
          frame_duration: clip.frame_duration,
          looping: clip.looping,
        };
      }
      const firstClip = Object.keys(clips)[0] ?? "";
      components.sprite_animator = {
        clips,
        current_clip: firstClip,
        current_frame: 0,
        elapsed: 0,
        playing: false,
        speed: 1,
      };
      break;
    }

    // ── GUI files → GUI component ──
    case "gui":
      components.gui = {
        gui_id: assetPath,
        visible: true,
        variables: {},
      };
      break;

    // ── Scripts → Script component ──
    case "lua":
      components.script = { source_path: assetPath, language: "lua" };
      break;
    case "js":
      components.script = { source_path: assetPath, language: "javascript" };
      break;

    // ── Behavior graphs → BehaviorGraph component ──
    case "behavior":
      components.behavior_graph = { graph_path: assetPath, overrides: {} };
      break;

    // ── Tilemap files → Tilemap component (data inlined, source kept) ──
    case "tilemap": {
      const data = await readTilemapFile(projectPath, assetPath);
      components.tilemap = {
        source: assetPath,
        tile_size: data.tile_size,
        tilesets: data.tilesets,
        layers: data.layers,
      };
      break;
    }

    // ── Audio → AudioSource ──
    case "ogg":
    case "wav":
    case "mp3":
    case "flac":
      components.audio_source = {
        clip_id: assetPath,
        volume: 1,
        pitch: 1,
        looping: false,
        auto_play: false,
      };
      break;
  }

  return { name, components };
}
