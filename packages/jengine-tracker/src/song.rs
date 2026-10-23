//! The `.song` format: a chip-music tracker module, as JSON.
//!
//! A port of jSuite's `src/lib/tracker/song.ts`, rule for rule - the same
//! defaults, the same clamps, the same errors - so a file jSuite opens is a
//! file the engine plays. `FORMAT.md` beside this crate is the spec.
//!
//! Patterns are tracker text, one string per row, cells joined by `|`:
//! `"C-4 01 40 ... | ... .. .. ... | E-2 03 .. 0F2"` - `NOTE INST VOL FX`.

use serde_json::{Map, Value};
use std::fmt;

pub const NOTE_OFF: i32 = -1;
pub const NOTE_CUT: i32 = -2;
pub const MAX_CHANNELS: usize = 16;
pub const MAX_ROWS: usize = 256;
/// Channel volume runs 0..=64 (`0x40`).
pub const MAX_VOLUME: f64 = 64.0;
pub const FORMAT_VERSION: f64 = 1.0;

/// The effect commands the player understands. Anything else is a warning
/// and is ignored at playback.
pub const EFFECTS: &[u8] = b"0123489ABCDEF";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wave {
    Pulse,
    Triangle,
    Saw,
    Sine,
    Noise,
    Wave,
}

impl Wave {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "pulse" => Wave::Pulse,
            "triangle" => Wave::Triangle,
            "saw" => Wave::Saw,
            "sine" => Wave::Sine,
            "noise" => Wave::Noise,
            "wave" => Wave::Wave,
            _ => return None,
        })
    }
}

