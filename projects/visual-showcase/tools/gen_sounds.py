r"""Write the showcase's sounds: loops and one-shots synthesised from numpy, so
they are made the way the art is - in code, regenerable, no recordings.

Every loop is seamless: noise beds are cross-faded end into start, and tonal
loops are built from whole cycles of their beat. 16-bit mono WAV at 22050 Hz
- small, and every runtime decodes it.

Re-run after editing:
    python tools/gen_sounds.py      (from the project directory)
"""
import wave
from pathlib import Path

import numpy as np

OUT = Path(__file__).resolve().parent.parent / "assets" / "sounds"
SR = 22050
RNG = np.random.default_rng(43)


def write(name, x):
    """Normalise to -1 dB and write 16-bit mono."""
    x = np.asarray(x, dtype=np.float64)
    peak = np.max(np.abs(x)) or 1.0
    x = x / peak * 0.89
    pcm = (np.clip(x, -1, 1) * 32767).astype("<i2")
    OUT.mkdir(parents=True, exist_ok=True)
    with wave.open(str(OUT / name), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())
    print(f"wrote assets/sounds/{name}  {len(x) / SR:.1f} s")


def seamless(x, fade=0.5):
    """Cross-fade a loop's tail into its head, so it repeats without a click."""
    n = int(fade * SR)
    head, tail = x[:n].copy(), x[-n:]
    t = np.linspace(0, 1, n)
    x = x[:-n].copy()
    x[:n] = head * t + tail * (1 - t)
    return x


def lowpass(x, a):
    """One-pole low-pass: `a` near 1 is darker."""
    y = np.empty_like(x)
    acc = 0.0
    for i, v in enumerate(x):
        acc = a * acc + (1 - a) * v
        y[i] = acc
    return y


def env_pluck(n, decay):
    t = np.arange(n) / SR
    return np.minimum(t / 0.004, 1.0) * np.exp(-t / decay)


def pluck(freq, dur, decay=0.45, bright=0.5):
    """A plucked string: a few harmonics fading faster as they rise."""
    n = int(dur * SR)
    t = np.arange(n) / SR
    out = np.zeros(n)
    for k in range(1, 7):
        out += (bright ** (k - 1)) * np.sin(2 * np.pi * freq * k * t) * np.exp(-t * k / (decay * 2.5))
    return out * env_pluck(n, decay)


def bell(freq, dur, decay=1.2):
    """A bell: inharmonic partials."""
    n = int(dur * SR)
    t = np.arange(n) / SR
    out = np.zeros(n)
    for ratio, amp in ((1.0, 1.0), (2.76, 0.5), (5.40, 0.25), (8.93, 0.12)):
        out += amp * np.sin(2 * np.pi * freq * ratio * t) * np.exp(-t * ratio ** 0.5 / decay)
    return out * env_pluck(n, decay)


def place(buf, x, at):
    i = int(at * SR)
    j = min(len(buf), i + len(x))
    buf[i:j] += x[: j - i]


