import { invoke } from "@tauri-apps/api/core";

/**
 * Preferences of the editor itself, for every project
 * (`editor/src-tauri/src/commands/editor_prefs.rs`).
 */
export interface EditorPrefs {
  /** Which GPU the editor draws on, from its next start:
   *  `high_performance` passes WebView2 `--force_high_performance_gpu`;
   *  `system` lets Windows choose (usually the integrated GPU). */
  gpu: "high_performance" | "system";
  /** How hard the editor viewport's WebAssembly is optimised when it is
   *  built (`jengine_builder::wasm_opt`). */
  viewport_wasm_opt: WasmOptLevel;
}

/** A `wasm-opt` level (`jengine_builder::wasm_opt::WasmOpt`). */
export type WasmOptLevel = "off" | "fast" | "optimized" | "size" | "max";

/** The levels with the words the settings show. The timings are the editor
 *  viewport's (16 MB of code, Oct 2026); a game's module is smaller. The
 *  levels hardly differ in speed (`jengine-benchmark-cpu`), and `fast` is the
 *  smallest *download* - the others only shrink the raw module
 *  (docs/build-sizes.md). The viewport defaults to `optimized`, a game's web
 *  build to `fast`. */
export const WASM_OPT_LEVELS: { id: WasmOptLevel; label: string }[] = [
  { id: "off", label: "Off - fastest build, largest module" },
  { id: "fast", label: "Fast (-O1) - seconds, smallest download" },
  { id: "optimized", label: "Optimized (-O) - minutes, ~5% smaller raw" },
  { id: "size", label: "Size (-Oz) - minutes, smallest raw module" },
  { id: "max", label: "Max (-O3) - slowest build, ~3% faster code" },
];

export function getEditorPrefs(): Promise<EditorPrefs> {
  return invoke<EditorPrefs>("get_editor_prefs");
}

export function setEditorPrefs(prefs: EditorPrefs): Promise<void> {
  return invoke("set_editor_prefs", { prefs });
}
