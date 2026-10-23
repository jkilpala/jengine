/**
 * build.ts — orchestrator
 *
 * Runs all sync / extract / generate scripts in sequence.
 * Called by `npm run sync` which runs before `astro build`.
 */

import { readFileSync } from 'fs';
import { join } from 'path';
import { syncScriptingApi } from './sync-scripting-api.js';
import { extractComponents } from './extract-components.js';
import { extractFeatures } from './extract-features.js';
import { extractPackages } from './extract-packages.js';
import { syncChangelog } from './sync-changelog.js';
import { generateAiSpec } from './generate-ai-spec.js';

// Namespaced, as scripts call them (`scripting::namespaces`).
const REQUIRED_API_FUNCTIONS = [
  'Transform.get_position', 'Transform.set_position',
  'Entity.spawn', 'Entity.destroy',
  'Entity.find_by_name', 'Entity.get_name',
  'Input.is_action_pressed', 'Input.is_key_pressed',
  'Audio.play_sound', 'Scene.load',
];

function validateAiSpec(): void {
  const specPath = join(new URL('.', import.meta.url).pathname.replace(/^\/([A-Z]:)/, '$1'), '..', 'public', 'ai', 'api-spec.json');
  const spec = JSON.parse(readFileSync(specPath, 'utf-8'));
  const names: Set<string> = new Set((spec.functions as Array<{ name: string }>).map(f => f.name));
  const missing = REQUIRED_API_FUNCTIONS.filter(fn => !names.has(fn));
  if (missing.length > 0) {
    console.error(`\n❌ api-spec.json is missing required functions: ${missing.join(', ')}`);
    process.exit(1);
  }
  console.log(`  ✓ api-spec.json contains all ${REQUIRED_API_FUNCTIONS.length} required functions`);
}

function validateSceneSchema(): void {
  const schemaPath = join(new URL('.', import.meta.url).pathname.replace(/^\/([A-Z]:)/, '$1'), '..', 'public', 'ai', 'scene-schema.json');
  const schema = JSON.parse(readFileSync(schemaPath, 'utf-8')) as Record<string, unknown>;

  const components = (((schema.properties as any)?.entities?.items?.properties?.components) ?? null) as any;
  if (!components || typeof components !== 'object') {
    console.error('\n❌ scene-schema.json is missing entities.items.properties.components');
    process.exit(1);
  }

  const failures: string[] = [];

  if (components.additionalProperties !== true) {
    failures.push('components.additionalProperties must be true (core-subset schema)');
  }

  const colliderProps = components?.properties?.collider?.properties ?? {};
  if (colliderProps?.shape?.type !== 'object') {
    failures.push('collider.shape.type must be "object" (serde-tagged enum style)');
  }
  if ('size' in colliderProps) {
    failures.push('collider.size must not exist (legacy Rectangle/size format)');
  }
  if ('collision_groups' in colliderProps || 'solver_groups' in colliderProps) {
    failures.push('collider legacy fields collision_groups/solver_groups must not exist');
  }
  if (!('collision_membership' in colliderProps) || !('collision_filter' in colliderProps)) {
    failures.push('collider must expose collision_membership and collision_filter');
  }

  const audioProps = components?.properties?.audio_source?.properties ?? {};
  for (const key of ['clip_id', 'volume', 'pitch', 'looping', 'auto_play']) {
    if (!(key in audioProps)) failures.push(`audio_source missing required field: ${key}`);
  }
  for (const legacyKey of ['audio_path', 'play_on_start', 'loop_audio']) {
    if (legacyKey in audioProps) failures.push(`audio_source legacy field must not exist: ${legacyKey}`);
  }

  const cameraProps = components?.properties?.camera?.properties ?? {};
  for (const key of ['zoom', 'is_active', 'viewport_width', 'viewport_height']) {
    if (!(key in cameraProps)) failures.push(`camera missing required field: ${key}`);
  }
  if ('active' in cameraProps) {
    failures.push('camera legacy field must not exist: active');
  }

  if (failures.length > 0) {
    console.error('\n❌ scene-schema.json contract regression detected:');
    for (const f of failures) {
      console.error(`   - ${f}`);
    }
    process.exit(1);
  }

  console.log('  ✓ scene-schema.json contract checks passed');
}

async function main(): Promise<void> {
  console.log('=== jEngine docs sync ===\n');
  const start = Date.now();

  // Content sync (order doesn't matter, run sequentially for clean logs)
  syncScriptingApi();
  extractComponents();
  extractFeatures();
  extractPackages();
  syncChangelog();

  // AI spec generation
  generateAiSpec();

  // Validate AI spec contract
  validateAiSpec();
  validateSceneSchema();

  const elapsed = ((Date.now() - start) / 1000).toFixed(2);
  console.log(`\n=== Done in ${elapsed}s ===`);
}

main().catch(err => {
  console.error('Sync failed:', err);
  process.exit(1);
});
