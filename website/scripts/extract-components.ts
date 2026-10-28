/**
 * extract-components.ts
 *
 * Generates the components reference page from what core *registers*
 * (`package/core_registration.rs`, read through `core-metadata.ts`), so a
 * component is on the page wherever its type is defined - `ecs/components.rs`
 * or the module of the engine feature that carries it. The page used to be
 * parsed out of `components.rs` alone, which left out every component defined
 * in its own module and showed no scene key for any of them (it looked the
 * keys up in a typed `ComponentBundle` that no longer exists).
 */

import { readFileSync, writeFileSync } from 'fs';
import { resolve } from 'path';
import { coreComponentMeta, type ComponentMeta } from './core-metadata.js';

const REPO_ROOT = process.env.JENGINE_ROOT || resolve(import.meta.dirname, '../..');
const COMPONENTS_FILE = resolve(REPO_ROOT, 'crates/jengine-core/src/ecs/components.rs');
const OUTPUT = resolve(import.meta.dirname, '../src/content/docs/api-reference/components.mdx');

interface EnumVariant {
  name: string;
  fields?: string;
}

interface EnumDef {
  name: string;
  doc: string;
  variants: EnumVariant[];
}

/** The `pub enum`s of `components.rs`, with their docs and variants. */
function parseEnums(source: string): EnumDef[] {
  const enums: EnumDef[] = [];
  const lines = source.replace(/\r\n/g, '\n').split('\n');
  for (let i = 0; i < lines.length; i++) {
    const m = lines[i].match(/^pub enum (\w+)/);
    if (!m) continue;
    const docLines: string[] = [];
    let j = i - 1;
    while (j >= 0 && lines[j].trim().startsWith('#[')) j--;
    while (j >= 0 && lines[j].trim().startsWith('///')) {
      docLines.unshift(lines[j].trim().replace(/^\/\/\/\s?/, ''));
      j--;
    }
    const variants: EnumVariant[] = [];
    let k = i + 1;
    while (k < lines.length) {
      const el = lines[k].trim();
      if (el === '}') break;
      const simple = el.match(/^(\w+),?\s*$/);
      const complex = el.match(/^(\w+)\s*\{/);
      if (simple) {
        variants.push({ name: simple[1] });
      } else if (complex) {
        const fields: string[] = [];
        k++;
        while (k < lines.length && !lines[k].trim().startsWith('}')) {
          const vf = lines[k].trim();
          if (vf.startsWith('pub ') || /^\w+:/.test(vf)) fields.push(vf.replace(/,$/, '').replace(/^pub /, '').trim());
          k++;
        }
        variants.push({ name: complex[1], fields: fields.join(', ') });
      }
      k++;
    }
    enums.push({ name: m[1], doc: docLines.join(' ').trim(), variants });
    i = k;
  }
  return enums;
}

/**
 * Doc text made safe for MDX prose: outside code spans, `{`, `}` and `<`
 * would be read as JSX. In a table cell a `|` would end the cell, code span
 * or not.
 */
function mdx(text: string, cell = false): string {
  const out = text
    .split(/(`[^`]*`)/)
    .map((part, i) => (i % 2 ? part : part.replace(/[{}]/g, c => `\\${c}`).replace(/</g, '&lt;')))
    .join('');
  return cell ? out.replace(/\|/g, '\\|').replace(/\n+/g, ' ') : out;
}

function firstSentence(text: string): string {
  const s = text.split(/(?<=\.)\s/)[0] ?? '';
  return s || text;
}

/** Categories in the order the page lists them: core things first. */
const CATEGORY_ORDER = ['Core', 'Rendering', 'Lighting', 'Animation', 'Skeleton', 'Physics', 'Tilemap', 'Navigation', 'Behaviors', 'AI', 'Gameplay', 'Combat', 'UI', 'Audio', 'Effects', 'Scripting', 'Performance'];

function categoryRank(c: string): number {
  const i = CATEGORY_ORDER.indexOf(c);
  return i < 0 ? CATEGORY_ORDER.length : i;
}

/** The heading's slug: Starlight's slugger keeps underscores. */
function anchor(c: ComponentMeta): string {
  return c.sceneKey;
}

export function extractComponents(): void {
  console.log('[sync] Extracting components from the core registry');

  const components = coreComponentMeta();
  const enums = parseEnums(readFileSync(COMPONENTS_FILE, 'utf-8'));
  const categories = [...new Set(components.map(c => c.category || 'Other'))]
    .sort((a, b) => categoryRank(a) - categoryRank(b) || a.localeCompare(b));
  const inCategory = (cat: string) => components
    .filter(c => (c.category || 'Other') === cat)
    .sort((a, b) => a.sceneKey.localeCompare(b.sceneKey));
  const featured = components.filter(c => c.feature).length;

  let out = `---
title: Components Reference
draft: false
description: Every component the engine registers, by the key a scene names it with.
sidebar:
  badge:
    text: Auto-synced
    variant: tip
---

{/* Auto-generated from crates/jengine-core/src/package/core_registration.rs by website/scripts/extract-components.ts */}
{/* Do not edit manually — changes will be overwritten on next build */}

<p class="auto-generated">
  Auto-generated from the components <code>crates/jengine-core/src/package/core_registration.rs</code> registers
</p>

jEngine registers ${components.length} components. A scene, a prefab or a script's spawn names one by its **scene key** - the \`"components"\` object of an entity is keyed by it. Every component is plain data and reads and writes as JSON; a field left out takes its default.

${featured} of them belong to an **engine feature**: an optional subsystem a game's build carries only when its project uses it. The builder finds the features a project uses from its assets - a component key in a scene or prefab is one of the signs - so naming one of these components is enough. A component made only by a script, from a key assembled at run time, is a sign the builder cannot see: list its feature under \`include\` in the project's \`settings.engine_features\`. A component whose feature column is empty is in every build.

Packages register components of their own; those are on each [package's page](/packages/).

## Summary

`;

  for (const cat of categories) {
    out += `**${cat}**\n\n| Scene key | Type | Engine feature | Description |\n|-----------|------|----------------|-------------|\n`;
    for (const c of inCategory(cat)) {
      out += `| [\`${c.sceneKey}\`](#${anchor(c)}) | \`${c.name}\` | ${c.feature ? `\`${c.feature}\`` : ''} | ${mdx(firstSentence(c.description), true)} |\n`;
    }
    out += '\n';
  }

  out += '---\n\n';

  for (const cat of categories) {
    out += `## ${cat}\n\n`;
    for (const c of inCategory(cat)) {
      out += `### ${c.sceneKey}\n\n`;
      const facts = [`**Type:** \`${c.name}\``, `**Editor:** ${mdx(c.label)}`];
      if (c.feature) facts.push(`**Engine feature:** \`${c.feature}\``);
      if (c.file) facts.push(`**Defined in:** \`${c.file}\``);
      out += facts.join(' · ') + '\n\n';
      if (c.doc) out += `${mdx(c.doc)}\n\n`;
      if (c.fields.length > 0) {
        out += '| Field | Type | Description |\n|-------|------|-------------|\n';
        for (const f of c.fields) {
          out += `| \`${f.name}\` | \`${f.type.replace(/\|/g, '\\|')}\` | ${mdx(f.description, true)} |\n`;
        }
        out += '\n';
      }
    }
  }

  if (enums.length > 0) {
    out += '## Enums\n\nThe enums of `ecs/components.rs` that component fields take.\n\n';
    for (const en of enums) {
      out += `### ${en.name}\n\n`;
      if (en.doc) out += `${mdx(en.doc)}\n\n`;
      out += '| Variant | Fields |\n|---------|--------|\n';
      for (const v of en.variants) {
        out += `| \`${v.name}\` | ${v.fields ? `\`${v.fields.replace(/\|/g, '\\|')}\`` : '—'} |\n`;
      }
      out += '\n';
    }
  }

  writeFileSync(OUTPUT, out, 'utf-8');
  console.log('[sync] Written components reference →', OUTPUT);
  console.log(`[sync]   ${components.length} components (${featured} in engine features), ${enums.length} enums`);
}

if (import.meta.url === `file:///${process.argv[1]?.replace(/\\/g, '/')}` ||
    process.argv[1]?.endsWith('extract-components.ts')) {
  extractComponents();
}