/// One cell of one row.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Cell {
    /// MIDI note (C-4 = 60), [`NOTE_OFF`], [`NOTE_CUT`], or none.
    pub note: Option<i32>,
    /// 0-based instrument index (written 1-based).
    pub inst: Option<usize>,
    /// 0..=64.
    pub vol: Option<f64>,
    /// Effect command, as its upper-case character.
    pub fx: Option<u8>,
    /// Effect parameter, 0..=255.
    pub arg: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    /// `rows[row][channel]`, every row as wide as the song.
    pub rows: Vec<Vec<Cell>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vibrato {
    /// Peak deviation in semitones.
    pub depth: f64,
    /// Hz.
    pub rate: f64,
    /// Seconds after the note starts before it begins.
    pub delay: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instrument {
    pub name: String,
    pub wave: Wave,
    /// Pulse width, 0.01..=0.99.
    pub duty: f64,
    /// `wave` only: 32 steps, each 0..=15.
    pub table: [f64; 32],
    /// `noise` only: the short LFSR tap - metallic, tonal.
    pub periodic: bool,
    pub volume: f64,
    /// Seconds.
    pub attack: f64,
    pub decay: f64,
    /// Level, 0..=1.
    pub sustain: f64,
    pub release: f64,
    /// -1 (left) to 1 (right), added to the channel's `8xx` pan.
    pub pan: f64,
    /// Semitones added to every note.
    pub transpose: f64,
    pub vibrato: Option<Vibrato>,
    /// Pitch drift in semitones a second from note start (a kick is `-150`).
    pub slide: f64,
    /// Semitone offsets cycled one per tick from note start.
    pub arp: Vec<f64>,
    /// One-pole low-pass cutoff in Hz; 0 is off.
    pub cutoff: f64,
}

/// The 4-bit triangle every `wave` instrument starts with.
pub const TRIANGLE_TABLE: [f64; 32] = [
    0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 15.0, 14.0, 13.0, 12.0, 11.0,
    10.0, 9.0, 8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0, 0.0,
];

impl Instrument {
    pub fn new(name: &str, wave: Wave) -> Self {
        Self {
            name: name.to_string(),
            wave,
            duty: 0.5,
            table: TRIANGLE_TABLE,
            periodic: false,
            volume: 0.8,
            attack: 0.002,
            decay: 0.1,
            sustain: 0.7,
            release: 0.08,
            pan: 0.0,
            transpose: 0.0,
            vibrato: None,
            slide: 0.0,
            arp: Vec::new(),
            cutoff: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Song {
    pub name: String,
    /// Musical beats a minute.
    pub bpm: f64,
    pub rows_per_beat: f64,
    /// Ticks a row - the resolution effects run at.
    pub speed: f64,
    pub channels: usize,
    /// Master gain, 0..=1.
    pub volume: f64,
    pub instruments: Vec<Instrument>,
    pub patterns: Vec<Pattern>,
    /// Pattern indices in play order.
    pub order: Vec<usize>,
    /// The order index to go to after the last, or `None` to end.
    pub loop_to: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    pub message: String,
}

/// Every error a document has, collected rather than stopping at the first.
#[derive(Debug, Clone)]
pub struct SongError {
    pub issues: Vec<Issue>,
}

impl fmt::Display for SongError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let errors: Vec<&str> =
            self.issues.iter().filter(|i| i.severity == Severity::Error).map(|i| i.message.as_str()).collect();
        f.write_str(&errors.join("\n"))
    }
}

impl std::error::Error for SongError {}

/// JavaScript's `Math.round`: halves go up, `-2.5` to `-2`.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

fn is_integer(v: &Value) -> bool {
    v.as_f64().is_some_and(|x| x.fract() == 0.0)
}

/// A number clamped to its range, or the fallback for anything that is not one.
fn num(v: Option<&Value>, fallback: f64, min: f64, max: f64) -> f64 {
    match v.and_then(Value::as_f64) {
        Some(x) => x.max(min).min(max),
        None => fallback,
    }
}

fn hex2(n: usize) -> String {
    format!("{n:02X}")
}

// ── Notes ────────────────────────────────────────────────────────

/// `C-4` to 60; also `C4`, `c#4`, `Db4`. `None` if it is not a note.
pub fn parse_note_name(text: &str) -> Option<i32> {
    let b = text.trim().as_bytes();
    let (letter, accidental, octave) = match b {
        [l, o] => (*l, 0u8, *o),
        [l, a, o] => (*l, *a, *o),
        _ => return None,
    };
    let base = match letter.to_ascii_lowercase() {
        b'c' => 0,
        b'd' => 2,
        b'e' => 4,
        b'f' => 5,
        b'g' => 7,
        b'a' => 9,
        b'b' => 11,
        _ => return None,
    };
    let accidental = match accidental {
        0 | b'-' => 0,
        b'#' => 1,
        b'b' => -1,
        _ => return None,
    };
    if !octave.is_ascii_digit() {
        return None;
    }
    let note = (i32::from(octave - b'0') + 1) * 12 + base + accidental;
    (0..=131).contains(&note).then_some(note)
}

// ── Cell text ────────────────────────────────────────────────────

fn is_dots(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c == b'.')
}

fn is_hex(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_hexdigit())
}

/// One cell, with missing trailing fields empty.
pub fn parse_cell(text: &str) -> Result<Cell, String> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.len() > 4 {
        return Err(format!("\"{}\" has more than 4 fields", text.trim()));
    }
    let field = |i: usize, empty: &'static str| parts.get(i).copied().unwrap_or(empty);
    let (n, i, v, f) = (field(0, "..."), field(1, ".."), field(2, ".."), field(3, "..."));
    let mut cell = Cell::default();

    if n == "===" {
        cell.note = Some(NOTE_OFF);
    } else if n == "^^^" {
        cell.note = Some(NOTE_CUT);
    } else if !is_dots(n) {
        cell.note = Some(parse_note_name(n).ok_or_else(|| format!("bad note \"{n}\""))?);
    }

    if !is_dots(i) {
        if !is_hex(i, 1, 2) {
            return Err(format!("bad instrument \"{i}\""));
        }
        let inst = usize::from_str_radix(i, 16).unwrap_or(0);
        if inst == 0 {
            return Err("instrument 00 does not exist - instruments are numbered from 01".into());
        }
        cell.inst = Some(inst - 1);
    }

    if !is_dots(v) {
        if !is_hex(v, 1, 2) {
            return Err(format!("bad volume \"{v}\""));
        }
        let vol = u32::from_str_radix(v, 16).unwrap_or(0) as f64;
        if vol > MAX_VOLUME {
            return Err(format!("volume {v} is above 40"));
        }
        cell.vol = Some(vol);
    }

    if !is_dots(f) {
        let b = f.as_bytes();
        if b.len() != 3 || !b[0].is_ascii_alphanumeric() || !is_hex(&f[1..], 2, 2) {
            return Err(format!("bad effect \"{f}\""));
        }
        cell.fx = Some(b[0].to_ascii_uppercase());
        cell.arg = u32::from_str_radix(&f[1..], 16).unwrap_or(0);
    }
    Ok(cell)
}

