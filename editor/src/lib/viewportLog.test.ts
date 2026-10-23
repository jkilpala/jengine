import { describe, expect, it } from "vitest";
import { classifyViewportLog } from "./viewportLog";

describe("viewport log lines", () => {
  it("a script that does not compile is an error, linked to its line", () => {
    const line = classifyViewportLog(
      "[viewport] compile 'scripts/player.lua' failed: Lua load 'scripts/player.lua': scripts/player.lua:21: syntax error near 'if'",
    );
    expect(line).toMatchObject({ level: "error", scriptPath: "scripts/player.lua", scriptLine: 21 });
    expect(line.text.startsWith("compile 'scripts/player.lua' failed")).toBe(true);
  });

  it("a runtime error is an error", () => {
    expect(classifyViewportLog("scripts/hud.lua:77: attempt to call a nil value (field 'set_label')").level).toBe("error");
    expect(classifyViewportLog("[ERROR] [hud.lua] boom").level).toBe("error");
  });

  it("a summary that counted failures is a warning, a clean one is not", () => {
    expect(classifyViewportLog("[viewport] scripts: loaded=0, errors=1").level).toBe("warn");
    expect(classifyViewportLog("[viewport] scripts: loaded=3, errors=0").level).toBe("info");
  });

  it("a print says where it was called from, and the tag leaves the text", () => {
    expect(classifyViewportLog("[@scripts/player.lua:18] player ready")).toEqual({
      level: "info",
      text: "player ready",
      scriptPath: "scripts/player.lua",
      scriptLine: 18,
    });
  });

  it("a line no script wrote names who did", () => {
    expect(classifyViewportLog("[viewport] entering play mode")).toEqual({
      level: "info",
      text: "entering play mode",
      source: "viewport",
    });
    expect(classifyViewportLog("player ready")).toEqual({ level: "info", text: "player ready", source: "script" });
  });

  it("a bracketed script still links", () => {
    expect(classifyViewportLog("[player.lua] dash ready")).toMatchObject({ level: "info", scriptPath: "player.lua" });
  });
});
