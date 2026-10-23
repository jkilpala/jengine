// The browser preamble's `Accessibility.announce` / `speech`, run from the
// shipped source (see preambleSpawn.test.ts for the harness). They ask two
// host hooks, `state.accessibility_announce(text)` / `accessibility_speech()`
// (adapters/js/accessibility.rs), over jengine_core::ui::accessibility,
// whose hidden ARIA mirror and live region the screen reader reads.
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

function api(state: Record<string, unknown> | undefined) {
  const window = { __jengine_play: { state } };
  const body = ['accessibility_announce', 'accessibility_speech'].map(extract).join('\n');
  // eslint-disable-next-line no-new-func
  return new Function('window', `${body}\nreturn { accessibility_announce, accessibility_speech };`)(window) as {
    accessibility_announce: (t: unknown) => void;
    accessibility_speech: () => string;
  };
}
const preamble = (state: Record<string, unknown> | undefined) => api(state).accessibility_announce;

describe('Accessibility in a browser', () => {
  it('hands the words to the host as a string', () => {
    const said: unknown[] = [];
    const announce = preamble({ accessibility_announce: (t: unknown) => said.push(t) });
    announce('Saved');
    announce(3);
    expect(said).toEqual(['Saved', '3']);
  });

  it('does nothing without a host', () => {
    expect(() => preamble(undefined)('Saved')).not.toThrow();
    expect(() => preamble({})('Saved')).not.toThrow();
    expect(api(undefined).accessibility_speech()).toBe('');
  });

  it('asks the host what a reader says for the focus', () => {
    expect(api({ accessibility_speech: () => 'Play, button' }).accessibility_speech()).toBe('Play, button');
  });
});
