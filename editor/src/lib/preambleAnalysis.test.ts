// The browser preamble's `Audio.get_spectrum` / `get_level` / `synth`, run
// from the shipped source (see preambleSpawn.test.ts for the harness). Each
// asks one host hook, `state.audio_analysis(op, a)` (adapters/js/analysis.rs),
// over jengine_core::audio::analysis. Without a host they answer silence.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const SRC = readFileSync(
  resolve(__dirname, '../../../crates/jengine-core/src/scripting/bridge/adapters/js/browser.rs'),
  'utf8',
).replace(/\r\n/g, '\n');

const NAMES = ['__analysis', 'audio_get_spectrum', 'audio_get_level', 'audio_synth'];

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

describe('Audio analysis in a browser', () => {
  it('hands every call to the host hook', () => {
    const calls: any[][] = [];
    const api = preamble({
      audio_analysis: (op: string, a: any) => {
        calls.push([op, a].filter((v) => v !== undefined));
        if (op === 'spectrum') return [0.1, 0.9];
        if (op === 'level') return 0.4;
        return null;
      },
    });
    expect(api.audio_get_spectrum(2)).toEqual([0.1, 0.9]);
    expect(api.audio_get_level()).toBe(0.4);
    api.audio_synth({ note: 60 });
    expect(calls).toEqual([['spectrum', 2], ['level'], ['synth', { note: 60 }]]);
  });

  it('answers silence without a host', () => {
    const api = preamble(undefined);
    expect(api.audio_get_spectrum(4)).toEqual([]);
    expect(api.audio_get_level()).toBe(0);
    expect(() => api.audio_synth({})).not.toThrow();
  });
});
