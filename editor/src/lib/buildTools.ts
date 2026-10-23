/**
 * Help > Check Build Tools: the rows `commands/build_tools.rs` reports, and
 * the logic the dialog and the build-failure hook share - no DOM, no Tauri,
 * so it is tested in `buildTools.test.ts`.
 */

export type ToolStatus = "ok" | "missing" | "outdated" | "optional_missing";
/** `core`: every build. `web`: web builds and package viewports. */
export type ToolGroup = "core" | "web" | "optional";

export type ToolAction =
  | { kind: "download"; url: string; label: string }
  | { kind: "install"; id: string; label: string };

export interface ToolRow {
  id: string;
  name: string;
  status: ToolStatus;
  group: ToolGroup;
  version: string | null;
  needed_for: string;
  detail: string | null;
  action: ToolAction | null;
  /** A second way to get it, when the action is an install. */
  help_url: string | null;
}

export interface BuildToolsReport {
  /** `windows`, `macos`, `linux`. */
  os: string;
  rows: ToolRow[];
}

/** What kind of build failed, for which rows stand in its way. */
export type BuildKind = "windows" | "linux" | "macos" | "web" | "android" | "ios" | "viewport" | "native";

export function osName(os: string): string {
  return os === "windows" ? "Windows" : os === "macos" ? "macOS" : os === "linux" ? "Linux" : os;
}

function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

/** "missing", "out of date" or both, for a set of rows that are not ok. */
function whatsWrong(rows: ToolRow[]): string {
  const outdated = rows.filter((r) => r.status === "outdated").length;
  if (outdated === 0) return "missing";
  if (outdated === rows.length) return "out of date";
  return "missing or out of date";
}

export interface Summary {
  text: string;
  /** For the line's colour and icon. */
  level: "ok" | "warn" | "err";
}

/** The one line over the rows. Optional rows never count against it. */
export function summarize(report: BuildToolsReport): Summary {
  const os = osName(report.os);
  const core = report.rows.filter((r) => r.group === "core" && r.status !== "ok");
  const web = report.rows.filter((r) => r.group === "web" && r.status !== "ok");
  if (core.length > 0) {
    return { text: `${plural(core.length, "tool")} ${whatsWrong(core)} for building games`, level: "err" };
  }
  if (web.length > 0) {
    return {
      text: `Ready to build for ${os}; ${plural(web.length, "tool")} ${whatsWrong(web)} for web builds`,
      level: "warn",
    };
  }
  return { text: `Ready to build for ${os} and web`, level: "ok" };
}

/** Rows that would have made a build of this kind fail. */
export function blockers(report: BuildToolsReport, kind: BuildKind): ToolRow[] {
  const wanted = (r: ToolRow): boolean => {
    if (r.group === "core") return true;
    if (r.group === "web") return kind === "web" || kind === "viewport";
    if (kind === "android") return r.id === "android_sdk" || r.id === "cargo_ndk";
    if (kind === "linux") return r.id === "wsl";
    return false;
  };
  return report.rows.filter((r) => r.status !== "ok" && wanted(r));
}

/** A row's status icon: a shape as well as a colour. */
export function statusGlyph(status: ToolStatus): { glyph: string; label: string } {
  switch (status) {
    case "ok":
      return { glyph: "✓", label: "Found" };
    case "missing":
      return { glyph: "✕", label: "Missing" };
    case "outdated":
      return { glyph: "!", label: "Out of date" };
    case "optional_missing":
      return { glyph: "–", label: "Not installed (optional)" };
  }
}
