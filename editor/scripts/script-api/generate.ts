/**
 * The script editor's completions, generated from the engine's own lists.
 *
 * The editor used to carry a hand-written table of lowercase namespaces
 * (`entity.`, `sprite.`) and a few names no runtime installs (`set_parallax`),
 * so autocomplete taught an API that did not exist. This builds the data from
 * the three places that decide what a script can call:
 *
 *   - `crates/jengine-core/src/scripting/namespaces.rs` - `CORE` (namespace,
 *     short name, flat global) and `GLOBALS`, the one list every runtime builds
 *     its namespace tables from;
 *   - the generated API Index in `docs/scripting-api.md` - each function's
 *     arguments, what it returns, whether it is deferred and where it is
 *     installed (rebuilt and guarded by `cargo run -p jengine-core --example
 *     script_api_index -- --check`);
 *   - the website's readers of the engine source: `core-metadata.ts` for each
 *     function's one-line description (the `reg!` rows in
 *     `package/core_script_api.rs`), and `extract-packages.ts` for every
 *     package's commands under the namespace its crate name gives it.
 *
 * Write it with
 *
 *     JENGINE_UPDATE_SCRIPT_API=1 npx vitest run scripts/script-api
 *
 * (from `editor/`); without the variable the same test fails when the
 * checked-in `src/lib/generated/script-api.json` is stale.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export const REPO_ROOT = resolve(__dirname, '../../..');
export const OUTPUT = resolve(__dirname, '../../src/lib/generated/script-api.json');

export interface ApiFunction {
  /** The short name inside its namespace (`set_color`), or a global's name. */
  name: string;
  /** The flat global the same function is installed under (`set_sprite_color`). */
  flat: string;
  /** `(entity_id, r, g, b, a)`. */
  args: string;
  /** `float`, `entity_id | nil`, or `""` for nothing. */
  returns: string;
  /** One line: what it does. */
  info: string;
}

export interface ApiNamespace {
  name: string;
  /** `core`, or the package whose crate name gives the namespace. */
  source: string;
  functions: ApiFunction[];
}

export interface ScriptApiData {
  _generated: string;
  namespaces: ApiNamespace[];
  globals: ApiFunction[];
}

export function readRepo(path: string): string {
  return readFileSync(resolve(REPO_ROOT, path), 'utf8').replace(/\r\n/g, '\n');
}

/** `CORE` and `GLOBALS` from `namespaces.rs`. */
export function readNamespaceTable(): { core: Array<[string, string, string]>; globals: string[] } {
  const src = readRepo('crates/jengine-core/src/scripting/namespaces.rs');
  const coreBlock = src.slice(src.indexOf('pub const CORE'), src.indexOf('\n];', src.indexOf('pub const CORE')));
  const core = [...coreBlock.matchAll(/\(\s*"([A-Za-z0-9]+)"\s*,\s*"([a-z_0-9]+)"\s*,\s*"([a-z_0-9]+)"\s*\)/g)].map(
    (m) => [m[1], m[2], m[3]] as [string, string, string],
  );
  const globalsLine = src.match(/pub const GLOBALS: &\[&str\] = &\[([^\]]*)\]/);
  const globals = globalsLine ? [...globalsLine[1].matchAll(/"([a-z_0-9]+)"/g)].map((m) => m[1]) : [];
  if (core.length === 0) throw new Error('namespaces.rs: no CORE rows found');
  return { core, globals };
}

export interface IndexRow {
  namespace: string;
  /** `Sprite.set_color` or `log`. */
  call: string;
  args: string;
  flat: string;
  returns: string;
  sideEffect: string;
  runtimes: string;
}

/** The rows of the generated block in `docs/scripting-api.md`. */
export function readApiIndex(): IndexRow[] {
  const doc = readRepo('docs/scripting-api.md');
  const begin = doc.indexOf('<!-- BEGIN GENERATED API INDEX -->');
  const end = doc.indexOf('<!-- END GENERATED API INDEX -->');
  if (begin < 0 || end < begin) throw new Error('docs/scripting-api.md: no generated API index');
  const rows: IndexRow[] = [];
  for (const line of doc.slice(begin, end).split('\n')) {
    if (!line.startsWith('| ') || line.startsWith('| Namespace ')) continue;
    // Split on `|` that is not escaped: a returns cell reads `entity_id \| nil`.
    const cells = line
      .split(/(?<!\\)\|/)
      .slice(1, -1)
      .map((c) => c.trim().replace(/\\\|/g, '|'));
    if (cells.length !== 6) continue;
    const tick = (s: string) => s.replace(/^`|`$/g, '');
    const fn = tick(cells[1]);
    const open = fn.indexOf('(');
    if (open <= 0) continue;
    const returns = tick(cells[3]);
    rows.push({
      namespace: cells[0],
      call: fn.slice(0, open),
      args: fn.slice(open),
      flat: tick(cells[2]),
      returns: returns === '—' ? '' : returns,
      sideEffect: cells[4] === '—' ? '' : cells[4],
      runtimes: cells[5],
    });
  }
  return rows;
}

