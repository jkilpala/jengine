/**
 * generate-ai-spec.ts
 *
 * Generates machine-readable API specifications for AI agents:
 *   - public/ai/api-spec.json      — complete engine API (functions, components, input keys)
 *   - public/ai/scene-schema.json  — JSON Schema for .scene files
 *   - public/ai/llm-context.md     — single-file reference for LLM ingestion
 */

import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'fs';
import { readPackages } from './extract-packages.js';
import { coreFunctionMeta, coreComponentMeta } from './core-metadata.js';
import { resolve } from 'path';

const REPO_ROOT = process.env.JENGINE_ROOT || resolve(import.meta.dirname, '../..');
const AI_DIR = resolve(import.meta.dirname, '../public/ai');

// Lists the spec used to hand-maintain, read from the source that defines them.
// Each falls back to an empty list rather than a guess, so a moved file shows
// up as an empty array in review instead of a confident wrong answer.

/** Variant names of a Rust `pub enum` in `file`. */
function enumVariants(file: string, name: string): string[] {
  const src = readIfExists(resolve(REPO_ROOT, file));
  const m = src.match(new RegExp(`pub enum ${name}\\s*\\{([\\s\\S]*?)\\n\\}`));
  if (!m) return [];
  return [...m[1].matchAll(/^\s*([A-Z][A-Za-z0-9]*)\s*[,({]/gm)].map(v => v[1]);
}

/** Canonical key names: `KeyCode::name()` in `input/mod.rs`. */
function keyNames(): string[] {
  const src = readIfExists(resolve(REPO_ROOT, 'crates/jengine-core/src/input/mod.rs'));
  const i = src.indexOf('pub fn name(&self) -> &\'static str');
  if (i < 0) return [];
  const body = src.slice(i, src.indexOf('\n    }\n', i));
  return [...body.matchAll(/=> "([A-Za-z0-9]+)"/g)].map(m => m[1]);
}

/** UI anchors the entity-UI parser accepts. */
function uiAnchors(): string[] {
  const src = readIfExists(resolve(REPO_ROOT, 'crates/jengine-core/src/ui/mod.rs'));
  return [...new Set([...src.matchAll(/"((?:top|center|bottom)(?:_(?:left|center|right))?)" =>/g)].map(m => m[1]))];
}

/** The editor's particle preset labels. */
function particlePresets(): string[] {
  const src = readIfExists(resolve(REPO_ROOT, 'editor/src/lib/api/particle-presets.ts'));
  return [...src.matchAll(/label:\s*"([^"]+)"/g)].map(m => m[1]);
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

function readIfExists(path: string): string {
  return existsSync(path) ? readFileSync(path, 'utf-8') : '';
}

interface ApiFn {
  name: string;
  /** The older flat global the same function is still installed under
   *  (`set_sprite_color` for `Sprite.set_color`), when there is one. */
  flatName?: string;
  section: string;
  signature: string;
  description: string;
  parameters: { name: string; type: string; description: string }[];
  returns: string;
  deferred: boolean;
}

interface ComponentField {
  name: string;
  type: string;
  description: string;
  optional: boolean;
}

interface Component {
  name: string;
  description: string;
  fields: ComponentField[];
}

function stripBackticks(value: string): string {
  const trimmed = value.trim();
  if (trimmed.startsWith('`') && trimmed.endsWith('`')) {
    return trimmed.slice(1, -1).trim();
  }
  return trimmed;
}

// ---------------------------------------------------------------------------
// parameter type inference from argument names
// ---------------------------------------------------------------------------

function inferParamType(name: string): string {
  const n = name.trim().replace(/[?…]$/, '');
  // Entity handle
  if (/^e$|^entity/.test(n)) return 'entity_id';
  // Boolean params
  if (/^(visible|active|enabled|looping|loop|is_sensor|lock_x|lock_y|lock_rotation|playing|paused)$/.test(n)) return 'boolean';
  // String params
  if (/^(key|action|name|tag|path|clip_name|clip_id|event_name|message|text|anchor|param|state_name|gui_id|node_id|material_id|prefab_name|property|easing|preset|callback_name|field|value_str|scene_name|layer|sound_id)$/.test(n)) return 'string';
  // Numeric params — coordinates, angles, scalars
  if (/^(x|y|z|w|vx|vy|fx|fy|ix|iy|r|g|b|a|sx|sy|ox|oy|wx|wy|tx|ty|dx|dy|nx|ny|ex|ey|px|py|lx|ly|omega|speed|rate|duration|delay|interval|volume|pitch|intensity|radius|mass|restitution|friction|scale|angle|zoom|value|delta|count|n|index|index_1based|frame|tile_id|tween_id|timer_id|handle_id|listener_id|emitter_entity|float_val|x1|y1|x2|y2|cx|cy|hw|hh|bone_id|col|row)$/.test(n)) return 'number';
  // Generic fallback
  if (/\?$/.test(name)) return 'any';
  return 'any';
}

function inferParametersFromSignature(args: string): { name: string; type: string; description: string }[] {
  if (!args.trim()) return [];
  return args
    .split(',')
    .map(a => a.trim())
    .filter(a => a.length > 0)
    .map(a => {
      const name = a.replace(/[?…].*$/, '').trim();
      return { name, type: inferParamType(a), description: '' };
    });
}

// ---------------------------------------------------------------------------
// parse scripting-api.md → function list
// ---------------------------------------------------------------------------

function parseScriptingApiIndex(raw: string): ApiFn[] {
  const lines = raw.split('\n');
  const fns: ApiFn[] = [];
  let inApiIndex = false;

  for (const line of lines) {
    if (!inApiIndex) {
      if (/^##\s+API Index\s*$/.test(line)) {
        inApiIndex = true;
      }
      continue;
    }

    if (/^##\s+/.test(line)) {
      break;
    }

    if (!line.trim().startsWith('|')) {
      continue;
    }

    // Skip markdown header/separator rows.
    if (/^\|\s*-+\s*\|/.test(line) || /\|\s*(Category|Namespace)\s*\|\s*Function\s*\|/i.test(line)) {
      continue;
    }

    // Not on an escaped `\|` - a returns cell like `entity_id \| nil`.
    const cols = line.split(/(?<!\\)\|/).map(c => c.trim().replace(/\\\|/g, '|'));
    // Now: "", namespace, function, flat name, returns, side effect, runtimes, ""
    // Before Sep 2026: "", category, function, returns, side effect, [runtimes], ""
    if (cols.length < 5) {
      continue;
    }
    const withFlat = cols.length >= 8;

    const category = stripBackticks(cols[1] || '');
    const functionCell = stripBackticks(cols[2] || '');
    const flatCell = withFlat ? stripBackticks(cols[3] || '') : '';
    const returnsCell = stripBackticks(cols[withFlat ? 4 : 3] || '');
    const sideEffectCell = stripBackticks(cols[withFlat ? 5 : 4] || '');

    if (!functionCell || !functionCell.includes('(') || !functionCell.includes(')')) {
      continue;
    }

    const open = functionCell.indexOf('(');
    const close = functionCell.lastIndexOf(')');
    if (open <= 0 || close < open) {
      continue;
    }

    const name = functionCell.slice(0, open).trim();
    const args = functionCell.slice(open + 1, close).trim();
    const deferred = /deferred/i.test(sideEffectCell);

    fns.push({
      name,
      flatName: /^[a-z_][a-z0-9_]*$/.test(flatCell) ? flatCell : undefined,
      section: category,
      signature: `${name}(${args})`,
      description: flatCell.startsWith('→')
        ? `Deprecated alias of ${flatCell.replace(/^→\s*/, '')}; call that instead.`
        : '',
      parameters: inferParametersFromSignature(args),
      returns: returnsCell || 'void',
      deferred,
    });
  }

  return fns;
}

function parseScriptingApiHeadings(raw: string): ApiFn[] {
  if (!raw) return [];

  const fns: ApiFn[] = [];
  let currentSection = '';

  const lines = raw.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // Track ## sections
    const secMatch = line.match(/^## (.+)/);
    if (secMatch) {
      currentSection = secMatch[1].trim();
      continue;
    }

    // Match ### function signatures like:  ### `set_position(entity, x, y)`
    const fnMatch = line.match(/^### `(.+?)\((.+?)?\)`/);
    const fnMatchNoArg = line.match(/^### `(.+?)\(\)`/);
    const fnMatchReturn = line.match(/^### `(.+?)\((.+?)?\)\s*→\s*(.+)`/);

    let name = '';
    let argsStr = '';
    let returnStr = '';

    if (fnMatchReturn) {
      name = fnMatchReturn[1];
      argsStr = fnMatchReturn[2] || '';
      returnStr = fnMatchReturn[3];
    } else if (fnMatchNoArg) {
      name = fnMatchNoArg[1];
    } else if (fnMatch) {
      name = fnMatch[1];
      argsStr = fnMatch[2] || '';
    }

    if (!name) continue;

    // Collect description from next non-empty line
    let desc = '';
    for (let j = i + 1; j < Math.min(i + 6, lines.length); j++) {
      const dl = lines[j].trim();
      if (dl && !dl.startsWith('|') && !dl.startsWith('```') && !dl.startsWith('#')) {
        desc = dl;
        break;
      }
    }

    // Check "Returns:" line nearby
    if (!returnStr) {
      for (let j = i + 1; j < Math.min(i + 12, lines.length); j++) {
        const rl = lines[j];
        const retMatch = rl?.match(/\*\*Returns:\*\*\s*(.+)/);
        if (retMatch) {
          returnStr = retMatch[1].trim();
          break;
        }
      }
    }

    // Parse parameter table (lines starting with |)
    const params: ApiFn['parameters'] = [];
    for (let j = i + 1; j < Math.min(i + 20, lines.length); j++) {
      const pl = lines[j];
      if (!pl) continue;
      if (pl.startsWith('###')) break;
      const pMatch = pl.match(/^\|\s*`(\w+)`\s*\|\s*`([^`]+)`\s*\|\s*(.+?)\s*\|/);
      if (pMatch) {
        params.push({ name: pMatch[1], type: pMatch[2], description: pMatch[3].trim() });
      }
    }

    const deferred = desc.toLowerCase().includes('deferred') || argsStr === '';

    fns.push({
      name,
      section: currentSection,
      signature: `${name}(${argsStr})${returnStr ? ' → ' + returnStr : ''}`,
      description: desc,
      parameters: params,
      returns: returnStr || 'void',
      deferred: /deferred|queued/i.test(desc + (lines[i + 1] || '')),
    });
  }

  return fns;
}

function parseScriptingApi(): ApiFn[] {
  const raw = readIfExists(resolve(REPO_ROOT, 'docs/scripting-api.md'));
  if (!raw) return [];

  // Prefer authoritative API Index table extraction.
  const tableFns = parseScriptingApiIndex(raw);
  if (tableFns.length > 0) {
    return tableFns;
  }

  // Backward-compatible fallback for older docs that used ### function headings.
  return parseScriptingApiHeadings(raw);
}

/** `ScriptValueType` from core_script_api.rs, as the spec spells types. */
function specType(rust: string, fallback: string): string {
  const map: Record<string, string> = {
    Float: 'number', Int: 'integer', Bool: 'boolean', String: 'string', EntityId: 'entity_id',
    Vec2: '[number, number]', Rgba: '[number, number, number, number]', Any: 'any', Nil: 'nil',
  };
  return map[rust] ?? fallback;
}

// ---------------------------------------------------------------------------
// parse components.rs → component list
// ---------------------------------------------------------------------------

function parseComponents(): Component[] {
  const raw = readIfExists(resolve(REPO_ROOT, 'crates/jengine-core/src/ecs/components.rs'))
    .replace(/\r\n/g, '\n');
  if (!raw) return [];

  const components: Component[] = [];
  const structRe = /\/\/\/\s*(.+)\n(?:\/\/\/.*\n)*#\[derive\([^\]]*\)\]\npub struct (\w+)\s*\{([^}]+)\}/g;
  let match;

  while ((match = structRe.exec(raw)) !== null) {
    const description = match[1].trim();
    const name = match[2];
    const body = match[3];

    const fields: ComponentField[] = [];
    const fieldRe = /(?:\/\/\/\s*(.+)\n\s*)?(?:#\[.*\]\n\s*)*pub (\w+):\s*(.+),/g;
    let fm;
    while ((fm = fieldRe.exec(body)) !== null) {
      fields.push({
        name: fm[2],
        type: fm[3].trim(),
        description: fm[1]?.trim() || '',
        optional: fm[3].includes('Option<'),
      });
    }

    components.push({ name, description, fields });
  }

  return components;
}

// ---------------------------------------------------------------------------
// generate api-spec.json
// ---------------------------------------------------------------------------

function generateApiSpec(): void {
  console.log('[ai-spec] Generating api-spec.json');

  // Descriptions and argument types from core_script_api.rs, the same
  // metadata the editor's tooltips show. A function it does not describe
  // keeps an empty description rather than a placeholder that reads as one.
  const meta = coreFunctionMeta();
  // Lua's own globals, which the engine keeps rather than registers.
  meta.set('print', { description: 'Write to the log (as log does)', returns: 'Nil', args: [] });
  meta.set('require', {
    description: 'Load a Lua module by path under assets/scripts. Cached for the whole session, not per scene, so reset module state in init',
    returns: 'Any',
    args: [],
  });
  const functions = parseScriptingApi().map(f => {
    const m = meta.get(f.flatName ?? f.name);
    if (!m) return f;
    const parameters = m.args.length === f.parameters.length
      ? f.parameters.map((p, i) => ({ ...p, type: specType(m.args[i].type, p.type) }))
      : f.parameters;
    return { ...f, description: f.description || m.description, parameters };
  });
  const components = coreComponentMeta();
  const undescribed = functions.filter(f => !f.description).map(f => f.name);

  const spec = {
    $schema: 'https://json-schema.org/draft/2020-12/schema',
    engine: 'jEngine',
    version: '0.1.0',
    description: 'jEngine 2D game engine API specification for AI agents.',
    scriptingLanguages: ['lua', 'javascript'],
    lifecycle: {
      functions: ['init(entity)', 'update(entity, dt)'],
      notes: 'All mutation calls are deferred — commands are queued and applied after all scripts finish each frame.',
    },
    functions: functions.map(f => ({
      name: f.name,
      ...(f.flatName ? { flatName: f.flatName } : {}),
      section: f.section,
      signature: f.signature,
      description: f.description,
      parameters: f.parameters,
      returns: f.returns,
      deferred: f.deferred,
    })),
    // Every component core registers, by the key a scene uses for it.
    components: components.map(c => ({
      sceneKey: c.sceneKey,
      name: c.name,
      description: c.description,
      fields: c.fields,
    })),
    inputKeys: keyNames(),
    gamepadBindings: 'Pad<n>_<button> (Pad0_A, Pad0_B, Pad0_X, Pad0_Y, Pad0_Start, Pad0_LeftBumper, Pad0_DPadUp, ...) or a stick half (Pad0_LeftStickY+); stick up is positive',
    colliderShapes: enumVariants('crates/jengine-core/src/ecs/components.rs', 'ColliderShape'),
    bodyTypes: enumVariants('crates/jengine-core/src/ecs/components.rs', 'BodyType'),
    easingFunctions: enumVariants('crates/jengine-core/src/animation/easing.rs', 'EasingFunction'),
    particlePresets: particlePresets(),
    anchors: uiAnchors(),
    // Rust packages a project can link (top-level "packages" in project.jengine).
    // Built-ins ship with every runtime; the rest must be listed, with every
    // package they depend on, and need a v2 scene for their components.
    packages: readPackages().map(p => ({
      name: p.name,
      description: p.description,
      builtIn: p.builtIn,
      namespace: p.namespace,
      dependsOn: p.dependsOn,
      components: p.components.map(c => ({ key: c.key, type: c.type, core: c.core })),
      commands: p.commands.map(c => ({ call: `${c.call}(${c.args.join(', ')})`, flatName: c.flat })),
      docs: `/packages/${p.slug}/`,
    })),
  };

  mkdirSync(AI_DIR, { recursive: true });
  writeFileSync(resolve(AI_DIR, 'api-spec.json'), JSON.stringify(spec, null, 2), 'utf-8');
  console.log(`[ai-spec]   ${functions.length} functions, ${components.length} components, ${spec.packages.length} packages`);
  if (undescribed.length) {
    console.log(`[ai-spec]   ${undescribed.length} function(s) with no description in core_script_api.rs: ${undescribed.join(', ')}`);
  }
}

// ---------------------------------------------------------------------------
// generate scene-schema.json (JSON Schema for .scene files)
// ---------------------------------------------------------------------------

function generateSceneSchema(): void {
  console.log('[ai-spec] Generating scene-schema.json');

  const schema = {
    $schema: 'https://json-schema.org/draft/2020-12/schema',
    title: 'jEngine Scene File',
    description: 'JSON Schema for jEngine .scene files.',
    type: 'object',
    required: ['entities'],
    properties: {
      version: { type: 'integer', enum: [1, 2], description: 'Use 2: it dispatches every component through the registry, so package components load. A v1 scene keeps an unknown key as inert custom data.' },
      name: { type: 'string', description: 'Scene display name.' },
      audio_refs: { type: 'array', items: { type: 'string' }, description: 'Every clip the scene may play, as paths under assets/. A clip not listed does not play.' },
      atlas_refs: { type: 'array', items: { type: 'string' }, description: 'Ready-made sprite sheets (.atlas) to load with the scene.' },
      entities: {
        type: 'array',
        items: {
          type: 'object',
          required: ['name', 'components'],
          properties: {
            name: { type: 'string' },
            parent: { type: 'string', description: 'Name of the entity this one hangs from.' },
            parent_index: { type: 'integer', description: 'Index of the parent, written when its name is shared.' },
            components: {
              type: 'object',
              properties: {
                transform: {
                  type: 'object',
                  properties: {
                    position: { type: 'array', items: { type: 'number' }, minItems: 2, maxItems: 2 },
                    rotation: { type: 'number', description: 'Radians' },
                    scale: { type: 'array', items: { type: 'number' }, minItems: 2, maxItems: 2 },
                  },
                },
                sprite: {
                  type: 'object',
                  properties: {
                    texture_id: { type: 'string' },
                    color: { type: 'array', items: { type: 'number' }, minItems: 4, maxItems: 4 },
                    z_order: { type: 'integer' },
                    size: { type: ['array', 'null'], items: { type: 'number' }, minItems: 2, maxItems: 2 },
                    uv_rect: { type: ['array', 'null'], items: { type: 'number' }, minItems: 4, maxItems: 4 },
                  },
                },
                script: {
                  type: 'object',
                  properties: {
                    source_path: { type: 'string' },
                    language: { type: 'string', enum: ['Lua', 'JavaScript'] },
                  },
                },
                rigid_body: {
                  type: 'object',
                  properties: {
                    body_type: { type: 'string', enum: ['Dynamic', 'Static', 'Kinematic'] },
                  },
                },
                collider: {
                  type: 'object',
                  properties: {
                    shape: {
                      type: 'object',
                      description: 'Serde-tagged enum. Common forms: {"Box":{"width":W,"height":H}}, {"Circle":{"radius":R}}, {"Capsule":{"half_height":H,"radius":R}}',
                    },
                    offset: { type: 'array', items: { type: 'number' }, minItems: 2, maxItems: 2 },
                    is_sensor: { type: 'boolean' },
                    restitution: { type: 'number' },
                    friction: { type: 'number' },
                    density: { type: 'number' },
                    collision_membership: { type: 'integer' },
                    collision_filter: { type: 'integer' },
                  },
                },
                audio_source: {
                  type: 'object',
                  properties: {
                    clip_id: { type: 'string' },
                    volume: { type: 'number', minimum: 0.0, maximum: 1.0 },
                    pitch: { type: 'number' },
                    looping: { type: 'boolean' },
                    auto_play: { type: 'boolean' },
                  },
                },
                particle_emitter: {
                  type: 'object',
                  properties: {
                    particle_source: { type: 'string', description: 'A .particle definition to load the emitter from.' },
                    emission_rate: { type: 'number' },
                    max_particles: { type: 'integer' },
                  },
                },
                sprite_animator: {
                  type: 'object',
                  properties: {
                    clips: { type: 'object', description: 'Named clips of frames.' },
                    current_clip: { type: ['string', 'null'] },
                    playing: { type: 'boolean' },
                    speed: { type: 'number' },
                  },
                },
                camera: {
                  type: 'object',
                  properties: {
                    zoom: { type: 'number' },
                    is_active: { type: 'boolean' },
                    viewport_width: { type: 'number' },
                    viewport_height: { type: 'number' },
                  },
                },
                tilemap: {
                  type: 'object',
                  description: 'Cell (0, 0) hangs from the transform (its top-left corner) with rows running down; tile id 0 is empty.',
                  properties: {
                    tile_size: { type: 'array', items: { type: 'number' }, minItems: 2, maxItems: 2 },
                    orientation: { type: 'string' },
                    layers: { type: 'array' },
                    tilesets: { type: 'array' },
                    source: { type: ['string', 'null'] },
                    z_order: { type: 'integer' },
                  },
                },
                gui: {
                  type: 'object',
                  description: 'Names a .gui document; a document no entity names is never loaded.',
                  properties: {
                    gui_id: { type: 'string' },
                    visible: { type: 'boolean' },
                  },
                },
              },
              // Every other engine and package component, and custom
              // components (CapitalCase keys, e.g. "Health": { "current": 100 }).
              // A misspelt engine key is accepted here and silently becomes
              // custom data at load.
              additionalProperties: true,
            },
          },
        },
      },
    },
  };

  writeFileSync(resolve(AI_DIR, 'scene-schema.json'), JSON.stringify(schema, null, 2), 'utf-8');
}

// ---------------------------------------------------------------------------
// generate llm-context.md — single-file reference for LLMs
// ---------------------------------------------------------------------------

function generateLlmContext(): void {
  console.log('[ai-spec] Generating llm-context.md');

  const scriptingApi = readIfExists(resolve(REPO_ROOT, 'docs/scripting-api.md'));

  let md = `# jEngine — Complete AI/LLM Reference

> **Machine-readable context file**, generated from the engine source by website/scripts/generate-ai-spec.ts.
> This file is designed for LLMs and AI coding agents to understand jEngine\'s API.

## Engine Overview

jEngine is a 2D game engine written in Rust. Games are built using:
- **ECS (Entity Component System)** via \`hecs\`
- **Scripting** in Lua (Luau natively, Lua 5.3 via Fengari in the editor and on the web) or JavaScript (desktop only)
- **Scenes** stored as JSON files (\`.scene\`; use \`"version": 2\`)
- **Rendering** via \`wgpu\` with WGSL shaders (DX12 / Vulkan / Metal / WebGPU / WebGL2 / GLES)
- **Packages**: Rust crates adding components and script commands, linked per project

Scripts define \`init(entity)\` and \`update(entity, dt)\` functions.
All mutation calls are **deferred** — they queue commands applied after all scripts finish each frame.

## Key Rules for AI Agents

1. Lua scripts call the engine through capitalised namespaces: \`Transform.get_position(e)\`, \`Input.is_action_pressed(a)\`. \`log\`, \`print\` and \`require\` are global. JavaScript uses the flat names (\`get_position\`).
2. Mutation functions (\`Transform.set_position\`, \`Entity.spawn_prefab\`, ...) are deferred — they don't take effect immediately
3. Entity IDs are numbers — you receive your entity ID as the first parameter
4. Scenes are JSON — see \`scene-schema.json\`. A misspelt component key silently becomes custom data
5. Assets (textures, scripts, audio) are referenced by path relative to \`assets/\`; every sound must be listed in the scene's \`audio_refs\`
6. Coordinates: X-right, Y-up, origin at world center; one world unit is about a centimetre
7. Rotation is in radians (use \`math.pi\` in Lua, \`Math.PI\` in JS)
8. Colors are RGBA arrays with values 0.0–1.0, in **linear** space
9. Keep to the Lua subset both dialects share: probe for \`math.atan2\`; floor before \`string.format("%d", x)\`
10. A package's commands live under its namespace (\`Online.join\`); check \`if Online == nil\` before use

## Quick Example — Lua

\`\`\`lua
function init(entity)
    Transform.set_position(entity, 0, 0)
end

function update(entity, dt)
    local x, y = Transform.get_position(entity)
    if Input.is_key_pressed("D") then
        x = x + 200 * dt
    end
    if Input.is_key_pressed("A") then
        x = x - 200 * dt
    end
    Transform.set_position(entity, x, y)
end
\`\`\`

## Quick Example — JavaScript

\`\`\`javascript
function init(entity) {
    set_position(entity, 0, 0);
}

function update(entity, dt) {
    const [x, y] = get_position(entity);
    let newX = x;
    if (is_key_pressed("D")) newX += 200 * dt;
    if (is_key_pressed("A")) newX -= 200 * dt;
    set_position(entity, newX, y);
}
\`\`\`

---

`;

  // Append full scripting API stripped of the metadata blockquote
  if (scriptingApi) {
    const stripped = scriptingApi
      .replace(/^# .+\n/, '')
      .replace(/^>.*\n(>.*\n)*/m, '')
      .trim();

    md += `## Full Scripting API Reference\n\n${stripped}\n\n`;
  }

  // Packages: what each adds and how scripts call it.
  const packages = readPackages();
  md += `## Packages\n\nA project links a Rust package by listing it (and every package it depends on) in the top-level \`"packages"\` array of \`project.jengine\`. Built-ins ship with every runtime. Package components need a v2 scene.\n\n`;
  for (const p of packages) {
    md += `### ${p.name}${p.builtIn ? ' (built-in)' : ''}\n\n${p.description}\n\n`;
    if (p.dependsOn.length) md += `Depends on: ${p.dependsOn.join(', ')}\n\n`;
    if (p.components.length) md += `Components: ${p.components.map(c => `\`${c.key}\``).join(', ')}\n\n`;
    if (p.commands.length) md += `Commands: ${p.commands.map(c => `\`${c.call}(${c.args.join(', ')})\``).join(', ')}\n\n`;
  }

  md += `---\n\n*End of jEngine AI context — see api-spec.json for structured data.*\n`;

  writeFileSync(resolve(AI_DIR, 'llm-context.md'), md, 'utf-8');
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

export function generateAiSpec(): void {
  mkdirSync(AI_DIR, { recursive: true });
  generateApiSpec();
  generateSceneSchema();
  generateLlmContext();
  console.log('[ai-spec] Done');
}

if (import.meta.url === `file:///${process.argv[1]?.replace(/\\/g, '/')}` ||
    process.argv[1]?.endsWith('generate-ai-spec.ts')) {
  generateAiSpec();
}