/// One row: its cells, padded with empty ones to the song's width.
pub fn parse_row(text: &str, channels: usize) -> Result<Vec<Cell>, String> {
    let mut cells = if text.trim().is_empty() {
        Vec::new()
    } else {
        text.split('|').map(parse_cell).collect::<Result<Vec<_>, _>>()?
    };
    if cells.len() > channels {
        return Err(format!("{} cells but the song has {channels} channels", cells.len()));
    }
    cells.resize(channels, Cell::default());
    Ok(cells)
}

// ── Song JSON ────────────────────────────────────────────────────

fn parse_instrument(o: &Value, index: usize, issues: &mut Vec<Issue>) -> Instrument {
    let mut d = Instrument::new(&format!("Instrument {}", index + 1), Wave::Pulse);
    let mut err = |message: String| issues.push(Issue { severity: Severity::Error, message });
    let Some(r) = o.as_object() else {
        err(format!("instrument {} is not an object", index + 1));
        return d;
    };
    if let Some(w) = r.get("wave") {
        match w.as_str().and_then(Wave::parse) {
            Some(wave) => d.wave = wave,
            None => err(format!("instrument {}: unknown wave \"{}\"", index + 1, js_string(w))),
        }
    }
    if let Some(Value::Array(t)) = r.get("table") {
        let ok = t.len() == 32 && t.iter().all(|x| is_integer(x) && (0.0..=15.0).contains(&x.as_f64().unwrap_or(-1.0)));
        if ok {
            for (slot, x) in d.table.iter_mut().zip(t) {
                *slot = x.as_f64().unwrap_or(0.0);
            }
        } else {
            err(format!("instrument {}: table must be 32 integers 0–15", index + 1));
        }
    }
    // `typeof v === 'object'` takes an array too, with every field defaulted.
    d.vibrato = match r.get("vibrato") {
        Some(v @ (Value::Object(_) | Value::Array(_))) => Some(Vibrato {
            depth: num(v.get("depth"), 0.0, 0.0, 24.0),
            rate: num(v.get("rate"), 5.0, 0.0, 50.0),
            delay: num(v.get("delay"), 0.0, 0.0, 10.0),
        }),
        _ => None,
    };
    if let Some(name) = r.get("name").and_then(Value::as_str) {
        d.name = name.to_string();
    }
    d.duty = num(r.get("duty"), d.duty, 0.01, 0.99);
    d.periodic = r.get("periodic") == Some(&Value::Bool(true));
    d.volume = num(r.get("volume"), d.volume, 0.0, 1.0);
    d.attack = num(r.get("attack"), d.attack, 0.0, 10.0);
    d.decay = num(r.get("decay"), d.decay, 0.0, 10.0);
    d.sustain = num(r.get("sustain"), d.sustain, 0.0, 1.0);
    d.release = num(r.get("release"), d.release, 0.0, 10.0);
    d.pan = num(r.get("pan"), d.pan, -1.0, 1.0);
    d.transpose = js_round(num(r.get("transpose"), d.transpose, -48.0, 48.0));
    d.slide = num(r.get("slide"), d.slide, -10000.0, 10000.0);
    if let Some(Value::Array(a)) = r.get("arp") {
        d.arp = a.iter().filter_map(Value::as_f64).map(js_round).collect();
    }
    d.cutoff = num(r.get("cutoff"), d.cutoff, 0.0, 22050.0);
    d
}

/// How JavaScript's `String(v)` spells a value, for messages that match jSuite's.
fn js_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Parse a `.song` document. Collects every problem rather than stopping at
/// the first, and fails only if one is an error - so a song with an unknown
/// effect still loads, with the warning returned beside it.
pub fn parse_song(json: &Value) -> Result<(Song, Vec<Issue>), SongError> {
    let Some(o) = json.as_object() else {
        return Err(SongError {
            issues: vec![Issue { severity: Severity::Error, message: "a .song must be a JSON object".into() }],
        });
    };
    let mut issues = Vec::new();
    let song = read_song(o, &mut issues);
    if issues.iter().any(|i| i.severity == Severity::Error) {
        return Err(SongError { issues });
    }
    Ok((song, issues))
}

