// The browser preamble's `find_child` and the parts `spawn_prefab` sends,
// run from the shipped source (see preambleSpawn.test.ts for the harness).
// A prefab can be a hierarchy since Oct 2026: `spawn_prefab` hands the host
// its parts as `kids`, and `find_child` reads the `kids` map each host
// publishes (`publish::publish_kids`) - the same rule as native
// `ecs::hierarchy::find_child`, which `prefabs_have_parts` tests.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = readFileSync(
  resolve(__dirname, '../../../crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs'),
  'utf8',
).replace(/\r\n/g, '\n');

function extract(name: string): string {
  const start = SRC.indexOf(`\n  function ${name}(`);
  if (start < 0) throw new Error(`preamble has no function ${name}`);
  const end = SRC.indexOf('\n  }}', start);
  return SRC.slice(start, end + 5).replace(/\{\{/g, '{').replace(/\}\}/g, '}');
}

function preamble(play: Record<string, unknown>) {
  const names = ['__ent', '__dirty', '__defer', '__pending', '__note_spawn', 'spawn_prefab', 'find_child'];
  const body = names.map(extract).join('\n');
  const window = { __jengine_play: play };
  // eslint-disable-next-line no-new-func
  return new Function('window', `${body}\nreturn { spawn_prefab, find_child };`)(window) as {
    spawn_prefab: (name: string) => number;
    find_child: (parent: number, path: string) => number | null;
  };
}

describe('find_child', () => {
  // A cart (7) with two wheels (8, 9) and a mast (10) carrying a flag (11);
  // a second cart (20) with a wheel (21).
  const kids = { 7: [8, 'Wheel', 9, 'Wheel', 10, 'Mast'], 10: [11, 'Flag'], 20: [21, 'Wheel'] };

  it('finds a child, a grandchild breadth first, and a path', () => {
    const api = preamble({ kids, commands: [] });
    expect(api.find_child(7, 'Wheel')).toBe(8);
    expect(api.find_child(7, 'Flag')).toBe(11);
    expect(api.find_child(7, 'Mast/Flag')).toBe(11);
    expect(api.find_child(20, 'Wheel')).toBe(21);
  });

  it('answers null for a part it has not', () => {
    const api = preamble({ kids, commands: [] });
    expect(api.find_child(7, 'Sail')).toBeNull();
    expect(api.find_child(20, 'Flag')).toBeNull();
    expect(api.find_child(99, 'Wheel')).toBeNull();
    expect(preamble({ commands: [] }).find_child(7, 'Wheel')).toBeNull();
  });
});

describe('spawn_prefab', () => {
  it("sends a prefab's parts with it", () => {
    const commands: any[] = [];
    const parts = [{ name: 'Wheel', components: { transform: { position: [-30, -15], rotation: 0, scale: [1, 1] } } }];
    const api = preamble({
      commands,
      next_entity_id: 1000000,
      prefabs: { Cart: { name: 'Cart', components: { sprite: { texture_id: 'cart.png' } }, children: parts }, Box: { name: 'Box', components: {} } },
    });
    api.spawn_prefab('Cart');
    api.spawn_prefab('Box');
    expect(JSON.parse(commands[0].kids)).toEqual(parts);
    expect(commands[1].kids).toBeNull();
  });
});
