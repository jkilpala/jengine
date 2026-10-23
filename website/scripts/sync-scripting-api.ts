/**
 * sync-scripting-api.ts
 * 
 * Reads docs/scripting-api.md from the repo root and converts it to MDX
 * with Starlight frontmatter. Outputs to src/content/docs/api-reference/scripting-api.mdx
 */

import { readFileSync, writeFileSync } from 'fs';
import { resolve } from 'path';
import { unlinkRepoPaths } from './repo-links';

const REPO_ROOT = process.env.JENGINE_ROOT || resolve(import.meta.dirname, '../..');
const INPUT = resolve(REPO_ROOT, 'docs/scripting-api.md');
const OUTPUT = resolve(import.meta.dirname, '../src/content/docs/api-reference/scripting-api.mdx');

/**
 * Escape <, {, } in Markdown content outside of fenced code blocks.
 * Code blocks (``` ... ```) are left untouched.
 */
function escapeMdxContent(raw: string): string {
  const lines = raw.split('\n');
  const result: string[] = [];
  let inCodeBlock = false;

  for (const line of lines) {
    if (line.trim().startsWith('```')) {
      inCodeBlock = !inCodeBlock;
      result.push(line);
      continue;
    }

    if (inCodeBlock) {
      result.push(line);
      continue;
    }

    // Escape < that isn't part of HTML-style tags we want to keep
    // and { } that aren't MDX expressions we intentionally placed
    let escaped = unlinkRepoPaths(line);

    // Replace < that's followed by a digit or = or space (not an HTML tag)
    // e.g. "<1" or "< 1" → "&lt;1"
    escaped = escaped.replace(/<(?=[\d =])/g, '&lt;');

    // Replace > that's preceded by a digit (e.g. ">1") — but not in | table pipes
    escaped = escaped.replace(/(?<=\d)>(?=[\d ])/g, '&gt;');

    // Escape { } in inline code is fine (backticks protect them)
    // But loose { } in table cells or prose need escaping
    // Only escape { } that are NOT inside backtick spans
    escaped = escapeLooseBraces(escaped);

    result.push(escaped);
  }

  return result.join('\n');
}

/**
 * Escape { and } that appear outside of backtick spans.
 */
function escapeLooseBraces(line: string): string {
  const parts: string[] = [];
  let inBacktick = false;
  let i = 0;

  while (i < line.length) {
    if (line[i] === '`') {
      inBacktick = !inBacktick;
      parts.push('`');
      i++;
    } else if (!inBacktick && line[i] === '{') {
      parts.push('\\{');
      i++;
    } else if (!inBacktick && line[i] === '}') {
      parts.push('\\}');
      i++;
    } else {
      parts.push(line[i]);
      i++;
    }
  }

  return parts.join('');
}

export function syncScriptingApi(): void {
  console.log('[sync] Reading scripting API from', INPUT);

  const raw = readFileSync(INPUT, 'utf-8');

  // Strip the title line (first # heading) — we'll use Starlight's frontmatter title
  const lines = raw.split('\n');
  let startIdx = 0;
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].startsWith('# ')) {
      startIdx = i + 1;
      break;
    }
  }

  // Skip the "Auto-maintained" blockquote immediately after the title
  while (startIdx < lines.length && (lines[startIdx].startsWith('>') || lines[startIdx].trim() === '')) {
    startIdx++;
  }

  const content = lines.slice(startIdx).join('\n');

  // Escape JSX-incompatible characters in MDX outside of code blocks
  // MDX treats < { } as JSX — must escape in prose/tables
  // Strip HTML comments before escaping. MDX has no `<!-- -->` syntax and
  // fails to parse it; the markdown source uses them to delimit the
  // generated API index block, which is meaningless on the site anyway.
  const withoutHtmlComments = content.replace(/^[ 	]*<!--[\s\S]*?-->[ 	]*$/gm, '');
  let mdxContent = escapeMdxContent(withoutHtmlComments);

  // Convert :::note / :::tip / :::caution / :::danger to Starlight format (they're already compatible)
  // No conversion needed — Starlight supports this syntax natively

  const frontmatter = `---
title: Scripting API Reference
draft: false
description: Complete API reference for jEngine scripting — Luau (native), Lua via Fengari (editor + web), and JavaScript.
sidebar:
  badge:
    text: Auto-synced
    variant: tip
---

{/* Auto-generated from docs/scripting-api.md by website/scripts/sync-scripting-api.ts */}
{/* Do not edit manually — changes will be overwritten on next build */}

<p class="auto-generated">
  Auto-generated from <code>docs/scripting-api.md</code>
</p>

`;

  const output = frontmatter + mdxContent;

  writeFileSync(OUTPUT, output, 'utf-8');
  console.log('[sync] Written scripting API →', OUTPUT);
}

// Run if called directly
if (import.meta.url === `file:///${process.argv[1]?.replace(/\\/g, '/')}` ||
    process.argv[1]?.endsWith('sync-scripting-api.ts')) {
  syncScriptingApi();
}
