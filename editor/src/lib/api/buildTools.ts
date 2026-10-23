import { invoke } from "@tauri-apps/api/core";
import { blockers, type BuildKind, type BuildToolsReport } from "../buildTools";
import { toast } from "../stores/toastStore";

/** Every check (`commands/build_tools.rs`). Takes a few seconds. */
export function checkBuildTools(): Promise<BuildToolsReport> {
  return invoke<BuildToolsReport>("check_build_tools");
}

/** Install `wasm32_target` or `wasm_pack`; output goes to the Build panel. */
export function installBuildTool(id: string): Promise<string> {
  return invoke<string>("install_build_tool", { id });
}

/** The window event the Toolbar opens the dialog on. */
export const BUILD_TOOLS_EVENT = "editor-build-tools";

export function openBuildToolsDialog() {
  window.dispatchEvent(new CustomEvent(BUILD_TOOLS_EVENT));
}

/**
 * After a build fails: if a tool that build needs is missing, say so with a
 * toast that opens the dialog. Quiet when everything is there - the failure
 * is then the build's own, and its toast already says so.
 */
export async function checkToolsAfterBuildFailure(kind: BuildKind): Promise<void> {
  try {
    const missing = blockers(await checkBuildTools(), kind);
    if (missing.length === 0) return;
    toast({
      kind: "warn",
      title: missing.length === 1 ? "A build tool is missing" : `${missing.length} build tools are missing`,
      detail: `${missing.map((r) => r.name).join(", ")} - click to check build tools.`,
      action: openBuildToolsDialog,
    });
  } catch {
    // The check itself failing is no reason to add noise to a failed build.
  }
}