fn read_song(o: &Map<String, Value>, issues: &mut Vec<Issue>) -> Song {
    let err = |issues: &mut Vec<Issue>, message: String| issues.push(Issue { severity: Severity::Error, message });

    match o.get("version") {
        None => {}
        Some(v) if v.as_f64() == Some(FORMAT_VERSION) => {}
        Some(v) => match v.as_f64() {
            Some(n) if n > FORMAT_VERSION => {
                err(issues, format!("version {} is newer than this player ({FORMAT_VERSION})", js_string(v)))
            }
            _ => err(issues, format!("unknown version {}", js_string(v))),
        },
    }

    let channels = js_round(num(o.get("channels"), 4.0, 1.0, MAX_CHANNELS as f64)) as usize;
    if let Some(c) = o.get("channels")
        && c.as_f64() != Some(channels as f64)
    {
        err(issues, format!("channels must be 1–{MAX_CHANNELS}"));
    }

    let instruments: Vec<Instrument> = match o.get("instruments") {
        Some(Value::Array(a)) => a.iter().enumerate().map(|(i, x)| parse_instrument(x, i, issues)).collect(),
        _ => Vec::new(),
    };
    if instruments.is_empty() {
        err(issues, "the song has no instruments".into());
    }
    if instruments.len() > 255 {
        err(issues, "at most 255 instruments".into());
    }

    let empty = |rows: usize| Pattern { rows: vec![vec![Cell::default(); channels]; rows] };
    let mut patterns = Vec::new();
    match o.get("patterns") {
        Some(Value::Array(ps)) if !ps.is_empty() => {
            for (pi, p) in ps.iter().enumerate() {
                let rows_json = match p.get("rows") {
                    Some(Value::Array(r)) if !r.is_empty() && r.len() <= MAX_ROWS => r,
                    _ => {
                        err(issues, format!("pattern {pi}: rows must be 1–{MAX_ROWS} strings"));
                        patterns.push(empty(64));
                        continue;
                    }
                };
                let mut rows = Vec::with_capacity(rows_json.len());
                for (ri, line) in rows_json.iter().enumerate() {
                    let Some(line) = line.as_str() else {
                        err(issues, format!("pattern {pi} row {ri}: not a string"));
                        rows.push(vec![Cell::default(); channels]);
                        continue;
                    };
                    match parse_row(line, channels) {
                        Ok(row) => {
                            for c in &row {
                                if let Some(inst) = c.inst
                                    && inst >= instruments.len()
                                {
                                    err(issues, format!("pattern {pi} row {ri}: instrument {} does not exist", hex2(inst + 1)));
                                }
                                if let Some(fx) = c.fx
                                    && !EFFECTS.contains(&fx)
                                {
                                    issues.push(Issue {
                                        severity: Severity::Warning,
                                        message: format!(
                                            "pattern {pi} row {ri}: effect {} is not supported and will be ignored",
                                            fx as char
                                        ),
                                    });
                                }
                            }
                            rows.push(row);
                        }
                        Err(e) => {
                            err(issues, format!("pattern {pi} row {ri}: {e}"));
                            rows.push(vec![Cell::default(); channels]);
                        }
                    }
                }
                patterns.push(Pattern { rows });
            }
        }
        _ => err(issues, "the song has no patterns".into()),
    }

    let order_json: Vec<f64> = match o.get("order") {
        Some(Value::Array(a)) => a.iter().filter(|x| is_integer(x)).filter_map(Value::as_f64).collect(),
        _ => Vec::new(),
    };
    if order_json.is_empty() {
        err(issues, "the order list is empty".into());
    }
    let mut order = Vec::with_capacity(order_json.len());
    for p in order_json {
        if p < 0.0 || p >= patterns.len() as f64 {
            err(issues, format!("order refers to pattern {p}, which does not exist"));
        } else {
            order.push(p as usize);
        }
    }

    let mut loop_to = None;
    if let Some(v) = o.get("loop")
        && let Some(n) = v.as_f64()
    {
        if n.fract() == 0.0 && n >= 0.0 && n < order.len() as f64 {
            loop_to = Some(n as usize);
        } else {
            err(issues, format!("loop {} is outside the order list", js_string(v)));
        }
    }

    Song {
        name: o.get("name").and_then(Value::as_str).unwrap_or("Untitled Song").to_string(),
        bpm: num(o.get("bpm"), 120.0, 20.0, 999.0),
        rows_per_beat: js_round(num(o.get("rows_per_beat"), 4.0, 1.0, 16.0)),
        speed: js_round(num(o.get("speed"), 6.0, 1.0, 31.0)),
        channels,
        volume: num(o.get("volume"), 0.5, 0.0, 1.0),
        instruments,
        patterns,
        order,
        loop_to,
    }
}

