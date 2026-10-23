// The browser preamble's `Sprite.paint`, run from the shipped source (see
// preambleSpawn.test.ts for the harness): one command, tag `spnt`, handled
// once for both browser hosts by host_commands.rs (ecs::paint). Leaving
// out the colour, shape or rotation gives white, a splat, 0. `Sprite.press`
// shares the tag.
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
  const end = SRC.indexOf('\n  }}\n', start + 1);
  return SRC.slice(start, end + 5).replace(/\{\{/g, '{').replace(/\}\}/g, '}');
}

describe('Sprite.paint in a browser', () => {
  it('queues one stamp, with defaults for what is left out', () => {
    const commands: unknown[] = [];
    const window = { __jengine_play: { commands } };
    // eslint-disable-next-line no-new-func
    const paint = new Function('window', `${extract('paint_sprite')}\nreturn paint_sprite;`)(window) as (
      ...a: unknown[]) => void;
    paint(3, 10, -4, 30, 0.4, 0, 0, 0.8, 'streak', 1.5, 'erase');
    paint(3, 1, 2, 5);
    expect(commands).toEqual([
      { t: 'spnt', e: 3, x: 10, y: -4, s: 30, cr: 0.4, cg: 0, cb: 0, ca: 0.8, sh: 'streak', rot: 1.5, bl: 'erase' },
      { t: 'spnt', e: 3, x: 1, y: 2, s: 5, cr: 1, cg: 1, cb: 1, ca: 1, sh: '', rot: 0, bl: '' },
    ]);
  });

  // `Sprite.press` is the same command with a depth (`dp`): a dent in the
  // sprite's normal map. Left out, the depth is 1 and the shape a disc.
  it('presses through the same command, with a depth', () => {
    const commands: unknown[] = [];
    const window = { __jengine_play: { commands } };
    // eslint-disable-next-line no-new-func
    const press = new Function('window', `${extract('press_sprite')}
return press_sprite;`)(window) as (
      ...a: unknown[]) => void;
    press(4, 6, 7, 12, -0.5, 'streak', 0.3);
    press(4, 0, 0, 9);
    expect(commands).toEqual([
      { t: 'spnt', e: 4, x: 6, y: 7, s: 12, dp: -0.5, sh: 'streak', rot: 0.3 },
      { t: 'spnt', e: 4, x: 0, y: 0, s: 9, dp: 1, sh: 'disc', rot: 0 },
    ]);
  });
});
