/**
 * core-metadata.ts
 *
 * What the engine source says about its own script functions and components,
 * for the AI spec. Two sources:
 *
 *   - `package/core_script_api.rs`: one `reg!(api, "flat_name", "description",
 *     Ret, "arg" => Type, ...)` per core script function - the descriptions the
 *     editor shows as tooltips;
 *   - `package/core_registration.rs`: every `register::<Type>("scene_key", ...)`,
 *     whose types' doc comments and fields live somewhere under `src/`.
 */

import { readFileSync, readdirSync, statSync, existsSync } from 'fs';
import { join, resolve } from 'path';

const REPO_ROOT = process.env.JENGINE_ROOT || resolve(import.meta.dirname, '../..');
const CORE_SRC = resolve(REPO_ROOT, 'crates/jengine-core/src');

export interface FunctionMeta {
  description: string;
  returns: string;
  args: Array<{ name: string; type: string }>;
}

/** Flat name -> what `core_script_api.rs` records about it. */
export function coreFunctionMeta(): Map<string, FunctionMeta> {
  const path = join(CORE_SRC, 'package/core_script_api.rs');
  const out = new Map<string, FunctionMeta>();
  if (!existsSync(path)) return out;
  const src = readFileSync(path, 'utf-8');
  const re = /reg!\(\s*api\s*,\s*"([a-z_0-9]+)"\s*,\s*"((?:[^"\\]|\\.)*)"\s*,\s*(\w+)((?:\s*,\s*"[^"]+"\s*=>\s*\w+)*)\s*,?\s*\)/g;
  for (const m of src.matchAll(re)) {
    const args = [...m[4].matchAll(/"([^"]+)"\s*=>\s*(\w+)/g)].map(a => ({ name: a[1], type: a[2] }));
    out.set(m[1], { description: m[2].replace(/\\"/g, '"'), returns: m[3], args });
  }
  return out;
}

export interface ComponentFieldMeta {
  name: string;
  type: string;
  description: string;
  optional: boolean;
}

export interface ComponentMeta {
  sceneKey: string;
  name: string;
  description: string;
  fields: ComponentFieldMeta[];
}

function rustFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir).sort()) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) out.push(...rustFiles(p));
    else if (p.endsWith('.rs')) out.push(p);
  }
  return out;
}

/** The first paragraph of a `///` block, as one line, rustdoc links unwrapped. */
function firstParagraph(lines: string[]): string {
  const para: string[] = [];
  for (const l of lines) {
    if (!l.trim()) { if (para.length) break; else continue; }
    para.push(l.trim());
  }
  return para.join(' ')
    .replace(/\[(`[^`\]]+`)\](\([^)]*\))?/g, '$1')
    .trim();
}

/** The `///` lines directly above line `i` (attributes in between are skipped). */
function docAbove(lines: string[], i: number): string[] {
  const doc: string[] = [];
  let j = i - 1;
  while (j >= 0 && /^\s*#\[/.test(lines[j])) j--;
  while (j >= 0 && /^\s*\/\/\//.test(lines[j])) {
    doc.unshift(lines[j].replace(/^\s*\/\/\/\s?/, ''));
    j--;
  }
  return doc;
}

/** Every `register::<Type>("key", ...)` in core, with the type's docs and fields. */
export function coreComponentMeta(): ComponentMeta[] {
  const regPath = join(CORE_SRC, 'package/core_registration.rs');
  if (!existsSync(regPath)) return [];
  const reg = readFileSync(regPath, 'utf-8');
  const pairs = [...reg.matchAll(/\.register(?:_defaulted)?::<\s*([\w:]+)\s*>\(\s*"([^"]+)"/g)]
    .map(m => ({ type: m[1].split('::').pop()!, key: m[2] }));

  const sources = rustFiles(CORE_SRC).map(p => readFileSync(p, 'utf-8').replace(/\r\n/g, '\n').split('\n'));
  const out: ComponentMeta[] = [];
  const seen = new Set<string>();
  for (const { type, key } of pairs) {
    if (seen.has(key)) continue;
    seen.add(key);
    let description = '';
    const fields: ComponentFieldMeta[] = [];
    for (const lines of sources) {
      const i = lines.findIndex(l => new RegExp(`^pub struct ${type}\\b`).test(l));
      if (i < 0) continue;
      description = firstParagraph(docAbove(lines, i));
      if (/\{\s*$/.test(lines[i])) {
        for (let k = i + 1; k < lines.length && !/^\}/.test(lines[k]); k++) {
          const f = lines[k].match(/^\s{4}pub (\w+):\s*(.+?),?\s*(\/\/.*)?$/);
          if (!f) continue;
          fields.push({
            name: f[1],
            type: f[2].replace(/,$/, '').trim(),
            description: firstParagraph(docAbove(lines, k)),
            optional: f[2].includes('Option<'),
          });
        }
      }
      break;
    }
    out.push({ sceneKey: key, name: type, description, fields });
  }
  return out;
}
