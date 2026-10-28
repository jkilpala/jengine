import { invoke } from "@tauri-apps/api/core";
import type { WasmOptLevel } from "./editorPrefs";

export function buildWindows(projectPath: string, outputPath: string, release: boolean, generateReport: boolean, enableProfiling: boolean, debugOverlay: boolean): Promise<string> {
  return invoke<string>("build_windows", { projectPath, outputPath, release, generateReport, enableProfiling, debugOverlay });
}

/**
 * `platform`: the web portal to build for (`none`, `jcabinet`, `poki`,
 * `crazygames`, `youtube`), or `project` for the project's `web_platform`.
 * See docs/web-platforms.md.
 */
export function buildWeb(projectPath: string, outputPath: string, release: boolean, generateReport: boolean, wasmLto: boolean, wasmOptSize: boolean, wasmSingleCodegen: boolean, wasmOpt: WasmOptLevel, debugOverlay: boolean, logging: boolean, platform: string = "project"): Promise<string> {
  return invoke<string>("build_web", { projectPath, outputPath, release, generateReport, wasmLto, wasmOptSize, wasmSingleCodegen, wasmOpt, debugOverlay, logging, platform });
}

/**
 * A Linux folder (built inside WSL from Windows) or a macOS `.app` (on a Mac
 * only). Resolves to the folder or the bundle.
 */
export function buildDesktop(platform: "linux" | "macos", projectPath: string, outputPath: string, release: boolean, generateReport: boolean, debugOverlay: boolean): Promise<string> {
  return invoke<string>("build_desktop", { platform, projectPath, outputPath, release, generateReport, debugOverlay });
}

/**
 * An iOS `.app`, on a Mac only: the simulator (ad-hoc signed) or a device
 * (`identity` and `profile`). Resolves to the bundle's path.
 */
export function buildIos(
  projectPath: string,
  outputPath: string,
  release: boolean,
  debugOverlay: boolean,
  device: boolean,
  install: boolean,
  launch: boolean,
  identity: string | null,
  profile: string | null,
): Promise<string> {
  return invoke<string>("build_ios", { projectPath, outputPath, release, debugOverlay, device, install, launch, identity, profile });
}

/**
 * Build an APK (and with `bundle` the `.aab` Google Play takes); `install` /
 * `launch` go through `adb`. `releaseKey` signs with the project's keystore,
 * whose password is passed for this build only. Resolves to the bundle's path
 * when one was made, else the APK's.
 */
export function buildAndroid(
  projectPath: string,
  outputPath: string,
  release: boolean,
  debugOverlay: boolean,
  install: boolean,
  launch: boolean,
  bundle: boolean,
  releaseKey: boolean,
  keystorePassword: string | null,
): Promise<string> {
  return invoke<string>("build_android", {
    projectPath,
    outputPath,
    release,
    debugOverlay,
    install,
    launch,
    bundle,
    releaseKey,
    keystorePassword,
  });
}

export function openBuildReport(reportPath: string): Promise<void> {
  return invoke<void>("open_build_report", { reportPath });
}

export function openBuildFolder(folderPath: string): Promise<void> {
  return invoke<void>("open_build_folder", { folderPath });
}

/// Whether the loaded viewport module carries a project's Rust packages.
///
/// The viewport is one prebuilt WASM module shared by every project, so it
/// carries whichever project it was last built for. Without this the symptom
/// of a mismatch is a package that silently does nothing.
export interface ViewportStatus {
  needs_rebuild: boolean;
  reason: string;
  /** Packages the loaded module carries. */
  linked: string[];
  /** Packages this project declares. */
  wanted: string[];
  /** Project the loaded module was built for; empty for a stock build. */
  built_for: string;
  /** This project has a current build of its own, so `switchProjectViewport`
   * loads it without compiling. Meaningful only with `needs_rebuild`. */
  cached: boolean;
}

export function viewportPackageStatus(projectPath: string): Promise<ViewportStatus> {
  return invoke<ViewportStatus>("viewport_package_status", { projectPath });
}

/** Copy this project's own cached viewport build into the editor if nothing it
 * was compiled from has changed. `false`: there is none, rebuild instead.
 * `true`: the editor must reload to load it. */
export function switchProjectViewport(projectPath: string): Promise<boolean> {
  return invoke<boolean>("switch_project_viewport", { projectPath });
}

/** Rebuild the viewport with this project's packages. Needs an editor reload. */
export function rebuildProjectViewport(projectPath: string): Promise<string[]> {
  return invoke<string[]>("rebuild_project_viewport", { projectPath });
}

/** Compile the native runtime and start `instances` copies from the project
 * folder (each with its own saves, tiled on screen); `scene` is a path under
 * `assets/`, or null for the project's start scene. */
export function buildAndRunNative(projectPath: string, scene: string | null, instances: number): Promise<string> {
  return invoke<string>("build_and_run_native", { projectPath, scene, instances });
}

/** Close every copy the last Build & Run started; how many were running. */
export function stopNativeInstances(): Promise<number> {
  return invoke<number>("stop_native_instances");
}

export function nativeInstancesRunning(): Promise<number> {
  return invoke<number>("native_instances_running");
}
