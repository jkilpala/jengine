//! Native audio decodes the formats the engine documents: PCM WAV above all,
//! since that is what every sound generator here writes.
//!
//! kira's `wav` and `ogg` features are containers only. With `pcm` and
//! `vorbis` left out, symphonia found the stream and refused it -
//! "unsupported feature: core (codec): unsupported audio codec" - so on
//! native every `play_sound` of a WAV logged a warning and played nothing,
//! while the editor, which decodes with the browser, played it. Found when
//! a space shooter ran natively in silence (BUGS #104). The editor
//! is where sounds get checked, so nothing had noticed.
#![cfg(feature = "audio")]

use kira::sound::static_sound::StaticSoundData;
use std::io::Cursor;

fn pcm_wav(samples: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

#[test]
fn a_16_bit_pcm_wav_decodes() {
    let samples: Vec<i16> = (0..2205).map(|i| if (i / 25) % 2 == 0 { 8000 } else { -8000 }).collect();
    let sound = StaticSoundData::from_cursor(Cursor::new(pcm_wav(&samples, 22050))).expect("PCM WAV decodes");
    assert_eq!(sound.frames.len(), samples.len());
}

#[test]
fn every_shipped_sound_decodes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects");
    let (mut n, mut oggs) = (0, 0);
    for project in ["engine-test-lab", "visual-showcase"] {
        let dir = root.join(project).join("assets/sounds");
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "wav" || e == "ogg") {
                let bytes = std::fs::read(&path).unwrap();
                let sound = StaticSoundData::from_cursor(Cursor::new(bytes))
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert!(sound.duration().as_secs_f64() > 0.0, "{} is empty", path.display());
                if path.extension().is_some_and(|e| e == "ogg") {
                    oggs += 1;
                }
                n += 1;
            }
        }
    }
    assert!(n > 10, "found only {n} sounds");
    assert!(oggs >= 1, "no Ogg Vorbis sound to decode");
}
