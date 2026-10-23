// The browser preamble's spawn-then-write path, run for real.
//
// The preamble is JavaScript kept as a template in
// crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs (braces
// doubled for Rust's format!). This pulls the functions it needs out of that
// file and runs them against a stub `window.__jengine_play`, so what is tested
// is the shipped source, not a copy.
//
// The bug: `spawn_prefab` returns a handle for an entity the host creates
// later that frame, and `set_position` wrote straight into the positions
// typed array whenever the handle fit inside it. The arrays grow ahead of use,
// so once enough had been spawned a new handle fitted, the write landed in a
// slot applied before the entity existed, and every spawn appeared at its
// prefab's position - test-lab scene 87 (BUGS #161).
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
  const names = ['__ent', '__dirty', '__defer', '__pending', '__note_spawn', 'set_position', 'spawn_prefab'];
  const body = names.map(extract).join('\n');
  const window = { __jengine_play: play };
  // eslint-disable-next-line no-new-func
  return new Function('window', `${body}\nreturn { set_position, spawn_prefab };`)(window) as {
    set_position: (e: number, x: number, y: number) => void;
    spawn_prefab: (name: string) => number;
  };
}

describe('writing to an entity spawned this frame', () => {
  it('defers the write until the host has created it', () => {
    const positions = new Float32Array(4096); // room for handle 1030 already
    const commands: any[] = [];
    const play: Record<string, any> = {
      positions,
      commands,
      dirty_positions: [],
      next_entity_id: 1030,
      prefabs: { Crate: { components: {} } },
    };
    const api = preamble(play);

    const e = api.spawn_prefab('Crate');
    api.set_position(e, -260, 40);

    expect(e).toBe(1030);
    expect(positions[e * 2]).toBe(0);
    expect(play.dirty_positions).toEqual([]);
    const deferred = commands.find((c) => c.t === 'dtw' && c.k === 'p');
    expect(deferred).toMatchObject({ e: 1030, a: -260, b: 40 });
    // After the spawn, in the queue the host drains in order.
    expect(commands.indexOf(deferred)).toBeGreaterThan(commands.findIndex((c) => c.t === 'spf'));
  });

  it('writes directly once the host has published the entity', () => {
    const positions = new Float32Array(64);
    const commands: any[] = [];
    const play: Record<string, any> = {
      positions,
      commands,
      dirty_positions: [],
      next_entity_id: 1030,
      prefabs: {},
    };
    const api = preamble(play);
    const e = api.spawn_prefab('Crate');
    play.alias = { [e]: 7 }; // the host created it as entity 7

    api.set_position(e, 12, 34);

    expect([positions[14], positions[15]]).toEqual([12, 34]);
    expect(play.dirty_positions).toEqual([7]);
    expect(commands.some((c) => c.t === 'dtw')).toBe(false);
  });
});
