import { describe, expect, it } from "vitest";
import { blockers, statusGlyph, summarize, type BuildToolsReport, type ToolRow, type ToolStatus } from "./buildTools";

function row(id: string, group: ToolRow["group"], status: ToolStatus): ToolRow {
  return { id, name: id, status, group, version: null, needed_for: "x", detail: null, action: null, help_url: null };
}

function report(rows: ToolRow[], os = "windows"): BuildToolsReport {
  return { os, rows };
}

const allOk = [
  row("rustc", "core", "ok"),
  row("cargo", "core", "ok"),
  row("msvc", "core", "ok"),
  row("wasm32_target", "web", "ok"),
  row("wasm_pack", "web", "ok"),
  row("android_sdk", "optional", "optional_missing"),
  row("wsl", "optional", "optional_missing"),
];

describe("summarize", () => {
  it("is ready when every required row is ok, whatever the optional ones say", () => {
    expect(summarize(report(allOk))).toEqual({ text: "Ready to build for Windows and web", level: "ok" });
    expect(summarize(report(allOk, "macos")).text).toBe("Ready to build for macOS and web");
  });

  it("counts what is missing for every build first", () => {
    const rows = allOk.map((r) => (r.id === "msvc" || r.id === "cargo" ? { ...r, status: "missing" as const } : r));
    expect(summarize(report(rows))).toEqual({ text: "2 tools missing for building games", level: "err" });
  });

  it("says out of date when that is all it is", () => {
    const rows = allOk.map((r) => (r.id === "rustc" ? { ...r, status: "outdated" as const } : r));
    expect(summarize(report(rows)).text).toBe("1 tool out of date for building games");
    const mixed = rows.map((r) => (r.id === "msvc" ? { ...r, status: "missing" as const } : r));
    expect(summarize(report(mixed)).text).toBe("2 tools missing or out of date for building games");
  });

  it("is ready for the desktop with only web tools missing", () => {
    const rows = allOk.map((r) => (r.group === "web" ? { ...r, status: "missing" as const } : r));
    expect(summarize(report(rows))).toEqual({
      text: "Ready to build for Windows; 2 tools missing for web builds",
      level: "warn",
    });
  });
});

describe("blockers", () => {
  const rows = allOk.map((r) => (r.group === "web" ? { ...r, status: "missing" as const } : r));

  it("does not blame web tools for a Windows build", () => {
    expect(blockers(report(rows), "windows")).toEqual([]);
  });

  it("blames them for a web build and a viewport rebuild", () => {
    expect(blockers(report(rows), "web").map((r) => r.id)).toEqual(["wasm32_target", "wasm_pack"]);
    expect(blockers(report(rows), "viewport").map((r) => r.id)).toEqual(["wasm32_target", "wasm_pack"]);
  });

  it("names the platform's own optional tools", () => {
    expect(blockers(report(allOk), "android").map((r) => r.id)).toEqual(["android_sdk"]);
    expect(blockers(report(allOk), "linux").map((r) => r.id)).toEqual(["wsl"]);
  });
});

it("every status has its own shape", () => {
  const glyphs = (["ok", "missing", "outdated", "optional_missing"] as const).map((s) => statusGlyph(s).glyph);
  expect(new Set(glyphs).size).toBe(4);
});
