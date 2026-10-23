/**
 * extract-features.ts
 *
 * Scans crates/jengine-core/src/ subdirectories and reads module structure
 * to generate a features overview page. The one-line descriptions below are
 * hand-kept; the public exports are read from each module's mod.rs.
 *
 * Deterministic: the same source tree always gives the same page (no dates,
 * directory entries sorted), so CI can check the committed page is current.
 */

import { readFileSync, writeFileSync, readdirSync, existsSync } from 'fs';
import { resolve, join } from 'path';

const REPO_ROOT = process.env.JENGINE_ROOT || resolve(import.meta.dirname, '../..');
const CORE_SRC = resolve(REPO_ROOT, 'crates/jengine-core/src');
const OUTPUT = resolve(import.meta.dirname, '../src/content/docs/features/overview.mdx');

interface ModuleInfo {
  name: string;
  description: string;
  exports: string[];
  status: 'stable' | 'beta' | 'planned';
}

const MODULE_DESCRIPTIONS: Record<string, { desc: string; status: 'stable' | 'beta' | 'planned' }> = {
  renderer: { desc: 'wgpu renderer (WebGPU with a WebGL2 fallback on the web; DX12, Vulkan or Metal natively; WGSL shaders): sprites, meshes, text, tilemaps and vector paths sorted in one pass, 2D lighting and shadows, post effects, shader-graph materials and cameras', status: 'stable' },
  ecs: { desc: 'Entity-Component-System on hecs: the built-in components (see the Components Reference) and the systems that run them', status: 'stable' },
  scripting: { desc: 'Luau natively, Lua 5.3 through Fengari in the editor and on the web, and JavaScript on the desktop, over a deferred command bridge', status: 'stable' },
  scene: { desc: 'JSON scene format (v1 and v2) with save/load, a scene stack, prefabs, prefab variants and streamed scene chunks', status: 'stable' },
  input: { desc: 'Keyboard, mouse, gamepads and touch; input actions with rebinding, hold / tap interactions and gestures', status: 'stable' },
  asset: { desc: 'Asset loading for textures, atlases, fonts, audio, scripts and GUI documents, plus downloadable asset packs', status: 'stable' },
  physics: { desc: '2D physics via rapier2d: rigid bodies, colliders, joints, character controllers, raycasts and shape queries, collision contacts', status: 'stable' },
  audio: { desc: 'Audio via kira natively and Web Audio in the browser: sound effects, music with crossfades and layers, mixer buses with effects, spatial sound', status: 'stable' },
  animation: { desc: 'Sprite sheet animations, skeletal animation with blending and IK, and property tweens', status: 'stable' },
  tilemap: { desc: 'Tilemaps: Tiled JSON import; square, isometric and hexagonal grids; autotiling, animated tiles and tile colliders', status: 'stable' },
  particles: { desc: 'CPU particle emitters with presets, collisions and sub-emitters, plus GPU particles', status: 'stable' },
  ui: { desc: 'Entity UI for HUDs and menus, and JSON GUI documents (.gui) with constraint layout, controller navigation and screen-reader support', status: 'stable' },
  behavior: { desc: 'Behaviour graphs: node graphs that drive entities without a script', status: 'stable' },
  data: { desc: 'Data tables (.data) and their schemas (.dataschema), read by scripts and packages', status: 'stable' },
  save: { desc: 'Save game slots: schema-versioned key/value storage', status: 'stable' },
  profiling: { desc: 'Frame profiler with timing spans, draw-call counters, captures and the F3 debug overlay', status: 'stable' },
  video: { desc: 'Video playback into a texture (Windows natively, and in the browser)', status: 'beta' },
  liveops: { desc: 'Live ops in the game: signed remote config, experiments, live events and content releases from a jengine-server live ops service', status: 'beta' },
};

function getModuleExports(modulePath: string): string[] {
  const exports: string[] = [];
  const modFile = join(modulePath, 'mod.rs');

  if (!existsSync(modFile)) return exports;

  const content = readFileSync(modFile, 'utf-8');
  const pubLines = content.match(/pub (mod|use|struct|fn|enum|trait|type)\s+(\w+)/g) || [];

  for (const line of pubLines) {
    const match = line.match(/pub (?:mod|use|struct|fn|enum|trait|type)\s+(\w+)/);
    if (match) exports.push(match[1]);
  }

  return exports;
}

