/**
 * extract-components.ts
 * 
 * Parses crates/jengine-core/src/ecs/components.rs to extract component struct definitions,
 * their fields, types, and doc comments. Generates an MDX reference page.
 */

import { readFileSync, writeFileSync } from 'fs';
import { resolve } from 'path';

const REPO_ROOT = process.env.JENGINE_ROOT || resolve(import.meta.dirname, '../..');
const COMPONENTS_FILE = resolve(REPO_ROOT, 'crates/jengine-core/src/ecs/components.rs');
const FORMAT_FILE = resolve(REPO_ROOT, 'crates/jengine-core/src/scene/format.rs');
const OUTPUT = resolve(import.meta.dirname, '../src/content/docs/api-reference/components.mdx');

interface Field {
  name: string;
  type: string;
  doc: string;
  default?: string;
}

interface Component {
  name: string;
  doc: string;
  fields: Field[];
}

interface EnumVariant {
  name: string;
  fields?: string;
}

interface EnumDef {
  name: string;
  doc: string;
  variants: EnumVariant[];
}

function parseComponents(source: string): { components: Component[]; enums: EnumDef[] } {
  const components: Component[] = [];
  const enums: EnumDef[] = [];
  const lines = source.split('\n');

  let i = 0;
  while (i < lines.length) {
    const line = lines[i];

    // Collect doc comments
    if (line.trim().startsWith('///')) {
      const docLines: string[] = [];
      while (i < lines.length && lines[i].trim().startsWith('///')) {
        docLines.push(lines[i].trim().replace(/^\/\/\/\s?/, ''));
        i++;
      }
      const doc = docLines.join(' ').trim();

      // Skip attributes
      while (i < lines.length && lines[i].trim().startsWith('#[')) {
        i++;
      }

      const structLine = lines[i]?.trim() || '';

      // Parse struct
      const structMatch = structLine.match(/^pub struct (\w+)/);
      if (structMatch) {
        const name = structMatch[1];
        const fields: Field[] = [];
        i++;

        // Check if it's a newtype struct (single line)
        if (structLine.includes('(pub')) {
          const newtypeMatch = structLine.match(/\(pub (.+)\)/);
          if (newtypeMatch) {
            fields.push({ name: '0', type: newtypeMatch[1].replace(';', '').trim(), doc: 'Inner value' });
          }
          components.push({ name, doc, fields });
          continue;
        }

        // Regular struct with braces
        let fieldDoc = '';
        while (i < lines.length) {
          const fl = lines[i].trim();
          if (fl === '}') { i++; break; }

          if (fl.startsWith('///')) {
            // Accumulate: a multi-line field doc used to be overwritten line
            // by line, so the table showed only its last line - which for a
            // doc ending in a caveat read as a fragment. Joined with spaces
            // because the value lands in a markdown table cell, where a
            // newline would break the row.
            const line = fl.replace(/^\/\/\/\s?/, '');
            fieldDoc = fieldDoc ? `${fieldDoc} ${line}`.trim() : line;
          } else if (fl.startsWith('#[')) {
            // Check for serde default
            const defaultMatch = fl.match(/default\s*=\s*"(\w+)"/);
            if (defaultMatch) {
              // Store default hint
            }
          } else if (fl.startsWith('pub ')) {
            const fieldMatch = fl.match(/^pub (\w+):\s*(.+?),?\s*$/);
            if (fieldMatch) {
              fields.push({
                name: fieldMatch[1],
                type: fieldMatch[2].replace(/,$/, '').trim(),
                doc: fieldDoc,
              });
              fieldDoc = '';
            }
          }
          i++;
        }

        components.push({ name, doc, fields });
        continue;
      }

      // Parse enum
      const enumMatch = structLine.match(/^pub enum (\w+)/);
      if (enumMatch) {
        const name = enumMatch[1];
        const variants: EnumVariant[] = [];
        i++;

        while (i < lines.length) {
          const el = lines[i].trim();
          if (el === '}') { i++; break; }

          // Skip doc comments and attributes inside enum
          if (el.startsWith('///') || el.startsWith('#[') || el === '') {
            i++;
            continue;
          }

          // Simple variant
          const simpleMatch = el.match(/^(\w+),?\s*$/);
          if (simpleMatch) {
            variants.push({ name: simpleMatch[1] });
            i++;
            continue;
          }

          // Variant with fields
          const complexMatch = el.match(/^(\w+)\s*\{/);
          if (complexMatch) {
            const variantFields: string[] = [];
            i++;
            while (i < lines.length && !lines[i].trim().startsWith('}')) {
              const vf = lines[i].trim();
              if (vf.startsWith('pub ') || vf.match(/^\w+:/)) {
                variantFields.push(vf.replace(/,$/, '').replace(/^pub /, '').trim());
              }
              if (vf.startsWith('#[')) { i++; continue; }
              i++;
            }
            variants.push({ name: complexMatch[1], fields: variantFields.join(', ') });
            i++;
            continue;
          }

          i++;
        }

        enums.push({ name, doc, variants });
        continue;
      }
    }

    i++;
  }

  return { components, enums };
}

function parseSceneBundle(source: string): Map<string, string> {
  const mapping = new Map<string, string>();
  const bundleMatch = source.match(/pub struct ComponentBundle\s*\{([\s\S]*?)\n\}/);
  if (!bundleMatch) return mapping;

  const body = bundleMatch[1];
  const fieldRegex = /pub (\w+):\s*Option<(\w+)>/g;
  let m;
  while ((m = fieldRegex.exec(body)) !== null) {
    mapping.set(m[2], m[1]); // Component type → JSON field name
  }
  return mapping;
}

export function extractComponents(): void {
  console.log('[sync] Extracting components from', COMPONENTS_FILE);

  const componentsSource = readFileSync(COMPONENTS_FILE, 'utf-8');
  const formatSource = readFileSync(FORMAT_FILE, 'utf-8');

  const { components, enums } = parseComponents(componentsSource);
  const sceneMapping = parseSceneBundle(formatSource);

  let mdx = `---
title: Components Reference
draft: false
description: Complete reference for all built-in ECS components in jEngine.
sidebar:
  badge:
    text: Auto-synced
    variant: tip
---

{/* Auto-generated from crates/jengine-core/src/ecs/components.rs by website/scripts/extract-components.ts */}
{/* Do not edit manually — changes will be overwritten on next build */}

<p class="auto-generated">
  Auto-generated from <code>crates/jengine-core/src/ecs/components.rs</code>
</p>

jEngine provides ${components.length} built-in component types. All components derive \`Serialize\` and \`Deserialize\` for JSON scene files.

## Summary

| Component | Scene Key | Description |
|-----------|-----------|-------------|
`;

  for (const comp of components) {
    const sceneKey = sceneMapping.get(comp.name) || '—';
    const shortDoc = comp.doc.split('.')[0] + '.';
    mdx += `| \`${comp.name}\` | \`${sceneKey}\` | ${shortDoc} |\n`;
  }

  mdx += '\n---\n\n';

  // Detail sections
  for (const comp of components) {
    mdx += `## ${comp.name}\n\n`;
    mdx += `${comp.doc}\n\n`;

    const sceneKey = sceneMapping.get(comp.name);
    if (sceneKey) {
      mdx += `**Scene JSON key:** \`${sceneKey}\`\n\n`;
    }

    if (comp.fields.length > 0 && comp.fields[0].name !== '0') {
      mdx += '| Field | Type | Description |\n';
      mdx += '|-------|------|-------------|\n';
      for (const field of comp.fields) {
        mdx += `| \`${field.name}\` | \`${field.type}\` | ${field.doc} |\n`;
      }
    } else if (comp.fields[0]?.name === '0') {
      mdx += `Newtype wrapper around \`${comp.fields[0].type}\`.\n`;
    }

    mdx += '\n---\n\n';
  }

  // Enum sections
  if (enums.length > 0) {
    mdx += '## Enums\n\n';
    for (const en of enums) {
      mdx += `### ${en.name}\n\n`;
      mdx += `${en.doc}\n\n`;
      mdx += '| Variant | Fields |\n';
      mdx += '|---------|--------|\n';
      for (const v of en.variants) {
        const escapedFields = v.fields ? `\`${v.fields}\`` : '—';
        mdx += `| \`${v.name}\` | ${escapedFields} |\n`;
      }
      mdx += '\n';
    }
  }

  writeFileSync(OUTPUT, mdx, 'utf-8');
  console.log('[sync] Written components reference →', OUTPUT);
  console.log(`[sync]   ${components.length} components, ${enums.length} enums extracted`);
}

if (import.meta.url === `file:///${process.argv[1]?.replace(/\\/g, '/')}` ||
    process.argv[1]?.endsWith('extract-components.ts')) {
  extractComponents();
}
