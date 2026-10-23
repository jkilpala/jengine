//! The player against jSuite's: the digests in `tests/data/` were rendered
//! by jSuite's `synth.ts` (copied from `src/lib/tracker/fixtures/`), and the
//! rest is its `synth.test.ts` ported - timing, pitch and every effect.
//! Re-copy the `.song` and `.digest.json` files when jSuite's player changes.

use jengine_tracker::digest::{compare, digest, Digest};
use jengine_tracker::song::{Cell, Instrument, Pattern, Song, Wave, parse_row};
use jengine_tracker::synth::{Sequencer, note_hz, render};
use jengine_tracker::{RATE, parse_song_text};
use std::path::PathBuf;

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)
}

fn load(name: &str) -> Song {
    parse_song_text(&std::fs::read_to_string(data(name)).unwrap()).unwrap().0
}

const PINNED: &[&str] = &["effects", "sunny-fields", "deep-cavern", "victory"];

#[test]
fn every_pinned_song_matches_its_jsuite_digest() {
    let mut failures = Vec::new();
    for name in PINNED {
        let r = render(&load(&format!("{name}.song")), RATE);
        let actual = digest(&r.left, &r.right, RATE, r.loop_start);
        let expected: Digest =
            serde_json::from_str(&std::fs::read_to_string(data(&format!("{name}.digest.json"))).unwrap()).unwrap();
        // The tolerances the format promises a port: timing exact, RMS
        // within 20e-5, crossings within 2 a window.
        let diff = compare(&expected, &actual, 20, 2);
        if !diff.is_empty() {
            failures.push(format!("{name}: {} mismatches, first {:?}", diff.len(), &diff[..diff.len().min(5)]));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_same_samples_however_the_buffers_are_sliced() {
    let s = load("effects.song");
    let whole = render(&s, RATE);
    let mut seq = Sequencer::new(&s, RATE);
    let mut l = vec![0f32; whole.left.len()];
    let mut r = vec![0f32; whole.left.len()];
    let sizes = [1, 7, 128, 33, 4096, 999];
    let mut at = 0;
    let mut k = 0;
    while !seq.done && at < l.len() {
        let want = sizes[k % sizes.len()].min(l.len() - at);
        at += seq.render(&mut l[at..at + want], &mut r[at..at + want]);
        k += 1;
    }
    assert_eq!(at, whole.left.len());
    assert!(l == whole.left && r == whole.right);
}

/// A one-channel song with the given rows, at 10 rows a second.
fn song(rows: &[&str], wave: Wave) -> Song {
    let inst = Instrument { attack: 0.001, sustain: 1.0, decay: 0.001, release: 0.001, ..Instrument::new("x", wave) };
    Song {
        name: "test".into(),
        bpm: 150.0,
        rows_per_beat: 4.0,
        speed: 6.0,
        channels: 1,
        volume: 1.0,
        instruments: vec![inst],
        patterns: vec![Pattern { rows: rows.iter().map(|r| parse_row(r, 1).unwrap()).collect() }],
        order: vec![0],
        loop_to: None,
    }
}

fn sine(rows: &[&str]) -> Song {
    song(rows, Wave::Sine)
}

fn onset(buf: &[f32], threshold: f32) -> Option<usize> {
    buf.iter().position(|s| s.abs() > threshold)
}

fn rms(buf: &[f32], from: usize, to: usize) -> f64 {
    (buf[from..to].iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>() / (to - from) as f64).sqrt()
}

/// Rising zero crossings a second - the pitch of a clean tone.
fn pitch_of(buf: &[f32], from: usize, to: usize) -> f64 {
    let c = (from + 1..to).filter(|&i| buf[i - 1] < 0.0 && buf[i] >= 0.0).count();
    c as f64 * f64::from(RATE) / (to - from) as f64
}

const ROW: usize = 4410;
const TICK: usize = 735;

#[test]
fn each_row_starts_on_the_frame_the_tempo_says() {
    let r = render(&sine(&["", "", "C-4 01", ""]), RATE);
    let start = onset(&r.left, 0.05).unwrap();
    assert!((2 * ROW..2 * ROW + 44).contains(&start), "{start}");
}

#[test]
fn a_note_plays_the_pitch_it_names() {
    let r = render(&sine(&["A-4 01", "", "", "", ""]), RATE);
    assert!((pitch_of(&r.left, ROW, 4 * ROW) - 440.0).abs() < 5.0);
    assert_eq!(note_hz(69.0), 440.0);
    assert!((note_hz(60.0) - 261.63).abs() < 0.01);
}

#[test]
fn note_off_releases_and_note_cut_silences() {
    let off = render(&sine(&["A-4 01", "===", "", ""]), RATE);
    assert!(rms(&off.left, ROW + 500, 2 * ROW) < 0.001);
    let cut = render(&sine(&["A-4 01", "^^^", "", ""]), RATE);
    assert!(rms(&cut.left, ROW + 500, 2 * ROW) < 0.001);
}

#[test]
fn edx_delays_a_note_and_ecx_cuts_it() {
    let delayed = render(&sine(&["", "A-4 01 .. ED3", "", ""]), RATE);
    let start = onset(&delayed.left, 0.05).unwrap();
    assert!((ROW + 3 * TICK..ROW + 3 * TICK + 40).contains(&start), "{start}");

    let cut = render(&sine(&["A-4 01 .. EC2", "", ""]), RATE);
    assert!(rms(&cut.left, 2 * TICK + 400, ROW) < 0.001);
    assert!(rms(&cut.left, 0, 2 * TICK) > 0.3);
}

#[test]
fn slides_move_the_pitch_and_portamento_glides_to_a_note() {
    let up = render(&sine(&["A-4 01 .. 110", "... .. .. 110", "... .. .. 110", "", "", "", "", ""]), RATE);
    // One semitone a tick, five effect ticks a row, three rows: +15.
    let want = note_hz(69.0 + 15.0);
    assert!((pitch_of(&up.left, 3 * ROW, 8 * ROW) - want).abs() < want * 0.02);

    let glide = render(&sine(&["A-4 01", "A-5 .. .. 330", "... .. .. 300", "", ""]), RATE);
    assert!((pitch_of(&glide.left, 2 * ROW + 200, 4 * ROW) - 880.0).abs() < 10.0);
}

#[test]
fn fxx_changes_the_tempo_from_its_own_row() {
    // F78 = 120 BPM: 8 rows a second, 5512.5 frames a row.
    let r = render(&sine(&["A-4 01 .. F78", "", "===", ""]), RATE);
    let release = 2 * 5512;
    assert!(rms(&r.left, release - 1000, release - 100) > 0.3);
    assert!(rms(&r.left, release + 400, release + 2000) < 0.001);
}

#[test]
fn a_song_reports_where_it_loops() {
    let mut s = sine(&["A-4 01", "", "", ""]);
    s.patterns.push(Pattern { rows: vec![vec![Cell::default()]; 4] });
    s.order = vec![0, 1];
    s.loop_to = Some(1);
    let r = render(&s, RATE);
    assert_eq!(r.loop_start, Some(4 * ROW as u64));
    assert_eq!(r.left.len(), 8 * ROW);
}

#[test]
fn a_song_with_no_loop_ends_once_its_notes_die_away() {
    let r = render(&sine(&["A-4 01", "==="]), RATE);
    assert_eq!(r.loop_start, None);
    assert!((2 * ROW..2 * ROW + 2000).contains(&r.left.len()), "{}", r.left.len());
}

#[test]
fn eight_xx_pans() {
    let r = render(&sine(&["A-4 01 .. 800", "", ""]), RATE);
    assert!(rms(&r.left, 1000, 8000) > 0.5);
    assert!(rms(&r.right, 1000, 8000) < 0.01);
}

#[test]
fn every_wave_renders_without_nan() {
    for wave in [Wave::Pulse, Wave::Triangle, Wave::Saw, Wave::Sine, Wave::Noise, Wave::Wave] {
        for note in ["C-0", "C-4", "C-9"] {
            let mut s = song(&[&format!("{note} 01"), "", "==="], wave);
            s.instruments[0].cutoff = 3000.0;
            let r = render(&s, RATE);
            assert!(r.left.iter().chain(&r.right).all(|v| v.is_finite()), "{wave:?} {note}");
            assert!(rms(&r.left, 0, ROW) > 0.01, "{wave:?} {note} is silent");
        }
    }
}

#[test]
fn every_demo_stays_below_full_scale() {
    for name in PINNED {
        let r = render(&load(&format!("{name}.song")), RATE);
        let mut over = 0;
        let mut peak = 0f32;
        for (l, r) in r.left.iter().zip(&r.right) {
            let v = l.abs().max(r.abs());
            peak = peak.max(v);
            if v > 0.8 {
                over += 1;
            }
        }
        assert!(peak < 1.0, "{name}: peak {peak}");
        assert!((over as f64) / (r.left.len() as f64) < 0.001, "{name}: limiter busy");
    }
}
