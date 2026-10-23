import { writable, get } from "svelte/store";
import { toast } from "./toastStore";
import { project } from "./projectStore";

export interface LogEntry {
  level: "info" | "warn" | "error";
  message: string;
  timestamp: number;
  /** Optional script asset path — enables click-to-open in console. */
  scriptPath?: string;
  /** Optional 1-based line number within the script — enables jump-to-line. */
  scriptLine?: number;
  /** Where a line not about a script came from (`viewport`, `scene`); the
   *  Console shows `editor` for a line with neither this nor a script. */
  source?: string;
}

export const logs = writable<LogEntry[]>([]);

export function logInfo(message: string, scriptPath?: string, scriptLine?: number, source?: string) {
  logs.update((l) => [...l, { level: "info", message, timestamp: Date.now(), scriptPath, scriptLine, source }]);
}

export function logWarn(message: string, scriptPath?: string, scriptLine?: number, source?: string) {
  logs.update((l) => [...l, { level: "warn", message, timestamp: Date.now(), scriptPath, scriptLine, source }]);
}

export function logError(message: string, scriptPath?: string, scriptLine?: number, source?: string) {
  logs.update((l) => [...l, { level: "error", message, timestamp: Date.now(), scriptPath, scriptLine, source }]);
  // A script's error is worth seeing from any panel: a toast that opens it.
  if (scriptPath) {
    const file = scriptPath.split(/[\\/]/).pop() ?? scriptPath;
    toast({
      kind: "err",
      title: "Script error",
      detail: scriptLine ? `${file}:${scriptLine}` : file,
      mono: true,
      action: () => openScriptAt(scriptPath, scriptLine),
    });
  }
}

/** Open a script (a path under `assets/`) in the script editor at a line. */
export function openScriptAt(scriptPath: string, line?: number) {
  const proj = get(project);
  if (!proj) return;
  window.dispatchEvent(
    new CustomEvent("open-script-editor", {
      detail: { fullPath: `${proj.path}/assets/${scriptPath}`, line },
    }),
  );
}

export function clearLogs() {
  logs.set([]);
}
