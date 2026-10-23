import { invoke } from "@tauri-apps/api/core";

export interface PhysicsSettings {
  gravity_x: number;
  gravity_y: number;
  /** Physics backend: "rapier2d" or "none". */
  backend: string;
  /** Fixed timestep for physics simulation in seconds (default 1/60). */
  fixed_timestep: number;
  /** How many world units equal 1 meter (default 1.0). Higher for pixel-scale games. */
  length_unit: number;
  /** Named collision layers (16 max). */
  collision_layer_names?: string[];
  /** 16-element array of u16 bitmasks: collision_matrix[i] = which layers layer i collides with. Symmetric. Default: all-collide (0xFFFF). */
  collision_matrix?: number[];
}

export type FieldType = "Float" | "Int" | "String" | "Vec2" | "Bool" | "Color";

export interface ComponentFieldDef {
  name: string;
  field_type: FieldType;
  default_value: unknown;
}

export interface ComponentDef {
  name: string;
  fields: ComponentFieldDef[];
}

export interface InputAction {
  name: string;
  keys: string[];
  /** How far a stick or trigger binding must be pushed, 0 to 1 (0: the
   *  engine's default, 0.35). Keys ignore it. */
  dead_zone?: number;
}

export interface ProjectSettings {
  width: number;
  height: number;
  background_color: [number, number, number, number];
  /** Read by no runtime (superseded by `frame_rate_cap`); kept so a save does not drop it. */
  target_fps: number;
  /**
   * Most frames a second each platform's build draws; `0` is the display's
   * rate, a missing key the platform default (Android 60, others display).
   */
  frame_rate_cap?: { desktop?: number; web?: number; android?: number };
  title: string;
  entry_scene: string | null;
  physics: PhysicsSettings;
  input_actions: InputAction[];
  /** Preferred code editor command (e.g. "code", "cursor"). Empty = auto-detect. */
  code_editor: string;
  /** Asset pipeline settings (atlas packing & compression). */
  asset_pipeline?: AssetPipelineSettings;
  /** Which built-in packages are enabled for this project. */
  enabled_packages: string[];
  /** Graphics backends compiled into each platform's build. */
  graphics_backends?: GraphicsBackends;
  /** How the game installs on a phone. See `jengine_builder::app`. */
  app?: AppSettings;
  /** Android-only build numbers. */
  android?: AndroidSettings;
  /**
   * What the UI does when the window is not `width` x `height`.
   *
   * `"ScaleWithScreen"` (the default) scales `.gui` documents and entity UI
   * uniformly so a UI authored once fits any window. `"ConstantPixelSize"`
   * is one UI pixel per screen pixel - what every runtime did before, and
   * what a project whose scripts fit their own UI wants.
   */
  ui_scale_mode?: "ScaleWithScreen" | "ScaleAndCenter" | "ConstantPixelSize";
  /** How textures are sampled: a default and texture-id prefix overrides
   *  (`jengine_core::renderer::texture_filter`). Absent: all nearest. */
  texture_filter?: {
    default?: "nearest" | "linear";
    overrides?: Record<string, "nearest" | "linear">;
  };
  /** Which of two GPUs a built game asks for
   *  (`jengine_core::renderer::gpu_preference`). Absent: high performance. */
  gpu_preference?: "high_performance" | "low_power";
  /** The project's mixer: buses and their effects before any script touches
   *  them (Rust `audio::MixerLayout`; the Mixer panel, `mixerLayout.ts`). */
  audio_mixer?: import("../mixerLayout").Layout;
  /** Asset packs a build moves out of the game, downloaded when a script
   *  asks (Rust `asset::packs::PackSettings`). */
  asset_packs?: AssetPackSettings;
  liveops?: LiveOpsSettings;
  /** The web portal a web build is made for unless the Build panel picks
   *  another (`jengine_builder::web_platform`, docs/web-platforms.md).
   *  Absent: none. */
  web_platform?: "none" | "jcabinet" | "poki" | "crazygames" | "youtube";
  /** When the portal hears the game can be played: on the first frame, or
   *  when a script calls `Platform.game_ready()`. Absent: first frame. */
  web_game_ready?: "first_frame" | "script";
  /** Extra `<script>` tags a web build's page loads, in order. */
  web_scripts?: string[];
}

/** `settings.liveops`: the live ops service a built game asks, and the
 * remote config values it ships with (`jengine_core::liveops`). */
export interface LiveOpsSettings {
  /** The service's address, `https://ops.example.com`. Empty: off. */
  url?: string;
  /** What a built game asks for; empty is `live`. */
  channel?: string;
  /** What the editor's Play asks for; empty is `dev`. */
  editor_channel?: string;
  /** `jlo-public-...`: nothing the service did not sign is believed. */
  public_key?: string;
  /** Every remote config value, as the game ships it. */
  defaults?: Record<string, unknown>;
  /** The region rules see (`FI`); empty is the system locale's. */
  region?: string;
  /** The game's own attributes rules can target, sent as text. */
  attributes?: Record<string, unknown>;
}

