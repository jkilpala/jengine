// The editor's Lua runtime, run for real: Fengari compiling a script against
// stubbed bridge functions, the way the viewport installs them.
//
// What this pins is the unpacking rule. A bridge function hands back a JS
// array for several values, and `makeBridge` spreads it into separate Lua
// values only for names in `MULTI_RETURN`; anything else arrives as one
// table. `get_health` was missing, so `local hp, max = get_health(e)` gave a
// table and nil in the editor and on the web (test-lab scene 36). The Rust
// test `multi_return_matches_native` keeps the list in step with native; this
// proves the list does what it is for.
import { afterEach, describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';

/** The core namespace rows, read from the one list the hosts publish from
 *  (`crates/jengine-core/src/scripting/namespaces.rs`). */
function coreRows(): [string, string, string][] {
  const src = readFileSync(
    new URL('../../../crates/jengine-core/src/scripting/namespaces.rs', import.meta.url),
    'utf8',
  );
  const table = src.slice(src.indexOf('pub const CORE'), src.indexOf('pub const GLOBALS'));
  return [...table.matchAll(/\("(\w+)", "(\w+)", "(\w+)"\)/g)].map(
    (m) => [m[1], m[2], m[3]] as [string, string, string],
  );
}

type Fns = Record<string, (...args: any[]) => any>;

async function run(source: string, fns: Fns): Promise<string[]> {
  const lines: string[] = [];
  const all: Fns = { log: (s: any) => void lines.push(String(s)), ...fns };
  (globalThis as any).window = {
    __jengine_play: { __api_lookup: (name: string) => all[name] ?? null },
  };
  const { compileLua } = await import('./lua-runtime');
  const script = compileLua('test.lua', source);
  script.update?.(1, 0.016);
  return lines;
}

afterEach(() => {
  delete (globalThis as any).window;
});

describe('multi-value bridge functions', () => {
  it('unpacks get_health into current and max', async () => {
    const out = await run(
      `function update(e, dt)
         local hp, max = get_health(7)
         log(type(hp) .. " " .. (hp + 1 == 31 and "31" or "?") .. " " .. (max == 100 and "100" or tostring(max)))
       end`,
      { get_health: () => [30, 100] },
    );
    expect(out).toEqual(['number 31 100']);
  });

  it('unpacks get_contact into its fields', async () => {
    const out = await run(
      `function update(e, dt)
         local a, b, started = get_contact(1)
         log((a == 4 and "4" or tostring(a)) .. " " .. (b == 9 and "9" or tostring(b)) .. " " .. tostring(started))
       end`,
      { get_contact: () => [4, 9, true, 0, 0, 0] },
    );
    expect(out).toEqual(['4 9 true']);
  });

  it('still hands a list-returning function its table', async () => {
    const out = await run(
      `function update(e, dt)
         local list = get_entities_by_tag("x")
         log(type(list) .. " " .. tostring(#list))
       end`,
      { get_entities_by_tag: () => [1, 2, 3] },
    );
    expect(out).toEqual(['table 3']);
  });
});

describe('a getter with no answer', () => {
  it('is one nil, so a builtin taking it as an argument does not throw', async () => {
    // The showcase hub's `tonumber(Save.get(MEMORY, "hub_top"))` on a first
    // visit, which threw "bad argument #1 to 'tonumber' (value expected)".
    const out = await run(
      `function update(e, dt)
         log(tostring(tonumber(save_get("memory", "hub_top"))) .. " " .. select("#", save_get("memory", "x")))
       end`,
      { save_get: () => null },
    );
    expect(out).toEqual(['nil 1']);
  });

  it('while a command still returns nothing', async () => {
    const out = await run(
      `function update(e, dt)
         log(tostring(select("#", save_set("memory", "x", 1))))
       end`,
      { save_set: () => undefined },
    );
    expect(out).toEqual(['0']);
  });
});

// Script inheritance, the native Luau adapter's rule (`adapters/luau.rs`,
// tests `extends_*`), run through Fengari. Before Sep 2026 this runtime
// ignored `extends`, so a script that inherited its `update` stood still in
// the editor and on the web while the "parity" tests passed - they compare
// native against the old transpiler, not against this VM.
describe('extends', () => {
  async function play(
    source: string,
    modules: Record<string, string>,
    hooks: ('init' | 'update')[] = ['update'],
  ): Promise<string[]> {
    const lines: string[] = [];
    const log = (s: any) => void lines.push(String(s));
    (globalThis as any).window = {
      __jengine_play: {
        __api_lookup: (name: string) => (name === 'log' ? log : null),
        prefab_scripts: modules,
      },
    };
    const { compileLua } = await import('./lua-runtime');
    const script = compileLua('child.lua', source);
    for (const h of hooks) {
      if (h === 'init') script.init?.(7);
      else script.update?.(7, 0.016);
    }
    return lines;
  }

  const parent = `
    local M = {}
    function M.update(e, dt) log("parent update " .. math.floor(e)) end
    function M.greet() log("parent greet") end
    return M`;

  it('inherits update when the child defines none', async () => {
    const out = await play(`script = { extends = "lib/parent" }`, { 'lib/parent.lua': parent });
    expect(out).toEqual(['parent update 7']);
  });

  it('lets the child override it', async () => {
    const out = await play(
      `script = { extends = "lib/parent" }
       function update(e, dt) log("child update") end`,
      { 'lib/parent.lua': parent },
    );
    expect(out).toEqual(['child update']);
  });

  it('reaches the parent through super', async () => {
    const out = await play(
      `script = { extends = "lib/parent" }
       function update(e, dt) super.update(e, dt); log("child after") end`,
      { 'lib/parent.lua': parent },
    );
    expect(out).toEqual(['parent update 7', 'child after']);
  });

  it('walks a chain to the grandparent', async () => {
    const out = await play(`script = { extends = "lib/parent" }`, {
      'lib/grandparent.lua': parent,
      'lib/parent.lua': `script = { extends = "lib/grandparent" }
        local M = {}
        function M.greet() log("parent greet, then"); super.greet() end
        return M`,
    }, ['init', 'update']);
    expect(out).toEqual(['parent update 7']);
  });

  it('gives a module its own super and keeps its globals to itself', async () => {
    const out = await play(
      `script = { extends = "lib/parent" }
       function init(e) greet(); log(tostring(helper)) end`,
      {
        'lib/grandparent.lua': parent,
        'lib/parent.lua': `script = { extends = "lib/grandparent" }
          function helper() end
          local M = {}
          function M.greet() log("parent greet, then"); super.greet() end
          return M`,
      },
      ['init'],
    );
    expect(out).toEqual(['parent greet, then', 'parent greet', 'nil']);
  });

  it('finds a .luau module', async () => {
    const out = await play(`script = { extends = "lib/parent" }`, {
      'assets/scripts/lib/parent.luau': parent,
    });
    expect(out).toEqual(['parent update 7']);
  });

  it('refuses a parent that cannot be found', async () => {
    await expect(play(`script = { extends = "lib/missing" }`, {})).rejects.toThrow(
      /lib\/missing.*not found/,
    );
  });

  it('refuses a parent that is not a table', async () => {
    await expect(
      play(`script = { extends = "lib/bad" }`, { 'lib/bad.lua': 'return 42' }),
    ).rejects.toThrow(/did not return a table/);
  });

  it('inherits an event handler, as native does', async () => {
    (globalThis as any).window = {
      __jengine_play: {
        __api_lookup: () => null,
        prefab_scripts: {
          'lib/parent.lua': 'local M = {}; function M.on_ping(e, data) end; return M',
        },
      },
    };
    const { compileLua } = await import('./lua-runtime');
    const script = compileLua('child.lua', 'script = { extends = "lib/parent" }');
    expect(typeof script.lookup('on_ping')).toBe('function');
    expect(script.lookup('on_pong')).toBe(null);
  });

  it('refuses a cycle rather than recursing', async () => {
    await expect(
      play(`script = { extends = "lib/a" }`, {
        'lib/a.lua': 'script = { extends = "lib/b" }\nreturn {}',
        'lib/b.lua': 'script = { extends = "lib/a" }\nreturn {}',
      }),
    ).rejects.toThrow(/circular/);
  });
});

describe('the inheritance demo', () => {
  // The engine's inheritance fixture, from disk, keyed the way the Toolbar seeds
  // `prefab_scripts` (path under `assets/`, and again with the prefix).
  // Native runs the same files in `the_inheritance_demo_inherits_across_two_levels`.
  it('inherits update two levels up and greets through super', async () => {
    const dir = new URL('../../../crates/jengine-core/tests/fixtures/inheritance/', import.meta.url);
    const read = (rel: string) => readFileSync(new URL(rel, dir), 'utf8');
    const modules: Record<string, string> = {};
    for (const rel of ['scripts/lib/creature.luau', 'scripts/lib/npc.luau']) {
      modules[rel] = read(rel);
      modules[`assets/${rel}`] = read(rel);
    }
    const lines: string[] = [];
    (globalThis as any).window = {
      __jengine_play: {
        __api_lookup: (name: string) =>
          name === 'log' ? (s: any) => void lines.push(String(s)) : null,
        prefab_scripts: modules,
      },
    };
    const { compileLua } = await import('./lua-runtime');
    const goblin = compileLua('scripts/goblin.luau', read('scripts/goblin.luau'));
    goblin.init?.(1);
    for (let i = 0; i < 60; i++) goblin.update?.(1, 0.016);
    expect(lines.some((l) => l.includes('hello — I am a npc'))).toBe(true);
    expect(lines.some((l) => l.includes('tick 60'))).toBe(true);
  });
});

describe('test-lab scene 103', () => {
  // The scene's own scripts, through this VM. Native runs the same files in
  // `crates/jengine-core/tests/scene_103_script_inheritance.rs`.
  it('spins, tints and pulses each box through what it inherits', async () => {
    const assets = new URL('../../../projects/engine-test-lab/assets/', import.meta.url);
    const read = (rel: string) => readFileSync(new URL(rel, assets), 'utf8');
    const modules: Record<string, string> = {};
    for (const rel of ['scripts/lua/inherit/spinner.lua', 'scripts/lua/inherit/tinted.lua']) {
      modules[rel] = read(rel);
      modules[`assets/${rel}`] = read(rel);
    }
    const calls: string[] = [];
    const fns: Record<string, (...a: any[]) => any> = {
      get_rotation: () => 0,
      get_scale: () => [1, 1],
      get_position: () => [0, 20],
      set_rotation: (e: number) => void calls.push(`rot ${e}`),
      set_scale: (e: number, x: number) => void calls.push(`scale ${e} ${x}`),
      set_position: (e: number) => void calls.push(`pos ${e}`),
      set_sprite_color: (e: number, r: number, g: number, b: number) =>
        void calls.push(`colour ${e} ${r} ${g} ${b}`),
      on_event: (name: string, e: number) => void calls.push(`listen ${name} ${e}`),
      register_event_listener: (name: string, e: number) => void calls.push(`listen ${name} ${e}`),
    };
    (globalThis as any).window = {
      __jengine_play: {
        __api_lookup: (n: string) => fns[n] ?? null,
        prefab_scripts: modules,
        __script_namespaces: coreRows(),
      },
    };
    const { compileLua } = await import('./lua-runtime');
    const scripts = ['scene_103_plain', 'scene_103_override', 'scene_103_two_levels'];
    scripts.forEach((name, i) => {
      const s = compileLua(`scripts/lua/${name}.lua`, read(`scripts/lua/${name}.lua`));
      s.init?.(i + 1);
      s.update?.(i + 1, 1 / 60);
      (s.lookup('on_ping') as Function)?.(i + 1, null);
    });
    for (const e of [1, 2, 3]) {
      expect(calls).toContain(`rot ${e}`);
      expect(calls).toContain(`listen ping ${e}`);
      expect(calls).toContain(`scale ${e} 1.5`);
    }
    expect(calls).toContain('pos 2');
    expect(calls).not.toContain('pos 1');
    expect(calls).toContain('colour 3 1 0.6 0.2');
    expect(calls.some((c) => c.startsWith('colour 1'))).toBe(false);
  });
});

describe('namespaces', () => {
  // Built from the rows the host publishes (`scripting::namespaces`), each
  // field wrapping the flat global under the flat name.
  async function runNs(source: string, fns: Fns, rows: [string, string, string][]) {
    const lines: string[] = [];
    const all: Fns = { log: (s: any) => void lines.push(String(s)), ...fns };
    (globalThis as any).window = {
      __jengine_play: {
        __api_lookup: (name: string) => all[name] ?? null,
        __script_namespaces: rows,
      },
    };
    const { compileLua } = await import('./lua-runtime');
    compileLua('ns.lua', source).update?.(1, 0.016);
    return lines;
  }

  it('unpacks a namespaced getter like its flat name', async () => {
    // The hand-written `sprite.get_color` this replaces returned one table.
    const out = await runNs(
      `function update(e, dt)
         local r, g, b, a = Sprite.get_color(e)
         log(tostring(r) .. " " .. tostring(a))
       end`,
      { get_sprite_color: () => [0.5, 0, 0, 1] },
      [['Sprite', 'get_color', 'get_sprite_color']],
    );
    // A whole number prints as Luau prints it: `1`, not Lua 5.3's `1.0`.
    expect(out).toEqual(['0.5 1']);
  });

  it('is the same function under both names, and Math is ours', async () => {
    const out = await runNs(
      `function update(e, dt)
         log(tostring(Math.lerp(0, 10, 0.5)))
         log(tostring(Math.lerp == lerp) .. " " .. tostring(Sprite == nil) .. " " .. tostring(math.lerp))
       end`,
      { lerp: (a: number, b: number, t: number) => a + (b - a) * t },
      [['Math', 'lerp', 'lerp']],
    );
    // No `Sprite` rows were published; Lua's `math` is left alone, as native's
    // read-only sandbox leaves it.
    expect(out).toEqual(['5', 'true true nil']);
  });
});

describe('parseExtends', () => {
  it('reads the header the way native does', async () => {
    const { parseExtends } = await import('./lua-runtime');
    expect(parseExtends(`script = { extends = "lib/npc" }`)).toBe('lib/npc');
    expect(parseExtends(`script = { tick_rate = 10, extends = 'lib/npc' }`)).toBe('lib/npc');
    expect(parseExtends(`script: { extends: "lib/npc" }`)).toBe(null); // no `=`/`{` shape after `script`
    expect(parseExtends(`-- a script that extends\nscript = { extends = "a" }`)).toBe('a');
    expect(parseExtends(`my_script = { extends = "x" }`)).toBe(null);
    expect(parseExtends(`script = { extends = lib }`)).toBe(null);
    expect(parseExtends(`local x = 1`)).toBe(null);
  });

  it('does not read a header out of a comment', async () => {
    const { parseExtends } = await import('./lua-runtime');
    // Scene 103's controller: its first line describes `extends`.
    expect(parseExtends('-- `script = { extends = "..." }` and `super`.\nlocal nav = require("lua/nav")')).toBe(null);
    expect(parseExtends('--[[ script = { extends = "a" } ]]\n--[==[ script = { extends = "b" } ]==]')).toBe(null);
    expect(parseExtends('/* script = { extends: "c" } */\n// script = { extends: "d" }')).toBe(null);
    expect(parseExtends('-- like script = { extends = "no" }\nscript = { extends = "lib/yes" }')).toBe('lib/yes');
  });
});

describe('tables a script passes to the bridge', () => {
  it('an empty table is an empty list, as on native', async () => {
    let seen: any = 'unset';
    let filled: any = 'unset';
    await run(
      `function update(e, dt)
         set_custom(e, "Probe", "empty", {})
         set_custom(e, "Probe", "map", { a = 1 })
       end`,
      {
        set_custom: (_e: any, _c: any, field: string, v: any) => {
          if (field === 'empty') seen = v;
          else filled = v;
        },
      },
    );
    expect(seen).toEqual([]);
    expect(Array.isArray(seen)).toBe(true);
    expect(filled).toEqual({ a: 1 });
  });
});
