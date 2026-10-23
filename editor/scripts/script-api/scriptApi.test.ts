// The script editor's completions against the engine.
//
// Three promises: the generated API file is what the generator would write
// now (so a function added to the engine reaches autocomplete); every name
// the completions offer is one a runtime installs (no lowercase namespaces,
// no `set_parallax` that nothing binds); and every callback offered is one
// the engine calls, and every callback core queues is offered.
//
// Rewrite the generated file with
//   JENGINE_UPDATE_SCRIPT_API=1 npx vitest run scripts/script-api
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  buildScriptApi,
  OUTPUT,
  readApiIndex,
  readNamespaceTable,
  readRepo,
  REPO_ROOT,
  serialise,
} from './generate';
import { JS_API_LABELS, LUA_API_LABELS } from '../../src/lib/scriptCompletions';
import { SCRIPT_CALLBACKS } from '../../src/lib/scriptCallbacks';

function rustFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) out.push(...rustFiles(p));
    else if (p.endsWith('.rs')) out.push(p);
  }
  return out;
}

/** What the runtimes are made of - not their tests. */
function runtimeSource(): string {
  const dirs = ['crates/jengine-core/src', 'crates/jengine-runtime/src', 'crates/jengine-web/src', 'crates/jengine-editor-viewport/src'];
  const parts = dirs.flatMap((d) => rustFiles(resolve(REPO_ROOT, d))).map((f) => {
    const src = readFileSync(f, 'utf8');
    // Drop an inline `#[cfg(test)] mod tests { ... }` (always last in a file here).
    const test = src.indexOf('#[cfg(test)]\nmod tests');
    return test >= 0 ? src.slice(0, test) : src;
  });
  parts.push(readRepo('editor/src/lib/lua-runtime.ts'));
  return parts.join('\n');
}

describe('generated script API', () => {
  it('is current', async () => {
    const want = serialise(await buildScriptApi());
    if (process.env.JENGINE_UPDATE_SCRIPT_API) {
      writeFileSync(OUTPUT, want, 'utf8');
      return;
    }
    const have = readFileSync(OUTPUT, 'utf8').replace(/\r\n/g, '\n');
    expect(
      have === want,
      'editor/src/lib/generated/script-api.json is stale - run ' +
        '`JENGINE_UPDATE_SCRIPT_API=1 npx vitest run scripts/script-api` in editor/',
    ).toBe(true);
  }, 60_000);

  it('covers every core function the API index says is installed everywhere', async () => {
    const data = await buildScriptApi();
    const offered = new Set(data.namespaces.flatMap((ns) => ns.functions.map((f) => `${ns.name}.${f.name}`)));
    const installed = readApiIndex().filter((r) => r.call.includes('.') && r.runtimes === 'all');
    expect(installed.length).toBeGreaterThan(300);
    expect(installed.map((r) => r.call).filter((c) => !offered.has(c))).toEqual([]);
  }, 60_000);
});

describe('completions', () => {
  const { core, globals } = readNamespaceTable();
  const luau = readRepo('crates/jengine-core/src/scripting/bridge/adapters/luau.rs');
  const luauGlobals = new Set([...luau.matchAll(/globals(?:\(\))?\.set\(\s*"([a-z_0-9]+)"/g)].map((m) => m[1]));
  const ts = readRepo('editor/src/lib/lua-runtime.ts');
  const bridgeBlock = ts.slice(ts.indexOf('const BRIDGE_FUNCTIONS'), ts.indexOf('];', ts.indexOf('const BRIDGE_FUNCTIONS')));
  const bridge = new Set([...bridgeBlock.matchAll(/["']([a-z_0-9]+)["']/g)].map((m) => m[1]));

  it('offer only capitalised namespaces', () => {
    const namespaces = LUA_API_LABELS.filter((l) => !l.includes('.') && !globals.includes(l));
    expect(namespaces.filter((n) => !/^[A-Z]/.test(n))).toEqual([]);
  });

  it('name only functions a runtime installs', async () => {
    const coreCalls = new Map(core.map(([ns, short, flat]) => [`${ns}.${short}`, flat]));
    const data = await buildScriptApi();
    const packageCalls = new Set(
      data.namespaces.filter((n) => n.source !== 'core').flatMap((n) => n.functions.map((f) => `${n.name}.${f.name}`)),
    );
    // Package commands come from the packages' own `register_script_commands`
    // calls, read again here rather than trusted from the generated file.
    const { readPackages } = await import(
      /* @vite-ignore */ new URL(`file:///${resolve(REPO_ROOT, 'website/scripts/extract-packages.ts').replace(/\\/g, '/')}`).href
    );
    const registered = new Set<string>(
      (readPackages() as Array<{ commands: Array<{ call: string }> }>).flatMap((p) => p.commands.map((c) => c.call)),
    );

    const bad: string[] = [];
    for (const label of LUA_API_LABELS) {
      if (!label.includes('.')) {
        const isNamespace =
          core.some(([ns]) => ns === label) || data.namespaces.some((n) => n.name === label && n.source !== 'core');
        if (!isNamespace && !globals.includes(label)) bad.push(`${label}: neither a namespace nor a global`);
        if (globals.includes(label) && !luauGlobals.has(label) && label !== 'print')
          bad.push(`${label}: not bound natively`);
        continue;
      }
      const flat = coreCalls.get(label);
      if (flat) {
        if (!luauGlobals.has(flat)) bad.push(`${label} (${flat}): not bound in luau.rs`);
        if (!bridge.has(flat)) bad.push(`${label} (${flat}): not in BRIDGE_FUNCTIONS`);
      } else if (!packageCalls.has(label) || !registered.has(label)) {
        bad.push(`${label}: in no namespace table and no package`);
      }
    }
    expect(bad).toEqual([]);

    const flats = new Set([...coreCalls.values(), ...globals]);
    const packageFlats = new Set(data.namespaces.filter((n) => n.source !== 'core').flatMap((n) => n.functions.map((f) => f.flat)));
    expect(JS_API_LABELS.filter((l) => !flats.has(l) && !packageFlats.has(l))).toEqual([]);
  }, 60_000);

  it('offer callbacks the engine calls', () => {
    const src = runtimeSource();
    const unknown = SCRIPT_CALLBACKS.filter((cb) => !src.includes(cb.evidence ?? `"${cb.name}"`)).map((cb) => cb.name);
    expect(unknown).toEqual([]);
  }, 60_000);

  it('offer every callback core queues for a script', () => {
    const src = runtimeSource();
    const queued = new Set<string>();
    for (const m of src.matchAll(/callbacks::push\(\s*[^,]+,\s*"(on_[a-z_]+)"\s*\)/g)) queued.add(m[1]);
    // `push(e, END_HANDLER)` and friends: the constant's value.
    for (const m of src.matchAll(/const\s+[A-Z_]*HANDLER\s*:\s*&str\s*=\s*"(on_[a-z_]+)"/g)) queued.add(m[1]);
    expect(queued.size).toBeGreaterThan(10);
    const listed = new Set(SCRIPT_CALLBACKS.map((cb) => cb.name));
    expect([...queued].filter((n) => !listed.has(n)).sort()).toEqual([]);
  }, 60_000);
});
