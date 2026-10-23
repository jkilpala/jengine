import { invoke } from "@tauri-apps/api/core";
import type { WasmOptLevel } from "./editorPrefs";

export interface EditorSettings {
  build_output_dir: string;
  build_profile: "debug" | "release";
  build_target: "windows" | "web" | "android";
  build_generate_report: boolean;
  code_editor: string;
  show_colliders: boolean;
  show_profiler: boolean;
  wasm_lto: boolean;
  wasm_opt_size: boolean;
  wasm_single_codegen: boolean;
  /** The wasm-opt pass after a release web build - this project's own. */
  wasm_opt_level: WasmOptLevel;
  /** Windows debug builds only: emit a run-profiled.cmd launcher with
   * JENGINE_SCRIPT_PROFILE set so per-phase script timings get logged. */
  build_enable_profiling: boolean;
  /** Ship the F3 stats HUD / F12 inspector in the built game. */
  build_debug_overlay: boolean;
}

export function defaultEditorSettings(): EditorSettings {
  return {
    build_output_dir: "",
    build_profile: "release",
    build_target: "windows",
    build_generate_report: true,
    code_editor: "",
    show_colliders: false,
    show_profiler: false,
    wasm_lto: false,
    wasm_opt_size: false,
    wasm_single_codegen: false,
    wasm_opt_level: "optimized",
    build_enable_profiling: false,
    build_debug_overlay: true,
  };
}

export function loadEditorSettings(projectPath: string): Promise<EditorSettings> {
  return invoke<EditorSettings>("load_editor_settings", { projectPath });
}

export function saveEditorSettings(projectPath: string, settings: EditorSettings): Promise<void> {
  return invoke<void>("save_editor_settings", { projectPath, settings });
}