/** The website's source readers, loaded by path so the editor's type check
 *  does not follow them (they are Node scripts with their own tsconfig). */
async function websiteReaders() {
  const load = (p: string) => import(/* @vite-ignore */ pathToFileURL(resolve(REPO_ROOT, p)).href);
  const meta = await load('website/scripts/core-metadata.ts');
  const packages = await load('website/scripts/extract-packages.ts');
  return {
    coreFunctionMeta: meta.coreFunctionMeta as () => Map<string, { description: string }>,
    readPackages: packages.readPackages as () => Array<{
      name: string;
      description: string;
      namespace: string | null;
      commands: Array<{ flat: string; args: string[]; call: string }>;
    }>,
  };
}

function sentence(s: string): string {
  const t = s.trim();
  if (!t) return t;
  return /[.!?)]$/.test(t) ? t : `${t}.`;
}

function info(description: string, sideEffect: string): string {
  const parts = [sentence(description)];
  // `deferred`, `immediate`, or a note after a dash: `— the sprite's tint`.
  const effect = sideEffect.replace(/`/g, '').replace(/^—\s*/, '').trim();
  if (effect) parts.push(`(${effect})`);
  return parts.filter(Boolean).join(' ');
}

const byName = <T extends { name: string }>(a: T, b: T) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0);

export async function buildScriptApi(): Promise<ScriptApiData> {
  const { core, globals } = readNamespaceTable();
  const index = readApiIndex();
  const { coreFunctionMeta, readPackages } = await websiteReaders();
  const meta = coreFunctionMeta();

  const indexByFlat = new Map(index.filter((r) => r.call.includes('.')).map((r) => [r.flat, r]));
  const namespaces = new Map<string, ApiNamespace>();
  for (const [ns, short, flat] of core) {
    const row = indexByFlat.get(flat);
    // Only what the index says is installed in every runtime; a row the
    // index lacks is a stale doc, which `script_api_index --check` fails.
    if (!row || row.runtimes !== 'all' || row.call !== `${ns}.${short}`) continue;
    const entry = namespaces.get(ns) ?? { name: ns, source: 'core', functions: [] };
    entry.functions.push({
      name: short,
      flat,
      args: row.args,
      returns: row.returns,
      info: info(meta.get(flat)?.description ?? '', row.sideEffect),
    });
    namespaces.set(ns, entry);
  }

  for (const pkg of readPackages()) {
    if (!pkg.namespace || pkg.commands.length === 0 || namespaces.has(pkg.namespace)) continue;
    const functions = pkg.commands
      .filter((c) => c.call.startsWith(`${pkg.namespace}.`))
      .map((c) => ({
        name: c.call.slice(pkg.namespace!.length + 1),
        flat: c.flat,
        args: `(${c.args.join(', ')})`,
        returns: '',
        info: `A command of the ${pkg.name} package (deferred; the project must list the package).`,
      }));
    namespaces.set(pkg.namespace, { name: pkg.namespace, source: pkg.name, functions });
  }

  const globalFns: ApiFunction[] = [];
  for (const name of globals) {
    const row = index.find((r) => r.namespace === 'Global' && r.call === name);
    if (!row || row.runtimes !== 'all') continue;
    globalFns.push({
      name,
      flat: name,
      args: row.args,
      returns: row.returns,
      info: info(meta.get(name)?.description ?? GLOBAL_INFO[name] ?? '', row.sideEffect),
    });
  }

  return {
    _generated:
      'Generated by editor/scripts/script-api/generate.ts from scripting/namespaces.rs, the API Index in ' +
      'docs/scripting-api.md and the package sources. Do not edit: run ' +
      '`JENGINE_UPDATE_SCRIPT_API=1 npx vitest run scripts/script-api` in editor/.',
    namespaces: [...namespaces.values()]
      .map((n) => ({ ...n, functions: [...n.functions].sort(byName) }))
      .sort(byName),
    globals: globalFns.sort(byName),
  };
}

/** `print` and `require` have no `reg!` row; say what they are. */
const GLOBAL_INFO: Record<string, string> = {
  print: 'Write to the log, as log does',
  require: 'Load a Lua module by path under assets/scripts; cached for the whole session, not per scene',
};

export function serialise(data: ScriptApiData): string {
  return `${JSON.stringify(data, null, 2)}\n`;
}
