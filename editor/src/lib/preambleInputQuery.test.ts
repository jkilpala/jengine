// The browser preamble's hold / tap / double-tap, pad identity and prompt
// functions, run from the shipped source (see preambleSpawn.test.ts for the
// harness). Each asks one host hook, `state.input_query(op, a, b)`
// (adapters/js/input_query.rs), which answers from the engine's InputView;
// without a host they answer "nothing is happening" rather than throwing.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = readFileSync(
  resolve(__dirname, '../../../crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs'),
  'utf8',
).replace(/\r\n/g, '\n');

const NAMES = [
  '__input_query', 'get_action_hold_time', 'is_action_held', 'is_action_tapped', 'is_action_double_tapped',
  'get_gamepad_name', 'get_gamepad_type', 'get_last_input_device', 'get_last_gamepad', 'get_action_prompt',
];

function extract(name: string): string {
  const start = SRC.indexOf(`\n  function ${name}(`);
  if (start < 0) throw new Error(`preamble has no function ${name}`);
  const end = SRC.indexOf('\n', start + 1);
  return SRC.slice(start, end).replace(/\{\{/g, '{').replace(/\}\}/g, '}');
}

function preamble(state: Record<string, unknown> | undefined) {
  const body = NAMES.map(extract).join('\n');
  const window = { __jengine_play: { state } };
  // eslint-disable-next-line no-new-func
  return new Function('window', `${body}\nreturn { ${NAMES.slice(1).join(', ')} };`)(window) as Record<string, (...a: any[]) => any>;
}

describe('holds, taps and prompts in a browser', () => {
  it('hand every question to the host hook', () => {
    const calls: any[][] = [];
    const api = preamble({
      input_query: (op: string, a: any, b: any) => {
        calls.push([op, a, b].filter((v) => v !== undefined));
        if (op === 'held') return b === 1.5;
        if (op === 'hold_time') return 1.5;
        if (op === 'pad_type') return 'switch';
        if (op === 'last_pad') return 0;
        if (op === 'prompt') return { label: 'B', glyph: 'switch/b', binding: 'Pad0_A', style: 'switch' };
        return null;
      },
    });
    expect(api.is_action_held('jump', 1.5)).toBe(true);
    expect(api.is_action_held('jump')).toBe(false);
    expect(api.get_action_hold_time('jump')).toBe(1.5);
    expect(api.get_gamepad_type(0)).toBe('switch');
    expect(api.get_last_gamepad()).toBe(0);
    expect(api.get_action_prompt('jump', 'switch').glyph).toBe('switch/b');
    expect(calls).toEqual([
      ['held', 'jump', 1.5], ['held', 'jump'], ['hold_time', 'jump'], ['pad_type', 0], ['last_pad'],
      ['prompt', 'jump', 'switch'],
    ]);
  });

  it('answer that nothing is happening without a host', () => {
    const api = preamble(undefined);
    expect(api.is_action_tapped('jump')).toBe(false);
    expect(api.is_action_double_tapped('jump')).toBe(false);
    expect(api.get_action_hold_time('jump')).toBe(0);
    expect(api.get_gamepad_name()).toBe('');
    expect(api.get_last_input_device()).toBe('none');
    expect(api.get_last_gamepad()).toBe(-1);
    expect(api.get_action_prompt('jump')).toEqual({ label: '', glyph: '', binding: '', style: 'keyboard' });
  });
});