def market_music():
    """A lute tune, eight bars at 100 bpm, in A minor - a whole number of
    beats, so it loops."""
    beat = 60 / 100
    bars = 8
    n = int(bars * 4 * beat * SR)
    buf = np.zeros(n + SR)
    a = 220.0
    scale = [0, 2, 3, 5, 7, 8, 10, 12, 14, 15]
    tune = [0, 4, 7, 4, 5, 4, 2, 0, 3, 5, 7, 9, 7, 5, 4, 2,
            0, 4, 7, 9, 8, 7, 5, 4, 2, 4, 5, 2, 0, 2, 0, 0]
    for i, deg in enumerate(tune):
        f = a * 2 ** (scale[deg] / 12)
        place(buf, pluck(f, beat * 1.9, 0.55) * 0.55, i * beat)
        if i % 2 == 0:
            root = a / 2 * 2 ** (scale[[0, 3, 5, 4][(i // 8) % 4]] / 12)
            place(buf, pluck(root, beat * 2.2, 0.8, 0.35) * 0.45, i * beat)
    # Wrap the ringing tail onto the start, so the loop is seamless.
    buf[:SR] += buf[n:n + SR]
    write("market_lute.wav", buf[:n])


def grill_sizzle():
    """Fat on a griddle: bright hiss that swells and spits."""
    n = 5 * SR
    hiss = RNG.normal(0, 1, n + SR // 2)
    hiss = hiss - lowpass(hiss, 0.6)                 # high-passed
    swell = 0.7 + 0.3 * np.sin(np.linspace(0, 2 * np.pi * 3, len(hiss)))
    hiss *= swell
    for _ in range(60):
        at = RNG.integers(0, len(hiss) - 800)
        pop = RNG.normal(0, 1, 800) * np.exp(-np.arange(800) / 120) * RNG.uniform(2, 5)
        hiss[at:at + 800] += pop
    write("market_grill.wav", seamless(hiss))


def fountain_water():
    """Falling water: low rumble, a splash bed, and bubbles."""
    n = 6 * SR
    m = n + SR // 2
    brown = np.cumsum(RNG.normal(0, 1, m))
    brown -= lowpass(brown, 0.999)
    splash = RNG.normal(0, 1, m)
    splash = lowpass(splash, 0.55) - lowpass(splash, 0.92)
    x = brown * 0.015 + splash * 1.2
    t = np.arange(m) / SR
    for _ in range(90):
        at = RNG.uniform(0, m / SR - 0.1)
        f0 = RNG.uniform(500, 1400)
        k = (t >= at) & (t < at + 0.06)
        tt = t[k] - at
        x[k] += 0.25 * np.sin(2 * np.pi * (f0 + 4000 * tt) * tt) * np.exp(-tt / 0.015)
    write("market_fountain.wav", seamless(x))


def wind_chimes():
    """Chimes on a stall, struck by the breeze: random notes of a
    pentatonic set, a few a second, in a loop whose tails wrap round."""
    dur = 8.0
    n = int(dur * SR)
    buf = np.zeros(n + 3 * SR)
    notes = [880.0 * 2 ** (s / 12) for s in (0, 2, 4, 7, 9, 12, 14)]
    at = 0.0
    while at < dur:
        place(buf, bell(RNG.choice(notes), 2.5, RNG.uniform(0.8, 1.6)) * RNG.uniform(0.3, 0.7), at)
        at += RNG.uniform(0.25, 0.9)
    buf[:3 * SR] += buf[n:n + 3 * SR]
    write("market_chimes.wav", buf[:n])


def coin():
    """A coin tossed onto a counter: two bright rings."""
    x = np.zeros(int(0.8 * SR))
    place(x, bell(2350, 0.7, 0.25) * 0.8, 0.0)
    place(x, bell(3150, 0.6, 0.2) * 0.5, 0.07)
    write("market_coin.wav", x)


def cove_waves():
    """Surf on a beach: slow swells of filtered noise, each with a hiss as
    it breaks - eight seconds, seamless."""
    n = 8 * SR
    m = n + SR // 2
    noise = RNG.normal(0, 1, m)
    body = lowpass(noise, 0.97)
    hiss = noise - lowpass(noise, 0.7)
    t = np.arange(m) / SR
    swell = 0.35 + 0.65 * np.maximum(0, np.sin(2 * np.pi * t / 4.0)) ** 2
    brk = np.maximum(0, np.sin(2 * np.pi * (t - 0.6) / 4.0)) ** 6
    x = body * 6.0 * swell + hiss * 0.5 * brk
    write("cove_waves.wav", seamless(x))


def cove_drips():
    """Water dripping in a cave: a few clear plinks a second, dry - so the
    cave's reverb is what makes them sound like a cave."""
    dur = 6.0
    n = int(dur * SR)
    buf = np.zeros(n + SR)
    at = 0.1
    while at < dur:
        f0 = RNG.uniform(700, 1500)
        m = int(0.12 * SR)
        tt = np.arange(m) / SR
        drop = np.sin(2 * np.pi * (f0 + 2500 * tt) * tt) * np.exp(-tt / 0.025)
        place(buf, drop * RNG.uniform(0.4, 1.0), at)
        at += RNG.uniform(0.35, 1.1)
    buf[:SR] += buf[n:n + SR]
    write("cove_drips.wav", buf[:n])


def cove_gulls():
    """Gulls far off: a cry or two every few seconds, falling in pitch."""
    dur = 10.0
    n = int(dur * SR)
    buf = np.zeros(n + 2 * SR)
    at = 0.5
    while at < dur:
        for k in range(RNG.integers(1, 4)):
            m = int(0.32 * SR)
            tt = np.arange(m) / SR
            f = 1500 - 900 * (tt / 0.32) ** 0.7 + 60 * np.sin(2 * np.pi * 30 * tt)
            phase = 2 * np.pi * np.cumsum(f) / SR
            cry = (np.sin(phase) + 0.4 * np.sin(2 * phase) + 0.2 * np.sin(3 * phase)) * np.sin(np.pi * tt / 0.32) ** 1.5
            place(buf, cry * RNG.uniform(0.3, 0.6), at + k * 0.38)
        at += RNG.uniform(2.0, 4.0)
    buf[:2 * SR] += buf[n:n + 2 * SR]
    write("cove_gulls.wav", buf[:n])


def cove_music():
    """A slow harp arpeggio in D major, sixteen beats at 72 bpm - whole
    beats, so it loops; its tails wrap round."""
    beat = 60 / 72
    n = int(16 * beat * SR)
    buf = np.zeros(n + 3 * SR)
    d = 293.66
    chords = [[0, 4, 7, 12], [9, 12, 16, 21], [7, 11, 14, 19], [5, 9, 12, 17]]
    for bar, chord in enumerate(chords):
        for i in range(8):
            step = chord[[0, 1, 2, 3, 2, 3, 1, 2][i]]
            f = d / 2 * 2 ** (step / 12)
            place(buf, pluck(f, 2.5, 1.1, 0.4) * 0.5, (bar * 4 + i * 0.5) * beat)
        place(buf, pluck(d / 4 * 2 ** (chord[0] / 12), 3.5, 1.6, 0.3) * 0.45, bar * 4 * beat)
    buf[:3 * SR] += buf[n:n + 3 * SR]
    write("cove_music.wav", buf[:n])


def cove_splash():
    """Breaking the surface: a burst of noise and a low plop."""
    m = int(0.7 * SR)
    t = np.arange(m) / SR
    noise = RNG.normal(0, 1, m)
    burst = (noise - lowpass(noise, 0.5)) * np.exp(-t / 0.12)
    plop = np.sin(2 * np.pi * (180 + 300 * np.exp(-t / 0.05)) * t) * np.exp(-t / 0.15)
    write("cove_splash.wav", burst * 0.6 + plop)


def forge_clang():
    """A hammer on hot iron: a bright inharmonic ring over a short crack."""
    m = int(1.2 * SR)
    t = np.arange(m) / SR
    x = np.zeros(m)
    for ratio, amp, decay in ((1.0, 1.0, 0.35), (2.41, 0.6, 0.22), (3.83, 0.45, 0.16), (5.97, 0.3, 0.1), (8.2, 0.2, 0.07)):
        x += amp * np.sin(2 * np.pi * 920 * ratio * t) * np.exp(-t / decay)
    noise = RNG.normal(0, 1, m)
    crack = (noise - lowpass(noise, 0.4)) * np.exp(-t / 0.015)
    write("forge_clang.wav", x * 0.6 + crack * 0.9)


def forge_fire():
    """Coals in a forge: a low roar and crackles, six seconds, seamless."""
    n = 6 * SR
    m = n + SR // 2
    roar = np.cumsum(RNG.normal(0, 1, m))
    roar -= lowpass(roar, 0.995)
    x = roar * 0.02
    t = np.arange(m) / SR
    for _ in range(140):
        at = RNG.integers(0, m - 400)
        pop = RNG.normal(0, 1, 400) * np.exp(-np.arange(400) / RNG.uniform(20, 80)) * RNG.uniform(0.3, 1.2)
        x[at:at + 400] += pop
    write("forge_fire.wav", seamless(x))


def forge_hiss():
    """Hot iron in water: a loud hiss that boils down."""
    m = int(1.6 * SR)
    t = np.arange(m) / SR
    noise = RNG.normal(0, 1, m)
    hiss = noise - lowpass(noise, 0.75)
    bubbles = np.zeros(m)
    for _ in range(30):
        at = RNG.uniform(0.05, 1.3)
        k = (t >= at) & (t < at + 0.03)
        tt = t[k] - at
        bubbles[k] += 0.3 * np.sin(2 * np.pi * RNG.uniform(300, 800) * tt) * np.exp(-tt / 0.01)
    env = np.minimum(t / 0.02, 1) * np.exp(-t / 0.55)
    write("forge_hiss.wav", hiss * env + bubbles * env)


def forge_bellows():
    """A pump of the bellows: a soft whoosh."""
    m = int(0.6 * SR)
    t = np.arange(m) / SR
    noise = RNG.normal(0, 1, m)
    air = lowpass(noise, 0.85) - lowpass(noise, 0.97)
    env = np.sin(np.pi * t / 0.6) ** 2
    write("forge_bellows.wav", air * env)


def stage_beat():
    """A band's groove, eight bars at 112 bpm: kick, snare, hats and a bass
    line - whole bars, so it loops."""
    beat = 60 / 112
    bars = 8
    n = int(bars * 4 * beat * SR)
    buf = np.zeros(n + SR)

    def kick():
        m = int(0.35 * SR)
        t = np.arange(m) / SR
        return np.sin(2 * np.pi * (50 + 110 * np.exp(-t / 0.04)) * t) * np.exp(-t / 0.18)

    def snare():
        m = int(0.25 * SR)
        t = np.arange(m) / SR
        noise = RNG.normal(0, 1, m)
        return (noise - lowpass(noise, 0.6)) * np.exp(-t / 0.07) * 0.6 + np.sin(2 * np.pi * 190 * t) * np.exp(-t / 0.05) * 0.4

    def hat():
        m = int(0.06 * SR)
        noise = RNG.normal(0, 1, m)
        return (noise - lowpass(noise, 0.3)) * np.exp(-np.arange(m) / SR / 0.015) * 0.25

    k, s = kick(), snare()
    bass_notes = [41.2, 41.2, 49.0, 55.0, 36.7, 36.7, 49.0, 46.2]
    for bar in range(bars):
        for b in range(4):
            at = (bar * 4 + b) * beat
            if b in (0, 2):
                place(buf, k, at)
            if b in (1, 3):
                place(buf, s, at)
            place(buf, hat(), at)
            place(buf, hat() * 0.6, at + beat / 2)
            f = bass_notes[bar] * (2 if b == 3 else 1)
            m = int(beat * 0.9 * SR)
            t = np.arange(m) / SR
            tone = np.sign(np.sin(2 * np.pi * f * 2 * t)) * 0.25 + np.sin(2 * np.pi * f * 2 * t)
            place(buf, lowpass(tone, 0.85) * np.exp(-t / 0.35) * 0.5, at)
    buf[:SR] += buf[n:n + SR]
    write("stage_beat.wav", buf[:n])



def canyon_shout():
    """A call of "hey!": a voiced pitch gliding up then down, through two
    formants that move from 'e' to 'i' - a short vowel the echo can carry."""
    dur = 0.55
    n = int(dur * SR)
    t = np.arange(n) / SR
    f0 = 210 + 60 * np.sin(np.pi * np.clip(t / dur, 0, 1)) - 30 * t
    phase = 2 * np.pi * np.cumsum(f0) / SR
    voice = np.zeros(n)
    for k in range(1, 16):
        voice += np.sin(k * phase) / k
    # Formants: resonances as Gaussian weights on each harmonic's frequency.
    f1 = 550 - 200 * (t / dur)
    f2 = 1900 + 500 * (t / dur)
    shaped = np.zeros(n)
    for k in range(1, 16):
        fk = k * f0
        w = np.exp(-((fk - f1) / 160) ** 2) + 0.7 * np.exp(-((fk - f2) / 260) ** 2) + 0.08
        shaped += w * np.sin(k * phase) / k ** 0.5
    breath = lowpass(RNG.standard_normal(n), 0.6) * 0.05
    env = np.minimum(t / 0.03, 1.0) * np.clip((dur - t) / 0.18, 0, 1)
    h = np.exp(-t / 0.05) * RNG.standard_normal(n) * 0.25  # the 'h'
    write("canyon_shout.wav", (shaped * 0.6 + voice * 0.1 + breath) * env + h * np.minimum(t / 0.01, 1))


def canyon_step():
    """A boot on gravel: a dull thud and a short crunch of grit."""
    n = int(0.16 * SR)
    t = np.arange(n) / SR
    thud = np.sin(2 * np.pi * 90 * t) * np.exp(-t / 0.03)
    grit = lowpass(RNG.standard_normal(n), 0.35) * np.exp(-t / 0.045)
    clicks = (RNG.random(n) > 0.985) * RNG.standard_normal(n) * np.exp(-t / 0.06)
    write("canyon_step.wav", thud * 0.8 + grit * 0.5 + clicks * 0.6)


def canyon_radio():
    """An old swing tune for the camp radio: a walking bass, a muted horn
    line and brushed drums, eight bars at 120 bpm - a loop of whole beats."""
    beat = 60 / 120
    bars = 8
    n = int(bars * 4 * beat * SR)
    buf = np.zeros(n + SR)
    roots = [196.0, 196.0, 261.6, 261.6, 196.0, 146.8, 196.0, 146.8]
    for bar in range(bars):
        r = roots[bar]
        for b in range(4):
            at = (bar * 4 + b) * beat
            f = r * [1, 1.26, 1.5, 1.68][b] / 2
            m = int(beat * 0.95 * SR)
            tt = np.arange(m) / SR
            place(buf, np.sin(2 * np.pi * f * tt) * np.exp(-tt / 0.3) * 0.7, at)
            brush = lowpass(RNG.standard_normal(int(0.12 * SR)), 0.2) * np.exp(-np.arange(int(0.12 * SR)) / SR / 0.04)
            place(buf, brush * (0.35 if b % 2 else 0.2), at)
    tune = [7, 9, 10, 9, 7, 4, 7, -1, 5, 7, 9, 7, 5, 2, 4, -1,
            7, 9, 10, 12, 10, 9, 7, -1, 5, 4, 2, 4, 0, -1, 0, -1]
    for i, deg in enumerate(tune):
        if deg < 0:
            continue
        f = 392.0 * 2 ** (deg / 12) / 2
        m = int(beat * 0.85 * SR)
        tt = np.arange(m) / SR
        horn = sum(np.sin(2 * np.pi * f * k * tt) / k for k in range(1, 7))
        env = np.minimum(tt / 0.03, 1) * np.exp(-tt / 0.6)
        place(buf, horn * env * 0.35, i * beat)
    buf[:SR] += buf[n:n + SR]
    write("canyon_radio.wav", buf[:n])


def canyon_wind():
    """Wind along the canyon: noise swelling and falling, darker as it falls."""
    n = int(8.0 * SR)
    t = np.arange(n) / SR
    swell = 0.55 + 0.45 * np.sin(2 * np.pi * t / 8.0) * np.sin(2 * np.pi * t / 2.7 + 1)
    x = lowpass(RNG.standard_normal(n), 0.97) * swell
    write("canyon_wind.wav", seamless(x * 6.0))



# ── 88 Night Drive ─────────────────────────────────────────────────────
# A synthwave score in four stems of one length - pad, drums, bass, lead -
# that the scene fades in as the car speeds up (Audio.set_music_layer); two
# radio tracks for its playlist; and an engine note whose pitch follows the
# speed (Audio.set_sound_pitch). The stems are whole bars at one tempo, so
# started together they stay together for as long as they loop.

ND_BPM = 110
ND_BARS = 4
ND_CHORDS = [(110.0, 130.8, 164.8), (87.3, 110.0, 130.8), (130.8, 164.8, 196.0), (98.0, 123.5, 146.8)]  # Am F C G


def nd_len():
    return int(ND_BARS * 4 * 60 / ND_BPM * SR)


def nd_saw(f, t):
    return 2.0 * ((f * t) % 1.0) - 1.0


def nd_stem_pad():
    n = nd_len()
    bar = n // ND_BARS
    t = np.arange(bar) / SR
    out = np.zeros(n)
    for b, chord in enumerate(ND_CHORDS):
        tone = sum(nd_saw(f * d, t) for f in chord for d in (1.0, 1.006))
        swell = np.minimum(t / 0.6, 1.0) * np.minimum((t[-1] - t) / 0.25 + 0.05, 1.0)
        out[b * bar:(b + 1) * bar] = lowpass(tone, 0.93) * swell
    write("nd_pad.wav", out)


def nd_stem_drums():
    beat = 60 / ND_BPM
    n = nd_len()
    buf = np.zeros(n + SR)
    m = int(0.35 * SR)
    t = np.arange(m) / SR
    kick = np.sin(2 * np.pi * (48 + 120 * np.exp(-t / 0.035)) * t) * np.exp(-t / 0.2)
    m = int(0.3 * SR)
    t = np.arange(m) / SR
    noise = RNG.normal(0, 1, m)
    snare = ((noise - lowpass(noise, 0.5)) * 0.7 + np.sin(2 * np.pi * 180 * t) * 0.4) * np.exp(-t / 0.12)
    for k in range(ND_BARS * 4):
        at = k * beat
        place(buf, kick, at)
        if k % 2 == 1:
            place(buf, snare, at)
        for h in range(2):
            m = int(0.05 * SR)
            hn = RNG.normal(0, 1, m)
            place(buf, (hn - lowpass(hn, 0.25)) * np.exp(-np.arange(m) / SR / 0.012) * (0.3 if h else 0.18), at + h * beat / 2)
    buf[:SR] += buf[n:n + SR]
    write("nd_drums.wav", buf[:n])


def nd_stem_bass():
    beat = 60 / ND_BPM
    n = nd_len()
    buf = np.zeros(n + SR)
    m = int(beat / 2 * 0.95 * SR)
    t = np.arange(m) / SR
    for b, chord in enumerate(ND_CHORDS):
        root = chord[0] / 2
        for k in range(8):
            f = root * (2 if k % 2 else 1)
            note = lowpass(nd_saw(f, t), 0.8) * np.exp(-t / 0.25)
            place(buf, note, (b * 4 + k / 2) * beat)
    buf[:SR] += buf[n:n + SR]
    write("nd_bass.wav", buf[:n])


def nd_stem_lead():
    beat = 60 / ND_BPM
    n = nd_len()
    buf = np.zeros(n + SR)
    m = int(beat / 4 * 0.9 * SR)
    t = np.arange(m) / SR
    for b, chord in enumerate(ND_CHORDS):
        arp = [chord[0] * 2, chord[1] * 2, chord[2] * 2, chord[1] * 4]
        for k in range(16):
            f = arp[k % 4]
            tone = (np.sign(np.sin(2 * np.pi * f * t)) * 0.4 + nd_saw(f * 1.004, t) * 0.6)
            place(buf, lowpass(tone, 0.7) * np.exp(-t / 0.09), (b * 4 + k / 4) * beat)
    # A slapback echo, so it sounds like a synth through a cheap delay.
    d = int(beat * 0.75 * SR)
    buf[d:] += buf[:-d] * 0.35
    buf[:SR] += buf[n:n + SR]
    write("nd_lead.wav", buf[:n])


def nd_radio(name, bpm, chords, bright):
    """A radio station: a short loop of its own - a slower or brighter
    groove, eight bars, with a hiss of airwaves on top."""
    beat = 60 / bpm
    n = int(8 * 4 * beat * SR)
    buf = np.zeros(n + SR)
    m = int(beat * 0.9 * SR)
    t = np.arange(m) / SR
    for bar in range(8):
        chord = chords[bar % len(chords)]
        for k in range(4):
            for f in chord:
                place(buf, lowpass(nd_saw(f * (1 + bright), t), 0.88) * np.exp(-t / 0.5) * 0.3, (bar * 4 + k) * beat)
            if k in (0, 2):
                mt = np.arange(int(0.3 * SR)) / SR
                place(buf, np.sin(2 * np.pi * (50 + 90 * np.exp(-mt / 0.04)) * mt) * np.exp(-mt / 0.16), (bar * 4 + k) * beat)
    buf[:SR] += buf[n:n + SR]
    x = buf[:n] + lowpass(RNG.normal(0, 1, n), 0.2) * 0.02
    write(name, x)


def nd_engine():
    """An engine note at idle: the firing pulse and its harmonics, one second
    of whole cycles of 45 Hz so it loops - the scene raises its pitch with the
    speed."""
    n = SR
    t = np.arange(n) / SR
    f = 45.0
    x = sum(np.sin(2 * np.pi * f * k * t + k) / k ** 0.8 for k in range(1, 9))
    x *= 0.8 + 0.2 * np.sin(2 * np.pi * f / 2 * t)
    x = lowpass(x + RNG.normal(0, 0.15, n), 0.55)
    write("nd_engine.wav", seamless(np.concatenate([x, x[:int(0.25 * SR)]]), 0.25))


def nd88_drive():
    nd_stem_pad()
    nd_stem_drums()
    nd_stem_bass()
    nd_stem_lead()
    nd_radio("nd_radio_coast.wav", 92, [(110.0, 138.6, 164.8), (123.5, 146.8, 185.0)], 0.0)
    nd_radio("nd_radio_glow.wav", 124, [(146.8, 185.0, 220.0), (130.8, 164.8, 196.0), (110.0, 130.8, 164.8)], 0.002)
    nd_engine()


# ── 112 Haunted Manor ───────────────────────────────────────────────────
# A loop for each room, crossfaded as the torch-bearer walks from one to
# the next: an organ drone for the hall, a music box for the library, a
# waltz for the ballroom, a choir with a bell for the chapel. Each a whole
# number of beats, its tails wrapped round, so it loops.

def hm_organ(freqs, dur):
    """A soft organ chord: a few partials each, slow swell, a slow tremolo."""
    n = int(dur * SR)
    t = np.arange(n) / SR
    out = np.zeros(n)
    for f in freqs:
        for k, a in ((1, 1.0), (2, 0.4), (3, 0.15), (4, 0.08)):
            out += a * np.sin(2 * np.pi * f * k * t)
    env = np.minimum(t / 1.2, 1.0) * np.minimum((dur - t) / 1.2, 1.0)
    return out * env * (0.85 + 0.15 * np.sin(2 * np.pi * 4.5 * t))


def manor_hall():
    beat = 60 / 60
    n = int(16 * beat * SR)
    buf = np.zeros(n + 3 * SR)
    a = 110.0
    chords = [[0, 3, 7], [-4, 0, 3], [-2, 2, 5], [-5, -1, 2]]
    for bar, chord in enumerate(chords):
        place(buf, hm_organ([a * 2 ** (c / 12) for c in chord] + [a / 2 * 2 ** (chord[0] / 12)], 4.6) * 0.3, bar * 4 * beat)
    buf[:3 * SR] += buf[n:n + 3 * SR]
    write("manor_hall.wav", buf[:n])


def manor_library():
    beat = 60 / 84
    n = int(16 * beat * SR)
    buf = np.zeros(n + 3 * SR)
    e = 659.25
    tune = [0, 3, 7, 12, 10, 7, 3, 5, 7, 5, 3, 2, 0, -2, 0, None]
    for i, step in enumerate(tune):
        if step is not None:
            place(buf, bell(e * 2 ** (step / 12), 2.0, 0.6) * 0.4, i * beat)
    buf[:3 * SR] += buf[n:n + 3 * SR]
    write("manor_library.wav", buf[:n])


def manor_ballroom():
    beat = 60 / 132
    n = int(24 * beat * SR)
    buf = np.zeros(n + 3 * SR)
    d = 146.83
    chords = [[0, 3, 7], [5, 8, 12], [7, 10, 14], [0, 3, 7], [-2, 2, 5], [7, 11, 14], [0, 3, 7], [7, 10, 14]]
    for bar, chord in enumerate(chords):
        t0 = bar * 3 * beat
        place(buf, pluck(d / 2 * 2 ** (chord[0] / 12), 1.4, 0.7, 0.35) * 0.6, t0)
        for b in (1, 2):
            for c in chord[1:]:
                place(buf, pluck(d * 2 ** (c / 12), 0.8, 0.35, 0.4) * 0.25, t0 + b * beat)
        place(buf, pluck(d * 2 * 2 ** (chord[2] / 12), 1.2, 0.6, 0.5) * 0.3, t0)
    buf[:3 * SR] += buf[n:n + 3 * SR]
    write("manor_ballroom.wav", buf[:n])


def manor_chapel():
    beat = 60 / 50
    n = int(12 * beat * SR)
    buf = np.zeros(n + 4 * SR)
    g = 98.0
    for bar, chord in enumerate([[0, 7, 12, 15], [-4, 3, 8, 12], [-2, 5, 10, 14]]):
        voices = hm_organ([g * 2 ** (c / 12) for c in chord], 5.4)
        voices = lowpass(voices, 0.85)
        place(buf, voices * 0.35, bar * 4 * beat)
        place(buf, bell(g * 4 * 2 ** (chord[0] / 12), 4.0, 1.6) * 0.25, bar * 4 * beat)
    buf[:4 * SR] += buf[n:n + 4 * SR]
    write("manor_chapel.wav", buf[:n])



def bt122_shot():
    """A pistol shot in a brick room: a crack of bright noise, a low thump
    under it and a short room tail - so slowed down it becomes a boom."""
    n = int(0.9 * SR)
    t = np.arange(n) / SR
    noise = RNG.normal(0, 1, n)
    crack = (noise - lowpass(noise, 0.5)) * np.exp(-t / 0.012)
    thump = np.sin(2 * np.pi * (60 + 140 * np.exp(-t / 0.02)) * t) * np.exp(-t / 0.09)
    tail = lowpass(RNG.normal(0, 1, n), 0.8) * np.exp(-t / 0.25) * 0.25
    write("bt_shot.wav", crack * 0.9 + thump * 0.8 + tail)


def bt122_music():
    """A tense minor riff at 128 bpm, four bars: a pulsing bass on the
    eighths, a kick, and a lead that climbs - a tune, so when the world
    slows and it bends down with it the drop is heard."""
    beat = 60 / 128
    bars = 4
    n = int(bars * 4 * beat * SR)
    buf = np.zeros(n + SR)
    m = int(0.3 * SR)
    tk = np.arange(m) / SR
    kick = np.sin(2 * np.pi * (48 + 120 * np.exp(-tk / 0.035)) * tk) * np.exp(-tk / 0.16)
    roots = [55.0, 55.0, 43.65, 49.0]          # A, A, F, G
    lead = [440.0, 523.3, 659.3, 587.3, 523.3, 493.9, 440.0, 392.0,
            349.2, 440.0, 523.3, 440.0, 392.0, 493.9, 587.3, 659.3]
    for bar in range(bars):
        for b in range(4):
            at = (bar * 4 + b) * beat
            place(buf, kick * 0.9, at)
            for half in (0.0, 0.5):
                f = roots[bar] * 2
                q = int(beat * 0.45 * SR)
                t = np.arange(q) / SR
                tone = np.sign(np.sin(2 * np.pi * f * t)) * 0.3 + np.sin(2 * np.pi * f * t) * 0.7
                place(buf, lowpass(tone, 0.8) * np.exp(-t / 0.12) * 0.45, at + half * beat)
            f = lead[(bar * 4 + b) % len(lead)]
            q = int(beat * 0.95 * SR)
            t = np.arange(q) / SR
            vib = 1 + 0.004 * np.sin(2 * np.pi * 5.5 * t)
            tone = np.sin(2 * np.pi * f * vib * t) + 0.35 * np.sin(4 * np.pi * f * vib * t)
            place(buf, tone * np.minimum(1, t / 0.02) * np.exp(-t / 0.5) * 0.28, at)
    buf[:SR] += buf[n:n + SR]
    write("bt_music.wav", buf[:n])



def bh132_music():
    """A club loop at 124 bpm, eight bars - four on the floor, a clap on two
    and four, open hats on the off-beat, an off-beat bass and a chord stab
    that changes every bar: whole bars, so it loops on a downbeat, which is
    what the Beat Hall's `beat_clock` counts from."""
    bpm = 124
    beat = 60 / bpm
    bars = 8
    n = int(round(bars * 4 * beat * SR))
    buf = np.zeros(n + SR)
    m = int(0.32 * SR)
    tk = np.arange(m) / SR
    kick = np.sin(2 * np.pi * (46 + 140 * np.exp(-tk / 0.03)) * tk) * np.exp(-tk / 0.2)

    def clap():
        q = int(0.22 * SR)
        t = np.arange(q) / SR
        noise = RNG.normal(0, 1, q)
        body = noise - lowpass(noise, 0.5)
        # Three quick hits a few ms apart, as hands are.
        env = sum(np.exp(-np.clip(t - d, 0, None) / 0.012) * (t >= d) for d in (0.0, 0.008, 0.017))
        return body * (env * 0.5 + np.exp(-t / 0.08) * 0.4) * 0.5

    def hat(open_=False):
        q = int((0.16 if open_ else 0.05) * SR)
        noise = RNG.normal(0, 1, q)
        return (noise - lowpass(noise, 0.25)) * np.exp(-np.arange(q) / SR / (0.06 if open_ else 0.012)) * 0.22

    roots = [55.0, 55.0, 43.65, 49.0, 55.0, 55.0, 41.2, 49.0]      # A A F G A A E G
    chords = [(220.0, 261.6, 329.6), (220.0, 261.6, 329.6), (174.6, 220.0, 261.6), (196.0, 246.9, 293.7),
              (220.0, 261.6, 329.6), (220.0, 277.2, 329.6), (164.8, 207.7, 246.9), (196.0, 246.9, 293.7)]
    for bar in range(bars):
        for b in range(4):
            at = (bar * 4 + b) * beat
            place(buf, kick, at)
            if b in (1, 3):
                place(buf, clap(), at)
            place(buf, hat(), at)
            place(buf, hat(open_=True), at + beat / 2)
            # The bass on the off-beat, an octave jump on the last.
            f = roots[bar] * (2 if b == 3 else 1)
            q = int(beat * 0.45 * SR)
            t = np.arange(q) / SR
            tone = np.sign(np.sin(2 * np.pi * f * t)) * 0.35 + np.sin(2 * np.pi * f * t)
            place(buf, lowpass(tone, 0.82) * np.minimum(1, t / 0.005) * np.exp(-t / 0.14) * 0.5, at + beat / 2)
        # A stab on the bar's first beat and the "and" of its third.
        for off in (0.0, 2.5):
            q = int(beat * 0.6 * SR)
            t = np.arange(q) / SR
            stab = sum(np.sign(np.sin(2 * np.pi * f * t)) * 0.3 + np.sin(2 * np.pi * f * 1.003 * t)
                       for f in chords[bar])
            place(buf, lowpass(stab, 0.6) * np.minimum(1, t / 0.004) * np.exp(-t / 0.16) * 0.22,
                  (bar * 4 + off) * beat)
    buf[:SR] += buf[n:n + SR]
    write("bh_club.wav", buf[:n])


def thunder():
    """156 Thunderhead: a clap - a sharp crack, then a rolling rumble that
    swells and dies over five seconds. Played at a pitch the script varies
    (`Audio.set_variation`), so one clip is many claps."""
    dur = 5.5
    n = int(dur * SR)
    t = np.arange(n) / SR
    noise = RNG.standard_normal(n)
    crack = lowpass(noise, 0.55) * np.exp(-t / 0.08)
    # The rumble: dark noise, swelling in a few bumps as the sound arrives
    # from further along the bolt.
    deep = lowpass(lowpass(noise, 0.985), 0.97)
    swell = np.zeros(n)
    for at, size in ((0.15, 1.0), (0.7, 0.8), (1.4, 0.6), (2.3, 0.4)):
        swell += size * np.clip((t - at) / 0.25, 0, 1) * np.exp(-np.clip(t - at, 0, None) / 1.1)
    rumble = deep * swell * np.minimum(1.0, (dur - t) / 0.8)
    x = 0.6 * crack / (np.max(np.abs(crack)) or 1) + rumble / (np.max(np.abs(rumble)) or 1)
    write("th_thunder.wav", x)


def heavy_rain():
    """156 Thunderhead: rain on the sea, a seamless loop."""
    n = int(6.0 * SR)
    hiss = lowpass(RNG.standard_normal(n), 0.35)
    body = lowpass(RNG.standard_normal(n), 0.92) * 0.6
    # Patters: a scatter of tiny ticks.
    ticks = np.zeros(n)
    for at in RNG.integers(0, n - 200, 2400):
        ticks[at:at + 60] += RNG.uniform(0.2, 0.6) * np.exp(-np.arange(60) / 9.0) * RNG.choice([-1, 1])
    x = hiss * 0.5 + body + lowpass(ticks, 0.4) * 0.8
    write("th_rain.wav", seamless(x))


if __name__ == "__main__":
    market_music()
    grill_sizzle()
    fountain_water()
    wind_chimes()
    coin()
    thunder()
    heavy_rain()
    cove_waves()
    cove_drips()
    cove_gulls()
    cove_music()
    cove_splash()
    forge_clang()
    forge_fire()
    forge_hiss()
    forge_bellows()
    stage_beat()
    canyon_shout()
    canyon_step()
    canyon_radio()
    canyon_wind()
    nd88_drive()
    manor_hall()
    manor_library()
    manor_ballroom()
    manor_chapel()
    bt122_shot()
    bt122_music()
    bh132_music()
