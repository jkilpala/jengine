// The Lua debugger against the real runtime: Fengari compiling scripts with
// stubbed bridge functions, as `luaRuntime.test.ts` does.
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { scriptKey } from "./luaDebugger";

let lines: string[] = [];

async function setup() {
  lines = [];
  (globalThis as any).window = {
    __jengine_play: { __api_lookup: (name: string) => (name === "log" ? (s: any) => void lines.push(String(s)) : null) },
  };
  const { luaDebugger } = await import("./luaDebugger");
  const { compileLua } = await import("./lua-runtime");
  luaDebugger.reset();
  luaDebugger.clearBreakpoints();
  return { dbg: luaDebugger, compileLua };
}

beforeEach(() => {
  lines = [];
});

afterEach(() => {
  delete (globalThis as any).window;
});

const SCRIPT = `-- line 1
local function double(x)
  local y = x * 2
  return y
end
function update(e, dt)
  local hp = 30
  log("before")
  local d = double(hp)
  log("after " .. d)
end
`;

describe("the Lua debugger", () => {
  it("keys scripts by their path under assets", () => {
    expect(scriptKey("C:\\proj\\assets\\scripts\\a.lua")).toBe("scripts/a.lua");
    expect(scriptKey("=scripts/a.lua")).toBe("scripts/a.lua");
    expect(scriptKey("assets/scripts/a.lua")).toBe("scripts/a.lua");
  });

  it("stops at a breakpoint with the locals, and runs the rest on continue", async () => {
    const { dbg, compileLua } = await setup();
    const script = compileLua("scripts/hero.lua", SCRIPT);
    dbg.setBreakpoints("scripts/hero.lua", [8]);
    script.update!(7, 0.016);
    expect(dbg.paused).toBe(true);
    expect(dbg.pause?.line).toBe(8);
    expect(dbg.pause?.source).toBe("scripts/hero.lua");
    const top = dbg.pause!.frames[0];
    expect(top.name).toBe("update");
    expect(top.locals).toEqual(
      expect.arrayContaining([
        { name: "e", value: "7" },
        { name: "hp", value: "30" },
      ]),
    );
    expect(lines).toEqual([]);
    dbg.continue();
    expect(dbg.paused).toBe(false);
    expect(lines).toEqual(["before", "after 60"]);
  });

  it("runs no script while paused", async () => {
    const { dbg, compileLua } = await setup();
    const a = compileLua("scripts/a.lua", `function update() log("a") end`);
    const b = compileLua("scripts/b.lua", `function update() log("b") end`);
    dbg.setBreakpoints("scripts/a.lua", [1]);
    a.update!(1, 0);
    b.update!(2, 0);
    expect(lines).toEqual([]);
    dbg.continue();
    b.update!(2, 0);
    expect(lines).toEqual(["a", "b"]);
  });

  it("steps over a call, into it, and out of it", async () => {
    const { dbg, compileLua } = await setup();
    const script = compileLua("scripts/hero.lua", SCRIPT);
    dbg.setBreakpoints("scripts/hero.lua", [9]);
    script.update!(1, 0);
    expect(dbg.pause?.line).toBe(9);
    // Over the call to double: the next line of update.
    dbg.setBreakpoints("scripts/hero.lua", []);
    dbg.stepOver();
    expect(dbg.pause?.line).toBe(10);
    dbg.continue();
    expect(lines).toEqual(["before", "after 60"]);

    // Into it, then out again.
    lines = [];
    dbg.setBreakpoints("scripts/hero.lua", [9]);
    script.update!(1, 0);
    dbg.setBreakpoints("scripts/hero.lua", []);
    dbg.stepInto();
    expect(dbg.pause?.line).toBe(3);
    expect(dbg.pause?.frames[0].locals).toEqual(expect.arrayContaining([{ name: "x", value: "30" }]));
    expect(dbg.pause?.frames.map((f) => f.line)).toEqual([3, 9]);
    // Out: back in update. Returning into line 9 is no new line, so Lua
    // stops on the next one.
    dbg.stepOut();
    expect(dbg.pause?.line).toBe(10);
    expect(dbg.pause?.frames[0].name).toBe("update");
    dbg.continue();
    expect(lines).toEqual(["before", "after 60"]);
    expect(dbg.armed).toBe(false);
  });

  it("reports an error after a resume as the callback would have", async () => {
    const { dbg, compileLua } = await setup();
    const errors: string[] = [];
    (globalThis as any).__jengine_script_error = (m: string) => void errors.push(m);
    const script = compileLua("scripts/bad.lua", `function update()\n  local t = nil\n  return t.x\nend`);
    dbg.setBreakpoints("scripts/bad.lua", [2]);
    script.update!(1, 0);
    expect(dbg.paused).toBe(true);
    dbg.continue();
    expect(errors.some((e) => e.includes("attempt to index"))).toBe(true);
    delete (globalThis as any).__jengine_script_error;
  });

  it("does not stop in a script's top level, which runs as it loads", async () => {
    const { dbg, compileLua } = await setup();
    dbg.setBreakpoints("scripts/top.lua", [1]);
    compileLua("scripts/top.lua", `log("loaded")\nfunction update() end`);
    expect(dbg.paused).toBe(false);
    expect(lines).toEqual(["loaded"]);
  });
});
