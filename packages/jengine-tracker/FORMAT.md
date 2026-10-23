# `.song` — tracker music for jEngine

*Copied from jSuite's `docs/song-format.md`; the `src/lib/tracker/...` paths below are jSuite's.*

A `.song` is chip music as notes instead of samples: JSON a few kilobytes
long that the player synthesises at load. jSuite's Music Tracker (🎼)
writes it; the engine plays it once it has the player described in
[Engine integration](#engine-integration).

| Song (demo) | `.song` | gzipped | OGG q6 | OGG q3 |
|---|---|---|---|---|
| Sunny Fields, 48 s | 25 KB | 1.1 KB | 966 KB | 544 KB |
| Deep Cavern, 50 s | 25 KB | 1.2 KB | 1048 KB | 608 KB |
| Victory jingle, 3 s | 2 KB | 0.5 KB | 55 KB | 34 KB |

The trade is disk for CPU at load: the song is rendered to PCM once, the
same PCM an OGG decodes to, so **memory at runtime is unchanged** and only
the shipped bytes shrink.

**The reference implementation is `src/lib/tracker/synth.ts`.** This
document is normative where it gives a number, but where the two disagree,
`synth.ts` plus the digests in `src/lib/tracker/fixtures/` are what a port
must match. Every rule below exists so that a port *can* match — read
"Determinism" before changing anything.

## File

```json
{
  "version": 1,
  "name": "Sunny Fields",
  "bpm": 140,
  "rows_per_beat": 4,
  "speed": 6,
  "channels": 4,
  "volume": 0.45,
  "instruments": [ { "name": "Lead", "wave": "pulse", "duty": 0.25, … } ],
  "patterns": [ { "rows": [ "C-5 01 .. ... | C-4 02 .. 047 | C-2 03 .. ... | C-4 04 .. ...", "", … ] } ],
  "order": [0, 1, 2, 1, 2, 3, 4],
  "loop": 1
}
```

| Field | Type | Default | Range | Meaning |
|---|---|---|---|---|
| `version` | int | — | `1` | Refuse anything newer |
| `name` | string | `"Untitled Song"` | | |
| `bpm` | number | 120 | 20–999 | Musical beats per minute |
| `rows_per_beat` | int | 4 | 1–16 | 4 → a row is a 16th note |
| `speed` | int | 6 | 1–31 | Ticks per row — the resolution effects run at |
| `channels` | int | 4 | 1–16 | Cells per row |
| `volume` | number | 0.5 | 0–1 | Master gain |
| `instruments` | array | | 1–255 | Referenced 1-based from cells |
| `patterns` | array | | ≥ 1 | Each `{ "rows": [string; 1–256] }` |
| `order` | int array | | ≥ 1 | Pattern indices in play order |
| `loop` | int \| null | null | index into `order` | Where to go after the last order entry; null ends the song |

Out-of-range numbers are clamped; structural problems (unknown wave, a
cell naming a missing instrument, an order entry naming a missing pattern,
a malformed cell) are errors. An unknown effect is a **warning** and is
ignored at playback.

### Instruments

| Field | Default | Range | Meaning |
|---|---|---|---|
| `name` | | | |
| `wave` | `pulse` | `pulse` `triangle` `saw` `sine` `noise` `wave` | |
| `duty` | 0.5 | 0.01–0.99 | Pulse width (pulse only) |
| `table` | 4-bit triangle | 32 ints 0–15 | Wavetable (`wave` only) |
| `periodic` | false | | Noise: short LFSR tap, metallic (`noise` only) |
| `volume` | 0.8 | 0–1 | |
| `attack` / `decay` / `release` | 0.002 / 0.1 / 0.08 | 0–10 s | Linear segments |
| `sustain` | 0.7 | 0–1 | Level |
| `pan` | 0 | −1…1 | Added to the channel's `8xx` pan |
| `transpose` | 0 | −48…48 | Semitones added to every note |
| `vibrato` | null | `{depth, rate, delay}` | Semitones peak, Hz, seconds after note start |
| `slide` | 0 | st/s | Pitch drift from note start (a kick is `-150`) |
| `arp` | `[]` | semitones | Offsets cycled one per tick from note start |
| `cutoff` | 0 | 0–22050 Hz | One-pole low-pass; 0 = off |

### Rows and cells

A row is its cells joined by `|`. A cell is up to four space-separated
fields, `NOTE INST VOL FX`; missing trailing fields and missing trailing
cells are empty, and an empty row may be `""`. Any run of `.` is an empty
field. jSuite writes every cell in full (`C-4 01 .. ...`) so columns line
up; a parser must accept the short forms.

| Field | Form | Meaning |
|---|---|---|
| NOTE | `C-4`, `C#4`, `Db4` | **C-4 = MIDI 60**, A-4 = 69 = 440 Hz. Octaves −1…9 |
| | `===` | Note off: release |
| | `^^^` | Note cut: release over 4 ms |
| INST | hex `01`–`FF` | **1-based**; `00` is an error |
| VOL | hex `00`–`40` | Channel volume 0–64 |
| FX | one char `[0-9A-Z]` + two hex digits | See Effects |

### Effects

`x` and `y` are the parameter's high and low nibbles. "Per tick" effects
act on ticks 1…speed−1 of their row, not tick 0. Effect state that is not
called "memory" lasts only for the row it is written on.

| Effect | Meaning |
|---|---|
| `0xy` | Arpeggio: pitch + 0, + x, + y semitones on ticks `t % 3` = 0, 1, 2. `000` does nothing |
| `1xx` | Slide up `xx / 16` semitones per tick |
| `2xx` | Slide down `xx / 16` semitones per tick |
| `3xx` | Portamento toward the cell's note at `xx / 16` st/tick; `00` = last speed (memory). The note does not retrigger if the channel is sounding |
| `4xy` | Vibrato: phase += x per tick in a 64-step cycle, depth y / 8 semitones; 0 = last value (memory) |
| `8xx` | Channel pan `(xx − 0x80) / 0x7F`, clamped to −1…1: `00` left, `80` centre, `FF` right. Persists |
| `9xx` | Pulse width `max(1, xx) / 256`. Persists until the next instrument |
| `Axy` | Volume slide per tick: + x if x > 0, else − y. Clamped 0–64 |
| `Bxx` | After this row, jump to order `xx`, row 0 |
| `Cxx` | Set volume `min(xx, 0x40)` |
| `Dxx` | After this row, go to the next order entry, row `xx` (row 0 if out of range) |
| `ECx` | Cut on tick x (`EC0` = at once) |
| `EDx` | Delay this cell's note, instrument and volume to tick x |
| `Fxx` | `xx ≥ 0x20`: BPM = xx. `0 < xx < 0x20`: speed = xx. Takes effect on its own row |

## Playback model

Everything below is per channel unless it says otherwise. Read it next to
`synth.ts`; the names match.

### Time

```
spt = rate * 60 / (bpm * rows_per_beat * speed)        // f64 frames per tick
tick_frames = floor(tick_pos + spt) - floor(tick_pos); tick_pos += spt
```

`tick_pos` starts at 0 and is never reset, so tempo changes do not drift.
Within a tick, frames are rendered in **control blocks** of
`min(32, frames left in tick)`; each block starts by recomputing every
channel's frequency and pan gains. The block and tick counters live in the
sequencer, never in the caller's buffer size.

### Sequencing

At the start of each tick:

1. **Tick 0** reads the row: for each channel, reset the row's effect
   state (arp, slide, porta, volume slide, vibrato-on, cut tick, delay),
   then *trigger* the cell (or, with `EDx`, x > 0, store it for tick x),
   then apply the cell's effect setup. Then any `EC0` cuts.
   **Ticks 1+** run, per channel in order: a delayed trigger due this tick;
   a cut due this tick; slide (pitch clamped 0–140); portamento; volume
   slide; vibrato phase += speed (`& 63`).
2. Every sounding channel's `note_ticks += 1`.
3. Compute the tick's frame count (with the *current* bpm and speed).
4. Advance: `tick += 1`; when it reaches `speed`, `tick = 0` and move to
   the next row — a `Bxx` / `Dxx` from this row wins, else row + 1, rolling
   into the next order entry. Past the last entry, go to `loop`, or end.

**Trigger** (a cell's note/instrument/volume):

- An instrument sets the channel's instrument, volume to 64, duty to the
  instrument's, and the low-pass coefficient
  `1 − exp(−2π · cutoff / rate)` (1 when cutoff is 0).
- A volume sets the channel volume.
- `===` releases over the instrument's release; `^^^` over 4 ms.
- A note (pitch = note + transpose), unless `3xx` on a sounding channel
  (then it only sets the portamento target): pitch and target = pitch,
  phase = 0, LFSR = 1, vibrato phase = 0, note frames/ticks = 0; if the
  channel was idle, mark it to **snap** its gains; enter **attack from the
  current envelope level** with step `1 / (max(attack, 0.001) · rate)`.

### Control block (per channel)

```
p  = pitch
   + arp effect offset (by the tick being played, % 3)
   + vib_depth · sin(2π · vib_phase / 64)            (if 4xy this row)
   + inst.arp[(note_ticks − 1) % len]                (if any)
   + inst.slide · t                                  (t = note_frames / rate)
   + v.depth · sin(2π · v.rate · (t − v.delay))      (if t ≥ v.delay)
hz = 440 · 2^((clamp(p, −24, 140) − 69) / 12)
gain = inst.volume · vol / 64;  pan = clamp(inst.pan + channel pan, −1, 1)
target_l = gain · cos((pan + 1)π/4);  target_r = gain · sin((pan + 1)π/4)
if snap: gain_l/r = target_l/r, snap = false
```

An idle channel's targets are 0.

### Per frame (per channel)

1. Gains glide: `g += (target − g) · (1 − exp(−1 / (0.004 · rate)))`.
2. Envelope (skip if idle): attack adds its step until 1 → decay, step
   `(1 − sustain) / (max(decay, 0.001) · rate)`, until ≤ sustain → sustain
   (or **idle** if sustain is 0). Release subtracts
   `level_at_release / (max(release, 0.001) · rate)` until ≤ 0 → idle.
3. Oscillator at `dt = hz / rate`, phase in [0, 1):
   - pulse: `±1` (`+1` while phase < duty) + PolyBLEP at 0 − PolyBLEP at duty
   - triangle: `phase < 0.5 ? 4·phase − 1 : 3 − 4·phase`
   - saw: `2·phase − 1 − PolyBLEP(phase)`
   - sine: `sin(2π · phase)`
   - wave: `table[floor(phase · 32)] / 7.5 − 1`
   - noise: `lfsr & 1 ? 1 : −1`
4. Advance: noise clocks a 15-bit LFSR at `16 · hz` —
   `bit = (lfsr ^ (lfsr >> tap)) & 1; lfsr = (lfsr >> 1) | (bit << 14)`,
   tap 6 if periodic else 1 — once per whole phase step. Others:
   `phase += dt; if phase ≥ 1 { phase −= floor(phase) }`.
5. `out = osc · env`, then the low-pass `lp += (out − lp) · coef` if on.

### Mix (per frame)

Sum `out · gain_l` / `out · gain_r` over channels, × master volume, then a
DC blocker per side (`y = x − x₁ + R·y₁`, `R = 1 − 2π · 10 / rate`), then a
soft limiter: linear to ±0.8, beyond that
`0.8 + 0.2 · tanh((|x| − 0.8) / 0.2)` with the sign restored.

### One pass, loop point and end

The engine renders **one pass**: stop at the first row (order, row) that
has already played — its first frame is the **loop start** — or, for a
song with no loop, keep ticking (no rows) after the last row until every
channel is idle or 4 s have passed. Guard at 20 minutes.

## Determinism

The fixtures were produced by `synth.ts` in f64. A port in Rust f64 will
not be bit-identical — `sin`, `exp`, `pow` and `tanh` may differ in the
last place between libms — so the fixtures are **digests**, not samples
(`src/lib/tracker/digest.ts`):

- render at **44100 Hz**, one pass;
- `frames` and `loop_start` must match **exactly** (timing has no excuse);
- per window of 2048 frames: RMS of left and right as `round(rms · 1e5)`,
  within **20**; zero crossings of the left channel, counted with
  hysteresis (a sign registers only past ±0.01, a crossing is a change
  between registered signs; the state carries across windows), within **2**.

Fixtures: `effects` (every effect and waveform, a `Dxx` into a `Bxx` loop
mid-pattern) plus the three demo songs. If the port fails a window, it is
almost certainly an ordering difference — which tick a thing happens on,
or a value read before or after it changes — not a float problem.

## Engine integration

*Built Oct 2026 as `packages/jengine-tracker`* - see its `README.md`. In
short: core has `audio::transcode`, a process-wide registry of clip
transcoders that both `load_clip`s (kira, Web Audio) ask before decoding a
file; the package registers `.song` there through
`JEnginePackage::register_audio`. A rendered song goes straight in as
stereo frames (no WAV round trip), each backend keeps its loop start per
clip and loops from it, and native `audio_refs` preloading renders the
song while the scene loads (`transcode::prepare`). A project that plays a
`.song` without the package is told which package to add, and
`every_song_a_project_plays_has_its_player_linked` fails it.

The fixture comparison is `tests/songs_match_jsuite.rs`, over jSuite's
fixtures copied into `tests/data/`.

### Later: live channels

Pre-rendering makes a song a clip, which is what lets every music feature
work unchanged — and is also why individual channels cannot be muted or
re-voiced at runtime. Adaptive music *inside* a song (drums only in
combat) would need the sequencer running as a streaming kira `Sound` and a
`ScriptProcessor`/worklet on the web. Until then, the music director's
stem layers do the same job: export each layer as its own `.song` and use
`Audio.play_music_layers`.
