import { invoke } from "@tauri-apps/api/core";

/** What the About box says about this build (`commands/help.rs`). */
export interface AboutInfo {
  /** The workspace version - the engine's, not the frontend package's. */
  version: string;
  /** Short commit hash, or `unknown` when the build had no git. */
  commit: string;
  /** `YYYY-MM-DD`. */
  build_date: string;
  profile: string;
  os: string;
  arch: string;
  tauri: string;
}

export function aboutInfo(): Promise<AboutInfo> {
  return invoke("about_info");
}

/** Open a web page in the system browser. Only `http(s)` addresses. */
export function openExternalUrl(url: string): Promise<void> {
  return invoke("open_external_url", { url });
}

/** Where the editor writes its log. */
export function logsFolder(): Promise<string> {
  return invoke("logs_folder");
}

/** Open that folder in the system file manager. */
export function openLogsFolder(): Promise<void> {
  return invoke("open_logs_folder");
}
