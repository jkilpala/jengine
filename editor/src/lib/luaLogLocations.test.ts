// What a script prints carries where it printed it, when the host asks - the
// editor does, so its Console can link each line to its script and line.
// The runtime is the real one (Fengari), as in `luaDebugger.test.ts`.
import { afterEach, describe, expect, it } from "vitest";
import { classifyViewportLog } from "./viewportLog";

let lines: string[] = [];

async function compile(path: string, source: string) {
  lines = [];
  (globalThis as any).window = {
    __jengine_play: { __api_lookup: (name: string) => (name === "log" ? (s: any) => void lines.push(String(s)) : null) },
  };
  const { compileLua } = await import("./lua-runtime");
  return compileLua(path, source);
}

afterEach(() => {
  delete (globalThis as any).window;
  delete (globalThis as any).__jengine_log_locations;
});

const SCRIPT = `-- line 1
function update(e, dt)
  log("ready")
  print("hp", 30)
end
`;

describe("log locations", () => {
  it("print and log say which script and line wrote them", async () => {
    (globalThis as any).__jengine_log_locations = true;
    const script = await compile("scripts/hero.lua", SCRIPT);
    script.update!(1, 0.016);
    expect(lines).toEqual(["[@scripts/hero.lua:3] ready", "[@scripts/hero.lua:4] hp\t30"]);
    // ... which the Console reads back as the row's link and its text.
    expect(classifyViewportLog(lines[1])).toEqual({
      level: "info",
      text: "hp\t30",
      scriptPath: "scripts/hero.lua",
      scriptLine: 4,
    });
  });

  it("a host that does not ask gets the plain line", async () => {
    const script = await compile("scripts/hero.lua", SCRIPT);
    script.update!(1, 0.016);
    expect(lines).toEqual(["ready", "hp\t30"]);
  });
});