/** `settings.asset_packs`: where packs are served from, and what each takes. */
export interface AssetPackSettings {
  /** `https://...` or a folder; empty is `packs/` beside the game. */
  url?: string;
  packs?: { name: string; include: string[] }[];
}

/**
 * Graphics APIs each platform's build carries.
 *
 * wgpu compiles one code path per API and selects at runtime, so listing
 * only what a target can use keeps the binary (and the wasm) smaller.
 */
export interface GraphicsBackends {
  /** `webgpu`, optionally plus `webgl` as a fallback. */
  web: string[];
  /** `dx12` and/or `vulkan`. */
  windows: string[];
  macos: string[];
  ios: string[];
  android: string[];
}

/**
 * `settings.app`: what the game is called once installed on a phone. Every
 * field is optional; the build fills gaps (see `mobileApp.ts`).
 */
export interface AppSettings {
  /** Reverse-domain id - the Android package / iOS bundle id. */
  id?: string;
  /** Name under the icon. Default: the window title. */
  name?: string;
  /** Version players see. */
  version?: string;
  /** Store build number; must rise with every upload. */
  build?: number;
  /** Square PNG, project-relative. */
  icon?: string;
  /** `#rrggbb` behind the icon where a launcher crops it. */
  icon_background?: string;
  orientation?: "landscape" | "portrait" | "any";
}

export interface AndroidSettings {
  min_sdk?: number;
  target_sdk?: number;
  abis?: string[];
  /** Hide the system bars while the game has focus. Default: on. */
  immersive?: boolean;
  /** Release keystore, project-relative. Its password is never stored. */
  keystore?: string;
  key_alias?: string;
}

export interface AssetPipelineSettings {
  atlas: {
    enabled: boolean;
    max_atlas_size: number;
    padding: number;
    power_of_two: boolean;
    exclude_patterns: string[];
  };
  compression: {
    enabled: boolean;
    format: string;
  };
}

export interface ProjectInfo {
  name: string;
  path: string;
  default_scene: string | null;
  settings: ProjectSettings;
  component_definitions: ComponentDef[];
  scene_files: string[];
  script_files: string[];
  sprite_files: string[];
  prefab_files: string[];
  gui_files: string[];
  anim_files: string[];
  /** `.behavior` graph assets (Step 7 / Slice 7.7). */
  behavior_files: string[];
}

export function defaultSettings(): ProjectSettings {
  return {
    width: 1280,
    height: 720,
    background_color: [0.1, 0.1, 0.12, 1.0],
    target_fps: 60,
    title: "jEngine Game",
    entry_scene: null,
    physics: { gravity_x: 0, gravity_y: -9.8, backend: "rapier2d", fixed_timestep: 1 / 60, length_unit: 1.0 },
    input_actions: [],
    code_editor: "",
    asset_pipeline: {
      atlas: {
        enabled: false,
        max_atlas_size: 4096,
        padding: 1,
        power_of_two: true,
        exclude_patterns: [],
      },
      compression: {
        enabled: false,
        format: "PngOptimized",
      },
    },
    enabled_packages: ["jengine-physics", "jengine-audio", "jengine-particles", "jengine-parallax", "jengine-swarm"],
    graphics_backends: {
      web: ["webgpu"],
      windows: ["dx12"],
      macos: ["metal"],
      ios: ["metal"],
      android: ["vulkan"],
    },
  };
}

export interface AssetNode {
  name: string;
  /** Path relative to assets/ using forward slashes. */
  path: string;
  is_dir: boolean;
  children: AssetNode[];
}

/** The project templates `jengine_builder::new_project` knows, by name. */
export type ProjectTemplate = "starter" | "empty";

export function createProject(
  path: string,
  name: string,
  template: ProjectTemplate = "starter",
): Promise<ProjectInfo> {
  return invoke<ProjectInfo>("create_project", { path, name, template });
}

export function openProject(path: string): Promise<ProjectInfo> {
  return invoke<ProjectInfo>("open_project", { path });
}

export function saveProject(
  path: string,
  name: string,
  defaultScene: string | null,
  settings: ProjectSettings,
  componentDefinitions: ComponentDef[],
): Promise<void> {
  return invoke("save_project", { path, name, defaultScene, settings, componentDefinitions });
}

