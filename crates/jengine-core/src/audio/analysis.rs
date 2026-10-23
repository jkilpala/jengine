//! What the speakers are playing, as numbers, and sounds made from numbers
//! (Oct 2026): a **spectrum** of the mix (`Audio.get_spectrum`,
//! `Audio.get_level`) and **synthesised** notes (`Audio.synth`) - Godot's
//! `AudioEffectSpectrumAnalyzer` and `AudioStreamGenerator`, Unity's
//! `GetSpectrumData` and `OnAudioFilterRead`.
//!
//! # The spectrum
//!
//! Every runtime feeds the same ring of mono samples ([`feed`]) - natively a
//! kira effect on the master track copies what it plays, in the browsers
//! the backend pumps an `AnalyserNode`'s time-domain data each frame - and
//! the analysis is done here, once: the last [`WINDOW`] samples under a Hann
//! window, a radix-2 FFT, the magnitudes summed into bands spaced evenly on
//! a log scale from 40 Hz to 16 kHz, in decibels mapped -70..0 dB to 0..1,
//! rising at once and falling back over a fifth of a second. So a band's
//! number is the same in every runtime for the same sound.
//!
//! # Synthesis
//!
//! [`Voice`] is a note - a waveform (sine, square, saw, triangle, noise) at
//! a frequency that may slide, under an attack / decay / sustain / release
//! envelope - rendered to samples by [`Voice::render`]. `Audio.synth` queues
//! one ([`request`]); [`super::music::drive`], which every host calls once a
//! frame with its backend, plays what is queued through
//! `MusicOut::play_samples` - so no host is wired for either half.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde_json::Value;

/// How many samples the spectrum looks at: 2048 at 48 kHz is 43 ms.
pub const WINDOW: usize = 2048;

/// The ring every runtime feeds, and the sample rate it is fed at.
struct Ring {
    samples: VecDeque<f32>,
    rate: u32,
}

static RING: Mutex<Ring> = Mutex::new(Ring { samples: VecDeque::new(), rate: 48_000 });

/// Hand the analyser samples the speakers are playing (mono, -1..1) at
/// `rate`. The audio thread calls it natively, so it never waits: a busy
/// ring drops the block.
pub fn feed(samples: impl IntoIterator<Item = f32>, rate: u32) {
    let Ok(mut r) = RING.try_lock() else { return };
    r.rate = rate.max(1);
    r.samples.extend(samples);
    let excess = r.samples.len().saturating_sub(WINDOW * 2);
    r.samples.drain(..excess);
}

/// Forget what was fed (a scene change, a test).
pub fn clear() {
    if let Ok(mut r) = RING.lock() {
        r.samples.clear();
    }
    SMOOTH.with(|s| s.borrow_mut().clear());
}

/// The last `n` samples (fewer if fewer were fed) and the rate.
fn last(n: usize) -> (Vec<f32>, u32) {
    let r = RING.lock().unwrap_or_else(|p| p.into_inner());
    let skip = r.samples.len().saturating_sub(n);
    (r.samples.iter().skip(skip).copied().collect(), r.rate)
}

/// The mix's loudness now: the RMS of the last 1024 samples, 0..1.
pub fn level() -> f32 {
    let (s, _) = last(1024);
    if s.is_empty() {
        return 0.0;
    }
    (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt().min(1.0)
}

/// An in-place radix-2 FFT of `re` / `im` (lengths a power of two).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (xr, xi) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

/// The magnitude spectrum of `samples` (zero-padded to [`WINDOW`]) under a
/// Hann window: [`WINDOW`] / 2 bins, bin `k` at `k * rate / WINDOW` Hz,
/// scaled so a full-scale sine reads about 1 in its bin.
pub fn magnitudes(samples: &[f32]) -> Vec<f32> {
    let mut re = vec![0.0f32; WINDOW];
    let mut im = vec![0.0f32; WINDOW];
    let n = samples.len().min(WINDOW);
    for (i, s) in samples[samples.len() - n..].iter().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n.max(2) - 1) as f32).cos();
        re[i] = s * w;
    }
    fft(&mut re, &mut im);
    // A Hann window keeps half the energy: 4 / N brings a sine to 1.
    let scale = 4.0 / WINDOW as f32;
    (0..WINDOW / 2).map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt() * scale).collect()
}

