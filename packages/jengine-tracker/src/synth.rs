//! The `.song` player: a deterministic sample loop.
//!
//! A port of jSuite's `src/lib/tracker/synth.ts`, kept in its order and
//! under its names so the two can be read side by side - the editor
//! previews what the game plays only while they agree, and the digests in
//! `tests/data/` (made by `synth.ts`) are what holds them to it. If a
//! window there fails, the cause is almost always an ordering difference -
//! which tick a thing happens on, or a value read before or after it
//! changes - rather than a float one.
//!
//! Per tick: tick 0 reads the row, later ticks run the continuous effects;
//! the tick's frame count is `floor(pos + spt) - floor(pos)`; frames are
//! rendered in control blocks of up to [`CONTROL`] frames, each recomputing
//! every channel's frequency and pan gains first. The block and tick
//! counters live here, never in the caller's buffer size.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use crate::song::{Cell, Instrument, MAX_VOLUME, NOTE_CUT, NOTE_OFF, Song, Wave};

/// Frames per control block - how often pitch and gain targets update.
pub const CONTROL: u32 = 32;
/// Time constant of the click-free gain smoothing, seconds.
const SMOOTH_SECONDS: f64 = 0.004;
/// Shortest envelope segment, seconds - anything shorter clicks.
const MIN_SEGMENT: f64 = 0.001;
/// Release used by note cuts (`^^^`, `ECx`).
const CUT_RELEASE: f64 = 0.004;
/// DC blocker corner, Hz - pulse waves below 50% carry a DC offset.
const DC_HZ: f64 = 10.0;
/// Longest tail rendered after a song with no loop ends, seconds.
const MAX_TAIL: f64 = 4.0;
/// Longest one-pass render, seconds - a song that never repeats a row.
const MAX_SONG: f64 = 20.0 * 60.0;
/// Where the soft limiter starts bending.
const KNEE: f64 = 0.8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

pub fn note_hz(semitones: f64) -> f64 {
    440.0 * 2f64.powf((semitones - 69.0) / 12.0)
}

/// Linear to `KNEE`, then a tanh curve that meets 1 smoothly - a hard clamp
/// turns four loud channels into square-wave fuzz.
fn limit(x: f64) -> f64 {
    if x > KNEE {
        KNEE + (1.0 - KNEE) * ((x - KNEE) / (1.0 - KNEE)).tanh()
    } else if x < -KNEE {
        -KNEE - (1.0 - KNEE) * ((-x - KNEE) / (1.0 - KNEE)).tanh()
    } else {
        x
    }
}

/// PolyBLEP residual for a discontinuity at phase 0.
fn blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[derive(Debug, Clone)]
struct Channel {
    inst: Option<usize>,
    /// Current pitch in semitones, slides included.
    pitch: f64,
    porta_target: Option<f64>,
    porta_speed: f64,
    vol: f64,
    pan: f64,
    duty: f64,

    // Per-row effect state, reset as each row is read.
    arp_x: f64,
    arp_y: f64,
    arp_on: bool,
    slide: f64,
    porta: bool,
    vol_slide: f64,
    vib_on: bool,
    vib_speed: u32,
    vib_depth: f64,
    vib_phase: u32,
    cut_tick: i64,
    delay_tick: i64,
    delayed: Option<Cell>,

    // Voice state.
    stage: Stage,
    env: f64,
    env_step: f64,
    phase: f64,
    lfsr: u32,
    lp: f64,
    lp_coef: f64,
    note_frames: u64,
    note_ticks: i64,
    /// Frequency for the current control block.
    hz: f64,
    gain_l: f64,
    gain_r: f64,
    target_l: f64,
    target_r: f64,
    /// Started from silence: jump the gains to their targets instead of
    /// gliding, or every drum hit is blunted by a few milliseconds.
    snap: bool,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            inst: None,
            pitch: 60.0,
            porta_target: None,
            porta_speed: 0.0,
            vol: MAX_VOLUME,
            pan: 0.0,
            duty: 0.5,
            arp_x: 0.0,
            arp_y: 0.0,
            arp_on: false,
            slide: 0.0,
            porta: false,
            vol_slide: 0.0,
            vib_on: false,
            vib_speed: 0,
            vib_depth: 0.0,
            vib_phase: 0,
            cut_tick: -1,
            delay_tick: -1,
            delayed: None,
            stage: Stage::Idle,
            env: 0.0,
            env_step: 0.0,
            phase: 0.0,
            lfsr: 1,
            lp: 0.0,
            lp_coef: 1.0,
            note_frames: 0,
            note_ticks: 0,
            hz: 0.0,
            gain_l: 0.0,
            gain_r: 0.0,
            target_l: 0.0,
            target_r: 0.0,
            snap: false,
        }
    }
}