export interface ProjectFileLists {
  scene_files: string[];
  script_files: string[];
  sprite_files: string[];
  prefab_files: string[];
  gui_files: string[];
  anim_files: string[];
  /** `.behavior` graph assets (Step 7 / Slice 7.7). */
  behavior_files: string[];
}

export function refreshProjectFiles(projectPath: string): Promise<ProjectFileLists> {
  return invoke<ProjectFileLists>("refresh_project_files", { projectPath });
}

export function listAssets(projectPath: string): Promise<string[]> {
  return invoke<string[]>("list_assets", { projectPath });
}

export function listProjectImages(projectPath: string): Promise<string[]> {
  return invoke<string[]>("list_project_images", { projectPath });
}

/** What a PSD import wrote, as paths under `assets/`. */
export interface PsdImportReport {
  prefab: string;
  sprites: string[];
  /** Layers left out, and why. */
  skipped: string[];
}

/** Import a layered `.psd` (project-relative, under `assets/`) as a sprite
 *  per layer and a prefab of them (`jengine_builder::psd_import`). */
export function importPsdLayers(
  projectPath: string,
  relativePath: string,
  origin: "centre" | "bottom" = "centre",
  includeHidden = false,
): Promise<PsdImportReport> {
  return invoke<PsdImportReport>("import_psd_layers", { projectPath, relativePath, origin, includeHidden });
}

export function readAssetBinary(projectPath: string, relativePath: string): Promise<number[]> {
  return invoke<number[]>("read_asset_binary", { projectPath, relativePath });
}

export function listAssetTree(projectPath: string): Promise<AssetNode[]> {
  return invoke<AssetNode[]>("list_asset_tree", { projectPath });
}

export interface PrefabEntry {
  name: string;
  file: string;
  data: import("./scene").EntityData;
}

export function listPrefabs(projectPath: string): Promise<PrefabEntry[]> {
  return invoke<PrefabEntry[]>("list_prefabs", { projectPath });
}

export function createAssetFolder(projectPath: string, relativePath: string): Promise<void> {
  return invoke("create_asset_folder", { projectPath, relativePath });
}

export function deleteAsset(projectPath: string, relativePath: string): Promise<void> {
  return invoke("delete_asset", { projectPath, relativePath });
}

export function renameAsset(projectPath: string, oldRelativePath: string, newRelativePath: string): Promise<void> {
  return invoke("rename_asset", { projectPath, oldRelativePath, newRelativePath });
}

/** One line of the project that names an asset. */
export interface AssetReference {
  path: string;
  line: number;
  text: string;
}

/** Every line naming the asset at `relativePath` (project-relative, a file
 *  or a folder) as a whole quoted path (`commands/references.rs`). */
export function findAssetReferences(projectPath: string, relativePath: string): Promise<AssetReference[]> {
  return invoke<AssetReference[]>("find_asset_references", { projectPath, relativePath });
}

export interface RenameSummary {
  files_changed: string[];
  references: number;
  failures: string[];
}

/** Rename or move an asset and rewrite every reference to it on disk. */
export function renameAssetWithReferences(
  projectPath: string,
  oldRelativePath: string,
  newRelativePath: string,
): Promise<RenameSummary> {
  return invoke<RenameSummary>("rename_asset_with_references", { projectPath, oldRelativePath, newRelativePath });
}

/** The same rewrite on text the editor holds (an open scene). */
export function rewriteAssetReferencesInText(
  text: string,
  oldRelativePath: string,
  newRelativePath: string,
  folder: boolean,
): Promise<[string, number]> {
  return invoke<[string, number]>("rewrite_asset_references_in_text", { text, oldRelativePath, newRelativePath, folder });
}

export function importAsset(projectPath: string, sourcePath: string, destRelativePath: string): Promise<void> {
  return invoke("import_asset", { projectPath, sourcePath, destRelativePath });
}

export function duplicateAsset(projectPath: string, relativePath: string, newRelativePath: string): Promise<void> {
  return invoke("duplicate_asset", { projectPath, relativePath, newRelativePath });
}

export function createAssetFile(projectPath: string, relativePath: string, content: string): Promise<void> {
  return invoke("create_asset_file", { projectPath, relativePath, content });
}

// Recent projects

export interface RecentProject {
  name: string;
  path: string;
}

export function getRecentProjects(): Promise<RecentProject[]> {
  return invoke<RecentProject[]>("get_recent_projects");
}

export function removeRecentProject(path: string): Promise<void> {
  return invoke("remove_recent_project", { path });
}

export function openInCodeEditor(projectPath: string, relativePath: string, editorCmd: string): Promise<void> {
  return invoke("open_in_code_editor", { projectPath, relativePath, editorCmd });
}

export function showInFileManager(projectPath: string, relativePath: string): Promise<void> {
  return invoke("show_in_file_manager", { projectPath, relativePath });
}
