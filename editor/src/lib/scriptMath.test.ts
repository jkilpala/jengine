// A script's maths in the editor's Lua: the patched Fengari's `%` and `^`
// (`scripts/patch-fengari.mjs`) and the `math` table `lua-runtime.ts` fills
// from the engine (`globalThis.__jengine_script_math`) - the rules the native
// Luau computes by, so a number is the same in both
// (docs/determinism-proposal.md, phase 3). The values Rust gives are checked
// in `luau.rs`'s `script_maths_come_from_script_math`; this checks the
// browser half routes to the same place.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

let lines: unknown[] = [];

// Numbers leave through a stub package function, which hands them over as
// JavaScript numbers - `log` would print them at Lua's 14 digits.
// `num(i)` hands a script the `i`-th of `nums`, exactly.
async function run(body: string, nums: number[] = []): Promise<unknown[]> {
  lines = [];
  const api: Record<string, (...a: any[]) => unknown> = {
    probe: (v: unknown) => void lines.push(v),
    num: (i: number) => nums[i - 1],
  };
  (globalThis as any).window = {
    __jengine_play: {
      __package_api_names: Object.keys(api),
      __api_lookup: (name: string) => api[name] ?? null,
    },
  };
  const { compileLua } = await import("./lua-runtime");
  const script = compileLua("maths.lua", `function init(e)\n${body}\nend\n`);
  script.init(1);
  return lines;
}

afterEach(() => {
  delete (globalThis as any).window;
  delete (globalThis as any).__jengine_script_math;
});

describe("script maths in Fengari", () => {
  it("is the patched bundle", () => {
    const bundle = readFileSync(resolve(__dirname, "../../node_modules/fengari-web/dist/fengari-web.bundle.js"), "utf8");
    for (const mark of ["/*jengine-script-maths-1*/", "/*jengine-script-numbers-2*/"]) {
      expect(bundle.includes(mark), `run \`npm install\` - its postinstall patches fengari-web (${mark})`).toBe(true);
    }
  });

  it("computes % and ^ by Luau's rules", async () => {
    const out = await run(`
      local a, b, e, h = 5.3, 0.1, 0.5, 3
      probe(a % b)
      probe(2 ^ e)
      probe(2 ^ 0.5)
      probe(1.1 ^ h)
      probe(1.1 ^ 3)
      probe(1.7 ^ 2)
    `);
    // Luau's `a - floor(a / b) * b`, not Lua 5.3's fmod - they differ here.
    const luau = 5.3 - Math.floor(5.3 / 0.1) * 0.1;
    let fmod = 5.3 % 0.1;
    if (fmod * 0.1 < 0) fmod += 0.1;
    expect(luau).not.toBe(fmod);
    expect(out).toEqual([luau, Math.sqrt(2), Math.sqrt(2), 1.1 * 1.1 * 1.1, 1.1 * 1.1 * 1.1, 1.7 * 1.7]);
  });

  it("asks the engine for every other number", async () => {
    const seeds: number[] = [];
    (globalThis as any).__jengine_script_math = {
      sin: () => 11, cos: () => 12, tan: () => 13, asin: () => 14, acos: () => 15, atan: () => 16,
      atan2: () => 17, exp: () => 18, log: (_x: number, base?: number) => (base === undefined ? 19 : 20),
      log10: () => 21, pow: () => 22, sinh: () => 23, cosh: () => 24, tanh: () => 25,
      random: (m?: number, n?: number) => (m === undefined ? 0.25 : n === undefined ? 5 : 4),
      randomseed: (x: number) => void seeds.push(x),
    };
    const out = await run(`
      probe(math.sin(1)); probe(math.cos(1)); probe(math.tan(1)); probe(math.asin(0.5)); probe(math.acos(0.5))
      probe(math.atan(1)); probe(math.atan(1, 2)); probe(math.atan2(1, 2)); probe(math.exp(1))
      probe(math.log(2)); probe(math.log(2, 3)); probe(math.log10(2)); probe(math.pow(2, 3))
      probe(math.sinh(1)); probe(math.cosh(1)); probe(math.tanh(1))
      local e = 2.3
      probe(1.7 ^ e)
      math.randomseed(42)
      probe(math.random()); probe(math.random(6)); probe(math.random(1, 6))
      probe(tostring(math.random(1, 6)))
    `);
    expect(out).toEqual([11, 12, 13, 14, 15, 16, 17, 17, 18, 19, 20, 21, 22, 23, 24, 25, 22, 0.25, 5, 4, "4"]);
    expect(seeds).toEqual([42]);
  });

  // Luau's numbers are doubles; Fengari's whole numbers were 32-bit and
  // wrapped (docs/parity-gaps.md).
  it("does whole-number arithmetic past 2^31 as Luau does", async () => {
    const out = await run(`
      local a, b = 2000000000, 2147483647
      probe(a + a); probe(b + 1); probe(-b - 2); probe(65536 * 65536); probe(-(-2147483648))
      probe(2000000000 + 2000000000)
      probe(7 // 2); probe(-7 // 2); probe(-7 % 3); probe(7 % -3)
      probe(4294967296 // 3); probe(a * a)
      local z = 0
      probe(5 // z); probe(5 % z ~= 5 % z)
      probe(math.type(3 + 4)); probe(math.type(a + a))
      local n = 0
      for i = 1, 3 do n = n + b end
      probe(n)
    `);
    expect(out).toEqual([
      4e9, 2147483648, -2147483649, 4294967296, 2147483648,
      4e9,
      3, -4, 2, -2,
      1431655765, 4e18,
      Infinity, true,
      "integer", "float",
      3 * 2147483647,
    ]);
  });

  it("prints numbers as the native Luau does", async () => {
    // Written by `luau.rs`'s `numbers_print_as_the_fixture_says`.
    const rows: { bits: string; text: string; d: string | null }[] = JSON.parse(
      readFileSync(resolve(__dirname, "../../../crates/jengine-core/tests/fixtures/lua_number_text.json"), "utf8"),
    );
    const view = new DataView(new ArrayBuffer(8));
    const nums = rows.map((r) => (view.setBigUint64(0, BigInt("0x" + r.bits)), view.getFloat64(0)));
    const out = await run(
      `for i = 1, ${nums.length} do
        local x = num(i)
        probe(tostring(x)); probe(x .. ""); probe(string.format("%d", x))
      end`,
      nums,
    );
    const wrong: string[] = [];
    rows.forEach((r, i) => {
      const [text, cat, d] = out.slice(i * 3, i * 3 + 3);
      if (text !== r.text || cat !== r.text) wrong.push(`${nums[i]}: Luau "${r.text}", Fengari "${text}" / "${cat}"`);
      if (r.d !== null && d !== r.d) wrong.push(`%d of ${nums[i]}: Luau "${r.d}", Fengari "${d}"`);
    });
    expect(wrong).toEqual([]);
  });
});
