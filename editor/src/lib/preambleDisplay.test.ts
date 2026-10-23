// The browser preamble's `Display.*` / `Cursor.*` functions, run from the
// shipped source (see preambleSpawn.test.ts for the harness). Each asks one
// host hook, `state.display(op, ...)` (adapters/js/display.rs); without a
// host they answer what a page would have rather than throwing.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = readFileSync(
  resolve(__dirname, '../../../crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs'),
  'utf8',
).replace(/\r\n/g, '\n');

const NAMES = [
  '__display', 'display_set_mode', 'display_get_mode', 'display_set_size', 'display_get_size',
  'display_get_resolutions', 'display_set_vsync', 'display_get_vsync', 'display_set_max_fps',
  'display_get_max_fps', 'display_supports', 'cursor_set_visible', 'cursor_set_confined',
  'cursor_set_locked', 'cursor_set_icon', 'cursor_set_image',
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

describe('Display and Cursor in a browser', () => {
  it('hand every call to the host hook', () => {
    const calls: any[][] = [];
    const api = preamble({
      display: (op: string, a: any, b: any, c: any) => {
        calls.push([op, a, b, c].filter((v) => v !== undefined));
        if (op === 'get_mode') return 'borderless';
        if (op === 'get_size') return [1920, 1080];
        if (op === 'supports') return a === 'fullscreen';
        return null;
      },
    });
    api.display_set_mode('borderless');
    api.cursor_set_locked(true);
    api.cursor_set_image('sprites/hand.png', 4, 2);
    api.cursor_set_image();
    expect(calls).toEqual([
      ['set_mode', 'borderless'],
      ['cursor_locked', true],
      ['cursor_image', 'sprites/hand.png', 4, 2],
      ['cursor_image', '', 0, 0],
    ]);
    expect(api.display_get_mode()).toBe('borderless');
    expect(api.display_get_size()).toEqual([1920, 1080]);
    expect(api.display_supports('fullscreen')).toBe(true);
    expect(api.display_supports('vsync')).toBe(false);
  });

  it('answer what a page has without a host', () => {
    const api = preamble(undefined);
    expect(api.display_get_mode()).toBe('windowed');
    expect(api.display_get_size()).toEqual([0, 0]);
    expect(api.display_get_resolutions()).toEqual([]);
    expect(api.display_get_vsync()).toBe(true);
    expect(api.display_get_max_fps()).toBe(0);
    expect(api.display_supports('vsync')).toBe(false);
    expect(() => api.cursor_set_visible(false)).not.toThrow();
  });
});