/// `count` bands from `mags`, spaced evenly on a log scale from 40 Hz to
/// 16 kHz, each the loudest bin in it, as -70..0 dB mapped to 0..1.
pub fn bands_of(mags: &[f32], rate: u32, count: usize) -> Vec<f32> {
    let count = count.clamp(1, 256);
    let (lo, hi) = (40.0f32, 16_000.0f32.min(rate as f32 / 2.0));
    let bin_hz = rate as f32 / WINDOW as f32;
    (0..count)
        .map(|b| {
            let f0 = lo * (hi / lo).powf(b as f32 / count as f32);
            let f1 = lo * (hi / lo).powf((b + 1) as f32 / count as f32);
            let k0 = (f0 / bin_hz).floor() as usize;
            let k1 = ((f1 / bin_hz).ceil() as usize).max(k0 + 1).min(mags.len());
            let m = mags.get(k0..k1).map_or(0.0, |s| s.iter().copied().fold(0.0, f32::max));
            let db = 20.0 * m.max(1e-6).log10();
            ((db + 70.0) / 70.0).clamp(0.0, 1.0)
        })
        .collect()
}

thread_local! {
    /// Each band count's last answer, for the fall-off.
    static SMOOTH: std::cell::RefCell<std::collections::HashMap<usize, (f64, Vec<f32>)>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// `count` bands of what is playing now, 0..1, low to high: up at once,
/// down by at most five a second, so a beat is seen and does not flicker.
pub fn spectrum(count: usize) -> Vec<f32> {
    let (s, rate) = last(WINDOW);
    let raw = if s.is_empty() { vec![0.0; count.clamp(1, 256)] } else { bands_of(&magnitudes(&s), rate, count) };
    let now = crate::profiling::unix_time();
    SMOOTH.with(|m| {
        let mut m = m.borrow_mut();
        let (then, prev) = m.entry(raw.len()).or_insert_with(|| (now, raw.clone()));
        let fall = ((now - *then) as f32 * 5.0).max(0.0);
        for (p, r) in prev.iter_mut().zip(&raw) {
            *p = if *r >= *p { *r } else { (*p - fall).max(*r) };
        }
        *then = now;
        prev.clone()
    })
}

/// A waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wave {
    #[default]
    Sine,
    Square,
    Saw,
    Triangle,
    Noise,
}

/// A synthesised note.
#[derive(Debug, Clone, PartialEq)]
pub struct Voice {
    pub wave: Wave,
    /// Hz at the start...
    pub freq: f32,
    /// ...and at the end (a slide); `None` holds `freq`.
    pub freq_end: Option<f32>,
    /// How long it is held before its release, in seconds.
    pub duration: f32,
    pub attack: f32,
    pub decay: f32,
    /// The level held after the decay, 0..1.
    pub sustain: f32,
    pub release: f32,
    pub volume: f32,
    /// The mixer bus it plays on.
    pub bus: String,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            wave: Wave::Sine,
            freq: 440.0,
            freq_end: None,
            duration: 0.3,
            attack: 0.01,
            decay: 0.08,
            sustain: 0.7,
            release: 0.2,
            volume: 0.5,
            bus: "sfx".into(),
        }
    }
}

/// Twelve-tone equal temperament: MIDI note `n` (69 is A4) in Hz.
pub fn note_hz(n: f32) -> f32 {
    440.0 * 2f32.powf((n - 69.0) / 12.0)
}

impl Voice {
    /// The settings from a script's table - `{ wave = "saw", freq = 220,
    /// note = 57, slide = 110, duration = 0.4, attack, decay, sustain,
    /// release, volume, bus }`; `note` (MIDI) wins over `freq`. Anything
    /// left out is the default; an unknown wave is logged and sine.
    pub fn from_json(v: &Value) -> Self {
        let mut s = Self::default();
        let Some(o) = v.as_object() else { return s };
        let num = |k: &str| o.get(k).and_then(Value::as_f64).map(|n| n as f32);
        if let Some(w) = o.get("wave").and_then(Value::as_str) {
            s.wave = match w {
                "sine" => Wave::Sine,
                "square" => Wave::Square,
                "saw" => Wave::Saw,
                "triangle" => Wave::Triangle,
                "noise" => Wave::Noise,
                other => {
                    log::warn!("[synth] unknown wave '{other}' - sine, square, saw, triangle or noise");
                    Wave::Sine
                }
            };
        }
        if let Some(f) = num("freq") {
            s.freq = f;
        }
        if let Some(n) = num("note") {
            s.freq = note_hz(n);
        }
        s.freq_end = num("slide");
        for (k, field) in [
            ("duration", &mut s.duration),
            ("attack", &mut s.attack),
            ("decay", &mut s.decay),
            ("sustain", &mut s.sustain),
            ("release", &mut s.release),
            ("volume", &mut s.volume),
        ] {
            if let Some(n) = num(k) {
                *field = n;
            }
        }
        if let Some(b) = o.get("bus").and_then(Value::as_str) {
            s.bus = b.to_string();
        }
        s
    }