export function extractFeatures(): void {
  console.log('[sync] Extracting features from', CORE_SRC);

  const modules: ModuleInfo[] = [];

  // Sorted: readdir order is the file system's, and differs between machines.
  const entries = readdirSync(CORE_SRC, { withFileTypes: true })
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    const name = entry.name;
    const info = MODULE_DESCRIPTIONS[name];
    if (!info) continue;

    const exports = getModuleExports(join(CORE_SRC, name));

    modules.push({
      name,
      description: info.desc,
      exports,
      status: info.status,
    });
  }

  // Stable first, then beta, then planned; the table's own order within each.
  const order = { stable: 0, beta: 1, planned: 2 };
  const listed = Object.keys(MODULE_DESCRIPTIONS);
  modules.sort((a, b) => order[a.status] - order[b.status] || listed.indexOf(a.name) - listed.indexOf(b.name));

  const statusEmoji = { stable: '✅', beta: '⚡', planned: '🔮' };

  let mdx = `---
title: Features Overview
draft: false
description: Complete feature list of jEngine 2D game engine.
sidebar:
  badge:
    text: Auto-synced
    variant: tip
---

{/* Auto-generated from crates/jengine-core/src by website/scripts/extract-features.ts */}
{/* Do not edit manually — changes will be overwritten on next build */}

<p class="auto-generated">
  Auto-generated from the engine's module structure (<code>crates/jengine-core/src</code>)
</p>

## Engine Systems

| Module | Status | Description |
|--------|--------|-------------|
`;

  for (const mod of modules) {
    mdx += `| **${mod.name}** | ${statusEmoji[mod.status]} ${mod.status} | ${mod.description} |\n`;
  }

  mdx += `\n**Legend:** ✅ Stable — ⚡ Beta — 🔮 Planned. jEngine is pre-1.0, so even a stable module's API may change in a 0.x release; see [Versioning & Stability](/getting-started/versioning/).\n\n`;
  mdx += `---\n\n## Module Details\n\n`;

  for (const mod of modules) {
    mdx += `### ${mod.name}\n\n`;
    mdx += `${mod.description}\n\n`;

    if (mod.exports.length > 0) {
      mdx += `**Public exports:** ${mod.exports.map(e => `\`${e}\``).join(', ')}\n\n`;
    }
  }

  mdx += `\n---\n\n## Build Targets\n\n`;
  mdx += `\`jengine\` is the engine's CLI (\`cargo run -p jengine-builder --bin jengine --\`); see [Installation](/getting-started/installation/).\n\n`;
  mdx += `| Target | How | Status |\n`;
  mdx += `|--------|-----|--------|\n`;
  mdx += `| Windows | \`jengine run <project>\`, or the editor's Build panel | Native runtime and editor |\n`;
  mdx += `| Web | \`jengine web-build <project>\` | WebGPU, with a WebGL2 fallback |\n`;
  mdx += `| Android | \`jengine android-build <project>\` | APK / app bundle; Lua scripts only |\n`;
  mdx += `| Linux | \`jengine linux-build <project>\` | Builds and runs; tested under WSL only |\n`;
  mdx += `| macOS | \`jengine macos-build <project>\` | Type-checked only, never linked or run: unverified |\n`;
  mdx += `| iOS | \`jengine ios-build <project>\` | Experimental: has never run |\n`;
  mdx += `\nSee [Cross-Platform](/architecture/cross-platform/) for what each platform can do.\n`;

  writeFileSync(OUTPUT, mdx, 'utf-8');
  console.log('[sync] Written features overview →', OUTPUT);
  console.log(`[sync]   ${modules.length} modules extracted`);
}

if (import.meta.url === `file:///${process.argv[1]?.replace(/\\/g, '/')}` ||
    process.argv[1]?.endsWith('extract-features.ts')) {
  extractFeatures();
}
