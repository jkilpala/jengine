// The browser preamble's `Noise.*` functions, run from the shipped source
// (see preambleSpawn.test.ts for the harness). Each asks one host hook,
// `state.noise(op, ...)` (adapters/js/noise.rs), which answers with
// jengine_core::noise - the same Rust native calls. Without a host they
// answer a flat field rather than throwing.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = readFileSync(
  resolve(__dirname, '../../../crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs'),
  'utf8',
).replace(/\r\n/g, '\n');

const NAMES = ['__noise', 'noise_perlin', 'noise_perlin3', 'noise_simplex', 'noise_cellular', 'noise_fractal', 'noise_grid'];

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

describe('Noise in a browser', () => {
  it('hands every call to the host hook with its arguments in order', () => {
    const calls: any[][] = [];
    const api = preamble({
      noise: (...args: any[]) => {
        calls.push(args.filter((v) => v !== undefined));
        if (args[0] === 'cellular') return [0.25, 0.5, 0.75];
        if (args[0] === 'grid') return [0.1, 0.2];
        return 0.5;
      },
    });
    expect(api.noise_perlin(1, 2, 3)).toBe(0.5);
    expect(api.noise_perlin3(1, 2, 3, 4)).toBe(0.5);
    expect(api.noise_simplex(1, 2)).toBe(0.5);
    expect(api.noise_cellular(1, 2, 3)).toEqual([0.25, 0.5, 0.75]);
    expect(api.noise_fractal(1, 2, { octaves: 3 })).toBe(0.5);
    expect(api.noise_grid(0, 0, 2, 1, 0.5, { seed: 4 })).toEqual([0.1, 0.2]);
    expect(calls).toEqual([
      ['perlin', 1, 2, 3],
      ['perlin3', 1, 2, 3, 4],
      ['simplex', 1, 2],
      ['cellular', 1, 2, 3],
      ['fractal', 1, 2, { octaves: 3 }],
      ['grid', 0, 0, 2, 1, 0.5, { seed: 4 }],
    ]);
  });

  it('answers a flat field without a host', () => {
    const api = preamble(undefined);
    expect(api.noise_perlin(1, 2)).toBe(0);
    expect(api.noise_cellular(1, 2)).toEqual([0, 0, 0]);
    expect(api.noise_grid(0, 0, 4, 4, 1)).toEqual([]);
  });
});