/// Plays one pass of a song into caller-supplied buffers: up to the first
/// row it would play a second time (the loop point), or for a song that
/// ends, until its last notes have died away.
pub struct Sequencer<'a> {
    rate: f64,
    song: &'a Song,
    chans: Vec<Channel>,

    bpm: f64,
    speed: f64,
    order: usize,
    row: usize,
    tick: u32,
    /// The tick being rendered now (`tick` has already moved past it).
    play_tick: u32,
    /// Frames left in the current tick.
    tick_left: u64,
    ctrl_left: u64,
    /// Exact song position in frames, fractional, for tick boundaries.
    tick_pos: f64,
    /// Frames rendered so far.
    pub frame: u64,

    jump_order: Option<usize>,
    break_row: Option<usize>,

    /// No more rows - rendering the tail of the last notes.
    ending: bool,
    tail_frames: u64,
    /// Nothing more to render.
    pub done: bool,
    /// The frame where the repeated row first played.
    pub loop_frame: Option<u64>,
    visited: HashMap<usize, u64>,

    smooth: f64,
    dc_r: f64,
    dc_in_l: f64,
    dc_in_r: f64,
    dc_out_l: f64,
    dc_out_r: f64,
}

impl<'a> Sequencer<'a> {
    pub fn new(song: &'a Song, rate: u32) -> Self {
        let rate = f64::from(rate);
        Self {
            rate,
            song,
            chans: vec![Channel::default(); song.channels],
            bpm: song.bpm,
            speed: song.speed,
            order: 0,
            row: 0,
            tick: 0,
            play_tick: 0,
            tick_left: 0,
            ctrl_left: 0,
            tick_pos: 0.0,
            frame: 0,
            jump_order: None,
            break_row: None,
            ending: song.order.is_empty(),
            tail_frames: 0,
            done: false,
            loop_frame: None,
            visited: HashMap::new(),
            smooth: 1.0 - (-1.0 / (SMOOTH_SECONDS * rate)).exp(),
            dc_r: 1.0 - (TAU * DC_HZ) / rate,
            dc_in_l: 0.0,
            dc_in_r: 0.0,
            dc_out_l: 0.0,
            dc_out_r: 0.0,
        }
    }

