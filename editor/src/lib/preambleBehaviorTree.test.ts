// The browser preamble's `BehaviorTree.set`, run from the shipped source
// (see preambleSpawn.test.ts for the harness): one command, tag `sbtf`,
// handled once for both browser hosts by host_commands.rs
// (ecs::behavior_tree::set_behavior_tree_field).
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
  const end = SRC.indexOf('\n', start + 1);
  return SRC.slice(start, end).replace(/\{\{/g, '{').replace(/\}\}/g, '}');
}

describe('BehaviorTree.set in a browser', () => {
  it('queues one command with the field and its value', () => {
    const commands: unknown[] = [];
    const window = { __jengine_play: { commands } };
    // eslint-disable-next-line no-new-func
    const set = new Function('window', `${extract('set_behavior_tree_field')}\nreturn set_behavior_tree_field;`)(window) as (
      e: number, f: string, v?: unknown) => void;
    set(7, 'root', { type: 'wait', seconds: 1 });
    set(7, 'restart');
    expect(commands).toEqual([
      { t: 'sbtf', e: 7, f: 'root', v: { type: 'wait', seconds: 1 } },
      { t: 'sbtf', e: 7, f: 'restart', v: null },
    ]);
  });
});