    /// The envelope at `t` seconds: up over `attack`, down to `sustain` over
    /// `decay`, held until `duration`, then down to nothing over `release`.
    pub fn envelope(&self, t: f32) -> f32 {
        let (a, d, s, r) = (self.attack.max(1e-4), self.decay.max(1e-4), self.sustain.clamp(0.0, 1.0), self.release.max(1e-4));
        let held = self.duration.max(0.0);
        let level_at = |t: f32| {
            if t < a {
                t / a
            } else if t < a + d {
                1.0 - (1.0 - s) * (t - a) / d
            } else {
                s
            }
        };
        if t <= held {
            level_at(t)
        } else {
            (level_at(held) * (1.0 - (t - held) / r)).max(0.0)
        }
    }

    /// How long it sounds, release included.
    pub fn length(&self) -> f32 {
        self.duration.max(0.0) + self.release.max(0.0)
    }

    /// Its samples, mono at `rate` - at most 10 seconds.
    pub fn render(&self, rate: u32) -> Vec<f32> {
        let n = ((self.length().min(10.0)) * rate as f32).ceil() as usize;
        let mut out = Vec::with_capacity(n);
        let (mut phase, mut seed) = (0.0f32, 0x2545_f491u32);
        let total = self.length().max(1e-4);
        for i in 0..n {
            let t = i as f32 / rate as f32;
            let f = match self.freq_end {
                Some(end) => self.freq * (end / self.freq.max(1e-3)).powf((t / total).min(1.0)),
                None => self.freq,
            };
            phase = (phase + f / rate as f32).fract();
            let s = match self.wave {
                Wave::Sine => (phase * std::f32::consts::TAU).sin(),
                Wave::Square => if phase < 0.5 { 1.0 } else { -1.0 },
                Wave::Saw => 2.0 * phase - 1.0,
                Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
                Wave::Noise => {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as f32 / u32::MAX as f32 * 2.0 - 1.0
                }
            };
            out.push(s * self.envelope(t) * self.volume.clamp(0.0, 1.0));
        }
        out
    }
}

/// Voices asked for and not yet played.
static QUEUE: Mutex<Vec<Voice>> = Mutex::new(Vec::new());

/// Queue a note; the next [`super::music::drive`] plays it.
pub fn request(v: Voice) {
    let mut q = QUEUE.lock().unwrap_or_else(|p| p.into_inner());
    if q.len() < 64 {
        q.push(v);
    }
}

/// Take what is queued.
pub fn take() -> Vec<Voice> {
    std::mem::take(&mut *QUEUE.lock().unwrap_or_else(|p| p.into_inner()))
}

/// Replace the ring with the latest window - the browsers', whose
/// `AnalyserNode` hands over the last [`WINDOW`] samples each frame rather
/// than the new ones.
pub fn set_window(samples: &[f32], rate: u32) {
    let Ok(mut r) = RING.lock() else { return };
    r.rate = rate.max(1);
    r.samples.clear();
    r.samples.extend(samples.iter().copied());
}

/// The rate voices are rendered at.
pub const SYNTH_RATE: u32 = 44_100;

impl Voice {
    /// The clip a voice is loaded as - one per distinct note, so a melody
    /// of eight notes loads eight clips once and plays them from then on.
    pub fn clip_name(&self) -> String {
        let key = format!("{:?}", self);
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in key.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("__synth/{h:016x}")
    }
}

/// Mono samples as a 16-bit PCM WAV file in memory - how a voice reaches a
/// backend that loads clips from bytes.
pub fn wav_bytes(samples: &[f32], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}