    /// Render up to `left.len()` frames. Returns how many were written -
    /// fewer only once the song has finished.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) -> usize {
        let frames = left.len().min(right.len());
        let master = self.song.volume;
        let mut i = 0;
        while i < frames {
            if self.tick_left == 0 {
                if self.done {
                    break;
                }
                self.start_tick();
                if self.done {
                    break;
                }
            }
            if self.ctrl_left == 0 {
                self.ctrl_left = u64::from(CONTROL).min(self.tick_left);
                self.control();
            }
            let n = (self.ctrl_left as usize).min(frames - i);
            for k in 0..n {
                let mut l = 0.0;
                let mut r = 0.0;
                for c in 0..self.chans.len() {
                    let s = self.sample(c);
                    let ch = &self.chans[c];
                    l += s * ch.gain_l;
                    r += s * ch.gain_r;
                }
                l *= master;
                r *= master;
                // DC blocker: y = x - x1 + R * y1.
                let yl = l - self.dc_in_l + self.dc_r * self.dc_out_l;
                let yr = r - self.dc_in_r + self.dc_r * self.dc_out_r;
                self.dc_in_l = l;
                self.dc_in_r = r;
                self.dc_out_l = yl;
                self.dc_out_r = yr;
                left[i + k] = limit(yl) as f32;
                right[i + k] = limit(yr) as f32;
            }
            i += n;
            self.ctrl_left -= n as u64;
            self.tick_left -= n as u64;
            self.frame += n as u64;
        }
        i
    }

    // ── Sequencing ──────────────────────────────────────────────────

    fn start_tick(&mut self) {
        if self.ending {
            // The tail: keep ticking with no rows until every voice is silent.
            let silent = self.chans.iter().all(|ch| ch.stage == Stage::Idle);
            if silent || self.tail_frames as f64 >= MAX_TAIL * self.rate {
                self.done = true;
                return;
            }
        } else if self.tick == 0 {
            let key = self.order * 1024 + self.row;
            if let Some(&first) = self.visited.get(&key) {
                self.loop_frame = Some(first);
                self.done = true;
                return;
            }
            if self.frame as f64 >= MAX_SONG * self.rate {
                self.done = true;
                return;
            }
            self.visited.insert(key, self.frame);
            self.read_row();
        } else {
            self.tick_effects();
        }

        for ch in &mut self.chans {
            if ch.stage != Stage::Idle {
                ch.note_ticks += 1;
            }
        }
        self.play_tick = if self.ending { self.play_tick + 1 } else { self.tick };

        let spt = (self.rate * 60.0) / (self.bpm * self.song.rows_per_beat * self.speed);
        let before = self.tick_pos.floor();
        self.tick_pos += spt;
        self.tick_left = (self.tick_pos.floor() - before) as u64;
        if self.ending {
            self.tail_frames += self.tick_left;
        }
        self.ctrl_left = 0;

        if !self.ending {
            self.tick += 1;
            if f64::from(self.tick) >= self.speed {
                self.tick = 0;
                self.advance_row();
            }
        }
    }

    fn pattern_rows(&self) -> usize {
        self.song.order.get(self.order).and_then(|&p| self.song.patterns.get(p)).map_or(0, |p| p.rows.len())
    }

    fn advance_row(&mut self) {
        let jump = self.jump_order.take();
        let brk = self.break_row.take();

        if jump.is_some() || brk.is_some() {
            self.order = jump.unwrap_or(self.order + 1);
            self.row = brk.unwrap_or(0);
        } else {
            self.row += 1;
            if self.row < self.pattern_rows() {
                return;
            }
            self.row = 0;
            self.order += 1;
        }

        if self.order >= self.song.order.len() {
            match self.song.loop_to {
                Some(l) if l < self.song.order.len() => self.order = l,
                _ => {
                    self.ending = true;
                    return;
                }
            }
        }
        // A break past the end of the next pattern starts it from the top.
        let rows = self.pattern_rows();
        if rows == 0 {
            self.ending = true;
        } else if self.row >= rows {
            self.row = 0;
        }
    }

    fn read_row(&mut self) {
        let song = self.song;
        let row = song.order.get(self.order).and_then(|&p| song.patterns.get(p)).and_then(|p| p.rows.get(self.row));
        for c in 0..self.chans.len() {
            {
                let ch = &mut self.chans[c];
                ch.arp_on = false;
                ch.slide = 0.0;
                ch.porta = false;
                ch.vol_slide = 0.0;
                ch.vib_on = false;
                ch.cut_tick = -1;
                ch.delay_tick = -1;
                ch.delayed = None;
            }
            let Some(&cell) = row.and_then(|r| r.get(c)) else { continue };

            let x = cell.arg >> 4;
            let y = cell.arg & 15;

            if cell.fx == Some(b'E') && x == 0xd && y > 0 {
                let ch = &mut self.chans[c];
                ch.delay_tick = i64::from(y);
                ch.delayed = Some(cell);
            } else {
                self.trigger(c, &cell);
            }

            let arg = f64::from(cell.arg);
            let ch = &mut self.chans[c];
            match cell.fx {
                Some(b'0') => {
                    if cell.arg != 0 {
                        ch.arp_on = true;
                        ch.arp_x = f64::from(x);
                        ch.arp_y = f64::from(y);
                    }
                }
                Some(b'1') => ch.slide = arg / 16.0,
                Some(b'2') => ch.slide = -arg / 16.0,
                Some(b'3') => {
                    ch.porta = true;
                    if cell.arg != 0 {
                        ch.porta_speed = arg / 16.0;
                    }
                }
                Some(b'4') => {
                    ch.vib_on = true;
                    if x != 0 {
                        ch.vib_speed = x;
                    }
                    if y != 0 {
                        ch.vib_depth = f64::from(y) / 8.0;
                    }
                }
                Some(b'8') => ch.pan = ((arg - 128.0) / 127.0).clamp(-1.0, 1.0),
                Some(b'9') => ch.duty = arg.max(1.0) / 256.0,
                Some(b'A') => ch.vol_slide = if x != 0 { f64::from(x) } else { -f64::from(y) },
                Some(b'B') => self.jump_order = Some(cell.arg as usize),
                Some(b'C') => ch.vol = arg.min(MAX_VOLUME),
                Some(b'D') => self.break_row = Some(cell.arg as usize),
                Some(b'E') => {
                    if x == 0xc {
                        ch.cut_tick = i64::from(y);
                    }
                }
                Some(b'F') => {
                    if cell.arg >= 0x20 {
                        self.bpm = arg;
                    } else if cell.arg > 0 {
                        self.speed = arg;
                    }
                }
                _ => {}
            }
        }
        // ECx with x = 0 cuts at once.
        for c in 0..self.chans.len() {
            if self.chans[c].cut_tick == 0 {
                self.cut(c);
            }
        }
    }

    fn tick_effects(&mut self) {
        let tick = i64::from(self.tick);
        for c in 0..self.chans.len() {
            if self.chans[c].delay_tick == tick
                && let Some(cell) = self.chans[c].delayed.take()
            {
                self.trigger(c, &cell);
            }
            if self.chans[c].cut_tick == tick {
                self.cut(c);
            }
            let ch = &mut self.chans[c];
            if ch.slide != 0.0 {
                ch.pitch = (ch.pitch + ch.slide).min(140.0).max(0.0);
            }
            if ch.porta
                && let Some(target) = ch.porta_target
            {
                ch.pitch = if ch.pitch < target {
                    target.min(ch.pitch + ch.porta_speed)
                } else {
                    target.max(ch.pitch - ch.porta_speed)
                };
            }
            if ch.vol_slide != 0.0 {
                ch.vol = (ch.vol + ch.vol_slide).min(MAX_VOLUME).max(0.0);
            }
            if ch.vib_on {
                ch.vib_phase = (ch.vib_phase + ch.vib_speed) & 63;
            }
        }
    }

    fn trigger(&mut self, c: usize, cell: &Cell) {
        let rate = self.rate;
        let song = self.song;
        let ch = &mut self.chans[c];
        if let Some(i) = cell.inst
            && let Some(inst) = song.instruments.get(i)
        {
            ch.inst = Some(i);
            ch.vol = MAX_VOLUME;
            ch.duty = inst.duty;
            ch.lp_coef = if inst.cutoff > 0.0 { 1.0 - ((-TAU * inst.cutoff) / rate).exp() } else { 1.0 };
        }
        if let Some(v) = cell.vol {
            ch.vol = v;
        }

        let Some(note) = cell.note else { return };
        if note == NOTE_OFF {
            let release = ch.inst.map_or(0.0, |i| song.instruments[i].release);
            self.release(c, release);
            return;
        }
        if note == NOTE_CUT {
            self.cut(c);
            return;
        }
        let Some(inst) = ch.inst.map(|i| &song.instruments[i]) else { return };
        let pitch = f64::from(note) + inst.transpose;
        if cell.fx == Some(b'3') && ch.stage != Stage::Idle {
            ch.porta_target = Some(pitch);
            return;
        }
        ch.pitch = pitch;
        ch.porta_target = Some(pitch);
        ch.phase = 0.0;
        ch.lfsr = 1;
        ch.vib_phase = 0;
        ch.note_frames = 0;
        ch.note_ticks = 0;
        if ch.stage == Stage::Idle {
            ch.snap = true;
        }
        // Attack from wherever the envelope is, so a retrigger does not click.
        ch.stage = Stage::Attack;
        ch.env_step = 1.0 / (inst.attack.max(MIN_SEGMENT) * rate);
    }

    fn release(&mut self, c: usize, seconds: f64) {
        let rate = self.rate;
        let ch = &mut self.chans[c];
        if ch.stage == Stage::Idle {
            return;
        }
        ch.stage = Stage::Release;
        ch.env_step = ch.env / (seconds.max(MIN_SEGMENT) * rate);
    }

    fn cut(&mut self, c: usize) {
        self.release(c, CUT_RELEASE);
    }

    /// Once per control block: frequency and pan gains for every channel.
    fn control(&mut self) {
        let play_tick = self.play_tick;
        let rate = self.rate;
        let song = self.song;
        for ch in &mut self.chans {
            let inst = match ch.inst {
                Some(i) if ch.stage != Stage::Idle => &song.instruments[i],
                _ => {
                    ch.target_l = 0.0;
                    ch.target_r = 0.0;
                    continue;
                }
            };
            let mut p = ch.pitch;
            if ch.arp_on {
                p += match play_tick % 3 {
                    1 => ch.arp_x,
                    2 => ch.arp_y,
                    _ => 0.0,
                };
            }
            if ch.vib_on {
                p += ch.vib_depth * ((TAU * f64::from(ch.vib_phase)) / 64.0).sin();
            }
            if !inst.arp.is_empty() {
                p += inst.arp[(ch.note_ticks - 1).rem_euclid(inst.arp.len() as i64) as usize];
            }
            let t = ch.note_frames as f64 / rate;
            if inst.slide != 0.0 {
                p += inst.slide * t;
            }
            if let Some(v) = inst.vibrato
                && v.depth > 0.0
                && t >= v.delay
            {
                p += v.depth * (TAU * v.rate * (t - v.delay)).sin();
            }
            ch.hz = note_hz(p.min(140.0).max(-24.0));

            let gain = inst.volume * (ch.vol / MAX_VOLUME);
            let pan = (inst.pan + ch.pan).min(1.0).max(-1.0);
            let angle = ((pan + 1.0) * PI) / 4.0;
            ch.target_l = gain * angle.cos();
            ch.target_r = gain * angle.sin();
            if ch.snap {
                ch.gain_l = ch.target_l;
                ch.gain_r = ch.target_r;
                ch.snap = false;
            }
        }
    }

    /// One frame of one channel, before pan: oscillator times envelope, filtered.
    fn sample(&mut self, c: usize) -> f64 {
        let smooth = self.smooth;
        let rate = self.rate;
        let song = self.song;
        let ch = &mut self.chans[c];
        ch.gain_l += (ch.target_l - ch.gain_l) * smooth;
        ch.gain_r += (ch.target_r - ch.gain_r) * smooth;
        let inst: &Instrument = match ch.inst {
            Some(i) if ch.stage != Stage::Idle => &song.instruments[i],
            _ => return 0.0,
        };

        // Envelope, linear segments.
        match ch.stage {
            Stage::Attack => {
                ch.env += ch.env_step;
                if ch.env >= 1.0 {
                    ch.env = 1.0;
                    ch.stage = Stage::Decay;
                    ch.env_step = (1.0 - inst.sustain) / (inst.decay.max(MIN_SEGMENT) * rate);
                }
            }
            Stage::Decay => {
                ch.env -= ch.env_step;
                if ch.env <= inst.sustain {
                    ch.env = inst.sustain;
                    ch.stage = if inst.sustain > 0.0 { Stage::Sustain } else { Stage::Idle };
                }
            }
            Stage::Release => {
                ch.env -= ch.env_step;
                if ch.env <= 0.0 {
                    ch.env = 0.0;
                    ch.stage = Stage::Idle;
                }
            }
            Stage::Sustain | Stage::Idle => {}
        }

        // Oscillator.
        let dt = ch.hz / rate;
        let ph = ch.phase;
        let s = match inst.wave {
            Wave::Pulse => {
                let duty = ch.duty;
                let mut s = if ph < duty { 1.0 } else { -1.0 };
                s += blep(ph, dt);
                let mut t2 = ph - duty;
                if t2 < 0.0 {
                    t2 += 1.0;
                }
                s - blep(t2, dt)
            }
            Wave::Triangle => {
                if ph < 0.5 {
                    4.0 * ph - 1.0
                } else {
                    3.0 - 4.0 * ph
                }
            }
            Wave::Saw => 2.0 * ph - 1.0 - blep(ph, dt),
            Wave::Sine => (TAU * ph).sin(),
            Wave::Wave => inst.table[((ph * 32.0).floor() as usize).min(31)] / 7.5 - 1.0,
            Wave::Noise => {
                if ch.lfsr & 1 != 0 {
                    1.0
                } else {
                    -1.0
                }
            }
        };

        if inst.wave == Wave::Noise {
            // The LFSR is clocked at 16x the note: high notes hiss, low ones rumble.
            ch.phase += dt * 16.0;
            let tap = if inst.periodic { 6 } else { 1 };
            while ch.phase >= 1.0 {
                ch.phase -= 1.0;
                let bit = (ch.lfsr ^ (ch.lfsr >> tap)) & 1;
                ch.lfsr = (ch.lfsr >> 1) | (bit << 14);
            }
        } else {
            ch.phase += dt;
            if ch.phase >= 1.0 {
                ch.phase -= ch.phase.floor();
            }
        }

        ch.note_frames += 1;
        let mut out = s * ch.env;
        if ch.lp_coef < 1.0 {
            ch.lp += (out - ch.lp) * ch.lp_coef;
            out = ch.lp;
        }
        out
    }
}

/// One pass of a song, as PCM.
#[derive(Debug, Clone)]
pub struct Rendered {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub rate: u32,
    /// The frame the loop goes back to, or `None` for a song that ends.
    pub loop_start: Option<u64>,
}

/// Render one pass: up to the point it would repeat a row (reported as
/// `loop_start`) or to the end of its last notes. Play it, and loop from
/// `loop_start`.
pub fn render(song: &Song, rate: u32) -> Rendered {
    let mut seq = Sequencer::new(song, rate);
    const CHUNK: usize = 1 << 16;
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut l = vec![0f32; CHUNK];
    let mut r = vec![0f32; CHUNK];
    while !seq.done {
        let n = seq.render(&mut l, &mut r);
        if n == 0 {
            break;
        }
        left.extend_from_slice(&l[..n]);
        right.extend_from_slice(&r[..n]);
    }
    Rendered { left, right, rate, loop_start: seq.loop_frame }
}