/// Parse `.song` text - a JSON syntax error is a [`SongError`] too.
pub fn parse_song_text(text: &str) -> Result<(Song, Vec<Issue>), SongError> {
    let json: Value = serde_json::from_str(text).map_err(|e| SongError {
        issues: vec![Issue { severity: Severity::Error, message: format!("not valid JSON: {e}") }],
    })?;
    parse_song(&json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_read_as_jsuite_reads_them() {
        assert_eq!(parse_note_name("C-4"), Some(60));
        assert_eq!(parse_note_name("A-4"), Some(69));
        assert_eq!(parse_note_name("c#4"), Some(61));
        assert_eq!(parse_note_name("Db4"), Some(61));
        assert_eq!(parse_note_name("B4"), Some(71));
        assert_eq!(parse_note_name("C-0"), Some(12));
        assert_eq!(parse_note_name("---"), None);
        assert_eq!(parse_note_name("H-4"), None);
    }

    #[test]
    fn cells_take_short_forms() {
        let c = parse_cell("C-4 01").unwrap();
        assert_eq!((c.note, c.inst, c.vol, c.fx), (Some(60), Some(0), None, None));
        let c = parse_cell("... .. 20 a0f").unwrap();
        assert_eq!((c.note, c.vol, c.fx, c.arg), (None, Some(32.0), Some(b'A'), 0x0f));
        assert_eq!(parse_cell("===").unwrap().note, Some(NOTE_OFF));
        assert_eq!(parse_cell("^^^").unwrap().note, Some(NOTE_CUT));
        assert!(parse_cell("C-4 00").is_err());
        assert!(parse_cell("C-4 01 41").is_err());
        assert!(parse_cell("C-4 01 .. ... x").is_err());
        assert_eq!(parse_row("", 3).unwrap().len(), 3);
        assert!(parse_row("C-4 | C-4", 1).is_err());
    }

    #[test]
    fn a_document_reports_every_problem() {
        let json = serde_json::json!({
            "version": 1,
            "channels": 1,
            "instruments": [{ "wave": "organ" }],
            "patterns": [{ "rows": ["C-4 02", "C-4 01 .. Z00"] }],
            "order": [0, 3],
            "loop": 5,
        });
        let e = parse_song(&json).unwrap_err();
        let text = e.to_string();
        assert!(text.contains("unknown wave \"organ\""), "{text}");
        assert!(text.contains("instrument 02 does not exist"), "{text}");
        assert!(text.contains("order refers to pattern 3"), "{text}");
        assert!(text.contains("loop 5 is outside"), "{text}");
        assert!(e.issues.iter().any(|i| i.severity == Severity::Warning && i.message.contains("effect Z")));
    }

    #[test]
    fn an_unknown_effect_is_only_a_warning() {
        let json = serde_json::json!({
            "version": 1, "channels": 1,
            "instruments": [{ "wave": "sine" }],
            "patterns": [{ "rows": ["C-4 01 .. Z00"] }],
            "order": [0],
        });
        let (song, issues) = parse_song(&json).unwrap();
        assert_eq!(song.loop_to, None);
        assert_eq!(issues.len(), 1);
        assert!(parse_song_text(r#"{"version": 2}"#).unwrap_err().to_string().contains("newer"));
    }
}
